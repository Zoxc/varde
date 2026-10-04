//! The faces a face session picks, shared by the sessions picking faces
//! of one body (a shell's; an offset face's and a draft's once there are
//! those): what the app hands the panel of them and their field, each
//! face a row, "Face 2" by its place in the list (as regeneration's
//! messages count them, "its open face 2 of 3 wasn't found"), with what
//! kind of face it is beside it and a cross taking it out.

use iced::Element;
use varde_document::FaceRef;

use super::{MotionLook, MotionPick, MotionState};
use crate::icons::Icon;
use crate::operation_panel::{PanelHover, field, pick_field, picked_row};
use crate::{Look, Message};

/// A face picked, as its row shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PickedFace {
    /// The face as the feature stores it, which its cross takes out.
    pub face: FaceRef,
    /// Its name: "Face 2", its place in the list.
    pub name: String,
    /// Beside it, what kind of face it is in the model shown, if it's
    /// found there: "Planar face", "Cylindrical face".
    pub meta: Option<String>,
}

/// The faces picked, sorted as the feature keeps them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PickedFaces {
    pub faces: Vec<PickedFace>,
}

/// The panel's field of faces, labelled `label`: a row per face, and
/// while picking, or with none, where to click (`place`).
pub(super) fn faces_field<'a>(
    state: &MotionState<'a>,
    faces: &PickedFaces,
    label: &'a str,
    place: &str,
) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: MotionLook| editable.then_some(Message::Look(Look::Motion(look)));
    let on = state.picking == MotionPick::Faces;
    let press = send(MotionLook::Picking(MotionPick::Faces));
    let rows: Vec<_> = (faces.faces.iter().enumerate())
        .map(|(at, face)| {
            picked_row(
                Icon::SeFace,
                face.name.clone(),
                face.meta.clone(),
                send(MotionLook::DropFace(face.face)),
                press.clone(),
                PanelHover::Face(at),
                state.hover,
            )
        })
        .collect();
    let place = (rows.is_empty() || on).then(|| place.to_owned());
    field(label, pick_field(rows, place, on, press))
}
