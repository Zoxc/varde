//! The operation panel's form controls: labels, tiles, toggles, fields
//! picked into, typed fields, list rows and the Bodies list.

use std::collections::BTreeSet;

use iced::advanced::mouse;
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
    Text, button, checkbox, column, container, hover, mouse_area, row, text, text_input,
};
use iced::{Alignment, Element, Length, Padding};
use varde_document::{BodyId, FeatureId};

use super::model::{BodyTarget, OperationKind, PanelHover, TypedField, hovering, joined_into};
use crate::Message;
use crate::chrome::{hrule, sentence, tip};
use crate::escape::OnEscape;
use crate::icons::{self, Icon};
use crate::theme::{self, BOLD};

/// How tall the panel's controls are, as the Timeline's rows, and the
/// size of their words.
pub(crate) const CONTROL_HEIGHT: f32 = 28.0;
pub(crate) const CONTROL_TEXT: f32 = 12.5;

/// The rows of a profile's regions: those `picked` of the sketch
/// `source`, each with a cross sending `pick` of it, then the edited
/// feature's `missing` ones, each with a cross sending `drop_missing` of
/// its place among them. A click on a row sends `press`.
pub(crate) fn region_rows<'a>(
    source: Option<FeatureId>,
    picked: &BTreeSet<usize>,
    missing: usize,
    pick: impl Fn(FeatureId, usize) -> Option<Message>,
    drop_missing: impl Fn(usize) -> Option<Message>,
    press: Option<Message>,
    hovered: Option<PanelHover>,
) -> Vec<Element<'a, Message>> {
    let picked =
        (source.into_iter()).flat_map(|sketch| picked.iter().map(move |&region| (sketch, region)));
    picked
        .map(|(sketch, region)| {
            ListRow::new(
                Icon::SeRegion,
                region_name(region),
                PanelHover::Region { sketch, region },
            )
            .remove(pick(sketch, region))
            .press(press.clone())
            .view(hovered)
        })
        .chain(missing_rows(missing, drop_missing, press.clone(), hovered))
        .collect()
}

/// What a region picked is called in the panel: "Region 1", by its index
/// in its sketch's profiles.
pub(crate) fn region_name(region: usize) -> String {
    format!("Region {}", region.saturating_add(1))
}

/// A label of the panel's, a field's or a section's alike: 11 px, bold,
/// faint.
pub(crate) fn label(label: &str) -> Text<'_> {
    text(label).size(11).font(BOLD).style(theme::faint_text)
}

/// `content` under its `label`: a field, or a section.
pub(crate) fn field<'a>(
    label_text: &'a str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![label(label_text), content.into()].spacing(3).into()
}

/// Choices among a few, as tiles side by side, sharing the width.
pub(crate) fn tiles<'a>(
    tiles: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    row(tiles).spacing(4).into()
}

/// A choice of the panel's as a tile, `icon` over `label`, picked while
/// `on`, sending `message`, or disabled without one.
pub(crate) fn tile<'a>(
    icon: Icon,
    label: &'a str,
    on: bool,
    message: Option<Message>,
) -> Element<'a, Message> {
    let glyph: Element<'a, Message> = if message.is_some() {
        icons::icon(icon, 22.0)
    } else {
        icons::tinted(icon, 22.0, |p| p.faint).into()
    };
    button(
        column![
            glyph,
            text(label)
                .size(10.5)
                .line_height(LineHeight::Relative(1.15))
                .wrapping(Wrapping::WordOrGlyph)
                .align_x(Alignment::Center)
                .width(Length::Fill),
        ]
        .spacing(3)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding::from([6.0, 2.0]).bottom(4.0))
    .style(theme::tile(on))
    .on_press_maybe(message)
    .into()
}

/// An option of the panel's as an icon toggle: a square button with the
/// option's `icon` beside its `label`, lit while `on`, the row and the
/// button tinted on hover, sending `message`, or disabled without one.
/// Hovering it tells `note`, if there is one.
pub(crate) fn toggle<'a>(
    icon: Icon,
    label: &'a str,
    on: bool,
    message: Option<Message>,
    note: Option<&'a str>,
) -> Element<'a, Message> {
    let enabled = message.is_some();
    let square = move |hovered: bool| -> Element<'a, Message> {
        let glyph: Element<'a, Message> =
            Element::from(icons::tinted(icon, 18.0, move |p| {
                match (enabled, on, hovered) {
                    (false, ..) => p.faint,
                    (true, true, _) => p.accent,
                    (true, false, true) => p.text,
                    (true, false, false) => p.muted,
                }
            }));
        container(glyph)
            .center(CONTROL_HEIGHT)
            .style(theme::toggle_square(on, hovered))
            .into()
    };
    let padding = Padding::from([3.0, 4.0]);
    let base = button(
        row![square(false), text(label).size(CONTROL_TEXT)]
            .spacing(10)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(padding)
    .style(theme::toggle_row)
    .on_press_maybe(message);
    // Hovered, the square is drawn again over itself, lit: the label
    // isn't, so it shows once.
    let toggle: Element<'a, Message> = if enabled {
        hover(base, container(square(true)).padding(padding))
    } else {
        base.into()
    };
    match note {
        Some(note) => tip(toggle, text(note)),
        None => toggle,
    }
}

