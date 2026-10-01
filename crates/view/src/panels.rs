//! The side panel: the Timeline and Objects tabs, and in a sketch the
//! Sketch tab in place of the Timeline.

use iced::widget::{
    MouseArea, Space, button, column, container, hover, mouse_area, row, space, stack, text,
    text_input,
};
use iced::{Alignment, Element, Font, Length, Padding};
use varde_document::{BodyId, Document, Extent, Feature, FeatureId, FeatureKind};
use varde_expr::LengthUnit;
use varde_sketch::{ConstraintEntry, Curve, DimensionEntry, Id, Sketch};

use crate::chrome::{self, ChipSize, Edge, edged, icon_button, key_chip};
use crate::escape::OnEscape;
use crate::icons::{self, Icon};
use crate::shortcut::{Held, Shortcut};
use crate::theme::{self, SEMIBOLD, SIDE_PANEL_WIDTH, TAB_HEIGHT, TabLook, Tone};
use crate::{
    ConstraintKind, DocumentState, Edit, Look, Message, Panel, SketchState, VALUE_FIELD,
    ValueTarget, dimension, split,
};

pub(crate) const ROW_HEIGHT: f32 = 28.0;

/// The docked panel left of the viewport. Shows the selected tab, or the
/// other one while peeking with the peek key held. Bodies and features can
/// only be hidden, shown and removed if the document is editable.
pub fn side_panel<'a>(state: &DocumentState<'a>) -> Element<'a, Message> {
    let selected = state.panel;
    let peek = state.peek;
    let sketching = state.sketch.is_some();
    let shown = if peek {
        selected.other(sketching)
    } else {
        selected
    };

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

    let features = if sketching {
        tab(Panel::Sketch, Icon::Sketch)
    } else {
        tab(Panel::Timeline, Icon::Rollback)
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
        row![features, tab(Panel::Objects, Icon::Body)]
            .spacing(2)
            .padding(Padding::from(8).bottom(0)),
    ];

    let document = state.editor.document();
    let editable = state.editable();
    let content = match (shown, state.sketch) {
        (Panel::Sketch, Some(sketch)) => sketch_tab(sketch),
        (Panel::Objects, _) => scrolled(objects(document, state.merged, editable)),
        _ => scrolled(timeline(
            document,
            state.selected_feature,
            state.unsolved,
            state.failed,
        )),
    };

    edged(
        container(column![strip, content])
            .height(Length::Fill)
            .style(theme::side_panel),
        Edge::Right,
        SIDE_PANEL_WIDTH,
    )
}

/// `list` scrolled within the rest of the panel.
fn scrolled<'a>(list: Element<'a, Message>) -> Element<'a, Message> {
    // The scroller floats in the right padding.
    chrome::scrolled(container(list).padding([6, 8]), 2.0)
        .height(Length::Fill)
        .into()
}

/// A note in place of an empty list.
fn empty_note<'a>(note: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    container(text(note).style(theme::muted_text))
        .padding([14, 12])
        .into()
}

/// The features in the order they were added, the `selected` one
/// highlighted, those `unsolved` or `failed` marked.
fn timeline<'a>(
    document: &'a Document,
    selected: Option<FeatureId>,
    unsolved: &[FeatureId],
    failed: &'a [(FeatureId, String)],
) -> Element<'a, Message> {
    if document.features().is_empty() {
        return empty_note(format!(
            "No features yet. Press {} to start a sketch.",
            Shortcut::SKETCH.label()
        ));
    }
    let units = document.units();
    column(document.features().iter().map(|feature| {
        let unsolved = unsolved.contains(&feature.id);
        let failed = failed
            .iter()
            .find(|(id, _)| *id == feature.id)
            .map(|(_, why)| why.as_str());
        feature_row(
            feature,
            units,
            selected == Some(feature.id),
            unsolved,
            failed,
        )
    }))
    .into()
}

/// The icon of `feature`, in the Timeline and wherever it's listed.
pub(crate) fn feature_icon(feature: &Feature) -> Icon {
    match feature.kind {
        FeatureKind::Sketch { .. } => Icon::Sketch,
        FeatureKind::Extrude(_) => Icon::Extrude,
    }
}

