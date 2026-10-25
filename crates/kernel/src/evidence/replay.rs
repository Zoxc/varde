//! A failure replayed in full, in process: the operation run again on
//! its operands with its parameters and budget, giving the same
//! [`Failure`] bit for bit.
//!
//! **What a full replay keeps.** Every failure whose error is replayed in
//! full ([`ReplayKind::Full`](super::ReplayKind::Full): every running out, every failure of the
//! kernel's own, a repair failing, a feature operation's) carries, once
//! its operation's entry has built it, the operation ([`Operation`]: its
//! context, its parameters, its budget) and its operands' digests
//! ([`digest`]). Under [`EvidenceMode::Capture`] it also keeps the
//! operands themselves, as the shared handles the caller gave
//! ([`Captured`]); otherwise the caller hands them back, and the digests
//! tell whether they are the same bodies ([`replay_with`]). There is no
//! file: a capture lives as long as the failure.
//!
//! **Replaying.** [`replay`] runs the operation again on the captured
//! operands, [`replay_with`] on the ones given, in the mode the failure
//! was built in: same operands, parameters and budget give the same
//! failure, its evidence and its replay included, on any number of
//! threads. A replay that doesn't fail is a [`ReplayError::Passed`].
//!
//! **Operations.** Each operation names its parameters in [`Params`] as
//! its entry lands; so far a body's check, as an operand, a constructed
//! body or a result ([`check_body`]): the last is how an operation's own
//! defect (`Internal`) is replayed.

use std::fmt::{self, Write as _};
use std::sync::Arc;

use super::failure::{Failure, Replay};
use super::inputref::Leaf;
use crate::infra::budget::{Budget, Work};
use crate::infra::ops::OpCtx;
use crate::names::keys::{Digest, TextDigest};
use crate::topo::brep::Brep;
use crate::topo::check::{Inv, Outcome, Role, Scope};

#[cfg(test)]
mod tests;

/// What an operation's failures keep for a full replay (module docs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EvidenceMode {
    /// The operation and its operands' digests: the caller hands the
    /// operands back to replay it.
    #[default]
    Digests,
    /// Also the operands themselves, as shared handles.
    Capture,
}

/// An operation's parameters, by operation (module docs).
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum Params {
    /// A body's check as a body of `role`, invariant `except` left out:
    /// one operand, the body.
    Check {
        /// The body's role.
        role: Role,
        /// The invariant left out.
        except: Option<Inv>,
    },
}

impl Params {
    /// How many operands it takes.
    pub fn operands(&self) -> usize {
        match self {
            Params::Check { .. } => 1,
        }
    }
}

/// An operation as replayed: its context, its parameters and its budget.
#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    /// Its context.
    pub ctx: OpCtx,
    /// Its parameters.
    pub params: Params,
    /// Its budget.
    pub budget: Budget,
}

/// The operands a failure captured ([`EvidenceMode::Capture`]), as the
/// caller's shared handles; none otherwise. Its `Debug` shows how many,
/// not the bodies. Two compare equal when both hold as many operands or
/// both none: the bodies themselves compare by the digests kept beside
/// them ([`FullReplay::digests`]).
#[derive(Clone, Default)]
pub struct Captured(Option<Arc<[Arc<Brep>]>>);

impl Captured {
    /// The operands, if captured.
    pub fn operands(&self) -> Option<&[Arc<Brep>]> {
        self.0.as_deref()
    }
}

impl fmt::Debug for Captured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            None => f.write_str("Captured(None)"),
            Some(b) => write!(f, "Captured({} bodies)", b.len()),
        }
    }
}

impl PartialEq for Captured {
    fn eq(&self, o: &Self) -> bool {
        self.0.as_ref().map(|b| b.len()) == o.0.as_ref().map(|b| b.len())
    }
}

/// What a full replay needs (module docs): the operation, its operands'
/// digests in order, and the operands if captured. Empty (no operation)
/// for a failure no operation's entry has built.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FullReplay {
    /// The operation.
    pub operation: Option<Operation>,
    /// Its operands' digests, in order.
    pub digests: Vec<Digest>,
    /// Its operands, if captured.
    pub captured: Captured,
}

/// Why a failure isn't replayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayError {
    /// Its error isn't replayed in full (a local replay, or none).
    NotFull,
    /// No operation's entry built it: there is nothing to run.
    NoOperation,
    /// Its operands weren't captured: [`replay_with`] takes them.
    NotCaptured,
    /// As many operands as given, not as many as the operation takes.
    Operands {
        /// How many were given.
        given: usize,
    },
    /// Operand `operand` isn't the body it failed on: its digest differs.
    Digest {
        /// The operand, by its place.
        operand: usize,
    },
    /// The operation didn't fail on replay.
    Passed,
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayError::NotFull => f.write_str("the failure isn't replayed in full"),
            ReplayError::NoOperation => f.write_str("the failure names no operation"),
            ReplayError::NotCaptured => f.write_str("the failure's operands weren't captured"),
            ReplayError::Operands { given } => {
                write!(f, "{given} operands given, not the operation's")
            }
            ReplayError::Digest { operand } => {
                write!(
                    f,
                    "operand {operand} isn't the body the operation failed on"
                )
            }
            ReplayError::Passed => f.write_str("the operation didn't fail on replay"),
        }
    }
}

