//! Features: the steps a design is built from, sketches, extrudes and
//! revolves.

use serde::{Deserialize, Serialize};
use varde_sketch::Sketch;

use crate::{Extrude, Operation, Plane, Revolve};

/// A feature's handle in one document. It's opaque: ids come from the
/// document's features, not from literals. Features and bodies take their
/// ids from the same counter, so no feature has a body's number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FeatureId(pub(crate) u64);

impl FeatureId {
    /// Its number, unique in its document among features and bodies: for
    /// naming what the feature makes outside the document, such as the
    /// faces of an extrude's solid.
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    /// Whether it's drawn with the model: a sketch's curves, when it isn't
    /// being edited. An extrude's or revolve's solid is drawn by its
    /// body's visibility.
    pub visible: bool,
    pub kind: FeatureKind,
}

/// What a feature is. Files store a kind by its variant name, so new kinds
/// can go anywhere; a name is never renamed or reused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FeatureKind {
    Sketch { plane: Plane, sketch: Sketch },
    Extrude(Extrude),
    Revolve(Revolve),
}

impl FeatureKind {
    /// What a feature of this kind is called, as in its default name,
    /// "Revolve 2".
    pub fn noun(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude(_) => "Extrude",
            FeatureKind::Revolve(_) => "Revolve",
        }
    }

    /// The features it builds on, which come before it: an extrude's or a
    /// revolve's sketch. Removing one of them removes this too. Sorted,
    /// without repeats. Not the maker of the body under a sketch's face
    /// plane ([`Plane::Face`]): removing it leaves the sketch, which then
    /// doesn't regenerate until it's put on another plane.
    pub fn uses(&self) -> Vec<FeatureId> {
        match self.sketch() {
            Some(sketch) => vec![sketch],
            None => Vec::new(),
        }
    }

    /// The sketch whose regions it takes: an extrude's or a revolve's,
    /// which adding it hides.
    pub fn sketch(&self) -> Option<FeatureId> {
        match self {
            FeatureKind::Sketch { .. } => None,
            FeatureKind::Extrude(extrude) => Some(extrude.sketch),
            FeatureKind::Revolve(revolve) => Some(revolve.sketch),
        }
    }

    /// What it does with the solid it makes: an extrude's or a revolve's
    /// operation.
    pub fn operation(&self) -> Option<&Operation> {
        match self {
            FeatureKind::Sketch { .. } => None,
            FeatureKind::Extrude(extrude) => Some(&extrude.operation),
            FeatureKind::Revolve(revolve) => Some(&revolve.operation),
        }
    }

    /// The same, to change.
    pub(crate) fn operation_mut(&mut self) -> Option<&mut Operation> {
        match self {
            FeatureKind::Sketch { .. } => None,
            FeatureKind::Extrude(extrude) => Some(&mut extrude.operation),
            FeatureKind::Revolve(revolve) => Some(&mut revolve.operation),
        }
    }

    /// The body it makes, for an [`Operation::NewBody`].
    pub fn new_body(&self) -> Option<crate::BodyId> {
        self.operation().and_then(Operation::new_body)
    }
}

impl From<Extrude> for FeatureKind {
    fn from(extrude: Extrude) -> Self {
        FeatureKind::Extrude(extrude)
    }
}

impl From<Revolve> for FeatureKind {
    fn from(revolve: Revolve) -> Self {
        FeatureKind::Revolve(revolve)
    }
}
