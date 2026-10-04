//! Aligns in the history: a pin aligned by its rim into the example
//! plate's hole and joined (its analytic volume), following the hole
//! when it's moved upstream and back on undo; a block aligned face to
//! face by a corner, with an offset, a turn, a secondary pair and a flip
//! (to the bit, the directions square to the world axes); references
//! found through joins; each refusal; what's noted for the draft; and
//! the cache keyed by the motion.

use glam::DVec3;
use varde_document::{
    Align, AlignRefs, Axis3, AxisRef, BodyOp, Combine, DirRef, EdgeRef, FaceRef, Move, PointRef,
};
use varde_kernel::Budget;
use varde_kernel::mesh::{FaceKey, Form};
use varde_kernel::topology::Topology;

use super::motion::{add, block, boxed, cylinder, failure, key_on, key_where, near_box, set};
use super::*;

fn offset(document: &Document, text: &str) -> Value {
    Value::new(text, &Move::offset_ask(&document.design())).unwrap()
}

fn angle(document: &Document, text: &str) -> Value {
    Value::new(text, &Move::angle_ask(&document.design())).unwrap()
}

/// The edge of `body` (whose solid is `solid`) between the faces of the
/// keys `a` and `b`, sorted.
fn edge(body: BodyId, a: FaceKey, b: FaceKey, near: DVec3) -> EdgeRef {
    EdgeRef {
        body,
        faces: [a.min(b), a.max(b)],
        near,
    }
}

/// The rim of `body`'s cylinder on its flat face `n·x = d`.
fn rim(solid: &Solid, body: BodyId, n: DVec3, d: f64) -> EdgeRef {
    edge(body, key_on(solid, n, d), cylinder(solid), DVec3::ZERO)
}

/// The corner of `body` where its faces on the planes `planes` meet.
fn corner(solid: &Solid, body: BodyId, planes: [(DVec3, f64); 3], near: DVec3) -> PointRef {
    let mut faces = planes.map(|(n, d)| key_on(solid, n, d));
    faces.sort();
    PointRef::Corner { body, faces, near }
}

/// The flat face of `body` on `n·x = d`, its normal.
fn normal(solid: &Solid, body: BodyId, n: DVec3, d: f64, near: DVec3) -> DirRef {
    DirRef::Normal(FaceRef {
        body,
        key: key_on(solid, n, d),
        near,
    })
}

/// A side of an align: a point, and a primary and secondary if given.
fn refs(point: PointRef, primary: Option<DirRef>, secondary: Option<DirRef>) -> AlignRefs {
    AlignRefs {
        point,
        primary,
        secondary,
    }
}

/// The volume and centre of mass of `solid`.
fn mass(solid: &Solid) -> (f64, DVec3) {
    let moments = solid.moments(&Budget::DEFAULT).unwrap();
    (moments.volume, moments.centre.unwrap())
}

/// The example plate, and a pin of the hole's radius standing 25 tall at
/// x = 100: the editor, the plate, the pin, and the align of the pin's
/// bottom rim into the hole's top rim, 10 down (so it fills the hole and
/// stands 15 above the plate).
fn pin_in_hole() -> (Editor, BodyId, BodyId, Align) {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let pin = add_body(&mut editor, disc((100.0, 0.0), 8.0), "25");
    let evaluation = evaluated(editor.document());
    let (plate_solid, pin_solid) = (solid_of(&evaluation, plate), solid_of(&evaluation, pin));
    let foot = rim(pin_solid, pin, DVec3::NEG_Z, 0.0);
    let hole = rim(plate_solid, plate, DVec3::Z, 10.0);
    let align = Align {
        body: pin,
        from: refs(
            PointRef::Centre(foot),
            Some(DirRef::Axis(AxisRef::Edge(foot))),
            None,
        ),
        to: refs(
            PointRef::Centre(hole),
            Some(DirRef::Axis(AxisRef::Edge(hole))),
            None,
        ),
        flip: false,
        offset: Some(offset(editor.document(), "-10")),
        turn: None,
    };
    (editor, plate, pin, align)
}

