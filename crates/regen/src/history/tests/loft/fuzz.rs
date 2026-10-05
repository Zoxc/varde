//! Random histories of blocks and discs made as bodies, joins and cuts,
//! section sketches (squares on XY and on blocks' tops, the same square
//! on both now and then, with a point in the middle or a second point on
//! a corner; a point on its own on XZ; a disc), rail sketches (lines on
//! YZ, open or closed), and lofts (the kernel's loft replaced by the
//! stand-in extruding the first of two equal parallel sections to the
//! second) through two to four sections, regions or points, from start
//! points at corners or not, ruled or smooth, open or closed, with rails
//! or not, making a body, joining or cutting; earlier lofts edited,
//! section sketches redrawn (moved, a start's point moved a hair off its
//! corner, swapped for a disc: the lofts following), blocks' heights
//! changed (the sketches on their tops following), the tolerance changed,
//! removals, undo and redo, combines merging bodies, lofts drafted.
//!
//! After each step: the document passes its check; the cache warm and
//! cold give the same evaluation; every loft fails alike in the whole
//! history and in the history ending with it; one that failed changed
//! nothing; one that worked had two region sections on parallel planes,
//! open, no rails, and gave exactly what an extrude of its first
//! section's region as far as the second's plane gives; every later edit
//! can still be made; the document survives its bytes, flipped bits
//! included; and a request with a loft's draft and its answer cross the
//! wire as they went.

use varde_document::Placement;

use super::super::combine::fuzz::{
    Rng, bytes, not_stuck, random_combine, random_shape, same_bodies, truncated, wire,
};
use super::*;
use crate::{Draft, Regenerator};

/// A sketch's drawing, to be drawn.
type Drawing = Box<dyn FnOnce(&mut Sketch)>;

/// A square of side 4 at `(x, y)` (its low corner), with a point in its
/// middle (`1`) or a second point on its first corner drawn before the
/// square's (`2`), or neither (`0`).
fn square_at((x, y): (f64, f64), extra: usize) -> Drawing {
    Box::new(move |sketch: &mut Sketch| {
        if extra == 2 {
            sketch.add_point(DVec2::new(x, y)).unwrap();
        }
        rectangle((x, y), (x + 4.0, y + 4.0))(sketch);
        if extra == 1 {
            sketch.add_point(DVec2::new(x + 2.0, y + 2.0)).unwrap();
        }
    })
}

/// A square's low corner on the grid the blocks are on.
fn random_corner(rng: &mut Rng) -> (f64, f64) {
    (5.0 * rng.below(6) as f64, 5.0 * rng.below(4) as f64)
}

/// The top of an extrude of `document` making a body of a rectangle or a
/// disc on XY one way up, as a plane to sketch on, about the middle of
/// its sketch.
fn random_top(document: &Document, rng: &mut Rng) -> Option<Plane> {
    let tops: Vec<(FeatureId, BodyId, DVec3)> = (document.features().iter())
        .filter_map(|feature| {
            let FeatureKind::Extrude(extrude) = &feature.kind else {
                return None;
            };
            let (Extent::OneSide(height), false, Operation::NewBody(body)) =
                (&extrude.extent, extrude.flip, &extrude.operation)
            else {
                return None;
            };
            let FeatureKind::Sketch { sketch, .. } = &document.feature(extrude.sketch)?.kind else {
                return None;
            };
            let first = sketch.points.first()?.at;
            let last = sketch.points.get(2).map_or(first, |point| point.at);
            let middle = (first + last) / 2.0;
            Some((feature.id, *body, middle.extend(height.value)))
        })
        .collect();
    if tops.is_empty() {
        return None;
    }
    let (maker, body, near) = tops[rng.below(tops.len())];
    Some(Plane::Face(face(body, maker, PartKey::EndCap, near.into())))
}

