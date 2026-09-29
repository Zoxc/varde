//! The `.vrdp` file format: an append-only log of document snapshots.
//!
//! ```text
//! file   = MAGIC | version: u32 LE | record*
//! record = len: u32 LE | crc: u32 LE | payload: [u8; len] | FOOTER
//! ```
//!
//! `payload` is a postcard-encoded [`Document`] (see [`codec`]), compressed
//! with raw snappy, and `crc` is the CRC-32 of `payload`. Every save appends
//! one record, so the file holds every saved version and the last valid
//! record is the current one.
//!
//! Saves are atomic because existing bytes are never rewritten: a crash while
//! appending leaves a torn record at the end, which readers ignore and the next
//! save truncates. Before appending, a save checks the file still ends with the
//! record it last read or wrote, so saving over changes made by someone else is
//! refused.
//!
//! `read`, `write_new` and `save` do this on any `Storage`. Only the
//! format is here: the native lane (`src/native/files/document_file.rs`) opens and
//! locks a design's file around each of them, and replaces a file by
//! renaming a new one over it.
//!
//! `HeldFile` is the same format in a file its owner keeps locked, like an
//! auto-save sidecar: no lock per operation, and it may be empty. It works
//! on any `Storage`: a [`File`] natively, an Origin Private File System
//! handle in the web build's IO worker. Its records may hold another
//! `Payload` than a plain [`Document`], e.g. an auto-save along with the
//! [`Tail`] of the design it was based on.
//!
//! [`to_bytes`], [`from_bytes`] and `check_unchanged` do the same for a
//! whole file in memory, for the web build, which can only read and replace
//! files of the user's whole. The check before a save is the same code
//! either way, reading through `ReadAt`. Outside this crate, only the
//! whole-file [`to_bytes`] and [`from_bytes`] are public, with [`Error`].
//!
//! `MAGIC` is `varde-cad` followed by a random u128, so it won't collide with
//! other formats. `FOOTER` is another random u128 closing every record, so a
//! file that ends with it ends with a complete record. `version` stays 1
//! while the app is WIP, see `AGENTS.md`.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use varde_document::codec::{self, DecodeError};
use varde_document::{APP_NAME, CheckError, Document, Unchecked};

const MAGIC: &[u8; 25] =
    b"varde-cad\x7c\x2f\x88\x90\x95\xb8\x39\xf6\xf9\x1d\xa3\x73\x4d\xb9\x90\x04";
const FOOTER: &[u8; 16] = b"\xcb\x18\x2a\xf4\xf0\xcd\xd8\x95\xad\x3a\x78\x9e\xa9\x85\x4a\x22";
const VERSION: u32 = 1;
/// Length of `MAGIC` plus `version`, i.e. the offset of the first record.
const FILE_HEADER_LEN: usize = MAGIC.len() + 4;
/// `MAGIC` and `version`, which every file starts with.
const FILE_HEADER: [u8; FILE_HEADER_LEN] = {
    let mut header = [0; FILE_HEADER_LEN];
    let (magic, version) = header.split_at_mut(MAGIC.len());
    magic.copy_from_slice(MAGIC);
    version.copy_from_slice(&VERSION.to_le_bytes());
    header
};
/// Length of a record header.
const HEADER_LEN: u64 = 8;
/// Bytes a record adds around its payload.
const RECORD_OVERHEAD: u64 = HEADER_LEN + FOOTER.len() as u64;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The file doesn't start with the `.vrdp` magic number.
    NotVarde,
    /// The file was written in a format version this build can't read.
    UnsupportedVersion(u32),
    /// The file contains no complete record.
    Empty,
    /// A record in the middle of the file is damaged.
    Corrupt {
        offset: u64,
    },
    /// The newest record couldn't be decoded, e.g. it was written by an
    /// incompatible build.
    Decode(DecodeError),
    /// The file changed since it was last read or written.
    Conflict,
    /// The document is too large to save: it would decode to more than a
    /// file may hold, so it couldn't be opened again.
    TooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::NotVarde => write!(f, "not a {APP_NAME} file"),
            Error::UnsupportedVersion(v) => write!(f, "unsupported file format version {v}"),
            Error::Empty => f.write_str("file contains no saved document"),
            Error::Corrupt { offset } => write!(f, "file is damaged at byte {offset}"),
            Error::Decode(e) => write!(f, "couldn't decode document: {e}"),
            Error::Conflict => f.write_str("file was changed by someone else"),
            Error::TooLarge => f.write_str("document is too large to save"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Decode(e) => Some(e),
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

/// Where the valid part of a file ends, as last read or written by us.
///
/// It also tells which saved version the file holds: another save, by
/// anyone, or the file rewritten with other content, gives it another tail
/// (short of a CRC-32 collision at the same offsets). The IO lane hands out
/// its design file's to be stored, e.g. with an auto-save, and compared
/// later; one read back from a file is only ever compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tail {
    /// Offset of the last record.
    last: u64,
    /// Checksum of the last record, to tell it apart from a replacement.
    crc: u32,
    /// Offset just past the last record.
    end: u64,
}

