use varde_document::{Command, Document, Editor, FeatureId, FeatureKind, OriginPlane, Plane};
use varde_lane::thread::testing::join_in_time;

use super::*;
use crate::{Asked, Transport, handle};

/// The next response that isn't a [`Response::Progress`].
fn next(responses: &mut Responses) -> Response {
    loop {
        match varde_lane::thread::testing::next(responses) {
            Response::Progress(_) => {}
            response => return response,
        }
    }
}

fn regenerate(editor: &Editor) -> Request {
    Request::Regenerate {
        sight: None,
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        until: None,
        draft: None,
        inspect: None,
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
        asked: Asked { generation, .. },
        sketches,
        ..
    } = next(&mut responses)
    else {
        panic!("regeneration failed");
    };
    assert_eq!(generation, editor.generation());
    assert_eq!(
        *sketches,
        crate::flatten_sketches(editor.document(), &[], None).unwrap()
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
            asked: Asked { generation, .. },
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
    assert_ne!(
        request.asked().map(|asked| asked.generation).map(u64::from),
        Some(1),
        "the kernel failed"
    );
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
        Response::Failed { asked: Asked { generation, .. }, .. } if u64::from(generation) == 1
    ));

    add_line(&mut editor, feature);
    lane.send(regenerate(&editor));
    let Response::Regenerated {
        asked: Asked { generation, .. },
        sketches,
        ..
    } = next(&mut responses)
    else {
        panic!("generation 2 failed");
    };
    assert_eq!(u64::from(generation), 2);
    assert_eq!(sketches.ends().len(), 2);
}

/// Answers like the lane, slowly, so requests sent meanwhile wait.
fn slowly(request: Request) -> Response {
    std::thread::sleep(std::time::Duration::from_millis(50));
    handle(request)
}

#[test]
fn an_export_edge_facts_and_a_measure_are_answered_though_regenerations_follow_them() {
    let (mut editor, feature) = with_sketch();
    let (mut lane, mut responses) = spawn_on(slowly);
    // One running, then an export and a burst of edits waiting behind it:
    // the edits replace each other, never the export.
    lane.send(regenerate(&editor));
    lane.send(Request::Export {
        export: 1,
        document: editor.snapshot(),
    });
    // And edge facts behind the export, also never replaced.
    lane.send(Request::Query {
        tag: 2,
        generation: editor.generation(),
        document: editor.snapshot(),
        until: None,
        query: crate::Query::Topology(varde_document::BodyId::NEW),
    });
    // And a client's measure, beside the user's riding on the edits.
    let pick = crate::InspectPick {
        body: varde_document::BodyId::NEW,
        entity: crate::Entity::Body,
        near: [0.0; 3],
    };
    lane.send(Request::Query {
        tag: 3,
        generation: editor.generation(),
        document: editor.snapshot(),
        until: None,
        query: crate::Query::Measure(Box::new(crate::Inspect {
            revision: 1,
            first: pick,
            second: None,
        })),
    });
    for _ in 0..5 {
        add_line(&mut editor, feature);
        lane.send(regenerate(&editor));
    }
    let mut exported = false;
    let mut topology = false;
    let mut measured = false;
    loop {
        match next(&mut responses) {
            Response::Exported { export, result } => {
                assert_eq!(export, 1);
                // Nothing visible to weld in a sketch.
                assert_eq!(result, Ok(Vec::new()));
                assert!(!exported, "answered twice");
                exported = true;
            }
            Response::Answered {
                tag,
                answer: crate::Answer::Topology { result, .. },
                ..
            } => {
                assert_eq!(tag, 2);
                assert!(exported, "edge facts answered ahead of the export");
                // A sketch makes no body.
                assert!(result.is_err());
                assert!(!topology, "answered twice");
                topology = true;
            }
            Response::Answered {
                tag,
                answer: crate::Answer::Measured(result),
                ..
            } => {
                assert_eq!(tag, 3);
                assert!(topology, "the measure answered ahead of the edge facts");
                // A sketch makes no body.
                assert!(result.unwrap().first.is_err());
                assert!(!measured, "answered twice");
                measured = true;
            }
            Response::Regenerated {
                asked: Asked { generation, .. },
                ..
            } if generation == editor.generation() => {
                break;
            }
            Response::Regenerated { .. } => {}
            Response::Failed { error, .. } => panic!("{error}"),
            Response::Progress(_) => unreachable!("skipped by next"),
        }
    }
    assert!(exported && topology && measured);
}

#[test]
fn progress_comes_ahead_of_the_answer() {
    let (editor, _) = crate::tests::sketched();
    let features = editor.document().features();
    let (mut lane, mut responses) = spawn();
    lane.send(regenerate(&editor));
    let mut told = Vec::new();
    let answer = loop {
        match varde_lane::thread::testing::next(&mut responses) {
            Response::Progress(progress) => told.push(progress),
            answer => break answer,
        }
    };
    assert!(matches!(answer, Response::Regenerated { .. }));
    let steps = u32::try_from(features.len()).unwrap() + 1;
    let mut expected: Vec<_> = (features.iter().zip(0..))
        .map(|(feature, step)| crate::Progress {
            step,
            steps,
            stage: crate::Stage::Feature(feature.name.clone()),
        })
        .collect();
    expected.push(crate::Progress {
        step: steps - 1,
        steps,
        stage: crate::Stage::Drawing,
    });
    assert_eq!(told, expected);
}
