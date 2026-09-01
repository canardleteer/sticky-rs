//! UART event lines for the Sticky embassy-debug image.
//!
//! The firmware owns buses, pins, and the Embassy tasks. This crate owns the
//! **strings** it prints so the log contract can be tested on the host:
//! timestamped button, touch, GT911 status, IMU, mic-energy, PCM-dump,
//! radio-scan, read-only SD identify, and charge-sit lines, and no factory
//! serial / USB serial / MAC / card product-serial fields.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::fmt::{self, Write};
use core::str;

/// Token before every UART line (`embassy-debug: …`).
pub const LOG_PREFIX: &str = "embassy-debug";

/// Default IMU report period, in seconds.
pub const IMU_REPORT_SECS: u32 = 5;

/// Mic energy report period, in milliseconds (`--features mic` image).
pub const MIC_REPORT_MS: u32 = 250;

/// PDM window length in i16 samples (`--features mic` image).
pub const PCM_WINDOW_SAMPLES: usize = 256;

/// Samples per `pcm` UART row.
pub const PCM_ROW_SAMPLES: usize = 16;

/// Nominal buzzer tone on AI Voice capture, in Hz (LEDC 1 kHz).
pub const BUZZER_TONE_HZ: u32 = 1000;

/// How long the AI Voice buzzer tone stays on, in milliseconds.
pub const BUZZER_TONE_MS: u32 = 400;

/// PDM windows to dump after the tone starts.
pub const TONE_DUMP_WINDOWS: u32 = 2;

/// Wi-Fi / BLE scan period, in seconds (`--features radio` image).
pub const RADIO_REPORT_SECS: u32 = 10;

/// How long `/CE` stays enabled on `--features charge`, in milliseconds.
///
/// Includes [`CHARGE_SETTLE_MS`]. Default images never enable.
pub const CHARGE_PULSE_MS: u32 = 2000;

/// Wait after enable or disable before reading STAT / gauge current, in
/// milliseconds. On a physical unit, STAT after disable was still low without this.
pub const CHARGE_SETTLE_MS: u32 = 200;

/// Max SSID or BLE local-name characters after sanitize.
pub const RADIO_LABEL_MAX: usize = 24;

/// Silicon maximum concurrent touches (GT911 Rev.09 §1). This FPC delivers 5.
pub const MAX_TOUCH_POINTS: usize = 5;

/// Bytes reserved for any event line, including five touch points.
pub const LINE_CAPACITY: usize = 160;

/// Bytes reserved for a git identity line (`git=<40 hex> dirty=0`).
pub const GIT_CAPACITY: usize = 80;

/// Bytes reserved for the boot latch line.
pub const LATCHED_CAPACITY: usize = 32;

mod idle;

pub use idle::IdleListen;

/// Why a format into a caller buffer failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatError {
    /// The buffer was shorter than the formatted line.
    Truncated,
}

/// Enclosure pose labels. These match the board classifier names and are not
/// a `Debug` dump.
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
    /// Lying face up. Embassy-debug still draws the USB-down portrait page.
    FaceUp,
    /// Lying face down. Embassy-debug still draws the USB-down portrait page.
    FaceDown,
}

impl ImuPose {
    /// Token written after `imu=` in a report line.
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

/// One mapped touch sample on the 800×480 screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TouchPoint {
    /// Screen X in pixels.
    pub x: u16,
    /// Screen Y in pixels.
    pub y: u16,
}

/// Pages the panel can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    /// Cold-boot title card: Ferris, `sticky-rs`, then a smaller hint.
    /// Drawn USB-down portrait (also used for FaceUp / FaceDown).
    Splash,
    /// Geometric shapes.
    Shapes,
    /// Button legend.
    Legend,
    /// Four boxes, one OTP gray level each.
    Tones,
}

impl Scene {
    /// Cycle order for Page Up / Page Down.
    pub const ALL: [Self; 4] = [Self::Splash, Self::Shapes, Self::Legend, Self::Tones];

    /// Token written after `scene=`.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Splash => "splash",
            Self::Shapes => "shapes",
            Self::Legend => "legend",
            Self::Tones => "tones",
        }
    }

    /// Packed byte for RTC-persistent resume. Not a UART token.
    #[inline]
    #[must_use]
    pub const fn persist_byte(self) -> u8 {
        match self {
            Self::Splash => 0,
            Self::Shapes => 1,
            Self::Legend => 2,
            Self::Tones => 3,
        }
    }

    /// Inverse of [`Self::persist_byte`].
    #[inline]
    #[must_use]
    pub const fn from_persist_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Splash),
            1 => Some(Self::Shapes),
            2 => Some(Self::Legend),
            3 => Some(Self::Tones),
            _ => None,
        }
    }

    /// Next scene, wrapping.
    #[inline]
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Splash => Self::Shapes,
            Self::Shapes => Self::Legend,
            Self::Legend => Self::Tones,
            Self::Tones => Self::Splash,
        }
    }

    /// Previous scene, wrapping.
    #[inline]
    #[must_use]
    pub const fn prev(self) -> Self {
        match self {
            Self::Splash => Self::Tones,
            Self::Shapes => Self::Splash,
            Self::Legend => Self::Shapes,
            Self::Tones => Self::Legend,
        }
    }
}

