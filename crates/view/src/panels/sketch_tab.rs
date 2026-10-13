//! The Sketch tab: a sketch's geometry and its constraints and
//! dimensions.

use iced::widget::{
    MouseArea, Space, button, column, container, hover, mouse_area, responsive, row, space, stack,
    text, text_input,
};
use iced::{Alignment, Element, Length, Padding};
use std::collections::BTreeSet;
use varde_document::Document;

use varde_sketch::{ConstraintEntry, Curve, DimensionEntry, Id, LinkKind, Selectable, Sketch};

use super::rows::{
    EXPANDER_SIZE, Expander, SelectableRow, TREE_INDENT, construction_rail, empty_note, name,
    row_context, row_menu, tree_header, virtual_list,
};
use super::timeline::CHANGE_PLANE;
use super::{GeometryGroup, ROW_HEIGHT};
use crate::chrome::{self, icon_button};
use crate::escape::OnEscape;
use crate::icons::{self, Icon};
use crate::menu::{menu_item, menu_separator, ticked};
use crate::shortcut::Shortcut;
use crate::theme::{self, Tone};
use crate::{
    ConstraintKind, Edit, LinkRow, Look, Message, RowMenu, SketchState, VALUE_FIELD, ValueTarget,
    dimension, divider,
};

/// The sketch of `document` being edited: where it is, with a button
/// sending `change` to put it on another plane (disabled without), over
/// the Geometry list over the Constraints list, split by a divider that
/// can be dragged.
pub(super) fn sketch_tab<'a>(
    document: &Document,
    sketch: SketchState<'a>,
    change: Option<Message>,
) -> Element<'a, Message> {
    let plane = row![
        icons::icon(Icon::Plane, icons::INLINE),
        text(crate::plane_pick::on_plane(document, &sketch.plane))
            .size(12)
            .width(Length::Fill)
            .wrapping(text::Wrapping::None)
            .style(theme::muted_text),
        button(text(CHANGE_PLANE).size(12).wrapping(text::Wrapping::None))
            .padding([2, 10])
            .style(theme::Emphasis::Secondary.button_style())
            .on_press_maybe(change),
    ]
    .spacing(8)
    .height(ROW_HEIGHT)
    .padding([0, 16])
    .align_y(Alignment::Center);
    let lists = divider::vertical_split(
        sketch.split,
        move |height| geometry(sketch, height),
        move |height| constraints(sketch, height),
        |share| Message::Look(Look::SplitSketchTab(share)),
    );
    column![container(plane).padding(Padding::ZERO.top(6)), lists].into()
}

/// A row of the Sketch tab's Geometry list, see [`geometry_rows`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeometryRow {
    /// A group's header: which, and how many rows it has unfolded.
    Header(GeometryGroup, usize),
    /// A curve of the sketch's own, by its place in its curves.
    Curve(usize),
    /// A point of the sketch's own, by its place in its points: under the
    /// curve it makes if `child`, else one no curve has.
    Point { at: usize, child: bool },
    /// A link, by its place among `links`.
    Link(usize),
}

