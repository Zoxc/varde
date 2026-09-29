//! Listening for the page going away, in the browser.

use std::pin::Pin;
use std::task::{Context, Poll};

use iced::Subscription;
use iced::futures::Stream;
use iced::futures::channel::mpsc::{UnboundedReceiver, unbounded};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{BeforeUnloadEvent, Event, VisibilityState};

use crate::Message;

/// Yields [`Message::PageLeaving`] as the page is hidden or goes away, for
/// as long as the app runs.
pub(crate) fn leaving() -> Subscription<Message> {
    fn listen() -> Listener {
        Listener::new(&["visibilitychange", "pagehide"], |event| {
            event.type_() != "visibilitychange"
                || web_sys::window()
                    .and_then(|window| window.document())
                    .is_some_and(|document| document.visibility_state() == VisibilityState::Hidden)
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
        Listener::new(&["beforeunload"], |event| {
            event.prevent_default();
            // Older browsers ask only if it's set.
            if let Some(event) = event.dyn_ref::<BeforeUnloadEvent>() {
                event.set_return_value("unsaved");
            }
            true
        })
    }
    Subscription::run(listen)
}

/// Listens to `events` on the window for as long as it's kept, yielding
/// [`Message::PageLeaving`] for each that `leaving` says may be the page
/// going away.
struct Listener {
    events: &'static [&'static str],
    receiver: UnboundedReceiver<Message>,
    on_event: Closure<dyn FnMut(Event)>,
}

impl Listener {
    fn new(events: &'static [&'static str], leaving: fn(&Event) -> bool) -> Self {
        let (sender, receiver) = unbounded();
        let on_event = Closure::new(move |event: Event| {
            if leaving(&event) {
                let _ = sender.unbounded_send(Message::PageLeaving);
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
