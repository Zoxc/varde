//! A sketch's links found again on the model as the features before the
//! sketch leave it, and the sketch with what's changed given to them.
//!
//! Each link's source ([`LinkSource`]) is resolved where the sketch is in
//! the history: another sketch's point or curve on that sketch as it is,
//! placed where it was placed, projected through the affine map from its
//! plane into this one's ([`Sketch::project_item`]); a model edge, face
//! or corner on its body among those the features before have made,
//! through [`Evaluation::holder`] and on through the splits before the
//! sketch, as a sketch's face is found ([`place_on_face`](super::place_on_face)),
//! so a split that moved it onto another body still finds it. A model
//! edge projected is sampled exactly along its conics and fitted
//! ([`LinkShape::fit`]: a point, a line, a circle or an arc to the
//! resolution, else a spline within the fit tolerance), a corner a
//! point; an edge intersected gives the points where it crosses the
//! plane, a face the curves where the plane cuts it
//! ([`varde_kernel::section`]), each fitted likewise. One that isn't
//! found, or gives nothing, is broken with why, keeping what it holds.
//!
//! A link whose shape found isn't the one it holds, to the resolution
//! ([`Sketch::link_follows`]), is stale: the sketch is proposed with every
//! stale link given its shape ([`SketchEdit::Relink`]), solved so the
//! rest follows, or, if it doesn't solve, as relinked unsolved (the
//! sketch then shows as not solving). That sketch is what
//! [`Evaluation::relinked`] lists, for the app to fold into the change
//! that moved the sources. This regeneration goes on with the sketch as
//! the document holds it: the next, of the document relinked, finds it
//! as it now is.

use std::sync::Arc;

use glam::{DAffine2, DMat2, DVec2, DVec3};
use varde_document::{
    BodyId, Document, FeatureId, LinkSource, OutsideRef, Placement, PointRef, Sketch, Tolerance,
};
use varde_kernel::section::{self, Plane as CutPlane, SectionError};
use varde_sketch::{Budget, Id, LinkKind, LinkShape, SampledChain, SketchEdit};

use super::{BodySolid, Evaluation, SketchOutput};
use crate::cache::{Cache, Key, Keyer};

/// How many places each conic of a projected edge is sampled at.
const PER_CONIC: usize = 32;

/// The sketch it projects from isn't there.
pub(crate) const SOURCE_SKETCH_GONE: &str = "its sketch isn't there";
/// The sketch it projects from isn't placed.
pub(crate) const SOURCE_SKETCH_NOT_PLACED: &str = "its sketch isn't placed";
/// The point or curve it projects isn't in its sketch.
pub(crate) const SOURCE_ITEM_GONE: &str = "it isn't in its sketch any more";
/// The sketch the link is in isn't placed.
pub(crate) const NOT_PLACED: &str = "the sketch isn't placed";
/// The body it's on is gone.
pub(crate) const BODY_GONE: &str = "its body is gone";
pub(crate) const EDGE_NOT_FOUND: &str = "its edge wasn't found";
pub(crate) const FACE_NOT_FOUND: &str = "its face wasn't found";
pub(crate) const CORNER_NOT_FOUND: &str = "its corner wasn't found";
/// An edge intersected that doesn't meet the plane.
pub(crate) const EDGE_MISSES: &str = "its edge doesn't cross the sketch's plane";
/// An edge intersected that lies in the plane.
pub(crate) const EDGE_IN_PLANE: &str = "its edge lies in the sketch's plane: project it instead";
/// A face intersected that the plane doesn't cut.
pub(crate) const FACE_MISSES: &str = "the sketch's plane doesn't cut its face";
/// A face intersected that lies in the plane.
pub(crate) const FACE_IN_PLANE: &str =
    "its face lies in the sketch's plane: project its edges instead";
/// More than a section or a sampling may take.
pub(crate) const TOO_COMPLEX: &str = "it's too complex to follow";
/// What it found can't be held by the sketch (past the limit).
pub(crate) const CANT_HOLD: &str = "what it finds is outside what a sketch can hold";
/// The kind of source a link can't take (a file's).
pub(crate) const WRONG_KIND: &str = "it can't take what it comes from";

/// What one link of a sketch found: its shape, or why it's broken.
pub(crate) type Found = (Id, Result<LinkShape, String>);

/// Finds the links of the sketch feature `feature` holding `sketch`, at
/// `placement` (`None` where it isn't placed), whose sources are
/// `sources`, on the model `evaluation` holds and the sketches before it,
/// `sketches`, in `document`.
#[expect(
    clippy::too_many_arguments,
    reason = "what a sketch's place in the history is"
)]
pub(super) fn find(
    document: &Document,
    sketch: &Sketch,
    sources: &[LinkSource],
    placement: Option<Placement>,
    sketches: &[SketchOutput],
    evaluation: &Evaluation,
    tolerance: Tolerance,
    cache: &mut Cache,
) -> Vec<Found> {
    let design = document.design();
    (sources.iter())
        .map(|from| {
            let Some(link) = sketch.link(from.link) else {
                return (from.link, Err(WRONG_KIND.to_owned()));
            };
            let Some(placement) = placement else {
                return (from.link, Err(NOT_PLACED.to_owned()));
            };
            let found = one(
                link.kind,
                &from.source,
                &placement,
                sketches,
                evaluation,
                tolerance,
                cache,
            )
            .and_then(|shape| {
                if shape.fits(design.max) {
                    Ok(shape)
                } else {
                    Err(CANT_HOLD.to_owned())
                }
            });
            (from.link, found)
        })
        .collect()
}

