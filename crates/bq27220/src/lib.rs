//! Read-only driver for the TI BQ27220 battery fuel gauge.
//!
//! The BQ27220 is a **CEDV** gauge (compensated end-of-discharge voltage), not
//! an Impedance Track part. That matters for crate selection: [`bq27xxx`] on
//! crates.io targets the BQ27426/427 Impedance Track family and is the wrong
//! driver for this silicon, not merely an incomplete one.
//!
//! [`bq27xxx`]: https://crates.io/crates/bq27xxx
//!
//! # Reads are safe, writes are not
//!
//! Everything in the default feature set is a read. Gauge configuration lives
//! in data memory reachable only after an unseal, and the documented update
//! path is enter `CFGUPDATE`, write, verify, exit, re-seal — every step
//! timeout-prone, with a one-time-programmable OTP behind it.
//!
//! So writes require the off-by-default `config-write` feature, and even then
//! this crate exposes only documented raw primitives. It deliberately ships no
//! `enter_cfgupdate()` or `set_full_charge_capacity()` convenience wrapper:
//! nobody should reach a destructive sequence by autocomplete.
//!
//! # Standard commands
//!
//! [`Command`] lists the named rows of the TI BQ27220 TRM SLUUBD4 section
//! `2 Standard Data Commands` / `Table 2-1. Standard Commands`. Convenience
//! readers stay on the handful firmware uses; unused variants are still
//! public. [`Bq27220::read_u16`] remains the escape hatch for a garbled
//! extract row. CEDV data-memory **block** layout is not typed.
//!
//! Hazardous `Control()` subcommands (`SEALED`, `ENTER_CFG_UPDATE`,
//! `ENTER_ROM`, …) stay in a commented block — not autocomplete.
//!
//! ```
//! use bq27220::{Bq27220, DEVICE_TYPE_BQ27220};
//! # use embedded_hal_mock::eh1::i2c::{Mock, Transaction};
//! # use embedded_hal_mock::eh1::delay::NoopDelay;
//! # let i2c = Mock::new(&[
//! #     Transaction::write(0x55, vec![0x00, 0x01, 0x00]),
//! #     Transaction::write_read(0x55, vec![0x40], vec![0x20, 0x02]),
//! #     Transaction::write_read(0x55, vec![0x08], vec![0x0c, 0x10]),
//! # ]);
//! let mut gauge = Bq27220::new(i2c);
//! let mut delay = NoopDelay::new();
//!
//! assert_eq!(gauge.device_type(&mut delay).unwrap(), DEVICE_TYPE_BQ27220);
//! let millivolts = gauge.voltage_mv().unwrap();
//! # assert_eq!(millivolts, 4108);
//! # gauge.release().done();
//! ```

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(test)]
extern crate std;

use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::{I2c, SevenBitAddress};

/// Default I2C address of the gauge on the reTerminal Sticky sensor bus.
///
/// The TI BQ27220 datasheet SLUSCB7 section `7.3.1.1 I2C Interface` says the
/// 7-bit device address is fixed as `1010101` (`0x55`).
pub const ADDRESS: SevenBitAddress = 0x55;

/// Value SLUUBD4 section `2.2.2 DEVICE_NUMBER: 0x0001` says `MACData()`
/// returns for this part (`0x0220`).
pub const DEVICE_TYPE_BQ27220: u16 = 0x0220;

/// Settling time between a `Control()` subcommand and reading `MACData()`.
///
/// The board contract calls for ~15 ms; this crate waits that long because a
/// silently stale `MACData()` is the failure mode.
///
/// The TI BQ27220 datasheet SLUSCB7 section `7.3.1.3 I2C Command Waiting Time`
/// shows 66 ms on the Control-subcommand / status-read figure (and `t(BUF)` ≥
/// 66 μs between packets). Leave the sheet figure unused:
///
/// ```text
/// // SLUSCB7 `7.3.1.3 I2C Command Waiting Time` figure: 66 ms between the
/// // Control() subcommand write and reading the status / MACData() result.
/// // const MAC_DATA_SETTLE_MS_SLUSCB7_FIGURE: u32 = 66;
/// ```
const MAC_DATA_SETTLE_MS: u32 = 15;

