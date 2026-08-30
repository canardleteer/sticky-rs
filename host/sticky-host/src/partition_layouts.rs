//! Versioned catalog of known Sticky flash layouts.
//!
//! The bytes on the chip remain the source of truth. These tables exist so
//! classification and `flash-app` can name a layout (`factory-32mb-v1`) or
//! refuse an unknown / mismatched table. Later factory revisions become
//! `factory-32mb-v2`; do not overwrite v1.

use crate::partitions::Partition;

/// One row in a known layout (label + type/subtype + offset + size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutEntry {
    /// ESP-IDF label.
    pub label: &'static str,
    /// Raw type byte (`0x00` app, `0x01` data).
    pub type_id: u8,
    /// Raw subtype byte.
    pub subtype_id: u8,
    /// Byte offset in flash.
    pub offset: u32,
    /// Byte length.
    pub size: u32,
}

/// A named, append-only partition table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownPartitionLayout {
    /// Stable id (`factory-32mb-v1`).
    pub id: &'static str,
    /// Rows in table order.
    pub entries: &'static [LayoutEntry],
}

/// Observed factory 32 MiB table (nvs at `0x9000`, app0 at `0x90000`).
pub const FACTORY_32MB_V1: KnownPartitionLayout = KnownPartitionLayout {
    id: "factory-32mb-v1",
    entries: &[
        LayoutEntry {
            label: "nvs",
            type_id: 0x01,
            subtype_id: 0x02,
            offset: 0x9000,
            size: 0x7d000,
        },
        LayoutEntry {
            label: "otadata",
            type_id: 0x01,
            subtype_id: 0x00,
            offset: 0x86000,
            size: 0x2000,
        },
        LayoutEntry {
            label: "phy_init",
            type_id: 0x01,
            subtype_id: 0x01,
            offset: 0x88000,
            size: 0x1000,
        },
        LayoutEntry {
            label: "app0",
            type_id: 0x00,
            subtype_id: 0x10,
            offset: 0x90000,
            size: 0x600000,
        },
        LayoutEntry {
            label: "app1",
            type_id: 0x00,
            subtype_id: 0x11,
            offset: 0x690000,
            size: 0x600000,
        },
        LayoutEntry {
            label: "sys_storage",
            type_id: 0x01,
            subtype_id: 0x83,
            offset: 0xc90000,
            size: 0x930000,
        },
        LayoutEntry {
            label: "usr_storage",
            type_id: 0x01,
            subtype_id: 0x83,
            offset: 0x15c0000,
            size: 0xa00000,
        },
        LayoutEntry {
            label: "coredump",
            type_id: 0x01,
            subtype_id: 0x03,
            offset: 0x1fc0000,
            size: 0x40000,
        },
    ],
};

/// Layouts the host knows how to name. Append only.
pub const KNOWN_LAYOUTS: &[KnownPartitionLayout] = &[FACTORY_32MB_V1];

/// Result of matching a parsed dump table against [`KNOWN_LAYOUTS`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutMatch {
    /// Every row matches a catalog table.
    Known {
        /// Catalog id.
        id: &'static str,
    },
    /// Same labels as a catalog table, but type/subtype/offset/size differ.
    Mismatch {
        /// Catalog id whose labels matched.
        id: &'static str,
    },
    /// Not a known label sequence.
    Unknown,
}

impl LayoutMatch {
    /// Catalog id, or `None` when [`LayoutMatch::Unknown`].
    #[must_use]
    pub fn id(&self) -> Option<&'static str> {
        match self {
            Self::Known { id } | Self::Mismatch { id } => Some(id),
            Self::Unknown => None,
        }
    }

    /// Operator-facing token for logs and manifests.
    #[must_use]
    pub fn evidence_token(&self) -> String {
        match self {
            Self::Known { id } => (*id).to_string(),
            Self::Mismatch { id } => format!("mismatch:{id}"),
            Self::Unknown => "unknown".into(),
        }
    }

    /// `flash-app` may write `app0` without `--allow-unknown-layout`.
    #[must_use]
    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known { .. })
    }
}

