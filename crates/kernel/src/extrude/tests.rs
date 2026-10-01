#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::PI;

use glam::{DVec2, DVec3};

use super::*;
use crate::mesh::FacePart;
use crate::par::assert_deterministic;
use crate::profile::tests::{arc, circle, polygon, rect, reversed};
use crate::profile::{Loop, Segment};
use crate::test_rng::Rng;
use crate::{Budget, Display};

const TOL: Tolerance = Tolerance::DEFAULT;

fn profile(loops: Vec<Loop>) -> Profile {
    Profile { loops }
}

fn run(p: &Profile, frame: &Frame, from: f64, to: f64) -> Result<Solid, KernelError> {
    extrude(p, frame, from, to, 9, &TOL, &Budget::DEFAULT)
}

/// Extrudes `p` on the XY plane from `from` to `to`, and checks what
/// holds for every extrude beyond `check` (face tags included): volume
/// and area as the profile's area and perimeter say within `rel`
/// (relative), and that it draws.
fn extruded(p: &Profile, from: f64, to: f64, perimeter: f64, rel: f64) -> Solid {
    let solid = run(p, &Frame::XY, from, to).unwrap();
    let h = to - from;
    let area = p.area();
    let volume = solid.volume();
    assert!(
        (volume - area * h).abs() <= rel * area * h,
        "volume {volume}, not {}",
        area * h
    );
    let surface = 2.0 * area + perimeter * h;
    assert!(
        (solid.area() - surface).abs() <= rel * surface,
        "area {}, not {surface}",
        solid.area()
    );
    solid.tessellate(&Display::default()).unwrap();
    solid
}

fn parts(solid: &Solid) -> Vec<FacePart> {
    solid.mesh().faces().iter().map(|f| f.name.part).collect()
}

fn side(curve: u64, segment: u32) -> FacePart {
    FacePart::Side { curve, segment }
}

/// The slot of length `l` between its centres and radius `r`, round the
/// origin along x: two sides and two ends of two quarter arcs each.
fn slot(l: f64, r: f64) -> Loop {
    let (h, c0, c1) = (l / 2.0, DVec2::new(-l / 2.0, 0.0), DVec2::new(l / 2.0, 0.0));
    let p = |x: f64, y: f64| DVec2::new(x, y);
    Loop {
        segments: vec![
            Segment::line(p(-h, -r), p(h, -r), 0).unwrap(),
            arc(c1, p(h, -r), p(h + r, 0.0), 1),
            arc(c1, p(h + r, 0.0), p(h, r), 1),
            Segment::line(p(h, r), p(-h, r), 2).unwrap(),
            arc(c0, p(-h, r), p(-h - r, 0.0), 3),
            arc(c0, p(-h - r, 0.0), p(-h, -r), 3),
        ],
    }
}

#[test]
fn a_box_is_two_triangles_a_side() {
    let p = profile(vec![rect(DVec2::ZERO, DVec2::new(3.0, 2.0), 0)]);
    let solid = extruded(&p, 0.0, 4.0, 10.0, 1e-14);
    assert_eq!(solid.mesh().tris().len(), 12);
    assert_eq!(solid.mesh().verts().len(), 8);
    assert_eq!(
        parts(&solid),
        [
            FacePart::StartCap,
            FacePart::EndCap,
            side(0, 0),
            side(1, 0),
            side(2, 0),
            side(3, 0)
        ]
    );
    assert!(solid.mesh().faces().iter().all(|f| f.name.feature == 9));
    let b = solid.bounds3().unwrap();
    assert_eq!((b.min, b.max), (DVec3::ZERO, DVec3::new(3.0, 2.0, 4.0)));
}

#[test]
fn a_cylinder_is_exact() {
    for (r, h) in [(2.0, 5.0), (1e-2, 3.0), (400.0, 0.5), (0.3, 1e3)] {
        let p = profile(vec![circle(DVec2::new(1.0, -2.0), r, 0, false)]);
        let solid = extruded(&p, -h, 0.0, 2.0 * PI * r, 1e-12);
        let names = parts(&solid);
        assert_eq!(names[..2], [FacePart::StartCap, FacePart::EndCap]);
        // One face per segment of the one curve.
        assert_eq!(names[2..], [0, 1, 2, 3].map(|s| side(0, s)));
        assert!((solid.volume() - PI * r * r * h).abs() < 1e-12 * PI * r * r * h);
    }
}

#[test]
fn a_plate_with_holes() {
    let p = profile(vec![
        rect(DVec2::ZERO, DVec2::new(100.0, 60.0), 0),
        circle(DVec2::new(25.0, 30.0), 5.0, 4, true),
        circle(DVec2::new(75.0, 30.0), 5.0, 5, true),
        circle(DVec2::new(50.0, 10.0), 3.0, 6, true),
        reversed(&rect(DVec2::new(45.0, 40.0), DVec2::new(55.0, 50.0), 7)),
    ]);
    let perimeter = 320.0 + 2.0 * PI * 13.0 + 40.0;
    let solid = extruded(&p, 0.0, 8.0, perimeter, 1e-12);
    let exact = (6000.0 - PI * 59.0 - 100.0) * 8.0;
    assert!((solid.volume() - exact).abs() < 1e-12 * exact);
}

