use std::collections::VecDeque;
use std::sync::mpsc;
use std::task::Waker;
use std::time::{Duration, Instant};

use super::*;

/// How long a test waits for the lane before failing.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Requests in the order sent, except that `0` is refused.
#[derive(Default)]
struct Fifo(VecDeque<u32>);

impl Pending<u32> for Fifo {
    fn push(&mut self, request: u32) -> Option<u32> {
        if request == 0 {
            return Some(request);
        }
        self.0.push_back(request);
        None
    }

    fn pop(&mut self) -> Option<u32> {
        self.0.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The answer to a panic in tests that don't expect one.
fn no_panic<S>(_: &u32) -> fn(String) -> S {
    |error| panic!("the lane panicked: {error}")
}

/// The next response, polled without an executor.
fn next<S>(responses: &mut Responses<u32, S>) -> S {
    let start = Instant::now();
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        match Pin::new(&mut *responses).poll_next(&mut cx) {
            Poll::Ready(Some(response)) => return response,
            Poll::Ready(None) => panic!("the lane ended"),
            Poll::Pending if start.elapsed() < TIMEOUT => thread::sleep(Duration::from_millis(1)),
            Poll::Pending => panic!("no response from the lane"),
        }
    }
}

fn join_in_time(join: JoinHandle<()>) {
    let (done, finished) = mpsc::channel();
    thread::spawn(move || done.send(join.join().is_ok()));
    assert_eq!(
        finished.recv_timeout(TIMEOUT),
        Ok(true),
        "the lane didn't end"
    );
}

#[test]
fn answers_in_the_order_taken() {
    let (mut lane, mut responses) =
        spawn("test", Fifo::default(), OnClose::Stop, |n| n * 2, no_panic);
    lane.send(0);
    lane.send(1);
    lane.send(2);
    assert_eq!(next(&mut responses), 2);
    assert_eq!(next(&mut responses), 4);
}

#[test]
fn a_panic_is_answered_and_the_lane_goes_on() {
    let (mut lane, mut responses) = spawn(
        "test",
        Fifo::default(),
        OnClose::Stop,
        |n| {
            if n == 1 {
                panic!("on purpose");
            }
            Ok(n)
        },
        |&n| move |error| Err((n, error)),
    );
    lane.send(1);
    lane.send(2);
    assert_eq!(
        next(&mut responses),
        Err((1, "internal error: on purpose".to_owned()))
    );
    assert_eq!(next(&mut responses), Ok(2));
}

/// Starts a lane whose first request waits for the test to say go, and
/// returns what it handled. `sent` is sent while the first one runs, then
/// the responses are dropped.
fn close_while_busy(on_close: OnClose, sent: &[u32]) -> Vec<u32> {
    let (started, running) = mpsc::channel();
    let (go, wait) = mpsc::channel::<()>();
    let (handled, done) = mpsc::channel();
    let (mut lane, responses) = spawn(
        "test",
        Fifo::default(),
        on_close,
        move |n| {
            if n == 1 {
                let _ = started.send(());
                let _ = wait.recv_timeout(TIMEOUT);
            }
            let _ = handled.send(n);
        },
        no_panic,
    );
    lane.send(1);
    running.recv_timeout(TIMEOUT).unwrap();
    for &n in sent {
        lane.send(n);
    }
    let join = responses.close();
    go.send(()).unwrap();
    join_in_time(join);
    done.try_iter().collect()
}

#[test]
fn draining_handles_what_is_pending_after_closing() {
    assert_eq!(close_while_busy(OnClose::Drain, &[2, 3]), [1, 2, 3]);
}

#[test]
fn stopping_drops_what_is_pending_after_closing() {
    assert_eq!(close_while_busy(OnClose::Stop, &[2, 3]), [1]);
}

#[test]
fn thread_ends_when_idle_and_dropped() {
    let (lane, responses) = spawn("test", Fifo::default(), OnClose::Drain, |n| n, no_panic);
    drop(lane);
    join_in_time(responses.close());
}

#[test]
fn a_lane_outliving_its_responses_does_not_keep_the_thread() {
    let (mut lane, responses) = spawn("test", Fifo::default(), OnClose::Stop, |n| n, no_panic);
    let join = responses.close();
    lane.send(1);
    join_in_time(join);
}
