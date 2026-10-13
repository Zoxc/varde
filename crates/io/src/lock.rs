//! A design's lock, which its auto-saves go to while it's editable, and
//! what it offers back of a session that ended without closing it: the
//! same natively, where it's the design's sidecar next to it (see
//! `src/native/sidecar.rs`), and on the web for a design in browser
//! storage, whose sidecar sits next to it there (see `src/browser.rs`).

use varde_document::{Document, Snapshot};

use crate::autosave::{Ending, Held};
use crate::open::KEPT;
use crate::vrdp::{Error as FileError, Storage, Tail};
use crate::{Access, Damage, Offer, ReadOnly, RecoveryError};

/// What a [`Lock`] holds: the auto-saves of a design, natively with the
/// path they're at.
pub(crate) trait AsHeld {
    type Storage: Storage;

    fn as_held(&mut self) -> &mut Held<Self::Storage>;
}

impl<S: Storage> AsHeld for Held<S> {
    type Storage = S;

    fn as_held(&mut self) -> &mut Held<S> {
        self
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
        /// What a crashed session left in it, offered to the user and not
        /// answered yet.
        left: Left,
    },
    /// Not held: the document is read-only.
    ReadOnly(ReadOnly),
}

/// What a crashed session left in a held sidecar, as offered to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Left {
    /// Nothing offered, or the offer is answered.
    Nothing,
    /// Offered as [`Opened::recovered`](crate::Opened::recovered), not
    /// answered yet: until it's discarded or auto-saved over, saves leave
    /// it be. So is one whose records are all damaged, which the offer
    /// says why.
    Offered,
    /// Offered so, as one that can't be read ([`RecoveryError::kept`]):
    /// auto-saves are refused, too, till it's discarded.
    Kept,
}

impl<H: AsHeld> Lock<H> {
    /// The lock `taken`, or why the design is read-only.
    pub(crate) fn new(taken: Result<H, ReadOnly>) -> Self {
        match taken {
            Ok(sidecar) => Lock::Sidecar {
                sidecar,
                left: Left::Nothing,
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
    /// [`Left::Offered`] if there's an offer, or [`Left::Kept`] if it's
    /// kept as it can't be read.
    pub(crate) fn offer(
        &mut self,
        tail: Tail,
        based_past: impl FnOnce(Tail) -> bool,
        document: &Document,
    ) -> Result<Option<Offer>, RecoveryError> {
        // Only the editor holding the lock may look: otherwise the sidecar
        // is another editor's, auto-saving as it goes.
        let Lock::Sidecar { sidecar, left } = self else {
            return Ok(None);
        };
        *left = Left::Nothing;
        match sidecar.as_held().read_with_report() {
            Ok(Some(read)) if *read.payload.document != *document => {
                *left = Left::Offered;
                let recovered = read.payload;
                let newer_base = recovered
                    .base
                    .is_some_and(|base| base != tail && based_past(base));
                Ok(Some(Offer {
                    design_changed: !recovered.based_on(tail),
                    document: Snapshot::unwrap_or_clone(recovered.document),
                    damage: Damage::of(&read.report),
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
                if unreadable {
                    *left = Left::Kept;
                }
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
        matches!(
            self,
            Lock::Sidecar {
                left: Left::Kept,
                ..
            }
        )
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
        matches!(
            self,
            Lock::Sidecar {
                left: Left::Offered | Left::Kept,
                ..
            }
        )
    }

    /// The user answered the offer of what a crashed session left.
    pub(crate) fn answered(&mut self) {
        if let Lock::Sidecar { left, .. } = self {
            *left = Left::Nothing;
        }
    }

    /// Auto-saves to the sidecar by `append`: refused while it keeps what
    /// a crashed session left that can't be read, till the user discards
    /// it, and when it isn't held. The UI only auto-saves once the offer
    /// is answered, so one that's made answers it.
    pub(crate) fn auto_save(
        &mut self,
        append: impl FnOnce(&mut H) -> Result<(), FileError>,
    ) -> Result<(), String> {
        if self.kept() {
            return Err(KEPT.to_owned());
        }
        let sidecar = self.held().ok_or_else(|| READ_ONLY.to_owned())?;
        append(sidecar).map_err(|e| e.to_string())?;
        self.answered();
        Ok(())
    }

    /// The user discarded what a crashed session left: empties the
    /// sidecar, if it's held, and answers the offer.
    pub(crate) fn discard_recovery(&mut self) -> Result<(), String> {
        if let Some(sidecar) = self.held() {
            sidecar.as_held().clear().map_err(|e| e.to_string())?;
        }
        self.answered();
        Ok(())
    }

    /// How to let go of it as the design is saved as another file: what a
    /// crashed session left that the user hasn't answered stays with the
    /// design it's of, to be offered again.
    pub(crate) fn ending_for_saved_as(&self) -> Ending {
        if self.offered() {
            Ending::Release
        } else {
            Ending::Close
        }
    }

    /// The design was just saved, so what was auto-saved is older. Failing
    /// to empty it only leaves an older state to be offered should this
    /// session crash; the next save or clean close tries again. What a
    /// crashed session left is kept until the user answers the offer.
    pub(crate) fn saved(&mut self) {
        if let Lock::Sidecar {
            sidecar,
            left: Left::Nothing,
        } = self
        {
            let _ = sidecar.as_held().clear();
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
