//! Features: the steps a design is built from, sketches, extrudes and
//! revolves, and the planes sketches are drawn on.

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use varde_sketch::Sketch;

use crate::{Extrude, Operation, Revolve};

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

/// What a feature is. New kinds are appended: a kind's place in the list
/// is how files store it.
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
    /// without repeats.
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

/// A plane a sketch is drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Plane {
    Origin(OriginPlane),
}

impl Plane {
    /// The plane as the user sees it: "XY".
    pub fn name(self) -> &'static str {
        match self {
            Plane::Origin(plane) => plane.name(),
        }
    }

    /// Where the plane is in the world.
    pub fn placement(self) -> Placement {
        match self {
            Plane::Origin(plane) => plane.placement(),
        }
    }
}

/// One of the planes through the origin spanned by two world axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OriginPlane {
    XY,
    XZ,
    YZ,
}

impl OriginPlane {
    pub const ALL: [OriginPlane; 3] = [OriginPlane::XY, OriginPlane::XZ, OriginPlane::YZ];

    /// The plane as the user sees it: "XY".
    pub fn name(self) -> &'static str {
        match self {
            OriginPlane::XY => "XY",
            OriginPlane::XZ => "XZ",
            OriginPlane::YZ => "YZ",
        }
    }

    /// Its placement. The world is Z-up, and each plane is seen from the
    /// side its normal points to as the render crate's views show it: XY
    /// from the top, XZ from the front and YZ from the right, with the
    /// sketch's x axis to the right and its y axis up.
    pub fn placement(self) -> Placement {
        let (x, y) = match self {
            OriginPlane::XY => (DVec3::X, DVec3::Y),
            OriginPlane::XZ => (DVec3::X, DVec3::Z),
            OriginPlane::YZ => (DVec3::Y, DVec3::Z),
        };
        Placement {
            origin: DVec3::ZERO,
            x,
            y,
            normal: x.cross(y),
        }
    }
}

/// Where a sketch plane is in the world: its origin and unit axes, `x`
/// and `y` in the plane and `normal = x × y` out of it, which is the side
/// it's seen from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub origin: DVec3,
    pub x: DVec3,
    pub y: DVec3,
    pub normal: DVec3,
}

impl Placement {
    /// The world position of the sketch point `at`.
    pub fn to_world(&self, at: DVec2) -> DVec3 {
        self.origin + self.x * at.x + self.y * at.y
    }
}

#[cfg(test)]
mod tests;