/// Reads the newest document of the `.vrdp` file `file`, with the file's
/// tail. A torn record at the end is ignored; damage before the newest
/// record is [`Error::Corrupt`]. Anything but a `.vrdp` is refused having
/// read no more than the file header.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn read(file: &impl ReadAt) -> Result<(Document, Tail)> {
    from_bytes(&read_all(file)?)
}

/// Writes `document` to the empty file `file`, making it a `.vrdp` holding
/// it as its one record, and returns the file's tail. Syncing is up to the
/// caller.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn write_new(file: &mut impl Storage, document: &Document) -> Result<Tail> {
    append(file, 0, document)
}

/// Appends `document` to the `.vrdp` file `file` as a new version, and
/// returns the file's new tail. The file must still be the saved version
/// `tail`, as last read or written: someone else's change is
/// [`Error::Conflict`], see [`check_unchanged`], the same check. A torn
/// append after it is dropped.
///
/// The record is synced before this returns. Should that fail, the file is
/// cut back to `tail`: the record may not be on disk, and syncing a later
/// one wouldn't put it there, so the next save writes it anew rather than
/// finding a newer version and calling it a conflict.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn save(file: &mut impl Storage, tail: Tail, document: &Document) -> Result<Tail> {
    check_unchanged(file, tail)?;
    let new = append(file, tail.end, document)?;
    if let Err(error) = file.sync() {
        let _ = file.truncate(tail.end);
        return Err(error.into());
    }
    Ok(new)
}

/// A whole `.vrdp` file holding `document` as its one record, and the
/// file's tail: what the web build writes to a file of the user's, which it
/// can only replace, not append to (see `src/pick.rs`).
pub fn to_bytes(document: &Document) -> Result<(Vec<u8>, Tail)> {
    whole(document)
}

/// Reads the newest document of the whole `.vrdp` file `bytes`, with the
/// file's tail, as `read` does with the file it reads.
pub fn from_bytes(bytes: &[u8]) -> Result<(Document, Tail)> {
    let tail = Records::scan(bytes, false)?.last()?.ok_or(Error::Empty)?;
    let document = decode(record_payload(bytes, tail))?;
    Ok((document, tail))
}

