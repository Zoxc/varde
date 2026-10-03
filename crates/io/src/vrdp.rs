//! The `.vrdp` file format: an append-only chain of checksummed blocks,
//! one record per save.
//!
//! ```text
//! file   = header | block*
//! header = MAGIC: [u8; 25] | version: u32 LE | id: u128 LE
//! block  = len: u64 LE | prev: u128 LE | sum: u128 LE
//!          | tag_len: u16 LE | tag: [u8; tag_len]
//!          | payload: [u8; len - 2 - tag_len]
//!          | len: u64 LE | kind: [u8; 16]
//! ```
//!
//! There are two file types, each with its own `MAGIC` (`varde-cad`
//! followed by random bytes) and its own kinds of block: a design file,
//! whose records are `RECORD` blocks, and a held file (an auto-save
//! sidecar or a store entry, see `HeldFile`), whose records are `AUTOSAVE`
//! blocks. A file of one type is refused as the other by its header, and a
//! kind of one is a kind the other doesn't know. Every kind is a random 16
//! byte value; blocks of a kind a reader doesn't know are stepped over.
//!
//! A design file's previews ([`Preview`], such as a thumbnail) are
//! `PREVIEW` blocks after its last record, each following on from the block
//! before: its `tag` a media type, at most [`MAX_MEDIA_TYPE`] bytes, its
//! payload the data as it is, at most [`MAX_PREVIEW`] bytes, and at most
//! [`MAX_PREVIEWS`] of them. Every save and whole-file write takes a list
//! of previews, written after its record in the same write; as a save cuts
//! the file to its tail first, it replaces the previews of the save
//! before. Held files never get any. Opening a file steps over them as
//! over any kind it doesn't know, and they're never needed to open or save
//! it: damaged, missing or stale, they're ignored. `read_preview` reads
//! them from the end of the file, walking back to the newest record and
//! reading no record's payload. Saves write one, the design's thumbnail
//! as a PNG (see [`thumbnail`](crate::thumbnail)), when the app rendered
//! it.
//!
//! `len` is the length of `tag_len`, `tag` and `payload`, written before
//! and after them, so a block is 64 + `len` bytes and `kind` ends it.
//! `sum` is the XXH3-128 of the file's `id` (LE) followed by every byte of
//! the block but `sum` itself; `prev` is the `sum` of the block before,
//! the first block's the XXH3-128 of the header. The `id` is random, made
//! anew by every write of a whole file (`write_new`, [`to_bytes`], a
//! replace, a held file written from its start) and kept by appends. So a
//! block of another file, or of an earlier whole-file write, never checks
//! in this one, even holding the same document, and `prev` keeps out
//! intact blocks of this file's own past, like one truncated away that a
//! shorter append didn't cover.
//!
//! A record's `tag` is the writer, `APP_NAME` and this crate's version, at
//! most `MAX_TAG` bytes (readers take any), which a record that won't
//! decode is reported with. Its payload is a MessagePack map `{ time,
//! payload }`, compressed with raw snappy: `time` is when it was written,
//! in [`UnixSeconds`] (only for showing: a clock can be wrong), and
//! `payload` a [`Document`] or whatever else a `HeldFile` holds, see
//! `Payload`. Every length read from a file is checked before anything is
//! allocated, with checked arithmetic (a `usize` is 32 bits on wasm): a
//! block's `len` is at most `MAX_BLOCK`, and what a payload decompresses to
//! at most `MAX_DECOMPRESSED`. Writers check the same bounds, so whatever
//! is written can be read.
//!
//! Reading follows the chain from the first block, each block intact
//! (both its lengths agree and its `sum` checks) and following on (its
//! `prev` is the `sum` of the block before); the newest record reached is
//! the current one. Damage is stepped over by a block's header where that
//! leads to an intact block following on from it, and otherwise ends the
//! chain: in a torn tail, at the newest save damaged, or in a search of the
//! rest of the file, see `chain::Chain::scan`. How the file ended is
//! reported with what's read, see [`Report`]; damage after an intact record
//! never keeps a file from opening, and one with no intact record but
//! damage is [`Error::Corrupt`]. `agents/file-format.md` describes reading
//! and saving in full.
//!
//! The MessagePack is designed to be extended with new features: structs
//! are maps by field name and enum variants go by name
//! (`rmp_serde::to_vec_named`), so a new field `#[serde(default)]` (on the
//! type and its `Unchecked` twin) reads from older files as its default and
//! older builds skip it, and a new variant can go anywhere. Names are what
//! the file holds: never rename or reuse one (`#[serde(rename)]` keeps it
//! if the Rust name changes). Prefer keeping older files working this way;
//! while the app is WIP `version` stays 1 with no migrations, so a change
//! that can't be made so breaks them. The preferred design once the format
//! must be stable:
//!
//! - incompatible changes bump `version`, and every older version is still
//!   decoded, into its own types, and migrated forward before the check;
//! - each record lists the features it needs (names, e.g. `"revolve"`), and
//!   a build lacking one refuses the file or opens it read-only rather than
//!   silently dropping what it doesn't know;
//! - unknown fields are kept and written back on save.
//!
//! The workers are sent documents as postcard instead (see [`codec`]), by
//! position, which is fine as they're built along with the page.
//!
//! Saves are atomic because existing bytes are never rewritten: a crash while
//! appending leaves a torn tail, which readers ignore and the next save
//! truncates. Before appending, a save checks the file still holds the
//! record it last read or wrote, linked to the block before it, with
//! nothing after it but a torn tail or blocks of other kinds, so saving
//! over changes made by someone else is refused, and so is saving over
//! damage reading didn't find (see `check_unchanged`). A save that fails
//! cuts the file back; should that fail too, its record, which may have
//! landed, wholly or partly, is remembered as an `Attempt` and taken for
//! our own by the next save's check. What a writer knows of a file to save
//! to it is a `Known`: after the newest save, damaged with an intact
//! header, a save appends after it, linked to the `sum` its header holds,
//! so nothing is cut off; a file read as [`Outcome::Damaged`] is never
//! saved to, only saved as another file.
//!
//! `read_with_found`, `write_new` and `save` do this on any `Storage`.
//! Only the format is here: the native lane
//! (`src/native/files/document_file.rs`) opens and locks a design's file
//! around each of them, and replaces a file by renaming a new one over it.
//!
//! `HeldFile` is the held file type in a file its owner keeps locked, like
//! an auto-save sidecar: no lock per operation, and it may be empty. It
//! works on any `Storage`: a [`File`] natively, an Origin Private File
//! System handle in the web build's IO worker. Its records may hold another
//! `Payload` than a plain [`Document`], e.g. an auto-save along with the
//! [`Tail`] of the design it was based on.
//!
//! [`to_bytes`], [`from_bytes`] and `check_unchanged` do the same for a
//! whole design file in memory, for the web build, which can only read and
//! replace files of the user's whole, so its files hold one record each.
//! The check before a save is the same code either way, reading through
//! `ReadAt`. Outside this crate, only the whole-file [`to_bytes`] and
//! [`from_bytes`] are public, with the types around them, such as
//! [`Error`], [`Tail`], [`Report`] and [`Preview`].
//!
//! `version` stays 1 while the app is WIP, see `AGENTS.md`.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::{Xxh3Default, xxh3_128};

