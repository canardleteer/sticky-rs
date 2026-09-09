//! Length-prefixed JSON request types. Not clap, not JSON-RPC 2.0.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Error;

/// Called after a successful pair when [`ConnectReq::remember`] is true.
pub type RememberHook = Arc<dyn Fn(&ConnectReq) -> Result<(), Error> + Send + Sync>;

/// One RPC from a CLI or MCP client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum BrokerRequest {
    /// Pair (caller opener; PIN / UART / other policy) and hold GATT.
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
    /// Six-digit PIN (skip a UART or other scrape). Optional.
    pub pin: Option<u32>,
    /// Serial device or other opener hint. Optional; foreign openers ignore it.
    pub port: Option<String>,
    /// Advertise name (socket key). Never a MAC.
    pub name: String,
    /// Keep the host BLE bond; opener / [`RememberHook`] decide what that means.
    pub remember: bool,
}

/// Arguments for [`BrokerRequest::Reboot`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebootReq {
    /// Do not Connect again after the MCU reset.
    pub no_reconnect: bool,
    /// Six-digit PIN for reconnect (skip scrape).
    pub pin: Option<u32>,
    /// Serial device for reconnect scrape. Optional.
    pub port: Option<String>,
    /// Advertise name.
    pub name: String,
    /// Keep the host BLE bond after the new pair.
    pub remember: bool,
}

/// Session meter for [`BrokerRequest::Status`] / [`BrokerRequest::Connect`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrokerPhase {
    /// No GATT and no in-flight pair.
    #[default]
    Disconnected,
    /// Opener worker is running (BlueZ / UART / other).
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
    /// Planes from a successful get (caller writes files).
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
    /// Firmware scene persist byte, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<u32>,
    /// Targets walk id, when present.
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

/// Options for [`crate::serve_with`].
#[derive(Clone)]
pub struct ServeOpts<'a> {
    /// Directory for the socket and pid file.
    pub dir: &'a Path,
    /// Advertise name (socket stem). Never a MAC.
    pub name: &'a str,
    /// Install a SIGINT handler that disconnects and exits.
    pub install_ctrlc: bool,
    /// Log `connected` / `snapshot` / errors on stderr (never a MAC).
    pub log: bool,
    /// After a successful pair with [`ConnectReq::remember`].
    pub on_remember: Option<RememberHook>,
}

/// How [`crate::ensure_broker`] starts a detached serve.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    /// Binary to spawn (the caller’s serve CLI).
    pub exe: std::path::PathBuf,
    /// Full argv after the executable. No implicit `remote-debug serve`.
    pub args: Vec<String>,
}
