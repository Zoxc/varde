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
    assert!(extrude(KernelError::TooComplex).contains("too complex to extrude"));
    let touching = KernelError::Profile(ProfileError::Touching([(0, 1), (1, 0)]));
    assert_eq!(
        extrude(touching),
        "its outline touches or crosses itself, or comes too close to itself"
    );
    let text = extrude(KernelError::Invalid(CheckError::Counts));
    assert!(text.contains("too thin or too close"), "{text}");
}

#[test]
fn every_boolean_failure_starts_in_lower_case() {
    let errors = [
        KernelError::TooComplex,
        KernelError::Invalid(CheckError::Counts),
        KernelError::Patch(varde_kernel::patch::PatchError::Mismatch),
        KernelError::Profile(ProfileError::Nesting),
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
