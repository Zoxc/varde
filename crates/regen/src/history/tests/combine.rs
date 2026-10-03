//! Combines in the history: union, subtract and intersect of two and of
//! several bodies against their exact volumes, tools consumed or kept, a
//! union's tools tried again once another bridged them, results that
//! would leave nothing, bodies with no solid of their own, later features
//! on consumed bodies following the target, what's cached, and where a
//! failing step shows its faces.

use glam::DVec3;
use varde_document::{BodyOp, Combine, FaceRef};
use varde_kernel::mesh::{FaceKey, PartKey};

use super::*;
use crate::Draft;
use crate::tests::{answered, regenerate_with};

/// Adds a combine as one edit: its id.
fn add_combine(editor: &mut Editor, combine: Combine) -> FeatureId {
    editor
        .apply(editor.document().add_feature(combine.into()))
        .unwrap();
    editor.document().features().last().unwrap().id
}

fn combine(target: BodyId, tools: &[BodyId], op: BodyOp) -> Combine {
    Combine {
        target,
        tools: tools.to_vec(),
        op,
        keep_tools: false,
    }
}

/// Adds the block from `(x0, y0)` to `(x1, y1)`, `height` up from XY: its
/// body.
fn block(editor: &mut Editor, x0: f64, y0: f64, x1: f64, y1: f64, height: &str) -> BodyId {
    add_body(editor, rectangle((x0, y0), (x1, y1)), height)
}

/// The bodies in `evaluation` that have a solid, in order.
fn made(evaluation: &Evaluation) -> Vec<BodyId> {
    evaluation.bodies.iter().map(|made| made.body).collect()
}

/// What `evaluation` says of `feature`'s failing, if it failed.
fn failure(evaluation: &Evaluation, feature: FeatureId) -> Option<&str> {
    (evaluation.failed.iter())
        .find(|f| f.feature == feature)
        .map(|f| f.message.as_str())
}

#[test]
fn a_union_of_two_overlapping_blocks_consumes_the_tool() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 5.0, 0.0, 15.0, 10.0, "10");
    add_combine(&mut editor, combine(a, &[b], BodyOp::Union));
    let evaluation = evaluated(editor.document());
    assert_near(only_body(&evaluation).volume(), 15.0 * 10.0 * 10.0);
    assert_eq!(made(&evaluation), [a]);
    assert_eq!(evaluation.merged, [(b, a)]);
    assert_eq!(evaluation.holder(b), Some(a));
    // A combine touches nothing an extrude's panel would list.
    assert!(evaluation.touched.is_empty());
}

#[test]
fn kept_tools_stay_as_they_were() {
    for op in [BodyOp::Union, BodyOp::Subtract, BodyOp::Intersect] {
        let mut editor = Editor::new(Document::default());
        let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
        let b = block(&mut editor, 5.0, 0.0, 15.0, 10.0, "10");
        let kept = Combine {
            keep_tools: true,
            ..combine(a, &[b], op)
        };
        add_combine(&mut editor, kept);
        let evaluation = evaluated(editor.document());
        assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
        assert_eq!(made(&evaluation), [a, b]);
        assert!(evaluation.merged.is_empty());
        let target = match op {
            BodyOp::Union => 1500.0,
            BodyOp::Subtract | BodyOp::Intersect => 500.0,
        };
        assert_near(solid_of(&evaluation, a).volume(), target);
        assert_near(solid_of(&evaluation, b).volume(), 1000.0);
    }
}

