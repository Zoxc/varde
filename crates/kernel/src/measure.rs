//! Measuring a solid as built: what the measure tool shows of a picked
//! face, edge, corner or body.
//!
//! - **Lengths** of chains ([`Topology`]'s edges), curve by curve: a
//!   straight one is the distance between its ends; a circular arc is
//!   `r·θ` (the circle by `circle_of`, `θ/2` by `atan2` of the control
//!   point's rise over the half chord); any other conic is integrated by
//!   8-point Gauss–Legendre over pieces halved until their weights are
//!   within `0.97..=1.03` and their control legs turn at most 45° and
//!   are within a factor 1.2 of each other's length (on random conics of
//!   every weight, within `2e-14` of a fine adaptive rule; weights alone
//!   left `1e-7`, a nearly half ellipse's halves being lopsided).
//! - **Areas, volumes and the centre of mass** by [`Solid::area`]'s rule
//!   (8 × 8 Gauss–Legendre on each quarter of a patch, weights brought
//!   within `0.7..=1.4` first), over a region's patches for a face. The
//!   centre of mass (uniform density) is `∫ x_i dV = ½ ∮ x_i² n_i dA`,
//!   measured from the middle of the solid's box.
//! - **The tight box**: the vertices, each edge's extremes along the
//!   axes (a conic's are a quadratic's roots: exact to rounding), and the
//!   insides of curved patches by a bounded search (control hulls as upper
//!   bounds, points of the patch as lower ones, Newton's method at the
//!   end) to the resolution. Patches on planes and ruled quadrics
//!   (cylinders and cones, checked to the resolution) have their extremes
//!   on their boundaries and aren't searched.
//! - **Forms, points and directions**: a face's kind and parameters are
//!   its [`Form`] (fitted faces are on theirs within the fit tolerance);
//!   an edge's shape (a line, a circle, an ellipse) is read off its
//!   curves; angles between directions are `atan2(|a × b|, a · b)`.
//!
//! Only `+ − × ÷ √` and [`trig`]. Integrals are summed in patch order and
//! every search is sequential, so the results are the same bits at any
//! thread count. Everything is charged to a [`Budget`]: past it,
//! [`MeasureError::TooComplex`].
//!
//! A pick ([`Pick`]) is an entity of one solid's topology, a [`Target`]
//! names it with the solid; minimum distances between two targets are to
//! come beside [`measure`].

use std::ops::{Add, RangeInclusive};

use glam::DVec3;

use crate::budget::{Budget, Work};
use crate::mesh::{Face, Form, Mesh, Surface, circle_of};
use crate::par::par_map;
use crate::patch::{Bounds3, Conic3, Patch};
use crate::quadrature::{GAUSS8, triangle_rule};
use crate::solid::{INTEGRATE_WORK, piece_count, pieces};
use crate::topology::NotFound;
use crate::topology::distance::{blossom_point, quarters};
use crate::{KernelError, MAX_REFINE_DEPTH, Solid, Tolerance, Topology, trig};

/// The weights within which a curve's piece is integrated as it is.
const CURVE_SHAPED: RangeInclusive<f64> = 0.97..=1.03;

/// The cosine of the most a curve piece's control legs may turn for it
/// to be integrated as it is: 45°.
const CURVE_TURN_COS: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// How much longer one control leg of a curve piece may be than the
/// other for it to be integrated as it is.
const CURVE_LEGS: f64 = 1.2;

/// How often a curve may be halved for its length. Weights reach
/// [`CURVE_SHAPED`] within six halvings from the weight bounds, and the
/// legs' turn and their lengths' ratio go to 0 and 1 as pieces shrink;
/// past this, a curve with a turn too tight for it is too complex.
const MAX_CURVE_DEPTH: u32 = 24;

/// The work of one curve piece's 8 points, in units of about half a
/// microsecond.
const CURVE_WORK: usize = 1;

/// The work of a pass over four edges for their extremes along the axes.
const EDGES_PER_UNIT: usize = 4;

