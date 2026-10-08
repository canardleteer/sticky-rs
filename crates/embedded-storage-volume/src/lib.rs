//! Partitioned filesystems for managed 512-byte media.
//!
//! The caller owns scheduling and bus arbitration. A successful block-device
//! flush means the device has completed its advertised write protocol; it does
//! not promise that a consumer SD controller survives unexpected power loss.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

pub mod fat;
pub mod jobs;
pub mod littlefs;
pub mod package;
pub mod partition;
pub mod record;
#[cfg(feature = "sd")]
pub mod sd;

pub use hadris_storage::sync::{BlockDevice, BlockDeviceMut};
pub use hadris_storage::{BlockCount, BlockGeometry, BlockIndex, BlockSize};
pub use partition::{FilesystemKind, Layout, Volume, ALIGN_SECTORS, SECTOR_BYTES};

/// Classified adapter failures; filesystem callers can inspect the backend
/// error separately when they need its native diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A lower-level read, write, or flush failed.
    Media,
    /// A request crosses a partition boundary or overflows an address.
    Bounds,
    /// Geometry is incompatible with the compiled filesystem profile.
    Geometry,
    /// Partition metadata is missing, corrupt, or overlaps another volume.
    Partition,
    /// The filesystem could not mount or complete an operation.
    Filesystem,
    /// Update metadata, length, digest, or target is invalid.
    Package,
    /// Destructive provisioning was not explicitly confirmed.
    Unconfirmed,
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl core::error::Error for Error {}
impl embedded_io::Error for Error {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            Self::Media | Self::Filesystem => embedded_io::ErrorKind::Other,
            _ => embedded_io::ErrorKind::InvalidInput,
        }
    }
}

/// Controls when an application's accumulated log buffer must be synchronized.
/// Filesystem adapters still complete every physical write before returning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritePolicy {
    /// Commit each application operation; appropriate for state and updates.
    Immediate,
    /// Accumulate application data up to a size or elapsed-time limit.
    Batched {
        /// Maximum pending application bytes.
        bytes: usize,
        /// Maximum time since the last synchronization, in milliseconds.
        milliseconds: u64,
    },
    /// The application explicitly requests each synchronization.
    Explicit,
}
impl WritePolicy {
    /// Default logging policy: 4 KiB or one second.
    pub const LOG: Self = Self::Batched {
        bytes: 4096,
        milliseconds: 1000,
    };
    /// Whether the caller must commit its buffer now. A backwards clock forces
    /// synchronization so resets cannot defer buffered data indefinitely.
    pub const fn due(self, pending: usize, now_ms: u64, last_sync_ms: u64) -> bool {
        if pending == 0 {
            return false;
        }
        match self {
            Self::Immediate => true,
            Self::Explicit => false,
            Self::Batched {
                bytes,
                milliseconds,
            } => pending >= bytes || now_ms < last_sync_ms || now_ms - last_sync_ms >= milliseconds,
        }
    }
}
