//! The Timeline tab: the document's features in order, with the
//! rollback marker.

use iced::Element;
use iced::widget::column;
use varde_document::{Document, Feature, FeatureId, FeatureKind, Named};

use super::rows::{
    SelectableRow, empty_note, rename_row, renamed, row_context, row_menu, visibility_item,
};
use crate::feature_text::{feature_icon, feature_note};
use crate::icons::Icon;
use crate::menu::{menu_item, menu_separator};
use crate::shortcut::Shortcut;
use crate::theme::Palette;
use crate::{DocumentState, Edit, Look, Message, Rollback, RowMenu};

/// What [`timeline`] lists, and how, out of a [`DocumentState`].
#[derive(Clone, Copy)]
pub(super) struct TimelineView<'a> {
    pub(super) document: &'a Document,
    pub(super) selected: Option<FeatureId>,
    pub(super) rollback: Rollback,
    pub(super) menu: Option<RowMenu>,
    pub(super) editable: bool,
    pub(super) unsolved: &'a [FeatureId],
    pub(super) failed: &'a [varde_regen::FeatureFailure],
    pub(super) renaming: Option<(Named, &'a str)>,
    pub(super) palette: &'static Palette,
}

impl<'a> TimelineView<'a> {
    pub(super) fn of(state: &DocumentState<'a>) -> Self {
        Self {
            document: state.editor.document(),
            selected: state.selected_feature,
            rollback: state.rollback,
            menu: state.row_menu,
            editable: state.editable(),
            unsolved: state.unsolved,
            failed: state.failed,
            renaming: state.renaming,
            palette: state.mode.palette(),
        }
    }
}

/// The features in the order they were added, the `selected` one
/// highlighted, the rollback marker before the feature `rollback` names
/// (at the end if none), the features after it faint, labelled (rolled
/// back or forward from the document's marker) and fixed while it's only
/// there as a feature is edited, unlabelled at the end, the one `menu` is on with its context menu open, which
/// right-clicking a feature asks for, those `unsolved` or
/// `failed` marked. Features can only be deleted if the document is
/// `editable`. The row `renaming` is of holds the rename field.
pub(super) fn timeline<'a>(view: TimelineView<'a>) -> Element<'a, Message> {
    let TimelineView {
        document,
        selected,
        rollback: Rollback {
            before: rollback,
            editing,
        },
        menu,
        editable,
        renaming,
        ..
    } = view;
    if document.features().is_empty() {
        return empty_note(format!(
            "No features yet. Press {} to start a sketch.",
            Shortcut::SKETCH.label()
        ));
    }
    let features = document.features();
    let ids: Vec<FeatureId> = features.iter().map(|feature| feature.id).collect();
    let kept = rollback
        .and_then(|until| ids.iter().position(|&id| id == until))
        .unwrap_or(ids.len());
    // While a feature is edited, which way that moved the marker from
    // where the document has it; nothing at the end where the document
    // has it there too, as for a new feature.
    let set = (document.rollback())
        .and_then(|until| ids.iter().position(|&id| id == until))
        .unwrap_or(ids.len());
    let label = (editing && (kept < ids.len() || kept != set)).then_some(if kept > set {
        "Rolled forward"
    } else {
        "Rolled back"
    });
    let marker = crate::rollback::marker(label);
    let mut rows: Vec<Element<'a, Message>> = features
        .iter()
        .enumerate()
        .map(|(index, feature)| {
            let rolled = index >= kept;
            // Rolled to just after it, by its menu.
            let after = ids.get(index + 1).copied();
            let roll = (after != rollback).then(|| {
                let label = if rolled {
                    "Roll forward here"
                } else {
                    "Roll back here"
                };
                let roll = Message::Edit(Edit::SetRollback(after));
                (label, (editable && !editing).then_some(roll))
            });
            if let Some(text) = renamed(renaming, Named::Feature(feature.id)) {
                return rename_row(feature_icon(feature), text, 8.0);
            }
            let selected = selected == Some(feature.id);
            let row = feature_row(&view, feature, rolled);
            let on = RowMenu::Feature(feature.id);
            let menu =
                (selected && menu == Some(on)).then(|| feature_menu(feature, editable, roll));
            row_context(row, on, menu)
        })
        .collect();
    rows.insert(kept, marker);
    crate::rollback::draggable(column(rows).into(), kept, ids, editable && !editing)
}

