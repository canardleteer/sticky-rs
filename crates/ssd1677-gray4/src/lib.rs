//! Driver for the Solomon Systech **SSD1677** e-paper controller, with
//! four gray levels on black-and-white film.
//!
//! The long explanation of how e-paper works, why this panel uses factory
//! waveforms, and what the acronyms mean is
//! [docs/ssd1677.md](../../../docs/ssd1677.md). Register facts come from the
//! SSD1677 datasheet **Rev 1.0 (Nov 2018)** and are cited at each definition.
//! Board wiring is not this crate's business: it depends on `embedded-hal` 1.0
//! only and knows nothing about ESP32-S3.
//!
//! # How e-paper differs from an LCD
//!
//! Charged pigment moves when you apply a voltage for a measured time, then
//! **stays**. That timed recipe is a **waveform**. A recipe meant for another
//! module can damage this film slowly. A full update takes seconds; the
//! controller holds the BUSY pin high while it runs. Do not send commands or
//! drop the panel rail until it falls.
//!
//! # Why this crate exists
//!
//! Existing SSD1677 crates drive black/white(/red) panels. This one targets
//! **mono** film showing four gray levels. The controller has no grayscale
//! mode. Grayscale is two things together:
//!
//! 1. **Two RAM planes** ([`planes`]) whose bit pair selects waveform slot
//!    LUT0..LUT3 (look-up table index 0 through 3).
//! 2. Either the **panel OTP** (Rev 1.0 section `6.10 One Time Programmable
//!    (OTP) Memory`, no `Write LUT register` — Seeed Sticky) or a
//!    **panel-specific 105-byte table** the microcontroller writes with
//!    Table 7-1 `Write LUT register` ([`Lut`]).
//!
//! The datasheet cannot invent a safe four-gray table, so this crate ships
//! **no default look-up table**. The Sticky's confirmed path uses OTP
//! sequences in [`sequence`] and
//! [`crate::planes::PlaneMapping::SEEED_OTP`]. See [`lut`].
//!
//! # Waiting on BUSY
//!
//! BUSY is high while the controller works. [`Ssd1677::wait_until_idle`]
//! polls; with the `async` feature, `Ssd1677::wait_until_idle_async` waits on
//! an edge instead, which is what you want on battery. Either way, do not
//! issue commands during an update: the datasheet warns that interrupting
//! Master Activation corrupts the image.
//!
//! # Typestate
//!
//! Deep sleep is a type state, not a flag. In [`Asleep`] there are no command
//! methods to call, and the only way back is [`Ssd1677::wake`], which performs
//! the hardware reset the datasheet requires.
//!
//! Stock panel **standby** is not a type state. [`Ssd1677::standby`] and
//! [`Ssd1677::resume`] stay on [`Active`]: they run Table 7-1 `0x22`
//! sequences (disable analog+clock / enable clock+analog) and keep RAM.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(test)]
extern crate std;

pub mod analog;
pub mod command;
#[cfg(feature = "graphics")]
pub mod graphics;
pub mod lut;
pub mod planes;
pub mod sequence;

use core::marker::PhantomData;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;

pub use crate::analog::{AnalogVoltages, GateVoltage, SourceVoltage, Vcom};
pub use crate::command::Command;
pub use crate::lut::{Lut, LUT_LEN};
pub use crate::planes::{PackError, PlaneMapping};
pub use crate::sequence::{BoosterSoftStart, GateScan, UpdateSequence};

/// Reset pulse width from the board contract: 10 ms low, 10 ms high.
const RESET_PULSE_MS: u32 = 10;

/// How often [`Ssd1677::wait_until_idle`] samples BUSY.
const BUSY_POLL_MS: u32 = 10;

/// Errors this driver can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<SpiError, PinError> {
    /// The SPI bus failed.
    Spi(SpiError),
    /// A GPIO operation failed.
    Pin(PinError),
    /// BUSY stayed high past the caller's timeout.
    ///
    /// Treat this as "the panel may be mid-update": do not power down the rail
    /// and do not retry blindly.
    BusyTimeout,
    /// A RAM address exceeded the controller's 10-bit limits.
    AddressOutOfRange {
        /// The offending value.
        value: u16,
        /// Maximum from `8.3` / `8.4` (`RAM_X_MAX` / `RAM_Y_MAX`).
        max: u16,
    },
}

/// A failed transition into deep sleep, returning the still-active driver so
/// the caller can retry rather than lose the bus and pins.
#[derive(Debug)]
pub struct SleepError<SPI, DC, RST, BUSY, DELAY>
where
    SPI: embedded_hal::spi::ErrorType,
    DC: embedded_hal::digital::ErrorType,
{
    /// The driver, still awake.
    pub driver: Ssd1677<SPI, DC, RST, BUSY, DELAY, Active>,
    /// The underlying error.
    pub source: Error<SPI::Error, DC::Error>,
}