/// The sketches of `document` with regions, those of points on their
/// own only, and the rails' (on YZ).
fn sketches(document: &Document) -> [Vec<FeatureId>; 3] {
    let mut found = [Vec::new(), Vec::new(), Vec::new()];
    for feature in document.features() {
        let FeatureKind::Sketch { sketch, plane, .. } = &feature.kind else {
            continue;
        };
        if *plane == Plane::Origin(OriginPlane::YZ) {
            found[2].push(feature.id);
        } else if sketch.curves.is_empty() && !sketch.points.is_empty() {
            found[1].push(feature.id);
        } else if sketch
            .profiles()
            .is_ok_and(|profiles| !profiles.regions.is_empty())
        {
            found[0].push(feature.id);
        }
    }
    found
}

fn drawn(document: &Document, feature: FeatureId) -> Option<&Sketch> {
    match &document.feature(feature)?.kind {
        FeatureKind::Sketch { sketch, .. } => Some(sketch),
        _ => None,
    }
}

/// A region section of sketch `sketch` at random: one of its regions,
/// from a point a curve uses mostly, now and then any point or none.
fn random_region(document: &Document, sketch: FeatureId, rng: &mut Rng) -> Option<Section> {
    let drawn = drawn(document, sketch)?;
    let profiles = drawn.profiles().ok()?;
    let region = profiles.reference(rng.below(profiles.regions.len()))?;
    let corners: Vec<varde_sketch::Id> = (drawn.curves.iter())
        .flat_map(|entry| entry.curve.points())
        .collect();
    let start = match rng.below(8) {
        0 => None,
        1 => Some(drawn.points[rng.below(drawn.points.len())].id),
        _ => corners.get(rng.below(corners.len().max(1))).copied(),
    };
    Some(Section::Region {
        sketch,
        region,
        start,
    })
}

/// A loft of `document`'s sketches at random: mostly two region sections,
/// the second of a sketch drawn as the first's, now and then up to four,
/// a point first or last, closed, with rails.
fn random_loft(document: &Document, rng: &mut Rng) -> Option<Loft> {
    let [regions, points, rails] = sketches(document);
    if regions.is_empty() {
        return None;
    }
    let first = regions[rng.below(regions.len())];
    let mut sections = vec![random_region(document, first, rng)?];
    let twins: Vec<FeatureId> = (regions.iter().copied())
        .filter(|&other| {
            let plane = |id| document.feature(id).map(|f| f.kind.clone());
            let on = |id| match plane(id) {
                Some(FeatureKind::Sketch { plane, .. }) => Some(plane),
                _ => None,
            };
            on(other) != on(first)
                && drawn(document, other).map(|s| &s.points)
                    == drawn(document, first).map(|s| &s.points)
        })
        .collect();
    let count = if rng.below(3) == 0 {
        2 + rng.below(3)
    } else {
        2
    };
    while sections.len() < count {
        let pool = if !twins.is_empty() && rng.below(3) != 0 {
            &twins
        } else {
            &regions
        };
        sections.extend(random_region(document, pool[rng.below(pool.len())], rng));
    }
    if !points.is_empty() && rng.below(5) == 0 {
        let sketch = points[rng.below(points.len())];
        let point = drawn(document, sketch)?.points[0].id;
        let section = Section::Point { sketch, point };
        if rng.below(2) == 0 {
            sections.insert(0, section);
        } else {
            sections.push(section);
        }
    }
    if rng.below(2) == 0 {
        sections.reverse();
    }
    let closed = rng.below(8) == 0;
    let rails = if !closed && !rails.is_empty() && rng.below(6) == 0 {
        let sketch = rails[rng.below(rails.len())];
        let mut curves: Vec<varde_sketch::Id> = drawn(document, sketch)?
            .curves
            .iter()
            .map(|c| c.id)
            .collect();
        curves.sort();
        vec![CurveChain { sketch, curves }]
    } else {
        Vec::new()
    };
    Some(Loft {
        sections,
        mode: if rng.below(2) == 0 {
            LoftMode::Smooth
        } else {
            LoftMode::Ruled
        },
        closed,
        rails,
        operation: match rng.below(5) {
            0 => Operation::Join(Targets::default()),
            1 => Operation::Cut(Targets::default()),
            _ => new_body(),
        },
    })
}

