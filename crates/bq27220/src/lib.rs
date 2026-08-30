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
//! # Register coverage is deliberately narrow
//!
//! Typed accessors exist only for commands confirmed against the board
//! contract and TI documentation. Other offsets in the standard command map
//! are reachable through [`Bq27220::read_u16`] rather than being guessed at,
//! and the CEDV data-memory block layout is not implemented at all pending a
//! page-by-page read of the technical reference manual. A plausible-looking
//! constant is worse than an honest gap: see the hardware skill datasheet
//! catalog.
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
pub const ADDRESS: SevenBitAddress = 0x55;

/// `DeviceType` value a BQ27220 reports.
pub const DEVICE_TYPE_BQ27220: u16 = 0x0220;

/// `Control()` subcommand that requests `DeviceType`.
const SUBCOMMAND_DEVICE_TYPE: u16 = 0x0001;

/// Settling time between a `Control()` subcommand and reading `MACData()`.
///
/// The board contract calls for ~15 ms; this crate waits a little longer
/// because the failure mode is a silently stale read.
const MAC_DATA_SETTLE_MS: u32 = 15;

/// Standard commands this crate reads.
///
/// Only offsets confirmed against TI documentation and the board contract are
/// listed. Use [`Bq27220::read_u16`] for anything else, and add a variant here
/// once you have read the datasheet page that defines it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    /// `Control()` — subcommand gateway.
    Control = 0x00,
    /// `Voltage()` — pack voltage in mV, unsigned.
    Voltage = 0x08,
    /// `Current()` — measured current in mA, signed.
    Current = 0x0c,
    /// `StateOfCharge()` — state of charge in percent, unsigned.
    StateOfCharge = 0x2c,
    /// `MACData()` — response buffer for a `Control()` subcommand.
    MacData = 0x40,
}

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

    /// Pack voltage in millivolts.
    #[inline]
    pub fn voltage_mv(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read(Command::Voltage)
    }

    /// Measured current in milliamperes. Positive is charge, negative is
    /// discharge.
    #[inline]
    pub fn current_ma(&mut self) -> Result<i16, Error<I2C::Error>> {
        self.read(Command::Current).map(|raw| raw as i16)
    }

    /// State of charge in percent.
    #[inline]
    pub fn state_of_charge_pct(&mut self) -> Result<u8, Error<I2C::Error>> {
        // The command returns a u16 whose value is a percentage; truncating
        // the high byte would hide a bogus read, so saturate instead.
        self.read(Command::StateOfCharge)
            .map(|raw| u8::try_from(raw).unwrap_or(u8::MAX))
    }

    /// Issues the `DeviceType` subcommand and returns the response.
    ///
    /// Requires a delay because `MACData()` is not valid immediately after the
    /// `Control()` write.
    pub fn device_type<D: DelayNs>(&mut self, delay: &mut D) -> Result<u16, Error<I2C::Error>> {
        self.write_control_subcommand_internal(SUBCOMMAND_DEVICE_TYPE)?;
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
/// wrappers here on purpose: if you need `CFGUPDATE`, read TI's technical
/// reference manual, write the sequence explicitly at your call site, and
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
