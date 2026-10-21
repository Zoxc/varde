//! The flat predicates against exact rationals, on an adversarial set:
//! general positions, exact ties (collinear and coplanar inputs on small
//! grids), ties off by one ulp, Shewchuk's near-collinear grid at `0.5`,
//! cancellation at the input bound, and inputs so small their products
//! leave the expansions' range. Each form's sign is the sign of its
//! spatial meaning worked out otherwise in rationals (a height by
//! division, a shadow side in shadow coordinates, a determinant expanded
//! another way), so the forms are positive multiples of what they claim;
//! their orders are the exact derivatives along each motion, the first
//! nonzero one is the perturbed sign, and every stage is reported under
//! the form's kind.

use std::cmp::Ordering;

use malachite::Rational as Q;

use super::*;
use crate::num::exact::{self, MAX_INPUT, NotFinite, ORDERS, moved_point, value_sign};

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn zero() -> Q {
    Q::from(0u32)
}

type Q3 = [Q; 3];

fn q3(p: DVec3) -> Q3 {
    [q(p.x), q(p.y), q(p.z)]
}

fn qsub(a: &Q3, b: &Q3) -> Q3 {
    [&a[0] - &b[0], &a[1] - &b[1], &a[2] - &b[2]]
}

fn qdot(a: &Q3, b: &Q3) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn qcross(a: &Q3, b: &Q3) -> Q3 {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

fn qdet(a: &Q3, b: &Q3, c: &Q3) -> Q {
    qdot(&qcross(a, b), c)
}

fn qscale(a: &Q3, s: &Q) -> Q3 {
    [&a[0] * s, &a[1] * s, &a[2] * s]
}

fn sgn(x: &Q) -> Ordering {
    x.cmp(&zero())
}

/// The forms, each over a list of 3D inputs (a scalar input is the `x`
/// of its own, a 2D point the `x, y` of its own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    /// `[dir, q, w]`.
    Along,
    /// `[a, b, c]`.
    Orient2,
    /// `[a, b, c, d]`.
    Dot2,
    /// `[a, b, c, d]`.
    Orient3,
    /// `[p, a, b, d]`.
    Orient3Dir,
    /// `[n, x, c]`.
    PlaneSide,
    /// `[up, d, off]`.
    ShadowSide,
    /// `[n, c, q, up]`.
    PlaneHeight,
    /// `[up, e₀, e₁, g₀, g₁]`.
    Crossing,
    /// `[axis, X_p, W_p, X_q, W_q]`: `axis·hom_sub(p, q)`.
    Hom,
    /// `[r, q, w]`.
    NearGap,
}

impl Form {
    const ALL: [Form; 11] = [
        Form::Along,
        Form::Orient2,
        Form::Dot2,
        Form::Orient3,
        Form::Orient3Dir,
        Form::PlaneSide,
        Form::ShadowSide,
        Form::PlaneHeight,
        Form::Crossing,
        Form::Hom,
        Form::NearGap,
    ];

    fn inputs(self) -> usize {
        match self {
            Form::Along | Form::Orient2 | Form::PlaneSide | Form::ShadowSide | Form::NearGap => 3,
            Form::Dot2 | Form::Orient3 | Form::Orient3Dir | Form::PlaneHeight => 4,
            Form::Crossing | Form::Hom => 5,
        }
    }

    fn kind(self) -> PredKind {
        match self {
            Form::Along | Form::PlaneSide => PredKind::Side,
            Form::Orient2 | Form::Dot2 | Form::NearGap => PredKind::Orient2,
            Form::Orient3 | Form::Orient3Dir | Form::ShadowSide | Form::PlaneHeight => {
                PredKind::Orient3
            }
            Form::Crossing => PredKind::Crossing,
            Form::Hom => PredKind::Hom,
        }
    }

