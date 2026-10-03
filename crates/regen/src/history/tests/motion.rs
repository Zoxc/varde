//! Moves and mirrors in the history: a block moved and then joined where
//! it went, quarter turns exact to the bit, turns about model edges and
//! round faces and rims, mirrors with and without the original, in an
//! origin plane and in planar faces (square and tilted), refusals (out
//! of range, a mirror face that isn't flat, an axis face that isn't
//! round or isn't there, a consumed body), sketches following a moved
//! face, and what's cached.

use glam::DVec3;
use varde_document::{Axis3, AxisRef, BodyOp, Combine, EdgeRef, FaceRef, Mirror, Move, PlaneRef};
use varde_kernel::mesh::{FaceKey, Form, PartKey};

use super::*;
use crate::picking::region_form;

/// A length or an angle of `text` for a move in `document`.
fn offset(document: &Document, text: &str) -> Value {
    Value::new(text, &Move::offset_ask(&document.design())).unwrap()
}

fn angle(document: &Document, text: &str) -> Value {
    Value::new(text, &Move::angle_ask(&document.design())).unwrap()
}

/// A move of `bodies` by the offsets typed, turned about `turn` first if
/// it's some.
fn shift(
    document: &Document,
    bodies: &[BodyId],
    [x, y, z]: [&str; 3],
    turn: Option<(AxisRef, &str)>,
) -> Move {
    Move {
        bodies: bodies.to_vec(),
        offset: [x, y, z].map(|text| offset(document, text)),
        turn: turn.map(|(axis, text)| (axis, angle(document, text))),
    }
}

/// Adds `kind` as one edit: its id.
fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> FeatureId {
    editor
        .apply(editor.document().add_feature(kind.into()))
        .unwrap();
    editor.document().features().last().unwrap().id
}

/// Adds a [`shift`]: its id.
fn add_move(
    editor: &mut Editor,
    bodies: &[BodyId],
    offsets: [&str; 3],
    turn: Option<(AxisRef, &str)>,
) -> FeatureId {
    let moved = shift(editor.document(), bodies, offsets, turn);
    add(editor, moved)
}

/// Sets the move `feature` to a [`shift`].
fn set_move(
    editor: &mut Editor,
    feature: FeatureId,
    bodies: &[BodyId],
    offsets: [&str; 3],
    turn: Option<(AxisRef, &str)>,
) {
    let moved = shift(editor.document(), bodies, offsets, turn);
    set(editor, feature, moved);
}

/// Sets feature `feature` to `kind`.
fn set(editor: &mut Editor, feature: FeatureId, kind: impl Into<FeatureKind>) {
    editor
        .apply(Command::SetFeature {
            feature,
            kind: Box::new(kind.into()),
        })
        .unwrap();
}

/// Adds the block from `(x0, y0)` to `(x1, y1)`, `height` up from XY: its
/// body.
fn block(editor: &mut Editor, x0: f64, y0: f64, x1: f64, y1: f64, height: &str) -> BodyId {
    add_body(editor, rectangle((x0, y0), (x1, y1)), height)
}

/// Whether `solid`'s box is `min` to `max` to the bit.
fn boxed(solid: &Solid, min: [f64; 3], max: [f64; 3]) -> bool {
    let bounds = solid.bounds3().unwrap();
    bounds.min == DVec3::from(min) && bounds.max == DVec3::from(max)
}

/// Whether `solid`'s box is `min` to `max` within rounding.
fn near_box(solid: &Solid, min: [f64; 3], max: [f64; 3]) -> bool {
    let bounds = solid.bounds3().unwrap();
    (bounds.min - DVec3::from(min)).abs().max_element() < 1e-9
        && (bounds.max - DVec3::from(max)).abs().max_element() < 1e-9
}

/// The key of the one region of `solid` whose form `pick` takes.
fn key_where(solid: &Solid, pick: impl Fn(&Form) -> bool) -> FaceKey {
    let topology = solid.topology();
    let found: Vec<FaceKey> = (topology.regions().iter())
        .filter(|region| pick(region_form(solid, region)))
        .map(|region| region.key)
        .collect();
    let [key] = found[..] else {
        panic!("one region: {found:?}");
    };
    key
}

/// The key of the one region of `solid` on the plane `n·x = d`.
fn key_on(solid: &Solid, n: DVec3, d: f64) -> FaceKey {
    key_where(solid, |form| {
        matches!(*form, Form::Plane { n: m, d: e }
            if m.abs_diff_eq(n, 1e-12) && (e - d).abs() < 1e-9)
    })
}