/// Match a parsed table on label + type/subtype + offset + size.
#[must_use]
pub fn match_layout(parts: &[Partition]) -> LayoutMatch {
    for known in KNOWN_LAYOUTS {
        if exact_match(parts, known) {
            return LayoutMatch::Known { id: known.id };
        }
    }
    for known in KNOWN_LAYOUTS {
        if labels_eq(parts, known) {
            return LayoutMatch::Mismatch { id: known.id };
        }
    }
    LayoutMatch::Unknown
}

fn labels_eq(parts: &[Partition], known: &KnownPartitionLayout) -> bool {
    parts.len() == known.entries.len()
        && parts
            .iter()
            .zip(known.entries.iter())
            .all(|(part, entry)| part.label == entry.label)
}

fn exact_match(parts: &[Partition], known: &KnownPartitionLayout) -> bool {
    parts.len() == known.entries.len()
        && parts.iter().zip(known.entries.iter()).all(|(part, entry)| {
            part.label == entry.label
                && part.type_id == entry.type_id
                && part.subtype_id == entry.subtype_id
                && part.offset == entry.offset
                && part.size == entry.size
        })
}

/// Build [`Partition`] rows for a catalog table (tests and fixtures).
#[must_use]
pub fn partitions_from_layout(known: &KnownPartitionLayout) -> Vec<Partition> {
    known
        .entries
        .iter()
        .map(|entry| Partition {
            label: entry.label.into(),
            type_name: match entry.type_id {
                0x00 => "app".into(),
                0x01 => "data".into(),
                other => format!("0x{other:02x}"),
            },
            type_id: entry.type_id,
            subtype: match (entry.type_id, entry.subtype_id) {
                (0x00, 0x10) => "ota_0".into(),
                (0x00, 0x11) => "ota_1".into(),
                (0x01, 0x00) => "ota".into(),
                (0x01, 0x01) => "phy".into(),
                (0x01, 0x02) => "nvs".into(),
                (0x01, 0x03) => "coredump".into(),
                (0x01, 0x83) => "0x83".into(),
                (_, other) => format!("0x{other:02x}"),
            },
            subtype_id: entry.subtype_id,
            offset: entry.offset,
            size: entry.size,
            flags: 0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::partitions::{parse_partition_table, test_entry};

    #[test]
    fn factory_v1_matches_exact_rows() {
        let parts = partitions_from_layout(&FACTORY_32MB_V1);
        assert_eq!(
            match_layout(&parts),
            LayoutMatch::Known {
                id: "factory-32mb-v1"
            }
        );
    }

    #[test]
    fn same_labels_wrong_size_is_mismatch() {
        let mut parts = partitions_from_layout(&FACTORY_32MB_V1);
        parts[0].size = 16;
        assert_eq!(
            match_layout(&parts),
            LayoutMatch::Mismatch {
                id: "factory-32mb-v1"
            }
        );
    }

    #[test]
    fn nvs_only_is_unknown() {
        let parts = vec![Partition {
            label: "nvs".into(),
            type_name: "data".into(),
            type_id: 0x01,
            subtype: "nvs".into(),
            subtype_id: 0x02,
            offset: 0x9000,
            size: 16,
            flags: 0,
        }];
        assert_eq!(match_layout(&parts), LayoutMatch::Unknown);
    }

    #[test]
    fn table_bytes_round_trip_to_v1() {
        let mut table = Vec::new();
        for entry in FACTORY_32MB_V1.entries {
            table.extend_from_slice(&test_entry(
                entry.label,
                entry.type_id,
                entry.subtype_id,
                entry.offset,
                entry.size,
            ));
        }
        let parts = parse_partition_table(&table).unwrap();
        assert_eq!(
            match_layout(&parts),
            LayoutMatch::Known {
                id: "factory-32mb-v1"
            }
        );
    }
}
