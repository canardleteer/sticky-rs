//! Gate, source, and common-voltage (VCOM) analog registers.
//!
//! These are the high-voltage rails that actually move pigment. Datasheet
//! Rev 1.0: command 0x32 carries 105 bytes of waveform phases. Gate (0x03),
//! source (0x04), and VCOM (0x2C) are **separate** writes. Seeed's Sticky
//! SSD1677 driver never sends them — factory OTP (one-time programmable
//! memory on the panel) brings analog up with the stored waveform. Writing a
//! guessed VGH/VSH/VCOM envelope is how you cook film.
//!
//! [`AnalogVoltages::POR`] is the controller power-on default from Table 7-1,
//! not a Sticky calibration. Do not send it unless a panel note says the OTP
//! path is not in use.
//!
//! Unconfirmed Sticky / FreeInk bytes are **not compiled**. See
//! [docs/ssd1677.md](../../../docs/ssd1677.md).

/// One-byte VGH for command 0x03. Table 7-1: `A[4:0]`, POR `00h` = 20 V.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateVoltage(u8);

impl GateVoltage {
    /// Table 7-1 POR (`00h`, 20 V).
    pub const POR: Self = Self(0x00);

    /// Wraps a raw 0x03 byte that is not [`Self::POR`].
    ///
    /// Prefer [`Self::POR`]. Use this only when a panel note cites a VGH byte.
    /// Do not treat a FreeInk dump as a Sticky default.
    #[inline]
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    /// Raw `A[7:0]` as written on the wire.
    #[inline]
    #[must_use]
    pub const fn byte(self) -> u8 {
        self.0
    }
}

/// Three-byte VSH1 / VSH2 / VSL for command 0x04.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceVoltage {
    /// VSH1. POR `0x41` = 15 V.
    pub vsh1: u8,
    /// VSH2. POR `0xA8` = 5 V.
    pub vsh2: u8,
    /// VSL. POR `0x32` = −15 V.
    pub vsl: u8,
}

impl SourceVoltage {
    /// Table 7-1 POR.
    pub const POR: Self = Self {
        vsh1: 0x41,
        vsh2: 0xa8,
        vsl: 0x32,
    };

    /// Wire order A, B, C.
    #[inline]
    #[must_use]
    pub const fn bytes(self) -> [u8; 3] {
        [self.vsh1, self.vsh2, self.vsl]
    }
}

/// One-byte VCOM for command 0x2C. POR `00h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vcom(u8);

impl Vcom {
    /// Table 7-1 POR.
    pub const POR: Self = Self(0x00);

    /// `0x30` = −1.2 V in Table 7-1. Cited so a reader can decode a dump;
    /// **not** a Sticky default (Seeed does not write 0x2C on this panel).
    pub const NEG_1V2: Self = Self(0x30);

    /// Wraps a raw 0x2C byte that is not [`Self::POR`] or [`Self::NEG_1V2`].
    ///
    /// Prefer the named constants. Use this only when a panel note cites a
    /// VCOM byte. Do not treat a FreeInk dump as a Sticky default.
    #[inline]
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    /// Raw byte.
    #[inline]
    #[must_use]
    pub const fn byte(self) -> u8 {
        self.0
    }
}

/// Optional analog trio written after booster if a MCU LUT path needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalogVoltages {
    /// Command 0x03.
    pub gate: GateVoltage,
    /// Command 0x04.
    pub source: SourceVoltage,
    /// Command 0x2C.
    pub vcom: Vcom,
}

impl AnalogVoltages {
    /// Controller POR values from Table 7-1. Not a panel calibration.
    pub const POR: Self = Self {
        gate: GateVoltage::POR,
        source: SourceVoltage::POR,
        vcom: Vcom::POR,
    };
}

// Unconfirmed: FreeInk `lut_grayscale_sticky` voltage tail (VGH, VSH1, VSH2,
// VSL, VCOM) = 0x17, 0x41, 0xA8, 0x32, 0x30. Those five bytes were **not** in
// stock `reterminal_template` app0. Seeed's SSD1677 driver does not write
// 0x03 / 0x04 / 0x2C. Leave commented: not factory, and analog rails are not
// something this repository can dump from glass.
//
// const UNCONFIRMED_FREEINK_STICKY_ANALOG: AnalogVoltages = AnalogVoltages {
//     gate: GateVoltage::from_byte(0x17),
//     source: SourceVoltage { vsh1: 0x41, vsh2: 0xA8, vsl: 0x32 },
//     vcom: Vcom::NEG_1V2,
// };

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_por_matches_table_7_1() {
        assert_eq!(SourceVoltage::POR.bytes(), [0x41, 0xa8, 0x32]);
        assert_eq!(GateVoltage::POR.byte(), 0x00);
        assert_eq!(Vcom::POR.byte(), 0x00);
        assert_eq!(Vcom::NEG_1V2.byte(), 0x30);
        assert_eq!(GateVoltage::from_byte(0x17).byte(), 0x17);
        assert_eq!(Vcom::from_byte(0x30), Vcom::NEG_1V2);
    }
}
