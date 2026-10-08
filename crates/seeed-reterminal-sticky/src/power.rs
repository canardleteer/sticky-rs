//! The power latch, as a type you cannot skip.
//!
//! Drive `PWR_HOLD` (GPIO45) then `PWR_LOCK` (GPIO46) high before operation,
//! according to the board contract. Keep these outputs high during operation;
//! GPIO levels alone do not measure the MCU supply.
//!
//! # Why a witness type
//!
//! Every rail and bus constructor in this crate wants a [`&Latched`][Latched]
//! reference. "Bring up a peripheral before latching power" therefore does not
//! compile, which is a better guarantee than a comment at the top of `main`.
//!
//! # Releasing is a decision
//!
//! [`Latch::release`] drives both control pins low to request board power-off.
//! Actual MCU supply removal depends on the board circuit and external power;
//! the GPIO writes alone cannot establish it. Stock firmware latches during
//! init and then releases when the power button was not the boot cause — a deliberate
//! policy, and one that looks exactly like a crash if you copy it by accident
//! while running from USB.

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;

/// GPIO settle after driving the latch high, used by [`Latch::acquire`].
///
/// Official `board_init` waits [`LATCH_PERIPHERAL_SETTLE_MS`] (100 ms)
/// before talking to peripherals. This path stays 10 ms (observed
/// bring-up that already works). The 100 ms figure is vendor intent
/// until someone measures the latch deadline.
const LATCH_SETTLE_MS: u32 = 10;

/// Official dashboard `board_init` wait after latch high, in milliseconds.
///
/// Vendor intent, not measured on a unit in this tree. [`Latch::acquire`]
/// uses 10 ms. Do not pulse GPIO46 while waiting.
pub const LATCH_PERIPHERAL_SETTLE_MS: u32 = 100;

/// Witness that the latch's high output writes and settle completed.
///
/// Obtained from [`Latch::acquire`] and required by every rail constructor.
/// It has no public constructor. It enforces software sequencing without
/// measuring the latch circuit's electrical response.
#[derive(Debug)]
pub struct Latched {
    _private: (),
}

/// The two latch pins.
#[derive(Debug)]
pub struct Latch<HOLD, LOCK> {
    hold: HOLD,
    lock: LOCK,
    witness: Latched,
}

impl<HOLD, LOCK> Latch<HOLD, LOCK>
where
    HOLD: OutputPin,
    LOCK: OutputPin<Error = HOLD::Error>,
{
    /// Drives `PWR_HOLD` high, then `PWR_LOCK` high, then settles.
    ///
    /// Call this before logging, bus init, or anything else. Order matters and
    /// is fixed here so callers cannot get it wrong.
    ///
    /// `PWR_HOLD` then `PWR_LOCK` are strapping pins with default weak
    /// pull-down (ESP32-S3 v2.2 section `3 Boot Configurations` /
    /// `Table 3-1. Default Configuration of Strapping Pins`). Drive them
    /// high; never pulse `PWR_LOCK`.
    pub fn acquire<D: DelayNs>(
        mut hold: HOLD,
        mut lock: LOCK,
        delay: &mut D,
    ) -> Result<Self, HOLD::Error> {
        hold.set_high()?;
        lock.set_high()?;
        delay.delay_ms(LATCH_SETTLE_MS);
        Ok(Self {
            hold,
            lock,
            witness: Latched { _private: () },
        })
    }

    /// The witness that rails and buses require.
    #[inline]
    #[must_use]
    pub const fn witness(&self) -> &Latched {
        &self.witness
    }

    /// Drives both latch control pins low to request power-off on battery.
    ///
    /// Only the deliberate shutdown path should call this. Everything that can
    /// fail before shutdown should have failed already, because after this the
    /// board may simply stop. This method reports GPIO write errors; verify MCU
    /// supply removal independently on the actual board.
    pub fn release(mut self) -> Result<(HOLD, LOCK), HOLD::Error> {
        self.lock.set_low()?;
        self.hold.set_low()?;
        Ok((self.hold, self.lock))
    }

    /// Consumes the latch and returns the pins **without** dropping them
    /// (`C-FREE`). The board stays powered; the caller takes over the pins.
    #[inline]
    pub fn release_ownership_only(self) -> (HOLD, LOCK) {
        (self.hold, self.lock)
    }
}

#[cfg(test)]
mod tests {
    use embedded_hal_mock::eh1::delay::{CheckedDelay, Transaction as DelayTransaction};
    use embedded_hal_mock::eh1::digital::{Mock, State, Transaction};

    use super::*;

    #[test]
    fn official_peripheral_settle_is_named_and_unused_by_acquire() {
        assert_eq!(LATCH_PERIPHERAL_SETTLE_MS, 100);
        assert_eq!(LATCH_SETTLE_MS, 10);
        assert_ne!(LATCH_SETTLE_MS, LATCH_PERIPHERAL_SETTLE_MS);
    }

    #[test]
    fn acquire_drives_hold_before_lock_and_then_settles() {
        let hold = Mock::new(&[Transaction::set(State::High)]);
        let lock = Mock::new(&[Transaction::set(State::High)]);
        let mut delay = CheckedDelay::new(&[DelayTransaction::delay_ms(LATCH_SETTLE_MS)]);

        let latch = Latch::acquire(hold, lock, &mut delay).unwrap();

        // Ordering between the two pins is enforced by each mock seeing exactly
        // one write, and by the settle transaction landing last.
        delay.done();
        let (mut hold, mut lock) = latch.release_ownership_only();
        hold.done();
        lock.done();
    }

    #[test]
    fn release_drops_lock_before_hold() {
        let hold = Mock::new(&[Transaction::set(State::High), Transaction::set(State::Low)]);
        let lock = Mock::new(&[Transaction::set(State::High), Transaction::set(State::Low)]);
        let mut delay = CheckedDelay::new(&[DelayTransaction::delay_ms(LATCH_SETTLE_MS)]);

        let latch = Latch::acquire(hold, lock, &mut delay).unwrap();
        let (mut hold, mut lock) = latch.release().unwrap();

        delay.done();
        hold.done();
        lock.done();
    }
}