#[test]
fn a_slot_and_a_d() {
    let (l, r) = (20.0, 3.0);
    let solid = extruded(
        &profile(vec![slot(l, r)]),
        0.0,
        2.0,
        2.0 * l + 2.0 * PI * r,
        1e-12,
    );
    assert!((solid.volume() - (2.0 * r * l + PI * r * r) * 2.0).abs() < 1e-11);

    // A half disc: two quarter arcs and the diameter, which meet the arcs
    // square.
    let c = DVec2::ZERO;
    let (a, top, b) = (DVec2::new(r, 0.0), DVec2::new(0.0, r), DVec2::new(-r, 0.0));
    let d = Loop {
        segments: vec![
            arc(c, a, top, 0),
            arc(c, top, b, 0),
            Segment::line(b, a, 1).unwrap(),
        ],
    };
    let solid = extruded(&profile(vec![d]), 1.0, 4.0, PI * r + 2.0 * r, 1e-12);
    assert!((solid.volume() - PI * r * r / 2.0 * 3.0).abs() < 1e-12);
    // A lens: one quarter arc and its chord.
    let lens = Loop {
        segments: vec![arc(c, a, top, 0), Segment::line(top, a, 1).unwrap()],
    };
    let chord = r * 2f64.sqrt();
    extruded(&profile(vec![lens]), 0.0, 1.0, PI * r / 2.0 + chord, 1e-12);
}

#[test]
fn a_thin_band() {
    // A ring a hundredth of its radius wide: the arcs are halved until the
    // chords of the two circles keep apart.
    for (outer, inner) in [(10.0, 9.9), (10.0, 9.999)] {
        let p = profile(vec![
            circle(DVec2::ZERO, outer, 0, false),
            circle(DVec2::ZERO, inner, 1, true),
        ]);
        let solid = run(&p, &Frame::XY, 0.0, 1.0).unwrap();
        // Measured against the whole disc, whose volume the thin one's is
        // the difference of.
        let (disc, exact) = (PI * outer * outer, PI * (outer * outer - inner * inner));
        assert!((solid.volume() - exact).abs() < 1e-13 * disc);
        let surface = 2.0 * exact + 2.0 * PI * (outer + inner);
        assert!((solid.area() - surface).abs() < 1e-13 * disc);
        // The pieces of a halved arc are walls of its face.
        assert!(solid.mesh().tris().len() > 4 * 8 * 2);
        let walls = [0, 1].map(|c| [0, 1, 2, 3].map(|s| side(c, s)));
        assert_eq!(parts(&solid)[2..], *walls.as_flattened());
    }
}

#[test]
fn sides_are_numbered_per_curve_in_profile_order() {
    // Curve 5 is two sides of the outline and the hole: its walls count
    // on through both loops.
    let mut outline = rect(DVec2::ZERO, DVec2::splat(10.0), 0);
    outline.segments[1].curve = 5;
    outline.segments[3].curve = 5;
    let p = profile(vec![outline, circle(DVec2::splat(5.0), 2.0, 5, true)]);
    let solid = run(&p, &Frame::XY, 0.0, 1.0).unwrap();
    let mut expected = vec![FacePart::StartCap, FacePart::EndCap];
    expected.extend([side(0, 0), side(5, 0), side(2, 0), side(5, 1)]);
    expected.extend((2..6).map(|s| side(5, s)));
    assert_eq!(parts(&solid), expected);
}

