//! Waveform look-up tables written by the microcontroller.
//!
//! # There is no default table, and there will not be one
//!
//! A **waveform** is the timed voltage recipe that moves pigment. It is panel
//! data, not something the controller datasheet can invent. Command
//! `Write LUT register` (0x32) lets firmware download a 105-byte **look-up
//! table** (LUT). There is nothing to read back and nothing safe to infer.
//! A table taken from a different panel can drive the film outside its
//! intended voltage-time envelope; that damage is cumulative and invisible
//! until it is not.
//!
//! The Seeed Sticky does **not** use this path. It runs waveforms already
//! burned into **OTP** (one-time programmable memory on the panel). See
//! [docs/ssd1677.md](../../../docs/ssd1677.md). This module still accepts an
//! attributed [`Lut`] for a panel that needs a microcontroller-written table.
//!
//! A 105-byte FreeInk Sticky table was compared to stock `reterminal_template`
//! app0 and **was not present**. It is not compiled in. Leave it commented:
//! the controller does not read factory OTP back, and this crate has no
//! capture path. See [docs/ssd1677.md](../../../docs/ssd1677.md).

/// Length of the `Write LUT register` payload: **105 bytes**.
///
/// Datasheet Rev 1.0, Table 7-1: command 0x32 takes 105 bytes containing
/// `VS[nX-LUT]`, `TP#[nX]`, and `RP#[n]`. Section 6.7 separately describes 112
/// bytes of on-chip waveform storage including gate/source voltage and frame
/// rate; the MCU-facing command is the shorter one.
pub const LUT_LEN: usize = 105;

/// A 105-byte waveform look-up table the microcontroller can write with
/// command `0x32`, plus a string saying where those bytes came from.
///
/// The Sticky factory path does not use this type (`Config::lut` stays
/// `None`). The `source` string exists so an unattributed table cannot compile
/// in: if you cannot say where a waveform came from, you cannot ship it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lut {
    bytes: [u8; LUT_LEN],
    source: &'static str,
}

impl Lut {
    /// Wraps a waveform table together with a description of where it came
    /// from, such as the project and license it was ported from.
    ///
    /// # Panics
    ///
    /// Panics if `source` is empty. An unattributed waveform is a maintenance
    /// and licensing problem, and this is the cheapest place to catch it.
    #[must_use]
    pub const fn new(bytes: [u8; LUT_LEN], source: &'static str) -> Self {
        assert!(
            !source.is_empty(),
            "a waveform LUT must record where it came from"
        );
        Self { bytes, source }
    }

    /// The payload for [`crate::Command::WriteLutRegister`].
    #[inline]
    #[must_use]
    pub const fn bytes(&self) -> &[u8; LUT_LEN] {
        &self.bytes
    }

    /// Where this waveform came from.
    #[inline]
    #[must_use]
    pub const fn source(&self) -> &'static str {
        self.source
    }
}

// Unconfirmed MCU waveform. FreeInk `lut_grayscale_sticky` (105 bytes of
// VS/TP/RP + frame rate). Stock `reterminal_template` 1.1.0 app0 does not
// contain this table; Seeed `seeed_epaper` `ssd1677.c` never sends command
// 0x32. Leave commented. Details: docs/ssd1677.md.
//
// const UNCONFIRMED_FREEINK_STICKY_LUT: [u8; LUT_LEN] = [
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
    fn lut_length_matches_the_datasheet_command() {
        assert_eq!(LUT_LEN, 105);
        let lut = Lut::new([0; LUT_LEN], "test vector");
        assert_eq!(lut.bytes().len(), 105);
        assert_eq!(lut.source(), "test vector");
    }
}