/// First byte of a standard-command pair from SLUUBD4 `Table 2-1. Standard
/// Commands`. Each command is two consecutive offsets; this enum names the
/// first byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    /// SLUUBD4 `2.2 Control()/CONTROL_STATUS(): 0x00 and 0x01`.
    Control = 0x00,
    /// SLUUBD4 `2.3 AtRate(): 0x02 and 0x03`. Table 2-1 unit: mA. RW.
    AtRate = 0x02,
    /// SLUUBD4 `2.4 AtRateTimeToEmpty(): 0x04 and 0x05`. Minutes. R.
    AtRateTimeToEmpty = 0x04,
    /// SLUUBD4 `2.5 Temperature(): 0x06 and 0x07`. 0.1 K. RW.
    Temperature = 0x06,
    /// SLUUBD4 `2.6 Voltage(): 0x08 and 0x09`. Table 2-1 / section: mV. R.
    Voltage = 0x08,
    /// SLUUBD4 `2.7 BatteryStatus(): 0x0A and 0x0B`. R.
    BatteryStatus = 0x0a,
    /// SLUUBD4 `2.8 Current(): 0x0C and 0x0D`. Section `2.8` units are **mA**
    /// (signed). Table 2-1 UNIT column says mAh — follow the section text.
    Current = 0x0c,
    /// SLUUBD4 `2.9 RemainingCapacity(): 0x10 and 0x11`. mAh. R.
    RemainingCapacity = 0x10,
    /// SLUUBD4 `2.10 FullChargeCapacity(): 0x12 and 0x13`. mAh. **Read** of
    /// compensated full capacity, not a data-memory FCC write.
    FullChargeCapacity = 0x12,
    /// SLUUBD4 `Table 2-1. Standard Commands` `AverageCurrent()`. 0x14/0x15. mA.
    AverageCurrent = 0x14,
    /// SLUUBD4 `2.11 TimeToEmpty(): 0x16 and 0x17`. Minutes. R.
    TimeToEmpty = 0x16,
    /// SLUUBD4 `2.12 TimeToFull(): 0x18 and 0x19`. Minutes. R.
    TimeToFull = 0x18,
    /// SLUUBD4 `2.13 StandbyCurrent(): 0x1A and 0x1B`. mA. R.
    StandbyCurrent = 0x1a,
    /// SLUUBD4 `2.14 StandbyTimeToEmpty(): 0x1C and 0x1D`. Minutes. R.
    StandbyTimeToEmpty = 0x1c,
    /// SLUUBD4 `2.15 MaxLoadCurrent(): 0x1E and 0x1F`. mA. R.
    MaxLoadCurrent = 0x1e,
    /// SLUUBD4 `2.16 MaxLoadTimeToEmpty(): 0x20 and 0x21`. min. R.
    MaxLoadTimeToEmpty = 0x20,
    /// SLUUBD4 `2.17 RawCoulombCount(): 0x22 and 0x23`. mAh. R.
    RawCoulombCount = 0x22,
    /// SLUUBD4 `2.18 AveragePower(): 0x24 and 0x25`. mW. R.
    AveragePower = 0x24,
    /// SLUUBD4 `2.19 InternalTemperature(): 0x28 and 0x29`. 0.1 K. R.
    InternalTemperature = 0x28,
    /// SLUUBD4 `2.20 CycleCount(): 0x2A and 0x2B`. R.
    CycleCount = 0x2a,
    /// SLUUBD4 `2.21 StateOfCharge(): 0x2C and 0x2D` (`RelativeStateOfCharge`
    /// in Table 2-1). Percent, 0–100. R.
    StateOfCharge = 0x2c,
    /// SLUUBD4 `2.22 StateOfHealth(): 0x2E and 0x2F`. R.
    StateOfHealth = 0x2e,
    /// SLUUBD4 `Table 2-1` `ChargeVoltage()` (TOC: `2.23 ChargingVoltage()`).
    /// 0x30/0x31. mV. R.
    ChargeVoltage = 0x30,
    /// SLUUBD4 `Table 2-1` `ChargeCurrent()` (TOC: `2.24 ChargingCurrent()`).
    /// 0x32/0x33. mA. R.
    ChargeCurrent = 0x32,
    /// SLUUBD4 `2.25 BTPDischargeSet(): 0x34 and 0x35`. mAh.
    BtpDischargeSet = 0x34,
    /// SLUUBD4 `2.26 BTPChargeSet(): 0x36 and 0x37`. mAh.
    BtpChargeSet = 0x36,
    /// SLUUBD4 `2.27 OperationStatus(): 0x3A and 0x3B`. R.
    OperationStatus = 0x3a,
    /// SLUUBD4 `2.28 DesignCapacity(): 0x3C and 0x3D`. mAh. R.
    DesignCapacity = 0x3c,
    /// SLUUBD4 `2.29 MACData(): 0x40 through 0x5F`. First byte of the MAC
    /// response window.
    MacData = 0x40,
    /// SLUUBD4 `2.30 MACDataSum(): 0x60`.
    MacDataSum = 0x60,
    /// SLUUBD4 `2.31 MACDataLen(): 0x61`.
    MacDataLen = 0x61,
    /// SLUUBD4 `2.32 AnalogCount(): 0x79`.
    AnalogCount = 0x79,
    /// SLUUBD4 `2.33 RawCurrent(): 0x7A and 0x7B`.
    RawCurrent = 0x7a,
    /// SLUUBD4 `2.34 RawVoltage(): 0x7C and 0x7D`.
    RawVoltage = 0x7c,
    /// SLUUBD4 `Table 2-1. Standard Commands` (continued) `RawIntTemp()`.
    /// 0x7E/0x7F.
    RawIntTemp = 0x7e,
}