#[test]
fn rounded_and_bitten_outlines() {
    // A rounded rectangle: lines tangent to convex quarter arcs.
    let (w, h, r) = (10.0, 6.0, 1.5);
    let p = |x: f64, y: f64| DVec2::new(x, y);
    let rounded = Loop {
        segments: vec![
            Segment::line(p(r, 0.0), p(w - r, 0.0), 0).unwrap(),
            arc(p(w - r, r), p(w - r, 0.0), p(w, r), 1),
            Segment::line(p(w, r), p(w, h - r), 2).unwrap(),
            arc(p(w - r, h - r), p(w, h - r), p(w - r, h), 3),
            Segment::line(p(w - r, h), p(r, h), 4).unwrap(),
            arc(p(r, h - r), p(r, h), p(0.0, h - r), 5),
            Segment::line(p(0.0, h - r), p(0.0, r), 6).unwrap(),
            arc(p(r, r), p(0.0, r), p(r, 0.0), 7),
        ],
    };
    let perimeter = 2.0 * (w + h) - 8.0 * r + 2.0 * PI * r;
    extruded(&profile(vec![rounded]), 0.0, 3.0, perimeter, 1e-12);

    // A square with a half disc bitten out of its top: concave arcs on
    // the outer loop, meeting the top square.
    let (s, b) = (10.0, 3.0);
    let bitten = Loop {
        segments: vec![
            Segment::line(p(0.0, 0.0), p(s, 0.0), 0).unwrap(),
            Segment::line(p(s, 0.0), p(s, s), 1).unwrap(),
            Segment::line(p(s, s), p(5.0 + b, s), 2).unwrap(),
            arc(p(5.0, s), p(5.0 + b, s), p(5.0, s - b), 3),
            arc(p(5.0, s), p(5.0, s - b), p(5.0 - b, s), 3),
            Segment::line(p(5.0 - b, s), p(0.0, s), 4).unwrap(),
            Segment::line(p(0.0, s), p(0.0, 0.0), 5).unwrap(),
        ],
    };
    let solid = extruded(
        &profile(vec![bitten.clone()]),
        0.0,
        2.0,
        4.0 * s - 2.0 * b + PI * b,
        1e-12,
    );
    assert!((solid.volume() - (s * s - PI * b * b / 2.0) * 2.0).abs() < 1e-11);

    // An S: a convex arc running smoothly into a concave one.
    let wave = Loop {
        segments: vec![
            Segment::line(p(0.0, 0.0), p(4.0, 0.0), 0).unwrap(),
            Segment::line(p(4.0, 0.0), p(4.0, 2.0), 1).unwrap(),
            arc(p(3.0, 2.0), p(4.0, 2.0), p(3.0, 3.0), 2),
            arc(p(3.0, 4.0), p(3.0, 3.0), p(2.0, 4.0), 3),
            Segment::line(p(2.0, 4.0), p(0.0, 4.0), 4).unwrap(),
            Segment::line(p(0.0, 4.0), p(0.0, 0.0), 5).unwrap(),
        ],
    };
    extruded(&profile(vec![wave]), 0.0, 1.0, 12.0 + PI, 1e-12);
}

#[test]
fn an_ellipse_and_a_parabola() {
    // A circle's quarter arcs scaled along x: an exact ellipse.
    let (a, b) = (5.0, 2.0);
    let squash = |s: Segment| Segment {
        conic: Conic2::new(
            s.conic.p0 * DVec2::new(a, b),
            s.conic.c * DVec2::new(a, b),
            s.conic.w,
            s.conic.p1 * DVec2::new(a, b),
        )
        .unwrap(),
        curve: s.curve,
    };
    let ellipse = Loop {
        segments: circle(DVec2::ZERO, 1.0, 0, false)
            .segments
            .into_iter()
            .map(squash)
            .collect(),
    };
    let p = profile(vec![ellipse]);
    let solid = run(&p, &Frame::XY, 0.0, 1.0).unwrap();
    assert!((solid.volume() - PI * a * b).abs() < 1e-12 * PI * a * b);

    // A parabolic arch (weight 1) over its chord: area ⅔ of its box.
    let arch = Loop {
        segments: vec![
            Segment {
                conic: Conic2::new(
                    DVec2::new(2.0, 0.0),
                    DVec2::new(0.0, 4.0),
                    1.0,
                    DVec2::new(-2.0, 0.0),
                )
                .unwrap(),
                curve: 0,
            },
            Segment::line(DVec2::new(-2.0, 0.0), DVec2::new(2.0, 0.0), 1).unwrap(),
        ],
    };
    let solid = run(&profile(vec![arch]), &Frame::XY, 0.0, 1.0).unwrap();
    assert!((solid.volume() - 2.0 / 3.0 * 4.0 * 2.0).abs() < 1e-12);
}

#[test]
fn a_tilted_frame_far_out() {
    let axis = DVec3::new(1.0, 2.0, 3.0).normalize();
    let x = axis.any_orthonormal_vector();
    let frame = Frame {
        origin: DVec3::new(3e4, -2e4, 1e4),
        x,
        y: axis.cross(x),
    };
    let p = profile(vec![
        rect(DVec2::new(-10.0, -8.0), DVec2::new(12.0, 9.0), 0),
        circle(DVec2::new(1.0, 1.0), 4.0, 4, true),
    ]);
    let solid = run(&p, &frame, 2.0, 7.0).unwrap();
    let exact = p.area() * 5.0;
    assert!((solid.volume() - exact).abs() < 1e-9 * exact);
    // The end cap lies on its plane, 7 along the normal.
    let end = solid.mesh().faces()[1];
    let Surface::Plane { n, d } = end.surface else {
        panic!("a plane cap");
    };
    let top = frame.origin + frame.normal() * 7.0;
    assert!((n.dot(top) - d).abs() < 1e-9);
    assert!((n - frame.normal()).length() < 1e-15);

    // Axes a little off square and off unit length: the walls' tags still
    // hold, far out.
    let skewed = Frame {
        x: (frame.x + frame.y * 5e-10) * (1.0 + 5e-10),
        ..frame
    };
    let solid = run(&p, &skewed, 2.0, 7.0).unwrap();
    assert_eq!(
        solid.mesh().check_faces(&Tolerance::new(1e-5).unwrap()),
        Ok(())
    );
}

