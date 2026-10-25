//! Keys against exact values: normalised and rounded once, the same bits
//! from every construction of one form, ordered by their bits; implicits
//! zero on their forms.

use malachite::base::num::basic::traits::One;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;
use malachite::{Float, Rational as Q};
use varde_testkit::rng::Rng;

use super::*;
use crate::geom::formkey::RouteCounts;
use crate::geom::round::{nearest, nearest_root};

fn bits(k: &FormKey) -> Vec<u64> {
    k.values().iter().map(|x| x.to_bits()).collect()
}

/// `a/√s` rounded through a 300-bit root: an independent reference.
fn reference(a: f64, s: &Q) -> f64 {
    let a = q(a);
    let (f, _) = Float::sqrt_rational_prec_round_ref(&(&a * &a / s), 300, Nearest);
    let m = f64::rounding_from(&f, Nearest).0;
    if a < 0u32 { -m } else { m }
}

#[test]
fn directions_are_unit_and_signed() {
    let k = PlaneKey::new([-3.0, -4.0, 0.0], [0.0, 0.0, 5.0]).unwrap();
    assert_eq!(k.normal(), [0.6, 0.8, 0.0]);
    assert_eq!(k.offset(), 0.0);
    let k = PlaneKey::new([0.0, 0.0, -2.0], [1.0, 2.0, 3.0]).unwrap();
    assert_eq!((k.normal(), k.offset()), ([0.0, 0.0, 1.0], 3.0));
    let k = CylinderKey::new([3.0, 4.0, 7.0], [0.0, 0.0, -0.5], 2.0).unwrap();
    assert_eq!((k.axis(), k.foot()), ([0.0, 0.0, 1.0], [3.0, 4.0, 0.0]));
    let k = SphereKey::new([1.0, 2.0, 3.0], 4.0, [0.0, -1.0, 1.0]).unwrap();
    let h = std::f64::consts::FRAC_1_SQRT_2;
    assert_eq!(k.axis(), [0.0, h, -h]);
}

#[test]
fn parameters_are_rounded_once() {
    let mut rng = Rng::new(3);
    for _ in 0..varde_testing::pick(300, 5_000) {
        let n = rng.direction() * rng.log_range(1e-3, 1e3);
        let p = rng.point(1e6);
        let k = PlaneKey::new(n.to_array(), p.to_array()).unwrap();
        let sign = if n.to_array().into_iter().find(|x| *x != 0.0).unwrap() < 0.0 {
            -1.0
        } else {
            1.0
        };
        let nq = n.to_array().map(|x| q(sign * x));
        let s = dot(&nq, &nq);
        for c in 0..3 {
            assert_eq!(k.normal()[c], reference(sign * n[c], &s));
        }
        let d = nearest(&dot(&nq, &p.to_array().map(q)));
        // The offset's numerator `n·p` is exact; round it through the
        // reference only where it is an f64 already.
        if q(d) == dot(&nq, &p.to_array().map(q)) {
            assert_eq!(k.offset(), reference(d, &s));
        }
        // The foot point is the exact foot, rounded.
        let a = rng.direction();
        let c = CylinderKey::new(p.to_array(), a.to_array(), 1.0).unwrap();
        let (pq, aq) = (p.to_array().map(q), a.to_array().map(q));
        let t = dot(&pq, &aq) / dot(&aq, &aq);
        for i in 0..3 {
            assert_eq!(c.foot()[i], nearest(&(&pq[i] - &t * &aq[i])));
        }
    }
}

