use std::cell::RefCell;
use std::rc::Rc;

use varde_document::{Command, Document, FeatureId, FeatureKind};

use varde_regen::handle;

use super::*;
use crate::tests::Deferred;

/// A feed sending to a lane that keeps its requests, and the list they're
/// kept in.
fn connected() -> (MeshFeed, Rc<RefCell<Vec<Request>>>) {
    let requests = Rc::default();
    let mut feed = MeshFeed::new();
    feed.connect(Deferred(Rc::clone(&requests)));
    (feed, requests)
}

/// An editor, at generation 0, on a document with a sketch holding one
/// line.
fn one_line() -> Editor {
    let mut editor = Editor::new(Document::default());
    add_sketch(&mut editor);
    Editor::new(editor.document().clone())
}

/// Adds a line to the first sketch, as one edit, so the lines shown tell
/// the generations apart.
fn add_line(editor: &mut Editor) {
    let feature = &editor.document().features()[0];
    let FeatureKind::Sketch { sketch, .. } = &feature.kind else {
        panic!("the first feature is a sketch");
    };
    let mut sketch = sketch.clone();
    let y = sketch.curves.len() as f64;
    let start = sketch.add_point(glam::DVec2::new(0.0, y)).unwrap();
    let end = sketch.add_point(glam::DVec2::new(1.0, y)).unwrap();
    sketch
        .add_curve(varde_sketch::Curve::Line { start, end }, false)
        .unwrap();
    let feature = feature.id;
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
}

#[test]
fn requests_only_newer_generations() {
    let mut editor = one_line();
    let (mut feed, regen) = connected();

    feed.request(&editor, None);
    feed.request(&editor, None);
    assert_eq!(regen.borrow().len(), 1);

    add_line(&mut editor);
    feed.request(&editor, None);
    let generations: Vec<_> = regen
        .borrow()
        .iter()
        .map(|request| u64::from(request.generation().unwrap()))
        .collect();
    assert_eq!(generations, [0, 1]);
}

#[test]
fn undo_is_regenerated_and_shown() {
    let mut editor = one_line();
    let (mut feed, regen) = connected();
    add_line(&mut editor);
    feed.request(&editor, None);
    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.sketches().segment_count(), 2);

    // Back to a state seen before, but a newer generation: asked for and
    // shown again rather than taken for the older mesh.
    editor.undo();
    feed.request(&editor, None);
    assert_eq!(feed.status(&editor), MeshStatus::Regenerating);
    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
    assert_eq!(feed.sketches().segment_count(), 1);
}

#[test]
fn keeps_the_last_mesh_while_regenerating() {
    let mut editor = one_line();
    let (mut feed, regen) = connected();
    feed.request(&editor, None);
    feed.apply(handle(regen.borrow_mut().remove(0)));

    add_line(&mut editor);
    feed.request(&editor, None);
    assert_eq!(feed.status(&editor), MeshStatus::Regenerating);
    assert_eq!(feed.generation(), Some(Generation::from(0)));
    assert_eq!(feed.sketches().segment_count(), 1);

    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
    assert_eq!(feed.sketches().segment_count(), 2);
}

#[test]
fn drops_out_of_order_and_superseded_responses() {
    let mut editor = one_line();
    let (mut feed, regen) = connected();
    for _ in 0..3 {
        feed.request(&editor, None);
        add_line(&mut editor);
    }
    feed.request(&editor, None);
    let mut responses: Vec<_> = regen.take().into_iter().map(handle).collect();
    let newest = responses.pop().unwrap();
    let older = responses.pop().unwrap();

    // The newest arrives first; the one it superseded is dropped after it.
    feed.apply(newest.clone());
    feed.apply(older);
    assert_eq!(feed.generation(), Some(Generation::from(3)));
    assert_eq!(feed.sketches().segment_count(), 4);

    // A repeat of what's shown changes nothing either.
    feed.apply(newest);
    assert_eq!(feed.generation(), Some(Generation::from(3)));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
}

