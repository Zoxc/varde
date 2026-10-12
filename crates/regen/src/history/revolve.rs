//! A revolve's axis: found in its sketch or on a model edge, and the
//! frame the kernel turns the profile in about it.

use std::collections::BTreeSet;
use std::sync::Arc;

use glam::{DVec2, DVec3};
use varde_document::{AxisLine, EdgeRef, MAX_COORD, Placement, Revolve, Sketch};
use varde_kernel_compat::measure::{EdgeShape, edge_shape};
use varde_kernel_compat::patch::{Conic2, Conic3};
use varde_kernel_compat::{Budget, Frame, Loop, Profile, Segment, Solid, Tolerance};
use varde_sketch::{Curve, Id};

use crate::cache::{Cache, Key, Keyer};
use crate::error_geometry::ErrorGeometry;
use crate::why::{self};

use super::{Evaluation, Failed, Run, chain_curves, motion};

impl Run<'_> {
    /// A revolve's tool solid and the key it's filed under: the regions
    /// turned about its axis over its span (see [`axis_frame`]). An axis
    /// edge is found on its body as the features before the revolve
    /// (`evaluation`) leave it ([`edge_axis`]).
    pub(super) fn revolved(
        &self,
        revolve: &Revolve,
        evaluation: &Evaluation,
        cache: &mut Cache,
    ) -> Result<(Arc<Solid>, Key), Failed> {
        let span = revolve.span();
        let placement = self.placement()?;
        let edge = match &revolve.axis {
            AxisLine::Edge(edge) => Some(edge_axis(
                edge,
                evaluation,
                &placement,
                self.tolerance,
                cache,
            )?),
            AxisLine::Curve(_) | AxisLine::SketchX | AxisLine::SketchY => None,
        };
        let mut keyer = Keyer::new("revolve");
        keyer.number(self.feature.id.get()).value(&revolve.regions);
        match &edge {
            // An edge by where it lies in the sketch, so the tool follows
            // the body under it, and a reference picked again on the same
            // edge finds it.
            Some(axis) => {
                (keyer.bytes(b"edge")).floats([axis.at.x, axis.at.y, axis.along.x, axis.along.y]);
            }
            // A line or axis of the sketch, whose key holds where it is.
            None => {
                keyer.value(&revolve.axis);
            }
        }
        let key = keyer
            .number(self.tolerance.fit().to_bits())
            .value(&span)
            .key(self.sketch.key)
            .placement(&placement)
            .finish();
        let solid = cache.solid(key, || {
            let profile = self.profile()?;
            let axis = match edge {
                Some(axis) => axis,
                None => axis_line(self.sketch.sketch, revolve.axis)
                    .map_err(|error| self.axis_failed(error, &placement))?,
            };
            let (profile, frame, same_way) = axis_frame(&profile, &axis, &placement)
                .map_err(|error| self.axis_failed(error, &placement))?;
            let sweep = match span {
                None => varde_kernel_compat::Sweep::Full,
                Some((from, to)) if same_way => varde_kernel_compat::Sweep::Part { from, to },
                Some((from, to)) => varde_kernel_compat::Sweep::Part {
                    from: -to,
                    to: -from,
                },
            };
            varde_kernel_compat::revolve(
                &profile,
                &frame,
                sweep,
                self.feature.id.get(),
                &self.tolerance,
                &Budget::DEFAULT,
            )
            .map_err(|failure| self.kernel_error(failure))
        })?;
        Ok((solid, key))
    }

    /// Why its revolve's axis gives no frame, `error`, on its sketch
    /// placed at `placement`: a line of no length shows its point and
    /// marks the line; the others show nothing (a line that's gone isn't
    /// anywhere, and a profile too far from the axis is past where
    /// anything is drawn).
    pub(super) fn axis_failed(&self, error: AxisError, placement: &Placement) -> Failed {
        let AxisError::NoLength { at, curve } = error else {
            return error.message().into();
        };
        let mut evidence = varde_kernel_compat::Evidence::default();
        evidence.add_points([placement.to_world(at)]);
        evidence.add_sketch_curves(curve.map(|id| u64::from(id.get())));
        Failed {
            missing: false,
            message: error.message().to_owned(),
            geometry: ErrorGeometry::of_evidence(&evidence, &self.tolerance),
        }
    }
}

