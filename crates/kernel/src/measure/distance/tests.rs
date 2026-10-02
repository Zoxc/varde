#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use glam::{DVec2, DVec3};

use super::*;
use crate::mesh::tests::TOL;
use crate::par::assert_deterministic;
use crate::patch::{Conic2, cylinder_strip};
use crate::sweep::tests::{Axis, Revolution, solid_of_revolution};
use crate::topology::NotFound;
use crate::{Frame, Loop, Profile, Segment, Solid, Tolerance, Topology, extrude};

/// A solid with its topology, to pick from.
struct Picked {
    solid: Solid,
    topology: Topology,
}

impl Picked {
    fn new(solid: Solid) -> Picked {
        let topology = solid.topology();
        Picked { solid, topology }
    }

    fn target(&self, pick: Pick) -> Target<'_> {
        Target {
            solid: &self.solid,
            topology: &self.topology,
            pick,
        }
    }

    fn body(&self) -> Target<'_> {
        self.target(Pick::Body)
    }

    /// The face whose form is `want`'s.
    fn face(&self, want: impl Fn(&Form) -> bool) -> Target<'_> {
        let mesh = self.solid.mesh();
        let r = self
            .topology
            .regions()
            .iter()
            .position(|region| {
                want(&mesh.faces()[mesh.tris()[region.tris[0] as usize].face as usize].form)
            })
            .expect("a face");
        self.target(Pick::Face(r as u32))
    }

    /// The plane face with the outward normal `n`.
    fn plane(&self, n: DVec3) -> Target<'_> {
        self.face(|form| matches!(form, Form::Plane { n: m, .. } if m.distance(n) < 1e-12))
    }

    /// The edge from `a` to `b`, either way.
    fn edge(&self, a: DVec3, b: DVec3) -> Target<'_> {
        let mesh = self.solid.mesh();
        let c = self
            .topology
            .chains()
            .iter()
            .position(|chain| {
                let first = mesh.curve(chain.halfedges[0]).p0;
                let last = mesh.curve(*chain.halfedges.last().unwrap()).p1;
                (first, last) == (a, b) || (first, last) == (b, a)
            })
            .expect("an edge");
        self.target(Pick::Edge(c as u32))
    }

    /// The corner at `p`.
    fn corner(&self, p: DVec3) -> Target<'_> {
        let verts = self.solid.mesh().verts();
        let c = self
            .topology
            .corners()
            .iter()
            .position(|corner| verts[corner.vertex as usize] == p)
            .expect("a corner");
        self.target(Pick::Corner(c as u32))
    }
}

fn cuboid(min: DVec3, size: DVec3) -> Picked {
    Picked::new(Solid::cuboid(min, size, 1, &TOL).unwrap())
}

fn between(a: &Target<'_>, b: &Target<'_>) -> Distance {
    between_at(a, b, &TOL)
}

/// The distance at `tol`, the same either way round.
fn between_at(a: &Target<'_>, b: &Target<'_>, tol: &Tolerance) -> Distance {
    let d = distance(a, b, tol, &Budget::DEFAULT).unwrap();
    assert_eq!(d.distance, d.points[0].distance(d.points[1]), "{d:?}");
    let back = distance(b, a, tol, &Budget::DEFAULT).unwrap();
    assert!(
        (back.distance - d.distance).abs() <= 1e-12 * d.distance.max(1.0),
        "{d:?} {back:?}"
    );
    d
}

/// The work a distance takes.
fn spent(a: &Target<'_>, b: &Target<'_>) -> u64 {
    spent_at(a, b, &TOL)
}

fn spent_at(a: &Target<'_>, b: &Target<'_>, tol: &Tolerance) -> u64 {
    let mut work = Work::new(&Budget::DEFAULT);
    distance_within(a, b, tol, &mut work).unwrap();
    Budget::DEFAULT.work() - work.left()
}

/// The fit tolerances at both ends and the default.
fn tolerances() -> [Tolerance; 3] {
    [
        Tolerance::new(Tolerance::MAX_FIT).unwrap(),
        TOL,
        Tolerance::new(Tolerance::MIN_FIT).unwrap(),
    ]
}

/// A plate from the origin to `size`, 4 thick, with round holes.
fn plate(size: DVec2, holes: &[(DVec2, f64)]) -> Picked {
    let corners = [
        DVec2::ZERO,
        DVec2::new(size.x, 0.0),
        size,
        DVec2::new(0.0, size.y),
    ];
    let outline = Loop {
        segments: (0..4)
            .map(|i| Segment::line(corners[i], corners[(i + 1) % 4], i as u64).unwrap())
            .collect(),
    };
    let loops = std::iter::once(outline)
        .chain(holes.iter().map(|&(c, r)| circle(c, r, true)))
        .collect();
    extruded(loops, &Frame::XY, 4.0)
}

/// The cylindrical face of radius `r`.
fn wall(p: &Picked, r: f64) -> Target<'_> {
    p.face(move |form| matches!(form, Form::Cylinder { radius, .. } if (radius - r).abs() < 1e-12))
}

/// Whether `p` is on the surface of the box from `min` to `max`.
fn on_box(p: DVec3, min: DVec3, max: DVec3) -> bool {
    p.cmpge(min).all() && p.cmple(max).all() && (0..3).any(|k| p[k] == min[k] || p[k] == max[k])
}

