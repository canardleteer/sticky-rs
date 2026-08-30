//! Errors from factory-firmware host operations.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Recoverable failure. Printed on stderr by the binary.
#[derive(Debug)]
pub enum Error {
    /// Filesystem failure.
    Io(io::Error),
    /// JSON encode/decode failure.
    Json(serde_json::Error),
    /// Stock UART did not print `serial_number`.
    MissingFactorySerial,
    /// Stock UART printed more than one distinct `serial_number`.
    AmbiguousFactorySerial,
    /// Serial is empty or not a safe directory name.
    InvalidFactorySerial(String),
    /// `developer-data/backups/original/{serial}/` already exists (write-once).
    OriginalExists(PathBuf),
    /// `developer-data/backups/captures/{unit}/{slug}/` already exists.
    CaptureExists(PathBuf),
    /// Leftover repo-root `backups/` must be moved by the operator.
    LegacyBackupsDir(PathBuf),
    /// No original directory matches the live unit.
    MissingOriginal,
    /// `--capture` did not match a snapshot for this unit.
    MissingCapture(String),
    /// More than one capture MANIFEST has this MAC (and no original).
    AmbiguousCapture,
    /// Classification needs `--name` (or a TTY prompt in xtask).
    NeedsSnapshotName {
        /// Project / version / layout / serial evidence.
        evidence: String,
    },
    /// `--as-original` on a dump classified as not factory.
    NotFactoryAsOriginal,
    /// Live MAC or USB serial does not match the original MANIFEST.
    IdentityMismatch {
        /// Why the bind-check failed.
        reason: String,
    },
    /// More than one original MANIFEST has this MAC.
    AmbiguousOriginal,
    /// `board-info` did not report 32 MB flash.
    FlashSizeNot32Mb(String),
    /// Dump length is not a full 32 MiB image.
    DumpLength(usize),
    /// Partition table magic or bounds failed.
    PartitionTable(String),
    /// Live command needs `ESPFLASH_PORT`.
    MissingPort,
    /// `--probe` needs exactly one Sticky CH343, or an explicit `--port`.
    AmbiguousStickyUart,
    /// `--probe` found no QinHeng `1a86:55d3` UART.
    MissingStickyUart,
    /// `--port` is Espressif native USB-Serial/JTAG (`303a`), not this board's CH343.
    EspressifNativeUsb,
    /// `--port` is a USB-serial device that is not QinHeng `1a86:55d3`.
    NotStickyUart {
        /// USB vendor id from sysfs, when known.
        vid: Option<u16>,
        /// USB product id from sysfs, when known.
        pid: Option<u16>,
    },
    /// `--port` could not be classified as QinHeng (no by-id marker, no sysfs ids).
    UnclassifiedUsbPort,
    /// Another xtask already holds the UART session (dump, restore, probe, etc.).
    UartBusy {
        /// Holder pid, when the lock file could be read.
        pid: Option<u32>,
        /// Holder command name, when the lock file could be read.
        command: Option<String>,
    },
    /// Restore without `--yes`.
    RestoreNotConfirmed,
    /// `flash-app` without `--yes`.
    FlashNotConfirmed,
    /// Image is empty or an ELF, not an application flash payload.
    ImageNotApp,
    /// Image is larger than the `app0` partition.
    ImageTooLarge {
        /// File length in bytes.
        size: u64,
        /// `app0` partition size in bytes.
        max: u32,
    },
    /// `app0` offset would write below `0x90000`.
    UnsafeAppOffset(u32),
    /// Bound snapshot table is unknown or mismatched (needs `--allow-unknown-layout`).
    UnsafePartitionLayout {
        /// `unknown` or `mismatch:factory-32mb-v1`.
        status: String,
    },
    /// `--part` label is not in the original table.
    UnknownPartition(String),
    /// The `espflash` library or UART sample failed.
    Device(String),
    /// `learn-uart` YAML encode failed.
    Yaml(String),
    /// No `learn-uart/*.yaml` for this factory serial.
    MissingLearnReport,
    /// Human steps need a terminal stdin.
    LearnNeedsTty,
    /// `--import` source is unusable.
    Import(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
            Self::MissingFactorySerial => write!(
                f,
                "no factory serial_number in UART log (need stock firmware)"
            ),
            Self::AmbiguousFactorySerial => {
                write!(f, "UART log contains more than one serial_number value")
            }
            Self::InvalidFactorySerial(serial) => {
                write!(f, "refusing factory serial as a directory name: {serial:?}")
            }
            Self::OriginalExists(path) => write!(
                f,
                "original already exists (write-once): {}\nrun confirm-factory-firmware to measure drift",
                path.display()
            ),
            Self::CaptureExists(path) => write!(
                f,
                "capture already exists: {}",
                path.display()
            ),
            Self::LegacyBackupsDir(path) => write!(
                f,
                "leftover backups/ at {}; mkdir -p developer-data && mv backups developer-data/backups",
                path.display()
            ),
            Self::MissingOriginal => write!(
                f,
                "no snapshot matches this unit; run cargo xtask backup-factory-firmware first \
                 (originals in developer-data/backups/original/<serial>/, captures in \
                 developer-data/backups/captures/<unit-id>/<slug>/)"
            ),
            Self::MissingCapture(slug) => {
                write!(
                    f,
                    "no developer-data/backups/captures/<unit-id>/{slug}/ matches this unit"
                )
            }
            Self::AmbiguousCapture => write!(
                f,
                "multiple captures match this unit; pass --capture SLUG"
            ),
            Self::NeedsSnapshotName { evidence } => write!(
                f,
                "this dump is not a known factory image; pass --name SLUG (or --as-original if it is factory). {evidence}"
            ),
            Self::NotFactoryAsOriginal => write!(
                f,
                "refusing --as-original: this dump is not factory-shaped (in-tree image, git= stamp, or mismatched table)"
            ),
            Self::IdentityMismatch { reason } => write!(f, "identity mismatch: {reason}"),
            Self::AmbiguousOriginal => {
                write!(
                    f,
                    "multiple originals; pass a QinHeng by-id port from cargo xtask detect-connected"
                )
            }
            Self::FlashSizeNot32Mb(found) => {
                write!(f, "expected 32MB flash in board-info, found {found:?}")
            }
            Self::DumpLength(len) => write!(
                f,
                "expected 32 MiB dump ({}), got {len} bytes",
                crate::FLASH_SIZE
            ),
            Self::PartitionTable(reason) => write!(f, "partition table: {reason}"),
            Self::MissingPort => write!(f, "set ESPFLASH_PORT or pass --port"),
            Self::AmbiguousStickyUart => write!(
                f,
                "multiple Sticky CH343 ports; pass --port or set ESPFLASH_PORT"
            ),
            Self::MissingStickyUart => write!(
                f,
                "no QinHeng CH343 (1a86:55d3) found; plug in the Sticky or pass --port"
            ),
            Self::EspressifNativeUsb => write!(
                f,
                "refusing Espressif native USB (VID 303a); this product's USB-C is QinHeng CH343 1a86:55d3. Run cargo xtask detect-connected"
            ),
            Self::NotStickyUart { vid, pid } => match (vid, pid) {
                (Some(vid), Some(pid)) => write!(
                    f,
                    "refusing USB {vid:04x}:{pid:04x}; expected QinHeng CH343 1a86:55d3. Run cargo xtask detect-connected"
                ),
                _ => write!(
                    f,
                    "refusing this serial port; expected QinHeng CH343 1a86:55d3. Run cargo xtask detect-connected"
                ),
            },
            Self::UnclassifiedUsbPort => write!(
                f,
                "could not confirm this port is QinHeng CH343 1a86:55d3; pass a by-id node from cargo xtask detect-connected"
            ),
            Self::UartBusy { pid, command } => {
                write!(
                    f,
                    "refusing to reset the UART: another xtask already holds it"
                )?;
                match (pid, command) {
                    (Some(pid), Some(command)) => {
                        write!(f, " ({command}, pid {pid})")?;
                    }
                    (Some(pid), None) => write!(f, " (pid {pid})")?,
                    (None, Some(command)) => write!(f, " ({command})")?,
                    (None, None) => {}
                }
                write!(
                    f,
                    ". Wait for that dump, restore, probe, etc. to finish"
                )
            }
            Self::RestoreNotConfirmed => write!(f, "restore refuses to write without --yes"),
            Self::FlashNotConfirmed => write!(f, "flash-app refuses to write without --yes"),
            Self::ImageNotApp => write!(
                f,
                "image is empty or an ELF; pass a flash payload from cargo espflash save-image"
            ),
            Self::ImageTooLarge { size, max } => write!(
                f,
                "image is {size} bytes; app0 is only {max} bytes"
            ),
            Self::UnsafeAppOffset(offset) => write!(
                f,
                "refusing app0 offset {offset:#x}; writes below 0x90000 would land on nvs"
            ),
            Self::UnsafePartitionLayout { status } => write!(
                f,
                "refusing flash-app: snapshot partition table is {status}; pass --allow-unknown-layout only if you accept writing app0 against that table"
            ),
            Self::UnknownPartition(label) => write!(f, "no partition labelled {label:?}"),
            Self::Device(reason) => write!(f, "{reason}"),
            Self::Yaml(reason) => write!(f, "yaml: {reason}"),
            Self::MissingLearnReport => write!(
                f,
                "no learn-uart YAML under developer-data/uart-inspection-records/<serial>/; run cargo xtask learn-uart first"
            ),
            Self::LearnNeedsTty => write!(
                f,
                "learn-uart human steps need a terminal; pass --unattended-only or run interactively"
            ),
            Self::Import(reason) => write!(f, "import: {reason}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
