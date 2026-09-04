//! USB inventory (default) or UART/chip probe.
//!
//! Default is sysfs / by-id inventory (no DTR). `--probe` opens the port:
//! stock UART `serial_number`, then flasher board-info (chip, 32 MB, MAC).
//! `--probe` takes the exclusive UART session lock before any reset.
//!
//! Confirmed on a Sticky CH343: unique QinHeng pick ignores other CDC
//! devices; udev by-id uses an underscore before the USB serial.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::device::DeviceIo;
use crate::identity::{
    parse_board_info, parse_factory_serial, parse_usb_serial_from_port, BoardInfo,
};
use crate::Error;

/// QinHeng CH343P on this product (`lsusb` `ID 1a86:55d3`).
pub const QINHENG_VID: u16 = 0x1A86;
/// QinHeng “USB Single Serial” product id.
pub const QINHENG_PID: u16 = 0x55D3;
/// Espressif USB VID (native USB-Serial/JTAG), not this board's debug connector.
pub const ESPRESSIF_VID: u16 = 0x303A;

/// How a USB serial node relates to the Sticky.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    /// CH343P (`1a86:55d3`) or a QinHeng by-id node.
    StickyCh343,
    /// Espressif native USB (wrong connector for this product).
    EspressifUsb,
    /// Some other USB-serial adapter.
    Other,
}

/// One discovered serial node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Stable by-id path when udev created one.
    pub by_id: Option<PathBuf>,
    /// Kernel ACM/USB-serial name (`ttyACM1`, …).
    pub tty_name: Option<String>,
    /// CH343 serial from by-id or sysfs.
    pub usb_serial: Option<String>,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub product: Option<String>,
    pub kind: PortKind,
}

impl Candidate {
    /// Preferred `ESPFLASH_PORT` value (by-id if present).
    #[must_use]
    pub fn preferred_port(&self) -> Option<String> {
        if let Some(path) = &self.by_id {
            return Some(path.display().to_string());
        }
        self.tty_name
            .as_ref()
            .map(|tty| host_dev_dir().join(tty).display().to_string())
    }
}

/// Host device directory (`/dev`) without embedding a gated tty or serial path.
#[must_use]
pub fn host_dev_dir() -> PathBuf {
    PathBuf::from("/dev")
}

fn serial_by_id_dir() -> PathBuf {
    host_dev_dir().join("serial").join("by-id")
}

fn sys_class_tty_dir() -> PathBuf {
    PathBuf::from("/sys/class/tty")
}

/// Classify from USB ids and/or a by-id path string.
#[must_use]
pub fn classify(vid: Option<u16>, pid: Option<u16>, port_path: Option<&str>) -> PortKind {
    if vid == Some(QINHENG_VID) && pid == Some(QINHENG_PID) {
        return PortKind::StickyCh343;
    }
    if port_path.and_then(parse_usb_serial_from_port).is_some() {
        return PortKind::StickyCh343;
    }
    if vid == Some(ESPRESSIF_VID) {
        return PortKind::EspressifUsb;
    }
    PortKind::Other
}

fn parse_hex_u16(raw: &str) -> Option<u16> {
    u16::from_str_radix(raw.trim().trim_start_matches("0x"), 16).ok()
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// USB device that backs a QinHeng ACM node (`/dev/bus/usb/{busnum}/{devnum}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsbDeviceKey {
    /// `busnum` from sysfs.
    pub busnum: u8,
    /// `devnum` from sysfs (USB device address).
    pub devnum: u8,
    pub vid: u16,
    pub pid: u16,
}

/// Map `--port` / by-id / ACM name to the USB device, without opening the TTY.
pub fn usb_device_key_for_port(port: &str) -> Result<UsbDeviceKey, Error> {
    usb_device_key_from(port, &sys_class_tty_dir())
}

