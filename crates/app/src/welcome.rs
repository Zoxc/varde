//! The welcome screen: making a new design and opening one, from a file,
//! the recent files list or what crashed sessions left behind, and on the
//! web from browser storage.

use std::path::{Path, PathBuf};

use iced::Element;
use varde_document::Document;
use varde_document::name::UNTITLED;
use varde_io::{
    Access, BrowserDesign, Chosen, Closing, Damage, DamageKind, DownloadStatus, ListedDamage,
    OpenId, Opened, PickedFrom, Recovered, RecoveryError, Request as IoRequest, UnixSeconds,
};
use varde_view::{Message as Ui, Mode, ThemeChoice, Welcome as WelcomeUi};

use crate::doc::{Dialog, Doc, FileDamage, Leave, Origin, Recovery, Target, design_name};
use crate::{Files, Next, when};

/// State of the welcome screen.
#[derive(Default)]
pub(crate) struct Welcome {
    /// Why the last open failed, if it did.
    error: Option<String>,
    /// Whether the Open dialog is showing.
    picking: bool,
    /// The open the screen is waiting for. Answers to other opens are
    /// stale: the user has moved on from them, and they were abandoned,
    /// see [`Io::abandon`].
    ///
    /// [`Io::abandon`]: crate::io::Io::abandon
    opening: Option<Opening>,
    /// The file just opened and found damaged, asked about before its
    /// design shows, if one is.
    damaged: Option<Box<Damaged>>,
    /// Whether the whole of the panic recorded is showing.
    showing_panic: bool,
    /// On the web, whether a file is dragged over the page, which lights
    /// the drop zone.
    dragging: bool,
    /// On the web, the design in browser storage the user is asked about
    /// deleting, by its file name there, see [`Welcome::delete_from_browser`].
    deleting: Option<String>,
}

/// A file opened and found damaged past the save opened (see
/// [`DamageKind::Damaged`]), whose design waits for the user to choose:
/// the save opened, the one found after the damage, if one was, or
/// neither. Its file is open in the IO lane meanwhile, and nothing is
/// auto-saved, as there's no document.
struct Damaged {
    source: Source,
    /// The path the lane made absolute, if it has one.
    path: Option<PathBuf>,
    /// The name a store entry's design is known by, as listed when it was
    /// opened: the lane lists the store again without the entry, which it
    /// holds now.
    title: Option<String>,
    opened: Opened,
    damage: Damage,
    /// The tag of the [`IoRequest::OpenFound`] asked for, until answered.
    finding: Option<OpenId>,
    /// Why opening the save found failed, if it did.
    error: Option<String>,
}

/// An open the welcome screen is waiting for.
struct Opening {
    id: OpenId,
    source: Source,
}

/// What an open opens.
#[derive(Clone, PartialEq)]
pub(crate) enum Source {
    /// A file of the user's: a path from the recent files list or the Open
    /// dialog, or on the web the file the user picked, or a design in
    /// browser storage.
    Chosen(Chosen),
    /// The store entry of a new design left behind by a crash.
    Recovered(PathBuf),
}

impl Source {
    /// The request opening it, tagged `id`.
    pub(crate) fn request(&self, id: OpenId) -> IoRequest {
        match self.clone() {
            Source::Chosen(from) => IoRequest::Open { id, from },
            Source::Recovered(path) => IoRequest::OpenRecovered { id, path },
        }
    }
}

