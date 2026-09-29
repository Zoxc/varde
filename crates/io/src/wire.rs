//! The bytes that cross to the web's IO worker and back.
//!
//! A worker shares no memory with the page, so requests and responses are
//! encoded, posted and decoded. Plain Rust, so it's tested natively; the
//! web [`lane`](crate::lane) moves the bytes.
//!
//! ```text
//! to the worker = postcard(ToWorker)
//! to the page   = postcard(Reply)
//! ```
//!
//! Each message is posted as one `ArrayBuffer`, transferred, not copied
//! (see `varde_lane::page::Host::post`); a request using a file the user
//! picked has the JS object for the file posted along with it, cloned (see
//! `src/pick.rs`).
//! Documents inside are their [`Document::to_postcard`] bytes, and are
//! checked as they're decoded (see [`codec`]). Paths are strings: on the web they're names in the
//! Origin Private File System, which the lane made, or the names of files
//! the user picked.
//!
//! Neither side sends a message larger than [`MAX_MESSAGE_BYTES`], and the
//! receiving side copies one out of its buffer only within it (see
//! [`varde_lane::bytes`]). Decoding checks what it's given: every byte of
//! it used, and documents that decode and pass their checks. Malformed
//! bytes are refused, never a panic.
//!
//! [`Document::to_postcard`]: varde_document::Document::to_postcard
//! [`codec`]: varde_document::codec

use std::fmt;

use serde::{Deserialize, Serialize};

use varde_document::{DecodeError, codec};

use crate::{Request, Response};

/// The largest message accepted, in bytes. Documents are far smaller;
/// the bound keeps a broken message from being copied without end.
pub const MAX_MESSAGE_BYTES: usize = 1 << 30;

/// A message from the page to the worker: a request, numbered in the order
/// sent. Its response carries the number, see [`Reply`]. The page sends one
/// holding a `&Request`, which encodes the same, so that it needn't clone
/// it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToWorker<R = Request> {
    pub seq: u64,
    pub request: R,
}

/// A message from the worker to the page: the answer to the request
/// numbered `seq`. Requests are handled in order, so everything sent before
/// it was handled too, or replaced by a newer request (see the crate docs).
/// That the worker is ready, or panicked, [`varde_lane`] says.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub seq: u64,
    pub response: Response,
}

/// Encodes `message` to post to the other side, refusing one larger than
/// `max` bytes, which is [`MAX_MESSAGE_BYTES`] but in tests. Also fails
/// for a path that isn't Unicode, which the web's paths always are.
pub fn encode<T: Serialize>(message: &T, max: usize) -> Result<Vec<u8>, Error> {
    let bytes = postcard::to_stdvec(message).map_err(Error::Encode)?;
    if bytes.len() > max {
        return Err(Error::TooLarge(bytes.len()));
    }
    Ok(bytes)
}

/// Encodes the [`Reply`] answering the request numbered `seq`, as
/// [`encode`]. A response that can't be encoded, such as one larger than
/// `max`, is answered with `failed` of why instead: only that request is
/// lost, as when the page can't encode one.
pub fn encode_response(
    seq: u64,
    response: Response,
    failed: impl FnOnce(String) -> Response,
    max: usize,
) -> Result<Vec<u8>, Error> {
    encode(&Reply { seq, response }, max).or_else(|error| {
        let response = failed(error.to_string());
        encode(&Reply { seq, response }, max)
    })
}

/// Decodes a message from the other side, checking it on the way, see
/// the module docs. The bytes were bounded as they were copied.
pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, Error> {
    codec::from_postcard_exact(bytes).map_err(Error::Decode)
}

/// Why a message was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// A message of this many bytes is larger than [`MAX_MESSAGE_BYTES`].
    TooLarge(usize),
    /// The message couldn't be encoded.
    Encode(postcard::Error),
    /// The message, or a document in it, couldn't be decoded, or bytes were
    /// left over after it.
    Decode(DecodeError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooLarge(len) => write!(f, "message too large: {len} bytes"),
            Error::Encode(e) => write!(f, "couldn't encode the message: {e}"),
            Error::Decode(e) => write!(f, "couldn't decode the message: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Encode(e) => Some(e),
            Error::Decode(e) => Some(e),
            Error::TooLarge(_) => None,
        }
    }
}

#[cfg(test)]
mod tests;
