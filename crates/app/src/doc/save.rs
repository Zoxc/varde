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
use varde_document::name::with_extension;
use varde_document::{Command, EXTENSION, Revision, Snapshot};
use varde_io::{
    Chosen, Closing, FileId, OpenId, Picked, Request as IoRequest, SaveError, SaveTo, UnixSeconds,
};
use varde_view::Unsaved;

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
            leaving: None,
        }
    }
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
}

/// Which kind of save an answer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveKind {
    /// To the document's own file.
    Save,
    /// To a new file, or by downloading on the web.
    SaveAs,
}

/// Where a document is written, held by the IO lane.
#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    /// Nowhere yet: a new design whose store entry is on its way (see
    /// [`Persist::creating`]) or couldn't be made, or whose Save As is.
    None,
    /// The store entry of a design never saved as a file of the user's,
    /// which is only auto-saved to: Save asks where to.
    Entry {
        file: FileId,
        /// On the web, whether the entry holds the design as downloaded,
        /// see [`Doc::downloaded`], or may: one opened from the welcome
        /// screen may under the changes recovered. The page isn't told
        /// whether the user kept the download, so closing the design keeps
        /// the entry, gone back to the download if need be (see
        /// [`IoRequest::Close`]), and lists it again. Cleared by a save to
        /// a file, which empties the entry.
        downloaded: bool,
    },
    /// The file of the user's the design was opened from or last saved
    /// as, which Save writes.
    File {
        file: FileId,
        /// On the web, the file as picked, if it can be written to: the
        /// browser is asked whether it may be as the user saves, see
        /// [`Doc::save_design`].
        picked: Option<Picked>,
    },
}

impl Target {
    /// What the IO lane knows it by, if anything: what auto-saves go to
    /// and closing closes.
    pub(crate) fn file(&self) -> Option<FileId> {
        match *self {
            Target::None => None,
            Target::Entry { file, .. } | Target::File { file, .. } => Some(file),
        }
    }

    /// The design's own file, if it has been saved as one, rather than a
    /// store entry or nothing.
    pub(crate) fn design_file(&self) -> Option<FileId> {
        match *self {
            Target::File { file, .. } => Some(file),
            _ => None,
        }
    }
}

/// Hands a design to the browser as a download, by the file name and its
/// bytes, see [`Doc::downloaded`]: [`varde_io::pick::downloader`]'s, or a
/// test's.
pub(crate) type Downloader = Box<dyn Fn(&str, &[u8]) -> Result<(), String>>;

/// The design as downloaded on the web, to be kept in its store entry.
#[derive(Debug)]
pub(crate) struct Download {
    /// The editor revision downloaded.
    pub(crate) revision: Revision,
    pub(crate) document: Snapshot,
}

impl Download {
    /// Keeps it in `file`, its store entry, marked as downloaded.
    pub(crate) fn request(self, file: FileId) -> IoRequest {
        IoRequest::KeepDownload {
            file,
            revision: self.revision,
            document: self.document,
        }
    }
}

/// Designs downloaded just after New, before their store entry was made,
/// by the tag of the [`IoRequest::New`] making it: the design as
/// downloaded is kept in the entry once it's made, see [`Doc::created`].
/// Closed meanwhile, the entry isn't given up on, but closed cleanly once
/// it has the download, see [`Downloads::settle`].
#[derive(Debug, Default)]
pub(crate) struct Downloads(Vec<(OpenId, Download)>);

impl Downloads {
    /// Keeps `download` for entry `id`, in place of an older one.
    fn keep(&mut self, id: OpenId, download: Download) {
        self.0.retain(|(awaited, _)| *awaited != id);
        self.0.push((id, download));
    }

    /// Takes the answer to the store entry `id` asked for for a document
    /// gone since. The lane closes the entry once told so, see
    /// `Io::abandon`, unless it's to keep a download: downloaded, and
    /// closed before the entry was made, the clean close keeps it, as the
    /// design as downloaded.
    pub(crate) fn settle(&mut self, io: &mut Io, id: OpenId, result: Result<FileId, String>) {
        if let Some(download) = self.take(id)
            && let Ok(file) = result
        {
            io.send(download.request(file));
            // Listed by the lane once closed.
            io.close_clean(file);
        }
    }

