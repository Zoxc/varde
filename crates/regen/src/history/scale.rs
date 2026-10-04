//! Evaluating a scale: the bodies' solids, as the features before it
//! leave them, scaled about its point by one [`Motion::scale`].
//!
//! The point is found as an align's ([`Found::point`]): the origin, or a
//! corner, an edge's middle or a rim's centre on a body made before (one
//! of the scaled bodies too). The factors are typed, one or one per
//! world axis, or worked out from an edge of one of the scaled bodies:
//! the edge found on its body's topology by its names and point and
//! measured by the measure tool's lengths ([`measure`]: a straight chain
//! by one square root, a round one from its conics, any other by
//! quadrature), then the typed length over that one, uniform, or along
//! the world axis the edge runs along only (straight, and within a sine
//! of [`AXIS_SINE`] of the axis). Every factor must be within the
//! document's `1e-3 ..= 1e3`. The bodies are then moved as a move's
//! are ([`place`]: a body whose box the scale takes past the coordinate
//! limit is refused before it's scaled; the scaled solid cached by the
//! body's key and the motion's bits, so an edge length that gives the
//! same factor finds it again). What it found is noted for the draft's
//! reply ([`Evaluation::scaled`]), whether or not it goes on to work.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DVec3;
use varde_document::{BodyId, Document, EdgeRef, FeatureId, MAX_SCALE_FACTOR, Scale, ScaleFactor};
use varde_kernel::measure::{EdgeShape, Measured, Pick, Target, measure};
use varde_kernel::mesh::Surface;
use varde_kernel::{Budget, Evidence, Motion, Tolerance};

use super::align::Found;
use super::motion::{How, place, reference_key};
use super::{Evaluation, Failed, own_solids};
use crate::cache::{Cache, EdgeLength};
use crate::message::{self, Moving, Whose};
use crate::{ErrorGeometry, ScaleFound, inspect};

/// How near a world axis an edge must run for a scale along its axis
/// only: the sine of the angle between them at most this. It names the
/// intent (an edge drawn along the axis, rounded); the edge's other
/// components, at most this much of it, aren't scaled, so the edge gets
/// its typed length to within about `1e-12` relative over the factors
/// allowed.
pub const AXIS_SINE: f64 = 1e-9;

/// Changes the bodies of `evaluation` that the scale `scale`, the feature
/// `feature`, scales, or says why it fails, changing nothing.
///
/// Every body it names must have a solid of its own, as a move's
/// ([`own_solids`]). Its point is found, then its edge measured into its
/// factor if it scales to an edge's length, then the factors checked
/// (see the module's docs); what's found is noted, and the first failure
/// of those, in that order, is the scale's.
pub(super) fn evaluate_scale(
    document: &Document,
    feature: FeatureId,
    scale: &Scale,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    own_solids(document, scale.bodies.iter().copied(), evaluation)?;
    let found = Found {
        moved: None,
        evaluation,
        tolerance,
    };
    let centre = found.point(&scale.about, Whose::ScaleCentre, cache);
    let edge = (scale.factor.edge()).map(|edge| measured(edge, evaluation, tolerance, cache));
    let factors = factors(&scale.factor, edge.as_ref(), tolerance);
    let fitted = fitted(&scale.bodies, evaluation);
    note(
        evaluation,
        feature,
        &centre,
        edge.as_ref(),
        &factors,
        fitted,
    );
    let centre = centre?;
    // An edge length refused (its edge's failure comes through the
    // factors) shows the edge, once it's found.
    let factors = factors.map_err(|failed| match scale.factor.edge() {
        Some(on) if failed.geometry.is_none() => Failed {
            geometry: edge_geometry(on, evaluation, tolerance, cache),
            ..failed
        },
        _ => failed,
    })?;
    let motion = Motion::scale(centre, factors).ok_or(message::SCALE_FACTOR)?;
    let how = How {
        moving: Moving::Scale,
        copy: None,
    };
    place(
        document,
        &scale.bodies,
        &motion,
        how,
        tolerance,
        evaluation,
        cache,
    )
}

