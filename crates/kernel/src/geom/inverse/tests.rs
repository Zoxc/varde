//! The inverse against the surfaces' own points: every form, at the base
//! level and finer, its points back to their UV within a certified bound
//! that covers the exact distance; points off the surface along its
//! normal back to their foot's UV on the forms whose inverse is the
//! foot; the turn nearest the surface's, the axis's points at the middle
//! angle, points beyond the domain unbounded; the angle and span maps
//! against their parametrisations.

use malachite::Rational as Q;
use varde_decide::num::trig::{atan2, cos, sin};
use varde_testkit::rng::Rng;

use super::*;
use crate::geom::conic::Conic;
use crate::geom::form::{ConeKey, CylinderKey, PlaneKey, SphereKey, TorusKey};
use crate::geom::round::q;
use crate::geom::samples::{Sample, surfaces};

/// A bound within this many units of the tile's coordinate scale.
const TOL: f64 = 1e-14;

/// The point at `uv` (its certified value's centre).
fn point(s: &Surface, uv: [f64; 2]) -> [f64; 3] {
    s.eval(uv).unwrap().map(|b| b.v)
}

/// The largest coordinate magnitude of the surface's control points.
fn scale(s: &Surface) -> f64 {
    s.tiles()
        .iter()
        .flat_map(|t| t.net().iter().flat_map(|p| p[..3].iter().map(|x| x.abs())))
        .fold(0.0, |m, x| if x > m { x } else { m })
}

/// The exact squared distance from the stored surface's exact point at
/// `uv` (its tile's exact patch at the local coordinates) to `x`.
fn exact_distance2(s: &Surface, uv: [f64; 2], x: &[f64; 3]) -> Q {
    let at = s.locate(uv).unwrap();
    let [lu, lv] = [(uv[0], at.i), (uv[1], at.j)].map(|(c, i)| q(c) - Q::from(i));
    let p = s.tiles()[at.index].exact().eval(&lu, &lv);
    (0..3)
        .map(|k| {
            let d = &p[k] - q(x[k]);
            &d * &d
        })
        .sum()
}

#[test]
fn points_return_to_their_uv() {
    let mut rng = Rng::new(1);
    for Sample { name, surface } in surfaces(1) {
        let s = scale(&surface);
        let rect = surface.rect().clone();
        for _ in 0..40 {
            let uv = [
                rng.range(rect.u.start as f64, rect.u.end as f64),
                rng.range(rect.v.start as f64, rect.v.end as f64),
            ];
            let x = point(&surface, uv);
            let inv = surface.inverse(x).unwrap();
            assert!(
                inv.dev <= TOL * s,
                "{name}: {uv:?} gave {:?}, {:e} of {s:e}",
                inv.uv,
                inv.dev
            );
            let d2 = exact_distance2(&surface, inv.uv, &x);
            assert!(d2 <= q(inv.dev) * q(inv.dev), "{name}: the bound holds");
            // The UV itself, where the point is a single one of the
            // surface's (off its poles and apexes).
            let du = surface.eval_partials(uv).unwrap();
            let speed = |d: &[Bounded<f64>; 3]| {
                (d[0].v * d[0].v + d[1].v * d[1].v + d[2].v * d[2].v).sqrt()
            };
            let (su, sv) = (speed(&du.du), speed(&du.dv));
            if su > 1e-6 * s && sv > 1e-6 * s {
                for k in 0..2 {
                    let sp = [su, sv][k];
                    assert!(
                        (inv.uv[k] - uv[k]).abs() * sp <= 4.0 * TOL * s,
                        "{name}: {uv:?} gave {:?}",
                        inv.uv
                    );
                }
            }
        }
    }
}

/// The unit normal at `uv` (`x_u × x_v` normalised).
fn normal(s: &Surface, uv: [f64; 2]) -> [f64; 3] {
    let p = s.eval_partials(uv).unwrap();
    let (a, b) = (p.du.map(|x| x.v), p.dv.map(|x| x.v));
    let n = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    n.map(|c| c / l)
}

