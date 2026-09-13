//! `cargo xtask remote-debug` — CLI and MCP clients of the ConnectRPC owner.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use remote_debug_host::DEFAULT_ADV_NAME;
use serde_json::Value;
use sticky_host::{
    broker_runtime_dir, control, ensure_broker, refuse_if_legacy_backups_at_repo_root,
    resolve_broker_exe, serve_live, shared, write_snapshot_planes, ControlClient, Error, Layout,
};

use crate::cli::repo_root;

/// Live BLE remote-debug. Pair then hold. Encrypted GATT. Never a MAC.
pub const ABOUT: &str = "\
Live BLE remote-debug (encrypted GATT after DisplayOnly pair). Advertise \
name `sticky-rs`. BlueZ Connect, not Pair(). A ConnectRPC owner holds \
0..N GATT sessions (assembler, last nonce, remember-me) keyed by \
advertise name, never a MAC. `connect` starts a detached owner if needed \
and returns `pairing`; poll `status` until `connected` or `pair failed`. \
CLI `connect --wait` polls in this process (MCP connect stays pairing). \
Later leaves are RPC. `logs` drains the Target / Scene ring. \
`serve` is an optional foreground log. \
`disconnect` drops one session; `Shutdown` (empty map / sit over, or \
serve Ctrl-C) ends the owner. Default `connect` scrapes a new UART \
`pair pin=` when a Sticky CH343 is present (UART lock only for that \
scrape; fails if `monitor` holds it). `--pin` skips UART. `--remember` \
allowlists this unit (factory / USB serial in gitignored \
developer-data/remote-debug/; never a MAC, never the PIN).

`reboot` software-resets the embedded MCU (not this host) and re-pairs \
unless `--no-reconnect`. After reset, UART may reprint `pair pin=` every \
5s on splash or the pair card until `pair ok`.

`--mcp` is the same client (this subtree only; not flash-app / restore). \
Do not also run `monitor` during auto-PIN. `inject-touch --page` is page \
pixels. Snapshot PNG is page space. Read `sticky-rs://remote-debug/pickup`.";

/// Full-argv parser for `cargo xtask remote-debug` (including `--mcp`).
#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Live BLE remote-debug", long_about = ABOUT)]
pub struct RemoteDebugAttach {
    /// `remote-debug` only (never flash-app / restore).
    #[command(subcommand)]
    pub command: RemoteDebugGate,
}

/// Gate so `--mcp` and leaves stay under `remote-debug`, not the full CLI.
#[derive(Debug, Subcommand)]
pub enum RemoteDebugGate {
    /// Live BLE remote-debug session
    #[command(name = "remote-debug", long_about = ABOUT)]
    RemoteDebug {
        /// Serve stdio MCP (this subtree only; never flash-app / restore).
        #[arg(long)]
        mcp: bool,
        /// Session command. Optional so `remote-debug --mcp` can attach.
        #[command(subcommand)]
        command: Option<RemoteDebugCommand>,
    },
}

/// Nested remote-debug parser for `cargo xtask --help` (not the MCP root).
#[derive(Debug, Args)]
pub struct RemoteDebugCli {
    /// Session command.
    #[command(subcommand)]
    pub command: RemoteDebugCommand,
}

/// Advertise name + hidden owner dir (same owner for every leaf).
#[derive(Debug, Clone, Args)]
pub struct BrokerTarget {
    /// Advertise name (`target`; default `sticky-rs`). Never a MAC.
    #[arg(long, default_value = DEFAULT_ADV_NAME)]
    pub name: String,
    /// Runtime dir for the owner endpoint (tests).
    #[arg(long, hide = true)]
    pub socket_dir: Option<PathBuf>,
}

