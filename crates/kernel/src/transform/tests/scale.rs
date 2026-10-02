//! Scales, and the cones and spheres the rigid motions' tests leave out.

use std::f64::consts::PI;

use glam::{DVec2, DVec3};

use super::{TOL, close, forms_face_out, part, transformed};
use crate::extrude::Frame;
use crate::mesh::{Face, Form, Mesh, Surface, samples};
use crate::par::assert_deterministic;
use crate::patch::PatchError;
use crate::profile::tests::{arc, circle, polygon, rect};
use crate::profile::{Loop, Profile, Segment};
use crate::transform::{MAX_SCALE, Motion};
use crate::{Budget, KernelError, Solid, Sweep, revolve};

/// The frame with its axis along `z` through `origin`, the profile in a
/// plane through it along `x`.
fn about_z(origin: DVec3) -> Frame {
    Frame {
        origin,
        x: DVec3::X,
        y: DVec3::Z,
    }
}

fn revolved(loops: Vec<Loop>, origin: DVec3) -> Solid {
    revolve(
        &Profile { loops },
        &about_z(origin),
        Sweep::Full,
        7,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap()
}

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// A cylinder of radius 3 and height 5 standing on `origin`.
fn cylinder(origin: DVec3) -> Solid {
    revolved(vec![rect(v(0.0, 0.0), v(3.0, 5.0), 1)], origin)
}

/// A cone of base radius 4 and height 3 standing on `origin`.
fn cone(origin: DVec3) -> Solid {
    revolved(
        vec![polygon(&[v(0.0, 0.0), v(4.0, 0.0), v(0.0, 3.0)], 1)],
        origin,
    )
}

/// A ball of radius 2 around `origin`.
fn sphere(origin: DVec3) -> Solid {
    let ball = Loop {
        segments: vec![
            arc(v(0.0, 0.0), v(0.0, -2.0), v(2.0, 0.0), 1),
            arc(v(0.0, 0.0), v(2.0, 0.0), v(0.0, 2.0), 1),
            Segment::line(v(0.0, 2.0), v(0.0, -2.0), 2).unwrap(),
        ],
    };
    revolved(vec![ball], origin)
}

/// A torus of radii 10 and 2 around `origin`: fitted all over.
fn torus(origin: DVec3) -> Solid {
    revolved(vec![circle(v(10.0, 0.0), 2.0, 1, false)], origin)
}

fn kinds(solid: &Solid) -> Vec<&'static str> {
    let mut kinds: Vec<_> = solid
        .mesh()
        .faces()
        .iter()
        .map(|f| match f.form {
            Form::Unknown => "unknown",
            Form::Plane { .. } => "plane",
            Form::Cylinder { .. } => "cylinder",
            Form::ConicCylinder { .. } => "conic cylinder",
            Form::Cone { .. } => "cone",
            Form::Sphere { .. } => "sphere",
            Form::Torus { .. } => "torus",
            Form::Revolved { .. } => "revolved",
            Form::Quadric(_) => "quadric",
        })
        .collect();
    kinds.sort_unstable();
    kinds.dedup();
    kinds
}

/// Scales `solid` by `factors` about `centre` and checks the result: its
/// volume `sx·sy·sz` times, its tags, its planes facing out (the debug
/// check of the forms ran in `transformed`). Gives it.
fn scaled(solid: &Solid, centre: DVec3, factors: DVec3) -> Solid {
    let motion = Motion::scale(centre, factors).unwrap();
    let image = transformed(solid, &motion, None);
    let want = solid.volume() * factors.x * factors.y * factors.z;
    assert!(
        close(image.volume(), want, 1e-12),
        "{factors}: {} for {want}",
        image.volume()
    );
    image.mesh().check_faces(&TOL).unwrap();
    forms_face_out(&image);
    for (a, b) in solid.mesh().faces().iter().zip(image.mesh().faces()) {
        assert_eq!(a.name, b.name);
        let slack = if motion.stretch() > 1.0 {
            a.slack * motion.stretch()
        } else {
            a.slack
        };
        assert_eq!(b.slack, slack);
    }
    image
}

