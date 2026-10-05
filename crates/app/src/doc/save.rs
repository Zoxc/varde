//! Saving and leaving a document. They go in steps, each on something the
//! user did or the IO lane answered, and each saying what the app is to do
//! next, see [`Next`]: a document can't ask the user or close its own
//! screen.

use std::borrow::Cow;
use std::mem;
use std::path::Path;
use std::time::Duration;

use iced::time::Instant;
use iced::window;
use varde_document::name::{UNTITLED, with_extension};
use varde_document::{Command, EXTENSION, Revision};
use varde_io::{
    Chosen, Closing, FileId, LastDownload, OpenId, Picked, Request as IoRequest, SaveError, SaveTo,
    UnixSeconds,
};
use varde_view::{NOT_SAVED, SavePlace, Unsaved};

use super::{Doc, FileDamage, Recovery, design_name, read_only, shown_damage};
use crate::io::Io;
use crate::{Files, Next};

/// Where a document is written, and how saving and leaving it go: the
/// part of [`Doc`] the save and leave steps keep to themselves.
pub(super) struct Persist {
    /// Where the document is written.
    target: Target,
    /// The store entry asked for for this new design, until it's made.
    /// A Save As answered first gives the design a file, and the entry is
    /// closed once made.
    creating: Option<OpenId>,
    /// The editor revision last saved, or when the document was loaded or
    /// created. `None` if what it was loaded as was never saved, as for a
    /// new design recovered after a crash.
    saved_revision: Option<Revision>,
    /// When to auto-save.
    auto_save: AutoSave,
    /// What a session that crashed left of the document, offered to be
    /// restored or kept as it can't be read. Auto-saves wait for the
    /// user's answer, so as not to replace it, and closing keeps it if
    /// there's none.
    recovered: Option<Recovery>,
    /// How the file the document was opened from was found damaged, if it
    /// was and a banner says so ([`FileDamage::has_banner`]), until
    /// dismissed or saved as another file.
    damage: Option<FileDamage>,
    /// Whether the document's file of the user's was opened past damage
    /// (see [`FileDamage::kept_as_it_is`]), or the IO lane said so
    /// ([`SaveError::OpenedDamaged`]): the title says so, and Save acts as
    /// Save As, leaving the file as it is. Saved as another file, it's no
    /// longer.
    damaged_file: bool,
    /// The saves sent to the IO lane and not answered yet.
    saves: Saves,
    /// Why the newest save failed, if it did. One failing while a newer
    /// save that supersedes it is in flight isn't kept: the newer one's
    /// outcome decides, see [`Saves::supersede`]. A save sent, or a Save
    /// As answered as done, clears it.
    save_error: Option<SaveError>,
    /// Why the newest auto-save failed, if it did, or why the document has
    /// nowhere to be auto-saved to, shown after `save_error`. Cleared by an
    /// auto-save answered as done, and by a save answered as done, which
    /// leaves nothing unsaved to protect: should auto-saving still fail,
    /// the next auto-save says so again.
    auto_save_error: Option<String>,
    /// What the document is waiting on the user or the browser for.
    picking: Option<Picking>,
    /// The name asked for in the app's own Save As dialog, on the web,
    /// while it shows (`picking` is [`Picking::Name`] then).
    naming: Option<Naming>,
    /// Whether the name field of the Save As dialog is to take the focus,
    /// as it shows: see [`Doc::take_name_focus`].
    name_focus: bool,
    /// The last download recorded of a design in browser storage, if
    /// there's one, see [`DocDownload`].
    download: Option<DocDownload>,
    /// On the way out, from the user asking to leave until the document is
    /// gone or they stay after all.
    leaving: Option<Leaving>,
}

impl Persist {
    /// Writing to `target`, with `creating`, `recovered` and `damage` as
    /// in an [`Origin`](super::Origin),
    /// loaded at editor `revision` and saved at `saved_revision`.
    pub(super) fn new(
        target: Target,
        creating: Option<OpenId>,
        recovered: Option<Recovery>,
        damage: Option<FileDamage>,
        saved_revision: Option<Revision>,
        revision: Revision,
    ) -> Self {
        Self {
            target,
            creating,
            saved_revision,
            auto_save: AutoSave {
                // What it was loaded as is saved, or in its store entry.
                sent: Some(revision),
                ..AutoSave::default()
            },
            recovered,
            damaged_file: damage.is_some_and(|damage| damage.kept_as_it_is()),
            damage: damage.filter(FileDamage::has_banner),
            saves: Saves::default(),
            save_error: None,
            auto_save_error: None,
            picking: None,
            naming: None,
            name_focus: false,
            download: None,
            leaving: None,
        }
    }
}

/// The last download of a design in browser storage: when, and the editor
/// revision it was the design as saved at, if it was, with no changes
/// since then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocDownload {
    pub(crate) time: UnixSeconds,
    pub(crate) revision: Option<Revision>,
}

/// The app's own Save As dialog, on the web, where browser storage has no
/// picker of the system's: the name, and on Chromium where to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Naming {
    /// As typed.
    pub(crate) name: String,
    pub(crate) place: SavePlace,
    /// Whether it renames the design rather than saving it as another.
    pub(crate) rename: bool,
    /// The file name in browser storage to ask about replacing: one of the
    /// designs listed there has it. Confirming again replaces it.
    pub(crate) replacing: Option<String>,
}

/// What a document is waiting on the user or the browser for, see
/// [`Persist::picking`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Picking {
    /// The Save As dialog is showing, answered with [`Doc::save_as_picked`].
    SaveAs,
    /// On the web, the browser is asking whether the document may be saved
    /// to its file, answered with [`Doc::writable`].
    Writable,
    /// On the web, the app's own Save As dialog is showing, asking for a
    /// name: see [`Naming`].
    Name,
}

/// Which kind of save an answer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveKind {
    /// To the document's own file.
    Save,
    /// To a new file, or on the web a design in browser storage by name.
    SaveAs,
}

/// Where a document is written, held by the IO lane.
#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    /// Nowhere yet: a new design whose store entry is on its way (see
    /// [`Persist::creating`]) or couldn't be made, or whose Save As is.
    None,
    /// The store entry of a design never saved, which is only auto-saved
    /// to: Save asks where to.
    Entry { file: FileId },
    /// The file of the user's the design was opened from or last saved
    /// as, which Save writes.
    File {
        file: FileId,
        /// On the web, the file as picked, if it can be written to: the
        /// browser is asked whether it may be as the user saves, see
        /// [`Doc::save_design`].
        picked: Option<Picked>,
    },
    /// On the web, the design `name` in browser storage, which Save
    /// appends to.
    Browser { file: FileId, name: String },
}

