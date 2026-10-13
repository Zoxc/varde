//! The app's side of the MCP server (`varde-mcp`, see `agents/mcp.md`, "The
//! app's side"): the server's subscription, its state on `Varde`, and the
//! tool calls answered from the open design. Native only.
//!
//! The settings' switch turns the feature on (the panel and its toolbar
//! toggle); the server runs once the panel's Start sets [`Mcp::running`]
//! (never saved: each launch starts stopped), while the feature is on
//! and the token has come from the IO lane ([`Mcp::configure`] sets
//! [`Mcp::serve`], which tests may also set directly). Calls are
//! answered at once, or wait in the open design for what they need
//! (`waiting.rs`), at most [`waiting::BOUND`]; [`Mcp::sync`] sends the
//! replies of the calls done after each message.
//!
//! The calls are kept in a log for the MCP panel ([`Mcp::log`]), bounded
//! by [`LOG_CAP`]: the session's, cleared as the next session starts, the
//! server stops or restarts, or the design closes.

mod args;
mod ask;
mod blend;
mod changes;
mod draw;
mod edit;
pub(crate) mod key;
mod make;
mod modify;
mod motion;
mod query;
pub(crate) mod revert;
mod set;
mod sketches;
mod sweep;
mod text;
mod undo;
pub(crate) mod waiting;

use std::collections::VecDeque;
use std::net::SocketAddr;

use iced::{Subscription, Task};
use varde_io::Request as IoRequest;
use varde_mcp::guard::Token;
use varde_mcp::server::Event;
use varde_mcp::session::{self, SessionId};
use varde_mcp::tools::{Call, ToolReply};
use varde_view::{McpCallState, McpPanel, McpTool};

use crate::doc::Doc;
use crate::io::Io;
use crate::message::{ForDoc, Message};
use text::{args, plain, summary, told_line, tool, undid_row, user_rows};

/// The MCP server's state, the app's.
#[derive(Debug, Default)]
pub(crate) struct Mcp {
    /// Where to serve, while the server is on: set by
    /// [`Mcp::configure`], from the settings and [`Mcp::token`].
    pub(crate) serve: Option<Serve>,
    /// The bearer token, once the IO lane has read or made it.
    token: Option<Token>,
    /// How many servers were started: the next one's [`Serve::start`].
    starts: u64,
    /// The running server's handle, once it's said ready.
    handle: Option<varde_mcp::server::Handle>,
    status: Status,
    /// The session open, if one is.
    session: Option<Session>,
    /// The client's calls, oldest first, at most [`LOG_CAP`]: what the
    /// panel lists.
    log: VecDeque<Logged>,
    /// How many calls the log let go, the oldest first, since it was last
    /// cleared.
    dropped: usize,
    /// The number the next call logged takes, to find its row again.
    serials: u64,
    /// The client whose session ended, with the name it gave, while the
    /// log still holds its calls.
    gone: Option<Option<String>>,
    /// Whether the MCP panel is open.
    pub(crate) open: bool,
    /// Whether the user started the server from the panel (and hasn't
    /// stopped it or turned the feature off since). Not saved.
    pub(crate) running: bool,
    /// The design as the session's previous call left it (at its
    /// `initialize` before its first): what the next result's
    /// `user_changes` are from (`changes.rs`). None until a design is
    /// open in the session.
    seen: Option<changes::Seen>,
    /// The user's changes since the session's previous call, a line
    /// each, for the panel's rows after the last call; with the state of
    /// the design they were worked out at, so they're worked out again
    /// only when it changes.
    live: Option<(changes::Seen, Vec<String>)>,
    /// The edit row whose context menu is open, by its serial.
    menu: Option<u64>,
    /// The reverts made from the panel, oldest first, at most
    /// [`LOG_CAP`]: rows of their own, and what greys the rows they took
    /// back (`revert.rs`).
    reverts: VecDeque<revert::Reverted>,
}