/// Checks that `file` still ends with the saved version `tail`, as last
/// read or written, before a save appends to it or replaces it: [`save`]
/// makes this check, and the web build makes it on a whole file in memory.
/// Its last record must still be there, unchanged, and anything after it
/// be a torn append: a file shorter than `tail`, another record where its
/// last one was, or a newer record after it, is someone else's change,
/// [`Error::Conflict`]. That record damaged, or damage after it, is
/// [`Error::Corrupt`], as appending after it would leave the file
/// unreadable. Only that record and the one after it are read: not what
/// comes before, nor the rest of whatever someone appended.
pub(crate) fn check_unchanged(file: &(impl ReadAt + ?Sized), tail: Tail) -> Result<()> {
    let len = file.len()?;
    if len < tail.end {
        return Err(Error::Conflict);
    }
    let mut header = [0; HEADER_LEN as usize];
    file.read_at(&mut header, tail.last)?;
    let (payload_len, crc) = parse_header(&header);
    let end = tail.last.checked_add(RECORD_OVERHEAD + payload_len);
    if end != Some(tail.end) || crc != tail.crc {
        return Err(Error::Conflict);
    }
    // Read as if the file ended with it, which it spans: valid or damaged.
    if !matches!(record_at(file, tail.last, tail.end)?, Record::Valid { .. }) {
        return Err(Error::Corrupt { offset: tail.last });
    }
    if len > tail.end {
        match record_at(file, tail.end, len)? {
            Record::Valid { .. } => return Err(Error::Conflict),
            Record::Torn => {}
            Record::Corrupt => return Err(Error::Corrupt { offset: tail.end }),
        }
    }
    Ok(())
}

