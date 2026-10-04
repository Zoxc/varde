//! Random histories of blocks and discs made as bodies, joins and cuts,
//! sketches of paths (lines up and down the world's z on XZ and YZ, in
//! one piece or several, bent, slanted, starting off the profile's plane,
//! lying in it, a circle) and of small profiles on XY, and sweeps (the
//! kernel's sweep replaced by the stand-in extruding along straight
//! paths) of a profile along one to three parts (a path sketch's curves,
//! all or some; upright or other edges of a body as the history left it,
//! now and then named on the body their maker made, tangent chains taken
//! in or not), or round a helix (about an origin axis or an edge, its
//! pitch times its turns now and then past the coordinate limit), kept
//! upright or following, twisted or not, making a body, joining or
//! cutting; earlier sweeps edited, path sketches redrawn (the sweep
//! following), upstream extrudes changed, removals, undo and redo,
//! combines merging bodies, sweeps drafted. After each step: the document passes its check, the
//! cache warm and cold give the same evaluation; every sweep fails alike
//! in the whole history and in the history ending with it, one that
//! failed changed nothing, and one that worked had a path the stand-in
//! can sweep, worked out here apart from regeneration (its pieces
//! straight up or down one line along the profile's normal, end to end
//! from the profile's plane without gaps or overlaps, untwisted), and
//! gave exactly what an extrude of its regions as far as the path goes
//! gives; every later edit can still be made; the document survives its
//! bytes, flipped bits included; and a request with a sweep's draft and
//! its answer cross the wire as they went.

use varde_document::Placement;
use varde_kernel::Topology;

use super::super::combine::fuzz::{
    Rng, bytes, not_stuck, random_combine, random_shape, same_bodies, solid, truncated, wire,
};
use super::*;
use crate::{Draft, Regenerator};

/// A sketch's drawing, to be drawn.
type Drawing = Box<dyn FnOnce(&mut Sketch)>;

/// The heights path lines run between, on the grid the blocks are on.
const HEIGHTS: [f64; 6] = [-10.0, 0.0, 5.0, 10.0, 15.0, 20.0];

/// A path sketch's drawing at random: on XZ or YZ (or now and then XY),
/// lines along one upright line at `u` (0, 5 or 10, so XZ's and YZ's
/// meet on the z axis and run along the blocks' corners), straight up or
/// down from the profile's plane in one to three pieces, or bent,
/// slanted, starting off the plane; or a circle.
fn random_path(rng: &mut Rng) -> (OriginPlane, Drawing) {
    let plane = match rng.below(9) {
        0..=3 => OriginPlane::XZ,
        4..=7 => OriginPlane::YZ,
        _ => OriginPlane::XY,
    };
    let u = 5.0 * rng.below(3) as f64;
    let mut points = vec![(u, 0.0)];
    match rng.below(10) {
        // A circle: a closed path.
        0 => return (plane, Box::new(disc((u, 5.0), 2.5))),
        // Bent at a corner.
        1 => points.extend([(u, 5.0), (u + 5.0, 10.0)]),
        // Slanted.
        2 => points.push((u + 1.0, 10.0)),
        // Off the profile's plane.
        3 => points = vec![(u, 5.0), (u, 15.0)],
        _ => {
            let down = rng.below(5) == 0;
            for _ in 0..1 + rng.below(3) {
                let last = points.last().unwrap().1;
                let step = 5.0 * (1 + rng.below(2)) as f64;
                points.push((u, if down { last - step } else { last + step }));
            }
            if rng.below(4) == 0 {
                // From the top down instead.
                points.reverse();
            }
        }
    }
    if rng.below(5) == 0 {
        // Lifted, to meet another part's end.
        let lift = HEIGHTS[rng.below(HEIGHTS.len())];
        for point in &mut points {
            point.1 += lift;
        }
    }
    (plane, Box::new(polyline(points)))
}

/// A small profile on XY on the blocks' grid: a square or a disc.
fn random_profile(rng: &mut Rng) -> Drawing {
    let x = 5.0 * rng.below(5) as f64 - 2.5;
    let y = 5.0 * rng.below(3) as f64 - 2.5;
    if rng.below(4) == 0 {
        Box::new(disc((x, y), 1.5))
    } else {
        Box::new(rectangle((x, y), (x + 2.0, y + 2.0)))
    }
}

