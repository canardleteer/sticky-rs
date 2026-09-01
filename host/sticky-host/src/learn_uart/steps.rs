//! Human UART steps: duration, attention, skip rules.

use crate::learn_uart_impl::parse::ParsedLine;

/// A skippable operator action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepId {
    /// GPIO4 AI/OK.
    ButtonsOk,
    /// GPIO5 up.
    ButtonsUp,
    /// GPIO6 down.
    ButtonsDown,
    /// USB-C unplug/replug (GPIO9).
    Vbus,
    /// Tilt or rotate the board.
    Imu,
    /// Accepted `--skip` token; not a human step (captured during tilt).
    Gpio7,
    /// Insert/remove MicroSD (do not mount).
    SdDetect,
    /// Finger on the panel (GT911 count).
    Gt911Contacts,
}

impl StepId {
    /// Clap / YAML token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ButtonsOk => "buttons_ok",
            Self::ButtonsUp => "buttons_up",
            Self::ButtonsDown => "buttons_down",
            Self::Vbus => "vbus",
            Self::Imu => "imu",
            Self::Gpio7 => "gpio7",
            Self::SdDetect => "sd_detect",
            Self::Gt911Contacts => "gt911_contacts",
        }
    }

    /// Parse a `--skip` token. `buttons` skips all three keys.
    #[must_use]
    pub fn from_skip_token(s: &str) -> Option<Vec<Self>> {
        match s {
            "buttons" => Some(vec![Self::ButtonsOk, Self::ButtonsUp, Self::ButtonsDown]),
            "buttons_ok" => Some(vec![Self::ButtonsOk]),
            "buttons_up" => Some(vec![Self::ButtonsUp]),
            "buttons_down" => Some(vec![Self::ButtonsDown]),
            "vbus" => Some(vec![Self::Vbus]),
            "imu" => Some(vec![Self::Imu]),
            "gpio7" => Some(vec![Self::Gpio7]),
            "sd" | "sd_detect" => Some(vec![Self::SdDetect]),
            "gt911" | "gt911_contacts" | "touch" => Some(vec![Self::Gt911Contacts]),
            _ => None,
        }
    }
}

/// What UART evidence closes the wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitFor {
    /// `btn N down`.
    ButtonDown(u8),
    /// Any edge on this net name.
    LevelEdge(&'static str),
    /// Heartbeat `imu=` different from the baseline (not `none`).
    ImuChange,
    /// `contacts=` greater than zero.
    ContactsNonZero,
}

/// Firmware name vs enclosure location for one key. UART still prints `btn N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonIdentity {
    /// ESP32-S3 GPIO (4, 5, or 6).
    pub gpio: u8,
    /// Vendor / pin-map name. Not a silkscreen guarantee.
    pub firmware_claim: &'static str,
    /// Where to look on the enclosure without assuming a printed label.
    pub enclosure_hint: &'static str,
}

/// One real-world step.
#[derive(Debug, Clone, Copy)]
pub struct StepSpec {
    /// Stable id.
    pub id: StepId,
    /// Shown in the YAML `human` map.
    pub yaml_key: &'static str,
    /// Short title for the CLI.
    pub title: &'static str,
    /// Action lines shown one per line (not a paragraph).
    pub do_lines: &'static [&'static str],
    /// What we record (shown after the action, not instead of it).
    pub capture_note: Option<&'static str>,
    /// Expected hands-on time if it works first try.
    pub expected_secs: u32,
    /// UART wait before we ask whether they tried.
    pub timeout_secs: u32,
    /// Needs undivided attention (easy to miss the window).
    pub full_attention: bool,
    /// Extra timing warning, if any.
    pub attention_note: Option<&'static str>,
    /// USB unplug / handling that can click, drop, or surprise.
    pub noisy: bool,
    /// Needs a card in hand. We never mount it.
    pub needs_microsd: bool,
    /// Needs lifting/rotating the enclosure (USB cable slack, not glued to the desk).
    pub needs_free_motion: bool,
    /// Needs two hands at once (hold the board and unplug, tilt, or handle a card).
    pub needs_both_hands: bool,
    /// When set, ask for a human label (and optional note) after the wait.
    pub button: Option<ButtonIdentity>,
    /// UART match.
    pub wait: WaitFor,
    /// Ask the operator to put the board still and snapshot pose before matching.
    /// USB unplug already tilts the enclosure; matching immediately would be accidental.
    pub snapshot_before_wait: bool,
    /// Wait for Enter before the timed wait (operator must be ready; do not start the clock on the card).
    pub ready_before_wait: bool,
    /// Timeout without a match is still a valid result.
    pub timeout_is_success: bool,
}

