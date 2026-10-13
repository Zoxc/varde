//! The rows the side panel's lists are made of, shared by its tabs: a
//! selectable row, renaming one, its context menu, the tree's headers
//! and the virtualised list.

use iced::widget::{
    MouseArea, Space, column, container, hover, mouse_area, row, stack, text, text_input,
};
use iced::{Alignment, Element, Length, Padding};
use varde_document::Named;

use super::ROW_HEIGHT;
use crate::chrome;
use crate::context_menu::ContextMenu;
use crate::escape::OnEscape;
use crate::icons::{self, Icon};
use crate::menu::menu_item;
use crate::overflow_tip::OverflowTip;
use crate::theme;
use crate::{Edit, Look, Message, RENAME_FIELD, RowMenu};

/// A note in place of an empty list.
pub(super) fn empty_note<'a>(note: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    container(text(note).style(theme::muted_text))
        .padding([14, 12])
        .into()
}

/// The name typed for `target` if `renaming` is of it.
pub(super) fn renamed(renaming: Option<(Named, &str)>, target: Named) -> Option<&str> {
    renaming.and_then(|(on, text)| (on == target).then_some(text))
}

/// The row of what's being renamed, in its list: its icon and the rename
/// field holding `text` as typed, `Enter` renaming and `Esc` closing it,
/// whatever has the focus. `indent` is the room left of the icon.
pub(super) fn rename_row<'a>(icon: Icon, text: &'a str, indent: f32) -> Element<'a, Message> {
    let input = text_input("Name", text)
        .id(RENAME_FIELD)
        .on_input(|text| Message::Look(Look::RenameInput(text)))
        .on_submit(Message::Edit(Edit::CommitRename))
        .padding([2, 4])
        .width(Length::Fill);
    container(
        row![
            icons::icon(icon, icons::INLINE),
            OnEscape::new(input, Message::Look(Look::CancelRename)),
        ]
        .spacing(8)
        .height(ROW_HEIGHT)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([0, 8]).left(indent))
    .style(theme::selected_row)
    .into()
}

/// A row's context menu holding `items`.
pub(crate) fn row_menu<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    sized_row_menu(items, ROW_MENU_WIDTH)
}

/// The width of a row's context menu.
pub(super) const ROW_MENU_WIDTH: f32 = 180.0;

/// The width of a body's context menu, wider for its sliders.
pub(super) const BODY_MENU_WIDTH: f32 = 256.0;

/// `row` with the context menu `on` it: `menu` while it's open, which
/// right-clicking the row asks for.
pub(super) fn row_context<'a>(
    row: impl Into<Element<'a, Message>>,
    on: RowMenu,
    menu: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    ContextMenu::new(
        row,
        menu,
        Message::Look(Look::OpenMenu(on)),
        Message::Look(Look::CloseMenu),
    )
    .into()
}

/// A context menu's Hide or Show item for something `visible` or not,
/// sending `toggle` if `editable`.
pub(super) fn visibility_item<'a>(
    visible: bool,
    editable: bool,
    toggle: Message,
) -> Element<'a, Message> {
    let (eye, label) = if visible {
        (Icon::EyeOff, "Hide")
    } else {
        (Icon::Eye, "Show")
    };
    menu_item(eye, label.into(), None, editable.then_some(toggle)).into()
}

/// A row's context menu of `items`, `width` wide.
pub(super) fn sized_row_menu<'a>(
    items: Vec<Element<'a, Message>>,
    width: f32,
) -> Element<'a, Message> {
    container(column(items).width(width))
        .padding(4)
        .style(theme::menu)
        .into()
}

