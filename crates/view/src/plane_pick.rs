//! Picking a plane for a sketch: an origin plane from the toolbar, or a
//! flat face of the model in the viewport, for a new sketch or for one
//! whose plane is changed.

use std::borrow::Cow;

use glam::DVec3;
use varde_document::{BodyId, Document, FeatureId, Plane};
use varde_kernel::mesh::PartKey;

use crate::document::CURVED_FACE;
use crate::pick::PickIndex;

/// What a plane is being picked for, and so which faces can take it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanePick {
    /// The sketch whose plane is changed and its name; none for a new
    /// sketch.
    pub sketch: Option<(FeatureId, String)>,
    /// Why the sketch has to be put on another plane, if it does: why
    /// regenerating couldn't place it ("its face wasn't found").
    pub failed: Option<String>,
    /// The bodies made by the sketch or features after it: a sketch can
    /// only go on a face of a body made before it.
    later_bodies: Vec<BodyId>,
    /// The numbers of the sketch and the features after it: a face named
    /// by one of them came later than the sketch too.
    later_features: Vec<u64>,
}

impl PlanePick {
    /// Picking the plane for a new sketch: any flat face takes it.
    pub fn new_sketch() -> Self {
        Self::default()
    }

    /// Picking another plane for the sketch feature `sketch` of
    /// `document`, which `failed` to be placed if it says why: only faces
    /// of bodies made before it, named by features before it, as
    /// [`Document::check`] has a sketch's face. None if it isn't a
    /// sketch of `document`.
    pub fn change(document: &Document, sketch: FeatureId, failed: Option<String>) -> Option<Self> {
        let index = (document.features().iter()).position(|feature| feature.id == sketch)?;
        let feature = &document.features()[index];
        if !matches!(feature.kind, varde_document::FeatureKind::Sketch { .. }) {
            return None;
        }
        let later = &document.features()[index..];
        let later_features: Vec<u64> = later.iter().map(|feature| feature.id.get()).collect();
        let later_bodies = (document.bodies().iter())
            .filter(|body| later.iter().any(|feature| feature.id == body.created_by))
            .map(|body| body.id)
            .collect();
        Some(Self {
            sketch: Some((sketch, feature.name.clone())),
            failed,
            later_bodies,
            later_features,
        })
    }

    /// Why face `face` of `index`'s model can't take the sketch, if it
    /// can't: it isn't flat, or it came after the sketch.
    pub fn refusal(&self, index: &PickIndex, face: u32) -> Option<Cow<'static, str>> {
        if index.face_placement(face).is_none() {
            return Some(CURVED_FACE.into());
        }
        let later = index.face_ref(face, DVec3::ZERO).is_none_or(|found| {
            self.later_bodies.contains(&found.body)
                || self.later_features.contains(&found.key.feature)
        });
        match (&self.sketch, later) {
            (Some((_, name)), true) => {
                Some(format!("{name} can only go on a face made before it").into())
            }
            (None, true) => Some("That face isn't in the model".into()),
            (_, false) => None,
        }
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
    let Some(face) = plane.face() else {
        return plane.name().to_owned();
    };
    if let Some(maker) = document.feature(face.maker()) {
        let part = match face.key.part {
            PartKey::StartCap => "start",
            PartKey::EndCap => "end",
            PartKey::Side { .. } => "side",
            _ => return format!("on a face of {}", maker.name),
        };
        return format!("on {}'s {part}", maker.name);
    }
    match document.body(face.body) {
        Some(body) => format!("on {}", body.name),
        None => "on a face".to_owned(),
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