/// A revolve's axis in its sketch's coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Axis {
    /// A point on it: the line's start, or the sketch's origin.
    pub(crate) at: DVec2,
    /// The way it points, not zero (not unit).
    pub(crate) along: DVec2,
    /// The line it is, if it's one of the sketch's curves.
    pub(crate) curve: Option<Id>,
}

/// Why a revolve's axis gives no frame to turn its profile in
/// ([`axis_line`], [`axis_frame`]), worded by [`AxisError::message`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AxisError {
    /// The curve it names isn't there or isn't a line any more.
    NotFound,
    /// The line has no length, or none a float can give a direction:
    /// where it is in the sketch, and the line if it's one of the
    /// sketch's curves.
    NoLength { at: DVec2, curve: Option<Id> },
    /// The profile moved into the axis's frame isn't within the
    /// coordinate limit.
    TooFar,
}

impl AxisError {
    /// Why the revolve fails, in words.
    pub(crate) fn message(self) -> &'static str {
        match self {
            AxisError::NotFound => why::AXIS_NOT_FOUND,
            AxisError::NoLength { .. } => why::AXIS_NO_LENGTH,
            AxisError::TooFar => why::AXIS_TOO_FAR,
        }
    }
}

/// The axis `axis` names in `sketch`, or why there's none: the curve it
/// names isn't there or isn't a line any more ([`AxisError::NotFound`]),
/// or the line has no length. A model edge isn't the sketch's: it's
/// found on its body ([`edge_axis`]), and is "not found" here.
pub(crate) fn axis_line(sketch: &Sketch, axis: AxisLine) -> Result<Axis, AxisError> {
    let (at, along, curve) = match axis {
        AxisLine::Edge(_) => return Err(AxisError::NotFound),
        AxisLine::SketchX => (DVec2::ZERO, DVec2::X, None),
        AxisLine::SketchY => (DVec2::ZERO, DVec2::Y, None),
        AxisLine::Curve(id) => {
            let entry = sketch.curve(id).ok_or(AxisError::NotFound)?;
            let Curve::Line { start, end } = entry.curve else {
                return Err(AxisError::NotFound);
            };
            let at = |point| (sketch.point(point).map(|point| point.at)).ok_or(AxisError::NotFound);
            let (start, end) = (at(start)?, at(end)?);
            // Both within the coordinate limit, so the difference is
            // finite.
            (start, end - start, Some(id))
        }
    };
    if along == DVec2::ZERO {
        return Err(AxisError::NoLength { at, curve });
    }
    Ok(Axis { at, along, curve })
}

/// The axis a revolve about the model edge `edge` turns about, in the
/// coordinates of its sketch placed at `placement`, given the bodies the
/// features before it made (`evaluation`), or why there's none. The
/// edge's body is looked for through [`Evaluation::holder`], as a sketch
/// on a face's is (one with no solid is "gone"); the edge is found on its
/// solid's [`Topology`](varde_kernel_compat::Topology) between faces of its two
/// names, the nearest to its point among several
/// ([`Topology::edge`](varde_kernel_compat::Topology::edge); none is "wasn't
/// found"); it must be a line ([`edge_shape`]; "isn't straight"),
/// directed as [`EdgeRef`] says ([`edge_ends`]). Both its ends must be
/// within the tolerance's resolution of the sketch's plane, a decision
/// on geometry stated as one ("isn't in the sketch's plane"): an edge of
/// the face a sketch is on is in its plane to the bit where the face is
/// square to the world's axes, and to rounding elsewhere. The axis is
/// the line from the first end to the second, mapped into the sketch.
/// The edge's ends are cached by the solid's key, the reference's names
/// and point and the fit tolerance (which a curved edge is drawn at), so
/// a revolve edited in other ways finds them again. An edge off the plane
/// shows itself and its ends; one that isn't straight, its curves.
pub(crate) fn edge_axis(
    edge: &EdgeRef,
    evaluation: &Evaluation,
    placement: &Placement,
    tolerance: Tolerance,
    cache: &mut Cache,
) -> Result<Axis, Failed> {
    let made = evaluation.holding(edge.body).ok_or(why::EDGE_BODY_GONE)?;
    let key = motion::reference_key("edge", made.key, &edge.faces, edge.near, &tolerance);
    let [from, to] = cache.edge(key, || edge_ends(&made.solid, edge, &tolerance))?;
    let local = |p: DVec3| {
        let height = (p - placement.origin).dot(placement.normal);
        (placement.to_sketch(p), height)
    };
    let ((at, a), (end, b)) = (local(from), local(to));
    let resolution = tolerance.resolution();
    if !(a.abs() <= resolution && b.abs() <= resolution) {
        // The edge, and its ends: those off the plane show where.
        let mut evidence = varde_kernel_compat::Evidence::default();
        evidence.add_curves(Conic3::line(from, to).ok());
        evidence.add_points([from, to]);
        return Err(Failed {
            missing: false,
            message: why::EDGE_OFF_PLANE.to_owned(),
            geometry: ErrorGeometry::of_evidence(&evidence, &tolerance),
        });
    }
    // The ends are a solid's vertices, within the coordinate limit, and
    // the placement's origin too, so the difference is finite.
    let along = end - at;
    if along == DVec2::ZERO {
        let mut evidence = varde_kernel_compat::Evidence::default();
        evidence.add_points([from]);
        return Err(Failed {
            missing: false,
            message: why::AXIS_NO_LENGTH.to_owned(),
            geometry: ErrorGeometry::of_evidence(&evidence, &tolerance),
        });
    }
    Ok(Axis {
        at,
        along,
        curve: None,
    })
}

