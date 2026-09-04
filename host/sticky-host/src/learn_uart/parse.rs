//! Parse `simple-debug:` UART lines. No identifiers.

use simple_debug::LOG_PREFIX;

/// One decoded firmware line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedLine {
    /// Heartbeat of raw levels.
    Heartbeat(Heartbeat),
    /// Active-low button went down or up.
    Button {
        /// GPIO 4, 5, or 6.
        gpio: u8,
        /// `true` is press (low).
        down: bool,
    },
    /// Raw net `name from -> to`.
    Level {
        /// `vbus`, `gpio7`, `gpio40`, or `sd_cd`.
        name: String,
        /// Previous level.
        from: u8,
        /// New level.
        to: u8,
    },
    /// I2C probe result.
    Ack {
        /// 7-bit address.
        addr: u8,
        /// Whether the transaction ACKed.
        ack: bool,
    },
    /// Gauge DeviceType handshake succeeded.
    GaugeDeviceType,
    /// GT911 point count.
    Contacts(u8),
    /// Operator prompt id from firmware.
    Prompt(String),
    /// GT911 poll returned an error (operator image).
    Gt911PollFailed,
    /// Raw GT911 status-register byte from `gt911 st=0xNN`.
    Gt911Status(u8),
    /// GPIO21 INT level from `gt911 int=0` / `int=1`.
    Gt911Int(bool),
    /// Image git identity (`git=<hash> dirty=<0|1>`).
    Git {
        /// `git rev-parse HEAD`, or `unknown`.
        hash: String,
        /// Working tree was dirty at compile time.
        dirty: bool,
    },
    /// Anything else (ROM, ANSI, boot chatter).
    Ignored,
}

/// Fields from a heartbeat line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heartbeat {
    /// Seconds since the loop started.
    pub t_s: u32,
    /// GPIO9.
    pub vbus: bool,
    /// GPIO7.
    pub gpio7: bool,
    /// GPIO40.
    pub gpio40: bool,
    /// GPIO11.
    pub sd_cd: bool,
    /// State of charge percent.
    pub soc_pct: u8,
    /// Millivolts.
    pub voltage_mv: u16,
    /// Signed milliamperes.
    pub current_ma: i16,
    /// Pose token, or `none`.
    pub imu: String,
}

/// Strip ANSI and decode one logical line (no trailing newline).
#[must_use]
pub fn parse_line(raw: &str) -> ParsedLine {
    let line = strip_ansi(raw).trim().to_string();
    let Some(rest) = line.strip_prefix(LOG_PREFIX) else {
        return ParsedLine::Ignored;
    };
    let rest = rest.strip_prefix(':').unwrap_or(rest).trim();

    if let Some(hb) = parse_heartbeat(rest) {
        return ParsedLine::Heartbeat(hb);
    }
    if let Some((gpio, down)) = parse_button(rest) {
        return ParsedLine::Button { gpio, down };
    }
    if let Some((name, from, to)) = parse_level(rest) {
        return ParsedLine::Level { name, from, to };
    }
    if let Some((addr, ack)) = parse_ack(rest) {
        return ParsedLine::Ack { addr, ack };
    }
    if rest.starts_with("gauge DeviceType") {
        return ParsedLine::GaugeDeviceType;
    }
    if let Some(n) = rest.strip_prefix("contacts=") {
        if let Ok(n) = n.trim().parse() {
            return ParsedLine::Contacts(n);
        }
    }
    if let Some(id) = rest.strip_prefix("prompt ") {
        return ParsedLine::Prompt(id.trim().to_string());
    }
    if rest == "gt911 poll failed" {
        return ParsedLine::Gt911PollFailed;
    }
    if let Some(status) = parse_gt911_status(rest) {
        return ParsedLine::Gt911Status(status);
    }
    if let Some(high) = parse_gt911_int(rest) {
        return ParsedLine::Gt911Int(high);
    }
    if let Some((hash, dirty)) = parse_git(rest) {
        return ParsedLine::Git { hash, dirty };
    }
    ParsedLine::Ignored
}

fn parse_gt911_status(rest: &str) -> Option<u8> {
    let hex = rest.strip_prefix("gt911 st=")?;
    let hex = hex.strip_prefix("0x").or_else(|| hex.strip_prefix("0X"))?;
    u8::from_str_radix(hex.trim(), 16).ok()
}

