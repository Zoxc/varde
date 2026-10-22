//! Expansions against exact rational arithmetic and against Geogram's
//! exact predicates (`geogram_predicates`): every result equals the exact
//! value, its components are in increasing magnitude, none zero,
//! nonoverlapping, so its sign is the exact sign; the signs of 2D and 3D
//! orientations and 3×3 determinants built from the operations agree with
//! Geogram's, on random, degenerate and nearly degenerate inputs (4×4
//! determinants against the rationals only: that crate's `det_4d` filter
//! leaves the last column out of its error bound and certifies a wrong
//! sign, see `det4_geogram_filter_misses_a_zero`; 3D orientations against
//! its exact expansions alone, as its `orient_3d` filter certifies wrong
//! signs too, see `orient3d_geogram_filter_certifies_a_wrong_sign`; Geogram
//! v1.10.1's own filters, which that crate's should be, bound both
//! soundly). Plus sums, products and
//! scalings across the documented range, regroupings of one value that
//! cancel to zero, and the same bits with and without hardware FMA.

use std::cmp::Ordering;

use geogram_predicates::Sign;
use malachite::Rational as Q;

use super::*;

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn value(e: &Exp) -> Q {
    e.components().iter().fold(q(0.0), |s, &x| s + q(x))
}

/// The bits of a finite nonzero `x`: its lowest and highest set bits'
/// exponents.
fn bit_span(x: f64) -> (i64, i64) {
    let bits = x.to_bits();
    let field = ((bits >> 52) & 0x7ff) as i64;
    let frac = bits & ((1 << 52) - 1);
    let (m, e) = if field == 0 {
        (frac, -1074)
    } else {
        (frac | 1 << 52, field - 1075)
    };
    (
        e + i64::from(m.trailing_zeros()),
        e + 63 - i64::from(m.leading_zeros()),
    )
}

/// Panics unless `e` is a valid expansion: components finite, none zero,
/// in increasing magnitude and nonoverlapping (each one's highest set bit
/// below the next one's lowest).
fn check_form(e: &Exp, what: &str) {
    let c = e.components();
    for &x in c {
        assert!(
            x.is_finite() && x != 0.0,
            "{what}: component {x:e} in {c:?}"
        );
    }
    for i in 1..c.len() {
        let (_, small_hi) = bit_span(c[i - 1]);
        let (large_lo, _) = bit_span(c[i]);
        assert!(small_hi < large_lo, "{what}: {c:?} overlaps at {i}");
    }
}

/// `e` checked against its exact value.
fn check(e: &Exp, exact: &Q, what: &str) {
    check_form(e, what);
    assert!(
        value(e) == *exact,
        "{what}: {:?} isn't exact",
        e.components()
    );
}

fn sign_of(g: Sign) -> Ordering {
    match g {
        Sign::Negative => Ordering::Less,
        Sign::Zero => Ordering::Equal,
        Sign::Positive => Ordering::Greater,
    }
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
    /// `lo..=hi` (`1 ≤ |m| < 2`, normal).
    fn float(&mut self, lo: i64, hi: i64) -> f64 {
        let e = lo + self.below((hi - lo + 1) as u64) as i64;
        let x = f64::from_bits(((e + 1023) as u64) << 52 | self.0.next_u64() >> 12);
        if self.below(2) == 0 { x } else { -x }
    }

    /// A chain's input: a random float, one of a few bits, or one of 51
    /// to 53 bits all set, across a wide exponent range.
    fn input(&mut self) -> f64 {
        let bits = match self.below(4) {
            0 | 1 => return self.float(-100, 100),
            2 => 1 + self.below(4),
            _ => 51 + self.below(3),
        };
        if bits > 4 {
            let m = ((1u64 << bits) - 1) as f64;
            let e = self.below(201) as i64 - 100 - bits as i64;
            let x = m * f64::from_bits(((e + 1023) as u64) << 52);
            return if self.below(2) == 0 { x } else { -x };
        }
        let m = (self.0.next_u64() >> (64 - bits)) as f64 + 1.0;
        let e = self.below(201) as i64 - 100;
        let x = m * f64::from_bits(((e + 1023) as u64) << 52);
        if self.below(2) == 0 { x } else { -x }
    }

    /// A matrix entry: mostly a float of moderate exponent, sometimes a
    /// small integer or zero, so determinants vanish or nearly do.
    fn entry(&mut self) -> f64 {
        match self.below(8) {
            0 => 0.0,
            1 | 2 => self.below(7) as f64 - 3.0,
            _ => self.float(-60, 60),
        }
    }

    /// Rows for a determinant: random, or the last a sum of two others
    /// (exact on these small integers and dyadics: a zero determinant),
    /// perhaps moved by an ulp.
    fn rows<const N: usize>(&mut self) -> [[f64; N]; N] {
        let mut m = [[0.0; N]; N];
        for row in &mut m {
            for x in row.iter_mut() {
                *x = self.entry();
            }
        }
        if self.below(2) == 0 {
            for j in 0..N {
                m[0][j] = self.below(1 << 20) as f64 / 1024.0;
                m[1][j] = self.below(1 << 20) as f64 / 1024.0;
                m[N - 1][j] = m[0][j] + m[1][j];
            }
            if self.below(2) == 0 {
                let j = self.below(N as u64) as usize;
                m[N - 1][j] = if self.below(2) == 0 {
                    m[N - 1][j].next_up()
                } else {
                    m[N - 1][j].next_down()
                };
            }
        }
        m
    }
}

