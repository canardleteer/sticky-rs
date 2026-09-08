//! UART `pair pin=` scrape and remember-me allowlist.
//!
//! Identity is factory serial and/or CH343 USB serial in gitignored
//! `developer-data/remote-debug/`. Never a MAC. Never store the PIN.

use std::fs;
use std::io::Read;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::identity::{parse_usb_serial_from_port, validate_factory_serial};
use crate::original::Layout;
use crate::Error;

/// One allowlisted desk unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RememberedUnit {
    /// Factory `serial_number` when known. Never a MAC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory_serial: Option<String>,
    /// CH343 USB serial from a QinHeng by-id path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usb_serial: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AllowlistFile {
    #[serde(default)]
    units: Vec<RememberedUnit>,
}

/// Parse `embassy-debug: t=1204 pair pin=000042` (six digits, leading zeros).
#[must_use]
pub fn parse_pair_pin_line(line: &str) -> Option<u32> {
    let rest = line.split("pair pin=").nth(1)?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() != 6 {
        return None;
    }
    digits.parse().ok()
}

/// True when the line is a finished pair (`pair ok`).
#[must_use]
pub fn parse_pair_ok_line(line: &str) -> bool {
    line.contains("pair ok")
}

/// Scan a UART buffer for pins and `pair ok` (host tests / fixtures).
#[must_use]
pub fn scan_pair_uart(text: &str) -> (Vec<u32>, bool) {
    let mut pins = Vec::new();
    let mut ok = false;
    for line in text.lines() {
        if let Some(pin) = parse_pair_pin_line(line) {
            pins.push(pin);
        }
        if parse_pair_ok_line(line) {
            ok = true;
        }
    }
    (pins, ok)
}

