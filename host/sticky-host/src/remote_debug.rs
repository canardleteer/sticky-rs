//! UART `pair pin=` scrape and remember-me allowlist.
//!
//! Identity is factory serial and/or CH343 USB serial in gitignored
//! `developer-data/remote-debug/`. Never a MAC. Never store the PIN.

use std::fs;
use std::io::Read;
use std::time::{Duration, Instant};

use seeed_reterminal_sticky::display::{
    page_to_framebuffer, screen_to_framebuffer, PageRotation, HEIGHT, WIDTH,
};
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
        "no new pair pin= on UART (remote-debug: stay on splash; else scene=pair; or pass --pin)"
            .into(),
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
    hold: Option<u32>,
) -> Result<std::path::PathBuf, Error> {
    let dir = layout.remote_debug_snapshots();
    fs::create_dir_all(&dir)?;
    let stem = dir.join(format!("snap-{nonce:016x}"));
    fs::write(stem.with_extension("bw"), bw)?;
    if let Some(red) = red {
        fs::write(stem.with_extension("red"), red)?;
    }
    let png = match hold.and_then(hold_rotation) {
        Some(rotation) => page_png(bw, red, rotation),
        None => framebuffer_png(bw, red, u32::from(WIDTH), u32::from(HEIGHT)),
    };
    if let Ok(png) = png {
        fs::write(stem.with_extension("png"), png)?;
    }
    Ok(stem)
}

/// Snapshot `hold` token from embassy-debug (`0..=3`).
fn hold_rotation(hold: u32) -> Option<PageRotation> {
    match hold {
        0 => Some(PageRotation::Portrait0),
        1 => Some(PageRotation::Portrait180),
        2 => Some(PageRotation::Landscape0),
        3 => Some(PageRotation::Landscape180),
        _ => None,
    }
}

/// MSB-first 1-bit on an 800-wide plane.
fn plane_bit(plane: &[u8], x: u16, y: u16) -> bool {
    if x >= WIDTH || y >= HEIGHT {
        return false;
    }
    let stride = (WIDTH / 8) as usize;
    let i = usize::from(y) * stride + usize::from(x) / 8;
    plane
        .get(i)
        .is_some_and(|byte| (byte >> (7 - (x % 8))) & 1 != 0)
}

/// Uncompressed RGB PNG in **page** space (same origin as `--page` / expect).
///
/// Gray4 DRAW already stores Seeed OTP 180° (`set_gray`). Mono does not.
fn page_png(bw: &[u8], red: Option<&[u8]>, rotation: PageRotation) -> Result<Vec<u8>, Error> {
    let (page_w, page_h) = rotation.page_size();
    let gray4 = red.is_some();
    let mut raw = Vec::with_capacity(((u32::from(page_w) * 3 + 1) * u32::from(page_h)) as usize);
    for py in 0..page_h {
        raw.push(0);
        for px in 0..page_w {
            let Some((hx, hy)) = page_to_framebuffer(px, py, rotation) else {
                return Err(Error::RemoteDebug("png map".into()));
            };
            let (fx, fy) = if gray4 {
                screen_to_framebuffer(hx, hy).ok_or_else(|| Error::RemoteDebug("png map".into()))?
            } else {
                (hx, hy)
            };
            let bit = plane_bit(bw, fx, fy);
            let rbit = red.is_some_and(|plane| plane_bit(plane, fx, fy));
            let (r, g, b) = if rbit {
                (180, 40, 40)
            } else if bit {
                (20, 20, 20)
            } else {
                (250, 250, 250)
            };
            raw.extend_from_slice(&[r, g, b]);
        }
    }
    Ok(encode_rgb_png(u32::from(page_w), u32::from(page_h), &raw))
}