impl Welcome {
    /// Takes `message`, from the welcome screen.
    pub(crate) fn update(&mut self, files: &mut Files, message: WelcomeUi) -> Next {
        match message {
            WelcomeUi::NewDesign => self.new_design(files),
            WelcomeUi::Open => self.pick(),
            WelcomeUi::OpenPath(path) => self.open(files, Source::Chosen(Chosen::Path(path))),
            WelcomeUi::OpenStored(path) => self.open(files, Source::Recovered(path)),
            WelcomeUi::DiscardStored(path) => self.discard_recovered(files, path),
            WelcomeUi::OpenFromBrowser(name) => {
                self.open(files, Source::Chosen(Chosen::Browser(name)))
            }
            WelcomeUi::DeleteFromBrowser(name) => self.delete_from_browser(files, name),
            WelcomeUi::DownloadFromBrowser(name) => {
                files.io.send(IoRequest::DownloadFromBrowser { name });
                Next::Stay
            }
            WelcomeUi::ConfirmDelete => {
                if let Some(name) = self.deleting.take() {
                    delete(files, name);
                }
                Next::Stay
            }
            WelcomeUi::CancelDelete => {
                self.deleting = None;
                Next::Stay
            }
            WelcomeUi::OpenDamaged => self.open_damaged(files),
            WelcomeUi::OpenFound => self.open_found(files),
            WelcomeUi::CancelDamaged => {
                self.cancel_damaged(files);
                Next::Stay
            }
            WelcomeUi::ShowPanic => {
                self.showing_panic = files.panic.is_some();
                Next::Stay
            }
            WelcomeUi::ClosePanic => {
                self.showing_panic = false;
                Next::Stay
            }
            WelcomeUi::DiscardPanic => {
                self.showing_panic = false;
                if let Some(panic) = files.panic.take() {
                    files.io.send(IoRequest::DiscardPanic { panic });
                }
                Next::Stay
            }
        }
    }

    /// Whether a damaged file is being asked about, see [`Damaged`].
    pub(crate) fn prompting(&self) -> bool {
        self.damaged.is_some()
    }

    /// The dialog over the screen, if one is showing: no key opens
    /// anything meanwhile, and `Esc` cancels it.
    pub(crate) fn dialog(&self) -> Option<Dialog> {
        if self.prompting() {
            Some(Dialog::Damaged)
        } else if self.deleting.is_some() {
            Some(Dialog::DeleteFromBrowser)
        } else if self.showing_panic {
            Some(Dialog::Panic)
        } else {
            None
        }
    }

    /// Opens the newest save that can be read of the damaged file asked
    /// about, unless the save found is on its way.
    fn open_damaged(&mut self, files: &mut Files) -> Next {
        match self.damaged.take_if(|damaged| damaged.finding.is_none()) {
            Some(damaged) => Next::Show(Box::new(show(
                files,
                damaged.source,
                damaged.path,
                damaged.title,
                damaged.opened,
            ))),
            None => Next::Stay,
        }
    }

    /// Asks the lane for the save found after the damage of the file asked
    /// about instead, answered like an open, see [`Welcome::found`].
    fn open_found(&mut self, files: &mut Files) -> Next {
        if let Some(damaged) = &mut self.damaged
            && damaged.finding.is_none()
            && let DamageKind::Damaged { found: Some(found) } = damaged.damage.kind
        {
            let id = files.io.tag();
            files.io.send(IoRequest::OpenFound {
                id,
                file: damaged.opened.file,
                found: found.tail,
            });
            damaged.finding = Some(id);
            damaged.error = None;
        }
        Next::Stay
    }

    /// Leaves the damaged file asked about as it is, if one is: the lane
    /// closes it, keeping what a crashed session left beside it for next
    /// time. A copy of a file from a file input, just made in browser
    /// storage to open it, goes again: the user didn't open it after all.
    fn cancel_damaged(&mut self, files: &mut Files) {
        let Some(damaged) = self.damaged.take() else {
            return;
        };
        let copy = match (&damaged.source, damaged.opened.browser) {
            (Source::Chosen(Chosen::File(_)), Some(name)) => Some(name),
            _ => None,
        };
        let closing = match copy {
            Some(_) => Closing::Clean,
            None => Closing::Keep,
        };
        files.io.send(IoRequest::Close {
            file: damaged.opened.file,
            closing,
        });
        if let Some(name) = copy {
            files.io.send(IoRequest::DeleteFromBrowser { name });
        }
    }

    /// Makes a new design.
    fn new_design(&mut self, files: &mut Files) -> Next {
        // Not with one on its way as a document opened, which would
        // replace it without closing it.
        self.give_up(files);
        let origin = Origin {
            // Auto-saves go to a store entry the lane makes for it.
            creating: Some(files.io.create()),
            ..Origin::new(Target::None, Access::Edit, UNTITLED.to_owned())
        };
        Next::Show(Box::new(Doc::new(Document::default(), origin)))
    }

