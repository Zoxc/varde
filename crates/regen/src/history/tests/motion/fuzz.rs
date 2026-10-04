//! Random histories of blocks and discs made as bodies, joins, cuts and
//! intersects over them, combines, and moves, mirrors, patterns and
//! aligns among them: moves by offsets and turns about origin axes,
//! straight model edges and round faces; mirrors in origin planes and
//! flat faces, keeping their originals or not; linear and circular
//! patterns of two to four copies along or about the same axes, half of
//! them of bodies patterned already, about their copies' edges and faces
//! (copies of copies), a third of them with each copy a body of its own
//! (later features then moving, mirroring, patterning, combining,
//! joining and naming the copy bodies); aligns of corners, edges'
//! middles and rims' centres, with flat faces' normals, round faces'
//! axes and edges' directions or none, onto another body's or the
//! origin's, with secondaries, flips, offsets and turns or not; earlier
//! ones edited, earlier patterns changed one way (count, tick, kind, a
//! body more or less: each copy keeps its body as it was, new ones named
//! apart), copy bodies hidden or removed with their pattern, removals,
//! undo and redo. After each step: the document passes its check, the
//! cache warm and cold give the same evaluation; every move and mirror
//! that worked put each of its bodies where the motion worked out here
//! takes it (the volume kept, the centre of mass moved, turned or
//! reflected, by `glam`'s own rotations and reflections), a mirror
//! keeping its original holding the body and its image as the boolean
//! identities say, every pattern each body and its copies (each copy's
//! centre where `glam` puts it, the whole as the copies united one by
//! one; unjoined, the body as it was and each copy the solid of its own
//! body, the copy itself), every other body left alone, the axis or
//! plane found on what it names; every align that worked noted what
//! the topology gives for its references before it, and put the moved
//! body so its point, primary and frame meet the target's as they must
//! (its centre of mass at the same place relative to them, its
//! references found again where the target's are); one that failed
//! changed nothing; every
//! later edit can still be made; the document survives its bytes,
//! flipped bits included; and a request with a move's, mirror's or
//! pattern's draft and its answer cross the wire as they went.

use std::collections::BTreeMap;

use glam::{DMat3, DQuat};
use varde_document::{Align, AlignRefs, DirRef, PointRef, Scale, ScaleFactor};
use varde_document::{Copies, Pattern, PatternKind};
use varde_kernel::measure::{EdgeShape, edge_shape};
use varde_kernel::topology::Topology;
use varde_kernel::{Budget, Instance, Motion, Op};

use super::super::combine::fuzz::{
    Rng, bytes, not_stuck, random_combine, random_shape, same_bodies, solid, truncated, wire,
};
use super::*;
use crate::{Draft, Regenerator};

const OFFSETS: [&str; 8] = ["0", "0", "5", "-5", "2.5", "12.5", "-7.5", "3.3"];
const ANGLES: [&str; 8] = ["90", "-90", "180", "30", "45", "-17.5", "270", "360"];
const PLANES: [OriginPlane; 3] = [OriginPlane::XY, OriginPlane::XZ, OriginPlane::YZ];
const COUNTS: [&str; 4] = ["2", "3", "4", "1 + 2"];
const SPACINGS: [&str; 6] = ["10", "-7.5", "25", "3", "12.5", "-40"];
const SPANS: [&str; 6] = ["360", "90", "180", "45", "270", "120"];

/// The bodies of `evaluation` that have solids of their own, made by
/// features before feature `before` of `document` (all without).
fn movable(document: &Document, evaluation: &Evaluation, before: Option<usize>) -> Vec<BodyId> {
    let index = |id: FeatureId| document.features().iter().position(|f| f.id == id);
    (evaluation.bodies.iter())
        .map(|made| made.body)
        .filter(|&body| {
            let maker = document.body(body).and_then(|body| index(body.created_by));
            maker.is_some_and(|maker| before.is_none_or(|before| maker < before))
        })
        .collect()
}

/// A point on the face `region` of `solid`: its first triangle's middle.
fn on_face(solid: &Solid, region: &varde_kernel::topology::Region) -> DVec3 {
    let patch = solid.mesh().patch(region.tris[0] as usize);
    patch.eval(DVec3::splat(1.0 / 3.0))
}

/// A random straight edge of `body` in `evaluation`, as an axis.
fn random_edge(evaluation: &Evaluation, body: BodyId, rng: &mut Rng) -> Option<AxisRef> {
    let solid = solid(evaluation, body)?;
    let topology = solid.topology();
    let lines: Vec<(usize, DVec3)> = (topology.chains().iter().enumerate())
        .filter_map(|(at, chain)| match edge_shape(solid, chain) {
            EdgeShape::Line { from, to } => Some((at, (from + to) / 2.0)),
            _ => None,
        })
        .collect();
    let &(at, near) = lines.get(rng.below(lines.len()))?;
    let [a, b] = topology.chains()[at]
        .regions
        .map(|r| topology.regions()[r as usize].key);
    // Sorted: the edge's direction is the faces'.
    let faces = (a != b).then(|| [a.min(b), a.max(b)])?;
    Some(AxisRef::Edge(EdgeRef { body, faces, near }))
}

/// A random region of `body` in `evaluation` whose form `wanted` takes,
/// as a face reference.
fn random_face(
    evaluation: &Evaluation,
    body: BodyId,
    rng: &mut Rng,
    wanted: impl Fn(&Form) -> bool,
) -> Option<FaceRef> {
    let solid = solid(evaluation, body)?;
    let topology = solid.topology();
    let regions: Vec<_> = (topology.regions().iter())
        .filter(|region| wanted(region_form(solid, region)))
        .collect();
    let region = regions.get(rng.below(regions.len()))?;
    Some(FaceRef {
        body,
        key: region.key,
        near: on_face(solid, region),
    })
}

/// Some of `bodies`, at least one, sorted.
fn some_of(bodies: &[BodyId], rng: &mut Rng) -> Vec<BodyId> {
    let mut picked: Vec<BodyId> = (bodies.iter().copied())
        .filter(|_| rng.below(3) == 0)
        .collect();
    if picked.is_empty() {
        picked.push(bodies[rng.below(bodies.len())]);
    }
    picked.sort_unstable();
    picked
}

/// A random move or mirror of `document`'s bodies made before feature
/// `before` (all without), its axis or plane named on the bodies as
/// `evaluation` (the history before it) has them.
fn random_motion(
    document: &Document,
    evaluation: &Evaluation,
    rng: &mut Rng,
    before: Option<usize>,
) -> Option<FeatureKind> {
    let bodies = movable(document, evaluation, before);
    if bodies.is_empty() {
        return None;
    }
    if rng.below(4) == 0 {
        return random_align(evaluation, &bodies, rng).map(FeatureKind::from);
    }
    if rng.below(4) == 0 {
        return random_scale(document, evaluation, &bodies, rng).map(FeatureKind::from);
    }
    let picked = some_of(&bodies, rng);
    let any = bodies[rng.below(bodies.len())];
    if rng.below(4) == 0 {
        // Half the time a pattern of bodies patterned already, about a
        // face or edge of theirs: copies of copies.
        let patterned = patterned(document, &bodies, before);
        if !patterned.is_empty() && rng.below(2) == 0 {
            let picked = some_of(&patterned, rng);
            let any = patterned[rng.below(patterned.len())];
            return Some(random_pattern(document, evaluation, rng, picked, any).into());
        }
        return Some(random_pattern(document, evaluation, rng, picked, any).into());
    }
    if rng.below(3) == 0 {
        let plane = match rng.below(3) {
            0 => random_face(evaluation, any, rng, |form| {
                matches!(form, Form::Plane { .. })
            })
            .map(PlaneRef::Face),
            _ => None,
        };
        return Some(
            Mirror {
                bodies: picked,
                plane: plane.unwrap_or(PlaneRef::Origin(PLANES[rng.below(3)])),
                keep_original: rng.below(2) == 0,
            }
            .into(),
        );
    }
    let offsets = [0, 1, 2].map(|_| OFFSETS[rng.below(OFFSETS.len())]);
    let axis = match rng.below(6) {
        0 | 1 => None,
        2 => random_edge(evaluation, any, rng),
        3 => random_face(evaluation, any, rng, |form| {
            matches!(form, Form::Cylinder { .. } | Form::Cone { .. })
        })
        .map(AxisRef::Face),
        _ => Some(AxisRef::Origin(Axis3::ALL[rng.below(3)])),
    };
    let turn = axis.map(|axis| (axis, ANGLES[rng.below(ANGLES.len())]));
    Some(shift(document, &picked, offsets, turn).into())
}

