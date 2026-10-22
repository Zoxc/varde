//! What happens when a cluster's snap fails: one routing of every
//! failure, halving the gap bound `b_K` only where the failure depends on
//! it, and stopping where halving no longer helps.
//!
//! **Failures.** A cluster fails at one of the snap's own certificates
//! ([`Failed`], [`Cert`]): no target the snapped sheet is a graph over
//! (its graph or curvature margin), a rim not clear on its level-set part
//! (the box pieces, the only ones `b_K` moves), a wall thinner than `b_K`,
//! or the rewrite's validity (its local check, embedding, a snapped
//! edge's fit, a join refit). Or a later check names it ([`Signal`]): the
//! zone's cut-point check or a zero-area region in it, naming rim pieces
//! on the level-set part, or only pinned elements (rim pieces along a
//! target or a snapped-side edge, anchors, branch vertices, bracket ends,
//! corner-rule vertices, junctions, inner and snapped pieces), which no
//! value of `b_K` moves. A certificate still open at a fixed limit, or the
//! budget spent, is [`Outcome::Exhausted`].
//!
//! **Routing** ([`route`], [`route_signal`]), in this order:
//! 1. still open at a limit, or the budget spent: out of budget (the
//!    caller's error, passed on);
//! 2. a validity failure in a window holding a corner-rule placement or a
//!    bracket end whose on-radius is its bands' sum, above `b_K`
//!    ([`Failed::pins`]): a rule gap at once ([`Gap::Pin`]); halving
//!    can't move it;
//! 3. the stop: the failure is the same certificate kind and key as the
//!    one that led to the halving before it, with no strict improvement of
//!    its certified quantity ([`Gap::Stop`]); for a later check, the same
//!    check failing again at the same piece (by the caller's geometric
//!    locator, never a piece number), at the same `b_K`, at rung 2 or
//!    later, with no key it read moved since ([`Rung::unchanged`]);
//! 4. halving would take `b_K` below the storage floor [`eps_min`] at the
//!    cluster's coordinate magnitude ([`Route::Floor`]: unrepresentable,
//!    unless a collapse repair, not done here, succeeds first);
//! 5. a certificate failure: halve; a later check: no halving of its own.
//!    The rim clearance of the level-set pieces it names is re-run
//!    ([`Resnap::clearance`]); failing, that failure is routed as a
//!    certificate's (and halves, with its own stop and floor, however many
//!    rungs); clear, or naming only pinned elements, the ladder goes on to
//!    its next rung ([`Route::Rung`]: precision and the tie band), up to
//!    its last, [`K_MAX`], where the failure ends as out of precision if a
//!    near-tie moved ([`Route::Precision`]), else a rule gap
//!    ([`Gap::Last`]).
//!
//! So an incidence on the pinned part of the rim never reaches a halving:
//! its failure is decided by rules or ends as a rule gap.
//!
//! **Abutment first.** Before anything else, the first failure of a
//! cluster whose leaves abut leaves of the other kind (a recertified leaf
//! of one kind meeting only windows of the other's,
//! [`super::stale::Abutment`]) recertifies the abutting leaves as Simple
//! ([`Resnap::simple`]), once, and retries at the same `b_K`; a
//! transversal face certifies Simple, its arc reaches the snapped edge,
//! and the cluster may dissolve.
//!
//! **Halving.** `b_K` is halved (exactly: a power of two) and the cluster
//! re-run at it ([`Resnap::attempt`]: its leaves recertified at the new
//! bound, those no longer passing split, then the snap). A true tangency
//! leaves a smaller cluster that passes, a spurious one dissolves; each
//! halving is charged.
//!
//! **Rule gaps** ([`RuleGap`]): failures no halving, precision or tie
//! band settles, the kernel's own known gap, given with what led to it
//! ([`Event`]): the bounds tried, each failure with its kind, key and
//! quantity, or its piece per rung. It is the kernel's
//! [`Internal::Inconsistent`], of the cluster class, until the error's
//! evidence names it.
//!
//! **Work.** Each halving is one item of a pass charged a unit; the
//! retries are the caller's, charged by it.

use std::fmt::Debug;

use super::corner::r_on;
use super::stale::Abutment;
use crate::infra::budget::{Halt, Pass, Step, Stop, Work};
use crate::infra::error::{Internal, KernelError};
pub use varde_decide::num::storage::eps_min;

/// The escalation ladder's last rung: a check still failing there ends
/// the ladder.
pub const K_MAX: u8 = 4;

