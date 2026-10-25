//! The broad phase of a boolean (§5.1): the pieces of the two operands
//! whose boxes meet, so that only those are decided exactly.
//!
//! **Leaves** ([`leaves`]), per operand:
//! - a **face tile**: each tile of a face's surface that the box of its
//!   trims' UV hulls meets, boxed by the tile's patch restricted to that
//!   UV box (the patch split at the box's sides inside the tile, its
//!   certified hull; the tile's own hull where the box covers it);
//! - an **edge span**: each span of an edge's curve over the edge's
//!   range, boxed by its control points (the weights positive, the span
//!   lies in their hull).
//!
//! **Pairs** ([`candidates`]): every A face tile against every B face
//! tile, every edge span of either operand against the other's face
//! tiles, boxes inflated by `ε_dec` and an edge span's further by its
//! edge's band `β_g = edge.tol + ρ_op`. Which pairs come out is a fact of
//! the boxes; they come sorted by leaf index.
//!
//! **The projection column** ([`column()`]): for points (vertices, seeds)
//! the face tiles whose shadow along a direction `r̂` holds the point's
//! shadow: the boxes and points mapped by a linear map whose kernel is
//! exactly `r̂`'s line (the cross products of `r̂` with the two world axes
//! other than its largest component's, each coordinate a product of two
//! of `r̂`'s components, so the map's rows are orthogonal to `r̂` exactly),
//! each mapped value widened by its dot product's rounding bound. A tile
//! whose point lies on the point's column has a mapped box holding the
//! point's mapped value.
//!
//! **Work.** Forming each leaf is charged one unit (a restricted tile's
//! splits four more), in passes over the faces and the edges; the trees
//! charge their leaves and their queries count their hits before
//! collecting them ([`Bvh`]).

use std::convert::Infallible;

use varde_decide::bezier::Dir;
use varde_decide::num::bounded::Bounded;

use crate::geom::surface::Surface;
use crate::infra::budget::{Halt, Pass, Step, Stop, Work};
use crate::infra::bvh::{Aabb, Bvh};
use crate::infra::error::KernelError;
use crate::topo::brep::{Brep, EdgeId, FaceId};

#[cfg(test)]
mod tests;

/// A face tile leaf: the face, its tile's indices, its box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FaceTile {
    pub(crate) face: FaceId,
    pub(crate) tile: [i64; 2],
    pub(crate) boxed: Aabb<3>,
}

/// An edge span leaf: the edge, the span's index in its curve, its box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EdgeSpan {
    pub(crate) edge: EdgeId,
    pub(crate) span: usize,
    pub(crate) boxed: Aabb<3>,
}

/// An operand's leaves, faces in id order (each face's tiles in `v`,
/// then `u` order), edges in id order (spans in order).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Leaves {
    pub(crate) faces: Vec<FaceTile>,
    pub(crate) edges: Vec<EdgeSpan>,
}

/// The pairs of the two operands' leaves whose boxes meet, as indices
/// into their [`Leaves`], each list sorted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Candidates {
    /// `[A face tile, B face tile]`.
    pub(crate) faces: Vec<[u32; 2]>,
    /// `[A edge span, B face tile]`.
    pub(crate) a_edges: Vec<[u32; 2]>,
    /// `[B edge span, A face tile]`.
    pub(crate) b_edges: Vec<[u32; 2]>,
}

/// The empty box.
const EMPTY: Aabb<3> = Aabb {
    lo: [1.0; 3],
    hi: [0.0; 3],
};

/// The box of the trims' UV hulls of face `f`; `None` for a face without
/// trims or with a value that isn't finite.
fn uv_box(b: &Brep, f: FaceId) -> Option<[[f64; 2]; 2]> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for l in b.loops(f) {
        for c in b.loop_coedges(l) {
            for s in b.coedge_pcurve(c).spans() {
                let [a, z] = s.hull();
                for k in 0..2 {
                    if !(a[k].is_finite() && z[k].is_finite()) {
                        return None;
                    }
                    if a[k] < lo[k] {
                        lo[k] = a[k];
                    }
                    if z[k] > hi[k] {
                        hi[k] = z[k];
                    }
                }
            }
        }
    }
    (lo[0] <= hi[0] && lo[1] <= hi[1]).then_some([lo, hi])
}

