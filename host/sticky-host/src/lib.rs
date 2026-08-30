//! Programmatic host API for per-unit factory flash originals.
//!
//! `cargo xtask` and a later standalone CLI both call these methods. Do not
//! open a UART unless a human explicitly asked.

#![allow(missing_docs)]

pub mod backup;
pub mod build_fw;
pub mod cdc_listen;
pub mod classify;
pub mod confirm;
pub mod detect;
pub mod device;
pub mod dump;
pub mod error;
#[path = "flash_app.rs"]
pub mod flash_app_impl;
pub mod git;
pub mod identity;
#[path = "learn_uart/mod.rs"]
pub mod learn_uart_impl;
pub mod manifest;
#[path = "monitor.rs"]
pub mod monitor_impl;
pub mod original;
pub mod partition_layouts;
pub mod partitions;
#[path = "restore.rs"]
pub mod restore_impl;
pub mod uart_lock;

use std::path::{Path, PathBuf};

pub use backup::BackupRequest;
pub use build_fw::{build_fw, BuildFwArgs, BuildFwOutput, FirmwareImage};
pub use confirm::DivergenceReport;
pub use device::{DeviceIo, RealDevice};
pub use error::Error;
pub use learn_uart_impl::LearnUartArgs;
pub use manifest::SnapshotKind;
pub use monitor_impl::MonitorOptions;
pub use original::{load_manifest, refuse_if_legacy_backups_at_repo_root, Layout};
pub use uart_lock::{try_acquire, UartSession, UART_LOCK_ENV};

/// Full-chip image size (32 MiB).
pub const FLASH_SIZE: usize = 32 * 1024 * 1024;
/// Chunk size for `espflash` library reads (one 32 MiB request buffers the chip).
pub const CHUNK_SIZE: usize = 1024 * 1024;
/// Number of chunks in a full dump.
pub const CHUNKS: usize = FLASH_SIZE / CHUNK_SIZE;

/// USB inventory, optionally a DTR `--probe`.
///
/// Inventory without probe does not take the UART lock. Probe does.
pub fn detect_connected(probe: bool, port: Option<String>, all_devices: bool) -> Result<(), Error> {
    detect::run(&RealDevice, probe, port, all_devices)
}

/// Live capture: UART sample, board-info, chunked 32 MiB read, write-once dir.
///
/// `ask_name` is invoked when classification needs a slug and `request.name`
/// is empty (xtask prompts on a TTY).
pub fn backup_live<F>(
    layout: &Layout,
    port: Option<String>,
    request: &BackupRequest,
    ask_name: F,
) -> Result<PathBuf, Error>
where
    F: FnOnce(&str) -> Result<Option<String>, Error>,
{
    let port = detect::resolve_sticky_port(port)?;
    let _uart = uart_lock::try_acquire(&port, "backup-factory-firmware")?;
    backup::backup_live(&RealDevice, layout, &port, request, ask_name)
}

/// Host-only copy of an already-taken 32 MiB dump tree. Still write-once.
pub fn backup_import<F>(
    layout: &Layout,
    source: &Path,
    request: &BackupRequest,
    ask_name: F,
) -> Result<PathBuf, Error>
where
    F: FnOnce(&str) -> Result<Option<String>, Error>,
{
    backup::backup_import(layout, source, request, ask_name)
}

/// Read live flash, compare to the matching original (or `--capture`).
pub fn confirm_live(
    layout: &Layout,
    port: Option<String>,
    capture: Option<&str>,
) -> Result<DivergenceReport, Error> {
    let port = detect::resolve_sticky_port(port)?;
    let _uart = uart_lock::try_acquire(&port, "confirm-factory-firmware")?;
    confirm::confirm_live(&RealDevice, layout, &port, capture)
}

/// Restore that unit's original (or `--capture`) via `write-bin` only.
pub fn restore(
    layout: &Layout,
    port: Option<String>,
    yes: bool,
    part: Option<&str>,
    capture: Option<&str>,
) -> Result<(), Error> {
    let port = detect::resolve_sticky_port(port)?;
    let _uart = uart_lock::try_acquire(&port, "restore-factory-firmware")?;
    restore_impl::restore(&RealDevice, layout, &port, yes, part, capture)
}

/// `write_bin` of `image` at this unit's `app0` offset.
pub fn flash_app(
    layout: &Layout,
    port: Option<String>,
    image: &Path,
    yes: bool,
    allow_unknown_layout: bool,
    capture: Option<&str>,
) -> Result<(), Error> {
    let port = detect::resolve_sticky_port(port)?;
    let _uart = uart_lock::try_acquire(&port, "flash-app")?;
    flash_app_impl::flash_app(
        &RealDevice,
        layout,
        &port,
        image,
        yes,
        allow_unknown_layout,
        capture,
    )
}

/// UART learn session. Takes the session lock for the whole call, including
/// optional `flash-app` / restore-app0 children.
pub fn learn_uart(layout: &Layout, args: LearnUartArgs) -> Result<(), Error> {
    let port = detect::resolve_sticky_port(args.port.clone())?;
    let _uart = uart_lock::try_acquire(&port, "learn-uart")?;
    learn_uart_impl::run(layout, args)
}

/// Host-only comparison of two learn-uart YAML reports.
pub fn diff_learn_uart(
    layout: &Layout,
    left: &str,
    right: &str,
    show_serials: bool,
) -> Result<(), Error> {
    learn_uart_impl::diff::run(layout, left, right, show_serials)
}

/// Copy the CH343 UART to stdout (and optionally a file) until interrupted
/// or a listen budget is exhausted.
pub fn monitor(port: Option<String>, options: &MonitorOptions) -> Result<(), Error> {
    let port = detect::resolve_sticky_port(port)?;
    let _uart = uart_lock::try_acquire(&port, "monitor")?;
    monitor_impl::monitor(&port, options)
}