/// How long Page Down must stay low to request deep sleep, in milliseconds.
pub const PAGE_DOWN_SLEEP_MS: u32 = 4_000;

/// How long Page Down must stay low after a deep-sleep wake to resume, in
/// milliseconds.
pub const PAGE_DOWN_RESUME_MS: u32 = 1_000;

/// How long Page Up must stay low to run panel standby then resume, in
/// milliseconds.
pub const PAGE_UP_STANDBY_MS: u32 = 2_000;

/// How long the glass sits after panel `standby()` so a human can
/// confirm the last card stayed.
pub const STANDBY_LOOK_MS: u32 = 2_000;

/// Page Down hold while awake: short press versus sleep request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepHold {
    /// Still low, under [`PAGE_DOWN_SLEEP_MS`].
    Waiting,
    /// Released before the sleep threshold.
    Short,
    /// Held through [`PAGE_DOWN_SLEEP_MS`].
    RequestSleep,
}

/// Classifies an awake Page Down hold.
#[inline]
#[must_use]
pub const fn classify_sleep_hold(held_ms: u32, still_low: bool) -> SleepHold {
    if still_low {
        if held_ms >= PAGE_DOWN_SLEEP_MS {
            SleepHold::RequestSleep
        } else {
            SleepHold::Waiting
        }
    } else if held_ms < PAGE_DOWN_SLEEP_MS {
        SleepHold::Short
    } else {
        SleepHold::RequestSleep
    }
}

/// Page Down hold after `ext1` wake: resume versus go back to sleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeHold {
    /// Still low, under [`PAGE_DOWN_RESUME_MS`].
    Waiting,
    /// Held through [`PAGE_DOWN_RESUME_MS`].
    Ready,
    /// Released before the resume threshold.
    Abort,
}

/// Page Up hold while awake: short press versus panel standby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandbyHold {
    /// Still low, under [`PAGE_UP_STANDBY_MS`].
    Waiting,
    /// Released before the standby threshold.
    Short,
    /// Held through [`PAGE_UP_STANDBY_MS`].
    RequestStandby,
}

/// Classifies an awake Page Up hold.
#[inline]
#[must_use]
pub const fn classify_standby_hold(held_ms: u32, still_low: bool) -> StandbyHold {
    if still_low {
        if held_ms >= PAGE_UP_STANDBY_MS {
            StandbyHold::RequestStandby
        } else {
            StandbyHold::Waiting
        }
    } else if held_ms < PAGE_UP_STANDBY_MS {
        StandbyHold::Short
    } else {
        StandbyHold::RequestStandby
    }
}

/// Classifies a post-wake Page Down hold.
#[inline]
#[must_use]
pub const fn classify_resume_hold(held_ms: u32, still_low: bool) -> ResumeHold {
    if !still_low {
        ResumeHold::Abort
    } else if held_ms >= PAGE_DOWN_RESUME_MS {
        ResumeHold::Ready
    } else {
        ResumeHold::Waiting
    }
}

/// A timestamped event the firmware sends to the log task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Active-low button edge. `down` is true on press (`1 -> 0`).
    Button {
        /// Milliseconds since boot.
        t_ms: u32,
        /// ESP32-S3 GPIO number (4, 5, or 6).
        gpio: u8,
        /// `true` when the key went low.
        down: bool,
    },
    /// Read-only GT911 status byte (`Register::Status` / `StatusBits`).
    /// Printed when board `touch::STATUS_HEARTBEAT` is on.
    Gt911Status {
        /// Milliseconds since boot.
        t_ms: u32,
        /// Raw status byte.
        status: u8,
    },
    /// GT911 contact set, already mapped onto the 800×480 screen.
    Touch {
        /// Milliseconds since boot.
        t_ms: u32,
        /// How many of [`Event::Touch::points`] are valid (0..=5 on this FPC).
        n: u8,
        /// Screen-space points. Only the first `n` entries are printed.
        points: [TouchPoint; MAX_TOUCH_POINTS],
    },
    /// Periodic accelerometer sample.
    Imu {
        /// Milliseconds since boot.
        t_ms: u32,
        /// Classified pose, or `None` when no axis dominates.
        pose: Option<ImuPose>,
        /// Raw X LSB.
        x: i16,
        /// Raw Y LSB.
        y: i16,
        /// Raw Z LSB.
        z: i16,
    },
    /// Wi-Fi scan finished (`--features radio` image). `n` is how many
    /// APs the scan returned, not how many lines follow.
    Wifi {
        /// Milliseconds since boot.
        t_ms: u32,
        /// Access points the scan returned.
        n: u8,
    },
    /// BLE scan window (`--features radio` image). `n` is reports heard,
    /// not how many named lines follow. No addresses.
    Ble {
        /// Milliseconds since boot.
        t_ms: u32,
        /// Advertisements heard in the window.
        n: u8,
    },
    /// PDM window energy (`--features mic` image).
    Mic {
        /// Milliseconds since boot.
        t_ms: u32,
        /// Integer RMS of the i16 window.
        rms: u32,
        /// Max absolute sample in the window.
        peak: u32,
    },
    /// The panel finished a scene.
    Scene {
        /// Milliseconds since boot.
        t_ms: u32,
        /// Scene that was transmitted.
        scene: Scene,
    },
    /// The event channel dropped `dropped` events because the log task lagged.
    Overflow {
        /// Milliseconds since boot.
        t_ms: u32,
        /// How many events were discarded.
        dropped: u32,
    },
    /// Sleep card is on the panel; MCU will enter deep sleep after release.
    Sleeping {
        /// Milliseconds since boot.
        t_ms: u32,
    },
    /// Reset reason was deep-sleep wake (before the 1 s resume hold).
    Woke {
        /// Milliseconds since this wake.
        t_ms: u32,
    },
    /// Panel `standby()` finished; analog is down, RAM kept.
    Standby {
        /// Milliseconds since boot.
        t_ms: u32,
    },
    /// Panel can take commands again after the standby sit.
    ///
    /// On this glass that may follow a hardware reset
    /// (`epd resume rst`), not a successful stock `0xC0`.
    Resumed {
        /// Milliseconds since boot.
        t_ms: u32,
    },
}