/// A pin aligned by its rim drops into the hole (rims meet opposed by
/// default, as faces do), and joined to the plate fills it: the analytic
/// volume. Moving the hole upstream takes the pin with it; undo brings
/// both back.
#[test]
fn a_pin_aligned_by_its_rim_fills_the_hole() {
    let (mut editor, plate, pin, align) = pin_in_hole();
    let id = add(&mut editor, align);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let (volume, centre) = mass(solid_of(&evaluation, pin));
    let pin_volume = PI * 64.0 * 25.0;
    assert_near(volume, pin_volume);
    assert!(
        centre.distance(DVec3::new(0.0, 0.0, 12.5)) < 1e-9,
        "{centre}"
    );
    // What was found, for the draft: the rims' centres and axes.
    let [(found, datums)] = &evaluation.aligned[..] else {
        panic!("{:?}", evaluation.aligned);
    };
    assert_eq!(*found, id);
    assert!(datums.opposed);
    let moved = datums.moved.unwrap();
    let target = datums.target.unwrap();
    assert!(DVec3::from(moved.point).distance(DVec3::new(100.0, 0.0, 0.0)) < 1e-12);
    assert!(DVec3::from(target.point).distance(DVec3::new(0.0, 0.0, 10.0)) < 1e-12);
    assert_eq!(moved.primary, Some([0.0, 0.0, -1.0]));
    assert_eq!(target.primary, Some([0.0, 0.0, 1.0]));
    // Joined.
    let combine = Combine {
        target: plate,
        tools: vec![pin],
        op: BodyOp::Union,
        keep_tools: false,
    };
    add(&mut editor, combine);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let joined = 60.0 * 40.0 * 10.0 + PI * 64.0 * 15.0;
    assert_near(solid_of(&evaluation, plate).volume(), joined);
    // The hole moved upstream: the pin follows, and the join still fills
    // it.
    edit_sketch(&mut editor, |sketch| {
        let Curve::Circle { center, .. } = sketch.curve(circle(sketch)).unwrap().curve else {
            unreachable!()
        };
        sketch.point_mut(center).unwrap().at = DVec2::new(12.5, -4.0);
    });
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_near(solid_of(&evaluation, plate).volume(), joined);
    let (_, centre) = mass(solid_of(&evaluation, plate));
    let expected = (DVec3::new(0.0, 0.0, 5.0) * 24000.0
        + DVec3::new(12.5, -4.0, 17.5) * (PI * 64.0 * 15.0))
        / joined;
    assert!(centre.distance(expected) < 1e-9, "{centre} {expected}");
    // Undone, the hole and the join go back.
    editor.undo();
    let undone = evaluated(editor.document());
    let (_, centre) = mass(solid_of(&undone, plate));
    let expected = (DVec3::new(0.0, 0.0, 5.0) * 24000.0
        + DVec3::new(0.0, 0.0, 17.5) * (PI * 64.0 * 15.0))
        / joined;
    assert!(centre.distance(expected) < 1e-9, "{centre} {expected}");
    editor.undo();
    let undone = evaluated(editor.document());
    let (_, centre) = mass(solid_of(&undone, pin));
    assert!(
        centre.distance(DVec3::new(0.0, 0.0, 12.5)) < 1e-9,
        "{centre}"
    );
}

