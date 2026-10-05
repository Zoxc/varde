//! The varde application: owns all state and handles its `Message`s.
//!
//! Frontends (`binary`, `web`) only call [`run`].

mod doc;
mod io;
mod keys;
mod message;
mod platform;
mod recent;
mod samples;
mod settings;
mod welcome;
mod when;

use std::path::{Path, PathBuf};

use iced::futures::Stream;
use iced::keyboard::{self, key};
use iced::{Element, Subscription, Task, window};

use varde_document::APP_NAME;
use varde_document::Revision;
use varde_io::{
    BrowserDesign, Chosen, Panic, Picked, Recovered, Request as IoRequest, Response as IoResponse,
    SaveError,
};
use varde_view::{File, Held, Look, Message as Ui, Mode, Unsaved, ViewOptions};

use crate::doc::{Dialog, Doc, DocId, Downloader, Focus, Leave};
use crate::io::Io;
use crate::keys::{document_key, welcome_key};
use crate::message::{ForDoc, Message};
use crate::recent::Recent;
use crate::settings::Settings;
use crate::welcome::Welcome;

pub(crate) struct Varde {
    screen: Screen,
    /// The mode the system prefers, if it says: the theme's when the user
    /// chose [`ThemeChoice::Auto`](varde_view::ThemeChoice::Auto).
    system: Option<Mode>,
    /// What the view options menu turns on and off, and the theme chosen.
    options: ViewOptions,
    /// What's stored of `options`.
    settings: Settings,
    /// Whether the peek key is held, see [`Held::PEEK`].
    peeking: bool,
    /// Whether the command modifier (`Ctrl`, or `Cmd` on macOS) is held,
    /// with which clicking a list's row adds to the selection.
    command: bool,
    files: Files,
    /// The window to close once the IO lane is done, see [`Varde::quit`].
    quitting: Option<window::Id>,
}

/// The files side of the app, which both screens' steps use: the IO lane,
/// downloads, and the lists the welcome screen shows.
struct Files {
    io: Io,
    /// How a design is handed over as a download: on the web, by the page.
    /// `None` natively, where there's nothing to download. Tests set it
    /// natively.
    downloader: Option<Downloader>,
    /// Whether designs are saved in browser storage, by name, as on the
    /// web: Save As asks for a name in the app's own dialog. Tests set it
    /// natively.
    browser_storage: bool,
    /// Whether the File System Access API is there (Chromium): files of
    /// the user's are picked and saved back to, Save As offers one, and
    /// exports aren't downloaded.
    file_system_access: bool,
    recent: Recent,
    /// New designs left behind by sessions that crashed, newest first.
    recovered: Vec<Recovered>,
    /// On the web, the designs saved in browser storage, newest first, as
    /// last listed, and their thumbnails as iced handles, made once per
    /// design and save, see [`Files::browser_listed`].
    browser: Vec<BrowserDesign>,
    browser_thumbnails: Vec<BrowserThumbnail>,
    /// The sample designs' thumbnails ([`samples::SAMPLES`]), each as its
    /// file has one, made once.
    sample_thumbnails: Vec<Option<ThumbnailHandles>>,
    /// On the web, what the browser says of its storage, see
    /// [`varde_io::storage`].
    storage: StorageState,
    /// Whether to ask the browser to keep its storage for good, as the
    /// user just saved to it: done after the step, see `Varde::update`.
    ask_persist: bool,
    /// The recent files' thumbnails, by path, as last read: see
    /// [`Files::load_thumbnails`].
    thumbnails: Vec<(PathBuf, ThumbnailHandles)>,
    /// The panic a session recorded, see [`varde_io::panicked`], till the
    /// user discards it.
    panic: Option<Panic>,
}

/// The thumbnail of a design in browser storage, as an iced handle, and
/// the design's name and the sum of its newest save, which tell when it's
/// another.
struct BrowserThumbnail {
    name: String,
    sum: u128,
    handles: ThumbnailHandles,
}

/// A design's thumbnail as iced handles, one for each theme's image, made
/// once: a handle is uploaded once per id, the first time it's shown.
#[derive(Clone)]
struct ThumbnailHandles {
    light: iced::widget::image::Handle,
    dark: iced::widget::image::Handle,
}

impl ThumbnailHandles {
    fn new(thumbnail: varde_io::thumbnail::Thumbnail) -> ThumbnailHandles {
        let handle = |image: varde_io::thumbnail::Image| {
            let (width, height) = (image.width(), image.height());
            iced::widget::image::Handle::from_rgba(width, height, image.into_rgba())
        };
        ThumbnailHandles {
            light: handle(thumbnail.light),
            dark: handle(thumbnail.dark),
        }
    }

    /// The handle of the image in `mode`'s colours.
    fn of(&self, mode: Mode) -> iced::widget::image::Handle {
        match mode {
            Mode::Light => self.light.clone(),
            Mode::Dark => self.dark.clone(),
        }
    }
}

/// What the platform offers the document screen, as [`Files`] knows it:
/// the file menu offers Download where a design can be downloaded, and
/// the Save As dialog a file on the computer where the File System Access
/// API is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Offers {
    pub(crate) download: bool,
    pub(crate) file_system_access: bool,
}

/// What the browser says of its storage, on the web.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct StorageState {
    /// Whether it keeps the site's storage for good, if it says.
    pub(crate) persisted: Option<bool>,
    /// How much of it the site uses, and may, if it says.
    pub(crate) space: Option<varde_io::storage::Space>,
    /// Whether the app asked it to keep the site's storage this session.
    pub(crate) asked: bool,
}

