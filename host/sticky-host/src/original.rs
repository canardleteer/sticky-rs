//! Snapshots under `backups/original/` and `backups/captures/`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::identity::{parse_usb_serial_from_port, LiveIdentity};
use crate::manifest::{Manifest, SnapshotKind};
use crate::Error;

/// Filesystem layout for gitignored backups.
#[derive(Debug, Clone)]
pub struct Layout {
    /// `backups/` directory (usually `{repo}/backups`).
    pub backups_root: PathBuf,
}

impl Layout {
    /// `{repo}/backups`.
    #[must_use]
    pub fn from_repo_root(repo_root: impl Into<PathBuf>) -> Self {
        let backups_root = repo_root.into().join("backups");
        Self { backups_root }
    }

    /// `backups/original/`.
    #[must_use]
    pub fn originals_dir(&self) -> PathBuf {
        self.backups_root.join("original")
    }

    /// `backups/original/{factory-serial}/`.
    #[must_use]
    pub fn original_dir(&self, factory_serial: &str) -> PathBuf {
        self.originals_dir().join(factory_serial)
    }

    /// `backups/captures/`.
    #[must_use]
    pub fn captures_dir(&self) -> PathBuf {
        self.backups_root.join("captures")
    }

    /// `backups/captures/{unit-id}/{slug}/`.
    #[must_use]
    pub fn capture_dir(&self, unit_id: &str, slug: &str) -> PathBuf {
        self.captures_dir().join(unit_id).join(slug)
    }

    /// `backups/original/{factory-serial}/learn-uart/` (session YAML; not the dump).
    #[must_use]
    pub fn learn_uart_dir(&self, factory_serial: &str) -> PathBuf {
        self.original_dir(factory_serial).join("learn-uart")
    }

    /// `learn-uart/` under a bound snapshot (original or capture).
    #[must_use]
    pub fn learn_uart_in(snapshot_dir: &Path) -> PathBuf {
        snapshot_dir.join("learn-uart")
    }
}

/// An on-disk snapshot plus its MANIFEST.
#[derive(Debug, Clone)]
pub struct OriginalBackup {
    /// Directory path.
    pub dir: PathBuf,
    /// Parsed MANIFEST.
    pub manifest: Manifest,
}

impl OriginalBackup {
    /// True when this tree is a factory original (path or kind).
    #[must_use]
    pub fn is_original(&self) -> bool {
        matches!(self.manifest.kind, SnapshotKind::Original)
            && self
                .dir
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|name| name == "original")
    }
}

/// Safety net for `flash-app` / learn-uart: original preferred, else a capture.
#[derive(Debug, Clone)]
pub struct SafetyNet {
    /// Bound snapshot.
    pub snapshot: OriginalBackup,
    /// True when the snapshot is under `original/`.
    pub is_original: bool,
}

/// Load `MANIFEST.yaml`, or existing `MANIFEST.json`.
pub fn load_manifest(dir: &Path) -> Result<Manifest, Error> {
    let yaml = dir.join("MANIFEST.yaml");
    if yaml.is_file() {
        let text = fs::read_to_string(yaml)?;
        return noyalib::from_str(&text).map_err(|error| Error::Yaml(error.to_string()));
    }
    let json = dir.join("MANIFEST.json");
    if json.is_file() {
        let text = fs::read_to_string(json)?;
        return Ok(serde_json::from_str(&text)?);
    }
    Err(Error::Import(format!(
        "no MANIFEST.yaml or MANIFEST.json in {}",
        dir.display()
    )))
}

fn is_usable_dir(path: &Path) -> bool {
    path.is_dir()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !name.ends_with(".partial"))
}

/// flash-app, restore, and confirm: refuse unless a matching original exists.
pub fn require_original_backup(
    layout: &Layout,
    live: &LiveIdentity,
) -> Result<OriginalBackup, Error> {
    let mut matches = snapshots_for_mac(&list_originals(layout)?, live);
    match matches.len() {
        0 => Err(Error::MissingOriginal),
        1 => {
            let found = matches.pop().unwrap();
            bind_check(&found.manifest.identity(), live)?;
            Ok(found)
        }
        _ => Err(Error::AmbiguousOriginal),
    }
}

/// Prefer a bound original; else the unique capture for this MAC.
pub fn require_safety_net(layout: &Layout, live: &LiveIdentity) -> Result<SafetyNet, Error> {
    match require_original_backup(layout, live) {
        Ok(snapshot) => {
            return Ok(SafetyNet {
                is_original: true,
                snapshot,
            });
        }
        Err(Error::MissingOriginal) => {}
        Err(error) => return Err(error),
    }
    let mut matches = snapshots_for_mac(&list_captures(layout)?, live);
    match matches.len() {
        0 => Err(Error::MissingOriginal),
        1 => {
            let found = matches.pop().unwrap();
            bind_check(&found.manifest.identity(), live)?;
            Ok(SafetyNet {
                is_original: false,
                snapshot: found,
            })
        }
        _ => Err(Error::AmbiguousCapture),
    }
}

