//! Random histories of blocks and discs made as bodies, joins and cuts,
//! combines, and chamfers (the kernel's chamfer replaced by the prism
//! stand-in) of one to four edges of a body as the history left it,
//! now and then named on the body their faces' maker made (one a join
//! or combine used up since), of every size, flipped or not, with
//! tangent chains or not; earlier chamfers edited, upstream extrudes
//! changed (moving the edges, or taking their faces away), removals,
//! undo and redo, chamfers drafted. After each step: the document
//! passes its check, the cache warm and cold give the same evaluation;
//! every chamfer fails alike in the whole history and in the history
//! ending with it, one that failed changed nothing and one that worked
//! took material off its body alone, keeping its id; every later edit
//! can still be made; the document survives its bytes, flipped bits
//! included; and a request with a chamfer's draft and its answer cross
//! the wire as they went.

use super::super::combine::fuzz::{
    Rng, bytes, not_stuck, random_combine, random_shape, same_bodies, solid, truncated, wire,
};
use super::*;
use crate::{Draft, Regenerator};

const DISTANCES: [&str; 6] = ["0.5", "1", "2", "3", "7.5", "20"];
const ANGLES: [&str; 5] = ["10", "30", "45", "60", "89"];

/// A random size for a chamfer in `document`.
fn random_size(document: &Document, rng: &mut Rng) -> ChamferSize {
    let length = |rng: &mut Rng| distance(document, DISTANCES[rng.below(DISTANCES.len())]);
    match rng.below(3) {
        0 => ChamferSize::Equal(length(rng)),
        1 => ChamferSize::Two(length(rng), length(rng)),
        _ => ChamferSize::Angle(
            length(rng),
            angle(document, ANGLES[rng.below(ANGLES.len())]),
        ),
    }
}

