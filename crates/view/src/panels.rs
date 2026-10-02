//! The side panel: the Timeline and Objects tabs, and in a sketch the
//! Sketch tab in place of the Timeline.

use iced::widget::{
    MouseArea, Space, button, column, container, hover, mouse_area, row, slider, space, stack,
    text, text_input,
};
use iced::{Alignment, Element, Font, Length, Padding};
use varde_document::{BodyId, Document, Extent, Feature, FeatureId, FeatureKind, Opacity};
use varde_expr::LengthUnit;
use varde_sketch::{ConstraintEntry, Curve, DimensionEntry, Id, Sketch};

use crate::chrome::{self, ChipSize, Edge, edged, icon_button, key_chip};
use crate::context_menu::ContextMenu;
use crate::document::shown_opacity;
use crate::escape::OnEscape;
use crate::icons::{self, Icon};
use crate::mouse_only::MouseOnly;
use crate::shortcut::{Held, Shortcut};
use crate::theme::{self, SEMIBOLD, SIDE_PANEL_WIDTH, TAB_HEIGHT, TabLook, Tone};
use crate::toolbar::{menu_item, menu_separator};
use crate::{
    ConstraintKind, DocumentState, Edit, Look, Message, Panel, RowMenu, SketchState, VALUE_FIELD,
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
        (Panel::Sketch, Some(sketch)) => {
            // The sketch edited stays selected in the Timeline.
            let change = (state.selected_feature)
                .filter(|_| editable)
                .map(|id| Message::Look(Look::ChangePlane(id)));
            sketch_tab(document, sketch, change)
        }
        (Panel::Objects, _) => scrolled(objects(
            document,
            state.merged,
            editable,
            state.row_menu,
            state.model_selection,
            state.opacity_preview,
        )),
        _ => scrolled(timeline(
            document,
            state.selected_feature,
            state.row_menu,
            editable,
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
/// highlighted, the one `menu` is on with its context menu open, which
/// right-clicking a feature asks for, those `unsolved` or
/// `failed` marked. Features can only be deleted if the document is
/// `editable`.
fn timeline<'a>(
    document: &'a Document,
    selected: Option<FeatureId>,
    menu: Option<RowMenu>,
    editable: bool,
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
        let selected = selected == Some(feature.id);
        let row = feature_row(document, feature, units, selected, unsolved, failed);
        let on = RowMenu::Feature(feature.id);
        let menu = (selected && menu == Some(on)).then(|| feature_menu(feature, editable));
        ContextMenu::new(
            row,
            menu,
            Message::Look(Look::OpenMenu(on)),
            Message::Look(Look::CloseMenu),
        )
        .into()
    }))
    .into()
}

/// The icon of `feature`, in the Timeline and wherever it's listed.
pub(crate) fn feature_icon(feature: &Feature) -> Icon {
    match feature.kind {
        FeatureKind::Sketch { .. } => Icon::Sketch,
        FeatureKind::Extrude(_) => Icon::Extrude,
        FeatureKind::Revolve(_) => Icon::Revolve,
        // Until the combine's own icon comes with its tool.
        FeatureKind::Combine(_) => Icon::Body,
    }
}

/// A feature of `document` in the Timeline, with its note: a sketch's
/// plane ([`plane_note`](crate::plane_note)), an extrude's distances in
/// `units`, how far a revolve turns in all. Marked failed if it's
/// `unsolved`, or `failed` and why, which hovering it tells. Clicking
/// selects it, double-clicking edits it.
fn feature_row<'a>(
    document: &Document,
    feature: &'a Feature,
    units: LengthUnit,
    selected: bool,
    unsolved: bool,
    failed: Option<&'a str>,
) -> Element<'a, Message> {
    let note = match &feature.kind {
        FeatureKind::Sketch { .. } if unsolved => "Doesn't solve".into(),
        FeatureKind::Sketch { plane, .. } => crate::plane_note(document, plane).into(),
        FeatureKind::Extrude(extrude) => extent_note(&extrude.extent, units).into(),
        FeatureKind::Revolve(revolve) => turn_note(&revolve.extent).into(),
        FeatureKind::Combine(combine) => combine.op.label().into(),
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

/// The context menu of `feature` in the Timeline: edit it, put a sketch
/// on another plane, or delete it, those two if the document is
/// `editable`, by the keys that do the same to the feature selected.
fn feature_menu<'a>(feature: &Feature, editable: bool) -> Element<'a, Message> {
    let id = feature.id;
    let edit = menu_item(
        feature_icon(feature),
        edit_label(feature).into(),
        Some(Shortcut::ENTER),
        Some(Message::Look(Look::EditFeature(id))),
    );
    let change_plane = matches!(feature.kind, FeatureKind::Sketch { .. }).then(|| {
        menu_item(
            Icon::Plane,
            CHANGE_PLANE.into(),
            None,
            editable.then_some(Message::Look(Look::ChangePlane(id))),
        )
        .into()
    });
    row_menu(
        vec![
            Some(edit.into()),
            change_plane,
            Some(menu_separator().into()),
            Some(
                menu_item(
                    Icon::Trash,
                    "Delete".into(),
                    Some(Shortcut::DELETE),
                    editable.then_some(Message::Edit(Edit::RemoveFeature(id))),
                )
                .into(),
            ),
        ]
        .into_iter()
        .flatten()
        .collect(),
    )
}

/// What putting a sketch on another plane is called.
pub(crate) const CHANGE_PLANE: &str = "Change plane";

/// What editing `feature` is called in its context menu.
fn edit_label(feature: &Feature) -> &'static str {
    match feature.kind {
        FeatureKind::Sketch { .. } => "Edit sketch",
        FeatureKind::Extrude(_) => "Edit extrude",
        FeatureKind::Revolve(_) => "Edit revolve",
        FeatureKind::Combine(_) => "Edit combine",
    }
}

