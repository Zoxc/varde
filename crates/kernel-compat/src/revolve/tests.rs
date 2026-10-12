#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::collections::BTreeSet;
use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

use glam::{DVec2, DVec3};

use super::*;
use crate::Stripped;
use crate::mesh::{CheckError, FaceKey, PartKey};
use crate::par::assert_deterministic;
use crate::profile::tests::{arc, circle, polygon, rect, reversed};
use crate::test_rng::Rng;
use crate::{Budget, Display, Op, boolean, extrude};

mod random;
mod refusals;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn profile(loops: Vec<Loop>) -> Profile {
    Profile { loops }
}

/// The frame with its axis along `z` through the origin, the profile in
/// the `xz` plane.
const Z: Frame = Frame {
    origin: DVec3::ZERO,
    x: DVec3::X,
    y: DVec3::Z,
};

fn random_frame(rng: &mut Rng, extent: f64) -> Frame {
    let y = rng.direction();
    Frame {
        origin: rng.point(extent),
        x: y.any_orthonormal_vector(),
        y,
    }
}

/// `∮ x² dy / 2` and `∮ x ds` round the profile, and its area, by
/// Simpson's rule on each segment's parameter: a turn through θ sweeps
/// θ times the first (Pappus), its walls' area is θ times the second.
fn moments(profile: &Profile) -> (f64, f64, f64) {
    let (mut volume, mut walls, mut area) = (0.0, 0.0, 0.0);
    for lp in &profile.loops {
        for seg in &lp.segments {
            let steps = 20_000;
            let (mut sv, mut sw, mut sa) = (0.0, 0.0, 0.0);
            for i in 0..=steps {
                let weight = if i == 0 || i == steps {
                    1.0
                } else if i % 2 == 1 {
                    4.0
                } else {
                    2.0
                };
                let (p, d) = seg.conic.eval_deriv(i as f64 / steps as f64);
                sv += weight * p.x * p.x * d.y;
                sw += weight * p.x * d.length();
                sa += weight * (p - seg.conic.p0).perp_dot(d);
            }
            let h = 1.0 / (3.0 * steps as f64);
            volume += 0.5 * sv * h;
            walls += sw * h;
            area += 0.5 * sa * h + 0.5 * seg.conic.p0.perp_dot(seg.conic.p1);
        }
    }
    (volume, walls, area)
}

/// The angle a sweep turns through.
fn angle(sweep: Sweep) -> f64 {
    match sweep {
        Sweep::Full => TAU,
        Sweep::Part { from, to } => to - from,
    }
}

/// A profile to revolve, and what to expect of it.
struct Shape {
    name: &'static str,
    profile: Profile,
    /// Has fitted faces (tori, poles), so volumes are good to the area
    /// times half the fit tolerance; its smallest radius of curvature,
    /// which bounds how far off the area is.
    fitted: Option<f64>,
    /// Touches the axis at a lone vertex: part turns only.
    part_only: bool,
}

fn shape(name: &'static str, loops: Vec<Loop>, fitted: Option<f64>) -> Shape {
    Shape {
        name,
        profile: profile(loops),
        fitted,
        part_only: false,
    }
}

