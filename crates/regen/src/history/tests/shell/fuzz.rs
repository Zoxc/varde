//! Random histories of blocks and discs made as bodies, joins and cuts,
//! combines, and shells (the kernel's shell replaced by the box
//! stand-in) of a body as the history left it, opening none to four of
//! its faces (opposite ones and one face named twice among them), now
//! and then named on the body their faces' maker made (one a join or
//! combine used up since), at thicknesses about half a side and past a
//! side, inward or outward; earlier shells edited, upstream extrudes
//! changed (moving the faces, or taking them away), removals, undo and
//! redo, shells drafted. After each step: the document passes its
//! check, the cache warm and cold give the same evaluation; every shell
//! fails alike in the whole history and in the history ending with it,
//! one that failed changed nothing, and one that worked was of a box
//! and gave the volume the box, its open faces, the thickness and the
//! direction give, its body alone changed, keeping its id; every later
//! edit can still be made; the document survives its bytes, flipped
//! bits included; and a request with a shell's draft and its answer
//! cross the wire as they went.

use super::super::combine::fuzz::{
    Rng, bytes, not_stuck, random_combine, random_shape, same_bodies, solid, truncated, wire,
};
use super::*;
use crate::picking::region_form;
use crate::{Draft, Regenerator};

const THICKNESSES: [&str; 9] = [
    "0.5",
    "1",
    "2",
    "2.4999999",
    "2.5",
    "4.9999999",
    "5",
    "7.5",
    "15",
];

/// A random shell of a body of `document` as it regenerates, opening
/// none to four of its regions, each named by its key at its first
/// triangle's middle (now and then at another's, or twice); now and then
/// on the body the first face's maker made instead.
fn random_shell(document: &Document, rng: &mut Rng) -> Option<Shell> {
    let evaluation = evaluated(document);
    let made = evaluation.bodies.get(rng.below(evaluation.bodies.len()))?;
    let solid = &made.solid;
    let topology = solid.topology();
    let regions = topology.regions();
    if regions.is_empty() {
        return None;
    }
    let mut body = made.body;
    let mut open = Vec::new();
    for _ in 0..rng.below(5) {
        let region = &regions[rng.below(regions.len())];
        let tri = region.tris[rng.below(region.tris.len())] as usize;
        let p = solid.mesh().patch(tri).p;
        let near = (p[0] + p[1] + p[2]) / 3.0;
        open.push(FaceRef {
            body,
            key: region.key,
            near,
        });
    }
    if !open.is_empty() && rng.below(6) == 0 {
        let maker = open[0].key.feature;
        if let Some(other) = (document.bodies().iter()).find(|b| b.created_by.get() == maker) {
            body = other.id;
            for face in &mut open {
                face.body = body;
            }
        }
    }
    open.sort_by(FaceRef::order);
    open.dedup_by(|a, b| a.order(b).is_eq());
    Some(Shell {
        body,
        open,
        thickness: thickness(document, THICKNESSES[rng.below(THICKNESSES.len())]),
        outward: rng.below(3) == 0,
    })
}

/// One of `shell`'s parts changed: its thickness, direction, or a face
/// taken out.
fn tweaked(shell: &Shell, document: &Document, rng: &mut Rng) -> Shell {
    let mut shell = shell.clone();
    match rng.below(3) {
        0 => shell.thickness = thickness(document, THICKNESSES[rng.below(THICKNESSES.len())]),
        1 => shell.outward = !shell.outward,
        _ if !shell.open.is_empty() => {
            shell.open.remove(rng.below(shell.open.len()));
        }
        _ => {}
    }
    shell
}

