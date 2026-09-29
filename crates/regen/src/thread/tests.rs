use varde_document::{Command, Document, Editor, FeatureId, FeatureKind, OriginPlane, Plane};
use varde_lane::thread::testing::{join_in_time, next};

use super::*;
use crate::{Transport, handle};

fn regenerate(editor: &Editor) -> Request {
    Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: None,
    }
}

/// An editor, at generation 0, on a document holding an empty sketch on
/// XY, and the sketch's id.
fn with_sketch() -> (Editor, FeatureId) {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let feature = editor.document().features()[0].id;
    (Editor::new(editor.document().clone()), feature)
}

/// Adds a line to the sketch `feature`, as one edit, so the lines
/// answered for each generation are as many as it is.
fn add_line(editor: &mut Editor, feature: FeatureId) {
    let FeatureKind::Sketch { sketch, .. } = &editor.document().features()[0].kind else {
        panic!("the first feature is a sketch");
    };
    let mut sketch = sketch.clone();
    let y = sketch.curves.len() as f64;
    let start = sketch.add_point(glam::DVec2::new(0.0, y)).unwrap();
    let end = sketch.add_point(glam::DVec2::new(1.0, y)).unwrap();
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
fn round_trip() {
    let (editor, _) = crate::tests::sketched();
    let (mut lane, mut responses) = spawn();
    lane.send(regenerate(&editor));
    let Response::Regenerated {
        generation,
        sketches,
        ..
    } = next(&mut responses)
    else {
        panic!("regeneration failed");
    };
    assert_eq!(generation, editor.generation());
    assert_eq!(
        *sketches,
        crate::flatten_sketches(editor.document(), None).unwrap()
    );
}

#[test]
fn burst_ends_with_the_newest() {
    let (mut editor, feature) = with_sketch();
    let (mut lane, mut responses) = spawn();
    for _ in 0..20 {
        add_line(&mut editor, feature);
        lane.send(regenerate(&editor));
    }
    // Some may be skipped, but those that arrive are in order, and the last
    // is the newest.
    let mut last = 0;
    while last < u64::from(editor.generation()) {
        let Response::Regenerated {
            generation,
            sketches,
            ..
        } = next(&mut responses)
        else {
            panic!("regeneration failed");
        };
        let generation = u64::from(generation);
        assert!(generation > last);
        assert_eq!(sketches.ends().len() as u64, generation);
        last = generation;
    }
}

#[test]
fn thread_ends_when_responses_are_dropped() {
    let editor = Editor::new(Document::example());
    let (mut lane, responses) = spawn();
    let join = responses.close();
    // The lane handle outliving the stream doesn't keep the thread alive.
    lane.send(regenerate(&editor));
    join_in_time(join);
}

#[test]
fn thread_ends_when_idle_and_dropped() {
    let (lane, responses) = spawn();
    drop(lane);
    join_in_time(responses.close());
}

/// Tessellates, except that generation 1 panics.
fn panics_on_generation_1(request: Request) -> Response {
    assert_ne!(u64::from(request.generation()), 1, "the kernel failed");
    handle(request)
}

#[test]
fn lane_keeps_going_after_a_job_panics() {
    let (mut editor, feature) = with_sketch();
    let (mut lane, mut responses) = spawn_on(panics_on_generation_1);
    add_line(&mut editor, feature);
    lane.send(regenerate(&editor));
    assert!(matches!(
        next(&mut responses),
        Response::Failed { generation, .. } if u64::from(generation) == 1
    ));

    add_line(&mut editor, feature);
    lane.send(regenerate(&editor));
    let Response::Regenerated {
        generation,
        sketches,
        ..
    } = next(&mut responses)
    else {
        panic!("generation 2 failed");
    };
    assert_eq!(u64::from(generation), 2);
    assert_eq!(sketches.ends().len(), 2);
}
