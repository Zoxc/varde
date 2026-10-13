//! The Objects tab: the origin, the bodies and the sketches, with each
//! body's menu.

use iced::widget::{Space, column, container, hover, mouse_area, opaque, row, space, text};
use iced::{Alignment, Element, Padding};
use std::collections::BTreeSet;
use varde_document::{BodyId, Document, Feature, FeatureKind, Named, Opacity, Tint};
use varde_render::OriginShown;

use super::body_look::{color_rows, look_area, opacity_rows};
use super::rows::{
    BODY_MENU_WIDTH, ROW_MENU_WIDTH, name, rename_row, renamed, row_context, sized_row_menu,
    tree_header, visibility_item,
};
use super::timeline::edit_label;
use super::{ObjectGroup, ObjectRow, OriginObject, ROW_HEIGHT};
use crate::chrome::icon_button;
use crate::document::{shown_color, shown_opacity};
use crate::icons::{self, Icon};
use crate::menu::{menu_item, menu_separator};
use crate::theme::{self, Tone};
use crate::{DocumentState, Edit, Look, Message, RowMenu};

/// What [`objects`] lists, and how, out of a [`DocumentState`].
#[derive(Clone, Copy)]
pub(super) struct ObjectsView<'a> {
    pub(super) document: &'a Document,
    pub(super) merged: &'a [(BodyId, BodyId)],
    pub(super) editable: bool,
    pub(super) menu: Option<RowMenu>,
    pub(super) selection: &'a crate::Selection,
    pub(super) preview: Option<(BodyId, Opacity)>,
    pub(super) color_preview: Option<(BodyId, Tint)>,
    pub(super) body_look_hovered: bool,
    pub(super) origin: OriginShown,
    pub(super) rows: &'a [ObjectRow],
    pub(super) folded: &'a BTreeSet<ObjectGroup>,
    pub(super) renaming: Option<(Named, &'a str)>,
}

impl<'a> ObjectsView<'a> {
    /// `document`'s, editable, with nothing merged, selected, folded,
    /// previewed or renamed, and the origin as a new document shows it.
    #[cfg(test)]
    pub(super) fn plain(document: &'a Document) -> Self {
        static NO_SELECTION: std::sync::LazyLock<crate::Selection> =
            std::sync::LazyLock::new(Default::default);
        static NOT_FOLDED: BTreeSet<ObjectGroup> = BTreeSet::new();
        Self {
            document,
            merged: &[],
            editable: true,
            menu: None,
            selection: &NO_SELECTION,
            preview: None,
            color_preview: None,
            body_look_hovered: false,
            origin: OriginShown::DEFAULT,
            rows: &[],
            folded: &NOT_FOLDED,
            renaming: None,
        }
    }

    pub(super) fn of(state: &DocumentState<'a>) -> Self {
        Self {
            document: state.editor.document(),
            merged: state.merged,
            editable: state.editable(),
            menu: state.row_menu,
            selection: state.model_selection,
            preview: state.opacity_preview,
            color_preview: state.color_preview,
            body_look_hovered: state.body_look_hovered,
            origin: state.origin,
            rows: state.objects_selected,
            folded: state.objects_folded,
            renaming: state.renaming,
        }
    }
}