    /// Opens `source`. Not a store entry listed as one nothing can be read
    /// of, which can only be discarded.
    fn open(&mut self, files: &mut Files, source: Source) -> Next {
        if let Source::Recovered(path) = &source
            && files.recovered.iter().any(|design| {
                design.path == *path && design.damage == Some(ListedDamage::Unreadable)
            })
        {
            return Next::Stay;
        }
        // Clicking a file that is already on its way doesn't open it again.
        if self.opening.as_ref().is_none_or(|o| o.source != source) {
            self.error = None;
            self.give_up(files);
            let id = files.io.tag();
            files.io.send(source.request(id));
            self.opening = Some(Opening { id, source });
        }
        Next::Stay
    }

    /// Shows the Open dialog, unless it's showing already.
    fn pick(&mut self) -> Next {
        if self.picking {
            return Next::Stay;
        }
        self.picking = true;
        Next::PickOpen
    }

    /// The Open dialog closed, with the file the user `chosen`, if they
    /// did. Ignored if the user went on to something else while the
    /// dialog was up, as the welcome screen isn't showing then.
    pub(crate) fn picked(&mut self, files: &mut Files, chosen: Option<Chosen>) -> Next {
        self.picking = false;
        match chosen {
            Some(chosen) => self.open(files, Source::Chosen(chosen)),
            None => Next::Stay,
        }
    }

    /// On the web, a file was dragged over the page, or off it again
    /// (`over`): the drop zone lights while one is, unless a dialog is up.
    pub(crate) fn dragged(&mut self, over: bool) -> Next {
        self.dragging = over && self.dialog().is_none();
        Next::Stay
    }

    /// On the web, the file `chosen` was dropped on the page, if it was a
    /// file: opened as one picked with Open… would be, from a file input,
    /// as a copy. Not while a dialog is up, over which it was dropped: then
    /// it's let go of, see [`forget`].
    pub(crate) fn dropped(&mut self, files: &mut Files, chosen: Option<Chosen>) -> Next {
        self.dragging = false;
        match chosen {
            Some(chosen) if self.dialog().is_none() => self.open(files, Source::Chosen(chosen)),
            Some(chosen) => {
                forget(&chosen);
                Next::Stay
            }
            None => Next::Stay,
        }
    }

    /// Forgets the new design at `path` left behind by a crash.
    fn discard_recovered(&mut self, files: &mut Files, path: PathBuf) -> Next {
        if self.prompting() {
            return Next::Stay;
        }
        // Gone from the list at once. The lane lists what's left after.
        files.recovered.retain(|design| design.path != path);
        files.io.send(IoRequest::DiscardRecovered { path });
        Next::Stay
    }

    /// Deletes the design `name` in browser storage: at once if its latest
    /// is downloaded, otherwise once the user agrees, as this browser may
    /// have the only copy. Not one another tab has open, whose card offers
    /// no Delete.
    fn delete_from_browser(&mut self, files: &mut Files, name: String) -> Next {
        let listed = (files.browser.iter()).find(|design| design.name == name);
        if self.dialog().is_some() || listed.is_some_and(|design| design.in_use) {
            return Next::Stay;
        }
        let latest =
            listed.is_some_and(|design| matches!(design.download, DownloadStatus::Latest(_)));
        if latest {
            delete(files, name);
        } else {
            self.deleting = Some(name);
        }
        Next::Stay
    }

    /// The design asked about deleting, if one is.
    #[cfg(test)]
    pub(crate) fn deleting(&self) -> Option<&str> {
        self.deleting.as_deref()
    }

    /// Something asked of the files from the welcome screen failed: says
    /// `error`, a sentence.
    pub(crate) fn failed(&mut self, error: &str) -> Next {
        self.error = Some(error.to_owned());
        Next::Stay
    }

    /// Discarding a recovered design failed, with `error`.
    pub(crate) fn discard_failed(&mut self, error: &str) -> Next {
        self.failed(&format!("Couldn't discard the design: {error}"))
    }

    /// Leaves the welcome screen for `to`: only quitting does, which gives
    /// up on the open in flight.
    pub(crate) fn leave(&mut self, files: &mut Files, to: Leave) -> Next {
        match to {
            Leave::Close => Next::Stay,
            Leave::Quit(_) => {
                self.give_up(files);
                Next::Left(to)
            }
        }
    }

