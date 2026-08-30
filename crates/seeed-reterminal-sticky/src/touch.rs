//! GT911 coordinate transform and reset sequence timings.
//!
//! The digitizer is **portrait 480x800** under a **landscape 800x480** panel,
//! and the display is transmitted with a 180-degree rotation. Two rotations
//! and a mirror is exactly the kind of arithmetic that is easy to get half
//! right, so the transform is a pure function with corner tests.
//!
//! If the display rotation changes, this transform must change with it.
//!
//! Goodix GT911 datasheet **Rev.09 (11 Mar 2015)** is cited for 8-bit I2C
//! addresses (§6.1), the 400 kbps cap, “up to 5” contacts (§1), and the
//! init window (under 200 ms). Wiring is schematic Rev 01. Rev.07 deleted
//! the register map, so [`Register`] values are on-glass `GT911_REG_*`
//! names, not a Rev.09 table. simple-debug after reset writes
//! [`StatusWrite::Clear`] at [`Register::Status`]. embassy-debug does not
//! write Status or Command at init. Neither writes config RAM. Bus:
//! [`I2C_HZ`] (simple-debug) or [`I2C_MAX_HZ`] (embassy-debug, datasheet
//! cap). Read-only `gt911 st=` cadence is [`STATUS_HEARTBEAT`].

/// Physical panel width in pixels.
pub const PANEL_WIDTH: u32 = 800;
/// Physical panel height in pixels.
pub const PANEL_HEIGHT: u32 = 480;
/// Digitizer width in its own portrait orientation.
pub const DIGITIZER_WIDTH: u32 = 480;
/// Digitizer height in its own portrait orientation.
pub const DIGITIZER_HEIGHT: u32 = 800;

/// Conservative GT911 bus clock (inside Rev.09 §6.1). embassy-debug uses
/// [`I2C_MAX_HZ`].
pub const I2C_HZ: u32 = 100_000;

/// Datasheet I2C cap (Rev.09 §6.1: “at or below 400Kbps”).
pub const I2C_MAX_HZ: u32 = 400_000;

/// Silicon maximum concurrent touches (Rev.09 §1). This FPC delivers 5.
pub const MAX_TOUCH_POINTS: u8 = 5;

/// Init including idle-capacitance self-cal (Rev.09 features / §8.6).
pub const INIT_WINDOW_MS: u32 = 200;

/// Product ID at [`Register::Id`]: four ASCII bytes (`911\0` on glass).
pub const PRODUCT_ID_LEN: usize = 4;

/// Bytes per contact at [`Register::Points`] (on-glass record length).
pub const POINT_RECORD_LEN: usize = 8;

/// X is little-endian at this offset in each [`POINT_RECORD_LEN`] record.
pub const POINT_X_OFFSET: usize = 0;

/// Y is little-endian at this offset in each [`POINT_RECORD_LEN`] record.
pub const POINT_Y_OFFSET: usize = 2;

/// Software poll while waiting for [`StatusBits::BUFFER_READY`].
pub const STATUS_POLL_MS: u64 = 8;

/// Read-only `gt911 st=` UART cadence.
///
/// Firmware that polls the GT911 consults [`STATUS_HEARTBEAT`]. The line
/// reads [`Register::Status`] and does not write it. Contact events stay
/// on change regardless of this setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusHeartbeat {
    /// No periodic status line.
    Off,
    /// Print every this many seconds (`0` is treated as [`StatusHeartbeat::Off`]).
    EverySecs(u32),
}

impl StatusHeartbeat {
    /// Seconds between status lines, or `None` when off.
    #[inline]
    #[must_use]
    pub const fn interval_secs(self) -> Option<u32> {
        match self {
            Self::Off | Self::EverySecs(0) => None,
            Self::EverySecs(secs) => Some(secs),
        }
    }

    /// Whether a periodic status line is enabled.
    #[inline]
    #[must_use]
    pub const fn is_on(self) -> bool {
        self.interval_secs().is_some()
    }
}