#[test]
fn two_boxes_are_exactly_their_gap_apart() {
    let a = cuboid(DVec3::ZERO, DVec3::ONE);
    // Face to face, edge to edge and corner to corner.
    for (min, gap) in [
        (DVec3::new(3.0, 0.25, 0.5), DVec3::new(2.0, 0.0, 0.0)),
        (DVec3::new(2.0, 3.0, 0.5), DVec3::new(1.0, 2.0, 0.0)),
        (DVec3::new(2.0, 3.0, 4.0), DVec3::new(1.0, 2.0, 3.0)),
    ] {
        let b = cuboid(min, DVec3::ONE);
        let d = between(&a.body(), &b.body());
        assert_eq!(d.distance, gap.length(), "{d:?}");
        assert!(on_box(d.points[0], DVec3::ZERO, DVec3::ONE), "{d:?}");
        assert!(on_box(d.points[1], min, min + DVec3::ONE), "{d:?}");
        assert_eq!((d.points[1] - d.points[0]).max(DVec3::ZERO), gap, "{d:?}");
    }

    let b = cuboid(DVec3::new(3.0, 0.25, 0.5), DVec3::ONE);
    let d = between(&a.plane(DVec3::X), &b.plane(DVec3::NEG_X));
    assert_eq!(d.distance, 2.0);
    let d = between(&b.corner(DVec3::new(3.0, 0.25, 0.5)), &a.plane(DVec3::X));
    assert_eq!(d.distance, 2.0);
    assert!(
        d.points[1].distance(DVec3::new(1.0, 0.25, 0.5)) < 1e-15,
        "{d:?}"
    );
    // The far face: through the box.
    let d = between(
        &b.corner(DVec3::new(3.0, 0.25, 0.5)),
        &a.plane(DVec3::NEG_X),
    );
    assert_eq!(d.distance, 3.0);
    assert!(
        d.points[1].distance(DVec3::new(0.0, 0.25, 0.5)) < 1e-15,
        "{d:?}"
    );

    let b = cuboid(DVec3::new(2.0, 3.0, 0.5), DVec3::ONE);
    let d = between(
        &a.edge(DVec3::new(1.0, 1.0, 0.0), DVec3::new(1.0, 1.0, 1.0)),
        &b.edge(DVec3::new(2.0, 3.0, 0.5), DVec3::new(2.0, 3.0, 1.5)),
    );
    assert_eq!(d.distance, 5.0f64.sqrt());
    assert!(d.points[0].z >= 0.5 && d.points[0].z <= 1.0, "{d:?}");

    let b = cuboid(DVec3::new(2.0, 3.0, 4.0), DVec3::ONE);
    let d = between(&a.corner(DVec3::ONE), &b.corner(DVec3::new(2.0, 3.0, 4.0)));
    assert_eq!(d.distance, 14.0f64.sqrt());
    assert_eq!(d.points, [DVec3::ONE, DVec3::new(2.0, 3.0, 4.0)]);
}

#[test]
fn picks_of_one_box_meet_where_they_share_points() {
    let a = cuboid(DVec3::ZERO, DVec3::ONE);
    let top = a.plane(DVec3::Z);
    for other in [
        a.plane(DVec3::X),
        a.plane(DVec3::Z),
        a.body(),
        a.edge(DVec3::new(1.0, 1.0, 0.0), DVec3::new(1.0, 1.0, 1.0)),
        a.corner(DVec3::ONE),
    ] {
        assert_eq!(between(&top, &other).distance, 0.0);
    }
    // Opposite faces: the box's height.
    assert_eq!(between(&top, &a.plane(DVec3::NEG_Z)).distance, 1.0);
    let d = between(&a.corner(DVec3::ZERO), &a.plane(DVec3::Z));
    assert_eq!(d.distance, 1.0);
}

#[test]
fn a_point_and_a_cylinder() {
    let (r, h) = (2.0, 3.0);
    let c = Picked::new(Solid::cylinder(DVec3::ZERO, r, h, 1, &TOL).unwrap());
    let wall = c.face(|form| matches!(form, Form::Cylinder { .. }));
    // Off the wall: `ρ − r`, at the foot of the perpendicular; above its
    // rim: from the rim.
    for (p, want, foot) in [
        (DVec3::new(4.0, 3.0, 1.5), 3.0, DVec3::new(1.6, 1.2, 1.5)),
        (
            DVec3::new(4.0, 3.0, 5.0),
            13.0f64.sqrt(),
            DVec3::new(1.6, 1.2, 3.0),
        ),
        (DVec3::new(-3.0, 0.0, 0.5), 1.0, DVec3::new(-2.0, 0.0, 0.5)),
    ] {
        let point = cuboid(p, DVec3::ONE);
        for target in [c.body(), wall] {
            let d = between(&point.corner(p), &target);
            assert!((d.distance - want).abs() < 1e-14, "{p} {d:?}");
            assert!(d.points[1].distance(foot) < 1e-14, "{p} {d:?}");
        }
    }
    // Above the top: the cap.
    let point = cuboid(DVec3::new(0.5, 0.5, 7.0), DVec3::ONE);
    let d = between(&point.corner(DVec3::new(0.5, 0.5, 7.0)), &c.body());
    assert!((d.distance - 4.0).abs() < 1e-14, "{d:?}");
    assert!(
        d.points[1].distance(DVec3::new(0.5, 0.5, 3.0)) < 1e-14,
        "{d:?}"
    );

    // On the axis inside a tall one: every point of the wall is `r` off,
    // which the wall's round tells at once.
    let tall = Picked::new(Solid::cylinder(DVec3::ZERO, r, 10.0, 1, &TOL).unwrap());
    let p = DVec3::new(0.0, 0.0, 5.0);
    let point = cuboid(p, DVec3::ONE);
    let (corner, body) = (point.corner(p), tall.body());
    let d = between(&corner, &body);
    assert!((d.distance - r).abs() < 1e-14, "{d:?}");
    assert!((d.points[1].truncate().length() - r).abs() < 1e-14, "{d:?}");
    assert!(spent(&corner, &body) < 10_000, "{}", spent(&corner, &body));
}

