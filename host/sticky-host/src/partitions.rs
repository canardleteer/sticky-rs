//! ESP-IDF partition table parsing (32-byte entries at `0x8000` in a dump).

use serde::{Deserialize, Serialize};

use crate::Error;

/// Offset of the partition table in a 32 MiB dump.
pub const PARTITION_TABLE_OFFSET: usize = 0x8000;
/// Bytes reserved for the table on this product.
pub const PARTITION_TABLE_LEN: usize = 0x1000;
/// Second-stage bootloader occupies `0x0` .. table.
pub const BOOTLOADER_LEN: usize = PARTITION_TABLE_OFFSET;

const ENTRY_MAGIC: u16 = 0x50AA;
const MD5_MAGIC: u16 = 0xEBEB;
const APP_DESC_MAGIC: u32 = 0xABCD_5432;

/// One partition table row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Partition {
    /// ESP-IDF label.
    pub label: String,
    /// `app` or `data` (or a raw type id).
    pub type_name: String,
    /// Raw type byte.
    pub type_id: u8,
    /// Subtype name (`nvs`, `ota_0`, …).
    pub subtype: String,
    /// Raw subtype byte.
    pub subtype_id: u8,
    /// Byte offset in flash.
    pub offset: u32,
    /// Byte length.
    pub size: u32,
    /// Flags field.
    pub flags: u32,
}

/// Application descriptor extracted from an app image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDesc {
    /// Version string.
    pub version: String,
    /// Project name.
    pub project_name: String,
    /// IDF version string.
    pub idf_ver: String,
}

/// Parse consecutive 32-byte entries (the table payload, not a whole dump).
pub fn parse_partition_table(table: &[u8]) -> Result<Vec<Partition>, Error> {
    let mut parts = Vec::new();
    let mut off = 0;
    while off + 32 <= table.len() {
        let magic = u16::from_le_bytes(table[off..off + 2].try_into().unwrap());
        if magic == MD5_MAGIC {
            break;
        }
        if magic != ENTRY_MAGIC {
            break;
        }
        let type_id = table[off + 2];
        let subtype_id = table[off + 3];
        let offset = u32::from_le_bytes(table[off + 4..off + 8].try_into().unwrap());
        let size = u32::from_le_bytes(table[off + 8..off + 12].try_into().unwrap());
        let label_bytes = &table[off + 12..off + 28];
        let end = label_bytes.iter().position(|&b| b == 0).unwrap_or(16);
        let label = String::from_utf8_lossy(&label_bytes[..end]).into_owned();
        let flags = u32::from_le_bytes(table[off + 28..off + 32].try_into().unwrap());
        if label.is_empty() {
            return Err(Error::PartitionTable("empty label".into()));
        }
        parts.push(Partition {
            label,
            type_name: type_name(type_id),
            type_id,
            subtype: subtype_name(type_id, subtype_id),
            subtype_id,
            offset,
            size,
            flags,
        });
        off += 32;
    }
    if parts.is_empty() {
        return Err(Error::PartitionTable("no entries".into()));
    }
    Ok(parts)
}

/// Parse the table at [`PARTITION_TABLE_OFFSET`] in a flash dump.
pub fn parse_partitions_in_dump(dump: &[u8]) -> Result<Vec<Partition>, Error> {
    if dump.len() < PARTITION_TABLE_OFFSET + 32 {
        return Err(Error::PartitionTable("dump shorter than table".into()));
    }
    let end = (PARTITION_TABLE_OFFSET + PARTITION_TABLE_LEN).min(dump.len());
    parse_partition_table(&dump[PARTITION_TABLE_OFFSET..end])
}

