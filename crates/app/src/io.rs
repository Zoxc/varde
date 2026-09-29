//! The IO lane, as the app sees it: requests sent before it has started
//! wait, in order, and requests making a file are tagged.

use std::mem;

use varde_io::{Closing, FileId, OpenId, Request as IoRequest, Transport};

/// The app's end of the IO lane, see [`varde_io::lane`].
pub(crate) struct Io {
    lane: Lane,
    /// Tags the next [`IoRequest::Open`], or other request making a file.
    next_open: OpenId,
}

/// Whether the lane has started yet, see [`Varde::io_lane`].
///
/// [`Varde::io_lane`]: crate::Varde::io_lane
enum Lane {
    /// Not yet: the requests sent so far, in order.
    Starting(Vec<IoRequest>),
    Running(Box<dyn Transport<IoRequest>>),
}

impl Io {
    pub(crate) fn new() -> Self {
        Self {
            lane: Lane::Starting(Vec::new()),
            next_open: OpenId(0),
        }
    }

    pub(crate) fn send(&mut self, request: IoRequest) {
        match &mut self.lane {
            Lane::Starting(waiting) => waiting.push(request),
            Lane::Running(lane) => lane.send(request),
        }
    }

    /// Starts sending to `lane`, first what was sent before it started.
    pub(crate) fn ready(&mut self, mut lane: Box<dyn Transport<IoRequest>>) {
        if let Lane::Starting(waiting) = mem::replace(&mut self.lane, Lane::Starting(Vec::new())) {
            for request in waiting {
                lane.send(request);
            }
        }
        self.lane = Lane::Running(lane);
    }

    /// The requests waiting for the lane to start.
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> &[IoRequest] {
        match &self.lane {
            Lane::Starting(waiting) => waiting,
            Lane::Running(_) => &[],
        }
    }

    /// Gives up on the open tagged `id`, if any: the lane closes the file
    /// it opened. Sent before whatever the user moved on to, so a later
    /// open of the same design doesn't find it still locked, which would
    /// make it read-only.
    pub(crate) fn abandon(&mut self, id: Option<OpenId>) {
        if let Some(id) = id {
            self.send(IoRequest::Abandon { id });
        }
    }

    /// Closes `file` cleanly, see [`IoRequest::Close`]: nothing is left to
    /// write it, or what it holds is saved.
    pub(crate) fn close_clean(&mut self, file: FileId) {
        self.send(IoRequest::Close {
            file,
            closing: Closing::Clean,
        });
    }

    /// A new tag for a request making a file.
    pub(crate) fn tag(&mut self) -> OpenId {
        let id = self.next_open;
        // Counted up once per click, so it never gets near saturating.
        self.next_open = OpenId(id.0.saturating_add(1));
        id
    }

    /// Asks for a store entry for a new design, returning the tag its
    /// answer will carry.
    pub(crate) fn create(&mut self) -> OpenId {
        let id = self.tag();
        self.send(IoRequest::New { id });
        id
    }
}
