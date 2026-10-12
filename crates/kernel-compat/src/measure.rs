//! Measuring a solid as built: what the measure tool shows of a picked
//! face, edge, corner or body, and the distance between two picks.
//!
//! A pick ([`Pick`]) is an entity of one solid's topology, a [`Target`]
//! names it with the solid. [`measure`] measures one target;
//! [`distance()`] gives the minimum distance between two and the points
//! where it is reached.
//!
//! The old kernel's measures are gone. Every solid of the facade is
//! empty, so they answer for the empty solid: a body measures nothing
//! (no volume, area, centre or box), its topology has no face, edge or
//! corner to pick ([`MeasureError::NotFound`]), and a distance to its
//! body is [`MeasureError::Empty`]. The Varde kernel's measures take
//! over here.

use glam::DVec3;

use crate::mesh::Form;
use crate::patch::Bounds3;
use crate::topology::{Chain, NotFound};
use crate::{Budget, KernelError, Solid, Tolerance, Topology, trig};

/// What is picked of a solid: an entity of its [`Topology`] by index, or
/// the whole body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// The whole solid.
    Body,
    /// A region (a face as users see it).
    Face(u32),
    /// A chain (an edge).
    Edge(u32),
    /// A corner.
    Corner(u32),
    /// A chain's point ([`EdgeMeasure::point`]): a straight edge's
    /// middle, a round edge's centre. An edge of another shape has none
    /// ([`MeasureError::NotFound`]).
    EdgePoint(u32),
}

/// A pick of a solid: the solid, its topology ([`Solid::topology`]) and
/// what of it is picked.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub solid: &'a Solid,
    pub topology: &'a Topology,
    pub pick: Pick,
}

/// Why a measure has no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureError {
    /// The measure ran out of its budget (or a piece couldn't be split
    /// further): "too complex to measure".
    TooComplex,
    /// The pick names nothing in its topology, or the topology isn't its
    /// solid's.
    NotFound(NotFound),
    /// A distance to a body with nothing in it (the empty solid).
    Empty,
}

impl std::fmt::Display for MeasureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeasureError::TooComplex => f.write_str("too complex to measure"),
            MeasureError::NotFound(e) => e.fmt(f),
            MeasureError::Empty => f.write_str("the body is empty"),
        }
    }
}

impl std::error::Error for MeasureError {}

impl From<KernelError> for MeasureError {
    /// Every kernel error a measure meets is a search that couldn't go
    /// on: out of budget, or a piece too small to split.
    fn from(_: KernelError) -> Self {
        MeasureError::TooComplex
    }
}

/// A direction a pick has: a unit vector, and whether it is only a line
/// (an edge's or an axis's, whose sign means nothing) rather than a
/// plane's normal out of the solid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Direction {
    pub v: DVec3,
    pub line: bool,
}

impl Direction {
    /// `v` scaled to unit length, `None` if it has none.
    fn of(v: DVec3, line: bool) -> Option<Direction> {
        let v = crate::transform::unit(v)?;
        Some(Direction { v, line })
    }
}

/// The angle between two directions in radians: `atan2(|a × b|, a · b)`,
/// in `[0, π]`; where either is only a line, the smaller of the two
/// angles between the lines, in `[0, π/2]`. About an ulp of the angle.
pub fn angle(a: Direction, b: Direction) -> f64 {
    let cross = a.v.cross(b.v).length();
    let dot = a.v.dot(b.v);
    let dot = if a.line || b.line { dot.abs() } else { dot };
    trig::atan2(cross, dot)
}

/// What a pick measures.
#[derive(Debug, Clone, PartialEq)]
pub enum Measured {
    Body(BodyMeasure),
    Face(FaceMeasure),
    Edge(EdgeMeasure),
    /// A point: a corner's, or an edge's ([`Pick::EdgePoint`]).
    Point(DVec3),
}

