//! The held file type in a file its owner keeps locked: an auto-save
//! sidecar or a store entry, see [`HeldFile`].

use std::fs::File;
use std::io;
use std::marker::PhantomData;

use varde_document::Document;

use super::record::record_at;
use super::storage::{read_all, write_or_cut_back};
use super::{
    Chain, DamagedRecord, Error, FileType, Opened, Outcome, Payload, Result, Storage, Tail,
    Written, whole,
};

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
/// be emptied again. Appends are crash-safe like [`save`](super::save)'s: the last
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
    pub(super) file: S,
    pub(super) held: Held,
    pub(super) payload: PhantomData<fn() -> P>,
}

/// What a [`HeldFile`] is known to hold.
#[derive(Debug, Clone, Copy)]
pub(super) enum Held {
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

    /// Reads the newest record, as [`from_bytes_with_report`](super::from_bytes_with_report) does with a
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
        let (start, Written { bytes, tail, id }) = match self.held {
            Held::Damaged { offset } => return Err(Error::Corrupt { offset }),
            Held::At { id, tail, damaged } => {
                let (at, prev) =
                    damaged.map_or((tail.end, tail.sum), |damaged| (damaged.end, damaged.sum));
                let (bytes, tail) = record_at(id, prev, at, FileType::Held, payload)?;
                (at, Written { bytes, tail, id })
            }
            // Held files never get previews.
            Held::Empty | Held::Unknown => (0, whole(FileType::Held, payload, &[])?),
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
}
