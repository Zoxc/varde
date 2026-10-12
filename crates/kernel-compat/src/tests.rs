//! The facade's own answers: an empty profile refused, the empty solid
//! combined exactly, and directions of any finite size.

use glam::DVec3;

use crate::*;

const TOL: Tolerance = Tolerance::DEFAULT;

#[test]
fn an_empty_profile_is_refused_as_empty() {
    let budget = Budget::DEFAULT;
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
}

/// Exact for any kernel: every boolean of the empty solid with itself
/// is empty, it touches nothing, assembling none or empties is empty and
/// moving it, however far, leaves it empty.
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
