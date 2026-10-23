//! A snapped zone's trim boolean on each target face it reaches: the
//! face's own edges and the rewrite's edges on it realised by
//! [`realise`] (the rewrite's edges reusing a target edge run along it,
//! the others are transversal arcs), then its zero-area regions dropped
//! or folded by [`fold`].
//!
//! **Windings.** A region's winding counts the sheets covering it on the
//! face: the target face itself and the snapped faces on it. A target
//! edge steps by one where the face lies on its left; a rewrite edge by
//! the snapped faces using it, each one where it lies on the edge's
//! left in the target face's UV (the rewrite's loops run counter-clockwise
//! in the snapped face's UV, turned where the two UVs turn opposite ways
//! about the shared plane).
//!
//! **What the fold reads.** Side order within a bundle: the target edge's
//! piece rightmost, then the arcs in arc order (the snap carries the
//! snapped side onto the target from its own side, so its copy lies
//! next to the target's). No zero-length edge is contracted. Sources: the
//! target operand's lines, arcs new, vertices the target's where they are
//! its own, else new, each in input order. Across each piece: the other
//! target face across a line that two target faces share, none else.
//! Operand edges: each line the target edge it lies on, each arc the
//! snapped edge it is a piece of, none for the rim's and the cuts' arcs;
//! the fold reads from the stores whether they lie on an open boundary.
//!
//! **Cut points** ([`cut_points`]). The zone's cut-point check on the faces
//! folded: the rim records and the cells, each cell labelled by the snapped
//! sheet's coverage (the sheets' regions have no winding in the other
//! operand), each edge of the snapped sheet's open boundary counting its
//! coverage step from the store.

use std::collections::BTreeMap;

use varde_decide::ids::{EdgeId, FaceId, VertId};
use varde_decide::records::keys::Operand;

use super::super::corner::Placement;
use super::super::cuts::{
    Count, Curve, CurvePiece, Cut, CutError, CutFace, CutFailure, CutInput, TrimAt, ZoneCurve,
    check,
};
use super::super::rewrite::{EdgeCurve, SnapEdge, SnapVert};
use super::run::{Setup, Snap, work};
use crate::boolean::arrange::coincident::{
    Along as LineAlong, Arc, Coincident, CoincidentError, FaceInput, Line, PieceOf, Place, ZeroOf,
    realise,
};
use crate::boolean::arrange::fold::{
    Fate, Fold, FoldError, FoldInput, Operand as FoldOperand, OperandEdge, Source, fold,
};
use crate::geom::pcurve::PCurve;

/// A target face of a zone, realised and folded.
#[derive(Debug)]
#[allow(dead_code, reason = "read through `Debug` in the test reports")]
pub(crate) struct FaceZone {
    pub(crate) face: FaceId,
    pub(crate) realised: Coincident,
    pub(crate) fold: Fold,
    /// Per line, its target edge.
    pub(crate) line_edges: Vec<EdgeId>,
    /// The rewrite edges given as arcs, their target edge where they run
    /// along one.
    pub(crate) arcs: Vec<Option<EdgeId>>,
    /// Per arc, the rewrite edge it is; none for a zero-length join.
    pub(crate) arc_keys: Vec<Option<SnapEdge>>,
}

impl FaceZone {
    /// The zero-area regions kept (folded) and discarded (dropped).
    pub(crate) fn fates(&self) -> [usize; 2] {
        let folded = self
            .fold
            .regions
            .iter()
            .filter(|r| matches!(r.fate, Fate::Folded(_)))
            .count();
        [folded, self.fold.regions.len() - folded]
    }
}

/// An arc's ends, the line it runs along (and whether forward), and its
/// winding step.
type ArcData = ([u32; 2], Option<(u32, bool)>, i32);

/// Why a target face's trim boolean can't be made.
#[derive(Debug)]
#[allow(dead_code, reason = "read through `Debug` in the test reports")]
pub(crate) enum ZoneError {
    /// A rewrite vertex on the face has no point there.
    Vert(SnapVert),
    Realise(FaceId, CoincidentError),
    Fold(FaceId, FoldError),
}

/// Whether two faces' UVs turn one way seen along the zone's `ℓ`: their
/// key normals on one side of `ℓ⊥`, exactly.
fn same_turn(s: &Setup<'_>, z: &Snap, a: (Operand, FaceId), b: (Operand, FaceId)) -> bool {
    let super::super::target::Map::Along(ell) = &z.target.map else {
        panic!("a direction")
    };
    let side = |(o, f): (Operand, FaceId)| {
        let n = super::run::key_normal(s.body(o), f);
        (0..3).fold(malachite::Rational::from(0), |acc, k| {
            acc + super::q(n[k]) * &ell[k]
        }) > 0u32
    };
    side(a) == side(b)
}