/// Leaf tools. Broker owns GATT; one serialized link.
#[derive(Debug, Clone, Subcommand)]
pub enum RemoteDebugCommand {
    /// Foreground broker (desk log). Ctrl-C disconnects
    Serve(ServeArgs),
    /// Start pair (returns `pairing`; poll `status` until `connected`)
    #[command(long_about = "\
Start the ConnectRPC owner if needed and begin BlueZ Connect (not Pair()). \
Returns pairing immediately. Poll status until connected or pair failed. \
CLI --wait polls here (~60 s). MCP connect has no --wait. \
UART auto-PIN unless --pin. Stay on splash. No --remember unless asked. \
Never a MAC. After a fresh flash, retry on le-connection-abort-by-local \
or CDC busy.")]
    Connect(ConnectArgs),
    /// Synthetic tap. --page is page pixels; --phase for slides
    #[command(long_about = "\
Synthetic tap. Default --x/--y are pre-rotation framebuffer. --page treats \
them as page pixels for the last compose hold (gray4 hit-test inverse, \
same origin as snapshot expect / page PNG). --phase down/move/up; unset \
is a tap. Not UART p0=. Wait ~2–3 s for compose. Do not tap Wi-Fi START \
unless asked.")]
    InjectTouch(InjectTouchArgs),
    /// Short-press ok / page-up / page-down
    #[command(long_about = "\
Synthetic product-key edge. --key ok / page-up / page-down. Default is \
a short press. --release sends the up edge. Wait ~2–3 s for compose \
before the next inject or get-snapshot. Seven page-downs from splash \
reach scene=targets.")]
    InjectButton(InjectButtonArgs),
    /// Arm LAST DRAW; write page-space PNG plus scene / expect
    #[command(long_about = "\
Arm the frozen LAST DRAW slot. Writes a page-space .png (open that; \
portrait 480×800 or landscape 800×480 from hold). Sibling .bw and .red \
are packed SSD1677 planes (48 KiB each); .red is the second gray4 plane, \
not pigment. Structured JSON omits those bytes and adds png (absolute \
path). Message includes png= / scene / hold / step / expect / last_log. \
Tap --page at expect. A leftover arm is SnapshotBusy; snapshot-clear \
then retry. Ack when done.")]
    GetSnapshot(GetSnapshotArgs),
    /// Release the snapshot slot
    SnapshotAck(SnapshotAckArgs),
    /// Operator abort (no nonce); use after a failed get
    SnapshotClear(BrokerTarget),
    /// `pairing` / `connected` / `disconnected` / `pair failed`
    Status(BrokerTarget),
    /// Drain Target / Scene LogLine copies (never a PIN or MAC)
    #[command(long_about = "\
Drain GATT LogLine copies (Target / Scene only; never a PIN or MAC). \
Oldest first. --limit caps the print (default 16). last_log is the \
newest line. scene / hold / expect on the line is the last snapshot \
cache, stale after inject.")]
    Logs(LogsArgs),
    /// Advertise names the owner currently tracks
    ListTargets(ListTargetsArgs),
    /// Software-reset the embedded MCU (not this host)
    #[command(long_about = "\
Software-reset the embedded MCU, not this host. GATT dies. Leftover BlueZ \
LTK must not be reused. Re-pairs unless --no-reconnect. Stay on splash.")]
    Reboot(RebootArgs),
    /// Drop GATT and stop the broker; unknown units lose the BlueZ bond
    Disconnect(BrokerTarget),
}

/// `serve` flags.
#[derive(Debug, Clone, Args)]
pub struct ServeArgs {
    /// Runtime dir for the owner endpoint (tests).
    #[arg(long, hide = true)]
    pub socket_dir: Option<PathBuf>,
}

/// `list-targets` flags.
#[derive(Debug, Clone, Args)]
pub struct ListTargetsArgs {
    /// Runtime dir for the owner endpoint (tests).
    #[arg(long, hide = true)]
    pub socket_dir: Option<PathBuf>,
}

/// `connect` flags.
#[derive(Debug, Clone, Args)]
pub struct ConnectArgs {
    /// Six-digit PIN (skip UART).
    #[arg(long)]
    pub pin: Option<u32>,
    /// Serial device. Also `ESPFLASH_PORT`. Optional if one Sticky CH343.
    #[arg(long, env = "ESPFLASH_PORT", hide_env_values = true)]
    pub port: Option<String>,
    /// Advertise name (default `sticky-rs`).
    #[arg(long, default_value = DEFAULT_ADV_NAME)]
    pub name: String,
    /// Keep the BlueZ bond; write this unit into gitignored allowlist.
    #[arg(long)]
    pub remember: bool,
    /// Poll status until connected or pair failed (CLI only; ~60 s).
    #[arg(long)]
    pub wait: bool,
    /// Runtime dir for the owner endpoint (tests).
    #[arg(long, hide = true)]
    pub socket_dir: Option<PathBuf>,
}

/// Framebuffer tap (not UART `p0=`).
#[derive(Debug, Clone, Args)]
pub struct InjectTouchArgs {
    /// Framebuffer X (pre-rotation 800×480).
    #[arg(long)]
    pub x: u16,
    /// Framebuffer Y.
    #[arg(long)]
    pub y: u16,
    /// Optional GT911 slot 0..=4.
    #[arg(long)]
    pub slot: Option<u8>,
    /// `down` / `move` / `up`. Unset is a tap.
    #[arg(long)]
    pub phase: Option<String>,
    /// Treat `--x` / `--y` as page pixels for the last compose hold.
    #[arg(long)]
    pub page: bool,
    /// Advertise name (default `sticky-rs`).
    #[arg(long, default_value = DEFAULT_ADV_NAME)]
    pub name: String,
    /// Runtime dir for the owner endpoint (tests).
    #[arg(long, hide = true)]
    pub socket_dir: Option<PathBuf>,
}