#[test]
fn many_random_outlines() {
    // Outlines with a straight, bulging or dented side between each two
    // corners: those whose sides touch, or meet in a cusp, are refused,
    // the rest have their volumes.
    let mut rng = Rng::new(17);
    let mut built = 0;
    for _ in 0..40 {
        let scale = rng.log_range(1e-1, 1e3);
        let p = profile(vec![random_loop(&mut rng, DVec2::ZERO, scale, 0)]);
        let height = scale * rng.range(0.05, 2.0);
        match run(&p, &Frame::XY, 0.0, height) {
            Ok(solid) => {
                let exact = p.area() * height;
                assert!((solid.volume() - exact).abs() < 1e-12 * scale * scale * height);
                built += 1;
            }
            Err(KernelError::Profile(
                ProfileError::Touching(_) | ProfileError::Nesting | ProfileError::Cusp(..),
            )) => {}
            Err(e) => panic!("{e}: {p:?}"),
        }
    }
    assert!(built > 25, "{built}");
}

#[test]
fn a_plate_with_many_holes() {
    let mut loops = vec![rect(DVec2::ZERO, DVec2::splat(80.0), 0)];
    for i in 0..8 {
        for j in 0..8 {
            let at = DVec2::new(5.0 + 10.0 * i as f64, 5.0 + 10.0 * j as f64);
            let r = if (i + j) % 2 == 0 { 4.0 } else { 4.9 };
            loops.push(circle(at, r, 10 + (8 * i + j) as u64, true));
        }
    }
    let p = profile(loops);
    let solid = run(&p, &Frame::XY, 0.0, 2.0).unwrap();
    let exact = p.area() * 2.0;
    assert!((solid.volume() - exact).abs() < 1e-12 * 6400.0 * 2.0);
}

#[test]
fn extruding_is_deterministic() {
    let p = profile(vec![
        rect(DVec2::ZERO, DVec2::new(100.0, 60.0), 0),
        circle(DVec2::new(25.0, 30.0), 5.0, 4, true),
        circle(DVec2::new(75.0, 30.0), 20.0, 5, true),
        slot(5.0, 1.0),
    ]);
    let p = Profile {
        loops: p
            .loops
            .into_iter()
            .enumerate()
            .map(|(i, lp)| {
                if i == 3 {
                    let moved = |q: DVec2| q + DVec2::new(20.0, 50.0);
                    reversed(&Loop {
                        segments: lp
                            .segments
                            .iter()
                            .map(|s| Segment {
                                conic: Conic2::new(
                                    moved(s.conic.p0),
                                    moved(s.conic.c),
                                    s.conic.w,
                                    moved(s.conic.p1),
                                )
                                .unwrap(),
                                curve: s.curve + 10,
                            })
                            .collect(),
                    })
                } else {
                    lp
                }
            })
            .collect(),
    };
    let solid = assert_deterministic(|| run(&p, &Frame::XY, 0.0, 3.0).unwrap());
    let exact = p.area() * 3.0;
    assert!((solid.volume() - exact).abs() < 1e-12 * exact);
}