/// Testable [`usb_device_key_for_port`].
pub fn usb_device_key_from(port: &str, sys_tty: &Path) -> Result<UsbDeviceKey, Error> {
    let tty = tty_name_for_port(port)
        .ok_or_else(|| Error::Device(format!("cannot map {} to a ttyACM/ttyUSB name", port)))?;
    usb_device_key_for_tty(sys_tty, &tty).ok_or_else(|| {
        Error::Device(format!(
            "no USB busnum/devnum in sysfs for {tty}; is the CH343 still plugged in?"
        ))
    })
}

fn tty_name_for_port(port: &str) -> Option<String> {
    if let Some(name) = port_file_name(port) {
        if name.starts_with("ttyACM") || name.starts_with("ttyUSB") {
            return Some(name.to_string());
        }
    }
    tty_from_by_id_link(Path::new(port))
}

fn usb_device_key_for_tty(sys_tty: &Path, tty: &str) -> Option<UsbDeviceKey> {
    let info = usb_sysfs_for_tty(sys_tty, tty)?;
    Some(UsbDeviceKey {
        busnum: info.busnum?,
        devnum: info.devnum?,
        vid: info.vid?,
        pid: info.pid?,
    })
}

struct UsbSysfs {
    vid: Option<u16>,
    pid: Option<u16>,
    product: Option<String>,
    serial: Option<String>,
    busnum: Option<u8>,
    devnum: Option<u8>,
}

fn parse_u8_dec(raw: &str) -> Option<u8> {
    raw.trim().parse().ok()
}

fn usb_sysfs_for_tty(sys_tty: &Path, tty: &str) -> Option<UsbSysfs> {
    let start = sys_tty.join(tty).join("device");
    let mut cur = fs::canonicalize(&start).ok()?;
    for _ in 0..10 {
        let vendor = cur.join("idVendor");
        if vendor.is_file() {
            return Some(UsbSysfs {
                vid: read_trimmed(&vendor).and_then(|s| parse_hex_u16(&s)),
                pid: read_trimmed(&cur.join("idProduct")).and_then(|s| parse_hex_u16(&s)),
                product: read_trimmed(&cur.join("product")),
                serial: read_trimmed(&cur.join("serial")),
                busnum: read_trimmed(&cur.join("busnum")).and_then(|s| parse_u8_dec(&s)),
                devnum: read_trimmed(&cur.join("devnum")).and_then(|s| parse_u8_dec(&s)),
            });
        }
        cur = cur.parent()?.to_path_buf();
    }
    None
}

fn usb_info_for_tty(
    sys_tty: &Path,
    tty: &str,
) -> (Option<u16>, Option<u16>, Option<String>, Option<String>) {
    match usb_sysfs_for_tty(sys_tty, tty) {
        Some(info) => (info.vid, info.pid, info.product, info.serial),
        None => (None, None, None, None),
    }
}

fn tty_from_by_id_link(link: &Path) -> Option<String> {
    let target = fs::read_link(link).ok()?;
    target
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .filter(|n| n.starts_with("ttyACM") || n.starts_with("ttyUSB"))
}

/// Scan udev by-id plus ACM/USB-serial nodes (paths constructed, not literals).
pub fn scan() -> Result<Vec<Candidate>, Error> {
    scan_from(&serial_by_id_dir(), &host_dev_dir(), &sys_class_tty_dir())
}

