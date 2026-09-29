//! Files of the user's on the page, see `src/pick.rs`: the pickers,
//! the file input, asking to write and downloading. Never in the IO worker,
//! which has no `window`; what it does with them is
//! `src/web/worker/disk.rs`'s.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;

use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use varde_document::name::download_name;
use varde_document::{APP_NAME, EXTENSION};
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Blob, BlobPropertyBag, FileSystemFileHandle, HtmlAnchorElement, HtmlInputElement, Url, console,
};

use crate::pick::{Download, filter};
use crate::web::js::{call, js_error};
use crate::{Chosen, Picked, PickedFrom};

// What web-sys has only among its unstable APIs.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = window, js_name = showOpenFilePicker)]
    fn show_open_file_picker(options: &Object) -> Result<Promise, JsValue>;

    #[wasm_bindgen(catch, js_namespace = window, js_name = showSaveFilePicker)]
    fn show_save_file_picker(options: &Object) -> Result<Promise, JsValue>;

    /// A handle, with the permission methods.
    #[wasm_bindgen(extends = FileSystemFileHandle)]
    type Permissions;

    #[wasm_bindgen(catch, method, js_name = queryPermission)]
    fn query_permission(this: &Permissions, descriptor: &Object) -> Result<Promise, JsValue>;

    #[wasm_bindgen(catch, method, js_name = requestPermission)]
    fn request_permission(this: &Permissions, descriptor: &Object) -> Result<Promise, JsValue>;
}

/// What the pickers handed over, by [`Picked::id`]. Handles are kept for
/// the session, to ask to write to them; each is a few bytes, and one per
/// file opened or saved as.
#[derive(Default)]
struct Registry {
    next: u64,
    objects: HashMap<u64, JsValue>,
}

thread_local! {
    static PICKED: RefCell<Registry> = RefCell::default();
}

fn register(object: JsValue, name: String, from: PickedFrom) -> Picked {
    PICKED.with_borrow_mut(|registry| {
        let id = registry.next;
        // A u64 counted up once per pick never overflows.
        registry.next += 1;
        registry.objects.insert(id, object);
        Picked { id, name, from }
    })
}

/// What the page holds for `picked`, to post to the worker along with the
/// request using it. A file from a file input is read once, so it's let
/// go of.
pub(crate) fn object(picked: &Picked) -> Option<JsValue> {
    PICKED.with_borrow_mut(|registry| match picked.from {
        PickedFrom::Handle => registry.objects.get(&picked.id).cloned(),
        PickedFrom::Input => registry.objects.remove(&picked.id),
    })
}

/// Whether the browser has the File System Access API's pickers, so the
/// user's files can be written back to: Chromium's. Elsewhere designs are
/// downloaded.
fn file_system_access() -> bool {
    let window = js_sys::global();
    ["showOpenFilePicker", "showSaveFilePicker"]
        .iter()
        .all(|name| Reflect::has(&window, &JsValue::from_str(name)).unwrap_or(false))
}

/// How saving hands the design over, if it downloads it: without the File
/// System Access API.
pub fn downloader() -> Option<Download> {
    (!file_system_access()).then_some(download as Download)
}

/// Asks the user for a design to open: the File System Access picker if
/// there is one, otherwise a file input. `None` if they backed out, or it
/// failed, which the console says.
pub async fn pick_open() -> Option<Chosen> {
    let picked = if file_system_access() {
        pick_handle().await
    } else {
        pick_input().await
    };
    logged(picked, "Couldn't pick a file:")
}

async fn pick_handle() -> Result<Option<Picked>, JsValue> {
    let Some(handles) = shown(show_open_file_picker(&picker_options(None)?)?).await? else {
        return Ok(None);
    };
    register_handle(Array::from(&handles).get(0)).map(Some)
}

async fn pick_input() -> Result<Option<Picked>, JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or("no document")?;
    let input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
    input.set_type("file");
    input.set_accept(&format!(".{EXTENSION}"));
    // Answered with a change once picked, or a cancel if not.
    let done = Promise::new(&mut |resolve, _| {
        for event in ["change", "cancel"] {
            let _ = input.add_event_listener_with_callback(event, &resolve);
        }
    });
    input.click();
    JsFuture::from(done).await?;
    let file = input.files().and_then(|files| files.get(0));
    Ok(file.map(|file| {
        let name = file.name();
        register(file.into(), name, PickedFrom::Input)
    }))
}

/// Asks the user where to save the design `name` in the File System
/// Access picker. `None` if they backed out, or it failed, which the
/// console says.
pub async fn pick_save(name: &str) -> Option<Chosen> {
    let pick = async {
        let options = picker_options(Some(&download_name(name)))?;
        match shown(show_save_file_picker(&options)?).await? {
            Some(handle) => register_handle(handle).map(Some),
            None => Ok(None),
        }
    };
    logged(pick.await, "Couldn't pick where to save:")
}