/// A row of a list that's selected by clicking it: an icon, a name, and a
/// note at the right, highlighted while selected or hovered.
pub(super) struct SelectableRow<'a> {
    pub(super) icon: Icon,
    pub(super) name: text::Fragment<'a>,
    /// Whether the name is faint, as a hidden feature's is, or an item
    /// waiting on the solver.
    pub(super) faint: bool,
    /// Whether the icon is faint too, as a feature's rolled back.
    pub(super) dim: bool,
    /// Whether the name is in the danger colour, as a constraint in
    /// conflict is.
    pub(super) danger: bool,
    /// Whether it failed, as a feature that failed to regenerate or a
    /// sketch that doesn't solve: its name in the strong danger colour
    /// with an alert after it, its note staying at the right.
    pub(super) failed: bool,
    pub(super) note: Option<text::Fragment<'a>>,
    /// Room left of the icon, in pixels.
    pub(super) indent: f32,
    pub(super) selected: bool,
    /// Whether it's construction, as a link out of profiles: its icon in
    /// the construction colour with a dashed rail left of it, as a
    /// construction curve's row in the Geometry list.
    pub(super) construction: bool,
}

impl<'a> SelectableRow<'a> {
    /// The row, sending `on_press` when it's clicked.
    pub(super) fn view(self, on_press: Message) -> MouseArea<'a, Message> {
        let name_line = || {
            row![
                if self.failed {
                    text(self.name.clone()).style(theme::failed_text)
                } else if self.danger {
                    text(self.name.clone()).style(theme::danger_text)
                } else {
                    name(self.name.clone(), !self.faint)
                }
                .wrapping(text::Wrapping::None),
                self.failed
                    .then(|| icons::tinted(Icon::Alert, 14.0, |p| p.danger_strong)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        };
        let content = |hovered: bool| {
            let row = container(
                row![
                    if self.dim {
                        Element::from(icons::tinted(self.icon, icons::INLINE, |p| p.faint))
                    } else if self.construction {
                        icons::tinted(self.icon, icons::INLINE, |p| p.sketching.construction).into()
                    } else {
                        icons::icon(self.icon, icons::INLINE)
                    },
                    // A name too long for the room left is cut at the
                    // row's edge (or its note's) rather than wrapped over
                    // the next, and told whole in a tooltip while hovered.
                    OverflowTip::new(
                        chrome::tip(
                            container(name_line()).width(Length::Fill).clip(true),
                            text(self.name.clone())
                        ),
                        name_line(),
                    ),
                    self.note.clone().map(|note| {
                        container(text(note).size(11.5).style(theme::faint_text))
                            .padding(Padding::ZERO.right(8))
                    }),
                ]
                .spacing(8)
                .height(ROW_HEIGHT)
                .align_y(Alignment::Center),
            )
            .padding(Padding::ZERO.left(self.indent));
            let row: Element<'a, Message> = if self.construction {
                stack![row, construction_rail(self.indent)].into()
            } else {
                row.into()
            };
            container(row).style(theme::list_row(self.selected, hovered))
        };
        mouse_area(hover(content(false), content(true))).on_press(on_press)
    }
}

/// A body's or feature's name, faint while it's hidden.
pub(super) fn name<'a>(name: impl text::IntoFragment<'a>, visible: bool) -> iced::widget::Text<'a> {
    text(name).style(if visible {
        text::default
    } else {
        theme::faint_text
    })
}

/// How far each level of a tree is indented, in pixels.
pub(super) const TREE_INDENT: f32 = 16.0;

/// What's left of a row's icon in a tree.
#[derive(Clone)]
pub(super) enum Expander {
    /// A chevron, pointing down if `open`, sending `on_press` on a click.
    Toggle { open: bool, on_press: Message },
    /// Room for one, on a row with nothing under it.
    Leaf,
}

impl Expander {
    pub(super) fn view<'a>(self) -> Element<'a, Message> {
        match self {
            Expander::Toggle { open, on_press } => {
                let chevron = icons::tinted(
                    if open { Icon::Chev } else { Icon::ChevRight },
                    EXPANDER_SIZE,
                    |p| p.muted,
                );
                mouse_area(chevron)
                    .on_press(on_press)
                    .interaction(iced::mouse::Interaction::Pointer)
                    .into()
            }
            Expander::Leaf => Space::new().width(EXPANDER_SIZE).into(),
        }
    }
}

/// The side of a tree's chevron, in pixels.
pub(super) const EXPANDER_SIZE: f32 = 14.0;