/// The rows of the Sketch tab's Geometry list as a tree, groups with a
/// header row each: Geometry, the sketch's own curves (what no link
/// made), each followed by its points while it's `expanded`, then the
/// points no curve has; then Sketch face, the outline of the face it's
/// on, then Projected and Intersected, its other links of each kind, a
/// link a row. A point a few curves share is under each. A group
/// `folded` lists its header alone. A group with nothing in it isn't
/// listed, but for Geometry while there's no link either, which says
/// nothing's drawn.
pub(crate) fn geometry_rows(
    sketch: &Sketch,
    links: &[LinkRow],
    folded: &BTreeSet<GeometryGroup>,
    expanded: &BTreeSet<Id>,
) -> Vec<GeometryRow> {
    let linked = sketch.linked();
    let own = |id: &Id| !linked.contains(id);
    let place = |id: Id| {
        sketch
            .points
            .binary_search_by_key(&id, |point| point.id)
            .ok()
    };
    let mut on_curves = BTreeSet::new();
    let mut tree = Vec::new();
    for (i, entry) in sketch.curves.iter().enumerate() {
        if !own(&entry.id) {
            continue;
        }
        on_curves.extend(entry.curve.points());
        tree.push(GeometryRow::Curve(i));
        if expanded.contains(&entry.id) {
            for at in entry.curve.points().filter_map(place) {
                tree.push(GeometryRow::Point { at, child: true });
            }
        }
    }
    let loose = (sketch.points.iter().enumerate())
        .filter(|(_, point)| own(&point.id) && !on_curves.contains(&point.id))
        .map(|(at, _)| GeometryRow::Point { at, child: false });
    tree.extend(loose);
    let mut rows = Vec::new();
    if !tree.is_empty() || links.is_empty() {
        rows.push(GeometryRow::Header(GeometryGroup::Own, tree.len()));
        if !folded.contains(&GeometryGroup::Own) {
            rows.extend(tree);
        }
    }
    let groups = [
        (GeometryGroup::SketchFace, LinkKind::Project, true),
        (GeometryGroup::Projected, LinkKind::Project, false),
        (GeometryGroup::Intersected, LinkKind::Intersect, false),
    ];
    for (group, kind, sketch_face) in groups {
        let of: Vec<GeometryRow> = (links.iter().enumerate())
            .filter(|(_, link)| link.kind == kind && link.sketch_face == sketch_face)
            .map(|(i, _)| GeometryRow::Link(i))
            .collect();
        if !of.is_empty() {
            rows.push(GeometryRow::Header(group, of.len()));
            if !folded.contains(&group) {
                rows.extend(of);
            }
        }
    }
    rows
}

/// What a group of the Geometry list is headed.
pub(super) fn group_label(group: GeometryGroup) -> &'static str {
    match group {
        GeometryGroup::Own => "Geometry",
        GeometryGroup::SketchFace => "Sketch face",
        GeometryGroup::Projected => LinkKind::Project.name(),
        GeometryGroup::Intersected => LinkKind::Intersect.name(),
    }
}

/// The icon of the curve `entry`.
pub(super) fn curve_icon(entry: &varde_sketch::CurveEntry) -> Icon {
    match (entry.corner, &entry.curve) {
        (Some(_), Curve::Arc { .. }) => Icon::Fillet,
        (Some(_), _) => Icon::Chamfer,
        (None, Curve::Line { .. }) => Icon::Line,
        (None, Curve::Circle { .. }) => Icon::Circle,
        (None, Curve::Arc { .. }) => Icon::Arc,
        (None, Curve::Spline(_)) => Icon::Spline,
    }
}

