//! One snap run on two built bodies: clusters, target, then per cluster
//! the rim, the corner rule, `SnapAlong`, the rewrite and its geometry,
//! the elements on the target and the zone's decision, every pass's
//! input made from the stores.
//!
//! **Carrying the target into a snapped face's UV.** A target point is
//! carried along the cluster's `ℓ` onto the snapped face's plane exactly
//! (the plane through its lattice origin, normal its key's), rounded to
//! `f64`, and given its UV on the snapped face's lattice as the store
//! does ([`uv_on`]), so a point both bodies share gets the same bits on
//! both. The target faces a snapped face's leaves reach bound its target
//! part: their loops' edges used by one of them are its target trims,
//! those used by two its target cuts.
//!
//! **Bands.** An operand edge's band is its tolerance plus `ρ_op`, a
//! vertex's alike; a rim piece along box sides, and the rewrite's new
//! edges and vertices, get `b_K/2 + ρ_op`: the mid-surface they go to is
//! within half the gap of either sheet.

use std::collections::{BTreeMap, BTreeSet};

use malachite::Rational;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;
use varde_decide::count::boxes::UvSpan;
use varde_decide::ids::{CoedgeId, EdgeId, FaceId, VertId};
use varde_decide::leaves::flat::Leaf;
use varde_decide::num::decide::Sign;
use varde_decide::records::keys::Operand;
use varde_decide::records::near::OpBands;

use super::super::along::{AlongError, AlongInput, SnapAlong, snap_along};
use super::super::cluster::{Cluster, ClusterLeaf, Tangency, clusters};
use super::super::corner::{Corner, CornerError, CornerInput, corner};
use super::super::cuts::{
    Count, Curve, CurvePiece, Cut, CutError, CutFailure, CutInput, ZoneCurve, check,
};
use super::super::decide::{DecideError, DecideInput, Zone, decide_zone};
use super::super::rewrite::{
    GeomInput, Geometry, Rewrite, RewriteError, TargetCut, TargetEdge, TargetFace, geometry,
    rewrite,
};
use super::super::rim::{
    Along, Element, Implicit, Rim, RimError, RimInput, RimTrim, SnappedFace, add_inner_target, rim,
};
use super::super::stale::{OnT, OnTError, OnTInput, on_t};
use super::super::target::{Candidate, Map, Target, TargetError, choose};
use super::{Body, Key, Order, TARGET, q};
use crate::geom::curve::PCurve;
use crate::geom::lattice::CELL;
use crate::infra::budget::{Budget, Work};
use crate::topo::brep_min::planar::UV_GRID;

type Q = Rational;

/// The work budget of one run.
pub(crate) const BUDGET: u64 = 1 << 26;

pub(crate) fn work() -> Work {
    Work::new(&Budget::new(BUDGET))
}

/// The two bodies of a run and what every pass shares.
pub(crate) struct Setup<'a> {
    pub(crate) a: &'a Body,
    pub(crate) b: &'a Body,
    pub(crate) order: Order<'a>,
    pub(crate) bands: OpBands,
    pub(crate) b_k: f64,
    /// Whether the rim's window takes the target edges inside the target
    /// faces' part ([`add_inner_target`]); without them a snapped edge
    /// lying on such an edge is fitted as a new edge along it, with no
    /// record linking the two.
    pub(crate) inner_target: bool,
    /// Per body, per edge, its coedges (face, coedge), in face order.
    coedges: [Vec<Vec<(FaceId, CoedgeId)>>; 2],
}

