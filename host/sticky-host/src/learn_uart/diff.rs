//! Host-only comparison of two learn-uart YAML reports.

use std::collections::BTreeSet;
use std::path::Path;

use crate::identity::validate_factory_serial;
use crate::learn_uart_impl::report::{
    latest_report_path, load_report, ButtonMapEntry, HumanStep, Report, SnapshotYaml, Status,
};
use crate::original::Layout;
use crate::Error;

/// One compared field that differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDiff {
    /// Dotted path (`button_map.gpio4.human_label`).
    pub path: String,
    /// Left value.
    pub left: String,
    /// Right value.
    pub right: String,
}

/// Load `spec` as a YAML path or as a factory serial (latest report).
pub fn load_spec(layout: &Layout, spec: &str) -> Result<(String, Report), Error> {
    let as_path = Path::new(spec);
    if spec.contains('/') || spec.contains('\\') || as_path.is_file() {
        let report = load_report(as_path)?;
        return Ok((report.factory_serial.clone(), report));
    }
    validate_factory_serial(spec)?;
    let dir = layout.learn_uart_dir(spec);
    let path = latest_report_path(&dir)?;
    let report = load_report(&path)?;
    Ok((report.factory_serial.clone(), report))
}

/// Compare UART-learn fields that can differ between units (or over time).
#[must_use]
pub fn diff_reports(left: &Report, right: &Report) -> Vec<FieldDiff> {
    let mut out = Vec::new();
    push_ne(
        &mut out,
        "uart0_heartbeat",
        status(left.uart0_heartbeat),
        status(right.uart0_heartbeat),
    );
    push_ne(
        &mut out,
        "gauge_device_type",
        left.gauge_device_type.to_string(),
        right.gauge_device_type.to_string(),
    );
    push_ne(
        &mut out,
        "gt911_contacts_max",
        left.gt911_contacts_max.to_string(),
        right.gt911_contacts_max.to_string(),
    );
    push_ne(
        &mut out,
        "gt911_status_max",
        left.gt911_status_max.to_string(),
        right.gt911_status_max.to_string(),
    );
    push_ne(
        &mut out,
        "gt911_int",
        left.gt911_int
            .map(|v| v.to_string())
            .unwrap_or_else(|| "none".into()),
        right
            .gt911_int
            .map(|v| v.to_string())
            .unwrap_or_else(|| "none".into()),
    );
    push_ne(
        &mut out,
        "gt911_int_changed",
        left.gt911_int_changed.to_string(),
        right.gt911_int_changed.to_string(),
    );
    let addrs: BTreeSet<_> = left.i2c.keys().chain(right.i2c.keys()).cloned().collect();
    for addr in addrs {
        push_ne(
            &mut out,
            &format!("i2c.{addr}"),
            left.i2c
                .get(&addr)
                .cloned()
                .unwrap_or_else(|| "missing".into()),
            right
                .i2c
                .get(&addr)
                .cloned()
                .unwrap_or_else(|| "missing".into()),
        );
    }
    match (&left.snapshot, &right.snapshot) {
        (None, None) => {}
        (None, Some(_)) => push_ne(&mut out, "snapshot", "missing", "present"),
        (Some(_), None) => push_ne(&mut out, "snapshot", "present", "missing"),
        (Some(a), Some(b)) => diff_snapshot(&mut out, a, b),
    }
    let buttons: BTreeSet<_> = left
        .button_map
        .keys()
        .chain(right.button_map.keys())
        .cloned()
        .collect();
    for key in buttons {
        diff_button(
            &mut out,
            &key,
            left.button_map.get(&key),
            right.button_map.get(&key),
        );
    }
    let steps: BTreeSet<_> = left
        .human
        .keys()
        .chain(right.human.keys())
        .cloned()
        .collect();
    for key in steps {
        diff_human(&mut out, &key, left.human.get(&key), right.human.get(&key));
    }
    out
}

fn diff_snapshot(out: &mut Vec<FieldDiff>, left: &SnapshotYaml, right: &SnapshotYaml) {
    push_ne(
        out,
        "snapshot.vbus",
        left.vbus.to_string(),
        right.vbus.to_string(),
    );
    push_ne(
        out,
        "snapshot.gpio7",
        left.gpio7.to_string(),
        right.gpio7.to_string(),
    );
    push_ne(
        out,
        "snapshot.gpio40",
        left.gpio40.to_string(),
        right.gpio40.to_string(),
    );
    push_ne(
        out,
        "snapshot.sd_cd",
        left.sd_cd.to_string(),
        right.sd_cd.to_string(),
    );
    push_ne(out, "snapshot.imu", left.imu.clone(), right.imu.clone());
}

fn diff_button(
    out: &mut Vec<FieldDiff>,
    key: &str,
    left: Option<&ButtonMapEntry>,
    right: Option<&ButtonMapEntry>,
) {
    match (left, right) {
        (None, None) => {}
        (None, Some(_)) => push_ne(out, &format!("button_map.{key}"), "missing", "present"),
        (Some(_), None) => push_ne(out, &format!("button_map.{key}"), "present", "missing"),
        (Some(a), Some(b)) => {
            push_ne(
                out,
                &format!("button_map.{key}.human_label"),
                a.human_label.clone(),
                b.human_label.clone(),
            );
            push_ne(
                out,
                &format!("button_map.{key}.operator_note"),
                opt(&a.operator_note),
                opt(&b.operator_note),
            );
            push_ne(
                out,
                &format!("button_map.{key}.uart_status"),
                status(a.uart_status),
                status(b.uart_status),
            );
        }
    }
}

