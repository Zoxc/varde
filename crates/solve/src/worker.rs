//! A lane on the web: a Web Worker per document.
//!
//! The same API as the native lane, so the app doesn't tell them apart. The
//! worker is a second wasm instance built from
//! `src/bin/varde-solve-worker.rs` by trunk (see `crates/web/index.html`)
//! and shares no memory with the page: requests and replies are posted as
//! bytes, see [`wire`]. The page's side never waits; how its requests
//! reach the worker is [`varde_lane::page`]'s.
//!
//! The requests wait on the page in a [`mailbox`] holding an [`Order`]:
//! one request is with the worker at a time, since it can't see messages
//! while it solves, so drag steps can still replace each other and take
//! turns with the proposals queued. The page checks each reply against the
//! request it had.
//!
//! The worker keeps the drag session, so the page posts a session's
//! sketch only with the first step it posts to a worker; the later ones
//! carry the targets alone.
//!
//! A worker that stops (a panic traps its wasm instance, or its script or
//! wasm didn't load) is terminated and the request it had is answered with
//! [`Response::Failed`]. A request waiting behind that one never reached it,
//! so a new worker starts for it; otherwise the next request starts one.
//! See [`Mailbox::fail`](mailbox::Mailbox::fail) for why that can't loop.
//! A new worker has no drag session, so the step it's posted first
//! carries the sketch again.

use std::cell::Cell;

use js_sys::Uint8Array;
use varde_lane::mailbox::{self, Wire};
use varde_lane::page::{Host, Refused};
use varde_lane::{bytes, worker};

use crate::lane::{Lane, Responses};
use crate::order::Order;
use crate::wire;
use crate::{Request, Response, Solver};

/// Starts a lane in a new Web Worker. Send requests through the [`Lane`],
/// read responses from [`Responses`]; dropping the latter terminates the
/// worker.
pub fn spawn() -> (Lane, Responses) {
    let solve = Solve {
        session: Cell::new(None),
    };
    mailbox::spawn("varde-solve-worker", "the solver worker", solve)
}

/// How the lane's requests and replies cross, see [`wire`].
struct Solve {
    /// The drag session the worker has the sketch of, if any.
    session: Cell<Option<u64>>,
}

impl Wire for Solve {
    type Request = Request;
    type Response = Response;
    type Pending = Order;

    /// Posts `request` to the worker, transferring its bytes, with the
    /// sketch of a drag step only if the worker hasn't the session.
    fn post(&self, host: &Host, request: &Request) -> Result<(), String> {
        let with_sketch = match request {
            Request::Drag { session, .. } => self.session.replace(Some(*session)) != Some(*session),
            Request::Propose { .. } | Request::Analyse { .. } => true,
        };
        host.post(&[&wire::encode_request(request, with_sketch)], &[])
    }

    /// A reply is one part, checked against the request it answers: none
    /// for a drag step that didn't converge.
    fn receive(
        &self,
        parts: Vec<Uint8Array>,
        asked: Option<&Request>,
    ) -> Result<Option<Response>, Refused> {
        let [part] = &parts[..] else {
            return Err(Refused::Else);
        };
        let asked = asked.ok_or(Refused::Else)?;
        let bytes = bytes::copy(part, wire::MAX_MESSAGE_BYTES)?;
        Ok(wire::decode_reply(&bytes, asked)?)
    }

    fn failed(&self, request: &Request, error: String) -> Response {
        Response::Failed {
            tag: request.tag(),
            error,
        }
    }

    /// A new worker has no drag session.
    fn starting(&self) {
        self.session.set(None);
    }
}

/// Runs the worker's side: answers each request posted to it, one at a
/// time, keeping the drag session between them. Called by the worker's
/// `main`, in the worker.
pub fn serve() {
    let mut solver = Solver::default();
    worker::serve("the solver worker", move |message| {
        let ([part], []) = (&message.parts[..], &message.objects[..]) else {
            return Err(Refused::Else);
        };
        let bytes = bytes::copy(part, wire::MAX_MESSAGE_BYTES)?;
        let posted = wire::decode_request(&bytes)?;
        let tag = posted.tag();
        let response = posted.answer(&mut solver);
        worker::post(&[&wire::encode_reply(tag, response.as_ref())]);
        Ok(())
    });
}