fn parse_gt911_int(rest: &str) -> Option<bool> {
    let bit = rest.strip_prefix("gt911 int=")?;
    match bit.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

fn parse_git(rest: &str) -> Option<(String, bool)> {
    let rest = rest.strip_prefix("git=")?;
    let (hash, dirty_part) = rest.split_once(" dirty=")?;
    if hash.is_empty() {
        return None;
    }
    let dirty = match dirty_part.trim() {
        "1" => true,
        "0" => false,
        _ => return None,
    };
    Some((hash.to_string(), dirty))
}

fn parse_heartbeat(rest: &str) -> Option<Heartbeat> {
    if !rest.starts_with("t=") {
        return None;
    }
    let mut t_s = None;
    let mut vbus = None;
    let mut gpio7 = None;
    let mut gpio40 = None;
    let mut sd_cd = None;
    let mut soc_pct = None;
    let mut voltage_mv = None;
    let mut current_ma = None;
    let mut imu = None;
    for token in rest.split_whitespace() {
        let Some((k, v)) = token.split_once('=') else {
            continue;
        };
        match k {
            "t" => t_s = v.parse().ok(),
            "vbus" => vbus = parse_bit(v),
            "gpio7" => gpio7 = parse_bit(v),
            "gpio40" => gpio40 = parse_bit(v),
            "sd_cd" => sd_cd = parse_bit(v),
            "soc" => soc_pct = v.parse().ok(),
            "v" => voltage_mv = v.parse().ok(),
            "i" => current_ma = v.parse().ok(),
            "imu" => imu = Some(v.to_string()),
            _ => {}
        }
    }
    Some(Heartbeat {
        t_s: t_s?,
        vbus: vbus?,
        gpio7: gpio7?,
        gpio40: gpio40?,
        sd_cd: sd_cd?,
        soc_pct: soc_pct?,
        voltage_mv: voltage_mv?,
        current_ma: current_ma?,
        imu: imu?,
    })
}

fn parse_button(rest: &str) -> Option<(u8, bool)> {
    let mut parts = rest.split_whitespace();
    if parts.next()? != "btn" {
        return None;
    }
    let gpio = parts.next()?.parse().ok()?;
    match parts.next()? {
        "down" => Some((gpio, true)),
        "up" => Some((gpio, false)),
        _ => None,
    }
}

fn parse_level(rest: &str) -> Option<(String, u8, u8)> {
    let mut parts = rest.split_whitespace();
    let name = parts.next()?.to_string();
    if !matches!(name.as_str(), "vbus" | "gpio7" | "gpio40" | "sd_cd") {
        return None;
    }
    let from = parts.next()?.parse().ok()?;
    if parts.next()? != "->" {
        return None;
    }
    let to = parts.next()?.parse().ok()?;
    Some((name, from, to))
}

fn parse_ack(rest: &str) -> Option<(u8, bool)> {
    let mut parts = rest.split_whitespace();
    let addr = parts.next()?;
    let ack = match parts.next()? {
        "ack" => true,
        "nak" => false,
        _ => return None,
    };
    let addr = addr.strip_prefix("0x").unwrap_or(addr);
    let addr = u8::from_str_radix(addr, 16).ok()?;
    Some((addr, ack))
}

fn parse_bit(v: &str) -> Option<bool> {
    match v {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'[' => {
                    i += 2;
                    while i < bytes.len() && !(bytes[i] >= b'@' && bytes[i] <= b'~') {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                }
                b']' => {
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                b'(' | b')' => {
                    i += 2;
                    if i < bytes.len() {
                        i += 1;
                    }
                }
                _ => i += 2,
            }
            continue;
        }
        match s[i..].chars().next() {
            Some(ch) => {
                out.push(ch);
                i += ch.len_utf8();
            }
            None => break,
        }
    }
    out
}

/// Running decode of a session.
#[derive(Debug, Default, Clone)]
pub struct Accumulator {
    /// Last heartbeat, if any.
    pub heartbeat: Option<Heartbeat>,
    /// I2C address → last ACK/NAK.
    pub acks: std::collections::BTreeMap<u8, bool>,
    /// DeviceType line seen.
    pub gauge_device_type: bool,
    /// Highest GT911 contact count this session.
    pub contacts_max: u8,
    /// Heartbeats seen.
    pub heartbeats: u32,
    /// Operator image printed `gt911 poll failed` at least once.
    pub gt911_poll_failed: bool,
    /// Highest GT911 status-register byte from `gt911 st=`.
    pub gt911_status_max: u8,
    /// Last GT911 INT level from `gt911 int=`, if any.
    pub gt911_int: Option<bool>,
    /// True if INT was seen both low and high this session.
    pub gt911_int_changed: bool,
    /// Last `gpio7 from -> to` this session, if any.
    pub gpio7_edge: Option<(u8, u8)>,
    /// Firmware git line, if printed.
    pub firmware_git: Option<(String, bool)>,
}

impl Accumulator {
    /// Fold one parsed line.
    pub fn observe(&mut self, line: &ParsedLine) {
        match line {
            ParsedLine::Heartbeat(hb) => {
                self.heartbeats = self.heartbeats.saturating_add(1);
                self.heartbeat = Some(hb.clone());
            }
            ParsedLine::Ack { addr, ack } => {
                self.acks.insert(*addr, *ack);
            }
            ParsedLine::GaugeDeviceType => self.gauge_device_type = true,
            ParsedLine::Contacts(n) => self.contacts_max = self.contacts_max.max(*n),
            ParsedLine::Gt911PollFailed => self.gt911_poll_failed = true,
            ParsedLine::Gt911Status(status) => {
                self.gt911_status_max = self.gt911_status_max.max(*status);
            }
            ParsedLine::Gt911Int(high) => {
                if let Some(prev) = self.gt911_int {
                    if prev != *high {
                        self.gt911_int_changed = true;
                    }
                }
                self.gt911_int = Some(*high);
            }
            ParsedLine::Level { name, from, to } => {
                if name == "gpio7" {
                    self.gpio7_edge = Some((*from, *to));
                }
            }
            ParsedLine::Git { hash, dirty } => {
                self.firmware_git = Some((hash.clone(), *dirty));
            }
            ParsedLine::Button { .. } | ParsedLine::Prompt(_) | ParsedLine::Ignored => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_round_trip_from_the_log_crate() {
        let line =
            "simple-debug: t=12 vbus=1 gpio7=0 gpio40=1 sd_cd=1 soc=100 v=4195 i=0 imu=FaceUp";
        match parse_line(line) {
            ParsedLine::Heartbeat(hb) => {
                assert_eq!(hb.t_s, 12);
                assert!(hb.vbus);
                assert!(!hb.gpio7);
                assert!(hb.gpio40);
                assert_eq!(hb.imu, "FaceUp");
                assert_eq!(hb.current_ma, 0);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ansi_prefix_is_stripped() {
        let line = "\x1b[32msimple-debug: t=1 vbus=1 gpio7=0 gpio40=1 sd_cd=1 soc=1 v=1 i=-12 imu=none\x1b[0m";
        assert!(matches!(parse_line(line), ParsedLine::Heartbeat(_)));
    }

    #[test]
    fn strip_ansi_keeps_utf8_and_drops_osc() {
        assert_eq!(
            strip_ansi("\u{1b}]0;title\u{7}simple-debug: prompt café"),
            "simple-debug: prompt café"
        );
        assert_eq!(
            strip_ansi("\u{1b}]0;title\u{1b}\\simple-debug: btn 4 down"),
            "simple-debug: btn 4 down"
        );
        assert_eq!(
            strip_ansi("\u{1b}csimple-debug: prompt vbus"),
            "simple-debug: prompt vbus"
        );
        assert!(matches!(
            parse_line(
                "\x1b[1msimple-debug: t=1 vbus=1 gpio7=0 gpio40=1 sd_cd=1 soc=1 v=1 i=0 imu=none"
            ),
            ParsedLine::Heartbeat(_)
        ));
    }

    #[test]
    fn button_and_level_edges() {
        assert_eq!(
            parse_line("simple-debug: btn 4 down"),
            ParsedLine::Button {
                gpio: 4,
                down: true
            }
        );
        assert_eq!(
            parse_line("simple-debug: vbus 1 -> 0"),
            ParsedLine::Level {
                name: "vbus".into(),
                from: 1,
                to: 0
            }
        );
    }

    #[test]
    fn ack_and_contacts_and_prompt() {
        assert_eq!(
            parse_line("simple-debug: 0x14 ack"),
            ParsedLine::Ack {
                addr: 0x14,
                ack: true
            }
        );
        assert_eq!(
            parse_line("simple-debug: 0x44 nak"),
            ParsedLine::Ack {
                addr: 0x44,
                ack: false
            }
        );
        assert_eq!(
            parse_line("simple-debug: contacts=2"),
            ParsedLine::Contacts(2)
        );
        assert_eq!(
            parse_line("simple-debug: prompt vbus"),
            ParsedLine::Prompt("vbus".into())
        );
        assert_eq!(
            parse_line("simple-debug: gauge DeviceType 0x0220"),
            ParsedLine::GaugeDeviceType
        );
        assert_eq!(
            parse_line("simple-debug: gt911 poll failed"),
            ParsedLine::Gt911PollFailed
        );
        assert_eq!(
            parse_line("simple-debug: gt911 st=0x81"),
            ParsedLine::Gt911Status(0x81)
        );
        assert_eq!(
            parse_line("simple-debug: gt911 int=0"),
            ParsedLine::Gt911Int(false)
        );
        assert_eq!(
            parse_line("simple-debug: git=deadbeef dirty=1"),
            ParsedLine::Git {
                hash: "deadbeef".into(),
                dirty: true
            }
        );
    }

    #[test]
    fn rom_noise_is_ignored() {
        assert_eq!(parse_line("rst:0x1 (POWERON)"), ParsedLine::Ignored);
    }

    #[test]
    fn accumulator_tracks_max_contacts() {
        let mut acc = Accumulator::default();
        acc.observe(&ParsedLine::Contacts(1));
        acc.observe(&ParsedLine::Contacts(0));
        acc.observe(&ParsedLine::Contacts(2));
        assert_eq!(acc.contacts_max, 2);
        acc.observe(&ParsedLine::Gt911Status(0x00));
        acc.observe(&ParsedLine::Gt911Status(0x81));
        acc.observe(&ParsedLine::Gt911Status(0x01));
        assert_eq!(acc.gt911_status_max, 0x81);
    }

    #[test]
    fn accumulator_records_gpio7_level_edges() {
        let mut acc = Accumulator::default();
        acc.observe(&ParsedLine::Level {
            name: "gpio7".into(),
            from: 1,
            to: 0,
        });
        assert_eq!(acc.gpio7_edge, Some((1, 0)));
    }
}