use varde_document::codec::{self, DecodeError};
use varde_document::{APP_NAME, CheckError, Document, Unchecked};

use crate::UnixSeconds;

use chain::Chain;
pub(crate) use check::check_unchanged;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use preview::read_preview;
#[cfg_attr(target_arch = "wasm32", allow(unused_imports))]
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) use preview::{FileEnd, end};
pub use preview::{MAX_MEDIA_TYPE, MAX_PREVIEW, MAX_PREVIEWS, Preview};

mod chain;
mod check;
mod preview;

/// The start of a design file.
const DESIGN_MAGIC: &[u8; 25] =
    b"varde-cad\x7c\x2f\x88\x90\x95\xb8\x39\xf6\xf9\x1d\xa3\x73\x4d\xb9\x90\x04";
/// The start of a held file: an auto-save sidecar or a store entry.
const HELD_MAGIC: &[u8; 25] =
    b"varde-cad\x81\x59\xb5\x69\xd9\x5c\xe0\x02\x3f\x4f\xb0\x41\x48\xa4\x7b\xa6";
const VERSION: u32 = 1;
/// Length of `MAGIC` plus `version`: the part of the file header that's
/// the same in every file of a type.
const PREFIX_LEN: usize = 25 + 4;
/// Length of the file header, `MAGIC`, `version` and `id`: the offset of
/// the first block.
pub(crate) const FILE_HEADER_LEN: usize = PREFIX_LEN + 16;

/// What kind of block a block is, its last 16 bytes.
type Kind = [u8; 16];
/// A design file's record: a saved [`Document`].
const RECORD: Kind = *b"\xf0\x92\xcd\x00\x73\x28\xa2\xda\x21\x72\x72\xf7\x83\x46\x33\x4d";
/// A design file's preview, after its last record, see [`Preview`].
/// Reading a file's records steps over them as over any kind it doesn't
/// know.
const PREVIEW: Kind = *b"\xf1\x98\x66\xb7\x25\xe7\xf7\xb0\x9d\x73\xef\x9a\x3a\x14\x50\xb6";
/// A held file's record: what a [`HeldFile`] holds.
const AUTOSAVE: Kind = *b"\x16\x85\x72\x3f\x4d\x25\x9d\x78\x8e\x39\xc4\x5f\x2d\xa8\xf1\x8d";

/// Offset of `prev` in a block.
const PREV_AT: usize = 8;
/// Offset of `sum` in a block.
const SUM_AT: usize = 24;
/// Offset of `tag_len` in a block, after `len`, `prev` and `sum`.
const TAG_LEN_AT: usize = 40;
/// Bytes a block adds around what its `len` covers.
const BLOCK_OVERHEAD: u64 = 64;
/// Bytes after what a block's `len` covers: `len` again, and `kind`.
const BACK_LEN: usize = 8 + 16;

/// The most any payload may decompress to. Documents are nowhere near it;
/// the bound keeps a file from having the decoder allocate gigabytes.
/// [`encode`] refuses anything larger, so what's saved can be opened.
const MAX_DECOMPRESSED: usize = 1 << 30;

/// The most a block's `len` may be: what snappy may compress
/// [`MAX_DECOMPRESSED`] bytes to (`snap::raw::max_compress_len`, which
/// isn't `const`, so its formula, checked by a test), `tag_len`, and the
/// longest tag. Checked before anything is allocated for a block, whatever
/// its kind.
const MAX_BLOCK: u64 = {
    let decompressed = MAX_DECOMPRESSED as u64;
    32 + decompressed + decompressed / 6 + 2 + u16::MAX as u64
};

/// The longest tag a writer writes. Readers take any a `u16` holds.
const MAX_TAG: usize = 1024;

/// Which of the two types of `.vrdp` file a file is, see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileType {
    /// A design's own file.
    Design,
    /// An auto-save sidecar or a store entry, see [`HeldFile`].
    Held,
}