/// The hole's rim found after the plate merged another body: a block
/// joined onto the plate's far end keeps the rim's names, and the pin
/// aligned after it still goes into the hole.
#[test]
fn references_are_found_through_a_join() {
    let (mut editor, plate, pin, align) = pin_in_hole();
    let block = block(&mut editor, 25.0, -5.0, 45.0, 5.0, "10");
    let up = Extent::OneSide(length(editor.document(), "2"));
    add_extrude(
        &mut editor,
        rectangle((20.0, -3.0), (35.0, 3.0)),
        up,
        Operation::Join(Targets::default()),
    );
    let id = add(&mut editor, align);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(evaluation.merged, [(block, plate)]);
    let (_, centre) = mass(solid_of(&evaluation, pin));
    assert!(
        centre.distance(DVec3::new(0.0, 0.0, 12.5)) < 1e-9,
        "{centre}"
    );
    // Aligning the plate (which now holds the block) onto a reference on
    // the block is aligning it onto itself: refused.
    let FeatureKind::Align(mut onto_block) = editor.document().feature(id).unwrap().kind.clone()
    else {
        unreachable!()
    };
    let block_solid = solid_of(&evaluation, plate);
    std::mem::swap(&mut onto_block.from, &mut onto_block.to);
    onto_block.body = plate;
    onto_block.offset = None;
    onto_block.to = refs(
        corner(
            block_solid,
            block,
            [(DVec3::X, 45.0), (DVec3::Y, 5.0), (DVec3::Z, 10.0)],
            DVec3::new(45.0, 5.0, 10.0),
        ),
        Some(normal(
            block_solid,
            block,
            DVec3::Z,
            10.0,
            DVec3::new(40.0, 0.0, 10.0),
        )),
        None,
    );
    let other = add(&mut editor, *onto_block);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, other).unwrap().message,
        "its point on the target is in the moved body now: a feature before this one merged \
         them; pick it on another body"
    );
}

/// The second block, from (50, 50) to (60, 70) and 5 tall, by its corner
/// at (50, 50, 0) and bottom onto the first's top corner at (0, 0, 10):
/// face to face, the directions square to the world axes, so every move
/// is exact to the bit; then with an offset, a turn, a secondary pair,
/// and flipped.
#[test]
fn a_block_aligned_face_to_face_is_exact() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 50.0, 50.0, 60.0, 70.0, "5");
    let evaluation = evaluated(editor.document());
    let (sa, sb) = (solid_of(&evaluation, a), solid_of(&evaluation, b));
    let from_corner = corner(
        sb,
        b,
        [
            (DVec3::NEG_Z, 0.0),
            (DVec3::NEG_X, -50.0),
            (DVec3::NEG_Y, -50.0),
        ],
        DVec3::new(50.0, 50.0, 0.0),
    );
    let to_corner = corner(
        sa,
        a,
        [(DVec3::Z, 10.0), (DVec3::NEG_X, 0.0), (DVec3::NEG_Y, 0.0)],
        DVec3::new(0.0, 0.0, 10.0),
    );
    let bottom = normal(sb, b, DVec3::NEG_Z, 0.0, DVec3::new(55.0, 60.0, 0.0));
    let top = normal(sa, a, DVec3::Z, 10.0, DVec3::new(5.0, 5.0, 10.0));
    let align = Align {
        body: b,
        from: refs(from_corner, Some(bottom), None),
        to: refs(to_corner, Some(top), None),
        flip: false,
        offset: None,
        turn: None,
    };
    let id = add(&mut editor, align.clone());
    let check = |editor: &Editor, min: [f64; 3], max: [f64; 3]| {
        let evaluation = evaluated(editor.document());
        assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
        let solid = solid_of(&evaluation, b);
        assert!(boxed(solid, min, max), "{:?}", solid.bounds3());
        assert_near(solid.volume(), 10.0 * 20.0 * 5.0);
        assert!(boxed(
            solid_of(&evaluation, a),
            [0.0, 0.0, 0.0],
            [10.0, 10.0, 10.0]
        ));
    };
    check(&editor, [0.0, 0.0, 10.0], [10.0, 20.0, 15.0]);
    let document = editor.document().clone();
    let with = |change: &dyn Fn(&mut Align)| {
        let mut changed = align.clone();
        change(&mut changed);
        changed
    };
    set(
        &mut editor,
        id,
        with(&|a| a.offset = Some(offset(&document, "2"))),
    );
    check(&editor, [0.0, 0.0, 12.0], [10.0, 20.0, 17.0]);
    // Turned a quarter about the top's normal, through the corner.
    set(
        &mut editor,
        id,
        with(&|a| {
            a.offset = Some(offset(&document, "2"));
            a.turn = Some(angle(&document, "90"));
        }),
    );
    check(&editor, [-20.0, 0.0, 12.0], [0.0, 10.0, 17.0]);
    // A secondary pair: the block's −X side onto the first's −Y side
    // turns it the same quarter.
    let x_side = normal(sb, b, DVec3::NEG_X, -50.0, DVec3::new(50.0, 60.0, 2.0));
    let y_side = normal(sa, a, DVec3::NEG_Y, 0.0, DVec3::new(5.0, 0.0, 5.0));
    set(
        &mut editor,
        id,
        with(&|a| {
            a.from.secondary = Some(x_side);
            a.to.secondary = Some(y_side);
        }),
    );
    check(&editor, [-20.0, 0.0, 10.0], [0.0, 10.0, 15.0]);
    // Flipped, the bottom goes the top's way: a half turn about X.
    set(&mut editor, id, with(&|a| a.flip = true));
    check(&editor, [0.0, -20.0, 5.0], [10.0, 0.0, 10.0]);
    // The target's secondary parallel to its primary: refused, showing
    // both faces.
    let underside = normal(sa, a, DVec3::NEG_Z, 0.0, DVec3::new(5.0, 5.0, 0.0));
    set(
        &mut editor,
        id,
        with(&|a| {
            a.from.secondary = Some(x_side);
            a.to.secondary = Some(underside);
        }),
    );
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).unwrap();
    assert_eq!(
        failed.message,
        "its second direction on the target is parallel to its first: pick one across it"
    );
    assert!(failed.geometry.is_some());
    // A point alone: a move, the block's corner onto the origin.
    set(
        &mut editor,
        id,
        with(&|a| {
            a.from.primary = None;
            a.to = refs(PointRef::Origin, None, None);
        }),
    );
    check(&editor, [0.0, 0.0, 0.0], [10.0, 20.0, 5.0]);
}

