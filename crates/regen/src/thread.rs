//! A lane natively: a thread that handles one document's requests one at a
//! time.
//!
//! Regenerations go into a single slot, so a newer one replaces one still
//! pending instead of queueing behind it: a burst of edits costs one run
//! after the current one. Exports queue in order and are never replaced
//! (see `src/newest.rs`). Responses come back on a [`Responses`] stream,
//! for an iced `Subscription::run` to yield as messages. The sending side
//! never waits for work, see [`varde_lane::thread`]. The thread ends once
//! its [`Responses`] is dropped, which is when the subscription owning it
//! stops.
//!
//! Needs `std::thread`, so native only; on the web a Web Worker takes its
//! place, with the same API (`src/worker.rs`). Both are [`crate::lane`].

use varde_lane::thread::{self, OnClose};

use crate::lane::{Lane, Responses};
use crate::newest::Newest;
use crate::{Regenerator, Request, Response};

/// Starts a lane on a new thread. Send requests through the [`Lane`], read
/// responses from [`Responses`]; dropping the latter ends the thread. A
/// regeneration's [`Response::Progress`] comes ahead of its answer. A job
/// that panics is answered with [`Response::Failed`], so the UI hears
/// back either way, and the lane goes on.
pub fn spawn() -> (Lane, Responses) {
    let mut regenerator = Regenerator::default();
    thread::spawn_reporting(
        "regenerate",
        Newest::default(),
        OnClose::Stop,
        move |request, send| {
            regenerator.handle_reporting(request, &mut |progress| {
                send(Response::Progress(progress));
            })
        },
        Request::failure,
    )
}

/// Starts a lane doing its work with `handle`, which tests swap for one
/// that fails, without progress.
#[cfg(test)]
fn spawn_on(handle: impl FnMut(Request) -> Response + Send + 'static) -> (Lane, Responses) {
    thread::spawn(
        "regenerate",
        Newest::default(),
        OnClose::Stop,
        handle,
        Request::failure,
    )
}

#[cfg(test)]
mod tests;