#[test]
fn bad_input_is_refused() {
    let square = rect(DVec2::ZERO, DVec2::ONE, 0);
    let ok = profile(vec![square.clone()]);
    assert!(run(&ok, &Frame::XY, 0.0, 1.0).is_ok());

    // Extents.
    for (from, to) in [(1.0, 1.0), (2.0, 1.0), (f64::NAN, 1.0), (0.0, 2e6)] {
        assert!(matches!(
            run(&ok, &Frame::XY, from, to),
            Err(KernelError::Patch(_))
        ));
    }
    // Frames.
    let skew = Frame {
        y: DVec3::new(0.1, 1.0, 0.0).normalize(),
        ..Frame::XY
    };
    let far = Frame {
        origin: DVec3::new(0.0, 0.0, 9.9e5),
        ..Frame::XY
    };
    assert!(run(&ok, &skew, 0.0, 1.0).is_err());
    assert!(run(&ok, &far, 0.0, 1.0).is_ok());
    assert!(matches!(
        run(&ok, &far, 0.0, 2e4),
        Err(KernelError::Patch(PatchError::Coordinate(_)))
    ));

    // The profile's own checks.
    assert_eq!(
        run(&Profile::default(), &Frame::XY, 0.0, 1.0),
        Err(KernelError::Profile(ProfileError::Empty))
    );

    // Loops that overlap, touch or cross.
    let overlapping = profile(vec![
        square.clone(),
        rect(DVec2::splat(0.5), DVec2::splat(2.0), 4),
    ]);
    assert!(matches!(
        run(&overlapping, &Frame::XY, 0.0, 1.0),
        Err(KernelError::Profile(ProfileError::Touching(_)))
    ));
    let corner_to_corner = profile(vec![square.clone(), rect(DVec2::ONE, DVec2::splat(2.0), 4)]);
    assert!(matches!(
        run(&corner_to_corner, &Frame::XY, 0.0, 1.0),
        Err(KernelError::Profile(ProfileError::Touching(_)))
    ));
    let circles = profile(vec![
        circle(DVec2::ZERO, 1.0, 0, false),
        circle(DVec2::new(1.5, 0.0), 1.0, 1, false),
    ]);
    // Named as given, not by the pieces the arcs were halved into.
    assert!(matches!(
        run(&circles, &Frame::XY, 0.0, 1.0),
        Err(KernelError::Profile(ProfileError::Touching([(0, a), (1, b)]))) if a < 4 && b < 4
    ));
    let bowtie = profile(vec![polygon(
        &[
            DVec2::ZERO,
            DVec2::ONE,
            DVec2::new(1.0, 0.0),
            DVec2::new(0.0, 1.0),
        ],
        0,
    )]);
    assert!(run(&bowtie, &Frame::XY, 0.0, 1.0).is_err());

    // Loops that don't nest: a hole running the wrong way, a hole on its
    // own, a second outer loop inside the first.
    let wrong_hole = profile(vec![
        rect(DVec2::ZERO, DVec2::splat(10.0), 0),
        circle(DVec2::splat(5.0), 1.0, 4, false),
    ]);
    let lone_hole = profile(vec![circle(DVec2::splat(5.0), 1.0, 4, true)]);
    let inside = profile(vec![
        rect(DVec2::ZERO, DVec2::splat(10.0), 0),
        rect(DVec2::splat(4.0), DVec2::splat(6.0), 4),
    ]);
    for p in [wrong_hole, lone_hole, inside] {
        assert_eq!(
            run(&p, &Frame::XY, 0.0, 1.0),
            Err(KernelError::Profile(ProfileError::Nesting))
        );
    }
    // An island in a hole is fine.
    let island = profile(vec![
        rect(DVec2::ZERO, DVec2::splat(10.0), 0),
        reversed(&rect(DVec2::splat(2.0), DVec2::splat(8.0), 4)),
        circle(DVec2::splat(5.0), 1.0, 8, false),
    ]);
    let solid = run(&island, &Frame::XY, 0.0, 1.0).unwrap();
    assert!((solid.volume() - island.area()).abs() < 1e-12 * island.area());

    // A cusp: two arcs leaving a point the same way.
    let p = |x: f64, y: f64| DVec2::new(x, y);
    let cusp = profile(vec![Loop {
        segments: vec![
            arc(p(0.0, 3.0), p(0.0, 0.0), p(3.0, 3.0), 0),
            Segment::line(p(3.0, 3.0), p(1.0, 1.0), 1).unwrap(),
            arc(p(0.0, 1.0), p(1.0, 1.0), p(0.0, 0.0), 2),
        ],
    }]);
    assert_eq!(
        run(&cusp, &Frame::XY, 0.0, 1.0),
        Err(KernelError::Profile(ProfileError::Cusp(0, 0)))
    );

    // A straight segment whose control point lies past its end.
    let back = profile(vec![Loop {
        segments: vec![
            Segment {
                conic: Conic2::new(DVec2::ZERO, DVec2::new(2.0, 0.0), 1.0, DVec2::X).unwrap(),
                curve: 0,
            },
            Segment::line(DVec2::X, DVec2::ONE, 1).unwrap(),
            Segment::line(DVec2::ONE, DVec2::ZERO, 2).unwrap(),
        ],
    }]);
    assert_eq!(
        run(&back, &Frame::XY, 0.0, 1.0),
        Err(KernelError::Profile(ProfileError::Degenerate(0, 0)))
    );

    // Too thin for the resolution.
    assert!(matches!(
        run(&ok, &Frame::XY, 0.0, 1e-7),
        Err(KernelError::Invalid(_) | KernelError::TooComplex)
    ));
    // Out of budget.
    let plate = profile(vec![
        rect(DVec2::ZERO, DVec2::splat(10.0), 0),
        circle(DVec2::splat(5.0), 1.0, 4, true),
    ]);
    assert_eq!(
        extrude(&plate, &Frame::XY, 0.0, 1.0, 1, &TOL, &Budget::new(10)),
        Err(KernelError::TooComplex)
    );
}

#[test]
fn nearly_straight_segments_are_straight() {
    // A control point within the resolution of its chord: a flat wall.
    let p = |x: f64, y: f64| DVec2::new(x, y);
    let bent = Segment {
        conic: Conic2::new(p(0.0, 0.0), p(0.5, -1e-7), 0.9, p(1.0, 0.0)).unwrap(),
        curve: 0,
    };
    let lp = Loop {
        segments: vec![
            bent,
            Segment::line(p(1.0, 0.0), p(1.0, 1.0), 1).unwrap(),
            Segment::line(p(1.0, 1.0), p(0.0, 1.0), 2).unwrap(),
            Segment::line(p(0.0, 1.0), p(0.0, 0.0), 3).unwrap(),
        ],
    };
    let solid = run(&profile(vec![lp]), &Frame::XY, 0.0, 1.0).unwrap();
    assert!(matches!(
        solid.mesh().faces()[2].surface,
        Surface::Plane { .. }
    ));
    assert!((solid.volume() - 1.0).abs() < 1e-14);
}