impl Files {
    fn new(downloader: Option<Downloader>) -> Self {
        Self {
            io: Io::new(),
            downloader,
            browser_storage: cfg!(target_arch = "wasm32"),
            file_system_access: varde_io::pick::file_system_access(),
            recent: Recent::default(),
            recovered: Vec::new(),
            browser: Vec::new(),
            browser_thumbnails: Vec::new(),
            sample_thumbnails: (samples::SAMPLES.iter())
                .map(|sample| varde_io::thumbnail::of_file(sample.file).map(ThumbnailHandles::new))
                .collect(),
            storage: StorageState::default(),
            ask_persist: false,
            thumbnails: Vec::new(),
            panic: None,
        }
    }

    /// Takes the designs in browser storage as listed. Their thumbnails'
    /// handles are kept while the design is saved as it was, by its sum:
    /// a handle is uploaded once per id.
    fn browser_listed(&mut self, designs: Vec<BrowserDesign>) {
        let mut kept = std::mem::take(&mut self.browser_thumbnails);
        self.browser_thumbnails = (designs.iter())
            .filter_map(|design| {
                let (thumbnail, sum) = (design.thumbnail.as_ref()?, design.sum?);
                let same = |thumbnail: &BrowserThumbnail| {
                    thumbnail.name == design.name && thumbnail.sum == sum
                };
                let handles = match kept.iter().position(same) {
                    Some(at) => kept.swap_remove(at).handles,
                    None => ThumbnailHandles::new(thumbnail.clone()),
                };
                Some(BrowserThumbnail {
                    name: design.name.clone(),
                    sum,
                    handles,
                })
            })
            .collect();
        self.browser = designs;
    }

    /// The thumbnail of `design`, listed in browser storage, if it has
    /// one, in `mode`'s colours.
    fn browser_thumbnail(
        &self,
        design: &BrowserDesign,
        mode: Mode,
    ) -> Option<iced::widget::image::Handle> {
        (self.browser_thumbnails.iter())
            .find(|thumbnail| Some(thumbnail.sum) == design.sum && thumbnail.name == design.name)
            .map(|thumbnail| thumbnail.handles.of(mode))
    }

    /// The thumbnail of the sample `index` ([`samples::SAMPLES`]), if its
    /// file has one, in `mode`'s colours.
    fn sample_thumbnail(&self, index: usize, mode: Mode) -> Option<iced::widget::image::Handle> {
        let handles = self.sample_thumbnails.get(index)?.as_ref()?;
        Some(handles.of(mode))
    }

    /// What the platform offers the document screen.
    fn offers(&self) -> Offers {
        Offers {
            download: self.downloader.is_some(),
            file_system_access: self.file_system_access,
        }
    }

    /// Asks the lane for the designs in browser storage, where there's
    /// that, and the browser for what it says of it.
    fn list_browser(&mut self) {
        if self.browser_storage {
            self.io.send(IoRequest::ListBrowser);
        }
    }

    /// Whether to ask the browser to keep its storage for good now: once a
    /// session, as the user saves to it, unless it does already.
    fn take_ask_persist(&mut self) -> bool {
        let ask = std::mem::take(&mut self.ask_persist)
            && !self.storage.asked
            && self.storage.persisted != Some(true);
        self.storage.asked |= ask;
        ask
    }

    /// Takes the answer to a Save As whose document has gone since: the
    /// file it made is closed, as there's nothing left to write it.
    fn settle_saved_as(&mut self, result: Result<varde_io::SavedAs, SaveError>) {
        if let Ok(saved) = result {
            self.io.close_clean(saved.file);
        }
    }

    /// `path` as the file cell's tooltip shows it, the home directory as
    /// `~`.
    fn shown_path(&self, path: &Path) -> String {
        let dir = self.recent.display_dir(path);
        let file = path.file_name().unwrap_or_default();
        Path::new(&dir).join(file).display().to_string()
    }

    /// Records `path` as opened just now in the recent files list, and
    /// stores the list once it's complete.
    fn remember(&mut self, path: PathBuf) {
        if let Some(write) = self.recent.opened(path, when::now()) {
            self.io.send(write);
        }
    }
}

/// What the app is to do after a step of either screen, which the screen
/// can't do itself.
enum Next {
    /// Nothing more for now.
    Stay,
    /// Ask the user for a design to open, answered with
    /// [`Message::Picked`].
    PickOpen,
    /// Ask the user where to save document `id`, suggesting `name`,
    /// answered with [`ForDoc::SaveAsPicked`].
    PickSaveAs { id: DocId, name: String },
    /// Ask the user where to export document `id`'s bodies, suggesting
    /// `name`, answered with [`ForDoc::ExportPicked`].
    PickExport { id: DocId, name: String },
    /// Ask the browser whether document `id` may be saved to the file the
    /// user `picked`, answered with [`ForDoc::Writable`].
    AskWritable { id: DocId, picked: Picked },
    /// Download `bytes`, a design's file named `name` in browser storage,
    /// with the page's downloader, and say why the download couldn't be
    /// recorded, if `not_recorded`.
    Download {
        name: String,
        bytes: Vec<u8>,
        not_recorded: Option<String>,
    },
    /// Show the document, made or opened on the welcome screen.
    Show(Box<Doc>),
    /// The screen is left: the document's file is closed, and the screen
    /// goes. The welcome screen is only left to quit.
    Left(Leave),
}

enum Screen {
    Welcome(Welcome),
    Document(Box<Doc>),
}

impl Screen {
    /// The open document, if that's the screen showing.
    fn doc(&self) -> Option<&Doc> {
        match self {
            Screen::Document(doc) => Some(doc),
            Screen::Welcome(_) => None,
        }
    }

    fn doc_mut(&mut self) -> Option<&mut Doc> {
        match self {
            Screen::Document(doc) => Some(doc),
            Screen::Welcome(_) => None,
        }
    }
}