/// Bind confirm/restore to a named capture for this MAC.
pub fn require_capture_backup(
    layout: &Layout,
    live: &LiveIdentity,
    slug: &str,
) -> Result<OriginalBackup, Error> {
    crate::identity::validate_factory_serial(slug)?;
    let mut matches: Vec<_> = list_captures(layout)?
        .into_iter()
        .filter(|snap| {
            snap.dir.file_name().and_then(|name| name.to_str()) == Some(slug)
                && snap.manifest.mac == live.mac
        })
        .collect();
    match matches.len() {
        0 => Err(Error::MissingCapture(slug.to_string())),
        1 => {
            let found = matches.pop().unwrap();
            bind_check(&found.manifest.identity(), live)?;
            Ok(found)
        }
        _ => Err(Error::AmbiguousCapture),
    }
}

fn snapshots_for_mac(listed: &[OriginalBackup], live: &LiveIdentity) -> Vec<OriginalBackup> {
    listed
        .iter()
        .filter(|snap| snap.manifest.mac == live.mac)
        .cloned()
        .collect()
}

/// MAC must match. USB serial must match when both sides have one.
pub fn bind_check(original: &LiveIdentity, live: &LiveIdentity) -> Result<(), Error> {
    if original.mac != live.mac {
        return Err(Error::IdentityMismatch {
            reason: "MAC does not match the original MANIFEST".into(),
        });
    }
    if let (Some(expected), Some(got)) = (&original.usb_serial, &live.usb_serial) {
        if expected != got {
            return Err(Error::IdentityMismatch {
                reason: "USB serial does not match the original MANIFEST".into(),
            });
        }
    }
    Ok(())
}

/// Readable `original/{serial}/` trees (skips dirs without a MANIFEST).
pub fn list_originals(layout: &Layout) -> Result<Vec<OriginalBackup>, Error> {
    list_snapshot_dirs(&layout.originals_dir())
}

/// Readable `captures/{unit-id}/{slug}/` trees.
pub fn list_captures(layout: &Layout) -> Result<Vec<OriginalBackup>, Error> {
    let root = layout.captures_dir();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for unit in fs::read_dir(&root)? {
        let unit = unit?;
        if !is_usable_dir(&unit.path()) {
            continue;
        }
        out.extend(list_snapshot_dirs(&unit.path())?);
    }
    Ok(out)
}

fn list_snapshot_dirs(dir: &Path) -> Result<Vec<OriginalBackup>, Error> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !is_usable_dir(&path) {
            continue;
        }
        let Ok(manifest) = load_manifest(&path) else {
            continue;
        };
        out.push(OriginalBackup {
            dir: path,
            manifest,
        });
    }
    Ok(out)
}

/// Bind a UART to an original without a DTR `board-info` reset.
///
/// Prefers the CH343 USB serial from a QinHeng by-id `port`. If the port has
/// no USB serial and exactly one original exists, uses that unit.
pub fn require_original_from_port(layout: &Layout, port: &str) -> Result<OriginalBackup, Error> {
    unique_from_port(&list_originals(layout)?, port, Error::AmbiguousOriginal)
}

/// Bind a UART to the safety net (original preferred, else unique capture).
pub fn require_safety_net_from_port(layout: &Layout, port: &str) -> Result<SafetyNet, Error> {
    let originals = list_originals(layout)?;
    if let Ok(snapshot) = unique_from_port(&originals, port, Error::AmbiguousOriginal) {
        return Ok(SafetyNet {
            is_original: true,
            snapshot,
        });
    }
    if originals.is_empty() {
        let captures = list_captures(layout)?;
        let snapshot = unique_from_port(&captures, port, Error::AmbiguousCapture)?;
        return Ok(SafetyNet {
            is_original: false,
            snapshot,
        });
    }
    match unique_from_port(&originals, port, Error::AmbiguousOriginal) {
        Ok(snapshot) => Ok(SafetyNet {
            is_original: true,
            snapshot,
        }),
        Err(Error::MissingOriginal) => {
            let captures = list_captures(layout)?;
            let snapshot = unique_from_port(&captures, port, Error::AmbiguousCapture)?;
            Ok(SafetyNet {
                is_original: false,
                snapshot,
            })
        }
        Err(error) => Err(error),
    }
}

