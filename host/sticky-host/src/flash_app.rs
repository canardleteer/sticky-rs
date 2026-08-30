//! Write a custom application image into factory `app0` only.

use std::fs;
use std::path::Path;

use crate::device::DeviceIo;
use crate::original::{require_original_backup, Layout};
use crate::Error;

/// Factory `app0` starts here. Nothing below this is an app-flash target.
pub const APP0_MIN_OFFSET: u32 = 0x90000;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// `write_bin` of `image` at this unit's `app0` offset. Never erase, never
/// the `espflash` `flash` subcommand, never a caller-chosen address.
pub fn flash_app<D: DeviceIo>(
    device: &D,
    layout: &Layout,
    port: &str,
    image: &Path,
    yes: bool,
) -> Result<(), Error> {
    if !yes {
        return Err(Error::FlashNotConfirmed);
    }
    let (_, board) = crate::detect::read_live_board(device, port)?;
    let original = require_original_backup(layout, &board.identity)?;
    let app0 = original
        .manifest
        .partitions
        .iter()
        .find(|part| part.label == "app0")
        .ok_or_else(|| Error::UnknownPartition("app0".into()))?;
    if app0.offset < APP0_MIN_OFFSET {
        return Err(Error::UnsafeAppOffset(app0.offset));
    }
    let bytes = fs::read(image)?;
    validate_app_image(&bytes, app0.size)?;
    device.write_bin(port, app0.offset, image)
}