impl Varde {
    pub(crate) fn new() -> Self {
        let downloader = varde_io::pick::downloader().map(|f| Box::new(f) as Downloader);
        let mut files = Files::new(downloader);
        // The welcome screen shows the lists once they have arrived.
        files.io.send(IoRequest::LoadRecent);
        // Browser storage first: listing it may give the store a design.
        files.list_browser();
        files.io.send(IoRequest::ListRecovered);
        // The theme is the system's until the stored one arrives.
        files.io.send(IoRequest::LoadSettings);
        files.io.send(IoRequest::LoadPanic);
        Self {
            screen: Screen::Welcome(Welcome::default()),
            system: None,
            options: ViewOptions::default(),
            settings: Settings::default(),
            peeking: false,
            command: false,
            files,
            quitting: None,
        }
    }

    /// The app as it starts, and the task asking for the mode the system
    /// prefers, which [`Varde::subscription`] hears of changes to after.
    fn boot() -> (Self, Task<Message>) {
        let app = Self::new();
        let storage = if app.files.browser_storage {
            storage_state()
        } else {
            Task::none()
        };
        let theme = iced::system::theme().map(Message::SystemTheme);
        (app, Task::batch([theme, storage]))
    }

    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        // A value field opened, or a value refused, has the field take the
        // focus once it shows.
        let task = match self.screen.doc_mut().and_then(Doc::take_focus) {
            Some(focus) => Task::batch([task, focus_field(focus)]),
            None => task,
        };
        // The rail's list scrolled to show the row the keys moved to.
        let task = match self.screen.doc_mut().and_then(|doc| doc.rail.take_scroll()) {
            Some(share) => Task::batch([task, scroll_rail(share)]),
            None => task,
        };
        // The Save As dialog's name field takes the focus as it shows.
        let task = match self.screen.doc_mut().is_some_and(Doc::take_name_focus) {
            true => Task::batch([task, focus_name()]),
            false => task,
        };
        // Asked as the user first saves to browser storage.
        let task = match self.files.take_ask_persist() {
            true => Task::batch([
                task,
                Task::perform(varde_io::storage::persist(), Message::Persisted),
            ]),
            false => task,
        };
        platform::show_title(&self.title());
        // A thumbnail asked for, rendered by the viewport's next frame.
        match self.screen.doc_mut() {
            Some(doc) => match doc.take_thumbnail() {
                Some((tag, answer)) => Task::batch([task, thumbnail(doc.id, tag, answer)]),
                None => task,
            },
            None => task,
        }
    }

    /// Takes `message`, see [`Varde::update`].
    fn handle(&mut self, message: Message) -> Task<Message> {
        // Once quitting, whatever is sent to the lane would come after the
        // flush the window closes on, see `Varde::quit`.
        if self.quitting.is_some() && !while_quitting(&message) {
            return Task::none();
        }
        match message {
            Message::Ui(Ui::Welcome(message)) => {
                return self.welcome(|welcome, files| welcome.update(files, message));
            }
            Message::Picked(chosen) => {
                return self.welcome(|welcome, files| welcome.picked(files, chosen));
            }
            Message::FileDragged(over) => {
                return self.welcome(|welcome, _| welcome.dragged(over));
            }
            Message::FileDropped(chosen) => {
                // With a document open there's no drop zone, and nothing
                // opens.
                if self.screen.doc().is_some() {
                    chosen.iter().for_each(welcome::forget);
                    return Task::none();
                }
                return self.welcome(|welcome, files| welcome.dropped(files, chosen));
            }
            Message::Ui(Ui::File(message)) => return self.file(message),
            Message::AutoSaveTick(now) => {
                return self.step(|doc, cx| {
                    doc.auto_save(cx, now);
                    doc.thumbnail_overdue(cx, now)
                });
            }
            Message::CloseRequested(window) => return self.leave(Leave::Quit(window)),
            Message::PageLeaving => {
                // Auto-saved now, in case it's gone before the next tick.
                self.with_doc(|doc, files| doc.auto_save_now(files));
            }
            Message::Doc(id, message) => {
                return self.step(|doc, cx| {
                    if doc.id != id {
                        return Next::Stay;
                    }
                    doc.answer(cx, message)
                });
            }
            Message::IoReady(lane) => self.files.io.ready(Box::new(lane)),
            Message::Io(response) => return self.io_response(response),

            Message::Ui(Ui::Edit(message)) => {
                // Undo may drop the edits a save waits for.
                return self.step(|doc, cx| {
                    doc.update(message);
                    doc.proposals_settled(cx)
                });
            }
            Message::Ui(Ui::Look(message)) => {
                let message = match message {
                    // A clicked tab ends the peek, or the peek would show
                    // the other one; the next change of modifiers starts it
                    // again.
                    Look::SelectPanel(_) => {
                        self.peeking = false;
                        message
                    }
                    // A row or a label knows nothing of the keys held.
                    Look::ClickRow(id) => Look::ClickGeometry {
                        hit: Some(id),
                        add: self.command,
                    },
                    Look::ChooseOverlap { index, .. } => Look::ChooseOverlap {
                        index,
                        add: self.command,
                    },
                    Look::PressLabel { id, .. } => Look::PressLabel {
                        id,
                        add: self.command,
                    },
                    Look::ClickBody { body, .. } => Look::ClickBody {
                        body,
                        add: self.command,
                    },
                    Look::ClickObject { row, .. } => Look::ClickObject {
                        row,
                        add: self.command,
                    },
                    message => message,
                };
                // The delete prompt cancelled may free what waited for it.
                return self.step(|doc, cx| {
                    doc.look(message);
                    doc.proposals_settled(cx)
                });
            }
            Message::Ui(Ui::CycleTheme) => {
                self.options.theme = self.options.theme.cycled();
                if let Some(write) = self.settings.chose_theme(self.options) {
                    self.files.io.send(write);
                }
            }
            Message::SystemTheme(mode) => {
                self.system = match mode {
                    iced::theme::Mode::Light => Some(Mode::Light),
                    iced::theme::Mode::Dark => Some(Mode::Dark),
                    iced::theme::Mode::None => None,
                };
            }
            Message::Ui(Ui::Copy(text)) => return platform::copy(text),
            Message::Ui(Ui::ToggleMouseHints) => {
                self.options.mouse_hints = !self.options.mouse_hints;
                self.with_doc(|doc, _| doc.view_menu = false);
                if let Some(write) = self.settings.chose_mouse_hints(self.options) {
                    self.files.io.send(write);
                }
            }
            Message::Ui(Ui::ToggleHiddenEdges) => {
                self.options.hidden_edges = !self.options.hidden_edges;
                self.with_doc(|doc, _| doc.view_menu = false);
            }
            Message::Ui(Ui::SetEdges(edges)) => {
                self.options.edges = edges;
                self.with_doc(|doc, _| doc.view_menu = false);
            }
            Message::Ui(Ui::SetShading(shading)) => {
                self.options.shading = shading;
                self.with_doc(|doc, _| doc.view_menu = false);
            }
            Message::PeekPanel(peeking) => {
                // The tab shown changes: the Timeline's rows go, or come,
                // without an exit or an enter.
                if peeking != self.peeking {
                    self.with_doc(|doc, _| doc.look(Look::HoverFeature(None)));
                }
                self.peeking = peeking;
            }
            Message::CommandHeld(held) => self.command = held,
            Message::StorageState { persisted, space } => {
                self.files.storage.persisted = persisted.or(self.files.storage.persisted);
                self.files.storage.space = space.or(self.files.storage.space);
            }
            Message::Persisted(persisted) => {
                if persisted.is_some() {
                    self.files.storage.persisted = persisted;
                }
                // The space used has changed with the save.
                return storage_state();
            }
            Message::AnimationFrame(now) => self.with_doc(|doc, _| {
                doc.animation_frame(now);
                doc.tick(now);
                doc.feed.tick(&doc.editor, now);
                doc.rail.tick(now);
            }),
        }
        Task::none()
    }

    /// Takes `message`, about the open document's file.
    fn file(&mut self, message: File) -> Task<Message> {
        match message {
            File::CloseDocument => return self.leave(Leave::Close),
            File::Save => return self.step(|doc, cx| doc.request_save(cx)),
            File::SaveAs => return self.step(|doc, cx| doc.request_save_as(cx)),
            File::Export => return self.step(|doc, cx| doc.request_export(cx)),
            File::Download => self.with_doc(|doc, files| doc.download(files)),
            File::Rename => self.with_doc(|doc, _| doc.request_rename()),
            File::Name(name) => self.with_doc(|doc, _| doc.name_changed(name)),
            File::Place(place) => self.with_doc(|doc, _| doc.place_chosen(place)),
            File::ConfirmName => return self.step(|doc, cx| doc.name_confirmed(cx)),
            File::CancelName => return self.step(|doc, cx| doc.name_cancelled(cx)),
            File::Unsaved(choice) => return self.step(|doc, cx| doc.answer_unsaved(cx, choice)),
            File::RestoreChanges => return self.step(|doc, cx| doc.restore_recovered(cx)),
            File::DiscardChanges => self.with_doc(|doc, files| doc.discard_recovered(files)),
        }
        Task::none()
    }

    /// Leaves the document for `to`, or the welcome screen if that's the
    /// one showing. Asks what to do about unsaved changes first, and waits
    /// for saves in flight, see [`Doc::leave`].
    fn leave(&mut self, to: Leave) -> Task<Message> {
        match self.screen {
            Screen::Welcome(_) => self.welcome(|welcome, files| welcome.leave(files, to)),
            Screen::Document(_) => self.step(|doc, cx| doc.leave(cx, to)),
        }
    }

    /// Takes `step` of saving or leaving the open document, if there is
    /// one, and does what it says to next.
    fn step(&mut self, step: impl FnOnce(&mut Doc, &mut Files) -> Next) -> Task<Message> {
        let next = match self.screen.doc_mut() {
            Some(doc) => step(doc, &mut self.files),
            None => Next::Stay,
        };
        self.follow(next)
    }

    /// Does `act` to the open document, if there is one, with the files
    /// side: for what doesn't say what to do next.
    fn with_doc(&mut self, act: impl FnOnce(&mut Doc, &mut Files)) {
        if let Some(doc) = self.screen.doc_mut() {
            act(doc, &mut self.files);
        }
    }

    /// Takes `step` on the welcome screen, if it's showing, and does what
    /// it says to next.
    fn welcome(&mut self, step: impl FnOnce(&mut Welcome, &mut Files) -> Next) -> Task<Message> {
        let next = match &mut self.screen {
            Screen::Welcome(welcome) => step(welcome, &mut self.files),
            Screen::Document(_) => Next::Stay,
        };
        self.follow(next)
    }

    /// Does what a step of either screen said to `next`. The only place
    /// the screen changes.
    fn follow(&mut self, next: Next) -> Task<Message> {
        match next {
            Next::Stay => Task::none(),
            Next::PickOpen => pick_open(),
            Next::PickSaveAs { id, name } => pick_save_as(id, &name),
            Next::PickExport { id, name } => pick_export(id, &name),
            Next::AskWritable { id, picked } => ask_writable(id, picked),
            Next::Download {
                name,
                bytes,
                not_recorded,
            } => {
                let result = match &self.files.downloader {
                    Some(download) => download(&name, &bytes),
                    None => Err("there's nowhere to download to".to_owned()),
                };
                let error = match (result, not_recorded) {
                    (Err(error), _) => Some(error),
                    (Ok(()), Some(why)) => Some(format!(
                        "Downloaded {name}, but couldn't record the download: {why}"
                    )),
                    (Ok(()), None) => None,
                };
                // Listed again, with the download recorded.
                self.files.list_browser();
                if let Some(error) = error {
                    return self.welcome(|welcome, _| welcome.failed(&error));
                }
                Task::none()
            }
            Next::Show(doc) => {
                self.screen = Screen::Document(doc);
                Task::none()
            }
            Next::Left(Leave::Close) => {
                self.screen = Screen::Welcome(Welcome::default());
                // Saved since, with another.
                self.files.load_thumbnails();
                // Listed after the close, so what it lets go of shows as
                // it's left.
                self.files.list_browser();
                if self.files.browser_storage {
                    return storage_state();
                }
                Task::none()
            }
            Next::Left(Leave::Quit(window)) => self.quit(window),
        }
    }

    /// Closes `window`, which quits, once the IO lane has done everything
    /// asked of it: the last saves and closes, which delete the lock files,
    /// and the recent files list. Exiting before would cut its thread off.
    /// Until then only messages [`while_quitting`] lets through act.
    fn quit(&mut self, window: window::Id) -> Task<Message> {
        self.quitting = Some(window);
        self.files.io.send(IoRequest::Flush);
        Task::none()
    }

    fn io_response(&mut self, response: IoResponse) -> Task<Message> {
        match response {
            IoResponse::Opened { id, path, result } => {
                return self.welcome(|welcome, files| welcome.opened(files, id, path, result));
            }
            // One not the open document's was given up on, and the lane
            // closes it, see `Io::abandon`.
            IoResponse::Created { id, result } => {
                if let Some(doc) = self.screen.doc_mut() {
                    doc.created(&mut self.files, id, &result);
                }
            }
            IoResponse::AutoSaved { file, result, .. } => {
                self.with_doc(|doc, _| doc.auto_saved(file, result))
            }
            IoResponse::RecoveryDiscarded { result, .. } => {
                report_failure("discard what was auto-saved", result);
            }
            IoResponse::RecoveredListed { designs } => self.files.recovered = designs,
            IoResponse::BrowserListed { designs } => self.files.browser_listed(designs),
            IoResponse::DeletedFromBrowser { name, result } => {
                self.files.list_browser();
                if let Err(error) = result {
                    return self.welcome(|welcome, _| {
                        welcome.failed(&format!("Couldn't delete {name}: {error}"))
                    });
                }
            }
            IoResponse::DownloadedFromBrowser {
                name,
                result,
                not_recorded,
            } => {
                return match result {
                    Ok(bytes) => self.follow(Next::Download {
                        name,
                        bytes,
                        not_recorded,
                    }),
                    Err(error) => self.welcome(|welcome, _| {
                        welcome.failed(&format!("Couldn't download {name}: {error}"))
                    }),
                };
            }
            IoResponse::Renamed { file, name, result } => {
                // Another name taken, or another freed.
                self.files.list_browser();
                self.with_doc(|doc, _| doc.renamed(file, name, result));
            }
            IoResponse::DownloadRecorded { file, result } => {
                self.with_doc(|doc, _| doc.download_recorded(file, result));
            }
            IoResponse::RecoveredDiscarded { result, .. } => {
                // The lane lists what's left after it either way.
                if let Err(error) = result {
                    return self.welcome(|welcome, _| welcome.discard_failed(&error));
                }
            }
            IoResponse::Closed { result, .. } | IoResponse::Abandoned { result, .. } => {
                report_failure("close a file", result);
            }
            IoResponse::RecentLoaded { entries, home } => {
                if let Some(write) = self.files.recent.loaded(entries, home) {
                    self.files.io.send(write);
                }
                self.files.load_thumbnails();
            }
            IoResponse::ThumbnailsLoaded { thumbnails } => {
                self.files.thumbnails = (thumbnails.into_iter())
                    .map(|(path, thumbnail)| (path, ThumbnailHandles::new(thumbnail)))
                    .collect();
            }
            IoResponse::RecentWritten { result } => {
                report_failure("save the recent files", result);
            }
            IoResponse::SettingsLoaded { settings } => {
                if let Some(write) = self.settings.loaded(settings, &mut self.options) {
                    self.files.io.send(write);
                }
            }
            IoResponse::SettingsWritten { result } => {
                report_failure("save the settings", result);
            }
            IoResponse::PanicLoaded { panic } => self.files.panic = panic,
            IoResponse::PanicDiscarded { result } => {
                report_failure("discard the internal error", result);
            }
            IoResponse::Saved {
                file,
                revision,
                result,
            } => {
                return self.step(|doc, cx| doc.saved_to(cx, file, revision, result));
            }
            IoResponse::SavedAs {
                to,
                revision,
                result,
                ..
            } => {
                // A name taken, or replaced: what the Save As dialog asks
                // about replacing follows.
                if matches!(to, Chosen::Browser(_)) {
                    self.files.list_browser();
                }
                return self.saved_as(to, revision, result);
            }
            IoResponse::Exported { result, .. } => match self.screen.doc_mut() {
                Some(doc) => doc.export_written(result),
                // Closed while it was written: the file is written or not
                // all the same.
                None => report_failure("export", result),
            },
            IoResponse::Flushed => {
                if let Some(window) = self.quitting {
                    return window::close(window);
                }
            }
        }
        Task::none()
    }

    /// Takes the answer to a Save As to the file the user `chose`, see
    /// [`Doc::saved_as`]. Leaving waits for saves, so the document that
    /// asked is the one open, if any.
    fn saved_as(
        &mut self,
        chose: Chosen,
        revision: Revision,
        result: Result<varde_io::SavedAs, SaveError>,
    ) -> Task<Message> {
        if self.screen.doc().is_none() {
            self.files.settle_saved_as(result);
            return Task::none();
        }
        self.step(|doc, cx| doc.saved_as(cx, chose, revision, result))
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        let view = match &self.screen {
            Screen::Welcome(welcome) => welcome.view(&self.files, self.mode(), self.options.theme),
            Screen::Document(doc) => {
                doc.view(self.peeking, self.mode(), self.options, self.files.offers())
            }
        };
        view.map(Message::Ui)
    }

    pub(crate) fn title(&self) -> String {
        match &self.screen {
            Screen::Welcome(_) => format!("Welcome — {APP_NAME}"),
            Screen::Document(doc) if doc.saving() => {
                format!("{} — Saving… — {APP_NAME}", doc.title_name())
            }
            Screen::Document(doc) if doc.edited() => {
                format!("{} — Edited — {APP_NAME}", doc.title_name())
            }
            Screen::Document(doc) => format!("{} — {APP_NAME}", doc.title_name()),
        }
    }

    /// Whether a shortcut of the screen takes Tab, which otherwise backs
    /// out as Escape does, see `escape_key`.
    fn tab_taken(&self) -> bool {
        let Screen::Document(doc) = &self.screen else {
            return false;
        };
        doc.keys().is_some_and(|keys| {
            let tab = keyboard::Key::Named(key::Named::Tab);
            varde_view::claimed(
                varde_view::document_bindings(keys),
                &tab,
                keyboard::Modifiers::empty(),
            )
        })
    }

    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let doc = self.screen.doc();
        let dialog = match &self.screen {
            Screen::Document(doc) => doc.dialog(),
            Screen::Welcome(welcome) => welcome.dialog(),
        };
        Subscription::batch([
            keyboard::listen().filter_map(peek_key),
            keyboard::listen().filter_map(command_key),
            keyboard::listen()
                .with((dialog, self.tab_taken()))
                .filter_map(escape_key),
            // The release is never seen if the window loses focus while
            // the peek key is held, e.g. to an Alt+Tab.
            window::events().filter_map(unfocused),
            match &self.screen {
                // No key opens anything behind the prompt.
                Screen::Welcome(welcome) => keyboard::listen()
                    .with(welcome.dialog().is_none())
                    .filter_map(welcome_key),
                Screen::Document(doc) => {
                    keyboard::listen().with(doc.keys()).filter_map(document_key)
                }
            },
            // Closing waits for saves and asks about unsaved changes, see
            // `run`.
            window::close_requests().map(Message::CloseRequested),
            // On the web, the page going away instead.
            platform::leaving(),
            // On the web, files dragged over the page and dropped on it:
            // always, so that the browser never takes one, which would
            // leave the page for it. Only the welcome screen opens them.
            platform::drops(),
            // The browser asks before the page goes, while it would lose
            // changes.
            only_if(self.at_stake(), platform::guard),
            // While the camera turns, and edits wait on the solver or the
            // model on regeneration until they've waited long enough to
            // say so; and while a thumbnail is rendered, by the
            // viewport's next frame, and on the web read back on a later
            // one.
            only_if(
                doc.is_some_and(|doc| {
                    doc.animating()
                        || doc.timing()
                        || doc.feed.timing(&doc.editor)
                        || doc.rendering_thumbnail()
                }),
                || window::frames().map(Message::AnimationFrame),
            ),
            iced::system::theme_changes().map(Message::SystemTheme),
            self.regen_lane(),
            self.solve_lane(),
            Self::io_lane(),
            // Also giving up on a thumbnail that isn't drawn.
            only_if(
                self.auto_saving() || doc.is_some_and(Doc::rendering_thumbnail),
                platform::auto_save_ticks,
            ),
        ])
    }

    /// Whether leaving now would lose changes, unsaved or on their way to
    /// the file: on the web the browser asks before the page goes while
    /// it's so, see `platform::guard`.
    fn at_stake(&self) -> bool {
        self.screen.doc().is_some_and(|doc| doc.at_stake())
    }

    /// Whether the auto-save timer runs: for an editable document. It only
    /// sends anything once there's something to auto-save, see
    /// [`Doc::auto_save`].
    fn auto_saving(&self) -> bool {
        self.quitting.is_none() && self.screen.doc().is_some_and(|doc| doc.editable())
    }

    /// Runs the IO lane, for as long as the app: yields
    /// [`Message::IoReady`] once it has started, then its responses.
    fn io_lane() -> Subscription<Message> {
        use iced::futures::{StreamExt, future, stream};

        fn start() -> impl iced::futures::Stream<Item = Message> {
            let (lane, responses) = varde_io::lane::spawn();
            stream::once(future::ready(Message::IoReady(lane))).chain(responses.map(Message::Io))
        }

        Subscription::run(start)
    }

    /// Runs the open document's regeneration lane: yields [`ForDoc::RegenReady`]
    /// once it has started, then its responses. Keyed by the document, so
    /// the lane ends with it (the thread stops, the worker is terminated)
    /// and a new document gets a new one.
    fn regen_lane(&self) -> Subscription<Message> {
        fn start(id: &DocId) -> impl Stream<Item = Message> + use<> {
            let lane = varde_regen::lane::spawn();
            doc_lane(*id, lane, ForDoc::RegenReady, ForDoc::Computed)
        }
        match self.screen.doc() {
            Some(doc) => Subscription::run_with(doc.id, start),
            None => Subscription::none(),
        }
    }

    /// Runs the open document's solver lane: yields [`ForDoc::SolveReady`]
    /// once it has started, then its responses. Keyed by the document, like
    /// [`Varde::regen_lane`], so the lane ends with it.
    fn solve_lane(&self) -> Subscription<Message> {
        fn start(id: &DocId) -> impl Stream<Item = Message> + use<> {
            let lane = varde_solve::lane::spawn();
            doc_lane(*id, lane, ForDoc::SolveReady, ForDoc::Solved)
        }
        match self.screen.doc() {
            Some(doc) => Subscription::run_with(doc.id, start),
            None => Subscription::none(),
        }
    }

    /// Whether the UI is light or dark: as chosen, or as the system
    /// prefers.
    fn mode(&self) -> Mode {
        self.options.theme.mode(self.system)
    }

    pub(crate) fn theme(&self) -> iced::Theme {
        varde_view::iced_theme(self.mode())
    }
}