/// The tile `(i, j)`'s patch restricted to the global UV box `uv` (which
/// meets the tile's square), boxed: the tile's hull where the box covers
/// the square, else the hull of the patch split at the box's sides inside
/// it. Each side cut is at the box's side itself (an exact local value),
/// so the piece holds the restriction.
fn tile_box(surface: &Surface, i: i64, j: i64, uv: &[[f64; 2]; 2]) -> Option<Aabb<3>> {
    let tile = surface.tile(i, j)?;
    let local = |x: f64, base: i64| x - base as f64;
    let [ulo, uhi] = [local(uv[0][0], i), local(uv[1][0], i)];
    let [vlo, vhi] = [local(uv[0][1], j), local(uv[1][1], j)];
    let clamp = |x: f64| x.clamp(0.0, 1.0);
    let (a, z) = ([clamp(ulo), clamp(vlo)], [clamp(uhi), clamp(vhi)]);
    if a == [0.0; 2] && z == [1.0; 2] {
        return Some(tile.hull_box().into());
    }
    let mut patch = tile.patch().clone();
    for (k, dir) in [(0, Dir::U), (1, Dir::V)] {
        // Cut at `a` keeping the far piece, then at `z` within it; the
        // second parameter rounded up so the piece reaches `z`.
        if a[k] > 0.0 {
            patch = patch.split_at(dir, &Bounded::exact(a[k]))[1].clone();
        }
        if z[k] < 1.0 {
            let t = if a[k] > 0.0 {
                ((z[k] - a[k]) / (1.0 - a[k])).next_up()
            } else {
                z[k]
            };
            if t < 1.0 {
                patch = patch.split_at(dir, &Bounded::exact(t))[0].clone();
            }
        }
    }
    patch.hull_box().map(Aabb::from)
}

/// The leaves of face `f` (module docs).
fn face_leaves(b: &Brep, f: FaceId) -> Vec<FaceTile> {
    let Some(uv) = uv_box(b, f) else {
        return Vec::new();
    };
    let surface = b.face_surface(f);
    let rect = surface.rect();
    // The tiles whose closed squares meet the box.
    let range = |k: usize, r: &std::ops::Range<i64>| {
        let lo = (uv[0][k].floor() as i64 - 1).max(r.start);
        let hi = (uv[1][k].ceil() as i64).min(r.end - 1);
        lo..=hi
    };
    let mut out = Vec::new();
    for j in range(1, &rect.v) {
        for i in range(0, &rect.u) {
            let meets = (i as f64) <= uv[1][0]
                && uv[0][0] <= (i + 1) as f64
                && (j as f64) <= uv[1][1]
                && uv[0][1] <= (j + 1) as f64;
            if !meets {
                continue;
            }
            let boxed = tile_box(surface, i, j, &uv).unwrap_or(Aabb {
                lo: [f64::NEG_INFINITY; 3],
                hi: [f64::INFINITY; 3],
            });
            out.push(FaceTile {
                face: f,
                tile: [i, j],
                boxed,
            });
        }
    }
    out
}

/// The leaves of edge `e` (module docs): its curve's spans over its
/// range.
fn edge_leaves(b: &Brep, e: EdgeId) -> Vec<EdgeSpan> {
    let c = b.edge_curve(e);
    let [t0, t1] = b.t_range(e);
    let k = c.knots();
    c.spans()
        .iter()
        .enumerate()
        .filter(|&(i, _)| k[i] < t1 && k[i + 1] > t0)
        .map(|(span, s)| {
            let mut bx = EMPTY;
            for p in s {
                let pt = Aabb::point([p[0], p[1], p[2]]);
                bx = if bx.is_empty() { pt } else { bx.union(&pt) };
            }
            EdgeSpan {
                edge: e,
                span,
                boxed: bx,
            }
        })
        .collect()
}