    /// The form's value on `v`.
    fn value<N: Num>(self, v: &[V3<N>]) -> N {
        let two = |p: &V3<N>| [p[0].clone(), p[1].clone()];
        match self {
            Form::Along => along(&v[0], &v[1], &v[2]),
            Form::Orient2 => orient2(&two(&v[0]), &two(&v[1]), &two(&v[2])),
            Form::Dot2 => dot2(&two(&v[0]), &two(&v[1]), &two(&v[2]), &two(&v[3])),
            Form::Orient3 => orient3(&v[0], &v[1], &v[2], &v[3]),
            Form::Orient3Dir => orient3_dir(&v[0], &v[1], &v[2], &v[3]),
            Form::PlaneSide => plane_side(&v[0], &v[2][0], &v[1]),
            Form::ShadowSide => shadow_side(&v[0], &v[1], &v[2]),
            Form::PlaneHeight => plane_height(&v[0], &v[1][0], &v[2], &v[3]),
            Form::Crossing => crossing_height(&v[0], [&v[1], &v[2]], [&v[3], &v[4]]),
            Form::Hom => dot(&v[0], &hom_sub((&v[1], &v[2][0]), (&v[3], &v[4][0]))),
            Form::NearGap => near_gap(&v[0][0], &v[1], &v[2]),
        }
    }

    /// The sign of what the form means, worked out otherwise in
    /// rationals; `None` where that meaning is undefined (no crossing,
    /// a plane along `up`, a weight zero, an `up` of zero).
    fn spatial(self, v: &[Q3]) -> Option<Ordering> {
        Some(match self {
            Form::Along => sgn(&(qdot(&v[0], &v[2]) - qdot(&v[0], &v[1]))),
            Form::Orient2 => {
                // `det[a − c, b − c]`, robust's form of the same value.
                let (u, w) = (qsub(&v[0], &v[2]), qsub(&v[1], &v[2]));
                sgn(&(&u[0] * &w[1] - &u[1] * &w[0]))
            }
            Form::Dot2 => {
                let [a, b, c, d] = [0, 1, 2, 3].map(|i| [v[i][0].clone(), v[i][1].clone()]);
                let d2 = |x: &[Q; 2], y: &[Q; 2]| &x[0] * &y[0] + &x[1] * &y[1];
                sgn(&(d2(&b, &d) - d2(&b, &c) - d2(&a, &d) + d2(&a, &c)))
            }
            // `det(a − d, b − d, c − d)` is minus the form.
            Form::Orient3 => sgn(&qdet(
                &qsub(&v[0], &v[3]),
                &qsub(&v[1], &v[3]),
                &qsub(&v[2], &v[3]),
            ))
            .reverse(),
            // Multilinear: `det(a, b, d) − det(p, b, d) − det(a, p, d)`.
            Form::Orient3Dir => {
                let (p, a, b, d) = (&v[0], &v[1], &v[2], &v[3]);
                sgn(&(qdet(a, b, d) - qdet(p, b, d) - qdet(a, p, d)))
            }
            // The step `t` along `n` from `x` to the plane is `(c −
            // n·x)/|n|²`: `x` is on the side `n` points to when it is
            // negative.
            Form::PlaneSide => {
                let nn = qdot(&v[0], &v[0]);
                if nn == 0u32 {
                    return None;
                }
                sgn(&((&v[2][0] - qdot(&v[0], &v[1])) / nn)).reverse()
            }
            // `orient2` in shadow coordinates `(r·x, m·x)`, `r ⊥ up`,
            // `m = up × r`.
            Form::ShadowSide => {
                let up = &v[0];
                if up.iter().all(|x| *x == 0u32) {
                    return None;
                }
                let e = if up[1] == 0u32 && up[2] == 0u32 {
                    [zero(), Q::from(1u32), zero()]
                } else {
                    [Q::from(1u32), zero(), zero()]
                };
                let r = qcross(up, &e);
                let m = qcross(up, &r);
                let at = |x: &Q3| [qdot(&r, x), qdot(&m, x)];
                let (d, o) = (at(&v[1]), at(&v[2]));
                sgn(&(&d[0] * &o[1] - &d[1] * &o[0]))
            }
            // The step `t` along `up` from `q` to the plane.
            Form::PlaneHeight => {
                let (n, c, x, up) = (&v[0], &v[1][0], &v[2], &v[3]);
                let nu = qdot(n, up);
                if nu == 0u32 {
                    return None;
                }
                sgn(&((c - qdot(n, x)) / nu))
            }
            // `s·de − t·dg + h·up = g₀ − e₀`, by Cramer's rule.
            Form::Crossing => {
                let (up, e0, e1, g0, g1) = (&v[0], &v[1], &v[2], &v[3], &v[4]);
                let (de, dg) = (qsub(e1, e0), qsub(g1, g0));
                let ndg = qscale(&dg, &Q::from(-1));
                let den = qdet(&de, &ndg, up);
                if den == 0u32 {
                    return None;
                }
                sgn(&(qdet(&de, &ndg, &qsub(g0, e0)) / den))
            }
            // `axis·(X_p/W_p − X_q/W_q)` times `W_p·W_q`'s sign.
            Form::Hom => {
                let (wp, wq) = (&v[2][0], &v[4][0]);
                if *wp == 0u32 || *wq == 0u32 {
                    return None;
                }
                let d = qsub(
                    &qscale(&v[1], &(Q::from(1u32) / wp)),
                    &qscale(&v[3], &(Q::from(1u32) / wq)),
                );
                let s = sgn(&qdot(&v[0], &d));
                if sgn(wp) == sgn(wq) { s } else { s.reverse() }
            }
            Form::NearGap => {
                let (r, a, b) = (&v[0][0], &v[1], &v[2]);
                let dd = qdot(b, b) - Q::from(2u32) * qdot(a, b) + qdot(a, a);
                sgn(&(r * r - dd))
            }
        })
    }