/// A feature in the Timeline, with its note: a sketch's plane, an
/// extrude's distances in `units`. Marked failed if it's `unsolved`, or
/// `failed` and why, which hovering it tells. Clicking selects it,
/// double-clicking edits it.
fn feature_row<'a>(
    feature: &'a Feature,
    units: LengthUnit,
    selected: bool,
    unsolved: bool,
    failed: Option<&'a str>,
) -> Element<'a, Message> {
    let note = match &feature.kind {
        FeatureKind::Sketch { .. } if unsolved => "Doesn't solve".into(),
        FeatureKind::Sketch { plane, .. } => plane.name().into(),
        FeatureKind::Extrude(extrude) => extent_note(&extrude.extent, units).into(),
    };
    let row = SelectableRow {
        icon: feature_icon(feature),
        name: feature.name.as_str().into(),
        faint: !feature.visible,
        danger: unsolved || failed.is_some(),
        note: Some(note),
        indent: 8.0,
        selected,
    };
    let row = row
        .view(Message::Look(Look::SelectFeature(feature.id)))
        .on_double_click(Message::Look(Look::EditFeature(feature.id)));
    match failed {
        Some(why) => crate::chrome::tip(
            row,
            text(crate::chrome::sentence(why)).style(theme::danger_text),
        ),
        None => row.into(),
    }
}

/// How far an extrude goes, for its Timeline row, in `units`: "10 mm",
/// "10 mm symmetric", "10 mm + 5 mm", "Through all".
pub(crate) fn extent_note(extent: &Extent, units: LengthUnit) -> String {
    let length = |value| length_note(value, units);
    match extent {
        Extent::OneSide(d) => length(d),
        Extent::Symmetric(d) => format!("{} symmetric", length(d)),
        Extent::TwoSides(a, b) => format!("{} + {}", length(a), length(b)),
        Extent::ThroughAll => "Through all".to_owned(),
    }
}

/// A distance of an extrude in `units`: "10 mm".
pub(crate) fn length_note(value: &varde_expr::Value, units: LengthUnit) -> String {
    varde_expr::format(value.value, Some(units.into()))
}

/// A row of a list that's selected by clicking it: an icon, a name, and a
/// note at the right, highlighted while selected or hovered.
struct SelectableRow<'a> {
    icon: Icon,
    name: text::Fragment<'a>,
    /// Whether the name is faint, as a hidden feature's is, or an item
    /// waiting on the solver.
    faint: bool,
    /// Whether the name is in the danger colour, as a sketch that doesn't
    /// solve or a constraint in conflict is.
    danger: bool,
    note: Option<text::Fragment<'a>>,
    /// Room left of the icon, in pixels.
    indent: f32,
    selected: bool,
}

impl<'a> SelectableRow<'a> {
    /// The row, sending `on_press` when it's clicked.
    fn view(self, on_press: Message) -> iced::widget::MouseArea<'a, Message> {
        let content = |hovered: bool| {
            container(
                row![
                    icons::icon(self.icon, icons::INLINE),
                    if self.danger {
                        text(self.name.clone()).style(theme::danger_text)
                    } else {
                        name(self.name.clone(), !self.faint)
                    },
                    space::horizontal(),
                    self.note
                        .clone()
                        .map(|note| text(note).size(11.5).style(theme::faint_text)),
                ]
                .spacing(8)
                .height(ROW_HEIGHT)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([0, 8]).left(self.indent))
            .style(theme::list_row(self.selected, hovered))
        };
        mouse_area(hover(content(false), content(true))).on_press(on_press)
    }
}

/// A body's or feature's name, faint while it's hidden.
fn name<'a>(name: impl text::IntoFragment<'a>, visible: bool) -> iced::widget::Text<'a> {
    text(name).style(if visible {
        text::default
    } else {
        theme::faint_text
    })
}

/// A group's heading in a list: its `label` and how many it holds.
fn group<'a>(label: &'a str, count: usize) -> Element<'a, Message> {
    row![
        icons::tinted(Icon::Chev, icons::INLINE, |p| p.muted),
        text(label)
            .size(11.5)
            .font(SEMIBOLD)
            .style(theme::muted_text),
        text(count).size(11.5).style(theme::faint_text),
    ]
    .spacing(8)
    .height(ROW_HEIGHT)
    .padding([0, 8])
    .align_y(Alignment::Center)
    .into()
}

