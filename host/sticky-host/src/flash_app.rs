//! Write a custom application image into factory `app0` only.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::device::DeviceIo;
use crate::original::{require_capture_backup, require_safety_net, Layout};
use crate::partition_layouts::match_layout;
use crate::Error;

/// Factory `app0` starts here. Nothing below this is an app-flash target.
pub const APP0_MIN_OFFSET: u32 = 0x90000;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// Which backup prerequisite applies to an application-only flash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackupPolicy {
    /// Require a matching original or capture stored locally.
    #[default]
    RequireLocalSnapshot,
    /// The operator has a backup elsewhere; validate the live table instead.
    ExternallyBackedUp,
}

/// Application flash policy; these options never authorize a full erase.
#[derive(Clone, Debug, Default)]
pub struct FlashAppOptions {
    /// Explicit confirmation that application flash may be written.
    pub yes: bool,
    /// Backup prerequisite, defaulting to a bound local snapshot.
    pub backup_policy: BackupPolicy,
    /// Permit an unknown snapshot table; unavailable with external-backup mode.
    pub allow_unknown_layout: bool,
    /// Named local capture; unavailable with external-backup mode.
    pub capture: Option<String>,
}

/// Flash with explicit backup policy, retaining all app0 address boundaries.
///
/// External-backup mode reads the live table and verifies its checksum and
/// geometry against the factory catalog. It does not persist a flash dump.
pub fn flash_app_with_options<D: DeviceIo>(
    device: &D,
    layout: &Layout,
    port: &str,
    image: &Path,
    options: &FlashAppOptions,
) -> Result<(), Error> {
    if options.backup_policy == BackupPolicy::RequireLocalSnapshot {
        return flash_app(
            device,
            layout,
            port,
            image,
            options.yes,
            options.allow_unknown_layout,
            options.capture.as_deref(),
        );
    }
    if !options.yes {
        return Err(Error::FlashNotConfirmed);
    }
    if options.capture.is_some() || options.allow_unknown_layout {
        return Err(Error::Device(
            "external-backup mode requires the known live factory layout".into(),
        ));
    }
    let (_, board) = crate::detect::read_live_board(device, port)?;
    if board.secure_boot != Some(false) || board.flash_encryption != Some(false) {
        return Err(Error::Device(
            "force flash requires explicitly disabled secure boot and flash encryption".into(),
        ));
    }
    let table = device.read_flash(
        port,
        crate::partitions::PARTITION_TABLE_OFFSET as u32,
        crate::partitions::PARTITION_TABLE_LEN as u32,
    )?;
    // Inspection accepts table prefixes; writing requires the complete table
    // and the upstream parser's checksum and overlap validation.
    esp_idf_part::PartitionTable::try_from_bytes(table.clone())
        .map_err(|error| Error::PartitionTable(error.to_string()))?;
    let parts = crate::partitions::parse_partition_table(&table)?;
    let checksum_offset = parts.len() * 32;
    if table.get(checksum_offset..checksum_offset + 2) != Some(&[0xeb, 0xeb]) {
        return Err(Error::PartitionTable("live table has no MD5 record".into()));
    }
    let status = match_layout(&parts);
    if !status.is_known() || parts.iter().any(|part| part.flags != 0) {
        return Err(Error::UnsafePartitionLayout {
            status: status.evidence_token(),
        });
    }
    let app0 = parts
        .iter()
        .find(|part| part.label == "app0")
        .ok_or_else(|| Error::UnknownPartition("app0".into()))?;
    if app0.offset < APP0_MIN_OFFSET {
        return Err(Error::UnsafeAppOffset(app0.offset));
    }
    let bytes = read_app_image(image, app0.size)?;
    validate_app_image(&bytes, app0.size)?;
    validate_esp32s3_image(&bytes)?;
    // Write the bytes we actually validated, even if the source path changes
    // while live discovery/table validation or flash-window retries run.
    let mut validated = tempfile::NamedTempFile::new()?;
    validated.write_all(&bytes)?;
    validated.flush()?;
    device.write_bin(port, app0.offset, validated.path())
}

