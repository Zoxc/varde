//! A lane on the web: a Web Worker per document.
//!
//! The same API as the native lane, so the app doesn't tell them apart. The
//! worker is a second instance of the page's wasm, started in the role
//! [`WORKER_ROLE`](crate::WORKER_ROLE), and shares no memory with the
//! page: requests are posted as bytes and meshes come back as transferred
//! buffers, see [`wire`]. The page's side never waits; how its requests
//! reach the worker is [`varde_lane::page`]'s.
//!
//! Latest wins through a [`mailbox`] holding a [`Newest`]: one request is
//! with the worker at a time and newer regenerations replace each other on
//! the page until it answers; exports wait in order, never replaced. A job that has started runs to the end, since the
//! worker can't see messages meanwhile. The worker keeps its
//! [`Regenerator`]'s cache from one request to the next, as the native
//! thread does.
//!
//! A worker that stops (a panic traps its wasm instance, or its script or
//! wasm didn't load) is terminated and the request it had is answered with
//! [`Response::Failed`]. A request waiting behind that one never reached it,
//! so a new worker starts for it; otherwise the next request starts one.
//! See [`Mailbox::fail`](mailbox::Mailbox::fail) for why that can't loop.

use js_sys::Uint8Array;
use varde_lane::mailbox::{self, Wire};
use varde_lane::page::{Host, Refused};
use varde_lane::{bytes, worker};

use crate::lane::{Lane, Responses};
use crate::newest::Newest;
use crate::wire;
use crate::{Regenerator, Request, Response};

/// Starts a lane in a new Web Worker. Send requests through the [`Lane`],
/// read responses from [`Responses`]; dropping the latter terminates the
/// worker.
pub fn spawn() -> (Lane, Responses) {
    mailbox::spawn(crate::WORKER_ROLE, "the regeneration worker", Regenerate)
}

/// How the lane's requests and replies cross, see [`wire`].
struct Regenerate;

impl Wire for Regenerate {
    type Request = Request;
    type Response = Response;
    type Pending = Newest;

    /// Posts `request` to the worker, transferring its bytes.
    fn post(&self, host: &Host, request: &Request) -> Result<(), String> {
        host.post(&[&wire::encode_request(request)], &[])
    }

    /// A reply is a head and the model's parts.
    fn receive(
        &self,
        parts: Vec<Uint8Array>,
        _: Option<&Request>,
    ) -> Result<Option<Response>, Refused> {
        let (head, model) = parts.split_first().ok_or(Refused::Else)?;
        Ok(Some(wire::decode_reply(head, model)?))
    }

    /// A regeneration's progress comes ahead of its answer.
    fn finishes(&self, response: &Response) -> bool {
        !matches!(response, Response::Progress(_))
    }

    fn failed(&self, request: &Request, error: String) -> Response {
        request.failure()(error)
    }
}

/// Posts `response` to the page.
fn post(response: &Response) {
    let (head, body) = wire::encode_reply(response);
    let mut parts = vec![&head[..]];
    parts.extend(body.iter().map(|part| &**part));
    worker::post(&parts);
}

/// Runs the worker's side: answers each request posted to it, one at a
/// time, its progress ahead of each regeneration's answer. Called by the
/// worker's `main`, in the worker.
pub fn serve() {
    let mut regenerator = Regenerator::default();
    worker::serve("the regeneration worker", move |message| {
        let ([part], []) = (&message.parts[..], &message.objects[..]) else {
            return Err(Refused::Else);
        };
        let bytes = bytes::copy(part, wire::MAX_REQUEST_BYTES)?;
        let request = wire::decode_request(&bytes)?;
        let response = regenerator.handle_reporting(request, &mut |progress| {
            post(&Response::Progress(progress));
        });
        post(&response);
        Ok(())
    });
}
