//! UART snapshot and edge lines for the Sticky simple-debug image.
//!
//! The firmware owns buses and pins. This crate owns the **strings** it
//! prints so the log contract can be tested on the host: a one-second
//! heartbeat of raw levels, extra lines only when a GPIO changes, and no
//! factory serial / USB serial / MAC fields.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::fmt::{self, Write};
use core::str;

/// Token before every UART line (`simple-debug: …`).
pub const LOG_PREFIX: &str = "simple-debug";

/// Bytes reserved for a heartbeat. Sized for the documented field widths,
/// including `t=u32::MAX` and `imu=Landscape180`.
pub const HEARTBEAT_CAPACITY: usize = 128;

/// Bytes reserved for a single edge line.
pub const EDGE_CAPACITY: usize = 48;

/// Bytes reserved for an operator prompt line.
pub const PROMPT_CAPACITY: usize = 48;

/// Bytes reserved for a GT911 contact-count line.
pub const CONTACTS_CAPACITY: usize = 32;

/// Bytes reserved for a git identity line (`git=<40 hex> dirty=0`).
pub const GIT_CAPACITY: usize = 80;

/// Bytes reserved for a GT911 status line (`gt911 st=0xNN`).
pub const GT911_STATUS_CAPACITY: usize = 32;

/// Bytes reserved for a GT911 product-ID line (`gt911 id=911`).
pub const GT911_ID_CAPACITY: usize = 48;

/// Bytes reserved for a GT911 INT line (`gt911 int=0`).
pub const GT911_INT_CAPACITY: usize = 32;

/// Bytes reserved for a SHT40 line (`sht t=… rh=…` or `sht none`).
pub const SHT_CAPACITY: usize = 48;

/// Bytes reserved for a PCF8563 line (`rtc y=…` or `rtc none`).
pub const RTC_CAPACITY: usize = 64;

/// Why a format into a caller buffer failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatError {
    /// The buffer was shorter than the formatted line.
    Truncated,
}

/// Enclosure pose labels used in the heartbeat. These match the board
/// classifier names and are not a `Debug` dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImuPose {
    /// USB-C on the bottom short edge.
    Portrait0,
    /// USB-C on the top short edge.
    Portrait180,
    /// USB-C on the right short edge.
    Landscape0,
    /// USB-C on the left short edge.
    Landscape180,
    /// Lying face up.
    FaceUp,
    /// Lying face down.
    FaceDown,
}

impl ImuPose {
    /// Token written after `imu=` in the heartbeat.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Portrait0 => "Portrait0",
            Self::Portrait180 => "Portrait180",
            Self::Landscape0 => "Landscape0",
            Self::Landscape180 => "Landscape180",
            Self::FaceUp => "FaceUp",
            Self::FaceDown => "FaceDown",
        }
    }
}

/// One sample of the raw levels the simple-debug image is allowed to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    /// Seconds since the learning loop started.
    pub t_s: u32,
    /// GPIO9: high means external power present.
    pub vbus: bool,
    /// GPIO7: shared IMU INT1 / gauge GPOUT; input only.
    pub gpio7: bool,
    /// GPIO40: raw STAT level, not "charging".
    pub gpio40: bool,
    /// GPIO11: card-detect; insert = 0 (10 kΩ pull-up).
    pub sd_cd: bool,
    /// Gauge state of charge in percent.
    pub soc_pct: u8,
    /// Pack voltage in millivolts.
    pub voltage_mv: u16,
    /// Signed current in milliamperes. Positive is charge.
    pub current_ma: i16,
    /// Classified pose, or `None` when no axis dominates.
    pub imu: Option<ImuPose>,
}

/// Input levels used to detect edges. `true` is high.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpioLevels {
    /// GPIO4 (AI/OK), active low.
    pub btn4: bool,
    /// GPIO5 (up), active low.
    pub btn5: bool,
    /// GPIO6 (down), active low.
    pub btn6: bool,
    /// GPIO9 external-power sense.
    pub vbus: bool,
    /// GPIO7 shared IMU INT1 / gauge GPOUT (input only).
    pub gpio7: bool,
    /// GPIO40 charger STAT.
    pub gpio40: bool,
    /// GPIO11 MicroSD card detect.
    pub sd_cd: bool,
}