/// The shapes the tests revolve: the plan's disc, washer, cylinder, cone,
/// sphere and torus, a lathe profile of lines and arcs, and profiles with
/// edges on the axis, holes, and a vertex alone on it.
fn shapes() -> Vec<Shape> {
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    // A turned part: base, wall, chamfer, neck, a round (a quarter
    // torus), a shoulder, a pin and a ball end, back down the axis.
    let lathe = Loop {
        segments: vec![
            line(v(0.0, 0.0), v(6.0, 0.0), 1),
            line(v(6.0, 0.0), v(6.0, 3.0), 2),
            line(v(6.0, 3.0), v(5.0, 4.0), 3),
            line(v(5.0, 4.0), v(5.0, 6.0), 4),
            arc(v(4.0, 6.0), v(5.0, 6.0), v(4.0, 7.0), 5),
            line(v(4.0, 7.0), v(2.0, 7.0), 6),
            line(v(2.0, 7.0), v(2.0, 9.0), 7),
            arc(v(0.0, 9.0), v(2.0, 9.0), v(0.0, 11.0), 8),
            line(v(0.0, 11.0), v(0.0, 0.0), 9),
        ],
    };
    // A groove: a concave quarter round between a wall and a floor.
    let groove = Loop {
        segments: vec![
            line(v(2.0, 0.0), v(8.0, 0.0), 1),
            line(v(8.0, 0.0), v(8.0, 2.0), 2),
            arc(v(8.0, 4.0), v(8.0, 2.0), v(6.0, 4.0), 3),
            line(v(6.0, 4.0), v(2.0, 4.0), 4),
            line(v(2.0, 4.0), v(2.0, 0.0), 5),
        ],
    };
    let ball = Loop {
        segments: vec![
            arc(v(0.0, 0.0), v(0.0, -2.0), v(2.0, 0.0), 1),
            arc(v(0.0, 0.0), v(2.0, 0.0), v(0.0, 2.0), 1),
            line(v(0.0, 2.0), v(0.0, -2.0), 2),
        ],
    };
    // A hollow ball: between spheres of radius 3 and 2, along the axis
    // between them.
    let shell = Loop {
        segments: vec![
            arc(v(0.0, 0.0), v(0.0, -3.0), v(3.0, 0.0), 1),
            arc(v(0.0, 0.0), v(3.0, 0.0), v(0.0, 3.0), 1),
            line(v(0.0, 3.0), v(0.0, 2.0), 2),
            arc(v(0.0, 0.0), v(0.0, 2.0), v(2.0, 0.0), 3),
            arc(v(0.0, 0.0), v(2.0, 0.0), v(0.0, -2.0), 3),
            line(v(0.0, -2.0), v(0.0, -3.0), 4),
        ],
    };
    vec![
        shape("disc", vec![rect(v(0.0, 0.0), v(5.0, 0.5), 1)], None),
        shape("washer", vec![rect(v(2.0, 0.0), v(3.0, 1.0), 1)], None),
        shape("cylinder", vec![rect(v(0.0, 0.0), v(3.0, 5.0), 1)], None),
        shape(
            "cone",
            vec![polygon(&[v(0.0, 0.0), v(4.0, 0.0), v(0.0, 3.0)], 1)],
            Some(1.0),
        ),
        shape(
            "hollow frustum",
            vec![polygon(
                &[v(1.0, 0.0), v(4.0, 0.0), v(3.0, 2.0), v(1.0, 2.0)],
                1,
            )],
            None,
        ),
        shape("sphere", vec![ball], Some(2.0)),
        shape("hollow ball", vec![shell], Some(2.0)),
        shape(
            "torus",
            vec![circle(v(10.0, 0.0), 2.0, 1, false)],
            Some(2.0),
        ),
        shape("lathe profile", vec![lathe], Some(1.0)),
        shape("groove", vec![groove], Some(2.0)),
        shape(
            "ring with a round hole",
            vec![
                rect(v(4.0, -2.0), v(10.0, 2.0), 1),
                circle(v(7.0, 0.0), 1.0, 5, true),
            ],
            Some(1.0),
        ),
        // A double cone hollowed out of a cylinder about its axis: the
        // notch's ends on the axis are its apexes.
        shape(
            "cavity between axis edges",
            vec![polygon(
                &[
                    v(0.0, 0.0),
                    v(4.0, 0.0),
                    v(4.0, 4.0),
                    v(0.0, 4.0),
                    v(0.0, 3.0),
                    v(2.0, 2.0),
                    v(0.0, 1.0),
                ],
                1,
            )],
            Some(1.0),
        ),
        Shape {
            part_only: true,
            ..shape(
                "diamond on the axis",
                vec![polygon(
                    &[v(0.0, 0.0), v(2.0, -1.0), v(4.0, 0.0), v(2.0, 1.0)],
                    1,
                )],
                Some(1.0),
            )
        },
    ]
}

/// The sweeps the tests turn through: full, the plan's 30°, 90° and 270°,
/// and one from −45° to 60°.
fn sweeps() -> [Sweep; 5] {
    let deg = |a: f64| a.to_radians();
    [
        Sweep::Full,
        Sweep::Part {
            from: 0.0,
            to: deg(30.0),
        },
        Sweep::Part {
            from: 0.0,
            to: deg(90.0),
        },
        Sweep::Part {
            from: 0.0,
            to: deg(270.0),
        },
        Sweep::Part {
            from: deg(-45.0),
            to: deg(60.0),
        },
    ]
}

