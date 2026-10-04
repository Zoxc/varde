//! Random sequences of what the user can do around the draft session,
//! regeneration drafting boxes' sides
//! ([`varde_regen::testing::draft_by_boxes`]): starting and editing
//! drafts (from idle, or with an offset face or a shell being set up),
//! faces clicked (on the model shown or one gone by, edges and other
//! bodies' faces among them) to pick them or take them out, clicked one
//! after another without the models answered between them, rows'
//! crosses, the neutral plane's row toggled and the plane picked as a
//! face (flat or not, of the draft's body or another, one of the faces
//! drafted, faces already drafted) or an origin plane from the toolbar
//! (also while the plane doesn't pick), Flip and Tangent faces, angles
//! typed (out of range, overflowing, not numbers among them), units
//! changed, rows hovered, the list of what overlaps opened on the model
//! shown (while the plane picks too) and its rows hovered, ticked and
//! chosen, undo and redo (taking the neutral face's body away and back
//! among them), joins and combines merging bodies before the draft (the
//! neutral face's body among them, and undone), bodies added and
//! features removed, commits, Add anyway and cancels, edits taking every
//! face out then a merge.
//!
//! After each step: a session that's ready is whole, passes its own
//! check and the document's for its faces and neutral plane, its faces
//! on one body that the feature can name and that is the session's, and
//! is previewed as set up; one ready whose preview didn't fail commits,
//! and the document holds what it drafted; the faces lit are on the body
//! drawing the faces' body; the overlap list is of the model shown and
//! its ticks are the session's own (none while the plane picks); the
//! neutral plane drawn is finite, its pull unit; a draft edited opens to
//! what it stores and OK on it straight away writes nothing; a session
//! never outlives the draft it edits; the panel's texts never panic; and
//! the document passes its check. Each draft the model shows working was
//! of a box, its pull along a world axis, and gives the volume its
//! box's cross-sections give.

use glam::DVec2;
use varde_document::{BodyId, BodyOp, Combine, Command, FeatureId, Operation, Targets};
use varde_expr::LengthUnit;
use varde_kernel::mesh::Form;
use varde_view::{OverlapItems, Overlaps, PanelHover, Pick};

use super::super::chamfer::fuzz::Rng;
use super::super::shell::fuzz::random_pick;
use super::*;
use crate::tests::{add_disc, screen_texts, two_sides};

const ANGLES: &[&str] = &[
    "3", "1", "10", "0.5°", "30", "45", "60 deg", "89.9999", "90", "0", "-3", "180", "1e308",
    "1e400", "1/0", "0.05 rad", "1e-9", "abc", "", "3 mm",
];

/// The drafts of `plates`' document.
fn drafts(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::FaceDraft(_)))
        .map(|feature| feature.id)
        .collect()
}

/// Holds the session, if it's a draft's, to what the module's docs say.
fn check_session(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    // The overlap list's ticks are the session's own while it picks.
    if let (Some(ticks), Some(listed)) = (plates.doc.overlap_ticks(), &plates.doc.overlaps)
        && let OverlapItems::Model(picks) = &listed.list.items
    {
        for (&tick, &pick) in ticks.iter().zip(picks) {
            assert_eq!(Some(tick), plates.doc.motion_has(pick), "{what}");
        }
    }
    if session.kind != MotionKind::Draft {
        return;
    }
    if session.picking == MotionPick::Reference {
        assert_eq!(plates.doc.overlap_ticks(), None, "{what}");
    }
    let document = plates.doc.editor.document();
    if let Some(id) = session.feature {
        let kind = document.feature(id).map(|feature| &feature.kind);
        assert!(matches!(kind, Some(FeatureKind::FaceDraft(_))), "{what}");
    }
    let faces = &session.faces.refs;
    assert!(
        faces.windows(2).all(|pair| pair[0].order(&pair[1]).is_lt()),
        "{what}: {faces:?}"
    );
    assert!(
        faces.iter().all(|face| face.body == faces[0].body),
        "{what}: {faces:?}"
    );
    assert!(session.bodies.len() <= 1, "{what}: {:?}", session.bodies);
    if let Some(face) = faces.first() {
        assert_eq!(session.bodies, [face.body], "{what}");
    }
    check_lit(plates, what);
    if let Some(state) = plates.doc.motion_state()
        && let Some([at, pull]) = state.line
    {
        assert!(at.is_finite() && pull.is_finite(), "{what}: {at} {pull}");
        assert!((pull.length() - 1.0).abs() < 1e-9, "{what}: {pull}");
    }
    if !plates.doc.motion_ready() {
        return;
    }
    let draft = session
        .face_draft()
        .unwrap_or_else(|| panic!("{what}: ready, not whole"));
    draft
        .check_own(&document.design())
        .unwrap_or_else(|why| panic!("{what}: {why}: {draft:?}"));
    let features = document.features();
    let index = (session.feature)
        .and_then(|id| features.iter().position(|feature| feature.id == id))
        .unwrap_or(features.len());
    let body = draft.body().expect("faces");
    document
        .check_face_set(index, body, &draft.faces)
        .unwrap_or_else(|why| panic!("{what}: {why}: {draft:?}"));
    document
        .check_neutral_plane(index, &draft.neutral)
        .unwrap_or_else(|why| panic!("{what}: {why}: {draft:?}"));
    assert!(
        crate::doc::combine::pickable(document, body, session.feature),
        "{what}: {draft:?}"
    );
    assert_eq!(
        plates.doc.motion_draft(),
        Some((session.feature, FeatureKind::FaceDraft(draft))),
        "{what}"
    );
}