impl Target {
    /// What the IO lane knows it by, if anything: what auto-saves go to
    /// and closing closes.
    pub(crate) fn file(&self) -> Option<FileId> {
        match *self {
            Target::None => None,
            Target::Entry { file } | Target::File { file, .. } | Target::Browser { file, .. } => {
                Some(file)
            }
        }
    }

    /// The design's own file, if it has been saved as one, rather than a
    /// store entry or nothing.
    pub(crate) fn design_file(&self) -> Option<FileId> {
        match *self {
            Target::File { file, .. } | Target::Browser { file, .. } => Some(file),
            _ => None,
        }
    }

    /// Where the design is kept, as the file cell says: on the web only.
    pub(crate) fn location(&self) -> Option<varde_view::Location> {
        match self {
            Target::Browser { .. } => Some(varde_view::Location::Browser),
            Target::File {
                picked: Some(_), ..
            } => Some(varde_view::Location::Computer),
            _ => None,
        }
    }
}

/// Hands a design to the browser as a download, by the file name and its
/// bytes, see [`Doc::download`]: [`varde_io::pick::downloader`]'s, or a
/// test's.
pub(crate) type Downloader = Box<dyn Fn(&str, &[u8]) -> Result<(), String>>;

/// Saves of a document sent to the IO lane and not answered yet, and
/// those waiting to be sent.
///
/// The lane answers every Save As, in order. A Save is only answered if
/// no newer one replaced it while it waited, but the newest one always
/// is, and saves are answered in order, so waiting for the newest one is
/// waiting for all of them.
#[derive(Debug, Default)]
pub(crate) struct Saves {
    /// The revision of the newest Save.
    pub(crate) save: Option<Revision>,
    /// The revisions of the Save Ases, in the order sent.
    pub(crate) save_as: Vec<Revision>,
    /// Saves the user asked for while edits waited on the solver, in the
    /// order asked: sent once the edits are answered, so what's on screen
    /// is saved, see [`Doc::proposals_settled`]. Then, or once there's
    /// nothing to wait for, they wait for their thumbnail, see
    /// `thumbnail.rs`.
    pub(crate) waiting: Vec<Deferred>,
}

/// A save waiting for the edits waiting on the solver, or its thumbnail.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Deferred {
    Save,
    /// To where the user chose.
    SaveAs(SaveTo),
}

impl Saves {
    pub(crate) fn any(&self) -> bool {
        self.save.is_some() || !self.save_as.is_empty() || !self.waiting.is_empty()
    }

    /// Whether a save of kind `failed` failing is superseded by one still
    /// in flight or waiting to be sent, which is newer, since saves are
    /// answered in the order sent: a Save As, which gives the document
    /// another file or fails itself, or a Save if the failed one was a
    /// Save too. A Save writes the document's own file, not the one a
    /// Save As was to write.
    pub(crate) fn supersede(&self, failed: SaveKind) -> bool {
        let save_as = |deferred: &Deferred| matches!(deferred, Deferred::SaveAs(_));
        let save_waits = self.waiting.contains(&Deferred::Save);
        !self.save_as.is_empty()
            || self.waiting.iter().any(save_as)
            || (failed == SaveKind::Save && (self.save.is_some() || save_waits))
    }

    /// Whether `revision` is on its way to a file.
    pub(crate) fn covers(&self, revision: Revision) -> bool {
        self.save == Some(revision) || self.save_as.contains(&revision)
    }
}

/// What the user is leaving the document for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leave {
    /// Going back to the welcome screen.
    Close,
    /// Closing the window, which quits.
    Quit(window::Id),
}

impl Leave {
    /// Leaving for `self`, then asked to leave for `other`: quitting wins.
    pub(crate) fn and(self, other: Leave) -> Leave {
        match (self, other) {
            (Leave::Close, quit @ Leave::Quit(_)) => quit,
            (leave, _) => leave,
        }
    }
}

/// Leaving the document, for `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Leaving {
    pub(crate) to: Leave,
    pub(crate) step: Step,
}

/// What leaving the document waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// The user's answer about unsaved changes.
    Asking,
    /// The saves in flight. Unless the user chose to discard unsaved
    /// changes, a save failing cancels leaving, so they can see why and
    /// try again, unless a newer one in flight supersedes it, see
    /// [`Saves::supersede`].
    Saving(Changes),
    /// The edits waiting on the solver, which may leave changes to ask
    /// about once they're committed.
    Proposing,
}

/// What happens to unsaved changes on the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Changes {
    /// Saved, or leaving waits for the user to decide.
    Keep,
    /// Lost, as the user chose.
    Discard,
}

/// How long the document must go unedited before it's auto-saved.
pub(crate) const AUTO_SAVE_IDLE: Duration = Duration::from_secs(3);
/// The longest edits go without an auto-save while they keep coming.
pub(crate) const AUTO_SAVE_MAX_WAIT: Duration = Duration::from_secs(120);

/// When to auto-save, told by the ticks of a timer: once the document has
/// gone [`AUTO_SAVE_IDLE`] without an edit, and at least every
/// [`AUTO_SAVE_MAX_WAIT`] while edits keep coming. Edits are seen at ticks,
/// by the editor revision changing.
#[derive(Debug, Default)]
pub(crate) struct AutoSave {
    /// The newest revision sent to be auto-saved, or known to be where it
    /// would be: the revision the document was loaded at, or saved at, or
    /// being saved at, see [`AutoSave::saving`].
    pub(crate) sent: Option<Revision>,
    /// The revision seen at the last tick, and the tick it was first seen.
    pub(crate) seen: Option<(Revision, Instant)>,
    /// The tick the first edit not auto-saved yet was seen.
    pub(crate) since: Option<Instant>,
}

impl AutoSave {
    /// Whether to auto-save `revision` at `now`, if it's `wanted` at all.
    /// Once it says so, it's taken to be sent.
    pub(crate) fn due(&mut self, revision: Revision, wanted: bool, now: Instant) -> bool {
        if !wanted || self.sent == Some(revision) {
            self.seen = None;
            self.since = None;
            return false;
        }
        let changed = match self.seen {
            Some((seen, at)) if seen == revision => at,
            _ => {
                self.seen = Some((revision, now));
                now
            }
        };
        let since = *self.since.get_or_insert(now);
        let idle = now.saturating_duration_since(changed) >= AUTO_SAVE_IDLE;
        let waited = now.saturating_duration_since(since) >= AUTO_SAVE_MAX_WAIT;
        if idle || waited {
            self.sent = Some(revision);
            self.since = None;
        }
        idle || waited
    }