#[test]
fn boxes_cylinders_cones_and_spheres_scale_uniformly() {
    let centre = DVec3::new(1.0, -2.0, 0.5);
    let cube = Solid::cuboid(
        DVec3::new(-1.0, 0.0, 2.0),
        DVec3::new(2.0, 3.0, 4.0),
        1,
        &TOL,
    )
    .unwrap();
    for f in [2.5, 0.3, 1.0 / 3.0, 40.0] {
        let factors = DVec3::splat(f);
        let image = scaled(&cube, centre, factors);
        assert_eq!(kinds(&image), ["plane"]);
        let image = scaled(&cylinder(DVec3::ZERO), centre, factors);
        assert_eq!(kinds(&image), ["cylinder", "plane"]);
        for face in image.mesh().faces() {
            if let Form::Cylinder {
                point,
                axis,
                radius,
            } = face.form
            {
                assert!(close(radius, 3.0 * f, 1e-15));
                assert_eq!(axis, DVec3::Z);
                assert!((point - (centre - centre * f)).length() < 1e-13 * f.max(1.0));
            }
        }
        let image = scaled(&cone(DVec3::ZERO), centre, factors);
        assert_eq!(kinds(&image), ["cone", "plane"]);
        for face in image.mesh().faces() {
            if let Form::Cone { apex, cos, sin, .. } = face.form {
                // The half-angle stays: tan = 4/3.
                assert!(close(sin / cos, 4.0 / 3.0, 1e-14));
                let want = centre + (DVec3::new(0.0, 0.0, 3.0) - centre) * f;
                assert!((apex - want).length() < 1e-13 * f.max(1.0));
            }
        }
        let image = scaled(&sphere(DVec3::ZERO), centre, factors);
        assert_eq!(kinds(&image), ["sphere"]);
        for face in image.mesh().faces() {
            let Form::Sphere { radius, .. } = face.form else {
                unreachable!()
            };
            assert!(close(radius, 2.0 * f, 1e-15));
        }
    }
}

#[test]
fn boxes_cylinders_cones_and_spheres_scale_per_axis() {
    let centre = DVec3::new(-0.5, 1.0, 2.0);
    let cube = Solid::cuboid(
        DVec3::new(-1.0, 0.0, 2.0),
        DVec3::new(2.0, 3.0, 4.0),
        1,
        &TOL,
    )
    .unwrap();
    for factors in [
        DVec3::new(2.0, 0.5, 3.0),
        DVec3::new(1.7, 0.31, 1.0),
        DVec3::new(0.25, 6.5, 1.1),
        DVec3::new(1.0, 1.0, 3.0),
    ] {
        let along_z = factors.x == factors.y;
        scaled(&cube, centre, factors);
        // A cylinder along z: circular while x and y scale alike, an
        // elliptic cylinder otherwise.
        let image = scaled(&cylinder(DVec3::ZERO), centre, factors);
        if along_z {
            assert_eq!(kinds(&image), ["cylinder", "plane"]);
        } else {
            assert_eq!(kinds(&image), ["conic cylinder", "plane"]);
        }
        let image = scaled(&cone(DVec3::ZERO), centre, factors);
        if along_z {
            assert_eq!(kinds(&image), ["cone", "plane"]);
            for face in image.mesh().faces() {
                if let Form::Cone { cos, sin, .. } = face.form {
                    let want = 4.0 * factors.x / (3.0 * factors.z);
                    assert!(close(sin / cos, want, 1e-14), "{} {want}", sin / cos);
                }
            }
        } else {
            assert_eq!(kinds(&image), ["plane", "quadric"]);
        }
        let image = scaled(&sphere(DVec3::ZERO), centre, factors);
        assert_eq!(kinds(&image), ["quadric"]);
    }
}

