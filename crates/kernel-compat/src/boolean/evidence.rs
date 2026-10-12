//! Gathering what a boolean's own failure is about, for the decisions'
//! and the assembly's sites: an operand's edges as curves, its triangles
//! as patches with the faces they lie on by name, and points (see
//! "Boolean evidence" in the kernel notes).

use std::collections::BTreeSet;

use glam::DVec3;

use super::BooleanError;
use super::input::{Input, Side};
use crate::budget::Work;
use crate::failure::evidence_work;
use crate::mesh::FaceKey;
use crate::patch::Conic3;
use crate::{Evidence, Failure, KernelError, MAX_EVIDENCE, Operand};

/// One failure's evidence as it is gathered, from a fresh
/// [`EVIDENCE_WORK`](crate::EVIDENCE_WORK) allowance: a unit an item
/// (and what a step says it charges for looking), up to the caps.
pub(super) struct Gather {
    evidence: Evidence,
    work: Work,
    /// The faces named so far, each named once.
    named: BTreeSet<(Operand, FaceKey)>,
}

impl Gather {
    pub(super) fn new() -> Gather {
        #[cfg(test)]
        if let Some(units) = tests::ALLOWANCE.get() {
            return Gather {
                evidence: Evidence::default(),
                work: Work::new(&crate::Budget::new(units)),
                named: BTreeSet::new(),
            };
        }
        Gather {
            evidence: Evidence::default(),
            work: evidence_work(),
            named: BTreeSet::new(),
        }
    }

    /// Takes `units` of the allowance, or marks the evidence truncated and
    /// says no once it has run out.
    pub(super) fn afford(&mut self, units: usize) -> bool {
        self.evidence.afford(&mut self.work, units)
    }

    /// Whether some was left out already, past the allowance or a cap:
    /// a step adding many items stops there, so what it gave are the
    /// first in its order, whole.
    pub(super) fn truncated(&self) -> bool {
        self.evidence.truncated
    }

    /// Triangle `t` of `input`, operand `side`: its patch (as the operand
    /// holds it, refined where the decisions refined it), and the name of
    /// the operand's face it lies on, once.
    pub(super) fn tri(&mut self, side: Side, input: &Input, t: u32) {
        if self.afford(1) {
            self.evidence.add_patches([input.patches[t as usize]]);
            self.name(side, input, t);
        }
    }

    /// Triangle `p` of `A` and `q` of `B`, both or neither (neither where
    /// only one would fit under the cap): as [`Gather::tri`], `A`'s first.
    pub(super) fn pair(&mut self, a: &Input, b: &Input, [p, q]: [u32; 2]) {
        if self.evidence.patches.len().saturating_add(2) > MAX_EVIDENCE.patches {
            self.evidence.truncated = true;
            return;
        }
        if self.afford(2) {
            self.evidence
                .add_patches([a.patches[p as usize], b.patches[q as usize]]);
            self.name(Side::A, a, p);
            self.name(Side::B, b, q);
        }
    }

    /// The name of the face triangle `t` of `input` (operand `side`) lies
    /// on, once, without its patch.
    pub(super) fn face(&mut self, side: Side, input: &Input, t: u32) {
        if self.afford(1) {
            self.name(side, input, t);
        }
    }

    fn name(&mut self, side: Side, input: &Input, t: u32) {
        let face = (
            Operand::from(side),
            input.mesh.faces()[input.face(t) as usize].name.key(),
        );
        if self.named.insert(face) {
            self.evidence.add_faces([face]);
        }
    }

    /// Edge `e` of `input` as its curve.
    pub(super) fn edge(&mut self, input: &Input, e: u32) {
        self.curve(input.conic(e));
    }

    pub(super) fn curve(&mut self, curve: Conic3) {
        if self.afford(1) {
            self.evidence.add_curves([curve]);
        }
    }

    pub(super) fn point(&mut self, p: DVec3) {
        if self.afford(1) {
            self.evidence.add_points([p]);
        }
    }

    /// The failure `error` with what was gathered.
    pub(super) fn failure(self, error: BooleanError) -> Failure {
        Failure {
            error: KernelError::Boolean(error),
            evidence: Box::new(self.evidence),
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
