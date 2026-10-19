//! The work budget of one kernel operation, charged so that whether and
//! where an operation runs out doesn't depend on the thread count.
//!
//! An operation runs as passes: maps over items sorted by stable keys.
//! [`Work::pass`] runs a pass's items through [`par_map_indexed`], each
//! against its own [`Ledger`] with the share "budget left at the pass
//! start", so a runaway item stops on its own. Then, sequentially and in key order, it
//! sums the items' costs. The pass is decided at the first key whose
//! cumulative cost goes past the budget (the budget runs out there), or,
//! if earlier, at the first key that failed; at one key, running out comes
//! first. Only that prefix of keys counts: towards the work used, which
//! the next attempt runs on, and towards the [`BudgetProfile`]; the items
//! after it are as if never run. Each item depends only on itself and the
//! share, so the outcome, the work used and the profile are the same on any
//! number of threads.
//!
//! While the items run, the keys whose items have finished are summed in
//! key order as far as they go, and once that prefix decides the pass the
//! items after its deciding key stop at their next charge: they can't
//! change the outcome, and without stopping them a pass of `n` items could
//! do `n` times the budget's work.
//!
//! Each ledger records its item's steps in order, each a [`Step`]: a
//! precision step (a computation repeated at a higher precision) or any
//! other. Running out is [`Cause::Precision`] when the unit the budget
//! ran out at belongs to a precision step, [`Cause::Work`] otherwise.
//! Reaching a fixed limit ([`Cap`]) is its own failure, never a budget
//! crossing.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use varde_decide::records::shadow;

use super::error::{Cause, KernelError};
use super::par::par_map_indexed;

/// The most work one operation may do, in the units of [`Budget`]: about
/// two seconds of work on one thread of the reference machine, less on
/// several. A unit is about half a microsecond, each computation kind
/// charged its own units from the unit table ([`shadow::UNITS`]); the
/// calibration benchmark (`benches/calibrate.rs`) measures a seam pass
/// that uses it up at 2.1 s on one thread, and fails past four times the
/// two seconds.
pub const MAX_WORK: u64 = 1 << 22;

pub use varde_decide::records::shadow::MAX_SUBDIV_DEPTH;

/// How much work one kernel operation may do, in units of about half a
/// microsecond of work ([`MAX_WORK`]). Running out gives
/// [`KernelError::OutOfBudget`], never a hang. One budget covers the
/// whole operation, its retries included.
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

/// The kind of one charged step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    /// Any step but a precision step: a box tested or split, a record
    /// evaluated, a certificate checked.
    Work,
    /// A computation repeated at precision level `level` (0 is `f64`).
    Precision {
        /// The precision level.
        level: u8,
    },
}

/// The precision levels of the escalation schedule: `f64` (0),
/// double-double (1), then big-float at 212, 424 and 848 bits (2 to 4).
/// The top level's width is the highest precision there is: an item still
/// open there reaches [`Cap::PrecisionBits`].
pub(crate) const LEVELS: u8 = shadow::LEVELS as u8;

/// The step of one computation at precision level `level`: ordinary work
/// at the first level, a precision step above it.
pub(crate) fn level_step(level: u8) -> Step {
    match level {
        0 => Step::Work,
        level => Step::Precision { level },
    }
}

/// The units of one seam-bound cell at precision level `level`, from the
/// unit table ([`shadow::UNITS`]).
pub(crate) fn seam_cell_units(level: u8) -> u64 {
    shadow::UNITS.seam_cell[usize::from(level)]
}

/// What [`KernelError::OutOfBudget`] reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Limit {
    /// The operation's [`Budget`], of this many units.
    Budget(u64),
    /// A fixed limit of one item.
    Cap(Cap),
}