/// Read-safe `Control()` MAC subcommands from SLUUBD4 `2.2
/// Control()/CONTROL_STATUS(): 0x00 and 0x01` / `Table 2-2. Control() MAC
/// Subcommands`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ControlSubcommand {
    /// SLUUBD4 `2.2.1 CONTROL_STATUS: 0x0000`.
    ControlStatus = 0x0000,
    /// SLUUBD4 `2.2.2 DEVICE_NUMBER: 0x0001`. Response `0x0220` in `MACData()`.
    DeviceNumber = 0x0001,
    /// SLUUBD4 `2.2.3 FW_VERSION: 0x0002`.
    FwVersion = 0x0002,
    /// SLUUBD4 `2.2.4 HW_VERSION: 0x0003`.
    HwVersion = 0x0003,
    /// SLUUBD4 `2.2.5 BOARD_OFFSET: 0x0009`.
    BoardOffset = 0x0009,
    /// SLUUBD4 `2.2.6 CC_OFFSET: 0x000A`.
    CcOffset = 0x000a,
    /// SLUUBD4 `2.2.17 OPERATION_STATUS: 0x0054`.
    OperationStatus = 0x0054,
    /// SLUUBD4 `2.2.18 GaugingStatus: 0x0056`.
    GaugingStatus = 0x0056,
}

impl ControlSubcommand {
    /// Little-endian subcommand word written to `Control()`.
    #[inline]
    #[must_use]
    pub const fn word(self) -> u16 {
        self as u16
    }
}

