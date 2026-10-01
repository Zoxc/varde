#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use super::*;
use crate::test_rng::Rng;

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

#[test]
fn circles_are_told_from_other_conics() {
    let mut rng = Rng::new(3);
    for _ in 0..500 {
        let centre = DVec2::new(rng.range(-1e3, 1e3), rng.range(-1e3, 1e3));
        let r = rng.log_range(0.1, 1e5);
        let start = rng.range(-TAU, TAU);
        let sweep = rng.log_range(1e-2, FRAC_PI_2) * if rng.unit() < 0.5 { 1.0 } else { -1.0 };
        let arc = Conic2::arc(centre, r, start, sweep).unwrap();
        let (c, radius) = circle_of(&arc).unwrap();
        let scale = r + centre.length();
        assert!((c - centre).length() <= 1e-6 * scale, "{c} {centre}");
        assert!((radius - r).abs() <= 1e-6 * scale);
        // Its halves too, and the same arc from its ends.
        for half in arc.split_half().unwrap() {
            assert!(circle_of(&half).is_some());
        }
        let between = Conic2::arc_between(centre, r, arc.p0, arc.p1).unwrap();
        assert!(circle_of(&between).is_some());
        // An ellipse arc, or the weight changed a little: not a circle.
        let squashed = Conic2 {
            c: arc.c + (arc.c - (arc.p0 + arc.p1) * 0.5) * 1e-3,
            ..arc
        };
        assert_eq!(circle_of(&squashed), None);
        let heavier = Conic2 {
            w: arc.w * (1.0 + 1e-6),
            ..arc
        };
        assert_eq!(circle_of(&heavier), None);
    }
    let line = Conic2::line(DVec2::ZERO, DVec2::X).unwrap();
    assert_eq!(circle_of(&line), None);
    // Small arcs far out, whose coordinates' rounding is a good part of
    // their size.
    for (r, sweep) in [(1e-2, 0.1), (1e-3, 0.5), (1e-4, 1.0)] {
        let centre = DVec2::new(9e5, -7e5);
        let arc = Conic2::arc(centre, r, 0.3, sweep).unwrap();
        let (c, radius) = circle_of(&arc).unwrap();
        assert!((c - centre).length() < 1e-6 && (radius - r).abs() < 1e-6);
    }
}

#[test]
fn forms_moved_rigidly_measure_the_same() {
    let mut rng = Rng::new(4);
    let forms = [
        Form::plane(DVec3::new(1.0, 2.0, 2.0), 3.0),
        Form::Cylinder {
            point: DVec3::new(1.0, 2.0, 3.0),
            axis: DVec3::Z,
            radius: 2.0,
        },
        Form::Cone {
            apex: DVec3::ONE,
            axis: DVec3::new(0.6, 0.8, 0.0),
            cos: 0.8,
            sin: 0.6,
        },
        Form::Sphere {
            centre: DVec3::X,
            radius: 3.0,
        },
        Form::Torus {
            centre: DVec3::Y,
            axis: DVec3::X,
            major: 4.0,
            minor: 1.0,
        },
    ];
    for _ in 0..50 {
        let q = glam::DQuat::from_axis_angle(rng.direction(), rng.range(0.0, TAU));
        let shift = rng.point(100.0);
        let mirror = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
        let f = |p: DVec3| q * DVec3::new(p.x * mirror, p.y, p.z) + shift;
        for form in forms {
            let moved = form.moved(f);
            for _ in 0..10 {
                let p = rng.point(10.0);
                let (a, b) = (form.distance(p), moved.distance(f(p)));
                assert!((a - b).abs() <= 1e-12 * 200.0, "{form:?}: {a} {b}");
            }
        }
    }
}

mod constructed {
    use glam::{DVec2, DVec3};