/// One of `loft`'s parts changed: its mode, closed, a section's start,
/// its sections reversed, one taken out, its operation.
fn tweaked(loft: &Loft, document: &Document, rng: &mut Rng) -> Loft {
    let mut loft = loft.clone();
    match rng.below(6) {
        0 => {
            loft.mode = match loft.mode {
                LoftMode::Smooth => LoftMode::Ruled,
                LoftMode::Ruled => LoftMode::Smooth,
            }
        }
        1 => {
            loft.closed = !loft.closed;
            if loft.closed {
                loft.rails.clear();
            }
        }
        2 => {
            let at = rng.below(loft.sections.len());
            if let Section::Region { sketch, start, .. } = &mut loft.sections[at]
                && let Some(drawn) = drawn(document, *sketch)
                && !drawn.points.is_empty()
            {
                *start = Some(drawn.points[rng.below(drawn.points.len())].id);
            }
        }
        3 => loft.sections.reverse(),
        4 if loft.sections.len() > 2 => {
            loft.sections.remove(rng.below(loft.sections.len()));
        }
        _ => {
            loft.operation = match (&loft.operation, rng.below(3)) {
                // A body it makes stays its.
                (Operation::NewBody(body), _) => Operation::NewBody(*body),
                (_, 0) => Operation::Join(Targets::default()),
                _ => Operation::Cut(Targets::default()),
            }
        }
    }
    loft
}

/// A section sketch redrawn: its square moved (the same ids), a point a
/// curve uses handed over to a new one at its place and moved off by a
/// hair or more, or a disc in its place.
fn redraw_section(editor: &mut Editor, rng: &mut Rng) {
    let [regions, ..] = sketches(editor.document());
    if regions.is_empty() {
        return;
    }
    let sketch = regions[rng.below(regions.len())];
    let corner = random_corner(rng);
    let hair = [0.0, 5e-9, 5e-7, 5e-6, 5e-5, 5e-4, 0.5][rng.below(7)];
    let pick = rng.below(4);
    let which = rng.below(4);
    edit(editor, sketch, |drawn| match pick {
        0 => {
            let Some(first) = drawn.points.first().map(|point| point.at) else {
                return;
            };
            let by = DVec2::new(corner.0, corner.1) - first;
            for point in &mut drawn.points {
                point.at += by;
            }
        }
        1 => {
            *drawn = Sketch::default();
            disc((corner.0 + 2.0, corner.1 + 2.0), 2.0)(drawn);
        }
        _ => {
            let used: Vec<varde_sketch::Id> = (drawn.curves.iter())
                .flat_map(|entry| entry.curve.points())
                .collect();
            let Some(&old) = used.get(which % used.len().max(1)) else {
                return;
            };
            let at = drawn.point(old).unwrap().at;
            let Ok(fresh) = drawn.add_point(at) else {
                return;
            };
            for entry in &mut drawn.curves {
                if let Curve::Line { start, end } = &mut entry.curve {
                    for end in [start, end] {
                        if *end == old {
                            *end = fresh;
                        }
                    }
                }
            }
            drawn.point_mut(old).unwrap().at.x += hair;
        }
    });
}

/// Where the sketch `feature` of `document` is placed, as `evaluation`
/// (of the history before a feature using it) found it.
fn placed(document: &Document, evaluation: &Evaluation, feature: FeatureId) -> Option<Placement> {
    match &document.feature(feature)?.kind {
        FeatureKind::Sketch {
            plane: Plane::Origin(plane),
            ..
        } => Some(plane.placement()),
        FeatureKind::Sketch { .. } => (evaluation.placements.iter())
            .find(|(id, _)| *id == feature)
            .map(|(_, placement)| *placement),
        _ => None,
    }
}

