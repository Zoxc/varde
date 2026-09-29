//! The side panel: the Timeline and Objects tabs.

use iced::widget::{button, column, container, hover, row, scrollable, space, stack, text};
use iced::{Alignment, Element, Font, Length, Padding};
use varde_document::{Body, Document};

use crate::chrome::{ChipSize, Edge, edged, icon_button, key_chip};
use crate::icons::{self, Icon};
use crate::shortcut::Held;
use crate::theme::{self, SEMIBOLD, SIDE_PANEL_WIDTH, TAB_HEIGHT, TabLook, Tone};
use crate::{Edit, Look, Message, Panel};

const ROW_HEIGHT: f32 = 28.0;

/// The docked panel left of the viewport. Shows the `selected` tab, or the
/// other one while `peek`ing with the peek key held. Bodies can only be hidden,
/// shown and removed if the document is `editable`.
pub fn side_panel(
    document: &Document,
    selected: Panel,
    peek: bool,
    editable: bool,
) -> Element<'_, Message> {
    let shown = if peek { selected.other() } else { selected };

    let tab = |panel: Panel, icon: Icon| {
        let look = if panel != shown {
            TabLook::Flat
        } else if peek {
            TabLook::Peek
        } else {
            TabLook::Raised
        };
        let alt = (panel != selected && !peek).then(|| key_chip(Held::PEEK, ChipSize::Small));
        button(
            row![
                icons::tinted(icon, icons::INLINE, move |p| look.content(p)),
                text(panel.label()).font(if look == TabLook::Flat {
                    Font::DEFAULT
                } else {
                    SEMIBOLD
                }),
                alt,
            ]
            .spacing(6)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .height(TAB_HEIGHT)
        .padding([0, 12])
        .style(theme::tab(look))
        .on_press(Message::Look(Look::SelectPanel(panel)))
    };

    // The tabs hang 1 px over the strip's bottom border, so the raised tab
    // joins the panel below.
    let strip = stack![
        edged(
            container(space::vertical())
                .width(Length::Fill)
                .style(theme::tab_strip),
            Edge::Bottom,
            8.0 + TAB_HEIGHT,
        ),
        row![
            tab(Panel::Timeline, Icon::Rollback),
            tab(Panel::Objects, Icon::Body)
        ]
        .spacing(2)
        .padding(Padding::from(8).bottom(0)),
    ];

    let list = match shown {
        Panel::Timeline => timeline(),
        Panel::Objects => objects(document, editable),
    };

    edged(
        container(column![
            strip,
            scrollable(container(list).padding([6, 8])).height(Length::Fill),
        ])
        .height(Length::Fill)
        .style(theme::side_panel),
        Edge::Right,
        SIDE_PANEL_WIDTH,
    )
}

fn timeline<'a>() -> Element<'a, Message> {
    container(text("No features yet.").style(theme::muted_text))
        .padding([14, 12])
        .into()
}

fn objects(document: &Document, editable: bool) -> Element<'_, Message> {
    let group = row![
        icons::tinted(Icon::Chev, icons::INLINE, |p| p.muted),
        text("Bodies")
            .size(11.5)
            .font(SEMIBOLD)
            .style(theme::muted_text),
        text(document.bodies().len())
            .size(11.5)
            .style(theme::faint_text),
    ]
    .spacing(8)
    .height(ROW_HEIGHT)
    .padding([0, 8])
    .align_y(Alignment::Center);

    column(
        std::iter::once(group.into()).chain(
            document
                .bodies()
                .iter()
                .map(|body| body_row(body, editable)),
        ),
    )
    .into()
}

/// A body in the Objects list. The eye and remove buttons show on hover; the
/// eye also shows while the body is hidden. Unless the document is
/// `editable`, only the eye of a hidden body shows, and does nothing.
fn body_row(body: &Body, editable: bool) -> Element<'_, Message> {
    let content = |hovered: bool| {
        let eye = ((hovered && editable) || !body.visible).then(|| {
            icon_button(
                if body.visible {
                    Icon::Eye
                } else {
                    Icon::EyeOff
                },
                Tone::Faint,
                editable.then_some(Message::Edit(Edit::ToggleVisible(body.id))),
            )
        });
        let remove = (hovered && editable).then(|| {
            icon_button(
                Icon::Trash,
                Tone::Faint,
                Some(Message::Edit(Edit::RemoveBody(body.id))),
            )
        });
        let name = text(&body.name).style(if body.visible {
            text::default
        } else {
            theme::faint_text
        });
        container(
            row![
                icons::icon(Icon::Body, icons::INLINE),
                name,
                space::horizontal(),
                eye,
                remove,
            ]
            .spacing(8)
            .height(ROW_HEIGHT)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([0, 2]).left(24))
    };

    hover(content(false), content(true).style(theme::hovered_row))
}