impl Measured {
    /// The point the pick stands for, where it has one: a corner's, an
    /// edge's (see [`EdgeMeasure::point`]), a body's centre of mass.
    pub fn point(&self) -> Option<DVec3> {
        match self {
            Measured::Body(body) => body.centre,
            Measured::Face(_) => None,
            Measured::Edge(edge) => edge.point(),
            Measured::Point(p) => Some(*p),
        }
    }

    /// The direction the pick has, where it has one (see
    /// [`FaceMeasure::direction`], [`EdgeMeasure::direction`]).
    pub fn direction(&self) -> Option<Direction> {
        match self {
            Measured::Face(face) => face.direction(),
            Measured::Edge(edge) => edge.direction(),
            Measured::Body(_) | Measured::Point(_) => None,
        }
    }
}

/// A body's measures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyMeasure {
    pub volume: f64,
    pub area: f64,
    /// The centre of mass at uniform density; `None` for the empty solid.
    pub centre: Option<DVec3>,
    /// The least box around it (see [`Solid::tight_bounds`]); `None` for
    /// the empty solid.
    pub bounds: Option<Bounds3>,
}

/// A face's measures: its area and its form, what surface it was built to
/// be with its parameters (a fitted face is on it within the fit
/// tolerance).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceMeasure {
    pub area: f64,
    pub form: Form,
    /// For a flat face bounded by four straight edges, each square to the
    /// next (see [`rectangle`]): two sides from one corner.
    pub rectangle: Option<[DVec3; 2]>,
}

impl FaceMeasure {
    /// A plane's normal (out of the solid); a cylinder's, cone's,
    /// torus's or surface of revolution's axis, or the direction a conic
    /// cylinder runs along (lines). `None` for a sphere, a general
    /// quadric (claimed as curved only) and an unknown form.
    pub fn direction(&self) -> Option<Direction> {
        match self.form {
            Form::Plane { n, .. } => Direction::of(n, false),
            Form::Cylinder { axis, .. }
            | Form::Cone { axis, .. }
            | Form::Torus { axis, .. }
            | Form::Revolved { axis, .. } => Direction::of(axis, true),
            Form::ConicCylinder { along, .. } => Direction::of(along, true),
            Form::Sphere { .. } | Form::Quadric(_) | Form::Unknown => None,
        }
    }

    /// A cone's half-angle in radians, from its cosine and sine.
    pub fn half_angle(&self) -> Option<f64> {
        match self.form {
            Form::Cone { cos, sin, .. } => Some(trig::atan2(sin, cos)),
            _ => None,
        }
    }
}

/// What an edge's curves make.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeShape {
    /// A straight edge from `from` to `to`.
    Line { from: DVec3, to: DVec3 },
    /// An arc of the circle (or the whole circle) around `centre` in the
    /// plane square to the unit `axis`, which turns the way the edge runs
    /// (right-handed).
    Circle {
        centre: DVec3,
        axis: DVec3,
        radius: f64,
    },
    /// An arc of the ellipse (or the whole of it) around `centre` in the
    /// plane square to the unit `axis`, with its semi-axes, `major ≥
    /// minor`.
    Ellipse {
        centre: DVec3,
        axis: DVec3,
        major: f64,
        minor: f64,
    },
    /// Anything else: parabolas, hyperbolas, fitted chains, curves of
    /// several conics.
    Other,
}

impl EdgeShape {
    /// A straight edge's middle, a round edge's centre.
    pub fn point(&self) -> Option<DVec3> {
        match *self {
            EdgeShape::Line { from, to } => Some((from + to) * 0.5),
            EdgeShape::Circle { centre, .. } | EdgeShape::Ellipse { centre, .. } => Some(centre),
            EdgeShape::Other => None,
        }
    }
}

/// An edge's measures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeMeasure {
    pub length: f64,
    /// Whether it is a closed loop.
    pub closed: bool,
    pub shape: EdgeShape,
}

