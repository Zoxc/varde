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
//! otherwise, then the generic translations) would decide them, so flush
//! faces behave as they do between flat operands; so are crossings of a
//! ray at its vertex, shadows lying along each other (where the edges
//! lie on each other in space too, crossing by crossing where the
//! perturbation parts them), crossings at an edge's end and crossings at
//! a patch's side. The exact predicates take near ties within the same
//! distance as ties, so both see one configuration.

use std::cmp::Ordering;

use glam::DVec3;

use super::assemble::param_on;
use super::count::Crossing;
use super::exact;
use super::flat::Flat;
use super::input::{Input, Side};
use super::{BooleanError, Cross11, Found, Primitives, UP, segment};
use crate::Tolerance;
use crate::mesh::Surface;
use crate::patch::Conic3;

mod arcs;
pub(super) mod bernstein;
mod ray;
pub(super) mod solve;

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
/// along each other) is 512 units, not the least, 16. Testing each
/// piece's control hulls for a gap made a piece about twice as dear
/// there (some 2.3 µs), so it is 2, not 4.
const NODES_PER_UNIT: usize = 2;

/// Where two edges lie on each other, the polynomial whose roots are the
/// perturbed shadows' crossings is taken as zero (and the next order of
/// the perturbation asked) when every coefficient is within this of the
/// size of its terms: rounding.
const ALONG_ZERO: f64 = 1e-12;

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
    /// Whether the crossing searches drop pieces whose control hulls are
    /// apart and halve long patch pieces: always, but for tests under
    /// `assemble::LOOSE`.
    tall_walls: bool,
}

impl<'a> Curved<'a> {
    /// `grow`: whether `A` grows (a union) or shrinks, for ties.
    pub(super) fn new(
        a: &'a Input<'a>,
        b: &'a Input<'a>,
        grow: bool,
        tol: &Tolerance,
    ) -> Curved<'a> {
        let tie = super::tie(tol);
        Curved {
            a,
            b,
            flat: Flat::tied(a, b, grow, tie),
            axes: Axes::new(),
            tie,
            resolution: tol.resolution(),
            #[cfg(test)]
            tall_walls: !super::assemble::LOOSE.get(),
            #[cfg(not(test))]
            tall_walls: true,
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
            self.tie,
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
        if m.length() <= 1e-9 * dg.length() * de.length() {
            return self.parallel_above(e, t);
        }
        let up = sign(UP.dot(m));
        if up == 0 {
            return self.tie_above(e, t);
        }
        match first_sign(delta, |d| d.dot(m)) {
            0 => self.tie_above(e, t),
            x => x == up,
        }
    }

    /// Whether edge `e` of `A`, at `t`, is above an edge of `B` running
    /// along it there (their tangents parallel, the heights tied) once
    /// `A` is perturbed: moving `e` by `δ`, the crossing slides along
    /// the tangent `T`, and `e` rises by `(T × δ)·(T × UP) / |T × UP|²`.
    fn parallel_above(&self, e: u32, t: f64) -> bool {
        let (_, tangent) = self.curve(Side::A, e).eval_deriv(t);
        let across = tangent.cross(UP);
        match first_sign(self.perturb_along(e, t), |d| tangent.cross(d).dot(across)) {
            0 => self.tie_above(e, t),
            x => x > 0,
        }
    }

    /// Whether `e` is above `g` where their shadows run along each other:
    /// by their heights where they are nearest, or as a tie there.
    fn along_above(&self, e: u32, g: u32) -> bool {
        let (ce, cg) = (self.curve(Side::A, e), self.curve(Side::B, g));
        let flat = |p: DVec3| {
            let q = p - UP * (p.dot(UP) / UP.length_squared());
            (q, p.dot(self.axes.up))
        };
        let samples = 16;
        let mut best = (f64::INFINITY, 0.0, 0.5);
        for i in 1..samples {
            let t = i as f64 / samples as f64;
            let (qe, he) = flat(ce.eval(t));
            for k in 0..=samples {
                let (qg, hg) = flat(cg.eval(k as f64 / samples as f64));
                let d = (qe - qg).length_squared();
                if d < best.0 {
                    best = (d, he - hg, t);
                }
            }
        }
        if best.1.abs() <= self.tie {
            self.parallel_above(e, best.2)
        } else {
            best.1 > 0.0
        }
    }

