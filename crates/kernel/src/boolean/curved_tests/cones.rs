//! Cones and coaxial surfaces of revolution: a cone cut by planes (a
//! countersink, half of one, slabs tilted across it), and turned solids
//! on one axis joined and cut, whose walls meet in parallels: every cut
//! exact, its bands on their quadrics to about `1e-12`.

use super::*;
use crate::mesh::Form;
use crate::revolve::{Sweep, revolve};
use crate::test_rng::Rng;

/// The frame with its axis along `z` through the origin.
const Z: Frame = Frame {
    origin: DVec3::ZERO,
    x: DVec3::X,
    y: DVec3::Z,
};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// The polygon of `(radius, height)` points turned all the way round `z`.
fn turned(points: &[DVec2], feature: u64) -> Solid {
    turned_at(points, feature, &TOL)
}

fn turned_at(points: &[DVec2], feature: u64, tol: &Tolerance) -> Solid {
    revolve(
        &Profile {
            loops: vec![polygon(points, 1)],
        },
        &Z,
        Sweep::Full,
        feature,
        tol,
        &Budget::DEFAULT,
    )
    .unwrap()
}

/// A solid of revolution about `z` by its radius at each height: a
/// polyline of `(height, radius)`, heights increasing, zero outside.
#[derive(Debug, Clone)]
struct Radii(Vec<(f64, f64)>);

impl Radii {
    fn at(&self, h: f64) -> f64 {
        let p = &self.0;
        if h < p[0].0 || h > p[p.len() - 1].0 {
            return 0.0;
        }
        for w in p.windows(2) {
            let ((h0, r0), (h1, r1)) = (w[0], w[1]);
            if h >= h0 && h <= h1 && h1 > h0 {
                return r0 + (r1 - r0) * (h - h0) / (h1 - h0);
            }
        }
        0.0
    }
}

/// `∫ π·f(h)² dh` where `f` is `a` and `b`'s radii combined by `pick`
/// (`f64::min` for their intersection, `f64::max` for their union), in
/// closed form: the radii are linear between their corners and their
/// crossings, and so is `f`, whose square integrates exactly there.
fn turned_volume(a: &Radii, b: &Radii, pick: fn(f64, f64) -> f64) -> f64 {
    let mut cuts: Vec<f64> = a.0.iter().chain(&b.0).map(|p| p.0).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    // Where the two cross between corners.
    let mut all = cuts.clone();
    for w in cuts.windows(2) {
        let (h0, h1) = (w[0], w[1]);
        let d0 = a.at(h0 + 1e-15 * (h1 - h0)) - b.at(h0 + 1e-15 * (h1 - h0));
        let d1 = a.at(h1 - 1e-15 * (h1 - h0)) - b.at(h1 - 1e-15 * (h1 - h0));
        if d0 * d1 < 0.0 {
            all.push(h0 + (h1 - h0) * d0 / (d0 - d1));
        }
    }
    all.sort_by(f64::total_cmp);
    all.windows(2)
        .map(|w| {
            let (h0, h1) = (w[0], w[1]);
            let inside = |h: f64| h0 + (h1 - h0) * h;
            // Just inside the interval: a radius jumps at a cap.
            let f = |t: f64| pick(a.at(inside(t)), b.at(inside(t)));
            let (r0, r1) = (f(1e-12), f(1.0 - 1e-12));
            // The ends nudged inside: undo it on the line through them.
            let slope = (r1 - r0) / (1.0 - 2e-12);
            let (r0, r1) = (r0 - slope * 1e-12, r1 + slope * 1e-12);
            PI * (h1 - h0) / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1)
        })
        .sum()
}

/// The profile of `radii` (heights increasing, radii positive) as a
/// polygon to turn: up the axis side, out along the bottom.
fn profile_of(radii: &Radii) -> Vec<DVec2> {
    let p = &radii.0;
    let mut points = vec![v(0.0, p[0].0)];
    points.extend(p.iter().map(|&(h, r)| v(r, h)));
    points.push(v(0.0, p[p.len() - 1].0));
    points
}