/// The work of one visit of the box search: a piece's six control points
/// by blossoming, and one point of it.
const BOX_WORK: usize = 1;

/// The work of polishing an extreme by Newton's method: up to
/// [`NEWTON_STEPS`] steps of five evaluations.
const NEWTON_WORK: usize = 16;

/// Newton steps polishing an extreme inside a patch.
const NEWTON_STEPS: usize = 16;

/// How many times one patch's box search tries Newton's method, and how
/// deep a piece improving the best must be for a try from it (a quarter
/// of the patch across; the first try is from the patch's middle).
const NEWTON_TRIES: u32 = 4;
const NEWTON_DEPTH: u32 = 2;

/// How far apart, relative to an edge's size, its curves may be from one
/// line, circle or ellipse for the edge to be named one (with a few
/// roundings of its coordinates more): it names intent, as
/// [`circle_of`] does, and nothing is decided by it.
const SHAPE_SLACK: f64 = 1e-9;

/// The six directions a box is the extremes along: `+x, −x, +y, −y, +z,
/// −z`.
const DIRECTIONS: [DVec3; 6] = [
    DVec3::X,
    DVec3::NEG_X,
    DVec3::Y,
    DVec3::NEG_Y,
    DVec3::Z,
    DVec3::NEG_Z,
];

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
}

impl std::fmt::Display for MeasureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeasureError::TooComplex => f.write_str("too complex to measure"),
            MeasureError::NotFound(e) => e.fmt(f),
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
        let v = v.try_normalize()?;
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
    /// A corner's point.
    Corner(DVec3),
}

impl Measured {
    /// The point the pick stands for, where it has one: a corner's, an
    /// edge's (see [`EdgeMeasure::point`]), a body's centre of mass.
    pub fn point(&self) -> Option<DVec3> {
        match self {
            Measured::Body(body) => body.centre,
            Measured::Face(_) => None,
            Measured::Edge(edge) => edge.point(),
            Measured::Corner(p) => Some(*p),
        }
    }

