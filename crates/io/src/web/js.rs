//! Waiting for JS promises, and JS exceptions as IO errors, see the parent
//! module.

use std::io;

use js_sys::Promise;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::DomException;

use crate::js::error_kind;

/// Waits for `promise`.
pub(crate) async fn call(promise: Promise) -> io::Result<JsValue> {
    JsFuture::from(promise).await.map_err(js_error)
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
