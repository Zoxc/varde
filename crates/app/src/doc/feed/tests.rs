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
        detail: None,
        draft: None,
        inspect: None,
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
        detail: None,
        draft: None,
        inspect: None,
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
        detail: None,
        draft: None,
        inspect: None,
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
        detail: None,
        draft: None,
        inspect: None,
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
        detail: None,
        draft: None,
        inspect: None,
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
    feed.request_with(&editor, None, drafted(draft.clone()), None);
    feed.request_with(&editor, None, drafted(draft.clone()), None);
    let mut taller = draft.clone();
    taller.1.extent = varde_document::Extent::OneSide(
        varde_expr::Value::new(
            "20",
            &varde_document::Extent::ask(&editor.document().design()),
        )
        .unwrap(),
    );
    feed.request_with(&editor, None, drafted(taller), None);
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
    feed.request_with(&editor, None, None, None);
    assert_eq!(revisions(&regen), [None]);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.shown_draft(), None);
    assert_eq!(feed.mesh().triangle_count(), 0);

    // The same draft again is a new one, after another.
    feed.request_with(&editor, None, drafted(draft), None);
    assert_eq!(revisions(&regen), [Some(3)]);
}

#[test]
fn a_failing_draft_says_why_for_its_revision_only() {
    let (editor, (feature, mut extrude)) = plate_draft();
    extrude.regions.clear();
    let (mut feed, regen) = connected();
    feed.request_with(&editor, None, drafted((feature, extrude.clone())), None);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert!(feed.draft_error().is_some());
    // Answered with the model without it.
    assert_eq!(feed.shown_draft(), Some(1));
    assert_eq!(feed.mesh().triangle_count(), 0);
    // Another draft asked for, the error is no longer its.
    extrude.flip = true;
    feed.request_with(&editor, None, drafted((feature, extrude)), None);
    assert_eq!(feed.draft_error(), None);
    // The panel keeps the one before until the answer, or SLOW passes.
    assert!(feed.shown_draft_error().is_some());
    let now = Instant::now();
    feed.tick(&editor, now);
    assert!(feed.shown_draft_error().is_some());
    feed.tick(&editor, now + SLOW);
    assert_eq!(feed.shown_draft_error(), None);
}

#[test]
fn a_draft_dropped_is_regenerating_until_the_model_without_it_shows() {
    let (editor, draft) = plate_draft();
    let (mut feed, regen) = connected();
    feed.request_with(&editor, None, drafted(draft.clone()), None);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
    assert!(feed.mesh().triangle_count() > 0);

    // Cancelled: the draft's model still shows, which isn't the
    // document's.
    feed.request_with(&editor, None, None, None);
    assert_eq!(feed.status(&editor), MeshStatus::Regenerating);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.status(&editor), MeshStatus::Current);

    // Another draft likewise, until its answer.
    feed.request_with(&editor, None, drafted(draft), None);
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
    feed.request_with(&editor, None, drafted((None, join.clone())), None);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.draft_touched(), [body]);

    // A draft the document refuses, its sketch the extrude, keeps the
    // list: its touch test didn't run.
    let mut refused = join.clone();
    refused.sketch = plate;
    feed.request_with(&editor, None, drafted((None, refused)), None);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert!(feed.draft_error().is_some());
    assert_eq!(feed.draft_touched(), [body]);

    // Without a draft, unanswered, then another: a new run, listing
    // nothing until its answer.
    feed.request_with(&editor, None, None, None);
    regen.take();
    feed.request_with(&editor, None, drafted((None, join.clone())), None);
    assert_eq!(feed.draft_touched(), []);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.draft_touched(), [body]);

    // Another feature's draft, with none in between, likewise.
    feed.request_with(&editor, None, drafted((Some(plate), join)), None);
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
        detail: None,
        generation: editor.generation(),
        exclude: None,
        draft: None,
        inspect: None,
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

/// The body of each of the mesh's parts is kept with the mesh, through a
/// failure shown next to it; across a replacement of the document it's
/// given out as none, as the model's other ids are, until a model of the
/// new document shows: a body's id may name another, of another opacity.
#[test]
fn the_parts_bodies_follow_the_mesh_shown() {
    let mut editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let (mut feed, regen) = connected();
    assert!(feed.parts().is_empty());
    feed.request(&editor, None);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.parts(), [body]);
    assert_eq!(feed.mesh().part_ends().len(), 1);

    editor.apply(Command::SetVisible(body, false)).unwrap();
    feed.request(&editor, None);
    let request = regen.take().pop().unwrap();
    feed.apply(Response::Failed {
        detail: None,
        generation: request.generation().unwrap(),
        exclude: None,
        draft: None,
        inspect: None,
        error: "no".to_owned(),
    });
    assert_eq!(feed.parts(), [body]);

    // Replaced, as restoring recovered changes does.
    let mut other = Editor::new(Document::example());
    let faint = varde_document::Opacity::MIN;
    other.apply(Command::SetOpacity(body, faint)).unwrap();
    let document = other.document().clone();
    editor.apply(Command::Replace(Box::new(document))).unwrap();
    feed.replaced(editor.generation());
    assert_eq!(feed.parts(), []);
    feed.request(&editor, None);
    feed.apply(handle(regen.take().pop().unwrap()));
    assert_eq!(feed.parts(), [editor.document().bodies()[0].id]);
}

