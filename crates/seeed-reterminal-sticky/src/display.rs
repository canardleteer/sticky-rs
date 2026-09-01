//! Panel geometry, OTP refresh modes, and the confirmed SSD1677 init.
//!
//! The film is **mono**. Four gray levels on this product come from Seeed's
//! OTP gray4 path (SSD1677 Rev 1.0 `6.5 RAM` + `6.10 One Time Programmable
//! (OTP) Memory` + [`UpdateSequence::SEEED_GRAY4`]), not from a shipped MCU
//! LUT. Provenance:
//! [docs/ssd1677.md](../../../docs/ssd1677.md) and the hardware skill
//! `references/display.md`.
//!
//! Controller opcodes live in `ssd1677-gray4`. This module is **this glass**.

use ssd1677_gray4::sequence::{
    border, BoosterSoftStart, GateScan, UpdateSequence, DATA_ENTRY_Y_INC_X_INC,
    SEEED_GRAY4_TEMPERATURE,
};
use ssd1677_gray4::{Config, PlaneMapping, Window};

/// Panel width in pixels, landscape scan.
pub const WIDTH: u16 = 800;
/// Panel height in pixels, landscape scan.
pub const HEIGHT: u16 = 480;

/// USB-down portrait page width (short edge, keys on the right).
///
/// Matches the enclosure diagram: glass facing you, USB-C on the bottom
/// short edge.
pub const PAGE_WIDTH: u16 = HEIGHT;
/// USB-down portrait page height (long edge).
pub const PAGE_HEIGHT: u16 = WIDTH;

/// In-plane page used to draw upright content for a hold.
///
/// Face-up and face-down are not variants: a flat unit has no in-plane
/// “up.” Keep the last rotation (default [`Self::Portrait0`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRotation {
    /// USB-C on the bottom short edge. Page is [`PAGE_WIDTH`] ×
    /// [`PAGE_HEIGHT`].
    Portrait0,
    /// USB-C on the top short edge. Same portrait page, mapped 180°.
    Portrait180,
    /// USB-C on the right short edge. Page is [`WIDTH`] × [`HEIGHT`].
    Landscape0,
    /// USB-C on the left short edge. Same landscape page, mapped 180°.
    Landscape180,
}

impl PageRotation {
    /// Logical page size for this hold, `(width, height)`.
    #[must_use]
    pub const fn page_size(self) -> (u16, u16) {
        match self {
            Self::Portrait0 | Self::Portrait180 => (PAGE_WIDTH, PAGE_HEIGHT),
            Self::Landscape0 | Self::Landscape180 => (WIDTH, HEIGHT),
        }
    }
}

/// Maps a USB-down portrait pixel onto the pre-rotation 800×480 canvas.
///
/// Same as [`page_to_framebuffer`] with [`PageRotation::Portrait0`].
/// Embassy-debug does not `mirror_x_plane` after this (that reverse_bits
/// 8-pixel vertical bands on this page).
#[must_use]
pub const fn portrait_to_framebuffer(px: u16, py: u16) -> Option<(u16, u16)> {
    page_to_framebuffer(px, py, PageRotation::Portrait0)
}

/// Maps a page pixel onto the pre-rotation 800×480 canvas.
///
/// Page `(0, 0)` is the top-left as the user holds the card. Output is
/// the same space [`portrait_to_framebuffer`] already used: callers that
/// then write `(WIDTH-1-x, HEIGHT-1-y)` must not add another rotation.
#[must_use]
pub const fn page_to_framebuffer(px: u16, py: u16, rotation: PageRotation) -> Option<(u16, u16)> {
    let (page_w, page_h) = rotation.page_size();
    if px >= page_w || py >= page_h {
        return None;
    }
    Some(match rotation {
        PageRotation::Portrait0 => (WIDTH - 1 - py, HEIGHT - 1 - px),
        PageRotation::Portrait180 => (py, px),
        // Identity landscape printed Latin right-to-left on a physical unit
        // (2026-08-30). Mirror X; 180 stays the other landscape hold.
        PageRotation::Landscape0 => (WIDTH - 1 - px, py),
        PageRotation::Landscape180 => (px, HEIGHT - 1 - py),
    })
}