/// The permutations of `0..n` with their signs.
fn permutations(n: usize) -> Vec<(Vec<usize>, bool)> {
    if n == 1 {
        return vec![(vec![0], true)];
    }
    let mut out = Vec::new();
    for (p, even) in permutations(n - 1) {
        // Insert n−1 at each place; moving it k places left flips the
        // sign k times.
        for k in 0..n {
            let mut r = p.clone();
            r.insert(n - 1 - k, n - 1);
            out.push((r, even == (k % 2 == 0)));
        }
    }
    out
}

/// The determinant (`N ≥ 2`) by the Leibniz formula: each term a product of
/// entries by `of_product` and `scale`, the terms summed.
fn det<const N: usize>(m: &[[f64; N]; N]) -> (Exp, Q) {
    let mut e = Exp::zero();
    let mut exact = q(0.0);
    for (p, even) in permutations(N) {
        let mut t = Exp::of_product(m[0][p[0]], m[1][p[1]]);
        let mut tq = q(m[0][p[0]]);
        for (i, &j) in p.iter().enumerate().skip(1) {
            if i >= 2 {
                t = &t * m[i][j];
            }
            tq *= q(m[i][j]);
        }
        check(&t, &tq, "term");
        if even {
            e = &e + &t;
            exact += tq;
        } else {
            e = &e - &t;
            exact -= tq;
        }
        check(&e, &exact, "partial sum");
    }
    (e, exact)
}

#[test]
fn determinants_match_geogram() {
    for seed in varde_testing::seeds(4, 64) {
        let mut rng = Rng::new(seed);
        let (mut zero, mut nonzero) = (0, 0);
        for _ in 0..500 {
            let m: [[f64; 3]; 3] = rng.rows();
            let (e, exact) = det(&m);
            check(&e, &exact, "det3");
            let g = geogram_predicates::det_3d(&m[0], &m[1], &m[2]);
            assert_eq!(e.sign(), sign_of(g), "{m:?}");
            if e.is_zero() {
                zero += 1;
            } else {
                nonzero += 1;
            }
            let m: [[f64; 4]; 4] = rng.rows();
            let (e, exact) = det(&m);
            check(&e, &exact, "det4");
        }
        assert!(
            zero > 20 && nonzero > 200,
            "seed {seed}: {zero} zero, {nonzero} not"
        );
    }
}

