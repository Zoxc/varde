#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use super::*;
use varde_testkit::rng::Rng;

/// A unit vector square to `axis`.
fn across(axis: DVec3) -> DVec3 {
    axis.any_orthonormal_vector()
}

#[test]
fn planes_are_kept_unit() {
    let form = Form::plane(DVec3::new(0.0, 0.0, 4.0), 8.0);
    assert_eq!(
        form,
        Form::Plane {
            n: DVec3::Z,
            d: 2.0
        }
    );
    assert_eq!(form.distance(DVec3::new(5.0, -3.0, 3.5)), 1.5);
    assert_eq!(
        form.flipped(),
        Form::plane(DVec3::new(0.0, 0.0, -1.0), -2.0)
    );
    assert_eq!(form.flipped().flipped(), form);
    for bad in [DVec3::ZERO, DVec3::NAN, DVec3::new(f64::INFINITY, 0.0, 0.0)] {
        assert_eq!(Form::plane(bad, 1.0), Form::Unknown);
    }
    assert_eq!(Form::plane(DVec3::X, f64::NAN), Form::Unknown);
    assert_eq!(Form::Unknown.distance(DVec3::splat(1e9)), 0.0);
}

#[test]
fn curved_forms_measure_true_distances_and_dont_flip() {
    let mut rng = Rng::new(1);
    for _ in 0..200 {
        let (o, axis) = (rng.point(1e3), rng.direction());
        let (x, y) = (across(axis), axis.cross(across(axis)));
        let phi = rng.range(0.0, TAU);
        let radial = x * phi.cos() + y * phi.sin();
        let r = rng.log_range(0.1, 100.0);
        let h = rng.range(-100.0, 100.0);
        let off = rng.range(-0.5, 0.5) * r;
        let near = |got: f64, want: f64| {
            assert!(
                (got - want).abs() <= 1e-9 * (1.0 + o.length() + r + h.abs()),
                "{got} {want}"
            )
        };

        let cylinder = Form::Cylinder {
            point: o,
            axis,
            radius: r,
        };
        near(
            cylinder.distance(o + axis * h + radial * (r + off)),
            off.abs(),
        );

        let sphere = Form::Sphere {
            centre: o,
            radius: r,
        };
        let dir = rng.direction();
        near(sphere.distance(o + dir * (r + off)), off.abs());

        let minor = r * rng.range(0.1, 0.9);
        let torus = Form::Torus {
            centre: o,
            axis,
            major: r,
            minor,
        };
        let turn = rng.range(0.0, TAU);
        let tube = radial * turn.cos() + axis * turn.sin();
        near(
            torus.distance(o + radial * r + tube * (minor + off * 0.1)),
            (off * 0.1).abs(),
        );

        // A cone of half-angle `a`: points along the ruling and moved
        // square to it, and behind the apex the distance to it.
        let a = rng.range(0.05, 1.5);
        let cone = Form::Cone {
            apex: o,
            axis,
            cos: a.cos(),
            sin: a.sin(),
        };
        let ruling = axis * a.cos() + radial * a.sin();
        let normal = radial * a.cos() - axis * a.sin();
        let along = rng.range(0.0, 100.0);
        // Near enough the ruling that it is the nearest.
        let off = rng.range(-0.4, 0.4) * along * a.sin() * a.cos();
        near(cone.distance(o + ruling * along + normal * off), off.abs());
        let behind = o - axis * rng.range(0.1, 10.0);
        near(cone.distance(behind), (behind - o).length());

        for form in [cylinder, sphere, torus, cone] {
            assert_eq!(form.flipped(), form);
        }
    }
}

#[test]
fn conic_forms_measure_to_first_order() {
    // Revolved meridians and conic cylinders: on the conic (both arcs of
    // it) the distance is zero to rounding, a little off it about as far.
    let mut rng = Rng::new(2);
    for _ in 0..200 {
        let (o, axis) = (rng.point(1e3), rng.direction());
        let (x, y) = (across(axis), axis.cross(across(axis)));
        let size = rng.log_range(0.1, 100.0);
        let stretch = rng.range(0.3, 3.0);
        // An ellipse arc in the meridian half-plane, away from the axis.
        let centre = DVec2::new(2.0 * size, rng.range(-size, size));
        let start = rng.range(0.0, TAU);
        let sweep = rng.range(0.1, FRAC_PI_2);
        let ellipse = |t: f64| centre + DVec2::new(t.cos() * size, t.sin() * size * stretch);
        let circle = Conic2::arc(DVec2::ZERO, size, start, sweep).unwrap();
        let squash = |p: DVec2| centre + DVec2::new(p.x, p.y * stretch);
        let meridian = Conic2 {
            p0: squash(circle.p0),
            c: squash(circle.c),
            w: circle.w,
            p1: squash(circle.p1),
        };
        let revolved = Form::Revolved {
            origin: o,
            axis,
            meridian,
        };
        let phi = rng.range(0.0, TAU);
        let radial = x * phi.cos() + y * phi.sin();
        let place = |p: DVec2| o + radial * p.x + axis * p.y;
        let reach = o.length() + 4.0 * size * (1.0 + stretch);
        for t in [start, start + sweep / 3.0, start + PI, start + 4.0] {
            let p = ellipse(t);
            assert!(revolved.distance(place(p)) <= 1e-12 * reach);
            let normal = DVec2::new(t.cos() * stretch, t.sin()).normalize();
            let off = 1e-5 * size;
            let d = revolved.distance(place(p + normal * off));
            assert!((d - off).abs() <= 1e-3 * off, "{d} {off}");
        }

        let along = (x.cross(y) + rng.point(0.5)).normalize();
        let conic = Conic3 {
            p0: place(meridian.p0),
            c: place(meridian.c),
            w: meridian.w,
            p1: place(meridian.p1),
        };
        let cylinder = Form::ConicCylinder { conic, along };
        for t in [start, start + sweep / 2.0, start + 2.0] {
            let p = place(ellipse(t)) + along * rng.range(-size, size);
            assert!(cylinder.distance(p) <= 1e-12 * reach);
        }
    }
}