impl Default for Varde {
    fn default() -> Self {
        Self::new()
    }
}

/// The messages of a lane started for the document `id`, as `(lane,
/// responses)`: `ready` with the lane, then each response as `answer`
/// makes it.
fn doc_lane<L, S: Stream>(
    id: DocId,
    (lane, responses): (L, S),
    ready: fn(L) -> ForDoc,
    answer: fn(S::Item) -> ForDoc,
) -> impl Stream<Item = Message> + use<L, S> {
    use iced::futures::{StreamExt, future, stream};

    stream::once(future::ready(Message::Doc(id, ready(lane))))
        .chain(responses.map(move |response| Message::Doc(id, answer(response))))
}

/// Whether `message` still acts while the window waits for the IO lane to
/// flush: the lane's own and those that only change the view, none of which
/// can send the lane more work, see `Varde::quit`. Not an export's welded
/// bodies, which would be sent to be written, nor a theme chosen or the
/// mouse's hints turned on or off, which would be stored.
fn while_quitting(message: &Message) -> bool {
    matches!(
        message,
        Message::IoReady(_)
            | Message::Io(_)
            | Message::Doc(
                _,
                ForDoc::RegenReady(_)
                    | ForDoc::Computed(
                        varde_regen::Response::Regenerated { .. }
                            | varde_regen::Response::Failed { .. }
                    )
                    | ForDoc::SolveReady(_)
                    | ForDoc::Solved(_)
            )
            | Message::AnimationFrame(_)
            | Message::PeekPanel(_)
            | Message::CommandHeld(_)
            | Message::SystemTheme(_)
            | Message::StorageState { .. }
            | Message::Persisted(_)
            | Message::Ui(
                Ui::Look(_)
                    | Ui::ToggleHiddenEdges
                    | Ui::SetEdges(_)
                    | Ui::SetShading(_)
                    | Ui::Copy(_)
            )
    )
}