impl FileType {
    fn magic(self) -> &'static [u8; 25] {
        match self {
            FileType::Design => DESIGN_MAGIC,
            FileType::Held => HELD_MAGIC,
        }
    }

    /// The kind of its records.
    fn record(self) -> Kind {
        match self {
            FileType::Design => RECORD,
            FileType::Held => AUTOSAVE,
        }
    }

    fn other(self) -> FileType {
        match self {
            FileType::Design => FileType::Held,
            FileType::Held => FileType::Design,
        }
    }

    /// The error for a file of this type read as the other.
    fn read_as_other(self) -> Error {
        match self {
            FileType::Design => Error::IsDesign,
            FileType::Held => Error::IsAutoSave,
        }
    }

    /// `MAGIC` and `version`, which every file of the type starts with.
    fn prefix(self) -> [u8; PREFIX_LEN] {
        let mut prefix = [0; PREFIX_LEN];
        let (magic, version) = prefix.split_at_mut(25);
        magic.copy_from_slice(self.magic());
        version.copy_from_slice(&VERSION.to_le_bytes());
        prefix
    }

    /// The file header of a file of this type with `id`.
    fn header(self, id: u128) -> [u8; FILE_HEADER_LEN] {
        let mut header = [0; FILE_HEADER_LEN];
        let (prefix, rest) = header.split_at_mut(PREFIX_LEN);
        prefix.copy_from_slice(&self.prefix());
        rest.copy_from_slice(&id.to_le_bytes());
        header
    }
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The file doesn't start with a `.vrdp` magic number.
    NotVarde,
    /// The file is an auto-save, opened as a design.
    IsAutoSave,
    /// The file is a design, opened as an auto-save.
    IsDesign,
    /// The file was written in a format version this build can't read.
    UnsupportedVersion(u32),
    /// The file contains no complete record.
    Empty,
    /// The file is damaged at `offset`: before any intact record when
    /// reading, or, appending to a held file, where its records, none of
    /// them intact, begin to be damaged.
    Corrupt {
        offset: u64,
    },
    /// The newest record couldn't be decoded, e.g. it was written by an
    /// incompatible build: `writer`, the record's tag, as far as it's
    /// shown.
    Decode {
        writer: String,
        error: DecodeError,
    },
    /// The file changed since it was last read or written.
    Conflict,
    /// The file was damaged since it was last read or written: damage
    /// reading didn't find, or the record it read or wrote last. Saving
    /// is refused, as it could cut off what's still readable.
    Damaged,
    /// The file was read as [`Outcome::Damaged`]: saving to it is refused,
    /// as cutting it short could lose what can still be got out of it, so
    /// it's only saved as another file.
    OpenedDamaged,
    /// The document is too large to save: it would decode to more than a
    /// file may hold, so it couldn't be opened again.
    TooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::NotVarde => write!(f, "not a {APP_NAME} file"),
            Error::IsAutoSave => f.write_str("an auto-save, not a design"),
            Error::IsDesign => f.write_str("a design, not an auto-save"),
            Error::UnsupportedVersion(v) => write!(f, "unsupported file format version {v}"),
            Error::Empty => f.write_str("file contains no saved document"),
            Error::Corrupt { offset } => write!(f, "file is damaged at byte {offset}"),
            Error::Decode { writer, error } => write!(
                f,
                "couldn't decode document (saved by {writer}, this is {}): {error}",
                env!("CARGO_PKG_VERSION")
            ),
            Error::Conflict => f.write_str("file was changed by someone else"),
            Error::Damaged => f.write_str("file was damaged since it was opened"),
            Error::OpenedDamaged => {
                f.write_str("file is damaged, so it can only be saved as another file")
            }
            Error::TooLarge => f.write_str("document is too large to save"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Decode { error, .. } => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Where the chain of a file ends, as last read or written by us: its last
/// record.
///
/// It also tells which saved version the file holds: another save, by
/// anyone, or the file rewritten, even with the same content, gives it
/// another tail, as the record's `sum` covers the file's `id` and, through
/// `prev`, what the record was appended to. The IO lane hands out its
/// design file's to be stored, e.g. with an auto-save, and compared later;
/// one read back from a file is only ever compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tail {
    /// Offset of the last record.
    last: u64,
    /// The last record's `sum`.
    sum: u128,
    /// Offset just past the last record.
    end: u64,
}

/// A save that failed once it had started writing its record, which may
/// have landed anyway, wholly or partly, with cutting the file back failing
/// too. [`Known`] keeps it until a save succeeds, so that
/// [`check_unchanged`] takes a block whose header holds its `sum` and
/// `prev`, intact or not, for this save's own, to be saved over, rather
/// than someone else's change or damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Attempt {
    /// The `sum` the record's header holds.
    sum: u128,
    /// The `prev` the record's header holds.
    prev: u128,
}

impl Attempt {
    /// The attempt to write `block`, a whole block, as its header has it.
    fn of(block: &[u8]) -> Attempt {
        Attempt {
            sum: u128_at(block, SUM_AT),
            prev: u128_at(block, PREV_AT),
        }
    }
}

/// What a writer knows of a design file, as it last read or wrote it, to
/// save to it: its `id`, its [`Tail`], where the block before the tail's
/// record starts, how reading found what follows, and a failed save's
/// [`Attempt`]. Natively a `DocumentFile` keeps one, and the web build's
/// IO worker one for each file of the user's it saves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Known {
    id: u128,
    tail: Tail,
    /// Where the block the tail's record follows on from starts, as
    /// reading or writing found it, perhaps a damaged one stepped over by
    /// its header, whose trailing `len` may be damaged too: `None` for the
    /// file's first record, which follows on from the header, or if it's
    /// not known.
    before: Option<u64>,
    after: After,
    attempt: Option<Attempt>,
}

/// What reading found after the record at a [`Known`] tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum After {
    /// Nothing that matters to a save: the end, blocks of other kinds, a
    /// torn tail.
    Nothing,
    /// The newest save, damaged, its header intact: a save appends after
    /// it, see [`Outcome::NewestDamaged`].
    NewestDamaged(DamagedRecord),
    /// Damage found only by searching, see [`Outcome::Damaged`]: saves are
    /// refused.
    Damaged,
}

impl Tail {
    /// The last record's `sum`, which tells it from any other save, as
    /// downloads are recorded by (`src/downloads.rs`).
    #[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
    pub(crate) fn sum(&self) -> u128 {
        self.sum
    }

    /// Where its record is in the file.
    #[cfg(test)]
    pub(crate) fn span(&self) -> std::ops::Range<u64> {
        self.last..self.end
    }
}

impl Known {
    /// A file with `id` just written whole, or saved to, ending at `tail`.
    fn written(id: u128, tail: Tail) -> Known {
        Known {
            id,
            tail,
            before: None,
            after: After::Nothing,
            attempt: None,
        }
    }

    /// The saved version the file held when last read or written.
    pub(crate) fn tail(&self) -> Tail {
        self.tail
    }
}

/// Writes `document` to the empty file `file`, making it a design file with
/// a new `id`, holding it as its one record followed by `previews`, see
/// [`whole_file`], and returns what's known of it, to save to it. Syncing
/// is up to the caller.
pub(crate) fn write_new(
    file: &mut impl Storage,
    document: &Document,
    previews: &[Preview],
) -> Result<Known> {
    let (bytes, known) = whole_file(document, previews)?;
    file.truncate(0)?;
    file.write_at(&bytes, 0)?;
    Ok(known)
}

/// Appends `document` to the design file `file` as a new version, and
/// returns the file's new tail, which `known` holds from then on. The file
/// must still be as `known` last read or wrote it, see [`check_unchanged`],
/// the same check: someone else's change is [`Error::Conflict`], damage
/// since [`Error::Damaged`], and a file read as [`Outcome::Damaged`] is
/// refused, [`Error::OpenedDamaged`]. A torn tail after it is dropped, and
/// so is the record of the last failed save, if it's there. After the
/// newest save damaged, its header intact, as reading found it, the record
/// goes after that, linked to the `sum` its header holds, so nothing is
/// cut off and reading steps over it by its header. The file keeps its
/// `id`. The first [`MAX_PREVIEWS`] of `previews` follow the record, in the
/// same write, replacing the previews of the save before, which were after
/// its tail.
///
/// The record is synced before this returns. Should truncating, writing or
/// syncing fail, the file is cut back to where it went: the record may be
/// partly written, or not on disk, and syncing a later one wouldn't put it
/// there, so the next save writes it anew rather than finding a newer
/// version and calling it a conflict. Should cutting back fail too, the
/// record is left for `known` to recognize by its header: it's remembered
/// once writing starts, and forgotten once a save succeeds.
pub(crate) fn save(
    file: &mut impl Storage,
    known: &mut Known,
    document: &Document,
    previews: &[Preview],
) -> Result<Tail> {
    let append = check_unchanged(file, known)?;
    let (mut bytes, new) = record_at(
        append.id,
        append.prev,
        append.at,
        FileType::Design,
        document,
    )?;
    preview::append_previews(&mut bytes, append.id, new, previews)?;
    write_or_cut_back(file, append.at, &bytes, || {
        known.attempt = Some(Attempt::of(&bytes));
    })?;
    *known = Known {
        before: Some(append.before),
        ..Known::written(append.id, new)
    };
    Ok(new)
}

