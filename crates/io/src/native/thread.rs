//! The lane natively: one thread for the app, handling requests in order.
//!
//! Requests wait in a [`Queue`]; responses come back on a [`Responses`]
//! stream, for an iced `Subscription::run` to yield as messages. The
//! sending side never waits for work, see [`varde_lane::thread`].
//!
//! Once the [`Responses`] are dropped, the thread handles what's still
//! queued, so a last write isn't lost, closes the files still open and
//! ends.
//!
//! A request that panics is answered with the panic as its error, and the
//! lane goes on: a lane that ended would leave the UI waiting forever.

use varde_lane::thread::{self, OnClose};

use super::files::Files;
use crate::lane::{Lane, Responses};
use crate::queue::Queue;
use crate::{Request, Response, Stores};

/// Starts the lane on a new thread, with the app's own files in their
/// usual places, see [`Stores::user`]. Send requests through the [`Lane`],
/// read responses from [`Responses`]; dropping the latter ends the thread.
pub fn spawn() -> (Lane, Responses) {
    spawn_at(Stores::user())
}

/// [`spawn`] with the app's own files kept in `stores`, which keeps tests
/// off the user's.
pub fn spawn_at(stores: Stores) -> (Lane, Responses) {
    let mut files = Files::new(stores);
    spawn_answering(move |request| files.answer(request))
}

/// Starts the lane, answering each request with `handle`, which tests use
/// to stand in for [`Files`]. Whatever `handle` owns is dropped as the
/// thread ends.
#[cfg(test)]
pub(crate) fn spawn_on(
    mut handle: impl FnMut(Request) -> Response + Send + 'static,
) -> (Lane, Responses) {
    spawn_answering(move |request| [handle(request)])
}

/// Starts the lane, answering each request with the responses `handle`
/// gives, in order.
fn spawn_answering<A: IntoIterator<Item = Response>>(
    handle: impl FnMut(Request) -> A + Send + 'static,
) -> (Lane, Responses) {
    // A request that panics leaves the files as they are: no worse than it
    // found them, short of the one it was about.
    thread::spawn_answering(
        "io",
        Queue::default(),
        OnClose::Drain,
        handle,
        Request::failure,
    )
}

#[cfg(test)]
mod tests;
