//! Fitted p-curves on spheres (oblique small circles, whole and in
//! part, at several lattice and curve levels), a cylinder (an oblique
//! ellipse wobbling across an axial tile line) and a ring torus (a
//! Villarceau circle): each certified at its target by the seam bound,
//! every span in one tile, the knots the curve's and the given end UVs
//! kept to the bit, and the certified deviation found at dense samples
//! below the bound. A circle off its sphere stops within `TIGHT` of the
//! deviation at its knots; a curve leaving the domain, or a range outside
//! the curve's, is refused; the charges are the same on any number of
//! threads and the budget running out stops the fit.

use super::calibration::{fit, small_circle, sphere};
use super::*;
use crate::geom::conic::Conic;
use crate::geom::form::{CylinderKey, TorusKey};
use crate::geom::lattice::{Lattice, Level, Rect};
use crate::infra::budget::{Halt, MAX_WORK};

/// A seam target: a quarter of the least tolerance, `10⁻⁵` mm.
const TARGET: f64 = 2.5e-6;

const CENTRE: [f64; 3] = [3.0, -7.0, 11.0];
const AXIS: [f64; 3] = [0.6, 0.0, 0.8];

/// The fit of `c` on `s` over its whole range towards `target`.
fn fit_all(s: &Surface, c: &Curve3D, target: f64) -> Fitted {
    let ck = c.knots();
    let range = [ck[0], ck[ck.len() - 1]];
    fit_alone(s, c, range, None, target, MAX_WORK).expect("a fit")
}

/// Checks what every fit keeps (module docs): spans in tiles, the
/// curve's knots among the p-curve's, its range, and the certified
/// deviation at dense samples (the p-curve's canonical points) below the
/// bound, up to the surface's speed times the canonical points' rounding.
fn check(s: &Surface, c: &Curve3D, range: [f64; 2], f: &Fitted, what: &str) {
    let p = &f.pcurve;
    assert!(p.in_tiles(), "{what}");
    let pk = p.knots();
    assert_eq!([pk[0], pk[pk.len() - 1]], range, "{what}");
    for t in c.knots().iter().filter(|&&t| range[0] < t && t < range[1]) {
        assert!(pk.contains(t), "{what}: knot {t}");
    }
    assert!(f.seam.bound >= f.lower, "{what}: {f:?}");
    let n = 400;
    for k in 0..=n {
        let t = range[0] + (range[1] - range[0]) * (k as f64 / n as f64);
        let uv = p.point_at(t).unwrap();
        let x = c.eval(t).unwrap();
        let y = s.eval(uv).expect("in the domain");
        let d = lower(&y, &x);
        assert!(
            d <= f.seam.bound + 1e-9,
            "{what}: deviation {d} above the bound {} at {t}",
            f.seam.bound
        );
    }
}

#[test]
fn sphere_circles_are_fitted_to_their_target() {
    let mut spans = Vec::new();
    for level in [0, 2] {
        let s = sphere(CENTRE, 40.0, AXIS, level);
        let circles = [(1.0, 0.0, 0.3), (0.7, 0.5, 0.25), (1.2, -0.6, 0.05)];
        for (k, (angle, lat, rho)) in circles.into_iter().enumerate() {
            let cl = (k as u32 + level) % 2;
            let c = small_circle(&s, angle, lat, rho, cl);
            let what = format!("level {level}, circle {angle} {lat} {rho} at {cl}");
            let f = fit_all(&s, &c, TARGET);
            assert!(f.seam.bound <= TARGET, "{what}: {:?}", f.seam);
            let ck = c.knots();
            let range = [ck[0], ck[ck.len() - 1]];
            check(&s, &c, range, &f, &what);
            spans.push(f.pcurve.spans().len());
            if k == 0 {
                // About a tile line (the equator), its end on the line;
                // closed in UV with its ends given as one vertex's.
                let p = &f.pcurve;
                let e = p.knot_point(p.spans().len()).unwrap();
                assert_eq!(e[1], 0.0, "{what}");
                let g = fit_alone(&s, &c, range, Some([e, e]), TARGET, MAX_WORK).unwrap();
                assert!(g.seam.bound <= TARGET, "{what}: {:?}", g.seam);
                let p = &g.pcurve;
                assert_eq!(p.knot_point(0), p.knot_point(p.spans().len()), "{what}");
            }
        }
    }
    println!("spans per fitted circle: {spans:?}");
}

