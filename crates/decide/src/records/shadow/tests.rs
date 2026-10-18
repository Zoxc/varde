//! The shadow records against the moved configuration they stand for:
//! every point–vertex and ray record equals the exact signs of the
//! operands really moved (in rationals, `A`'s points by tiny powers of
//! two times the motions' rates, one direction for all of `A` or one per
//! vertex, edges staying straight); a closed chain's ray records sum to its
//! winding number, the same as a ray in another direction counts, also
//! when the ray runs through its vertices (half-open ends), at every
//! perturbation order a tie reaches; the sign convention's two checks
//! (`m1`: a counter-clockwise square winds `+1` for either ray direction,
//! and the four-term crossing identity; `m2`: rays signed by their actual
//! direction); one record per point pair whichever side reads it; each
//! record decided once and read by every ray record that needs it; the
//! same records, work and reads on any number of threads and for any
//! order of the wanted keys, and nothing stored of a pass that runs out.

use std::collections::BTreeMap;

use glam::DVec3;
use malachite::Rational as Q;
use malachite::base::num::arithmetic::traits::PowerOf2;

use super::*;
use crate::ids::{EdgeId, NONE};
use crate::num::exact::ORDERS;
use crate::perturb::{NU, R_HAT, Rate, T, UP, rates_a};
use crate::records::keys::{Query, XClusterIx};

use Operand::{A, B};

/// One operand's chain or loop of edges, and points of both.
#[derive(Default)]
struct Geom {
    pos: BTreeMap<Point, DVec3>,
    edges: BTreeMap<(Operand, EdgeId), [VertId; 2]>,
    radius: BTreeMap<Point, f64>,
    /// `A`'s direction field: this vector, so `A` moves as a translation,
    /// except at the vertices of `dirs`.
    d: DVec3,
    /// `A` vertices with a direction of their own; a straight edge then
    /// moves as its ends do, and stays straight.
    dirs: BTreeMap<VertId, DVec3>,
    sigma_a: Option<Sign>,
    /// Extra bits of smallness of the motions, for tiny coordinates.
    fine: i64,
}

impl Geom {
    fn with(d: DVec3, sigma_a: Sign) -> Geom {
        Geom {
            d,
            sigma_a: Some(sigma_a),
            ..Geom::default()
        }
    }

    fn vert(&mut self, op: Operand, id: u32, at: DVec3) -> VertId {
        let p = Point::vert(op, VertId(id));
        assert!(self.pos.insert(p, at).is_none(), "{p:?} twice");
        VertId(id)
    }

    /// Edge `id` between `v` and `w`, stored from the lower id.
    fn edge(&mut self, op: Operand, id: u32, v: VertId, w: VertId) -> EdgeId {
        let ends = if v < w { [v, w] } else { [w, v] };
        assert!(self.edges.insert((op, EdgeId(id)), ends).is_none());
        EdgeId(id)
    }

    /// The direction of `A`'s point `p`.
    fn dir(&self, p: Point) -> DVec3 {
        match p.q {
            Query::Vert(v) if p.operand == A => self.dirs.get(&v).copied().unwrap_or(self.d),
            _ => self.d,
        }
    }

    fn sigma(&self) -> Sign {
        self.sigma_a.unwrap_or(Sign::Pos)
    }

    fn rates(&self, p: Point) -> [Rate; ORDERS] {
        match p.operand {
            A => rates_a(self.sigma(), self.dir(p)),
            B => [Rate::ZERO; ORDERS],
        }
    }

    /// `p`'s position moved: `A` by `δ·(σ_A·d + η·T₂ + η²·T₃ + η³·T₄)`,
    /// `δ = 2^−(200 + fine)`, `η = 2^−20`, far enough apart that each motion
    /// decides only where the ones before are exact zeros, as the
    /// perturbation's orders do, and `δ² ≪ δ·η³`, so an edge whose ends
    /// move apart is decided to first order in `δ` as the rows are.
    fn moved(&self, p: Point) -> [Q; 3] {
        let at = q3(self.pos[&p]);
        if p.operand == B {
            return at;
        }
        let sigma = match self.sigma() {
            Sign::Pos => 1.0,
            Sign::Neg => -1.0,
        };
        let rates = [self.dir(p) * sigma, T[0], T[1], T[2]];
        let mut out = at;
        for (k, r) in (0i64..).zip(rates) {
            let eps = Q::power_of_2(-200 - self.fine - 20 * k);
            let r = q3(r);
            for i in 0..3 {
                out[i] = &out[i] + &eps * &r[i];
            }
        }
        out
    }

    fn ends_moved(&self, op: Operand, g: EdgeId) -> [[Q; 3]; 2] {
        self.edges[&(op, g)].map(|v| self.moved(Point::vert(op, v)))
    }
}

impl ShadowGeometry for Geom {
    fn point(&self, p: Point) -> Moving {
        Moving {
            at: self.pos[&p],
            rates: self.rates(p),
        }
    }

    fn near_radius(&self, p: Point) -> f64 {
        self.radius.get(&p).copied().unwrap_or(0.0)
    }

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&(operand, g)]
    }

    /// On `A`, the rates at the foot of `q`'s shadow: the ends' directions
    /// interpolated there, exactly.
    fn line(&self, q: Point, g: EdgeId) -> Seg {
        let op = q.operand.other();
        let ends = self.edges[&(op, g)].map(|v| Point::vert(op, v));
        let [a, b] = ends.map(|p| self.pos[&p]);
        let [d0, d1] = ends.map(|p| self.dir(p));
        let rates = match op {
            B => [Rate::ZERO; ORDERS],
            A if d0 == d1 => rates_a(self.sigma(), d0),
            A => {
                // The foot's parameter on the line, in the shadow plane
                // (dot products across UP, times |UP|²).
                let (t, u, up) = (q3(b - a), q3(self.pos[&q] - a), q3(UP));
                let across = |x: &[Q; 3], y: &[Q; 3]| {
                    qdot(&up, &up) * qdot(x, y) - qdot(x, &up) * qdot(y, &up)
                };
                let s = across(&u, &t) / across(&t, &t);
                let (d0, d1) = (q3(d0), q3(d1));
                let foot = [0, 1, 2].map(|i| &d0[i] + &s * (&d1[i] - &d0[i]));
                rates_a(self.sigma(), Rate::exact(foot))
            }
        };
        Seg { a, b, rates }
    }
}

