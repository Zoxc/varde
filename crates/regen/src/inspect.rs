//! Measuring picks of the model: what the measure tool asks with a
//! regeneration ([`Inspect`]) and what comes back with the model
//! ([`Inspected`]).
//!
//! A pick ([`InspectPick`]) names a body and, for a face, an edge, an
//! edge's point or a corner, the keys a reference would store and the
//! point the user picked it at. It's resolved on the body's topology as
//! references are ([`Topology::face`], [`Topology::edge`],
//! [`Topology::corner`]: by key or alias, the nearest to the point among
//! several), on the model the answer draws, so the same picks sent after
//! an edit measure what they name now. The answer says where each pick
//! is in its own picking tables ([`At`]), for the viewport to highlight,
//! or why it wasn't found.
//!
//! The measures are the kernel's ([`varde_kernel::measure`]), each within
//! [`Budget::DEFAULT`], the most an operation may do: coaxial curved faces
//! can take millions of units for a distance, and past the budget the
//! answer is "too complex to measure", never a hang. Each pick's measure
//! and the distance between the two are kept in the [`Cache`] by their
//! bodies' solids, the picks resolved and the tolerance, so a request
//! whose picks and their bodies didn't change (another body edited, a
//! sketch, the camera's asking again) costs nothing more.
//!
//! What an answer carries is checked ([`Inspected::checked`]) where it's
//! made and again on the page, as it comes from the web worker: numbers
//! finite, lengths, areas and volumes not negative, points within
//! [`Picking::MAX_VALUE`], directions unit vectors, each pick's place
//! within its table. One that fails is answered as an error, the model
//! with it as usual.

use std::sync::Arc;

use glam::DVec3;
use serde::{Deserialize, Serialize};
use varde_document::{BodyId, Document};
use varde_kernel::measure::{self, Direction, EdgeShape, MeasureError, Measured, Pick, Target};
use varde_kernel::mesh::FaceKey;
use varde_kernel::{Budget, RenderMesh, Solid, Topology};

use crate::cache::{Cache, Keyer};
use crate::{Evaluation, Picking, Summary};

/// A measure being taken, sent with a regeneration
/// ([`Request::Regenerate`](crate::Request::Regenerate)): one pick or two.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inspect {
    /// Counted up by the app with every change to the picks, so the
    /// newest answer can be told apart from older ones of the same
    /// generation.
    pub revision: u64,
    pub first: InspectPick,
    pub second: Option<InspectPick>,
}

/// What the measure tool picked of a body, resolved as a reference is
/// (by key or alias, the nearest to `near` among several).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InspectPick {
    pub body: BodyId,
    pub entity: Entity,
    /// Where it was picked, which of several entities of the same keys
    /// it is: the nearest to it. Not used for a body.
    pub near: [f64; 3],
}

/// What of a body is picked, by the keys a reference stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Entity {
    /// The whole body.
    Body,
    /// A face, by its key (or one of its aliases).
    Face(FaceKey),
    /// An edge, by its two faces' keys, either way round
    /// ([`Picking::edge_keys`]).
    Edge([FaceKey; 2]),
    /// An edge's point (a straight edge's middle, a round one's centre:
    /// [`Picking::snaps`]), by the edge's keys.
    EdgePoint([FaceKey; 2]),
    /// A corner, by three of the faces meeting there
    /// ([`Picking::corner_keys`]).
    Corner([FaceKey; 3]),
}

/// The answer to an [`Inspect`], with the model it was measured on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inspected {
    /// The [`Inspect::revision`] it answers.
    pub revision: u64,
    /// The first pick, or why it wasn't found ("face not found", "body
    /// not found"): one not found is dropped with a note.
    pub first: Result<Probed, String>,
    /// The second, if there was one.
    pub second: Option<Result<Probed, String>>,
    /// Between the two, where both were found.
    pub between: Option<Between>,
}

/// A pick found on the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Probed {
    /// Where it is in the answer's picking tables: `None` for a body,
    /// and for anything of a body that isn't shown.
    pub at: Option<At>,
    /// What it measures, or why it can't be measured ("too complex to
    /// measure").
    pub measure: Result<Measure, String>,
}

/// An entry of the picking tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum At {
    /// An index into [`Picking::faces`].
    Face(u32),
    /// An edge id of the mesh: an edge, or an edge's point.
    Edge(u32),
    /// An index into [`Picking::corners`].
    Corner(u32),
}

/// What a pick measures (see [`varde_kernel::measure`]), in model units
/// (millimetres) and radians.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Measure {
    Body {
        volume: f64,
        area: f64,
        /// The centre of mass at uniform density.
        centre: Option<[f64; 3]>,
        /// The least box around it: its least and greatest corner.
        bounds: Option<[[f64; 3]; 2]>,
    },
    Face {
        area: f64,
        /// What surface it is, from its form: a plane's normal, a round
        /// face's radius and axis.
        summary: Summary,
        /// A cone's half-angle.
        half_angle: Option<f64>,
    },
    Edge {
        length: f64,
        /// Whether it closes on itself.
        closed: bool,
        shape: EdgeForm,
    },
    /// A corner's point, or an edge's.
    Point([f64; 3]),
}

