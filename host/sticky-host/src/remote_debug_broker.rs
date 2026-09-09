//! Unix-socket broker that owns one remote-debug [`Session`].
//!
//! CLI (and later `--mcp`) processes send one length-prefixed JSON request
//! and wait for one reply. The broker serializes GATT so two snapshot or
//! inject clients cannot interleave ATT chunks. Client hangup does **not**
//! drop the link; [`BrokerRequest::Disconnect`] or serve exit does.
//!
//! Socket path is `$XDG_RUNTIME_DIR/sticky-rs/remote-debug-<name>.sock`
//! (same runtime-dir family as the UART flock; `/run/user/<uid>/sticky-rs`
//! when `XDG_RUNTIME_DIR` is unset). The key is the advertise name
//! (`sticky-rs`), never a MAC. This module is clap-free.
//!
//! `connect` starts pair on a worker and returns `pairing`. Agents poll
//! [`BrokerRequest::Status`]. Inject / snapshot need `connected`.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use panel_view::{FrameKind, TouchSample, TouchSource};
use remote_debug_host::{Session, Transport, DEFAULT_ADV_NAME};
use remote_debug_wire::v1::{ProductKey, TouchPhase};
use serde::{Deserialize, Serialize};

use crate::original::Layout;
use crate::uart_lock::default_lock_dir;
use crate::Error;

/// Printed when the socket is missing or the peer is gone.
pub const NO_BROKER: &str = "no broker; run connect or serve";

/// Max JSON body (gray4 planes plus envelope).
const MAX_FRAME: usize = 8 * 1024 * 1024;
/// How long a client waits for one reply.
///
/// `connect` returns `pairing` without waiting for BlueZ. Snapshot
/// notify budget is 30s.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(120);
/// How long [`ensure_broker`] waits for the child to bind.
const SPAWN_WAIT: Duration = Duration::from_secs(5);

/// One RPC from a CLI or MCP client. Not clap types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum BrokerRequest {
    /// Pair (UART auto-PIN or a supplied PIN) and hold GATT.
    Connect(ConnectReq),
    /// Synthetic framebuffer tap.
    InjectTouch {
        /// Pre-rotation framebuffer X.
        x: u16,
        /// Pre-rotation framebuffer Y.
        y: u16,
        /// Optional GT911 slot 0..=4.
        slot: Option<u8>,
        /// `down` / `move` / `up`. Unset is a tap.
        #[serde(default)]
        phase: Option<String>,
        /// `x`/`y` are page pixels for the last compose hold.
        #[serde(default)]
        page: bool,
    },
    /// Synthetic product-key edge.
    InjectButton {
        /// `ok` / `page-up` / `page-down`.
        key: String,
        /// Press (`true`) or release.
        down: bool,
    },
    /// Arm the frozen LAST planes.
    GetSnapshot {
        /// Host nonce. Omit to use a time-based value.
        nonce: Option<u64>,
    },
    /// Release the armed nonce.
    SnapshotAck {
        /// Nonce from the last get when omitted.
        nonce: Option<u64>,
    },
    /// Operator abort (no nonce).
    SnapshotClear,
    /// Whether the broker holds a session.
    Status,
    /// Software-reset the embedded MCU (not this host).
    Reboot(RebootReq),
    /// Drop GATT and stop the broker.
    Disconnect,
}

/// Arguments for [`BrokerRequest::Connect`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectReq {
    /// Six-digit PIN (skip UART).
    pub pin: Option<u32>,
    /// Serial device. Also `ESPFLASH_PORT`.
    pub port: Option<String>,
    /// Advertise name (socket key). Never a MAC.
    pub name: String,
    /// Keep the BlueZ bond; write this unit into the allowlist.
    pub remember: bool,
}

/// Arguments for [`BrokerRequest::Reboot`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebootReq {
    /// Do not Connect again after the MCU reset.
    pub no_reconnect: bool,
    /// Six-digit PIN for reconnect (skip UART).
    pub pin: Option<u32>,
    /// Serial device for reconnect auto-PIN.
    pub port: Option<String>,
    /// Advertise name.
    pub name: String,
    /// Keep the BlueZ bond after the new pair.
    pub remember: bool,
}

/// Session meter for [`BrokerRequest::Status`] / [`BrokerRequest::Connect`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrokerPhase {
    /// No GATT and no in-flight pair.
    #[default]
    Disconnected,
    /// Opener worker is running (BlueZ / UART).
    Pairing,
    /// Broker holds GATT.
    Connected,
}

/// One RPC reply. Snapshot plane bytes travel here; the caller writes files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerReply {
    /// Command succeeded.
    pub ok: bool,
    /// Human line (no MAC).
    pub message: String,
    /// Last snapshot nonce, if any.
    pub nonce: Option<u64>,
    /// Whether the broker holds GATT.
    pub connected: bool,
    /// `disconnected` / `pairing` / `connected`. Default for old readers.
    #[serde(default)]
    pub phase: BrokerPhase,
    /// Planes from a successful get (caller writes `developer-data/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotWire>,
    /// Last Target / Scene `LogLine` (never a MAC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_log: Option<String>,
}

/// LAST planes on the wire (not a file path).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotWire {
    /// Host nonce that armed the slot.
    pub nonce: u64,
    /// Packed width.
    pub width: u16,
    /// Packed height.
    pub height: u16,
    /// `mono` or `gray4`.
    pub kind: String,
    /// Product hold token.
    pub hold: Option<u32>,
    /// `Scene::persist_byte`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<u32>,
    /// Targets walk id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_step: Option<u32>,
    /// Wire `TargetKind` discriminant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<u32>,
    /// Expected page X.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_expect_x: Option<u32>,
    /// Expected page Y.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_expect_y: Option<u32>,
    /// Black/white plane.
    pub bw: Vec<u8>,
    /// Red/gray plane when gray4.
    pub red: Option<Vec<u8>>,
}

