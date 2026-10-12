//! The MCP server's pieces (see `notes/AgentSupport.md`): MCP's Streamable
//! HTTP on `127.0.0.1`, by hand, the JSON-RPC on it, and the tools an
//! agent edits the open design with. No UI code: `varde-app` starts the
//! server, runs the tools and owns the session's state.
//!
//! Builds on every target, so the wasm check compiles it; only the
//! sockets are native.

pub mod guard;
pub mod http;
#[cfg(not(target_arch = "wasm32"))]
pub mod relay;
pub mod rpc;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;
pub mod session;
pub mod tools;

/// `N` bytes from the system's (or the browser's) random source.
fn random<const N: usize>() -> Result<[u8; N], getrandom::Error> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes)?;
    Ok(bytes)
}

/// `text` cut to at most `bytes`, on a character boundary.
pub fn cut(text: &str, bytes: usize) -> &str {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