/// Waits for the thumbnail tagged `tag` of document `id` to come back on
/// `answer`, see `doc/thumbnail.rs`: `None` if it couldn't be rendered,
/// its sender dropped uncalled.
fn thumbnail(
    id: DocId,
    tag: u64,
    answer: iced::futures::channel::oneshot::Receiver<Option<varde_view::ThumbnailImages>>,
) -> Task<Message> {
    use varde_io::thumbnail::{Image, Thumbnail};
    Task::perform(answer, move |images| {
        let image =
            |image: varde_render::PreviewImage| Image::new(image.width, image.height, image.rgba);
        let thumbnail = images.ok().flatten().and_then(|images| {
            Some(Thumbnail {
                light: image(images.light)?,
                dark: image(images.dark)?,
            })
        });
        Message::Doc(id, ForDoc::Thumbnail(tag, thumbnail))
    })
}

/// Reports `result` if it failed to do `what`, for the IO lane's answers
/// that are only reported, not shown.
fn report_failure(what: &str, result: Result<(), String>) {
    if let Err(error) = result {
        log::error!("Couldn't {what}: {error}");
    }
}

/// `subscription` while `on`, and none otherwise.
fn only_if(
    on: bool,
    subscription: impl FnOnce() -> Subscription<Message>,
) -> Subscription<Message> {
    if on {
        subscription()
    } else {
        Subscription::none()
    }
}