/// The 4×4 determinant whose exact value is zero (the last row is the sum
/// of the first two) but where `geogram_predicates` 0.4.0's `det_4d`
/// reports a negative sign: its filter bounds the error by the first three
/// columns' magnitudes, and the third row's large last entry is outside
/// them. The expansions get zero. If this crate's filter is fixed, its
/// `det_4d` can join the oracle above.
#[test]
fn det4_geogram_filter_misses_a_zero() {
    let m = [
        [
            769.6826171875,
            649.8271484375,
            375.9794921875,
            404.1728515625,
        ],
        [
            392.5126953125,
            1011.556640625,
            934.0927734375,
            889.009765625,
        ],
        [
            0.0,
            5.196567615698994e-11,
            -7.232071624160839e-12,
            34787087.215070024,
        ],
        [
            1162.1953125,
            1661.3837890625,
            1310.072265625,
            1293.1826171875,
        ],
    ];
    let (e, exact) = det(&m);
    check(&e, &exact, "det4");
    assert!(e.is_zero());
    let g = geogram_predicates::det_4d(&m[0], &m[1], &m[2], &m[3]);
    assert_eq!(g, Sign::Negative, "geogram_predicates' det_4d changed");
}

/// `det(p₁ − p₀, p₂ − p₀, p₃ − p₀)`'s sign by `geogram_predicates`' exact
/// expansions alone: `det_compare_4d` of the rows `(pᵢ, 1)` (its last
/// row `p₄ − p₃` with `p₃` zero), which is minus the orientation.
fn geogram_orient3d_exact(p: &[[f64; 3]; 4]) -> Ordering {
    let h = |i: usize| [p[i][0], p[i][1], p[i][2], 1.0];
    sign_of(geogram_predicates::det_compare_4d(
        &h(0),
        &h(1),
        &h(2),
        &[0.0; 4],
        &h(3),
    ))
    .reverse()
}

/// Difference rows `(ε, 1, 1)`, `(ε, 1, 1)` nearly parallel in their last
/// two columns and `(ε, ε, ε)`, with the first entry of the third chosen
/// so the exact orientation nearly cancels: `geogram_predicates` 0.4.0's
/// `orient_3d` filter bounds the error by `1.78·10⁻¹⁵·max₁·max₂·max₃`
/// with its third group missing `a₁₂, a₁₃, a₂₂, a₂₃`, so the term `a₃₁·(a₁₂a₂₃
/// − a₁₃a₂₂)` and its rounding (about `u·ε`) lie outside a bound of order
/// `ε²`, and it certifies the wrong sign. Geogram v1.10.1's generated
/// filter (constant `5.11·10⁻¹⁵`, the third group `a₂₂, a₂₃, a₃₂, a₃₃`)
/// leaves it to its exact stage, as the expansions here decide it. If the
/// crate's filter is fixed, its `orient_3d` can be the oracle above again.
#[test]
fn orient3d_geogram_filter_certifies_a_wrong_sign() {
    let p = [
        [0.0, 0.0, 0.0],
        [
            -1.7522873058184817e-8,
            -0.18041593380608867,
            -0.7471274138760322,
        ],
        [
            -9.398726613909101e-8,
            -0.0946386205547942,
            -0.39191239639951025,
        ],
        [
            -9.44286628109264e-8,
            2.1614344203940886e-7,
            7.329057152846528e-8,
        ],
    ];
    let d = |i: usize, k: usize| q(p[i][k]) - q(p[0][k]);
    let exact = d(1, 0) * (d(2, 1) * d(3, 2) - d(2, 2) * d(3, 1))
        - d(2, 0) * (d(1, 1) * d(3, 2) - d(1, 2) * d(3, 1))
        + d(3, 0) * (d(1, 1) * d(2, 2) - d(1, 2) * d(2, 1));
    let want = exact.cmp(&q(0.0));
    assert_ne!(want, Ordering::Less);
    assert_eq!(geogram_orient3d_exact(&p), want);
    let g = geogram_predicates::orient_3d(&p[0], &p[1], &p[2], &p[3]);
    assert_eq!(g, Sign::Negative, "geogram_predicates' orient_3d changed");
}

/// Points `p0`, `p1`, `p2`, and then `p3` (in `D` dimensions): random, or
/// the last on the line `p0 p1` as rounded (in 3D on the plane of the
/// others, as rounded), perhaps then an ulp off.
fn points<const D: usize>(rng: &mut Rng) -> [[f64; D]; 4] {
    let mut p = [[0.0; D]; 4];
    for c in p.iter_mut().flatten() {
        *c = rng.float(-30, 30);
    }
    if rng.below(2) == 0 {
        let (s, t) = (rng.float(-3, 0), rng.float(-3, 0));
        let last = D;
        for k in 0..D {
            let along = p[0][k] + s * (p[1][k] - p[0][k]);
            p[last][k] = if D == 2 {
                along
            } else {
                along + t * (p[2][k] - p[0][k])
            };
        }
        if rng.below(2) == 0 {
            p[last][0] = p[last][0].next_up();
        }
    }
    p
}