/// The kind of a snap certificate that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cert {
    /// No target the snapped sheet is a certified graph over: its graph
    /// margin.
    Graph,
    /// No target the snapped sheet is a certified graph over: its
    /// curvature margin (`b_K` times the curvature).
    Curvature,
    /// The rim not clear on its level-set part (its box pieces).
    Rim,
    /// A wall thinner than `b_K`.
    ThinWall,
    /// The rewrite's validity: its local check, embedding, a snapped
    /// edge's fit or a join refit.
    Validity,
}

/// A snap certificate certified failing (not merely still open).
#[derive(Clone, Debug, PartialEq)]
pub struct Failed<K> {
    /// Its kind.
    pub cert: Cert,
    /// Which certificate of that kind, by the caller's geometric key.
    pub key: K,
    /// Its certified quantity (margin, clearance, gap or thickness),
    /// larger nearer to passing; finite.
    pub quantity: f64,
    /// For [`Cert::Validity`]: the bands `[β_x, β_y]` of each corner-rule
    /// placement and bracket end in the failing window (its on-radius is
    /// `max(b_K, β_x + β_y)`, the sum rounded up). Read for no other kind.
    pub pins: Vec<[f64; 2]>,
}

/// How a cluster's attempt ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome<K> {
    /// Every certificate passed.
    Pass,
    /// A certificate certified failing: the first, in the snap's order.
    Fail(Failed<K>),
    /// A certificate still open at a fixed limit, or the budget spent:
    /// [`KernelError::OutOfBudget`].
    Exhausted(KernelError),
}

/// The check, after the snap, that names a cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Check {
    /// The zone's cut-point check: crossings counted along a piece against
    /// its sides' labels.
    CutPoints,
    /// A zero-area region, or a collapse, in the zone.
    ZeroArea,
}

/// A later check failing in a cluster's zone: a signal of the escalation
/// ladder.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal<L> {
    /// The check.
    pub check: Check,
    /// The rim pieces on the level-set part it names, by their positions
    /// in the cluster's rim. Empty when it names only pinned elements.
    pub level_set: Vec<u32>,
    /// The failing piece, located geometrically (e.g. its nearest anchor
    /// or branch vertex and its side), never by a piece number.
    pub locator: L,
}

/// The ladder's attempt a [`Signal`] came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rung {
    /// The rung, from 0.
    pub k: u8,
    /// No key the failing check read changed its decision class or tie
    /// band since this cluster's previous signal, and none is a near-tie
    /// whose precision level changed (precision still acting).
    pub unchanged: bool,
    /// A near-tie the check read moved under precision over the rungs:
    /// its sign or decision class changed, or its error shrank below its
    /// value.
    pub near_tie_moved: bool,
}

/// Why a failure is a rule gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Gap {
    /// A validity failure certified at a pinned placement.
    Pin,
    /// The stop: the same failure again, no better.
    Stop,
    /// A later check still failing at the ladder's last rung, with no
    /// near-tie moved.
    Last,
}

/// Where a failure goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    /// Halve `b_K` and re-run the cluster.
    Halve,
    /// Halving would take `b_K` below the storage floor: unrepresentable.
    Floor,
    /// A rule gap.
    Gap(Gap),
    /// No halving: the ladder's next rung (precision and the tie band).
    Rung,
    /// At the ladder's last rung with a near-tie moved: out of precision.
    Precision,
}

/// A failure's routing input that isn't usable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    /// A quantity that is a NaN or infinite.
    Quantity,
    /// A band that isn't finite and `≥ 0`.
    Band,
    /// A gap bound that isn't finite and `> 0`.
    Bound,
    /// A coordinate magnitude that isn't finite.
    Magnitude,
}

