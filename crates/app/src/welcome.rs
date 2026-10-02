//! The welcome screen: making a new design and opening one, from a file,
//! the recent files list or what crashed sessions left behind.

use std::path::{Path, PathBuf};

use iced::Element;
use varde_document::Document;
use varde_document::name::UNTITLED;
use varde_io::{
    Access, Chosen, Closing, Damage, DamageKind, OpenId, Opened, PickedFrom, Recovered,
    RecoveryError, Request as IoRequest, StoredDamage, UnixSeconds,
};
use varde_view::{Message as Ui, Mode, ThemeChoice, Welcome as WelcomeUi};

use crate::doc::{Doc, FileDamage, Leave, Origin, Recovery, Target, design_name};
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
    /// dialog, or on the web the file the user picked.
    Chosen(Chosen),
    /// The store entry of a new design left behind by a crash, or
    /// downloaded on the web.
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
            WelcomeUi::OpenDamaged => self.open_damaged(files),
            WelcomeUi::OpenFound => self.open_found(files),
            WelcomeUi::CancelDamaged => {
                self.cancel_damaged(files);
                Next::Stay
            }
        }
    }

    /// Whether a damaged file is being asked about, see [`Damaged`]: no
    /// key opens anything else meanwhile.
    pub(crate) fn prompting(&self) -> bool {
        self.damaged.is_some()
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
    /// time.
    fn cancel_damaged(&mut self, files: &mut Files) {
        if let Some(damaged) = self.damaged.take() {
            files.io.send(IoRequest::Close {
                file: damaged.opened.file,
                closing: Closing::Keep,
            });
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
                design.path == *path && design.damage == Some(StoredDamage::Unreadable)
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

    /// Forgets the new design at `path` left behind by a crash, or
    /// downloaded on the web.
    fn discard_recovered(&mut self, files: &mut Files, path: PathBuf) -> Next {
        if self.prompting() {
            return Next::Stay;
        }
        // Gone from the list at once. The lane lists what's left after:
        // changes never saved may be on top of a download, which it goes
        // back to, to be listed as such.
        files.recovered.retain(|design| design.path != path);
        files.io.send(IoRequest::DiscardRecovered { path });
        Next::Stay
    }

    /// Discarding a recovered design failed, with `error`.
    pub(crate) fn discard_failed(&mut self, error: &str) -> Next {
        self.error = Some(format!("Couldn't discard the design: {error}"));
        Next::Stay
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
        let recent = RECENT_FILES.then(|| {
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
                })
                .collect()
        });
        let (downloaded, recovered): (Vec<_>, Vec<_>) =
            files.recovered.iter().partition(|design| design.downloaded);
        let recovered = recovered
            .into_iter()
            .map(|design| recovered_design(design, recovered_name(design), now))
            .collect();
        // By the name it was downloaded as.
        let downloaded = downloaded
            .into_iter()
            .map(|design| {
                let name = varde_document::name::download_name(&recovered_name(design));
                recovered_design(design, name, now)
            })
            .collect();
        let damaged = self
            .damaged
            .as_ref()
            .map(|damaged| damaged.prompt(files, now));
        varde_view::welcome(varde_view::WelcomeState {
            error: self.error.as_deref(),
            recent,
            recovered,
            downloaded,
            damaged,
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
        // Picked on the web: the design goes on from the file if it can
        // be written to, otherwise it's a copy.
        Source::Chosen(Chosen::File(picked)) => {
            let title = design_name(Path::new(&picked.name));
            let file = opened.file;
            let target = match picked.from {
                PickedFrom::Handle => Target::File {
                    file,
                    picked: Some(picked),
                },
                PickedFrom::Input => Target::Entry {
                    file,
                    downloaded: false,
                },
            };
            let origin = Origin {
                recovered: offered(opened.recovered, &title),
                damage: damage(false),
                ..Origin::new(target, opened.access, title)
            };
            Doc::new(opened.document, origin)
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
            Doc::new(opened.document, origin)
        }
        // A new design left behind by a crash: never saved, and its
        // store entry is auto-saved to from now on. Known by the name
        // of the file it was opened from, if it was, as on the web. One
        // downloaded on the web goes on as a copy too, but isn't
        // edited: it's what was downloaded, as far as is known, so
        // closing it again keeps it listed rather than asking. As its
        // entry holds it now, not as listed: another tab may have left
        // changes in it since.
        Source::Recovered(path) => {
            let title = title.unwrap_or_else(|| files.recovered_title(&path));
            files.recovered.retain(|design| design.path != path);
            // Changes recovered may be on top of a download, which the
            // entry goes back to if they aren't saved.
            let target = Target::Entry {
                file: opened.file,
                downloaded: true,
            };
            let origin = Origin {
                saved: opened.downloaded,
                damage: damage(true),
                ..Origin::new(target, opened.access, title)
            };
            Doc::new(opened.document, origin)
        }
    }
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

/// The welcome screen's card or row for `design`, known by `name`, with
/// when it was last written relative to `now`.
fn recovered_design(
    design: &Recovered,
    name: String,
    now: UnixSeconds,
) -> varde_view::StoredDesign<'_> {
    varde_view::StoredDesign {
        path: &design.path,
        name,
        written: design.modified.map(|modified| when::ago(modified, now)),
        damaged: design.damage.is_some(),
        opens: design.damage != Some(StoredDamage::Unreadable),
    }
}

/// Whether there's a list of recently opened files. Browsers only hand
/// over files the user picks, and the handles to them would have to be
/// kept in IndexedDB to be opened again later, which isn't done yet.
const RECENT_FILES: bool = cfg!(not(target_arch = "wasm32"));