/// Stored when the operator leaves the human name blank.
pub const UNKNOWN_LABEL: &str = "unknown";

impl StepSpec {
    /// Log / YAML prose: the `do_lines` joined.
    #[must_use]
    pub fn instruction(&self) -> String {
        self.do_lines.join(" ")
    }
}

const HUMAN_LABEL_MAX: usize = 80;
const OPERATOR_NOTE_MAX: usize = 200;

/// All operator steps in recommended order. Titles are actions a human performs.
#[must_use]
pub fn catalog() -> &'static [StepSpec] {
    &[
        StepSpec {
            id: StepId::ButtonsOk,
            yaml_key: "buttons_ok",
            title: "Press the top right-edge button",
            do_lines: &[
                "Glass toward you, USB-C at the bottom.",
                "The three buttons are on the right edge — not on the glass, not the Reset pinhole.",
                "Press and hold the top button for about a third of a second.",
            ],
            capture_note: Some("We record the press, then ask what you would call this key."),
            expected_secs: 15,
            timeout_secs: 45,
            full_attention: true,
            attention_note: Some(
                "A very quick tap can be missed. Hold it long enough to feel the click.",
            ),
            noisy: false,
            needs_microsd: false,
            needs_free_motion: false,
            needs_both_hands: false,
            button: Some(ButtonIdentity {
                gpio: 4,
                firmware_claim: "AI / OK / power",
                enclosure_hint: "right edge, top of the three (Seeed: AI Voice Button); not the glass, not Reset",
            }),
            snapshot_before_wait: false,
            ready_before_wait: false,
            timeout_is_success: false,
            wait: WaitFor::ButtonDown(4),
        },
        StepSpec {
            id: StepId::ButtonsUp,
            yaml_key: "buttons_up",
            title: "Press the middle right-edge button",
            do_lines: &[
                "Same hold: glass toward you, USB-C at the bottom.",
                "Press and hold the middle button on the right edge for about a third of a second.",
            ],
            capture_note: Some("We record the press, then ask what you would call this key."),
            expected_secs: 15,
            timeout_secs: 45,
            full_attention: true,
            attention_note: None,
            noisy: false,
            needs_microsd: false,
            needs_free_motion: false,
            needs_both_hands: false,
            button: Some(ButtonIdentity {
                gpio: 5,
                firmware_claim: "Up / left",
                enclosure_hint: "right edge, middle of the three (Seeed: Page Up Button)",
            }),
            snapshot_before_wait: false,
            ready_before_wait: false,
            timeout_is_success: false,
            wait: WaitFor::ButtonDown(5),
        },
        StepSpec {
            id: StepId::ButtonsDown,
            yaml_key: "buttons_down",
            title: "Press the bottom right-edge button",
            do_lines: &[
                "Same hold: glass toward you, USB-C at the bottom.",
                "Press and hold the bottom button on the right edge for about a third of a second.",
            ],
            capture_note: Some("We record the press, then ask what you would call this key."),
            expected_secs: 15,
            timeout_secs: 45,
            full_attention: true,
            attention_note: None,
            noisy: false,
            needs_microsd: false,
            needs_free_motion: false,
            needs_both_hands: false,
            button: Some(ButtonIdentity {
                gpio: 6,
                firmware_claim: "Down / right",
                enclosure_hint: "right edge, bottom of the three (Seeed: Page Down Button)",
            }),
            snapshot_before_wait: false,
            ready_before_wait: false,
            timeout_is_success: false,
            wait: WaitFor::ButtonDown(6),
        },
        StepSpec {
            id: StepId::Vbus,
            yaml_key: "vbus_edge",
            title: "Unplug USB-C, then plug it back in",
            do_lines: &[
                "Unplug USB-C from the Sticky (bottom edge).",
                "This computer will go quiet — that is expected, not a crash.",
                "Count about one second, then plug the same cable back in.",
                "When we say the cable is back, you are done.",
            ],
            capture_note: Some(
                "We record that USB left and returned. You do not need to watch for extra messages.",
            ),
            expected_secs: 40,
            timeout_secs: 75,
            full_attention: true,
            attention_note: Some(
                "Wait until we say the cable is gone, then plug the same one back in.",
            ),
            noisy: true,
            needs_microsd: false,
            needs_free_motion: false,
            needs_both_hands: true,
            button: None,
            snapshot_before_wait: false,
            ready_before_wait: false,
            timeout_is_success: false,
            wait: WaitFor::LevelEdge("vbus"),
        },
        StepSpec {
            id: StepId::Imu,
            yaml_key: "imu_pose_change",
            title: "Tilt or rotate the board",
            do_lines: &[
                "Unplugging USB already moved it, and holding it always tilts it.",
                "First: set it still on the desk (glass up is fine), then press Enter.",
                "When we say READY: lift or rotate and hold about one second.",
                "You need cable slack. Magnets may snap to metal.",
            ],
            capture_note: Some(
                "We record orientation, and anything else that happens while you move it.",
            ),
            expected_secs: 25,
            timeout_secs: 60,
            full_attention: true,
            attention_note: Some(
                "Wait for READY before you move it. We will not ask you to type while tilting.",
            ),
            noisy: false,
            needs_microsd: false,
            needs_free_motion: true,
            needs_both_hands: true,
            button: None,
            snapshot_before_wait: true,
            ready_before_wait: false,
            timeout_is_success: false,
            wait: WaitFor::ImuChange,
        },
        StepSpec {
            id: StepId::SdDetect,
            yaml_key: "sd_detect_edge",
            title: "Insert or remove a MicroSD card",
            do_lines: &[
                "The slot is on the left edge.",
                "Insert a card, or remove one if it is already in.",
                "Do not format or mount it.",
            ],
            capture_note: Some("We only record whether a card is in the slot."),
            expected_secs: 25,
            timeout_secs: 60,
            full_attention: true,
            attention_note: Some("Have the card in hand before we start waiting."),
            noisy: false,
            needs_microsd: true,
            needs_free_motion: false,
            needs_both_hands: true,
            button: None,
            snapshot_before_wait: false,
            ready_before_wait: true,
            timeout_is_success: false,
            wait: WaitFor::LevelEdge("sd_cd"),
        },
        StepSpec {
            id: StepId::Gt911Contacts,
            yaml_key: "gt911_contacts",
            title: "Touch the glass",
            do_lines: &[
                "Put one finger on the glass, hold still, then lift.",
                "A second finger is optional. The screen does not need to redraw.",
            ],
            capture_note: Some("We record whether a finger is seen (count on lift can feel slow)."),
            expected_secs: 25,
            timeout_secs: 45,
            full_attention: true,
            attention_note: Some(
                "Do not touch the glass until we say so. Then put a finger down, hold, and lift.",
            ),
            noisy: false,
            needs_microsd: false,
            needs_free_motion: false,
            needs_both_hands: false,
            button: None,
            snapshot_before_wait: false,
            ready_before_wait: true,
            timeout_is_success: false,
            wait: WaitFor::ContactsNonZero,
        },
    ]
}