/// A motion the same as before (an offset typed another way) finds the
/// moved body in the cache; another moves it again.
#[test]
fn an_unchanged_align_is_found_in_the_cache() {
    let (mut editor, _, pin, align) = pin_in_hole();
    let id = add(&mut editor, align.clone());
    let mut cache = Cache::default();
    cache.begin();
    let first = evaluate(editor.document(), &mut cache);
    let again = |editor: &mut Editor, cache: &mut Cache, text: &str| {
        let mut changed = align.clone();
        changed.offset = Some(offset(editor.document(), text));
        set(editor, id, changed);
        cache.begin();
        evaluate(editor.document(), cache)
    };
    let held = |evaluation: &Evaluation| {
        let made = evaluation.bodies.iter().find(|made| made.body == pin);
        Arc::clone(&made.unwrap().solid)
    };
    let same = again(&mut editor, &mut cache, "-5 - 5");
    assert!(Arc::ptr_eq(&held(&first), &held(&same)));
    let other = again(&mut editor, &mut cache, "-9");
    assert!(!Arc::ptr_eq(&held(&first), &held(&other)));
}

/// Each way an align fails, said with the reference it's about and,
/// where it's of the wrong kind or parallel, what it found; a failing
/// align changes no body.
#[test]
fn refusals() {
    let (mut editor, plate, pin, align) = pin_in_hole();
    let unmoved = evaluated(editor.document());
    let (plate_solid, pin_solid) = (
        Arc::clone(&unmoved.bodies[0].solid),
        solid_of(&unmoved, pin).clone(),
    );
    let id = add(&mut editor, align.clone());
    let fails = |editor: &mut Editor, changed: Align, message: &str, shown: bool| {
        set(editor, id, changed);
        let evaluation = evaluated(editor.document());
        let failed = failure(&evaluation, id).unwrap_or_else(|| panic!("{message}"));
        assert_eq!(failed.message, message);
        assert_eq!(failed.geometry.is_some(), shown, "{message}");
        let [min, max] = unmoved_box(&unmoved, pin);
        assert!(
            boxed(solid_of(&evaluation, pin), min, max),
            "{message}: {:?} not {min:?} {max:?}",
            solid_of(&evaluation, pin).bounds3()
        );
    };
    let wall = cylinder(&plate_solid);
    // A face of the wrong kind: the hole's wall as a normal.
    let mut changed = align.clone();
    changed.to.primary = Some(DirRef::Normal(FaceRef {
        body: plate,
        key: wall,
        near: DVec3::new(8.0, 0.0, 5.0),
    }));
    fails(
        &mut editor,
        changed,
        "its first direction on the target is a face that isn't flat",
        true,
    );
    // The plate's top as an axis.
    let top = key_on(&plate_solid, DVec3::Z, 10.0);
    let mut changed = align.clone();
    changed.to.primary = Some(DirRef::Axis(AxisRef::Face(FaceRef {
        body: plate,
        key: top,
        near: DVec3::new(20.0, 0.0, 10.0),
    })));
    fails(
        &mut editor,
        changed,
        "its first direction on the target is a face that isn't round",
        true,
    );
    // A round edge's middle.
    let mut changed = align.clone();
    let PointRef::Centre(foot) = align.from.point else {
        unreachable!()
    };
    changed.from.point = PointRef::Middle(foot);
    fails(
        &mut editor,
        changed,
        "its point on the moved body is an edge that isn't straight",
        true,
    );
    // A reference gone: a side the pin's extrude never made.
    let mut changed = align.clone();
    let mut gone = foot;
    gone.faces[1].part = varde_kernel::mesh::PartKey::Side { curve: 999 };
    gone.faces.sort();
    changed.from.primary = Some(DirRef::Axis(AxisRef::Edge(gone)));
    fails(
        &mut editor,
        changed,
        "its first direction on the moved body wasn't found",
        false,
    );
    // A corner that isn't there.
    let mut changed = align.clone();
    let pin_bottom = key_on(&pin_solid, DVec3::NEG_Z, 0.0);
    let pin_top = key_on(&pin_solid, DVec3::Z, 25.0);
    let mut faces = [pin_bottom, pin_top, cylinder(&pin_solid)];
    faces.sort();
    changed.from.point = PointRef::Corner {
        body: pin,
        faces,
        near: DVec3::ZERO,
    };
    fails(
        &mut editor,
        changed,
        "its point on the moved body wasn't found",
        false,
    );
    // A secondary parallel to its primary: the pin's top normal against
    // its bottom rim's axis; the target's plate top across its hole's.
    let mut changed = align.clone();
    changed.from.secondary = Some(normal(
        &pin_solid,
        pin,
        DVec3::Z,
        25.0,
        DVec3::new(100.0, 0.0, 25.0),
    ));
    changed.to.secondary = Some(DirRef::Origin(Axis3::X));
    fails(
        &mut editor,
        changed,
        "its second direction on the moved body is parallel to its first: pick one across it",
        true,
    );
    // Out of range.
    let mut changed = align.clone();
    changed.offset = Some(offset(editor.document(), "1e6"));
    fails(
        &mut editor,
        changed,
        "aligning Body 2 takes it out of range: every part must stay within 1000000 mm of \
         the origin",
        false,
    );
    // The target's body gone.
    set(&mut editor, id, align);
    editor.apply(Command::RemoveBody(plate)).unwrap();
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "its point on the target is on a body that's gone"
    );
}