/// The world's origin objects, as `origin` has them shown, then the
/// bodies, then the sketches. The origin objects can be shown and hidden,
/// read-only too, as that's no edit, but not deleted. A body a join merged into another
/// (`merged`, see [`DocumentState::merged`]) is listed faint, with the body
/// holding it as its note: it's drawn as that one is, so it has no eye
/// nor opacity or colour of its own, but it can still be removed.
/// Right-clicking a row asks for its context menu, shown on the one `menu`
/// is on, with a body's opacity as `preview` has it, and its colour as
/// `color_preview` has it, while their sliders are dragged.
/// Clicking a body's row selects it where bodies are selected, as
/// `selection`, which marks the rows of the bodies it holds, says. Each
/// group's header folds it, as the Sketch tab's do, those `folded`
/// listing nothing under it.
pub(super) fn objects<'a>(view: ObjectsView<'a>) -> Element<'a, Message> {
    let ObjectsView {
        document,
        merged,
        editable,
        menu,
        selection,
        preview,
        color_preview,
        body_look_hovered,
        origin,
        rows,
        folded,
        renaming,
    } = view;
    let click = |row| Message::Look(Look::ClickObject { row, add: false });
    let origins = OriginObject::ALL.into_iter().map(|object| {
        let (icon, label) = origin_label(object);
        object_row(Object {
            icon,
            label,
            visible: *object.shown(&mut { origin }),
            editable: true,
            toggle: Some(Message::Look(Look::ToggleOrigin(object))),
            remove: None,
            note: None,
            selected: rows.contains(&ObjectRow::Origin(object)),
            on_press: Some(click(ObjectRow::Origin(object))),
            on_double_click: None,
            hover: Some((
                Message::Look(Look::HoverOrigin(Some(object))),
                Message::Look(Look::LeaveOrigin(object)),
            )),
            menu: None,
        })
    });
    let selected: Vec<BodyId> = selection.bodies().collect();
    let takes_bodies = selection.mode().takes_bodies();
    let bodies = document.bodies().iter().map(|body| {
        if let Some(text) = renamed(renaming, Named::Body(body.id)) {
            return rename_row(Icon::Body, text, 24.0);
        }
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
            on_double_click: None,
            hover: Some((
                Message::Look(Look::HoverBodyRow(Some(body.id))),
                Message::Look(Look::LeaveBodyRow(body.id)),
            )),
            menu: Some(ObjectMenu {
                on: RowMenu::Body(body.id),
                open: menu == Some(RowMenu::Body(body.id)),
                edit: None,
                rename: Message::Look(Look::StartRename(Named::Body(body.id))),
                opacity: own.then(|| (body.id, shown_opacity(body, preview))),
                color: own.then(|| (body.id, shown_color(body, color_preview))),
                look_hovered: body_look_hovered,
                delete: Message::Edit(Edit::RemoveBody(body.id)),
            }),
        })
    });
    let is_sketch = |feature: &&Feature| matches!(feature.kind, FeatureKind::Sketch { .. });
    let count = document.features().iter().filter(is_sketch).count();
    let sketches = document.features().iter().filter(is_sketch).map(|feature| {
        if let Some(text) = renamed(renaming, Named::Feature(feature.id)) {
            return rename_row(Icon::Sketch, text, 24.0);
        }
        object_row(Object {
            icon: Icon::Sketch,
            label: &feature.name,
            visible: feature.visible,
            editable,
            toggle: Some(Message::Edit(Edit::ToggleFeatureVisible(feature.id))),
            remove: None,
            note: None,
            selected: rows.contains(&ObjectRow::Sketch(feature.id)),
            on_press: Some(click(ObjectRow::Sketch(feature.id))),
            on_double_click: Some(Message::Look(Look::EditFeature(feature.id))),
            hover: None,
            menu: Some(ObjectMenu {
                on: RowMenu::Sketch(feature.id),
                open: menu == Some(RowMenu::Sketch(feature.id)),
                edit: Some((
                    edit_label(feature),
                    Message::Look(Look::EditFeature(feature.id)),
                )),
                rename: Message::Look(Look::StartRename(Named::Feature(feature.id))),
                opacity: None,
                color: None,
                look_hovered: false,
                delete: Message::Edit(Edit::RemoveFeature(feature.id)),
            }),
        })
    });
    let header = |label, count, group| {
        let toggle = Message::Look(Look::ToggleObjectGroup(group));
        container(tree_header(label, count, folded.contains(&group), toggle))
            .padding([0, 8])
            .into()
    };
    let origin_folded = folded.contains(&ObjectGroup::Origin);
    let bodies_folded = folded.contains(&ObjectGroup::Bodies);
    let sketches_folded = folded.contains(&ObjectGroup::Sketches);
    let bodies_count = bodies_after_joins(document, merged);
    column(
        std::iter::once(header(
            "Origin",
            OriginObject::ALL.len(),
            ObjectGroup::Origin,
        ))
        .chain(origins.filter(|_| !origin_folded))
        .chain([header("Bodies", bodies_count, ObjectGroup::Bodies)])
        .chain(bodies.filter(|_| !bodies_folded))
        .chain([header("Sketches", count, ObjectGroup::Sketches)])
        .chain(sketches.filter(|_| !sketches_folded)),
    )
    .into()
}

/// The icon and name of `object` in Objects.
pub(super) fn origin_label(object: OriginObject) -> (Icon, &'static str) {
    match object {
        OriginObject::Point => (Icon::Point, "Origin"),
        OriginObject::Axis(axis) => (Icon::SeAxis, crate::motion::axis_label(axis)),
        OriginObject::Plane(plane) => (
            crate::icons::plane_icon(plane),
            crate::motion::plane_label(plane),
        ),
    }
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
    let holder = varde_regen::merged_into(merged, body)?;
    Some(format!("in {}", document.body(holder)?.name))
}

