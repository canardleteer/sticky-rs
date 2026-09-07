//! One frozen snapshot pull. Capacity is 1.

/// Host `GetSnapshot` result. UART tokens live in `embassy-debug`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GetOutcome {
    /// `Empty` → `Armed`. LAST was consistent.
    Armed,
    /// Already armed with this nonce (host retry).
    Retry,
    /// Armed with a different nonce. Echo that nonce.
    Busy {
        /// Pull the host still owes an Ack or Clear.
        armed: u64,
    },
    /// LAST was never published (or last copy was inconsistent).
    Empty,
    /// Nonce `0`. Does not arm.
    Zero,
}

/// Host `SnapshotAck` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckOutcome {
    /// Matching nonce. Slot is `Empty`.
    Released,
    /// Armed, but the nonce does not match. Slot unchanged.
    Miss {
        /// Pull still armed.
        armed: u64,
    },
    /// Ack while `Empty` (including after Clear).
    Stale,
    /// Nonce `0`. Slot unchanged.
    Zero,
}

/// `SnapshotClear` always succeeds. Armed or already empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearOutcome {
    /// Slot is `Empty`.
    Cleared,
}

/// Capacity-1 pull id. Planes stay with the implementor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SnapshotSlot {
    armed: Option<u64>,
}

impl SnapshotSlot {
    /// Empty slot (nothing frozen).
    #[must_use]
    pub const fn new() -> Self {
        Self { armed: None }
    }

    /// True while a host pull owns LAST.
    #[must_use]
    pub const fn is_armed(&self) -> bool {
        self.armed.is_some()
    }

    /// Armed nonce, or `None` when empty.
    #[must_use]
    pub const fn armed_nonce(&self) -> Option<u64> {
        self.armed
    }

    /// Arm, retry, or refuse a `GetSnapshot`.
    ///
    /// `last_ready` is the last consistent compose. Zero nonce does
    /// not arm. A different nonce while armed is [`GetOutcome::Busy`].
    pub fn on_get(&mut self, nonce: u64, last_ready: bool) -> GetOutcome {
        if nonce == 0 {
            return GetOutcome::Zero;
        }
        match self.armed {
            None if last_ready => {
                self.armed = Some(nonce);
                GetOutcome::Armed
            }
            None => GetOutcome::Empty,
            Some(armed) if armed == nonce => GetOutcome::Retry,
            Some(armed) => GetOutcome::Busy { armed },
        }
    }

    /// Release on a matching Ack. Mismatch / zero / stale do not
    /// change state.
    pub fn on_ack(&mut self, nonce: u64) -> AckOutcome {
        if nonce == 0 {
            return AckOutcome::Zero;
        }
        match self.armed {
            None => AckOutcome::Stale,
            Some(armed) if armed == nonce => {
                self.armed = None;
                AckOutcome::Released
            }
            Some(armed) => AckOutcome::Miss { armed },
        }
    }

    /// Operator abort. Always [`ClearOutcome::Cleared`].
    pub fn on_clear(&mut self) -> ClearOutcome {
        self.armed = None;
        ClearOutcome::Cleared
    }
}
