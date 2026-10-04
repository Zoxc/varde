//! A fillet in the move's session ([`MotionKind::Fillet`]): its edges
//! picked as a blend's (see `blend`), its radius and the Tangent chain
//! tick, the model mock's fillet panel.

use varde_document::{Design, FeatureKind, Fillet};
use varde_expr::Value;
use varde_view::{FilletView, MotionField, MotionKind};

use super::blend::BlendSetup;
use super::{Doc, MotionSession};
use crate::doc::regions::TypedText;

/// The fillet's radius field as a new one opens it, the UI mock's: 2 of
/// the design's units, with their symbol.
pub(super) fn radius_field(design: &Design) -> TypedText {
    let ask = Fillet::radius_ask(design);
    Value::new("2", &ask).map_or_else(
        |_| TypedText::read("2".to_owned(), &ask),
        |value| TypedText::of(&value, &ask),
    )
}

impl MotionSession {
    /// The fillet as set up, if it's whole: its edges, and its radius as
    /// it last read.
    pub(super) fn fillet(&self) -> Option<Fillet> {
        if self.blend.edges.refs.is_empty() {
            return None;
        }
        Some(Fillet {
            edges: self.blend.edges.refs.clone(),
            radius: self.field(MotionField::Radius).value.clone()?,
            chains: self.blend.chains,
        })
    }

    /// Opens the fillet `fillet` in this session: its edges, radius and
    /// Tangent chain.
    pub(super) fn open_fillet(&mut self, fillet: &Fillet) {
        let ask = Fillet::radius_ask(&self.design);
        self.fields[MotionField::Radius.index()] = TypedText::of(&fillet.radius, &ask);
        self.blend = BlendSetup::of(&fillet.edges, fillet.chains);
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
                .map(|fillet| varde_view::fillet_info(&fillet, units)),
        }
    }
}

/// The feature a fillet session makes.
pub(super) fn fillet_kind(session: &MotionSession) -> Option<FeatureKind> {
    session.fillet().map(FeatureKind::Fillet)
}
