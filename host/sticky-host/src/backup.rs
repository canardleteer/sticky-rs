//! Write-once factory originals and named captures.

use std::fs;
use std::path::{Path, PathBuf};

use crate::classify::{classify, Classification, ClassifyInput, ClassifyResult};
use crate::device::DeviceIo;
use crate::dump::{require_full_dump, sha256_hex, split_image};
use crate::identity::{
    parse_board_info, parse_factory_serial, unit_id, validate_factory_serial, BoardInfo,
};
use crate::manifest::{Manifest, PartitionHash, SnapshotKind, MANIFEST_SCHEMA};
use crate::original::{refuse_if_capture_exists, refuse_if_original_exists, Layout};
use crate::partition_layouts::match_layout;
use crate::{Error, CHUNKS, CHUNK_SIZE, FLASH_SIZE};

/// Flags for a backup (live dump or `--import`).
#[derive(Debug, Clone, Default)]
pub struct BackupRequest {
    /// Capture slug. Required when classification is not [`Classification::KnownFactory`].
    pub name: Option<String>,
    /// Store an uncertain-stock dump under `original/` (records the fingerprint
    /// in the manifest only; does not add it to the in-repo catalog).
    pub as_original: bool,
}

/// Live capture: UART sample, board-info, chunked 32 MiB read, write-once dir.
///
/// `ask_name` is called once when a slug is required and [`BackupRequest::name`]
/// is empty. Return `Some(slug)` or `None` to surface [`Error::NeedsSnapshotName`].
/// xtask prompts on a TTY; tests pass a closure. The dump is not repeated.
pub fn backup_live<D, F>(
    device: &D,
    layout: &Layout,
    port: &str,
    request: &BackupRequest,
    ask_name: F,
) -> Result<PathBuf, Error>
where
    D: DeviceIo,
    F: FnOnce(&str) -> Result<Option<String>, Error>,
{
    let uart = device.sample_uart(port)?;
    if request.as_original {
        if let Ok(serial) = parse_factory_serial(&uart) {
            refuse_if_original_exists(layout, &serial)?;
        }
    }
    let (info_text, board) = crate::detect::read_live_board(device, port)?;
    let dump = read_full_flash(device, port)?;
    persist_classified(
        layout, &dump, &board, &uart, &info_text, true, request, ask_name,
    )
}