/// Orientations from differences of coordinates: `of_sum` of a
/// coordinate and a negated one, then products of expansions, as the
/// predicates will; nearly collinear and coplanar points included.
#[test]
fn orientations_match_geogram() {
    for seed in varde_testing::seeds(4, 64) {
        let mut rng = Rng::new(seed);
        for _ in 0..1000 {
            let p: [[f64; 2]; 4] = points(&mut rng);
            let d = |i: usize, k: usize| Exp::of_sum(p[i][k], -p[0][k]);
            let qd = |i: usize, k: usize| q(p[i][k]) - q(p[0][k]);
            let e = &(&d(1, 0) * &d(2, 1)) - &(&d(1, 1) * &d(2, 0));
            let exact = qd(1, 0) * qd(2, 1) - qd(1, 1) * qd(2, 0);
            check(&e, &exact, "orient2d");
            let g = geogram_predicates::orient_2d(&p[0], &p[1], &p[2]);
            assert_eq!(e.sign(), sign_of(g), "{p:?}");

            let p: [[f64; 3]; 4] = points(&mut rng);
            let d = |i: usize, k: usize| Exp::of_sum(p[i][k], -p[0][k]);
            let qd = |i: usize, k: usize| q(p[i][k]) - q(p[0][k]);
            let minor = |i: usize, j: usize, k: usize, l: usize| {
                &(&d(i, k) * &d(j, l)) - &(&d(i, l) * &d(j, k))
            };
            let qminor =
                |i: usize, j: usize, k: usize, l: usize| qd(i, k) * qd(j, l) - qd(i, l) * qd(j, k);
            let e = &(&(&d(1, 0) * &minor(2, 3, 1, 2)) - &(&d(2, 0) * &minor(1, 3, 1, 2)))
                + &(&d(3, 0) * &minor(1, 2, 1, 2));
            let exact = qd(1, 0) * qminor(2, 3, 1, 2) - qd(2, 0) * qminor(1, 3, 1, 2)
                + qd(3, 0) * qminor(1, 2, 1, 2);
            check(&e, &exact, "orient3d");
            let g = geogram_orient3d_exact(&p);
            assert_eq!(e.sign(), g, "{p:?}");
        }
    }
}

/// Whether every component of `e` is within `2^±480`: the products of
/// two such stay in the range where products are exact.
fn halfway(e: &Exp) -> bool {
    e.components()
        .iter()
        .all(|&x| (-480..480).contains(&bit_span(x).1))
}

/// Long chains of sums, differences, scalings, products and
/// cancellations to zero stay exact and nonoverlapping: the operations'
/// outputs are valid inputs again.
/// Inputs of a few bits or all bits set, across a wide exponent range,
/// make ties and adjacent components common.
#[test]
fn chains_stay_exact_and_nonoverlapping() {
    for seed in varde_testing::seeds(64, 4096) {
        let mut rng = Rng::new(seed);
        let mut pool: Vec<(Exp, Q)> = vec![(Exp::zero(), q(0.0))];
        for step in 0..400 {
            let a = rng.input();
            let b = if rng.below(4) == 0 { -a } else { rng.input() };
            let (i, j) = (
                rng.below(pool.len() as u64) as usize,
                rng.below(pool.len() as u64) as usize,
            );
            let (e, ex) = match rng.below(8) {
                0 => (Exp::of_sum(a, b), q(a) + q(b)),
                1 => (Exp::of_product(a, b), q(a) * q(b)),
                2 => (&pool[i].0 + &pool[j].0, &pool[i].1 + &pool[j].1),
                3 => (&pool[i].0 - &pool[j].0, &pool[i].1 - &pool[j].1),
                4 if pool[i].0.components().len() < 64 => {
                    let b = rng.float(-8, 8);
                    (&pool[i].0 * b, &pool[i].1 * q(b))
                }
                5 if pool[i].0.components().len() * pool[j].0.components().len() < 64
                    && halfway(&pool[i].0)
                    && halfway(&pool[j].0) =>
                {
                    (&pool[i].0 * &pool[j].0, &pool[i].1 * &pool[j].1)
                }
                6 => {
                    // `e − (e − f) − f`, zero through a cancellation.
                    let d = &pool[i].0 - &pool[j].0;
                    (&(&pool[i].0 - &d) - &pool[j].0, q(0.0))
                }
                _ => (-&pool[i].0, -pool[i].1.clone()),
            };
            check(&e, &ex, &format!("seed {seed} step {step}"));
            assert_eq!(e.sign(), ex.partial_cmp(&q(0.0)).expect("ordered"));
            pool.push((e, ex));
        }
    }
}