/// Options for [`serve_with`].
#[derive(Debug, Clone, Copy)]
pub struct ServeOpts<'a> {
    /// Directory for the socket and pid file.
    pub dir: &'a Path,
    /// Advertise name (socket stem). Never a MAC.
    pub name: &'a str,
    /// Install a SIGINT handler that disconnects and exits.
    pub install_ctrlc: bool,
    /// Log `connected` / `snapshot` / errors on stderr (never a MAC).
    pub log: bool,
    /// Remember-me allowlist root. `None` skips the write (tests).
    pub layout: Option<&'a Layout>,
}

struct Inner<T: Transport> {
    session: Option<Session<T>>,
    last_nonce: Option<u64>,
    pairing: bool,
    /// Bumped to discard a late opener result after disconnect / new pair.
    pair_gen: u64,
    last_error: Option<String>,
}

struct BrokerState<T: Transport, F> {
    inner: Mutex<Inner<T>>,
    opener: Mutex<F>,
    shutdown: AtomicBool,
    socket_path: PathBuf,
    log: bool,
    layout: Option<Layout>,
}

/// Runtime dir for the broker socket (same family as the UART flock).
#[must_use]
pub fn broker_runtime_dir() -> PathBuf {
    default_lock_dir()
}

/// `$dir/remote-debug-<sanitized-name>.sock`.
#[must_use]
pub fn broker_socket_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{}.sock", socket_stem(name)))
}

/// Pid sidecar next to the socket.
#[must_use]
pub fn broker_pid_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{}.pid", socket_stem(name)))
}

/// Append-only log for a detached `serve` (`connect` auto-start).
#[must_use]
pub fn broker_log_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{}.log", socket_stem(name)))
}

/// On-disk `target/debug/xtask` when present; otherwise `current_exe`.
///
/// A stdio MCP process that outlives `cargo build -p xtask` has a
/// deleted inode; spawning that path is `ENOENT`.
#[must_use]
pub fn resolve_broker_exe(repo_root: &Path, current_exe: &Path) -> PathBuf {
    let on_disk = repo_root.join("target").join("debug").join("xtask");
    if on_disk.is_file() {
        on_disk
    } else {
        current_exe.to_path_buf()
    }
}

fn no_broker_at(path: &Path) -> Error {
    Error::RemoteDebug(format!("{NO_BROKER} ({})", path.display()))
}

fn socket_stem(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let slug = if slug.is_empty() {
        DEFAULT_ADV_NAME.to_string()
    } else {
        slug
    };
    format!("remote-debug-{slug}")
}

/// Product key from `ok` / `page-up` / `page-down`.
///
/// # Errors
///
/// Unknown token.
pub fn parse_product_key(raw: &str) -> Result<ProductKey, Error> {
    match raw.to_ascii_lowercase().as_str() {
        "ok" | "4" => Ok(ProductKey::PRODUCT_KEY_OK),
        "page-up" | "pageup" | "5" => Ok(ProductKey::PRODUCT_KEY_PAGE_UP),
        "page-down" | "pagedown" | "6" => Ok(ProductKey::PRODUCT_KEY_PAGE_DOWN),
        _ => Err(Error::RemoteDebug(
            "key must be ok, page-up, or page-down".into(),
        )),
    }
}

/// `down` / `move` / `up`. Unset is a tap.
fn parse_touch_phase(raw: Option<&str>) -> Result<TouchPhase, Error> {
    match raw.map(str::to_ascii_lowercase).as_deref() {
        None | Some("down") | Some("tap") => Ok(TouchPhase::TOUCH_PHASE_DOWN),
        Some("move") => Ok(TouchPhase::TOUCH_PHASE_MOVE),
        Some("up") => Ok(TouchPhase::TOUCH_PHASE_UP),
        Some(_) => Err(Error::RemoteDebug("phase must be down, move, or up".into())),
    }
}

/// Send one request and wait for one reply.
///
/// # Errors
///
/// Missing broker, I/O, or JSON.
pub fn rpc(dir: &Path, name: &str, request: &BrokerRequest) -> Result<BrokerReply, Error> {
    let path = broker_socket_path(dir, name);
    if !path.exists() {
        return Err(no_broker_at(&path));
    }
    let mut stream = UnixStream::connect(&path).map_err(|error| map_connect(&path, error))?;
    stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
    write_msg(&mut stream, request)?;
    read_msg(&mut stream)
}

fn map_connect(path: &Path, error: std::io::Error) -> Error {
    match error.kind() {
        std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::NotFound
        | std::io::ErrorKind::ConnectionReset => no_broker_at(path),
        _ if path.exists() => Error::RemoteDebug(format!(
            "broker socket {} exists but connect failed: {error}",
            path.display()
        )),
        _ => error.into(),
    }
}

/// Unlink a leftover socket when the peer pid is dead, then spawn `serve`
/// if nothing is listening.
///
/// `exe` is the same xtask binary (`remote-debug serve --name …`).
///
/// # Errors
///
/// Spawn failure or the child never bound.
pub fn ensure_broker(dir: &Path, name: &str, exe: &Path) -> Result<(), Error> {
    reclaim_stale(dir, name)?;
    if broker_listening(dir, name) {
        return Ok(());
    }
    if !exe.is_file() {
        return Err(Error::RemoteDebug(format!(
            "cannot spawn serve; no xtask at {}",
            exe.display()
        )));
    }
    fs::create_dir_all(dir)?;
    let log_path = broker_log_path(dir, name);
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| {
            Error::RemoteDebug(format!(
                "cannot open broker log {}: {error}",
                log_path.display()
            ))
        })?;
    let log_err = log.try_clone().map_err(|error| {
        Error::RemoteDebug(format!(
            "cannot clone broker log {}: {error}",
            log_path.display()
        ))
    })?;
    let mut cmd = Command::new(exe);
    cmd.args(["remote-debug", "serve", "--name", name]);
    cmd.arg("--socket-dir").arg(dir);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::from(log));
    cmd.stderr(Stdio::from(log_err));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map_err(|error| {
        Error::RemoteDebug(format!(
            "cannot spawn serve from {}: {error}",
            exe.display()
        ))
    })?;
    wait_for_broker(dir, name, SPAWN_WAIT)
}