#[test]
fn points_off_the_surface_return_to_their_foot() {
    let mut rng = Rng::new(2);
    let a = [0.36, -0.48, 0.8];
    let p = [12.0, -7.5, 3.25];
    let forms: Vec<(&str, FormKey, f64)> = vec![
        ("plane", PlaneKey::new(a, p).unwrap().into(), 5.0),
        ("cylinder", CylinderKey::new(p, a, 3.0).unwrap().into(), 1.0),
        ("cone", ConeKey::new(p, a, 1.0, 0.75).unwrap().into(), 1.0),
        ("sphere", SphereKey::new(p, 3.0, a).unwrap().into(), 1.0),
        ("torus", TorusKey::new(p, a, 5.0, 2.0).unwrap().into(), 0.5),
    ];
    for (name, key, reach) in forms {
        let lattice = Lattice::new(key).unwrap();
        let rect = lattice.half(0).unwrap();
        let surface = Surface::new(lattice, rect.clone()).unwrap();
        for _ in 0..40 {
            // Away from the cone's apex and the sphere's poles, within a
            // cell's reach of the axis on the cylinder and cone.
            let uv = [
                rng.range(rect.u.start as f64 + 0.05, rect.u.end as f64 - 0.05),
                match name {
                    "cylinder" => 0.5 + rng.range(-1e-5, 1e-5),
                    "cone" => rng.range(1e-6, 1e-5),
                    "plane" => 0.5 + rng.range(-1e-5, 1e-5),
                    _ => rng.range(rect.v.start as f64 + 0.05, rect.v.end as f64 - 0.05),
                },
            ];
            let n = normal(&surface, uv);
            let d = rng.range(-reach, reach);
            let x: [f64; 3] = std::array::from_fn(|k| point(&surface, uv)[k] + d * n[k]);
            let inv = surface.inverse(x).unwrap();
            // The foot's UV: the parameters' error times the speeds within
            // the evaluation's rounding.
            let p = surface.eval_partials(uv).unwrap();
            let speed = |d: &[Bounded<f64>; 3]| {
                (d[0].v * d[0].v + d[1].v * d[1].v + d[2].v * d[2].v).sqrt()
            };
            let moved =
                (inv.uv[0] - uv[0]).abs() * speed(&p.du) + (inv.uv[1] - uv[1]).abs() * speed(&p.dv);
            assert!(
                moved <= TOL * scale(&surface),
                "{name}: {uv:?} gave {:?}",
                inv.uv
            );
            assert!(
                (inv.dev - d.abs()).abs() <= TOL * scale(&surface),
                "{name}: {} off, bound {}",
                d.abs(),
                inv.dev
            );
        }
    }
}

#[test]
fn angles_take_the_surface_s_turn() {
    let key = CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], 2.0).unwrap();
    for (first, deg) in [(3, 10.0f64), (3, 300.0), (-2, 200.0), (1, 120.0), (0, 0.0)] {
        let lattice = Lattice::new(key.into()).unwrap();
        let [e1, e2, a] = lattice.axes();
        let (c, s) = (cos(deg.to_radians()), sin(deg.to_radians()));
        let x: [f64; 3] = std::array::from_fn(|k| 2.0 * (c * e1[k] + s * e2[k]) + a[k]);
        let rect = lattice.half(first).unwrap();
        let surface = Surface::new(lattice, rect).unwrap();
        let inv = surface.inverse(x).unwrap();
        // The angle's cell, taken in the turn of the surface's range
        // `[first, first + 2]` (its middle `first + 1`).
        let cell = (deg / 90.0).floor();
        let want = cell + 4.0 * ((first as f64 + 1.0 - cell - 0.5) / 4.0 + 0.5).floor();
        assert_eq!(inv.uv[0].floor(), want, "{first}, {deg}°: {:?}", inv.uv);
        // The canonical tiles reach `CELL/2` along the axis.
        assert!(inv.dev <= 1e-14 * CELL, "{first}, {deg}°: {:e}", inv.dev);
    }
}

#[test]
fn the_axis_takes_the_middle_angle() {
    let a = [0.0, 0.6, 0.8];
    let c = [1.0, 2.0, 3.0];
    let sphere = SphereKey::new(c, 2.0, a).unwrap();
    let lattice = Lattice::new(sphere.into()).unwrap();
    let rect = lattice.half(1).unwrap();
    let surface = Surface::new(lattice, rect).unwrap();
    for (sign, v) in [(1.0, 1.0), (-1.0, -1.0)] {
        let pole: [f64; 3] = std::array::from_fn(|k| c[k] + sign * 2.0 * a[k]);
        let inv = surface.inverse(pole).unwrap();
        assert_eq!(inv.uv, [2.0, v]);
        assert!(inv.dev < 1e-14, "{:e}", inv.dev);
    }
    let cone = ConeKey::new(c, a, 1.0, 1.0).unwrap();
    let lattice = Lattice::new(cone.into()).unwrap();
    let rect = lattice.half(-1).unwrap();
    let surface = Surface::new(lattice, rect).unwrap();
    let inv = surface.inverse(c).unwrap();
    assert_eq!(inv.uv, [0.0, 0.0]);
    assert!(inv.dev < 1e-150, "{:e}", inv.dev);
}

