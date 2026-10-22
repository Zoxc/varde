//! Every row against central differences of the really moved geometry,
//! worked out in big floats: the moving operand's points moved by `δ`
//! times a random affine field (motion 1, standing in for the direction
//! field) or by `Tₖ` (motions 2 to 4), the other operand fixed, the
//! record's spatial value recomputed on the moved geometry (the foot, the
//! hit and the crossing solved again), and its difference quotient
//! compared with the row at several `δ`, to within `δ²`. Flat rows are
//! taken exactly ([`order_value`]) and divided by their positive scale,
//! at ties (where only the rate at the record's point counts); curved
//! rows at the big-float level, away from ties too where the row is the
//! derivative there (a shadow distance to the foot, a height over a
//! face). Then the frame and the translations, the flat records through
//! `decide`, the refusals of curved rows, and certified errors at every
//! level.

use malachite::{Float, Rational as Q};

use super::*;
use crate::ids::VertId;
use crate::num::bigf::{Big, Width};
use crate::num::decide::Row;
use crate::num::exact::{order_value, value};
use crate::records::keys::{Query, RecordKey, VvKey};

type B = Bounded<Big>;

/// The width of the moved geometry's arithmetic: 424 bits.
const W: Width = Width::ALL[1];

fn big(x: f64) -> B {
    Bounded::exact(Big::from_f64(x, W))
}

fn bigq(x: &Q) -> B {
    let (f, _) = Float::from_rational_prec(x.clone(), u64::from(W.bits()));
    Bounded::exact(Big::from_float(f, W).v)
}

fn f(x: &B) -> f64 {
    x.v.to_f64()
}

/// The value alone, its error dropped: the moved geometry's solvers only
/// need values.
fn clean(x: &B) -> B {
    Bounded::exact(x.v.clone())
}

fn v3(p: DVec3) -> [B; 3] {
    lit3(p, &big(0.0))
}

fn add3(a: &[B; 3], b: &[B; 3]) -> [B; 3] {
    [0, 1, 2].map(|k| a[k].add(&b[k]))
}

fn scale3(a: &[B; 3], s: &B) -> [B; 3] {
    [0, 1, 2].map(|k| a[k].mul(s))
}

fn p2(e: i32) -> f64 {
    f64::from_bits(u64::from((1023 + e) as u32) << 52)
}

/// The unit `UP`.
fn up() -> [B; 3] {
    unit3(&v3(UP))
}

/// A vector's shadow-plane components, along the unit `R_HAT` and `NU`.
fn shadow(v: &[B; 3]) -> [B; 2] {
    [dot3(&unit3(&v3(R_HAT)), v), dot3(&unit3(&v3(NU)), v)]
}

struct Rng(varde_testkit::rng::Rng);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(varde_testkit::rng::Rng::new(seed))
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        self.0.range(lo, hi)
    }

    /// A small integer in `-n..=n`.
    fn int(&mut self, n: u64) -> f64 {
        self.below(2 * n + 1) as f64 - n as f64
    }

    fn ints(&mut self, n: u64) -> DVec3 {
        DVec3::new(self.int(n), self.int(n), self.int(n))
    }

    /// A multiple of `1/8` in `[−1, 1]`: affine fields of these move
    /// half-integer points exactly in `f64`.
    fn eighths(&mut self) -> DVec3 {
        self.ints(8) / 8.0
    }

    fn field(&mut self) -> Field {
        Field {
            f0: self.eighths(),
            j: [self.eighths(), self.eighths(), self.eighths()],
        }
    }

    fn a_is_first(&mut self) -> bool {
        self.below(2) == 0
    }
}

/// An affine field `x ↦ f0 + J·x` (`J` by rows): an affine map keeps
/// lines straight and moves a rational curve or patch with its control
/// points, so the moved geometry is exact to build.
#[derive(Clone, Copy, Debug)]
struct Field {
    f0: DVec3,
    j: [DVec3; 3],
}

impl Field {
    fn constant(t: DVec3) -> Field {
        Field {
            f0: t,
            j: [DVec3::ZERO; 3],
        }
    }

    /// The field of motion `k` on `A` (`1..=ORDERS`): `own` for motion 1,
    /// then `Tₖ`.
    fn motion(own: &Field, k: usize) -> Field {
        if k == 1 {
            *own
        } else {
            Field::constant(T[k - 2])
        }
    }

    /// At a flat point, exactly for the tests' points and fields.
    fn at(&self, x: DVec3) -> DVec3 {
        self.f0 + DVec3::new(self.j[0].dot(x), self.j[1].dot(x), self.j[2].dot(x))
    }

    fn at_big(&self, x: &[B; 3]) -> [B; 3] {
        let f0 = v3(self.f0);
        [0, 1, 2].map(|k| f0[k].add(&dot3(&v3(self.j[k]), x)))
    }

    /// `x + δ·F(x)`.
    fn moved(&self, x: &[B; 3], delta: &B) -> [B; 3] {
        add3(x, &scale3(&self.at_big(x), delta))
    }

    /// Its rates at `x` along every motion, `A`'s field.
    fn rates(own: &Field, x: DVec3) -> [Rate; ORDERS] {
        std::array::from_fn(|i| Field::motion(own, i + 1).at(x).into())
    }

    fn rates_big(own: &Field, x: &[B; 3]) -> [[B; 3]; ORDERS] {
        std::array::from_fn(|i| Field::motion(own, i + 1).at_big(x))
    }
}

/// `δ` for the differences: `2⁻²⁰, 2⁻³⁰, 2⁻⁴⁰`.
const DELTAS: [i32; 3] = [-20, -30, -40];

/// The central difference of `moved` at each `δ` agrees with `row` to
/// within `10⁹·δ²` (generous for the geometry's
/// curvature): first-order agreement, at every step.
fn agrees(row: &B, moved: impl Fn(&B) -> B, what: &str) {
    for e in DELTAS {
        let d = big(p2(e));
        let fd = moved(&d).sub(&moved(&d.neg())).div(&d.add(&d));
        let diff = f(&fd.sub(row)).abs();
        let bound = 1e9 * p2(2 * e) * (1.0 + f(row).abs());
        assert!(
            diff <= bound,
            "{what}: δ = 2^{e}: difference {:e} vs row {:e} (off {diff:e})",
            f(&fd),
            f(row)
        );
    }
}

/// `x` solving `φ(x) = 0` near `x`, by Newton's method with a central
/// difference Jacobian (step `2⁻¹⁰⁰`, far below what the checks see).
fn solve2(x: [B; 2], phi: impl Fn(&[B; 2]) -> [B; 2]) -> [B; 2] {
    let h = big(p2(-100));
    let mut x = x;
    for _ in 0..12 {
        let r = phi(&x);
        let col = |i: usize| {
            let (mut xp, mut xm) = (x.clone(), x.clone());
            xp[i] = xp[i].add(&h);
            xm[i] = xm[i].sub(&h);
            let (p, m) = (phi(&xp), phi(&xm));
            [0, 1].map(|k| p[k].sub(&m[k]).div(&h.add(&h)))
        };
        let (c0, c1) = (col(0), col(1));
        let det = c0[0].mul(&c1[1]).sub(&c1[0].mul(&c0[1]));
        let d0 = r[0].mul(&c1[1]).sub(&c1[0].mul(&r[1])).div(&det);
        let d1 = c0[0].mul(&r[1]).sub(&r[0].mul(&c0[1])).div(&det);
        x = [clean(&x[0].sub(&d0)), clean(&x[1].sub(&d1))];
    }
    x
}

fn solve1(x: B, phi: impl Fn(&B) -> B) -> B {
    let [x, _] = solve2([x, big(0.0)], |x| [phi(&x[0]), x[1].clone()]);
    x
}

/// `a·x + b·y + c·z = r` for 3-vectors, by Cramer's rule.
fn cramer(a: &[B; 3], b: &[B; 3], c: &[B; 3], r: &[B; 3]) -> [B; 3] {
    let det = dot3(&cross3(a, b), c);
    [
        dot3(&cross3(r, b), c).div(&det),
        dot3(&cross3(a, r), c).div(&det),
        dot3(&cross3(a, b), r).div(&det),
    ]
}

/// A flat row's order `k` exactly, at the big-float width.
fn order_big<P: Pred>(pred: &P, k: usize) -> B {
    let q = match k {
        1 => order_value::<P, 1>(pred),
        2 => order_value::<P, 2>(pred),
        3 => order_value::<P, 3>(pred),
        _ => order_value::<P, 4>(pred),
    };
    bigq(&q)
}

/// A point of either operand: moved by `own` (motion 1) and `Tₖ` on `A`,
/// fixed on `B`.
fn flat_moving(at: DVec3, on_a: bool, own: &Field) -> Moving {
    if on_a {
        Moving {
            at,
            rates: Field::rates(own, at),
        }
    } else {
        Moving::fixed(at)
    }
}

/// The geometry of motion `k` at `δ`: `A`'s points moved, `B`'s kept.
fn mover(on_a: bool, own: &Field, k: usize) -> impl Fn(&[B; 3], &B) -> [B; 3] {
    let fld = Field::motion(own, k);
    move |x, d| if on_a { fld.moved(x, d) } else { x.clone() }
}