    /// Stops waiting for the open in flight, if any: the lane closes what
    /// it opened, see [`Io::abandon`]. A damaged file being asked about is
    /// left as it is, as by Cancel.
    ///
    /// [`Io::abandon`]: crate::io::Io::abandon
    fn give_up(&mut self, files: &mut Files) {
        files
            .io
            .abandon(self.opening.take().map(|opening| opening.id));
        self.cancel_damaged(files);
    }

    /// Takes the answer to the open tagged `id`, of `path` as the lane
    /// made it absolute, if it has one. A file found damaged past the save
    /// opened is asked about first, see [`Damaged`].
    pub(crate) fn opened(
        &mut self,
        files: &mut Files,
        id: OpenId,
        path: Option<PathBuf>,
        result: Result<Opened, String>,
    ) -> Next {
        if self
            .damaged
            .as_ref()
            .is_some_and(|damaged| damaged.finding == Some(id))
        {
            return self.found(files, result);
        }
        // An answer the screen isn't waiting for is stale: the user moved
        // on, and the lane closes the file once told so, see `Io::abandon`.
        let Some(opening) = self.opening.take_if(|o| o.id == id) else {
            return Next::Stay;
        };
        let opened = match result {
            Ok(opened) => opened,
            Err(error) => {
                self.error = Some(match (opening.source, path) {
                    // The lane lists what's there now, as the entry may be
                    // gone or changed.
                    (Source::Recovered(_), _) => {
                        format!("Couldn't open the recovered design: {error}")
                    }
                    (Source::Chosen(Chosen::Path(asked)), path) => {
                        let path = path.unwrap_or(asked);
                        format!("Couldn't open {}: {error}", path.display())
                    }
                    (Source::Chosen(Chosen::File(picked)), _) => {
                        format!("Couldn't open {}: {error}", picked.name)
                    }
                    (Source::Chosen(Chosen::Browser(name)), _) => {
                        format!("Couldn't open {name}: {error}")
                    }
                });
                return Next::Stay;
            }
        };
        if let Some(damage) = opened.damage
            && matches!(damage.kind, DamageKind::Damaged { .. })
        {
            let title = match &opening.source {
                Source::Recovered(path) => Some(files.recovered_title(path)),
                Source::Chosen(_) => None,
            };
            // Asked about in place of the panic shown, which isn't
            // shown again after.
            self.showing_panic = false;
            self.damaged = Some(Box::new(Damaged {
                source: opening.source,
                path,
                title,
                opened,
                damage,
                finding: None,
                error: None,
            }));
            return Next::Stay;
        }
        Next::Show(Box::new(show(files, opening.source, path, None, opened)))
    }

    /// Takes the answer to the save found after the damage of the file
    /// asked about: shown in its place, or why it couldn't be opened, in
    /// the prompt, which then offers the save opened alone.
    fn found(&mut self, files: &mut Files, result: Result<Opened, String>) -> Next {
        let Some(mut damaged) = self.damaged.take() else {
            return Next::Stay;
        };
        match result {
            Ok(opened) => Next::Show(Box::new(show(
                files,
                damaged.source,
                damaged.path,
                damaged.title,
                opened,
            ))),
            Err(error) => {
                damaged.finding = None;
                damaged.error = Some(error);
                damaged.damage.kind = DamageKind::Damaged { found: None };
                self.damaged = Some(damaged);
                Next::Stay
            }
        }
    }

    /// Whether the drop zone is lit, a file dragged over the page.
    #[cfg(test)]
    pub(crate) fn dragging(&self) -> bool {
        self.dragging
    }

