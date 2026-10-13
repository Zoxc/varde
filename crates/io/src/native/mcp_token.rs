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

/// The MCP token, kept in its store, or in memory without one.
#[derive(Debug, Default)]
pub(crate) struct Tokens {
    store: Option<PathBuf>,
    memory: Option<Token>,
}

impl Tokens {
    /// Tokens kept in `store`, or in memory if `None`.
    pub(crate) fn new(store: Option<PathBuf>) -> Self {
        Self {
            store,
            memory: None,
        }
    }

    /// The token stored, or a new one stored in its place if there's
    /// none to trust. One that can't be stored is still given, for this
    /// session. Without a store, the one kept in memory, made the first
    /// time.
    pub(crate) fn load(&mut self) -> Result<Token, String> {
        let Some(store) = &self.store else {
            let token = match self.memory.take() {
                Some(token) => token,
                None => generate()?,
            };
            self.memory = Some(token.clone());
            return Ok(token);
        };
        if let Some(token) = load(store) {
            return Ok(token);
        }
        let token = generate()?;
        if let Err(error) = write(store, &token) {
            log::warn!(
                "Couldn't store the MCP token in {}: {error}",
                store.display()
            );
        }
        Ok(token)
    }

    /// A new token, stored in place of the old, or kept in memory
    /// without a store. One that can't be stored is an error, the old one
    /// staying.
    pub(crate) fn regenerate(&mut self) -> Result<Token, String> {
        let token = generate()?;
        match &self.store {
            Some(store) => write(store, &token).map_err(|e| e.to_string())?,
            None => self.memory = Some(token.clone()),
        }
        Ok(token)
    }
}

/// The most bytes of the file read: a larger one is taken as gone bad.
const MAX_BYTES: u64 = 1024;

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
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    options.open(store)
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