/// Poll until the pid file is live or `budget` elapses.
///
/// # Errors
///
/// Timeout.
pub fn wait_for_broker(dir: &Path, name: &str, budget: Duration) -> Result<(), Error> {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if broker_listening(dir, name) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(no_broker_at(&broker_socket_path(dir, name)))
}

fn broker_listening(dir: &Path, name: &str) -> bool {
    let sock = broker_socket_path(dir, name);
    let pid_path = broker_pid_path(dir, name);
    sock.exists() && read_pid(&pid_path).is_some_and(pid_alive)
}

fn reclaim_stale(dir: &Path, name: &str) -> Result<(), Error> {
    let sock = broker_socket_path(dir, name);
    let pid_path = broker_pid_path(dir, name);
    if broker_listening(dir, name) {
        return Ok(());
    }
    if sock.exists() {
        if let Some(pid) = read_pid(&pid_path) {
            if pid_alive(pid) {
                return Ok(());
            }
        }
        let _ = fs::remove_file(&sock);
        let _ = fs::remove_file(&pid_path);
    } else if pid_path.exists() && read_pid(&pid_path).is_none_or(|pid| !pid_alive(pid)) {
        let _ = fs::remove_file(&pid_path);
    }
    Ok(())
}

fn read_pid(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    text.trim().parse().ok()
}

fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn write_pid(path: &Path) -> Result<(), Error> {
    fs::write(path, format!("{}\n", std::process::id()))?;
    Ok(())
}

/// Foreground live owner (Linux BlueZ). Blocks until disconnect or Ctrl-C.
///
/// # Errors
///
/// Bind failure, BlueZ, or UART scrape.
pub fn serve_live(layout: &Layout, name: &str, socket_dir: Option<&Path>) -> Result<(), Error> {
    let default = broker_runtime_dir();
    let dir = socket_dir.unwrap_or(&default);
    fs::create_dir_all(dir)?;
    serve_live_inner(ServeOpts {
        dir,
        name,
        install_ctrlc: true,
        log: true,
        layout: Some(layout),
    })
}

fn serve_live_inner(opts: ServeOpts<'_>) -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    {
        serve_with(opts, open_live_session)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = opts;
        Err(Error::RemoteDebug(
            "remote-debug serve needs Linux BlueZ".into(),
        ))
    }
}

/// Pair and open a [`remote_debug_host::BluerTransport`] session.
///
/// UART lock is held only while scraping a new `pair pin=`, then released.
///
/// # Errors
///
/// Port, lock, PIN timeout, or BlueZ.
#[cfg(target_os = "linux")]
pub fn open_live_session(
    req: &ConnectReq,
) -> Result<Session<remote_debug_host::BluerTransport>, Error> {
    use std::sync::mpsc;

    use remote_debug_host::{connect, connect_with, ChannelPasskey, FixedPasskey, PasskeySource};

    use crate::detect;
    use crate::uart_lock::try_acquire;
    use crate::wait_new_pair_pin;

    let window = Duration::from_secs(remote_debug_host::PAIR_WINDOW_SECS);
    let transport = if let Some(pin) = req.pin {
        let passkey: std::sync::Arc<dyn PasskeySource> =
            std::sync::Arc::new(FixedPasskey::new(pin));
        connect(&req.name, passkey).map_err(map_ble)?
    } else {
        let port = req.port.clone();
        let (tx, rx) = mpsc::channel();
        let (start_tx, start_rx) = mpsc::channel();
        thread::spawn(move || {
            if start_rx.recv_timeout(window).is_err() {
                return;
            }
            let opened = match detect::resolve_sticky_port(port.clone()) {
                Ok(resolved) => try_acquire(&resolved, "remote-debug").and_then(|uart| {
                    crate::cdc_listen::CdcListen::open(&resolved).map(|cdc| (cdc, uart))
                }),
                // `cdc-acm` unbound after a listen skipped Drop: usbfs still
                // sees the unique QinHeng. Do not require a replug for auto-PIN.
                Err(Error::MissingStickyUart) if port.is_none() => {
                    crate::cdc_listen::CdcListen::open_unique_locked(|path| {
                        try_acquire(path, "remote-debug")
                    })
                }
                Err(_) => return,
            };
            let Ok((mut cdc, uart)) = opened else {
                return;
            };
            let _uart = uart;
            let mut seen = Vec::new();
            if let Ok(pin) = wait_new_pair_pin(&mut cdc, &mut seen, window) {
                let _ = tx.send(pin);
            }
        });
        let passkey: std::sync::Arc<dyn PasskeySource> =
            std::sync::Arc::new(ChannelPasskey::new(rx, window));
        connect_with(&req.name, passkey, move || {
            let _ = start_tx.send(());
        })
        .map_err(map_ble)?
    };
    Ok(Session::new(transport, req.remember))
}

