//! The page's side of a lane whose worker takes one request at a time,
//! the rest waiting on the page in a [`Mailbox`]: the page does what the
//! mailbox says, and the lane's [`Wire`] says how its requests and replies
//! cross and how one its worker failed on is answered.

use std::cell::RefCell;

use futures::channel::mpsc::UnboundedSender;
use js_sys::Uint8Array;

use super::{Mailbox, Next};
use crate::Pending;
use crate::page::{self, Host, Lane, Page, Refused, Responses};

/// What a lane whose requests wait in a [`Mailbox`] knows of its own:
/// how to post a request, what a reply says, and the answer to a request
/// its worker failed on.
pub trait Wire: 'static {
    type Request: Clone + 'static;
    type Response: 'static;
    /// Where requests wait for the worker, which decides their order and
    /// which replace others.
    type Pending: Pending<Self::Request> + Default + 'static;

    /// Posts `request` to the worker through `host`, see [`Host::post`].
    fn post(&self, host: &Host, request: &Self::Request) -> Result<(), String>;

    /// The response in the reply `parts` the worker posted, see
    /// [`Page::receive`], to `asked`, the request it had if it had one;
    /// none for a request that isn't answered. Refusing it fails the
    /// worker.
    fn receive(
        &self,
        parts: Vec<Uint8Array>,
        asked: Option<&Self::Request>,
    ) -> Result<Option<Self::Response>, Refused>;

    /// Whether `response` ends the job of the request the worker has, and
    /// so the next may go: all do but news the worker sends while it
    /// works, such as how far it has got.
    fn finishes(&self, response: &Self::Response) -> bool {
        let _ = response;
        true
    }

    /// The answer to `request`, which the worker failed on for the reason
    /// `error` gives.
    fn failed(&self, request: &Self::Request, error: String) -> Self::Response;

    /// A worker is starting, knowing nothing of what the last one was
    /// posted.
    fn starting(&self) {}
}

/// Starts a lane on the page whose worker, in the role `role` and
/// called `name` in errors (see [`page::spawn`]), takes one request at a
/// time, speaking `wire`. Started now rather than on the first request, so
/// it loads while the document opens. Send requests through the [`Lane`],
/// read responses from [`Responses`]; dropping the latter terminates the
/// worker.
pub fn spawn<W, R, S>(role: &'static str, name: &'static str, wire: W) -> (Lane<R>, Responses<R, S>)
where
    W: Wire<Request = R, Response = S>,
    R: 'static,
{
    page::spawn(role, name, |host, sender| Posting {
        host,
        mailbox: RefCell::default(),
        wire,
        sender,
    })
}

/// State shared between the [`Responses`] stream and its worker's
/// callbacks.
struct Posting<W: Wire> {
    host: Host,
    mailbox: RefCell<Mailbox<W::Request, W::Pending>>,
    wire: W,
    sender: UnboundedSender<W::Response>,
}

impl<W: Wire> Page<W::Request> for Posting<W> {
    fn host(&self) -> &Host {
        &self.host
    }

    fn send(&self, request: W::Request) {
        let next = self.mailbox.borrow_mut().send(request);
        self.act(next);
    }

    fn ready(&self) {
        let next = self.mailbox.borrow_mut().ready();
        self.act(next);
    }

    /// Handles a reply from the worker to the request it had, and posts
    /// the next unless the worker is still working on it (see
    /// [`Wire::finishes`]).
    fn receive(&self, parts: Vec<Uint8Array>) -> Result<(), Refused> {
        let response = {
            let mailbox = self.mailbox.borrow();
            self.wire.receive(parts, mailbox.busy())?
        };
        if let Some(response) = response {
            let finishes = self.wire.finishes(&response);
            let _ = self.sender.unbounded_send(response);
            if !finishes {
                return Ok(());
            }
        }
        let next = self.mailbox.borrow_mut().done();
        self.act(next);
        Ok(())
    }

    /// Answers the request the stopped worker had with `error`, and starts
    /// a new worker if a request is waiting for one.
    fn fail(&self, error: String) {
        let (failed, next) = self.mailbox.borrow_mut().fail();
        if let Some(request) = failed {
            let _ = self
                .sender
                .unbounded_send(self.wire.failed(&request, error));
        }
        // A worker that doesn't start fails again, but then the mailbox
        // gives up on what waited for it rather than start another.
        self.act(next);
    }
}

impl<W: Wire> Posting<W> {
    /// Does what the mailbox said.
    fn act(&self, next: Next<W::Request>) {
        match next {
            Next::Nothing => {}
            Next::Post(request) => {
                if let Err(error) = self.wire.post(&self.host, &request) {
                    self.fail(error);
                }
            }
            Next::Start => {
                self.wire.starting();
                if let Err(error) = self.host.start() {
                    self.fail(error);
                }
            }
        }
    }
}
