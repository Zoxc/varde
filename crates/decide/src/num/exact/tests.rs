//! Exact predicates against `robust`'s orientations and exact rationals:
//! orientations written once as a [`Pred`] give `robust`'s signs on
//! random, nearly degenerate and degenerate points within the kernel's
//! coordinate range, and the rationals' signs on points so close to zero,
//! or values so far beyond `f64`'s range, that expansions can't hold them;
//! a perturbed predicate's orders are its derivatives along each motion,
//! and its sign is that of the first nonzero one; the filter only ever
//! gives the exact sign; and the stages run are reported.

use std::cmp::Ordering;

use malachite::Rational as Q;

use super::*;

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

/// The kernels' seeded test generator, with draws for exact arithmetic.
struct Rng(varde_testkit::rng::Rng);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(varde_testkit::rng::Rng::new(seed))
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    /// A float with a random mantissa and sign and an exponent in
    /// `lo..=hi` (`1 ≤ |m| < 2`).
    fn float(&mut self, lo: i64, hi: i64) -> f64 {
        let e = lo + self.below((hi - lo + 1) as u64) as i64;
        let x = f64::from_bits(((e + 1023) as u64) << 52 | self.0.next_u64() >> 12);
        if self.below(2) == 0 { x } else { -x }
    }

    /// A small integer in `-n..=n`.
    fn int(&mut self, n: u64) -> f64 {
        self.below(2 * n + 1) as f64 - n as f64
    }

    /// `N` points within the kernel's coordinate range (below `2²⁰`):
    /// random, with exponents down to `2⁻³⁰`; the last on the line (in
    /// 2D, `dim` 2) or plane of the first ones as rounded, perhaps an ulp
    /// off; on a grid of small integers times a power of two; or the last
    /// an exact combination of those, an exact tie.
    fn points<const N: usize>(&mut self, dim: usize) -> [DVec3; N] {
        let mut p = [DVec3::ZERO; N];
        let kind = self.below(4);
        if kind < 2 {
            for x in &mut p {
                *x = DVec3::new(
                    self.float(-30, 19),
                    self.float(-30, 19),
                    self.float(-30, 19),
                );
            }
        } else {
            let scale = f64::from_bits((1013 + self.below(30)) << 52);
            for x in &mut p {
                *x = DVec3::new(self.int(8), self.int(8), self.int(8)) * scale;
            }
        }
        if kind % 2 == 1 {
            let (s, t) = if kind == 1 {
                (self.float(-3, 0), self.float(-3, 0))
            } else {
                (self.int(2), self.int(2))
            };
            let mut along = p[0] + (p[1] - p[0]) * s;
            if dim == 3 {
                along += (p[2] - p[0]) * t;
            }
            p[N - 1] = along;
            if kind == 1 && self.below(2) == 0 {
                p[N - 1].x = p[N - 1].x.next_up();
            }
        }
        if dim == 2 {
            for x in &mut p {
                x.z = 0.0;
            }
        }
        p
    }
}

/// `det[a − d, b − d, c − d]`, `robust::orient3d`'s value: positive when
/// `d` is below the plane of `a, b, c` (counterclockwise seen from above).
/// The points with rows move along the motions at those rates.
struct Orient3d {
    p: [DVec3; 4],
    rows: [Option<[DVec3; ORDERS]>; 4],
}

impl Orient3d {
    fn fixed(p: [DVec3; 4]) -> Orient3d {
        Orient3d { p, rows: [None; 4] }
    }
}

impl Pred for Orient3d {
    fn eval<N: Num>(&self) -> N {
        let [a, b, c, d] = [0, 1, 2, 3].map(|i| match self.rows[i] {
            Some(rows) => moved_point(self.p[i], rows),
            None => point(self.p[i]),
        });
        det3(&sub(&a, &d), &sub(&b, &d), &sub(&c, &d))
    }
}