    /// The library's predicate type for the form on stored inputs, where
    /// it has one: its sign and work.
    fn stored(self, p: &[DVec3]) -> Option<(exact::ExactSign, exact::Work, PredKind)> {
        fn run(pred: &impl Pred) -> Option<(exact::ExactSign, exact::Work, PredKind)> {
            let (s, w) = value_sign(pred);
            Some((s, w, pred.kind()))
        }
        let two = |i: usize| [p[i].x, p[i].y];
        match self {
            Form::Along => run(&Along {
                dir: p[0],
                q: p[1],
                w: p[2],
            }),
            Form::Orient2 => run(&Orient2d([two(0), two(1), two(2)])),
            Form::Dot2 => run(&Dot2d([two(0), two(1), two(2), two(3)])),
            Form::Orient3 => run(&Orient3d([p[0], p[1], p[2], p[3]])),
            Form::Orient3Dir => run(&Orient3Dir {
                p: p[0],
                a: p[1],
                b: p[2],
                d: p[3],
            }),
            Form::PlaneSide => run(&PlaneSide {
                normal: p[0],
                offset: p[2].x,
                p: p[1],
            }),
            Form::NearGap => run(&NearGap {
                q: p[1],
                w: p[2],
                r: q(p[0].x),
            }),
            _ => None,
        }
    }
}

/// A form on inputs moving at rates along the motions.
#[derive(Clone, Debug)]
struct Case {
    form: Form,
    p: Vec<DVec3>,
    rates: Vec<[DVec3; ORDERS]>,
}

impl Pred for Case {
    fn eval<N: Num>(&self) -> N {
        let v: Vec<V3<N>> = (0..self.p.len())
            .map(|i| moved_point(self.p[i], self.rates[i]))
            .collect();
        self.form.value(&v)
    }

    fn kind(&self) -> PredKind {
        self.form.kind()
    }
}

/// The case with every input moved by `t` along motion `k` (`1..=4`),
/// as exact rational inputs.
struct At<'a> {
    case: &'a Case,
    k: usize,
    t: i32,
}

impl Pred for At<'_> {
    fn eval<N: Num>(&self) -> N {
        let t = Q::from(self.t);
        let v: Vec<V3<N>> = (0..self.case.p.len())
            .map(|i| {
                let (p, r) = (q3(self.case.p[i]), q3(self.case.rates[i][self.k - 1]));
                [0, 1, 2].map(|j| N::rational(&(&p[j] + &r[j] * &t)))
            })
            .collect();
        self.case.form.value(&v)
    }

    fn kind(&self) -> PredKind {
        self.case.form.kind()
    }
}

/// The derivative at 0 along motion `k`, from seven exact values: exact
/// for polynomials of degree 6 at most, which every form is in `t` (the
/// crossing's, with every input moving, the highest).
fn derivative(case: &Case, k: usize) -> Q {
    let g = |t| exact::value(&At { case, k, t });
    let c = |x: i32| Q::from(x);
    (c(-1) * g(-3) + c(9) * g(-2) - c(45) * g(-1) + c(45) * g(1) - c(9) * g(2) + g(3)) / c(60)
}