/// The sketch's geometry as a tree ([`geometry_rows`]): headers that fold
/// their groups on a click, curves that unfold to their points on their
/// chevron's, each noted with its size or where it is, as the Timeline
/// notes features. Only the rows in view of the list's `height` are laid
/// out, since a sketch can hold tens of thousands of items: the rest are
/// room.
pub(super) fn geometry(sketch: SketchState<'_>, height: f32) -> Element<'_, Message> {
    let rows = geometry_rows(sketch.sketch, sketch.links, sketch.folded, sketch.expanded);
    let conflicts = sketch.conflicting_items();
    let row_at = |i: usize| match rows[i] {
        GeometryRow::Header(group, count) => {
            let folded = sketch.folded.contains(&group);
            tree_header(
                group_label(group),
                count,
                folded,
                Message::Look(Look::ToggleGroup(group)),
            )
        }
        GeometryRow::Curve(at) => {
            let entry = &sketch.sketch.curves[at];
            let size = dimension::size_note(sketch.sketch, entry, sketch.units);
            // Driven: the constraints set its size, though no driving
            // dimension of its own gives it.
            let sized = sketch
                .analysis
                .is_some_and(|analysis| analysis.sized.contains(&entry.id));
            let (note, driven) = match size {
                Some((size, driving)) => (Some(size), sized && !driving),
                None => (None, false),
            };
            let danger = conflicts.contains(&entry.id);
            let open = sketch.expanded.contains(&entry.id);
            // Folded, its points' rows don't show their selection.
            let holds_selected = !open
                && (sketch.selection.iter()).any(|&target| match target {
                    Selectable::Item(id) => entry.curve.points().any(|point| point == id),
                    Selectable::HandleLine(_) => false,
                });
            let expander = Expander::Toggle {
                open,
                on_press: Message::Look(Look::ToggleExpanded(entry.id)),
            };
            let item = Item {
                id: entry.id.into(),
                icon: curve_icon(entry),
                name: entry.name(),
                note,
                at: None,
                driven,
                danger,
                holds_selected,
                construction: entry.construction,
            };
            geometry_item(sketch, item, expander, TREE_INDENT)
        }
        GeometryRow::Point { at, child } => {
            let point = &sketch.sketch.points[at];
            let item = Item {
                id: point.id.into(),
                icon: Icon::Point,
                name: geometry_point_name(sketch.sketch, point, child),
                note: None,
                at: Some(point.at),
                driven: false,
                danger: conflicts.contains(&point.id),
                holds_selected: false,
                construction: false,
            };
            let indent = if child {
                2.0 * TREE_INDENT
            } else {
                TREE_INDENT
            };
            geometry_item(sketch, item, Expander::Leaf, indent)
        }
        GeometryRow::Link(at) => link_row(sketch, &sketch.links[at]),
    };
    let list = if rows == [GeometryRow::Header(GeometryGroup::Own, 0)] {
        column![
            container(row_at(0)).padding(Padding::from([0, 8]).right(0)),
            empty_note("Nothing drawn yet.")
        ]
        .into()
    } else {
        virtual_list(rows.len(), sketch.scroll, height, row_at, |offset| {
            Message::Look(Look::ScrollGeometry(offset))
        })
    };
    container(list).padding(Padding::ZERO.top(6)).into()
}

/// A point or curve as a row of the Geometry list.
pub(super) struct Item {
    pub(super) id: Selectable,
    pub(super) icon: Icon,
    pub(super) name: String,
    pub(super) note: Option<String>,
    /// Where a point is, noted in place of `note` as fits the row.
    pub(super) at: Option<glam::DVec2>,
    /// Whether the size in the note is driven, set by the constraints
    /// though no driving dimension of its own gives it: it's shown in a
    /// green pill after an equals sign ([`theme::driven_pill`]), not faint.
    pub(super) driven: bool,
    pub(super) danger: bool,
    /// Whether a point of the curve's is selected while its rows are
    /// folded away: a round dot between the accent and a hovered
    /// selected row's tint follows the name.
    pub(super) holds_selected: bool,
    /// Whether it's a construction curve: its icon is in the construction
    /// colour, with a short dashed rail left of it ([`construction_rail`]).
    pub(super) construction: bool,
}

/// A driven size as a Geometry list row notes it: an equals sign before
/// it, in a green pill ([`theme::driven_pill`]).
pub(super) fn driven_note<'a>(note: String) -> Element<'a, Message> {
    let mark = text("=").size(11.5).style(theme::driven_mark);
    let note = text(note).size(11.5).wrapping(text::Wrapping::None);
    container(row![mark, note].spacing(3).align_y(Alignment::Center))
        .padding(Padding::ZERO.left(5).right(6))
        .style(|theme| theme::driven_pill(theme, false))
        .into()
}

/// About how wide a character of a row's note is, in pixels, at its size:
/// a digit's width, with some to spare.
pub(super) const NOTE_CHAR_WIDTH: f32 = 7.0;

/// The room between the parts of a Geometry list row, in pixels.
pub(super) const ITEM_SPACING: f32 = 6.0;

/// The side of the dot on a folded curve's row with a point selected.
pub(super) const SELECTED_POINT_SIZE: f32 = 7.0;

/// A point's name in the Geometry list: a handle's end listed under its
/// spline leaves the spline out ("End 1 of Handle 2"), as the row it's
/// under says which.
pub(super) fn geometry_point_name(
    sketch: &Sketch,
    point: &varde_sketch::Point,
    child: bool,
) -> String {
    let name = sketch.point_name(point);
    let spline = sketch.splines().find(|(_, spline)| {
        let mut arms = spline.handles.iter().flat_map(|handle| handle.arms());
        arms.any(|arm| arm == point.id)
    });
    match spline.and_then(|(id, _)| sketch.curve(id)) {
        Some(entry) if child => match name.strip_suffix(&format!(" of {}", entry.name())) {
            Some(role) => role.to_owned(),
            None => name,
        },
        _ => name,
    }
}

/// `item`'s row of the Geometry list, `indent` in with `expander` before
/// its icon, selected on a click (`Ctrl` adds it), highlighting it in the
/// viewport while hovered, with a context menu deleting it (the
/// selection, if it's among it), and detaching a point curves share.
pub(super) fn geometry_item<'a>(
    sketch: SketchState<'a>,
    item: Item,
    expander: Expander,
    indent: f32,
) -> Element<'a, Message> {
    let id = item.id;
    let selected = sketch.selection.contains(&id);
    let faint = sketch.pending.contains(&id.id());
    let content = move |hovered: bool, expander: Element<'a, Message>| {
        let name: Element<'a, Message> = if item.danger {
            text(item.name.clone())
                .style(theme::danger_text)
                .wrapping(text::Wrapping::None)
                .into()
        } else {
            name(item.name.clone(), !faint)
                .wrapping(text::Wrapping::None)
                .into()
        };
        let holds_selected = item.holds_selected.then(|| {
            container(Space::new())
                .width(SELECTED_POINT_SIZE)
                .height(SELECTED_POINT_SIZE)
                .style(|theme| container::Style {
                    background: Some(theme::selection_mark(theme::palette(theme)).into()),
                    border: iced::border::rounded(SELECTED_POINT_SIZE / 2.0),
                    ..container::Style::default()
                })
        });
        // A note too long for the room left is cut at the row's edge
        // rather than wrapped over the next; a point's coordinates are
        // rounded to fit first.
        let note_text = move |note: String| {
            let note: Element<'a, Message> = if item.driven {
                driven_note(note)
            } else {
                text(note)
                    .size(11.5)
                    .wrapping(text::Wrapping::None)
                    .style(theme::faint_text)
                    .into()
            };
            container(note)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
                .align_y(Alignment::Center)
                .clip(true)
        };
        let note: Option<Element<'a, Message>> = match (item.at, item.note.clone()) {
            (Some(at), _) => {
                let units = sketch.units;
                Some(
                    responsive(move |size| {
                        let room = (size.width / NOTE_CHAR_WIDTH).max(0.0) as usize;
                        note_text(dimension::point_note(at, units, room)).into()
                    })
                    .into(),
                )
            }
            (None, Some(note)) => Some(note_text(note).into()),
            (None, None) => None,
        };
        let icon: Element<'a, Message> = if item.construction {
            icons::tinted(item.icon, icons::INLINE, |p| p.sketching.construction).into()
        } else {
            icons::icon(item.icon, icons::INLINE)
        };
        let left = indent - EXPANDER_SIZE;
        let row = row![
            expander,
            icon,
            name,
            holds_selected,
            note.unwrap_or_else(|| space::horizontal().into()),
        ]
        .spacing(ITEM_SPACING)
        .height(ROW_HEIGHT)
        .align_y(Alignment::Center);
        let row: Element<'a, Message> = if item.construction {
            stack![row, construction_rail(EXPANDER_SIZE + ITEM_SPACING)].into()
        } else {
            row.into()
        };
        container(row)
            .padding(Padding::from([0, 8]).left(left))
            .style(theme::list_row(selected, hovered))
    };
    let row = mouse_area(hover(
        content(false, expander.clone().view()),
        content(true, expander.view()),
    ))
    .on_press(Message::Look(Look::ClickRow(id)))
    .on_enter(Message::Look(Look::HoverItem(Some(id))))
    .on_exit(Message::Look(Look::LeaveItem(id)));
    let menu = (sketch.item_menu == Some(id)).then(|| {
        let editable = sketch.editable && !id.id().is_builtin();
        let detachable = id.item().filter(|&point| sketch.sketch.detachable(point));
        let detach = detachable.map(|point| {
            let message = Message::Edit(Edit::DetachPoint(point));
            let item = menu_item(
                Icon::Split,
                "Detach".into(),
                None,
                sketch.editable.then_some(message),
            );
            [item.into(), menu_separator().into()]
        });
        let closing = crate::spline::closings(sketch.sketch, &BTreeSet::from([id]));
        let close = closing.map(|(close, _)| {
            let (icon, label) = if close {
                (Icon::CloseCurve, "Close")
            } else {
                (Icon::OpenCurve, "Open")
            };
            let message = Message::Edit(Edit::ToggleClosedItem(id));
            let item = menu_item(
                icon,
                label.into(),
                selected.then_some(Shortcut::CLOSE),
                sketch.editable.then_some(message),
            );
            [item.into(), menu_separator().into()]
        });
        row_menu(
            (close.into_iter().flatten())
                .chain(detach.into_iter().flatten())
                .chain([menu_item(
                    Icon::Trash,
                    "Delete".into(),
                    selected.then_some(Shortcut::DELETE),
                    editable.then_some(Message::Edit(Edit::DeleteItem(id))),
                )
                .into()])
                .collect(),
        )
    });
    row_context(row, RowMenu::Item(id), menu)
}