/// The trim boolean of zone `z` on each target face it reaches, regions
/// of winding `w` kept where `keep(w)` (module docs).
pub(crate) fn faces(
    s: &Setup<'_>,
    z: &Snap,
    keep: &(dyn Fn(i64) -> bool + Sync),
) -> Result<Vec<FaceZone>, ZoneError> {
    let t_op = z.target.operand;
    let s_op = t_op.other();
    let tb = s.body(t_op);
    let br = &tb.brep;
    let geo_e: BTreeMap<_, _> = z.geometry.edges.iter().map(|e| (e.key, e)).collect();
    let geo_v: BTreeMap<_, _> = z.geometry.verts.iter().map(|v| (v.key, v)).collect();
    let mut out = Vec::new();
    for &(t, _) in &z.targets {
        // The face's own vertices and lines.
        let mut verts: Vec<Place> = Vec::new();
        let mut vsrc: Vec<Source> = Vec::new();
        let mut vert_of: BTreeMap<SnapVert, u32> = BTreeMap::new();
        let mut lines_data: Vec<(EdgeId, [u32; 2], PCurve, i32)> = Vec::new();
        let mut own_vert: BTreeMap<VertId, u32> = BTreeMap::new();
        let fop = match t_op {
            Operand::A => FoldOperand::A,
            Operand::B => FoldOperand::B,
        };
        for l in br.loops(t) {
            for c in br.loop_coedges(l) {
                let e = br.edge(c);
                let ends = br.ends(e);
                let mut ix = [0u32; 2];
                for (k, v) in ends.iter().enumerate() {
                    let n = *own_vert.entry(*v).or_insert_with(|| {
                        let p = br.pcurve(c);
                        let uv = if k == 0 {
                            p.knot_point(0)
                        } else {
                            p.knot_point(p.spans().len())
                        }
                        .expect("a p-curve end");
                        verts.push(Place::Uv(uv));
                        vsrc.push(Source {
                            operand: fop,
                            order: v.0,
                        });
                        (verts.len() - 1) as u32
                    });
                    ix[k] = n;
                    vert_of.insert(SnapVert::Vert(t_op, *v), n);
                }
                let left = br.reversed(c) == br.flipped(t);
                lines_data.push((e, ix, br.pcurve(c).clone(), if left { 1 } else { -1 }));
            }
        }
        let line_of: BTreeMap<EdgeId, u32> = lines_data
            .iter()
            .enumerate()
            .map(|(k, l)| (l.0, k as u32))
            .collect();
        // The rewrite's edges on the face, with their windings.
        let mut wind: BTreeMap<_, i32> = BTreeMap::new();
        let mut order: Vec<_> = Vec::new();
        for f in &z.rewrite.faces {
            if f.on != (t_op, t) {
                continue;
            }
            let sf = match f.key {
                super::super::rewrite::SnapFace::Snapped { face, .. }
                | super::super::rewrite::SnapFace::Kept { face, .. } => face,
            };
            let turn = if same_turn(s, z, (s_op, sf), (t_op, t)) {
                1
            } else {
                -1
            };
            for l in &f.loops {
                for u in l {
                    let w = wind.entry(u.edge).or_insert_with(|| {
                        order.push(u.edge);
                        0
                    });
                    *w += turn * if u.forward { 1 } else { -1 };
                }
            }
        }
        let mut arcs_data: Vec<ArcData> = Vec::new();
        let mut arc_edges: Vec<Option<EdgeId>> = Vec::new();
        let mut arc_on: Vec<Option<OperandEdge>> = Vec::new();
        let mut arc_keys: Vec<Option<SnapEdge>> = Vec::new();
        for k in &order {
            let e = z
                .rewrite
                .edges
                .iter()
                .find(|e| e.key == *k)
                .expect("an edge");
            let g = geo_e.get(k).copied();
            // Its ends' places.
            let mut ix = [0u32; 2];
            for (j, v) in e.ends.iter().enumerate() {
                if let Some(&n) = vert_of.get(v) {
                    ix[j] = n;
                    continue;
                }
                // A vertex the corner rule sent onto one of the face's
                // own vertices is at that vertex's point.
                let place =
                    z.rewrite
                        .verts
                        .iter()
                        .find(|x| x.key == *v)
                        .and_then(|x| match x.moved.to {
                            Placement::Vert(o, id) if o == t_op => {
                                own_vert.get(&id).map(|&n| Place::At(n))
                            }
                            _ => None,
                        });
                let place = match place {
                    Some(p) => p,
                    None => {
                        let gv = geo_v.get(v).ok_or(ZoneError::Vert(*v))?;
                        let on = gv
                            .on
                            .iter()
                            .find_map(|(te, s)| line_of.get(te).map(|&li| (li, *s)));
                        match on {
                            Some((li, s))
                                if lines_data[li as usize].2.knots()[0] < s
                                    && s < *lines_data[li as usize]
                                        .2
                                        .knots()
                                        .last()
                                        .expect("knots") =>
                            {
                                Place::On { line: li, s }
                            }
                            _ => Place::Uv(uv_of(s, t_op, t, gv.pos)),
                        }
                    }
                };
                verts.push(place);
                vsrc.push(Source {
                    operand: FoldOperand::New,
                    order: verts.len() as u32,
                });
                let n = (verts.len() - 1) as u32;
                vert_of.insert(*v, n);
                ix[j] = n;
            }
            if ix[0] == ix[1] {
                continue;
            }
            let along = match g.map(|g| &g.curve) {
                Some(EdgeCurve::Reused { edge, t }) => {
                    line_of.get(edge).map(|&li| (li, t[0] < t[1]))
                }
                _ => None,
            };
            arcs_data.push((ix, along, wind[k]));
            arc_keys.push(Some(*k));
            arc_edges.push(along.map(|(li, _)| lines_data[li as usize].0));
            arc_on.push(match k {
                SnapEdge::Piece { edge, .. } => Some((s_op, *edge)),
                _ => None,
            });
        }
        // Each rewrite vertex the corner rule put on one of the face's own
        // vertices: a zero-length edge from that vertex, the two records of
        // one operand vertex, contracted.
        let mut joins: Vec<u32> = Vec::new();
        for (v, &n) in &vert_of {
            if let Place::At(m) = verts[n as usize]
                && !matches!(v, SnapVert::Vert(o, _) if *o == t_op)
            {
                joins.push(arcs_data.len() as u32);
                arcs_data.push(([m, n], None, 0));
                arc_keys.push(None);
                arc_edges.push(None);
                arc_on.push(None);
            }
        }
        let lines: Vec<Line<'_>> = lines_data
            .iter()
            .map(|(_, ends, pc, w)| Line {
                ends: *ends,
                pcurve: pc,
                wind: *w,
            })
            .collect();
        let alongs: Vec<Vec<LineAlong>> = arcs_data
            .iter()
            .map(|(_, a, _)| {
                a.map(|(line, forward)| vec![LineAlong { line, forward }])
                    .unwrap_or_default()
            })
            .collect();
        let arcs: Vec<Arc<'_>> = arcs_data
            .iter()
            .zip(&alongs)
            .map(|((ends, _, w), al)| Arc {
                ends: *ends,
                along: al,
                wind: *w,
            })
            .collect();
        let input = FaceInput {
            verts: &verts,
            lines: &lines,
            arcs: &arcs,
        };
        let realised = realise(&input, &mut work()).map_err(|e| ZoneError::Realise(t, e))?;
        // Side ranks: the line's piece first, then the arcs', per bundle.
        let mut side = vec![0u32; realised.pieces.len()];
        for b in &realised.bundles {
            for (r, &m) in b.members.iter().enumerate() {
                side[m as usize] = r as u32;
            }
        }
        let lsrc: Vec<Source> = lines_data
            .iter()
            .enumerate()
            .map(|(k, _)| Source {
                operand: fop,
                order: k as u32,
            })
            .collect();
        let arc_order: Vec<u32> = (0..arcs.len() as u32).collect();
        let line_on: Vec<Option<OperandEdge>> =
            lines_data.iter().map(|l| Some((t_op, l.0))).collect();
        let across: Vec<u32> = realised
            .pieces
            .iter()
            .map(|p| match p.of {
                crate::boolean::arrange::coincident::PieceOf::Line { line, .. } => {
                    let e = lines_data[line as usize].0;
                    u32::from(faces_of(s, t_op, e) > 1)
                }
                _ => 0,
            })
            .collect();
        let contract: Vec<u32> = realised
            .zeros
            .iter()
            .enumerate()
            .filter(|(_, z)| matches!(z.of, ZeroOf::Arc(a) if joins.contains(&a)))
            .map(|(k, _)| k as u32)
            .collect();
        let fi = FoldInput {
            face: &input,
            realised: &realised,
            side: &side,
            contract: &contract,
            lines: &lsrc,
            arcs: &arc_order,
            verts: &vsrc,
            across: &across,
            stores: [&s.a.brep, &s.b.brep],
            line_edges: &line_on,
            arc_edges: &arc_on,
            keep,
        };
        let fd = fold(&fi, &mut work()).map_err(|e| ZoneError::Fold(t, e))?;
        out.push(FaceZone {
            face: t,
            realised,
            fold: fd,
            line_edges: lines_data.iter().map(|l| l.0).collect(),
            arcs: arc_edges,
            arc_keys,
        });
    }
    Ok(out)
}