/// `gₖ` of `case` through the stages' first-order numbers.
fn order(case: &Case, k: usize) -> Q {
    match k {
        1 => exact::order_value::<Case, 1>(case),
        2 => exact::order_value::<Case, 2>(case),
        3 => exact::order_value::<Case, 3>(case),
        _ => exact::order_value::<Case, 4>(case),
    }
}

/// The classes of the adversarial set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// Random mantissas, exponents from `2⁻³⁰` to `2¹⁹`.
    General,
    /// Small integers times a power of two.
    Grid,
    /// All inputs on one line of a grid: collinear, exact ties.
    Line,
    /// All inputs on one plane of a grid: coplanar, exact ties.
    Plane,
    /// [`Class::Line`] or [`Class::Plane`] with one coordinate one ulp
    /// off.
    Nudged,
    /// Shewchuk's grid: `0.5`, `12` and `24` along the diagonal, each
    /// moved by a few units of `2⁻⁵³`.
    Shewchuk,
    /// [`Class::Plane`] at about `2³¹` with differences of `2⁻²⁰`: large
    /// cancellation at the input bound.
    Large,
    /// [`Class::Grid`] or [`Class::Plane`] scaled by `2⁻⁵⁰⁰` to
    /// `2⁻¹⁰⁶⁰`: products below the expansions' range.
    Tiny,
}

const CLASSES: [Class; 8] = [
    Class::General,
    Class::Grid,
    Class::Line,
    Class::Plane,
    Class::Nudged,
    Class::Shewchuk,
    Class::Large,
    Class::Tiny,
];

/// The kernels' seeded test generator.
struct Rng(varde_testkit::rng::Rng);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(varde_testkit::rng::Rng::new(seed))
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    /// A small integer in `-n..=n`.
    fn int(&mut self, n: u64) -> f64 {
        self.below(2 * n + 1) as f64 - n as f64
    }

    /// A float with a random mantissa and sign and an exponent in
    /// `lo..=hi`.
    fn float(&mut self, lo: i64, hi: i64) -> f64 {
        let e = lo + self.below((hi - lo + 1) as u64) as i64;
        let x = f64::from_bits(((e + 1023) as u64) << 52 | self.0.next_u64() >> 12);
        if self.below(2) == 0 { x } else { -x }
    }

    fn ints(&mut self, n: u64) -> DVec3 {
        DVec3::new(self.int(n), self.int(n), self.int(n))
    }

    /// `n` inputs of `class`.
    fn inputs(&mut self, class: Class, n: usize) -> Vec<DVec3> {
        let pow2 = |e: i32| {
            let mut x = 1.0f64;
            for _ in 0..e.unsigned_abs() {
                x = if e < 0 { x * 0.5 } else { x * 2.0 };
            }
            x
        };
        match class {
            Class::General => (0..n)
                .map(|_| {
                    DVec3::new(
                        self.float(-30, 19),
                        self.float(-30, 19),
                        self.float(-30, 19),
                    )
                })
                .collect(),
            Class::Grid => {
                let s = pow2(self.below(30) as i32 - 10);
                self.lattice(Class::Grid, n, s, DVec3::ZERO)
            }
            Class::Line | Class::Plane => {
                let s = pow2(self.below(30) as i32 - 10);
                self.lattice(class, n, s, DVec3::ZERO)
            }
            Class::Nudged => {
                let base = if self.below(2) == 0 {
                    Class::Line
                } else {
                    Class::Plane
                };
                let mut p = self.inputs(base, n);
                let (i, j) = (self.below(n as u64) as usize, self.below(3) as usize);
                p[i][j] = if self.below(2) == 0 {
                    p[i][j].next_up()
                } else {
                    p[i][j].next_down()
                };
                p
            }
            Class::Shewchuk => {
                let u = pow2(-53);
                (0..n)
                    .map(|_| {
                        let c = [0.5, 12.0, 24.0][self.below(3) as usize];
                        DVec3::splat(c) + self.ints(8) * u
                    })
                    .collect()
            }
            Class::Large => {
                let base = DVec3::new(pow2(31), -pow2(31), pow2(30));
                self.lattice(Class::Plane, n, pow2(-20), base)
            }
            Class::Tiny => {
                let s = pow2(-(500 + self.below(561) as i32));
                let base = if self.below(2) == 0 {
                    Class::Grid
                } else {
                    Class::Plane
                };
                self.lattice(base, n, s, DVec3::ZERO)
            }
        }
    }

    /// `n` points `base + s·x` with `x` of small integers: random
    /// ([`Class::Grid`]), on one line or on one plane (below `2⁶` in
    /// magnitude, so exact at any power-of-two scale `s` down to
    /// `2⁻¹⁰⁶⁰`, and beside a `base` of `2³¹` at `2⁻²⁰`).
    fn lattice(&mut self, class: Class, n: usize, s: f64, base: DVec3) -> Vec<DVec3> {
        let (o, u, v) = (self.ints(8), self.ints(4), self.ints(4));
        (0..n)
            .map(|_| {
                let x = match class {
                    Class::Grid => self.ints(8),
                    Class::Line => o + u * self.int(3),
                    _ => o + u * self.int(3) + v * self.int(3),
                };
                base + x * s
            })
            .collect()
    }

    /// Rates for `n` inputs: small grid vectors, some zero (motion 1 of
    /// every input zero, sometimes, so the later motions decide).
    fn rates(&mut self, n: usize) -> Vec<[DVec3; ORDERS]> {
        let still = self.below(3) == 0;
        (0..n)
            .map(|_| {
                std::array::from_fn(|k| {
                    if (k == 0 && still) || self.below(4) == 0 {
                        DVec3::ZERO
                    } else {
                        self.ints(8) / 16.0
                    }
                })
            })
            .collect()
    }
}

