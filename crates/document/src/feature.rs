//! Features: the steps a design is built from, today only sketches, and
//! the planes they're drawn on.

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use varde_sketch::Sketch;

/// A feature's handle in one document. It's opaque: ids come from the
/// document's features, not from literals. Features and bodies take their
/// ids from the same counter, so no feature has a body's number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FeatureId(pub(crate) u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    /// Whether it's drawn with the model: a sketch's curves, when it isn't
    /// being edited.
    pub visible: bool,
    pub kind: FeatureKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FeatureKind {
    Sketch { plane: Plane, sketch: Sketch },
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
