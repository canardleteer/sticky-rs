//! Device I/O via the `espflash` library (the crate `cargo-espflash` wraps).
//!
//! `cargo-espflash` is a Cargo plugin binary, not a library. Host tests inject
//! [`MockDevice`] and never open a port.
//!
//! Live public methods hold [`crate::uart_lock::UartSession`] around these
//! calls so a second host command cannot DTR/RTS-reset mid-dump or mid-write.
//! Subprocesses that might pulse DTR use
//! [`UartSession::status`](crate::uart_lock::UartSession::status) on that
//! same session.

use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use espflash::connection::{Connection, ResetAfterOperation, ResetBeforeOperation};
use espflash::flasher::{FlashSize, Flasher};
use espflash::target::{Chip, ProgressCallbacks};
use serialport::{FlowControl, UsbPortInfo};

use crate::{Error, CHUNK_SIZE};

/// Baud after the flasher stub is loaded (same as `cargo espflash --baud`).
/// Confirmed: 32 × 1 MiB `read-flash` at this rate completes (~14 s/MiB).
pub const ESPFLASH_BAUD: u32 = 921_600;
/// ROM connect baud. [`Flasher::connect`] then raises to [`ESPFLASH_BAUD`].
pub const CONNECT_BAUD: u32 = 115_200;
/// How long to listen for stock `serial_number` after a run-mode EN/RTS pulse.
///
/// Confirmed on the CH343 UART: opening ACM does not reprint the log; after
/// a run-mode EN/RTS pulse the `key=serial_number` line appears in about
/// 4.5–6.5 s (IDF `I (5672)`). Twenty seconds covers a slower boot.
pub const UART_SAMPLE_SECS: u64 = 20;
/// `read_flash` packet size used by `cargo-espflash` (`FLASH_SECTOR_SIZE`).
const READ_BLOCK: u32 = 0x1000;
/// Un-acked packets allowed by `cargo-espflash read-flash`.
const MAX_IN_FLIGHT: u32 = 64;

/// Operations that would touch a Sticky. Mocked in unit tests.
pub trait DeviceIo {
    /// Stock firmware UART (115200) used to read `serial_number`.
    fn sample_uart(&self, port: &str) -> Result<String, Error>;
    /// Text matching `cargo espflash board-info` (parsed by [`crate::identity`]).
    fn board_info(&self, port: &str) -> Result<String, Error>;
    /// One flash window. [`RealDevice`] keeps one flasher session for the call.
    fn read_flash(&self, port: &str, offset: u32, size: u32) -> Result<Vec<u8>, Error>;
    /// `write_bin_to_flash` of `file` at `offset`. Never a full-chip erase.
    fn write_bin(&self, port: &str, offset: u32, file: &Path) -> Result<(), Error>;
}

/// In-process `espflash` flasher plus UART sample via `serialport`.
pub struct RealDevice;

fn map_serialport(error: serialport::Error) -> Error {
    Error::Device(error.to_string())
}

fn map_espflash(error: espflash::Error) -> Error {
    Error::Device(error.to_string())
}

/// Pulse EN (RTS) with IO0 high so the app boots, not the ROM download stub.
///
/// Confirmed on this product's CH343: same polarity as `espflash`
/// `reset_after_flash` for a UART bridge. Opening the ACM node does not
/// reprint stock `serial_number`.
fn reset_into_app(serial: &mut dyn serialport::SerialPort) -> Result<(), Error> {
    serial
        .write_data_terminal_ready(false)
        .map_err(map_serialport)?;
    serial.write_request_to_send(true).map_err(map_serialport)?;
    std::thread::sleep(Duration::from_millis(100));
    serial
        .write_request_to_send(false)
        .map_err(map_serialport)?;
    Ok(())
}

fn connect(port: &str, after_baud: Option<u32>) -> Result<Flasher, Error> {
    crate::detect::require_sticky_ch343(port)?;
    let serial = serialport::new(port, CONNECT_BAUD)
        .flow_control(FlowControl::None)
        .open_native()
        .map_err(|error| Error::Device(format!("serial open failed: {error}")))?;
    // Confirmed on QinHeng CH343: not USB-JTAG, so dummy USB ids pick the UART
    // reset strategy (same as `espflash::cli::connect` for unknown port types).
    let usb = UsbPortInfo {
        vid: 0,
        pid: 0,
        serial_number: None,
        manufacturer: None,
        product: None,
    };
    let connection = Connection::new(
        serial,
        usb,
        ResetAfterOperation::HardReset,
        ResetBeforeOperation::DefaultReset,
        CONNECT_BAUD,
    );
    let mut flasher =
        Flasher::connect(connection, true, true, true, None, after_baud).map_err(map_espflash)?;
    if flasher.chip() != Chip::Esp32s3 {
        return Err(Error::Device(format!(
            "expected ESP32-S3, found {}",
            flasher.chip()
        )));
    }
    flasher.set_flash_size(FlashSize::_32Mb);
    Ok(flasher)
}

