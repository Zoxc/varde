//! Listening for the page going away, and for files dropped on it, in
//! the browser.

use std::pin::Pin;
use std::task::{Context, Poll};

use iced::Subscription;
use iced::futures::Stream;
use iced::futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{BeforeUnloadEvent, DataTransfer, DragEvent, Event, VisibilityState};

use crate::Message;

/// Yields [`Message::PageLeaving`] as the page is hidden or goes away, for
/// as long as the app runs.
pub(crate) fn leaving() -> Subscription<Message> {
    fn listen() -> Listener {
        Listener::new(&["visibilitychange", "pagehide"], |event, _| {
            let leaving = event.type_() != "visibilitychange"
                || web_sys::window()
                    .and_then(|window| window.document())
                    .is_some_and(|document| document.visibility_state() == VisibilityState::Hidden);
            leaving.then_some(Message::PageLeaving)
        })
    }
    Subscription::run(listen)
}

/// While subscribed, the browser asks the user before the page goes; also
/// yields [`Message::PageLeaving`] as it's about to, which the user may yet
/// call off. Only subscribed while there are changes to lose: a page
/// listening for `beforeunload` is kept out of the browser's back/forward
/// cache.
pub(crate) fn guard() -> Subscription<Message> {
    fn listen() -> Listener {
        Listener::new(&["beforeunload"], |event, _| {
            event.prevent_default();
            // Older browsers ask only if it's set.
            if let Some(event) = event.dyn_ref::<BeforeUnloadEvent>() {
                event.set_return_value("unsaved");
            }
            Some(Message::PageLeaving)
        })
    }
    Subscription::run(listen)
}

/// Files dragged over the page and dropped on it, always the app's rather
/// than the browser's, which would leave the page for them, whatever
/// screen shows: yields [`Message::FileDragged`] as one comes over the
/// page and goes off it, and [`Message::FileDropped`] with the first file
/// dropped, kept like one the Open picker handed over (see
/// `varde_io::pick::dropped`): its handle where the File System Access API
/// is, which the browser hands over a moment later, else the file, which
/// the app lets go of if it doesn't open it. Only what carries files is
/// taken; text dragged about is left to the browser.
///
/// Whether one is over the page is counted, by the elements it entered
/// and hasn't left: entering the next comes before leaving the last, and
/// WebKit says of neither where the drag came from or went. Each message
/// is sent once, as it changes, and a drag over the page with none
/// counted, say one in from before the page loaded, comes over it again.
pub(crate) fn drops() -> Subscription<Message> {
    fn listen() -> Listener {
        // The elements entered and not left, and whether the app was told
        // a file is over the page.
        let mut entered = 0u32;
        let mut over = false;
        Listener::new(
            &["dragenter", "dragover", "dragleave", "drop"],
            move |event, sender| {
                let event = event.dyn_ref::<DragEvent>()?;
                let transfer = event.data_transfer().filter(carries_files)?;
                let was = over;
                match event.type_().as_str() {
                    "dragenter" => {
                        event.prevent_default();
                        // At most one per element the page has.
                        entered = entered.saturating_add(1);
                        over = true;
                    }
                    // Taken, or the drop isn't the page's.
                    "dragover" => {
                        event.prevent_default();
                        transfer.set_drop_effect("copy");
                        over = true;
                    }
                    "dragleave" => {
                        entered = entered.saturating_sub(1);
                        over = entered > 0;
                    }
                    "drop" => {
                        event.prevent_default();
                        (entered, over) = (0, false);
                        let dropped = varde_io::pick::dropped(&transfer);
                        let sender = sender.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = sender.unbounded_send(Message::FileDropped(dropped.await));
                        });
                        return was.then_some(Message::FileDragged(false));
                    }
                    _ => {}
                }
                (over != was).then_some(Message::FileDragged(over))
            },
        )
    }
    Subscription::run(listen)
}

/// Whether `transfer` carries files, which the browser says while they're
/// dragged, before it lets the page have them.
fn carries_files(transfer: &DataTransfer) -> bool {
    transfer
        .types()
        .iter()
        .any(|kind| kind.as_string().as_deref() == Some("Files"))
}

/// Listens to `events` on the window for as long as it's kept, yielding
/// the message `heard` makes of each, if it makes one; `heard` may send
/// one later, too, through the sender it's handed.
struct Listener {
    events: &'static [&'static str],
    receiver: UnboundedReceiver<Message>,
    on_event: Closure<dyn FnMut(Event)>,
}

impl Listener {
    fn new(
        events: &'static [&'static str],
        mut heard: impl FnMut(&Event, &UnboundedSender<Message>) -> Option<Message> + 'static,
    ) -> Self {
        let (sender, receiver) = unbounded();
        let on_event = Closure::new(move |event: Event| {
            if let Some(message) = heard(&event, &sender) {
                let _ = sender.unbounded_send(message);
            }
        });
        if let Some(window) = web_sys::window() {
            for event in events {
                let _ = window
                    .add_event_listener_with_callback(event, on_event.as_ref().unchecked_ref());
            }
        }
        Self {
            events,
            receiver,
            on_event,
        }
    }
}

impl Stream for Listener {
    type Item = Message;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Message>> {
        Pin::new(&mut self.receiver).poll_next(cx)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if let Some(window) = web_sys::window() {
            for event in self.events {
                let _ = window.remove_event_listener_with_callback(
                    event,
                    self.on_event.as_ref().unchecked_ref(),
                );
            }
        }
    }
}