/// Holds the faces lit on a model of the document as it is to the body
/// drawing the faces' body.
fn check_lit(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    let lit = plates.doc.faces_lit();
    let current = plates.doc.feed.generation() == Some(plates.doc.editor.generation());
    if lit.is_empty() || !current {
        return;
    }
    let body =
        (session.faces.body()).unwrap_or_else(|| panic!("{what}: lit {lit:?} with no faces"));
    let merged = plates.doc.feed.merged_bodies();
    let shown = (merged.iter())
        .find(|(consumed, _)| *consumed == body)
        .map_or(body, |&(_, holder)| holder);
    let index = plates.doc.feed.pick_index();
    for target in lit {
        assert_eq!(index.body(target), Some(shown), "{what}: lit {target:?}");
    }
}

/// Edits the draft `id`, holding the session to what it stores, and OK
/// on it straight away to writing nothing.
fn edit_and_ok(plates: &mut Plates, id: FeatureId, what: &str) {
    let Some(FeatureKind::FaceDraft(stored)) =
        (plates.doc.editor.document().feature(id)).map(|feature| feature.kind.clone())
    else {
        return;
    };
    plates.doc.look(Look::EditFeature(id));
    let Some(session) = plates.doc.motion.as_ref() else {
        return;
    };
    assert_eq!(session.feature, Some(id), "{what}");
    assert_eq!(session.face_draft(), Some(stored), "{what}");
    let revision = plates.doc.editor.revision();
    let document = plates.doc.editor.document().clone();
    plates.doc.update(Edit::CommitMotion);
    if plates.doc.motion.is_none() {
        assert_eq!(plates.doc.editor.revision(), revision, "{what}");
        assert_eq!(*plates.doc.editor.document(), document, "{what}");
    }
}

/// A join or a combine merging bodies: a disc joined across the first
/// box, or one body combined into another (the session's body or its
/// neutral face's into another, or another into it, among them).
fn merge(plates: &mut Plates, rng: &mut Rng) {
    if rng.below(3) == 0 {
        let extent = two_sides(plates.doc.editor.document(), "12", "1");
        let operation = Operation::Join(Targets::default());
        add_disc(&mut plates.doc.editor, (40.0, 15.0), extent, operation);
    } else {
        let document = plates.doc.editor.document();
        let all: Vec<BodyId> = document.bodies().iter().map(|body| body.id).collect();
        if all.len() < 2 {
            return;
        }
        let mut target = *rng.pick(&all);
        let mut tool = *rng.pick(&all);
        let session = plates.doc.motion.as_ref();
        let neutral = session.and_then(|session| match session.plane {
            Some(PlaneRef::Face(face)) => Some(face.body),
            _ => None,
        });
        let own = session.and_then(|session| session.bodies.first().copied());
        if let Some(body) = [own, neutral][rng.below(2)]
            && rng.below(3) != 0
        {
            if rng.below(2) == 0 {
                tool = body;
            } else {
                target = body;
            }
        }
        if target == tool {
            return;
        }
        let combine = Combine {
            target,
            tools: vec![tool],
            op: BodyOp::Union,
            keep_tools: rng.below(3) == 0,
        };
        let add = document.add_feature(combine.into());
        plates.doc.apply(add);
    }
    plates.doc.sync();
}