#[test]
fn failure_ends_regenerating_and_keeps_the_last_mesh() {
    let mut editor = one_line();
    let (mut feed, regen) = connected();
    feed.request(&editor, None);
    feed.apply(handle(regen.borrow_mut().remove(0)));

    add_line(&mut editor);
    feed.request(&editor, None);
    let request = regen.borrow_mut().remove(0);
    feed.apply(Response::Failed {
        draft: None,
        generation: request.generation().unwrap(),
        exclude: request.exclude(),
        error: "the kernel failed".to_owned(),
    });
    assert_eq!(
        feed.status(&editor),
        MeshStatus::Failed("the kernel failed")
    );
    assert_eq!(feed.generation(), Some(Generation::from(0)));
    assert_eq!(feed.sketches().segment_count(), 1);

    // A late mesh for the generation that failed changes nothing.
    feed.apply(handle(request));
    assert_eq!(feed.generation(), Some(Generation::from(0)));

    // The next edit hides the error while it regenerates, then clears it.
    add_line(&mut editor);
    feed.request(&editor, None);
    assert_eq!(feed.status(&editor), MeshStatus::Regenerating);
    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
    assert_eq!(feed.sketches().segment_count(), 3);
}

/// Nothing is asked for before the lane has started, and then the
/// editor's newest generation.
#[test]
fn requests_nothing_until_connected() {
    let mut editor = one_line();
    let mut feed = MeshFeed::new();
    feed.request(&editor, None);
    add_line(&mut editor);
    feed.request(&editor, None);
    assert!(!feed.connected());

    let requests = Rc::default();
    feed.connect(Deferred(Rc::clone(&requests)));
    feed.request(&editor, None);
    let generations: Vec<_> = requests
        .borrow()
        .iter()
        .map(|request| u64::from(request.generation().unwrap()))
        .collect();
    assert_eq!(generations, [1]);
}

/// A sketch on XY with a line in it, added to `editor`.
fn add_sketch(editor: &mut Editor) {
    use glam::DVec2;
    use varde_document::{OriginPlane, Plane, Sketch};

    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = Sketch::default();
    let start = sketch.add_point(DVec2::ZERO).unwrap();
    let end = sketch.add_point(DVec2::new(1.0, 2.0)).unwrap();
    sketch
        .add_curve(varde_sketch::Curve::Line { start, end }, false)
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
}

#[test]
fn sketches_are_shown_with_their_mesh() {
    let mut editor = Editor::new(Document::default());
    let (mut feed, regen) = connected();
    assert_eq!(feed.sketches().segment_count(), 0);
    add_sketch(&mut editor);
    feed.request(&editor, None);
    let request = regen.borrow_mut().remove(0);
    assert_eq!(request.exclude(), None);
    feed.apply(handle(request));
    assert_eq!(feed.sketches().points(), [[0.0; 3], [1.0, 2.0, 0.0]]);

    // Kept while the next generation fails, like the mesh.
    add_sketch(&mut editor);
    feed.request(&editor, None);
    let request = regen.borrow_mut().remove(0);
    feed.apply(Response::Failed {
        draft: None,
        generation: request.generation().unwrap(),
        exclude: request.exclude(),
        error: "no".to_owned(),
    });
    assert_eq!(feed.sketches().segment_count(), 1);

    add_sketch(&mut editor);
    feed.request(&editor, None);
    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.sketches().segment_count(), 3);
}

/// What `requests` asked to leave out, in order.
fn left_out(requests: &RefCell<Vec<Request>>) -> Vec<Option<FeatureId>> {
    requests.borrow().iter().map(Request::exclude).collect()
}

#[test]
fn leaving_out_another_sketch_asks_again() {
    let mut editor = Editor::new(Document::default());
    add_sketch(&mut editor);
    let feature = editor.document().features()[0].id;
    let (mut feed, regen) = connected();
    feed.request(&editor, None);
    feed.request(&editor, Some(feature));
    feed.request(&editor, Some(feature));
    feed.request(&editor, None);
    assert_eq!(left_out(&regen), [None, Some(feature), None]);
    let generations: Vec<_> = regen
        .borrow()
        .iter()
        .map(|request| u64::from(request.generation().unwrap()))
        .collect();
    assert_eq!(generations, [2, 2, 2]);
}

