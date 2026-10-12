//! The MCP server's bearer token, `mcp-token.toml` in the config
//! directory beside `settings.toml`: one key,
//!
//! ```toml
//! token = "..." # 43 characters of base64url, see varde_mcp::guard::Token
//! ```
//!
//! Made on first start, and anew on Regenerate. Written as the settings
//! are (a temporary file renamed over it), but created at mode 0600, so
//! only the user can read it. One that can't be trusted is made anew: a
//! symlink or anything but a plain file, and on Unix one not at 0600 or
//! not owned by the directory's owner (the user's own config directory:
//! std has no way to ask for the process's user), or one replaced between
//! looking at it and opening it (opened without following a symlink and,
//! so a FIFO can't block the IO lane, without blocking). So is one
//! that doesn't hold a token.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use varde_mcp::guard::Token;

use super::config;

/// The most bytes of the file read: a larger one is taken as gone bad.
const MAX_BYTES: u64 = 1024;

/// Where the token is stored, if there's a config directory to put it in.
pub(crate) fn store() -> Option<PathBuf> {
    super::project_dirs().map(|dirs| dirs.config_dir().join("mcp-token.toml"))
}

/// The token stored in `store`, if there's one to trust.
pub(crate) fn load(store: &Path) -> Option<Token> {
    let looked = std::fs::symlink_metadata(store).ok()?;
    if !looked.file_type().is_file() {
        return None;
    }
    let file = open(store).ok()?;
    let opened = file.metadata().ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let owner = std::fs::metadata(store.parent()?).ok()?.uid();
        let same = looked.dev() == opened.dev() && looked.ino() == opened.ino();
        if !same || opened.mode() & 0o777 != 0o600 || opened.uid() != owner {
            return None;
        }
    }
    if !opened.is_file() {
        return None;
    }
    parse(&config::read_from(file, MAX_BYTES)?)
}

/// Opens `store` to read without blocking on a FIFO or following a
/// symlink swapped in since it was looked at: on Unix with `O_NONBLOCK`
/// and `O_NOFOLLOW`, so the IO lane can't hang on it. What's opened is
/// checked to be a plain file before it's read.
fn open(store: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(flags::NONBLOCK | flags::NOFOLLOW);
    }
    options.open(store)
}

/// `O_NONBLOCK` and `O_NOFOLLOW`, which std doesn't name and libc isn't
/// a dependency for: their values per target, from the system headers.
/// A target not listed gets 0 for both, leaving the device and inode
/// check `load` makes, which still catches a swap but not a FIFO's
/// blocking open.
#[cfg(unix)]
mod flags {
    #[cfg(all(
        target_os = "linux",
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "riscv64",
            target_arch = "loongarch64",
            target_arch = "powerpc",
            target_arch = "powerpc64",
            target_arch = "s390x",
        )
    ))]
    pub(super) const NONBLOCK: i32 = 0o4000;
    #[cfg(all(
        target_os = "linux",
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "riscv64",
            target_arch = "loongarch64",
            target_arch = "s390x",
        )
    ))]
    pub(super) const NOFOLLOW: i32 = 0o400000;
    #[cfg(all(
        target_os = "linux",
        any(
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "powerpc",
            target_arch = "powerpc64",
        )
    ))]
    pub(super) const NOFOLLOW: i32 = 0o100000;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
    ))]
    pub(super) const NONBLOCK: i32 = 0x4;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
    ))]
    pub(super) const NOFOLLOW: i32 = 0x100;
    #[cfg(not(any(
        all(
            target_os = "linux",
            any(
                target_arch = "x86",
                target_arch = "x86_64",
                target_arch = "arm",
                target_arch = "aarch64",
                target_arch = "riscv64",
                target_arch = "loongarch64",
                target_arch = "powerpc",
                target_arch = "powerpc64",
                target_arch = "s390x",
            )
        ),
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
    )))]
    pub(super) const NONBLOCK: i32 = 0;
    #[cfg(not(any(
        all(
            target_os = "linux",
            any(
                target_arch = "x86",
                target_arch = "x86_64",
                target_arch = "arm",
                target_arch = "aarch64",
                target_arch = "riscv64",
                target_arch = "loongarch64",
                target_arch = "powerpc",
                target_arch = "powerpc64",
                target_arch = "s390x",
            )
        ),
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
    )))]
    pub(super) const NOFOLLOW: i32 = 0;
}

/// The token `toml` holds, if it holds one.
fn parse(toml: &str) -> Option<Token> {
    let table = toml.parse::<toml::Table>().ok()?;
    table.get("token")?.as_str()?.parse().ok()
}

/// Writes `token` to `store`, replacing what's there, readable by the
/// user only.
pub(crate) fn write(store: &Path, token: &Token) -> io::Result<()> {
    config::replace_private(store, &serialize(token))
}

fn serialize(token: &Token) -> String {
    // Base64url needs no escaping in a TOML string.
    format!(
        "# Varde's MCP server's bearer token: anyone with it can use the server.\ntoken = \"{token}\"\n"
    )
}

/// A new random token.
pub(crate) fn generate() -> Result<Token, String> {
    Token::generate().map_err(|error| format!("couldn't make a token: {error}"))
}

#[cfg(test)]
mod tests;