/// The key of the one cylinder of `solid`.
fn cylinder(solid: &Solid) -> FaceKey {
    key_where(solid, |form| matches!(form, Form::Cylinder { .. }))
}

/// What `evaluation` says of `feature`'s failing, if it failed.
fn failure(evaluation: &Evaluation, feature: FeatureId) -> Option<&FeatureFailure> {
    evaluation.failed.iter().find(|f| f.feature == feature)
}

/// The volume of the example plate.
fn the_plate() -> f64 {
    plate(8.0, 10.0)
}

/// Joined where it went: a block moved flush against another is merged
/// with it by a slab bridging the two, which before the move touched
/// only the first.
#[test]
fn a_moved_block_is_joined_at_its_new_place() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 30.0, 0.0, 40.0, 10.0, "10");
    let moved = shift(editor.document(), &[b], ["-20", "0", "0"], None);
    let id = add(&mut editor, moved);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert!(boxed(
        solid_of(&evaluation, b),
        [10.0, 0.0, 0.0],
        [20.0, 10.0, 10.0]
    ));
    // A slab from x = 5 to 15, joined.
    let up = Extent::OneSide(length(editor.document(), "2"));
    let join = add_extrude(
        &mut editor,
        rectangle((5.0, 2.0), (15.0, 8.0)),
        up,
        Operation::Join(Targets::default()),
    );
    // The slab is drawn on XY, from z = 0 to 2: inside both blocks'
    // bottoms, so it touches both and merges them.
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(evaluation.touched, [(join, vec![a, b])]);
    assert_eq!(evaluation.merged, [(b, a)]);
    assert_near(solid_of(&evaluation, a).volume(), 2000.0);
    // Before the move, the slab touched only the first.
    editor.apply(Command::RemoveFeature(id)).unwrap();
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.touched, [(join, vec![a])]);
    assert!(evaluation.merged.is_empty());
}

/// A quarter turn about a world axis, then a shift, moves every point to
/// the bit; the volume stays.
#[test]
fn quarter_turns_about_world_axes_are_exact() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 10.0, 0.0, 20.0, 5.0, "3");
    let turn = Some((AxisRef::Origin(Axis3::Z), "90"));
    let id = add_move(&mut editor, &[a], ["1", "0", "-3"], turn);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = solid_of(&evaluation, a);
    // (x, y) turns to (−y, x), then shifts.
    assert!(boxed(solid, [-4.0, 10.0, -3.0], [1.0, 20.0, 0.0]));
    assert_near(solid.volume(), 150.0);
    // About X by −90 and 270 alike; Y by 180.
    for (axis, text, min, max) in [
        (Axis3::X, "-90", [10.0, 0.0, -5.0], [20.0, 3.0, 0.0]),
        (Axis3::X, "270", [10.0, 0.0, -5.0], [20.0, 3.0, 0.0]),
        (Axis3::Y, "180", [-20.0, 0.0, -3.0], [-10.0, 5.0, 0.0]),
    ] {
        let turn = Some((AxisRef::Origin(axis), text));
        set_move(&mut editor, id, &[a], ["0", "0", "0"], turn);
        let evaluation = evaluated(editor.document());
        assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
        assert!(boxed(solid_of(&evaluation, a), min, max), "{axis:?} {text}");
    }
    // Any other angle within rounding: 30° about Z keeps the volume.
    let turn = Some((AxisRef::Origin(Axis3::Z), "30"));
    set_move(&mut editor, id, &[a], ["0", "0", "0"], turn);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_near(solid_of(&evaluation, a).volume(), 150.0);
}

/// The example plate turned about its own top front edge: right-handed
/// about the edge's direction, the way it runs with its first face on
/// its left from outside.
#[test]
fn a_turn_about_a_model_edge_follows_its_direction() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let top = key_on(&solid, DVec3::Z, 10.0);
    let front = key_on(&solid, -DVec3::Y, 20.0);
    let edge = EdgeRef {
        body: plate,
        faces: [top.min(front), top.max(front)],
        near: DVec3::new(0.0, -20.0, 10.0),
    };
    let turn = Some((AxisRef::Edge(edge), "90"));
    add_move(&mut editor, &[plate], ["0", "0", "0"], turn);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = solid_of(&evaluation, plate);
    assert_near(solid.volume(), the_plate());
    // The top's boundary runs along +x at the front: about +x the plate
    // stands up behind the edge, about −x it falls in front, below it.
    let along_x = edge.faces[0] == top;
    let (min, max) = if along_x {
        ([-30.0, -20.0, 10.0], [30.0, -10.0, 50.0])
    } else {
        ([-30.0, -30.0, -30.0], [30.0, -20.0, 10.0])
    };
    assert!(boxed(solid, min, max), "{:?}", solid.bounds3());
}

