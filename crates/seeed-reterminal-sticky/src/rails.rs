//! Peripheral power rails, with settle times and cut-order rules in the types.
//!
//! Each rail is an active-high load switch that needs a [`Latched`] witness to
//! construct, starts **disabled**, and knows its own settle time. Getting a
//! settle wrong shows up as an intermittently missing peripheral, which is a
//! miserable thing to debug on e-paper.
//!
//! # The panel rail is not like the others
//!
//! Cutting `EPD_EN` while the controller is awake is on the hazard list, so
//! [`EpdRail`] has no `disable`. It has
//! [`Rail::disable_after_panel_sleep`][disable_after_panel_sleep], which
//! requires a [`PanelParked`] token whose only constructor is named after the
//! obligation it represents. Rails that are safe to cut at any time implement
//! [`CutAnytime`] and get a plain `disable`.
//!
//! [disable_after_panel_sleep]: Rail::disable_after_panel_sleep
//!
//! That is a compiler guarantee, not a convention:
//!
//! ```compile_fail
//! use embedded_hal_mock::eh1::delay::NoopDelay;
//! use embedded_hal_mock::eh1::digital::{Mock, State, Transaction};
//! use seeed_reterminal_sticky::rails::{EpdRail, Rail};
//! use seeed_reterminal_sticky::Latch;
//!
//! let hold = Mock::new(&[Transaction::set(State::High)]);
//! let lock = Mock::new(&[Transaction::set(State::High)]);
//! let latch = Latch::acquire(hold, lock, &mut NoopDelay::new()).unwrap();
//!
//! let pin = Mock::new(&[
//!     Transaction::set(State::Low),
//!     Transaction::set(State::High),
//! ]);
//! let rail: EpdRail<_, _> = Rail::new(pin, latch.witness()).unwrap();
//! let rail = rail.enable(&mut NoopDelay::new()).unwrap();
//!
//! // The panel rail has no unconditional `disable`: dropping panel power
//! // mid-waveform is a hazard, so this does not compile.
//! let _ = rail.disable();
//! ```

use core::marker::PhantomData;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;

use crate::power::Latched;

mod sealed {
    pub trait Sealed {}
}

/// Rail enable states tracked at compile time.
pub trait RailState: sealed::Sealed {}

/// The rail is off.
#[derive(Debug)]
pub struct Disabled;

/// The rail is on and has settled.
#[derive(Debug)]
pub struct Enabled;

impl sealed::Sealed for Disabled {}
impl sealed::Sealed for Enabled {}
impl RailState for Disabled {}
impl RailState for Enabled {}

/// Identifies a rail and carries its documented settle time.
pub trait RailKind: sealed::Sealed {
    /// Milliseconds to wait after enabling, before using the peripheral.
    const SETTLE_MS: u32;
    /// Human-readable name, for logs.
    const NAME: &'static str;
}

/// Rails that can be cut at any time without a device-side preamble.
pub trait CutAnytime: RailKind {}

/// E-paper panel rail, GPIO47. ~100 ms settle. Cutting it needs the controller
/// asleep first.
#[derive(Debug)]
pub struct Epd;

/// Touch rail, GPIO42. ~250 ms settle, then the GT911 reset sequence.
#[derive(Debug)]
pub struct Touch;

/// MicroSD rail, GPIO10. Settle time unmeasured on this board.
#[derive(Debug)]
pub struct Sd;

/// Microphone rail, GPIO38. Settle time unmeasured on this board.
#[derive(Debug)]
pub struct Mic;

impl sealed::Sealed for Epd {}
impl sealed::Sealed for Touch {}
impl sealed::Sealed for Sd {}
impl sealed::Sealed for Mic {}

impl RailKind for Epd {
    const SETTLE_MS: u32 = 100;
    const NAME: &'static str = "EPD";
}

impl RailKind for Touch {
    const SETTLE_MS: u32 = 250;
    const NAME: &'static str = "touch";
}

impl RailKind for Sd {
    // Unmeasured: no vendor figure and no measurement on a physical unit. Ten
    // milliseconds is a guess for a load switch, and it is labelled as one.
    const SETTLE_MS: u32 = 10;
    const NAME: &'static str = "microSD";
}

impl RailKind for Mic {
    // Unmeasured, as above.
    const SETTLE_MS: u32 = 10;
    const NAME: &'static str = "microphone";
}

impl CutAnytime for Touch {}
impl CutAnytime for Sd {}
impl CutAnytime for Mic {}

/// Proof that the panel controller has been put into deep sleep.
///
/// The only constructor is [`PanelParked::after_deep_sleep_command`], so the
/// requirement is stated at the call site rather than in a comment somewhere
/// else. This crate cannot verify the SPI traffic — that would mean depending
/// on a display driver — but it can make you say out loud that you did it.
#[derive(Debug)]
pub struct PanelParked {
    _private: (),
}

impl PanelParked {
    /// Assert that the panel controller's deep-sleep command has completed.
    ///
    /// Call this immediately after the display driver's sleep call succeeded,
    /// and nowhere else.
    #[inline]
    #[must_use]
    pub const fn after_deep_sleep_command() -> Self {
        Self { _private: () }
    }
}

/// An active-high power rail.
#[derive(Debug)]
pub struct Rail<P, K: RailKind, S: RailState> {
    pin: P,
    _kind: PhantomData<K>,
    _state: PhantomData<S>,
}