fn validate_app_image(bytes: &[u8], app0_size: u32) -> Result<(), Error> {
    if bytes.is_empty() || bytes.starts_with(&ELF_MAGIC) {
        return Err(Error::ImageNotApp);
    }
    let size = bytes.len() as u64;
    if size > u64::from(app0_size) {
        return Err(Error::ImageTooLarge {
            size,
            max: app0_size,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::persist_original;
    use crate::device::MockDevice;
    use crate::identity::{parse_board_info, test_mac};
    use crate::partitions::{test_entry, PARTITION_TABLE_OFFSET};
    use std::cell::RefCell;
    use std::fs;

    const APP0_SIZE: u32 = 256;

    fn dump_with_app0() -> Vec<u8> {
        let end = APP0_MIN_OFFSET as usize + APP0_SIZE as usize;
        let mut dump = vec![0u8; end];
        dump[PARTITION_TABLE_OFFSET..PARTITION_TABLE_OFFSET + 32]
            .copy_from_slice(&test_entry("nvs", 0x01, 0x02, 0x9000, 16));
        dump[PARTITION_TABLE_OFFSET + 32..PARTITION_TABLE_OFFSET + 64]
            .copy_from_slice(&test_entry("app0", 0x00, 0x10, APP0_MIN_OFFSET, APP0_SIZE));
        dump[APP0_MIN_OFFSET as usize] = 0xE9;
        dump
    }

    fn persist(layout: &Layout, info: &str) {
        persist_original(
            layout,
            "TESTFACTORY001",
            &dump_with_app0(),
            &parse_board_info(info).unwrap(),
            "",
            info,
            false,
        )
        .unwrap();
    }

    fn info() -> String {
        let mac = test_mac();
        format!(
            "Flash size: 32MB\nMAC address: {mac}\nSecure Boot: Disabled\nFlash Encryption: Disabled\n"
        )
    }

    fn payload(dir: &Path, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join("app.bin");
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn validate_rejects_elf_and_empty() {
        assert!(matches!(
            validate_app_image(&[], APP0_SIZE),
            Err(Error::ImageNotApp)
        ));
        let mut elf = ELF_MAGIC.to_vec();
        elf.extend_from_slice(&[0u8; 16]);
        assert!(matches!(
            validate_app_image(&elf, APP0_SIZE),
            Err(Error::ImageNotApp)
        ));
    }

    #[test]
    fn validate_rejects_oversized() {
        let bytes = vec![0xE9; APP0_SIZE as usize + 1];
        assert!(matches!(
            validate_app_image(&bytes, APP0_SIZE),
            Err(Error::ImageTooLarge { size, max }) if size == u64::from(APP0_SIZE) + 1 && max == APP0_SIZE
        ));
    }

    #[test]
    fn flash_without_yes_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        let mock = RefCell::new(MockDevice::default());
        let image = payload(tmp.path(), &[0xE9, 0x01]);
        let err = flash_app(&mock, &layout, "PORT", &image, false).unwrap_err();
        assert!(matches!(err, Error::FlashNotConfirmed));
    }

    #[test]
    fn flash_refuses_without_original() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        let board = info();
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let image = payload(tmp.path(), &[0xE9, 0x01]);
        let err = flash_app(&mock, &layout, "PORT", &image, true).unwrap_err();
        assert!(matches!(err, Error::MissingOriginal));
    }

    #[test]
    fn flash_refuses_identity_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        let board = info();
        persist(&layout, &board);
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
        let image = payload(tmp.path(), &[0xE9, 0x01]);
        assert!(matches!(
            flash_app(&mock, &layout, "PORT", &image, true),
            Err(Error::MissingOriginal)
        ));
    }

    #[test]
    fn flash_writes_app0_only() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        let board = info();
        persist(&layout, &board);
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let bytes = vec![0xE9, 0x03, 0x02, 0x01];
        let image = payload(tmp.path(), &bytes);
        flash_app(&mock, &layout, "PORT", &image, true).unwrap();
        let writes = &mock.borrow().writes;
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, APP0_MIN_OFFSET);
        assert_eq!(writes[0].1, bytes);
    }

    #[test]
    fn flash_refuses_elf_file() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        let board = info();
        persist(&layout, &board);
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let mut elf = ELF_MAGIC.to_vec();
        elf.extend_from_slice(&[1, 2, 3, 4]);
        let image = payload(tmp.path(), &elf);
        assert!(matches!(
            flash_app(&mock, &layout, "PORT", &image, true),
            Err(Error::ImageNotApp)
        ));
        assert!(mock.borrow().writes.is_empty());
    }

    #[test]
    fn flash_refuses_when_table_has_no_app0() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        let board = info();
        let nvs_off = 0x9000u32;
        let mut dump = vec![0u8; (nvs_off + 16) as usize];
        dump[PARTITION_TABLE_OFFSET..PARTITION_TABLE_OFFSET + 32]
            .copy_from_slice(&test_entry("nvs", 0x01, 0x02, nvs_off, 16));
        persist_original(
            &layout,
            "TESTFACTORY001",
            &dump,
            &parse_board_info(&board).unwrap(),
            "",
            &board,
            false,
        )
        .unwrap();
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let image = payload(tmp.path(), &[0xE9, 0x01]);
        assert!(matches!(
            flash_app(&mock, &layout, "PORT", &image, true),
            Err(Error::UnknownPartition(label)) if label == "app0"
        ));
    }

    #[test]
    fn flash_refuses_empty_and_oversized_files() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        let board = info();
        persist(&layout, &board);
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let empty = payload(tmp.path(), &[]);
        assert!(matches!(
            flash_app(&mock, &layout, "PORT", &empty, true),
            Err(Error::ImageNotApp)
        ));
        let huge = payload(tmp.path(), &vec![0xE9; APP0_SIZE as usize + 1]);
        assert!(matches!(
            flash_app(&mock, &layout, "PORT", &huge, true),
            Err(Error::ImageTooLarge { size, max }) if size == u64::from(APP0_SIZE) + 1 && max == APP0_SIZE
        ));
        assert!(mock.borrow().writes.is_empty());
    }

    #[test]
    fn flash_refuses_usb_serial_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        let board_text = info();
        let mut board = parse_board_info(&board_text).unwrap();
        board.identity.usb_serial = Some("AAA".into());
        persist_original(
            &layout,
            "TESTFACTORY001",
            &dump_with_app0(),
            &board,
            "",
            &board_text,
            false,
        )
        .unwrap();
        let port = format!("prefix/usb-1a86_{}BBB-if00", "USB_Single_Serial-");
        let mock = RefCell::new(MockDevice {
            board_info: board_text,
            ..MockDevice::default()
        });
        let image = payload(tmp.path(), &[0xE9, 0x01]);
        assert!(matches!(
            flash_app(&mock, &layout, &port, &image, true),
            Err(Error::IdentityMismatch { .. })
        ));
        assert!(mock.borrow().writes.is_empty());
    }

    #[test]
    fn flash_refuses_app0_below_min_offset() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        let board = info();
        let unsafe_off = 0x88000u32;
        let mut dump = vec![0u8; unsafe_off as usize + 16];
        dump[PARTITION_TABLE_OFFSET..PARTITION_TABLE_OFFSET + 32]
            .copy_from_slice(&test_entry("nvs", 0x01, 0x02, 0x9000, 16));
        dump[PARTITION_TABLE_OFFSET + 32..PARTITION_TABLE_OFFSET + 64]
            .copy_from_slice(&test_entry("app0", 0x00, 0x10, unsafe_off, 16));
        persist_original(
            &layout,
            "TESTFACTORY001",
            &dump,
            &parse_board_info(&board).unwrap(),
            "",
            &board,
            false,
        )
        .unwrap();
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let image = payload(tmp.path(), &[0xE9, 0x01]);
        assert!(matches!(
            flash_app(&mock, &layout, "PORT", &image, true),
            Err(Error::UnsafeAppOffset(offset)) if offset == unsafe_off
        ));
        assert!(mock.borrow().writes.is_empty());
    }
}
