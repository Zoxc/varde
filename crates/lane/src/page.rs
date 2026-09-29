//! The page's side of a lane on the web, which is a Web Worker.
//!
//! The worker and its callbacks belong to the [`Responses`] stream, since
//! iced wants messages, and so the [`Lane`] in one, to be `Send`, which JS
//! objects aren't. [`Lane::send`] only queues the request for the stream,
//! which wakes up, hands it to the lane's [`Page`] to post, and yields the
//! responses the worker's `onmessage` feeds it. The page's side never
//! waits.
//!
//! What a lane posts, when, and what it does once the worker stops is its
//! own [`Page`]'s business; [`Host`] owns the worker and its callbacks,
//! which [`spawn`] builds and starts for it.
//!
//! The worker's side (see [`worker::serve`](crate::worker::serve)) posts
//! three kinds of message, told apart by their JS shape: `null` once it
//! listens, a string if it panicked, and otherwise an array of
//! `ArrayBuffer`s, the lane's own. The page posts only the latter, with
//! any objects along with them, see [`Host::post`]. The host turns them into calls to
//! [`Page::ready`], [`Page::fail`] and [`Page::receive`], so a lane's wire
//! format carries only its requests and responses.
//!
//! However the worker fails, the host words why, naming the worker, logs
//! it and terminates the worker before the page hears of it, so a lane
//! only answers what the worker owed.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll};

use futures::Stream;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use js_sys::Uint8Array;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{ErrorEvent, Event, MessageEvent, Worker};

use crate::Transport;
pub use crate::message::Refused;
use crate::message::{self, Message};

/// A lane's state on the page, shared between its [`Responses`] stream and
/// its worker's callbacks.
pub trait Page<R> {
    /// The worker's host, which [`spawn`] gave the page.
    fn host(&self) -> &Host;

    /// Posts `request` to the worker, or keeps it until it can, or answers
    /// it with an error if it never will.
    fn send(&self, request: R);

    /// The worker listens now: requests posted before this would have been
    /// lost. Called once per worker, first.
    fn ready(&self);

    /// Handles a message of the lane's the worker posted: its parts, see
    /// [`worker::post`](crate::worker::post), to copy out with
    /// [`bytes::copy`](crate::bytes::copy). Only after
    /// [`ready`](Self::ready). Refusing it fails the worker, as "… sent
    /// something else" or "… sent something broken: …".
    fn receive(&self, parts: Vec<Uint8Array>) -> Result<(), Refused>;

    /// The worker stopped, or couldn't start or be posted to, or panicked
    /// or posted something out of turn, for the reason `error` gives. By
    /// then `error` is logged and the worker terminated; the page answers
    /// what it owed.
    fn fail(&self, error: String);
}

/// Starts a lane on the page, with a worker built from the binary called
/// `bin`, e.g. "varde-io-worker" for `src/bin/varde-io-worker.rs` (see
/// `crates/web/index.html`), and called `name` in errors, e.g. "the file
/// worker": `build` makes its [`Page`] from the
/// worker's [`Host`], not started yet, and the sender the page answers
/// through. The worker then starts, or the page hears why not through
/// [`Page::fail`]. Send requests through the [`Lane`], read responses from
/// [`Responses`]; dropping the latter terminates the worker.
pub fn spawn<R: 'static, S, P: Page<R> + 'static>(
    bin: &'static str,
    name: &'static str,
    build: impl FnOnce(Host, UnboundedSender<S>) -> P,
) -> (Lane<R>, Responses<R, S>) {
    let (lane, requests) = unbounded();
    let (sender, receiver) = unbounded();
    let page = Rc::new_cyclic(|this: &Weak<P>| build(Host::new(this, bin, name), sender));
    if let Err(error) = page.host().start() {
        // Already logged and stopped by `start`.
        page.fail(error);
    }
    let responses = Responses {
        requests,
        receiver,
        page,
    };
    (Lane { requests: lane }, responses)
}

/// Sends requests to a lane's worker, through its [`Responses`]. Cheap to
/// clone; all clones feed the same worker. `Send`, as iced wants of what
/// goes in messages.
pub struct Lane<R> {
    requests: UnboundedSender<R>,
}

impl<R> Clone for Lane<R> {
    fn clone(&self) -> Self {
        Self {
            requests: self.requests.clone(),
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
        // Fails only once the lane has ended, when there's nobody to answer.
        let _ = self.requests.unbounded_send(request);
    }
}

/// The responses of a lane, as a stream, which also hands its requests to
/// the page to post. Dropping it stops the page, terminating the worker,
/// even mid-request.
pub struct Responses<R, S> {
    requests: UnboundedReceiver<R>,
    receiver: UnboundedReceiver<S>,
    page: Rc<dyn Page<R>>,
}

impl<R, S> fmt::Debug for Responses<R, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Responses").finish_non_exhaustive()
    }
}

impl<R, S> Stream for Responses<R, S> {
    type Item = S;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S>> {
        while let Poll::Ready(Some(request)) = Pin::new(&mut self.requests).poll_next(cx) {
            self.page.send(request);
        }
        Pin::new(&mut self.receiver).poll_next(cx)
    }
}

