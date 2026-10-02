use glam::DVec3;
use varde_kernel::mesh::{FaceKey, PartKey};

use super::*;

/// The key of the end cap of feature `feature`.
fn key(feature: u64) -> FaceKey {
    FaceKey {
        feature,
        part: PartKey::EndCap,
        instance: 0,
    }
}

/// An edge reference between the faces keyed by features 1 and 2.
fn reference() -> EdgeRef {
    EdgeRef {
        body: BodyId(1),
        faces: [key(1), key(2)],
        near: DVec3::ZERO,
    }
}

/// An edge runs with the first key's face on its left: named by key or
/// by alias, each key naming one face. Where aliases name each face by
/// both keys, the face's own key decides; where it can't, or the keys
/// don't name the faces, the direction can't be told.
#[test]
fn an_edge_runs_with_its_first_key_s_face_on_its_left() {
    let edge = reference();
    let (a, b, c, d) = (key(1), key(2), key(3), key(4));
    let none: &[FaceKey] = &[];
    // By the faces' own keys.
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&a, none), (&b, none)),
        Some(true)
    );
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&b, none), (&a, none)),
        Some(false)
    );
    // A face renamed, its old key an alias.
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&c, &[a]), (&b, none)),
        Some(true)
    );
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&b, none), (&c, &[a])),
        Some(false)
    );
    // A face named by both keys, the other by one: the other decides,
    // whichever of them is its own key.
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&a, &[b]), (&c, &[a])),
        Some(false)
    );
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&c, &[a, b]), (&a, none)),
        Some(false)
    );
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&c, &[a, b]), (&b, none)),
        Some(true)
    );
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&a, &[b]), (&b, none)),
        Some(true)
    );
    // Both faces named by both keys: by their own keys.
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&a, &[b]), (&b, &[a])),
        Some(true)
    );
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&b, &[a]), (&a, &[b])),
        Some(false)
    );
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&c, &[a, b]), (&b, &[a])),
        Some(true)
    );
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&c, &[a, b]), (&d, &[a, b])),
        None
    );
    // Keys naming the faces neither way.
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&a, none), (&c, none)),
        None
    );
    assert_eq!(
        EdgeRef::runs_with(&edge.faces, (&c, none), (&d, none)),
        None
    );
}
