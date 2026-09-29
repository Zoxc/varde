//! A lane natively: a thread handling requests one at a time.
//!
//! Requests wait in the lane's [`Pending`], which decides the order they're
//! taken in and which replace others; responses come back on a
//! [`Responses`] stream. The sending side never waits for work: it only
//! holds the pending requests' lock to push, which the thread never holds
//! for longer. Once the [`Responses`] are dropped the thread ends, as
//! [`OnClose`] says.

use std::fmt;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};
use std::thread::{self, JoinHandle};

use futures::Stream;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};

use crate::{Pending, Transport, panic};

/// What the thread does once nobody listens any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnClose {
    /// Handles what's still pending first, so a last write isn't lost.
    Drain,
    /// Ends once the request running, if any, is done.
    Stop,
}

/// Starts a lane on a new thread called `name`, answering each request
/// with `handle`. A request `handle` panics on is answered by what
/// `failed` makes of it, taken before the request is handed over, called
/// with the panic's message (see [`crate::panic`]), and the lane goes on.
/// Send requests through the [`Lane`], read responses from [`Responses`];
/// dropping the latter ends the thread as `on_close` says, which drops
/// `handle` and whatever it owns.
pub fn spawn<R, S, F>(
    name: &str,
    pending: impl Pending<R> + Send + 'static,
    on_close: OnClose,
    mut handle: impl FnMut(R) -> S + Send + 'static,
    failed: impl Fn(&R) -> F + Send + 'static,
) -> (Lane<R>, Responses<R, S>)
where
    R: Send + 'static,
    S: Send + 'static,
    F: FnOnce(String) -> S,
{
    spawn_answering(
        name,
        pending,
        on_close,
        move |request| [handle(request)],
        failed,
    )
}

/// [`spawn`], with `handle` answering each request with any number of
/// responses, in order, such as its answer followed by news it brought
/// about. A request it panics on is answered by `failed` alone.
pub fn spawn_answering<R, S, A, F>(
    name: &str,
    pending: impl Pending<R> + Send + 'static,
    on_close: OnClose,
    mut handle: impl FnMut(R) -> A + Send + 'static,
    failed: impl Fn(&R) -> F + Send + 'static,
) -> (Lane<R>, Responses<R, S>)
where
    R: Send + 'static,
    S: Send + 'static,
    A: IntoIterator<Item = S>,
    F: FnOnce(String) -> S,
{
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            pending: Box::new(pending),
            closed: false,
        }),
        wake: Condvar::new(),
    });
    let (sender, receiver) = unbounded();
    let worker = Arc::clone(&shared);
    let thread = thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            while let Some(request) = worker.next(on_close) {
                let failed = failed(&request);
                let responses = panic::catch(
                    |request| handle(request).into_iter().collect(),
                    request,
                    |error| vec![failed(error)],
                );
                // Nobody may be listening any more, but the work is still
                // done.
                for response in responses {
                    let _ = sender.unbounded_send(response);
                }
            }
        })
        .unwrap_or_else(|error| panic!("couldn't start the {name} thread: {error}"));
    let lane = Lane {
        shared: Arc::clone(&shared),
    };
    let responses = Responses {
        receiver,
        shared,
        thread: Some(thread),
    };
    (lane, responses)
}

/// State shared between the lane's thread and its handles.
struct Shared<R> {
    state: Mutex<State<R>>,
    /// Notified when `state` changes.
    wake: Condvar,
}

struct State<R> {
    pending: Box<dyn Pending<R> + Send>,
    /// Set once [`Responses`] is dropped: nobody is listening any more.
    closed: bool,
}

impl<R> Shared<R> {
    fn lock(&self) -> MutexGuard<'_, State<R>> {
        // Nothing in here can panic while holding the lock, so the state is
        // never left half updated.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits for the next request, or `None` once the lane is closed and,
    /// if it drains, nothing is left pending.
    fn next(&self, on_close: OnClose) -> Option<R> {
        let mut state = self
            .wake
            .wait_while(self.lock(), |state| {
                !state.closed && state.pending.is_empty()
            })
            .unwrap_or_else(PoisonError::into_inner);
        if state.closed && on_close == OnClose::Stop {
            None
        } else {
            state.pending.pop()
        }
    }
}

/// Sends requests to a lane. Cheap to clone; all clones feed the same
/// thread.
pub struct Lane<R> {
    shared: Arc<Shared<R>>,
}

impl<R> Clone for Lane<R> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<R> fmt::Debug for Lane<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lane").finish_non_exhaustive()
    }
}

impl<R> Transport<R> for Lane<R> {
    fn send(&mut self, request: R) {
        let replaced = self.shared.lock().pending.push(request);
        self.shared.wake.notify_one();
        drop(replaced);
    }
}

/// The responses of a lane, as a stream. Dropping it ends the lane's
/// thread, see [`OnClose`]; a request already running finishes first.
pub struct Responses<R, S> {
    receiver: UnboundedReceiver<S>,
    shared: Arc<Shared<R>>,
    /// The lane's thread, until [`close`](Self::close) hands it out.
    thread: Option<JoinHandle<()>>,
}

impl<R, S> Responses<R, S> {
    /// Closes the lane as dropping the responses does, and returns its
    /// thread, for tests to wait for it to end. Dropping never waits.
    pub fn close(mut self) -> JoinHandle<()> {
        self.thread.take().expect("taken only here")
    }
}

impl<R, S> fmt::Debug for Responses<R, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Responses").finish_non_exhaustive()
    }
}

impl<R, S> Stream for Responses<R, S> {
    type Item = S;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S>> {
        Pin::new(&mut self.receiver).poll_next(cx)
    }
}

impl<R, S> Drop for Responses<R, S> {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.wake.notify_one();
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;