/// The box of `body`'s solid in `evaluation`.
fn unmoved_box(evaluation: &Evaluation, body: BodyId) -> [[f64; 3]; 2] {
    let bounds = solid_of(evaluation, body).bounds3().unwrap();
    [bounds.min.to_array(), bounds.max.to_array()]
}

/// A body a join or combine consumed can't be aligned.
#[test]
fn a_consumed_body_fails_an_align() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 5.0, 0.0, 15.0, 10.0, "10");
    let evaluation = evaluated(editor.document());
    let sb = solid_of(&evaluation, b).clone();
    add(
        &mut editor,
        Combine {
            target: a,
            tools: vec![b],
            op: BodyOp::Union,
            keep_tools: false,
        },
    );
    let align = Align {
        body: b,
        from: refs(
            corner(
                &sb,
                b,
                [
                    (DVec3::NEG_Z, 0.0),
                    (DVec3::NEG_X, -5.0),
                    (DVec3::NEG_Y, 0.0),
                ],
                DVec3::new(5.0, 0.0, 0.0),
            ),
            None,
            None,
        ),
        to: refs(PointRef::Origin, None, None),
        flip: false,
        offset: None,
        turn: None,
    };
    let id = add(&mut editor, align);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "Body 2 is in Body 1 now: a feature before this one merged it in"
    );
    assert!(evaluation.aligned.is_empty());
}