/// Writes `embassy-debug: latched` into `buf` without a trailing newline.
pub fn format_latched(buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(buf, format_args!("{LOG_PREFIX}: latched"))
}

/// Writes `embassy-debug: sd cd=<0|1>` (`0` = inserted).
pub fn format_sd_cd(inserted: bool, buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(
        buf,
        format_args!("{}: sd cd={}", LOG_PREFIX, u8::from(!inserted)),
    )
}

/// Writes `embassy-debug: sd none <reason>` (`empty`, `timeout`, `nak`).
pub fn format_sd_none<'a>(reason: &str, buf: &'a mut [u8]) -> Result<&'a str, FormatError> {
    write_into(buf, format_args!("{LOG_PREFIX}: sd none {reason}"))
}

/// Writes `embassy-debug: sd hz=<n> type=<sdsc|sdhc> mid=0xNN name=<name>`.
///
/// `name` is already sanitized. Never a CID product serial.
pub fn format_sd_id<'a>(
    hz: u32,
    kind: &str,
    mid: u8,
    name: &str,
    buf: &'a mut [u8],
) -> Result<&'a str, FormatError> {
    write_into(
        buf,
        format_args!("{LOG_PREFIX}: sd hz={hz} type={kind} mid={mid:#04x} name={name}"),
    )
}

/// Writes `embassy-debug: sd vol=<n>`.
pub fn format_sd_vol(idx: u8, buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(buf, format_args!("{LOG_PREFIX}: sd vol={idx}"))
}

/// Writes `embassy-debug: sd dir n=<n>`.
pub fn format_sd_dir(n: u8, buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(buf, format_args!("{LOG_PREFIX}: sd dir n={n}"))
}

/// Writes `embassy-debug: sd ent name=<name> dir` or `… bytes=<n>`.
///
/// `name` is already sanitized. Never file contents.
pub fn format_sd_ent<'a>(
    name: &str,
    bytes: Option<u32>,
    buf: &'a mut [u8],
) -> Result<&'a str, FormatError> {
    match bytes {
        None => write_into(buf, format_args!("{LOG_PREFIX}: sd ent name={name} dir")),
        Some(n) => write_into(
            buf,
            format_args!("{LOG_PREFIX}: sd ent name={name} bytes={n}"),
        ),
    }
}

/// Writes `embassy-debug: sd read name=<name> n=<n>` after a ReadOnly read.
pub fn format_sd_read<'a>(name: &str, n: u32, buf: &'a mut [u8]) -> Result<&'a str, FormatError> {
    write_into(buf, format_args!("{LOG_PREFIX}: sd read name={name} n={n}"))
}

/// Writes `embassy-debug: sd hz=<n> ack` or `… nak`.
pub fn format_sd_ack(hz: u32, ok: bool, buf: &mut [u8]) -> Result<&str, FormatError> {
    let token = if ok { "ack" } else { "nak" };
    write_into(buf, format_args!("{LOG_PREFIX}: sd hz={hz} {token}"))
}

/// Writes `embassy-debug: ce parked gpio40=<0|1> vbus=<0|1>` and optional `i=`.
pub fn format_ce_parked(
    gpio40_high: bool,
    vbus: bool,
    i_ma: Option<i16>,
    buf: &mut [u8],
) -> Result<&str, FormatError> {
    write_ce(
        "parked",
        u8::from(gpio40_high),
        Some(u8::from(vbus)),
        i_ma,
        buf,
    )
}

