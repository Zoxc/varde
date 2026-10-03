//! Browser storage as the page sees it, on the web: whether the browser
//! keeps it for good, and how much of it is used. Browser storage can be
//! cleared by the user, or by the browser itself when it runs short of
//! space, unless the site's storage is persistent, which only the page can
//! ask for (`navigator.storage.persist()`): the app asks as the user first
//! saves a design there, and the welcome screen says when it's refused.
//! Natively there's no browser storage: these answer `None`.

/// How much browser storage the site uses, and may, in bytes, as the
/// browser estimates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    pub used: u64,
    pub quota: u64,
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::Space;

    pub async fn persist() -> Option<bool> {
        None
    }

    pub async fn persisted() -> Option<bool> {
        None
    }

    pub async fn estimate() -> Option<Space> {
        None
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use js_sys::{Promise, Reflect};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::StorageManager;

    use super::Space;
    use crate::js::size;

    fn manager() -> Option<StorageManager> {
        Some(web_sys::window()?.navigator().storage())
    }

    async fn ask(promise: Result<Promise, wasm_bindgen::JsValue>) -> Option<wasm_bindgen::JsValue> {
        JsFuture::from(promise.ok()?).await.ok()
    }

    /// Asks the browser to keep the site's storage for good, which it may
    /// ask the user about: whether it does.
    pub async fn persist() -> Option<bool> {
        ask(manager()?.persist()).await?.as_bool()
    }

    /// Whether the browser keeps the site's storage for good.
    pub async fn persisted() -> Option<bool> {
        ask(manager()?.persisted()).await?.as_bool()
    }

    /// How much browser storage the site uses, and may.
    pub async fn estimate() -> Option<Space> {
        let estimate = ask(manager()?.estimate()).await?;
        let number = |key: &str| {
            let value = Reflect::get(&estimate, &key.into()).ok()?.as_f64()?;
            size(value).ok()
        };
        Some(Space {
            used: number("usage")?,
            quota: number("quota")?,
        })
    }
}

pub use imp::{estimate, persist, persisted};