/// A whole design file holding `document` as its one record, with a new
/// `id`, followed by the first [`MAX_PREVIEWS`] of `previews`, and the
/// file's tail: what the web build writes to a file of the user's, which
/// it can only replace, not append to (see `src/pick.rs`).
pub fn to_bytes(document: &Document, previews: &[Preview]) -> Result<(Vec<u8>, Tail)> {
    let (bytes, known) = whole_file(document, previews)?;
    Ok((bytes, known.tail))
}

/// [`to_bytes`], with what's known of the file written, to save to it.
pub(crate) fn whole_file(document: &Document, previews: &[Preview]) -> Result<(Vec<u8>, Known)> {
    let (bytes, tail, id) = whole(FileType::Design, document, previews)?;
    Ok((bytes, Known::written(id, tail)))
}

/// Reads the newest document of the whole design file `bytes`, with the
/// file's tail, as `from_bytes_with_report` does, less the report.
pub fn from_bytes(bytes: &[u8]) -> Result<(Document, Tail)> {
    from_bytes_with_report(bytes).map(|opened| (opened.payload, opened.tail))
}

/// Reads the newest document the chain of the whole design file `bytes`
/// proves, with the file's tail and how reading found the file, see
/// [`Report`]. Damage after an intact record never keeps the file from
/// opening; a file with no intact record is [`Error::Empty`], or
/// [`Error::Corrupt`] if it's damaged.
pub(crate) fn from_bytes_with_report(bytes: &[u8]) -> Result<Opened<Document>> {
    Chain::scan(bytes, FileType::Design)?
        .open(bytes)?
        .ok_or(Error::Empty)
}

/// Reads the design file `file` whole, as [`from_bytes_with_found`] reads
/// it. Anything but a design file is refused having read no more than the
/// file header.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn read_with_found(file: &impl ReadAt) -> Result<WithFound> {
    from_bytes_with_found(&read_all(file, FileType::Design)?)
}

/// [`from_bytes_with_report`], with the newest record the search found
/// past damage, if the file is [`Outcome::Damaged`] and that's another
/// record than the one opened, opened instead, if it decodes: its tail is
/// the [`FoundRecord::tail`] the report names, and its report
/// [`Outcome::Damaged`] with nothing else found. What's known of the file
/// through it ([`Opened::known`]) refuses saves, as the file is damaged.
pub(crate) fn from_bytes_with_found(bytes: &[u8]) -> Result<WithFound> {
    let chain = Chain::scan(bytes, FileType::Design)?;
    let opened = chain.open(bytes)?.ok_or(Error::Empty)?;
    let found = chain.open_found(bytes);
    Ok(WithFound { opened, found })
}

/// A design file read with [`from_bytes_with_found`].
#[derive(Debug)]
pub(crate) struct WithFound {
    /// The newest record the chain proves, as [`from_bytes_with_report`]
    /// opens it.
    pub(crate) opened: Opened<Document>,
    /// The newest record a search found past damage, if it's another and
    /// decodes.
    pub(crate) found: Option<Opened<Document>>,
}

/// Whether an auto-save based on the save at `base` of a design file,
/// `file`, opened at `opened`, was based on a newer save than the one
/// opened, which couldn't be read: `base` is at or after the end of the
/// save opened, and the block there has a header, intact or not, whose
/// `sum` is `base`'s.
pub(crate) fn based_past(
    file: &(impl ReadAt + ?Sized),
    opened: Tail,
    base: Tail,
) -> io::Result<bool> {
    if base.last < opened.end {
        return Ok(false);
    }
    Ok(stored_at(file, base.last, file.len()?)?.is_some_and(|stored| stored.sum == base.sum))
}

/// A record read from a file: what it holds, the file's tail as reading
/// found it, and how reading found the file.
#[derive(Debug)]
pub(crate) struct Opened<P> {
    pub(crate) payload: P,
    /// Where the record opened is: what a save appends after.
    pub(crate) tail: Tail,
    pub(crate) report: Report,
    /// The file's `id`.
    id: u128,
    /// Where the block the record follows on from starts, see
    /// [`Known`]'s.
    before: Option<u64>,
}

impl<P> Opened<P> {
    /// What's known of the design file read, to save to it: a save goes
    /// after the newest save if it's damaged with an intact header, and is
    /// refused if reading found the file [`Outcome::Damaged`].
    pub(crate) fn known(&self) -> Known {
        let after = match self.report.outcome {
            Outcome::NewestDamaged(damaged) => After::NewestDamaged(damaged),
            Outcome::Damaged { .. } => After::Damaged,
            Outcome::Intact | Outcome::TornTail | Outcome::Bridged => After::Nothing,
        };
        Known {
            before: self.before,
            after,
            ..Known::written(self.id, self.tail)
        }
    }
}

/// How reading a file found it: how its chain ended, see [`Outcome`], when
/// the record opened was written and how many bytes can't be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub outcome: Outcome,
    /// When the record opened was written, by the writer's clock: only for
    /// showing.
    pub time: UnixSeconds,
    /// The bytes that can't be read: damaged blocks, blocks of the file's
    /// past found by a search, and whatever a search didn't take in. A
    /// torn tail isn't counted.
    pub unreadable: u64,
}