/// The datums the kernel resolves are what the align noted, on the
/// bodies as the features before it leave them.
#[test]
fn what_is_noted_is_what_the_topology_gives() {
    let (mut editor, plate, pin, align) = pin_in_hole();
    let before = evaluated(editor.document());
    let id = add(&mut editor, align.clone());
    let evaluation = evaluated(editor.document());
    let (_, datums) = (evaluation.aligned.iter())
        .find(|(feature, _)| *feature == id)
        .unwrap();
    // The moved side where it was before the align.
    let moved = solid_of(&before, pin);
    let PointRef::Centre(foot) = align.from.point else {
        unreachable!()
    };
    let topology: Topology = moved.topology();
    let centre = topology.centre(moved, foot.faces, foot.near).unwrap();
    assert_eq!(datums.moved.unwrap().point, centre.to_array());
    let down = topology
        .edge_direction(moved, foot.faces, foot.near)
        .unwrap();
    assert_eq!(datums.moved.unwrap().primary, Some(down.to_array()));
    let solid = solid_of(&before, plate);
    let topology: Topology = solid.topology();
    let PointRef::Centre(hole) = align.to.point else {
        unreachable!()
    };
    let centre = topology.centre(solid, hole.faces, hole.near).unwrap();
    assert_eq!(datums.target.unwrap().point, centre.to_array());
    let wall = topology.face(solid, &cylinder(solid), DVec3::ZERO).unwrap();
    assert!(matches!(topology.form(solid, wall), Form::Cylinder { .. }));
}

/// An align being set up answers with what it found on each side, and
/// still does when it fails.
#[test]
fn a_draft_answers_with_its_datums() {
    use crate::tests::{answered, regenerate_with};
    let (editor, _, _, align) = pin_in_hole();
    let mut regenerator = crate::Regenerator::default();
    let mut draft = crate::Draft {
        revision: 1,
        feature: None,
        kind: align.clone().into(),
    };
    let first = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
    let drafted = first.draft.unwrap();
    assert_eq!(drafted.error, None);
    let datums = drafted.datums.unwrap();
    assert!(datums.opposed);
    assert_eq!(datums.target.unwrap().primary, Some([0.0, 0.0, 1.0]));
    // Flipped and out of range: it fails, and still says what it found.
    let mut far = align;
    far.flip = true;
    far.offset = Some(offset(editor.document(), "999999"));
    draft.kind = far.into();
    draft.revision = 2;
    let second = answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
    let drafted = second.draft.unwrap();
    assert!(drafted.error.unwrap().contains("out of range"));
    let datums = drafted.datums.unwrap();
    assert!(!datums.opposed);
    assert!(datums.moved.is_some());
}

/// The example plate (Body 1), then discs of radius 5 standing 15 tall
/// at (20, 0) and (−20, 0) (Bodies 2 and 3), the first combined into the
/// plate, and the plate aligned by that disc's top rim (named on the
/// plate, which holds it at the align) onto the other disc's, face to
/// face: the editor, the bodies, the combine and the align's id.
fn aligned_by_what_was_merged() -> (Editor, [BodyId; 3], FeatureId, FeatureId) {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let right = add_body(&mut editor, disc((20.0, 0.0), 5.0), "15");
    let left = add_body(&mut editor, disc((-20.0, 0.0), 5.0), "15");
    let evaluation = evaluated(editor.document());
    let (right_solid, left_solid) = (solid_of(&evaluation, right), solid_of(&evaluation, left));
    let combine = Combine {
        target: plate,
        tools: vec![right],
        op: BodyOp::Union,
        keep_tools: false,
    };
    let combine = add(&mut editor, combine);
    let from = rim(right_solid, plate, DVec3::Z, 15.0);
    let to = rim(left_solid, left, DVec3::Z, 15.0);
    let axis = |edge| Some(DirRef::Axis(AxisRef::Edge(edge)));
    let align = Align {
        body: plate,
        from: refs(PointRef::Centre(from), axis(from), None),
        to: refs(PointRef::Centre(to), axis(to), None),
        flip: false,
        offset: None,
        turn: None,
    };
    let id = add(&mut editor, align);
    (editor, [plate, right, left], combine, id)
}

