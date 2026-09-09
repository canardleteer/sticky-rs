//! ConnectRPC owner that holds 0..N remote-debug [`Session`]s.
//!
//! One loopback HTTP listener. Clients send generated
//! `RemoteDebugControlService` RPCs. The owner serializes GATT per
//! advertise name so two snapshot or inject clients cannot interleave
//! ATT chunks. Client hangup does **not** drop the link;
//! [`DisconnectRequest`] drops one session; [`ShutdownRequest`] or
//! serve exit drops every session.
//!
//! This crate has **no** clap, **no** UART, and **no** Sticky pin map.
//! Callers pass an endpoint directory and an opener that builds a
//! [`Session`]. Detached serve uses [`SpawnSpec`] (exe + argv), not a
//! hardcoded xtask leaf.
//!
//! The map key is the advertise name (`target`), never a MAC.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod client;
mod error;
mod types;

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use buffa::Enumeration;
use connectrpc::{
    ConnectError, ErrorCode, RequestContext, Response, Router, Server, ServiceRequest,
    ServiceResult,
};
use panel_view::{FrameKind, TouchSample, TouchSource};
use remote_debug_host::{Session, Transport, DEFAULT_ADV_NAME};
use remote_debug_wire::v1::{ProductKey, TouchPhase};

pub use client::ControlClient;
pub use error::Error;
pub use types::{RememberHook, ServeOpts, SpawnSpec};

/// Generated buffa messages (`sticky.remote.{shared,v1,control}.v1`).
#[allow(missing_docs, clippy::derivable_impls, clippy::match_single_binding)]
pub mod proto {
    /// Proto packages under `sticky`.
    #[allow(missing_docs)]
    pub mod sticky {
        /// Remote-debug packages.
        #[allow(missing_docs)]
        pub mod remote {
            /// Shared inject / snapshot types.
            #[allow(missing_docs)]
            pub mod shared {
                /// `sticky.remote.shared.v1`.
                #[allow(missing_docs, clippy::derivable_impls, clippy::match_single_binding)]
                pub mod v1 {
                    include!("gen/buffa/sticky.remote.shared.v1.mod.rs");
                }
            }
            /// GATT `Envelope` package (host copy with JSON).
            #[allow(missing_docs, clippy::derivable_impls, clippy::match_single_binding)]
            pub mod v1 {
                include!("gen/buffa/sticky.remote.v1.mod.rs");
            }
            /// ConnectRPC control plane.
            #[allow(missing_docs)]
            pub mod control {
                /// `sticky.remote.control.v1`.
                #[allow(missing_docs, clippy::derivable_impls, clippy::match_single_binding)]
                pub mod v1 {
                    include!("gen/buffa/sticky.remote.control.v1.mod.rs");
                }
            }
        }
    }
}

/// Generated ConnectRPC stubs.
#[allow(missing_docs, clippy::type_complexity)]
pub mod connect {
    /// Proto packages under `sticky`.
    #[allow(missing_docs)]
    pub mod sticky {
        /// Remote-debug packages.
        #[allow(missing_docs)]
        pub mod remote {
            /// Control service stubs.
            #[allow(missing_docs)]
            pub mod control {
                /// `sticky.remote.control.v1`.
                #[allow(missing_docs, clippy::match_single_binding, clippy::type_complexity)]
                pub mod v1 {
                    include!("gen/connect/sticky.remote.control.v1.mod.rs");
                }
            }
        }
    }
}

/// `sticky.remote.control.v1` request / response types.
pub use proto::sticky::remote::control::v1 as control;
/// `sticky.remote.shared.v1` inject / snapshot types (host JSON copy).
pub use proto::sticky::remote::shared::v1 as shared;

pub(crate) use connect::sticky::remote::control::v1 as connect_svc;

/// Printed when the endpoint file is missing or the peer is gone.
pub const NO_BROKER: &str = "no broker; run connect or serve";

/// How long [`ensure_broker`] waits for the child to bind.
const SPAWN_WAIT: Duration = Duration::from_secs(5);

/// `$dir/remote-debug.connect` (loopback URI, one owner).
#[must_use]
pub fn endpoint_path(dir: &Path) -> PathBuf {
    dir.join("remote-debug.connect")
}

/// Pid sidecar next to the endpoint file.
#[must_use]
pub fn broker_pid_path(dir: &Path) -> PathBuf {
    dir.join("remote-debug.pid")
}