#[test]
fn sub_ranges_and_given_ends_are_kept() {
    let s = sphere(CENTRE, 40.0, AXIS, 1);
    let c = small_circle(&s, 1.0, 0.4, 0.3, 0);
    let range = [0.3, 3.7];
    // The inverse's ends, each moved by an ulp: kept to the bit.
    let inv = |t: f64| s.inverse(c.eval(t).unwrap().map(|x| x.v)).unwrap().uv;
    let ends = [
        inv(range[0]).map(f64::next_up),
        inv(range[1]).map(f64::next_down),
    ];
    let f = fit_alone(&s, &c, range, Some(ends), TARGET, MAX_WORK).unwrap();
    assert!(f.seam.bound <= TARGET, "{:?}", f.seam);
    check(&s, &c, range, &f, "sub-range");
    let p = &f.pcurve;
    assert_eq!(p.knot_point(0), Some(ends[0]));
    assert_eq!(p.knot_point(p.spans().len()), Some(ends[1]));
    // Without them, the inverse's.
    let f = fit_alone(&s, &c, range, None, TARGET, MAX_WORK).unwrap();
    assert_eq!(f.pcurve.knot_point(0), Some(inv(range[0])));
}

#[test]
fn an_oblique_ellipse_on_a_cylinder_is_fitted_across_a_tile_line() {
    // The plane through the axis' foot at `θ` from the cross-section: an
    // ellipse whose height `−r·tan θ·cos φ` crosses the axial tile line
    // `v = 1` of level 1 at `φ = 90°`.
    let r = 25.0;
    let key = CylinderKey::new([1.0, 2.0, -3.0], [0.0, 0.6, 0.8], r).unwrap();
    let lat = Lattice::new(key.into()).unwrap();
    let (o, [e1, e2, a]) = (lat.origin(), lat.axes());
    let rect = Rect {
        level: Level { u: 1, v: 1 },
        u: -1..6,
        v: 0..2,
    };
    let s = Surface::new(lat, rect).unwrap();
    let (sin, cos) = (0.6, 0.8);
    let major: [f64; 3] = std::array::from_fn(|i| cos * e1[i] - sin * a[i]);
    let conic = Conic::ellipse(o, [major, e2], [r / cos, r]).unwrap();
    for cl in 0..2 {
        let c = Curve3D::ellipse(&conic, cl, 0, 2 << cl).unwrap();
        let what = format!("ellipse at {cl}");
        let f = fit_all(&s, &c, TARGET);
        assert!(f.seam.bound <= TARGET, "{what}: {:?}", f.seam);
        let ck = c.knots();
        check(&s, &c, [ck[0], ck[ck.len() - 1]], &f, &what);
        // Broken at the tile line it crosses, exactly on it there.
        let p = &f.pcurve;
        let on_line = (0..=p.spans().len()).any(|k| p.knot_point(k).unwrap()[1] == 1.0);
        assert!(on_line, "{what}");
    }
}

