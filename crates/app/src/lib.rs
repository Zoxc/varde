//! The varde application: owns all state and handles its `Message`s.
//!
//! Frontends (`binary`, `web`) only call [`run`].

mod doc;
mod io;
mod keys;
#[cfg(not(target_arch = "wasm32"))]
mod mcp;
mod message;
mod platform;
mod recent;
mod samples;
mod settings;
mod toast;
mod welcome;
mod when;
mod words;

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
use crate::toast::Toast;
use crate::welcome::Welcome;
use varde_view::Setting;

pub(crate) struct Varde {
    screen: Screen,
    /// The mode the system prefers, if it says: the theme's when the user
    /// chose [`ThemeChoice::Auto`](varde_view::ThemeChoice::Auto).
    system: Option<Mode>,
    /// What the view options menu turns on and off, and the theme chosen.
    options: ViewOptions,
    /// What's stored of `options`.
    settings: Settings,
    /// The settings page: whether it shows, and how.
    page: Page,
    /// Where the settings are kept, as the page's foot says it: `None`
    /// natively without a config directory.
    settings_shown: Option<String>,
    /// Whether the peek key is held, see [`Held::PEEK`].
    peeking: bool,
    /// Whether the command modifier (`Ctrl`, or `Cmd` on macOS) is held,
    /// with which clicking a list's row adds to the selection.
    command: bool,
    files: Files,
    /// The window to close once the IO lane is done, see [`Varde::quit`].
    quitting: Option<window::Id>,
    /// The short message showing over the screen for a few seconds, if
    /// one is: a document's ([`Doc::take_toast`]) or the app's.
    toast: Toast,
    /// Whether the welcome screen carries [`varde_view::warm_up`]: once
    /// the IO lane first answers, after the screen has shown, so building
    /// the viewport's pipelines neither holds up its first frame nor waits
    /// for the first design.
    warm_up: bool,
    /// The MCP server's state, see `mcp.rs`.
    #[cfg(not(target_arch = "wasm32"))]
    mcp: mcp::Mcp,
}

/// The settings page's own state: the app's, not a document's, but
/// closed by any move to another view (see [`Varde::show`]).
#[derive(Debug, Default)]
struct Page {
    open: bool,
    tab: varde_view::SettingsTab,
    /// Whether Reset to defaults is asking first.
    resetting: bool,
    /// The interface scale the slider is dragged to, set once let go.
    scale_dragged: Option<u16>,
    /// The MCP port as typed, while it isn't the port set.
    port_typed: Option<String>,
    /// Whether the MCP token shows.
    token_shown: bool,
}

