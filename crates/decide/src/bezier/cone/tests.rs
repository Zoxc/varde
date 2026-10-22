//! Normal cones, gaps and boxes against exact rationals: the cone holds
//! every value of the form, also when its components have different
//! degrees and when only its pieces certify it; failures say what helps;
//! cones apart have no parallel directions; gaps and boxes hold the
//! exact separations.

use malachite::Rational as Q;
use varde_testkit::rng::Rng;

use super::*;
use crate::num::bounded::Bounded;
use crate::num::trig;

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn dot_q(x: &[Q; 3], y: &[Q; 3]) -> Q {
    &x[0] * &y[0] + &x[1] * &y[1] + &x[2] * &y[2]
}

/// Whether the exact `n` lies in the cone: `a·n > 0` and
/// `(a·n)² ≥ cos²·|a|²·|n|²`.
fn in_cone(c: &NormalCone, n: [Q; 3]) -> bool {
    let a = c.axis.map(q);
    let d = dot_q(&a, &n);
    d > 0u32 && &d * &d >= q(c.cos) * q(c.cos) * dot_q(&a, &a) * dot_q(&n, &n)
}

fn t(m: usize, n: usize, c: &[f64]) -> Tensor<Bounded<f64>> {
    Tensor::from_f64(m, n, c).unwrap()
}

fn exact(f: &Tensor<Bounded<f64>>) -> Tensor<Q> {
    Tensor::new(
        f.degrees().0,
        f.degrees().1,
        f.coefs().iter().map(|b| q(b.v)),
    )
    .unwrap()
}

fn holds_values(c: &NormalCone, n: &[Tensor<Bounded<f64>>; 3]) {
    let e = n.clone().map(|f| exact(&f));
    for k in 0..=16 {
        for (u, v) in [(k as f64 / 16.0, 0.0), (k as f64 / 16.0, 0.75), (0.5, 0.25)] {
            let value = e.clone().map(|f| f.eval(&q(u), &q(v)));
            assert!(in_cone(c, value), "{c:?} misses N({u}, {v})");
        }
    }
}

#[test]
fn components_of_different_degrees_are_raised_first() {
    // `N = (1, −10·u, 0)`: a constant `x` and a linear `y` in `u`. Paired
    // index by index, only the first coefficients would count.
    let cases = [
        [t(0, 0, &[1.0]), t(1, 0, &[0.0, -10.0]), t(0, 0, &[0.0])],
        // The same `u` and `v` lines, the degrees swapped between
        // components: `N = (1, −10·u, 10·v)`.
        [
            t(0, 0, &[1.0]),
            t(1, 0, &[0.0, -10.0]),
            t(0, 1, &[0.0, 10.0]),
        ],
    ];
    let mut checked = 0;
    for n in &cases {
        for axis in [Axis::Smallest, Axis::Mean] {
            let Ok(c) = NormalCone::of_forms([n], axis) else {
                continue;
            };
            checked += 1;
            holds_values(&c, n);
        }
    }
    assert!(checked > 0, "every case uncertified");
}

/// `N = (0, 0, z)` with `z(u) = (1 − u)² − 0.8·u·(1 − u) + u²` positive
/// (`0.3` at its least) but its middle coefficient negative: no cone
/// holds the coefficients, whose spread fails it (space), and the
/// halves' do.
#[test]
fn pieces_certify_where_the_whole_form_spreads_too_far() {
    let n = [
        t(2, 0, &[0.0; 3]),
        t(0, 0, &[0.0]),
        t(2, 0, &[1.0, -0.4, 1.0]),
    ];
    for axis in [Axis::Smallest, Axis::Mean] {
        assert_eq!(NormalCone::of_forms([&n], axis), Err(ConeFail::Space));
    }
    let [a, b, c] = n.each_ref().map(|f| f.split_half(Dir::U));
    let pieces = [
        [a[0].clone(), b[0].clone(), c[0].clone()],
        [a[1].clone(), b[1].clone(), c[1].clone()],
    ];
    let cone = NormalCone::of_forms(&pieces, Axis::Mean).expect("the halves certify");
    assert!(cone.cos > 0.0 && cone.sin <= 1.0, "{cone:?}");
    holds_values(&cone, &n);
}