#[test]
fn the_frame_and_the_translations_are_exact_and_generic() {
    assert_eq!(R_HAT.dot(UP), 0.0);
    assert_eq!(UP.cross(R_HAT), NU);
    let m = glam::DMat3::from_cols(T[0], T[1], T[2]);
    assert!(m.determinant().abs() > 0.1);
    for t in T {
        assert!((t.length() - 1.0).abs() < 0.25, "{t}");
        for axis in [UP, R_HAT, NU, DVec3::X, DVec3::Y, DVec3::Z] {
            assert!(t.dot(axis).abs() > 0.05 * axis.length(), "{t} ⊥ {axis}");
        }
    }
    let d = DVec3::new(0.5, -1.0, 0.25);
    assert_eq!(rates_a(Sign::Pos, d), Rate::floats([d, T[0], T[1], T[2]]));
    assert_eq!(rates_a(Sign::Neg, d), Rate::floats([-d, T[0], T[1], T[2]]));
    // A curved point of `A` reads the same rates at every level.
    let z = Bounded::exact(0.0);
    for sigma in [Sign::Pos, Sign::Neg] {
        let flat = Moving::of_a(DVec3::new(1.0, 2.0, 3.0), sigma, d);
        let curved = MovingAt::of_a(lit3(flat.at, &z), sigma, lit3(d, &z));
        assert_eq!(curved, MovingAt::from_flat(&flat, &z), "{sigma:?}");
    }
}

/// `ν·(w − q)` and `r̂·(w − q)`, flat and curved: linear, so the
/// differences are the rows exactly.
#[test]
fn vertex_vertex_shadow_rows_are_the_derivatives() {
    for seed in varde_testing::seeds(16, 256) {
        let mut r = Rng::new(seed);
        let (q, w) = (r.ints(16), r.ints(16));
        let own = r.field();
        let q_on_a = r.a_is_first();
        let axis = [ShadowAxis::Side, ShadowAxis::Along][r.below(2) as usize];
        let pred = VvShadow {
            q: flat_moving(q, q_on_a, &own),
            w: flat_moving(w, !q_on_a, &own),
            axis,
        };
        let len = unit3(&v3(axis.vector()));
        let scale = dot3(&len, &v3(axis.vector()));
        let curved = vv_shadow(
            &MovingAt::from_flat(&pred.q, &big(0.0)),
            &MovingAt::from_flat(&pred.w, &big(0.0)),
            axis,
        );
        let spatial = |qd: &[B; 3], wd: &[B; 3]| dot3(&len, &sub3(wd, qd));
        let g = bigq(&value(&pred)).div(&scale);
        assert!(f(&g.sub(&spatial(&v3(q), &v3(w)))).abs() < 1e-100);
        for k in 1..=ORDERS {
            let row = order_big(&pred, k).div(&scale);
            assert!(f(&row.sub(&curved.ord[k - 1])).abs() < 1e-100);
            let (mq, mw) = (mover(q_on_a, &own, k), mover(!q_on_a, &own, k));
            let moved = |d: &B| spatial(&mq(&v3(q), d), &mw(&v3(w), d));
            agrees(&row, moved, &format!("seed {seed} order {k}"));
        }
    }
}

/// The shadow distance of `q` from a line through `a`, `b`, signed `+` to
/// its left seen from above.
fn line_distance(q: &[B; 3], a: &[B; 3], b: &[B; 3]) -> B {
    let nu = unit3(&cross3(&v3(UP), &sub3(b, a)));
    dot3(&nu, &sub3(q, a))
}

/// `q` on the shadow of a line, the line or `q` moving: the line's ends
/// at their own rates of the affine field, the row reading the line at
/// its rate at the foot.
#[test]
fn line_shadow_rows_are_the_derivatives_at_ties() {
    for seed in varde_testing::seeds(16, 256) {
        let mut r = Rng::new(seed);
        let (a, b) = (r.ints(16), r.ints(16));
        if UP.cross(b - a).length() < 1.0 {
            continue;
        }
        let foot = a + (b - a) * 0.5;
        let q = foot + UP * r.int(2);
        let own = r.field();
        let q_on_a = r.a_is_first();
        let line = if q_on_a {
            Seg::fixed(a, b)
        } else {
            Seg {
                a,
                b,
                rates: Field::rates(&own, foot),
            }
        };
        let pred = LineShadow {
            q: flat_moving(q, q_on_a, &own),
            line,
        };
        assert_eq!(value(&pred), Q::from(0u32));
        let nu = v3(UP.cross(b - a));
        let scale = dot3(&nu, &nu).sqrt();
        for k in 1..=ORDERS {
            let row = order_big(&pred, k).div(&scale);
            let (mq, ml) = (mover(q_on_a, &own, k), mover(!q_on_a, &own, k));
            let moved = |d: &B| line_distance(&mq(&v3(q), d), &ml(&v3(a), d), &ml(&v3(b), d));
            agrees(&row, moved, &format!("seed {seed} order {k}"));
        }
    }
}

/// The plane `n·x = c` through three integer points, `q` on it or
/// straight below or above it: the plane (at its rate at the hit) or `q`
/// moving. The value is the height along the unit `UP`.
#[test]
fn plane_height_rows_are_the_derivatives() {
    for seed in varde_testing::seeds(16, 256) {
        let mut r = Rng::new(seed);
        let (p0, p1, p2) = (r.ints(16), r.ints(16), r.ints(16));
        let (u, v) = (p1 - p0, p2 - p0);
        let n = u.cross(v);
        if n.dot(UP).abs() < 1.0 {
            continue;
        }
        let hit = p0 + u * 0.5 + v * 0.25;
        let q = hit - UP * r.int(2);
        let own = r.field();
        let q_on_a = r.a_is_first();
        let pred = PlaneHeight {
            normal: n,
            offset: n.dot(p0),
            rates: if q_on_a {
                [Rate::ZERO; ORDERS]
            } else {
                Field::rates(&own, hit)
            },
            q: flat_moving(q, q_on_a, &own),
        };
        let e = up();
        let nu = dot3(&v3(n), &v3(UP));
        let scale = nu.mul(&nu).div(&dot3(&v3(UP), &v3(UP)).sqrt());
        // Height of the plane through `p0 + s·u + t·v` above `q`.
        let height = |q: &[B; 3], p: [[B; 3]; 3]| {
            let [p0, p1, p2] = p;
            let rhs = sub3(q, &p0);
            let [_, _, h] = cramer(&sub3(&p1, &p0), &sub3(&p2, &p0), &e, &rhs);
            h.neg()
        };
        let g = bigq(&value(&pred)).div(&scale);
        let h0 = height(&v3(q), [v3(p0), v3(p1), v3(p2)]);
        assert!(f(&g.sub(&h0)).abs() < 1e-100, "seed {seed}");
        for k in 1..=ORDERS {
            let row = order_big(&pred, k).div(&scale);
            let (mq, mf) = (mover(q_on_a, &own, k), mover(!q_on_a, &own, k));
            let moved = |d: &B| {
                height(
                    &mq(&v3(q), d),
                    [mf(&v3(p0), d), mf(&v3(p1), d), mf(&v3(p2), d)],
                )
            };
            agrees(&row, moved, &format!("seed {seed} order {k}"));
        }
    }
}

/// Two segments through one point whose shadows cross there, either
/// moving at its rate at the crossing. The value is the height of `g`
/// above `e` along the unit `UP` where the shadows cross.
#[test]
fn edge_edge_rows_are_the_derivatives_at_ties() {
    for seed in varde_testing::seeds(16, 256) {
        let mut r = Rng::new(seed);
        let x = r.ints(16);
        let (u, v) = (r.ints(4), r.ints(4));
        let m = u.cross(v);
        if m.dot(UP).abs() < 1.0 {
            continue;
        }
        let (a, b, c, d) = (x - u, x + u, x - v, x + v * 3.0);
        let own = r.field();
        let e_on_a = r.a_is_first();
        let seg = |p: DVec3, q: DVec3, on_a: bool| {
            if on_a {
                Seg {
                    a: p,
                    b: q,
                    rates: Field::rates(&own, x),
                }
            } else {
                Seg::fixed(p, q)
            }
        };
        let pred = EdgeEdge {
            e: seg(a, b, e_on_a),
            g: seg(c, d, !e_on_a),
        };
        assert_eq!(value(&pred), Q::from(0u32));
        let mu = dot3(&v3((b - a).cross(d - c)), &v3(UP));
        let scale = mu.mul(&mu).div(&dot3(&v3(UP), &v3(UP)).sqrt());
        let e_up = up();
        let gap = |p: [[B; 3]; 4]| {
            let [a, b, c, d] = p;
            // c + s(d − c) − a − t(b − a) − h·ê = 0.
            let [_, _, h] = cramer(
                &sub3(&d, &c),
                &sub3(&a, &b),
                &e_up.clone().map(|x| x.neg()),
                &sub3(&a, &c),
            );
            h
        };
        for k in 1..=ORDERS {
            let row = order_big(&pred, k).div(&scale);
            let (me, mg) = (mover(e_on_a, &own, k), mover(!e_on_a, &own, k));
            let moved = |dl: &B| {
                gap([
                    me(&v3(a), dl),
                    me(&v3(b), dl),
                    mg(&v3(c), dl),
                    mg(&v3(d), dl),
                ])
            };
            agrees(&row, moved, &format!("seed {seed} order {k}"));
        }
    }
}

