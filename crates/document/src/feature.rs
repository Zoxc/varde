//! Features: the steps a design is built from, sketches, extrudes,
//! revolves, combines, moves and mirrors.

use serde::{Deserialize, Serialize};
use varde_sketch::Sketch;

use crate::{BodyId, Combine, Extrude, Mirror, Move, Operation, Plane, Revolve};

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
    Combine(Combine),
    Move(Move),
    Mirror(Mirror),
}

impl FeatureKind {
    /// What a feature of this kind is called, as in its default name,
    /// "Revolve 2".
    pub fn noun(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude(_) => "Extrude",
            FeatureKind::Revolve(_) => "Revolve",
            FeatureKind::Combine(_) => "Combine",
            FeatureKind::Move(_) => "Move",
            FeatureKind::Mirror(_) => "Mirror",
        }
    }

    /// The features it builds on, which come before it: an extrude's or a
    /// revolve's sketch. Removing one of them removes this too. Sorted,
    /// without repeats. The makers of the bodies it names
    /// ([`FeatureKind::bodies`]) aren't listed, as finding them takes the
    /// document, but [`Document::removal`](crate::Document::removal)
    /// follows them too. Not the maker of the body under a sketch's face
    /// plane ([`Plane::Face`]): removing it leaves the sketch, which then
    /// doesn't regenerate until it's put on another plane.
    pub fn uses(&self) -> Vec<FeatureId> {
        match self.sketch() {
            Some(sketch) => vec![sketch],
            None => Vec::new(),
        }
    }

    /// The bodies it names, which features before it make, and which it
    /// depends on: a combine's target and tools, the bodies a move or a
    /// mirror moves. Removing one of them, or its maker, removes this too.
    /// Not the bodies an extrude or revolve takes out of its targets,
    /// which are dropped from its list instead, nor the body under a
    /// sketch's face plane, a revolve's axis edge or a move's axis or a
    /// mirror's plane (the feature stays, and fails until it's given
    /// another).
    pub fn bodies(&self) -> Vec<BodyId> {
        match self {
            FeatureKind::Combine(combine) => combine.bodies().collect(),
            FeatureKind::Move(moved) => moved.bodies.clone(),
            FeatureKind::Mirror(mirror) => mirror.bodies.clone(),
            FeatureKind::Sketch { .. } | FeatureKind::Extrude(_) | FeatureKind::Revolve(_) => {
                Vec::new()
            }
        }
    }

    /// The sketch whose regions it takes: an extrude's or a revolve's,
    /// which adding it hides.
    pub fn sketch(&self) -> Option<FeatureId> {
        match self {
            FeatureKind::Sketch { .. }
            | FeatureKind::Combine(_)
            | FeatureKind::Move(_)
            | FeatureKind::Mirror(_) => None,
            FeatureKind::Extrude(extrude) => Some(extrude.sketch),
            FeatureKind::Revolve(revolve) => Some(revolve.sketch),
        }
    }

    /// What it does with the solid it makes: an extrude's or a revolve's
    /// operation. A combine has none: it makes no solid of its own.
    pub fn operation(&self) -> Option<&Operation> {
        match self {
            FeatureKind::Sketch { .. }
            | FeatureKind::Combine(_)
            | FeatureKind::Move(_)
            | FeatureKind::Mirror(_) => None,
            FeatureKind::Extrude(extrude) => Some(&extrude.operation),
            FeatureKind::Revolve(revolve) => Some(&revolve.operation),
        }
    }

    /// The same, to change.
    pub(crate) fn operation_mut(&mut self) -> Option<&mut Operation> {
        match self {
            FeatureKind::Sketch { .. }
            | FeatureKind::Combine(_)
            | FeatureKind::Move(_)
            | FeatureKind::Mirror(_) => None,
            FeatureKind::Extrude(extrude) => Some(&mut extrude.operation),
            FeatureKind::Revolve(revolve) => Some(&mut revolve.operation),
        }
    }

    /// The body it makes, for an [`Operation::NewBody`].
    pub fn new_body(&self) -> Option<BodyId> {
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

impl From<Move> for FeatureKind {
    fn from(moved: Move) -> Self {
        FeatureKind::Move(moved)
    }
}

impl From<Mirror> for FeatureKind {
    fn from(mirror: Mirror) -> Self {
        FeatureKind::Mirror(mirror)
    }
}

impl From<Combine> for FeatureKind {
    fn from(combine: Combine) -> Self {
        FeatureKind::Combine(combine)
    }
}