/// The list of two to four things of the model shown, as the left
/// button held still over them lists them.
fn random_overlaps(plates: &Plates, rng: &mut Rng) -> Option<Overlaps> {
    let mut picks: Vec<Pick> = Vec::new();
    for _ in 0..2 + rng.below(3) {
        if let Some(mut pick) = random_pick(plates, rng) {
            pick.model = plates.doc.feed.pick_index().model();
            if !picks.contains(&pick) {
                picks.push(pick);
            }
        }
    }
    (picks.len() > 1).then_some(Overlaps {
        held: DVec2::ZERO,
        at: DVec2::ZERO,
        items: OverlapItems::Model(picks),
    })
}

/// The form of the region `face` names on `solid`, if found.
fn form_of<'a>(solid: &'a varde_kernel::Solid, face: &varde_document::FaceRef) -> Option<&'a Form> {
    let topology = solid.topology();
    let region = topology.face(solid, &face.key, face.near).ok()?;
    let mesh = solid.mesh();
    let tri = &mesh.tris()[topology.regions()[region as usize].tris[0] as usize];
    Some(&mesh.faces()[tri.face as usize].form)
}

/// The volume `draft` of `solid` should give, the history before it
/// `before`, if `solid` is a box along the axes, the pull along an axis
/// and the draft can work: the integral of its cross-sections along the
/// pull, each side drafted moving in by `tan α` times the height above
/// the neutral plane.
fn wanted_volume(
    solid: &varde_kernel::Solid,
    before: &varde_regen::Evaluation,
    draft: &FaceDraft,
) -> Option<f64> {
    let topology = solid.topology();
    let bounds = solid.bounds3()?;
    let size = bounds.max - bounds.min;
    let whole = size.x * size.y * size.z;
    if topology.regions().len() != 6 || (solid.volume() - whole).abs() > 1e-9 * whole {
        return None;
    }
    let (normal, point) = match &draft.neutral {
        PlaneRef::Origin(origin) => (origin.placement().normal, DVec3::ZERO),
        PlaneRef::Face(face) => {
            let holder = before.holder(face.body)?;
            let made = before.bodies.iter().find(|made| made.body == holder)?;
            match *form_of(&made.solid, face)? {
                Form::Plane { n, d } => (n, n * d),
                _ => return None,
            }
        }
    };
    let up = normal.abs().max_position();
    if (normal.abs()[up] - 1.0).abs() > 1e-12 {
        return None;
    }
    let sign = normal[up].signum() * if draft.flip { -1.0 } else { 1.0 };
    let mut regions = Vec::new();
    for face in &draft.faces {
        regions.push(topology.face(solid, &face.key, face.near).ok()?);
    }
    regions.sort_unstable();
    regions.dedup();
    let mut drafted = [0.0f64; 3];
    for region in regions {
        let mesh = solid.mesh();
        let tri = &mesh.tris()[topology.regions()[region as usize].tris[0] as usize];
        let Form::Plane { n, .. } = mesh.faces()[tri.face as usize].form else {
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
    let area = |h: f64| width(a, h) * width(b, h);
    Some((hi - lo) / 6.0 * (area(lo) + 4.0 * area((lo + hi) / 2.0) + area(hi)))
}

/// Holds each draft the model shows working to the volume
/// [`wanted_volume`] gives.
fn check_volumes(plates: &Plates, what: &str) {
    let document = plates.doc.editor.document();
    let failed = plates.doc.feed.failed_features();
    for (at, feature) in document.features().iter().enumerate() {
        let FeatureKind::FaceDraft(draft) = &feature.kind else {
            continue;
        };
        if failed.iter().any(|failed| failed.feature == feature.id) {
            continue;
        }
        let body = draft.body().unwrap();
        let evaluation = |count: usize| {
            let mut cut = Editor::new(document.clone());
            while cut.document().features().len() > count {
                let last = cut.document().features().last().unwrap().id;
                cut.apply(Command::RemoveFeature(last)).unwrap();
            }
            varde_regen::evaluate(cut.document(), &mut varde_regen::Cache::default())
        };
        let (before, after) = (evaluation(at), evaluation(at + 1));
        let solid = |e: &varde_regen::Evaluation| {
            (e.bodies.iter())
                .find(|made| made.body == body)
                .map(|made| made.solid.clone())
        };
        let (Some(was), Some(now)) = (solid(&before), solid(&after)) else {
            panic!("{what}: {draft:?} works on no body");
        };
        let wanted = wanted_volume(&was, &before, draft)
            .unwrap_or_else(|| panic!("{what}: {draft:?} worked, giving {}", now.volume()));
        let got = now.volume();
        assert!(
            (got - wanted).abs() <= 1e-6 * was.volume().max(wanted),
            "{what}: {got} vs {wanted}"
        );
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0xbb67_ae85_84ca_a73b ^ (seed + 1).wrapping_mul(0x9e37_79b9));
    varde_regen::testing::draft_by_boxes();
    let mut plates = boxes(true);
    let base = plates.doc.editor.document().features().len();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let open = plates.doc.motion.is_some();
        let roll = match rng.below(4) {
            0 if !open => 0,
            _ => rng.below(54),
        };
        match roll {
            0 | 1 if open && rng.below(6) != 0 => plates.answer(),
            0 | 1 => plates.doc.look(Look::StartDraft),
            2 => {
                let ids = drafts(&plates);
                if !ids.is_empty() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                }
            }
            3..=9 => {
                if let Some(pick) = random_pick(&plates, &mut rng) {
                    click(&mut plates, pick);
                }
            }
            10 => {
                // Quick clicks: several, nothing answered between them,
                // the plane's row toggled among them now and then.
                for _ in 0..2 + rng.below(3) {
                    if let Some(pick) = random_pick(&plates, &mut rng) {
                        click(&mut plates, pick);
                    }
                    match rng.below(5) {
                        0 => plates.doc.look(Look::StartDraft),
                        1 => plates.motion(MotionLook::Picking(MotionPick::Reference)),
                        _ => {}
                    }
                }
            }
            11 => {
                let faces = (plates.doc.motion.as_ref())
                    .map(|session| session.faces.refs.clone())
                    .unwrap_or_default();
                if !faces.is_empty() {
                    plates.motion(MotionLook::DropFace(*rng.pick(&faces)));
                }
            }
            12 => plates.motion(MotionLook::Flip),
            13 => plates.motion(MotionLook::TangentFaces),
            14..=16 => plates.input(MotionField::Angle, rng.pick(ANGLES)),
            17..=19 => {
                // The neutral plane's row, then a face clicked or an
                // origin plane.
                plates.motion(MotionLook::Picking(MotionPick::Reference));
                if rng.below(3) == 0 {
                    plates.answer();
                }
                match rng.below(3) {
                    0 => {
                        let plane = *rng.pick(&[OriginPlane::XY, OriginPlane::XZ, OriginPlane::YZ]);
                        plates.motion(MotionLook::OriginPlane(plane));
                    }
                    _ => {
                        if let Some(pick) = random_pick(&plates, &mut rng) {
                            click(&mut plates, pick);
                        }
                    }
                }
            }
            20 => {
                let units = *rng.pick(&LengthUnit::ALL);
                plates.doc.update(Edit::SetUnits(units));
            }
            21 => {
                let at = rng.below(4);
                if rng.below(2) == 0 {
                    plates
                        .doc
                        .look(Look::HoverPanel(Some(PanelHover::Face(at))));
                } else {
                    plates.doc.look(Look::LeavePanel(PanelHover::Face(at)));
                }
            }
            22 => {
                if let Some(pick) = random_pick(&plates, &mut rng) {
                    plates.doc.look(Look::Hover(Some(pick)));
                }
            }
            23 | 24 if rng.below(2) == 0 => {
                plates.doc.update(Edit::Undo);
                if plates.doc.editor.document().features().len() < base {
                    plates.doc.update(Edit::Redo);
                }
            }
            25 if rng.below(2) == 0 => plates.doc.update(Edit::Redo),
            26..=28 => {
                let before = plates.doc.motion.as_ref().and_then(|session| {
                    let ready =
                        plates.doc.motion_ready() && plates.doc.feed.draft_error().is_none();
                    (ready && session.kind == MotionKind::Draft)
                        .then(|| (session.feature, session.face_draft()))
                });
                plates.doc.update(Edit::CommitMotion);
                if let Some((edited, Some(draft))) = before {
                    assert!(
                        plates.doc.motion.is_none(),
                        "{what}: not committed: {:?}",
                        plates.doc.edit_error
                    );
                    let id = edited.unwrap_or_else(|| plates.last_feature().0);
                    let stored = &plates.doc.editor.document().feature(id).unwrap().kind;
                    assert_eq!(*stored, FeatureKind::FaceDraft(draft), "{what}");
                }
            }
            // Add anyway, past a failed preview.
            29 => plates.doc.update(Edit::AcceptError),
            30 if rng.below(3) == 0 => plates.motion(MotionLook::Cancel),
            31 | 32 => merge(&mut plates, &mut rng),
            33 if rng.below(2) == 0 => {
                if rng.below(2) == 0 {
                    let mut editor = plates.doc.editor.clone();
                    let x = 100.0 + 30.0 * rng.below(3) as f64;
                    add_box(&mut editor, DVec2::new(x, 0.0), DVec2::new(x + 20.0, 10.0));
                    let document = editor.document().clone();
                    let added =
                        document.features().len() - plates.doc.editor.document().features().len();
                    for feature in &document.features()[document.features().len() - added..] {
                        let add = plates
                            .doc
                            .editor
                            .document()
                            .add_feature(feature.kind.clone());
                        plates.doc.apply(add);
                    }
                    plates.doc.sync();
                } else {
                    // A feature after the boxes removed, with what it made.
                    let features = plates.doc.editor.document().features();
                    if features.len() > base {
                        let id = features[base + rng.below(features.len() - base)].id;
                        plates.doc.apply(Command::RemoveFeature(id));
                        plates.doc.sync();
                    }
                }
            }
            34 => {
                let ids = drafts(&plates);
                if !ids.is_empty() {
                    let id = *rng.pick(&ids);
                    edit_and_ok(&mut plates, id, &what);
                }
            }
            35 => {
                // An edit taking every face out, then its body merged.
                let ids = drafts(&plates);
                if !ids.is_empty() && plates.doc.motion.is_none() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                    let faces = (plates.doc.motion.as_ref())
                        .map(|session| session.faces.refs.clone())
                        .unwrap_or_default();
                    for face in faces {
                        plates.motion(MotionLook::DropFace(face));
                    }
                    if rng.below(2) == 0 {
                        plates.answer();
                    }
                    merge(&mut plates, &mut rng);
                }
            }
            36 => {
                // A merge, undone at once, redone now and then.
                merge(&mut plates, &mut rng);
                if rng.below(2) == 0 {
                    plates.answer();
                }
                plates.doc.update(Edit::Undo);
                if rng.below(3) == 0 {
                    plates.answer();
                    plates.doc.update(Edit::Redo);
                }
            }
            37 => {
                // A draft with an offset face or a shell being set up.
                if rng.below(2) == 0 {
                    plates.doc.look(Look::StartShell);
                } else {
                    plates.doc.look(Look::StartOffsetFace);
                }
                if rng.below(2) == 0
                    && let Some(pick) = random_pick(&plates, &mut rng)
                {
                    click(&mut plates, pick);
                }
                if rng.below(2) == 0 {
                    plates.answer();
                }
                plates.doc.look(Look::StartDraft);
            }
            38 | 39 => {
                if rng.below(3) == 0 {
                    plates.motion(MotionLook::Picking(MotionPick::Reference));
                }
                if let Some(list) = random_overlaps(&plates, &mut rng) {
                    plates.doc.look(Look::OpenOverlaps(list));
                }
            }
            40..=42 => {
                let rows =
                    (plates.doc.overlaps.as_ref()).map_or(0, |listed| listed.list.items.len());
                let row = rng.below(rows + 1);
                match rng.below(4) {
                    0 => plates.doc.look(Look::HoverOverlap(Some(row))),
                    1 => plates.doc.look(Look::ChooseOverlap {
                        index: row,
                        add: false,
                    }),
                    _ => plates.doc.look(Look::ToggleOverlap(row)),
                }
            }
            43 => {
                // An origin plane from the toolbar, whatever picks.
                let plane = *rng.pick(&[OriginPlane::XY, OriginPlane::XZ, OriginPlane::YZ]);
                plates.motion(MotionLook::OriginPlane(plane));
            }
            _ => plates.answer(),
        }
        if rng.below(3) == 0 {
            plates.answer();
        }
        // The list of what overlaps is of the model shown.
        if let Some(listed) = &plates.doc.overlaps
            && let OverlapItems::Model(picks) = &listed.list.items
        {
            assert!(!picks.is_empty(), "{what}");
            let model = plates.doc.feed.model();
            assert!(picks.iter().all(|pick| pick.model == model), "{what}");
        }
        check_session(&plates, &what);
        let _ = screen_texts(&plates.doc);
        plates
            .doc
            .editor
            .document()
            .check()
            .unwrap_or_else(|error| panic!("{what}: {error}"));
    }
    plates.doc.motion = None;
    plates.doc.sync();
    plates.answer();
    check_volumes(&plates, &format!("seed {seed} at the end"));
}

/// See the module's docs. `VARDE_DRAFT_SEEDS` runs more seeds
/// (`VARDE_DRAFT_FROM` the first).
#[test]
fn random_draft_sessions_hold() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let from = number("VARDE_DRAFT_FROM", 0);
    for seed in from..from.saturating_add(number("VARDE_DRAFT_SEEDS", 3)) {
        run(seed, 200);
    }
}
