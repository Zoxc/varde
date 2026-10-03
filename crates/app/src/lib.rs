//! The varde application: owns all state and handles its `Message`s.
//!
//! Frontends (`binary`, `web`) only call [`run`].

mod doc;
mod io;
mod keys;
mod message;
mod platform;
mod recent;
mod settings;
mod welcome;
mod when;

use std::path::PathBuf;

use iced::futures::Stream;
use iced::keyboard::{self, key};
use iced::{Element, Subscription, Task, window};

use varde_document::APP_NAME;
use varde_document::Revision;
use varde_io::{
    Chosen, FileId, OpenId, Picked, Recovered, Request as IoRequest, Response as IoResponse,
    SaveError,
};
use varde_view::{File, Held, Look, Message as Ui, Mode, Unsaved, ViewOptions};

use crate::doc::{Dialog, Doc, DocId, Downloader, Downloads, Focus, Leave};
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
/// saving by download, and the lists the welcome screen shows.
struct Files {
    io: Io,
    downloads: Downloads,
    /// Saving by download: `None` where saves go to files, natively or on
    /// the web with the File System Access API. Tests set it natively.
    downloader: Option<Downloader>,
    recent: Recent,
    /// New designs left behind by sessions that crashed, newest first, and
    /// on the web designs downloaded and closed since.
    recovered: Vec<Recovered>,
    /// The recent files' thumbnails, by path, as last read: see
    /// [`Files::load_thumbnails`].
    thumbnails: Vec<(PathBuf, iced::widget::image::Handle)>,
}

impl Files {
    fn new(downloader: Option<Downloader>) -> Self {
        Self {
            io: Io::new(),
            downloads: Downloads::default(),
            downloader,
            recent: Recent::default(),
            recovered: Vec::new(),
            thumbnails: Vec::new(),
        }
    }

    /// Takes the answer to the store entry `id` asked for for a document
    /// gone since, see [`Downloads::settle`].
    fn settle_created(&mut self, id: OpenId, result: Result<FileId, String>) {
        self.downloads.settle(&mut self.io, id, result);
    }

    /// Takes the answer to a Save As whose document has gone since: the
    /// file it made is closed, as there's nothing left to write it.
    fn settle_saved_as(&mut self, result: Result<varde_io::SavedAs, SaveError>) {
        if let Ok(saved) = result {
            self.io.close_clean(saved.file);
        }
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
        files.io.send(IoRequest::ListRecovered);
        // The theme is the system's until the stored one arrives.
        files.io.send(IoRequest::LoadSettings);
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
        (Self::new(), iced::system::theme().map(Message::SystemTheme))
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
                    Look::PressLabel { id, .. } => Look::PressLabel {
                        id,
                        add: self.command,
                    },
                    Look::ClickBody { body, .. } => Look::ClickBody {
                        body,
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
                if let Some(write) = self.settings.chose(self.options.theme) {
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
            Message::AnimationFrame(now) => self.with_doc(|doc, _| {
                doc.animation_frame(now);
                doc.tick(now);
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
            Next::Show(doc) => {
                self.screen = Screen::Document(doc);
                Task::none()
            }
            Next::Left(Leave::Close) => {
                self.screen = Screen::Welcome(Welcome::default());
                // Saved since, with another.
                self.files.load_thumbnails();
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
            IoResponse::Created { id, result } => {
                let mine = self
                    .screen
                    .doc_mut()
                    .is_some_and(|doc| doc.created(&mut self.files, id, &result));
                if !mine {
                    self.files.settle_created(id, result);
                }
            }
            IoResponse::AutoSaved { file, result, .. } => {
                self.with_doc(|doc, _| doc.auto_saved(file, result))
            }
            IoResponse::RecoveryDiscarded { result, .. } => {
                report_failure("discard what was auto-saved", result);
            }
            IoResponse::RecoveredListed { designs } => self.files.recovered = designs,
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
                // Made once here: a handle is uploaded once per id.
                self.files.thumbnails = (thumbnails.into_iter())
                    .map(|(path, image)| {
                        let (width, height) = (image.width(), image.height());
                        let handle = iced::widget::image::Handle::from_rgba(
                            width,
                            height,
                            image.into_rgba(),
                        );
                        (path, handle)
                    })
                    .collect();
            }
            IoResponse::RecentWritten { result } => {
                report_failure("save the recent files", result);
            }
            IoResponse::SettingsLoaded { settings } => {
                let (theme, write) = self.settings.loaded(settings, self.options.theme);
                self.options.theme = theme;
                if let Some(write) = write {
                    self.files.io.send(write);
                }
            }
            IoResponse::SettingsWritten { result } => {
                report_failure("save the settings", result);
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
            } => return self.saved_as(to, revision, result),
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
            Screen::Document(doc) => doc.view(self.peeking, self.mode(), self.options),
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

    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let doc = self.screen.doc();
        let dialog = match &self.screen {
            Screen::Document(doc) => doc.dialog(),
            Screen::Welcome(welcome) => welcome.prompting().then_some(Dialog::Damaged),
        };
        Subscription::batch([
            keyboard::listen().filter_map(peek_key),
            keyboard::listen().filter_map(command_key),
            keyboard::listen().with(dialog).filter_map(escape_key),
            // The release is never seen if the window loses focus while
            // the peek key is held, e.g. to an Alt+Tab.
            window::events().filter_map(unfocused),
            match &self.screen {
                // No key opens anything behind the prompt.
                Screen::Welcome(welcome) => keyboard::listen()
                    .with(!welcome.prompting())
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
            // The browser asks before the page goes, while it would lose
            // changes.
            only_if(self.at_stake(), platform::guard),
            // While the camera turns, and edits wait on the solver until
            // they've waited long enough to say so; and while a thumbnail
            // is rendered, by the viewport's next frame, and on the web
            // read back on a later one.
            only_if(
                doc.is_some_and(|doc| doc.animating() || doc.timing() || doc.rendering_thumbnail()),
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
/// bodies, which would be sent to be written, nor a theme chosen, which
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
            | Message::PeekPanel(_)
            | Message::CommandHeld(_)
            | Message::SystemTheme(_)
            | Message::Ui(
                Ui::Look(_)
                    | Ui::ToggleMouseHints
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
    answer: iced::futures::channel::oneshot::Receiver<Option<varde_render::PreviewImage>>,
) -> Task<Message> {
    Task::perform(answer, move |image| {
        let image = image.ok().flatten().and_then(|image| {
            varde_io::thumbnail::Image::new(image.width, image.height, image.rgba)
        });
        Message::Doc(id, ForDoc::Thumbnail(tag, image))
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

/// Escape, given the prompt the user is being asked, if any: cancels it
/// (staying, or deleting nothing), and otherwise backs out of what's
/// open, see [`Look::Escape`].
fn escape_key((dialog, event): (Option<Dialog>, keyboard::Event)) -> Option<Message> {
    // With any modifiers, unlike a shortcut.
    let keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(key::Named::Escape),
        ..
    } = event
    else {
        return None;
    };
    Some(Message::Ui(match dialog {
        Some(Dialog::Unsaved) => Ui::File(File::Unsaved(Unsaved::Cancel)),
        Some(Dialog::Delete) => Ui::Look(Look::CancelDelete),
        Some(Dialog::Damaged) => Ui::Welcome(varde_view::Welcome::CancelDamaged),
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

pub fn run() -> Result {
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