/// A circle of radius `r` round `centre` as four arcs, counter-clockwise,
/// or clockwise for a hole.
fn circle(centre: DVec2, r: f64, hole: bool) -> Loop {
    let at = |i: usize| {
        centre
            + match i % 4 {
                0 => DVec2::new(r, 0.0),
                1 => DVec2::new(0.0, r),
                2 => DVec2::new(-r, 0.0),
                _ => DVec2::new(0.0, -r),
            }
    };
    let mut segments: Vec<Segment> = (0..4)
        .map(|i| Segment {
            conic: Conic2::arc_between(centre, r, at(i), at(i + 1)).unwrap(),
            curve: i as u64,
        })
        .collect();
    if hole {
        segments.reverse();
        for s in &mut segments {
            s.conic = s.conic.reversed();
        }
    }
    Loop { segments }
}

fn extruded(loops: Vec<Loop>, frame: &Frame, height: f64) -> Picked {
    Picked::new(
        extrude(
            &Profile { loops },
            frame,
            0.0,
            height,
            1,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap(),
    )
}

/// A rod of radius `r` from `base` along the unit `along`, `length` long.
fn rod(base: DVec3, along: DVec3, r: f64, length: f64) -> Picked {
    let x = along.any_orthonormal_vector();
    let frame = Frame {
        origin: base,
        x,
        y: along.cross(x),
    };
    extruded(vec![circle(DVec2::ZERO, r, false)], &frame, length)
}

#[test]
fn two_skew_cylinders_are_their_axes_less_their_radii_apart() {
    let (r1, r2, axes) = (1.0, 1.5, 4.0);
    let a = Picked::new(Solid::cylinder(DVec3::new(0.0, 0.0, -5.0), r1, 10.0, 1, &TOL).unwrap());
    // Square to the z axis and to `u`, through (0, 0, 1).
    let u = DVec3::new(1.0, 0.5, 0.0).normalize();
    let n = DVec3::Z.cross(u).normalize();
    let q = DVec3::new(0.0, 0.0, 1.0) + n * axes;
    let b = rod(q - u * 5.0, u, r2, 10.0);
    let want = axes - r1 - r2;
    let (pa, pb) = (DVec3::new(0.0, 0.0, 1.0) + n * r1, q - n * r2);
    fn walls(p: &Picked) -> Target<'_> {
        p.face(|form| matches!(form, Form::Cylinder { .. }))
    }
    for (x, y) in [(a.body(), b.body()), (walls(&a), walls(&b))] {
        let d = between(&x, &y);
        assert!((d.distance - want).abs() < 1e-13, "{d:?} {want}");
        assert!(
            d.points[0].distance(pa) < 1e-7 && d.points[1].distance(pb) < 1e-7,
            "{d:?}"
        );
    }
    // Tilted out of the plane too, the closest points off the seams.
    let u = DVec3::new(0.3, 0.8, 0.5).normalize();
    let n = DVec3::Z.cross(u).normalize();
    let q = DVec3::new(0.0, 0.0, 0.7) + n * axes;
    let b = rod(q - u * 3.0, u, r2, 7.0);
    let d = between(&a.body(), &b.body());
    assert!((d.distance - want).abs() < 1e-13, "{d:?} {want}");
}

#[test]
fn touching_and_crossing_picks_are_nothing_apart() {
    let a = cuboid(DVec3::ZERO, DVec3::ONE);
    // Face to face, an edge on a face, crossing.
    for min in [
        DVec3::new(1.0, 0.2, 0.3),
        DVec3::new(0.5, 1.0, 0.25),
        DVec3::new(0.5, 0.25, -0.5),
    ] {
        let b = cuboid(min, DVec3::ONE);
        let d = between(&a.body(), &b.body());
        assert!(d.distance < 1e-15, "{min} {d:?}");
    }
    // Rods side by side, and crossing. Side by side the squared distance
    // grows as the fourth power of the way round from the line they
    // touch along, so the points settle to about the square root of the
    // rounding, and their distance to some hundred roundings.
    let one = rod(DVec3::ZERO, DVec3::Z, 1.0, 3.0);
    for (base, along, within) in [
        (DVec3::new(2.0, 0.0, 0.5), DVec3::Z, 1e-12),
        (DVec3::new(0.6, 0.8, 0.5), DVec3::Z, 1e-14),
        (DVec3::new(-2.0, 0.3, 1.5), DVec3::X, 1e-14),
    ] {
        let other = rod(base, along, 1.0, 4.0);
        let d = between(&one.body(), &other.body());
        assert!(d.distance < within, "{base} {d:?}");
    }
}

