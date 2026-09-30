//! Numerical solves on patches: the points of a patch straight above or
//! below a vertex, and where an edge passes through a patch.
//!
//! Both split the patch's parameter triangle (and the edge's parameter
//! range) into pieces, exactly by blossoming, and drop the pieces whose
//! control points' box can't hold a solution; a piece small or simple
//! enough is handed to Newton's method, started at its middle, and a
//! solution it finds counts if it lies in that piece (so each is found
//! once, and the next piece finds its own). Only `+ − × ÷ √`, so the
//! answers are the same on every platform. What they find are positions
//! and candidates; the counts decide how many there are.

use glam::{DVec2, DVec3};

use super::Axes;
use crate::patch::{Bounds3, Conic3, Patch};

/// How deep the parameter triangle is split looking for points above a
/// vertex: pieces `2^-14` of it across.
const MAX_HIT_DEPTH: u32 = 14;

/// How deep the edge and the patch together are split looking for their
/// crossings.
const MAX_CROSS_DEPTH: u32 = 36;

/// The most pieces a search looks at: it stops splitting past this, as
/// happens where the edge runs along the patch.
const MAX_NODES: usize = 1024;

/// The most steps of Newton's method.
const MAX_NEWTON: usize = 40;

/// A Newton step this small (in parameters) ends the iteration.
const STEP: f64 = 1e-13;

/// How far from solving it (relative to the size of what is solved) the
/// point Newton's method ends at may be and still count as a solution.
const RESIDUAL: f64 = 1e-9;

/// How far outside a piece, in its own barycentric coordinates, a
/// solution Newton finds may be and still count as the piece's.
const PIECE_SLACK: f64 = 1e-9;

/// A point of a patch whose shadow along `UP` is a vertex's: where it is
/// in the patch (`u`, barycentric, which may be a little outside the
/// triangle), how far above the vertex it is, which way the patch faces
/// there (+1 up, −1 down) and how far outside the triangle `u` is (0
/// inside).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Hit {
    pub(crate) u: DVec3,
    pub(crate) dh: f64,
    pub(crate) facing: i8,
    pub(crate) out: f64,
}

/// The points of `patch` straight above or below `v` (along `axes.up`),
/// ordered by `u`. A point where the patch is vertical, whose shadow
/// folds, is found or not as rounding has it, with its twin of the other
/// facing.
pub(crate) fn hits(patch: &Patch, v: DVec3, axes: &Axes) -> Vec<Hit> {
    let bounds = patch.bounds();
    let scale = (bounds.max - bounds.min).max_element();
    if !(scale > 0.0 && scale.is_finite()) {
        return Vec::new();
    }
    let local = scaled(patch, v, scale);
    let mut search = HitSearch {
        patch: &local,
        axes,
        found: Vec::new(),
        nodes: 0,
    };
    search.visit(DVec3::AXES, 0);
    let mut found = search.found;
    found.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).expect("finite"));
    found.dedup_by(|a, b| (*a - *b).abs().max_element() <= 1e-10);
    found
        .into_iter()
        .filter_map(|u| {
            let facing = sign(patch.normal(u).dot(axes.up));
            (facing != 0).then(|| Hit {
                u,
                dh: (patch.eval(u) - v).dot(axes.up),
                facing,
                out: (-u.min_element()).max(0.0),
            })
        })
        .collect()
}

/// A crossing of an edge through a patch: where along the edge (`t`) and
/// in the patch (`u`), +1 where the edge enters the patch's solid (runs
/// against its outward normal) and −1 where it leaves, and how far
/// outside the edge's range and the patch's triangle it is (0 inside).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EdgeHit {
    pub(crate) t: f64,
    pub(crate) u: DVec3,
    pub(crate) x: i8,
    pub(crate) out: f64,
}

