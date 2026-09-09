//! `cargo xtask remote-debug` — CLI and MCP clients of the Unix-socket broker.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clap::{Args, Parser, Subcommand};
use clap_mcp::{AsStructured, ClapMcp, ParseOrServeMcpWithState};
use remote_debug_host::DEFAULT_ADV_NAME;
use serde::Serialize;
use sticky_host::{
    broker_runtime_dir, ensure_broker, refuse_if_legacy_backups_at_repo_root, resolve_broker_exe,
    rpc, serve_live, write_snapshot_planes, BrokerPhase, BrokerReply, BrokerRequest, ConnectReq,
    Error, Layout, RebootReq,
};

use crate::cli::repo_root;

/// Live BLE remote-debug. Pair then hold. Encrypted GATT. Never a MAC.
pub const ABOUT: &str = "\
Live BLE remote-debug (encrypted GATT after DisplayOnly pair). Advertise \
name `sticky-rs`. BlueZ Connect, not Pair(). A Unix-socket broker owns the \
GATT session (assembler, last nonce, remember-me). `connect` starts a \
detached broker if needed and returns `pairing`; poll `status` until \
`connected` or `pair failed`. Later leaves are RPC. \
`serve` is an optional foreground log. `disconnect` (or serve Ctrl-C) \
is the only GATT close. Default `connect` scrapes a new UART `pair pin=` when a Sticky \
CH343 is present (UART lock only for that scrape; fails if `monitor` \
holds it). `--pin` skips UART. `--remember` allowlists this unit \
(factory / USB serial in gitignored developer-data/remote-debug/; never \
a MAC, never the PIN).

`reboot` software-resets the embedded MCU (not this host) and re-pairs \
unless `--no-reconnect`. After reset, UART may reprint `pair pin=` every \
5s on splash or the pair card until `pair ok`.

`--mcp` is the same client (this subtree only; not flash-app / restore). \
Do not also run `monitor` during auto-PIN. Coordinates are framebuffer.";

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

/// Advertise name + hidden socket dir (same broker for every leaf).
#[derive(Debug, Clone, Args)]
pub struct BrokerTarget {
    /// Advertise name (socket key; default `sticky-rs`). Never a MAC.
    #[arg(long, default_value = DEFAULT_ADV_NAME)]
    pub name: String,
    /// Runtime dir for the broker socket (tests).
    #[arg(long, hide = true)]
    pub socket_dir: Option<PathBuf>,
}

/// Leaf tools. Broker owns GATT (`reinvocation_safe`); one serialized link.
#[derive(Debug, Clone, Subcommand, ClapMcp)]
#[clap_mcp(reinvocation_safe, parallel_safe = false)]
#[clap_mcp_output_from_with_state = "run"]
#[clap_mcp_state_type = "Mutex<RemoteDebugState>"]
#[clap_mcp_output_type = "RemoteDebugOutput"]
pub enum RemoteDebugCommand {
    /// Foreground broker (desk log). Ctrl-C disconnects
    Serve(ServeArgs),
    /// Start pair (returns `pairing`; poll `status` until `connected`)
    Connect(ConnectArgs),
    /// Synthetic framebuffer tap
    InjectTouch(InjectTouchArgs),
    /// Synthetic product-key edge
    InjectButton(InjectButtonArgs),
    /// Arm the frozen LAST planes
    GetSnapshot(GetSnapshotArgs),
    /// Release the snapshot slot
    SnapshotAck(SnapshotAckArgs),
    /// Operator abort (no nonce)
    SnapshotClear(BrokerTarget),
    /// `pairing` / `connected` / `disconnected` / `pair failed`
    Status(BrokerTarget),
    /// Software-reset the embedded MCU (not this host)
    Reboot(RebootArgs),
    /// Drop GATT and stop the broker; unknown units lose the BlueZ bond
    Disconnect(BrokerTarget),
}