/// A rational quadratic span's control points `(x, w)`, at the big-float
/// width.
type Ctrl = Vec<([B; 3], B)>;

fn random_span(r: &mut Rng) -> Ctrl {
    (0..3)
        .map(|_| (v3(r.0.point(8.0)), big(r.range(0.5, 2.0))))
        .collect()
}

/// The span with each control point moved by `mv` (an affine map moves
/// the curve with its control points).
fn span(c: &Ctrl, mv: impl Fn(&[B; 3]) -> [B; 3]) -> HomCurve<B, 3> {
    HomCurve::new(c.iter().map(|(x, w)| (scale3(&mv(x), w), w.clone()))).expect("a span")
}

/// The patch with each control point moved by `mv`.
fn patch(c: &[([B; 3], B)], mv: impl Fn(&[B; 3]) -> [B; 3]) -> HomPatch<B> {
    HomPatch::new(
        2,
        2,
        c.iter().map(|(x, w)| {
            let [a, b, cz] = scale3(&mv(x), w);
            [a, b, cz, w.clone()]
        }),
    )
    .expect("a patch")
}

/// The shadow distance from `q` to the span's foot near `s`: the foot
/// solved again, then `ν·(q − C)` there.
fn foot_distance(c: &HomCurve<B, 3>, q: &[B; 3], s: &B) -> B {
    let s = solve1(s.clone(), |s| {
        let (p, t) = curve_point(c, s);
        let [t0, t1] = shadow(&t);
        let [d0, d1] = shadow(&sub3(q, &p));
        t0.mul(&d0).add(&t1.mul(&d1))
    });
    let (p, t) = curve_point(c, &s);
    let nu = unit3(&cross3(&v3(UP), &t));
    dot3(&nu, &sub3(q, &p))
}

/// The vertex–curve shadow row (and a box side's) on random rational
/// quadratics: `q` beside the curve's shadow or on it, `q` or the curve
/// moving. The distance to the foot moves by the row at any `q`.
#[test]
fn curve_shadow_rows_are_the_derivatives() {
    for seed in varde_testing::seeds(6, 128) {
        let mut r = Rng::new(seed);
        let ctrl = random_span(&mut r);
        let s0 = big(r.range(0.3, 0.7));
        let fixed = span(&ctrl, |x| x.clone());
        let (c, t) = curve_point(&fixed, &s0);
        if f(&dot3(&unit3(&t), &up())).abs() > 0.9 {
            continue;
        }
        let nu = unit3(&cross3(&v3(UP), &t));
        let rho = if r.below(2) == 0 {
            0.0
        } else {
            r.range(-0.2, 0.2)
        };
        let q = add3(
            &add3(&c, &scale3(&nu, &big(rho))),
            &scale3(&up(), &big(r.int(3))),
        );
        let own = r.field();
        let q_on_a = r.a_is_first();
        let rates = |on_a: bool, x: &[B; 3]| {
            if on_a {
                MovingAt {
                    at: x.clone(),
                    rates: Field::rates_big(&own, x),
                }
            } else {
                MovingAt::fixed(x.clone())
            }
        };
        let row = curve_shadow(&rates(q_on_a, &q), &rates(!q_on_a, &c), &t);
        assert!(f(&row.g.sub(&big(rho))).abs() < 1e-100, "seed {seed}");
        for k in 1..=ORDERS {
            let (mq, mc) = (mover(q_on_a, &own, k), mover(!q_on_a, &own, k));
            let moved = |d: &B| foot_distance(&span(&ctrl, |x| mc(x, d)), &mq(&q, d), &s0);
            agrees(&row.ord[k - 1], moved, &format!("seed {seed} order {k}"));
        }
    }
}

/// A random rational biquadratic patch, a graph over the shadow plane:
/// a grid across `UP` with random heights.
fn random_patch(r: &mut Rng) -> Vec<([B; 3], B)> {
    let base = r.0.point(4.0);
    let (a1, a2) = (R_HAT / R_HAT.length(), NU / NU.length());
    let up = UP / UP.length();
    let mut net = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            let x = base + a1 * (2.0 * i as f64) + a2 * (2.0 * j as f64) + up * r.range(-1.0, 1.0);
            net.push((v3(x), big(r.range(0.7, 1.4))));
        }
    }
    net
}

/// The height along the unit `UP` of the patch above `q`, its hit solved
/// again near `(u, v)`.
fn patch_height(face: &HomPatch<B>, q: &[B; 3], uv: &[B; 2]) -> B {
    let [u, v] = solve2(uv.clone(), |x| shadow(&sub3(&face.eval(&x[0], &x[1]), q)));
    dot3(&up(), &sub3(&face.eval(&u, &v), q))
}

/// The layer-height row, the tangent plane of a curved face at the hit,
/// on random rational patches: `q` on the face or below or above it, `q`
/// or the face moving. The height moves by the row over the facing
/// `|n·ê|`, at any `q`.
#[test]
fn tangent_plane_row_rows_are_the_derivatives() {
    for seed in varde_testing::seeds(6, 128) {
        let mut r = Rng::new(seed);
        let net = random_patch(&mut r);
        let fixed = patch(&net, |x| x.clone());
        let uv = [big(r.range(0.3, 0.7)), big(r.range(0.3, 0.7))];
        let (hit, normal) = patch_point(&fixed, &uv[0], &uv[1]);
        let q = sub3(&hit, &scale3(&up(), &big(r.int(2) * 0.5)));
        let own = r.field();
        let q_on_a = r.a_is_first();
        let qm = if q_on_a {
            MovingAt {
                at: q.clone(),
                rates: Field::rates_big(&own, &q),
            }
        } else {
            MovingAt::fixed(q.clone())
        };
        let face_rates = if q_on_a {
            MovingAt::fixed(hit.clone()).rates
        } else {
            Field::rates_big(&own, &hit)
        };
        // Either orientation of the face gives the same row.
        let flipped = normal.clone().map(|x| x.neg());
        let ord = tangent_plane_row(&normal, &face_rates, &qm).expect("a facing");
        assert_eq!(
            tangent_plane_row(&flipped, &face_rates, &qm),
            Some(ord.clone())
        );
        let facing = dot3(&unit3(&normal), &up()).abs();
        for k in 1..=ORDERS {
            let row = ord[k - 1].div(&facing);
            let (mq, mf) = (mover(q_on_a, &own, k), mover(!q_on_a, &own, k));
            let moved = |d: &B| patch_height(&patch(&net, |x| mf(x, d)), &mq(&q, d), &uv);
            agrees(&row, moved, &format!("seed {seed} order {k}"));
        }
    }
}

/// The height along the unit `UP` of `g` above `e` where their shadows
/// cross, the crossing solved again near `(s, t)`.
fn crossing_gap(e: &HomCurve<B, 3>, g: &HomCurve<B, 3>, st: &[B; 2]) -> B {
    let [s, t] = solve2(st.clone(), |x| {
        shadow(&sub3(&curve_point(g, &x[0]).0, &curve_point(e, &x[1]).0))
    });
    dot3(&up(), &sub3(&curve_point(g, &s).0, &curve_point(e, &t).0))
}

/// The edge–edge z-order row on random rational quadratics through one
/// point (a tie), the edge of `A` moving: the gap moves by the row over
/// the facing `m·ê`.
#[test]
fn z_order_rows_are_the_derivatives_at_ties() {
    for seed in varde_testing::seeds(6, 128) {
        let mut r = Rng::new(seed);
        let ctrl_e = random_span(&mut r);
        let ctrl_g0 = random_span(&mut r);
        let (s0, t0) = (big(r.range(0.3, 0.7)), big(r.range(0.3, 0.7)));
        let (x, te) = curve_point(&span(&ctrl_e, |x| x.clone()), &t0);
        let (y, _) = curve_point(&span(&ctrl_g0, |x| x.clone()), &s0);
        // `g` translated through `C_e(t₀)`, exactly at this width.
        let shift = sub3(&x, &y);
        let ctrl_g: Ctrl = ctrl_g0
            .iter()
            .map(|(p, w)| (add3(p, &shift), w.clone()))
            .collect();
        let (xg, tg) = curve_point(&span(&ctrl_g, |x| x.clone()), &s0);
        let m = unit3(&cross3(&te, &tg));
        let facing = dot3(&m, &up()).abs();
        if f(&facing) < 0.1 {
            continue;
        }
        let own = r.field();
        let e = MovingAt {
            at: x.clone(),
            rates: Field::rates_big(&own, &x),
        };
        let g = MovingAt::fixed(xg);
        let row = z_order(&e, &te, &g, &tg).expect("crossing shadows");
        assert!(f(&row.g).abs() < 1e-100, "seed {seed}");
        // The other operand's edge as the moving one gives the row negated.
        let swapped = z_order(&g, &tg, &e, &te).expect("crossing shadows");
        for k in 1..=ORDERS {
            assert!(f(&row.ord[k - 1].add(&swapped.ord[k - 1])).abs() < 1e-100);
            let row_k = row.ord[k - 1].div(&facing);
            let me = mover(true, &own, k);
            let moved = |d: &B| {
                crossing_gap(
                    &span(&ctrl_e, |x| me(x, d)),
                    &span(&ctrl_g, |x| x.clone()),
                    &[s0.clone(), t0.clone()],
                )
            };
            agrees(&row_k, moved, &format!("seed {seed} order {k}"));
        }
    }
}