/// The most calls the log keeps, the oldest let go first. A row is a few
/// hundred bytes and a widget row the panel builds on every view (it
/// isn't a virtual list), so a client polling for a whole session can't
/// make the panel slow or the app grow; 200 rows are far more than a
/// task's worth of calls to look back over. The edit rows' designs
/// before and after are bounded apart ([`revert::SNAPSHOTS`]).
pub(crate) const LOG_CAP: usize = 200;

/// The session open on the server.
#[derive(Debug, PartialEq)]
struct Session {
    id: SessionId,
    /// The name its client gave, if any.
    client: Option<String>,
    /// When it started.
    since: varde_io::UnixSeconds,
}

/// Pushes `item` onto `queue`, first letting the oldest go if `queue`
/// holds `cap` already: the one it let go, if any.
pub(crate) fn push_bounded<T>(queue: &mut VecDeque<T>, cap: usize, item: T) -> Option<T> {
    let evicted = if queue.len() >= cap {
        queue.pop_front()
    } else {
        None
    };
    queue.push_back(item);
    evicted
}

/// The longest result summary kept for a row's tooltip, in bytes.
const RESULT_BYTES: usize = 300;

/// A call in the log, with the user's own edits its result told the
/// client of (shown as rows before it), and an applied edit's states
/// for Revert.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Logged {
    serial: u64,
    pub(crate) tool: McpTool,
    /// Its arguments, as typed text.
    pub(crate) args: String,
    /// When it came.
    pub(crate) came: varde_io::UnixSeconds,
    pub(crate) state: McpCallState,
    /// What came of it, summed up, once answered.
    pub(crate) result: Option<String>,
    /// The user's changes its result told the client of, a line each:
    /// its "Told: ..." line.
    pub(crate) told: Vec<String>,
    /// The same but for the panel's reverts and what they changed
    /// (`changes::Changes::panel`): the rows of the user's own edits
    /// before it.
    pub(crate) told_rows: Vec<String>,
    /// The revision of the design the lines `told` were worked out on
    /// (an edit's before): undone, they're greyed.
    pub(crate) told_at: Option<varde_document::Revision>,
    /// An applied edit's states, for its row's menu.
    pub(crate) edit: Option<revert::Edited>,
}

/// The port and token the server listens with; the subscription is keyed
/// by both, so a new one restarts the server. `start` tells this server's
/// events from those of one stopped, still queued as it stopped.
#[derive(Debug, Clone, Hash)]
pub(crate) struct Serve {
    pub(crate) port: u16,
    pub(crate) token: Token,
    pub(crate) start: u64,
}

/// How the server is.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) enum Status {
    #[default]
    Off,
    Listening(SocketAddr),
    /// The port couldn't be bound: why.
    BindFailed(String),
}

impl Mcp {
    /// The server, while `on` and it has somewhere to serve: its events
    /// as [`Message::Mcp`], tagged with its [`Serve::start`].
    pub(crate) fn subscription(&self, on: bool) -> Subscription<Message> {
        fn start(serve: &Serve) -> impl iced::futures::Stream<Item = Message> + use<> {
            use iced::futures::StreamExt;
            let start = serve.start;
            varde_mcp::server::start(serve.port, serve.token.clone())
                .map(move |event| Message::Mcp(start, event))
        }
        match &self.serve {
            Some(serve) if on => Subscription::run_with(serve.clone(), start),
            _ => Subscription::none(),
        }
    }

    /// Serves on `port` if the feature is `on`, the server
    /// [`Mcp::running`] and the token has come, else stops; the feature
    /// off also stops it and closes the panel. A new port or token
    /// restarts the server, as the subscription is keyed by them. Stopped
    /// or restarted, the old server is forgotten at once, and the events
    /// it still had queued are dropped as they come; stopped, or
    /// restarted from listening, its status is [`Status::Off`]. A
    /// [`Status::BindFailed`] stays until the server is stopped or
    /// another start says how it went.
    pub(crate) fn configure(&mut self, on: bool, port: u16) {
        self.reconfigure(on, port, false);
    }

