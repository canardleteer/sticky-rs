//! Write-once factory original capture.

use std::fs;
use std::path::{Path, PathBuf};

use crate::device::DeviceIo;
use crate::dump::{require_full_dump, sha256_hex, split_image};
use crate::identity::{parse_board_info, parse_factory_serial, BoardInfo};
use crate::manifest::{Manifest, PartitionHash};
use crate::original::{refuse_if_original_exists, Layout};
use crate::partitions::partitions_csv;
use crate::{Error, CHUNKS, CHUNK_SIZE, FLASH_SIZE};

/// Live capture: UART serial, board-info, chunked 32 MiB read, write-once dir.
pub fn backup_live<D: DeviceIo>(device: &D, layout: &Layout, port: &str) -> Result<PathBuf, Error> {
    let uart = device.sample_uart(port)?;
    let factory_serial = parse_factory_serial(&uart)?;
    refuse_if_original_exists(layout, &factory_serial)?;
    let (info_text, board) = crate::detect::read_live_board(device, port)?;
    let dump = read_full_flash(device, port)?;
    persist_original(
        layout,
        &factory_serial,
        &dump,
        &board,
        &uart,
        &info_text,
        true,
    )
}

/// Host-only copy of an already-taken 32 MiB dump tree. Still write-once.
pub fn backup_import(layout: &Layout, source: &Path) -> Result<PathBuf, Error> {
    let dump_path = source.join("flash-32mb.bin");
    let dump = fs::read(&dump_path).map_err(|error| {
        Error::Import(format!("failed to read {}: {error}", dump_path.display()))
    })?;
    require_full_dump(&dump)?;
    let uart = read_optional_text(source, "uart-sample.txt")
        .or_else(|| read_optional_text(source, "serial-samples.txt"))
        .unwrap_or_default();
    let factory_serial = factory_serial_for_import(source, &uart)?;
    refuse_if_original_exists(layout, &factory_serial)?;
    let info_text = read_optional_text(source, "board-info.txt").unwrap_or_default();
    let mut board = if info_text.is_empty() {
        return Err(Error::Import(
            "board-info.txt missing; cannot bind MAC".into(),
        ));
    } else {
        parse_board_info(&info_text)?
    };
    board.identity.usb_serial = None;
    persist_original(
        layout,
        &factory_serial,
        &dump,
        &board,
        &uart,
        &info_text,
        true,
    )
}

/// Full-chip 32 MiB read. [`crate::device::RealDevice`] chunks internally on one flasher session.
pub fn read_full_flash<D: DeviceIo>(device: &D, port: &str) -> Result<Vec<u8>, Error> {
    let dump = device.read_flash(port, 0, FLASH_SIZE as u32)?;
    require_full_dump(&dump)?;
    Ok(dump)
}

fn read_optional_text(dir: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name)).ok()
}

/// Factory serial for `--import`: an xtask [`Manifest`] if that file parses,
/// otherwise `key=serial_number` from the UART sample. Sibling dump trees
/// often ship a different `MANIFEST.json` (offsets and hashes, no
/// `factory_serial` / `type_name`).
fn factory_serial_for_import(source: &Path, uart: &str) -> Result<String, Error> {
    let manifest_path = source.join("MANIFEST.json");
    if manifest_path.is_file() {
        if let Ok(existing) = serde_json::from_str::<Manifest>(&fs::read_to_string(manifest_path)?)
        {
            crate::identity::validate_factory_serial(&existing.factory_serial)?;
            return Ok(existing.factory_serial);
        }
    }
    parse_factory_serial(uart).map_err(|error| match error {
        Error::MissingFactorySerial => Error::Import(
            "no xtask MANIFEST.json factory_serial and no key=serial_number in uart-sample.txt / serial-samples.txt"
                .into(),
        ),
        other => other,
    })
}

