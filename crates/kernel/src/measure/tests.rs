#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::PI;

use glam::{DVec2, DVec3};

use super::*;
use crate::mesh::tests::TOL;
use crate::par::assert_deterministic;
use crate::patch::Conic2;
use crate::sweep::tests::{Axis, Revolution, solid_of_revolution};
use crate::{Frame, Loop, Profile, Segment, extrude};

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(f64::MIN_POSITIVE)
}

fn close3(a: DVec3, b: DVec3, scale: f64, rel: f64) -> bool {
    (a - b).abs().max_element() <= rel * scale
}

fn target<'a>(solid: &'a Solid, topology: &'a Topology, pick: Pick) -> Target<'a> {
    Target {
        solid,
        topology,
        pick,
    }
}

fn measured(solid: &Solid, topology: &Topology, pick: Pick) -> Measured {
    measure(&target(solid, topology, pick), &TOL, &Budget::DEFAULT).unwrap()
}

fn body_of(solid: &Solid) -> BodyMeasure {
    let topology = solid.topology();
    match measured(solid, &topology, Pick::Body) {
        Measured::Body(body) => body,
        other => panic!("{other:?}"),
    }
}

fn edges(solid: &Solid) -> Vec<EdgeMeasure> {
    let topology = solid.topology();
    (0..topology.chains().len() as u32)
        .map(|c| match measured(solid, &topology, Pick::Edge(c)) {
            Measured::Edge(edge) => edge,
            other => panic!("{other:?}"),
        })
        .collect()
}

fn faces(solid: &Solid) -> Vec<FaceMeasure> {
    let topology = solid.topology();
    (0..topology.regions().len() as u32)
        .map(|r| match measured(solid, &topology, Pick::Face(r)) {
            Measured::Face(face) => face,
            other => panic!("{other:?}"),
        })
        .collect()
}

