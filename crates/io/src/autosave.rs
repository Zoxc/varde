//! What a sidecar or store entry holds, natively and on the web: its
//! records, [`AutoSaved`], each an auto-saved document along with the saved
//! version of the design it was based on, so opening can tell exactly
//! whether the design changed since; the auto-saves as they're [`Held`]
//! through the lock; and what's kept of them as they're let go of, see
//! [`Ending`].
//!
//! An explicit save empties them, unless they hold what a crashed session
//! left that the user hasn't answered the offer of yet. A clean close, once
//! the design is saved or the user chose not to save it, empties them and
//! deletes the file. Anything else, a crash or the lane ending, leaves
//! what's in them for the next editor to recover.
//!
//! Natively the file is a lock file, a design's sidecar or a store entry,
//! see `src/native/sidecar.rs`; on the web a store entry or a sidecar in
//! the Origin Private File System (`src/web/worker/files.rs`,
//! `src/browser.rs`). Only deleting it differs.

use std::{fmt, io};

use serde::{Deserialize, Serialize, Serializer};
use varde_document::{CheckError, DecodeError, MAX_NAME_LEN, Snapshot, Unchecked};

use crate::vrdp::{Error as FileError, HeldFile, Opened, Payload, Storage, Tail};
use crate::{Closing, Damage};

/// What each record of a sidecar or store entry holds: an auto-saved
/// document, and the saved version of the design it was based on.
#[derive(Debug, Serialize)]
pub(crate) struct AutoSaved {
    /// The design's file as the lane last read or wrote it, when this was
    /// auto-saved: `DocumentFile::tail` (`src/native/files/document_file.rs`).
    /// `None` for a new design, which has no file: a store entry's.
    pub(crate) base: Option<Tail>,
    /// The design's file name, on the web, where a store entry holds the
    /// auto-saves of designs opened from the user's files too, so one left
    /// behind is known by it. `None` for a new design, and natively, where
    /// the sidecar sits next to the design.
    pub(crate) name: Option<String>,
    #[serde(serialize_with = "serialize_shared")]
    pub(crate) document: Snapshot,
    pub(crate) origin: Origin,
}

/// What an auto-save holds: the design as edited, or, in entries an older
/// web build left, as it was downloaded, which that build kept past a
/// clean close. Nothing writes `Downloaded` now; it's still read, so those
/// entries are offered back as recovered designs like any other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Origin {
    Edited,
    Downloaded,
}

/// The design in an entry left behind, to open, from `read` of it: its
/// newest intact record, with how reading found the entry damaged, if it
/// did, or why there's none.
pub(crate) fn to_open(
    read: Result<Option<Opened<AutoSaved>>, FileError>,
) -> Result<(AutoSaved, Option<Damage>), String> {
    match read {
        Ok(Some(opened)) => {
            let damage = Damage::of(&opened.report, None);
            Ok((opened.payload, damage))
        }
        Ok(None) => Err("there's nothing in it".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// [`AutoSaved::document`] as the document it shares, the way
/// [`UncheckedAutoSaved`] reads it back, so serde needn't know about `Arc`.
fn serialize_shared<S: Serializer>(document: &Snapshot, s: S) -> Result<S::Ok, S::Error> {
    document.as_ref().serialize(s)
}

/// [`AutoSaved`] as decoded, before [`Payload::check`]: the same fields in
/// the same order.
#[derive(Deserialize)]
pub(crate) struct UncheckedAutoSaved {
    base: Option<Tail>,
    name: Option<String>,
    document: Unchecked,
    origin: Origin,
}

/// Why an [`AutoSaved`] read back from a file fails [`Payload::check`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AutoSavedError {
    /// The design's name is this many bytes, over [`MAX_NAME_LEN`].
    Name(usize),
    /// The document fails its check.
    Document(CheckError),
}

impl fmt::Display for AutoSavedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutoSavedError::Name(len) => write!(
                f,
                "the design's name is {len} bytes, over the limit of {MAX_NAME_LEN}"
            ),
            AutoSavedError::Document(why) => why.fmt(f),
        }
    }
}

impl From<AutoSavedError> for DecodeError {
    fn from(why: AutoSavedError) -> DecodeError {
        match why {
            AutoSavedError::Name(_) => DecodeError::new(why.to_string()),
            AutoSavedError::Document(why) => why.into(),
        }
    }
}