/// Those of `bodies` a pattern of `document` before feature `before`
/// (all without) patterns, sorted.
fn patterned(document: &Document, bodies: &[BodyId], before: Option<usize>) -> Vec<BodyId> {
    let features = &document.features()[..before.unwrap_or(document.features().len())];
    let mut patterned: Vec<BodyId> = (bodies.iter().copied())
        .filter(|body| {
            features.iter().any(|feature| {
                matches!(&feature.kind, FeatureKind::Pattern(pattern)
                    if pattern.bodies.binary_search(body).is_ok())
            })
        })
        .collect();
    patterned.sort_unstable();
    patterned
}

/// A random pattern of `bodies`, its axis an origin axis or named on
/// `any` as `evaluation` has it.
fn random_pattern(
    document: &Document,
    evaluation: &Evaluation,
    rng: &mut Rng,
    bodies: Vec<BodyId>,
    any: BodyId,
) -> Pattern {
    let design = document.design();
    let axis = match rng.below(5) {
        0 => random_edge(evaluation, any, rng),
        1 => random_face(evaluation, any, rng, |form| {
            matches!(form, Form::Cylinder { .. } | Form::Cone { .. })
        })
        .map(AxisRef::Face),
        _ => None,
    };
    let axis = axis.unwrap_or(AxisRef::Origin(Axis3::ALL[rng.below(3)]));
    let count = Value::new(
        COUNTS[rng.below(COUNTS.len())],
        &Pattern::count_ask(&design),
    )
    .unwrap();
    let kind = if rng.below(2) == 0 {
        PatternKind::Linear {
            along: axis,
            count,
            spacing: Value::new(
                SPACINGS[rng.below(SPACINGS.len())],
                &Pattern::spacing_ask(&design),
            )
            .unwrap(),
        }
    } else {
        PatternKind::Circular {
            about: axis,
            count,
            angle: Value::new(SPANS[rng.below(SPANS.len())], &Pattern::angle_ask(&design)).unwrap(),
        }
    };
    let copies = if rng.below(3) == 0 {
        Copies::Separate(Vec::new())
    } else {
        Copies::Joined
    };
    Pattern {
        bodies,
        kind,
        copies,
    }
}

/// `old`, the pattern feature `index` of `document`, changed one way, as
/// an edit of it might: its count, its Join to original ticked or not,
/// linear and circular swapped (the axis, count and tick kept), a body
/// added or taken out; `evaluation` is the history before it.
fn tweaked(
    document: &Document,
    evaluation: &Evaluation,
    old: &Pattern,
    index: usize,
    rng: &mut Rng,
) -> Pattern {
    let design = document.design();
    let mut pattern = old.clone();
    match rng.below(5) {
        0 => {
            let count = Value::new(
                COUNTS[rng.below(COUNTS.len())],
                &Pattern::count_ask(&design),
            )
            .unwrap();
            match &mut pattern.kind {
                PatternKind::Linear { count: was, .. }
                | PatternKind::Circular { count: was, .. } => {
                    *was = count;
                }
            }
        }
        1 => {
            pattern.copies = match pattern.copies {
                Copies::Joined => Copies::Separate(Vec::new()),
                Copies::Separate(_) => Copies::Joined,
            };
        }
        2 => {
            let axis = *pattern.kind.axis();
            let count = pattern.kind.count_value().clone();
            pattern.kind = match pattern.kind {
                PatternKind::Linear { .. } => PatternKind::Circular {
                    about: axis,
                    count,
                    angle: Value::new(SPANS[rng.below(SPANS.len())], &Pattern::angle_ask(&design))
                        .unwrap(),
                },
                PatternKind::Circular { .. } => PatternKind::Linear {
                    along: axis,
                    count,
                    spacing: Value::new(
                        SPACINGS[rng.below(SPACINGS.len())],
                        &Pattern::spacing_ask(&design),
                    )
                    .unwrap(),
                },
            };
        }
        3 if pattern.bodies.len() > 1 => {
            pattern.bodies.remove(rng.below(pattern.bodies.len()));
        }
        _ => {
            let others: Vec<BodyId> = (movable(document, evaluation, Some(index)).into_iter())
                .filter(|body| !pattern.bodies.contains(body))
                .collect();
            if !others.is_empty() {
                pattern.bodies.push(others[rng.below(others.len())]);
                pattern.bodies.sort_unstable();
            }
        }
    }
    pattern
}

/// The copy bodies of the pattern feature `feature` of `document`, by
/// the body each is a copy of and its `k`.
fn copies_of(document: &Document, feature: FeatureId) -> BTreeMap<(BodyId, u32), BodyId> {
    match document.feature(feature).map(|feature| &feature.kind) {
        Some(FeatureKind::Pattern(pattern)) => pattern
            .copy_bodies()
            .map(|(source, k, body)| ((source, k), body))
            .collect(),
        _ => BTreeMap::new(),
    }
}

/// Holds an edit of the pattern `feature` from `before` to `after` to
/// keeping each copy's body (by its original and `k`) as it was, name,
/// visibility and opacity, removing those of copies it no longer makes
/// and adding new ones, named apart from every body there was.
fn kept_copies(before: &Document, after: &Document, feature: FeatureId, what: &str) {
    let (old, new) = (copies_of(before, feature), copies_of(after, feature));
    for (copy, &body) in &new {
        match old.get(copy) {
            Some(&was) => {
                assert_eq!(body, was, "{what}: copy {copy:?} changed body");
                assert_eq!(after.body(body), before.body(was), "{what}: {body:?}");
            }
            None => {
                assert!(before.body(body).is_none(), "{what}: {body:?} reused");
                let name = &after.body(body).unwrap().name;
                assert!(
                    before.bodies().iter().all(|other| other.name != *name),
                    "{what}: {name} named twice"
                );
            }
        }
    }
    for (copy, &was) in &old {
        if !new.contains_key(copy) {
            assert!(after.body(was).is_none(), "{what}: {was:?} left behind");
        }
    }
}

