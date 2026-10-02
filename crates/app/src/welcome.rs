//! The welcome screen: making a new design and opening one, from a file,
//! the recent files list or what crashed sessions left behind.

use std::path::{Path, PathBuf};

use iced::Element;
use varde_document::Document;
use varde_document::name::UNTITLED;
use varde_io::{
    Access, Chosen, OpenId, Opened, PickedFrom, Recovered, Request as IoRequest, UnixSeconds,
};
use varde_view::{Message as Ui, Mode, ThemeChoice, Welcome as WelcomeUi};

use crate::doc::{Doc, Leave, Origin, Target, design_name};
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

    /// Opens `source`.
    fn open(&mut self, files: &mut Files, source: Source) -> Next {
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
    /// it opened, see [`Io::abandon`].
    ///
    /// [`Io::abandon`]: crate::io::Io::abandon
    fn give_up(&mut self, files: &mut Files) {
        files
            .io
            .abandon(self.opening.take().map(|opening| opening.id));
    }

    /// Takes the answer to the open tagged `id`, of `path` as the lane
    /// made it absolute, if it has one.
    pub(crate) fn opened(
        &mut self,
        files: &mut Files,
        id: OpenId,
        path: Option<PathBuf>,
        result: Result<Opened, String>,
    ) -> Next {
        // An answer the screen isn't waiting for is stale: the user moved
        // on, and the lane closes the file once told so, see `Io::abandon`.
        let Some(opening) = self.opening.take_if(|o| o.id == id) else {
            return Next::Stay;
        };
        let doc = match (opening.source, result) {
            // Picked on the web: the design goes on from the file if it can
            // be written to, otherwise it's a copy.
            (Source::Chosen(Chosen::File(picked)), Ok(opened)) => {
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
                let origin = Origin::new(target, opened.access, title);
                Doc::new(opened.document, origin)
            }
            (Source::Chosen(Chosen::Path(asked)), Ok(opened)) => {
                let path = path.unwrap_or(asked);
                files.remember(path.clone());
                let recovered = opened.recovered.unwrap_or_else(|error| {
                    log::error!("Ignored what was auto-saved of {}: {error}", path.display());
                    None
                });
                let target = Target::File {
                    file: opened.file,
                    picked: None,
                };
                let origin = Origin {
                    recovered,
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
            (Source::Recovered(path), Ok(opened)) => {
                let title = files.recovered_title(&path);
                files.recovered.retain(|design| design.path != path);
                // Changes recovered may be on top of a download, which the
                // entry goes back to if they aren't saved.
                let target = Target::Entry {
                    file: opened.file,
                    downloaded: true,
                };
                let origin = Origin {
                    saved: opened.downloaded,
                    ..Origin::new(target, opened.access, title)
                };
                Doc::new(opened.document, origin)
            }
            (Source::Recovered(_), Err(error)) => {
                // The lane lists what's there now, as the entry may be gone
                // or changed.
                self.error = Some(format!("Couldn't open the recovered design: {error}"));
                return Next::Stay;
            }
            (Source::Chosen(Chosen::Path(asked)), Err(error)) => {
                let path = path.unwrap_or(asked);
                self.error = Some(format!("Couldn't open {}: {error}", path.display()));
                return Next::Stay;
            }
            (Source::Chosen(Chosen::File(picked)), Err(error)) => {
                self.error = Some(format!("Couldn't open {}: {error}", picked.name));
                return Next::Stay;
            }
        };
        Next::Show(Box::new(doc))
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
        varde_view::welcome(varde_view::WelcomeState {
            error: self.error.as_deref(),
            recent,
            recovered,
            downloaded,
            mode,
            theme,
        })
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
    }
}

/// Whether there's a list of recently opened files. Browsers only hand
/// over files the user picks, and the handles to them would have to be
/// kept in IndexedDB to be opened again later, which isn't done yet.
const RECENT_FILES: bool = cfg!(not(target_arch = "wasm32"));
