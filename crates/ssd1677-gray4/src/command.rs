//! SSD1677 command opcodes.
//!
//! Every variant below was read out of the Solomon Systech SSD1677 datasheet
//! **Rev 1.0 (Nov 2018), Table 7-1**. This is a deliberate subset: a command
//! this crate does not need is a command whose parameter layout we have not
//! verified, and an unverified opcode is how you corrupt a frame or stress
//! glass. Add variants by reading the table, not by pattern-matching other
//! SSD16xx drivers — SSD1677 differs from its relatives, notably in using
//! 10-bit RAM address units.

/// A datasheet-verified SSD1677 command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[non_exhaustive]
pub enum Command {
    /// `Driver Output control` — gate lines, scan direction (0x01).
    DriverOutputControl = 0x01,
    /// `Gate Driving voltage Control` — VGH (0x03). One data byte.
    GateDrivingVoltage = 0x03,
    /// `Source Driving voltage Control` — VSH1, VSH2, VSL (0x04). Three data bytes.
    SourceDrivingVoltage = 0x04,
    /// `Booster Soft-start Control` — inrush current (0x0C). Five data bytes.
    BoosterSoftStart = 0x0c,
    /// `Deep Sleep mode` — `A[1:0] = 0b11` (`0x03`) enters deep sleep (0x10).
    ///
    /// `0x01` does **not** sleep (Table 7-1). Lotus / bb_epaper write it as a
    /// “RAM keep” mode; this crate still sends `0x03`.
    DeepSleepMode = 0x10,
    /// `Data Entry mode setting` — address increment direction (0x11).
    DataEntryMode = 0x11,
    /// `SW RESET` — resets commands and parameters, but not RAM (0x12).
    SoftwareReset = 0x12,
    /// `Temperature Sensor Control` — `0x80` selects the internal sensor (0x18).
    TemperatureSensorControl = 0x18,
    /// `Temperature Sensor Control (Write to temperature register)` (0x1A).
    /// Two data bytes, 12-bit value.
    WriteTemperatureRegister = 0x1a,
    /// `Master Activation` — runs the update sequence from 0x22 (0x20).
    MasterActivation = 0x20,
    /// `Display Update Control 1` — RAM content options (0x21).
    ///
    /// Seeed's Sticky OTP init never sends this. Lotus / bb_epaper EP397 write
    /// `{0x40, 0x00}` (full) or `{0x00, 0x00}` (partial). Those payloads are
    /// commented in [`crate::sequence`]; do not guess them onto this glass.
    DisplayUpdateControl1 = 0x21,
    /// `Display Update Control 2` — which stages Master Activation runs (0x22).
    DisplayUpdateControl2 = 0x22,
    /// `Write RAM (Black White)` — the first of the two planes (0x24).
    WriteRamBlackWhite = 0x24,
    /// `Write RAM (RED)` — the second plane; on mono film it selects a LUT
    /// rather than red ink (0x26).
    WriteRamRed = 0x26,
    /// `Write VCOM register` (0x2C). One data byte.
    WriteVcomRegister = 0x2c,
    /// `Write LUT register` — 105 bytes of waveform (0x32).
    WriteLutRegister = 0x32,
    /// `Border Waveform Control` — border (VBD) behaviour (0x3C).
    BorderWaveformControl = 0x3c,
    /// `Set RAM X - address Start / End position` — 10-bit values (0x44).
    SetRamXStartEnd = 0x44,
    /// `Set RAM Y - address Start / End position` — 10-bit values (0x45).
    SetRamYStartEnd = 0x45,
    /// `Auto Write RED RAM for Regular Pattern` (0x46).
    ///
    /// Datasheet Figure 9-1 uses this (data `0xF7`) to fill a plane on init.
    /// Table 7-1 names 0x46 as RED RAM; the same figure's prose pairs 0x46 with
    /// RAM 0x24. Seeed's Sticky driver does not send 0x46. Do not guess which
    /// plane it fills — call this only if your panel notes say to.
    AutoWriteRedRam = 0x46,
    /// `Auto Write B/W RAM for Regular Pattern` (0x47).
    ///
    /// Same caveat as [`Command::AutoWriteRedRam`]: Figure 9-1 pairs 0x47 with
    /// RAM 0x26, the table name says B/W, and Seeed's Sticky driver skips both.
    AutoWriteBwRam = 0x47,
    /// `Set RAM X address counter` — 10-bit value (0x4E).
    SetRamXCounter = 0x4e,
    /// `Set RAM Y address counter` — 10-bit value (0x4F).
    SetRamYCounter = 0x4f,
    /// `NOP` (0x7F).
    Nop = 0x7f,
}

impl Command {
    /// The opcode byte sent with D/C low.
    #[inline]
    #[must_use]
    pub const fn opcode(self) -> u8 {
        self as u8
    }
}

/// Parameter for [`Command::DeepSleepMode`]: `A[1:0] = 0b11`.
///
/// After this the chip enters deep sleep and BUSY stays high. Only a hardware
/// reset brings it back (datasheet Rev 1.0, Table 7-1).
pub const DEEP_SLEEP_ENTER: u8 = 0b11;

/// Parameter for [`Command::TemperatureSensorControl`] selecting the internal
/// sensor. Power-on default is `0x48`, the external sensor.
pub const TEMPERATURE_SENSOR_INTERNAL: u8 = 0x80;

/// Highest RAM X address unit, `0x3BF` (datasheet Rev 1.0 §8.3).
pub const RAM_X_MAX: u16 = 0x03bf;

/// Highest RAM Y address unit, `0x2A7` (datasheet Rev 1.0 §8.4).
pub const RAM_Y_MAX: u16 = 0x02a7;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcodes_match_the_datasheet_command_table() {
        assert_eq!(Command::SoftwareReset.opcode(), 0x12);
        assert_eq!(Command::WriteRamBlackWhite.opcode(), 0x24);
        assert_eq!(Command::WriteRamRed.opcode(), 0x26);
        assert_eq!(Command::WriteLutRegister.opcode(), 0x32);
        assert_eq!(Command::SetRamXStartEnd.opcode(), 0x44);
        assert_eq!(Command::SetRamYCounter.opcode(), 0x4f);
        assert_eq!(Command::MasterActivation.opcode(), 0x20);
        assert_eq!(Command::DeepSleepMode.opcode(), 0x10);
        assert_eq!(Command::BoosterSoftStart.opcode(), 0x0c);
        assert_eq!(Command::GateDrivingVoltage.opcode(), 0x03);
        assert_eq!(Command::SourceDrivingVoltage.opcode(), 0x04);
        assert_eq!(Command::WriteVcomRegister.opcode(), 0x2c);
        assert_eq!(Command::WriteTemperatureRegister.opcode(), 0x1a);
        assert_eq!(Command::AutoWriteRedRam.opcode(), 0x46);
        assert_eq!(Command::AutoWriteBwRam.opcode(), 0x47);
    }

    #[test]
    fn ram_limits_are_ten_bit() {
        assert_eq!(RAM_X_MAX, 959);
        assert_eq!(RAM_Y_MAX, 679);
    }
}