/// Write `backups/original/{serial}/` via a `.partial` directory then rename.
pub fn persist_original(
    layout: &Layout,
    factory_serial: &str,
    dump: &[u8],
    board: &BoardInfo,
    uart_sample: &str,
    board_info_text: &str,
    require_full: bool,
) -> Result<PathBuf, Error> {
    crate::identity::validate_factory_serial(factory_serial)?;
    refuse_if_original_exists(layout, factory_serial)?;
    if require_full {
        require_full_dump(dump)?;
    }
    let split = split_image(dump)?;
    let dest = layout.original_dir(factory_serial);
    let partial = layout
        .originals_dir()
        .join(format!("{factory_serial}.partial"));
    if partial.exists() {
        fs::remove_dir_all(&partial)?;
    }
    fs::create_dir_all(&partial)?;
    fs::write(partial.join("flash-32mb.bin"), dump)?;
    if dump.len() == FLASH_SIZE {
        let chunks = partial.join("chunks");
        fs::create_dir_all(&chunks)?;
        for index in 0..CHUNKS {
            let start = index * CHUNK_SIZE;
            fs::write(
                chunks.join(format!("{index:02}.bin")),
                &dump[start..start + CHUNK_SIZE],
            )?;
        }
    }
    fs::write(partial.join("bootloader.bin"), &split.bootloader)?;
    fs::write(partial.join("partition-table.bin"), &split.table)?;
    let mut partition_sha256 = Vec::new();
    for (part, data) in &split.parts {
        fs::write(partial.join(format!("part-{}.bin", part.label)), data)?;
        partition_sha256.push(PartitionHash {
            label: part.label.clone(),
            sha256: sha256_hex(data),
        });
    }
    fs::write(
        partial.join("partitions.csv"),
        partitions_csv(&split.partitions),
    )?;
    fs::write(partial.join("uart-sample.txt"), uart_sample)?;
    fs::write(partial.join("board-info.txt"), board_info_text)?;

    let manifest = Manifest {
        factory_serial: factory_serial.to_string(),
        usb_serial: board.identity.usb_serial.clone(),
        mac: board.identity.mac.clone(),
        flash_size: board.flash_size.clone(),
        secure_boot: board.secure_boot,
        flash_encryption: board.flash_encryption,
        dump_sha256: sha256_hex(dump),
        bootloader_sha256: sha256_hex(&split.bootloader),
        partition_table_sha256: sha256_hex(&split.table),
        boot_slot: split.boot_slot,
        app0_desc: split.app0_desc,
        partitions: split.partitions,
        partition_sha256,
    };
    fs::write(
        partial.join("MANIFEST.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    let mut sums = String::new();
    for name in ["flash-32mb.bin", "bootloader.bin", "partition-table.bin"] {
        let path = partial.join(name);
        if path.is_file() {
            sums.push_str(&format!("{}  {name}\n", sha256_hex(&fs::read(path)?)));
        }
    }
    fs::write(partial.join("SHA256SUMS"), sums)?;
    fs::create_dir_all(layout.originals_dir())?;
    fs::rename(&partial, &dest)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::test_mac;
    use crate::partitions::{test_entry, PARTITION_TABLE_OFFSET};

    fn tmp_layout() -> (tempfile::TempDir, Layout) {
        let tmp = tempfile::tempdir().unwrap();
        let backups_root = tmp.path().join("backups");
        (tmp, Layout { backups_root })
    }

    fn tiny_dump() -> Vec<u8> {
        let nvs_off = 0x9000u32;
        let mut dump = vec![0u8; (nvs_off + 16) as usize];
        dump[PARTITION_TABLE_OFFSET..PARTITION_TABLE_OFFSET + 32]
            .copy_from_slice(&test_entry("nvs", 0x01, 0x02, nvs_off, 16));
        dump
    }

    fn board() -> BoardInfo {
        let mac = test_mac();
        parse_board_info(&format!(
            "Flash size: 32MB\nMAC address: {mac}\nSecure Boot: Disabled\nFlash Encryption: Disabled\n"
        ))
        .unwrap()
    }

    #[test]
    fn persist_refuses_second_write() {
        let (_tmp, layout) = tmp_layout();
        let dump = tiny_dump();
        persist_original(
            &layout,
            "TESTFACTORY001",
            &dump,
            &board(),
            "",
            "Flash size: 32MB\n",
            false,
        )
        .unwrap();
        let err = persist_original(
            &layout,
            "TESTFACTORY001",
            &dump,
            &board(),
            "",
            "Flash size: 32MB\n",
            false,
        )
        .unwrap_err();
        assert!(matches!(err, Error::OriginalExists(_)));
    }

    #[test]
    fn persist_writes_manifest_and_nvs_slice() {
        let (_tmp, layout) = tmp_layout();
        let dest = persist_original(
            &layout,
            "TESTFACTORY001",
            &tiny_dump(),
            &board(),
            "key=serial_number value=TESTFACTORY001\n",
            &format!("Flash size: 32MB\nMAC address: {}\n", test_mac()),
            false,
        )
        .unwrap();
        assert!(dest.join("MANIFEST.json").is_file());
        assert!(dest.join("part-nvs.bin").is_file());
        assert!(dest.join("partitions.csv").is_file());
    }

    #[test]
    fn backup_live_refuses_when_original_exists() {
        let (_tmp, layout) = tmp_layout();
        persist_original(
            &layout,
            "TESTFACTORY001",
            &tiny_dump(),
            &board(),
            "",
            "Flash size: 32MB\n",
            false,
        )
        .unwrap();
        let mock = std::cell::RefCell::new(crate::device::MockDevice {
            uart: "key=serial_number value=TESTFACTORY001\n".into(),
            ..crate::device::MockDevice::default()
        });
        let err = backup_live(&mock, &layout, "PORT").unwrap_err();
        assert!(matches!(err, Error::OriginalExists(_)));
    }

    #[test]
    fn import_rejects_short_dump() {
        let (tmp, layout) = tmp_layout();
        let source = tmp.path().join("incoming");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("flash-32mb.bin"), [0u8; 16]).unwrap();
        assert!(matches!(
            backup_import(&layout, &source),
            Err(Error::DumpLength(_))
        ));
    }

    #[test]
    fn import_serial_uses_xtask_manifest_when_it_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("incoming");
        fs::create_dir_all(&source).unwrap();
        let manifest = Manifest {
            factory_serial: "TESTFACTORY001".into(),
            usb_serial: None,
            mac: crate::identity::test_mac(),
            flash_size: "32MB".into(),
            secure_boot: false,
            flash_encryption: false,
            dump_sha256: String::new(),
            bootloader_sha256: String::new(),
            partition_table_sha256: String::new(),
            boot_slot: None,
            app0_desc: None,
            partitions: Vec::new(),
            partition_sha256: Vec::new(),
        };
        fs::write(
            source.join("MANIFEST.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            source.join("serial-samples.txt"),
            "key=serial_number value=OTHERSERIAL\n",
        )
        .unwrap();
        let uart = fs::read_to_string(source.join("serial-samples.txt")).unwrap();
        assert_eq!(
            factory_serial_for_import(&source, &uart).unwrap(),
            "TESTFACTORY001"
        );
    }

    #[test]
    fn import_serial_falls_back_when_manifest_is_a_sibling_dump() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("incoming");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("MANIFEST.json"),
            r#"{
              "dump_file": "flash-32mb.bin",
              "partitions": [{ "label": "nvs", "type": "data", "offset": 0, "size": 1 }]
            }"#,
        )
        .unwrap();
        let uart = "key=serial_number value=TESTFACTORY001\n";
        fs::write(source.join("serial-samples.txt"), uart).unwrap();
        assert_eq!(
            factory_serial_for_import(&source, uart).unwrap(),
            "TESTFACTORY001"
        );
    }

    #[test]
    fn import_serial_explains_when_neither_source_works() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("incoming");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("MANIFEST.json"),
            r#"{"dump_file":"flash-32mb.bin"}"#,
        )
        .unwrap();
        let err = factory_serial_for_import(&source, "").unwrap_err();
        assert!(matches!(err, Error::Import(_)));
    }
}