/// Revolves `shape` and checks the solid: `check` with face tags, its
/// volume by Pappus (within `1e-10` relative for exact faces, plus the
/// area times half the fit tolerance with fitted ones) and its area
/// (within `1e-10` relative, or `4·A·fit/2` over the smallest radius
/// with fitted faces), refused turned inside out, drawn. Gives it.
fn revolved(shape: &Shape, frame: &Frame, sweep: Sweep, tol: &Tolerance) -> Solid {
    let solid = revolve(&shape.profile, frame, sweep, 7, tol, &Budget::DEFAULT)
        .unwrap_or_else(|e| panic!("{} {sweep:?} at {:e}: {e:?}", shape.name, tol.fit()));
    let theta = angle(sweep);
    let (moment, walls, region) = moments(&shape.profile);
    let volume = theta * moment;
    let mut area = theta * walls;
    if !matches!(sweep, Sweep::Full) {
        area += 2.0 * region;
    }
    let mesh = solid.mesh().clone();
    mesh.check(tol).unwrap();
    mesh.check_faces(tol).unwrap();
    // Rounding far out.
    let floor = 1e-13 * area * frame.origin.length();
    let (volume_slack, area_slack) = match shape.fitted {
        Some(radius) => {
            let half = 0.5 * tol.fit() * area;
            (half, (4.0 * half / radius).max(1e-6 * area))
        }
        None => (0.0, 0.0),
    };
    let (sv, sa) = (solid.volume(), solid.area());
    let what = format!("{} {sweep:?} at {:e}", shape.name, tol.fit());
    assert!(
        (sv - volume).abs() <= 1e-10 * volume + volume_slack + floor,
        "{what}: volume {sv} for {volume} ({:e})",
        (sv - volume) / volume
    );
    assert!(
        (sa - area).abs() <= 1e-10 * area + area_slack + floor,
        "{what}: area {sa} for {area} ({:e})",
        (sa - area) / area
    );
    let turned = crate::sweep::tests::inside_out(&mesh);
    assert!(matches!(
        Solid::new(turned, tol),
        Err(KernelError::Invalid(CheckError::InsideOut(_)))
    ));
    solid.tessellate(&Display::default()).unwrap();
    solid
}

const TOL: Tolerance = Tolerance::DEFAULT;

/// Whether a test over [`shapes`] and [`sweeps`] runs shape `i` through
/// sweep `j`: every pair under `VARDE_TESTS=full`, two sweeps a shape by
/// turns by default (every sweep for some shapes, every shape twice).
fn runs(i: usize, j: usize) -> bool {
    varde_testing::full() || j == i % 5 || j == (i + 2) % 5
}

#[test]
fn shapes_are_solids_of_their_volume_and_area() {
    for (i, shape) in shapes().iter().enumerate() {
        for (j, sweep) in sweeps().into_iter().enumerate() {
            if shape.part_only && sweep == Sweep::Full || !runs(i, j) {
                continue;
            }
            revolved(shape, &Z, sweep, &TOL);
        }
    }
}

#[test]
fn shapes_anywhere_at_any_tolerance() {
    let mut rng = Rng::new(41);
    for (i, shape) in shapes().iter().enumerate() {
        for (j, sweep) in sweeps().into_iter().enumerate() {
            if shape.part_only && sweep == Sweep::Full {
                continue;
            }
            // Drawn for every pair, so each runs in the same frame either
            // way.
            let frame = random_frame(&mut rng, 1e3);
            if !runs(i, j) {
                continue;
            }
            let tol = Tolerance::new([1e-2, 1e-3, 1e-4][(i + j) % 3]).unwrap();
            revolved(shape, &frame, sweep, &tol);
        }
    }
}

#[test]
fn exact_faces_are_exact() {
    // A frustum with a hole and a washer: planes and cones only, no pole.
    let deg = |a: f64| a.to_radians();
    for shape in shapes().iter().filter(|s| s.fitted.is_none()) {
        let solid = revolved(shape, &Z, Sweep::Full, &TOL);
        for f in solid.mesh().faces() {
            assert!(!matches!(f.surface, Surface::Free), "{}", shape.name);
        }
        let part = Sweep::Part {
            from: deg(10.0),
            to: deg(100.0),
        };
        revolved(shape, &Z, part, &TOL);
    }
}

#[test]
fn part_turns_start_and_end_where_asked() {
    // The start cap lies in the plane at `from`, facing back; the end
    // cap at `to`, facing on.
    let shape = &shapes()[1];
    let deg = |a: f64| a.to_radians();
    for (from, to) in [(0.0, 30.0), (-45.0, 60.0), (90.0, 180.0), (200.0, 470.0)] {
        let sweep = Sweep::Part {
            from: deg(from),
            to: deg(to),
        };
        let solid = revolved(shape, &Z, sweep, &TOL);
        for face in solid.mesh().faces() {
            let (at, back) = match face.name.part {
                FacePart::StartCap => (deg(from), true),
                FacePart::EndCap => (deg(to), false),
                _ => continue,
            };
            let Surface::Plane { n, d } = face.surface else {
                panic!("an end isn't flat");
            };
            // `x` turned towards `x × y = −y` (world) by `at`.
            let way = DVec3::new(at.cos(), -at.sin(), 0.0).cross(DVec3::Z);
            let expect = if back { -way } else { way };
            assert!((n - expect).length() < 1e-12, "{n} for {expect}");
            assert!(d.abs() < 1e-12);
        }
    }
}