#[test]
fn points_beyond_the_domain_are_unbounded() {
    let key = SphereKey::new([0.0; 3], 1.0, [0.0, 0.0, 1.0]).unwrap();
    let lattice = Lattice::new(key.into()).unwrap();
    let rect = lattice.half(0).unwrap();
    let surface = Surface::new(lattice, rect).unwrap();
    // At 270° from the frame's `e₁`, on the other half.
    let e2 = surface.lattice().axes()[1];
    let inv = surface.inverse(e2.map(|c| -c)).unwrap();
    assert_eq!(inv.uv, [3.0, 0.0]);
    assert_eq!(inv.dev, f64::INFINITY);
    assert_eq!(
        surface.inverse([f64::NAN, 0.0, 0.0]),
        Err(InverseError::NotFinite)
    );
    assert_eq!(
        surface.inverse([0.0, f64::INFINITY, 0.0]),
        Err(InverseError::NotFinite)
    );
}

#[test]
fn quarter_turns_follow_the_quarter_circle() {
    let mut rng = Rng::new(3);
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let quarter = [[1.0, 0.0, 1.0], [1.0, 1.0, w], [0.0, 1.0, 1.0]];
    for _ in 0..1000 {
        let angle = rng.range(-4.0, 4.0);
        let r = rng.log_range(1e-3, 1e3);
        let theta = angle * std::f64::consts::FRAC_PI_2;
        let (x, y) = (r * cos(theta), r * sin(theta));
        let t = quarter_turns(x, y).unwrap();
        assert!((0.0..=4.0).contains(&t), "{t}");
        // The quarter circle's point at the cell's parameter, turned to
        // the cell, points the same way.
        let k = t.floor();
        let p = span_point(&quarter, t - k);
        let back = atan2(p[1], p[0]) + k * std::f64::consts::FRAC_PI_2;
        let want = atan2(y, x).rem_euclid(2.0 * std::f64::consts::PI);
        let diff = (back - want).abs();
        let other = 2.0 * std::f64::consts::PI - diff;
        let diff = if other < diff { other } else { diff };
        assert!(diff <= 4e-16 * 8.0, "{angle}: {diff:e}");
    }
    assert_eq!(quarter_turns(0.0, 0.0), None);
    assert_eq!(quarter_turns(-0.0, 0.0), None);
    assert_eq!(quarter_turns(1.0, 0.0), Some(0.0));
    assert_eq!(quarter_turns(0.0, 1.0), Some(1.0));
    assert_eq!(quarter_turns(-1.0, 0.0), Some(2.0));
    assert_eq!(quarter_turns(0.0, -1.0), Some(3.0));
    assert_eq!(quarter_turns(1.0, -0.0), Some(0.0));
}

#[test]
fn span_parameters_invert_span_points() {
    let mut rng = Rng::new(4);
    let axes = [[1.0, 0.0], [0.0, 1.0]];
    for k in 0..300 {
        let c = [rng.range(-10.0, 10.0), rng.range(-10.0, 10.0)];
        let r = rng.log_range(1e-2, 1e2);
        let span: Span2 = match k % 4 {
            0 => Conic::ellipse(c, axes, [r, 0.4 * r])
                .unwrap()
                .quarter::<3>(k as i64 / 4)
                .unwrap(),
            1 => Conic::parabola(c, axes, r)
                .unwrap()
                .arc::<3>(rng.range(-2.0, 0.0), rng.range(0.1, 2.0))
                .unwrap(),
            2 => Conic::hyperbola(c, axes, [r, 0.7 * r])
                .unwrap()
                .arc::<3>(rng.range(0.5, 1.0), rng.range(1.1, 2.0))
                .unwrap(),
            _ => {
                // A straight span with its middle point anywhere between
                // its ends and any weights.
                let d = [rng.range(-1.0, 1.0) * r, rng.range(-1.0, 1.0) * r];
                let m = rng.range(0.1, 0.9);
                [
                    [c[0], c[1], rng.range(0.5, 2.0)],
                    [c[0] + m * d[0], c[1] + m * d[1], rng.range(0.5, 2.0)],
                    [c[0] + d[0], c[1] + d[1], rng.range(0.5, 2.0)],
                ]
            }
        };
        let s = rng.unit();
        let p = span_point(&span, s);
        let got = span_param(&span, p);
        assert!((got - s).abs() <= 1e-10, "span {k}: {s} gave {got}");
    }
}

#[test]
fn profiles_take_the_nearest_span() {
    // Two quarters of a circle about `(3, 0)` in the meridian: the point
    // at 120° is the second span's.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let spans = [
        [[4.0, 0.0, 1.0], [4.0, 1.0, w], [3.0, 1.0, 1.0]],
        [[3.0, 1.0, 1.0], [2.0, 1.0, w], [2.0, 0.0, 1.0]],
    ];
    let at = |deg: f64| {
        let t = deg.to_radians();
        [3.0 + cos(t), sin(t)]
    };
    let s = along(&spans, &(0..=1), at(120.0));
    assert!(s > 1.0 && s < 2.0, "{s}");
    let s = along(&spans, &(0..=1), at(30.0));
    assert!(s > 0.0 && s < 1.0, "{s}");
    // Only the spans asked for.
    let s = along(&spans, &(1..=1), at(30.0));
    assert!(s < 1.0, "{s}");
}
