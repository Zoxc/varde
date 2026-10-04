//! Evaluating an align: the moved body's solid, as the features before
//! it leave it, moved by the one [`Motion`] that takes the point and
//! directions picked on it onto those picked on the target
//! ([`Motion::align`]).
//!
//! Each reference is found on its body as the features before the align
//! leave it, through the kernel's topology ([`Topology::corner_point`],
//! [`Topology::middle`], [`Topology::centre`], [`Topology::normal`],
//! [`Topology::face_axis`], [`Topology::edge_direction`]), each cached by
//! its body's key, its names, its point and the fit tolerance. A
//! reference on the target side whose body a join or combine merged into
//! the moved body fails it: the target would move with what it's the
//! target of. What's found on each side is noted for the draft's reply
//! ([`Evaluation::aligned`]), whether or not the align goes on to work.
//!
//! [`Topology::corner_point`]: varde_kernel::Topology::corner_point
//! [`Topology::middle`]: varde_kernel::Topology::middle
//! [`Topology::centre`]: varde_kernel::Topology::centre
//! [`Topology::normal`]: varde_kernel::Topology::normal
//! [`Topology::face_axis`]: varde_kernel::Topology::face_axis
//! [`Topology::edge_direction`]: varde_kernel::Topology::edge_direction

use std::f64::consts::PI;
use std::sync::Arc;

use glam::DVec3;
use varde_document::{
    Align, AlignRefs, AxisRef, BodyId, DirRef, Document, EdgeRef, FaceRef, FeatureId, MAX_COORD,
    PointRef,
};
use varde_kernel::measure::{EdgeShape, edge_shape};
use varde_kernel::topology::Unresolved;
use varde_kernel::{AlignError, AlignOptions, Evidence, Motion, Solid, Tolerance, Topology};

use super::motion::{How, holding, place, reference_key};
use super::{BodySolid, Evaluation, Failed, chain_curves, face_geometry, own_solids};
use crate::cache::{Cache, Datum};
use crate::error_geometry::ErrorGeometry;
use crate::message::{self, AlignRef, Moving, Side};
use crate::{AlignDatums, AlignFound};

/// Changes the body of `evaluation` that the align `align`, the feature
/// `feature`, moves, or says why it fails, changing nothing.
///
/// The body must have a solid of its own, as a move's bodies must
/// ([`own_solids`]). Both sides are found (see the module's docs), the
/// moved side's first, and noted; the primaries meet opposed by default
/// where each is a flat face's normal or a round edge's axis, the flip
/// turning that round; the turn is stored in radians and turned in
/// degrees, as a move's is. The motion's refusals are said with the
/// references they're about. The body is then moved as a move's are
/// ([`place`]: refused past the coordinate limit, cached by the motion's
/// bits), its faces keeping their names.
pub(super) fn evaluate_align(
    document: &Document,
    feature: FeatureId,
    align: &Align,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    own_solids(document, std::iter::once(align.body), evaluation)?;
    let found = Found {
        moved: align.body,
        evaluation,
        tolerance,
    };
    let moved = found.side(&align.from, Side::Moved, cache);
    let target = found.side(&align.to, Side::Target, cache);
    let opposed = match (&moved, &target) {
        (Ok(m), Ok(t)) => (m.outward && t.outward) != align.flip,
        _ => align.flip,
    };
    note(evaluation, feature, [&moved, &target], opposed);
    let (moved, target) = (moved?, target?);
    let options = AlignOptions {
        flip: opposed,
        offset: align.offset.as_ref().map_or(0.0, |offset| offset.value),
        degrees: align
            .turn
            .as_ref()
            .map_or(0.0, |turn| turn.value / (PI / 180.0)),
    };
    let motion = Motion::align(&moved.datum, &target.datum, &options).map_err(|why| {
        let found = Found {
            moved: align.body,
            evaluation,
            tolerance,
        };
        found.refused(why, align, [&moved, &target])
    })?;
    let how = How {
        moving: Moving::Align,
        copy: None,
    };
    place(
        document,
        &[align.body],
        &motion,
        how,
        tolerance,
        evaluation,
        cache,
    )
}