/// The edge `edge` as measured on its body, one of the scale's (each of
/// which has a solid of its own in `evaluation`), cached by the body's
/// key, the edge's names and point and the fit tolerance; or why it
/// isn't found or measured. The body's topology is the one drawing it
/// keeps ([`inspect::topology`]): the edge is picked on the body as the
/// features before the scale leave it, which the model then draws.
fn measured(
    edge: &EdgeRef,
    evaluation: &Evaluation,
    tolerance: &Tolerance,
    cache: &mut Cache,
) -> Result<EdgeLength, Failed> {
    let made = (evaluation.bodies.iter())
        .find(|made| made.body == edge.body)
        .ok_or(message::SCALE_EDGE_NOT_FOUND)?;
    let key = reference_key("scale edge", made.key, &edge.faces, edge.near, tolerance);
    let topology = (!cache.holds(key)).then(|| inspect::topology(made, cache));
    cache.length(key, || {
        let solid = &made.solid;
        let topology = topology.unwrap_or_else(|| Arc::new(solid.topology()));
        let chain = (topology.edge(solid, edge.faces, edge.near))
            .map_err(|_| message::SCALE_EDGE_NOT_FOUND)?;
        let target = Target {
            solid,
            topology: &topology,
            pick: Pick::Edge(chain),
        };
        match measure(&target, tolerance, &Budget::DEFAULT) {
            Ok(Measured::Edge(edge)) => Ok(EdgeLength {
                length: edge.length,
                line: match edge.shape {
                    EdgeShape::Line { from, to } => Some([from, to]),
                    _ => None,
                },
            }),
            Ok(_) => Err(message::SCALE_EDGE_NOT_FOUND.into()),
            Err(error) => Err(message::scale_measure(error).into()),
        }
    })
}

/// What a refused edge length shows: the curves of `edge` on its body's
/// solid in `evaluation`, if it's found there, drawn at the display of
/// `tolerance` as an align's refused references are. The topology is
/// the one drawing the body keeps ([`inspect::topology`]): a refused
/// scale leaves its bodies as they were, so the model draws them, and a
/// length typed again and again refused doesn't work it out each time.
fn edge_geometry(
    edge: &EdgeRef,
    evaluation: &Evaluation,
    tolerance: &Tolerance,
    cache: &mut Cache,
) -> Option<Arc<ErrorGeometry>> {
    let made = (evaluation.bodies.iter()).find(|made| made.body == edge.body)?;
    let solid = &made.solid;
    let topology = inspect::topology(made, cache);
    let chain = topology.edge(solid, edge.faces, edge.near).ok()?;
    let chain = topology.chains().get(chain as usize)?;
    let mesh = solid.mesh();
    let tris = mesh.tris().len();
    let mut evidence = Evidence::default();
    evidence.add_curves(
        (chain.halfedges.iter())
            .filter(|&&h| (h as usize) / 3 < tris)
            .map(|&h| mesh.curve(h)),
    );
    ErrorGeometry::of_evidence(&evidence, tolerance)
}