/// `det[a − c, b − c]` of the points' `x, y`, `robust::orient2d`'s value:
/// positive when `a, b, c` turn counterclockwise.
struct Orient2d([DVec3; 3]);

impl Pred for Orient2d {
    fn eval<N: Num>(&self) -> N {
        let [a, b, c] = self.0.map(point::<N>);
        let (u, v) = (sub(&a, &c), sub(&b, &c));
        det2([&u[0], &u[1]], [&v[0], &v[1]])
    }
}

/// `det[a − d, b − d, c − d]` in rationals, each point moved by `t` times
/// its rates along one motion.
fn orient3d_q(p: &[DVec3; 4], moves: &[DVec3; 4], t: i32) -> Q {
    let pt = |i: usize| [0, 1, 2].map(|k| q(p[i][k]) + q(moves[i][k]) * Q::from(t));
    let [a, b, c, d] = [pt(0), pt(1), pt(2), pt(3)];
    let e = |x: &[Q; 3], k: usize| &x[k] - &d[k];
    let (a, b, c) = (
        [0, 1, 2].map(|k| e(&a, k)),
        [0, 1, 2].map(|k| e(&b, k)),
        [0, 1, 2].map(|k| e(&c, k)),
    );
    &a[0] * (&b[1] * &c[2] - &b[2] * &c[1]) - &a[1] * (&b[0] * &c[2] - &b[2] * &c[0])
        + &a[2] * (&b[0] * &c[1] - &b[1] * &c[0])
}

fn sign_of(x: f64) -> Ordering {
    x.partial_cmp(&0.0).expect("not NaN")
}

fn sign_q(x: &Q) -> Ordering {
    x.cmp(&Q::from(0))
}

fn c3(p: DVec3) -> robust::Coord3D<f64> {
    robust::Coord3D {
        x: p.x,
        y: p.y,
        z: p.z,
    }
}

fn c2(p: DVec3) -> robust::Coord<f64> {
    robust::Coord { x: p.x, y: p.y }
}

/// The value's sign as [`sign`] gives it unperturbed: `Equal` for an
/// exact zero.
fn unmoved(s: Signs) -> Ordering {
    match s.first {
        Some((0, s)) => s,
        None => Ordering::Equal,
        Some(other) => panic!("an unmoved predicate decided at order {other:?}"),
    }
}

#[test]
fn orientations_match_robust() {
    let (mut filtered, mut exact, mut zeros) = (0, 0, 0);
    for seed in varde_testing::seeds(4, 64) {
        let mut rng = Rng::new(seed);
        for _ in 0..1000 {
            let p: [DVec3; 4] = rng.points(3);
            let s = sign(&Orient3d::fixed(p));
            let want = sign_of(robust::orient3d(c3(p[0]), c3(p[1]), c3(p[2]), c3(p[3])));
            assert_eq!(unmoved(s), want, "orient3d {p:?}");
            assert_eq!(value_sign(&Orient3d::fixed(p)).0, want, "{p:?}");
            let zero = [DVec3::ZERO; 4];
            assert_eq!(want, sign_q(&orient3d_q(&p, &zero, 0)), "{p:?}");
            assert_eq!(s.work.rationals, 0, "{p:?}");
            if s.work.expansions == 0 {
                filtered += 1;
            } else {
                exact += 1;
            }
            if want == Ordering::Equal {
                zeros += 1;
            }

            let p: [DVec3; 3] = rng.points(2);
            let (s, _) = value_sign(&Orient2d(p));
            let want = sign_of(robust::orient2d(c2(p[0]), c2(p[1]), c2(p[2])));
            assert_eq!(s, want, "orient2d {p:?}");
        }
    }
    // Every stage was taken, and exact zeros were met.
    assert!(
        filtered > 500 && exact > 500 && zeros > 100,
        "{filtered} {exact} {zeros}"
    );
}