#[test]
fn a_tube_and_a_pin_are_their_radii_apart() {
    let tube = extruded(
        vec![
            circle(DVec2::ZERO, 2.0, false),
            circle(DVec2::ZERO, 1.0, true),
        ],
        &Frame::XY,
        3.0,
    );
    let pin = rod(DVec3::new(0.0, 0.0, -1.0), DVec3::Z, 0.5, 5.0);
    let (outer, inner) = (wall(&tube, 2.0), wall(&tube, 1.0));
    let cases = [
        (outer, inner, 1.0),
        (wall(&pin, 0.5), inner, 0.5),
        (pin.body(), tube.body(), 0.5),
    ];
    for (x, y, want) in cases {
        let d = between(&x, &y);
        assert!((d.distance - want).abs() < 1e-14, "{d:?} {want}");
        // Everywhere the same, which the rounds tell at once: no
        // splitting to the resolution round the walls.
        assert!(spent(&x, &y) < 20_000, "{}", spent(&x, &y));
    }
    // The inner rim, a circle, to the outer wall.
    let top = DVec3::new(1.0, 0.0, 3.0);
    let rim = tube.target(Pick::Edge(
        tube.topology
            .chains()
            .iter()
            .position(|chain| {
                (0..chain.halfedges.len())
                    .any(|i| tube.solid.mesh().curve(chain.halfedges[i]).p0.distance(top) < 1e-12)
            })
            .unwrap() as u32,
    ));
    let d = between(&rim, &outer);
    assert!((d.distance - 1.0).abs() < 1e-14, "{d:?}");
    assert!(spent(&rim, &outer) < 20_000, "{}", spent(&rim, &outer));
}

#[test]
fn rounds_bound_their_pieces() {
    // An exact quarter cylinder strip is its radius from its axis to
    // rounding; moved off it, the range still holds every sample.
    let base = Conic3::new(
        DVec3::new(2.0, 0.0, 0.0),
        DVec3::new(2.0, 2.0, 0.0),
        std::f64::consts::FRAC_1_SQRT_2,
        DVec3::new(0.0, 2.0, 0.0),
    )
    .unwrap();
    let [first, second] = cylinder_strip(&base, DVec3::Z * 3.0).unwrap();
    let axis = Round::line(DVec3::new(0.0, 0.0, -4.0), DVec3::Z).unwrap();
    for patch in [first, second] {
        let (lo, hi) = axis.range(&Shape::Patch(patch)).unwrap();
        assert!(
            (lo - 2.0).abs() < 1e-13 && (hi - 2.0).abs() < 1e-13,
            "{lo} {hi}"
        );
    }
    let (lo, hi) = axis.range(&Shape::Curve(base)).unwrap();
    assert!(
        (lo - 2.0).abs() < 1e-13 && (hi - 2.0).abs() < 1e-13,
        "{lo} {hi}"
    );
    let mut rng = crate::test_rng::Rng::new(5);
    for _ in 0..300 {
        let mut patch = first;
        let k = ((rng.unit() * 6.0) as usize).min(5);
        let nudge = rng.point(0.5);
        if k < 3 {
            patch.p[k] += nudge;
        } else {
            patch.c[k - 3] += nudge;
            patch.w[k - 3] *= rng.log_range(0.5, 2.0);
        }
        let round = if rng.unit() < 0.5 {
            Round::line(rng.point(3.0), rng.direction()).unwrap()
        } else {
            Round::point(rng.point(3.0)).unwrap()
        };
        let (lo, hi) = round.range(&Shape::Patch(patch)).unwrap();
        for i in 0..=8 {
            for j in 0..=8 - i {
                let u = DVec3::new(i as f64, j as f64, (8 - i - j) as f64) / 8.0;
                let f = round.across(patch.eval(u)).length();
                assert!(lo <= f && f <= hi, "{lo} {f} {hi}");
            }
        }
        let curve = patch.edge(k % 3);
        let (lo, hi) = round.range(&Shape::Curve(curve)).unwrap();
        for i in 0..=8 {
            let f = round.across(curve.eval(i as f64 / 8.0)).length();
            assert!(lo <= f && f <= hi, "{lo} {f} {hi}");
        }
    }
    // A circle's arc is on its axis's round and its centre's.
    let rounds = Round::of_curve(&base);
    assert_eq!(rounds[0].unwrap().axis.unwrap().z.abs(), 1.0);
    assert_eq!(rounds[1].unwrap().axis, None);
    let line = Conic3::line(DVec3::ZERO, DVec3::X).unwrap();
    assert_eq!(Round::of_curve(&line), [None, None]);
}