/// `a`'s four results with `b` by their volumes against the closed forms
/// (`within`), each exact.
fn four_exact(name: &str, a: &Radii, b: &Radii, sa: &Solid, sb: &Solid, within: f64, size: f64) {
    let results = all_four(sa, sb, within);
    let both = turned_volume(a, b, f64::min);
    volumes(name, sa, sb, &results, both, within);
    let union = turned_volume(a, b, f64::max);
    assert!(
        (results[0].volume() - union).abs() <= within,
        "{name}: union {} not {union}",
        results[0].volume()
    );
    for (k, solid) in results.iter().enumerate() {
        exact(&format!("{name} {k}"), solid, size);
    }
}

#[test]
fn closed_form_volumes_of_turned_solids() {
    // The helper itself, on a cylinder and a frustum crossing it.
    let cyl = Radii(vec![(0.0, 1.0), (4.0, 1.0)]);
    let cone = Radii(vec![(2.0, 1.5), (5.0, 0.5)]);
    let both = turned_volume(&cyl, &cone, f64::min);
    // Below 3.5 the cylinder, above it the cone to 4.
    let want = PI * 1.5 + PI * 0.5 / 3.0 * (1.0 + 5.0 / 6.0 + 25.0 / 36.0);
    assert!((both - want).abs() < 1e-14, "{both} {want}");
    let one = turned_volume(&cyl, &Radii(vec![(0.0, 0.0), (0.0, 0.0)]), f64::max);
    assert!((one - 4.0 * PI).abs() < 1e-14);
}

/// A countersunk hole's tool: a hole of radius ½ through to `z = ½`, then
/// a 45° cone out to radius 1½ at `z = 1½`, its apex at the origin.
fn countersink() -> Solid {
    turned(
        &[
            v(0.0, -0.5),
            v(0.5, -0.5),
            v(0.5, 0.5),
            v(1.5, 1.5),
            v(0.0, 1.5),
        ],
        2,
    )
}

#[test]
fn a_countersink_is_exact() {
    // Cut into a plate 1 thick (its top at the cone's widest, radius 1),
    // upright and on a frame turned and moved: the cone cut by the plate's
    // top in a circle, by the hole's cylinder in nothing, all exact.
    let plate = cube([-3.0, -2.0, 0.0], [6.0, 4.0, 1.0]);
    let tool = countersink();
    let hole = PI * 0.25 * 0.5 + PI * 0.5 / 3.0 * (0.25 + 0.5 + 1.0);
    let q = DQuat::from_rotation_x(0.4) * DQuat::from_rotation_z(0.3);
    let place = |s: &Solid| moved(s, |p| q * p + DVec3::new(0.3, -0.2, 0.0));
    for (name, plate, tool) in [
        ("upright", plate.clone(), tool.clone()),
        ("turned", place(&plate), place(&tool)),
    ] {
        let results = all_four(&plate, &tool, 1e-12);
        volumes(name, &plate, &tool, &results, hole, 1e-12);
        for (k, solid) in results.iter().enumerate() {
            exact(&format!("countersink {name} {k}"), solid, 6.0);
        }
        // The countersunk plate keeps the cone's face, on its quadric.
        assert!(results[2].mesh().faces().iter().any(|f| matches!(
            (f.form, f.surface),
            (Form::Cone { .. }, Surface::Quadric(_))
        )));
    }
    assert_deterministic(|| run(&plate, &tool, Op::Difference).into_mesh());
}

