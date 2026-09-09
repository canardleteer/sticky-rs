//! Unix-socket broker that owns one remote-debug [`Session`].
//!
//! CLI and MCP clients send one length-prefixed JSON request and wait
//! for one reply. The broker serializes GATT so two snapshot or inject
//! clients cannot interleave ATT chunks. Client hangup does **not**
//! drop the link; [`BrokerRequest::Disconnect`] or serve exit does.
//!
//! This crate has **no** clap, **no** UART, and **no** Sticky pin map.
//! Callers pass a socket directory and an opener that builds a
//! [`Session`]. Detached serve uses [`SpawnSpec`] (exe + argv), not a
//! hardcoded xtask leaf.
//!
//! The socket key is the advertise name, never a MAC.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod types;

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

pub use error::Error;
pub use types::{
    BrokerPhase, BrokerReply, BrokerRequest, ConnectReq, RebootReq, RememberHook, ServeOpts,
    SnapshotWire, SpawnSpec,
};

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
    on_remember: Option<RememberHook>,
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

fn no_broker_at(path: &Path) -> Error {
    Error::message(format!("{NO_BROKER} ({})", path.display()))
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
        _ => Err(Error::message("key must be ok, page-up, or page-down")),
    }
}

/// `down` / `move` / `up`. Unset is a tap.
fn parse_touch_phase(raw: Option<&str>) -> Result<TouchPhase, Error> {
    match raw.map(str::to_ascii_lowercase).as_deref() {
        None | Some("down") | Some("tap") => Ok(TouchPhase::TOUCH_PHASE_DOWN),
        Some("move") => Ok(TouchPhase::TOUCH_PHASE_MOVE),
        Some("up") => Ok(TouchPhase::TOUCH_PHASE_UP),
        Some(_) => Err(Error::message("phase must be down, move, or up")),
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
        _ if path.exists() => Error::message(format!(
            "broker socket {} exists but connect failed: {error}",
            path.display()
        )),
        _ => error.into(),
    }
}

/// Unlink a leftover socket when the peer pid is dead, then spawn serve
/// if nothing is listening.
///
/// # Errors
///
/// Spawn failure or the child never bound.
pub fn ensure_broker(dir: &Path, name: &str, spec: &SpawnSpec) -> Result<(), Error> {
    reclaim_stale(dir, name)?;
    if broker_listening(dir, name) {
        return Ok(());
    }
    if !spec.exe.is_file() {
        return Err(Error::message(format!(
            "cannot spawn serve; no executable at {}",
            spec.exe.display()
        )));
    }
    fs::create_dir_all(dir)?;
    let log_path = broker_log_path(dir, name);
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| {
            Error::message(format!(
                "cannot open broker log {}: {error}",
                log_path.display()
            ))
        })?;
    let log_err = log.try_clone().map_err(|error| {
        Error::message(format!(
            "cannot clone broker log {}: {error}",
            log_path.display()
        ))
    })?;
    let mut cmd = Command::new(&spec.exe);
    cmd.args(&spec.args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::from(log));
    cmd.stderr(Stdio::from(log_err));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map_err(|error| {
        Error::message(format!(
            "cannot spawn serve from {}: {error}",
            spec.exe.display()
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

/// Accept clients until [`BrokerRequest::Disconnect`] or Ctrl-C.
///
/// `opener` builds a [`Session`] (tests pass
/// [`remote_debug_host::FakeTransport`]).
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
        return Err(Error::message("already serving"));
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
        on_remember: opts.on_remember,
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
            let nonce = nonce
                .or(inner.last_nonce)
                .ok_or_else(|| Error::message("no nonce; pass --nonce or get-snapshot first"))?;
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
        Err(_) => Err(Error::message("session lock")),
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
                if let Some(hook) = &state.on_remember {
                    if let Err(error) = hook(&req) {
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
    let mut session = inner
        .session
        .take()
        .ok_or_else(|| Error::message("not connected; run remote-debug connect first"))?;
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
        Err(error) => Err(error.into()),
    }
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
        .ok_or_else(|| Error::message("not connected; run remote-debug connect first"))
}

fn lock_inner<'a, T: Transport, F>(
    state: &'a BrokerState<T, F>,
) -> Result<std::sync::MutexGuard<'a, Inner<T>>, Error> {
    state
        .inner
        .lock()
        .map_err(|_| Error::message("session lock"))
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

fn write_msg<T: Serialize>(stream: &mut UnixStream, value: &T) -> Result<(), Error> {
    let bytes = serde_json::to_vec(value)?;
    let len = u32::try_from(bytes.len())
        .map_err(|_| Error::message("remote-debug frame larger than u32"))?;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}

fn read_msg<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> Result<T, Error> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = usize::try_from(u32::from_le_bytes(len_buf))
        .map_err(|_| Error::message("remote-debug frame length"))?;
    if len == 0 || len > MAX_FRAME {
        return Err(Error::message("remote-debug frame length"));
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
                    on_remember: None,
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
            Err(Error::message("Connect dropped before pair"))
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