    /// Why the last open failed, if it did.
    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The welcome screen, listing the recent files and the recovered
    /// designs of `files`.
    pub(crate) fn view<'a>(
        &'a self,
        files: &'a Files,
        mode: Mode,
        theme: ThemeChoice,
    ) -> Element<'a, Ui> {
        let now = when::now();
        // Where designs are kept in browser storage, the screen is the
        // web's page, which has no recent files.
        let recent = (RECENT_FILES && !files.browser_storage).then(|| {
            files
                .recent
                .entries()
                .iter()
                .map(|listed| varde_view::RecentCard {
                    path: &listed.entry.path,
                    name: design_name(&listed.entry.path),
                    extension: listed
                        .entry
                        .path
                        .extension()
                        .map_or_else(String::new, |s| format!(".{}", s.to_string_lossy())),
                    dir: files.recent.display_dir(&listed.entry.path),
                    opened: when::ago(listed.entry.opened, now),
                    available: listed.available,
                    thumbnail: files.thumbnail(&listed.entry.path),
                })
                .collect()
        });
        // Newest first: on the web the designs saved in browser storage
        // among the new designs left behind.
        let mut stored: Vec<(Option<UnixSeconds>, varde_view::DesignCard<'a>)> =
            (files.recovered.iter())
                .map(|design| (design.modified, recovered_card(design, now)))
                .collect();
        stored.extend(
            (files.browser.iter()).map(|design| (design.saved, browser_card(files, design, now))),
        );
        stored.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
        let stored = stored.into_iter().map(|(_, design)| design).collect();
        let deleting = self
            .deleting
            .as_deref()
            .map(|name| varde_view::DeleteFromBrowserPrompt {
                name,
                downloads: (files.browser.iter())
                    .find(|design| design.name == name)
                    .map_or(varde_view::Downloads::Never, |design| {
                        downloads(design.download, now)
                    }),
            });
        let storage = files.browser_storage.then(|| varde_view::StorageNote {
            may_clear: files.storage.persisted == Some(false),
            used: files.storage.space.map(space_used),
        });
        let damaged = self
            .damaged
            .as_ref()
            .map(|damaged| damaged.prompt(files, now));
        let panic = files.panic.as_ref().map(|panic| varde_view::PanicNote {
            when: panic.time.map(|time| when::ago_since_a_session(time, now)),
            message: &panic.message,
            report: self.showing_panic.then(|| panic.report()),
        });
        varde_view::welcome(varde_view::WelcomeState {
            error: self.error.as_deref(),
            recent,
            stored,
            storage,
            deleting,
            dragging: self.dragging,
            damaged,
            panic,
            mode,
            theme,
        })
    }
}

impl Damaged {
    /// The prompt asking about it, with times relative to `now`.
    fn prompt(&self, files: &Files, now: UnixSeconds) -> varde_view::DamagedPrompt<'_> {
        let name = match &self.source {
            Source::Chosen(Chosen::Path(asked)) => {
                let path = self.path.as_deref().unwrap_or(asked);
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                )
            }
            Source::Chosen(Chosen::File(picked)) => picked.name.clone(),
            Source::Chosen(Chosen::Browser(name)) => name.clone(),
            Source::Recovered(path) => {
                let title = (self.title.clone()).unwrap_or_else(|| files.recovered_title(path));
                format!("the recovered {title}")
            }
        };
        let found = match self.damage.kind {
            DamageKind::Damaged { found } => found,
            _ => None,
        };
        varde_view::DamagedPrompt {
            name,
            unreadable: kilobytes(self.damage.unreadable),
            from: when::ago_in_sentence(self.damage.time, now),
            found: found.map(|found| when::ago_in_sentence(found.time, now)),
            auto_saves: matches!(self.source, Source::Recovered(_)),
            opening: self.finding.is_some(),
            error: self.error.as_deref(),
        }
    }
}