/// Holding the peek key peeks at the other side panel tab.
fn peek_key(event: keyboard::Event) -> Option<Message> {
    match event {
        keyboard::Event::ModifiersChanged(modifiers) => {
            Some(Message::PeekPanel(Held::PEEK.is_held(modifiers)))
        }
        _ => None,
    }
}

/// Tells whether the command modifier is held, see [`Varde::command`].
fn command_key(event: keyboard::Event) -> Option<Message> {
    match event {
        keyboard::Event::ModifiersChanged(modifiers) => {
            Some(Message::CommandHeld(modifiers.command()))
        }
        _ => None,
    }
}

/// Escape, or Tab where no shortcut takes it (`tab_taken`), given the
/// prompt the user is being asked, if any: cancels it (staying, or
/// deleting nothing), and otherwise backs out of what's open, see
/// [`Look::Escape`].
fn escape_key(
    ((dialog, tab_taken), event): ((Option<Dialog>, bool), keyboard::Event),
) -> Option<Message> {
    // Escape with any modifiers, unlike a shortcut.
    let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
        return None;
    };
    let tab = key == keyboard::Key::Named(key::Named::Tab);
    if !varde_view::escapes(&key, modifiers) || (tab && tab_taken) {
        return None;
    }
    Some(Message::Ui(match dialog {
        Some(Dialog::Unsaved) => Ui::File(File::Unsaved(Unsaved::Cancel)),
        Some(Dialog::Delete) => Ui::Look(Look::CancelDelete),
        Some(Dialog::Damaged) => Ui::Welcome(varde_view::Welcome::CancelDamaged),
        Some(Dialog::Panic) => Ui::Welcome(varde_view::Welcome::ClosePanic),
        Some(Dialog::Naming) => Ui::File(File::CancelName),
        Some(Dialog::DeleteFromBrowser) => Ui::Welcome(varde_view::Welcome::CancelDelete),
        None => Ui::Look(Look::Escape),
    }))
}

