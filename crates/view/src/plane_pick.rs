//! Picking a plane for a sketch: an origin plane from the toolbar, or a
//! flat face of the model in the viewport, for a new sketch or for one
//! whose plane is changed.

use std::borrow::Cow;

use glam::DVec3;
use varde_document::{Document, FaceRef, FeatureId, FeatureKind, Plane};
use varde_kernel_compat::mesh::PartKey;

use crate::document::CURVED_FACE;
use crate::naming::NamingPicks;
use crate::pick::PickIndex;
use varde_regen::{Naming, Shown, Unnamed};

/// What a plane is being picked for, and so which faces can take it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanePick {
    /// The sketch whose plane is changed and its name; none for a new
    /// sketch.
    pub sketch: Option<(FeatureId, String)>,
    /// Why the sketch has to be put on another plane, if it does: why
    /// regenerating couldn't place it ("its face wasn't found").
    pub failed: Option<String>,
    /// How the faces that can take it are named: those of the document
    /// before the sketch if its plane is changed.
    naming: Naming,
}

impl PlanePick {
    /// Picking the plane for a new sketch of `document`, whose model
    /// shown found `shown`: any flat face of it takes it.
    pub fn new_sketch(document: &Document, shown: Shown) -> Self {
        Self {
            naming: Naming::before(document, document.features().len(), shown),
            ..Self::default()
        }
    }

    /// Picking another plane for the sketch feature `sketch` of
    /// `document`, whose model shown found `shown`, which
    /// `failed` to be placed if it says why: only faces of bodies made
    /// before it, named by features before it, as [`Document::check`]
    /// has a sketch's face, which also lets a face name a body or
    /// feature that's gone, but such a face is of a model shown from
    /// before an edit, so it's refused here. None if it isn't a sketch
    /// of `document`.
    pub fn change(
        document: &Document,
        sketch: FeatureId,
        failed: Option<String>,
        shown: Shown,
    ) -> Option<Self> {
        let index = document.feature_index(sketch)?;
        let feature = &document.features()[index];
        if !matches!(feature.kind, FeatureKind::Sketch { .. }) {
            return None;
        }
        Some(Self {
            sketch: Some((sketch, feature.name.clone())),
            failed,
            naming: Naming::before(document, index, shown),
        })
    }

    /// The reference to face `face` of `index`'s model picked at `near`,
    /// as a sketch on it stores it: see [`Naming::face_ref`].
    pub fn face_ref(&self, index: &PickIndex, face: u32, near: DVec3) -> Option<FaceRef> {
        self.naming.face_ref(index, face, near)
    }

    /// Why face `face` of `index`'s model can't take the sketch, if it
    /// can't: it isn't flat, or it came after the sketch, or it isn't
    /// the document's (a model shown from before an undo).
    pub fn refusal(&self, index: &PickIndex, face: u32) -> Option<Cow<'static, str>> {
        if index.face_placement(face).is_none() {
            return Some(CURVED_FACE.into());
        }
        let unnamed = (self.naming)
            .checked_face_ref(index, face, DVec3::ZERO)
            .err()?;
        Some(match (&self.sketch, unnamed) {
            (None, _) => "That face isn't in the model".into(),
            (Some((_, name)), Unnamed::Unclear) => {
                format!("Which body that face is on at {name} can't be told: pick another").into()
            }
            (Some((_, name)), Unnamed::Later | Unnamed::Missing) => {
                format!("{name} can only go on a face made before it").into()
            }
        })
    }

    /// Whether face `face` of `index`'s model can take the sketch.
    pub fn takes(&self, index: &PickIndex, face: u32) -> bool {
        self.refusal(index, face).is_none()
    }

    /// What the status bar asks for while picking.
    pub(crate) fn asking(&self) -> String {
        match (&self.sketch, &self.failed) {
            (None, _) => "Pick a plane or a flat face for the new sketch".to_owned(),
            (Some((_, name)), Some(why)) => format!("{name}: {why}. Pick a plane for it"),
            (Some((_, name)), None) => format!("Pick a plane or a flat face for {name}"),
        }
    }
}

/// Where a sketch on `plane` is, as the Timeline notes it: "XY", or the
/// face by the feature that made it, "on Extrude 1's end", or by its body
/// if that feature is gone, "on Body 1".
pub fn plane_note(document: &Document, plane: &Plane) -> String {
    match plane.face() {
        Some(face) => format!("on {}", face_name(document, face)),
        None => plane.name().to_owned(),
    }
}

/// A face of the model as its reference `face` names it, for notes:
/// by the feature that made it, "Extrude 1's end", "a face of Revolve
/// 1", or by its body if that feature is gone, "Body 1", or "a face".
pub fn face_name(document: &Document, face: &FaceRef) -> String {
    if let Some(maker) = document.feature(face.maker()) {
        let part = match face.key.part {
            PartKey::StartCap => "start",
            PartKey::EndCap => "end",
            PartKey::Side { .. } => "side",
            _ => return format!("a face of {}", maker.name),
        };
        return format!("{}'s {part}", maker.name);
    }
    match document.body(face.body) {
        Some(body) => body.name.clone(),
        None => "a face".to_owned(),
    }
}

/// Where a sketch on `plane` is, for the status bar: "on XY", "on
/// Extrude 1's end".
pub(crate) fn on_plane(document: &Document, plane: &Plane) -> String {
    match plane {
        Plane::Origin(plane) => format!("on {}", plane.name()),
        Plane::Face(_) => plane_note(document, plane),
    }
}