impl std::error::Error for ReplayError {}

/// The digest of `body`: of its whole store, every field and value bit
/// for bit, as its `Debug` text writes it (an `f64` as the shortest
/// decimal reading back to its bits, `-0.0` apart from `0.0`; derived
/// caches left out). Two bodies of one build have one digest exactly
/// when they store the same (digest collisions aside); the text is not
/// fixed across builds, so neither is the digest.
pub fn digest(body: &Brep) -> Digest {
    let mut d = TextDigest::new();
    // Writing into a digest never fails.
    let _ = write!(d, "{body:?}");
    d.finish()
}

/// How a check that didn't fail went ([`check_body`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checked {
    /// Every invariant holds.
    Passed,
    /// A result's seam bound over its target: an accuracy item of the
    /// operation, not a failure.
    Accuracy,
    /// A result's orientation wrong: a signal to the operation's ladder.
    LadderSignal,
}

/// The check of `body` as a body of `role`, invariant `except` left out,
/// as operation `ctx` on a budget of `budget`; its failure as
/// [`Failure::of_check`] gives it for the body as leaf 0, kept for a
/// full replay as `mode` says (module docs).
pub fn check_body(
    body: &Arc<Brep>,
    role: Role,
    except: Option<Inv>,
    ctx: OpCtx,
    budget: Budget,
    mode: EvidenceMode,
) -> Result<Checked, Failure> {
    let op = Operation {
        ctx,
        params: Params::Check { role, except },
        budget,
    };
    run(&op, std::slice::from_ref(body), mode)
}

/// `op` run on `operands`, which are as many as it takes, its failure
/// kept for a full replay as `mode` says.
fn run(op: &Operation, operands: &[Arc<Brep>], mode: EvidenceMode) -> Result<Checked, Failure> {
    debug_assert_eq!(operands.len(), op.params.operands());
    let failure = match op.params {
        Params::Check { role, except } => {
            let body = &operands[0];
            let mut work = Work::new(&op.budget);
            let Err(e) = body.check_in(Scope::Full, except, &mut work, 0) else {
                return Ok(Checked::Passed);
            };
            match Failure::of_check(&e, role, &Leaf::new(0, body), op.ctx, 0) {
                Some(f) => f,
                // No failure only for an accuracy item or a ladder
                // signal.
                None => {
                    return Ok(match e.outcome(role) {
                        Outcome::LadderSignal => Checked::LadderSignal,
                        Outcome::Accuracy | Outcome::Error(_) => Checked::Accuracy,
                    });
                }
            }
        }
    };
    Err(kept(failure, op, operands, mode))
}

/// `f` with what its full replay needs, if it is replayed in full.
fn kept(mut f: Failure, op: &Operation, operands: &[Arc<Brep>], mode: EvidenceMode) -> Failure {
    if let Replay::Full(full) = &mut f.evidence.replay {
        full.operation = Some(op.clone());
        full.digests = operands.iter().map(|b| digest(b)).collect();
        full.captured = Captured(match mode {
            EvidenceMode::Digests => None,
            EvidenceMode::Capture => Some(operands.into()),
        });
    }
    f
}

/// `failure` replayed in full on its captured operands (module docs):
/// the failure the operation gives again.
pub fn replay(failure: &Failure) -> Result<Failure, ReplayError> {
    let full = full(failure)?;
    if full.operation.is_none() {
        return Err(ReplayError::NoOperation);
    }
    let operands = full.captured.operands().ok_or(ReplayError::NotCaptured)?;
    replay_with(failure, operands)
}

/// `failure` replayed in full on `operands`, which must be the bodies it
/// failed on, by their digests (module docs): the failure the operation
/// gives again.
pub fn replay_with(failure: &Failure, operands: &[Arc<Brep>]) -> Result<Failure, ReplayError> {
    let full = full(failure)?;
    let op = full.operation.as_ref().ok_or(ReplayError::NoOperation)?;
    if operands.len() != op.params.operands() || operands.len() != full.digests.len() {
        return Err(ReplayError::Operands {
            given: operands.len(),
        });
    }
    if let Some(operand) = (operands.iter().zip(&full.digests)).position(|(b, &d)| digest(b) != d) {
        return Err(ReplayError::Digest { operand });
    }
    let mode = match full.captured.operands() {
        Some(_) => EvidenceMode::Capture,
        None => EvidenceMode::Digests,
    };
    match run(op, operands, mode) {
        Ok(_) => Err(ReplayError::Passed),
        Err(f) => Ok(f),
    }
}

/// `failure`'s full replay.
fn full(failure: &Failure) -> Result<&FullReplay, ReplayError> {
    match &failure.evidence.replay {
        Replay::Full(full) => Ok(full),
        Replay::None | Replay::Local { .. } => Err(ReplayError::NotFull),
    }
}