/// References on the moved side picked on what a combine merged into the
/// moved body are named on it: aligned, the plate turns over onto the
/// other disc (`x − 40, −y, 30 − z`). The combine edited so the merge
/// goes (its tool kept apart, its tool another body, the combine gone)
/// or kept with its tool: either the same place, or not found (or on the
/// moved body) and nothing moved, never placed by something else.
#[test]
fn references_on_the_holder_after_the_merge_goes() {
    let (mut editor, [plate, right, left], combine, id) = aligned_by_what_was_merged();
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let moved = solid_of(&evaluation, plate);
    assert!(
        near_box(moved, [-70.0, -20.0, 15.0], [-10.0, 20.0, 30.0]),
        "{:?}",
        moved.bounds3()
    );
    let placed = moved.clone();
    // Fails, the plate as the history before the align leaves it.
    let unmoved = |editor: &Editor, why: &str| {
        let evaluation = evaluated(editor.document());
        assert_eq!(failure(&evaluation, id).unwrap().message, why);
        let mut without = editor.clone();
        without.apply(Command::RemoveFeature(id)).unwrap();
        let before = evaluated(without.document());
        assert_eq!(solid_of(&evaluation, plate), solid_of(&before, plate));
    };
    let not_found = "its point on the moved body wasn't found";
    let combined = |tools: Vec<BodyId>, keep_tools: bool| Combine {
        target: plate,
        tools,
        op: BodyOp::Union,
        keep_tools,
    };
    // Its tool kept as a body of its own: the plate still holds a copy of
    // it, by its names, so the align is the same.
    set(&mut editor, combine, combined(vec![right], true));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(*solid_of(&evaluation, plate), placed);
    // The other disc combined instead: the right one's rim isn't on the
    // plate, and the target is in it.
    set(&mut editor, combine, combined(vec![left], false));
    unmoved(&editor, not_found);
    // Both: the target is in the moved body.
    set(&mut editor, combine, combined(vec![right, left], false));
    unmoved(
        &editor,
        "its point on the target is in the moved body now: a feature before this one merged \
         them; pick it on another body",
    );
    // The combine gone.
    editor.apply(Command::RemoveFeature(combine)).unwrap();
    assert!(editor.document().feature(id).is_some());
    unmoved(&editor, not_found);
    // Undone back to the merge: the same place again.
    for _ in 0..4 {
        editor.undo();
    }
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(*solid_of(&evaluation, plate), placed);
}

/// The same through a join: a block joined to the plate by a bridge,
/// the plate aligned by the block's top corner (named on the plate) to
/// the origin; the block taken out of the join, so the merge goes, its
/// corner isn't found on the plate and nothing moves.
#[test]
fn references_on_the_holder_after_a_join_lets_go() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let block = block(&mut editor, 40.0, -5.0, 50.0, 5.0, "10");
    let evaluation = evaluated(editor.document());
    let block_solid = solid_of(&evaluation, block).clone();
    let up = Extent::OneSide(length(editor.document(), "2"));
    let join = add_extrude(
        &mut editor,
        rectangle((20.0, -3.0), (45.0, 3.0)),
        up,
        Operation::Join(Targets::default()),
    );
    let corner = corner(
        &block_solid,
        plate,
        [(DVec3::X, 50.0), (DVec3::Y, 5.0), (DVec3::Z, 10.0)],
        DVec3::new(50.0, 5.0, 10.0),
    );
    let align = Align {
        body: plate,
        from: refs(corner, None, None),
        to: refs(PointRef::Origin, None, None),
        flip: false,
        offset: None,
        turn: None,
    };
    let id = add(&mut editor, align);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(evaluation.merged, [(block, plate)]);
    let moved = solid_of(&evaluation, plate);
    assert!(
        near_box(moved, [-80.0, -25.0, -10.0], [0.0, 15.0, 0.0]),
        "{:?}",
        moved.bounds3()
    );
    set_extrude(&mut editor, join, |extrude| {
        extrude.operation = Operation::Join(Targets {
            excluded: vec![block],
        });
    });
    let evaluation = evaluated(editor.document());
    assert!(evaluation.merged.is_empty());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "its point on the moved body wasn't found"
    );
    let mut without = editor.clone();
    without.apply(Command::RemoveFeature(id)).unwrap();
    let before = evaluated(without.document());
    assert_eq!(solid_of(&evaluation, plate), solid_of(&before, plate));
}