/// A round face's axis, and a round edge's, turn the plate about its
/// hole: a quarter turn swaps its width and depth.
#[test]
fn a_turn_about_a_round_face_or_rim() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let wall = cylinder(&solid);
    let top = key_on(&solid, DVec3::Z, 10.0);
    let face = FaceRef {
        body: plate,
        key: wall,
        near: DVec3::new(8.0, 0.0, 5.0),
    };
    let rim = EdgeRef {
        body: plate,
        faces: [top.min(wall), top.max(wall)],
        near: DVec3::new(8.0, 0.0, 10.0),
    };
    let id = add_move(
        &mut editor,
        &[plate],
        ["0", "0", "0"],
        Some((AxisRef::Face(face), "90")),
    );
    for axis in [AxisRef::Face(face), AxisRef::Edge(rim)] {
        for text in ["90", "-90"] {
            set_move(
                &mut editor,
                id,
                &[plate],
                ["0", "0", "0"],
                Some((axis, text)),
            );
            let evaluation = evaluated(editor.document());
            assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
            let solid = solid_of(&evaluation, plate);
            assert_near(solid.volume(), the_plate());
            assert!(
                near_box(solid, [-20.0, -30.0, 0.0], [20.0, 30.0, 10.0]),
                "{axis:?} {text}: {:?}",
                solid.bounds3()
            );
        }
    }
}

/// Without the original a body is its image, its faces keeping their
/// names (the top still the top, facing up); in an origin plane, exactly.
#[test]
fn a_mirror_without_the_original_replaces_the_body() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 2.0, 0.0, 10.0, 10.0, "10");
    let before = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let mirror = Mirror {
        bodies: vec![a],
        plane: PlaneRef::Origin(OriginPlane::YZ),
        keep_original: false,
    };
    add(&mut editor, mirror);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = solid_of(&evaluation, a);
    assert!(boxed(solid, [-10.0, 0.0, 0.0], [-2.0, 10.0, 10.0]));
    assert_near(solid.volume(), 800.0);
    assert_eq!(
        key_on(solid, DVec3::Z, 10.0),
        key_on(&before, DVec3::Z, 10.0)
    );
    assert_eq!(
        key_on(solid, -DVec3::X, 10.0),
        key_on(&before, DVec3::X, 10.0)
    );
}

/// With the original, a body and its image are one body: side by side
/// when apart, united when they meet; the image's faces named as a copy.
#[test]
fn a_mirror_keeping_the_original_holds_both() {
    for (x0, volume, min) in [
        (5.0, 2000.0, -15.0),
        (0.0, 2000.0, -10.0),
        (-4.0, 1200.0, -6.0),
    ] {
        let mut editor = Editor::new(Document::default());
        let a = block(&mut editor, x0, 0.0, x0 + 10.0, 10.0, "10");
        let mirror = Mirror {
            bodies: vec![a],
            plane: PlaneRef::Origin(OriginPlane::YZ),
            keep_original: true,
        };
        add(&mut editor, mirror);
        let evaluation = evaluated(editor.document());
        assert!(
            evaluation.failed.is_empty(),
            "{x0}: {:?}",
            evaluation.failed
        );
        let solid = solid_of(&evaluation, a);
        assert_near(solid.volume(), volume);
        assert!(
            boxed(solid, [min, 0.0, 0.0], [-min, 10.0, 10.0]),
            "{x0}: {:?}",
            solid.bounds3()
        );
        let copies = (solid.mesh().faces().iter())
            .filter(|face| face.name.instance != 0)
            .count();
        assert!(copies > 0, "{x0}: the image's faces are named as copies");
        assert_eq!(evaluation.bodies.len(), 1);
    }
}