/// What an edge's curves make (see [`EdgeShape`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EdgeForm {
    Line {
        from: [f64; 3],
        to: [f64; 3],
    },
    /// An arc of a circle, or the whole of it, in the plane square to the
    /// unit `axis` through `centre`.
    Circle {
        centre: [f64; 3],
        axis: [f64; 3],
        radius: f64,
    },
    /// An arc of an ellipse, or the whole of it, `major ≥ minor`.
    Ellipse {
        centre: [f64; 3],
        axis: [f64; 3],
        major: f64,
        minor: f64,
    },
    Other,
}

/// What lies between two picks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Between {
    /// The minimum distance and where it's reached, or why it can't be
    /// measured.
    pub distance: Result<Gap, String>,
    /// The angle between their directions in radians, where both have
    /// one (see [`measure::angle`]): within `[0, π]`, `[0, π/2]` where
    /// either is only a line.
    pub angle: Option<f64>,
}

/// The minimum distance between two picks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Gap {
    pub distance: f64,
    /// A point of the first pick and one of the second, `distance`
    /// apart.
    pub points: [[f64; 3]; 2],
}

/// The message an answer that fails its checks carries in place of its
/// measures.
const BROKEN: &str = "the measure came back broken";

impl Inspected {
    /// It as it is if it holds (numbers finite, lengths, areas and
    /// volumes not negative, points within [`Picking::MAX_VALUE`],
    /// directions unit vectors, angles within their range, each pick's
    /// place within its table of `mesh` and `picking`, the answer's mesh
    /// and tables, an edge a chain), else
    /// every pick's outcome an error.
    pub fn checked(self, mesh: &RenderMesh, picking: &Picking) -> Inspected {
        let probed = |probed: &Result<Probed, String>| match probed {
            Ok(probed) => probed.valid(mesh, picking),
            Err(_) => true,
        };
        let between = |between: &Between| {
            between.distance.as_ref().map_or(true, Gap::valid)
                && between
                    .angle
                    .is_none_or(|a| (0.0..=std::f64::consts::PI).contains(&a))
        };
        if probed(&self.first)
            && self.second.as_ref().is_none_or(probed)
            && self.between.as_ref().is_none_or(between)
            && (self.between.is_none()
                || (self.first.is_ok() && matches!(self.second, Some(Ok(_)))))
        {
            return self;
        }
        Inspected {
            revision: self.revision,
            first: Err(BROKEN.to_owned()),
            second: self.second.map(|_| Err(BROKEN.to_owned())),
            between: None,
        }
    }
}

/// Whether `x` is a coordinate the answer may hold.
fn value(x: f64) -> bool {
    x.abs() <= Picking::MAX_VALUE
}

fn point(p: [f64; 3]) -> bool {
    p.into_iter().all(value)
}

fn size(x: f64) -> bool {
    x >= 0.0 && value(x)
}

fn unit(v: [f64; 3]) -> bool {
    (DVec3::from(v).length() - 1.0).abs() <= Picking::UNIT
}

impl Probed {
    fn valid(&self, mesh: &RenderMesh, picking: &Picking) -> bool {
        let at = match self.at {
            None => true,
            Some(At::Face(f)) => (f as usize) < picking.faces().len(),
            // An edge of the mesh between two faces: a chain, not a
            // crease.
            Some(At::Edge(e)) => (mesh.edge_faces().get(e as usize)).is_some_and(|[a, b]| a != b),
            Some(At::Corner(c)) => (c as usize) < picking.corners().len(),
        };
        at && self.measure.as_ref().map_or(true, Measure::valid)
    }
}

impl Measure {
    fn valid(&self) -> bool {
        match *self {
            Measure::Body {
                volume,
                area,
                centre,
                bounds,
            } => {
                size(volume)
                    && size(area)
                    && centre.is_none_or(point)
                    && bounds.is_none_or(|[min, max]| {
                        point(min) && point(max) && (0..3).all(|i| min[i] <= max[i])
                    })
            }
            Measure::Face {
                area,
                summary,
                half_angle,
            } => {
                size(area)
                    && summary.valid()
                    && half_angle.is_none_or(|a| (0.0..=std::f64::consts::FRAC_PI_2).contains(&a))
            }
            Measure::Edge { length, shape, .. } => size(length) && shape.valid(),
            Measure::Point(p) => point(p),
        }
    }
}