/// One side of an align as found: the kernel's datum, and whether its
/// primary points out of its body ([`Datum::outward`]).
struct Resolved {
    datum: varde_kernel::Datum,
    outward: bool,
}

/// Notes what `feature` found on each side in `evaluation`, if it found
/// either and the wire takes it ([`AlignDatums::fits`]).
fn note(
    evaluation: &mut Evaluation,
    feature: FeatureId,
    sides: [&Result<Resolved, Failed>; 2],
    opposed: bool,
) {
    let [moved, target] = sides.map(|side| {
        let datum = &side.as_ref().ok()?.datum;
        let side = AlignFound {
            point: datum.point.to_array(),
            primary: datum.primary.map(|v| v.to_array()),
            secondary: datum.secondary.map(|v| v.to_array()),
        };
        side.fits().then_some(side)
    });
    let datums = AlignDatums {
        moved,
        target,
        opposed,
    };
    if datums.fits() {
        evaluation.aligned.push((feature, datums));
    }
}

/// Finds an align's references on the bodies as the features before it
/// leave them.
struct Found<'a> {
    /// The body the align moves.
    moved: BodyId,
    evaluation: &'a Evaluation,
    tolerance: &'a Tolerance,
}

impl Found<'_> {
    /// The side `refs`, on `side`, as found, or why one of its references
    /// isn't (the point's first, then the primary's, then the
    /// secondary's).
    fn side(&self, refs: &AlignRefs, side: Side, cache: &mut Cache) -> Result<Resolved, Failed> {
        let point = self.point(&refs.point, side, cache)?;
        let mut direction = |which, direction: &Option<DirRef>| {
            (direction.as_ref())
                .map(|direction| self.direction(direction, which, side, cache))
                .transpose()
        };
        let primary = direction(AlignRef::Primary, &refs.primary)?;
        let secondary = direction(AlignRef::Secondary, &refs.secondary)?;
        Ok(Resolved {
            datum: varde_kernel::Datum {
                point,
                primary: primary.map(|found| found.at),
                secondary: secondary.map(|found| found.at),
            },
            outward: primary.is_some_and(|found| found.outward),
        })
    }

    /// The body holding `body`'s solid, for a reference `which` on
    /// `side`: one with no solid is gone; on the target side, the moved
    /// body can't hold it.
    fn holding(&self, body: BodyId, which: AlignRef, side: Side) -> Result<&BodySolid, Failed> {
        let made = holding(body, self.evaluation)
            .ok_or_else(|| message::align_ref(which, side, message::ALIGN_BODY_GONE))?;
        if side == Side::Target && made.body == self.moved {
            return Err(message::align_ref(which, side, message::ALIGN_ON_MOVED).into());
        }
        Ok(made)
    }

    /// The point `point` names on `side`.
    fn point(&self, point: &PointRef, side: Side, cache: &mut Cache) -> Result<DVec3, Failed> {
        let which = AlignRef::Point;
        let found = match point {
            PointRef::Origin => return Ok(DVec3::ZERO),
            PointRef::Corner { body, faces, near } => {
                self.find(*body, "corner", faces, *near, which, side, cache, |t, s| {
                    t.corner_point(s, *faces, *near).map(Datum::point)
                })
            }
            PointRef::Middle(edge) => self.find(
                edge.body,
                "middle",
                &edge.faces,
                edge.near,
                which,
                side,
                cache,
                |t, s| t.middle(s, edge.faces, edge.near).map(Datum::point),
            ),
            PointRef::Centre(edge) => self.find(
                edge.body,
                "centre",
                &edge.faces,
                edge.near,
                which,
                side,
                cache,
                |t, s| t.centre(s, edge.faces, edge.near).map(Datum::point),
            ),
        };
        found.map(|datum| datum.at)
    }

    /// The direction `direction`, the reference `which` on `side`.
    fn direction(
        &self,
        direction: &DirRef,
        which: AlignRef,
        side: Side,
        cache: &mut Cache,
    ) -> Result<Datum, Failed> {
        match direction {
            DirRef::Origin(axis) | DirRef::Axis(AxisRef::Origin(axis)) => Ok(Datum {
                at: axis.direction(),
                outward: false,
            }),
            DirRef::Normal(face) => {
                let FaceRef { body, key, near } = *face;
                self.find(body, "normal", &key, near, which, side, cache, |t, s| {
                    let at = t.normal(s, &key, near)?;
                    Ok(Datum { at, outward: true })
                })
            }
            DirRef::Axis(AxisRef::Face(face)) => {
                let FaceRef { body, key, near } = *face;
                self.find(body, "face axis", &key, near, which, side, cache, |t, s| {
                    let [_, at] = t.face_axis(s, &key, near)?;
                    Ok(Datum { at, outward: false })
                })
            }
            DirRef::Axis(AxisRef::Edge(edge)) => {
                let EdgeRef { body, faces, near } = *edge;
                self.find(
                    body,
                    "edge axis",
                    &faces,
                    near,
                    which,
                    side,
                    cache,
                    |t, s| {
                        let chain = t.edge(s, faces, near).map_err(Unresolved::from)?;
                        let shape = edge_shape(s, &t.chains()[chain as usize]);
                        let round =
                            matches!(shape, EdgeShape::Circle { .. } | EdgeShape::Ellipse { .. });
                        let at = t.edge_direction(s, faces, near)?;
                        Ok(Datum { at, outward: round })
                    },
                )
            }
        }
    }

    /// What `find` finds on the solid holding `body`, cached by that
    /// solid's key, `kind`, the reference's `names` and `near`, which
    /// reference it is and the fit tolerance; or why not, with the face
    /// or edge found where it's of the wrong kind.
    #[expect(
        clippy::too_many_arguments,
        reason = "a reference's parts, taken apart"
    )]
    fn find(
        &self,
        body: BodyId,
        kind: &str,
        names: &impl serde::Serialize,
        near: DVec3,
        which: AlignRef,
        side: Side,
        cache: &mut Cache,
        find: impl FnOnce(&Topology, &Solid) -> Result<Datum, Unresolved>,
    ) -> Result<Datum, Failed> {
        let made = self.holding(body, which, side)?;
        let kind = format!("align {kind} {which:?} {side:?}");
        let key = reference_key(&kind, made.key, names, near, self.tolerance);
        cache.datum(key, || {
            let solid = &made.solid;
            let topology = solid.topology();
            find(&topology, solid)
                .map_err(|why| unresolved(solid, &topology, why, which, side, self.tolerance))
        })
    }

    /// Why the motion of `align`, whose sides were found as `sides`,
    /// was refused as `why`, with the references it's about.
    fn refused(&self, why: AlignError, align: &Align, sides: [&Resolved; 2]) -> Failed {
        let in_range = |p: DVec3| p.is_finite() && p.abs().max_element() <= f64::from(MAX_COORD);
        let named = [(Side::Moved, &align.from), (Side::Target, &align.to)];
        match why {
            AlignError::Parallel => {
                // The side whose own frame is refused.
                let at = (sides.iter())
                    .position(|side| {
                        let alone =
                            Motion::align(&side.datum, &side.datum, &AlignOptions::default());
                        alone == Err(AlignError::Parallel)
                    })
                    .unwrap_or(0);
                let (side, refs) = named[at];
                let shown = refs.directions().filter_map(dir_shown);
                Failed {
                    message: message::align_parallel(side),
                    geometry: self.shown(shown),
                }
            }
            AlignError::Point => {
                let at = (sides.iter())
                    .position(|side| !in_range(side.datum.point))
                    .unwrap_or(0);
                let (side, refs) = named[at];
                Failed {
                    message: message::align_too_far(side),
                    geometry: self.shown(point_shown(&refs.point)),
                }
            }
            AlignError::Direction | AlignError::Unpaired | AlignError::Options => {
                message::ALIGN_MALFORMED.into()
            }
        }
    }

    /// What to draw of the faces and edges `shown` names: each on the
    /// solid holding its body, the faces' triangles and the edges'
    /// curves, those that are found.
    fn shown<'r>(&self, shown: impl IntoIterator<Item = Shown<'r>>) -> Option<Arc<ErrorGeometry>> {
        let mut evidence = Evidence::default();
        for shown in shown {
            let body = match shown {
                Shown::Face(face) => face.body,
                Shown::Edge(edge) => edge.body,
            };
            let Some(made) = holding(body, self.evaluation) else {
                continue;
            };
            let solid = &made.solid;
            let topology = solid.topology();
            match shown {
                Shown::Face(face) => {
                    if let Ok(region) = topology.face(solid, &face.key, face.near) {
                        let tris = &topology.regions()[region as usize].tris;
                        let mesh = solid.mesh();
                        evidence.add_patches(tris.iter().map(|&tri| mesh.patch(tri as usize)));
                    }
                }
                Shown::Edge(edge) => {
                    if let Ok(chain) = topology.edge(solid, edge.faces, edge.near) {
                        let chain = &topology.chains()[chain as usize];
                        let mesh = solid.mesh();
                        let tris = mesh.tris().len();
                        evidence.add_curves(
                            (chain.halfedges.iter())
                                .filter(|&&h| (h as usize) / 3 < tris)
                                .map(|&h| mesh.curve(h)),
                        );
                    }
                }
            }
        }
        ErrorGeometry::of_evidence(&evidence, self.tolerance)
    }
}