/// A random chamfer of one to four edges of a body of `document` as it
/// regenerates, named by their regions' keys at their first curve's
/// chord's middle; now and then on the body their first face's maker
/// made instead.
fn random_chamfer(document: &Document, rng: &mut Rng) -> Option<Chamfer> {
    let evaluation = evaluated(document);
    let made = evaluation.bodies.get(rng.below(evaluation.bodies.len()))?;
    let solid = &made.solid;
    let topology = solid.topology();
    let regions = topology.regions();
    let chains = topology.chains();
    if chains.is_empty() {
        return None;
    }
    let mut body = made.body;
    let mut edges = Vec::new();
    for _ in 0..1 + rng.below(4) {
        let chain = &chains[rng.below(chains.len())];
        let mut faces = chain.regions.map(|r| regions[r as usize].key);
        if faces[0] == faces[1] {
            continue;
        }
        faces.sort();
        let curve = solid.mesh().curve(chain.halfedges[0]);
        let near = (curve.p0 + curve.p1) / 2.0;
        edges.push(EdgeRef { body, faces, near });
    }
    if edges.is_empty() {
        return None;
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
    Some(Chamfer {
        edges,
        distances: random_size(document, rng),
        chains: rng.below(3) != 0,
        flip: rng.below(2) == 0,
    })
}

/// One of `chamfer`'s parts changed: its size, flip, chains, or an edge
/// taken out.
fn tweaked(chamfer: &Chamfer, document: &Document, rng: &mut Rng) -> Chamfer {
    let mut chamfer = chamfer.clone();
    match rng.below(4) {
        0 => chamfer.distances = random_size(document, rng),
        1 => chamfer.flip = !chamfer.flip,
        2 => chamfer.chains = !chamfer.chains,
        _ if chamfer.edges.len() > 1 => {
            chamfer.edges.remove(rng.below(chamfer.edges.len()));
        }
        _ => {}
    }
    chamfer
}

/// Holds every chamfer of `document` (evaluated whole as `evaluation`)
/// to what the module's docs say, against the history before and after
/// it.
fn check_chamfers(document: &Document, evaluation: &Evaluation, cache: &mut Cache, what: &str) {
    for (index, feature) in document.features().iter().enumerate() {
        let FeatureKind::Chamfer(chamfer) = &feature.kind else {
            continue;
        };
        let what = format!("{what}: chamfer {index} {chamfer:?}");
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
        let body = chamfer.body().unwrap();
        let was = solid(&before, body).expect("a body worked on");
        let now = solid(&after, body).expect("the body keeps its id");
        assert!(
            now.volume() < was.volume(),
            "{what}: {} not under {}",
            now.volume(),
            was.volume()
        );
        assert_eq!(after.merged, before.merged, "{what}");
        let ids = |e: &Evaluation| e.bodies.iter().map(|m| m.body).collect::<Vec<_>>();
        assert_eq!(ids(&after), ids(&before), "{what}");
        for made in &before.bodies {
            if made.body != body {
                assert_eq!(
                    solid(&after, made.body),
                    Some(&made.solid),
                    "{what}: {:?} changed",
                    made.body
                );
            }
        }
    }
}

fn run(seed: u64, steps: usize) {
    with_wedges();
    let mut rng = Rng(0x2545_f491_4f6c_dd1d ^ (seed + 1).wrapping_mul(0x5bd1_e995));
    let mut editor = Editor::new(Document::default());
    let mut cache = Cache::default();
    let mut regenerator = Regenerator::default();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let document = editor.document().clone();
        let roll = if step < 2 { 0 } else { rng.below(14) };
        let mut draft = None;
        let mut apply = |command: Command| {
            let _ = editor.apply(command);
        };
        match roll {
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
                if let Some(chamfer) = random_chamfer(&document, &mut rng) {
                    apply(document.add_feature(chamfer.into()));
                }
            }
            6 => {
                if let Some(combine) = random_combine(&document, &mut rng, None) {
                    apply(document.add_feature(combine.into()));
                }
            }
            7 => {
                // An earlier chamfer edited.
                let chamfers: Vec<(FeatureId, Chamfer)> = (document.features().iter())
                    .filter_map(|feature| match &feature.kind {
                        FeatureKind::Chamfer(chamfer) => Some((feature.id, chamfer.clone())),
                        _ => None,
                    })
                    .collect();
                if !chamfers.is_empty() {
                    let (id, chamfer) = &chamfers[rng.below(chamfers.len())];
                    let kind = tweaked(chamfer, &document, &mut rng);
                    apply(Command::SetFeature {
                        feature: *id,
                        kind: Box::new(kind.into()),
                    });
                }
            }
            8 => {
                // Upstream: an extrude's height, direction or operation.
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
                        extrude.operation = match rng.below(3) {
                            0 => Operation::NewBody(BodyId::NEW),
                            1 => Operation::Join(Targets::default()),
                            _ => Operation::Cut(Targets::default()),
                        };
                    }
                    apply(Command::SetFeature {
                        feature: feature.id,
                        kind: Box::new(extrude.into()),
                    });
                }
            }
            9 => {
                let features = document.features();
                if !features.is_empty() {
                    apply(Command::RemoveFeature(
                        features[rng.below(features.len())].id,
                    ));
                }
            }
            10 if editor.can_undo() => editor.undo(),
            11 if editor.can_redo() => editor.redo(),
            _ => {
                // A chamfer drafted, new or in place of an earlier one.
                let chamfers: Vec<FeatureId> = (document.features().iter())
                    .filter(|f| matches!(f.kind, FeatureKind::Chamfer(_)))
                    .map(|f| f.id)
                    .collect();
                let edited = (!chamfers.is_empty() && rng.below(2) == 0)
                    .then(|| chamfers[rng.below(chamfers.len())]);
                if let Some(chamfer) = random_chamfer(&document, &mut rng) {
                    draft = Some(Draft {
                        revision: step as u64,
                        feature: edited,
                        kind: chamfer.into(),
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
        check_chamfers(&document, &warm, &mut cache, &what);
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
fn random_histories_with_chamfers_hold() {
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
const QUICK_STEPS: usize = 10;
