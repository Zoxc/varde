//! The server itself, native only: a listener thread on `127.0.0.1` and
//! a thread per connection, the app reached through a stream of
//! [`Event`]s that [`start`] returns. Not a `varde-lane` lane: a lane's
//! thread waits on the app's requests and can't also wait on a socket.
//!
//! The port is bound before the stream says anything, so a port in use
//! comes back as [`Event::BindFailed`], never another port. Nothing sets
//! address reuse; std's `TcpListener::bind` sets `SO_REUSEADDR` itself on
//! Unix (not on Windows, where it would let another socket take the
//! port). On Linux and the BSDs that only allows binding past a closed
//! connection's `TIME_WAIT`: a port another socket listens on still
//! fails. `SO_REUSEPORT`, which would share it, is never set.
//!
//! The listener is nonblocking, polled every [`POLL`]. The stream's drop
//! sets a stop flag and closes the listener itself, so the port is free
//! once the drop returns and a server started on it at once can bind it;
//! the listener thread ends on its next poll. Each poll also ends the session if it has idled
//! past [`session::IDLE`]. Connections are bounded at [`CONNECTIONS`];
//! one past it is closed unanswered, as a busy server's backlog would
//! refuse it. Each request's head must come within [`HEAD_TIMEOUT`] of
//! its first byte, and a connection's first byte within it of the
//! accept; only a connection one of whose requests has passed the guard
//! may wait up to [`READ_TIMEOUT`] for its next. The head is read with
//! [`http::read_request_head`] and checked by the guard before the body
//! is read; a refused request is answered and its connection closed,
//! its body never read. The body must come within [`READ_TIMEOUT`] of
//! the head's end and the response be taken within [`WRITE_TIMEOUT`]:
//! deadlines rather than per-read timeouts, so a peer trickling bytes,
//! token or not, can't keep one of the [`CONNECTIONS`] for long. A
//! connection is served until it closes, a read fails or a response
//! goes with `Connection: close`; one already open when the server
//! stops ends after its next request.
//!
//! The session's state is shared by the connection threads, so
//! `initialize`, `tools/list` and `ping` never reach the app; a
//! `tools/call` goes to it as [`Event::Call`] with a [`Reply`], waited on
//! for at most [`REPLY`]. Calls on different connections can be in flight
//! at once: keeping edits to one at a time is the app's to do.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::thread;
use std::time::{Duration, Instant};

use futures::Stream;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::guard::{Guard, Token};
use crate::http::{self, Response, write_response};
use crate::rpc::Id;
use crate::session::{self, Answer, Dispatch, SessionId, State};
use crate::tools::{Call, ToolReply};

/// How often the listener looks for a connection, the stop flag and an
/// idle session.
pub const POLL: Duration = Duration::from_millis(50);

/// How many connections are served at once.
pub const CONNECTIONS: usize = 8;

/// How long a connection waits for its next request, whole (the wait
/// for its first byte too), and for a response to be taken.
pub const READ_TIMEOUT: Duration = Duration::from_secs(60);
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a request's head may take to come, from its first byte (for
/// a connection's first request, from its accepting): a peer without
/// the token can't hold a connection longer than this.
pub const HEAD_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a call waits for the app's reply: a backstop behind the
/// app's own 30 s.
pub const REPLY: Duration = Duration::from_secs(35);

/// The answer to a call whose [`Reply`] was dropped unanswered.
pub const CLOSING: &str = "Varde is closing";

/// The answer to a call the app didn't answer within [`REPLY`].
pub const TIMED_OUT: &str = "The call timed out in Varde";

/// What the server tells the app.
#[derive(Clone, Debug)]
pub enum Event {
    /// The first event: bound and listening.
    Ready(Handle),
    /// The first and last event: the port couldn't be bound, such as
    /// one in use, or the listener's thread couldn't start.
    BindFailed(String),
    /// A tool call to run and answer through `reply`.
    Call {
        session: SessionId,
        id: Id,
        call: Call,
        reply: Reply,
    },
    /// The session started, ended, or had a call cancelled.
    Session(session::Event),
}

