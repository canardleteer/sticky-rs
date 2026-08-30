//! Snapshot `MANIFEST.yaml` (JSON is a read fallback for older trees).

use serde::{Deserialize, Serialize};

use crate::identity::LiveIdentity;
use crate::partitions::{AppDesc, Partition};

/// Schema id written on new snapshots.
pub const MANIFEST_SCHEMA: &str = "sticky-firmware-snapshot/v1";

fn default_schema() -> String {
    MANIFEST_SCHEMA.to_string()
}

/// Whether this tree is a factory original or a named capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    /// Factory-classified or human-confirmed factory (`backups/original/`).
    #[default]
    Original,
    /// Named “what is on the chip now” (`backups/captures/`).
    Capture,
}

/// Provenance and hashes for a snapshot directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Document identity.
    #[serde(default = "default_schema")]
    pub schema: String,
    /// Original vs named capture.
    #[serde(default)]
    pub kind: SnapshotKind,
    /// Catalog layout id when known (`factory-32mb-v1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_id: Option<String>,
    /// Classification tag (`known_factory:…`, `uncertain_stock:…`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<String>,
    /// Capture slug when [`SnapshotKind::Capture`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_name: Option<String>,
    /// Factory UART `serial_number`, or `mac-<hex>` when UART had none.
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
