//! Unattended simple-debug UART: latch, gauge type, heartbeat, SHT, RTC.
//!
//! Sit still. Do not require a key, card, or `/CE`. Ignore ROM / IDF lines.

use crate::{ImuPose, LOG_PREFIX};

/// Safe idle tokens seen in one listen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IdleListen {
    /// `latched` (bring-up line).
    pub latched: bool,
    /// `git=` line.
    pub git: bool,
    /// `gauge DeviceType 0x0220`
    pub gauge_type: bool,
    /// Last heartbeat `vbus` (USB present).
    pub vbus: Option<bool>,
    /// Last heartbeat GPIO7.
    pub gpio7: Option<bool>,
    /// Last heartbeat GPIO40 (parked STAT).
    pub gpio40: Option<bool>,
    /// Last heartbeat SD detect (empty slot is 1).
    pub sd_cd: Option<bool>,
    /// Last heartbeat pose.
    pub imu: Option<ImuPose>,
    /// A live `sht t=` line (not `sht none`).
    pub sht: bool,
    /// A live `rtc` line with `vl=` (not `rtc none`).
    pub rtc: bool,
}

impl IdleListen {
    /// Scan `log` for prefix lines. Non-`simple-debug:` chatter is ignored.
    #[must_use]
    pub fn evaluate(log: &str) -> Self {
        let mut seen = Self::default();
        for raw in log.lines() {
            let line = raw.trim();
            let Some(rest) = strip_prefix(line) else {
                continue;
            };
            if rest.starts_with("latched") {
                seen.latched = true;
            }
            if rest.starts_with("git=") {
                seen.git = true;
            }
            if rest.contains("gauge DeviceType 0x0220") {
                seen.gauge_type = true;
            }
            if let Some(hb) = parse_heartbeat(rest) {
                seen.vbus = Some(hb.0);
                seen.gpio7 = Some(hb.1);
                seen.gpio40 = Some(hb.2);
                seen.sd_cd = Some(hb.3);
                seen.imu = hb.4;
            }
            if rest.starts_with("sht t=") {
                seen.sht = true;
            }
            if rest.starts_with("rtc ") && rest.contains("vl=") {
                seen.rtc = true;
            }
        }
        seen
    }

    /// Every safe idle token is present.
    #[must_use]
    pub const fn ok(self) -> bool {
        self.latched
            && self.git
            && self.gauge_type
            && self.vbus.is_some()
            && self.gpio7.is_some()
            && self.gpio40.is_some()
            && self.sd_cd.is_some()
            && self.imu.is_some()
            && self.sht
            && self.rtc
    }
}

fn strip_prefix(line: &str) -> Option<&str> {
    let start = line.find(LOG_PREFIX)?;
    line.get(start..)?
        .strip_prefix(LOG_PREFIX)?
        .strip_prefix(": ")
}

fn parse_heartbeat(rest: &str) -> Option<(bool, bool, bool, bool, Option<ImuPose>)> {
    if !rest.contains(" vbus=") || !rest.contains(" imu=") {
        return None;
    }
    let vbus = flag_after(rest, "vbus=")?;
    let gpio7 = flag_after(rest, "gpio7=")?;
    let gpio40 = flag_after(rest, "gpio40=")?;
    let sd_cd = flag_after(rest, "sd_cd=")?;
    let imu_tok = rest.split_once(" imu=")?.1.split_whitespace().next()?;
    let imu = ImuPose::from_token(imu_tok);
    Some((vbus, gpio7, gpio40, sd_cd, imu))
}

fn flag_after(rest: &str, key: &str) -> Option<bool> {
    let after = rest.split_once(key)?.1;
    match after.as_bytes().first() {
        Some(b'0') => Some(false),
        Some(b'1') => Some(true),
        _ => None,
    }
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
I (100) serial_number: SECRET
simple-debug: latched (PWR_HOLD then PWR_LOCK)
simple-debug: git=deadbeef dirty=0
simple-debug: gauge DeviceType 0x0220
simple-debug: t=12 vbus=1 gpio7=0 gpio40=1 sd_cd=1 soc=87 v=3870 i=0 imu=FaceUp
simple-debug: sht t=23400 rh=45100
simple-debug: rtc y=26 mo=8 d=30 h=15 mi=14 s=0 vl=0
";

    #[test]
    fn idle_listen_accepts_a_quiet_sit() {
        let seen = IdleListen::evaluate(IDLE);
        assert!(seen.ok());
        assert_eq!(seen.vbus, Some(true));
        assert_eq!(seen.gpio7, Some(false));
        assert_eq!(seen.gpio40, Some(true));
        assert_eq!(seen.sd_cd, Some(true));
        assert_eq!(seen.imu, Some(ImuPose::FaceUp));
    }

    #[test]
    fn idle_listen_rejects_sht_none() {
        let log = IDLE.replace("sht t=23400 rh=45100", "sht none");
        assert!(!IdleListen::evaluate(&log).ok());
    }
}
