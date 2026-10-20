//! The whole snap of an operation on two built bodies: the clusters,
//! merged where their windows share an element, snapped in the geometric
//! order by [`snap_clusters`] with the stale leaves recertified after each
//! snap, every snap settled by the escalation ([`Escalation`]), then the
//! moved-vertex join once.
//!
//! **Recertification.** The driver keeps the bodies as built: a leaf a
//! snap left stale is recertified on the bodies before the snap (the
//! snapped geometry is not written back to the stores), and comes back as
//! it was. So a stale leaf merges into a snapped cluster exactly where its
//! boxes meet that cluster's window and its kind is the cluster's.
//!
//! **Attempts.** An attempt at a gap bound below the operation's
//! recertifies the cluster's leaves at that bound, each pair of boxes as
//! it is, and keeps those still of the cluster's kind (a leaf no longer
//! within the bound leaves the cluster unsplit). A snap pass refusing at
//! its own validity (a fitted edge beyond its target) is the attempt's
//! [`Cert::Validity`] failure; a pass running out of budget is
//! [`Outcome::Exhausted`]; any other refusal is the driver's error.

use std::collections::BTreeMap;

use varde_decide::ids::{EdgeId, FaceId, VertId};
use varde_decide::leaves::flat::{self, FlatPiece, Leaf, LeafWork};
use varde_decide::num::decide::Sign;
use varde_decide::records::keys::Operand;

use super::super::cluster::{Cluster, ClusterLeaf, Tangency, merge_sharing};
use super::super::corner::Placement;
use super::super::escalate::{Cert, EscalateError, Escalation, Failed, Outcome, Resnap};
use super::super::join::{
    ClusterMoves, JoinCoedge, JoinCurve, JoinEdge, JoinError, JoinInput, Joined, MovedVert, Origin,
    join,
};
use super::super::rewrite::{EdgeClass, RewriteError, SnapEdge, SnapVert};
use super::super::rim::{Element, RimError};
use super::super::stale::{
    Abutment, Fresh, Run, Snapped, Snapper, StaleError, Window, cluster_window, join_windows,
    snap_clusters,
};
use super::run::{Setup, Snap, Stop, snap_leaves, work};
use super::{Key, TARGET, q};
use crate::infra::budget::{Ledger, Stop as BudgetStop, Work};
use crate::infra::error::KernelError;

/// What the escalation keys a certificate by: the rewrite edge that
/// failed, or the cluster's own.
pub(crate) type CertKey = Option<SnapEdge>;

/// Why the driver stops: a pass refused its input (a gap of the passes or a
/// bug), or the escalation ended the cluster.
#[derive(Debug)]
#[allow(dead_code, reason = "read through `Debug` in the test reports")]
pub(crate) enum DriveError {
    /// A snap pass refused.
    Pass(Stop),
    /// The escalation ended without the cluster passing.
    Escalate(String),
    /// The join pass refused.
    Join(JoinError),
}

/// One cluster's escalation record.
#[derive(Clone, Debug)]
#[allow(dead_code, reason = "read through `Debug` in the test reports")]
pub(crate) struct Settled {
    /// The cluster's place.
    pub(crate) slot: u32,
    pub(crate) kind: Tangency,
    /// Its leaves at the operation's bound.
    pub(crate) leaves: usize,
    /// The bound it passed at.
    pub(crate) b_k: f64,
    pub(crate) halvings: u32,
    /// The attempts made.
    pub(crate) attempts: u32,
    /// Its coordinate magnitude.
    pub(crate) magnitude: f64,
}

/// The snap's driver (module docs).
pub(crate) struct Driver<'s, 'a> {
    pub(crate) s: &'s Setup<'a>,
    pub(crate) sigma_a: Sign,
    /// The live snaps, by place.
    pub(crate) snaps: BTreeMap<u32, Snap>,
    /// Every settled run, in run order.
    pub(crate) settled: Vec<Settled>,
    /// The join pass's last outcome.
    pub(crate) joined: Option<Joined<(), u32>>,
    /// The edges of the snapped side left to the join with an end
    /// moved but cut by a rewrite (not refit).
    pub(crate) cut_skipped: usize,
}

/// One cluster's attempts for the escalation.
struct Attempts<'x, 'a> {
    s: &'x Setup<'a>,
    leaves: Vec<&'x Leaf>,
    kind: Tangency,
    fixed: Vec<(Operand, VertId)>,
    sigma_a: Sign,
    last: Option<Snap>,
    attempts: u32,
    magnitude: f64,
}