#[test]
fn half_a_countersink_cuts_the_cone_in_rulings() {
    // The plate's side through the axis cuts the cone in two of its
    // rulings, paced by the geometric mean of their ends' distances from
    // the apex: with the midpoint (a cylinder's ruling) the triangles
    // beside them were 1e-5 off the cone, on copies claiming no surface.
    let half = cube([0.0, -2.0, 0.0], [3.0, 4.0, 1.0]);
    let tool = countersink();
    let hole = PI * 0.25 * 0.5 + PI * 0.5 / 3.0 * (0.25 + 0.5 + 1.0);
    let results = all_four(&half, &tool, 1e-12);
    volumes("half", &half, &tool, &results, hole / 2.0, 1e-12);
    for (k, solid) in results.iter().enumerate() {
        exact(&format!("half {k}"), solid, 6.0);
    }
}

#[test]
fn slabs_tilted_through_a_cone_are_exact() {
    // A slab through the countersink's cone at angles giving ellipse,
    // parabola-like and hyperbola cuts; the four results' volumes add up,
    // every one exact.
    let tool = countersink();
    for angle in [0.3f64, std::f64::consts::FRAC_PI_4, 1.2] {
        let q = DQuat::from_rotation_y(angle);
        let slab = moved(&cube([-3.0, -3.0, -0.2], [6.0, 6.0, 0.6]), |p| {
            q * p + DVec3::new(0.2, 0.1, 1.0)
        });
        let results = all_four(&tool, &slab, 1e-11);
        let both = results[1].volume();
        volumes("tilted slab", &tool, &slab, &results, both, 1e-11);
        for (k, solid) in results.iter().enumerate() {
            exact(&format!("slab at {angle}, {k}"), solid, 6.0);
        }
    }
}

#[test]
fn a_cylinder_and_a_cone_on_one_axis_meet_in_a_parallel() {
    // The cone's wall crosses the cylinder's at `z = 3.5`: an exact
    // circle, and the pairs along it certified rather than refined (they
    // were split until their normals parted: 1 412 patches and two of the
    // four `Invalid`).
    let cyl = Radii(vec![(0.0, 1.0), (4.0, 1.0)]);
    let cone = Radii(vec![(2.0, 1.5), (5.0, 0.5)]);
    let (a, b) = (turned(&profile_of(&cyl), 3), turned(&profile_of(&cone), 4));
    four_exact("cylinder and cone", &cyl, &cone, &a, &b, 1e-11, 6.0);
    let both = run(&a, &b, Op::Intersection);
    assert!(
        both.mesh().tris().len() < 200,
        "{}",
        both.mesh().tris().len()
    );
    assert_deterministic(|| run(&a, &b, Op::Union).into_mesh());
}

#[test]
fn cones_through_a_cylinders_cap() {
    // A cone on the cylinder's axis through its top, inside its wall, cut
    // in a circle near the rim. The walls' pairs are certified now, not
    // refined, so the cap's ring between the rim's quarter arcs and the
    // cut came whole to the triangulation, and the rim's chords cross the
    // circle: it was fanned from a rim vertex past the circle's tangent
    // point, the circle halved there round after round (`Invalid` in 4 of
    // the 12), until the rim arcs were split for their bulges.
    let cyl = Radii(vec![(0.0, 1.0), (4.0, 1.0)]);
    let a = turned(&profile_of(&cyl), 3);
    for (r0, r1) in [(0.9, 0.6), (0.95, 0.5), (0.99, 0.7), (0.7, 0.95)] {
        let cone = Radii(vec![(2.0, r0), (5.0, r1)]);
        let b = turned(&profile_of(&cone), 4);
        four_exact(&format!("cone {r0} {r1}"), &cyl, &cone, &a, &b, 1e-11, 6.0);
    }
}

