//! Keys against exact values: normalised and rounded once, the same bits
//! from every construction of one form, ordered by their bits; implicits
//! zero on their forms.

use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;
use malachite::{Float, Rational as Q};
use varde_testkit::rng::Rng;

use super::*;

fn bits(k: &FormKey) -> Vec<u64> {
    k.parts().1.iter().map(|x| x.to_bits()).collect()
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
    let mut keys = vec![s, p(2.0), c, p(-1.0), p(1.0)];
    keys.sort();
    assert_eq!(keys, vec![p(-1.0), p(1.0), p(2.0), c, s]);
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
    assert_eq!(k.implicit(&[1.0, 2.0, 7.0].map(q)), Q::from(0u32));
    assert!(k.implicit(&[1.0, 2.0, 8.0].map(q)) > 0u32);
    let k = FormKey::from(CylinderKey::new([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2.0).unwrap());
    assert_eq!(k.implicit(&[3.0, 0.0, 100.0].map(q)), Q::from(0u32));
    assert!(k.implicit(&[2.0, 0.0, -3.0].map(q)) < 0u32);
    let k = FormKey::from(SphereKey::new([1.0, 1.0, 1.0], 3.0, [1.0, 0.0, 0.0]).unwrap());
    assert_eq!(k.implicit(&[1.0, 4.0, 1.0].map(q)), Q::from(0u32));
    assert!(k.implicit(&[1.0, 1.0, 1.0].map(q)) < 0u32);
    assert_eq!(k.implicit_degree(), 2);
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
            let d = match a {
                FormKey::Plane(k) => k.normal(),
                FormKey::Cylinder(k) => k.axis(),
                FormKey::Sphere(k) => k.axis(),
            };
            assert!(d.into_iter().find(|x| *x != 0.0).unwrap() > 0.0, "{a:?}");
        }
    }
}