/// The edge–edge z-order orders over a box: random rational quadratics
/// through one point (a tie), cut to small pieces about it. The cone
/// enclosure of each order holds the row at the crossing, and for small
/// pieces it certifies the row's sign.
#[test]
fn z_order_cones_hold_the_row_over_a_box() {
    let mut certified = 0;
    for seed in varde_testing::seeds(8, 128) {
        let mut r = Rng::new(seed);
        let ctrl_e = random_span(&mut r);
        let ctrl_g0 = random_span(&mut r);
        let (s0, t0) = (r.range(0.3, 0.7), r.range(0.3, 0.7));
        let (x, te) = curve_point(&span(&ctrl_e, |x| x.clone()), &big(t0));
        let (y, _) = curve_point(&span(&ctrl_g0, |x| x.clone()), &big(s0));
        let shift = sub3(&x, &y);
        let ctrl_g: Ctrl = ctrl_g0
            .iter()
            .map(|(p, w)| (add3(p, &shift), w.clone()))
            .collect();
        let (xg, tg) = curve_point(&span(&ctrl_g, |x| x.clone()), &big(s0));
        if f(&dot3(&unit3(&cross3(&te, &tg)), &up())).abs() < 0.1 {
            continue;
        }
        let own = r.field();
        let e = MovingAt {
            at: x.clone(),
            rates: Field::rates_big(&own, &x),
        };
        let g = MovingAt::fixed(xg);
        let row = z_order(&e, &te, &g, &tg).expect("crossing shadows");
        let deltas: [[B; 3]; ORDERS] = std::array::from_fn(|k| sub3(&g.rates[k], &e.rates[k]));
        for h in [0.25, 1.0 / 64.0, 1.0 / 4096.0] {
            let piece = |c: &Ctrl, at: f64| {
                let full = span(c, |x| x.clone());
                let [_, right] = full.split_at(&big(at - h));
                let [left, _] = right.split_at(&big(2.0 * h).div(&big(1.0 - (at - h))));
                let w = left.weights().coefs().to_vec();
                let pts: Vec<[B; 3]> = (0..w.len())
                    .map(|i| [0, 1, 2].map(|k| left.coords()[k].coefs()[i].div(&w[i])))
                    .collect();
                let mut out = Vec::new();
                for i in 0..pts.len() {
                    for j in i + 1..pts.len() {
                        out.push(sub3(&pts[j], &pts[i]));
                    }
                }
                out
            };
            let Some(cone) = z_order_cone(&piece(&ctrl_e, t0), &piece(&ctrl_g, s0), &deltas) else {
                continue;
            };
            for k in 0..ORDERS {
                let v = f(&row.ord[k]);
                let [lo, hi] = cone[k];
                assert!(
                    lo <= v && v <= hi,
                    "seed {seed} h {h} order {k}: {v} not in {:?}",
                    cone[k]
                );
                if h < 0.001 && (lo > 0.0 || hi < 0.0) {
                    certified += 1;
                }
            }
        }
    }
    assert!(certified > 0);
}

/// The point–curve z-order orders, exactly, at a point `q` lying on a
/// curve with the tangent `tangent` there (any non-zero multiple): the
/// signs of the derivatives of the curve's height above `q` along each
/// motion, `ê·(ΔC − Δq) + (ê·T)·ṡ` with `ṡ = T_xy·(Δq − ΔC)_xy/‖T_xy‖²`
/// (the foot sliding along the curve to stay under the moved `q`; §
/// "Rows" in [`crate::records::height`]), `deltas[k] = Δq − ΔC` along
/// motion `k`. Times `‖T_xy‖²·|ê|`-like positive factors that is
/// `(u·T)(T·Δ) − (u·Δ)(T·T)`, `u` = [`UP`], which these are the exact
/// signs of: the curved row [`shared_zorder`]'s numerator, worked out
/// exactly in rationals as the test's reference. All `Equal` where `T`
/// is along `UP` (no foot to slide).
fn point_curve_signs(
    tangent: &[Rational; 3],
    deltas: &[[Rational; 3]; ORDERS],
) -> [ExactSign; ORDERS] {
    let u = [UP.x, UP.y, UP.z].map(rat);
    let d3 = |a: &[Rational; 3], b: &[Rational; 3]| -> Rational {
        &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
    };
    let (a, tt) = (d3(&u, tangent), d3(tangent, tangent));
    let vertical = &tt * d3(&u, &u) == &a * &a;
    deltas.each_ref().map(|d| {
        if vertical {
            return Ok(Ordering::Equal);
        }
        let v = &a * d3(tangent, d) - d3(&u, d) * &tt;
        Ok(v.cmp(&Rational::from(0u32)))
    })
}

/// The point–curve z-order row's signs at a point on a curve (a tie),
/// the point or the curve moving: the curve's height above the moved
/// point, its foot solved again, moves with those signs.
#[test]
fn point_curve_signs_are_the_derivatives_at_ties() {
    let mut checked = 0;
    for seed in varde_testing::seeds(8, 128) {
        let mut r = Rng::new(seed);
        let ctrl = random_span(&mut r);
        let on_a = r.a_is_first();
        let own = r.field();
        // `q` at the span's start control point, the tangent there the
        // chord to the middle one.
        let q0 = ctrl[0].0.clone();
        let tangent: [Q; 3] = std::array::from_fn(|i| {
            Q::try_from(f(&ctrl[1].0[i])).expect("finite") - Q::try_from(f(&q0[i])).expect("finite")
        });
        let at = DVec3::new(f(&q0[0]), f(&q0[1]), f(&q0[2]));
        // `q`'s rates less the curve's there.
        let rates = Field::rates(&own, at);
        let deltas: [[Q; 3]; ORDERS] = std::array::from_fn(|k| {
            let d = rate_rational(&rates[k]);
            if on_a { d } else { d.map(|x| -x) }
        });
        let signs = point_curve_signs(&tangent, &deltas);
        for k in 1..=ORDERS {
            let me = mover(true, &own, k);
            let height = |d: &B| {
                let (c, q) = if on_a {
                    (span(&ctrl, |x| x.clone()), me(&q0, d))
                } else {
                    (span(&ctrl, |x| me(x, d)), q0.clone())
                };
                let s = solve1(big(0.0), |s| {
                    let (p, t) = curve_point(&c, s);
                    let [t0, t1] = shadow(&t);
                    let [d0, d1] = shadow(&sub3(&q, &p));
                    t0.mul(&d0).add(&t1.mul(&d1))
                });
                dot3(&up(), &sub3(&curve_point(&c, &s).0, &q))
            };
            let d = big(p2(-30));
            let fd = f(&height(&d).sub(&height(&d.neg())).div(&d.add(&d)));
            if fd.abs() < 1e-6 {
                continue;
            }
            let want = if fd > 0.0 {
                Ordering::Greater
            } else {
                Ordering::Less
            };
            assert_eq!(signs[k - 1], Ok(want), "seed {seed} order {k}: {fd}");
            checked += 1;
        }
    }
    assert!(checked > 8, "{checked} checked");
}

fn key(row: Row) -> TieKey {
    TieKey {
        row,
        record: RecordKey::Vv(VvKey {
            a: Query::Vert(VertId(1)),
            b: Query::Vert(VertId(2)),
        }),
    }
}

/// Flat records through `decide`: a vertex of `A` on a line of `B`'s
/// shadow is decided by its own direction across the line, or by `T₂`
/// when that runs along the line; a height or z-order record's orders
/// are read only at a tie of its key's value.
#[test]
fn flat_rows_decide_ties_by_the_first_motion_that_moves_them() {
    let line = Seg::fixed(DVec3::new(0.0, 0.0, 0.0), DVec3::new(4.0, 0.0, 0.0));
    // On the line's shadow, straight above its middle.
    let at = DVec3::new(2.0, 0.0, 0.0) + UP;
    let k = key(Row::VertexCurveShadow);
    let across = LineShadow {
        q: Moving::of_a(at, Sign::Pos, DVec3::new(0.0, 1.0, 0.0)),
        line: line.clone(),
    };
    let left = UP.cross(DVec3::X).dot(DVec3::Y) > 0.0;
    let want = if left { Sign::Pos } else { Sign::Neg };
    assert_eq!(decide_flat(&across, k).0, Ok(want));
    let flipped = LineShadow {
        q: Moving::of_a(at, Sign::Neg, DVec3::new(0.0, 1.0, 0.0)),
        line: line.clone(),
    };
    assert_ne!(decide_flat(&flipped, k).0, Ok(want));
    // Along the line (and along `UP`): `T₂` decides.
    for d in [DVec3::X, UP, DVec3::ZERO] {
        let along = LineShadow {
            q: Moving::of_a(at, Sign::Pos, d),
            line: line.clone(),
        };
        let t2 = UP.cross(DVec3::X).dot(T[0]);
        let want = if t2 > 0.0 { Sign::Pos } else { Sign::Neg };
        let (s, work) = decide_flat(&along, k);
        assert_eq!(s, Ok(want), "{d}");
        assert_eq!(work.filters(), 3, "the value, then motions 1 and 2");
    }
    // A height record: the key's sign decides, its orders unread.
    let plane = PlaneHeight {
        normal: DVec3::Z,
        offset: 1.0,
        rates: rates_a(Sign::Pos, DVec3::Z),
        q: Moving::fixed(DVec3::new(0.0, 0.0, 1.0)),
    };
    let k = key(Row::ColumnHit);
    let (s, work) = decide_flat_orders(Ordering::Less, &plane, k);
    assert_eq!((s, work.filters()), (Ok(Sign::Neg), 0));
    // At a tie the face of `A` moving up along its normal puts it above.
    let (s, work) = decide_flat_orders(Ordering::Equal, &plane, k);
    assert_eq!((s, work.filters()), (Ok(Sign::Pos), 1));
    // On a vertical plane every order is zero: the guard.
    let vertical = PlaneHeight {
        normal: R_HAT,
        offset: 0.0,
        rates: rates_a(Sign::Pos, R_HAT),
        q: Moving::fixed(DVec3::ZERO),
    };
    assert_eq!(
        decide_flat_orders(Ordering::Equal, &vertical, k).0,
        Err(Fail::Internal { key: k })
    );
    // Parallel shadows have no z-order: the guard.
    let e = Seg::of_a(DVec3::ZERO, DVec3::X, Sign::Pos, DVec3::Z);
    let parallel = EdgeEdge {
        e: e.clone(),
        g: Seg::fixed(DVec3::Y, DVec3::new(3.0, 1.0, 0.0)),
    };
    let k = key(Row::ZOrder);
    assert_eq!(
        decide_flat_orders(Ordering::Equal, &parallel, k).0,
        Err(Fail::Internal { key: k })
    );
    // Crossing at a tie: `e` of `A` moving up passes above `g`.
    let crossing = EdgeEdge {
        e,
        g: Seg::fixed(DVec3::new(0.5, -1.0, 0.0), DVec3::new(0.5, 1.0, 0.0)),
    };
    assert_eq!(
        decide_flat_orders(Ordering::Equal, &crossing, k).0,
        Ok(Sign::Neg)
    );
}