impl<'a> Setup<'a> {
    pub(crate) fn new(a: &'a Body, b: &'a Body, b_k: f64) -> Setup<'a> {
        let m = |x: &Body| x.brep.max_coord();
        let (ma, mb) = (m(a), m(b));
        let bands = OpBands::new(if ma > mb { ma } else { mb }).expect("finite");
        let coedges = [a, b].map(|x| {
            let mut out = vec![Vec::new(); x.brep.edge_count()];
            for f in 0..x.brep.face_count() {
                let face = FaceId(f as u32);
                for l in x.brep.loops(face) {
                    for c in x.brep.loop_coedges(l) {
                        out[x.brep.edge(c).index()].push((face, c));
                    }
                }
            }
            out
        });
        Setup {
            a,
            b,
            order: Order { a, b },
            bands,
            b_k,
            inner_target: true,
            coedges,
        }
    }

    pub(crate) fn body(&self, o: Operand) -> &'a Body {
        match o {
            Operand::A => self.a,
            Operand::B => self.b,
        }
    }

    fn coedges_of(&self, o: Operand, e: EdgeId) -> &[(FaceId, CoedgeId)] {
        &self.coedges[o as usize][e.index()]
    }

    /// An operand element's band (module docs).
    pub(crate) fn band(&self, x: Element) -> f64 {
        let tol = match x {
            Element::Edge(o, e) => self.body(o).brep.edge_tol(e),
            Element::Vert(o, v) => self.body(o).brep.vert_tol(v),
        };
        self.bands.band(tol).expect("a finite tolerance")
    }

    /// The band of a new element of a snap at gap bound `b_k` (module
    /// docs).
    pub(crate) fn new_band(&self, b_k: f64) -> f64 {
        self.bands.band(b_k / 2.0).expect("finite")
    }

    /// The clusters of `leaves`, faces keyed by their planes.
    pub(crate) fn clusters(&self, leaves: &[Leaf]) -> Vec<Cluster<Key>> {
        clusters(leaves, |o, f| self.body(o).face_key(f), &mut work()).expect("clusters")
    }
}

/// `p` carried along `ell` onto the plane of face `f` of `body` (module
/// docs), rounded.
fn carry(body: &Body, f: FaceId, ell: &[Q; 3], p: [f64; 3]) -> [f64; 3] {
    let lat = body.brep.surface(f).lattice();
    let n = match lat.key() {
        crate::geom::form::FormKey::Plane(k) => k.normal().map(q),
        _ => panic!("a plane"),
    };
    let o = lat.origin().map(q);
    let pq = p.map(q);
    let dot = |a: &[Q; 3], b: &[Q; 3]| (0..3).fold(Q::from(0), |s, k| s + &a[k] * &b[k]);
    let d = [0, 1, 2].map(|k| &o[k] - &pq[k]);
    let t = dot(&n, &d) / dot(&n, ell);
    [0, 1, 2].map(|k| f64::rounding_from(&(&pq[k] + &t * &ell[k]), Nearest).0)
}