/// A loop round `center` of 3 to 12 corners, up to `scale` out, each
/// side straight or a conic bulging either way.
fn random_loop(rng: &mut Rng, center: DVec2, scale: f64, curve: u64) -> Loop {
    let n = 3 + (rng.next_u64() % 10) as usize;
    let corners: Vec<DVec2> = (0..n)
        .map(|i| {
            let angle = (i as f64 + rng.range(-0.3, 0.3)) / n as f64 * std::f64::consts::TAU;
            center + DVec2::new(angle.cos(), angle.sin()) * scale * rng.range(0.3, 1.0)
        })
        .collect();
    let mut segments = Vec::new();
    for i in 0..n {
        let (a, b) = (corners[i], corners[(i + 1) % n]);
        let mid = (a + b) * 0.5 + (b - a) * rng.range(-0.3, 0.3);
        let across = (b - a).perp() * rng.range(-0.4, 0.4);
        let w = rng.range(0.3, 1.5);
        segments.push(match rng.next_u64() % 3 {
            0 => Segment::line(a, b, curve + i as u64).unwrap(),
            _ => Segment {
                conic: Conic2::new(a, mid + across, w, b).unwrap(),
                curve: curve + i as u64,
            },
        });
    }
    Loop { segments }
}

/// 30 random outlines with up to five random holes each, at every
/// tolerance, with a height to extrude them to.
fn random_plates() -> Vec<(Profile, Tolerance, f64)> {
    let mut rng = Rng::new(5);
    (0..30)
        .map(|case| {
            let mut loops = vec![random_loop(&mut rng, DVec2::ZERO, 100.0, 0)];
            for h in 0..(rng.next_u64() % 6) {
                let at = DVec2::new(rng.range(-40.0, 40.0), rng.range(-40.0, 40.0));
                let size = rng.log_range(0.5, 30.0);
                loops.push(reversed(&random_loop(&mut rng, at, size, 100 * (h + 1))));
            }
            let fit = [Tolerance::MIN_FIT, 1e-3, Tolerance::MAX_FIT][case % 3];
            let h = rng.log_range(0.1, 100.0);
            (profile(loops), Tolerance::new(fit).unwrap(), h)
        })
        .collect()
}

#[test]
fn random_plates_with_holes() {
    // Random outlines with random holes, at every tolerance: those whose
    // loops touch or don't nest are refused, the rest have their volumes.
    let mut built = 0;
    for (case, (p, tol, h)) in random_plates().into_iter().enumerate() {
        match extrude(&p, &Frame::XY, 0.0, h, 1, &tol, &Budget::DEFAULT) {
            Ok(solid) => {
                let exact = p.area() * h;
                assert!((solid.volume() - exact).abs() < 1e-12 * 1e4 * h);
                built += 1;
            }
            Err(KernelError::Profile(ProfileError::Touching(_) | ProfileError::Nesting)) => {}
            Err(e) => panic!("case {case}: {e}"),
        }
    }
    assert!(built >= 10, "{built}");
}

#[test]
fn crowded_boxes_run_out_of_budget_not_memory() {
    // A star of 65 535 chords, each nearly a diameter and leaving the one
    // before at 0.28°: every two boxes overlap, some two billion pairs,
    // which must run out of the budget before they are collected.
    let (n, k) = (65_535, 32_717);
    let points: Vec<DVec2> = (0..n)
        .map(|i| {
            let angle = (i * k % n) as f64 / n as f64 * std::f64::consts::TAU;
            DVec2::new(angle.cos(), angle.sin()) * 100.0
        })
        .collect();
    let star = profile(vec![polygon(&points, 0)]);
    assert_eq!(star.check(), Ok(()));
    assert_eq!(
        extrude(&star, &Frame::XY, 0.0, 1.0, 1, &TOL, &Budget::new(1 << 20)),
        Err(KernelError::TooComplex)
    );
}

/// `p` with the segments an extrude at `tol` takes as straight, those
/// whose control point is within the resolution of their chord, made
/// straight: the profile whose area the solid has.
fn straightened(p: &Profile, tol: &Tolerance) -> Profile {
    let straight = |s: &Segment| {
        let c = &s.conic;
        let chord = c.p1 - c.p0;
        if chord.perp_dot(c.c - c.p0).abs() > tol.resolution() * chord.length() {
            *s
        } else {
            Segment::line(c.p0, c.p1, s.curve).unwrap()
        }
    };
    Profile {
        loops: p
            .loops
            .iter()
            .map(|lp| Loop {
                segments: lp.segments.iter().map(straight).collect(),
            })
            .collect(),
    }
}

/// The circle of radius `r` round the origin cut at random angles into
/// arcs of `min` to 1.5 radians and what is left, of curves `0..`.
fn cut_circle(rng: &mut Rng, r: f64, min: f64) -> Loop {
    let mut angles = vec![0.0];
    loop {
        let next = angles.last().unwrap() + rng.log_range(min, 1.5);
        if next >= std::f64::consts::TAU - min {
            break;
        }
        angles.push(next);
    }
    let points: Vec<DVec2> = angles
        .iter()
        .map(|&a| DVec2::new(a.cos(), a.sin()) * r)
        .collect();
    let n = points.len();
    Loop {
        segments: (0..n)
            .map(|i| arc(DVec2::ZERO, points[i], points[(i + 1) % n], i as u64))
            .collect(),
    }
}