    /// [`Mcp::configure`], keeping the log if `keep_log` (a session
    /// ended at once as the token was regenerated, its client shown gone).
    fn reconfigure(&mut self, on: bool, port: u16, keep_log: bool) {
        if !on {
            self.running = false;
            self.open = false;
        }
        let on = on && self.running;
        let wanted = (self.token.clone()).filter(|_| on);
        let same = match (&self.serve, &wanted) {
            (Some(serve), Some(token)) => serve.port == port && serve.token == *token,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        if self.serve.is_some() {
            self.restarted(keep_log);
            self.handle = None;
            // The old server's address is no longer listened on.
            if matches!(self.status, Status::Listening(_)) {
                self.status = Status::Off;
            }
        }
        self.serve = wanted.map(|token| {
            self.starts = self.starts.wrapping_add(1);
            Serve {
                port,
                token,
                start: self.starts,
            }
        });
        if self.serve.is_none() {
            self.status = Status::Off;
        }
    }

    /// Takes the token the IO lane read or made, `regenerated` on the
    /// user's asking, and serves with it, the feature `on`, on `port`
    /// ([`Mcp::configure`]). Regenerated, the session ends at once: its
    /// calls waiting in `doc`, the open design, are answered as at a
    /// session's end and its client is shown gone, before the server
    /// restarts with the new token (whose old `Ended` is dropped).
    pub(crate) fn token_loaded(
        &mut self,
        token: Token,
        regenerated: bool,
        on: bool,
        port: u16,
        doc: Option<&mut Doc>,
    ) {
        let mut ended = false;
        if regenerated {
            if let Some(handle) = &self.handle {
                handle.end_session();
            }
            if let Some(session) = self.open_session().cloned() {
                self.session_ended(&session, doc);
                ended = true;
            }
        }
        self.token = Some(token);
        self.reconfigure(on, port, ended);
    }

    /// The panel's Start, the feature `on`, serving on `port`: starts the
    /// server, or, running but the port not bound, tries it again.
    pub(crate) fn start(&mut self, on: bool, port: u16) {
        if self.running {
            self.retry();
        } else {
            self.running = true;
            self.configure(on, port);
        }
    }

    /// The panel's Stop: stops the server, the feature staying `on`.
    pub(crate) fn stop(&mut self, on: bool, port: u16) {
        self.running = false;
        self.configure(on, port);
    }

    /// Tries the port again after it couldn't be bound. The subscription
    /// is keyed by the start too, so a new one starts the server again.
    fn retry(&mut self) {
        if !matches!(self.status, Status::BindFailed(_)) {
            return;
        }
        if let Some(serve) = &mut self.serve {
            self.starts = self.starts.wrapping_add(1);
            serve.start = self.starts;
            self.status = Status::Off;
        }
    }

    /// The design closed (another opened, back to the welcome screen):
    /// the log's rows were of it, so they go.
    pub(crate) fn design_closed(&mut self) {
        self.clear_log();
    }

    /// Empties the log.
    fn clear_log(&mut self) {
        self.log.clear();
        self.reverts.clear();
        self.menu = None;
        self.dropped = 0;
        self.gone = None;
    }

    /// The panel, its toggle, the status bar's chip and the popup `doc`,
    /// the open design, has up, the feature `on` or not, serving on
    /// `port`, at `now`. Always some here; none on the web.
    pub(crate) fn panel<'a>(
        &'a self,
        on: bool,
        port: u16,
        now: varde_io::UnixSeconds,
        doc: Option<&'a Doc>,
    ) -> Option<varde_view::McpShown<'a>> {
        let client = (self.session.as_ref())
            .map(|session| session.client.as_deref())
            .or(self.gone.as_ref().map(Option::as_deref))
            .flatten();
        Some(varde_view::McpShown {
            ask: (doc.and_then(|doc| doc.mcp.ask.as_ref())).map(|ask| ask.shown(client)),
            on,
            open: self.open,
            status: self.shown(now).status,
            port,
            client: (self.session.as_ref()).map(|session| session.client.as_deref()),
            gone: self.gone.as_ref().map(Option::as_deref),
            rows: self.rows(now, doc),
            dropped: self.dropped,
            revert: (doc.and_then(|doc| doc.mcp.revert_ask.as_ref())).map(|ask| {
                varde_view::McpRevertAsk {
                    to: &ask.to,
                    differences: &ask.differences,
                }
            }),
        })
    }

    /// The panel's rows at `now`, greyed as `doc`'s design took them
    /// back: each call after the user's edits its result told of, the
    /// reverts in their places, then the user's edits since the last.
    fn rows<'a>(
        &'a self,
        now: varde_io::UnixSeconds,
        doc: Option<&'a Doc>,
    ) -> Vec<varde_view::McpRow<'a>> {
        let mut rows = Vec::new();
        let mut reverts = self.reverts.iter().peekable();
        let revert_row = |revert: &'a revert::Reverted| {
            varde_view::McpRow::Revert(varde_view::McpRevert {
                to: revert.to.as_str().into(),
                tip: revert.tip.as_str().into(),
                reverted: doc.is_some_and(|doc| Mcp::taken_back(doc, Some(revert.after))),
            })
        };
        for logged in &self.log {
            while let Some(revert) = reverts.next_if(|revert| revert.serial < logged.serial) {
                rows.push(revert_row(revert));
            }
            let user_reverted = doc.is_some_and(|doc| Mcp::taken_back(doc, logged.told_at));
            rows.extend(user_rows(&logged.told_rows, user_reverted));
            let menu = self.menu == Some(logged.serial);
            let edit = (logged.edit.as_ref()).map(|edit| edit.shown(logged.serial, menu, doc));
            rows.push(varde_view::McpRow::Call(varde_view::McpCall {
                tool: logged.tool,
                args: logged.args.as_str().into(),
                ago: crate::when::ago(logged.came, now),
                state: logged.state,
                result: logged.result.as_deref(),
                told: told_line(&logged.told).map(Into::into),
                edit,
            }));
        }
        rows.extend(reverts.map(revert_row));
        rows.extend(user_rows(
            self.live.as_ref().map_or(&[][..], |(_, lines)| lines),
            false,
        ));
        rows
    }

    /// Logs `call` as waiting, letting the oldest go past [`LOG_CAP`]:
    /// its serial.
    fn log_call(&mut self, call: &Call) -> u64 {
        let serial = self.next_serial();
        let logged = Logged {
            serial,
            tool: tool(call),
            args: plain(&args(call)),
            came: crate::when::now(),
            state: McpCallState::Waiting,
            result: None,
            told: Vec::new(),
            told_rows: Vec::new(),
            told_at: None,
            edit: None,
        };
        if push_bounded(&mut self.log, LOG_CAP, logged).is_some() {
            self.dropped = self.dropped.saturating_add(1);
        }
        serial
    }

    /// The next serial: the one order the client's calls and the panel's
    /// reverts share, which [`Mcp::rows`] interleaves them by.
    fn next_serial(&mut self) -> u64 {
        let serial = self.serials;
        self.serials = self.serials.wrapping_add(1);
        serial
    }

    /// Marks the call logged as `serial` answered with `reply`, saying
    /// `state`, if it's still in the log.
    fn log_answer(&mut self, serial: u64, call: &Call, reply: &ToolReply, state: McpCallState) {
        if let Some(logged) = self.log.iter_mut().find(|logged| logged.serial == serial) {
            logged.state = state;
            logged.result = Some(summary(call, reply));
        }
    }

    /// `reply`, to `from`, carrying the user's changes to `doc`'s design
    /// since the session's previous call if `from` is of the session
    /// open, and the design's revision; with the changes it carries, for
    /// the call's row, unless it couldn't carry them (too long,
    /// [`changes::fitted`]), which leaves them for the next. Nothing is
    /// taken as told here: [`Mcp::told`] does that once the reply is
    /// taken.
    fn tell(
        &self,
        doc: &Doc,
        from: &waiting::Caller,
        reply: ToolReply,
    ) -> (ToolReply, Option<changes::Changes>) {
        if self.open_session() != Some(&from.session) {
            return (reply, None);
        }
        let changes = match &self.seen {
            Some(seen) => changes::since(doc, seen, from.start, &from.session),
            None => changes::Changes::default(),
        };
        let (reply, told) = changes::fitted(reply, &changes, doc.editor.revision());
        (reply, told.then_some(changes))
    }

    /// Takes the design now as what `from`'s session saw last: its
    /// reply carrying the user's changes ([`Mcp::tell`]) was taken.
    fn told(&mut self, doc: &mut Doc, from: &waiting::Caller) {
        for step in &mut doc.mcp.history.steps {
            if step.of(from.start, &from.session) {
                step.told_done = doc.editor.is_done(step.after);
            }
        }
        self.seen = Some(changes::Seen::of(doc));
        self.live = None;
    }

    /// Works the panel's rows of the user's changes since the session's
    /// previous call out again, if `doc`'s design changed since and the
    /// panel is open (opened, the sync after works them out).
    fn refresh_live(&mut self, doc: &Doc) {
        let (true, Some(session), Some(seen), Some(start)) =
            (self.open, self.open_session(), &self.seen, self.serving())
        else {
            self.live = None;
            return;
        };
        if self.live.as_ref().is_some_and(|(at, _)| at.is_now(doc)) {
            return;
        }
        let lines = (changes::since(doc, seen, start, session).panel.iter())
            .map(|line| plain(line))
            .collect();
        self.live = Some((changes::Seen::of(doc), lines));
    }

    /// Ends the client's session, as the settings' Disconnect does: its
    /// `Ended` comes from the server.
    pub(crate) fn disconnect(&self) {
        if let Some(handle) = &self.handle {
            handle.end_session();
        }
    }

    /// What the settings' MCP tab shows of the server at `now`: stopped
    /// unless [`Mcp::running`], else starting until the server says how
    /// it went.
    pub(crate) fn shown(&self, now: varde_io::UnixSeconds) -> varde_view::McpState<'_> {
        use varde_view::{McpClient, McpStatus};
        varde_view::McpState {
            status: match (&self.status, self.running) {
                (_, false) => McpStatus::Off,
                (Status::Listening(address), true) => McpStatus::Listening(address.port()),
                (Status::BindFailed(why), true) => McpStatus::BindFailed(why),
                (Status::Off, true) => McpStatus::Starting,
            },
            client: (self.session.as_ref()).map(|session| McpClient {
                name: session.client.as_deref(),
                since: crate::when::ago_in_sentence(session.since, now),
            }),
            token: self.token.as_ref().map(Token::to_string),
            token_shown: false,
            port_typed: None,
            exe: exe(),
        }
    }

    /// Takes the server's `event`, answering a call from `doc`, the open
    /// design, if there is one. `start` says which server it's from: one
    /// from a server since stopped is dropped (a call answered with an
    /// error, which goes nowhere).
    pub(crate) fn event(&mut self, start: u64, event: Event, doc: Option<&mut Doc>) {
        if self.serving() != Some(start) {
            if let Event::Call { reply, .. } = event {
                reply.send(ToolReply::error(waiting::SERVER_STOPPED));
            }
            return;
        }
        match event {
            Event::Ready(handle) => {
                self.status = Status::Listening(handle.address());
                self.handle = Some(handle);
            }
            Event::BindFailed(why) => {
                self.status = Status::BindFailed(why);
                self.handle = None;
            }
            Event::Call {
                session,
                id,
                call,
                reply,
            } => {
                let serial = self.log_call(&call);
                let Some(doc) = doc else {
                    let answer = ToolReply::error(query::NO_DESIGN);
                    let state = match call.edits() {
                        true => McpCallState::NotApplied,
                        false => McpCallState::Failed,
                    };
                    self.log_answer(serial, &call, &answer, state);
                    reply.send(answer);
                    return;
                };
                let from = waiting::Caller {
                    serial,
                    start,
                    session,
                    id,
                    came: reply.came(),
                    reply,
                };
                doc.mcp_call(call, from);
                self.sync(Some(doc));
            }
            // A new client's calls start a new log.
            Event::Session(session::Event::Started { session, client }) => {
                self.clear_log();
                // Its first call's changes are from the design now.
                self.seen = doc.as_deref().map(changes::Seen::of);
                self.live = None;
                let client =
                    (client.map(|name| plain(&name))).filter(|name| !name.trim().is_empty());
                self.session = Some(Session {
                    id: session,
                    client,
                    since: crate::when::now(),
                });
            }
            Event::Session(session::Event::Ended { session, end: _ }) => {
                self.session_ended(&session, doc);
            }
            // Its client ignores the reply: the user's changes aren't
            // told in it, but left for the next.
            Event::Session(session::Event::Cancelled { session, request }) => {
                let which = |from: &waiting::Caller| from.session == session && from.id == request;
                if let Some(doc) = doc {
                    doc.mcp_end(which, waiting::Ended::Cancelled);
                    self.sync_untold(Some(doc), which);
                }
            }
        }
    }

    /// The open session's id, if one is open.
    fn open_session(&self) -> Option<&SessionId> {
        self.session.as_ref().map(|session| &session.id)
    }

    /// The running server's start, if one is running: what tells its
    /// calls and events from a stopped one's.
    fn serving(&self) -> Option<u64> {
        self.serve.as_ref().map(|serve| serve.start)
    }

    /// The session `session` ended: its client shown gone if it's the
    /// one open, its calls waiting in `doc` answered so.
    fn session_ended(&mut self, session: &SessionId, doc: Option<&mut Doc>) {
        if self.open_session() == Some(session)
            && let Some(open) = self.session.take()
        {
            self.gone = Some(open.client);
        }
        self.end(
            doc,
            |from| from.session == *session,
            waiting::Ended::Session,
        );
    }

    /// Forgets the server before, stopped or replaced by one on another
    /// port or token: its session, which no `Ended` will come for, and its
    /// log unless `keep_log`. Its calls waiting in the design are
    /// answered by the next [`Mcp::sync`].
    fn restarted(&mut self, keep_log: bool) {
        self.session = None;
        if !keep_log {
            self.clear_log();
        }
    }

    /// Ends the calls waiting in `doc`, the open design, that have waited
    /// [`waiting::BOUND`] by `now`: the tick while one waits.
    pub(crate) fn tick(&mut self, now: iced::time::Instant, doc: Option<&mut Doc>) {
        self.end(doc, |from| from.late(now), waiting::Ended::Bound);
    }

    /// Fails the calls waiting in `doc` as it closes.
    pub(crate) fn closing(&mut self, doc: Option<&mut Doc>) {
        self.end(doc, |_| true, waiting::Ended::DesignClosed);
    }

    /// Fails the calls waiting in `doc` as Varde quits: what comes for
    /// them after isn't taken ([`crate::while_quitting`]).
    pub(crate) fn quitting(&mut self, doc: Option<&mut Doc>) {
        self.end(doc, |_| true, waiting::Ended::Quitting);
    }

    /// Ends the calls waiting in `doc`, the open design, that `which`
    /// picks, answered with `why`, an error, and sends their replies at
    /// once.
    fn end(
        &mut self,
        doc: Option<&mut Doc>,
        which: impl Fn(&waiting::Caller) -> bool,
        why: waiting::Ended,
    ) {
        if let Some(doc) = doc {
            doc.mcp_end(which, why);
            self.sync(Some(doc));
        }
    }

    /// Sends the replies of the calls done in `doc`, the open design,
    /// marking their rows in the log; first ends those of a server
    /// stopped since they came (or of none, switched off).
    pub(crate) fn sync(&mut self, doc: Option<&mut Doc>) {
        self.sync_untold(doc, |_| false);
    }

    /// [`Mcp::sync`], telling the calls `untold` picks nothing of the
    /// user's changes: those cancelled, whose replies their clients
    /// ignore.
    fn sync_untold(&mut self, doc: Option<&mut Doc>, untold: impl Fn(&waiting::Caller) -> bool) {
        let Some(doc) = doc else {
            return;
        };
        let start = self.serving();
        doc.mcp_end(
            |from| Some(from.start) != start,
            waiting::Ended::ServerStopped,
        );
        for done in doc.mcp.finished() {
            self.log_answer(done.from.serial, &done.call, &done.reply, done.state);
            let session_open = self.open_session() == Some(&done.from.session);
            let cancelled = untold(&done.from);
            let (reply, told) = match cancelled {
                true => (done.reply, None),
                false => self.tell(doc, &done.from, done.reply),
            };
            let sent = done.from.reply.send(reply);
            // Not taken, the changes are left for the next reply.
            let told = told.filter(|_| sent);
            if told.is_some() {
                self.told(doc, &done.from);
            }
            let serial = done.from.serial;
            let Some(logged) = self.log.iter_mut().find(|logged| logged.serial == serial) else {
                continue;
            };
            if let Some(told) = told {
                logged.told = told.lines.iter().map(|line| plain(line)).collect();
                logged.told_rows = told.panel.iter().map(|line| plain(line)).collect();
                logged.told_at = Some(match &done.edit {
                    Some((before, _)) => before.revision(),
                    None => doc.editor.revision(),
                });
            }
            let not_told = !sent || !session_open || cancelled;
            if let Some(undid) = done.undid {
                // An undo's row says what it took back, and offers no
                // Revert: it made no state of its own.
                let (args, result) = undid_row(&undid, &done.call, logged.result.as_deref());
                logged.args = args;
                logged.result = Some(result);
                logged.edit = done.edit.map(|(_, after)| revert::Edited {
                    before: None,
                    after: None,
                    after_revision: after,
                    feature: undid.feature,
                    not_told,
                    undid: undid.afters,
                });
            } else if let Some((before, after)) = done.edit {
                logged.edit = Some(revert::Edited {
                    before: Some(before),
                    after: doc.editor.state_of(after),
                    after_revision: after,
                    feature: done.feature,
                    not_told,
                    undid: Vec::new(),
                });
                self.bound_snapshots();
            }
        }
        self.refresh_live(doc);
    }

    /// Asks the IO lane for the token at launch: the server waits for it,
    /// even while switched off, so switching it on starts it at once.
    pub(crate) fn boot(&self, io: &mut Io) {
        io.send(IoRequest::LoadMcpToken);
    }

    /// Asks the IO lane for a new token, which restarts the server and so
    /// ends the session.
    pub(crate) fn regenerate_token(&self, io: &mut Io) {
        io.send(IoRequest::RegenerateMcpToken);
    }

    /// The MCP panel's toggle and its head's Start and Stop, given the
    /// settings' switch `on` and `port`. Start starts the server, or,
    /// running and the port not bound, tries it again; neither touches
    /// the settings.
    pub(crate) fn panel_update(
        &mut self,
        message: McpPanel,
        on: bool,
        port: u16,
        doc: Option<&mut Doc>,
    ) {
        match message {
            McpPanel::Toggle => self.open = on && !self.open,
            McpPanel::Start => self.start(on, port),
            McpPanel::Stop => self.stop(on, port),
            McpPanel::ApplyYours => {
                if let Some(doc) = doc {
                    doc.mcp_apply_yours();
                }
            }
            McpPanel::KeepEditing => {
                if let Some(doc) = doc {
                    doc.mcp_keep_editing();
                }
            }
            McpPanel::OpenMenu(_)
            | McpPanel::CloseMenu
            | McpPanel::ShowInTimeline(_)
            | McpPanel::RevertToHere(_)
            | McpPanel::RevertBefore(_)
            | McpPanel::ConfirmRevert
            | McpPanel::CancelRevert => self.panel_message(message, doc),
        }
    }

    /// The tasks `doc` asks for after a message: the Timeline scrolled to
    /// the feature the panel showed, and the pictures `render` calls
    /// asked for, drawn by the viewport's next frame (see
    /// `mcp/query/render.rs`).
    pub(crate) fn doc_tasks(&self, doc: &mut Doc) -> Vec<Task<Message>> {
        let id = doc.id;
        let scroll = doc
            .mcp
            .scroll
            .take()
            .map(|share| crate::tasks::snap_list(varde_view::TIMELINE_LIST, share));
        let renders = doc.mcp.take_renders().into_iter().map(|(tag, answer)| {
            Task::perform(answer, move |image| {
                let image = image.ok().flatten().and_then(|image| {
                    varde_io::thumbnail::Image::new(image.width, image.height, image.rgba)
                });
                Message::Doc(id, ForDoc::McpRendered(tag, image))
            })
        });
        scroll.into_iter().chain(renders).collect()
    }
}