/// Desk-debug default. Set to [`StatusHeartbeat::Off`] to silence `gt911 st=`.
pub const STATUS_HEARTBEAT: StatusHeartbeat = StatusHeartbeat::EverySecs(10);

/// INT levels at RST (Rev.09 §6.1): low first ([`SlaveAddress::PairBaBb`]),
/// then high ([`SlaveAddress::Pair28_29`]).
pub const ADDR_SELECT_INT_HIGH_AT_RST: [bool; 2] = [false, true];

/// I2C slave pair from GT911 Rev.09 §6.1.
///
/// The datasheet names two 8-bit write/read pairs. `embedded-hal` I2C takes
/// the 7-bit form ([`SlaveAddress::seven_bit`]). INT level at RST selects
/// which pair this board latches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SlaveAddress {
    /// Datasheet pair `0x28`/`0x29`. 7-bit `0x14`. INT high at RST on this board.
    Pair28_29 = 0x28,
    /// Datasheet pair `0xBA`/`0xBB`. 7-bit `0x5D`. INT low at RST on this board.
    PairBaBb = 0xBA,
}

impl SlaveAddress {
    /// 8-bit write address from Rev.09 §6.1.
    #[inline]
    #[must_use]
    pub const fn write_8bit(self) -> u8 {
        self as u8
    }

    /// 8-bit read address from Rev.09 §6.1 (write byte with R/W = 1).
    #[inline]
    #[must_use]
    pub const fn read_8bit(self) -> u8 {
        self.write_8bit() | 1
    }

    /// 7-bit address for `embedded-hal` I2C (`write_8bit >> 1`).
    #[inline]
    #[must_use]
    pub const fn seven_bit(self) -> u8 {
        self.write_8bit() >> 1
    }

    /// INT level at RST rising that selects this pair (Rev.09 §6.1).
    ///
    /// `true` = drive INT high → [`SlaveAddress::Pair28_29`].
    /// `false` = drive INT low → [`SlaveAddress::PairBaBb`].
    #[inline]
    #[must_use]
    pub const fn int_high_at_rst(self) -> bool {
        matches!(self, Self::Pair28_29)
    }

    /// 7-bit pairs to probe after an address-select reset (Rev.09 §6.1).
    #[inline]
    #[must_use]
    pub const fn probe_order() -> [Self; 2] {
        [Self::PairBaBb, Self::Pair28_29]
    }
}

/// GT911 register **addresses** used after the INT-during-reset dance.
///
/// These are ports, not the bytes written to them. Write a [`Command`]
/// to [`Register::Command`]. Write [`StatusWrite`] to
/// [`Register::Status`]; a read of that port is [`StatusBits`].
///
/// Variant names match on-glass `GT911_REG_COMMAND`, `GT911_REG_ID`, and
/// `GT911_REG_STATUS`. Rev.09 deleted the map (Rev.07); these numbers are
/// not a datasheet table. Gesture mode still names `0x8040` as a command
/// port (Rev.09 §8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Register {
    /// `GT911_REG_COMMAND`. Command port (Rev.09 §8.1). See [`Command`].
    Command = 0x8040,
    /// `GT911_REG_ID`. Product ID, four ASCII bytes.
    Id = 0x8140,
    /// `GT911_REG_STATUS`. Buffer handshake. See [`StatusWrite`] / [`StatusBits`].
    Status = 0x814E,
    /// `GT911_REG_POINTS`. First contact record (on-glass `0x8150`).
    Points = 0x8150,
}

/// Byte written to [`Register::Command`].
///
/// This is the opcode enum for that port. Sleep / Approach encodings are
/// **not** listed: Rev.09 names those modes but not a command byte at
/// `0x8040`. Do not invent them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    /// `gt911` crate `init()`: write `0` (“switch to command mode” /
    /// “read coordinates”). **Not** in Rev.09. This board path does not
    /// send it (simple-debug does not either).
    ReadCoordinates = 0,
    /// Rev.09 §8.1 Gesture mode: write `8` to `0x8046` and then to
    /// [`Register::Command`]. Not used on this board path.
    Gesture = 8,
}