/// A body mirrored in a flat face: its own end, so it doubles; and a
/// block mirrored in a wedge's slanted face, `x + y = 10`, landing where
/// the reflection `(x, y) → (10 − y, 10 − x)` puts it.
#[test]
fn a_planar_face_is_a_mirror_plane() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let end = FaceRef {
        body: a,
        key: key_on(&solid, DVec3::X, 10.0),
        near: DVec3::new(10.0, 5.0, 5.0),
    };
    let doubled = add(
        &mut editor,
        Mirror {
            bodies: vec![a],
            plane: PlaneRef::Face(end),
            keep_original: true,
        },
    );
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = solid_of(&evaluation, a);
    assert_near(solid.volume(), 2000.0);
    assert!(boxed(solid, [0.0, 0.0, 0.0], [20.0, 10.0, 10.0]));
    editor.apply(Command::RemoveFeature(doubled)).unwrap();

    // The wedge, and a block beside it.
    let wedge = add_body(
        &mut editor,
        |sketch: &mut Sketch| {
            let corners = [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]
                .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
            for k in 0..3 {
                let (start, end) = (corners[k], corners[(k + 1) % 3]);
                sketch.add_curve(Curve::Line { start, end }, false).unwrap();
            }
        },
        "10",
    );
    let b = block(&mut editor, 20.0, 0.0, 22.0, 2.0, "2");
    let evaluation = evaluated(editor.document());
    let wedge_solid = solid_of(&evaluation, wedge);
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let slant = FaceRef {
        body: wedge,
        key: key_on(wedge_solid, DVec3::new(half, half, 0.0), 10.0 * half),
        near: DVec3::new(5.0, 5.0, 5.0),
    };
    add(
        &mut editor,
        Mirror {
            bodies: vec![b],
            plane: PlaneRef::Face(slant),
            keep_original: false,
        },
    );
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = solid_of(&evaluation, b);
    assert_near(solid.volume(), 8.0);
    assert!(
        near_box(solid, [8.0, -12.0, 0.0], [10.0, -10.0, 2.0]),
        "{:?}",
        solid.bounds3()
    );
    // Every corner where the reflection puts it.
    let reflect = |p: DVec3| DVec3::new(10.0 - p.y, 10.0 - p.x, p.z);
    for &p in solid.mesh().verts() {
        let back = reflect(p);
        let on_block =
            (20.0 - 1e-9..=22.0 + 1e-9).contains(&back.x) && (-1e-9..=2.0 + 1e-9).contains(&back.y);
        assert!(on_block, "{p} reflects to {back}");
    }
}

/// A round face is no mirror plane, and shows itself; a flat face no
/// axis, and shows itself; a face that isn't there is "not found".
#[test]
fn references_of_the_wrong_shape_fail() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let wall = FaceRef {
        body: plate,
        key: cylinder(&solid),
        near: DVec3::new(8.0, 0.0, 5.0),
    };
    let mirror = add(
        &mut editor,
        Mirror {
            bodies: vec![plate],
            plane: PlaneRef::Face(wall),
            keep_original: false,
        },
    );
    let top = FaceRef {
        body: plate,
        key: key_on(&solid, DVec3::Z, 10.0),
        near: DVec3::new(0.0, 15.0, 10.0),
    };
    let flat = add_move(
        &mut editor,
        &[plate],
        ["1", "0", "0"],
        Some((AxisRef::Face(top), "45")),
    );
    let missing = FaceRef {
        key: FaceKey {
            part: PartKey::Side { curve: 999 },
            ..top.key
        },
        ..top
    };
    let gone = add_move(
        &mut editor,
        &[plate],
        ["1", "0", "0"],
        Some((AxisRef::Face(missing), "45")),
    );
    let evaluation = evaluated(editor.document());
    let mirrored = failure(&evaluation, mirror).unwrap();
    assert_eq!(mirrored.message, "its mirror face isn't flat");
    assert!(mirrored.geometry.is_some());
    let turned = failure(&evaluation, flat).unwrap();
    assert_eq!(turned.message, "its axis face isn't round");
    assert!(turned.geometry.is_some());
    assert_eq!(
        failure(&evaluation, gone).unwrap().message,
        "its axis face wasn't found"
    );
    // None of them changed the plate.
    assert!(boxed(
        solid_of(&evaluation, plate),
        [-30.0, -20.0, 0.0],
        [30.0, 20.0, 10.0]
    ));
}

/// A move or a mirror taking a body past the coordinate limit fails
/// before moving it, changing nothing.
#[test]
fn out_of_range_is_refused() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let far = block(&mut editor, 900_000.0, 0.0, 900_010.0, 10.0, "10");
    let moved = add_move(&mut editor, &[a], ["999995", "0", "0"], None);
    let solid = Arc::clone(&evaluated(editor.document()).bodies[1].solid);
    let face = FaceRef {
        body: far,
        key: key_on(&solid, -DVec3::X, -900_000.0),
        near: DVec3::new(900_000.0, 5.0, 5.0),
    };
    let mirrored = add(
        &mut editor,
        Mirror {
            bodies: vec![a, far],
            plane: PlaneRef::Face(face),
            keep_original: false,
        },
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, moved).unwrap().message,
        "moving Body 1 takes it out of range: every part must stay within 1000000 mm of the \
         origin"
    );
    assert_eq!(
        failure(&evaluation, mirrored).unwrap().message,
        "mirroring Body 1 takes it out of range: every part must stay within 1000000 mm of the \
         origin"
    );
    // Neither body moved, the far one though it alone was in range.
    assert!(boxed(
        solid_of(&evaluation, a),
        [0.0, 0.0, 0.0],
        [10.0, 10.0, 10.0]
    ));
    assert!(boxed(
        solid_of(&evaluation, far),
        [900_000.0, 0.0, 0.0],
        [900_010.0, 10.0, 10.0]
    ));
    // Within range, both go.
    set_move(&mut editor, moved, &[a], ["999990", "0", "0"], None);
    let evaluation = evaluated(editor.document());
    assert!(failure(&evaluation, moved).is_none());
}