/// Read CDC bytes until a **new** `pair pin=` or timeout.
///
/// `seen` pins already observed this listen are ignored so a leftover
/// glass PIN is not reused. Returns the first unseen six digits.
///
/// # Errors
///
/// [`Error::RemoteDebug`] on timeout or interrupt.
pub fn wait_new_pair_pin<R: Read>(
    reader: &mut R,
    seen: &mut Vec<u32>,
    budget: Duration,
) -> Result<u32, Error> {
    let deadline = Instant::now() + budget;
    let mut leftover = String::new();
    let mut buf = [0u8; 256];
    while Instant::now() < deadline {
        if crate::cdc_listen::interrupt_requested() {
            return Err(Error::RemoteDebug("interrupted".into()));
        }
        match reader.read(&mut buf) {
            Ok(0) => continue,
            Ok(n) => {
                leftover.push_str(&String::from_utf8_lossy(&buf[..n]));
                while let Some(idx) = leftover.find('\n') {
                    let line: String = leftover.drain(..=idx).collect();
                    if let Some(pin) = parse_pair_pin_line(&line) {
                        if !seen.contains(&pin) {
                            seen.push(pin);
                            return Ok(pin);
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(Error::RemoteDebug(
        "no new pair pin= on UART (walk to scene=pair, or pass --pin)".into(),
    ))
}

/// USB serial from a QinHeng by-id `--port` / `ESPFLASH_PORT` value.
#[must_use]
pub fn usb_serial_from_port(port: &str) -> Option<String> {
    parse_usb_serial_from_port(port)
}

/// Load the gitignored allowlist (empty if missing).
///
/// # Errors
///
/// YAML parse failure.
pub fn load_allowlist(layout: &Layout) -> Result<Vec<RememberedUnit>, Error> {
    let path = layout.remote_debug_allowlist();
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    let file: AllowlistFile =
        noyalib::from_str(&text).map_err(|error| Error::Yaml(error.to_string()))?;
    Ok(file.units)
}

/// True when `factory` or `usb` matches an allowlisted unit.
#[must_use]
pub fn is_remembered(
    units: &[RememberedUnit],
    factory_serial: Option<&str>,
    usb_serial: Option<&str>,
) -> bool {
    units.iter().any(|unit| {
        match_opt(unit.factory_serial.as_deref(), factory_serial)
            || match_opt(unit.usb_serial.as_deref(), usb_serial)
    })
}

fn match_opt(stored: Option<&str>, got: Option<&str>) -> bool {
    matches!((stored, got), (Some(a), Some(b)) if !a.is_empty() && a == b)
}

/// Append this unit (no PIN, no MAC).
///
/// # Errors
///
/// Unsafe serial, I/O, or YAML.
pub fn remember_unit(
    layout: &Layout,
    factory_serial: Option<&str>,
    usb_serial: Option<&str>,
) -> Result<(), Error> {
    if factory_serial.is_none() && usb_serial.is_none() {
        return Err(Error::RemoteDebug(
            "--remember needs a factory serial or CH343 USB serial".into(),
        ));
    }
    if let Some(serial) = factory_serial {
        validate_factory_serial(serial)?;
    }
    if let Some(serial) = usb_serial {
        validate_factory_serial(serial)?;
    }
    let mut units = load_allowlist(layout)?;
    if is_remembered(&units, factory_serial, usb_serial) {
        return Ok(());
    }
    units.push(RememberedUnit {
        factory_serial: factory_serial.map(str::to_string),
        usb_serial: usb_serial.map(str::to_string),
    });
    write_allowlist(layout, &units)
}

fn write_allowlist(layout: &Layout, units: &[RememberedUnit]) -> Result<(), Error> {
    let dir = layout.remote_debug_dir();
    fs::create_dir_all(&dir)?;
    let file = AllowlistFile {
        units: units.to_vec(),
    };
    let yaml = noyalib::to_string(&file).map_err(|error| Error::Yaml(error.to_string()))?;
    fs::write(layout.remote_debug_allowlist(), yaml)?;
    Ok(())
}

/// Save snapshot planes under `developer-data/remote-debug/snapshots/`
/// (nonce in the filename; no serial).
///
/// # Errors
///
/// I/O.
pub fn write_snapshot_planes(
    layout: &Layout,
    nonce: u64,
    bw: &[u8],
    red: Option<&[u8]>,
) -> Result<std::path::PathBuf, Error> {
    let dir = layout.remote_debug_snapshots();
    fs::create_dir_all(&dir)?;
    let stem = dir.join(format!("snap-{nonce:016x}"));
    fs::write(stem.with_extension("bw"), bw)?;
    if let Some(red) = red {
        fs::write(stem.with_extension("red"), red)?;
    }
    Ok(stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Layout;

    #[test]
    fn parses_embassy_debug_pair_pin_line() {
        assert_eq!(
            parse_pair_pin_line("embassy-debug: t=1204 pair pin=000042"),
            Some(42)
        );
        assert_eq!(parse_pair_pin_line("embassy-debug: t=1800 pair ok"), None);
        assert!(parse_pair_ok_line("embassy-debug: t=1800 pair ok"));
        let (pins, ok) =
            scan_pair_uart("embassy-debug: t=1 pair pin=000001\nembassy-debug: t=2 pair ok\n");
        assert_eq!(pins, [1]);
        assert!(ok);
    }

    #[test]
    fn allowlist_matches_fixture_serials_not_macs() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::from_developer_data_root(tmp.path());
        remember_unit(&layout, Some("TESTFACTORY001"), Some("USBFIXTURE1")).unwrap();
        let units = load_allowlist(&layout).unwrap();
        assert!(is_remembered(&units, Some("TESTFACTORY001"), None));
        assert!(is_remembered(&units, None, Some("USBFIXTURE1")));
        assert!(!is_remembered(&units, Some("OTHER"), Some("NOPE")));
        let yaml = std::fs::read_to_string(layout.remote_debug_allowlist()).unwrap();
        assert!(!yaml.to_ascii_lowercase().contains("pin="));
        assert!(!yaml.contains("aa:bb"));
    }
}