/// Makes a form whose scalar input a lattice doesn't tie by itself an
/// exact tie, where the tie's input is a float within the bound: the
/// plane through the point, a radius the distance.
fn tie(form: Form, p: &mut [DVec3]) {
    let through = |n: DVec3, x: DVec3| {
        let c = n.dot(x);
        (c.abs() <= MAX_INPUT && q(c) == qdot(&q3(n), &q3(x))).then_some(c)
    };
    match form {
        Form::PlaneSide => {
            if let Some(c) = through(p[0], p[1]) {
                p[2].x = c;
            }
        }
        Form::PlaneHeight => {
            if let Some(c) = through(p[0], p[2]) {
                p[1].x = c;
            }
        }
        Form::NearGap => {
            // `‖(3, 4, 12)‖ = 13`.
            let m = p[0].x / 16.0;
            p[2] = p[1] + DVec3::new(3.0, 4.0, 12.0) * m;
            p[0].x = 13.0 * m;
        }
        _ => {}
    }
}

fn check_inputs(p: &[DVec3]) {
    for x in p {
        assert!(x.to_array().iter().all(|c| c.abs() <= MAX_INPUT), "{x}");
    }
}

/// Every form on every class: its exact sign is that of its spatial
/// meaning; the library's stored predicate gives the same sign at the
/// same price; exact ties are found as zeros, through the expansions.
#[test]
fn forms_are_exact_on_the_adversarial_set() {
    let mut zeros = [0u32; 11];
    let mut rationals = [0u32; 11];
    for seed in varde_testing::seeds(8, 256) {
        let mut rng = Rng::new(seed);
        for (f, form) in Form::ALL.iter().enumerate() {
            for class in CLASSES {
                let mut p = rng.inputs(class, form.inputs());
                if matches!(class, Class::Line | Class::Plane) {
                    tie(*form, &mut p);
                }
                check_inputs(&p);
                let case = Case {
                    form: *form,
                    rates: vec![[DVec3::ZERO; ORDERS]; p.len()],
                    p,
                };
                let (s, work) = value_sign(&case);
                let s = s.expect("finite inputs");
                let v: Vec<Q3> = case.p.iter().map(|&x| q3(x)).collect();
                let want = sgn(&exact::value(&case));
                assert_eq!(s, want, "{form:?} {class:?} seed {seed}: {:?}", case.p);
                if let Some(m) = form.spatial(&v) {
                    assert_eq!(s, m, "{form:?} {class:?} seed {seed}: {:?}", case.p);
                }
                // Only the form's kind is charged; a zero always takes
                // an exact stage.
                let r = work.of(form.kind());
                assert_eq!(work.total(), r, "{form:?}");
                assert_eq!(r.filters, 1);
                if s == Ordering::Equal && case.p.iter().any(|x| *x != DVec3::ZERO) {
                    zeros[f] += 1;
                }
                rationals[f] += r.rationals;
                if let Some((ls, lw, kind)) = form.stored(&case.p) {
                    assert_eq!(kind, form.kind(), "{form:?}");
                    assert_eq!(ls, Ok(s), "{form:?} {class:?} seed {seed}");
                    assert_eq!(lw, work, "{form:?} {class:?} seed {seed}");
                }
            }
        }
    }
    // The set reaches the ties and the rational stage of every form of
    // degree 2 and up in its inputs.
    for (f, form) in Form::ALL.iter().enumerate() {
        assert!(zeros[f] > 0, "{form:?} met no exact tie");
        assert!(rationals[f] > 0, "{form:?} never took the rationals");
    }
}

