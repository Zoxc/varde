//! Random histories of blocks and discs made as bodies, joins and cuts,
//! combines, moves, sketches on the bodies' flat faces, and splits of
//! the bodies (the kernel's split replaced by two booleans) by other
//! bodies, by sketches' regions, by origin planes, flat faces' planes,
//! faces' surfaces and open lines (whose tools fail as too complex for
//! now), keeping both pieces or one, either piece keeping the id; splits
//! of splits' new bodies, of bodies merged into others and merged on
//! later; earlier splits edited (which piece keeps the id, which are
//! kept, the tool body), removals, undo and redo. After each step: the
//! document passes its check, the cache warm and cold give the same
//! evaluation; every split that worked gave the piece it keeps to its
//! body and the other to its new body (each exactly the booleans' front
//! or back, worked out here for a tool body; their volumes adding up to
//! the body's for regions), noted the pair, left every other body alone,
//! and one that failed changed nothing; every sketch on a face that's
//! placed is on that face's plane, the face found on the body holding it
//! or on a piece a split cut from that; every later edit can still be
//! made; and the document survives its bytes, flipped bits included.

use varde_document::{Combine, Plane};
use varde_kernel::mesh::Form;
use varde_kernel::{Budget, Op};

use super::super::combine::fuzz::{
    Rng, bytes, not_stuck, random_combine, random_shape, same_bodies, solid, truncated,
};
use super::*;
use crate::picking::region_form;

/// The bodies of `evaluation` with solids of their own.
fn owned(evaluation: &Evaluation) -> Vec<BodyId> {
    evaluation.bodies.iter().map(|made| made.body).collect()
}

/// A random flat face of `body` in `evaluation`, `document`'s, now and
/// then named on the body its maker made (as a reference made before a
/// split moved it, or an edit swapped the pieces, has it).
fn random_flat_face(
    document: &Document,
    evaluation: &Evaluation,
    body: BodyId,
    rng: &mut Rng,
) -> Option<FaceRef> {
    let solid = solid(evaluation, body)?;
    let topology = solid.topology();
    let regions: Vec<_> = (topology.regions().iter())
        .filter(|region| matches!(region_form(solid, region), Form::Plane { .. }))
        .collect();
    let region = regions.get(rng.below(regions.len()))?;
    let patch = solid.mesh().patch(region.tris[0] as usize);
    let made = (document.bodies().iter())
        .find(|made| made.created_by.get() == region.key.feature)
        .map(|made| made.id);
    let body = match made {
        Some(made) if rng.below(2) == 0 => made,
        _ => body,
    };
    Some(FaceRef {
        body,
        key: region.key,
        near: patch.eval(DVec3::splat(1.0 / 3.0)),
    })
}

/// A random split of one of `evaluation`'s bodies (the history as it
/// ends), by one of its tools: `editor` gets the sketch a regions' or
/// line's tool needs.
fn random_split(editor: &mut Editor, rng: &mut Rng) -> Option<Split> {
    let evaluation = evaluated(editor.document());
    let bodies = owned(&evaluation);
    if bodies.is_empty() {
        return None;
    }
    let body = bodies[rng.below(bodies.len())];
    let others: Vec<BodyId> = bodies.iter().copied().filter(|&b| b != body).collect();
    // Half the body's box, or a disc in its middle, on XY.
    let bounds = solid(&evaluation, body)?.bounds3()?;
    let (low, high) = (bounds.min, bounds.max);
    let middle = (low + high) / 2.0;
    let half: Box<dyn FnOnce(&mut Sketch)> = if rng.below(3) == 0 {
        let radius = (high - low).x.min((high - low).y) / 4.0;
        Box::new(disc((middle.x, middle.y), radius.max(0.5)))
    } else {
        Box::new(rectangle(
            (low.x - 1.0, low.y - 1.0),
            (middle.x, high.y + 1.0),
        ))
    };
    let tool = match rng.below(9) {
        0 | 1 if !others.is_empty() => SplitTool::Body(others[rng.below(others.len())]),
        0..=3 => {
            // A tool body made for it, over the body's height and more.
            let extent = two_sides(
                editor.document(),
                &format!("{}", high.z + 1.0),
                &format!("{}", 1.0 - low.z),
            );
            add_extrude(editor, half, extent, Operation::NewBody(BodyId::NEW));
            SplitTool::Body(editor.document().bodies().last()?.id)
        }
        4 | 5 => {
            let sketch = add_sketch(editor, half);
            let FeatureKind::Sketch { sketch: drawn, .. } =
                &editor.document().feature(sketch).unwrap().kind
            else {
                unreachable!()
            };
            let region = drawn.profiles().ok()?.reference(0)?;
            SplitTool::Regions {
                sketch,
                regions: vec![region],
            }
        }
        6 => SplitTool::Plane(PlaneRef::Origin(
            [OriginPlane::XY, OriginPlane::XZ, OriginPlane::YZ][rng.below(3)],
        )),
        7 => {
            let face = random_flat_face(
                editor.document(),
                &evaluation,
                bodies[rng.below(bodies.len())],
                rng,
            )?;
            if rng.below(2) == 0 {
                SplitTool::Plane(PlaneRef::Face(face))
            } else {
                SplitTool::Face(face)
            }
        }
        _ => {
            let (sketch, curves) = polyline(editor, &[(-1.0, 3.0), (12.0, 3.0), (12.0, 20.0)]);
            SplitTool::Chain { sketch, curves }
        }
    };
    Some(Split {
        body,
        tool,
        original: [Side::Front, Side::Back][rng.below(2)],
        keep: [Keep::Both, Keep::Both, Keep::Front, Keep::Back][rng.below(4)],
        new_body: None,
    })
}

