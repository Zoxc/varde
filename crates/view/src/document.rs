//! The document screen: its layout, the banners over it, the prompt about
//! unsaved changes and the status bar's info.

use std::borrow::Cow;
use std::sync::Arc;

use iced::widget::{Space, button, column, container, opaque, row, space, stack, text};
use iced::{Alignment, Element, Length};
use varde_document::EXTENSION;
use varde_document::{APP_NAME, EditError, Editor};
use varde_kernel::RenderMesh;
use varde_render::Camera;

use crate::chrome::{self, key_hint, small_button};
use crate::shortcut::Held;
use crate::theme::Emphasis;
use crate::{Edit, File, Message, Panel, Unsaved, panels, theme, toolbar, viewport};

/// Borrowed state needed to build the document screen.
pub struct DocumentState<'a> {
    pub editor: &'a Editor,
    pub camera: &'a Camera,
    /// The document's mesh, which the app gets from the regeneration side,
    /// so it may lag behind the document.
    pub mesh: &'a Arc<RenderMesh>,
    /// How `mesh` stands against the document.
    pub mesh_status: MeshStatus<'a>,
    /// The document name, without extension.
    pub name: &'a str,
    /// Whether there are unsaved changes.
    pub edited: bool,
    /// Why the document can't be edited, if it can't. Edit commands are
    /// disabled then; the camera still works.
    pub read_only: Option<&'a str>,
    /// Why the last edit was refused, if it was.
    pub edit_error: Option<&'a EditError>,
    /// Whether a save is in flight.
    pub saving: bool,
    /// Why the last save or auto-save failed, if it did. Shown with Save
    /// As, which keeps what the user has whatever went wrong with the file.
    pub save_error: Option<Cow<'a, str>>,
    /// Unsaved changes a session that crashed left of the document, if
    /// there are any to offer to restore.
    pub recovered: Option<RecoveredChanges>,
    /// What's shown over the screen, if anything.
    pub overlay: Option<Overlay>,
    /// The selected side panel tab.
    pub panel: Panel,
    /// Whether the peek key is held, showing the other tab.
    pub peek: bool,
    pub mode: theme::Mode,
}

impl DocumentState<'_> {
    /// Whether the document can be changed, including by undo, and saved.
    pub(crate) fn editable(&self) -> bool {
        self.read_only.is_none()
    }
}

/// Unsaved changes a session that crashed left of a document.
pub struct RecoveredChanges {
    /// Whether the design changed since the changes were made to it, so
    /// restoring them may undo changes saved since.
    pub design_changed: bool,
}

/// How the mesh shown stands against the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshStatus<'a> {
    /// The mesh is of the document.
    Current,
    /// The document is newer, and its mesh is still being built.
    Regenerating,
    /// The document's mesh couldn't be built, and why. The mesh shown is an
    /// older one.
    Failed(&'a str),
}

/// A layer over the whole document screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    /// Asks what to do about unsaved changes.
    UnsavedPrompt,
    FileMenu,
}