/// How far along its first section's normal the stand-in lofts `loft`,
/// if it's a loft it can: two region sections on parallel planes, open,
/// no rails. Worked out apart from regeneration.
fn lofted_height(document: &Document, before: &Evaluation, loft: &Loft) -> Option<f64> {
    if loft.closed || !loft.rails.is_empty() {
        return None;
    }
    let [
        Section::Region { sketch: a, .. },
        Section::Region { sketch: b, .. },
    ] = loft.sections.as_slice()
    else {
        return None;
    };
    let (a, b) = (placed(document, before, *a)?, placed(document, before, *b)?);
    let parallel = a.normal.cross(b.normal).length() <= 1e-12;
    parallel.then(|| (b.origin - a.origin).dot(a.normal))
}

/// `document` up to feature `index`, then an extrude in the loft's place
/// of its first section's region as far as `height` along its plane's
/// normal, doing what it does.
fn extruded_instead(document: &Document, index: usize, loft: &Loft, height: f64) -> Document {
    let mut editor = Editor::new(truncated(document, index));
    let Section::Region { sketch, region, .. } = &loft.sections[0] else {
        unreachable!("a region first");
    };
    let operation = match &loft.operation {
        Operation::NewBody(_) => new_body(),
        other => other.clone(),
    };
    let extrude = Extrude {
        sketch: *sketch,
        regions: vec![region.clone()],
        extent: Extent::OneSide(length(editor.document(), &format!("{}", height.abs()))),
        flip: height < 0.0,
        operation,
        taper: None,
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

/// Holds every loft of `document` (evaluated whole as `evaluation`) to
/// what the module's docs say, against the history before and after it.
fn check_lofts(document: &Document, evaluation: &Evaluation, cache: &mut Cache, what: &str) {
    for (index, feature) in document.features().iter().enumerate() {
        let FeatureKind::Loft(loft) = &feature.kind else {
            continue;
        };
        let what = format!("{what}: loft {index} {loft:?}");
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
        let height = lofted_height(document, &before, loft)
            .unwrap_or_else(|| panic!("{what}: worked on sections the stand-in can't loft"));
        let instead = evaluate(&extruded_instead(document, index, loft, height), cache);
        assert!(instead.failed.len() == before.failed.len(), "{what}");
        assert!(alike(&after, &instead), "{what}: not the extrude's");
        assert_eq!(after.merged.len(), instead.merged.len(), "{what}");
    }
}

fn run(seed: u64, steps: usize) {
    with_extrudes();
    let mut rng = Rng(0x6a09_e667_f3bc_c908 ^ (seed + 1).wrapping_mul(0x2f69_3b1d));
    let mut editor = Editor::new(Document::default());
    let mut cache = Cache::default();
    let mut regenerator = Regenerator::default();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let document = editor.document().clone();
        let roll = if step < 4 { step } else { rng.below(20) };
        let mut draft = None;
        let mut apply = |command: Command| {
            let _ = editor.apply(command);
        };
        match roll {
            0 | 4 => {
                let height = ["5", "10", "15"][rng.below(3)];
                let operation = match rng.below(6) {
                    0 => Operation::Join(Targets::default()),
                    1 => Operation::Cut(Targets::default()),
                    _ => new_body(),
                };
                add_extrude(
                    &mut editor,
                    random_shape(&mut rng),
                    Extent::OneSide(length(&document, height)),
                    operation,
                );
            }
            1 | 5 | 6 => {
                // A square on XY and, now and then, the same on a top.
                let corner = random_corner(&mut rng);
                let extra = rng.below(4).min(2);
                add_sketch(
                    &mut editor,
                    Plane::Origin(OriginPlane::XY),
                    square_at(corner, extra),
                );
                if rng.below(3) != 0
                    && let Some(top) = random_top(&document, &mut rng)
                {
                    add_sketch(&mut editor, top, square_at(corner, extra));
                }
            }
            2 => {
                // A point on its own, or a disc.
                if rng.below(2) == 0 {
                    add_sketch(&mut editor, Plane::Origin(OriginPlane::XZ), |sketch| {
                        sketch.add_point(DVec2::new(10.0, 25.0)).unwrap();
                    });
                } else {
                    let corner = random_corner(&mut rng);
                    add_sketch(
                        &mut editor,
                        Plane::Origin(OriginPlane::XY),
                        disc((corner.0 + 2.0, corner.1 + 2.0), 2.0),
                    );
                }
            }
            3 => {
                // A rail: open lines, or a closed square.
                if rng.below(3) == 0 {
                    add_sketch(
                        &mut editor,
                        Plane::Origin(OriginPlane::YZ),
                        rectangle((0.0, 0.0), (4.0, 10.0)),
                    );
                } else {
                    add_sketch(&mut editor, Plane::Origin(OriginPlane::YZ), |sketch| {
                        let [a, b, c] = [(0.0, 0.0), (0.0, 5.0), (0.0, 10.0)]
                            .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
                        sketch
                            .add_curve(Curve::Line { start: a, end: b }, false)
                            .unwrap();
                        sketch
                            .add_curve(Curve::Line { start: b, end: c }, false)
                            .unwrap();
                    });
                }
            }
            7..=9 => {
                if let Some(loft) = random_loft(&document, &mut rng) {
                    apply(document.add_feature(loft.into()));
                }
            }
            10 => {
                // An earlier loft edited.
                let lofts: Vec<(FeatureId, Loft)> = (document.features().iter())
                    .filter_map(|feature| match &feature.kind {
                        FeatureKind::Loft(loft) => Some((feature.id, loft.clone())),
                        _ => None,
                    })
                    .collect();
                if !lofts.is_empty() {
                    let (id, loft) = &lofts[rng.below(lofts.len())];
                    let kind = tweaked(loft, &document, &mut rng);
                    apply(Command::SetFeature {
                        feature: *id,
                        kind: Box::new(kind.into()),
                    });
                }
            }
            11 => redraw_section(&mut editor, &mut rng),
            12 => {
                // Upstream: an extrude's height, the sketches on its top
                // following.
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
                    apply(Command::SetFeature {
                        feature: feature.id,
                        kind: Box::new(extrude.into()),
                    });
                }
            }
            13 => {
                let fit = [1e-5, 1e-4, 1e-3, 1e-2, 0.1][rng.below(5)];
                apply(Command::SetTolerance(Tolerance::new(fit).unwrap()));
            }
            14 => {
                let features = document.features();
                if !features.is_empty() {
                    apply(Command::RemoveFeature(
                        features[rng.below(features.len())].id,
                    ));
                }
            }
            15 => {
                if let Some(combine) = random_combine(&document, &mut rng, None) {
                    apply(document.add_feature(combine.into()));
                }
            }
            16 if editor.can_undo() => editor.undo(),
            17 if editor.can_redo() => editor.redo(),
            _ => {
                // A loft drafted, new or in place of an earlier one.
                let lofts: Vec<(FeatureId, Loft)> = (document.features().iter())
                    .filter_map(|f| match &f.kind {
                        FeatureKind::Loft(loft) => Some((f.id, loft.clone())),
                        _ => None,
                    })
                    .collect();
                let edited = (!lofts.is_empty() && rng.below(2) == 0)
                    .then(|| &lofts[rng.below(lofts.len())]);
                let kind = match edited {
                    Some((_, loft)) => Some(tweaked(loft, &document, &mut rng)),
                    None => random_loft(&document, &mut rng),
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
        check_lofts(&document, &warm, &mut cache, &what);
        if step % 8 == 7 {
            not_stuck(&document, &what);
            bytes(&document, &mut rng, &mut cache, &what);
        }
        wire(&editor, draft, &mut regenerator, &what);
    }
}

/// See the module's docs. Quick runs seed 0's first `QUICK_STEPS` steps;
/// `VARDE_TESTS=full` runs seeds 0 to 2 to 40 steps, and
/// `VARDE_TEST_SEED` replays one seed to 40 steps. Each step evaluates
/// the whole history cold, so a run's time grows with its steps squared.
#[test]
fn random_histories_with_lofts_hold() {
    let replay = varde_testing::replay_seed().is_some();
    let steps = if replay {
        40
    } else {
        varde_testing::pick(QUICK_STEPS, 40)
    };
    for seed in varde_testing::seeds(1, 3) {
        run(seed, steps);
    }
}

/// The steps of the quick run.
const QUICK_STEPS: usize = 24;
