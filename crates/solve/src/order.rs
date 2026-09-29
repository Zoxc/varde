//! The order the solver lane takes requests in, natively and on the web.
//!
//! Proposals are ordered: each is made on the committed sketch the one
//! before it produced, so they queue and none is dropped. Analyses queue
//! with them, a newer one replacing one still waiting, since only the
//! newest revision's matters. Drag steps are latest wins, in a slot of
//! their own. The queue and the slot take turns: after a drag step the
//! next queued request goes first, and after a queued request the drag
//! step waiting, so a proposal waits for at most one drag step, however
//! fast they come, and a drag step for at most one proposal, however many
//! are queued.
//!
//! Natively it's the thread's pending requests (`src/thread.rs`), on the
//! web what the page's [`Mailbox`](varde_lane::mailbox::Mailbox) keeps
//! (`src/worker.rs`). Plain Rust, so the rule is written and tested once.

use std::collections::VecDeque;

use varde_lane::Pending;

use crate::Request;

/// Requests not yet started.
#[derive(Debug, Default)]
pub struct Order {
    /// Proposals and analyses, in the order sent.
    queue: VecDeque<Request>,
    /// The newest drag step.
    drag: Option<Request>,
    /// The newest drag session sent, if any was.
    session: Option<u64>,
    /// Whether the last request taken was a drag step, so the queue goes
    /// next.
    dragged: bool,
}

impl Pending<Request> for Order {
    /// Queues a proposal or an analysis, or puts a drag step in its slot.
    /// Returns what that replaces, or the request itself if it's refused,
    /// for the caller to drop when it likes: a waiting analysis is
    /// replaced by a newer one, a waiting drag step by any newer one, and
    /// a step of a session older than one sent before it is refused, even
    /// one already taken, since that drag is over.
    fn push(&mut self, request: Request) -> Option<Request> {
        match request {
            Request::Drag { session, .. } => {
                if self.session > Some(session) {
                    return Some(request);
                }
                self.session = Some(session);
                self.drag.replace(request)
            }
            Request::Analyse { .. } => {
                let replaced = self
                    .queue
                    .iter()
                    .position(|queued| matches!(queued, Request::Analyse { .. }))
                    .and_then(|at| self.queue.remove(at));
                self.queue.push_back(request);
                replaced
            }
            Request::Propose { .. } => {
                self.queue.push_back(request);
                None
            }
        }
    }

    /// Takes the next request: the queue's and the drag step's in turn,
    /// either if the other has none.
    fn pop(&mut self) -> Option<Request> {
        let next = if self.dragged {
            self.queue.pop_front().or_else(|| self.drag.take())
        } else {
            self.drag.take().or_else(|| self.queue.pop_front())
        };
        self.dragged = matches!(next, Some(Request::Drag { .. }));
        next
    }

    fn is_empty(&self) -> bool {
        self.queue.is_empty() && self.drag.is_none()
    }
}

#[cfg(test)]
mod tests;