/// A row's context menu holding `items`.
fn row_menu<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(column(items).width(180))
        .padding(4)
        .style(theme::menu)
        .into()
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

/// How far a revolve turns in all, for its Timeline row: "360°", "90°",
/// "135°" for two sides of 90° and 45°.
pub(crate) fn turn_note(turn: &varde_document::Turn) -> String {
    use varde_document::Turn;
    let total = match turn {
        Turn::Full => std::f64::consts::TAU,
        Turn::OneSide(a) | Turn::Symmetric(a) => a.value,
        // Each side is checked to be at most a turn, so the sum is
        // finite.
        Turn::TwoSides(a, b) => a.value + b.value,
    };
    angle_note(total)
}

/// How far a revolve turns, for the status bar: "Full 360°", "One side
/// 90°", "Symmetric 90°", "Two sides 90° + 45°".
pub(crate) fn turn_info(turn: &varde_document::Turn) -> String {
    use varde_document::Turn;
    match turn {
        Turn::Full => format!("Full {}", angle_note(std::f64::consts::TAU)),
        Turn::OneSide(a) => format!("One side {}", angle_note(a.value)),
        Turn::Symmetric(a) => format!("Symmetric {}", angle_note(a.value)),
        Turn::TwoSides(a, b) => {
            format!(
                "Two sides {} + {}",
                angle_note(a.value),
                angle_note(b.value)
            )
        }
    }
}

/// An angle of `radians`, in degrees: "90°".
fn angle_note(radians: f64) -> String {
    varde_expr::format(radians, Some(varde_expr::AngleUnit::Deg.into()))
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
        chrome::heading(label),
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
/// holding it as its note: it's drawn as that one is, so it has no eye
/// nor opacity of its own, but it can still be removed. Right-clicking a
/// row asks for its context menu, shown on the one `menu` is on, with a
/// body's opacity as `preview` has it while its slider is dragged.
/// Clicking a body's row selects it where bodies are selected, as
/// `selection`, which marks the rows of the bodies it holds, says.
fn objects<'a>(
    document: &'a Document,
    merged: &[(BodyId, BodyId)],
    editable: bool,
    menu: Option<RowMenu>,
    selection: &crate::Selection,
    preview: Option<(BodyId, Opacity)>,
) -> Element<'a, Message> {
    let selected: Vec<BodyId> = selection.bodies().collect();
    let takes_bodies = selection.mode().takes_bodies();
    let bodies = document.bodies().iter().map(|body| {
        let note = consumed_note(document, merged, body.id);
        let own = note.is_none();
        object_row(Object {
            icon: Icon::Body,
            label: &body.name,
            visible: body.visible && own,
            editable,
            toggle: own.then_some(Message::Edit(Edit::ToggleVisible(body.id))),
            remove: Some(Message::Edit(Edit::RemoveBody(body.id))),
            note,
            selected: selected.contains(&body.id),
            on_press: takes_bodies.then_some(Message::Look(Look::ClickBody {
                body: body.id,
                add: false,
            })),
            menu: ObjectMenu {
                on: RowMenu::Body(body.id),
                open: menu == Some(RowMenu::Body(body.id)),
                edit: None,
                opacity: own.then(|| (body.id, shown_opacity(body, preview))),
                delete: Message::Edit(Edit::RemoveBody(body.id)),
            },
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
            selected: false,
            on_press: None,
            menu: ObjectMenu {
                on: RowMenu::Sketch(feature.id),
                open: menu == Some(RowMenu::Sketch(feature.id)),
                edit: Some((
                    edit_label(feature),
                    Message::Look(Look::EditFeature(feature.id)),
                )),
                opacity: None,
                delete: Message::Edit(Edit::RemoveFeature(feature.id)),
            },
        })
    });
    column(
        std::iter::once(group("Bodies", bodies_after_joins(document, merged)))
            .chain(bodies)
            .chain([group("Sketches", count)])
            .chain(sketches),
    )
    .into()
}