#[test]
fn cylinders_off_the_axes_stay_cylinders_over_conics() {
    // A cylinder along (1, 1, 1) scaled per axis, then turned: an
    // elliptic cylinder, its tags exact.
    let solid = transformed(
        &cylinder(DVec3::new(1.0, 2.0, 3.0)),
        &Motion::turn(DVec3::ZERO, DVec3::new(1.0, -1.0, 0.0), 54.7356).unwrap(),
        None,
    );
    let image = scaled(&solid, DVec3::ZERO, DVec3::new(1.5, 0.75, 2.0));
    assert_eq!(kinds(&image), ["conic cylinder", "plane"]);
    let turn = Motion::turn(DVec3::X, DVec3::new(0.3, 0.1, 1.0), 33.0).unwrap();
    let scale = Motion::scale(DVec3::ZERO, DVec3::new(1.5, 0.75, 2.0)).unwrap();
    let both = scale.then(&turn);
    assert_eq!(both.stretch(), 2.0);
    let image = transformed(&solid, &both, None);
    assert!(close(image.volume(), solid.volume() * 2.25, 1e-12));
    image.mesh().check_faces(&TOL).unwrap();
    assert_eq!(kinds(&image), ["conic cylinder", "plane"]);
    // Scaled across its axis alike after all: a cylinder.
    let flat = transformed(
        &cylinder(DVec3::ZERO),
        &Motion::turn(DVec3::ZERO, DVec3::X, 90.0).unwrap(),
        None,
    );
    let image = scaled(&flat, DVec3::ZERO, DVec3::new(2.0, 0.5, 2.0));
    assert_eq!(kinds(&image), ["cylinder", "plane"]);
}

#[test]
fn a_sphere_scaled_per_axis_is_an_exact_ellipsoid() {
    let centre = DVec3::new(3.0, -1.0, 2.0);
    let ball = sphere(centre);
    for (about, factors) in [
        (DVec3::ZERO, DVec3::new(2.0, 3.0, 0.5)),
        (DVec3::new(1.0, 1.0, 1.0), DVec3::new(1.3, 0.7, 2.9)),
    ] {
        let image = scaled(&ball, about, factors);
        let middle = about + (centre - about) * factors;
        let semi = factors * 2.0;
        let mesh = image.mesh();
        let mut exact = 0;
        for t in 0..mesh.tris().len() {
            let face = &mesh.faces()[mesh.tris()[t].face as usize];
            let Form::Quadric(q) = face.form else {
                panic!("{:?}", face.form)
            };
            let patch = mesh.patch(t);
            for u in samples() {
                let x = patch.eval(u);
                let off = ((x - middle) / semi).length_squared() - 1.0;
                let d = q.distance(x);
                match face.surface {
                    // The strips: on the ellipsoid to 1e-12.
                    Surface::Quadric(_) => {
                        assert!(off.abs() < 1e-12, "{off:e}");
                        assert!(d < 1e-12 * semi.max_element(), "{d:e}");
                    }
                    // The caps at the poles, fitted: within the fit
                    // stretched.
                    _ => assert!(d <= TOL.fit() * face.slack, "{d:e}"),
                }
            }
            exact += usize::from(matches!(face.surface, Surface::Quadric(_)));
        }
        assert!(exact > mesh.tris().len() / 2);
    }
}

#[test]
fn powers_of_two_are_exact_to_the_bit() {
    let (solid, volume) = part();
    let factors = DVec3::new(2.0, 0.5, 4.0);
    let inverse = factors.recip();
    let motion = Motion::scale(DVec3::ZERO, factors).unwrap();
    let image = transformed(&solid, &motion, None);
    assert!(close(image.volume(), volume * 4.0, 1e-12));
    let (a, b) = (solid.mesh(), image.mesh());
    for (p, q) in a.verts().iter().zip(b.verts()) {
        assert_eq!(*p * factors, *q);
    }
    for (e, f) in a.edges().iter().zip(b.edges()) {
        assert_eq!(e.ctrl * factors, f.ctrl);
        assert_eq!(e.weight, f.weight);
    }
    for (f, g) in a.faces().iter().zip(b.faces()) {
        match (f.surface, g.surface) {
            (Surface::Plane { n, d }, Surface::Plane { n: m, d: e }) => {
                assert_eq!((n * inverse, d), (m, e));
            }
            (Surface::Quadric(p), Surface::Quadric(q)) => {
                let s = glam::DMat3::from_diagonal(inverse);
                assert_eq!(s * p.a * s, q.a);
                assert_eq!(p.b * inverse, q.b);
                assert_eq!(p.origin * factors, q.origin);
                assert_eq!(p.c, q.c);
            }
            (Surface::Free, Surface::Free) => {}
            other => panic!("{other:?}"),
        }
    }
    // And back: the same points and tags.
    let back = transformed(&image, &Motion::scale(DVec3::ZERO, inverse).unwrap(), None);
    assert_eq!(back.mesh().verts(), a.verts());
    assert_eq!(back.mesh().edges(), a.edges());
    assert_eq!(back.mesh().tris(), a.tris());
    for (f, g) in a.faces().iter().zip(back.mesh().faces()) {
        assert_eq!(f.surface, g.surface);
    }
    // A uniform doubling doubles radii exactly.
    let twice = transformed(
        &cylinder(DVec3::ZERO),
        &Motion::scale(DVec3::ZERO, DVec3::splat(2.0)).unwrap(),
        None,
    );
    for face in twice.mesh().faces() {
        if let Form::Cylinder { radius, .. } = face.form {
            assert_eq!(radius, 6.0);
        }
    }
}