impl EdgeMeasure {
    /// A straight edge's middle, a round edge's centre
    /// ([`EdgeShape::point`]).
    pub fn point(&self) -> Option<DVec3> {
        self.shape.point()
    }

    /// A straight edge's line, a round edge's axis.
    pub fn direction(&self) -> Option<Direction> {
        match self.shape {
            EdgeShape::Line { from, to } => Direction::of(to - from, true),
            EdgeShape::Circle { axis, .. } | EdgeShape::Ellipse { axis, .. } => {
                Direction::of(axis, true)
            }
            EdgeShape::Other => None,
        }
    }
}

/// What `target` measures. The empty solid's body measures nothing; a
/// pick of a face, edge or corner names nothing in its (empty) topology,
/// [`MeasureError::NotFound`].
pub fn measure(
    target: &Target<'_>,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Measured, MeasureError> {
    match nothing_in(target) {
        MeasureError::Empty => Ok(Measured::Body(BodyMeasure {
            volume: 0.0,
            area: 0.0,
            centre: None,
            bounds: None,
        })),
        e => Err(e),
    }
}

/// A solid's volume and centre of mass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moments {
    pub volume: f64,
    /// The centre of mass at uniform density; `None` for the empty solid.
    pub centre: Option<DVec3>,
}

impl Solid {
    /// Its volume and centre of mass (uniform density): none for the
    /// empty solid.
    pub fn moments(&self, _budget: &Budget) -> Result<Moments, KernelError> {
        if self.is_empty() {
            Ok(Moments {
                volume: 0.0,
                centre: None,
            })
        } else {
            Err(KernelError::NotImplemented("measuring moments"))
        }
    }

    /// The least box around it: `None` for the empty solid.
    pub fn tight_bounds(
        &self,
        _tol: &Tolerance,
        _budget: &Budget,
    ) -> Result<Option<Bounds3>, KernelError> {
        if self.is_empty() {
            Ok(None)
        } else {
            Err(KernelError::NotImplemented("measuring tight bounds"))
        }
    }
}

/// Two sides from one corner of region `r` of `solid`'s `topology`, if
/// it's a rectangle. The facade finds none.
pub fn rectangle(_solid: &Solid, _topology: &Topology, _r: u32) -> Option<[DVec3; 2]> {
    None
}

/// What `chain`, a chain of `solid`'s topology, makes. The facade reads
/// no curves: [`EdgeShape::Other`].
pub fn edge_shape(_solid: &Solid, _chain: &Chain) -> EdgeShape {
    EdgeShape::Other
}

/// The minimum distance between two picks and where it is reached.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distance {
    /// `|points[1] − points[0]|`.
    pub distance: f64,
    /// A point of the first pick and one of the second, as far apart as
    /// `distance`.
    pub points: [DVec3; 2],
}

/// The minimum distance between `a` and `b` and the two points where it
/// is reached: [`MeasureError::Empty`] for the empty solid's body,
/// [`MeasureError::NotFound`] for a face, edge or corner of its (empty)
/// topology.
pub fn distance(
    a: &Target<'_>,
    _b: &Target<'_>,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Distance, MeasureError> {
    // Every pick of the facade's solids has no point to measure from;
    // `a`'s says why first.
    Err(nothing_in(a))
}

/// Why `target` has nothing to measure: the empty solid's body is empty,
/// its topology has no face, edge or corner.
fn nothing_in(target: &Target<'_>) -> MeasureError {
    match target.pick {
        Pick::Body if target.solid.is_empty() => MeasureError::Empty,
        // No non-empty solid exists in the facade.
        Pick::Body => MeasureError::TooComplex,
        Pick::Face(_) => MeasureError::NotFound(NotFound::Face),
        Pick::Edge(_) | Pick::EdgePoint(_) => MeasureError::NotFound(NotFound::Edge),
        Pick::Corner(_) => MeasureError::NotFound(NotFound::Corner),
    }
}