/// The leaves of body `b` (module docs), charged to `work` in a pass over
/// its faces and one over its edges.
pub(crate) fn leaves(b: &Brep, work: &mut Work) -> Result<Leaves, KernelError> {
    let pass = |stage| Pass { stage, rung: 0 };
    let faces: Vec<FaceId> = (0..b.face_count() as u32).map(FaceId).collect();
    let per_face = work
        .pass(pass("broad leaves"), &faces, |&f, ledger| {
            let out = face_leaves(b, f);
            ledger.charge(Step::Work, 5 * out.len() as u64 + 1)?;
            Ok::<_, Stop<Infallible>>(out)
        })
        .map_err(out_of_budget)?;
    let edges: Vec<EdgeId> = (0..b.edge_count() as u32).map(EdgeId).collect();
    let per_edge = work
        .pass(pass("broad leaves"), &edges, |&e, ledger| {
            let out = edge_leaves(b, e);
            ledger.charge(Step::Work, out.len() as u64 + 1)?;
            Ok::<_, Stop<Infallible>>(out)
        })
        .map_err(out_of_budget)?;
    Ok(Leaves {
        faces: per_face.into_iter().flatten().collect(),
        edges: per_edge.into_iter().flatten().collect(),
    })
}

/// A pass's halt: it only runs out.
fn out_of_budget(halt: Halt<Infallible>) -> KernelError {
    match halt {
        Halt::OutOfBudget(error) => error,
        Halt::Failed { error, .. } => match error {},
    }
}

/// A tree over boxes, built in one charged pass.
fn tree<const D: usize>(boxes: &[Aabb<D>], work: &mut Work) -> Result<Bvh<D>, KernelError> {
    let pass = Pass {
        stage: "broad tree",
        rung: 0,
    };
    let mut t = work
        .pass(pass, &[()], |(), ledger| {
            Bvh::build(boxes, ledger).map_err(Stop::<Infallible>::from)
        })
        .map_err(out_of_budget)?;
    Ok(t.remove(0))
}

/// The candidate pairs of operands `a` and `b` (module docs): boxes
/// inflated by `eps_dec`, an edge span's further by its edge's band
/// (`edge.tol` of its body, `beta_a`/`beta_b` giving it, plus `rho_op`).
pub(crate) fn candidates(
    a: &Leaves,
    b: &Leaves,
    beta_a: impl Fn(EdgeId) -> f64,
    beta_b: impl Fn(EdgeId) -> f64,
    eps_dec: f64,
    work: &mut Work,
) -> Result<Candidates, KernelError> {
    let pass = Pass {
        stage: "broad pairs",
        rung: 0,
    };
    let inflate = |x: &Aabb<3>, m: f64| x.inflated(m);
    let fa: Vec<Aabb<3>> = a.faces.iter().map(|l| inflate(&l.boxed, eps_dec)).collect();
    let fb: Vec<Aabb<3>> = b.faces.iter().map(|l| inflate(&l.boxed, eps_dec)).collect();
    let ea: Vec<Aabb<3>> = a
        .edges
        .iter()
        .map(|l| inflate(&l.boxed, add_up(eps_dec, beta_a(l.edge))))
        .collect();
    let eb: Vec<Aabb<3>> = b
        .edges
        .iter()
        .map(|l| inflate(&l.boxed, add_up(eps_dec, beta_b(l.edge))))
        .collect();
    let tb = tree(&fb, work)?;
    let ta = tree(&fa, work)?;
    Ok(Candidates {
        faces: tb.pairs(&fa, work, pass)?,
        a_edges: tb.pairs(&ea, work, pass)?,
        b_edges: ta.pairs(&eb, work, pass)?,
    })
}

/// `a + b` rounded upward (both non-negative); a NaN for a NaN.
fn add_up(a: f64, b: f64) -> f64 {
    (a + b).next_up()
}