/// A sum with a tie inside: `2⁻¹²²` added to a larger component is a
/// tie, rounded to even, and the next step cancels exactly. Exact and
/// nonoverlapping (the fast expansion sum gave adjacent components here,
/// against Shewchuk's Theorem 13).
#[test]
fn sum_after_a_tie_is_nonoverlapping() {
    let tiny = f64::from_bits((1023 - 122) << 52);
    let e = Exp::from_f64(-3.114_074_360_535_194_8e-21);
    let f = &Exp::from_f64(tiny) + &Exp::from_f64(3.056_167_264_320_221_3e-21);
    assert_eq!(f.components().len(), 2);
    for h in [&e + &f, &f + &e] {
        check(&h, &(value(&e) + value(&f)), "tie");
        assert_eq!(h.sign(), Ordering::Less);
    }
}

/// A random nonoverlapping expansion of up to five components of a few
/// bits (or 53), with gaps of zero to two bits between them, so adjacent
/// components and ties are common.
fn tight(rng: &mut Rng) -> (Exp, Q) {
    let mut c = Vec::new();
    let mut lo = rng.below(10) as i32 - 60;
    for _ in 0..=rng.below(5) {
        let w = if rng.below(3) == 0 {
            53
        } else {
            1 + rng.below(6) as u32
        };
        // The top and bottom bits set, the others random.
        let inner = if w > 2 {
            rng.0.next_u64() >> (66 - w)
        } else {
            0
        };
        let m = if w == 1 {
            1
        } else {
            1 << (w - 1) | inner << 1 | 1
        };
        let x = m as f64 * f64::from_bits(((lo + 1023) as u64) << 52);
        c.push(if rng.below(2) == 0 { x } else { -x });
        lo += w as i32 + rng.below(3) as i32;
    }
    let exact = c.iter().fold(q(0.0), |s, &x| s + q(x));
    let e = Exp(c.into_iter().collect());
    check(&e, &exact, "tight operand");
    (e, exact)
}

/// Sums and differences of expansions with adjacent components (not
/// strongly nonoverlapping) are exact and nonoverlapping.
#[test]
fn sums_of_tight_expansions_are_nonoverlapping() {
    for seed in varde_testing::seeds(16, 1024) {
        let mut rng = Rng::new(seed);
        for _ in 0..2000 {
            let (e, ex) = tight(&mut rng);
            let (f, fx) = tight(&mut rng);
            check(&(&e + &f), &(&ex + &fx), &format!("seed {seed} sum"));
            check(&(&e - &f), &(&ex - &fx), &format!("seed {seed} difference"));
        }
    }
}

