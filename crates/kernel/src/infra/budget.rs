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
//! key order as far as they go. Each item still running then has at most
//! the budget left after that prefix, so its share tightens to it; and
//! once the prefix decides the pass the items after its deciding key stop
//! at their next charge: they can't change the outcome, and without
//! stopping them a pass of `n` items could do `n` times the budget's
//! work. Neither changes the outcome, the work used or the profile: an
//! item stopped early either comes after the deciding key or is past its
//! room already, and its ledger up to there is the same.
//!
//! Each ledger records its item's steps in order, each a [`Step`]: a
//! precision step (a computation repeated at a higher precision) or any
//! other. Running out is [`Cause::Precision`] when the unit the budget
//! ran out at belongs to a precision step, [`Cause::Work`] otherwise.
//! Reaching a fixed limit ([`Cap`]) is its own failure, never a budget
//! crossing.
//!
//! A pass deciding records ([`Work::records`]) names each item by its
//! record's key ([`ItemName::record`]), so running out at a record names
//! the operand entities its key holds, not only a pass and a position; a
//! pass knowing where its items lie names them by a world box too
//! ([`Work::named`]).
//!
//! The booleans of a batch (a union of many solids) run as nodes of one
//! tree in a fixed post-order on one budget: run in that order through
//! one [`Work`], their passes come one after another, so the work used,
//! the cause and the profile follow the rule above over node, then rung,
//! pass and key, and the batch fails at the first failure in that order.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use varde_decide::num::schedule;
use varde_decide::records::keys::{Key, RecordKey};
use varde_decide::records::table::Fresh;

use super::error::{Cause, KernelError};
use super::par::par_map_indexed;

/// The most work one operation may do, in the units of [`Budget`]: about
/// two seconds of work on one thread of the reference machine, less on
/// several. A unit is about half a microsecond, each computation kind
/// charged its own units from the unit table ([`schedule::UNITS`]); the
/// calibration benchmark (`benches/calibrate.rs`) measures a seam pass
/// that uses it up at 2.1 s on one thread, and fails past four times the
/// two seconds.
pub const MAX_WORK: u64 = 1 << 22;

pub use varde_decide::num::schedule::MAX_SUBDIV_DEPTH;

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
pub(crate) const LEVELS: u8 = schedule::LEVELS as u8;

/// The step of one computation at precision level `level`: ordinary work
/// at the first level, a precision step above it.
pub(crate) fn level_step(level: u8) -> Step {
    match level {
        0 => Step::Work,
        level => Step::Precision { level },
    }
}

/// The units of one seam-bound cell at precision level `level`, from the
/// unit table ([`schedule::UNITS`]).
pub(crate) fn seam_cell_units(level: u8) -> u64 {
    schedule::UNITS.seam_cell[usize::from(level)]
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
/// key-sorted items, and what names it. Positions follow from the keys,
/// so they are the same on every run; they number items, the name says
/// which input they are about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemAt {
    /// The pass, counted from 0 over the operation.
    pub pass: u64,
    /// The item's position in the pass (as [`Halt::Failed`]'s).
    pub item: usize,
    /// What its pass knows of it, for evidence.
    pub name: ItemName,
}

/// What names one item of a pass in a [`BudgetProfile`]: whatever its
/// pass knows of it, from which evidence names the input entities and
/// where they lie. Neither part is charged or changes an outcome.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ItemName {
    /// The key of the record the item decides, in a pass deciding records
    /// ([`Work::records`]): evidence names the item by its entities
    /// ([`RecordKey::entities`]).
    pub record: Option<RecordKey>,
    /// A box `[lo, hi]` in world coordinates (mm) holding the item's
    /// geometry, when its pass knows one ([`Work::named`]).
    pub world: Option<[[f64; 3]; 2]>,
}

/// Where an operation stopped: the item that decided its pass, and the
/// step of its ledger the budget ran out at, or its last step on reaching
/// another limit (`None` if it had charged none).
#[derive(Debug, Clone, Copy, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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
    /// (ties in pass and item order): each item's units in its one pass,
    /// never summed over passes or rungs.
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
/// share: the budget left when the pass started, tightened to what is
/// left after the finished items before it ([`Frontier`]).
#[derive(Debug)]
pub(crate) struct Ledger<'p> {
    total: u64,
    /// Runs of steps of one kind: consecutive steps of a kind merged.
    steps: Vec<(Step, u64)>,
    /// The item's position in its pass.
    item: usize,
    frontier: &'p Frontier,
}