/// Shewchuk's near-collinear grid for `orient2d`: `a = (0.5 + i·u, 0.5 +
/// j·u)`, `u = 2⁻⁵³`, against `b = (12, 12)`, `c = (24, 24)`, where a
/// naive evaluation gets the sign wrong on a large part of the grid:
/// every sign exact.
#[test]
fn orient2d_on_shewchuks_grid() {
    let u = f64::EPSILON / 2.0;
    let (b, c) = ([12.0, 12.0], [24.0, 24.0]);
    let mut seen = [0u32; 3];
    for i in 0..64 {
        for j in 0..64 {
            let a = [0.5 + f64::from(i) * u, 0.5 + f64::from(j) * u];
            let (s, _) = value_sign(&Orient2d([a, b, c]));
            let v = |p: [f64; 2]| [q(p[0]), q(p[1])];
            let (a, b, c) = (v(a), v(b), v(c));
            let want = (&b[0] - &a[0]) * (&c[1] - &a[1]) - (&b[1] - &a[1]) * (&c[0] - &a[0]);
            let want = sgn(&want);
            assert_eq!(s, Ok(want), "{i} {j}");
            seen[(want as i8 + 1) as usize] += 1;
        }
    }
    assert!(seen.iter().all(|&n| n > 0), "{seen:?}");
}

/// The perturbed forms: each order is the exact derivative of the form
/// along its motion (the fixed translations' rows are the same forms
/// with their rates), and the sign is that of the first of `g, g₁, …,
/// g₄` that isn't zero.
#[test]
fn orders_are_the_derivatives_and_the_first_nonzero_decides() {
    let mut later = 0u32;
    for seed in varde_testing::seeds(4, 64) {
        let mut rng = Rng::new(seed);
        for form in Form::ALL {
            for class in CLASSES {
                if class == Class::Tiny {
                    // Five evaluations in rationals each: covered by the
                    // values above.
                    continue;
                }
                let p = rng.inputs(class, form.inputs());
                let rates = rng.rates(p.len());
                let case = Case { form, p, rates };
                let mut all = vec![exact::value(&case)];
                for k in 1..=ORDERS {
                    let d = order(&case, k);
                    assert_eq!(
                        d,
                        derivative(&case, k),
                        "{form:?} {class:?} seed {seed} order {k}"
                    );
                    all.push(d);
                }
                let first = all
                    .iter()
                    .position(|x| *x != 0u32)
                    .map(|k| (k, Ok(sgn(&all[k]))));
                let signs = exact::sign(&case);
                assert_eq!(signs.first, first, "{form:?} {class:?} seed {seed}");
                if first.is_some_and(|(k, _)| k >= 2) {
                    later += 1;
                }
                // One filter run per order tried, all of the form's kind.
                let tried = first.map_or(ORDERS + 1, |(k, _)| k + 1);
                let r = signs.work.of(form.kind());
                assert_eq!(signs.work.total(), r);
                assert_eq!(r.filters as usize, tried, "{form:?} {class:?} seed {seed}");
            }
        }
    }
    assert!(later > 0, "no case decided by a fixed translation");
}

