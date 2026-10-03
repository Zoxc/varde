//! The panic recorded on the web, in the page's `localStorage`, see
//! `src/panicked.rs`: the page answers the lane's requests about it
//! itself, as the worker has no `localStorage`.

use crate::panicked::{MAX_BYTES, Panic};
use crate::{Request, Response};

/// The `localStorage` key it's kept under.
const KEY: &str = "varde-panic";

/// The page's `localStorage`, if it has one it may use.
fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// Records `panic`, from the panic hook: failing is only logged.
pub(crate) fn record(panic: &Panic) {
    let stored = storage().map(|storage| storage.set_item(KEY, &panic.serialize()));
    if !matches!(stored, Some(Ok(()))) {
        web_sys::console::error_1(&"Couldn't record the panic".into());
    }
}

/// The panic recorded, if there's one that can be read.
fn load() -> Option<Panic> {
    let toml = storage()?.get_item(KEY).ok().flatten()?;
    let fits = u64::try_from(toml.len()).is_ok_and(|len| len <= MAX_BYTES);
    fits.then(|| Panic::parse(&toml)).flatten()
}

/// Deletes the panic recorded if it's `panic`, rather than one recorded
/// since, in another tab.
fn discard(panic: &Panic) -> Result<(), String> {
    if load().as_ref() != Some(panic) {
        return Ok(());
    }
    let storage = storage().ok_or("the browser keeps no local storage")?;
    storage
        .remove_item(KEY)
        .map_err(|e| format!("the browser refused: {e:?}"))
}

/// The answer to `request`, if it's about the panic recorded; otherwise
/// `request` back, for the worker.
pub(crate) fn answer(request: Request) -> Result<Response, Request> {
    match request {
        Request::LoadPanic => Ok(Response::PanicLoaded { panic: load() }),
        Request::DiscardPanic { panic } => Ok(Response::PanicDiscarded {
            result: discard(&panic),
        }),
        request => Err(request),
    }
}