#[test]
fn distances_are_the_same_on_any_thread_count() {
    let a = Picked::new(Solid::cylinder(DVec3::new(0.0, 0.0, -5.0), 1.0, 10.0, 1, &TOL).unwrap());
    let u = DVec3::new(0.3, 0.8, 0.5).normalize();
    let n = DVec3::Z.cross(u).normalize();
    let b = rod(DVec3::new(0.0, 0.0, 0.7) + n * 4.0 - u * 3.0, u, 1.5, 7.0);
    let c = cuboid(DVec3::new(2.0, -1.0, 3.0), DVec3::new(1.0, 2.0, 1.0));
    let tube = extruded(
        vec![
            circle(DVec2::ZERO, 2.0, false),
            circle(DVec2::ZERO, 1.0, true),
        ],
        &Frame::XY,
        3.0,
    );
    assert_deterministic(|| {
        [
            (a.body(), b.body()),
            (a.body(), c.body()),
            (b.body(), c.body()),
            (c.corner(DVec3::new(2.0, -1.0, 3.0)), a.body()),
            (tube.body(), a.body()),
        ]
        .map(|(x, y)| {
            let mut work = Work::new(&Budget::DEFAULT);
            let d = distance_within(&x, &y, &TOL, &mut work);
            (d, work.left())
        })
        .to_vec()
    });
}

#[test]
fn past_the_budget_is_too_complex() {
    let a = Picked::new(Solid::cylinder(DVec3::new(0.0, 0.0, -5.0), 1.0, 10.0, 1, &TOL).unwrap());
    let u = DVec3::new(0.3, 0.8, 0.5).normalize();
    let n = DVec3::Z.cross(u).normalize();
    let b = rod(DVec3::new(0.0, 0.0, 0.7) + n * 4.0 - u * 3.0, u, 1.5, 7.0);
    let needed = spent(&a.body(), &b.body());
    assert_eq!(
        distance(&a.body(), &b.body(), &TOL, &Budget::new(needed - 1)),
        Err(MeasureError::TooComplex)
    );
    assert!(distance(&a.body(), &b.body(), &TOL, &Budget::new(needed)).is_ok());
    assert_eq!(
        distance(&a.body(), &b.body(), &TOL, &Budget::new(10)),
        Err(MeasureError::TooComplex)
    );
}

#[test]
fn picks_naming_nothing_or_empty_bodies_have_no_distance() {
    let a = cuboid(DVec3::ZERO, DVec3::ONE);
    let missing = [
        (Pick::Face(6), NotFound::Face),
        (Pick::Edge(12), NotFound::Edge),
        (Pick::Corner(8), NotFound::Corner),
    ];
    for (pick, error) in missing {
        assert_eq!(
            distance(&a.target(pick), &a.body(), &TOL, &Budget::DEFAULT),
            Err(MeasureError::NotFound(error))
        );
        assert_eq!(
            distance(&a.body(), &a.target(pick), &TOL, &Budget::DEFAULT),
            Err(MeasureError::NotFound(error))
        );
    }
    let empty = Picked::new(Solid::empty());
    assert_eq!(
        distance(&empty.body(), &a.body(), &TOL, &Budget::DEFAULT),
        Err(MeasureError::Empty)
    );
    assert_eq!(MeasureError::Empty.to_string(), "the body is empty");
}

#[test]
fn random_points_and_a_cylinder_are_their_analytic_distance_apart() {
    // Inside, outside, past the rims, at every tolerance.
    let mut rng = crate::test_rng::Rng::new(11);
    let (base, r, h) = (DVec3::new(0.5, -0.5, 1.0), 1.5, 4.0);
    for tol in tolerances() {
        let c = Picked::new(Solid::cylinder(base, r, h, 1, &tol).unwrap());
        for _ in 0..60 {
            let p = rng.point(5.0);
            let q = p - base;
            let (rho, z) = (q.truncate().length(), q.z);
            let want = if rho <= r && (0.0..=h).contains(&z) {
                (r - rho).min(z).min(h - z)
            } else {
                (rho - r).max(0.0).hypot((-z).max(z - h).max(0.0))
            };
            let point = cuboid(p, DVec3::splat(0.1));
            let d = between_at(&point.corner(p), &c.body(), &tol);
            assert!(
                (d.distance - want).abs() < 1e-14 * want.max(1.0),
                "{p} {d:?} {want}"
            );
        }
    }
}

#[test]
fn random_boxes_are_exactly_their_gap_apart() {
    let mut rng = crate::test_rng::Rng::new(12);
    let size = |rng: &mut crate::test_rng::Rng| {
        DVec3::new(
            rng.range(0.1, 2.0),
            rng.range(0.1, 2.0),
            rng.range(0.1, 2.0),
        )
    };
    for tol in tolerances() {
        for _ in 0..20 {
            let (m0, s0, m1, s1) = (
                rng.point(3.0),
                size(&mut rng),
                rng.point(3.0),
                size(&mut rng),
            );
            let gap = (m0 - (m1 + s1)).max(m1 - (m0 + s0)).max(DVec3::ZERO);
            if gap == DVec3::ZERO {
                continue;
            }
            let (a, b) = (cuboid(m0, s0), cuboid(m1, s1));
            assert_eq!(
                between_at(&a.body(), &b.body(), &tol).distance,
                gap.length()
            );
        }
    }
}