/// Three tools cut from a bar: two notching its ends, one clear of it
/// (cutting nothing), all consumed.
#[test]
fn several_tools_are_subtracted_in_turn() {
    let mut editor = Editor::new(Document::default());
    let bar = block(&mut editor, 0.0, 0.0, 30.0, 10.0, "10");
    let left = block(&mut editor, -5.0, -5.0, 5.0, 15.0, "20");
    let right = block(&mut editor, 25.0, -5.0, 35.0, 15.0, "20");
    let clear = block(&mut editor, 50.0, 0.0, 60.0, 10.0, "10");
    add_combine(
        &mut editor,
        combine(bar, &[left, right, clear], BodyOp::Subtract),
    );
    let evaluation = evaluated(editor.document());
    assert_near(only_body(&evaluation).volume(), 3000.0 - 2.0 * 500.0);
    let bounds = only_body(&evaluation).bounds3().unwrap();
    assert_eq!((bounds.min.x, bounds.max.x), (5.0, 25.0));
    assert_eq!(evaluation.merged, [(left, bar), (right, bar), (clear, bar)]);
}

#[test]
fn several_tools_are_intersected_in_turn() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 5.0, 0.0, 15.0, 10.0, "10");
    let c = block(&mut editor, 0.0, 5.0, 10.0, 15.0, "10");
    add_combine(&mut editor, combine(a, &[b, c], BodyOp::Intersect));
    let evaluation = evaluated(editor.document());
    let solid = only_body(&evaluation);
    assert_near(solid.volume(), 5.0 * 5.0 * 10.0);
    let bounds = solid.bounds3().unwrap();
    assert_eq!(bounds.min, DVec3::new(5.0, 5.0, 0.0));
    assert_eq!(bounds.max, DVec3::new(10.0, 10.0, 10.0));
    assert_eq!(evaluation.merged, [(b, a), (c, a)]);
}

/// Three blocks in a row, a union of all: one body, every block once.
#[test]
fn several_tools_are_united_into_the_target() {
    let mut editor = Editor::new(Document::default());
    let blocks = [0.0, 8.0, 16.0].map(|x| block(&mut editor, x, 0.0, x + 10.0, 10.0, "10"));
    let [a, b, c] = blocks;
    add_combine(&mut editor, combine(b, &[a, c], BodyOp::Union));
    let evaluation = evaluated(editor.document());
    assert_near(only_body(&evaluation).volume(), 26.0 * 10.0 * 10.0);
    // The target holds them, made first or not.
    assert_eq!(made(&evaluation), [b]);
    assert_eq!(evaluation.merged, [(a, b), (c, b)]);
}

/// The example plate and a plate below it, flush at z = 0: a flush union
/// and a flush subtract (nothing to take).
#[test]
fn flush_bodies_combine() {
    for op in [BodyOp::Union, BodyOp::Subtract] {
        let mut editor = Editor::new(Document::example());
        let top = editor.document().bodies()[0].id;
        let below = plate_below(&mut editor);
        add_combine(&mut editor, combine(top, &[below], op));
        let evaluation = evaluated(editor.document());
        let volume = match op {
            BodyOp::Union => plate(8.0, 10.0) + plate(8.0, 3.0),
            _ => plate(8.0, 10.0),
        };
        assert_near(only_body(&evaluation).volume(), volume);
        assert_eq!(evaluation.merged, [(below, top)]);
    }
}

/// A tool meeting the target only along an edge makes no clean solid
/// with it, but does once a later tool has bridged them: the union tries
/// it again at the end.
#[test]
fn a_union_tries_a_tool_again_once_another_bridges_it() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let edge = block(&mut editor, 10.0, 10.0, 20.0, 20.0, "10");
    let bridge = block(&mut editor, 5.0, 5.0, 15.0, 15.0, "10");
    let before = evaluated(editor.document());
    let alone = varde_kernel::boolean(
        solid_of(&before, a),
        solid_of(&before, edge),
        varde_kernel::Op::Union,
        &editor.document().tolerance(),
        &varde_kernel::Budget::DEFAULT,
    );
    assert!(alone.is_err(), "the blocks alone make a solid");
    let united = add_combine(&mut editor, combine(a, &[edge, bridge], BodyOp::Union));
    let evaluation = evaluated(editor.document());
    assert_near(
        only_body(&evaluation).volume(),
        3.0 * 1000.0 - 2.0 * 5.0 * 5.0 * 10.0,
    );
    assert_eq!(evaluation.merged, [(edge, a), (bridge, a)]);
    // Without the bridge it fails, naming the step, and changes nothing.
    editor
        .apply(Command::SetFeature {
            feature: united,
            kind: Box::new(combine(a, &[edge], BodyOp::Union).into()),
        })
        .unwrap();
    let evaluation = evaluated(editor.document());
    let why = failure(&evaluation, united).unwrap();
    assert!(
        why.starts_with("joining Body 2 to Body 1 leaves no clean solid"),
        "{why}"
    );
    assert_eq!(made(&evaluation), [a, edge, bridge]);
    assert!(evaluation.merged.is_empty());
}