/// An item went past its share, the budget left at its pass's start or
/// after the finished items before it, so the pass runs out at it or
/// before it; or the pass is decided at an
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
    fn new(item: usize, frontier: &'p Frontier) -> Ledger<'p> {
        Ledger {
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
        if self.total > self.frontier.share() || self.frontier.decided_before(self.item) {
            Err(Exhausted(()))
        } else {
            Ok(())
        }
    }

    /// The item's steps so far, in its order, consecutive steps of a kind
    /// merged with their units: its escalation sequence, for tests.
    #[cfg(test)]
    pub(crate) fn steps(&self) -> &[(Step, u64)] {
        &self.steps
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
/// items whose outcome doesn't count; and it tightens the share of the
/// items still running to the budget left after the finished prefix,
/// never below any one's room, so it stops only items past their room.
#[derive(Debug)]
pub(crate) struct Frontier {
    /// The budget left at the pass start.
    left: u64,
    /// Whether it stops items early (tests compare runs without).
    stops: bool,
    /// The share of every item still running: `left` less the units of
    /// the finished prefix.
    share: AtomicU64,
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

    fn new(left: u64, items: usize, stops: bool) -> Frontier {
        Frontier {
            left,
            stops,
            share: AtomicU64::new(left),
            finished: (0..items).map(|_| AtomicU64::new(Self::RUNNING)).collect(),
            prefix: Mutex::new((0, 0)),
            decided: AtomicUsize::new(usize::MAX),
        }
    }

    /// Whether the pass is decided at a key before `item`'s.
    fn decided_before(&self, item: usize) -> bool {
        self.decided.load(Ordering::Relaxed) < item
    }

    /// The share of an item still running. The items before the first
    /// one not summed have all finished, so a running item comes at or
    /// after it, and its room is at most what the summed ones left.
    fn share(&self) -> u64 {
        self.share.load(Ordering::Relaxed)
    }

    /// Records that `item` finished, having charged `total`, with a result
    /// or not, and sums the finished prefix on as far as it goes: up to the
    /// first item that runs out or gives no result, which decides the
    /// pass, as in [`Work::pass`].
    fn finish(&self, item: usize, total: u64, result: bool) {
        if !self.stops {
            return;
        }
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
        self.share.store(self.left - *sum, Ordering::Relaxed);
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
    /// at a charge once an earlier key decides the pass or it is past the
    /// budget the finished items before it left.
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
        self.run(pass, items, |_| ItemName::default(), f)
    }

    /// [`Work::pass`] over a batch of a record table's keys, deciding
    /// each: charged as any pass, and each item named by its record's key
    /// in the profile ([`ItemName::record`]), so running out at a record
    /// names its key's entities.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the operations' record stages call it as they land"
        )
    )]
    pub(crate) fn records<K, R, E, F>(
        &mut self,
        pass: Pass,
        keys: &[Fresh<K>],
        f: F,
    ) -> Result<Vec<R>, Halt<E>>
    where
        K: Key,
        R: Send,
        E: Send,
        F: Fn(&Fresh<K>, &mut Ledger<'_>) -> Result<R, Stop<E>> + Sync + Send,
    {
        self.run(
            pass,
            keys,
            |k| ItemName {
                record: Some(k.record()),
                world: None,
            },
            f,
        )
    }

    /// [`Work::pass`], naming each item in the profile by `name`: what
    /// the pass knows of it, its record and where it lies. The names are
    /// read only for the profile, never charged.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the operations' passes that know their items' boxes call it as they land"
        )
    )]
    pub(crate) fn named<T, R, E, F>(
        &mut self,
        pass: Pass,
        items: &[T],
        name: impl Fn(&T) -> ItemName,
        f: F,
    ) -> Result<Vec<R>, Halt<E>>
    where
        T: Sync,
        R: Send,
        E: Send,
        F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<E>> + Sync + Send,
    {
        self.run(pass, items, name, f)
    }

    /// [`Work::pass`], naming each item by `name`.
    fn run<T, R, E, F>(
        &mut self,
        pass: Pass,
        items: &[T],
        name: impl Fn(&T) -> ItemName,
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
        let frontier = Frontier::new(left, items.len(), early_stop());
        let ran = par_map_indexed(items, |i, item| {
            let mut ledger = Ledger::new(i, &frontier);
            let result = f(item, &mut ledger);
            frontier.finish(i, ledger.total, result.is_ok());
            (ledger, result)
        });
        let mut out = Vec::with_capacity(ran.len());
        let mut sum = 0u64;
        for (i, ((ledger, result), item)) in ran.into_iter().zip(items).enumerate() {
            let at = ItemAt {
                pass: index,
                item: i,
                name: name(item),
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

/// Whether a pass stops its items early ([`Frontier`]): always, but tests
/// run passes without it to show it changes nothing
/// (`without_early_stop`).
#[cfg(not(test))]
fn early_stop() -> bool {
    true
}

#[cfg(test)]
thread_local! {
    static EARLY_STOP: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

#[cfg(test)]
fn early_stop() -> bool {
    EARLY_STOP.get()
}

/// `f` run with the passes it starts on this thread not stopping their
/// items early: each item runs until it is done or past the budget left
/// at its pass's start.
#[cfg(test)]
pub(crate) fn without_early_stop<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            EARLY_STOP.set(self.0);
        }
    }
    let _restore = Restore(EARLY_STOP.replace(false));
    f()
}

/// Runs `f` as [`assert_deterministic`] does, with its passes stopping
/// items early and without ([`without_early_stop`]), and asserts all the
/// results are the same to the bit: outcomes, work used and profiles.
/// Returns the first.
///
/// [`assert_deterministic`]: super::par::assert_deterministic
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn assert_early_stop_changes_nothing<R: Send + std::fmt::Debug>(
    f: impl Fn() -> R + Sync,
) -> R {
    use super::par::assert_deterministic;
    let stopping = assert_deterministic(&f);
    let running = assert_deterministic(|| without_early_stop(&f));
    assert_eq!(
        format!("{stopping:?}"),
        format!("{running:?}"),
        "stopping items early changes the outcome"
    );
    stopping
}

#[cfg(test)]
mod tests {
    use super::*;
    use varde_decide::ids::{Entity, VertId};
    use varde_decide::records::keys::{Named, Operand, Point, Query, VvKey};
    use varde_decide::records::table::{FillError, Table};

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
        assert_eq!(
            profile.stopped.at,
            ItemAt {
                pass: 0,
                item: 2,
                name: ItemName::default(),
            }
        );
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

    /// How an item of [`passes_over_uneven_items`] ends, when it isn't
    /// done or out of budget.
    #[cfg(not(target_arch = "wasm32"))]
    #[derive(Clone, Copy)]
    enum End {
        /// It fails.
        Fails,
        /// It reaches a fixed limit.
        Caps,
        /// Its value can't be stored.
        Unstorable,
    }

    /// Up to three attempts over 5000 items of uneven cost: item 3001 of
    /// the first ends as `end` says, the attempt after a failure runs on
    /// what is left. Gives each attempt's outcome and the work used after
    /// it.
    #[cfg(not(target_arch = "wasm32"))]
    fn passes_over_uneven_items(budget: u64, end: End) -> Vec<(Result<usize, KernelError>, u64)> {
        use crate::Internal;
        use varde_decide::num::storage::Unrepresentable;
        let items: Vec<u64> = (0..5000).collect();
        let mut work = Work::new(&Budget::new(budget));
        let mut outcomes = Vec::new();
        for rung in 0..3u8 {
            let pass = Pass {
                stage: "uneven",
                rung,
            };
            let outcome = work.pass(pass, &items, |&i, ledger| {
                for (step, units) in uneven(i) {
                    ledger.charge(step, units)?;
                }
                match end {
                    _ if rung > 0 || i != 3001 => Ok(i),
                    End::Fails => Err(Stop::Failed(KernelError::Internal(Internal::Inconsistent))),
                    End::Caps => Err(Stop::Cap(Cap::SubdivisionDepth)),
                    End::Unstorable => Err(Stop::Failed(
                        Unrepresentable::Tolerance {
                            eps: 0.0,
                            eps_min: 1.0,
                        }
                        .into(),
                    )),
                }
            });
            let done = matches!(outcome, Ok(_) | Err(Halt::OutOfBudget(_)));
            outcomes.push((
                outcome.map(|v| v.len()).map_err(KernelError::from),
                work.used(),
            ));
            if done {
                break;
            }
        }
        outcomes
    }

    /// Passes over many items of uneven cost: an item failing, reaching a
    /// fixed limit or giving a value that can't be stored ends the first
    /// attempt, and by the budget, the budget runs out in it, in a retry,
    /// or never. The outcomes, the work used and the profiles are the
    /// same on 1 and 8 threads, with the items run backwards, and with
    /// the items after the deciding key stopped early or run on.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn charging_is_the_same_on_any_number_of_threads_and_without_early_stop() {
        use crate::Internal;
        for end in [End::Fails, End::Caps, End::Unstorable] {
            for budget in [50_000, 200_000, 400_000, 1_500_000, 2_000_000] {
                let out =
                    assert_early_stop_changes_nothing(|| passes_over_uneven_items(budget, end));
                let (last, _) = out.last().expect("an attempt");
                // A large budget reaches item 3001 and ends there.
                if budget == 2_000_000 {
                    let want = match end {
                        End::Fails => KernelError::Internal(Internal::Inconsistent),
                        End::Caps => {
                            assert!(matches!(
                                last,
                                Err(KernelError::OutOfBudget {
                                    cause: Cause::SizeLimit,
                                    ..
                                })
                            ));
                            continue;
                        }
                        End::Unstorable => KernelError::Unrepresentable,
                    };
                    assert_eq!(out[0].0, Err(want));
                }
            }
        }
    }

    /// A pass ending in a fixed limit or a value that can't be stored
    /// charges only the keys through the deciding one, however much the
    /// items after it ran: the next attempt runs on the rest.
    #[test]
    fn a_pass_ending_at_a_cap_or_unstorable_value_charges_its_prefix() {
        use varde_decide::num::storage::Unrepresentable;
        // Items of 2, 3, 4, 5, 6, 7 units, the third ending, then the
        // costly ones after it.
        let items = [2u64, 3, 4, 50, 60, 70];
        for cap in [false, true] {
            let mut work = Work::new(&Budget::new(100));
            let halt = work
                .pass(PASS, &items, |&n, ledger| {
                    ledger.charge(W, n)?;
                    match n {
                        4 if cap => Err(Stop::Cap(Cap::SubdivisionDepth)),
                        4 => Err(Stop::Failed(Unrepresentable::OutOfRange {
                            value: f64::INFINITY,
                            magnitude: f64::INFINITY,
                        })),
                        n => Ok(n),
                    }
                })
                .unwrap_err();
            assert_eq!(work.used(), 9);
            match KernelError::from(halt.clone()) {
                KernelError::OutOfBudget { cause, profile } => {
                    assert!(cap);
                    assert_eq!(
                        (cause, profile.used, profile.stopped.at.item),
                        (Cause::SizeLimit, 9, 2)
                    );
                    assert_eq!(profile.stages, vec![("test", 9)]);
                }
                KernelError::Unrepresentable => {
                    assert!(!cap);
                    assert!(matches!(halt, Halt::Failed { item: 2, .. }));
                }
                other => panic!("{other:?}"),
            }
            assert_eq!(work.left(), 91);
        }
    }

    /// An item still running gets the share left after the finished items
    /// before it: on one thread, after an item of 600 units on a budget
    /// of 1000, one charging without end stops at its 401st unit, past its
    /// room, and not at the 1001st; without early stop it runs to the
    /// budget left at the pass's start. The outcome is the same.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_share_tightens_to_what_the_items_before_it_left() {
        use super::super::par::on_threads;
        use std::sync::atomic::{AtomicU64, Ordering};
        let ran = |stops: bool| {
            let done = AtomicU64::new(0);
            let pass = || {
                let mut work = Work::new(&Budget::new(1000));
                let out = work.pass(PASS, &[600u64, 0], |&n, ledger| -> Result<(), Stop<()>> {
                    if n > 0 {
                        return Ok(ledger.charge(W, n)?);
                    }
                    loop {
                        ledger.charge(W, 1)?;
                        done.fetch_add(1, Ordering::Relaxed);
                    }
                });
                out_of_budget(out.unwrap_err())
            };
            let out = on_threads(1, || {
                if stops {
                    pass()
                } else {
                    without_early_stop(pass)
                }
            });
            (out, done.into_inner())
        };
        let (stopping, n) = ran(true);
        assert_eq!(n, 400, "units charged before the one past the room");
        let (running, n) = ran(false);
        assert_eq!(n, 1000);
        assert_eq!(stopping, running);
        assert_eq!((stopping.1.used, stopping.1.stopped.at.item), (1000, 1));
    }

    /// A pass naming its items gives the profile each one's name: its
    /// record and its world box, for the item it stopped at and the
    /// hottest ones. Names are never charged.
    #[test]
    fn a_named_pass_names_its_items_in_the_profile() {
        let boxed = |n: &u64| ItemName {
            record: Some(RecordKey::Vv(vv(*n as u32))),
            world: Some([[0.0, 0.0, *n as f64], [1.0, 1.0, *n as f64 + 1.0]]),
        };
        let mut work = Work::new(&Budget::new(10));
        let halt = work
            .named(PASS, &[3u64, 4, 5], boxed, |&n, ledger| {
                run(&[(W, n)], false, ledger)
            })
            .unwrap_err();
        let (_, profile) = out_of_budget(halt);
        assert_eq!(profile.stopped.at.name, boxed(&5));
        let names: Vec<(ItemName, u64)> = profile
            .hottest
            .iter()
            .map(|(at, n)| (at.name, *n))
            .collect();
        assert_eq!(names, vec![(boxed(&4), 4), (boxed(&3), 3), (boxed(&5), 3)]);
        assert_eq!(work.used(), 10);
    }

    /// The booleans of a batch, run in post-order on one budget: each
    /// node's passes count on from the last node's, a node's retry runs
    /// on what is left, and the node running out is named by its pass;
    /// the hottest items tie in node, then pass, then key order, each
    /// counted in its one pass.
    #[test]
    fn batch_nodes_in_post_order_share_one_budget() {
        let mut work = Work::new(&Budget::new(30));
        let node = |work: &mut Work, rung: u8, items: &[(u64, bool)]| {
            let pass = Pass {
                stage: "node",
                rung,
            };
            work.pass(pass, items, |&(n, fail), ledger| {
                run(&[(W, n)], fail, ledger)
            })
        };
        // Node 0 in one pass; node 1 fails at its second item, retried on
        // the rest; node 2 runs out.
        assert!(node(&mut work, 0, &[(4, false), (5, false)]).is_ok());
        assert!(matches!(
            node(&mut work, 0, &[(5, false), (4, true), (9, false)]),
            Err(Halt::Failed { item: 1, .. })
        ));
        assert_eq!(work.used(), 18);
        assert!(node(&mut work, 1, &[(5, false)]).is_ok());
        let (cause, profile) =
            out_of_budget(node(&mut work, 0, &[(4, false), (5, false)]).unwrap_err());
        assert_eq!(cause, Cause::Work);
        assert_eq!((profile.used, profile.rungs.clone()), (30, vec![25, 5]));
        assert_eq!((profile.stopped.at.pass, profile.stopped.at.item), (3, 1));
        let hottest: Vec<(u64, usize, u64)> = profile
            .hottest
            .iter()
            .map(|(at, n)| (at.pass, at.item, *n))
            .collect();
        assert_eq!(
            hottest,
            vec![
                (0, 1, 5),
                (1, 0, 5),
                (2, 0, 5),
                (0, 0, 4),
                (1, 1, 4),
                (3, 0, 4),
                (3, 1, 3)
            ]
        );
    }

    /// A test record key: point `i` of `A` against vertex 0 of `B`.
    fn vv(i: u32) -> VvKey {
        VvKey::of(Point::vert(Operand::A, VertId(i)), VertId(0)).key
    }

    /// Decides the keys of `wanted` not yet stored or in flight as one
    /// record pass, a unit per key, and stores them.
    fn decide(
        work: &mut Work,
        t: &mut Table<VvKey, u32>,
        wanted: &[u32],
    ) -> Result<(), FillError<VvKey, Halt<()>>> {
        t.fill(wanted.iter().map(|&i| vv(i)), |keys| {
            work.records(PASS, keys, |k, ledger| {
                ledger.charge(W, 1)?;
                Ok::<_, Stop<()>>(id(k) + 100)
            })
        })
    }

    /// The point id of a test key.
    fn id(k: &VvKey) -> u32 {
        match k.a {
            Query::Vert(v) => v.0,
            _ => unreachable!("test keys are vertex pairs"),
        }
    }

    /// A record table filled by passes: each batch of keys not yet stored
    /// is one pass over them in key order, a unit per key, so asking for
    /// stored keys again charges nothing; a pass that runs out stores
    /// nothing.
    #[test]
    fn record_tables_are_filled_by_charged_passes() {
        let mut work = Work::new(&Budget::new(6));
        let mut t = Table::new();
        decide(&mut work, &mut t, &[3, 1, 2, 3]).unwrap();
        assert_eq!(work.used(), 3);
        decide(&mut work, &mut t, &[1, 2, 3, 4]).unwrap();
        assert_eq!(work.used(), 4);
        assert_eq!(t.get(&vv(4)), Some(&104));
        let out = decide(&mut work, &mut t, &[5, 6, 7]);
        assert!(matches!(out, Err(FillError::Decide(Halt::OutOfBudget(_)))));
        assert_eq!(work.used(), 6);
        assert_eq!(t.len(), 4);
        assert!(!t.in_flight(&vv(5)));

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

    /// Two batches requesting one key, made before either is stored: the
    /// key is decided and charged once, in the first; the second waits
    /// for it. The work, the answers and the profile of the pass that
    /// runs out are the same on 1 and 8 threads.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn two_batches_requesting_one_key_decide_and_charge_it_once() {
        use super::super::par::assert_deterministic;
        use std::sync::atomic::{AtomicU64, Ordering};
        let unit = |k: &Fresh<VvKey>, ledger: &mut Ledger, n: &AtomicU64| {
            ledger.charge(W, 1 + u64::from(id(k) % 3))?;
            n.fetch_add(1, Ordering::Relaxed);
            Ok::<_, Stop<()>>(id(k) * 2)
        };
        for budget in [1000, 40] {
            assert_deterministic(|| {
                let decided = AtomicU64::new(0);
                let mut work = Work::new(&Budget::new(budget));
                let mut t: Table<VvKey, u32> = Table::new();
                let first = t.missing((0..20).map(vv)).unwrap();
                let second = t.missing((10..30).map(vv)).unwrap();
                assert_eq!(second.waits(), (10..20).map(vv).collect::<Vec<_>>());
                let a = work.records(PASS, first.keys(), |k, l| unit(k, l, &decided));
                let b = work.records(PASS, second.keys(), |k, l| unit(k, l, &decided));
                let mut out = Vec::new();
                for (batch, values) in [(second, b), (first, a)] {
                    match values {
                        Ok(v) => t.insert(batch, v).unwrap(),
                        Err(Halt::OutOfBudget(KernelError::OutOfBudget { profile, .. })) => {
                            out.push(profile.stopped.at);
                        }
                        Err(other) => panic!("{other:?}"),
                    }
                }
                let stored: Vec<(VvKey, u32)> = t.iter().map(|(k, v)| (*k, *v)).collect();
                // Items past the key a pass runs out at may run on more
                // threads; within the budget each key ran once.
                let decided = decided.into_inner();
                assert!(!out.is_empty() || decided == 30, "{decided} items run");
                (work.used(), stored, out)
            });
        }
        let mut work = Work::new(&Budget::new(1000));
        let mut t: Table<VvKey, u32> = Table::new();
        let first = t.missing((0..20).map(vv)).unwrap();
        let second = t.missing((10..30).map(vv)).unwrap();
        let b = work.records(PASS, second.keys(), |k, l| unit(k, l, &AtomicU64::new(0)));
        t.insert(second, b.unwrap()).unwrap();
        let a = work.records(PASS, first.keys(), |k, l| unit(k, l, &AtomicU64::new(0)));
        t.insert(first, a.unwrap()).unwrap();
        // 30 keys at 1, 2 or 3 units each, every key once.
        let once: u64 = (0..30u64).map(|i| 1 + i % 3).sum();
        assert_eq!(work.used(), once);
        assert_eq!(t.len(), 30);
        assert!(t.missing((0..30).map(vv)).unwrap().is_empty());
    }

    /// Running out at a record names the record by its key, and so the
    /// operand entities the key holds; the hottest items too.
    #[test]
    fn running_out_at_a_record_names_its_key_s_entities() {
        let mut work = Work::new(&Budget::new(2));
        let mut t = Table::new();
        let out = decide(&mut work, &mut t, &[7, 8, 9]);
        let Err(FillError::Decide(Halt::OutOfBudget(KernelError::OutOfBudget { profile, .. }))) =
            out
        else {
            panic!("{out:?}");
        };
        let at = profile.stopped.at;
        assert_eq!((at.item, at.name.record), (2, Some(RecordKey::Vv(vv(9)))));
        let named = |op, v| Named {
            operand: op,
            entity: Entity::Vert(VertId(v)),
        };
        assert_eq!(
            at.name.record.map(|r| r.entities()),
            Some(vec![named(Operand::A, 9), named(Operand::B, 0)])
        );
        assert!(
            profile
                .hottest
                .iter()
                .all(|(at, _)| matches!(at.name.record, Some(RecordKey::Vv(_))))
        );
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
                item: 1,
                name: ItemName::default(),
            }
        );
        assert_eq!(work.passes, (1 << 32) + 1);
    }
}