/// How many bodies `document` has once its joins have merged some
/// (`merged`, see [`DocumentState::merged`]) into others: a merged body is
/// one with the body holding it.
pub(crate) fn bodies_after_joins(document: &Document, merged: &[(BodyId, BodyId)]) -> usize {
    (document.bodies().iter())
        .filter(|body| !merged.iter().any(|(consumed, _)| *consumed == body.id))
        .count()
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
    /// Whether it's marked selected.
    selected: bool,
    /// What clicking the row sends, if anything.
    on_press: Option<Message>,
    menu: ObjectMenu,
}

/// The context menu of an object in the Objects list.
struct ObjectMenu {
    /// The row it's on.
    on: RowMenu,
    open: bool,
    /// What editing the object is called and sends, if it can be edited.
    edit: Option<(&'static str, Message)>,
    /// The body it is and how opaque it's shown, if it has an opacity.
    opacity: Option<(BodyId, Opacity)>,
    /// What deleting it sends.
    delete: Message,
}

impl ObjectMenu {
    /// The menu, for an object `visible` or not, whose eye sends
    /// `toggle` if it has one: editing it, showing or hiding it, its
    /// opacity if it has one and deleting it, the last three only if the
    /// document is `editable`. No keys are given, as the keys act on the
    /// Timeline's selection.
    fn view<'a>(
        self,
        icon: Icon,
        visible: bool,
        toggle: Option<Message>,
        editable: bool,
    ) -> Element<'a, Message> {
        let edit = (self.edit)
            .map(|(label, message)| menu_item(icon, label.into(), None, Some(message)).into());
        let toggle = toggle.map(|toggle| {
            let (eye, label) = if visible {
                (Icon::EyeOff, "Hide")
            } else {
                (Icon::Eye, "Show")
            };
            menu_item(eye, label.into(), None, editable.then_some(toggle)).into()
        });
        let opacity = (self.opacity.into_iter()).flat_map(|(body, opacity)| {
            [
                menu_separator().into(),
                opacity_rows(body, opacity, editable),
            ]
        });
        let delete = menu_item(
            Icon::Trash,
            "Delete".into(),
            None,
            editable.then_some(self.delete),
        );
        row_menu(
            edit.into_iter()
                .chain(toggle)
                .chain(opacity)
                .chain([menu_separator().into(), delete.into()])
                .collect(),
        )
    }
}

/// How far the Opacity slider moves at a time, in percent.
const OPACITY_STEP: f32 = 5.0;

/// The Opacity rows of `body`'s context menu: a heading over a slider from
/// [`Opacity::MIN`] to [`Opacity::MAX`] showing `opacity`, the percentage
/// beside it. Dragging previews, letting go commits; it takes only the
/// mouse ([`MouseOnly`]). Unless the document is `editable`, it's faded
/// and the app ignores it.
fn opacity_rows<'a>(body: BodyId, opacity: Opacity, editable: bool) -> Element<'a, Message> {
    let percent = |opacity: Opacity| f32::from(opacity.percent());
    let slider = slider(
        percent(Opacity::MIN)..=percent(Opacity::MAX),
        percent(opacity),
        move |percent| Message::Look(Look::PreviewOpacity(body, Opacity::clamped(percent))),
    )
    .step(OPACITY_STEP)
    .on_release(Message::Edit(Edit::CommitOpacity))
    .height(2.0 * theme::SLIDER_HANDLE_RADIUS)
    .style(theme::slider(editable));
    let heading = container(chrome::heading("Opacity")).padding([4, 8]);
    let value = text(opacity.to_string())
        .width(OPACITY_VALUE_WIDTH)
        .align_x(Alignment::End)
        .style(theme::muted_text);
    column![
        heading,
        row![MouseOnly::new(slider), value]
            .spacing(10)
            .height(28)
            .padding([0, 8])
            .align_y(Alignment::Center),
    ]
    .into()
}