/// What a link of `kind` finds of `source` for a sketch at `placement`.
fn one(
    kind: LinkKind,
    source: &OutsideRef,
    placement: &Placement,
    sketches: &[SketchOutput],
    evaluation: &Evaluation,
    tolerance: Tolerance,
    cache: &mut Cache,
) -> Result<LinkShape, String> {
    if !source.takes(kind) {
        return Err(WRONG_KIND.to_owned());
    }
    let (exact, fit) = (tolerance.resolution(), tolerance.fit());
    match *source {
        OutsideRef::Sketch { sketch, item } => {
            let source = (sketches.iter())
                .find(|output| output.id == sketch)
                .ok_or(SOURCE_SKETCH_GONE)?;
            let from = source.placement.ok_or(SOURCE_SKETCH_NOT_PLACED)?;
            let map = affine(&from, placement);
            let key = Keyer::new("link sketch")
                .key(source.key)
                .placement(&from)
                .placement(placement)
                .value(&item)
                .number(fit.to_bits())
                .finish();
            cache.link(key, || {
                (source.sketch.project_item(item, &map, exact, fit)).map_err(|why| match why {
                    varde_sketch::ProjectError::Missing => SOURCE_ITEM_GONE.to_owned(),
                    varde_sketch::ProjectError::Fit(why) => why.to_string(),
                })
            })
        }
        OutsideRef::Edge(edge) => {
            on_body(edge.body, evaluation, |made| {
                let key = model_key("link edge", made, source, kind, placement, &tolerance);
                let topology = crate::inspect::topology(made, cache);
                cache.link(key, || {
                    let solid = &made.solid;
                    let chain = (topology.edge(solid, edge.faces, edge.near))
                        .map_err(|_| EDGE_NOT_FOUND.to_owned())?;
                    let chain = &topology.chains()[chain as usize];
                    let mesh = solid.mesh();
                    let tris = mesh.tris().len();
                    if (chain.halfedges.iter()).any(|&h| (h as usize) / 3 >= tris) {
                        return Err(EDGE_NOT_FOUND.to_owned());
                    }
                    let curves: Vec<_> = chain.halfedges.iter().map(|&h| mesh.curve(h)).collect();
                    match kind {
                        LinkKind::Project => {
                            let places = section::sample(&curves, PER_CONIC, chain.closed)
                                .map_err(|_| TOO_COMPLEX.to_owned())?;
                            let places = places.iter().map(|&p| placement.to_sketch(p)).collect();
                            let chain = SampledChain {
                                places,
                                closed: chain.closed,
                            };
                            LinkShape::fit(&[chain], exact, fit).map_err(|why| why.to_string())
                        }
                        LinkKind::Intersect => {
                            let points = section::crossings(&curves, &cut(placement), exact)
                                .map_err(|why| match why {
                                    SectionError::InPlane => EDGE_IN_PLANE.to_owned(),
                                    SectionError::TooComplex => TOO_COMPLEX.to_owned(),
                                })?;
                            if points.is_empty() {
                                return Err(EDGE_MISSES.to_owned());
                            }
                            let mut shape = LinkShape::default();
                            for p in points {
                                shape.point(placement.to_sketch(p));
                            }
                            Ok(shape)
                        }
                    }
                })
            })
        }
        OutsideRef::Face(face) => {
            on_body(face.body, evaluation, |made| {
                let key = model_key("link face", made, source, kind, placement, &tolerance);
                let topology = crate::inspect::topology(made, cache);
                cache.link(key, || {
                    let solid = &made.solid;
                    let region = (topology.face(solid, &face.key, face.near))
                        .map_err(|_| FACE_NOT_FOUND.to_owned())?;
                    let tris = &topology.regions()[region as usize].tris;
                    let sections = section::face_section(solid, tris, &cut(placement), exact)
                        .map_err(|why| match why {
                            SectionError::InPlane => FACE_IN_PLANE.to_owned(),
                            SectionError::TooComplex => TOO_COMPLEX.to_owned(),
                        })?;
                    if sections.is_empty() {
                        return Err(FACE_MISSES.to_owned());
                    }
                    LinkShape::fit(&chains(placement, sections), exact, fit)
                        .map_err(|why| why.to_string())
                })
            })
        }
        OutsideRef::Corner(PointRef::Corner { body, faces, near }) => {
            on_body(body, evaluation, |made| {
                let key = model_key("link corner", made, source, kind, placement, &tolerance);
                let topology = crate::inspect::topology(made, cache);
                cache.link(key, || {
                    let at = (topology.corner_point(&made.solid, faces, near))
                        .map_err(|_| CORNER_NOT_FOUND.to_owned())?;
                    let mut shape = LinkShape::default();
                    shape.point(placement.to_sketch(at));
                    Ok(shape)
                })
            })
        }
        OutsideRef::Corner(_) => Err(WRONG_KIND.to_owned()),
    }
}