/// A `.vrdp` file its owner holds open and keeps other writers away from,
/// like an auto-save sidecar, which the IO lane holds an OS lock on for as
/// long as its design is open, or, on the web, a store entry held through
/// the Origin Private File System's exclusive sync access handle.
///
/// Unlike a design's file, which the IO lane locks for each operation, it
/// needs no lock of its own: the owner's is enough, and one of its own
/// would be refused as held by someone else (`flock` locks per open file
/// description, `LockFileEx` per handle). Nor does it check for changes
/// made by others. It may be empty, which holds no document yet, and can
/// be emptied again. Appends are crash-safe like [`save`]'s: the last
/// complete record stays readable.
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
    /// No record: nothing at all, the file header, or a torn first record.
    Empty,
    /// Records, the last one ending at `end`.
    At(Tail),
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

    /// Reads the newest record, or `None` if there is none yet. A torn
    /// record at the end is ignored, like [`read`] does, and
    /// so is a file header cut short, as left by a crash while the first
    /// record was written.
    pub(crate) fn read(&mut self) -> Result<Option<P>> {
        self.held = Held::Unknown;
        let bytes = read_all(&self.file)?;
        match Records::scan(&bytes, true)?.last()? {
            None => {
                self.held = Held::Empty;
                Ok(None)
            }
            Some(tail) => {
                let payload = decode(record_payload(&bytes, tail))?;
                self.held = Held::At(tail);
                Ok(Some(payload))
            }
        }
    }

    /// Makes the newest record that `keep` accepts the newest again,
    /// dropping the records after it, and a torn one, and returns it. If
    /// `keep` accepts none, the file is left as it is and it's `None`.
    /// Records that can't be read aren't accepted: ones that don't decode,
    /// and from a corrupt one on, which going back to one before drops too,
    /// rather than keeping the records before it from being gone back to.
    /// Crash-safe like [`HeldFile::append`]: cutting the file short leaves
    /// records whole.
    pub(crate) fn roll_back(&mut self, keep: impl Fn(&P) -> bool) -> Result<Option<P>> {
        self.held = Held::Unknown;
        let bytes = read_all(&self.file)?;
        let tails = Records::scan(&bytes, true)?.tails;
        let found = tails.into_iter().rev().find_map(|tail| {
            let payload = decode(record_payload(&bytes, tail)).ok()?;
            keep(&payload).then_some((tail, payload))
        });
        let Some((tail, payload)) = found else {
            return Ok(None);
        };
        // Within the file: the scan found the record in it.
        self.file.truncate(tail.end)?;
        self.file.sync()?;
        self.held = Held::At(tail);
        Ok(Some(payload))
    }

    /// Appends `payload` as the newest record. Whatever the file holds
    /// that can't be read, it starts over from: it's of no use to anyone.
    pub(crate) fn append(&mut self, payload: &P) -> Result<()> {
        if let Held::Unknown = self.held
            && self.read().is_err()
        {
            self.held = Held::Empty;
        }
        let start = match self.held {
            Held::At(tail) => tail.end,
            Held::Empty | Held::Unknown => 0,
        };
        // Should writing fail, the file is read again before the next one.
        self.held = Held::Unknown;
        let tail = append(&mut self.file, start, payload)?;
        self.file.sync()?;
        self.held = Held::At(tail);
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

/// What a record holds, postcard-encoded: a [`Document`] in a design's
/// file, or what the owner of a [`HeldFile`] keeps in it.
pub(crate) trait Payload: Serialize + Sized {
    /// The payload as decoded from a file, before [`Payload::check`]: the
    /// same fields in the same order, holding an [`Unchecked`] where the
    /// payload holds a [`Document`], so the message of a document failing
    /// its check isn't lost in decoding.
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

enum Record {
    Valid {
        crc: u32,
        end: usize,
    },
    /// Cut off by the end of the file, as left by an interrupted append.
    Torn,
    /// Complete but with a bad checksum or footer.
    Corrupt,
}

fn next_record(bytes: &[u8], at: usize) -> Record {
    let Some(start) = at.checked_add(HEADER_LEN as usize) else {
        return Record::Torn;
    };
    let Some(header) = bytes.get(at..start) else {
        return Record::Torn;
    };
    let (len, crc) = parse_header(header.try_into().unwrap());
    let Some(payload) = usize::try_from(len)
        .ok()
        .and_then(|len| bytes.get(start..start.checked_add(len)?))
    else {
        return Record::Torn;
    };
    let footer_start = start + payload.len(); // In bounds, as `payload` is a subslice.
    let Some(footer) = bytes.get(footer_start..footer_start.saturating_add(FOOTER.len())) else {
        return Record::Torn;
    };
    // A crash can extend the file before the record is fully written, leaving
    // zeros at the end. That's torn rather than corrupt if nothing follows,
    // or if it's all zeros: with a zeroed header, the record reads as empty
    // and ends early. A real record is never all zeros, as `FOOTER` isn't.
    let end = footer_start + footer.len();
    match (
        checksum(payload) == crc && footer == FOOTER,
        end == bytes.len() || bytes[at..].iter().all(|&b| b == 0),
    ) {
        (true, _) => Record::Valid { crc, end },
        (false, true) => Record::Torn,
        (false, false) => Record::Corrupt,
    }
}

/// The record at `at` in `file`, which is `len` bytes long, as
/// [`next_record`] finds it in all the bytes from `at` on, but reading
/// only that record, if the file holds all of it, and not the rest of a
/// file someone may have appended any amount to. Only a complete record of
/// zeros, which may be the start of a crash's zeros, has the rest read,
/// piece by piece, to tell whether it's all zeros.
fn record_at(file: &(impl ReadAt + ?Sized), at: u64, len: u64) -> Result<Record> {
    let rest = len
        .checked_sub(at)
        .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
    if rest < HEADER_LEN {
        return Ok(Record::Torn);
    }
    let mut header = [0; HEADER_LEN as usize];
    file.read_at(&mut header, at)?;
    // At most `u32::MAX` plus the overhead, so exact.
    let record_len = RECORD_OVERHEAD + parse_header(&header).0;
    if rest < record_len {
        return Ok(Record::Torn);
    }
    let size = buffer_len(record_len)?;
    let mut record = Vec::new();
    record.try_reserve_exact(size).map_err(|_| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            format!("the record is too large to read: {record_len} bytes"),
        )
    })?;
    record.resize(size, 0);
    file.read_at(&mut record, at)?;
    // Ends where `record` does, so valid or torn.
    let found = next_record(&record, 0);
    if matches!(found, Record::Valid { .. }) || rest == record_len {
        return Ok(found);
    }
    if record.iter().any(|&b| b != 0) {
        return Ok(Record::Corrupt);
    }
    let mut chunk = vec![0; 1 << 16];
    let mut from = at + record_len;
    while from < len {
        // At most the chunk's length, so exact.
        let n = (len - from).min(chunk.len() as u64) as usize;
        file.read_at(&mut chunk[..n], from)?;
        if chunk[..n].iter().any(|&b| b != 0) {
            return Ok(Record::Corrupt);
        }
        from += n as u64;
    }
    Ok(Record::Torn)
}