/// The document `opened` from `source`, at `path` as the lane made it
/// absolute, if it has one. A store entry's design is known by `title`,
/// if it's known already, else as it's listed.
fn show(
    files: &mut Files,
    source: Source,
    path: Option<PathBuf>,
    title: Option<String>,
    opened: Opened,
) -> Doc {
    // Of a file of the user's: a store entry's is said of auto-saves.
    let damage = |entry| opened.damage.map(|damage| FileDamage { damage, entry });
    match source {
        Source::Chosen(Chosen::Browser(name)) => in_browser(opened, name),
        // Picked on the web: the design goes on from the file if it can
        // be written to. One only read is copied into browser storage by
        // the lane, and goes on from there, which the status bar says, and
        // the browser is asked to keep it; should copying it fail, it's a
        // copy, a new design, saying why.
        Source::Chosen(Chosen::File(picked)) => {
            if let Some(name) = opened.browser.clone() {
                files.ask_persist = true;
                let mut doc = in_browser(opened, name.clone());
                doc.notice = Some(format!("Copied to browser storage as {name}"));
                return doc;
            }
            let title = design_name(Path::new(&picked.name));
            let file = opened.file;
            let target = match picked.from {
                PickedFrom::Handle => Target::File {
                    file,
                    picked: Some(picked),
                },
                PickedFrom::Input => Target::Entry { file },
            };
            let origin = Origin {
                recovered: offered(opened.recovered, &title),
                damage: damage(false),
                ..Origin::new(target, opened.access, title)
            };
            let mut doc = Doc::new(opened.document, origin);
            doc.notice =
                (opened.not_copied).map(|why| format!("Opened as a copy, not kept: {why}"));
            doc
        }
        Source::Chosen(Chosen::Path(asked)) => {
            let path = path.unwrap_or(asked);
            files.remember(path.clone());
            let recovered = offered(opened.recovered, &path.display().to_string());
            let target = Target::File {
                file: opened.file,
                picked: None,
            };
            let origin = Origin {
                recovered,
                damage: damage(false),
                ..Origin::new(target, opened.access, design_name(&path))
            };
            let mut doc = Doc::new(opened.document, origin);
            doc.path = Some(files.shown_path(&path));
            doc
        }
        // A new design left behind by a crash: never saved, and its
        // store entry is auto-saved to from now on. Known by the name
        // of the file it was opened from, if it was, as on the web.
        Source::Recovered(path) => {
            let title = title.unwrap_or_else(|| files.recovered_title(&path));
            files.recovered.retain(|design| design.path != path);
            let target = Target::Entry { file: opened.file };
            let origin = Origin {
                saved: false,
                damage: damage(true),
                ..Origin::new(target, opened.access, title)
            };
            Doc::new(opened.document, origin)
        }
    }
}

/// The document `opened` from browser storage, where it's called `name`
/// (as the lane answers, see [`Opened::browser`]).
fn in_browser(opened: Opened, name: String) -> Doc {
    let name = opened.browser.unwrap_or(name);
    let title = design_name(Path::new(&name));
    let target = Target::Browser {
        file: opened.file,
        name: name.clone(),
    };
    let origin = Origin {
        recovered: offered(opened.recovered, &name),
        damage: opened.damage.map(|damage| FileDamage {
            damage,
            entry: false,
        }),
        download: opened.download,
        ..Origin::new(target, opened.access, title)
    };
    Doc::new(opened.document, origin)
}

/// Lets go of what the page holds for `chosen`, a file dropped on it that
/// isn't opened: kept as it was dropped, it would be held for the rest of
/// the session otherwise, see [`varde_io::pick::forget`].
pub(crate) fn forget(chosen: &Chosen) {
    if let Chosen::File(picked) = chosen {
        varde_io::pick::forget(picked);
        #[cfg(test)]
        FORGOTTEN.with_borrow_mut(|forgotten| forgotten.push(picked.clone()));
    }
}

