//! What an operation reports of its work when it runs out
//! ([`BudgetProfile`]), and what of the evidence attached to a result's
//! warnings its allowance decides ([`WarningEvidence`]).
//!
//! **The profile** is assembled from the budget's own counts
//! ([`Counts`]), never charged. Charging ([`Work`](crate::infra::budget))
//! counts only the key-order prefix of each pass, through the key that
//! decides it (the first to cross the budget, counted up to the crossing,
//! or the first to fail): the units by stage, by rung and by precision
//! level, the units of steps that are not precision steps apart, and the
//! [`HOTTEST`] items of the prefix by units, ties in key order (pass,
//! then position), each item's units in its one pass, never summed over
//! passes or rungs. Every count follows from the prefix alone, and items
//! after the deciding key, stopped early or run to the end, never reach
//! it; so the profile is the same on any number of threads, whether or
//! not they are stopped.
//!
//! **Warnings' evidence** is built after the outcome is fixed, from the
//! status alone (read, never changed) and its own allowance
//! ([`Allowance`]), never the operation's budget. Running out of the
//! allowance degrades the warnings after that point to their names
//! (kind and bound, in the status) and counts them; the warnings, their
//! bounds, their count, the approximate flag and the largest deviation
//! are the same whatever the allowance. Once built, it is kept on the
//! status ([`Status::attach_evidence`]), which still compares equal on
//! everything else ([`Status::same_outcome`]).

use varde_decide::records::keys::{Named, RecordKey};

use super::Allowance;
use super::failure::Evidence;
#[cfg(doc)]
use crate::infra::budget::{Halt, Work};
use crate::infra::budget::{Limit, Step};
use crate::infra::ops::{Status, Warning};

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

impl ItemAt {
    /// Its place in key order over the operation: pass, then position.
    fn key(&self) -> (u64, usize) {
        (self.pass, self.item)
    }
}

/// What names one item of a pass in a [`BudgetProfile`]: whatever its
/// pass knows of it, from which evidence names the input entities and
/// where they lie. Neither part is charged or changes an outcome.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ItemName {
    /// The key of the record the item decides, in a pass deciding
    /// records ([`Work::records`]): evidence names the item by its
    /// entities ([`ItemName::entities`]).
    pub record: Option<RecordKey>,
    /// A box `[lo, hi]` in world coordinates (mm) holding the item's
    /// geometry, when its pass knows one ([`Work::named`]).
    pub world: Option<[[f64; 3]; 2]>,
}

