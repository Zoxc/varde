//! The native build's side: the lane's thread (`thread`), what it does
//! with each request (`files`), the lock files (`sidecar`), the store of
//! new designs, the recent files list and the settings at their paths
//! (`store`, `recent`, `settings`, the last two through `config`),
//! exported files (`export`), the paths both take (`path`), the panic
//! recorded (`panicked`), and the platform's file dialogs (`pick`).

mod atomic;
mod config;
mod export;
pub(crate) mod files;
pub(crate) mod mcp_token;
pub(crate) mod panicked;
mod path;
pub(crate) mod pick;
mod recent;
pub(crate) mod settings;
mod sidecar;
mod store;
pub(crate) mod thread;
mod unique;

use crate::Stores;

/// The platform's directories for the app, named `varde-cad` after it on
/// every OS, e.g. `~/.config/varde-cad` and `~/.local/share/varde-cad` on
/// Linux.
pub(crate) fn project_dirs() -> Option<directories::ProjectDirs> {
    let name = varde_document::APP_NAME.to_lowercase().replace(' ', "-");
    directories::ProjectDirs::from("", "", &name)
}

/// The user's stores, see [`Stores::user`]: the recent files list, the
/// settings and the MCP token in the config directory, new designs and
/// the panic recorded in the data directory.
pub(crate) fn user_stores() -> Stores {
    let dirs = project_dirs();
    let config = |name: &str| dirs.as_ref().map(|d| d.config_dir().join(name));
    let data = |name: &str| dirs.as_ref().map(|d| d.data_dir().join(name));
    Stores {
        recent: config("recent.toml"),
        settings: config("settings.toml"),
        mcp_token: config("mcp-token.toml"),
        designs: data("designs"),
        panic: data("panic.toml"),
    }
}