#[test]
fn a_combine_leaving_nothing_fails_and_changes_nothing() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let cover = block(&mut editor, -5.0, -5.0, 15.0, 15.0, "20");
    let apart = block(&mut editor, 30.0, 0.0, 40.0, 10.0, "10");
    let cut = add_combine(&mut editor, combine(a, &[cover], BodyOp::Subtract));
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, cut),
        Some(
            "cutting Body 2 from Body 1 would leave nothing of Body 1: take Body 2 out of the \
             tools, or delete Body 1"
        )
    );
    assert_eq!(made(&evaluation), [a, cover, apart]);
    assert!(evaluation.merged.is_empty());
    assert_near(solid_of(&evaluation, a).volume(), 1000.0);
    let intersect = add_combine(&mut editor, combine(a, &[apart], BodyOp::Intersect));
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, intersect),
        Some("intersecting Body 1 with Body 3 would leave nothing of Body 1: they don't overlap")
    );
    assert_eq!(made(&evaluation), [a, cover, apart]);
    // An intersect emptying the target at its second tool changes nothing
    // either, the first step's result dropped.
    let both = combine(a, &[cover, apart], BodyOp::Intersect);
    editor
        .apply(Command::SetFeature {
            feature: intersect,
            kind: Box::new(both.into()),
        })
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert!(failure(&evaluation, intersect).is_some());
    assert_near(solid_of(&evaluation, a).volume(), 1000.0);
}

/// A body an earlier join consumed has no solid of its own: a combine
/// naming it, as target or tool, fails, naming the body holding it.
#[test]
fn a_combine_naming_a_consumed_body_fails() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 20.0, 0.0, 30.0, 10.0, "10");
    let c = block(&mut editor, 40.0, 0.0, 50.0, 10.0, "10");
    // A bar from b to c merges c into b.
    let extent = Extent::OneSide(length(editor.document(), "5"));
    add_extrude(
        &mut editor,
        rectangle((25.0, 2.0), (45.0, 8.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let tool = add_combine(&mut editor, combine(a, &[c], BodyOp::Union));
    let target = add_combine(&mut editor, combine(c, &[a], BodyOp::Union));
    let evaluation = evaluated(editor.document());
    let message = "Body 3 is in Body 2 now: a feature before this one merged it in";
    assert_eq!(failure(&evaluation, tool), Some(message));
    assert_eq!(failure(&evaluation, target), Some(message));
    assert_eq!(made(&evaluation), [a, b]);
    assert_eq!(evaluation.merged, [(c, b)]);
    // And so does one consumed by an earlier combine.
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 5.0, 0.0, 15.0, 10.0, "10");
    add_combine(&mut editor, combine(a, &[b], BodyOp::Subtract));
    let again = add_combine(&mut editor, combine(a, &[b], BodyOp::Union));
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, again),
        Some("Body 2 is in Body 1 now: a feature before this one merged it in")
    );
    assert_eq!(made(&evaluation), [a]);
    assert_near(solid_of(&evaluation, a).volume(), 500.0);
}