/// How a file's chain ended, as reading followed it, from the best to the
/// worst. The record opened is the newest the chain proves, except where
/// [`Outcome::Damaged`] says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The chain reaches the end of the file.
    Intact,
    /// The chain ends in a torn tail: nothing after its end is framed like
    /// a record (an interrupted save, zeros, a block of the file's past, a
    /// damaged preview, say). It's ignored, and the next save cuts it off.
    TornTail,
    /// Damaged blocks were stepped over by their headers to a record after
    /// them; the chain ended as [`Outcome::TornTail`] or
    /// [`Outcome::Intact`] would. Damaged blocks of other kinds after the
    /// last record, like a damaged preview, are a torn tail instead.
    Bridged,
    /// The block after the chain's end is framed like a record, with an
    /// intact header, but isn't intact, or the chain stepped over such a
    /// record by its header to blocks of other kinds only: the newest save
    /// is damaged, and the record opened is the one before it.
    NewestDamaged(DamagedRecord),
    /// Past the chain's end, blocks could only be found by searching the
    /// rest of the file, or nothing could; something there is framed like
    /// a record.
    Damaged {
        /// The newest record the search found, if it isn't the record
        /// opened. That's the search's newest when the chain holds no
        /// record.
        found: Option<FoundRecord>,
    },
}

/// The newest save, damaged but with an intact header: where it is, and
/// the `sum` its header holds, which a save appending after it links to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamagedRecord {
    /// Its offset in the file.
    pub(crate) start: u64,
    /// The offset just past it, by its header's `len`.
    pub(crate) end: u64,
    /// The `sum` its header holds.
    pub(crate) sum: u128,
}

/// A record found by searching a file past damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoundRecord {
    /// Where it is, as the tail of the file cut short after it.
    pub tail: Tail,
    /// When it was written, unless its payload won't decode.
    pub time: Option<UnixSeconds>,
}

/// A held file: a `.vrdp` file its owner holds open and keeps other writers
/// away from, like an auto-save sidecar, which the IO lane holds an OS lock
/// on for as long as its design is open, or, on the web, a store entry
/// held through the Origin Private File System's exclusive sync access
/// handle.
///
/// Unlike a design's file, which the IO lane locks for each operation, it
/// needs no lock of its own: the owner's is enough, and one of its own
/// would be refused as held by someone else (`flock` locks per open file
/// description, `LockFileEx` per handle). Nor does it check for changes
/// made by others. It may be empty, which holds no document yet, and can
/// be emptied again. Appends are crash-safe like [`save`]'s: the last
/// complete record stays readable. Written from its start, it gets a new
/// `id`; appends keep it. Like a save, an append goes after the newest
/// record if it's damaged with an intact header, but cuts off other damage
/// after the record read; one whose records are framed but none intact
/// isn't written to at all, see [`HeldFile::append`].
///
/// Each record holds a `P`: a [`Document`] like a design's file, or
/// whatever else its owner keeps, see [`Payload`].
#[derive(Debug)]
pub(crate) struct HeldFile<S = File, P = Document> {
    file: S,
    held: Held,
    payload: PhantomData<fn() -> P>,
}

/// What a [`HeldFile`] is known to hold.
#[derive(Debug, Clone, Copy)]
enum Held {
    /// Not read yet, or left unknown by an error.
    Unknown,
    /// No record, or nothing of use: nothing at all, the file header, a
    /// torn first record, or what isn't a held file or won't decode.
    Empty,
    /// Records in the file with `id`, the last one read or written at
    /// `tail`, and after it the newest record, `damaged` with an intact
    /// header, if reading found one.
    At {
        id: u128,
        tail: Tail,
        damaged: Option<DamagedRecord>,
    },
    /// Records framed, but none intact: damaged from `offset`.
    Damaged { offset: u64 },
}

impl<S: Storage, P: Payload> HeldFile<S, P> {
    /// Takes over `file`, which must be open for reading and writing.
    pub(crate) fn new(file: S) -> Self {
        Self {
            file,
            held: Held::Unknown,
            payload: PhantomData,
        }
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn storage(&self) -> &S {
        &self.file
    }

    /// Whether the file has nothing in it at all.
    pub(crate) fn is_empty(&self) -> io::Result<bool> {
        self.file.is_empty()
    }

    /// Reads the newest record, or `None` if there is none yet, as
    /// [`HeldFile::read_with_report`] does, less the report.
    pub(crate) fn read(&mut self) -> Result<Option<P>> {
        Ok(self.read_with_report()?.map(|opened| opened.payload))
    }

    /// Reads the newest record, as [`from_bytes_with_report`] does with a
    /// design's file, or `None` if there is none and nothing is damaged:
    /// nothing at all, a torn first record, or a file header cut short, as
    /// left by a crash while the first record was written. No intact record
    /// but damage is [`Error::Corrupt`].
    pub(crate) fn read_with_report(&mut self) -> Result<Option<Opened<P>>> {
        self.held = Held::Unknown;
        let bytes = read_all(&self.file, FileType::Held)?;
        let chain = Chain::scan(&bytes, FileType::Held)?;
        let opened = chain.open(&bytes);
        self.held = match &opened {
            Ok(None) => Held::Empty,
            Ok(Some(opened)) => Held::At {
                id: chain.id,
                tail: opened.tail,
                damaged: match opened.report.outcome {
                    Outcome::NewestDamaged(damaged) => Some(damaged),
                    _ => None,
                },
            },
            Err(Error::Corrupt { offset }) => Held::Damaged { offset: *offset },
            // The newest record won't decode.
            Err(_) => Held::Unknown,
        };
        opened
    }

    /// Appends `payload` as the newest record, after the record last read
    /// or written, or after the newest record if reading found it damaged
    /// with an intact header, cutting off a torn tail and other damage.
    ///
    /// A file whose records are framed but none intact is left as it is,
    /// [`Error::Corrupt`], until its owner empties it, say once the user
    /// discards it: what's in it may still be got out. So is one that
    /// couldn't be read, [`Error::Io`]. Whatever else the file holds that
    /// can't be read, something not a held file or a newest record that
    /// won't decode, it starts over from, with a new `id`: it's of no use
    /// to anyone.
    pub(crate) fn append(&mut self, payload: &P) -> Result<()> {
        if let Held::Unknown = self.held {
            match self.read() {
                Ok(_) | Err(Error::Corrupt { .. }) => {}
                Err(Error::Io(e)) => return Err(Error::Io(e)),
                Err(_) => self.held = Held::Empty,
            }
        }
        let (start, bytes, id, tail) = match self.held {
            Held::Damaged { offset } => return Err(Error::Corrupt { offset }),
            Held::At { id, tail, damaged } => {
                let (at, prev) =
                    damaged.map_or((tail.end, tail.sum), |damaged| (damaged.end, damaged.sum));
                let (bytes, new) = record_at(id, prev, at, FileType::Held, payload)?;
                (at, bytes, id, new)
            }
            Held::Empty | Held::Unknown => {
                // Held files never get previews.
                let (bytes, tail, id) = whole(FileType::Held, payload, &[])?;
                (0, bytes, id, tail)
            }
        };
        // Should writing fail, the file is cut back, and read again before
        // the next one, in case that failed too.
        self.held = Held::Unknown;
        write_or_cut_back(&mut self.file, start, &bytes, || {})?;
        self.held = Held::At {
            id,
            tail,
            damaged: None,
        };
        Ok(())
    }

    /// Empties the file.
    pub(crate) fn clear(&mut self) -> io::Result<()> {
        self.held = Held::Unknown;
        self.file.truncate(0)?;
        self.file.sync()?;
        self.held = Held::Empty;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn into_storage(self) -> S {
        self.file
    }
}

/// What a record holds: a [`Document`] in a design's file, or what the
/// owner of a [`HeldFile`] keeps in it. Extended as the module docs say.
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
struct Record<'a, P> {
    time: UnixSeconds,
    payload: &'a P,
}

/// [`Record`] as decoded, before its payload's check.
#[derive(Deserialize)]
struct UncheckedRecord<U> {
    time: UnixSeconds,
    payload: U,
}

/// What the `.vrdp` code needs to read a file: its length, and reading at
/// an offset. [`Storage`] adds writing; a byte slice, a whole file read
/// into memory, only reads.
pub(crate) trait ReadAt {
    /// The length of the file, in bytes.
    fn len(&self) -> io::Result<u64>;