#[cfg(test)]
thread_local! {
    /// What [`forget`] let go of, as natively it lets go of nothing.
    pub(crate) static FORGOTTEN: std::cell::RefCell<Vec<varde_io::Picked>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// `bytes` in kilobytes, as the prompt about a damaged file says how much
/// can't be read: rounded up, so that any is at least "1 KB".
fn kilobytes(bytes: u64) -> String {
    format!("{} KB", bytes.div_ceil(1024).max(1))
}

/// What a crashed session left of a design, from `recovered` as the lane
/// read it for the design `name`: an error is logged, and the design opens
/// without it, kept if it can't be read.
fn offered(
    recovered: Result<Option<varde_io::Offer>, RecoveryError>,
    name: &str,
) -> Option<Recovery> {
    match recovered {
        Ok(offer) => offer.map(Recovery::Offered),
        Err(error) => {
            log::error!("Ignored what was auto-saved of {name}: {error}");
            error.kept.then_some(Recovery::Kept)
        }
    }
}

impl Files {
    /// Asks the lane for the recent files' thumbnails, shown on the
    /// welcome screen once they arrive: as the list loads, and as the
    /// welcome screen shows again, a design maybe saved since.
    pub(crate) fn load_thumbnails(&mut self) {
        let paths: Vec<_> = (self.recent.entries().iter())
            .map(|listed| listed.entry.path.clone())
            .collect();
        if RECENT_FILES && !paths.is_empty() {
            self.io.send(IoRequest::LoadThumbnails { paths });
        }
    }

    /// The thumbnail of the recent file at `path`, if it has one.
    fn thumbnail(&self, path: &Path) -> Option<iced::widget::image::Handle> {
        (self.thumbnails.iter())
            .find(|(at, _)| at == path)
            .map(|(_, handle)| handle.clone())
    }

    /// The name the recovered design at `path` is known by, see
    /// [`recovered_name`], or Untitled if it isn't listed.
    fn recovered_title(&self, path: &Path) -> String {
        self.recovered
            .iter()
            .find(|design| design.path == path)
            .map_or_else(|| UNTITLED.to_owned(), recovered_name)
    }
}

/// The name a recovered design is known by: that of the file it was
/// opened from, if it was, as on the web.
fn recovered_name(design: &Recovered) -> String {
    design
        .name
        .as_deref()
        .map_or_else(|| UNTITLED.to_owned(), |file| design_name(Path::new(file)))
}

/// The welcome screen's card for `design`, a new design left behind, with
/// when it was last written relative to `now`.
fn recovered_card(design: &Recovered, now: UnixSeconds) -> varde_view::DesignCard<'_> {
    varde_view::DesignCard {
        key: varde_view::CardKey::Recovered(&design.path),
        name: recovered_name(design),
        file: design.name.is_some(),
        written: design.modified.map(|modified| when::ago(modified, now)),
        downloads: None,
        damaged: design.damage.is_some(),
        opens: design.damage != Some(ListedDamage::Unreadable),
        thumbnail: None,
        note: None,
        deletable: true,
    }
}

/// The welcome screen's card for `design`, saved in browser storage, with
/// times relative to `now`: where it stands against its downloads, and
/// whether a closed tab left changes in it or another tab has it open.
fn browser_card<'a>(
    files: &'a Files,
    design: &'a BrowserDesign,
    now: UnixSeconds,
) -> varde_view::DesignCard<'a> {
    let readable = design.damage != Some(ListedDamage::Unreadable);
    let note = if design.in_use {
        Some("Open in another tab")
    } else if design.unsaved {
        Some("Changes not saved")
    } else {
        None
    };
    varde_view::DesignCard {
        key: varde_view::CardKey::Browser(&design.name),
        name: design_name(Path::new(&design.name)),
        file: true,
        written: design.saved.map(|saved| when::ago(saved, now)),
        downloads: readable.then(|| downloads(design.download, now)),
        damaged: design.damage.is_some(),
        opens: readable,
        thumbnail: files.browser_thumbnail(design),
        note,
        deletable: !design.in_use,
    }
}

/// How the view says where a design stands against its downloads, with
/// times relative to `now`, to go in a sentence ("Latest downloaded just
/// now").
pub(crate) fn downloads(status: DownloadStatus, now: UnixSeconds) -> varde_view::Downloads {
    let at = |time| Some(when::ago_in_sentence(time, now));
    match status {
        DownloadStatus::Never => varde_view::Downloads::Never,
        DownloadStatus::Changed(time) => varde_view::Downloads::Changed(at(time)),
        DownloadStatus::Latest(time) => varde_view::Downloads::Latest(at(time)),
    }
}

/// How much browser storage is used, as the foot says: "1.2 MB of 2 GB
/// used".
pub(crate) fn space_used(space: varde_io::storage::Space) -> String {
    format!("{} of {} used", bytes(space.used), bytes(space.quota))
}

/// `n` bytes, in the largest unit that's at least one of it, to one
/// decimal under ten.
fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    match unit {
        0 => format!("{n} bytes"),
        _ if value < 10.0 => format!("{value:.1} {}", UNITS[unit]),
        _ => format!("{value:.0} {}", UNITS[unit]),
    }
}

/// Deletes the design `name` in browser storage, gone from the list at
/// once: the lane lists what's left after.
fn delete(files: &mut Files, name: String) {
    files.browser.retain(|design| design.name != name);
    files.io.send(IoRequest::DeleteFromBrowser { name });
}

/// Whether there's a list of recently opened files. Browsers only hand
/// over files the user picks, and the handles to them would have to be
/// kept in IndexedDB to be opened again later, which isn't done yet.
const RECENT_FILES: bool = cfg!(not(target_arch = "wasm32"));

#[cfg(test)]
mod tests;