#[test]
fn a_turned_shaft_joined_and_cut_coaxially_is_exact() {
    // A cylinder, a cone and a cylinder joined end to end (shared rims and
    // flush discs), then a cone overlapping all three, a V groove and a
    // centre drill: every wall on one axis, cut in parallels.
    let parts = [
        Radii(vec![(0.0, 2.0), (3.0, 2.0)]),
        Radii(vec![(3.0, 2.0), (5.0, 1.0)]),
        Radii(vec![(5.0, 1.0), (9.0, 1.0)]),
    ];
    let mut shaft = turned(&profile_of(&parts[0]), 1);
    let mut want = turned_volume(&parts[0], &parts[0], f64::max);
    for (k, part) in parts.iter().enumerate().skip(1) {
        let next = turned(&profile_of(part), 1 + k as u64);
        shaft = run(&shaft, &next, Op::Union);
        want += turned_volume(part, part, f64::max);
        assert!((shaft.volume() - want).abs() <= 1e-11, "{}", shaft.volume());
        exact(&format!("joined {k}"), &shaft, 10.0);
    }
    // Ends joined: one wall each, the shared rims drawn once.
    assert_eq!(shaft.mesh().tris().len(), 36);
    let whole = Radii(vec![(0.0, 2.0), (3.0, 2.0), (5.0, 1.0), (9.0, 1.0)]);
    let cone = Radii(vec![(2.0, 2.5), (6.0, 0.5)]);
    let d = turned(&profile_of(&cone), 4);
    four_exact("shaft and cone", &whole, &cone, &shaft, &d, 1e-10, 10.0);
    // A V groove round the first cylinder, 1 deep, its sides at 45°.
    let groove = turned(&[v(1.5, 1.5), v(2.5, 0.5), v(2.5, 2.5)], 5);
    let grooved = run(&shaft, &groove, Op::Difference);
    // The groove takes the ring from radius 1.5 to 2 between its sides.
    let groove_want = whole_volume(&whole) - groove_ring();
    assert!(
        (grooved.volume() - groove_want).abs() <= 1e-10,
        "{} not {groove_want}",
        grooved.volume()
    );
    exact("grooved", &grooved, 10.0);
    // A centre drill: its tip's cap is fitted, the rest exact.
    let drill = turned(&[v(0.0, 7.0), v(0.5, 7.5), v(0.5, 10.0), v(0.0, 10.0)], 6);
    let drilled = run(&shaft, &drill, Op::Difference);
    let drill_want = whole_volume(&whole) - PI * 0.25 * 1.5 - PI * 0.25 * 0.5 / 3.0;
    let within = TOL.fit() * (shaft.area() + drill.area()) / 5.0;
    assert!(
        (drilled.volume() - drill_want).abs() <= within,
        "{} not {drill_want}",
        drilled.volume()
    );
    assert_deterministic(|| run(&shaft, &d, Op::Difference).into_mesh());
}

/// The volume of the solid of `radii`.
fn whole_volume(radii: &Radii) -> f64 {
    turned_volume(radii, radii, f64::max)
}

/// What a V groove from radius 1.5 (at `z = 1.5`) out at 45° both ways
/// takes from a cylinder of radius 2: the ring between radius `ρ = 1.5 +
/// |z − 1.5|` and 2, for `z` within ½ of 1.5.
fn groove_ring() -> f64 {
    // 2 ∫₀^½ π (4 − (1.5 + t)²) dt.
    2.0 * PI * (4.0 * 0.5 - (2.0f64.powi(3) - 1.5f64.powi(3)) / 3.0)
}