fn reset_after(flasher: &mut Flasher) -> Result<(), Error> {
    let chip = flasher.chip();
    flasher
        .connection()
        .reset_after(true, chip)
        .map_err(map_espflash)
}

fn format_board_info(flasher: &mut Flasher) -> Result<String, Error> {
    let info = flasher.device_info().map_err(map_espflash)?;
    let mut text = String::new();
    let _ = writeln!(text, "Chip type:         {}", info.chip);
    if let Some((major, minor)) = info.revision {
        let _ = writeln!(text, "Chip revision:     v{major}.{minor}");
    }
    let _ = writeln!(text, "Crystal frequency: {}", info.crystal_frequency);
    let _ = writeln!(text, "Flash size:        {}", info.flash_size);
    if !info.features.is_empty() {
        let _ = writeln!(text, "Features:          {}", info.features.join(", "));
    }
    match &info.mac_address {
        Some(mac) => {
            let _ = writeln!(text, "MAC address:       {mac}");
        }
        None => {
            return Err(Error::Device(
                "board-info missing MAC address (secure download mode?)".into(),
            ));
        }
    }
    if info.chip != Chip::Esp32 {
        match flasher.security_info() {
            Ok(security) => {
                let _ = write!(text, "{security}");
            }
            Err(_) => {
                let _ = writeln!(text, "Secure Boot: Disabled");
                let _ = writeln!(text, "Flash Encryption: Disabled");
            }
        }
    }
    Ok(text)
}

/// How many times to reconnect and retry the current 1 MiB write window.
const WRITE_WINDOW_RETRIES: u32 = 3;

/// Windows needed to write `len` bytes in [`CHUNK_SIZE`] slices.
#[must_use]
pub(crate) fn write_bin_window_count(len: usize) -> usize {
    if len == 0 {
        0
    } else {
        len.div_ceil(CHUNK_SIZE)
    }
}

/// Percent complete from espflash chunk counts (`init`/`update` are chunks,
/// not bytes — stub writes 16 KiB at a time).
#[must_use]
pub(crate) fn write_bin_percent(current_chunks: usize, total_chunks: usize) -> u8 {
    if total_chunks == 0 {
        100
    } else {
        current_chunks
            .saturating_mul(100)
            .checked_div(total_chunks)
            .unwrap_or(100)
            .min(100) as u8
    }
}

/// Heartbeats for `write_bin_to_flash` so a 32 MiB restore is not silent.
///
/// [`ProgressCallbacks::init`] `total` and [`ProgressCallbacks::update`]
/// `current` are **chunk counts**, not bytes. A full-chip restore is thousands
/// of 16 KiB stub chunks; this prints a stderr percent line as they land.
struct WriteBinProgress {
    window: usize,
    windows: usize,
    image_bytes: usize,
    total_chunks: usize,
    last_percent: u8,
    started: std::time::Instant,
}

impl WriteBinProgress {
    fn new(window: usize, windows: usize, image_bytes: usize) -> Self {
        Self {
            window,
            windows,
            image_bytes,
            total_chunks: 0,
            last_percent: 0,
            started: std::time::Instant::now(),
        }
    }

    fn paint(&self, current: usize, extra: &str) {
        let pct = write_bin_percent(current, self.total_chunks);
        let written = self
            .image_bytes
            .saturating_mul(current)
            .checked_div(self.total_chunks)
            .unwrap_or(self.image_bytes);
        eprint!(
            "\rwrite-bin window {}/{} {current}/{} ({pct}%) {}/{} bytes elapsed={:?}{extra}   ",
            self.window,
            self.windows,
            self.total_chunks,
            written,
            self.image_bytes,
            self.started.elapsed(),
        );
        let _ = std::io::Write::flush(&mut std::io::stderr());
    }
}

impl ProgressCallbacks for WriteBinProgress {
    fn init(&mut self, addr: u32, total: usize) {
        self.total_chunks = total;
        self.last_percent = 0;
        self.started = std::time::Instant::now();
        log::info!(
            "write-bin window {}/{} offset={addr:#010x} chunks={total} bytes={}",
            self.window,
            self.windows,
            self.image_bytes
        );
        eprintln!(
            "write-bin window {}/{} offset={addr:#010x} chunks={total} bytes={}",
            self.window, self.windows, self.image_bytes
        );
        self.paint(0, "");
    }