fn diff_human(
    out: &mut Vec<FieldDiff>,
    key: &str,
    left: Option<&HumanStep>,
    right: Option<&HumanStep>,
) {
    match (left, right) {
        (None, None) => {}
        (None, Some(_)) => push_ne(out, &format!("human.{key}"), "missing", "present"),
        (Some(_), None) => push_ne(out, &format!("human.{key}"), "present", "missing"),
        (Some(a), Some(b)) => {
            push_ne(
                out,
                &format!("human.{key}.status"),
                status(a.status),
                status(b.status),
            );
            push_ne(
                out,
                &format!("human.{key}.human_label"),
                opt(&a.human_label),
                opt(&b.human_label),
            );
            push_ne(
                out,
                &format!("human.{key}.operator_note"),
                opt(&a.operator_note),
                opt(&b.operator_note),
            );
        }
    }
}

fn status(value: Status) -> String {
    match value {
        Status::Observed => "observed".into(),
        Status::Timeout => "timeout".into(),
        Status::Skipped => "skipped".into(),
        Status::NotApplicable => "not_applicable".into(),
    }
}

fn opt(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "—".into())
}

fn push_ne(
    out: &mut Vec<FieldDiff>,
    path: &str,
    left: impl Into<String>,
    right: impl Into<String>,
) {
    let left = left.into();
    let right = right.into();
    if left != right {
        out.push(FieldDiff {
            path: path.into(),
            left,
            right,
        });
    }
}

/// Unit labels for a pasteable diff. Serials stay out unless `show_serials`.
#[must_use]
pub fn format_diff(
    left_serial: &str,
    right_serial: &str,
    left: &Report,
    right: &Report,
    diffs: &[FieldDiff],
    show_serials: bool,
) -> String {
    let (label_l, label_r) = if show_serials {
        (left_serial.to_string(), right_serial.to_string())
    } else if left_serial == right_serial {
        (
            "UNIT (serial redacted)".into(),
            "UNIT (serial redacted)".into(),
        )
    } else {
        ("UNIT_A".into(), "UNIT_B".into())
    };
    let mut out = String::new();
    out.push_str("learn-uart diff (UART evidence, not a schematic)\n");
    out.push_str(&format!(
        "  left:  {label_l}  captured_at {}\n",
        left.captured_at
    ));
    out.push_str(&format!(
        "  right: {label_r}  captured_at {}\n",
        right.captured_at
    ));
    if diffs.is_empty() {
        out.push_str("  no differences in compared fields\n");
        return out;
    }
    out.push_str(&format!("  {} difference(s):\n", diffs.len()));
    for diff in diffs {
        out.push_str(&format!(
            "    {}: {}  vs  {}\n",
            diff.path, diff.left, diff.right
        ));
    }
    out
}

/// Run the host-only comparison and print it.
pub fn run(
    layout: &Layout,
    left_spec: &str,
    right_spec: &str,
    show_serials: bool,
) -> Result<(), Error> {
    let (left_serial, left) = load_spec(layout, left_spec)?;
    let (right_serial, right) = load_spec(layout, right_spec)?;
    let diffs = diff_reports(&left, &right);
    print!(
        "{}",
        format_diff(
            &left_serial,
            &right_serial,
            &left,
            &right,
            &diffs,
            show_serials
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn_uart_impl::parse::{Accumulator, Heartbeat};
    use crate::learn_uart_impl::report::{assemble, unattended_human, Briefing};

    fn report(serial: &str, gpio7: u8, label: &str) -> Report {
        let acc = Accumulator {
            heartbeat: Some(Heartbeat {
                t_s: 1,
                vbus: true,
                gpio7: gpio7 != 0,
                gpio40: true,
                sd_cd: true,
                soc_pct: 100,
                voltage_mv: 4195,
                current_ma: 0,
                imu: "FaceUp".into(),
            }),
            heartbeats: 1,
            acks: std::collections::BTreeMap::from([(0x14, true)]),
            ..Accumulator::default()
        };
        let mut report = assemble(
            &acc,
            Briefing {
                expected_minutes: 8,
                present_for_full_session: true,
                noisy_ok: false,
                microsd_handy: false,
                free_to_move: false,
                both_hands_free: false,
                terminal_in_view: false,
            },
            unattended_human(),
            crate::learn_uart_impl::report::ReportStamp {
                factory_serial: serial.into(),
                captured_at: "20260828T000000Z".into(),
                unattended_only: true,
                skipped_by_flag: vec![],
                only: vec![],
                complete: true,
                package_git: crate::git::GitRef {
                    hash: "abc".into(),
                    dirty: false,
                },
            },
        );
        if let Some(entry) = report.button_map.get_mut("gpio4") {
            entry.human_label = label.into();
        }
        report
    }

    #[test]
    fn identical_reports_have_no_field_diffs() {
        let a = report("TESTFACTORY001", 0, "unknown");
        let b = report("TESTFACTORY002", 0, "unknown");
        assert!(diff_reports(&a, &b).is_empty());
    }

    #[test]
    fn gpio7_and_button_label_show_up() {
        let a = report("TESTFACTORY001", 0, "front key");
        let b = report("TESTFACTORY002", 1, "unknown");
        let diffs = diff_reports(&a, &b);
        let paths: Vec<_> = diffs.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"snapshot.gpio7"));
        assert!(paths.contains(&"button_map.gpio4.human_label"));
    }

    #[test]
    fn default_format_redacts_serials() {
        let a = report("TESTFACTORY001", 0, "front key");
        let b = report("TESTFACTORY002", 1, "unknown");
        let text = format_diff(
            "TESTFACTORY001",
            "TESTFACTORY002",
            &a,
            &b,
            &diff_reports(&a, &b),
            false,
        );
        assert!(text.contains("UNIT_A"));
        assert!(text.contains("UNIT_B"));
        assert!(!text.contains("TESTFACTORY001"));
        assert!(!text.contains("TESTFACTORY002"));
    }
}
