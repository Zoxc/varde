//! Evaluating a sweep's tool: its regions, as an extrude's, moved along
//! its path by the kernel's [`varde_kernel::sweep::sweep`]. What's done
//! with the tool (a new body, or touches and booleans) is an extrude's
//! (`Run::evaluate`).
//!
//! **The path** is built in the world from what the feature names, as
//! the features before the sweep leave it:
//!
//! - **A sketch chain**: its curves ordered end to end
//!   ([`profile::path_chain`]: lines, arcs of at most 90° and splines'
//!   fitted conics, as a split's line is; a circle or a closed spline
//!   alone, or curves joining in a loop, is a closed chain), mapped into
//!   the world by its sketch's placement: each line a line piece, each arc
//!   or circle an arc piece about its centre, turning right-handed about
//!   the sketch's normal or against it, each spline a curve piece with
//!   the sketch's normal. A sketch that isn't placed fails it ("its
//!   path's sketch isn't placed"), and so do curves gone from it ("path
//!   not found").
//! - **A model edge chain**: each edge found on its body's topology (the
//!   one drawing it keeps) by its faces' names and point, as a chamfer's
//!   ("its path edge wasn't found"); with `tangent`, every chain with the
//!   same root in [`Topology::tangent_chains`] taken in; the chains then
//!   joined end to end by their vertices (topology, not distance), each a
//!   piece by its shape ([`edge_shape`]): a line, an arc about its circle's
//!   centre (turning the way the chain runs), anything else a curve piece
//!   of its conics with no plane. A chain closed on itself (a rim) must be
//!   alone.
//! - **Joining**: a closed part must be the only one. Otherwise the
//!   first is the part with an end on the profile's plane (within the
//!   resolution, a decision on geometry stated as one; of several, the
//!   nearest the middle of the profile's box, a choice by distance, not
//!   a merge), run from that end; each next the only part with an end
//!   within the resolution of where the chain has got to (none or
//!   several: "its path's parts don't join"), the gap left carried, not
//!   closed. Every joint between pieces, inside parts and between them
//!   (but not inside a piece: a traced chain's own conics are tangent only
//!   to its fit), must be tangent-continuous, the tangents within a sine
//!   of `1e-6` of each other ("its path has a corner", the joint drawn);
//!   an open path's start must be square to the profile's plane, a sine
//!   within `1e-6` ("its profile isn't square to its path"): the
//!   kernel's rules ([`JOINT_SINE`]), which its sweep guards with too. A
//!   piece closed on itself (a circle, a rim) has no joint. A closed
//!   path's start, where the profile's plane crosses it, is the kernel's
//!   to find.
//! - **A helix**: its axis resolved as a move's turn's
//!   ([`motion::resolve_axis`]), reversed with `flip`, noted for the
//!   draft's reply ([`Evaluation::references`]), its pitch times its
//!   turns within the coordinate limit.
//!
//! The tool is cached by the profile (its sketch's key and placement and
//! the regions), the path as built (every number's bits: an edit that
//! leaves it where it was finds the tool again), the options and the
//! fit tolerance. The kernel's refusals are worded for the Timeline
//! (`message::sweep_refused`), a corner with its point drawn; its other
//! failures as a tool's.
//!
//! The kernel's sweep isn't built yet: it fails as too complex, which
//! reaches the user as "sweeping its regions along its path is too
//! complex to work out", and the rest of the history goes on.
//!
//! [`profile::path_chain`]: crate::profile::path_chain
//! [`Topology::tangent_chains`]: varde_kernel::Topology::tangent_chains
//! [`edge_shape`]: varde_kernel::measure::edge_shape
//! [`JOINT_SINE`]: varde_kernel::sweep::JOINT_SINE

use std::sync::Arc;

use glam::DVec3;
use varde_document::{
    CurveChain, EdgeRef, Helix, MAX_COORD, Orientation, PathPart, PathRef, Placement, Sweep,
};
use varde_kernel::measure::{EdgeShape, edge_shape};
use varde_kernel::patch::Conic3;
use varde_kernel::sweep::{self as path_sweep, Path, Piece, SweepError};
use varde_kernel::{Budget, Evidence, Frame, Profile, Solid, Tolerance, Topology};
use varde_sketch::Curve;

