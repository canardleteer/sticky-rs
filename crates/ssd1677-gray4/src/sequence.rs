//! Datasheet-verified parameter blocks for init and refresh.
//!
//! These are **controller** facts from SSD1677 Rev 1.0 `Table 7-1: Command
//! Table`: booster inrush, which stored waveform sequence Master Activation
//! will run, and gate-scan bits (`8.1 Driver Output Control (01h)`). Which
//! values **this glass** wants live in `seeed-reterminal-sticky` and
//! [docs/ssd1677.md]. On the Sticky those sequences load factory OTP
//! (section `6.10 One Time Programmable (OTP) Memory`); they are not a
//! 105-byte `Write LUT register` table.

/// Five-byte payload for [`crate::Command::BoosterSoftStart`] (0x0C).
///
/// Table 7-1 lists two inrush levels; both share `AE C7 C3 C0` and differ only
/// in the last byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoosterSoftStart {
    bytes: [u8; 5],
}

impl BoosterSoftStart {
    /// Level 1: last byte `0x40`.
    pub const LEVEL_1: Self = Self {
        bytes: [0xae, 0xc7, 0xc3, 0xc0, 0x40],
    };

    /// Level 2: last byte `0x80`.
    ///
    /// This is the table entry Seeed's `seeed_epaper` SSD1677 driver writes, and
    /// the same five bytes sit in stock `reterminal_template` next to
    /// `ssd1677_init_base` / `write softstart failed`.
    pub const LEVEL_2: Self = Self {
        bytes: [0xae, 0xc7, 0xc3, 0xc0, 0x80],
    };

    /// The five data bytes, in Table 7-1 order A..E.
    #[inline]
    #[must_use]
    pub const fn bytes(self) -> [u8; 5] {
        self.bytes
    }
}

/// `Display Update Control 2` (0x22) parameter.
///
/// Table 7-1 lists the stage combinations Master Activation will run. A wrong
/// byte can skip analog enable, skip the OTP LUT load, or leave the booster
/// running. Prefer the named constants. [`Self::from_byte`] is only for a
/// panel note that cites a sequence the table excerpt does not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateSequence(u8);

impl UpdateSequence {
    /// Enable clock only (`0x80`).
    pub const ENABLE_CLOCK: Self = Self(0x80);
    /// Disable clock only (`0x01`).
    pub const DISABLE_CLOCK: Self = Self(0x01);
    /// Enable clock, then analog (`0xC0`).
    ///
    /// Written after [`crate::Command::DisplayUpdateControl2`], then
    /// [`crate::Command::MasterActivation`]. Stock `ssd1677_resume`.
    pub const ENABLE_CLOCK_AND_ANALOG: Self = Self(0xc0);
    /// Disable analog, then clock (`0x03`).
    ///
    /// Written after [`crate::Command::DisplayUpdateControl2`], then
    /// [`crate::Command::MasterActivation`]. Stock `ssd1677_standby`.
    /// Controller RAM stays; this is not [`crate::Command::DeepSleepMode`].
    pub const DISABLE_ANALOG_AND_CLOCK: Self = Self(0x03);
    /// Stock standby: [`Self::DISABLE_ANALOG_AND_CLOCK`].
    pub const STANDBY: Self = Self::DISABLE_ANALOG_AND_CLOCK;
    /// Stock resume: [`Self::ENABLE_CLOCK_AND_ANALOG`].
    pub const RESUME: Self = Self::ENABLE_CLOCK_AND_ANALOG;
    /// Enable clock and analog, DISPLAY Mode 1, then power down (`0xC7`).
    pub const DISPLAY_MODE_1: Self = Self(0xc7);
    /// Enable clock and analog, DISPLAY Mode 2, then power down (`0xCF`).
    pub const DISPLAY_MODE_2: Self = Self(0xcf);
    /// Load temperature from the internal sensor, DISPLAY Mode 1, power down
    /// (`0xF7`). Table 7-1. Seeed uses this for a full black/white refresh.
    pub const DISPLAY_MODE_1_WITH_TEMP: Self = Self(0xf7);
    /// Same as [`Self::DISPLAY_MODE_1_WITH_TEMP`] but DISPLAY Mode 2 (`0xFF`).
    /// Table 7-1 POR. Seeed uses this for a partial / DU refresh.
    pub const DISPLAY_MODE_2_WITH_TEMP: Self = Self(0xff);
    /// Seeed / stock gray4 OTP refresh (`0xD7`), then Master Activation.
    ///
    /// Not a named row in the Table 7-1 excerpt we extracted. Seeed
    /// `ssd1677_refresh` writes it for GRAY4; stock `reterminal_template` uses
    /// that same symbol. Pair with [`SEEED_GRAY4_TEMPERATURE`] and
    /// [`crate::planes::PlaneMapping::SEEED_OTP`].
    pub const SEEED_GRAY4: Self = Self(0xd7);