/// A block 10 wide whose top is a shallow arc about a centre `reach`
/// below it, aligned by that arc's centre (and its axis, the top's
/// normal) to the origin (and Z, the same way): a nearly straight arc's
/// centre far out. Where the centre is past the coordinate limit it's
/// refused as too far out; where it's within but the body would go past,
/// as out of range; else the body goes where the centre the sketch drew
/// takes it, never elsewhere. (A sketch holds points within the
/// coordinate limit, so the centre drawn is; the one found from the
/// curve could only be past it by its rounding.)
#[test]
fn far_arc_centres_are_aligned_or_refused() {
    for reach in [50.0, 1e3, 1e5, 9e5, 999_990.0, 1_000_010.0] {
        let mut editor = Editor::new(Document::default());
        let top = 10.0;
        let extent = Extent::OneSide(length(editor.document(), "5"));
        add_extrude(
            &mut editor,
            domed((0.0, 0.0), (10.0, top), reach),
            extent,
            Operation::NewBody(BodyId::NEW),
        );
        let body = editor.document().bodies()[0].id;
        let evaluation = evaluated(editor.document());
        assert!(
            evaluation.failed.is_empty(),
            "{reach}: {:?}",
            evaluation.failed
        );
        let made = solid_of(&evaluation, body).clone();
        let wall = key_where(&made, |form| matches!(form, Form::Cylinder { .. }));
        let cap = key_on(&made, DVec3::Z, 5.0);
        let arc = edge(body, wall, cap, DVec3::new(5.0, top, 5.0));
        for primaries in [false, true] {
            let axis = |d| primaries.then_some(d);
            let align = Align {
                body,
                from: refs(
                    PointRef::Centre(arc),
                    axis(DirRef::Axis(AxisRef::Edge(arc))),
                    None,
                ),
                to: refs(PointRef::Origin, axis(DirRef::Origin(Axis3::Z)), None),
                flip: false,
                offset: None,
                turn: None,
            };
            let mut aligned = editor.clone();
            let id = add(&mut aligned, align);
            let evaluation = evaluated(aligned.document());
            let centre = DVec3::new(5.0, top - reach, 5.0);
            match failure(&evaluation, id) {
                None => {
                    // Moved by what takes the centre to the origin (the
                    // top's normal is Z already: no turn).
                    let solid = solid_of(&evaluation, body);
                    let [low, high] = [made.bounds3().unwrap().min, made.bounds3().unwrap().max];
                    let now = solid.bounds3().unwrap();
                    let slack = 1e-9 * reach.max(1.0);
                    assert!(
                        (now.min - (low - centre)).abs().max_element() < slack
                            && (now.max - (high - centre)).abs().max_element() < slack,
                        "{reach} {primaries}: {now:?} not {:?} less {centre}",
                        [low, high]
                    );
                }
                Some(failed) => {
                    let far = "its point on the moved body is too far out to align by";
                    let out = failed
                        .message
                        .starts_with("aligning Body 1 takes it out of range");
                    assert!(
                        (failed.message == far && reach > 999_990.0) || (out && reach > 9e5),
                        "{reach} {primaries}: {}",
                        failed.message
                    );
                    let before = evaluated(editor.document());
                    assert_eq!(solid_of(&evaluation, body), solid_of(&before, body));
                }
            }
        }
    }
}