/// A fixed limit of one item of a pass: reaching it ends the operation
/// whatever budget is left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cap {
    /// The highest precision of the escalation schedule, a sign or
    /// certificate still open ([`Cause::Precision`]).
    PrecisionBits,
    /// The deepest subdivision of an item, [`MAX_SUBDIV_DEPTH`].
    SubdivisionDepth,
    /// The deepest search for a point certifying a region's label.
    AuditDepth,
    /// The most rounds of refitting one curve or surface.
    RefitRounds,
    /// The most triangles of a tessellation.
    Triangles,
    /// The most refinement of one site of a tessellation.
    Refinement,
}

impl Cap {
    /// The cause of reaching this limit.
    fn cause(self) -> Cause {
        match self {
            Cap::PrecisionBits => Cause::Precision,
            Cap::SubdivisionDepth
            | Cap::AuditDepth
            | Cap::RefitRounds
            | Cap::Triangles
            | Cap::Refinement => Cause::SizeLimit,
        }
    }
}

/// One item of one pass of an operation: its position among the pass's
/// key-sorted items. Positions follow from the keys, so they are the same
/// on every run; the operation maps them back to its keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemAt {
    /// The pass, counted from 0 over the operation.
    pub pass: u64,
    /// The item's position in the pass (as [`Halt::Failed`]'s).
    pub item: usize,
}

/// Where an operation stopped: the item that decided its pass, and the
/// step of its ledger the budget ran out at, or its last step on reaching
/// another limit (`None` if it had charged none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stopped {
    /// The deciding item.
    pub at: ItemAt,
    /// The stage of the item's pass.
    pub stage: &'static str,
    /// The attempt (rung of the escalation ladder) of the item's pass.
    pub rung: u8,
    /// The step.
    pub step: Option<Step>,
}

/// The most items [`BudgetProfile::hottest`] lists.
pub const HOTTEST: usize = 8;

/// Where an operation's work went, up to the point it ran out: over the
/// passes before and, in the last pass, the items up to the deciding one,
/// that one counted up to where the budget ran out. Read from the
/// budget's own counts (not charged); the same on any number of threads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetProfile {
    /// The limit reached.
    pub limit: Limit,
    /// The units used.
    pub used: u64,
    /// The units of steps that are not precision steps.
    pub work: u64,
    /// The units of precision steps, by precision level.
    pub precision: Vec<u64>,
    /// The units by stage, in the order the stages first charged.
    pub stages: Vec<(&'static str, u64)>,
    /// The units by attempt (rung), from rung 0.
    pub rungs: Vec<u64>,
    /// Where it stopped.
    pub stopped: Stopped,
    /// The items that took the most units, at most [`HOTTEST`], most first
    /// (ties in pass and item order).
    pub hottest: Vec<(ItemAt, u64)>,
}

/// One pass of an operation: one stage's map over its key-sorted items at
/// one attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pass {
    /// The stage, for the profile.
    pub(crate) stage: &'static str,
    /// The attempt: the rung of the escalation ladder, from 0.
    pub(crate) rung: u8,
}

/// The steps one item of a pass charged, in its own order, against its
/// share: the budget left when the pass started.
#[derive(Debug)]
pub(crate) struct Ledger<'p> {
    share: u64,
    total: u64,
    /// Runs of steps of one kind: consecutive steps of a kind merged.
    steps: Vec<(Step, u64)>,
    /// The item's position in its pass.
    item: usize,
    frontier: &'p Frontier,
}

/// An item went past its share, the budget left at its pass's start, so
/// the pass runs out at it or before it; or the pass is decided at an
/// earlier key. Either way the item must stop. Only [`Ledger::charge`]
/// gives one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Exhausted(());

/// Why an item of a pass stopped.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stop<E> {
    /// It went past its share ([`Ledger::charge`]).
    Exhausted(Exhausted),
    /// It reached a fixed limit.
    Cap(Cap),
    /// It failed.
    Failed(E),
}

impl<E> From<Exhausted> for Stop<E> {
    fn from(exhausted: Exhausted) -> Self {
        Stop::Exhausted(exhausted)
    }
}