/// Where a call's answer goes. Cloneable, as the app's messages are;
/// the first [`Reply::send`] is taken, and every copy dropped unanswered
/// answers [`CLOSING`].
#[derive(Clone, Debug)]
pub struct Reply {
    sender: SyncSender<ToolReply>,
    /// When the call's request had come whole: what the app's bound on
    /// its wait runs from, the time it queued for the app included.
    came: Instant,
}

impl Reply {
    /// A reply into `sender` for a call come at `came`, for the app's
    /// tests, which feed it calls of their own.
    #[cfg(feature = "testing")]
    pub fn for_tests(sender: SyncSender<ToolReply>, came: Instant) -> Self {
        Self { sender, came }
    }

    /// When the call's request had come whole to the server, before it
    /// queued for the app.
    pub fn came(&self) -> Instant {
        self.came
    }

    /// Answers the call; nothing if it was answered already or its
    /// connection has given up on it. Whether the answer was taken.
    pub fn send(&self, reply: ToolReply) -> bool {
        self.sender.try_send(reply).is_ok()
    }
}

/// The running server, for the app.
#[derive(Clone)]
pub struct Handle {
    address: SocketAddr,
    shared: Arc<Shared>,
}

impl Handle {
    /// The address bound: the port chosen, if 0 was asked for.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Ends the session, as Disconnect does: its event comes on the
    /// stream.
    pub fn end_session(&self) {
        let mut state = self.shared.state();
        if let Some(event) = state.end() {
            self.shared.send(Event::Session(event));
        }
    }
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

/// What the threads share.
struct Shared {
    state: Mutex<State>,
    events: UnboundedSender<Event>,
    guard: Guard,
    start: Instant,
    stop: AtomicBool,
    /// The listener, taken and closed by the stream's drop, so the port
    /// is free again the moment the drop returns.
    listener: Mutex<Option<TcpListener>>,
    timeouts: Timeouts,
}

impl Shared {
    /// The session state. A thread that panicked holding it left it as
    /// whole as any: each change is made at once.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    /// Sends `event`; nothing if the stream is gone.
    fn send(&self, event: Event) {
        let _ = self.events.unbounded_send(event);
    }

    /// The listener's lock. Each use leaves it whole.
    fn listener(&self) -> MutexGuard<'_, Option<TcpListener>> {
        self.listener.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// Starts the server on `127.0.0.1:port` (0 for any free port). The
/// stream's first event is [`Event::Ready`] or [`Event::BindFailed`];
/// dropping it stops the server.
pub fn start(port: u16, token: Token) -> impl Stream<Item = Event> + Send + 'static {
    start_with(port, token, Timeouts::default())
}

/// How long the server waits on a connection and on the app.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Timeouts {
    pub read: Duration,
    pub head: Duration,
    pub write: Duration,
    pub reply: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            read: READ_TIMEOUT,
            head: HEAD_TIMEOUT,
            write: WRITE_TIMEOUT,
            reply: REPLY,
        }
    }
}

/// [`start`] with `timeouts`.
pub(crate) fn start_with(port: u16, token: Token, timeouts: Timeouts) -> Events {
    let (sender, receiver) = unbounded();
    let listener = match bind(port) {
        Ok(bound) => bound,
        Err(error) => {
            let _ = sender.unbounded_send(Event::BindFailed(error.to_string()));
            return Events {
                receiver,
                shared: None,
            };
        }
    };
    let (listener, address) = listener;
    let shared = Arc::new(Shared {
        state: Mutex::new(State::new()),
        events: sender,
        guard: Guard {
            port: address.port(),
            token,
        },
        start: Instant::now(),
        stop: AtomicBool::new(false),
        listener: Mutex::new(Some(listener)),
        timeouts,
    });
    let listening = shared.clone();
    let spawned = thread::Builder::new()
        .name("mcp listener".into())
        .spawn(move || {
            // Sent from here, so a thread that can't start leaves
            // BindFailed the only event, and Ready still comes before
            // any connection's.
            listening.send(Event::Ready(Handle {
                address,
                shared: listening.clone(),
            }));
            listen(&listening);
        });
    if let Err(error) = spawned {
        shared.send(Event::BindFailed(error.to_string()));
    }
    Events {
        receiver,
        shared: Some(shared),
    }
}

