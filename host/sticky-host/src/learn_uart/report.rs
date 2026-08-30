//! YAML report for a UART learn session.
//!
//! Factory `serial_number` is recorded so a unit can be compared later. The
//! file lives under gitignored `developer-data/uart-inspection-records/<serial>/`. Do
//! not commit it. Do not put MAC or CH343 USB serial in this document.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::GitRef;
use crate::learn_uart_impl::parse::{Accumulator, Heartbeat};
use crate::learn_uart_impl::stamp::utc_compact_stamp;
use crate::learn_uart_impl::steps::{catalog, UNKNOWN_LABEL};
use crate::original::Layout;

/// Document identity.
pub const SCHEMA: &str = "sticky-uart-learn/v1";

/// Well-known alias: last session that finished without aborting.
pub const LATEST_YAML_NAME: &str = "learn-uart-latest.yaml";
/// Sidecar UART log for [`LATEST_YAML_NAME`].
pub const LATEST_LOG_NAME: &str = "learn-uart-latest.uart.log";

/// UART / operator outcome for one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Matching UART line seen this session.
    Observed,
    /// Wait ended without a match.
    Timeout,
    /// Not attempted (skip flag, consent, or operator said they did not try).
    Skipped,
    /// `--unattended-only` or not applicable.
    NotApplicable,
}

/// One human step in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanStep {
    /// UART evidence.
    pub status: Status,
    /// After timeout: whether they said they performed the action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_says_tried: Option<bool>,
    /// Why it was skipped, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    /// Extra UART notes (pose tokens, contact count).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Operator name for a key (`unknown` if they left it blank).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_label: Option<String>,
    /// Short note when the enclosure mapping is still unclear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_note: Option<String>,
}

/// GPIO key as a human would describe it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonMapEntry {
    /// ESP32-S3 GPIO.
    pub gpio: u8,
    /// UART edge token (`btn 4`).
    pub uart_token: String,
    /// Vendor / pin-map name. Not a silkscreen guarantee.
    pub firmware_claim: String,
    /// Where to look on the enclosure.
    pub enclosure_hint: String,
    /// Operator name, or `unknown`.
    pub human_label: String,
    /// Optional short note when the mapping is still unclear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_note: Option<String>,
    /// UART wait outcome for this key.
    pub uart_status: Status,
}

/// Consent captured before timed waits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Briefing {
    /// Wall-clock estimate we told the operator.
    pub expected_minutes: u32,
    /// Whether they said they can stay for that whole window.
    pub present_for_full_session: bool,
    /// USB unplug / disruptive handling allowed.
    pub noisy_ok: bool,
    /// Working MicroSD in hand.
    pub microsd_handy: bool,
    /// Can lift and rotate the enclosure (USB cable slack).
    #[serde(default)]
    pub free_to_move: bool,
    /// Both hands free to handle the board.
    #[serde(default)]
    pub both_hands_free: bool,
    /// Can see this terminal while holding the board.
    #[serde(default)]
    pub terminal_in_view: bool,
}

/// Whole session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// Schema id.
    pub schema: String,
    /// UTC stamp (`YYYYMMDDThhmmssZ`).
    pub captured_at: String,
    /// Factory UART `serial_number` (same as `developer-data/uart-inspection-records/<serial>/`).
    pub factory_serial: String,
    /// No human steps.
    pub unattended_only: bool,
    /// `--skip` tokens as given.
    pub skipped_by_flag: Vec<String>,
    /// `--only` tokens as given (empty means the full catalog).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
    /// `true` only if this process finished the planned steps (timeouts and
    /// skips count). Crash / Ctrl-C before this file is written does **not**
    /// publish [`LATEST_YAML_NAME`].
    #[serde(default)]
    pub complete: bool,
    /// Host package that wrote this file (`unknown` if git was missing).
    #[serde(default, alias = "xtask_git")]
    pub package_git: String,
    /// Host working tree was dirty at compile time.
    #[serde(default, alias = "xtask_git_dirty")]
    pub package_git_dirty: bool,
    /// Firmware UART `git=` line, when seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_git: Option<String>,
    /// Firmware `dirty=` bit, when a git line was seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_git_dirty: Option<bool>,
    /// Pre-session questions.
    pub briefing: Briefing,
    /// At least one heartbeat.
    pub uart0_heartbeat: Status,
    /// Address hex → ack/nak/missing.
    pub i2c: BTreeMap<String, String>,
    /// DeviceType line.
    pub gauge_device_type: bool,
    /// Last heartbeat snapshot, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotYaml>,
    /// Operator steps.
    pub human: BTreeMap<String, HumanStep>,
    /// GPIO → human name (and optional note) for the three keys.
    pub button_map: BTreeMap<String, ButtonMapEntry>,
    /// Highest `contacts=` this session.
    pub gt911_contacts_max: u8,
    /// Highest GT911 status-register byte from `gt911 st=0xNN` this session.
    #[serde(default)]
    pub gt911_status_max: u8,
    /// Last `gt911 int=` this session, if any (`0` or `1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gt911_int: Option<u8>,
    /// INT was seen both low and high this session.
    #[serde(default)]
    pub gt911_int_changed: bool,
    /// NYC ids this session must not pretend to close.
    pub nyc_still_open: Vec<String>,
}