#[test]
fn constructions_of_one_form_agree() {
    // A plane by a scaled, reversed normal and another point on it.
    let a = PlaneKey::new([1.0, 2.0, 3.0], [10.0, 20.0, -5.0]).unwrap();
    let b = PlaneKey::new([-4.0, -8.0, -12.0], [31.0, 20.0, -12.0]).unwrap();
    assert_eq!(bits(&a.into()), bits(&b.into()));
    assert_eq!(FormKey::from(a), FormKey::from(b));
    // A cylinder by another point on the axis and a scaled direction.
    let a = CylinderKey::new([1.0, -2.0, 0.25], [1.0, 2.0, 2.0], 3.0).unwrap();
    let b = CylinderKey::new([6.0, 8.0, 10.25], [-0.5, -1.0, -1.0], 3.0).unwrap();
    assert_eq!(bits(&a.into()), bits(&b.into()));
    let a = SphereKey::new([1.0, 2.0, 3.0], 4.0, [3.0, 0.0, 4.0]).unwrap();
    let b = SphereKey::new([1.0, 2.0, 3.0], 4.0, [-0.75, 0.0, -1.0]).unwrap();
    assert_eq!(FormKey::from(a), FormKey::from(b));
    assert_eq!(a.axis(), [0.6, 0.0, 0.8]);
}

/// A plane through three points is their exact plane rounded once: any
/// three of its points, in either turn, give the same key, the key of
/// its exact normal.
#[test]
fn a_plane_through_three_points_is_their_exact_plane() {
    let a = PlaneKey::through([[0.0, 0.0, 5.0], [1.0, 0.0, 5.0], [0.0, 1.0, 5.0]]).unwrap();
    let b = PlaneKey::new([0.0, 0.0, 1.0], [0.0, 0.0, 5.0]).unwrap();
    assert_eq!(bits(&a.into()), bits(&b.into()));
    // On x + 2y + 3z = 6.
    let a = PlaneKey::through([[6.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 2.0]]).unwrap();
    let b = PlaneKey::through([[1.0, 1.0, 1.0], [0.0, 0.0, 2.0], [4.0, 1.0, 0.0]]).unwrap();
    let c = PlaneKey::new([1.0, 2.0, 3.0], [6.0, 0.0, 0.0]).unwrap();
    assert_eq!(bits(&a.into()), bits(&b.into()));
    assert_eq!(bits(&a.into()), bits(&c.into()));
    assert_eq!(
        PlaneKey::through([[0.0; 3], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]]).unwrap_err(),
        FormError::ZeroDirection
    );
}

#[test]
fn zeros_and_signs_are_canonical_after_rounding() {
    // A sphere given a −0 centre component is the sphere at +0.
    let a = SphereKey::new([-0.0, 1.0, -0.0], 2.0, [0.0, 0.0, 1.0]).unwrap();
    let b = SphereKey::new([0.0, 1.0, 0.0], 2.0, [0.0, 0.0, 1.0]).unwrap();
    assert_eq!(bits(&a.into()), bits(&b.into()));
    // A component rounding to zero is +0, not −0.
    let a = PlaneKey::new([4.0, -5e-324, 0.0], [1.0, 0.0, 0.0]).unwrap();
    let b = PlaneKey::new([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]).unwrap();
    assert_eq!(bits(&a.into()), bits(&b.into()));
    // A leading component rounding to zero leaves the stored normal's
    // first non-zero component negative: the stored plane is negated.
    let a = PlaneKey::new([5e-324, -4.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
    assert_eq!((a.normal(), a.offset()), ([0.0, 1.0, 0.0], 1.0));
    let b = PlaneKey::new([0.0, 1.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
    assert_eq!(bits(&a.into()), bits(&b.into()));
    let c = CylinderKey::new([0.0; 3], [5e-324, -4.0, 0.0], 1.0).unwrap();
    assert_eq!(
        c.axis().map(f64::to_bits),
        [0.0, 1.0, 0.0].map(f64::to_bits)
    );
}

#[test]
fn bad_inputs_are_refused() {
    let nan = f64::NAN;
    assert_eq!(
        PlaneKey::new([0.0; 3], [1.0; 3]).unwrap_err(),
        FormError::ZeroDirection
    );
    assert_eq!(
        PlaneKey::new([1.0, 0.0, 0.0], [nan, 0.0, 0.0]).unwrap_err(),
        FormError::NotFinite
    );
    assert_eq!(
        CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], 0.0).unwrap_err(),
        FormError::Radius
    );
    assert_eq!(
        CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], -1.0).unwrap_err(),
        FormError::Radius
    );
    assert_eq!(
        SphereKey::new([0.0; 3], f64::INFINITY, [0.0, 0.0, 1.0]).unwrap_err(),
        FormError::NotFinite
    );
    assert_eq!(
        SphereKey::new([0.0; 3], 1.0, [0.0, f64::INFINITY, 1.0]).unwrap_err(),
        FormError::NotFinite
    );
    // A foot point past the f64 range.
    assert_eq!(
        CylinderKey::new([f64::MAX, -f64::MAX, 0.0], [1.0, 2.0, 0.0], 1.0).unwrap_err(),
        FormError::Range
    );
}

