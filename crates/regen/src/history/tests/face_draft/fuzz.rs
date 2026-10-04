//! Random histories of blocks and discs made as bodies, joins and cuts,
//! combines, and drafts (the kernel's draft replaced by the box
//! stand-in) of one to four faces of a body as the history left it
//! (tops and bottoms facing the pull, opposite sides and one face named
//! twice among them), now and then named on the body their faces'
//! maker made (one a join or combine used up since), about an origin
//! plane or a flat face of a body (the draft's own, one of its faces
//! drafted, another body's, one a combine merged before it), at angles
//! small to steep, flipped or not, tangent faces taken in or not;
//! earlier drafts edited (the neutral plane among what changes),
//! upstream extrudes changed (moving the faces, or taking them away),
//! removals, undo and redo, drafts drafted. After each step: the
//! document passes its check, the cache warm and cold give the same
//! evaluation; every draft fails alike in the whole history and in the
//! history ending with it, one that failed changed nothing, and one
//! that worked was of a box, its pull along a world axis, and gave the
//! volume the box's cross-sections give, every face keeping its name,
//! its body alone changed, keeping its id; every later edit can still
//! be made; the document survives its bytes, flipped bits included;
//! and a request with a draft's draft and its answer cross the wire as
//! they went.

use varde_document::{OriginPlane, PlaneRef};

use super::super::combine::fuzz::{
    Rng, bytes, not_stuck, random_combine, random_shape, same_bodies, solid, truncated, wire,
};
use super::*;
use crate::picking::region_form;
use crate::{Draft, Regenerator};

const ANGLES: [&str; 10] = [
    "0.5", "1", "3", "10", "26.5", "30", "45", "60", "80", "89.999",
];

/// A face of `made`'s solid at random, named by its key at a
/// triangle's middle, on `body`.
fn random_face(made: &crate::history::BodySolid, body: BodyId, rng: &mut Rng) -> Option<FaceRef> {
    let solid = &made.solid;
    let topology = solid.topology();
    let regions = topology.regions();
    if regions.is_empty() {
        return None;
    }
    let region = &regions[rng.below(regions.len())];
    let tri = region.tris[rng.below(region.tris.len())] as usize;
    let p = solid.mesh().patch(tri).p;
    Some(FaceRef {
        body,
        key: region.key,
        near: (p[0] + p[1] + p[2]) / 3.0,
    })
}

/// A random neutral plane: an origin plane (XY mostly), or a face of a
/// body of `evaluation` of `document` (now and then one of `faces`, or
/// named on the body its maker made), flat or not.
fn random_neutral(
    document: &Document,
    evaluation: &Evaluation,
    faces: &[FaceRef],
    rng: &mut Rng,
) -> PlaneRef {
    match rng.below(8) {
        0..=2 => PlaneRef::Origin(OriginPlane::XY),
        3 => PlaneRef::Origin([OriginPlane::XZ, OriginPlane::YZ][rng.below(2)]),
        4 if !faces.is_empty() => PlaneRef::Face(faces[rng.below(faces.len())]),
        _ => {
            let Some(made) = evaluation.bodies.get(rng.below(evaluation.bodies.len())) else {
                return PlaneRef::Origin(OriginPlane::XY);
            };
            let Some(mut face) = random_face(made, made.body, rng) else {
                return PlaneRef::Origin(OriginPlane::XY);
            };
            let maker = face.key.feature;
            if rng.below(4) == 0
                && let Some(other) =
                    (document.bodies().iter()).find(|b| b.created_by.get() == maker)
            {
                face.body = other.id;
            }
            PlaneRef::Face(face)
        }
    }
}

