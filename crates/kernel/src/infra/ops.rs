//! What every kernel operation takes besides its operands, and what it
//! reports besides its result: the document's tolerance ([`Tolerance`]),
//! the caller's context ([`OpCtx`]) and the result's status ([`Status`]).
//!
//! **One behaviour.** An operation has no mode choosing between refusing
//! and approximating. Where a target is met only approximately (an
//! accuracy certificate stopping at its limit, a value rounded to
//! storage below the spacing it needs, a rounded result repaired), the
//! operation returns its result anyway, flagged approximate, and lists
//! the item as a [`Warning`] with its achieved bound, always finite.
//! Refusing such results is the caller's choice, made on the status.
//! Every decision, and the topology following from them, is the same
//! whether or not the result is approximate.
//!
//! **The tolerance** `ε` is the document's one setting, passed unchanged
//! to every operation on it; every band and target an operation uses
//! derives from it and from `ρ_op`, the operation's one rounding bound,
//! fixed when it starts ([`OpStart`]). A result is within contract when
//! its largest deviation is at most `3ε/4 + ρ_op`.
//!
//! **The context** names the feature (and the copy of it) the operation
//! builds, as the caller numbers them: the result's faces are named after
//! it, never after a counter of the kernel's, so a feature rebuilt gives
//! the same names.
//!
//! **The status** holds, so far, the approximate flag, the largest
//! deviation and the warnings. A result's status also gains, as the
//! operations building them land: the changed regions where tangent
//! contacts were settled (zones), the edges whose tolerance is above the
//! contract (bounds), the touching faces newly joined, notes (such as
//! regions the label audit skipped and a move's rounding), the entities'
//! history through the operation, and the evidence attached to each
//! warning. The evidence never changes the warnings, their bounds or
//! their count.

use std::cmp::Ordering;

use malachite::Rational;
use varde_decide::num::exact::rat;
use varde_decide::num::storage::{EPS_RANGE, OpStart};

/// The document's tolerance `ε` (mm): one setting, passed unchanged to
/// every operation on the document. It lies in
/// [`MIN`](Self::MIN)`..=`[`MAX`](Self::MAX), so it is never below the
/// least tolerance storage allows at the largest coordinate.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Tolerance {
    eps: f64,
}

impl Tolerance {
    /// The smallest tolerance: 10⁻⁵ mm.
    pub const MIN: f64 = *EPS_RANGE.start();
    /// The largest tolerance: 0.1 mm.
    pub const MAX: f64 = *EPS_RANGE.end();
    /// A tolerance of 1 µm.
    pub const DEFAULT: Tolerance = Tolerance { eps: 1e-3 };

    /// The tolerance `eps`, if it lies within
    /// [`MIN`](Self::MIN)`..=`[`MAX`](Self::MAX) (so not if it isn't a
    /// number).
    pub fn new(eps: f64) -> Option<Tolerance> {
        EPS_RANGE.contains(&eps).then_some(Tolerance { eps })
    }

    /// `ε`.
    pub fn eps(self) -> f64 {
        self.eps
    }

    /// Whether a deviation of `deviation` is within contract for an
    /// operation of rounding bound `rho_op`: `deviation ≤ 3ε/4 + ρ_op`,
    /// compared exactly. False unless both are finite.
    pub fn within_contract(self, deviation: f64, rho_op: f64) -> bool {
        if !deviation.is_finite() || !rho_op.is_finite() {
            return false;
        }
        let bound = rat(self.eps) * Rational::from_unsigneds(3u32, 4u32) + rat(rho_op);
        rat(deviation) <= bound
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The feature an operation builds, as the caller numbers it. Names,
/// not positions: the kernel only compares them, and orders faces by
/// them (the leaf order: feature, then instance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FeatureId(pub u64);

/// Which copy of a feature an operation builds, as the caller numbers it
/// (a pattern's index); [`InstanceId::ORIGINAL`] for the feature itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct InstanceId(pub u32);

impl InstanceId {
    /// The feature itself, not a copy of it.
    pub const ORIGINAL: InstanceId = InstanceId(0);
}

/// The caller's context of one operation, its last argument: the feature
/// and the copy of it the operation builds. The faces the operation
/// makes are named after it; ordered by feature, then instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OpCtx {
    /// The feature.
    pub feature: FeatureId,
    /// The copy of it.
    pub instance: InstanceId,
}

impl OpCtx {
    /// The context of feature `feature` itself, not a copy.
    pub fn new(feature: FeatureId) -> OpCtx {
        OpCtx {
            feature,
            instance: InstanceId::ORIGINAL,
        }
    }
}

/// The most warnings a status lists; past it, warnings are only counted.
pub const MAX_WARNINGS: usize = 64;

/// The most rounds the repair of one site of a rounded result may take
/// (each round a reorder, a bend or a collapse); a site still failing
/// after them makes the result unrepresentable
/// ([`Storage::Repair`](crate::Storage::Repair)). Not a size limit of the
/// budget: reaching it is a storage cause. Provisional: its value is set
/// from measurement when the repair is built.
pub const MAX_REPAIR_ROUNDS: u32 = 8;

/// How a repair settled a site of a rounded result where rounding broke
/// validity or an order the result relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Repair {
    /// Another stored value within the vertex's tolerance.
    Reorder,
    /// The offending arc split at an extra vertex on the stored grid.
    Bend,
    /// A feature smaller than the storage spacing contracted to a vertex
    /// or an edge, recorded as a contact.
    Collapse,
}