/// Append-only log for a detached `serve` (`connect` auto-start).
#[must_use]
pub fn broker_log_path(dir: &Path) -> PathBuf {
    dir.join("remote-debug.log")
}

fn no_broker_at(path: &Path) -> Error {
    Error::message(format!("{NO_BROKER} ({})", path.display()))
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

/// Advertise name, or [`DEFAULT_ADV_NAME`] when empty.
fn target_name(raw: &str) -> String {
    let name = raw.trim();
    if name.is_empty() {
        DEFAULT_ADV_NAME.to_string()
    } else {
        name.to_string()
    }
}

/// Unlink a leftover endpoint when the peer pid is dead, then spawn
/// serve if nothing is listening.
///
/// # Errors
///
/// Spawn failure or the child never bound.
pub fn ensure_broker(dir: &Path, spec: &SpawnSpec) -> Result<(), Error> {
    reclaim_stale(dir)?;
    if broker_listening(dir) {
        return Ok(());
    }
    if !spec.exe.is_file() {
        return Err(Error::message(format!(
            "cannot spawn serve; no executable at {}",
            spec.exe.display()
        )));
    }
    fs::create_dir_all(dir)?;
    let log_path = broker_log_path(dir);
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
    wait_for_broker(dir, SPAWN_WAIT)
}

/// Poll until the pid file is live or `budget` elapses.
///
/// # Errors
///
/// Timeout.
pub fn wait_for_broker(dir: &Path, budget: Duration) -> Result<(), Error> {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if broker_listening(dir) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(no_broker_at(&endpoint_path(dir)))
}

fn broker_listening(dir: &Path) -> bool {
    let endpoint = endpoint_path(dir);
    let pid_path = broker_pid_path(dir);
    endpoint.exists() && read_pid(&pid_path).is_some_and(pid_alive)
}

fn reclaim_stale(dir: &Path) -> Result<(), Error> {
    let endpoint = endpoint_path(dir);
    let pid_path = broker_pid_path(dir);
    if broker_listening(dir) {
        return Ok(());
    }
    if endpoint.exists() {
        if let Some(pid) = read_pid(&pid_path) {
            if pid_alive(pid) {
                return Ok(());
            }
        }
        let _ = fs::remove_file(&endpoint);
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

struct Slot<T: Transport> {
    session: Option<Session<T>>,
    last_nonce: Option<u64>,
    pairing: bool,
    pair_gen: u64,
    last_error: Option<String>,
}

impl<T: Transport> Default for Slot<T> {
    fn default() -> Self {
        Self {
            session: None,
            last_nonce: None,
            pairing: false,
            pair_gen: 0,
            last_error: None,
        }
    }
}

struct Inner<T: Transport> {
    targets: HashMap<String, Slot<T>>,
}

struct OwnerState<T: Transport, F> {
    inner: Mutex<Inner<T>>,
    opener: Mutex<F>,
    shutdown: AtomicBool,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    log: bool,
    on_remember: Option<RememberHook>,
}

/// Accept clients until [`ShutdownRequest`] or Ctrl-C.
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
    F: FnMut(&control::ConnectRequest) -> Result<Session<T>, Error> + Send + 'static,
{
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(serve_async(opts, opener))
}

async fn serve_async<T, F>(opts: ServeOpts<'_>, opener: F) -> Result<(), Error>
where
    T: Transport + Send + 'static,
    F: FnMut(&control::ConnectRequest) -> Result<Session<T>, Error> + Send + 'static,
{
    fs::create_dir_all(opts.dir)?;
    reclaim_stale(opts.dir)?;
    let endpoint = endpoint_path(opts.dir);
    let pid_path = broker_pid_path(opts.dir);
    if broker_listening(opts.dir) {
        return Err(Error::message("already serving"));
    }
    let addr = opts.listen.unwrap_or(SocketAddr::from(([127, 0, 0, 1], 0)));
    let bound = Server::bind(addr)
        .await
        .map_err(|error| Error::message(format!("bind: {error}")))?;
    let local = bound
        .local_addr()
        .map_err(|error| Error::message(format!("local addr: {error}")))?;
    fs::write(&endpoint, format!("http://{local}\n"))?;
    write_pid(&pid_path)?;
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let state = Arc::new(OwnerState {
        inner: Mutex::new(Inner {
            targets: HashMap::new(),
        }),
        opener: Mutex::new(opener),
        shutdown: AtomicBool::new(false),
        shutdown_tx,
        log: opts.log,
        on_remember: opts.on_remember,
    });
    if opts.install_ctrlc {
        let shutdown = Arc::clone(&state);
        let _ = ctrlc::set_handler(move || {
            request_shutdown(&shutdown);
        });
    }
    if opts.log {
        eprintln!("remote-debug: owner listening on http://{local}");
    }
    let router = Router::new().add_service(Arc::new(Owner(Arc::clone(&state))));
    let serve = bound.serve_with_graceful_shutdown(router, async move {
        let _ = shutdown_rx.wait_for(|stop| *stop).await;
    });
    let serve_result = serve
        .await
        .map_err(|error| Error::message(format!("serve: {error}")));
    off_runtime(|| drop_all_sessions(&state, opts.log));
    let _ = fs::remove_file(&endpoint);
    let _ = fs::remove_file(&pid_path);
    serve_result
}

fn request_shutdown<T: Transport, F>(state: &OwnerState<T, F>) {
    state.shutdown.store(true, Ordering::SeqCst);
    let _ = state.shutdown_tx.send(true);
}

fn drop_all_sessions<T: Transport, F>(state: &OwnerState<T, F>, log: bool) {
    let Ok(mut inner) = state.inner.lock() else {
        return;
    };
    for slot in inner.targets.values_mut() {
        invalidate_pair(slot);
        if let Some(mut session) = slot.session.take() {
            if let Err(error) = session.disconnect() {
                if log {
                    eprintln!("remote-debug: {error}");
                }
            }
        }
    }
    inner.targets.clear();
}

fn lock_inner<T: Transport, F>(
    state: &OwnerState<T, F>,
) -> Result<std::sync::MutexGuard<'_, Inner<T>>, ConnectError> {
    state
        .inner
        .lock()
        .map_err(|_| ConnectError::new(ErrorCode::Internal, "session lock"))
}

fn slot_mut<'a, T: Transport>(inner: &'a mut Inner<T>, target: &str) -> &'a mut Slot<T> {
    inner.targets.entry(target.to_string()).or_default()
}

fn phase_of<T: Transport>(slot: &Slot<T>) -> control::SessionPhase {
    if slot.session.is_some() {
        control::SessionPhase::SESSION_PHASE_CONNECTED
    } else if slot.pairing {
        control::SessionPhase::SESSION_PHASE_PAIRING
    } else {
        control::SessionPhase::SESSION_PHASE_DISCONNECTED
    }
}

fn status_fields<T: Transport>(slot: &Slot<T>) -> (String, control::SessionPhase, bool) {
    let phase = phase_of(slot);
    let message = match phase {
        control::SessionPhase::SESSION_PHASE_CONNECTED => "connected".to_string(),
        control::SessionPhase::SESSION_PHASE_PAIRING => "pairing".to_string(),
        _ => match &slot.last_error {
            Some(error) => format!("pair failed: {error}"),
            None => "disconnected".to_string(),
        },
    };
    let connected = matches!(phase, control::SessionPhase::SESSION_PHASE_CONNECTED);
    (message, phase, connected)
}

fn last_log<T: Transport>(slot: &mut Slot<T>) -> Option<String> {
    slot.session.as_mut().and_then(|session| {
        session.drain_logs();
        session.last_log().map(str::to_string)
    })
}

fn session_mut<T: Transport>(slot: &mut Slot<T>) -> Result<&mut Session<T>, ConnectError> {
    slot.session.as_mut().ok_or_else(|| {
        ConnectError::new(
            ErrorCode::FailedPrecondition,
            "not connected; run remote-debug connect first",
        )
    })
}

fn invalidate_pair<T: Transport>(slot: &mut Slot<T>) {
    slot.pair_gen = slot.pair_gen.wrapping_add(1);
    slot.pairing = false;
}

fn start_pair<T, F>(state: &Arc<OwnerState<T, F>>, slot: &mut Slot<T>, req: control::ConnectRequest)
where
    T: Transport + Send + 'static,
    F: FnMut(&control::ConnectRequest) -> Result<Session<T>, Error> + Send + 'static,
{
    slot.pair_gen = slot.pair_gen.wrapping_add(1);
    let gen = slot.pair_gen;
    slot.pairing = true;
    slot.last_error = None;
    let target = target_name(&req.target);
    let state = Arc::clone(state);
    thread::spawn(move || finish_pair(state, target, req, gen));
}

/// Service wrapper so handlers can [`Arc::clone`] the owner.
struct Owner<T: Transport, F>(Arc<OwnerState<T, F>>);

/// Run `f` on a thread that is not inside the owner's Tokio runtime.
///
/// [`remote_debug_host::BluerTransport`] uses its own runtime and
/// `block_on`. Calling that from a ConnectRPC worker panics
/// (`Cannot start a runtime from within a runtime`).
fn off_runtime<R: Send>(f: impl FnOnce() -> R + Send) -> R {
    thread::scope(|scope| match scope.spawn(f).join() {
        Ok(value) => value,
        Err(_) => panic!("remote-debug owner worker panicked"),
    })
}

fn finish_pair<T, F>(
    state: Arc<OwnerState<T, F>>,
    target: String,
    req: control::ConnectRequest,
    gen: u64,
) where
    T: Transport,
    F: FnMut(&control::ConnectRequest) -> Result<Session<T>, Error>,
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
    let slot = slot_mut(&mut inner, &target);
    if slot.pair_gen != gen || state.shutdown.load(Ordering::SeqCst) {
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
            slot.session = Some(session);
            slot.pairing = false;
            slot.last_error = None;
        }
        Err(error) => {
            slot.pairing = false;
            slot.last_error = Some(error.to_string());
            if state.log {
                eprintln!("remote-debug: {error}");
            }
        }
    }
}