/// Where `edge` passes through `patch`, in order along the edge, the
/// parameter along the edge where the two came closest, for a crossing
/// the count has and the search didn't find, and how many pieces the
/// search looked at (at most a little over [`MAX_NODES`]).
pub(crate) fn edge_patch(edge: &Conic3, patch: &Patch) -> (Vec<EdgeHit>, f64, usize) {
    let bounds = edge.bounds().union(patch.bounds());
    let origin = (bounds.min + bounds.max) * 0.5;
    let scale = (bounds.max - bounds.min).max_element();
    if !(scale > 0.0 && scale.is_finite()) {
        return (Vec::new(), 0.5, 0);
    }
    let local_edge = Conic3 {
        p0: (edge.p0 - origin) / scale,
        c: (edge.c - origin) / scale,
        w: edge.w,
        p1: (edge.p1 - origin) / scale,
    };
    let local_patch = scaled(patch, origin, scale);
    let mut search = CrossSearch {
        edge: &local_edge,
        patch: &local_patch,
        found: Vec::new(),
        nodes: 0,
        closest: (f64::INFINITY, u32::MAX, 0.5),
    };
    search.visit([0.0, 1.0], DVec3::AXES, 0);
    let mut found = search.found;
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.dedup_by(|a, b| (a.0 - b.0).abs() <= 1e-10 && (a.1 - b.1).abs().max_element() <= 1e-10);
    let hits = found
        .into_iter()
        .filter_map(|(t, u)| {
            let (_, d) = edge.eval_deriv(t);
            let x = -sign(d.dot(patch.normal(u)));
            (x != 0).then(|| EdgeHit {
                t,
                u,
                x,
                out: (-t).max(t - 1.0).max(-u.min_element()).max(0.0),
            })
        })
        .collect();
    (hits, search.closest.2, search.nodes)
}

fn sign(x: f64) -> i8 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// `patch` moved by `-origin` and scaled by `1 / scale` (weights kept).
fn scaled(patch: &Patch, origin: DVec3, scale: f64) -> Patch {
    Patch {
        p: patch.p.map(|p| (p - origin) / scale),
        c: patch.c.map(|c| (c - origin) / scale),
        w: patch.w,
    }
}

/// The six control points of the piece of `patch` over the barycentric
/// triangle `d`, as points (their homogeneous weights are positive).
fn piece_points(patch: &Patch, d: [DVec3; 3]) -> [DVec3; 6] {
    let point = |a: DVec3, b: DVec3| {
        let h = patch.blossom(a, b);
        h.truncate() / h.w
    };
    [
        point(d[0], d[0]),
        point(d[1], d[1]),
        point(d[2], d[2]),
        point(d[0], d[1]),
        point(d[1], d[2]),
        point(d[2], d[0]),
    ]
}

/// The four pieces of the barycentric triangle `d`, split at its sides'
/// midpoints.
fn quarters(d: [DVec3; 3]) -> [[DVec3; 3]; 4] {
    let m01 = (d[0] + d[1]) * 0.5;
    let m12 = (d[1] + d[2]) * 0.5;
    let m20 = (d[2] + d[0]) * 0.5;
    [
        [d[0], m01, m20],
        [m01, d[1], m12],
        [m20, m12, d[2]],
        [m01, m12, m20],
    ]
}

/// Whether the barycentric point `u` lies in the barycentric triangle
/// `d`, within [`PIECE_SLACK`] of the piece's own coordinates.
fn in_piece(d: [DVec3; 3], u: DVec3) -> bool {
    let (a, b, c) = (d[0].truncate(), d[1].truncate(), d[2].truncate());
    let p = u.truncate();
    let cross = |x: DVec2, y: DVec2| x.x * y.y - x.y * y.x;
    let area = cross(b - a, c - a);
    if area == 0.0 {
        return false;
    }
    let l1 = cross(p - a, c - a) / area;
    let l2 = cross(b - a, p - a) / area;
    let l0 = 1.0 - l1 - l2;
    l0 >= -PIECE_SLACK && l1 >= -PIECE_SLACK && l2 >= -PIECE_SLACK
}

struct HitSearch<'a> {
    /// The patch moved so the vertex is at the origin, and scaled.
    patch: &'a Patch,
    axes: &'a Axes,
    found: Vec<DVec3>,
    nodes: usize,
}