/// The ends of the straight edge `edge` names on `solid`, in the order
/// it runs with the face of its first key on its left seen from outside
/// (see [`EdgeRef`]), or why there's none: no edge has its names ("wasn't
/// found"), the one found isn't a line ("isn't straight"), or aliases
/// name both its faces by both keys so which is the first key's can't be
/// told ("direction can't be told"). A chain's halfedges run on its
/// first region's side, along that region's own boundary (its triangles
/// run round anticlockwise seen from outside, a mirrored copy's too, as
/// a mirror reverses them), so they run the right way where that region
/// is the first key's ([`EdgeRef::runs_with`]). An edge that isn't
/// straight shows its curves, drawn at the
/// [`Display`](varde_kernel_compat::Display) of `tolerance` (the first
/// [`MAX_EVIDENCE`](varde_kernel_compat::MAX_EVIDENCE)`.curves`), by value as a
/// face that isn't flat is ([`face_geometry`](super::face_geometry)); one whose direction
/// can't be told, itself.
pub(crate) fn edge_ends(
    solid: &Solid,
    edge: &EdgeRef,
    tolerance: &Tolerance,
) -> Result<[DVec3; 2], Failed> {
    let topology = solid.topology();
    let chain = (topology.edge(solid, edge.faces, edge.near)).map_err(|_| why::EDGE_NOT_FOUND)?;
    let chain = &topology.chains()[chain as usize];
    let EdgeShape::Line { from, to } = edge_shape(solid, chain) else {
        return Err(Failed {
            missing: false,
            message: why::EDGE_NOT_STRAIGHT.to_owned(),
            geometry: chain_curves(solid, chain, tolerance),
        });
    };
    match chain_runs_with(&topology, chain, edge) {
        Some(true) => Ok([from, to]),
        Some(false) => Ok([to, from]),
        None => {
            let mut evidence = varde_kernel_compat::Evidence::default();
            evidence.add_curves(Conic3::line(from, to).ok());
            Err(Failed {
                missing: false,
                message: why::EDGE_UNDIRECTED.to_owned(),
                geometry: ErrorGeometry::of_evidence(&evidence, tolerance),
            })
        }
    }
}

/// Whether `chain` of `topology` runs the way `edge`, a reference to it,
/// directs it ([`EdgeRef::runs_with`]): its halfedges run on its first
/// region's side, along that region's own boundary (its triangles run
/// round anticlockwise seen from outside, a mirrored copy's too, as a
/// mirror reverses them), so they run the right way where that region is
/// the first key's. `None` where aliases name both its faces by both
/// keys and that can't be told.
pub(crate) fn chain_runs_with(
    topology: &varde_kernel_compat::Topology,
    chain: &varde_kernel_compat::topology::Chain,
    edge: &EdgeRef,
) -> Option<bool> {
    let [left, right] = chain.regions.map(|r| {
        let region = &topology.regions()[r as usize];
        (&region.key, &region.aliases[..])
    });
    EdgeRef::runs_with(&edge.faces, left, right)
}