    /// The direction the pick has, where it has one (see
    /// [`FaceMeasure::direction`], [`EdgeMeasure::direction`]).
    pub fn direction(&self) -> Option<Direction> {
        match self {
            Measured::Face(face) => face.direction(),
            Measured::Edge(edge) => edge.direction(),
            Measured::Body(_) | Measured::Corner(_) => None,
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

/// An edge's measures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeMeasure {
    pub length: f64,
    /// Whether it is a closed loop.
    pub closed: bool,
    pub shape: EdgeShape,
}

impl EdgeMeasure {
    /// A straight edge's middle, a round edge's centre.
    pub fn point(&self) -> Option<DVec3> {
        match self.shape {
            EdgeShape::Line { from, to } => Some((from + to) * 0.5),
            EdgeShape::Circle { centre, .. } | EdgeShape::Ellipse { centre, .. } => Some(centre),
            EdgeShape::Other => None,
        }
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

/// What `target` measures, within `budget`; the box's search goes to
/// `tol`'s resolution. A pick naming nothing in its topology (or a
/// topology of another solid) is [`MeasureError::NotFound`].
pub fn measure(
    target: &Target<'_>,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Measured, MeasureError> {
    let (solid, topology) = (target.solid, target.topology);
    let mesh = solid.mesh();
    let tris = mesh.tris().len();
    let mut work = Work::new(budget);
    match target.pick {
        Pick::Body => Ok(Measured::Body(body(solid, tol, &mut work)?)),
        Pick::Face(r) => {
            let region = topology
                .regions()
                .get(r as usize)
                .filter(|region| region.tris.iter().all(|&t| (t as usize) < tris))
                .filter(|region| !region.tris.is_empty())
                .ok_or(MeasureError::NotFound(NotFound::Face))?;
            // Only the area is read, which doesn't depend on the origin.
            let area = integrate(mesh, &region.tris, DVec3::ZERO, &mut work)?.area;
            let face = mesh.faces()[mesh.tris()[region.tris[0] as usize].face as usize];
            Ok(Measured::Face(FaceMeasure {
                area,
                form: face.form,
            }))
        }
        Pick::Edge(c) => {
            let chain = topology
                .chains()
                .get(c as usize)
                .filter(|chain| chain.halfedges.iter().all(|&h| (h as usize) / 3 < tris))
                .filter(|chain| !chain.halfedges.is_empty())
                .ok_or(MeasureError::NotFound(NotFound::Edge))?;
            let curves: Vec<Conic3> = chain.halfedges.iter().map(|&h| mesh.curve(h)).collect();
            Ok(Measured::Edge(EdgeMeasure {
                length: chain_length(&curves, &mut work)?,
                closed: chain.closed,
                shape: edge_shape(&curves),
            }))
        }
        Pick::Corner(c) => {
            let corner = topology
                .corners()
                .get(c as usize)
                .filter(|corner| (corner.vertex as usize) < mesh.verts().len())
                .ok_or(MeasureError::NotFound(NotFound::Corner))?;
            Ok(Measured::Corner(mesh.verts()[corner.vertex as usize]))
        }
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
    /// Its volume and centre of mass (uniform density), within `budget`:
    /// [`Solid::volume`]'s rule, the centre by `∫ x_i dV = ½ ∮ x_i² n_i
    /// dA` over the same points, measured from the middle of its box.
    /// [`KernelError::TooComplex`] past the budget.
    pub fn moments(&self, budget: &Budget) -> Result<Moments, KernelError> {
        let mut work = Work::new(budget);
        let (integrals, o) = self.integrals(&mut work)?;
        Ok(Moments {
            volume: integrals.volume,
            centre: integrals.centre(o),
        })
    }

    /// The least box around it, within `budget` (`None` for the empty
    /// solid): its vertices, its edges' extremes along the axes (exact to
    /// rounding) and the extremes inside its curved patches, found by a
    /// bounded search to `tol`'s resolution with Newton's method. Within the box of its control points ([`Solid::bounds3`]),
    /// and as tight as the resolution: every side is a point of the solid
    /// or within the resolution of one. [`KernelError::TooComplex`] past
    /// the budget.
    pub fn tight_bounds(
        &self,
        tol: &Tolerance,
        budget: &Budget,
    ) -> Result<Option<Bounds3>, KernelError> {
        tight_bounds(self.mesh(), tol.resolution(), &mut Work::new(budget))
    }

    /// The integrals over all its patches, and the point the moments are
    /// measured from (the middle of its box).
    fn integrals(&self, work: &mut Work) -> Result<(Integrals, DVec3), KernelError> {
        let Some(bounds) = self.bounds3() else {
            return Ok((Integrals::default(), DVec3::ZERO));
        };
        let o = middle(&bounds);
        let tris: Vec<u32> = (0..self.mesh().tris().len() as u32).collect();
        Ok((integrate(self.mesh(), &tris, o, work)?, o))
    }
}

/// A body's volume, area, centre and tight box.
fn body(solid: &Solid, tol: &Tolerance, work: &mut Work) -> Result<BodyMeasure, KernelError> {
    let (integrals, o) = solid.integrals(work)?;
    Ok(BodyMeasure {
        volume: integrals.volume,
        area: integrals.area,
        centre: integrals.centre(o),
        bounds: tight_bounds(solid.mesh(), tol.resolution(), work)?,
    })
}

fn middle(b: &Bounds3) -> DVec3 {
    (b.min + b.max) * 0.5
}

/// Integrals over patches: the area, the volume (a third of `∫ (P − o)·n`)
/// and the first moments about `o` (`½ ∫ (P − o)_i² n_i`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Integrals {
    area: f64,
    volume: f64,
    moment: DVec3,
}

impl Integrals {
    /// The centre of mass, the moments having been measured from `o`;
    /// `None` without a volume.
    fn centre(&self, o: DVec3) -> Option<DVec3> {
        let centre = o + self.moment / self.volume;
        (self.volume > 0.0 && centre.is_finite()).then_some(centre)
    }
}

impl Add for Integrals {
    type Output = Integrals;
    fn add(self, other: Integrals) -> Integrals {
        Integrals {
            area: self.area + other.area,
            volume: self.volume + other.volume,
            moment: self.moment + other.moment,
        }
    }
}

impl std::iter::Sum for Integrals {
    fn sum<I: Iterator<Item = Integrals>>(iter: I) -> Integrals {
        iter.fold(Integrals::default(), Add::add)
    }
}

/// [`Integrals`] of one patch (piece) by the triangle rule.
fn patch_integrals(patch: &Patch, o: DVec3) -> Integrals {
    let mut sum = Integrals::default();
    for (u, w) in triangle_rule() {
        let [p, pu, pv] = patch.eval_derivs(u);
        let n = pu.cross(pv);
        let x = p - o;
        sum.area += w * n.length();
        sum.volume += w * x.dot(n);
        sum.moment += x * x * n * (w * 0.5);
    }
    sum.volume /= 3.0;
    sum
}

/// [`Integrals`] over the triangles `tris` of `mesh`, about `o`, split
/// where their weights ask ([`pieces`]), summed in the order given. The
/// pieces are counted first (a unit a triangle) and charged
/// [`INTEGRATE_WORK`] each before any is integrated.
fn integrate(
    mesh: &Mesh,
    tris: &[u32],
    o: DVec3,
    work: &mut Work,
) -> Result<Integrals, KernelError> {
    work.spend(tris.len())?;
    let counts = par_map(tris, |&t| piece_count(&mesh.patch(t as usize), 0));
    let total = counts.iter().fold(0usize, |a, &b| a.saturating_add(b));
    work.spend(total.saturating_mul(INTEGRATE_WORK))?;
    let parts = par_map(tris, |&t| {
        pieces(&mesh.patch(t as usize), 0, &|piece| {
            patch_integrals(piece, o)
        })
    });
    Ok(parts.into_iter().sum())
}

/// The length of a chain of curves, added in order.
pub(crate) fn chain_length(curves: &[Conic3], work: &mut Work) -> Result<f64, KernelError> {
    let mut length = 0.0;
    for curve in curves {
        length += curve_length(curve, work)?;
    }
    Ok(length)
}

/// The length of `curve`: straight, the distance between its ends; a
/// circular arc, `r·θ`; any other conic by 8-point Gauss–Legendre over
/// pieces halved until even ([`even`]), each piece charged
/// [`CURVE_WORK`].
pub(crate) fn curve_length(curve: &Conic3, work: &mut Work) -> Result<f64, KernelError> {
    let chord = curve.p1 - curve.p0;
    let half = chord.length() * 0.5;
    if on_chord(curve) {
        return Ok(half * 2.0);
    }
    work.spend(CURVE_WORK)?;
    if let Some((_, radius)) = circle_of(curve) {
        // The angle between the chord and the tangent at an end is half
        // the arc's: its tangent the control point's rise over the half
        // chord.
        let rise = (curve.c - (curve.p0 + curve.p1) * 0.5).length();
        return Ok(2.0 * radius * trig::atan2(rise, half));
    }
    gauss_length(curve, 0, work)
}

/// Whether `curve` runs straight from end to end: its control point on
/// the segment between its ends, to a few roundings of its coordinates.
/// The curve then lies on that segment, running one way along it (its
/// position along the segment has a derivative of one sign).
fn on_chord(curve: &Conic3) -> bool {
    let chord = curve.p1 - curve.p0;
    let to_c = curve.c - curve.p0;
    let coords = curve
        .p0
        .abs()
        .max(curve.c.abs())
        .max(curve.p1.abs())
        .max_element();
    let slack = 64.0 * f64::EPSILON * coords;
    let length2 = chord.length_squared();
    if !(length2 > 0.0 && length2.is_finite()) {
        // Both ends at one point: straight only if the control point is
        // there too.
        return to_c.length() <= slack;
    }
    let along = to_c.dot(chord) / length2;
    let across = (to_c - chord * along).length();
    across <= slack && along * length2.sqrt() >= -slack && (along - 1.0) * length2.sqrt() <= slack
}

/// The length of `curve` by Gauss–Legendre, halving it until each piece
/// is [`even`].
fn gauss_length(curve: &Conic3, depth: u32, work: &mut Work) -> Result<f64, KernelError> {
    if even(curve) {
        return Ok(GAUSS8
            .iter()
            .map(|&(t, w)| w * curve.eval_deriv(t).1.length())
            .sum());
    }
    if depth >= MAX_CURVE_DEPTH {
        return Err(KernelError::TooComplex);
    }
    let [a, b] = curve.split_half()?;
    work.spend(CURVE_WORK)?;
    Ok(gauss_length(&a, depth + 1, work)? + gauss_length(&b, depth + 1, work)?)
}

/// Whether a curve piece is even enough for 8-point Gauss–Legendre to
/// follow its speed: its weight within [`CURVE_SHAPED`], its control
/// legs turning at most 45° and neither longer than [`CURVE_LEGS`] times
/// the other.
fn even(curve: &Conic3) -> bool {
    let (a, b) = (curve.c - curve.p0, curve.p1 - curve.c);
    let (la, lb) = (a.length(), b.length());
    CURVE_SHAPED.contains(&curve.w)
        && a.dot(b) >= CURVE_TURN_COS * la * lb
        && la <= CURVE_LEGS * lb
        && lb <= CURVE_LEGS * la
}

/// What `curves`, a chain end to end, make: a line, a circle or an
/// ellipse within [`SHAPE_SLACK`] of their size, else
/// [`EdgeShape::Other`].
fn edge_shape(curves: &[Conic3]) -> EdgeShape {
    let (Some(first), Some(last)) = (curves.first(), curves.last()) else {
        return EdgeShape::Other;
    };
    let points: Vec<DVec3> = curves.iter().flat_map(|c| [c.p0, c.c, c.p1]).collect();
    let Some(bounds) = Bounds3::around(&points) else {
        return EdgeShape::Other;
    };
    let size = (bounds.max - bounds.min).length();
    let coords = bounds.min.abs().max(bounds.max.abs()).max_element();
    let slack = SHAPE_SLACK * size + 64.0 * f64::EPSILON * coords;
    if !(size > 0.0 && size.is_finite()) {
        return EdgeShape::Other;
    }

    // A line: every curve straight, every point on the line through the
    // chain's ends.
    let (from, to) = (first.p0, last.p1);
    if let Some(dir) = (to - from).try_normalize()
        && curves.iter().all(on_chord)
        && points.iter().all(|&p| {
            let d = p - from;
            (d - dir * d.dot(dir)).length() <= slack
        })
    {
        return EdgeShape::Line { from, to };
    }

    // A circle or an ellipse: from the curve turning the most (the least
    // weight), and five points of each curve on it (five points fix a
    // conic, so each curve is then an arc of it).
    let Some(reference) = curves.iter().min_by(|a, b| a.w.total_cmp(&b.w)) else {
        return EdgeShape::Other;
    };
    let axis = curves
        .iter()
        .fold(DVec3::ZERO, |n, c| n + (c.c - c.p0).cross(c.p1 - c.c));
    let Some(axis) = axis.try_normalize() else {
        return EdgeShape::Other;
    };
    let fits = |on: &dyn Fn(DVec3) -> bool, centre: DVec3| {
        curves.iter().all(|c| {
            [0.0, 0.25, 0.5, 0.75, 1.0].iter().all(|&t| {
                let p = c.eval(t);
                (p - centre).dot(axis).abs() <= slack && on(p)
            })
        })
    };
    if let Some((centre, radius)) = circle_of(reference)
        && fits(&|p| ((p - centre).length() - radius).abs() <= slack, centre)
    {
        return EdgeShape::Circle {
            centre,
            axis,
            radius,
        };
    }
    if let Some(ellipse) = Ellipse::of(reference)
        && fits(&|p| ellipse.off(p) <= slack, ellipse.centre)
    {
        return EdgeShape::Ellipse {
            centre: ellipse.centre,
            axis,
            major: ellipse.major,
            minor: ellipse.minor,
        };
    }
    EdgeShape::Other
}

/// The ellipse a conic of weight under 1 is an arc of, by its conjugate
/// semi-diameters.
struct Ellipse {
    centre: DVec3,
    /// Conjugate semi-diameters: to the arc's middle, and along its chord.
    f: [DVec3; 2],
    major: f64,
    minor: f64,
}

impl Ellipse {
    /// The least `1 − w²` taken: an arc so flat that its ellipse is
    /// lost in rounding is named [`EdgeShape::Other`].
    const MIN_FLATNESS: f64 = 1e-6;

    /// The ellipse `conic` is an arc of, if its weight `w` is under 1.
    /// An affine image of a circle's arc of half-angle `φ`, `w = cos φ`:
    /// the centre is `c + (m − c)/(1 − w²)` (`m` the chord's middle); the
    /// semi-diameter to the arc's middle is `(c − O)·w`, the one along
    /// the chord, conjugate to it, `(p1 − p0)/(2√(1 − w²))`. With `S` the
    /// sum of their squares and `P` the area of the parallelogram they
    /// span, the semi-axes are `(√(S + 2P) ± √(S − 2P))/2`.
    fn of(conic: &Conic3) -> Option<Ellipse> {
        let w = conic.w;
        let k = 1.0 - w * w;
        if k.is_nan() || k < Self::MIN_FLATNESS {
            return None;
        }
        let m = (conic.p0 + conic.p1) * 0.5;
        let centre = conic.c + (m - conic.c) / k;
        let f = [
            (conic.c - centre) * w,
            (conic.p1 - conic.p0) / (2.0 * k.sqrt()),
        ];
        let s = f[0].length_squared() + f[1].length_squared();
        let p = f[0].cross(f[1]).length();
        let (big, small) = ((s + 2.0 * p).sqrt(), (s - 2.0 * p).max(0.0).sqrt());
        let ellipse = Ellipse {
            centre,
            f,
            major: (big + small) * 0.5,
            minor: (big - small) * 0.5,
        };
        (centre.is_finite() && ellipse.minor > 0.0 && ellipse.major.is_finite()).then_some(ellipse)
    }

    /// About how far `p`, in the ellipse's plane, is from it: `p − O` in
    /// the semi-diameters' coordinates `(α, β)`, and how far `√(α² + β²)`
    /// is from 1, times the major semi-axis.
    fn off(&self, p: DVec3) -> f64 {
        let d = p - self.centre;
        let [f0, f1] = self.f;
        let (a, b, c) = (f0.dot(f0), f0.dot(f1), f1.dot(f1));
        let (r0, r1) = (f0.dot(d), f1.dot(d));
        let det = a * c - b * b;
        let (alpha, beta) = ((c * r0 - b * r1) / det, (a * r1 - b * r0) / det);
        let off = ((alpha * alpha + beta * beta).sqrt() - 1.0).abs() * self.major;
        if off.is_finite() { off } else { f64::INFINITY }
    }
}

/// Whether a face's extremes along any direction lie on its patches'
/// boundaries, to the resolution: a plane, or a cylinder or cone (ruled:
/// a coordinate is linear along each ruling, which runs on to a
/// boundary), its patches checked to lie on it within the resolution
/// (its tag).
fn ruled(face: &Face) -> bool {
    match face.surface {
        Surface::Plane { .. } => true,
        Surface::Quadric(_) => matches!(
            face.form,
            Form::Cylinder { .. } | Form::ConicCylinder { .. } | Form::Cone { .. }
        ),
        Surface::Free => false,
    }
}

/// The least box around `mesh` (see [`Solid::tight_bounds`]), the
/// insides of curved patches searched to `eps`.
pub(crate) fn tight_bounds(
    mesh: &Mesh,
    eps: f64,
    work: &mut Work,
) -> Result<Option<Bounds3>, KernelError> {
    let Some(corners) = Bounds3::around(mesh.verts()) else {
        return Ok(None);
    };
    // The greatest of `d·x` found so far, for each direction `d`.
    let mut best = DIRECTIONS.map(|d| {
        if d.max_element() > 0.0 {
            corners.max.dot(d)
        } else {
            corners.min.dot(d)
        }
    });
    let include = |best: &mut [f64; 6], p: DVec3| {
        for (b, d) in best.iter_mut().zip(DIRECTIONS) {
            *b = b.max(p.dot(d));
        }
    };

    // Each edge once, by its lower halfedge.
    let halfedges = mesh.tris().len() * 3;
    work.spend(halfedges / 2 / EDGES_PER_UNIT)?;
    for h in 0..halfedges as u32 {
        if mesh.halfedge(h).pair < h {
            continue;
        }
        let curve = mesh.curve(h);
        for k in 0..3 {
            for t in turns(&curve, k) {
                include(&mut best, curve.eval(t));
            }
        }
    }

    // The insides of curved patches, patch by patch in order.
    work.spend(mesh.tris().len())?;
    for (t, tri) in mesh.tris().iter().enumerate() {
        if ruled(&mesh.faces()[tri.face as usize]) {
            continue;
        }
        let patch = mesh.patch(t);
        let hull = patch.hull();
        for (i, d) in DIRECTIONS.into_iter().enumerate() {
            let top = hull
                .iter()
                .map(|p| p.dot(d))
                .fold(f64::NEG_INFINITY, f64::max);
            if top <= best[i] + eps {
                continue;
            }
            if let Some(value) = patch_extreme(&patch, d, best[i], eps, work)? {
                best[i] = value;
            }
        }
    }

    let bounds = Bounds3 {
        min: DVec3::new(-best[1], -best[3], -best[5]),
        max: DVec3::new(best[0], best[2], best[4]),
    };
    Ok(Some(bounds))
}

/// The parameters in `(0, 1)` where `curve`'s coordinate `k` turns: the
/// roots of the numerator of its derivative. With the homogeneous
/// weights `(1, w, 1)` and the coordinates `x0, c, x1`, that numerator
/// is twice `w(c − x0)(1 − t)² + (x1 − x0)·t(1 − t) + w(x1 − c)·t²`;
/// with `s = t/(1 − t)` it is the quadratic `w(x1 − c)s² + (x1 − x0)s +
/// w(c − x0)`, whose positive roots give `t = s/(1 + s)`.
fn turns(curve: &Conic3, k: usize) -> impl Iterator<Item = f64> {
    let (x0, c, x1, w) = (curve.p0[k], curve.c[k], curve.p1[k], curve.w);
    let (qa, qb, qc) = (w * (x1 - c), x1 - x0, w * (c - x0));
    let roots: [Option<f64>; 2] = if qa == 0.0 {
        [(qb != 0.0).then(|| -qc / qb), None]
    } else {
        let disc = qb * qb - 4.0 * qa * qc;
        if disc < 0.0 {
            [None, None]
        } else {
            // The stable pair: no cancellation in either root.
            let q = -0.5 * (qb + disc.sqrt().copysign(qb));
            [Some(q / qa), (q != 0.0).then(|| qc / q)]
        }
    };
    roots
        .into_iter()
        .flatten()
        .filter(|s| *s > 0.0 && s.is_finite())
        .map(|s| s / (1.0 + s))
        .filter(|t| *t > 0.0 && *t < 1.0)
}

/// The greatest `d·x` over `patch`'s inside, if more than `floor`: a
/// depth-first search over its pieces (by blossoming, in a fixed
/// order), each piece's control points bounding it above and its middle
/// point below, dropping the pieces that can't come more than `eps`
/// above the best so far, and Newton's method from the patch's middle
/// and from pieces that find a new best (at most [`NEWTON_TRIES`]
/// times). Returns the value of the best point found, which is within
/// `eps` of the greatest. Each visit costs
/// [`BOX_WORK`]; a piece at [`MAX_REFINE_DEPTH`] still open is
/// [`KernelError::TooComplex`].
fn patch_extreme(
    patch: &Patch,
    d: DVec3,
    floor: f64,
    eps: f64,
    work: &mut Work,
) -> Result<Option<f64>, KernelError> {
    let mut best = floor;
    let mut found = None;
    let mut tries = 0;
    let mut stack: Vec<([DVec3; 3], u32)> = vec![(DVec3::AXES, 0)];
    while let Some((dom, depth)) = stack.pop() {
        work.spend(BOX_WORK)?;
        let top = [
            (dom[0], dom[0]),
            (dom[1], dom[1]),
            (dom[2], dom[2]),
            (dom[0], dom[1]),
            (dom[1], dom[2]),
            (dom[2], dom[0]),
        ]
        .iter()
        .map(|&(a, b)| blossom_point(patch, a, b).dot(d))
        .fold(f64::NEG_INFINITY, f64::max);
        if top.is_nan() || top <= best + eps {
            continue;
        }
        let u = (dom[0] + dom[1] + dom[2]) / 3.0;
        let value = patch.eval(u).dot(d);
        let improved = value > best;
        if improved {
            best = value;
            found = Some(value);
        }
        // Newton's method from the patch's middle, and from new bests of
        // small enough pieces: it lifts the best to the extreme near
        // them at once, so the pieces round it are dropped as soon as
        // their hulls come within `eps` of it.
        if (depth == 0 || (improved && depth >= NEWTON_DEPTH)) && tries < NEWTON_TRIES {
            tries += 1;
            work.spend(NEWTON_WORK)?;
            if let Some(p) = newton_extreme(patch, d, u)
                && p.dot(d) > best
            {
                best = p.dot(d);
                found = Some(best);
            }
        }
        if top.is_nan() || top <= best + eps {
            continue;
        }
        if depth >= MAX_REFINE_DEPTH {
            return Err(KernelError::TooComplex);
        }
        // Pushed last to first, so visited first to last.
        for q in quarters(dom).into_iter().rev() {
            stack.push((q, depth + 1));
        }
    }
    Ok(found)
}

/// The point where `d·x` is greatest on `patch` near the parameters `u`
/// by Newton's method (second derivatives by central differences): the
/// point of the patch it reaches, or `None` where the steps end outside
/// the patch or meet no maximum.
fn newton_extreme(patch: &Patch, d: DVec3, mut u: DVec3) -> Option<DVec3> {
    /// The step of the central differences, in the domain.
    const H: f64 = 1e-5;
    let gradient = |u: DVec3| {
        let [_, pu, pv] = patch.eval_derivs(u);
        (pu.dot(d), pv.dot(d))
    };
    let (along_u, along_v) = (DVec3::new(H, 0.0, -H), DVec3::new(0.0, H, -H));
    for _ in 0..NEWTON_STEPS {
        let (e, f) = gradient(u);
        let (uu1, uv1) = gradient(u + along_u);
        let (uu0, uv0) = gradient(u - along_u);
        let (_, vv1) = gradient(u + along_v);
        let (_, vv0) = gradient(u - along_v);
        let (a, b, c) = (
            (uu1 - uu0) / (2.0 * H),
            (uv1 - uv0) / (2.0 * H),
            (vv1 - vv0) / (2.0 * H),
        );
        // A maximum's Hessian is negative definite.
        let det = a * c - b * b;
        if !(a < 0.0 && det > 0.0 && det.is_finite()) {
            return None;
        }
        let (s, t) = ((b * f - c * e) / det, (b * e - a * f) / det);
        let next = DVec3::new(u.x + s, u.y + t, u.z - s - t);
        // Steps may pass outside the patch on the way (its polynomials
        // go on there) but not wander off.
        if !next.is_finite() || next.min_element() < -1.0 {
            return None;
        }
        u = next;
        if s.abs().max(t.abs()) <= 1e-15 {
            break;
        }
    }
    let p = patch.eval(u);
    (u.min_element() >= 0.0 && p.is_finite()).then_some(p)
}

#[cfg(test)]
mod tests;