impl Command {
    /// Opcode byte on the wire.
    #[inline]
    #[must_use]
    pub const fn byte(self) -> u8 {
        self as u8
    }
}

/// Host write to [`Register::Status`].
///
/// A *read* of that port is a bitfield ([`StatusBits`]), not a mode enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StatusWrite {
    /// Handshake: host finished with the coordinate buffer (`0`).
    Clear = 0,
}

impl StatusWrite {
    /// Byte on the wire.
    #[inline]
    #[must_use]
    pub const fn byte(self) -> u8 {
        self as u8
    }
}

/// One read of [`Register::Status`].
///
/// This is a **bitfield**, not a closed mode enum. Bit 7 = new buffer and
/// bits 3–0 = contact count are crate / on-glass (`gt911` `get_num_touch_points`).
/// **Not** named in Rev.09.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusBits(pub u8);

impl StatusBits {
    /// Bit 7: crate / on-glass “buffer ready” (`NotReady` when clear).
    pub const BUFFER_READY: u8 = 0x80;
    /// Bits 3–0: crate / on-glass contact count.
    pub const COUNT_MASK: u8 = 0x0F;

    /// Wrap a status byte from the chip.
    #[inline]
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    /// True when bit 7 is set.
    #[inline]
    #[must_use]
    pub const fn buffer_ready(self) -> bool {
        self.0 & Self::BUFFER_READY != 0
    }

    /// Contact count in the low nibble (0..=15 in the field; silicon max 5).
    #[inline]
    #[must_use]
    pub const fn touch_count(self) -> u8 {
        self.0 & Self::COUNT_MASK
    }
}

/// [`StatusWrite::Clear`] as a byte. Prefer the enum at new call sites.
pub const STATUS_CLEAR: u8 = StatusWrite::Clear as u8;

/// [`Command::ReadCoordinates`] as a byte. Prefer the enum at new call sites.
///
/// The `gt911` crate `init()` writes this at [`Register::Command`]. This
/// board path does not.
pub const COMMAND_READ_COORDINATES: u8 = Command::ReadCoordinates as u8;

/// [`StatusBits::BUFFER_READY`]. Prefer [`StatusBits`] at new call sites.
pub const STATUS_BUFFER_READY: u8 = StatusBits::BUFFER_READY;

impl Register {
    /// 16-bit register address on the wire.
    #[inline]
    #[must_use]
    pub const fn addr(self) -> u16 {
        self as u16
    }

    /// High then low address byte for an I2C register pointer.
    #[inline]
    #[must_use]
    pub const fn addr_bytes(self) -> [u8; 2] {
        self.addr().to_be_bytes()
    }

    /// Address bytes plus one data byte for an I2C write.
    #[inline]
    #[must_use]
    pub const fn write_u8(self, value: u8) -> [u8; 3] {
        let [hi, lo] = self.addr_bytes();
        [hi, lo, value]
    }
}

/// Reset sequence timing that has worked on glass, in milliseconds.
///
/// 1. RST low with INT at the address-select level, hold [`RESET_HOLD_MS`].
/// 2. RST high, wait [`RESET_RELEASE_MS`].
/// 3. INT back to **floating** input (no MCU pull; ESP32-S3 GPIO21 has none
///    at reset), wait [`INT_SETTLE_MS`], then [`POST_RESET_SETTLE_MS`].
/// 4. Probe I2C. Init including self-cal is under 200 ms (GT911 Rev.09).
///
/// Power the touch rail before starting.
pub const RESET_HOLD_MS: u32 = 20;
/// See [`RESET_HOLD_MS`].
pub const RESET_RELEASE_MS: u32 = 20;
/// See [`RESET_HOLD_MS`].
pub const INT_SETTLE_MS: u32 = 80;
/// Extra wait after INT is released as input, still inside the 200 ms window.
pub const POST_RESET_SETTLE_MS: u32 = 30;

