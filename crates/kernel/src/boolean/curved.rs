//! The primitives when an operand has curved patches: numerical solves,
//! each worked out once and shared, and exact wherever what they are about
//! is straight or flat.
//!
//! **Consistency by construction.** The counting's identities hold
//! whatever values `s02` and the crossings' split into `a_under` and
//! `b_under` take, as long as, for every pair of faces, the signed number
//! of crossings of their projected boundaries is zero, as it is for two
//! closed curves. Curved edges' crossings found one pair at a time can't
//! promise that: one near a shared vertex could be counted by both edges
//! there, or by neither. So, as Manifold derives its edge crossings from
//! shared vertex decisions, the signed number of crossings `I(e, h)` of
//! the shadows of `e` (from `a` to `b`) and `h` (from `c` to `d`) is
//! derived from **ray tests** (see [`mod@ray`]), each about one vertex and one
//! edge and the same whoever asks:
//!
//! ```text
//! I(e, h) = ρ(b, h) − ρ(a, h) − ρ⁻(c, e) + ρ⁻(d, e)
//! ```
//!
//! with `ρ(v, h)` the signed crossings of `h` with the ray ahead of `v`
//! and `ρ⁻(c, e)` those of `e` with the ray behind `c`: walking `v` along
//! `e`, `ρ(v, h)` changes by `σ` each time `v` crosses `h`, and each time
//! the ray sweeps over an end of `h` (where `v` crosses the ray behind that
//! end). Summed over the edges of a face `q`, the end terms cancel, and
//! what is left is the change of `q`'s projected winding number
//! `ω(v, q) = Σ ρ(v, h)` from `a` to `b`, which sums to zero round a face
//! of the other operand. Where the crossings are, and which edge is above
//! at each, comes from solving for them (see [`arcs`]); where that finds a
//! different number, the derived one wins.
//!
//! **Layers above a vertex.** `s02(v, f)` for a flat triangle is exact;
//! for a planar patch it is `ω(v, f)` where its plane is above `v` (exact)
//! and 0 where it is below; for a curved one the points of `f` straight
//! above and below `v` are solved for (see [`solve`]), and their signed
//! number made `ω(v, f)` (a shadow covers a point its boundary winds
//! round that many times, counted by facing) before those above are
//! counted.
//!
//! **Ties.** Heights closer than a 64th of the resolution are ties,
//! decided the way `A`'s perturbation (out of `A` for a union, in
//! otherwise) would decide them, so flush faces behave as they do between
//! flat operands.

use std::cmp::Ordering;

use glam::DVec3;

use super::count::Crossing;
use super::flat::Flat;
use super::input::{Input, Side};
use super::{BooleanError, Cross11, Found, Primitives, UP, segment};
use crate::Tolerance;
use crate::patch::Conic3;

mod arcs;
mod bernstein;
mod ray;
mod solve;

use ray::{RayEdge, ray};

/// Unit axes of the projection: along the rays, across them, and up.
#[derive(Debug, Clone, Copy)]
pub(super) struct Axes {
    pub(super) along: DVec3,
    pub(super) across: DVec3,
    pub(super) up: DVec3,
}

impl Axes {
    pub(super) fn new() -> Axes {
        Axes {
            along: ray::RAY / ray::RAY.length(),
            across: ray::ACROSS / ray::ACROSS.length(),
            up: UP / UP.length(),
        }
    }
}

/// How much work a search for an edge's crossings through a patch is at
/// least, in units of the budget: spent before it runs.
const SEARCH_WORK: usize = 16;

/// How many pieces a search for an edge's crossings looks at for a unit
/// of the budget: one that runs to its cap (as where two surfaces lie
/// along each other) is 256 units, not the least, 16.
const NODES_PER_UNIT: usize = 4;

/// The primitives of two operands, one of them with curved patches.
pub(super) struct Curved<'a> {
    a: &'a Input<'a>,
    b: &'a Input<'a>,
    /// The exact primitives, for what is straight or flat, and `A`'s
    /// perturbation.
    flat: Flat<'a>,
    axes: Axes,
    /// Heights closer than this are ties.
    tie: f64,
    resolution: f64,
}