/// A GPIO change to print as its own line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// Active-low button went low (`1 -> 0`).
    ButtonDown {
        /// ESP32-S3 GPIO number (4, 5, or 6).
        gpio: u8,
    },
    /// Active-low button went high (`0 -> 1`).
    ButtonUp {
        /// ESP32-S3 GPIO number (4, 5, or 6).
        gpio: u8,
    },
    /// A raw level field changed.
    Level {
        /// Field name in the log (`vbus`, `gpio7`, `gpio40`, `sd_cd`).
        name: &'static str,
        /// Previous level as `0` or `1`.
        from: u8,
        /// New level as `0` or `1`.
        to: u8,
    },
}

/// Writes `snapshot` into `buf` as a heartbeat line without a trailing newline.
pub fn format_heartbeat<'a>(
    snapshot: &Snapshot,
    buf: &'a mut [u8],
) -> Result<&'a str, FormatError> {
    let imu = snapshot.imu.map_or("none", ImuPose::as_str);
    write_into(
        buf,
        format_args!(
            "{}: t={} vbus={} gpio7={} gpio40={} sd_cd={} soc={} v={} i={} imu={}",
            LOG_PREFIX,
            snapshot.t_s,
            u8::from(snapshot.vbus),
            u8::from(snapshot.gpio7),
            u8::from(snapshot.gpio40),
            u8::from(snapshot.sd_cd),
            snapshot.soc_pct,
            snapshot.voltage_mv,
            snapshot.current_ma,
            imu,
        ),
    )
}

/// Writes `edge` into `buf` without a trailing newline.
pub fn format_edge(edge: Edge, buf: &mut [u8]) -> Result<&str, FormatError> {
    match edge {
        Edge::ButtonDown { gpio } => {
            write_into(buf, format_args!("{}: btn {gpio} down", LOG_PREFIX))
        }
        Edge::ButtonUp { gpio } => write_into(buf, format_args!("{}: btn {gpio} up", LOG_PREFIX)),
        Edge::Level { name, from, to } => {
            write_into(buf, format_args!("{}: {name} {from} -> {to}", LOG_PREFIX))
        }
    }
}

/// Writes `simple-debug: prompt <step_id>` into `buf` without a trailing newline.
pub fn format_prompt<'a>(step_id: &str, buf: &'a mut [u8]) -> Result<&'a str, FormatError> {
    write_into(buf, format_args!("{}: prompt {step_id}", LOG_PREFIX))
}

/// Writes `simple-debug: contacts=<n>` into `buf` without a trailing newline.
///
/// `n` is 0..=5 on this FPC (GT911 Rev.09 §1). Status-line cadence is
/// board `touch::STATUS_HEARTBEAT`, not this formatter.
pub fn format_contacts(n: u8, buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(buf, format_args!("{}: contacts={n}", LOG_PREFIX))
}

/// Writes `simple-debug: git=<hash> dirty=<0|1>` into `buf` without a trailing newline.
pub fn format_git<'a>(hash: &str, dirty: bool, buf: &'a mut [u8]) -> Result<&'a str, FormatError> {
    write_into(
        buf,
        format_args!("{}: git={hash} dirty={}", LOG_PREFIX, u8::from(dirty)),
    )
}

/// Writes `simple-debug: gt911 st=0xNN` into `buf` without a trailing newline.
pub fn format_gt911_status(status: u8, buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(buf, format_args!("{}: gt911 st={status:#04x}", LOG_PREFIX))
}

/// Writes `simple-debug: gt911 id=911` (or hex) into `buf` without a trailing newline.
pub fn format_gt911_id<'a>(id: &[u8; 4], buf: &'a mut [u8]) -> Result<&'a str, FormatError> {
    if id == b"911\0" {
        write_into(buf, format_args!("{}: gt911 id=911", LOG_PREFIX))
    } else {
        write_into(
            buf,
            format_args!(
                "{}: gt911 id={:02x}{:02x}{:02x}{:02x}",
                LOG_PREFIX, id[0], id[1], id[2], id[3]
            ),
        )
    }
}

/// Writes `simple-debug: sht t=<milli °C> rh=<milli % RH>` into `buf`.
pub fn format_sht(t_mc: i32, rh_mp: i32, buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(buf, format_args!("{LOG_PREFIX}: sht t={t_mc} rh={rh_mp}"))
}

