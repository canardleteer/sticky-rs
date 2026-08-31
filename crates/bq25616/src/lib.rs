//! GPIO control for the TI BQ25616 standalone battery charger.
//!
//! The BQ25616 has no I2C interface: charge behaviour is set by hardware and
//! the host only gets a chip-enable pin plus optional status inputs. That
//! makes the whole risk surface a single polarity mistake, so this crate's job
//! is to make that mistake impossible to express.
//!
//! # Polarity
//!
//! `/CE` is **active low**. The TI BQ25616 datasheet SLUSDF7 section
//! `Table 7-1. Pin Functions` says the CE pin enables battery charging when
//! driven LOW. Section `9.3.5 Standalone Charger` says charging is enabled or
//! disabled via the CE pin. No caller ever writes a raw level — [`Charger`]
//! tracks state in the type system and [`Charger::new`] leaves the part
//! disabled.
//!
//! ```
//! use bq25616::Charger;
//! # use embedded_hal_mock::eh1::digital::{Mock, State, Transaction};
//! # let ce = Mock::new(&[
//! #     Transaction::set(State::High),
//! #     Transaction::set(State::Low),
//! #     Transaction::set(State::High),
//! # ]);
//! let charger = Charger::new(ce).unwrap(); // starts disabled
//! let charger = charger.enable_charging().unwrap();
//! # let charger = charger.disable_charging().unwrap();
//! # charger.release().done();
//! ```
//!
//! [`Charger<CE, Enabled>`] parks `/CE` high on [`Drop`] so a panic during a
//! short enable window does not leave charging on. [`Charger::release`] of an
//! enabled charger is still `C-FREE` (the pin stays low). Prefer
//! [`Charger::enable_charging_if_external_power`] over a raw enable when the
//! board has a VBUS sense pin.
//!
//! # Status inputs
//!
//! [`ExternalPower`] wraps the external-power sense input, where high means
//! external power is present. [`StatPinState`] records what SLUSDF7 section
//! `9.3.8.2 Charging Status Indicator (STAT)` / `Table 9-6. STAT Pin State`
//! says about the **chip** STAT pin. [`ChargeStatus`] still reports a raw
//! [`Level`]. On the reTerminal Sticky that net is BQ25616 STAT (schematic
//! Rev 01): low while charging when `/CE` is enabled. This driver still
//! does not guess `is_charging()` for you.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(test)]
extern crate std;

use core::marker::PhantomData;

use embedded_hal::digital::{InputPin, OutputPin};

mod sealed {
    pub trait Sealed {}
}

/// Charge-enable states tracked at compile time.
pub trait ChargeState: sealed::Sealed {
    /// Whether this state means charging is enabled.
    const CHARGING: bool;
}

/// Charging is disabled: `/CE` is high. The state [`Charger::new`] returns.
///
/// SLUSDF7 `Table 7-1. Pin Functions`: CE driven LOW enables charging.
#[derive(Debug)]
pub struct Disabled;

/// Charging is enabled: `/CE` is low.
///
/// SLUSDF7 `Table 7-1. Pin Functions`: CE driven LOW enables charging.
#[derive(Debug)]
pub struct Enabled;

impl sealed::Sealed for Disabled {}
impl sealed::Sealed for Enabled {}

impl ChargeState for Disabled {
    const CHARGING: bool = false;
}

impl ChargeState for Enabled {
    const CHARGING: bool = true;
}

/// A failed state transition, returning the charger so the caller can retry
/// or shut down cleanly instead of losing the pin.
#[derive(Debug)]
pub struct TransitionError<CE: embedded_hal::digital::ErrorType + OutputPin, S: ChargeState> {
    /// The charger, still in its original state.
    pub charger: Charger<CE, S>,
    /// The underlying GPIO error.
    pub source: CE::Error,
}

/// Why [`Charger::enable_charging_if_external_power`] refused.
#[derive(Debug)]
pub enum EnableIfError<CE: embedded_hal::digital::ErrorType + OutputPin, E> {
    /// External power was not present. Charging stayed disabled.
    NoExternalPower {
        /// The charger, still disabled.
        charger: Charger<CE, Disabled>,
    },
    /// Reading the external-power sense pin failed.
    Sense {
        /// The charger, still disabled.
        charger: Charger<CE, Disabled>,
        /// The underlying GPIO error.
        source: E,
    },
    /// External power was present, but driving `/CE` low failed.
    Transition(TransitionError<CE, Disabled>),
}