impl<R, S> Drop for Responses<R, S> {
    fn drop(&mut self) {
        self.page.host().stop();
    }
}

/// The script that loads a worker, next to the page, see
/// `crates/web/worker_loader.js`.
const LOADER: &str = "./worker_loader.js";

/// A Web Worker and the callbacks it calls, which hold the page they call
/// back weakly: the page owns them, so a strong hold would keep both alive.
pub struct Host {
    /// The binary the worker is built from, which the loader loads.
    bin: &'static str,
    /// What the worker is called in errors, e.g. "the file worker".
    name: &'static str,
    /// `None` before the worker starts and after it stopped.
    worker: RefCell<Option<Worker>>,
    /// Whether the worker said it listens.
    ready: Cell<bool>,
    on_message: Closure<dyn FnMut(MessageEvent)>,
    on_error: Closure<dyn FnMut(Event)>,
}

impl Host {
    /// A host for the worker built from the binary `bin`, called `name` in
    /// errors, that isn't started yet. Its messages go to the page `this`
    /// refers to, see [`deliver`], and if it stops, why goes to
    /// [`Page::fail`]. For a page made with `Rc::new_cyclic`.
    fn new<R, P: Page<R> + 'static>(this: &Weak<P>, bin: &'static str, name: &'static str) -> Self {
        let weak = this.clone();
        let on_message = Closure::new(move |event: MessageEvent| {
            if let Some(page) = weak.upgrade() {
                deliver(&*page, &event.data());
            }
        });
        let weak = this.clone();
        let on_error = Closure::new(move |event: Event| {
            if let Some(page) = weak.upgrade() {
                let error = event.dyn_ref::<ErrorEvent>().map_or_else(
                    || format!("{name} didn't load"),
                    |event| format!("{name} stopped: {}", event.message()),
                );
                fail(&*page, error);
            }
        });
        Self {
            bin,
            name,
            worker: RefCell::default(),
            ready: Cell::new(false),
            on_message,
            on_error,
        }
    }

    /// Starts a worker, replacing none: the one before must have stopped.
    /// If it can't, why is logged, for the page to [`fail`](Page::fail)
    /// with.
    pub fn start(&self) -> Result<(), String> {
        let worker = Worker::new(&format!("{LOADER}?{}", self.bin))
            .map_err(|e| self.stopped(format!("couldn't start {}: {e:?}", self.name)))?;
        worker.set_onmessage(Some(self.on_message.as_ref().unchecked_ref()));
        worker.set_onerror(Some(self.on_error.as_ref().unchecked_ref()));
        *self.worker.borrow_mut() = Some(worker);
        self.ready.set(false);
        Ok(())
    }

    /// Posts `parts` to the worker, transferring them, followed by
    /// `objects`, cloned: the worker gets them as a
    /// [`crate::worker::Message`]. If it can't, the worker is
    /// terminated and why logged, for the page to [`fail`](Page::fail)
    /// with.
    pub fn post(&self, parts: &[&[u8]], objects: &[JsValue]) -> Result<(), String> {
        let (message, transfer) = message::frame(parts, objects);
        let posted = match &*self.worker.borrow() {
            Some(worker) => worker
                .post_message_with_transfer(&message, &transfer)
                .map_err(|e| format!("couldn't post to {}: {e:?}", self.name)),
            None => Err(format!("{} stopped", self.name)),
        };
        posted.map_err(|error| self.stopped(error))
    }

    /// Terminates the worker, if one is running. Its callbacks go first, so
    /// nothing it sent on the way out reaches the page.
    pub fn stop(&self) {
        if let Some(worker) = self.worker.borrow_mut().take() {
            worker.set_onmessage(None);
            worker.set_onerror(None);
            worker.terminate();
        }
    }

    /// Logs `error`, which fails the worker, and terminates it. Returns
    /// `error`, for the page.
    fn stopped(&self, error: String) -> String {
        log::error!("{error}");
        self.stop();
        error
    }
}

/// Fails `page`'s worker for the reason `error` gives: logs it, terminates
/// the worker and tells the page.
fn fail<R>(page: &impl Page<R>, error: String) {
    let error = page.host().stopped(error);
    page.fail(error);
}

/// Hands the message `data` the worker posted to `page`: `null` says it's
/// ready, which it says once, first; a string is why it panicked; and
/// buffers are the lane's. Anything else, or out of turn, fails the worker.
fn deliver<R>(page: &impl Page<R>, data: &JsValue) {
    let host = page.host();
    let ready = host.ready.get();
    if data.is_null() && !ready {
        host.ready.set(true);
        page.ready();
    } else if let Some(error) = data.as_string() {
        fail(page, error);
    } else if let Some(message) = Message::from_js(data).filter(|m| ready && m.objects.is_empty()) {
        if let Err(refused) = page.receive(message.parts) {
            fail(page, refused.word(host.name, "sent"));
        }
    } else {
        fail(page, Refused::Else.word(host.name, "sent"));
    }
}