impl<'a> Curved<'a> {
    /// `grow`: whether `A` grows (a union) or shrinks, for ties.
    pub(super) fn new(
        a: &'a Input<'a>,
        b: &'a Input<'a>,
        grow: bool,
        tol: &Tolerance,
    ) -> Curved<'a> {
        Curved {
            a,
            b,
            flat: Flat::new(a, b, grow),
            axes: Axes::new(),
            tie: tol.resolution() / 64.0,
            resolution: tol.resolution(),
        }
    }

    fn input(&self, side: Side) -> &Input<'a> {
        match side {
            Side::A => self.a,
            Side::B => self.b,
        }
    }

    /// Edge `e` of `side`, straight edges as the segment between their
    /// ends.
    fn curve(&self, side: Side, e: u32) -> Conic3 {
        let input = self.input(side);
        let conic = input.conic(e);
        if input.straight[e as usize] {
            segment(conic.p0, conic.p1)
        } else {
            conic
        }
    }

    fn ray_edge(&self, side: Side, e: u32) -> RayEdge {
        let input = self.input(side);
        let [c, d] = input.edges[e as usize].map(|v| self.flat.pt(side, v));
        RayEdge {
            c,
            d,
            conic: input.conic(e),
            straight: input.straight[e as usize],
        }
    }

    /// `ρ`: the signed crossings of edge `e` of the other operand than
    /// `side` with the ray from vertex `v` of `side`, ahead of it or
    /// behind.
    fn ray(&self, side: Side, v: u32, e: u32, ahead: bool) -> i32 {
        ray(
            self.flat.pt(side, v),
            &self.ray_edge(side.other(), e),
            ahead,
            &self.axes,
        )
    }

    /// `ω(v, f)`: how often the shadow of face `f` of the other operand
    /// winds round that of vertex `v` of `side`, counter-clockwise seen
    /// from `+UP`: the sum of the rays' crossings of its edges, as the
    /// face runs them.
    fn winding(&self, side: Side, v: u32, f: u32) -> i32 {
        let other = self.input(side.other());
        other.tri_edges[f as usize]
            .iter()
            .map(|&(e, forward)| {
                let r = self.ray(side, v, e, true);
                if forward { r } else { -r }
            })
            .sum()
    }

    /// `I(e, g)`: the signed number of crossings of the shadows of edge
    /// `e` of `A` and `g` of `B` (+1 where `e` crosses `g` from its right
    /// to its left), from the ray tests.
    fn shadow_crossings(&self, e: u32, g: u32) -> i32 {
        let [a0, a1] = self.a.edges[e as usize];
        let [b0, b1] = self.b.edges[g as usize];
        self.ray(Side::A, a1, g, true)
            - self.ray(Side::A, a0, g, true)
            - self.ray(Side::B, b0, e, false)
            + self.ray(Side::B, b1, e, false)
    }

    /// Whether, at a tie in height between edge `e` of `A` (at `t`) and
    /// something of `B`, `e` is above: `A`'s perturbation there moves it
    /// up.
    fn tie_above(&self, e: u32, t: f64) -> bool {
        self.perturb_along(e, t).dot(UP) > 0.0
    }

    /// `A`'s perturbation along edge `e` at `t`.
    fn perturb_along(&self, e: u32, t: f64) -> DVec3 {
        let [a0, a1] = self.a.edges[e as usize];
        self.flat.perturb(a0) * (1.0 - t) + self.flat.perturb(a1) * t
    }

    /// Whether, at a tie in height where the shadows of edge `e` of `A`
    /// (at `t`) and `g` of `B` (at `s`) cross, `e` is above `g` once `A`
    /// is perturbed: moving `e` by `δ` raises its point over `g`'s by
    /// `δ·m / UP·m`, `m = g' × e'` (the crossing moves along `g` as the
    /// shadows shift, so only the part of `δ` off the plane of the two
    /// tangents counts). Where that is zero, by `δ·UP`.
    fn crossing_above(&self, e: u32, t: f64, g: u32, s: f64) -> bool {
        let delta = self.perturb_along(e, t);
        let (_, de) = self.curve(Side::A, e).eval_deriv(t);
        let (_, dg) = self.curve(Side::B, g).eval_deriv(s);
        let m = dg.cross(de);
        match (sign(delta.dot(m)), sign(UP.dot(m))) {
            (0, _) | (_, 0) => self.tie_above(e, t),
            (x, y) => x == y,
        }
    }

    /// Whether `e` is above `g` where their shadows come closest, for
    /// crossings the ray tests count and the solve didn't find: they are
    /// near where an end of one passes the other.
    fn above_where_closest(&self, e: u32, g: u32) -> bool {
        let (ce, cg) = (self.curve(Side::A, e), self.curve(Side::B, g));
        let flat = |p: DVec3| {
            let q = p - UP * (p.dot(UP) / UP.length_squared());
            (q, p.dot(self.axes.up))
        };
        // Each end of one against samples of the other: the height of
        // `e`'s point less `g`'s, at the pair whose shadows are nearest.
        let mut best = (f64::INFINITY, 0.0);
        let samples = 32;
        for (ends, other, e_is_end) in [(&ce, &cg, true), (&cg, &ce, false)] {
            for p in [ends.p0, ends.p1] {
                let (qp, hp) = flat(p);
                for k in 0..=samples {
                    let (qo, ho) = flat(other.eval(k as f64 / samples as f64));
                    let d = (qp - qo).length_squared();
                    if d < best.0 {
                        best = (d, if e_is_end { hp - ho } else { ho - hp });
                    }
                }
            }
        }
        if best.1.abs() <= self.tie {
            self.tie_above(e, 0.5)
        } else {
            best.1 > 0.0
        }
    }

    /// Whether a point of face `f` of the other operand at `u`, a height
    /// `dh` above vertex `v` of `side` (straight above or below it), is
    /// above it, ties decided by `A`'s perturbation `δ`: where the patch's
    /// normal there is `n`, the ray from `v` meets it `−n·δ / n·UP`
    /// further up when `v` moves by `δ` (`v` of `A`), and `n·δ / n·UP`
    /// when the patch does (`v` of `B`). Where that is zero, by `δ·UP`.
    fn hit_above(&self, side: Side, v: u32, f: u32, u: DVec3, dh: f64) -> bool {
        if dh.abs() > self.tie {
            return dh > 0.0;
        }
        let n = self.input(side.other()).patches[f as usize].normal(u);
        let facing = sign(n.dot(UP));
        match side {
            // `v` moves; the point is above if it moves down.
            Side::A => {
                let delta = self.flat.perturb(v);
                match sign(n.dot(delta)) {
                    0 => delta.dot(UP) < 0.0,
                    x => facing != 0 && x != facing,
                }
            }
            // The patch moves, as its corners do.
            Side::B => {
                let corners = self.a.tris[f as usize];
                let delta: DVec3 = (0..3).map(|k| self.flat.perturb(corners[k]) * u[k]).sum();
                match sign(n.dot(delta)) {
                    0 => delta.dot(UP) > 0.0,
                    x => facing != 0 && x == facing,
                }
            }
        }
    }

    /// `s02` for a curved patch: the points of the patch straight above or
    /// below the vertex, as many as its winding number says, counted
    /// above.
    fn curved_layers(&self, side: Side, v: u32, f: u32, winding: i32) -> i8 {
        let other = self.input(side.other());
        let patch = &other.patches[f as usize];
        let p = self.input(side).pos(v);
        let heights = patch.hull().map(|x| (x - p).dot(self.axes.up));
        let low = heights.iter().copied().fold(f64::INFINITY, f64::min);
        let high = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if low > self.tie {
            return clamp(winding);
        }
        if high < -self.tie {
            return 0;
        }
        let hits = solve::hits(patch, p, &self.axes);
        // The points inside the triangle, less or plus those nearest its
        // sides, until their facings add up to the winding number.
        let (chosen, missing) = fit_count(
            hits.iter().map(|h| (h.facing, h.out, h.u.min_element())),
            winding,
        );
        let phantoms: i32 = missing.iter().map(|&s| i32::from(s)).sum();
        let mut above: i32 = hits
            .iter()
            .zip(&chosen)
            .filter(|&(h, &c)| c && self.hit_above(side, v, f, h.u, h.dh))
            .map(|(h, _)| i32::from(h.facing))
            .sum();
        if phantoms != 0 {
            // Points the winding number has and the solve didn't find:
            // above if most of the patch is.
            if -low < high {
                above += phantoms;
            }
        }
        clamp(above)
    }
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