    /// A save of `revision` was sent: once it's done, what was auto-saved
    /// before it is emptied, as if `revision` was auto-saved.
    pub(crate) fn saving(&mut self, revision: Revision) {
        self.sent = Some(revision);
    }

    /// The save of `revision` failed, leaving what was auto-saved before
    /// it: unless auto-saved since, it's auto-saved again, see
    /// [`AutoSave::saving`].
    pub(crate) fn not_saved(&mut self, revision: Revision) {
        if self.sent == Some(revision) {
            self.sent = None;
        }
    }
}

impl Doc {
    /// Whether there are unsaved changes: whether the document isn't as
    /// last saved, which undoing back to it makes it again.
    pub(crate) fn edited(&self) -> bool {
        self.persist.saved_revision != Some(self.editor.revision())
    }

    /// Whether there are changes that would be lost on leaving: edits not
    /// saved and not on their way to the file either.
    pub(crate) fn unsaved(&self) -> bool {
        self.edited() && !self.persist.saves.covers(self.editor.revision())
    }

    /// Where the user is asked to leave for while asking them about
    /// unsaved changes.
    pub(crate) fn prompt(&self) -> Option<Leave> {
        self.persist
            .leaving
            .and_then(|leaving| (leaving.step == Step::Asking).then_some(leaving.to))
    }

    /// Unsaved changes a crashed session left, offered to be restored.
    #[cfg(test)]
    pub(crate) fn recovered(&self) -> Option<&varde_io::Offer> {
        match &self.persist.recovered {
            Some(Recovery::Offered(offer)) => Some(offer),
            _ => None,
        }
    }

    /// Whether what a crashed session left can't be read, and is kept
    /// until discarded, see [`Recovery::Kept`].
    #[cfg(test)]
    pub(crate) fn recovery_kept(&self) -> bool {
        self.persist.recovered == Some(Recovery::Kept)
    }

    /// What the banner offering what a crashed session left shows, if
    /// there's that, with times relative to `now`.
    pub(crate) fn recovered_changes(
        &self,
        now: UnixSeconds,
    ) -> Option<varde_view::RecoveredChanges> {
        Some(match self.persist.recovered.as_ref()? {
            Recovery::Offered(offer) => varde_view::RecoveredChanges {
                design_changed: offer.design_changed,
                newer_base: offer.newer_base,
                damage: offer.damage.map(|damage| shown_damage(&damage, now)),
                unreadable: false,
            },
            Recovery::Kept => varde_view::RecoveredChanges {
                design_changed: false,
                newer_base: false,
                damage: None,
                unreadable: true,
            },
        })
    }

    /// How the file the document was opened from was found damaged, see
    /// [`Persist::damage`].
    pub(crate) fn damage(&self) -> Option<&FileDamage> {
        self.persist.damage.as_ref()
    }

    /// Hides the banner saying the file was found damaged.
    pub(super) fn dismiss_damage(&mut self) {
        self.persist.damage = None;
    }

    /// Whether the document's file was opened past damage, see
    /// [`Persist::damaged_file`].
    #[cfg(test)]
    pub(crate) fn damaged_file(&self) -> bool {
        self.persist.damaged_file
    }

    /// Whether the design has no name: never saved, nor known by the name
    /// of a file it was opened from. "Not saved" shows in its name's place.
    pub(crate) fn unnamed(&self) -> bool {
        self.persist.target.design_file().is_none() && self.name == UNTITLED
    }

    /// The name to suggest saving or exporting the design as: its own,
    /// none for one with no name.
    pub(crate) fn suggested_name(&self) -> String {
        if self.unnamed() {
            String::new()
        } else {
            self.name.clone()
        }
    }

    /// The design's name as the window's title shows it: with its
    /// extension, marked "(damaged file)" while its file was opened past
    /// damage; "Not saved" for one with no name.
    pub(crate) fn title_name(&self) -> String {
        if self.unnamed() {
            return NOT_SAVED.to_owned();
        }
        let damaged = if self.persist.damaged_file {
            " (damaged file)"
        } else {
            ""
        };
        format!("{}.{EXTENSION}{damaged}", self.name)
    }

    /// Where the document is written, see [`Persist::target`].
    #[cfg(test)]
    pub(crate) fn target(&self) -> &Target {
        &self.persist.target
    }

    /// Where the document is written, for the view.
    pub(super) fn persist_target(&self) -> &Target {
        &self.persist.target
    }

    /// The saves in flight, see [`Persist::saves`].
    #[cfg(test)]
    pub(crate) fn saves(&self) -> &Saves {
        &self.persist.saves
    }

    /// Why the newest save failed, see [`Persist::save_error`].
    #[cfg(test)]
    pub(crate) fn save_error(&self) -> Option<&SaveError> {
        self.persist.save_error.as_ref()
    }

    /// Why the newest auto-save failed, see [`Persist::auto_save_error`].
    #[cfg(test)]
    pub(crate) fn auto_save_error(&self) -> Option<&str> {
        self.persist.auto_save_error.as_deref()
    }

    /// What the document is waiting on, see [`Persist::picking`].
    #[cfg(test)]
    pub(crate) fn picking(&self) -> Option<Picking> {
        self.persist.picking
    }

    /// Leaving the document, see [`Persist::leaving`].
    #[cfg(test)]
    pub(crate) fn leaving(&self) -> Option<Leaving> {
        self.persist.leaving
    }

    /// Has the document as it is count as saved and auto-saved where
    /// `was`, its revision before, did: for what follows from reading it
    /// alone, see `relink.rs`.
    pub(super) fn keep_clean(&mut self, was: Revision) {
        let now = self.editor.revision();
        if self.persist.saved_revision == Some(was) {
            self.persist.saved_revision = Some(now);
        }
        if self.persist.auto_save.sent == Some(was) {
            self.persist.auto_save.sent = Some(now);
        }
    }

    /// The editor revision last saved, see [`Persist::saved_revision`].
    #[cfg(test)]
    pub(crate) fn saved_revision(&self) -> Option<Revision> {
        self.persist.saved_revision
    }

    /// Whether saves are on their way to a file.
    pub(crate) fn saving(&self) -> bool {
        self.persist.saves.any()
    }