impl<CE: embedded_hal::digital::ErrorType + OutputPin, E> EnableIfError<CE, E> {
    /// Returns the still-disabled charger from any refuse path.
    #[inline]
    #[must_use]
    pub fn into_charger(self) -> Charger<CE, Disabled> {
        match self {
            Self::NoExternalPower { charger } | Self::Sense { charger, .. } => charger,
            Self::Transition(err) => err.charger,
        }
    }
}

/// The charge-enable pin of a BQ25616, with charge state in the type.
///
/// `ce` is `None` only after [`Charger::release`] or a successful
/// typestate move, so [`Drop`] does not drive the pin twice.
#[derive(Debug)]
pub struct Charger<CE: OutputPin, S: ChargeState> {
    ce: Option<CE>,
    _state: PhantomData<S>,
}

impl<CE: OutputPin> Charger<CE, Disabled> {
    /// Takes ownership of the `/CE` pin and drives it high, so charging starts
    /// disabled regardless of how the pin was left (SLUSDF7 `9.3.5 Standalone
    /// Charger`: charging is enabled or disabled via CE).
    ///
    /// Starting disabled is the safe default: it is recoverable, whereas
    /// enabling a charger against an unknown pack state is not.
    pub fn new(mut ce: CE) -> Result<Self, CE::Error> {
        ce.set_high()?;
        Ok(Self {
            ce: Some(ce),
            _state: PhantomData,
        })
    }

    /// Drives `/CE` low, enabling charging (SLUSDF7 `Table 7-1. Pin Functions`).
    ///
    /// Prefer [`Charger::enable_charging_if_external_power`] when a VBUS
    /// sense pin is available.
    pub fn enable_charging(
        mut self,
    ) -> Result<Charger<CE, Enabled>, TransitionError<CE, Disabled>> {
        match self.ce_mut().set_low() {
            Ok(()) => Ok(Charger {
                ce: self.ce.take(),
                _state: PhantomData,
            }),
            Err(source) => Err(TransitionError {
                charger: self,
                source,
            }),
        }
    }

    /// Enables charging only when [`ExternalPower::is_present`] is `true`.
    ///
    /// On the reTerminal Sticky that sense is GPIO9 (`PWR_IN_VOLT`).
    /// A missing VBUS leaves `/CE` high.
    pub fn enable_charging_if_external_power<P: InputPin>(
        self,
        vbus: &mut ExternalPower<P>,
    ) -> Result<Charger<CE, Enabled>, EnableIfError<CE, P::Error>> {
        match vbus.is_present() {
            Ok(true) => self.enable_charging().map_err(EnableIfError::Transition),
            Ok(false) => Err(EnableIfError::NoExternalPower { charger: self }),
            Err(source) => Err(EnableIfError::Sense {
                charger: self,
                source,
            }),
        }
    }

    /// Drives `/CE` high again. Use after a disable if STAT has not risen.
    pub fn hold_disabled(&mut self) -> Result<(), CE::Error> {
        self.ce_mut().set_high()
    }
}

impl<CE: OutputPin> Charger<CE, Enabled> {
    /// Drives `/CE` high, disabling charging (SLUSDF7 `Table 7-1. Pin Functions`).
    pub fn disable_charging(
        mut self,
    ) -> Result<Charger<CE, Disabled>, TransitionError<CE, Enabled>> {
        match self.ce_mut().set_high() {
            Ok(()) => Ok(Charger {
                ce: self.ce.take(),
                _state: PhantomData,
            }),
            Err(source) => Err(TransitionError {
                charger: self,
                source,
            }),
        }
    }
}

impl<CE: OutputPin, S: ChargeState> Drop for Charger<CE, S> {
    /// Parks `/CE` high when this value is [`Enabled`]. Errors are swallowed
    /// because [`Drop`] cannot fail. [`Disabled`] and a consumed pin are
    /// no-ops.
    fn drop(&mut self) {
        if S::CHARGING {
            if let Some(ce) = self.ce.as_mut() {
                let _ = ce.set_high();
            }
        }
    }
}

impl<CE: OutputPin, S: ChargeState> Charger<CE, S> {
    /// Returns whether this type state means charging is enabled. Const-known;
    /// present for logging rather than control flow.
    #[inline]
    #[must_use]
    pub fn is_charging_enabled(&self) -> bool {
        S::CHARGING
    }

    fn ce_mut(&mut self) -> &mut CE {
        self.ce
            .as_mut()
            .expect("charger pin is present until release or a typestate move")
    }

