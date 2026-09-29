//! A lane on the web: a Web Worker per document.
//!
//! The same API as the native lane, so the app doesn't tell them apart. The
//! worker is a second wasm instance built from `src/bin/varde-regen-worker.rs`
//! by trunk (see `crates/web/index.html`) and shares no memory with the
//! page: requests are posted as bytes and meshes come back as transferred
//! buffers, see [`wire`]. The page's side never waits; how its requests
//! reach the worker is [`varde_lane::page`]'s.
//!
//! Latest wins through a [`Mailbox`]: one request is with the worker at a
//! time and newer ones replace each other on the page until it answers. A
//! job that has started runs to the end, since the worker can't see
//! messages meanwhile; jobs are short so far.
//!
//! A worker that stops (a panic traps its wasm instance, or its script or
//! wasm didn't load) is terminated and the request it had is answered with
//! [`Response::Failed`]. A request waiting behind that one never reached it,
//! so a new worker starts for it; otherwise the next request starts one.
//! See [`Mailbox::fail`] for why that can't loop.

use std::cell::RefCell;

use futures::channel::mpsc::UnboundedSender;
use js_sys::Uint8Array;
use varde_lane::page::{self, Host, Page, Refused};
use varde_lane::{bytes, worker};

use crate::lane::{Lane, Responses};
use crate::mailbox::{Mailbox, Next};
use crate::wire;
use crate::{Request, Response, handle};

/// Starts a lane in a new Web Worker. Send requests through the [`Lane`],
/// read responses from [`Responses`]; dropping the latter terminates the
/// worker.
pub fn spawn() -> (Lane, Responses) {
    // Started now rather than on the first request, so it loads while the
    // document opens.
    page::spawn(
        "varde-regen-worker",
        "the regeneration worker",
        |host, sender| Shared {
            host,
            mailbox: RefCell::default(),
            sender,
        },
    )
}

/// State shared between the [`Responses`] stream and its worker's callbacks.
struct Shared {
    host: Host,
    mailbox: RefCell<Mailbox>,
    sender: UnboundedSender<Response>,
}

impl Page<Request> for Shared {
    fn host(&self) -> &Host {
        &self.host
    }

    fn send(&self, request: Request) {
        let next = self.mailbox.borrow_mut().send(request);
        self.act(next);
    }

    fn ready(&self) {
        let next = self.mailbox.borrow_mut().ready();
        self.act(next);
    }

    /// Handles a reply from the worker: a head and the mesh parts.
    fn receive(&self, parts: Vec<Uint8Array>) -> Result<(), Refused> {
        let (head, mesh) = parts.split_first().ok_or(Refused::Else)?;
        let response = wire::decode_reply(head, mesh)?;
        let _ = self.sender.unbounded_send(response);
        let next = self.mailbox.borrow_mut().done();
        self.act(next);
        Ok(())
    }

    /// Answers the request the stopped worker had with `error`, and starts
    /// a new worker if a request is waiting for one.
    fn fail(&self, error: String) {
        let (failed, next) = self.mailbox.borrow_mut().fail();
        if let Some(generation) = failed {
            let _ = self
                .sender
                .unbounded_send(Response::Failed { generation, error });
        }
        // A worker that doesn't start fails again, but then the mailbox
        // gives up on what waited for it rather than start another.
        self.act(next);
    }
}

impl Shared {
    /// Does what the mailbox said.
    fn act(&self, next: Next) {
        match next {
            Next::Nothing => {}
            Next::Post(request) => self.post(&request),
            Next::Start => {
                if let Err(error) = self.host.start() {
                    self.fail(error);
                }
            }
        }
    }

    /// Posts `request` to the worker, transferring its bytes.
    fn post(&self, request: &Request) {
        if let Err(error) = self.host.post(&[&wire::encode_request(request)], &[]) {
            self.fail(error);
        }
    }
}

/// Runs the worker's side: answers each request posted to it, one at a
/// time. Called by the worker's `main`, in the worker.
pub fn serve() {
    worker::serve("the regeneration worker", |message| {
        let ([part], []) = (&message.parts[..], &message.objects[..]) else {
            return Err(Refused::Else);
        };
        let bytes = bytes::copy(part, wire::MAX_REQUEST_BYTES)?;
        let request = wire::decode_request(&bytes)?;
        let response = handle(request);
        let (head, mesh) = wire::encode_reply(&response);
        let mut parts = vec![&head[..]];
        parts.extend(mesh.into_iter().flatten());
        worker::post(&parts);
        Ok(())
    });
}