/// The e-paper panel rail.
pub type EpdRail<P, S> = Rail<P, Epd, S>;
/// The touch rail.
pub type TouchRail<P, S> = Rail<P, Touch, S>;
/// The MicroSD rail.
pub type SdRail<P, S> = Rail<P, Sd, S>;
/// The microphone rail.
pub type MicRail<P, S> = Rail<P, Mic, S>;

impl<P: OutputPin, K: RailKind> Rail<P, K, Disabled> {
    /// Takes the enable pin and drives it low.
    ///
    /// Requires a [`Latched`] witness: a rail brought up before the power latch
    /// can drop mid-sequence when USB is removed.
    pub fn new(mut pin: P, _latched: &Latched) -> Result<Self, P::Error> {
        pin.set_low()?;
        Ok(Self {
            pin,
            _kind: PhantomData,
            _state: PhantomData,
        })
    }

    /// Drives the rail high and waits [`RailKind::SETTLE_MS`].
    pub fn enable<D: DelayNs>(mut self, delay: &mut D) -> Result<Rail<P, K, Enabled>, P::Error> {
        self.pin.set_high()?;
        delay.delay_ms(K::SETTLE_MS);
        Ok(Rail {
            pin: self.pin,
            _kind: PhantomData,
            _state: PhantomData,
        })
    }
}

impl<P: OutputPin, K: CutAnytime> Rail<P, K, Enabled> {
    /// Drives the rail low.
    pub fn disable(mut self) -> Result<Rail<P, K, Disabled>, P::Error> {
        self.pin.set_low()?;
        Ok(Rail {
            pin: self.pin,
            _kind: PhantomData,
            _state: PhantomData,
        })
    }
}

impl<P: OutputPin> Rail<P, Epd, Enabled> {
    /// Drives the panel rail low, once the controller is asleep.
    ///
    /// There is no unconditional `disable` for this rail on purpose: dropping
    /// panel power mid-waveform is on the hazard list.
    pub fn disable_after_panel_sleep(
        mut self,
        _parked: PanelParked,
    ) -> Result<Rail<P, Epd, Disabled>, P::Error> {
        self.pin.set_low()?;
        Ok(Rail {
            pin: self.pin,
            _kind: PhantomData,
            _state: PhantomData,
        })
    }
}

impl<P, K: RailKind, S: RailState> Rail<P, K, S> {
    /// This rail's name, for logs.
    #[inline]
    #[must_use]
    pub const fn name(&self) -> &'static str {
        K::NAME
    }

    /// Consumes the rail and returns the pin (`C-FREE`), leaving it at
    /// whatever level the type state says.
    #[inline]
    pub fn release(self) -> P {
        self.pin
    }
}

#[cfg(test)]
mod tests {
    use embedded_hal_mock::eh1::delay::{CheckedDelay, Transaction as DelayTransaction};
    use embedded_hal_mock::eh1::digital::{Mock, State, Transaction};

    use super::*;
    use crate::power::Latch;

    /// A latch built from throwaway mocks, for tests that only need a witness.
    fn latched() -> Latch<Mock, Mock> {
        let hold = Mock::new(&[Transaction::set(State::High)]);
        let lock = Mock::new(&[Transaction::set(State::High)]);
        let mut delay = CheckedDelay::new(&[DelayTransaction::delay_ms(10)]);
        let latch = Latch::acquire(hold, lock, &mut delay).unwrap();
        delay.done();
        latch
    }

    fn finish(latch: Latch<Mock, Mock>) {
        let (mut hold, mut lock) = latch.release_ownership_only();
        hold.done();
        lock.done();
    }

    #[test]
    fn a_rail_starts_low_and_settles_for_its_documented_time() {
        let latch = latched();
        let pin = Mock::new(&[Transaction::set(State::Low), Transaction::set(State::High)]);
        let mut delay = CheckedDelay::new(&[DelayTransaction::delay_ms(250)]);

        let rail: TouchRail<_, _> = Rail::new(pin, latch.witness()).unwrap();
        assert_eq!(rail.name(), "touch");
        let rail = rail.enable(&mut delay).unwrap();

        delay.done();
        rail.release().done();
        finish(latch);
    }

    #[test]
    fn the_panel_rail_settles_for_one_hundred_milliseconds() {
        let latch = latched();
        let pin = Mock::new(&[
            Transaction::set(State::Low),
            Transaction::set(State::High),
            Transaction::set(State::Low),
        ]);
        let mut delay = CheckedDelay::new(&[DelayTransaction::delay_ms(100)]);

        let rail: EpdRail<_, _> = Rail::new(pin, latch.witness()).unwrap();
        let rail = rail.enable(&mut delay).unwrap();

        // Cutting the rail requires stating that the controller is parked.
        let rail = rail
            .disable_after_panel_sleep(PanelParked::after_deep_sleep_command())
            .unwrap();

        delay.done();
        rail.release().done();
        finish(latch);
    }

    #[test]
    fn rails_that_are_safe_to_cut_disable_directly() {
        let latch = latched();
        let pin = Mock::new(&[
            Transaction::set(State::Low),
            Transaction::set(State::High),
            Transaction::set(State::Low),
        ]);
        let mut delay = CheckedDelay::new(&[DelayTransaction::delay_ms(10)]);

        let rail: SdRail<_, _> = Rail::new(pin, latch.witness()).unwrap();
        let rail = rail.enable(&mut delay).unwrap().disable().unwrap();

        delay.done();
        rail.release().done();
        finish(latch);
    }
}