#[test]
fn random_coaxial_frustums() {
    // Two stacks of a frustum and a cylinder each on one axis, random
    // heights and radii (some radii equal, some ends flush), upright and
    // on a random frame: every result's volume against the closed form,
    // exact where it is upright. Never a wrong result; most succeed.
    let mut rng = Rng::new(12);
    let (mut ok, mut total) = (0, 0);
    let cases = if cfg!(debug_assertions) { 6 } else { 40 };
    for case in 0..cases {
        let mut stack = || {
            let h0 = (rng.range(-2.0, 2.0) * 4.0).round() / 4.0;
            let h1 = h0 + (rng.range(0.5, 3.0) * 4.0).round() / 4.0;
            let h2 = h1 + (rng.range(0.5, 3.0) * 4.0).round() / 4.0;
            let r0 = (rng.range(0.5, 2.0) * 8.0).round() / 8.0;
            let r1 = if rng.range(0.0, 1.0) < 0.3 {
                r0
            } else {
                (rng.range(0.5, 2.0) * 8.0).round() / 8.0
            };
            Radii(vec![(h0, r0), (h1, r1), (h2, r1)])
        };
        let (a, b) = (stack(), stack());
        let frame = if case % 2 == 0 {
            None
        } else {
            let axis = rng.direction();
            Some((DQuat::from_rotation_arc(DVec3::Z, axis), rng.point(10.0)))
        };
        let place = |s: Solid| match frame {
            None => s,
            Some((q, o)) => moved(&s, |p| q * p + o),
        };
        let (sa, sb) = (
            place(turned(&profile_of(&a), 1)),
            place(turned(&profile_of(&b), 2)),
        );
        let both = turned_volume(&a, &b, f64::min);
        let want = [
            turned_volume(&a, &b, f64::max),
            both,
            whole_volume(&a) - both,
            whole_volume(&b) - both,
        ];
        for (k, (x, y, op)) in [
            (&sa, &sb, Op::Union),
            (&sa, &sb, Op::Intersection),
            (&sa, &sb, Op::Difference),
            (&sb, &sa, Op::Difference),
        ]
        .into_iter()
        .enumerate()
        {
            total += 1;
            let Ok(result) = boolean(x, y, op, &TOL, &Budget::DEFAULT) else {
                continue;
            };
            ok += 1;
            let within = 1e-9 * (1.0 + want[k]);
            assert!(
                (result.volume() - want[k]).abs() <= within,
                "case {case} {a:?} {b:?} {op:?}: {} not {}",
                result.volume(),
                want[k]
            );
            if frame.is_none() {
                exact(&format!("case {case} {k}"), &result, 10.0);
            }
        }
    }
    println!("TALLY coaxial frustums: {ok} of {total}");
    assert!(ok * 10 >= total * 9, "{ok} of {total}");
}

#[test]
fn ring_tops_sloping_cut_by_a_cylinder_on_their_axis() {
    // A ring whose top slopes a little (a cone), cut by a cylinder on its
    // axis through the top. Sloping a hundredth or more, the cone claims
    // its quadric and the cut is exact; a few millionths (too nearly flat
    // to claim it: `Free`, its form the cone), the operation may be
    // refused (it is, as it was before, by another error) but is never
    // wrong.
    let tol = Tolerance::new(1e-4).unwrap();
    for rise in [3e-6, 1e-4, 1e-2, 1.0] {
        // Slow in debug builds: there only the cones claiming quadrics.
        if cfg!(debug_assertions) && rise < 1e-3 {
            continue;
        }
        let ring = turned_at(
            &[
                v(100.0, 0.0),
                v(200.0, 0.0),
                v(200.0, 50.0 + rise),
                v(100.0, 50.0),
            ],
            1,
            &tol,
        );
        let free = ring
            .mesh()
            .faces()
            .iter()
            .any(|f| matches!((f.form, f.surface), (Form::Cone { .. }, Surface::Free)));
        assert_eq!(free, rise < 1e-3, "{rise}");
        let pin = turned_at(
            &[v(0.0, 40.0), v(150.0, 40.0), v(150.0, 60.0), v(0.0, 60.0)],
            2,
            &tol,
        );
        // The ring between radii 100 and 150 above `z = 40`, its top at
        // `50 + rise·(ρ − 100)/100`: `∫ 2πρ·(top(ρ) − 40) dρ`.
        let moment = |r: f64| (10.0 - rise) * r * r / 2.0 + rise / 100.0 * r * r * r / 3.0;
        let both = 2.0 * PI * (moment(150.0) - moment(100.0));
        match boolean(&ring, &pin, Op::Intersection, &tol, &Budget::DEFAULT) {
            Ok(result) => {
                assert!(
                    (result.volume() - both).abs() <= 1e-10 * both,
                    "{rise}: {} not {both}",
                    result.volume()
                );
                if !free {
                    exact_to(&format!("rise {rise}"), &result, 200.0, 1e-12);
                }
            }
            Err(e) => {
                assert!(free, "{rise}: {e:?}");
                println!("REFUSED a ring's top rising {rise}: {e:?}");
            }
        }
    }
}