/// A link's row: its kind's icon and what it comes from, in the
/// construction look if its curves are out of profiles, in the danger
/// colour with why if it's broken. Clicking selects what it made, hovering lights what it comes
/// from in the model, and its context menu removes it or has its curves
/// count for profiles or not.
pub(super) fn link_row<'a>(sketch: SketchState<'a>, link: &'a LinkRow) -> Element<'a, Message> {
    let id = link.link;
    let made = sketch.sketch.link(id);
    // Selected when its curves are, as a click on one in the viewport
    // selects it without its points; a link of points alone, when they are.
    let selected = made.is_some_and(|made| {
        let items = if made.curves.is_empty() {
            &made.points
        } else {
            &made.curves
        };
        !items.is_empty() && (items.iter()).all(|&item| sketch.selection.contains(&item.into()))
    });
    let icon = match link.kind {
        LinkKind::Project => Icon::Project,
        LinkKind::Intersect => Icon::Intersect,
    };
    let note = link.broken.as_ref().map(|why| why.as_str().into());
    let row = SelectableRow {
        icon,
        name: link.source.as_str().into(),
        faint: sketch.pending.contains(&id),
        dim: false,
        danger: link.broken.is_some(),
        failed: false,
        note,
        indent: 24.0,
        selected,
        construction: !link.profiles,
    }
    .view(Message::Look(Look::ClickLink(id)))
    .on_enter(Message::Look(Look::HoverLink(Some(id))))
    .on_exit(Message::Look(Look::LeaveLink(id)));
    let menu = (sketch.link_menu == Some(id)).then(|| {
        let editable = sketch.editable;
        let profiles = if link.profiles {
            "Leave out of profiles"
        } else {
            "Use in profiles"
        };
        let toggle = Message::Edit(Edit::SetLinkProfiles(id, !link.profiles));
        let remove = Message::Edit(Edit::RemoveLink(id));
        // The sketch face can't be removed; it's construction, out of
        // profiles, or not.
        let items = if link.sketch_face {
            vec![
                menu_item(
                    ticked(!link.profiles),
                    "Construction".into(),
                    Some(Shortcut::CONSTRUCTION),
                    editable.then_some(toggle),
                )
                .into(),
            ]
        } else {
            vec![
                menu_item(icon, profiles.into(), None, editable.then_some(toggle)).into(),
                menu_separator().into(),
                menu_item(
                    Icon::Trash,
                    "Remove".into(),
                    None,
                    editable.then_some(remove),
                )
                .into(),
            ]
        };
        row_menu(items)
    });
    let on = RowMenu::Link(id);
    row_context(row, on, menu)
}