    /// Takes the download kept for entry `id`, if there is one.
    fn take(&mut self, id: OpenId) -> Option<Download> {
        let at = self.0.iter().position(|(awaited, _)| *awaited == id)?;
        Some(self.0.remove(at).1)
    }

    /// Whether a download is kept for entry `id`.
    fn awaits(&self, id: OpenId) -> bool {
        self.0.iter().any(|(awaited, _)| *awaited == id)
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

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
    /// To the file the user chose.
    SaveAs(Chosen),
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

    /// The design's name as the window's title shows it: with its
    /// extension, marked "(damaged file)" while its file was opened past
    /// damage.
    pub(crate) fn title_name(&self) -> String {
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

    /// Stops waiting on the user or the browser, as if they never answer.
    #[cfg(test)]
    pub(crate) fn forget_picking(&mut self) {
        self.persist.picking = None;
    }

    /// Leaving the document, see [`Persist::leaving`].
    #[cfg(test)]
    pub(crate) fn leaving(&self) -> Option<Leaving> {
        self.persist.leaving
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
                Deferred::SaveAs(chose) => self.save_as(cx, chose),
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
        self.persist.saves.save = Some(revision);
        self.persist.save_error = None;
        self.persist.auto_save.saving(revision);
    }

    /// Sends the document to the new file the user `chose` in the Save As
    /// dialog, which asked about replacing the one there.
    fn save_as(&mut self, cx: &mut Files, chose: Chosen) {
        if self.defer(Deferred::SaveAs(chose.clone()))
            || self.defer_for_thumbnail(Deferred::SaveAs(chose.clone()))
        {
            return;
        }
        let revision = self.editor.revision();
        let (file, document) = (self.persist.target.file(), self.editor.snapshot());
        let to = match chose {
            Chosen::Path(path) => {
                // The dialog only asked about replacing the file the user
                // named, not one with the extension added.
                let (path, overwrite) = with_extension(path);
                SaveTo::Path { path, overwrite }
            }
            Chosen::File(picked) => SaveTo::Picked(picked),
        };
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
            // so this is the newest one to land. A download, booked here
            // at once, can't overtake one: saving downloads only where
            // the lane has no file of the user's to save to.
            Ok(()) => {
                self.persist.saved_revision = Some(revision);
                // Sending it cleared the save error, so one there now is
                // of a Save As failing since, which a Save doesn't
                // supersede.
                if kind == SaveKind::SaveAs {
                    self.persist.save_error = None;
                }
                self.persist.auto_save_error = None;
                // The lane emptied the entry. A download marks it again
                // after this, see `Doc::downloaded`.
                if let Target::Entry { downloaded, .. } = &mut self.persist.target {
                    *downloaded = false;
                }
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
    /// sidecar or store entry is emptied, as by a save, or on the web goes
    /// back to the design as downloaded, which it's at. Not while offering
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
        match self.persist.target {
            Target::Entry {
                downloaded: true, ..
            } => io.send(
                Download {
                    revision,
                    document: self.editor.snapshot(),
                }
                .request(file),
            ),
            _ => io.send(IoRequest::DiscardRecovery { file }),
        }
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

    /// Sends `download`, the design as downloaded, to be kept in its
    /// store entry, if it has one, see [`Target::Entry`]. Auto-saves of
    /// edits since follow it as usual.
    fn send_download(&mut self, io: &mut Io, download: Download) {
        if let Target::Entry { file, downloaded } = &mut self.persist.target {
            self.persist.auto_save.sent = Some(download.revision);
            io.send(download.request(*file));
            *downloaded = true;
        }
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
    /// [`Doc::save_as_picked`].
    pub(crate) fn request_save_as(&mut self, cx: &mut Files) -> Next {
        self.file_menu = false;
        // A Save As of a design never saved still on its way would make a
        // second file, so this waits for it like `Doc::request_save` does.
        let new_on_its_way =
            self.persist.target.design_file().is_none() && self.persist.saves.any();
        if self.persist.picking.is_some() || new_on_its_way {
            return Next::Stay;
        }
        if let Some(download) = &cx.downloader {
            let revision = self.editor.revision();
            let result = self.download(download);
            return self.downloaded(cx, revision, result);
        }
        self.persist.picking = Some(Picking::SaveAs);
        Next::PickSaveAs {
            id: self.id,
            name: self.name.clone(),
        }
    }

    /// Hands the document to the browser with `download`, encoded on the
    /// page: one encoding and one pass of snappy, cheaper than sending it
    /// to the lane and having the file sent back.
    fn download(&self, download: &Downloader) -> Result<(), String> {
        let name = varde_document::name::download_name(&self.name);
        varde_io::vrdp::to_bytes(self.editor.document(), &[])
            .map_err(|e| e.to_string())
            .and_then(|(bytes, _)| download(&name, &bytes))
    }

    /// The document was saved by downloading `revision`, on the web without
    /// the File System Access API, with `result` as the browser's answer.
    /// Once downloaded it counts as saved: the browser has it, and there's
    /// nothing else to tell.
    ///
    /// Whether the user kept the download isn't something the page is
    /// told: they may have cancelled the browser's save dialog. So rather
    /// than being emptied as by a save, the design's store entry gets the
    /// design as downloaded, marked so, and closing keeps it, going back to
    /// it should the user choose not to save changes made since, see
    /// [`IoRequest::Close`]. The welcome screen lists it apart from designs
    /// never saved, to open again or discard.
    fn downloaded(
        &mut self,
        cx: &mut Files,
        revision: Revision,
        result: Result<(), String>,
    ) -> Next {
        let ok = result.is_ok();
        // Booked as a Save As, which it stands in for.
        self.saved(
            SaveKind::SaveAs,
            revision,
            result.map_err(SaveError::Failed),
        );
        // Kept in the entry, marked as downloaded: after the auto-saves
        // sent before it, and before any of a newer revision. What a
        // crashed session left is kept until the user answers the offer, as
        // by a Save.
        if ok && self.persist.recovered.is_none() {
            let download = Download {
                revision,
                document: self.editor.snapshot(),
            };
            match (&self.persist.target, self.persist.creating) {
                // Just after New: kept once the entry is made, in place of
                // an older download. Should making it fail, the design
                // isn't auto-saved at all, which the banner says.
                (Target::None, Some(id)) => cx.downloads.keep(id, download),
                _ => self.send_download(&mut cx.io, download),
            }
        }
        self.resume_leaving(cx)
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
        let download = cx.downloads.take(id);
        // Unless a Save As without it is on its way, which gives the
        // design a file of its own.
        let needed = self.persist.target == Target::None && !self.persist.saves.any();
        match *result {
            Ok(file) if needed => {
                self.persist.target = Target::Entry {
                    file,
                    downloaded: false,
                };
                // Before any auto-save, which needs the entry.
                if let Some(download) = download {
                    self.send_download(&mut cx.io, download);
                }
            }
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
        self.persist.picking = None;
        match chosen {
            Some(chosen) => {
                self.save_as(cx, chosen);
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
        let result = result.map(|saved| {
            let picked = match chose {
                Chosen::Path(path) => {
                    self.name = design_name(&path);
                    cx.remember(path);
                    None
                }
                // A file picked on the web isn't a recent file: there's no
                // keeping its handle.
                Chosen::File(picked) => {
                    self.name = design_name(Path::new(&picked.name));
                    Some(picked)
                }
            };
            self.persist.target = Target::File {
                file: saved.file,
                picked,
            };
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
        // Its store entry is closed once made, see `Io::abandon`, unless
        // it's to keep a download, see `Downloads`.
        let creating = self.persist.creating.take();
        if !creating.is_some_and(|id| cx.downloads.awaits(id)) {
            cx.io.abandon(creating);
        }
        let target = mem::replace(&mut self.persist.target, Target::None);
        if let Some(file) = target.file() {
            // Changes offered to be restored and not answered are kept, to
            // be offered again. Anything else auto-saved is saved, or the
            // user chose not to save it, but for the design as downloaded,
            // which the lane keeps, see `IoRequest::Close`: the download
            // may not have been kept. A store entry kept is listed again
            // by the lane, so the welcome screen shows it.
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