/// The volume `shell` of `solid` should give, if `solid` is a box along
/// the axes and the shell leaves something: its box, the sides its
/// faces open, the thickness and the direction. `None` for one that
/// can't work.
fn wanted_volume(solid: &Solid, shell: &Shell) -> Option<f64> {
    let topology = solid.topology();
    let bounds = solid.bounds3()?;
    let size = bounds.max - bounds.min;
    let whole = size.x * size.y * size.z;
    if topology.regions().len() != 6 || (solid.volume() - whole).abs() > 1e-9 * whole {
        return None;
    }
    let mut opened = [[false; 2]; 3];
    for face in &shell.open {
        let region = topology.face(solid, &face.key, face.near).ok()?;
        let Form::Plane { n, .. } = *region_form(solid, &topology.regions()[region as usize])
        else {
            return None;
        };
        let axis = n.abs().max_position();
        opened[axis][usize::from(n[axis] > 0.0)] = true;
    }
    let t = shell.thickness.value;
    let closed = |axis: usize| f64::from(2 - opened[axis].iter().filter(|&&o| o).count() as u8);
    let volume = if shell.outward {
        (0..3).map(|a| size[a] + t * closed(a)).product::<f64>() - whole
    } else {
        let inner: Vec<f64> = (0..3).map(|a| size[a] - t * closed(a)).collect();
        if inner.iter().any(|&side| side <= 0.0) {
            return None;
        }
        whole - inner.iter().product::<f64>()
    };
    (volume > 0.0).then_some(volume)
}

/// Holds every shell of `document` (evaluated whole as `evaluation`) to
/// what the module's docs say, against the history before and after it.
fn check_shells(document: &Document, evaluation: &Evaluation, cache: &mut Cache, what: &str) {
    for (index, feature) in document.features().iter().enumerate() {
        let FeatureKind::Shell(shell) = &feature.kind else {
            continue;
        };
        let what = format!("{what}: shell {index} {shell:?}");
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
        let was = solid(&before, shell.body).expect("a body worked on");
        let now = solid(&after, shell.body).expect("the body keeps its id");
        let wanted = wanted_volume(was, shell)
            .unwrap_or_else(|| panic!("{what}: worked, giving {}", now.volume()));
        let got = now.volume();
        assert!(
            (got - wanted).abs() <= 1e-6 * was.volume().max(wanted),
            "{what}: {got} vs {wanted}"
        );
        assert_eq!(after.merged, before.merged, "{what}");
        let ids = |e: &Evaluation| e.bodies.iter().map(|m| m.body).collect::<Vec<_>>();
        assert_eq!(ids(&after), ids(&before), "{what}");
        for made in &before.bodies {
            if made.body != shell.body {
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
    with_boxes();
    let mut rng = Rng(0x5851_f42d_4c95_7f2d ^ (seed + 1).wrapping_mul(0x5bd1_e995));
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
                let operation = match rng.below(6) {
                    0 => Operation::Join(Targets::default()),
                    1 => Operation::Cut(Targets::default()),
                    _ => Operation::NewBody(BodyId::NEW),
                };
                let extent = Extent::OneSide(length(&document, height));
                add_extrude(&mut editor, random_shape(&mut rng), extent, operation);
            }
            3..=5 => {
                if let Some(shell) = random_shell(&document, &mut rng) {
                    apply(document.add_feature(shell.into()));
                }
            }
            6 => {
                if let Some(combine) = random_combine(&document, &mut rng, None) {
                    apply(document.add_feature(combine.into()));
                }
            }
            7 => {
                // An earlier shell edited.
                let shells: Vec<(FeatureId, Shell)> = (document.features().iter())
                    .filter_map(|feature| match &feature.kind {
                        FeatureKind::Shell(shell) => Some((feature.id, shell.clone())),
                        _ => None,
                    })
                    .collect();
                if !shells.is_empty() {
                    let (id, shell) = &shells[rng.below(shells.len())];
                    let kind = tweaked(shell, &document, &mut rng);
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
                // A shell drafted, new or in place of an earlier one.
                let shells: Vec<FeatureId> = (document.features().iter())
                    .filter(|f| matches!(f.kind, FeatureKind::Shell(_)))
                    .map(|f| f.id)
                    .collect();
                let edited = (!shells.is_empty() && rng.below(2) == 0)
                    .then(|| shells[rng.below(shells.len())]);
                if let Some(shell) = random_shell(&document, &mut rng) {
                    draft = Some(Draft {
                        revision: step as u64,
                        feature: edited,
                        kind: shell.into(),
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
        check_shells(&document, &warm, &mut cache, &what);
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
fn random_histories_with_shells_hold() {
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
const QUICK_STEPS: usize = 12;