/// Product key (`ok` / `page-up` / `page-down`).
#[derive(Debug, Clone, Args)]
pub struct InjectButtonArgs {
    /// `ok`, `page-up`, or `page-down`.
    #[arg(long)]
    pub key: String,
    /// Send the up edge instead of a short press.
    #[arg(long)]
    pub release: bool,
    /// Advertise name (default `sticky-rs`).
    #[arg(long, default_value = DEFAULT_ADV_NAME)]
    pub name: String,
    /// Runtime dir for the owner endpoint (tests).
    #[arg(long, hide = true)]
    pub socket_dir: Option<PathBuf>,
}

/// `logs` flags.
#[derive(Debug, Clone, Args)]
pub struct LogsArgs {
    /// Max lines (default 16, the host ring depth).
    #[arg(long)]
    pub limit: Option<u32>,
    /// Advertise name (default `sticky-rs`).
    #[arg(long, default_value = DEFAULT_ADV_NAME)]
    pub name: String,
    /// Runtime dir for the owner endpoint (tests).
    #[arg(long, hide = true)]
    pub socket_dir: Option<PathBuf>,
}

/// Host-chosen nonce (non-zero).
#[derive(Debug, Clone, Args)]
pub struct GetSnapshotArgs {
    /// `fixed64` nonce. Omit to use a time-based value.
    #[arg(long)]
    pub nonce: Option<u64>,
    /// Advertise name (default `sticky-rs`).
    #[arg(long, default_value = DEFAULT_ADV_NAME)]
    pub name: String,
    /// Runtime dir for the owner endpoint (tests).
    #[arg(long, hide = true)]
    pub socket_dir: Option<PathBuf>,
}

/// Ack the armed nonce.
#[derive(Debug, Clone, Args)]
pub struct SnapshotAckArgs {
    /// Nonce from the last get (default: last get).
    #[arg(long)]
    pub nonce: Option<u64>,
    /// Advertise name (default `sticky-rs`).
    #[arg(long, default_value = DEFAULT_ADV_NAME)]
    pub name: String,
    /// Runtime dir for the owner endpoint (tests).
    #[arg(long, hide = true)]
    pub socket_dir: Option<PathBuf>,
}

/// Reset the **embedded MCU**, then optionally Connect again.
#[derive(Debug, Clone, Args)]
pub struct RebootArgs {
    /// Do not re-pair after the device reset.
    #[arg(long)]
    pub no_reconnect: bool,
    /// Six-digit PIN for `--reconnect` (skip UART).
    #[arg(long)]
    pub pin: Option<u32>,
    /// Serial device. Also `ESPFLASH_PORT`. Optional if one Sticky CH343.
    #[arg(long, env = "ESPFLASH_PORT", hide_env_values = true)]
    pub port: Option<String>,
    /// Advertise name (default `sticky-rs`).
    #[arg(long, default_value = DEFAULT_ADV_NAME)]
    pub name: String,
    /// Keep the BlueZ bond after the new pair; write this unit into allowlist.
    #[arg(long)]
    pub remember: bool,
    /// Runtime dir for the owner endpoint (tests).
    #[arg(long, hide = true)]
    pub socket_dir: Option<PathBuf>,
}

/// Layout for snapshot plane writes. GATT lives in the broker.
pub struct RemoteDebugState {
    layout: Layout,
}

impl RemoteDebugState {
    pub(crate) fn new() -> Self {
        Self {
            layout: Layout::from_repo_root(repo_root()),
        }
    }
}

/// CLI dispatch when `Cli` parsed `remote-debug` (no `--mcp`).
pub fn run_cli(cli: RemoteDebugCli) -> Result<(), Error> {
    match cli.command {
        RemoteDebugCommand::Serve(args) => {
            let layout = Layout::from_repo_root(repo_root());
            serve_live(&layout, args.socket_dir.as_deref())
        }
        command => {
            let state = Mutex::new(RemoteDebugState::new());
            let out = run(command, &state).map_err(Error::RemoteDebug)?;
            println!("{}", message_of(&out));
            Ok(())
        }
    }
}