impl EdgeForm {
    fn valid(&self) -> bool {
        match *self {
            EdgeForm::Line { from, to } => point(from) && point(to),
            EdgeForm::Circle {
                centre,
                axis,
                radius,
            } => point(centre) && unit(axis) && size(radius) && radius > 0.0,
            EdgeForm::Ellipse {
                centre,
                axis,
                major,
                minor,
            } => point(centre) && unit(axis) && size(major) && minor > 0.0 && minor <= major,
            EdgeForm::Other => true,
        }
    }

    fn of(shape: &EdgeShape) -> EdgeForm {
        let a = DVec3::to_array;
        match *shape {
            EdgeShape::Line { from, to } => EdgeForm::Line {
                from: a(&from),
                to: a(&to),
            },
            EdgeShape::Circle {
                centre,
                axis,
                radius,
            } => EdgeForm::Circle {
                centre: a(&centre),
                axis: a(&axis),
                radius,
            },
            EdgeShape::Ellipse {
                centre,
                axis,
                major,
                minor,
            } => EdgeForm::Ellipse {
                centre: a(&centre),
                axis: a(&axis),
                major,
                minor,
            },
            EdgeShape::Other => EdgeForm::Other,
        }
    }
}

impl Gap {
    fn valid(&self) -> bool {
        size(self.distance) && self.points.into_iter().all(point)
    }
}

/// A pick's measure as the cache keeps it: with its direction, for the
/// angle between two.
pub(crate) type Kept = Result<(Measure, Option<Direction>), String>;

/// A pick resolved: its body's solid and topology, what the kernel
/// calls it, and where it is in the tables.
struct Resolved<'a> {
    solid: &'a Solid,
    /// What the solid is kept under.
    key: crate::cache::Key,
    topology: Arc<Topology>,
    pick: Pick,
    at: Option<At>,
}

impl Resolved<'_> {
    fn target(&self) -> Target<'_> {
        Target {
            solid: self.solid,
            topology: &self.topology,
            pick: self.pick,
        }
    }

    /// Writes what it is into `keyer`: its solid and the pick.
    fn key(&self, keyer: &mut Keyer) {
        let (kind, index) = match self.pick {
            Pick::Body => (0, 0),
            Pick::Face(i) => (1, i),
            Pick::Edge(i) => (2, i),
            Pick::Corner(i) => (3, i),
            Pick::EdgePoint(i) => (4, i),
        };
        keyer.key(self.key).number(kind).number(index.into());
    }
}

/// Answers `inspect` on the model of `document` whose history gave
/// `evaluation` and whose drawn tables are `picking`.
pub(crate) fn inspect(
    inspect: &Inspect,
    document: &Document,
    evaluation: &Evaluation,
    mesh: &RenderMesh,
    picking: &Picking,
    cache: &mut Cache,
) -> Inspected {
    let fit = document.tolerance().fit().to_bits();
    let first = resolve(&inspect.first, evaluation, mesh, picking, cache);
    let second =
        (inspect.second.as_ref()).map(|pick| resolve(pick, evaluation, mesh, picking, cache));
    let first_kept = kept(&first, document, fit, cache);
    let second_kept = second
        .as_ref()
        .map(|second| kept(second, document, fit, cache));
    let between = match (&first, &second) {
        (Ok(a), Some(Ok(b))) => {
            let mut keyer = Keyer::new("distance");
            a.key(&mut keyer);
            b.key(&mut keyer);
            let key = keyer.number(fit).finish();
            let distance = cache.distance(key, || {
                let tol = document.tolerance();
                let gap = measure::distance(&a.target(), &b.target(), &tol, &Budget::DEFAULT)
                    .map_err(|e| e.to_string())?;
                Ok(Gap {
                    distance: gap.distance,
                    points: gap.points.map(|p| p.to_array()),
                })
            });
            let direction = |kept: Option<&Arc<Kept>>| kept?.as_ref().as_ref().ok()?.1;
            let angle = match (
                direction(first_kept.as_ref()),
                direction(second_kept.as_ref().and_then(Option::as_ref)),
            ) {
                (Some(a), Some(b)) => Some(measure::angle(a, b)),
                _ => None,
            };
            Some(Between { distance, angle })
        }
        _ => None,
    };
    let probed = |resolved: Result<Resolved, String>, kept: Option<Arc<Kept>>| {
        let resolved = resolved?;
        let kept = kept.ok_or("not measured")?;
        Ok(Probed {
            at: resolved.at,
            measure: kept
                .as_ref()
                .as_ref()
                .map(|(m, _)| *m)
                .map_err(Clone::clone),
        })
    };
    Inspected {
        revision: inspect.revision,
        first: probed(first, first_kept),
        second: second.map(|second| probed(second, second_kept.flatten())),
        between,
    }
    .checked(mesh, picking)
}