fn clamp(x: i32) -> i8 {
    x.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8
}

impl Primitives for Curved<'_> {
    fn s02(&self, side: Side, v: u32, f: u32) -> i8 {
        let other = self.input(side.other());
        if other.flat[f as usize] {
            return self.flat.s02(side, v, f);
        }
        let winding = self.winding(side, v, f);
        if other.planar[f as usize] {
            return match self.flat.plane_above(side, v, f) {
                Some(true) => clamp(winding),
                _ => 0,
            };
        }
        self.curved_layers(side, v, f, winding)
    }

    fn s11(&self, e: u32, g: u32) -> Cross11 {
        let count = self.shadow_crossings(e, g);
        if self.a.straight[e as usize] && self.b.straight[g as usize] {
            if count == 0 {
                return Cross11::default();
            }
            let sigma = count.signum() as i8;
            return if self.flat.e_above(e, g, sigma) {
                Cross11 {
                    a_under: 0,
                    b_under: clamp(-count),
                }
            } else {
                Cross11 {
                    a_under: clamp(count),
                    b_under: 0,
                }
            };
        }
        let (ce, cg) = (self.curve(Side::A, e), self.curve(Side::B, g));
        let (mut a_under, mut b_under, mut found) = (0i32, 0i32, 0i32);
        for c in arcs::cross(&ce, &cg, &self.axes) {
            let above = if c.dh.abs() <= self.tie {
                self.crossing_above(e, c.t, g, c.s)
            } else {
                c.dh > 0.0
            };
            let sigma = i32::from(c.sigma);
            if above {
                b_under -= sigma;
            } else {
                a_under += sigma;
            }
            found += sigma;
        }
        let missing = count - found;
        if missing != 0 {
            if self.above_where_closest(e, g) {
                b_under -= missing;
            } else {
                a_under += missing;
            }
        }
        Cross11 {
            a_under: clamp(a_under),
            b_under: clamp(b_under),
        }
    }

    fn searches(&self, side: Side, e: u32, f: u32) -> bool {
        // A segment meets a plane once at most; anything else may pass
        // through and back.
        !(self.input(side).straight[e as usize] && self.input(side.other()).planar[f as usize])
    }

    fn search_work(&self) -> usize {
        SEARCH_WORK
    }

    fn margin(&self) -> f64 {
        // Straight edges and planar patches are taken as such within the
        // resolution, and ties are closer still.
        self.resolution
    }

    fn crossings(&self, side: Side, e: u32, f: u32, x: i32) -> Result<Found, BooleanError> {
        if !self.searches(side, e, f) {
            // Exact, against the plane of the patch's corners.
            return self.flat.crossings(side, e, f, x);
        }
        let edge = self.curve(side, e);
        let patch = &self.input(side.other()).patches[f as usize];
        let (found, closest, nodes) = solve::edge_patch(&edge, patch);
        let cost = SEARCH_WORK.max(nodes.div_ceil(NODES_PER_UNIT));
        Ok((pick(&found, x, closest), cost))
    }

    fn order(&self, side: Side, e: u32, c1: &Crossing, c2: &Crossing) -> Ordering {
        let other = self.input(side.other());
        if c1.face != c2.face
            && self.input(side).straight[e as usize]
            && other.planar[c1.face as usize]
            && other.planar[c2.face as usize]
        {
            return self.flat.order_faces(side, e, c1.face, c2.face);
        }
        c1.t.total_cmp(&c2.t)
            .then(c1.face.cmp(&c2.face))
            .then(c1.i.cmp(&c2.i))
    }
}

