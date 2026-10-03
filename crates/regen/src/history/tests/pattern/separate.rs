//! Patterns whose copies are bodies of their own: each copy's body and
//! its faces' names, overlapping copies left apart, the tick toggled in
//! an edit and undone, later features using a copy body (a combine, a
//! join, a pattern about a copy's face), and the cache.

use varde_document::Copies;

use super::*;

/// `pattern` with each copy a body of its own.
fn separate(mut pattern: Pattern) -> Pattern {
    pattern.copies = Copies::Separate(Vec::new());
    pattern
}

/// The copy bodies the pattern feature `id` of `document` lists.
fn copy_bodies(document: &Document, id: FeatureId) -> Vec<BodyId> {
    match &document.feature(id).unwrap().kind {
        FeatureKind::Pattern(Pattern {
            copies: Copies::Separate(made),
            ..
        }) => made.clone(),
        _ => Vec::new(),
    }
}

/// Two pins, 20 apart in Y, patterned 4 times 3 along X unjoined: the
/// copies overlap each other and their originals (the pins are 4 wide),
/// and stay bodies of their own, each a pin where its copy goes, its
/// wall named as that copy; the originals are as they were.
#[test]
fn unjoined_copies_are_bodies_of_their_own() {
    let mut editor = Editor::new(Document::default());
    let a = pin(&mut editor, 0.0, 0.0);
    let b = pin(&mut editor, 0.0, 20.0);
    let maker = editor.document().features()[1].id;
    let before = evaluated(editor.document());
    let pattern = separate(linear(editor.document(), &[a, b], X, "4", "3"));
    let id = add(&mut editor, pattern);
    let document = editor.document();
    let made = copy_bodies(document, id);
    assert_eq!(made.len(), 6);
    let evaluation = evaluated(document);
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    // The originals first, unchanged; then the copies by id.
    let order: Vec<BodyId> = evaluation.bodies.iter().map(|made| made.body).collect();
    assert_eq!(order, [&[a, b][..], &made[..]].concat());
    assert_eq!(solid_of(&evaluation, a), solid_of(&before, a));
    assert_eq!(solid_of(&evaluation, b), solid_of(&before, b));
    assert!(evaluation.merged.is_empty());
    let wall = cylinders(solid_of(&before, a))[0];
    assert_eq!(wall.feature, maker.get());
    for (k, row) in (1..4u64).zip(made.chunks(2)) {
        for ((&copy, y), original) in row.iter().zip([0.0, 20.0]).zip([a, b]) {
            let wall = cylinders(solid_of(&before, original))[0];
            let solid = solid_of(&evaluation, copy);
            let (volume, centre) = mass(solid);
            assert!(near(volume, PIN, 1e-9), "copy {k}: {volume}");
            let x = 3.0 * k as f64;
            assert!(
                centre.abs_diff_eq(DVec3::new(x, y, 5.0), 1e-9),
                "copy {k}: {centre}"
            );
            assert_eq!(shells(solid), 1, "{copy:?}: one pin");
            assert_eq!(cylinders(solid), [wall.copy(id.get(), k)]);
        }
    }
}

/// The tick toggled in an edit: joined, the copies are in the body;
/// unjoined, each is its own; joined again, back in the body; undo takes
/// each step back.
#[test]
fn toggling_the_tick_moves_the_copies_and_undo_takes_it_back() {
    let mut editor = Editor::new(Document::default());
    let body = pin(&mut editor, 0.0, 0.0);
    let joined = linear(editor.document(), &[body], X, "3", "10");
    let id = add(&mut editor, joined.clone());
    let volume = |evaluation: &Evaluation, body| mass(solid_of(evaluation, body)).0;
    let evaluation = evaluated(editor.document());
    assert!(near(volume(&evaluation, body), 3.0 * PIN, 1e-9));
    assert_eq!(evaluation.bodies.len(), 1);
    set(&mut editor, id, separate(joined.clone()));
    let made = copy_bodies(editor.document(), id);
    assert_eq!(made.len(), 2);
    let evaluation = evaluated(editor.document());
    assert!(near(volume(&evaluation, body), PIN, 1e-9));
    for &copy in &made {
        assert!(near(volume(&evaluation, copy), PIN, 1e-9));
    }
    set(&mut editor, id, joined);
    assert!(copy_bodies(editor.document(), id).is_empty());
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.bodies.len(), 1);
    assert!(near(volume(&evaluation, body), 3.0 * PIN, 1e-9));
    editor.undo();
    let evaluation = evaluated(editor.document());
    assert_eq!(copy_bodies(editor.document(), id), made);
    assert_eq!(evaluation.bodies.len(), 3);
    assert!(near(volume(&evaluation, made[1]), PIN, 1e-9));
    editor.undo();
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.bodies.len(), 1);
    assert!(near(volume(&evaluation, body), 3.0 * PIN, 1e-9));
}