/// Uncompressed RGB PNG in inject/framebuffer space (MSB-first 1-bit).
///
/// Fallback when `hold` is missing. Prefer [`page_png`].
fn framebuffer_png(
    bw: &[u8],
    red: Option<&[u8]>,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, Error> {
    if width == 0 || height == 0 || !width.is_multiple_of(8) {
        return Err(Error::RemoteDebug("png size".into()));
    }
    let stride = (width / 8) as usize;
    let need = stride.saturating_mul(height as usize);
    if bw.len() < need {
        return Err(Error::RemoteDebug("png plane".into()));
    }
    let mut raw = Vec::with_capacity(((width * 3 + 1) * height) as usize);
    for y in 0..height as usize {
        raw.push(0);
        for x in 0..width as usize {
            let byte = bw[y * stride + x / 8];
            let bit = (byte >> (7 - (x % 8))) & 1;
            let rbit = red
                .and_then(|plane| plane.get(y * stride + x / 8))
                .map(|b| (b >> (7 - (x % 8))) & 1)
                .unwrap_or(0);
            let (r, g, b) = if rbit != 0 {
                (180, 40, 40)
            } else if bit != 0 {
                (20, 20, 20)
            } else {
                (250, 250, 250)
            };
            raw.extend_from_slice(&[r, g, b]);
        }
    }
    Ok(encode_rgb_png(width, height, &raw))
}

fn encode_rgb_png(width: u32, height: u32, raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    put_png_chunk(&mut out, *b"IHDR", &ihdr);
    put_png_chunk(&mut out, *b"IDAT", &zlib_store(raw));
    put_png_chunk(&mut out, *b"IEND", &[]);
    out
}

fn put_png_chunk(out: &mut Vec<u8>, ty: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&ty);
    out.extend_from_slice(data);
    let mut crc = crc32_init();
    crc = crc32_update(crc, &ty);
    crc = crc32_update(crc, data);
    out.extend_from_slice(&crc32_finish(crc).to_be_bytes());
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut off = 0;
    while off < data.len() {
        let n = (data.len() - off).min(65535);
        let last = off + n == data.len();
        out.push(u8::from(last));
        let len = n as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&data[off..off + n]);
        off += n;
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for &b in data {
        s1 = (s1 + u32::from(b)) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    (s2 << 16) | s1
}

fn crc32_init() -> u32 {
    0xffff_ffff
}

fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = if crc & 1 == 1 { 0xedb8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    crc
}

fn crc32_finish(crc: u32) -> u32 {
    !crc
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
    fn framebuffer_png_writes_signature() {
        let bw = [0xF0u8, 0x00, 0x0F, 0x00];
        let png = framebuffer_png(&bw, None, 8, 4).expect("png");
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(png.len() > 32);
    }

    fn png_size(png: &[u8]) -> (u32, u32) {
        let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
        (w, h)
    }

    fn set_plane_bit(plane: &mut [u8], x: u16, y: u16) {
        let stride = (WIDTH / 8) as usize;
        let i = usize::from(y) * stride + usize::from(x) / 8;
        plane[i] |= 1 << (7 - (x % 8));
    }

    #[test]
    fn page_png_is_portrait_for_hold_0() {
        let need = (WIDTH as usize / 8) * HEIGHT as usize;
        let mut bw = vec![0u8; need];
        let (hx, hy) = page_to_framebuffer(0, 0, PageRotation::Portrait0).expect("map");
        set_plane_bit(&mut bw, hx, hy);
        let png = page_png(&bw, None, PageRotation::Portrait0).expect("png");
        assert_eq!(png_size(&png), (480, 800));
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    }

    #[test]
    fn page_png_is_landscape_for_hold_2() {
        let need = (WIDTH as usize / 8) * HEIGHT as usize;
        let bw = vec![0u8; need];
        let png = page_png(&bw, None, PageRotation::Landscape0).expect("png");
        assert_eq!(png_size(&png), (800, 480));
    }

    #[test]
    fn gray4_page_png_undoes_otp_180() {
        let need = (WIDTH as usize / 8) * HEIGHT as usize;
        let mut bw = vec![0u8; need];
        let red = vec![0u8; need];
        let (hx, hy) = page_to_framebuffer(0, 0, PageRotation::Portrait0).expect("map");
        let (fx, fy) = screen_to_framebuffer(hx, hy).expect("otp");
        set_plane_bit(&mut bw, fx, fy);
        let png = page_png(&bw, Some(&red), PageRotation::Portrait0).expect("png");
        assert_eq!(png_size(&png), (480, 800));
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