/// Points around a center far smaller than `2⁻³⁰⁰`, or tiny offsets from
/// ordinary points: the expansions' products would underflow, so the
/// rationals decide, and agree with the exact value.
#[test]
fn tiny_coordinates_take_the_rationals() {
    let mut rationals = 0;
    for seed in varde_testing::seeds(4, 64) {
        let mut rng = Rng::new(seed);
        for k in 0..300 {
            let mut p: [DVec3; 4] = rng.points(3);
            let scale = f64::from_bits((1023 - 300 - rng.below(700)) << 52);
            for x in &mut p {
                *x *= scale;
            }
            if k % 2 == 0 {
                // One point at an ordinary place, the rest within tiny
                // offsets of the origin.
                p[0] = DVec3::new(rng.float(-5, 5), rng.float(-5, 5), rng.float(-5, 5));
            }
            let zero = [DVec3::ZERO; 4];
            let want = sign_q(&orient3d_q(&p, &zero, 0));
            let (s, work) = value_sign(&Orient3d::fixed(p));
            assert_eq!(s, want, "{p:?}");
            rationals += work.rationals;
            // Swapping two points flips the sign, through whichever stage.
            let swapped = [p[1], p[0], p[2], p[3]];
            assert_eq!(value_sign(&Orient3d::fixed(swapped)).0, want.reverse());
        }
    }
    assert!(rationals > 100, "{rationals}");
}

/// The product of `factors` differences `x − y`.
struct Product(Vec<(f64, f64)>);

impl Pred for Product {
    fn eval<N: Num>(&self) -> N {
        self.0.iter().fold(N::lit(1.0), |acc, &(x, y)| {
            acc.mul(&N::lit(x).sub(&N::lit(y)))
        })
    }
}

/// Products of 40 differences past `f64`'s range at either end are signed
/// by the rationals; of 12 within it, some cancelling to the last bit, by
/// the filter or the expansions alone.
#[test]
fn products_beyond_the_range_take_the_rationals() {
    let mut rng = Rng::new(3);
    for (lo, hi, rational) in [(28, 31, true), (-31, -28, true), (-5, 5, false)] {
        for _ in 0..50 {
            let factors: Vec<(f64, f64)> = (0..if rational { 40 } else { 12 })
                .map(|_| {
                    let x = rng.float(lo, hi);
                    // Within the range, some differences cancel to the
                    // last bit; beyond it, none shrink.
                    let y = if !rational && rng.below(2) == 0 {
                        x.next_up()
                    } else {
                        -x.signum() * rng.float(lo, hi).abs()
                    };
                    (x, y)
                })
                .collect();
            let want = factors
                .iter()
                .fold(Ordering::Greater, |s, &(x, y)| match sign_of(x - y) {
                    Ordering::Less => s.reverse(),
                    Ordering::Equal => Ordering::Equal,
                    Ordering::Greater => s,
                });
            let (s, work) = value_sign(&Product(factors));
            assert_eq!(s, want);
            assert_eq!(work.rationals > 0, rational, "{lo} {hi} {work:?}");
        }
    }
}

/// The derivative at zero of a polynomial `f` of degree at most 3, from
/// its values at `±1` and `±2`: with `h(t) = (f(t) − f(−t))/2 = b·t + d·t³`,
/// `b = (8·h(1) − h(2)) / 6`.
fn derivative(f: impl Fn(i32) -> Q) -> Q {
    let h = |t: i32| (f(t) - f(-t)) / Q::from(2);
    (Q::from(8) * h(1) - h(2)) / Q::from(6)
}

/// Rates for a point: a random direction of small integers or dyadics,
/// often zero in a component, sometimes all zero.
fn rates(rng: &mut Rng) -> DVec3 {
    let mut c = || match rng.below(4) {
        0 => 0.0,
        1 => rng.int(3),
        _ => rng.float(-4, 2),
    };
    DVec3::new(c(), c(), c())
}