/// Last RAM X address unit for a full-width window (`8.3` address units).
pub const RAM_X_END: u16 = WIDTH - 1;
/// Last RAM Y address unit for a full-height window (`8.4` address units).
pub const RAM_Y_END: u16 = HEIGHT - 1;

/// Value for the controller's `Driver Output control`: gate lines minus one.
pub const GATE_LINES_MINUS_ONE: u16 = HEIGHT - 1;

/// SPI clock used on a physical unit.
///
/// The controller's own maximum is 20 MHz. Panel refresh and a read-only
/// card `send_status` both ACK at that rate after 400 kHz init. This
/// default stays 10 MHz. A widely copied board profile defaults to 40 MHz,
/// which is out of spec — do not inherit it.
pub const SPI_MAX_HZ: u32 = 10_000_000;

/// SPI mode used on a physical unit (CPOL=0, CPHA=0).
pub const SPI_MODE: u8 = 0;

/// Bytes in a full-screen 2 bits-per-pixel four-gray framebuffer.
pub const GRAY4_FRAME_BYTES: usize = (WIDTH as usize / 4) * HEIGHT as usize;

/// Bytes in one 1 bit-per-pixel controller plane.
pub const PLANE_BYTES: usize = (WIDTH as usize / 8) * HEIGHT as usize;

/// Reset pulse width used on a physical unit, in milliseconds (low, then high).
pub const RESET_PULSE_MS: u32 = 10;

/// Seeed waits this long after [`ssd1677_gray4::command::DEEP_SLEEP_ENTER`]
/// before cutting the rail.
pub const SLEEP_HOLD_MS: u32 = 100;

/// Full-panel window in datasheet address units (not bytes).
pub const FULL_WINDOW: Window = Window {
    x_start: 0,
    x_end: RAM_X_END,
    y_start: 0,
    y_end: RAM_Y_END,
};

/// How Seeed's stock driver refreshes this panel.
///
/// These are OTP sequences (section `6.10`). They do **not** write Table 7-1
/// `Write LUT register` (0x32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshKind {
    /// Black/white full update: software reset, [`border::FOLLOW_LUT1`],
    /// [`UpdateSequence::DISPLAY_MODE_1_WITH_TEMP`].
    Full,
    /// Black/white partial / DU: no software reset, [`border::VCOM`],
    /// [`UpdateSequence::DISPLAY_MODE_2_WITH_TEMP`].
    Partial,
    /// Four-gray OTP: no software reset, [`border::FOLLOW_LUT0`],
    /// [`SEEED_GRAY4_TEMPERATURE`], [`UpdateSequence::SEEED_GRAY4`].
    Gray4,
}

impl RefreshKind {
    /// `Display Update Control 2` byte for this mode.
    #[inline]
    #[must_use]
    pub const fn sequence(self) -> UpdateSequence {
        match self {
            Self::Full => UpdateSequence::DISPLAY_MODE_1_WITH_TEMP,
            Self::Partial => UpdateSequence::DISPLAY_MODE_2_WITH_TEMP,
            Self::Gray4 => UpdateSequence::SEEED_GRAY4,
        }
    }

    /// Border (0x3C) Seeed writes during `init_base` for this mode.
    #[inline]
    #[must_use]
    pub const fn border(self) -> u8 {
        match self {
            Self::Full => border::FOLLOW_LUT1,
            Self::Partial => border::VCOM,
            Self::Gray4 => border::FOLLOW_LUT0,
        }
    }

    /// Whether `init` should send [`ssd1677_gray4::Command::SoftwareReset`].
    /// Seeed only does this on full refresh.
    #[inline]
    #[must_use]
    pub const fn software_reset(self) -> bool {
        matches!(self, Self::Full)
    }

    /// Temperature register (0x1A) written immediately before Master Activation
    /// on gray4. `None` for black/white modes
    /// ([`UpdateSequence`] loads temperature itself).
    #[inline]
    #[must_use]
    pub const fn temperature_override(self) -> Option<[u8; 2]> {
        match self {
            Self::Gray4 => Some(SEEED_GRAY4_TEMPERATURE),
            Self::Full | Self::Partial => None,
        }
    }