    /// Whether the file has nothing in it.
    fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Fills `buf` with the bytes starting at `at`, failing with
    /// [`io::ErrorKind::UnexpectedEof`] if the file ends before.
    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()>;
}

/// What the `.vrdp` code needs of a file: reading and writing at an
/// offset, truncating and syncing. [`File`] natively; the web build's IO
/// worker implements it on an Origin Private File System sync access
/// handle, which only has these, and no cursor.
pub(crate) trait Storage: ReadAt {
    /// Writes all of `buf` starting at `at`, growing the file if needed.
    fn write_at(&mut self, buf: &[u8], at: u64) -> io::Result<()>;

    /// Cuts the file to `len` bytes, or grows it with zeros to that.
    fn truncate(&mut self, len: u64) -> io::Result<()>;

    /// Makes what was written durable.
    fn sync(&mut self) -> io::Result<()>;
}

impl ReadAt for [u8] {
    fn len(&self) -> io::Result<u64> {
        // `usize` is at most 64 bits wide, so exact.
        Ok(<[u8]>::len(self) as u64)
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        let bytes = usize::try_from(at)
            .ok()
            .and_then(|at| self.get(at..at.checked_add(buf.len())?))
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        buf.copy_from_slice(bytes);
        Ok(())
    }
}

impl ReadAt for File {
    fn len(&self) -> io::Result<u64> {
        Ok(self.metadata()?.len())
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        let mut file = self;
        file.seek(SeekFrom::Start(at))?;
        file.read_exact(buf)
    }
}

impl Storage for File {
    fn write_at(&mut self, buf: &[u8], at: u64) -> io::Result<()> {
        self.seek(SeekFrom::Start(at))?;
        self.write_all(buf)
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.set_len(len)
    }

    fn sync(&mut self) -> io::Result<()> {
        self.sync_data()
    }
}

/// An intact block: both its lengths agree, and its `sum` checks.
#[derive(Debug, Clone, Copy)]
struct Block {
    /// Its offset in the file.
    start: u64,
    /// Its `len`.
    len: u64,
    tag_len: u16,
    prev: u128,
    sum: u128,
    kind: Kind,
    /// The offset just past it.
    end: u64,
}

impl Block {
    /// The tail of the file cut short right after it.
    fn tail(&self) -> Tail {
        Tail {
            last: self.start,
            sum: self.sum,
            end: self.end,
        }
    }

    /// Its tag and its payload, in `bytes`, all of the file it's in.
    fn parts<'a>(&self, bytes: &'a [u8]) -> (&'a [u8], &'a [u8]) {
        // Within `bytes`, where the block was found intact, so exact.
        let start = self.start as usize + TAG_LEN_AT + 2;
        let tag_end = start + usize::from(self.tag_len);
        let end = self.start as usize + TAG_LEN_AT + self.len as usize;
        (&bytes[start..tag_end], &bytes[tag_end..end])
    }
}

/// `block`, all of a block by its first `len`, at `start` in a file with
/// `id`, if it's intact.
fn intact(block: &[u8], start: u64, id: u128) -> Option<Block> {
    let size = block.len();
    let len = u64_at(block, 0);
    let tag_len = u16::from_le_bytes([block[TAG_LEN_AT], block[TAG_LEN_AT + 1]]);
    if len > MAX_BLOCK || u64_at(block, size - BACK_LEN) != len || u64::from(tag_len) + 2 > len {
        return None;
    }
    let sum = u128_at(block, SUM_AT);
    if block_sum(id, block) != sum {
        return None;
    }
    Some(Block {
        start,
        len,
        tag_len,
        prev: u128_at(block, PREV_AT),
        sum,
        kind: block[size - 16..].try_into().unwrap(),
        // `usize` is at most 64 bits wide.
        end: start.checked_add(size as u64)?,
    })
}

/// The `sum` of `block` in a file with `id`: of `id` and every byte of the
/// block but its `sum`.
fn block_sum(id: u128, block: &[u8]) -> u128 {
    let mut hasher = Xxh3Default::new();
    hasher.update(&id.to_le_bytes());
    hasher.update(&block[..SUM_AT]);
    hasher.update(&block[SUM_AT + 16..]);
    hasher.digest128()
}

/// What a block's header holds, whether or not the block is intact.
struct Stored {
    prev: u128,
    sum: u128,
    /// Where the block ends by its first `len`, if that's within bounds:
    /// at least 2, for `tag_len`, and at most [`MAX_BLOCK`].
    end: Option<u64>,
}

/// What the header of the block at `at` in `file`, which is `len` bytes
/// long, holds, if the file holds all of the header.
fn stored_at(file: &(impl ReadAt + ?Sized), at: u64, len: u64) -> io::Result<Option<Stored>> {
    // `TAG_LEN_AT` is small, so exact.
    if len
        .checked_sub(at)
        .is_none_or(|rest| rest < TAG_LEN_AT as u64)
    {
        return Ok(None);
    }
    let mut front = [0; TAG_LEN_AT];
    file.read_at(&mut front, at)?;
    let block_len = u64_at(&front, 0);
    Ok(Some(Stored {
        prev: u128_at(&front, PREV_AT),
        sum: u128_at(&front, SUM_AT),
        end: (2..=MAX_BLOCK)
            .contains(&block_len)
            .then(|| at.checked_add(BLOCK_OVERHEAD + block_len))
            .flatten(),
    }))
}

