//! The app's side of the MCP server (`varde-mcp`, see `notes/AgentSupport.md`):
//! the server's subscription, its state on `Varde`, and the tool calls
//! answered from the open design. Native only.
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
mod undo;
pub(crate) mod waiting;

use std::collections::VecDeque;
use std::net::SocketAddr;

use iced::Subscription;
use varde_mcp::guard::Token;
use varde_mcp::server::Event;
use varde_mcp::session::{self, SessionId};
use varde_mcp::tools::{
    Axis, Call, Direction, ItemKind, LinkFrom, LinkKind, Profiles, Reference, SplitWith,
    SweepAlong, Target, ToolReply, Worded, World,
};
use varde_view::{McpCallState, McpTool};

use crate::doc::Doc;
use crate::message::Message;
use waiting::Undid;

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
    /// The session open, its client's name and when it started, if one
    /// is.
    session: Option<(SessionId, Option<String>, varde_io::UnixSeconds)>,
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
            if let Some((session, ..)) = self.session.clone() {
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
    /// `port`, at `now`.
    pub(crate) fn panel<'a>(
        &'a self,
        on: bool,
        port: u16,
        now: varde_io::UnixSeconds,
        doc: Option<&'a Doc>,
    ) -> varde_view::McpShown<'a> {
        let client = (self.session.as_ref())
            .map(|(_, name, _)| name.as_deref())
            .or(self.gone.as_ref().map(Option::as_deref))
            .flatten();
        varde_view::McpShown {
            ask: (doc.and_then(|doc| doc.mcp.ask.as_ref())).map(|ask| ask.shown(client)),
            on,
            open: self.open,
            status: self.shown(now).status,
            port,
            client: (self.session.as_ref()).map(|(_, name, _)| name.as_deref()),
            gone: self.gone.as_ref().map(Option::as_deref),
            rows: self.rows(now, doc),
            dropped: self.dropped,
            revert: (doc.and_then(|doc| doc.mcp.revert_ask.as_ref())).map(|ask| {
                varde_view::McpRevertAsk {
                    to: &ask.to,
                    differences: &ask.differences,
                }
            }),
        }
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
            let edit = (logged.edit.as_ref()).map(|edit| {
                let offered = |state: &Option<varde_document::EditorState>| {
                    state.as_ref().is_some_and(|state| {
                        doc.is_some_and(|doc| state.lineage() == doc.editor.lineage())
                    })
                };
                varde_view::McpEdit {
                    serial: logged.serial,
                    // An undo is taken back when the user redoes what it
                    // undid.
                    reverted: doc.is_some_and(|doc| match edit.undid.is_empty() {
                        true => Mcp::taken_back(doc, Some(edit.after_revision)),
                        false => (edit.undid.iter()).any(|after| doc.in_effect(*after)),
                    }),
                    undo: !edit.undid.is_empty(),
                    not_told: edit.not_told,
                    menu: self.menu == Some(logged.serial),
                    timeline: edit.feature.is_some(),
                    here: offered(&edit.after)
                        && doc.is_some_and(|doc| {
                            doc.mcp.amended_to(edit.after_revision) != doc.editor.revision()
                                && !edit
                                    .after
                                    .as_ref()
                                    .is_some_and(|after| doc.shows_state(after))
                        }),
                    before: offered(&edit.before),
                }
            });
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
        let serial = self.serials;
        self.serials = self.serials.wrapping_add(1);
        while self.log.len() >= LOG_CAP {
            self.log.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.log.push_back(Logged {
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
        });
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
        from: &waiting::From,
        reply: ToolReply,
    ) -> (ToolReply, Option<changes::Changes>) {
        let open = self.session.as_ref().map(|(session, ..)| session);
        if open != Some(&from.session) {
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
    fn told(&mut self, doc: &mut Doc, from: &waiting::From) {
        for step in &mut doc.mcp.steps {
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
        let (true, Some((session, ..)), Some(seen), Some(start)) = (
            self.open,
            &self.session,
            &self.seen,
            self.serve.as_ref().map(|serve| serve.start),
        ) else {
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
            client: (self.session.as_ref()).map(|(_, name, since)| McpClient {
                name: name.as_deref(),
                since: crate::when::ago_in_sentence(*since, now),
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
        if self.serve.as_ref().map(|serve| serve.start) != Some(start) {
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
                let from = waiting::From {
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
                self.session = Some((session, client, crate::when::now()));
            }
            Event::Session(session::Event::Ended { session, end: _ }) => {
                self.session_ended(&session, doc);
            }
            // Its client ignores the reply: the user's changes aren't
            // told in it, but left for the next.
            Event::Session(session::Event::Cancelled { session, request }) => {
                let which = |from: &waiting::From| from.session == session && from.id == request;
                if let Some(doc) = doc {
                    doc.mcp_end(which, waiting::Ended::Cancelled);
                    self.sync_untold(Some(doc), which);
                }
            }
        }
    }

    /// The session `session` ended: its client shown gone if it's the
    /// one open, its calls waiting in `doc` answered so.
    fn session_ended(&mut self, session: &SessionId, doc: Option<&mut Doc>) {
        if self
            .session
            .as_ref()
            .is_some_and(|(open, ..)| open == session)
            && let Some((_, name, _)) = self.session.take()
        {
            self.gone = Some(name);
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
        which: impl Fn(&waiting::From) -> bool,
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
    fn sync_untold(&mut self, doc: Option<&mut Doc>, untold: impl Fn(&waiting::From) -> bool) {
        let Some(doc) = doc else {
            return;
        };
        let start = self.serve.as_ref().map(|serve| serve.start);
        doc.mcp_end(
            |from| Some(from.start) != start,
            waiting::Ended::ServerStopped,
        );
        for done in doc.mcp.finished() {
            self.log_answer(done.from.serial, &done.call, &done.reply, done.state);
            let feature =
                (done.reply.structured.as_ref()).and_then(|data| data.get("id")?.as_u64());
            let open = self.session.as_ref().map(|(session, ..)| session);
            let session_open = open == Some(&done.from.session);
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
                    feature,
                    not_told,
                    undid: Vec::new(),
                });
                self.bound_snapshots();
            }
        }
        self.refresh_live(doc);
    }
}

/// The answer to `call` from `doc`, the open design, for the calls
/// answered at once; `body_topology`, `sketch`, `measure` and `render` wait
/// (`waiting.rs`), so they're only checked here.
pub(crate) fn answer_now(doc: &Doc, call: &Call) -> ToolReply {
    match call {
        Call::DesignSummary(page) => query::design_summary(doc, *page),
        Call::Feature { name, page } => query::feature(doc, name, *page),
        Call::BodyTopology(topology) => match query::body_topology(doc, topology) {
            Ok(_) => ToolReply::error("internal error: body_topology waits"),
            Err(reply) => reply,
        },
        Call::Sketch(args) => match query::sketch(doc, args) {
            Ok(_) => ToolReply::error("internal error: sketch waits"),
            Err(reply) => reply,
        },
        Call::Measure(args) => match query::measure(doc, args) {
            Ok(_) => ToolReply::error("internal error: measure waits"),
            Err(reply) => reply,
        },
        Call::Render(args) => match query::render(doc, args) {
            Ok(_) => ToolReply::error("internal error: render waits"),
            Err(reply) => reply,
        },
        _ => ToolReply::error("internal error: an edit is applied, not answered"),
    }
}

/// The panel's rows of the user's own edits, `lines`, at most
/// [`changes::LINES`], the rest counted in one more.
fn user_rows(lines: &[String], reverted: bool) -> impl Iterator<Item = varde_view::McpRow<'_>> {
    fn row(said: std::borrow::Cow<'_, str>, reverted: bool) -> varde_view::McpRow<'_> {
        match reverted {
            true => varde_view::McpRow::UserReverted(said),
            false => varde_view::McpRow::User(said),
        }
    }
    let more = lines.len().saturating_sub(changes::LINES);
    (lines.iter().take(changes::LINES))
        .map(move |line| row(line.as_str().into(), reverted))
        .chain((more > 0).then(|| {
            row(
                match more {
                    1 => "1 more change".into(),
                    n => format!("{n} more changes").into(),
                },
                reverted,
            )
        }))
}

/// An undo's row once applied, from what it took back, `undid`: its
/// arguments ("renamed feature Extrude 1 to Base", or "3 edits") and its
/// tooltip, listing them newest first after `summary`, the result's own
/// (its failures).
fn undid_row(undid: &Undid, call: &Call, summary: Option<&str>) -> (String, String) {
    let lowered = |shown: &str| -> String {
        let mut chars = shown.chars();
        (chars.next().into_iter())
            .flat_map(char::to_lowercase)
            .chain(chars)
            .collect()
    };
    let args = match (call, undid.shown.as_slice()) {
        (Call::Undo { steps: 1 }, [one]) => lowered(one),
        _ => counted(undid.shown.len(), "edit", "edits"),
    };
    let mut said = format!("Undid: {}", undid.shown.join("; "));
    // The result's failures, after "applied".
    if let Some(rest) = summary.and_then(|summary| summary.split_once(" · ")) {
        said.push_str(&format!(" · {}", rest.1));
    }
    (plain(&args), clipped(plain(&said), RESULT_BYTES))
}

/// The line under a call whose result told the client of the user's
/// changes, `lines`: "Told: since its last call, you renamed ...", cut
/// to [`RESULT_BYTES`].
fn told_line(lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let said = format!("Told: since its last call, you {}", lines.join(", "));
    Some(clipped(said, RESULT_BYTES))
}

/// The tool `call` is of, as the panel shows it.
fn tool(call: &Call) -> McpTool {
    match call {
        Call::DesignSummary(_) => McpTool::DesignSummary,
        Call::Feature { .. } => McpTool::Feature,
        Call::BodyTopology(_) => McpTool::BodyTopology,
        Call::Sketch(_) => McpTool::Sketch,
        Call::Measure(_) => McpTool::Measure,
        Call::Render(_) => McpTool::Render,
        Call::Rename(_) => McpTool::Rename,
        Call::SetVisible { .. } => McpTool::SetVisible,
        Call::SetColor { .. } => McpTool::SetColor,
        Call::SetOpacity { .. } => McpTool::SetOpacity,
        Call::RemoveFeature { .. } => McpTool::RemoveFeature,
        Call::RemoveBody { .. } => McpTool::RemoveBody,
        Call::AddParameter { .. } => McpTool::AddParameter,
        Call::SetParameter { .. } => McpTool::SetParameter,
        Call::RenameParameter { .. } => McpTool::RenameParameter,
        Call::RemoveParameter { .. } => McpTool::RemoveParameter,
        Call::SetUnits(_) => McpTool::SetUnits,
        Call::SetTolerance { .. } => McpTool::SetTolerance,
        Call::SetRollback { .. } => McpTool::SetRollback,
        Call::AddSketch { .. } => McpTool::AddSketch,
        Call::SetSketchPlane { .. } => McpTool::SetSketchPlane,
        Call::AddLink(_) => McpTool::AddLink,
        Call::EditSketch(_) => McpTool::EditSketch,
        Call::SetFeature(_) => McpTool::SetFeature,
        Call::Extrude(_) => McpTool::Extrude,
        Call::Revolve(_) => McpTool::Revolve,
        Call::Combine(_) => McpTool::Combine,
        Call::MoveBody(_) => McpTool::MoveBody,
        Call::Mirror(_) => McpTool::Mirror,
        Call::Pattern(_) => McpTool::Pattern,
        Call::Align(_) => McpTool::Align,
        Call::Scale(_) => McpTool::Scale,
        Call::Split(_) => McpTool::Split,
        Call::Shell(_) => McpTool::Shell,
        Call::OffsetFace(_) => McpTool::OffsetFace,
        Call::Draft(_) => McpTool::Draft,
        Call::Chamfer(_) => McpTool::Chamfer,
        Call::Fillet(_) => McpTool::Fillet,
        Call::Sweep(_) => McpTool::Sweep,
        Call::Loft(_) => McpTool::Loft,
        Call::Undo { .. } => McpTool::Undo,
    }
}

/// `call`'s arguments as the panel shows them, as typed: names as given,
/// filters and pages as the client set them, what's left at its default
/// unsaid.
fn args(call: &Call) -> String {
    let page = |offset: usize, limit: usize, default: usize| {
        let mut said = Vec::new();
        if offset != 0 {
            said.push(format!("from {offset}"));
        }
        if limit != default {
            said.push(format!("{limit} at most"));
        }
        said
    };
    let said: Vec<String> = match call {
        Call::DesignSummary(at) => page(at.offset, at.limit, varde_mcp::tools::SUMMARY_LIMIT),
        Call::Feature { name, .. } => vec![name.clone()],
        Call::BodyTopology(topology) => {
            let mut said = vec![topology.body.clone()];
            if let Some(normal) = topology.faces.and_then(|faces| faces.normal) {
                said.push(format!("faces facing {}", direction(normal)));
            }
            let edges = topology.edges.unwrap_or_default();
            match edges.convex {
                Some(true) => said.push("convex edges".to_owned()),
                Some(false) => said.push("concave edges".to_owned()),
                None => {}
            }
            if let Some(along) = edges.direction {
                said.push(format!("edges along {}", direction(along)));
            }
            said.extend(page(
                topology.offset,
                topology.limit,
                varde_mcp::tools::LIMIT,
            ));
            said
        }
        Call::Sketch(args) => {
            let mut said = vec![args.name.clone()];
            said.extend(page(
                args.page.offset,
                args.page.limit,
                varde_mcp::tools::SKETCH_LIMIT,
            ));
            said
        }
        Call::Measure(args) => {
            let mut said = vec![measured(&args.a)];
            if let Some(b) = &args.b {
                said.push(format!("to {}", measured(b)));
            }
            said
        }
        Call::Render(args) => {
            let mut said = match args.bodies.is_empty() {
                true => vec!["the model".to_owned()],
                false => vec![args.bodies.join(", ")],
            };
            said.push(match &args.from {
                varde_mcp::tools::RenderFrom::View(view) => {
                    format!("from {}", format!("{view:?}").to_lowercase())
                }
                varde_mcp::tools::RenderFrom::Angles { yaw, pitch } => {
                    format!("from yaw {yaw}, pitch {pitch}")
                }
            });
            if args.perspective {
                said.push("in perspective".to_owned());
            }
            if args.zoom != 1.0 {
                said.push(format!("zoomed {}", args.zoom));
            }
            if !args.highlight.is_empty() {
                said.push(format!(
                    "highlighting {}",
                    counted(args.highlight.len(), "item", "items")
                ));
            }
            said
        }
        Call::Rename(rename) => {
            let (what, name) = match &rename.target {
                Target::Feature(name) => ("feature", name),
                Target::Body(name) => ("body", name),
            };
            vec![format!("{what} {name} to {}", rename.to)]
        }
        Call::SetVisible { body, visible } => {
            vec![format!(
                "{body} {}",
                if *visible { "shown" } else { "hidden" }
            )]
        }
        Call::SetColor { body, color } => vec![match color {
            Some(color) => format!(
                "{body} to hue {}°, saturation {}",
                varde_expr::format_number(color.hue, None),
                varde_expr::format_number(color.saturation, None)
            ),
            None => format!("{body} to the theme's"),
        }],
        Call::SetOpacity { body, opacity } => vec![format!(
            "{body} to {}",
            varde_expr::format_number(*opacity, None)
        )],
        Call::RemoveFeature { feature: name }
        | Call::RemoveBody { body: name }
        | Call::RemoveParameter { name } => vec![name.clone()],
        Call::AddParameter { name, value } | Call::SetParameter { name, value } => {
            vec![format!("{name} = {value}")]
        }
        Call::RenameParameter { name, to } => vec![format!("{name} to {to}")],
        Call::SetUnits(units) => vec![units.word().to_owned()],
        Call::SetTolerance { tolerance } => vec![tolerance.clone()],
        Call::SetRollback { before } => vec![match before {
            Some(name) => format!("before {name}"),
            None => "to the end".to_owned(),
        }],
        Call::AddSketch { plane, .. } => vec![reference(plane)],
        Call::SetSketchPlane { sketch, plane } => vec![format!("{sketch} to {}", reference(plane))],
        Call::AddLink(link) => {
            let from = match &link.from {
                LinkFrom::Item(item) => match item.kind {
                    ItemKind::Edge => "an edge".to_owned(),
                    kind => format!("a {}", kind.word()),
                },
                LinkFrom::Curve(curve) => format!("{}'s curve {}", curve.sketch, curve.curve),
                LinkFrom::Point { sketch, point } => format!("{sketch}'s point {point}"),
            };
            let kind = match link.kind {
                LinkKind::Project => "projected",
                LinkKind::Intersect => "intersected",
            };
            vec![format!("{} {from} {kind}", link.sketch)]
        }
        // The sketch and what's done in it, by the operations' names.
        Call::EditSketch(edit) => {
            let ops: Vec<&str> = (edit.operations.iter().take(8))
                .map(|operation| operation.op.word())
                .collect();
            let more = edit.operations.len().saturating_sub(ops.len());
            let mut said = format!("{}: {}", edit.sketch, ops.join(", "));
            if more > 0 {
                said.push_str(&format!(" and {more} more"));
            }
            vec![said]
        }
        // The fields changed, by name: their values may be keys.
        Call::SetFeature(set) => {
            let fields: Vec<&str> = set.patch.keys().map(String::as_str).collect();
            vec![format!("{}: {}", set.name, fields.join(", "))]
        }
        Call::Extrude(extrude) => {
            let mut said = vec![extrude.sketch.clone(), profiles(&extrude.profiles)];
            said.extend(extrude.distance.clone());
            said
        }
        Call::Revolve(revolve) => {
            let mut said = vec![revolve.sketch.clone(), profiles(&revolve.profiles)];
            said.push(match &revolve.axis {
                Axis::SketchX => "about x".to_owned(),
                Axis::SketchY => "about y".to_owned(),
                Axis::Line(id) => format!("about line {id}"),
                Axis::Edge(_) => "about an edge".to_owned(),
            });
            said.extend(revolve.angle.clone());
            said
        }
        Call::Combine(combine) => {
            vec![format!(
                "{} with {}",
                combine.target,
                combine.tools.join(", ")
            )]
        }
        Call::MoveBody(moved) => {
            let mut said = vec![moved.bodies.join(", ")];
            let offset: Vec<&str> = (moved.offset.iter())
                .map(|offset| offset.as_deref().unwrap_or("0"))
                .collect();
            if moved.offset.iter().any(Option::is_some) {
                said.push(format!("by {}", offset.join(", ")));
            }
            said.extend(moved.angle.clone());
            said
        }
        Call::Mirror(mirror) => vec![format!(
            "{} across {}",
            mirror.bodies.join(", "),
            reference(&mirror.plane)
        )],
        Call::Pattern(pattern) => {
            let mut said = vec![pattern.bodies.join(", ")];
            said.extend(pattern.count.as_ref().map(|count| format!("×{count}")));
            said.extend(pattern.spread.clone());
            said
        }
        Call::Align(align) => vec![format!(
            "{} to {}",
            reference(&align.from.point),
            reference(&align.to.point)
        )],
        Call::Scale(scale) => {
            let mut said = vec![scale.bodies.join(", ")];
            said.extend(scale.factor.clone());
            said.extend(scale.length.clone());
            said
        }
        Call::Split(split) => {
            let tool = match &split.tool {
                SplitWith::Surface(surface) => reference(surface),
                SplitWith::Body(body) => body.clone(),
                SplitWith::Regions {
                    sketch,
                    profiles: picked,
                } => {
                    format!("{sketch} {}", profiles(picked))
                }
                SplitWith::Line { sketch, curves } => {
                    format!(
                        "{sketch}'s {} curve{}",
                        curves.len(),
                        if curves.len() == 1 { "" } else { "s" }
                    )
                }
            };
            vec![format!("{} by {tool}", split.body)]
        }
        Call::Shell(shell) => {
            let mut said: Vec<String> = shell.body.iter().cloned().collect();
            said.push(faces(shell.faces.len()));
            said.extend(shell.thickness.clone());
            said
        }
        Call::OffsetFace(offset) => {
            let mut said = vec![faces(offset.faces.len())];
            said.extend(offset.distance.clone());
            said
        }
        Call::Draft(draft) => {
            let mut said = vec![faces(draft.faces.len())];
            said.extend(
                draft
                    .neutral
                    .as_ref()
                    .map(|neutral| format!("from {}", reference(neutral))),
            );
            said.extend(draft.angle.clone());
            said
        }
        Call::Chamfer(chamfer) => {
            let mut said = blend(&chamfer.edges, &chamfer.faces);
            said.extend(chamfer.distance.clone());
            said
        }
        Call::Fillet(fillet) => {
            let mut said = blend(&fillet.edges, &fillet.faces);
            said.extend(fillet.radius.clone());
            said
        }
        Call::Sweep(sweep) => {
            let along = match sweep.along {
                SweepAlong::Path { .. } => "a path",
                SweepAlong::Helix { .. } => "a helix",
            };
            vec![format!(
                "{} {} along {along}",
                sweep.sketch,
                profiles(&sweep.profiles)
            )]
        }
        Call::Loft(loft) => {
            let mut said = vec![counted(loft.sections.len(), "section", "sections")];
            if !loft.rails.is_empty() {
                said.push(counted(loft.rails.len(), "rail", "rails"));
            }
            said
        }
        // What it's to take back; once it has, its row says what it did
        // (`undid_row`).
        Call::Undo { steps: 1 } => vec!["its last edit".to_owned()],
        Call::Undo { steps } => vec![format!("its last {steps} edits")],
    };
    let mut said = said;
    if let Some(bodies) = (call.naming().map(|naming| &naming.bodies)).filter(|b| !b.is_empty()) {
        said.push(format!("making {}", bodies.join(", ")));
    }
    // What it's to be named, first: "Base: Sketch 1, all, 20 mm".
    match call.new_name() {
        Some(name) => format!("{name}: {}", said.join(", ")),
        None => said.join(", "),
    }
}

/// What a motion tool refers to, for its row: the world's by its word, a
/// key by what it names (its text means nothing to the user).
fn reference<W: World>(reference: &Reference<W>) -> String {
    match reference {
        Reference::World(world) => match world.word() {
            "origin" => "the origin".to_owned(),
            word => word.to_owned(),
        },
        Reference::Key(item) => match item.kind {
            ItemKind::Edge => "an edge".to_owned(),
            kind => format!("a {}", kind.word()),
        },
    }
}

/// A face tool's faces, for its row: how many (their keys mean nothing
/// to the user).
fn faces(count: usize) -> String {
    counted(count, "face", "faces")
}

/// A blend's edges and faces, for its row: how many of each it names.
fn blend(edges: &[String], faces: &[String]) -> Vec<String> {
    let mut said = Vec::new();
    if !edges.is_empty() {
        said.push(counted(edges.len(), "edge", "edges"));
    }
    if !faces.is_empty() {
        said.push(self::faces(faces.len()));
    }
    said
}

/// "1 face", "3 faces": `count` of `one`, or of `many` if not one.
fn counted(count: usize, one: &str, many: &str) -> String {
    match count {
        1 => format!("1 {one}"),
        count => format!("{count} {many}"),
    }
}

/// A feature tool's profiles, for its row.
fn profiles(profiles: &Profiles) -> String {
    match profiles {
        Profiles::All => "every region".to_owned(),
        Profiles::Picked(items) if items.len() == 1 => "1 region".to_owned(),
        Profiles::Picked(items) => format!("{} regions", items.len()),
    }
}

/// What `measure` measures, as its row says it: a body's name, or what
/// a key names (not the key, which says nothing to the user).
fn measured(item: &varde_mcp::tools::Measured) -> String {
    match item {
        varde_mcp::tools::Measured::Body(name) => name.clone(),
        varde_mcp::tools::Measured::Item(item) => match item.kind {
            varde_mcp::tools::ItemKind::Face => "a face".to_owned(),
            varde_mcp::tools::ItemKind::Edge => "an edge".to_owned(),
            varde_mcp::tools::ItemKind::Corner => "a corner".to_owned(),
        },
    }
}

/// `direction` as a client would type it: an axis (`+z`, or `z` either
/// way), else its three numbers.
fn direction(direction: Direction) -> String {
    let [x, y, z] = direction.vector;
    let zeros = [x, y, z].iter().filter(|value| **value == 0.0).count();
    for (axis, value) in [("x", x), ("y", y), ("z", z)] {
        if value.abs() == 1.0 && zeros == 2 {
            return match (direction.signed, value > 0.0) {
                (false, _) => axis.to_owned(),
                (true, true) => format!("+{axis}"),
                (true, false) => format!("-{axis}"),
            };
        }
    }
    let number = |value: f64| varde_expr::format_number(value, None);
    format!("{}, {}, {}", number(x), number(y), number(z))
}

/// What came of `call`, summed up for its row's tooltip: what the answer
/// lists, or why it failed, cut to [`RESULT_BYTES`].
fn summary(call: &Call, reply: &ToolReply) -> String {
    let total = |listing: &str| {
        let data = reply.structured.as_ref()?;
        data.get(listing)?.get("total")?.as_u64()
    };
    let counted = |listings: &[(&str, &str, &str)]| -> Option<String> {
        let said: Option<Vec<String>> = (listings.iter())
            .map(|(listing, one, many)| {
                total(listing).map(|n| match n {
                    1 => format!("1 {one}"),
                    n => format!("{n} {many}"),
                })
            })
            .collect();
        said.map(|said| said.join(" · "))
    };
    let said = match call {
        _ if reply.is_error => None,
        Call::DesignSummary(_) => counted(&[
            ("bodies", "body", "bodies"),
            ("timeline", "feature", "features"),
            ("parameters", "parameter", "parameters"),
        ]),
        Call::Feature { .. } => (reply.structured.as_ref()).and_then(|data| {
            let kind = data.get("kind")?.as_str()?;
            Some(
                match data.get("failure").and_then(|failure| failure.as_str()) {
                    Some(failure) => format!("{kind} · fails: {failure}"),
                    None => kind.to_owned(),
                },
            )
        }),
        Call::BodyTopology(_) => counted(&[
            ("faces", "face", "faces"),
            ("edges", "edge", "edges"),
            ("corners", "corner", "corners"),
        ]),
        Call::Sketch(_) => counted(&[
            ("points", "point", "points"),
            ("curves", "curve", "curves"),
            ("regions", "region", "regions"),
        ])
        .map(|said| {
            let free = (reply.structured.as_ref())
                .and_then(|data| data.get("degrees_of_freedom")?.as_u64());
            match free {
                Some(0) => format!("{said} · fully constrained"),
                Some(1) => format!("{said} · 1 degree of freedom"),
                Some(n) => format!("{said} · {n} degrees of freedom"),
                None => said,
            }
        }),
        Call::Measure(_) => (reply.structured.as_ref()).and_then(|data| {
            let first = |of: &str| -> Option<String> {
                let values = data.get(of)?.get("values")?.as_object()?;
                let (label, value) = values.iter().next()?;
                Some(format!("{}: {}", label.replace('_', " "), value.as_str()?))
            };
            let between = data.get("between").and_then(|between| {
                let said = |key: &str| between.get(key).and_then(serde_json::Value::as_str);
                match (said("distance"), said("angle")) {
                    (Some(d), Some(a)) => Some(format!("distance {d} · angle {a}")),
                    (Some(d), None) => Some(format!("distance {d}")),
                    (None, Some(a)) => Some(format!("angle {a}")),
                    (None, None) => None,
                }
            });
            between.or_else(|| first("a"))
        }),
        Call::Render(_) => (reply.structured.as_ref()).and_then(|data| {
            let size = data.get("size")?.as_array()?;
            let side = |i: usize| size.get(i)?.as_u64();
            Some(format!("{} × {} PNG", side(0)?, side(1)?))
        }),
        _ => edit::summary(reply),
    };
    clipped(
        plain(&said.unwrap_or_else(|| reply.text.clone())),
        RESULT_BYTES,
    )
}

/// `text` cut to at most `bytes` (on a character boundary), with an
/// ellipsis where it was cut.
fn clipped(text: String, bytes: usize) -> String {
    let kept = varde_mcp::cut(&text, bytes);
    match kept.len() < text.len() {
        true => format!("{kept}…"),
        false => text,
    }
}

/// `text`, which may come off the network, made safe to show on one
/// line: line breaks, tabs and other whitespace controls as spaces,
/// other controls and the bidi marks, embeddings, overrides and isolates
/// (which would turn the text around them) as U+FFFD.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            c if c.is_control() && c.is_whitespace() => ' ',
            '\u{061C}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}' => '\u{FFFD}',
            c if c.is_control() => '\u{FFFD}',
            c => c,
        })
        .collect()
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
