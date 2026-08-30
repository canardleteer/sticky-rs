//! `MANIFEST.json` for a factory original.

use serde::{Deserialize, Serialize};

use crate::identity::LiveIdentity;
use crate::partitions::{AppDesc, Partition};

/// Provenance and hashes for `backups/original/{factory-serial}/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Factory UART `serial_number` (directory name).
    pub factory_serial: String,
    /// CH343 serial when the port was a by-id node.
    pub usb_serial: Option<String>,
    /// Station MAC from `board-info`.
    pub mac: String,
    /// Raw flash-size field.
    pub flash_size: String,
    /// Secure boot reported enabled.
    pub secure_boot: bool,
    /// Flash encryption reported enabled.
    pub flash_encryption: bool,
    /// SHA-256 of `flash-32mb.bin`.
    pub dump_sha256: String,
    /// SHA-256 of the bootloader slice.
    pub bootloader_sha256: String,
    /// SHA-256 of the partition table slice.
    pub partition_table_sha256: String,
    /// Active OTA slot if `otadata` was present.
    pub boot_slot: Option<String>,
    /// App descriptor from `app0` if present.
    pub app0_desc: Option<AppDesc>,
    /// Partition table as parsed from the dump.
    pub partitions: Vec<Partition>,
    /// SHA-256 of each `part-{label}.bin`.
    pub partition_sha256: Vec<PartitionHash>,
}

/// Hash of one named slice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionHash {
    /// Partition label.
    pub label: String,
    /// Hex SHA-256.
    pub sha256: String,
}

impl Manifest {
    /// Bind fields used against a live unit.
    #[must_use]
    pub fn identity(&self) -> LiveIdentity {
        LiveIdentity {
            mac: self.mac.clone(),
            usb_serial: self.usb_serial.clone(),
        }
    }
}
