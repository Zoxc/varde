//! Numerical solves on patches: the points of a patch straight above or
//! below a vertex, and where an edge passes through a patch.
//!
//! Both split the patch's parameter triangle (and the edge's parameter
//! range) into pieces, exactly by blossoming, and drop the pieces whose
//! control points' box can't hold a solution (crossings: also those on
//! either side of a slab, or whose control hulls are apart); a piece
//! small or simple enough is handed to Newton's method, started at its
//! middle, and a
//! solution it finds counts if it lies in that piece (so each is found
//! once, and the next piece finds its own; above a vertex, one found from
//! another piece counts too where the search ran out of pieces before
//! getting to that one). Only `+ − × ÷ √`, so the
//! answers are the same on every platform. What they find are positions
//! and candidates; the counts decide how many there are.

use glam::{DVec2, DVec3};

use super::super::chain::trace::invert;
use super::Axes;
use crate::mesh::apart;
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

/// How near (in the patch's parameters) a point Newton's method found
/// from a piece it isn't in may be to one already found, facing the same
/// way, and be taken for it (see [`hits`]).
const ELSEWHERE_SAME: f64 = 1e-4;

/// How far from solving it (relative to the size of what is solved) the
/// point Newton's method ends at may be and still count as a solution.
const RESIDUAL: f64 = 1e-9;

/// How far outside a piece, in its own barycentric coordinates, a
/// solution Newton finds may be and still count as the piece's.
const PIECE_SLACK: f64 = 1e-9;

/// How far apart, for each unit of the two pieces' sizes (their boxes'
/// longest sides), an edge's piece's control hull and a patch's piece's
/// must be, beyond `1e-12` (thousands of times the rounding of their
/// blossomed control points in the search's unit frame), for a crossing
/// search to drop the two: so it drops none that holds a solution
/// Newton's method would count as the pieces', up to [`PIECE_SLACK`]
/// outside them in their own parameters, as one just past the patch's
/// side or the edge's end is. A point of a piece `ε` outside it is the
/// piece's control points weighted by Bernstein terms of which those of
/// the edge control points are negative, about `-2ε` times their
/// homogeneous weights in all, over the piece's weight function there;
/// so it is within `2ε·ρ` of the piece's diameter (at most `√3` times
/// its box's side) from the hull, `ρ` those weights over the weight
/// function: in the standard form with weights within
/// [`W_MIN`](crate::patch::W_MIN)`..=`[`W_MAX`](crate::patch::W_MAX),
/// at most `64` over `1/2` along an edge and over `1/3` in a patch,
/// twice that at a patch's corner, outside two sides. That is under
/// `1 400·ε` of the patch's piece's size and `450·ε` of the edge's.
/// The pieces' boxes and the slab along the patch's piece hold its hull,
/// so they are tested with the same margin.
const HULL_SLACK: f64 = 2048.0 * PIECE_SLACK;

/// How elongated a patch's piece may be, its longest side over its
/// height across that side, and still be quartered in a crossing search:
/// one more so is halved across its longest side instead. Quartering a
/// wall 1 000 tall and a few wide split it across its width as often as
/// along its height, and a curved edge (whose control hull holds the
/// width of the wall near its height) had every piece of that row to
/// look at: searches ran out of pieces and a crossing was found or not.
const MAX_ASPECT: f64 = 8.0;

/// Once Newton's method finds a crossing in a piece, the rest of the
/// edge's piece either side of it is searched again, but for this share
/// of it round the crossing: another crossing that near is a tangency
/// the counting's ties decide.
const AROUND: f64 = 1e-3;

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
        elsewhere: Vec::new(),
        nodes: 0,
    };
    search.visit(DVec3::AXES, 0);
    // Newton's method from one piece may end in another: where the search
    // ran out of pieces (a steep patch, whose pieces' shadows all hold the
    // vertex's) before it got to that one, a point of the patch all the
    // same (in the triangle, within the slack), kept. Where it didn't,
    // that piece found its own, and a second find of it (near a fold,
    // where Newton's method converges loosely, further off than the
    // dedup below) would count it twice.
    let mut found = search.found;
    found.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).expect("finite"));
    found.dedup_by(|a, b| (*a - *b).abs().max_element() <= 1e-10);
    if search.nodes > MAX_NODES {
        // Each piece Newton's method started from ends somewhere else
        // along the stretch where the line along `UP` grazes the patch
        // (its shadow there within `RESIDUAL` of the vertex's): one point
        // per stretch, facing one way, not one per piece. Ten finds of a
        // point on a wall seen 1e-4 off its axis, 1e-9 apart, were ten
        // points.
        let mut elsewhere = search.elsewhere;
        elsewhere.retain(|&u| in_piece(DVec3::AXES, u));
        elsewhere.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).expect("finite"));
        let facing = |u: DVec3| sign(local.normal(u).dot(axes.up));
        for u in elsewhere {
            let twin = found
                .iter()
                .any(|&k| (k - u).abs().max_element() <= ELSEWHERE_SAME && facing(k) == facing(u));
            if !twin {
                found.push(u);
            }
        }
        found.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).expect("finite"));
    }
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