/// Host-only copy of an already-taken 32 MiB dump tree. Still write-once.
pub fn backup_import<F>(
    layout: &Layout,
    source: &Path,
    request: &BackupRequest,
    ask_name: F,
) -> Result<PathBuf, Error>
where
    F: FnOnce(&str) -> Result<Option<String>, Error>,
{
    let dump_path = source.join("flash-32mb.bin");
    let dump = fs::read(&dump_path).map_err(|error| {
        Error::Import(format!("failed to read {}: {error}", dump_path.display()))
    })?;
    require_full_dump(&dump)?;
    let uart = read_optional_text(source, "uart-sample.txt")
        .or_else(|| read_optional_text(source, "serial-samples.txt"))
        .unwrap_or_default();
    let info_text = read_optional_text(source, "board-info.txt").unwrap_or_default();
    let mut board = if info_text.is_empty() {
        return Err(Error::Import(
            "board-info.txt missing; cannot bind MAC".into(),
        ));
    } else {
        parse_board_info(&info_text)?
    };
    board.identity.usb_serial = None;
    persist_classified(
        layout, &dump, &board, &uart, &info_text, true, request, ask_name,
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

#[allow(clippy::too_many_arguments)]
fn persist_classified<F>(
    layout: &Layout,
    dump: &[u8],
    board: &BoardInfo,
    uart_sample: &str,
    board_info_text: &str,
    require_full: bool,
    request: &BackupRequest,
    ask_name: F,
) -> Result<PathBuf, Error>
where
    F: FnOnce(&str) -> Result<Option<String>, Error>,
{
    if require_full {
        require_full_dump(dump)?;
    }
    let split = split_image(dump)?;
    let classed = classify(ClassifyInput {
        partitions: &split.partitions,
        app0_desc: split.app0_desc.as_ref(),
        uart: uart_sample,
    });
    eprintln!("backup: {}", classed.evidence.summary());
    if matches!(classed.class, Classification::KnownFactory { .. }) {
        eprintln!("backup: classified as known factory");
    } else {
        eprintln!("backup: not claiming factory ({})", classed.manifest_tag());
    }

    let uart_serial = parse_factory_serial(uart_sample).ok();
    let dest = decide_dest(
        &classed,
        request,
        uart_serial.as_deref(),
        &board.identity.mac,
        ask_name,
    )?;
    persist_tree(
        layout,
        dest,
        dump,
        board,
        uart_sample,
        board_info_text,
        require_full,
        &classed,
    )
}

enum Dest {
    Original { serial: String },
    Capture { unit_id: String, slug: String },
}

fn decide_dest<F>(
    classed: &ClassifyResult,
    request: &BackupRequest,
    uart_serial: Option<&str>,
    mac: &str,
    ask_name: F,
) -> Result<Dest, Error>
where
    F: FnOnce(&str) -> Result<Option<String>, Error>,
{
    match &classed.class {
        Classification::KnownFactory { .. } => {
            if let Some(name) = request.name.as_deref() {
                if request.as_original {
                    original_dest(uart_serial)
                } else {
                    capture_dest(uart_serial, mac, name)
                }
            } else {
                original_dest(uart_serial)
            }
        }
        Classification::UncertainStock { .. } => {
            if request.as_original {
                original_dest(uart_serial)
            } else {
                let slug = resolve_name(request, &classed.evidence.summary(), ask_name)?;
                capture_dest(uart_serial, mac, &slug)
            }
        }
        Classification::NotFactory { .. } => {
            if request.as_original {
                return Err(Error::NotFactoryAsOriginal);
            }
            let slug = resolve_name(request, &classed.evidence.summary(), ask_name)?;
            capture_dest(uart_serial, mac, &slug)
        }
    }
}

fn original_dest(uart_serial: Option<&str>) -> Result<Dest, Error> {
    let serial = uart_serial.ok_or(Error::MissingFactorySerial)?;
    validate_factory_serial(serial)?;
    Ok(Dest::Original {
        serial: serial.to_string(),
    })
}

fn capture_dest(uart_serial: Option<&str>, mac: &str, slug: &str) -> Result<Dest, Error> {
    validate_factory_serial(slug)?;
    Ok(Dest::Capture {
        unit_id: unit_id(uart_serial, mac)?,
        slug: slug.to_string(),
    })
}

fn resolve_name<F>(request: &BackupRequest, evidence: &str, ask_name: F) -> Result<String, Error>
where
    F: FnOnce(&str) -> Result<Option<String>, Error>,
{
    if let Some(name) = request.name.as_deref() {
        validate_factory_serial(name)?;
        return Ok(name.to_string());
    }
    match ask_name(evidence)? {
        Some(name) => {
            validate_factory_serial(&name)?;
            Ok(name)
        }
        None => Err(Error::NeedsSnapshotName {
            evidence: evidence.to_string(),
        }),
    }
}

/// Write `developer-data/backups/original/{serial}/` via a `.partial` directory then rename.
pub fn persist_original(
    layout: &Layout,
    factory_serial: &str,
    dump: &[u8],
    board: &BoardInfo,
    uart_sample: &str,
    board_info_text: &str,
    require_full: bool,
) -> Result<PathBuf, Error> {
    validate_factory_serial(factory_serial)?;
    if require_full {
        require_full_dump(dump)?;
    }
    let split = split_image(dump)?;
    let classed = classify(ClassifyInput {
        partitions: &split.partitions,
        app0_desc: split.app0_desc.as_ref(),
        uart: uart_sample,
    });
    persist_tree(
        layout,
        Dest::Original {
            serial: factory_serial.to_string(),
        },
        dump,
        board,
        uart_sample,
        board_info_text,
        require_full,
        &classed,
    )
}

#[allow(clippy::too_many_arguments)]
fn persist_tree(
    layout: &Layout,
    dest: Dest,
    dump: &[u8],
    board: &BoardInfo,
    uart_sample: &str,
    board_info_text: &str,
    require_full: bool,
    classed: &ClassifyResult,
) -> Result<PathBuf, Error> {
    if require_full {
        require_full_dump(dump)?;
    }
    let split = split_image(dump)?;
    let (final_dir, partial, kind, factory_serial, image_name) = match &dest {
        Dest::Original { serial } => {
            refuse_if_original_exists(layout, serial)?;
            let dest_dir = layout.original_dir(serial);
            let partial = layout.originals_dir().join(format!("{serial}.partial"));
            (
                dest_dir,
                partial,
                SnapshotKind::Original,
                serial.clone(),
                None,
            )
        }
        Dest::Capture { unit_id, slug } => {
            refuse_if_capture_exists(layout, unit_id, slug)?;
            let dest_dir = layout.capture_dir(unit_id, slug);
            let partial = layout
                .captures_dir()
                .join(unit_id)
                .join(format!("{slug}.partial"));
            (
                dest_dir,
                partial,
                SnapshotKind::Capture,
                unit_id.clone(),
                Some(slug.clone()),
            )
        }
    };
    if partial.exists() {
        fs::remove_dir_all(&partial)?;
    }
    if let Some(parent) = partial.parent() {
        fs::create_dir_all(parent)?;
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
    fs::write(partial.join("uart-sample.txt"), uart_sample)?;
    fs::write(partial.join("board-info.txt"), board_info_text)?;

    let layout_match = match_layout(&split.partitions);
    let manifest = Manifest {
        schema: MANIFEST_SCHEMA.to_string(),
        kind,
        layout_id: layout_match.id().map(str::to_string),
        classification: Some(classed.manifest_tag()),
        image_name,
        factory_serial,
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
    let yaml = noyalib::to_string(&manifest).map_err(|error| Error::Yaml(error.to_string()))?;
    fs::write(partial.join("MANIFEST.yaml"), yaml)?;
    let mut sums = String::new();
    for name in ["flash-32mb.bin", "bootloader.bin", "partition-table.bin"] {
        let path = partial.join(name);
        if path.is_file() {
            sums.push_str(&format!("{}  {name}\n", sha256_hex(&fs::read(path)?)));
        }
    }
    fs::write(partial.join("SHA256SUMS"), sums)?;
    if let Some(parent) = final_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&partial, &final_dir)?;
    Ok(final_dir)
}

/// Factory serial from an xtask manifest (YAML or JSON) when that file parses.
pub fn factory_serial_for_import(source: &Path, uart: &str) -> Result<String, Error> {
    for name in ["MANIFEST.yaml", "MANIFEST.json"] {
        let path = source.join(name);
        if !path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let parsed = if name.ends_with(".yaml") {
            noyalib::from_str::<Manifest>(&text).ok()
        } else {
            serde_json::from_str::<Manifest>(&text).ok()
        };
        if let Some(existing) = parsed {
            validate_factory_serial(&existing.factory_serial)?;
            return Ok(existing.factory_serial);
        }
    }
    parse_factory_serial(uart).map_err(|error| match error {
        Error::MissingFactorySerial => Error::Import(
            "no xtask MANIFEST.yaml / MANIFEST.json factory_serial and no key=serial_number in uart-sample.txt / serial-samples.txt"
                .into(),
        ),
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::test_mac;
    use crate::original::load_manifest;
    use crate::partitions::{test_entry, PARTITION_TABLE_OFFSET};

    fn tmp_layout() -> (tempfile::TempDir, Layout) {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::from_developer_data_root(tmp.path());
        (tmp, layout)
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

    fn no_name(_: &str) -> Result<Option<String>, Error> {
        Ok(None)
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
    fn persist_writes_yaml_and_nvs_slice() {
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
        assert!(dest.join("MANIFEST.yaml").is_file());
        assert!(!dest.join("MANIFEST.json").exists());
        assert!(!dest.join("partitions.csv").exists());
        assert!(dest.join("part-nvs.bin").is_file());
        assert!(dest.join("SHA256SUMS").is_file());
        let manifest = load_manifest(&dest).unwrap();
        assert_eq!(manifest.kind, SnapshotKind::Original);
        assert_eq!(manifest.schema, MANIFEST_SCHEMA);
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
        let request = BackupRequest {
            as_original: true,
            ..BackupRequest::default()
        };
        let err = backup_live(&mock, &layout, "PORT", &request, no_name).unwrap_err();
        assert!(matches!(err, Error::OriginalExists(_)));
    }

    #[test]
    fn classified_custom_writes_capture() {
        let (_tmp, layout) = tmp_layout();
        let dest = persist_classified(
            &layout,
            &tiny_dump(),
            &board(),
            "git=deadbeef dirty=0\n",
            &format!("Flash size: 32MB\nMAC address: {}\n", test_mac()),
            false,
            &BackupRequest {
                name: Some("after-simple-debug".into()),
                as_original: false,
            },
            no_name,
        )
        .unwrap();
        assert!(dest.to_string_lossy().contains("captures/"));
        assert!(dest.join("MANIFEST.yaml").is_file());
        let manifest = load_manifest(&dest).unwrap();
        assert_eq!(manifest.kind, SnapshotKind::Capture);
        assert_eq!(manifest.image_name.as_deref(), Some("after-simple-debug"));
        assert!(manifest
            .classification
            .as_deref()
            .is_some_and(|tag| tag.starts_with("not_factory:")));
    }

    #[test]
    fn classified_custom_refuses_as_original() {
        let (_tmp, layout) = tmp_layout();
        let err = persist_classified(
            &layout,
            &tiny_dump(),
            &board(),
            "git=deadbeef dirty=0\n",
            &format!("Flash size: 32MB\nMAC address: {}\n", test_mac()),
            false,
            &BackupRequest {
                as_original: true,
                ..BackupRequest::default()
            },
            no_name,
        )
        .unwrap_err();
        assert!(matches!(err, Error::NotFactoryAsOriginal));
    }

    #[test]
    fn classified_uncertain_needs_name() {
        let (_tmp, layout) = tmp_layout();
        let err = persist_classified(
            &layout,
            &tiny_dump(),
            &board(),
            "key=serial_number value=TESTFACTORY001\n",
            &format!("Flash size: 32MB\nMAC address: {}\n", test_mac()),
            false,
            &BackupRequest::default(),
            no_name,
        )
        .unwrap_err();
        assert!(matches!(err, Error::NeedsSnapshotName { .. }));
    }

    #[test]
    fn import_rejects_short_dump() {
        let (tmp, layout) = tmp_layout();
        let source = tmp.path().join("incoming");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("flash-32mb.bin"), [0u8; 16]).unwrap();
        assert!(matches!(
            backup_import(&layout, &source, &BackupRequest::default(), no_name),
            Err(Error::DumpLength(_))
        ));
    }

    #[test]
    fn import_serial_uses_xtask_manifest_when_it_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("incoming");
        fs::create_dir_all(&source).unwrap();
        let manifest = Manifest {
            schema: MANIFEST_SCHEMA.into(),
            kind: SnapshotKind::Original,
            layout_id: None,
            classification: None,
            image_name: None,
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
    fn import_serial_reads_yaml_first() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("incoming");
        fs::create_dir_all(&source).unwrap();
        let mut manifest = Manifest {
            schema: MANIFEST_SCHEMA.into(),
            kind: SnapshotKind::Original,
            layout_id: None,
            classification: None,
            image_name: None,
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
            source.join("MANIFEST.yaml"),
            noyalib::to_string(&manifest).unwrap(),
        )
        .unwrap();
        manifest.factory_serial = "FROMJSON".into();
        fs::write(
            source.join("MANIFEST.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(
            factory_serial_for_import(&source, "").unwrap(),
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