/// A [`pick_field`] of `rows`, with where to click, `place`, while it has
/// none or is the one picking (`on`).
pub(crate) fn pick_rows<'a>(
    rows: Vec<Element<'a, Message>>,
    place: &str,
    on: bool,
    message: Option<Message>,
) -> Element<'a, Message> {
    let place = (rows.is_empty() || on).then(|| place.to_owned());
    pick_field(rows, place, on, message)
}

/// A field picked into by clicks in the viewport: what's picked as
/// `rows` ([`ListRow`]), then where to click for more, `place`, if
/// there is one, under a rule (or alone, the field's one row). Outlined
/// in the accent while it's the one picking (`on`); a click on it makes
/// it so, sending `message`.
pub(crate) fn pick_field<'a>(
    rows: Vec<Element<'a, Message>>,
    place: Option<String>,
    on: bool,
    message: Option<Message>,
) -> Element<'a, Message> {
    let alone = rows.is_empty();
    let place = place.map(|place| {
        let line = button(
            row![
                // In the column of the rows' icons, so its words start
                // where their names do.
                container(icons::tinted(Icon::Plus, 13.0, move |p| if on {
                    p.accent
                } else {
                    p.muted
                }))
                .center_x(icons::INLINE),
                text(place)
                    .size(11.5)
                    .wrapping(Wrapping::WordOrGlyph)
                    .style(move |theme: &iced::Theme| text::Style {
                        color: on.then_some(theme::palette(theme).accent),
                    }),
            ]
            .spacing(8)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(if alone {
            CONTROL_HEIGHT - 2.0
        } else {
            CONTROL_HEIGHT - 4.0
        })
        // The rows' icons sit 2 + 6 in from the box's inside.
        .padding([0, 8])
        .style(theme::place_line(alone))
        .on_press_maybe(message.clone());
        if alone {
            Element::from(line)
        } else {
            column![hrule(), line].into()
        }
    });
    let rows = (!alone).then(|| container(column(rows)).padding(2));
    let field = container(column![rows, place])
        .width(Length::Fill)
        .padding(1)
        .style(theme::pick_box(on));
    match message {
        Some(message) => mouse_area(field)
            .on_press(message)
            .interaction(mouse::Interaction::Pointer)
            .into(),
        None => field.into(),
    }
}

/// A row of what's picked in a [`pick_field`], as the Timeline's: an
/// icon, a name, a `meta` at the right, and a cross taking it out sending
/// `remove`, if it can be. A click on the row sends `press`. It's `what`
/// to the viewport, which lights it up while the row is hovered. Built
/// by [`ListRow::new`] and its setters, shown by [`ListRow::view`].
pub(crate) struct ListRow<'a> {
    icon: Icon,
    name: text::Fragment<'a>,
    meta: Option<String>,
    moves: Option<[Option<Message>; 2]>,
    remove: Option<Message>,
    press: Option<Message>,
    what: PanelHover,
    tone: RowTone,
}

impl<'a> ListRow<'a> {
    /// A plain row named `name`, `what` to the viewport, with nothing
    /// beside it and sending nothing.
    pub(crate) fn new(icon: Icon, name: impl text::IntoFragment<'a>, what: PanelHover) -> Self {
        Self {
            icon,
            name: name.into_fragment(),
            meta: None,
            moves: None,
            remove: None,
            press: None,
            what,
            tone: RowTone::Plain,
        }
    }

    /// With `meta` at the right, faint.
    pub(crate) fn meta(self, meta: Option<String>) -> Self {
        Self { meta, ..self }
    }

    /// With a cross sending `remove`, if there's one.
    pub(crate) fn remove(self, remove: Option<Message>) -> Self {
        Self { remove, ..self }
    }

    /// Sending `press` when clicked.
    pub(crate) fn press(self, press: Option<Message>) -> Self {
        Self { press, ..self }
    }

