//! Write-once originals under `backups/original/{factory-serial}/`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::identity::{parse_usb_serial_from_port, LiveIdentity};
use crate::manifest::Manifest;
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

    /// `backups/original/{factory-serial}/learn-uart/` (session YAML; not the dump).
    #[must_use]
    pub fn learn_uart_dir(&self, factory_serial: &str) -> PathBuf {
        self.original_dir(factory_serial).join("learn-uart")
    }
}

/// An on-disk original plus its MANIFEST.
#[derive(Debug, Clone)]
pub struct OriginalBackup {
    /// Directory path.
    pub dir: PathBuf,
    /// Parsed MANIFEST.
    pub manifest: Manifest,
}

/// Load MANIFEST.json from an original directory.
pub fn load_manifest(dir: &Path) -> Result<Manifest, Error> {
    let text = fs::read_to_string(dir.join("MANIFEST.json"))?;
    Ok(serde_json::from_str(&text)?)
}

/// flash-app, restore, and confirm: refuse unless a matching original exists.
pub fn require_original_backup(
    layout: &Layout,
    live: &LiveIdentity,
) -> Result<OriginalBackup, Error> {
    let originals = layout.originals_dir();
    if !originals.is_dir() {
        return Err(Error::MissingOriginal);
    }

    let mut matches = Vec::new();
    for entry in fs::read_dir(&originals)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let dir = entry.path();
        let Ok(manifest) = load_manifest(&dir) else {
            continue;
        };
        if manifest.mac == live.mac {
            matches.push(OriginalBackup { dir, manifest });
        }
    }

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

/// Readable `original/{serial}/` trees (skips dirs without `MANIFEST.json`).
pub fn list_originals(layout: &Layout) -> Result<Vec<OriginalBackup>, Error> {
    let originals = layout.originals_dir();
    if !originals.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&originals)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let dir = entry.path();
        let Ok(manifest) = load_manifest(&dir) else {
            continue;
        };
        out.push(OriginalBackup { dir, manifest });
    }
    Ok(out)
}

/// Bind a UART to an original without a DTR `board-info` reset.
///
/// Prefers the CH343 USB serial from a QinHeng by-id `port`. If the port has
/// no USB serial and exactly one original exists, uses that unit.
pub fn require_original_from_port(layout: &Layout, port: &str) -> Result<OriginalBackup, Error> {
    let listed = list_originals(layout)?;
    if listed.is_empty() {
        return Err(Error::MissingOriginal);
    }
    if let Some(usb) = parse_usb_serial_from_port(port) {
        let matches: Vec<_> = listed
            .into_iter()
            .filter(|original| original.manifest.usb_serial.as_deref() == Some(usb.as_str()))
            .collect();
        return unique_original(matches);
    }
    if listed.len() == 1 {
        return Ok(listed.into_iter().next().unwrap());
    }
    Err(Error::AmbiguousOriginal)
}

fn unique_original(mut matches: Vec<OriginalBackup>) -> Result<OriginalBackup, Error> {
    match matches.len() {
        0 => Err(Error::MissingOriginal),
        1 => Ok(matches.pop().unwrap()),
        _ => Err(Error::AmbiguousOriginal),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::test_mac;
    use crate::manifest::PartitionHash;

    fn layout_tmp() -> (tempfile::TempDir, Layout) {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        (tmp, layout)
    }

    fn stub_manifest(mac: String, usb: Option<String>) -> Manifest {
        Manifest {
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
}