    fn update(&mut self, current: usize) {
        let pct = write_bin_percent(current, self.total_chunks);
        if pct != self.last_percent || current == self.total_chunks {
            self.last_percent = pct;
            self.paint(current, "");
        }
    }

    fn verifying(&mut self) {
        log::info!("write-bin verifying");
        self.paint(self.total_chunks, " verifying");
    }

    fn finish(&mut self, skipped: bool) {
        if skipped {
            self.paint(self.total_chunks, " skipped (checksum match)");
        } else {
            self.paint(self.total_chunks, " done");
        }
        eprintln!();
        log::info!("write-bin done skipped={skipped}");
    }
}

impl DeviceIo for RealDevice {
    fn sample_uart(&self, port: &str) -> Result<String, Error> {
        crate::detect::require_sticky_ch343(port)?;
        log::info!(
            "sampling UART for stock serial_number (up to {UART_SAMPLE_SECS}s; EN/RTS pulse, run mode)"
        );
        let mut serial = serialport::new(port, 115_200)
            .timeout(Duration::from_millis(250))
            .open()
            .map_err(|error| Error::Device(format!("UART open failed: {error}")))?;
        reset_into_app(&mut *serial)?;
        let mut buf = vec![0u8; 8192];
        let mut collected = Vec::new();
        let started = std::time::Instant::now();
        let deadline = started + Duration::from_secs(UART_SAMPLE_SECS);
        while std::time::Instant::now() < deadline {
            match serial.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => collected.extend_from_slice(&buf[..n]),
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(error) => return Err(Error::Device(format!("UART read failed: {error}"))),
            }
            let text = String::from_utf8_lossy(&collected);
            if crate::identity::uart_has_unique_factory_serial(&text) {
                log::info!("caught serial_number after {:?}", started.elapsed());
                return Ok(text.into_owned());
            }
        }
        Ok(String::from_utf8_lossy(&collected).into_owned())
    }

    fn board_info(&self, port: &str) -> Result<String, Error> {
        log::info!("connecting flasher for board-info");
        let mut flasher = connect(port, Some(ESPFLASH_BAUD))?;
        let text = format_board_info(&mut flasher)?;
        reset_after(&mut flasher)?;
        Ok(text)
    }

    fn read_flash(&self, port: &str, offset: u32, size: u32) -> Result<Vec<u8>, Error> {
        let total_chunks = size.div_ceil(CHUNK_SIZE as u32).max(1);
        log::info!(
            "connecting flasher for read-flash {size} bytes in {total_chunks}×{} KiB windows @ {ESPFLASH_BAUD}",
            CHUNK_SIZE / 1024
        );
        let mut flasher = connect(port, Some(ESPFLASH_BAUD))?;
        let mut dump = Vec::with_capacity(size as usize);
        let mut remaining = size;
        let mut addr = offset;
        let mut index = 0u32;
        let started = std::time::Instant::now();
        while remaining > 0 {
            index += 1;
            let chunk = remaining.min(CHUNK_SIZE as u32);
            log::info!(
                "read-flash {index}/{total_chunks} offset={addr:#010x} size={chunk} elapsed={:?}",
                started.elapsed()
            );
            let tmp = tempfile::Builder::new()
                .prefix("sticky-xtask-")
                .suffix(".bin")
                .tempfile()
                .map_err(Error::from)?;
            let path = tmp.path().to_path_buf();
            flasher
                .read_flash(addr, chunk, READ_BLOCK, MAX_IN_FLIGHT, path.clone())
                .map_err(map_espflash)?;
            let bytes = std::fs::read(&path)?;
            if bytes.len() != chunk as usize {
                return Err(Error::Device(format!(
                    "read at {addr:#x} returned {} bytes, expected {chunk}",
                    bytes.len()
                )));
            }
            dump.extend(bytes);
            addr = addr.saturating_add(chunk);
            remaining -= chunk;
        }
        log::info!("read-flash finished in {:?}", started.elapsed());
        reset_after(&mut flasher)?;
        Ok(dump)
    }

    fn write_bin(&self, port: &str, offset: u32, file: &Path) -> Result<(), Error> {
        let data = std::fs::read(file)?;
        let windows = write_bin_window_count(data.len());
        log::info!(
            "connecting flasher for write-bin {} bytes in {windows}×{} KiB windows @ {ESPFLASH_BAUD}",
            data.len(),
            CHUNK_SIZE / 1024
        );
        eprintln!(
            "write-bin: {} bytes at {offset:#x} in {windows}×{} KiB windows (device MD5 per window can skip a match; reconnect on drop)",
            data.len(),
            CHUNK_SIZE / 1024
        );
        if data.is_empty() {
            return Err(Error::Device("write-bin image is empty".into()));
        }
        let mut flasher = connect(port, Some(ESPFLASH_BAUD))?;
        let started = std::time::Instant::now();
        let mut index = 0usize;
        let mut retries_left = WRITE_WINDOW_RETRIES;
        while index < windows {
            let start = index.saturating_mul(CHUNK_SIZE);
            let end = start.saturating_add(CHUNK_SIZE).min(data.len());
            let addr = offset.saturating_add(start as u32);
            let slice = &data[start..end];
            let window = index.saturating_add(1);
            eprintln!(
                "write-bin window {window}/{windows} offset={addr:#010x} bytes={} elapsed={:?}",
                slice.len(),
                started.elapsed()
            );
            log::info!(
                "write-bin window {window}/{windows} offset={addr:#010x} bytes={}",
                slice.len()
            );
            let mut progress = WriteBinProgress::new(window, windows, slice.len());
            match flasher.write_bin_to_flash(addr, slice, &mut progress) {
                Ok(()) => {
                    index = index.saturating_add(1);
                    retries_left = WRITE_WINDOW_RETRIES;
                }
                Err(error) => {
                    let mapped = map_espflash(error);
                    if retries_left == 0 {
                        return Err(mapped);
                    }
                    retries_left -= 1;
                    eprintln!(
                        "write-bin window {window}/{windows} dropped ({mapped}); reconnecting, {retries_left} retries left"
                    );
                    log::warn!(
                        "write-bin window {window}/{windows} failed ({mapped}); reconnecting ({retries_left} left)"
                    );
                    drop(flasher);
                    flasher = connect(port, Some(ESPFLASH_BAUD))?;
                }
            }
        }
        log::info!("write-bin finished in {:?}", started.elapsed());
        reset_after(&mut flasher)
    }
}