/// The sketches of `document` and whether each is a profile's (on XY
/// with regions) or not.
fn sketches(document: &Document) -> Vec<(FeatureId, bool)> {
    (document.features().iter())
        .filter_map(|feature| match &feature.kind {
            FeatureKind::Sketch { sketch, plane, .. } => {
                let regions = sketch
                    .profiles()
                    .is_ok_and(|profiles| !profiles.regions.is_empty());
                let xy = *plane == Plane::Origin(OriginPlane::XY);
                Some((feature.id, regions && xy))
            }
            _ => None,
        })
        .collect()
}

/// An edge of `made`'s solid at random, upright ones mostly, named by
/// its faces' keys and its middle, on `body`.
fn random_edge(made: &crate::history::BodySolid, body: BodyId, rng: &mut Rng) -> Option<EdgeRef> {
    let solid = &made.solid;
    let topology = solid.topology();
    let chains = topology.chains();
    if chains.is_empty() {
        return None;
    }
    let upright: Vec<usize> = (0..chains.len())
        .filter(|&c| {
            matches!(edge_shape(solid, &chains[c]),
                EdgeShape::Line { from, to } if from.x == to.x && from.y == to.y)
        })
        .collect();
    let chain = if !upright.is_empty() && rng.below(4) != 0 {
        upright[rng.below(upright.len())]
    } else {
        rng.below(chains.len())
    };
    let chain = &chains[chain];
    let mesh = solid.mesh();
    let halfedge = chain.halfedges[rng.below(chain.halfedges.len())];
    let conic = mesh.curve(halfedge);
    let mut faces = chain.regions.map(|r| topology.regions()[r as usize].key);
    faces.sort();
    Some(EdgeRef {
        body,
        faces,
        near: conic.eval(0.5),
    })
}

/// A part of a path at random: a path sketch's curves, all of them or
/// some, or one or two edges of a body as `document` regenerates.
fn random_part(
    document: &Document,
    evaluation: &Evaluation,
    profile: FeatureId,
    rng: &mut Rng,
) -> Option<PathPart> {
    let paths: Vec<FeatureId> = (sketches(document).into_iter())
        .filter(|&(id, _)| id != profile)
        .map(|(id, _)| id)
        .collect();
    if rng.below(3) != 0 && !paths.is_empty() {
        let sketch = paths[rng.below(paths.len())];
        let FeatureKind::Sketch { sketch: drawn, .. } = &document.feature(sketch)?.kind else {
            return None;
        };
        let mut curves: Vec<Id> = drawn.curves.iter().map(|entry| entry.id).collect();
        if curves.len() > 1 && rng.below(4) == 0 {
            curves.remove(rng.below(curves.len()));
        }
        curves.sort();
        return (!curves.is_empty()).then_some(PathPart::Curves(CurveChain { sketch, curves }));
    }
    let made = evaluation
        .bodies
        .get(rng.below(evaluation.bodies.len().max(1)))?;
    let mut body = made.body;
    let mut edges = Vec::new();
    for _ in 0..1 + rng.below(2) {
        edges.push(random_edge(made, body, rng)?);
    }
    if rng.below(6) == 0 {
        let maker = edges[0].faces[0].feature;
        if let Some(other) = (document.bodies().iter()).find(|b| b.created_by.get() == maker) {
            body = other.id;
            for edge in &mut edges {
                edge.body = body;
            }
        }
    }
    edges.sort_by(EdgeRef::order);
    edges.dedup_by(|a, b| a.order(b).is_eq());
    Some(PathPart::Edges {
        edges,
        tangent: rng.below(2) == 0,
    })
}

