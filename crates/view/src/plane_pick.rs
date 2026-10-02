//! Picking a plane for a sketch: an origin plane from the toolbar, or a
//! flat face of the model in the viewport, for a new sketch or for one
//! whose plane is changed.

use std::borrow::Cow;

use glam::DVec3;
use varde_document::{BodyId, Document, FaceRef, FeatureId, Plane};
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
    /// The bodies whose faces can take the sketch, ascending: the
    /// document's, made by a feature before the sketch if its plane is
    /// changed.
    bodies: Vec<BodyId>,
    /// The numbers of the features whose faces can take the sketch,
    /// ascending: the document's, before the sketch if its plane is
    /// changed.
    features: Vec<u64>,
    /// Each body a join merged into another, as the model shown has it:
    /// the number of the feature that made it, it, and the body holding
    /// it, whose faces the model shows as its own.
    merged: Vec<(u64, BodyId, BodyId)>,
}

impl PlanePick {
    /// Picking the plane for a new sketch of `document`, whose model
    /// shown has joins merge `merged` (each body and the one holding it):
    /// any flat face of it takes it.
    pub fn new_sketch(document: &Document, merged: &[(BodyId, BodyId)]) -> Self {
        Self::taking(document, document.features().len(), merged)
    }

    /// Picking another plane for the sketch feature `sketch` of
    /// `document`, whose model shown has joins merge `merged`, which
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
        merged: &[(BodyId, BodyId)],
    ) -> Option<Self> {
        let index = (document.features().iter()).position(|feature| feature.id == sketch)?;
        let feature = &document.features()[index];
        if !matches!(feature.kind, varde_document::FeatureKind::Sketch { .. }) {
            return None;
        }
        Some(Self {
            sketch: Some((sketch, feature.name.clone())),
            failed,
            ..Self::taking(document, index, merged)
        })
    }

    /// Faces of `document` named by its first `before` features, of
    /// bodies not made by a later one, taking a sketch.
    fn taking(document: &Document, before: usize, merged: &[(BodyId, BodyId)]) -> Self {
        let later = &document.features()[before..];
        let mut features: Vec<u64> = (document.features()[..before].iter())
            .map(|feature| feature.id.get())
            .collect();
        features.sort_unstable();
        let mut bodies: Vec<BodyId> = (document.bodies().iter())
            .filter(|body| !later.iter().any(|feature| feature.id == body.created_by))
            .map(|body| body.id)
            .collect();
        bodies.sort_unstable();
        let merged = (merged.iter())
            .filter_map(|&(body, holder)| {
                Some((document.body(body)?.created_by.get(), body, holder))
            })
            .collect();
        Self {
            sketch: None,
            failed: None,
            bodies,
            features,
            merged,
        }
    }

    /// The reference to face `face` of `index`'s model picked at `near`,
    /// as a sketch on it stores it: of the body the model shows it on,
    /// or if that holds a body a join merged into it and the feature that
    /// made that body names the face, of that body. Regenerating follows
    /// a merged body on to the one holding it, so either finds the face
    /// at the end of the history, but only the body made with the face
    /// has it before the join: where a sketch whose plane is changed may
    /// be.
    pub fn face_ref(&self, index: &PickIndex, face: u32, near: DVec3) -> Option<FaceRef> {
        let mut found = index.face_ref(face, near)?;
        if let Some(&(_, body, _)) = (self.merged.iter())
            .find(|&&(maker, _, holder)| maker == found.key.feature && holder == found.body)
        {
            found.body = body;
        }
        Some(found)
    }

    /// Why face `face` of `index`'s model can't take the sketch, if it
    /// can't: it isn't flat, or it came after the sketch, or it isn't
    /// the document's (a model shown from before an undo).
    pub fn refusal(&self, index: &PickIndex, face: u32) -> Option<Cow<'static, str>> {
        if index.face_placement(face).is_none() {
            return Some(CURVED_FACE.into());
        }
        let taken = self
            .face_ref(index, face, DVec3::ZERO)
            .is_some_and(|found| {
                self.bodies.binary_search(&found.body).is_ok()
                    && self.features.binary_search(&found.key.feature).is_ok()
            });
        match (&self.sketch, taken) {
            (_, true) => None,
            (Some((_, name)), false) => {
                Some(format!("{name} can only go on a face made before it").into())
            }
            (None, false) => Some("That face isn't in the model".into()),
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
