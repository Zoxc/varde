//! The faces a face session picks, shared by the sessions picking faces
//! of one body (a shell's, an offset face's; a draft's once there's
//! one): what the app hands the panel of them and their field, each
//! face a row, "Face 2" by its place in the list (as regeneration's
//! messages count them, "its open face 2 of 3 wasn't found"), with what
//! kind of face it is beside it and a cross taking it out.

use iced::Element;
use varde_document::FaceRef;

use super::{MotionLook, MotionPick, MotionState, PickedRow, picks_field};
use crate::Message;
use crate::icons::Icon;
use crate::operation_panel::PanelHover;

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
    let rows = (faces.faces.iter().enumerate()).map(|(at, face)| PickedRow {
        icon: Icon::SeFace,
        name: face.name.clone(),
        meta: face.meta.clone(),
        drop: MotionLook::DropFace(face.face),
        hover: PanelHover::Face(at),
        failed: false,
    });
    picks_field(state, MotionPick::Faces, label, place, rows)
}