/// `profile`, in its sketch's coordinates, moved into the frame the
/// kernel revolves it on about `axis`, and that frame, on the sketch
/// placed at `placement`; and whether the kernel's angles turn as the
/// revolve's do (otherwise they're the other way).
///
/// The frame's origin is the axis's point, its `y` along the axis and its
/// `x` square to it in the sketch's plane, toward the profile: to the
/// side of the profile's point farthest from the axis (of its segments'
/// ends and middles). `y` points along or against the axis so that `x ×
/// y` is the sketch's normal: the map is a rigid motion of the plane,
/// keeping the loops' turning. The kernel turns `x` toward `x × y`,
/// right-handed about `−y`, and the revolve right-handed about the
/// axis's direction, so they agree where `y` points against the axis.
///
/// The ends of the segments of the axis line itself, and every segment
/// end at the same point, are put at `x = 0` exactly (and the axis
/// segments' control points); the kernel puts the others within its
/// resolution there, and refuses any reaching across. Fails if the axis
/// has no direction a float can hold, or the profile moved isn't within
/// the coordinate limit.
pub(crate) fn axis_frame(
    profile: &Profile,
    axis: &Axis,
    placement: &Placement,
) -> Result<(Profile, Frame, bool), AxisError> {
    let along = (axis.along.try_normalize()).ok_or(AxisError::NoLength {
        at: axis.at,
        curve: axis.curve,
    })?;
    let left = along.perp();
    let segments = || profile.loops.iter().flat_map(|lp| lp.segments.iter());
    // The side of the farthest point; the first of equals.
    let mut far = 0.0_f64;
    for segment in segments() {
        let c = &segment.conic;
        // The conic's middle; its weight is positive.
        let middle = (c.p0 + c.c * (2.0 * c.w) + c.p1) / (2.0 + 2.0 * c.w);
        for p in [c.p0, middle, c.p1] {
            let distance = (p - axis.at).dot(left);
            if distance.abs() > far.abs() {
                far = distance;
            }
        }
    }
    let same_way = far >= 0.0;
    let (x, y) = if same_way {
        (left, -along)
    } else {
        (-left, along)
    };
    let map = |p: DVec2| {
        let q = p - axis.at;
        DVec2::new(q.dot(x), q.dot(y))
    };
    let on_axis =
        |segment: &Segment| (axis.curve).is_some_and(|id| segment.curve == u64::from(id.get()));
    // The ends of the axis line's own segments, as mapped.
    let bits = |p: DVec2| (p.x.to_bits(), p.y.to_bits());
    let ends: BTreeSet<(u64, u64)> = segments()
        .filter(|segment| on_axis(segment))
        .flat_map(|segment| [map(segment.conic.p0), map(segment.conic.p1)])
        .map(bits)
        .collect();
    let snap = |p: DVec2| {
        let p = map(p);
        if ends.contains(&bits(p)) {
            DVec2::new(0.0, p.y)
        } else {
            p
        }
    };
    let max = f64::from(MAX_COORD);
    let within = |p: DVec2| p.is_finite() && p.abs().max_element() <= max;
    let mut moved = Profile::default();
    for lp in &profile.loops {
        let mut segments = Vec::with_capacity(lp.segments.len());
        for segment in &lp.segments {
            let c = segment.conic;
            let mut conic = Conic2 {
                p0: snap(c.p0),
                c: map(c.c),
                w: c.w,
                p1: snap(c.p1),
            };
            if on_axis(segment) {
                conic.c.x = 0.0;
            }
            if ![conic.p0, conic.c, conic.p1].into_iter().all(within) {
                return Err(AxisError::TooFar);
            }
            segments.push(Segment {
                conic,
                curve: segment.curve,
            });
        }
        moved.loops.push(Loop { segments });
    }
    let world = |v: DVec2| placement.x * v.x + placement.y * v.y;
    let frame = Frame {
        origin: placement.to_world(axis.at),
        x: world(x),
        y: world(y),
    };
    Ok((moved, frame, same_way))
}