// The limits `varde_mcp` mirrors by value, as it depends on no other
// `varde-*` crate, held to their sources here, which sees both. The
// fractions are whole percents on this side, checked in the tests.
mod mirrored {
    use varde_mcp::tools as mcp;

    const _: () = assert!(mcp::NAME_BYTES == varde_document::MAX_NAME_LEN);
    const _: () = assert!(mcp::MAX_UNDO_STEPS == varde_document::MAX_UNDO);
    const _: () = assert!(mcp::TEXT_BYTES == varde_expr::MAX_LEN);
    const _: () = assert!(mcp::MAX_PATTERN_COUNT == varde_document::MAX_PATTERN_COUNT);
    const _: () = assert!(
        mcp::MAX_ITEMS == varde_document::MAX_EXTRUDE_REGIONS
            && mcp::MAX_ITEMS == varde_document::MAX_REVOLVE_REGIONS
            && mcp::MAX_ITEMS == varde_document::MAX_FEATURE_BODIES
            && mcp::MAX_ITEMS == varde_document::MAX_SHELL_FACES
            && mcp::MAX_ITEMS == varde_document::MAX_OFFSET_FACES
            && mcp::MAX_ITEMS == varde_document::MAX_DRAFT_FACES
            && mcp::MAX_ITEMS == varde_document::MAX_SPLIT_CURVES
    );
    const _: () = assert!(mcp::SPLINE_POINTS == varde_sketch::MAX_SPLINE_POINTS);
    const _: () = assert!(
        mcp::MIN_SIDES == varde_view::typed::MIN_SIDES
            && mcp::MAX_SIDES == varde_view::typed::MAX_SIDES
    );
}