/// How a pass ended without results.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Halt<E> {
    /// The budget ran out, or an item reached a fixed limit:
    /// [`KernelError::OutOfBudget`], which ends the operation.
    OutOfBudget(KernelError),
    /// The item at position `item`, the first to fail, failed with
    /// `error`; the work used counts the items up to it.
    Failed {
        /// The item's position in the pass.
        item: usize,
        /// Its failure.
        error: E,
    },
}

impl<E: Into<KernelError>> From<Halt<E>> for KernelError {
    fn from(halt: Halt<E>) -> Self {
        match halt {
            Halt::OutOfBudget(error) => error,
            Halt::Failed { error, .. } => error.into(),
        }
    }
}

impl<'p> Ledger<'p> {
    fn new(share: u64, item: usize, frontier: &'p Frontier) -> Ledger<'p> {
        Ledger {
            share,
            total: 0,
            steps: Vec::new(),
            item,
            frontier,
        }
    }

    /// Charges `units` of `step`, or fails with [`Exhausted`] once the
    /// item's charges go past its share or the pass is decided at an
    /// earlier key; the item must then stop.
    pub(crate) fn charge(&mut self, step: Step, units: u64) -> Result<(), Exhausted> {
        self.total = self.total.saturating_add(units);
        match self.steps.last_mut() {
            Some((last, run)) if *last == step => *run = run.saturating_add(units),
            _ => self.steps.push((step, units)),
        }
        if self.total > self.share || self.frontier.decided_before(self.item) {
            Err(Exhausted(()))
        } else {
            Ok(())
        }
    }

    /// The step holding unit `offset` (from 0) of the item's charges.
    fn step_at(&self, offset: u64) -> Option<Step> {
        let mut start = 0u64;
        for &(step, units) in &self.steps {
            let end = start.saturating_add(units);
            if offset < end {
                return Some(step);
            }
            start = end;
        }
        None
    }
}

/// The items of one running pass, summed in key order as far as they have
/// finished, to find the deciding key while the later items still run.
/// It only ever names the key the pass is decided at, so it stops only
/// items whose outcome doesn't count.
#[derive(Debug)]
pub(crate) struct Frontier {
    /// The budget left at the pass start.
    left: u64,
    /// Per item, [`Frontier::RUNNING`] until it finishes, then its units
    /// (at most `left + 1`, past any room) with [`Frontier::STOPPED`] if it
    /// gave no result.
    finished: Vec<AtomicU64>,
    /// The first item not yet summed, and the units of those before it.
    prefix: Mutex<(usize, u64)>,
    /// The deciding item once the prefix reaches it, `usize::MAX` before.
    decided: AtomicUsize,
}

impl Frontier {
    const RUNNING: u64 = u64::MAX;
    const STOPPED: u64 = 1 << 63;

    fn new(left: u64, items: usize) -> Frontier {
        Frontier {
            left,
            finished: (0..items).map(|_| AtomicU64::new(Self::RUNNING)).collect(),
            prefix: Mutex::new((0, 0)),
            decided: AtomicUsize::new(usize::MAX),
        }
    }

    /// Whether the pass is decided at a key before `item`'s.
    fn decided_before(&self, item: usize) -> bool {
        self.decided.load(Ordering::Relaxed) < item
    }

    /// Records that `item` finished, having charged `total`, with a result
    /// or not, and sums the finished prefix on as far as it goes: up to the
    /// first item that runs out or gives no result, which decides the
    /// pass, as in [`Work::pass`].
    fn finish(&self, item: usize, total: u64, result: bool) {
        let units = total.min(self.left + 1);
        let mark = if result { 0 } else { Self::STOPPED };
        self.finished[item].store(units | mark, Ordering::Release);
        let mut prefix = self.prefix.lock().unwrap_or_else(PoisonError::into_inner);
        let (next, sum) = &mut *prefix;
        while self.decided.load(Ordering::Relaxed) == usize::MAX && *next < self.finished.len() {
            let done = self.finished[*next].load(Ordering::Acquire);
            if done == Self::RUNNING {
                break;
            }
            let units = done & !Self::STOPPED;
            if done & Self::STOPPED != 0 || units > self.left - *sum {
                self.decided.store(*next, Ordering::Relaxed);
                break;
            }
            *sum += units;
            *next += 1;
        }
    }
}

/// The work an operation has done and has left, counted up from 0 to its
/// [`Budget`], and where it went. The passes of one operation, its
/// retries included, share one.
#[derive(Debug)]
pub(crate) struct Work {
    budget: u64,
    used: u64,
    passes: u64,
    work: u64,
    precision: Vec<u64>,
    stages: Vec<(&'static str, u64)>,
    rungs: Vec<u64>,
    hottest: Vec<(ItemAt, u64)>,
}

impl Work {
    pub(crate) fn new(budget: &Budget) -> Work {
        Work {
            budget: budget.work,
            used: 0,
            passes: 0,
            work: 0,
            precision: Vec::new(),
            stages: Vec::new(),
            rungs: Vec::new(),
            hottest: Vec::new(),
        }
    }

