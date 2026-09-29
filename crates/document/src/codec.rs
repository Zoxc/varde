//! A [`Document`] as bytes: postcard, checked as it's decoded.
//!
//! These bytes are a `.vrdp` record's payload before compression (see
//! `varde-io`'s `vrdp` module), and what the regeneration and IO lanes send a
//! Web Worker to work on, through [`document`] and [`snapshot`]. The two
//! share the encoding, not the file format: a worker is sent no records.

use std::fmt;

use serde::de::{self, DeserializeOwned, Visitor};
use serde::{Deserialize, Deserializer, Serializer};

use crate::{CheckError, Document, Snapshot, Unchecked};

impl Document {
    /// Encodes the document as postcard.
    pub fn to_postcard(&self) -> Vec<u8> {
        postcard::to_stdvec(self).expect("documents always serialize")
    }

    /// Decodes a document encoded by [`to_postcard`](Self::to_postcard).
    /// Malformed input, or a document that fails [`Document::check`], is an
    /// error, never a panic.
    pub fn from_postcard(bytes: &[u8]) -> Result<Document, DecodeError> {
        from_postcard(bytes, Unchecked::check)
    }
}

/// Why bytes weren't taken for a document, or for what a file record holds.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeError(Why);

#[derive(Debug, Clone, PartialEq)]
enum Why {
    /// The bytes don't decode, or decode to something refused for another
    /// reason than a document's check.
    Malformed(String),
    /// A document decoded but fails [`Document::check`].
    Invalid(CheckError),
}

impl DecodeError {
    /// Bytes refused for `why`, other than a document failing its check,
    /// which is [`From<CheckError>`].
    pub fn new(why: impl Into<String>) -> DecodeError {
        DecodeError(Why::Malformed(why.into()))
    }
}

impl From<CheckError> for DecodeError {
    fn from(why: CheckError) -> DecodeError {
        DecodeError(Why::Invalid(why))
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Why::Malformed(why) => f.write_str(why),
            Why::Invalid(why) => why.fmt(f),
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.0 {
            Why::Malformed(_) => None,
            Why::Invalid(why) => Some(why),
        }
    }
}

/// Decodes a `T` from all of `bytes`, as postcard. Bytes left over are an
/// error: they're what tells another payload, like an auto-save holding a
/// document, from a prefix that decodes as this one. The one place that
/// rule is kept, for files and for what the lanes send their workers.
pub fn from_postcard_exact<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, DecodeError> {
    let (value, rest) =
        postcard::take_from_bytes(bytes).map_err(|e| DecodeError::new(e.to_string()))?;
    if !rest.is_empty() {
        return Err(DecodeError::new(format!(
            "{} bytes after the end",
            rest.len()
        )));
    }
    Ok(value)
}

/// Decodes an unchecked `U` from all of `bytes`, see
/// [`from_postcard_exact`], and has `check` make it a `T`. Also how
/// `varde-io` decodes what else a file record may hold.
pub fn from_postcard<U: DeserializeOwned, T, E: Into<DecodeError>>(
    bytes: &[u8],
    check: impl FnOnce(U) -> Result<T, E>,
) -> Result<T, DecodeError> {
    check(from_postcard_exact(bytes)?).map_err(Into::into)
}

/// A document as the bytes of [`Document::to_postcard`], for
/// `#[serde(with)]`.
pub mod document {
    use super::*;

    pub fn serialize<S: Serializer>(document: &Document, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&document.to_postcard())
    }

    /// Checks the document, see [`Document::from_postcard`].
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Document, D::Error> {
        d.deserialize_bytes(DocumentVisitor)
    }

    struct DocumentVisitor;

    impl Visitor<'_> for DocumentVisitor {
        type Value = Document;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an encoded document")
        }

        fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Document, E> {
            Document::from_postcard(bytes).map_err(E::custom)
        }
    }
}

/// A shared document, like [`document`].
pub mod snapshot {
    use super::*;

    pub fn serialize<S: Serializer>(document: &Snapshot, s: S) -> Result<S::Ok, S::Error> {
        super::document::serialize(document, s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Snapshot, D::Error> {
        super::document::deserialize(d).map(Snapshot::new)
    }
}

#[cfg(test)]
mod tests;