/// Stops peeking once the window loses focus.
fn unfocused((_, event): (window::Id, window::Event)) -> Option<Message> {
    matches!(event, window::Event::Unfocused).then_some(Message::PeekPanel(false))
}

/// Has the value field take the focus, and select its text as `focus`
/// says: all of it to overtype, or the part a refusal is about. An
/// operation panel's body, where the field may be, scrolls back to its
/// top if the field shows from there, else down to the field
/// ([`RevealField`]): an extrude edited while another's panel was
/// scrolled keeps that panel, and so its scroll.
fn focus_field(focus: Focus) -> Task<Message> {
    use iced::widget::operation;

    let select = match focus {
        Focus::All => operation::select_all(varde_view::VALUE_FIELD),
        Focus::Range(start, end) => operation::select_range(varde_view::VALUE_FIELD, start, end),
    };
    operation::focus(varde_view::VALUE_FIELD)
        .chain(select)
        .chain(iced::advanced::widget::operate(RevealField::default()))
}

/// Scrolls an operation panel's body so the value field in it shows with
/// its label over it: to the body's top where the field shows from
/// there, else with the label at the top. A field outside the body (a
/// dimension's, in a sketch) leaves it be.
#[derive(Default)]
struct RevealField {
    /// The body's bounds and its content's.
    body: Option<(iced::Rectangle, iced::Rectangle)>,
    field: Option<iced::Rectangle>,
}