impl ItemName {
    /// The operand entities the item is about, by name: those its
    /// record's key holds, the first operand's first; none when its pass
    /// decides no record.
    pub fn entities(&self) -> Vec<Named> {
        self.record.map(|r| r.entities()).unwrap_or_default()
    }
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

impl Stopped {
    /// The precision level of the step it stopped at; `None` at a step
    /// that is not a precision step, or none.
    pub fn level(&self) -> Option<u8> {
        match self.step {
            Some(Step::Precision { level }) => Some(level),
            Some(Step::Work) | None => None,
        }
    }
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

/// How a [`BudgetProfile`]'s counts disagree with each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileDefect {
    /// The units of work and of precision steps don't sum to the units
    /// used.
    Steps,
    /// The units by stage don't sum to the units used.
    Stages,
    /// The units by rung don't sum to the units used.
    Rungs,
    /// A stage is listed twice.
    Stage,
    /// More than [`HOTTEST`] items listed, or one with more units than
    /// were used, or none.
    Hottest,
    /// The items listed aren't most first, ties in key order.
    Order,
    /// Two listed items are one.
    Twice,
}

impl BudgetProfile {
    /// Whether its counts agree: work and precision steps, stages and
    /// rungs each sum to the units used; the hottest items are at most
    /// [`HOTTEST`], each one item once with units it used, most first,
    /// ties in key order.
    pub fn check(&self) -> Result<(), ProfileDefect> {
        fn sum(it: impl IntoIterator<Item = u64>) -> Option<u64> {
            it.into_iter().try_fold(0u64, u64::checked_add)
        }
        let used = Some(self.used);
        if sum(std::iter::once(self.work).chain(self.precision.iter().copied())) != used {
            return Err(ProfileDefect::Steps);
        }
        if sum(self.stages.iter().map(|&(_, n)| n)) != used {
            return Err(ProfileDefect::Stages);
        }
        if sum(self.rungs.iter().copied()) != used {
            return Err(ProfileDefect::Rungs);
        }
        for (i, (stage, _)) in self.stages.iter().enumerate() {
            if self.stages[..i].iter().any(|(s, _)| s == stage) {
                return Err(ProfileDefect::Stage);
            }
        }
        if self.hottest.len() > HOTTEST
            || self.hottest.iter().any(|&(_, n)| n == 0 || n > self.used)
        {
            return Err(ProfileDefect::Hottest);
        }
        for (i, (a, _)) in self.hottest.iter().enumerate() {
            if self.hottest[..i].iter().any(|(b, _)| b.key() == a.key()) {
                return Err(ProfileDefect::Twice);
            }
        }
        for pair in self.hottest.windows(2) {
            let [(a, m), (b, n)] = pair else {
                unreachable!("windows of two")
            };
            if m < n || (m == n && a.key() > b.key()) {
                return Err(ProfileDefect::Order);
            }
        }
        Ok(())
    }
}

/// The budget's counts of where its work went, over the key-order
/// prefix of each pass: what a [`BudgetProfile`] is assembled from. Only
/// charging adds to them, in key order.
#[derive(Debug, Default)]
pub(crate) struct Counts {
    work: u64,
    precision: Vec<u64>,
    stages: Vec<(&'static str, u64)>,
    rungs: Vec<u64>,
    hottest: Vec<(ItemAt, u64)>,
}

impl Counts {
    /// Adds the first `units` of the charges of item `at` of a pass of
    /// stage `stage` at rung `rung`, whose steps were `steps` in its order
    /// (consecutive steps of a kind merged). Items come in key order.
    pub(crate) fn add(
        &mut self,
        stage: &'static str,
        rung: u8,
        at: ItemAt,
        steps: &[(Step, u64)],
        units: u64,
    ) {
        if units == 0 {
            return;
        }
        let mut left = units;
        for &(step, run) in steps {
            let take = run.min(left);
            match step {
                _ if take == 0 => {}
                Step::Work => self.work = self.work.saturating_add(take),
                Step::Precision { level } => {
                    let level = usize::from(level);
                    if self.precision.len() <= level {
                        self.precision.resize(level + 1, 0);
                    }
                    self.precision[level] = self.precision[level].saturating_add(take);
                }
            }
            left -= take;
            if left == 0 {
                break;
            }
        }
        match self.stages.iter_mut().find(|(s, _)| *s == stage) {
            Some((_, n)) => *n = n.saturating_add(units),
            None => self.stages.push((stage, units)),
        }
        let rung = usize::from(rung);
        if self.rungs.len() <= rung {
            self.rungs.resize(rung + 1, 0);
        }
        self.rungs[rung] = self.rungs[rung].saturating_add(units);
        // Most units first, ties in key order; items come in key order,
        // so a new one goes after those with as many units.
        let place = self.hottest.partition_point(|&(_, n)| n >= units);
        if place < HOTTEST {
            self.hottest.insert(place, (at, units));
            self.hottest.truncate(HOTTEST);
        }
    }

    /// The profile of an operation that reached `limit` having used
    /// `used` units, stopped as `stopped` says.
    pub(crate) fn profile(&self, limit: Limit, used: u64, stopped: Stopped) -> BudgetProfile {
        let profile = BudgetProfile {
            limit,
            used,
            work: self.work,
            precision: self.precision.clone(),
            stages: self.stages.clone(),
            rungs: self.rungs.clone(),
            stopped,
            hottest: self.hottest.clone(),
        };
        debug_assert_eq!(profile.check(), Ok(()), "{profile:?}");
        profile
    }
}

/// The evidence attached to a status's listed warnings
/// ([`Status::warnings`]), one entry each, in their order: built while
/// the allowance lasts, then degraded to the warning's name (its kind
/// and bound, in the status) and counted in
/// [`truncated`](Self::truncated). Never changes the status.
#[derive(Debug, Clone, PartialEq)]
pub struct WarningEvidence<E = Evidence> {
    /// Per listed warning, its evidence, or `None` where it is degraded
    /// to its name.
    pub items: Vec<Option<E>>,
    /// How many were degraded.
    pub truncated: u32,
}

impl<E> WarningEvidence<E> {
    /// Builds the evidence of `status`'s listed warnings in their order
    /// from `allowance`: for each, `cost` gives the units its evidence
    /// takes, charged before `build` makes it. From the first one the
    /// allowance can't afford on, every warning is degraded to its name.
    /// `status` is only read: its warnings, their bounds and count are
    /// the same whatever the allowance.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the operations' warnings call it as they land")
    )]
    pub(crate) fn build(
        status: &Status,
        allowance: &mut Allowance,
        mut cost: impl FnMut(usize, &Warning) -> u64,
        mut build: impl FnMut(usize, &Warning) -> E,
    ) -> WarningEvidence<E> {
        let mut items = Vec::with_capacity(status.warnings().len());
        let mut truncated = 0u32;
        for (i, w) in status.warnings().iter().enumerate() {
            if allowance.afford(cost(i, w)) {
                items.push(Some(build(i, w)));
            } else {
                items.push(None);
                truncated = truncated.saturating_add(1);
            }
        }
        WarningEvidence { items, truncated }
    }
}

#[cfg(test)]
mod tests;