/// The linear map of the projection column along `r` (module docs): two
/// rows, each orthogonal to `r` exactly; `None` for a zero or non-finite
/// direction.
fn column_map(r: [f64; 3]) -> Option<[[f64; 3]; 2]> {
    if r.iter().any(|x| !x.is_finite()) || r.iter().all(|&x| x == 0.0) {
        return None;
    }
    let mut big = 0;
    for k in 1..3 {
        if r[k].abs() > r[big].abs() {
            big = k;
        }
    }
    // `r × e_i` for the two axes `i ≠ big`: components of `r`, negated.
    let cross_axis = |i: usize| -> [f64; 3] {
        let mut out = [0.0; 3];
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        // `r × e_i`: component j is `r_k`, component k is `−r_j`.
        out[j] = r[k];
        out[k] = -r[j];
        out
    };
    let others: Vec<usize> = (0..3).filter(|&i| i != big).collect();
    Some([cross_axis(others[0]), cross_axis(others[1])])
}

/// The mapped value `row·p` with a certified enclosure: the rounded dot
/// product widened by its bound `γ₃·Σ|row_k·p_k|`, outward.
fn mapped(row: &[f64; 3], p: &[f64; 3]) -> [f64; 2] {
    let v = row[0] * p[0] + row[1] * p[1] + row[2] * p[2];
    let s = (row[0] * p[0]).abs() + (row[1] * p[1]).abs() + (row[2] * p[2]).abs();
    // `γ₃ = 3u/(1 − 3u)` below `2⁻⁵⁰`; the bound's own rounding is inside
    // that margin (relative `2⁻⁵¹`).
    let err = (s * TWO_M50).next_up();
    [(v - err).next_down(), (v + err).next_up()]
}

/// `2⁻⁵⁰`.
const TWO_M50: f64 = 1.0 / 1_125_899_906_842_624.0;

/// A 3D box mapped by `m`: its corners' enclosures' box.
fn map_box(m: &[[f64; 3]; 2], b: &Aabb<3>) -> Aabb<2> {
    if b.is_empty() {
        return Aabb {
            lo: [1.0; 2],
            hi: [0.0; 2],
        };
    }
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for c in 0..8 {
        let p: [f64; 3] = std::array::from_fn(|k| if c >> k & 1 == 1 { b.hi[k] } else { b.lo[k] });
        for (r, row) in m.iter().enumerate() {
            let [a, z] = mapped(row, &p);
            // A NaN (an infinite side times zero) leaves the side open.
            if a.is_nan() || a < lo[r] {
                lo[r] = if a.is_nan() { f64::NEG_INFINITY } else { a };
            }
            if z.is_nan() || z > hi[r] {
                hi[r] = if z.is_nan() { f64::INFINITY } else { z };
            }
        }
    }
    Aabb { lo, hi }
}

/// The pairs `[point, face tile]` of `points` and `leaves`' face tiles
/// whose shadows along `r` the point's shadow may lie in (module docs),
/// sorted; none for a zero direction.
pub(crate) fn column(
    leaves: &Leaves,
    points: &[[f64; 3]],
    r: [f64; 3],
    work: &mut Work,
) -> Result<Vec<[u32; 2]>, KernelError> {
    let Some(m) = column_map(r) else {
        return Ok(Vec::new());
    };
    let shadows: Vec<Aabb<2>> = leaves.faces.iter().map(|l| map_box(&m, &l.boxed)).collect();
    let queries: Vec<Aabb<2>> = points
        .iter()
        .map(|p| {
            let [a, b] = [mapped(&m[0], p), mapped(&m[1], p)];
            Aabb {
                lo: [a[0], b[0]],
                hi: [a[1], b[1]],
            }
        })
        .collect();
    let t = tree(&shadows, work)?;
    let pass = Pass {
        stage: "broad column",
        rung: 0,
    };
    t.pairs(&queries, work, pass)
}