/// Accept clients until [`BrokerRequest::Disconnect`] or Ctrl-C.
///
/// `opener` builds a [`Session`] (tests pass [`remote_debug_host::FakeTransport`]).
///
/// # Errors
///
/// Bind, I/O, or opener failure on connect.
pub fn serve_with<T, F>(opts: ServeOpts<'_>, opener: F) -> Result<(), Error>
where
    T: Transport + Send + 'static,
    F: FnMut(&ConnectReq) -> Result<Session<T>, Error> + Send + 'static,
{
    fs::create_dir_all(opts.dir)?;
    reclaim_stale(opts.dir, opts.name)?;
    let socket_path = broker_socket_path(opts.dir, opts.name);
    let pid_path = broker_pid_path(opts.dir, opts.name);
    if broker_listening(opts.dir, opts.name) {
        return Err(Error::RemoteDebug("already serving".into()));
    }
    let listener = match UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            reclaim_stale(opts.dir, opts.name)?;
            UnixListener::bind(&socket_path)?
        }
        Err(error) => return Err(error.into()),
    };
    write_pid(&pid_path)?;
    let state = Arc::new(BrokerState {
        inner: Mutex::new(Inner {
            session: None,
            last_nonce: None,
            pairing: false,
            pair_gen: 0,
            last_error: None,
        }),
        opener: Mutex::new(opener),
        shutdown: AtomicBool::new(false),
        socket_path: socket_path.clone(),
        log: opts.log,
        layout: opts.layout.cloned(),
    });
    if opts.install_ctrlc {
        let shutdown = Arc::clone(&state);
        let path = socket_path.clone();
        let _ = ctrlc::set_handler(move || {
            shutdown.shutdown.store(true, Ordering::SeqCst);
            let _ = UnixStream::connect(&path);
        });
    }
    if opts.log {
        eprintln!("remote-debug: broker listening");
    }
    let accept_result = accept_loop(&listener, &state);
    {
        let mut inner = lock_inner(&state)?;
        invalidate_pair(&mut inner);
        if let Some(mut session) = inner.session.take() {
            if let Err(error) = session.disconnect() {
                if opts.log {
                    eprintln!("remote-debug: {error}");
                }
            }
        }
    }
    let _ = fs::remove_file(&socket_path);
    let _ = fs::remove_file(&pid_path);
    accept_result
}

