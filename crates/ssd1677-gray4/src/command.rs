//! SSD1677 command opcodes.
//!
//! Variants are named rows of the Solomon Systech SSD1677 datasheet **Rev 1.0
//! (Nov 2018)** section `Table 7-1: Command Table`. Unused opcodes are still
//! listed. OTP-program / waveform-program commands stay in the commented
//! block at the bottom of this file — those write the panel OTP. Do not
//! pattern-match other SSD16xx drivers; this part uses 10-bit RAM address
//! units (sections `8.3`–`8.5`).

/// A datasheet-verified SSD1677 command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[non_exhaustive]
pub enum Command {
    /// Table 7-1 `Driver Output control` (0x01). Section `8.1 Driver Output
    /// Control (01h)`.
    DriverOutputControl = 0x01,
    /// Table 7-1 `Gate Driving voltage Control` (0x03).
    GateDrivingVoltage = 0x03,
    /// Table 7-1 `Source Driving voltage Control` (0x04).
    SourceDrivingVoltage = 0x04,
    /// Table 7-1 `Booster Soft-start Control` (0x0C).
    BoosterSoftStart = 0x0c,
    /// Table 7-1 `Deep Sleep mode`. Parameter is [`DeepSleep`].
    DeepSleepMode = 0x10,
    /// Table 7-1 `Data Entry mode setting` (0x11). Section `8.2 Data Entry
    /// Mode Setting (11h)`.
    DataEntryMode = 0x11,
    /// Table 7-1 `SW RESET` (0x12).
    SoftwareReset = 0x12,
    /// Table 7-1 `HV Ready Detection` (0x14).
    HvReadyDetection = 0x14,
    /// Table 7-1 `VCI Detection` (0x15).
    VciDetection = 0x15,
    /// Table 7-1 `Temperature Sensor Selection` (0x18).
    TemperatureSensorControl = 0x18,
    /// Table 7-1 `Temperature Sensor` write to temperature register (0x1A).
    WriteTemperatureRegister = 0x1a,
    /// Table 7-1 `Temperature Sensor` read from temperature register (0x1B).
    ReadTemperatureRegister = 0x1b,
    /// Table 7-1 `Temperature Sensor` write command to external sensor (0x1C).
    WriteExternalTemperature = 0x1c,
    /// Table 7-1 `Master Activation`. Follows [`Self::DisplayUpdateControl2`].
    MasterActivation = 0x20,
    /// Table 7-1 `Display Update` RAM content option (0x21).
    ///
    /// Seeed Sticky OTP init never sends this. Lotus / bb_epaper payloads stay
    /// commented in [`crate::sequence`].
    DisplayUpdateControl1 = 0x21,
    /// Table 7-1 `Display Update Sequence Option`. Parameter is [`crate::UpdateSequence`].
    DisplayUpdateControl2 = 0x22,
    /// Table 7-1 `Write RAM (Black White)` (0x24). Section `6.5 RAM`.
    WriteRamBlackWhite = 0x24,
    /// Table 7-1 `Write RAM (Dithering)` (0x25).
    WriteRamDithering = 0x25,
    /// Table 7-1 `Write RAM (RED)` (0x26). On mono film this is the second
    /// LUT-index plane, not red ink.
    WriteRamRed = 0x26,
    /// Table 7-1 `Read RAM` (0x27).
    ReadRam = 0x27,
    /// Table 7-1 `VCOM Sense` (0x28).
    VcomSense = 0x28,
    /// Table 7-1 `VCOM Sense Duration` (0x29).
    VcomSenseDuration = 0x29,
    /// Table 7-1 `Write Register for` glitch-reduction (0x2B).
    WriteRegisterGlitch = 0x2b,
    /// Table 7-1 `Write VCOM register` (0x2C).
    WriteVcomRegister = 0x2c,
    /// Table 7-1 `OTP Register Read for Display Option` (0x2D). **Read.**
    OtpRegisterRead = 0x2d,
    /// Table 7-1 `User ID Read` (0x2E). **Read** of OTP user id.
    UserIdRead = 0x2e,
    /// Table 7-1 `Status Bit Read` (0x2F).
    StatusBitRead = 0x2f,
    /// Table 7-1 `Load WS OTP` (0x31). Loads waveform from OTP; does not
    /// program OTP. Sticky uses OTP via `0x22` stage bits more often than
    /// this opcode.
    LoadWsOtp = 0x31,
    /// Table 7-1 `Write LUT register` (0x32), 105 bytes. Sticky path is OTP
    /// (`6.10 One Time Programmable (OTP) Memory`), not this command.
    WriteLutRegister = 0x32,
    /// Table 7-1 `CRC calculation` (0x34).
    CrcCalculation = 0x34,
    /// Table 7-1 `CRC Status Read` (0x35).
    CrcStatusRead = 0x35,
    /// Table 7-1 `Write Register for Display Option` (0x37).
    WriteRegisterDisplayOption = 0x37,
    /// Table 7-1 `Border Waveform Control` (0x3C).
    BorderWaveformControl = 0x3c,
    /// Table 7-1 `Read RAM Option` (0x41).
    ReadRamOption = 0x41,
    /// Table 7-1 `Set RAM X - address Start / End position` (0x44).
    /// Section `8.3 Set RAM X - Address Start / End Position (44h)`.
    SetRamXStartEnd = 0x44,
    /// Table 7-1 `Set RAM Y- address Start / End position` (0x45).
    /// Section `8.4 Set RAM Y - Address Start / End Position (45h)`.
    SetRamYStartEnd = 0x45,
    /// Table 7-1 `Auto Write RED RAM for Regular Pattern` (0x46).
    AutoWriteRedRam = 0x46,
    /// Table 7-1 `Auto Write B/W RAM for Regular Pattern` (0x47).
    AutoWriteBwRam = 0x47,
    /// Table 7-1 `Dithering engine` (0x4D).
    DitheringEngine = 0x4d,
    /// Table 7-1 `Set RAM X address counter` (0x4E).
    /// Section `8.5 Set RAM Address Counter (4EH-4FH)`.
    SetRamXCounter = 0x4e,
    /// Table 7-1 `Set RAM Y address counter` (0x4F).
    /// Section `8.5 Set RAM Address Counter (4EH-4FH)`.
    SetRamYCounter = 0x4f,
    /// Table 7-1 `NOP` (0x7F).
    Nop = 0x7f,
}