    /// For a list whose order counts, a loft's sections: an up and a
    /// down chevron before its cross, each sending its message, or shown
    /// faint without one (the first row's up, the last's down).
    pub(crate) fn moves(self, moves: [Option<Message>; 2]) -> Self {
        Self {
            moves: Some(moves),
            ..self
        }
    }

    /// Its name in the colour `tone` says.
    pub(crate) fn tone(self, tone: RowTone) -> Self {
        Self { tone, ..self }
    }

    /// The row, shown hovered while `hovered` is its `what`.
    pub(crate) fn view(self, hovered: Option<PanelHover>) -> Element<'a, Message> {
        let Self {
            icon,
            name,
            meta,
            moves,
            remove,
            press,
            what,
            tone,
        } = self;
        let chevron = |glyph: Icon, message: Option<Message>| {
            let enabled = message.is_some();
            let tint = icons::tinted(
                glyph,
                14.0,
                move |p| if enabled { p.muted } else { p.faint },
            );
            button(container(tint).center(22))
                .padding(0)
                .style(theme::remove_button)
                .on_press_maybe(message)
        };
        let moves = moves.map(|[up, down]| {
            row![chevron(Icon::ChevUp, up), chevron(Icon::Chev, down)].spacing(0)
        });
        let cross = remove.map(|remove| {
            button(container(icons::tinted(Icon::Remove, 14.0, |p| p.faint)).center(22))
                .padding(0)
                .style(theme::remove_button)
                .on_press(remove)
        });
        let meta = meta.map(|meta| {
            text(meta)
                .size(11.5)
                .wrapping(Wrapping::None)
                .style(theme::faint_text)
        });
        let row = button(
            row![
                icons::icon(icon, icons::INLINE),
                container({
                    let name = text(name).size(CONTROL_TEXT).wrapping(Wrapping::None);
                    match tone {
                        RowTone::Plain => name,
                        RowTone::Failed => name.style(theme::failed_text),
                        RowTone::Excluded => name.style(theme::excluded_text),
                    }
                })
                .width(Length::Fill)
                .clip(true),
                meta,
                moves,
                cross,
            ]
            .spacing(8)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(CONTROL_HEIGHT)
        .padding(Padding::from([0.0, 3.0]).left(6.0))
        .style(theme::picked_row(hovered == Some(what)))
        .on_press_maybe(press);
        let (enter, exit) = hovering(what);
        mouse_area(row).on_enter(enter).on_exit(exit).into()
    }
}

/// The colour of a [`ListRow`]'s name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RowTone {
    #[default]
    Plain,
    /// The strong danger colour, as a failed feature's in the Timeline:
    /// what an edited feature names that wasn't found.
    Failed,
    /// The exclusions' purple: what a blend names to leave out (an edge
    /// around a face it names).
    Excluded,
}

/// The rows of an edited feature's `missing` regions that weren't found,
/// "Missing region" in the strong danger colour, each with a cross sending `remove` of its place
/// among them.
pub(crate) fn missing_rows<'a>(
    missing: usize,
    remove: impl Fn(usize) -> Option<Message>,
    press: Option<Message>,
    hovered: Option<PanelHover>,
) -> Vec<Element<'a, Message>> {
    (0..missing)
        .map(|index| {
            ListRow::new(Icon::SeRegion, "Missing region", PanelHover::Missing(index))
                .remove(remove(index))
                .press(press.clone())
                .tone(RowTone::Failed)
                .view(hovered)
        })
        .collect()
}

/// A typed value's field, named `label`, with the id `id`, showing why its
/// text is refused under it, and offering the design's parameters' names
/// as they're typed (see the `suggest` module). It sends `input` of the text typed, if the
/// document can be changed; `Enter` in it sends `submit` (OK), `Esc`
/// `cancel`, whatever has the focus: once, however many of a panel's
/// fields send it (`OnEscape`).
pub(crate) fn value_field<'a>(
    label_text: &'a str,
    id: iced::widget::Id,
    field_text: TypedField<'a>,
    input: Option<impl Fn(String) -> Message + Copy + 'a>,
    submit: Message,
    cancel: Message,
) -> Element<'a, Message> {
    let field_input = text_input(label_text, field_text.text)
        .id(id)
        .size(CONTROL_TEXT)
        // 28 px tall with its border, as the panel's other controls.
        .line_height(LineHeight::Absolute(16.0.into()))
        .padding([6, 8])
        .width(Length::Fill)
        .style(theme::field_input(field_text.error.is_some()));
    let field_input: Element<'a, Message> = match input {
        Some(on_input) => {
            let typed = field_input.on_input(on_input).on_submit(submit);
            let params = field_text.params;
            crate::suggest::suggesting(typed, field_text.text, params, Some(&on_input))
        }
        None => field_input.into(),
    };
    let field_input = OnEscape::new(field_input, cancel);
    let error = field_text.error.map(|error| {
        text(sentence(&error.to_string()).into_owned())
            .size(11.5)
            .wrapping(Wrapping::WordOrGlyph)
            .style(theme::danger_text)
    });
    column![label(label_text), field_input, error]
        .spacing(3)
        .into()
}