/// Last heartbeat, YAML-friendly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotYaml {
    /// GPIO9.
    pub vbus: u8,
    /// GPIO7.
    pub gpio7: u8,
    /// GPIO40.
    pub gpio40: u8,
    /// GPIO11.
    pub sd_cd: u8,
    /// Percent.
    pub soc_pct: u8,
    /// Millivolts.
    pub voltage_mv: u16,
    /// Milliamperes.
    pub current_ma: i16,
    /// Pose token.
    pub imu: String,
}

impl From<&Heartbeat> for SnapshotYaml {
    fn from(hb: &Heartbeat) -> Self {
        Self {
            vbus: u8::from(hb.vbus),
            gpio7: u8::from(hb.gpio7),
            gpio40: u8::from(hb.gpio40),
            sd_cd: u8::from(hb.sd_cd),
            soc_pct: hb.soc_pct,
            voltage_mv: hb.voltage_mv,
            current_ma: hb.current_ma,
            imu: hb.imu.clone(),
        }
    }
}

const WATCHED_ADDRS: [u8; 5] = [0x14, 0x44, 0x51, 0x55, 0x6a];

/// NYC rows UART cannot close.
#[must_use]
pub fn nyc_still_open() -> Vec<String> {
    ["nyc-gauge-profile"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// Identity and flags that are not UART-derived.
pub struct ReportStamp {
    /// Factory UART `serial_number`.
    pub factory_serial: String,
    /// UTC stamp (`YYYYMMDDThhmmssZ`).
    pub captured_at: String,
    /// `--unattended-only` (or operator not present).
    pub unattended_only: bool,
    /// `--skip` tokens as given.
    pub skipped_by_flag: Vec<String>,
    /// `--only` tokens as given.
    pub only: Vec<String>,
    /// Session reached a planned end.
    pub complete: bool,
    /// Host package git.
    pub package_git: GitRef,
}

/// Build the document from the UART accumulator and per-step outcomes.
#[must_use]
pub fn assemble(
    acc: &Accumulator,
    briefing: Briefing,
    human: BTreeMap<String, HumanStep>,
    stamp: ReportStamp,
) -> Report {
    let mut i2c = BTreeMap::new();
    for addr in WATCHED_ADDRS {
        let key = format!("{addr:#04x}");
        let val = match acc.acks.get(&addr) {
            Some(true) => "ack",
            Some(false) => "nak",
            None => "not_seen",
        };
        i2c.insert(key, val.to_string());
    }
    let button_map = button_map_from(&human);
    let (firmware_git, firmware_git_dirty) = match &acc.firmware_git {
        Some((hash, dirty)) => (Some(hash.clone()), Some(*dirty)),
        None => (None, None),
    };
    let mut report = Report {
        schema: SCHEMA.into(),
        captured_at: stamp.captured_at,
        factory_serial: stamp.factory_serial,
        unattended_only: stamp.unattended_only,
        skipped_by_flag: stamp.skipped_by_flag,
        only: stamp.only,
        complete: stamp.complete,
        package_git: stamp.package_git.hash,
        package_git_dirty: stamp.package_git.dirty,
        firmware_git,
        firmware_git_dirty,
        briefing,
        uart0_heartbeat: if acc.heartbeats > 0 {
            Status::Observed
        } else {
            Status::Timeout
        },
        i2c,
        gauge_device_type: acc.gauge_device_type,
        snapshot: acc.heartbeat.as_ref().map(SnapshotYaml::from),
        human,
        button_map,
        gt911_contacts_max: acc.contacts_max,
        gt911_status_max: acc.gt911_status_max,
        gt911_int: acc.gt911_int.map(u8::from),
        gt911_int_changed: acc.gt911_int_changed,
        nyc_still_open: nyc_still_open(),
    };
    report
        .human
        .entry("gpio7_edges".into())
        .or_insert_with(|| gpio7_from_acc(acc));
    report
}

fn gpio7_from_acc(acc: &Accumulator) -> HumanStep {
    match acc.gpio7_edge {
        Some((from, to)) => HumanStep {
            status: Status::Observed,
            operator_says_tried: None,
            skip_reason: None,
            notes: Some(format!("{from} -> {to}")),
            human_label: None,
            operator_note: None,
        },
        None => HumanStep {
            status: Status::Observed,
            operator_says_tried: None,
            skip_reason: None,
            notes: Some("no_edge_this_session".into()),
            human_label: None,
            operator_note: None,
        },
    }
}

/// GPIO keys from the catalog, filled from operator answers when present.
#[must_use]
pub fn button_map_from(human: &BTreeMap<String, HumanStep>) -> BTreeMap<String, ButtonMapEntry> {
    let mut map = BTreeMap::new();
    for spec in catalog() {
        let Some(button) = spec.button else {
            continue;
        };
        let step = human.get(spec.yaml_key);
        map.insert(
            format!("gpio{}", button.gpio),
            ButtonMapEntry {
                gpio: button.gpio,
                uart_token: format!("btn {}", button.gpio),
                firmware_claim: button.firmware_claim.into(),
                enclosure_hint: button.enclosure_hint.into(),
                human_label: step
                    .and_then(|s| s.human_label.clone())
                    .unwrap_or_else(|| UNKNOWN_LABEL.into()),
                operator_note: step.and_then(|s| s.operator_note.clone()),
                uart_status: step.map(|s| s.status).unwrap_or(Status::Skipped),
            },
        );
    }
    map
}

/// Canonical YAML path: `developer-data/uart-inspection-records/<serial>/<stamp>.yaml`.
#[must_use]
pub fn default_report_path(layout: &Layout, factory_serial: &str, stamp: &str) -> PathBuf {
    layout
        .learn_uart_dir(factory_serial)
        .join(format!("{stamp}.yaml"))
}

/// Sidecar next to the YAML: every device UART line plus host events, timestamped.
#[must_use]
pub fn uart_log_path(yaml: &Path) -> PathBuf {
    yaml.with_extension("uart.log")
}

/// If `path` exists, append `-2`, `-3`, … before the extension.
#[must_use]
pub fn unique_report_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("learn-uart");
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for n in 2u32.. {
        let candidate = parent.join(format!("{stem}-{n}.yaml"));
        if !candidate.exists() {
            return candidate;
        }
    }
    path
}

/// Newest trusted report: [`LATEST_YAML_NAME`] when `complete`, else the newest
/// complete stamp YAML. Incomplete files are ignored.
pub fn latest_report_path(dir: &Path) -> Result<PathBuf, crate::Error> {
    let alias = dir.join(LATEST_YAML_NAME);
    if alias.is_file() {
        if let Ok(report) = load_report(&alias) {
            if report.complete {
                return Ok(alias);
            }
        }
    }
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("yaml"))
            .filter(|path| path.file_name().and_then(|n| n.to_str()) != Some(LATEST_YAML_NAME))
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(crate::Error::MissingLearnReport);
        }
        Err(error) => return Err(error.into()),
    };
    files.sort();
    files.reverse();
    for path in files {
        if let Ok(report) = load_report(&path) {
            if report.complete {
                return Ok(path);
            }
        }
    }
    Err(crate::Error::MissingLearnReport)
}