/// The faces of `o` using edge `e`.
fn faces_of(s: &Setup<'_>, o: Operand, e: EdgeId) -> usize {
    let br = &s.body(o).brep;
    let mut n = 0;
    for f in 0..br.face_count() {
        for l in br.loops(FaceId(f as u32)) {
            n += br.loop_coedges(l).filter(|&c| br.edge(c) == e).count();
        }
    }
    n
}

/// Point `p`'s UV on face `f` of `o`.
fn uv_of(s: &Setup<'_>, o: Operand, f: FaceId, p: [f64; 3]) -> [f64; 2] {
    super::run::uv_on(s.body(o), f, p)
}

/// A zone curve as the cut-point check's cells part reads it on the
/// sheets (module docs of [`cut_points`]): its ends, its count, and the
/// trim boolean's pieces it is realised as.
#[derive(Clone, Debug)]
pub(crate) struct CellCurve {
    pub(crate) curve: Curve,
    pub(crate) ends: [SnapVert; 2],
    pub(crate) count: i64,
    pub(crate) trim: Vec<TrimAt>,
}

/// Why a zone's curves can't be counted on the sheets.
#[derive(Debug)]
#[allow(dead_code, reason = "read through `Debug` in the test reports")]
pub(crate) enum CellsError {
    /// A rim piece along box sides: the label beyond it is the gap's, which
    /// the sheets' coverage doesn't give.
    BoxSide(SnapEdge),
    /// A trim piece of no rewrite edge, or a rewrite edge not in the
    /// rewrite.
    Unknown(u32, u32),
    /// More than `u32` faces or pieces.
    TooMany,
}