#[test]
fn a_torus_scaled_up_carries_its_slack() {
    let ring = torus(DVec3::new(1.0, 2.0, 3.0));
    let image = scaled(&ring, DVec3::ZERO, DVec3::splat(25.4));
    // Within its fit of the analytic volume, scaled: the area times
    // half the fit tolerance.
    let cube = 25.4 * 25.4 * 25.4;
    let (volume, area) = (2.0 * PI * PI * 10.0 * 4.0, 4.0 * PI * PI * 20.0);
    assert!((image.volume() - volume * cube).abs() <= area * TOL.fit() / 2.0 * cube);
    for face in image.mesh().faces() {
        assert_eq!(face.slack, 25.4);
        let Form::Torus { major, minor, .. } = face.form else {
            panic!("{:?}", face.form)
        };
        assert!(close(major, 254.0, 1e-15) && close(minor, 50.8, 1e-15));
    }
    // The debug check passed with the slack (in `transformed`); without
    // it, the stretched fitted patches stray past the fit tolerance.
    #[cfg(debug_assertions)]
    {
        let mesh = image.mesh();
        let patches: Vec<_> = (0..mesh.tris().len()).map(|t| mesh.patch(t)).collect();
        assert_eq!(mesh.off_forms(&patches, &TOL), None);
        let faces = mesh
            .faces()
            .iter()
            .map(|&f| Face { slack: 1.0, ..f })
            .collect();
        let unslack = Mesh::from_parts(
            mesh.verts().to_vec(),
            mesh.edges().to_vec(),
            mesh.tris().to_vec(),
            faces,
        );
        assert!(unslack.off_forms(&patches, &TOL).is_some());
    }
    // Twice scaled up, the slacks multiply; scaled down, they stay.
    let again = scaled(&image, DVec3::ZERO, DVec3::splat(2.0));
    assert!(again.mesh().faces().iter().all(|f| f.slack == 50.8));
    let down = scaled(&again, DVec3::ZERO, DVec3::splat(0.5));
    assert!(down.mesh().faces().iter().all(|f| f.slack == 50.8));
    // Per axis, a torus is no form there is; the slack still grows.
    let image = scaled(&ring, DVec3::ZERO, DVec3::new(1.0, 1.0, 3.0));
    assert_eq!(kinds(&image), ["unknown"]);
    assert!(image.mesh().faces().iter().all(|f| f.slack == 3.0));
}

#[test]
fn a_scale_down_under_the_resolution_is_refused() {
    // A plate 0.01 thick, scaled to a ten-millionth across.
    let plate = Solid::cuboid(DVec3::ZERO, DVec3::new(1.0, 1.0, 0.01), 1, &TOL).unwrap();
    let thin = Motion::scale(DVec3::ZERO, DVec3::new(1.0, 1.0, 1e-5)).unwrap();
    assert!(matches!(
        plate.transformed(&thin, None, &TOL, &Budget::DEFAULT),
        Err(KernelError::Invalid(_))
    ));
    // A part shrunk a millionfold is under it everywhere.
    let (solid, _) = part();
    let tiny = Motion::scale(DVec3::ZERO, DVec3::splat(1.0 / MAX_SCALE)).unwrap();
    assert!(matches!(
        solid.transformed(&tiny, None, &TOL, &Budget::DEFAULT),
        Err(KernelError::Invalid(_))
    ));
    // Kept above it, it passes.
    let small = Motion::scale(DVec3::ZERO, DVec3::new(1.0, 1.0, 0.01)).unwrap();
    assert!(
        plate
            .transformed(&small, None, &TOL, &Budget::DEFAULT)
            .is_ok()
    );
}