fn time_nonce() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
        .max(1)
}

fn snapshot_proto(snap: remote_debug_host::SnapshotPlanes) -> shared::Snapshot {
    let kind = match snap.kind {
        FrameKind::Mono => shared::FrameKind::FRAME_KIND_MONO,
        FrameKind::Gray4 => shared::FrameKind::FRAME_KIND_GRAY4,
    };
    let target_kind = snap
        .target_kind
        .and_then(|kind| shared::TargetKind::from_i32(kind as i32))
        .unwrap_or(shared::TargetKind::TARGET_KIND_UNSPECIFIED);
    shared::Snapshot {
        nonce: snap.nonce,
        width: u32::from(snap.width),
        height: u32::from(snap.height),
        kind: kind.into(),
        hold: snap.hold,
        bw: snap.bw,
        red: snap.red.unwrap_or_default(),
        scene: snap.scene,
        target_step: snap.target_step,
        target_kind: target_kind.into(),
        target_expect_x: snap.target_expect_x,
        target_expect_y: snap.target_expect_y,
        ..shared::Snapshot::default()
    }
}

fn wire_phase(inject: &shared::InjectTouch) -> TouchPhase {
    match inject.phase.as_known() {
        Some(shared::TouchPhase::TOUCH_PHASE_MOVE) => TouchPhase::TOUCH_PHASE_MOVE,
        Some(shared::TouchPhase::TOUCH_PHASE_UP) => TouchPhase::TOUCH_PHASE_UP,
        _ => TouchPhase::TOUCH_PHASE_DOWN,
    }
}

