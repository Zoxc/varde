use crate::{KernelError, MAX_WORK};

/// How much work one kernel operation may do, in units of about one patch
/// or one pair of patches tested or split. Running out gives
/// [`KernelError::TooComplex`], never a hang.
///
/// The work is counted in sequential passes over results collected in a
/// fixed order, so whether an operation runs out doesn't depend on the
/// thread count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    work: u64,
}

impl Budget {
    /// The most any operation may do: [`MAX_WORK`].
    pub const DEFAULT: Budget = Budget { work: MAX_WORK };

    /// A budget of `work` units, at most [`MAX_WORK`].
    pub fn new(work: u64) -> Budget {
        Budget {
            work: work.min(MAX_WORK),
        }
    }

    /// The units it allows.
    pub fn work(self) -> u64 {
        self.work
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The work an operation has left, counted down from its [`Budget`].
/// Steps of one operation share one.
#[derive(Debug)]
pub(crate) struct Work {
    left: u64,
}

impl Work {
    pub(crate) fn new(budget: &Budget) -> Work {
        Work { left: budget.work }
    }

    /// The units left.
    pub(crate) fn left(&self) -> u64 {
        self.left
    }

    /// Takes `units` more, or fails with [`KernelError::TooComplex`] once
    /// there aren't that many left.
    pub(crate) fn spend(&mut self, units: usize) -> Result<(), KernelError> {
        let units = u64::try_from(units).unwrap_or(u64::MAX);
        match self.left.checked_sub(units) {
            Some(left) => {
                self.left = left;
                Ok(())
            }
            None => {
                self.left = 0;
                Err(KernelError::TooComplex)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_runs_out() {
        let mut work = Work::new(&Budget::new(10));
        assert_eq!(work.spend(4), Ok(()));
        assert_eq!(work.spend(6), Ok(()));
        assert_eq!(work.spend(1), Err(KernelError::TooComplex));
        assert_eq!(work.spend(0), Ok(()));
        assert_eq!(Budget::new(u64::MAX), Budget::DEFAULT);
        let mut work = Work::new(&Budget::DEFAULT);
        assert_eq!(work.spend(usize::MAX), Err(KernelError::TooComplex));
    }
}
