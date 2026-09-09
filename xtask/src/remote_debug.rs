//! `cargo xtask remote-debug` — CLI and MCP clients of the ConnectRPC owner.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clap::{Args, Parser, Subcommand};
use clap_mcp::{
    parse_or_serve_mcp_with_state, AsStructured, ClapMcp, ClapMcpConfigProvider, ClapMcpRunOptions,
};
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
Later leaves are RPC. `serve` is an optional foreground log. \
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

/// Root used so `cargo xtask remote-debug --mcp` is valid argv.
#[derive(Debug, Parser, ClapMcp)]
#[clap_mcp(
    reinvocation_safe,
    stateful,
    parallel_safe = false,
    skip_root_when_subcommands
)]
#[command(name = "xtask", about = "Live BLE remote-debug", long_about = ABOUT)]
pub struct RemoteDebugMcpRoot {
    /// `remote-debug` only (never flash-app / restore).
    #[command(subcommand)]
    pub command: RemoteDebugGate,
}

/// Gate so tools live under `remote-debug_*` and the root xtask is not MCP.
#[derive(Debug, Subcommand, ClapMcp)]
#[clap_mcp(reinvocation_safe, skip_root_when_subcommands)]
#[clap_mcp_output_from_with_state = "run_gate"]
#[clap_mcp_state_type = "Mutex<RemoteDebugState>"]
pub enum RemoteDebugGate {
    /// Live BLE remote-debug session
    #[command(long_about = ABOUT)]
    RemoteDebug {
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

/// Leaf tools. Broker owns GATT (`reinvocation_safe`); one serialized link.
#[derive(Debug, Clone, Subcommand, ClapMcp)]
#[clap_mcp(reinvocation_safe, parallel_safe = false)]
#[clap_mcp_output_from_with_state = "run"]
#[clap_mcp_state_type = "Mutex<RemoteDebugState>"]
#[clap_mcp_output_type = "serde_json::Value"]
pub enum RemoteDebugCommand {
    /// Foreground broker (desk log). Ctrl-C disconnects
    Serve(ServeArgs),
    /// Start pair (returns `pairing`; poll `status` until `connected`)
    #[command(long_about = "\
Start the ConnectRPC owner if needed and begin BlueZ Connect (not Pair()). \
Returns pairing immediately. Poll status until connected or pair failed. \
UART auto-PIN unless --pin. Stay on splash. No --remember unless asked. \
Never a MAC. After a fresh flash, retry on le-connection-abort-by-local \
or CDC busy.")]
    #[clap_mcp(open_world)]
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
Synthetic product-key edge. --key ok / page-up / page-down. --down true \
is a short press. Wait ~2–3 s for compose before the next inject or \
get-snapshot. Seven page-downs from splash reach scene=targets.")]
    InjectButton(InjectButtonArgs),
    /// Arm LAST DRAW; write page-space PNG plus scene / expect
    #[command(long_about = "\
Arm the frozen LAST DRAW slot. Writes snap-<hex>.bw/.red and a page-space \
.png (portrait 480×800 or landscape 800×480 from hold). Message includes \
scene / hold / step / expect / last_log. Tap --page at expect. A leftover \
arm is SnapshotBusy; snapshot-clear then retry. Ack when done.")]
    GetSnapshot(GetSnapshotArgs),
    /// Release the snapshot slot
    #[clap_mcp(idempotent)]
    SnapshotAck(SnapshotAckArgs),
    /// Operator abort (no nonce); use after a failed get
    #[clap_mcp(idempotent)]
    SnapshotClear(BrokerTarget),
    /// `pairing` / `connected` / `disconnected` / `pair failed`
    #[clap_mcp(read_only, idempotent)]
    Status(BrokerTarget),
    /// Advertise names the owner currently tracks
    #[clap_mcp(read_only, idempotent)]
    ListTargets(ListTargetsArgs),
    /// Software-reset the embedded MCU (not this host)
    #[command(long_about = "\
Software-reset the embedded MCU, not this host. GATT dies. Leftover BlueZ \
LTK must not be reused. Re-pairs unless --no-reconnect. Stay on splash.")]
    #[clap_mcp(destructive, open_world)]
    Reboot(RebootArgs),
    /// Drop GATT and stop the broker; unknown units lose the BlueZ bond
    #[clap_mcp(destructive)]
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
    /// Press (`true`) or release.
    #[arg(long, default_value_t = true)]
    pub down: bool,
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
    fn new() -> Self {
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
            println!("{}", message_of(&out.0));
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
    let state = Arc::new(Mutex::new(RemoteDebugState::new()));
    let parsed = parse_or_serve_mcp_with_state::<RemoteDebugMcpRoot>(
        ClapMcpRunOptions {
            config: RemoteDebugMcpRoot::clap_mcp_config(),
            serve: crate::remote_debug_mcp::serve_options(),
        },
        state.clone(),
    );
    match parsed.command {
        RemoteDebugGate::RemoteDebug {
            command: Some(RemoteDebugCommand::Serve(args)),
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
        } => match run(command, state.as_ref()) {
            Ok(out) => {
                println!("{}", message_of(&out.0));
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        RemoteDebugGate::RemoteDebug { command: None } => {
            eprintln!("remote-debug needs a leaf (or --mcp)");
            ExitCode::FAILURE
        }
    }
}

/// MCP gate `run` (unwraps to the leaf).
fn run_gate(
    cmd: RemoteDebugGate,
    state: &Mutex<RemoteDebugState>,
) -> Result<AsStructured<Value>, String> {
    run(cmd.into_leaf()?, state)
}

impl RemoteDebugGate {
    fn into_leaf(self) -> Result<RemoteDebugCommand, String> {
        match self {
            Self::RemoteDebug {
                command: Some(command),
            } => Ok(command),
            Self::RemoteDebug { command: None } => {
                Err("remote-debug needs a leaf (or --mcp)".into())
            }
        }
    }
}

/// Shared CLI + MCP dispatch (RPC). `serve` as an MCP tool is refused.
pub fn run(
    cmd: RemoteDebugCommand,
    state: &Mutex<RemoteDebugState>,
) -> Result<AsStructured<Value>, String> {
    if matches!(cmd, RemoteDebugCommand::Serve(_)) {
        return Err("use a terminal; connect auto-starts the owner".into());
    }
    let guard = state.lock().map_err(|_| "session lock".to_string())?;
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
        RemoteDebugCommand::Status(target) => value(client.status(target.into())),
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
            if resp.snapshot.is_set() {
                let snap = &*resp.snapshot;
                let red = if snap.red.is_empty() {
                    None
                } else {
                    Some(snap.red.as_slice())
                };
                let path =
                    write_snapshot_planes(&guard.layout, snap.nonce, &snap.bw, red, snap.hold)
                        .map_err(map_host)?;
                resp.message = decorate_snapshot(
                    format!("{} wrote {}", resp.message, path.display()),
                    snap,
                    resp.last_log.as_deref(),
                );
            }
            json_value(resp)
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
            down: args.down,
            ..shared::InjectButton::default()
        }
        .into(),
        ..control::InjectButtonRequest::default()
    })
}

fn decorate_snapshot(
    mut message: String,
    snap: &shared::Snapshot,
    last_log: Option<&str>,
) -> String {
    if let Some(scene) = snap.scene {
        message.push_str(&format!(" scene={scene}"));
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

fn value<T: serde::Serialize>(
    result: Result<T, remote_debug_broker::Error>,
) -> Result<AsStructured<Value>, String> {
    json_value(result.map_err(map_broker)?)
}

fn json_value<T: serde::Serialize>(body: T) -> Result<AsStructured<Value>, String> {
    serde_json::to_value(body)
        .map(AsStructured)
        .map_err(|error| error.to_string())
}

fn message_of(value: &Value) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("ok")
        .to_string()
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
    use clap_mcp::{schema_from_command_with_metadata, ClapMcpSchemaMetadataProvider};
    use sticky_host::NO_BROKER;

    #[test]
    fn mcp_schema_exposes_leaf_tools_and_not_flash_app() {
        let schema = schema_from_command_with_metadata(
            &RemoteDebugMcpRoot::command(),
            &RemoteDebugMcpRoot::clap_mcp_schema_metadata(),
        );
        let leaves: Vec<_> = schema
            .root
            .all_commands()
            .into_iter()
            .filter(|c| c.subcommands.is_empty())
            .map(|c| c.name.clone())
            .collect();
        assert!(
            leaves.iter().any(|n| n.contains("connect")),
            "leaves={leaves:?}"
        );
        assert!(
            leaves.iter().any(|n| n.contains("serve")),
            "leaves={leaves:?}"
        );
        assert!(
            leaves
                .iter()
                .any(|n| n.contains("get-snapshot") || n.contains("get_snapshot")),
            "leaves={leaves:?}"
        );
        assert!(
            !leaves.iter().any(|n| n.contains("flash")),
            "flash-app must not be an MCP tool: {leaves:?}"
        );
        assert!(
            !leaves.iter().any(|n| n.contains("restore")),
            "restore must not be an MCP tool: {leaves:?}"
        );
        assert!(
            leaves.iter().any(|n| n.contains("reboot")),
            "leaves={leaves:?}"
        );
        assert!(
            leaves
                .iter()
                .any(|n| n.contains("list-targets") || n.contains("list_targets")),
            "leaves={leaves:?}"
        );
    }

    #[test]
    fn remote_debug_connect_parses_remember() {
        let cli =
            RemoteDebugMcpRoot::try_parse_from(["xtask", "remote-debug", "connect", "--remember"])
                .expect("parse");
        match cli.command {
            RemoteDebugGate::RemoteDebug { command } => match command {
                Some(RemoteDebugCommand::Connect(args)) => {
                    assert!(args.remember);
                    assert!(args.pin.is_none());
                }
                other => panic!("{other:?}"),
            },
        }
    }

    #[test]
    fn remote_debug_serve_parses_without_name() {
        let cli =
            RemoteDebugMcpRoot::try_parse_from(["xtask", "remote-debug", "serve"]).expect("parse");
        match cli.command {
            RemoteDebugGate::RemoteDebug { command } => match command {
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
            socket_dir: None,
        });
        assert_eq!(req.target, "sticky-rs");
        assert_eq!(req.pin, Some(123456));
        assert!(req.remember);
    }

    #[test]
    fn remote_debug_without_leaf_parses_for_mcp_attach() {
        let cli = RemoteDebugMcpRoot::try_parse_from(["xtask", "remote-debug"]).expect("parse");
        match cli.command {
            RemoteDebugGate::RemoteDebug { command: None } => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn map_host_strips_prefix_on_no_broker() {
        assert_eq!(map_host(Error::RemoteDebug(NO_BROKER.into())), NO_BROKER);
    }
}