impl RevealField {
    /// How far above the field its label starts, with a little room.
    const LABEL: f32 = 24.0;
}

impl<T: Send + 'static> iced::advanced::widget::Operation<T> for RevealField {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<T>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        bounds: iced::Rectangle,
        content_bounds: iced::Rectangle,
        _translation: iced::Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&varde_view::PANEL_BODY) {
            self.body = Some((bounds, content_bounds));
        }
    }

    fn focusable(
        &mut self,
        id: Option<&iced::widget::Id>,
        bounds: iced::Rectangle,
        _state: &mut dyn iced::advanced::widget::operation::Focusable,
    ) {
        if id == Some(&varde_view::VALUE_FIELD) {
            self.field = Some(bounds);
        }
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<T> {
        use iced::advanced::widget::operation::{Outcome, scrollable};

        let (Some((body, content)), Some(field)) = (self.body, self.field) else {
            return Outcome::None;
        };
        // Where the field is in the content, which isn't moved by the
        // scroll in layout.
        let (top, bottom) = (field.y - content.y, field.y + field.height - content.y);
        if top < 0.0 || bottom > content.height {
            return Outcome::None;
        }
        let y = if bottom <= body.height {
            0.0
        } else {
            (top - Self::LABEL).max(0.0)
        };
        let offset = scrollable::AbsoluteOffset {
            x: None,
            y: Some(y),
        };
        Outcome::Chain(Box::new(scrollable::scroll_to(
            varde_view::PANEL_BODY,
            offset,
        )))
    }
}

/// Has the Save As dialog's name field take the focus, its text selected
/// to overtype.
fn focus_name() -> Task<Message> {
    use iced::widget::operation;

    operation::focus(varde_view::NAME_FIELD).chain(operation::select_all(varde_view::NAME_FIELD))
}

/// Asks the browser what it says of its storage, answering with
/// [`Message::StorageState`]; natively it says nothing.
fn storage_state() -> Task<Message> {
    Task::perform(
        async {
            let persisted = varde_io::storage::persisted().await;
            let space = varde_io::storage::estimate().await;
            (persisted, space)
        },
        |(persisted, space)| Message::StorageState { persisted, space },
    )
}

/// Scrolls the rail's open list to `share` of the way to its end.
fn scroll_rail(share: f32) -> Task<Message> {
    use iced::widget::operation::{self, RelativeOffset};

    let offset = RelativeOffset {
        x: None,
        y: Some(share),
    };
    operation::snap_to(varde_view::RAIL_LIST, offset)
}

/// Asks the user for a design to open, answering with
/// [`Message::Picked`]: see `varde_io::pick`.
fn pick_open() -> Task<Message> {
    Task::perform(varde_io::pick::pick_open(), Message::Picked)
}

/// Asks the user where to save the document `id`, suggesting `name`,
/// answering with [`ForDoc::SaveAsPicked`].
fn pick_save_as(id: DocId, name: &str) -> Task<Message> {
    let name = name.to_owned();
    Task::perform(
        async move { varde_io::pick::pick_save(&name).await },
        move |chosen| Message::Doc(id, ForDoc::SaveAsPicked(chosen)),
    )
}

/// Asks the user where to export the document `id`'s bodies, suggesting
/// `name`, answering with [`ForDoc::ExportPicked`].
fn pick_export(id: DocId, name: &str) -> Task<Message> {
    let name = name.to_owned();
    Task::perform(
        async move { varde_io::pick::pick_export(&name).await },
        move |chosen| Message::Doc(id, ForDoc::ExportPicked(chosen)),
    )
}

/// Asks the browser whether the document `id` may be saved to the file
/// the user `picked`, answering with [`ForDoc::Writable`]: see
/// `varde_io::pick`.
fn ask_writable(id: DocId, picked: Picked) -> Task<Message> {
    Task::perform(
        async move { varde_io::pick::writable(&picked).await },
        move |result| Message::Doc(id, ForDoc::Writable(result)),
    )
}

/// What [`run`] ends with, so the executables need not name iced. The
/// parameters default to that and otherwise make it `std`'s `Result`.
pub type Result<T = (), E = iced::Error> = std::result::Result<T, E>;

/// Runs the app. Sets the panic hook first (see [`varde_io::panicked`]),
/// keeping the one the frontend set, which logs.
pub fn run() -> Result {
    varde_io::panicked::install();
    iced::application(Varde::boot, Varde::update, Varde::view)
        .title(Varde::title)
        .subscription(Varde::subscription)
        .theme(Varde::theme)
        .settings(iced::Settings {
            default_text_size: 13.into(),
            antialiasing: true,
            ..Default::default()
        })
        .window(iced::window::Settings {
            size: (1280.0, 800.0).into(),
            icon: platform::window_icon(),
            // Closing may have to wait for a save or ask about unsaved
            // changes; `Varde::subscription` hears the request instead.
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}

#[cfg(test)]
mod tests;
