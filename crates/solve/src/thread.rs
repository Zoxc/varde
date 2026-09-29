//! A lane natively: a thread that handles one document's requests one at a
//! time, in the [`Order`] they wait in.
//!
//! Responses come back on a [`Responses`] stream, for an iced
//! `Subscription::run` to yield as messages. The sending side never waits
//! for work, see [`varde_lane::thread`]. The thread's [`Solver`] keeps the
//! drag in progress. The thread ends once its [`Responses`] is dropped,
//! which is when the subscription owning it stops.
//!
//! Needs `std::thread`, so native only; on the web a Web Worker takes its
//! place, with the same API (`src/worker.rs`). Both are [`crate::lane`].

use varde_lane::thread::{self, OnClose};

use crate::lane::{Lane, Responses};
use crate::order::Order;
use crate::{Request, Response, Solver};

/// Starts a lane on a new thread. Send requests through the [`Lane`], read
/// responses from [`Responses`]; dropping the latter ends the thread.
pub fn spawn() -> (Lane, Responses) {
    let mut solver = Solver::default();
    spawn_on(move |request| solver.handle(request))
}

/// Starts a lane doing its work with `handle`, which tests can swap for one
/// that fails. A job that panics is answered with [`Response::Failed`], so
/// the UI hears back either way, and the lane goes on.
fn spawn_on(handle: impl FnMut(Request) -> Option<Response> + Send + 'static) -> (Lane, Responses) {
    thread::spawn_answering(
        "solve",
        Order::default(),
        OnClose::Stop,
        handle,
        |request: &Request| {
            let tag = request.tag();
            move |error| Response::Failed { tag, error }
        },
    )
}

#[cfg(test)]
mod tests;
