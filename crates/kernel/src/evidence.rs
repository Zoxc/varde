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
//! - [`failure`]: an operation's [`Failure`]: its error and its
//!   [`Evidence`], whose [`Site`] refines the error's cause; a body's
//!   check failing, by role ([`Failure::of_check`]).
//! - [`inputref`]: the input entities evidence names ([`InputRef`]: leaf,
//!   copy, name, id, a point, the part named), the mapping of parts and
//!   of an operation's own entities to them, and the [`Item`]s it lists.
//! - [`snippet`]: the geometry, faces of the leaves cut out as sheets
//!   ([`SheetSnippet`]) that pass the check with the failing invariant
//!   left out.
//! - [`budget_profile`]: the profile of the work when it runs out, and
//!   the evidence of a result's warnings, kept on its status
//!   ([`WarningEvidence`](budget_profile::WarningEvidence)).
//! - [`cluster`]: the evidence of a tangency cluster no rule settles.
//! - [`replay`](mod@replay): a failure replayed in full, in process: the operation
//!   it came from, its parameters and its operands' digests, the
//!   operands themselves when captured ([`EvidenceMode::Capture`]).

pub mod budget_profile;
pub mod cluster;
pub mod failure;
pub mod inputref;
pub mod replay;
pub mod snippet;

pub use failure::{
    AuditSite, BudgetRow, Detail, Evidence, EvidenceDefect, Failure, InternalSite, LadderStep,
    MAX_EVIDENCE_ITEMS, RepairDetail, Replay, ReplayKind, Site, SnapSite,
};
pub use inputref::{InputRef, Item, ItemRole, Leaf, Window, edge_pair, snap_inputs, through_snap};
pub use replay::{
    Captured, Checked, EvidenceMode, FullReplay, Operation, Params, ReplayError, check_body,
    digest, replay, replay_with,
};
pub use snippet::{CutError, MAX_SNIPPET, Piece, SheetSnippet};

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
    use std::collections::BTreeMap;

    use varde_decide::num::storage::OpStart;

    use super::*;
    use crate::geom::curve::Curve3D;
    use crate::geom::form::PlaneKey;
    use crate::geom::lattice::{CELL, Lattice};
    use crate::geom::pcurve::{PCurve, Span2};
    use crate::geom::surface::Surface;
    use crate::infra::ops::{FeatureId, InstanceId, Status, Tolerance};
    use crate::topo::brep::{
        BodyKind, Brep, ExactRows, FaceKey, FacePart, RegionId, RegionKind, ShellId, ShellKind,
        Side,
    };
    use crate::topo::build::{Builder, Use};

    /// The unit cube's corners: corner `i` at the bits of `i` (x, y, z).
    pub(super) const CUBE: [[f64; 3]; 8] = {
        let mut p = [[0.0; 3]; 8];
        let mut i = 0;
        while i < 8 {
            p[i] = [(i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64];
            i += 1;
        }
        p
    };

    /// The cube's faces, corners counter-clockwise seen from outside.
    pub(super) const CUBE_FACES: [[u32; 4]; 6] = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];

    /// Face `j`'s key: feature 1, part `j`.
    pub(super) fn key(j: u32) -> FaceKey {
        FaceKey {
            feature: FeatureId(1),
            part: FacePart::Primitive(j),
            instance: InstanceId::ORIGINAL,
        }
    }

    /// The unit cube as a solid, built row by row: face `j` keyed
    /// [`key`]`(j)` on a plane of its own (each corner's UV its offset
    /// along the lattice's axes), each edge a line from its lower corner
    /// to its higher, the outer sides' shell and the lump.
    pub(super) fn cube() -> Brep {
        let start = OpStart {
            max_coord: 1.0,
            rho_op: 0.0,
        };
        let status = Status::new(Tolerance::DEFAULT, &start);
        let mut b = Builder::new(BodyKind::Solid, status);
        let verts: Vec<_> = CUBE.iter().map(|&p| b.add_vert(p).unwrap()).collect();
        let mut edges = BTreeMap::new();
        let mut faces = Vec::new();
        for (j, f) in CUBE_FACES.iter().enumerate() {
            let [p0, p1, p2] = [0, 1, 2].map(|k| CUBE[f[k] as usize]);
            let (d, e) = (
                [0, 1, 2].map(|k| p1[k] - p0[k]),
                [0, 1, 2].map(|k| p2[k] - p1[k]),
            );
            let n = [
                d[1] * e[2] - d[2] * e[1],
                d[2] * e[0] - d[0] * e[2],
                d[0] * e[1] - d[1] * e[0],
            ];
            let plane = PlaneKey::new(n, p0).unwrap();
            let lat = Lattice::new(plane.into()).unwrap();
            let surface = Surface::new(lat.clone(), lat.half(0).unwrap()).unwrap();
            let s = b.add_surface(surface, None, ExactRows::default()).unwrap();
            let flipped = (0..3).map(|k| n[k] * plane.normal()[k]).sum::<f64>() < 0.0;
            let face = b.add_face(s, flipped, key(j as u32)).unwrap();
            let (o, [ua, va, _]) = (lat.origin(), lat.axes());
            let uv = |x: [f64; 3]| {
                let d = [0, 1, 2].map(|k| x[k] - o[k]);
                let [u, v] =
                    [ua, va].map(|a| (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5);
                [u, v, 1.0]
            };
            let uses: Vec<Use> = (0..4)
                .map(|k| {
                    let (from, to) = (f[k], f[(k + 1) % 4]);
                    let (lo, hi) = (from.min(to), from.max(to));
                    let (a, z) = (CUBE[lo as usize], CUBE[hi as usize]);
                    let edge = *edges.entry([lo, hi]).or_insert_with(|| {
                        let m = [0, 1, 2].map(|i| (a[i] + z[i]) * 0.5);
                        let pt = |p: [f64; 3]| [p[0], p[1], p[2], 1.0];
                        let line = Curve3D::new(vec![[pt(a), pt(m), pt(z)]], vec![0.0, 1.0]);
                        let c = b.add_curve(line.unwrap()).unwrap();
                        let ends = [verts[lo as usize], verts[hi as usize]];
                        b.add_edge(c, [0.0, 1.0], ends).unwrap()
                    });
                    let (ua, uz) = (uv(a), uv(z));
                    let mid = [(ua[0] + uz[0]) * 0.5, (ua[1] + uz[1]) * 0.5, 1.0];
                    let span: Span2 = [ua, mid, uz];
                    let pcurve = b.add_pcurve(PCurve::new(vec![span], vec![0.0, 1.0]).unwrap());
                    Use {
                        edge,
                        reversed: from > to,
                        pcurve: pcurve.unwrap(),
                        seam_bound: 0.0,
                    }
                })
                .collect();
            b.add_loop(face, true, &uses).unwrap();
            faces.push(face);
        }
        let sides = |side| faces.iter().map(|&f| (f, side)).collect::<Vec<_>>();
        let out = b.add_shell(ShellKind::Island, sides(Side::Out)).unwrap();
        let inn = b.add_shell(ShellKind::Outer, sides(Side::In)).unwrap();
        b.add_region(RegionKind::Empty, ShellId::NONE, vec![out], RegionId::NONE)
            .unwrap();
        b.add_region(RegionKind::Material, inn, Vec::new(), RegionId::NONE)
            .unwrap();
        b.finish().unwrap().0
    }

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