// Hazardous Control() MAC subcommands (SLUUBD4 `2.2 Control()/CONTROL_STATUS()`
// / `Table 2-2`). Not compiled: a named variant here is autocomplete for a
// one-way or OTP-adjacent sequence. `config-write` stays raw `u16` only.
//
// SLUUBD4 `2.2.7 CC_OFFSET_SAVE: 0x000B`
// SLUUBD4 `2.2.8 OCV_CMD: 0x000C`
// SLUUBD4 `2.2.9 BAT_INSERT: 0x000D`
// SLUUBD4 `2.2.10 BAT_REMOVE: 0x000E`
// SLUUBD4 `2.2.11 SET_SNOOZE: 0x0013`
// SLUUBD4 `2.2.12 CLEAR_SNOOZE: 0x0014`
// SLUUBD4 `2.2.13 SET_PROFILE_1/2/3/4/5/6: 0x0015–0x001A`
// SLUUBD4 `2.2.14 CAL_TOGGLE: 0x002D`
// SLUUBD4 `2.2.15 SEALED: 0x0030`
// SLUUBD4 `2.2.16 RESET: 0x0041`
// SLUUBD4 `2.2.19 EXIT_CAL: 0x0080`
// SLUUBD4 `2.2.20 ENTER_CAL: 0x0081`
// SLUUBD4 `2.2.21 ENTER_CFG_UPDATE: 0x0090`
// SLUUBD4 `2.2.22 EXIT_CFG_UPDATE_REINIT: 0x0091`
// SLUUBD4 `2.2.23 EXIT_CFG_UPDATE: 0x0092`
// SLUUBD4 `2.2.24 ENTER_ROM: 0x0F00`
// SLUUBD4 `3.3 Sealing and Unsealing Data Memory Access` (unseal keys)
//
// // const CONTROL_SEALED: u16 = 0x0030;
// // const CONTROL_ENTER_CFG_UPDATE: u16 = 0x0090;
// // const CONTROL_EXIT_CFG_UPDATE_REINIT: u16 = 0x0091;
// // const CONTROL_EXIT_CFG_UPDATE: u16 = 0x0092;
// // const CONTROL_ENTER_ROM: u16 = 0x0F00;

impl Command {
    /// The register offset on the wire.
    #[inline]
    #[must_use]
    pub const fn offset(self) -> u8 {
        self as u8
    }
}

/// Errors this driver can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<E> {
    /// The underlying I2C bus failed.
    I2c(E),
    /// A `DeviceType` read returned something other than a BQ27220.
    ///
    /// Treat this as "do not write to this part": the register map you are
    /// about to use may not be the one this silicon implements.
    UnexpectedDeviceType(u16),
}

impl<E> From<E> for Error<E> {
    #[inline]
    fn from(value: E) -> Self {
        Self::I2c(value)
    }
}

/// A BQ27220 on an I2C bus.
///
/// The bus is not locked internally: pass an `I2c` implementation (typically a
/// shared-bus device from `embedded-hal-bus`) so the caller keeps control of
/// arbitration on a bus this board shares between four sensors.
#[derive(Debug)]
pub struct Bq27220<I2C> {
    i2c: I2C,
    address: SevenBitAddress,
}

impl<I2C: I2c> Bq27220<I2C> {
    /// Wraps a gauge at the default [`ADDRESS`].
    #[inline]
    pub const fn new(i2c: I2C) -> Self {
        Self {
            i2c,
            address: ADDRESS,
        }
    }

    /// Wraps a gauge at a caller-chosen address.
    #[inline]
    pub const fn with_address(i2c: I2C, address: SevenBitAddress) -> Self {
        Self { i2c, address }
    }

    /// Reads a little-endian `u16` from an arbitrary standard-command offset.
    ///
    /// This is the escape hatch for offsets that do not yet have a typed
    /// accessor. Cite the datasheet in your call site.
    pub fn read_u16(&mut self, offset: u8) -> Result<u16, Error<I2C::Error>> {
        let mut buf = [0u8; 2];
        self.i2c
            .write_read(self.address, &[offset], &mut buf)
            .map_err(Error::I2c)?;
        Ok(u16::from_le_bytes(buf))
    }

    /// Reads a typed standard command as a little-endian `u16`.
    #[inline]
    pub fn read(&mut self, command: Command) -> Result<u16, Error<I2C::Error>> {
        self.read_u16(command.offset())
    }

    /// Pack voltage in millivolts (SLUUBD4 `2.6 Voltage(): 0x08 and 0x09`).
    #[inline]
    pub fn voltage_mv(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read(Command::Voltage)
    }

    /// Instantaneous current in milliamperes (SLUUBD4 `2.8 Current(): 0x0C
    /// and 0x0D`; signed). Positive is charge, negative is discharge.
    #[inline]
    pub fn current_ma(&mut self) -> Result<i16, Error<I2C::Error>> {
        self.read(Command::Current).map(|raw| raw as i16)
    }