/// A perturbed orientation's sign is that of the first nonzero of its
/// value and its derivatives along the motions, from rationals over the
/// really moved points; the filter's sign, when it gives one, is the
/// exact one at every order; and the sign flips with a swap of two points.
#[test]
fn orders_are_the_derivatives_along_each_motion() {
    let mut decided = [0; ORDERS + 2];
    for seed in varde_testing::seeds(4, 64) {
        let mut rng = Rng::new(seed);
        for _ in 0..300 {
            let mut p: [DVec3; 4] = rng.points(3);
            // Often four coplanar points of a grid, a tie.
            if rng.below(2) == 0 {
                let mut grid = || DVec3::new(rng.int(8), rng.int(8), rng.int(8)) * 0.25;
                let [a, b, c] = [grid(), grid(), grid()];
                let (s, t) = (rng.int(2), rng.int(2));
                p = [a, b, c, a + (b - a) * s + (c - a) * t];
            }
            let mut rows = [None; 4];
            for r in &mut rows {
                if rng.below(3) != 0 {
                    *r = Some([0; ORDERS].map(|_| rates(&mut rng)));
                }
            }
            // Sometimes every point moves alike along the first motions, a
            // translation: those orders are zero.
            if rng.below(3) == 0 {
                let shared: [DVec3; ORDERS] = [0; ORDERS].map(|_| rates(&mut rng));
                let alike = 1 + rng.below(ORDERS as u64) as usize;
                for r in &mut rows {
                    let mut own = r.unwrap_or([DVec3::ZERO; ORDERS]);
                    own[..alike].copy_from_slice(&shared[..alike]);
                    *r = Some(own);
                }
            }
            let pred = Orient3d { p, rows };
            let moves = |k: usize| rows.map(|r| r.map_or(DVec3::ZERO, |r| r[k]));
            let zero = [DVec3::ZERO; 4];
            let mut exact = vec![orient3d_q(&p, &zero, 0)];
            for k in 0..ORDERS {
                exact.push(derivative(|t| orient3d_q(&p, &moves(k), t)));
            }
            let want = exact
                .iter()
                .enumerate()
                .map(|(k, g)| (k, sign_q(g)))
                .find(|&(_, s)| s != Ordering::Equal);
            let got = sign(&pred);
            assert_eq!(got.first, want, "{p:?} {rows:?}");
            decided[want.map_or(ORDERS + 1, |(k, _)| k)] += 1;

            let filters = [
                pred.eval::<Approx>().sign(),
                pred.eval::<Dual<Approx, 1>>().d.sign(),
                pred.eval::<Dual<Approx, 2>>().d.sign(),
                pred.eval::<Dual<Approx, 3>>().d.sign(),
                pred.eval::<Dual<Approx, 4>>().d.sign(),
            ];
            for (f, g) in filters.iter().zip(&exact) {
                if let Some(f) = f {
                    assert_eq!(*f, sign_q(g), "{p:?} {rows:?}");
                }
            }

            let swapped = Orient3d {
                p: [p[1], p[0], p[2], p[3]],
                rows: [rows[1], rows[0], rows[2], rows[3]],
            };
            let flipped = want.map(|(k, s)| (k, s.reverse()));
            assert_eq!(sign(&swapped).first, flipped, "{p:?} {rows:?}");
        }
    }
    // Every order decided somewhere, and translations left none.
    assert!(decided.iter().all(|&n| n > 0), "{decided:?}");
}