/// The point–face height key's value on random rational patches: `q` at
/// height `h` below the hit along the unit `UP` reads `h·|n̂·ê|`, positive
/// with the face above whichever way it faces (the patch flipped gives the
/// same), and the flat row on the tangent plane has the same sign.
#[test]
fn face_height_is_the_height_above_whichever_way_the_face_faces() {
    for seed in varde_testing::seeds(6, 64) {
        let mut r = Rng::new(seed);
        let net = random_patch(&mut r);
        let face = patch(&net, |x| x.clone());
        let uv = [big(r.range(0.3, 0.7)), big(r.range(0.3, 0.7))];
        let (hit, normal) = patch_point(&face, &uv[0], &uv[1]);
        let h = big(r.int(3) * 0.25);
        let q = sub3(&hit, &scale3(&up(), &h));
        let got = face_height(&normal, &hit, &q).expect("a facing");
        let flipped = normal.clone().map(|x| x.neg());
        assert_eq!(face_height(&flipped, &hit, &q), Some(got.clone()));
        let facing = dot3(&unit3(&normal), &up()).abs();
        let want = h.mul(&facing);
        let diff = f(&got.sub(&want)).abs();
        assert!(
            diff <= 1e-60 * (1.0 + f(&want).abs()),
            "seed {seed}: {diff:e}"
        );
        // The flat row on the tangent plane at the hit, rounded to `f64`:
        // a positive multiple of the same value, so of its sign.
        let fl = |p: &[B; 3]| DVec3::new(f(&p[0]), f(&p[1]), f(&p[2]));
        let (n, at, qf) = (fl(&normal), fl(&hit), fl(&hit) - UP * (r.int(3) * 0.25));
        let flat = PlaneHeight {
            normal: n,
            offset: n.dot(at),
            rates: [Rate::ZERO; ORDERS],
            q: Moving::fixed(qf),
        };
        let curved = face_height(&v3(n), &v3(at), &v3(qf)).expect("a facing");
        let offset_exact = Q::try_from(n.dot(at)).unwrap() == q3dot(n, at);
        if offset_exact && let Some(s) = curved.sign() {
            assert_eq!(value(&flat).cmp(&Q::from(0u32)), s, "seed {seed}");
        }
    }
}

/// `n·a` exactly.
fn q3dot(n: DVec3, a: DVec3) -> Q {
    let q = |x: f64| Q::try_from(x).unwrap();
    q(n.x) * q(a.x) + q(n.y) * q(a.y) + q(n.z) * q(a.z)
}

/// A curved row has no answer where its orientation isn't certified: a
/// face seen edge-on, shadows parallel.
#[test]
fn curved_rows_refuse_an_uncertified_orientation() {
    let z = Bounded::exact(0.0);
    let at = MovingAt::fixed([z; 3]);
    let edge_on = lit3(UP.cross(DVec3::X), &z);
    let rates = MovingAt::fixed([z; 3]).rates;
    assert_eq!(tangent_plane_row(&edge_on, &rates, &at), None);
    assert_eq!(face_height(&edge_on, &at.at, &at.at), None);
    let t = lit3(DVec3::X, &z);
    assert_eq!(z_order(&at, &t, &at, &t), None);
    // A tangent along `UP` has no shadow normal: uncertified, never a sign.
    let q = MovingAt::of_a([z; 3], Sign::Pos, lit3(DVec3::X, &z));
    let row = curve_shadow(&q, &at, &lit3(UP, &z));
    assert!(row.ord.iter().all(|x| x.sign().is_none()));
}

/// Curved rows at every level hold the big-float value within their
/// certified errors, on random curves and patches.
#[test]
fn curved_rows_certify_their_errors_at_every_level() {
    fn within<S: Curved>(x: &S, b: &B) {
        let [lo, hi] = x.enclose();
        let v = f(b);
        assert!(lo <= v && v <= hi, "{v:e} outside [{lo:e}, {hi:e}]");
    }
    fn rows<S: Curved>(
        c: &[([f64; 3], f64)],
        net: &[([f64; 3], f64)],
        q: DVec3,
        d: DVec3,
        like: &S,
    ) -> Vec<S> {
        let lit = |x: f64| S::from_f64(x, like);
        let s = lit(0.375);
        let curve =
            HomCurve::new(c.iter().map(|(x, w)| (x.map(|v| lit(v * w)), lit(*w)))).expect("a span");
        let surface = HomPatch::new(
            2,
            2,
            net.iter()
                .map(|(x, w)| [lit(x[0] * w), lit(x[1] * w), lit(x[2] * w), lit(*w)]),
        )
        .expect("a patch");
        let (p, t) = curve_point(&curve, &s);
        let qa = MovingAt::of_a(lit3(q, like), Sign::Pos, lit3(d, like));
        let foot = MovingAt::fixed(p.clone());
        let mut out = Vec::new();
        let cs = curve_shadow(&qa, &foot, &t);
        out.push(cs.g);
        out.extend(cs.ord);
        let (_, n) = patch_point(&surface, &lit(0.625), &s);
        if let Some(ord) = tangent_plane_row(&n, &MovingAt::fixed(p.clone()).rates, &qa) {
            out.extend(ord);
        }
        let mut t2 = t.clone();
        t2.swap(0, 1);
        if let Some(z) = z_order(&qa, &t2, &foot, &t) {
            out.push(z.g);
            out.extend(z.ord);
        }
        out.extend(vv_shadow(&qa, &foot, ShadowAxis::Side).ord);
        out
    }
    for seed in varde_testing::seeds(16, 256) {
        let mut r = Rng::new(seed);
        // Dyadic data, so the levels read the same inputs.
        let mut dy = |e: f64| (r.range(-e, e) * 1024.0).round() / 1024.0;
        let c: Vec<_> = (0..3)
            .map(|_| ([dy(8.0), dy(8.0), dy(8.0)], 1.0 + dy(0.5).abs()))
            .collect();
        let net: Vec<_> = (0..9)
            .map(|i| {
                let (u, v) = ((i / 3) as f64, (i % 3) as f64);
                (
                    [2.0 * u + dy(0.5), 2.0 * v + dy(0.5), dy(2.0)],
                    1.0 + dy(0.3).abs(),
                )
            })
            .collect();
        let q = DVec3::new(dy(8.0), dy(8.0), dy(8.0));
        let d = DVec3::new(dy(1.0), dy(1.0), dy(1.0));
        let want = rows(&c, &net, q, d, &big(0.0));
        let first = rows(&c, &net, q, d, &Bounded::exact(0.0));
        let second = rows(&c, &net, q, d, &Bounded::exact(Dd::from(0.0)));
        assert_eq!((first.len(), second.len()), (want.len(), want.len()));
        for ((a, b), w) in first.iter().zip(&second).zip(&want) {
            within(a, w);
            within(b, w);
        }
    }
}

/// Exact rationals of a point.
fn qv(p: DVec3) -> [Q; 3] {
    [p.x, p.y, p.z].map(|x| Q::try_from(x).expect("finite"))
}

fn qdot(a: &[Q; 3], b: &[Q; 3]) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

/// Shadow-plane coordinates, along `R_HAT` and `NU`: each a positive
/// multiple of a unit coordinate, so sides and crossings are kept.
fn qshadow(p: &[Q; 3]) -> [Q; 2] {
    [qdot(&qv(R_HAT), p), qdot(&qv(NU), p)]
}

/// `p` moved by motion `k` at `δ = 1`, unmoved for `k = 0`.
fn qat(p: DVec3, rates: &[DVec3; ORDERS], k: usize) -> [Q; 3] {
    let p = qv(p);
    if k == 0 {
        return p;
    }
    let r = qv(rates[k - 1]);
    [0, 1, 2].map(|i| &p[i] + &r[i])
}