// Table 7-1 commands that **program** OTP or enter OTP program mode.
// Leave commented — section `6.10 One Time Programmable (OTP) Memory`.
//
// 0x08 Initial Code Setting OTP Program
// 0x09 Write Register for Initial Code Setting
// 0x0A Read Register for Initial Code Setting
// 0x2A Program VCOM OTP
// 0x30 Program WS OTP
// 0x36 Program OTP selection
// 0x38 Write Register for User ID
// 0x39 OTP program mode
// 0x3A / 0x3B Reserved
//
// // const INITIAL_CODE_SETTING_OTP_PROGRAM: u8 = 0x08;
// // const PROGRAM_VCOM_OTP: u8 = 0x2A;
// // const PROGRAM_WS_OTP: u8 = 0x30;
// // const OTP_PROGRAM_MODE: u8 = 0x39;

impl Command {
    /// The opcode byte sent with D/C low.
    #[inline]
    #[must_use]
    pub const fn opcode(self) -> u8 {
        self as u8
    }
}

/// Table 7-1 parameter for [`Command::DeepSleepMode`].
///
/// Hex belongs only here and in the opcode test. Callers use the variant
/// name. [`Self::Enter`] is the only value that sleeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DeepSleep {
    /// `A[1:0] = 00` — Normal Mode (POR). Not a sleep.
    Normal = 0b00,
    /// `A[1:0] = 01` — does **not** sleep (Lotus / bb_epaper “keep RAM”).
    Inactive = 0b01,
    /// `A[1:0] = 11` — enter deep sleep. BUSY stays high. Exit needs HWRESET.
    Enter = 0b11,
}

impl DeepSleep {
    /// The byte written after [`Command::DeepSleepMode`].
    #[inline]
    #[must_use]
    pub const fn byte(self) -> u8 {
        self as u8
    }
}

/// [`DeepSleep::Enter`] as a byte. Prefer [`DeepSleep::Enter`] in new code.
pub const DEEP_SLEEP_ENTER: u8 = DeepSleep::Enter.byte();

/// Parameter for [`Command::TemperatureSensorControl`] selecting the internal
/// sensor (`0x80`). Table 7-1 temperature-sensor selection; POR for the
/// external path is `0x48` in the extract.
pub const TEMPERATURE_SENSOR_INTERNAL: u8 = 0x80;

/// Highest RAM X address unit, `0x3BF`.
///
/// SSD1677 Rev 1.0 section `8.3 Set RAM X - Address Start / End Position (44h)`.
pub const RAM_X_MAX: u16 = 0x03bf;

/// Highest RAM Y address unit, `0x2A7`.
///
/// SSD1677 Rev 1.0 section `8.4 Set RAM Y - Address Start / End Position (45h)`.
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
        assert_eq!(Command::DisplayUpdateControl2.opcode(), 0x22);
        assert_eq!(Command::DeepSleepMode.opcode(), 0x10);
        assert_eq!(Command::BoosterSoftStart.opcode(), 0x0c);
        assert_eq!(Command::GateDrivingVoltage.opcode(), 0x03);
        assert_eq!(Command::SourceDrivingVoltage.opcode(), 0x04);
        assert_eq!(Command::WriteVcomRegister.opcode(), 0x2c);
        assert_eq!(Command::WriteTemperatureRegister.opcode(), 0x1a);
        assert_eq!(Command::HvReadyDetection.opcode(), 0x14);
        assert_eq!(Command::WriteRamDithering.opcode(), 0x25);
        assert_eq!(Command::LoadWsOtp.opcode(), 0x31);
        assert_eq!(Command::DitheringEngine.opcode(), 0x4d);
        assert_eq!(Command::AutoWriteRedRam.opcode(), 0x46);
        assert_eq!(Command::AutoWriteBwRam.opcode(), 0x47);
    }

    #[test]
    fn deep_sleep_param_matches_table_7_1() {
        assert_eq!(DeepSleep::Normal.byte(), 0b00);
        assert_eq!(DeepSleep::Inactive.byte(), 0b01);
        assert_eq!(DeepSleep::Enter.byte(), 0b11);
        assert_eq!(DEEP_SLEEP_ENTER, DeepSleep::Enter.byte());
    }

    #[test]
    fn ram_limits_are_ten_bit() {
        assert_eq!(RAM_X_MAX, 959);
        assert_eq!(RAM_Y_MAX, 679);
    }
}