/// An object in the Objects list.
pub(super) struct Object<'a> {
    pub(super) icon: Icon,
    pub(super) label: &'a str,
    /// Whether it's drawn: shown faint if not.
    pub(super) visible: bool,
    pub(super) editable: bool,
    /// What its eye sends, if it has one.
    pub(super) toggle: Option<Message>,
    /// What its remove button sends, if it has one.
    pub(super) remove: Option<Message>,
    /// Shown faint at its end.
    pub(super) note: Option<String>,
    /// Whether it's marked selected.
    pub(super) selected: bool,
    /// What clicking the row sends, if anything.
    pub(super) on_press: Option<Message>,
    /// What double-clicking the row sends, if anything: a sketch's opens
    /// it, as its Timeline row's does.
    pub(super) on_double_click: Option<Message>,
    /// What entering and leaving the row send, if anything: the
    /// viewport lights the object while it's hovered.
    pub(super) hover: Option<(Message, Message)>,
    /// Its context menu, if it has one.
    pub(super) menu: Option<ObjectMenu>,
}

/// The context menu of an object in the Objects list.
pub(super) struct ObjectMenu {
    /// The row it's on.
    pub(super) on: RowMenu,
    pub(super) open: bool,
    /// What editing the object is called and sends, if it can be edited.
    pub(super) edit: Option<(String, Message)>,
    /// What renaming it sends.
    pub(super) rename: Message,
    /// The body it is and how opaque it's shown, if it has an opacity.
    pub(super) opacity: Option<(BodyId, Opacity)>,
    /// The body it is and the colour it's shown in, none for the theme's,
    /// if it has a colour of its own to set.
    pub(super) color: Option<(BodyId, Option<Tint>)>,
    /// Whether its Opacity and Colour part is hovered, washed so.
    pub(super) look_hovered: bool,
    /// What deleting it sends.
    pub(super) delete: Message,
}

impl ObjectMenu {
    /// The menu, for an object `visible` or not, whose eye sends
    /// `toggle` if it has one: editing it, renaming it, showing or hiding
    /// it, its opacity and colour if it has them and deleting it, all but
    /// editing only if the
    /// document is `editable`. No keys are given, as the keys act on the
    /// Timeline's selection.
    pub(super) fn view<'a>(
        self,
        icon: Icon,
        visible: bool,
        toggle: Option<Message>,
        editable: bool,
    ) -> Element<'a, Message> {
        let edit = (self.edit)
            .map(|(label, message)| menu_item(icon, label.into(), None, Some(message)).into());
        let rename = menu_item(
            Icon::Rename,
            "Rename".into(),
            None,
            editable.then_some(self.rename),
        );
        let toggle = toggle.map(|toggle| visibility_item(visible, editable, toggle));
        let parts: Vec<Element<'a, Message>> = (self.opacity)
            .map(|(body, opacity)| opacity_rows(body, opacity, editable))
            .into_iter()
            .chain((self.color).map(|(body, color)| color_rows(body, color, editable)))
            .collect();
        let look = (!parts.is_empty())
            .then(|| [menu_separator().into(), look_area(parts, self.look_hovered)]);
        let width = if self.opacity.is_some() || self.color.is_some() {
            BODY_MENU_WIDTH
        } else {
            ROW_MENU_WIDTH
        };
        let delete = menu_item(
            Icon::Trash,
            "Delete".into(),
            None,
            editable.then_some(self.delete),
        );
        sized_row_menu(
            edit.into_iter()
                .chain([rename.into()])
                .chain(toggle)
                .chain(look.into_iter().flatten())
                .chain([menu_separator().into(), delete.into()])
                .collect(),
            width,
        )
    }
}

/// An object's row in the Objects list. The eye, sending `toggle`, and the
/// remove button, sending `remove` if there is one, show on hover; the eye
/// also shows while the object is hidden. Unless the document is
/// `editable`, only the eye of a hidden object shows, and does nothing.
/// Without a `toggle` there's no eye.
pub(super) fn object_row(object: Object<'_>) -> Element<'_, Message> {
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
        on_double_click,
        hover: on_hover,
        menu,
    } = object;
    let on = menu.as_ref().map(|menu| menu.on);
    let menu = (menu.filter(|menu| menu.open))
        .map(|menu| menu.view(icon, visible, toggle.clone(), editable));
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
        Some(message) => {
            let row = mouse_area(hover(
                content(false).style(theme::list_row(selected, false)),
                content(true).style(theme::list_row(selected, true)),
            ))
            .on_press(message);
            match on_double_click {
                Some(message) => row.on_double_click(message).into(),
                None => row.into(),
            }
        }
        None => {
            let row = hover(content(false), content(true).style(theme::hovered_row));
            match on_double_click {
                Some(message) => mouse_area(row).on_double_click(message).into(),
                // Not empty: a click on it doesn't clear the selection.
                None => opaque(row),
            }
        }
    };
    let row = match on_hover {
        Some((enter, exit)) => mouse_area(row).on_enter(enter).on_exit(exit).into(),
        None => row,
    };
    match on {
        Some(on) => row_context(row, on, menu),
        None => row,
    }
}