use super::{Evaluation, Failed, Run, motion};
use crate::SweepFound;
use crate::cache::{Cache, Key, Keyer};
use crate::error_geometry::{ErrorGeometry, KernelFailure};
use crate::inspect;
use crate::message::{self, SweepRefusal};
use crate::profile::{ChainError, path_chain};

/// How far apart two tangents at a joint may be, or the path's start
/// from square to the profile's plane, as a sine: the kernel's own rule,
/// which its sweep guards with.
const TANGENT_SINE: f64 = path_sweep::JOINT_SINE;

/// The kernel's sweep, which tests may replace with a stand-in to check
/// what regeneration does with the result before the kernel's is built.
type Sweeper = fn(
    &Profile,
    &Frame,
    &Path,
    path_sweep::Orientation,
    f64,
    u64,
    &Tolerance,
    &Budget,
) -> Result<Solid, SweepError>;

#[cfg(any(test, feature = "testing"))]
thread_local! {
    /// The sweep a test asks for in place of the kernel's, on its own
    /// thread.
    pub(crate) static SWEEPER: std::cell::Cell<Option<Sweeper>> =
        const { std::cell::Cell::new(None) };
}

/// The sweep to run: the kernel's, or the one a test set.
fn sweeper() -> Sweeper {
    #[cfg(any(test, feature = "testing"))]
    if let Some(sweeper) = SWEEPER.get() {
        return sweeper;
    }
    path_sweep::sweep
}

/// Sweeps on this thread by [`by_extrude`] from now on.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn sweep_by_extrude() {
    SWEEPER.set(Some(by_extrude));
}

/// A stand-in for the kernel's sweep, for tests: a path of straight
/// pieces all along one line, starting on the profile's plane and
/// square to it, untwisted, is the profile extruded along it, as far as
/// the pieces reach. Anything else is [`KernelError::TooComplex`].
///
/// [`KernelError::TooComplex`]: varde_kernel::KernelError::TooComplex
#[cfg(any(test, feature = "testing"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn by_extrude(
    profile: &Profile,
    frame: &Frame,
    path: &Path,
    _orientation: path_sweep::Orientation,
    twist: f64,
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, SweepError> {
    use varde_kernel::KernelError;
    let too_complex = || SweepError::Failed(KernelError::TooComplex.into());
    let Path::Chain {
        pieces,
        closed: false,
    } = path
    else {
        return Err(too_complex());
    };
    if twist != 0.0 {
        return Err(too_complex());
    }
    let mut ends = Vec::with_capacity(pieces.len());
    for piece in pieces {
        let Piece::Line { from, to } = *piece else {
            return Err(too_complex());
        };
        ends.push((from, to));
    }
    let (Some(&(start, _)), Some(&(_, end))) = (ends.first(), ends.last()) else {
        return Err(too_complex());
    };
    let along = (end - start).try_normalize().ok_or_else(too_complex)?;
    let resolution = tol.resolution();
    for (k, &(from, to)) in ends.iter().enumerate() {
        let way = (to - from).try_normalize().ok_or_else(too_complex)?;
        if way.cross(along).length() > 1e-9 || way.dot(along) <= 0.0 {
            return Err(too_complex());
        }
        if k > 0 && from.distance(ends[k - 1].1) > resolution {
            return Err(too_complex());
        }
    }
    let normal = frame.normal();
    if (start - frame.origin).dot(normal).abs() > resolution {
        return Err(SweepError::OffStart);
    }
    if along.cross(normal).length() > TANGENT_SINE {
        return Err(SweepError::NotSquare);
    }
    let length = (end - start).dot(normal);
    let (from, to) = if length > 0.0 {
        (0.0, length)
    } else {
        (length, 0.0)
    };
    Ok(varde_kernel::extrude(
        profile, frame, from, to, feature, tol, budget,
    )?)
}

/// One part of a path as built: its pieces end to end, and whether they
/// close on themselves.
pub(super) struct Part {
    pub(super) pieces: Vec<Piece>,
    pub(super) closed: bool,
}