/// One of `split`'s parts changed: which piece keeps the id, which are
/// kept, or its tool body another.
fn tweaked(split: &Split, document: &Document, rng: &mut Rng) -> Split {
    let mut split = split.clone();
    match rng.below(3) {
        0 => split.original = split.original.other(),
        1 => split.keep = [Keep::Both, Keep::Front, Keep::Back][rng.below(3)],
        _ => {
            if let SplitTool::Body(_) = split.tool {
                let bodies = document.bodies();
                split.tool = SplitTool::Body(bodies[rng.below(bodies.len())].id);
            }
        }
    }
    split
}

fn close(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= 1e-6 * scale.max(1.0)
}

/// Holds every split of `document` (evaluated whole as `evaluation`) to
/// what the module's docs say, against the history before and after it.
fn check_splits(document: &Document, evaluation: &Evaluation, cache: &mut Cache, what: &str) {
    let tolerance = document.tolerance();
    for (index, feature) in document.features().iter().enumerate() {
        let FeatureKind::Split(split) = &feature.kind else {
            continue;
        };
        let what = format!("{what}: split {index} {split:?}");
        let fails = |e: &Evaluation| e.failed.iter().any(|f| f.feature == feature.id);
        let before = evaluate(&truncated(document, index), cache);
        let after = evaluate(&truncated(document, index + 1), cache);
        assert_eq!(fails(evaluation), fails(&after), "{what}");
        if fails(&after) {
            assert!(
                same_bodies(&before, &after),
                "{what}: failing, it changed bodies"
            );
            assert_eq!(before.splits, after.splits, "{what}");
            continue;
        }
        let body = solid(&before, split.body).expect("a body worked on");
        let kept = solid(&after, split.body).expect("the body keeps a piece");
        let other = split.made_body().map(|new| {
            assert_eq!(after.bodies.last().unwrap().body, new, "{what}: last");
            assert_eq!(after.splits.last(), Some(&(split.body, new)), "{what}");
            solid(&after, new).expect("the new body has the other piece")
        });
        assert_eq!(other.is_some(), split.keeps_both(), "{what}");
        if let SplitTool::Body(tool) = split.tool {
            // The booleans' pieces, worked out here.
            let tool = solid(&before, tool).expect("a tool body");
            let piece = |op| varde_kernel::boolean(body, tool, op, &tolerance, &Budget::DEFAULT);
            let front = piece(Op::Intersection).unwrap();
            let back = piece(Op::Difference).unwrap();
            let (mine, theirs) = match split.kept() {
                Side::Front => (front, back),
                Side::Back => (back, front),
            };
            assert_eq!(**kept, mine, "{what}: the kept piece");
            if let Some(other) = other {
                assert_eq!(**other, theirs, "{what}: the other piece");
            }
        } else if let Some(other) = other {
            let (whole, a, b) = (body.volume(), kept.volume(), other.volume());
            assert!(
                close(a + b, whole, whole),
                "{what}: {a} + {b} isn't {whole}"
            );
        } else {
            assert!(kept.volume() < body.volume(), "{what}: a trim");
        }
        // Every other body left alone.
        for made in &before.bodies {
            if made.body != split.body {
                assert_eq!(
                    solid(&after, made.body),
                    Some(&made.solid),
                    "{what}: {:?} changed",
                    made.body
                );
            }
        }
        assert_eq!(
            after.bodies.len(),
            before.bodies.len() + usize::from(split.keeps_both()),
            "{what}"
        );
    }
}

