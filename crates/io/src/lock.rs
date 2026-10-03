//! A design's lock, which its auto-saves go to while it's editable, and
//! what it offers back of a session that ended without closing it: the
//! same natively, where it's the design's sidecar next to it (see
//! `src/native/sidecar.rs`), and on the web for a design in browser
//! storage, whose sidecar sits next to it there (see `src/browser.rs`).

use std::io;

use varde_document::{Document, Snapshot};

use crate::autosave::{AutoSaved, Held};
use crate::vrdp::{Error as FileError, Opened, Storage, Tail};
use crate::{Access, Damage, Offer, ReadOnly, RecoveryError};

/// What a [`Lock`] holds: the auto-saves of a design.
pub(crate) trait AutoSaves {
    /// The newest intact auto-save, if there is one, with how reading
    /// found the file.
    fn read_with_report(&mut self) -> Result<Option<Opened<AutoSaved>>, FileError>;

    /// Empties it.
    fn clear(&mut self) -> io::Result<()>;
}

impl<S: Storage> AutoSaves for Held<S> {
    fn read_with_report(&mut self) -> Result<Option<Opened<AutoSaved>>, FileError> {
        Held::read_with_report(self)
    }

    fn clear(&mut self) -> io::Result<()> {
        Held::clear(self)
    }
}

/// Why a document can't be saved or auto-saved to.
pub(crate) const READ_ONLY: &str = "the design is read-only";

/// A design's lock, which auto-saves go to if it's editable.
#[derive(Debug)]
pub(crate) enum Lock<H> {
    /// The design's sidecar, held.
    Sidecar {
        sidecar: H,
        /// Whether it holds what a crashed session left, offered to the
        /// user as [`Opened::recovered`](crate::Opened::recovered) and not
        /// answered yet: until it's discarded or auto-saved over, saves
        /// leave it be. So is one whose records are all damaged, or that
        /// couldn't be read, which the offer says why.
        offered: bool,
        /// Whether it's offered so, as one that can't be read
        /// ([`RecoveryError::kept`]): auto-saves are refused till it's
        /// discarded.
        kept: bool,
    },
    /// Not held: the document is read-only.
    ReadOnly(ReadOnly),
}

impl<H: AutoSaves> Lock<H> {
    /// The lock `taken`, or why the design is read-only.
    pub(crate) fn new(taken: Result<H, ReadOnly>) -> Self {
        match taken {
            Ok(sidecar) => Lock::Sidecar {
                sidecar,
                offered: false,
                kept: false,
            },
            Err(read_only) => Lock::ReadOnly(read_only),
        }
    }

    /// What a crashed session left in the sidecar of the design just
    /// opened holding `document`, its file ending at `tail`, to offer the
    /// user, or why it couldn't be read. `based_past` tells whether an
    /// auto-save based on a save of the design was based on a newer one
    /// than the one opened, which couldn't be read (see
    /// [`vrdp::based_past`](crate::vrdp::based_past)). Marks the lock
    /// `offered` if there's an offer, and `kept` too if it's kept as it
    /// can't be read.
    pub(crate) fn offer(
        &mut self,
        tail: Tail,
        based_past: impl FnOnce(Tail) -> bool,
        document: &Document,
    ) -> Result<Option<Offer>, RecoveryError> {
        // Only the editor holding the lock may look: otherwise the sidecar
        // is another editor's, auto-saving as it goes.
        let Lock::Sidecar {
            sidecar,
            offered,
            kept,
        } = self
        else {
            return Ok(None);
        };
        (*offered, *kept) = (false, false);
        match sidecar.read_with_report() {
            Ok(Some(read)) if *read.payload.document != *document => {
                *offered = true;
                let recovered = read.payload;
                let newer_base = recovered
                    .base
                    .is_some_and(|base| base != tail && based_past(base));
                Ok(Some(Offer {
                    design_changed: !recovered.based_on(tail),
                    document: Snapshot::unwrap_or_clone(recovered.document),
                    damage: Damage::of(&read.report, None),
                    newer_base,
                }))
            }
            Ok(_) => Ok(None),
            Err(error) => {
                // What may yet be got out of it, damaged records or one that
                // couldn't be read, stays until the user answers, as an
                // offer does: auto-saves are refused rather than start it
                // over, nor do saves empty it. What can't ever be read,
                // auto-saving starts over.
                let unreadable = matches!(error, FileError::Corrupt { .. } | FileError::Io(_));
                (*offered, *kept) = (unreadable, unreadable);
                let message = match error {
                    FileError::Corrupt { .. } => {
                        "what was auto-saved is damaged and can't be read".to_owned()
                    }
                    error => format!("couldn't read what was auto-saved: {error}"),
                };
                Err(RecoveryError {
                    message,
                    kept: unreadable,
                })
            }
        }
    }

    /// Whether it holds what a crashed session left that can't be read,
    /// kept till the user discards it, see [`RecoveryError::kept`].
    pub(crate) fn kept(&self) -> bool {
        matches!(self, Lock::Sidecar { kept: true, .. })
    }

    pub(crate) fn access(&self) -> Access {
        match self {
            Lock::Sidecar { .. } => Access::Edit,
            Lock::ReadOnly(read_only) => Access::ReadOnly(read_only.clone()),
        }
    }

    /// The sidecar, if it's held.
    pub(crate) fn held(&mut self) -> Option<&mut H> {
        match self {
            Lock::Sidecar { sidecar, .. } => Some(sidecar),
            Lock::ReadOnly(_) => None,
        }
    }

    /// Whether it's held: the design may be saved and auto-saved.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn is_held(&self) -> bool {
        matches!(self, Lock::Sidecar { .. })
    }

    pub(crate) fn offered(&self) -> bool {
        matches!(self, Lock::Sidecar { offered: true, .. })
    }

    /// The user answered the offer of what a crashed session left.
    pub(crate) fn answered(&mut self) {
        if let Lock::Sidecar { offered, kept, .. } = self {
            (*offered, *kept) = (false, false);
        }
    }

    /// The design was just saved, so what was auto-saved is older. Failing
    /// to empty it only leaves an older state to be offered should this
    /// session crash; the next save or clean close tries again. What a
    /// crashed session left is kept until the user answers the offer.
    pub(crate) fn saved(&mut self) {
        if let Lock::Sidecar {
            sidecar,
            offered: false,
            ..
        } = self
        {
            let _ = sidecar.clear();
        }
    }

    /// The sidecar, to let go of, if it's held.
    pub(crate) fn into_held(self) -> Option<H> {
        match self {
            Lock::Sidecar { sidecar, .. } => Some(sidecar),
            Lock::ReadOnly(_) => None,
        }
    }
}