/// Writes `embassy-debug: ce on gpio40=<0|1>` and optional `i=`.
pub fn format_ce_on(
    gpio40_high: bool,
    i_ma: Option<i16>,
    buf: &mut [u8],
) -> Result<&str, FormatError> {
    write_ce("on", u8::from(gpio40_high), None, i_ma, buf)
}

/// Writes `embassy-debug: ce off gpio40=<0|1>` and optional `i=`.
pub fn format_ce_off(
    gpio40_high: bool,
    i_ma: Option<i16>,
    buf: &mut [u8],
) -> Result<&str, FormatError> {
    write_ce("off", u8::from(gpio40_high), None, i_ma, buf)
}

/// Writes `embassy-debug: ce skip no-vbus`.
pub fn format_ce_skip(buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(buf, format_args!("{LOG_PREFIX}: ce skip no-vbus"))
}

fn write_ce<'a>(
    phase: &str,
    gpio40: u8,
    vbus: Option<u8>,
    i_ma: Option<i16>,
    buf: &'a mut [u8],
) -> Result<&'a str, FormatError> {
    match (vbus, i_ma) {
        (Some(vbus), Some(i_ma)) => write_into(
            buf,
            format_args!("{LOG_PREFIX}: ce {phase} gpio40={gpio40} vbus={vbus} i={i_ma}"),
        ),
        (Some(vbus), None) => write_into(
            buf,
            format_args!("{LOG_PREFIX}: ce {phase} gpio40={gpio40} vbus={vbus}"),
        ),
        (None, Some(i_ma)) => write_into(
            buf,
            format_args!("{LOG_PREFIX}: ce {phase} gpio40={gpio40} i={i_ma}"),
        ),
        (None, None) => write_into(
            buf,
            format_args!("{LOG_PREFIX}: ce {phase} gpio40={gpio40}"),
        ),
    }
}

/// Writes `embassy-debug: git=<hash> dirty=<0|1>` into `buf` without a trailing newline.
pub fn format_git<'a>(hash: &str, dirty: bool, buf: &'a mut [u8]) -> Result<&'a str, FormatError> {
    write_into(
        buf,
        format_args!("{}: git={hash} dirty={}", LOG_PREFIX, u8::from(dirty)),
    )
}

/// Writes `event` into `buf` without a trailing newline.
pub fn format_event<'a>(event: &Event, buf: &'a mut [u8]) -> Result<&'a str, FormatError> {
    match *event {
        Event::Button { t_ms, gpio, down } => {
            let edge = if down { "down" } else { "up" };
            write_into(
                buf,
                format_args!("{LOG_PREFIX}: t={t_ms} btn {gpio} {edge}"),
            )
        }
        Event::Gt911Status { t_ms, status } => write_into(
            buf,
            format_args!("{LOG_PREFIX}: t={t_ms} gt911 st={status:#04x}"),
        ),
        Event::Touch { t_ms, n, points } => format_touch(t_ms, n, &points, buf),
        Event::Imu {
            t_ms,
            pose,
            x,
            y,
            z,
        } => {
            let imu = pose.map_or("none", ImuPose::as_str);
            write_into(
                buf,
                format_args!("{LOG_PREFIX}: t={t_ms} imu={imu} x={x} y={y} z={z}"),
            )
        }
        Event::Wifi { t_ms, n } => {
            write_into(buf, format_args!("{LOG_PREFIX}: t={t_ms} wifi n={n}"))
        }
        Event::Ble { t_ms, n } => write_into(buf, format_args!("{LOG_PREFIX}: t={t_ms} ble n={n}")),
        Event::Mic { t_ms, rms, peak } => write_into(
            buf,
            format_args!("{LOG_PREFIX}: t={t_ms} mic rms={rms} peak={peak}"),
        ),
        Event::Scene { t_ms, scene } => write_into(
            buf,
            format_args!("{}: t={t_ms} scene={}", LOG_PREFIX, scene.as_str()),
        ),
        Event::Overflow { t_ms, dropped } => {
            write_into(buf, format_args!("{LOG_PREFIX}: t={t_ms} drop={dropped}"))
        }
        Event::Sleeping { t_ms } => {
            write_into(buf, format_args!("{LOG_PREFIX}: t={t_ms} scene=sleeping"))
        }
        Event::Woke { t_ms } => write_into(buf, format_args!("{LOG_PREFIX}: t={t_ms} woke")),
        Event::Standby { t_ms } => write_into(buf, format_args!("{LOG_PREFIX}: t={t_ms} standby")),
        Event::Resumed { t_ms } => write_into(buf, format_args!("{LOG_PREFIX}: t={t_ms} resume")),
    }
}

/// Writes `embassy-debug: sleeping` (MCU about to `sleep_deep`).
pub fn format_sleeping(buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(buf, format_args!("{LOG_PREFIX}: sleeping"))
}