    /// The units used.
    pub(crate) fn used(&self) -> u64 {
        self.used
    }

    /// The units left.
    pub(crate) fn left(&self) -> u64 {
        self.budget - self.used
    }

    /// Runs `f` on each of `items` (sorted by their keys) with
    /// [`par_map_indexed`], each charging its own [`Ledger`], and gives the
    /// results in order; or how the pass ended without them: the budget ran out
    /// at a key, or an item reached a fixed limit or failed, whichever key
    /// came first (running out first at one key). The work used and the
    /// profile count the keys up to that one. `f` must depend only on its
    /// item, and charge each step before the work it stands for; it stops
    /// at a charge once an earlier key decides the pass.
    pub(crate) fn pass<T, R, E, F>(
        &mut self,
        pass: Pass,
        items: &[T],
        f: F,
    ) -> Result<Vec<R>, Halt<E>>
    where
        T: Sync,
        R: Send,
        E: Send,
        F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<E>> + Sync + Send,
    {
        let index = self.passes;
        self.passes = self.passes.saturating_add(1);
        let left = self.left();
        let frontier = Frontier::new(left, items.len());
        let ran = par_map_indexed(items, |i, item| {
            let mut ledger = Ledger::new(left, i, &frontier);
            let result = f(item, &mut ledger);
            frontier.finish(i, ledger.total, result.is_ok());
            (ledger, result)
        });
        let mut out = Vec::with_capacity(ran.len());
        let mut sum = 0u64;
        for (i, (ledger, result)) in ran.into_iter().enumerate() {
            let at = ItemAt {
                pass: index,
                item: i,
            };
            let room = left - sum;
            let stopped = |step| Stopped {
                at,
                stage: pass.stage,
                rung: pass.rung,
                step,
            };
            // Past the room the budget runs out at this key, whatever the
            // item gave: its own failure follows the crossing in its
            // ledger. An exhausted item is past its share, at least the
            // room, so it lands here too: the items stopped because an
            // earlier key decides the pass come after this loop returns.
            let crossed = ledger.total > room;
            if !crossed {
                sum += ledger.total;
                self.count(pass, at, &ledger, ledger.total);
            }
            match result {
                Ok(r) if !crossed => out.push(r),
                Err(Stop::Cap(cap)) if !crossed => {
                    self.used += sum;
                    let step = ledger.steps.last().map(|&(step, _)| step);
                    return Err(Halt::OutOfBudget(self.out_of_budget(
                        cap.cause(),
                        Limit::Cap(cap),
                        stopped(step),
                    )));
                }
                Err(Stop::Failed(error)) if !crossed => {
                    self.used += sum;
                    return Err(Halt::Failed { item: i, error });
                }
                _ => {
                    let step = ledger.step_at(room);
                    self.count(pass, at, &ledger, room);
                    self.used = self.budget;
                    let cause = match step {
                        Some(Step::Precision { .. }) => Cause::Precision,
                        Some(Step::Work) | None => Cause::Work,
                    };
                    return Err(Halt::OutOfBudget(self.out_of_budget(
                        cause,
                        Limit::Budget(self.budget),
                        stopped(step),
                    )));
                }
            }
        }
        self.used += sum;
        Ok(out)
    }