/// `partitions.csv` body matching the dump (not the repo firmware template).
#[must_use]
pub fn partitions_csv(parts: &[Partition]) -> String {
    let mut lines = vec!["# Name, Type, SubType, Offset, Size, Flags".to_string()];
    for part in parts {
        lines.push(format!(
            "{}, {}, {}, 0x{:x}, 0x{:x},",
            part.label, part.type_name, part.subtype, part.offset, part.size
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

/// ESP-IDF app descriptor in an app partition image, if present.
#[must_use]
pub fn extract_app_desc(image: &[u8]) -> Option<AppDesc> {
    if image.len() < 0x120 {
        return None;
    }
    let mut base = 0x20;
    let magic = u32::from_le_bytes(image[0x20..0x24].try_into().ok()?);
    if magic != APP_DESC_MAGIC {
        let needle = APP_DESC_MAGIC.to_le_bytes();
        let idx = image.windows(4).position(|w| w == needle)?;
        if idx + 0xB0 > image.len() {
            return None;
        }
        base = idx;
    }
    Some(AppDesc {
        version: cstr(&image[base + 16..base + 48]),
        project_name: cstr(&image[base + 48..base + 80]),
        idf_ver: cstr(&image[base + 112..base + 144]),
    })
}

/// OTA boot slot from the first 32 bytes of `otadata`.
#[must_use]
pub fn boot_slot_from_otadata(otadata: &[u8]) -> Option<&'static str> {
    if otadata.len() < 4 {
        return None;
    }
    let seq = u32::from_le_bytes(otadata[0..4].try_into().ok()?);
    if seq == 0xFFFF_FFFF {
        return Some("unset");
    }
    Some(if seq % 2 == 1 { "app0" } else { "app1" })
}

fn type_name(type_id: u8) -> String {
    match type_id {
        0x00 => "app".into(),
        0x01 => "data".into(),
        other => format!("0x{other:02x}"),
    }
}

fn subtype_name(type_id: u8, subtype_id: u8) -> String {
    if type_id == 0x00 {
        return match subtype_id {
            0x00 => "factory".into(),
            0x10 => "ota_0".into(),
            0x11 => "ota_1".into(),
            0x12 => "ota_2".into(),
            0x20 => "test".into(),
            other => format!("0x{other:02x}"),
        };
    }
    match subtype_id {
        0x00 => "ota".into(),
        0x01 => "phy".into(),
        0x02 => "nvs".into(),
        0x03 => "coredump".into(),
        0x04 => "nvs_keys".into(),
        0x80 => "esphttpd".into(),
        0x81 => "fat".into(),
        0x82 => "spiffs".into(),
        0x83 => "littlefs".into(),
        other => format!("0x{other:02x}"),
    }
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Build one 32-byte table entry (tests and fixtures).
#[cfg(test)]
pub fn test_entry(label: &str, type_id: u8, subtype_id: u8, offset: u32, size: u32) -> [u8; 32] {
    let mut entry = [0u8; 32];
    entry[0..2].copy_from_slice(&ENTRY_MAGIC.to_le_bytes());
    entry[2] = type_id;
    entry[3] = subtype_id;
    entry[4..8].copy_from_slice(&offset.to_le_bytes());
    entry[8..12].copy_from_slice(&size.to_le_bytes());
    let bytes = label.as_bytes();
    entry[12..12 + bytes.len()].copy_from_slice(bytes);
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nvs_and_app0() {
        let mut table = Vec::new();
        table.extend_from_slice(&test_entry("nvs", 0x01, 0x02, 0x9000, 0x100));
        table.extend_from_slice(&test_entry("app0", 0x00, 0x10, 0x90000, 0x200));
        table.extend_from_slice(&MD5_MAGIC.to_le_bytes());
        let parts = parse_partition_table(&table).unwrap();
        assert_eq!(parts[0].label, "nvs");
        assert_eq!(parts[0].subtype, "nvs");
        assert_eq!(parts[1].label, "app0");
        assert_eq!(parts[1].subtype, "ota_0");
    }

    #[test]
    fn dump_table_at_8000() {
        let mut dump = vec![0u8; PARTITION_TABLE_OFFSET + 64];
        dump[PARTITION_TABLE_OFFSET..PARTITION_TABLE_OFFSET + 32]
            .copy_from_slice(&test_entry("nvs", 0x01, 0x02, 0x9000, 0x10));
        let parts = parse_partitions_in_dump(&dump).unwrap();
        assert_eq!(parts[0].label, "nvs");
    }
}
