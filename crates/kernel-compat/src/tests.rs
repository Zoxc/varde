//! The facade's stubs: exact answers for the empty solid, errors naming
//! what isn't built for anything else, and no panic on odd input.

use glam::{DVec2, DVec3};

use crate::*;

const TOL: Tolerance = Tolerance::DEFAULT;

fn square() -> Profile {
    let p = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(x, y)| DVec2::new(x, y));
    Profile {
        loops: vec![Loop {
            segments: (0..4)
                .map(|i| Segment::line(p[i], p[(i + 1) % 4], i as u64).unwrap())
                .collect(),
        }],
    }
}

fn not_implemented<T: std::fmt::Debug>(result: Result<T, Failure>) -> bool {
    matches!(
        result,
        Err(Failure {
            error: KernelError::NotImplemented(_),
            ..
        })
    )
}

#[test]
fn making_geometry_isn_t_implemented_but_a_bad_profile_says_so() {
    let budget = Budget::DEFAULT;
    let made = extrude(&square(), &Frame::XY, 0.0, 1.0, 1, &TOL, &budget);
    assert!(not_implemented(made));
    let turned = revolve(&square(), &Frame::XY, Sweep::Full, 1, &TOL, &budget);
    assert!(not_implemented(turned));
    let empty = Profile::default();
    let bad = extrude(&empty, &Frame::XY, 0.0, 1.0, 1, &TOL, &budget);
    assert_eq!(
        bad.unwrap_err().error,
        KernelError::Profile(ProfileError::Empty)
    );
    let bad = revolve(&empty, &Frame::XY, Sweep::Full, 1, &TOL, &budget);
    assert_eq!(
        bad.unwrap_err().error,
        KernelError::Profile(ProfileError::Empty)
    );
    let bad = extrude_tapered(&empty, &Frame::XY, 0.0, 1.0, 0.1, 1, &TOL, &budget);
    assert!(matches!(
        bad,
        Err(TaperError::Failed(Failure {
            error: KernelError::Profile(ProfileError::Empty),
            ..
        }))
    ));
    assert_eq!(
        Solid::cuboid(DVec3::ZERO, DVec3::ONE, 1, &TOL),
        Err(KernelError::NotImplemented("boxes"))
    );
    assert_eq!(
        KernelError::NotImplemented("extruding").to_string(),
        "extruding isn't implemented yet"
    );
}

#[test]
fn the_empty_solid_combines_exactly() {
    let (budget, empty) = (Budget::DEFAULT, Solid::empty());
    for op in [Op::Union, Op::Difference, Op::Intersection] {
        assert_eq!(
            boolean(&empty, &empty, op, &TOL, &budget),
            Ok(Solid::empty())
        );
    }
    assert_eq!(touches(&empty, &empty, &TOL, &budget), Ok(false));
    assert_eq!(assemble(&[], &TOL, &budget), Ok(Solid::empty()));
    assert_eq!(
        assemble(&[empty.clone(), empty.clone()], &TOL, &budget),
        Ok(Solid::empty())
    );
    let far = Motion::translation(DVec3::splat(1e300)).unwrap();
    assert_eq!(
        empty.transformed(&far, None, &TOL, &budget),
        Ok(Solid::empty())
    );
}

#[test]
fn the_empty_solid_answers_queries_without_panicking() {
    let (budget, empty) = (Budget::DEFAULT, Solid::empty());
    assert!(empty.is_empty());
    assert_eq!(empty.bounds(), None);
    assert_eq!(empty.bounds3(), None);
    assert_eq!(empty.volume(), 0.0);
    assert_eq!(empty.area(), 0.0);
    assert_eq!(
        empty.tessellate(&Display::default()),
        Ok(RenderMesh::default())
    );
    assert_eq!(
        empty.manifold_mesh(&Display::default()),
        Err(ManifoldError::Empty)
    );
    assert_eq!(empty.tight_bounds(&TOL, &budget), Ok(None));
    assert_eq!(empty.moments(&budget).unwrap().centre, None);
    let topology = empty.topology();
    for pick in [
        measure::Pick::Body,
        measure::Pick::Face(u32::MAX),
        measure::Pick::Edge(0),
        measure::Pick::Corner(7),
        measure::Pick::EdgePoint(1),
    ] {
        let target = measure::Target {
            solid: &empty,
            topology: &topology,
            pick,
        };
        let measured = measure::measure(&target, &TOL, &budget);
        assert_eq!(measured.is_ok(), pick == measure::Pick::Body, "{pick:?}");
        assert!(measure::distance(&target, &target, &TOL, &budget).is_err());
    }
    let plane = section::Plane {
        point: DVec3::ZERO,
        normal: DVec3::Z,
    };
    assert_eq!(
        section::face_section(&empty, &[0, 9], &plane, 1e-6),
        Ok(Vec::new())
    );
}

/// An axis or direction of any finite size other than zero is one:
/// quadrics about it are made and a face's form gives it, unit.
#[test]
fn directions_of_any_finite_size_are_directions() {
    use crate::mesh::{Form, Quadric};
    for length in [1e-200, 1e-10, 49.0, 1e200, f64::MAX] {
        let axis = DVec3::new(0.0, 0.0, length);
        let cylinder = Quadric::cylinder(DVec3::ZERO, axis, 2.0).expect("an axis");
        assert_eq!(
            cylinder.distance(DVec3::new(2.0, 0.0, 7.0)),
            0.0,
            "{length}"
        );
        let cone = Quadric::cone(DVec3::ZERO, axis, 1.0, 1.0).expect("an axis");
        assert!(
            cone.value(DVec3::new(1.0, 0.0, 1.0)).abs() < 1e-15,
            "{length}"
        );
        assert!(Quadric::revolution(DVec3::ZERO, axis, 1.0, 0.0, 0.0).is_some());
        let face = measure::FaceMeasure {
            area: 0.0,
            form: Form::Cylinder {
                point: DVec3::ZERO,
                axis,
                radius: 1.0,
            },
            rectangle: None,
        };
        assert_eq!(face.direction().map(|d| d.v), Some(DVec3::Z), "{length}");
    }
    assert_eq!(Quadric::cylinder(DVec3::ZERO, DVec3::ZERO, 1.0), None);
    assert_eq!(Quadric::cylinder(DVec3::ZERO, DVec3::NAN, 1.0), None);
}