/// Testable scan against fixture directories (no live `/dev` required).
pub fn scan_from(
    by_id_dir: &Path,
    dev_dir: &Path,
    sys_tty: &Path,
) -> Result<Vec<Candidate>, Error> {
    let mut by_tty: BTreeMap<String, Candidate> = BTreeMap::new();
    let mut by_id_only = Vec::new();

    if by_id_dir.is_dir() {
        for entry in fs::read_dir(by_id_dir)? {
            let entry = entry?;
            let path = entry.path();
            let path_str = path.to_string_lossy();
            let tty = tty_from_by_id_link(&path);
            let usb_from_name = parse_usb_serial_from_port(&path_str);
            let (vid, pid, product, sys_serial) = tty
                .as_deref()
                .map(|tty| usb_info_for_tty(sys_tty, tty))
                .unwrap_or((None, None, None, None));
            let usb_serial = usb_from_name.or(sys_serial);
            let kind = classify(vid, pid, Some(path_str.as_ref()));
            let cand = Candidate {
                by_id: Some(path),
                tty_name: tty.clone(),
                usb_serial,
                vid,
                pid,
                product,
                kind,
            };
            if let Some(tty) = tty {
                by_tty.insert(tty, cand);
            } else {
                by_id_only.push(cand);
            }
        }
    }

    if dev_dir.is_dir() {
        for entry in fs::read_dir(dev_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.starts_with("ttyACM") && !name.starts_with("ttyUSB") {
                continue;
            }
            if by_tty.contains_key(name) {
                continue;
            }
            let (vid, pid, product, serial) = usb_info_for_tty(sys_tty, name);
            let kind = classify(vid, pid, None);
            by_tty.insert(
                name.to_string(),
                Candidate {
                    by_id: None,
                    tty_name: Some(name.to_string()),
                    usb_serial: serial,
                    vid,
                    pid,
                    product,
                    kind,
                },
            );
        }
    }

    let mut out: Vec<_> = by_tty.into_values().collect();
    out.extend(by_id_only);
    out.sort_by_cached_key(|a| a.preferred_port());
    Ok(out)
}

fn kind_label(kind: PortKind) -> &'static str {
    match kind {
        PortKind::StickyCh343 => "Sticky CH343P (QinHeng 1a86:55d3)",
        PortKind::EspressifUsb => "Espressif native USB (not this board's USB-C UART)",
        PortKind::Other => "other USB-serial",
    }
}

fn listed_for_inventory(candidates: &[Candidate], all_devices: bool) -> Vec<&Candidate> {
    if all_devices {
        candidates.iter().collect()
    } else {
        candidates
            .iter()
            .filter(|c| c.kind == PortKind::StickyCh343)
            .collect()
    }
}

/// Print inventory. Does not open a serial port.
///
/// Default lists QinHeng CH343 only. `--all-devices` includes every USB-serial
/// node (Espressif native USB, other adapters).
pub fn print_inventory(candidates: &[Candidate], all_devices: bool) {
    let listed = listed_for_inventory(candidates, all_devices);
    let hidden = candidates.len().saturating_sub(listed.len());
    let sticky: Vec<_> = candidates
        .iter()
        .filter(|c| c.kind == PortKind::StickyCh343)
        .collect();

    if listed.is_empty() {
        if candidates.is_empty() {
            println!("detect-connected: no USB-serial nodes found");
        } else {
            println!("detect-connected: no QinHeng 1a86:55d3 Sticky UART classified");
            if hidden > 0 {
                println!("({hidden} other USB-serial node(s) omitted; pass --all-devices)");
            }
        }
        return;
    }

    if all_devices {
        println!("detect-connected: {} USB-serial node(s)", listed.len());
    } else {
        println!("detect-connected: {} Sticky CH343 node(s)", listed.len());
    }
    for (i, c) in listed.iter().enumerate() {
        println!("{}. {}", i + 1, kind_label(c.kind));
        if let Some(p) = &c.by_id {
            println!("   by-id: {}", p.display());
        } else {
            println!("   by-id: (none; unstable ACM node)");
        }
        match &c.tty_name {
            Some(t) => println!("   kernel: {t}"),
            None => println!("   kernel: (unknown)"),
        }
        match (c.vid, c.pid) {
            (Some(v), Some(p)) => println!("   vid:pid: {v:04x}:{p:04x}"),
            _ => println!("   vid:pid: (not in sysfs)"),
        }
        if let Some(product) = &c.product {
            println!("   product: {product}");
        }
        if let Some(serial) = &c.usb_serial {
            println!("   usb serial: {serial}");
        }
        if let Some(port) = c.preferred_port() {
            if c.kind == PortKind::StickyCh343 {
                println!("   ESPFLASH_PORT={port}");
            } else {
                println!("   path: {port}");
            }
        }
    }
    match sticky.len() {
        1 => {
            if let Some(port) = sticky[0].preferred_port() {
                println!("suggested: export ESPFLASH_PORT={port}");
            }
        }
        n if n > 1 => println!("multiple Sticky CH343 nodes; pass --port to --probe"),
        _ => println!("no QinHeng 1a86:55d3 Sticky UART classified"),
    }
    if hidden > 0 {
        println!("({hidden} other USB-serial node(s) omitted; pass --all-devices)");
    }
}