/// The factors along X, Y and Z that `factor` gives, its edge measured
/// as `edge` (for an edge length), or why there are none: an edge length
/// divides only by an edge longer than the resolution, and the factor it
/// gives must be within range, as a typed one must.
fn factors(
    factor: &ScaleFactor,
    edge: Option<&Result<EdgeLength, Failed>>,
    tolerance: &Tolerance,
) -> Result<DVec3, Failed> {
    let factors = match factor {
        ScaleFactor::Uniform(value) => DVec3::splat(value.value),
        ScaleFactor::PerAxis([x, y, z]) => DVec3::new(x.value, y.value, z.value),
        ScaleFactor::EdgeLength {
            length, axis_only, ..
        } => {
            let edge = match edge {
                Some(Ok(edge)) => *edge,
                Some(Err(failed)) => return Err(failed.clone()),
                None => return Err(message::SCALE_EDGE_NOT_FOUND.into()),
            };
            // Checked before dividing: the factor is then finite.
            if edge.length.partial_cmp(&tolerance.resolution()) != Some(Ordering::Greater) {
                return Err(message::SCALE_EDGE_SHORT.into());
            }
            let f = length.value / edge.length;
            if !in_range(f) {
                return Err(message::SCALE_TOO_FAR.into());
            }
            if *axis_only {
                let [from, to] = edge.line.ok_or(message::SCALE_EDGE_NOT_STRAIGHT)?;
                let axis = along_axis(to - from).ok_or(message::SCALE_EDGE_SLANTED)?;
                let mut factors = DVec3::ONE;
                factors[axis] = f;
                factors
            } else {
                DVec3::splat(f)
            }
        }
    };
    if !factors.to_array().into_iter().all(in_range) {
        return Err(message::SCALE_FACTOR.into());
    }
    Ok(factors)
}

/// Whether `f` is a factor a scale takes: within `1e-3 ..= 1e3`.
fn in_range(f: f64) -> bool {
    (1.0 / MAX_SCALE_FACTOR..=MAX_SCALE_FACTOR).contains(&f)
}

/// The world axis (0, 1 or 2) the direction `d` runs along, if it's
/// within a sine of [`AXIS_SINE`] of one: the sum of the squares of its
/// other two components at most `AXIS_SINE²` of its length's square. A
/// decision by `+ −  ×` alone. `d` is a difference of points within the
/// coordinate limit, so its squares are finite. The panel offers "Along
/// its axis only" by it too.
pub fn along_axis(d: DVec3) -> Option<usize> {
    let squares = d * d;
    let axis = if squares.x >= squares.y && squares.x >= squares.z {
        0
    } else if squares.y >= squares.z {
        1
    } else {
        2
    };
    let all = squares.x + squares.y + squares.z;
    let across = all - squares[axis];
    (all > 0.0 && across <= AXIS_SINE * AXIS_SINE * all).then_some(axis)
}

/// How many faces of `bodies`' solids in `evaluation` are fitted, that
/// is claim no exact surface ([`Surface::Free`]), counted by key per
/// body: those a scale up takes further from what they stand for.
fn fitted(bodies: &[BodyId], evaluation: &Evaluation) -> u32 {
    let mut keys = BTreeSet::new();
    for made in (evaluation.bodies.iter()).filter(|made| bodies.binary_search(&made.body).is_ok()) {
        for face in made.solid.mesh().faces() {
            if matches!(face.surface, Surface::Free) {
                keys.insert((made.body, face.name.key()));
            }
        }
    }
    u32::try_from(keys.len()).unwrap_or(u32::MAX)
}

/// Notes what `feature` found in `evaluation`, each part the wire takes
/// ([`ScaleFound::fits`]), if it found its point, its edge or its
/// factors.
fn note(
    evaluation: &mut Evaluation,
    feature: FeatureId,
    centre: &Result<DVec3, Failed>,
    edge: Option<&Result<EdgeLength, Failed>>,
    factors: &Result<DVec3, Failed>,
    fitted: u32,
) {
    let found = ScaleFound {
        centre: (centre.as_ref().ok())
            .map(|centre| centre.to_array())
            .filter(|&centre| ScaleFound::centre_fits(centre)),
        length: (edge.and_then(|edge| edge.as_ref().ok()))
            .map(|edge| edge.length)
            .filter(|&length| ScaleFound::length_fits(length)),
        factors: (factors.as_ref().ok())
            .map(|factors| factors.to_array())
            .filter(|&factors| ScaleFound::factors_fit(factors)),
        fitted,
    };
    if found.fits() {
        evaluation.scaled.push((feature, found));
    }
}