fn accept_loop<T, F>(listener: &UnixListener, state: &Arc<BrokerState<T, F>>) -> Result<(), Error>
where
    T: Transport + Send + 'static,
    F: FnMut(&ConnectReq) -> Result<Session<T>, Error> + Send + 'static,
{
    listener.set_nonblocking(false)?;
    loop {
        if state.shutdown.load(Ordering::SeqCst) {
            break;
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if state.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let state = Arc::clone(state);
                thread::spawn(move || {
                    if let Err(error) = handle_client(&mut stream, &state) {
                        if state.log {
                            eprintln!("remote-debug: {error}");
                        }
                    }
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                if state.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                return Err(error.into());
            }
        }
    }
    Ok(())
}

fn handle_client<T, F>(stream: &mut UnixStream, state: &Arc<BrokerState<T, F>>) -> Result<(), Error>
where
    T: Transport + Send + 'static,
    F: FnMut(&ConnectReq) -> Result<Session<T>, Error> + Send + 'static,
{
    stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
    let request = match read_msg::<BrokerRequest>(stream) {
        Ok(request) => request,
        Err(_) => return Ok(()),
    };
    let reply = dispatch(state, &request);
    let disconnect = matches!(request, BrokerRequest::Disconnect);
    write_msg(stream, &reply)?;
    if disconnect {
        state.shutdown.store(true, Ordering::SeqCst);
        // Wake `accept` on this thread so serve exits and unlinks.
        let _ = UnixStream::connect(&state.socket_path);
    }
    Ok(())
}

fn dispatch<T, F>(state: &Arc<BrokerState<T, F>>, request: &BrokerRequest) -> BrokerReply
where
    T: Transport + Send + 'static,
    F: FnMut(&ConnectReq) -> Result<Session<T>, Error> + Send + 'static,
{
    match dispatch_inner(state, request) {
        Ok(reply) => {
            if state.log {
                log_reply(&reply);
            }
            reply
        }
        Err(error) => {
            if state.log {
                eprintln!("remote-debug: {error}");
            }
            let (connected, phase) = session_meter(state);
            BrokerReply {
                ok: false,
                message: error.to_string(),
                nonce: None,
                connected,
                phase,
                snapshot: None,
                last_log: None,
            }
        }
    }
}

fn session_meter<T, F>(state: &BrokerState<T, F>) -> (bool, BrokerPhase)
where
    T: Transport,
{
    state
        .inner
        .lock()
        .map(|inner| (inner.session.is_some(), phase_of(&inner)))
        .unwrap_or((false, BrokerPhase::Disconnected))
}

fn phase_of<T: Transport>(inner: &Inner<T>) -> BrokerPhase {
    if inner.session.is_some() {
        BrokerPhase::Connected
    } else if inner.pairing {
        BrokerPhase::Pairing
    } else {
        BrokerPhase::Disconnected
    }
}

fn status_reply<T: Transport>(inner: &mut Inner<T>) -> BrokerReply {
    let phase = phase_of(inner);
    let message = match phase {
        BrokerPhase::Connected => "connected".to_string(),
        BrokerPhase::Pairing => "pairing".to_string(),
        BrokerPhase::Disconnected => match &inner.last_error {
            Some(error) => format!("pair failed: {error}"),
            None => "disconnected".to_string(),
        },
    };
    with_session_log(inner, ok_reply(&message, inner.last_nonce, phase, None))
}

fn dispatch_inner<T, F>(
    state: &Arc<BrokerState<T, F>>,
    request: &BrokerRequest,
) -> Result<BrokerReply, Error>
where
    T: Transport + Send + 'static,
    F: FnMut(&ConnectReq) -> Result<Session<T>, Error> + Send + 'static,
{
    let mut inner = lock_inner(state)?;
    match request {
        BrokerRequest::Connect(req) => connect_locked(state, &mut inner, req),
        BrokerRequest::InjectTouch {
            x,
            y,
            slot,
            phase,
            page,
        } => {
            let phase = parse_touch_phase(phase.as_deref())?;
            session_mut(&mut inner)?.inject_touch_ex(
                TouchSample {
                    source: TouchSource::Synthetic,
                    x: *x,
                    y: *y,
                    slot: *slot,
                },
                phase,
                *page,
            )?;
            let nonce = inner.last_nonce;
            Ok(with_session_log(
                &mut inner,
                ok_reply("inject-touch queued", nonce, BrokerPhase::Connected, None),
            ))
        }
        BrokerRequest::InjectButton { key, down } => {
            let parsed = parse_product_key(key)?;
            session_mut(&mut inner)?.inject_button(parsed, *down)?;
            Ok(ok_reply(
                "inject-button queued",
                inner.last_nonce,
                BrokerPhase::Connected,
                None,
            ))
        }
        BrokerRequest::GetSnapshot { nonce } => get_snapshot_locked(&mut inner, *nonce),
        BrokerRequest::SnapshotAck { nonce } => {
            let nonce = nonce.or(inner.last_nonce).ok_or_else(|| {
                Error::RemoteDebug("no nonce; pass --nonce or get-snapshot first".into())
            })?;
            session_mut(&mut inner)?.snapshot_ack(nonce)?;
            Ok(ok_reply(
                "snapshot ack sent",
                Some(nonce),
                BrokerPhase::Connected,
                None,
            ))
        }
        BrokerRequest::SnapshotClear => {
            session_mut(&mut inner)?.snapshot_clear()?;
            Ok(ok_reply(
                "snapshot clear",
                inner.last_nonce,
                BrokerPhase::Connected,
                None,
            ))
        }
        BrokerRequest::Status => Ok(status_reply(&mut inner)),
        BrokerRequest::Reboot(req) => reboot_locked(state, &mut inner, req),
        BrokerRequest::Disconnect => {
            invalidate_pair(&mut inner);
            if let Some(mut session) = inner.session.take() {
                session.disconnect()?;
            }
            inner.last_nonce = None;
            inner.last_error = None;
            Ok(ok_reply(
                "disconnected",
                None,
                BrokerPhase::Disconnected,
                None,
            ))
        }
    }
}

fn invalidate_pair<T: Transport>(inner: &mut Inner<T>) {
    inner.pair_gen = inner.pair_gen.wrapping_add(1);
    inner.pairing = false;
}

fn connect_locked<T, F>(
    state: &Arc<BrokerState<T, F>>,
    inner: &mut Inner<T>,
    req: &ConnectReq,
) -> Result<BrokerReply, Error>
where
    T: Transport + Send + 'static,
    F: FnMut(&ConnectReq) -> Result<Session<T>, Error> + Send + 'static,
{
    if inner.session.is_some() {
        return Ok(ok_reply(
            "already connected",
            inner.last_nonce,
            BrokerPhase::Connected,
            None,
        ));
    }
    if inner.pairing {
        return Ok(ok_reply(
            "pairing",
            inner.last_nonce,
            BrokerPhase::Pairing,
            None,
        ));
    }
    start_pair(state, inner, req.clone());
    Ok(ok_reply(
        "pairing",
        inner.last_nonce,
        BrokerPhase::Pairing,
        None,
    ))
}

fn start_pair<T, F>(state: &Arc<BrokerState<T, F>>, inner: &mut Inner<T>, req: ConnectReq)
where
    T: Transport + Send + 'static,
    F: FnMut(&ConnectReq) -> Result<Session<T>, Error> + Send + 'static,
{
    inner.pair_gen = inner.pair_gen.wrapping_add(1);
    let gen = inner.pair_gen;
    inner.pairing = true;
    inner.last_error = None;
    let state = Arc::clone(state);
    thread::spawn(move || finish_pair(state, req, gen));
}

fn finish_pair<T, F>(state: Arc<BrokerState<T, F>>, req: ConnectReq, gen: u64)
where
    T: Transport,
    F: FnMut(&ConnectReq) -> Result<Session<T>, Error>,
{
    let result = match state.opener.lock() {
        Ok(mut opener) => opener(&req),
        Err(_) => Err(Error::RemoteDebug("session lock".into())),
    };
    let mut inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            if let Ok(mut session) = result {
                let _ = session.disconnect();
            }
            return;
        }
    };
    if inner.pair_gen != gen || state.shutdown.load(Ordering::SeqCst) {
        if let Ok(mut session) = result {
            let _ = session.disconnect();
        }
        return;
    }
    match result {
        Ok(session) => {
            if req.remember {
                if let Some(layout) = &state.layout {
                    if let Err(error) = remember_from_req(layout, &req) {
                        if state.log {
                            eprintln!("remote-debug: {error}");
                        }
                    }
                }
            }
            inner.session = Some(session);
            inner.pairing = false;
            inner.last_error = None;
        }
        Err(error) => {
            inner.pairing = false;
            inner.last_error = Some(error.to_string());
            if state.log {
                eprintln!("remote-debug: {error}");
            }
        }
    }
}