    // Unconfirmed: Lotus and bb_epaper EP397 use 0xFC for "partial" instead of
    // Seeed [`Self::DISPLAY_MODE_2_WITH_TEMP`]. Not a Table 7-1 named row we
    // extracted. Do not switch the crate.
    // const UNCONFIRMED_BB_EPAPER_PARTIAL: UpdateSequence =
    //     UpdateSequence::from_byte(0xfc);

    // Unconfirmed: Lotus / bb_epaper Display Update Control 1 (0x21). Seeed OTP
    // never sends 0x21.
    // const UNCONFIRMED_LOTUS_UPDATE_CONTROL1_FULL: [u8; 2] = [0x40, 0x00];
    // const UNCONFIRMED_LOTUS_UPDATE_CONTROL1_PARTIAL: [u8; 2] = [0x00, 0x00];

    /// Wraps a raw 0x22 byte that is not one of the table names above.
    ///
    /// Prefer the associated constants. Use this only when a panel note cites
    /// a sequence the table lists under a different description, or when the
    /// table excerpt we have does not name that byte.
    #[inline]
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    /// The byte written after opcode 0x22.
    #[inline]
    #[must_use]
    pub const fn byte(self) -> u8 {
        self.0
    }
}

/// `Driver Output control` (0x01) scan byte `B[2:0]`.
///
/// SSD1677 Rev 1.0 section `8.1 Driver Output Control (01h)`: `GD` first-gate
/// select, `SM` odd/even split, `TB` scan direction. **`TB = 1` is reserved**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateScan {
    /// `B[2]`: first gate output channel (`0` = G0 first).
    pub gd: bool,
    /// `B[1]`: `false` = interlaced (POR), `true` = no odd/even split.
    pub sm: bool,
    /// `B[0]`: must stay `false` (reserved when set).
    pub tb: bool,
}

impl GateScan {
    /// Power-on default: `GD=0`, `SM=0`, `TB=0`.
    pub const POR: Self = Self {
        gd: false,
        sm: false,
        tb: false,
    };

    /// Seeed Sticky: `GD=0`, `SM=1`, `TB=0` (`0x02`).
    ///
    /// Written by `seeed_epaper` `ssd1677_init_base` as the third byte of
    /// command 0x01. Stock firmware uses that same init.
    pub const SEEED_STICKY: Self = Self {
        gd: false,
        sm: true,
        tb: false,
    };

    /// Packs `B[2:0]`. Returns `None` if `TB` is set (reserved).
    #[inline]
    #[must_use]
    pub const fn byte(self) -> Option<u8> {
        if self.tb {
            return None;
        }
        Some(((self.gd as u8) << 2) | ((self.sm as u8) << 1))
    }
}

/// `Data Entry mode setting` (0x11) `A[2:0]`.
///
/// SSD1677 Rev 1.0 section `8.2 Data Entry Mode Setting (11h)`. POR is
/// `0b011`: Y increment, X increment, address updates in X.
pub const DATA_ENTRY_Y_INC_X_INC: u8 = 0b011;

