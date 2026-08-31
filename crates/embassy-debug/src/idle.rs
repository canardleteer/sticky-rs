//! Unattended embassy-debug UART: boot tokens plus idle IMU / GT911 status.
//!
//! Sit still. Do not require a tap, key, or tilt. Ignore ROM / IDF lines
//! (they can carry a factory serial).

use crate::{ImuPose, LOG_PREFIX};

/// Safe idle tokens seen in one listen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IdleListen {
    /// `embassy-debug: latched`
    pub latched: bool,
    /// `git=` line.
    pub git: bool,
    /// `gt911 addr dance`
    pub addr_dance: bool,
    /// First address select was INT low (`gt911 int=0`).
    pub int_low: bool,
    /// `0x5d ack` after that dance.
    pub ack_5d: bool,
    /// `gt911 no init status clear`
    pub no_init_clear: bool,
    /// `gt911 no command write`
    pub no_command_write: bool,
    /// `imu accel init ok`
    pub imu_init_ok: bool,
    /// Last `imu=` pose, if any.
    pub imu: Option<ImuPose>,
    /// Last `gt911 st=0xNN` byte, if any.
    pub gt911_status: Option<u8>,
}

impl IdleListen {
    /// Scan `log` for prefix lines. Non-`embassy-debug:` chatter is ignored.
    #[must_use]
    pub fn evaluate(log: &str) -> Self {
        let mut seen = Self::default();
        for raw in log.lines() {
            let line = raw.trim();
            let Some(rest) = strip_prefix(line) else {
                continue;
            };
            if rest == "latched" {
                seen.latched = true;
            }
            if rest.starts_with("git=") {
                seen.git = true;
            }
            if rest == "gt911 addr dance" {
                seen.addr_dance = true;
            }
            if rest == "gt911 int=0" {
                seen.int_low = true;
            }
            if rest.eq_ignore_ascii_case("0x5d ack") {
                seen.ack_5d = true;
            }
            if rest == "gt911 no init status clear" {
                seen.no_init_clear = true;
            }
            if rest == "gt911 no command write" {
                seen.no_command_write = true;
            }
            if rest == "imu accel init ok" {
                seen.imu_init_ok = true;
            }
            if let Some(pose) = parse_imu(rest) {
                seen.imu = Some(pose);
            }
            if let Some(status) = parse_gt911_status(rest) {
                seen.gt911_status = Some(status);
            }
        }
        seen
    }

    /// Every safe idle token is present.
    #[must_use]
    pub const fn ok(self) -> bool {
        self.latched
            && self.git
            && self.addr_dance
            && self.int_low
            && self.ack_5d
            && self.no_init_clear
            && self.no_command_write
            && self.imu_init_ok
            && self.imu.is_some()
            && self.gt911_status.is_some()
    }
}

fn strip_prefix(line: &str) -> Option<&str> {
    let start = line.find(LOG_PREFIX)?;
    line.get(start..)?
        .strip_prefix(LOG_PREFIX)?
        .strip_prefix(": ")
}

fn parse_imu(rest: &str) -> Option<ImuPose> {
    let after = rest.split_once(" imu=")?.1;
    let token = after.split_whitespace().next()?;
    ImuPose::from_token(token)
}

fn parse_gt911_status(rest: &str) -> Option<u8> {
    let after = rest.split_once("gt911 st=")?.1;
    let token = after.split_whitespace().next()?;
    let hex = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))?;
    u8::from_str_radix(hex, 16).ok()
}

impl ImuPose {
    /// Parse a heartbeat `imu=` token.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "Portrait0" => Some(Self::Portrait0),
            "Portrait180" => Some(Self::Portrait180),
            "Landscape0" => Some(Self::Landscape0),
            "Landscape180" => Some(Self::Landscape180),
            "FaceUp" => Some(Self::FaceUp),
            "FaceDown" => Some(Self::FaceDown),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IdleListen;
    use crate::ImuPose;

    const IDLE: &str = "\
rst:0x1 (POWERON)
embassy-debug: latched
embassy-debug: git=deadbeef dirty=0
embassy-debug: gt911 addr dance
embassy-debug: gt911 int=0
embassy-debug: 0x5d ack
embassy-debug: 0x14 nak
embassy-debug: gt911 no init status clear
embassy-debug: gt911 no command write
embassy-debug: imu accel init ok
embassy-debug: t=5000 imu=FaceUp x=12 y=-30 z=16300
embassy-debug: t=10000 gt911 st=0x80
";

    #[test]
    fn idle_listen_accepts_a_quiet_sit() {
        let seen = IdleListen::evaluate(IDLE);
        assert!(seen.ok());
        assert_eq!(seen.imu, Some(ImuPose::FaceUp));
        assert_eq!(seen.gt911_status, Some(0x80));
    }

    #[test]
    fn idle_listen_skips_a_leading_garbage_byte() {
        let mut dirty = String::from("\u{80}");
        dirty.push_str(IDLE);
        assert!(IdleListen::evaluate(&dirty).ok());
    }

    #[test]
    fn idle_listen_ignores_rom_chatter() {
        let seen = IdleListen::evaluate("I (5672) serial_number: SECRET\n");
        assert!(!seen.ok());
        assert!(!seen.latched);
    }

    #[test]
    fn idle_listen_needs_status_and_imu() {
        let missing_status = IdleListen::evaluate(
            "embassy-debug: latched\n\
             embassy-debug: git=x dirty=0\n\
             embassy-debug: gt911 addr dance\n\
             embassy-debug: gt911 int=0\n\
             embassy-debug: 0x5d ack\n\
             embassy-debug: gt911 no init status clear\n\
             embassy-debug: gt911 no command write\n\
             embassy-debug: imu accel init ok\n\
             embassy-debug: t=1 imu=FaceUp x=0 y=0 z=1\n",
        );
        assert!(missing_status.imu.is_some());
        assert!(missing_status.gt911_status.is_none());
        assert!(!missing_status.ok());
    }
}
