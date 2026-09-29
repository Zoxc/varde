//! Evaluating the feature history into the bodies' solids.
//!
//! Features run in the document's order. A sketch gives its profiles
//! ([`Sketch::profiles`]). An extrude finds its regions again in its
//! sketch's profiles ([`Profiles::resolve`]; one that's gone is "region
//! not found"), merges them ([`Profiles::merge`]), turns the loops into a
//! kernel profile ([`profile`]) and sweeps it
//! with [`varde_kernel::extrude`] on the sketch's plane, over
//! [`Extrude::span`], within the document's tolerance and the default
//! budget, its faces named by its feature id. A new body gets that
//! solid. Join, cut and intersect aren't available yet, and fail so; a
//! through-all extent's span is worked out from the bodies it could cut
//! ([`through_all`]). A feature that fails records why and changes no
//! body; the later ones still run.
//!
//! Every result goes through the [`Cache`], keyed by what it depends on,
//! so only what an edit changes runs again.
//!
//! [`Sketch::profiles`]: varde_sketch::Sketch::profiles

use std::sync::Arc;

use varde_document::{
    BodyId, Document, Extrude, Feature, FeatureId, FeatureKind, MAX_COORD, Operation, Plane, Sketch,
};
use varde_kernel::{Budget, Frame, Solid, Tolerance};
use varde_sketch::{Profiles, TooComplex};

use crate::cache::{Cache, Key, Keyer};
use crate::profile::profile;

/// What the history gives: the solids of the bodies, and the features
/// that failed.
#[derive(Debug, Clone, Default)]
pub struct Evaluation {
    /// Each body that has a solid, in the order the features made them.
    pub bodies: Vec<BodySolid>,
    /// The features that failed and why, in the document's order.
    pub failed: Vec<(FeatureId, String)>,
}

/// A body's solid.
#[derive(Debug, Clone)]
pub struct BodySolid {
    pub body: BodyId,
    pub solid: Arc<Solid>,
    /// What the solid was filed under, for drawing it.
    pub(crate) key: Key,
}

/// A sketch evaluated: the sketch, its profiles, where it is, and its
/// key.
struct SketchOutput<'a> {
    id: FeatureId,
    sketch: &'a Sketch,
    profiles: Arc<Result<Profiles, TooComplex>>,
    plane: Plane,
    key: Key,
}

/// The margin a through-all span gets past the bodies' extent at each
/// end: this share of the extent, and [`THROUGH_ALL_MARGIN_MM`].
const THROUGH_ALL_MARGIN: f64 = 0.01;
const THROUGH_ALL_MARGIN_MM: f64 = 1.0;

/// Evaluates the history of `document`, see the module's docs.
pub fn evaluate(document: &Document, cache: &mut Cache) -> Evaluation {
    let tolerance = document.tolerance();
    let mut sketches: Vec<SketchOutput> = Vec::new();
    let mut evaluation = Evaluation::default();
    for feature in document.features() {
        match &feature.kind {
            FeatureKind::Sketch { plane, sketch } => {
                let key = Keyer::new("sketch").value(plane).value(sketch).finish();
                let profiles = cache.profiles(key, || sketch.profiles());
                sketches.push(SketchOutput {
                    id: feature.id,
                    sketch,
                    profiles,
                    plane: *plane,
                    key,
                });
            }
            FeatureKind::Extrude(extrude) => {
                // A checked document's extrude names a sketch before it.
                let Some(sketch) = sketches.iter().find(|s| s.id == extrude.sketch) else {
                    evaluation
                        .failed
                        .push((feature.id, "its sketch isn't there".to_owned()));
                    continue;
                };
                let run = Run {
                    feature,
                    extrude,
                    sketch,
                    tolerance,
                };
                match run.evaluate(&evaluation.bodies, cache) {
                    Ok(made) => evaluation.bodies.extend(made),
                    Err(error) => evaluation.failed.push((feature.id, error)),
                }
            }
        }
    }
    evaluation
}

/// An extrude being evaluated.
struct Run<'a> {
    feature: &'a Feature,
    extrude: &'a Extrude,
    sketch: &'a SketchOutput<'a>,
    tolerance: Tolerance,
}