/// The plane of a sketch at `placement`.
fn cut(placement: &Placement) -> CutPlane {
    CutPlane {
        point: placement.origin,
        normal: placement.normal,
    }
}

/// The key of what a link of `kind` finds of `source` on the solid
/// `made`, for a sketch at `placement`.
fn model_key(
    what: &str,
    made: &BodySolid,
    source: &OutsideRef,
    kind: LinkKind,
    placement: &Placement,
    tolerance: &Tolerance,
) -> Key {
    Keyer::new(what)
        .key(made.key)
        .value(source)
        .value(&kind)
        .placement(placement)
        .number(tolerance.fit().to_bits())
        .finish()
}

/// The affine map taking a place in a sketch at `from` to the place its
/// projection square onto the plane of `to` is at, in that sketch.
fn affine(from: &Placement, to: &Placement) -> DAffine2 {
    let along = |axis: DVec3| DVec2::new(axis.dot(to.x), axis.dot(to.y));
    let columns = DMat2::from_cols(along(from.x), along(from.y));
    DAffine2::from_mat2_translation(columns, to.to_sketch(from.origin))
}

/// `find` on the body `body`'s solid among those `evaluation` holds
/// (through [`Evaluation::holder`]), or, where it says what it looks for
/// isn't there ([`not_there`]), on the bodies splits before made of it
/// and it of others, the first split first, as a sketch's face is
/// followed.
fn on_body<T>(
    body: BodyId,
    evaluation: &Evaluation,
    mut find: impl FnMut(&BodySolid) -> Result<T, String>,
) -> Result<T, String> {
    let made = super::motion::holding(body, evaluation).ok_or(BODY_GONE)?;
    let first = find(made);
    if !first.as_ref().is_err_and(|why| not_there(why)) {
        return first;
    }
    let mut on = vec![made.body];
    let mut at = 0;
    while let Some(&body) = on.get(at) {
        at += 1;
        for &(split, new) in &evaluation.splits {
            let (Some(split), Some(new)) = (evaluation.holder(split), evaluation.holder(new))
            else {
                continue;
            };
            let next = if split == body {
                new
            } else if new == body {
                split
            } else {
                continue;
            };
            if on.contains(&next) {
                continue;
            }
            on.push(next);
            let Some(made) = evaluation.bodies.iter().find(|made| made.body == next) else {
                continue;
            };
            let followed = find(made);
            if !followed.as_ref().is_err_and(|why| not_there(why)) {
                return followed;
            }
        }
    }
    first
}

/// Whether `why` says what was looked for isn't on the body, which a
/// split may have moved onto another.
fn not_there(why: &str) -> bool {
    [EDGE_NOT_FOUND, FACE_NOT_FOUND, CORNER_NOT_FOUND].contains(&why)
}

/// The places of `sections` in the sketch at `placement`.
fn chains(placement: &Placement, sections: Vec<section::Section>) -> Vec<SampledChain> {
    (sections.into_iter())
        .map(|section| SampledChain {
            places: section
                .places
                .iter()
                .map(|&p| placement.to_sketch(p))
                .collect(),
            closed: section.closed,
        })
        .collect()
}

/// The sketch `sketch`, feature `feature`, with the links `found` that
/// are stale given what they found, solved, noted in `evaluation`
/// ([`Evaluation::relinked`]), and those broken noted with why
/// ([`Evaluation::broken`]). Nothing for a sketch whose links all hold
/// what they find.
pub(super) fn relink(
    feature: FeatureId,
    sketch: &Sketch,
    sketch_key: Key,
    found: Vec<Found>,
    document: &Document,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) {
    let within = document.tolerance().resolution();
    let mut stale = Vec::new();
    for (link, shape) in found {
        match shape {
            Err(why) => evaluation.broken.push((feature, link, why)),
            Ok(shape) => {
                if !sketch.link_follows(link, &shape, within) {
                    stale.push((link, shape));
                }
            }
        }
    }
    if stale.is_empty() {
        return;
    }
    let key = Keyer::new("relink")
        .key(sketch_key)
        .value(&stale)
        .value(&document.units())
        .finish();
    let design = document.design();
    let edit = SketchEdit::Relink(stale.clone());
    let relinked = cache.relinked(key, || {
        match varde_sketch::propose(sketch, &edit, &design, &Budget::default()) {
            Ok(accepted) => Ok(Arc::new(accepted.sketch)),
            // Not solving, it's relinked as it is: what follows the links
            // is then out of place, which the sketch not solving says.
            Err(_) => edit
                .apply(sketch, &design)
                .map(Arc::new)
                .map_err(|why| why.to_string()),
        }
    });
    match relinked {
        Ok(relinked) => evaluation.relinked.push((feature, relinked)),
        Err(why) => {
            for (link, _) in stale {
                evaluation
                    .broken
                    .push((feature, link, format!("{CANT_HOLD}: {why}")));
            }
        }
    }
}

#[cfg(test)]
mod tests;