impl HitSearch<'_> {
    fn visit(&mut self, d: [DVec3; 3], depth: u32) {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return;
        }
        // The shadow of the piece lies in its control points' shadows'
        // hull: it can only cover the vertex's if their box does.
        let points = piece_points(self.patch, d);
        let (mut lo, mut hi) = (DVec2::INFINITY, DVec2::NEG_INFINITY);
        for p in points {
            let q = DVec2::new(p.dot(self.axes.along), p.dot(self.axes.across));
            lo = lo.min(q);
            hi = hi.max(q);
        }
        let m = 1e-9;
        if lo.x > m || lo.y > m || hi.x < -m || hi.y < -m {
            return;
        }
        let deep = depth >= MAX_HIT_DEPTH;
        if deep || (depth >= 2 && self.unfolded(d)) {
            let start = (d[0] + d[1] + d[2]) / 3.0;
            if let Some(u) = self.newton(start)
                && in_piece(d, u)
            {
                self.found.push(u);
                return;
            }
            if deep {
                return;
            }
        }
        for q in quarters(d) {
            self.visit(q, depth + 1);
        }
    }

    /// Whether the piece over `d` faces one way along `UP` all over, so
    /// its shadow doesn't fold and holds each point once at most: the
    /// normal's Bernstein coefficients all lean the same way along `UP`.
    fn unfolded(&self, d: [DVec3; 3]) -> bool {
        let Ok(piece) = self.patch.sub(d) else {
            return false;
        };
        let ups = piece.normal_coeffs().map(|c| c.dot(self.axes.up));
        ups.iter().all(|&x| x > 0.0) || ups.iter().all(|&x| x < 0.0)
    }

    /// The point near `start` whose shadow is the origin's, by Newton's
    /// method on the shadow's two coordinates.
    fn newton(&self, start: DVec3) -> Option<DVec3> {
        let (a, b) = (self.axes.along, self.axes.across);
        let shadow = |p: DVec3| DVec2::new(p.dot(a), p.dot(b));
        let mut u = start;
        for _ in 0..MAX_NEWTON {
            let [p, pu, pv] = self.patch.eval_derivs(u);
            let f = shadow(p);
            let (j00, j01, j10, j11) = (pu.dot(a), pv.dot(a), pu.dot(b), pv.dot(b));
            let det = j00 * j11 - j01 * j10;
            let du0 = -(j11 * f.x - j01 * f.y) / det;
            let du1 = -(j00 * f.y - j10 * f.x) / det;
            if !(du0.is_finite() && du1.is_finite()) {
                return None;
            }
            u = DVec3::new(u.x + du0, u.y + du1, 0.0);
            u.z = 1.0 - u.x - u.y;
            if u.abs().max_element() > 4.0 {
                return None;
            }
            if du0.abs().max(du1.abs()) <= STEP {
                break;
            }
        }
        (shadow(self.patch.eval(u)).length() <= RESIDUAL).then_some(u)
    }
}

struct CrossSearch<'a> {
    edge: &'a Conic3,
    patch: &'a Patch,
    found: Vec<(f64, DVec3)>,
    nodes: usize,
    /// The closest pieces seen: the gap between their boxes (0 where they
    /// meet), `u32::MAX` less their depth, and the middle of the edge's
    /// piece.
    closest: (f64, u32, f64),
}