/// 120 circles of random radii cut at random angles, at three
/// tolerances, with their radii.
fn uneven_circles() -> Vec<(Profile, f64, Tolerance)> {
    let mut rng = Rng::new(3);
    (0..120)
        .map(|case| {
            let r = rng.log_range(0.1, 1e3);
            let p = profile(vec![cut_circle(&mut rng, r, 2e-3)]);
            let tol = Tolerance::new([Tolerance::MIN_FIT, 1e-3, 1e-2][case % 3]).unwrap();
            (p, r, tol)
        })
        .collect()
}

#[test]
fn circles_cut_unevenly() {
    // Short arcs among long ones meet the next nearly straight, so the
    // caps' corners there are flat: an ear's centroid lies about as
    // close to its sides as the ear is flat, and slivers along the loop
    // come within the resolution of the walls. Moving Steiner points in
    // from those corners mends them. The same bits at 1 and 8 threads.
    let cases = uneven_circles();
    let results = assert_deterministic(|| {
        cases
            .iter()
            .map(|(p, r, tol)| extrude(p, &Frame::XY, 0.0, *r, 1, tol, &Budget::DEFAULT))
            .collect::<Vec<_>>()
    });
    for (case, ((p, r, tol), result)) in cases.iter().zip(results).enumerate() {
        let solid = result.unwrap_or_else(|e| panic!("case {case}: {e}"));
        let exact = straightened(p, tol).area() * r;
        assert!(
            (solid.volume() - exact).abs() < 1e-12 * exact,
            "case {case}"
        );
    }
}

#[test]
fn fine_rings_triangulate_in_time() {
    // Two circles of 16 384 straight sides each: inserted in the loops'
    // order, each vertex of the inner one would take apart about half
    // the triangles between the circles, some 10^8 flips in all, before
    // the budget is ever looked at. Shuffled, each takes a few.
    let ring = |r: f64| {
        let n = 16_384;
        let points: Vec<DVec2> = (0..n)
            .map(|i| {
                let angle = i as f64 / n as f64 * std::f64::consts::TAU;
                DVec2::new(angle.cos(), angle.sin()) * r
            })
            .collect();
        polygon(&points, 0)
    };
    let p = profile(vec![ring(100.0), reversed(&ring(50.0))]);
    let start = std::time::Instant::now();
    // The budget runs out after the first triangulation.
    let result = extrude(&p, &Frame::XY, 0.0, 1.0, 1, &TOL, &Budget::new(1 << 19));
    assert_eq!(result, Err(KernelError::TooComplex));
    // About a second unoptimized; in the loops' order, over a minute.
    assert!(start.elapsed().as_secs() < 30, "{:?}", start.elapsed());
}

#[test]
fn tops_are_where_they_are_asked_to_be() {
    // `from + (to − from)` rounds away from `to` for many values: the top
    // is placed at `to` itself, so a solid extruded from there on stands
    // flush on it, not a rounding off.
    let mut rng = Rng::new(17);
    for _ in 0..50 {
        let from = rng.range(-3.0, 3.0);
        let to = from + rng.range(0.1, 3.0);
        let solid = run(
            &profile(vec![circle(DVec2::ZERO, 1.0, 0, false)]),
            &Frame::XY,
            from,
            to,
        )
        .unwrap();
        let zs: Vec<f64> = solid.mesh().verts().iter().map(|p| p.z).collect();
        assert!(zs.iter().all(|&z| z == from || z == to), "{from} {to}");
    }
}

/// The caps of `p` at `tol`: those the second try makes, resuming from
/// the first try's fork, are the caps and the halved chain that
/// triangulating with flat corners from the start makes, for no more
/// work; with no fork, the first try's are. Returns the fork's round, or
/// `None` for a profile refused before its caps or with no fork.
fn resumes_as_from_the_start(p: &Profile, tol: &Tolerance) -> Option<usize> {
    let margin = tol.resolution();
    let mut work = Work::new(&Budget::DEFAULT);
    let mut chain = Chain::new(p, margin).ok()?;
    chain.separate(&mut work).ok()?;
    let spent = |work: &Work| Budget::DEFAULT.work() - work.left();
    let caps = |start: Rounds, flat_corners: bool, fork: &mut Option<Rounds>| {
        let mut work = Work::new(&Budget::DEFAULT);
        let caps = cap::triangulate(start, margin, flat_corners, fork, &mut work);
        (format!("{caps:?}"), spent(&work))
    };
    let mut fork = None;
    let first = caps(Rounds::new(chain.clone()), false, &mut fork);
    let (fresh, fresh_work) = caps(Rounds::new(chain), true, &mut None);
    let round = fork.as_ref().map(Rounds::round);
    match fork {
        Some(fork) => {
            let (resumed, resumed_work) = caps(fork, true, &mut None);
            assert_eq!(resumed, fresh);
            assert!(resumed_work <= fresh_work, "{resumed_work} > {fresh_work}");
        }
        // With no flat corner the flag changes nothing, work included.
        None => assert_eq!(first, (fresh, fresh_work)),
    }
    round
}

