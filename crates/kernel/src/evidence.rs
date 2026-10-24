//! What an operation reports beside its outcome, for people and tests to
//! find where it went: the profile of its work when it runs out
//! ([`budget_profile`]), and the evidence of a failure or a warning.
//!
//! **The evidence pass.** Deciding carries only the failing key. Once the
//! outcome is fixed, evidence is built in a pass of its own, charged to
//! its own allowance ([`EVIDENCE_WORK`], [`Allowance`]), never to the
//! operation's budget: running out of it degrades the items after that
//! point to their names and counts them. So evidence never changes a
//! result (its body, tolerances, bounds or history, nor its warnings,
//! their bounds and their count), an error's kind or key, or any decision;
//! only the evidence attached to failures, warnings, notes and zones
//! depends on the allowance.
//!
//! The pieces built so far: the budget profile and the degrading of
//! warnings' evidence ([`budget_profile`]), and the evidence of a
//! tangency cluster no rule settles ([`cluster`]).

pub mod budget_profile;
pub mod cluster;

/// The units, as the [`Budget`](crate::Budget)'s, one evidence pass may
/// spend apart from the operation's budget: about 30 ms of work on one
/// thread. Past it, items are degraded to their names.
pub const EVIDENCE_WORK: u64 = 1 << 16;

/// The allowance of one evidence pass, spent in the pass's fixed order:
/// whether an item gets its evidence depends only on the items before it,
/// so it is the same on every run. Once it can't afford an item it is
/// spent: every later item is degraded too, however small.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Allowance {
    left: u64,
    spent: bool,
}

impl Allowance {
    /// An allowance of `units`; [`Allowance::default`] is
    /// [`EVIDENCE_WORK`]. Tests give 0 and `u64::MAX` to show the outcome
    /// doesn't depend on it.
    pub(crate) fn new(units: u64) -> Allowance {
        Allowance {
            left: units,
            spent: false,
        }
    }

    /// Takes `units` for the next item, before its evidence is built:
    /// whether they were left. Once not, it takes nothing more.
    pub(crate) fn afford(&mut self, units: u64) -> bool {
        if self.spent || units > self.left {
            self.spent = true;
            return false;
        }
        self.left -= units;
        true
    }
}

impl Default for Allowance {
    fn default() -> Self {
        Allowance::new(EVIDENCE_WORK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_allowance_once_spent_affords_nothing() {
        let mut a = Allowance::new(10);
        assert!(a.afford(4));
        assert!(a.afford(0));
        assert!(!a.afford(7));
        // Smaller than what was left, but after an item it couldn't afford.
        assert!(!a.afford(1));
        assert!(!a.afford(0));
        let mut full = Allowance::default();
        assert!(full.afford(EVIDENCE_WORK));
        assert!(!full.afford(1));
        let mut none = Allowance::new(0);
        assert!(none.afford(0));
        assert!(!none.afford(1));
    }
}
