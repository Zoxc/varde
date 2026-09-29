use std::pin::Pin;
use std::sync::mpsc;
use std::task::{Context, Poll, Waker};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use futures::Stream;
use glam::Vec3;
use varde_document::{Command, Document, Editor};
use varde_kernel::Shape;

use super::*;
use crate::Transport;

/// How long a test waits for the lane before failing.
const TIMEOUT: Duration = Duration::from_secs(10);

fn regenerate(editor: &Editor) -> Request {
    Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
    }
}

fn add_cube(editor: &mut Editor) {
    editor
        .apply(Command::AddBody {
            name: "Cube".to_owned(),
            shape: Shape::cuboid(Vec3::splat(1.0)),
            position: Vec3::ZERO,
        })
        .unwrap();
}

/// The next response, polled without an executor.
fn next(responses: &mut Responses) -> Response {
    let start = Instant::now();
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        match Pin::new(&mut *responses).poll_next(&mut cx) {
            Poll::Ready(Some(response)) => return response,
            Poll::Ready(None) => panic!("the lane ended"),
            Poll::Pending if start.elapsed() < TIMEOUT => {
                std::thread::sleep(Duration::from_millis(1))
            }
            Poll::Pending => panic!("no response from the lane"),
        }
    }
}

fn join_in_time(join: JoinHandle<()>) {
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || done.send(join.join().is_ok()));
    assert_eq!(
        finished.recv_timeout(TIMEOUT),
        Ok(true),
        "the lane didn't end"
    );
}

#[test]
fn round_trip() {
    let editor = Editor::new(Document::example());
    let (mut lane, mut responses) = spawn();
    lane.send(regenerate(&editor));
    let Response::Regenerated { generation, mesh } = next(&mut responses) else {
        panic!("regeneration failed");
    };
    assert_eq!(generation, editor.generation());
    assert_eq!(*mesh, crate::tessellate(editor.document()).unwrap());
}

#[test]
fn burst_ends_with_the_newest() {
    let mut editor = Editor::new(Document::default());
    let (mut lane, mut responses) = spawn();
    for _ in 0..20 {
        add_cube(&mut editor);
        lane.send(regenerate(&editor));
    }
    // Some may be skipped, but those that arrive are in order, and the last
    // is the newest.
    let mut last = 0;
    while last < u64::from(editor.generation()) {
        let Response::Regenerated { generation, mesh } = next(&mut responses) else {
            panic!("regeneration failed");
        };
        let generation = u64::from(generation);
        assert!(generation > last);
        assert_eq!(mesh.triangle_count() as u64, generation * 12);
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
    let mut editor = Editor::new(Document::default());
    let (mut lane, mut responses) = spawn_on(panics_on_generation_1);
    add_cube(&mut editor);
    lane.send(regenerate(&editor));
    assert!(matches!(
        next(&mut responses),
        Response::Failed { generation, .. } if u64::from(generation) == 1
    ));

    add_cube(&mut editor);
    lane.send(regenerate(&editor));
    let Response::Regenerated { generation, mesh } = next(&mut responses) else {
        panic!("generation 2 failed");
    };
    assert_eq!(u64::from(generation), 2);
    assert_eq!(mesh.triangle_count(), 24);
}