/// A combine cutting a copy body from a plate: the plate less one pin,
/// the copy consumed into it; a join over two copies merges them into
/// the first; a pattern about a copy's wall finds it on the copy's body.
#[test]
fn later_features_use_copy_bodies() {
    let mut editor = Editor::new(Document::default());
    let body = pin(&mut editor, 0.0, 0.0);
    let plate = add_body(&mut editor, rectangle((15.0, -5.0), (25.0, 5.0)), "5");
    let row = separate(linear(editor.document(), &[body], X, "4", "10"));
    let id = add(&mut editor, row);
    let made = copy_bodies(editor.document(), id);
    let cut = Combine {
        target: plate,
        tools: vec![made[1]],
        op: BodyOp::Subtract,
        keep_tools: false,
    };
    let combined = add(&mut editor, cut);
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, combined), None);
    let plate_volume = 10.0 * 10.0 * 5.0;
    let (volume, _) = mass(solid_of(&evaluation, plate));
    assert!(
        near(volume, plate_volume - PI * 4.0 * 5.0, 1e-9),
        "{volume}"
    );
    assert_eq!(evaluation.merged, [(made[1], plate)]);
    assert_eq!(evaluation.holder(made[1]), Some(plate));
    // The pattern of copy 3 about copy 1's wall, as a ring of 2: the
    // wall is found on copy 1's body.
    let wall = cylinders(solid_of(&evaluation, made[0]))[0];
    let about = AxisRef::Face(FaceRef {
        body: made[0],
        key: wall,
        near: DVec3::new(12.0, 0.0, 5.0),
    });
    let ring = circular(editor.document(), &[made[2]], about, "2", "360");
    let turned = add(&mut editor, ring);
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, turned), None);
    let reference = (evaluation.references.iter()).find(|(f, _)| *f == turned);
    let [point, along] = reference.unwrap().1;
    assert!(
        (point - DVec3::new(10.0, 0.0, point.z)).length() < 1e-9,
        "{point}"
    );
    assert!(along.normalize().abs().abs_diff_eq(DVec3::Z, 1e-12));
    // Copy 3 at x 30 and its image at x −10 about x 10, in its body.
    let (volume, centre) = mass(solid_of(&evaluation, made[2]));
    assert!(near(volume, 2.0 * PIN, 1e-9), "{volume}");
    assert!(
        centre.abs_diff_eq(DVec3::new(10.0, 0.0, 5.0), 1e-9),
        "{centre}"
    );
    // A join over copies 1 and 3's image: merged into the first made.
    let extent = Extent::OneSide(length(editor.document(), "2"));
    let bridge = add_extrude(
        &mut editor,
        rectangle((-11.0, -1.0), (11.0, 1.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, bridge), None);
    let touched = (evaluation.touched.iter()).find(|(f, _)| *f == bridge);
    assert_eq!(touched.unwrap().1, [body, made[0], made[2]]);
    assert_eq!(evaluation.holder(made[0]), Some(body));
    assert_eq!(evaluation.holder(made[2]), Some(body));
}

/// A higher count finds the copies made before in the cache, each body's
/// solid the same; another spacing makes them again.
#[test]
fn unjoined_copies_are_found_in_the_cache() {
    let mut editor = Editor::new(Document::default());
    let body = pin(&mut editor, 0.0, 0.0);
    let row = |editor: &Editor, n: &str, step: &str| {
        separate(linear(editor.document(), &[body], X, n, step))
    };
    let three = row(&editor, "3", "10");
    let id = add(&mut editor, three);
    let mut cache = Cache::default();
    cache.begin();
    let first = evaluate(editor.document(), &mut cache);
    let made = copy_bodies(editor.document(), id);
    let more = row(&editor, "5", "10");
    set(&mut editor, id, more);
    assert_eq!(copy_bodies(editor.document(), id)[..2], made[..]);
    cache.begin();
    let more = evaluate(editor.document(), &mut cache);
    assert_eq!(more.bodies.len(), 5);
    let arc = |evaluation: &Evaluation, body: BodyId| {
        let made = evaluation.bodies.iter().find(|m| m.body == body).unwrap();
        made.solid.clone()
    };
    for &copy in &made {
        assert!(Arc::ptr_eq(&arc(&first, copy), &arc(&more, copy)));
    }
    let wider = row(&editor, "5", "12");
    set(&mut editor, id, wider);
    cache.begin();
    let other = evaluate(editor.document(), &mut cache);
    assert!(!Arc::ptr_eq(&arc(&first, made[0]), &arc(&other, made[0])));
}