/// Copy a completed stamp YAML (and its UART log) to the well-known latest names.
pub fn publish_latest(yaml: &Path) -> Result<(), crate::Error> {
    let Some(dir) = yaml.parent() else {
        return Ok(());
    };
    let latest_yaml = dir.join(LATEST_YAML_NAME);
    if latest_yaml == yaml {
        return Ok(());
    }
    fs_copy(yaml, &latest_yaml)?;
    let log = uart_log_path(yaml);
    if log.is_file() {
        fs_copy(&log, &dir.join(LATEST_LOG_NAME))?;
    }
    Ok(())
}

fn fs_copy(from: &Path, to: &Path) -> Result<(), crate::Error> {
    std::fs::copy(from, to)?;
    Ok(())
}

/// Parse a learn-uart YAML file.
pub fn load_report(path: &Path) -> Result<Report, crate::Error> {
    let text = std::fs::read_to_string(path)?;
    noyalib::from_str(&text).map_err(|error| crate::Error::Yaml(error.to_string()))
}

/// Unix time used for `captured_at` and the filename.
#[must_use]
pub fn now_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    utc_compact_stamp(secs)
}

/// Empty skipped human map for `--unattended-only`.
#[must_use]
pub fn unattended_human() -> BTreeMap<String, HumanStep> {
    let mut map = BTreeMap::new();
    for spec in catalog() {
        let (human_label, operator_note) = if spec.button.is_some() {
            (Some(UNKNOWN_LABEL.into()), None)
        } else {
            (None, None)
        };
        map.insert(
            spec.yaml_key.to_string(),
            HumanStep {
                status: Status::NotApplicable,
                operator_says_tried: None,
                skip_reason: Some("unattended_only".into()),
                notes: None,
                human_label,
                operator_note,
            },
        );
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitRef;
    use crate::learn_uart_impl::parse::Heartbeat;

    fn test_stamp(complete: bool) -> ReportStamp {
        ReportStamp {
            factory_serial: "TESTFACTORY001".into(),
            captured_at: "20260828T161900Z".into(),
            unattended_only: true,
            skipped_by_flag: vec![],
            only: vec![],
            complete,
            package_git: GitRef {
                hash: "abc".into(),
                dirty: false,
            },
        }
    }

    fn test_briefing() -> Briefing {
        Briefing {
            expected_minutes: 8,
            present_for_full_session: true,
            noisy_ok: false,
            microsd_handy: false,
            free_to_move: false,
            both_hands_free: false,
            terminal_in_view: false,
        }
    }

    #[test]
    fn yaml_has_schema_and_no_serial_keys() {
        let acc = Accumulator {
            heartbeat: Some(Heartbeat {
                t_s: 1,
                vbus: true,
                gpio7: false,
                gpio40: true,
                sd_cd: true,
                soc_pct: 100,
                voltage_mv: 4195,
                current_ma: 0,
                imu: "FaceUp".into(),
            }),
            heartbeats: 1,
            acks: std::collections::BTreeMap::from([(0x14, true)]),
            firmware_git: Some(("deadbeef".into(), true)),
            ..Accumulator::default()
        };
        let report = assemble(&acc, test_briefing(), unattended_human(), test_stamp(true));
        let yaml = noyalib::to_string(&report).expect("yaml");
        assert!(yaml.contains("schema: sticky-uart-learn/v1"));
        assert!(yaml.contains("factory_serial: TESTFACTORY001"));
        assert!(yaml.contains("complete: true"));
        assert!(yaml.contains("package_git: abc"));
        assert!(yaml.contains("firmware_git: deadbeef"));
        assert!(yaml.contains("uart0_heartbeat: observed"));
        assert!(yaml.contains("button_map:"));
        assert!(yaml.contains("gpio4:"));
        assert!(yaml.contains("AI / OK / power"));
        assert!(yaml.contains("gpio7_edges"));
        assert!(yaml.contains("no_edge_this_session"));
        assert!(!yaml.to_ascii_lowercase().contains("mac"));
        assert!(!yaml.contains("usb_serial"));
        let round: Report = noyalib::from_str(&yaml).expect("roundtrip");
        assert_eq!(round.factory_serial, "TESTFACTORY001");
        assert!(round.complete);
        assert_eq!(round.firmware_git.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn latest_path_ignores_incomplete_and_prefers_alias() {
        let dir = tempfile::tempdir().unwrap();
        let incomplete = assemble(
            &Accumulator::default(),
            test_briefing(),
            unattended_human(),
            ReportStamp {
                captured_at: "20260828T000000Z".into(),
                complete: false,
                ..test_stamp(false)
            },
        );
        let complete = assemble(
            &Accumulator::default(),
            test_briefing(),
            unattended_human(),
            ReportStamp {
                captured_at: "20260828T010000Z".into(),
                complete: true,
                ..test_stamp(true)
            },
        );
        let inc_path = dir.path().join("20260828T000000Z.yaml");
        let ok_path = dir.path().join("20260828T010000Z.yaml");
        std::fs::write(&inc_path, noyalib::to_string(&incomplete).unwrap()).unwrap();
        std::fs::write(&ok_path, noyalib::to_string(&complete).unwrap()).unwrap();
        assert_eq!(latest_report_path(dir.path()).unwrap(), ok_path);
        publish_latest(&ok_path).unwrap();
        assert_eq!(
            latest_report_path(dir.path()).unwrap(),
            dir.path().join(LATEST_YAML_NAME)
        );
        let loaded = load_report(&dir.path().join(LATEST_YAML_NAME)).unwrap();
        assert!(loaded.complete);
    }

    #[test]
    fn default_path_is_under_original_serial() {
        let layout = Layout::from_repo_root("/repo");
        let path = default_report_path(&layout, "TESTFACTORY001", "20260828T161900Z");
        assert_eq!(
            path,
            PathBuf::from(
                "/repo/developer-data/uart-inspection-records/TESTFACTORY001/20260828T161900Z.yaml"
            )
        );
        assert_eq!(
            uart_log_path(&path),
            PathBuf::from(
                "/repo/developer-data/uart-inspection-records/TESTFACTORY001/20260828T161900Z.uart.log"
            )
        );
    }
}