/// A random nonoverlapping expansion of up to seven components, its
/// lowest bit near `2^lo` and every bit below `2^top`: components of one
/// to six bits, 52 or 53, or all bits set, with gaps of zero to two bits,
/// so ties, adjacent components and full significands are common.
fn spread(rng: &mut Rng, lo: i32, top: i32) -> (Exp, Q) {
    let mut c = Vec::new();
    let mut lo = lo + rng.below(20) as i32;
    for _ in 0..=rng.below(7) {
        let w = match rng.below(4) {
            0 => 53,
            1 => 52 + rng.below(2) as u32,
            _ => 1 + rng.below(6) as u32,
        };
        if lo + w as i32 > top {
            break;
        }
        let m = if w == 1 {
            1
        } else if rng.below(3) == 0 {
            (1 << w) - 1
        } else {
            // The top and bottom bits set, the others random.
            let inner = if w > 2 {
                rng.0.next_u64() >> (66 - w)
            } else {
                0
            };
            1 << (w - 1) | inner << 1 | 1
        };
        // `m · 2^lo`, in two exact steps for a subnormal `2^lo`.
        let x = m as f64
            * f64::from_bits(((lo.max(-1022) + 1023) as u64) << 52)
            * f64::from_bits(((lo.min(-1022) + 2045) as u64) << 52);
        c.push(if rng.below(2) == 0 { x } else { -x });
        lo += w as i32 + rng.below(3) as i32;
    }
    let exact = c.iter().fold(q(0.0), |s, &x| s + q(x));
    let e = Exp(c.into_iter().collect());
    check(&e, &exact, "spread operand");
    (e, exact)
}

/// Sums and differences of expansions near the limits: components from
/// the subnormals up to just below `2¹⁰²¹`, the documented bound (above
/// it a partial sum can overflow although the sum doesn't), and across
/// the whole range; some operands cancel most of the other.
#[test]
fn expansion_sums_are_exact_near_the_limits() {
    for seed in varde_testing::seeds(32, 1024) {
        let mut rng = Rng::new(seed);
        for _ in 0..2000 {
            let (lo, top) = match rng.below(4) {
                0 => (-1074, -900),
                1 => (900, 1021),
                2 => (-1074, 1021),
                _ => (-60, 60),
            };
            let (e, ex) = spread(&mut rng, lo, top);
            let (f, fx) = if rng.below(4) == 0 {
                let (g, gx) = spread(&mut rng, lo, top);
                (&g - &e, gx - &ex)
            } else {
                spread(&mut rng, lo, top)
            };
            for (h, exact, what) in [
                (&e + &f, &ex + &fx, "sum"),
                (&e - &f, &ex - &fx, "difference"),
                (&f - &e, &fx - &ex, "reversed difference"),
            ] {
                check(&h, &exact, &format!("seed {seed} {what}"));
                assert_eq!(h.sign(), exact.partial_cmp(&q(0.0)).expect("ordered"));
            }
        }
    }
}

/// A float of one bit, all bits, three leading bits or random bits, its
/// exponent in `lo..=hi` (normal).
fn shaped(rng: &mut Rng, lo: i64, hi: i64) -> f64 {
    let e = lo + rng.below((hi - lo + 1) as u64) as i64;
    let frac = match rng.below(4) {
        0 => 0,
        1 => (1 << 52) - 1,
        2 => (rng.0.next_u64() >> 61) << 49,
        _ => rng.0.next_u64() >> 12,
    };
    let x = f64::from_bits(((e + 1023) as u64) << 52 | frac);
    if rng.below(2) == 0 { x } else { -x }
}

/// One value reached by different groupings has different expansions,
/// and their differences cancel to the empty expansion: products of three
/// floats in every order, `(s + t)(s − t)` against `s² − t²`, sums
/// regrouped; small, large and moderate exponents.
#[test]
fn regroupings_cancel_to_zero() {
    for seed in varde_testing::seeds(32, 1024) {
        let mut rng = Rng::new(seed);
        for _ in 0..1000 {
            let (lo, hi) = match rng.below(3) {
                0 => (-250, -150),
                1 => (150, 320),
                _ => (-200, 200),
            };
            let [a, b, c, d] = [(); 4].map(|()| shaped(&mut rng, lo, hi));
            let abc = &Exp::of_product(a, b) * c;
            let bca = &Exp::of_product(b, c) * a;
            let acb = &Exp::of_product(a, c) * b;
            check(&abc, &(q(a) * q(b) * q(c)), "abc");
            assert!((&abc - &bca).is_zero(), "{a:e} {b:e} {c:e}");
            assert!((&(&abc - &acb) + &(&acb - &bca)).is_zero());
            let s = Exp::of_sum(a, b);
            let t = Exp::of_sum(c, d);
            let u = &(&s + &t) * &(&s - &t);
            let v = &(&s * &s) - &(&t * &t);
            let exact = (q(a) + q(b) + q(c) + q(d)) * (q(a) + q(b) - q(c) - q(d));
            check(&u, &exact, "(s + t)(s - t)");
            check(&v, &exact, "s² - t²");
            assert!((&u - &v).is_zero());
            assert!((&(&(&s + &t) + &abc) - &(&abc + &(&t + &s))).is_zero());
        }
    }
}