/// `leaf` recertified at gap bound `b_k` on the built bodies.
fn recertify(s: &Setup<'_>, leaf: &Leaf, b_k: f64) -> Option<Leaf> {
    let piece = |o: Operand, b| {
        let body = s.body(o);
        let f = leaf_face(b);
        FlatPiece::of_box(&body.tiles[f.0 as usize], b, body.brep.flipped(f)).ok()
    };
    let (pa, pb) = (piece(Operand::A, &leaf.a)?, piece(Operand::B, &leaf.b)?);
    let kind = flat::certify(&pa, &pb, &q(b_k), &mut LeafWork::default())?;
    Some(Leaf {
        a: leaf.a,
        b: leaf.b,
        kind,
        world: leaf.world,
    })
}

fn leaf_face(b: &varde_decide::count::boxes::BoxKey) -> FaceId {
    b.face()
}

impl Resnap for Attempts<'_, '_> {
    type Key = CertKey;
    type Error = Stop;

    fn attempt(&mut self, b_k: f64, _rung: u8, _work: &mut Work) -> Result<Outcome<CertKey>, Stop> {
        self.attempts += 1;
        let owned: Vec<Leaf>;
        let ls: Vec<&Leaf> = if b_k == self.s.b_k {
            self.leaves.clone()
        } else {
            owned = self
                .leaves
                .iter()
                .filter_map(|l| recertify(self.s, l, b_k))
                .filter(|l| l.tangency() == Some(self.kind))
                .collect();
            owned.iter().collect()
        };
        if ls.is_empty() {
            // The cluster dissolved: nothing to snap.
            self.last = None;
            return Ok(Outcome::Pass);
        }
        match snap_leaves(self.s, &ls, self.kind, b_k, &self.fixed, self.sigma_a) {
            Ok(z) => {
                self.magnitude = magnitude(self.s, &z);
                self.last = Some(z);
                Ok(Outcome::Pass)
            }
            Err(Stop::Geometry(RewriteError::Fit(e))) => Ok(Outcome::Fail(Failed {
                cert: Cert::Validity,
                key: Some(e),
                quantity: 0.0,
                pins: Vec::new(),
            })),
            Err(
                Stop::Rim(RimError::OutOfBudget(e))
                | Stop::Rewrite(RewriteError::OutOfBudget(e))
                | Stop::Geometry(RewriteError::OutOfBudget(e)),
            ) => Ok(Outcome::Exhausted(e)),
            Err(e) => Err(e),
        }
    }

    fn simple(&mut self, _: &[Abutment], _: u8, _: &mut Work) -> Result<(), Stop> {
        Ok(())
    }

    fn clearance(
        &mut self,
        _: &[u32],
        _: f64,
        _: u8,
        _: &mut Work,
    ) -> Result<Outcome<CertKey>, Stop> {
        // The rim's box pieces were certified one-signed or clustered
        // when it was made: clear.
        Ok(Outcome::Pass)
    }

    fn magnitude(&self) -> f64 {
        self.magnitude
    }
}

/// A snap's coordinate magnitude: the largest coordinate of its window's
/// vertices and of its rim's edges' ends.
fn magnitude(s: &Setup<'_>, z: &Snap) -> f64 {
    let mut m = 0.0f64;
    let mut see = |p: [f64; 3]| {
        for x in p {
            if x.abs() > m {
                m = x.abs();
            }
        }
    };
    for e in &z.corner.window {
        match *e {
            Element::Vert(o, v) => see(s.body(o).brep.pos(v)),
            Element::Edge(o, g) => {
                let br = &s.body(o).brep;
                for v in br.ends(g) {
                    see(br.pos(v));
                }
            }
        }
    }
    m
}

impl<'s, 'a> Driver<'s, 'a> {
    pub(crate) fn new(s: &'s Setup<'a>, sigma_a: Sign) -> Self {
        Driver {
            s,
            sigma_a,
            snaps: BTreeMap::new(),
            settled: Vec::new(),
            joined: None,
            cut_skipped: 0,
        }
    }