/// In-memory device for host tests. Never opens a port or constructs a flasher.
#[derive(Debug, Clone, Default)]
pub struct MockDevice {
    /// UART log returned by [`DeviceIo::sample_uart`].
    pub uart: String,
    /// `board-info` text.
    pub board_info: String,
    /// Full (or fixture-sized) flash contents.
    pub flash: Vec<u8>,
    /// Recorded write-bin calls: offset and file bytes.
    pub writes: Vec<(u32, Vec<u8>)>,
}

impl DeviceIo for std::cell::RefCell<MockDevice> {
    fn sample_uart(&self, _port: &str) -> Result<String, Error> {
        Ok(self.borrow().uart.clone())
    }

    fn board_info(&self, _port: &str) -> Result<String, Error> {
        Ok(self.borrow().board_info.clone())
    }

    fn read_flash(&self, _port: &str, offset: u32, size: u32) -> Result<Vec<u8>, Error> {
        let flash = &self.borrow().flash;
        let start = offset as usize;
        let end = start.saturating_add(size as usize);
        if end > flash.len() {
            return Err(Error::Device("mock flash shorter than read window".into()));
        }
        Ok(flash[start..end].to_vec())
    }

    fn write_bin(&self, _port: &str, offset: u32, file: &Path) -> Result<(), Error> {
        let bytes = std::fs::read(file)?;
        self.borrow_mut().writes.push((offset, bytes));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{write_bin_percent, write_bin_window_count};
    use crate::CHUNK_SIZE;

    #[test]
    fn write_bin_percent_treats_counts_as_chunks() {
        assert_eq!(write_bin_percent(0, 238), 0);
        assert_eq!(write_bin_percent(119, 238), 50);
        assert_eq!(write_bin_percent(238, 238), 100);
        assert_eq!(write_bin_percent(1, 0), 100);
    }

    #[test]
    fn write_bin_window_count_matches_backup_chunks() {
        assert_eq!(write_bin_window_count(0), 0);
        assert_eq!(write_bin_window_count(1), 1);
        assert_eq!(write_bin_window_count(CHUNK_SIZE), 1);
        assert_eq!(write_bin_window_count(CHUNK_SIZE + 1), 2);
        assert_eq!(write_bin_window_count(32 * CHUNK_SIZE), 32);
    }
}