/// A boolean-producing feature's Operation tiles, a tile per kind, the
/// one `operation` lit and each sending `choose` of its kind, then its
/// [`bodies`] list, `toggle` and `hovered` as there.
pub(crate) fn operation_section<'a>(
    operation: OperationKind,
    targets: &[BodyTarget<'a>],
    choose: impl Fn(OperationKind) -> Option<Message>,
    toggle: impl Fn(BodyId) -> Option<Message>,
    hovered: Option<PanelHover>,
) -> Element<'a, Message> {
    let operations = OperationKind::ALL
        .map(|kind| tile(kind.icon(), kind.label(), operation == kind, choose(kind)));
    column![
        field("Operation", tiles(operations)),
        bodies(operation, targets, toggle, hovered),
    ]
    .spacing(10)
    .into()
}

/// The Bodies list of a join, cut or intersect: a row per body of
/// `targets` in a box, as a [`pick_field`]'s, a checkbox before the
/// body's icon and name (muted while taken out), a click anywhere on it
/// sending `toggle` of it if the document can be changed; a body an
/// earlier join merged into another with "in Body 1" at the right; and
/// for a join ticked for two or more, which body it merges them into.
/// Hovering a row lights its body in the viewport, and it shows hovered
/// while `hovered` is it. None for a new body, or with nothing to list.
pub(super) fn bodies<'a>(
    operation: OperationKind,
    targets: &[BodyTarget<'a>],
    toggle: impl Fn(BodyId) -> Option<Message>,
    hovered: Option<PanelHover>,
) -> Option<Element<'a, Message>> {
    if !operation.has_targets() || targets.is_empty() {
        return None;
    }
    let rows = targets.iter().map(|&target| {
        let message = toggle(target.body);
        let check = checkbox(target.included)
            .size(15)
            .style(theme::tick)
            .on_toggle_maybe(message.clone().map(|message| move |_| message.clone()));
        let included = target.included;
        let name = text(target.name)
            .size(CONTROL_TEXT)
            .wrapping(Wrapping::None)
            .style(move |theme: &iced::Theme| text::Style {
                color: (!included).then_some(theme::palette(theme).muted),
            });
        // Faint, as the Objects list notes a merged body.
        let holder = target.holder.map(|holder| {
            text(format!("in {holder}"))
                .size(11.5)
                .wrapping(Wrapping::None)
                .style(theme::faint_text)
        });
        let what = PanelHover::Body(target.body);
        let row = container(
            row![
                check,
                icons::icon(Icon::Body, icons::INLINE),
                container(name).width(Length::Fill).clip(true),
                holder,
            ]
            .spacing(8)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(CONTROL_HEIGHT)
        .padding(Padding::from([0.0, 8.0]).left(6.0))
        .style(theme::body_row(hovered == Some(what)));
        let (enter, exit) = hovering(what);
        let row = mouse_area(row).on_enter(enter).on_exit(exit);
        Element::from(match message {
            Some(message) => row
                .on_press(message)
                .interaction(mouse::Interaction::Pointer),
            None => row,
        })
    });
    let merging = joined_into(operation, targets).map(|holder| {
        // The mock's panel note: faint.
        text(format!("Joined into {holder}"))
            .size(12)
            .wrapping(Wrapping::WordOrGlyph)
            .style(theme::faint_text)
    });
    let list = container(column(rows))
        .width(Length::Fill)
        .padding(3)
        .style(theme::pick_box(false));
    Some(
        column![label("Bodies"), column![list, merging].spacing(6)]
            .spacing(6)
            .into(),
    )
}

/// The text of a message in the panel, in `style`, broken within words
/// where they don't fit, as a message may quote a name.
pub(crate) fn message_text<'a>(
    message: impl text::IntoFragment<'a>,
    style: fn(&iced::Theme) -> text::Style,
) -> Element<'a, Message> {
    text(message)
        .size(CONTROL_TEXT)
        .wrapping(Wrapping::WordOrGlyph)
        .style(style)
        .into()
}
