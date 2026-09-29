//! Latest wins for a worker that takes one request at a time.
//!
//! A Web Worker can't look at new messages while a job runs, so it can't
//! drop the requests that queued up meanwhile. The page keeps them instead:
//! at most one request is with the worker, and newer ones wait in a single
//! slot here, each replacing the last. When the worker answers, the newest
//! goes next, so a burst of edits costs one run after the current one, like
//! a native [`lane`](crate::lane), whose slot ([`Newest`]) this is too.
//! Waiting here rather than in the worker also means a superseded document
//! is never encoded.
//!
//! It also knows whether there's a worker, and tells the page what to do
//! next: post a request, or start a worker. Plain Rust, so the whole
//! lifecycle is tested natively; the web lane only does what it's told.

use varde_document::Generation;
use varde_lane::Pending;

use crate::Request;
use crate::newest::Newest;

/// What the page does next, as the [`Mailbox`] says.
#[derive(Debug)]
pub enum Next {
    Nothing,
    /// Post this request to the worker.
    Post(Request),
    /// Start a worker. What waits goes to it once it's ready; if it can't
    /// start, that's a [`fail`](Mailbox::fail) like any other.
    Start,
}

#[derive(Debug, Default)]
pub struct Mailbox {
    worker: Worker,
    /// The newest request not yet with the worker. It also remembers the
    /// newest generation posted, so an older request is refused whether the
    /// newer one waits or runs.
    pending: Newest,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Worker {
    /// There's none: the last one died and nothing waited for another.
    Stopped,
    /// Started, not listening yet: requests posted now would be lost. A
    /// lane's first worker starts with it.
    #[default]
    Starting,
    Idle,
    /// Working on this generation.
    Busy(Generation),
}

impl Mailbox {
    /// Takes `request`, which goes to the worker now if it's idle. It waits
    /// otherwise, for a new worker if there's none. One older than a
    /// request waiting or with the worker is dropped instead.
    pub fn send(&mut self, request: Request) -> Next {
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
    pub fn ready(&mut self) -> Next {
        match self.worker {
            Worker::Starting => self.next(),
            Worker::Stopped | Worker::Idle | Worker::Busy(_) => Next::Nothing,
        }
    }

    /// The worker answered the request it had. Ignored unless it had one.
    pub fn done(&mut self) -> Next {
        match self.worker {
            Worker::Busy(_) => self.next(),
            Worker::Stopped | Worker::Starting | Worker::Idle => Next::Nothing,
        }
    }

    /// Hands the worker the request waiting, if one is, or leaves it idle.
    fn next(&mut self) -> Next {
        match self.pending.pop() {
            Some(request) => {
                self.worker = Worker::Busy(request.generation());
                Next::Post(request)
            }
            None => {
                self.worker = Worker::Idle;
                Next::Nothing
            }
        }
    }

    /// The worker is gone, or couldn't start. Returns the generation to
    /// report as failed: the one it was working on, which is what killed it
    /// as far as anyone knows. A request waiting behind that one was never
    /// tried, so a new worker starts for it, from scratch.
    ///
    /// A worker that died before it was ready, or didn't start, never tried
    /// anything, and another would likely die the same way, so the request
    /// waiting for it is reported instead of kept, and no worker starts. So
    /// each worker that dies costs one request, and a crashing worker can't
    /// be restarted without end.
    pub fn fail(&mut self) -> (Option<Generation>, Next) {
        let failed = match self.worker {
            Worker::Busy(generation) => Some(generation),
            Worker::Starting => self.pending.pop().as_ref().map(Request::generation),
            Worker::Stopped | Worker::Idle => None,
        };
        if self.pending.is_empty() {
            self.worker = Worker::Stopped;
            (failed, Next::Nothing)
        } else {
            self.worker = Worker::Starting;
            (failed, Next::Start)
        }
    }
}

#[cfg(test)]
mod tests;
