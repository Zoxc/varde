//! The planes sketches are drawn on: an origin plane, or a flat face of a
//! body, and where in the world each is ([`Placement`]).

use std::cmp::Ordering;
use std::fmt;

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use varde_kernel::mesh::FaceKey;

use crate::{BodyId, FeatureId, MAX_COORD};

/// A plane a sketch is drawn on. New kinds are appended: a kind's place
/// in the list is how files store it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Plane {
    Origin(OriginPlane),
    /// A flat face of a body, as the features before the sketch leave
    /// it. Where it is comes from regenerating the history up to the
    /// sketch (the face's form, through [`Placement::on_plane`]): the
    /// document stores the reference, never the placement.
    Face(FaceRef),
}

impl Plane {
    /// The plane as the user sees it: "XY", or "a face".
    pub fn name(self) -> &'static str {
        match self {
            Plane::Origin(plane) => plane.name(),
            Plane::Face(_) => "a face",
        }
    }

    /// Where an origin plane is in the world; `None` for a face, whose
    /// placement only regenerating finds.
    pub fn placement(self) -> Option<Placement> {
        match self {
            Plane::Origin(plane) => Some(plane.placement()),
            Plane::Face(_) => None,
        }
    }

    /// The face it's on, if it's on one.
    pub fn face(&self) -> Option<&FaceRef> {
        match self {
            Plane::Origin(_) => None,
            Plane::Face(face) => Some(face),
        }
    }
}

/// A face of a body, as picked: the body, the face's key (the kernel's
/// name for it, from the feature that made it) and the picked point,
/// which chooses among several faces with the key (see
/// `varde_kernel::topology::Topology::face`). The body may since have been
/// removed, or the face renamed or gone: the reference then doesn't
/// resolve, which regenerating reports, as a region it can't find.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FaceRef {
    pub body: BodyId,
    pub key: FaceKey,
    /// Finite and within [`MAX_COORD`].
    pub near: DVec3,
}

impl FaceRef {
    /// The feature the key names as the face's maker, which may not be
    /// there.
    pub fn maker(&self) -> FeatureId {
        FeatureId(self.key.feature)
    }

    /// The order lists of references are kept in (a shell's open
    /// faces): by body, then the key, then the point's coordinates in
    /// turn ([`f64::total_cmp`]). `Equal` only for the same reference to
    /// the bit, which such a list doesn't repeat.
    pub fn order(&self, other: &FaceRef) -> Ordering {
        (self.body, self.key)
            .cmp(&(other.body, other.key))
            .then_with(|| self.near.x.total_cmp(&other.near.x))
            .then_with(|| self.near.y.total_cmp(&other.near.y))
            .then_with(|| self.near.z.total_cmp(&other.near.z))
    }

    /// Checks what needs only the reference: its point is finite and
    /// within [`MAX_COORD`]. What it names is
    /// [`Document::check`](crate::Document::check)'s.
    pub fn check_own(&self) -> Result<(), PlaneError> {
        let near = self.near;
        if crate::in_bounds(near) {
            Ok(())
        } else {
            Err(PlaneError::Near(near))
        }
    }
}

/// What's wrong with a sketch's plane, see
/// [`CheckError::SketchPlane`](crate::CheckError::SketchPlane).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaneError {
    /// Its face's body, this one, is made by the sketch or a feature
    /// after it, or isn't there and has an id a body made later could
    /// take.
    Body(BodyId),
    /// Its face's key names this feature, which is the sketch or comes
    /// after it, or isn't there and has an id a feature made later could
    /// take.
    Maker(FeatureId),
    /// Its face's point isn't finite, or is further from zero than
    /// [`MAX_COORD`].
    Near(DVec3),
}

impl fmt::Display for PlaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlaneError::Body(body) => write!(
                f,
                "is on a face of body {}, which isn't made before it",
                body.0
            ),
            PlaneError::Maker(feature) => write!(
                f,
                "is on a face made by feature {}, which doesn't come before it",
                feature.0
            ),
            PlaneError::Near(at) => write!(f, "its face's point {at} is out of bounds"),
        }
    }
}

impl std::error::Error for PlaneError {}

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