/// A random draft of one to four faces of a body of `document` as it
/// regenerates, each named by its key at a triangle's middle (or
/// twice); now and then on the body the first face's maker made
/// instead; about a random neutral plane.
fn random_draft(document: &Document, rng: &mut Rng) -> Option<FaceDraft> {
    let evaluation = evaluated(document);
    let made = evaluation.bodies.get(rng.below(evaluation.bodies.len()))?;
    let mut body = made.body;
    let mut faces = Vec::new();
    for _ in 0..1 + rng.below(4) {
        faces.push(random_face(made, body, rng)?);
    }
    if rng.below(6) == 0 {
        let maker = faces[0].key.feature;
        if let Some(other) = (document.bodies().iter()).find(|b| b.created_by.get() == maker) {
            body = other.id;
            for face in &mut faces {
                face.body = body;
            }
        }
    }
    faces.sort_by(FaceRef::order);
    faces.dedup_by(|a, b| a.order(b).is_eq());
    let neutral = random_neutral(document, &evaluation, &faces, rng);
    Some(FaceDraft {
        faces,
        neutral,
        angle: angle(document, ANGLES[rng.below(ANGLES.len())]),
        flip: rng.below(3) == 0,
        tangent: rng.below(3) != 0,
    })
}

/// One of `draft`'s parts changed: its angle, flip, tangent faces, its
/// neutral plane, or a face taken out.
fn tweaked(draft: &FaceDraft, document: &Document, rng: &mut Rng) -> FaceDraft {
    let mut draft = draft.clone();
    match rng.below(5) {
        0 => draft.angle = angle(document, ANGLES[rng.below(ANGLES.len())]),
        1 => draft.flip = !draft.flip,
        2 => draft.tangent = !draft.tangent,
        3 => draft.neutral = random_neutral(document, &evaluated(document), &draft.faces, rng),
        _ if draft.faces.len() > 1 => {
            draft.faces.remove(rng.below(draft.faces.len()));
        }
        _ => {}
    }
    draft
}

/// The neutral plane of `draft` in `before` (the history before it), as
/// a unit normal and the point of it nearest the origin, if found and
/// flat.
fn neutral_plane(before: &Evaluation, draft: &FaceDraft) -> Option<(DVec3, DVec3)> {
    match &draft.neutral {
        PlaneRef::Origin(origin) => Some((origin.placement().normal, DVec3::ZERO)),
        PlaneRef::Face(face) => {
            // Found on the body holding its body's solid (a combine may
            // have merged it).
            let solid = solid(before, before.holder(face.body)?)?;
            let topology = solid.topology();
            let region = topology.face(solid, &face.key, face.near).ok()?;
            match *region_form(solid, &topology.regions()[region as usize]) {
                Form::Plane { n, d } => Some((n, n * d)),
                _ => None,
            }
        }
    }
}

/// The volume `draft` of `solid` should give, the history before it
/// `before`, if `solid` is a box along the axes, the pull along an axis
/// and the draft can work: the integral of its cross-sections along the
/// pull, each side drafted moving in by `tan α` times the height above
/// the neutral plane. `None` for one that can't work.
fn wanted_volume(solid: &Solid, before: &Evaluation, draft: &FaceDraft) -> Option<f64> {
    let topology = solid.topology();
    let bounds = solid.bounds3()?;
    let size = bounds.max - bounds.min;
    let whole = size.x * size.y * size.z;
    if topology.regions().len() != 6 || (solid.volume() - whole).abs() > 1e-9 * whole {
        return None;
    }
    let (normal, point) = neutral_plane(before, draft)?;
    let up = normal.abs().max_position();
    if (normal.abs()[up] - 1.0).abs() > 1e-12 {
        return None;
    }
    let sign = normal[up].signum() * if draft.flip { -1.0 } else { 1.0 };
    let mut regions = Vec::new();
    for face in &draft.faces {
        regions.push(topology.face(solid, &face.key, face.near).ok()?);
    }
    // A region named twice is drafted once.
    regions.sort_unstable();
    regions.dedup();
    let mut drafted = [0.0f64; 3];
    for region in regions {
        let Form::Plane { n, .. } = *region_form(solid, &topology.regions()[region as usize])
        else {
            return None;
        };
        let axis = n.abs().max_position();
        if axis == up {
            return None;
        }
        drafted[axis] += 1.0;
    }
    let t = draft.angle.value.tan();
    let width = |axis: usize, h: f64| size[axis] - drafted[axis] * t * sign * (h - point[up]);
    let (a, b) = ((up + 1) % 3, (up + 2) % 3);
    let (lo, hi) = (bounds.min[up], bounds.max[up]);
    for h in [lo, hi] {
        if width(a, h) <= 1e-6 || width(b, h) <= 1e-6 {
            return None;
        }
    }
    Some(integral(lo, hi, |h| width(a, h) * width(b, h)))
}

