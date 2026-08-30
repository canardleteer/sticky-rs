//! UART event lines for the Sticky embassy-debug image.
//!
//! The firmware owns buses, pins, and the Embassy tasks. This crate owns the
//! **strings** it prints so the log contract can be tested on the host:
//! timestamped button, touch, and IMU lines, and no factory serial / USB
//! serial / MAC fields.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::fmt::{self, Write};
use core::str;

/// Token before every UART line (`embassy-debug: …`).
pub const LOG_PREFIX: &str = "embassy-debug";

/// Default IMU report period, in seconds.
pub const IMU_REPORT_SECS: u32 = 5;

/// Silicon maximum concurrent touches (GT911 Rev.09 §1).
pub const MAX_TOUCH_POINTS: usize = 5;

/// Bytes reserved for any event line, including five touch points.
pub const LINE_CAPACITY: usize = 160;

/// Bytes reserved for a git identity line (`git=<40 hex> dirty=0`).
pub const GIT_CAPACITY: usize = 80;

/// Bytes reserved for the boot latch line.
pub const LATCHED_CAPACITY: usize = 32;

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
    /// Gravity dominant on +X.
    Portrait0,
    /// Gravity dominant on -X.
    Portrait180,
    /// Gravity dominant on -Y.
    Landscape0,
    /// Gravity dominant on +Y.
    Landscape180,
    /// Gravity dominant on +Z.
    FaceUp,
    /// Gravity dominant on -Z.
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
    /// GT911 contact set, already mapped onto the 800×480 screen.
    Touch {
        /// Milliseconds since boot.
        t_ms: u32,
        /// How many of [`Event::Touch::points`] are valid (0..=5).
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
}

/// Writes `embassy-debug: latched` into `buf` without a trailing newline.
pub fn format_latched(buf: &mut [u8]) -> Result<&str, FormatError> {
    write_into(buf, format_args!("{LOG_PREFIX}: latched"))
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
        Event::Scene { t_ms, scene } => write_into(
            buf,
            format_args!("{}: t={t_ms} scene={}", LOG_PREFIX, scene.as_str()),
        ),
        Event::Overflow { t_ms, dropped } => {
            write_into(buf, format_args!("{LOG_PREFIX}: t={t_ms} drop={dropped}"))
        }
    }
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
