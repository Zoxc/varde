//! The document toolbar and its file menu.

use iced::widget::{Space, button, column, container, mouse_area, opaque, row, space, text};
use iced::{Alignment, Element, Length, Padding, mouse};
use varde_document::EXTENSION;

use crate::chrome::{Edge, edged, hrule, icon_button, key_label, vrule};
use crate::icons::{self, Icon};
use crate::shortcut::{Binding, Shortcut, document_bindings};
use crate::theme::{self, SEMIBOLD, SIDE_PANEL_INNER_WIDTH, Tone};
use crate::{DocumentState, Edit, File, Look, Message, Overlay};

/// Includes the 1 px border.
const TOOLBAR_HEIGHT: f32 = 40.0;

pub fn toolbar<'a>(state: &DocumentState<'a>) -> Element<'a, Message> {
    let editor = state.editor;

    let editable = state.editable();
    // The operations, which will follow the context once there are more.
    let ops = row![op(
        Icon::Box,
        "Box",
        editable.then_some(Message::Edit(Edit::AddCube))
    )]
    .spacing(2)
    .padding([0, 6])
    .align_y(Alignment::Center);

    let bar = row![
        file_cell(
            state.name,
            state.edited,
            state.overlay == Some(Overlay::FileMenu),
        ),
        vrule(),
        container(text("Model").font(SEMIBOLD)).padding([0, 12]),
        vrule(),
        ops,
        space::horizontal(),
        icon_button(
            Icon::Undo,
            Tone::Muted,
            (editable && editor.can_undo()).then_some(Message::Edit(Edit::Undo))
        ),
        icon_button(
            Icon::Redo,
            Tone::Muted,
            (editable && editor.can_redo()).then_some(Message::Edit(Edit::Redo))
        ),
        container(vrule()).height(18).padding([0, 4]),
        // TODO: open the command palette.
        icon_button(Icon::Search, Tone::Muted, None),
        crate::chrome::app_buttons(state.mode),
        Space::new().width(8),
    ]
    .spacing(2)
    .height(Length::Fill)
    .align_y(Alignment::Center);

    edged(
        container(bar).width(Length::Fill).style(theme::toolbar),
        Edge::Bottom,
        TOOLBAR_HEIGHT,
    )
}

/// The document name with a dirty dot and a chevron, opening the file menu.
fn file_cell<'a>(name: &'a str, edited: bool, open: bool) -> Element<'a, Message> {
    let dirty = edited.then(|| container(Space::new().width(6).height(6)).style(theme::dirty_dot));

    let content = row![
        row![
            text(name).font(SEMIBOLD),
            text(format!(".{EXTENSION}")).style(theme::faint_text)
        ],
        dirty,
        space::horizontal(),
        icons::tinted(Icon::Chev, icons::INLINE, |p| p.muted),
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Center);

    button(content)
        .width(SIDE_PANEL_INNER_WIDTH)
        .height(Length::Fill)
        .padding(Padding::from([0, 10]).left(12))
        .style(theme::file_cell(open))
        .on_press(Message::Edit(Edit::ToggleFileMenu))
        .into()
}

/// A toolbar operation's button, sending `message`; disabled without one.
fn op(icon: Icon, label: &'static str, message: Option<Message>) -> Element<'static, Message> {
    button(
        row![icons::icon(icon, icons::INLINE), text(label)]
            .spacing(6)
            .height(Length::Fill)
            .align_y(Alignment::Center),
    )
    .height(28)
    .padding([0, 8])
    .style(theme::flat_button(false))
    .on_press_maybe(message)
    .into()
}

/// The file menu, as a layer over the whole screen. Clicking outside the
/// menu closes it. Save is disabled unless the document is `editable`.
pub fn file_menu(editable: bool) -> Element<'static, Message> {
    let item = |icon, label, key: Option<Shortcut>, message: Option<Message>| {
        let enabled = message.is_some();
        let key = key.map(|key| container(key_label(key)).align_right(Length::Fill));
        button(
            row![
                // Text-toned, so hovering doesn't change it.
                icons::tinted(icon, icons::INLINE, move |p| {
                    theme::flat_content(p, Tone::Text, enabled, false)
                }),
                text(label),
                key,
            ]
            .spacing(10)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(28)
        .padding([0, 8])
        .style(theme::flat_button(false))
        .on_press_maybe(message)
    };
    let separator = || container(hrule()).padding([4, 2]);

    let bound = |icon, label, binding: Binding| {
        let message = binding.sends();
        item(icon, label, Some(binding.shortcut), message)
    };

    // Export and the rest join Save once they exist.
    let [save, save_as] = document_bindings(editable);
    let saving = column![
        bound(Icon::Save, "Save", save),
        bound(Icon::Save, "Save As…", save_as),
        separator(),
    ];
    let menu = container(
        column![
            saving,
            item(
                Icon::Close,
                "Close document",
                None,
                Some(Message::File(File::CloseDocument))
            ),
        ]
        .width(232),
    )
    .padding(4)
    .style(theme::menu);

    mouse_area(
        container(opaque(menu))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::ZERO.top(TOOLBAR_HEIGHT + 4.0).left(8)),
    )
    .interaction(mouse::Interaction::Idle)
    .on_press(Message::Look(Look::CloseFileMenu))
    .on_right_press(Message::Look(Look::CloseFileMenu))
    .into()
}