/// [`edge_patch_with`] dropping pieces by their hulls and halving long
/// ones, as the booleans do.
#[cfg(test)]
pub(crate) fn edge_patch(edge: &Conic3, patch: &Patch) -> (Vec<EdgeHit>, f64, usize) {
    edge_patch_with(edge, patch, true)
}

/// Where `edge` passes through `patch`, in order along the edge, the
/// parameter along the edge where the two came closest, for a crossing
/// the count has and the search didn't find, and how many pieces the
/// search looked at (at most a little over [`MAX_NODES`]). Pieces whose
/// control hulls are apart are dropped, and long patch pieces halved
/// rather than quartered, only if `tall_walls`: tests turn it off to see
/// what the rest makes of a search that runs out of pieces, as it did on
/// tall walls without them.
pub(crate) fn edge_patch_with(
    edge: &Conic3,
    patch: &Patch,
    tall_walls: bool,
) -> (Vec<EdgeHit>, f64, usize) {
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
        tall_walls,
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

/// The most pieces [`near_patch`] looks at.
const MAX_NEAR_NODES: usize = 512;

/// Whether `x` lies within `within` of `patch` (its triangle of the
/// domain, not the surface beyond), certified: `Some` of the distance to
/// a point of the patch found that near, `None` where none was found
/// (farther, or past the search's cap), and how many pieces it looked at.
/// First the foot of the perpendicular from `x` (Gauss–Newton from the
/// middle), moved into the triangle; then, where that isn't near enough,
/// the patch's triangle split by blossoming, pieces whose control points'
/// box (which holds each piece) is farther than `within` dropped, and the
/// rest looked at nearest box first, then nearest middle. Each is a
/// point of the patch, so the distance found is one to the patch.
pub(crate) fn near_patch(x: DVec3, patch: &Patch, within: f64) -> (Option<f64>, usize) {
    let middle = |d: [DVec3; 3]| patch.eval((d[0] + d[1] + d[2]) / 3.0).distance(x);
    let foot = invert(patch, x, DVec3::splat(1.0 / 3.0)).max(DVec3::ZERO);
    let sum = foot.element_sum();
    let mut best = if sum > 0.0 && sum.is_finite() {
        patch.eval(foot / sum).distance(x)
    } else {
        f64::INFINITY
    };
    if best <= within {
        return (Some(best), 1);
    }
    let mut nodes = 1;
    // Pieces to look at: how near their box comes, how near their middle
    // is, and their triangle.
    let mut open: Vec<(f64, f64, [DVec3; 3])> = vec![(0.0, middle(DVec3::AXES), DVec3::AXES)];
    while let Some(i) = open
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.0.total_cmp(&b.1.0).then(a.1.1.total_cmp(&b.1.1)))
        .map(|(i, _)| i)
    {
        let (_, mid, d) = open.swap_remove(i);
        nodes += 1;
        if nodes > MAX_NEAR_NODES {
            break;
        }
        let points = piece_points(patch, d);
        for p in &points[..3] {
            best = best.min(p.distance(x));
        }
        best = best.min(mid);
        if best <= within {
            return (Some(best), nodes);
        }
        for q in quarters(d) {
            let Some(b) = Bounds3::around(&piece_points(patch, q)) else {
                continue;
            };
            let gap = (b.min - x).max(x - b.max).max(DVec3::ZERO).length();
            if gap <= within {
                open.push((gap, middle(q), q));
            }
        }
    }
    (None, nodes)
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
    patch
        .blossoms([
            (d[0], d[0]),
            (d[1], d[1]),
            (d[2], d[2]),
            (d[0], d[1]),
            (d[1], d[2]),
            (d[2], d[0]),
        ])
        .map(|h| h.truncate() / h.w)
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

/// The two halves of the barycentric triangle `d`, split at the middle
/// of its longest side, where the piece over it, with control `points`
/// (corners first), is more than [`MAX_ASPECT`] times longer than high
/// across that side; else `None`, to be quartered.
fn halves(d: [DVec3; 3], points: &[DVec3; 6]) -> Option<[[DVec3; 3]; 2]> {
    let c = [points[0], points[1], points[2]];
    let side = [0, 1, 2].map(|i| (c[(i + 1) % 3] - c[i]).length_squared());
    let i = if side[0] >= side[1] && side[0] >= side[2] {
        0
    } else if side[1] >= side[2] {
        1
    } else {
        2
    };
    // The longest side `l` over the height `2A / l` on it: `l² / 2A`.
    let twice_area = (c[1] - c[0]).cross(c[2] - c[0]).length();
    (side[i] > MAX_ASPECT * twice_area).then(|| {
        let (a, b, o) = (d[i], d[(i + 1) % 3], d[(i + 2) % 3]);
        let m = (a + b) * 0.5;
        [[a, m, o], [m, b, o]]
    })
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

/// Whether the edge's piece with control points `edge` and the patch's
/// piece with control `points` (corners first) lie on either side of a
/// slab along the piece's corners' normal, more than `margin` from it: a
/// finer test than the boxes where the edge runs close along the patch,
/// as at a tangency.
fn slab_apart(edge: &[DVec3; 3], points: &[DVec3; 6], margin: f64) -> bool {
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
    let (elo, ehi) = range(&mut edge.iter().map(|&p| (p - o).dot(n)));
    elo > phi + margin || ehi < plo - margin
}

struct HitSearch<'a> {
    /// The patch moved so the vertex is at the origin, and scaled.
    patch: &'a Patch,
    axes: &'a Axes,
    /// Points found in their own pieces.
    found: Vec<DVec3>,
    /// Points Newton's method found from a piece they aren't in.
    elsewhere: Vec<DVec3>,
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
            if let Some(u) = self.newton(start) {
                if in_piece(d, u) {
                    self.found.push(u);
                    return;
                }
                self.elsewhere.push(u);
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
    /// Whether pieces whose control hulls are apart are dropped and long
    /// patch pieces halved.
    tall_walls: bool,
}

impl CrossSearch<'_> {
    fn visit(&mut self, t: [f64; 2], d: [DVec3; 3], depth: u32) {
        self.visit_with(t, d, None, None, depth);
    }

    /// [`Self::visit`], given the edge's piece's control points where the
    /// caller has them (its piece over `t`, the same) and the patch's
    /// piece's (over `d`): halving one piece visits the other's again.
    fn visit_with(
        &mut self,
        t: [f64; 2],
        d: [DVec3; 3],
        edge_points: Option<[DVec3; 3]>,
        points: Option<[DVec3; 6]>,
        depth: u32,
    ) {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return;
        }
        // The edge's piece's control points (its weights are positive, so
        // the piece lies in their hull).
        let edge_points = edge_points.unwrap_or_else(|| {
            let point = |a: f64, b: f64| {
                let h = self.edge.blossom(a, b);
                h.truncate() / h.w
            };
            [point(t[0], t[0]), point(t[0], t[1]), point(t[1], t[1])]
        });
        let edge_box = Bounds3::point(edge_points[0])
            .include(edge_points[1])
            .include(edge_points[2]);
        let points = points.unwrap_or_else(|| piece_points(self.patch, d));
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
        let size = |b: &Bounds3| (b.max - b.min).max_element();
        let (se, sp) = (size(&edge_box), size(&patch_box));
        // Apart by more than a solution Newton's method counts as the
        // pieces' can lie outside them: one just past the patch's side or
        // the edge's end (or a split of its range) is just outside their
        // boxes and slab as it is their hulls.
        let slack = 1e-12 + HULL_SLACK * (se + sp);
        // The hulls: a tall or long patch's pieces all lie in a fat
        // edge's box across the patch's width, and splitting them on (in
        // both directions) ran the search out of pieces.
        if gap > slack
            || slab_apart(&edge_points, &points, slack)
            || (self.tall_walls && apart(&edge_points, &points, slack))
        {
            return;
        }
        let deep = depth >= MAX_CROSS_DEPTH;
        if deep || (depth >= 4 && se.max(sp) <= 1.0 / 16.0) {
            let start = (mid, (d[0] + d[1] + d[2]) / 3.0);
            if let Some((tt, u)) = self.newton(start)
                && tt >= t[0] - PIECE_SLACK * (t[1] - t[0])
                && tt <= t[1] + PIECE_SLACK * (t[1] - t[0])
                && in_piece(d, u)
            {
                self.found.push((tt, u));
                // The rest of the edge's piece, either side of it, may
                // cross the patch's piece again: an edge running through
                // a wall a little inside its rim goes in and out within
                // one piece, and the second crossing was never looked for.
                let gap = AROUND * (t[1] - t[0]);
                if !deep {
                    if tt - gap > t[0] {
                        self.visit_with([t[0], tt - gap], d, None, Some(points), depth + 1);
                    }
                    if tt + gap < t[1] {
                        self.visit_with([tt + gap, t[1]], d, None, Some(points), depth + 1);
                    }
                }
                return;
            }
            if deep {
                return;
            }
        }
        if se >= sp {
            self.visit_with([t[0], mid], d, None, Some(points), depth + 1);
            self.visit_with([mid, t[1]], d, None, Some(points), depth + 1);
        } else if let Some(halves) = halves(d, &points).filter(|_| self.tall_walls) {
            for h in halves {
                self.visit_with(t, h, Some(edge_points), None, depth + 1);
            }
        } else {
            for q in quarters(d) {
                self.visit_with(t, q, Some(edge_points), None, depth + 1);
            }
        }
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