/// What a picker handed over, a failure logged to the console after
/// `what` and taken as backing out.
fn logged(picked: Result<Option<Picked>, JsValue>, what: &str) -> Option<Chosen> {
    picked
        .unwrap_or_else(|error| {
            console::error_2(&what.into(), &error);
            None
        })
        .map(Chosen::File)
}

/// The pickers' options: `.vrdp` files, suggesting `name` to save as.
fn picker_options(name: Option<&str>) -> Result<Object, JsValue> {
    let accept = Object::new();
    Reflect::set(
        &accept,
        &"application/octet-stream".into(),
        &Array::of1(&format!(".{EXTENSION}").into()),
    )?;
    let kind = Object::new();
    Reflect::set(&kind, &"description".into(), &filter().into())?;
    Reflect::set(&kind, &"accept".into(), &accept)?;
    let options = Object::new();
    Reflect::set(&options, &"types".into(), &Array::of1(&kind))?;
    if let Some(name) = name {
        Reflect::set(&options, &"suggestedName".into(), &name.into())?;
    }
    Ok(options)
}

/// What the picker `showing` hands over, `None` if the user closed it.
async fn shown(showing: Promise) -> Result<Option<JsValue>, JsValue> {
    match JsFuture::from(showing).await {
        Ok(picked) => Ok(Some(picked)),
        Err(error)
            if error
                .dyn_ref::<web_sys::DomException>()
                .is_some_and(|error| error.name() == "AbortError") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// Keeps the handle a picker handed over.
fn register_handle(handle: JsValue) -> Result<Picked, JsValue> {
    let handle: FileSystemFileHandle = handle.dyn_into()?;
    let name = handle.name();
    Ok(register(handle.into(), name, PickedFrom::Handle))
}

/// Makes sure the page may write to `picked`, asking the user if it hasn't
/// been allowed to yet. Only a page can ask, and only while the user has
/// just done something, so this runs as they save.
pub async fn writable(picked: &Picked) -> Result<(), String> {
    let handle = (picked.from == PickedFrom::Handle)
        .then(|| object(picked))
        .flatten()
        .and_then(|object| object.dyn_into::<Permissions>().ok())
        .ok_or_else(|| format!("{} can't be written to", picked.name))?;
    let state = |promise: Result<Promise, JsValue>| async {
        let state = call(promise.map_err(js_error)?).await?;
        Ok::<_, io::Error>(state.as_string().unwrap_or_default())
    };
    let granted = async {
        let descriptor = Object::new();
        Reflect::set(&descriptor, &"mode".into(), &"readwrite".into()).map_err(js_error)?;
        Ok::<_, io::Error>(match state(handle.query_permission(&descriptor)).await {
            Ok(state) if state == "granted" => true,
            Ok(_) => state(handle.request_permission(&descriptor)).await? == "granted",
            // Browsers that don't ask: writing says whether it may.
            Err(_) => true,
        })
    };
    let granted = granted.await.map_err(|e| e.to_string())?;
    if granted {
        Ok(())
    } else {
        Err(format!(
            "{APP_NAME} wasn't allowed to save to {}",
            picked.name
        ))
    }
}

/// Downloads `bytes` as the file `name`.
fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    download_blob(name, bytes).map_err(|e| format!("couldn't download {name}: {e}"))
}

fn download_blob(name: &str, bytes: &[u8]) -> io::Result<()> {
    let missing = |what: &str| io::Error::other(format!("no {what}"));
    let window = web_sys::window().ok_or_else(|| missing("window"))?;
    let document = window.document().ok_or_else(|| missing("document"))?;
    let body = document.body().ok_or_else(|| missing("body"))?;
    let options = BlobPropertyBag::new();
    options.set_type("application/octet-stream");
    let blob = Blob::new_with_u8_array_sequence_and_options(
        &Array::of1(&Uint8Array::from(bytes)),
        &options,
    )
    .map_err(js_error)?;
    let url = Url::create_object_url_with_blob(&blob).map_err(js_error)?;
    let anchor: HtmlAnchorElement = document
        .create_element("a")
        .and_then(|element| element.dyn_into().map_err(JsValue::from))
        .map_err(js_error)?;
    anchor.set_href(&url);
    anchor.set_download(name);
    body.append_child(&anchor).map_err(js_error)?;
    anchor.click();
    anchor.remove();
    // The browser reads it after the click: let go of it once it surely
    // has.
    let revoke = Closure::once_into_js(move || {
        let _ = Url::revoke_object_url(&url);
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 60_000)
        .map_err(js_error)?;
    Ok(())
}