/// Integer RMS and peak of a PCM window. Empty window is `(0, 0)`.
#[must_use]
pub fn pcm_energy(samples: &[i16]) -> (u32, u32) {
    if samples.is_empty() {
        return (0, 0);
    }
    let mut sum_sq: u64 = 0;
    let mut peak: u32 = 0;
    for &sample in samples {
        let abs = u32::from(sample.unsigned_abs());
        if abs > peak {
            peak = abs;
        }
        sum_sq += u64::from(abs) * u64::from(abs);
    }
    let mean_sq = sum_sq / samples.len() as u64;
    (isqrt_u64(mean_sq) as u32, peak)
}

/// Header before a PCM dump (`mic pcm hz=… n=…`).
pub fn format_mic_pcm_header(
    t_ms: u32,
    hz: u32,
    n: usize,
    buf: &mut [u8],
) -> Result<&str, FormatError> {
    write_into(
        buf,
        format_args!("{LOG_PREFIX}: t={t_ms} mic pcm hz={hz} n={n}"),
    )
}

/// Sanitize an SSID or BLE local name for UART. ASCII printable except
/// `=` and space become `_`; longer labels are truncated. Never a MAC.
#[must_use]
pub fn sanitize_radio_label<'a>(raw: &[u8], out: &'a mut [u8; RADIO_LABEL_MAX]) -> &'a str {
    let mut n = 0;
    for &byte in raw {
        if n == RADIO_LABEL_MAX {
            break;
        }
        out[n] = if byte.is_ascii_graphic() && byte != b'=' {
            byte
        } else {
            b'_'
        };
        n += 1;
    }
    // Empty or all-replaced still prints `?` so the line is visible.
    if n == 0 {
        out[0] = b'?';
        n = 1;
    }
    str::from_utf8(&out[..n]).unwrap_or("?")
}

/// One Wi-Fi AP (`wifi ssid=… rssi=…`). `ssid` is already sanitized.
pub fn format_wifi_ssid<'a>(
    t_ms: u32,
    ssid: &str,
    rssi: i8,
    buf: &'a mut [u8],
) -> Result<&'a str, FormatError> {
    write_into(
        buf,
        format_args!("{LOG_PREFIX}: t={t_ms} wifi ssid={ssid} rssi={rssi}"),
    )
}

/// One BLE advertisement (`ble name=… rssi=…`). `name` is already sanitized.
/// No address field.
pub fn format_ble_name<'a>(
    t_ms: u32,
    name: &str,
    rssi: i8,
    buf: &'a mut [u8],
) -> Result<&'a str, FormatError> {
    write_into(
        buf,
        format_args!("{LOG_PREFIX}: t={t_ms} ble name={name} rssi={rssi}"),
    )
}

/// One row of signed i16 samples (`pcm <offset> s0 s1 …`).
pub fn format_mic_pcm_row<'a>(
    offset: usize,
    samples: &[i16],
    buf: &'a mut [u8],
) -> Result<&'a str, FormatError> {
    let pos = {
        let mut writer = SliceWriter { buf, pos: 0 };
        writer
            .write_fmt(format_args!("{LOG_PREFIX}: pcm {offset:03}"))
            .map_err(|_| FormatError::Truncated)?;
        for sample in samples {
            writer
                .write_fmt(format_args!(" {sample}"))
                .map_err(|_| FormatError::Truncated)?;
        }
        writer.pos
    };
    str::from_utf8(&buf[..pos]).map_err(|_| FormatError::Truncated)
}

const fn isqrt_u64(value: u64) -> u64 {
    if value == 0 {
        return 0;
    }
    let mut x = value;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

fn format_touch<'a>(
    t_ms: u32,
    n: u8,
    points: &[TouchPoint; MAX_TOUCH_POINTS],
    buf: &'a mut [u8],
) -> Result<&'a str, FormatError> {
    let n = core::cmp::min(n as usize, MAX_TOUCH_POINTS);
    match n {
        0 => write_into(buf, format_args!("{LOG_PREFIX}: t={t_ms} touch n=0")),
        1 => write_into(
            buf,
            format_args!(
                "{LOG_PREFIX}: t={t_ms} touch n=1 p0={},{}",
                points[0].x, points[0].y
            ),
        ),
        2 => write_into(
            buf,
            format_args!(
                "{LOG_PREFIX}: t={t_ms} touch n=2 p0={},{} p1={},{}",
                points[0].x, points[0].y, points[1].x, points[1].y
            ),
        ),
        3 => write_into(
            buf,
            format_args!(
                "{LOG_PREFIX}: t={t_ms} touch n=3 p0={},{} p1={},{} p2={},{}",
                points[0].x, points[0].y, points[1].x, points[1].y, points[2].x, points[2].y
            ),
        ),
        4 => write_into(
            buf,
            format_args!(
                "{LOG_PREFIX}: t={t_ms} touch n=4 p0={},{} p1={},{} p2={},{} p3={},{}",
                points[0].x,
                points[0].y,
                points[1].x,
                points[1].y,
                points[2].x,
                points[2].y,
                points[3].x,
                points[3].y
            ),
        ),
        _ => write_into(
            buf,
            format_args!(
                "{LOG_PREFIX}: t={t_ms} touch n=5 p0={},{} p1={},{} p2={},{} p3={},{} p4={},{}",
                points[0].x,
                points[0].y,
                points[1].x,
                points[1].y,
                points[2].x,
                points[2].y,
                points[3].x,
                points[3].y,
                points[4].x,
                points[4].y
            ),
        ),
    }
}