impl CrossSearch<'_> {
    fn visit(&mut self, t: [f64; 2], d: [DVec3; 3], depth: u32) {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return;
        }
        let edge_box = {
            let point = |a: f64, b: f64| {
                let h = self.edge.blossom(a, b);
                h.truncate() / h.w
            };
            Bounds3::point(point(t[0], t[0]))
                .include(point(t[0], t[1]))
                .include(point(t[1], t[1]))
        };
        let points = piece_points(self.patch, d);
        let patch_box = Bounds3::around(&points).expect("six points");
        let gap = (edge_box.min - patch_box.max)
            .max(patch_box.min - edge_box.max)
            .max_element();
        let mid = (t[0] + t[1]) * 0.5;
        // The closest place: the deepest piece whose boxes meet, else the
        // smallest gap.
        let near = (gap.max(0.0), u32::MAX - depth);
        if near < (self.closest.0, self.closest.1) {
            self.closest = (near.0, near.1, mid);
        }
        if gap > 1e-12 || self.slab_apart(t, &points) {
            return;
        }
        let size = |b: &Bounds3| (b.max - b.min).max_element();
        let (se, sp) = (size(&edge_box), size(&patch_box));
        let deep = depth >= MAX_CROSS_DEPTH;
        if deep || (depth >= 4 && se.max(sp) <= 1.0 / 16.0) {
            let start = (mid, (d[0] + d[1] + d[2]) / 3.0);
            if let Some((tt, u)) = self.newton(start)
                && tt >= t[0] - PIECE_SLACK * (t[1] - t[0])
                && tt <= t[1] + PIECE_SLACK * (t[1] - t[0])
                && in_piece(d, u)
            {
                self.found.push((tt, u));
                return;
            }
            if deep {
                return;
            }
        }
        if se >= sp {
            self.visit([t[0], mid], d, depth + 1);
            self.visit([mid, t[1]], d, depth + 1);
        } else {
            for q in quarters(d) {
                self.visit(t, q, depth + 1);
            }
        }
    }

    /// Whether the edge's piece over `t` and the patch's piece with
    /// control `points` (corners first) lie on either side of a slab
    /// along the piece's corners' normal: a finer test than the boxes
    /// where the edge runs close along the patch, as at a tangency.
    fn slab_apart(&self, t: [f64; 2], points: &[DVec3; 6]) -> bool {
        let Some(n) = (points[1] - points[0])
            .cross(points[2] - points[0])
            .try_normalize()
        else {
            return false;
        };
        let range = |xs: &mut dyn Iterator<Item = f64>| {
            xs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                (lo.min(x), hi.max(x))
            })
        };
        let o = points[0];
        let (plo, phi) = range(&mut points.iter().map(|&p| (p - o).dot(n)));
        let point = |a: f64, b: f64| {
            let h = self.edge.blossom(a, b);
            h.truncate() / h.w
        };
        let edge = [point(t[0], t[0]), point(t[0], t[1]), point(t[1], t[1])];
        let (elo, ehi) = range(&mut edge.iter().map(|&p| (p - o).dot(n)));
        elo > phi + 1e-12 || ehi < plo - 1e-12
    }

    /// The crossing near `start`, by Newton's method on `E(t) = P(u)`.
    fn newton(&self, start: (f64, DVec3)) -> Option<(f64, DVec3)> {
        let (mut t, mut u) = start;
        for _ in 0..MAX_NEWTON {
            let (e, de) = self.edge.eval_deriv(t);
            let [p, pu, pv] = self.patch.eval_derivs(u);
            // Solve de·Δt − pu·Δu0 − pv·Δu1 = p − e by Cramer's rule.
            let f = p - e;
            let (c0, c1, c2) = (de, -pu, -pv);
            let det = c0.dot(c1.cross(c2));
            let dt = f.dot(c1.cross(c2)) / det;
            let du0 = c0.dot(f.cross(c2)) / det;
            let du1 = c0.dot(c1.cross(f)) / det;
            if !(dt.is_finite() && du0.is_finite() && du1.is_finite()) {
                return None;
            }
            t += dt;
            u = DVec3::new(u.x + du0, u.y + du1, 0.0);
            u.z = 1.0 - u.x - u.y;
            if t.abs() > 4.0 || u.abs().max_element() > 4.0 {
                return None;
            }
            if dt.abs().max(du0.abs()).max(du1.abs()) <= STEP {
                break;
            }
        }
        let gap = (self.edge.eval(t) - self.patch.eval(u)).length();
        (gap <= RESIDUAL).then_some((t, u))
    }
}