/// Unattended UART listen before human steps (seconds).
pub const UNATTENDED_SECS: u32 = 20;

/// Briefing / confirm questions (seconds, wall-clock guess).
pub const BRIEFING_SECS: u32 = 150;

/// Operator answers that drop whole classes of steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Consent {
    /// USB unplug / disruptive handling allowed.
    pub noisy_ok: bool,
    /// Working MicroSD in hand.
    pub microsd_handy: bool,
    /// Can lift and rotate the enclosure (cable slack).
    pub free_to_move: bool,
    /// Both hands free to handle the board.
    pub both_hands_free: bool,
}

impl Consent {
    /// Preview / “everything on the table”.
    pub const ALL: Self = Self {
        noisy_ok: true,
        microsd_handy: true,
        free_to_move: true,
        both_hands_free: true,
    };
}

/// Preamble before consent questions. Duration, full-attention windows, timeouts.
#[must_use]
pub fn format_session_briefing(preview: &[StepSpec]) -> String {
    use std::fmt::Write as _;
    let secs = expected_total_secs(preview, false);
    let minutes = secs.div_ceil(60);
    let mut out = String::new();
    let _ = writeln!(out);
    let _ = writeln!(out, "learn-uart");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "About {minutes} minute(s) if things work first try (~{secs}s)."
    );
    let _ = writeln!(
        out,
        "We listen ~{UNATTENDED_SECS}s with the board still, then these actions:"
    );
    let _ = writeln!(out);
    for step in preview {
        let _ = writeln!(out, "  - {}", step.title);
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "Enter accepts the default on the next questions.");
    if preview.iter().any(|s| s.noisy) {
        let _ = writeln!(
            out,
            "Unplugging USB-C disconnects this computer until you plug the same cable back in."
        );
    }
    if preview.iter().any(|s| s.needs_free_motion) {
        let _ = writeln!(
            out,
            "Tilting needs cable slack. We will not ask you to type while tilting."
        );
    }
    if preview.iter().any(|s| s.button.is_some()) {
        let _ = writeln!(
            out,
            "After each button we ask what you would call that key."
        );
    }
    if preview.iter().any(|s| s.needs_microsd) {
        let _ = writeln!(
            out,
            "MicroSD is insert or remove only. We will not mount it."
        );
    }
    let _ = writeln!(out, "Press s to skip a step. You do not need Enter.");
    let _ = writeln!(out);
    out
}