fn unique_from_port(
    listed: &[OriginalBackup],
    port: &str,
    ambiguous: Error,
) -> Result<OriginalBackup, Error> {
    if listed.is_empty() {
        return Err(Error::MissingOriginal);
    }
    if let Some(usb) = parse_usb_serial_from_port(port) {
        let matches: Vec<_> = listed
            .iter()
            .filter(|snap| snap.manifest.usb_serial.as_deref() == Some(usb.as_str()))
            .cloned()
            .collect();
        return unique_snapshot(matches, ambiguous);
    }
    if listed.len() == 1 {
        return Ok(listed[0].clone());
    }
    Err(ambiguous)
}

fn unique_snapshot(
    mut matches: Vec<OriginalBackup>,
    ambiguous: Error,
) -> Result<OriginalBackup, Error> {
    match matches.len() {
        0 => Err(Error::MissingOriginal),
        1 => Ok(matches.pop().unwrap()),
        _ => Err(ambiguous),
    }
}

/// Refuse if the final original directory already exists.
pub fn refuse_if_original_exists(layout: &Layout, factory_serial: &str) -> Result<(), Error> {
    let dest = layout.original_dir(factory_serial);
    if dest.exists() {
        return Err(Error::OriginalExists(dest));
    }
    Ok(())
}