/// A pass deciding in order on one thread, counting the keys it decided
/// and the work.
#[derive(Default)]
struct Seq {
    decided: usize,
    work: Cost,
}

impl Pass for Seq {
    type Error = Fail;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, Fail> {
        keys.iter()
            .map(|k| {
                self.decided += 1;
                let (v, w) = decide(k);
                self.work = self.work.and(w);
                v
            })
            .collect()
    }
}

/// `2^k`, exactly, for `−1022 ≤ k ≤ 1023`.
fn pow2(k: i32) -> f64 {
    let e = u64::try_from(1023 + k).expect("a normal power of two");
    f64::from_bits(e << 52)
}

fn q3(p: DVec3) -> [Q; 3] {
    [p.x, p.y, p.z].map(|x| Q::try_from(x).expect("finite"))
}

fn qsub(a: &[Q; 3], b: &[Q; 3]) -> [Q; 3] {
    [0, 1, 2].map(|i| &a[i] - &b[i])
}

fn qdot(a: &[Q; 3], b: &[Q; 3]) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn qcross(a: &[Q; 3], b: &[Q; 3]) -> [Q; 3] {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

fn qsign(x: &Q) -> Sign {
    assert!(*x != 0u32, "the moved configuration has a tie");
    if *x > 0u32 { Sign::Pos } else { Sign::Neg }
}

/// The ray direction of `op`'s points.
fn ray_dir(op: Operand) -> [Q; 3] {
    match op {
        A => q3(-R_HAT),
        B => q3(R_HAT),
    }
}

/// The signed crossing of the ray from `q` along `dir` with the segment
/// `a → b`, in the shadow plane: `sign(UP·(dir × (b − a)))`, or 0. Exact,
/// on the moved positions, solved directly (no records, no perturbation).
fn crossing(q: &[Q; 3], dir: &[Q; 3], a: &[Q; 3], b: &[Q; 3]) -> i8 {
    let up = q3(UP);
    let t = qsub(b, a);
    // The shadow plane's coordinates of x along a basis (dir, n), n = up × dir.
    let n = qcross(&up, dir);
    let den = qdot(&n, &t);
    if den == 0u32 {
        return 0; // parallel to the ray
    }
    let s = qdot(&n, &qsub(q, a)) / &den;
    assert!(s != 0u32 && s != 1u32, "the ray runs through a moved end");
    if !(s > 0u32 && s < 1u32) {
        return 0;
    }
    let x = [0, 1, 2].map(|i| &a[i] + &s * &t[i]);
    let along = qdot(dir, &qsub(&x, q));
    if qsign(&along) == Sign::Neg {
        return 0;
    }
    qsign(&qdot(&up, &qcross(dir, &t))).as_i8()
}

/// The moved configuration's point–vertex record of `q` against `w`.
fn vv_oracle(g: &Geom, q: Point, w: VertId) -> (Sign, Sign) {
    let d = qsub(&g.moved(Point::vert(q.operand.other(), w)), &g.moved(q));
    (qsign(&qdot(&q3(NU), &d)), qsign(&qdot(&q3(R_HAT), &d)))
}

/// The moved configuration's ray record of `q` against edge `e`.
fn ray_oracle(g: &Geom, q: Point, e: EdgeId) -> i8 {
    let [a, b] = g.ends_moved(q.operand.other(), e);
    crossing(&g.moved(q), &ray_dir(q.operand), &a, &b)
}

/// The winding number of the moved closed chain `cycle` (edges with the
/// sense they are run in) about `q`, by a ray across `r̂` (along `ν`).
fn winding_oracle(g: &Geom, q: Point, cycle: &[(EdgeId, bool)]) -> i32 {
    let op = q.operand.other();
    let nu = q3(NU);
    cycle
        .iter()
        .map(|&(e, forward)| {
            let [a, b] = g.ends_moved(op, e);
            let c = i32::from(crossing(&g.moved(q), &nu, &a, &b));
            if forward { c } else { -c }
        })
        .sum()
}

/// Fills the ray records of `q` against every edge of `cycle` and gives
/// their sum with the senses: the winding number the counting reads.
fn ray_sum(s: &mut Shadows, g: &Geom, q: Point, cycle: &[(EdgeId, bool)]) -> i32 {
    let keys: Vec<RayKey> = cycle.iter().map(|&(e, _)| RayKey { q, g: e }).collect();
    s.fill_rays(keys, g, &mut Seq::default()).expect("decided");
    cycle
        .iter()
        .map(|&(e, forward)| {
            let c = i32::from(s.ray(q, e).expect("stored").count());
            if forward { c } else { -c }
        })
        .sum()
}

/// A closed loop of `op` through `pts` in order, vertex ids `ids[i]`, edge
/// ids from `first_edge`: each edge with the sense it is run in.
fn closed(
    g: &mut Geom,
    op: Operand,
    pts: &[DVec3],
    ids: &[u32],
    first_edge: u32,
) -> Vec<(EdgeId, bool)> {
    let vs: Vec<VertId> = pts
        .iter()
        .zip(ids)
        .map(|(&p, &i)| g.vert(op, i, p))
        .collect();
    (0..vs.len())
        .map(|i| {
            let (v, w) = (vs[i], vs[(i + 1) % vs.len()]);
            (g.edge(op, first_edge + i as u32, v, w), v < w)
        })
        .collect()
}

/// A point of the lattice of the shadow frame: `x` along `r̂`, `y` along
/// `ν`, `z` along `UP`. Points with equal `y` lie on one `r̂` line, with
/// equal `x` across from each other, with both equal on one column.
fn lat(x: f64, y: f64, z: f64) -> DVec3 {
    R_HAT * x + NU * y + UP * z
}

/// The shadow plane of the review's checks, in tenths: the axes
/// `(19, −514, 47)` and `(−30, 5140, −480)`, `⊥ UP`, counter-clockwise
/// seen from above, with `(1, 0.1) ↦ r̂` exactly.
fn tenths(x: i32, y: i32) -> DVec3 {
    let ex = DVec3::new(19.0, -514.0, 47.0);
    let ey = DVec3::new(-30.0, 5140.0, -480.0);
    ex * f64::from(x) + ey * f64::from(y)
}

#[test]
fn the_check_frame_maps_the_review_direction_to_r_hat() {
    assert_eq!(tenths(10, 1), R_HAT * 10.0);
    let (ex, ey) = (tenths(1, 0), tenths(0, 1));
    assert_eq!(ex.dot(UP), 0.0);
    assert_eq!(ey.dot(UP), 0.0);
    assert!(ex.cross(ey).dot(UP) > 0.0);
}

/// A ray record's count, filled on demand.
fn ray(s: &mut Shadows, g: &Geom, q: Point, e: EdgeId) -> i32 {
    s.fill_rays([RayKey { q, g: e }], g, &mut Seq::default())
        .expect("decided");
    i32::from(s.ray(q, e).expect("stored").count())
}

/// m1: a counter-clockwise square winds `+1` about a point inside it for
/// either ray direction (an `A` point's ray goes `−r̂`, a `B` point's
/// `+r̂`); and with `e = (0,0) → (2,0)` of `A` under the up-facing square
/// `f = [−1, 1]²` of `B`, `g = (1,−1) → (1,1)` its right side, the four
/// ray terms give `X(e, g) = −1`, the value `s11` needs: `s02(a, f) = 1`,
/// `s02(b, f) = 0`, and `e` passes under `f` without crossing it, so
/// `x12 = s02(b) − s02(a) − Σ_c sgn(c)·X(e, g_c) = 0`.
#[test]
fn m1_a_counter_clockwise_square_winds_once_and_x_has_the_identity_sign() {
    let corners = [(-10, -10), (10, -10), (10, 10), (-10, 10)].map(|(x, y)| tenths(x, y));
    for op in [A, B] {
        let mut g = Geom::with(DVec3::ZERO, Sign::Pos);
        // f's corners raised above the shadow plane, ids so that the
        // right side runs from (1, −1) up to (1, 1).
        let up: Vec<DVec3> = corners.iter().map(|&c| c + UP * 4.0).collect();
        let f = closed(&mut g, op.other(), &up, &[3, 0, 1, 2], 0);
        let a = Point::vert(op, g.vert(op, 10, tenths(0, 0)));
        let b = Point::vert(op, g.vert(op, 11, tenths(20, 0)));
        let mut s = Shadows::new();
        assert_eq!(ray_sum(&mut s, &g, a, &f), 1, "inside, rays of {op:?}");
        assert_eq!(ray_sum(&mut s, &g, b, &f), 0, "outside, rays of {op:?}");
        for q in [a, b] {
            assert_eq!(ray_sum(&mut s, &g, q, &f), winding_oracle(&g, q, &f));
        }
    }

    let mut g = Geom::with(DVec3::ZERO, Sign::Pos);
    let up: Vec<DVec3> = corners.iter().map(|&c| c + UP * 4.0).collect();
    let f = closed(&mut g, B, &up, &[3, 0, 1, 2], 0);
    let (gid, gfwd) = f[1];
    assert_eq!(g.ends(B, gid), [VertId(0), VertId(1)]);
    assert!(gfwd, "g runs (1, −1) → (1, 1), as f's boundary does");
    let a = g.vert(A, 10, tenths(0, 0));
    let b = g.vert(A, 11, tenths(20, 0));
    let e = g.edge(A, 20, a, b);
    let mut s = Shadows::new();
    // X(e, g) = Ray(b, g) − Ray(a, g) + Ray(g0, e) − Ray(g1, e).
    let x = |s: &mut Shadows, h: EdgeId| {
        let [h0, h1] = g.ends(B, h);
        ray(s, &g, Point::vert(A, b), h) - ray(s, &g, Point::vert(A, a), h)
            + ray(s, &g, Point::vert(B, h0), e)
            - ray(s, &g, Point::vert(B, h1), e)
    };
    assert_eq!(x(&mut s, gid), -1);
    assert_eq!(ray(&mut s, &g, Point::vert(A, b), gid), -1);
    let (s02_a, s02_b) = (1, 0);
    let sum: i32 = f
        .iter()
        .map(|&(h, fwd)| if fwd { x(&mut s, h) } else { -x(&mut s, h) })
        .sum();
    assert_eq!(s02_b - s02_a - sum, 0, "x12 of an edge passing under f");
}

/// m2: rays are signed by their actual direction. `e = (0,0) → (2,0)` of
/// `A`, `g = (−5,−0.6) → (−5,5)` of `B`: `Ray(a, g) = −1` (the `−r̂` ray
/// crosses `g` going down-left), `Ray(g0, e) = −1` (the `+r̂` ray crosses
/// `e`), the others 0, so `X = 0`; signing both against a fixed `r̂` would
/// give 2.
#[test]
fn m2_rays_are_signed_by_their_actual_direction() {
    let mut g = Geom::with(DVec3::ZERO, Sign::Pos);
    let a = g.vert(A, 0, tenths(0, 0));
    let b = g.vert(A, 1, tenths(20, 0));
    let e = g.edge(A, 0, a, b);
    let g0 = g.vert(B, 0, tenths(-50, -6) + UP * 3.0);
    let g1 = g.vert(B, 1, tenths(-50, 50) + UP * 3.0);
    let h = g.edge(B, 0, g0, g1);
    let mut s = Shadows::new();
    let ra_g = ray(&mut s, &g, Point::vert(A, a), h);
    let rb_g = ray(&mut s, &g, Point::vert(A, b), h);
    let rg0_e = ray(&mut s, &g, Point::vert(B, g0), e);
    let rg1_e = ray(&mut s, &g, Point::vert(B, g1), e);
    assert_eq!((ra_g, rb_g, rg0_e, rg1_e), (-1, 0, -1, 0));
    assert_eq!(rb_g - ra_g + rg0_e - rg1_e, 0);
    for (q, e) in [
        (Point::vert(A, a), h),
        (Point::vert(A, b), h),
        (Point::vert(B, g0), e),
        (Point::vert(B, g1), e),
    ] {
        assert_eq!(i32::from(ray_oracle(&g, q, e)), ray(&mut s, &g, q, e));
    }
}

/// The directions of `A` that make a point–vertex tie reach each order:
/// a generic one (order 1 decides side and ahead), none (the first
/// translation, order 2), one along `r̂` (side at order 2, ahead at 1),
/// one along `ν` (side at 1, ahead at 2), one along `UP` (both at 2).
const DIRECTIONS: [DVec3; 5] = [DVec3::new(0.25, -0.5, 0.125), DVec3::ZERO, R_HAT, NU, UP];

/// The order a flat row's sign is decided at.
fn order(pred: &impl Pred) -> usize {
    exact::sign(pred).first.expect("decided").0
}

/// A ray through a vertex shared by two edges counts once: a chain of two
/// edges through `w`, on `q`'s `r̂` line and on its ray, crosses the ray
/// once when it goes on across, never when it turns back, whichever side
/// of `w` the perturbation puts the ray, at each order a tie reaches, for
/// rays of either operand and every id order of the chain; each answer the
/// moved configuration's. With `w` on `q`'s column (its shadow `q`'s) the
/// perturbation also puts `w` ahead or behind: the moved chain crosses the
/// ray line next to `q`, ahead or behind it, so the ray counts at most
/// one crossing (a chain turning back at `w` around the moved `q` crosses
/// the line twice, once on either side).
#[test]
fn a_ray_through_a_shared_vertex_counts_once() {
    let mut side_orders = [false; ORDERS + 1];
    let mut along_orders = [false; ORDERS + 1];
    for d in DIRECTIONS {
        for sigma in [Sign::Pos, Sign::Neg] {
            for op in [A, B] {
                // A's rays go −r̂, B's +r̂.
                let on_ray = match op {
                    A => -2.0,
                    B => 2.0,
                };
                for w_x in [on_ray, 0.0] {
                    for across in [true, false] {
                        for ids in [[0, 1, 2], [2, 1, 0], [1, 0, 2], [0, 2, 1]] {
                            let mut g = Geom::with(d, sigma);
                            let far = if across { -1.0 } else { 1.0 };
                            let other = op.other();
                            let u = g.vert(other, ids[0], lat(w_x - 1.0, 1.0, 2.0));
                            let w = g.vert(other, ids[1], lat(w_x, 0.0, -1.0));
                            let v = g.vert(other, ids[2], lat(w_x + 1.0, far, 3.0));
                            let e0 = g.edge(other, 0, u, w);
                            let e1 = g.edge(other, 1, w, v);
                            let q = Point::vert(op, g.vert(op, 7, lat(0.0, 0.0, 0.0)));
                            let mut s = Shadows::new();
                            let keys = [RayKey { q, g: e0 }, RayKey { q, g: e1 }];
                            s.fill_rays(keys, &g, &mut Seq::default()).unwrap();
                            let chain = [(e0, u < w), (e1, w < v)];
                            let sum: i32 = chain
                                .iter()
                                .map(|&(e, fwd)| {
                                    let c = i32::from(s.ray(q, e).unwrap().count());
                                    assert_eq!(c, i32::from(ray_oracle(&g, q, e)));
                                    if fwd { c } else { -c }
                                })
                                .sum();
                            let at = format!("{d:?} {sigma:?} {op:?} {w_x} {ids:?}");
                            if w_x == on_ray {
                                assert_eq!(sum.abs(), i32::from(across), "{at}");
                            } else {
                                assert!(sum.abs() <= 1, "{at}");
                            }
                            let rec = s.vv(q, w).unwrap();
                            assert_eq!(
                                (rec.place.side, rec.place.ahead),
                                vv_oracle(&g, q, w),
                                "{at}"
                            );
                            assert!(w_x != on_ray || rec.place.on_ray(op));
                            let row = |axis| VvShadow {
                                q: g.point(q),
                                w: g.point(Point::vert(other, w)),
                                axis,
                            };
                            side_orders[order(&row(ShadowAxis::Side))] = true;
                            along_orders[order(&row(ShadowAxis::Along))] = true;
                        }
                    }
                }
            }
        }
    }
    // The side of w is a tie, decided at order 1 or 2 (the first
    // translation is off every shadow axis, so no later order is reached);
    // ahead is a tie on q's column.
    assert_eq!(side_orders, [false, true, true, false, false]);
    assert_eq!(along_orders, [true, true, true, false, false]);
}

/// A point on the shadow line of an edge, between its ends along `r̂`:
/// only the point–line row places the crossing, here decided at order 3
/// (no direction, and the edge along the first translation, so its
/// shadow line is moved along itself by it).
#[test]
fn a_point_on_an_edge_s_shadow_is_placed_by_the_line_row_at_order_three() {
    for op in [A, B] {
        for flip in [false, true] {
            let mut g = Geom::with(DVec3::ZERO, Sign::Pos);
            let other = op.other();
            let t = T[0] * 16.0;
            let c = lat(1.0, -1.0, 2.0);
            let (lo, hi) = if flip { (c + t, c - t) } else { (c - t, c + t) };
            let g0 = g.vert(other, 0, lo);
            let g1 = g.vert(other, 1, hi);
            let e = g.edge(other, 0, g0, g1);
            let q = Point::vert(op, g.vert(op, 0, c + UP * 5.0));
            let mut s = Shadows::new();
            let got = ray(&mut s, &g, q, e);
            assert_eq!(got, i32::from(ray_oracle(&g, q, e)), "{op:?} {flip}");
            let row = LineShadow {
                q: g.point(q),
                line: g.line(q, e),
            };
            assert_eq!(order(&row), 3);
            let (r0, r1) = (s.vv(q, g0).unwrap(), s.vv(q, g1).unwrap());
            assert_ne!(r0.place.side, r1.place.side);
            assert_ne!(r0.place.on_ray(op), r1.place.on_ray(op));
        }
    }
}

/// Random closed loops on the shadow frame's lattice, so vertices lie on
/// query points' ray lines, across from them and on their columns: every
/// record is the moved configuration's, and the ray records of a loop sum
/// to its winding number about the point, which a ray across `r̂` counts
/// the same.
#[test]
fn records_on_lattice_loops_are_the_moved_configuration_s() {
    for seed in varde_testing::seeds(64, 1024) {
        let mut r = varde_testkit::rng::Rng::new(seed);
        let mut int = |n: u64| (r.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let d = DIRECTIONS[(int(2) + 2.0) as usize];
        let sigma = if int(1) >= 0.0 { Sign::Pos } else { Sign::Neg };
        let op = if int(1) >= 0.0 { A } else { B };
        let mut g = Geom::with(d, sigma);
        let n = 3 + (int(2) + 2.0) as usize;
        let pts: Vec<DVec3> = (0..n).map(|_| lat(int(3), int(3), int(3))).collect();
        let mut ids: Vec<u32> = (0..n as u32).collect();
        for i in (1..n).rev() {
            ids.swap(i, (int(n as u64) + n as f64) as usize % (i + 1));
        }
        let lp = closed(&mut g, op.other(), &pts, &ids, 0);
        let qs: Vec<Point> = (0..4)
            .map(|i| Point::vert(op, g.vert(op, 100 + i, lat(int(3), int(3), int(3)))))
            .collect();
        let mut s = Shadows::new();
        for &q in &qs {
            let w = ray_sum(&mut s, &g, q, &lp);
            assert_eq!(w, winding_oracle(&g, q, &lp), "seed {seed}");
            for &(e, _) in &lp {
                let got = s.ray(q, e).unwrap().count();
                assert_eq!(got, ray_oracle(&g, q, e), "seed {seed}, {q:?} {e:?}");
                for v in g.ends(op.other(), e) {
                    let rec = s.vv(q, v).unwrap();
                    assert_eq!(
                        (rec.place.side, rec.place.ahead),
                        vv_oracle(&g, q, v),
                        "seed {seed}"
                    );
                }
            }
        }
        assert_eq!(s.replay(&g), Ok(()));
    }
}

/// As above, with a direction of its own at each vertex of `A`: an edge's
/// ends move apart, so a ray line through both ends' shadows (an edge
/// along `r̂`, a point on it) is crossed only by the motion, and a point
/// on the edge's shadow line is placed by the line moving at its rate at
/// the foot. Every record is still the moved configuration's, and a
/// loop's ray records sum to its winding number. (Edges whose shadow is a
/// point are left out: they have no shadow line.)
#[test]
fn records_with_a_direction_per_vertex_are_the_moved_configuration_s() {
    for seed in varde_testing::seeds(64, 1024) {
        let mut r = varde_testkit::rng::Rng::new(seed);
        let mut int = |n: u64| (r.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let sigma = if int(1) >= 0.0 { Sign::Pos } else { Sign::Neg };
        let op = if int(1) >= 0.0 { A } else { B };
        let mut g = Geom::with(DIRECTIONS[0], sigma);
        let n = 3 + (int(2) + 2.0) as usize;
        let pts: Vec<DVec3> = loop {
            let xy: Vec<(f64, f64)> = (0..n).map(|_| (int(2), int(2))).collect();
            if (0..n).all(|i| xy[i] != xy[(i + 1) % n]) {
                break xy.into_iter().map(|(x, y)| lat(x, y, int(3))).collect();
            }
        };
        let mut ids: Vec<u32> = (0..n as u32).collect();
        for i in (1..n).rev() {
            ids.swap(i, (int(n as u64) + n as f64) as usize % (i + 1));
        }
        let lp = closed(&mut g, op.other(), &pts, &ids, 0);
        let qs: Vec<Point> = (0..4)
            .map(|i| Point::vert(op, g.vert(op, 100 + i, lat(int(2), int(2), int(3)))))
            .collect();
        let a_verts: Vec<VertId> = g
            .pos
            .keys()
            .filter_map(|p| match p.q {
                Query::Vert(v) if p.operand == A => Some(v),
                _ => None,
            })
            .collect();
        for v in a_verts {
            let d = DIRECTIONS[(int(2) + 2.0) as usize];
            g.dirs.insert(v, d);
        }
        let mut s = Shadows::new();
        for &q in &qs {
            let w = ray_sum(&mut s, &g, q, &lp);
            assert_eq!(w, winding_oracle(&g, q, &lp), "seed {seed}");
            for &(e, _) in &lp {
                let got = s.ray(q, e).unwrap().count();
                assert_eq!(got, ray_oracle(&g, q, e), "seed {seed}, {q:?} {e:?}");
                for v in g.ends(op.other(), e) {
                    let rec = s.vv(q, v).unwrap();
                    assert_eq!(
                        (rec.place.side, rec.place.ahead),
                        vv_oracle(&g, q, v),
                        "seed {seed}"
                    );
                }
            }
        }
        assert_eq!(s.replay(&g), Ok(()));
    }
}

/// One record per point pair, `A`'s point first: read from the `B` point
/// it has side and ahead negated, `near` and "the ray passes" as stored;
/// each reading is the moved configuration's from its own point. A cut
/// point of `B` keys the same way.
#[test]
fn a_point_pair_has_one_record_read_from_either_side() {
    let mut g = Geom::with(DVec3::new(0.5, 0.25, -1.0), Sign::Neg);
    let a = g.vert(A, 3, lat(0.0, 0.0, 1.0));
    let b = g.vert(B, 1, lat(-2.0, 0.0, 4.0)); // on a's ray
    let cut = Point {
        operand: B,
        q: Query::Cut(EdgeId(5), EdgeId(2), XClusterIx(0)),
    };
    g.pos.insert(cut, lat(1.0, 1.0, 0.0));
    let (pa, pb) = (Point::vert(A, a), Point::vert(B, b));
    g.radius.insert(pa, 2.0);
    let mut s = Shadows::new();
    let keys = [
        VvKey::of(pa, b).key,
        VvKey::of(pb, a).key,
        VvKey::of(cut, a).key,
    ];
    let mut pass = Seq::default();
    s.fill_vv(keys, &g, &mut pass).unwrap();
    assert_eq!(pass.decided, 2);
    assert_eq!(s.vv_table().len(), 2);

    let from_a = s.vv(pa, b).unwrap();
    let from_b = s.vv(pb, a).unwrap();
    assert_eq!(from_b.place.side, neg(from_a.place.side));
    assert_eq!(from_b.place.ahead, neg(from_a.place.ahead));
    assert_eq!(from_b.near, from_a.near);
    assert_eq!(from_a.place.on_ray(A), from_b.place.on_ray(B));
    assert!(from_a.place.on_ray(A));
    assert_eq!(
        (from_a.place.side, from_a.place.ahead),
        vv_oracle(&g, pa, b)
    );
    assert_eq!(
        (from_b.place.side, from_b.place.ahead),
        vv_oracle(&g, pb, a)
    );
    let from_cut = s.vv(cut, a).unwrap();
    let at = |p| q3(g.pos[&p]);
    let d = qsub(&g.moved(pa), &at(cut));
    assert_eq!(from_cut.place.side, qsign(&qdot(&q3(NU), &d)));
    assert_eq!(from_cut.place.ahead, qsign(&qdot(&q3(R_HAT), &d)));
    #[cfg(debug_assertions)]
    assert_eq!(s.vv_table().reads(&VvKey::of(pa, b).key), Some(2));
}

/// `near` compares the stored distance with the larger of the two radii,
/// exactly, the bound included.
#[test]
fn near_is_the_stored_distance_within_the_larger_radius() {
    for (ra, rb, near) in [
        (5.0, 1.0, true),
        (1.0, 5.0, true),
        (4.999, 0.0, false),
        (0.0, 0.0, false),
    ] {
        let mut g = Geom::with(DVec3::ZERO, Sign::Pos);
        let a = g.vert(A, 0, DVec3::new(1.0, 2.0, 3.0));
        let b = g.vert(B, 0, DVec3::new(4.0, 6.0, 3.0)); // 5 away
        g.radius.insert(Point::vert(A, a), ra);
        g.radius.insert(Point::vert(B, b), rb);
        let mut s = Shadows::new();
        let key = VvKey::of(Point::vert(A, a), b).key;
        s.fill_vv([key], &g, &mut Seq::default()).unwrap();
        assert_eq!(s.vv(Point::vert(A, a), b).unwrap().near, near, "{ra} {rb}");
    }
}

/// A near radius is any finite `r ≥ 0`, also one beyond the coordinates'
/// input range: points `2³³` apart are near within a radius of `2³³`, not
/// within the float below it.
#[test]
fn near_takes_a_radius_beyond_the_input_range() {
    let d = pow2(33);
    for (r, near) in [(d, true), (d.next_down(), false), (f64::MAX, true)] {
        let mut g = Geom::with(DVec3::ZERO, Sign::Pos);
        let a = g.vert(A, 0, DVec3::new(-d / 2.0, 0.0, 0.0));
        let b = g.vert(B, 0, DVec3::new(d / 2.0, 0.0, 0.0));
        g.radius.insert(Point::vert(B, b), r);
        let mut s = Shadows::new();
        let key = VvKey::of(Point::vert(A, a), b).key;
        s.fill_vv([key], &g, &mut Seq::default()).unwrap();
        assert_eq!(s.vv(Point::vert(A, a), b).unwrap().near, near, "{r:e}");
    }
}

/// Every record is decided once: filling the rays of points against a
/// loop decides each point–vertex record once, and each ray record reads
/// the stored records of its two ends (each vertex, the end of two edges,
/// read twice); asking again decides and reads nothing; a reader of the
/// other operand reads the same record.
#[test]
fn records_are_decided_once_and_read_by_every_ray() {
    let mut g = Geom::with(DVec3::new(1.0, 0.0, 0.0), Sign::Pos);
    let sq = [
        lat(-1.0, -1.0, 2.0),
        lat(1.0, -1.0, 2.0),
        lat(1.0, 1.0, 2.0),
        lat(-1.0, 1.0, 2.0),
    ];
    let lp = closed(&mut g, B, &sq, &[0, 1, 2, 3], 0);
    let qs: Vec<Point> = [lat(0.0, 0.0, 0.0), lat(3.0, 1.0, 0.0), lat(-3.0, -1.0, 0.0)]
        .iter()
        .enumerate()
        .map(|(i, &p)| Point::vert(A, g.vert(A, i as u32, p)))
        .collect();
    let keys: Vec<RayKey> = qs
        .iter()
        .flat_map(|&q| lp.iter().map(move |&(e, _)| RayKey { q, g: e }))
        .collect();
    let mut s = Shadows::new();
    let mut pass = Seq::default();
    s.fill_rays(keys.clone(), &g, &mut pass).unwrap();
    assert_eq!(pass.decided, qs.len() * 4 + keys.len());
    assert_eq!(s.vv_table().len(), qs.len() * 4);
    assert_eq!(s.ray_table().len(), keys.len());
    let ray_sum: i32 = lp
        .iter()
        .map(|&(e, f)| {
            let c = i32::from(s.ray(qs[0], e).unwrap().count());
            if f { c } else { -c }
        })
        .sum();
    assert_eq!(ray_sum, 1);

    s.fill_rays(keys.clone(), &g, &mut pass).unwrap();
    assert_eq!(pass.decided, qs.len() * 4 + keys.len());
    #[cfg(debug_assertions)]
    {
        for (k, _) in s.vv_table().iter().collect::<Vec<_>>() {
            // The iteration above read each once.
            assert_eq!(s.vv_table().reads(k), Some(3), "{k:?}");
        }
        let b = VertId(2);
        let key = VvKey::of(qs[0], b).key;
        let _ = s.vv(Point::vert(B, b), VertId(0));
        assert_eq!(s.vv_table().reads(&key), Some(4));
    }
}

/// A pass on `threads` threads: its chunks decided at once, the answers
/// given in key order, the work summed in key order, the first failure in
/// key order.
struct Par {
    threads: usize,
    decided: usize,
    work: Cost,
}

impl Pass for Par {
    type Error = Fail;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, Fail> {
        let n = keys.len().div_ceil(self.threads).max(1);
        let parts: Vec<Vec<Decided<V>>> = std::thread::scope(|s| {
            let hs: Vec<_> = keys
                .chunks(n)
                .map(|c| s.spawn(move || c.iter().map(decide).collect::<Vec<_>>()))
                .collect();
            hs.into_iter().map(|h| h.join().expect("joined")).collect()
        });
        let mut out = Vec::with_capacity(keys.len());
        for (v, w) in parts.into_iter().flatten() {
            self.decided += 1;
            self.work = self.work.and(w);
            out.push(v?);
        }
        Ok(out)
    }
}

/// A pass running out at its `at`-th key, counted over every batch.
struct OutAt {
    at: usize,
    seen: usize,
}

impl Pass for OutAt {
    type Error = &'static str;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, &'static str> {
        let mut out = Vec::new();
        for k in keys {
            if self.seen == self.at {
                return Err("out");
            }
            self.seen += 1;
            out.push(decide(k).0.map_err(|_| "failed")?);
        }
        Ok(out)
    }
}

/// The stored records of both tables, each in key order.
type Stored = (Vec<(VvKey, VvRec)>, Vec<(RayKey, RayRec)>);

/// Every stored record, in key order.
fn stored(s: &Shadows) -> Stored {
    (
        s.vv_table().iter().map(|(k, v)| (*k, *v)).collect(),
        s.ray_table().iter().map(|(k, v)| (*k, *v)).collect(),
    )
}

/// Closed walks over a small pool of vertices of one operand, so a vertex
/// is the end of many edges (a fan), a walk passes it more than once, and
/// runs of edges lie along query points' ray lines; points on the
/// vertices' columns and a cut point; ids up to `NONE`; coordinates scaled
/// up near the input range or down to where the rational stage decides.
/// Every record is the moved configuration's and every walk's ray records
/// sum to its winding number. The records and the work are the same on 1
/// to 4 threads and for the wanted keys permuted and repeated, and so are
/// the reads; a pass running out stores nothing of its batch, the end
/// records' batch before the rays'.
#[test]
fn records_of_walks_through_fans_are_the_moved_configuration_s_on_any_threads() {
    'seeds: for seed in varde_testing::seeds(24, 1024) {
        let mut r = varde_testkit::rng::Rng::new(seed);
        let mut int = |n: u64| (r.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let dirs = [DIRECTIONS[0], DIRECTIONS[1], R_HAT, NU, UP, -R_HAT - NU];
        let sigma = if int(1) >= 0.0 { Sign::Pos } else { Sign::Neg };
        let op = if int(1) >= 0.0 { A } else { B };
        let per_vertex = int(1) >= 0.0;
        let (scale, fine) = match (seed % 4, int(1) >= 0.0) {
            (1, _) => (pow2(20), 0),
            (2, true) => (pow2(-300), 300),
            (2, false) => (pow2(-560), 560),
            _ => (1.0, 0),
        };
        let mut g = Geom::with(dirs[(int(2) + 2.0) as usize], sigma);
        g.fine = fine;
        let other = op.other();
        let m = 3 + (int(2) + 2.0) as usize;
        let span = 1 + (int(1) + 1.0) as u64;
        let pool: Vec<[f64; 3]> = (0..m).map(|_| [int(span), int(span), int(3)]).collect();
        let vs: Vec<VertId> = (0..m)
            .map(|i| {
                let id = if seed % 3 == 0 {
                    NONE - i as u32
                } else {
                    3 * i as u32 + 1
                };
                let [x, y, z] = pool[i];
                g.vert(other, id, lat(x, y, z) * scale)
            })
            .collect();
        // An edge of `A` whose ends move apart needs a shadow line.
        let edge_ok = |i: usize, j: usize| {
            i != j && !(per_vertex && other == A && pool[i][..2] == pool[j][..2])
        };
        let mut walks: Vec<Vec<(EdgeId, bool)>> = Vec::new();
        let mut next_edge = 0u32;
        for _ in 0..1 + (int(1) + 1.0) as usize {
            let len = 3 + (int(3) + 3.0) as usize;
            let mut tries = 0;
            let walk = loop {
                let w: Vec<usize> = (0..len).map(|_| (int(8) + 8.0) as usize % m).collect();
                if (0..len).all(|i| edge_ok(w[i], w[(i + 1) % len])) {
                    break w;
                }
                tries += 1;
                if tries == 100 {
                    continue 'seeds;
                }
            };
            walks.push(
                (0..len)
                    .map(|i| {
                        let (v, w) = (vs[walk[i]], vs[walk[(i + 1) % len]]);
                        let id = if next_edge == 1 && seed % 2 == 0 {
                            NONE
                        } else {
                            next_edge
                        };
                        next_edge += 1;
                        (g.edge(other, id, v, w), v < w)
                    })
                    .collect(),
            );
        }
        let qs: Vec<Point> = (0..5u32)
            .map(|i| {
                let at = if int(1) >= 0.0 {
                    let [x, y, _] = pool[(int(8) + 8.0) as usize % m];
                    lat(x, y, int(4))
                } else {
                    lat(int(span + 1), int(span + 1), int(3))
                } * scale;
                let q = if i == 4 {
                    Point {
                        operand: op,
                        q: Query::Cut(EdgeId(NONE), EdgeId(NONE), XClusterIx(NONE)),
                    }
                } else {
                    Point::vert(op, VertId(if seed % 4 == 1 { NONE - i } else { 100 + i }))
                };
                g.pos.insert(q, at);
                q
            })
            .collect();
        if per_vertex {
            let a_verts: Vec<VertId> = g
                .pos
                .keys()
                .filter_map(|p| match p.q {
                    Query::Vert(v) if p.operand == A => Some(v),
                    _ => None,
                })
                .collect();
            for v in a_verts {
                g.dirs.insert(v, dirs[(int(2) + 3.0) as usize]);
            }
        }
        let keys: Vec<RayKey> = qs
            .iter()
            .flat_map(|&q| {
                walks
                    .iter()
                    .flatten()
                    .map(move |&(e, _)| RayKey { q, g: e })
            })
            .collect();

        let mut s = Shadows::new();
        let mut seq = Seq::default();
        s.fill_rays(keys.clone(), &g, &mut seq)
            .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
        let records = stored(&s);
        for &q in &qs {
            for w in &walks {
                let sum: i32 = w
                    .iter()
                    .map(|&(e, fwd)| {
                        let c = s.ray(q, e).expect("stored").count();
                        assert_eq!(c, ray_oracle(&g, q, e), "seed {seed}, {q:?} {e:?}");
                        let c = i32::from(c);
                        if fwd { c } else { -c }
                    })
                    .sum();
                assert_eq!(sum, winding_oracle(&g, q, w), "seed {seed}, {q:?}");
                for v in w.iter().flat_map(|&(e, _)| g.ends(other, e)) {
                    let rec = s.vv(q, v).expect("stored");
                    assert_eq!(
                        (rec.place.side, rec.place.ahead),
                        vv_oracle(&g, q, v),
                        "seed {seed}"
                    );
                }
            }
        }
        assert_eq!(s.replay(&g), Ok(()));

        // The reads of a fresh table filled once, on one thread.
        #[cfg(debug_assertions)]
        let reads: Vec<Option<u64>> = {
            let mut s1 = Shadows::new();
            s1.fill_rays(keys.clone(), &g, &mut Seq::default())
                .expect("decided");
            records
                .0
                .iter()
                .map(|(k, _)| s1.vv_table().reads(k))
                .collect()
        };
        for threads in 1..=4 {
            let mut wanted = keys.clone();
            wanted.rotate_left(seed as usize * threads % keys.len());
            wanted.reverse();
            wanted.extend(keys.iter().step_by(threads + 1));
            let mut par = Par {
                threads,
                decided: 0,
                work: Cost::default(),
            };
            let mut s2 = Shadows::new();
            s2.fill_rays(wanted, &g, &mut par).expect("decided");
            assert_eq!(
                (par.decided, par.work),
                (seq.decided, seq.work),
                "seed {seed}"
            );
            #[cfg(debug_assertions)]
            {
                let got: Vec<_> = records
                    .0
                    .iter()
                    .map(|(k, _)| s2.vv_table().reads(k))
                    .collect();
                assert_eq!(got, reads, "seed {seed}, {threads} threads");
            }
            assert_eq!(stored(&s2), records, "seed {seed}, {threads} threads");
        }

        let n_vv = records.0.len();
        let at = (seed as usize * 7) % seq.decided;
        let mut s3 = Shadows::new();
        let mut out = OutAt { at, seen: 0 };
        assert_eq!(
            s3.fill_rays(keys.clone(), &g, &mut out),
            Err(ShadowError::Pass("out"))
        );
        let (vv, rays) = stored(&s3);
        assert_eq!(vv.len(), if at < n_vv { 0 } else { n_vv }, "seed {seed}");
        assert!(rays.is_empty());
        s3.fill_rays(keys.clone(), &g, &mut Seq::default())
            .expect("decided");
        assert_eq!(stored(&s3), records);
    }
}

/// A record's cost in units: each kind of work at its price in parts,
/// summed over the exact stages and the levels, rounded up to whole units
/// once for the record, not per kind.
#[test]
fn a_record_s_cost_is_its_parts_rounded_up_once() {
    let u = RecordUnits {
        filter: 3,
        expansion: 5,
        rational: 7,
        eval: [1, 2, 3, 4, 5],
        root: [10, 20, 30, 40, 50],
    };
    let mut cost = Cost::exact(Work {
        filters: 2,
        expansions: 1,
        longest: 9,
        rationals: 1,
    });
    assert_eq!(cost.exact_parts(&u), 2 * 3 + 5 + 7);
    cost.levels[0].evals = 4;
    cost.levels[3].evals = 1;
    cost.levels[3].roots = RootWork {
        splits: 1,
        tests: 3,
        steps: 2,
    };
    assert_eq!(cost.level_parts(0, &u), 4);
    assert_eq!(cost.level_parts(3, &u), 4 + 6 * 40);
    let parts = 18 + 4 + 244;
    assert_eq!(cost.parts(&u), parts);
    assert_eq!(cost.units(&u), parts.div_ceil(UNIT_PARTS));
    // Below a unit, a record costs one; nothing costs nothing.
    let one = Cost::exact(Work {
        filters: 1,
        ..Work::default()
    });
    assert_eq!(one.units(&u), 1);
    assert_eq!(Cost::default().units(&UNITS.record), 0);
    // Counts past `u32` per kind saturate, never wrap.
    let mut huge = Cost::default();
    huge.levels[4].evals = u32::MAX;
    let dear = RecordUnits {
        eval: [u64::MAX; LEVELS],
        ..u
    };
    assert_eq!(huge.parts(&dear), u64::MAX);
}

/// Every kind with levels costs at least as much at a level as at the one
/// below, and something at each: a precision step is never free, nor
/// cheaper than the computation it repeats.
#[test]
fn the_unit_table_grows_with_the_level() {
    let r = &UNITS.record;
    for (kind, row) in [
        ("seam cell", UNITS.seam_cell),
        ("count piece", UNITS.count_piece),
        ("record eval", r.eval),
        ("record root", r.root),
    ] {
        assert!(row[0] > 0, "{kind}: {row:?}");
        assert!(row.windows(2).all(|w| w[0] <= w[1]), "{kind}: {row:?}");
    }
    assert!(r.filter > 0 && r.expansion > 0 && r.rational > 0);
}