/// A row of the sketch's lists for the item `id`: its icon, its name and
/// a `note`, faint while it waits on the solver and in the danger colour
/// if it's in `danger`. Clicking selects it (`Ctrl` adds it), hovering
/// highlights it in the viewport.
pub(super) fn item_row<'a>(
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
        dim: false,
        danger,
        failed: false,
        note: note.map(Into::into),
        indent: 24.0,
        selected: sketch.selection.contains(&id.into()),
        construction: false,
    };
    row.view(Message::Look(Look::ClickRow(id.into())))
        .on_enter(Message::Look(Look::HoverItem(Some(id.into()))))
        .on_exit(Message::Look(Look::LeaveItem(id.into())))
}

/// The sketch's constraints and dimensions on the geometry selected, or
/// all of them when none is, those in conflict first, each with what
/// it ties together: "Tangent · Line 2, Arc 1", "Length 40 mm · Line 3".
/// A dimension's value is changed in place, its row double-clicked. The
/// header, "Constraints for selection" while it lists on the selection,
/// toggles the constraints' glyphs in the viewport.
pub(super) fn constraints(sketch: SketchState<'_>, height: f32) -> Element<'_, Message> {
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
    let title = if sketch.listed_on.is_empty() {
        "Constraints"
    } else {
        "Constraints for selection"
    };
    let header = container(
        row![
            chrome::heading(title),
            text(shown.len()).size(11.5).style(theme::faint_text),
            space::horizontal(),
            glyphs
        ]
        .spacing(8)
        .height(ROW_HEIGHT)
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
pub(super) enum Listed<'a> {
    Constraint(&'a ConstraintEntry),
    Dimension(&'a DimensionEntry),
}

impl Listed<'_> {
    pub(super) fn id(self) -> Id {
        match self {
            Listed::Constraint(entry) => entry.id,
            Listed::Dimension(entry) => entry.id,
        }
    }

    /// Whether it's on `id`.
    pub(super) fn on(self, id: Id) -> bool {
        match self {
            Listed::Constraint(entry) => entry.constraint.items().any(|(item, _)| item == id),
            Listed::Dimension(entry) => entry.dimension.measure.items().any(|(item, _)| item == id),
        }
    }
}

/// The constraints and dimensions the Constraints list shows: those on the
/// points and curves it lists on ([`SketchState::listed_on`]) and the
/// points of those curves, or selected themselves, or all when it lists
/// on none; those in conflict first, then by id.
pub(super) fn listed<'a>(sketch: &SketchState<'a>) -> Vec<Listed<'a>> {
    let selection = sketch.selection;
    // A curve lists what's on its points too.
    let points = (sketch.listed_on.iter())
        .filter_map(|&id| sketch.sketch.curve(id))
        .flat_map(|entry| entry.curve.points());
    let on: BTreeSet<Id> = sketch.listed_on.iter().copied().chain(points).collect();
    let conflicts = sketch.conflicts();
    let constraints = sketch.sketch.constraints.iter().map(Listed::Constraint);
    let dimensions = sketch.sketch.dimensions.iter().map(Listed::Dimension);
    let mut shown: Vec<_> = constraints
        .chain(dimensions)
        .filter(|listed| {
            on.is_empty()
                || selection.contains(&listed.id().into())
                || on.iter().any(|&id| listed.on(id))
        })
        .collect();
    // Stable, so by id within each.
    shown.sort_by_key(|listed| listed.id());
    shown.sort_by_key(|listed| !conflicts.contains(&listed.id()));
    shown
}

/// The names of the points and curves `items` of `sketch`: "Line 2, Arc 1".
pub(super) fn tied(sketch: &Sketch, items: impl Iterator<Item = Id>) -> String {
    let names: Vec<_> = items.filter_map(|id| sketch.name(id)).collect();
    names.join(", ")
}

/// The Constraints list's row of the dimension `entry`: "Length 40 mm ·
/// Line 3", a reference's value in brackets, in the danger colour if it's
/// in a conflict. Double-clicking a driving one opens the value field in
/// the row, with its expression as typed; while it's open, the row holds
/// it, and why the value typed was refused if it was.
pub(super) fn dimension_row<'a>(
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
                dimension_value_field("", field.text, field.params),
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

/// A dimension's value field, in the Constraints list or on its chip in
/// the viewport: `text` as typed, `placeholder` shown while there's
/// none, sending what's typed, `Enter` taking it and `Esc` closing it,
/// whatever has the focus.
pub(crate) fn dimension_value_field<'a>(
    placeholder: &str,
    text: &'a str,
    params: crate::ParamsIn<'a>,
) -> Element<'a, Message> {
    let on_input = |text| Message::Look(Look::ValueInput(text));
    let input = text_input(placeholder, text)
        .id(VALUE_FIELD)
        .on_input(on_input)
        .on_submit(Message::Edit(Edit::SubmitValue))
        .size(12)
        .padding([2, 4])
        .width(96);
    let input = crate::suggest::suggesting(input, text, params, Some(&on_input));
    OnEscape::new(input, Message::Look(Look::CancelValue)).into()
}