/// A coefficient whose enclosure holds zero, or whose centre lies on the
/// axis's side while its enclosure doesn't, fails by rounding; an exact
/// zero fails as the form's own value (space).
#[test]
fn rounding_failures_ask_for_precision() {
    let wide = |v: f64, e: f64| Bounded::new(v, 0.0, e);
    let form = |c: Bounded<f64>| {
        [
            t(1, 0, &[0.0, 0.0]),
            t(1, 0, &[0.0, 0.0]),
            Tensor::new(1, 0, [Bounded::exact(1.0), c]).unwrap(),
        ]
    };
    for (c, want) in [
        (wide(1e-20, 1e-16), ConeFail::Precision),
        (wide(1e-16, 2e-16), ConeFail::Precision),
        (wide(-1.0, 1e-16), ConeFail::Space),
        (Bounded::exact(0.0), ConeFail::Space),
    ] {
        for axis in [Axis::Smallest, Axis::Mean] {
            assert_eq!(NormalCone::of_forms([&form(c)], axis), Err(want), "{c:?}");
        }
    }
    let uncertified = form(wide(1.0, f64::INFINITY));
    assert_eq!(
        NormalCone::of_forms([&uncertified], Axis::Mean),
        Err(ConeFail::Precision)
    );
}

/// The core-set search finds the exhaustive search's cone: the same
/// least dot product with every direction.
#[test]
fn the_smallest_cone_is_the_exhaustive_ones() {
    let mut rng = Rng::new(3);
    for k in 0..varde_testing::pick(64, 512) {
        let spread = [0.1, 1.0, 3.0][k % 3];
        let n = 1 + (rng.next_u64() % 40) as usize;
        let dirs: Vec<[f64; 3]> = (0..n)
            .filter_map(|_| unit([rng.range(-spread, spread), rng.range(-spread, spread), 1.0]))
            .collect();
        let least = |d: &[f64; 3]| {
            dirs.iter()
                .map(|c| dot3(c, d))
                .fold(f64::INFINITY, |m, x| if x < m { x } else { m })
        };
        let all = smallest_of(&dirs).expect("directions");
        match smallest_cone(&dirs) {
            Some(a) => assert!(
                (least(&a) - all.1).abs() <= 1e-12,
                "{} {}",
                least(&a),
                all.1
            ),
            None => assert!(all.1 <= 1e-12, "{all:?}"),
        }
    }
    // No half-space holds opposite directions.
    assert_eq!(smallest_cone(&[[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]]), None);
}

fn cone(axis: [f64; 3], half_angle: f64) -> NormalCone {
    // `cos` rounded down past its own rounding.
    let cos = down(down(trig::cos(half_angle)));
    let sin = up(up(1.0 - down(cos * cos)).sqrt());
    NormalCone {
        axis,
        cos,
        sin: if sin > 1.0 { 1.0 } else { sin },
    }
}

/// Cones are apart exactly when the angle between their axes clears the
/// sum of their half-angles both ways round; then no direction of one is
/// parallel to a direction of the other (sampled on the rims).
#[test]
fn apart_cones_have_no_parallel_directions() {
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    assert!(cone(x, 0.3).apart(&cone(y, 0.3)));
    assert!(!cone(x, 0.8).apart(&cone(y, 0.8)));
    // Opposite axes: the cones of `a` and `−b` meet.
    assert!(!cone(x, 0.1).apart(&cone([-1.0, 0.0, 0.0], 0.1)));
    assert!(!cone(x, 0.1).apart(&cone(x, 0.1)));
    let mut rng = Rng::new(9);
    let mut seen = [0u32; 2];
    for _ in 0..varde_testing::pick(256, 4096) {
        let a = rng.direction().to_array();
        let b = rng.direction().to_array();
        let (ta, tb) = (rng.range(0.0, 0.7), rng.range(0.0, 0.7));
        let (ca, cb) = (cone(a, ta), cone(b, tb));
        let apart = ca.apart(&cb);
        assert_eq!(apart, cb.apart(&ca));
        seen[usize::from(apart)] += 1;
        let phi = trig::acos(dot3(&a, &b).clamp(-1.0, 1.0));
        let reach = ta + tb;
        if apart {
            assert!(
                reach < phi && phi < std::f64::consts::PI - reach,
                "{phi} {reach}"
            );
        } else if reach + 1e-9 < phi && phi < std::f64::consts::PI - reach - 1e-9 {
            panic!("missed apart cones: {phi} {reach}");
        }
    }
    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
}

fn exact_dot(l: &[f64; 3], p: &[f64; 3]) -> Q {
    dot_q(&l.map(q), &p.map(q))
}