/// A `w` × `h` plate with `cols` × `rows` round holes 10 apart, radii 4
/// and 4.9 by turns, the first centred at (5, 5).
fn plate_with_holes(cols: usize, rows: usize, w: f64, h: f64) -> Profile {
    let mut loops = vec![rect(DVec2::ZERO, DVec2::new(w, h), 0)];
    for i in 0..cols {
        for j in 0..rows {
            let at = DVec2::new(5.0 + 10.0 * i as f64, 5.0 + 10.0 * j as f64);
            let r = if (i + j) % 2 == 0 { 4.0 } else { 4.9 };
            loops.push(circle(at, r, 10 + (cols * j + i) as u64, true));
        }
    }
    profile(loops)
}

#[test]
fn the_second_try_resumes_where_the_first_found_a_flat_corner() {
    // The two tries differ only in the flat corners, so up to the first
    // round that finds one they are the same: the second resumes there
    // and makes the caps it would have made from the start.
    let strip = plate_with_holes(20, 2, 210.0, 30.0);
    assert_eq!(resumes_as_from_the_start(&strip, &TOL), Some(9));
    let small = plate_with_holes(4, 4, 50.0, 50.0);
    assert_eq!(resumes_as_from_the_start(&small, &TOL), None);
    // A fine polygon at a coarse tolerance is flat at once.
    let points: Vec<DVec2> = (0..1024)
        .map(|i| {
            let angle = i as f64 / 1024.0 * std::f64::consts::TAU;
            DVec2::new(angle.cos(), angle.sin()) * 10.0
        })
        .collect();
    let coarse = Tolerance::new(0.1).unwrap();
    assert_eq!(
        resumes_as_from_the_start(&profile(vec![polygon(&points, 0)]), &coarse),
        Some(0)
    );
    // `circles_cut_unevenly`'s circles and `random_plates_with_holes`'
    // plates.
    let forked = uneven_circles()
        .iter()
        .filter(|(p, _, tol)| resumes_as_from_the_start(p, tol).is_some())
        .count();
    assert!((10..110).contains(&forked), "{forked}");
    for (p, tol, _) in random_plates() {
        resumes_as_from_the_start(&p, &tol);
    }
}

#[test]
fn the_second_try_is_charged_only_from_where_it_resumes() {
    // A 210 × 30 strip with 40 holes, the bottom row 0.1 from its side:
    // the first try fails, the second, with flat corners, passes. Starting the
    // second over took 191 858 units in all; resuming it, 149 450. The
    // same bits at 1 and 8 threads.
    let p = plate_with_holes(20, 2, 210.0, 30.0);
    let solid = assert_deterministic(|| {
        extrude(&p, &Frame::XY, 0.0, 2.0, 9, &TOL, &Budget::new(170_000)).unwrap()
    });
    let exact = p.area() * 2.0;
    assert!((solid.volume() - exact).abs() < 1e-12 * exact);
    assert_eq!(solid.mesh().check_faces(&TOL), Ok(()));
    assert_eq!(solid.mesh().tris().len(), 2988);
    assert_eq!(
        extrude(&p, &Frame::XY, 0.0, 2.0, 9, &TOL, &Budget::new(149_449)),
        Err(KernelError::TooComplex)
    );
    assert_eq!(
        extrude(&p, &Frame::XY, 0.0, 2.0, 9, &TOL, &Budget::new(149_450)),
        Ok(solid)
    );
}

#[test]
fn a_first_try_out_of_work_in_its_fork_round_has_no_second() {
    // The least budget with which the strip's first try gets as far as
    // the round it forks in runs out within that round, after the fork:
    // with no work left there is no second try, just as one less unit
    // runs out before the fork.
    let p = plate_with_holes(20, 2, 210.0, 30.0);
    let margin = TOL.resolution();
    let mut work = Work::new(&Budget::DEFAULT);
    let mut chain = Chain::new(&p, margin).unwrap();
    chain.separate(&mut work).unwrap();
    let separated = Budget::DEFAULT.work() - work.left();
    let first = |units: u64| {
        let mut fork = None;
        let caps = cap::triangulate(
            Rounds::new(chain.clone()),
            margin,
            false,
            &mut fork,
            &mut Work::new(&Budget::new(units)),
        );
        (caps.map(|_| ()), fork.as_ref().map(Rounds::round))
    };
    let (mut lo, mut hi) = (0, Budget::DEFAULT.work());
    assert_eq!(first(hi).1, Some(9));
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if first(mid).1.is_some() {
            hi = mid
        } else {
            lo = mid
        }
    }
    assert_eq!(first(hi), (Err(KernelError::TooComplex), Some(9)));
    assert_eq!(first(lo), (Err(KernelError::TooComplex), None));
    for units in [lo, hi] {
        let budget = Budget::new(separated + units);
        assert_eq!(
            extrude(&p, &Frame::XY, 0.0, 2.0, 9, &TOL, &budget),
            Err(KernelError::TooComplex)
        );
    }
}