#[test]
fn keys_order_by_kind_then_bits() {
    let p = |d: f64| FormKey::from(PlaneKey::new([0.0, 0.0, 1.0], [0.0, 0.0, d]).unwrap());
    let c = FormKey::from(CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], 1.0).unwrap());
    let s = FormKey::from(SphereKey::new([0.0; 3], 1.0, [0.0, 0.0, 1.0]).unwrap());
    let mut keys = vec![s.clone(), p(2.0), c.clone(), p(-1.0), p(1.0)];
    keys.sort();
    assert_eq!(keys, vec![p(-1.0), p(1.0), p(2.0), c, s.clone()]);
    assert!(p(1.0) != p(1.0f64.next_up()));
    assert!(p(1.0) < p(1.0f64.next_up()));
    // Total on the bits: a −0 component orders before +0.
    let mut a = SphereKey::new([0.0; 3], 1.0, [0.0, 0.0, 1.0]).unwrap();
    a.centre[0] = -0.0;
    assert!(FormKey::from(a) < s);
    assert_ne!(FormKey::from(a), s);
}

#[test]
fn implicits_vanish_on_their_forms() {
    let k = FormKey::from(PlaneKey::new([0.0, 0.0, 1.0], [5.0, 6.0, 7.0]).unwrap());
    assert_eq!(k.implicit(&[1.0, 2.0, 7.0].map(q)), Some(Q::from(0u32)));
    assert!(k.implicit(&[1.0, 2.0, 8.0].map(q)).unwrap() > 0u32);
    let k = FormKey::from(CylinderKey::new([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2.0).unwrap());
    assert_eq!(k.implicit(&[3.0, 0.0, 100.0].map(q)), Some(Q::from(0u32)));
    assert!(k.implicit(&[2.0, 0.0, -3.0].map(q)).unwrap() < 0u32);
    let k = FormKey::from(SphereKey::new([1.0, 1.0, 1.0], 3.0, [1.0, 0.0, 0.0]).unwrap());
    assert_eq!(k.implicit(&[1.0, 4.0, 1.0].map(q)), Some(Q::from(0u32)));
    assert!(k.implicit(&[1.0, 1.0, 1.0].map(q)).unwrap() < 0u32);
    assert_eq!(k.implicit_degree(), Some(2));
}

#[test]
fn random_constructions_of_one_form_agree() {
    // Small integer normals and axes, other points on the exact form,
    // directions scaled (to subnormal and huge) and reversed.
    let mut rng = Rng::new(31);
    let int = |rng: &mut Rng, m: u64| (rng.next_u64() % (2 * m + 1)) as f64 - m as f64;
    let scales = [-3.0, 0.5, -1e-200, f64::from_bits(1 << 14), 1e250, -7.0];
    for _ in 0..varde_testing::pick(300, 5_000) {
        let n = [0; 3].map(|_| int(&mut rng, 5));
        if n == [0.0; 3] {
            continue;
        }
        let p = [0; 3].map(|_| int(&mut rng, 1000));
        let s = scales[(rng.next_u64() % scales.len() as u64) as usize];
        let n2 = n.map(|x| x * s);
        // p + n × w lies on the plane, p + t·n on the axis.
        let w = [0; 3].map(|_| int(&mut rng, 7));
        let c = [
            n[1] * w[2] - n[2] * w[1],
            n[2] * w[0] - n[0] * w[2],
            n[0] * w[1] - n[1] * w[0],
        ];
        let t = int(&mut rng, 100);
        let on_plane = [0, 1, 2].map(|k| p[k] + c[k]);
        let on_axis = [0, 1, 2].map(|k| p[k] + t * n[k]);
        let pairs: [(FormKey, FormKey); 3] = [
            (
                PlaneKey::new(n, p).unwrap().into(),
                PlaneKey::new(n2, on_plane).unwrap().into(),
            ),
            (
                CylinderKey::new(p, n, 2.5).unwrap().into(),
                CylinderKey::new(on_axis, n2, 2.5).unwrap().into(),
            ),
            (
                SphereKey::new(p, 3.0, n).unwrap().into(),
                SphereKey::new(p, 3.0, n2).unwrap().into(),
            ),
        ];
        for (a, b) in pairs {
            assert_eq!(bits(&a), bits(&b), "{n:?} {p:?} {s}");
            // +0 zeros, and the sign rule on the stored direction.
            assert!(bits(&a).iter().all(|x| *x != (-0.0f64).to_bits()));
            let d = match &a {
                FormKey::Plane(k) => k.normal(),
                FormKey::Cylinder(k) => k.axis(),
                FormKey::Sphere(k) => k.axis(),
                _ => unreachable!("a plane, cylinder or sphere"),
            };
            assert!(d.into_iter().find(|x| *x != 0.0).unwrap() > 0.0, "{a:?}");
        }
    }
}

/// `f` with an uncharged meter, and the routes it took.
fn metered<K>(
    f: impl FnOnce(&mut KeyMeter<'_, Uncharged>) -> Result<K, KeyError<Infallible>>,
) -> (K, RouteCounts) {
    let mut free = Uncharged;
    let mut m = KeyMeter::new(&mut free);
    let k = f(&mut m).unwrap_or_else(|e| panic!("{e:?}"));
    (k, m.counts())
}

/// Pythagorean axes: integer directions of integer length.
const AXES: [([f64; 3], f64); 6] = [
    ([1.0, 2.0, 2.0], 3.0),
    ([2.0, -3.0, 6.0], 7.0),
    ([-4.0, 4.0, 7.0], 9.0),
    ([0.0, 3.0, -4.0], 5.0),
    ([0.0, 0.0, 1.0], 1.0),
    ([2.0, 6.0, 9.0], 11.0),
];

#[test]
fn every_key_kind_rounds_without_the_fallback() {
    // Random inputs of every kind built so far: each parameter takes the
    // rational or the quadratic-irrational route, never the fallback.
    let mut rng = Rng::new(17);
    let mut all = RouteCounts::default();
    let mut add = |c: RouteCounts| {
        all.rational += c.rational;
        all.exact += c.exact;
        all.fallback += c.fallback;
        for (a, b) in all.levels.iter_mut().zip(c.levels) {
            *a += b;
        }
    };
    for _ in 0..varde_testing::pick(100, 1_000) {
        let d = (rng.direction() * rng.log_range(1e-3, 1e3)).to_array();
        let p = rng.point(1e6).to_array();
        let (r, s) = (rng.log_range(1e-3, 1e3), rng.log_range(1e-3, 1e3));
        let span = |rng: &mut Rng| {
            let mut c = [[0.0; 4]; 3];
            for (k, row) in c.iter_mut().enumerate() {
                let x = rng.point(1e3).to_array();
                *row = [x[0], x[1], x[2], if k == 1 { 0.75 } else { 1.0 }];
            }
            c
        };
        let a = span(&mut rng);
        let mut b = span(&mut rng);
        b[0] = a[2];
        let meridian = [[r, 1.0, 1.0], [r + s, 2.0, 0.5], [s, 3.0, 1.0]];
        add(metered(|m| PlaneKey::metered(d, p, m)).1);
        add(metered(|m| CylinderKey::metered(p, d, r, m)).1);
        add(metered(|m| ConicCylinderKey::metered(d, &[a, b], m)).1);
        add(metered(|m| ConeKey::metered(p, d, r, s, m)).1);
        add(metered(|m| SphereKey::metered(p, r, d, m)).1);
        add(metered(|m| TorusKey::metered(p, d, r + s, s, m)).1);
        add(metered(|m| RevolvedKey::metered(p, d, &[meridian], m)).1);
    }
    assert_eq!(all.fallback, 0, "{all:?}");
    assert!(all.rational > 0 && all.levels[1] > 0, "{all:?}");
}

#[test]
fn new_kinds_hold_their_exact_values_rounded() {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    // A cone opening down `−z` at 45°, and one at `atan(3/4)`.
    let k = ConeKey::new([1.0, 2.0, 3.0], [0.0, 0.0, -2.0], 5.0, 5.0).unwrap();
    assert_eq!((k.apex(), k.axis()), ([1.0, 2.0, 3.0], [0.0, 0.0, -1.0]));
    assert_eq!((k.cos(), k.sin()), (h, h));
    let k = ConeKey::new([0.0; 3], [1.0, 1.0, 0.0], 4e-200, 3e-200).unwrap();
    assert_eq!((k.cos(), k.sin()), (0.8, 0.6));
    assert_eq!(k.axis(), [h, h, 0.0]);
    // A torus by a reversed axis.
    let k = TorusKey::new([-0.0, 1.0, 2.0], [0.0, -3.0, -4.0], 5.0, 1.0).unwrap();
    assert_eq!(k.axis(), [0.0, 0.6, 0.8]);
    assert_eq!(
        k.centre().map(f64::to_bits),
        [0.0, 1.0, 2.0].map(f64::to_bits)
    );
    assert_eq!((k.major(), k.minor()), (5.0, 1.0));
    // A conic-chain cylinder along `z`: frame `e₁ = e_x × z = −y`,
    // `e₂ = z × e₁ = x`; the profile is the spans' `(−y, x)`, the chain
    // in its canonical direction.
    let arc = [
        [1.0, 0.0, 7.0, 1.0],
        [1.0, 1.0, 7.0, h],
        [0.0, 1.0, 7.0, 1.0],
    ];
    let k = ConicCylinderKey::new([0.0, 0.0, 3.0], &[arc]).unwrap();
    assert_eq!(k.axis(), [0.0, 0.0, 1.0]);
    assert_eq!(
        k.profile().spans(),
        &[[[-1.0, 0.0, 1.0], [-1.0, 1.0, h], [0.0, 1.0, 1.0]]]
    );
    // A revolved conic about the line through (0, 0, 5) along `−z`: `z`
    // from the foot (the origin) along `+z` is `5 − z`.
    let span = [[1.0, 0.0, 1.0], [2.0, 1.0, 0.5], [1.0, 2.0, 1.0]];
    let k = RevolvedKey::new([0.0, 0.0, 5.0], [0.0, 0.0, -1.0], &[span]).unwrap();
    assert_eq!((k.axis(), k.foot()), ([0.0, 0.0, 1.0], [0.0; 3]));
    assert_eq!(
        k.profile().spans(),
        &[[[1.0, 3.0, 1.0], [2.0, 4.0, 0.5], [1.0, 5.0, 1.0]]]
    );
    // The profile's `z` and the frame's coordinates are irrational where
    // the axis is: exact values, rounded once.
    let k = RevolvedKey::new([1.0, 0.0, 0.0], [1.0, 1.0, 0.0], &[span]).unwrap();
    let r = Q::from_signeds(1, 2);
    assert_eq!(k.profile().spans()[0][0][1], nearest_root(&Q::ONE, &r));
    assert_eq!(k.foot(), [0.5, -0.5, 0.0]);
}

#[test]
fn constructions_of_the_new_kinds_agree() {
    let mut rng = Rng::new(23);
    let int = |rng: &mut Rng, m: u64| (rng.next_u64() % (2 * m + 1)) as f64 - m as f64;
    // Scales that keep the directions exact.
    let scales = [
        -3.0,
        0.5,
        -f64::from_bits(1 << 14),
        f64::from_bits(1900 << 52),
        -7.0,
    ];
    for _ in 0..varde_testing::pick(200, 3_000) {
        let (a, len) = AXES[(rng.next_u64() % AXES.len() as u64) as usize];
        let s = scales[(rng.next_u64() % scales.len() as u64) as usize];
        let a2 = a.map(|x| x * s);
        let p = [0; 3].map(|_| int(&mut rng, 1000));
        let t = int(&mut rng, 50);
        let moved = [0, 1, 2].map(|k| p[k] + t * a[k]);
        // Cones and tori by a scaled direction and half-angle.
        let (h, w) = (1.0 + int(&mut rng, 9).abs(), 1.0 + int(&mut rng, 9).abs());
        let pos = s.abs();
        let c1 = ConeKey::new(p, a, h, w).unwrap();
        let c2 = ConeKey::new(p, a.map(|x| x * pos), h * 0.125, w * 0.125).unwrap();
        assert_eq!(FormKey::from(c1), FormKey::from(c2));
        let t1 = TorusKey::new(p, a, 7.0, 2.0).unwrap();
        let t2 = TorusKey::new(p, a2, 7.0, 2.0).unwrap();
        assert_eq!(FormKey::from(t1), FormKey::from(t2));
        // A conic-chain cylinder from its profile at another height along
        // the axis, the chain reversed, the axis scaled.
        let pt = |rng: &mut Rng| [0; 3].map(|_| int(rng, 100));
        let (x0, x1, x2, x3, x4) = (
            pt(&mut rng),
            pt(&mut rng),
            pt(&mut rng),
            pt(&mut rng),
            pt(&mut rng),
        );
        let lift = |x: [f64; 3], w: f64| [x[0] + t * a[0], x[1] + t * a[1], x[2] + t * a[2], w];
        let at = |x: [f64; 3], w: f64| [x[0], x[1], x[2], w];
        let chain = [
            [at(x0, 1.0), at(x1, 0.5), at(x2, 1.0)],
            [at(x2, 2.0), at(x3, 1.0), at(x4, 1.0)],
        ];
        let back = [
            [lift(x4, 1.0), lift(x3, 1.0), lift(x2, 2.0)],
            [lift(x2, 1.0), lift(x1, 0.5), lift(x0, 1.0)],
        ];
        let k1 = FormKey::from(ConicCylinderKey::new(a, &chain).unwrap());
        let k2 = FormKey::from(ConicCylinderKey::new(a2, &back).unwrap());
        assert_eq!(bits(&k1), bits(&k2), "{a:?} {s} {t}");
        // A revolved conic about the line through another point on the
        // axis, the axis reversed (`z` measured the other way), the chain
        // reversed.
        let m = |r: f64, z: f64, w: f64| [r, z, w];
        let z = [0; 3].map(|_| int(&mut rng, 100));
        let r = [0; 3].map(|_| int(&mut rng, 100).abs());
        let fwd = [[m(r[0], z[0], 1.0), m(r[1], z[1], 0.5), m(r[2], z[2], 1.0)]];
        let z2 = |z: f64| t * len - z;
        let rev = [[
            m(r[2], z2(z[2]), 1.0),
            m(r[1], z2(z[1]), 0.5),
            m(r[0], z2(z[0]), 1.0),
        ]];
        let k1 = FormKey::from(RevolvedKey::new(p, a, &fwd).unwrap());
        let k2 = FormKey::from(RevolvedKey::new(moved, a.map(|x| -x), &rev).unwrap());
        assert_eq!(bits(&k1), bits(&k2), "{a:?} {p:?} {t}");
        assert!(bits(&k1).iter().all(|x| *x != (-0.0f64).to_bits()));
    }
}

#[test]
fn new_kinds_refuse_bad_inputs() {
    let z = [0.0, 0.0, 1.0];
    assert_eq!(
        ConeKey::new([0.0; 3], z, 0.0, 1.0).unwrap_err(),
        FormError::Angle
    );
    assert_eq!(
        ConeKey::new([0.0; 3], z, 1.0, -1.0).unwrap_err(),
        FormError::Angle
    );
    assert_eq!(
        ConeKey::new([0.0; 3], [0.0; 3], 1.0, 1.0).unwrap_err(),
        FormError::ZeroDirection
    );
    assert_eq!(
        TorusKey::new([0.0; 3], z, 2.0, 2.0).unwrap_err(),
        FormError::Horn
    );
    assert_eq!(
        TorusKey::new([0.0; 3], z, 0.0, 2.0).unwrap_err(),
        FormError::Radius
    );
    let span = [
        [1.0, 0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
    ];
    let mut apart = span;
    apart[0][0] = 2.0;
    assert_eq!(
        ConicCylinderKey::new(z, &[]).unwrap_err(),
        FormError::Profile
    );
    assert_eq!(
        ConicCylinderKey::new(z, &[span, apart]).unwrap_err(),
        FormError::Profile
    );
    let mut light = span;
    light[1][3] = 0.0;
    assert_eq!(
        ConicCylinderKey::new(z, &[light]).unwrap_err(),
        FormError::Profile
    );
    let m = [[1.0, 0.0, 1.0], [1.0, 1.0, f64::NAN], [0.0, 1.0, 1.0]];
    assert_eq!(
        RevolvedKey::new([0.0; 3], z, &[m]).unwrap_err(),
        FormError::NotFinite
    );
    let plane = FormKey::from(PlaneKey::new(z, [0.0; 3]).unwrap());
    let id = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let flat = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]];
    assert_eq!(
        AffineKey::new(plane.clone(), flat, [0.0; 3]).unwrap_err(),
        FormError::Singular
    );
    let image = FormKey::from(AffineKey::new(plane, id, [0.0; 3]).unwrap());
    assert_eq!(
        AffineKey::new(image, id, [0.0; 3]).unwrap_err(),
        FormError::Nested
    );
}

#[test]
fn every_kind_orders_by_kind_then_bits() {
    let z = [0.0, 0.0, 1.0];
    let arc = [
        [1.0, 0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0, 0.5],
        [0.0, 1.0, 0.0, 1.0],
    ];
    let span = [[1.0, 0.0, 1.0], [2.0, 1.0, 0.5], [1.0, 2.0, 1.0]];
    let plane = FormKey::from(PlaneKey::new(z, [0.0; 3]).unwrap());
    let scale = [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let keys: Vec<FormKey> = vec![
        plane.clone(),
        CylinderKey::new([0.0; 3], z, 1.0).unwrap().into(),
        ConicCylinderKey::new(z, &[arc]).unwrap().into(),
        ConicCylinderKey::new(
            z,
            &[arc, [arc[2], [-1.0, 1.0, 0.0, 0.5], [-1.0, 0.0, 0.0, 1.0]]],
        )
        .unwrap()
        .into(),
        ConeKey::new([0.0; 3], z, 1.0, 1.0).unwrap().into(),
        SphereKey::new([0.0; 3], 1.0, z).unwrap().into(),
        TorusKey::new([0.0; 3], z, 3.0, 1.0).unwrap().into(),
        RevolvedKey::new([0.0; 3], z, &[span]).unwrap().into(),
        AffineKey::new(plane, scale, [0.0; 3]).unwrap().into(),
    ];
    let mut sorted = keys.clone();
    sorted.reverse();
    sorted.sort();
    assert_eq!(sorted, keys);
    for (i, a) in keys.iter().enumerate() {
        for (j, b) in keys.iter().enumerate() {
            assert_eq!(a == b, i == j, "{a:?} {b:?}");
        }
    }
    // An affine image orders by its map, then its source.
    let cyl = FormKey::from(CylinderKey::new([0.0; 3], z, 1.0).unwrap());
    let a = FormKey::from(AffineKey::new(keys[0].clone(), scale, [0.0; 3]).unwrap());
    let b = FormKey::from(AffineKey::new(cyl, scale, [0.0; 3]).unwrap());
    assert!(a < b);
}
