use varde_kernel::mesh::CheckError;

use super::*;

#[test]
fn boolean_failures_name_the_body_and_what_to_try() {
    let invalid = KernelError::Invalid(CheckError::Counts);
    let text = boolean(Doing::Joining, "Body 2", invalid);
    assert!(
        text.starts_with("joining it to Body 2 leaves no clean solid"),
        "{text}"
    );
    assert!(text.contains("tangent"), "{text}");
    // Not every boolean `Invalid` is such a contact: a guess, not a fact,
    // and no tolerance to try.
    assert!(text.contains("they may meet"), "{text}");
    assert!(text.contains("if so, move it"), "{text}");
    assert!(!text.contains("tolerance"), "{text}");
    let text = boolean(
        Doing::Cutting,
        "Body 1",
        KernelError::Boolean(BooleanError::Inconsistent),
    );
    assert!(
        text.starts_with("cutting it from Body 1 can't be worked out"),
        "{text}"
    );
    let text = boolean(Doing::Intersecting, "Body 3", KernelError::TooComplex);
    assert!(
        text.starts_with("intersecting it with Body 3 is too complex"),
        "{text}"
    );
    let text = boolean(Doing::Touching, "Body 3", KernelError::TooComplex);
    assert!(text.starts_with("finding where it meets Body 3"), "{text}");
}

#[test]
fn extrude_failures_speak_of_the_regions() {
    let touching = KernelError::Profile(ProfileError::Touching([(0, 1), (1, 0)]));
    assert_eq!(
        extrude(touching),
        "its outline touches or crosses itself, or comes too close to itself"
    );
    assert_eq!(
        extrude(KernelError::Profile(ProfileError::Triangulation)),
        "its end faces couldn't be made"
    );
}

/// Every way an extrude can fail, kernel errors and profile ones.
fn extrude_errors() -> Vec<KernelError> {
    let profile = [
        ProfileError::Empty,
        ProfileError::TooManySegments(70_000),
        ProfileError::Short(0),
        ProfileError::Segment(0, 1, varde_kernel::patch::PatchError::Mismatch),
        ProfileError::Degenerate(0, 1),
        ProfileError::Open(0, 1),
        ProfileError::Area(0),
        ProfileError::Cusp(0, 1),
        ProfileError::Touching([(0, 1), (1, 0)]),
        ProfileError::Nesting,
        ProfileError::Triangulation,
        ProfileError::TooFine(1, 2),
    ];
    let mut errors = vec![
        KernelError::TooComplex,
        KernelError::Invalid(CheckError::Counts),
        KernelError::Patch(varde_kernel::patch::PatchError::Mismatch),
        KernelError::Boolean(BooleanError::Inconsistent),
    ];
    errors.extend(profile.map(KernelError::Profile));
    errors
}

#[test]
fn extrude_failures_name_the_tolerance_only_to_suggest_a_finer_one() {
    // A coarser tolerance mends none of them: the budget and the limits
    // don't depend on it, and detail too small for it gets worse. Where a
    // finer one isn't known to help either, the tolerance isn't named.
    let mut naming = 0;
    for error in extrude_errors() {
        let text = extrude(error);
        assert!(!text.contains("coarser"), "{text}");
        assert!(text.starts_with(char::is_lowercase), "{text}");
        if text.contains("tolerance") {
            assert!(text.ends_with(": try a finer tolerance"), "{text}");
            naming += 1;
        }
    }
    // `Invalid` and `TooFine`.
    assert_eq!(naming, 2);
    // Nor do regen's own profile errors, shown as they are.
    let profile = [
        crate::ProfileError::Missing,
        crate::ProfileError::TooManySegments,
        crate::ProfileError::Fit,
        crate::ProfileError::Patch(varde_kernel::patch::PatchError::Mismatch),
    ];
    for error in profile {
        let text = error.to_string();
        assert!(!text.contains("tolerance"), "{text}");
    }
}

#[test]
fn detail_too_fine_for_the_tolerance_suggests_a_finer_one() {
    let text = extrude(KernelError::Profile(ProfileError::TooFine(1, 2)));
    assert_eq!(
        text,
        "its outline has detail too small for this tolerance: try a finer tolerance"
    );
    let text = extrude(KernelError::Invalid(CheckError::Counts));
    assert_eq!(
        text,
        "its regions have parts too thin or too close together for this tolerance: \
         try a finer tolerance"
    );
    // Out of budget, it's the curves, not the tolerance.
    let text = extrude(KernelError::TooComplex);
    assert_eq!(
        text,
        "its regions are too complex to extrude: try fewer or simpler curves"
    );
}

#[test]
fn every_boolean_failure_starts_in_lower_case() {
    let errors = [
        KernelError::TooComplex,
        KernelError::Invalid(CheckError::Counts),
        KernelError::Patch(varde_kernel::patch::PatchError::Mismatch),
        KernelError::Profile(ProfileError::Nesting),
        KernelError::Profile(ProfileError::TooFine(0, 3)),
        KernelError::Boolean(BooleanError::Inconsistent),
        KernelError::Boolean(BooleanError::Degenerate),
    ];
    let doings = [
        Doing::Touching,
        Doing::Joining,
        Doing::Cutting,
        Doing::Intersecting,
    ];
    for error in errors {
        for doing in doings {
            let text = boolean(doing, "Body 2", error);
            assert!(text.starts_with(doing.name()), "{text}");
            assert!(text.contains("Body 2"), "{text}");
        }
        let text = extrude(error);
        assert!(text.starts_with(char::is_lowercase), "{text}");
    }
}