/// Holds every draft of `document` (evaluated whole as `evaluation`)
/// to what the module's docs say, against the history before and after
/// it.
fn check_drafts(document: &Document, evaluation: &Evaluation, cache: &mut Cache, what: &str) {
    for (index, feature) in document.features().iter().enumerate() {
        let FeatureKind::FaceDraft(draft) = &feature.kind else {
            continue;
        };
        let what = format!("{what}: draft {index} {draft:?}");
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
        let body = draft.body().unwrap();
        let was = solid(&before, body).expect("a body worked on");
        let now = solid(&after, body).expect("the body keeps its id");
        let wanted = wanted_volume(was, &before, draft)
            .unwrap_or_else(|| panic!("{what}: worked, giving {}", now.volume()));
        let got = now.volume();
        assert!(
            (got - wanted).abs() <= 1e-6 * was.volume().max(wanted),
            "{what}: {got} vs {wanted}"
        );
        let keys = |solid: &Solid| {
            let mut keys: Vec<FaceKey> = (solid.topology().regions().iter())
                .map(|region| region.key)
                .collect();
            keys.sort();
            keys
        };
        assert_eq!(keys(now), keys(was), "{what}: names not kept");
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
    with_boxes();
    let mut rng = Rng(0x6a09_e667_f3bc_c908 ^ (seed + 1).wrapping_mul(0x5bd1_e995));
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
                let extent = if rng.below(4) == 0 {
                    two_sides(&document, height, "2.5")
                } else {
                    Extent::OneSide(length(&document, height))
                };
                add_extrude(&mut editor, random_shape(&mut rng), extent, operation);
            }
            3..=5 => {
                if let Some(kind) = random_draft(&document, &mut rng) {
                    apply(document.add_feature(kind.into()));
                }
            }
            6 => {
                if let Some(combine) = random_combine(&document, &mut rng, None) {
                    apply(document.add_feature(combine.into()));
                }
            }
            7 => {
                // An earlier draft edited.
                let drafts: Vec<(FeatureId, FaceDraft)> = (document.features().iter())
                    .filter_map(|feature| match &feature.kind {
                        FeatureKind::FaceDraft(draft) => Some((feature.id, draft.clone())),
                        _ => None,
                    })
                    .collect();
                if !drafts.is_empty() {
                    let (id, kind) = &drafts[rng.below(drafts.len())];
                    let kind = tweaked(kind, &document, &mut rng);
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
                // A draft drafted, new or in place of an earlier one.
                let drafts: Vec<FeatureId> = (document.features().iter())
                    .filter(|f| matches!(f.kind, FeatureKind::FaceDraft(_)))
                    .map(|f| f.id)
                    .collect();
                let edited = (!drafts.is_empty() && rng.below(2) == 0)
                    .then(|| drafts[rng.below(drafts.len())]);
                if let Some(kind) = random_draft(&document, &mut rng) {
                    draft = Some(Draft {
                        revision: step as u64,
                        feature: edited,
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
        check_drafts(&document, &warm, &mut cache, &what);
        if step % 8 == 7 {
            not_stuck(&document, &what);
            bytes(&document, &mut rng, &mut cache, &what);
        }
        wire(&editor, draft, &mut regenerator, &what);
    }
}

/// See the module's docs. `VARDE_DRAFT_SEEDS` runs more seeds
/// (`VARDE_DRAFT_FROM` the first).
#[test]
fn random_histories_with_drafts_hold() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let from = number("VARDE_DRAFT_FROM", 0);
    for seed in from..from.saturating_add(number("VARDE_DRAFT_SEEDS", 3)) {
        run(seed, 40);
    }
}