#[test]
fn only_the_sketch_asked_for_last_is_shown_left_out() {
    let mut editor = Editor::new(Document::default());
    add_sketch(&mut editor);
    let feature = editor.document().features()[0].id;
    let (mut feed, regen) = connected();
    feed.request(&editor, None);
    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.left_out(), Some(None));
    assert_eq!(feed.sketches().segment_count(), 1);

    // Entering the sketch: the same generation, answered again without it.
    feed.request(&editor, Some(feature));
    let entered = handle(regen.borrow_mut().remove(0));
    // Leaving it before the answer came: that answer is out of date.
    feed.request(&editor, None);
    let left = handle(regen.borrow_mut().remove(0));
    feed.apply(entered.clone());
    assert_eq!(feed.left_out(), Some(None));
    assert_eq!(feed.sketches().segment_count(), 1);
    feed.apply(left);
    assert_eq!(feed.left_out(), Some(None));

    // Entering it again: its answer is shown, once.
    feed.request(&editor, Some(feature));
    regen.borrow_mut().clear();
    feed.apply(entered.clone());
    assert_eq!(feed.left_out(), Some(Some(feature)));
    assert_eq!(feed.sketches().segment_count(), 0);
    assert_eq!(feed.status(&editor), MeshStatus::Current);

    // A failure of the same generation doesn't replace it.
    feed.apply(Response::Failed {
        draft: None,
        generation: editor.generation(),
        exclude: Some(feature),
        error: "no".to_owned(),
    });
    assert_eq!(feed.status(&editor), MeshStatus::Current);
    feed.apply(entered);
    assert_eq!(feed.left_out(), Some(Some(feature)));
}

#[test]
fn a_failure_doesn_t_hold_back_leaving_out_another_sketch() {
    let mut editor = Editor::new(Document::default());
    add_sketch(&mut editor);
    let feature = editor.document().features()[0].id;
    let (mut feed, regen) = connected();
    // The request with nothing left out fails, say the worker died.
    feed.request(&editor, None);
    let request = regen.borrow_mut().remove(0);
    feed.apply(Response::Failed {
        draft: None,
        generation: request.generation().unwrap(),
        exclude: None,
        error: "the worker stopped".to_owned(),
    });
    assert_eq!(
        feed.status(&editor),
        MeshStatus::Failed("the worker stopped")
    );

    // Entering the sketch asks again, of the same generation: its answer
    // is shown, and the failure was of another request.
    feed.request(&editor, Some(feature));
    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.left_out(), Some(Some(feature)));
    assert_eq!(feed.status(&editor), MeshStatus::Current);

    // Leaving it, that request fails in turn: it's reported, and a late
    // model for it changes nothing.
    feed.request(&editor, None);
    let request = regen.borrow_mut().remove(0);
    feed.apply(Response::Failed {
        draft: None,
        generation: request.generation().unwrap(),
        exclude: None,
        error: "again".to_owned(),
    });
    assert_eq!(feed.status(&editor), MeshStatus::Failed("again"));
    feed.apply(handle(request));
    assert_eq!(feed.left_out(), Some(Some(feature)));
}

/// The example's plate sketch, and its extrude as a draft of a new one.
fn plate_draft() -> (Editor, (Option<FeatureId>, varde_document::Extrude)) {
    let example = Document::example();
    let FeatureKind::Extrude(extrude) = &example.features()[1].kind else {
        panic!("the second feature is an extrude");
    };
    let extrude = extrude.clone();
    let mut editor = Editor::new(example);
    let feature = editor.document().features()[1].id;
    editor.apply(Command::RemoveFeature(feature)).unwrap();
    (Editor::new(editor.document().clone()), (None, extrude))
}

/// `draft`, an extrude's, as a feed takes it.
fn drafted(
    (feature, extrude): (Option<FeatureId>, varde_document::Extrude),
) -> Option<(Option<FeatureId>, FeatureKind)> {
    Some((feature, extrude.into()))
}

/// The draft revision each request waiting carries.
fn revisions(regen: &RefCell<Vec<Request>>) -> Vec<Option<u64>> {
    regen.borrow().iter().map(Request::draft).collect()
}

#[test]
fn a_draft_is_asked_for_once_per_change_and_its_newest_answer_kept() {
    let (editor, draft) = plate_draft();
    let (mut feed, regen) = connected();
    feed.request(&editor, None);
    feed.request_with(&editor, None, drafted(draft.clone()));
    feed.request_with(&editor, None, drafted(draft.clone()));
    let mut taller = draft.clone();
    taller.1.extent = varde_document::Extent::OneSide(
        varde_expr::Value::new(
            "20",
            &varde_document::Extent::ask(&editor.document().design()),
        )
        .unwrap(),
    );
    feed.request_with(&editor, None, drafted(taller));
    assert_eq!(revisions(&regen), [None, Some(1), Some(2)]);

    // Answered in order, only the newest draft's answer is shown: the
    // model without it and the older draft come after it here.
    let answers: Vec<Response> = regen.take().into_iter().map(handle).collect();
    let [plain, first, second] = <[Response; 3]>::try_from(answers).unwrap();
    feed.apply(second);
    assert_eq!(feed.shown_draft(), Some(2));
    assert_eq!(feed.draft_error(), None);
    feed.apply(first);
    feed.apply(plain);
    assert_eq!(feed.shown_draft(), Some(2));

    // Without the draft, the model is asked for again, and taken.
    feed.request_with(&editor, None, None);
    assert_eq!(revisions(&regen), [None]);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.shown_draft(), None);
    assert_eq!(feed.mesh().triangle_count(), 0);

    // The same draft again is a new one, after another.
    feed.request_with(&editor, None, drafted(draft));
    assert_eq!(revisions(&regen), [Some(3)]);
}