/// What [`Ssd1677::sleep`] returns: the sleeping driver, or [`SleepError`].
pub type SleepResult<SPI, DC, RST, BUSY, DELAY> =
    Result<Ssd1677<SPI, DC, RST, BUSY, DELAY, Asleep>, SleepError<SPI, DC, RST, BUSY, DELAY>>;

/// A RAM window in datasheet address units.
///
/// Values are passed through unmodified. SSD1677 Rev 1.0 sections `8.3 Set
/// RAM X - Address Start / End Position (44h)` and `8.4 Set RAM Y - Address
/// Start / End Position (45h)` limit X to `0x000..=0x3BF` and Y to
/// `0x000..=0x2A7`. This crate does **not** silently divide by 8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// First X address unit.
    pub x_start: u16,
    /// Last X address unit, inclusive.
    pub x_end: u16,
    /// First Y address unit.
    pub y_start: u16,
    /// Last Y address unit, inclusive.
    pub y_end: u16,
}

impl Window {
    /// Validates against the datasheet limits.
    fn validate<S, P>(&self) -> Result<(), Error<S, P>> {
        for value in [self.x_start, self.x_end] {
            if value > command::RAM_X_MAX {
                return Err(Error::AddressOutOfRange {
                    value,
                    max: command::RAM_X_MAX,
                });
            }
        }
        for value in [self.y_start, self.y_end] {
            if value > command::RAM_Y_MAX {
                return Err(Error::AddressOutOfRange {
                    value,
                    max: command::RAM_Y_MAX,
                });
            }
        }
        Ok(())
    }
}

/// Initialisation parameters.
///
/// Order follows Seeed `ssd1677_init_base` (stock `reterminal_template`):
/// optional software reset, temperature sensor, border, booster, gate scan,
/// data entry, optional analog, window, cursor, optional microcontroller
/// look-up table.
///
/// On the Sticky, analog stays `None` and `lut` stays `None`: factory OTP
/// (one-time programmable memory on the panel) supplies the waveform.
#[derive(Debug, Clone)]
pub struct Config<'lut> {
    /// Value for `Driver Output control` (0x01): gate lines minus one.
    pub gate_lines: u16,
    /// Scan direction bits for `Driver Output control` (0x01) `B[2:0]`.
    /// Use [`GateScan::byte`] so reserved `TB=1` cannot slip in.
    pub scan_bits: u8,
    /// `Data Entry mode setting` (0x11) `A[2:0]`. Power-on default is `0b011`.
    pub data_entry_mode: u8,
    /// RAM window covering the area you intend to write.
    pub window: Window,
    /// Microcontroller-written 105-byte waveform (`0x32`). `None` is correct
    /// for the Sticky factory-OTP path.
    pub lut: Option<&'lut Lut>,
    /// `Border Waveform Control` (0x3C). `None` leaves the power-on default.
    pub border_waveform: Option<u8>,
    /// Select the controller's internal temperature sensor (0x18 = `0x80`).
    pub internal_temperature_sensor: bool,
    /// Booster soft-start (0x0C). Seeed Sticky writes [`BoosterSoftStart::LEVEL_2`].
    pub booster: Option<BoosterSoftStart>,
    /// When `true` (Seeed full refresh), send 0x12 first. Partial and gray4
    /// skip it so RAM survives.
    pub software_reset: bool,
    /// Gate / source / VCOM analog rails. Seeed Sticky leaves this `None`
    /// (factory OTP brings analog up).
    pub analog: Option<AnalogVoltages>,
}

mod sealed {
    pub trait Sealed {}
}

/// Controller power states tracked at compile time.
pub trait State: sealed::Sealed {}

/// The controller is awake and accepting commands.
#[derive(Debug)]
pub struct Active;

/// The controller is in deep sleep. Only a hardware reset revives it.
#[derive(Debug)]
pub struct Asleep;

impl sealed::Sealed for Active {}
impl sealed::Sealed for Asleep {}
impl State for Active {}
impl State for Asleep {}

/// An SSD1677 on a SPI bus.
///
/// `SPI` is an `embedded-hal` [`SpiDevice`], so chip select and bus sharing
/// stay with the caller — necessary on boards that share one controller
/// between the panel and an SD card.
#[derive(Debug)]
pub struct Ssd1677<SPI, DC, RST, BUSY, DELAY, S: State> {
    spi: SPI,
    dc: DC,
    rst: RST,
    busy: BUSY,
    delay: DELAY,
    _state: PhantomData<S>,
}

type DriverResult<T, SPI, DC> = Result<
    T,
    Error<
        <SPI as embedded_hal::spi::ErrorType>::Error,
        <DC as embedded_hal::digital::ErrorType>::Error,
    >,
>;