#[test]
fn cones_cut_square_to_their_axis_across_a_faces_diagonal() {
    // A box's face square to a cone's axis cuts it in a circle, which
    // the face's diagonal crosses anywhere (the box moved and spun about
    // the axis): the circle's arcs by their angles, on both triangles of
    // the face, every result against the frustum's closed form and every
    // patch on its surface.
    let mut rng = Rng::new(7);
    // A frustum of a 45° cone (its apex, which revolving fits, cut off).
    let cone = turned(&[v(0.0, 0.1), v(0.1, 0.1), v(1.5, 1.5), v(0.0, 1.5)], 2);
    let cases = if cfg!(debug_assertions) { 3 } else { 24 };
    for case in 0..cases {
        let d = rng.range(0.3, 0.95);
        let (a, b) = (rng.range(-0.5, 0.5), rng.range(-0.5, 0.5));
        let q = DQuat::from_rotation_z(rng.range(0.0, 6.3));
        let block = moved(&cube([-3.0 + a, -3.0 + b, d], [6.0, 6.0, 4.0]), |p| q * p);
        // The frustum above `d`.
        let above = PI * (1.5 - d) / 3.0 * (d * d + d * 1.5 + 1.5 * 1.5);
        let results = all_four(&cone, &block, 1e-12);
        volumes(
            &format!("case {case}"),
            &cone,
            &block,
            &results,
            above,
            1e-11,
        );
        for (k, solid) in results.iter().enumerate() {
            exact(&format!("case {case} {k}"), solid, 6.0);
        }
    }
}

#[test]
fn nearly_flat_cones_cut_through_their_axis_are_exact() {
    // A frustum of a cone opening at 80° to 89.5° from its axis (radius
    // ½ to 20½), its middle half cut out by half and a quarter of a plate
    // whose sides run through the axis along the cone's own seams. The
    // rulings there were checked against the long thin cone patch by
    // Newton's method from its middle, which strayed: some came out as
    // chords paced as a cylinder's, the triangles beside them `3e-7` off
    // the cone (`2e-6` in volume), or traced on copies claiming no
    // surface. Upright and far from the origin.
    for half in [80.0f64, 88.0, 89.5] {
        let tan = half.to_radians().tan();
        let (r0, r1) = (0.5, 20.5);
        let h1 = 20.0 / tan;
        for far in [0.0, 1e3] {
            let origin = DVec3::new(far, -0.7 * far, 0.3 * far);
            let frame = Frame {
                origin,
                x: DVec3::X,
                y: DVec3::Z,
            };
            let tool = revolve(
                &Profile {
                    loops: vec![polygon(
                        &[v(0.0, 0.0), v(r0, 0.0), v(r1, h1), v(0.0, h1)],
                        1,
                    )],
                },
                &frame,
                Sweep::Full,
                2,
                &TOL,
                &Budget::DEFAULT,
            )
            .unwrap();
            let (z0, z1) = (0.25 * h1, 0.75 * h1);
            let (a, b) = (r0 + 5.0, r0 + 15.0);
            let frustum = PI * (z1 - z0) / 3.0 * (a * a + a * b + b * b);
            for (share, min, size) in [
                (0.5, [0.0, -22.0, z0], [22.0, 44.0, z1 - z0]),
                (0.25, [0.0, 0.0, z0], [22.0, 22.0, z1 - z0]),
            ] {
                let plate =
                    Solid::cuboid(origin + DVec3::from(min), DVec3::from(size), 1, &TOL).unwrap();
                let name = format!("{half}° at {far}, {share}");
                let within = 1e-12 * (plate.volume() + tool.volume()) * (1.0 + far);
                let results = all_four(&plate, &tool, within);
                volumes(&name, &plate, &tool, &results, share * frustum, within);
                for (k, solid) in results.iter().enumerate() {
                    exact(&format!("{name} {k}"), solid, 22.0 + far);
                }
            }
        }
    }
}