#[test]
fn scale_factors_and_bounds_are_checked() {
    let z = DVec3::ZERO;
    for bad in [
        0.0,
        -1.0,
        f64::NAN,
        f64::INFINITY,
        MAX_SCALE * 2.0,
        0.5 / MAX_SCALE,
    ] {
        assert_eq!(Motion::scale(z, DVec3::new(1.0, bad, 1.0)), None, "{bad}");
    }
    assert!(Motion::scale(z, DVec3::new(MAX_SCALE, 1.0 / MAX_SCALE, 1.0)).is_some());
    assert_eq!(Motion::scale(DVec3::splat(f64::NAN), DVec3::ONE), None);
    assert_eq!(Motion::scale(DVec3::splat(f64::INFINITY), DVec3::ONE), None);
    // An offset past f64's range: `centre − S·centre`.
    assert_eq!(
        Motion::scale(DVec3::splat(f64::MAX), DVec3::splat(1e3)),
        None
    );
    // A cube scaled past MAX_COORD is refused, about the origin or a far
    // centre.
    let cube = Solid::cuboid(DVec3::splat(1.0), DVec3::splat(1000.0), 1, &TOL).unwrap();
    let big = Motion::scale(z, DVec3::new(1.0, 2000.0, 1.0)).unwrap();
    assert!(matches!(
        cube.transformed(&big, None, &TOL, &Budget::DEFAULT),
        Err(KernelError::Patch(PatchError::Coordinate(_)))
    ));
    let far = Motion::scale(DVec3::splat(6e5), DVec3::splat(3.0)).unwrap();
    assert!(
        cube.transformed(&far, None, &TOL, &Budget::DEFAULT)
            .is_err()
    );
    assert!(
        cube.transformed(
            &Motion::scale(z, DVec3::splat(500.0)).unwrap(),
            None,
            &TOL,
            &Budget::DEFAULT
        )
        .is_ok()
    );
}

#[test]
fn scales_are_deterministic() {
    let (solid, _) = part();
    let ball = sphere(DVec3::new(1.0, 2.0, 3.0));
    assert_deterministic(|| {
        let motion = Motion::scale(DVec3::new(0.5, 0.25, -1.0), DVec3::new(1.3, 0.6, 2.2))
            .unwrap()
            .then(&Motion::turn(DVec3::ZERO, DVec3::new(1.0, 2.0, 3.0), 21.0).unwrap());
        (
            transformed(&solid, &motion, None),
            transformed(&ball, &motion, None),
        )
    });
}

#[test]
fn cones_and_spheres_move_turn_and_mirror() {
    let apex = DVec3::new(1.0, -2.0, 6.0);
    let foot = DVec3::new(1.0, -2.0, 3.0);
    let centre = DVec3::new(-3.0, 0.5, 1.0);
    let solids = [cone(foot), sphere(centre)];
    let motions = [
        Motion::translation(DVec3::new(10.0, -3.5, 0.25)).unwrap(),
        Motion::turn(DVec3::new(1.0, 2.0, 3.0), DVec3::new(1.0, 1.0, 1.0), 30.0).unwrap(),
        Motion::turn(DVec3::ZERO, DVec3::Y, 90.0).unwrap(),
        Motion::mirror(DVec3::new(0.0, 0.0, 2.0), DVec3::Z).unwrap(),
        Motion::mirror(DVec3::new(3.0, -1.0, 0.0), DVec3::new(1.0, 2.0, -0.5)).unwrap(),
    ];
    for motion in &motions {
        for solid in &solids {
            let image = transformed(solid, motion, None);
            assert!(close(image.volume(), solid.volume(), 1e-12));
            assert!(close(image.area(), solid.area(), 1e-12));
            image.mesh().check_faces(&TOL).unwrap();
            forms_face_out(&image);
            assert_eq!(kinds(&image), kinds(solid));
            for face in image.mesh().faces() {
                match face.form {
                    Form::Cone {
                        apex: a,
                        axis,
                        cos,
                        sin,
                    } => {
                        assert!((a - motion.point(apex)).length() < 1e-13);
                        let want = motion.vector(DVec3::NEG_Z);
                        assert!((axis - want).length() < 1e-15);
                        assert!(close(sin / cos, 4.0 / 3.0, 1e-14));
                    }
                    Form::Sphere { centre: c, radius } => {
                        assert!((c - motion.point(centre)).length() < 1e-13);
                        assert!(close(radius, 2.0, 1e-15));
                    }
                    _ => {}
                }
            }
        }
    }
}