fn port_file_name(port: &str) -> Option<&str> {
    Path::new(port).file_name()?.to_str()
}

fn candidate_matches(candidate: &Candidate, port: &str) -> bool {
    if let Some(by_id) = &candidate.by_id {
        if by_id == Path::new(port) {
            return true;
        }
        if by_id.file_name().and_then(|n| n.to_str()) == port_file_name(port) {
            return true;
        }
    }
    if candidate.preferred_port().as_deref() == Some(port) {
        return true;
    }
    if let Some(tty) = &candidate.tty_name {
        if port_file_name(port) == Some(tty.as_str()) {
            return true;
        }
    }
    matches!(
        (
            parse_usb_serial_from_port(port),
            candidate.usb_serial.as_deref(),
        ),
        (Some(from_port), Some(from_usb)) if from_port == from_usb
    )
}

/// Refuse a port that is not this product's QinHeng CH343 (`1a86:55d3`).
///
/// Runs before DTR / flasher connect. Chip `ESP32-S3` and 32 MB board-info
/// remain silicon checks after the UART is open.
pub fn require_sticky_ch343(port: &str) -> Result<(), Error> {
    require_sticky_ch343_from(port, &scan()?, &sys_class_tty_dir())
}

/// Testable [require_sticky_ch343] against a fixture inventory.
pub fn require_sticky_ch343_from(
    port: &str,
    candidates: &[Candidate],
    sys_tty: &Path,
) -> Result<(), Error> {
    let (kind, vid, pid) =
        if let Some(found) = candidates.iter().find(|c| candidate_matches(c, port)) {
            (found.kind, found.vid, found.pid)
        } else {
            let tty = port_file_name(port)
                .filter(|name| name.starts_with("ttyACM") || name.starts_with("ttyUSB"));
            let (vid, pid, _, _) = tty
                .map(|tty| usb_info_for_tty(sys_tty, tty))
                .unwrap_or((None, None, None, None));
            (classify(vid, pid, Some(port)), vid, pid)
        };
    match kind {
        PortKind::StickyCh343 => {
            if parse_usb_serial_from_port(port).is_none() {
                log::warn!(
                    "QinHeng CH343 confirmed, but this is not a by-id node; ACM numbers move and MANIFEST usb_serial may be empty"
                );
            }
            Ok(())
        }
        PortKind::EspressifUsb => Err(Error::EspressifNativeUsb),
        PortKind::Other => {
            if vid.is_none() && pid.is_none() && parse_usb_serial_from_port(port).is_none() {
                Err(Error::UnclassifiedUsbPort)
            } else {
                Err(Error::NotStickyUart { vid, pid })
            }
        }
    }
}

fn pick_sticky_port(port: Option<String>, candidates: &[Candidate]) -> Result<String, Error> {
    if let Some(port) = port {
        return Ok(port);
    }
    let sticky: Vec<_> = candidates
        .iter()
        .filter(|c| c.kind == PortKind::StickyCh343)
        .collect();
    match sticky.len() {
        0 => Err(Error::MissingStickyUart),
        1 => sticky[0].preferred_port().ok_or(Error::MissingStickyUart),
        _ => Err(Error::AmbiguousStickyUart),
    }
}