    /// State of charge in percent (SLUUBD4 `2.21 StateOfCharge(): 0x2C and
    /// 0x2D`, range 0–100).
    #[inline]
    pub fn state_of_charge_pct(&mut self) -> Result<u8, Error<I2C::Error>> {
        // The command returns a u16 whose value is a percentage; truncating
        // the high byte would hide a bogus read, so saturate instead.
        self.read(Command::StateOfCharge)
            .map(|raw| u8::try_from(raw).unwrap_or(u8::MAX))
    }

    /// Issues SLUUBD4 `2.2.2 DEVICE_NUMBER: 0x0001` and returns `MACData()`.
    ///
    /// Requires a delay because `MACData()` is not valid immediately after the
    /// `Control()` write (this crate waits 15 ms; see `MAC_DATA_SETTLE_MS`).
    pub fn device_type<D: DelayNs>(&mut self, delay: &mut D) -> Result<u16, Error<I2C::Error>> {
        self.write_control_subcommand_internal(ControlSubcommand::DeviceNumber.word())?;
        delay.delay_ms(MAC_DATA_SETTLE_MS);
        self.read(Command::MacData)
    }

    /// Confirms the part really is a BQ27220 before you trust the register map.
    ///
    /// Worth calling once at bring-up: an unexpected `DeviceType` means the
    /// offsets you are about to read are not necessarily the ones this silicon
    /// implements.
    pub fn verify_device_type<D: DelayNs>(
        &mut self,
        delay: &mut D,
    ) -> Result<(), Error<I2C::Error>> {
        let reported = self.device_type(delay)?;
        if reported == DEVICE_TYPE_BQ27220 {
            Ok(())
        } else {
            Err(Error::UnexpectedDeviceType(reported))
        }
    }

    /// Writes a `Control()` subcommand. Internal so that the public surface
    /// stays read-only unless `config-write` is enabled.
    fn write_control_subcommand_internal(
        &mut self,
        subcommand: u16,
    ) -> Result<(), Error<I2C::Error>> {
        let bytes = subcommand.to_le_bytes();
        self.i2c
            .write(
                self.address,
                &[Command::Control.offset(), bytes[0], bytes[1]],
            )
            .map_err(Error::I2c)
    }

    /// Consumes the driver and returns the bus (`C-FREE`).
    #[inline]
    pub fn release(self) -> I2C {
        self.i2c
    }
}

/// Raw write primitives, gated behind the `config-write` feature.
///
/// # Danger
///
/// Enabling this feature unlocks paths that can permanently misconfigure a
/// gauge, and the OTP is one-time-programmable. There are no convenience
/// wrappers here on purpose: if you need `CFGUPDATE`, read SLUUBD4 section
/// `2.2.21 ENTER_CFG_UPDATE: 0x0090` and `3.3 Sealing and Unsealing Data
/// Memory Access`, write the sequence explicitly at your call site, and
/// verify every step. See `docs/SAFETY.md`.
#[cfg(feature = "config-write")]
impl<I2C: I2c> Bq27220<I2C> {
    /// Writes a raw `Control()` subcommand.
    ///
    /// # Danger
    ///
    /// Subcommands include seal-state changes and configuration-mode entry.
    pub fn write_control_subcommand(&mut self, subcommand: u16) -> Result<(), Error<I2C::Error>> {
        self.write_control_subcommand_internal(subcommand)
    }

    /// Writes a little-endian `u16` to an arbitrary command offset.
    ///
    /// # Danger
    ///
    /// This can write configuration the gauge relies on for correct reporting,
    /// and a partial `CFGUPDATE` sequence can leave the part inconsistent.
    pub fn write_u16(&mut self, offset: u8, value: u16) -> Result<(), Error<I2C::Error>> {
        let bytes = value.to_le_bytes();
        self.i2c
            .write(self.address, &[offset, bytes[0], bytes[1]])
            .map_err(Error::I2c)
    }
}

#[cfg(test)]
mod tests {
    use embedded_hal_mock::eh1::delay::{CheckedDelay, Transaction as DelayTransaction};
    use embedded_hal_mock::eh1::i2c::{Mock, Transaction};
    use std::vec;