    /// Plane mapping that matches Seeed's inverted OTP gray4 builder.
    #[inline]
    #[must_use]
    pub const fn plane_mapping(self) -> PlaneMapping {
        match self {
            Self::Gray4 => PlaneMapping::SEEED_OTP,
            Self::Full | Self::Partial => PlaneMapping::LUT_INDEX_ORDER,
        }
    }

    /// Controller [`Config`] for this mode. `lut` is always `None` (OTP).
    #[must_use]
    pub fn controller_config(self) -> Config<'static> {
        Config {
            gate_lines: GATE_LINES_MINUS_ONE,
            scan_bits: GateScan::SEEED_STICKY
                .byte()
                .expect("Seeed scan does not set reserved TB"),
            data_entry_mode: DATA_ENTRY_Y_INC_X_INC,
            window: FULL_WINDOW,
            lut: None,
            border_waveform: Some(self.border()),
            internal_temperature_sensor: true,
            booster: Some(BoosterSoftStart::LEVEL_2),
            software_reset: self.software_reset(),
            analog: None,
        }
    }
}

// Unconfirmed MCU 105-byte waveform (FreeInk `lut_grayscale_sticky`).
// Not Table 7-1 / `6.10` OTP. Compared to stock `reterminal_template` 1.1.0
// app0: the table is absent. Seeed never sends `Write LUT register`. Leave
// commented. See docs/ssd1677.md.
//
// const UNCONFIRMED_FREEINK_STICKY_LUT: [u8; 105] = [
//     0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//     0x54, 0x54, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//     0xAA, 0xA0, 0xA8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//     0xA2, 0x22, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//     0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//     0x01, 0x01, 0x01, 0x01, 0x00,
//     0x01, 0x01, 0x01, 0x01, 0x00,
//     0x01, 0x01, 0x01, 0x01, 0x00,
//     0x00, 0x00, 0x00, 0x00, 0x00,
//     0x00, 0x00, 0x00, 0x00, 0x00,
//     0x00, 0x00, 0x00, 0x00, 0x00,
//     0x00, 0x00, 0x00, 0x00, 0x00,
//     0x00, 0x00, 0x00, 0x00, 0x00,
//     0x00, 0x00, 0x00, 0x00, 0x00,
//     0x00, 0x00, 0x00, 0x00, 0x00,
//     0x8F, 0x8F, 0x8F, 0x8F, 0x8F,
// ];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_sizes_match_the_board_contract() {
        assert_eq!(GRAY4_FRAME_BYTES, 96_000);
        assert_eq!(PLANE_BYTES, 48_000);
        assert_eq!(GRAY4_FRAME_BYTES, PLANE_BYTES * 2);
        assert_eq!(PAGE_WIDTH, 480);
        assert_eq!(PAGE_HEIGHT, 800);
    }

    #[test]
    fn usb_down_portrait_corners_land_on_the_landscape_canvas() {
        assert_eq!(portrait_to_framebuffer(0, 0), Some((799, 479)));
        assert_eq!(portrait_to_framebuffer(479, 0), Some((799, 0)));
        assert_eq!(portrait_to_framebuffer(0, 799), Some((0, 479)));
        assert_eq!(portrait_to_framebuffer(479, 799), Some((0, 0)));
        assert_eq!(portrait_to_framebuffer(480, 0), None);
        assert_eq!(portrait_to_framebuffer(0, 800), None);
        assert_eq!(
            page_to_framebuffer(0, 0, PageRotation::Portrait0),
            portrait_to_framebuffer(0, 0)
        );
    }

    #[test]
    fn usb_up_portrait_corners_are_the_180_of_usb_down() {
        assert_eq!(
            page_to_framebuffer(0, 0, PageRotation::Portrait180),
            Some((0, 0))
        );
        assert_eq!(
            page_to_framebuffer(479, 0, PageRotation::Portrait180),
            Some((0, 479))
        );
        assert_eq!(
            page_to_framebuffer(0, 799, PageRotation::Portrait180),
            Some((799, 0))
        );
        assert_eq!(
            page_to_framebuffer(479, 799, PageRotation::Portrait180),
            Some((799, 479))
        );
        assert_eq!(page_to_framebuffer(480, 0, PageRotation::Portrait180), None);
    }

    #[test]
    fn usb_right_landscape_corners_mirror_x_on_the_canvas() {
        assert_eq!(
            page_to_framebuffer(0, 0, PageRotation::Landscape0),
            Some((799, 0))
        );
        assert_eq!(
            page_to_framebuffer(799, 0, PageRotation::Landscape0),
            Some((0, 0))
        );
        assert_eq!(
            page_to_framebuffer(0, 479, PageRotation::Landscape0),
            Some((799, 479))
        );
        assert_eq!(
            page_to_framebuffer(799, 479, PageRotation::Landscape0),
            Some((0, 479))
        );
        assert_eq!(page_to_framebuffer(800, 0, PageRotation::Landscape0), None);
        assert_eq!(page_to_framebuffer(0, 480, PageRotation::Landscape0), None);
    }

    #[test]
    fn usb_left_landscape_corners_are_the_180_of_usb_right() {
        assert_eq!(
            page_to_framebuffer(0, 0, PageRotation::Landscape180),
            Some((0, 479))
        );
        assert_eq!(
            page_to_framebuffer(799, 0, PageRotation::Landscape180),
            Some((799, 479))
        );
        assert_eq!(
            page_to_framebuffer(0, 479, PageRotation::Landscape180),
            Some((0, 0))
        );
        assert_eq!(
            page_to_framebuffer(799, 479, PageRotation::Landscape180),
            Some((799, 0))
        );
        assert_eq!(
            page_to_framebuffer(800, 0, PageRotation::Landscape180),
            None
        );
    }

    #[test]
    fn the_window_fits_the_controller_address_space() {
        const { assert!(RAM_X_END <= 0x03bf) };
        const { assert!(RAM_Y_END <= 0x02a7) };
    }

    #[test]
    fn the_spi_clock_stays_inside_the_controller_spec() {
        const { assert!(SPI_MAX_HZ <= 20_000_000) };
        assert_eq!(SPI_MODE, 0);
    }

    #[test]
    fn seeed_modes_match_the_open_driver_and_datasheet() {
        assert_eq!(
            RefreshKind::Full.sequence(),
            UpdateSequence::DISPLAY_MODE_1_WITH_TEMP
        );
        assert_eq!(
            RefreshKind::Partial.sequence(),
            UpdateSequence::DISPLAY_MODE_2_WITH_TEMP
        );
        assert_eq!(RefreshKind::Gray4.sequence(), UpdateSequence::SEEED_GRAY4);
        assert_eq!(RefreshKind::Full.border(), border::FOLLOW_LUT1);
        assert_eq!(RefreshKind::Partial.border(), border::VCOM);
        assert_eq!(RefreshKind::Gray4.border(), border::FOLLOW_LUT0);
        assert!(RefreshKind::Full.software_reset());
        assert!(!RefreshKind::Partial.software_reset());
        assert_eq!(
            RefreshKind::Gray4.temperature_override(),
            Some(SEEED_GRAY4_TEMPERATURE)
        );
        assert_eq!(RefreshKind::Gray4.plane_mapping(), PlaneMapping::SEEED_OTP);
    }

    #[test]
    fn sticky_config_is_otp_with_level2_booster() {
        let config = RefreshKind::Full.controller_config();
        assert_eq!(config.gate_lines, GATE_LINES_MINUS_ONE);
        assert_eq!(config.scan_bits, GateScan::SEEED_STICKY.byte().unwrap());
        assert_eq!(config.data_entry_mode, DATA_ENTRY_Y_INC_X_INC);
        assert!(config.lut.is_none());
        assert_eq!(config.analog, None);
        assert_eq!(config.booster, Some(BoosterSoftStart::LEVEL_2));
        assert!(config.software_reset);

        let partial = RefreshKind::Partial.controller_config();
        assert!(!partial.software_reset);
        assert_eq!(partial.border_waveform, Some(border::VCOM));
    }
}
