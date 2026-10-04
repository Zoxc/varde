//! Random histories of blocks and discs made as bodies, joins, cuts and
//! intersects over them, combines, and moves, mirrors and patterns among
//! them: moves by offsets and turns about origin axes, straight model
//! edges and round faces; mirrors in origin planes and flat faces,
//! keeping their originals or not; linear and circular patterns of two
//! to four copies along or about the same axes, half of them of bodies
//! patterned already, about their copies' edges and faces (copies of
//! copies), a third of them with each copy a body of its own (later
//! features then moving, mirroring, patterning, combining, joining and
//! naming the copy bodies); earlier ones edited, earlier patterns
//! changed one way (count, tick, kind, a body more or less: each copy
//! keeps its body as it was, new ones named apart), copy bodies hidden
//! or removed with their pattern, removals, undo and redo. After each
//! step: the document passes its check, the cache warm and cold give
//! the same evaluation; every move and mirror that worked put each of its
//! bodies where the motion worked out here takes it (the volume kept, the
//! centre of mass moved, turned or reflected, by `glam`'s own rotations
//! and reflections), a mirror keeping its original holding the body and
//! its image as the boolean identities say, every pattern each body and
//! its copies (each copy's centre where `glam` puts it, the whole as the
//! copies united one by one; unjoined, the body as it was and each copy
//! the solid of its own body, the copy itself), every other body left
//! alone, the axis or
//! plane found on what it names; one that failed changed nothing; every
//! later edit can still be made; the document survives its bytes,
//! flipped bits included; and a request with a move's, mirror's or
//! pattern's draft and its answer cross the wire as they went.

use std::collections::BTreeMap;

use glam::DQuat;
use varde_document::{Copies, Pattern, PatternKind};
use varde_kernel::measure::{EdgeShape, edge_shape};
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
                    FeatureKind::Move(_) | FeatureKind::Mirror(_) | FeatureKind::Pattern(_)
                )
            })
            .collect();
        match roll {
            0 | 1 => {
                let shape = random_shape(&mut rng);
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
fn random_histories_with_moves_mirrors_and_patterns_hold() {
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