/// The room the Opacity slider's percentage takes, "100 %" at the widest.
const OPACITY_VALUE_WIDTH: f32 = 40.0;

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
        selected,
        on_press,
        menu,
    } = object;
    let (on, open) = (menu.on, menu.open);
    let menu = open.then(|| menu.view(icon, visible, toggle.clone(), editable));
    // A button that shows only on hover keeps its room while it's hidden,
    // as the mock's do, so the note doesn't move: the hovered row is drawn
    // over the plain one, which shows through a translucent highlight.
    let room = |shown: bool, button: Option<Element<'static, Message>>| {
        button.or_else(|| shown.then(|| Space::new().width(icons::BUTTON_SIZE).into()))
    };
    let (has_eye, has_bin) = (toggle.is_some() && editable, remove.is_some() && editable);
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
                .into()
            });
        let remove = remove
            .clone()
            .filter(|_| hovered && editable)
            .map(|message| icon_button(Icon::Trash, Tone::Faint, Some(message)).into());
        let note = (note.clone()).map(|note| text(note).size(11.5).style(theme::faint_text));
        container(
            row![
                icons::icon(icon, icons::INLINE),
                name(label, visible),
                space::horizontal(),
                note,
                room(has_eye, eye),
                room(has_bin, remove),
            ]
            .spacing(8)
            .height(ROW_HEIGHT)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([0, 2]).left(24))
    };

    let row: Element<'_, Message> = match on_press {
        Some(message) => mouse_area(hover(
            content(false).style(theme::list_row(selected, false)),
            content(true).style(theme::list_row(selected, true)),
        ))
        .on_press(message)
        .into(),
        None => hover(content(false), content(true).style(theme::hovered_row)),
    };
    ContextMenu::new(
        row,
        menu,
        Message::Look(Look::OpenMenu(on)),
        Message::Look(Look::CloseMenu),
    )
    .into()
}

/// The sketch of `document` being edited: where it is, with a button
/// sending `change` to put it on another plane (disabled without), over
/// the Geometry list over the Constraints list, split by a divider that
/// can be dragged.
fn sketch_tab<'a>(
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
    let lists = split::vertical_split(
        sketch.split,
        move |height| geometry(sketch, height),
        move |height| constraints(sketch, height),
        |share| Message::Look(Look::SplitSketchTab(share)),
    );
    column![container(plane).padding(Padding::ZERO.top(6)), lists].into()
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
        let command = editor.document().add_feature(extrude.clone().into());
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
            let objects = objects(document, merged, true, None, &Default::default(), None);
            let mut laid = crate::testing::Laid::new(objects, iced::Size::new(300.0, 400.0));
            laid.texts().into_iter().map(|shown| shown.text).collect()
        };
        let shown = texts(&merged);
        let at = |text: &str| shown.iter().position(|shown| shown == text);
        // The note is on Body 2's row, after its name.
        let note = at("in Body 1").unwrap_or_else(|| panic!("{shown:?}"));
        assert_eq!(at("Body 2"), Some(note - 1), "{shown:?}");
        assert!(!texts(&[]).iter().any(|text| text.starts_with("in ")));

        // The row hovered is drawn over the plain one, which shows through
        // a translucent highlight: its note is laid out in the same place
        // in both, though the hovered one has its bin.
        let objects = objects(document, &merged, true, None, &Default::default(), None);
        let mut laid = crate::testing::Laid::new(objects, iced::Size::new(300.0, 400.0));
        let notes: Vec<_> = (laid.texts().into_iter())
            .filter(|shown| shown.text == "in Body 1")
            .map(|shown| shown.bounds)
            .collect();
        assert_eq!(notes.len(), 2, "plain and hovered");
        assert_eq!(notes[0], notes[1]);
    }

    /// A body's menu has an Opacity row between Hide and Delete, the
    /// slider's value beside it; a menu without an opacity, none.
    #[test]
    fn a_body_s_menu_has_an_opacity_row() {
        let body = Document::example().bodies()[0].id;
        let menu = |opacity| {
            let menu = ObjectMenu {
                on: RowMenu::Body(body),
                open: true,
                edit: None,
                opacity,
                delete: Message::Edit(Edit::RemoveBody(body)),
            };
            let toggle = Some(Message::Edit(Edit::ToggleVisible(body)));
            let view = menu.view(Icon::Body, true, toggle, true);
            let mut laid = crate::testing::Laid::new(view, iced::Size::new(300.0, 400.0));
            let shown = laid.texts().into_iter().map(|shown| shown.text);
            shown.collect::<Vec<_>>()
        };
        let shown = menu(Some((body, Opacity::new(40).unwrap())));
        assert_eq!(shown, ["Hide", "Opacity", "40 %", "Delete"]);
        assert_eq!(menu(None), ["Hide", "Delete"]);
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
