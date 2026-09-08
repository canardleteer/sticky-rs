//! `cargo xtask remote-debug` — live BLE session (MCP is this subtree only).

use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use clap_mcp::{AsStructured, ClapMcp, ParseOrServeMcpWithState};
use panel_view::{TouchSample, TouchSource};
use remote_debug_host::{
    connect, ChannelPasskey, FixedPasskey, PasskeySource, Session, DEFAULT_ADV_NAME,
};
use remote_debug_wire::v1::ProductKey;
use serde::Serialize;
use sticky_host::{
    detect, refuse_if_legacy_backups_at_repo_root, remember_unit, try_acquire,
    usb_serial_from_port, wait_new_pair_pin, write_snapshot_planes, Error, Layout,
};

use crate::cli::repo_root;

/// Live BLE remote-debug. Pair then hold. Encrypted GATT. Never a MAC.
pub const ABOUT: &str = "\
Live BLE remote-debug (encrypted GATT after DisplayOnly pair). Advertise \
name `sticky-rs`. BlueZ Connect, not Pair(). Default `connect` scrapes a \
new UART `pair pin=` when a Sticky CH343 is present (takes the UART lock; \
fails if `monitor` holds it). `--pin` skips UART. `--remember` allowlists \
this unit (factory / USB serial in gitignored developer-data/remote-debug/; \
never a MAC, never the PIN).

`--mcp` is a long-lived session on this subtree only (not flash-app / \
restore). Do not also run `monitor`. Coordinates are framebuffer.";

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
        /// Session command.
        #[command(subcommand)]
        command: RemoteDebugCommand,
    },
}

/// Nested remote-debug parser for `cargo xtask --help` (not the MCP root).
#[derive(Debug, Args)]
pub struct RemoteDebugCli {
    /// Session command.
    #[command(subcommand)]
    pub command: RemoteDebugCommand,
}

/// Leaf tools. Stateful in-process (`reinvocation_safe`); one BLE link.
#[derive(Debug, Clone, Subcommand, ClapMcp)]
#[clap_mcp(reinvocation_safe, parallel_safe = false)]
#[clap_mcp_output_from_with_state = "run"]
#[clap_mcp_state_type = "Mutex<RemoteDebugState>"]
#[clap_mcp_output_type = "RemoteDebugOutput"]
pub enum RemoteDebugCommand {
    /// Pair (UART auto-PIN or `--pin`) and hold the GATT link
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
    SnapshotClear,
    /// Whether a session is held
    Status,
    /// Drop GATT; unknown units lose the BlueZ bond
    Disconnect,
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
}

/// Host-chosen nonce (non-zero).
#[derive(Debug, Clone, Args)]
pub struct GetSnapshotArgs {
    /// `fixed64` nonce. Omit to use a time-based value.
    #[arg(long)]
    pub nonce: Option<u64>,
}

/// Ack the armed nonce.
#[derive(Debug, Clone, Args)]
pub struct SnapshotAckArgs {
    /// Nonce from the last get (default: last get).
    #[arg(long)]
    pub nonce: Option<u64>,
}

/// Process-lifetime BLE session (MCP). CLI one-shots connect as needed.
pub struct RemoteDebugState {
    layout: Layout,
    session: Option<Session<remote_debug_host::BluerTransport>>,
    last_nonce: Option<u64>,
}