/// `Border Waveform Control` (0x3C) helpers.
///
/// Table 7-1: `A[7:6]` selects VBD source, `A[5:4]` a fixed level, `A[1:0]` a
/// LUT follow. POR is `0xC0` (HiZ).
pub mod border {
    /// POR: VBD = HiZ (`A[7:6] = 11`).
    pub const HIZ: u8 = 0xc0;
    /// VBD = VCOM (`A[7:6] = 10`). Seeed partial refresh uses this (`0x80`).
    pub const VCOM: u8 = 0x80;
    /// Follow LUT0 (`A[7:6] = 00`, `A[1:0] = 00`). Seeed gray4 uses `0x00`.
    pub const FOLLOW_LUT0: u8 = 0x00;
    /// Follow LUT1 (`A[1:0] = 01`). Seeed full refresh uses `0x01`.
    pub const FOLLOW_LUT1: u8 = 0x01;
}

/// Pattern byte Seeed does **not** send, but Figure 9-1 uses with 0x46 / 0x47.
pub const AUTO_WRITE_FILL: u8 = 0xf7;

/// Seeed gray4 writes this 12-bit temperature (0x1A) before 0x22.
///
/// Layout is Table 7-1: first byte `A[11:4]`, second byte `A[3:0]` in the high
/// nibble. These two bytes are opaque panel data from `seeed_epaper`
/// (`Force_temprature_EPD_by_OTP_Update`); they are not a Celsius conversion
/// we have verified.
pub const SEEED_GRAY4_TEMPERATURE: [u8; 2] = [0x67, 0x00];

// Unconfirmed: bb_epaper EP397 / OpenDisplay / TRMNL write a single 0x1A byte
// 0x5A for 4GRAY. Seeed writes two bytes {0x67, 0x00}. Do not mix.
// const UNCONFIRMED_BB_EPAPER_GRAY4_TEMPERATURE: u8 = 0x5a;

// Unconfirmed: bb_epaper EP397 uses data entry 0x11 = 0x01 (Y↑ X↓) vs Seeed
// 0x03 (Y↑ X↑). Sticky stays on DATA_ENTRY_Y_INC_X_INC.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn booster_levels_match_the_datasheet_table() {
        assert_eq!(BoosterSoftStart::LEVEL_1.bytes()[4], 0x40);
        assert_eq!(BoosterSoftStart::LEVEL_2.bytes()[4], 0x80);
        assert_eq!(
            &BoosterSoftStart::LEVEL_2.bytes()[..4],
            &[0xae, 0xc7, 0xc3, 0xc0]
        );
    }

    #[test]
    fn named_update_sequences_match_table_7_1() {
        assert_eq!(UpdateSequence::DISPLAY_MODE_1_WITH_TEMP.byte(), 0xf7);
        assert_eq!(UpdateSequence::DISPLAY_MODE_2_WITH_TEMP.byte(), 0xff);
        assert_eq!(UpdateSequence::SEEED_GRAY4.byte(), 0xd7);
        assert_eq!(UpdateSequence::DISPLAY_MODE_1.byte(), 0xc7);
        assert_eq!(UpdateSequence::ENABLE_CLOCK_AND_ANALOG.byte(), 0xc0);
        assert_eq!(UpdateSequence::DISABLE_ANALOG_AND_CLOCK.byte(), 0x03);
        assert_eq!(
            UpdateSequence::STANDBY,
            UpdateSequence::DISABLE_ANALOG_AND_CLOCK
        );
        assert_eq!(
            UpdateSequence::RESUME,
            UpdateSequence::ENABLE_CLOCK_AND_ANALOG
        );
    }

    #[test]
    fn gate_scan_rejects_reserved_tb() {
        assert_eq!(GateScan::POR.byte(), Some(0));
        assert_eq!(GateScan::SEEED_STICKY.byte(), Some(0x02));
        let reserved = GateScan {
            gd: false,
            sm: false,
            tb: true,
        };
        assert_eq!(reserved.byte(), None);
    }

    #[test]
    fn data_entry_por_is_y_inc_x_inc() {
        assert_eq!(DATA_ENTRY_Y_INC_X_INC, 0x03);
    }
}
