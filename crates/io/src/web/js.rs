//! Waiting for JS promises, and JS exceptions as IO errors, see the parent
//! module.

use std::io;

use js_sys::{Array, ArrayBuffer, Function, Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::DomException;

use crate::web_support::numbers::error_kind;

/// Waits for `promise`.
pub(crate) async fn call(promise: Promise) -> io::Result<JsValue> {
    JsFuture::from(promise).await.map_err(js_error)
}

/// `object.name`, if the browser has it as a method: for what not every
/// browser has, looked for rather than called blindly.
pub(crate) fn method(object: &JsValue, name: &str) -> Option<Function> {
    Reflect::get(object, &JsValue::from_str(name))
        .ok()?
        .dyn_into()
        .ok()
}

/// Calls `method` on `object` with `args`, for the promise it returns:
/// checked to be one, as the browser says what it returns.
pub(crate) fn call_method(
    object: &JsValue,
    method: &Function,
    args: &Array,
) -> io::Result<Promise> {
    method
        .apply(object, args)
        .map_err(js_error)?
        .dyn_into()
        .map_err(|_| io::Error::other("the browser answered with something other than a promise"))
}

/// A JS exception as an IO error.
pub(crate) fn js_error(error: JsValue) -> io::Error {
    if let Some(exception) = error.dyn_ref::<DomException>() {
        return io::Error::new(error_kind(&exception.name()), exception.message());
    }
    if let Some(error) = error.dyn_ref::<js_sys::Error>() {
        let name = String::from(error.name());
        return io::Error::new(error_kind(&name), String::from(error.message()));
    }
    io::Error::other(format!("{error:?}"))
}

/// The bytes of `buffer` into `buf`, refused unless the browser made it
/// exactly `buf.len()` long: the length is the browser's to say, twice,
/// and the two must agree.
pub(crate) fn copy_exact(buffer: &ArrayBuffer, buf: &mut [u8]) -> io::Result<()> {
    let bytes = Uint8Array::new(buffer);
    if usize::try_from(bytes.length()).ok() != Some(buf.len()) {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the file changed while it was read",
        ));
    }
    bytes.copy_to(buf);
    Ok(())
}