/// How near to horizontal, as `n̂x² + n̂y²` of its unit normal, a plane
/// is taken as horizontal by [`Placement::on_plane`]: a decision on
/// geometry, stated as one. About a nanoradian of tilt.
const HORIZONTAL: f64 = 1e-18;

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

    /// Whether it's a placement a sketch can be drawn and built on: every
    /// number finite, `x` and `y` unit and square to each other within
    /// [`Placement::SLACK`], `normal` equal to `x × y` within it, and the
    /// origin within [`MAX_COORD`] on each axis. What regeneration
    /// refuses to place a sketch at, and what the page checks a
    /// placement from the regeneration worker against.
    pub fn valid(&self) -> bool {
        let vectors = [self.origin, self.x, self.y, self.normal];
        let near = |a: f64, b: f64| (a - b).abs() <= Self::SLACK;
        vectors.iter().all(|v| v.is_finite())
            && self.origin.abs().max_element() <= f64::from(MAX_COORD)
            && near(self.x.length_squared(), 1.0)
            && near(self.y.length_squared(), 1.0)
            && near(self.x.dot(self.y), 0.0)
            && (self.normal - self.x.cross(self.y)).abs().max_element() <= Self::SLACK
    }

    /// How far from unit, square and `x × y` [`Placement::valid`] lets a
    /// placement's axes be: far more than [`Placement::on_plane`]'s
    /// rounding, far less than anything drawn would show.
    pub const SLACK: f64 = 1e-9;

    /// The placement of a sketch on the plane `n·p = d`, a flat face's
    /// form with `n` out of the solid: one rule, so that regeneration and
    /// the app, calling it on the same bits, get the same bits.
    ///
    /// - The normal is `n̂ = n / |n|`, so the sketch is seen from outside
    ///   and an extrude's positive side grows the body.
    /// - The origin is `n̂ d / |n|`, the plane's point nearest the world
    ///   origin: moving the face along its normal moves the origin with
    ///   it and the drawing nowhere else; the face's size moves nothing.
    /// - On a horizontal plane (`n̂x² + n̂y² ≤ 1e-18`) `x` is world X
    ///   projected into the plane and `y = n̂ × x`; on any other, `y` is
    ///   world Z projected into the plane ("up stays up", as the Z-up
    ///   camera shows a face it looks at) and `x = y × n̂`. The stored
    ///   normal is `x × y`, as an origin plane's.
    ///
    /// A plane parallel to an origin plane and facing its way gets that
    /// plane's axes exactly, to the bit (signed zeros made positive); a
    /// bottom face gets `x` = X and `y` = −Y. Only `+ − × ÷ √` and one
    /// comparison, so the bits are the same natively and on the web.
    /// `None` if `n` is zero or isn't finite, or anything worked out
    /// isn't (`d` among them).
    pub fn on_plane(n: DVec3, d: f64) -> Option<Placement> {
        let length = n.length();
        if !(length > 0.0 && length.is_finite() && d.is_finite()) {
            return None;
        }
        let n = n / length;
        let d = d / length;
        let tilt = n.x * n.x + n.y * n.y;
        let (x, y) = if tilt <= HORIZONTAL {
            // World X less its part along n: (1 − n̂x², −n̂x n̂y, −n̂x n̂z).
            let x = DVec3::new(n.y * n.y + n.z * n.z, 0.0 - n.x * n.y, 0.0 - n.x * n.z);
            let x = x / x.length();
            (x, n.cross(x))
        } else {
            // World Z less its part along n: (−n̂x n̂z, −n̂y n̂z, 1 − n̂z²),
            // the last as n̂x² + n̂y², which doesn't cancel.
            let y = DVec3::new(0.0 - n.x * n.z, 0.0 - n.y * n.z, tilt);
            let y = y / y.length();
            (y.cross(n), y)
        };
        // Adding zero turns −0 into +0 and changes nothing else.
        let positive = |v: DVec3| v + DVec3::ZERO;
        let placement = Placement {
            origin: positive(n * d),
            x: positive(x),
            y: positive(y),
            normal: positive(x.cross(y)),
        };
        let finite = [placement.origin, placement.x, placement.y, placement.normal]
            .iter()
            .all(|v| v.is_finite());
        finite.then_some(placement)
    }
}

#[cfg(test)]
mod tests;
