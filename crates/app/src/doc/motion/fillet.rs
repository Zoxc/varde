//! A fillet in the move's session ([`MotionKind::Fillet`]): its edges
//! picked as a blend's (see `blend`), its radius and the Tangent chain
//! tick, the model mock's fillet panel.

use varde_document::{Design, FeatureKind, Fillet};
use varde_render::Camera;
use varde_view::{FilletView, MotionField, MotionKind};

use super::blend::BlendSetup;
use super::refs::Refs;
use super::{BLEND_SHARE, Doc, MotionSession, length_field};
use crate::doc::camera::fitting_length;
use crate::doc::regions::TypedText;

/// The fillet's radius field as a new one opens it, seen by `camera`:
/// [`BLEND_SHARE`] of the view's height made nice ([`fitting_length`]),
/// in the design's units with their symbol.
pub(super) fn radius_field(design: &Design, camera: &Camera) -> TypedText {
    let length = fitting_length(camera, design.units, BLEND_SHARE);
    length_field(length, &Fillet::radius_ask(design), design)
}

impl MotionSession {
    /// The fillet as set up, if it's whole: its edges or faces, and its radius as
    /// it last read, while its field takes it (not dragged to zero).
    pub(super) fn fillet(&self) -> Option<Fillet> {
        let radius = self.field(MotionField::Radius);
        let none = self.blend.edges.refs.is_empty() && self.faces.refs.is_empty();
        if none || radius.error.is_some() {
            return None;
        }
        Some(Fillet {
            edges: self.blend.edges.refs.clone(),
            faces: self.faces.refs.clone(),
            radius: radius.value.clone()?,
            chains: self.blend.stored,
        })
    }

    /// Opens the fillet `fillet` in this session: its edges, faces,
    /// radius and Tangent chain.
    pub(super) fn open_fillet(&mut self, fillet: &Fillet) {
        let ask = Fillet::radius_ask(&self.read_in.design());
        self.fields[MotionField::Radius.index()] = TypedText::of(&fillet.radius, &ask);
        self.blend = BlendSetup::blend_of(&fillet.edges, fillet.chains);
        self.faces = Refs::of(&fillet.faces);
        self.blend_body();
    }
}

impl Doc {
    /// What the panel shows of the fillet being set up.
    pub(super) fn fillet_view(&self, session: &MotionSession) -> FilletView {
        let units = self.editor.document().units();
        FilletView {
            edges: self.blend_edges(session),
            info: (session.fillet())
                .filter(|_| session.kind == MotionKind::Fillet)
                .map(|fillet| {
                    // The tick, which says what clicks pick, or the
                    // feature's stored chains.
                    let chains = fillet.chains || session.blend.chains;
                    varde_view::fillet_info(&Fillet { chains, ..fillet }, units)
                }),
        }
    }
}

/// The feature a fillet session makes.
pub(super) fn fillet_kind(session: &MotionSession) -> Option<FeatureKind> {
    session.fillet().map(FeatureKind::Fillet)
}
