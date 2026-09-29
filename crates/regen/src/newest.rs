//! The slot both lanes keep their waiting request in: latest wins.
//!
//! Natively it's the thread's pending request (`src/thread.rs`), on the web
//! the page's `Mailbox` (`src/mailbox.rs`). Plain Rust, so the rule is
//! written and tested once.

use varde_document::Generation;
use varde_lane::Pending;

use crate::Request;

/// The newest request not yet started, and the newest generation sent.
#[derive(Debug, Default)]
pub struct Newest {
    request: Option<Request>,
    /// The newest generation sent, if any was.
    wanted: Option<Generation>,
}

impl Pending<Request> for Newest {
    /// Replaces the request waiting. Refuses one older than a request sent
    /// before it, even one already taken: clones sending at once can get
    /// here out of order, and the newest must be neither lost nor followed
    /// by an older one. Returns the request that lost, refused or replaced,
    /// for the caller to drop when it likes.
    fn push(&mut self, request: Request) -> Option<Request> {
        if self.wanted > Some(request.generation()) {
            return Some(request);
        }
        self.wanted = Some(request.generation());
        self.request.replace(request)
    }

    /// Takes the request waiting, to start it.
    fn pop(&mut self) -> Option<Request> {
        self.request.take()
    }

    fn is_empty(&self) -> bool {
        self.request.is_none()
    }
}

#[cfg(test)]
mod tests;