impl<SPI, DC, RST, BUSY, DELAY> Ssd1677<SPI, DC, RST, BUSY, DELAY, Active>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error>,
    DELAY: DelayNs,
{
    /// Takes ownership of the bus and pins and performs a hardware reset.
    ///
    /// The reset leaves the controller awake but unconfigured; follow with
    /// [`Ssd1677::init`].
    pub fn new(
        spi: SPI,
        dc: DC,
        rst: RST,
        busy: BUSY,
        delay: DELAY,
    ) -> DriverResult<Self, SPI, DC> {
        let mut driver = Self {
            spi,
            dc,
            rst,
            busy,
            delay,
            _state: PhantomData,
        };
        driver.hardware_reset()?;
        Ok(driver)
    }

    /// Pulses RST low then high, per the board contract's 10 ms / 10 ms.
    pub fn hardware_reset(&mut self) -> DriverResult<(), SPI, DC> {
        self.rst.set_low().map_err(Error::Pin)?;
        self.delay.delay_ms(RESET_PULSE_MS);
        self.rst.set_high().map_err(Error::Pin)?;
        self.delay.delay_ms(RESET_PULSE_MS);
        Ok(())
    }

    /// Runs the configuration sequence in [`Config`].
    ///
    /// Order matches Seeed `ssd1677_init_base` / stock `reterminal_template`:
    /// software reset (optional), temperature sensor, border, booster, gate
    /// scan, data entry, analog (optional), window, cursor, MCU LUT (optional).
    pub fn init(&mut self, config: &Config<'_>) -> DriverResult<(), SPI, DC> {
        config.window.validate()?;

        if config.software_reset {
            self.write(Command::SoftwareReset, &[])?;
            self.wait_until_idle(200)?;
        }

        if config.internal_temperature_sensor {
            self.write(
                Command::TemperatureSensorControl,
                &[command::TEMPERATURE_SENSOR_INTERNAL],
            )?;
        }
        if let Some(border) = config.border_waveform {
            self.write(Command::BorderWaveformControl, &[border])?;
        }
        if let Some(booster) = config.booster {
            self.write_booster(booster)?;
        }

        let gates = config.gate_lines.to_le_bytes();
        self.write(
            Command::DriverOutputControl,
            &[gates[0], gates[1], config.scan_bits],
        )?;
        self.write(Command::DataEntryMode, &[config.data_entry_mode])?;

        if let Some(analog) = config.analog {
            self.write_analog(analog)?;
        }

        self.set_window(&config.window)?;
        self.set_cursor(config.window.x_start, config.window.y_start)?;

        if let Some(lut) = config.lut {
            self.write_lut(lut)?;
        }

        Ok(())
    }

    /// Writes booster soft-start (0x0C).
    pub fn write_booster(&mut self, booster: BoosterSoftStart) -> DriverResult<(), SPI, DC> {
        self.write(Command::BoosterSoftStart, &booster.bytes())
    }

    /// Writes gate (0x03), source (0x04), and VCOM (0x2C).
    ///
    /// The Sticky OTP path does not call this.
    pub fn write_analog(&mut self, analog: AnalogVoltages) -> DriverResult<(), SPI, DC> {
        self.write(Command::GateDrivingVoltage, &[analog.gate.byte()])?;
        self.write(Command::SourceDrivingVoltage, &analog.source.bytes())?;
        self.write(Command::WriteVcomRegister, &[analog.vcom.byte()])
    }

    /// Writes the 12-bit temperature register (0x1A).
    pub fn write_temperature_register(&mut self, data: [u8; 2]) -> DriverResult<(), SPI, DC> {
        self.write(Command::WriteTemperatureRegister, &data)
    }

    /// Starts an update from a typed [`UpdateSequence`].
    pub fn start_update_sequence(&mut self, sequence: UpdateSequence) -> DriverResult<(), SPI, DC> {
        self.start_update(sequence)
    }

    /// Writes the RAM window (`8.3` 0x44 / `8.4` 0x45) in address units.
    pub fn set_window(&mut self, window: &Window) -> DriverResult<(), SPI, DC> {
        window.validate()?;

        let x_start = window.x_start.to_le_bytes();
        let x_end = window.x_end.to_le_bytes();
        self.write(
            Command::SetRamXStartEnd,
            &[x_start[0], x_start[1], x_end[0], x_end[1]],
        )?;

        let y_start = window.y_start.to_le_bytes();
        let y_end = window.y_end.to_le_bytes();
        self.write(
            Command::SetRamYStartEnd,
            &[y_start[0], y_start[1], y_end[0], y_end[1]],
        )
    }

    /// Writes the RAM address counters (0x4E / 0x4F).
    pub fn set_cursor(&mut self, x: u16, y: u16) -> DriverResult<(), SPI, DC> {
        if x > command::RAM_X_MAX {
            return Err(Error::AddressOutOfRange {
                value: x,
                max: command::RAM_X_MAX,
            });
        }
        if y > command::RAM_Y_MAX {
            return Err(Error::AddressOutOfRange {
                value: y,
                max: command::RAM_Y_MAX,
            });
        }

        let x = x.to_le_bytes();
        self.write(Command::SetRamXCounter, &[x[0], x[1]])?;
        let y = y.to_le_bytes();
        self.write(Command::SetRamYCounter, &[y[0], y[1]])
    }

    /// Writes the 105-byte waveform (0x32).
    pub fn write_lut(&mut self, lut: &Lut) -> DriverResult<(), SPI, DC> {
        self.write(Command::WriteLutRegister, lut.bytes())
    }

    /// Writes the black/white plane (0x24).
    pub fn write_black_white_plane(&mut self, data: &[u8]) -> DriverResult<(), SPI, DC> {
        self.write(Command::WriteRamBlackWhite, data)
    }

    /// Writes the second plane (0x26).
    ///
    /// The datasheet calls this the RED RAM. On mono film it carries no colour;
    /// it is the high bit of the LUT index. See [`planes`].
    pub fn write_second_plane(&mut self, data: &[u8]) -> DriverResult<(), SPI, DC> {
        self.write(Command::WriteRamRed, data)
    }

    /// Writes both planes of a four-gray frame, resetting the cursor first.
    pub fn write_gray4_frame(
        &mut self,
        window: &Window,
        black_white: &[u8],
        second: &[u8],
    ) -> DriverResult<(), SPI, DC> {
        self.set_cursor(window.x_start, window.y_start)?;
        self.write_black_white_plane(black_white)?;
        self.set_cursor(window.x_start, window.y_start)?;
        self.write_second_plane(second)
    }

    /// Starts an update: `Display Update Control 2` (0x22) then
    /// `Master Activation` (0x20).
    ///
    /// `sequence` selects which stages run. Sticky OTP uses
    /// [`UpdateSequence::DISPLAY_MODE_1_WITH_TEMP`] (full),
    /// [`UpdateSequence::DISPLAY_MODE_2_WITH_TEMP`] (partial), or
    /// [`UpdateSequence::SEEED_GRAY4`].
    ///
    /// BUSY goes high. Wait for it before the next command.
    pub fn start_update(&mut self, sequence: UpdateSequence) -> DriverResult<(), SPI, DC> {
        self.write(Command::DisplayUpdateControl2, &[sequence.byte()])?;
        self.write(Command::MasterActivation, &[])
    }

    /// Starts an update and blocks until BUSY clears.
    pub fn refresh(
        &mut self,
        sequence: UpdateSequence,
        timeout_ms: u32,
    ) -> DriverResult<(), SPI, DC> {
        self.start_update(sequence)?;
        self.wait_until_idle(timeout_ms)
    }

    /// Returns whether BUSY is asserted (high means busy).
    pub fn is_busy(&mut self) -> DriverResult<bool, SPI, DC> {
        self.busy.is_high().map_err(Error::Pin)
    }

    /// Polls BUSY until it clears or `timeout_ms` elapses.
    ///
    /// Refreshes on this panel take seconds, so pick a timeout generously; a
    /// [`Error::BusyTimeout`] means the panel may still be mid-update.
    pub fn wait_until_idle(&mut self, timeout_ms: u32) -> DriverResult<(), SPI, DC> {
        let mut waited = 0;
        while self.is_busy()? {
            if waited >= timeout_ms {
                return Err(Error::BusyTimeout);
            }
            self.delay.delay_ms(BUSY_POLL_MS);
            waited += BUSY_POLL_MS;
        }
        Ok(())
    }

    /// Stock standby: disable analog, then clock (`0x22` = `0x03`, then
    /// `0x20`).
    ///
    /// Table 7-1 [`UpdateSequence::DISABLE_ANALOG_AND_CLOCK`]. Controller
    /// RAM stays and the type stays [`Active`]. This is not
    /// [`Ssd1677::sleep`]. BUSY goes high; wait before the next command.
    pub fn standby(&mut self) -> DriverResult<(), SPI, DC> {
        self.start_update(UpdateSequence::DISABLE_ANALOG_AND_CLOCK)
    }

    /// Stock resume after [`Ssd1677::standby`]: enable clock, then analog
    /// (`0x22` = `0xC0`, then `0x20`).
    ///
    /// Table 7-1 [`UpdateSequence::ENABLE_CLOCK_AND_ANALOG`]. The type
    /// stays [`Active`]. BUSY goes high; wait before the next command.
    pub fn resume(&mut self) -> DriverResult<(), SPI, DC> {
        self.start_update(UpdateSequence::ENABLE_CLOCK_AND_ANALOG)
    }

    /// Enters deep sleep (0x10 with `A[1:0] = 0b11`).
    ///
    /// The returned value has no command methods: reviving the controller
    /// requires [`Ssd1677::wake`]. Issue this **before** dropping the panel
    /// power rail.
    pub fn sleep(mut self) -> SleepResult<SPI, DC, RST, BUSY, DELAY> {
        match self.write(Command::DeepSleepMode, &[command::DEEP_SLEEP_ENTER]) {
            Ok(()) => Ok(Ssd1677 {
                spi: self.spi,
                dc: self.dc,
                rst: self.rst,
                busy: self.busy,
                delay: self.delay,
                _state: PhantomData,
            }),
            Err(source) => Err(SleepError {
                driver: self,
                source,
            }),
        }
    }

    /// Sends a command byte with D/C low, then any parameters with D/C high.
    fn write(&mut self, command: Command, params: &[u8]) -> DriverResult<(), SPI, DC> {
        self.dc.set_low().map_err(Error::Pin)?;
        self.spi.write(&[command.opcode()]).map_err(Error::Spi)?;

        if !params.is_empty() {
            self.dc.set_high().map_err(Error::Pin)?;
            self.spi.write(params).map_err(Error::Spi)?;
        }

        Ok(())
    }
}