#[cfg(test)]
mod tests;

/// The running program's path as text, for the settings' lines starting
/// it as `varde mcp`: looked up once, see [`relay_exe`].
fn exe() -> Option<&'static str> {
    static EXE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    EXE.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        let exists = exe.is_file();
        relay_exe(
            exe,
            exists,
            std::env::var_os("APPIMAGE").map(std::path::PathBuf::from),
            std::env::var_os("FLATPAK_ID").is_some(),
        )
    })
    .as_deref()
}

/// The path a client can start `varde mcp` from later, given the running
/// program's `exe` and whether it's still a file there: none if it's
/// gone (replaced while running, Linux's `current_exe` then ends in
/// ` (deleted)`), or the program runs in a Flatpak sandbox, whose path
/// means nothing outside it; an AppImage's own file (`APPIMAGE`) rather
/// than its mount, which goes when it exits; none if it isn't text.
fn relay_exe(
    exe: std::path::PathBuf,
    exists: bool,
    appimage: Option<std::path::PathBuf>,
    flatpak: bool,
) -> Option<String> {
    if flatpak {
        return None;
    }
    let exe = match appimage {
        Some(appimage) if appimage.is_absolute() => appimage,
        _ if exists => exe,
        _ => return None,
    };
    let exe = exe.into_os_string().into_string().ok()?;
    (!exe.ends_with(" (deleted)")).then_some(exe)
}
