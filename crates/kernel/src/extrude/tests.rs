#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::PI;

use glam::{DVec2, DVec3};

use super::*;
use crate::Stripped;
use crate::mesh::{CheckError, FacePart};
use crate::par::assert_deterministic;
use crate::profile::tests::{arc, circle, folding_cap, polygon, rect, reversed};
use crate::profile::{Loop, Segment};
use crate::test_rng::Rng;
use crate::{Budget, Display};

const TOL: Tolerance = Tolerance::DEFAULT;

pub(super) fn profile(loops: Vec<Loop>) -> Profile {
    Profile { loops }
}

fn run(p: &Profile, frame: &Frame, from: f64, to: f64) -> Result<Solid, KernelError> {
    extrude(p, frame, from, to, 9, &TOL, &Budget::DEFAULT).stripped()
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
        // The thinner one's caps, refined for quality, hold some 25 000
        // patches, whose areas' rounding adds up to 3e-13 of the disc's.
        assert!((solid.area() - surface).abs() < 1e-12 * disc);
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
                ProfileError::Touching(_) | ProfileError::Nesting(_) | ProfileError::Cusp(..),
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
    for (p, l) in [(wrong_hole, 1), (lone_hole, 0), (inside, 1)] {
        assert_eq!(
            run(&p, &Frame::XY, 0.0, 1.0),
            Err(KernelError::Profile(ProfileError::Nesting(l)))
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

    // Too thin for the resolution: a scale limit, never `TooComplex`.
    assert!(matches!(
        run(&ok, &Frame::XY, 0.0, 1e-7),
        Err(KernelError::Invalid(_))
    ));
    // Out of budget.
    let plate = profile(vec![
        rect(DVec2::ZERO, DVec2::splat(10.0), 0),
        circle(DVec2::splat(5.0), 1.0, 4, true),
    ]);
    assert_eq!(
        extrude(&plate, &Frame::XY, 0.0, 1.0, 1, &TOL, &Budget::new(10)).stripped(),
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
pub(super) fn random_plates() -> Vec<(Profile, Tolerance, f64)> {
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
        match extrude(&p, &Frame::XY, 0.0, h, 1, &tol, &Budget::DEFAULT).stripped() {
            Ok(solid) => {
                let exact = p.area() * h;
                assert!((solid.volume() - exact).abs() < 1e-12 * 1e4 * h);
                built += 1;
            }
            Err(KernelError::Profile(ProfileError::Touching(_) | ProfileError::Nesting(_))) => {}
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
        extrude(&star, &Frame::XY, 0.0, 1.0, 1, &TOL, &Budget::new(1 << 20)).stripped(),
        Err(KernelError::TooComplex)
    );
}

/// `p` with the segments an extrude at `tol` takes as straight, those
/// whose control point is within the resolution of their chord, made
/// straight: the profile whose area the solid has.
pub(super) fn straightened(p: &Profile, tol: &Tolerance) -> Profile {
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
pub(super) fn cut_circle(rng: &mut Rng, r: f64, min: f64) -> Loop {
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
    uneven_circles_from(3, [Tolerance::MIN_FIT, 1e-3, 1e-2])
}

/// 120 circles of radii 0.1 to 1 000 cut at random angles into arcs of
/// at least `2e-3` radians, from the generator seeded with `seed`, case
/// `i` at the tolerance `fits[i % 3]`, with their radii.
pub(super) fn uneven_circles_from(seed: u64, fits: [f64; 3]) -> Vec<(Profile, f64, Tolerance)> {
    let mut rng = Rng::new(seed);
    (0..120)
        .map(|case| {
            let r = rng.log_range(0.1, 1e3);
            let p = profile(vec![cut_circle(&mut rng, r, 2e-3)]);
            let tol = Tolerance::new(fits[case % 3]).unwrap();
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
    let result = extrude(&p, &Frame::XY, 0.0, 1.0, 1, &TOL, &Budget::new(1 << 19)).stripped();
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
        let mode = cap::Mode {
            quality: true,
            flat_corners,
        };
        let caps = cap::triangulate(start, margin, mode, fork, &mut false, &mut work);
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

#[test]
fn kept_triangulations_are_those_made_afresh() {
    // The rounds insert points into the triangulation they keep and
    // replace halved segments' chords in it: where no four points lie on
    // a circle, that is the triangulation of the last round's chords and
    // points made afresh. Outlines of 100 to 300 uneven straight sides
    // and arcs of conics round random holes, at fits 1e-3 and 1e-1, on
    // every try; most get Steiner points or halved segments.
    let mut rng = Rng::new(41);
    let (mut compared, mut changed) = (0, 0);
    for case in 0..12 {
        let n = 100 + (rng.next_u64() % 200) as usize;
        let phase = rng.range(0.0, 6.0);
        let mut points = Vec::with_capacity(n);
        for i in 0..n {
            let angle = (i as f64 + rng.range(-0.4, 0.4)) / n as f64 * std::f64::consts::TAU;
            let r = 30.0 * (1.0 + 0.3 * (3.0 * angle + phase).sin()) * rng.range(0.98, 1.0);
            points.push(DVec2::new(angle.cos(), angle.sin()) * r);
        }
        let mut loops = vec![polygon(&points, 0)];
        for h in 0..1 + rng.next_u64() % 3 {
            let at = DVec2::new(-10.0 + 10.0 * h as f64, rng.range(-3.0, 3.0));
            loops.push(reversed(&random_loop(&mut rng, at, 4.0, 1000 * (h + 1))));
        }
        let p = profile(loops);
        let tol = Tolerance::new([1e-3, 0.1][case % 2]).unwrap();
        let margin = tol.resolution();
        let mut work = Work::new(&Budget::DEFAULT);
        let Ok(mut chain) = Chain::new(&p, margin) else {
            continue;
        };
        if chain.separate(&mut work).is_err() {
            continue;
        }
        for mode in [Mode::QUALITY, Mode::FLAT_CORNERS, Mode::PLAIN] {
            let start = Rounds::new(chain.clone());
            let mut work = Work::new(&Budget::DEFAULT);
            let caps = cap::triangulate(start, margin, mode, &mut None, &mut false, &mut work);
            if let Ok((halved, cap)) = caps {
                let [fresh, kept] = cap::afresh(&halved, &cap).unwrap();
                assert_eq!(fresh, kept, "case {case} {mode:?}");
                compared += 1;
                changed += usize::from(!cap.steiner.is_empty() || halved.len() > chain.len());
            }
        }
    }
    assert!(compared >= 30 && changed >= 25, "{compared} {changed}");
}

/// A `w` × `h` plate with `cols` × `rows` round holes 10 apart, radii 4
/// and 4.9 by turns, the first centred at (5, 5).
pub(super) fn plate_with_holes(cols: usize, rows: usize, w: f64, h: f64) -> Profile {
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

/// A 10 × 10 square with a small hole of sharply weighted conics (from
/// a stress of random plates): at fit 1e-2 its first try finds a flat
/// corner only in round 5, and fails; the second, with flat corners,
/// passes.
fn late_fork() -> Profile {
    let hole = |pieces: &[([f64; 2], [f64; 2], f64)], curve: u64| Loop {
        segments: (0..pieces.len())
            .map(|i| {
                let (p0, c, w) = pieces[i];
                let p1 = pieces[(i + 1) % pieces.len()].0;
                let conic = Conic2::new(p0.into(), c.into(), w, p1.into()).unwrap();
                Segment {
                    conic,
                    curve: curve + i as u64,
                }
            })
            .collect(),
    };
    let first = hole(
        &[
            (
                [-2.875792837240438, 0.767483223403253],
                [-2.890660925723614, 0.7635802637126888],
                1.7433118507651248,
            ),
            (
                [-2.9030675584770083, 0.7421627767314306],
                [-2.8897883435795055, 0.6871759582484847],
                0.0924430246044685,
            ),
            (
                [-2.9436688801988975, 0.6538578455034685],
                [-2.950755066300978, 0.6118902117264731],
                2.329109567103195,
            ),
            (
                [-3.0311681095183642, 0.6155914900501958],
                [-3.0494790995449095, 0.6646566789247408],
                0.11484298722193315,
            ),
            (
                [-3.0528969203865106, 0.7049902541566684],
                [-3.0550152903496612, 0.7475604256957974],
                0.7920707473624892,
            ),
            (
                [-3.0537737423975715, 0.7607507853299873],
                [-3.1494152370859987, 0.8116840831291752],
                2.9319593279526823,
            ),
            (
                [-3.162260999409073, 0.8629769235394325],
                [-3.114782719740387, 0.8601306993229467],
                5.427032588809919,
            ),
            (
                [-3.038847795251609, 0.8486696894157061],
                [-3.026370989442436, 0.8444215553778929],
                0.054636538832601486,
            ),
            (
                [-3.0026744445954816, 0.8635741502866685],
                [-2.97480382993625, 0.8705352286561097],
                1.0,
            ),
            (
                [-2.9469332152770185, 0.8774963070255507],
                [-2.9349055172428185, 0.8383534187448597],
                0.2676830649356724,
            ),
            (
                [-2.929788767256126, 0.8286604217521056],
                [-2.8663848143122763, 0.8005842592711854],
                0.24463669698280374,
            ),
        ],
        100,
    );
    profile(vec![rect(DVec2::splat(-5.0), DVec2::splat(5.0), 0), first])
}

#[test]
fn the_second_try_resumes_where_the_first_found_a_flat_corner() {
    // The two tries differ only in the flat corners, so up to the first
    // round that finds one they are the same: the second resumes there
    // and makes the caps it would have made from the start.
    let fine = Tolerance::new(1e-2).unwrap();
    assert_eq!(resumes_as_from_the_start(&late_fork(), &fine), Some(5));
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
    // `late_fork` at fit 1e-2: the first try fails, the second, with
    // flat corners, passes, its mesh checked as built (no repair).
    // Starting the second over would take 19 553 units in all; resuming
    // it, 18 211. The same bits at 1 and 8 threads.
    let fine = Tolerance::new(1e-2).unwrap();
    let p = late_fork();
    let run =
        |units: u64| extrude(&p, &Frame::XY, 0.0, 2.0, 9, &fine, &Budget::new(units)).stripped();
    let solid = assert_deterministic(|| run(400_000).unwrap());
    let exact = p.area() * 2.0;
    assert!((solid.volume() - exact).abs() < 1e-12 * exact);
    assert_eq!(solid.mesh().check_faces(&fine), Ok(()));
    assert_eq!(solid.mesh().tris().len(), 220);
    // Out of work in the second try, the first try's error stands.
    assert!(matches!(run(18_210), Err(KernelError::Invalid(_))));
    assert_eq!(run(18_211), Ok(solid));
}

#[test]
fn a_first_try_out_of_work_in_its_fork_round_has_no_second() {
    // The least budget with which `late_fork`'s first try gets as far as
    // the round it forks in runs out within that round, after the fork:
    // with no work left there is no second try, just as one less unit
    // runs out before the fork.
    let fine = Tolerance::new(1e-2).unwrap();
    let p = late_fork();
    let margin = fine.resolution();
    let mut work = Work::new(&Budget::DEFAULT);
    let mut chain = Chain::new(&p, margin).unwrap();
    chain.separate(&mut work).unwrap();
    let separated = Budget::DEFAULT.work() - work.left();
    let first = |units: u64| {
        let mut fork = None;
        let caps = cap::triangulate(
            Rounds::new(chain.clone()),
            margin,
            cap::Mode::QUALITY,
            &mut fork,
            &mut false,
            &mut Work::new(&Budget::new(units)),
        );
        (caps.map(|_| ()), fork.as_ref().map(Rounds::round))
    };
    let (mut lo, mut hi) = (0, Budget::DEFAULT.work());
    assert_eq!(first(hi).1, Some(5));
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if first(mid).1.is_some() {
            hi = mid
        } else {
            lo = mid
        }
    }
    assert_eq!(first(hi), (Err(KernelError::TooComplex), Some(5)));
    assert_eq!(first(lo), (Err(KernelError::TooComplex), None));
    for units in [lo, hi] {
        let budget = Budget::new(separated + units);
        assert_eq!(
            extrude(&p, &Frame::XY, 0.0, 2.0, 9, &fine, &budget).stripped(),
            Err(KernelError::TooComplex)
        );
    }
}

#[test]
fn rows_of_holes_near_a_side_pass_on_the_first_try() {
    // A 210 × 30 strip with 40 holes, the bottom row 0.1 from its side:
    // the edges from the strip's corners to the arcs along the side run
    // along the arcs' tangents, and halving the arcs doesn't open those
    // corners. Past `MAX_MEND_DEPTH` halvings they are left to
    // refinement, whose halvings of the side give the arcs vertices
    // near, on the first try; halved on to `MAX_CAP_DEPTH`, the first
    // try failed and the second, with flat corners, passed. The same at
    // 1 and 8 threads.
    let p = plate_with_holes(20, 2, 210.0, 30.0);
    let margin = TOL.resolution();
    let mut work = Work::new(&Budget::DEFAULT);
    let mut chain = Chain::new(&p, margin).unwrap();
    chain.separate(&mut work).unwrap();
    let caps = cap::triangulate(
        Rounds::new(chain),
        margin,
        cap::Mode::QUALITY,
        &mut None,
        &mut false,
        &mut work,
    );
    assert!(caps.is_ok());
    let solid = assert_deterministic(|| run(&p, &Frame::XY, 0.0, 2.0).unwrap());
    let exact = p.area() * 2.0;
    assert!((solid.volume() - exact).abs() < 1e-12 * exact);
    assert_eq!(solid.mesh().check_faces(&TOL), Ok(()));
}

#[test]
fn a_corner_too_small_to_halve_is_left_to_refinement() {
    // A small hole of sharply weighted conics (from a stress of random
    // plates, weights 0.05 to 20, seed 2 case 11): at fits of 0.1 and
    // 0.05 the caps ask to halve its segment 3 where it is already under
    // `MIN_SPLIT` resolutions. That was `TooFine` (detail too small for
    // the tolerance); refinement's points take the corner apart instead.
    // (`a_refused_halving_names_detail_too_small_wherever_it_comes` has
    // the error for a halving refinement can't do without.)
    let p = |x: f64, y: f64| DVec2::new(x, y);
    let hole = [
        (
            p(27.110264020465635, 8.18980829207795),
            p(27.148755275236258, 7.991223208848414),
            1.0,
        ),
        (
            p(27.18724653000688, 7.792638125618878),
            p(27.066044759988173, 7.973079939121326),
            2.8902795371770957,
        ),
        (
            p(26.676456450726537, 7.746392274582473),
            p(26.429914500587888, 7.564404189159178),
            15.05158637397626,
        ),
        (
            p(26.32171598677136, 7.615822300683291),
            p(26.169215237129592, 7.778501927912703),
            0.06813088567924576,
        ),
        (
            p(26.37399226527523, 8.066824927126845),
            p(26.16303627989656, 8.093882770486916),
            1.0,
        ),
        (
            p(25.952080294517884, 8.120940613846987),
            p(25.803339626316774, 8.354883426245971),
            1.2203509743841443,
        ),
        (
            p(26.01562562551988, 8.62243755266067),
            p(26.193990851661308, 8.661359876731547),
            1.0,
        ),
        (
            p(26.372356077802735, 8.700282200802423),
            p(26.65233702997371, 8.62690992632961),
            0.16402529749735703,
        ),
        (
            p(26.762384791989902, 8.511615327972974),
            p(26.97729243684653, 8.584905265736102),
            18.60916956030496,
        ),
        (
            p(27.094576940059348, 8.691482610706228),
            p(27.204834913312506, 8.448184784080313),
            0.4307463967631604,
        ),
    ];
    let n = hole.len();
    let hole = Loop {
        segments: (0..n)
            .map(|i| {
                let (a, c, w) = hole[i];
                Segment {
                    conic: Conic2::new(a, c, w, hole[(i + 1) % n].0).unwrap(),
                    curve: 100 + i as u64,
                }
            })
            .collect(),
    };
    let plate = profile(vec![rect(p(20.0, 0.0), p(35.0, 15.0), 0), hole]);
    let at = |fit: f64| {
        let tol = Tolerance::new(fit).unwrap();
        extrude(&plate, &Frame::XY, 0.0, 1.0, 1, &tol, &Budget::DEFAULT)
    };
    for fit in [Tolerance::MAX_FIT, 0.05, 0.01] {
        let tol = Tolerance::new(fit).unwrap();
        let solid = at(fit).unwrap();
        let exact = straightened(&plate, &tol).area();
        assert!((solid.volume() - exact).abs() < 1e-12 * exact, "{fit}");
        assert_eq!(solid.mesh().check_faces(&tol), Ok(()), "{fit}");
    }
}

#[test]
fn small_holes_whose_plain_caps_repair_cant_split_extrude() {
    // Small holes of sharply weighted conics from a stress of random
    // plates (weights 0.05 to 20: seed 6 case 146, seed 24 case 293, seed
    // 18 case 539, cut down to the failing loops). At the coarse fits
    // repair needed the plain caps' pieces split smaller than it splits:
    // `TooComplex` once, then `Invalid`, mended by a finer fit. The caps
    // refined for quality need no such splits.
    let p = |x: f64, y: f64| DVec2::new(x, y);
    let conics = |segments: &[(DVec2, DVec2, f64)], curve: u64| {
        let n = segments.len();
        Loop {
            segments: (0..n)
                .map(|i| {
                    let (a, c, w) = segments[i];
                    Segment {
                        conic: Conic2::new(a, c, w, segments[(i + 1) % n].0).unwrap(),
                        curve: curve + i as u64,
                    }
                })
                .collect(),
        }
    };
    let outer = [
        (
            p(84.58239864925856, -8.93650620488482),
            p(73.34601650480002, 15.287904716038994),
            1.0,
        ),
        (
            p(62.10963436034146, 39.51231563696281),
            p(48.4732558658963, 54.82763220355444),
            0.10318645210895705,
        ),
        (
            p(-6.39335762149064, 60.68412369663893),
            p(-22.049990769123312, 44.133912338173666),
            1.0,
        ),
        (
            p(-37.70662391675598, 27.583700979708404),
            p(-45.5889852996602, 19.171443261536314),
            0.06751662894778927,
        ),
        (
            p(-62.07608285086013, -0.6779098564495205),
            p(-51.67256582211828, -31.908230721778562),
            1.0,
        ),
        (
            p(-41.26904879337643, -63.13855158710761),
            p(-28.568037376673843, -57.546094078245545),
            0.21453666642607067,
        ),
        (
            p(9.124048097614443, -76.12385997465438),
            p(30.117197991755734, -72.95990233661024),
            16.178608525050354,
        ),
        (
            p(62.519237703153394, -69.20757810037131),
            p(49.090410315144254, -38.280572597455475),
            0.19796647343104368,
        ),
    ];
    let hole_6_146 = [
        (
            p(-39.55832649287243, -33.3376806843723),
            p(-39.5303172868252, -33.50298972209562),
            1.0,
        ),
        (
            p(-39.50230808077797, -33.66829875981894),
            p(-39.55337294139827, -33.833099498365506),
            2.1612842853661594,
        ),
        (
            p(-39.70761749026822, -33.98180097654761),
            p(-39.93781550022229, -33.99012882084077),
            0.11615248197433417,
        ),
        (
            p(-40.11168762621217, -33.50487348381482),
            p(-40.23562041655502, -33.50607841288104),
            1.0,
        ),
        (
            p(-40.35955320689788, -33.50728334194725),
            p(-40.23955613278409, -33.47995296884042),
            0.06632460425800897,
        ),
        (
            p(-40.17018666599026, -33.200416682352476),
            p(-40.136063700908686, -33.06053717536551),
            1.5428227425689065,
        ),
        (
            p(-40.06935841941648, -32.99707466045286),
            p(-39.8732750107749, -32.99561356125778),
            0.07221574296327694,
        ),
        (
            p(-39.815809253095175, -32.781145265730025),
            p(-39.554756983977015, -32.67491878888499),
            0.23653135245298199,
        ),
        (
            p(-39.417291724669276, -32.75248601163126),
            p(-39.487809108770854, -33.04508334800178),
            1.0,
        ),
    ];
    let hole_24_293 = [
        (
            p(-11.73325441944377, 28.882869439843883),
            p(-11.758723886089317, 28.687138253763038),
            6.951321302418699,
        ),
        (
            p(-11.640264309717551, 28.593986775620287),
            p(-11.865251932360508, 28.65324842333672),
            0.055907706866586936,
        ),
        (
            p(-11.944660295315796, 28.588060277557577),
            p(-11.997572385460668, 28.499090481392223),
            0.06729377861908163,
        ),
        (
            p(-12.083084188283927, 28.524306527311516),
            p(-12.085608864849698, 28.71687574579696),
            4.1264965543860015,
        ),
        (
            p(-12.292529948547145, 28.775766377769568),
            p(-12.238459749471906, 28.836029639466666),
            0.18165433672344508,
        ),
        (
            p(-12.276004726881725, 28.917662181965678),
            p(-12.290146533706146, 29.10198910224345),
            1.0,
        ),
        (
            p(-12.304288340530565, 29.286316022521223),
            p(-12.055051087770918, 29.346776445971514),
            6.309403509768318,
        ),
        (
            p(-11.939162824899038, 29.4015837336499),
            p(-11.891422496833188, 29.226300159319962),
            0.1490522800395643,
        ),
        (
            p(-11.597440288346421, 29.0937536519099),
            p(-11.657170129006687, 28.914265814551495),
            15.653535249166824,
        ),
    ];
    let hole_18_539 = [
        (
            p(-29.066448988105734, 9.305420727887169),
            p(-29.119716030029878, 9.194191104327956),
            0.16250807474193119,
        ),
        (
            p(-28.973485171073328, 9.059338036606203),
            p(-29.138432090041135, 9.139591308734689),
            0.18114397838135546,
        ),
        (
            p(-29.20860564158459, 9.115541108214009),
            p(-29.296911889336556, 9.053779805035408),
            1.0,
        ),
        (
            p(-29.385218137088522, 8.992018501856805),
            p(-29.45563664102624, 8.963026583751708),
            5.285636773138308,
        ),
        (
            p(-29.543797811492084, 9.058428387863552),
            p(-29.646965115403425, 9.196363705694907),
            1.0,
        ),
        (
            p(-29.750132419314767, 9.334299023526262),
            p(-29.573070134369566, 9.350624449682888),
            1.0,
        ),
        (
            p(-29.396007849424368, 9.366949875839515),
            p(-29.442691067910438, 9.538361271464803),
            6.350811438534368,
        ),
        (
            p(-29.39019735888712, 9.612889296585548),
            p(-29.262721657523638, 9.618993178965315),
            1.0,
        ),
        (
            p(-29.135245956160155, 9.625097061345082),
            p(-29.056897774254217, 9.56116801916033),
            1.1126134273207642,
        ),
        (
            p(-29.022024561039245, 9.510846483711546),
            p(-28.984690941513808, 9.451129334414595),
            0.14875146252904442,
        ),
    ];
    let cases = [
        (
            profile(vec![conics(&outer, 0), conics(&hole_6_146, 100)]),
            0.16520266418541427,
            [Tolerance::MAX_FIT, 0.05, 0.01],
            1e-3,
        ),
        (
            profile(vec![
                rect(p(-14.0, 27.0), p(-10.0, 31.0), 0),
                conics(&hole_24_293, 100),
            ]),
            1.0,
            [Tolerance::MAX_FIT, 0.05, 0.05],
            0.01,
        ),
        (
            profile(vec![
                rect(p(-31.0, 7.0), p(-27.0, 11.0), 0),
                conics(&hole_18_539, 100),
            ]),
            1.0,
            [Tolerance::MAX_FIT, 0.05, 0.05],
            0.01,
        ),
    ];
    for (i, (plate, h, coarse, fine)) in cases.iter().enumerate() {
        let at = |fit: f64| {
            let tol = Tolerance::new(fit).unwrap();
            extrude(plate, &Frame::XY, 0.0, *h, 1, &tol, &Budget::DEFAULT)
        };
        for &fit in coarse.iter().chain([fine]) {
            let tol = Tolerance::new(fit).unwrap();
            let solid = at(fit).unwrap_or_else(|e| panic!("case {i} at {fit}: {e:?}"));
            let exact = straightened(plate, &tol).area() * h;
            assert!(
                (solid.volume() - exact).abs() < 1e-12 * exact,
                "case {i} at {fit}: {} against {exact}",
                solid.volume()
            );
            assert_eq!(solid.mesh().check_faces(&tol), Ok(()), "case {i} at {fit}");
        }
    }
}

#[test]
fn a_refused_halving_names_detail_too_small_wherever_it_comes() {
    // At a resolution of 1e-3 a piece must span 0.064 to be halved: the
    // big circle's quarters may be (until halved too often), the small
    // hole's may not.
    let p = profile(vec![
        circle(DVec2::ZERO, 1.0, 0, false),
        circle(DVec2::ZERO, 0.02, 1, true),
    ]);
    let fresh = || chain::Chain::new(&p, 1e-3).unwrap();
    let mut chain = fresh();
    chain.loops[0][1].depth = cap::MAX_CAP_DEPTH;
    // Halved too often: mending that doesn't converge.
    assert_eq!(
        chain.split(&[1], cap::MAX_CAP_DEPTH, false, cap::refused),
        Err(KernelError::TooComplex)
    );
    // A small one among them, even after one halved too often: the
    // detail is too fine, and it names the input segment.
    assert_eq!(
        chain.split(&[0, 1, 6], cap::MAX_CAP_DEPTH, false, cap::refused),
        Err(KernelError::Profile(ProfileError::TooFine(1, 2)))
    );
    // Refusing halves none.
    assert_eq!(chain.len(), 8);
    // The ones that may be halved are.
    let mut chain = fresh();
    assert_eq!(
        chain.split(&[0, 3], cap::MAX_CAP_DEPTH, false, cap::refused),
        Ok(())
    );
    assert_eq!(chain.len(), 10);
}

#[test]
fn a_cap_whose_straight_split_folds_extrudes() {
    // Each cap is one triangle with two curved sides, one concave of
    // weight above 1. It passes the fold check, so the extrude needs no
    // repair; but split with straight inner edges, as repair and a
    // boolean's refinement split pieces on a plane, a child's corner at a
    // curve's midpoint turns inside out, and splitting further keeps it
    // so. Patches that refine safely at construction would mend that.
    let p = profile(vec![folding_cap(false)]);
    let solid = run(&p, &Frame::XY, 0.0, 5.0).unwrap();
    let exact = p.area() * 5.0;
    assert!((solid.volume() - exact).abs() <= 1e-12 * exact);
    assert_eq!(solid.mesh().check_faces(&TOL), Ok(()));
    let mesh = solid.mesh();
    let caps: Vec<u32> = (0..mesh.tris().len() as u32)
        .filter(|&t| {
            let part = mesh.faces()[mesh.tris()[t as usize].face as usize]
                .name
                .part;
            matches!(part, FacePart::StartCap | FacePart::EndCap)
        })
        .collect();
    assert_eq!(caps.len(), 2);
    for &cap in &caps {
        assert!(mesh.patch(cap as usize).fold_direction().is_some());
        let mut refiner = crate::mesh::Refiner::new(mesh, TOL.resolution(), 0.0);
        let mut work = crate::budget::Work::new(&Budget::DEFAULT);
        let mut leaves = vec![cap];
        for depth in 1..=4 {
            refiner.split(&leaves, &mut work).unwrap();
            // The cap's pieces that fail the fold check, by leaf.
            leaves = refiner
                .pieces()
                .unwrap()
                .iter()
                .filter(|q| q.origin == cap && q.patch.fold_direction().is_none())
                .map(|q| q.leaf)
                .collect();
            leaves.sort_unstable();
            leaves.dedup();
            assert!(!leaves.is_empty(), "cap {cap} mended at depth {depth}");
        }
    }
    // With a parabola for the concave side, nothing folds.
    let p = profile(vec![folding_cap(true)]);
    let solid = run(&p, &Frame::XY, 0.0, 5.0).unwrap();
    let mesh = solid.mesh();
    let mut refiner = crate::mesh::Refiner::new(mesh, TOL.resolution(), 0.0);
    let mut work = crate::budget::Work::new(&Budget::DEFAULT);
    let all: Vec<u32> = (0..mesh.tris().len() as u32).collect();
    refiner.split(&all, &mut work).unwrap();
    assert!(
        refiner
            .pieces()
            .unwrap()
            .iter()
            .all(|q| q.patch.fold_direction().is_some())
    );
}

/// The first try at extruding `p` on the XY plane from `from` to `to`
/// (caps refined for quality), made as [`extruded`](super::extruded)
/// makes it within `units`: the mesh built, and the solid or failure
/// [`Solid::new_repaired_within`] makes of it.
fn first_try(
    p: &Profile,
    from: f64,
    to: f64,
    tol: &Tolerance,
    units: u64,
) -> (Mesh, Result<Solid, Failure>) {
    let margin = tol.resolution();
    let mut work = Work::new(&Budget::new(units));
    let mut chain = Chain::new(p, margin).unwrap();
    chain.separate(&mut work).unwrap();
    let start = Rounds::new(chain);
    let caps = cap::triangulate(
        start,
        margin,
        Mode::QUALITY,
        &mut None,
        &mut false,
        &mut work,
    );
    let (chain, cap) = caps.unwrap();
    let mesh = build(&chain, &cap, &Frame::XY, from, to, 9).unwrap();
    work.spend(mesh.tris().len()).unwrap();
    (
        mesh.clone(),
        Solid::new_repaired_within(mesh, tol, &mut work),
    )
}

/// The patches of `mesh`'s triangles a pair's or a triangle's error
/// names.
fn named_patches(mesh: &Mesh, why: CheckError) -> Vec<crate::patch::Patch> {
    let tris = match why {
        CheckError::Fold(t) | CheckError::Face(t) | CheckError::FacesAgainst(t) => vec![t],
        CheckError::Hull(t, u)
        | CheckError::EdgeNeighbours(t, u)
        | CheckError::VertexNeighbours(t, u)
        | CheckError::SameCorners(t, u)
            if t != u =>
        {
            vec![t, u]
        }
        CheckError::Hull(t, _)
        | CheckError::EdgeNeighbours(t, _)
        | CheckError::VertexNeighbours(t, _)
        | CheckError::SameCorners(t, _) => vec![t],
        why => panic!("{why:?}"),
    };
    tris.into_iter().map(|t| mesh.patch(t as usize)).collect()
}

/// Checks `patches` are pieces of the triangles of `mesh` that `why`
/// names, one each, as repair names them: each within its triangle's
/// box (a piece's hull lies in its triangle's).
fn pieces_of_named(mesh: &Mesh, why: CheckError, patches: &[crate::patch::Patch]) {
    let named = named_patches(mesh, why);
    assert_eq!(patches.len(), named.len(), "{why:?}");
    let slack = DVec3::splat(1e-12);
    for (piece, whole) in patches.iter().zip(&named) {
        let (inner, outer) = (piece.bounds(), whole.bounds());
        assert!(
            (outer.min - slack).cmple(inner.min).all() && inner.max.cmple(outer.max + slack).all(),
            "{why:?}"
        );
    }
}

#[test]
fn a_solid_too_thin_fails_with_the_triangles_it_names() {
    // A unit square extruded a tenth of the resolution: repair refuses
    // the built mesh, naming two of its triangles, whose patches come
    // with the error, on the solid's surface. The same at 1 and 8
    // threads, and the first try's (the only one: no flat corner).
    let square = profile(vec![rect(DVec2::ZERO, DVec2::ONE, 0)]);
    let h = 0.1 * TOL.resolution();
    let failure = assert_deterministic(|| {
        extrude(&square, &Frame::XY, 0.0, h, 9, &TOL, &Budget::DEFAULT).unwrap_err()
    });
    // The error it gave before failures carried their triangles.
    let why = CheckError::EdgeNeighbours(0, 1);
    assert_eq!(failure.error, KernelError::Invalid(why));
    let (mesh, first) = first_try(&square, 0.0, h, &TOL, Budget::DEFAULT.work());
    assert_eq!(first.unwrap_err(), failure);
    let named = named_patches(&mesh, why);
    assert_eq!(failure.evidence.patches, named, "{why:?}");
    for patch in &failure.evidence.patches {
        for p in patch.p.iter().chain(&patch.c) {
            let inside = (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y);
            assert!(inside && (0.0..=h).contains(&p.z), "{p}");
        }
    }
    let rest = crate::Evidence {
        patches: Vec::new(),
        ..*failure.evidence
    };
    assert!(rest.is_empty());
}

#[test]
fn the_first_tries_evidence_goes_with_its_error() {
    // `late_fork` at fit 1e-2 within 18 210 units: the first try fails
    // repair, the second runs out of work, and the first try's error
    // stands with the first try's evidence: the pieces of its mesh's
    // triangles that repair names. The same at 1 and 8 threads.
    let fine = Tolerance::new(1e-2).unwrap();
    let p = late_fork();
    let failure = assert_deterministic(|| {
        extrude(&p, &Frame::XY, 0.0, 2.0, 9, &fine, &Budget::new(18_210)).unwrap_err()
    });
    // Errors as they were before failures carried their triangles.
    let why = CheckError::VertexNeighbours(172, 178);
    assert_eq!(failure.error, KernelError::Invalid(why));
    let (mesh, first) = first_try(&p, 0.0, 2.0, &fine, 18_210);
    assert_eq!(first.unwrap_err(), failure);
    pieces_of_named(&mesh, why, &failure.evidence.patches);
    assert_ne!(failure.evidence.patches, named_patches(&mesh, why));

    // A circle cut unevenly at the coarsest tolerance: every try is
    // refused, the second (flat corners) on caps of its own with another
    // error. The first try's error is returned, with its evidence.
    let (p, r, tol) = uneven_circles_from(2, [Tolerance::MAX_FIT, 1e-2, 1e-3]).swap_remove(12);
    assert_eq!(tol, Tolerance::new(Tolerance::MAX_FIT).unwrap());
    let failure = assert_deterministic(|| {
        extrude(&p, &Frame::XY, 0.0, r, 9, &tol, &Budget::DEFAULT).unwrap_err()
    });
    let why = CheckError::VertexNeighbours(40, 42);
    assert_eq!(failure.error, KernelError::Invalid(why));
    let (mesh, first) = first_try(&p, 0.0, r, &tol, Budget::DEFAULT.work());
    assert_eq!(first.unwrap_err(), failure);
    assert_eq!(
        failure.evidence.patches,
        named_patches(&mesh, why),
        "{why:?}"
    );
    // The second try's failure, from the first's fork.
    let margin = tol.resolution();
    let mut work = Work::new(&Budget::DEFAULT);
    let mut chain = Chain::new(&p, margin).unwrap();
    chain.separate(&mut work).unwrap();
    let mut fork = None;
    let start = Rounds::new(chain);
    cap::triangulate(
        start,
        margin,
        Mode::QUALITY,
        &mut fork,
        &mut false,
        &mut work,
    )
    .unwrap();
    let fork = fork.expect("a fork");
    let caps = cap::triangulate(
        fork,
        margin,
        Mode::FLAT_CORNERS,
        &mut None,
        &mut false,
        &mut work,
    );
    let (chain, cap) = caps.unwrap();
    let second = build(&chain, &cap, &Frame::XY, 0.0, r, 9).unwrap();
    let second = Solid::new_repaired_within(second, &tol, &mut work).unwrap_err();
    assert!(matches!(second.error, KernelError::Invalid(_)));
    assert_ne!(second.error, failure.error);
}