/// The block ending at `end` in `file`, found through its trailing `len`,
/// if that leads back to a block starting at or after `from`: where it
/// starts, and what its header holds.
fn stored_before(
    file: &(impl ReadAt + ?Sized),
    end: u64,
    from: u64,
) -> io::Result<Option<(u64, Stored)>> {
    let Some(len_at) = end.checked_sub(BACK_LEN as u64) else {
        return Ok(None);
    };
    let mut len = [0; 8];
    file.read_at(&mut len, len_at)?;
    let start = u64::from_le_bytes(len)
        .checked_add(BLOCK_OVERHEAD)
        .and_then(|size| end.checked_sub(size))
        .filter(|&start| start >= from);
    let Some(start) = start else {
        return Ok(None);
    };
    // The block ends at `end`, so `end` bounds its header as the file's
    // length would.
    Ok(stored_at(file, start, end)?.map(|stored| (start, stored)))
}

/// The file header of `file`, which is `len` bytes long, if it's a design
/// file's, whatever its `id`.
fn design_header(
    file: &(impl ReadAt + ?Sized),
    len: u64,
) -> io::Result<Option<[u8; FILE_HEADER_LEN]>> {
    // `FILE_HEADER_LEN` is small, so exact.
    if len < FILE_HEADER_LEN as u64 {
        return Ok(None);
    }
    let mut header = [0; FILE_HEADER_LEN];
    file.read_at(&mut header, 0)?;
    Ok((header[..PREFIX_LEN] == FileType::Design.prefix()).then_some(header))
}

/// Reads `file` from `from` to `to` in pieces, handing each to `each` until
/// it returns false, so that a large block is never held in memory.
/// Whether `each` took every piece.
fn read_pieces(
    file: &(impl ReadAt + ?Sized),
    from: u64,
    to: u64,
    mut each: impl FnMut(&[u8]) -> bool,
) -> io::Result<bool> {
    let mut piece = vec![0; 1 << 16];
    let mut at = from;
    while at < to {
        // At most the piece's length, so exact.
        let n = (to - at).min(piece.len() as u64) as usize;
        file.read_at(&mut piece[..n], at)?;
        if !each(&piece[..n]) {
            return Ok(false);
        }
        at += n as u64;
    }
    Ok(true)
}

/// The record holding `payload` at `at` in a file of type `ty` with `id`,
/// after the block whose `sum` is `prev` (appending, `tail.sum` and
/// `tail.end`), and the file's tail once it's there.
fn record_at(
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

/// The block of `kind` holding `tag` and `payload` at `at` in a file with
/// `id`, after the block whose `sum` is `prev`, and the tail of the file
/// cut short after it. One too large to read back is [`Error::TooLarge`].
fn frame(
    id: u128,
    prev: u128,
    at: u64,
    kind: Kind,
    tag: &[u8],
    payload: &[u8],
) -> Result<(Vec<u8>, Tail)> {
    let tag_len = u16::try_from(tag.len()).map_err(|_| Error::TooLarge)?;
    let len = u64::try_from(payload.len())
        .ok()
        .and_then(|len| len.checked_add(u64::from(tag_len) + 2))
        .filter(|&len| len <= MAX_BLOCK)
        .ok_or(Error::TooLarge)?;
    let size = len
        .checked_add(BLOCK_OVERHEAD)
        .and_then(|size| usize::try_from(size).ok())
        .ok_or(Error::TooLarge)?;
    let mut block = Vec::new();
    block.try_reserve_exact(size).map_err(|_| Error::TooLarge)?;
    block.extend_from_slice(&len.to_le_bytes());
    block.extend_from_slice(&prev.to_le_bytes());
    block.extend_from_slice(&[0; 16]);
    block.extend_from_slice(&tag_len.to_le_bytes());
    block.extend_from_slice(tag);
    block.extend_from_slice(payload);
    block.extend_from_slice(&len.to_le_bytes());
    block.extend_from_slice(&kind);
    let sum = block_sum(id, &block);
    block[SUM_AT..SUM_AT + 16].copy_from_slice(&sum.to_le_bytes());
    let end = u64::try_from(size)
        .ok()
        .and_then(|size| at.checked_add(size))
        .ok_or_else(|| io::Error::from(io::ErrorKind::FileTooLarge))?;
    Ok((block, Tail { last: at, sum, end }))
}

/// A record's tag: the writer, `APP_NAME` and this crate's version.
fn writer() -> String {
    let writer = format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
    debug_assert!(writer.len() <= MAX_TAG);
    writer
}

/// Writes `bytes` at `at`, dropping whatever follows: a torn tail, or
/// anything else, and syncs them. Should any of that fail, the file is cut
/// back to `at`, as far as it can be. `writing` is called once the bytes
/// may land, before they're written.
fn write_or_cut_back(
    file: &mut impl Storage,
    at: u64,
    bytes: &[u8],
    writing: impl FnOnce(),
) -> io::Result<()> {
    let written = file.truncate(at).and_then(|()| {
        writing();
        file.write_at(bytes, at)?;
        file.sync()
    });
    if written.is_err() {
        let _ = file.truncate(at);
    }
    written
}

/// A whole file of type `ty` with a new `id` holding `payload` as its one
/// record, followed by the first [`MAX_PREVIEWS`] of `previews`, its tail
/// and its `id`.
fn whole(
    ty: FileType,
    payload: &impl Payload,
    previews: &[Preview],
) -> Result<(Vec<u8>, Tail, u128)> {
    let id = new_id()?;
    let header = ty.header(id);
    // `FILE_HEADER_LEN` is small, so exact.
    let (record, tail) = record_at(id, xxh3_128(&header), FILE_HEADER_LEN as u64, ty, payload)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(FILE_HEADER_LEN.saturating_add(record.len()))
        .map_err(|_| Error::TooLarge)?;
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&record);
    preview::append_previews(&mut bytes, id, tail, previews)?;
    Ok((bytes, tail, id))
}

/// A new random file `id`.
fn new_id() -> io::Result<u128> {
    let mut id = [0; 16];
    getrandom::fill(&mut id).map_err(|e| io::Error::other(format!("no random file id: {e}")))?;
    Ok(u128::from_le_bytes(id))
}

/// The `id` in a file header.
fn header_id(header: &[u8; FILE_HEADER_LEN]) -> u128 {
    u128_at(header, PREFIX_LEN)
}