/// Writes `payload` as a record at `at`, the end of the last valid one,
/// dropping whatever follows: a torn record, or anything else. At 0 the
/// file header comes first, making an empty file a `.vrdp`. Returns the
/// file's new tail; syncing is up to the caller.
fn append(file: &mut impl Storage, at: u64, payload: &impl Payload) -> Result<Tail> {
    let (bytes, tail) = if at == 0 {
        whole(payload)?
    } else {
        let (record, crc) = encode(payload)?;
        let end = at
            .checked_add(record.len() as u64)
            .ok_or_else(|| io::Error::from(io::ErrorKind::FileTooLarge))?;
        (record, Tail { last: at, crc, end })
    };
    file.truncate(at)?;
    file.write_at(&bytes, at)?;
    Ok(tail)
}

/// A whole file holding `payload` as its one record, and its tail.
fn whole(payload: &impl Payload) -> Result<(Vec<u8>, Tail)> {
    let (record, crc) = encode(payload)?;
    let mut bytes = FILE_HEADER.to_vec();
    bytes.extend_from_slice(&record);
    // `usize` is at most 64 bits wide, so these are exact.
    let tail = Tail {
        last: FILE_HEADER_LEN as u64,
        crc,
        end: bytes.len() as u64,
    };
    Ok((bytes, tail))
}

/// The records of a whole file: the valid ones, oldest first, each as the
/// tail of the file cut short right after it, ignoring a torn one at the
/// end, and where a corrupt one stopped the scan, if one did.
struct Records {
    tails: Vec<Tail>,
    corrupt: Option<u64>,
}

impl Records {
    /// Scans the whole file `bytes`. With `held`, a file shorter than the
    /// file header is a held file with no records yet, as left by a crash
    /// while the first record was written, rather than [`Error::NotVarde`].
    fn scan(bytes: &[u8], held: bool) -> Result<Records> {
        let mut records = Records {
            tails: Vec::new(),
            corrupt: None,
        };
        if held && bytes.len() < FILE_HEADER_LEN {
            check_file_header(bytes)?;
            return Ok(records);
        }
        check_file_header(bytes.get(..FILE_HEADER_LEN).ok_or(Error::NotVarde)?)?;
        let mut at = FILE_HEADER_LEN;
        while at < bytes.len() {
            match next_record(bytes, at) {
                Record::Valid { crc, end } => {
                    records.tails.push(Tail {
                        last: at as u64,
                        crc,
                        end: end as u64,
                    });
                    at = end;
                }
                Record::Torn => break,
                Record::Corrupt => {
                    records.corrupt = Some(at as u64);
                    break;
                }
            }
        }
        Ok(records)
    }

    /// The valid records, unless a corrupt one stopped the scan: then
    /// that's the error.
    fn undamaged(self) -> Result<Vec<Tail>> {
        match self.corrupt {
            Some(offset) => Err(Error::Corrupt { offset }),
            None => Ok(self.tails),
        }
    }

    /// The newest valid record, if there is one and no corrupt one.
    fn last(self) -> Result<Option<Tail>> {
        Ok(self.undamaged()?.last().copied())
    }
}

/// The payload of the last record of `tail`, which [`Records::scan`] has
/// already validated.
fn record_payload(bytes: &[u8], tail: Tail) -> &[u8] {
    let record = &bytes[tail.last as usize..tail.end as usize];
    &record[HEADER_LEN as usize..record.len() - FOOTER.len()]
}

fn parse_header(header: &[u8; HEADER_LEN as usize]) -> (u64, u32) {
    let len = u32::from_le_bytes(header[..4].try_into().unwrap());
    let crc = u32::from_le_bytes(header[4..].try_into().unwrap());
    (len.into(), crc)
}

