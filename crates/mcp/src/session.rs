//! The one client session and the requests that reach it, past the
//! guard: [`State::handle`] decides each, with the time passed in, so
//! the idle end is tested without waiting.
//!
//! The order a POST is checked in: the body read as JSON-RPC (400 with
//! the error if it isn't); `initialize`, which needs no session and
//! starts one, replacing any; else `MCP-Session-Id` (none 400, unknown
//! or ended 404), then `MCP-Protocol-Version` (one not supported 400,
//! none the negotiated one); then the message: before
//! `notifications/initialized` only `ping`, a reused id -32600, unknown
//! methods -32601. `initialize`, `ping` and `tools/list` are answered
//! here; a `tools/call` goes to the app as a [`Dispatch`], answered
//! later through [`respond`].

use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use crate::http::{Request, Response, Status};
use crate::rpc::{self, Code, Id, Message};
use crate::tools::{self, Call, CallError, ToolReply};

/// The protocol versions spoken, newest first.
pub const VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

/// How long a session with no call in flight lives on unused.
pub const IDLE: Duration = Duration::from_secs(30 * 60);

/// How many request ids a session remembers, to refuse one reused; past
/// it, the oldest are forgotten.
pub const USED_IDS: usize = 4096;

/// The longest client name kept from `clientInfo`, in bytes.
pub const CLIENT_NAME: usize = crate::SHOWN;

/// What the client is told in `initialize`, at most 2 KB (Claude Code
/// cuts there), the key points first.
pub const INSTRUCTIONS: &str = "Varde is a CAD application. These tools act on the \
one design open in it. Call design_summary first. A result carries user_changes when the \
user changed the design since your previous call (and the revision); query again what it \
names, or all of it when it says the design was replaced. Refer to features and bodies by the names the Timeline and \
Objects show, and to faces, edges and corners by the keys body_topology gives, passed \
back verbatim. Name what you create (name, body_name): descriptive names keep the \
Timeline readable. Values are typed text in the design's units (\"20 mm\", \"90 deg\"); a \
bare number is in those units. Each edit is one undo step the user can undo; undo takes \
back your own newest edits while nothing came after them. An edit \
can come back applied but failing: fix it with a further edit. One can come back \
refused because the user is editing: leave it to them. Use render to check the result \
by eye.";

/// The session's id: 16 random bytes in base64url, sent as
/// `MCP-Session-Id`.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    fn generate() -> Result<Self, getrandom::Error> {
        let bytes: [u8; 16] = crate::random()?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// An id of `text`, for the app's tests, which feed it calls of
    /// their own.
    #[cfg(feature = "testing")]
    pub fn for_tests(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl fmt::Debug for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SessionId({})", self.0)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a session ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// A new `initialize` took its place.
    Replaced,
    /// The client sent DELETE.
    Deleted,
    /// Unused past [`IDLE`].
    Idle,
    /// The app ended it ([`State::end`]): Disconnect, a new token.
    Disconnected,
}

/// What happened to the session, for the app: its waiting calls end
/// with their session, and the panel shows who's connected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Started {
        session: SessionId,
        /// `clientInfo.name`, cut to [`CLIENT_NAME`] bytes.
        client: Option<String>,
    },
    Ended {
        session: SessionId,
        end: End,
    },
    /// `notifications/cancelled` for `request`, a call in flight: it
    /// ends. Sent once per call; a cancel for anything else is dropped.
    Cancelled {
        session: SessionId,
        request: Id,
    },
}

/// A tool call for the app to run, and answer with [`respond`].
#[derive(Clone, Debug, PartialEq)]
pub struct Dispatch {
    pub session: SessionId,
    pub id: Id,
    pub call: Call,
}

/// The HTTP response carrying `reply` to the request `id`, cut at
/// [`ToolReply::CAP`].
pub fn respond(id: &Id, reply: &ToolReply) -> Response {
    Response::json(rpc::success(id, reply.result(ToolReply::CAP)))
}

/// What to do with a request.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// Write this now.
    Respond(Response),
    /// Hand the call to the app, write its reply when it comes, then
    /// call [`State::finish`] with its session and id (the server does
    /// so when it drops its record of the call, however the wait ends).
    Dispatch(Dispatch),
}

/// A request's [`Answer`] and what it did to the session.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub answer: Answer,
    /// In the order they happened: a replacing `initialize` ends the old
    /// session, then starts the new.
    pub events: Vec<Event>,
}

impl Outcome {
    fn respond(response: Response) -> Self {
        Self {
            answer: Answer::Respond(response),
            events: Vec::new(),
        }
    }

    fn status(status: Status) -> Self {
        Self::respond(Response::empty(status))
    }

    fn with_events(mut self, events: Vec<Event>) -> Self {
        self.events = events;
        self
    }
}

