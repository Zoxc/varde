//! A draft in the move's session ([`MotionKind::Draft`]): its faces
//! picked as a face session's (see `faces`), its neutral plane picked as
//! a mirror's plane (an origin plane from the toolbar, XY to begin
//! with, or a flat face clicked while its row picks), its angle typed,
//! Flip (the session's flip) and Tangent faces.

use glam::DVec3;
use varde_document::{Design, FaceDraft, FeatureKind, OriginPlane, PlaneRef};
use varde_expr::Value;
use varde_view::{DraftView, MotionField, MotionKind};

use super::refs::Refs;
use super::{Doc, MotionSession};
use crate::doc::regions::TypedText;

/// The neutral plane a new draft starts with: XY, so the pull is up.
pub(super) const NEUTRAL: PlaneRef = PlaneRef::Origin(OriginPlane::XY);

/// The angle field as a new one opens it: 3 (degrees).
pub(super) fn angle_field(design: &Design) -> TypedText {
    let ask = FaceDraft::angle_ask(design);
    Value::new("3", &ask).map_or_else(
        |_| TypedText::read("3".to_owned(), &ask),
        |value| TypedText::of(&value, &ask),
    )
}

impl MotionSession {
    /// The draft as set up, if it's whole: its faces (at least one), its
    /// neutral plane, its angle as it last read, Flip and Tangent faces.
    pub(super) fn face_draft(&self) -> Option<FaceDraft> {
        if self.faces.refs.is_empty() {
            return None;
        }
        Some(FaceDraft {
            faces: self.faces.refs.clone(),
            neutral: self.plane?,
            angle: self.field(MotionField::Angle).value.clone()?,
            flip: self.flip,
            tangent: self.tangent,
        })
    }

    /// Opens the draft `draft` in this session: its faces, neutral plane,
    /// angle, Flip and Tangent faces.
    pub(super) fn open_face_draft(&mut self, draft: &FaceDraft) {
        let ask = FaceDraft::angle_ask(&self.design);
        self.fields[MotionField::Angle.index()] = TypedText::of(&draft.angle, &ask);
        self.plane = Some(draft.neutral);
        self.flip = draft.flip;
        self.tangent = draft.tangent;
        self.faces = Refs::of(&draft.faces);
        self.faces_body();
    }
}

impl Doc {
    /// What the panel shows of the draft being set up.
    pub(super) fn face_draft_view(&self, session: &MotionSession) -> DraftView {
        let document = self.editor.document();
        DraftView {
            faces: self.picked_faces(session),
            flip: session.flip,
            tangent: session.tangent,
            info: (session.face_draft())
                .filter(|_| session.kind == MotionKind::Draft)
                .map(|draft| varde_view::draft_info(document, &draft)),
        }
    }
}

/// Where a draft's neutral plane is drawn: through the point of the plane
/// (`point`, its unit or other `normal`) nearest the centre of the
/// bodies' box `bounds` if it's known, along the pull (the normal,
/// reversed by `flip`), so the pull's arrow runs through the body.
pub(super) fn neutral_line(
    [point, normal]: [DVec3; 2],
    bounds: Option<[DVec3; 2]>,
    flip: bool,
) -> Option<[DVec3; 2]> {
    let normal = normal.try_normalize()?;
    let at = match bounds {
        Some([low, high]) => {
            let centre = (low + high) / 2.0;
            centre - normal * (centre - point).dot(normal)
        }
        None => point,
    };
    let pull = if flip { -normal } else { normal };
    at.is_finite().then_some([at, pull])
}

/// The feature a draft session makes.
pub(super) fn face_draft_kind(session: &MotionSession) -> Option<FeatureKind> {
    session.face_draft().map(FeatureKind::FaceDraft)
}