    /// Adds the first `units` of `ledger`'s charges, those of item `at`
    /// of `pass`, to the profile's counts.
    fn count(&mut self, pass: Pass, at: ItemAt, ledger: &Ledger<'_>, units: u64) {
        if units == 0 {
            return;
        }
        let mut left = units;
        for &(step, run) in &ledger.steps {
            let take = run.min(left);
            match step {
                Step::Work => self.work += take,
                Step::Precision { level } => {
                    let level = usize::from(level);
                    if self.precision.len() <= level {
                        self.precision.resize(level + 1, 0);
                    }
                    self.precision[level] += take;
                }
            }
            left -= take;
            if left == 0 {
                break;
            }
        }
        match self
            .stages
            .iter_mut()
            .find(|(stage, _)| *stage == pass.stage)
        {
            Some((_, n)) => *n += units,
            None => self.stages.push((pass.stage, units)),
        }
        let rung = usize::from(pass.rung);
        if self.rungs.len() <= rung {
            self.rungs.resize(rung + 1, 0);
        }
        self.rungs[rung] += units;
        // Most units first, ties in key order; items come in key order,
        // so a new one goes after those with as many units.
        let place = self.hottest.partition_point(|&(_, n)| n >= units);
        if place < HOTTEST {
            self.hottest.insert(place, (at, units));
            self.hottest.truncate(HOTTEST);
        }
    }