/// Writes `simple-debug: sht none` when the measure NAKs.
pub fn format_sht_none(buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(buf, format_args!("{LOG_PREFIX}: sht none"))
}

/// Writes a PCF8563 time line. `year` is the chip's 0–99 field. `vl` is
/// the seconds-register VL bit (NXP: 1 = integrity not guaranteed).
pub fn format_rtc(
    year: u8,
    month: u8,
    day: u8,
    hours: u8,
    minutes: u8,
    seconds: u8,
    vl: bool,
    buf: &mut [u8],
) -> Result<&str, FormatError> {
    write_into(
        buf,
        format_args!(
            "{}: rtc y={year} mo={month} d={day} h={hours} mi={minutes} s={seconds} vl={}",
            LOG_PREFIX,
            u8::from(vl),
        ),
    )
}

/// Writes `simple-debug: rtc none` when the RTC read NAKs.
pub fn format_rtc_none(buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(buf, format_args!("{LOG_PREFIX}: rtc none"))
}

/// Writes `simple-debug: gt911 int=0` or `int=1` into `buf` without a trailing newline.
pub fn format_gt911_int(high: bool, buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(
        buf,
        format_args!("{}: gt911 int={}", LOG_PREFIX, u8::from(high)),
    )
}

/// Appends edges from `prev` to `now` into `out`. Returns how many were written.
///
/// Buttons are active low. Other nets are logged as raw `0`/`1`. Unchanged
/// pins produce nothing.
pub fn collect_edges(prev: &GpioLevels, now: &GpioLevels, out: &mut [Edge]) -> usize {
    let mut n = 0;
    n += push_button(out, n, 4, prev.btn4, now.btn4);
    n += push_button(out, n, 5, prev.btn5, now.btn5);
    n += push_button(out, n, 6, prev.btn6, now.btn6);
    n += push_level(out, n, "vbus", prev.vbus, now.vbus);
    n += push_level(out, n, "gpio7", prev.gpio7, now.gpio7);
    n += push_level(out, n, "gpio40", prev.gpio40, now.gpio40);
    n += push_level(out, n, "sd_cd", prev.sd_cd, now.sd_cd);
    n
}

fn push_button(out: &mut [Edge], n: usize, gpio: u8, prev: bool, now: bool) -> usize {
    if prev == now || n >= out.len() {
        return 0;
    }
    out[n] = if prev && !now {
        Edge::ButtonDown { gpio }
    } else {
        Edge::ButtonUp { gpio }
    };
    1
}

fn push_level(out: &mut [Edge], n: usize, name: &'static str, prev: bool, now: bool) -> usize {
    if prev == now || n >= out.len() {
        return 0;
    }
    out[n] = Edge::Level {
        name,
        from: u8::from(prev),
        to: u8::from(now),
    };
    1
}

fn write_into<'a>(buf: &'a mut [u8], args: fmt::Arguments<'_>) -> Result<&'a str, FormatError> {
    let mut tmp = [0u8; HEARTBEAT_CAPACITY];
    let pos = {
        let mut writer = SliceWriter {
            buf: &mut tmp,
            pos: 0,
        };
        writer.write_fmt(args).map_err(|_| FormatError::Truncated)?;
        writer.pos
    };
    if pos > buf.len() {
        return Err(FormatError::Truncated);
    }
    buf[..pos].copy_from_slice(&tmp[..pos]);
    str::from_utf8(&buf[..pos]).map_err(|_| FormatError::Truncated)
}