    /// How the `count` crossings of the shadows of edge `e` of `A` and
    /// `g` of `B`, lying on one conic, split, where the two edges lie on
    /// each other in space over a stretch (the same arc in both operands,
    /// or pieces of it); `None` where they don't (one conic in shadow,
    /// the curves apart in height: a top rim seen along the axis over a
    /// bottom one).
    ///
    /// Over that stretch `A`'s perturbation `δ` takes `e`'s shadow to
    /// the left of `g`'s where `δ·(UP × g')` is positive, so the
    /// perturbed shadows cross where that changes sign, `σ` its sign
    /// after, each crossing above or below by its own rise
    /// ([`Self::parallel_above`]). With `e`'s homogeneous tangent `h`
    /// (a quadratic, [`hodograph`]) for `g'` (turned by `o`, the sign
    /// of `e'·g'`), `o·δ·(UP × h)` is a cubic in `t`: its roots are the
    /// crossings. Where it is zero to rounding, the generic
    /// translations take `δ`'s place, as in [`first_sign`]. Roots within
    /// the tie of the stretch's ends are left to the count, as at
    /// crossings at an end, and whatever the count has beyond those
    /// found goes by the rise at such a root, or at an end where the
    /// cubic is zero to rounding (a circle through exact axis points
    /// makes them), else by [`Self::along_above`]. Seen nearly edge on
    /// (a cap plane nearly along `UP`, as on the XZ and YZ planes), a
    /// rim's shadow is a thin ellipse round which the perturbed shadows
    /// cross once or twice with `A` above at one and below at the other:
    /// one sample's height for all of them split a crossing the wrong
    /// way.
    fn along_crossings(&self, e: u32, g: u32, count: i32) -> Option<Cross11> {
        let (ce, cg) = (self.curve(Side::A, e), self.curve(Side::B, g));
        let on_g = |p: DVec3| param_on(&cg, p, self.tie);
        let on_e = |p: DVec3| param_on(&ce, p, self.tie);
        // The stretch of `e` on `g`: from `e`'s ends on `g` and `g`'s on
        // `e`. Arcs are under half a turn, so it is one piece.
        let ends = [
            on_g(ce.p0).map(|_| 0.0),
            on_g(ce.p1).map(|_| 1.0),
            on_e(cg.p0),
            on_e(cg.p1),
        ];
        let (lo, hi) = ends
            .into_iter()
            .flatten()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| {
                (lo.min(t), hi.max(t))
            });
        let (end_lo, end_hi) = (ce.eval(lo), ce.eval(hi));
        if lo >= hi || end_lo.distance(end_hi) <= self.tie {
            return None;
        }
        // Lifts of one conic shadow meeting at three points of it are one
        // curve; the ends and three points between make sure.
        let mut mid = 0.5;
        for k in 1..4 {
            let s = on_g(ce.eval(lo + (hi - lo) * f64::from(k) / 4.0))?;
            if k == 2 {
                mid = s;
            }
        }
        let o = sign(ce.eval_deriv((lo + hi) * 0.5).1.dot(cg.eval_deriv(mid).1));
        if o == 0 {
            return None;
        }
        let tangent = hodograph(&ce);
        let [a0, a1] = self.a.edges[e as usize];
        let longest = tangent.iter().map(|h| h.length()).fold(0.0, f64::max);
        let poly = [
            (self.flat.perturb(a0), self.flat.perturb(a1)),
            (exact::T2, exact::T2),
            (exact::T3, exact::T3),
        ]
        .into_iter()
        .find_map(|(d0, d1)| {
            let f = |d: DVec3, h: DVec3| f64::from(o) * d.dot(UP.cross(h));
            let p = [
                f(d0, tangent[0]),
                (2.0 * f(d0, tangent[1]) + f(d1, tangent[0])) / 3.0,
                (f(d0, tangent[2]) + 2.0 * f(d1, tangent[1])) / 3.0,
                f(d1, tangent[2]),
            ];
            let zero = ALONG_ZERO * d0.length().max(d1.length()) * UP.length() * longest;
            p.iter().any(|c| c.abs() > zero).then_some((p, zero))
        });
        let (poly, zero) = poly?;
        let roots = bernstein::roots(&poly);
        let (mut a_under, mut b_under, mut found) = (0i32, 0i32, 0i32);
        // Where a crossing at an end of the stretch is, if there is one:
        // a root within the tie of it (on either side: rounding may put
        // one just outside), or the end itself where the polynomial is
        // zero there to rounding (its root may be further out, or not
        // found at all).
        let mut at_end = None;
        for (i, &t) in roots.iter().enumerate() {
            let at = ce.eval(t);
            if at.distance(end_lo) <= self.tie || at.distance(end_hi) <= self.tie {
                at_end.get_or_insert(t);
                continue;
            }
            if t <= lo || t >= hi {
                continue;
            }
            let next = roots.get(i + 1).copied().unwrap_or(1.0);
            let sigma = i32::from(sign(bernstein::eval(&poly, (t + next) * 0.5)));
            if self.parallel_above(e, t) {
                b_under -= sigma;
            } else {
                a_under += sigma;
            }
            found += sigma;
        }
        for end in [lo, hi] {
            if bernstein::eval(&poly, end).abs() <= zero {
                at_end.get_or_insert(end);
            }
        }
        let missing = count - found;
        if missing != 0 {
            let above = match at_end {
                Some(t) => self.parallel_above(e, t),
                None => self.along_above(e, g),
            };
            if above {
                b_under -= missing;
            } else {
                a_under += missing;
            }
        }
        Some(Cross11 {
            a_under: clamp(a_under),
            b_under: clamp(b_under),
        })
    }

    /// Whether `e` is above `g` at their shadows' crossing `c`, ties as
    /// the perturbation decides them.
    fn above_at(&self, e: u32, g: u32, c: &arcs::ArcCross) -> bool {
        if c.dh.abs() <= self.tie {
            self.crossing_above(e, c.t, g, c.s)
        } else {
            c.dh > 0.0
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
        // `e`'s point less `g`'s, at the pair whose shadows are nearest,
        // and where along each they are.
        let mut best = (f64::INFINITY, 0.0, 0.5, 0.5);
        let samples = 32;
        for (ends, other, e_is_end) in [(&ce, &cg, true), (&cg, &ce, false)] {
            for (p, end) in [(ends.p0, 0.0), (ends.p1, 1.0)] {
                let (qp, hp) = flat(p);
                let gap = |x: f64| (qp - flat(other.eval(x)).0).length_squared();
                let k = (0..=samples)
                    .min_by(|&i, &j| {
                        gap(i as f64 / samples as f64).total_cmp(&gap(j as f64 / samples as f64))
                    })
                    .expect("samples");
                // Narrowed down between the samples beside the nearest
                // (a golden-section search: the gap has one low there).
                let (mut lo, mut hi) = (
                    (k as f64 - 1.0).max(0.0) / samples as f64,
                    (k as f64 + 1.0).min(samples as f64) / samples as f64,
                );
                let r = 0.618_033_988_749_895;
                for _ in 0..60 {
                    let (x1, x2) = (hi - r * (hi - lo), lo + r * (hi - lo));
                    if gap(x1) <= gap(x2) {
                        hi = x2;
                    } else {
                        lo = x1;
                    }
                }
                let x = (lo + hi) * 0.5;
                let d = gap(x);
                if d < best.0 {
                    let ho = flat(other.eval(x)).1;
                    best = if e_is_end {
                        (d, hp - ho, end, x)
                    } else {
                        (d, ho - hp, x, end)
                    };
                }
            }
        }
        if best.1.abs() <= self.tie {
            self.crossing_above(e, best.2, g, best.3)
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
                match first_sign(delta, |d| n.dot(d)) {
                    0 => delta.dot(UP) < 0.0,
                    x => facing != 0 && x != facing,
                }
            }
            // The patch moves, as its corners do.
            Side::B => {
                let corners = self.a.tris[f as usize];
                let delta: DVec3 = (0..3).map(|k| self.flat.perturb(corners[k]) * u[k]).sum();
                match first_sign(delta, |d| n.dot(d)) {
                    0 => delta.dot(UP) > 0.0,
                    x => facing != 0 && x == facing,
                }
            }
        }
    }

    /// Whether the crossing `h` of edge `e` of `side` (`edge`) through
    /// face `f` of the other (`patch`), within a tie of one of the
    /// patch's sides, is inside the patch once `A` is perturbed, or `None`
    /// if it isn't that near a side (or is at an end of the edge, which
    /// the count decides). Moving the edge by `δ` relative to the patch
    /// moves the crossing by `δ − e'·(δ·n)/(e'·n)` on the surface; it
    /// stays in if that heads into the patch across the side.
    fn tie_inside(
        &self,
        side: Side,
        e: u32,
        f: u32,
        edge: &Conic3,
        patch: &crate::patch::Patch,
        h: &solve::EdgeHit,
    ) -> Option<bool> {
        if !(h.t > 1e-9 && h.t < 1.0 - 1e-9) {
            return None;
        }
        let k = (0..3)
            .min_by(|&i, &j| h.u[i].total_cmp(&h.u[j]))
            .expect("three coordinates");
        let [_, pu, pv] = patch.eval_derivs(h.u);
        // The domain step towards corner `k`, and how far the side is.
        let w = DVec3::AXES[k] - h.u;
        let into = pu * w.x + pv * w.y;
        let away = h.u[k].abs() * into.length();
        if away.is_nan() || away > self.tie {
            return None;
        }
        let (_, de) = edge.eval_deriv(h.t);
        let n = pu.cross(pv);
        let across = de.dot(n);
        if across == 0.0 {
            return None;
        }
        // `A`'s motion there (the edge's, or the patch's), and which way
        // it moves the edge relative to the patch.
        let (motion, towards) = match side {
            Side::A => (self.perturb_along(e, h.t), 1.0),
            Side::B => {
                let corners = self.a.tris[f as usize];
                let at: DVec3 = (0..3).map(|i| self.flat.perturb(corners[i]) * h.u[i]).sum();
                (at, -1.0)
            }
        };
        // The crossing's step in the domain, `(du0, du1)` by least squares
        // on `P_u`, `P_v`; its `u_k` part is `du·w`-like: that of the
        // barycentric step.
        let (g00, g01, g11) = (pu.dot(pu), pu.dot(pv), pv.dot(pv));
        let det = g00 * g11 - g01 * g01;
        if det.is_nan() || det <= 0.0 {
            return None;
        }
        let step_k = |d: DVec3| {
            let dx = d - de * (d.dot(n) / across);
            let (r0, r1) = (pu.dot(dx), pv.dot(dx));
            let du0 = (g11 * r0 - g01 * r1) / det;
            let du1 = (g00 * r1 - g01 * r0) / det;
            [du0, du1, -du0 - du1][k]
        };
        match first_sign(motion, |d| step_k(d * towards)) {
            0 => None,
            s => Some(s > 0),
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

/// The sign of `f` of `A`'s perturbation, order by order: of its first
/// order `delta` (each vertex's own direction, as interpolated there),
/// else of the two generic translations that follow it, as the exact
/// predicates take them; 0 if all three are.
///
/// `f` is linear (a motion's effect at a tie), and an order whose value
/// is only rounding, within [`exact::RHO`] of its terms `Σ |d_i·f(e_i)|`
/// (the motion `d` square to `f`'s gradient but for that), is taken as
/// zero, as [`exact::sign_tied`] takes the exact predicates' later
/// orders: at the exact tie it stands for, it is zero.
pub(super) fn first_sign(delta: DVec3, f: impl Fn(DVec3) -> f64) -> i8 {
    let gradient = DVec3::AXES.map(&f);
    [delta, exact::T2, exact::T3]
        .into_iter()
        .map(|d| {
            let value = f(d);
            let size: f64 = (0..3).map(|i| (d[i] * gradient[i]).abs()).sum();
            if size.is_finite() && value.abs() <= exact::RHO * size {
                0
            } else {
                sign(value)
            }
        })
        .find(|&s| s != 0)
        .unwrap_or(0)
}

/// The Bernstein coefficients of a positive multiple of `c`'s tangent,
/// `N'·W − N·W'` over 2 for its point `N / W`: `w·(c − p0)`,
/// `(p1 − p0) / 2`, `w·(p1 − c)`.
fn hodograph(c: &Conic3) -> [DVec3; 3] {
    [(c.c - c.p0) * c.w, (c.p1 - c.p0) * 0.5, (c.p1 - c.c) * c.w]
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
        let Some(solved) = arcs::cross(&ce, &cg, &self.axes) else {
            // The shadows run along each other: where the edges lie on
            // each other, crossing by crossing as the perturbation parts
            // them; else every crossing the ray tests count goes the same
            // way.
            if let Some(split) = self.along_crossings(e, g, count) {
                return split;
            }
            return if self.along_above(e, g) {
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
        };
        // Crossings at an end of either edge (within a tie of it) are
        // there or not as the perturbation has it, which the count knows
        // and the solve doesn't: they are left to it.
        let ends = [ce.p0, ce.p1, cg.p0, cg.p1];
        let mut at_end = None;
        for c in solved {
            let at = ce.eval(c.t);
            if ends.iter().any(|&x| x.distance(at) <= self.tie) {
                at_end.get_or_insert(c);
                continue;
            }
            let sigma = i32::from(c.sigma);
            if self.above_at(e, g, &c) {
                b_under -= sigma;
            } else {
                a_under += sigma;
            }
            found += sigma;
        }
        let missing = count - found;
        if missing != 0 {
            let above = match at_end {
                Some(c) => self.above_at(e, g, &c),
                None => self.above_where_closest(e, g),
            };
            if above {
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
        // through and back, unless it lies in the patch's surface (the
        // perturbation takes it off to one side): the search would find
        // crossings in and out where rounding has them.
        let other = self.input(side.other());
        if self.input(side).straight[e as usize] && other.planar[f as usize] {
            return false;
        }
        let surface = other.mesh.faces()[other.face(f) as usize].surface;
        if matches!(surface, Surface::Free) {
            return true;
        }
        let conic = self.curve(side, e);
        !(0..=4).all(|k| surface.distance(conic.eval(f64::from(k) / 4.0)) <= self.resolution)
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
        let (mut found, closest, nodes) = solve::edge_patch_with(&edge, patch, self.tall_walls);
        // Crossings on the patch's side, within a tie of it (an edge of one
        // operand lying on the other's where a face is flush with it):
        // there or not as the perturbation moves the edge across the side.
        for h in &mut found {
            if let Some(inside) = self.tie_inside(side, e, f, &edge, patch, h) {
                (h.out, h.u) = if inside {
                    (0.0, h.u.max(DVec3::splat(0.0)))
                } else {
                    (f64::INFINITY, h.u)
                };
            }
        }
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
/// their edges, until they do (see [`fit_count`]); any the search didn't
/// find go where it found the two meeting but kept no crossing (a tie at
/// the patch's side or the edge's end, whose sign the count overrules:
/// nearest the patch), else at `closest`; as `(sign, t, solved)` in order
/// along the edge, those only placed not solved.
fn pick(found: &[solve::EdgeHit], x: i32, closest: f64) -> Vec<(i8, f64, bool)> {
    let (chosen, missing) = fit_count(
        found
            .iter()
            .map(|h| (h.x, h.out, h.t.min(1.0 - h.t).min(h.u.min_element()))),
        x,
    );
    // Where the search found the two meeting and the count kept nothing:
    // the edge touches the patch there. At a tangency, or where the edge
    // leaves a vertex on the patch's corner, a crossing the count has
    // there, but not with the sign found, is missed, and `closest` is only
    // the middle of the smallest pieces the search looked at, which put a
    // crossing at a vertex a sixteenth of the edge away from it.
    let spare = found
        .iter()
        .zip(&chosen)
        .filter(|&(_, &c)| !c)
        .min_by(|a, b| a.0.out.total_cmp(&b.0.out))
        .map(|(h, _)| h.t);
    let at = spare.unwrap_or(closest).clamp(0.0, 1.0);
    let mut out: Vec<(i8, f64, bool)> = missing.into_iter().map(|s| (s, at, false)).collect();
    for (h, &c) in found.iter().zip(&chosen) {
        if c {
            out.push((h.x, h.t.clamp(0.0, 1.0), true));
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
