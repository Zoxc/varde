//! Random histories of blocks and discs made as bodies, joins, cuts and
//! intersects over them, combines of every operation (keeping their
//! tools or not, naming bodies earlier joins and combines used up),
//! upstream edits, removals, undo and redo. After each step: the cache
//! warm and cold give the same evaluation; every combine that worked is
//! what direct kernel booleans give on the same bodies, its tools
//! consumed or kept and every other body left alone, and one that failed
//! changed nothing; every later edit can still be made; the document
//! survives its bytes, flipped bits included; and a request with a
//! combine's draft and its answer cross the wire as they went.

use varde_kernel::{Budget, Op};

use super::*;
use crate::wire::{decode_reply, decode_request, encode_reply, encode_request};
use crate::{Regenerator, Request, Response};

/// A small deterministic generator: xorshift64*.
pub(in crate::history::tests) struct Rng(pub(in crate::history::tests) u64);

impl Rng {
    pub(in crate::history::tests) fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub(in crate::history::tests) fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const OPS: [BodyOp; 3] = [BodyOp::Union, BodyOp::Subtract, BodyOp::Intersect];

/// A block or a disc on a 5 mm grid, so neighbours overlap or are flush.
pub(in crate::history::tests) fn random_shape(rng: &mut Rng) -> Box<dyn FnOnce(&mut Sketch)> {
    let x = 5.0 * rng.below(7) as f64;
    let y = 5.0 * rng.below(4) as f64;
    if rng.below(4) == 0 {
        Box::new(disc((x, y), [2.5, 5.0, 7.5][rng.below(3)]))
    } else {
        let w = 5.0 * (1 + rng.below(3)) as f64;
        let h = 5.0 * (1 + rng.below(3)) as f64;
        Box::new(rectangle((x, y), (x + w, y + h)))
    }
}

/// A combine of `document`'s bodies made before feature `before` (all
/// without), consumed or not, with a random operation and keep.
pub(in crate::history::tests) fn random_combine(
    document: &Document,
    rng: &mut Rng,
    before: Option<usize>,
) -> Option<Combine> {
    let index = |id: FeatureId| document.features().iter().position(|f| f.id == id);
    let bodies: Vec<BodyId> = (document.bodies().iter())
        .filter(|body| before.is_none_or(|before| index(body.created_by) < Some(before)))
        .map(|body| body.id)
        .collect();
    if bodies.len() < 2 {
        return None;
    }
    let target = bodies[rng.below(bodies.len())];
    let others: Vec<BodyId> = bodies.into_iter().filter(|&b| b != target).collect();
    let mut tools: Vec<BodyId> = (others.iter().copied())
        .filter(|_| rng.below(2) == 0)
        .collect();
    if tools.is_empty() {
        tools.push(others[rng.below(others.len())]);
    }
    Some(Combine {
        target,
        tools,
        op: OPS[rng.below(3)],
        keep_tools: rng.below(3) == 0,
    })
}

/// `document` with the features from `index` on removed.
pub(in crate::history::tests) fn truncated(document: &Document, index: usize) -> Document {
    let mut editor = Editor::new(document.clone());
    for feature in document.features()[index..].iter().rev() {
        if editor.document().feature(feature.id).is_some() {
            editor.apply(Command::RemoveFeature(feature.id)).unwrap();
        }
    }
    editor.document().clone()
}

pub(in crate::history::tests) fn solid(
    evaluation: &Evaluation,
    body: BodyId,
) -> Option<&Arc<Solid>> {
    (evaluation.bodies.iter())
        .find(|made| made.body == body)
        .map(|made| &made.solid)
}

/// Whether two evaluations hold the same bodies, to the bit, merged the
/// same way.
pub(in crate::history::tests) fn same_bodies(a: &Evaluation, b: &Evaluation) -> bool {
    let ids = |e: &Evaluation| e.bodies.iter().map(|m| m.body).collect::<Vec<_>>();
    ids(a) == ids(b)
        && a.merged == b.merged
        && (a.bodies.iter().zip(&b.bodies)).all(|(x, y)| x.solid == y.solid)
}

fn near(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= 1e-6 * scale.max(1.0)
}

/// Holds every combine of `document` (evaluated whole as `evaluation`)
/// to direct kernel booleans on the bodies as the features before it
/// leave them.
fn check_combines(document: &Document, evaluation: &Evaluation, cache: &mut Cache, what: &str) {
    let tolerance = document.tolerance();
    for (index, feature) in document.features().iter().enumerate() {
        let FeatureKind::Combine(combine) = &feature.kind else {
            continue;
        };
        let what = format!("{what}: combine {index}");
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
        let target = solid(&before, combine.target).expect("a target worked on");
        let tools: Vec<(BodyId, &Arc<Solid>)> = (before.bodies.iter())
            .filter(|made| combine.tools.contains(&made.body))
            .map(|made| (made.body, &made.solid))
            .collect();
        assert_eq!(tools.len(), combine.tools.len(), "{what}");
        let op = match combine.op {
            BodyOp::Union => Op::Union,
            BodyOp::Subtract => Op::Difference,
            BodyOp::Intersect => Op::Intersection,
        };
        // Step by step, each against the volume of the intersection: the
        // boolean identities. A union step failing here was retried.
        let mut running = Some(Arc::clone(target));
        for (_, tool) in &tools {
            let Some(solid) = running.take() else {
                break;
            };
            let Ok(next) = varde_kernel::boolean(&solid, tool, op, &tolerance, &Budget::DEFAULT)
            else {
                assert_eq!(op, Op::Union, "{what}: a step failed that regen took");
                break;
            };
            let common = Op::Intersection;
            if let Ok(common) =
                varde_kernel::boolean(&solid, tool, common, &tolerance, &Budget::DEFAULT)
            {
                let (a, b, c, n) = (
                    solid.volume(),
                    tool.volume(),
                    common.volume(),
                    next.volume(),
                );
                let holds = match op {
                    Op::Union => near(n + c, a + b, a + b),
                    Op::Difference => near(n + c, a, a + b),
                    Op::Intersection => near(n, c, a + b),
                };
                assert!(holds, "{what}: {op:?} of {a} and {b}, common {c}, gave {n}");
            }
            running = Some(Arc::new(next));
        }
        let result = solid(&after, combine.target).expect("the target has a solid");
        if let Some(direct) = running {
            assert_eq!(**result, *direct, "{what}: not the kernel's booleans");
        }
        for (tool, was) in &tools {
            match solid(&after, *tool) {
                Some(kept) => assert!(combine.keep_tools && kept == *was, "{what}: {tool:?}"),
                None => {
                    assert!(!combine.keep_tools, "{what}: {tool:?} used up");
                    assert_eq!(after.holder(*tool), Some(combine.target), "{what}");
                }
            }
        }
        for made in &before.bodies {
            if made.body != combine.target && !combine.tools.contains(&made.body) {
                let still = solid(&after, made.body);
                assert_eq!(still, Some(&made.solid), "{what}: {:?} changed", made.body);
            }
        }
    }
}

/// Every later edit can be made to `document`: each feature and body
/// removed, each feature set as it is, each combine's operation and keep
/// changed, a new body added.
pub(in crate::history::tests) fn not_stuck(document: &Document, what: &str) {
    let edits = |document: &Document| {
        let mut edits = Vec::new();
        for feature in document.features() {
            edits.push(Command::RemoveFeature(feature.id));
            if !matches!(feature.kind, FeatureKind::Sketch { .. }) {
                edits.push(Command::SetFeature {
                    feature: feature.id,
                    kind: Box::new(feature.kind.clone()),
                });
            }
            if let FeatureKind::Combine(combine) = &feature.kind {
                for op in OPS {
                    for keep_tools in [false, true] {
                        let kind = Combine {
                            op,
                            keep_tools,
                            ..combine.clone()
                        };
                        edits.push(Command::SetFeature {
                            feature: feature.id,
                            kind: Box::new(kind.into()),
                        });
                    }
                }
            }
        }
        edits.extend(document.bodies().iter().map(|b| Command::RemoveBody(b.id)));
        edits
    };
    for edit in edits(document) {
        let shown = format!("{edit:?}");
        let mut editor = Editor::new(document.clone());
        editor
            .apply(edit)
            .unwrap_or_else(|error| panic!("{what}: {shown}: {error}"));
    }
    let mut editor = Editor::new(document.clone());
    block(&mut editor, 100.0, 100.0, 110.0, 110.0, "5");
}

/// The document through postcard and its MessagePack by name, whole and
/// with bits flipped: never a panic, and what's taken passes its check,
/// regenerates and can still be edited.
pub(in crate::history::tests) fn bytes(
    document: &Document,
    rng: &mut Rng,
    cache: &mut Cache,
    what: &str,
) {
    let postcard = document.to_postcard();
    assert_eq!(Document::from_postcard(&postcard).as_ref(), Ok(document));
    let named = rmp_serde::to_vec_named(document).unwrap();
    let unchecked: varde_document::Unchecked = rmp_serde::from_slice(&named).unwrap();
    assert_eq!(unchecked.check().as_ref(), Ok(document), "{what}");
    for _ in 0..8 {
        let named_bytes = rng.below(2) == 0;
        let mut flipped = if named_bytes {
            named.clone()
        } else {
            postcard.clone()
        };
        for _ in 0..1 + rng.below(3) {
            let at = rng.below(flipped.len());
            flipped[at] ^= 1 << rng.below(8);
        }
        let taken = if named_bytes {
            (rmp_serde::from_slice::<varde_document::Unchecked>(&flipped).ok())
                .and_then(|unchecked| unchecked.check().ok())
        } else {
            Document::from_postcard(&flipped).ok()
        };
        if let Some(taken) = taken {
            taken.check().unwrap();
            cache.begin();
            evaluate(&taken, cache);
            not_stuck(&taken, &format!("{what}: flipped"));
        }
    }
}

/// A request for `editor`'s document with `draft`, and its answer, across
/// the wire both ways.
pub(in crate::history::tests) fn wire(
    editor: &Editor,
    draft: Option<Draft>,
    regenerator: &mut Regenerator,
    what: &str,
) {
    let request = Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: draft.map(Box::new),
        inspect: None,
    };
    let sent = encode_request(&request);
    let decoded = decode_request(&sent).unwrap();
    assert_eq!(encode_request(&decoded), sent, "{what}");
    let response = regenerator.handle(decoded);
    let (head, parts) = encode_reply(&response);
    let parts: Vec<&[u8]> = parts.iter().map(|part| &**part).collect();
    let back = decode_reply(&head[..], &parts).unwrap();
    let (
        Response::Regenerated {
            draft,
            failed,
            merged,
            bodies,
            ..
        },
        Response::Regenerated {
            draft: draft_back,
            failed: failed_back,
            merged: merged_back,
            bodies: bodies_back,
            ..
        },
    ) = (&response, &back)
    else {
        panic!("{what}: not regenerated: {back:?}");
    };
    assert_eq!(draft, draft_back, "{what}");
    assert_eq!(failed, failed_back, "{what}");
    assert_eq!(merged, merged_back, "{what}");
    assert_eq!(bodies, bodies_back, "{what}");
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ (seed + 1).wrapping_mul(0x1000_0001));
    let mut editor = Editor::new(Document::default());
    let mut warm = Cache::default();
    let mut regenerator = Regenerator::default();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let document = editor.document().clone();
        let roll = if step < 3 { 0 } else { rng.below(12) };
        let mut draft = None;
        // Each edit is one the editor takes, or refuses for what it
        // names: either way the document stays checked.
        let mut apply = |command: Command| {
            let _ = editor.apply(command);
        };
        match roll {
            0 | 1 => {
                let shape = random_shape(&mut rng);
                let height = ["5", "10", "7.5"][rng.below(3)];
                let extent = Extent::OneSide(length(&document, height));
                add_extrude(&mut editor, shape, extent, Operation::NewBody(BodyId::NEW));
            }
            2 => {
                let shape = random_shape(&mut rng);
                let mut excluded: Vec<BodyId> = (document.bodies().iter())
                    .map(|body| body.id)
                    .filter(|_| rng.below(4) == 0)
                    .collect();
                excluded.sort_unstable();
                let targets = Targets {
                    excluded,
                    held: None,
                };
                let operation = match rng.below(3) {
                    0 => Operation::Join(targets),
                    1 => Operation::Cut(targets),
                    _ => Operation::Intersect(targets),
                };
                let height = ["5", "10", "15"][rng.below(3)];
                let extent = Extent::OneSide(length(&document, height));
                add_extrude(&mut editor, shape, extent, operation);
            }
            3..=5 => {
                if let Some(combine) = random_combine(&document, &mut rng, None) {
                    editor
                        .apply(document.add_feature(combine.into()))
                        .unwrap_or_else(|error| panic!("{what}: {error}"));
                }
            }
            6 => {
                // An earlier combine changed: keep, operation or bodies.
                let combines: Vec<usize> = (0..document.features().len())
                    .filter(|&i| matches!(document.features()[i].kind, FeatureKind::Combine(_)))
                    .collect();
                if !combines.is_empty() {
                    let index = combines[rng.below(combines.len())];
                    let feature = &document.features()[index];
                    let FeatureKind::Combine(mut combine) = feature.kind.clone() else {
                        unreachable!()
                    };
                    match rng.below(3) {
                        0 => combine.keep_tools = !combine.keep_tools,
                        1 => combine.op = OPS[rng.below(3)],
                        _ => {
                            if let Some(again) = random_combine(&document, &mut rng, Some(index)) {
                                combine = again;
                            }
                        }
                    }
                    let set = Command::SetFeature {
                        feature: feature.id,
                        kind: Box::new(combine.into()),
                    };
                    editor
                        .apply(set)
                        .unwrap_or_else(|error| panic!("{what}: {error}"));
                }
            }
            7 => {
                // Upstream: an extrude's height, direction or operation
                // (refused where it would stop making a body a combine
                // names).
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
                    if rng.below(4) == 0 {
                        extrude.operation = match rng.below(4) {
                            0 => Operation::NewBody(BodyId::NEW),
                            1 => Operation::Join(Targets::default()),
                            2 => Operation::Cut(Targets::default()),
                            _ => Operation::Intersect(Targets::default()),
                        };
                    }
                    apply(Command::SetFeature {
                        feature: feature.id,
                        kind: Box::new(extrude.into()),
                    });
                }
            }
            8 => {
                if rng.below(2) == 0 && !document.features().is_empty() {
                    let feature = &document.features()[rng.below(document.features().len())];
                    apply(Command::RemoveFeature(feature.id));
                } else if !document.bodies().is_empty() {
                    let body = &document.bodies()[rng.below(document.bodies().len())];
                    apply(Command::RemoveBody(body.id));
                }
            }
            9 => editor.undo(),
            10 => editor.redo(),
            _ => {
                // A combine drafted, new or in place of an earlier one.
                let combines: Vec<usize> = (0..document.features().len())
                    .filter(|&i| matches!(document.features()[i].kind, FeatureKind::Combine(_)))
                    .collect();
                let edited = (!combines.is_empty() && rng.below(2) == 0)
                    .then(|| combines[rng.below(combines.len())]);
                if let Some(combine) = random_combine(&document, &mut rng, edited) {
                    draft = Some(Draft {
                        revision: step as u64,
                        feature: edited.map(|index| document.features()[index].id),
                        kind: combine.into(),
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
        assert_eq!(hot.touched, cold.touched, "{what}");
        check_combines(&document, &hot, &mut warm, &what);
        if step % 4 == 3 {
            not_stuck(&document, &what);
            bytes(&document, &mut rng, &mut warm, &what);
        }
        wire(&editor, draft, &mut regenerator, &what);
    }
}

/// See the module's docs. Quick runs seed 0's first `QUICK_STEPS` steps;
/// `VARDE_TESTS=full` runs seeds 0 to 2 to 24 steps, and
/// `VARDE_TEST_SEED` replays one seed to 24 steps. Each step evaluates
/// the whole history cold, so a run's time grows with its steps squared.
#[test]
fn random_histories_with_combines_hold() {
    let replay = varde_testing::replay_seed().is_some();
    let steps = if replay {
        24
    } else {
        varde_testing::pick(QUICK_STEPS, 24)
    };
    for seed in varde_testing::seeds(1, 3) {
        run(seed, steps);
    }
}

/// The steps of the quick run.
const QUICK_STEPS: usize = 8;