impl Run<'_> {
    /// A sweep's tool solid and the key it's filed under: its regions
    /// moved along its path, built as the module's docs say on the bodies
    /// the features before it leave (`evaluation`, where a helix's axis
    /// found is noted).
    pub(super) fn swept(
        &self,
        sweep: &Sweep,
        evaluation: &mut Evaluation,
        cache: &mut Cache,
    ) -> Result<(Arc<Solid>, Key), Failed> {
        let placement = self.placement()?;
        let frame = Frame {
            origin: placement.origin,
            x: placement.x,
            y: placement.y,
        };
        let path = match &sweep.path {
            PathRef::Chain(parts) => self.chain_path(parts, &placement, evaluation, cache)?,
            PathRef::Helix(helix) => self.helix_path(helix, evaluation, cache)?,
        };
        let orientation = match sweep.orientation {
            Orientation::FollowPath => path_sweep::Orientation::Follow,
            Orientation::Keep => path_sweep::Orientation::Keep,
        };
        let found = match &path {
            Path::Chain { pieces, closed } => (!closed)
                .then(|| chain_end(pieces, &frame, orientation))
                .flatten(),
            Path::Helix(helix) => Some(helix_start(helix, self.profile_middle(&placement))),
        };
        if let Some(found) = found.filter(SweepFound::fits) {
            evaluation.swept.push((self.feature.id, found));
        }
        let twist = sweep.twist.as_ref().map_or(0.0, |twist| twist.value);
        let mut keyer = Keyer::new("sweep");
        keyer
            .number(self.feature.id.get())
            .value(&sweep.regions)
            .number(self.tolerance.fit().to_bits())
            .key(self.sketch.key)
            .placement(&placement)
            .value(&sweep.orientation)
            .number(twist.to_bits());
        path_bits(&mut keyer, &path);
        let key = keyer.finish();
        let sweep_by = sweeper();
        let solid = cache.solid(key, || {
            let profile = self.profile()?;
            sweep_by(
                &profile,
                &frame,
                &path,
                orientation,
                twist,
                self.feature.id.get(),
                &self.tolerance,
                &Budget::DEFAULT,
            )
            .map_err(|error| self.sweep_refused(error))
        })?;
        Ok((solid, key))
    }

    /// The chain path `parts` make, for a profile on the sketch placed at
    /// `placement`, as the module's docs say.
    fn chain_path(
        &self,
        parts: &[PathPart],
        placement: &Placement,
        evaluation: &Evaluation,
        cache: &mut Cache,
    ) -> Result<Path, Failed> {
        let mut built = Vec::with_capacity(parts.len());
        for part in parts {
            built.push(match part {
                PathPart::Curves(chain) => self.sketch_part(chain)?,
                PathPart::Edges { edges, tangent } => {
                    edge_part(edges, *tangent, evaluation, cache)?
                }
            });
        }
        let start = (placement.origin, placement.normal);
        join(
            built,
            start,
            || self.profile_middle(placement),
            &self.tolerance,
        )
    }

    /// The middle of the profile's box, in the world: where the start is
    /// looked for nearest among several ends on the profile's plane. The
    /// plane's origin if the profile can't be made (its failure is told
    /// when the kernel is asked).
    fn profile_middle(&self, placement: &Placement) -> DVec3 {
        let Ok(profile) = self.profile() else {
            return placement.origin;
        };
        let points = (profile.loops.iter())
            .flat_map(|lp| lp.segments.iter())
            .flat_map(|segment| [segment.conic.p0, segment.conic.p1]);
        let mut bounds: Option<(glam::DVec2, glam::DVec2)> = None;
        for p in points {
            bounds = Some(bounds.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))));
        }
        bounds.map_or(placement.origin, |(lo, hi)| {
            placement.to_world((lo + hi) * 0.5)
        })
    }

    /// The part a chain of a sketch's curves makes, in the world.
    fn sketch_part(&self, chain: &CurveChain) -> Result<Part, Failed> {
        let output = (self.sketches.iter())
            .find(|output| output.id == chain.sketch)
            .ok_or(message::PATH_SKETCH_GONE)?;
        let placement = output.placement.ok_or(message::PATH_SKETCH_NOT_PLACED)?;
        let sketch = output.sketch;
        let (segments, closed) = path_chain(
            sketch,
            &chain.curves,
            self.tolerance.resolution(),
            self.tolerance.fit(),
        )
        .map_err(|error| match error {
            ChainError::Missing => message::PATH_NOT_FOUND.to_owned(),
            ChainError::Closed => message::PATH_CLOSED_NOT_ALONE.to_owned(),
            ChainError::Branches => message::PATH_CURVES_BRANCH.to_owned(),
            ChainError::Profile(error) => message::path_curves(error),
        })?;
        let world = |p| placement.to_world(p);
        let conic = |segment: &varde_kernel::Segment| Conic3 {
            p0: world(segment.conic.p0),
            c: world(segment.conic.c),
            w: segment.conic.w,
            p1: world(segment.conic.p1),
        };
        let mut pieces: Vec<Piece> = Vec::new();
        // Each curve's segments, one after another along the chain.
        for run in segments.chunk_by(|a, b| a.curve == b.curve) {
            let id = (chain.curves.iter())
                .find(|id| u64::from(id.get()) == run[0].curve)
                .copied()
                .ok_or(message::PATH_NOT_FOUND)?;
            let entry = sketch.curve(id).ok_or(message::PATH_NOT_FOUND)?;
            let conics: Vec<Conic3> = run.iter().map(conic).collect();
            pieces.push(match &entry.curve {
                Curve::Line { .. } => Piece::Line {
                    from: conics[0].p0,
                    to: conics[conics.len() - 1].p1,
                },
                Curve::Arc { center, .. } | Curve::Circle { center, .. } => {
                    let centre = (sketch.point(*center))
                        .map(|point| point.at)
                        .ok_or(message::PATH_NOT_FOUND)?;
                    // Which way it turns: the first conic's start and
                    // control point, about the centre.
                    let first = &run[0].conic;
                    let turn = (first.p0 - centre).perp_dot(first.c - first.p0);
                    let axis = if turn >= 0.0 {
                        placement.normal
                    } else {
                        -placement.normal
                    };
                    Piece::Arc {
                        conics,
                        centre: world(centre),
                        axis,
                    }
                }
                Curve::Spline(_) => Piece::Curve {
                    conics,
                    normal: Some(placement.normal),
                },
            });
        }
        Ok(Part { pieces, closed })
    }

    /// The helix path `helix` names, its axis found on the bodies as the
    /// features before the sweep leave them and noted in `evaluation`.
    fn helix_path(
        &self,
        helix: &Helix,
        evaluation: &mut Evaluation,
        cache: &mut Cache,
    ) -> Result<Path, Failed> {
        let [point, direction] =
            motion::resolve_axis(&helix.axis, evaluation, &self.tolerance, cache)?;
        let direction = if helix.flip { -direction } else { direction };
        motion::note_reference(evaluation, self.feature.id, [point, direction]);
        let axis = direction
            .try_normalize()
            .ok_or(message::HELIX_NO_DIRECTION)?;
        let (pitch, turns) = (helix.pitch.value, helix.turns.value);
        // Both checked, at most `MAX_COORD` and `MAX_HELIX_TURNS`: the
        // product is finite.
        if pitch * turns > f64::from(MAX_COORD) {
            return Err(message::helix_too_long().into());
        }
        Ok(Path::Helix(path_sweep::Helix {
            point,
            axis,
            pitch,
            turns,
            left: helix.left_handed,
        }))
    }

    /// Why the kernel's sweep gave no solid, in words, with what to
    /// draw: a corner's point, or the failure's evidence.
    fn sweep_refused(&self, error: SweepError) -> Failed {
        let why = match error {
            SweepError::Corner { at } => return corner(at, &self.tolerance),
            SweepError::OffStart => SweepRefusal::OffStart,
            SweepError::NotSquare => SweepRefusal::NotSquare,
            SweepError::TooTight { .. } => SweepRefusal::TooTight,
            SweepError::Parallel => SweepRefusal::Parallel,
            SweepError::HelixPlane => SweepRefusal::HelixPlane,
            SweepError::ReachesAxis => SweepRefusal::ReachesAxis,
            SweepError::Pitch => SweepRefusal::Pitch,
            SweepError::IntoItself => SweepRefusal::IntoItself,
            SweepError::Failed(failure) => {
                let words = message::tool(
                    message::Making::Sweep,
                    failure.error,
                    self.tolerance.fit() <= Tolerance::MIN_FIT,
                );
                let failure = KernelFailure::new(failure, &self.tolerance);
                return Failed::kernel(words, &failure, [&[], &[]]);
            }
        };
        message::sweep_refused(why).into()
    }
}