#[test]
fn random_skew_rods_are_their_axes_less_their_radii_apart() {
    // Their axes' closest points well inside both.
    let mut rng = crate::test_rng::Rng::new(13);
    for _ in 0..24 {
        let (r1, r2) = (rng.range(0.2, 1.5), rng.range(0.2, 1.5));
        let u1 = rng.direction();
        let mut u2 = rng.direction();
        while u1.cross(u2).length() < 0.2 {
            u2 = rng.direction();
        }
        let n = u1.cross(u2).normalize();
        let gap = rng.log_range(1e-4, 3.0);
        let c1 = rng.point(3.0);
        let c2 = c1 + n * (r1 + r2 + gap);
        let (l1, l2) = (rng.range(2.0, 6.0), rng.range(2.0, 6.0));
        let (s1, s2) = (rng.range(0.25, 0.75) * l1, rng.range(0.25, 0.75) * l2);
        let a = rod(c1 - u1 * s1, u1, r1, l1);
        let b = rod(c2 - u2 * s2, u2, r2, l2);
        let d = between(&a.body(), &b.body());
        assert!((d.distance - gap).abs() < 1e-14 * 8.0, "{d:?} {gap}");
        assert!(d.points[0].distance(c1 + n * r1) < 1e-6, "{d:?}");
    }
}

#[test]
fn a_pin_along_a_holes_wall_and_two_holes() {
    // A pin off the hole's centre is nearest along a line of its wall:
    // the bodies, whose ends are rounds about the pin, at once; the walls
    // alone by splitting along that line, more at finer tolerances.
    let p = plate(
        DVec2::new(10.0, 6.0),
        &[(DVec2::new(3.0, 2.7), 1.0), (DVec2::new(7.1, 3.4), 0.6)],
    );
    let pin = rod(DVec3::new(3.2, 2.7, -1.0), DVec3::Z, 0.5, 6.0);
    for tol in tolerances() {
        let d = between_at(&pin.body(), &p.body(), &tol);
        assert!((d.distance - 0.3).abs() < 1e-15, "{d:?}");
        assert!(spent_at(&pin.body(), &p.body(), &tol) < 10_000);
        let d = between_at(&wall(&pin, 0.5), &wall(&p, 1.0), &tol);
        assert!((d.distance - 0.3).abs() < 1e-15, "{d:?}");
    }
    let want = DVec2::new(3.0, 2.7).distance(DVec2::new(7.1, 3.4)) - 1.6;
    let d = between(&wall(&p, 1.0), &wall(&p, 0.6));
    assert!((d.distance - want).abs() < 1e-14, "{d:?}");
    // And their rims at the top.
    let rim = |c: DVec2, r: f64| {
        let top = DVec3::new(c.x + r, c.y, 4.0);
        let chains = p.topology.chains();
        let c = chains
            .iter()
            .position(|chain| {
                chain
                    .halfedges
                    .iter()
                    .any(|&h| p.solid.mesh().curve(h).p0 == top)
            })
            .unwrap();
        p.target(Pick::Edge(c as u32))
    };
    let d = between(
        &rim(DVec2::new(3.0, 2.7), 1.0),
        &rim(DVec2::new(7.1, 3.4), 0.6),
    );
    assert!((d.distance - want).abs() < 1e-14, "{d:?}");
    // A plate is nothing from itself.
    assert_eq!(between(&p.body(), &p.body()).distance, 0.0);
}

#[test]
fn concentric_spheres_are_their_radii_apart() {
    let zone = |radius: f64, heights: &[f64]| {
        let axis = DVec3::new(0.3, 0.2, 1.0).normalize();
        let u = axis.any_orthonormal_vector();
        let axis = Axis {
            origin: DVec3::ZERO,
            u,
            v: axis.cross(u),
            z: axis,
        };
        let surface = Revolution::sphere(axis, radius);
        let stations: Vec<(f64, f64)> = heights.iter().map(|&h| (surface.rho(h), h)).collect();
        let surfaces = vec![surface; heights.len() - 1];
        Picked::new(
            Solid::new(
                solid_of_revolution(axis, &stations, &surfaces, 8, false),
                &TOL,
            )
            .unwrap(),
        )
    };
    let big = zone(3.0, &[-2.0, 0.0, 2.0]);
    let small = zone(1.0, &[-0.9, 0.0, 0.9]);
    fn sphere(p: &Picked, r: f64) -> Target<'_> {
        let mesh = p.solid.mesh();
        let regions = p.topology.regions();
        let i = regions
            .iter()
            .position(|region| {
                let face = mesh.tris()[region.tris[0] as usize].face as usize;
                matches!(mesh.faces()[face].form, Form::Sphere { radius, .. } if radius == r)
            })
            .unwrap();
        p.target(Pick::Face(i as u32))
    }
    // Every point of the small one's band is 2 from the big one's: their
    // rounds show it at once.
    let (outer, inner) = (sphere(&big, 3.0), sphere(&small, 1.0));
    let d = between(&outer, &inner);
    assert!((d.distance - 2.0).abs() < 1e-14, "{d:?}");
    assert!(spent(&outer, &inner) < 20_000, "{}", spent(&outer, &inner));
    // The bodies: the small one's cap to the big one's.
    let d = between(&big.body(), &small.body());
    assert!((d.distance - 1.1).abs() < 1e-14, "{d:?}");
}