fn write_into<'a>(buf: &'a mut [u8], args: fmt::Arguments<'_>) -> Result<&'a str, FormatError> {
    let mut tmp = [0u8; LINE_CAPACITY];
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

    fn line(event: &Event) -> String {
        let mut buf = [0u8; LINE_CAPACITY];
        String::from(format_event(event, &mut buf).unwrap())
    }

    #[test]
    fn boot_lines_match_the_agreed_shape() {
        let mut buf = [0u8; LATCHED_CAPACITY];
        assert_eq!(format_latched(&mut buf).unwrap(), "embassy-debug: latched");
        let mut buf = [0u8; GIT_CAPACITY];
        assert_eq!(
            format_git("deadbeef", false, &mut buf).unwrap(),
            "embassy-debug: git=deadbeef dirty=0"
        );
    }

    #[test]
    fn button_lines_include_the_timestamp() {
        assert_eq!(
            line(&Event::Button {
                t_ms: 1204,
                gpio: 4,
                down: true,
            }),
            "embassy-debug: t=1204 btn 4 down"
        );
        assert_eq!(
            line(&Event::Button {
                t_ms: 1410,
                gpio: 4,
                down: false,
            }),
            "embassy-debug: t=1410 btn 4 up"
        );
    }

    #[test]
    fn gt911_status_is_a_read_only_hex_byte() {
        assert_eq!(
            line(&Event::Gt911Status {
                t_ms: 2100,
                status: 0x00,
            }),
            "embassy-debug: t=2100 gt911 st=0x00"
        );
        assert_eq!(
            line(&Event::Gt911Status {
                t_ms: 2180,
                status: 0x81,
            }),
            "embassy-debug: t=2180 gt911 st=0x81"
        );
    }

    #[test]
    fn touch_zero_contacts_omits_points() {
        assert_eq!(
            line(&Event::Touch {
                t_ms: 2180,
                n: 0,
                points: [TouchPoint::default(); MAX_TOUCH_POINTS],
            }),
            "embassy-debug: t=2180 touch n=0"
        );
    }

    #[test]
    fn touch_one_point_matches_the_agreed_shape() {
        let mut points = [TouchPoint::default(); MAX_TOUCH_POINTS];
        points[0] = TouchPoint { x: 123, y: 456 };
        assert_eq!(
            line(&Event::Touch {
                t_ms: 2100,
                n: 1,
                points,
            }),
            "embassy-debug: t=2100 touch n=1 p0=123,456"
        );
    }

    #[test]
    fn imu_none_still_prints_the_sample() {
        assert_eq!(
            line(&Event::Imu {
                t_ms: 5000,
                pose: None,
                x: 800,
                y: 400,
                z: 200,
            }),
            "embassy-debug: t=5000 imu=none x=800 y=400 z=200"
        );
        assert_eq!(
            line(&Event::Imu {
                t_ms: 5000,
                pose: Some(ImuPose::FaceUp),
                x: 12,
                y: -30,
                z: 16300,
            }),
            "embassy-debug: t=5000 imu=FaceUp x=12 y=-30 z=16300"
        );
    }

    #[test]
    fn imu_period_is_five_seconds() {
        assert_eq!(IMU_REPORT_SECS, 5);
    }

    #[test]
    fn sd_identify_lines_omit_a_card_serial() {
        let mut buf = [0u8; LINE_CAPACITY];
        assert_eq!(
            format_sd_cd(true, &mut buf).unwrap(),
            "embassy-debug: sd cd=0"
        );
        assert_eq!(
            format_sd_cd(false, &mut buf).unwrap(),
            "embassy-debug: sd cd=1"
        );
        assert_eq!(
            format_sd_none("empty", &mut buf).unwrap(),
            "embassy-debug: sd none empty"
        );
        assert_eq!(
            format_sd_id(400_000, "sdhc", 0x03, "SC16G", &mut buf).unwrap(),
            "embassy-debug: sd hz=400000 type=sdhc mid=0x03 name=SC16G"
        );
        assert!(!format_sd_id(400_000, "sdhc", 0x03, "SC16G", &mut buf)
            .unwrap()
            .contains("serial"));
        assert_eq!(
            format_sd_ack(10_000_000, true, &mut buf).unwrap(),
            "embassy-debug: sd hz=10000000 ack"
        );
        assert_eq!(
            format_sd_ack(20_000_000, false, &mut buf).unwrap(),
            "embassy-debug: sd hz=20000000 nak"
        );
        assert_eq!(
            format_sd_vol(0, &mut buf).unwrap(),
            "embassy-debug: sd vol=0"
        );
        assert_eq!(
            format_sd_dir(2, &mut buf).unwrap(),
            "embassy-debug: sd dir n=2"
        );
        assert_eq!(
            format_sd_ent("FOO.TXT", Some(12), &mut buf).unwrap(),
            "embassy-debug: sd ent name=FOO.TXT bytes=12"
        );
        assert_eq!(
            format_sd_ent("BAR", None, &mut buf).unwrap(),
            "embassy-debug: sd ent name=BAR dir"
        );
        assert_eq!(
            format_sd_read("FOO.TXT", 12, &mut buf).unwrap(),
            "embassy-debug: sd read name=FOO.TXT n=12"
        );
    }

    #[test]
    fn charge_lines_match_the_agreed_shape() {
        let mut buf = [0u8; LINE_CAPACITY];
        assert_eq!(
            format_ce_parked(true, true, Some(0), &mut buf).unwrap(),
            "embassy-debug: ce parked gpio40=1 vbus=1 i=0"
        );
        assert_eq!(
            format_ce_parked(true, false, None, &mut buf).unwrap(),
            "embassy-debug: ce parked gpio40=1 vbus=0"
        );
        assert_eq!(
            format_ce_on(false, Some(-12), &mut buf).unwrap(),
            "embassy-debug: ce on gpio40=0 i=-12"
        );
        assert_eq!(
            format_ce_off(true, Some(4), &mut buf).unwrap(),
            "embassy-debug: ce off gpio40=1 i=4"
        );
        assert_eq!(
            format_ce_skip(&mut buf).unwrap(),
            "embassy-debug: ce skip no-vbus"
        );
        assert!(CHARGE_PULSE_MS > CHARGE_SETTLE_MS);
        assert_eq!(CHARGE_PULSE_MS, 2000);
        assert_eq!(CHARGE_SETTLE_MS, 200);
    }

    #[test]
    fn radio_lines_match_the_agreed_shape() {
        assert_eq!(
            line(&Event::Wifi { t_ms: 1204, n: 3 }),
            "embassy-debug: t=1204 wifi n=3"
        );
        assert_eq!(
            line(&Event::Ble { t_ms: 1204, n: 2 }),
            "embassy-debug: t=1204 ble n=2"
        );
        let mut buf = [0u8; LINE_CAPACITY];
        assert_eq!(
            format_wifi_ssid(1204, "Home", -42, &mut buf).unwrap(),
            "embassy-debug: t=1204 wifi ssid=Home rssi=-42"
        );
        assert_eq!(
            format_ble_name(1204, "Phone", -70, &mut buf).unwrap(),
            "embassy-debug: t=1204 ble name=Phone rssi=-70"
        );
        assert_eq!(RADIO_REPORT_SECS, 10);
        assert_eq!(RADIO_LABEL_MAX, 24);
        let mut label = [0u8; RADIO_LABEL_MAX];
        assert_eq!(
            sanitize_radio_label(b"Home Net=1", &mut label),
            "Home_Net_1"
        );
        assert_eq!(sanitize_radio_label(b"", &mut label), "?");
        let text = format_wifi_ssid(1, "Home", -1, &mut buf).unwrap();
        let lower = text.to_ascii_lowercase();
        assert!(!lower.contains("mac"));
        assert!(!lower.contains("bssid"));
    }

    #[test]
    fn mic_line_matches_the_agreed_shape() {
        assert_eq!(
            line(&Event::Mic {
                t_ms: 1204,
                rms: 12,
                peak: 40,
            }),
            "embassy-debug: t=1204 mic rms=12 peak=40"
        );
        assert_eq!(MIC_REPORT_MS, 250);
        assert_eq!(PCM_WINDOW_SAMPLES, 256);
        assert_eq!(PCM_ROW_SAMPLES, 16);
        assert_eq!(BUZZER_TONE_HZ, 1000);
        assert_eq!(BUZZER_TONE_MS, 400);
        assert_eq!(TONE_DUMP_WINDOWS, 2);
    }

    #[test]
    fn mic_pcm_dump_lines_match_the_agreed_shape() {
        let mut buf = [0u8; LINE_CAPACITY];
        assert_eq!(
            format_mic_pcm_header(1204, 1000, 256, &mut buf).unwrap(),
            "embassy-debug: t=1204 mic pcm hz=1000 n=256"
        );
        let row = [120i16, -30, 400, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        assert_eq!(
            format_mic_pcm_row(0, &row, &mut buf).unwrap(),
            "embassy-debug: pcm 000 120 -30 400 0 1 2 3 4 5 6 7 8 9 10 11 12"
        );
    }

    #[test]
    fn pcm_energy_is_rms_and_peak() {
        assert_eq!(pcm_energy(&[]), (0, 0));
        assert_eq!(pcm_energy(&[0, 0, 0, 0]), (0, 0));
        assert_eq!(pcm_energy(&[3, -4]), (3, 4));
        assert_eq!(pcm_energy(&[i16::MIN]), (32768, 32768));
    }

    #[test]
    fn scene_wraps_in_both_directions() {
        assert_eq!(Scene::Splash.next(), Scene::Shapes);
        assert_eq!(Scene::Legend.next(), Scene::Tones);
        assert_eq!(Scene::Tones.next(), Scene::Splash);
        assert_eq!(Scene::Splash.prev(), Scene::Tones);
        assert_eq!(
            line(&Event::Scene {
                t_ms: 9,
                scene: Scene::Shapes,
            }),
            "embassy-debug: t=9 scene=shapes"
        );
        assert_eq!(
            line(&Event::Scene {
                t_ms: 11,
                scene: Scene::Tones,
            }),
            "embassy-debug: t=11 scene=tones"
        );
    }

    #[test]
    fn page_down_holds_classify_short_sleep_and_resume() {
        assert_eq!(PAGE_DOWN_SLEEP_MS, 4_000);
        assert_eq!(PAGE_DOWN_RESUME_MS, 1_000);
        assert_eq!(classify_sleep_hold(20, true), SleepHold::Waiting);
        assert_eq!(classify_sleep_hold(20, false), SleepHold::Short);
        assert_eq!(
            classify_sleep_hold(PAGE_DOWN_SLEEP_MS, true),
            SleepHold::RequestSleep
        );
        assert_eq!(
            classify_sleep_hold(PAGE_DOWN_SLEEP_MS, false),
            SleepHold::RequestSleep
        );
        assert_eq!(classify_resume_hold(20, true), ResumeHold::Waiting);
        assert_eq!(classify_resume_hold(20, false), ResumeHold::Abort);
        assert_eq!(
            classify_resume_hold(PAGE_DOWN_RESUME_MS, true),
            ResumeHold::Ready
        );
        assert_eq!(PAGE_UP_STANDBY_MS, 2_000);
        assert_eq!(STANDBY_LOOK_MS, 2_000);
        assert_eq!(classify_standby_hold(20, true), StandbyHold::Waiting);
        assert_eq!(classify_standby_hold(20, false), StandbyHold::Short);
        assert_eq!(
            classify_standby_hold(PAGE_UP_STANDBY_MS, true),
            StandbyHold::RequestStandby
        );
    }

    #[test]
    fn scene_persist_round_trips() {
        for scene in Scene::ALL {
            assert_eq!(Scene::from_persist_byte(scene.persist_byte()), Some(scene));
        }
        assert_eq!(Scene::from_persist_byte(9), None);
    }

    #[test]
    fn sleep_and_wake_lines_match_the_agreed_shape() {
        assert_eq!(
            line(&Event::Sleeping { t_ms: 9 }),
            "embassy-debug: t=9 scene=sleeping"
        );
        assert_eq!(line(&Event::Woke { t_ms: 11 }), "embassy-debug: t=11 woke");
        assert_eq!(
            line(&Event::Standby { t_ms: 13 }),
            "embassy-debug: t=13 standby"
        );
        assert_eq!(
            line(&Event::Resumed { t_ms: 15 }),
            "embassy-debug: t=15 resume"
        );
        let mut buf = [0u8; LINE_CAPACITY];
        assert_eq!(
            format_sleeping(&mut buf).unwrap(),
            "embassy-debug: sleeping"
        );
    }

    #[test]
    fn overflow_reports_how_many_were_dropped() {
        assert_eq!(
            line(&Event::Overflow {
                t_ms: 10,
                dropped: 3,
            }),
            "embassy-debug: t=10 drop=3"
        );
    }

    #[test]
    fn worst_case_touch_fits_the_reserved_buffer() {
        let points = [TouchPoint {
            x: u16::MAX,
            y: u16::MAX,
        }; MAX_TOUCH_POINTS];
        let mut buf = [0u8; LINE_CAPACITY];
        let text = format_event(
            &Event::Touch {
                t_ms: u32::MAX,
                n: 5,
                points,
            },
            &mut buf,
        )
        .unwrap();
        assert!(text.len() < LINE_CAPACITY);
        assert!(text.starts_with("embassy-debug:"));
    }

    #[test]
    fn touch_capacity_matches_the_board_crate() {
        assert_eq!(
            MAX_TOUCH_POINTS,
            seeed_reterminal_sticky::touch::MAX_TOUCH_POINTS as usize
        );
        assert_eq!(
            seeed_reterminal_sticky::touch::STATUS_HEARTBEAT,
            seeed_reterminal_sticky::touch::StatusHeartbeat::EverySecs(10)
        );
    }

    #[test]
    fn a_short_buffer_is_an_error_not_a_clipped_prefix() {
        let mut buf = [0u8; 8];
        assert_eq!(
            format_event(
                &Event::Button {
                    t_ms: 1,
                    gpio: 4,
                    down: true,
                },
                &mut buf
            ),
            Err(FormatError::Truncated)
        );
        assert_eq!(&buf, &[0; 8]);
    }

    #[test]
    fn lines_have_no_identifier_fields() {
        let text = line(&Event::Imu {
            t_ms: 1,
            pose: Some(ImuPose::Landscape180),
            x: -1,
            y: -2,
            z: -3,
        });
        let lower = text.to_ascii_lowercase();
        assert!(!lower.contains("serial"));
        assert!(!lower.contains("mac"));
    }
}