/// Address-select reset: RST low with INT driven (Rev.09 §6.1).
///
/// Extracted Rev.09 markdown has no T2/T3 numbers. These holds worked on
/// glass and stay inside [`INIT_WINDOW_MS`].
pub const ADDR_SELECT_RESET_HOLD_MS: u32 = 10;
/// RST high, INT still driven at the select level.
pub const ADDR_SELECT_RESET_RELEASE_MS: u32 = 10;
/// INT kept as an output after RST rises (address latch).
pub const ADDR_SELECT_INT_HOLD_AFTER_RST_MS: u32 = 50;
/// INT as a floating input after the driven window.
pub const ADDR_SELECT_INT_FLOAT_MS: u32 = 50;

/// Rounded integer rescale of `value` from `0..=from` onto `0..=to_max`.
#[inline]
#[must_use]
const fn scale(value: u32, from: u32, to_max: u32) -> u32 {
    (value * to_max + from / 2) / from
}

/// Maps a controller sample onto the physical 800x480 screen.
///
/// Steps, from the board contract:
///
/// 1. `portrait_x = scale(cx, W, Pw - 1)`
/// 2. `portrait_y = scale(H - min(cy, H), H, Ph - 1)`
/// 3. `fb_x = W - portrait_y - 1`
/// 4. `fb_y = portrait_x`
/// 5. `sx = W - fb_x - 1`
/// 6. `sy = H - fb_y - 1`
///
/// Steps 5 and 6 undo the display's 180-degree transmit rotation; use
/// [`to_framebuffer`] if you want the pre-rotation canvas instead.
#[must_use]
pub fn to_screen(cx: u32, cy: u32) -> (u32, u32) {
    let (fb_x, fb_y) = to_framebuffer(cx, cy);
    (PANEL_WIDTH - fb_x - 1, PANEL_HEIGHT - fb_y - 1)
}