/// A random helix about an origin axis or an edge of a body, its pitch
/// times its turns now and then past the coordinate limit.
fn random_helix(document: &Document, evaluation: &Evaluation, rng: &mut Rng) -> Option<Helix> {
    let design = document.design();
    let axis = match evaluation
        .bodies
        .get(rng.below(evaluation.bodies.len() + 2))
    {
        Some(made) => AxisRef::Edge(random_edge(made, made.body, rng)?),
        None => AxisRef::Origin([Axis3::X, Axis3::Y, Axis3::Z][rng.below(3)]),
    };
    let pitch = ["1", "10", "2.5", "1e6", "0.001"][rng.below(5)];
    let turns = ["0.001", "1", "5", "1000", "999.5"][rng.below(5)];
    Some(Helix {
        axis,
        pitch: Value::new(pitch, &Sweep::pitch_ask(&design)).ok()?,
        turns: Value::new(turns, &Sweep::turns_ask(&design)).ok()?,
        left_handed: rng.below(2) == 0,
        flip: rng.below(2) == 0,
    })
}

/// A twist at random: none mostly, nothing, or a quarter turn.
fn random_twist(document: &Document, rng: &mut Rng) -> Option<Value> {
    let text = match rng.below(4) {
        0 => "0",
        1 => "90",
        _ => return None,
    };
    Value::new(text, &Sweep::twist_ask(&document.design())).ok()
}

fn random_operation(rng: &mut Rng) -> Operation {
    match rng.below(5) {
        0 => Operation::Join(Targets::default()),
        1 => Operation::Cut(Targets::default()),
        _ => Operation::NewBody(BodyId::NEW),
    }
}

/// A random sweep of a profile sketch of `document`, along one to three
/// random parts or round a random helix.
fn random_sweep(document: &Document, rng: &mut Rng) -> Option<Sweep> {
    let profiles: Vec<FeatureId> = (sketches(document).into_iter())
        .filter(|&(_, profile)| profile)
        .map(|(id, _)| id)
        .collect();
    if profiles.is_empty() {
        return None;
    }
    let profile = profiles[rng.below(profiles.len())];
    let FeatureKind::Sketch { sketch, .. } = &document.feature(profile)?.kind else {
        return None;
    };
    let regions_of = sketch.profiles().ok()?;
    let regions = (0..regions_of.regions.len())
        .filter_map(|index| regions_of.reference(index))
        .collect();
    let evaluation = evaluate(document, &mut Cache::default());
    let helix = rng.below(6) == 0;
    let path = if helix {
        PathRef::Helix(random_helix(document, &evaluation, rng)?)
    } else {
        let mut parts = Vec::new();
        for _ in 0..1 + rng.below(3) {
            parts.extend(random_part(document, &evaluation, profile, rng));
        }
        if parts.is_empty() {
            return None;
        }
        PathRef::Chain(parts)
    };
    Some(Sweep {
        sketch: profile,
        regions,
        path,
        orientation: if !helix && rng.below(3) == 0 {
            Orientation::Keep
        } else {
            Orientation::FollowPath
        },
        twist: if helix {
            None
        } else {
            random_twist(document, rng)
        },
        operation: random_operation(rng),
    })
}

/// One of `sweep`'s parts changed: its orientation, twist, operation, a
/// part taken out, added or its tangent flag, or its path made a helix
/// or a chain.
fn tweaked(sweep: &Sweep, document: &Document, rng: &mut Rng) -> Sweep {
    let mut sweep = sweep.clone();
    let evaluation = evaluate(document, &mut Cache::default());
    let helix = matches!(sweep.path, PathRef::Helix(_));
    match rng.below(6) {
        0 if !helix => {
            sweep.orientation = match sweep.orientation {
                Orientation::Keep => Orientation::FollowPath,
                Orientation::FollowPath => Orientation::Keep,
            }
        }
        1 if !helix => sweep.twist = random_twist(document, rng),
        2 => {
            sweep.operation = match (&sweep.operation, rng.below(3)) {
                // A body it makes stays its.
                (Operation::NewBody(body), _) => Operation::NewBody(*body),
                (_, 0) => Operation::Join(Targets::default()),
                _ => Operation::Cut(Targets::default()),
            }
        }
        3 => match &mut sweep.path {
            PathRef::Chain(parts) if parts.len() > 1 => {
                parts.remove(rng.below(parts.len()));
            }
            PathRef::Chain(parts) => {
                if let Some(part) = random_part(document, &evaluation, sweep.sketch, rng) {
                    parts.push(part);
                }
            }
            PathRef::Helix(helix) => helix.flip = !helix.flip,
        },
        4 => {
            if let PathRef::Chain(parts) = &mut sweep.path {
                for part in parts {
                    if let PathPart::Edges { tangent, .. } = part {
                        *tangent = !*tangent;
                    }
                }
            }
        }
        _ => {
            if helix {
                if let Some(part) = random_part(document, &evaluation, sweep.sketch, rng) {
                    sweep.path = PathRef::Chain(vec![part]);
                }
            } else if let Some(helix) = random_helix(document, &evaluation, rng) {
                sweep.path = PathRef::Helix(helix);
                sweep.orientation = Orientation::FollowPath;
                sweep.twist = None;
            }
        }
    }
    sweep
}