/// A tool whose maker failed upstream has no solid: the combine fails,
/// showing nothing, and the target stays as it was. Removing the tool's maker removes the
/// combine with it.
#[test]
fn a_tool_without_a_solid_fails_and_a_removed_one_takes_the_combine() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 5.0, 0.0, 15.0, 10.0, "10");
    let maker = editor.document().body(b).unwrap().created_by;
    let united = add_combine(&mut editor, combine(a, &[b], BodyOp::Union));
    // The tool's sketch emptied: its extrude finds no region.
    let features = editor.document().features();
    let index = features.iter().position(|f| f.id == maker).unwrap();
    let sketch = features[index - 1].id;
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::default(),
        })
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, maker), Some("region not found"));
    assert_eq!(
        failure(&evaluation, united),
        Some("Body 2 has no solid: the feature making it failed")
    );
    // Neither shows anything: the region and the solid are nowhere.
    assert!(evaluation.failed.iter().all(|f| f.geometry.is_none()));
    assert_eq!(made(&evaluation), [a]);
    assert_near(solid_of(&evaluation, a).volume(), 1000.0);
    editor.undo();
    editor.apply(Command::RemoveBody(b)).unwrap();
    assert!(editor.document().feature(united).is_none());
    let evaluation = evaluated(editor.document());
    assert_near(only_body(&evaluation).volume(), 1000.0);
}

/// Features after a combine find its tools in the target: a sketch on a
/// consumed tool's face is placed on the target's solid and a join from
/// it touches the target, and a second combine works on the first's
/// result.
#[test]
fn later_features_on_a_consumed_tool_follow_the_target() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 5.0, 0.0, 15.0, 10.0, "6");
    let b_maker = editor.document().body(b).unwrap().created_by;
    add_combine(&mut editor, combine(a, &[b], BodyOp::Union));
    // A boss on b's top, past a's side, joined.
    let face = FaceRef {
        body: b,
        key: FaceKey {
            feature: b_maker.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::new(13.0, 5.0, 6.0),
    };
    editor
        .apply(editor.document().add_sketch(Plane::Face(face)))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let mut drawn = Sketch::default();
    disc((13.0, 5.0), 1.0)(&mut drawn);
    let profiles = drawn.profiles().unwrap();
    let regions = vec![profiles.reference(0).unwrap()];
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let boss = Extrude {
        sketch,
        regions,
        extent: Extent::OneSide(length(editor.document(), "2")),
        flip: false,
        operation: Operation::Join(Targets::default()),
    };
    editor
        .apply(editor.document().add_feature(boss.into()))
        .unwrap();
    let join = editor.document().features().last().unwrap().id;
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let placement = (evaluation.placements.iter())
        .find(|(id, _)| *id == sketch)
        .unwrap()
        .1;
    assert_eq!(placement.origin, DVec3::new(0.0, 0.0, 6.0));
    assert_eq!(evaluation.touched, [(join, vec![a])]);
    assert_eq!(evaluation.merged, [(b, a)]);
    assert_near(
        only_body(&evaluation).volume(),
        1000.0 + 5.0 * 10.0 * 6.0 + PI * 2.0,
    );

    // A second combine after the first: a block cut from a's union.
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 5.0, 0.0, 15.0, 10.0, "10");
    let c = block(&mut editor, 12.0, -5.0, 20.0, 15.0, "20");
    add_combine(&mut editor, combine(a, &[b], BodyOp::Union));
    add_combine(&mut editor, combine(a, &[c], BodyOp::Subtract));
    let evaluation = evaluated(editor.document());
    assert_near(only_body(&evaluation).volume(), 12.0 * 10.0 * 10.0);
    assert_eq!(evaluation.merged, [(b, a), (c, a)]);
}

