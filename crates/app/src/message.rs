//! The app's messages: what the user asks for through the view, and what
//! the app's subscriptions, lanes and dialogs answer with.

use iced::time::Instant;
use iced::window;
use varde_io::lane::Lane as IoLane;
use varde_io::{Chosen, Response as IoResponse};
use varde_regen::Response as RegenResponse;
use varde_regen::lane::Lane as RegenLane;
use varde_solve::Response as SolveResponse;
use varde_solve::lane::Lane as SolveLane;

use crate::doc::DocId;

#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// What the user asked for through the view.
    Ui(varde_view::Message),
    /// An answer for document `.0`, dropped if it's no longer open: see
    /// [`ForDoc`].
    Doc(DocId, ForDoc),

    // For the welcome screen.
    /// The Open dialog closed, with the chosen file if there is one.
    Picked(Option<Chosen>),

    // For the open document, if there is one.
    /// A tick of the auto-save timer.
    AutoSaveTick(Instant),
    /// A frame while the camera is animating.
    AnimationFrame(Instant),

    // For the app.
    /// The peek key pressed or released, see `Held::PEEK`: shows the other
    /// panel tab while held.
    PeekPanel(bool),
    /// The command modifier (`Ctrl`, or `Cmd` on macOS) pressed or
    /// released: clicking a list's row adds to the selection while held.
    CommandHeld(bool),
    /// The mode the system prefers, as it starts and whenever it changes.
    SystemTheme(iced::theme::Mode),
    /// The user asked to close the window: the app decides when it does.
    CloseRequested(window::Id),
    /// On the web, the page may be going away (the tab closing or
    /// reloading), or is hidden, which it may never come back from.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    PageLeaving,
    /// The IO lane started; file requests go to it from now on.
    IoReady(IoLane),
    /// The IO lane answered a request.
    Io(IoResponse),
}

/// An answer for one document, tagged with its [`DocId`] in
/// [`Message::Doc`], since the document may be gone by the time it comes.
#[derive(Debug, Clone)]
#[expect(
    clippy::large_enum_variant,
    reason = "an answer is made once per response and handled at once"
)]
pub(crate) enum ForDoc {
    /// The Save As dialog closed, with the chosen file if there is one.
    SaveAsPicked(Option<Chosen>),
    /// The Export dialog closed, with the chosen file if there is one.
    ExportPicked(Option<Chosen>),
    /// On the web: whether the browser lets the document be saved to its
    /// file, asked as the user saves.
    Writable(Result<(), String>),
    /// The regeneration lane started; requests go to it from now on.
    RegenReady(RegenLane),
    /// Work handed to the regeneration lane is done.
    Computed(RegenResponse),
    /// The solver lane started; sketch edits are proposed through it.
    SolveReady(SolveLane),
    /// The solver lane answered a request.
    Solved(SolveResponse),
    /// The thumbnail tagged `.0` was rendered, or `None` if it couldn't
    /// be: see `doc/thumbnail.rs`.
    Thumbnail(u64, Option<varde_io::thumbnail::Image>),
}