/// The coverage step across the open-boundary edge `e` of the snapped
/// sheet on target face `t`, from the store: `+1` where its one face lies
/// on its left in `t`'s UV (along its stored direction), `−1` on its
/// right; `0` for an edge with two faces.
fn open_step(s: &Setup<'_>, z: &Snap, e: EdgeId, t: FaceId) -> i64 {
    let t_op = z.target.operand;
    let s_op = t_op.other();
    let [(sf, c)] = s.coedges_of(s_op, e) else {
        return 0;
    };
    let br = &s.body(s_op).brep;
    let left_s = br.reversed(*c) == br.flipped(*sf);
    if left_s == same_turn(s, z, (s_op, *sf), (t_op, t)) {
        1
    } else {
        -1
    }
}

/// The zone curves of `z` on its target faces `faces` as realised, each
/// counted (module docs of [`cut_points`]).
pub(crate) fn cell_curves(
    s: &Setup<'_>,
    z: &Snap,
    faces: &[FaceZone],
) -> Result<Vec<CellCurve>, CellsError> {
    let t_op = z.target.operand;
    let br = &s.body(t_op).brep;
    let mut out: Vec<CellCurve> = Vec::new();
    let mut at: BTreeMap<Curve, usize> = BTreeMap::new();
    for (fi, fz) in faces.iter().enumerate() {
        let fi = u32::try_from(fi).map_err(|_| CellsError::TooMany)?;
        for (pi, p) in fz.realised.pieces.iter().enumerate() {
            let pi = u32::try_from(pi).map_err(|_| CellsError::TooMany)?;
            let (curve, ends, count) = match p.of {
                PieceOf::Line { line, .. } => {
                    let e = fz.line_edges[line as usize];
                    let ends = br.ends(e).map(|v| SnapVert::Vert(t_op, v));
                    (Curve::Operand(t_op, e), ends, 0)
                }
                PieceOf::Arc { arc, .. } => {
                    let key = fz.arc_keys[arc as usize].ok_or(CellsError::Unknown(fi, pi))?;
                    let edge = z
                        .rewrite
                        .edges
                        .iter()
                        .find(|e| e.key == key)
                        .ok_or(CellsError::Unknown(fi, pi))?;
                    let step = match key {
                        SnapEdge::Piece { edge, .. } => open_step(s, z, edge, fz.face),
                        SnapEdge::Rim { .. } => return Err(CellsError::BoxSide(key)),
                        SnapEdge::Cut { .. } | SnapEdge::Zero { .. } => 0,
                    };
                    let records = i64::from(z.zone.rim.x21(key, fz.face));
                    (Curve::Snap(key), edge.ends, records + step)
                }
            };
            let trim = TrimAt {
                face: fi,
                piece: pi,
                forward: true,
            };
            match at.get(&curve) {
                Some(&k) => out[k].trim.push(trim),
                None => {
                    at.insert(curve, out.len());
                    out.push(CellCurve {
                        curve,
                        ends,
                        count,
                        trim: vec![trim],
                    });
                }
            }
        }
    }
    // The rim edges with records the faces don't hold (no length there).
    for r in &z.zone.rim.edges {
        let curve = Curve::Snap(r.edge);
        if at.contains_key(&curve) {
            continue;
        }
        let Some(e) = z.rewrite.edges.iter().find(|e| e.key == r.edge) else {
            continue;
        };
        at.insert(curve, out.len());
        out.push(CellCurve {
            curve,
            ends: e.ends,
            count: r.corners.iter().map(|c| i64::from(c.degree)).sum(),
            trim: Vec::new(),
        });
    }
    Ok(out)
}