    fn out_of_budget(&self, cause: Cause, limit: Limit, stopped: Stopped) -> KernelError {
        KernelError::OutOfBudget {
            cause,
            profile: Box::new(BudgetProfile {
                limit,
                used: self.used,
                work: self.work,
                precision: self.precision.clone(),
                stages: self.stages.clone(),
                rungs: self.rungs.clone(),
                stopped,
                hottest: self.hottest.clone(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The schedule: `f64`, double-double and the big-float widths, the
    /// last at 848 bits.
    #[test]
    fn the_schedule_ends_at_848_bits() {
        use varde_decide::num::bigf::Width;
        assert_eq!(usize::from(LEVELS), 2 + Width::ALL.len());
        assert_eq!(Width::MAX.bits(), 848);
        assert_eq!(level_step(LEVELS - 1), Step::Precision { level: 4 });
    }

    const PASS: Pass = Pass {
        stage: "test",
        rung: 0,
    };

    /// Charges each of an item's steps, then gives the item's number;
    /// `Err(..)` as the last step fails the item.
    fn run(
        steps: &[(Step, u64)],
        fail: bool,
        ledger: &mut Ledger,
    ) -> Result<u64, Stop<&'static str>> {
        for &(step, units) in steps {
            ledger.charge(step, units)?;
        }
        if fail {
            return Err(Stop::Failed("failed"));
        }
        Ok(steps.iter().map(|&(_, units)| units).sum())
    }

    const W: Step = Step::Work;
    const P1: Step = Step::Precision { level: 1 };

    fn out_of_budget<E: std::fmt::Debug>(halt: Halt<E>) -> (Cause, BudgetProfile) {
        match halt {
            Halt::OutOfBudget(KernelError::OutOfBudget { cause, profile }) => (cause, *profile),
            other => panic!("not out of budget: {other:?}"),
        }
    }

    #[test]
    fn a_pass_within_the_budget_gives_every_result_in_order() {
        let mut work = Work::new(&Budget::new(100));
        let items: Vec<u64> = (1..=10).collect();
        let out = work
            .pass(PASS, &items, |&n, ledger| run(&[(W, n)], false, ledger))
            .unwrap();
        assert_eq!(out, items);
        assert_eq!((work.used(), work.left()), (55, 45));
    }

    #[test]
    fn the_budget_runs_out_at_the_first_key_past_it() {
        // 10 units: 3 + 5 leave 2, and the third item runs out at its
        // third unit, in its precision step: the cause is `Precision`.
        let items = [
            vec![(W, 3)],
            vec![(W, 5)],
            vec![(W, 1), (P1, 4)],
            vec![(W, 1)],
        ];
        let mut work = Work::new(&Budget::new(10));
        let halt = work
            .pass(PASS, &items, |steps, ledger| run(steps, false, ledger))
            .unwrap_err();
        let (cause, profile) = out_of_budget(halt);
        assert_eq!(cause, Cause::Precision);
        assert_eq!(work.used(), 10);
        assert_eq!(profile.limit, Limit::Budget(10));
        assert_eq!(profile.used, 10);
        // The third item is counted up to the crossing: 1 work, 1 precision.
        assert_eq!((profile.work, profile.precision.clone()), (9, vec![0, 1]));
        assert_eq!(profile.stopped.at, ItemAt { pass: 0, item: 2 });
        assert_eq!(profile.stopped.step, Some(P1));
        assert_eq!(profile.stages, vec![("test", 10)]);
        assert_eq!(profile.rungs, vec![10]);
        let hottest: Vec<(usize, u64)> = profile
            .hottest
            .iter()
            .map(|(at, n)| (at.item, *n))
            .collect();
        assert_eq!(hottest, vec![(1, 5), (0, 3), (2, 2)]);

        // Running out within the item's work step is `Work`.
        let items = [vec![(W, 3)], vec![(W, 5)], vec![(W, 3), (P1, 4)]];
        let mut work = Work::new(&Budget::new(10));
        let halt = work
            .pass(PASS, &items, |steps, ledger| run(steps, false, ledger))
            .unwrap_err();
        assert_eq!(out_of_budget(halt).0, Cause::Work);
    }

    #[test]
    fn the_first_failing_key_decides_when_before_the_crossing() {
        let items = [(1, false), (2, true), (100, false), (1, true)];
        let mut work = Work::new(&Budget::new(10));
        let halt = work
            .pass(PASS, &items, |&(n, fail), ledger| {
                run(&[(W, n)], fail, ledger)
            })
            .unwrap_err();
        assert_eq!(
            halt,
            Halt::Failed {
                item: 1,
                error: "failed"
            }
        );
        // The prefix through the failing key: the next attempt runs on
        // what is left of it.
        assert_eq!(work.used(), 3);
        let out = work
            .pass(
                Pass {
                    stage: "test",
                    rung: 1,
                },
                &[7],
                |&n, ledger| run(&[(W, n)], false, ledger),
            )
            .unwrap();
        assert_eq!((out, work.left()), (vec![7], 0));
    }

    #[test]
    fn at_one_key_running_out_comes_before_the_failure() {
        let items = [(4, false), (7, true)];
        let mut work = Work::new(&Budget::new(10));
        let halt = work
            .pass(PASS, &items, |&(n, fail), ledger| {
                run(&[(W, n)], fail, ledger)
            })
            .unwrap_err();
        let (cause, profile) = out_of_budget(halt);
        assert_eq!(cause, Cause::Work);
        assert_eq!(profile.stopped.at.item, 1);
    }

    #[test]
    fn a_fixed_limit_is_its_own_cause() {
        let mut work = Work::new(&Budget::new(100));
        let run_to = |cap| {
            move |&n: &u64, ledger: &mut Ledger| -> Result<u64, Stop<()>> {
                ledger.charge(W, n)?;
                if n == 3 { Err(Stop::Cap(cap)) } else { Ok(n) }
            }
        };
        let halt = work
            .pass(PASS, &[1, 2, 3, 4], run_to(Cap::SubdivisionDepth))
            .unwrap_err();
        let Halt::OutOfBudget(KernelError::OutOfBudget { cause, profile }) = halt else {
            panic!("{halt:?}");
        };
        assert_eq!(
            (cause, profile.limit, profile.used),
            (Cause::SizeLimit, Limit::Cap(Cap::SubdivisionDepth), 6)
        );
        assert_eq!(
            (profile.stopped.at.item, profile.stopped.step),
            (2, Some(W))
        );
        assert_eq!(work.used(), 6);
        let halt = work
            .pass(PASS, &[3], run_to(Cap::PrecisionBits))
            .unwrap_err();
        let Halt::OutOfBudget(KernelError::OutOfBudget { cause, .. }) = halt else {
            panic!("{halt:?}");
        };
        assert_eq!(cause, Cause::Precision);
    }

    #[test]
    fn a_runaway_item_stops_at_its_share() {
        let mut work = Work::new(&Budget::new(1000));
        let halt = work
            .pass(PASS, &[(), ()], |_, ledger| -> Result<(), Stop<()>> {
                loop {
                    ledger.charge(W, 1)?;
                }
            })
            .unwrap_err();
        let (cause, profile) = out_of_budget(halt);
        assert_eq!(
            (cause, profile.used, profile.stopped.at.item),
            (Cause::Work, 1000, 0)
        );
    }

    /// A pass that runs out stops its items after the deciding key, so the
    /// work done stays near the budget however many items there are: on
    /// one thread, items of 100 units each past a budget of 1000, and
    /// items that never stop on their own.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn work_done_stays_near_the_budget_on_one_thread() {
        use super::super::par::on_threads;
        use std::sync::atomic::{AtomicU64, Ordering};
        let done = AtomicU64::new(0);
        let charge = |ledger: &mut Ledger| -> Result<(), Exhausted> {
            ledger.charge(W, 1)?;
            done.fetch_add(1, Ordering::Relaxed);
            Ok(())
        };
        let items = vec![(); 10_000];
        on_threads(1, || {
            let mut work = Work::new(&Budget::new(1000));
            let halt = work
                .pass(PASS, &items, |_, ledger| -> Result<(), Stop<()>> {
                    for _ in 0..100 {
                        charge(ledger)?;
                    }
                    Ok(())
                })
                .unwrap_err();
            assert_eq!(out_of_budget(halt).1.stopped.at.item, 10);
        });
        let ran = done.swap(0, Ordering::Relaxed);
        assert!(ran <= 2000, "{ran} units of work for a budget of 1000");
        on_threads(1, || {
            let mut work = Work::new(&Budget::new(1000));
            let halt = work
                .pass(PASS, &items, |_, ledger| -> Result<(), Stop<()>> {
                    loop {
                        charge(ledger)?;
                    }
                })
                .unwrap_err();
            assert_eq!(out_of_budget(halt).1.stopped.at.item, 0);
        });
        let ran = done.load(Ordering::Relaxed);
        assert!(ran <= 2000, "{ran} units of work for a budget of 1000");
    }

    #[test]
    fn the_depth_cap_reaches_the_resolution_on_the_base_tile() {
        // The base tile spans the coordinate box, 4·10⁶ mm: 48 halvings
        // bring it within a factor 1.5 of 10⁻⁸ mm, a depth of 40 left it
        // near 4·10⁻⁶ mm, and every depth is exact in a `u64` cell index.
        let side = 4.0e6 / (1u64 << MAX_SUBDIV_DEPTH) as f64;
        assert!(side < 1.5e-8, "{side}");
        assert!(4.0e6 / (1u64 << 40) as f64 > 1.0e-6);
        const { assert!(MAX_SUBDIV_DEPTH < u64::BITS) };
    }

    #[test]
    fn budgets_stop_at_max_work() {
        assert_eq!(Budget::new(u64::MAX), Budget::DEFAULT);
        assert_eq!(Budget::default().work(), MAX_WORK);
    }

    /// Item `i`'s steps: a pure function of `i`, of uneven cost, mixing
    /// precision steps of several levels with the rest.
    #[cfg(not(target_arch = "wasm32"))]
    fn uneven(i: u64) -> Vec<(Step, u64)> {
        let h = i.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40;
        (0..h % 5 + 1)
            .map(|k| {
                let step = match (h >> k) % 3 {
                    0 => W,
                    l => Step::Precision { level: l as u8 },
                };
                (step, (h >> (2 * k)) % 97 + 1)
            })
            .collect()
    }

    /// Passes over many items of uneven cost: a failure ends the first
    /// attempt, and by the budget, the budget runs out in it, in a retry,
    /// or never. The outcomes, the work used and the profiles are the
    /// same on 1 and 8 threads, and with the items run backwards.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn charging_is_the_same_on_any_number_of_threads() {
        use super::super::par::assert_deterministic;
        let items: Vec<u64> = (0..5000).collect();
        for budget in [50_000, 200_000, 400_000, 1_500_000, 2_000_000] {
            assert_deterministic(|| {
                let mut work = Work::new(&Budget::new(budget));
                let mut outcomes = Vec::new();
                for rung in 0..3u8 {
                    let pass = Pass {
                        stage: "uneven",
                        rung,
                    };
                    let fails = |i: u64| rung == 0 && i == 3001;
                    let outcome = work.pass(pass, &items, |&i, ledger| {
                        run(&uneven(i), fails(i), ledger).map(|n| (i, n))
                    });
                    let stop = matches!(outcome, Err(Halt::OutOfBudget(_)));
                    outcomes.push((outcome, work.used()));
                    if stop {
                        break;
                    }
                }
                outcomes
            });
        }
    }

    /// A record table filled by passes: each batch of keys not yet stored
    /// is one pass over them in key order, a unit per key, so asking for
    /// stored keys again charges nothing; a pass that runs out stores
    /// nothing.
    #[test]
    fn record_tables_are_filled_by_charged_passes() {
        use varde_decide::records::table::{FillError, Table};
        let decide = |work: &mut Work, t: &mut Table<u32, u32>, wanted: &[u32]| {
            t.fill(wanted.iter().copied(), |keys| {
                work.pass(PASS, keys, |k, ledger| {
                    ledger.charge(W, 1)?;
                    Ok::<_, Stop<()>>(**k + 100)
                })
            })
        };
        let mut work = Work::new(&Budget::new(6));
        let mut t = Table::new();
        decide(&mut work, &mut t, &[3, 1, 2, 3]).unwrap();
        assert_eq!(work.used(), 3);
        decide(&mut work, &mut t, &[1, 2, 3, 4]).unwrap();
        assert_eq!(work.used(), 4);
        assert_eq!(t.get(&4), Some(&104));
        let out = decide(&mut work, &mut t, &[5, 6, 7]);
        assert!(matches!(out, Err(FillError::Decide(Halt::OutOfBudget(_)))));
        assert_eq!(work.used(), 6);
        assert_eq!(t.len(), 4);

        // A batch that uses the budget up exactly is stored; stored keys
        // still read free; the next key runs out at its own pass.
        let mut work = Work::new(&Budget::new(3));
        let mut t = Table::new();
        decide(&mut work, &mut t, &[2, 3, 1]).unwrap();
        assert_eq!(work.used(), 3);
        decide(&mut work, &mut t, &[3, 1]).unwrap();
        let out = decide(&mut work, &mut t, &[1, 4]);
        assert!(matches!(out, Err(FillError::Decide(Halt::OutOfBudget(_)))));
        assert_eq!(work.used(), 3);
        assert_eq!(t.len(), 3);
    }

    /// Pass and item positions count past `2³²`: the pass counter is
    /// 64-bit, the item position the index into the pass's items.
    #[test]
    fn positions_count_past_32_bits() {
        let mut work = Work::new(&Budget::new(100));
        work.passes = u64::from(u32::MAX);
        work.pass(PASS, &[1], |&n, ledger| run(&[(W, n)], false, ledger))
            .unwrap();
        let halt = work
            .pass(PASS, &[1, 200], |&n, ledger| run(&[(W, n)], false, ledger))
            .unwrap_err();
        let (_, profile) = out_of_budget(halt);
        assert_eq!(
            profile.stopped.at,
            ItemAt {
                pass: 1 << 32,
                item: 1
            }
        );
        assert_eq!(work.passes, (1 << 32) + 1);
    }
}
