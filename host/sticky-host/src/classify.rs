//! Classify a full-chip dump before choosing `original/` vs a named capture.
//!
//! Factory images change. A stock-shaped UART `serial_number` is not proof
//! this is a factory original. Only an append-only catalog match is
//! `KnownFactory`. Everything else asks a human.

use crate::identity::parse_factory_serial;
use crate::partition_layouts::{match_layout, LayoutMatch};
use crate::partitions::{AppDesc, Partition};

/// In-tree custom images. Never store these under `original/`.
const IN_TREE_PROJECTS: &[&str] = &["simple-debug-fw", "embassy-debug-fw"];

/// One observed factory fingerprint. Append later revisions; do not overwrite.
const FACTORY_IMAGES: &[FactoryImage] = &[FactoryImage {
    catalog_id: "reterminal_template-1.1.0",
    project_name: "reterminal_template",
    version_prefix: "1.1.0",
    layout_id: "factory-32mb-v1",
}];

struct FactoryImage {
    catalog_id: &'static str,
    project_name: &'static str,
    version_prefix: &'static str,
    layout_id: &'static str,
}

/// Inputs after a dump (and optional UART sample).
#[derive(Debug, Clone)]
pub struct ClassifyInput<'a> {
    /// Parsed dump table.
    pub partitions: &'a [Partition],
    /// `app0` descriptor when present.
    pub app0_desc: Option<&'a AppDesc>,
    /// UART sample (may be empty).
    pub uart: &'a str,
}

/// What the host believes this dump is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    /// Catalog match: offer write-once `original/`.
    KnownFactory {
        /// In-repo fingerprint id.
        catalog_id: &'static str,
    },
    /// Stock-shaped, but not in the catalog. Ask (`--as-original` or `--name`).
    UncertainStock {
        /// Why it was not auto-original.
        reason: String,
    },
    /// Custom / damaged / no stock serial. Never `original/` unless a bug.
    NotFactory {
        /// Why this is not factory.
        reason: String,
    },
}

/// Printed evidence (project, version, layout, serial present/absent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyEvidence {
    /// `app0` project name when known.
    pub project: Option<String>,
    /// `app0` version when known.
    pub version: Option<String>,
    /// Layout token (`factory-32mb-v1`, `unknown`, `mismatch:…`).
    pub layout: String,
    /// Stock `key=serial_number` present and unique.
    pub serial_present: bool,
}

impl ClassifyEvidence {
    /// One-line operator summary.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "project={} version={} layout={} serial_number={}",
            self.project.as_deref().unwrap_or("(none)"),
            self.version.as_deref().unwrap_or("(none)"),
            self.layout,
            if self.serial_present {
                "present"
            } else {
                "absent"
            }
        )
    }
}

/// Result of [`classify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyResult {
    /// Bucket.
    pub class: Classification,
    /// What was observed.
    pub evidence: ClassifyEvidence,
}

impl ClassifyResult {
    /// Manifest `classification` field.
    #[must_use]
    pub fn manifest_tag(&self) -> String {
        match &self.class {
            Classification::KnownFactory { catalog_id } => {
                format!("known_factory:{catalog_id}")
            }
            Classification::UncertainStock { reason } => {
                format!("uncertain_stock:{reason}")
            }
            Classification::NotFactory { reason } => format!("not_factory:{reason}"),
        }
    }
}

/// Classify dump table + optional `app0` + optional stock UART.
#[must_use]
pub fn classify(input: ClassifyInput<'_>) -> ClassifyResult {
    let layout = match_layout(input.partitions);
    let serial_present = parse_factory_serial(input.uart).is_ok();
    let evidence = ClassifyEvidence {
        project: input.app0_desc.map(|d| d.project_name.clone()),
        version: input.app0_desc.map(|d| d.version.clone()),
        layout: layout.evidence_token(),
        serial_present,
    };

    if let LayoutMatch::Mismatch { id } = layout {
        return ClassifyResult {
            class: Classification::NotFactory {
                reason: format!("partition labels match {id} but offsets/sizes/types differ"),
            },
            evidence,
        };
    }

    if let Some(desc) = input.app0_desc {
        if is_in_tree_custom(desc) {
            return ClassifyResult {
                class: Classification::NotFactory {
                    reason: format!("app0 is {}", desc.project_name),
                },
                evidence,
            };
        }
        if let Some(catalog_id) = match_factory_image(desc, &layout) {
            return ClassifyResult {
                class: Classification::KnownFactory { catalog_id },
                evidence,
            };
        }
        if serial_present || looks_idf(desc) {
            return ClassifyResult {
                class: Classification::UncertainStock {
                    reason: format!(
                        "app0 {} {} is not in the factory catalog",
                        desc.project_name, desc.version
                    ),
                },
                evidence,
            };
        }
    }

    if input.uart.contains("git=") && !serial_present {
        return ClassifyResult {
            class: Classification::NotFactory {
                reason: "custom git= stamp, no stock serial_number".into(),
            },
            evidence,
        };
    }

    if serial_present {
        return ClassifyResult {
            class: Classification::UncertainStock {
                reason: "stock serial_number but app0 is not a known factory image".into(),
            },
            evidence,
        };
    }

    ClassifyResult {
        class: Classification::NotFactory {
            reason: "no stock serial_number and app0 is not a known factory image".into(),
        },
        evidence,
    }
}