impl Payload for AutoSaved {
    type Unchecked = UncheckedAutoSaved;
    type Error = AutoSavedError;

    /// Also bounds [`AutoSaved::name`], shown on the welcome screen and as
    /// the design's name once opened, as [`Document::check`] bounds body
    /// names: no file name comes near [`MAX_NAME_LEN`], but a store entry
    /// could hold any.
    ///
    /// [`Document::check`]: varde_document::Document::check
    fn check(unchecked: UncheckedAutoSaved) -> Result<AutoSaved, AutoSavedError> {
        let UncheckedAutoSaved {
            base,
            name,
            document,
            origin,
        } = unchecked;
        if let Some(name) = &name
            && name.len() > MAX_NAME_LEN
        {
            return Err(AutoSavedError::Name(name.len()));
        }
        Ok(AutoSaved {
            base,
            name,
            document: Snapshot::new(document.check().map_err(AutoSavedError::Document)?),
            origin,
        })
    }
}

/// The auto-saves in a sidecar or store entry, natively or on the web,
/// held through its lock, and what's kept of them as it's let go of, see
/// [`Ending`]. Deleting it is up to the holder: natively `LockFile`, on
/// the web the store's entry or the sidecar of a design in browser
/// storage.
#[derive(Debug)]
pub(crate) struct Held<S> {
    file: HeldFile<S, AutoSaved>,
}

/// How a sidecar or store entry is let go of: what's kept of what's in
/// it, and so whether it's deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ending {
    /// The clean close: emptied and deleted. What's in it is saved, or the
    /// user chose not to save it.
    Close,
    /// Deleted if there's nothing in it, but anything there is is kept for
    /// the next editor to recover.
    Release,
}

impl Closing {
    /// How a file closed so lets go of its sidecar or store entry.
    pub(crate) fn ending(self) -> Ending {
        match self {
            Closing::Clean => Ending::Close,
            Closing::Keep => Ending::Release,
        }
    }
}

impl<S: Storage> Held<S> {
    /// The auto-saves in `file`, locked.
    pub(crate) fn new(file: S) -> Self {
        Self {
            file: HeldFile::new(file),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn storage(&self) -> &S {
        self.file.storage()
    }

    /// Whether there's nothing in it.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn is_empty(&self) -> io::Result<bool> {
        self.file.is_empty()
    }

    /// The newest auto-save, if there is one.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn read(&mut self) -> Result<Option<AutoSaved>, FileError> {
        self.file.read()
    }

    /// The newest intact auto-save, if there is one, with how reading
    /// found the file, see [`HeldFile::read_with_report`].
    pub(crate) fn read_with_report(&mut self) -> Result<Option<Opened<AutoSaved>>, FileError> {
        self.file.read_with_report()
    }

    /// Auto-saves `document`, based on the design's file at `base`, if it
    /// has one, with the design's file `name` if it's kept with it (see
    /// [`AutoSaved::name`]).
    pub(crate) fn append(
        &mut self,
        base: Option<Tail>,
        name: Option<String>,
        document: &Snapshot,
    ) -> Result<(), FileError> {
        self.file.append(&AutoSaved {
            base,
            name,
            document: Snapshot::clone(document),
            origin: Origin::Edited,
        })
    }

    /// Appends `saved` as it is, as when a design's auto-saves go with it
    /// to another file.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn append_saved(&mut self, saved: &AutoSaved) -> Result<(), FileError> {
        self.file.append(saved)
    }

    /// Empties it, once what's in it is saved or not wanted.
    pub(crate) fn clear(&mut self) -> io::Result<()> {
        self.file.clear()
    }

    /// Keeps what `ending` keeps of it, returning whether it's to be
    /// deleted: then it's emptied already.
    pub(crate) fn end(&mut self, ending: Ending) -> io::Result<bool> {
        match ending {
            Ending::Close => {
                // Emptied first, so it holds nothing to recover should
                // deleting it fail, e.g. on Windows to another editor
                // opening it.
                self.file.clear()?;
                Ok(true)
            }
            Ending::Release => self.file.is_empty(),
        }
    }
}

impl AutoSaved {
    /// Whether this was based on the design whose file ends at `tail`.
    /// Anything else means the design changed since: saved by someone
    /// else, rewritten, or saved by the session that auto-saved this but
    /// kept it, e.g. crashing before emptying the sidecar.
    pub(crate) fn based_on(&self, tail: Tail) -> bool {
        self.base == Some(tail)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