/// Routes a certificate failure `f` at gap bound `b_k`, the cluster's
/// coordinate magnitude `x_k`, after `prev`, the failure that led to the
/// halving to `b_k` (if one did) (module docs: rows 2 to 5).
pub fn route<K: PartialEq>(
    f: &Failed<K>,
    prev: Option<&Failed<K>>,
    b_k: f64,
    x_k: f64,
) -> Result<Route, InputError> {
    if !(b_k.is_finite() && b_k > 0.0) {
        return Err(InputError::Bound);
    }
    if !f.quantity.is_finite() {
        return Err(InputError::Quantity);
    }
    if f.cert == Cert::Validity {
        let mut pinned = false;
        for &[bx, by] in &f.pins {
            if !(bx.is_finite() && bx >= 0.0 && by.is_finite() && by >= 0.0) {
                return Err(InputError::Band);
            }
            pinned |= r_on(b_k, bx, by) > b_k;
        }
        if pinned {
            return Ok(Route::Gap(Gap::Pin));
        }
    }
    if let Some(p) = prev
        && p.cert == f.cert
        && p.key == f.key
        // `p.quantity` passed this check when it was routed.
        && f.quantity <= p.quantity
    {
        return Ok(Route::Gap(Gap::Stop));
    }
    let floor = eps_min(x_k).ok_or(InputError::Magnitude)?;
    if b_k / 2.0 < floor {
        return Ok(Route::Floor);
    }
    Ok(Route::Halve)
}

/// Routes a later check's failure `s` at gap bound `b_k` and rung `rung`,
/// whose level-set pieces (if any) are clear, after `prev`, this cluster's
/// previous signal and the bound it came at (module docs: rows 3 and 5).
/// Never halves.
pub fn route_signal<L: PartialEq>(
    s: &Signal<L>,
    prev: Option<&(f64, Signal<L>)>,
    b_k: f64,
    rung: Rung,
) -> Route {
    if let Some((pb, p)) = prev
        && rung.k >= 2
        && rung.unchanged
        && *pb == b_k
        && p.check == s.check
        && p.locator == s.locator
    {
        return Route::Gap(Gap::Stop);
    }
    if rung.k >= K_MAX {
        return if rung.near_tie_moved {
            Route::Precision
        } else {
            Route::Gap(Gap::Last)
        };
    }
    Route::Rung
}

/// What happened to a cluster, in order: the evidence of a rule gap.
#[derive(Clone, Debug, PartialEq)]
pub enum Event<K, L> {
    /// An attempt failed at bound `b_k`.
    Failed { b_k: f64, failed: Failed<K> },
    /// The abutting leaves were recertified as Simple, at bound `b_k`.
    Simple { b_k: f64, leaves: usize },
    /// `b_K` was halved to `b_k`.
    Halved { b_k: f64 },
    /// An attempt passed at bound `b_k`.
    Passed { b_k: f64 },
    /// A later check failed at bound `b_k`, at rung `rung`.
    Signal {
        b_k: f64,
        rung: u8,
        signal: Signal<L>,
    },
}

/// A cluster failure that is a rule gap of the kernel: no halving,
/// precision or tie band settles it.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleGap<K, L> {
    /// Why.
    pub gap: Gap,
    /// What led to it.
    pub events: Vec<Event<K, L>>,
}

impl<K, L> RuleGap<K, L> {
    /// The kernel error it is.
    pub fn error(&self) -> KernelError {
        KernelError::Internal(Internal::Inconsistent)
    }
}

/// Why an escalation ended without the cluster passing.
#[derive(Clone, Debug, PartialEq)]
pub enum EscalateError<E, K, L> {
    /// A rule gap ([`RuleGap::error`]).
    RuleGap(Box<RuleGap<K, L>>),
    /// Halving would take `b_K` below the storage floor:
    /// [`KernelError::Unrepresentable`] unless a collapse repair succeeds.
    Floor {
        /// The bound that failed last.
        b_k: f64,
        /// The cluster's coordinate magnitude.
        x_k: f64,
    },
    /// The ladder's last rung with a near-tie moved: the ladder ends out
    /// of budget at precision, with its own profile.
    Precision,
    /// A certificate still open at a fixed limit, or the budget spent
    /// ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
    /// A failure's routing input isn't usable.
    Input(InputError),
    /// The caller's step failed.
    Step(E),
}

impl<E, K, L> From<InputError> for EscalateError<E, K, L> {
    fn from(e: InputError) -> Self {
        EscalateError::Input(e)
    }
}

/// The cluster's steps [`Escalation`] drives.
pub(crate) trait Resnap {
    /// A certificate's geometric key.
    type Key: Clone + PartialEq + Debug;
    /// A step's own failure.
    type Error;

    /// Recertifies the cluster's leaves at bound `b_k` and snaps it,
    /// charged to `work` at rung `rung`.
    fn attempt(
        &mut self,
        b_k: f64,
        rung: u8,
        work: &mut Work,
    ) -> Result<Outcome<Self::Key>, Self::Error>;