/// The straight segments `sweep`'s path is made of, in the world, as
/// `before` (the history before it) leaves its bodies: each line of a
/// part's sketch placed by its plane, each edge's chain (and its tangent
/// chain with `tangent`) found on its body's holder's topology. `None`
/// for a path that isn't all straight lines or names what isn't there.
fn segments(document: &Document, before: &Evaluation, sweep: &Sweep) -> Option<Vec<[DVec3; 2]>> {
    let PathRef::Chain(parts) = &sweep.path else {
        return None;
    };
    let mut segments = Vec::new();
    for part in parts {
        match part {
            PathPart::Curves(chain) => {
                let FeatureKind::Sketch {
                    sketch,
                    plane: Plane::Origin(plane),
                    ..
                } = &document.feature(chain.sketch)?.kind
                else {
                    return None;
                };
                let placement: Placement = plane.placement();
                for &id in &chain.curves {
                    let Curve::Line { start, end } = sketch.curve(id)?.curve else {
                        return None;
                    };
                    let at = |point| sketch.point(point).map(|p| placement.to_world(p.at));
                    segments.push([at(start)?, at(end)?]);
                }
            }
            PathPart::Edges { edges, tangent } => {
                let holder = before.holder(edges[0].body)?;
                let solid = solid(before, holder)?;
                let topology: &Topology = &solid.topology();
                let mut chains = Vec::new();
                for edge in edges {
                    chains.push(topology.edge(solid, edge.faces, edge.near).ok()?);
                }
                if *tangent {
                    let roots = topology.tangent_chains(solid);
                    let picked: Vec<u32> = chains.iter().map(|&c| roots[c as usize]).collect();
                    chains = (0..roots.len() as u32)
                        .filter(|&c| picked.contains(&roots[c as usize]))
                        .collect();
                }
                chains.sort_unstable();
                chains.dedup();
                for chain in chains {
                    let EdgeShape::Line { from, to } =
                        edge_shape(solid, &topology.chains()[chain as usize])
                    else {
                        return None;
                    };
                    segments.push([from, to]);
                }
            }
        }
    }
    Some(segments)
}

/// How far along z from the profile's plane (XY) the stand-in sweeps
/// `sweep`, if its path is one it can: every segment up or down one line
/// along z, end to end from z 0 to there without a gap or an overlap,
/// and no twist. Worked out apart from regeneration's joining.
fn swept_length(document: &Document, before: &Evaluation, sweep: &Sweep) -> Option<f64> {
    if sweep.twist.as_ref().is_some_and(|twist| twist.value != 0.0) {
        return None;
    }
    let segments = segments(document, before, sweep)?;
    let [first, _] = *segments.first()?;
    let mut spans = Vec::new();
    for [a, b] in segments {
        if a.x != first.x || a.y != first.y || b.x != first.x || b.y != first.y || a.z == b.z {
            return None;
        }
        spans.push((a.z.min(b.z), a.z.max(b.z)));
    }
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    for pair in spans.windows(2) {
        if pair[0].1 != pair[1].0 {
            return None;
        }
    }
    let (low, high) = (spans[0].0, spans[spans.len() - 1].1);
    match (low == 0.0, high == 0.0) {
        (true, false) => Some(high),
        (false, true) => Some(low),
        _ => None,
    }
}