/// A tree's group header, `label` and how many it holds, behind a
/// chevron pointing right while it's `folded`: a click anywhere on it
/// sends `on_press`.
pub(super) fn tree_header<'a>(
    label: &'a str,
    count: usize,
    folded: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let header = || {
        container(
            row![
                icons::tinted(
                    if folded { Icon::ChevRight } else { Icon::Chev },
                    icons::INLINE,
                    |p| p.muted
                ),
                chrome::heading(label),
                text(count).size(11.5).style(theme::faint_text),
            ]
            .spacing(8)
            .height(ROW_HEIGHT)
            .width(Length::Fill)
            .align_y(Alignment::Center),
        )
    };
    // Highlighted while hovered, as the rows are.
    mouse_area(hover(header(), header().style(theme::hovered_row)))
        .on_press(on_press)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

/// The dashes of a construction curve's rail, their length and the gaps
/// between them, in pixels.
pub(super) const RAIL_DASHES: usize = 3;
pub(super) const RAIL_DASH: f32 = 3.0;
/// How far left of the icon a construction curve's rail is, in pixels.
pub(super) const RAIL_GAP: f32 = 4.0;
pub(super) const RAIL_WIDTH: f32 = 2.0;

/// The rail left of a construction curve's icon: three short dashes in
/// the construction colour, faded, centred on the icon. `left` is where
/// the icon starts in the row.
pub(super) fn construction_rail<'a>(left: f32) -> Element<'a, Message> {
    let dash = || {
        container(Space::new())
            .width(RAIL_WIDTH)
            .height(RAIL_DASH)
            .style(|theme| container::Style {
                background: Some(
                    theme::palette(theme)
                        .sketching
                        .construction
                        .scale_alpha(0.5)
                        .into(),
                ),
                ..container::Style::default()
            })
            .into()
    };
    let dashes =
        iced::widget::Column::with_children((0..RAIL_DASHES).map(|_| dash())).spacing(RAIL_DASH);
    container(dashes)
        .padding(Padding::ZERO.left((left - RAIL_GAP - RAIL_WIDTH).max(0.0)))
        .height(ROW_HEIGHT)
        .align_y(Alignment::Center)
        .into()
}

/// A list of `count` rows [`ROW_HEIGHT`] tall, `height` tall and scrolled
/// by `offset`, laying out only the rows in view, made by `row_at`, and
/// reporting its offset with `on_scroll`.
pub(super) fn virtual_list<'a>(
    count: usize,
    offset: f32,
    height: f32,
    row_at: impl Fn(usize) -> Element<'a, Message>,
    on_scroll: fn(f32) -> Message,
) -> Element<'a, Message> {
    let rows = Rows::in_view(count, offset, height);
    let list = column![
        Space::new().height(rows.before),
        column(rows.shown.map(row_at)),
        Space::new().height(rows.after),
    ];
    // The scroller beside the rows rather than over their ends.
    chrome::scrolled(container(list).padding(Padding::from([0, 8]).right(0)), 0.0)
        .spacing(2)
        .height(Length::Fill)
        .on_scroll(move |viewport| on_scroll(viewport.absolute_offset().y))
        .into()
}

/// The rows of a list of rows [`ROW_HEIGHT`] tall that are in view, and the
/// room taken by those before and after them.
pub(super) struct Rows {
    pub(super) before: f32,
    pub(super) shown: std::ops::Range<usize>,
    pub(super) after: f32,
}

impl Rows {
    /// The rows in view of a list of `count` rows scrolled by `offset`
    /// pixels, `height` tall. The offset is iced's, which a list shown
    /// anew only reports on its first frame: until then it may be stale,
    /// so it's kept within the list.
    pub(super) fn in_view(count: usize, offset: f32, height: f32) -> Self {
        let fit = if height.is_finite() && height > 0.0 {
            ((height / ROW_HEIGHT).ceil() as usize).saturating_add(1)
        } else {
            1
        };
        let first = if offset.is_finite() && offset > 0.0 {
            (offset / ROW_HEIGHT) as usize
        } else {
            0
        };
        let first = first.min(count.saturating_sub(fit));
        let last = first.saturating_add(fit).min(count);
        Self {
            before: first as f32 * ROW_HEIGHT,
            shown: first..last,
            after: (count - last) as f32 * ROW_HEIGHT,
        }
    }
}