    /// Recertifies the abutting leaves `abutments` as Simple, charged to
    /// `work` at rung `rung`.
    fn simple(
        &mut self,
        abutments: &[Abutment],
        rung: u8,
        work: &mut Work,
    ) -> Result<(), Self::Error>;

    /// Re-runs the rim clearance of the level-set rim pieces `pieces` at
    /// bound `b_k`, charged to `work` at rung `rung`: its failure is a
    /// [`Cert::Rim`] one.
    fn clearance(
        &mut self,
        pieces: &[u32],
        b_k: f64,
        rung: u8,
        work: &mut Work,
    ) -> Result<Outcome<Self::Key>, Self::Error>;

    /// The cluster's coordinate magnitude: the largest of its rim window's
    /// coordinates, at its last attempt.
    fn magnitude(&self) -> f64;
}

/// After a later check's failure, where the ladder goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Next {
    /// The cluster is unchanged: the next rung's precision and tie band.
    Rung,
    /// The cluster was re-run (abutting leaves made Simple, or `b_K`
    /// halved) and passes at bound `b_k`: a changed decision.
    Resnapped {
        /// Its bound now.
        b_k: f64,
    },
}

/// One cluster's escalation: its gap bound and what happened to it.
#[derive(Clone, Debug)]
pub struct Escalation<K, L> {
    b_k: f64,
    abutments: Vec<Abutment>,
    simple_done: bool,
    /// The failure that led to the last halving, while that halving's
    /// route runs.
    halved_from: Option<Failed<K>>,
    /// The last later-check failure and its bound.
    last_signal: Option<(f64, Signal<L>)>,
    halvings: u32,
    events: Vec<Event<K, L>>,
}

/// The escalation's stage, in the budget's profile.
const STAGE: &str = "snap escalation";

/// The result of an escalation step.
type Esc<T, S, L> = Result<T, EscalateError<<S as Resnap>::Error, <S as Resnap>::Key, L>>;

impl<K: Clone + PartialEq + Debug, L: Clone + PartialEq> Escalation<K, L> {
    /// A cluster at gap bound `b_k` (finite, `> 0`) whose leaves
    /// `abutments` abut leaves of the other kind.
    pub fn new(b_k: f64, abutments: Vec<Abutment>) -> Result<Self, InputError> {
        if !(b_k.is_finite() && b_k > 0.0) {
            return Err(InputError::Bound);
        }
        Ok(Escalation {
            b_k,
            abutments,
            simple_done: false,
            halved_from: None,
            last_signal: None,
            halvings: 0,
            events: Vec::new(),
        })
    }

    /// The gap bound now.
    pub fn b_k(&self) -> f64 {
        self.b_k
    }

    /// The halvings made.
    pub fn halvings(&self) -> u32 {
        self.halvings
    }

    /// What happened so far.
    pub fn events(&self) -> &[Event<K, L>] {
        &self.events
    }

