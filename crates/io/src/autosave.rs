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
//! deletes the file; on the web a store entry holding the design as
//! downloaded is kept instead, see [`keep_downloaded`]. Anything else, a
//! crash or the lane ending, leaves what's in them for the next editor to
//! recover.
//!
//! Natively the file is a lock file, a design's sidecar or a store entry,
//! see `src/native/sidecar.rs`; on the web a store entry in the Origin Private
//! File System (`src/web/worker/files.rs`). Only deleting it differs.

use std::{fmt, io};

use serde::{Deserialize, Serialize, Serializer};
use varde_document::{CheckError, DecodeError, MAX_NAME_LEN, Snapshot, Unchecked};

use crate::Closing;
use crate::vrdp::{Error as FileError, HeldFile, Payload, Storage, Tail};

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

/// What an auto-save holds: the design as edited, or as it was just
/// downloaded, on the web without the File System Access API, see
/// [`Request::KeepDownload`](crate::Request::KeepDownload). The page isn't
/// told whether the download was kept, so an entry holding a download
/// outlives a clean close, going back to it (see [`keep_downloaded`]), and
/// is listed apart from designs never saved while it's the newest record.
/// Natively always [`Origin::Edited`].
///
/// Encoded as the `bool` it replaced was, `false` and `true`: postcard
/// writes a unit variant as its index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Origin {
    Edited,
    Downloaded,
}

impl Origin {
    /// Whether it's the design as downloaded.
    pub(crate) fn is_download(self) -> bool {
        self == Origin::Downloaded
    }
}

/// The design in an entry left behind, to open, from `read` of it: its
/// newest record, or why there's none.
pub(crate) fn to_open(read: Result<Option<AutoSaved>, FileError>) -> Result<AutoSaved, String> {
    match read {
        Ok(Some(saved)) => Ok(saved),
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
/// the web the store's entry.
#[derive(Debug)]
pub(crate) struct Held<S> {
    file: HeldFile<S, AutoSaved>,
    /// Whether it may hold the design as downloaded, which the clean close
    /// goes back to rather than deleting it, see [`keep_downloaded`]: one
    /// was auto-saved to it, or it's a store entry left behind, opened
    /// again. Otherwise the clean close doesn't read it.
    downloads: bool,
}

/// How a sidecar or store entry is let go of: what's kept of what's in
/// it, and so whether it's deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ending {
    /// The clean close: emptied and deleted. What's in it is saved, or the
    /// user chose not to save it.
    #[cfg(not(target_arch = "wasm32"))]
    Close,
    /// The clean close of a store entry, except that a design as
    /// downloaded in it is kept, see [`keep_downloaded`]: then it's only
    /// let go of.
    CloseButDownloaded,
    /// The user discarding a store entry from the welcome screen: deleted,
    /// unless changes never saved in it are on top of the design as
    /// downloaded, which it goes back to instead, see
    /// [`keep_download_under`]: then it's only let go of.
    Discard,
    /// Deleted if there's nothing in it, but anything there is is kept for
    /// the next editor to recover.
    Release,
}

impl Closing {
    /// How a file closed so lets go of its sidecar or store entry, `clean`
    /// being how the clean close does for it.
    pub(crate) fn ending(self, clean: Ending) -> Ending {
        match self {
            Closing::Clean => clean,
            Closing::Keep => Ending::Release,
        }
    }
}

impl<S: Storage> Held<S> {
    /// The auto-saves in `file`, locked.
    pub(crate) fn new(file: S) -> Self {
        Self {
            file: HeldFile::new(file),
            downloads: false,
        }
    }

    /// A store entry left behind, opened again: it may hold the design as
    /// downloaded, see [`Held::downloads`].
    pub(crate) fn left_behind(self) -> Self {
        Self {
            downloads: true,
            ..self
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn storage(&self) -> &S {
        self.file.storage()
    }

    /// The newest auto-save, if there is one.
    pub(crate) fn read(&mut self) -> Result<Option<AutoSaved>, FileError> {
        self.file.read()
    }

    /// Auto-saves `document`, based on the design's file at `base`, if it
    /// has one, with the design's file `name` if it's kept with it (see
    /// [`AutoSaved::name`]), holding it as `origin` says.
    pub(crate) fn append(
        &mut self,
        base: Option<Tail>,
        name: Option<String>,
        document: &Snapshot,
        origin: Origin,
    ) -> Result<(), FileError> {
        // Even should appending fail: it may have been written.
        self.downloads |= origin.is_download();
        self.file.append(&AutoSaved {
            base,
            name,
            document: Snapshot::clone(document),
            origin,
        })
    }

    /// Empties it, once what's in it is saved or not wanted.
    pub(crate) fn clear(&mut self) -> io::Result<()> {
        self.file.clear()
    }

    /// Keeps what `ending` keeps of it, returning whether it's to be
    /// deleted: then it's emptied already.
    pub(crate) fn end(&mut self, ending: Ending) -> io::Result<bool> {
        let keep = match ending {
            #[cfg(not(target_arch = "wasm32"))]
            Ending::Close => false,
            Ending::CloseButDownloaded => self.downloads && keep_downloaded(&mut self.file),
            Ending::Discard => keep_download_under(&mut self.file),
            Ending::Release => return self.file.is_empty(),
        };
        if keep {
            return Ok(false);
        }
        // Emptied first, so it holds nothing to recover should deleting it
        // fail, e.g. on Windows to another editor opening it.
        self.file.clear()?;
        Ok(true)
    }
}

/// For a clean close of a sidecar or store entry that holds the design as
/// downloaded, see [`Origin`]: rolls it back to the newest
/// such record, dropping the auto-saves of later edits, which the design is
/// saved without or the user chose not to save, and returns whether it did.
/// The download may not have been kept, so that stays. Without one, or if
/// what's in it can't be read or rolled back, it's up to the caller to
/// empty and delete it as usual. Natively nothing is marked.
fn keep_downloaded<S: Storage>(file: &mut HeldFile<S, AutoSaved>) -> bool {
    matches!(
        file.roll_back(|saved| saved.origin.is_download()),
        Ok(Some(_))
    )
}

/// For the user discarding a store entry listed on the welcome screen:
/// changes never saved in it that are on top of the design as downloaded
/// are gone back from, as by the clean close (see [`keep_downloaded`]),
/// rather than taking the download with them, which may be the only copy,
/// returning whether it did. The design as downloaded is discarded as
/// listed, as the newest record.
fn keep_download_under<S: Storage>(file: &mut HeldFile<S, AutoSaved>) -> bool {
    match file.read() {
        Ok(Some(newest)) if newest.origin.is_download() => false,
        _ => keep_downloaded(file),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