impl Page {
    /// Closes the page, with what it was asking or being dragged to.
    fn close(&mut self) {
        self.open = false;
        self.resetting = false;
        // A drag not let go on the page sets nothing.
        self.scale_dragged = None;
        // Nor a port typed out of range; the token is hidden again.
        self.port_typed = None;
        self.token_shown = false;
    }
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
    /// The units a new design starts in: the user's setting, kept in
    /// step with it by `Varde::update`.
    new_units: varde_document::LengthUnit,
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
            new_units: varde_document::LengthUnit::default(),
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
        // The MCP server waits for its token, even while switched off, so
        // switching it on starts it at once.
        #[cfg(not(target_arch = "wasm32"))]
        files.io.send(IoRequest::LoadMcpToken);
        Self {
            screen: Screen::Welcome(Welcome::default()),
            system: None,
            options: ViewOptions::default(),
            settings: Settings::default(),
            page: Page::default(),
            settings_shown: settings_shown(None),
            peeking: false,
            command: false,
            files,
            quitting: None,
            toast: Toast::default(),
            warm_up: false,
            #[cfg(not(target_arch = "wasm32"))]
            mcp: mcp::Mcp::default(),
        }
    }

    /// The app as it starts, and the task asking for the mode the system
    /// prefers, which [`Varde::subscription`] hears of changes to after.
    fn boot() -> (Self, Task<Message>) {
        #[allow(unused_mut, reason = "only native reads the settings early")]
        let mut app = Self::new();
        // Natively the stored theme is read before the window first draws,
        // which the IO lane's answer would come after: so it doesn't open
        // light and turn dark. The lane's answer still follows.
        #[cfg(not(target_arch = "wasm32"))]
        {
            Settings::preview(varde_io::settings::read_now(), &mut app.options);
            app.options_changed();
        }
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
        // The MCP calls done send their replies, and those of a server
        // stopped are ended.
        #[cfg(not(target_arch = "wasm32"))]
        self.mcp.sync(self.screen.doc_mut());
        // A new design starts in the units of the settings.
        self.files.new_units = self.options.length_unit;
        // A value field opened, or a value refused, has the field take the
        // focus once it shows.
        let task = match self.screen.doc_mut().and_then(Doc::take_focus) {
            Some(focus) => Task::batch([task, focus_field(focus)]),
            None => task,
        };
        // The Timeline scrolled to the feature the MCP panel showed.
        #[cfg(not(target_arch = "wasm32"))]
        let task = match self.screen.doc_mut().and_then(|doc| doc.mcp.scroll.take()) {
            Some(share) => Task::batch([task, snap_list(varde_view::TIMELINE_LIST, share)]),
            None => task,
        };
        // The rail's list scrolled to show the row the keys moved to.
        let task = match self.screen.doc_mut().and_then(|doc| doc.rail.take_scroll()) {
            Some(share) => Task::batch([task, snap_list(varde_view::RAIL_LIST, share)]),
            None => task,
        };
        // The rename field takes the focus as it opens, its text selected.
        let task = match self.screen.doc_mut().is_some_and(Doc::take_rename_focus) {
            true => Task::batch([task, focus_rename()]),
            false => task,
        };
        // A parameter just added has its name take the focus, its text
        // selected.
        let task = match self.screen.doc_mut().and_then(Doc::take_param_focus) {
            Some(index) => Task::batch([task, focus_param(index)]),
            None => task,
        };
        // What the document has to say shows as a toast.
        if let Some(message) = self.screen.doc_mut().and_then(Doc::take_toast) {
            self.toast.show(message, iced::time::Instant::now());
        }
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
        // The pictures MCP calls asked for, drawn by the viewport's next
        // frame.
        #[cfg(not(target_arch = "wasm32"))]
        let task = match self.screen.doc_mut() {
            Some(doc) => {
                let id = doc.id;
                let renders = doc.mcp.take_renders().into_iter();
                Task::batch(
                    std::iter::once(task)
                        .chain(renders.map(|(tag, answer)| mcp_render(id, tag, answer))),
                )
            }
            None => task,
        };
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
                // Only from the welcome screen: one sent on from it once a
                // document shows makes nothing new.
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
            Message::Tick(now) => {
                #[cfg(not(target_arch = "wasm32"))]
                self.mcp.tick(now, self.screen.doc_mut());
                let wait = auto_save_wait(&self.options);
                return self.step(|doc, cx| {
                    doc.auto_save(cx, now, wait);
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
            // A call is answered at once from the open design. Not while
            // quitting (see `while_quitting`): its reply dropped answers
            // that Varde is closing.
            #[cfg(not(target_arch = "wasm32"))]
            Message::Mcp(start, event) => self.mcp.event(start, event, self.screen.doc_mut()),
            Message::Io(response) => {
                self.warm_up = true;
                return self.io_response(response);
            }

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
                    Look::ClickFeature { feature, .. } => Look::ClickFeature {
                        feature,
                        roll: self.command,
                    },
                    message => message,
                };
                // The delete prompt cancelled may free what waited for it.
                return self.step(|doc, cx| {
                    doc.look(message);
                    doc.proposals_settled(cx)
                });
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
                self.set(Setting::MouseHints(!self.options.mouse_hints));
            }
            Message::Ui(Ui::ToggleHiddenEdges) => {
                self.set(Setting::HiddenEdges(!self.options.hidden_edges));
            }
            Message::Ui(Ui::ToggleNames) => self.set(Setting::Names(!self.options.names)),
            Message::Ui(Ui::SetEdges(edges)) => self.set(Setting::Edges(edges)),
            Message::Ui(Ui::SetShading(shading)) => self.set(Setting::Shading(shading)),
            Message::Ui(Ui::SetProjection(projection)) => {
                self.set(Setting::Projection(projection));
            }
            Message::Ui(Ui::ToggleSettings) => {
                let open = !self.page.open;
                self.page.close();
                self.page.open = open;
                self.with_doc(|doc, _| doc.view_menu = false);
            }
            Message::Ui(Ui::Settings(message)) => self.settings_page(message),
            Message::Ui(Ui::McpPanel(message)) => self.mcp_panel(message),
            Message::PeekPanel(peeking) => {
                // The tab shown changes: the Timeline's rows go, or come,
                // without an exit or an enter.
                if peeking != self.peeking {
                    self.with_doc(|doc, _| doc.look(Look::HoverFeature(None)));
                }
                self.peeking = peeking;
            }
            Message::CommandHeld(held) => {
                self.command = held;
                self.with_doc(|doc, _| doc.hold_command(held));
            }
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
            Message::ToastTick(now) => self.toast.tick(now),
            Message::AnimationFrame(now) => self.with_doc(|doc, _| {
                doc.animation_frame(now);
                doc.tick(now);
                doc.feed.tick(&doc.editor, now);
            }),
        }
        Task::none()
    }

    /// Sets `setting`, if it's a value the page offers (others are
    /// ignored), closing the view options menu, and stores it.
    fn set(&mut self, setting: Setting) {
        if !setting.offered() {
            return;
        }
        setting.apply(&mut self.options);
        self.with_doc(|doc, _| doc.view_menu = false);
        self.options_changed();
        if let Some(write) = self.settings.chose(setting, self.options) {
            self.files.io.send(write);
        }
    }

    /// Has the open document take the options as they are now: their
    /// projection, its camera's, and how it's turned; the values shown
    /// as they say; and the recent files kept as many as they say.
    fn options_changed(&mut self) {
        let options = self.options;
        options.shown().set();
        self.with_doc(|doc, _| doc.take_options(&options));
        if let Some(write) = self.files.recent.keep(usize::from(options.recent_designs)) {
            self.files.io.send(write);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.mcp.configure(options.mcp, options.mcp_port);
    }

    /// Takes `message`, from the settings page itself.
    fn settings_page(&mut self, message: varde_view::SettingsPage) {
        use varde_view::SettingsPage;
        match message {
            SettingsPage::Tab(tab) => {
                self.page.tab = tab;
                self.page.scale_dragged = None;
            }
            // Only the slider's label follows a drag, so the page doesn't
            // rescale under the pointer: the scale is set once let go.
            SettingsPage::DragScale(percent) => {
                self.page.scale_dragged =
                    varde_view::interface_scale_offered(percent).then_some(percent);
            }
            SettingsPage::ReleaseScale => {
                if let Some(percent) = self.page.scale_dragged.take() {
                    self.set(Setting::InterfaceScale(percent));
                }
            }
            SettingsPage::Set(setting) => {
                // A reset or a value chosen replaces what's typed.
                if matches!(setting, Setting::McpPort(_)) {
                    self.page.port_typed = None;
                }
                self.set(setting);
            }
            // Set once it's a port the field takes; kept as typed while
            // it isn't the port set, as an error out of range.
            SettingsPage::TypeMcpPort(typed) => {
                if let Some(port) = varde_view::mcp_port_of(&typed)
                    && port != self.options.mcp_port
                {
                    self.set(Setting::McpPort(port));
                }
                self.page.port_typed =
                    (typed != self.options.mcp_port.to_string()).then_some(typed);
            }
            SettingsPage::ShowMcpToken(shown) => self.page.token_shown = shown,
            #[cfg(not(target_arch = "wasm32"))]
            SettingsPage::DisconnectMcp => self.mcp.disconnect(),
            #[cfg(target_arch = "wasm32")]
            SettingsPage::DisconnectMcp => {}
            // With nothing to reset, the button is off.
            SettingsPage::AskReset => {
                self.page.resetting = !varde_view::changed_settings(self.options).is_empty();
            }
            SettingsPage::CancelReset => self.page.resetting = false,
            // Answered with the new token, which restarts the server
            // and so ends the session.
            #[cfg(not(target_arch = "wasm32"))]
            SettingsPage::RegenerateMcpToken => self.files.io.send(IoRequest::RegenerateMcpToken),
            #[cfg(target_arch = "wasm32")]
            SettingsPage::RegenerateMcpToken => {}
            SettingsPage::Reset => {
                // Only as asked: a stray Enter resets nothing.
                if std::mem::take(&mut self.page.resetting) {
                    self.page.port_typed = None;
                    self.options = ViewOptions::default();
                    self.options_changed();
                    if let Some(write) = self.settings.chose_all(self.options) {
                        self.files.io.send(write);
                    }
                }
            }
        }
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
            Next::Show(mut doc) => {
                // In the projection of the settings, turned as they say.
                doc.take_options(&self.options);
                self.show(Screen::Document(doc));
                Task::none()
            }
            Next::Left(Leave::Close) => {
                self.show(Screen::Welcome(Welcome::default()));
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

    /// Shows `screen`: the one place the screen changes, between the
    /// welcome screen and a document or from one document to another
    /// (a new design, an open, a close). The settings page closes on
    /// each, so what's moved to shows; a cancelled or failed open or
    /// close never gets here and leaves it as it is.
    fn show(&mut self, screen: Screen) {
        // The MCP calls waiting on the design before fail.
        #[cfg(not(target_arch = "wasm32"))]
        self.mcp.closing(self.screen.doc_mut());
        self.screen = screen;
        self.page.close();
        // The MCP panel's rows were of the design before.
        #[cfg(not(target_arch = "wasm32"))]
        self.mcp.design_closed();
    }

    /// Closes `window`, which quits, once the IO lane has done everything
    /// asked of it: the last saves and closes, which delete the lock files,
    /// and the recent files list. Exiting before would cut its thread off.
    /// Until then only messages [`while_quitting`] lets through act.
    fn quit(&mut self, window: window::Id) -> Task<Message> {
        self.quitting = Some(window);
        // The MCP calls waiting are answered now: neither their answers
        // nor the ticks that bound them act from here on.
        #[cfg(not(target_arch = "wasm32"))]
        self.mcp.quitting(self.screen.doc_mut());
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
                // The home directory as `~`, now it's known.
                self.settings_shown = settings_shown(Some(&self.files));
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
                self.options_changed();
            }
            IoResponse::SettingsWritten { result } => {
                report_failure("save the settings", result);
            }
            IoResponse::PanicLoaded { panic } => self.files.panic = panic,
            #[cfg(not(target_arch = "wasm32"))]
            IoResponse::PngEncoded { owner, tag, result } => {
                if let Some(doc) = self.screen.doc_mut()
                    && doc.id.number() == owner
                {
                    doc.mcp_encoded(tag, result);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            IoResponse::McpToken {
                regenerated,
                result,
            } => match result {
                Ok(token) => {
                    let (on, port) = (self.options.mcp, self.options.mcp_port);
                    let doc = self.screen.doc_mut();
                    self.mcp.token_loaded(token, regenerated, on, port, doc);
                }
                Err(error) => {
                    let what = if regenerated {
                        "make a new MCP token"
                    } else {
                        "read the MCP token"
                    };
                    report_failure(what, Err(error));
                }
            },
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
            IoResponse::Exported { to, result } => match self.screen.doc_mut() {
                Some(doc) => doc.export_written(&to, result),
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
        // The view's functions show values as the settings say.
        self.options.shown().set();
        let settings = self.page.open.then(|| varde_view::SettingsState {
            options: self.options,
            tab: self.page.tab,
            resetting: self.page.resetting,
            scale_dragged: self.page.scale_dragged,
            kept_in: match (&self.settings_shown, self.files.browser_storage) {
                (_, true) => varde_view::KeptIn::Browser,
                (Some(path), false) => varde_view::KeptIn::File(path),
                (None, false) => varde_view::KeptIn::Nowhere,
            },
            mcp: self.mcp_shown(),
        });
        let view = match &self.screen {
            Screen::Welcome(welcome) => {
                let view = welcome.view(&self.files, self.mode(), settings);
                match self.warm_up {
                    true => iced::widget::stack![view, varde_view::warm_up()].into(),
                    false => view,
                }
            }
            Screen::Document(doc) => doc.view_with(
                self.peeking,
                self.mode(),
                self.options,
                self.files.offers(),
                settings,
                self.mcp_panel_shown(),
            ),
        };
        let view = match self.toast.message() {
            Some(message) => iced::widget::stack![view, varde_view::toast(message)].into(),
            None => view,
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
        self.doc_keys().is_some_and(|keys| {
            let tab = keyboard::Key::Named(key::Named::Tab);
            varde_view::claimed(
                varde_view::document_bindings(keys),
                &tab,
                keyboard::Modifiers::empty(),
            )
        })
    }

    /// What the open document's shortcuts depend on, if it has any now:
    /// none while Reset to defaults asks, and only a few with the
    /// settings page in the viewport's place, see [`Doc::keys`].
    fn doc_keys(&self) -> Option<varde_view::DocumentKeys> {
        let keys = self.screen.doc()?.keys()?;
        (!self.page.resetting).then(|| keys.with_settings(self.page.open))
    }

    /// What `Esc` does on the settings page, if it's open: cancels Reset
    /// to defaults while it asks, else closes the page.
    fn page_escape(&self) -> Option<PageEscape> {
        match (self.page.open, self.page.resetting) {
            (false, _) => None,
            (true, true) => Some(PageEscape::CancelReset),
            (true, false) => Some(PageEscape::Close),
        }
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
                .with((dialog, self.tab_taken(), self.page_escape()))
                .filter_map(escape_key),
            // Enter resets, as Reset to defaults asks, but not under a
            // prompt over it.
            only_if(self.page.resetting && dialog.is_none(), || {
                keyboard::listen().filter_map(reset_key)
            }),
            // The release is never seen if the window loses focus while
            // the peek key is held, e.g. to an Alt+Tab.
            window::events().filter_map(unfocused),
            match &self.screen {
                // No key opens anything behind the prompt.
                Screen::Welcome(welcome) => keyboard::listen()
                    .with(welcome.dialog().is_none() && !self.page.resetting)
                    .filter_map(welcome_key),
                Screen::Document(_) => keyboard::listen()
                    .with(self.doc_keys())
                    .filter_map(document_key),
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
            // one, or a picture an MCP call asked for.
            only_if(
                doc.is_some_and(|doc| {
                    doc.animating()
                        || doc.timing()
                        || doc.feed.timing(&doc.editor)
                        || doc.rendering_thumbnail()
                        || doc.drawing_for_mcp()
                }),
                || window::frames().map(Message::AnimationFrame),
            ),
            only_if(self.toast.message().is_some(), platform::toast_ticks),
            iced::system::theme_changes().map(Message::SystemTheme),
            self.regen_lane(),
            self.solve_lane(),
            Self::io_lane(),
            self.mcp_server(),
            // Also giving up on a thumbnail that isn't drawn, and ending
            // the MCP calls waiting too long.
            only_if(
                self.auto_saving() || doc.is_some_and(Doc::ticking),
                platform::second_ticks,
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

    /// The MCP panel's toggle and its head's Start and Stop: natively
    /// only. Start starts the server, or, running and the port not bound,
    /// tries it again; neither touches the settings.
    fn mcp_panel(&mut self, message: varde_view::McpPanel) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use varde_view::McpPanel;
            let (on, port) = (self.options.mcp, self.options.mcp_port);
            match message {
                McpPanel::Toggle => self.mcp.open = on && !self.mcp.open,
                McpPanel::Start => self.mcp.start(on, port),
                McpPanel::Stop => self.mcp.stop(on, port),
                McpPanel::ApplyYours => self.with_doc(|doc, _| doc.mcp_apply_yours()),
                McpPanel::KeepEditing => self.with_doc(|doc, _| doc.mcp_keep_editing()),
                McpPanel::OpenMenu(_)
                | McpPanel::CloseMenu
                | McpPanel::ShowInTimeline(_)
                | McpPanel::RevertToHere(_)
                | McpPanel::RevertBefore(_)
                | McpPanel::ConfirmRevert
                | McpPanel::CancelRevert => self.mcp.panel_message(message, self.screen.doc_mut()),
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = message;
    }

    /// What the document screen shows of the MCP server: natively only.
    pub(crate) fn mcp_panel_shown(&self) -> Option<varde_view::McpShown<'_>> {
        #[cfg(not(target_arch = "wasm32"))]
        let shown = Some(self.mcp.panel(
            self.options.mcp,
            self.options.mcp_port,
            when::now(),
            self.screen.doc(),
        ));
        #[cfg(target_arch = "wasm32")]
        let shown = None;
        shown
    }

    /// What the settings' MCP tab shows: natively only.
    pub(crate) fn mcp_shown(&self) -> varde_view::McpState<'_> {
        #[cfg(not(target_arch = "wasm32"))]
        let shown = varde_view::McpState {
            token_shown: self.page.token_shown,
            port_typed: self.page.port_typed.as_deref(),
            ..self.mcp.shown(when::now())
        };
        #[cfg(target_arch = "wasm32")]
        let shown = varde_view::McpState::default();
        shown
    }

    /// Runs the MCP server while it's on and the app isn't quitting: see
    /// `mcp.rs`. Natively only.
    fn mcp_server(&self) -> Subscription<Message> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.mcp.subscription(self.quitting.is_none());
        #[cfg(target_arch = "wasm32")]
        Subscription::none()
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

    /// How large the UI is drawn, the model aside: the interface scale
    /// chosen, over the system's.
    fn scale_factor(&self) -> f32 {
        self.options.scale_factor()
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

/// How long after the first change not auto-saved a document is
/// auto-saved, as the settings say, or `None` for never: natively. The
/// web, which has no such setting, always waits [`doc::AUTO_SAVE_WAIT`].
fn auto_save_wait(options: &ViewOptions) -> Option<std::time::Duration> {
    if cfg!(target_arch = "wasm32") {
        return Some(doc::AUTO_SAVE_WAIT);
    }
    // At most 10 minutes as offered; bounded all the same.
    let minutes = options.auto_save.min(60);
    (minutes > 0).then(|| std::time::Duration::from_secs(u64::from(minutes) * 60))
}

/// Whether `message` still acts while the window waits for the IO lane to
/// flush: the lane's own and those that only change the view, none of which
/// can send the lane more work, see `Varde::quit`. Not an export's welded
/// bodies, which would be sent to be written, nor a setting chosen, which
/// would be stored.
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
            | Message::ToastTick(_)
            | Message::PeekPanel(_)
            | Message::CommandHeld(_)
            | Message::SystemTheme(_)
            | Message::StorageState { .. }
            | Message::Persisted(_)
            | Message::Ui(
                Ui::Look(_)
                    | Ui::ToggleSettings
                    | Ui::Settings(
                        varde_view::SettingsPage::Tab(_)
                            | varde_view::SettingsPage::DragScale(_)
                            | varde_view::SettingsPage::AskReset
                            | varde_view::SettingsPage::CancelReset
                            | varde_view::SettingsPage::ShowMcpToken(_)
                    )
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

/// Waits for the picture of the MCP `render` call tagged `tag` of
/// document `id` to come back on `answer`, see `mcp/query/render.rs`:
/// `None` if it couldn't be drawn, its sender dropped uncalled.
#[cfg(not(target_arch = "wasm32"))]
fn mcp_render(
    id: DocId,
    tag: u64,
    answer: iced::futures::channel::oneshot::Receiver<Option<varde_render::PreviewImage>>,
) -> Task<Message> {
    Task::perform(answer, move |image| {
        let image = image.ok().flatten().and_then(|image| {
            varde_io::thumbnail::Image::new(image.width, image.height, image.rgba)
        });
        Message::Doc(id, ForDoc::McpRendered(tag, image))
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
    ((dialog, tab_taken, page), event): (
        (Option<Dialog>, bool, Option<PageEscape>),
        keyboard::Event,
    ),
) -> Option<Message> {
    // Escape with any modifiers, unlike a shortcut.
    let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
        return None;
    };
    let tab = key == keyboard::Key::Named(key::Named::Tab);
    if !varde_view::escapes(&key, modifiers) || (tab && tab_taken) {
        return None;
    }
    // The settings page, over what's open in the document, takes it
    // before that does, but not before a prompt over it: Escape cancels
    // Reset's or closes the page, and Tab does nothing there.
    if let (None, Some(page)) = (dialog, page) {
        let message = match page {
            PageEscape::CancelReset => Ui::Settings(varde_view::SettingsPage::CancelReset),
            PageEscape::Close => Ui::ToggleSettings,
        };
        return (!tab).then_some(Message::Ui(message));
    }
    Some(Message::Ui(match dialog {
        Some(Dialog::Unsaved) => Ui::File(File::Unsaved(Unsaved::Cancel)),
        Some(Dialog::Delete) => Ui::Look(Look::CancelDelete),
        Some(Dialog::McpAsk) => Ui::McpPanel(varde_view::McpPanel::KeepEditing),
        Some(Dialog::McpRevert) => Ui::McpPanel(varde_view::McpPanel::CancelRevert),
        Some(Dialog::Damaged) => Ui::Welcome(varde_view::Welcome::CancelDamaged),
        Some(Dialog::Panic) => Ui::Welcome(varde_view::Welcome::ClosePanic),
        Some(Dialog::Naming) => Ui::File(File::CancelName),
        Some(Dialog::DeleteFromBrowser) => Ui::Welcome(varde_view::Welcome::CancelDelete),
        None => Ui::Look(Look::Escape),
    }))
}

/// What `Esc` does on the settings page, see `Varde::page_escape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PageEscape {
    CancelReset,
    Close,
}

/// `Enter`, as Reset to defaults asks: resets.
fn reset_key(event: keyboard::Event) -> Option<Message> {
    match event {
        keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key::Named::Enter),
            repeat: false,
            ..
        } => Some(Message::Ui(Ui::Settings(varde_view::SettingsPage::Reset))),
        _ => None,
    }
}

/// Where the settings are kept, as the settings page's foot says it:
/// with the home directory as `~` once the recent `files` say where it
/// is. `None` without a config directory.
fn settings_shown(files: Option<&Files>) -> Option<String> {
    let store = varde_io::settings::store()?;
    Some(match files {
        Some(files) => files.shown_path(&store),
        None => store.display().to_string(),
    })
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

/// Has the rename field take the focus, its text selected to overtype.
fn focus_rename() -> Task<Message> {
    use iced::widget::operation;

    operation::focus(varde_view::RENAME_FIELD)
        .chain(operation::select_all(varde_view::RENAME_FIELD))
}

/// Has the name field of the parameter at `index` take the focus, its
/// text selected to overtype.
fn focus_param(index: usize) -> Task<Message> {
    use iced::widget::operation;

    let id = varde_view::param_name_id(index);
    operation::focus(id.clone()).chain(operation::select_all(id))
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

/// Scrolls the scrolled list `id` (the rail's open list, the Timeline)
/// to `share` of the way to its end.
fn snap_list(id: iced::widget::Id, share: f32) -> Task<Message> {
    use iced::widget::operation::{self, RelativeOffset};

    let offset = RelativeOffset {
        x: None,
        y: Some(share),
    };
    operation::snap_to(id, offset)
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
        .scale_factor(Varde::scale_factor)
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