fn extruded(loops: Vec<Loop>, frame: &Frame, height: f64) -> Solid {
    extrude(
        &Profile { loops },
        frame,
        0.0,
        height,
        1,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap()
}

fn polygon(points: &[DVec2]) -> Loop {
    Loop {
        segments: (0..points.len())
            .map(|i| Segment::line(points[i], points[(i + 1) % points.len()], i as u64).unwrap())
            .collect(),
    }
}

/// A tilted frame: its normal along `(1, 2, 3)`.
fn tilted() -> Frame {
    let axis = DVec3::new(1.0, 2.0, 3.0).normalize();
    let x = axis.any_orthonormal_vector();
    Frame {
        origin: DVec3::new(30.0, -20.0, 10.0),
        x,
        y: axis.cross(x),
    }
}

/// The complete elliptic integral of the second kind `E(k)`, by the
/// arithmetic-geometric mean.
fn elliptic_e(k: f64) -> f64 {
    let (mut a, mut b, mut c) = (1.0, (1.0 - k * k).sqrt(), k);
    let (mut sum, mut power) = (0.5 * c * c, 0.5);
    while c.abs() > 1e-17 {
        (a, b, c) = ((a + b) / 2.0, (a * b).sqrt(), (a - b) / 2.0);
        power *= 2.0;
        sum += power * c * c;
    }
    PI / (2.0 * a) * (1.0 - sum)
}

#[test]
fn a_box_measures_its_lengths_areas_volume_and_centre() {
    let (min, size) = (DVec3::new(1.0, -2.0, 3.0), DVec3::new(2.0, 3.0, 4.0));
    let solid = Solid::cuboid(min, size, 1, &TOL).unwrap();
    let topology = solid.topology();

    let mut lengths: Vec<f64> = edges(&solid).iter().map(|e| e.length).collect();
    lengths.sort_by(f64::total_cmp);
    assert_eq!(
        lengths,
        [2.0; 4]
            .into_iter()
            .chain([3.0; 4])
            .chain([4.0; 4])
            .collect::<Vec<_>>()
    );
    for edge in edges(&solid) {
        let EdgeShape::Line { from, to } = edge.shape else {
            panic!("{edge:?}");
        };
        assert!(!edge.closed);
        assert_eq!(edge.point(), Some((from + to) * 0.5));
        let d = edge.direction().unwrap();
        assert!(d.line && d.v.abs().max_element() == 1.0, "{d:?}");
    }

    let mut areas: Vec<f64> = faces(&solid).iter().map(|f| f.area).collect();
    areas.sort_by(f64::total_cmp);
    for (a, b) in areas.iter().zip([6.0, 6.0, 8.0, 8.0, 12.0, 12.0]) {
        assert!(close(*a, b, 1e-12), "{a} {b}");
    }
    for face in faces(&solid) {
        let d = face.direction().unwrap();
        assert!(!d.line && d.v.abs().max_element() == 1.0);
        // The normal points out: away from the middle.
        let Form::Plane { n, d } = face.form else {
            panic!("a plane");
        };
        assert!(n.dot(min + size * 0.5) < d);
    }

    let body = body_of(&solid);
    assert!(close(body.volume, 24.0, 1e-12), "{}", body.volume);
    assert!(close(body.area, 52.0, 1e-12), "{}", body.area);
    assert!(close3(body.centre.unwrap(), min + size * 0.5, 4.0, 1e-12));
    let bounds = body.bounds.unwrap();
    assert_eq!((bounds.min, bounds.max), (min, min + size));
    let moments = solid.moments(&Budget::DEFAULT).unwrap();
    assert_eq!((moments.volume, moments.centre), (body.volume, body.centre));

    for (c, corner) in topology.corners().iter().enumerate() {
        let p = measured(&solid, &topology, Pick::Corner(c as u32));
        assert_eq!(
            p,
            Measured::Corner(solid.mesh().verts()[corner.vertex as usize])
        );
        assert_eq!(
            p.point(),
            Some(solid.mesh().verts()[corner.vertex as usize])
        );
    }
    assert_eq!(topology.corners().len(), 8);
}

#[test]
fn a_cylinder_measures_its_circles_and_walls() {
    let (base, r, h) = (DVec3::new(1.0, 2.0, 3.0), 2.0, 5.0);
    let solid = Solid::cylinder(base, r, h, 1, &TOL).unwrap();

    let rims = edges(&solid);
    assert_eq!(rims.len(), 2);
    let mut heights = Vec::new();
    for rim in &rims {
        assert!(rim.closed);
        assert!(close(rim.length, 2.0 * PI * r, 1e-12), "{}", rim.length);
        let EdgeShape::Circle {
            centre,
            axis,
            radius,
        } = rim.shape
        else {
            panic!("{rim:?}");
        };
        assert!(close(radius, r, 1e-12));
        assert!(close3(
            centre.truncate().extend(0.0),
            base.truncate().extend(0.0),
            1.0,
            1e-15
        ));
        assert_eq!(axis.z.abs(), 1.0);
        assert_eq!(rim.point(), Some(centre));
        assert_eq!(rim.direction().unwrap().v.z.abs(), 1.0);
        heights.push(centre.z);
    }
    heights.sort_by(f64::total_cmp);
    assert_eq!(heights, [base.z, base.z + h]);

    let faces = faces(&solid);
    assert_eq!(faces.len(), 3);
    let mut walls = 0;
    for face in &faces {
        match face.form {
            Form::Cylinder { axis, radius, .. } => {
                walls += 1;
                assert!(close(face.area, 2.0 * PI * r * h, 1e-12), "{}", face.area);
                assert_eq!(radius, r);
                assert_eq!(axis, DVec3::Z);
                assert_eq!(
                    face.direction(),
                    Some(Direction {
                        v: DVec3::Z,
                        line: true
                    })
                );
            }
            Form::Plane { .. } => assert!(close(face.area, PI * r * r, 1e-12), "{}", face.area),
            form => panic!("{form:?}"),
        }
    }
    assert_eq!(walls, 1);

    let body = body_of(&solid);
    assert!(close(body.volume, PI * r * r * h, 1e-12));
    assert!(close(body.area, 2.0 * PI * r * (r + h), 1e-12));
    assert!(close3(
        body.centre.unwrap(),
        base + DVec3::Z * h * 0.5,
        h,
        1e-12
    ));
    let bounds = body.bounds.unwrap();
    let corner = DVec3::new(r, r, 0.0);
    assert!(close3(bounds.min, base - corner, h, 1e-15));
    assert!(close3(bounds.max, base + corner + DVec3::Z * h, h, 1e-15));
}

#[test]
fn a_half_cylinder_has_its_centroid_at_four_r_over_three_pi() {
    // Two quarter arcs over the diameter, extruded.
    let r = 3.0;
    let (a, top, b) = (DVec2::new(r, 0.0), DVec2::new(0.0, r), DVec2::new(-r, 0.0));
    let half = Loop {
        segments: vec![
            Segment {
                conic: Conic2::arc_between(DVec2::ZERO, r, a, top).unwrap(),
                curve: 0,
            },
            Segment {
                conic: Conic2::arc_between(DVec2::ZERO, r, top, b).unwrap(),
                curve: 0,
            },
            Segment::line(b, a, 1).unwrap(),
        ],
    };
    let h = 2.0;
    let solid = extruded(vec![half], &Frame::XY, h);
    let body = body_of(&solid);
    assert!(close(body.volume, PI * r * r * h / 2.0, 1e-12));
    let centre = body.centre.unwrap();
    let exact = DVec3::new(0.0, 4.0 * r / (3.0 * PI), h / 2.0);
    assert!(close3(centre, exact, r, 1e-12), "{centre} {exact}");
    let bounds = body.bounds.unwrap();
    assert!(close3(bounds.min, DVec3::new(-r, 0.0, 0.0), r, 1e-15));
    assert!(close3(bounds.max, DVec3::new(r, r, h), r, 1e-15));

    // The rims: two half circles of length πr, and the diameters.
    let edges = edges(&solid);
    let arcs: Vec<&EdgeMeasure> = edges
        .iter()
        .filter(|e| matches!(e.shape, EdgeShape::Circle { .. }))
        .collect();
    assert_eq!(arcs.len(), 2);
    for arc in arcs {
        assert!(close(arc.length, PI * r, 1e-14), "{}", arc.length);
    }
}

#[test]
fn an_ellipse_quarter_has_its_length() {
    let (a, b) = (10.0f64, 4.0f64);
    let exact = a * elliptic_e((1.0f64 - b * b / (a * a)).sqrt());
    let quarter = Conic3::new(
        DVec3::new(a, 0.0, 0.0),
        DVec3::new(a, b, 0.0),
        0.5f64.sqrt(),
        DVec3::new(0.0, b, 0.0),
    )
    .unwrap();
    let mut work = Work::new(&Budget::DEFAULT);
    let length = curve_length(&quarter, &mut work).unwrap();
    assert!(close(length, exact, 1e-13), "{length} {exact}");
    assert!(close(
        curve_length(&quarter.reversed(), &mut work).unwrap(),
        exact,
        1e-13
    ));

    // The quarter's sector, extruded on a tilted frame: its rims are
    // arcs of the ellipse, of that length, about its centre.
    let frame = tilted();
    let sector = Loop {
        segments: vec![
            Segment::line(DVec2::ZERO, DVec2::new(a, 0.0), 0).unwrap(),
            Segment {
                conic: Conic2::new(
                    DVec2::new(a, 0.0),
                    DVec2::new(a, b),
                    0.5f64.sqrt(),
                    DVec2::new(0.0, b),
                )
                .unwrap(),
                curve: 1,
            },
            Segment::line(DVec2::new(0.0, b), DVec2::ZERO, 2).unwrap(),
        ],
    };
    let solid = extruded(vec![sector], &frame, 1.0);
    let rims: Vec<EdgeMeasure> = edges(&solid)
        .into_iter()
        .filter(|e| matches!(e.shape, EdgeShape::Ellipse { .. }))
        .collect();
    assert_eq!(rims.len(), 2);
    for rim in rims {
        assert!(close(rim.length, exact, 1e-13), "{} {exact}", rim.length);
        let EdgeShape::Ellipse {
            centre,
            axis,
            major,
            minor,
        } = rim.shape
        else {
            unreachable!()
        };
        assert!(
            close(major, a, 1e-12) && close(minor, b, 1e-12),
            "{major} {minor}"
        );
        let foot = frame.origin + frame.normal() * (centre - frame.origin).dot(frame.normal());
        assert!(close3(centre, foot, a, 1e-12), "{centre} {foot}");
        assert!(axis.cross(frame.normal()).length() < 1e-12);
    }
    // The curved wall is the cylinder over the ellipse's arc.
    let walls = faces(&solid)
        .into_iter()
        .filter(|f| matches!(f.form, Form::ConicCylinder { .. }))
        .count();
    assert_eq!(walls, 1);
}

#[test]
fn a_tilted_box_is_as_tight_as_its_corners_and_a_tilted_cylinder_tighter() {
    let frame = tilted();
    let rect = polygon(&[
        DVec2::new(-1.0, -2.0),
        DVec2::new(3.0, -2.0),
        DVec2::new(3.0, 1.0),
        DVec2::new(-1.0, 1.0),
    ]);
    let solid = extruded(vec![rect], &frame, 5.0);
    let tight = solid.tight_bounds(&TOL, &Budget::DEFAULT).unwrap().unwrap();
    let corners = Bounds3::around(solid.mesh().verts()).unwrap();
    // Flat: the extremes are corners, and the control points are the
    // edges' middles, inside the corners' box.
    assert_eq!((tight.min, tight.max), (corners.min, corners.max));
    assert_eq!(solid.bounds3(), Some(tight));
    let body = body_of(&solid);
    assert!(close(body.volume, 60.0, 1e-12));
    let centre = frame.point(DVec2::new(1.0, -0.5), 2.5);
    assert!(close3(body.centre.unwrap(), centre, 40.0, 1e-12));

    // A circle of radius 2 extruded 5 along a tilted axis: along each
    // world axis `k` its rims reach `r·√(1 − a_k²)` either side of their
    // centres, and the wall adds nothing.
    let r = 2.0;
    let circle = Loop {
        segments: (0..4)
            .map(|i| {
                let at = |i: usize| match i % 4 {
                    0 => DVec2::new(r, 0.0),
                    1 => DVec2::new(0.0, r),
                    2 => DVec2::new(-r, 0.0),
                    _ => DVec2::new(0.0, -r),
                };
                Segment {
                    conic: Conic2::arc_between(DVec2::ZERO, r, at(i), at(i + 1)).unwrap(),
                    curve: 0,
                }
            })
            .collect(),
    };
    let solid = extruded(vec![circle], &frame, 5.0);
    let tight = solid.tight_bounds(&TOL, &Budget::DEFAULT).unwrap().unwrap();
    let n = frame.normal();
    let reach = (DVec3::ONE - n * n).max(DVec3::ZERO).map(f64::sqrt) * r;
    let (bottom, top) = (frame.origin, frame.origin + n * 5.0);
    let exact_min = bottom.min(top) - reach;
    let exact_max = bottom.max(top) + reach;
    assert!(
        close3(tight.min, exact_min, 40.0, 1e-14),
        "{} {exact_min}",
        tight.min
    );
    assert!(
        close3(tight.max, exact_max, 40.0, 1e-14),
        "{} {exact_max}",
        tight.max
    );
    let control = solid.bounds3().unwrap();
    assert!(control.min.cmplt(tight.min - 0.1).all() && control.max.cmpgt(tight.max + 0.1).all());
    let body = body_of(&solid);
    assert!(close(body.volume, PI * r * r * 5.0, 1e-12));
    assert!(close3(
        body.centre.unwrap(),
        frame.point(DVec2::ZERO, 2.5),
        40.0,
        1e-12
    ));
}

/// A sphere of radius `radius` about the origin between the heights
/// `lo` and `hi` along `axis` (unit), in bands at `heights`, `pieces`
/// round.
fn sphere_zone(axis: DVec3, radius: f64, heights: &[f64], pieces: usize) -> Solid {
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
    Solid::new(
        solid_of_revolution(axis, &stations, &surfaces, pieces, false),
        &TOL,
    )
    .unwrap()
}

#[test]
fn a_sphere_zone_has_its_extremes_inside_its_patches() {
    let axis = DVec3::new(0.3, 0.2, 1.0).normalize();
    let radius = 5.0;
    let heights = [-4.5, -1.0, 2.0, 4.0];
    let solid = sphere_zone(axis, radius, &heights, 8);
    let body = body_of(&solid);
    let tight = body.bounds.unwrap();
    // Across: the sphere's own extremes, at heights within the zone.
    for k in 0..2 {
        for (got, exact) in [(tight.max[k], radius), (-tight.min[k], radius)] {
            assert!((got - exact).abs() <= 1e-12 * radius, "{k}: {got} {exact}");
        }
    }
    // Along z: the rims of the caps.
    let rim = |h: f64, side: f64| {
        let rho = (radius * radius - h * h).sqrt();
        h * axis.z + side * rho * (1.0 - axis.z * axis.z).sqrt()
    };
    assert!((tight.max.z - rim(4.0, 1.0)).abs() <= 1e-12 * radius);
    assert!((tight.min.z - rim(-4.5, -1.0)).abs() <= 1e-12 * radius);
    let control = solid.bounds3().unwrap();
    assert!(control.max.x > tight.max.x + 0.1);

    // Its volume and its centroid along the axis.
    let cube = |h: f64| radius * radius * h - h * h * h / 3.0;
    let fourth = |h: f64| radius * radius * h * h / 2.0 - h * h * h * h / 4.0;
    let (lo, hi) = (heights[0], heights[3]);
    let volume = PI * (cube(hi) - cube(lo));
    let along = PI * (fourth(hi) - fourth(lo)) / volume;
    assert!(
        close(body.volume, volume, 1e-10),
        "{} {volume}",
        body.volume
    );
    assert!(close3(body.centre.unwrap(), axis * along, radius, 1e-10));
}

#[test]
fn a_frustum_has_its_centroid() {
    // A cone's frustum between radii 2 and 4 over a height of 3 (its
    // apex 3 below the bottom), exact cone strips.
    let axis = Axis::Z;
    let (h0, h1, t) = (3.0, 6.0, 2.0 / 3.0);
    let surface = Revolution::cone(axis, 0.0, t, true);
    let stations = [(surface.rho(h0), h0), (surface.rho(h1), h1)];
    let mesh = solid_of_revolution(axis, &stations, &[surface], 8, true);
    let solid = Solid::new(mesh, &TOL).unwrap();
    let (r0, r1, h) = (2.0, 4.0, 3.0);
    let volume = PI * h * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0;
    let above =
        h * (r0 * r0 + 2.0 * r0 * r1 + 3.0 * r1 * r1) / (4.0 * (r0 * r0 + r0 * r1 + r1 * r1));
    let moments = solid.moments(&Budget::DEFAULT).unwrap();
    assert!(close(moments.volume, volume, 1e-12));
    assert!(close3(
        moments.centre.unwrap(),
        DVec3::Z * (h0 + above),
        h1,
        1e-12
    ));
    let walls: Vec<FaceMeasure> = faces(&solid)
        .into_iter()
        .filter(|f| matches!(f.form, Form::Cone { .. }))
        .collect();
    assert_eq!(walls.len(), 1);
    let half_angle = walls[0].half_angle().unwrap();
    assert!(close(half_angle, t.atan(), 1e-15), "{half_angle}");
    let slant = (h * h + (r1 - r0) * (r1 - r0)).sqrt();
    assert!(close(walls[0].area, PI * (r0 + r1) * slant, 1e-12));
}

#[test]
fn angles_between_directions() {
    let solid = Solid::cuboid(DVec3::ZERO, DVec3::ONE, 1, &TOL).unwrap();
    let faces = faces(&solid);
    let normals: Vec<Direction> = faces.iter().map(|f| f.direction().unwrap()).collect();
    for a in &normals {
        for b in &normals {
            let expected = if a.v == b.v {
                0.0
            } else if a.v == -b.v {
                PI
            } else {
                PI / 2.0
            };
            assert_eq!(angle(*a, *b), expected);
        }
    }
    // Lines fold: an edge and its reverse are parallel, and a line meets
    // a plane's normal at most square.
    let line = Direction {
        v: DVec3::new(1.0, 1.0, 0.0).normalize(),
        line: true,
    };
    let back = Direction {
        v: -line.v,
        line: true,
    };
    assert_eq!(angle(line, back), 0.0);
    let normal = Direction {
        v: DVec3::NEG_X,
        line: false,
    };
    assert!(close(angle(line, normal), PI / 4.0, 1e-15));
    assert!(close(
        angle(
            Direction {
                line: false,
                ..back
            },
            normal
        ),
        PI / 4.0,
        1e-15
    ));
    assert!(close(
        angle(
            Direction {
                line: false,
                ..line
            },
            normal
        ),
        3.0 * PI / 4.0,
        1e-15
    ));
}

#[test]
fn measures_are_the_same_on_any_thread_count() {
    let axis = DVec3::new(-0.4, 0.7, 0.5).normalize();
    let sphere = sphere_zone(axis, 3.0, &[-2.5, 0.5, 2.8], 16);
    let cylinder = extruded(
        vec![Loop {
            segments: vec![
                Segment {
                    conic: Conic2::new(
                        DVec2::new(4.0, 0.0),
                        DVec2::new(4.0, 2.0),
                        0.5f64.sqrt(),
                        DVec2::new(0.0, 2.0),
                    )
                    .unwrap(),
                    curve: 0,
                },
                Segment::line(DVec2::new(0.0, 2.0), DVec2::new(4.0, 0.0), 1).unwrap(),
            ],
        }],
        &tilted(),
        3.0,
    );
    assert_deterministic(|| {
        [&sphere, &cylinder]
            .map(|solid| {
                let topology = solid.topology();
                let mut all = vec![measured(solid, &topology, Pick::Body)];
                for r in 0..topology.regions().len() as u32 {
                    all.push(measured(solid, &topology, Pick::Face(r)));
                }
                for c in 0..topology.chains().len() as u32 {
                    all.push(measured(solid, &topology, Pick::Edge(c)));
                }
                all
            })
            .to_vec()
    });
}

#[test]
fn past_the_budget_is_too_complex() {
    let solid = sphere_zone(DVec3::new(0.3, 0.2, 1.0).normalize(), 5.0, &[-4.5, 4.0], 8);
    let topology = solid.topology();
    let body = target(&solid, &topology, Pick::Body);
    assert_eq!(
        measure(&body, &TOL, &Budget::new(100)),
        Err(MeasureError::TooComplex)
    );
    assert_eq!(
        solid.moments(&Budget::new(100)),
        Err(KernelError::TooComplex)
    );
    assert_eq!(
        solid.tight_bounds(&TOL, &Budget::new(10)),
        Err(KernelError::TooComplex)
    );
    assert_eq!(
        MeasureError::TooComplex.to_string(),
        "too complex to measure"
    );
    // The box's search alone, after the integrals: a budget that covers
    // them runs out in it.
    let mut work = Work::new(&Budget::DEFAULT);
    solid.integrals(&mut work).unwrap();
    let integrals = Budget::DEFAULT.work() - work.left();
    let short = Budget::new(integrals + 20);
    assert_eq!(measure(&body, &TOL, &short), Err(MeasureError::TooComplex));
    assert!(measure(&body, &TOL, &Budget::new(integrals * 2 + 100_000)).is_ok());

    // A curve whose length needs more pieces than the budget allows.
    let quarter = Conic3::new(DVec3::X, DVec3::ONE, 0.1, DVec3::Y).unwrap();
    let mut work = Work::new(&Budget::new(2));
    assert_eq!(
        curve_length(&quarter, &mut work),
        Err(KernelError::TooComplex)
    );
}

#[test]
fn picks_naming_nothing_are_not_found() {
    let solid = Solid::cuboid(DVec3::ZERO, DVec3::ONE, 1, &TOL).unwrap();
    let topology = solid.topology();
    let missing = [
        (Pick::Face(6), NotFound::Face),
        (Pick::Edge(12), NotFound::Edge),
        (Pick::Corner(8), NotFound::Corner),
    ];
    for (pick, error) in missing {
        assert_eq!(
            measure(&target(&solid, &topology, pick), &TOL, &Budget::DEFAULT),
            Err(MeasureError::NotFound(error))
        );
    }
    // A larger solid's topology against a smaller solid.
    let big = Solid::cylinder(DVec3::ZERO, 1.0, 1.0, 1, &TOL).unwrap();
    let small = Solid::cuboid(DVec3::ZERO, DVec3::ONE, 1, &TOL).unwrap();
    let small_topology = small.topology();
    let big_topology = big.topology();
    // Still well-formed lookups: never a panic.
    for pick in [Pick::Face(0), Pick::Edge(0), Pick::Corner(0), Pick::Body] {
        let _ = measure(&target(&small, &big_topology, pick), &TOL, &Budget::DEFAULT);
        let _ = measure(&target(&big, &small_topology, pick), &TOL, &Budget::DEFAULT);
    }
}

#[test]
fn edge_extremes_are_the_roots_of_a_quadratic() {
    // Random conics: each root found turns the coordinate, and the
    // extremes over many samples are no further out than the box's.
    let mut rng = crate::test_rng::Rng::new(3);
    for _ in 0..200 {
        let p = [rng.point(5.0), rng.point(5.0), rng.point(5.0)];
        let w = rng.log_range(0.05, 20.0);
        let Ok(curve) = Conic3::new(p[0], p[1], w, p[2]) else {
            continue;
        };
        for k in 0..3 {
            let mut hi = curve.p0[k].max(curve.p1[k]);
            let mut lo = curve.p0[k].min(curve.p1[k]);
            for t in turns(&curve, k) {
                let (_, d) = curve.eval_deriv(t);
                let scale = (curve.p1 - curve.p0).length() + (curve.c - curve.p0).length();
                assert!(d[k].abs() <= 1e-9 * scale * w.max(1.0 / w), "{d} at {t}");
                let x = curve.eval(t)[k];
                hi = hi.max(x);
                lo = lo.min(x);
            }
            for i in 0..=1000 {
                let x = curve.eval(f64::from(i) / 1000.0)[k];
                assert!(x <= hi + 1e-12 * (1.0 + hi.abs()) && x >= lo - 1e-12 * (1.0 + lo.abs()));
            }
        }
    }
}

#[test]
fn conic_lengths_match_a_fine_reference() {
    let mut work = Work::new(&Budget::DEFAULT);
    // The parabola y = x² over [−1, 1].
    let parabola = Conic3::new(
        DVec3::new(-1.0, 1.0, 0.0),
        DVec3::new(0.0, -1.0, 0.0),
        1.0,
        DVec3::new(1.0, 1.0, 0.0),
    )
    .unwrap();
    let exact = 5f64.sqrt() + 2f64.asinh() / 2.0;
    let length = curve_length(&parabola, &mut work).unwrap();
    assert!(close(length, exact, 1e-14), "{length} {exact}");

    // A straight curve with its control point off the middle runs along
    // its chord.
    let straight = Conic3::new(
        DVec3::ZERO,
        DVec3::new(0.6, 0.8, 0.0),
        3.0,
        DVec3::new(3.0, 4.0, 0.0),
    )
    .unwrap();
    assert_eq!(curve_length(&straight, &mut work), Ok(5.0));

    // Random conics, every kind and weight, against an adaptive rule.
    let mut rng = crate::test_rng::Rng::new(11);
    for _ in 0..300 {
        let p = [rng.point(10.0), rng.point(10.0), rng.point(10.0)];
        let w = rng.log_range(1.0 / 64.0, 64.0);
        let Ok(curve) = Conic3::new(p[0], p[1], w, p[2]) else {
            continue;
        };
        let reference = reference_length(&curve);
        let length = curve_length(&curve, &mut work).unwrap();
        assert!(
            close(length, reference, 1e-13),
            "{curve:?}: {length} {reference}"
        );
    }
}

/// The length of `curve` by 8-point Gauss–Legendre over pieces of its
/// parameter, halved until halving changes the sum by less than
/// `1e-14` of it.
fn reference_length(curve: &Conic3) -> f64 {
    fn piece(curve: &Conic3, a: f64, b: f64, depth: u32) -> f64 {
        let rule = |a: f64, b: f64| {
            GAUSS8
                .iter()
                .map(|&(t, w)| w * curve.eval_deriv(a + (b - a) * t).1.length())
                .sum::<f64>()
                * (b - a)
        };
        let m = (a + b) / 2.0;
        let (whole, halves) = (rule(a, b), rule(a, m) + rule(m, b));
        if depth > 20 || (whole - halves).abs() <= 1e-14 * halves.abs() {
            halves
        } else {
            piece(curve, a, m, depth + 1) + piece(curve, m, b, depth + 1)
        }
    }
    piece(curve, 0.0, 1.0, 0)
}

/// `profile` turned fully about `frame`'s `y`, at [`TOL`].
fn turned(loops: Vec<Loop>, frame: &Frame) -> Solid {
    crate::revolve(
        &Profile { loops },
        frame,
        crate::Sweep::Full,
        1,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap()
}

/// The frames the revolve-based tests turn about: the `z` axis through
/// the origin, and a tilted one off it.
fn axes() -> [Frame; 2] {
    let tilted = tilted();
    [
        Frame {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::Z,
        },
        Frame {
            origin: tilted.origin,
            x: tilted.x,
            y: tilted.x.cross(tilted.y),
        },
    ]
}

/// Checks a revolved body's volume, area and centre against the exact
/// ones: the solid is within half the fit tolerance of the shape it
/// stands for, so its volume within that times the area, its area within
/// that times the area over `radius` (the least radius of curvature of
/// its fitted faces), and its centre within twice that shell's moment
/// (half the fit times the area times `size`) over the volume.
fn assert_turned(
    what: &str,
    body: &BodyMeasure,
    (volume, area, centre): (f64, f64, DVec3),
    radius: f64,
    size: f64,
) {
    let half = TOL.fit() / 2.0;
    let got = body.centre.unwrap();
    assert!(
        (body.volume - volume).abs() <= half * area,
        "{what}: volume {} for {volume}",
        body.volume
    );
    assert!(
        (body.area - area).abs() <= 4.0 * half * area / radius,
        "{what}: area {} for {area}",
        body.area
    );
    assert!(
        got.distance(centre) <= 2.0 * half * area * size / volume,
        "{what}: centre {got} for {centre}"
    );
}

#[test]
fn revolved_cone_and_hemisphere_have_their_centroids() {
    use crate::profile::tests::{arc, polygon};
    let v = DVec2::new;
    for frame in axes() {
        // A cone of radius 4 and height 3: its centroid a quarter up. Its
        // wall is exact but for the cap round its apex.
        let (r, h) = (4.0, 3.0);
        let cone = turned(
            vec![polygon(&[v(0.0, 0.0), v(r, 0.0), v(0.0, h)], 1)],
            &frame,
        );
        let slant = (r * r + h * h).sqrt();
        let exact = (
            PI * r * r * h / 3.0,
            PI * r * (r + slant),
            frame.origin + frame.y * (h / 4.0),
        );
        let body = body_of(&cone);
        assert_turned("cone", &body, exact, 1.0, r);
        // Exact to far better than the fit: only the apex's cap is fitted.
        assert!(close(body.volume, exact.0, 1e-8), "{}", body.volume);
        let wall = faces(&cone)
            .into_iter()
            .find(|f| matches!(f.form, Form::Cone { .. }))
            .unwrap();
        assert!(close(wall.half_angle().unwrap(), (r / h).atan(), 1e-15));
        assert!(close(wall.area, PI * r * slant, 1e-6), "{}", wall.area);
        let axis = wall.direction().unwrap();
        assert!(axis.line && close(axis.v.dot(frame.y).abs(), 1.0, 1e-15));

        // A hemisphere of radius 5: its centroid 3/8 of the radius up.
        let radius = 5.0;
        let hemisphere = turned(
            vec![Loop {
                segments: vec![
                    Segment::line(v(0.0, 0.0), v(radius, 0.0), 1).unwrap(),
                    arc(v(0.0, 0.0), v(radius, 0.0), v(0.0, radius), 2),
                    Segment::line(v(0.0, radius), v(0.0, 0.0), 3).unwrap(),
                ],
            }],
            &frame,
        );
        let exact = (
            2.0 * PI * radius.powi(3) / 3.0,
            3.0 * PI * radius * radius,
            frame.origin + frame.y * (3.0 * radius / 8.0),
        );
        let body = body_of(&hemisphere);
        assert_turned("hemisphere", &body, exact, radius, radius);
        // Its box: along a direction on the dome's side, the sphere's
        // extreme; on the base's side, the rim's. Within the pole cap's
        // fit.
        let tight = body.bounds.unwrap();
        for k in 0..3 {
            let rim = radius * (1.0 - frame.y[k] * frame.y[k]).sqrt();
            let reach = |up: bool| if up { radius } else { rim };
            let (lo, hi) = (
                frame.origin[k] - reach(frame.y[k] <= 0.0),
                frame.origin[k] + reach(frame.y[k] >= 0.0),
            );
            assert!((tight.min[k] - lo).abs() <= TOL.fit(), "{k}: {tight:?}");
            assert!((tight.max[k] - hi).abs() <= TOL.fit(), "{k}: {tight:?}");
        }
        let dome = faces(&hemisphere)
            .into_iter()
            .find(|f| matches!(f.form, Form::Sphere { .. }))
            .unwrap();
        assert!(matches!(dome.form, Form::Sphere { radius: r, .. } if close(r, radius, 1e-15)));
        assert!(close(dome.area, 2.0 * PI * radius * radius, 1e-6));
    }
}

#[test]
fn a_revolved_torus_has_its_area_volume_and_box() {
    use crate::profile::tests::circle;
    let (big, small) = (10.0, 2.0);
    for frame in axes() {
        let torus = turned(vec![circle(DVec2::new(big, 0.0), small, 1, false)], &frame);
        let body = body_of(&torus);
        let exact = (
            2.0 * PI * PI * big * small * small,
            4.0 * PI * PI * big * small,
            frame.origin,
        );
        assert_turned("torus", &body, exact, small, big + small);
        // Its box: the middle circle's extremes and the tube's radius,
        // within the fit (the fitted patches' insides searched).
        let tight = body.bounds.unwrap();
        let control = torus.bounds3().unwrap();
        for k in 0..3 {
            let reach = big * (1.0 - frame.y[k] * frame.y[k]).sqrt() + small;
            let (lo, hi) = (frame.origin[k] - reach, frame.origin[k] + reach);
            assert!((tight.min[k] - lo).abs() <= TOL.fit(), "{k}: {tight:?}");
            assert!((tight.max[k] - hi).abs() <= TOL.fit(), "{k}: {tight:?}");
            assert!(control.min[k] <= tight.min[k] && tight.max[k] <= control.max[k]);
        }
        let tube = faces(&torus);
        assert_eq!(tube.len(), 1);
        assert!(matches!(
            tube[0].form,
            Form::Torus { major, minor, .. } if close(major, big, 1e-15) && close(minor, small, 1e-15)
        ));
        assert!(close(tube[0].area, body.area, 1e-15));
    }
}