struct Session {
    id: SessionId,
    initialized: bool,
    used: RecentIds,
    /// The ids of the calls handed to the app and not yet finished.
    in_flight: HashSet<Id>,
    /// The calls in flight already cancelled, so a cancel is passed on
    /// once per call: the app's events stay bounded by the calls in
    /// flight, however many cancels a client sends.
    cancelled: HashSet<Id>,
    last_active: Duration,
}

impl Session {
    fn idle(&self, now: Duration) -> bool {
        self.in_flight.is_empty() && now.saturating_sub(self.last_active) >= IDLE
    }

    /// Records `id`, `false` if it was used already or is a call still
    /// in flight (which may be older than the ids remembered).
    fn use_id(&mut self, id: &Id) -> bool {
        !self.in_flight.contains(id) && self.used.insert(id)
    }

    /// Handles the notification `n`: a cancel of a call in flight is an
    /// event.
    fn notification(&mut self, n: rpc::Notification) -> Outcome {
        let mut outcome = Outcome::status(Status::Accepted);
        match n.method.as_str() {
            "notifications/initialized" => self.initialized = true,
            "notifications/cancelled" => {
                if let Some(request) = n
                    .params
                    .as_ref()
                    .and_then(|p| p.get("requestId"))
                    .and_then(Id::from_value)
                    && self.in_flight.contains(&request)
                    && self.cancelled.insert(request.clone())
                {
                    outcome.events.push(Event::Cancelled {
                        session: self.id.clone(),
                        request,
                    });
                }
            }
            // Others are ignored.
            _ => {}
        }
        outcome
    }

    /// Answers the request `r`, or hands a `tools/call` to the app.
    fn request(&mut self, r: rpc::Request) -> Outcome {
        let error = |code, message| {
            Outcome::respond(Response::json(rpc::error(Some(&r.id), code, message)))
        };
        if !self.use_id(&r.id) {
            return error(Code::InvalidRequest, "This id was used already");
        }
        if !self.initialized && r.method != "ping" {
            return error(
                Code::InvalidRequest,
                "Not initialized: send notifications/initialized first",
            );
        }
        let success = |result| Outcome::respond(Response::json(rpc::success(&r.id, result)));
        match r.method.as_str() {
            "ping" => success(json!({})),
            "tools/list" => success(tools::list().clone()),
            "tools/call" => match tools::parse_call(r.params.as_ref()) {
                Ok(call) => {
                    self.in_flight.insert(r.id.clone());
                    Outcome {
                        answer: Answer::Dispatch(Dispatch {
                            session: self.id.clone(),
                            id: r.id,
                            call,
                        }),
                        events: Vec::new(),
                    }
                }
                Err(CallError::Params(message)) => error(Code::InvalidParams, &message),
                Err(error @ CallError::Arguments { .. }) => {
                    success(ToolReply::error(error.to_string()).result(ToolReply::CAP))
                }
            },
            _ => error(Code::MethodNotFound, "Method not found"),
        }
    }
}

/// The last [`USED_IDS`] request ids, oldest forgotten first.
#[derive(Default)]
struct RecentIds {
    /// Oldest first.
    order: VecDeque<Id>,
    set: HashSet<Id>,
}

impl RecentIds {
    /// Records `id`, `false` if it's remembered already.
    fn insert(&mut self, id: &Id) -> bool {
        if self.set.contains(id) {
            return false;
        }
        if self.order.len() >= USED_IDS
            && let Some(oldest) = self.order.pop_front()
        {
            self.set.remove(&oldest);
        }
        self.order.push_back(id.clone());
        self.set.insert(id.clone());
        true
    }
}

/// The server's session state: none, or the one session. Shared by the
/// connection threads behind a lock.
#[derive(Default)]
pub struct State {
    session: Option<Session>,
    limits: rpc::Limits,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// The open session's id, if any.
    pub fn session(&self) -> Option<&SessionId> {
        self.session.as_ref().map(|s| &s.id)
    }

    /// Ends the session, as Disconnect does.
    pub fn end(&mut self) -> Option<Event> {
        let session = self.session.take()?;
        Some(Event::Ended {
            session: session.id,
            end: End::Disconnected,
        })
    }

    /// Ends the session if it's been idle past [`IDLE`] at `now`, for a
    /// server checking now and then.
    pub fn expire(&mut self, now: Duration) -> Option<Event> {
        if !self.session.as_ref()?.idle(now) {
            return None;
        }
        let session = self.session.take()?;
        Some(Event::Ended {
            session: session.id,
            end: End::Idle,
        })
    }