/// Where the filter can't decide, the expansions do, and below their
/// range the rationals: each stage runs once per value, reported under
/// the predicate's kind.
#[test]
fn stages_run_in_order_under_the_kind() {
    let p = |x: f64, y: f64| [x, y];
    // General position: the filter alone.
    let (s, w) = value_sign(&Orient2d([p(0.0, 0.0), p(1.0, 0.0), p(0.3, 0.7)]));
    assert_eq!(s, Ok(Ordering::Greater));
    let r = w.of(PredKind::Orient2);
    assert_eq!((r.filters, r.expansions, r.rationals), (1, 0, 0));
    // An exact tie (points on `y = x`) whose differences round in the
    // filter: the expansions.
    let (s, w) = value_sign(&Orient2d([p(0.1, 0.1), p(0.2, 0.2), p(0.3, 0.3)]));
    assert_eq!(s, Ok(Ordering::Equal));
    let r = w.of(PredKind::Orient2);
    assert_eq!((r.filters, r.expansions, r.rationals), (1, 1, 0));
    // Collinear at `2⁻⁶⁰⁰`: products below `2⁻¹⁰⁷⁴`, the rationals.
    let s6 = f64::from_bits((1023 - 600) << 52);
    let (s, w) = value_sign(&Orient2d([
        p(s6, s6),
        p(2.0 * s6, 3.0 * s6),
        p(3.0 * s6, 5.0 * s6),
    ]));
    assert_eq!(s, Ok(Ordering::Equal));
    let r = w.of(PredKind::Orient2);
    assert_eq!((r.filters, r.expansions, r.rationals), (1, 1, 1));
    // One ulp off the line, as small: still exact.
    let (s, _) = value_sign(&Orient2d([
        p(s6, s6),
        p(2.0 * s6, 3.0 * s6),
        p(3.0 * s6, (5.0 * s6).next_up()),
    ]));
    assert_eq!(s, Ok(Ordering::Greater));
    // A plane side is priced as a side.
    let (_, w) = value_sign(&PlaneSide {
        normal: DVec3::new(1.0, 0.5, 0.25),
        offset: 1.0,
        p: DVec3::new(0.5, 0.5, 0.0),
    });
    assert_eq!(w.of(PredKind::Side).filters, 1);
    assert_eq!(w.total().filters, 1);
}

/// An input that isn't finite gives no sign, in every predicate.
#[test]
fn inputs_that_arent_finite_have_no_sign() {
    let bad = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    for x in bad {
        let p = DVec3::new(1.0, x, 2.0);
        let o = DVec3::new(0.5, 0.25, 0.125);
        assert_eq!(
            value_sign(&Orient3d([o, p, o * 2.0, o * 3.0])).0,
            Err(NotFinite)
        );
        assert_eq!(
            value_sign(&Orient2d([[0.0, 0.0], [1.0, x], [2.0, 1.0]])).0,
            Err(NotFinite)
        );
        assert_eq!(
            value_sign(&PlaneSide {
                normal: o,
                offset: x,
                p: o
            })
            .0,
            Err(NotFinite)
        );
        assert_eq!(value_sign(&Along { dir: o, q: o, w: p }).0, Err(NotFinite));
        assert_eq!(
            value_sign(&Orient3Dir {
                p: o,
                a: o * 2.0,
                b: o * 3.0,
                d: p
            })
            .0,
            Err(NotFinite)
        );
        assert_eq!(
            value_sign(&Dot2d([[0.0, 0.0], [1.0, x], [2.0, 1.0], [0.0, 1.0]])).0,
            Err(NotFinite)
        );
        assert_eq!(
            value_sign(&NearGap {
                q: o,
                w: p,
                r: Q::from(1u32)
            })
            .0,
            Err(NotFinite)
        );
    }
}

/// Radii of any size: a near gap with a radius far beyond the input
/// bound, or not dyadic, is still exact (the rationals take it).
#[test]
fn near_gap_radii_of_any_size() {
    let (a, b) = (DVec3::new(0.0, 0.0, 0.0), DVec3::new(3.0, 4.0, 12.0));
    let at = |r: Q| value_sign(&NearGap { q: a, w: b, r }).0;
    assert_eq!(at(Q::from(13u32)), Ok(Ordering::Equal));
    assert_eq!(
        at(Q::from(13u32) - Q::from(1u32) / Q::from(3u32)),
        Ok(Ordering::Less)
    );
    assert_eq!(
        at(Q::from(13u32) + Q::from(1u32) / Q::from(1u64 << 60)),
        Ok(Ordering::Greater)
    );
    let huge = Q::from(1u32) * Q::from(1u64 << 63) * Q::from(1u64 << 63);
    assert_eq!(at(huge), Ok(Ordering::Greater));
}