impl Datum {
    /// A point found.
    fn point(at: DVec3) -> Datum {
        Datum { at, outward: false }
    }
}

/// A face or an edge a reference names, to draw where an align fails.
enum Shown<'r> {
    Face(&'r FaceRef),
    Edge(&'r EdgeRef),
}

/// What the direction `direction` names on a body, if it names something.
fn dir_shown(direction: &DirRef) -> Option<Shown<'_>> {
    match direction {
        DirRef::Origin(_) | DirRef::Axis(AxisRef::Origin(_)) => None,
        DirRef::Normal(face) | DirRef::Axis(AxisRef::Face(face)) => Some(Shown::Face(face)),
        DirRef::Axis(AxisRef::Edge(edge)) => Some(Shown::Edge(edge)),
    }
}

/// What the point `point` names on a body, if it's an edge's.
fn point_shown(point: &PointRef) -> Option<Shown<'_>> {
    match point {
        PointRef::Middle(edge) | PointRef::Centre(edge) => Some(Shown::Edge(edge)),
        PointRef::Origin | PointRef::Corner { .. } => None,
    }
}

/// Why the reference `which` on `side` isn't found on `solid` (whose
/// topology is `topology`), as the kernel says (`why`), with the face or
/// edge found where it's of the wrong kind: its triangles or curves, as
/// a sketch's face that isn't flat or a revolve's edge that isn't
/// straight shows itself.
fn unresolved(
    solid: &Solid,
    topology: &Topology,
    why: Unresolved,
    which: AlignRef,
    side: Side,
    tolerance: &Tolerance,
) -> Failed {
    let region =
        |region: u32| face_geometry(solid, &topology.regions()[region as usize], tolerance);
    let chain = |chain: u32| chain_curves(solid, &topology.chains()[chain as usize], tolerance);
    let (why, geometry) = match why {
        Unresolved::NotFound(_) => ("wasn't found", None),
        Unresolved::NotFlat { region: r } => ("is a face that isn't flat", region(r)),
        Unresolved::NotRound { region: r } => ("is a face that isn't round", region(r)),
        Unresolved::NotStraight { chain: c } => ("is an edge that isn't straight", chain(c)),
        Unresolved::NotCircular { chain: c } => ("is an edge that isn't round", chain(c)),
        Unresolved::NotAnAxis { chain: c } => ("is an edge that isn't straight or round", chain(c)),
        Unresolved::Undirected { chain: c } => {
            ("is an edge whose direction can't be told", chain(c))
        }
    };
    Failed {
        message: message::align_ref(which, side, why),
        geometry,
    }
}