/// Holds every sketch on a face of `document` that's placed to that
/// face's plane, the face found on the body holding the face's body
/// before the sketch, or on a piece a split cut from that (and so on).
fn check_face_sketches(
    document: &Document,
    evaluation: &Evaluation,
    cache: &mut Cache,
    what: &str,
) {
    for (index, feature) in document.features().iter().enumerate() {
        let FeatureKind::Sketch {
            plane: Plane::Face(face),
            ..
        } = &feature.kind
        else {
            continue;
        };
        let Some((_, placement)) = (evaluation.placements.iter()).find(|(id, _)| *id == feature.id)
        else {
            continue;
        };
        let before = evaluate(&truncated(document, index), cache);
        let holder = before.holder(face.body).expect("placed on a body held");
        // The holder, and the pieces splits cut from it.
        let mut on = vec![holder];
        let mut at = 0;
        while let Some(&body) = on.get(at) {
            at += 1;
            for &(split, new) in &before.splits {
                let new = before.holder(new);
                if before.holder(split) == Some(body)
                    && let Some(new) = new
                    && !on.contains(&new)
                {
                    on.push(new);
                }
            }
        }
        let n = placement.normal;
        let d = n.dot(placement.origin);
        let found = on.iter().any(|&body| {
            let solid = solid(&before, body).unwrap();
            let topology = solid.topology();
            (topology.regions().iter()).any(|region| {
                region.key == face.key
                    && matches!(*region_form(solid, region), Form::Plane { n: m, d: e }
                        if m.abs_diff_eq(n, 1e-9) && (e - d).abs() < 1e-6)
            })
        });
        assert!(
            found,
            "{what}: sketch {index} on {face:?} placed off its face"
        );
    }
}

fn run(seed: u64, steps: usize) {
    with_booleans();
    let mut rng = Rng(0x2545_f491_4f6c_dd1d ^ (seed + 1).wrapping_mul(0x9e37_79b9));
    let mut editor = Editor::new(Document::default());
    let mut cache = Cache::default();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let document = editor.document().clone();
        match rng.below(12) {
            0..=2 => {
                let height = ["5", "10", "15"][rng.below(3)];
                let operation = match rng.below(5) {
                    0 => Operation::Join(Targets::default()),
                    1 => Operation::Cut(Targets::default()),
                    _ => Operation::NewBody(BodyId::NEW),
                };
                let extent = Extent::OneSide(length(&document, height));
                add_extrude(&mut editor, random_shape(&mut rng), extent, operation);
            }
            3..=5 => {
                if let Some(split) = random_split(&mut editor, &mut rng) {
                    let add = editor.document().add_feature(split.into());
                    let _ = editor.apply(add);
                }
            }
            6 => {
                if let Some(combine) = random_combine(&document, &mut rng, None) {
                    let _ = editor.apply(document.add_feature(Combine::into(combine)));
                }
            }
            7 => {
                // A sketch on a flat face, joined to by a block on it now
                // and then.
                let evaluation = evaluated(&document);
                let bodies = owned(&evaluation);
                if !bodies.is_empty() {
                    let body = bodies[rng.below(bodies.len())];
                    if let Some(face) = random_flat_face(&document, &evaluation, body, &mut rng) {
                        let _ = editor.apply(document.add_sketch(Plane::Face(face)));
                    }
                }
            }
            8 => {
                // An earlier split edited.
                let splits: Vec<(FeatureId, Split)> = (document.features().iter())
                    .filter_map(|feature| match &feature.kind {
                        FeatureKind::Split(split) => Some((feature.id, split.clone())),
                        _ => None,
                    })
                    .collect();
                if !splits.is_empty() {
                    let (id, split) = &splits[rng.below(splits.len())];
                    let kind = tweaked(split, &document, &mut rng);
                    let _ = editor.apply(Command::SetFeature {
                        feature: *id,
                        kind: Box::new(kind.into()),
                    });
                }
            }
            9 => {
                let features = document.features();
                if !features.is_empty() {
                    let id = features[rng.below(features.len())].id;
                    editor.apply(Command::RemoveFeature(id)).unwrap();
                }
            }
            10 if editor.can_undo() => editor.undo(),
            _ if editor.can_redo() => editor.redo(),
            _ => {}
        }
        let document = editor.document();
        document
            .check()
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        cache.begin();
        let warm = evaluate(document, &mut cache);
        let cold = evaluated(document);
        assert!(same_bodies(&warm, &cold), "{what}: warm and cold differ");
        assert_eq!(warm.splits, cold.splits, "{what}");
        check_splits(document, &warm, &mut cache, &what);
        check_face_sketches(document, &warm, &mut cache, &what);
        if step % 8 == 7 {
            not_stuck(document, &what);
            bytes(document, &mut rng, &mut cache, &what);
        }
    }
}

/// See the module's docs. `VARDE_SPLIT_SEEDS` runs more seeds
/// (`VARDE_SPLIT_FROM` the first).
#[test]
fn random_histories_with_splits_hold() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let from = number("VARDE_SPLIT_FROM", 0);
    for seed in from..from.saturating_add(number("VARDE_SPLIT_SEEDS", 3)) {
        run(seed, 40);
    }
}