/// Maps a controller sample onto the pre-rotation 800x480 framebuffer.
///
/// This is steps 1 to 4 of [`to_screen`]: swap-XY plus flip-both.
#[must_use]
pub fn to_framebuffer(cx: u32, cy: u32) -> (u32, u32) {
    let portrait_x = scale(cx.min(PANEL_WIDTH), PANEL_WIDTH, DIGITIZER_WIDTH - 1);
    let portrait_y = scale(
        PANEL_HEIGHT - cy.min(PANEL_HEIGHT),
        PANEL_HEIGHT,
        DIGITIZER_HEIGHT - 1,
    );

    let fb_x = PANEL_WIDTH - portrait_y.min(PANEL_WIDTH - 1) - 1;
    let fb_y = portrait_x.min(PANEL_HEIGHT - 1);
    (fb_x, fb_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_addr_bytes_are_big_endian() {
        for reg in [
            Register::Command,
            Register::Id,
            Register::Status,
            Register::Points,
        ] {
            assert_eq!(reg.addr_bytes(), reg.addr().to_be_bytes());
        }
        let [hi, lo] = Register::Status.addr_bytes();
        assert_eq!(
            Register::Status.write_u8(StatusWrite::Clear.byte()),
            [hi, lo, StatusWrite::Clear.byte()]
        );
        assert_eq!(STATUS_BUFFER_READY, StatusBits::BUFFER_READY);
        assert_eq!(COMMAND_READ_COORDINATES, Command::ReadCoordinates.byte());
        assert_eq!(Command::Gesture.byte(), Command::Gesture as u8);
        assert_eq!(StatusWrite::Clear.byte(), StatusWrite::Clear as u8);
        let one_contact = 1;
        let ready = StatusBits::from_byte(StatusBits::BUFFER_READY | one_contact);
        assert!(ready.buffer_ready());
        assert_eq!(ready.touch_count(), one_contact);
        let idle = StatusBits::from_byte(StatusWrite::Clear.byte());
        assert!(!idle.buffer_ready());
        assert_eq!(idle.touch_count(), 0);
    }

    #[test]
    fn status_heartbeat_is_on_or_off() {
        assert_eq!(StatusHeartbeat::Off.interval_secs(), None);
        assert!(!StatusHeartbeat::Off.is_on());
        assert_eq!(StatusHeartbeat::EverySecs(0).interval_secs(), None);
        assert_eq!(StatusHeartbeat::EverySecs(10).interval_secs(), Some(10));
        assert!(StatusHeartbeat::EverySecs(10).is_on());
        assert_eq!(STATUS_HEARTBEAT, StatusHeartbeat::EverySecs(10));
        assert_eq!(MAX_TOUCH_POINTS, 5);
    }

    #[test]
    fn seven_bit_addresses_are_the_datasheet_eight_bit_pairs_shifted() {
        for pair in SlaveAddress::probe_order() {
            assert_eq!(pair.read_8bit(), pair.write_8bit() | 1);
            assert_eq!(pair.seven_bit(), pair.write_8bit() >> 1);
        }
        assert!(!SlaveAddress::PairBaBb.int_high_at_rst());
        assert!(SlaveAddress::Pair28_29.int_high_at_rst());
    }

    #[test]
    fn reset_sequence_stays_inside_the_datasheet_init_window() {
        const {
            assert!(
                RESET_HOLD_MS + RESET_RELEASE_MS + INT_SETTLE_MS + POST_RESET_SETTLE_MS
                    < INIT_WINDOW_MS
            )
        };
        const {
            assert!(
                ADDR_SELECT_RESET_HOLD_MS
                    + ADDR_SELECT_RESET_RELEASE_MS
                    + ADDR_SELECT_INT_HOLD_AFTER_RST_MS
                    + ADDR_SELECT_INT_FLOAT_MS
                    < INIT_WINDOW_MS
            )
        };
    }

    #[test]
    fn opposite_corners_map_to_opposite_corners() {
        assert_eq!(to_screen(0, 0), (PANEL_WIDTH - 1, PANEL_HEIGHT - 1));
        assert_eq!(to_screen(PANEL_WIDTH, PANEL_HEIGHT), (0, 0));
    }

    #[test]
    fn the_framebuffer_mapping_is_the_screen_mapping_without_the_flip() {
        for (cx, cy) in [(0, 0), (400, 240), (800, 480), (123, 456)] {
            let (fb_x, fb_y) = to_framebuffer(cx, cy);
            let (sx, sy) = to_screen(cx, cy);
            assert_eq!(sx, PANEL_WIDTH - fb_x - 1);
            assert_eq!(sy, PANEL_HEIGHT - fb_y - 1);
        }
    }

    #[test]
    fn every_sample_stays_on_screen() {
        for cx in (0..=PANEL_WIDTH).step_by(17) {
            for cy in (0..=PANEL_HEIGHT).step_by(13) {
                let (sx, sy) = to_screen(cx, cy);
                assert!(sx < PANEL_WIDTH, "x {sx} from ({cx}, {cy})");
                assert!(sy < PANEL_HEIGHT, "y {sy} from ({cx}, {cy})");
            }
        }
    }

    #[test]
    fn out_of_range_samples_are_clamped_not_wrapped() {
        // A controller that reports past its configured range must not produce
        // a coordinate that indexes outside the framebuffer.
        let (sx, sy) = to_screen(PANEL_WIDTH * 2, PANEL_HEIGHT * 2);
        assert!(sx < PANEL_WIDTH);
        assert!(sy < PANEL_HEIGHT);
    }

    #[test]
    fn the_centre_maps_near_the_centre() {
        let (sx, sy) = to_screen(PANEL_WIDTH / 2, PANEL_HEIGHT / 2);
        assert!((sx as i32 - 400).abs() <= 2, "x was {sx}");
        assert!((sy as i32 - 240).abs() <= 2, "y was {sy}");
    }
}