    /// Whether leaving now would lose changes, unsaved, on their way to
    /// the file or waiting on the solver.
    pub(crate) fn at_stake(&self) -> bool {
        self.unsaved() || self.saving() || self.proposing()
    }

    /// Goes on with what waited for the edits waiting on the solver, once
    /// they're answered or dropped: the saves asked for meanwhile, then
    /// leaving. The same once a thumbnail is rendered, which they may have
    /// waited for too (see `thumbnail.rs`).
    pub(crate) fn proposals_settled(&mut self, cx: &mut Files) -> Next {
        if self.proposing() {
            return Next::Stay;
        }
        for deferred in mem::take(&mut self.persist.saves.waiting) {
            match deferred {
                Deferred::Save => self.save(cx),
                Deferred::SaveAs(to) => self.save_as(cx, to),
            }
        }
        self.resume_leaving(cx)
    }

    /// Keeps `deferred` to send once the edits waiting on the solver are
    /// answered, if any are: whether it waits. A Save waiting already
    /// saves what this would.
    fn defer(&mut self, deferred: Deferred) -> bool {
        if !self.proposing() {
            return false;
        }
        self.wait(deferred);
        true
    }

    /// Keeps `deferred` to send once its thumbnail is rendered, if it must
    /// wait for one (see [`Doc::thumbnail_waits`]): whether it waits.
    fn defer_for_thumbnail(&mut self, deferred: Deferred) -> bool {
        if !self.thumbnail_waits() {
            return false;
        }
        self.wait(deferred);
        true
    }

    /// Keeps `deferred` waiting, unless it's a Save and one waits already.
    fn wait(&mut self, deferred: Deferred) {
        let waiting = &mut self.persist.saves.waiting;
        if deferred != Deferred::Save || !waiting.contains(&deferred) {
            waiting.push(deferred);
        }
    }

    /// Hides why the last save or auto-save failed.
    pub(super) fn dismiss_save_error(&mut self) {
        self.persist.save_error = None;
        self.persist.auto_save_error = None;
    }

    /// Whether there's something to save that isn't on its way already, and
    /// the document may be saved.
    pub(crate) fn unsent(&self) -> bool {
        self.editable() && self.edited() && self.persist.saves.save != Some(self.editor.revision())
    }

    /// Sends the document to its file, unless there's nothing new to save
    /// or it has no file.
    fn save(&mut self, cx: &mut Files) {
        let Some(file) = self.persist.target.design_file() else {
            return;
        };
        if self.defer(Deferred::Save) || !self.unsent() || self.defer_for_thumbnail(Deferred::Save)
        {
            return;
        }
        let revision = self.editor.revision();
        cx.io.send(IoRequest::Save {
            file,
            revision,
            document: self.editor.snapshot(),
            thumbnail: self.thumbnail(),
        });
        // Asked as the user saves to browser storage, once a session.
        cx.ask_persist |= matches!(self.persist.target, Target::Browser { .. });
        self.persist.saves.save = Some(revision);
        self.persist.save_error = None;
        self.persist.auto_save.saving(revision);
    }

    /// Sends the document to the new file the user chose, `to`, in the
    /// Save As dialog, which asked about replacing the one there.
    fn save_as(&mut self, cx: &mut Files, to: SaveTo) {
        if self.defer(Deferred::SaveAs(to.clone()))
            || self.defer_for_thumbnail(Deferred::SaveAs(to.clone()))
        {
            return;
        }
        let revision = self.editor.revision();
        let (file, document) = (self.persist.target.file(), self.editor.snapshot());
        cx.io.send(IoRequest::SaveAs {
            file,
            to,
            revision,
            document,
            thumbnail: self.thumbnail(),
        });
        self.persist.saves.save_as.push(revision);
        self.persist.save_error = None;
        self.persist.auto_save.saving(revision);
    }

    /// The answer to a Save of `revision` to `file`, if it's still where
    /// the document is written.
    pub(crate) fn saved_to(
        &mut self,
        cx: &mut Files,
        file: FileId,
        revision: Revision,
        result: Result<(), SaveError>,
    ) -> Next {
        if self.persist.target.file() != Some(file) {
            return Next::Stay;
        }
        if result == Err(SaveError::OpenedDamaged) {
            return self.save_refused_as_damaged(cx, revision);
        }
        self.saved(SaveKind::Save, revision, result);
        self.resume_leaving(cx)
    }

    /// The Save of `revision` was refused, as the file was opened past
    /// damage, which the document didn't know: from now on Save acts as
    /// Save As, and it's asked where to save now, unless a newer save
    /// decides, or the user is being asked something else, when the
    /// banner says so instead.
    fn save_refused_as_damaged(&mut self, cx: &mut Files, revision: Revision) -> Next {
        self.persist.damaged_file = true;
        self.answered(SaveKind::Save, revision, false);
        if self.persist.saves.supersede(SaveKind::Save) {
            return Next::Stay;
        }
        if self.persist.picking.is_some() {
            self.persist.save_error = Some(SaveError::OpenedDamaged);
            self.stop_leaving_unless_discarding();
            return self.resume_leaving(cx);
        }
        // Leaving for it waits for the Save As instead.
        self.request_save_as(cx)
    }

    /// Takes the answer to a save of kind `kind` of `revision` off the
    /// saves in flight, `ok` or not.
    fn answered(&mut self, kind: SaveKind, revision: Revision, ok: bool) {
        match kind {
            SaveKind::SaveAs => {
                if let Some(at) = self
                    .persist
                    .saves
                    .save_as
                    .iter()
                    .position(|&r| r == revision)
                {
                    self.persist.saves.save_as.remove(at);
                }
            }
            SaveKind::Save => {
                if self.persist.saves.save == Some(revision) {
                    self.persist.saves.save = None;
                }
            }
        }
        if !ok {
            self.persist.auto_save.not_saved(revision);
        }
    }

    /// Takes the answer to a save of kind `kind` of `revision`.
    fn saved(&mut self, kind: SaveKind, revision: Revision, result: Result<(), SaveError>) {
        self.answered(kind, revision, result.is_ok());
        match result {
            // Not the current revision: edits made while saving keep the
            // document edited. The IO lane answers saves in the order sent,
            // so this is the newest one to land.
            Ok(()) => {
                self.persist.saved_revision = Some(revision);
                // Sending it cleared the save error, so one there now is
                // of a Save As failing since, which a Save doesn't
                // supersede.
                if kind == SaveKind::SaveAs {
                    self.persist.save_error = None;
                }
                self.persist.auto_save_error = None;
            }
            // The newer save decides: it clears the banner or shows its own
            // error, and leaving goes on or stops on its answer.
            Err(_) if self.persist.saves.supersede(kind) => {}
            Err(error) => {
                self.persist.save_error = Some(error);
                self.stop_leaving_unless_discarding();
            }
        }
    }