#[test]
fn a_villarceau_circle_on_a_ring_torus_is_fitted() {
    // The circle of radius `R` about `o + r·e₂` in the plane through the
    // centre spanned by `e₂` and `(√(R² − r²)·e₁ + r·a)/R`: its half from
    // the top of the tube over the outer equator to its bottom.
    let (big, small) = (30.0, 12.0);
    let key = TorusKey::new([-4.0, 5.0, 2.0], [0.0, 0.0, 1.0], big, small).unwrap();
    let lat = Lattice::new(key.into()).unwrap();
    let (o, [e1, e2, a]) = (lat.origin(), lat.axes());
    let rect = Rect {
        level: Level::BASE,
        u: 0..2,
        v: -1..2,
    };
    let s = Surface::new(lat, rect).unwrap();
    let root = ((big - small) * (big + small)).sqrt();
    let tilted: [f64; 3] = std::array::from_fn(|i| (root * e1[i] + small * a[i]) / big);
    let centre: [f64; 3] = std::array::from_fn(|i| o[i] + small * e2[i]);
    let conic = Conic::circle(centre, [tilted, e2], big).unwrap();
    let c = Curve3D::ellipse(&conic, 0, 0, 2).unwrap();
    let f = fit_all(&s, &c, TARGET);
    assert!(f.seam.bound <= TARGET, "{:?}", f.seam);
    check(&s, &c, [0.0, 2.0], &f, "Villarceau");
}

#[test]
fn a_circle_off_its_sphere_stops_within_tight_of_its_knots() {
    // The circle of the sphere 10⁻³ mm larger: about that far off the
    // surface everywhere, the target out of reach.
    let s = sphere(CENTRE, 40.0, AXIS, 0);
    let big = sphere(CENTRE, 40.001, AXIS, 0);
    let c = small_circle(&big, 1.0, 0.3, 0.3, 0);
    let f = fit_all(&s, &c, TARGET);
    assert!(f.lower > TARGET && f.lower <= 1.001e-3, "{f:?}");
    assert!(f.seam.bound <= TIGHT * f.lower, "{f:?}");
    check(&s, &c, [0.0, 4.0], &f, "off the sphere");
    // Not refined to the depth cap: a few spans per quarter.
    assert!(f.pcurve.spans().len() <= 64, "{}", f.pcurve.spans().len());
}

#[test]
fn ranges_outside_the_curve_and_curves_leaving_the_domain_are_refused() {
    let s = sphere(CENTRE, 40.0, AXIS, 0);
    let c = small_circle(&s, 1.0, 0.0, 0.3, 0);
    for range in [
        [1.0, 1.0],
        [2.0, 1.0],
        [-0.5, 1.0],
        [0.0, 4.5],
        [f64::NAN, 1.0],
    ] {
        let out = fit_alone(&s, &c, range, None, TARGET, MAX_WORK);
        assert!(
            matches!(
                out,
                Err(Halt::Failed {
                    error: FitError::Range,
                    ..
                })
            ),
            "{range:?}: {out:?}"
        );
    }
    // About the domain's side at angle 0: half of it outside.
    let c = small_circle(&s, 0.0, 0.0, 0.3, 0);
    let out = fit_alone(&s, &c, [0.0, 4.0], None, TARGET, MAX_WORK);
    assert!(
        matches!(
            out,
            Err(Halt::Failed {
                error: FitError::Outside { .. },
                ..
            })
        ),
        "{out:?}"
    );
}

#[test]
fn targets_below_zero_or_nan_count_as_zero() {
    let s = sphere(CENTRE, 40.0, AXIS, 0);
    let c = small_circle(&s, 1.0, 0.2, 0.2, 0);
    let zero = fit(&s, &c, 0.0, MAX_WORK);
    for t in [-1.0, f64::NAN] {
        assert_eq!(fit(&s, &c, t, MAX_WORK), zero, "{t}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fits_are_charged_the_same_on_any_number_of_threads() {
    use crate::infra::par::assert_deterministic;
    let s = sphere(CENTRE, 40.0, AXIS, 1);
    let c = small_circle(&s, 0.8, 0.3, 0.05, 0);
    let (out, used) = assert_deterministic(|| fit(&s, &c, TARGET, MAX_WORK));
    assert!(out.is_some());
    for budget in [1, used / 2, used - 1] {
        let (out, _) = assert_deterministic(|| fit(&s, &c, TARGET, budget));
        assert!(out.is_none(), "{budget}");
    }
}