/// The listener, nonblocking, and the address it's bound to.
fn bind(port: u16) -> io::Result<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    Ok((listener, address))
}

/// The server's events; dropping it stops the server.
pub struct Events {
    receiver: UnboundedReceiver<Event>,
    /// `None` if the bind failed.
    shared: Option<Arc<Shared>>,
}

impl Stream for Events {
    type Item = Event;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Event>> {
        Pin::new(&mut self.receiver).poll_next(cx)
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        if let Some(shared) = &self.shared {
            shared.stop.store(true, Ordering::Relaxed);
            // Closed here rather than on the listener thread's next
            // poll, so a server started on the same port right after
            // can bind it.
            drop(shared.listener().take());
        }
    }
}

/// Counts a connection open until dropped.
struct Open(Arc<AtomicUsize>);

impl Drop for Open {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Accepts connections until stopped, ending an idle session now and
/// then. Ends when the stream's drop has taken the listener.
fn listen(shared: &Arc<Shared>) {
    let open = Arc::new(AtomicUsize::new(0));
    while !shared.stopped() {
        let accepted = match shared.listener().as_ref() {
            Some(listener) => listener.accept(),
            None => return,
        };
        match accepted {
            Ok((stream, _)) => {
                // Only this thread adds, so the count can't pass the
                // bound between the check and the add.
                if open.load(Ordering::Relaxed) >= CONNECTIONS {
                    drop(stream);
                    continue;
                }
                open.fetch_add(1, Ordering::Relaxed);
                let counted = Open(open.clone());
                let shared = shared.clone();
                // A thread that can't be spawned drops the connection,
                // and its count with it.
                let _ = thread::Builder::new()
                    .name("mcp connection".into())
                    .spawn(move || {
                        let _counted = counted;
                        serve(stream, &shared);
                    });
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                expire(shared);
                thread::sleep(POLL);
            }
            // Such as a connection reset before it was accepted.
            Err(_) => thread::sleep(POLL),
        }
    }
}

fn expire(shared: &Shared) {
    let mut state = shared.state();
    if let Some(event) = state.expire(shared.now()) {
        shared.send(Event::Session(event));
    }
}

/// A connection's stream with a deadline: each read or write waits
/// only for what's left of it, so a peer trickling a byte at a time
/// can't hold the connection past it.
struct Timed {
    stream: TcpStream,
    until: Instant,
}

impl Timed {
    /// The deadline `timeout` from now; one past `Instant`'s range is
    /// as good as none.
    fn start(&mut self, timeout: Duration) {
        let now = Instant::now();
        self.until = now.checked_add(timeout).unwrap_or(now);
    }

    /// The deadline brought in to `timeout` from now, if that's sooner.
    fn cap(&mut self, timeout: Duration) {
        let now = Instant::now();
        if let Some(until) = now.checked_add(timeout) {
            self.until = self.until.min(until);
        }
    }

    /// What's left of the deadline, `Err` if it's passed.
    fn left(&self) -> io::Result<Duration> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(left)
    }
}

impl Read for Timed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.left()?))?;
        self.stream.read(buf)
    }
}

impl Write for Timed {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.left()?))?;
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