/// Rates for a flat test: `B`'s, `A`'s with a direction (often one that
/// leaves the record unmoved, or zero), or any rates with zeros.
fn tie_rates(r: &mut Rng, along: &[DVec3], s: f64) -> [DVec3; ORDERS] {
    let pick = |r: &mut Rng| along[r.below(along.len() as u64) as usize];
    match r.below(4) {
        0 => [DVec3::ZERO; ORDERS],
        1 => {
            let sigma = if r.a_is_first() { Sign::Pos } else { Sign::Neg };
            let d = match r.below(3) {
                0 => DVec3::ZERO,
                1 => pick(r),
                _ => r.ints(4) / 4.0,
            };
            let own = if sigma == Sign::Pos { d } else { -d };
            [own, T[0], T[1], T[2]]
        }
        _ => std::array::from_fn(|_| match r.below(3) {
            0 => DVec3::ZERO,
            1 => pick(r),
            _ => r.ints(4) * s,
        }),
    }
}

/// A flat row against its spatial value `h(k)` worked out independently
/// in rationals with motion `k` applied at `δ = 1` (`k = 0` unmoved).
/// Every row here is affine in translations, so `h(k) − h(0)` is the
/// derivative exactly. The value and orders are one positive multiple of
/// the spatial ones, and `decide` gives the first non-zero sign, or the
/// guard. Without a spatial value (`None`: a zero-length or vertical
/// line, a vertical plane, parallel shadows) every order is zero and the
/// guard is reached, never a sign.
fn agrees_exactly<P: Pred + std::fmt::Debug>(pred: &P, h: Option<&dyn Fn(usize) -> Q>, row: Row) {
    let rows = [
        value(pred),
        order_value::<P, 1>(pred),
        order_value::<P, 2>(pred),
        order_value::<P, 3>(pred),
        order_value::<P, 4>(pred),
    ];
    let got = decide_flat(pred, key(row)).0;
    let guard = Err(Fail::Internal { key: key(row) });
    let Some(h) = h else {
        assert!(rows.iter().all(|x| *x == 0u32), "{pred:?}");
        assert_eq!(got, guard, "{pred:?}");
        return;
    };
    let h0 = h(0);
    let spatial: [Q; 5] = std::array::from_fn(|k| if k == 0 { h0.clone() } else { h(k) - &h0 });
    let mut scale: Option<Q> = None;
    for (k, (x, want)) in rows.iter().zip(&spatial).enumerate() {
        assert_eq!(*x == 0u32, *want == 0u32, "order {k}: {pred:?}");
        if *want != 0u32 {
            let ratio = x / want;
            assert!(ratio > 0u32, "order {k}: {pred:?}");
            assert!(
                scale.as_ref().is_none_or(|s| *s == ratio),
                "order {k}: {pred:?}"
            );
            scale = Some(ratio);
        }
    }
    let want = spatial.iter().find(|x| **x != 0u32);
    let want = want.map_or(guard, |x| Ok(if *x > 0u32 { Sign::Pos } else { Sign::Neg }));
    assert_eq!(got, want, "{pred:?}");
}

/// Every flat row against exact spatial values (shadow sides in shadow
/// coordinates, heights along `UP`, the gap where two shadows cross):
/// exact ties at the value and at orders 1 to 3 (rates along the line or
/// `UP`, or zero, the saddle's `d = 0`), points on the line, zero-length
/// and vertical lines, vertical planes, parallel shadows, `σ_A = ±1`,
/// tiny coordinates (the rational stage) and large ones.
#[test]
fn flat_rows_are_the_spatial_values_exactly() {
    for seed in varde_testing::seeds(64, 4096) {
        let mut r = Rng::new(seed);
        let s = [1.0, p2(-40), p2(-1000), p2(12)][r.below(4) as usize];
        // Vertex–vertex: `w` straight above `q`, or beside it.
        let q = r.ints(6) * s;
        let w = match r.below(3) {
            0 => q + UP * r.int(3) * s,
            1 => q + (UP * r.int(3) + R_HAT * r.int(2) + NU * r.int(2) / 16.0) * s,
            _ => r.ints(6) * s,
        };
        let along = [UP * s, R_HAT * s, NU * s / 16.0];
        let (qr, wr) = (tie_rates(&mut r, &along, s), tie_rates(&mut r, &along, s));
        for axis in [ShadowAxis::Side, ShadowAxis::Along] {
            let pred = VvShadow {
                q: Moving {
                    at: q,
                    rates: Rate::floats(qr),
                },
                w: Moving {
                    at: w,
                    rates: Rate::floats(wr),
                },
                axis,
            };
            let i = usize::from(axis == ShadowAxis::Side);
            let h = |k: usize| {
                let (a, b) = (qshadow(&qat(w, &wr, k)), qshadow(&qat(q, &qr, k)));
                &a[i] - &b[i]
            };
            agrees_exactly(&pred, Some(&h), Row::VertexVertexShadow);
        }
        // A line, `q` on its shadow or not.
        let a = r.ints(6) * s;
        let dir = match r.below(5) {
            0 => DVec3::ZERO,
            1 => UP * r.int(2) * s,
            _ => r.ints(5) * s,
        };
        let q = if r.a_is_first() {
            a + dir * r.int(4) / 2.0 + UP * r.int(3) * s
        } else {
            r.ints(6) * s
        };
        let along = [dir, UP * s, dir + UP * s];
        let (qr, lr) = (tie_rates(&mut r, &along, s), tie_rates(&mut r, &along, s));
        let pred = LineShadow {
            q: Moving {
                at: q,
                rates: Rate::floats(qr),
            },
            line: Seg {
                a,
                b: a + dir,
                rates: Rate::floats(lr),
            },
        };
        let h = |k: usize| {
            let [a, b, q] =
                [(a, &lr), (a + dir, &lr), (q, &qr)].map(|(p, r)| qshadow(&qat(p, r, k)));
            (&b[0] - &a[0]) * (&q[1] - &a[1]) - (&b[1] - &a[1]) * (&q[0] - &a[0])
        };
        let flat = qshadow(&qv(dir)) == [Q::from(0u32), Q::from(0u32)];
        agrees_exactly(
            &pred,
            (!flat).then_some(&h as &dyn Fn(usize) -> Q),
            Row::VertexCurveShadow,
        );
        // A plane through `p`, `q` on it, above or below it, or not.
        let n = match r.below(4) {
            0 => R_HAT,
            1 => UP.cross(r.ints(3) + DVec3::X),
            _ => r.ints(3) + DVec3::new(0.0, 0.0, 4.0),
        };
        let p = r.ints(6) * s;
        let q = match r.below(3) {
            0 => p + UP * r.int(2) * s,
            1 => p + n.cross(r.ints(2)) * s,
            _ => r.ints(6) * s,
        };
        let along = [n, -n, UP, n.cross(DVec3::X)];
        let (fr, qr) = (tie_rates(&mut r, &along, s), tie_rates(&mut r, &along, s));
        let pred = PlaneHeight {
            normal: n,
            offset: n.dot(p),
            rates: Rate::floats(fr),
            q: Moving {
                at: q,
                rates: Rate::floats(qr),
            },
        };
        let (nq, pq) = (qv(n), qv(p));
        let facing = qdot(&nq, &qv(UP));
        let h = |k: usize| {
            let f = if k == 0 { pq.clone() } else { qat(p, &fr, k) };
            (qdot(&nq, &f) - qdot(&nq, &qat(q, &qr, k))) / &facing
        };
        let vertical = facing == 0u32;
        agrees_exactly(
            &pred,
            (!vertical).then_some(&h as &dyn Fn(usize) -> Q),
            Row::ColumnHit,
        );
        // Two edges, `g` straight above or below a point of `e`, or not.
        let e0 = r.ints(6) * s;
        let ed = match r.below(5) {
            0 => DVec3::ZERO,
            1 => UP * s,
            _ => r.ints(4) * s,
        };
        let gd = match r.below(6) {
            0 => ed * 2.0,
            1 => ed + UP * s,
            2 => UP * s,
            _ => r.ints(4) * s,
        };
        let g0 = if r.a_is_first() {
            e0 + ed * r.int(2) / 2.0 + UP * r.int(2) * s - gd * r.int(2) / 2.0
        } else {
            r.ints(6) * s
        };
        let along = [ed, gd, UP * s, ed - gd];
        let (er, gr) = (tie_rates(&mut r, &along, s), tie_rates(&mut r, &along, s));
        let pred = EdgeEdge {
            e: Seg {
                a: e0,
                b: e0 + ed,
                rates: Rate::floats(er),
            },
            g: Seg {
                a: g0,
                b: g0 + gd,
                rates: Rate::floats(gr),
            },
        };
        let [ed, gd] = [(e0, ed), (g0, gd)].map(|(a, d)| {
            let (a, b) = (qv(a), qv(a + d));
            [0, 1, 2].map(|i| &b[i] - &a[i])
        });
        let (es, gs) = (qshadow(&ed), qshadow(&gd));
        let det = &gs[0] * &es[1] - &es[0] * &gs[1];
        // `e0 + s·ed` and `g0 + t·gd` on one vertical: the gap along `UP`.
        let h = |k: usize| {
            let (ea, ga) = (qat(e0, &er, k), qat(g0, &gr, k));
            let d = qshadow(&[0, 1, 2].map(|i| &ga[i] - &ea[i]));
            let s = (&gs[0] * &d[1] - &gs[1] * &d[0]) / &det;
            let t = (&es[0] * &d[1] - &es[1] * &d[0]) / &det;
            let gap = [0, 1, 2].map(|i| &ga[i] + &t * &gd[i] - &ea[i] - &s * &ed[i]);
            assert_eq!(qshadow(&gap), [Q::from(0u32), Q::from(0u32)]);
            qdot(&qv(UP), &gap)
        };
        let parallel = det == 0u32;
        agrees_exactly(
            &pred,
            (!parallel).then_some(&h as &dyn Fn(usize) -> Q),
            Row::ZOrder,
        );
    }
}