    /// The vertices the live snaps other than `except` placed.
    fn fixed(&self, except: &[u32]) -> Vec<(Operand, VertId)> {
        let mut out = Vec::new();
        for (k, z) in &self.snaps {
            if except.contains(k) {
                continue;
            }
            for v in &z.rewrite.verts {
                if let SnapVert::Vert(o, id) = v.key
                    && v.moved.to != Placement::Stays
                {
                    out.push((o, id));
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The vertices of the snapped side snap `z` moved, with their new
    /// positions.
    fn moved(&self, z: &Snap) -> Vec<MovedVert> {
        let s_op = z.target.operand.other();
        let br = &self.s.body(s_op).brep;
        let mut out = Vec::new();
        for g in &z.geometry.verts {
            let SnapVert::Vert(o, v) = g.key else {
                continue;
            };
            if o != s_op || g.pos == br.pos(v) {
                continue;
            }
            let pinned = z
                .rewrite
                .verts
                .iter()
                .find(|x| x.key == g.key)
                .is_some_and(|x| x.moved.to != Placement::Stays);
            out.push(MovedVert {
                vert: v,
                pos: g.pos,
                r_on: pinned.then_some(z.b_k),
            });
        }
        out
    }
}

impl Snapper<Leaf, Key> for Driver<'_, '_> {
    type Error = DriveError;

    fn face_key(&self, operand: Operand, face: FaceId) -> Key {
        self.s.body(operand).face_key(face)
    }

    fn snap(
        &mut self,
        run: &Run<'_, Key>,
        leaves: &[Leaf],
        work: &mut Work,
    ) -> Result<Window, DriveError> {
        for k in run.replaces {
            self.snaps.remove(k);
        }
        let mut except = run.replaces.to_vec();
        except.push(run.slot);
        let fixed = self.fixed(&except);
        let mut at = Attempts {
            s: self.s,
            leaves: run.cluster.leaves().map(|i| &leaves[i as usize]).collect(),
            kind: run.cluster.kind(),
            fixed,
            sigma_a: self.sigma_a,
            last: None,
            attempts: 0,
            magnitude: 0.0,
        };
        let mut esc: Escalation<CertKey, ()> =
            Escalation::new(self.s.b_k, Vec::new()).expect("a positive bound");
        let b_k = esc.settle(&mut at, work).map_err(|e| match e {
            EscalateError::Step(s) => DriveError::Pass(s),
            e => DriveError::Escalate(format!("{e:?}")),
        })?;
        self.settled.push(Settled {
            slot: run.slot,
            kind: run.cluster.kind(),
            leaves: run.cluster.leaves().len(),
            b_k,
            halvings: esc.halvings(),
            attempts: at.attempts,
            magnitude: at.magnitude,
        });
        let Some(z) = at.last else {
            return Ok(Window::new());
        };
        let s_op = z.target.operand.other();
        self.snaps.insert(run.slot, z);
        Ok(cluster_window(run.cluster, leaves, s_op))
    }

    fn join(
        &mut self,
        snapped: &[(u32, &Cluster<Key>)],
        _leaves: &[Leaf],
        work: &mut Work,
    ) -> Result<Vec<(u32, Window)>, DriveError> {
        // Per snapped side, the clusters' moves and the edges to refit.
        let mut out = Vec::new();
        for s_op in [Operand::A, Operand::B] {
            let body = self.s.body(s_op);
            let br = &body.brep;
            let moves: Vec<(u32, f64, Vec<MovedVert>)> = snapped
                .iter()
                .filter_map(|(k, _)| self.snaps.get(k).map(|z| (k, z)))
                .filter(|(_, z)| z.target.operand.other() == s_op)
                .map(|(k, z)| (*k, z.b_k, self.moved(z)))
                .filter(|m| !m.2.is_empty())
                .collect();
            if moves.is_empty() {
                continue;
            }
            let moved: BTreeMap<VertId, ()> = moves
                .iter()
                .flat_map(|m| m.2.iter().map(|v| (v.vert, ())))
                .collect();
            // Edges inside a rewrite (not kept), or cut by one.
            let mut inside: BTreeMap<EdgeId, bool> = BTreeMap::new();
            for (k, _) in snapped {
                let Some(z) = self.snaps.get(k) else { continue };
                if z.target.operand.other() != s_op {
                    continue;
                }
                for g in &z.geometry.edges {
                    if let SnapEdge::Piece { edge, .. } = g.key
                        && g.class != EdgeClass::Kept
                    {
                        inside.insert(edge, true);
                    }
                }
                for r in &z.rewrite.replaced {
                    inside.entry(r.edge).or_insert(false);
                }
            }
            let coedges: Vec<Vec<(FaceId, varde_decide::ids::CoedgeId)>> = {
                let mut c = vec![Vec::new(); br.edge_count()];
                for f in 0..br.face_count() {
                    let face = FaceId(f as u32);
                    for l in br.loops(face) {
                        for ce in br.loop_coedges(l) {
                            c[br.edge(ce).index()].push((face, ce));
                        }
                    }
                }
                c
            };
            let mut edges: Vec<EdgeId> = Vec::new();
            for e in 0..br.edge_count() {
                let e = EdgeId(e as u32);
                if !br.ends(e).iter().any(|v| moved.contains_key(v)) {
                    continue;
                }
                match inside.get(&e) {
                    Some(true) => continue,
                    Some(false) => {
                        self.cut_skipped += 1;
                        continue;
                    }
                    None => edges.push(e),
                }
            }
            if edges.is_empty() {
                continue;
            }
            let jco: Vec<Vec<JoinCoedge<'_>>> = edges
                .iter()
                .map(|&e| {
                    coedges[e.index()]
                        .iter()
                        .map(|&(f, c)| JoinCoedge {
                            face: f,
                            surface: br.surface(f),
                            pcurve: br.pcurve(c),
                            seam: br.seam_bound(c),
                        })
                        .collect()
                })
                .collect();
            let jedges: Vec<JoinEdge<'_, (), u32>> = edges
                .iter()
                .zip(&jco)
                .map(|(&e, co)| JoinEdge {
                    edge: e,
                    curve: br.curve_id(e),
                    range: br.t_range(e),
                    ends: br.ends(e),
                    tol: br.edge_tol(e),
                    origin: Origin::Plain(()),
                    coedges: co,
                })
                .collect();
            let curves: Vec<JoinCurve<'_>> = edges
                .iter()
                .map(|&e| JoinCurve {
                    id: br.curve_id(e),
                    curve: br.curve(e),
                })
                .collect();
            let cm: Vec<ClusterMoves<'_, u32>> = moves
                .iter()
                .map(|(k, b_k, m)| ClusterMoves {
                    cluster: *k,
                    b_k: *b_k,
                    moved: m,
                })
                .collect();
            let j = join(
                &cm,
                &JoinInput {
                    curves: &curves,
                    edges: &jedges,
                    eps_seam: TARGET,
                },
                work,
            )
            .map_err(DriveError::Join)?;
            out.extend(join_windows(s_op, &j));
            self.joined = Some(j);
        }
        let mut v: Vec<(u32, Window)> = out.into_iter().collect();
        v.sort_by_key(|x| x.0);
        Ok(v)
    }

    fn recertify(
        &self,
        leaf: &Leaf,
        _ledger: &mut Ledger<'_>,
    ) -> Result<Vec<Fresh<Leaf>>, BudgetStop<DriveError>> {
        Ok(vec![Fresh::Leaf(leaf.clone())])
    }
}

/// An operation's snap (module docs).
pub(crate) struct Op<'s, 'a> {
    /// The clusters before the window merge.
    pub(crate) clusters: usize,
    /// The clusters after it.
    pub(crate) merged: usize,
    pub(crate) snapped: Snapped<Leaf, Key>,
    pub(crate) driver: Driver<'s, 'a>,
}

/// The snap of the operation of `s` on `leaves` (module docs).
pub(crate) fn operation<'s, 'a>(
    s: &'s Setup<'a>,
    leaves: Vec<Leaf>,
    sigma_a: Sign,
) -> Result<Op<'s, 'a>, StaleError<DriveError>> {
    let cs = s.clusters(&leaves);
    let n = cs.len();
    // The windows, from a first snap of each cluster as it is.
    let windows: Vec<Vec<Element>> = cs
        .iter()
        .map(|c| {
            let ls: Vec<&Leaf> = c.leaves().map(|i| &leaves[i as usize]).collect();
            snap_leaves(s, &ls, c.kind(), s.b_k, &[], sigma_a)
                .map(|z| z.corner.window)
                .unwrap_or_default()
        })
        .collect();
    let merged: Vec<Cluster<Key>> = merge_sharing(cs, &windows, &mut work())
        .map_err(StaleError::Cluster)?
        .into_iter()
        .map(|m| m.cluster)
        .collect();
    let m = merged.len();
    let mut driver = Driver::new(s, sigma_a);
    let snapped = snap_clusters(merged, leaves, &mut driver, &mut work())?;
    Ok(Op {
        clusters: n,
        merged: m,
        snapped,
        driver,
    })
}

impl From<KernelError> for DriveError {
    fn from(e: KernelError) -> Self {
        DriveError::Escalate(format!("{e:?}"))
    }
}
