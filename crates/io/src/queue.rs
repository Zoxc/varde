//! The order the IO lane handles requests in, natively and on the web.

use std::collections::{VecDeque, vec_deque};
use std::mem;

use varde_lane::Pending;

use crate::{FileId, Request};

/// What [`Queue`] holds: requests, or requests with something the lane
/// keeps along with them, like the number the web lane answers by.
pub(crate) trait Queued {
    fn request(&self) -> &Request;
}

impl Queued for Request {
    fn request(&self) -> &Request {
        self
    }
}

/// Requests not yet started, in order. Only [`Queue::push`] adds to it, so
/// what it holds always follows its rule.
#[derive(Debug)]
pub(crate) struct Queue<T = Request> {
    requests: VecDeque<T>,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            requests: VecDeque::new(),
        }
    }
}

/// What's queued, in order, e.g. to answer as a lane stops.
impl<T> IntoIterator for Queue<T> {
    type Item = T;
    type IntoIter = vec_deque::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.requests.into_iter()
    }
}

impl<T: Queued> Pending<T> for Queue<T> {
    /// Queues `request`. A `Save`, `AutoSave`, `WriteRecent` or
    /// `WriteSettings` replaces the last one queued of its kind and target,
    /// if nothing else for that target is queued after it. Never across a `Flush`, which is answered
    /// once everything sent before it is done, nor a `KeepDownload`, which
    /// a clean close may go back to, see [`Request::Close`], and which
    /// nothing replaces. The replaced request is returned for the caller to
    /// drop outside its lock.
    fn push(&mut self, item: T) -> Option<T> {
        let request = item.request();
        let replaced = request
            .target()
            .filter(|_| request.replaces())
            .and_then(|target| {
                self.requests.iter().rposition(|queued| {
                    let queued = queued.request();
                    matches!(queued, Request::Flush) || queued.target() == Some(target)
                })
            })
            .filter(|&at| {
                mem::discriminant(self.requests[at].request()) == mem::discriminant(request)
            })
            .and_then(|at| self.requests.remove(at));
        self.requests.push_back(item);
        replaced
    }

    fn pop(&mut self) -> Option<T> {
        self.requests.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }
}

/// What a request acts on, which requests replacing queued ones go by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    File(FileId),
    Recent,
    Settings,
}

impl Request {
    /// Whether the request replaces a queued one like it, see
    /// [`Queue::push`]: only the newest state of its target matters.
    fn replaces(&self) -> bool {
        matches!(
            self,
            Request::Save { .. }
                | Request::AutoSave { .. }
                | Request::WriteRecent { .. }
                | Request::WriteSettings { .. }
        )
    }

    /// What the request reads or writes, if it's something requests
    /// replacing queued ones go by: an open file, the recent files list or
    /// the settings. A request that only reads its target, like
    /// `LoadRecent`, is never
    /// replaced but keeps a write behind it from replacing one before it.
    fn target(&self) -> Option<Target> {
        match self {
            Request::Save { file, .. }
            | Request::AutoSave { file, .. }
            | Request::KeepDownload { file, .. }
            | Request::DiscardRecovery { file }
            | Request::OpenFound { file, .. }
            | Request::Close { file, .. }
            | Request::SaveAs {
                file: Some(file), ..
            } => Some(Target::File(*file)),
            Request::WriteRecent { .. } | Request::LoadRecent => Some(Target::Recent),
            Request::WriteSettings { .. } | Request::LoadSettings => Some(Target::Settings),
            Request::SaveAs { file: None, .. }
            | Request::Open { .. }
            | Request::New { .. }
            | Request::Abandon { .. }
            | Request::ListRecovered
            | Request::OpenRecovered { .. }
            | Request::DiscardRecovered { .. }
            | Request::Export { .. }
            | Request::Flush => None,
        }
    }
}

#[cfg(test)]
mod tests;
