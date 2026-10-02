use varde_kernel::mesh::CheckError;

use super::*;

/// Why extruding failed.
fn extrude(error: KernelError, finest: bool) -> String {
    tool(Making::Extrude, error, finest)
}

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
fn a_failure_with_other_bodies_says_to_leave_the_body_out() {
    let text = leave_out(
        boolean(Doing::Joining, "Body 2", KernelError::TooComplex),
        "Body 2",
    );
    assert!(
        text.starts_with("joining it to Body 2 is too complex"),
        "{text}"
    );
    assert!(
        text.ends_with("; untick Body 2 under Bodies to leave it out"),
        "{text}"
    );
}

#[test]
fn merging_failures_name_both_bodies_and_how_to_keep_them_apart() {
    for error in [
        KernelError::TooComplex,
        KernelError::Invalid(CheckError::Counts),
        KernelError::Boolean(BooleanError::Inconsistent),
        KernelError::Boolean(BooleanError::Degenerate),
        KernelError::Patch(varde_kernel::patch::PatchError::Mismatch),
    ] {
        let text = merging("Body 1", "Body 2", error);
        let tail = boolean(Doing::Joining, "Body 1", error)
            .strip_prefix("joining it to Body 1")
            .unwrap()
            .to_owned();
        let keep = "; or untick Body 2 under Bodies to keep it apart";
        assert_eq!(text, format!("merging Body 2 into Body 1{tail}{keep}"));
    }
    let text = merging("Body 1", "Body 3", KernelError::Invalid(CheckError::Counts));
    assert!(
        text.starts_with("merging Body 3 into Body 1 leaves no clean solid"),
        "{text}"
    );
}

#[test]
fn extrude_failures_speak_of_the_regions() {
    let touching = KernelError::Profile(ProfileError::Touching([(0, 1), (1, 0)]));
    assert_eq!(
        extrude(touching, false),
        "its outline touches or crosses itself, or comes too close to itself"
    );
    assert_eq!(
        extrude(KernelError::Profile(ProfileError::Triangulation), false),
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
        let text = extrude(error, false);
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
    let text = extrude(KernelError::Profile(ProfileError::TooFine(1, 2)), false);
    assert_eq!(
        text,
        "its outline has detail too small for this tolerance: try a finer tolerance"
    );
    let text = extrude(KernelError::Invalid(CheckError::Counts), false);
    assert_eq!(
        text,
        "its regions have parts too thin or too close together for this tolerance: \
         try a finer tolerance"
    );
    // Out of budget, it's the curves, not the tolerance.
    let text = extrude(KernelError::TooComplex, false);
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
        for finest in [false, true] {
            let text = extrude(error, finest);
            assert!(text.starts_with(char::is_lowercase), "{text}");
        }
    }
}

#[test]
fn at_the_finest_tolerance_none_finer_is_suggested() {
    // A strip 1e-7 wide is too thin even at the finest tolerance (fit
    // 1e-5, a resolution of 1e-8): there is no finer one to try.
    let mut naming = 0;
    for error in extrude_errors() {
        let text = extrude(error, true);
        assert!(!text.contains("try a finer"), "{text}");
        assert!(!text.contains("coarser"), "{text}");
        if text.contains("tolerance") {
            assert!(text.ends_with(", even at the finest tolerance"), "{text}");
            naming += 1;
        }
    }
    assert_eq!(naming, 2);
    assert_eq!(
        extrude(KernelError::Invalid(CheckError::Counts), true),
        "its regions have parts too thin or too close together to extrude, even at the \
         finest tolerance"
    );
    assert_eq!(
        extrude(KernelError::Profile(ProfileError::TooFine(1, 2)), true),
        "its outline has detail too small to extrude, even at the finest tolerance"
    );
    // The rest read the same at any tolerance.
    for error in extrude_errors() {
        if !matches!(
            error,
            KernelError::Invalid(_) | KernelError::Profile(ProfileError::TooFine(..))
        ) {
            assert_eq!(extrude(error, true), extrude(error, false));
        }
    }
}

#[test]
fn emptied_names_the_body_and_the_way_past_it() {
    let text = emptied(Doing::Cutting, "Body 1");
    assert!(
        text.starts_with("cutting it from Body 1 would leave nothing of it"),
        "{text}"
    );
    assert!(text.contains("untick it under Bodies"), "{text}");
    let text = emptied(Doing::Intersecting, "Body 2");
    assert!(
        text.starts_with("intersecting it with Body 2 would leave nothing of it"),
        "{text}"
    );
    assert!(text.contains("flip it or move it to overlap"), "{text}");
    let text = emptied(Doing::Joining, "Body 3");
    assert_eq!(text, "joining it to Body 3 would leave nothing of it");
}

#[test]
fn revolve_failures_say_revolve() {
    assert_eq!(
        tool(Making::Revolve, KernelError::TooComplex, false),
        "its regions are too complex to revolve: try fewer or simpler curves"
    );
    assert_eq!(
        tool(
            Making::Revolve,
            KernelError::Profile(ProfileError::TooFine(1, 2)),
            true
        ),
        "its outline has detail too small to revolve, even at the finest tolerance"
    );
    assert_eq!(
        tool(
            Making::Revolve,
            KernelError::Profile(ProfileError::CrossesAxis(0, 1)),
            false
        ),
        "its outline crosses the axis"
    );
    let text = tool(
        Making::Revolve,
        KernelError::Profile(ProfileError::Degenerate(0, 1)),
        false,
    );
    assert!(
        text.starts_with("its outline can't be revolved: "),
        "{text}"
    );
    for error in extrude_errors() {
        let text = tool(Making::Revolve, error, false);
        assert!(!text.contains("extrude"), "{text}");
    }
}