/// `document` up to feature `index`, then an extrude in the sweep's
/// place of its regions as far as `length` along z, doing what it does.
fn extruded_instead(document: &Document, index: usize, sweep: &Sweep, length: f64) -> Document {
    let mut editor = Editor::new(truncated(document, index));
    let text = format!("{}", length.abs());
    let operation = match &sweep.operation {
        Operation::NewBody(_) => Operation::NewBody(BodyId::NEW),
        other => other.clone(),
    };
    let extrude = Extrude {
        taper: None,
        sketch: sweep.sketch,
        regions: sweep.regions.clone(),
        extent: Extent::OneSide(super::length(editor.document(), &text)),
        flip: length < 0.0,
        operation,
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    editor.document().clone()
}

/// Whether `a` and `b` hold bodies of the same volumes and boxes, in
/// order (the one a feature makes may have another id).
fn alike(a: &Evaluation, b: &Evaluation) -> bool {
    a.bodies.len() == b.bodies.len()
        && (a.bodies.iter().zip(&b.bodies)).all(|(x, y)| {
            let (vx, vy) = (x.solid.volume(), y.solid.volume());
            let (bx, by) = (x.solid.bounds3(), y.solid.bounds3());
            (vx - vy).abs() <= 1e-9 * vx.abs().max(1.0)
                && bx.zip(by).is_some_and(|(bx, by)| {
                    bx.min.distance(by.min) <= 1e-9 && bx.max.distance(by.max) <= 1e-9
                })
        })
}

/// Holds every sweep of `document` (evaluated whole as `evaluation`) to
/// what the module's docs say, against the history before and after it.
fn check_sweeps(document: &Document, evaluation: &Evaluation, cache: &mut Cache, what: &str) {
    for (index, feature) in document.features().iter().enumerate() {
        let FeatureKind::Sweep(sweep) = &feature.kind else {
            continue;
        };
        let what = format!("{what}: sweep {index} {sweep:?}");
        let fails = |e: &Evaluation| e.failed.iter().any(|f| f.feature == feature.id);
        let before = evaluate(&truncated(document, index), cache);
        let after = evaluate(&truncated(document, index + 1), cache);
        assert_eq!(fails(evaluation), fails(&after), "{what}");
        if fails(&after) {
            assert!(
                same_bodies(&before, &after),
                "{what}: failing, it changed bodies"
            );
            continue;
        }
        let length = swept_length(document, &before, sweep)
            .unwrap_or_else(|| panic!("{what}: worked on a path the stand-in can't sweep"));
        let instead = evaluate(&extruded_instead(document, index, sweep, length), cache);
        assert!(instead.failed.len() == before.failed.len(), "{what}");
        assert!(alike(&after, &instead), "{what}: not the extrude's");
        assert_eq!(after.merged.len(), instead.merged.len(), "{what}");
    }
}

fn run(seed: u64, steps: usize) {
    with_extrudes();
    let mut rng = Rng(0x3c6e_f372_fe94_f82b ^ (seed + 1).wrapping_mul(0x2f69_3b1d));
    let mut editor = Editor::new(Document::default());
    let mut cache = Cache::default();
    let mut regenerator = Regenerator::default();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let document = editor.document().clone();
        let roll = if step < 3 { step } else { rng.below(17) };
        let mut draft = None;
        let mut apply = |command: Command| {
            let _ = editor.apply(command);
        };
        match roll {
            0 | 3 => {
                let height = ["5", "10", "15"][rng.below(3)];
                let operation = match rng.below(6) {
                    0 => Operation::Join(Targets::default()),
                    1 => Operation::Cut(Targets::default()),
                    _ => Operation::NewBody(BodyId::NEW),
                };
                add_extrude(
                    &mut editor,
                    random_shape(&mut rng),
                    Extent::OneSide(length(&document, height)),
                    operation,
                );
            }
            1 | 4 => {
                let (plane, draw) = random_path(&mut rng);
                sketch_on(&mut editor, plane, draw);
            }
            2 | 5 => {
                sketch_on(&mut editor, OriginPlane::XY, random_profile(&mut rng));
            }
            6..=8 => {
                if let Some(sweep) = random_sweep(&document, &mut rng) {
                    apply(document.add_feature(sweep.into()));
                }
            }
            9 => {
                // An earlier sweep edited.
                let sweeps: Vec<(FeatureId, Sweep)> = (document.features().iter())
                    .filter_map(|feature| match &feature.kind {
                        FeatureKind::Sweep(sweep) => Some((feature.id, sweep.clone())),
                        _ => None,
                    })
                    .collect();
                if !sweeps.is_empty() {
                    let (id, sweep) = &sweeps[rng.below(sweeps.len())];
                    let kind = tweaked(sweep, &document, &mut rng);
                    apply(Command::SetFeature {
                        feature: *id,
                        kind: Box::new(kind.into()),
                    });
                }
            }
            10 => {
                // A path sketch redrawn: the same ids where it has as many
                // curves, so the sweeps along it follow.
                let paths: Vec<FeatureId> = (sketches(&document).into_iter())
                    .filter(|&(_, profile)| !profile)
                    .map(|(id, _)| id)
                    .collect();
                if !paths.is_empty() {
                    let (_, draw) = random_path(&mut rng);
                    redraw(&mut editor, paths[rng.below(paths.len())], draw);
                }
            }
            11 => {
                // Upstream: an extrude's height or direction.
                let extrudes: Vec<&varde_document::Feature> = (document.features().iter())
                    .filter(|f| matches!(f.kind, FeatureKind::Extrude(_)))
                    .collect();
                if !extrudes.is_empty() {
                    let feature = extrudes[rng.below(extrudes.len())];
                    let FeatureKind::Extrude(mut extrude) = feature.kind.clone() else {
                        unreachable!()
                    };
                    let height = ["5", "10", "20"][rng.below(3)];
                    extrude.extent = Extent::OneSide(length(&document, height));
                    extrude.flip ^= rng.below(4) == 0;
                    apply(Command::SetFeature {
                        feature: feature.id,
                        kind: Box::new(extrude.into()),
                    });
                }
            }
            12 => {
                let features = document.features();
                if !features.is_empty() {
                    apply(Command::RemoveFeature(
                        features[rng.below(features.len())].id,
                    ));
                }
            }
            15 => {
                // Bodies merged: a path's edges then found on the holder.
                if let Some(combine) = random_combine(&document, &mut rng, None) {
                    apply(document.add_feature(combine.into()));
                }
            }
            13 if editor.can_undo() => editor.undo(),
            14 if editor.can_redo() => editor.redo(),
            _ => {
                // A sweep drafted, new or in place of an earlier one.
                let sweeps: Vec<(FeatureId, Sweep)> = (document.features().iter())
                    .filter_map(|f| match &f.kind {
                        FeatureKind::Sweep(sweep) => Some((f.id, sweep.clone())),
                        _ => None,
                    })
                    .collect();
                let edited = (!sweeps.is_empty() && rng.below(2) == 0)
                    .then(|| &sweeps[rng.below(sweeps.len())]);
                let kind = match edited {
                    Some((_, sweep)) => Some(tweaked(sweep, &document, &mut rng)),
                    None => random_sweep(&document, &mut rng),
                };
                if let Some(kind) = kind {
                    draft = Some(Draft {
                        revision: step as u64,
                        feature: edited.map(|(id, _)| *id),
                        kind: kind.into(),
                    });
                }
            }
        }
        let document = editor.document().clone();
        document
            .check()
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        cache.begin();
        let warm = evaluate(&document, &mut cache);
        let cold = evaluated(&document);
        assert!(same_bodies(&warm, &cold), "{what}: warm and cold differ");
        assert_eq!(warm.failed, cold.failed, "{what}");
        check_sweeps(&document, &warm, &mut cache, &what);
        if step % 8 == 7 {
            not_stuck(&document, &what);
            bytes(&document, &mut rng, &mut cache, &what);
        }
        wire(&editor, draft, &mut regenerator, &what);
    }
}

/// See the module's docs. `VARDE_SWEEP_SEEDS` runs more seeds
/// (`VARDE_SWEEP_FROM` the first).
#[test]
fn random_histories_with_sweeps_hold() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let from = number("VARDE_SWEEP_FROM", 0);
    for seed in from..from.saturating_add(number("VARDE_SWEEP_SEEDS", 3)) {
        run(seed, 40);
    }
}