    use super::*;

    #[test]
    fn voltage_is_read_little_endian() {
        let i2c = Mock::new(&[Transaction::write_read(
            ADDRESS,
            vec![0x08],
            vec![0x0c, 0x10],
        )]);
        let mut gauge = Bq27220::new(i2c);
        assert_eq!(gauge.voltage_mv().unwrap(), 0x100c);
        gauge.release().done();
    }

    #[test]
    fn current_is_signed() {
        // -250 mA as little-endian two's complement.
        let i2c = Mock::new(&[Transaction::write_read(
            ADDRESS,
            vec![0x0c],
            vec![0x06, 0xff],
        )]);
        let mut gauge = Bq27220::new(i2c);
        assert_eq!(gauge.current_ma().unwrap(), -250);
        gauge.release().done();
    }

    #[test]
    fn state_of_charge_saturates_instead_of_truncating() {
        let i2c = Mock::new(&[
            Transaction::write_read(ADDRESS, vec![0x2c], vec![0x64, 0x00]),
            Transaction::write_read(ADDRESS, vec![0x2c], vec![0x00, 0x01]),
        ]);
        let mut gauge = Bq27220::new(i2c);
        assert_eq!(gauge.state_of_charge_pct().unwrap(), 100);
        // 0x0100 would truncate to 0; saturating keeps the read obviously bad.
        assert_eq!(gauge.state_of_charge_pct().unwrap(), u8::MAX);
        gauge.release().done();
    }

    #[test]
    fn device_type_waits_before_reading_mac_data() {
        let i2c = Mock::new(&[
            Transaction::write(ADDRESS, vec![0x00, 0x01, 0x00]),
            Transaction::write_read(ADDRESS, vec![0x40], vec![0x20, 0x02]),
        ]);
        let mut delay = CheckedDelay::new(&[DelayTransaction::delay_ms(MAC_DATA_SETTLE_MS)]);

        let mut gauge = Bq27220::new(i2c);
        assert_eq!(gauge.device_type(&mut delay).unwrap(), DEVICE_TYPE_BQ27220);

        delay.done();
        gauge.release().done();
    }

    #[test]
    fn verify_device_type_rejects_a_foreign_part() {
        let i2c = Mock::new(&[
            Transaction::write(ADDRESS, vec![0x00, 0x01, 0x00]),
            Transaction::write_read(ADDRESS, vec![0x40], vec![0x26, 0x04]),
        ]);
        let mut delay = CheckedDelay::new(&[DelayTransaction::delay_ms(MAC_DATA_SETTLE_MS)]);

        let mut gauge = Bq27220::new(i2c);
        assert_eq!(
            gauge.verify_device_type(&mut delay),
            Err(Error::UnexpectedDeviceType(0x0426))
        );

        delay.done();
        gauge.release().done();
    }

    #[test]
    fn table_2_1_first_bytes_match() {
        assert_eq!(Command::AtRate.offset(), 0x02);
        assert_eq!(Command::FullChargeCapacity.offset(), 0x12);
        assert_eq!(Command::AverageCurrent.offset(), 0x14);
        assert_eq!(Command::MacDataSum.offset(), 0x60);
        assert_eq!(Command::RawIntTemp.offset(), 0x7e);
        assert_eq!(ControlSubcommand::DeviceNumber.word(), 0x0001);
    }

    #[test]
    fn raw_reads_are_available_for_undocumented_offsets() {
        let i2c = Mock::new(&[Transaction::write_read(
            ADDRESS,
            vec![0x0e],
            vec![0x2c, 0x01],
        )]);
        let mut gauge = Bq27220::new(i2c);
        assert_eq!(gauge.read_u16(0x0e).unwrap(), 0x012c);
        gauge.release().done();
    }

    #[cfg(feature = "config-write")]
    #[test]
    fn config_write_exposes_raw_primitives_only() {
        let i2c = Mock::new(&[Transaction::write(ADDRESS, vec![0x00, 0x14, 0x00])]);
        let mut gauge = Bq27220::new(i2c);
        gauge.write_control_subcommand(0x0014).unwrap();
        gauge.release().done();
    }
}