/// How many points each conic of a curved piece is walked by, carrying
/// the profile's x along an open chain to its end.
const END_STEPS: usize = 16;

/// Where an open chain of `pieces` ends, for the sweep's twist knob
/// ([`SweepFound::End`]): its end, its tangent there, and the x of the
/// profile's `frame` carried there, by rotation-minimizing frames
/// (double reflection over the pieces' points) to follow the path, or
/// taken square to the end's tangent to keep it. `None` for a chain
/// with no length at its start or end.
fn chain_end(
    pieces: &[Piece],
    frame: &Frame,
    orientation: path_sweep::Orientation,
) -> Option<SweepFound> {
    // Points along the chain with their tangents (not unit).
    let mut walk: Vec<(DVec3, DVec3)> = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Line { from, to } => {
                walk.push((*from, *to - *from));
                walk.push((*to, *to - *from));
            }
            Piece::Arc { conics, .. } | Piece::Curve { conics, .. } => {
                for conic in conics {
                    walk.extend(
                        (0..=END_STEPS).map(|k| conic.eval_deriv(k as f64 / END_STEPS as f64)),
                    );
                }
            }
        }
    }
    let unit = |(at, way): (DVec3, DVec3)| way.try_normalize().map(|way| (at, way));
    let walk: Vec<(DVec3, DVec3)> = walk.into_iter().filter_map(unit).collect();
    let (&(_, first), &(at, tangent)) = (walk.first()?, walk.last()?);
    let square = |x: DVec3, t: DVec3| (x - t * t.dot(x)).try_normalize();
    let zero = match orientation {
        path_sweep::Orientation::Keep => {
            square(frame.x, tangent).or_else(|| square(frame.y, tangent))?
        }
        path_sweep::Orientation::Follow => {
            let mut x = square(frame.x, first)?;
            for pair in walk.windows(2) {
                let [(p0, t0), (p1, t1)] = [pair[0], pair[1]];
                let v1 = p1 - p0;
                let c1 = v1.dot(v1);
                if c1 <= 0.0 {
                    continue;
                }
                let x_l = x - v1 * (2.0 * v1.dot(x) / c1);
                let t_l = t0 - v1 * (2.0 * v1.dot(t0) / c1);
                let v2 = t1 - t_l;
                let c2 = v2.dot(v2);
                x = if c2 > 0.0 {
                    x_l - v2 * (2.0 * v2.dot(x_l) / c2)
                } else {
                    x_l
                };
            }
            square(x, tangent)?
        }
    };
    Some(SweepFound::End {
        at: at.to_array(),
        tangent: tangent.to_array(),
        zero: zero.to_array(),
    })
}