fn is_in_tree_custom(desc: &AppDesc) -> bool {
    IN_TREE_PROJECTS
        .iter()
        .any(|name| desc.project_name == *name)
}

fn looks_idf(desc: &AppDesc) -> bool {
    !desc.idf_ver.is_empty()
}

fn match_factory_image(desc: &AppDesc, layout: &LayoutMatch) -> Option<&'static str> {
    let LayoutMatch::Known { id: layout_id } = layout else {
        return None;
    };
    FACTORY_IMAGES.iter().find_map(|image| {
        (image.layout_id == *layout_id
            && desc.project_name == image.project_name
            && desc.version.starts_with(image.version_prefix))
        .then_some(image.catalog_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::partition_layouts::{partitions_from_layout, FACTORY_32MB_V1};

    fn stock_uart() -> &'static str {
        "key=serial_number value=TESTFACTORY001\n"
    }

    fn factory_desc() -> AppDesc {
        AppDesc {
            version: "1.1.0".into(),
            project_name: "reterminal_template".into(),
            idf_ver: "v5.4-dirty".into(),
        }
    }

    fn custom_desc(name: &str) -> AppDesc {
        AppDesc {
            version: "0.1.0".into(),
            project_name: name.into(),
            idf_ver: String::new(),
        }
    }

    #[test]
    fn known_factory_reterminal_template() {
        let parts = partitions_from_layout(&FACTORY_32MB_V1);
        let desc = factory_desc();
        let result = classify(ClassifyInput {
            partitions: &parts,
            app0_desc: Some(&desc),
            uart: stock_uart(),
        });
        assert_eq!(
            result.class,
            Classification::KnownFactory {
                catalog_id: "reterminal_template-1.1.0"
            }
        );
        assert!(result.evidence.serial_present);
        assert_eq!(result.evidence.layout, "factory-32mb-v1");
        assert_eq!(
            result.manifest_tag(),
            "known_factory:reterminal_template-1.1.0"
        );
    }

    #[test]
    fn stock_serial_unknown_app_is_uncertain() {
        let parts = partitions_from_layout(&FACTORY_32MB_V1);
        let desc = AppDesc {
            version: "2.0.0".into(),
            project_name: "reterminal_template".into(),
            idf_ver: "v5.5".into(),
        };
        let result = classify(ClassifyInput {
            partitions: &parts,
            app0_desc: Some(&desc),
            uart: stock_uart(),
        });
        assert!(matches!(
            result.class,
            Classification::UncertainStock { .. }
        ));
    }

    #[test]
    fn simple_debug_is_not_factory() {
        let parts = partitions_from_layout(&FACTORY_32MB_V1);
        let desc = custom_desc("simple-debug-fw");
        let result = classify(ClassifyInput {
            partitions: &parts,
            app0_desc: Some(&desc),
            uart: "git=abc dirty=0\n",
        });
        assert!(matches!(result.class, Classification::NotFactory { .. }));
        assert!(!result.evidence.serial_present);
    }

    #[test]
    fn embassy_debug_is_not_factory() {
        let parts = partitions_from_layout(&FACTORY_32MB_V1);
        let desc = custom_desc("embassy-debug-fw");
        let result = classify(ClassifyInput {
            partitions: &parts,
            app0_desc: Some(&desc),
            uart: "",
        });
        assert!(matches!(result.class, Classification::NotFactory { .. }));
    }

    #[test]
    fn git_stamp_without_serial_is_not_factory() {
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
        let result = classify(ClassifyInput {
            partitions: &parts,
            app0_desc: None,
            uart: "simple-debug: git=deadbeef dirty=0\n",
        });
        assert!(matches!(result.class, Classification::NotFactory { .. }));
    }

    #[test]
    fn mismatch_table_is_not_factory() {
        let mut parts = partitions_from_layout(&FACTORY_32MB_V1);
        parts[0].size = 16;
        let desc = factory_desc();
        let result = classify(ClassifyInput {
            partitions: &parts,
            app0_desc: Some(&desc),
            uart: stock_uart(),
        });
        assert!(matches!(result.class, Classification::NotFactory { .. }));
        assert!(result.evidence.layout.starts_with("mismatch:"));
    }
}