fn wire_key(inject: &shared::InjectButton) -> Result<ProductKey, ConnectError> {
    match inject.key.as_known() {
        Some(shared::ProductKey::PRODUCT_KEY_OK) => Ok(ProductKey::PRODUCT_KEY_OK),
        Some(shared::ProductKey::PRODUCT_KEY_PAGE_UP) => Ok(ProductKey::PRODUCT_KEY_PAGE_UP),
        Some(shared::ProductKey::PRODUCT_KEY_PAGE_DOWN) => Ok(ProductKey::PRODUCT_KEY_PAGE_DOWN),
        _ => Err(ConnectError::new(
            ErrorCode::InvalidArgument,
            "key must be ok, page-up, or page-down",
        )),
    }
}

fn rpc_err(error: impl std::fmt::Display) -> ConnectError {
    ConnectError::new(ErrorCode::Internal, error.to_string())
}

#[allow(refining_impl_trait)]
impl<T, F> connect_svc::RemoteDebugControlService for Owner<T, F>
where
    T: Transport + Send + 'static,
    F: FnMut(&control::ConnectRequest) -> Result<Session<T>, Error> + Send + 'static,
{
    async fn connect(
        &self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, control::ConnectRequest>,
    ) -> ServiceResult<control::ConnectResponse> {
        off_runtime(|| {
            let mut req = request.to_owned_message();
            req.target = target_name(&req.target);
            let mut inner = lock_inner(&self.0)?;
            let slot = slot_mut(&mut inner, &req.target);
            if slot.session.is_some() {
                let (_, phase, connected) = status_fields(slot);
                return Response::ok(control::ConnectResponse {
                    message: "already connected".into(),
                    phase: phase.into(),
                    connected,
                    nonce: slot.last_nonce,
                    last_log: last_log(slot),
                    ..control::ConnectResponse::default()
                });
            }
            if slot.pairing {
                return Response::ok(control::ConnectResponse {
                    message: "pairing".into(),
                    phase: control::SessionPhase::SESSION_PHASE_PAIRING.into(),
                    connected: false,
                    nonce: slot.last_nonce,
                    last_log: None,
                    ..control::ConnectResponse::default()
                });
            }
            start_pair(&self.0, slot, req);
            Response::ok(control::ConnectResponse {
                message: "pairing".into(),
                phase: control::SessionPhase::SESSION_PHASE_PAIRING.into(),
                connected: false,
                nonce: None,
                last_log: None,
                ..control::ConnectResponse::default()
            })
        })
    }

    async fn status(
        &self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, control::StatusRequest>,
    ) -> ServiceResult<control::StatusResponse> {
        off_runtime(|| {
            let target = target_name(request.target);
            let mut inner = lock_inner(&self.0)?;
            let Some(slot) = inner.targets.get_mut(&target) else {
                return Response::ok(control::StatusResponse {
                    message: "disconnected".into(),
                    phase: control::SessionPhase::SESSION_PHASE_DISCONNECTED.into(),
                    connected: false,
                    ..control::StatusResponse::default()
                });
            };
            let (message, phase, connected) = status_fields(slot);
            Response::ok(control::StatusResponse {
                message,
                phase: phase.into(),
                connected,
                nonce: slot.last_nonce,
                last_log: last_log(slot),
                ..control::StatusResponse::default()
            })
        })
    }

    async fn list_targets(
        &self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, control::ListTargetsRequest>,
    ) -> ServiceResult<control::ListTargetsResponse> {
        let inner = lock_inner(&self.0)?;
        let targets = inner
            .targets
            .iter()
            .map(|(target, slot)| control::TargetInfo {
                target: target.clone(),
                phase: phase_of(slot).into(),
                connected: slot.session.is_some(),
                ..control::TargetInfo::default()
            })
            .collect();
        Response::ok(control::ListTargetsResponse {
            targets,
            ..control::ListTargetsResponse::default()
        })
    }

    async fn inject_touch(
        &self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, control::InjectTouchRequest>,
    ) -> ServiceResult<control::InjectTouchResponse> {
        off_runtime(|| {
            let req = request.to_owned_message();
            let target = target_name(&req.target);
            let inject = Option::<shared::InjectTouch>::from(req.inject).ok_or_else(|| {
                ConnectError::new(ErrorCode::InvalidArgument, "inject-touch body required")
            })?;
            if inject.x > u32::from(u16::MAX) || inject.y > u32::from(u16::MAX) {
                return Err(ConnectError::new(ErrorCode::InvalidArgument, "coord"));
            }
            let slot_n = match inject.slot {
                None => None,
                Some(s) if s <= 4 => Some(s as u8),
                Some(_) => return Err(ConnectError::new(ErrorCode::InvalidArgument, "slot")),
            };
            let page = matches!(
                inject.space.as_known(),
                Some(shared::TouchSpace::TOUCH_SPACE_PAGE)
            );
            let phase = wire_phase(&inject);
            let mut inner = lock_inner(&self.0)?;
            let slot = slot_or_err(&mut inner, &target)?;
            session_mut(slot)?
                .inject_touch_ex(
                    TouchSample {
                        source: TouchSource::Synthetic,
                        x: inject.x as u16,
                        y: inject.y as u16,
                        slot: slot_n,
                    },
                    phase,
                    page,
                )
                .map_err(rpc_err)?;
            let nonce = slot.last_nonce;
            let last_log = last_log(slot);
            Response::ok(control::InjectTouchResponse {
                message: "inject-touch queued".into(),
                phase: control::SessionPhase::SESSION_PHASE_CONNECTED.into(),
                connected: true,
                nonce,
                last_log,
                ..control::InjectTouchResponse::default()
            })
        })
    }

    async fn inject_button(
        &self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, control::InjectButtonRequest>,
    ) -> ServiceResult<control::InjectButtonResponse> {
        off_runtime(|| {
            let req = request.to_owned_message();
            let target = target_name(&req.target);
            let inject = Option::<shared::InjectButton>::from(req.inject).ok_or_else(|| {
                ConnectError::new(ErrorCode::InvalidArgument, "inject-button body required")
            })?;
            let key = wire_key(&inject)?;
            let mut inner = lock_inner(&self.0)?;
            let slot = slot_or_err(&mut inner, &target)?;
            session_mut(slot)?
                .inject_button(key, inject.down)
                .map_err(rpc_err)?;
            Response::ok(control::InjectButtonResponse {
                message: "inject-button queued".into(),
                phase: control::SessionPhase::SESSION_PHASE_CONNECTED.into(),
                connected: true,
                nonce: slot.last_nonce,
                last_log: last_log(slot),
                ..control::InjectButtonResponse::default()
            })
        })
    }

    async fn get_snapshot(
        &self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, control::GetSnapshotRequest>,
    ) -> ServiceResult<control::GetSnapshotResponse> {
        off_runtime(|| {
            let req = request.to_owned_message();
            let target = target_name(&req.target);
            let nonce = req.nonce.unwrap_or_else(time_nonce);
            let mut inner = lock_inner(&self.0)?;
            let slot = slot_or_err(&mut inner, &target)?;
            match session_mut(slot)?.get_snapshot(nonce) {
                Ok(snap) => {
                    slot.last_nonce = Some(snap.nonce);
                    let wire = snapshot_proto(snap);
                    let last_log = last_log(slot);
                    if self.0.log {
                        eprintln!(
                            "remote-debug: snapshot {}x{} kind={}",
                            wire.width,
                            wire.height,
                            match wire.kind.as_known() {
                                Some(shared::FrameKind::FRAME_KIND_GRAY4) => "gray4",
                                _ => "mono",
                            }
                        );
                    }
                    Response::ok(control::GetSnapshotResponse {
                        message: format!(
                            "snapshot {}x{} kind={}",
                            wire.width,
                            wire.height,
                            match wire.kind.as_known() {
                                Some(shared::FrameKind::FRAME_KIND_GRAY4) => "gray4",
                                _ => "mono",
                            }
                        ),
                        phase: control::SessionPhase::SESSION_PHASE_CONNECTED.into(),
                        connected: true,
                        nonce: Some(wire.nonce),
                        last_log,
                        snapshot: wire.into(),
                        ..control::GetSnapshotResponse::default()
                    })
                }
                Err(remote_debug_host::Error::SnapshotBusy { armed }) => Err(ConnectError::new(
                    ErrorCode::FailedPrecondition,
                    format!("snapshot busy (armed={armed:#x})"),
                )),
                Err(error) => Err(rpc_err(error)),
            }
        })
    }

    async fn snapshot_ack(
        &self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, control::SnapshotAckRequest>,
    ) -> ServiceResult<control::SnapshotAckResponse> {
        off_runtime(|| {
            let req = request.to_owned_message();
            let target = target_name(&req.target);
            let mut inner = lock_inner(&self.0)?;
            let slot = slot_or_err(&mut inner, &target)?;
            let nonce = req.nonce.or(slot.last_nonce).ok_or_else(|| {
                ConnectError::new(
                    ErrorCode::FailedPrecondition,
                    "no nonce; pass --nonce or get-snapshot first",
                )
            })?;
            session_mut(slot)?.snapshot_ack(nonce).map_err(rpc_err)?;
            Response::ok(control::SnapshotAckResponse {
                message: "snapshot ack sent".into(),
                phase: control::SessionPhase::SESSION_PHASE_CONNECTED.into(),
                connected: true,
                nonce: Some(nonce),
                last_log: last_log(slot),
                ..control::SnapshotAckResponse::default()
            })
        })
    }

    async fn snapshot_clear(
        &self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, control::SnapshotClearRequest>,
    ) -> ServiceResult<control::SnapshotClearResponse> {
        off_runtime(|| {
            let req = request.to_owned_message();
            let target = target_name(&req.target);
            let mut inner = lock_inner(&self.0)?;
            let slot = slot_or_err(&mut inner, &target)?;
            session_mut(slot)?.snapshot_clear().map_err(rpc_err)?;
            Response::ok(control::SnapshotClearResponse {
                message: "snapshot clear".into(),
                phase: control::SessionPhase::SESSION_PHASE_CONNECTED.into(),
                connected: true,
                nonce: slot.last_nonce,
                last_log: last_log(slot),
                ..control::SnapshotClearResponse::default()
            })
        })
    }

    async fn reboot(
        &self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, control::RebootRequest>,
    ) -> ServiceResult<control::RebootResponse> {
        off_runtime(|| {
            let req = request.to_owned_message();
            let target = target_name(&req.target);
            let mut inner = lock_inner(&self.0)?;
            let slot = slot_or_err(&mut inner, &target)?;
            let mut session = slot.session.take().ok_or_else(|| {
                ConnectError::new(
                    ErrorCode::FailedPrecondition,
                    "not connected; run remote-debug connect first",
                )
            })?;
            session.reboot().map_err(rpc_err)?;
            slot.last_nonce = None;
            if req.no_reconnect {
                invalidate_pair(slot);
                return Response::ok(control::RebootResponse {
                    message: "device reboot sent; GATT session gone (embedded MCU, not this host)"
                        .into(),
                    phase: control::SessionPhase::SESSION_PHASE_DISCONNECTED.into(),
                    connected: false,
                    last_log: None,
                    ..control::RebootResponse::default()
                });
            }
            start_pair(
                &self.0,
                slot,
                control::ConnectRequest {
                    target: target.clone(),
                    pin: req.pin,
                    port: req.port,
                    remember: req.remember,
                    ..control::ConnectRequest::default()
                },
            );
            Response::ok(control::RebootResponse {
                message: "device rebooted; pairing".into(),
                phase: control::SessionPhase::SESSION_PHASE_PAIRING.into(),
                connected: false,
                last_log: None,
                ..control::RebootResponse::default()
            })
        })
    }

    async fn disconnect(
        &self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, control::DisconnectRequest>,
    ) -> ServiceResult<control::DisconnectResponse> {
        off_runtime(|| {
            let target = target_name(request.target);
            let mut inner = lock_inner(&self.0)?;
            let mut remaining = inner.targets.len();
            if let Some(mut slot) = inner.targets.remove(&target) {
                remaining = inner.targets.len();
                invalidate_pair(&mut slot);
                if let Some(mut session) = slot.session.take() {
                    session.disconnect().map_err(rpc_err)?;
                }
            }
            Response::ok(control::DisconnectResponse {
                message: "disconnected".into(),
                phase: control::SessionPhase::SESSION_PHASE_DISCONNECTED.into(),
                connected: false,
                remaining: remaining as u32,
                ..control::DisconnectResponse::default()
            })
        })
    }

    async fn shutdown(
        &self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, control::ShutdownRequest>,
    ) -> ServiceResult<control::ShutdownResponse> {
        request_shutdown(&self.0);
        Response::ok(control::ShutdownResponse {
            message: "shutdown".into(),
            ..control::ShutdownResponse::default()
        })
    }
}