/// Curved rows on near-degenerate data at every level: a curve whose
/// tangent is within `ε` of `UP` (no shadow normal), a face within `ε` of
/// edge-on, tangents within `ε` of each other (near-parallel shadows), at
/// tiny and large scales, weights `1/64` to `64`, the foot at a curve's
/// end. Each level's enclosure meets the widest level's (the true value
/// is in both), a refused orientation shows as no value, and no value is
/// certified without a finite enclosure.
#[test]
fn curved_rows_stay_sound_near_degenerate_data() {
    fn rows<S: Curved>(
        c: &[(DVec3, f64)],
        net: &[(DVec3, f64)],
        at: (f64, [f64; 2]),
        q: &[DVec3; 3],
        like: &S,
    ) -> Vec<Option<S>> {
        let lit = |x: f64| S::from_f64(x, like);
        let curve =
            HomCurve::new(c.iter().map(|(x, w)| (lit3(*x * *w, like), lit(*w)))).expect("a span");
        let surface = HomPatch::new(
            2,
            2,
            net.iter()
                .map(|(x, w)| [x.x, x.y, x.z, 1.0].map(|v| lit(v * w))),
        )
        .expect("a patch");
        let (p, t) = curve_point(&curve, &lit(at.0));
        let [q, d, nudge] = q;
        let sigma = if d.x > 0.0 { Sign::Pos } else { Sign::Neg };
        let qa = MovingAt::of_a(lit3(*q, like), sigma, lit3(*d, like));
        let foot = MovingAt::of_a(p, Sign::Pos, lit3(*d * 0.5, like));
        let all = |c: Coeffs<S>| {
            std::iter::once(c.g)
                .chain(c.ord)
                .map(Some)
                .collect::<Vec<_>>()
        };
        let mut out = all(curve_shadow(&qa, &foot, &t));
        let (_, n) = patch_point(&surface, &lit(at.1[0]), &lit(at.1[1]));
        let height = tangent_plane_row(&n, &foot.rates, &qa);
        out.extend(height.map_or(vec![None; ORDERS], |o| o.map(Some).to_vec()));
        let tg = [0, 1, 2].map(|k| t[k].add(&lit(nudge[k])));
        let z = z_order(&qa, &tg, &foot, &t);
        out.extend(z.map_or(vec![None; ORDERS + 1], all));
        out.extend(all(vv_shadow(&qa, &foot, ShadowAxis::Side)));
        out.extend(all(vv_shadow(&qa, &foot, ShadowAxis::Along)));
        out
    }
    fn meets<S: Curved>(x: &Option<S>, want: &Option<B>, what: &str) {
        let (Some(x), Some(want)) = (x, want) else {
            return;
        };
        let [lo, hi] = x.enclose();
        assert!(!lo.is_nan() && !hi.is_nan(), "{what}: a NaN enclosure");
        if x.sign().is_some() {
            assert!(
                lo.is_finite() && hi.is_finite(),
                "{what}: a sign without a bound"
            );
        }
        let [wl, wh] = want.enclose();
        if wl.is_finite() && wh.is_finite() {
            assert!(
                lo <= wh && wl <= hi,
                "{what}: [{lo:e}, {hi:e}] misses [{wl:e}, {wh:e}]"
            );
        }
    }
    for seed in varde_testing::seeds(48, 2048) {
        let mut r = Rng::new(seed);
        let scale = [1.0, p2(18), p2(-20), p2(-600)][r.below(4) as usize];
        let eps = [0.0, p2(-10), p2(-30), p2(-52), p2(-70)][r.below(5) as usize];
        let weights = [1.0 / 64.0, 1.0, 64.0, 0.5];
        let weight = |r: &mut Rng| weights[r.below(4) as usize];
        let base = DVec3::new(1.0, -2.0, 3.0);
        let steep = r.a_is_first();
        let c: Vec<_> = (0..3)
            .map(|i| {
                let p = if steep {
                    base + UP * f64::from(i) + r.eighths() * eps
                } else {
                    base + r.ints(4)
                };
                (p * scale, weight(&mut r))
            })
            .collect();
        let edge_on = r.a_is_first();
        let net: Vec<_> = (0..9)
            .map(|i| {
                let (u, v) = (f64::from(i / 3), f64::from(i % 3));
                let p = if edge_on {
                    R_HAT * u + UP * v + r.eighths() * eps
                } else {
                    DVec3::new(2.0 * u, 2.0 * v, 0.0) + r.eighths() / 2.0
                };
                (p * scale, weight(&mut r))
            })
            .collect();
        let s = [0.0, 1.0, 0.375][r.below(3) as usize];
        let uv = [[0.0, 0.25], [1.0, 1.0], [0.625, 0.0]][r.below(3) as usize];
        // On the curve's end, straight above it, or anywhere.
        let q = match r.below(3) {
            0 => c[0].0,
            1 => c[0].0 + UP * scale * r.int(3),
            _ => (base + r.eighths() * 8.0) * scale,
        };
        let q = [q, r.eighths(), r.eighths() * eps * scale];
        let want = rows(
            &c,
            &net,
            (s, uv),
            &q,
            &Bounded::exact(Big::from_f64(0.0, Width::MAX)),
        );
        let first = rows(&c, &net, (s, uv), &q, &Bounded::exact(0.0));
        let second = rows(&c, &net, (s, uv), &q, &Bounded::exact(Dd::from(0.0)));
        let third = rows(&c, &net, (s, uv), &q, &big(0.0));
        for (i, w) in want.iter().enumerate() {
            let what = format!("seed {seed}, value {i}");
            meets(&first[i], w, &what);
            meets(&second[i], w, &what);
            meets(&third[i], w, &what);
        }
    }
}

/// The face key's value where the facing has no certified sign: a zero
/// normal (a degenerate point of the patch), an edge-on normal held with
/// `−0` components, and one tilted below what the level certifies give no
/// value, never a height of either sign.
#[test]
fn face_height_refuses_degenerate_and_edge_on_normals() {
    let z = Bounded::exact(0.0);
    let q = lit3(DVec3::ZERO, &z);
    let hit = lit3(UP, &z);
    assert_eq!(face_height(&[z; 3], &hit, &q), None);
    let nz = Bounded::exact(-0.0);
    let edge_on = lit3(UP.cross(DVec3::X), &z).map(|x| {
        if x.sign() == Some(Ordering::Equal) {
            nz
        } else {
            x
        }
    });
    assert_eq!(face_height(&edge_on, &hit, &q), None);
    let zb = big(0.0);
    assert_eq!(
        face_height(
            &[zb.clone(), zb.clone(), zb.clone()],
            &v3(UP),
            &v3(DVec3::ZERO)
        ),
        None
    );
}

/// The shared-curve crossing row: `ê·(T × Δₖ)` with unit `ê` and `T`,
/// the moved shadow's offset across the curve's shadow per unit motion
/// (a length): `|ν|` for a tangent along `r̂` and a motion along `ν`, of
/// either sign with either direction, 0 for a motion along the tangent
/// or along `ê`, whatever the tangent's length; order 0 is 0.
#[test]
fn the_shared_curve_row_is_the_motion_across_the_shadow() {
    let like = Bounded::exact(0.0);
    let v = |d: DVec3| lit3(d, &like);
    let zero = DVec3::ZERO;
    let row = |t: DVec3, d: DVec3| shared_crossing(&v(t), &[v(d), v(T[0]), v(T[1]), v(T[2])]);
    let r = row(R_HAT * 3.0, NU);
    assert_eq!(r.g.enclose(), [0.0, 0.0]);
    let [lo, hi] = r.ord[0].enclose();
    assert!(
        lo <= NU.length() && NU.length() <= hi && hi - lo < 1e-9,
        "{lo} {hi}"
    );
    assert_eq!(row(-R_HAT, NU).ord[0].sign(), Some(Ordering::Less));
    assert_eq!(row(R_HAT, -NU).ord[0].sign(), Some(Ordering::Less));
    for d in [R_HAT * 5.0, UP, zero] {
        let [lo, hi] = row(R_HAT, d).ord[0].enclose();
        assert!(lo <= 0.0 && 0.0 <= hi && hi - lo < 1e-9, "{d}: {lo} {hi}");
    }
    // Orders 2 to 4 are the translations' own, `ê·(T × Tₖ)`.
    let t = DVec3::new(1.0, -2.0, 0.5);
    let r = row(t, zero);
    for k in 0..3 {
        let want = UP.normalize().dot(t.normalize().cross(T[k]));
        let [lo, hi] = r.ord[k + 1].enclose();
        assert!(
            lo <= want + 1e-12 && want - 1e-12 <= hi,
            "{k}: {want} {lo} {hi}"
        );
    }
    // A tangent that isn't certainly non-zero escalates.
    assert_eq!(row(zero, NU).ord[0].sign(), None);
}