/// Encodes `payload` as a complete record, returning it and its checksum.
/// A payload [`decode`] would refuse as too large is [`Error::TooLarge`],
/// rather than a record that saves but won't open.
fn encode(payload: &impl Payload) -> Result<(Vec<u8>, u32)> {
    let raw = postcard::to_stdvec(payload).expect("payloads always serialize");
    if raw.len() > MAX_DECOMPRESSED {
        return Err(Error::TooLarge);
    }
    let payload = snap::raw::Encoder::new()
        .compress_vec(&raw)
        .expect("snappy input is within size limits");
    // Snappy output is at most about 7/6 of its input, so 1 GiB fits.
    let len = u32::try_from(payload.len()).expect("payload under MAX_DECOMPRESSED");
    let crc = checksum(&payload);
    let mut record = Vec::with_capacity(payload.len() + RECORD_OVERHEAD as usize);
    record.extend_from_slice(&len.to_le_bytes());
    record.extend_from_slice(&crc.to_le_bytes());
    record.extend_from_slice(&payload);
    record.extend_from_slice(FOOTER);
    Ok((record, crc))
}

/// How many times its compressed length a payload may decompress to.
/// Snappy's densest element, a 3 byte copy of 64 bytes, expands about 21
/// times, so a payload claiming more is lying.
const MAX_EXPANSION: usize = 32;

/// The most any payload may decompress to. Documents are nowhere near it;
/// the bound keeps a file from having the decoder allocate gigabytes.
/// [`encode`] refuses anything larger, so what's saved can be opened.
const MAX_DECOMPRESSED: usize = 1 << 30;

fn decode<P: Payload>(payload: &[u8]) -> Result<P> {
    // The decoder allocates what the payload claims up front, and the claim
    // is file data: up to 4 GiB.
    let claimed = snap::raw::decompress_len(payload)
        .map_err(|e| Error::Decode(DecodeError::new(e.to_string())))?;
    let bound = payload
        .len()
        .saturating_mul(MAX_EXPANSION)
        .min(MAX_DECOMPRESSED);
    if claimed > bound {
        return Err(Error::Decode(DecodeError::new(format!(
            "the document claims {claimed} bytes, more than {} compressed bytes can hold",
            payload.len()
        ))));
    }
    let raw = snap::raw::Decoder::new()
        .decompress_vec(payload)
        .map_err(|e| Error::Decode(DecodeError::new(e.to_string())))?;
    // As `Document::from_postcard`, for any payload.
    codec::from_postcard(&raw, P::check).map_err(Error::Decode)
}

fn checksum(bytes: &[u8]) -> u32 {
    crc32fast::hash(bytes)
}

/// All of `file`, once its start shows it's a `.vrdp` file, or one cut
/// short within the file header. Anything else is refused having read no
/// more than the file header, however large it is, and a file too large to
/// hold in memory is an error rather than an abort.
fn read_all(file: &impl ReadAt) -> Result<Vec<u8>> {
    let read = WholeRead::new(buffer_len(file.len()?)?);
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
/// them to [`WholeRead::buffer`], which refuses anything but a `.vrdp`
/// file, then read the rest over the buffer from `head_len` on.
pub(crate) struct WholeRead {
    len: usize,
}

impl WholeRead {
    pub(crate) fn new(len: usize) -> Self {
        WholeRead { len }
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
    pub(crate) fn buffer(&self, mut head: Vec<u8>) -> Result<Vec<u8>> {
        assert_eq!(head.len(), self.head_len(), "the head of a whole read");
        check_file_header(&head)?;
        let len = self.len;
        head.try_reserve_exact(len - head.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                format!("the file is too large to open: {len} bytes"),
            )
        })?;
        head.resize(len, 0);
        Ok(head)
    }
}

/// Checks that `head`, the first bytes of a file up to [`FILE_HEADER_LEN`]
/// of them, starts the file header: [`Error::NotVarde`] if not, or
/// [`Error::UnsupportedVersion`] if only the version differs.
fn check_file_header(head: &[u8]) -> Result<()> {
    if FILE_HEADER.starts_with(head) {
        return Ok(());
    }
    match head.get(MAGIC.len()..FILE_HEADER_LEN) {
        Some(version) if head.starts_with(MAGIC) => Err(Error::UnsupportedVersion(
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

#[cfg(test)]
mod tests;