    use crate::mesh::{FacePart, Form, Surface};
    use crate::patch::Conic2;
    use crate::profile::tests::circle;
    use crate::profile::{Loop, Profile, Segment};
    use crate::{Budget, Frame, Op, Solid, Tolerance, boolean, extrude};

    const TOL: Tolerance = Tolerance::DEFAULT;

    fn form(solid: &Solid, part: FacePart) -> Form {
        let faces = solid.mesh().faces();
        faces.iter().find(|f| f.name.part == part).unwrap().form
    }

    #[test]
    fn extrudes_give_their_faces_forms() {
        // A D of a line and an ellipse arc, with a round hole, extruded
        // from 1 to 3: planes facing out, a cylinder over the circle, a
        // conic cylinder over the ellipse.
        let d = Loop {
            segments: vec![
                Segment::line(DVec2::ZERO, DVec2::new(8.0, 0.0), 0).unwrap(),
                Segment {
                    conic: Conic2::new(
                        DVec2::new(8.0, 0.0),
                        DVec2::new(4.0, 6.0),
                        0.6,
                        DVec2::ZERO,
                    )
                    .unwrap(),
                    curve: 1,
                },
            ],
        };
        let hole = circle(DVec2::new(4.0, 1.0), 0.5, 2, true);
        let profile = Profile {
            loops: vec![d, hole],
        };
        let solid = extrude(&profile, &Frame::XY, 1.0, 3.0, 9, &TOL, &Budget::DEFAULT).unwrap();
        let side = |curve, segment| FacePart::Side { curve, segment };
        assert_eq!(
            form(&solid, FacePart::StartCap),
            Form::Plane {
                n: DVec3::NEG_Z,
                d: -1.0
            }
        );
        assert_eq!(
            form(&solid, FacePart::EndCap),
            Form::Plane {
                n: DVec3::Z,
                d: 3.0
            }
        );
        assert_eq!(
            form(&solid, side(0, 0)),
            Form::Plane {
                n: DVec3::NEG_Y,
                d: 0.0
            }
        );
        let Form::ConicCylinder { conic, along } = form(&solid, side(1, 0)) else {
            panic!("{:?}", form(&solid, side(1, 0)));
        };
        assert_eq!(along, DVec3::Z);
        assert_eq!(conic.w, 0.6);
        for segment in 0..4 {
            let Form::Cylinder {
                point,
                axis,
                radius,
            } = form(&solid, side(2, segment))
            else {
                panic!("{:?}", form(&solid, side(2, segment)));
            };
            assert!(
                (point - DVec3::new(4.0, 1.0, 1.0)).length() < 1e-12,
                "{point}"
            );
            assert_eq!(axis, DVec3::Z);
            assert!((radius - 0.5).abs() < 1e-12);
        }
        // Tilted, the debug check of `Solid::new` holds them too.
        let x = DVec3::new(2.0, 1.0, 2.0) / 3.0;
        let y = DVec3::new(-1.0, 2.0, 0.0).normalize();
        let frame = Frame {
            origin: DVec3::new(30.0, -20.0, 5.0),
            x,
            y,
        };
        let tilted = extrude(&profile, &frame, -2.0, 2.0, 9, &TOL, &Budget::DEFAULT).unwrap();
        let Form::Plane { n, .. } = form(&tilted, FacePart::EndCap) else {
            panic!()
        };
        assert!((n - frame.normal()).length() < 1e-12);
    }

    #[test]
    fn boxes_and_cylinders_give_their_faces_forms() {
        let cylinder = Solid::cylinder(DVec3::new(1.0, 2.0, 3.0), 2.0, 5.0, 4, &TOL).unwrap();
        for face in cylinder.mesh().faces() {
            let want = match face.name.part {
                FacePart::StartCap => Form::plane(DVec3::NEG_Z, -3.0),
                FacePart::EndCap => Form::plane(DVec3::Z, 8.0),
                _ => Form::Cylinder {
                    point: DVec3::new(1.0, 2.0, 3.0),
                    axis: DVec3::Z,
                    radius: 2.0,
                },
            };
            assert_eq!(face.form, want);
        }
        let cube = Solid::cuboid(DVec3::ZERO, DVec3::splat(2.0), 4, &TOL).unwrap();
        for face in cube.mesh().faces() {
            let Surface::Plane { n, d } = face.surface else {
                panic!()
            };
            assert_eq!(face.form, Form::plane(n, d));
        }
    }