    /// Consumes the wrapper and returns the pin, leaving the charger in
    /// whatever state the type says it is in (`C-FREE`).
    ///
    /// An enabled release does **not** park `/CE`. Use
    /// [`Charger::disable_charging`] first if you want the part parked.
    /// [`Drop`] on [`Enabled`] will not fire a second drive after this.
    #[inline]
    pub fn release(mut self) -> CE {
        self.ce
            .take()
            .expect("charger pin is present until release or a typestate move")
    }
}

/// External-power sense input. High means external power is present.
///
/// On the reTerminal Sticky this is GPIO9 (`PWR_IN_VOLT`): a 5.1 kΩ /
/// 5.1 kΩ divider from `VIN_5V`. ~2.5 V at 5 V VBUS still reads as a
/// GPIO high. This wrapper stays digital.
#[derive(Debug)]
pub struct ExternalPower<P> {
    pin: P,
}

impl<P: InputPin> ExternalPower<P> {
    /// Wraps the sense pin.
    #[inline]
    pub const fn new(pin: P) -> Self {
        Self { pin }
    }

    /// Returns `true` when external power is present.
    #[inline]
    pub fn is_present(&mut self) -> Result<bool, P::Error> {
        self.pin.is_high()
    }

    /// Consumes the wrapper and returns the pin (`C-FREE`).
    #[inline]
    pub fn release(self) -> P {
        self.pin
    }
}

/// A pin level, reported without interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Pin reads low.
    Low,
    /// Pin reads high.
    High,
}

/// What SLUSDF7 says the **chip** STAT pin means.
///
/// The TI BQ25616 datasheet SLUSDF7 section `9.3.8.2 Charging Status Indicator
/// (STAT)` / `Table 9-6. STAT Pin State` lists these encodings. This is
/// silicon documentation, not a reading of the reTerminal Sticky GPIO40 net
/// ([`ChargeStatus`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatPinState {
    /// Table 9-6: charging in progress (including recharge) — STAT **LOW**.
    ChargingInProgress,
    /// Table 9-6: charging termination, sleep, charge disable, or boost —
    /// STAT **HIGH**.
    HighIdle,
    /// Table 9-6: charge suspend (OVP, TS, safety timer, or system OVP) —
    /// STAT blinking at 1 Hz.
    ChargeSuspendBlink,
}

/// Charge-status input on a board net.
///
/// This intentionally exposes a [`Level`] and not `is_charging()`. SLUSDF7
/// `Table 9-6. STAT Pin State` describes the chip STAT pin ([`StatPinState`]).
/// On the reTerminal Sticky the net is STAT (schematic Rev 01): low while
/// charging when `/CE` is enabled, high after park. A read immediately
/// after disable can still be low; wait, then [`Charger::hold_disabled`]
/// if it has not risen. This driver still does not guess
/// `is_charging()`. Gauge `Current()` on that sit lagged (~2 s) and
/// did not print the schematic ~555 mA set.
#[derive(Debug)]
pub struct ChargeStatus<P> {
    pin: P,
}

impl<P: InputPin> ChargeStatus<P> {
    /// Wraps the status pin.
    #[inline]
    pub const fn new(pin: P) -> Self {
        Self { pin }
    }

    /// Reads the raw level.
    #[inline]
    pub fn level(&mut self) -> Result<Level, P::Error> {
        if self.pin.is_high()? {
            Ok(Level::High)
        } else {
            Ok(Level::Low)
        }
    }

    /// Consumes the wrapper and returns the pin (`C-FREE`).
    #[inline]
    pub fn release(self) -> P {
        self.pin
    }
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use embedded_hal_mock::eh1::digital::{Mock, State, Transaction};
    use embedded_hal_mock::eh1::MockError;

    use super::*;

    #[test]
    fn new_parks_the_charger_disabled() {
        let ce = Mock::new(&[Transaction::set(State::High)]);
        let charger = Charger::new(ce).unwrap();
        assert!(!charger.is_charging_enabled());
        charger.release().done();
    }

    #[test]
    fn enable_drives_ce_low_because_it_is_active_low() {
        let ce = Mock::new(&[Transaction::set(State::High), Transaction::set(State::Low)]);
        let charger = Charger::new(ce).unwrap().enable_charging().unwrap();
        assert!(charger.is_charging_enabled());
        charger.release().done();
    }