/// After a listen ends, keep `preferred` if that node is still there.
///
/// Otherwise pick the unique Sticky CH343 again. Dropping a CDC listen
/// reattaches `cdc-acm`; the ACM path can move.
pub fn port_after_listen(preferred: &str) -> Result<String, Error> {
    if Path::new(preferred).exists() {
        Ok(preferred.to_string())
    } else {
        resolve_sticky_port(None)
    }
}

/// Pick `--port` / `ESPFLASH_PORT`, or the unique Sticky CH343, then refuse a
/// non-QinHeng plug before DTR.
pub fn resolve_sticky_port(explicit: Option<String>) -> Result<String, Error> {
    resolve_sticky_port_from(explicit, &scan()?, &sys_class_tty_dir())
}

/// Testable [resolve_sticky_port].
pub fn resolve_sticky_port_from(
    explicit: Option<String>,
    candidates: &[Candidate],
    sys_tty: &Path,
) -> Result<String, Error> {
    let port = pick_sticky_port(explicit, candidates)?;
    require_sticky_ch343_from(&port, candidates, sys_tty)?;
    Ok(port)
}

/// Flasher board-info plus USB serial parsed from the port path.
pub fn read_live_board<D: DeviceIo>(device: &D, port: &str) -> Result<(String, BoardInfo), Error> {
    let info_text = device.board_info(port)?;
    let mut board = parse_board_info(&info_text)?;
    board.identity.usb_serial = parse_usb_serial_from_port(port);
    Ok((info_text, board))
}

/// UART `serial_number` then flasher board-info. Resets the chip (DTR/RTS).
///
/// Missing stock `serial_number` is reported, not fatal. Board-info must
/// parse as ESP32-S3 / 32 MB (silicon checks after the QinHeng USB gate).
pub fn probe<D: DeviceIo>(device: &D, port: &str) -> Result<(), Error> {
    println!("probe port: {port}");
    let uart = device.sample_uart(port)?;
    match parse_factory_serial(&uart) {
        Ok(serial) => println!("factory serial_number: {serial}"),
        Err(Error::MissingFactorySerial) => {
            println!("factory serial_number: (not in UART log; need stock firmware after reset)");
        }
        Err(error) => println!("factory serial_number: ({error})"),
    }
    let (text, _) = read_live_board(device, port)?;
    print!("{text}");
    Ok(())
}