fn reboot_locked<T, F>(
    state: &Arc<BrokerState<T, F>>,
    inner: &mut Inner<T>,
    req: &RebootReq,
) -> Result<BrokerReply, Error>
where
    T: Transport + Send + 'static,
    F: FnMut(&ConnectReq) -> Result<Session<T>, Error> + Send + 'static,
{
    let mut session = inner.session.take().ok_or_else(|| {
        Error::RemoteDebug("not connected; run remote-debug connect first".into())
    })?;
    session.reboot()?;
    inner.last_nonce = None;
    if req.no_reconnect {
        invalidate_pair(inner);
        return Ok(ok_reply(
            "device reboot sent; GATT session gone (embedded MCU, not this host)",
            None,
            BrokerPhase::Disconnected,
            None,
        ));
    }
    start_pair(
        state,
        inner,
        ConnectReq {
            pin: req.pin,
            port: req.port.clone(),
            name: req.name.clone(),
            remember: req.remember,
        },
    );
    Ok(ok_reply(
        "device rebooted; pairing",
        None,
        BrokerPhase::Pairing,
        None,
    ))
}

fn get_snapshot_locked<T: Transport>(
    inner: &mut Inner<T>,
    nonce: Option<u64>,
) -> Result<BrokerReply, Error> {
    let nonce = nonce.unwrap_or_else(time_nonce);
    match session_mut(inner)?.get_snapshot(nonce) {
        Ok(snap) => {
            inner.last_nonce = Some(snap.nonce);
            let kind = match snap.kind {
                FrameKind::Mono => "mono",
                FrameKind::Gray4 => "gray4",
            };
            let wire = SnapshotWire {
                nonce: snap.nonce,
                width: snap.width,
                height: snap.height,
                kind: kind.to_string(),
                hold: snap.hold,
                scene: snap.scene,
                target_step: snap.target_step,
                target_kind: snap.target_kind,
                target_expect_x: snap.target_expect_x,
                target_expect_y: snap.target_expect_y,
                bw: snap.bw,
                red: snap.red,
            };
            Ok(with_session_log(
                inner,
                ok_reply(
                    &format!("snapshot {}x{} kind={}", wire.width, wire.height, wire.kind),
                    Some(wire.nonce),
                    BrokerPhase::Connected,
                    Some(wire),
                ),
            ))
        }
        Err(remote_debug_host::Error::SnapshotBusy { armed }) => Ok(BrokerReply {
            ok: false,
            message: format!("snapshot busy (armed={armed:#x})"),
            nonce: Some(armed),
            connected: true,
            phase: BrokerPhase::Connected,
            snapshot: None,
            last_log: None,
        }),
        Err(error) => Err(map_ble(error)),
    }
}

fn remember_from_req(layout: &Layout, req: &ConnectReq) -> Result<(), Error> {
    use crate::detect;
    use crate::usb_serial_from_port;

    let usb = req
        .port
        .as_deref()
        .and_then(usb_serial_from_port)
        .or_else(|| {
            detect::resolve_sticky_port(req.port.clone())
                .ok()
                .and_then(|port| usb_serial_from_port(&port))
        });
    crate::remember_unit(layout, None, usb.as_deref())
}

fn time_nonce() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
        .max(1)
}

fn session_mut<T: Transport>(inner: &mut Inner<T>) -> Result<&mut Session<T>, Error> {
    inner
        .session
        .as_mut()
        .ok_or_else(|| Error::RemoteDebug("not connected; run remote-debug connect first".into()))
}

fn lock_inner<'a, T: Transport, F>(
    state: &'a BrokerState<T, F>,
) -> Result<std::sync::MutexGuard<'a, Inner<T>>, Error> {
    state
        .inner
        .lock()
        .map_err(|_| Error::RemoteDebug("session lock".into()))
}

fn ok_reply(
    message: &str,
    nonce: Option<u64>,
    phase: BrokerPhase,
    snapshot: Option<SnapshotWire>,
) -> BrokerReply {
    BrokerReply {
        ok: true,
        message: message.to_string(),
        nonce,
        connected: matches!(phase, BrokerPhase::Connected),
        phase,
        snapshot,
        last_log: None,
    }
}

fn with_session_log<T: Transport>(inner: &mut Inner<T>, mut reply: BrokerReply) -> BrokerReply {
    if let Some(session) = inner.session.as_mut() {
        session.drain_logs();
        reply.last_log = session.last_log().map(str::to_string);
    }
    reply
}

fn log_reply(reply: &BrokerReply) {
    eprintln!("remote-debug: {}", reply.message);
}

fn map_ble(error: remote_debug_host::Error) -> Error {
    Error::RemoteDebug(error.to_string())
}

impl From<remote_debug_host::Error> for Error {
    fn from(error: remote_debug_host::Error) -> Self {
        map_ble(error)
    }
}

fn write_msg<T: Serialize>(stream: &mut UnixStream, value: &T) -> Result<(), Error> {
    let bytes = serde_json::to_vec(value)?;
    let len = u32::try_from(bytes.len())
        .map_err(|_| Error::RemoteDebug("remote-debug frame larger than u32".into()))?;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}

