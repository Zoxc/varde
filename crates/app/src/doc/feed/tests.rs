use std::cell::RefCell;
use std::rc::Rc;

use glam::Vec3;
use varde_document::{Command, Document};
use varde_kernel::Shape;

use varde_regen::handle;

use super::*;

/// Keeps requests for the test to answer when and in what order it likes.
struct Deferred(Rc<RefCell<Vec<Request>>>);

impl Transport<Request> for Deferred {
    fn send(&mut self, request: Request) {
        self.0.borrow_mut().push(request);
    }
}

/// A feed sending to a lane that keeps its requests, and the list they're
/// kept in.
fn connected() -> (MeshFeed, Rc<RefCell<Vec<Request>>>) {
    let requests = Rc::default();
    let mut feed = MeshFeed::new();
    feed.connect(Deferred(Rc::clone(&requests)));
    (feed, requests)
}

fn add_cube(editor: &mut Editor) {
    let offset = editor.document().bodies().len() as f32 * 3.0;
    editor
        .apply(Command::AddBody {
            name: "Cube".to_owned(),
            shape: Shape::cuboid(Vec3::splat(2.0)),
            position: Vec3::X * offset,
        })
        .unwrap();
}

#[test]
fn requests_only_newer_generations() {
    let mut editor = Editor::new(Document::example());
    let (mut feed, regen) = connected();

    feed.request(&editor);
    feed.request(&editor);
    assert_eq!(regen.borrow().len(), 1);

    add_cube(&mut editor);
    feed.request(&editor);
    let generations: Vec<_> = regen
        .borrow()
        .iter()
        .map(|Request::Regenerate { generation, .. }| u64::from(*generation))
        .collect();
    assert_eq!(generations, [0, 1]);
}

#[test]
fn undo_is_regenerated_and_shown() {
    let mut editor = Editor::new(Document::example());
    let (mut feed, regen) = connected();
    add_cube(&mut editor);
    feed.request(&editor);
    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.mesh().triangle_count(), 24);

    // Back to a state seen before, but a newer generation: asked for and
    // shown again rather than taken for the older mesh.
    editor.undo();
    feed.request(&editor);
    assert_eq!(feed.status(&editor), MeshStatus::Regenerating);
    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
    assert_eq!(feed.mesh().triangle_count(), 12);
}

#[test]
fn keeps_the_last_mesh_while_regenerating() {
    let mut editor = Editor::new(Document::example());
    let (mut feed, regen) = connected();
    feed.request(&editor);
    feed.apply(handle(regen.borrow_mut().remove(0)));

    add_cube(&mut editor);
    feed.request(&editor);
    assert_eq!(feed.status(&editor), MeshStatus::Regenerating);
    assert_eq!(feed.generation(), Some(Generation::from(0)));
    assert_eq!(feed.mesh().triangle_count(), 12);

    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
    assert_eq!(feed.mesh().triangle_count(), 24);
}

#[test]
fn drops_out_of_order_and_superseded_responses() {
    let mut editor = Editor::new(Document::example());
    let (mut feed, regen) = connected();
    for _ in 0..3 {
        feed.request(&editor);
        add_cube(&mut editor);
    }
    feed.request(&editor);
    let mut responses: Vec<_> = regen.take().into_iter().map(handle).collect();
    let newest = responses.pop().unwrap();
    let older = responses.pop().unwrap();

    // The newest arrives first; the one it superseded is dropped after it.
    feed.apply(newest.clone());
    feed.apply(older);
    assert_eq!(feed.generation(), Some(Generation::from(3)));
    assert_eq!(feed.mesh().triangle_count(), 48);

    // A repeat of what's shown changes nothing either.
    feed.apply(newest);
    assert_eq!(feed.generation(), Some(Generation::from(3)));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
}

#[test]
fn failure_ends_regenerating_and_keeps_the_last_mesh() {
    let mut editor = Editor::new(Document::example());
    let (mut feed, regen) = connected();
    feed.request(&editor);
    feed.apply(handle(regen.borrow_mut().remove(0)));

    add_cube(&mut editor);
    feed.request(&editor);
    let request = regen.borrow_mut().remove(0);
    feed.apply(Response::Failed {
        generation: request.generation(),
        error: "the kernel failed".to_owned(),
    });
    assert_eq!(
        feed.status(&editor),
        MeshStatus::Failed("the kernel failed")
    );
    assert_eq!(feed.generation(), Some(Generation::from(0)));
    assert_eq!(feed.mesh().triangle_count(), 12);

    // A late mesh for the generation that failed changes nothing.
    feed.apply(handle(request));
    assert_eq!(feed.generation(), Some(Generation::from(0)));

    // The next edit hides the error while it regenerates, then clears it.
    add_cube(&mut editor);
    feed.request(&editor);
    assert_eq!(feed.status(&editor), MeshStatus::Regenerating);
    feed.apply(handle(regen.borrow_mut().remove(0)));
    assert_eq!(feed.status(&editor), MeshStatus::Current);
    assert_eq!(feed.mesh().triangle_count(), 36);
}

/// Nothing is asked for before the lane has started, and then the
/// editor's newest generation.
#[test]
fn requests_nothing_until_connected() {
    let mut editor = Editor::new(Document::example());
    let mut feed = MeshFeed::new();
    feed.request(&editor);
    add_cube(&mut editor);
    feed.request(&editor);
    assert!(!feed.connected());

    let requests = Rc::default();
    feed.connect(Deferred(Rc::clone(&requests)));
    feed.request(&editor);
    let generations: Vec<_> = requests
        .borrow()
        .iter()
        .map(|request| u64::from(request.generation()))
        .collect();
    assert_eq!(generations, [1]);
}
