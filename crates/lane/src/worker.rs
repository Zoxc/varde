//! The worker's side of a lane on the web, run by the Web Worker's `main`:
//! listening to the page and posting back to it.
//!
//! A worker is a wasm instance of its own, built from a binary of the
//! lane's crate so it holds that lane's side only, not iced and wgpu.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent, console};

use crate::message;
pub use crate::message::{Message, Refused};

fn scope() -> DedicatedWorkerGlobalScope {
    js_sys::global().unchecked_into()
}

/// Runs the worker's side of a lane, called `name` in errors, e.g. "the
/// file worker": hands each message the page posts to `on_message`, for as
/// long as the worker lives, and tells the page it listens. Called once, by
/// the worker's `main`. Anything the page posts that isn't a [`Message`],
/// or that `on_message` refuses, throws, worded "… got something else" or
/// "… got something broken: …", which stops the worker.
///
/// A panic traps the wasm instance, so it's told to the page first, which
/// then answers what the worker owed rather than wait for it: it's logged,
/// with its location, and its message posted as a string, worded as the
/// native lanes word it (see [`crate::panic`]).
pub fn serve(
    name: &'static str,
    mut on_message: impl FnMut(Message) -> Result<(), Refused> + 'static,
) {
    std::panic::set_hook(Box::new(|info| {
        console::error_1(&info.to_string().into());
        let message = JsValue::from_str(&crate::panic::message(info.payload()));
        // Nothing more can be done if it doesn't go.
        let _ = scope().post_message(&message);
    }));
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let handled = Message::from_js(&event.data()).ok_or(Refused::Else);
        if let Err(refused) = handled.and_then(&mut on_message) {
            wasm_bindgen::throw_str(&refused.word(name, "got"));
        }
    });
    scope().set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    // Lives as long as the worker.
    on_message.forget();
    // Messages posted before a worker listens are lost, so the page waits
    // for this.
    if let Err(e) = scope().post_message(&JsValue::NULL) {
        wasm_bindgen::throw_val(e);
    }
}

/// Posts `parts` to the page, transferring them, see
/// [`Page::receive`](crate::page::Page::receive).
pub fn post(parts: &[&[u8]]) {
    let (message, transfer) = message::frame(parts, &[]);
    if let Err(e) = scope().post_message_with_transfer(&message, &transfer) {
        wasm_bindgen::throw_val(e);
    }
}