/// Where a helix starts, for its pitch's and turns' knobs
/// ([`SweepFound::Helix`]): the profile's `middle`, its foot on the
/// axis, and the axis the way it climbs.
fn helix_start(helix: &path_sweep::Helix, middle: DVec3) -> SweepFound {
    let foot = helix.point + helix.axis * (middle - helix.point).dot(helix.axis);
    SweepFound::Helix {
        middle: middle.to_array(),
        foot: foot.to_array(),
        axis: helix.axis.to_array(),
    }
}

/// "Its path has a corner", the corner at `at` drawn.
fn corner(at: DVec3, tolerance: &Tolerance) -> Failed {
    let mut evidence = Evidence::default();
    evidence.add_points([at]);
    Failed {
        message: message::sweep_refused(SweepRefusal::Corner),
        geometry: ErrorGeometry::of_evidence(&evidence, tolerance),
    }
}

/// The part a chain of model edges makes, `edges` of one body found on
/// it as the features before the sweep leave it (`evaluation`), each
/// taking in its tangent chain with `tangent`.
fn edge_part(
    edges: &[EdgeRef],
    tangent: bool,
    evaluation: &Evaluation,
    cache: &mut Cache,
) -> Result<Part, Failed> {
    let Some(first) = edges.first() else {
        return Err(message::path_edge_not_found(0, 1).into());
    };
    let made = motion::holding(first.body, evaluation).ok_or(message::PATH_EDGE_BODY_GONE)?;
    let solid = Arc::clone(&made.solid);
    let topology = inspect::topology(made, cache);
    let count = edges.len();
    let mut chains = Vec::with_capacity(count);
    for (i, edge) in edges.iter().enumerate() {
        let chain = (topology.edge(&solid, edge.faces, edge.near))
            .map_err(|_| message::path_edge_not_found(i, count))?;
        chains.push(chain);
    }
    if tangent {
        let roots = topology.tangent_chains(&solid);
        let picked: Vec<u32> = chains.iter().map(|&c| roots[c as usize]).collect();
        chains = (0..roots.len() as u32)
            .filter(|&c| picked.contains(&roots[c as usize]))
            .collect();
    }
    chains.sort_unstable();
    chains.dedup();
    let (order, closed) = order_chains(&solid, &topology, &chains)?;
    let pieces = (order.iter())
        .map(|&(chain, backwards)| chain_piece(&solid, &topology, chain, backwards))
        .collect();
    Ok(Part { pieces, closed })
}

