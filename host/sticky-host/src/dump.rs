//! Split a flash dump into bootloader, table, and named partitions.

use sha2::{Digest, Sha256};

use crate::partitions::{
    boot_slot_from_otadata, extract_app_desc, parse_partitions_in_dump, Partition, BOOTLOADER_LEN,
    PARTITION_TABLE_LEN, PARTITION_TABLE_OFFSET,
};
use crate::Error;

/// SHA-256 hex of a byte slice.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Named slices taken from a dump.
#[derive(Debug)]
pub struct SplitImage {
    /// Parsed table.
    pub partitions: Vec<Partition>,
    /// `0x0` .. table.
    pub bootloader: Vec<u8>,
    /// Table bytes.
    pub table: Vec<u8>,
    /// Each partition's payload.
    pub parts: Vec<(Partition, Vec<u8>)>,
    /// OTA boot slot if `otadata` exists.
    pub boot_slot: Option<String>,
    /// `app0` descriptor if present.
    pub app0_desc: Option<crate::partitions::AppDesc>,
}

/// Split a dump that is at least large enough to cover every partition.
pub fn split_image(dump: &[u8]) -> Result<SplitImage, Error> {
    let partitions = parse_partitions_in_dump(dump)?;
    for part in &partitions {
        let end = part
            .offset
            .checked_add(part.size)
            .ok_or_else(|| Error::PartitionTable("offset overflow".into()))?
            as usize;
        if dump.len() < end {
            return Err(Error::PartitionTable(format!(
                "dump ends before partition {}",
                part.label
            )));
        }
    }
    if dump.len() < BOOTLOADER_LEN {
        return Err(Error::PartitionTable("dump shorter than bootloader".into()));
    }
    let table_end = (PARTITION_TABLE_OFFSET + PARTITION_TABLE_LEN).min(dump.len());
    let table = dump[PARTITION_TABLE_OFFSET..table_end].to_vec();
    let mut parts = Vec::new();
    let mut boot_slot = None;
    let mut app0_desc = None;
    for part in &partitions {
        let start = part.offset as usize;
        let end = start + part.size as usize;
        let data = dump[start..end].to_vec();
        if part.label == "otadata" {
            boot_slot = boot_slot_from_otadata(&data).map(str::to_string);
        }
        if part.label == "app0" {
            app0_desc = extract_app_desc(&data);
        }
        parts.push((part.clone(), data));
    }
    Ok(SplitImage {
        partitions,
        bootloader: dump[..BOOTLOADER_LEN].to_vec(),
        table,
        parts,
        boot_slot,
        app0_desc,
    })
}

/// Require a full-chip 32 MiB image.
pub fn require_full_dump(dump: &[u8]) -> Result<(), Error> {
    if dump.len() != crate::FLASH_SIZE {
        return Err(Error::DumpLength(dump.len()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::partitions::test_entry;

    #[test]
    fn splits_tiny_fixture() {
        let nvs_off = 0x9000u32;
        let nvs_size = 16u32;
        let mut dump = vec![0u8; (nvs_off + nvs_size) as usize];
        dump[PARTITION_TABLE_OFFSET..PARTITION_TABLE_OFFSET + 32]
            .copy_from_slice(&test_entry("nvs", 0x01, 0x02, nvs_off, nvs_size));
        dump[nvs_off as usize] = 0xAB;
        let split = split_image(&dump).unwrap();
        assert_eq!(split.parts[0].0.label, "nvs");
        assert_eq!(split.parts[0].1[0], 0xAB);
        assert_eq!(sha256_hex(&[0xAB]), sha256_hex(&split.parts[0].1[..1]));
    }
}