    #[test]
    fn disable_drives_ce_high() {
        let ce = Mock::new(&[
            Transaction::set(State::High),
            Transaction::set(State::Low),
            Transaction::set(State::High),
        ]);
        let charger = Charger::new(ce)
            .unwrap()
            .enable_charging()
            .unwrap()
            .disable_charging()
            .unwrap();
        assert!(!charger.is_charging_enabled());
        charger.release().done();
    }

    #[test]
    fn external_power_high_means_present() {
        let pin = Mock::new(&[Transaction::get(State::High), Transaction::get(State::Low)]);
        let mut sense = ExternalPower::new(pin);
        assert!(sense.is_present().unwrap());
        assert!(!sense.is_present().unwrap());
        sense.release().done();
    }

    #[test]
    fn charge_status_reports_levels_without_interpreting_them() {
        let pin = Mock::new(&[Transaction::get(State::Low), Transaction::get(State::High)]);
        let mut status = ChargeStatus::new(pin);
        assert_eq!(status.level().unwrap(), Level::Low);
        assert_eq!(status.level().unwrap(), Level::High);
        status.release().done();
    }

    #[test]
    fn transition_error_hands_the_charger_back() {
        let ce = Mock::new(&[
            Transaction::set(State::High),
            Transaction::set(State::Low).with_error(MockError::Io(ErrorKind::Other)),
        ]);
        let charger = Charger::new(ce).unwrap();

        let err = charger
            .enable_charging()
            .expect_err("the mock was scripted to fail the enable");

        // The pin came back with the charger, still in the disabled state, so
        // a caller can park the board instead of leaking the pin.
        assert!(!err.charger.is_charging_enabled());
        err.charger.release().done();
    }

    #[test]
    fn drop_of_enabled_parks_ce() {
        use core::convert::Infallible;
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::vec::Vec;

        #[derive(Debug)]
        struct Rec {
            levels: Rc<RefCell<Vec<bool>>>,
        }

        impl embedded_hal::digital::ErrorType for Rec {
            type Error = Infallible;
        }

        impl OutputPin for Rec {
            fn set_high(&mut self) -> Result<(), Infallible> {
                self.levels.borrow_mut().push(true);
                Ok(())
            }

            fn set_low(&mut self) -> Result<(), Infallible> {
                self.levels.borrow_mut().push(false);
                Ok(())
            }
        }

        let levels = Rc::new(RefCell::new(Vec::new()));
        let charger = Charger::new(Rec {
            levels: levels.clone(),
        })
        .unwrap()
        .enable_charging()
        .unwrap();
        drop(charger);
        assert_eq!(*levels.borrow(), [true, false, true]);
    }

    #[test]
    fn release_of_enabled_does_not_park() {
        let ce = Mock::new(&[Transaction::set(State::High), Transaction::set(State::Low)]);
        let mut pin = Charger::new(ce)
            .unwrap()
            .enable_charging()
            .unwrap()
            .release();
        pin.done();
    }

    #[test]
    fn enable_if_external_power_refuses_when_vbus_is_absent() {
        let ce = Mock::new(&[Transaction::set(State::High)]);
        let vbus = Mock::new(&[Transaction::get(State::Low)]);
        let mut vbus = ExternalPower::new(vbus);
        let err = Charger::new(ce)
            .unwrap()
            .enable_charging_if_external_power(&mut vbus)
            .expect_err("VBUS was scripted low");
        match err {
            EnableIfError::NoExternalPower { charger } => {
                assert!(!charger.is_charging_enabled());
                charger.release().done();
            }
            other => panic!("expected NoExternalPower, got {other:?}"),
        }
        vbus.release().done();
    }

    #[test]
    fn enable_if_external_power_enables_when_vbus_is_present() {
        let ce = Mock::new(&[
            Transaction::set(State::High),
            Transaction::set(State::Low),
            Transaction::set(State::High),
        ]);
        let vbus = Mock::new(&[Transaction::get(State::High)]);
        let mut vbus = ExternalPower::new(vbus);
        let charger = Charger::new(ce)
            .unwrap()
            .enable_charging_if_external_power(&mut vbus)
            .unwrap();
        assert!(charger.is_charging_enabled());
        charger.disable_charging().unwrap().release().done();
        vbus.release().done();
    }

    #[test]
    fn hold_disabled_drives_ce_high_again() {
        let ce = Mock::new(&[Transaction::set(State::High), Transaction::set(State::High)]);
        let mut charger = Charger::new(ce).unwrap();
        charger.hold_disabled().unwrap();
        charger.release().done();
    }
}