/// What made a result approximate at one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WarningKind {
    /// A seam or fit missed its target: refining it reached its limit,
    /// or its bound came within reach of a certified lower bound.
    Accuracy,
    /// A target finer than the storage spacing at its magnitude, the
    /// value rounded to storage.
    StorageRounding,
    /// A site the rounding broke, repaired.
    Repair(Repair),
}

/// One approximate item of a result: its kind and its achieved deviation
/// bound (mm), finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Warning {
    kind: WarningKind,
    bound: f64,
}

impl Warning {
    /// The warning of kind `kind` with achieved bound `bound`; `None`
    /// unless `bound` is finite and not negative. Every warning has a
    /// finite bound.
    pub fn new(kind: WarningKind, bound: f64) -> Option<Warning> {
        // `+ 0.0` turns a `-0` into `+0`.
        (bound.is_finite() && bound >= 0.0).then_some(Warning {
            kind,
            bound: bound + 0.0,
        })
    }

    /// Its kind.
    pub fn kind(&self) -> WarningKind {
        self.kind
    }

    /// Its achieved deviation bound (mm).
    pub fn bound(&self) -> f64 {
        self.bound
    }
}

/// What an operation reports of its result besides the result (module
/// docs): whether it is approximate, its largest deviation, and its
/// warnings, the first [`MAX_WARNINGS`] listed and all counted. The flag
/// and the deviation cover every warning, listed or not.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    eps: Tolerance,
    rho_op: f64,
    warnings: Vec<Warning>,
    count: u64,
    max_deviation: f64,
}

impl Status {
    /// The status of an operation on tolerance `eps` that started as
    /// `start`, before any warning.
    pub fn new(eps: Tolerance, start: &OpStart) -> Status {
        Status {
            eps,
            rho_op: start.rho_op,
            warnings: Vec::new(),
            count: 0,
            max_deviation: 0.0,
        }
    }

    /// Adds a warning: listed if fewer than [`MAX_WARNINGS`] are,
    /// counted either way. An operation adds them in the order of its
    /// items' keys, so the listed ones are the least.
    pub fn warn(&mut self, warning: Warning) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(warning);
        }
        self.count = self.count.saturating_add(1);
        if warning.bound.partial_cmp(&self.max_deviation) == Some(Ordering::Greater) {
            self.max_deviation = warning.bound;
        }
    }

    /// The operation's tolerance.
    pub fn eps(&self) -> Tolerance {
        self.eps
    }

    /// The operation's rounding bound `ρ_op`.
    pub fn rho_op(&self) -> f64 {
        self.rho_op
    }

    /// Whether the result is approximate: whether any warning was added.
    pub fn approximate(&self) -> bool {
        self.count > 0
    }

    /// The largest achieved bound over every warning (mm), finite; 0
    /// without warnings.
    pub fn max_deviation(&self) -> f64 {
        self.max_deviation
    }

    /// Whether the result is within contract: its largest deviation is
    /// at most `3ε/4 + ρ_op` ([`Tolerance::within_contract`]). A result
    /// beyond it is still returned.
    pub fn within_contract(&self) -> bool {
        self.eps.within_contract(self.max_deviation, self.rho_op)
    }

    /// The listed warnings, the first [`MAX_WARNINGS`] added.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// How many warnings were added, listed or not.
    pub fn warning_count(&self) -> u64 {
        self.count
    }
}

#[cfg(test)]
mod tests;