/// Toggling keep tools or swapping the operation back finds every
/// boolean in the cache.
#[test]
fn a_combine_s_steps_are_cached() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 5.0, 0.0, 15.0, 10.0, "10");
    let c = block(&mut editor, 0.0, 5.0, 10.0, 15.0, "10");
    let id = add_combine(&mut editor, combine(a, &[b, c], BodyOp::Subtract));
    let mut cache = Cache::default();
    cache.begin();
    let first = evaluate(editor.document(), &mut cache);
    let (_, worked) = cache.counts();
    let set = |editor: &mut Editor, combine: Combine| {
        editor
            .apply(Command::SetFeature {
                feature: id,
                kind: Box::new(combine.into()),
            })
            .unwrap();
    };
    let kept = Combine {
        keep_tools: true,
        ..combine(a, &[b, c], BodyOp::Subtract)
    };
    set(&mut editor, kept);
    cache.begin();
    let second = evaluate(editor.document(), &mut cache);
    assert_eq!(cache.counts().1, worked, "nothing worked out again");
    assert_eq!(solid_of(&first, a).volume(), solid_of(&second, a).volume());
    assert_eq!(made(&second), [a, b, c]);
    // Another operation works out its own steps, and back finds the first.
    set(&mut editor, combine(a, &[b, c], BodyOp::Intersect));
    cache.begin();
    evaluate(editor.document(), &mut cache);
    let (_, intersected) = cache.counts();
    assert_eq!(intersected, worked + 2);
    set(&mut editor, combine(a, &[b, c], BodyOp::Subtract));
    cache.begin();
    let back = evaluate(editor.document(), &mut cache);
    assert_eq!(cache.counts().1, intersected);
    assert_near(solid_of(&back, a).volume(), 1000.0 - 500.0 - 250.0);
}

/// A combine set up as a draft is answered as if it were applied, and its
/// error comes back as the draft's.
#[test]
fn a_combine_draft_is_previewed() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let b = block(&mut editor, 5.0, 0.0, 15.0, 10.0, "10");
    let mut regenerator = crate::Regenerator::default();
    let draft = Draft {
        revision: 1,
        feature: None,
        kind: combine(a, &[b], BodyOp::Union).into(),
    };
    let answer = answered(regenerator.handle(regenerate_with(&editor, Some(draft))));
    let drafted = answer.draft.unwrap();
    assert_eq!(drafted.error, None);
    assert_eq!(drafted.touched, None);
    assert_eq!(answer.parts, [a]);
    let [(body, bounds)] = answer.bodies[..] else {
        panic!("one body");
    };
    assert_eq!(body, a);
    assert_eq!(bounds.max.x, 15.0);
    let kept = Draft {
        revision: 2,
        feature: None,
        kind: Combine {
            keep_tools: true,
            ..combine(a, &[b], BodyOp::Subtract)
        }
        .into(),
    };
    let answer = answered(regenerator.handle(regenerate_with(&editor, Some(kept))));
    assert_eq!(answer.draft.unwrap().error, None);
    assert_eq!(answer.parts, [a, b]);
    let covered = Draft {
        revision: 3,
        feature: None,
        kind: combine(a, &[b], BodyOp::Intersect).into(),
    };
    let answer = answered(regenerator.handle(regenerate_with(&editor, Some(covered))));
    assert_eq!(answer.draft.unwrap().error, None);
    let refused = Draft {
        revision: 4,
        feature: None,
        kind: combine(a, &[a], BodyOp::Union).into(),
    };
    let answer = answered(regenerator.handle(regenerate_with(&editor, Some(refused))));
    let error = answer.draft.unwrap().error.unwrap();
    assert!(error.contains("is one of its tools too"), "{error}");
    assert_eq!(answer.parts, [a, b], "the committed model");
}

pub(super) mod fuzz;