/// Read at most the partition size plus one byte, rejecting a larger source
/// before allocating. A concurrent file growth is also bounded and rejected.
/// Returns an I/O error or [`Error::ImageTooLarge`] without device writes.
pub fn read_app_image(image: &Path, maximum: u32) -> Result<Vec<u8>, Error> {
    let source = fs::File::open(image)?;
    let size = source.metadata()?.len();
    if size > u64::from(maximum) {
        return Err(Error::ImageTooLarge { size, max: maximum });
    }
    let mut bytes = Vec::with_capacity(size as usize);
    source
        .take(u64::from(maximum) + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum as usize {
        return Err(Error::ImageTooLarge {
            size: bytes.len() as u64,
            max: maximum,
        });
    }
    Ok(bytes)
}

/// Check an ESP32-S3 app header, bounded segments, XOR checksum, and optional
/// image SHA-256 before externally backed-up app0 flashing.
/// Layout follows espflash 4.5's `ImageHeader` and `save_segment` implementation,
/// grounded in esptool's "Firmware Image Format" file/extended-header sections.
/// Returns [`Error::ImageNotApp`] for malformed, truncated, or wrong-target data.
pub fn validate_esp32s3_image(bytes: &[u8]) -> Result<(), Error> {
    use sha2::{Digest, Sha256};
    let invalid = || Error::ImageNotApp;
    if bytes.len() < 24
        || bytes[0] != ESP_IMAGE_MAGIC
        || !(1..=16).contains(&bytes[1])
        || u16::from_le_bytes([bytes[12], bytes[13]]) != espflash::target::Chip::Esp32s3.id()
        || bytes[23] > 1
    {
        return Err(invalid());
    }
    let mut cursor = 24usize;
    let mut checksum = 0xef;
    for _ in 0..bytes[1] {
        let header = bytes
            .get(cursor..cursor.checked_add(8).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        let size = u32::from_le_bytes(header[4..].try_into().map_err(|_| invalid())?) as usize;
        cursor = cursor.checked_add(8).ok_or_else(invalid)?;
        let end = cursor.checked_add(size).ok_or_else(invalid)?;
        let data = bytes.get(cursor..end).ok_or_else(invalid)?;
        for byte in data {
            checksum ^= byte;
        }
        cursor = end;
    }
    let checksum_offset = cursor.checked_add(15 - cursor % 16).ok_or_else(invalid)?;
    if bytes.get(checksum_offset) != Some(&checksum)
        || bytes
            .get(cursor..checksum_offset)
            .is_none_or(|padding| padding.iter().any(|b| *b != 0))
    {
        return Err(invalid());
    }
    let end = checksum_offset + 1;
    if bytes[23] == 1 {
        let digest: [u8; 32] = Sha256::digest(&bytes[..end]).into();
        if bytes.get(end..) != Some(digest.as_slice()) {
            return Err(invalid());
        }
    } else if bytes.len() != end {
        return Err(invalid());
    }
    Ok(())
}

/// First byte of an Espressif application image header (`esptool` / IDF
/// `ESP_IMAGE_HEADER_MAGIC`). `validate_app_image` names this so a
/// comment can cite it; it does not require the byte (empty and ELF
/// are the refusals).
pub const ESP_IMAGE_MAGIC: u8 = 0xE9;

/// `write_bin` of `image` at this unit's `app0` offset. Never erase, never
/// the `espflash` `flash` subcommand, never a caller-chosen address.
pub fn flash_app<D: DeviceIo>(
    device: &D,
    layout: &Layout,
    port: &str,
    image: &Path,
    yes: bool,
    allow_unknown_layout: bool,
    capture: Option<&str>,
) -> Result<(), Error> {
    if !yes {
        return Err(Error::FlashNotConfirmed);
    }
    let (_, board) = crate::detect::read_live_board(device, port)?;
    let net = if let Some(slug) = capture {
        let snapshot = require_capture_backup(layout, &board.identity, slug)?;
        crate::original::SafetyNet {
            is_original: false,
            snapshot,
        }
    } else {
        require_safety_net(layout, &board.identity)?
    };
    if !net.is_original {
        eprintln!(
            "flash-app: using capture {} — this is not a factory restore; lost nvs is not recoverable",
            net.snapshot.dir.display()
        );
    }
    let layout_status = match_layout(&net.snapshot.manifest.partitions);
    if !layout_status.is_known() && !allow_unknown_layout {
        return Err(Error::UnsafePartitionLayout {
            status: layout_status.evidence_token(),
        });
    }
    let app0 = net
        .snapshot
        .manifest
        .partitions
        .iter()
        .find(|part| part.label == "app0")
        .ok_or_else(|| Error::UnknownPartition("app0".into()))?;
    if app0.offset < APP0_MIN_OFFSET {
        return Err(Error::UnsafeAppOffset(app0.offset));
    }
    let bytes = read_app_image(image, app0.size)?;
    validate_app_image(&bytes, app0.size)?;
    device.write_bin(port, app0.offset, image)
}

/// Refuse an empty file or an ELF. A `save-image` payload usually
/// starts with [`ESP_IMAGE_MAGIC`]; this check does not require it.
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

    fn valid_s3_image() -> Vec<u8> {
        let mut image = vec![0; 48];
        image[0] = ESP_IMAGE_MAGIC;
        image[1] = 1;
        image[12] = 9;
        image[28..32].copy_from_slice(&4u32.to_le_bytes());
        image[32..36].copy_from_slice(&[1, 2, 3, 4]);
        image[47] = 0xef ^ 1 ^ 2 ^ 3 ^ 4;
        image
    }

    fn factory_table() -> Vec<u8> {
        let csv: String = crate::partition_layouts::FACTORY_32MB_V1
            .entries
            .iter()
            .map(|p| {
                format!(
                    "{},{},0x{:x},0x{:x},0x{:x},\n",
                    p.label,
                    if p.type_id == 0 { "app" } else { "data" },
                    p.subtype_id,
                    p.offset,
                    p.size
                )
            })
            .collect();
        esp_idf_part::PartitionTable::try_from_str(csv)
            .unwrap()
            .to_bin()
            .unwrap()
    }

    #[test]
    fn external_backup_flash_validates_live_table_and_image_before_app0_write() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::from_developer_data_root(tmp.path());
        let mut flash = vec![0xff; PARTITION_TABLE_OFFSET + PARTITION_TABLE_LEN];
        let table = factory_table();
        flash[PARTITION_TABLE_OFFSET..PARTITION_TABLE_OFFSET + table.len()].copy_from_slice(&table);
        let mock = RefCell::new(MockDevice {
            flash,
            board_info: info(),
            ..Default::default()
        });
        let bytes = valid_s3_image();
        let image = payload(tmp.path(), &bytes);
        let options = FlashAppOptions {
            yes: true,
            backup_policy: BackupPolicy::ExternallyBackedUp,
            ..Default::default()
        };
        flash_app_with_options(&mock, &layout, "PORT", &image, &options).unwrap();
        assert_eq!(mock.borrow().writes, [(APP0_MIN_OFFSET, bytes)]);
        mock.borrow_mut().writes.clear();
        for security in [
            "",
            "Secure Boot: Unknown\nFlash Encryption: Disabled\n",
            "Secure Boot: Disabled\nFlash Encryption: Unknown\n",
            "Secure Boot: Enabled\nFlash Encryption: Disabled\n",
            "Secure Boot: Disabled\nFlash Encryption: Enabled\n",
        ] {
            mock.borrow_mut().board_info =
                format!("Flash size: 32MB\nMAC address: {}\n{security}", test_mac());
            assert!(flash_app_with_options(&mock, &layout, "PORT", &image, &options).is_err());
            assert!(mock.borrow().writes.is_empty());
        }
        mock.borrow_mut().board_info = info();
        mock.borrow_mut().flash[PARTITION_TABLE_OFFSET + 16] ^= 1;
        assert!(flash_app_with_options(&mock, &layout, "PORT", &image, &options).is_err());
        assert!(mock.borrow().writes.is_empty());
    }

    #[test]
    fn force_image_check_rejects_wrong_chip_truncation_and_bad_checksum() {
        let bytes = valid_s3_image();
        validate_esp32s3_image(&bytes).unwrap();
        for size in 0..bytes.len() {
            assert!(validate_esp32s3_image(&bytes[..size]).is_err());
        }
        for index in [0, 1, 12, 28, 32, 47] {
            let mut invalid = bytes.clone();
            invalid[index] ^= 0x40;
            assert!(
                validate_esp32s3_image(&invalid).is_err(),
                "corrupted byte {index}"
            );
        }
        let mut hashed = bytes;
        hashed[23] = 1;
        use sha2::{Digest, Sha256};
        hashed.extend_from_slice(&Sha256::digest(&hashed));
        validate_esp32s3_image(&hashed).unwrap();
        *hashed.last_mut().unwrap() ^= 1;
        assert!(validate_esp32s3_image(&hashed).is_err());
    }
    use crate::manifest::SnapshotKind;
    use crate::original::load_manifest;
    use crate::partition_layouts::{partitions_from_layout, FACTORY_32MB_V1};
    use crate::partitions::{test_entry, PARTITION_TABLE_LEN, PARTITION_TABLE_OFFSET};
    use std::cell::RefCell;
    use std::fs;

    fn flash(
        mock: &RefCell<MockDevice>,
        layout: &Layout,
        port: &str,
        image: &Path,
        yes: bool,
    ) -> Result<(), Error> {
        flash_app(mock, layout, port, image, yes, true, None)
    }

    const APP0_SIZE: u32 = 256;

    fn dump_with_app0() -> Vec<u8> {
        let end = APP0_MIN_OFFSET as usize + APP0_SIZE as usize;
        let mut dump = vec![0u8; end];
        dump[PARTITION_TABLE_OFFSET..PARTITION_TABLE_OFFSET + 32]
            .copy_from_slice(&test_entry("nvs", 0x01, 0x02, 0x9000, 16));
        dump[PARTITION_TABLE_OFFSET + 32..PARTITION_TABLE_OFFSET + 64]
            .copy_from_slice(&test_entry("app0", 0x00, 0x10, APP0_MIN_OFFSET, APP0_SIZE));
        dump[APP0_MIN_OFFSET as usize] = ESP_IMAGE_MAGIC;
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
        let bytes = vec![ESP_IMAGE_MAGIC; APP0_SIZE as usize + 1];
        assert!(matches!(
            validate_app_image(&bytes, APP0_SIZE),
            Err(Error::ImageTooLarge { size, max }) if size == u64::from(APP0_SIZE) + 1 && max == APP0_SIZE
        ));
    }

    #[test]
    fn flash_without_yes_refuses() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
        let mock = RefCell::new(MockDevice::default());
        let image = payload(tmp.path(), &[ESP_IMAGE_MAGIC, 0x01]);
        let err = flash(&mock, &layout, "PORT", &image, false).unwrap_err();
        assert!(matches!(err, Error::FlashNotConfirmed));
    }

    #[test]
    fn flash_refuses_without_original() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
        let board = info();
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let image = payload(tmp.path(), &[ESP_IMAGE_MAGIC, 0x01]);
        let err = flash(&mock, &layout, "PORT", &image, true).unwrap_err();
        assert!(matches!(err, Error::MissingOriginal));
    }

    #[test]
    fn flash_refuses_identity_mismatch() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
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
        let image = payload(tmp.path(), &[ESP_IMAGE_MAGIC, 0x01]);
        assert!(matches!(
            flash(&mock, &layout, "PORT", &image, true),
            Err(Error::MissingOriginal)
        ));
    }

    #[test]
    fn flash_writes_app0_only() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
        let board = info();
        persist(&layout, &board);
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let bytes = vec![ESP_IMAGE_MAGIC, 0x03, 0x02, 0x01];
        let image = payload(tmp.path(), &bytes);
        flash(&mock, &layout, "PORT", &image, true).unwrap();
        let writes = &mock.borrow().writes;
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, APP0_MIN_OFFSET);
        assert_eq!(writes[0].1, bytes);
    }

    #[test]
    fn flash_refuses_elf_file() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
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
            flash(&mock, &layout, "PORT", &image, true),
            Err(Error::ImageNotApp)
        ));
        assert!(mock.borrow().writes.is_empty());
    }

    #[test]
    fn flash_refuses_when_table_has_no_app0() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
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
        let image = payload(tmp.path(), &[ESP_IMAGE_MAGIC, 0x01]);
        assert!(matches!(
            flash(&mock, &layout, "PORT", &image, true),
            Err(Error::UnknownPartition(label)) if label == "app0"
        ));
    }

    #[test]
    fn flash_refuses_empty_and_oversized_files() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
        let board = info();
        persist(&layout, &board);
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let empty = payload(tmp.path(), &[]);
        assert!(matches!(
            flash(&mock, &layout, "PORT", &empty, true),
            Err(Error::ImageNotApp)
        ));
        let huge = payload(tmp.path(), &vec![ESP_IMAGE_MAGIC; APP0_SIZE as usize + 1]);
        assert!(matches!(
            flash(&mock, &layout, "PORT", &huge, true),
            Err(Error::ImageTooLarge { size, max }) if size == u64::from(APP0_SIZE) + 1 && max == APP0_SIZE
        ));
        assert!(mock.borrow().writes.is_empty());
    }

    #[test]
    fn flash_refuses_usb_serial_mismatch() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
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
        let image = payload(tmp.path(), &[ESP_IMAGE_MAGIC, 0x01]);
        assert!(matches!(
            flash(&mock, &layout, &port, &image, true),
            Err(Error::IdentityMismatch { .. })
        ));
        assert!(mock.borrow().writes.is_empty());
    }

    #[test]
    fn flash_refuses_app0_below_min_offset() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
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
        let image = payload(tmp.path(), &[ESP_IMAGE_MAGIC, 0x01]);
        assert!(matches!(
            flash(&mock, &layout, "PORT", &image, true),
            Err(Error::UnsafeAppOffset(offset)) if offset == unsafe_off
        ));
        assert!(mock.borrow().writes.is_empty());
    }

    fn write_capture_with_parts(
        layout: &Layout,
        slug: &str,
        info: &str,
        partitions: Vec<crate::partitions::Partition>,
    ) {
        let board = parse_board_info(info).unwrap();
        persist_original(
            layout,
            "TESTFACTORY001",
            &dump_with_app0(),
            &board,
            "",
            info,
            false,
        )
        .unwrap();
        let original_dir = layout.original_dir("TESTFACTORY001");
        let mut manifest = load_manifest(&original_dir).unwrap();
        crate::backup::unseal_tree(&original_dir).unwrap();
        fs::remove_dir_all(&original_dir).unwrap();
        manifest.kind = SnapshotKind::Capture;
        manifest.image_name = Some(slug.into());
        manifest.partitions = partitions;
        let dest = layout.capture_dir("TESTFACTORY001", slug);
        fs::create_dir_all(&dest).unwrap();
        fs::write(
            dest.join("MANIFEST.yaml"),
            noyalib::to_string(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn flash_refuses_unknown_layout_without_override() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
        let board = info();
        persist(&layout, &board);
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let image = payload(tmp.path(), &[ESP_IMAGE_MAGIC, 0x01]);
        assert!(matches!(
            flash_app(&mock, &layout, "PORT", &image, true, false, None),
            Err(Error::UnsafePartitionLayout { .. })
        ));
        assert!(mock.borrow().writes.is_empty());
    }

    #[test]
    fn flash_accepts_factory_v1_without_override() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
        let board = info();
        persist(&layout, &board);
        let dir = layout.original_dir("TESTFACTORY001");
        crate::backup::unseal_tree(&dir).unwrap();
        let mut manifest = load_manifest(&dir).unwrap();
        manifest.partitions = partitions_from_layout(&FACTORY_32MB_V1);
        fs::write(
            dir.join("MANIFEST.yaml"),
            noyalib::to_string(&manifest).unwrap(),
        )
        .unwrap();
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let image = payload(tmp.path(), &[ESP_IMAGE_MAGIC, 0x01]);
        flash_app(&mock, &layout, "PORT", &image, true, false, None).unwrap();
        assert_eq!(mock.borrow().writes.len(), 1);
        assert_eq!(mock.borrow().writes[0].0, APP0_MIN_OFFSET);
    }

    #[test]
    fn flash_uses_capture_as_safety_net() {
        let tmp = crate::backup::UnsealOnDrop::new();
        let layout = Layout::from_developer_data_root(tmp.path());
        let board = info();
        write_capture_with_parts(
            &layout,
            "after-flash",
            &board,
            partitions_from_layout(&FACTORY_32MB_V1),
        );
        let mock = RefCell::new(MockDevice {
            board_info: board,
            ..MockDevice::default()
        });
        let image = payload(tmp.path(), &[ESP_IMAGE_MAGIC, 0x01]);
        flash_app(&mock, &layout, "PORT", &image, true, false, None).unwrap();
        assert_eq!(mock.borrow().writes.len(), 1);
    }
}