/// The support function and the gap hold the exact values; proposed
/// axes certify separated sets, and no gap is certified for sets whose
/// hulls meet.
#[test]
fn gaps_hold_the_exact_separation() {
    let mut rng = Rng::new(17);
    let mut certified = 0;
    for k in 0..varde_testing::pick(128, 1024) {
        let extent = [1.0, 1e3, 1e6][k % 3];
        let centre = rng.point(extent);
        let d = rng.direction();
        let size = rng.log_range(1e-3, 10.0);
        let shift = [3.0, 0.5, -1.0][k % 3] * size;
        let set = |rng: &mut Rng, at: f64| -> Vec<[f64; 3]> {
            (0..9)
                .map(|_| (centre + d * at + rng.direction() * (size * rng.unit())).to_array())
                .collect()
        };
        let (a, b) = (set(&mut rng, 0.0), set(&mut rng, 2.0 * size + shift));
        let l = rng.direction().to_array();
        let [lo, hi] = support(&a, &l);
        let h = a.iter().map(|p| exact_dot(&l, p)).max().unwrap();
        assert!(q(lo) <= h && h <= q(hi), "support");
        let g = gap(&a, &b, &l);
        let truth = b.iter().map(|p| exact_dot(&l, p)).min().unwrap() - &h;
        let [glo, ghi] = g.enclose();
        assert!(q(glo) <= truth && truth <= q(ghi), "{g:?}");
        if let Some(l) = propose_axis(&a, &b) {
            let g = gap(&a, &b, &l);
            if g.sign() == Some(std::cmp::Ordering::Greater) {
                certified += 1;
                for p in &a {
                    for r in &b {
                        assert!(exact_dot(&l, r) > exact_dot(&l, p));
                    }
                }
            }
        }
    }
    assert!(certified > 0, "no proposal certified");
    // Sets whose hulls share a point: never a positive gap.
    let a = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let b = [[0.5, 0.5, 0.0], [2.0, 2.0, 1.0], [3.0, 0.0, 0.0]];
    for l in [[1.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]] {
        assert_ne!(gap(&a, &b, &l).sign(), Some(std::cmp::Ordering::Greater));
    }
    assert_eq!(propose_axis(&a, &b), None);
    assert_eq!(propose_axis(&[], &b), None);
}

/// An oriented box holds its points exactly; its gap to a set beyond one
/// of its slabs is certified and at most the exact separation there.
#[test]
fn oriented_boxes_hold_their_points() {
    let mut rng = Rng::new(23);
    let mut separated = 0;
    for k in 0..varde_testing::pick(128, 1024) {
        let n = rng.direction();
        let origin = rng.point([1.0, 1e3, 1e6][k % 3]);
        let flat: Vec<[f64; 3]> = (0..9)
            .map(|_| {
                let p = rng.direction();
                let p = p - n * p.dot(n);
                (origin + p * 5.0 + n * rng.range(-1e-3, 1e-3)).to_array()
            })
            .collect();
        let obb = Obb::of_points(&flat, &n.to_array()).expect("a box");
        for p in &flat {
            for k in 0..3 {
                let d = exact_dot(&obb.axes[k], p);
                assert!(q(obb.lo[k]) <= d && d <= q(obb.hi[k]));
            }
        }
        // Thin along the normal.
        assert!(
            obb.hi[0] - obb.lo[0] < 3e-3 + 1e-9 * origin.length(),
            "{obb:?}"
        );
        let off = [0.5, 2.0][k % 2];
        let other: Vec<[f64; 3]> = flat
            .iter()
            .map(|p| add3(p, &(n * off).to_array()))
            .collect();
        let g = obb.gap(&other);
        if g > 0.0 {
            separated += 1;
            // Every point of `other` is at least `g` beyond a slab: along
            // the normal, the least `n·p` over `other` passes `hi`.
            let a = &obb.axes[0];
            let len2 = dot_q(&a.map(q), &a.map(q));
            let least = other.iter().map(|p| exact_dot(a, p)).min().unwrap();
            let sep = least - q(obb.hi[0]);
            assert!(&sep * &sep >= q(g) * q(g) * len2 && sep > 0u32);
        }
    }
    assert!(separated > 0);
    assert_eq!(Obb::of_points(&[], &[0.0, 0.0, 1.0]), None);
    assert_eq!(Obb::of_points(&[[0.0; 3]], &[0.0; 3]), None);
}

fn add3(p: &[f64; 3], d: &[f64; 3]) -> [f64; 3] {
    [p[0] + d[0], p[1] + d[1], p[2] + d[2]]
}