    #[test]
    fn booleans_keep_forms_and_a_difference_turns_the_tools_round() {
        let plate = Solid::cuboid(DVec3::ZERO, DVec3::new(10.0, 10.0, 2.0), 1, &TOL).unwrap();
        let pin = Solid::cylinder(DVec3::new(5.0, 5.0, -1.0), 2.0, 4.0, 2, &TOL).unwrap();
        let slab = Solid::cuboid(
            DVec3::new(-1.0, -1.0, 1.5),
            DVec3::new(3.0, 12.0, 1.0),
            3,
            &TOL,
        )
        .unwrap();
        let run = |a: &Solid, b: &Solid, op| boolean(a, b, op, &TOL, &Budget::DEFAULT).unwrap();
        let operands = [&plate, &pin, &slab];
        let source = |feature: u64, part: FacePart| {
            operands
                .iter()
                .flat_map(|s| s.mesh().faces())
                .find(|f| f.name.feature == feature && f.name.part == part)
                .unwrap()
                .form
        };
        // A hole and a step: every face keeps its form, the tools' turned
        // round (only planes say which way they face).
        let drilled = run(&plate, &pin, Op::Difference);
        let stepped = run(&drilled, &slab, Op::Difference);
        let mut tools = 0;
        for face in stepped.mesh().faces() {
            let was = source(face.name.feature, face.name.part);
            if face.name.feature == 1 {
                assert_eq!(face.form, was);
            } else {
                tools += 1;
                assert_eq!(face.form, was.flipped());
                if let Form::Plane { n, .. } = was {
                    assert_ne!(face.form, was, "{n}");
                }
            }
        }
        assert!(tools > 0);
        let joined = run(&plate, &slab, Op::Union);
        for face in joined.mesh().faces() {
            assert_eq!(face.form, source(face.name.feature, face.name.part));
        }
    }

    /// The cylinder of radius 2 with its faces' forms changed by `f`.
    fn reformed(f: impl Fn(Form) -> Form) -> crate::mesh::Mesh {
        let mesh = Solid::cylinder(DVec3::ZERO, 2.0, 3.0, 1, &TOL)
            .unwrap()
            .into_mesh();
        let faces = mesh
            .faces()
            .iter()
            .map(|&face| crate::mesh::Face {
                form: f(face.form),
                ..face
            })
            .collect();
        crate::mesh::Mesh::from_parts(
            mesh.verts().to_vec(),
            mesh.edges().to_vec(),
            mesh.tris().to_vec(),
            faces,
        )
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "strays from its face's form")]
    fn a_form_the_patches_are_off_is_a_bug() {
        let wider = |form| match form {
            Form::Cylinder { point, axis, .. } => Form::Cylinder {
                point,
                axis,
                radius: 2.0 + 2.0 * TOL.fit(),
            },
            form => form,
        };
        let _ = Solid::new(reformed(wider), &TOL);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "faces against its face's plane form")]
    fn a_plane_form_facing_in_is_a_bug() {
        let _ = Solid::new(reformed(Form::flipped), &TOL);
    }

    #[test]
    fn forms_within_the_fit_pass() {
        let wider = |form| match form {
            Form::Cylinder { point, axis, .. } => Form::Cylinder {
                point,
                axis,
                radius: 2.0 + 0.5 * TOL.fit(),
            },
            form => form,
        };
        assert!(Solid::new(reformed(wider), &TOL).is_ok());
    }
}