/// The bodies, then the sketches. A body a join merged into another
/// (`merged`, see [`DocumentState::merged`]) is listed faint, with the body
/// holding it as its note: it's drawn as that one is, so it has no eye,
/// but it can still be removed.
fn objects<'a>(
    document: &'a Document,
    merged: &[(BodyId, BodyId)],
    editable: bool,
) -> Element<'a, Message> {
    let bodies = document.bodies().iter().map(|body| {
        let note = consumed_note(document, merged, body.id);
        object_row(Object {
            icon: Icon::Body,
            label: &body.name,
            visible: body.visible && note.is_none(),
            editable,
            toggle: note
                .is_none()
                .then_some(Message::Edit(Edit::ToggleVisible(body.id))),
            remove: Some(Message::Edit(Edit::RemoveBody(body.id))),
            note,
        })
    });
    let is_sketch = |feature: &&Feature| matches!(feature.kind, FeatureKind::Sketch { .. });
    let count = document.features().iter().filter(is_sketch).count();
    let sketches = document.features().iter().filter(is_sketch).map(|feature| {
        object_row(Object {
            icon: Icon::Sketch,
            label: &feature.name,
            visible: feature.visible,
            editable,
            toggle: Some(Message::Edit(Edit::ToggleFeatureVisible(feature.id))),
            remove: None,
            note: None,
        })
    });
    column(
        std::iter::once(group("Bodies", document.bodies().len()))
            .chain(bodies)
            .chain([group("Sketches", count)])
            .chain(sketches),
    )
    .into()
}

/// The note of `body` in the Objects list if a join merged it into
/// another (see [`DocumentState::merged`]): "in" the holder's name.
pub(crate) fn consumed_note(
    document: &Document,
    merged: &[(BodyId, BodyId)],
    body: BodyId,
) -> Option<String> {
    let (_, holder) = merged.iter().find(|(consumed, _)| *consumed == body)?;
    Some(format!("in {}", document.body(*holder)?.name))
}

/// An object in the Objects list.
struct Object<'a> {
    icon: Icon,
    label: &'a str,
    /// Whether it's drawn: shown faint if not.
    visible: bool,
    editable: bool,
    /// What its eye sends, if it has one.
    toggle: Option<Message>,
    /// What its remove button sends, if it has one.
    remove: Option<Message>,
    /// Shown faint at its end.
    note: Option<String>,
}