/// The keys of `topology`'s regions `regions`, sorted without repeats.
fn keys_of(topology: &Topology, regions: &[u32]) -> Vec<FaceKey> {
    let mut keys: Vec<FaceKey> = (regions.iter())
        .map(|&r| topology.regions()[r as usize].key)
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// A random chain of `body`'s solid in `evaluation` whose shape `wanted`
/// takes, as an edge reference near a point of it.
fn random_chain(
    evaluation: &Evaluation,
    body: BodyId,
    rng: &mut Rng,
    wanted: impl Fn(&EdgeShape) -> bool,
) -> Option<EdgeRef> {
    let solid = solid(evaluation, body)?;
    let topology = solid.topology();
    let chains: Vec<_> = (topology.chains().iter())
        .filter(|chain| wanted(&edge_shape(solid, chain)))
        .collect();
    let chain = chains.get(rng.below(chains.len()))?;
    let [a, b] = keys_of(&topology, &chain.regions)[..] else {
        return None;
    };
    let near = solid.mesh().curve(chain.halfedges[0]).eval(0.5);
    Some(EdgeRef {
        body,
        faces: [a, b],
        near,
    })
}

fn is_line(shape: &EdgeShape) -> bool {
    matches!(shape, EdgeShape::Line { .. })
}

fn is_round(shape: &EdgeShape) -> bool {
    matches!(shape, EdgeShape::Circle { .. } | EdgeShape::Ellipse { .. })
}

/// A random point of `body` in `evaluation`: a corner, a straight edge's
/// middle or a round edge's centre.
fn random_point(evaluation: &Evaluation, body: BodyId, rng: &mut Rng) -> Option<PointRef> {
    match rng.below(3) {
        0 => {
            let solid = solid(evaluation, body)?;
            let topology = solid.topology();
            let corners = topology.corners();
            let corner = corners.get(rng.below(corners.len()))?;
            let keys = keys_of(&topology, &corner.regions);
            let faces: [FaceKey; 3] = keys.get(..3)?.try_into().ok()?;
            let near = solid.mesh().verts()[corner.vertex as usize];
            Some(PointRef::Corner { body, faces, near })
        }
        1 => random_chain(evaluation, body, rng, is_line).map(PointRef::Middle),
        _ => random_chain(evaluation, body, rng, is_round).map(PointRef::Centre),
    }
}

/// A random direction of `body` in `evaluation`: a flat face's normal, a
/// round face's axis, a straight edge's direction or a round edge's
/// axis, whichever of a few tries the body has.
fn random_direction(evaluation: &Evaluation, body: BodyId, rng: &mut Rng) -> Option<DirRef> {
    (0..4).find_map(|_| match rng.below(4) {
        0 => random_face(evaluation, body, rng, |form| {
            matches!(form, Form::Plane { .. })
        })
        .map(DirRef::Normal),
        1 => random_face(evaluation, body, rng, |form| {
            matches!(form, Form::Cylinder { .. } | Form::Cone { .. })
        })
        .map(|face| DirRef::Axis(AxisRef::Face(face))),
        2 => random_chain(evaluation, body, rng, is_line)
            .map(|edge| DirRef::Axis(AxisRef::Edge(edge))),
        _ => random_chain(evaluation, body, rng, is_round)
            .map(|edge| DirRef::Axis(AxisRef::Edge(edge))),
    })
}

/// A random align of one of `bodies` (those with solids of their own in
/// `evaluation`, the history before it) onto another or the origin.
fn random_align(evaluation: &Evaluation, bodies: &[BodyId], rng: &mut Rng) -> Option<Align> {
    let body = bodies[rng.below(bodies.len())];
    let others: Vec<BodyId> = bodies.iter().copied().filter(|&b| b != body).collect();
    let target = (rng.below(4) != 0)
        .then(|| others.get(rng.below(others.len())).copied())
        .flatten();
    let point = random_point(evaluation, body, rng)?;
    let target_point = match target {
        Some(target) => random_point(evaluation, target, rng)?,
        None => PointRef::Origin,
    };
    let direction = |on: Option<BodyId>, rng: &mut Rng| match on {
        Some(body) => random_direction(evaluation, body, rng),
        None => Some(DirRef::Origin(Axis3::ALL[rng.below(3)])),
    };
    let (mut from, mut to) = (
        AlignRefs {
            point,
            primary: None,
            secondary: None,
        },
        AlignRefs {
            point: target_point,
            primary: None,
            secondary: None,
        },
    );
    if rng.below(3) != 0
        && let (Some(a), Some(b)) = (direction(Some(body), rng), direction(target, rng))
    {
        from.primary = Some(a);
        to.primary = Some(b);
        if rng.below(2) == 0 {
            // Across its primary, mostly: one along it is refused.
            let across = |primary: &DirRef, secondary: &DirRef| {
                let at = |d| direction_on(evaluation, d, false);
                at(primary)
                    .zip(at(secondary))
                    .is_some_and(|(p, s)| p.normalize().cross(s.normalize()).length() > 0.1)
            };
            let tries = if rng.below(4) == 0 { 1 } else { 6 };
            let pick = |on, primary: &DirRef, rng: &mut Rng| {
                let mut last = None;
                for _ in 0..tries {
                    last = direction(on, rng);
                    if last.as_ref().is_some_and(|s| across(primary, s)) {
                        break;
                    }
                }
                last
            };
            if let (Some(a), Some(b)) = (pick(Some(body), &a, rng), pick(target, &b, rng)) {
                from.secondary = Some(a);
                to.secondary = Some(b);
            }
        }
    }
    let design = varde_document::Document::default().design();
    let primaries = from.primary.is_some();
    let offset = (primaries && rng.below(2) == 0).then(|| {
        Value::new(
            OFFSETS[rng.below(OFFSETS.len())],
            &Move::offset_ask(&design),
        )
        .unwrap()
    });
    let turn = (primaries && rng.below(2) == 0)
        .then(|| Value::new(ANGLES[rng.below(ANGLES.len())], &Move::angle_ask(&design)).unwrap());
    Some(Align {
        body,
        from,
        to,
        flip: primaries && rng.below(3) == 0,
        offset,
        turn,
    })
}

const FACTORS: [&str; 9] = [
    "2", "0.5", "1.5", "3", "0.25", "25.4", "1000", "0.001", "1/3",
];
const LENGTHS: [&str; 5] = ["20", "5", "50", "12.5", "0.1"];

/// A random scale of some of `bodies` (those with solids of their own in
/// `evaluation`, the history before it) about the origin or a point on
/// any of them: by one factor, one per axis, or to the length of an
/// edge of one of them, along its axis only or not.
fn random_scale(
    document: &Document,
    evaluation: &Evaluation,
    bodies: &[BodyId],
    rng: &mut Rng,
) -> Option<Scale> {
    let design = document.design();
    let picked = some_of(bodies, rng);
    let about = match rng.below(2) {
        0 => PointRef::Origin,
        _ => random_point(evaluation, bodies[rng.below(bodies.len())], rng)?,
    };
    let ask = Scale::factor_ask(&design);
    let factor = |rng: &mut Rng| Value::new(FACTORS[rng.below(FACTORS.len())], &ask).unwrap();
    let factor = match rng.below(3) {
        0 => ScaleFactor::Uniform(factor(rng)),
        1 => ScaleFactor::PerAxis([factor(rng), factor(rng), factor(rng)]),
        _ => {
            let on = picked[rng.below(picked.len())];
            let edge = random_chain(evaluation, on, rng, |_| true)?;
            let text = LENGTHS[rng.below(LENGTHS.len())];
            ScaleFactor::EdgeLength {
                edge,
                length: Value::new(text, &Scale::length_ask(&design)).unwrap(),
                axis_only: rng.below(2) == 0,
            }
        }
    };
    Some(Scale {
        bodies: picked,
        about,
        factor,
    })
}

/// The length of `edge` on the bodies as `evaluation` has them, by the
/// measure tool, and its ends if it's straight; `None` where it isn't
/// found or isn't the only chain of its names.
fn edge_length(evaluation: &Evaluation, edge: &EdgeRef) -> Option<(f64, Option<[DVec3; 2]>)> {
    let made = super::super::super::motion::holding(edge.body, evaluation)?;
    let topology = made.solid.topology();
    if chains_named(&topology, &edge.faces) != 1 {
        return None;
    }
    let chain = topology.edge(&made.solid, edge.faces, edge.near).ok()?;
    let target = varde_kernel::measure::Target {
        solid: &made.solid,
        topology: &topology,
        pick: varde_kernel::measure::Pick::Edge(chain),
    };
    let tolerance = Tolerance::default();
    match varde_kernel::measure::measure(&target, &tolerance, &Budget::DEFAULT).ok()? {
        varde_kernel::measure::Measured::Edge(measured) => Some((
            measured.length,
            match measured.shape {
                EdgeShape::Line { from, to } => Some([from, to]),
                _ => None,
            },
        )),
        _ => None,
    }
}

/// Holds the scale `scale` (the feature `feature`, named `what`), which
/// worked, to what it noted and did: its point where the topology before
/// it finds it, its factors those typed or the edge's length before it
/// over the length typed, each body scaled (its volume times the
/// factors' product, its centre of mass where the scale takes the old
/// one), the edge then the length typed, every other body left alone.
fn check_scale(
    scale: &Scale,
    feature: FeatureId,
    before: &Evaluation,
    after: &Evaluation,
    what: &str,
) {
    let found = (after.scaled.iter())
        .find(|(id, _)| *id == feature)
        .map(|(_, found)| *found)
        .unwrap_or_else(|| panic!("{what}: nothing noted"));
    let centre = DVec3::from(found.centre.expect("a point"));
    let factors = DVec3::from(found.factors.expect("factors"));
    if let Some(point) = point_on(before, &scale.about, true) {
        assert!(
            close_at(centre, point, point.abs().max_element()),
            "{what}: {centre} {point}"
        );
    }
    match &scale.factor {
        ScaleFactor::Uniform(f) => assert_eq!(factors, DVec3::splat(f.value), "{what}"),
        ScaleFactor::PerAxis(f) => {
            assert_eq!(factors.to_array(), f.each_ref().map(|f| f.value), "{what}");
        }
        ScaleFactor::EdgeLength {
            edge,
            length,
            axis_only,
        } => {
            let measured = found.length.expect("the edge's length");
            if let Some((was, line)) = edge_length(before, edge) {
                assert!(
                    (measured - was).abs() <= 1e-12 * was,
                    "{what}: {measured} {was}"
                );
                let f = length.value / was;
                let wanted = match (axis_only, line) {
                    (false, _) => DVec3::splat(f),
                    (true, Some([from, to])) => {
                        let axis = crate::history::scale::along_axis(to - from)
                            .unwrap_or_else(|| panic!("{what}: slanted, scaled"));
                        let mut wanted = DVec3::ONE;
                        wanted[axis] = f;
                        wanted
                    }
                    (true, None) => panic!("{what}: not straight, scaled along its axis"),
                };
                assert!(
                    (factors - wanted).abs().max_element() <= 1e-12 * wanted.max_element(),
                    "{what}: {factors} {wanted}"
                );
            }
            if let Some((now, _)) = edge_length(after, edge) {
                assert!(
                    (now - length.value).abs() <= 1e-9 * length.value,
                    "{what}: the edge is {now}, not {}",
                    length.value
                );
            }
        }
    }
    let product = factors.x * factors.y * factors.z;
    for made in &before.bodies {
        let now = solid(after, made.body).expect("still a body");
        if scale.bodies.binary_search(&made.body).is_err() {
            assert_eq!(now, &made.solid, "{what}: {:?} changed", made.body);
            continue;
        }
        let (volume, was) = mass(&made.solid);
        let (volume_now, centre_now) = mass(now);
        let wanted = volume * product;
        assert!(
            (volume_now - wanted).abs() <= 1e-6 * wanted,
            "{what}: {volume} × {product} became {volume_now}"
        );
        let to = centre + factors * (was - centre);
        assert!(
            close_at(centre_now, to, size(now).max(size(&made.solid))),
            "{what}: centre {was} went to {centre_now}, not {to}"
        );
    }
}

/// The point `point` names on the bodies as `evaluation` has them, by
/// the kernel's topology; `None` where it isn't found or isn't the only
/// one of its names (`unique`), so a stale `near` can't choose another.
fn point_on(evaluation: &Evaluation, point: &PointRef, unique: bool) -> Option<DVec3> {
    let on = |body| super::super::super::motion::holding(body, evaluation);
    match point {
        PointRef::Origin => Some(DVec3::ZERO),
        PointRef::Corner { body, faces, near } => {
            let made = on(*body)?;
            let topology = made.solid.topology();
            let count = (topology.corners().iter())
                .filter(|corner| {
                    faces.iter().all(|key| {
                        (corner.regions.iter()).any(|&r| topology.regions()[r as usize].named(key))
                    })
                })
                .count();
            if unique && count != 1 {
                return None;
            }
            topology.corner_point(&made.solid, *faces, *near).ok()
        }
        PointRef::Middle(edge) | PointRef::Centre(edge) => {
            let made = on(edge.body)?;
            let topology = made.solid.topology();
            if unique && chains_named(&topology, &edge.faces) != 1 {
                return None;
            }
            match point {
                PointRef::Middle(_) => topology.middle(&made.solid, edge.faces, edge.near),
                _ => topology.centre(&made.solid, edge.faces, edge.near),
            }
            .ok()
        }
    }
}

/// How many chains of `topology` are between faces of the keys `faces`.
fn chains_named(topology: &Topology, faces: &[FaceKey; 2]) -> usize {
    let named = |r: u32, key: &FaceKey| topology.regions()[r as usize].named(key);
    (topology.chains().iter())
        .filter(|chain| {
            let [r0, r1] = chain.regions;
            (named(r0, &faces[0]) && named(r1, &faces[1]))
                || (named(r0, &faces[1]) && named(r1, &faces[0]))
        })
        .count()
}

/// The direction `direction` names, as [`point_on`] finds a point.
fn direction_on(evaluation: &Evaluation, direction: &DirRef, unique: bool) -> Option<DVec3> {
    let on = |body| super::super::super::motion::holding(body, evaluation);
    match direction {
        DirRef::Origin(axis) | DirRef::Axis(AxisRef::Origin(axis)) => Some(axis.direction()),
        DirRef::Normal(face) | DirRef::Axis(AxisRef::Face(face)) => {
            let made = on(face.body)?;
            let topology = made.solid.topology();
            let count = (topology.regions().iter())
                .filter(|region| region.named(&face.key))
                .count();
            if unique && count != 1 {
                return None;
            }
            match direction {
                DirRef::Normal(_) => topology.normal(&made.solid, &face.key, face.near).ok(),
                _ => (topology.face_axis(&made.solid, &face.key, face.near))
                    .ok()
                    .map(|[_, axis]| axis),
            }
        }
        DirRef::Axis(AxisRef::Edge(edge)) => {
            let made = on(edge.body)?;
            let topology = made.solid.topology();
            if unique && chains_named(&topology, &edge.faces) != 1 {
                return None;
            }
            topology
                .edge_direction(&made.solid, edge.faces, edge.near)
                .ok()
        }
    }
}

/// The right-handed orthonormal frame of `a` and the part of `b` square
/// to it, by `glam`.
fn frame(a: DVec3, b: DVec3) -> [DVec3; 3] {
    let f1 = a.normalize();
    let f2 = (b - f1 * b.dot(f1)).normalize();
    [f1, f2, f1.cross(f2)]
}

/// A side's point, primary and secondary as found.
type Found = (DVec3, Option<DVec3>, Option<DVec3>);

/// The rotation taking the unit `a` onto the unit `b` by the smallest
/// turn, by `glam`'s axis and angle; where they're within `1e-8` (the
/// sine) of opposite, the half turn about the direction square to `b`
/// that `Motion::align` takes then (as a sketch placement chooses its x
/// axis), where within that of the same way, none.
fn rotation_arc(a: DVec3, b: DVec3) -> DMat3 {
    let axis = a.cross(b);
    let sine = axis.length();
    if sine >= 1e-8 {
        return DMat3::from_axis_angle(axis / sine, sine.atan2(a.dot(b)));
    }
    if a.dot(b) > 0.0 {
        return DMat3::IDENTITY;
    }
    let square = if b.x * b.x + b.y * b.y <= 1e-18 {
        DVec3::X
    } else {
        DVec3::Z.cross(b)
    };
    let square = (square - b * square.dot(b)).normalize();
    DMat3::from_axis_angle(square, std::f64::consts::PI)
}

/// Five points of the chain `edge` names on `solid`, spread along its
/// first curve.
fn ellipse_points(solid: &Solid, edge: &EdgeRef) -> [DVec3; 5] {
    let topology = solid.topology();
    let chain = topology.edge(solid, edge.faces, edge.near).unwrap();
    let curve = solid
        .mesh()
        .curve(topology.chains()[chain as usize].halfedges[0]);
    [0.0, 0.25, 0.5, 0.75, 1.0].map(|t| curve.eval(t))
}

/// The centre of the conic through `points` in their plane square to
/// `axis`, by solving for its coefficients (`A x² + B xy + C y² + D x +
/// E y = 1` about the points' mean, inside the conic so it isn't on it)
/// with Gaussian elimination: independent of the kernel's own centre.
fn conic_centre(points: &[DVec3; 5], axis: DVec3) -> DVec3 {
    let n = axis.normalize();
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let mean = points.iter().copied().sum::<DVec3>() / 5.0;
    let mut rows = points.map(|p| {
        let (x, y) = ((p - mean).dot(u), (p - mean).dot(v));
        [x * x, x * y, y * y, x, y, 1.0]
    });
    for col in 0..5 {
        let pivot = (col..5)
            .max_by(|&a, &b| rows[a][col].abs().total_cmp(&rows[b][col].abs()))
            .unwrap();
        rows.swap(col, pivot);
        for row in 0..5 {
            if row != col {
                let k = rows[row][col] / rows[col][col];
                for c in col..6 {
                    rows[row][c] -= k * rows[col][c];
                }
            }
        }
    }
    let [a, b, c, d, e] = std::array::from_fn(|i| rows[i][5] / rows[i][i]);
    // The gradient vanishes at the centre: 2a x + b y + d = 0 and
    // b x + 2c y + e = 0.
    let det = 4.0 * a * c - b * b;
    let x = (b * e - 2.0 * c * d) / det;
    let y = (b * d - 2.0 * a * e) / det;
    mean + u * x + v * y
}

/// Holds what `align`'s references found on `before` (`sides`) to what
/// the geometry says, told apart from the topology's own code: a flat
/// face's normal square to its triangles and out of the body (their
/// corners' turn); a round edge's centre the centre of the circle
/// through three of its points, and its axis square to their plane;
/// a straight edge's middle halfway along it and its direction along it.
fn check_found(align: &Align, before: &Evaluation, sides: (Found, Found), what: &str) {
    let on = |body| super::super::super::motion::holding(body, before).unwrap();
    // A chain's shape, the solid's size, its curves' ends (two of them
    // far apart if it's open) and a point inside its first curve.
    let chain_of = |edge: &EdgeRef| -> (EdgeShape, f64, Vec<DVec3>, DVec3) {
        let made = on(edge.body);
        let topology = made.solid.topology();
        let chain = topology.edge(&made.solid, edge.faces, edge.near).unwrap();
        let chain = &topology.chains()[chain as usize];
        let mesh = made.solid.mesh();
        let curves: Vec<_> = chain.halfedges.iter().map(|&h| mesh.curve(h)).collect();
        let ends = (curves.iter())
            .flat_map(|curve| [curve.eval(0.0), curve.eval(1.0)])
            .collect();
        let shape = edge_shape(&made.solid, chain);
        (shape, size(&made.solid).max(1.0), ends, curves[0].eval(0.5))
    };
    // The two of `points` furthest apart.
    let extremes = |points: &[DVec3]| {
        let mut best = (0.0, points[0], points[0]);
        for &a in points {
            for &b in points {
                if a.distance(b) > best.0 {
                    best = (a.distance(b), a, b);
                }
            }
        }
        [best.1, best.2]
    };
    let check_point = |point: &PointRef, found: DVec3| match point {
        PointRef::Centre(edge) => {
            let (shape, size, ends, inside) = chain_of(edge);
            if let EdgeShape::Ellipse { axis, .. } = shape {
                // An ellipse (a rim scaled per axis): the centre of the
                // conic through five of its points.
                let points = ellipse_points(&on(edge.body).solid, edge);
                let centre = conic_centre(&points, axis);
                let reach = size.max(centre.abs().max_element());
                assert!(
                    centre.distance(found) <= 1e-6 * reach,
                    "{what}: centre {found}, five points give {centre}"
                );
                return;
            }
            let [a, c] = extremes(&ends);
            let b = inside;
            // The circumcentre, where the three points tell it well (a
            // closed rim's ends are one point: its first curve's middle
            // and an end stand in).
            let (u, v) = (b - a, c - a);
            let n = u.cross(v);
            if n.length() > 1e-3 * u.length() * v.length() {
                let centre = a
                    + (n.cross(u) * v.length_squared() + v.cross(n) * u.length_squared())
                        / (2.0 * n.length_squared());
                let reach = size.max(centre.abs().max_element());
                assert!(
                    centre.distance(found) <= 1e-6 * reach,
                    "{what}: centre {found}, three points give {centre}"
                );
            }
        }
        PointRef::Middle(edge) => {
            let (_, size, ends, _) = chain_of(edge);
            let [a, b] = extremes(&ends);
            let middle = (a + b) / 2.0;
            assert!(
                middle.distance(found) <= 1e-9 * size,
                "{what}: middle {found}, its ends give {middle}"
            );
        }
        PointRef::Corner { .. } | PointRef::Origin => {}
    };
    let check_direction = |direction: &DirRef, found: DVec3| match direction {
        DirRef::Normal(face) => {
            let made = on(face.body);
            let topology = made.solid.topology();
            let region = topology.face(&made.solid, &face.key, face.near).unwrap();
            let mesh = made.solid.mesh();
            for &tri in &topology.regions()[region as usize].tris {
                let corners =
                    (mesh.tris()[tri as usize].halfedges).map(|h| mesh.verts()[h.start as usize]);
                let (u, v) = (corners[1] - corners[0], corners[2] - corners[0]);
                let turn = u.cross(v);
                if turn.length() > 1e-6 * u.length() * v.length() {
                    let off = turn.normalize().distance(found.normalize());
                    assert!(
                        off < 1e-6,
                        "{what}: normal {found}, its triangle turns {turn}"
                    );
                }
            }
        }
        DirRef::Axis(AxisRef::Edge(edge)) => {
            let (shape, _, ends, inside) = chain_of(edge);
            let [a, c] = extremes(&ends);
            let found = found.normalize();
            if is_round(&shape) {
                // Square to its chord, and to its first curve's middle's
                // way from an end (the circle's plane).
                for way in [c - a, inside - a] {
                    if way.length() > 0.0 {
                        let off = way.normalize().dot(found).abs();
                        assert!(off < 1e-6, "{what}: round edge's axis {found} along {way}");
                    }
                }
            } else {
                let off = (c - a).normalize().cross(found).length();
                assert!(
                    off < 1e-9,
                    "{what}: straight edge's direction {found} across {}",
                    c - a
                );
            }
        }
        _ => {}
    };
    let (moved, target) = sides;
    for (refs, (point, primary, secondary)) in [(&align.from, moved), (&align.to, target)] {
        check_point(&refs.point, point);
        for (direction, found) in [(&refs.primary, primary), (&refs.secondary, secondary)] {
            if let (Some(direction), Some(found)) = (direction, found) {
                check_direction(direction, found);
            }
        }
    }
}

/// Holds the align `align` (the feature `feature`, named `what`), which
/// worked, to what it must do: what it noted is what the topology gives
/// on `before` (the history before it); on `after` (with it), the moved
/// body's centre of mass stands to the target's point and directions as
/// it stood to its own, its point and primary are found again where the
/// target's are, and every other body is left alone.
fn check_align(
    align: &Align,
    feature: FeatureId,
    before: &Evaluation,
    after: &Evaluation,
    what: &str,
) {
    let noted = (after.aligned.iter())
        .find(|(id, _)| *id == feature)
        .map(|(_, datums)| *datums);
    let side = |refs: &AlignRefs| -> Option<(DVec3, Option<DVec3>, Option<DVec3>)> {
        let point = point_on(before, &refs.point, false)?;
        let primary = match &refs.primary {
            Some(d) => Some(direction_on(before, d, false)?),
            None => None,
        };
        let secondary = match &refs.secondary {
            Some(d) => Some(direction_on(before, d, false)?),
            None => None,
        };
        Some((point, primary, secondary))
    };
    let (moved, target) = (side(&align.from).unwrap(), side(&align.to).unwrap());
    let as_array = |(p, a, b): (DVec3, Option<DVec3>, Option<DVec3>)| {
        (
            p.to_array(),
            a.map(|v| v.to_array()),
            b.map(|v| v.to_array()),
        )
    };
    if let Some(noted) = noted {
        for (found, side) in [(moved, noted.moved), (target, noted.target)] {
            if let Some(side) = side {
                assert_eq!(
                    as_array(found),
                    (side.point, side.primary, side.secondary),
                    "{what}: noted"
                );
            }
        }
    }
    let Some(noted) = noted.filter(|n| n.moved.is_some() && n.target.is_some()) else {
        return;
    };
    // The default way the primaries meet, told here from what they name:
    // opposed where each is a flat face's normal or a round edge's axis.
    if let (Some(from), Some(to)) = (&align.from.primary, &align.to.primary) {
        let outward = |direction: &DirRef| match direction {
            DirRef::Normal(_) => true,
            DirRef::Axis(AxisRef::Edge(edge)) => {
                let made = super::super::super::motion::holding(edge.body, before).unwrap();
                let topology = made.solid.topology();
                let chain = topology.edge(&made.solid, edge.faces, edge.near).unwrap();
                is_round(&edge_shape(&made.solid, &topology.chains()[chain as usize]))
            }
            _ => false,
        };
        assert_eq!(
            noted.opposed,
            (outward(from) && outward(to)) != align.flip,
            "{what}: opposed"
        );
    }
    check_found(align, before, (moved, target), what);
    let (p, m, ms) = moved;
    let (q, t, ts) = target;
    let (Some(made), Some(now)) = (solid(before, align.body), solid(after, align.body)) else {
        panic!("{what}: the moved body has a solid");
    };
    let scale = size(made)
        .max(size(now))
        .max(p.abs().max_element())
        .max(q.abs().max_element());
    let offset = align.offset.as_ref().map_or(0.0, |v| v.value);
    let radians = align.turn.as_ref().map_or(0.0, |v| v.value);
    // The motion worked out here, by `glam`: the moved frame onto the
    // target's (the smallest rotation without secondaries), turned about
    // the target's primary, the point onto the target's point moved the
    // offset along it.
    let (rotation, lands) = match (m, t) {
        (Some(m), Some(t)) => {
            let along = t.normalize();
            let toward = if noted.opposed { -along } else { along };
            let tilt = match (ms, ts) {
                (Some(ms), Some(ts)) => {
                    let [a, b] = [frame(m, ms), frame(toward, ts)];
                    DMat3::from_cols(b[0], b[1], b[2])
                        * DMat3::from_cols(a[0], a[1], a[2]).transpose()
                }
                _ => rotation_arc(m.normalize(), toward),
            };
            let turn = DMat3::from_axis_angle(along, radians);
            (turn * tilt, q + along * offset)
        }
        _ => (DMat3::IDENTITY, q),
    };
    let image = |x: DVec3| rotation * (x - p) + lands;
    let (volume, centre) = mass(made);
    let (volume_now, centre_now) = mass(now);
    assert!(close(volume_now, volume, volume), "{what}: volume");
    assert!(
        close_at(centre_now, image(centre), scale),
        "{what}: centre {centre_now} not at {}",
        image(centre)
    );
    // Every vertex where the motion takes it (a move maps them in order).
    let (verts, verts_now) = (made.mesh().verts(), now.mesh().verts());
    if verts.len() == verts_now.len() {
        for (&v, &w) in verts.iter().zip(verts_now) {
            assert!(
                close_at(w, image(v), scale),
                "{what}: vertex {v} to {w}, not {}",
                image(v)
            );
        }
    }
    // Found again on the moved body where the target's are (where the
    // name is the only one, so the stale point can't choose another). A
    // nearly flat arc's centre, far off the body, is found again only as
    // well as the arc's sagitta is told at its new place: within the
    // rounding there over the sagitta, times the radius.
    let far = |found: DVec3| {
        let extent = now.bounds3().map_or(0.0, |b| (b.max - b.min).length());
        found.distance(centre_now) > 1e3 * extent.max(1.0)
    };
    if let Some(found) = point_on(after, &align.from.point, true)
        .filter(|&found| !(matches!(align.from.point, PointRef::Centre(_)) && far(found)))
    {
        assert!(
            close_at(found, lands, scale),
            "{what}: {found} not at {lands}"
        );
    }
    if let (Some(t), Some(found)) = (
        t,
        (align.from.primary.as_ref()).and_then(|d| direction_on(after, d, true)),
    ) {
        let toward = t.normalize() * if noted.opposed { -1.0 } else { 1.0 };
        assert!(
            found.normalize().distance(toward) < 1e-9,
            "{what}: primary {found} not along {toward}"
        );
    }
    for made in &before.bodies {
        if made.body != align.body {
            let now = solid(after, made.body).expect("still a body");
            assert_eq!(now, &made.solid, "{what}: {:?} changed", made.body);
        }
    }
}

fn close(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= 1e-6 * scale.max(1.0)
}

fn close_at(a: DVec3, b: DVec3, scale: f64) -> bool {
    a.distance(b) <= 1e-6 * scale.max(1.0)
}

/// The volume and centre of mass of `solid`.
fn mass(solid: &Solid) -> (f64, DVec3) {
    let moments = solid.moments(&Budget::DEFAULT).unwrap();
    (moments.volume, moments.centre.unwrap_or(DVec3::ZERO))
}

/// How big `solid` is, for tolerances: its box's largest coordinate.
fn size(solid: &Solid) -> f64 {
    solid.bounds3().map_or(1.0, |bounds| {
        bounds.min.abs().max(bounds.max.abs()).max_element() + 1.0
    })
}

/// Whether the line through `point` along `along` is the axis `axis`
/// names, found on the bodies as `before` has them: an origin axis
/// exactly; an edge's line through its ends, parallel; a face's form's
/// axis.
fn axis_found(axis: &AxisRef, [point, along]: [DVec3; 2], before: &Evaluation) -> bool {
    let on_line = |p: DVec3, q: DVec3, d: DVec3| (p - q).cross(d.normalize()).length() < 1e-6;
    let parallel = |a: DVec3, b: DVec3| a.normalize().cross(b.normalize()).length() < 1e-9;
    match axis {
        AxisRef::Origin(axis) => point == DVec3::ZERO && along == axis.direction(),
        AxisRef::Edge(edge) => {
            let Some(made) = super::super::super::motion::holding(edge.body, before) else {
                return false;
            };
            let topology = made.solid.topology();
            let Ok(chain) = topology.edge(&made.solid, edge.faces, edge.near) else {
                return false;
            };
            match edge_shape(&made.solid, &topology.chains()[chain as usize]) {
                EdgeShape::Line { from, to } => {
                    on_line(from, point, along) && on_line(to, point, along)
                }
                EdgeShape::Circle { centre, axis, .. } => {
                    on_line(centre, point, along) && parallel(axis, along)
                }
                _ => false,
            }
        }
        AxisRef::Face(face) => {
            let Some(made) = super::super::super::motion::holding(face.body, before) else {
                return false;
            };
            let topology = made.solid.topology();
            let Ok(region) = topology.face(&made.solid, &face.key, face.near) else {
                return false;
            };
            match *region_form(&made.solid, &topology.regions()[region as usize]) {
                Form::Cylinder { point: p, axis, .. } | Form::Cone { apex: p, axis, .. } => {
                    parallel(axis, along) && on_line(point, p, axis)
                }
                Form::Torus {
                    centre: p, axis, ..
                }
                | Form::Revolved {
                    origin: p, axis, ..
                } => parallel(axis, along) && on_line(point, p, axis),
                _ => false,
            }
        }
    }
}

/// Holds every move and mirror of `document` (evaluated whole as
/// `evaluation`) to the motion worked out here.
fn check_motions(document: &Document, evaluation: &Evaluation, cache: &mut Cache, what: &str) {
    let tolerance = document.tolerance();
    for (index, feature) in document.features().iter().enumerate() {
        let (bodies, turn, mirror) = match &feature.kind {
            FeatureKind::Move(moved) => (&moved.bodies, Some(moved), None),
            FeatureKind::Mirror(mirror) => (&mirror.bodies, None, Some(mirror)),
            FeatureKind::Pattern(pattern) => {
                let what = format!("{what}: {} {index}", feature.name);
                check_pattern(document, index, pattern, evaluation, cache, &what);
                continue;
            }
            FeatureKind::Align(_) | FeatureKind::Scale(_) => (&Vec::new(), None, None),
            _ => continue,
        };
        let what = format!("{what}: {} {index}", feature.name);
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
        if let FeatureKind::Align(align) = &feature.kind {
            check_align(align, feature.id, &before, &after, &what);
            continue;
        }
        if let FeatureKind::Scale(scale) = &feature.kind {
            check_scale(scale, feature.id, &before, &after, &what);
            continue;
        }
        let reference = (after.references.iter())
            .find(|(id, _)| *id == feature.id)
            .map(|(_, line)| *line);
        // The point each body's points go to.
        let map: Box<dyn Fn(DVec3) -> DVec3> = match (turn, mirror) {
            (Some(moved), _) => {
                let offset = moved.offset_vector();
                match &moved.turn {
                    Some((axis, angle)) => {
                        let [point, along] = reference.expect("a turn's axis is found");
                        assert!(
                            axis_found(axis, [point, along], &before),
                            "{what}: {axis:?}"
                        );
                        let turn = DQuat::from_axis_angle(along.normalize(), angle.value);
                        Box::new(move |p| turn * (p - point) + point + offset)
                    }
                    None => Box::new(move |p| p + offset),
                }
            }
            (None, Some(mirror)) => {
                let [point, normal] = reference.expect("a mirror's plane is found");
                if let PlaneRef::Origin(plane) = mirror.plane {
                    assert_eq!(point, DVec3::ZERO, "{what}");
                    assert_eq!(normal, plane.placement().normal, "{what}");
                }
                let n = normal.normalize();
                Box::new(move |p| p - 2.0 * (p - point).dot(n) * n)
            }
            (None, None) => unreachable!(),
        };
        for made in &before.bodies {
            let now = solid(&after, made.body).expect("still a body");
            if !bodies.contains(&made.body) {
                assert_eq!(now, &made.solid, "{what}: {:?} changed", made.body);
                continue;
            }
            let scale = size(&made.solid).max(size(now));
            let (volume, centre) = mass(&made.solid);
            let (volume_now, centre_now) = mass(now);
            let keep = mirror.is_some_and(|mirror| mirror.keep_original);
            if !keep {
                assert!(
                    close(volume_now, volume, volume),
                    "{what}: {volume} became {volume_now}"
                );
                assert!(
                    close_at(centre_now, map(centre), scale),
                    "{what}: centre {centre} went to {centre_now}, not {}",
                    map(centre)
                );
                continue;
            }
            // The original and its image: the image where the reflection
            // puts it, and the two together as the boolean identities say.
            let [point, normal] = reference.unwrap();
            let image = made
                .solid
                .transformed(
                    &Motion::mirror(point, normal).unwrap(),
                    Some(Instance {
                        feature: feature.id.get(),
                        index: 1,
                    }),
                    &tolerance,
                    &Budget::DEFAULT,
                )
                .unwrap();
            let (image_volume, image_centre) = mass(&image);
            assert!(close(image_volume, volume, volume), "{what}");
            assert!(close_at(image_centre, map(centre), scale), "{what}");
            let common = varde_kernel::boolean(
                &made.solid,
                &image,
                Op::Intersection,
                &tolerance,
                &Budget::DEFAULT,
            );
            match common {
                Ok(common) => {
                    let (shared, shared_centre) = mass(&common);
                    let whole = 2.0 * volume - shared;
                    assert!(
                        close(volume_now, whole, volume),
                        "{what}: {volume} and its image sharing {shared} gave {volume_now}"
                    );
                    if whole > 1e-9 {
                        let expected = (centre * volume + image_centre * volume
                            - shared_centre * shared)
                            / whole;
                        assert!(close_at(centre_now, expected, scale), "{what}");
                    }
                }
                Err(_) => assert!(
                    volume_now <= 2.0 * volume * (1.0 + 1e-6)
                        && volume_now >= volume * (1.0 - 1e-6),
                    "{what}: {volume} with its image gave {volume_now}"
                ),
            }
        }
    }
}

/// Holds the pattern `pattern`, feature `index` of `document` (evaluated
/// whole as `evaluation`), to the copies worked out here: each copy the
/// body's volume, its centre where `glam` turns or moves the body's to;
/// the body and its copies as they come out of uniting them one by one
/// (where that works; otherwise at least the body and at most all of
/// them); every other body left alone; one that failed changed nothing.
fn check_pattern(
    document: &Document,
    index: usize,
    pattern: &Pattern,
    evaluation: &Evaluation,
    cache: &mut Cache,
    what: &str,
) {
    let tolerance = document.tolerance();
    let feature = document.features()[index].id;
    let fails = |e: &Evaluation| e.failed.iter().any(|f| f.feature == feature);
    let before = evaluate(&truncated(document, index), cache);
    let after = evaluate(&truncated(document, index + 1), cache);
    assert_eq!(fails(evaluation), fails(&after), "{what}");
    if fails(&after) {
        assert!(
            same_bodies(&before, &after),
            "{what}: failing, it changed bodies"
        );
        return;
    }
    let [point, along] = (after.references.iter())
        .find(|(id, _)| *id == feature)
        .map(|(_, line)| *line)
        .expect("a pattern's axis is found");
    assert!(
        axis_found(pattern.kind.axis(), [point, along], &before),
        "{what}"
    );
    let count = pattern.count().expect("a checked count");
    let motions = crate::history::pattern::placements(pattern, count, [point, along]).unwrap();
    let d = along.normalize();
    // Where copy `k` takes the point `p`, by `glam`.
    let place = |k: u32, p: DVec3| match &pattern.kind {
        PatternKind::Linear { spacing, .. } => p + d * (f64::from(k) * spacing.value),
        PatternKind::Circular { .. } => {
            let (span, steps) = pattern.span_steps().unwrap();
            let turn =
                DQuat::from_axis_angle(d, (f64::from(k) * span / f64::from(steps)).to_radians());
            turn * (p - point) + point
        }
    };
    // Unjoined, the copy bodies are new, after the others.
    let made_here: Vec<BodyId> = pattern.copy_bodies().map(|(_, _, body)| body).collect();
    let mut bodies_after: Vec<BodyId> = before.bodies.iter().map(|made| made.body).collect();
    let mut separate: Vec<BodyId> = (made_here.iter().copied())
        .filter(|&body| solid(&after, body).is_some())
        .collect();
    separate.sort_unstable();
    bodies_after.extend(&separate);
    let order: Vec<BodyId> = after.bodies.iter().map(|made| made.body).collect();
    assert_eq!(order, bodies_after, "{what}: the bodies after");
    for made in &before.bodies {
        let now = solid(&after, made.body).expect("still a body");
        let Ok(source) = pattern.bodies.binary_search(&made.body) else {
            assert_eq!(now, &made.solid, "{what}: {:?} changed", made.body);
            continue;
        };
        let (volume, centre) = mass(&made.solid);
        if !pattern.joins() {
            assert_eq!(
                now, &made.solid,
                "{what}: the original {:?} changed",
                made.body
            );
            for (k, motion) in (1..).zip(&motions) {
                let body = pattern.copy_body(source, k).expect("a body per copy");
                let copy = solid(&after, body).expect("a copy body's solid");
                let expected = made
                    .solid
                    .transformed(
                        motion,
                        Some(Instance {
                            feature: feature.get(),
                            index: u64::from(k),
                        }),
                        &tolerance,
                        &Budget::DEFAULT,
                    )
                    .unwrap();
                assert_eq!(**copy, expected, "{what}: copy {k}'s body");
                let (copy_volume, copy_centre) = mass(copy);
                let scale = size(&made.solid).max(size(copy));
                assert!(close(copy_volume, volume, volume), "{what}: copy {k}");
                assert!(
                    close_at(copy_centre, place(k, centre), scale),
                    "{what}: copy {k}'s body at {copy_centre}, not {}",
                    place(k, centre)
                );
            }
            continue;
        }
        let (volume_now, centre_now) = mass(now);
        let mut copies = vec![(*made.solid).clone()];
        for (k, motion) in (1..).zip(&motions) {
            let copy = made
                .solid
                .transformed(
                    motion,
                    Some(Instance {
                        feature: feature.get(),
                        index: u64::from(k),
                    }),
                    &tolerance,
                    &Budget::DEFAULT,
                )
                .unwrap();
            let (copy_volume, copy_centre) = mass(&copy);
            let scale = size(&made.solid).max(size(&copy));
            assert!(close(copy_volume, volume, volume), "{what}: copy {k}");
            assert!(
                close_at(copy_centre, place(k, centre), scale),
                "{what}: copy {k} at {copy_centre}, not {}",
                place(k, centre)
            );
            copies.push(copy);
        }
        let scale = size(&made.solid).max(size(now));
        let united = (copies[1..].iter()).try_fold(copies[0].clone(), |all, copy| {
            varde_kernel::boolean(&all, copy, Op::Union, &tolerance, &Budget::DEFAULT)
        });
        match united {
            Ok(united) => {
                let (whole, whole_centre) = mass(&united);
                assert!(
                    close(volume_now, whole, whole),
                    "{what}: {count} copies of {volume} gave {volume_now}, united {whole}"
                );
                assert!(close_at(centre_now, whole_centre, scale), "{what}");
            }
            Err(_) => assert!(
                volume_now <= f64::from(count) * volume * (1.0 + 1e-6)
                    && volume_now >= volume * (1.0 - 1e-6),
                "{what}: {count} copies of {volume} gave {volume_now}"
            ),
        }
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d ^ (seed + 1).wrapping_mul(0x9e37_79b9));
    let mut editor = Editor::new(Document::default());
    let mut warm = Cache::default();
    let mut regenerator = Regenerator::default();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let document = editor.document().clone();
        warm.begin();
        let now = evaluate(&document, &mut warm);
        let roll = if step < 2 { 0 } else { rng.below(17) };
        let mut draft = None;
        let mut apply = |command: Command| {
            let _ = editor.apply(command);
        };
        // The moves, mirrors and patterns of the document, by index.
        let motions: Vec<usize> = (0..document.features().len())
            .filter(|&i| {
                matches!(
                    document.features()[i].kind,
                    FeatureKind::Move(_)
                        | FeatureKind::Mirror(_)
                        | FeatureKind::Pattern(_)
                        | FeatureKind::Align(_)
                        | FeatureKind::Scale(_)
                )
            })
            .collect();
        match roll {
            0 | 1 => {
                // Now and then a block whose top is a shallow arc, its
                // centre far below: up to the coordinate limit.
                let shape: Box<dyn FnOnce(&mut Sketch)> = if rng.below(6) == 0 {
                    let x = 5.0 * rng.below(7) as f64;
                    let reach = [20.0, 500.0, 5e4, 999_990.0][rng.below(4)];
                    Box::new(domed((x, 0.0), (x + 10.0, 10.0), reach))
                } else {
                    random_shape(&mut rng)
                };
                let height = ["5", "10", "7.5"][rng.below(3)];
                let extent = Extent::OneSide(length(&document, height));
                add_extrude(&mut editor, shape, extent, Operation::NewBody(BodyId::NEW));
            }
            2 => {
                let shape = random_shape(&mut rng);
                let targets = Targets::default();
                let operation = match rng.below(3) {
                    0 => Operation::Join(targets),
                    1 => Operation::Cut(targets),
                    _ => Operation::Intersect(targets),
                };
                let height = ["5", "10", "15"][rng.below(3)];
                let extent = Extent::OneSide(length(&document, height));
                add_extrude(&mut editor, shape, extent, operation);
            }
            3..=6 => {
                if let Some(kind) = random_motion(&document, &now, &mut rng, None) {
                    editor
                        .apply(document.add_feature(kind))
                        .unwrap_or_else(|error| panic!("{what}: {error}"));
                }
            }
            7 => {
                if let Some(combine) = random_combine(&document, &mut rng, None) {
                    editor
                        .apply(document.add_feature(combine.into()))
                        .unwrap_or_else(|error| panic!("{what}: {error}"));
                }
            }
            8 => {
                // An earlier move, mirror or pattern made again, of what
                // comes before it.
                if !motions.is_empty() {
                    let index = motions[rng.below(motions.len())];
                    let then = evaluate(&truncated(&document, index), &mut warm);
                    if let Some(kind) = random_motion(&document, &then, &mut rng, Some(index)) {
                        let feature = document.features()[index].id;
                        // Refused only where it'd drop a copy body a
                        // later feature names.
                        let dropped = document.copies_dropped(feature, &kind);
                        let named = (document.features().iter())
                            .any(|f| f.kind.bodies().iter().any(|b| dropped.contains(b)));
                        let set = Command::SetFeature {
                            feature,
                            kind: Box::new(kind),
                        };
                        match editor.apply(set) {
                            Ok(()) => {}
                            Err(_) if named => assert_eq!(*editor.document(), document),
                            Err(error) => panic!("{what}: {error}"),
                        }
                    }
                }
            }
            9 => {
                // Upstream: an extrude's height or direction.
                let extrudes: Vec<&varde_document::Feature> = (document.features().iter())
                    .filter(|f| matches!(f.kind, FeatureKind::Extrude(_)))
                    .collect();
                if !extrudes.is_empty() {
                    let feature = extrudes[rng.below(extrudes.len())];
                    let FeatureKind::Extrude(mut extrude) = feature.kind.clone() else {
                        unreachable!()
                    };
                    let height = ["2.5", "5", "10", "12.5"][rng.below(4)];
                    extrude.extent = Extent::OneSide(length(&document, height));
                    extrude.flip ^= rng.below(3) == 0;
                    apply(Command::SetFeature {
                        feature: feature.id,
                        kind: Box::new(extrude.into()),
                    });
                }
            }
            10 => {
                if rng.below(2) == 0 && !document.features().is_empty() {
                    let feature = &document.features()[rng.below(document.features().len())];
                    apply(Command::RemoveFeature(feature.id));
                } else if !document.bodies().is_empty() {
                    let body = &document.bodies()[rng.below(document.bodies().len())];
                    apply(Command::RemoveBody(body.id));
                }
            }
            14 | 15 => {
                // An earlier pattern changed one way.
                let patterns: Vec<usize> = (motions.iter().copied())
                    .filter(|&i| matches!(document.features()[i].kind, FeatureKind::Pattern(_)))
                    .collect();
                if !patterns.is_empty() {
                    let index = patterns[rng.below(patterns.len())];
                    let feature = document.features()[index].id;
                    let FeatureKind::Pattern(old) = &document.features()[index].kind else {
                        unreachable!()
                    };
                    let then = evaluate(&truncated(&document, index), &mut warm);
                    let kind =
                        FeatureKind::Pattern(tweaked(&document, &then, old, index, &mut rng));
                    let dropped = document.copies_dropped(feature, &kind);
                    let named = (document.features().iter())
                        .any(|f| f.kind.bodies().iter().any(|b| dropped.contains(b)));
                    let set = Command::SetFeature {
                        feature,
                        kind: Box::new(kind),
                    };
                    match editor.apply(set) {
                        Ok(()) => kept_copies(&document, editor.document(), feature, &what),
                        Err(_) if named => assert_eq!(*editor.document(), document),
                        Err(error) => panic!("{what}: {error}"),
                    }
                }
            }
            16 => {
                // A copy body hidden or shown, or removed (its pattern
                // with it).
                let copies: Vec<BodyId> = (document.features().iter())
                    .filter_map(|f| match &f.kind {
                        FeatureKind::Pattern(pattern) => Some(pattern.copy_bodies()),
                        _ => None,
                    })
                    .flatten()
                    .map(|(_, _, body)| body)
                    .collect();
                if !copies.is_empty() {
                    let body = copies[rng.below(copies.len())];
                    if rng.below(3) == 0 {
                        let maker = document.body(body).unwrap().created_by;
                        apply(Command::RemoveBody(body));
                        assert!(editor.document().feature(maker).is_none(), "{what}");
                        assert!(editor.document().body(body).is_none(), "{what}");
                    } else {
                        let visible = document.body(body).unwrap().visible;
                        apply(Command::SetVisible(body, !visible));
                    }
                }
            }
            11 => editor.undo(),
            12 => editor.redo(),
            _ => {
                // A move, mirror or pattern drafted, new or in place of an
                // earlier one.
                let edited = (!motions.is_empty() && rng.below(2) == 0)
                    .then(|| motions[rng.below(motions.len())]);
                let then = match edited {
                    Some(index) => evaluate(&truncated(&document, index), &mut warm),
                    None => now,
                };
                if let Some(kind) = random_motion(&document, &then, &mut rng, edited) {
                    draft = Some(Draft {
                        revision: step as u64,
                        feature: edited.map(|index| document.features()[index].id),
                        kind,
                    });
                }
            }
        }
        let document = editor.document().clone();
        document
            .check()
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        warm.begin();
        let hot = evaluate(&document, &mut warm);
        let cold = evaluate(&document, &mut Cache::default());
        assert!(same_bodies(&hot, &cold), "{what}: warm and cold differ");
        assert_eq!(hot.failed, cold.failed, "{what}");
        assert_eq!(hot.references, cold.references, "{what}");
        assert_eq!(hot.aligned, cold.aligned, "{what}");
        assert_eq!(hot.scaled, cold.scaled, "{what}");
        check_motions(&document, &hot, &mut warm, &what);
        if step % 4 == 3 {
            not_stuck(&document, &what);
            bytes(&document, &mut rng, &mut warm, &what);
        }
        wire(&editor, draft, &mut regenerator, &what);
    }
}

/// See the module's docs. `VARDE_MOTION_SEEDS` runs more seeds
/// (`VARDE_MOTION_FROM` the first).
#[test]
fn random_histories_with_moves_mirrors_patterns_and_aligns_hold() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let from = number("VARDE_MOTION_FROM", 0);
    for seed in from..from.saturating_add(number("VARDE_MOTION_SEEDS", 3)) {
        run(seed, 24);
    }
}