/// Serves one connection's requests, one after the other, within the
/// deadlines the module docs give.
fn serve(stream: TcpStream, shared: &Shared) {
    let ready = stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_nodelay(true));
    let Ok(read) = ready.and_then(|()| stream.try_clone()) else {
        return;
    };
    let now = Instant::now();
    let mut reader = BufReader::new(Timed {
        stream: read,
        until: now,
    });
    let mut writer = Timed { stream, until: now };
    let limits = http::Limits::default();
    let timeouts = shared.timeouts;
    // Whether a request on it has passed the guard: only then may the
    // connection idle up to the read timeout between requests.
    let mut trusted = false;
    loop {
        reader.get_mut().start(if trusted {
            timeouts.read
        } else {
            timeouts.head
        });
        // The wait for the first byte, then the head from it.
        match reader.fill_buf() {
            Ok([]) | Err(_) => return,
            Ok(_) => {}
        }
        reader.get_mut().cap(timeouts.head);
        let head = http::read_request_head(&mut reader, &limits);
        let (mut request, length) = match head {
            Ok(Some(head)) => head,
            Ok(None) => return,
            Err(error) => {
                if let Some(status) = error.status() {
                    writer.start(timeouts.write);
                    let _ = write_response(&mut writer, &Response::empty(status), true);
                }
                return;
            }
        };
        // Checked before the body is read: a refused request's body is
        // never read, and the connection closes after the answer.
        if let Err(refusal) = shared.guard.check(&request) {
            writer.start(timeouts.write);
            let _ = write_response(&mut writer, &refusal.response(), true);
            return;
        }
        trusted = true;
        // The body within the read timeout, from the head's end.
        reader.get_mut().start(timeouts.read);
        if http::read_body(&mut reader, &mut request, length).is_err() {
            return;
        }
        // What the app's bound on a call's wait runs from.
        let came = Instant::now();
        let pending = {
            let mut state = shared.state();
            let outcome = state.handle(&request, shared.now());
            // Sent under the lock, so events reach the app in
            // the order they happened: a call before any cancel
            // or end of its session.
            for event in outcome.events {
                shared.send(Event::Session(event));
            }
            match outcome.answer {
                Answer::Respond(response) => Pending::Now(response),
                Answer::Dispatch(dispatch) => {
                    Pending::Waiting(dispatch_call(shared, dispatch, came))
                }
            }
        };
        let response = match pending {
            Pending::Now(response) => response,
            Pending::Waiting(waiting) => waiting.wait(),
        };
        let close = request.close || shared.stopped();
        writer.start(timeouts.write);
        if write_response(&mut writer, &response, close).is_err() || close {
            return;
        }
    }
}

/// What a request is answered with: a response now, or the app's reply
/// to a call once it comes, waited for outside the state's lock.
enum Pending<'a> {
    Now(Response),
    Waiting(Waiting<'a>),
}

/// A call handed to the app, the request `id` of `session`: dropping it
/// finishes the call in the state ([`State::finish`]), on every path, so
/// the session isn't kept from idling out by a call no one waits for.
struct InFlight<'a> {
    shared: &'a Shared,
    session: SessionId,
    id: Id,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let now = self.shared.now();
        self.shared.state().finish(&self.session, &self.id, now);
    }
}

/// Hands `dispatch`, of a request come at `came`, to the app, under the
/// state's lock: what it returns waits for the reply.
fn dispatch_call<'a>(shared: &'a Shared, dispatch: Dispatch, came: Instant) -> Waiting<'a> {
    let Dispatch { session, id, call } = dispatch;
    let (sender, receiver) = mpsc::sync_channel(1);
    shared.send(Event::Call {
        session: session.clone(),
        id: id.clone(),
        call,
        reply: Reply { sender, came },
    });
    // Made last, as nothing after it here can fail: dropped under the
    // state's lock, it would wait on that lock for good.
    Waiting {
        call: InFlight {
            shared,
            session,
            id,
        },
        receiver,
    }
}

/// A call handed to the app, and where its reply comes.
struct Waiting<'a> {
    call: InFlight<'a>,
    receiver: Receiver<ToolReply>,
}

impl Waiting<'_> {
    /// Waits for the app's reply to the call.
    fn wait(self) -> Response {
        let Waiting { call, receiver } = self;
        let shared = call.shared;
        // With the stream gone, the event (and the reply in it) is dropped
        // at once: the wait ends as disconnected.
        let reply = match receiver.recv_timeout(shared.timeouts.reply) {
            Ok(reply) => reply,
            Err(RecvTimeoutError::Disconnected) => ToolReply::error(CLOSING),
            // One sent as the wait ran out was taken ([`Reply::send`] said
            // so): it's the answer. Then the receiver goes at once, before
            // the state's lock, so a later send isn't taken.
            Err(RecvTimeoutError::Timeout) => match receiver.try_recv() {
                Ok(reply) => reply,
                Err(_) => ToolReply::error(TIMED_OUT),
            },
        };
        drop(receiver);
        let response = session::respond(&call.id, &reply);
        drop(call);
        response
    }
}

#[cfg(test)]
mod tests;