/// Prompt after a UART wait expires.
#[must_use]
pub fn format_timeout_prompt(timeout_secs: u32) -> String {
    format!(
        "We didn't see a response (waited {timeout_secs}s).\n\nDid you try this?  [y] tried  [n] did not  [r] retry  "
    )
}

/// Seconds we tell the operator this session will take.
#[must_use]
pub fn expected_total_secs(steps: &[StepSpec], unattended_only: bool) -> u32 {
    if unattended_only {
        return UNATTENDED_SECS + 30;
    }
    BRIEFING_SECS
        + UNATTENDED_SECS
        + steps
            .iter()
            .map(|s| s.expected_secs + 15 + if s.button.is_some() { 25 } else { 0 })
            .sum::<u32>()
}

/// Filter catalog by consent, `--skip`, and optional `--only`.
#[must_use]
pub fn select(
    consent: Consent,
    skip: &[StepId],
    only: &[StepId],
    unattended_only: bool,
) -> Vec<StepSpec> {
    if unattended_only {
        return Vec::new();
    }
    catalog()
        .iter()
        .copied()
        .filter(|step| {
            if !only.is_empty() && !only.contains(&step.id) {
                return false;
            }
            if skip.contains(&step.id) {
                return false;
            }
            if step.noisy && !consent.noisy_ok {
                return false;
            }
            if step.needs_microsd && !consent.microsd_handy {
                return false;
            }
            if step.needs_free_motion && !consent.free_to_move {
                return false;
            }
            if step.needs_both_hands && !consent.both_hands_free {
                return false;
            }
            true
        })
        .collect()
}

/// Whether `line` satisfies `wait` given the last heartbeat imu token.
#[must_use]
pub fn line_matches(wait: WaitFor, line: &ParsedLine, imu_baseline: Option<&str>) -> bool {
    match (wait, line) {
        (WaitFor::ButtonDown(want), ParsedLine::Button { gpio, down: true }) => *gpio == want,
        (WaitFor::LevelEdge(name), ParsedLine::Level { name: got, .. }) => got == name,
        (WaitFor::ImuChange, ParsedLine::Heartbeat(hb)) => {
            if hb.imu == "none" {
                return false;
            }
            match imu_baseline {
                None => false,
                Some(base) => hb.imu != base,
            }
        }
        (WaitFor::ContactsNonZero, ParsedLine::Contacts(n)) => *n > 0,
        _ => false,
    }
}

/// Reply after a UART timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutReply {
    /// Operator says they did the action; UART still missed it.
    Tried,
    /// Operator says they did not try.
    DidNotTry,
    /// Run the wait again.
    Retry,
}

/// Parse `y` / `n` / `retry` (and aliases).
#[must_use]
pub fn parse_timeout_reply(s: &str) -> Option<TimeoutReply> {
    match s.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" | "tried" => Some(TimeoutReply::Tried),
        "n" | "no" | "skip" | "s" => Some(TimeoutReply::DidNotTry),
        "r" | "retry" => Some(TimeoutReply::Retry),
        _ => None,
    }
}

/// Parse a yes/no briefing answer.
#[must_use]
pub fn parse_yes_no(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Some(true),
        "n" | "no" => Some(false),
        _ => None,
    }
}

fn collapse_and_cap(s: &str, max_chars: usize) -> Option<String> {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let mut out = String::new();
    for ch in collapsed.chars() {
        if out.chars().count() >= max_chars {
            break;
        }
        out.push(ch);
    }
    Some(out)
}

/// Human name for a key. Blank becomes [`UNKNOWN_LABEL`].
#[must_use]
pub fn parse_human_label(s: &str) -> String {
    collapse_and_cap(s, HUMAN_LABEL_MAX).unwrap_or_else(|| UNKNOWN_LABEL.into())
}