#[test]
fn a_failing_draft_says_why_for_its_revision_only() {
    let (editor, (feature, mut extrude)) = plate_draft();
    extrude.regions.clear();
    let (mut feed, regen) = connected();
    feed.request_with(&editor, None, drafted((feature, extrude.clone())));
    feed.apply(handle(regen.take().pop().unwrap()));
    assert!(feed.draft_error().is_some());
    // Answered with the model without it.
    assert_eq!(feed.shown_draft(), Some(1));
    assert_eq!(feed.mesh().triangle_count(), 0);
    // Another draft asked for, the error is no longer its.
    extrude.flip = true;
    feed.request_with(&editor, None, drafted((feature, extrude)));
    assert_eq!(feed.draft_error(), None);
}

#[test]
fn a_draft_dropped_is_regenerating_until_the_model_without_it_shows() {
    let (editor, draft) = plate_draft();
    let (mut feed, regen) = connected();
    feed.request_with(&editor, None, drafted(draft.clone()));
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
    assert!(feed.mesh().triangle_count() > 0);

    // Cancelled: the draft's model still shows, which isn't the
    // document's.
    feed.request_with(&editor, None, None);
    assert_eq!(feed.status(&editor), MeshStatus::Regenerating);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.status(&editor), MeshStatus::Current);

    // Another draft likewise, until its answer.
    feed.request_with(&editor, None, drafted(draft));
    assert_eq!(feed.status(&editor), MeshStatus::Regenerating);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
}

#[test]
fn touched_bodies_are_kept_within_a_run_of_drafts_only() {
    // A join of the example's plate sketch, new, onto the plate.
    let editor = Editor::new(Document::example());
    let plate = editor.document().features()[1].id;
    let FeatureKind::Extrude(extrude) = &editor.document().features()[1].kind else {
        panic!("the example's second feature is its extrude");
    };
    let mut join = extrude.clone();
    join.operation = varde_document::Operation::Join(varde_document::Targets::default());
    let body = editor.document().bodies()[0].id;
    let (mut feed, regen) = connected();
    feed.request_with(&editor, None, drafted((None, join.clone())));
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.draft_touched(), [body]);

    // A draft the document refuses, its sketch the extrude, keeps the
    // list: its touch test didn't run.
    let mut refused = join.clone();
    refused.sketch = plate;
    feed.request_with(&editor, None, drafted((None, refused)));
    feed.apply(handle(regen.take().pop().unwrap()));
    assert!(feed.draft_error().is_some());
    assert_eq!(feed.draft_touched(), [body]);

    // Without a draft, unanswered, then another: a new run, listing
    // nothing until its answer.
    feed.request_with(&editor, None, None);
    regen.take();
    feed.request_with(&editor, None, drafted((None, join.clone())));
    assert_eq!(feed.draft_touched(), []);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.draft_touched(), [body]);

    // Another feature's draft, with none in between, likewise.
    feed.request_with(&editor, None, drafted((Some(plate), join)));
    assert_eq!(feed.draft_touched(), []);
}

#[test]
fn merged_bodies_follow_the_model_shown() {
    let (mut editor, [top, below], join) = crate::tests::merged_plates();
    let (mut feed, regen) = connected();
    feed.request(&editor, None);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.merged_bodies(), [(below, top)]);
    assert!(feed.merges(editor.document(), join));
    let document = editor.document();
    assert_eq!(feed.merged_before(document, None).holder(below), Some(top));
    // Before the join, nothing is merged yet.
    assert_eq!(feed.merged_before(document, Some(join)), Merges::default());

    // A failed answer keeps the model shown, and so its merge.
    editor.apply(Command::SetVisible(top, false)).unwrap();
    feed.request(&editor, None);
    regen.take();
    feed.apply(Response::Failed {
        generation: editor.generation(),
        exclude: None,
        draft: None,
        error: "failed".to_owned(),
    });
    assert_eq!(feed.merged_bodies(), [(below, top)]);

    // The next model has the merge it found: none once the join is gone.
    let generation = editor.generation();
    editor.apply(Command::RemoveFeature(join)).unwrap();
    feed.request(&editor, None);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert!(feed.generation() > Some(generation));
    assert_eq!(feed.merged_bodies(), []);
    assert_eq!(
        feed.merged_before(editor.document(), None).holder(below),
        None
    );
}