/// The zone's cut-point check ([`check`]) on the sheets, both parts: the
/// rim records (the corner records at its ties, its branch vertices) and
/// the cells of the trim boolean on `faces`.
///
/// **Labels** (an interim for sheets, whose regions have no winding in
/// the other operand): a cell's label is the snapped sheet's coverage of
/// it, the snapped faces on the target face covering it, each counted
/// `+1` where its UV turns as the target face's along `ℓ` and `−1` where
/// not: the region's winding less the target face's own. The region
/// outside the face is no cell of the zone.
///
/// **Counts.** Each zone curve is one piece end to end. Its records count:
/// nothing crosses an element on the target once the perturbation moves
/// `A` off it, so `0`, and a rim edge its corner records ([`super::super::decide::RimRecords::x21`]
/// against the face it is on). An edge of the snapped sheet's open boundary
/// adds its coverage step, read from the store (its one face's side, turned
/// by the two UVs' turn): the coverage ends there with no surface crossed.
/// Rim pieces along box sides are refused ([`CellsError::BoxSide`]).
pub(crate) fn cut_points(
    s: &Setup<'_>,
    z: &Snap,
    faces: &[FaceZone],
) -> Result<Result<Option<CutFailure<u32>>, CutError>, CellsError> {
    let curves = cell_curves(s, z, faces)?;
    Ok(check_cells(z, faces, &curves))
}

/// [`check`] on `curves` (from [`cell_curves`]) and the cells of `faces`.
pub(crate) fn check_cells(
    z: &Snap,
    faces: &[FaceZone],
    curves: &[CellCurve],
) -> Result<Option<CutFailure<u32>>, CutError> {
    let labels: Vec<Vec<Option<i64>>> = faces
        .iter()
        .map(|f| {
            f.realised
                .arrangement
                .regions()
                .iter()
                .enumerate()
                .map(|(k, r)| (k > 0).then(|| r.winding - 1))
                .collect()
        })
        .collect();
    let cut_faces: Vec<CutFace<'_>> = faces
        .iter()
        .zip(&labels)
        .map(|(f, l)| CutFace {
            realised: &f.realised,
            labels: l,
        })
        .collect();
    let pieces: Vec<[CurvePiece<'_>; 1]> = curves
        .iter()
        .map(|c| {
            [CurvePiece {
                from: Cut::Vert(c.ends[0]),
                to: Cut::Vert(c.ends[1]),
                trim: &c.trim,
                count: Count::Records(c.count),
            }]
        })
        .collect();
    let zc: Vec<ZoneCurve<'_>> = curves
        .iter()
        .zip(&pieces)
        .map(|(c, p)| ZoneCurve {
            curve: c.curve,
            pieces: p,
        })
        .collect();
    check(
        &0u32,
        &CutInput {
            rim: &z.zone.rim,
            faces: &cut_faces,
            curves: &zc,
            rung: 0,
        },
        &mut work(),
    )
}
