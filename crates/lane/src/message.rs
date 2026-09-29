//! How a message crosses between the page and a worker, both ways: one JS
//! array of the message's parts as `ArrayBuffer`s, transferred, not
//! copied, followed by any JS objects posted along with them, cloned. The
//! lane's own control messages are other shapes, see [`crate::page`].

use js_sys::{Array, ArrayBuffer, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

/// A message the other side posted: its parts, to copy out with
/// [`bytes::copy`](crate::bytes::copy), and the objects posted along with
/// them.
#[derive(Debug)]
pub struct Message {
    pub parts: Vec<Uint8Array>,
    pub objects: Vec<JsValue>,
}

impl Message {
    /// The message `data` holds, or `None` if it isn't one.
    pub(crate) fn from_js(data: &JsValue) -> Option<Self> {
        let mut items = data.dyn_ref::<Array>()?.iter().peekable();
        let mut parts = Vec::new();
        while let Some(part) = items.next_if(JsValue::is_instance_of::<ArrayBuffer>) {
            parts.push(Uint8Array::new(&part));
        }
        let objects = items.collect();
        Some(Self { parts, objects })
    }
}

/// Why a lane refused a message the other side posted. The lane words it,
/// naming the worker, see `Refused::word`, so each lane only says what
/// was wrong. Any error converts to [`Refused::Broken`], so `?` refuses
/// what doesn't decode.
#[derive(Debug)]
pub enum Refused {
    /// Its shape isn't one the lane takes: the wrong number of parts, or an
    /// object it doesn't expect.
    Else,
    /// It has the lane's shape but its bytes don't make sense, for this
    /// reason.
    Broken(String),
}

impl<E: std::error::Error> From<E> for Refused {
    fn from(error: E) -> Self {
        Self::Broken(error.to_string())
    }
}

impl Refused {
    /// Says that `name`, e.g. "the file worker", `did` this, e.g. "sent" or
    /// "got": "the file worker sent something broken: …".
    pub(crate) fn word(&self, name: &str, did: &str) -> String {
        match self {
            Refused::Else => format!("{name} {did} something else"),
            Refused::Broken(error) => format!("{name} {did} something broken: {error}"),
        }
    }
}

/// The array to post for `parts` and `objects`, and the buffers in it to
/// transfer.
pub(crate) fn frame(parts: &[&[u8]], objects: &[JsValue]) -> (Array, Array) {
    let buffers: Array = parts
        .iter()
        .map(|part| Uint8Array::from(*part).buffer())
        .collect();
    let message = buffers.concat(&objects.iter().collect());
    (message, buffers)
}