/// `chains` of `topology` (sorted, each once) joined end to end by their
/// vertices: each chain and whether it runs backwards along the walk,
/// and whether they close. A chain closed on itself must be alone; three
/// ends at a vertex, or chains in several pieces, are refused.
fn order_chains(
    solid: &Solid,
    topology: &Topology,
    chains: &[u32],
) -> Result<(Vec<(u32, bool)>, bool), Failed> {
    let mesh = solid.mesh();
    let all = topology.chains();
    let ends: Vec<[u32; 2]> = (chains.iter())
        .map(|&c| {
            let halfedges = &all[c as usize].halfedges;
            let (first, last) = (halfedges[0], halfedges[halfedges.len() - 1]);
            [mesh.halfedge(first).start, mesh.end(last)]
        })
        .collect();
    if let Some(closed) = chains.iter().position(|&c| all[c as usize].closed) {
        return match chains.len() {
            1 => Ok((vec![(chains[closed], false)], true)),
            _ => Err(message::PATH_CLOSED_NOT_ALONE.into()),
        };
    }
    // The chain ends at each vertex, found by binary search: a long
    // tangent chain is ordered in `n log n`.
    let mut by_vertex: Vec<(u32, usize, usize)> = (0..ends.len())
        .flat_map(|i| (0..2).map(move |e| (i, e)))
        .map(|(i, e)| (ends[i][e], i, e))
        .collect();
    by_vertex.sort_unstable();
    let at = |vertex: u32| -> &[(u32, usize, usize)] {
        let from = by_vertex.partition_point(|&(v, ..)| v < vertex);
        let to = by_vertex.partition_point(|&(v, ..)| v <= vertex);
        &by_vertex[from..to]
    };
    let branch = || Failed::from(message::PATH_EDGES_BRANCH);
    let mut start = None;
    for (i, pair) in ends.iter().enumerate() {
        for (e, &vertex) in pair.iter().enumerate() {
            let meeting = at(vertex).len();
            if meeting > 2 {
                return Err(branch());
            }
            if meeting == 1 && start.is_none() {
                start = Some((i, e));
            }
        }
    }
    let (loops, (mut i, mut e)) = match start {
        Some(start) => (false, start),
        None => (true, (0, 0)),
    };
    let mut walk = Vec::with_capacity(ends.len());
    let mut used = vec![false; ends.len()];
    loop {
        used[i] = true;
        walk.push((chains[i], e == 1));
        let vertex = ends[i][1 - e];
        let next = (at(vertex).iter()).find(|&&(_, j, _)| j != i && !used[j]);
        match next {
            Some(&(_, j, f)) => (i, e) = (j, f),
            None => break,
        }
    }
    if walk.len() != ends.len() {
        return Err(branch());
    }
    Ok((walk, loops))
}

