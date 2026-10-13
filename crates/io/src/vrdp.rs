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
//! it: damaged, missing or stale, they're ignored. `previews` reads
//! them from the end of the file, walking back to the newest record and
//! reading no record's payload. Saves write two, the design's thumbnail
//! as a PNG in each theme's colours (see [`thumbnail`](crate::thumbnail)),
//! when the app rendered it.
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
//! The workers are sent documents as postcard instead (see [`codec`](varde_document::codec)), by
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
//! works on any `Storage`: a [`File`](std::fs::File) natively, an Origin Private File
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

use std::io;

use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::xxh3_128;

use varde_document::codec::DecodeError;
use varde_document::{APP_NAME, Document};

use crate::UnixSeconds;

use chain::Chain;
pub(crate) use check::check_unchanged;
pub(crate) use preview::previews;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) use preview::{FileEnd, end};
pub use preview::{MAX_MEDIA_TYPE, MAX_PREVIEW, MAX_PREVIEWS, Preview};

mod block;
mod chain;
mod check;
mod held;
mod preview;
mod record;
mod storage;

use block::{AUTOSAVE, Kind, PREV_AT, RECORD, SUM_AT, stored_at, u128_at};
pub(crate) use held::HeldFile;
pub(crate) use record::Payload;
use record::record_at;
#[cfg(target_arch = "wasm32")]
pub(crate) use storage::WholeRead;
pub(crate) use storage::{ReadAt, Storage};
use storage::{read_all, write_or_cut_back};

/// Length of `MAGIC`.
const MAGIC_LEN: usize = 25;
/// The `MAGIC` a file starts with, telling its type.
type Magic = [u8; MAGIC_LEN];
/// The start of a design file.
const DESIGN_MAGIC: &Magic =
    b"varde-cad\x7c\x2f\x88\x90\x95\xb8\x39\xf6\xf9\x1d\xa3\x73\x4d\xb9\x90\x04";
/// The start of a held file: an auto-save sidecar or a store entry.
const HELD_MAGIC: &Magic =
    b"varde-cad\x81\x59\xb5\x69\xd9\x5c\xe0\x02\x3f\x4f\xb0\x41\x48\xa4\x7b\xa6";
const VERSION: u32 = 1;
/// Length of `MAGIC` plus `version`: the part of the file header that's
/// the same in every file of a type.
const PREFIX_LEN: usize = MAGIC_LEN + size_of::<u32>();
/// Length of the file header, `MAGIC`, `version` and `id`: the offset of
/// the first block.
pub(crate) const FILE_HEADER_LEN: usize = PREFIX_LEN + 16;

/// Which of the two types of `.vrdp` file a file is, see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileType {
    /// A design's own file.
    Design,
    /// An auto-save sidecar or a store entry, see [`HeldFile`].
    Held,
}

impl FileType {
    fn magic(self) -> &'static Magic {
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
        let (magic, version) = prefix.split_at_mut(MAGIC_LEN);
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

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] io::Error),
    /// The file doesn't start with a `.vrdp` magic number.
    #[error("not a {APP_NAME} file")]
    NotVarde,
    /// The file is an auto-save, opened as a design.
    #[error("an auto-save, not a design")]
    IsAutoSave,
    /// The file is a design, opened as an auto-save.
    #[error("a design, not an auto-save")]
    IsDesign,
    /// The file was written in a format version this build can't read.
    #[error("unsupported file format version {0}")]
    UnsupportedVersion(u32),
    /// The file contains no complete record.
    #[error("file contains no saved document")]
    Empty,
    /// The file is damaged at `offset`: before any intact record when
    /// reading, or, appending to a held file, where its records, none of
    /// them intact, begin to be damaged.
    #[error("file is damaged at byte {offset}")]
    Corrupt { offset: u64 },
    /// The newest record couldn't be decoded, e.g. it was written by an
    /// incompatible build: `writer`, the record's tag, as far as it's
    /// shown.
    #[error("couldn't decode document (saved by {writer}, this is {}): {error}", env!("CARGO_PKG_VERSION"))]
    Decode {
        writer: String,
        #[source]
        error: DecodeError,
    },
    /// The file changed since it was last read or written.
    #[error("file was changed by someone else")]
    Conflict,
    /// The file was damaged since it was last read or written: damage
    /// reading didn't find, or the record it read or wrote last. Saving
    /// is refused, as it could cut off what's still readable.
    #[error("file was damaged since it was opened")]
    Damaged,
    /// The file was read as [`Outcome::Damaged`]: saving to it is refused,
    /// as cutting it short could lose what can still be got out of it, so
    /// it's only saved as another file.
    #[error("file is damaged, so it can only be saved as another file")]
    OpenedDamaged,
    /// The document is too large to save: it would decode to more than a
    /// file may hold, so it couldn't be opened again.
    #[error("document is too large to save")]
    TooLarge,
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
    /// downloads are recorded by (`src/web_support/downloads.rs`).
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn sum(&self) -> u128 {
        self.sum
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
    let Written { bytes, tail, id } = whole(FileType::Design, document, previews)?;
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

/// Bytes to write to a file, with the file's tail and `id` once they are.
struct Written {
    bytes: Vec<u8>,
    tail: Tail,
    id: u128,
}

/// A whole file of type `ty` with a new `id` holding `payload` as its one
/// record, followed by the first [`MAX_PREVIEWS`] of `previews`.
fn whole(ty: FileType, payload: &impl Payload, previews: &[Preview]) -> Result<Written> {
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
    Ok(Written { bytes, tail, id })
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

#[cfg(test)]
mod tests;