/// A feature of the `view`'s document in the Timeline, with its note: a
/// sketch's plane ([`plane_note`](crate::plane_note)), an extrude's
/// distances in the document's units, how far a revolve turns in all.
/// Marked failed if it's unsolved, or failed and why, which hovering it tells ("Extrude
/// fails: ...", as its panel says it); a failure whose geometry has a box
/// gets a Show button that frames the camera on it (a tooltip can't be
/// clicked, so it's in the row). Hovering it shows the failure's geometry
/// in the viewport. Faint if it's hidden, or `rolled` back (after the
/// rollback marker). Clicking selects it, double-clicking edits it.
pub(super) fn feature_row<'a>(
    view: &TimelineView<'a>,
    feature: &'a Feature,
    rolled: bool,
) -> Element<'a, Message> {
    let document = view.document;
    let unsolved = view.unsolved.contains(&feature.id);
    let failed = (view.failed.iter()).find(|failed| failed.feature == feature.id);
    let note = match &feature.kind {
        FeatureKind::Sketch { .. } if unsolved => "Doesn't solve".into(),
        _ => feature_note(document, feature, document.units()).into(),
    };
    let row = SelectableRow {
        icon: feature_icon(feature),
        name: feature.name.as_str().into(),
        faint: !feature.visible || rolled,
        dim: rolled,
        danger: false,
        failed: unsolved || failed.is_some(),
        note: Some(note),
        indent: 8.0,
        selected: view.selected == Some(feature.id),
        construction: false,
    };
    let row = row
        .view(Message::Look(Look::ClickFeature {
            feature: feature.id,
            roll: false,
        }))
        .on_double_click(Message::Look(Look::EditFeature(feature.id)))
        .on_enter(Message::Look(Look::HoverFeature(Some(feature.id))))
        .on_exit(Message::Look(Look::LeaveFeature(feature.id)));
    match failed {
        Some(failed) => crate::chrome::failure_tip(
            row,
            format!("{} fails: ", feature.kind.noun()),
            crate::chrome::sentence(&failed.message),
            view.palette,
        ),
        None => row.into(),
    }
}

/// The context menu of `feature` in the Timeline: edit it, rename it,
/// show or hide a sketch or put it on another plane, `roll` the Timeline
/// to just after it (its label, and what it sends if it can), or delete
/// it, all but editing only if the document is `editable`, by the keys
/// that do the same to the feature selected.
pub(super) fn feature_menu<'a>(
    feature: &Feature,
    editable: bool,
    roll: Option<(&'static str, Option<Message>)>,
) -> Element<'a, Message> {
    let id = feature.id;
    let edit = menu_item(
        feature_icon(feature),
        edit_label(feature).into(),
        Some(Shortcut::ENTER),
        Some(Message::Look(Look::EditFeature(id))),
    );
    let sketch = matches!(feature.kind, FeatureKind::Sketch { .. });
    let toggle = sketch.then(|| {
        visibility_item(
            feature.visible,
            editable,
            Message::Edit(Edit::ToggleFeatureVisible(id)),
        )
    });
    let change_plane = sketch.then(|| {
        menu_item(
            Icon::Plane,
            CHANGE_PLANE.into(),
            None,
            editable.then_some(Message::Look(Look::ChangePlane(id))),
        )
        .into()
    });
    let rename = menu_item(
        Icon::Rename,
        "Rename".into(),
        Some(Shortcut::RENAME),
        editable.then_some(Message::Look(Look::StartRename(Named::Feature(id)))),
    );
    let roll = roll.map(|(label, roll)| menu_item(Icon::Rollback, label.into(), None, roll).into());
    row_menu(
        vec![
            Some(edit.into()),
            Some(rename.into()),
            toggle,
            change_plane,
            roll,
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
pub(super) fn edit_label(feature: &Feature) -> String {
    format!("Edit {}", feature.kind.noun().to_lowercase())
}
