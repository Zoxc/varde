//! Requests waiting on the page for a Web Worker that takes one at a time.
//!
//! A Web Worker can't look at new messages while a job runs, so it can't
//! reorder or drop the requests that queued up meanwhile. The page keeps
//! them instead: at most one request is with the worker, and the rest wait
//! here in the lane's [`Pending`], a slot a newer request replaces the one
//! in or a queue, as they would natively. When the worker answers, the
//! next goes. Waiting here rather than in the worker also means a request
//! that's replaced is never encoded.
//!
//! It also knows whether there's a worker, and tells the page what to do
//! next: post a request, or start a worker. Plain Rust, so the whole
//! lifecycle is tested natively; the page (`spawn`, on the web) only does
//! what it's told, and each lane only says how its requests and replies
//! cross (`Wire`).

use crate::Pending;

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
pub use page::{Wire, spawn};

/// What the page does next, as the [`Mailbox`] says.
#[derive(Debug)]
pub enum Next<R> {
    Nothing,
    /// Post this request to the worker.
    Post(R),
    /// Start a worker. What waits goes to it once it's ready; if it can't
    /// start, that's a [`fail`](Mailbox::fail) like any other.
    Start,
}

/// The page's side of a worker taking one request at a time, with the
/// requests waiting for it in `P`.
#[derive(Debug)]
pub struct Mailbox<R, P> {
    worker: Worker<R>,
    pending: P,
}

#[derive(Debug)]
enum Worker<R> {
    /// There's none: the last one died and nothing waited for another.
    Stopped,
    /// Started, not listening yet: requests posted now would be lost. A
    /// lane's first worker starts with it.
    Starting,
    Idle,
    /// Working on this request, kept to report it if the worker dies and
    /// to check the answer against.
    Busy(R),
}

/// A mailbox whose first worker is starting, with nothing waiting yet.
impl<R, P: Default> Default for Mailbox<R, P> {
    fn default() -> Self {
        Self {
            worker: Worker::Starting,
            pending: P::default(),
        }
    }
}

impl<R: Clone, P: Pending<R>> Mailbox<R, P> {
    /// Takes `request`, which goes to the worker now if it's idle. It waits
    /// otherwise, for a new worker if there's none. What `P` refuses or
    /// replaces is dropped.
    pub fn send(&mut self, request: R) -> Next<R> {
        drop(self.pending.push(request));
        match self.worker {
            Worker::Idle => self.next(),
            Worker::Stopped => {
                self.worker = Worker::Starting;
                Next::Start
            }
            Worker::Starting | Worker::Busy(_) => Next::Nothing,
        }
    }

    /// The worker is ready, having started. A worker says so once, first,
    /// so it's ignored at any other time.
    pub fn ready(&mut self) -> Next<R> {
        match self.worker {
            Worker::Starting => self.next(),
            Worker::Stopped | Worker::Idle | Worker::Busy(_) => Next::Nothing,
        }
    }

    /// The request with the worker, if one is: what its answer answers.
    pub fn busy(&self) -> Option<&R> {
        match &self.worker {
            Worker::Busy(request) => Some(request),
            Worker::Stopped | Worker::Starting | Worker::Idle => None,
        }
    }

    /// The worker answered the request it had. Ignored unless it had one.
    pub fn done(&mut self) -> Next<R> {
        match self.worker {
            Worker::Busy(_) => self.next(),
            Worker::Stopped | Worker::Starting | Worker::Idle => Next::Nothing,
        }
    }

    /// Hands the worker the request to handle next, if one waits, or
    /// leaves it idle.
    fn next(&mut self) -> Next<R> {
        match self.pending.pop() {
            Some(request) => {
                self.worker = Worker::Busy(request.clone());
                Next::Post(request)
            }
            None => {
                self.worker = Worker::Idle;
                Next::Nothing
            }
        }
    }

    /// The worker is gone, or couldn't start. Returns the request to
    /// report as failed: the one it was working on, which is what killed
    /// it as far as anyone knows. A request waiting behind that one was
    /// never tried, so a new worker starts for it, from scratch.
    ///
    /// A worker that died before it was ready, or didn't start, never tried
    /// anything, and another would likely die the same way, so the request
    /// that would have gone to it first is reported instead of kept, and a
    /// worker starts only if more wait. So each worker that dies costs one
    /// request, and a crashing worker can't be restarted without end.
    pub fn fail(&mut self) -> (Option<R>, Next<R>) {
        let failed = match std::mem::replace(&mut self.worker, Worker::Stopped) {
            Worker::Busy(request) => Some(request),
            Worker::Starting => self.pending.pop(),
            Worker::Stopped | Worker::Idle => None,
        };
        if self.pending.is_empty() {
            (failed, Next::Nothing)
        } else {
            self.worker = Worker::Starting;
            (failed, Next::Start)
        }
    }
}

#[cfg(test)]
mod tests;