    /// A save leaving was waiting on won't happen: leaving stops, so the
    /// user sees why, unless they chose to discard. See [`Step::Saving`].
    fn stop_leaving_unless_discarding(&mut self) {
        if matches!(
            self.persist.leaving,
            Some(Leaving {
                step: Step::Saving(Changes::Keep),
                ..
            })
        ) {
            self.persist.leaving = None;
        }
    }

    /// Sends the document to be auto-saved if it's time to, see
    /// [`AutoSave`]. Only an editable document with unsaved changes and a
    /// file to auto-save to is, and not while offering to restore what a
    /// crashed session auto-saved.
    pub(crate) fn auto_save(&mut self, cx: &mut Files, now: Instant) {
        self.roll_back_auto_save(&mut cx.io);
        let file = self.auto_save_file();
        if self
            .persist
            .auto_save
            .due(self.editor.revision(), file.is_some(), now)
            && let Some(file) = file
        {
            self.send_auto_save(&mut cx.io, file);
        }
    }

    /// Sends the document to be auto-saved now, if there's anything to,
    /// e.g. once recovered changes are restored: until then the IO lane
    /// keeps what was recovered from being emptied by a save. Not while
    /// offering to restore them, as [`Doc::auto_save`].
    pub(crate) fn auto_save_now(&mut self, cx: &mut Files) {
        self.roll_back_auto_save(&mut cx.io);
        if let Some(file) = self.auto_save_file()
            && self.persist.auto_save.sent != Some(self.editor.revision())
        {
            self.send_auto_save(&mut cx.io, file);
        }
    }

    /// Undone or redone back to as saved, the document has its auto-saves
    /// of the edits since taken back, so a crash doesn't offer them: its
    /// sidecar or store entry is emptied, as by a save. Not while offering
    /// to restore what a crashed session left, which the lane keeps.
    fn roll_back_auto_save(&mut self, io: &mut Io) {
        let revision = self.editor.revision();
        let Some(file) = self.persist.target.file() else {
            return;
        };
        if !self.editable()
            || self.persist.recovered.is_some()
            || self.edited()
            || self.persist.auto_save.sent == Some(revision)
        {
            return;
        }
        io.send(IoRequest::DiscardRecovery { file });
        self.persist.auto_save = AutoSave {
            sent: Some(revision),
            ..AutoSave::default()
        };
    }

    /// Where to auto-save the document to, if it's to be auto-saved at
    /// all: see [`Doc::auto_save`].
    fn auto_save_file(&self) -> Option<FileId> {
        let file = self.persist.target.file()?;
        (self.editable() && self.persist.recovered.is_none() && self.unsaved()).then_some(file)
    }

    /// Sends the document as it is to be auto-saved to `file`.
    fn send_auto_save(&mut self, io: &mut Io, file: FileId) {
        let revision = self.editor.revision();
        io.send(IoRequest::AutoSave {
            file,
            revision,
            document: self.editor.snapshot(),
        });
        self.persist.auto_save = AutoSave {
            sent: Some(revision),
            ..AutoSave::default()
        };
    }

    /// The answer to an auto-save to `file`, if it's still where the
    /// document is written.
    pub(crate) fn auto_saved(&mut self, file: FileId, result: Result<(), String>) {
        if self.persist.target.file() != Some(file) {
            return;
        }
        match result {
            Ok(()) => self.persist.auto_save_error = None,
            // Tried again after the next edit.
            Err(error) => self.auto_save_failed(&error),
        }
    }

    /// Shows that the document isn't auto-saved, and why: otherwise it
    /// would be lost with the app or tab without a word.
    pub(crate) fn auto_save_failed(&mut self, error: &str) {
        log::error!("Couldn't auto-save: {error}");
        self.persist.auto_save_error = Some(error.to_owned());
    }

