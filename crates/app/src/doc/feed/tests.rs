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
        .map(|Request::Regenerate { generation, .. }| u64::from(*generation))
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
        generation: request.generation(),
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
        .map(|request| u64::from(request.generation()))
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
    let Request::Regenerate { exclude, .. } = &request;
    assert_eq!(*exclude, None);
    feed.apply(handle(request));
    assert_eq!(feed.sketches().points(), [[0.0; 3], [1.0, 2.0, 0.0]]);

    // Kept while the next generation fails, like the mesh.
    add_sketch(&mut editor);
    feed.request(&editor, None);
    let request = regen.borrow_mut().remove(0);
    feed.apply(Response::Failed {
        generation: request.generation(),
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
    requests
        .borrow()
        .iter()
        .map(|Request::Regenerate { exclude, .. }| *exclude)
        .collect()
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
        .map(|request| u64::from(request.generation()))
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
        generation: request.generation(),
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
        generation: request.generation(),
        exclude: None,
        error: "again".to_owned(),
    });
    assert_eq!(feed.status(&editor), MeshStatus::Failed("again"));
    feed.apply(handle(request));
    assert_eq!(feed.left_out(), Some(Some(feature)));
}