fn slot_or_err<'a, T: Transport>(
    inner: &'a mut Inner<T>,
    target: &str,
) -> Result<&'a mut Slot<T>, ConnectError> {
    inner.targets.get_mut(target).ok_or_else(|| {
        ConnectError::new(
            ErrorCode::FailedPrecondition,
            "not connected; run remote-debug connect first",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use remote_debug_host::FakeTransport;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::thread;
    use std::time::Duration;

    fn serve_opts(dir: &Path) -> ServeOpts<'_> {
        ServeOpts {
            dir,
            listen: None,
            install_ctrlc: false,
            log: false,
            on_remember: None,
        }
    }

    fn spawn_owner(
        dir: PathBuf,
        opener: impl FnMut(&control::ConnectRequest) -> Result<Session<FakeTransport>, Error>
            + Send
            + 'static,
    ) -> thread::JoinHandle<Result<(), Error>> {
        thread::spawn(move || serve_with(serve_opts(&dir), opener))
    }

    fn open_tiny(_req: &control::ConnectRequest) -> Result<Session<FakeTransport>, Error> {
        Ok(Session::new(FakeTransport::tiny(), false))
    }

    fn wait_connected(client: &ControlClient, target: &str) -> control::StatusResponse {
        for _ in 0..80 {
            let status = client
                .status(control::StatusRequest {
                    target: target.into(),
                    ..control::StatusRequest::default()
                })
                .expect("status");
            if status.connected {
                return status;
            }
            if status.message.starts_with("pair failed") {
                panic!("{}", status.message);
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("timed out waiting for connected")
    }

    fn req(target: &str) -> control::ConnectRequest {
        control::ConnectRequest {
            target: target.into(),
            ..control::ConnectRequest::default()
        }
    }

    #[test]
    fn connect_pairs_and_client_hangup_keeps_gatt() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let owner = spawn_owner(dir.clone(), open_tiny);
        wait_for_broker(&dir, Duration::from_secs(2)).unwrap();

        let client = ControlClient::open(&dir).unwrap();
        let started = client.connect(req("sticky-rs")).unwrap();
        assert_eq!(started.message, "pairing");
        assert!(!started.connected);
        wait_connected(&client, "sticky-rs");
        drop(client);

        let again = ControlClient::open(&dir).unwrap();
        let status = again
            .status(control::StatusRequest {
                target: "sticky-rs".into(),
                ..control::StatusRequest::default()
            })
            .unwrap();
        assert!(status.connected, "{}", status.message);

        let listed = again
            .list_targets(control::ListTargetsRequest::default())
            .unwrap();
        assert_eq!(listed.targets.len(), 1);
        assert_eq!(listed.targets[0].target, "sticky-rs");
        assert!(listed.targets[0].connected);

        again.shutdown(control::ShutdownRequest::default()).unwrap();
        let _ = owner.join().unwrap();
    }

    #[test]
    fn snapshot_busy_is_failed_precondition() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let owner = spawn_owner(dir.clone(), open_tiny);
        wait_for_broker(&dir, Duration::from_secs(2)).unwrap();
        let client = ControlClient::open(&dir).unwrap();
        client.connect(req("")).unwrap();
        wait_connected(&client, "sticky-rs");

        let first = client
            .get_snapshot(control::GetSnapshotRequest {
                target: "sticky-rs".into(),
                nonce: Some(7),
                ..control::GetSnapshotRequest::default()
            })
            .unwrap();
        assert!(first.snapshot.is_set());
        assert_eq!(first.nonce, Some(7));

        let busy = client
            .get_snapshot(control::GetSnapshotRequest {
                target: "sticky-rs".into(),
                nonce: Some(8),
                ..control::GetSnapshotRequest::default()
            })
            .unwrap_err();
        assert!(busy.to_string().contains("snapshot busy"), "{busy}");

        client
            .snapshot_clear(control::SnapshotClearRequest {
                target: "sticky-rs".into(),
                ..control::SnapshotClearRequest::default()
            })
            .unwrap();
        client
            .shutdown(control::ShutdownRequest::default())
            .unwrap();
        let _ = owner.join().unwrap();
    }

    #[test]
    fn disconnect_drops_one_session_shutdown_stops_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let owner = spawn_owner(dir.clone(), open_tiny);
        wait_for_broker(&dir, Duration::from_secs(2)).unwrap();
        let client = ControlClient::open(&dir).unwrap();
        client.connect(req("alpha")).unwrap();
        wait_connected(&client, "alpha");
        client.connect(req("beta")).unwrap();
        wait_connected(&client, "beta");

        let listed = client
            .list_targets(control::ListTargetsRequest::default())
            .unwrap();
        assert_eq!(listed.targets.len(), 2);

        let gone = client
            .disconnect(control::DisconnectRequest {
                target: "alpha".into(),
                ..control::DisconnectRequest::default()
            })
            .unwrap();
        assert_eq!(gone.remaining, 1);
        assert!(endpoint_path(&dir).exists());

        let leftover = client
            .status(control::StatusRequest {
                target: "beta".into(),
                ..control::StatusRequest::default()
            })
            .unwrap();
        assert!(leftover.connected);

        client
            .shutdown(control::ShutdownRequest::default())
            .unwrap();
        let _ = owner.join().unwrap();
        assert!(!endpoint_path(&dir).exists());
    }

    #[test]
    fn remember_hook_runs_after_pair() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let hits = Arc::new(AtomicUsize::new(0));
        let hook_hits = Arc::clone(&hits);
        let on_remember: RememberHook = Arc::new(move |_req| {
            hook_hits.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        });
        let opts_dir = dir.clone();
        let owner = thread::spawn(move || {
            serve_with(
                ServeOpts {
                    dir: &opts_dir,
                    listen: None,
                    install_ctrlc: false,
                    log: false,
                    on_remember: Some(on_remember),
                },
                open_tiny,
            )
        });
        wait_for_broker(&dir, Duration::from_secs(2)).unwrap();
        let client = ControlClient::open(&dir).unwrap();
        client
            .connect(control::ConnectRequest {
                target: "sticky-rs".into(),
                remember: true,
                ..control::ConnectRequest::default()
            })
            .unwrap();
        wait_connected(&client, "sticky-rs");
        assert_eq!(hits.load(AtomicOrdering::SeqCst), 1);
        client
            .shutdown(control::ShutdownRequest::default())
            .unwrap();
        let _ = owner.join().unwrap();
    }
}