    /// The call `id` of `session` was answered (or given up on) at
    /// `now`: the idle clock restarts. Nothing if that session's gone or
    /// the call was finished already, so finishing twice is harmless.
    pub fn finish(&mut self, session: &SessionId, id: &Id, now: Duration) {
        if let Some(s) = &mut self.session
            && s.id == *session
            && s.in_flight.remove(id)
        {
            s.cancelled.remove(id);
            s.last_active = now;
        }
    }

    /// Decides `request`, already passed by the guard, at `now`.
    pub fn handle(&mut self, request: &Request, now: Duration) -> Outcome {
        match request.method.as_str() {
            "POST" => self.post(request, now),
            "DELETE" => self.delete(request, now),
            // GET would open an SSE stream; Varde never pushes.
            _ => Outcome::respond(
                Response::empty(Status::MethodNotAllowed).with_header("Allow", "POST, DELETE"),
            ),
        }
    }

    fn delete(&mut self, request: &Request, now: Duration) -> Outcome {
        let mut events = Vec::new();
        if let Err(status) = self.find(request, now, &mut events) {
            return Outcome::status(status).with_events(events);
        }
        let session = self.session.take().expect("found");
        events.push(Event::Ended {
            session: session.id,
            end: End::Deleted,
        });
        Outcome::status(Status::NoContent).with_events(events)
    }

    /// Finds the request's session: `Ok` if it's the open one, else the
    /// status to answer. One found idle is ended, its event pushed.
    fn find(
        &mut self,
        request: &Request,
        now: Duration,
        events: &mut Vec<Event>,
    ) -> Result<(), Status> {
        let id = request
            .once("mcp-session-id")
            .ok()
            .flatten()
            .ok_or(Status::BadRequest)?
            .trim_matches([' ', '\t']);
        if self.session().is_none_or(|s| s.as_str() != id) {
            return Err(Status::NotFound);
        }
        if let Some(event) = self.expire(now) {
            events.push(event);
            return Err(Status::NotFound);
        }
        Ok(())
    }

    fn post(&mut self, request: &Request, now: Duration) -> Outcome {
        let message = match rpc::parse(&request.body, &self.limits) {
            Ok(message) => message,
            Err(error) => {
                let mut response = Response::json(error.reply());
                response.status = Status::BadRequest;
                return Outcome::respond(response);
            }
        };
        if let Message::Request(r) = &message
            && r.method == "initialize"
        {
            return self.initialize(r, now);
        }
        let mut events = Vec::new();
        if let Err(status) = self.find(request, now, &mut events) {
            return Outcome::status(status).with_events(events);
        }
        match request.once("mcp-protocol-version") {
            Ok(None) => {}
            Ok(Some(version)) if VERSIONS.contains(&version.trim_matches([' ', '\t'])) => {}
            _ => return Outcome::status(Status::BadRequest),
        }
        let session = self.session.as_mut().expect("found");
        session.last_active = now;
        match message {
            Message::Response => Outcome::status(Status::Accepted),
            Message::Notification(n) => session.notification(n),
            Message::Request(r) => session.request(r),
        }
    }

    /// Starts a session, replacing any: the client's protocol version if
    /// spoken, else the newest.
    fn initialize(&mut self, request: &rpc::Request, now: Duration) -> Outcome {
        let params = request.params.as_ref();
        let asked = params
            .and_then(|p| p.get("protocolVersion"))
            .and_then(Value::as_str);
        let version = VERSIONS
            .iter()
            .find(|v| Some(**v) == asked)
            .copied()
            .unwrap_or(VERSIONS[0]);
        let client = params
            .and_then(|p| p.get("clientInfo"))
            .and_then(|c| c.get("name"))
            .and_then(Value::as_str)
            .map(|name| crate::cut(name, CLIENT_NAME).to_owned());
        let Ok(id) = SessionId::generate() else {
            return Outcome::respond(Response::json(rpc::error(
                Some(&request.id),
                Code::Internal,
                "No random source for a session id",
            )));
        };
        let mut events = Vec::new();
        events.extend(self.expire(now));
        if let Some(old) = self.session.take() {
            events.push(Event::Ended {
                session: old.id,
                end: End::Replaced,
            });
        }
        let mut session = Session {
            id: id.clone(),
            initialized: false,
            used: RecentIds::default(),
            in_flight: HashSet::new(),
            cancelled: HashSet::new(),
            last_active: now,
        };
        session.use_id(&request.id);
        self.session = Some(session);
        events.push(Event::Started {
            session: id.clone(),
            client,
        });
        let result = json!({
            "protocolVersion": version,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "varde",
                "title": "Varde",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": INSTRUCTIONS,
        });
        Outcome {
            answer: Answer::Respond(
                Response::json(rpc::success(&request.id, result))
                    .with_header("MCP-Session-Id", id.0),
            ),
            events,
        }
    }
}

#[cfg(test)]
mod tests;