/// An object's row in the Objects list. The eye, sending `toggle`, and the
/// remove button, sending `remove` if there is one, show on hover; the eye
/// also shows while the object is hidden. Unless the document is
/// `editable`, only the eye of a hidden object shows, and does nothing.
/// Without a `toggle` there's no eye.
fn object_row(object: Object<'_>) -> Element<'_, Message> {
    let Object {
        icon,
        label,
        visible,
        editable,
        toggle,
        remove,
        note,
    } = object;
    let content = move |hovered: bool| {
        let eye = toggle
            .clone()
            .filter(|_| (hovered && editable) || !visible)
            .map(|toggle| {
                icon_button(
                    if visible { Icon::Eye } else { Icon::EyeOff },
                    Tone::Faint,
                    editable.then_some(toggle),
                )
            });
        let remove = remove
            .clone()
            .filter(|_| hovered && editable)
            .map(|message| icon_button(Icon::Trash, Tone::Faint, Some(message)));
        let note = (note.clone()).map(|note| text(note).size(11.5).style(theme::faint_text));
        container(
            row![
                icons::icon(icon, icons::INLINE),
                name(label, visible),
                space::horizontal(),
                note,
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

/// The sketch being edited: the Geometry list over the Constraints list,
/// split by a divider that can be dragged.
fn sketch_tab(sketch: SketchState<'_>) -> Element<'_, Message> {
    split::vertical_split(
        sketch.split,
        move |height| geometry(sketch, height),
        move |height| constraints(sketch, height),
        |share| Message::Look(Look::SplitSketchTab(share)),
    )
}

/// The sketch's curves, then its points, by name, each selected on a
/// click. Only the rows in view of the list's `height` are laid out, since
/// a sketch can hold tens of thousands of items: the rest are room.
fn geometry(sketch: SketchState<'_>, height: f32) -> Element<'_, Message> {
    let items = sketch.sketch.curves.len() + sketch.sketch.points.len();
    let conflicts = sketch.conflicting_items();
    let row_at = |i: usize| {
        let curves = &sketch.sketch.curves;
        match curves.get(i) {
            Some(entry) => {
                let icon = match (entry.corner, &entry.curve) {
                    (Some(_), Curve::Arc { .. }) => Icon::Fillet,
                    (Some(_), _) => Icon::Chamfer,
                    (None, Curve::Line { .. }) => Icon::Line,
                    (None, Curve::Circle { .. }) => Icon::Circle,
                    (None, Curve::Arc { .. }) => Icon::Arc,
                    (None, Curve::Spline(_)) => Icon::Spline,
                };
                let note = entry.construction.then_some("Construction");
                let danger = conflicts.contains(&entry.id);
                item_row(sketch, entry.id, icon, entry.name(), note, danger).into()
            }
            None => {
                let point = &sketch.sketch.points[i - curves.len()];
                let danger = conflicts.contains(&point.id);
                item_row(sketch, point.id, Icon::Point, point.name(), None, danger).into()
            }
        }
    };
    let header = container(group("Geometry", items)).padding([0, 8]);
    let list = if items == 0 {
        empty_note("Nothing drawn yet.")
    } else {
        virtual_list(
            items,
            sketch.scroll,
            height - ROW_HEIGHT,
            row_at,
            |offset| Message::Look(Look::ScrollGeometry(offset)),
        )
    };
    column![header, list].padding(Padding::ZERO.top(6)).into()
}

/// A list of `count` rows [`ROW_HEIGHT`] tall, `height` tall and scrolled
/// by `offset`, laying out only the rows in view, made by `row_at`, and
/// reporting its offset with `on_scroll`.
fn virtual_list<'a>(
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
struct Rows {
    before: f32,
    shown: std::ops::Range<usize>,
    after: f32,
}

impl Rows {
    /// The rows in view of a list of `count` rows scrolled by `offset`
    /// pixels, `height` tall. The offset is iced's, which a list shown
    /// anew only reports on its first frame: until then it may be stale,
    /// so it's kept within the list.
    fn in_view(count: usize, offset: f32, height: f32) -> Self {
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

/// A row of the sketch's lists for the item `id`: its icon, its name and
/// a `note`, faint while it waits on the solver and in the danger colour
/// if it's in `danger`. Clicking selects it (`Ctrl` adds it), hovering
/// highlights it in the viewport.
fn item_row<'a>(
    sketch: SketchState<'a>,
    id: Id,
    icon: Icon,
    name: String,
    note: Option<&'static str>,
    danger: bool,
) -> MouseArea<'a, Message> {
    let row = SelectableRow {
        icon,
        name: name.into(),
        faint: sketch.pending.contains(&id),
        danger,
        note: note.map(Into::into),
        indent: 24.0,
        selected: sketch.selection.contains(&id),
    };
    row.view(Message::Look(Look::ClickRow(id)))
        .on_enter(Message::Look(Look::HoverItem(Some(id))))
        .on_exit(Message::Look(Look::HoverItem(None)))
}

/// The sketch's constraints and dimensions on the geometry selected, or
/// all of them when none is, those in conflict first, each with what
/// it ties together: "Tangent · Line 2, Arc 1", "Length 40 mm · Line 3".
/// A dimension's value is changed in place, its row double-clicked. A
/// header toggles the constraints' glyphs in the viewport.
fn constraints(sketch: SketchState<'_>, height: f32) -> Element<'_, Message> {
    let shown = listed(&sketch);
    let conflicts = sketch.conflicts();
    let glyphs = icon_button(
        if sketch.glyphs {
            Icon::Eye
        } else {
            Icon::EyeOff
        },
        Tone::Faint,
        Some(Message::Look(Look::ToggleGlyphs)),
    );
    let header = container(
        row![
            group("Constraints", shown.len()),
            space::horizontal(),
            glyphs
        ]
        .align_y(Alignment::Center),
    )
    .padding([0, 8]);
    let list = if shown.is_empty() {
        let none = sketch.sketch.constraints.is_empty() && sketch.sketch.dimensions.is_empty();
        empty_note(if none {
            "No constraints yet."
        } else {
            "None on the selection."
        })
    } else {
        let count = shown.len();
        let row_at = move |i: usize| {
            let danger = |id| conflicts.contains(&id);
            match shown[i] {
                Listed::Constraint(entry) => {
                    let kind = ConstraintKind::of(&entry.constraint);
                    let tied = tied(sketch.sketch, entry.constraint.items().map(|(id, _)| id));
                    let name = format!("{} · {tied}", kind.label());
                    item_row(sketch, entry.id, kind.icon(), name, None, danger(entry.id)).into()
                }
                Listed::Dimension(entry) => dimension_row(sketch, entry, danger(entry.id)),
            }
        };
        virtual_list(
            count,
            sketch.constraint_scroll,
            height - ROW_HEIGHT,
            row_at,
            |offset| Message::Look(Look::ScrollConstraints(offset)),
        )
    };
    column![header, list].padding(Padding::ZERO.top(6)).into()
}

/// A row of the Constraints list.
#[derive(Debug, Clone, Copy)]
enum Listed<'a> {
    Constraint(&'a ConstraintEntry),
    Dimension(&'a DimensionEntry),
}

impl Listed<'_> {
    fn id(self) -> Id {
        match self {
            Listed::Constraint(entry) => entry.id,
            Listed::Dimension(entry) => entry.id,
        }
    }

    /// Whether it's on `id`.
    fn on(self, id: Id) -> bool {
        match self {
            Listed::Constraint(entry) => entry.constraint.items().any(|(item, _)| item == id),
            Listed::Dimension(entry) => entry.dimension.measure.items().any(|(item, _)| item == id),
        }
    }
}

/// The constraints and dimensions the Constraints list shows: those on the
/// points and curves it lists on ([`SketchState::listed_on`]), or
/// selected themselves, or all when it lists on none; those in conflict
/// first, then by id.
fn listed<'a>(sketch: &SketchState<'a>) -> Vec<Listed<'a>> {
    let (selection, on) = (sketch.selection, sketch.listed_on);
    let conflicts = sketch.conflicts();
    let constraints = sketch.sketch.constraints.iter().map(Listed::Constraint);
    let dimensions = sketch.sketch.dimensions.iter().map(Listed::Dimension);
    let mut shown: Vec<_> = constraints
        .chain(dimensions)
        .filter(|listed| {
            on.is_empty() || selection.contains(&listed.id()) || on.iter().any(|&id| listed.on(id))
        })
        .collect();
    // Stable, so by id within each.
    shown.sort_by_key(|listed| listed.id());
    shown.sort_by_key(|listed| !conflicts.contains(&listed.id()));
    shown
}

/// The names of the points and curves `items` of `sketch`: "Line 2, Arc 1".
fn tied(sketch: &Sketch, items: impl Iterator<Item = Id>) -> String {
    let names: Vec<_> = items.filter_map(|id| sketch.name(id)).collect();
    names.join(", ")
}

/// The Constraints list's row of the dimension `entry`: "Length 40 mm ·
/// Line 3", a reference's value in brackets, in the danger colour if it's
/// in a conflict. Double-clicking a driving one opens the value field in
/// the row, with its expression as typed; while it's open, the row holds
/// it, and why the value typed was refused if it was.
fn dimension_row<'a>(
    sketch: SketchState<'a>,
    entry: &'a DimensionEntry,
    danger: bool,
) -> Element<'a, Message> {
    let id = entry.id;
    let dimension = &entry.dimension;
    let what = dimension::name(&dimension.measure);
    let tied = tied(sketch.sketch, dimension.measure.items().map(|(id, _)| id));
    let editing = sketch
        .value
        .filter(|field| field.in_list && *field.target == ValueTarget::Dimension(id));
    if let Some(field) = editing {
        let after: Element<'_, Message> = match field.error {
            Some(error) => text(crate::chrome::sentence(&error.to_string()).into_owned())
                .style(theme::danger_text)
                .into(),
            None => text(tied).style(theme::faint_text).into(),
        };
        return container(
            row![
                icons::icon(Icon::Dimension, icons::INLINE),
                text(what),
                value_field("", field.text),
                after,
            ]
            .spacing(8)
            .height(ROW_HEIGHT)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([0, 8]).left(24.0))
        .style(theme::selected_row)
        .into();
    }
    let value = dimension::label(sketch.sketch, dimension, sketch.units);
    let name = format!("{what} {value} · {tied}");
    let note = (!dimension.driving).then_some("Reference");
    let row = item_row(sketch, id, Icon::Dimension, name, note, danger);
    if dimension.driving {
        row.on_double_click(Message::Look(Look::EditDimension { id, in_list: true }))
            .into()
    } else {
        row.into()
    }
}

/// The value field: `text` as typed, `placeholder` shown while there's
/// none, sending what's typed, `Enter` taking it and `Esc` closing it,
/// whatever has the focus.
pub(crate) fn value_field<'a>(placeholder: &str, text: &'a str) -> Element<'a, Message> {
    let input = text_input(placeholder, text)
        .id(VALUE_FIELD)
        .on_input(|text| Message::Look(Look::ValueInput(text)))
        .on_submit(Message::Edit(Edit::SubmitValue))
        .size(12)
        .padding([2, 4])
        .width(96);
    OnEscape::new(input, Message::Look(Look::CancelValue)).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_extrude_is_noted_by_its_distances_in_the_units() {
        let length = |value: f64| varde_expr::Value {
            text: String::new(),
            value,
        };
        let mm = LengthUnit::Mm;
        assert_eq!(extent_note(&Extent::OneSide(length(10.0)), mm), "10 mm");
        assert_eq!(
            extent_note(&Extent::OneSide(length(12.7)), LengthUnit::In),
            "0.5 in"
        );
        assert_eq!(
            extent_note(&Extent::Symmetric(length(4.0)), mm),
            "4 mm symmetric"
        );
        assert_eq!(
            extent_note(&Extent::TwoSides(length(10.0), length(2.5)), mm),
            "10 mm + 2.5 mm"
        );
        assert_eq!(extent_note(&Extent::ThroughAll, mm), "Through all");
    }

    #[test]
    fn a_merged_body_is_noted_in_its_holder() {
        use varde_document::Editor;
        let mut editor = Editor::new(Document::example());
        let FeatureKind::Extrude(extrude) = &editor.document().features()[1].kind else {
            panic!("the example's second feature is its extrude");
        };
        let command = editor.document().add_extrude(extrude.clone());
        editor.apply(command).unwrap();
        let [top, below] = [0, 1].map(|k| editor.document().bodies()[k].id);
        let merged = [(below, top)];
        let document = editor.document();
        assert_eq!(consumed_note(document, &merged, top), None);
        assert_eq!(
            consumed_note(document, &merged, below).as_deref(),
            Some("in Body 1")
        );
        assert_eq!(consumed_note(document, &[], below), None);

        let texts = |merged: &[(BodyId, BodyId)]| -> Vec<String> {
            let objects = objects(document, merged, true);
            let mut laid = crate::testing::Laid::new(objects, iced::Size::new(300.0, 400.0));
            laid.texts().into_iter().map(|shown| shown.text).collect()
        };
        let shown = texts(&merged);
        let at = |text: &str| shown.iter().position(|shown| shown == text);
        // The note is on Body 2's row, after its name.
        let note = at("in Body 1").unwrap_or_else(|| panic!("{shown:?}"));
        assert_eq!(at("Body 2"), Some(note - 1), "{shown:?}");
        assert!(!texts(&[]).iter().any(|text| text.starts_with("in ")));
    }

    #[test]
    fn only_the_rows_in_view_are_laid_out() {
        let rows = Rows::in_view(1000, 10.0 * ROW_HEIGHT + 5.0, 4.0 * ROW_HEIGHT);
        assert_eq!(rows.shown, 10..15);
        assert_eq!(rows.before, 10.0 * ROW_HEIGHT);
        assert_eq!(rows.after, 985.0 * ROW_HEIGHT);

        // All that fit, from the top.
        let rows = Rows::in_view(3, 0.0, 10.0 * ROW_HEIGHT);
        assert_eq!((rows.before, rows.shown, rows.after), (0.0, 0..3, 0.0));

        // A stale offset past the end shows the last rows.
        let rows = Rows::in_view(20, 1e9, 4.0 * ROW_HEIGHT);
        assert_eq!(rows.shown, 15..20);
        assert_eq!(rows.after, 0.0);
        let rows = Rows::in_view(3, 1e9, 10.0 * ROW_HEIGHT);
        assert_eq!(rows.shown, 0..3);

        // Nonsense offsets and heights show something from the top.
        let rows = Rows::in_view(20, f32::NAN, f32::NAN);
        assert_eq!(rows.shown, 0..1);
        assert_eq!(Rows::in_view(0, 50.0, 100.0).shown, 0..0);
    }
}