/// The crossings to record from those `found`, whose signs must add up to
/// `x`: those inside the edge and the patch, less or plus those nearest
/// their edges, until they do (see [`fit_count`]), and at `closest` along
/// the edge for any the search didn't find; as `(sign, t)` in order along
/// the edge.
fn pick(found: &[solve::EdgeHit], x: i32, closest: f64) -> Vec<(i8, f64)> {
    let (chosen, missing) = fit_count(
        found
            .iter()
            .map(|h| (h.x, h.out, h.t.min(1.0 - h.t).min(h.u.min_element()))),
        x,
    );
    let mut out: Vec<(i8, f64)> = missing
        .into_iter()
        .map(|s| (s, closest.clamp(0.0, 1.0)))
        .collect();
    for (h, &c) in found.iter().zip(&chosen) {
        if c {
            out.push((h.x, h.t.clamp(0.0, 1.0)));
        }
    }
    out.sort_by(|a, b| a.1.total_cmp(&b.1));
    out
}

/// Which of the solutions a search found to keep so that their signs add
/// up to `count`, which the counting decided: each given as `(sign, out,
/// margin)`, `out` how far outside its domain it is (0 inside) and
/// `margin` how near its domain's sides it is if inside. Those inside
/// first; then, one step at a time, the most doubtful is turned (a
/// solution outside with the sign wanted taken in, or one inside with
/// the other sign dropped: whichever is nearest the domain's sides),
/// and where none is left the sign wanted is one the search missed.
/// Gives whether each is kept, and the signs of those missed.
fn fit_count(found: impl Iterator<Item = (i8, f64, f64)>, count: i32) -> (Vec<bool>, Vec<i8>) {
    let found: Vec<(i8, f64, f64)> = found.collect();
    let mut chosen: Vec<bool> = found.iter().map(|&(_, out, _)| out == 0.0).collect();
    let mut sum: i32 = found
        .iter()
        .zip(&chosen)
        .filter(|&(_, &c)| c)
        .map(|(&(s, _, _), _)| i32::from(s))
        .sum();
    let doubt = |&(_, out, margin): &(i8, f64, f64)| if out > 0.0 { out } else { margin };
    let mut missing = Vec::new();
    while sum != count {
        let want = (count - sum).signum() as i8;
        let pick = found
            .iter()
            .enumerate()
            .filter(|&(k, h)| if chosen[k] { h.0 == -want } else { h.0 == want })
            .min_by(|a, b| doubt(a.1).total_cmp(&doubt(b.1)));
        match pick {
            Some((k, _)) => chosen[k] = !chosen[k],
            None => missing.push(want),
        }
        sum += i32::from(want);
    }
    (chosen, missing)
}

#[cfg(test)]
mod tests;