    /// The save error to show, else the auto-save error.
    pub(crate) fn banner_error(&self) -> Option<Cow<'_, str>> {
        match (&self.persist.save_error, &self.persist.auto_save_error) {
            (Some(SaveError::Failed(error)), _) => Some(Cow::Borrowed(error)),
            (Some(error), _) => Some(Cow::Owned(error.to_string())),
            (None, Some(error)) => Some(Cow::Owned(format!(
                "changes aren't being auto-saved: {error}"
            ))),
            (None, None) => None,
        }
    }

    /// Restores the changes offered as recovered, as one undoable edit
    /// that leaves the document edited, and auto-saves them at once. A
    /// read-only document keeps offering them. The edits waiting on the
    /// solver, and the changes waiting behind them, were made on the
    /// document replaced, so they're dropped, and what waited for them
    /// goes on.
    pub(crate) fn restore_recovered(&mut self, cx: &mut Files) -> Next {
        if self.editable()
            && let Some(Recovery::Offered(offer)) = (self.persist.recovered)
                .take_if(|recovery| matches!(recovery, Recovery::Offered(_)))
        {
            self.drop_proposals();
            self.apply(Command::Replace(Box::new(offer.document)));
            self.sync();
            self.auto_save_now(cx);
        }
        self.proposals_settled(cx)
    }

    /// Drops the changes offered as recovered, or kept as they can't be
    /// read, and has the IO lane delete them.
    pub(crate) fn discard_recovered(&mut self, cx: &mut Files) {
        if self.persist.recovered.take().is_some()
            && let Some(file) = self.persist.target.file()
        {
            cx.io.send(IoRequest::DiscardRecovery { file });
        }
    }

    /// Saves the document to its file, or asks where to if it has none.
    pub(crate) fn request_save(&mut self, cx: &mut Files) -> Next {
        self.file_menu = false;
        if !self.editable() || self.persist.picking.is_some() {
            return Next::Stay;
        }
        self.save_or_ask(cx)
    }

    /// Saves the document to its own file if it may be, else waits for a
    /// Save As on its way, else asks where to save. A file opened past
    /// damage is never saved to, see [`Persist::damaged_file`].
    fn save_or_ask(&mut self, cx: &mut Files) -> Next {
        match self.persist.target.design_file() {
            Some(_) if self.editable() && !self.persist.damaged_file => self.save_design(cx),
            // A Save As on its way: saving again waits for it, and asks
            // again once it's answered, when the document may have a file
            // to save to.
            _ if self.persist.saves.any() => Next::Stay,
            // No file, or one that can't be saved to, e.g. after a Save As
            // to where no lock file can be made.
            _ => self.request_save_as(cx),
        }
    }

    /// Saves the document to its own file, then goes on leaving if that's
    /// what it's for. On the web the browser is asked first whether the
    /// file may be written, answered with [`Doc::writable`]: only the page
    /// can ask, and only as the user does something.
    fn save_design(&mut self, cx: &mut Files) -> Next {
        match &self.persist.target {
            Target::File {
                picked: Some(picked),
                ..
            } if self.unsent() => {
                self.persist.picking = Some(Picking::Writable);
                Next::AskWritable {
                    id: self.id,
                    picked: picked.clone(),
                }
            }
            _ => {
                self.save(cx);
                self.resume_leaving(cx)
            }
        }
    }

    /// The browser's answer to whether the document may be saved to its
    /// file, see [`Doc::save_design`].
    pub(super) fn writable(&mut self, cx: &mut Files, result: Result<(), String>) -> Next {
        if self.persist.picking != Some(Picking::Writable) {
            return Next::Stay;
        }
        self.persist.picking = None;
        match result {
            Ok(()) => {
                self.save(cx);
                self.resume_leaving(cx)
            }
            // Shown whatever else is in flight: saving to the file stays
            // refused until the user allows it. Leaving to discard goes on
            // once its saves are answered, which may have been while asking.
            Err(error) => {
                self.persist.save_error = Some(SaveError::Failed(error));
                self.stop_leaving_unless_discarding();
                self.resume_leaving(cx)
            }
        }
    }

    /// Asks where to save the document, answered with
    /// [`Doc::save_as_picked`]: natively the platform's dialog. On the
    /// web, where designs are saved in browser storage, which has no
    /// picker of the system's, the app's own dialog asks for a name (and
    /// on Chromium where), see [`Doc::name_confirmed`].
    pub(crate) fn request_save_as(&mut self, cx: &mut Files) -> Next {
        self.file_menu = false;
        // A Save As of a design never saved still on its way would make a
        // second file, so this waits for it like `Doc::request_save` does.
        let new_on_its_way =
            self.persist.target.design_file().is_none() && self.persist.saves.any();
        if self.persist.picking.is_some() || new_on_its_way {
            return Next::Stay;
        }
        if cx.browser_storage {
            self.ask_name(false);
            return Next::Stay;
        }
        self.persist.picking = Some(Picking::SaveAs);
        Next::PickSaveAs {
            id: self.id,
            name: self.suggested_name(),
        }
    }

    /// Shows the app's own Save As dialog, or as it renames the design.
    fn ask_name(&mut self, rename: bool) {
        self.persist.picking = Some(Picking::Name);
        self.persist.naming = Some(Naming {
            name: self.suggested_name(),
            place: SavePlace::Browser,
            rename,
            replacing: None,
        });
        self.persist.name_focus = true;
    }

    /// Asks for a new name for the design in browser storage, in the Save
    /// As dialog, see [`Doc::name_confirmed`]. Only a design kept there,
    /// editable and with nothing on its way, is renamed.
    pub(crate) fn request_rename(&mut self) {
        self.file_menu = false;
        if self.renamable() {
            self.ask_name(true);
        }
    }

    /// Whether the design may be renamed now, see [`Doc::request_rename`].
    pub(crate) fn renamable(&self) -> bool {
        matches!(self.persist.target, Target::Browser { .. })
            && self.editable()
            && self.persist.picking.is_none()
            && !self.persist.saves.any()
    }

    /// The Save As dialog, if it's showing.
    pub(crate) fn naming(&self) -> Option<&Naming> {
        self.persist.naming.as_ref()
    }

    /// Whether the Save As dialog's name field is to take the focus, as
    /// it's just shown: the app does that.
    pub(crate) fn take_name_focus(&mut self) -> bool {
        std::mem::take(&mut self.persist.name_focus)
    }

    /// The name typed in the Save As dialog changed.
    pub(crate) fn name_changed(&mut self, name: String) {
        if let Some(naming) = &mut self.persist.naming {
            naming.name = name;
            naming.replacing = None;
        }
    }

    /// Where to save the design was chosen in the Save As dialog.
    pub(crate) fn place_chosen(&mut self, place: SavePlace) {
        if let Some(naming) = &mut self.persist.naming {
            naming.place = place;
            naming.replacing = None;
        }
    }

    /// The Save As dialog's name was confirmed: saved as that design in
    /// browser storage, after asking about replacing one listed there of
    /// that name (confirming again replaces it), or renamed to it, which
    /// is refused for a name taken; or, for a file on the computer, the
    /// system's save picker shown. Saved as the design's own name, it's
    /// saved, appended to, its history kept; only one opened past damage,
    /// which isn't saved to, is replaced, after asking.
    pub(crate) fn name_confirmed(&mut self, cx: &mut Files) -> Next {
        let Some(naming) = self.persist.naming.clone() else {
            return Next::Stay;
        };
        if naming.place == SavePlace::Computer && !naming.rename {
            self.persist.naming = None;
            self.persist.picking = Some(Picking::SaveAs);
            return Next::PickSaveAs {
                id: self.id,
                name: naming.name,
            };
        }
        // Browser storage keeps a design by its name: there's no saving
        // one without.
        if naming.name.trim().is_empty() {
            return Next::Stay;
        }
        let name = varde_io::browser::file_name(&naming.name);
        let own = matches!(&self.persist.target, Target::Browser { name: current, .. } if *current == name);
        let asked = naming.replacing.as_deref() == Some(name.as_str());
        let taken = if own {
            // Renaming to its own name does nothing; saving as it saves.
            !naming.rename && self.persist.damaged_file
        } else {
            // Listed, or said to be taken by the lane, which asked.
            asked || cx.browser.iter().any(|design| design.name == name)
        };
        if taken && (naming.rename || !asked) {
            if let Some(naming) = &mut self.persist.naming {
                naming.replacing = Some(name);
            }
            return Next::Stay;
        }
        self.persist.naming = None;
        self.persist.picking = None;
        if naming.rename {
            if let Target::Browser { file, .. } = self.persist.target
                && !own
            {
                cx.io.send(IoRequest::Rename { file, name });
            }
            return self.resume_leaving(cx);
        }
        if own && !taken {
            return self.save_design(cx);
        }
        // Asked as the user saves there first, while the click counts.
        cx.ask_persist = true;
        self.save_as(
            cx,
            SaveTo::Browser {
                name,
                overwrite: taken,
            },
        );
        Next::Stay
    }

    /// The name the design was to be saved as in browser storage, `name`,
    /// is taken, as another tab saved a design of it since the app listed
    /// them: the Save As dialog shows again, asking about replacing it,
    /// unless the user is asked something else.
    fn ask_replacing(&mut self, name: String) -> bool {
        if self.persist.picking.is_some() {
            return false;
        }
        self.persist.picking = Some(Picking::Name);
        self.persist.naming = Some(Naming {
            name: design_name(Path::new(&name)),
            place: SavePlace::Browser,
            rename: false,
            replacing: Some(name),
        });
        self.persist.name_focus = true;
        true
    }

    /// The Save As dialog was closed without saving: as backing out of the
    /// system's, see [`Doc::save_as_picked`].
    pub(crate) fn name_cancelled(&mut self, cx: &mut Files) -> Next {
        if self.persist.picking != Some(Picking::Name) {
            return Next::Stay;
        }
        self.persist.naming = None;
        self.save_as_picked_with(cx, None)
    }

    /// The answer to renaming `file` to `name`, see [`Doc::request_rename`]:
    /// why it failed shows in the status bar.
    pub(crate) fn renamed(&mut self, file: FileId, name: String, result: Result<(), String>) {
        match (&mut self.persist.target, result) {
            (
                Target::Browser {
                    file: ours,
                    name: kept,
                },
                Ok(()),
            ) if *ours == file => {
                self.name = design_name(Path::new(&name));
                *kept = name;
            }
            (_, Ok(())) => {}
            (_, Err(error)) => self.notice = Some(format!("Couldn't rename the design: {error}")),
        }
    }

    /// Downloads the design as it is, as `<name>.vrdp`, on the web: encoded
    /// on the page, one encoding and one pass of snappy, cheaper than
    /// sending it to the lane and having the file sent back. A download
    /// changes nothing in storage, nor whether the design is saved: of a
    /// design in browser storage it's recorded, which the welcome screen
    /// and the file menu tell it stands against.
    pub(crate) fn download(&mut self, cx: &mut Files) {
        self.file_menu = false;
        let Some(download) = &cx.downloader else {
            return;
        };
        let name = varde_document::name::download_name(&self.name);
        let result = varde_io::vrdp::to_bytes(self.editor.document(), &[])
            .map_err(|e| e.to_string())
            .and_then(|(bytes, _)| download(&name, &bytes));
        if let Err(error) = result {
            self.notice = Some(format!("Couldn't download {name}: {error}"));
            return;
        }
        if let Target::Browser { file, .. } = self.persist.target {
            // With changes not saved, it's not the design as saved: nor
            // with them on their way to the file, as that save may fail.
            let edited = self.edited();
            let revision = self.editor.revision();
            cx.io.send(IoRequest::RecordDownload { file, edited });
            self.persist.download = Some(DocDownload {
                time: crate::when::now(),
                revision: (!edited).then_some(revision),
            });
        }
    }

    /// The lane's answer to recording a download of `file`: only a failure
    /// says anything.
    pub(crate) fn download_recorded(
        &mut self,
        file: FileId,
        result: Result<Option<LastDownload>, String>,
    ) {
        if self.persist.target.file() == Some(file)
            && let Err(error) = result
        {
            self.notice = Some(format!("Couldn't record the download: {error}"));
        }
    }

    /// Where the design stands against its downloads, as the file menu
    /// says, for a design in browser storage, with times relative to now.
    pub(crate) fn download_status(&self) -> Option<varde_view::Downloads> {
        if !matches!(self.persist.target, Target::Browser { .. }) {
            return None;
        }
        let revision = self.editor.revision();
        Some(match self.persist.download {
            None => varde_view::Downloads::Never,
            Some(download) => {
                let at = Some(crate::when::ago_in_sentence(
                    download.time,
                    crate::when::now(),
                ));
                let latest = download.revision == Some(revision)
                    && self.persist.saved_revision == Some(revision);
                if latest {
                    varde_view::Downloads::Latest(at)
                } else {
                    varde_view::Downloads::Changed(at)
                }
            }
        })
    }

    /// The last download of the design, as it's opened, see
    /// [`DocDownload`].
    pub(super) fn opened_download(&mut self, download: Option<LastDownload>) {
        let revision = self.editor.revision();
        self.persist.download = download.map(|download| DocDownload {
            time: download.time,
            revision: download.latest.then_some(revision),
        });
    }

    /// The answer to the store entry `id` asked for for a new design, see
    /// [`IoRequest::New`]: whether it was this document's.
    pub(crate) fn created(
        &mut self,
        cx: &mut Files,
        id: OpenId,
        result: &Result<FileId, String>,
    ) -> bool {
        if self.persist.creating != Some(id) {
            return false;
        }
        self.persist.creating = None;
        // Unless a Save As without it is on its way, which gives the
        // design a file of its own.
        let needed = self.persist.target == Target::None && !self.persist.saves.any();
        match *result {
            Ok(file) if needed => self.persist.target = Target::Entry { file },
            Ok(file) => cx.io.close_clean(file),
            // Shown: until saved, the design would be lost with the app
            // without a word.
            Err(ref error) if needed => self.auto_save_failed(error),
            Err(_) => {}
        }
        true
    }

    /// The Save As dialog of the document closed, with the file the user
    /// `chosen`, if they did.
    pub(super) fn save_as_picked(&mut self, cx: &mut Files, chosen: Option<Chosen>) -> Next {
        if self.persist.picking != Some(Picking::SaveAs) {
            return Next::Stay;
        }
        self.save_as_picked_with(cx, chosen)
    }

    /// The Save As dialog, the system's or the app's, closed with the file
    /// the user `chosen`, if they did.
    fn save_as_picked_with(&mut self, cx: &mut Files, chosen: Option<Chosen>) -> Next {
        self.persist.picking = None;
        match chosen {
            Some(chosen) => {
                let to = match chosen {
                    // The dialog only asked about replacing the file the
                    // user named, not one with the extension added.
                    Chosen::Path(path) => {
                        let (path, overwrite) = with_extension(path);
                        SaveTo::Path { path, overwrite }
                    }
                    Chosen::File(picked) => SaveTo::Picked(picked),
                    Chosen::Browser(name) => SaveTo::Browser {
                        name,
                        overwrite: false,
                    },
                };
                self.save_as(cx, to);
                Next::Stay
            }
            // Backing out of the dialog backs out of leaving too, if it was
            // waiting for it rather than for saves in flight. Leaving to
            // discard never waits for it, and goes on once its saves are
            // answered, which may have been while asking.
            None => {
                if !self.persist.saves.any() {
                    self.stop_leaving_unless_discarding();
                }
                self.resume_leaving(cx)
            }
        }
    }

    /// Takes the answer to a Save As to the file the user `chose`: the
    /// design goes on from that file, which is a recent file now.
    pub(crate) fn saved_as(
        &mut self,
        cx: &mut Files,
        chose: Chosen,
        revision: Revision,
        result: Result<varde_io::SavedAs, SaveError>,
    ) -> Next {
        // Another tab took the name meanwhile: asked about replacing it, as
        // the dialog would have, unless a newer save decides; leaving goes
        // on once that's answered.
        if let (Err(SaveError::Taken), Chosen::Browser(name)) = (&result, &chose) {
            self.answered(SaveKind::SaveAs, revision, false);
            if !self.persist.saves.supersede(SaveKind::SaveAs) && self.ask_replacing(name.clone()) {
                return self.resume_leaving(cx);
            }
        }
        let result = result.map(|saved| {
            let target = match chose {
                Chosen::Path(path) => {
                    self.name = design_name(&path);
                    self.path = Some(cx.shown_path(&path));
                    cx.remember(path);
                    Target::File {
                        file: saved.file,
                        picked: None,
                    }
                }
                // A file picked on the web isn't a recent file: there's no
                // keeping its handle.
                Chosen::File(picked) => {
                    self.name = design_name(Path::new(&picked.name));
                    self.path = None;
                    Target::File {
                        file: saved.file,
                        picked: Some(picked),
                    }
                }
                Chosen::Browser(name) => {
                    self.name = design_name(Path::new(&name));
                    self.path = None;
                    Target::Browser {
                        file: saved.file,
                        name,
                    }
                }
            };
            // Another design's downloads, unless saved over itself.
            let same = matches!((&self.persist.target, &target),
                (Target::Browser { name: was, .. }, Target::Browser { name, .. }) if was == name);
            if !same {
                self.persist.download = None;
            }
            self.persist.target = target;
            self.read_only = read_only(saved.access);
            // Answering it now would reach the new file, not the design
            // it's of.
            if !saved.offered {
                self.persist.recovered = None;
            }
            // Another file, or the damaged one written whole: what was
            // said of the damage is of the file left behind.
            self.persist.damage = None;
            self.persist.damaged_file = false;
        });
        let ok = result.is_ok();
        // The access may have changed, which the sketch's tool depends on.
        self.sync();
        self.saved(SaveKind::SaveAs, revision, result);
        // A new design whose store entry was closed, or failed, as this was
        // on its way has nowhere to be auto-saved to: it gets another.
        if !ok && self.persist.target == Target::None && self.persist.creating.is_none() {
            self.persist.creating = Some(cx.io.create());
        }
        self.resume_leaving(cx)
    }

    /// Leaves the document for `to`. Asks what to do about unsaved changes
    /// first, and waits for saves in flight.
    pub(crate) fn leave(&mut self, cx: &mut Files, to: Leave) -> Next {
        // Already on the way out.
        if let Some(leaving) = &mut self.persist.leaving {
            leaving.to = leaving.to.and(to);
            return Next::Stay;
        }
        self.go(cx, to, Changes::Keep)
    }

    /// Leaves the document for `to`, unless edits wait on the solver or
    /// the user is to be asked about unsaved changes first (unless they
    /// chose to discard them), or saves are still in flight: then it
    /// happens once they're answered.
    fn go(&mut self, cx: &mut Files, to: Leave, changes: Changes) -> Next {
        if changes == Changes::Keep && self.proposing() {
            self.file_menu = false;
            self.persist.leaving = Some(Leaving {
                to,
                step: Step::Proposing,
            });
            return Next::Stay;
        }
        if changes == Changes::Keep && self.unsaved() {
            self.file_menu = false;
            self.persist.leaving = Some(Leaving {
                to,
                step: Step::Asking,
            });
            return Next::Stay;
        }
        if self.persist.saves.any() {
            self.persist.leaving = Some(Leaving {
                to,
                step: Step::Saving(changes),
            });
            return Next::Stay;
        }
        // Its store entry is closed once made, see `Io::abandon`.
        cx.io.abandon(self.persist.creating.take());
        let target = mem::replace(&mut self.persist.target, Target::None);
        if let Some(file) = target.file() {
            // Changes offered to be restored and not answered are kept, to
            // be offered again. Anything else auto-saved is saved, or the
            // user chose not to save it. A store entry kept is listed
            // again by the lane, so the welcome screen shows it.
            let closing = match self.persist.recovered {
                None => Closing::Clean,
                Some(_) => Closing::Keep,
            };
            cx.io.send(IoRequest::Close { file, closing });
        }
        Next::Left(to)
    }

    /// Goes on leaving the document once its saves, or the edits waiting
    /// on the solver, are answered. A save failing has cancelled leaving
    /// already, see [`Doc::saved`].
    pub(crate) fn resume_leaving(&mut self, cx: &mut Files) -> Next {
        match self.persist.leaving {
            Some(Leaving {
                to,
                step: Step::Proposing,
            }) if !self.proposing() => {
                self.persist.leaving = None;
                self.go(cx, to, Changes::Keep)
            }
            // Not while asking where to save, or whether it may: the save
            // it's leaving with is still to come.
            Some(Leaving {
                to,
                step: Step::Saving(changes),
            }) if !self.persist.saves.any() && self.persist.picking.is_none() => {
                self.persist.leaving = None;
                self.go(cx, to, changes)
            }
            _ => Next::Stay,
        }
    }

    /// The user's answer to the prompt about unsaved changes.
    pub(crate) fn answer_unsaved(&mut self, cx: &mut Files, choice: Unsaved) -> Next {
        let Some(to) = self.prompt() else {
            return Next::Stay;
        };
        self.persist.leaving = None;
        match choice {
            Unsaved::Cancel => Next::Stay,
            Unsaved::Discard => self.go(cx, to, Changes::Discard),
            Unsaved::Save => {
                self.persist.leaving = Some(Leaving {
                    to,
                    step: Step::Saving(Changes::Keep),
                });
                self.save_or_ask(cx)
            }
        }
    }
}