    /// Snaps the cluster at its bound, escalating every failure until it
    /// passes; gives the bound it passes at.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the boolean's snap calls it as it lands")
    )]
    pub(crate) fn settle<S>(&mut self, snap: &mut S, work: &mut Work) -> Esc<f64, S, L>
    where
        S: Resnap<Key = K>,
    {
        let out = snap
            .attempt(self.b_k, 0, work)
            .map_err(EscalateError::Step)?;
        match self.outcome(out)? {
            None => Ok(self.b_k),
            Some(f) => {
                let f = match self.abut(snap, 0, work)? {
                    false => f,
                    true => match self.retry(snap, 0, work)? {
                        None => return Ok(self.b_k),
                        Some(g) => g,
                    },
                };
                self.halve(snap, f, 0, work)
            }
        }
    }

    /// Escalates a later check's failure `s` at rung `rung` (module docs):
    /// abutting leaves first, then the level-set pieces' rim clearance,
    /// halving if that fails, else the signal's own route.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the ladder calls it as it lands")
    )]
    pub(crate) fn signal<S>(
        &mut self,
        snap: &mut S,
        s: Signal<L>,
        rung: Rung,
        work: &mut Work,
    ) -> Esc<Next, S, L>
    where
        S: Resnap<Key = K>,
    {
        self.events.push(Event::Signal {
            b_k: self.b_k,
            rung: rung.k,
            signal: s.clone(),
        });
        if self.abut(snap, rung.k, work)? {
            self.last_signal = Some((self.b_k, s));
            return match self.retry(snap, rung.k, work)? {
                None => Ok(Next::Resnapped { b_k: self.b_k }),
                Some(f) => self
                    .halve(snap, f, rung.k, work)
                    .map(|b_k| Next::Resnapped { b_k }),
            };
        }
        if !s.level_set.is_empty() {
            let out = snap
                .clearance(&s.level_set, self.b_k, rung.k, work)
                .map_err(EscalateError::Step)?;
            if let Some(f) = self.outcome(out)? {
                self.last_signal = Some((self.b_k, s));
                return self
                    .halve(snap, f, rung.k, work)
                    .map(|b_k| Next::Resnapped { b_k });
            }
        }
        let route = route_signal(&s, self.last_signal.as_ref(), self.b_k, rung);
        self.last_signal = Some((self.b_k, s));
        match route {
            Route::Gap(gap) => Err(self.gap(gap)),
            Route::Precision => Err(EscalateError::Precision),
            _ => Ok(Next::Rung),
        }
    }

    /// Records an attempt's outcome: `None` when it passed.
    fn outcome<E>(&mut self, out: Outcome<K>) -> Result<Option<Failed<K>>, EscalateError<E, K, L>> {
        match out {
            Outcome::Pass => {
                self.events.push(Event::Passed { b_k: self.b_k });
                self.halved_from = None;
                Ok(None)
            }
            Outcome::Fail(f) => {
                self.events.push(Event::Failed {
                    b_k: self.b_k,
                    failed: f.clone(),
                });
                Ok(Some(f))
            }
            Outcome::Exhausted(e) => Err(EscalateError::OutOfBudget(e)),
        }
    }

    /// The abutting leaves recertified as Simple, the first time only:
    /// whether they were.
    fn abut<S: Resnap<Key = K>>(
        &mut self,
        snap: &mut S,
        rung: u8,
        work: &mut Work,
    ) -> Esc<bool, S, L> {
        if self.simple_done || self.abutments.is_empty() {
            return Ok(false);
        }
        self.simple_done = true;
        snap.simple(&self.abutments, rung, work)
            .map_err(EscalateError::Step)?;
        self.events.push(Event::Simple {
            b_k: self.b_k,
            leaves: self.abutments.len(),
        });
        Ok(true)
    }

    /// The cluster re-run at its bound: `None` when it passes.
    fn retry<S: Resnap<Key = K>>(
        &mut self,
        snap: &mut S,
        rung: u8,
        work: &mut Work,
    ) -> Esc<Option<Failed<K>>, S, L> {
        let out = snap
            .attempt(self.b_k, rung, work)
            .map_err(EscalateError::Step)?;
        self.outcome(out)
    }

    /// Routes certificate failure `f` and halves until the cluster passes
    /// (its bound then) or a route ends it.
    fn halve<S: Resnap<Key = K>>(
        &mut self,
        snap: &mut S,
        mut f: Failed<K>,
        rung: u8,
        work: &mut Work,
    ) -> Esc<f64, S, L> {
        loop {
            let x_k = snap.magnitude();
            match route(&f, self.halved_from.as_ref(), self.b_k, x_k)? {
                Route::Gap(gap) => return Err(self.gap(gap)),
                Route::Floor => return Err(EscalateError::Floor { b_k: self.b_k, x_k }),
                _ => {}
            }
            charge(work, rung).map_err(EscalateError::OutOfBudget)?;
            self.halved_from = Some(f);
            self.b_k /= 2.0;
            self.halvings = self.halvings.saturating_add(1);
            self.events.push(Event::Halved { b_k: self.b_k });
            match self.retry(snap, rung, work)? {
                None => return Ok(self.b_k),
                Some(g) => f = g,
            }
        }
    }

    /// The rule gap `gap`, with what led to it.
    fn gap<E>(&self, gap: Gap) -> EscalateError<E, K, L> {
        EscalateError::RuleGap(Box::new(RuleGap {
            gap,
            events: self.events.clone(),
        }))
    }
}

/// Charges one halving: a pass of one item, one unit.
fn charge(work: &mut Work, rung: u8) -> Result<(), KernelError> {
    work.pass(Pass { stage: STAGE, rung }, &[()], |_, ledger| {
        ledger
            .charge(Step::Work, 1)
            .map_err(Stop::<KernelError>::from)
    })
    .map(|_| ())
    .map_err(|halt| match halt {
        Halt::OutOfBudget(e) | Halt::Failed { error: e, .. } => e,
    })
}

#[cfg(test)]
mod tests;
