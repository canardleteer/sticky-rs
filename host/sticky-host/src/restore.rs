//! Restore that unit's original via `write-bin` only.

use std::path::{Path, PathBuf};

use crate::device::DeviceIo;
use crate::identity::LiveIdentity;
use crate::original::{require_capture_backup, require_original_backup, Layout};
use crate::Error;

/// Restore the full 32 MiB image or one named partition.
pub fn restore<D: DeviceIo>(
    device: &D,
    layout: &Layout,
    port: &str,
    yes: bool,
    part: Option<&str>,
    capture: Option<&str>,
) -> Result<(), Error> {
    if !yes {
        return Err(Error::RestoreNotConfirmed);
    }
    let live = live_identity(device, port)?;
    let original = if let Some(slug) = capture {
        require_capture_backup(layout, &live, slug)?
    } else {
        require_original_backup(layout, &live)?
    };
    match part {
        None => {
            let image = original.dir.join("flash-32mb.bin");
            if !image.is_file() {
                return Err(Error::Import("original missing flash-32mb.bin".into()));
            }
            let bytes = std::fs::metadata(&image)?.len();
            eprintln!("restore: writing flash-32mb.bin ({bytes} bytes) at 0x0 in 1 MiB windows");
            device.write_bin(port, 0, &image)
        }
        Some(label) => {
            let part = original
                .manifest
                .partitions
                .iter()
                .find(|p| p.label == label)
                .ok_or_else(|| Error::UnknownPartition(label.into()))?;
            let image = original.dir.join(format!("part-{label}.bin"));
            if !image.is_file() {
                return Err(Error::UnknownPartition(label.into()));
            }
            let bytes = std::fs::metadata(&image)?.len();
            eprintln!(
                "restore: writing part-{label}.bin ({bytes} bytes) at {:#x}",
                part.offset
            );
            device.write_bin(port, part.offset, &image)
        }
    }
}

fn live_identity<D: DeviceIo>(device: &D, port: &str) -> Result<LiveIdentity, Error> {
    let (_, board) = crate::detect::read_live_board(device, port)?;
    Ok(board.identity)
}

/// Used by tests that restore against a mock without a 32 MiB file.
pub fn restore_paths(original_dir: &Path, part: Option<&str>) -> Result<(u32, PathBuf), Error> {
    match part {
        None => Ok((0, original_dir.join("flash-32mb.bin"))),
        Some(label) => {
            let manifest = crate::original::load_manifest(original_dir)?;
            let part = manifest
                .partitions
                .iter()
                .find(|p| p.label == label)
                .ok_or_else(|| Error::UnknownPartition(label.into()))?;
            Ok((part.offset, original_dir.join(format!("part-{label}.bin"))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::persist_original;
    use crate::device::MockDevice;
    use crate::identity::{parse_board_info, test_mac};
    use crate::partitions::{test_entry, PARTITION_TABLE_OFFSET};
    use std::cell::RefCell;

    fn tiny_dump() -> Vec<u8> {
        let nvs_off = 0x9000u32;
        let mut dump = vec![0u8; (nvs_off + 16) as usize];
        dump[PARTITION_TABLE_OFFSET..PARTITION_TABLE_OFFSET + 32]
            .copy_from_slice(&test_entry("nvs", 0x01, 0x02, nvs_off, 16));
        dump[nvs_off as usize] = 0xEE;
        dump
    }

    #[test]
    fn restore_without_yes_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::from_developer_data_root(tmp.path());
        let mock = RefCell::new(MockDevice::default());
        let err = restore(&mock, &layout, "PORT", false, None, None).unwrap_err();
        assert!(matches!(err, Error::RestoreNotConfirmed));
    }

    #[test]
    fn restore_part_nvs_records_write() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::from_developer_data_root(tmp.path());
        let mac = test_mac();
        let info = format!(
            "Flash size: 32MB\nMAC address: {mac}\nSecure Boot: Disabled\nFlash Encryption: Disabled\n"
        );
        persist_original(
            &layout,
            "TESTFACTORY001",
            &tiny_dump(),
            &parse_board_info(&info).unwrap(),
            "",
            &info,
            false,
        )
        .unwrap();
        let mock = RefCell::new(MockDevice {
            board_info: info,
            ..MockDevice::default()
        });
        restore(&mock, &layout, "PORT", true, Some("nvs"), None).unwrap();
        let writes = &mock.borrow().writes;
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, 0x9000);
        assert_eq!(writes[0].1[0], 0xEE);
    }

    #[test]
    fn restore_paths_unknown_part() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::from_developer_data_root(tmp.path());
        let mac = test_mac();
        let info = format!("Flash size: 32MB\nMAC address: {mac}\n");
        let dest = persist_original(
            &layout,
            "TESTFACTORY001",
            &tiny_dump(),
            &parse_board_info(&info).unwrap(),
            "",
            &info,
            false,
        )
        .unwrap();
        assert!(matches!(
            restore_paths(&dest, Some("nope")),
            Err(Error::UnknownPartition(_))
        ));
    }

    #[test]
    fn restore_refuses_unknown_mac() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::from_developer_data_root(tmp.path());
        let mac = test_mac();
        let info = format!(
            "Flash size: 32MB\nMAC address: {mac}\nSecure Boot: Disabled\nFlash Encryption: Disabled\n"
        );
        persist_original(
            &layout,
            "TESTFACTORY001",
            &tiny_dump(),
            &parse_board_info(&info).unwrap(),
            "",
            &info,
            false,
        )
        .unwrap();
        let other = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":");
        let live = format!(
            "Flash size: 32MB\nMAC address: {other}\nSecure Boot: Disabled\nFlash Encryption: Disabled\n"
        );
        let mock = RefCell::new(MockDevice {
            board_info: live,
            ..MockDevice::default()
        });
        assert!(matches!(
            restore(&mock, &layout, "PORT", true, Some("nvs"), None),
            Err(Error::MissingOriginal)
        ));
    }

    #[test]
    fn restore_refuses_usb_serial_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::from_developer_data_root(tmp.path());
        let mac = test_mac();
        let info = format!(
            "Flash size: 32MB\nMAC address: {mac}\nSecure Boot: Disabled\nFlash Encryption: Disabled\n"
        );
        let mut board = parse_board_info(&info).unwrap();
        board.identity.usb_serial = Some("AAA".into());
        persist_original(
            &layout,
            "TESTFACTORY001",
            &tiny_dump(),
            &board,
            "",
            &info,
            false,
        )
        .unwrap();
        let port = format!("prefix/usb-1a86_{}BBB-if00", "USB_Single_Serial-");
        let mock = RefCell::new(MockDevice {
            board_info: info,
            ..MockDevice::default()
        });
        assert!(matches!(
            restore(&mock, &layout, &port, true, Some("nvs"), None),
            Err(Error::IdentityMismatch { .. })
        ));
    }
}