/// What `resolved` measures, from `cache` or measured and kept there,
/// if it was found.
fn kept(
    resolved: &Result<Resolved, String>,
    document: &Document,
    fit: u64,
    cache: &mut Cache,
) -> Option<Arc<Kept>> {
    let resolved = resolved.as_ref().ok()?;
    let mut keyer = Keyer::new("measure");
    resolved.key(&mut keyer);
    let key = keyer.number(fit).finish();
    Some(cache.measure(key, || measure_one(resolved, document)))
}

/// What `resolved` measures, with its direction.
fn measure_one(resolved: &Resolved, document: &Document) -> Kept {
    let tol = document.tolerance();
    let measured =
        measure::measure(&resolved.target(), &tol, &Budget::DEFAULT).map_err(|e| {
            match (e, resolved.pick) {
                (MeasureError::NotFound(_), Pick::EdgePoint(_)) => {
                    "the edge has no middle or centre".to_owned()
                }
                (e, _) => e.to_string(),
            }
        })?;
    let a = DVec3::to_array;
    let direction = measured.direction();
    let measure = match measured {
        Measured::Body(body) => Measure::Body {
            volume: body.volume,
            area: body.area,
            centre: body.centre.as_ref().map(a),
            bounds: body.bounds.map(|b| [b.min.to_array(), b.max.to_array()]),
        },
        Measured::Face(face) => Measure::Face {
            area: face.area,
            summary: Summary::of(&face.form),
            half_angle: face.half_angle(),
        },
        Measured::Edge(edge) => Measure::Edge {
            length: edge.length,
            closed: edge.closed,
            shape: EdgeForm::of(&edge.shape),
        },
        Measured::Point(p) => Measure::Point(p.to_array()),
    };
    Ok((measure, direction))
}

/// `pick` found on its body's solid in `evaluation`, with where it is in
/// `picking`, or why it isn't there.
fn resolve<'a>(
    pick: &InspectPick,
    evaluation: &'a Evaluation,
    mesh: &RenderMesh,
    picking: &Picking,
    cache: &mut Cache,
) -> Result<Resolved<'a>, String> {
    let made = (evaluation.bodies.iter())
        .find(|made| made.body == pick.body)
        .ok_or("body not found")?;
    let solid = &*made.solid;
    let topology = cache.topology(Keyer::new("topology").key(made.key).finish(), || {
        solid.topology()
    });
    let near = DVec3::from(pick.near);
    let found = |e: varde_kernel::topology::NotFound| e.to_string();
    let offsets = Offsets::of(mesh, picking, pick.body);
    let (pick, at) = match pick.entity {
        Entity::Body => (Pick::Body, None),
        Entity::Face(key) => {
            let r = topology.face(solid, &key, near).map_err(found)?;
            (
                Pick::Face(r),
                offsets.face.and_then(|o| o.checked_add(r)).map(At::Face),
            )
        }
        Entity::Edge(keys) => {
            let c = topology.edge(solid, keys, near).map_err(found)?;
            (
                Pick::Edge(c),
                offsets.chain.and_then(|o| o.checked_add(c)).map(At::Edge),
            )
        }
        Entity::EdgePoint(keys) => {
            let c = topology.edge(solid, keys, near).map_err(found)?;
            (
                Pick::EdgePoint(c),
                offsets.chain.and_then(|o| o.checked_add(c)).map(At::Edge),
            )
        }
        Entity::Corner(keys) => {
            let c = topology.corner(solid, keys, near).map_err(found)?;
            (
                Pick::Corner(c),
                offsets
                    .corner
                    .and_then(|o| o.checked_add(c))
                    .map(At::Corner),
            )
        }
    };
    Ok(Resolved {
        solid,
        key: made.key,
        topology,
        pick,
        at,
    })
}

/// Where a body's entries start in each of the picking tables, if it's
/// shown and has some: the mesh's faces and first edges are each shown
/// body's topology's regions and chains in order, one part after another,
/// and the corners table its corners.
struct Offsets {
    face: Option<u32>,
    chain: Option<u32>,
    corner: Option<u32>,
}

impl Offsets {
    fn of(mesh: &RenderMesh, picking: &Picking, body: BodyId) -> Offsets {
        let Some(part) = (picking.bodies().iter())
            .position(|&b| b == body)
            .and_then(|p| mesh.parts().nth(p))
        else {
            return Offsets {
                face: None,
                chain: None,
                corner: None,
            };
        };
        let first = |found: Option<usize>| found.and_then(|i| u32::try_from(i).ok());
        let faces = part.faces.clone();
        Offsets {
            face: first((!part.faces.is_empty()).then_some(part.faces.start)),
            chain: first((!part.edges.is_empty()).then_some(part.edges.start)),
            corner: first(
                (picking.corners().iter()).position(|c| faces.contains(&(c.faces[0] as usize))),
            ),
        }
    }
}

#[cfg(test)]
mod tests;