/// `ê·(T × d)` in rationals, the reference for [`SharedCrossing`].
fn crossing_rational(t: &[Rational; 3], d: &[Rational; 3]) -> Ordering {
    let u = [UP.x, UP.y, UP.z].map(rat);
    let c = [
        &t[1] * &d[2] - &t[2] * &d[1],
        &t[2] * &d[0] - &t[0] * &d[2],
        &t[0] * &d[1] - &t[1] * &d[0],
    ];
    (&u[0] * &c[0] + &u[1] * &c[1] + &u[2] * &c[2]).cmp(&Rational::from(0u32))
}

/// The shared-curve crossing row exactly at a stored point, its tangent
/// a chord of stored points: the signs of `ê·(T × Δₖ)`, the rationals'
/// and the normalised row's where that is certified, for float rates and
/// exact ones (a third: the rational stage); `h₁` exactly 0 where the
/// motion's shadow runs along the tangent's (orders 2–4 then the
/// translations' own, not all 0); all 0 only for a vertical tangent. The
/// first sign that isn't 0 is worked out lazily, one filter run of the
/// Orient3 kind per order up to it. The unnormalised order 1 has the
/// row's sign where certified.
#[test]
fn the_shared_curve_signs_are_the_rows_exactly() {
    let like = Bounded::exact(0.0);
    let v = |d: DVec3| lit3(d, &like);
    let q = |d: DVec3| [d.x, d.y, d.z].map(rat);
    let (mut checked, mut rational) = (0, 0);
    for seed in varde_testing::seeds(64, 1024) {
        let mut r = Rng::new(seed);
        let (base, t) = (r.ints(4), r.ints(4));
        let chord = [base, base + t];
        let d = if seed % 3 == 0 {
            // Along the tangent in the shadow: `h₁ = 0`.
            t * r.int(3) + UP * r.int(2)
        } else {
            r.ints(4)
        };
        let third = seed % 4 == 1;
        let own = if third {
            Rate::exact(q(d).map(|x| x / Rational::from(3u32)))
        } else {
            Rate::Float(d)
        };
        let rates = [own, T[0].into(), T[1].into(), T[2].into()];
        let exact: [Ordering; ORDERS] = std::array::from_fn(|k| {
            let (s, w) = exact::value_sign(&SharedCrossing {
                chord,
                rate: &rates[k],
            });
            assert_eq!(w.of(PredKind::Orient3).filters, 1);
            assert_eq!(w.total(), w.of(PredKind::Orient3));
            if w.rationals() > 0 {
                rational += 1;
            }
            s.expect("finite")
        });
        for k in 0..ORDERS {
            let want = crossing_rational(&q(t), &rate_rational(&rates[k]));
            assert_eq!(exact[k], want, "seed {seed} order {}", k + 1);
        }
        let row = shared_crossing(&v(t), &[d, T[0], T[1], T[2]].map(v));
        for k in 0..ORDERS {
            if let Some(s) = row.ord[k].sign().filter(|s| *s != Ordering::Equal) {
                assert_eq!(exact[k], s, "seed {seed} order {}", k + 1);
                checked += 1;
            }
        }
        // The first sign that isn't 0, worked out up to it.
        let mut work = Work::default();
        let first = shared_crossing_sign(chord, &rates, &mut work);
        let want = (1..).zip(exact).find(|&(_, s)| s != Ordering::Equal);
        assert_eq!(first, want.map(|(k, s)| (k, Ok(s))), "seed {seed}");
        let runs = want.map_or(ORDERS, |(k, _)| k);
        assert_eq!(
            work.of(PredKind::Orient3).filters as usize,
            runs,
            "seed {seed}"
        );
        // The unnormalised order 1: the same sign where certified.
        let vb = |d: DVec3| [d.x, d.y, d.z].map(big);
        if let Some(o) = shared_crossing_h1(&vb(t), &vb(d)).sign() {
            assert_eq!(o, exact[0], "seed {seed}");
        }
        let vertical = t.x == 0.0 && t.y == 0.0 && t.z == 0.0
            || q(t)[0].clone() * rat(UP.y) == q(t)[1].clone() * rat(UP.x)
                && q(t)[1].clone() * rat(UP.z) == q(t)[2].clone() * rat(UP.y)
                && q(t)[0].clone() * rat(UP.z) == q(t)[2].clone() * rat(UP.x);
        assert_eq!(first.is_none(), vertical, "seed {seed}");
    }
    assert!(
        checked > 100 && rational > 0,
        "{checked} checked, {rational} rational"
    );
    let rates = [DVec3::X, T[0], T[1], T[2]].map(Rate::Float);
    let mut work = Work::default();
    assert_eq!(
        shared_crossing_sign([DVec3::ONE, DVec3::ONE + UP], &rates, &mut work),
        None
    );
    assert_eq!(work.of(PredKind::Orient3).filters as usize, ORDERS);
}

/// The shared-curve z-order row against the moved geometry: a curve
/// through `q`'s foot with the tangent `T` there (fixed), `q` on it moved
/// by `δ·Δₖ`; the curve's height above the moved `q`, its foot solved
/// again (the foot slid along the curve to `q`'s shadow, the curve's
/// second order dropping out at first order in `δ`), over `δ`: the row,
/// within `δ`; a vertical tangent refused.
#[test]
fn the_shared_zorder_row_is_the_moved_height() {
    let mut checked = 0;
    for seed in varde_testing::seeds(64, 1024) {
        let mut r = Rng::new(seed);
        let mut v = || DVec3::new(r.range(-2.0, 2.0), r.range(-2.0, 2.0), r.range(-2.0, 2.0));
        let (t, d) = (v(), v());
        let e = UP.normalize();
        let txy = t - e * e.dot(t);
        if txy.length() < 1e-3 {
            continue;
        }
        let lit = |v: DVec3| [v.x, v.y, v.z].map(big);
        let rates = [d, T[0], T[1], T[2]];
        let row = shared_zorder(&lit(t), &rates.map(lit)).expect("not vertical");
        for (k, dk) in rates.iter().enumerate() {
            let delta = 1e-6;
            let moved = *dk * delta;
            // The foot on the line through 0 along `t` under the moved
            // point's shadow, and the line's height above it there.
            let s = (moved - e * e.dot(moved)).dot(txy) / txy.dot(txy);
            let fd = e.dot(t * s - moved) / delta;
            let got = f(&row[k]);
            assert!(
                (got - fd).abs() < 1e-6 * (1.0 + fd.abs()),
                "seed {seed} order {}: {got} {fd}",
                k + 1
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "{checked} checked");
    let like = Bounded::exact(0.0);
    let v = |d: DVec3| lit3(d, &like);
    assert!(shared_zorder(&v(UP * 3.0), &[v(NU), v(T[0]), v(T[1]), v(T[2])]).is_none());
}

/// Curved rows normalise tangents and normals scaled first by a power of
/// two: tangents of any magnitude (`~10¹⁶⁰`, whose cross products
/// overflow, or `~10⁻¹⁶⁰`, whose underflow) give the rows of the same
/// directions at unit scale, certified, with the same signs.
#[test]
fn curved_rows_take_tangents_of_any_magnitude() {
    type L = Bounded<f64>;
    let pt = |v: [f64; 3]| v.map(L::exact);
    let scaled = |v: [f64; 3], s: f64| pt(v.map(|x| x * s));
    let overlap = |a: &L, b: &L| {
        let ([al, ah], [bl, bh]) = (a.enclose(), b.enclose());
        a.is_certified() && al <= bh && bl <= ah && a.sign() == b.sign()
    };
    let (te, tg) = ([0.3, -0.7, 0.2], [0.8, 0.1, -0.4]);
    let e = MovingAt::of_a(pt([1.0, 2.0, 3.0]), Sign::Pos, pt([0.1, 0.2, 0.3]));
    let g = MovingAt::fixed(pt([1.25, 2.0, 3.5]));
    let z0 = z_order(&e, &pt(te), &g, &pt(tg)).expect("crossing shadows");
    let c0 = curve_shadow(&e, &g, &pt(te));
    let h0 = shared_crossing_h1(&pt(te), &e.rates[0]);
    for s in [p2(532), p2(-532), p2(1000), p2(-1000)] {
        let z = z_order(&e, &scaled(te, s), &g, &scaled(tg, s)).expect("crossing shadows");
        let c = curve_shadow(&e, &g, &scaled(te, s));
        for (a, b) in [(&z.g, &z0.g), (&c.g, &c0.g)] {
            assert!(overlap(a, b), "{s:e}: {a:?} vs {b:?}");
        }
        for k in 0..ORDERS {
            assert!(overlap(&z.ord[k], &z0.ord[k]), "{s:e} {k}: {z:?}");
            assert!(overlap(&c.ord[k], &c0.ord[k]), "{s:e} {k}: {c:?}");
        }
        let h = shared_crossing_h1(&scaled(te, s), &e.rates[0]);
        assert!(h.sign().is_some() && h.sign() == h0.sign(), "{s:e}: {h:?}");
    }
}
