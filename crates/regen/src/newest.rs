//! What both lanes keep their waiting requests in: latest wins for
//! regenerations, exports and client queries ([`Request::Query`]) in
//! order.
//!
//! Natively it's the thread's pending requests (`src/thread.rs`), on the
//! web what the page's [`Mailbox`](varde_lane::mailbox::Mailbox) keeps
//! (`src/worker.rs`). Plain Rust, so the rule is
//! written and tested once.

use std::collections::VecDeque;

use varde_document::Generation;
use varde_lane::Pending;

use crate::Request;

/// The newest [`Request::Regenerate`] not yet started, the newest
/// generation sent, and the [`Request::Export`]s and
/// [`Request::Query`]s not yet started.
#[derive(Debug, Default)]
pub struct Newest {
    request: Option<Request>,
    /// The newest generation sent, if any was.
    wanted: Option<Generation>,
    /// Exports and queries, in the order sent: each is answered, so
    /// none replaces another, and no regeneration replaces one. The user
    /// (or a client) waits on them, so they go first.
    exports: VecDeque<Request>,
}

impl Pending<Request> for Newest {
    /// Queues an export or edge facts. Replaces the regeneration waiting with a newer
    /// one. Refuses one older than a regeneration sent before it, even one
    /// already taken: clones sending at once can get here out of order, and
    /// the newest must be neither lost nor followed by an older one.
    /// Returns the request that lost, refused or replaced, for the caller
    /// to drop when it likes.
    fn push(&mut self, request: Request) -> Option<Request> {
        let generation = match &request {
            Request::Regenerate { generation, .. } => *generation,
            Request::Export { .. } | Request::Query { .. } => {
                self.exports.push_back(request);
                return None;
            }
        };
        if self.wanted > Some(generation) {
            return Some(request);
        }
        self.wanted = Some(generation);
        self.request.replace(request)
    }

    /// Takes the request to start: the oldest export or edge facts, else the
    /// regeneration waiting.
    fn pop(&mut self) -> Option<Request> {
        self.exports.pop_front().or_else(|| self.request.take())
    }

    fn is_empty(&self) -> bool {
        self.request.is_none() && self.exports.is_empty()
    }
}

#[cfg(test)]
mod tests;