impl<SPI, DC, RST, BUSY, DELAY> Ssd1677<SPI, DC, RST, BUSY, DELAY, Asleep>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error>,
    DELAY: DelayNs,
{
    /// Wakes the controller with the hardware reset deep sleep requires.
    ///
    /// Configuration is lost across deep sleep, so follow with
    /// [`Ssd1677::init`].
    pub fn wake(self) -> DriverResult<Ssd1677<SPI, DC, RST, BUSY, DELAY, Active>, SPI, DC> {
        let mut driver = Ssd1677::<SPI, DC, RST, BUSY, DELAY, Active> {
            spi: self.spi,
            dc: self.dc,
            rst: self.rst,
            busy: self.busy,
            delay: self.delay,
            _state: PhantomData,
        };
        driver.hardware_reset()?;
        Ok(driver)
    }
}

impl<SPI, DC, RST, BUSY, DELAY, S: State> Ssd1677<SPI, DC, RST, BUSY, DELAY, S> {
    /// Consumes the driver and returns the bus, pins, and delay (`C-FREE`).
    ///
    /// Call [`Ssd1677::sleep`] first if you are about to drop the panel rail.
    #[inline]
    pub fn release(self) -> (SPI, DC, RST, BUSY, DELAY) {
        (self.spi, self.dc, self.rst, self.busy, self.delay)
    }
}