/// Decodes the record `record`, found intact in `bytes`, all of the file,
/// with when it was written. Failing, the error says who wrote it, by its
/// tag's first 128 characters.
fn decode_record<P: Payload>(bytes: &[u8], record: &Block) -> Result<(P, UnixSeconds)> {
    let (tag, payload) = record.parts(bytes);
    decode(payload).map_err(|error| Error::Decode {
        writer: String::from_utf8_lossy(tag).chars().take(128).collect(),
        error,
    })
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn u128_at(bytes: &[u8], at: usize) -> u128 {
    u128::from_le_bytes(bytes[at..at + 16].try_into().unwrap())
}

/// Encodes `payload` as a record's payload, with the time now. A payload
/// [`decode`] would refuse as too large is [`Error::TooLarge`], rather
/// than a record that saves but won't open.
fn encode(payload: &impl Payload) -> Result<Vec<u8>> {
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
const MAX_EXPANSION: usize = 32;

/// Decodes a record's payload as a record holding a `P`, see
/// [`from_msgpack`], with when it was written.
fn decode<P: Payload>(payload: &[u8]) -> std::result::Result<(P, UnixSeconds), DecodeError> {
    from_msgpack(&decompress(payload)?)
}

/// Decompresses a record's payload, if it claims no more than it can hold.
fn decompress(payload: &[u8]) -> std::result::Result<Vec<u8>, DecodeError> {
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
const MAX_DEPTH: usize = 32;

/// Decodes a record holding a `P` from all of `raw`, as MessagePack, and
/// checks its payload, with when it was written. Bytes left over are an
/// error, see [`codec::whole`].
fn from_msgpack<P: Payload>(raw: &[u8]) -> std::result::Result<(P, UnixSeconds), DecodeError> {
    let unchecked = unchecked_record::<P::Unchecked>(raw)?;
    Ok((
        P::check(unchecked.payload).map_err(Into::into)?,
        unchecked.time,
    ))
}

/// Decodes a record holding a `U` from all of `raw`, as MessagePack,
/// before any check.
fn unchecked_record<U: DeserializeOwned>(
    raw: &[u8],
) -> std::result::Result<UncheckedRecord<U>, DecodeError> {
    let mut de = rmp_serde::Deserializer::new(raw);
    de.set_max_depth(MAX_DEPTH);
    let unchecked =
        UncheckedRecord::<U>::deserialize(&mut de).map_err(|e| DecodeError::new(e.to_string()))?;
    // Reading takes bytes off the front of the slice.
    codec::whole(unchecked, de.get_ref().len())
}

/// All of `file`, once its start shows it's a `.vrdp` file of type `ty`,
/// or one cut short within the file header. Anything else is refused
/// having read no more than the file header, however large it is, and a
/// file too large to hold in memory is an error rather than an abort.
fn read_all(file: &impl ReadAt, ty: FileType) -> Result<Vec<u8>> {
    let read = WholeRead::of_type(buffer_len(file.len()?)?, ty);
    let mut head = vec![0; read.head_len()];
    file.read_at(&mut head, 0)?;
    let mut bytes = read.buffer(head)?;
    // At most `FILE_HEADER_LEN`, so exact.
    let start = read.head_len();
    file.read_at(&mut bytes[start..], start as u64)?;
    Ok(bytes)
}

/// Reading a whole file of `len` bytes in two steps, as [`read_all`] does,
/// for callers that can't go through [`ReadAt`] (the web worker's async
/// `Blob` reads): read [`WholeRead::head_len`] bytes from the start, hand
/// them to [`WholeRead::buffer`], which refuses anything but a design file,
/// then read the rest over the buffer from `head_len` on.
pub(crate) struct WholeRead {
    len: usize,
    ty: FileType,
}

impl WholeRead {
    /// For a design file of `len` bytes.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn new(len: usize) -> Self {
        Self::of_type(len, FileType::Design)
    }

    fn of_type(len: usize, ty: FileType) -> Self {
        WholeRead { len, ty }
    }

    /// How many bytes to read first, from the start: the file header, or
    /// all of a file shorter than it.
    pub(crate) fn head_len(&self) -> usize {
        self.len.min(FILE_HEADER_LEN)
    }

    /// The buffer to read the whole file into, given `head`, its first
    /// [`WholeRead::head_len`] bytes: `head` followed by zeros to read the
    /// rest over, once [`check_file_header`] passes. Reserved fallibly, so
    /// a file too large to hold in memory is an error rather than an
    /// abort.
    ///
    /// Panics if `head` isn't `head_len` bytes: the header check accepts
    /// any prefix of the header, so a shorter one would pass anything.
    pub(crate) fn buffer(&self, head: Vec<u8>) -> Result<Vec<u8>> {
        assert_eq!(head.len(), self.head_len(), "the head of a whole read");
        check_file_header(&head, self.ty)?;
        let mut bytes = zeroed(self.len, "the file is too large to open")?;
        bytes[..head.len()].copy_from_slice(&head);
        Ok(bytes)
    }
}

/// Checks that `head`, the first bytes of a file up to [`FILE_HEADER_LEN`]
/// of them, starts the file header of a file of type `ty`: one of the other
/// type is refused as such, see [`FileType::read_as_other`], one of
/// another version is [`Error::UnsupportedVersion`], anything else
/// [`Error::NotVarde`]. The `id` may be anything.
fn check_file_header(head: &[u8], ty: FileType) -> Result<()> {
    let prefix = &head[..head.len().min(PREFIX_LEN)];
    if ty.prefix().starts_with(prefix) {
        return Ok(());
    }
    if head.starts_with(ty.other().magic()) {
        return Err(ty.other().read_as_other());
    }
    match head.get(25..PREFIX_LEN) {
        Some(version) if head.starts_with(ty.magic()) => Err(Error::UnsupportedVersion(
            u32::from_le_bytes(version.try_into().unwrap()),
        )),
        _ => Err(Error::NotVarde),
    }
}

/// A file length as a buffer length, which on the web's 32 bit wasm is
/// smaller than a file can be.
fn buffer_len(len: u64) -> io::Result<usize> {
    usize::try_from(len).map_err(|_| io::Error::from(io::ErrorKind::FileTooLarge))
}

/// `len` zeros to read into, reserved fallibly, so a file too large to
/// hold in memory is an error, `too_large` and the length, rather than an
/// abort.
fn zeroed(len: usize, too_large: &str) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            format!("{too_large}: {len} bytes"),
        )
    })?;
    bytes.resize(len, 0);
    Ok(bytes)
}

#[cfg(test)]
mod tests;