fn read_msg<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> Result<T, Error> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = usize::try_from(u32::from_le_bytes(len_buf))
        .map_err(|_| Error::RemoteDebug("remote-debug frame length".into()))?;
    if len == 0 || len > MAX_FRAME {
        return Err(Error::RemoteDebug("remote-debug frame length".into()));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(serde_json::from_slice(&buf)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use remote_debug_host::{FakeTransport, Transport as HostTransport};
    use std::sync::atomic::AtomicU32;

    struct SharedFake {
        inner: FakeTransport,
        disconnects: Arc<AtomicU32>,
    }

    impl HostTransport for SharedFake {
        fn write_frame(&mut self, framed: &[u8]) -> Result<(), remote_debug_host::Error> {
            self.inner.write_frame(framed)
        }

        fn read_chunk(&mut self) -> Result<Vec<u8>, remote_debug_host::Error> {
            self.inner.read_chunk()
        }

        fn disconnect(&mut self, keep_bond: bool) -> Result<(), remote_debug_host::Error> {
            let result = self.inner.disconnect(keep_bond);
            self.disconnects
                .store(self.inner.disconnects, Ordering::SeqCst);
            result
        }
    }

    fn start_broker(
        dir: &Path,
        disconnects: Arc<AtomicU32>,
    ) -> thread::JoinHandle<Result<(), Error>> {
        start_broker_with(dir, {
            let disconnects = disconnects.clone();
            move |_req| {
                Ok(Session::new(
                    SharedFake {
                        inner: FakeTransport::tiny(),
                        disconnects: disconnects.clone(),
                    },
                    false,
                ))
            }
        })
    }

    fn start_broker_with<F>(dir: &Path, opener: F) -> thread::JoinHandle<Result<(), Error>>
    where
        F: FnMut(&ConnectReq) -> Result<Session<SharedFake>, Error> + Send + 'static,
    {
        let dir = dir.to_path_buf();
        thread::spawn(move || {
            serve_with(
                ServeOpts {
                    dir: &dir,
                    name: DEFAULT_ADV_NAME,
                    install_ctrlc: false,
                    log: false,
                    layout: None,
                },
                opener,
            )
        })
    }

    fn wait_phase(dir: &Path, want: BrokerPhase) -> BrokerReply {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut last = None;
        while Instant::now() < deadline {
            let reply = rpc(dir, DEFAULT_ADV_NAME, &BrokerRequest::Status).expect("status");
            if reply.phase == want {
                return reply;
            }
            last = Some(reply);
            thread::sleep(Duration::from_millis(20));
        }
        panic!("wanted {want:?}, last={last:?}");
    }

    fn connect_req() -> BrokerRequest {
        BrokerRequest::Connect(ConnectReq {
            pin: Some(42),
            port: None,
            name: DEFAULT_ADV_NAME.to_string(),
            remember: false,
        })
    }

    #[test]
    fn socket_stem_strips_colons() {
        assert_eq!(socket_stem("sticky-rs"), "remote-debug-sticky-rs");
        assert_eq!(socket_stem("aa:bb:cc"), "remote-debug-aa_bb_cc");
    }

    #[test]
    fn rpc_without_socket_is_no_broker() {
        let tmp = tempfile::tempdir().unwrap();
        let err = rpc(tmp.path(), DEFAULT_ADV_NAME, &BrokerRequest::Status).unwrap_err();
        assert!(err.to_string().contains(NO_BROKER), "{err}");
        assert!(
            err.to_string().contains(
                &broker_socket_path(tmp.path(), DEFAULT_ADV_NAME)
                    .display()
                    .to_string()
            ),
            "{err}"
        );
    }

    #[test]
    fn rpc_socket_exists_but_not_listening() {
        let tmp = tempfile::tempdir().unwrap();
        let path = broker_socket_path(tmp.path(), DEFAULT_ADV_NAME);
        fs::write(&path, b"not a socket").unwrap();
        let err = rpc(tmp.path(), DEFAULT_ADV_NAME, &BrokerRequest::Status).unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("exists but connect failed") || text.contains(NO_BROKER),
            "{text}"
        );
        assert!(text.contains(&path.display().to_string()), "{text}");
    }

    #[test]
    fn broker_log_path_matches_stem() {
        assert_eq!(
            broker_log_path(Path::new("/run"), "sticky-rs"),
            PathBuf::from("/run/remote-debug-sticky-rs.log")
        );
    }

    #[test]
    fn resolve_broker_exe_prefers_on_disk_xtask() {
        let tmp = tempfile::tempdir().unwrap();
        let on_disk = tmp.path().join("target").join("debug").join("xtask");
        fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        fs::write(&on_disk, b"").unwrap();
        let current = tmp.path().join("deleted-inode");
        assert_eq!(resolve_broker_exe(tmp.path(), &current), on_disk);
    }

    #[test]
    fn resolve_broker_exe_falls_back_to_current() {
        let tmp = tempfile::tempdir().unwrap();
        let current = tmp.path().join("current");
        fs::write(&current, b"").unwrap();
        assert_eq!(resolve_broker_exe(tmp.path(), &current), current);
    }

    #[test]
    fn disconnect_exits_and_unlinks_socket() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let disconnects = Arc::new(AtomicU32::new(0));
        let handle = start_broker(dir, disconnects.clone());
        wait_for_broker(dir, DEFAULT_ADV_NAME, Duration::from_secs(2)).expect("listen");

        let gone = rpc(dir, DEFAULT_ADV_NAME, &BrokerRequest::Disconnect).expect("disconnect");
        assert!(gone.ok);
        assert!(!gone.connected);
        handle.join().expect("join").expect("serve");
        assert!(!broker_socket_path(dir, DEFAULT_ADV_NAME).exists());
        let err = rpc(dir, DEFAULT_ADV_NAME, &BrokerRequest::Status).unwrap_err();
        assert!(err.to_string().contains(NO_BROKER), "{err}");
    }

    #[test]
    fn two_clients_busy_hangup_does_not_disconnect() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let disconnects = Arc::new(AtomicU32::new(0));
        let handle = start_broker(dir, disconnects.clone());
        wait_for_broker(dir, DEFAULT_ADV_NAME, Duration::from_secs(2)).expect("listen");

        let started = rpc(dir, DEFAULT_ADV_NAME, &connect_req()).expect("connect");
        assert!(started.ok);
        assert_eq!(started.phase, BrokerPhase::Pairing);
        assert!(!started.connected);
        let connected = wait_phase(dir, BrokerPhase::Connected);
        assert!(connected.connected);

        let touch_a = thread::spawn({
            let dir = dir.to_path_buf();
            move || {
                rpc(
                    &dir,
                    DEFAULT_ADV_NAME,
                    &BrokerRequest::InjectTouch {
                        x: 1,
                        y: 2,
                        slot: None,
                        phase: None,
                        page: false,
                    },
                )
            }
        });
        let touch_b = thread::spawn({
            let dir = dir.to_path_buf();
            move || {
                rpc(
                    &dir,
                    DEFAULT_ADV_NAME,
                    &BrokerRequest::InjectTouch {
                        x: 3,
                        y: 4,
                        slot: None,
                        phase: None,
                        page: false,
                    },
                )
            }
        });
        assert!(touch_a.join().unwrap().unwrap().ok);
        assert!(touch_b.join().unwrap().unwrap().ok);

        let first = rpc(
            dir,
            DEFAULT_ADV_NAME,
            &BrokerRequest::GetSnapshot { nonce: Some(1) },
        )
        .expect("first get");
        assert!(first.ok, "{}", first.message);
        let snap = first.snapshot.expect("planes");
        assert_eq!(snap.width, 8);
        assert_eq!(snap.height, 4);
        assert_eq!(snap.kind, "mono");
        assert_eq!(snap.bw.len(), 4);

        let busy = rpc(
            dir,
            DEFAULT_ADV_NAME,
            &BrokerRequest::GetSnapshot { nonce: Some(2) },
        )
        .expect("busy get");
        assert!(!busy.ok);
        assert!(busy.message.contains("busy"), "{}", busy.message);
        assert!(busy.connected);

        let hangup = UnixStream::connect(broker_socket_path(dir, DEFAULT_ADV_NAME)).unwrap();
        drop(hangup);
        thread::sleep(Duration::from_millis(50));
        assert_eq!(disconnects.load(Ordering::SeqCst), 0);

        let status = rpc(dir, DEFAULT_ADV_NAME, &BrokerRequest::Status).expect("status");
        assert!(status.connected);

        rpc(
            dir,
            DEFAULT_ADV_NAME,
            &BrokerRequest::SnapshotAck { nonce: Some(1) },
        )
        .expect("ack");
        let second = rpc(
            dir,
            DEFAULT_ADV_NAME,
            &BrokerRequest::GetSnapshot { nonce: Some(3) },
        )
        .expect("after ack");
        assert!(second.ok, "{}", second.message);

        let gone = rpc(dir, DEFAULT_ADV_NAME, &BrokerRequest::Disconnect).expect("disconnect");
        assert!(gone.ok);
        assert!(!gone.connected);
        handle.join().expect("join").expect("serve");
        assert_eq!(disconnects.load(Ordering::SeqCst), 1);
        assert!(!broker_socket_path(dir, DEFAULT_ADV_NAME).exists());
    }

    #[test]
    fn connect_returns_while_opener_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let handle = start_broker_with(dir, |_req| {
            thread::sleep(Duration::from_millis(400));
            Ok(Session::new(
                SharedFake {
                    inner: FakeTransport::tiny(),
                    disconnects: Arc::new(AtomicU32::new(0)),
                },
                false,
            ))
        });
        wait_for_broker(dir, DEFAULT_ADV_NAME, Duration::from_secs(2)).expect("listen");

        let started = Instant::now();
        let pairing = rpc(dir, DEFAULT_ADV_NAME, &connect_req()).expect("connect");
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "connect blocked"
        );
        assert!(pairing.ok);
        assert_eq!(pairing.phase, BrokerPhase::Pairing);
        assert!(!pairing.connected);

        let status = rpc(dir, DEFAULT_ADV_NAME, &BrokerRequest::Status).expect("status");
        assert_eq!(status.phase, BrokerPhase::Pairing);
        assert_eq!(status.message, "pairing");

        let again = rpc(dir, DEFAULT_ADV_NAME, &connect_req()).expect("idempotent");
        assert_eq!(again.phase, BrokerPhase::Pairing);

        let connected = wait_phase(dir, BrokerPhase::Connected);
        assert!(connected.connected);
        assert_eq!(connected.message, "connected");

        rpc(dir, DEFAULT_ADV_NAME, &BrokerRequest::Disconnect).expect("disconnect");
        handle.join().expect("join").expect("serve");
    }

    #[test]
    fn disconnect_unlinks_during_pairing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let handle = start_broker_with(dir, |_req| {
            thread::sleep(Duration::from_millis(800));
            Ok(Session::new(
                SharedFake {
                    inner: FakeTransport::tiny(),
                    disconnects: Arc::new(AtomicU32::new(0)),
                },
                false,
            ))
        });
        wait_for_broker(dir, DEFAULT_ADV_NAME, Duration::from_secs(2)).expect("listen");

        let pairing = rpc(dir, DEFAULT_ADV_NAME, &connect_req()).expect("connect");
        assert_eq!(pairing.phase, BrokerPhase::Pairing);

        let gone = rpc(dir, DEFAULT_ADV_NAME, &BrokerRequest::Disconnect).expect("disconnect");
        assert!(gone.ok);
        assert!(!gone.connected);
        handle.join().expect("join").expect("serve");
        assert!(!broker_socket_path(dir, DEFAULT_ADV_NAME).exists());
    }

    #[test]
    fn status_reports_pair_failed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let handle = start_broker_with(dir, |_req| -> Result<Session<SharedFake>, Error> {
            Err(Error::RemoteDebug("Connect dropped before pair".into()))
        });
        wait_for_broker(dir, DEFAULT_ADV_NAME, Duration::from_secs(2)).expect("listen");

        rpc(dir, DEFAULT_ADV_NAME, &connect_req()).expect("connect");
        let failed = wait_phase(dir, BrokerPhase::Disconnected);
        assert!(failed.message.contains("pair failed"), "{}", failed.message);
        assert!(!failed.connected);

        rpc(dir, DEFAULT_ADV_NAME, &BrokerRequest::Disconnect).expect("disconnect");
        handle.join().expect("join").expect("serve");
    }
}