/// Optional short note. Blank is `None`.
#[must_use]
pub fn parse_optional_note(s: &str) -> Option<String> {
    collapse_and_cap(s, OPERATOR_NOTE_MAX)
}

/// Prompt that asks for a human name after a button wait.
#[must_use]
pub fn format_button_label_prompt(button: ButtonIdentity) -> String {
    format!(
        "What would you call this key?\n\n  {hint}\n\n  [Enter] unknown  ",
        hint = button.enclosure_hint,
    )
}

/// Prompt for a short note when the mapping is still unclear.
#[must_use]
pub fn format_button_note_prompt() -> &'static str {
    "Short note if still unsure\n\n  [Enter] skip  "
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn_uart_impl::parse::Heartbeat;

    fn hb(imu: &str) -> ParsedLine {
        ParsedLine::Heartbeat(Heartbeat {
            t_s: 1,
            vbus: true,
            gpio7: false,
            gpio40: true,
            sd_cd: true,
            soc_pct: 1,
            voltage_mv: 1,
            current_ma: 0,
            imu: imu.into(),
        })
    }

    #[test]
    fn skip_buttons_expands_to_three_keys() {
        assert_eq!(StepId::from_skip_token("buttons").unwrap().len(), 3);
    }

    #[test]
    fn no_microsd_drops_sd_step() {
        let steps = select(
            Consent {
                microsd_handy: false,
                ..Consent::ALL
            },
            &[],
            &[],
            false,
        );
        assert!(steps.iter().all(|s| s.id != StepId::SdDetect));
        assert!(steps.iter().any(|s| s.id == StepId::ButtonsOk));
    }

    #[test]
    fn noisy_not_ok_drops_vbus() {
        let steps = select(
            Consent {
                noisy_ok: false,
                ..Consent::ALL
            },
            &[],
            &[],
            false,
        );
        assert!(steps.iter().all(|s| s.id != StepId::Vbus));
        assert!(steps.iter().any(|s| s.id == StepId::SdDetect));
    }

    #[test]
    fn short_cable_drops_tilt_steps() {
        let steps = select(
            Consent {
                free_to_move: false,
                ..Consent::ALL
            },
            &[],
            &[],
            false,
        );
        assert!(steps.iter().all(|s| s.id != StepId::Imu));
        assert!(steps.iter().any(|s| s.id == StepId::ButtonsOk));
        assert!(steps.iter().any(|s| s.id == StepId::Gt911Contacts));
    }

    #[test]
    fn no_both_hands_keeps_desk_steps() {
        let steps = select(
            Consent {
                both_hands_free: false,
                ..Consent::ALL
            },
            &[],
            &[],
            false,
        );
        assert!(steps.iter().all(|s| !s.needs_both_hands));
        assert!(steps.iter().any(|s| s.id == StepId::ButtonsOk));
        assert!(steps.iter().any(|s| s.id == StepId::Gt911Contacts));
        assert!(steps.iter().all(|s| s.id != StepId::Vbus));
        assert!(steps.iter().all(|s| s.id != StepId::SdDetect));
    }

    #[test]
    fn unattended_selects_nothing() {
        assert!(select(Consent::ALL, &[], &[], true).is_empty());
    }

    #[test]
    fn only_touch_is_a_single_desk_step() {
        let steps = select(Consent::ALL, &[], &[StepId::Gt911Contacts], false);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, StepId::Gt911Contacts);
        let text = format_session_briefing(&steps);
        assert!(text.contains("Touch the glass"));
        assert!(!text.contains("Unplug USB-C"));
        assert!(!text.contains("Tilt or rotate"));
        assert!(!text.contains("what you would call that key"));
        assert!(!text.contains("MicroSD"));
    }

    #[test]
    fn imu_change_needs_a_different_token() {
        assert!(!line_matches(
            WaitFor::ImuChange,
            &hb("FaceUp"),
            Some("FaceUp")
        ));
        assert!(line_matches(
            WaitFor::ImuChange,
            &hb("Landscape0"),
            Some("FaceUp")
        ));
        assert!(!line_matches(
            WaitFor::ImuChange,
            &hb("none"),
            Some("FaceUp")
        ));
    }

    #[test]
    fn timeout_reply_aliases() {
        assert_eq!(parse_timeout_reply("Yes"), Some(TimeoutReply::Tried));
        assert_eq!(parse_timeout_reply("skip"), Some(TimeoutReply::DidNotTry));
        assert_eq!(parse_timeout_reply("s"), Some(TimeoutReply::DidNotTry));
        assert_eq!(parse_timeout_reply("retry"), Some(TimeoutReply::Retry));
        assert_eq!(parse_timeout_reply("maybe"), None);
    }

    #[test]
    fn blank_yes_no_is_none_so_the_prompt_can_use_a_default() {
        assert_eq!(parse_yes_no("  \n"), None);
        assert_eq!(parse_yes_no("y"), Some(true));
        assert_eq!(parse_yes_no("n"), Some(false));
    }

    #[test]
    fn total_secs_is_at_least_unattended() {
        assert!(expected_total_secs(&[], true) >= UNATTENDED_SECS);
        let all = catalog();
        assert!(expected_total_secs(all, false) > expected_total_secs(&[], true));
    }

    #[test]
    fn briefing_states_duration_attention_and_timeouts() {
        let text = format_session_briefing(catalog());
        assert!(text.contains("learn-uart"));
        assert!(text.contains("minute"));
        assert!(text.contains("Press the top right-edge button"));
        assert!(text.contains("Tilt or rotate the board"));
        assert!(text.contains("Touch the glass"));
        assert!(text.contains("Unplug USB-C"));
        assert!(text.contains("Enter accepts the default"));
        assert!(text.contains("Press s to skip"));
        assert!(text.contains("do not need Enter"));
        assert!(text.contains("cable slack"));
    }

    #[test]
    fn timeout_prompt_asks_tried_and_retry() {
        let text = format_timeout_prompt(45);
        assert!(text.contains("waited 45s"));
        assert!(text.contains("Did you try this?"));
        assert!(text.contains("[r] retry"));
    }

    #[test]
    fn blank_human_label_is_unknown() {
        assert_eq!(parse_human_label("  \n"), UNKNOWN_LABEL);
        assert_eq!(
            parse_human_label("front unmarked key"),
            "front unmarked key"
        );
    }

    #[test]
    fn optional_note_blank_is_none() {
        assert_eq!(parse_optional_note("   "), None);
        assert_eq!(
            parse_optional_note("  no AI print  on  this unit "),
            Some("no AI print on this unit".into())
        );
    }

    #[test]
    fn imu_snapshots_pose_after_usb_wiggle() {
        let imu = catalog().iter().find(|s| s.id == StepId::Imu).expect("imu");
        assert!(imu.snapshot_before_wait);
        assert!(!imu.timeout_is_success);
        assert!(imu
            .do_lines
            .iter()
            .any(|line| line.contains("set it still on the desk")));
    }

    #[test]
    fn catalog_titles_are_human_actions() {
        for step in catalog() {
            assert!(
                !step.title.contains("GPIO"),
                "title should be an action, got {:?}",
                step.title
            );
            assert!(!step.title.contains("GT911"), "{}", step.title);
            assert!(!step.title.contains("IMU"), "{}", step.title);
            assert!(step.capture_note.is_some(), "{}", step.title);
        }
        assert!(catalog().iter().all(|s| s.id != StepId::Gpio7));
    }

    #[test]
    fn gt911_is_touch_the_glass() {
        let touch = catalog()
            .iter()
            .find(|s| s.id == StepId::Gt911Contacts)
            .expect("touch");
        assert_eq!(touch.title, "Touch the glass");
        assert!(touch.ready_before_wait);
        assert!(!touch.timeout_is_success);
    }

    #[test]
    fn vbus_step_tells_a_human_to_unplug_and_replug() {
        let vbus = catalog()
            .iter()
            .find(|s| s.id == StepId::Vbus)
            .expect("vbus step");
        let text = vbus.instruction();
        assert!(text.contains("Unplug USB-C from the Sticky"));
        assert!(text.contains("This computer will go quiet"));
        assert!(text.contains("plug the same cable back in"));
        assert!(text.contains("When we say the cable is back"));
        assert!(vbus
            .attention_note
            .unwrap()
            .contains("Wait until we say the cable is gone"));
    }

    #[test]
    fn button_label_prompt_asks_what_you_would_call_it() {
        let btn = catalog()[0].button.expect("top button");
        let text = format_button_label_prompt(btn);
        assert!(text.contains("What would you call this key?"));
        assert!(text.contains("right edge, top of the three"));
        assert!(text.contains("[Enter] unknown"));
        assert!(!text.contains("btn 4"));
        assert!(!text.contains("GPIO"));
    }
}
