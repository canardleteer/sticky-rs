//! Compare live flash to a write-once original; do not modify the original image.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::device::DeviceIo;
use crate::dump::sha256_hex;
use crate::original::{require_capture_backup, require_original_backup, Layout, OriginalBackup};
use crate::partitions::{BOOTLOADER_LEN, PARTITION_TABLE_LEN, PARTITION_TABLE_OFFSET};
use crate::Error;

/// One region compared during confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionDiff {
    /// `bootloader`, `partition-table`, or a partition label.
    pub name: String,
    /// Whether SHA-256 matches the original slice.
    pub matches: bool,
    /// Original SHA-256 hex.
    pub original_sha256: String,
    /// Live SHA-256 hex.
    pub live_sha256: String,
}

/// Confirm report written next to the original (gitignored with `backups/`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DivergenceReport {
    /// Factory serial of the original.
    pub factory_serial: String,
    /// Unix timestamp when the comparison ran.
    pub compared_at_unix: u64,
    /// Whether the full dump hash matches (only if live dump is 32 MiB).
    pub dump_sha256_match: Option<bool>,
    /// Per-region comparison.
    pub regions: Vec<RegionDiff>,
}

/// Read live flash, compare to the matching original, write a divergence JSON.
pub fn confirm_live<D: DeviceIo>(
    device: &D,
    layout: &Layout,
    port: &str,
    capture: Option<&str>,
) -> Result<DivergenceReport, Error> {
    let (_, board) = crate::detect::read_live_board(device, port)?;
    let snapshot = if let Some(slug) = capture {
        require_capture_backup(layout, &board.identity, slug)?
    } else {
        require_original_backup(layout, &board.identity)?
    };
    let live = crate::backup::read_full_flash(device, port)?;
    let original_dump = fs::read(snapshot.dir.join("flash-32mb.bin"))?;
    let report = compare_dumps(&snapshot, &original_dump, &live)?;
    write_report(&snapshot, &report)?;
    Ok(report)
}

/// Compare two dumps using the original's partition table.
pub fn compare_dumps(
    original: &OriginalBackup,
    original_dump: &[u8],
    live_dump: &[u8],
) -> Result<DivergenceReport, Error> {
    let mut regions = Vec::new();
    push_region(
        &mut regions,
        "bootloader",
        slice(original_dump, 0, BOOTLOADER_LEN),
        slice(live_dump, 0, BOOTLOADER_LEN),
    );
    push_region(
        &mut regions,
        "partition-table",
        slice(original_dump, PARTITION_TABLE_OFFSET, PARTITION_TABLE_LEN),
        slice(live_dump, PARTITION_TABLE_OFFSET, PARTITION_TABLE_LEN),
    );
    for part in &original.manifest.partitions {
        let len = part.size as usize;
        let off = part.offset as usize;
        push_region(
            &mut regions,
            &part.label,
            slice(original_dump, off, len),
            slice(live_dump, off, len),
        );
    }
    let dump_sha256_match =
        if original_dump.len() == crate::FLASH_SIZE && live_dump.len() == crate::FLASH_SIZE {
            Some(sha256_hex(original_dump) == sha256_hex(live_dump))
        } else {
            None
        };
    Ok(DivergenceReport {
        factory_serial: original.manifest.factory_serial.clone(),
        compared_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        dump_sha256_match,
        regions,
    })
}

fn slice(dump: &[u8], offset: usize, len: usize) -> &[u8] {
    let end = offset.saturating_add(len).min(dump.len());
    if offset >= dump.len() {
        &[]
    } else {
        &dump[offset..end]
    }
}

fn push_region(regions: &mut Vec<RegionDiff>, name: &str, original: &[u8], live: &[u8]) {
    let original_sha256 = sha256_hex(original);
    let live_sha256 = sha256_hex(live);
    regions.push(RegionDiff {
        name: name.to_string(),
        matches: original_sha256 == live_sha256 && original.len() == live.len(),
        original_sha256,
        live_sha256,
    });
}

fn write_report(original: &OriginalBackup, report: &DivergenceReport) -> Result<(), Error> {
    let name = format!("divergence-{}.yaml", report.compared_at_unix);
    let yaml = noyalib::to_string(report).map_err(|error| Error::Yaml(error.to_string()))?;
    fs::write(original.dir.join(name), yaml)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::persist_original;
    use crate::identity::{parse_board_info, test_mac};
    use crate::partitions::{test_entry, PARTITION_TABLE_OFFSET};

    fn tiny_dump(marker: u8) -> Vec<u8> {
        let nvs_off = 0x9000u32;
        let mut dump = vec![0u8; (nvs_off + 16) as usize];
        dump[PARTITION_TABLE_OFFSET..PARTITION_TABLE_OFFSET + 32]
            .copy_from_slice(&test_entry("nvs", 0x01, 0x02, nvs_off, 16));
        dump[nvs_off as usize] = marker;
        dump
    }

    #[test]
    fn confirm_lists_nvs_when_it_differs() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            backups_root: tmp.path().join("backups"),
        };
        let mac = test_mac();
        let info = format!(
            "Flash size: 32MB\nMAC address: {mac}\nSecure Boot: Disabled\nFlash Encryption: Disabled\n"
        );
        persist_original(
            &layout,
            "TESTFACTORY001",
            &tiny_dump(0x01),
            &parse_board_info(&info).unwrap(),
            "",
            &info,
            false,
        )
        .unwrap();
        let original =
            require_original_backup(&layout, &parse_board_info(&info).unwrap().identity).unwrap();
        let original_dump = tiny_dump(0x01);
        let live_dump = tiny_dump(0x02);
        let report = compare_dumps(&original, &original_dump, &live_dump).unwrap();
        let nvs = report.regions.iter().find(|r| r.name == "nvs").unwrap();
        assert!(!nvs.matches);
        let boot = report
            .regions
            .iter()
            .find(|r| r.name == "bootloader")
            .unwrap();
        assert!(boot.matches);
    }
}
