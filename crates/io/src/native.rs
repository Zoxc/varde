//! The native build's side: the lane's thread (`thread`), what it does
//! with each request (`files`), the lock files (`sidecar`), the store of
//! new designs, the recent files list and the settings at their paths
//! (`store`, `recent`, `settings`, the last two through `config`),
//! exported files (`export`), and the platform's file dialogs (`pick`).

mod config;
mod export;
pub(crate) mod files;
pub(crate) mod pick;
mod recent;
mod settings;
mod sidecar;
mod store;
pub(crate) mod thread;
mod unique;

/// The platform's directories for the app, named `varde-cad` after it on
/// every OS, e.g. `~/.config/varde-cad` and `~/.local/share/varde-cad` on
/// Linux.
pub(crate) fn project_dirs() -> Option<directories::ProjectDirs> {
    let name = varde_document::APP_NAME.to_lowercase().replace(' ', "-");
    directories::ProjectDirs::from("", "", &name)
}