#[test]
fn bodies_far_apart_or_far_out_cost_little() {
    // A plate with 64 holes against a box a long way off, and over it:
    // the trees' boxes drop all but a few pairs.
    let holes: Vec<(DVec2, f64)> = (0..64)
        .map(|k| {
            (
                DVec2::new(1.25 + 2.5 * (k % 8) as f64, 1.25 + 2.5 * (k / 8) as f64),
                0.7,
            )
        })
        .collect();
    let grid = plate(DVec2::new(20.0, 20.0), &holes);
    assert!(grid.solid.mesh().tris().len() > 3000);
    let lid = cuboid(DVec3::new(3.0, 4.0, 4.5), DVec3::new(5.0, 5.0, 1.0));
    let far = cuboid(DVec3::new(300.0, -200.0, 50.0), DVec3::ONE);
    for (other, want) in [(&lid, 0.5), (&far, DVec3::new(280.0, 199.0, 46.0).length())] {
        let d = between(&grid.body(), &other.body());
        assert!((d.distance - want).abs() < 1e-12 * want, "{d:?} {want}");
        assert!(
            spent(&grid.body(), &other.body()) < 40_000,
            "{}",
            spent(&grid.body(), &other.body())
        );
    }
    // The same rods near the origin and far out: the same to the
    // rounding of the coordinates there.
    let at = |o: DVec3| {
        let a = rod(o, DVec3::Z, 1.0, 3.0);
        let b = rod(
            o + DVec3::new(2.5, 0.3, -1.0),
            DVec3::new(0.0, 0.6, 0.8),
            0.7,
            4.0,
        );
        between(&a.body(), &b.body()).distance
    };
    let o = DVec3::new(9000.0, -7000.0, 5000.0);
    assert!((at(o) - at(DVec3::ZERO)).abs() < 64.0 * f64::EPSILON * 9000.0);
}

#[test]
fn a_pin_off_centre_in_a_tube_near_the_coordinate_limit() {
    // A tube (radii 5 and 3) and a pin (radius 2) a quarter off its axis,
    // along a tilted axis a millionth short of the coordinate limit, at
    // the finest fit: the pin's wall is nearest the tube's walls along a
    // line, 0.75 from the bore and 2.75 from the outside. The rounds
    // show the pieces off that line apart only if their rounding goes by
    // the pieces' reach from the axis, not by the coordinates (which, at
    // `64 ε` of them, is more than the resolution here: too complex).
    use crate::profile::tests::circle;
    let tol = Tolerance::new(Tolerance::MIN_FIT).unwrap();
    let y = DVec3::new(0.5, -0.4, -0.7).normalize();
    let frame = Frame {
        origin: DVec3::new(999_000.0, -999_000.0, 999_000.0),
        x: y.any_orthonormal_vector(),
        y,
    };
    let make = |loops: Vec<Loop>, from: f64, to: f64, feature: u64| {
        Picked::new(
            extrude(
                &Profile { loops },
                &frame,
                from,
                to,
                feature,
                &tol,
                &Budget::DEFAULT,
            )
            .unwrap(),
        )
    };
    let tube = make(
        vec![
            circle(DVec2::ZERO, 5.0, 1, false),
            circle(DVec2::ZERO, 3.0, 2, true),
        ],
        0.0,
        15.0,
        1,
    );
    let pin = make(
        vec![circle(DVec2::new(0.25, 0.0), 2.0, 1, false)],
        -5.0,
        20.0,
        2,
    );
    let rounding = 64.0 * f64::EPSILON * 1e6;
    for (outer, want) in [(3.0, 0.75), (5.0, 2.75)] {
        let (a, b) = (wall(&tube, outer), wall(&pin, 2.0));
        for (a, b) in [(&a, &b), (&b, &a)] {
            let d = distance(a, b, &tol, &Budget::DEFAULT).unwrap();
            assert!(
                (d.distance - want).abs() <= tol.resolution() + rounding,
                "{outer}: {d:?}"
            );
        }
        let units = spent_at(&a, &b, &tol);
        assert!(units < 200_000, "{outer}: {units}");
    }
}