impl RemoteDebugState {
    fn new() -> Self {
        let repo = repo_root();
        Self {
            layout: Layout::from_repo_root(repo),
            session: None,
            last_nonce: None,
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
    /// Whether a GATT session is held in this process.
    pub connected: bool,
}

/// CLI dispatch when `Cli` parsed `remote-debug` (no `--mcp`).
pub fn run_cli(cli: RemoteDebugCli) -> Result<(), Error> {
    let state = Mutex::new(RemoteDebugState::new());
    let out = run(cli.command, &state).map_err(Error::RemoteDebug)?;
    println!("{}", out.0.message);
    Ok(())
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
    match run(parsed.command.into_leaf(), state.as_ref()) {
        Ok(out) => {
            println!("{}", out.0.message);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

impl RemoteDebugGate {
    fn into_leaf(self) -> RemoteDebugCommand {
        match self {
            Self::RemoteDebug { command } => command,
        }
    }
}

/// MCP gate `run` (unwraps to the leaf).
fn run_gate(
    cmd: RemoteDebugGate,
    state: &Mutex<RemoteDebugState>,
) -> Result<AsStructured<RemoteDebugOutput>, String> {
    run(cmd.into_leaf(), state)
}

/// Shared CLI + MCP dispatch.
pub fn run(
    cmd: RemoteDebugCommand,
    state: &Mutex<RemoteDebugState>,
) -> Result<AsStructured<RemoteDebugOutput>, String> {
    let mut guard = state.lock().map_err(|_| "session lock".to_string())?;
    let out = match cmd {
        RemoteDebugCommand::Connect(args) => connect_cmd(&mut guard, args)?,
        RemoteDebugCommand::InjectTouch(args) => inject_touch_cmd(&mut guard, args)?,
        RemoteDebugCommand::InjectButton(args) => inject_button_cmd(&mut guard, args)?,
        RemoteDebugCommand::GetSnapshot(args) => get_snapshot_cmd(&mut guard, args)?,
        RemoteDebugCommand::SnapshotAck(args) => ack_cmd(&mut guard, args)?,
        RemoteDebugCommand::SnapshotClear => {
            session_mut(&mut guard)?.snapshot_clear().map_err(map_ble)?;
            out("snapshot clear", guard.last_nonce, true)
        }
        RemoteDebugCommand::Status => out(
            if guard.session.is_some() {
                "connected"
            } else {
                "disconnected"
            },
            guard.last_nonce,
            guard.session.is_some(),
        ),
        RemoteDebugCommand::Disconnect => {
            if let Some(mut session) = guard.session.take() {
                session.disconnect().map_err(map_ble)?;
            }
            out("disconnected", None, false)
        }
    };
    Ok(AsStructured(out))
}

fn connect_cmd(
    state: &mut RemoteDebugState,
    args: ConnectArgs,
) -> Result<RemoteDebugOutput, String> {
    if state.session.is_some() {
        return Ok(out("already connected", state.last_nonce, true));
    }
    let mut uart_hold = None;
    let passkey: Arc<dyn PasskeySource> = if let Some(pin) = args.pin {
        Arc::new(FixedPasskey::new(pin))
    } else {
        let port = detect::resolve_sticky_port(args.port.clone()).map_err(map_host)?;
        uart_hold = Some(try_acquire(&port, "remote-debug").map_err(map_host)?);
        let mut cdc = sticky_host::cdc_listen::CdcListen::open(&port).map_err(map_host)?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            if let Ok(pin) = wait_new_pair_pin(&mut cdc, &mut seen, Duration::from_secs(45)) {
                let _ = tx.send(pin);
            }
        });
        Arc::new(ChannelPasskey::new(rx, Duration::from_secs(45)))
    };
    let _uart_hold = uart_hold;

    let transport = connect(&args.name, passkey).map_err(map_ble)?;
    let session = Session::new(transport, args.remember);
    if args.remember {
        let usb = args
            .port
            .as_deref()
            .and_then(usb_serial_from_port)
            .or_else(|| {
                detect::resolve_sticky_port(args.port.clone())
                    .ok()
                    .and_then(|p| usb_serial_from_port(&p))
            });
        remember_unit(&state.layout, None, usb.as_deref()).map_err(map_host)?;
    }
    // Touch the session so the link is owned here.
    let _ = session.last_nonce();
    state.session = Some(session);
    Ok(out(
        if args.remember {
            "connected (remembered this unit; no MAC printed)"
        } else {
            "connected (ephemeral BlueZ bond)"
        },
        None,
        true,
    ))
}

fn inject_touch_cmd(
    state: &mut RemoteDebugState,
    args: InjectTouchArgs,
) -> Result<RemoteDebugOutput, String> {
    session_mut(state)?
        .inject_touch(TouchSample {
            source: TouchSource::Synthetic,
            x: args.x,
            y: args.y,
            slot: args.slot,
        })
        .map_err(map_ble)?;
    Ok(out("inject-touch queued", state.last_nonce, true))
}

fn inject_button_cmd(
    state: &mut RemoteDebugState,
    args: InjectButtonArgs,
) -> Result<RemoteDebugOutput, String> {
    let key = parse_key(&args.key)?;
    session_mut(state)?
        .inject_button(key, args.down)
        .map_err(map_ble)?;
    Ok(out("inject-button queued", state.last_nonce, true))
}

fn get_snapshot_cmd(
    state: &mut RemoteDebugState,
    args: GetSnapshotArgs,
) -> Result<RemoteDebugOutput, String> {
    let nonce = args.nonce.unwrap_or_else(|| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        now.max(1)
    });
    let snap = session_mut(state)?.get_snapshot(nonce).map_err(map_ble)?;
    let path = write_snapshot_planes(&state.layout, snap.nonce, &snap.bw, snap.red.as_deref())
        .map_err(map_host)?;
    state.last_nonce = Some(snap.nonce);
    Ok(out(
        &format!(
            "snapshot {}x{} kind={:?} wrote {}",
            snap.width,
            snap.height,
            snap.kind,
            path.display()
        ),
        Some(snap.nonce),
        true,
    ))
}

fn ack_cmd(
    state: &mut RemoteDebugState,
    args: SnapshotAckArgs,
) -> Result<RemoteDebugOutput, String> {
    let nonce = args
        .nonce
        .or(state.last_nonce)
        .ok_or_else(|| "no nonce; pass --nonce or get-snapshot first".to_string())?;
    session_mut(state)?.snapshot_ack(nonce).map_err(map_ble)?;
    Ok(out("snapshot ack sent", Some(nonce), true))
}

fn session_mut(
    state: &mut RemoteDebugState,
) -> Result<&mut Session<remote_debug_host::BluerTransport>, String> {
    state
        .session
        .as_mut()
        .ok_or_else(|| "not connected; run remote-debug connect first".into())
}

fn parse_key(raw: &str) -> Result<ProductKey, String> {
    match raw.to_ascii_lowercase().as_str() {
        "ok" | "4" => Ok(ProductKey::PRODUCT_KEY_OK),
        "page-up" | "pageup" | "5" => Ok(ProductKey::PRODUCT_KEY_PAGE_UP),
        "page-down" | "pagedown" | "6" => Ok(ProductKey::PRODUCT_KEY_PAGE_DOWN),
        _ => Err("key must be ok, page-up, or page-down".into()),
    }
}

fn map_host(error: Error) -> String {
    error.to_string()
}

fn map_ble(error: remote_debug_host::Error) -> String {
    error.to_string()
}

fn out(message: &str, nonce: Option<u64>, connected: bool) -> RemoteDebugOutput {
    RemoteDebugOutput {
        ok: true,
        message: message.to_string(),
        nonce,
        connected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use clap_mcp::{schema_from_command_with_metadata, ClapMcpSchemaMetadataProvider};

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
    }

    #[test]
    fn remote_debug_connect_parses_remember() {
        let cli =
            RemoteDebugMcpRoot::try_parse_from(["xtask", "remote-debug", "connect", "--remember"])
                .expect("parse");
        match cli.command {
            RemoteDebugGate::RemoteDebug { command } => match command {
                RemoteDebugCommand::Connect(args) => {
                    assert!(args.remember);
                    assert!(args.pin.is_none());
                }
                other => panic!("{other:?}"),
            },
        }
    }
}