/// A body a combine consumed has no solid of its own to move.
#[test]
fn a_consumed_body_fails_a_move() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 5.0, 0.0, 15.0, 10.0, "10");
    add(
        &mut editor,
        Combine {
            target: a,
            tools: vec![b],
            op: BodyOp::Union,
            keep_tools: false,
        },
    );
    let moved = add_move(&mut editor, &[a, b], ["0", "0", "5"], None);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, moved).unwrap().message,
        "Body 2 is in Body 1 now: a feature before this one merged it in"
    );
    assert!(boxed(
        solid_of(&evaluation, a),
        [0.0, 0.0, 0.0],
        [15.0, 10.0, 10.0]
    ));
}

/// A sketch on a face of a moved body is placed where the face went: the
/// face keeps its name.
#[test]
fn a_sketch_on_a_moved_face_follows_it() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let top = FaceRef {
        body: plate,
        key: key_on(&solid, DVec3::Z, 10.0),
        near: DVec3::new(0.0, 15.0, 10.0),
    };
    add_move(&mut editor, &[plate], ["0", "0", "5"], None);
    editor
        .apply(editor.document().add_sketch(Plane::Face(top)))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let (_, placement) = (evaluation.placements.iter())
        .find(|(id, _)| *id == sketch)
        .unwrap();
    assert_eq!(placement.origin, DVec3::new(0.0, 0.0, 15.0));
}

/// A move whose motion is the same (an offset typed another way) finds
/// every body in the cache; another offset moves them again.
#[test]
fn an_unchanged_motion_is_found_in_the_cache() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let turn = Some((AxisRef::Origin(Axis3::Z), "30"));
    let id = add_move(&mut editor, &[plate], ["10", "0", "0"], turn);
    let mut cache = Cache::default();
    cache.begin();
    let first = evaluate(editor.document(), &mut cache);
    let again = |editor: &mut Editor, cache: &mut Cache, x: &str| {
        set(
            editor,
            id,
            shift(editor.document(), &[plate], [x, "0", "0"], turn),
        );
        cache.begin();
        evaluate(editor.document(), cache)
    };
    let same = again(&mut editor, &mut cache, "5 + 5");
    assert!(Arc::ptr_eq(&first.bodies[0].solid, &same.bodies[0].solid));
    let other = again(&mut editor, &mut cache, "11");
    assert!(!Arc::ptr_eq(&first.bodies[0].solid, &other.bodies[0].solid));
    assert_near(other.bodies[0].solid.volume(), the_plate());
}

/// A body mirrored onto itself keeping the original (the example plate
/// is symmetric about YZ and XZ) is itself, or fails: never anything
/// else.
#[test]
fn a_symmetric_body_mirrored_onto_itself_is_itself_or_fails() {
    for plane in [OriginPlane::YZ, OriginPlane::XZ] {
        let mut editor = Editor::new(Document::example());
        let plate = editor.document().bodies()[0].id;
        let mirror = Mirror {
            bodies: vec![plate],
            plane: PlaneRef::Origin(plane),
            keep_original: true,
        };
        let id = add(&mut editor, mirror);
        let evaluation = evaluated(editor.document());
        match failure(&evaluation, id) {
            None => {
                let solid = solid_of(&evaluation, plate);
                assert_near(solid.volume(), the_plate());
                assert!(boxed(solid, [-30.0, -20.0, 0.0], [30.0, 20.0, 10.0]));
            }
            Some(failed) => assert!(
                failed
                    .message
                    .starts_with("joining Body 1 to its mirror image"),
                "{plane:?}: {}",
                failed.message
            ),
        }
        eprintln!(
            "{plane:?}: {:?}",
            failure(&evaluation, id).map(|f| &f.message)
        );
    }
}