impl Run<'_> {
    /// The bodies it makes, given those made before it, or why it fails.
    fn evaluate(&self, before: &[BodySolid], cache: &mut Cache) -> Result<Vec<BodySolid>, String> {
        let placement = self.sketch.plane.placement();
        let frame = Frame {
            origin: placement.origin,
            x: placement.x,
            y: placement.y,
        };
        let span = match self.extrude.span() {
            Some(span) => span,
            None => {
                let targets = before
                    .iter()
                    .filter(|made| !self.extrude.operation.excluded().contains(&made.body))
                    .map(|made| &*made.solid);
                through_all(&frame, targets).ok_or("there's no body to go through")?
            }
        };
        let body = match &self.extrude.operation {
            Operation::NewBody(body) => *body,
            Operation::Join(_) => return Err("joining isn't available yet".to_owned()),
            Operation::Cut(_) => return Err("cutting isn't available yet".to_owned()),
            Operation::Intersect(_) => return Err("intersecting isn't available yet".to_owned()),
        };
        let key = Keyer::new("extrude")
            .number(self.feature.id.get())
            .value(self.extrude)
            .number(self.tolerance.fit().to_bits())
            .number(span.0.to_bits())
            .number(span.1.to_bits())
            .key(self.sketch.key)
            .finish();
        let solid = cache.solid(key, || self.solid(&frame, span))?;
        Ok(vec![BodySolid { body, solid, key }])
    }

    /// The solid swept from the regions over `span` on `frame`.
    fn solid(&self, frame: &Frame, (from, to): (f64, f64)) -> Result<Solid, String> {
        let profiles = match &*self.sketch.profiles {
            Ok(profiles) => profiles,
            Err(e) => return Err(format!("its sketch is {e}")),
        };
        let regions = profiles
            .resolve(&self.extrude.regions)
            .into_iter()
            .collect::<Option<Vec<usize>>>()
            .ok_or("region not found")?;
        let loops = profiles.merge(&regions).map_err(|e| e.to_string())?;
        let profile = profile(self.sketch.sketch, profiles, &loops, self.tolerance.fit())
            .map_err(|e| e.to_string())?;
        varde_kernel::extrude(
            &profile,
            frame,
            from,
            to,
            self.feature.id.get(),
            &self.tolerance,
            &Budget::DEFAULT,
        )
        .map_err(|e| e.to_string())
    }
}

/// The span along `frame`'s normal that goes through all of `bodies`:
/// from below the lowest of their boxes' corners to past the highest, by
/// a margin ([`THROUGH_ALL_MARGIN`] of the extent and
/// [`THROUGH_ALL_MARGIN_MM`]), within [`MAX_COORD`]. `None` if there are
/// no bodies, or they're all out of reach.
pub(crate) fn through_all<'a>(
    frame: &Frame,
    bodies: impl IntoIterator<Item = &'a Solid>,
) -> Option<(f64, f64)> {
    let normal = frame.normal();
    let mut span: Option<(f64, f64)> = None;
    for bounds in bodies.into_iter().filter_map(Solid::bounds3) {
        for corner in 0..8 {
            let pick = |bit: u32, min: f64, max: f64| if corner & bit == 0 { min } else { max };
            let point = glam::DVec3::new(
                pick(1, bounds.min.x, bounds.max.x),
                pick(2, bounds.min.y, bounds.max.y),
                pick(4, bounds.min.z, bounds.max.z),
            );
            let height = (point - frame.origin).dot(normal);
            span = Some(span.map_or((height, height), |(lo, hi)| {
                (lo.min(height), hi.max(height))
            }));
        }
    }
    let (lo, hi) = span?;
    let margin = (hi - lo) * THROUGH_ALL_MARGIN + THROUGH_ALL_MARGIN_MM;
    let max = f64::from(MAX_COORD);
    let (from, to) = ((lo - margin).max(-max), (hi + margin).min(max));
    (from < to).then_some((from, to))
}

#[cfg(test)]
pub(crate) mod tests;
