//! What tests of a lane's thread share: waiting for its responses, and
//! for it to end, each within [`TIMEOUT`] rather than hanging. Built for
//! other crates' tests with the `testing` feature.

use std::pin::Pin;
use std::sync::mpsc;
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures::Stream;

use super::Responses;

/// How long a test waits for the lane before failing.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// The next response, polled without an executor.
pub fn next<R, S>(responses: &mut Responses<R, S>) -> S {
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

/// Waits for the lane's thread, `join`, to end, and for it not to have
/// panicked.
pub fn join_in_time(join: JoinHandle<()>) {
    let (done, finished) = mpsc::channel();
    thread::spawn(move || done.send(join.join().is_ok()));
    assert_eq!(
        finished.recv_timeout(TIMEOUT),
        Ok(true),
        "the lane didn't end"
    );
}