/// A small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Adds a sketch on XY holding the block from `x` to `x + width` along
/// x and 0 to 10 along y, and an extrude of it `height` up with
/// `operation`.
fn add_block(editor: &mut Editor, x: f64, width: f64, height: &str, operation: Operation) {
    use varde_document::{Extent, OriginPlane, Plane};
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = varde_sketch::Sketch::default();
    let corners = [(x, 0.0), (x + width, 0.0), (x + width, 10.0), (x, 10.0)]
        .map(|(x, y)| sketch.add_point(glam::DVec2::new(x, y)).unwrap());
    for (k, &start) in corners.iter().enumerate() {
        let end = corners[(k + 1) % 4];
        let line = varde_sketch::Curve::Line { start, end };
        sketch.add_curve(line, false).unwrap();
    }
    let profiles = sketch.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let ask = Extent::ask(&editor.document().design());
    let extent = Extent::OneSide(varde_expr::Value::new(height, &ask).unwrap());
    let extrude = varde_document::Extrude {
        taper: None,
        sketch: feature,
        regions,
        extent,
        flip: false,
        operation,
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
}

/// A combine of some of `document`'s bodies, picked by `rng`: any of
/// them, consumed or not (one naming a consumed body fails, as regen
/// says), with a random operation and Keep tools. `None` with fewer than
/// two bodies.
fn random_combine(document: &Document, rng: &mut Rng) -> Option<varde_document::Combine> {
    use varde_document::BodyOp;
    let bodies: Vec<BodyId> = document.bodies().iter().map(|body| body.id).collect();
    if bodies.len() < 2 {
        return None;
    }
    let target = bodies[rng.below(bodies.len())];
    let mut tools: Vec<BodyId> = (bodies.iter().copied())
        .filter(|&body| body != target && rng.below(2) == 0)
        .collect();
    if tools.is_empty() {
        let others: Vec<BodyId> = bodies.into_iter().filter(|&b| b != target).collect();
        tools.push(others[rng.below(others.len())]);
    }
    let op = [BodyOp::Union, BodyOp::Subtract, BodyOp::Intersect][rng.below(3)];
    Some(varde_document::Combine {
        target,
        tools,
        op,
        keep_tools: rng.below(3) == 0,
    })
}

/// On random histories of blocks made as bodies, joins over them and
/// combines (later ones naming bodies earlier ones used up, some
/// failing), and edits of earlier combines: the app's replay of the
/// merges agrees with regen's evaluation, of the whole history
/// (`merged_bodies`) and stopped before each feature (regen evaluating
/// the history with the features from there on removed).
#[test]
fn merged_before_agrees_with_regen_on_random_histories() {
    for seed in varde_testing::seeds(1, 4) {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ (seed + 1).wrapping_mul(0x1000_0001));
        let mut editor = Editor::new(Document::default());
        let mut regen = varde_regen::Regenerator::default();
        // For the histories stopped early: shared, as their features are.
        let mut cache = varde_regen::Cache::default();
        let (mut feed, requests) = connected();
        // Where the blocks are, every 8 mm, each 10 mm long: neighbours
        // overlap.
        let mut places: Vec<f64> = Vec::new();
        for step in 0..14 {
            let what = format!("seed {seed} step {step}");
            let document = editor.document();
            // Three bodies first.
            let roll = if step < 3 { 0 } else { rng.below(6) };
            match roll {
                0 | 1 => {
                    let x = 8.0 * rng.below(6) as f64;
                    places.push(x);
                    let height = ["5", "10"][rng.below(2)];
                    add_block(
                        &mut editor,
                        x,
                        10.0,
                        height,
                        Operation::NewBody(BodyId::NEW),
                    );
                }
                2 => {
                    // From a block's place over the next one or two: it
                    // merges what it touches.
                    let x = places[rng.below(places.len())] + 4.0;
                    let join = Operation::Join(varde_document::Targets::default());
                    add_block(&mut editor, x, 8.0 * (1 + rng.below(2)) as f64, "7", join);
                }
                3 | 4 => {
                    if let Some(combine) = random_combine(document, &mut rng) {
                        editor
                            .apply(document.add_feature(combine.into()))
                            .unwrap_or_else(|error| panic!("{what}: {error}"));
                    }
                }
                _ => {
                    // An earlier combine edited: its keep and operation
                    // changed, or its bodies picked again.
                    let combines: Vec<FeatureId> = (document.features().iter())
                        .filter(|f| matches!(f.kind, FeatureKind::Combine(_)))
                        .map(|f| f.id)
                        .collect();
                    if combines.is_empty() {
                        continue;
                    }
                    let id = combines[rng.below(combines.len())];
                    let index = document.features().iter().position(|f| f.id == id).unwrap();
                    let FeatureKind::Combine(mut combine) = document.features()[index].kind.clone()
                    else {
                        unreachable!()
                    };
                    combine.keep_tools = !combine.keep_tools;
                    if rng.below(2) == 0 {
                        combine.op = random_combine(document, &mut rng).unwrap().op;
                    }
                    // Refused where it names a body made after it.
                    let _ = editor.apply(Command::SetFeature {
                        feature: id,
                        kind: Box::new(combine.into()),
                    });
                }
            }
            feed.request(&editor, None);
            // Nothing changed: nothing asked.
            let Some(request) = requests.borrow_mut().pop() else {
                continue;
            };
            feed.apply(regen.handle(request));
            assert_eq!(feed.generation(), Some(editor.generation()), "{what}");
            let document = editor.document();
            let bodies: Vec<BodyId> = document.bodies().iter().map(|b| b.id).collect();
            // The whole history.
            let replayed = feed.merged_before(document, None);
            for &body in &bodies {
                let regen = (feed.merged_bodies().iter())
                    .find(|(consumed, _)| *consumed == body)
                    .map(|&(_, holder)| holder);
                assert_eq!(replayed.holder(body), regen, "{what}: body {body:?}");
            }
            // Stopped before each feature.
            let features: Vec<FeatureId> = document.features().iter().map(|f| f.id).collect();
            for (at, &until) in features.iter().enumerate() {
                let mut before = Editor::new(document.clone());
                for &later in features[at..].iter().rev() {
                    if before.document().feature(later).is_some() {
                        before.apply(Command::RemoveFeature(later)).unwrap();
                    }
                }
                let evaluation = varde_regen::evaluate(before.document(), &mut cache);
                let replayed = feed.merged_before(document, Some(until));
                for &body in &bodies {
                    let regen = (evaluation.merged.iter())
                        .find(|(consumed, _)| *consumed == body)
                        .map(|&(_, holder)| holder);
                    assert_eq!(
                        replayed.holder(body),
                        regen,
                        "{what}: before feature {at}, body {body:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_slow_regeneration_shows_with_its_progress_until_answered() {
    let mut editor = one_line();
    let (mut feed, regen) = connected();
    feed.request(&editor, None);
    feed.apply(handle(regen.borrow_mut().remove(0)));
    let start = Instant::now();
    feed.tick(&editor, start);
    assert!(!feed.timing(&editor), "nothing to time while current");

    add_line(&mut editor);
    feed.request(&editor, None);
    assert!(feed.timing(&editor));
    feed.tick(&editor, start);
    assert_eq!(feed.slow(&editor), None, "not shown at once");
    assert!(feed.timing(&editor));

    feed.tick(&editor, start + SLOW);
    assert_eq!(feed.slow(&editor), Some(None), "shown before any progress");
    assert!(!feed.timing(&editor), "no frames while it shows");
    let progress = Progress {
        step: 0,
        steps: 2,
        stage: varde_regen::Stage::Feature("Sketch 1".to_owned()),
    };
    feed.apply(Response::Progress(progress.clone()));
    assert_eq!(feed.slow(&editor), Some(Some(&progress)));
    assert_eq!(feed.status(&editor), MeshStatus::Regenerating);

    // Answered: gone at once, and one more frame starts the clock again.
    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.slow(&editor), None);
    assert!(feed.timing(&editor));
    feed.tick(&editor, start + SLOW * 2);
    assert!(!feed.timing(&editor));

    add_line(&mut editor);
    feed.request(&editor, None);
    feed.tick(&editor, start + SLOW * 3);
    assert_eq!(feed.slow(&editor), None, "the next waits its turn too");
}

#[test]
fn the_view_asks_for_the_model_in_levels_without_regenerating() {
    let editor = one_line();
    let (mut feed, regen) = connected();
    // Not before the first model, which frames the camera.
    feed.view(100.0);
    assert_eq!(feed.detail(), None);
    feed.request(&editor, None);
    feed.apply(handle(regen.borrow_mut().remove(0)));

    // 100 mm tall asks for chords of 2^-4 mm (0.05 rounded).
    feed.view(100.0);
    assert_eq!(feed.detail(), Some(Detail(-4)));
    feed.request(&editor, None);
    let asked = regen.borrow_mut().remove(0);
    assert_eq!(asked.detail(), Some(Detail(-4)));
    // The model shown stays, current, while the finer one comes.
    assert_eq!(feed.status(&editor), MeshStatus::Current);
    assert_eq!(feed.generation(), Some(Generation::from(0)));

    // A little zoom asks for nothing new; past the slack, the next level.
    feed.view(130.0);
    feed.request(&editor, None);
    assert!(regen.borrow().is_empty());
    feed.view(40.0);
    assert_eq!(feed.detail(), Some(Detail(-6)));
    feed.request(&editor, None);
    let newest = regen.borrow_mut().remove(0);

    // The answer for the level let go of is dropped, the newest taken.
    let model = feed.model();
    feed.apply(handle(asked));
    assert_eq!(feed.model(), model);
    feed.apply(handle(newest));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
    assert_eq!(feed.shown.and_then(|shown| shown.detail), Some(Detail(-6)));
}