/// A union step that fails shows where, the faces it names found on the
/// bodies they are of: the target's running solid holds the tools united
/// with it before, so a face of one of those is on that tool's body. A
/// block, a disc overlapping it, and a disc beside that one touching it
/// along a line: the last fails, the walls touching on the two discs.
#[test]
fn a_failing_step_shows_the_faces_on_their_bodies() {
    let mut editor = Editor::new(Document::default());
    let target = block(&mut editor, -10.0, -2.0, -3.0, 2.0, "10");
    let first = add_body(&mut editor, disc((0.0, 0.0), 5.0), "10");
    let beside = add_body(&mut editor, disc((10.0, 0.0), 5.0), "10");
    let united = add_combine(
        &mut editor,
        combine(target, &[first, beside], BodyOp::Union),
    );
    let crate::Response::Regenerated {
        mesh,
        picking,
        failed,
        ..
    } = crate::handle(regenerate_with(&editor, None))
    else {
        panic!("regeneration failed");
    };
    let [failure] = &failed[..] else {
        panic!("{failed:?}");
    };
    assert_eq!(failure.feature, united);
    assert!(
        failure.message.starts_with("joining Body 3 to Body 1"),
        "{}",
        failure.message
    );
    let geometry = failure.geometry.as_ref().expect("geometry");
    let faces = geometry.faces();
    // The walls of both discs, every face of each in the model, and
    // nothing of the target.
    for body in [first, beside] {
        let named: Vec<u32> = (faces.iter())
            .filter(|&&(b, _)| b == body)
            .map(|&(_, f)| f)
            .collect();
        assert!(!named.is_empty(), "{body:?}: {faces:?}");
        let wall = picking.faces()[named[0] as usize].key;
        let walls: Vec<u32> = (0..mesh.face_count() as u32)
            .filter(|&f| {
                picking.face_body(&mesh, f) == Some(body) && picking.faces()[f as usize].key == wall
            })
            .collect();
        assert_eq!(named, walls, "{body:?}");
    }
    assert!(faces.iter().all(|&(body, _)| body != target), "{faces:?}");
}

/// A union step that fails leaves the running solid as it was, so the
/// steps after it are filed as without it: a tool put aside and tried
/// again once another bridged it ends under the key of the bridge's step
/// and then its own, each step keyed by the running solid's key and the
/// tool's.
#[test]
fn a_tool_put_aside_is_filed_after_the_steps_that_worked() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let edge = block(&mut editor, 10.0, 10.0, 20.0, 20.0, "10");
    let bridge = block(&mut editor, 5.0, 5.0, 15.0, 15.0, "10");
    let before = evaluated(editor.document());
    let key = |body: BodyId| {
        let made = before.bodies.iter().find(|made| made.body == body);
        made.unwrap().key
    };
    add_combine(&mut editor, combine(a, &[edge, bridge], BodyOp::Union));
    let evaluation = evaluated(editor.document());
    let [united] = &evaluation.bodies[..] else {
        panic!("one body: {:?}", made(&evaluation));
    };
    let bridged = boolean_key(Doing::Merging, key(a), key(bridge));
    assert_eq!(united.key, boolean_key(Doing::Merging, bridged, key(edge)));
}

/// A tool that fails again when tried a second time fails the union
/// with that try's error, its faces looked for on the target and the
/// tools united so far and on the tool; nothing changes. Another tool
/// put aside after it isn't tried again.
#[test]
fn a_tool_failing_again_fails_the_union_with_the_faces_of_its_second_try() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    // Along a's edge at (0, 10), which no tool bridges.
    let alone = block(&mut editor, -10.0, 10.0, 0.0, 20.0, "10");
    // Along a's edge at (10, 10), which the bridge covers.
    let edge = block(&mut editor, 10.0, 10.0, 20.0, 20.0, "10");
    let bridge = block(&mut editor, 5.0, 5.0, 15.0, 15.0, "10");
    let united = add_combine(
        &mut editor,
        combine(a, &[alone, edge, bridge], BodyOp::Union),
    );
    let crate::Response::Regenerated { failed, .. } = crate::handle(regenerate_with(&editor, None))
    else {
        panic!("regeneration failed");
    };
    let [failure] = &failed[..] else {
        panic!("{failed:?}");
    };
    assert_eq!(failure.feature, united);
    assert!(
        failure.message.starts_with("joining Body 2 to Body 1"),
        "{}",
        failure.message
    );
    if let Some(geometry) = &failure.geometry {
        assert!(
            (geometry.faces().iter()).all(|&(body, _)| [a, bridge, alone].contains(&body)),
            "{:?}",
            geometry.faces()
        );
    }
    let evaluation = evaluated(editor.document());
    assert_eq!(made(&evaluation), [a, alone, edge, bridge]);
    assert!(evaluation.merged.is_empty());
    assert_near(solid_of(&evaluation, a).volume(), 1000.0);
}