/// The document screen: toolbar on top, side panel on the left and the 3D
/// viewport filling the rest.
pub fn document<'a>(state: DocumentState<'a>) -> Element<'a, Message> {
    let document = state.editor.document();
    let editable = state.editable();

    let read_only = state
        .read_only
        .map(|reason| banner(text("Read-only").font(theme::SEMIBOLD), reason, None));
    let save_error = state.save_error.as_deref().map(|error| {
        let save_as =
            small_button("Save As…", Emphasis::Primary).on_press(Message::File(File::SaveAs));
        let dismiss = small_button("Dismiss", Emphasis::Secondary)
            .on_press(Message::Edit(Edit::DismissSaveError));
        banner(
            text("Couldn't save")
                .font(theme::SEMIBOLD)
                .style(theme::danger_text),
            error,
            Some(row![save_as, dismiss].spacing(6).into()),
        )
    });

    let recovered = state.recovered.as_ref().map(|recovered| {
        let restore = small_button("Restore", Emphasis::Primary)
            .on_press(Message::File(File::RestoreChanges));
        let discard = small_button("Discard", Emphasis::Secondary)
            .on_press(Message::File(File::DiscardChanges));
        banner(
            text("Unsaved changes found").font(theme::SEMIBOLD),
            &format!(
                "{APP_NAME} closed unexpectedly while this design had changes that weren't \
                 saved{}",
                if recovered.design_changed {
                    ", but the design has changed since: restoring them may undo newer changes"
                } else {
                    ""
                }
            ),
            Some(row![restore, discard].spacing(6).into()),
        )
    });

    let content = column![
        toolbar::toolbar(&state),
        read_only,
        recovered,
        save_error,
        row![
            panels::side_panel(document, state.panel, state.peek, editable),
            viewport::viewport(state.mesh, state.camera, state.mode.palette()),
        ]
        .height(Length::Fill),
    ];
    let content = match state.overlay {
        Some(Overlay::UnsavedPrompt) => Element::from(stack![content, unsaved_prompt(state.name)]),
        Some(Overlay::FileMenu) => Element::from(stack![content, toolbar::file_menu(editable)]),
        None => content.into(),
    };

    chrome::window(
        content,
        status(&state),
        viewport::hints()
            .into_iter()
            .chain([key_hint(Held::PEEK, state.panel.other().label())]),
    )
}

/// A strip under the toolbar telling something about the whole document:
/// `title`, then `detail`, then any `actions` at the right.
fn banner<'a>(
    title: impl Into<Element<'a, Message>>,
    detail: &str,
    actions: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        container(
            row![
                title.into(),
                text(format!("— {detail}")).style(theme::muted_text),
                space::horizontal(),
                actions,
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([6, 12])
        .style(theme::banner),
        chrome::hrule(),
    ]
    .into()
}

/// Asks whether to save the changes to the document `name` before it's
/// closed, as a dialog over the whole screen, which dims the rest and
/// keeps it from being clicked.
fn unsaved_prompt(name: &str) -> Element<'_, Message> {
    let choice = |label, choice, emphasis: Emphasis| {
        button(text(label).font(theme::SEMIBOLD))
            .padding([6, 14])
            .style(emphasis.button_style())
            .on_press(Message::File(File::Unsaved(choice)))
    };
    let dialog = container(
        column![
            text(format!("Save the changes to {name}.{EXTENSION}?"))
                .size(14)
                .font(theme::SEMIBOLD),
            text("Your changes are lost if you don't save them.").style(theme::muted_text),
            Space::new().height(4),
            row![
                choice("Don't save", Unsaved::Discard, Emphasis::Secondary),
                space::horizontal(),
                choice("Cancel", Unsaved::Cancel, Emphasis::Secondary),
                choice("Save", Unsaved::Save, Emphasis::Primary),
            ]
            .spacing(8),
        ]
        .spacing(8)
        .width(380),
    )
    .padding(18)
    .style(theme::menu);

    opaque(
        container(opaque(dialog))
            .center(Length::Fill)
            .style(theme::scrim),
    )
}

/// The status bar's info on the document. Whether its mesh is still
/// being regenerated, or why it couldn't be built, if it couldn't. Then why the last edit was
/// refused, if it was, and whether a save is in flight.
fn status<'a>(state: &DocumentState<'_>) -> Element<'a, Message> {
    let bodies = state.editor.document().bodies().len();
    let triangles = state.mesh.triangle_count();
    let regenerating = match state.mesh_status {
        MeshStatus::Current => String::new(),
        MeshStatus::Regenerating => " · Regenerating…".to_owned(),
        MeshStatus::Failed(error) => format!(" · Couldn't regenerate: {error}"),
    };
    let edit_error = state
        .edit_error
        .map_or_else(String::new, |error| format!(" · Couldn't edit: {error}"));
    let saving = if state.saving { " · Saving…" } else { "" };
    text(format!(
        "{bodies} {} · {triangles} triangles{regenerating}{edit_error}{saving}",
        if bodies == 1 { "body" } else { "bodies" }
    ))
    .size(12)
    .style(theme::muted_text)
    .into()
}
