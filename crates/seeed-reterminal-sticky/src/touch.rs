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
//! init window (under 200 ms). Rev.07 deleted the register map, so
//! [`Register`] values are the on-glass `GT911_REG_*` names, not a Rev.09
//! table. After reset this board path writes [`Register::Status`] =
//! [`STATUS_CLEAR`], then [`Register::Command`] = [`COMMAND_READ_COORDINATES`].
//! It does not write config RAM. Crate `NotReady` means no new buffer
//! (ignore). The touch bus is [`I2C_HZ`].

/// Physical panel width in pixels.
pub const PANEL_WIDTH: u32 = 800;
/// Physical panel height in pixels.
pub const PANEL_HEIGHT: u32 = 480;
/// Digitizer width in its own portrait orientation.
pub const DIGITIZER_WIDTH: u32 = 480;
/// Digitizer height in its own portrait orientation.
pub const DIGITIZER_HEIGHT: u32 = 800;

/// Dedicated GT911 bus clock (100 kHz on glass).
///
/// Rev.09 §6.1 recommends staying at or below [`I2C_MAX_HZ`].
pub const I2C_HZ: u32 = 100_000;

/// Datasheet I2C cap (Rev.09 §6.1: “at or below 400Kbps”).
pub const I2C_MAX_HZ: u32 = 400_000;

/// Silicon maximum concurrent touches (Rev.09 §1). FPC delivery is unmeasured.
pub const MAX_TOUCH_POINTS: u8 = 5;

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
}

/// GT911 registers used after the INT-during-reset dance.
///
/// Variant names match on-glass `GT911_REG_COMMAND`, `GT911_REG_ID`, and
/// `GT911_REG_STATUS`. Rev.09 deleted the map (Rev.07); these numbers are
/// not a datasheet table. Gesture mode still names `0x8040` as a command
/// port (Rev.09 §8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Register {
    /// `GT911_REG_COMMAND`. Named as a command port in Rev.09 §8.1.
    Command = 0x8040,
    /// `GT911_REG_ID`. Product ID, four ASCII bytes.
    Id = 0x8140,
    /// `GT911_REG_STATUS`. Buffer handshake; write [`STATUS_CLEAR`] after a read.
    Status = 0x814E,
}

/// Host write to [`Register::Status`] after reading coordinates.
pub const STATUS_CLEAR: u8 = 0;

/// Value `0` at [`Register::Command`].
///
/// Used by the `gt911` crate `init()` as “read coordinates”. **Not** in
/// Rev.09 (register map deleted). Not a screen-off / sleep encoding from
/// this PDF.
pub const COMMAND_READ_COORDINATES: u8 = 0;

/// Bit 7 of [`Register::Status`].
///
/// On-glass / crate idle test (`NotReady` when clear). **Not** named in
/// Rev.09.
pub const STATUS_BUFFER_READY: u8 = 0x80;

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
        assert_eq!(Register::Command.addr_bytes(), [0x80, 0x40]);
        assert_eq!(Register::Id.addr_bytes(), [0x81, 0x40]);
        assert_eq!(Register::Status.addr_bytes(), [0x81, 0x4E]);
        assert_eq!(
            Register::Status.write_u8(STATUS_CLEAR),
            [0x81, 0x4E, STATUS_CLEAR]
        );
        assert_eq!(STATUS_BUFFER_READY, 0x80);
        assert_eq!(COMMAND_READ_COORDINATES, 0);
    }

    #[test]
    fn seven_bit_addresses_are_the_datasheet_eight_bit_pairs_shifted() {
        assert_eq!(SlaveAddress::Pair28_29.write_8bit(), 0x28);
        assert_eq!(SlaveAddress::Pair28_29.read_8bit(), 0x29);
        assert_eq!(SlaveAddress::Pair28_29.seven_bit(), 0x14);
        assert_eq!(SlaveAddress::PairBaBb.write_8bit(), 0xBA);
        assert_eq!(SlaveAddress::PairBaBb.read_8bit(), 0xBB);
        assert_eq!(SlaveAddress::PairBaBb.seven_bit(), 0x5D);
    }

    #[test]
    fn reset_sequence_stays_inside_the_datasheet_init_window() {
        const { assert!(RESET_HOLD_MS + RESET_RELEASE_MS + INT_SETTLE_MS + POST_RESET_SETTLE_MS < 200) };
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
