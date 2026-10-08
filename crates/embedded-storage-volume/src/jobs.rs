//! Bounded receipts for asynchronous storage owners.
//!
//! Callers provide synchronization. Keeping completed receipts allows another
//! client to start work without changing the result of an earlier request.
use crate::Error;

/// Completion receipt for one nonzero caller-chosen request identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    /// Identifier supplied when the job was accepted.
    pub id: u64,
    /// The owner has finished all required I/O.
    pub complete: bool,
    /// Completed successfully; false while pending or after failure.
    pub ok: bool,
}

/// One active job and the four most recently accepted receipts.
/// Queries for evicted identifiers fail instead of borrowing another outcome.
#[derive(Default)]
pub struct Jobs {
    receipts: [Option<Receipt>; 4],
    next: usize,
}
impl Jobs {
    /// Construct empty receipt storage for a statically allocated owner.
    pub const fn new() -> Self {
        Self {
            receipts: [None; 4],
            next: 0,
        }
    }
    /// Accept one job. Reject duplicate identifiers and overlapping work.
    pub fn begin(&mut self, id: u64) -> Result<(), Error> {
        if id == 0
            || self
                .receipts
                .iter()
                .flatten()
                .any(|r| r.id == id || !r.complete)
        {
            return Err(Error::Bounds);
        }
        self.receipts[self.next] = Some(Receipt {
            id,
            complete: false,
            ok: false,
        });
        self.next = (self.next + 1) % self.receipts.len();
        Ok(())
    }
    /// Finish an accepted job exactly once, after its owner's I/O has ended.
    pub fn finish(&mut self, id: u64, ok: bool) -> Result<(), Error> {
        let receipt = self
            .receipts
            .iter_mut()
            .flatten()
            .find(|r| r.id == id && !r.complete)
            .ok_or(Error::Bounds)?;
        receipt.complete = true;
        receipt.ok = ok;
        Ok(())
    }
    /// Return the retained result for this identifier, without performing I/O.
    pub fn get(&self, id: u64) -> Option<Receipt> {
        self.receipts.iter().flatten().find(|r| r.id == id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_remains_correlated_across_later_jobs_and_eviction() {
        let mut jobs = Jobs::default();
        assert!(jobs.begin(0).is_err());
        jobs.begin(1).unwrap();
        assert!(jobs.begin(2).is_err());
        assert!(!jobs.get(1).unwrap().complete);
        jobs.finish(1, false).unwrap();
        assert!(jobs.finish(1, true).is_err());
        assert!(jobs.begin(1).is_err());
        for id in 2..=4 {
            jobs.begin(id).unwrap();
            jobs.finish(id, true).unwrap();
            assert_eq!(
                jobs.get(1),
                Some(Receipt {
                    id: 1,
                    complete: true,
                    ok: false
                })
            );
        }
        jobs.begin(5).unwrap();
        assert_eq!(jobs.get(1), None);
        assert!(jobs.finish(99, true).is_err());
    }
}