/// The piece chain `chain` of `topology` makes on `solid`, run backwards
/// with `backwards`.
fn chain_piece(solid: &Solid, topology: &Topology, chain: u32, backwards: bool) -> Piece {
    let chain = &topology.chains()[chain as usize];
    let mesh = solid.mesh();
    let mut conics: Vec<Conic3> = chain.halfedges.iter().map(|&h| mesh.curve(h)).collect();
    if backwards {
        conics.reverse();
        for conic in &mut conics {
            *conic = conic.reversed();
        }
    }
    match edge_shape(solid, chain) {
        EdgeShape::Line { .. } => Piece::Line {
            from: conics[0].p0,
            to: conics[conics.len() - 1].p1,
        },
        EdgeShape::Circle { centre, axis, .. } => Piece::Arc {
            conics,
            centre,
            axis: if backwards { -axis } else { axis },
        },
        EdgeShape::Ellipse { .. } | EdgeShape::Other => Piece::Curve {
            conics,
            normal: None,
        },
    }
}

/// The path `parts` make, joined as the module's docs say, for a profile
/// on the plane through `start.0` square to the unit `start.1`, whose
/// box's middle `middle` gives (worked out only where several ends are
/// on the plane: it takes the profile).
pub(super) fn join(
    mut parts: Vec<Part>,
    (origin, normal): (DVec3, DVec3),
    middle: impl FnOnce() -> DVec3,
    tolerance: &Tolerance,
) -> Result<Path, Failed> {
    let resolution = tolerance.resolution();
    if parts.iter().any(|part| part.closed) {
        if parts.len() > 1 {
            return Err(message::PATH_CLOSED_NOT_ALONE.into());
        }
        let pieces = parts.remove(0).pieces;
        check_joints(&pieces, true, tolerance)?;
        return Ok(Path::Chain {
            pieces,
            closed: true,
        });
    }
    let ends = |part: &Part| -> Option<[DVec3; 2]> {
        Some([part.pieces.first()?.start()?, part.pieces.last()?.end()?])
    };
    // The ends on the profile's plane; of several, the nearest its
    // middle (the first of equals).
    let mut on_plane: Vec<(usize, usize, DVec3)> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let Some(points) = ends(part) else {
            return Err(message::PATH_NOT_FOUND.into());
        };
        for (e, point) in points.into_iter().enumerate() {
            if (point - origin).dot(normal).abs() <= resolution {
                on_plane.push((i, e, point));
            }
        }
    }
    let (i, e) = match on_plane.as_slice() {
        [] => return Err(message::PATH_OFF_START.into()),
        &[(i, e, _)] => (i, e),
        several => {
            let middle = middle();
            let mut first = (several[0].0, several[0].1);
            let mut nearest = several[0].2.distance_squared(middle);
            for &(i, e, point) in &several[1..] {
                let distance = point.distance_squared(middle);
                if distance < nearest {
                    (first, nearest) = ((i, e), distance);
                }
            }
            first
        }
    };
    let mut chain = Vec::new();
    let mut part = parts.remove(i);
    if e == 1 {
        reverse(&mut part.pieces);
    }
    chain.append(&mut part.pieces);
    while !parts.is_empty() {
        let at = (chain.last())
            .and_then(Piece::end)
            .ok_or(message::PATH_NOT_FOUND)?;
        let mut found = None;
        for (i, part) in parts.iter().enumerate() {
            let points = ends(part).ok_or(message::PATH_NOT_FOUND)?;
            for (e, point) in points.into_iter().enumerate() {
                if point.distance(at) <= resolution {
                    if found.is_some() {
                        return Err(message::PATH_PARTS_APART.into());
                    }
                    found = Some((i, e));
                }
            }
        }
        let (i, e) = found.ok_or(message::PATH_PARTS_APART)?;
        let mut part = parts.remove(i);
        if e == 1 {
            reverse(&mut part.pieces);
        }
        chain.append(&mut part.pieces);
    }
    check_joints(&chain, false, tolerance)?;
    let start = chain
        .first()
        .and_then(|piece| tangents(piece).map(|t| t[0]));
    if start.is_none_or(|t| t.cross(normal).length() > TANGENT_SINE) {
        return Err(message::PATH_NOT_SQUARE.into());
    }
    Ok(Path::Chain {
        pieces: chain,
        closed: false,
    })
}

/// `pieces` run the other way.
fn reverse(pieces: &mut [Piece]) {
    pieces.reverse();
    for piece in pieces {
        match piece {
            Piece::Line { from, to } => std::mem::swap(from, to),
            Piece::Arc { conics, axis, .. } => {
                reverse_conics(conics);
                *axis = -*axis;
            }
            Piece::Curve { conics, .. } => reverse_conics(conics),
        }
    }
}