#[test]
fn merged_bodies_aren_t_given_out_across_a_replacement() {
    let (editor, [top, below], _) = crate::tests::merged_plates();
    let (mut feed, regen) = connected();
    feed.request(&editor, None);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.merged_bodies(), [(below, top)]);
    feed.replaced(Generation::from(u64::from(editor.generation()) + 1));
    assert_eq!(feed.merged_bodies(), []);
}

#[test]
fn a_body_merged_into_one_merged_later_moves_on() {
    // Three bodies' ids: the plates and a disc of its own.
    let (mut editor, [a, b], _) = crate::tests::merged_plates();
    let extent = crate::tests::two_sides(editor.document(), "1", "1");
    let new = varde_document::Operation::NewBody(varde_document::BodyId::NEW);
    crate::tests::add_disc(&mut editor, (100.0, 0.0), extent, new);
    let c = editor.document().bodies()[2].id;

    let mut merges = Merges::default();
    merges.join(&[b, c]);
    merges.join(&[a]);
    assert_eq!(
        [a, b, c].map(|body| merges.holder(body)),
        [None, None, Some(b)]
    );
    assert_eq!(merges.held_by(b).collect::<Vec<_>>(), [c]);
    // `b` merged into `a` takes `c` with it.
    merges.join(&[a, b]);
    assert_eq!(
        [a, b, c].map(|body| merges.holder(body)),
        [None, Some(a), Some(a)]
    );
    assert_eq!(merges.held_by(a).collect::<Vec<_>>(), [c, b]);
    assert_eq!(merges.held_by(b).count(), 0);
}

/// A model shown again, the same mesh and tables in new `Arc`s (as the
/// web worker's answers always are), keeps its count, and so its pick
/// index; another counts on.
#[test]
fn the_same_model_answered_anew_keeps_its_count() {
    let mut editor = Editor::new(varde_document::Document::example());
    let (mut feed, regen) = connected();
    feed.request(&editor, None);
    feed.apply(handle(regen.take().pop().unwrap()));
    let model = feed.model();
    let shown = feed.mesh().clone();
    feed.pick_index();
    // Inches: the same solid.
    editor
        .apply(Command::SetUnits(varde_document::LengthUnit::In))
        .unwrap();
    feed.request(&editor, None);
    let mut response = handle(regen.take().pop().unwrap());
    let Response::Regenerated { mesh, picking, .. } = &mut response else {
        panic!("{response:?}");
    };
    // Copies, as from the other side of the web worker.
    *mesh = Arc::new(RenderMesh::clone(mesh));
    *picking = Arc::new(Picking::clone(picking));
    feed.apply(response);
    assert!(!Arc::ptr_eq(feed.mesh(), &shown));
    assert_eq!(feed.model(), model);
    assert!(feed.index.get().is_some());
    // The plate hidden: another model.
    let body = editor.document().bodies()[0].id;
    editor.apply(Command::SetVisible(body, false)).unwrap();
    feed.request(&editor, None);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_ne!(feed.model(), model);
    assert!(feed.index.get().is_none());
}

/// The body of each of the mesh's parts is kept with the mesh, and kept
/// while a failure is shown next to it.
#[test]
fn the_parts_bodies_follow_the_mesh_shown() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let (mut feed, regen) = connected();
    assert!(feed.parts().is_empty());
    feed.request(&editor, None);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.parts(), [body]);
    assert_eq!(feed.mesh().part_ends().len(), 1);

    let mut editor = editor;
    editor.apply(Command::SetVisible(body, false)).unwrap();
    feed.request(&editor, None);
    let request = regen.take().pop().unwrap();
    feed.apply(Response::Failed {
        generation: request.generation().unwrap(),
        exclude: None,
        draft: None,
        error: "no".to_owned(),
    });
    assert_eq!(feed.parts(), [body]);
}