/// Rates given as expressions (an edge's direction, worked out exactly)
/// move a point as the same rates given as numbers.
#[test]
fn rates_from_expressions_move_as_numbers() {
    struct Height {
        p: DVec3,
        from: DVec3,
        to: DVec3,
    }
    impl Pred for Height {
        /// The height of `p` above `z = 0`, `p` moved along `to − from`
        /// at every order.
        fn eval<N: Num>(&self) -> N {
            let dir = sub(&point::<N>(self.to), &point(self.from));
            N::lit(self.p.z).moved_by(&[0, 1, 2, 3].map(|_| dir[2].clone()))
        }
    }
    let mut rng = Rng::new(5);
    for _ in 0..200 {
        let z = [0.0, rng.float(-20, 10)][rng.below(2) as usize];
        let [from, to, ..] = rng.points::<4>(3);
        let pred = Height {
            p: DVec3::new(1.0, 2.0, z),
            from,
            to,
        };
        let rise = q(to.z) - q(from.z);
        let want = match (sign_of(z), sign_q(&rise)) {
            (Ordering::Equal, Ordering::Equal) => None,
            (Ordering::Equal, s) => Some((1, s)),
            (s, _) => Some((0, s)),
        };
        assert_eq!(sign(&pred).first, want, "{z} {from} {to}");
    }
}

/// The filter decides a clear configuration and proves the exact zero of
/// a repeated point; a tie of distinct points needs the expansions.
#[test]
fn stages_are_reported() {
    let p = [
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(0.0, 1.0, 0.0),
        DVec3::new(0.0, 0.0, -1.0),
    ];
    let (s, work) = value_sign(&Orient3d::fixed(p));
    assert_eq!(s, Ordering::Greater);
    assert_eq!(
        work,
        Work {
            filters: 1,
            ..Work::default()
        }
    );

    let repeated = [p[0], p[1], p[0], p[3]];
    let (s, work) = value_sign(&Orient3d::fixed(repeated));
    assert_eq!((s, work.filters, work.expansions), (Ordering::Equal, 1, 0));

    // On the plane `z = x`, far out: the filter can't prove the zero.
    let coplanar = [
        DVec3::new(1e6, 1e6, 1e6),
        DVec3::new(1e6 + 0.1, 1e6, 1e6 + 0.1),
        DVec3::new(1e6, 1e6 + 0.1, 1e6),
        DVec3::new(1e6 + 0.2, 1e6 + 0.3, 1e6 + 0.2),
    ];
    let (s, work) = value_sign(&Orient3d::fixed(coplanar));
    assert_eq!(s, Ordering::Equal);
    assert_eq!((work.filters, work.expansions, work.rationals), (1, 1, 0));
    assert!(work.longest >= 1);

    // Moved off the plane by the second motion only: the filter proves
    // order 1 zero (no point moves along it) and decides order 2.
    let mut rows = [DVec3::ZERO; ORDERS];
    rows[1] = DVec3::Z;
    let pred = Orient3d {
        p: coplanar,
        rows: [None, None, None, Some(rows)],
    };
    let s = sign(&pred);
    assert!(matches!(
        s.first,
        Some((2, Ordering::Less | Ordering::Greater))
    ));
    assert_eq!((s.work.filters, s.work.expansions), (3, 1));
}

/// Debug builds assert every rate in range, also the rates of motions
/// whose order is never worked out (here the value decides).
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "beyond 2³²")]
fn rates_beyond_the_range_are_asserted() {
    let mut rows = [DVec3::ZERO; ORDERS];
    rows[3] = DVec3::splat(2.0 * MAX_INPUT);
    let pred = Orient3d {
        p: [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::NEG_Z],
        rows: [None, None, None, Some(rows)],
    };
    sign(&pred);
}

/// Four coincident points moving apart: the moved orientation is of
/// order `δ³`, not zero, but every first-order value is zero, so there
/// is no sign; the caller's rule is missing.
#[test]
fn no_first_order_sign_is_none() {
    let rows = |v: DVec3| {
        let mut r = [DVec3::ZERO; ORDERS];
        r[0] = v;
        Some(r)
    };
    let p = [DVec3::splat(1.5); 4];
    let moves = [DVec3::X, DVec3::Y, DVec3::Z, DVec3::ZERO];
    let pred = Orient3d {
        p,
        rows: [rows(moves[0]), rows(moves[1]), rows(moves[2]), None],
    };
    assert_eq!(sign_q(&orient3d_q(&p, &moves, 1)), Ordering::Greater);
    assert_eq!(sign(&pred).first, None);
}