#[test]
fn scales_compose_with_turns_and_mirrors() {
    // A spheroid: a half-ellipse of semi-axes 3 across and 2 along the
    // axis turned about it, a surface of revolution of no named kind.
    let quarter = |a: DVec2, c: DVec2, b: DVec2| Segment {
        conic: crate::patch::Conic2::new(a, c, std::f64::consts::FRAC_1_SQRT_2, b).unwrap(),
        curve: 3,
    };
    let spheroid = revolved(
        vec![Loop {
            segments: vec![
                quarter(v(0.0, -2.0), v(3.0, -2.0), v(3.0, 0.0)),
                quarter(v(3.0, 0.0), v(3.0, 2.0), v(0.0, 2.0)),
                Segment::line(v(0.0, 2.0), v(0.0, -2.0), 4).unwrap(),
            ],
        }],
        DVec3::new(1.0, 0.0, -1.0),
    );
    // Fitted: within its area times the fit tolerance.
    let want = 4.0 / 3.0 * PI * 9.0 * 2.0;
    assert!((spheroid.volume() - want).abs() <= spheroid.area() * TOL.fit());
    assert!(kinds(&spheroid).contains(&"revolved"));
    let solids = [
        part().0,
        cone(DVec3::new(1.0, -2.0, 3.0)),
        sphere(DVec3::new(-3.0, 0.5, 1.0)),
        torus(DVec3::new(1.0, 2.0, 3.0)),
        spheroid,
    ];
    let factors = DVec3::new(1.5, 0.75, 2.0);
    let scale = Motion::scale(DVec3::new(0.5, 1.0, -2.0), factors).unwrap();
    let uniform = Motion::scale(DVec3::new(1.0, 2.0, 3.0), DVec3::splat(1.75)).unwrap();
    let mirror = Motion::mirror(DVec3::Y, DVec3::new(1.0, 2.0, -0.5)).unwrap();
    let turn = Motion::turn(DVec3::X, DVec3::new(0.3, 0.1, 1.0), 33.0).unwrap();
    let stretched = factors.x * factors.y * factors.z;
    for (motion, volume, keeps_kinds) in [
        (scale.then(&mirror), stretched, false),
        (mirror.then(&scale), stretched, false),
        (turn.then(&scale).then(&mirror), stretched, false),
        (uniform.then(&mirror), 1.75 * 1.75 * 1.75, true),
        (mirror.then(&turn).then(&uniform), 1.75 * 1.75 * 1.75, true),
    ] {
        assert!(motion.mirrors());
        for solid in &solids {
            // Its planes face out and its tags hold; the debug check of
            // the forms, with the slack, ran in `transformed`.
            let image = transformed(solid, &motion, None);
            let want = solid.volume() * volume;
            assert!(
                close(image.volume(), want, 1e-12),
                "{} for {want}",
                image.volume()
            );
            image.mesh().check_faces(&TOL).unwrap();
            forms_face_out(&image);
            for (a, b) in solid.mesh().faces().iter().zip(image.mesh().faces()) {
                assert_eq!(b.slack, a.slack * motion.stretch());
            }
            if keeps_kinds {
                assert_eq!(kinds(&image), kinds(solid));
            } else {
                // Nothing stays round: a slanted mirror after or before
                // the scale keeps no circle a circle.
                for kind in kinds(&image) {
                    assert!(
                        ["plane", "conic cylinder", "quadric", "unknown"].contains(&kind),
                        "{kind}"
                    );
                }
            }
        }
    }
}