/// `p`'s UV on face `f`'s plane as the store gives it, on the UV grid.
pub(crate) fn uv_on(body: &Body, f: FaceId, p: [f64; 3]) -> [f64; 2] {
    let lat = body.brep.surface(f).lattice();
    let (o, [u, v, _]) = (lat.origin(), lat.axes());
    let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
    let along = |a: [f64; 3]| {
        let x = (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5;
        (x / UV_GRID).round() * UV_GRID
    };
    [along(u), along(v)]
}

/// The straight UV span from `a` to `b`, its middle the midpoint.
fn uv_span(a: [f64; 2], b: [f64; 2]) -> UvSpan {
    let m = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
    [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]
}

const KNOTS: [f64; 2] = [0.0, 1.0];

/// Face `f`'s outward plane: its key's, negated where the face is
/// flipped against it.
fn outward(body: &Body, f: FaceId) -> ([f64; 3], f64) {
    let k = match body.brep.surface(f).lattice().key() {
        crate::geom::form::FormKey::Plane(k) => *k,
        _ => panic!("a plane"),
    };
    if body.brep.flipped(f) {
        (k.normal().map(|x| -x), -k.offset())
    } else {
        (k.normal(), k.offset())
    }
}

/// Face `f`'s key normal: the one its UV turns counter-clockwise about.
pub(crate) fn key_normal(body: &Body, f: FaceId) -> [f64; 3] {
    match body.brep.surface(f).lattice().key() {
        crate::geom::form::FormKey::Plane(k) => k.normal(),
        _ => panic!("a plane"),
    }
}

/// A trim carried into a snapped face's UV, owned.
struct Carried {
    edge: EdgeId,
    ends: [VertId; 2],
    spans: [UvSpan; 1],
    left: bool,
    /// For a cut: the faces on its left and right.
    sides: Option<[FaceId; 2]>,
}

/// An own trim, owned: its edge, ends, spans, knots and side.
type OwnTrim = (EdgeId, [VertId; 2], Vec<UvSpan>, Vec<f64>, bool);

/// What a snapped face reads, owned.
struct FaceData {
    face: FaceId,
    tiles: [([i64; 2], varde_decide::bezier::HomPatch<Q>); 1],
    own: Vec<OwnTrim>,
    target: Vec<Carried>,
    cuts: Vec<Carried>,
}

/// Why a cluster's snap stopped, by pass.
#[derive(Debug)]
#[allow(dead_code, reason = "read through `Debug` in the test reports")]
pub(crate) enum Stop {
    Target(TargetError),
    /// No candidate target certifies.
    NoTarget,
    /// The map isn't along a direction (a closest point).
    Closest,
    Rim(RimError),
    Corner(CornerError),
    Along(AlongError),
    Rewrite(RewriteError),
    Geometry(RewriteError),
    OnT(OnTError),
    Decide(DecideError),
}

/// One cluster's snap, every pass's outcome.
#[derive(Debug)]
#[allow(dead_code, reason = "read through `Debug` in the test reports")]
pub(crate) struct Snap {
    pub(crate) kind: Tangency,
    pub(crate) target: Target,
    pub(crate) faces: Vec<FaceId>,
    pub(crate) targets: Vec<(FaceId, Implicit)>,
    pub(crate) rim: Rim,
    pub(crate) corner: Corner,
    pub(crate) along: SnapAlong,
    pub(crate) rewrite: Rewrite,
    pub(crate) geometry: Geometry,
    pub(crate) on: OnT,
    pub(crate) zone: Zone,
    pub(crate) b_k: f64,
    pub(crate) work: u64,
}

/// The snap of the cluster of `ls`, all of tangency `kind`, at gap bound
/// `b_k`, the vertices `fixed` placed by earlier clusters, at the
/// operation's `sigma_a` (module docs).
pub(crate) fn snap_leaves(
    s: &Setup<'_>,
    ls: &[&Leaf],
    kind: Tangency,
    b_k: f64,
    fixed: &[(Operand, VertId)],
    sigma_a: Sign,
) -> Result<Snap, Stop> {
    let mut w = work();
    let target = choose(kind, ls, [Candidate::default(); 2], &mut w)
        .map_err(Stop::Target)?
        .ok_or(Stop::NoTarget)?;
    let Map::Along(ell) = &target.map else {
        return Err(Stop::Closest);
    };
    let t_op = target.operand;
    let s_op = t_op.other();
    let (sb, tb) = (s.body(s_op), s.body(t_op));
    // The faces of each side, and per snapped face the target faces its
    // leaves reach.
    let mut reach: BTreeMap<FaceId, BTreeSet<FaceId>> = BTreeMap::new();
    let mut t_faces: BTreeSet<FaceId> = BTreeSet::new();
    for l in ls {
        let [x, y] = l.boxes();
        let (bs, bt) = if x.operand() == s_op { (x, y) } else { (y, x) };
        reach.entry(bs.face()).or_default().insert(bt.face());
        t_faces.insert(bt.face());
    }
    // Carried along `ℓ`, a target face's loop turns in the snapped face's
    // UV as about its outward normal where that normal and the snapped
    // face's key normal lie on one side of `ℓ⊥` (exactly).
    let side_of_ell = |n: [f64; 3]| {
        let d = (0..3).fold(Q::from(0), |acc, k| acc + q(n[k]) * &ell[k]);
        d > 0u32
    };
    let preserved = |sf: FaceId, tf: FaceId| {
        let (nt, _) = outward(tb, tf);
        side_of_ell(nt) == side_of_ell(key_normal(sb, sf))
    };
    let mut data: Vec<FaceData> = Vec::new();
    // Every snapped face reads the cluster's whole target part: where the
    // target folds across an edge both its faces take, that edge is a cut,
    // not a trim, on every snapped face.
    for &sf in reach.keys() {
        let tfs = &t_faces;
        let br = &sb.brep;
        let mut own = Vec::new();
        for l in br.loops(sf) {
            for c in br.loop_coedges(l) {
                let e = br.edge(c);
                let pc = br.pcurve(c);
                own.push((
                    e,
                    br.ends(e),
                    pc.spans().to_vec(),
                    pc.knots().to_vec(),
                    br.reversed(c) == br.flipped(sf),
                ));
            }
        }
        let mut target = Vec::new();
        let mut cuts = Vec::new();
        let mut seen: BTreeSet<EdgeId> = BTreeSet::new();
        for &tf in tfs {
            let tr = &tb.brep;
            for l in tr.loops(tf) {
                for c in tr.loop_coedges(l) {
                    let e = tr.edge(c);
                    let users: Vec<FaceId> = s
                        .coedges_of(t_op, e)
                        .iter()
                        .map(|x| x.0)
                        .filter(|f| tfs.contains(f))
                        .collect();
                    let ends = tr.ends(e);
                    let [p0, p1] = ends.map(|v| carry(sb, sf, ell, tr.pos(v)));
                    let span = [uv_span(uv_on(sb, sf, p0), uv_on(sb, sf, p1))];
                    let left = (!tr.reversed(c)) == preserved(sf, tf);
                    if users.len() >= 2 {
                        if !seen.insert(e) {
                            continue;
                        }
                        let other = users.iter().copied().find(|&f| f != tf).expect("two");
                        cuts.push(Carried {
                            edge: e,
                            ends,
                            spans: span,
                            left: true,
                            sides: Some(if left { [tf, other] } else { [other, tf] }),
                        });
                    } else {
                        target.push(Carried {
                            edge: e,
                            ends,
                            spans: span,
                            left,
                            sides: None,
                        });
                    }
                }
            }
        }
        data.push(FaceData {
            face: sf,
            tiles: [([0, 0], sb.tiles[sf.0 as usize].clone())],
            own,
            target,
            cuts,
        });
    }
    let own_trims: Vec<Vec<RimTrim<'_>>> = data
        .iter()
        .map(|d| {
            d.own
                .iter()
                .map(|(e, ends, spans, knots, left)| RimTrim {
                    edge: *e,
                    ends: *ends,
                    spans,
                    knots,
                    left: *left,
                })
                .collect()
        })
        .collect();
    let target_trims: Vec<Vec<RimTrim<'_>>> = data
        .iter()
        .map(|d| {
            d.target
                .iter()
                .map(|t| RimTrim {
                    edge: t.edge,
                    ends: t.ends,
                    spans: &t.spans,
                    knots: &KNOTS,
                    left: t.left,
                })
                .collect()
        })
        .collect();
    let faces: Vec<SnappedFace<'_>> = data
        .iter()
        .enumerate()
        .map(|(k, d)| SnappedFace {
            face: d.face,
            tiles: &d.tiles,
            own: &own_trims[k],
            target: &target_trims[k],
            flipped: sb.brep.flipped(d.face),
        })
        .collect();
    let cuts: Vec<TargetCut<'_>> = data
        .iter()
        .flat_map(|d| {
            d.cuts.iter().map(move |c| TargetCut {
                face: d.face,
                edge: c.edge,
                ends: c.ends,
                spans: &c.spans,
                knots: &KNOTS,
                sides: c.sides.expect("a cut's sides"),
            })
        })
        .collect();
    let targets: Vec<(FaceId, Implicit)> = t_faces
        .iter()
        .map(|&f| {
            let (normal, offset) = outward(tb, f);
            (f, Implicit::Plane { normal, offset })
        })
        .collect();
    let nb = s.new_band(b_k);
    let rim_band = |a: Along| match a {
        Along::Box => nb,
        Along::Own(e) => s.band(Element::Edge(s_op, e)),
        Along::Target(e) => s.band(Element::Edge(t_op, e)),
    };
    let rim_input = RimInput {
        snapped: s_op,
        faces: &faces,
        targets: &targets,
        band: &rim_band,
    };
    let mut rm = rim(ls, &rim_input, &mut w).map_err(Stop::Rim)?;
    let inner: Vec<(FaceId, RimTrim<'_>)> = cuts
        .iter()
        .map(|c| {
            (
                c.face,
                RimTrim {
                    edge: c.edge,
                    ends: c.ends,
                    spans: c.spans,
                    knots: c.knots,
                    left: true,
                },
            )
        })
        .collect();
    if s.inner_target {
        add_inner_target(&mut rm, t_op, &inner).map_err(Stop::Rim)?;
    }
    // The corner rule's candidates: the snapped faces' edges and vertices.
    let mut cands: BTreeSet<Element> = BTreeSet::new();
    for d in &data {
        for (e, ends, ..) in &d.own {
            cands.insert(Element::Edge(s_op, *e));
            for v in ends {
                cands.insert(Element::Vert(s_op, *v));
            }
        }
    }
    let cands: Vec<Element> = cands.into_iter().collect();
    let band = |x: Element| s.band(x);
    let ci = CornerInput {
        b_k,
        order: &s.order,
        band: &band,
        candidates: &cands,
        fixed,
        groups: &[],
        coincident: &[],
    };
    let cr = corner(&rm, &rim_input, &ci, &mut w).map_err(Stop::Corner)?;
    let unrelated = |_: EdgeId, _: EdgeId| false;
    let ai = AlongInput {
        b_k,
        order: &s.order,
        band: &band,
        map: &target.map,
        same_form: &unrelated,
    };
    let al = snap_along(&rm, &rim_input, &cr, &ai, &mut w).map_err(Stop::Along)?;
    let rw = rewrite(&rm, &rim_input, &cr, &al, &cuts, &mut w).map_err(Stop::Rewrite)?;
    // The target's faces and edges as the geometry reads them.
    let t_list: Vec<FaceId> = t_faces.iter().copied().collect();
    let tface: Vec<TargetFace<'_>> = t_list
        .iter()
        .map(|&f| TargetFace {
            face: f,
            tile: [0, 0],
            patch: &tb.tiles[f.0 as usize],
        })
        .collect();
    let mut t_edges: BTreeSet<EdgeId> = BTreeSet::new();
    for &f in &t_list {
        for l in tb.brep.loops(f) {
            for c in tb.brep.loop_coedges(l) {
                t_edges.insert(tb.brep.edge(c));
            }
        }
    }
    let t_pcurves: Vec<(EdgeId, Vec<(FaceId, PCurve)>)> = t_edges
        .iter()
        .map(|&e| {
            let pcs = s
                .coedges_of(t_op, e)
                .iter()
                .map(|&(f, c)| (f, tb.brep.pcurve(c).clone()))
                .collect();
            (e, pcs)
        })
        .collect();
    let tedges: Vec<TargetEdge<'_>> = t_pcurves
        .iter()
        .map(|(e, pcs)| TargetEdge {
            edge: *e,
            curve: tb.brep.curve(*e),
            range: tb.brep.t_range(*e),
            pcurves: pcs,
        })
        .collect();
    let gi = GeomInput {
        order: &s.order,
        map: &target.map,
        faces: &tface,
        edges: &tedges,
        eps_seam: TARGET,
    };
    let geo =
        geometry(&rw, &rm, &rim_input, &cr, &al, &cuts, &gi, &mut w).map_err(Stop::Geometry)?;
    let eb = |_| nb;
    let vb = |_| nb;
    let on = on_t(
        &rw,
        &geo,
        &OnTInput {
            snapped: s_op,
            faces: &tface,
            edge_band: &eb,
            vert_band: &vb,
        },
        &mut w,
    )
    .map_err(Stop::OnT)?;
    let zone = decide_zone(
        &rm,
        &cr,
        &rw,
        &geo,
        &on,
        &DecideInput {
            kind,
            sigma_a,
            snapped: s_op,
            targets: &targets,
            faces: &tface,
        },
        &mut w,
    )
    .map_err(Stop::Decide)?;
    Ok(Snap {
        kind,
        target,
        faces: reach.keys().copied().collect(),
        targets,
        rim: rm,
        corner: cr,
        along: al,
        rewrite: rw,
        geometry: geo,
        on,
        zone,
        b_k,
        work: w.used(),
    })
}

/// The zone's cut-point check on its rim records (the corner records at
/// its ties and its branch vertices): the trim boolean's cells are not
/// given, the sheets' regions having no winding in the other operand.
pub(crate) fn rim_cut_points(z: &Snap) -> Result<Option<CutFailure<u32>>, CutError> {
    // Each rim edge with records: a zone curve of one piece, end to end.
    let pieces: Vec<[CurvePiece<'_>; 1]> = z
        .zone
        .rim
        .edges
        .iter()
        .filter_map(|r| {
            let e = z.rewrite.edges.iter().find(|e| e.key == r.edge)?;
            Some([CurvePiece {
                from: Cut::Vert(e.ends[0]),
                to: Cut::Vert(e.ends[1]),
                trim: &[],
                count: Count::Rim {
                    target: *r.targets.first()?,
                },
            }])
        })
        .collect();
    let mut curves: Vec<ZoneCurve<'_>> = z
        .zone
        .rim
        .edges
        .iter()
        .zip(&pieces)
        .map(|(r, p)| ZoneCurve {
            curve: Curve::Snap(r.edge),
            pieces: p,
        })
        .collect();
    curves.sort_by_key(|c| c.curve);
    curves.dedup_by_key(|c| c.curve);
    check(
        &0u32,
        &CutInput {
            rim: &z.zone.rim,
            faces: &[],
            curves: &curves,
            rung: 0,
        },
        &mut work(),
    )
}
