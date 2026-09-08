//! Recoverable failures. Never include a MAC in the display text.

use std::fmt;

/// Host-side remote-debug failure.
#[derive(Debug)]
pub enum Error {
    /// No Bluetooth adapter, discovery miss, or GATT walk failed.
    Ble(String),
    /// Passkey source could not supply six digits.
    Passkey(String),
    /// Framed envelope was truncated, oversize, or the wrong version.
    Frame(remote_debug_wire::FrameError),
    /// Device replied `SnapshotBusy` (slot held by another nonce).
    SnapshotBusy {
        /// Nonce already armed on the device (0 if none).
        armed: u64,
    },
    /// Session is not connected.
    NotConnected,
    /// Inject or snapshot map failed.
    Map(remote_debug_wire::MapError),
    /// I/O or runtime failure.
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ble(reason) => write!(f, "ble: {reason}"),
            Self::Passkey(reason) => write!(f, "passkey: {reason}"),
            Self::Frame(err) => write!(f, "frame: {err:?}"),
            Self::SnapshotBusy { armed } => write!(f, "snapshot busy (armed={armed:#x})"),
            Self::NotConnected => write!(f, "not connected"),
            Self::Map(err) => write!(f, "map: {err:?}"),
            Self::Io(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<remote_debug_wire::FrameError> for Error {
    fn from(error: remote_debug_wire::FrameError) -> Self {
        Self::Frame(error)
    }
}
