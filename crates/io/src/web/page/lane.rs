//! The page's side of the web lane, see the parent module: posts each
//! request to the worker, numbered, and answers what it owed should it
//! stop. Never in the worker.

use std::cell::RefCell;
use std::collections::VecDeque;

use futures::channel::mpsc::UnboundedSender;
use js_sys::Uint8Array;
use varde_lane::Pending;
use varde_lane::page::{self, Host, Page, Refused};
use wasm_bindgen::prelude::*;

use super::pick;
use crate::lane::{Lane, Responses};
use crate::queue::Queue;
use crate::web::message_bytes;
use crate::wire::{self, Reply, ToWorker};
use crate::{Chosen, Picked, Request, Response, SaveTo};

/// Starts the lane in a new Web Worker, which keeps the app's own files in
/// their usual places, see [`Stores::user`](crate::Stores::user). Send
/// requests through the [`Lane`], read responses from [`Responses`];
/// dropping the latter terminates the worker.
pub fn spawn() -> (Lane, Responses) {
    page::spawn("varde-io-worker", "the file worker", |host, sender| {
        Shared {
            host,
            state: RefCell::new(State {
                phase: Phase::Starting(Vec::new()),
                next: 0,
                posted: VecDeque::new(),
            }),
            sender,
        }
    })
}

/// State shared between the [`Responses`] stream and its worker's callbacks.
struct Shared {
    host: Host,
    state: RefCell<State>,
    sender: UnboundedSender<Response>,
}

struct State {
    phase: Phase,
    /// The number of the next request posted.
    next: u64,
    /// The requests posted and not answered yet, with their numbers, in
    /// order.
    posted: VecDeque<(u64, Request)>,
}

enum Phase {
    /// Not listening yet: requests wait here.
    Starting(Vec<Request>),
    Ready,
    /// Stopped with this error, which answers every request.
    Stopped(String),
}

impl Page<Request> for Shared {
    fn host(&self) -> &Host {
        &self.host
    }

    fn send(&self, request: Request) {
        // Kept on the page, whatever became of the worker.
        let request = match super::panicked::answer(request) {
            Ok(response) => {
                let _ = self.sender.unbounded_send(response);
                return;
            }
            Err(request) => request,
        };
        let mut state = self.state.borrow_mut();
        match &mut state.phase {
            Phase::Starting(waiting) => waiting.push(request),
            Phase::Ready => {
                drop(state);
                self.post(request);
            }
            Phase::Stopped(error) => {
                let response = request.failed(error.clone());
                drop(state);
                let _ = self.sender.unbounded_send(response);
            }
        }
    }

    /// The worker listens now: posts the requests that waited for it.
    fn ready(&self) {
        let waiting = match std::mem::replace(&mut self.state.borrow_mut().phase, Phase::Ready) {
            Phase::Starting(waiting) => waiting,
            // The one worker says it's ready once, and never after it
            // stopped.
            Phase::Ready | Phase::Stopped(_) => Vec::new(),
        };
        for request in waiting {
            // Answered with the error once a post stops the worker.
            self.send(request);
        }
    }

    /// Handles a message from the worker: one buffer.
    fn receive(&self, parts: Vec<Uint8Array>) -> Result<(), Refused> {
        let [buffer] = &parts[..] else {
            return Err(Refused::Else);
        };
        let bytes = message_bytes(buffer)?;
        let Reply { seq, response } = wire::decode(&bytes)?;
        // Answered in order, so everything posted before it is done with
        // too, answered or replaced.
        {
            let posted = &mut self.state.borrow_mut().posted;
            while posted.front().is_some_and(|&(posted, _)| posted <= seq) {
                posted.pop_front();
            }
        }
        let _ = self.sender.unbounded_send(response);
        Ok(())
    }

    /// Answers every request the stopped worker owed, and every one after,
    /// with `error`.
    fn fail(&self, error: String) {
        let owed = {
            let mut state = self.state.borrow_mut();
            let waiting = match std::mem::replace(&mut state.phase, Phase::Stopped(error.clone())) {
                Phase::Starting(waiting) => waiting,
                Phase::Ready | Phase::Stopped(_) => Vec::new(),
            };
            // A save the worker had queued behind another it would have
            // replaced is never answered, as if it had gone on.
            let mut owed = Queue::default();
            for request in state
                .posted
                .drain(..)
                .map(|(_, request)| request)
                .chain(waiting)
            {
                owed.push(request);
            }
            owed
        };
        for request in owed {
            let _ = self.sender.unbounded_send(request.failed(error.clone()));
        }
    }
}

impl Shared {
    /// Posts `request` to the worker, numbered.
    fn post(&self, request: Request) {
        let seq = {
            let mut state = self.state.borrow_mut();
            let seq = state.next;
            // A u64 counted up once per request never overflows.
            state.next += 1;
            seq
        };
        // Missing only if the page lost it: the worker then says so.
        let object = picked(&request).and_then(pick::object);
        let message = ToWorker {
            seq,
            request: &request,
        };
        match self.post_message(&message, object) {
            // Only this request can't be sent.
            Err(Error::Encode(error)) => {
                let _ = self.sender.unbounded_send(request.failed(error));
            }
            result => {
                self.state.borrow_mut().posted.push_back((seq, request));
                if let Err(Error::Post(error)) = result {
                    self.fail(error);
                }
            }
        }
    }

    /// Posts `message` to the worker, transferring its bytes, and cloning
    /// `object` along with it if there is one.
    fn post_message(
        &self,
        message: &ToWorker<&Request>,
        object: Option<JsValue>,
    ) -> Result<(), Error> {
        let bytes = wire::encode(message, wire::MAX_MESSAGE_BYTES)
            .map_err(|e| Error::Encode(e.to_string()))?;
        self.host
            .post(&[&bytes], object.as_slice())
            .map_err(Error::Post)
    }
}

/// Why a message couldn't be posted.
enum Error {
    /// It couldn't be encoded: only that message is lost.
    Encode(String),
    /// The worker is gone, or refused it.
    Post(String),
}

/// The file the user picked that `request` uses, if any: what the browser
/// handed over for it is posted along with it.
fn picked(request: &Request) -> Option<&Picked> {
    match request {
        Request::Open {
            from: Chosen::File(picked),
            ..
        }
        | Request::SaveAs {
            to: SaveTo::Picked(picked),
            ..
        }
        | Request::Export {
            to: SaveTo::Picked(picked),
            ..
        } => Some(picked),
        _ => None,
    }
}
