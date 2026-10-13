//! A record's payload: what it holds ([`Payload`]), and its encoding,
//! MessagePack compressed with snappy, within the bounds a reader checks.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use varde_document::codec::{self, DecodeError};
use varde_document::{APP_NAME, CheckError, Document, Unchecked};

use crate::UnixSeconds;

use super::block::{Block, MAX_TAG, frame};
use super::{Error, FileType, Result, Tail};

/// The most any payload may decompress to. Documents are nowhere near it;
/// the bound keeps a file from having the decoder allocate gigabytes.
/// [`encode`] refuses anything larger, so what's saved can be opened.
pub(super) const MAX_DECOMPRESSED: usize = 1 << 30;

/// What a record holds: a [`Document`] in a design's file, or what the
/// owner of a [`HeldFile`](super::HeldFile) keeps in it. Extended as the module docs say.
pub(crate) trait Payload: Serialize + Sized {
    /// The payload as decoded from a file, before [`Payload::check`]: the
    /// same fields, holding an [`Unchecked`] where the payload holds a
    /// [`Document`], so a document failing its check says what's wrong.
    type Unchecked: DeserializeOwned;

    /// What [`Payload::check`] says is wrong, which becomes an
    /// [`Error::Decode`]: a [`CheckError`] stays one, as the
    /// [`DecodeError`]'s [`source`](std::error::Error::source).
    type Error: Into<DecodeError>;

    /// Checks a payload just decoded from a file, like
    /// [`Unchecked::check`], saying what's wrong if anything is.
    fn check(unchecked: Self::Unchecked) -> std::result::Result<Self, Self::Error>;
}

impl Payload for Document {
    type Unchecked = Unchecked;
    type Error = CheckError;

    fn check(unchecked: Unchecked) -> std::result::Result<Document, CheckError> {
        unchecked.check()
    }
}

/// What a record's MessagePack holds: when it was written, and its
/// payload.
#[derive(Serialize)]
pub(super) struct Record<'a, P> {
    pub(super) time: UnixSeconds,
    pub(super) payload: &'a P,
}

/// [`Record`] as decoded, before its payload's check.
#[derive(Deserialize)]
pub(super) struct UncheckedRecord<U> {
    pub(super) time: UnixSeconds,
    pub(super) payload: U,
}

/// The record holding `payload` at `at` in a file of type `ty` with `id`,
/// after the block whose `sum` is `prev` (appending, `tail.sum` and
/// `tail.end`), and the file's tail once it's there.
pub(super) fn record_at(
    id: u128,
    prev: u128,
    at: u64,
    ty: FileType,
    payload: &impl Payload,
) -> Result<(Vec<u8>, Tail)> {
    frame(
        id,
        prev,
        at,
        ty.record(),
        writer().as_bytes(),
        &encode(payload)?,
    )
}

/// A record's tag: the writer, `APP_NAME` and this crate's version.
pub(super) fn writer() -> String {
    let writer = format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
    debug_assert!(writer.len() <= MAX_TAG);
    writer
}

/// Decodes the record `record`, found intact in `bytes`, all of the file,
/// with when it was written. Failing, the error says who wrote it, by its
/// tag's first 128 characters.
pub(super) fn decode_record<P: Payload>(bytes: &[u8], record: &Block) -> Result<(P, UnixSeconds)> {
    let (tag, payload) = record.parts(bytes);
    decode(payload).map_err(|error| Error::Decode {
        writer: String::from_utf8_lossy(tag).chars().take(128).collect(),
        error,
    })
}

/// Encodes `payload` as a record's payload, with the time now. A payload
/// [`decode`] would refuse as too large is [`Error::TooLarge`], rather
/// than a record that saves but won't open.
pub(super) fn encode(payload: &impl Payload) -> Result<Vec<u8>> {
    let record = Record {
        time: UnixSeconds::now(),
        payload,
    };
    let raw = rmp_serde::to_vec_named(&record).expect("payloads always serialize");
    if raw.len() > MAX_DECOMPRESSED {
        return Err(Error::TooLarge);
    }
    Ok(snap::raw::Encoder::new()
        .compress_vec(&raw)
        .expect("snappy input is within size limits"))
}

/// How many times its compressed length a payload may decompress to.
/// Snappy's densest element, a 3 byte copy of 64 bytes, expands about 21
/// times, so a payload claiming more is lying.
pub(super) const MAX_EXPANSION: usize = 32;

/// Decodes a record's payload as a record holding a `P`, see
/// [`from_msgpack`], with when it was written.
pub(super) fn decode<P: Payload>(
    payload: &[u8],
) -> std::result::Result<(P, UnixSeconds), DecodeError> {
    from_msgpack(&decompress(payload)?)
}

/// Decompresses a record's payload, if it claims no more than it can hold.
pub(super) fn decompress(payload: &[u8]) -> std::result::Result<Vec<u8>, DecodeError> {
    // The decoder allocates what the payload claims up front, and the claim
    // is file data: up to 4 GiB.
    let claimed =
        snap::raw::decompress_len(payload).map_err(|e| DecodeError::new(e.to_string()))?;
    let bound = payload
        .len()
        .saturating_mul(MAX_EXPANSION)
        .min(MAX_DECOMPRESSED);
    if claimed > bound {
        return Err(DecodeError::new(format!(
            "the document claims {claimed} bytes, more than {} compressed bytes can hold",
            payload.len()
        )));
    }
    snap::raw::Decoder::new()
        .decompress_vec(payload)
        .map_err(|e| DecodeError::new(e.to_string()))
}

/// rmp-serde's depth limit for a record: arrays and maps may nest one
/// less deep than this. An auto-save's spline handles, the deepest, are 12
/// levels down, in the record's map. A value skipped as an unknown field
/// can nest as deep as the file likes, and rmp-serde's default of 1024
/// overflows the stack of an unoptimized build's lane thread or Web Worker.
pub(super) const MAX_DEPTH: usize = 32;

/// Decodes a record holding a `P` from all of `raw`, as MessagePack, and
/// checks its payload, with when it was written. Bytes left over are an
/// error, see [`codec::whole`].
pub(super) fn from_msgpack<P: Payload>(
    raw: &[u8],
) -> std::result::Result<(P, UnixSeconds), DecodeError> {
    let unchecked = unchecked_record::<P::Unchecked>(raw)?;
    Ok((
        P::check(unchecked.payload).map_err(Into::into)?,
        unchecked.time,
    ))
}

/// Decodes a record holding a `U` from all of `raw`, as MessagePack,
/// before any check.
pub(super) fn unchecked_record<U: DeserializeOwned>(
    raw: &[u8],
) -> std::result::Result<UncheckedRecord<U>, DecodeError> {
    let mut de = rmp_serde::Deserializer::new(raw);
    de.set_max_depth(MAX_DEPTH);
    let unchecked =
        UncheckedRecord::<U>::deserialize(&mut de).map_err(|e| DecodeError::new(e.to_string()))?;
    // Reading takes bytes off the front of the slice.
    codec::whole(unchecked, de.get_ref().len())
}