/// A ball of `radius` round `centre`, turned fully about the axis `y`
/// (unit) through it.
fn ball(centre: DVec3, y: DVec3, radius: f64) -> Picked {
    use crate::profile::tests::arc;
    let v = DVec2::new;
    let profile = Profile {
        loops: vec![Loop {
            segments: vec![
                arc(v(0.0, 0.0), v(0.0, -radius), v(radius, 0.0), 1),
                arc(v(0.0, 0.0), v(radius, 0.0), v(0.0, radius), 1),
                Segment::line(v(0.0, radius), v(0.0, -radius), 2).unwrap(),
            ],
        }],
    };
    let frame = Frame {
        origin: centre,
        x: y.any_orthonormal_vector(),
        y,
    };
    Picked::new(
        crate::revolve(
            &profile,
            &frame,
            crate::Sweep::Full,
            1,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap(),
    )
}

#[test]
fn revolved_balls_are_their_analytic_distance_from_others() {
    // Exact sphere strips but for the fitted caps round the poles, within
    // half the fit of the sphere.
    let near_pole = TOL.fit() / 2.0 + TOL.resolution();
    let (ca, cb) = (DVec3::new(1.0, 2.0, -1.0), DVec3::new(6.0, -2.0, 1.5));
    let a = ball(ca, DVec3::new(0.3, -0.2, 1.0).normalize(), 3.0);
    let b = ball(cb, DVec3::Y, 1.5);
    let on = |p: DVec3, c: DVec3, r: f64, slack: f64| (p.distance(c) - r).abs() <= slack;

    // Ball to ball, away from their poles: to rounding, and cheap, their
    // centres' rounds bounding every pair of pieces.
    let d = between(&a.body(), &b.body());
    assert!(
        (d.distance - (ca.distance(cb) - 4.5)).abs() < 1e-12,
        "{d:?}"
    );
    assert!(on(d.points[0], ca, 3.0, 1e-12) && on(d.points[1], cb, 1.5, 1e-12));
    assert!(spent(&a.body(), &b.body()) < 20_000);
    // Their sphere faces, the same.
    fn sphere(p: &Picked) -> Target<'_> {
        p.face(|f| matches!(f, Form::Sphere { .. }))
    }
    let faces = between(&sphere(&a), &sphere(&b));
    assert!((faces.distance - d.distance).abs() < 1e-12, "{faces:?}");

    // A lid over the tilted ball: its top, off its poles, to rounding.
    let lid = cuboid(DVec3::new(-3.0, -2.0, 3.0), DVec3::new(8.0, 8.0, 1.0));
    let d = between(&a.body(), &lid.body());
    assert!((d.distance - 1.0).abs() < 1e-12, "{d:?}");
    assert!(d.points[0].distance(ca + DVec3::Z * 3.0) < 1e-6, "{d:?}");
    let d = between(&sphere(&a), &lid.plane(DVec3::NEG_Z));
    assert!((d.distance - 1.0).abs() < 1e-12, "{d:?}");

    // A lid over the upright ball's pole: within its cap's fit.
    let over = cuboid(DVec3::new(4.0, 0.5, -0.5), DVec3::new(4.0, 1.0, 4.0));
    let d = between(&b.body(), &over.body());
    assert!((d.distance - 1.0).abs() <= near_pole, "{d:?}");
    assert!(on(d.points[0], cb, 1.5, near_pole), "{d:?}");
    assert!((d.points[1].y - 0.5).abs() < 1e-12, "{d:?}");

    // A box's corner and edge to the tilted ball.
    let corner = DVec3::new(6.0, 6.0, 4.0);
    let block = cuboid(corner, DVec3::ONE);
    let d = between(&a.body(), &block.corner(corner));
    assert!(
        (d.distance - (corner.distance(ca) - 3.0)).abs() < 1e-12,
        "{d:?}"
    );
    assert_eq!(d.points[1], corner);
    let end = corner + DVec3::X;
    let d = between(&sphere(&a), &block.edge(corner, end));
    assert!(
        (d.distance - (corner.distance(ca) - 3.0)).abs() < 1e-12,
        "{d:?}"
    );
}

#[test]
fn random_revolved_bodies_are_no_further_apart_than_their_points() {
    // A torus and a part turn of a cone placed at random: the distance is
    // between points of the two (checked by `between`) and no more than
    // that of any two of their patches' sampled points; where they cross,
    // 0.
    use crate::profile::tests::{circle, polygon};
    let v = DVec2::new;
    let mut rng = crate::test_rng::Rng::new(77);
    let samples = |s: &Solid| -> Vec<DVec3> {
        let mesh = s.mesh();
        (0..mesh.tris().len())
            .flat_map(|t| {
                let patch = mesh.patch(t);
                (0..=3).flat_map(move |i| {
                    (0..=3 - i).map(move |j| {
                        patch.eval(DVec3::new(i as f64, j as f64, (3 - i - j) as f64) / 3.0)
                    })
                })
            })
            .collect()
    };
    let mut frame = |extent: f64| {
        let y = rng.direction();
        Frame {
            origin: rng.point(extent),
            x: y.any_orthonormal_vector(),
            y,
        }
    };
    let mut crossing = 0;
    for _ in 0..20 {
        let (fa, fb) = (frame(3.0), frame(12.0));
        let torus = Profile {
            loops: vec![circle(v(4.0, 0.0), 1.0, 1, false)],
        };
        let cone = Profile {
            loops: vec![polygon(&[v(0.0, 0.0), v(2.0, 0.0), v(0.0, 3.0)], 1)],
        };
        let part = crate::Sweep::Part { from: 0.3, to: 4.0 };
        let a = Picked::new(
            crate::revolve(&torus, &fa, crate::Sweep::Full, 1, &TOL, &Budget::DEFAULT).unwrap(),
        );
        let b = Picked::new(crate::revolve(&cone, &fb, part, 1, &TOL, &Budget::DEFAULT).unwrap());
        let d = between(&a.body(), &b.body());
        let (sa, sb) = (samples(&a.solid), samples(&b.solid));
        let sampled = sa
            .iter()
            .flat_map(|p| sb.iter().map(move |q| p.distance(*q)))
            .fold(f64::INFINITY, f64::min);
        assert!(d.distance <= sampled, "{d:?} {sampled}");
        if d.distance < 1e-12 {
            crossing += 1;
        }
    }
    // Seed 77 places two of them crossing.
    assert_eq!(crossing, 2);
}