/// `serve` flags.
#[derive(Debug, Clone, Args)]
pub struct ServeArgs {
    /// Advertise name (socket key; default `sticky-rs`). Never a MAC.
    #[arg(long, default_value = DEFAULT_ADV_NAME)]
    pub name: String,
    /// Runtime dir for the broker socket (tests).
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
    /// Runtime dir for the broker socket (tests).
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
    /// Runtime dir for the broker socket (tests).
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
    /// Runtime dir for the broker socket (tests).
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
    /// Runtime dir for the broker socket (tests).
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
    /// Runtime dir for the broker socket (tests).
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
    /// Runtime dir for the broker socket (tests).
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

/// Structured tool output (CLI prints `message`).
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct RemoteDebugOutput {
    /// Command succeeded.
    pub ok: bool,
    /// Human line (no MAC).
    pub message: String,
    /// Last snapshot nonce, if any.
    pub nonce: Option<u64>,
    /// Whether the broker holds GATT.
    pub connected: bool,
    /// `disconnected` / `pairing` / `connected`.
    pub phase: String,
    /// Last Target / Scene UART copy (never a MAC).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_log: Option<String>,
    /// `Scene::persist_byte` from the last get.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene: Option<u32>,
    /// Targets walk id from the last get.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_step: Option<u32>,
    /// Expected page X from the last get.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_expect_x: Option<u32>,
    /// Expected page Y from the last get.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_expect_y: Option<u32>,
}

/// CLI dispatch when `Cli` parsed `remote-debug` (no `--mcp`).
pub fn run_cli(cli: RemoteDebugCli) -> Result<(), Error> {
    match cli.command {
        RemoteDebugCommand::Serve(args) => {
            let layout = Layout::from_repo_root(repo_root());
            serve_live(&layout, &args.name, args.socket_dir.as_deref())
        }
        command => {
            let state = Mutex::new(RemoteDebugState::new());
            let out = run(command, &state).map_err(Error::RemoteDebug)?;
            println!("{}", out.0.message);
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
    let parsed = RemoteDebugMcpRoot::parse_or_serve_mcp_with_state(state.clone());
    match parsed.command {
        RemoteDebugGate::RemoteDebug {
            command: Some(RemoteDebugCommand::Serve(args)),
        } => {
            let layout = Layout::from_repo_root(repo);
            match serve_live(&layout, &args.name, args.socket_dir.as_deref()) {
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
                println!("{}", out.0.message);
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
) -> Result<AsStructured<RemoteDebugOutput>, String> {
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
) -> Result<AsStructured<RemoteDebugOutput>, String> {
    if matches!(cmd, RemoteDebugCommand::Serve(_)) {
        return Err("use a terminal; connect auto-starts the broker".into());
    }
    let guard = state.lock().map_err(|_| "session lock".to_string())?;
    let (dir_owned, name, request, start_broker) = request_from_command(cmd);
    let default_dir = broker_runtime_dir();
    let dir = dir_owned.as_deref().unwrap_or(&default_dir);
    if start_broker {
        let current = std::env::current_exe().map_err(|error| error.to_string())?;
        let exe = resolve_broker_exe(&repo_root(), &current);
        ensure_broker(dir, &name, &exe).map_err(map_host)?;
    }
    let reply = rpc(dir, &name, &request).map_err(map_host)?;
    if let Some(snap) = &reply.snapshot {
        let path = write_snapshot_planes(&guard.layout, snap.nonce, &snap.bw, snap.red.as_deref())
            .map_err(map_host)?;
        let message = decorate_message(
            format!("{} wrote {}", reply.message, path.display()),
            &reply,
        );
        if !reply.ok {
            return Err(message);
        }
        return Ok(AsStructured(tool_output(true, message, &reply)));
    }
    if !reply.ok {
        return Err(reply.message);
    }
    Ok(AsStructured(tool_output(
        true,
        decorate_message(reply.message.clone(), &reply),
        &reply,
    )))
}

/// Put scene / expect / `last_log` on the printed line (CLI has no JSON).
fn decorate_message(message: String, reply: &BrokerReply) -> String {
    let mut out = message;
    if let Some(snap) = &reply.snapshot {
        if let Some(scene) = snap.scene {
            out.push_str(&format!(" scene={scene}"));
        }
        if let Some(step) = snap.target_step {
            out.push_str(&format!(" step={step}"));
        }
        if let (Some(x), Some(y)) = (snap.target_expect_x, snap.target_expect_y) {
            out.push_str(&format!(" expect={x},{y}"));
        }
    }
    if let Some(log) = &reply.last_log {
        out.push_str(" last_log=");
        out.push_str(log);
    }
    out
}

fn tool_output(ok: bool, message: String, reply: &BrokerReply) -> RemoteDebugOutput {
    let snap = reply.snapshot.as_ref();
    RemoteDebugOutput {
        ok,
        message,
        nonce: reply.nonce,
        connected: reply.connected,
        phase: phase_name(reply.phase),
        last_log: reply.last_log.clone(),
        scene: snap.and_then(|s| s.scene),
        target_step: snap.and_then(|s| s.target_step),
        target_expect_x: snap.and_then(|s| s.target_expect_x),
        target_expect_y: snap.and_then(|s| s.target_expect_y),
    }
}

fn phase_name(phase: BrokerPhase) -> String {
    match phase {
        BrokerPhase::Disconnected => "disconnected",
        BrokerPhase::Pairing => "pairing",
        BrokerPhase::Connected => "connected",
    }
    .into()
}

fn request_from_command(cmd: RemoteDebugCommand) -> (Option<PathBuf>, String, BrokerRequest, bool) {
    match cmd {
        RemoteDebugCommand::Serve(_) => unreachable!("serve is not RPC"),
        RemoteDebugCommand::Connect(args) => (
            args.socket_dir.clone(),
            args.name.clone(),
            BrokerRequest::Connect(ConnectReq {
                pin: args.pin,
                port: args.port,
                name: args.name,
                remember: args.remember,
            }),
            true,
        ),
        RemoteDebugCommand::InjectTouch(args) => (
            args.socket_dir,
            args.name,
            BrokerRequest::InjectTouch {
                x: args.x,
                y: args.y,
                slot: args.slot,
                phase: args.phase,
                page: args.page,
            },
            false,
        ),
        RemoteDebugCommand::InjectButton(args) => (
            args.socket_dir,
            args.name,
            BrokerRequest::InjectButton {
                key: args.key,
                down: args.down,
            },
            false,
        ),
        RemoteDebugCommand::GetSnapshot(args) => (
            args.socket_dir,
            args.name,
            BrokerRequest::GetSnapshot { nonce: args.nonce },
            false,
        ),
        RemoteDebugCommand::SnapshotAck(args) => (
            args.socket_dir,
            args.name,
            BrokerRequest::SnapshotAck { nonce: args.nonce },
            false,
        ),
        RemoteDebugCommand::SnapshotClear(target) => (
            target.socket_dir,
            target.name,
            BrokerRequest::SnapshotClear,
            false,
        ),
        RemoteDebugCommand::Status(target) => {
            (target.socket_dir, target.name, BrokerRequest::Status, false)
        }
        RemoteDebugCommand::Reboot(args) => (
            args.socket_dir.clone(),
            args.name.clone(),
            BrokerRequest::Reboot(RebootReq {
                no_reconnect: args.no_reconnect,
                pin: args.pin,
                port: args.port,
                name: args.name,
                remember: args.remember,
            }),
            false,
        ),
        RemoteDebugCommand::Disconnect(target) => (
            target.socket_dir,
            target.name,
            BrokerRequest::Disconnect,
            false,
        ),
    }
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
    fn remote_debug_serve_parses_name() {
        let cli = RemoteDebugMcpRoot::try_parse_from([
            "xtask",
            "remote-debug",
            "serve",
            "--name",
            "sticky-rs",
        ])
        .expect("parse");
        match cli.command {
            RemoteDebugGate::RemoteDebug { command } => match command {
                Some(RemoteDebugCommand::Serve(args)) => {
                    assert_eq!(args.name, "sticky-rs");
                }
                other => panic!("{other:?}"),
            },
        }
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