/// Scalings across the documented range: expansions of up to four
/// components spanning up to 150 bits, anywhere from `2⁻¹⁰²²` to `2⁹⁹⁶`,
/// scaled by a `b` that keeps every component's product in range.
#[test]
fn scalings_are_exact_across_the_range() {
    let mut rng = Rng::new(0x5CA1E);
    let mut n = 0;
    for _ in 0..varde_testing::pick(50_000, 1_000_000) {
        let lo = rng.below(1022 + 996) as i64 - 1022;
        let hi = (lo + rng.below(150) as i64).min(995);
        let (mut e, mut ex) = (Exp::zero(), q(0.0));
        for _ in 0..=rng.below(4) {
            let x = shaped(&mut rng, lo, hi);
            e = &e + &Exp::from_f64(x);
            ex += q(x);
        }
        let Some((&small, _)) = e.components().split_first() else {
            continue;
        };
        let large = *e.components().last().expect("nonempty");
        // Components below `2⁹⁹⁶`, `e_c + e_b ≥ −970` for every component
        // `c`, and products below `2¹⁰²¹`.
        let blo = (-970 - bit_span(small).1).max(-1022);
        let bhi = (1019 - bit_span(large).1).min(995);
        if bit_span(large).1 >= 996 || blo > bhi {
            continue;
        }
        let b = shaped(&mut rng, blo, bhi);
        check(
            &(&e * b),
            &(&ex * q(b)),
            &format!("{:?} · {b:e}", e.components()),
        );
        n += 1;
    }
    assert!(n > varde_testing::pick(20_000, 400_000), "{n} scalings");
}

/// Zeros: either sign of zero is the empty expansion, as is any result
/// that cancels; scaling by zero or by a signed zero gives zero.
#[test]
fn zeros_are_empty() {
    assert!(Exp::from_f64(0.0).is_zero());
    assert!(Exp::from_f64(-0.0).is_zero());
    assert!(Exp::of_sum(0.0, -0.0).is_zero());
    assert!(Exp::of_product(-0.0, 3.0).is_zero());
    assert_eq!(Exp::zero().sign(), Ordering::Equal);
    let e = &Exp::of_product(0.1, 0.3) + &Exp::of_sum(1e-30, 1.0);
    assert!((&e - &e).is_zero());
    assert!((&e * 0.0).is_zero());
    assert!((&e * -0.0).is_zero());
    assert!((&Exp::zero() * 3.0).is_zero());
    assert_eq!((&Exp::zero() + &e).components(), e.components());
    assert_eq!((&e + &Exp::zero()).components(), e.components());
    assert_eq!((&Exp::zero() - &e).components(), (-e.clone()).components());
    assert!((&e * &Exp::zero()).is_zero());
}

/// `two_product` is exact across the range the module documents:
/// `|a|, |b| < 2⁹⁹⁶`, a finite product, `e_a + e_b ≥ −970`, so down to
/// tails in the subnormal range; `split` recombines exactly there.
#[test]
fn products_are_exact_across_the_range() {
    let mut rng = Rng::new(0x9A9E);
    for _ in 0..varde_testing::pick(100_000, 2_000_000) {
        let ea = rng.below(1022 + 996) as i64 - 1022;
        let lo = (-970 - ea).max(-1022);
        let hi = (1021 - ea).min(995);
        let a = rng.float(ea, ea);
        let b = rng.float(lo, hi);
        let (x, y) = two_product(a, b);
        assert!(q(x) + q(y) == q(a) * q(b), "{a:e} · {b:e}: {x:e} + {y:e}");
        assert_eq!(x, a * b);
        let (h, l) = split(a);
        assert!(q(h) + q(l) == q(a), "split {a:e}: {h:e} + {l:e}");
        check(&Exp::of_product(a, b), &(q(a) * q(b)), "of_product");
    }
}