fn reverse_conics(conics: &mut [Conic3]) {
    conics.reverse();
    for conic in conics {
        *conic = conic.reversed();
    }
}

/// The unit tangents `piece` starts and ends along, the way it runs;
/// `None` where one has no direction.
fn tangents(piece: &Piece) -> Option<[DVec3; 2]> {
    match piece {
        Piece::Line { from, to } => {
            let t = (*to - *from).try_normalize()?;
            Some([t, t])
        }
        Piece::Arc { conics, .. } | Piece::Curve { conics, .. } => {
            let (first, last) = (conics.first()?, conics.last()?);
            let leaving = (first.c - first.p0)
                .try_normalize()
                .or_else(|| (first.p1 - first.p0).try_normalize())?;
            let arriving = (last.p1 - last.c)
                .try_normalize()
                .or_else(|| (last.p1 - last.p0).try_normalize())?;
            Some([leaving, arriving])
        }
    }
}

/// Checks that each of `pieces` runs on from the one before (and the
/// first from the last, if `closed`) along the same tangent, within a
/// sine of [`TANGENT_SINE`]; a corner fails with its joint drawn. A
/// piece closed on itself (a circle, a closed spline, a rim) has no
/// joint: where its last conic meets its first is inside the piece, as
/// its other conics' joints are, which a traced chain's fit leaves
/// tangent only to the tolerance.
fn check_joints(pieces: &[Piece], closed: bool, tolerance: &Tolerance) -> Result<(), Failed> {
    let count = pieces.len();
    let joints = if closed && count > 1 {
        count
    } else {
        count.saturating_sub(1)
    };
    for k in 0..joints {
        let (a, b) = (&pieces[k], &pieces[(k + 1) % count]);
        let at = b.start().unwrap_or(DVec3::ZERO);
        let (Some([_, out]), Some([into, _])) = (tangents(a), tangents(b)) else {
            return Err(corner(at, tolerance));
        };
        if out.dot(into) <= 0.0 || out.cross(into).length() > TANGENT_SINE {
            return Err(corner(at, tolerance));
        }
    }
    Ok(())
}

/// Every number of `path` into `keyer`, so the tool is filed by the path
/// as built.
fn path_bits(keyer: &mut Keyer, path: &Path) {
    let mut point = |keyer: &mut Keyer, p: DVec3| {
        for number in p.to_array() {
            keyer.number(number.to_bits());
        }
    };
    let conics =
        |keyer: &mut Keyer, conics: &[Conic3], point: &mut dyn FnMut(&mut Keyer, DVec3)| {
            keyer.number(conics.len() as u64);
            for conic in conics {
                point(keyer, conic.p0);
                point(keyer, conic.c);
                keyer.number(conic.w.to_bits());
                point(keyer, conic.p1);
            }
        };
    match path {
        Path::Chain { pieces, closed } => {
            keyer.bytes(b"chain").number(u64::from(*closed));
            keyer.number(pieces.len() as u64);
            for piece in pieces {
                match piece {
                    Piece::Line { from, to } => {
                        keyer.bytes(b"line");
                        point(keyer, *from);
                        point(keyer, *to);
                    }
                    Piece::Arc {
                        conics: arc,
                        centre,
                        axis,
                    } => {
                        keyer.bytes(b"arc");
                        conics(keyer, arc, &mut point);
                        point(keyer, *centre);
                        point(keyer, *axis);
                    }
                    Piece::Curve {
                        conics: curve,
                        normal,
                    } => {
                        keyer.bytes(b"curve");
                        conics(keyer, curve, &mut point);
                        match normal {
                            Some(normal) => {
                                keyer.number(1);
                                point(keyer, *normal);
                            }
                            None => {
                                keyer.number(0);
                            }
                        }
                    }
                }
            }
        }
        Path::Helix(helix) => {
            keyer.bytes(b"helix");
            point(keyer, helix.point);
            point(keyer, helix.axis);
            keyer
                .number(helix.pitch.to_bits())
                .number(helix.turns.to_bits())
                .number(u64::from(helix.left));
        }
    }
}