/// USB inventory, optionally `--probe`.
pub fn run<D: DeviceIo>(
    device: &D,
    probe_chip: bool,
    port: Option<String>,
    all_devices: bool,
) -> Result<(), Error> {
    let candidates = scan()?;
    print_inventory(&candidates, all_devices);
    let _ = io::stdout().flush();
    if probe_chip {
        let port = resolve_sticky_port_from(port, &candidates, &sys_class_tty_dir())?;
        let _uart = crate::uart_lock::try_acquire(&port, "detect-connected --probe")?;
        probe(device, &port)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{parse_usb_serial_from_port, qinheng_marker};
    use std::path::PathBuf;

    #[test]
    fn qinheng_ids_are_sticky() {
        assert_eq!(
            classify(Some(QINHENG_VID), Some(QINHENG_PID), None),
            PortKind::StickyCh343
        );
    }

    #[test]
    fn espressif_vid_is_not_ch343() {
        assert_eq!(
            classify(Some(ESPRESSIF_VID), Some(0x1001), None),
            PortKind::EspressifUsb
        );
    }

    #[test]
    fn by_id_name_classifies_without_sysfs() {
        let marker = qinheng_marker();
        let path = format!("prefix/{marker}_TESTUSB-if00");
        assert_eq!(classify(None, None, Some(&path)), PortKind::StickyCh343);
        assert_eq!(
            parse_usb_serial_from_port(&path).as_deref(),
            Some("TESTUSB")
        );
    }

    #[test]
    fn scan_by_id_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let by_id = tmp.path().join("by-id");
        let dev = tmp.path().join("dev");
        let sys_tty = tmp.path().join("sys-tty");
        fs::create_dir_all(&by_id).unwrap();
        fs::create_dir_all(&dev).unwrap();
        let marker = qinheng_marker();
        let name = format!("{marker}_TESTUSB-if00");
        std::os::unix::fs::symlink("../../ttyACM9", by_id.join(&name)).unwrap();
        let found = scan_from(&by_id, &dev, &sys_tty).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, PortKind::StickyCh343);
        assert_eq!(found[0].usb_serial.as_deref(), Some("TESTUSB"));
        assert_eq!(found[0].tty_name.as_deref(), Some("ttyACM9"));
        assert!(found[0].preferred_port().unwrap().contains(&name));
    }

    fn write_sysfs_usb(sys_tty: &Path, tty: &str, vid: &str, pid: &str, serial: &str) {
        let usb = sys_tty.join(tty).join("usb");
        fs::create_dir_all(&usb).unwrap();
        fs::write(usb.join("idVendor"), vid).unwrap();
        fs::write(usb.join("idProduct"), pid).unwrap();
        fs::write(usb.join("product"), "fixture").unwrap();
        fs::write(usb.join("serial"), serial).unwrap();
        fs::write(usb.join("busnum"), "3").unwrap();
        fs::write(usb.join("devnum"), "14").unwrap();
        std::os::unix::fs::symlink("usb", sys_tty.join(tty).join("device")).unwrap();
    }

    #[test]
    fn usb_device_key_reads_bus_and_address() {
        let tmp = tempfile::tempdir().unwrap();
        let sys_tty = tmp.path().join("sys-tty");
        write_sysfs_usb(&sys_tty, "ttyACM3", "1a86", "55d3", "CH343SERIAL");
        let key = usb_device_key_from("ttyACM3", &sys_tty).unwrap();
        assert_eq!(
            key,
            UsbDeviceKey {
                busnum: 3,
                devnum: 14,
                vid: QINHENG_VID,
                pid: QINHENG_PID,
            }
        );
    }

    #[test]
    fn scan_acm_node_from_sysfs_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let by_id = tmp.path().join("by-id");
        let dev = tmp.path().join("dev");
        let sys_tty = tmp.path().join("sys-tty");
        fs::create_dir_all(&by_id).unwrap();
        fs::create_dir_all(&dev).unwrap();
        fs::write(dev.join("ttyACM3"), b"").unwrap();
        write_sysfs_usb(&sys_tty, "ttyACM3", "1a86", "55d3", "CH343SERIAL");
        let found = scan_from(&by_id, &dev, &sys_tty).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, PortKind::StickyCh343);
        assert_eq!(found[0].vid, Some(QINHENG_VID));
        assert_eq!(found[0].pid, Some(QINHENG_PID));
        assert_eq!(found[0].usb_serial.as_deref(), Some("CH343SERIAL"));
        assert_eq!(found[0].tty_name.as_deref(), Some("ttyACM3"));
    }

    fn sticky_candidate(port: &str) -> Candidate {
        Candidate {
            by_id: Some(PathBuf::from(port)),
            tty_name: Some("ttyACM1".into()),
            usb_serial: Some("TESTUSB".into()),
            vid: Some(QINHENG_VID),
            pid: Some(QINHENG_PID),
            product: None,
            kind: PortKind::StickyCh343,
        }
    }

    #[test]
    fn probe_port_explicit_wins() {
        assert_eq!(
            pick_sticky_port(Some("explicit".into()), &[]).unwrap(),
            "explicit"
        );
    }

    #[test]
    fn probe_port_unique_sticky() {
        let c = sticky_candidate("/tmp/by-id-sticky");
        assert_eq!(pick_sticky_port(None, &[c]).unwrap(), "/tmp/by-id-sticky");
    }

    #[test]
    fn probe_port_missing_or_ambiguous() {
        assert!(matches!(
            pick_sticky_port(None, &[]),
            Err(Error::MissingStickyUart)
        ));
        let a = sticky_candidate("/tmp/a");
        let b = sticky_candidate("/tmp/b");
        assert!(matches!(
            pick_sticky_port(None, &[a, b]),
            Err(Error::AmbiguousStickyUart)
        ));
    }

    #[test]
    fn parse_sysfs_hex_ids() {
        assert_eq!(parse_hex_u16("1a86\n"), Some(0x1A86));
        assert_eq!(parse_hex_u16("55d3"), Some(0x55D3));
    }

    fn other_candidate() -> Candidate {
        Candidate {
            by_id: Some(PathBuf::from("/tmp/entropy")),
            tty_name: Some("ttyACM0".into()),
            usb_serial: Some("OTHER".into()),
            vid: Some(0x20DF),
            pid: Some(0x0001),
            product: None,
            kind: PortKind::Other,
        }
    }

    #[test]
    fn inventory_hides_non_sticky_unless_all_devices() {
        let sticky = sticky_candidate("/tmp/by-id-sticky");
        let other = other_candidate();
        let all = [sticky.clone(), other];
        let listed = listed_for_inventory(&all, false);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].kind, PortKind::StickyCh343);
        let listed_all = listed_for_inventory(&all, true);
        assert_eq!(listed_all.len(), 2);
    }

    #[test]
    fn require_accepts_qinheng_by_id_without_scan() {
        let marker = qinheng_marker();
        let port = format!("prefix/{marker}_TESTUSB-if00");
        require_sticky_ch343_from(&port, &[], Path::new("/no-sys")).unwrap();
    }

    #[test]
    fn require_accepts_sysfs_qinheng_acm() {
        let tmp = tempfile::tempdir().unwrap();
        let sys_tty = tmp.path().join("sys-tty");
        write_sysfs_usb(&sys_tty, "ttyACM3", "1a86", "55d3", "CH343SERIAL");
        require_sticky_ch343_from("ttyACM3", &[], &sys_tty).unwrap();
    }

    #[test]
    fn require_refuses_espressif_before_open() {
        let c = Candidate {
            by_id: None,
            tty_name: Some("ttyACM0".into()),
            usb_serial: None,
            vid: Some(ESPRESSIF_VID),
            pid: Some(0x1001),
            product: None,
            kind: PortKind::EspressifUsb,
        };
        assert!(matches!(
            require_sticky_ch343_from("ttyACM0", std::slice::from_ref(&c), Path::new("/no-sys")),
            Err(Error::EspressifNativeUsb)
        ));
        assert!(matches!(
            resolve_sticky_port_from(
                Some("ttyACM0".into()),
                std::slice::from_ref(&c),
                Path::new("/no-sys")
            ),
            Err(Error::EspressifNativeUsb)
        ));
    }

    #[test]
    fn require_refuses_other_vid() {
        let c = Candidate {
            by_id: None,
            tty_name: Some("ttyUSB0".into()),
            usb_serial: None,
            vid: Some(0x0403),
            pid: Some(0x6001),
            product: None,
            kind: PortKind::Other,
        };
        assert!(matches!(
            require_sticky_ch343_from("ttyUSB0", &[c], Path::new("/no-sys")),
            Err(Error::NotStickyUart {
                vid: Some(0x0403),
                pid: Some(0x6001)
            })
        ));
    }

    #[test]
    fn require_refuses_unclassified_acm() {
        assert!(matches!(
            require_sticky_ch343_from("ttyACM1", &[], Path::new("/no-sys")),
            Err(Error::UnclassifiedUsbPort)
        ));
    }

    #[test]
    fn port_after_listen_keeps_a_path_that_still_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ttyACM0");
        std::fs::write(&path, b"").unwrap();
        assert_eq!(
            port_after_listen(path.to_str().unwrap()).unwrap(),
            path.to_str().unwrap()
        );
    }
}