/// `two_sum` is exact for any `a`, `b` below `2¹⁰²³` in magnitude whose
/// sum doesn't overflow, subnormals included, and `fast_two_sum` for
/// `|a| ≥ |b|`: random bits across the whole range, sums near the top and
/// near cancellations there.
#[test]
fn sums_are_exact_across_the_range() {
    let top = f64::from_bits((1023 + 1023) << 52);
    let mut rng = Rng::new(0x5A5E);
    let mut n = 0;
    for _ in 0..varde_testing::pick(200_000, 4_000_000) {
        let a = f64::from_bits(rng.0.next_u64() >> 1 | rng.below(2) << 63);
        let b = match rng.below(3) {
            0 => f64::from_bits(rng.0.next_u64() >> 1 | rng.below(2) << 63),
            1 => -a + rng.float(900, 1022),
            _ => rng.float(960, 1022),
        };
        if !(a.abs() < top && b.abs() < top && (a + b).is_finite()) {
            continue;
        }
        n += 1;
        let (x, y) = two_sum(a, b);
        assert!(q(x) + q(y) == q(a) + q(b), "{a:e} + {b:e}: {x:e} + {y:e}");
        let (big, small) = if a.abs() >= b.abs() { (a, b) } else { (b, a) };
        let (fx, fy) = fast_two_sum(big, small);
        assert_eq!((fx.to_bits(), fy.to_bits()), (x.to_bits(), y.to_bits()));
    }
    assert!(n > varde_testing::pick(50_000, 1_000_000), "{n} sums");
}

/// FNV-1a over the bits of the results.
struct Hash(u64);

impl Hash {
    fn add(&mut self, x: u64) {
        for b in x.to_le_bytes() {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
        }
    }

    fn exp(&mut self, e: &Exp) {
        self.add(e.components().len() as u64);
        for x in e.components() {
            self.add(x.to_bits());
        }
    }
}

/// The hash of every operation's components over a fixed input set
/// (seeded, so independent of `VARDE_TESTS`). The same bits on every
/// platform, with or without hardware FMA (CI runs this crate's tests
/// with `-C target-feature=+fma` too, and on wasm): a change of algorithm
/// changes the hash, which then needs updating here.
#[test]
fn expansions_are_the_same_bits_everywhere() {
    let mut h = Hash(0xCBF2_9CE4_8422_2325);
    let mut rng = Rng::new(0x5EED);
    let mut acc = Exp::zero();
    for _ in 0..4000 {
        let (a, b, c) = (
            rng.float(-200, 200),
            rng.float(-200, 200),
            rng.float(-60, 60),
        );
        let p = Exp::of_product(a, b);
        let s = Exp::of_sum(a, b);
        let ps = &p * c;
        acc = &(&acc + &ps) - &s;
        if acc.components().len() > 32 {
            acc = Exp::from_f64(c);
        }
        for e in [&p, &s, &ps, &acc] {
            h.exp(e);
        }
        let (x, y) = split(a);
        h.add(x.to_bits());
        h.add(y.to_bits());
    }
    println!("hash of the results: {:#018x}", h.0);
    assert_eq!(
        h.0, 0x853c_9f2e_db6c_358f,
        "results differ from the reference bits"
    );
}

/// Nonoverlapping operands with adjacent full-significand components,
/// whose fast expansion sum (one running total) overlaps: the sum is
/// exact and nonoverlapping, and its sign is the exact one.
#[test]
fn sum_of_adjacent_full_components_is_nonoverlapping() {
    let two51 = f64::from_bits((1023 + 51) << 52);
    let e = Exp([0.999_999_999_999_992_5, two51 - 1.0, two51]
        .into_iter()
        .collect());
    let f = Exp([0.499_999_999_996_361_8, 4_503_462_188_417_023.5]
        .into_iter()
        .collect());
    check_form(&e, "e");
    check_form(&f, "f");
    for (h, exact) in [
        (&e + &f, value(&e) + value(&f)),
        (&f + &e, value(&e) + value(&f)),
        (&e - &f, value(&e) - value(&f)),
        (&(-&e) + &(-&f), -(value(&e) + value(&f))),
    ] {
        check(&h, &exact, "adjacent full components");
        assert_eq!(h.sign(), exact.partial_cmp(&q(0.0)).expect("ordered"));
    }
}
