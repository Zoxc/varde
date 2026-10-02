use super::*;
use crate::testing::{plate, with_body};
use crate::{
    CheckError, Command, Document, EditError, Editor, Extrude, FeatureKind, Operation, Removable,
};
use varde_kernel::mesh::PartKey;

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
        let placement = Plane::Origin(plane).placement().unwrap();
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

/// A face plane has no placement of its own: regenerating finds it.
#[test]
fn a_face_plane_is_placed_by_regenerating() {
    let face = Plane::Face(FaceRef {
        body: BodyId(1),
        key: FaceKey {
            feature: 1,
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::ZERO,
    });
    assert_eq!(face.placement(), None);
    assert_eq!(face.name(), "a face");
    assert!(face.face().is_some());
    assert_eq!(Plane::Origin(OriginPlane::XY).face(), None);
}

/// The bits of a placement, so that equality tells `-0.0` from `0.0`.
fn bits(placement: &Placement) -> [[u64; 3]; 4] {
    [placement.origin, placement.x, placement.y, placement.normal]
        .map(|v| v.to_array().map(f64::to_bits))
}

/// Checks `placement` is a right-handed frame of unit axes, to rounding,
/// with `normal = x × y` and its origin on `n·p = d`.
fn assert_frame(placement: &Placement, n: DVec3, d: f64) {
    let Placement {
        origin,
        x,
        y,
        normal,
    } = *placement;
    for axis in [x, y, normal] {
        assert!((axis.length() - 1.0).abs() < 1e-15, "{axis} isn't unit");
    }
    assert!(x.dot(y).abs() < 1e-15);
    assert!((x.cross(y) - normal).length() < 1e-15);
    assert!(
        (normal - n.normalize()).length() < 1e-15,
        "{normal} isn't {n}"
    );
    assert!((n.normalize().dot(origin) - d / n.length()).abs() < 1e-12);
    // The origin is the plane's point nearest the world origin.
    assert!(origin.cross(normal).length() < 1e-12);
}

/// A face parallel to an origin plane and facing its way gets that
/// plane's placement to the bit, moved along the normal, whatever the
/// normal's length.
#[test]
fn faces_parallel_to_origin_planes_get_their_axes() {
    for plane in OriginPlane::ALL {
        let origin = plane.placement();
        for (scale, d) in [(1.0, 0.0), (5.0, 10.0), (0.3, -7.0), (1e-3, 2.5)] {
            let placement = Placement::on_plane(origin.normal * scale, d).unwrap();
            let expected = Placement {
                origin: origin.normal * d / scale,
                ..origin
            };
            assert_eq!(
                bits(&placement),
                bits(&Placement {
                    origin: expected.origin + DVec3::ZERO,
                    ..expected
                }),
                "{plane:?} scaled by {scale}, at {d}"
            );
        }
    }
}

/// Side faces keep world Z up as the sketch's y; the bottom face is seen
/// from below with x along X and y along −Y.
#[test]
fn side_faces_keep_up_up_and_the_bottom_face_turns_over() {
    // The back (+Y) and left (−X) faces, seen from outside.
    let back = Placement::on_plane(DVec3::Y, 4.0).unwrap();
    assert_eq!(
        bits(&back)[1..3],
        bits(&Placement {
            origin: DVec3::ZERO,
            x: DVec3::NEG_X,
            y: DVec3::Z,
            normal: DVec3::Y,
        })[1..3]
    );
    let left = Placement::on_plane(DVec3::NEG_X, 0.0).unwrap();
    assert_eq!(
        (left.x, left.y, left.normal),
        (DVec3::NEG_Y, DVec3::Z, DVec3::NEG_X)
    );
    let bottom = Placement::on_plane(DVec3::new(0.0, 0.0, -2.0), 6.0).unwrap();
    assert_eq!(
        bits(&bottom),
        bits(&Placement {
            origin: DVec3::new(0.0, 0.0, -3.0),
            x: DVec3::X,
            y: DVec3::NEG_Y,
            normal: DVec3::NEG_Z,
        })
    );
    // A slanted wall of a hexagonal prism: y up its slope, x level.
    let n = DVec3::new(0.5, 3f64.sqrt() / 2.0, 0.0);
    let wall = Placement::on_plane(n, 8.0).unwrap();
    assert_frame(&wall, n, 8.0);
    assert_eq!(wall.y, DVec3::Z);
    assert_eq!(wall.x.z, 0.0);
    // A roof facing +X and up: y climbs its slope toward −X, x is level.
    let n = DVec3::new(1.0, 0.0, 1.0);
    let roof = Placement::on_plane(n, 3.0).unwrap();
    assert_frame(&roof, n, 3.0);
    assert!(roof.y.z > 0.0 && roof.y.x < 0.0);
    assert!((roof.x - DVec3::Y).length() < 1e-15);
}

/// Just inside the horizontal bound x is still world X projected; just
/// outside it y is world Z projected, which then lies almost in XY,
/// pointing up the slope. Both are frames on the plane.
#[test]
fn nearly_horizontal_faces_switch_rules_at_the_bound() {
    for z in [1.0, -1.0] {
        // n̂x² just under and just over 1e-18.
        let inside = DVec3::new(0.9e-9, 0.0, z);
        let placement = Placement::on_plane(inside, 1.0).unwrap();
        assert_frame(&placement, inside, 1.0);
        assert!((placement.x - DVec3::X).length() < 1e-8);
        assert!((placement.y - DVec3::Y * z).length() < 1e-8);

        let outside = DVec3::new(1.1e-9, 0.0, z);
        let placement = Placement::on_plane(outside, 1.0).unwrap();
        assert_frame(&placement, outside, 1.0);
        // y is Z less its part along n̂: up the slope, away from the tilt.
        assert!((placement.y - DVec3::NEG_X * z.signum()).length() < 1e-8);
        assert!(placement.y.z.abs() < 1e-8 && placement.y.z > 0.0);
    }
}

/// The axes don't depend on where the plane is along its normal, so a
/// face moved (a plate made thicker) keeps them to the bit and only the
/// origin moves; a normal scaled by a power of two changes nothing.
#[test]
fn moving_a_face_along_its_normal_moves_only_the_origin() {
    let n = DVec3::new(0.3, -0.7, 0.2);
    let before = Placement::on_plane(n, 1.0).unwrap();
    let after = Placement::on_plane(n, 25.0).unwrap();
    assert_frame(&before, n, 1.0);
    assert_frame(&after, n, 25.0);
    assert_eq!(bits(&before)[1..], bits(&after)[1..]);
    assert_eq!(
        bits(&Placement::on_plane(n * 4.0, 4.0).unwrap()),
        bits(&before)
    );
}

#[test]
fn degenerate_planes_have_no_placement() {
    for (n, d) in [
        (DVec3::ZERO, 0.0),
        (DVec3::new(f64::NAN, 0.0, 1.0), 0.0),
        (DVec3::new(f64::INFINITY, 0.0, 0.0), 0.0),
        (DVec3::Z, f64::NAN),
        (DVec3::Z, f64::INFINITY),
        // Its length overflows.
        (DVec3::splat(f64::MAX), 0.0),
        // Its length's square underflows to zero.
        (DVec3::new(1e-200, 0.0, 0.0), 0.0),
    ] {
        assert_eq!(Placement::on_plane(n, d), None, "{n} {d}");
    }
}

#[test]
fn face_points_are_checked_in_bounds() {
    let face = |near| FaceRef {
        body: BodyId(1),
        key: FaceKey {
            feature: 1,
            part: PartKey::StartCap,
            instance: 0,
        },
        near,
    };
    let max = f64::from(MAX_COORD);
    assert_eq!(face(DVec3::new(max, -max, 0.0)).check_own(), Ok(()));
    for near in [
        DVec3::new(max * 1.01, 0.0, 0.0),
        DVec3::new(0.0, 0.0, f64::NAN),
        DVec3::new(0.0, f64::NEG_INFINITY, 0.0),
    ] {
        assert!(matches!(face(near).check_own(), Err(PlaneError::Near(_))));
    }
}

// Sketches on faces in a document: commands, checks, removal, codec.

/// The example plate's top face (its extrude's end cap), picked at
/// `near`.
fn top_of(document: &Document, near: DVec3) -> FaceRef {
    let body = &document.bodies()[0];
    FaceRef {
        body: body.id,
        key: FaceKey {
            feature: body.created_by.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near,
    }
}

fn on_top(document: &Document) -> Plane {
    Plane::Face(top_of(document, DVec3::new(20.0, 0.0, 10.0)))
}

/// The plane of sketch feature `feature`.
fn plane_of(document: &Document, feature: FeatureId) -> Plane {
    match &document.feature(feature).unwrap().kind {
        FeatureKind::Sketch { plane, .. } => *plane,
        _ => panic!("not a sketch"),
    }
}

/// The example with a sketch added on its plate's top, holding the
/// example's own drawing: the editor and the sketch's id.
fn sketched_on_top() -> (Editor, FeatureId) {
    let mut editor = Editor::new(with_body());
    let plane = on_top(editor.document());
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let id = editor.document().features().last().unwrap().id;
    let FeatureKind::Sketch { sketch, .. } = &editor.document().features()[0].kind else {
        panic!("a sketch first");
    };
    let sketch = Box::new(sketch.clone());
    editor
        .apply(Command::SetSketch {
            feature: id,
            sketch,
        })
        .unwrap();
    (editor, id)
}

#[test]
fn a_sketch_is_added_on_a_face_as_one_edit() {
    let mut editor = Editor::new(with_body());
    let plane = on_top(editor.document());
    let command = editor.document().add_sketch(plane);
    assert!(matches!(&command, Command::AddSketch { name, .. } if name == "Sketch 2"));
    editor.apply(command).unwrap();
    let id = editor.document().features().last().unwrap().id;
    assert_eq!(plane_of(editor.document(), id), plane);
    editor.undo();
    assert!(editor.document().feature(id).is_none());
    editor.redo();
    assert_eq!(plane_of(editor.document(), id), plane);
}

#[test]
fn set_sketch_plane_moves_a_sketch_keeping_its_drawing() {
    let (mut editor, id) = sketched_on_top();
    let drawn = |editor: &Editor| match &editor.document().feature(id).unwrap().kind {
        FeatureKind::Sketch { sketch, .. } => sketch.clone(),
        _ => unreachable!(),
    };
    let drawing = drawn(&editor);
    let top = on_top(editor.document());
    let xz = Plane::Origin(OriginPlane::XZ);
    let revision = editor.revision();

    editor
        .apply(Command::SetSketchPlane {
            feature: id,
            plane: xz,
        })
        .unwrap();
    assert_eq!(plane_of(editor.document(), id), xz);
    assert_eq!(drawn(&editor), drawing);
    editor.undo();
    assert_eq!(plane_of(editor.document(), id), top);
    assert_eq!(editor.revision(), revision);
    editor.redo();
    assert_eq!(plane_of(editor.document(), id), xz);

    // Back onto the face, picked elsewhere on it: another reference.
    let elsewhere = Plane::Face(top_of(editor.document(), DVec3::new(-20.0, 5.0, 10.0)));
    editor
        .apply(Command::SetSketchPlane {
            feature: id,
            plane: elsewhere,
        })
        .unwrap();
    assert_eq!(plane_of(editor.document(), id), elsewhere);
    assert_eq!(drawn(&editor), drawing);

    // The plane it's on, a feature that isn't a sketch and one that isn't
    // there change nothing.
    let revision = editor.revision();
    let extrude = editor.document().features()[1].id;
    for (feature, plane) in [(id, elsewhere), (extrude, xz), (FeatureId(999), xz)] {
        editor
            .apply(Command::SetSketchPlane { feature, plane })
            .unwrap();
        assert_eq!(editor.revision(), revision);
    }
    assert!(matches!(
        editor.document().feature(extrude).unwrap().kind,
        FeatureKind::Extrude(_)
    ));
}

/// The refusal of setting sketch `feature`'s plane to `plane`.
fn refused(editor: &mut Editor, feature: FeatureId, plane: Plane) -> PlaneError {
    let before = editor.revision();
    let refusal = editor.apply(Command::SetSketchPlane { feature, plane });
    assert_eq!(editor.revision(), before);
    match refusal {
        Err(EditError::Invalid(CheckError::SketchPlane(id, why))) => {
            assert_eq!(id, feature);
            why
        }
        other => panic!("not refused for its plane: {other:?}"),
    }
}

#[test]
fn a_face_plane_names_only_what_comes_before_its_sketch() {
    let (mut editor, id) = sketched_on_top();
    let document = editor.document().clone();
    let first = document.features()[0].id;
    let top = top_of(&document, DVec3::new(20.0, 0.0, 10.0));

    // The first sketch can't go on the plate its own extrude makes.
    assert_eq!(
        refused(&mut editor, first, Plane::Face(top)),
        PlaneError::Body(top.body)
    );
    // Nor on a face whose key names a later feature, or itself, whatever
    // the body.
    let gone = BodyId(900);
    for maker in [document.features()[1].id, first] {
        let face = FaceRef {
            body: gone,
            key: FaceKey {
                feature: maker.get(),
                ..top.key
            },
            ..top
        };
        assert_eq!(
            refused(&mut editor, first, Plane::Face(face)),
            PlaneError::Maker(maker)
        );
    }
    let mine = FaceRef {
        key: FaceKey {
            feature: id.get(),
            ..top.key
        },
        ..top
    };
    assert_eq!(
        refused(&mut editor, id, Plane::Face(mine)),
        PlaneError::Maker(id)
    );
    // A point out of bounds.
    for near in [
        DVec3::new(f64::NAN, 0.0, 0.0),
        DVec3::new(0.0, 0.0, f64::from(crate::MAX_COORD) * 2.0),
    ] {
        assert!(matches!(
            refused(&mut editor, id, Plane::Face(FaceRef { near, ..top })),
            PlaneError::Near(_)
        ));
    }
    // A body or a key's feature that isn't there is allowed: it fails to
    // resolve, as a region can.
    let nowhere = FaceRef {
        body: gone,
        key: FaceKey {
            feature: 950,
            ..top.key
        },
        ..top
    };
    for plane in [
        Plane::Face(nowhere),
        Plane::Face(FaceRef { body: gone, ..top }),
    ] {
        editor
            .apply(Command::SetSketchPlane { feature: id, plane })
            .unwrap();
        assert_eq!(plane_of(editor.document(), id), plane);
    }
    // The same is checked of a document as read: an earlier sketch moved
    // onto a later body's face.
    let mut moved = document.clone();
    let FeatureKind::Sketch { plane, .. } = &mut moved.features[0].kind else {
        unreachable!();
    };
    *plane = Plane::Face(top);
    assert_eq!(
        moved.check(),
        Err(CheckError::SketchPlane(first, PlaneError::Body(top.body)))
    );
}

/// Removing the plate's extrude, or the plate, takes its body but not the
/// sketch on its face (nor what the sketch made), which stays naming the
/// body that's gone, for regenerating to fail.
#[test]
fn removing_a_face_s_body_leaves_the_sketch_on_it() {
    let (mut editor, sketch) = sketched_on_top();
    let extrude = Extrude {
        sketch,
        ..plate(Operation::NewBody(BodyId::NEW))
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    let document = editor.document().clone();
    let [plate_extrude, boss] = [1, 3].map(|i| document.features()[i].id);
    let [plate_body, boss_body] = [0, 1].map(|i| document.bodies()[i].id);
    let top = on_top(&document);

    for target in [
        Removable::Feature(plate_extrude),
        Removable::Body(plate_body),
    ] {
        let removal = document.removal(target);
        assert_eq!(removal.features, [plate_extrude]);
        assert_eq!(removal.bodies, [plate_body]);
        let command = match target {
            Removable::Feature(id) => Command::RemoveFeature(id),
            Removable::Body(id) => Command::RemoveBody(id),
        };
        editor.apply(command).unwrap();
        let after = editor.document();
        assert_eq!(plane_of(after, sketch), top);
        assert!(after.body(plate_body).is_none());
        assert_eq!(after.body(boss_body).unwrap().created_by, boss);
        assert_eq!(after.check(), Ok(()));
        editor.undo();
        assert_eq!(*editor.document(), document);
    }

    // The plate's extrude made a join, so no longer making the body: the
    // sketch stays too.
    let join = plate(Operation::Join(crate::Targets::default()));
    editor
        .apply(Command::SetFeature {
            feature: plate_extrude,
            kind: Box::new(join.into()),
        })
        .unwrap();
    assert!(editor.document().body(plate_body).is_none());
    assert_eq!(plane_of(editor.document(), sketch), top);

    // Removing the sketch still takes what it made.
    editor.undo();
    let removal = editor.document().removal(Removable::Feature(sketch));
    assert_eq!(removal.features, [sketch, boss]);
    assert_eq!(removal.bodies, [boss_body]);
}

#[test]
fn a_document_with_a_sketch_on_a_face_round_trips() {
    let (editor, _) = sketched_on_top();
    let document = editor.document().clone();
    let bytes = document.to_postcard();
    assert_eq!(Document::from_postcard(&bytes), Ok(document.clone()));
    // Changing units leaves the plane alone.
    let mut editor = editor;
    editor
        .apply(Command::SetUnits(crate::LengthUnit::In))
        .unwrap();
    let sketch = document.features().last().unwrap().id;
    assert_eq!(plane_of(editor.document(), sketch), on_top(&document));
}

/// Bytes naming a face wrongly, or a plane kind there isn't, are refused,
/// saying why.
#[test]
fn bad_face_planes_in_bytes_are_refused() {
    let (editor, sketch) = sketched_on_top();
    let document = editor.document().clone();
    let index = document.feature_index(sketch).unwrap();
    let with_plane = |plane: Plane| {
        let mut bad = document.clone();
        let FeatureKind::Sketch { plane: old, .. } = &mut bad.features[index].kind else {
            unreachable!();
        };
        *old = plane;
        // Serializing doesn't check.
        bad.to_postcard()
    };
    let top = top_of(&document, DVec3::ZERO);
    let later = FaceRef {
        key: FaceKey {
            feature: sketch.get(),
            ..top.key
        },
        ..top
    };
    for (plane, expected) in [
        (
            Plane::Face(FaceRef {
                near: DVec3::new(0.0, f64::INFINITY, 0.0),
                ..top
            }),
            "out of bounds",
        ),
        (Plane::Face(later), "doesn't come before it"),
    ] {
        let refusal = Document::from_postcard(&with_plane(plane)).unwrap_err();
        assert!(
            matches!(
                std::error::Error::source(&refusal).and_then(|why| why.downcast_ref()),
                Some(CheckError::SketchPlane(id, _)) if *id == sketch
            ),
            "{refusal}"
        );
        assert!(refusal.to_string().contains(expected), "{refusal}");
        assert!(postcard::from_bytes::<Document>(&with_plane(plane)).is_err());
    }

    // An empty document with one sketch on XY: the plane's kind is the
    // 15th byte (no bodies, one feature, its id, its name "Sketch 1"
    // by length, visible, the sketch kind). There's no third kind.
    let mut editor = Editor::new(Document::default());
    let xy = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(xy)).unwrap();
    let mut bytes = editor.document().to_postcard();
    assert_eq!(bytes[13..15], [0, 0]);
    bytes[14] = 2;
    assert!(Document::from_postcard(&bytes).is_err());
}