#[test]
fn faces_are_named_per_curve() {
    // The lathe profile: one face per segment's curve, the axis making
    // none; a part turn adds its ends. Two collinear lines are one face,
    // the second's key an alias; a circle's arcs one face.
    let shape = &shapes()[8];
    let keys = |solid: &Solid| -> BTreeSet<FaceKey> {
        solid.mesh().faces().iter().map(|f| f.name.key()).collect()
    };
    let side = |curve| FaceKey {
        feature: 7,
        part: PartKey::Side { curve },
        instance: 0,
    };
    let full = revolved(shape, &Z, Sweep::Full, &TOL);
    let expect: BTreeSet<FaceKey> = (1..=8).map(side).collect();
    assert_eq!(keys(&full), expect);
    let part = revolved(shape, &Z, Sweep::Part { from: 0.0, to: 1.0 }, &TOL);
    let mut expect = expect;
    for part in [PartKey::StartCap, PartKey::EndCap] {
        expect.insert(FaceKey {
            feature: 7,
            part,
            instance: 0,
        });
    }
    assert_eq!(keys(&part), expect);
    // The base drawn as two lines (curves 1 and 10), a circle of four
    // arcs of one curve and four of separate curves.
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    let split = Loop {
        segments: vec![
            line(v(0.0, 0.0), v(2.0, 0.0), 1),
            line(v(2.0, 0.0), v(5.0, 0.0), 10),
            line(v(5.0, 0.0), v(5.0, 2.0), 2),
            line(v(5.0, 2.0), v(0.0, 2.0), 3),
            line(v(0.0, 2.0), v(0.0, 0.0), 4),
        ],
    };
    let shape = Shape {
        name: "split base",
        profile: profile(vec![split]),
        fitted: None,
        part_only: false,
    };
    let solid = revolved(&shape, &Z, Sweep::Full, &TOL);
    assert_eq!(keys(&solid), [1, 2, 3].map(side).into());
    let aliases: BTreeSet<FaceKey> = (0..solid.mesh().faces().len() as u32)
        .flat_map(|f| solid.mesh().face_aliases(f).collect::<Vec<_>>())
        .collect();
    assert_eq!(aliases, [side(10)].into());
    let mut arcs = circle(v(10.0, 0.0), 2.0, 1, false);
    let torus = Shape {
        name: "torus",
        profile: profile(vec![arcs.clone()]),
        fitted: Some(2.0),
        part_only: false,
    };
    assert_eq!(
        keys(&revolved(&torus, &Z, Sweep::Full, &TOL)),
        [side(1)].into()
    );
    for (i, seg) in arcs.segments.iter_mut().enumerate() {
        seg.curve = 20 + i as u64;
    }
    let torus = Shape {
        profile: profile(vec![arcs]),
        ..torus
    };
    // Fitted faces claim no surface, so separate curves stay apart.
    assert_eq!(
        keys(&revolved(&torus, &Z, Sweep::Full, &TOL)),
        (20..24).map(side).collect()
    );
}