#[cfg(feature = "async")]
impl<SPI, DC, RST, BUSY, DELAY> Ssd1677<SPI, DC, RST, BUSY, DELAY, Active>
where
    SPI: SpiDevice,
    DC: OutputPin,
    BUSY: embedded_hal_async::digital::Wait<Error = DC::Error>,
{
    /// Waits for BUSY to fall using an interrupt rather than polling.
    ///
    /// Preferable on battery: a full refresh is seconds of otherwise idle CPU.
    pub async fn wait_until_idle_async(&mut self) -> Result<(), Error<SPI::Error, DC::Error>> {
        self.busy.wait_for_low().await.map_err(Error::Pin)
    }
}

#[cfg(test)]
mod tests {
    use embedded_hal_mock::eh1::delay::NoopDelay;
    use embedded_hal_mock::eh1::digital::{
        Mock as PinMock, State as PinState, Transaction as PinTransaction,
    };
    use embedded_hal_mock::eh1::spi::{Mock as SpiMock, Transaction as SpiTransaction};
    use std::vec;
    use std::vec::Vec;

    use super::*;

    const STICKY_WINDOW: Window = Window {
        x_start: 0,
        x_end: 799,
        y_start: 0,
        y_end: 479,
    };

    /// SPI transactions for one command with parameters.
    fn command_txns(opcode: u8, params: &[u8]) -> Vec<SpiTransaction<u8>> {
        let mut txns = vec![
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(vec![opcode]),
            SpiTransaction::transaction_end(),
        ];
        if !params.is_empty() {
            txns.extend([
                SpiTransaction::transaction_start(),
                SpiTransaction::write_vec(params.to_vec()),
                SpiTransaction::transaction_end(),
            ]);
        }
        txns
    }

    fn reset_pins() -> Vec<PinTransaction> {
        vec![
            PinTransaction::set(PinState::Low),
            PinTransaction::set(PinState::High),
        ]
    }

    /// D/C toggles for `count` commands that carry parameters: low for the
    /// opcode, high for the payload.
    fn dc_pairs(count: usize) -> Vec<PinTransaction> {
        (0..count)
            .flat_map(|_| {
                [
                    PinTransaction::set(PinState::Low),
                    PinTransaction::set(PinState::High),
                ]
            })
            .collect()
    }

    #[test]
    fn new_pulses_reset() {
        let spi = SpiMock::new(&[]);
        let dc = PinMock::new(&[]);
        let rst = PinMock::new(&reset_pins());
        let busy = PinMock::new(&[]);

        let driver = Ssd1677::new(spi, dc, rst, busy, NoopDelay::new()).unwrap();
        let (mut spi, mut dc, mut rst, mut busy, _) = driver.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }

    #[test]
    fn deep_sleep_sends_the_documented_parameter() {
        let mut expected = command_txns(
            Command::DeepSleepMode.opcode(),
            &[command::DEEP_SLEEP_ENTER],
        );
        let spi = SpiMock::new(&expected.split_off(0));

        let dc = PinMock::new(&[
            PinTransaction::set(PinState::Low),
            PinTransaction::set(PinState::High),
        ]);
        let rst = PinMock::new(&[]);
        let busy = PinMock::new(&[]);

        let driver = Ssd1677::<_, _, _, _, _, Active> {
            spi,
            dc,
            rst,
            busy,
            delay: NoopDelay::new(),
            _state: PhantomData,
        };

        let asleep = driver.sleep().map_err(|error| error.source).unwrap();
        let (mut spi, mut dc, mut rst, mut busy, _) = asleep.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }

    #[test]
    fn standby_and_resume_send_the_stock_sequences() {
        let mut expected = Vec::new();
        expected.extend(command_txns(
            Command::DisplayUpdateControl2.opcode(),
            &[UpdateSequence::DISABLE_ANALOG_AND_CLOCK.byte()],
        ));
        expected.extend(command_txns(Command::MasterActivation.opcode(), &[]));
        expected.extend(command_txns(
            Command::DisplayUpdateControl2.opcode(),
            &[UpdateSequence::ENABLE_CLOCK_AND_ANALOG.byte()],
        ));
        expected.extend(command_txns(Command::MasterActivation.opcode(), &[]));

        let spi = SpiMock::new(&expected);
        let dc = PinMock::new(&[
            PinTransaction::set(PinState::Low),
            PinTransaction::set(PinState::High),
            PinTransaction::set(PinState::Low),
            PinTransaction::set(PinState::Low),
            PinTransaction::set(PinState::High),
            PinTransaction::set(PinState::Low),
        ]);

        let mut driver = Ssd1677::<_, _, PinMock, PinMock, _, Active> {
            spi,
            dc,
            rst: PinMock::new(&[]),
            busy: PinMock::new(&[]),
            delay: NoopDelay::new(),
            _state: PhantomData,
        };

        driver.standby().unwrap();
        driver.resume().unwrap();

        let (mut spi, mut dc, mut rst, mut busy, _) = driver.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }

    #[test]
    fn window_and_cursor_are_little_endian_ten_bit_values() {
        let mut expected = Vec::new();
        // 0x44 with x 0..=799 (0x31F), then 0x45 with y 0..=479 (0x1DF).
        expected.extend(command_txns(0x44, &[0x00, 0x00, 0x1f, 0x03]));
        expected.extend(command_txns(0x45, &[0x00, 0x00, 0xdf, 0x01]));
        // 0x4E / 0x4F cursor at 12, 34.
        expected.extend(command_txns(0x4e, &[0x0c, 0x00]));
        expected.extend(command_txns(0x4f, &[0x22, 0x00]));

        let spi = SpiMock::new(&expected);
        let dc = PinMock::new(&[
            PinTransaction::set(PinState::Low),
            PinTransaction::set(PinState::High),
            PinTransaction::set(PinState::Low),
            PinTransaction::set(PinState::High),
            PinTransaction::set(PinState::Low),
            PinTransaction::set(PinState::High),
            PinTransaction::set(PinState::Low),
            PinTransaction::set(PinState::High),
        ]);

        let mut driver = Ssd1677::<_, _, PinMock, PinMock, _, Active> {
            spi,
            dc,
            rst: PinMock::new(&[]),
            busy: PinMock::new(&[]),
            delay: NoopDelay::new(),
            _state: PhantomData,
        };

        driver.set_window(&STICKY_WINDOW).unwrap();
        driver.set_cursor(12, 34).unwrap();

        let (mut spi, mut dc, mut rst, mut busy, _) = driver.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }

    #[test]
    fn addresses_beyond_the_datasheet_limits_are_rejected() {
        let mut driver = Ssd1677::<SpiMock<u8>, PinMock, PinMock, PinMock, _, Active> {
            spi: SpiMock::new(&[]),
            dc: PinMock::new(&[]),
            rst: PinMock::new(&[]),
            busy: PinMock::new(&[]),
            delay: NoopDelay::new(),
            _state: PhantomData,
        };

        assert_eq!(
            driver.set_cursor(960, 0),
            Err(Error::AddressOutOfRange {
                value: 960,
                max: 959
            })
        );
        assert_eq!(
            driver.set_cursor(0, 680),
            Err(Error::AddressOutOfRange {
                value: 680,
                max: 679
            })
        );

        let (mut spi, mut dc, mut rst, mut busy, _) = driver.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }

    #[test]
    fn refresh_starts_the_update_then_waits_for_busy_to_fall() {
        let mut expected = Vec::new();
        expected.extend(command_txns(
            Command::DisplayUpdateControl2.opcode(),
            &[UpdateSequence::DISPLAY_MODE_1_WITH_TEMP.byte()],
        ));
        expected.extend(command_txns(Command::MasterActivation.opcode(), &[]));

        let spi = SpiMock::new(&expected);
        let dc = PinMock::new(&[
            PinTransaction::set(PinState::Low),
            PinTransaction::set(PinState::High),
            PinTransaction::set(PinState::Low),
        ]);
        // Busy for two polls, then idle.
        let busy = PinMock::new(&[
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::Low),
        ]);

        let mut driver = Ssd1677::<_, _, PinMock, _, _, Active> {
            spi,
            dc,
            rst: PinMock::new(&[]),
            busy,
            delay: NoopDelay::new(),
            _state: PhantomData,
        };

        driver
            .refresh(UpdateSequence::DISPLAY_MODE_1_WITH_TEMP, 5_000)
            .unwrap();

        let (mut spi, mut dc, mut rst, mut busy, _) = driver.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }

    #[test]
    fn waking_pulses_reset_because_deep_sleep_needs_one() {
        // The recovery half of the sleep hazard: deep sleep is only escapable
        // through a hardware reset, so wake() must drive RST.
        let rst = PinMock::new(&reset_pins());

        let asleep = Ssd1677::<SpiMock<u8>, PinMock, _, PinMock, _, Asleep> {
            spi: SpiMock::new(&[]),
            dc: PinMock::new(&[]),
            rst,
            busy: PinMock::new(&[]),
            delay: NoopDelay::new(),
            _state: PhantomData,
        };

        let awake = asleep.wake().unwrap();

        let (mut spi, mut dc, mut rst, mut busy, _) = awake.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }

    #[cfg(feature = "async")]
    #[test]
    fn the_async_wait_blocks_on_a_busy_edge_rather_than_polling() {
        // Executed, not just type-checked: this is the path battery-powered
        // callers are meant to prefer over the polling loop.
        let busy = PinMock::new(&[PinTransaction::wait_for_state(PinState::Low)]);

        let mut driver = Ssd1677::<SpiMock<u8>, PinMock, PinMock, _, _, Active> {
            spi: SpiMock::new(&[]),
            dc: PinMock::new(&[]),
            rst: PinMock::new(&[]),
            busy,
            delay: NoopDelay::new(),
            _state: PhantomData,
        };

        embassy_futures::block_on(driver.wait_until_idle_async()).unwrap();

        let (mut spi, mut dc, mut rst, mut busy, _) = driver.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }

    #[test]
    fn a_stuck_busy_pin_times_out_instead_of_hanging() {
        let busy = PinMock::new(&[
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::High),
            PinTransaction::get(PinState::High),
        ]);

        let mut driver = Ssd1677::<SpiMock<u8>, PinMock, PinMock, _, _, Active> {
            spi: SpiMock::new(&[]),
            dc: PinMock::new(&[]),
            rst: PinMock::new(&[]),
            busy,
            delay: NoopDelay::new(),
            _state: PhantomData,
        };

        assert_eq!(driver.wait_until_idle(20), Err(Error::BusyTimeout));

        let (mut spi, mut dc, mut rst, mut busy, _) = driver.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }

    #[test]
    fn gray4_frame_writes_both_planes_from_the_window_origin() {
        let bw = [0xaa, 0xbb];
        let second = [0xcc, 0xdd];

        let mut expected = Vec::new();
        expected.extend(command_txns(0x4e, &[0x00, 0x00]));
        expected.extend(command_txns(0x4f, &[0x00, 0x00]));
        expected.extend(command_txns(0x24, &bw));
        expected.extend(command_txns(0x4e, &[0x00, 0x00]));
        expected.extend(command_txns(0x4f, &[0x00, 0x00]));
        expected.extend(command_txns(0x26, &second));

        let spi = SpiMock::new(&expected);
        let dc = PinMock::new(&dc_pairs(6));

        let mut driver = Ssd1677::<_, _, PinMock, PinMock, _, Active> {
            spi,
            dc,
            rst: PinMock::new(&[]),
            busy: PinMock::new(&[]),
            delay: NoopDelay::new(),
            _state: PhantomData,
        };

        driver
            .write_gray4_frame(&STICKY_WINDOW, &bw, &second)
            .unwrap();

        let (mut spi, mut dc, mut rst, mut busy, _) = driver.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }

    #[test]
    fn init_writes_the_lut_last_so_the_software_reset_cannot_clear_it() {
        let lut = Lut::new([0x5a; LUT_LEN], "test vector");
        let config = Config {
            gate_lines: 479,
            scan_bits: 0b000,
            data_entry_mode: 0b011,
            window: STICKY_WINDOW,
            lut: Some(&lut),
            border_waveform: Some(0x05),
            internal_temperature_sensor: true,
            booster: None,
            software_reset: true,
            analog: None,
        };

        let mut expected = Vec::new();
        expected.extend(command_txns(0x12, &[]));
        expected.extend(command_txns(0x18, &[0x80]));
        expected.extend(command_txns(0x3c, &[0x05]));
        expected.extend(command_txns(0x01, &[0xdf, 0x01, 0x00]));
        expected.extend(command_txns(0x11, &[0b011]));
        expected.extend(command_txns(0x44, &[0x00, 0x00, 0x1f, 0x03]));
        expected.extend(command_txns(0x45, &[0x00, 0x00, 0xdf, 0x01]));
        expected.extend(command_txns(0x4e, &[0x00, 0x00]));
        expected.extend(command_txns(0x4f, &[0x00, 0x00]));
        expected.extend(command_txns(0x32, &[0x5a; LUT_LEN]));

        let spi = SpiMock::new(&expected);
        // One command with no params (0x12) plus nine with params.
        let mut dc_txns = vec![PinTransaction::set(PinState::Low)];
        dc_txns.extend(dc_pairs(9));

        let mut driver = Ssd1677::<_, _, PinMock, _, _, Active> {
            spi,
            dc: PinMock::new(&dc_txns),
            rst: PinMock::new(&[]),
            busy: PinMock::new(&[PinTransaction::get(PinState::Low)]),
            delay: NoopDelay::new(),
            _state: PhantomData,
        };

        driver.init(&config).unwrap();

        let (mut spi, mut dc, mut rst, mut busy, _) = driver.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }

    #[test]
    fn seeed_full_init_writes_level2_booster_before_gate_scan() {
        let config = Config {
            gate_lines: 479,
            scan_bits: GateScan::SEEED_STICKY.byte().unwrap(),
            data_entry_mode: crate::sequence::DATA_ENTRY_Y_INC_X_INC,
            window: STICKY_WINDOW,
            lut: None,
            border_waveform: Some(crate::sequence::border::FOLLOW_LUT1),
            internal_temperature_sensor: true,
            booster: Some(BoosterSoftStart::LEVEL_2),
            software_reset: true,
            analog: None,
        };

        let mut expected = Vec::new();
        expected.extend(command_txns(Command::SoftwareReset.opcode(), &[]));
        expected.extend(command_txns(
            Command::TemperatureSensorControl.opcode(),
            &[command::TEMPERATURE_SENSOR_INTERNAL],
        ));
        expected.extend(command_txns(
            Command::BorderWaveformControl.opcode(),
            &[crate::sequence::border::FOLLOW_LUT1],
        ));
        expected.extend(command_txns(
            Command::BoosterSoftStart.opcode(),
            &BoosterSoftStart::LEVEL_2.bytes(),
        ));
        expected.extend(command_txns(
            Command::DriverOutputControl.opcode(),
            &[
                config.gate_lines.to_le_bytes()[0],
                config.gate_lines.to_le_bytes()[1],
                config.scan_bits,
            ],
        ));
        expected.extend(command_txns(
            Command::DataEntryMode.opcode(),
            &[config.data_entry_mode],
        ));
        expected.extend(command_txns(
            Command::SetRamXStartEnd.opcode(),
            &[
                config.window.x_start.to_le_bytes()[0],
                config.window.x_start.to_le_bytes()[1],
                config.window.x_end.to_le_bytes()[0],
                config.window.x_end.to_le_bytes()[1],
            ],
        ));
        expected.extend(command_txns(
            Command::SetRamYStartEnd.opcode(),
            &[
                config.window.y_start.to_le_bytes()[0],
                config.window.y_start.to_le_bytes()[1],
                config.window.y_end.to_le_bytes()[0],
                config.window.y_end.to_le_bytes()[1],
            ],
        ));
        expected.extend(command_txns(
            Command::SetRamXCounter.opcode(),
            &config.window.x_start.to_le_bytes(),
        ));
        expected.extend(command_txns(
            Command::SetRamYCounter.opcode(),
            &config.window.y_start.to_le_bytes(),
        ));

        let spi = SpiMock::new(&expected);
        let mut dc_txns = vec![PinTransaction::set(PinState::Low)];
        dc_txns.extend(dc_pairs(9));

        let mut driver = Ssd1677::<_, _, PinMock, _, _, Active> {
            spi,
            dc: PinMock::new(&dc_txns),
            rst: PinMock::new(&[]),
            busy: PinMock::new(&[PinTransaction::get(PinState::Low)]),
            delay: NoopDelay::new(),
            _state: PhantomData,
        };

        driver.init(&config).unwrap();

        let (mut spi, mut dc, mut rst, mut busy, _) = driver.release();
        spi.done();
        dc.done();
        rst.done();
        busy.done();
    }
}