/// Refuse if the final capture directory already exists.
pub fn refuse_if_capture_exists(layout: &Layout, unit_id: &str, slug: &str) -> Result<(), Error> {
    let dest = layout.capture_dir(unit_id, slug);
    if dest.exists() {
        return Err(Error::CaptureExists(dest));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::test_mac;
    use crate::manifest::{PartitionHash, SnapshotKind, MANIFEST_SCHEMA};

    fn layout_tmp() -> (tempfile::TempDir, Layout) {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        (tmp, layout)
    }

    fn stub_manifest(mac: String, usb: Option<String>) -> Manifest {
        Manifest {
            schema: MANIFEST_SCHEMA.into(),
            kind: SnapshotKind::Original,
            layout_id: None,
            classification: None,
            image_name: None,
            factory_serial: "TESTFACTORY001".into(),
            usb_serial: usb,
            mac,
            flash_size: "32MB".into(),
            secure_boot: false,
            flash_encryption: false,
            dump_sha256: "00".into(),
            bootloader_sha256: "00".into(),
            partition_table_sha256: "00".into(),
            boot_slot: Some("app0".into()),
            app0_desc: None,
            partitions: vec![],
            partition_sha256: vec![PartitionHash {
                label: "nvs".into(),
                sha256: "00".into(),
            }],
        }
    }

    fn write_original(layout: &Layout, manifest: &Manifest) {
        let dir = layout.original_dir(&manifest.factory_serial);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("MANIFEST.json"),
            serde_json::to_vec_pretty(manifest).unwrap(),
        )
        .unwrap();
    }

    fn write_capture(layout: &Layout, unit_id: &str, slug: &str, manifest: &Manifest) {
        let dir = layout.capture_dir(unit_id, slug);
        fs::create_dir_all(&dir).unwrap();
        let yaml = noyalib::to_string(manifest).unwrap();
        fs::write(dir.join("MANIFEST.yaml"), yaml).unwrap();
    }

    #[test]
    fn missing_original_refuses() {
        let (_tmp, layout) = layout_tmp();
        let live = LiveIdentity {
            mac: test_mac(),
            usb_serial: None,
        };
        assert!(matches!(
            require_original_backup(&layout, &live),
            Err(Error::MissingOriginal)
        ));
    }

    #[test]
    fn json_manifest_still_loads() {
        let (_tmp, layout) = layout_tmp();
        let mac = test_mac();
        write_original(&layout, &stub_manifest(mac.clone(), None));
        let dir = layout.original_dir("TESTFACTORY001");
        let loaded = load_manifest(&dir).unwrap();
        assert_eq!(loaded.factory_serial, "TESTFACTORY001");
        assert_eq!(loaded.kind, SnapshotKind::Original);
        assert_eq!(loaded.schema, MANIFEST_SCHEMA);
        let live = LiveIdentity {
            mac,
            usb_serial: None,
        };
        assert_eq!(
            require_original_backup(&layout, &live)
                .unwrap()
                .manifest
                .factory_serial,
            "TESTFACTORY001"
        );
    }

    #[test]
    fn yaml_manifest_preferred() {
        let (_tmp, layout) = layout_tmp();
        let mac = test_mac();
        let dir = layout.original_dir("TESTFACTORY001");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("MANIFEST.json"),
            "{\"factory_serial\":\"FROMJSON\"}",
        )
        .unwrap();
        let mut manifest = stub_manifest(mac, None);
        manifest.factory_serial = "TESTFACTORY001".into();
        fs::write(
            dir.join("MANIFEST.yaml"),
            noyalib::to_string(&manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(
            load_manifest(&dir).unwrap().factory_serial,
            "TESTFACTORY001"
        );
    }

    #[test]
    fn mac_mismatch_refuses() {
        let (_tmp, layout) = layout_tmp();
        let mac = test_mac();
        write_original(&layout, &stub_manifest(mac.clone(), None));
        let other = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":");
        let live = LiveIdentity {
            mac: other,
            usb_serial: None,
        };
        assert!(matches!(
            require_original_backup(&layout, &live),
            Err(Error::MissingOriginal)
        ));
    }

    #[test]
    fn usb_mismatch_refuses_when_both_present() {
        let mac = test_mac();
        let original = LiveIdentity {
            mac: mac.clone(),
            usb_serial: Some("AAA".into()),
        };
        let live = LiveIdentity {
            mac,
            usb_serial: Some("BBB".into()),
        };
        assert!(matches!(
            bind_check(&original, &live),
            Err(Error::IdentityMismatch { .. })
        ));
    }

    #[test]
    fn matching_mac_binds() {
        let (_tmp, layout) = layout_tmp();
        let mac = test_mac();
        write_original(&layout, &stub_manifest(mac.clone(), Some("USB1".into())));
        let live = LiveIdentity {
            mac,
            usb_serial: Some("USB1".into()),
        };
        let found = require_original_backup(&layout, &live).unwrap();
        assert_eq!(found.manifest.factory_serial, "TESTFACTORY001");
    }

    #[test]
    fn existing_dir_is_write_once() {
        let (_tmp, layout) = layout_tmp();
        fs::create_dir_all(layout.original_dir("TESTFACTORY001")).unwrap();
        assert!(matches!(
            refuse_if_original_exists(&layout, "TESTFACTORY001"),
            Err(Error::OriginalExists(_))
        ));
    }

    #[test]
    fn port_by_id_binds_usb_serial() {
        let (_tmp, layout) = layout_tmp();
        let mac = test_mac();
        write_original(&layout, &stub_manifest(mac, Some("TESTUSB".into())));
        let port = "/dev/serial/by-id/usb-1a86_USB_Single_Serial_TESTUSB-if00";
        let found = require_original_from_port(&layout, port).unwrap();
        assert_eq!(found.manifest.factory_serial, "TESTFACTORY001");
    }

    #[test]
    fn unique_original_binds_without_usb_serial_in_port() {
        let (_tmp, layout) = layout_tmp();
        write_original(&layout, &stub_manifest(test_mac(), None));
        let found = require_original_from_port(&layout, "/dev/ttyACM0").unwrap();
        assert_eq!(found.manifest.factory_serial, "TESTFACTORY001");
    }

    #[test]
    fn two_originals_need_by_id_port() {
        let (_tmp, layout) = layout_tmp();
        let mut a = stub_manifest(test_mac(), Some("USBAxx".into()));
        a.factory_serial = "TESTFACTORY001".into();
        write_original(&layout, &a);
        let mut b = stub_manifest(
            [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(":"),
            Some("USBBxx".into()),
        );
        b.factory_serial = "TESTFACTORY002".into();
        write_original(&layout, &b);
        assert!(matches!(
            require_original_from_port(&layout, "/dev/ttyACM0"),
            Err(Error::AmbiguousOriginal)
        ));
        let port = "/dev/serial/by-id/usb-1a86_USB_Single_Serial_USBBxx-if00";
        let found = require_original_from_port(&layout, port).unwrap();
        assert_eq!(found.manifest.factory_serial, "TESTFACTORY002");
    }

    #[test]
    fn safety_net_uses_unique_capture() {
        let (_tmp, layout) = layout_tmp();
        let mac = test_mac();
        let mut manifest = stub_manifest(mac.clone(), None);
        manifest.kind = SnapshotKind::Capture;
        manifest.image_name = Some("after-flash".into());
        write_capture(&layout, "TESTFACTORY001", "after-flash", &manifest);
        let live = LiveIdentity {
            mac,
            usb_serial: None,
        };
        assert!(matches!(
            require_original_backup(&layout, &live),
            Err(Error::MissingOriginal)
        ));
        let net = require_safety_net(&layout, &live).unwrap();
        assert!(!net.is_original);
        assert_eq!(
            net.snapshot.manifest.image_name.as_deref(),
            Some("after-flash")
        );
        let found = require_capture_backup(&layout, &live, "after-flash").unwrap();
        assert_eq!(found.dir, net.snapshot.dir);
    }

    #[test]
    fn safety_net_prefers_original() {
        let (_tmp, layout) = layout_tmp();
        let mac = test_mac();
        write_original(&layout, &stub_manifest(mac.clone(), None));
        let mut capture = stub_manifest(mac.clone(), None);
        capture.kind = SnapshotKind::Capture;
        write_capture(&layout, "TESTFACTORY001", "later", &capture);
        let live = LiveIdentity {
            mac,
            usb_serial: None,
        };
        let net = require_safety_net(&layout, &live).unwrap();
        assert!(net.is_original);
    }
}
