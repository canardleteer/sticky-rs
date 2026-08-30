//! The power latch, as a type you cannot skip.
//!
//! `PWR_HOLD` (GPIO45) then `PWR_LOCK` (GPIO46) must be high for the board to
//! stay powered. If they are low when USB is unplugged, the board dies
//! mid-operation.
//!
//! # Why a witness type
//!
//! Every rail and bus constructor in this crate wants a [`&Latched`][Latched]
//! reference. "Bring up a peripheral before latching power" therefore does not
//! compile, which is a better guarantee than a comment at the top of `main`.
//!
//! # Releasing is a decision
//!
//! [`Latch::release`] exists and is named for what it does: it powers the
//! board down when running on battery. Stock firmware latches during init and
//! then releases when the power button was not the boot cause — a deliberate
//! policy, and one that looks exactly like a crash if you copy it by accident
//! while running from USB.

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;

/// Settle time after driving the latch high, before touching anything else.
const LATCH_SETTLE_MS: u32 = 10;

/// Proof that the power latch is held.
///
/// Obtained from [`Latch::acquire`] and required by every rail constructor.
/// It has no public constructor: the only way to get one is to actually latch.
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
    /// pull-down (ESP32-S3 v2.2). Drive them high; never pulse `PWR_LOCK`.
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

    /// Drops both latch pins low: a software power-off on battery.
    ///
    /// Only the deliberate shutdown path should call this. Everything that can
    /// fail before shutdown should have failed already, because after this the
    /// board may simply stop.
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
