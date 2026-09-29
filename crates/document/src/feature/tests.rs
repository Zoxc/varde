use super::*;

/// The world axis `letter` names.
fn axis(letter: char) -> DVec3 {
    match letter {
        'X' => DVec3::X,
        'Y' => DVec3::Y,
        _ => DVec3::Z,
    }
}

#[test]
fn origin_planes_are_right_handed_and_span_their_axes() {
    for plane in OriginPlane::ALL {
        let placement = Plane::Origin(plane).placement();
        assert_eq!(placement.origin, DVec3::ZERO);
        assert_eq!(placement.x.cross(placement.y), placement.normal);
        // The plane's axes are the two of its name, in order, and the
        // normal is the third.
        let [x, y] = [0, 1].map(|i| axis(plane.name().chars().nth(i).unwrap()));
        assert_eq!((placement.x, placement.y), (x, y));
        assert_eq!(placement.normal.abs(), DVec3::ONE - x - y);
    }
}

#[test]
fn planes_face_the_views_they_are_drawn_from() {
    // Top looks down at XY, the front (from -Y) at XZ, the right (from +X)
    // at YZ.
    let normal = |plane| OriginPlane::placement(plane).normal;
    assert_eq!(normal(OriginPlane::XY), DVec3::Z);
    assert_eq!(normal(OriginPlane::XZ), DVec3::NEG_Y);
    assert_eq!(normal(OriginPlane::YZ), DVec3::X);
}

#[test]
fn sketch_points_map_into_the_plane() {
    let placement = OriginPlane::YZ.placement();
    assert_eq!(
        placement.to_world(DVec2::new(2.0, 3.0)),
        DVec3::new(0.0, 2.0, 3.0)
    );
    let moved = Placement {
        origin: DVec3::new(1.0, 1.0, 1.0),
        ..OriginPlane::XZ.placement()
    };
    assert_eq!(
        moved.to_world(DVec2::new(2.0, 3.0)),
        DVec3::new(3.0, 1.0, 4.0)
    );
}