#[test]
fn segment_numbers_count_a_curves_segments() {
    // A curve drawn as two separate segments (a line, interrupted):
    // `segment` counts them in profile order, as an extrude's walls.
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    let lp = Loop {
        segments: vec![
            line(v(1.0, 0.0), v(4.0, 0.0), 3),
            line(v(4.0, 0.0), v(4.0, 1.0), 4),
            line(v(4.0, 1.0), v(1.0, 2.0), 3),
            line(v(1.0, 2.0), v(1.0, 0.0), 5),
        ],
    };
    let solid = revolve(
        &profile(vec![lp]),
        &Z,
        Sweep::Full,
        7,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap();
    let parts: BTreeSet<FacePart> = solid.mesh().faces().iter().map(|f| f.name.part).collect();
    let side = |curve, segment| FacePart::Side { curve, segment };
    assert_eq!(
        parts,
        [side(3, 0), side(3, 1), side(4, 0), side(5, 0)].into()
    );
}

#[test]
fn forms_say_what_each_face_is() {
    let shape = &shapes()[8];
    let solid = revolved(shape, &Z, Sweep::Full, &TOL);
    for face in solid.mesh().faces() {
        let FacePart::Side { curve, .. } = face.name.part else {
            panic!("a full turn has no ends");
        };
        let ok = match (curve, face.form) {
            (1 | 6, Form::Plane { n, .. }) => n.z.abs() == 1.0,
            (2 | 4 | 7, Form::Cylinder { radius, .. }) => [6.0, 5.0, 2.0].contains(&radius),
            (3, Form::Cone { apex, cos, sin, .. }) => {
                (apex - DVec3::new(0.0, 0.0, 9.0)).length() < 1e-12 && (cos - sin).abs() < 1e-15
            }
            (5, Form::Torus { major, minor, .. }) => major == 4.0 && minor == 1.0,
            (8, Form::Sphere { centre, radius }) => centre.z == 9.0 && (radius - 2.0).abs() < 1e-14,
            _ => false,
        };
        assert!(ok, "curve {curve}: {:?}", face.form);
    }
}

#[test]
fn spheres_drawn_a_hair_off_the_axis_are_solids() {
    // A wide band of a sphere (170°, weight 0.087) round a centre a little
    // off the axis, closed by a wall: its strips stray about 5.8 times as
    // far as the centre from the sphere on the axis, so only a centre
    // within a quarter of the weight's share of the resolution is taken
    // for the sphere's (it was refused by the tag check from a fifth of
    // the resolution off). Further off, a torus or a lemon, fitted.
    let res = TOL.resolution();
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    for (off, sphere) in [
        (0.0, true),
        (0.02 * res, true),
        (-0.02 * res, true),
        (0.2 * res, false),
        (0.99 * res, false),
        (-0.5 * res, false),
    ] {
        let centre = v(off, 0.0);
        let (s, c) = 85f64.to_radians().sin_cos();
        let (a, b) = (centre + v(c, -s) * 5.0, centre + v(c, s) * 5.0);
        let wall = 0.2 * a.x;
        let band = Shape {
            name: "sphere band",
            profile: profile(vec![Loop {
                segments: vec![
                    arc(centre, a, b, 1),
                    line(b, v(wall, b.y), 2),
                    line(v(wall, b.y), v(wall, a.y), 3),
                    line(v(wall, a.y), a, 4),
                ],
            }]),
            fitted: Some(5.0),
            part_only: false,
        };
        for sweep in [Sweep::Full, sweeps()[3]] {
            let solid = revolved(&band, &Z, sweep, &TOL);
            let face = solid
                .mesh()
                .faces()
                .iter()
                .find(|f| matches!(f.name.part, FacePart::Side { curve: 1, .. }))
                .unwrap();
            assert_eq!(
                matches!(face.form, Form::Sphere { .. }),
                sphere,
                "{off:e}: {:?}",
                face.form
            );
        }
    }
}

#[test]
fn revolves_are_the_same_on_any_thread_count() {
    let mut rng = Rng::new(43);
    let frame = random_frame(&mut rng, 1e3);
    let shapes = shapes();
    for (i, sweep) in [Sweep::Full, sweeps()[4]].into_iter().enumerate() {
        let shape = &shapes[[8, 10][i]];
        assert_deterministic(|| {
            let solid = revolve(&shape.profile, &frame, sweep, 7, &TOL, &Budget::DEFAULT).unwrap();
            (solid.volume(), solid)
        });
    }
}

#[test]
fn lathe_pieces_grow_for_fitted_faces_only() {
    // One split for the whole solid: a cylinder keeps the four quarters,
    // a torus makes every face finer.
    let count =
        |shape: &Shape, tol: &Tolerance| revolved(shape, &Z, Sweep::Full, tol).mesh().tris().len();
    let shapes = shapes();
    let tight = Tolerance::new(1e-5).unwrap();
    assert_eq!(count(&shapes[2], &TOL), count(&shapes[2], &tight));
    assert!(count(&shapes[7], &tight) > 2 * count(&shapes[7], &TOL));
}

#[test]
fn a_spindle_and_a_lemon() {
    // Arcs whose circles reach across the axis: a lemon (centre across
    // the axis, both ends on it) and a spindle torus's outside (centre
    // off the axis, nearer it than the radius), cut off by a cylinder.
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    let r: f64 = 5.0;
    let c: f64 = -3.0;
    let h = (r * r - c * c).sqrt();
    let lemon = Loop {
        segments: vec![
            arc(v(c, 0.0), v(0.0, -h), v(2.0, 0.0), 1),
            arc(v(c, 0.0), v(2.0, 0.0), v(0.0, h), 1),
            line(v(0.0, h), v(0.0, -h), 2),
        ],
    };
    // From 60° below the outside to 60° above, off the tube's turns,
    // closed by two lines in towards the axis, or by a wall straight down
    // (which meets the arcs in creases only the pencil parts: without it
    // 229 136 patches, past the budget).
    let (c, r) = (v(2.0, 0.0), 3.0);
    let at = |a: f64| c + DVec2::new(a.cos(), a.sin()) * r;
    let (low, high) = (at(-PI / 3.0), at(PI / 3.0));
    let spindle = Loop {
        segments: vec![
            arc(c, low, v(5.0, 0.0), 1),
            arc(c, v(5.0, 0.0), high, 1),
            line(high, v(3.0, 0.0), 2),
            line(v(3.0, 0.0), low, 3),
        ],
    };
    let walled = Loop {
        segments: vec![
            arc(c, low, v(5.0, 0.0), 1),
            arc(c, v(5.0, 0.0), high, 1),
            line(high, low, 2),
        ],
    };
    for (name, lp) in [
        ("lemon", lemon),
        ("spindle", spindle),
        ("walled spindle", walled),
    ] {
        let shape = Shape {
            name,
            profile: profile(vec![lp]),
            fitted: Some(5.0),
            part_only: false,
        };
        for sweep in sweeps() {
            revolved(&shape, &Z, sweep, &TOL);
        }
    }
}

#[test]
fn conics_other_than_circles_are_fitted() {
    // An ellipse's quarter, a parabola and a hyperbola as one segment
    // each, closed down the axis: `Form::Revolved`.
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    for (w, c) in [
        (FRAC_1_SQRT_2, v(3.0, 2.0)),
        (1.0, v(3.0, 2.0)),
        (1.6, v(3.0, 2.0)),
    ] {
        let conic = Conic2::new(v(3.0, 0.0), c, w, v(0.0, 4.0)).unwrap();
        let lp = Loop {
            segments: vec![
                line(v(0.0, 0.0), v(3.0, 0.0), 1),
                Segment { conic, curve: 2 },
                line(v(0.0, 4.0), v(0.0, 0.0), 3),
            ],
        };
        let shape = Shape {
            name: "conic",
            profile: profile(vec![lp]),
            fitted: Some(1.0),
            part_only: false,
        };
        for sweep in [Sweep::Full, sweeps()[2]] {
            let solid = revolved(&shape, &Z, sweep, &TOL);
            assert!(solid.mesh().faces().iter().any(|f| matches!(
                (f.name.part, f.form),
                (FacePart::Side { curve: 2, .. }, Form::Revolved { .. })
            )));
        }
    }
}

#[test]
fn cones_nearly_flat_claim_no_quadric() {
    // A ring's top sloping a few hundred-millionths: a cone whose quadric,
    // on a tilted axis, rounds to a first-order distance far past the
    // resolution (its gradient vanishes with the slope), which the tag
    // check refused. It claims no surface; its form is still the cone.
    let tol = Tolerance::new(1e-4).unwrap();
    let mut rng = Rng::new(53);
    for i in 0..4 {
        let rise = [3e-6, -3e-6, 1e-5, -1e-5][i];
        let shape = Shape {
            name: "nearly flat top",
            profile: profile(vec![polygon(
                &[
                    v(100.0, 0.0),
                    v(200.0, 0.0),
                    v(200.0, 50.0 + rise),
                    v(100.0, 50.0),
                ],
                1,
            )]),
            fitted: None,
            part_only: false,
        };
        let frame = random_frame(&mut rng, 1e3);
        for sweep in [Sweep::Full, sweeps()[4]] {
            let solid = revolved(&shape, &frame, sweep, &tol);
            let top = solid
                .mesh()
                .faces()
                .iter()
                .find(|f| {
                    f.name.part
                        == FacePart::Side {
                            curve: 3,
                            segment: 0,
                        }
                })
                .unwrap();
            assert!(matches!(top.form, Form::Cone { .. }), "{:?}", top.form);
            assert_eq!(top.surface, Surface::Free);
        }
    }
    // Steeper, a cone keeps its quadric.
    let p = profile(vec![polygon(
        &[v(0.0, 0.0), v(200.0, 0.0), v(0.0, 150.0)],
        1,
    )]);
    let frame = random_frame(&mut rng, 1e3);
    let solid = revolve(&p, &frame, Sweep::Full, 7, &tol, &Budget::DEFAULT).unwrap();
    assert!(solid.mesh().faces().iter().any(|f| matches!(
        (f.name.part, f.surface),
        (FacePart::Side { curve: 2, .. }, Surface::Quadric(_))
    )));
}

#[test]
fn vertices_near_the_axis_are_put_on_it() {
    // A cylinder whose axis side is drawn a hair off the axis, either
    // way: within the resolution it is on it.
    let res = TOL.resolution();
    for off in [0.5 * res, -0.5 * res, res] {
        let lp = polygon(&[v(off, 0.0), v(3.0, 0.0), v(3.0, 5.0), v(off, 5.0)], 1);
        let solid = revolve(
            &profile(vec![lp]),
            &Z,
            Sweep::Full,
            7,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap();
        let volume = PI * 9.0 * 5.0;
        assert!((solid.volume() - volume).abs() < 1e-9 * volume);
        // No face for the axis side.
        assert!(solid.mesh().faces().iter().all(|f| f.name.part
            != FacePart::Side {
                curve: 4,
                segment: 0
            }));
    }
}

#[test]
fn a_profile_running_the_wrong_way_is_refused() {
    let lp = reversed(&rect(v(1.0, 0.0), v(3.0, 1.0), 1));
    assert_eq!(
        revolve(
            &profile(vec![lp]),
            &Z,
            Sweep::Full,
            7,
            &TOL,
            &Budget::DEFAULT
        )
        .stripped(),
        Err(KernelError::Profile(ProfileError::Nesting(0)))
    );
}

/// The lens between two circular arcs from `p` to `q`, each turning
/// through `2·half` (radians), bulging either side of the chord.
fn lens(p: DVec2, q: DVec2, half: f64, curve: u64) -> Loop {
    let m = (p + q) / 2.0;
    let d = q - p;
    let right = DVec2::new(d.y, -d.x).normalize();
    let t = 0.5 * d.length() / half.tan();
    Loop {
        segments: vec![
            arc(m - right * t, p, q, curve),
            arc(m + right * t, q, p, curve + 1),
        ],
    }
}

/// The crease profiles turned all the way round, cut by a box through a
/// crease's ring, a cylinder round the axis through it and a drill across
/// it: where the pencil left a ring's patches whole, repair and the
/// boolean's refinement meet them. Every result is checked with its face
/// tags, and the volumes add up: `a ∪ b` and `a ∩ b` to `a` and `b`, `a −
/// b` to `a` less `a ∩ b`.
#[test]
fn crease_revolves_through_booleans() {
    let mut ok = 0;
    // Slow: quick runs only the first three with the box.
    let shapes = crease_shapes();
    let shapes = if !varde_testing::full() {
        &shapes[..3]
    } else {
        &shapes[..]
    };
    for (shape, _) in shapes {
        let a = revolve(&shape.profile, &Z, Sweep::Full, 7, &TOL, &Budget::DEFAULT).unwrap();
        // The crease: the profile's first corner, at radius `r`, height `h`.
        let corner = shape.profile.loops[0].segments[0].conic.p0;
        let (r, h) = (corner.x, corner.y);
        let box_through = Solid::cuboid(
            DVec3::new(r - 0.4, -0.3, h - 0.35),
            DVec3::new(0.8, 0.6, 0.7),
            8,
            &TOL,
        )
        .unwrap();
        let round_it =
            Solid::cylinder(DVec3::new(0.0, 0.0, h - 0.5), r * 1.03, 1.0, 8, &TOL).unwrap();
        // Along `x` through the ring at `y = 0`.
        let across = Frame {
            origin: DVec3::ZERO,
            x: DVec3::Y,
            y: DVec3::Z,
        };
        let drill = extrude(
            &profile(vec![circle(v(0.1, h + 0.05), 0.3, 1, false)]),
            &across,
            r - 1.0,
            r + 1.0,
            8,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap();
        let tools = [("box", box_through), ("round", round_it), ("drill", drill)];
        let tools = if !varde_testing::full() {
            &tools[..1]
        } else {
            &tools[..]
        };
        for (tool, b) in tools {
            let what = format!("{} with the {tool}", shape.name);
            let volumes = [Op::Union, Op::Intersection, Op::Difference].map(|op| {
                let result = boolean(&a, b, op, &TOL, &Budget::DEFAULT).ok()?;
                result.mesh().check(&TOL).unwrap();
                result.mesh().check_faces(&TOL).unwrap();
                ok += 1;
                Some(result.volume())
            });
            let (va, vb) = (a.volume(), b.volume());
            let within = TOL.fit() * (a.area() + b.area()) / 5.0 + 1e-9;
            if let [Some(u), Some(i), _] = volumes {
                assert!(
                    (u + i - va - vb).abs() <= within,
                    "{what}: {u} + {i} vs {va} + {vb}"
                );
            }
            if let [_, Some(i), Some(d)] = volumes {
                assert!((d - (va - i)).abs() <= within, "{what}: {d} vs {va} - {i}");
            }
        }
    }
    // 72 of the 81 work; the rest are refused as `Invalid` (thin tips and
    // the cylinder brushing the rings).
    assert!(ok >= 72 || !varde_testing::full(), "{ok} results");
}

/// Profiles whose corners are creases where both faces leave the ring on
/// one side of its plane and on one side of the cylinder over it (or one
/// along it), with a ceiling on a full turn's patches (about twice those
/// measured at `1e-1` to `1e-5`); and a dovetail, whose acute corner the
/// plane parts, as a control.
fn crease_shapes() -> Vec<(Shape, usize)> {
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    let deg = |a: f64| a.to_radians();
    let lines =
        |name, points: &[DVec2], ceiling| (shape(name, vec![polygon(points, 1)], None), ceiling);
    // The wedge's sides leave its corner at 26.565° and 36.565°.
    let wedge = 3.0 + 6.0 * (deg(36.565).tan() - deg(26.565).tan());
    // A round just past its turn against a wall straight down.
    let centre = v(5.3, 1.0);
    let at = |a: f64| centre + DVec2::new(a.cos(), a.sin()) * 2.0;
    let (top, end) = (at(deg(100.0)), at(deg(200.0)));
    vec![
        // The triangle's inner corners: a wall up, cones up and out.
        lines("triangle", &[v(2.0, 0.0), v(5.0, 1.0), v(2.0, 2.0)], 304),
        // The same turned onto an outer wall.
        lines(
            "triangle flipped",
            &[v(5.0, 0.0), v(5.0, 2.0), v(2.0, 1.0)],
            192,
        ),
        // Two lines leaving a corner up and out.
        lines(
            "one quadrant",
            &[v(2.0, 0.0), v(6.0, 1.0), v(4.0, 3.0)],
            608,
        ),
        lines(
            "wedge 10°",
            &[v(2.0, 0.0), v(8.0, 3.0), v(8.0, wedge)],
            1_824,
        ),
        (
            shape(
                "lens 40°",
                vec![lens(v(2.0, 0.0), v(6.0, 2.0), deg(20.0), 1)],
                Some(6.5),
            ),
            2_048,
        ),
        (
            shape(
                "lens 10°",
                vec![lens(v(2.0, 0.0), v(6.0, 2.0), deg(5.0), 1)],
                Some(25.0),
            ),
            5_632,
        ),
        // A D: an arc from the bottom of its circle out and up, a wall
        // down from its top.
        (
            shape(
                "D",
                vec![Loop {
                    segments: vec![
                        arc(v(3.0, 2.0), v(3.0, 0.0), v(5.0, 2.0), 1),
                        arc(v(3.0, 2.0), v(5.0, 2.0), v(3.0, 4.0), 1),
                        line(v(3.0, 4.0), v(3.0, 0.0), 2),
                    ],
                }],
                Some(2.0),
            ),
            4_352,
        ),
        (
            shape(
                "round past its turn",
                vec![Loop {
                    segments: vec![
                        arc(centre, top, end, 1),
                        line(end, v(top.x, end.y), 2),
                        line(v(top.x, end.y), top, 3),
                    ],
                }],
                Some(2.0),
            ),
            4_096,
        ),
        lines(
            "dovetail",
            &[
                v(2.0, 0.0),
                v(8.0, 0.0),
                v(6.0, 3.0),
                v(9.0, 3.0),
                v(9.0, 4.0),
                v(2.0, 4.0),
            ],
            192,
        ),
    ]
}

#[test]
fn creases_are_parted_by_the_pencil() {
    // At a ring where two faces meet at a crease into one quadrant of the
    // meridian plane, neither the plane through the ring nor the cylinder
    // over it parts the patches either side, but a member of their pencil
    // does. Without it repair split the rings until their arcs were
    // straight to the resolution: the triangle took 14 192 patches at
    // `1e-1` and 229 232 at `1e-3`, and finer fits ran out of budget.
    // Quick, each shape at one fit and turn, by turns.
    let mut rng = Rng::new(96);
    let frame = random_frame(&mut rng, 1e3);
    let mut fits = vec![1e-2, 1e-3, 1e-4];
    if varde_testing::full() {
        fits.push(1e-5);
    }
    for (i, (shape, ceiling)) in crease_shapes().into_iter().enumerate() {
        for (j, &fit) in fits.iter().enumerate() {
            let tol = Tolerance::new(fit).unwrap();
            for (k, (frame, sweep)) in [
                (&Z, Sweep::Full),
                (&Z, Sweep::Part { from: 0.3, to: 2.0 }),
                (&frame, Sweep::Full),
            ]
            .into_iter()
            .enumerate()
            {
                if !varde_testing::full() && (j != i % 3 || k != i / 3 % 3) {
                    continue;
                }
                let patches = revolved(&shape, frame, sweep, &tol).mesh().tris().len();
                assert!(
                    patches <= ceiling,
                    "{} {sweep:?} at {fit:e}: {patches} patches",
                    shape.name
                );
            }
        }
    }
    // Repair runs on the full turns (the stations' vertex pairs, the
    // thin tips): the same bits at 1 and 8 threads.
    let shapes = crease_shapes();
    assert_deterministic(|| {
        shapes
            .iter()
            .map(|(shape, _)| {
                let solid = revolve(
                    &shape.profile,
                    &frame,
                    Sweep::Full,
                    7,
                    &TOL,
                    &Budget::DEFAULT,
                )
                .unwrap();
                (solid.volume(), solid)
            })
            .collect::<Vec<_>>()
    });
}