struct SliceWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl Write for SliceWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let rest = self.buf.len().saturating_sub(self.pos);
        if s.len() > rest {
            return Err(fmt::Error);
        }
        self.buf[self.pos..self.pos + s.len()].copy_from_slice(s.as_bytes());
        self.pos += s.len();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::string::String;

    fn sample() -> Snapshot {
        Snapshot {
            t_s: 12,
            vbus: true,
            gpio7: true,
            gpio40: false,
            sd_cd: true,
            soc_pct: 87,
            voltage_mv: 3870,
            current_ma: -12,
            imu: Some(ImuPose::FaceUp),
        }
    }

    fn idle_high() -> GpioLevels {
        GpioLevels {
            btn4: true,
            btn5: true,
            btn6: true,
            vbus: true,
            gpio7: true,
            gpio40: false,
            sd_cd: true,
        }
    }

    fn heartbeat(snapshot: &Snapshot) -> String {
        let mut buf = [0u8; HEARTBEAT_CAPACITY];
        String::from(format_heartbeat(snapshot, &mut buf).unwrap())
    }

    #[test]
    fn heartbeat_matches_the_agreed_shape() {
        assert_eq!(
            heartbeat(&sample()),
            "simple-debug: t=12 vbus=1 gpio7=1 gpio40=0 sd_cd=1 soc=87 v=3870 i=-12 imu=FaceUp"
        );
    }

    #[test]
    fn signed_current_keeps_the_minus_sign() {
        assert!(heartbeat(&sample()).contains("i=-12"));
    }

    #[test]
    fn missing_imu_is_an_explicit_none_token() {
        let mut snapshot = sample();
        snapshot.imu = None;
        assert!(heartbeat(&snapshot).contains("imu=none"));
    }

    #[test]
    fn imu_labels_are_the_classifier_names() {
        assert_eq!(ImuPose::Portrait0.as_str(), "Portrait0");
        assert_eq!(ImuPose::Landscape180.as_str(), "Landscape180");
        assert_eq!(ImuPose::FaceDown.as_str(), "FaceDown");
    }

    #[test]
    fn worst_case_heartbeat_fits_the_reserved_buffer() {
        let snapshot = Snapshot {
            t_s: u32::MAX,
            vbus: true,
            gpio7: true,
            gpio40: true,
            sd_cd: true,
            soc_pct: u8::MAX,
            voltage_mv: u16::MAX,
            current_ma: i16::MIN,
            imu: Some(ImuPose::Landscape180),
        };
        let mut buf = [0u8; HEARTBEAT_CAPACITY];
        let line = format_heartbeat(&snapshot, &mut buf).unwrap();
        assert!(line.len() < HEARTBEAT_CAPACITY);
        assert!(line.starts_with("simple-debug:"));
    }

    #[test]
    fn a_short_buffer_is_an_error_not_a_clipped_prefix() {
        let mut buf = [0u8; 8];
        assert_eq!(
            format_heartbeat(&sample(), &mut buf),
            Err(FormatError::Truncated)
        );
        assert_eq!(&buf, &[0; 8]);
    }

    #[test]
    fn heartbeat_has_no_identifier_fields() {
        let line = heartbeat(&sample());
        let lower = line.to_ascii_lowercase();
        assert!(!lower.contains("serial"));
        assert!(!lower.contains("mac"));
        assert!(!has_colon_mac(&lower));
    }

    #[test]
    fn button_high_to_low_is_down() {
        let prev = idle_high();
        let mut now = idle_high();
        now.btn4 = false;
        let mut edges = [Edge::ButtonDown { gpio: 0 }; 7];
        let n = collect_edges(&prev, &now, &mut edges);
        assert_eq!(n, 1);
        assert_eq!(edges[0], Edge::ButtonDown { gpio: 4 });
        let mut buf = [0u8; EDGE_CAPACITY];
        assert_eq!(
            format_edge(edges[0], &mut buf).unwrap(),
            "simple-debug: btn 4 down"
        );
    }

    #[test]
    fn button_low_to_high_is_up() {
        let mut prev = idle_high();
        prev.btn5 = false;
        let now = idle_high();
        let mut edges = [Edge::ButtonDown { gpio: 0 }; 7];
        let n = collect_edges(&prev, &now, &mut edges);
        assert_eq!(n, 1);
        assert_eq!(edges[0], Edge::ButtonUp { gpio: 5 });
        let mut buf = [0u8; EDGE_CAPACITY];
        assert_eq!(
            format_edge(edges[0], &mut buf).unwrap(),
            "simple-debug: btn 5 up"
        );
    }

    #[test]
    fn unchanged_buttons_emit_nothing() {
        let levels = idle_high();
        let mut edges = [Edge::ButtonDown { gpio: 0 }; 7];
        assert_eq!(collect_edges(&levels, &levels, &mut edges), 0);
    }

    #[test]
    fn raw_nets_log_levels_not_stories() {
        let prev = idle_high();
        let mut now = idle_high();
        now.vbus = false;
        now.sd_cd = false;
        now.gpio7 = false;
        now.gpio40 = true;
        let mut edges = [Edge::ButtonDown { gpio: 0 }; 7];
        let n = collect_edges(&prev, &now, &mut edges);
        assert_eq!(n, 4);
        let mut buf = [0u8; EDGE_CAPACITY];
        let lines: std::vec::Vec<_> = edges[..n]
            .iter()
            .map(|edge| String::from(format_edge(*edge, &mut buf).unwrap()))
            .collect();
        assert!(lines.contains(&String::from("simple-debug: vbus 1 -> 0")));
        assert!(lines.contains(&String::from("simple-debug: sd_cd 1 -> 0")));
        assert!(lines.contains(&String::from("simple-debug: gpio7 1 -> 0")));
        assert!(lines.contains(&String::from("simple-debug: gpio40 0 -> 1")));
        for line in &lines {
            assert!(!line.contains("charging"));
        }
    }

    #[test]
    fn prompt_and_contacts_lines_match_the_agreed_shape() {
        let mut buf = [0u8; PROMPT_CAPACITY];
        assert_eq!(
            format_prompt("vbus", &mut buf).unwrap(),
            "simple-debug: prompt vbus"
        );
        let mut buf = [0u8; CONTACTS_CAPACITY];
        assert_eq!(
            format_contacts(2, &mut buf).unwrap(),
            "simple-debug: contacts=2"
        );
        let mut buf = [0u8; GIT_CAPACITY];
        assert_eq!(
            format_git("deadbeef", true, &mut buf).unwrap(),
            "simple-debug: git=deadbeef dirty=1"
        );
        let mut buf = [0u8; GT911_STATUS_CAPACITY];
        assert_eq!(
            format_gt911_status(0x81, &mut buf).unwrap(),
            "simple-debug: gt911 st=0x81"
        );
        let mut buf = [0u8; GT911_ID_CAPACITY];
        assert_eq!(
            format_gt911_id(b"911\0", &mut buf).unwrap(),
            "simple-debug: gt911 id=911"
        );
        let mut buf = [0u8; GT911_INT_CAPACITY];
        assert_eq!(
            format_gt911_int(false, &mut buf).unwrap(),
            "simple-debug: gt911 int=0"
        );
        assert_eq!(seeed_reterminal_sticky::touch::MAX_TOUCH_POINTS, 5);
        assert_eq!(
            seeed_reterminal_sticky::touch::STATUS_HEARTBEAT,
            seeed_reterminal_sticky::touch::StatusHeartbeat::EverySecs(10)
        );
        let mut buf = [0u8; SHT_CAPACITY];
        assert_eq!(
            format_sht(23400, 45100, &mut buf).unwrap(),
            "simple-debug: sht t=23400 rh=45100"
        );
        assert_eq!(format_sht_none(&mut buf).unwrap(), "simple-debug: sht none");
        let mut buf = [0u8; RTC_CAPACITY];
        assert_eq!(
            format_rtc(26, 8, 30, 15, 14, 0, false, &mut buf).unwrap(),
            "simple-debug: rtc y=26 mo=8 d=30 h=15 mi=14 s=0 vl=0"
        );
        assert_eq!(format_rtc_none(&mut buf).unwrap(), "simple-debug: rtc none");
    }

    fn has_colon_mac(line: &str) -> bool {
        // xx:xx:xx:xx:xx:xx — the log must not grow a station MAC field.
        let bytes = line.as_bytes();
        if bytes.len() < 17 {
            return false;
        }
        bytes.windows(17).any(|window| {
            is_hex(window[0])
                && is_hex(window[1])
                && window[2] == b':'
                && is_hex(window[3])
                && is_hex(window[4])
                && window[5] == b':'
                && is_hex(window[6])
                && is_hex(window[7])
                && window[8] == b':'
                && is_hex(window[9])
                && is_hex(window[10])
                && window[11] == b':'
                && is_hex(window[12])
                && is_hex(window[13])
                && window[14] == b':'
                && is_hex(window[15])
                && is_hex(window[16])
        })
    }

    fn is_hex(b: u8) -> bool {
        b.is_ascii_hexdigit()
    }
}