/// Parse argv or serve MCP. `cargo xtask remote-debug --mcp`.
pub fn exec() -> ExitCode {
    let repo = repo_root();
    if let Err(error) = refuse_if_legacy_backups_at_repo_root(&repo) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    let parsed = RemoteDebugAttach::parse();
    match parsed.command {
        RemoteDebugGate::RemoteDebug { mcp: true, .. } => crate::remote_debug_mcp::serve(),
        RemoteDebugGate::RemoteDebug {
            command: Some(RemoteDebugCommand::Serve(args)),
            ..
        } => {
            let layout = Layout::from_repo_root(repo);
            match serve_live(&layout, args.socket_dir.as_deref()) {
                Ok(()) => {
                    println!("disconnected");
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        RemoteDebugGate::RemoteDebug {
            command: Some(command),
            ..
        } => {
            let wait_connect = match &command {
                RemoteDebugCommand::Connect(args) if args.wait => {
                    Some((args.name.clone(), args.socket_dir.clone()))
                }
                _ => None,
            };
            let state = Mutex::new(RemoteDebugState::new());
            match run(command, &state) {
                Ok(out) => {
                    if let Some((name, socket_dir)) = wait_connect {
                        match wait_until_connected(&name, socket_dir.as_deref()) {
                            Ok(line) => {
                                println!("{line}");
                                if line.starts_with("pair failed") {
                                    ExitCode::FAILURE
                                } else {
                                    ExitCode::SUCCESS
                                }
                            }
                            Err(error) => {
                                eprintln!("{error}");
                                ExitCode::FAILURE
                            }
                        }
                    } else {
                        println!("{}", message_of(&out));
                        ExitCode::SUCCESS
                    }
                }
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        RemoteDebugGate::RemoteDebug { command: None, .. } => {
            eprintln!("remote-debug needs a leaf (or --mcp)");
            ExitCode::FAILURE
        }
    }
}

/// Shared CLI + MCP dispatch (RPC). `serve` as an MCP tool is refused.
pub fn run(cmd: RemoteDebugCommand, state: &Mutex<RemoteDebugState>) -> Result<Value, String> {
    if matches!(cmd, RemoteDebugCommand::Serve(_)) {
        return Err("use a terminal; connect auto-starts the owner".into());
    }
    let guard = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let default_dir = broker_runtime_dir();
    let dir_owned = socket_dir_of(&cmd);
    let dir = dir_owned.as_deref().unwrap_or(&default_dir);
    if matches!(cmd, RemoteDebugCommand::Connect(_)) {
        let current = std::env::current_exe().map_err(|error| error.to_string())?;
        let exe = resolve_broker_exe(&repo_root(), &current);
        ensure_broker(dir, &exe).map_err(map_host)?;
    }
    let client = ControlClient::open(dir).map_err(map_broker)?;
    match cmd {
        RemoteDebugCommand::Serve(_) => unreachable!("serve is not RPC"),
        RemoteDebugCommand::Connect(args) => value(client.connect(args.into())),
        RemoteDebugCommand::Status(target) => {
            let mut resp = client.status(target.into()).map_err(map_broker)?;
            resp.message = decorate_status(&resp);
            json_value(resp)
        }
        RemoteDebugCommand::Logs(args) => {
            let mut resp = client.get_logs(args.into()).map_err(map_broker)?;
            resp.message = decorate_logs(&resp);
            json_value(resp)
        }
        RemoteDebugCommand::ListTargets(_) => {
            value(client.list_targets(control::ListTargetsRequest::default()))
        }
        RemoteDebugCommand::InjectTouch(args) => {
            value(client.inject_touch(inject_touch_request(args)?))
        }
        RemoteDebugCommand::InjectButton(args) => {
            value(client.inject_button(inject_button_request(args)?))
        }
        RemoteDebugCommand::GetSnapshot(args) => {
            let mut resp = client.get_snapshot(args.into()).map_err(map_broker)?;
            let png = write_and_strip_snapshot(&guard.layout, &mut resp)?;
            json_value_with_png(resp, png)
        }
        RemoteDebugCommand::SnapshotAck(args) => value(client.snapshot_ack(args.into())),
        RemoteDebugCommand::SnapshotClear(target) => value(client.snapshot_clear(target.into())),
        RemoteDebugCommand::Reboot(args) => value(client.reboot(args.into())),
        RemoteDebugCommand::Disconnect(target) => {
            let resp = client.disconnect(target.into()).map_err(map_broker)?;
            if resp.remaining == 0 {
                let _ = client.shutdown(control::ShutdownRequest::default());
            }
            json_value(resp)
        }
    }
}

fn socket_dir_of(cmd: &RemoteDebugCommand) -> Option<PathBuf> {
    match cmd {
        RemoteDebugCommand::Serve(args) => args.socket_dir.clone(),
        RemoteDebugCommand::Connect(args) => args.socket_dir.clone(),
        RemoteDebugCommand::InjectTouch(args) => args.socket_dir.clone(),
        RemoteDebugCommand::InjectButton(args) => args.socket_dir.clone(),
        RemoteDebugCommand::GetSnapshot(args) => args.socket_dir.clone(),
        RemoteDebugCommand::SnapshotAck(args) => args.socket_dir.clone(),
        RemoteDebugCommand::SnapshotClear(target) => target.socket_dir.clone(),
        RemoteDebugCommand::Status(target) => target.socket_dir.clone(),
        RemoteDebugCommand::Logs(args) => args.socket_dir.clone(),
        RemoteDebugCommand::ListTargets(args) => args.socket_dir.clone(),
        RemoteDebugCommand::Reboot(args) => args.socket_dir.clone(),
        RemoteDebugCommand::Disconnect(target) => target.socket_dir.clone(),
    }
}

impl From<ConnectArgs> for control::ConnectRequest {
    fn from(args: ConnectArgs) -> Self {
        Self {
            target: args.name,
            pin: args.pin,
            port: args.port,
            remember: args.remember,
            ..Self::default()
        }
    }
}

impl From<BrokerTarget> for control::StatusRequest {
    fn from(target: BrokerTarget) -> Self {
        Self {
            target: target.name,
            ..Self::default()
        }
    }
}

impl From<BrokerTarget> for control::SnapshotClearRequest {
    fn from(target: BrokerTarget) -> Self {
        Self {
            target: target.name,
            ..Self::default()
        }
    }
}

impl From<LogsArgs> for control::GetLogsRequest {
    fn from(args: LogsArgs) -> Self {
        Self {
            target: args.name,
            limit: args.limit,
            ..Self::default()
        }
    }
}

impl From<BrokerTarget> for control::DisconnectRequest {
    fn from(target: BrokerTarget) -> Self {
        Self {
            target: target.name,
            ..Self::default()
        }
    }
}

impl From<GetSnapshotArgs> for control::GetSnapshotRequest {
    fn from(args: GetSnapshotArgs) -> Self {
        Self {
            target: args.name,
            nonce: args.nonce,
            ..Self::default()
        }
    }
}

impl From<SnapshotAckArgs> for control::SnapshotAckRequest {
    fn from(args: SnapshotAckArgs) -> Self {
        Self {
            target: args.name,
            nonce: args.nonce,
            ..Self::default()
        }
    }
}

impl From<RebootArgs> for control::RebootRequest {
    fn from(args: RebootArgs) -> Self {
        Self {
            target: args.name,
            no_reconnect: args.no_reconnect,
            pin: args.pin,
            port: args.port,
            remember: args.remember,
            ..Self::default()
        }
    }
}

fn inject_touch_request(args: InjectTouchArgs) -> Result<control::InjectTouchRequest, String> {
    let phase = match args.phase.as_deref() {
        None | Some("down") => shared::TouchPhase::TOUCH_PHASE_DOWN,
        Some("move") => shared::TouchPhase::TOUCH_PHASE_MOVE,
        Some("up") => shared::TouchPhase::TOUCH_PHASE_UP,
        Some(_) => return Err("phase must be down, move, or up".into()),
    };
    let space = if args.page {
        shared::TouchSpace::TOUCH_SPACE_PAGE
    } else {
        shared::TouchSpace::TOUCH_SPACE_FRAMEBUFFER
    };
    Ok(control::InjectTouchRequest {
        target: args.name,
        inject: shared::InjectTouch {
            x: u32::from(args.x),
            y: u32::from(args.y),
            slot: args.slot.map(u32::from),
            phase: phase.into(),
            space: space.into(),
            ..shared::InjectTouch::default()
        }
        .into(),
        ..control::InjectTouchRequest::default()
    })
}

fn inject_button_request(args: InjectButtonArgs) -> Result<control::InjectButtonRequest, String> {
    let key = match args.key.to_ascii_lowercase().as_str() {
        "ok" | "4" => shared::ProductKey::PRODUCT_KEY_OK,
        "page-up" | "pageup" | "5" => shared::ProductKey::PRODUCT_KEY_PAGE_UP,
        "page-down" | "pagedown" | "6" => shared::ProductKey::PRODUCT_KEY_PAGE_DOWN,
        _ => return Err("key must be ok, page-up, or page-down".into()),
    };
    Ok(control::InjectButtonRequest {
        target: args.name,
        inject: shared::InjectButton {
            key: key.into(),
            down: !args.release,
            ..shared::InjectButton::default()
        }
        .into(),
        ..control::InjectButtonRequest::default()
    })
}

/// Write LAST DRAW under `developer-data/remote-debug/snapshots/`,
/// point the control line at the page PNG, then drop plane bytes.
///
/// The ConnectRPC body still carries planes; this client writes
/// files and must not echo ~125 KiB of SSD1677 RAM back through
/// CLI / MCP. `Snapshot.red` is the second gray4 plane, not pigment.
fn write_and_strip_snapshot(
    layout: &Layout,
    resp: &mut control::GetSnapshotResponse,
) -> Result<Option<PathBuf>, String> {
    if !resp.snapshot.is_set() {
        return Ok(None);
    }
    let stem = {
        let snap = &*resp.snapshot;
        let red = if snap.red.is_empty() {
            None
        } else {
            Some(snap.red.as_slice())
        };
        write_snapshot_planes(layout, snap.nonce, &snap.bw, red, snap.hold).map_err(map_host)?
    };
    let png = stem.with_extension("png");
    let wrote = png.is_file().then_some(png);
    let line = match wrote.as_ref() {
        Some(path) => format!("{} png={}", resp.message, path.display()),
        None => resp.message.clone(),
    };
    resp.message = decorate_snapshot(line, &resp.snapshot, resp.last_log.as_deref());
    strip_snapshot_planes(resp);
    Ok(wrote)
}

/// Drop LAST DRAW bytes after they are written under
/// `developer-data/remote-debug/snapshots/`.
///
/// `Snapshot.red` is the second packed SSD1677 plane (gray4), not a
/// red pigment. Empty fields are omitted from JSON.
fn strip_snapshot_planes(resp: &mut control::GetSnapshotResponse) {
    if let Some(snap) = resp.snapshot.as_option_mut() {
        snap.bw.clear();
        snap.red.clear();
    }
}

/// ConnectRPC JSON plus a host-only `png` path (not on the wire).
fn json_value_with_png(
    resp: control::GetSnapshotResponse,
    png: Option<PathBuf>,
) -> Result<Value, String> {
    let mut value = serde_json::to_value(resp).map_err(|error| error.to_string())?;
    if let Some(png) = png {
        let object = value
            .as_object_mut()
            .ok_or_else(|| "get-snapshot json".to_string())?;
        object.insert(
            "png".into(),
            Value::String(png.to_string_lossy().into_owned()),
        );
    }
    Ok(value)
}

fn decorate_snapshot(
    mut message: String,
    snap: &shared::Snapshot,
    last_log: Option<&str>,
) -> String {
    if let Some(scene) = snap.scene {
        message.push(' ');
        message.push_str(&format_scene(scene));
    }
    if let Some(hold) = snap.hold {
        message.push_str(&format!(" hold={hold}"));
    }
    if let Some(step) = snap.target_step {
        message.push_str(&format!(" step={step}"));
    }
    if let (Some(x), Some(y)) = (snap.target_expect_x, snap.target_expect_y) {
        message.push_str(&format!(" expect={x},{y}"));
    }
    if let Some(log) = last_log {
        message.push_str(" last_log=");
        message.push_str(log);
    }
    message
}

fn format_scene(persist: u32) -> String {
    match u8::try_from(persist)
        .ok()
        .and_then(embassy_debug::Scene::from_persist_byte)
        .map(embassy_debug::Scene::as_str)
    {
        Some(name) => format!("scene={persist}({name})"),
        None => format!("scene={persist}"),
    }
}

fn append_view(
    message: &mut String,
    scene: Option<u32>,
    hold: Option<u32>,
    step: Option<u32>,
    expect_x: Option<u32>,
    expect_y: Option<u32>,
) {
    if let Some(scene) = scene {
        message.push(' ');
        message.push_str(&format_scene(scene));
    }
    if let Some(hold) = hold {
        message.push_str(&format!(" hold={hold}"));
    }
    if let Some(step) = step {
        message.push_str(&format!(" step={step}"));
    }
    if let (Some(x), Some(y)) = (expect_x, expect_y) {
        message.push_str(&format!(" expect={x},{y}"));
    }
}

fn decorate_status(resp: &control::StatusResponse) -> String {
    let mut message = resp.message.clone();
    append_view(
        &mut message,
        resp.last_scene,
        resp.last_hold,
        resp.last_target_step,
        resp.last_expect_x,
        resp.last_expect_y,
    );
    if let Some(log) = resp.last_log.as_deref() {
        message.push_str(" last_log=");
        message.push_str(log);
    }
    message
}

fn decorate_logs(resp: &control::GetLogsResponse) -> String {
    let mut message = resp.message.clone();
    append_view(
        &mut message,
        resp.last_scene,
        resp.last_hold,
        resp.last_target_step,
        resp.last_expect_x,
        resp.last_expect_y,
    );
    if let Some(log) = resp.last_log.as_deref() {
        message.push_str(" last_log=");
        message.push_str(log);
    }
    for line in &resp.lines {
        message.push('\n');
        message.push_str(&format!("t={} {}", line.t_ms, line.text));
    }
    message
}

fn wait_until_connected(name: &str, socket_dir: Option<&Path>) -> Result<String, String> {
    let default_dir = broker_runtime_dir();
    let dir = socket_dir.unwrap_or(&default_dir);
    let client = ControlClient::open(dir).map_err(map_broker)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let status = client
            .status(control::StatusRequest {
                target: name.to_string(),
                ..control::StatusRequest::default()
            })
            .map_err(map_broker)?;
        let line = decorate_status(&status);
        if status.connected || status.message.starts_with("pair failed") {
            return Ok(line);
        }
        if Instant::now() >= deadline {
            return Err(format!("connect --wait timed out ({line})"));
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn value<T: serde::Serialize>(
    result: Result<T, remote_debug_broker::Error>,
) -> Result<Value, String> {
    json_value(result.map_err(map_broker)?)
}

fn json_value<T: serde::Serialize>(body: T) -> Result<Value, String> {
    serde_json::to_value(body).map_err(|error| error.to_string())
}

fn message_of(value: &Value) -> String {
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        if !message.is_empty() {
            return message.to_string();
        }
    }
    if let Some(targets) = value.get("targets").and_then(Value::as_array) {
        let names: Vec<&str> = targets
            .iter()
            .filter_map(|target| target.get("target").and_then(Value::as_str))
            .collect();
        if names.is_empty() {
            return "no targets".into();
        }
        return format!("targets={}", names.join(","));
    }
    "ok".to_string()
}

fn map_broker(error: remote_debug_broker::Error) -> String {
    map_host(Error::from(error))
}

fn map_host(error: Error) -> String {
    let text = error.to_string();
    text.strip_prefix("remote-debug: ")
        .unwrap_or(&text)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use sticky_host::NO_BROKER;

    #[test]
    fn attach_parser_builds() {
        RemoteDebugAttach::command().debug_assert();
    }

    #[test]
    fn remote_debug_connect_parses_remember() {
        let cli =
            RemoteDebugAttach::try_parse_from(["xtask", "remote-debug", "connect", "--remember"])
                .expect("parse");
        match cli.command {
            RemoteDebugGate::RemoteDebug { mcp, command } => {
                assert!(!mcp);
                match command {
                    Some(RemoteDebugCommand::Connect(args)) => {
                        assert!(args.remember);
                        assert!(args.pin.is_none());
                    }
                    other => panic!("{other:?}"),
                }
            }
        }
    }

    #[test]
    fn remote_debug_serve_parses_without_name() {
        let cli =
            RemoteDebugAttach::try_parse_from(["xtask", "remote-debug", "serve"]).expect("parse");
        match cli.command {
            RemoteDebugGate::RemoteDebug { command, .. } => match command {
                Some(RemoteDebugCommand::Serve(args)) => {
                    assert!(args.socket_dir.is_none());
                }
                other => panic!("{other:?}"),
            },
        }
    }

    #[test]
    fn connect_args_map_to_control_request() {
        let req = control::ConnectRequest::from(ConnectArgs {
            pin: Some(123456),
            port: Some("/dev/ttyUSB0".into()),
            name: "sticky-rs".into(),
            remember: true,
            wait: false,
            socket_dir: None,
        });
        assert_eq!(req.target, "sticky-rs");
        assert_eq!(req.pin, Some(123456));
        assert!(req.remember);
    }

    #[test]
    fn remote_debug_without_leaf_parses_for_mcp_attach() {
        let cli =
            RemoteDebugAttach::try_parse_from(["xtask", "remote-debug", "--mcp"]).expect("parse");
        match cli.command {
            RemoteDebugGate::RemoteDebug {
                mcp: true,
                command: None,
            } => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn remote_debug_without_leaf_or_mcp_parses() {
        let cli = RemoteDebugAttach::try_parse_from(["xtask", "remote-debug"]).expect("parse");
        match cli.command {
            RemoteDebugGate::RemoteDebug {
                mcp: false,
                command: None,
            } => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn map_host_strips_prefix_on_no_broker() {
        assert_eq!(map_host(Error::RemoteDebug(NO_BROKER.into())), NO_BROKER);
    }

    #[test]
    fn message_of_lists_advertise_names() {
        let value = serde_json::json!({
            "targets": [
                {"target": "sticky-rs", "connected": true}
            ]
        });
        assert_eq!(message_of(&value), "targets=sticky-rs");
    }

    #[test]
    fn strip_snapshot_planes_clears_last_draw() {
        let mut resp = control::GetSnapshotResponse {
            snapshot: shared::Snapshot {
                bw: vec![1, 2, 3],
                red: vec![4],
                ..shared::Snapshot::default()
            }
            .into(),
            ..control::GetSnapshotResponse::default()
        };
        strip_snapshot_planes(&mut resp);
        assert!(resp.snapshot.bw.is_empty());
        assert!(resp.snapshot.red.is_empty());
        let value = serde_json::to_value(&resp).expect("json");
        let snap = value.get("snapshot").expect("snapshot");
        assert!(snap.get("bw").is_none(), "{snap}");
        assert!(snap.get("red").is_none(), "{snap}");
    }

    #[test]
    fn get_snapshot_json_adds_png_not_planes() {
        let mut resp = control::GetSnapshotResponse {
            message: "ok".into(),
            snapshot: shared::Snapshot {
                bw: vec![1, 2, 3],
                red: vec![4],
                scene: Some(7),
                ..shared::Snapshot::default()
            }
            .into(),
            ..control::GetSnapshotResponse::default()
        };
        strip_snapshot_planes(&mut resp);
        let png = PathBuf::from("/tmp/snap-demo.png");
        let value = json_value_with_png(resp, Some(png)).expect("json");
        assert_eq!(
            value.get("png").and_then(Value::as_str),
            Some("/tmp/snap-demo.png")
        );
        let snap = value.get("snapshot").expect("snapshot");
        assert!(snap.get("bw").is_none(), "{snap}");
        assert!(snap.get("red").is_none(), "{snap}");
        assert_eq!(snap.get("scene").and_then(Value::as_u64), Some(7));
    }

    #[test]
    fn inject_button_release_parses_and_down_is_gone() {
        let cli = RemoteDebugAttach::try_parse_from([
            "xtask",
            "remote-debug",
            "inject-button",
            "--key",
            "page-down",
        ])
        .expect("parse");
        match cli.command {
            RemoteDebugGate::RemoteDebug { command, .. } => match command {
                Some(RemoteDebugCommand::InjectButton(args)) => {
                    assert!(!args.release);
                    let req = inject_button_request(args).expect("map");
                    assert!(req.inject.down);
                }
                other => panic!("{other:?}"),
            },
        }
        let released = RemoteDebugAttach::try_parse_from([
            "xtask",
            "remote-debug",
            "inject-button",
            "--key",
            "ok",
            "--release",
        ])
        .expect("parse");
        match released.command {
            RemoteDebugGate::RemoteDebug { command, .. } => match command {
                Some(RemoteDebugCommand::InjectButton(args)) => {
                    assert!(args.release);
                    let req = inject_button_request(args).expect("map");
                    assert!(!req.inject.down);
                }
                other => panic!("{other:?}"),
            },
        }
        assert!(RemoteDebugAttach::try_parse_from([
            "xtask",
            "remote-debug",
            "inject-button",
            "--key",
            "ok",
            "--down",
            "true",
        ])
        .is_err());
    }

    #[test]
    fn connect_wait_parses_and_is_not_on_the_wire() {
        let cli = RemoteDebugAttach::try_parse_from(["xtask", "remote-debug", "connect", "--wait"])
            .expect("parse");
        match cli.command {
            RemoteDebugGate::RemoteDebug { command, .. } => match command {
                Some(RemoteDebugCommand::Connect(args)) => {
                    assert!(args.wait);
                    let req = control::ConnectRequest::from(args);
                    assert!(!req.remember);
                }
                other => panic!("{other:?}"),
            },
        }
    }

    #[test]
    fn format_scene_uses_persist_byte_and_uart_token() {
        assert_eq!(format_scene(0), "scene=0(splash)");
        assert_eq!(format_scene(1), "scene=1(shapes)");
        assert_eq!(format_scene(7), "scene=7(targets)");
        assert_eq!(format_scene(99), "scene=99");
    }

    #[test]
    fn decorate_snapshot_names_targets() {
        let snap = shared::Snapshot {
            scene: Some(7),
            hold: Some(0),
            target_step: Some(0),
            target_expect_x: Some(240),
            target_expect_y: Some(400),
            ..shared::Snapshot::default()
        };
        let line = decorate_snapshot("ok".into(), &snap, None);
        assert!(line.contains("scene=7(targets)"), "{line}");
        assert!(line.contains("expect=240,400"), "{line}");
    }
}