/// A ball of radius `r` centred on `z` at height `h`.
fn ball(r: f64, h: f64) -> Solid {
    let c = v(0.0, h);
    let half = Loop {
        segments: vec![
            arc(c, v(0.0, h - r), v(r, h), 0),
            arc(c, v(r, h), v(0.0, h + r), 0),
            Segment::line(v(0.0, h + r), v(0.0, h - r), 1).unwrap(),
        ],
    };
    revolve(
        &Profile { loops: vec![half] },
        &Z,
        Sweep::Full,
        2,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap()
}

/// `a`'s four results with `b`, each passing, their volumes keeping the
/// identities within the fit tolerance over the operands' areas (some
/// cuts traced).
fn four_kept(name: &str, a: &Solid, b: &Solid) -> [Solid; 4] {
    let within = TOL.fit() * (a.area() + b.area()) / 5.0;
    let results = all_four(a, b, within);
    let both = results[1].volume();
    volumes(name, a, b, &results, both, within);
    results
}

#[test]
fn coaxial_shortcuts_that_fail_are_tried_again_as_before() {
    // Walls on one axis certified rather than refined keep their pieces
    // large, and cuts of quadrics on one axis in their parallels leave
    // bands fanned from a far corner: some results refinement and
    // tracing got right failed from them. Those are tried again as
    // before (and still exact where the cuts are).
    //
    // A cone and a stack a ten-millionth off its axis, meeting where the
    // stack's top cuts the cone: its pieces 1e-7 from the cone's seam
    // vertex left a sliver (`EdgeNeighbours`).
    let a = turned(
        &[v(0.0, 0.125), v(1.125, 0.125), v(1.5, 2.125), v(0.0, 2.125)],
        1,
    );
    let b = moved(
        &turned(
            &[
                v(0.0, -1.5),
                v(2.0, -1.5),
                v(1.625, -0.5),
                v(1.375, 0.25),
                v(0.0, 0.25),
            ],
            2,
        ),
        |p| p + DVec3::X * 1e-7,
    );
    let (r0, r1) = (1.125, 1.125 + 0.1875 * 0.125);
    let slab = PI * 0.125 / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1);
    // Refined as before, every cut is still a plane's on the cone, but
    // the triangles at the two vertices a ten-millionth apart are `1e-8`
    // off it.
    let both = run(&a, &b, Op::Intersection);
    exact_to("slab", &both, 3.0, 1e-8);
    assert!(
        (both.volume() - slab).abs() <= 1e-8 * slab,
        "{}",
        both.volume()
    );
    four_kept("hair apart", &a, &b);
    // A ring whose corner is a hundredth outside a cone's wall, the cuts
    // a fiftieth apart about it (`Hull`).
    let a = turned(
        &[
            v(0.0, -1.375),
            v(1.375, -1.375),
            v(0.875, -1.0),
            v(2.0, 0.75),
            v(1.25, 2.0),
            v(0.0, 2.0),
        ],
        1,
    );
    let ring = turned(
        &[
            v(1.625, 0.625),
            v(0.5, 0.875),
            v(0.125, 0.125),
            v(1.125, -0.625),
            v(1.25, 0.0),
        ],
        2,
    );
    four_kept("ring", &a, &ring);
    // A ball through a ring's inner cones: the bands between the cut
    // and the ball's polar cap fanned from the cap's corners, with
    // nothing to halve (`TooComplex`).
    let ring = turned(
        &[
            v(1.75, -0.5),
            v(0.625, -0.25),
            v(0.75, -0.625),
            v(1.25, -1.25),
            v(1.875, -1.375),
        ],
        2,
    );
    four_kept("ball", &ball(1.125, -0.625), &ring);
    assert_deterministic(|| run(&ball(1.125, -0.625), &ring, Op::Union).into_mesh());
}
