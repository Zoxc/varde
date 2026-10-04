//! Random sequences of what the user can do around the offset face
//! session, regeneration moving boxes' faces
//! ([`varde_regen::testing::offset_by_boxes`]): starting and editing
//! offsets (from idle, or with a shell or a chamfer being set up), faces
//! clicked (on the model shown or one gone by, edges and other bodies'
//! faces among them, the faces a preview moved too) to pick them or take
//! them out, clicked one after another without the models answered
//! between them, rows' crosses, Inward and Tangent faces, distances typed
//! (out of range, overflowing, not numbers among them) and dragged by the
//! handle (through zero, while the preview is on its way, the first face
//! changed between steps), units changed, rows hovered (past the last
//! among them), the list of what overlaps opened on the model shown and
//! its rows hovered, ticked and chosen, undo and redo, joins and combines
//! merging bodies before the offset (and undone), bodies added and
//! features removed, commits, Add anyway and cancels, edits taking every
//! face out then a merge.
//!
//! After each step: a session that's ready is whole, passes its own
//! check and the document's for its faces, its faces on one body that
//! the feature can name and that is the session's, and is previewed as
//! set up; one ready whose preview didn't fail commits, and the document
//! holds what it drafted; the faces lit are on the body drawing the
//! faces' body; the overlap list is of the model shown and its ticks are
//! the session's own; the handle's knob is at the distance as it reads
//! (negative inward), and, for a new offset whose working preview the
//! model shows as asked, stands on the first face as the preview moved
//! it with the handle on it as the document has it; an offset edited
//! opens to what it stores and OK on it straight away writes nothing; a
//! session never outlives the offset it edits; the panel's texts never
//! panic; and the document passes its check. Each offset the model shows
//! working was of a box and gives the volume its box, faces and signed
//! distance give.

use varde_document::{BodyOp, Combine, FeatureId, Targets};
use varde_expr::LengthUnit;
use varde_view::{OverlapItems, Overlaps, PanelHover};

use super::super::chamfer::fuzz::Rng;
use super::super::shell::fuzz::random_pick;
use super::*;
use crate::tests::{add_disc, two_sides};

const DISTANCES: &[&str] = &[
    "1",
    "2",
    "0.5",
    "3 mm",
    "0.1 in",
    "4.9999999",
    "5",
    "9.9999",
    "10",
    "15",
    "0",
    "-1",
    "1e308",
    "1e400",
    "1/0",
    "99999999999",
    "1e-9",
    "abc",
    "",
    "45°",
];

/// What the handle's drag sends (as the viewport formats it), the
/// signed distances in millimetres.
const DRAGS: &[f64] = &[
    1.0, -1.0, 2.5, -2.5, 0.1, -0.1, 9.0, -9.0, 12.0, -12.0, 1e-3,
];

/// The offset faces of `plates`' document.
fn offsets(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::OffsetFace(_)))
        .map(|feature| feature.id)
        .collect()
}

/// The plane `face` of the model shown lies on, if it's flat: its unit
/// normal and offset.
fn flat(plates: &Plates, face: u32) -> Option<(DVec3, f64)> {
    let index = plates.doc.feed.pick_index();
    match index.picking().faces().get(face as usize)?.summary {
        Summary::Plane { n, d } => Some((DVec3::from(n), d)),
        _ => None,
    }
}

/// Holds the handle, if the session shows one, to what the module's
/// docs say.
fn check_handle(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    let Some(handle) = handle(plates) else {
        return;
    };
    assert!(
        handle.origin.is_finite() && handle.normal.is_finite(),
        "{what}: {handle:?}"
    );
    assert!((handle.normal.length() - 1.0).abs() < 1e-9, "{what}");
    let reads = (session.field(MotionField::Distance).value.as_ref()).map_or(0.0, |v| v.value);
    let signed = if session.flip { -reads } else { reads };
    assert_eq!(handle.at, signed, "{what}");
    let feed = &plates.doc.feed;
    if session.feature.is_some()
        || !feed.answers_request()
        || !feed.shows_draft_of_run()
        || feed.draft_error().is_some()
        || feed.generation() != Some(plates.doc.editor.generation())
    {
        return;
    }
    // The model shown is the preview as asked: the knob on the first
    // face where it moved, along its normal (a flat face's exactly, not
    // to the drawn mesh's single precision).
    let first = session.faces.refs[0];
    let index = feed.pick_index();
    let Some((_, Some(found))) = session.faces.found(index.model()).next() else {
        return;
    };
    let Some((n, d)) = flat(plates, found) else {
        return;
    };
    let knob = handle.origin + handle.normal * handle.at;
    let off = |at: DVec3| 1e-9 * (1.0 + at.abs().max_element());
    assert!(
        (n.dot(knob) - d).abs() < off(knob),
        "{what}: knob {knob} off the face {n} · p = {d}"
    );
    assert!(
        n.distance(handle.normal) < 1e-6,
        "{what}: {n} vs {handle:?}"
    );
    // The handle stands on the face as the document has it.
    let evaluation = varde_regen::evaluate(
        plates.doc.editor.document(),
        &mut varde_regen::Cache::default(),
    );
    let Some(made) = (evaluation.bodies.iter()).find(|made| made.body == first.body) else {
        return;
    };
    let solid = &made.solid;
    let topology = solid.topology();
    let Ok(region) = topology.face(solid, &first.key, first.near) else {
        return;
    };
    let tri = topology.regions()[region as usize].tris[0] as usize;
    let p = solid.mesh().patch(tri).p[0];
    assert!(
        n.dot(handle.origin - p).abs() < off(knob).max(off(handle.origin)),
        "{what}: handle {} off the face as it is (through {p})",
        handle.origin
    );
}

/// Holds the session, if it's an offset face's, to what the module's
/// docs say.
fn check_session(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    // The overlap list's ticks are the session's own while it picks;
    // a row its preview took away is ticked while it still has it.
    if let (Some(ticks), Some(listed)) = (plates.doc.overlap_ticks(), &plates.doc.overlaps)
        && let OverlapItems::Model(picks) = &listed.list.items
    {
        for (tick, &pick) in ticks.iter().zip(picks) {
            if tick.note == varde_view::OverlapNote::Removed {
                continue;
            }
            if let Some(has) = plates.doc.motion_has(pick) {
                assert_eq!(tick.ticked, has, "{what}");
            }
        }
    }
    if session.kind != MotionKind::OffsetFace {
        return;
    }
    let document = plates.doc.editor.document();
    if let Some(id) = session.feature {
        let kind = document.feature(id).map(|feature| &feature.kind);
        assert!(matches!(kind, Some(FeatureKind::OffsetFace(_))), "{what}");
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
    check_handle(plates, what);
    if !plates.doc.motion_ready() {
        return;
    }
    let offset = session
        .offset_face()
        .unwrap_or_else(|| panic!("{what}: ready, not whole"));
    offset
        .check_own(&document.design())
        .unwrap_or_else(|why| panic!("{what}: {why}: {offset:?}"));
    let features = document.features();
    let index = (session.feature)
        .and_then(|id| features.iter().position(|feature| feature.id == id))
        .unwrap_or(features.len());
    let body = offset.body().expect("faces");
    document
        .check_face_set(index, body, &offset.faces)
        .unwrap_or_else(|why| panic!("{what}: {why}: {offset:?}"));
    assert!(
        crate::doc::combine::pickable(document, body, session.feature),
        "{what}: {offset:?}"
    );
    assert_eq!(
        plates.doc.motion_draft(),
        Some((session.feature, FeatureKind::OffsetFace(offset))),
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

/// Edits the offset `id`, holding the session to what it stores, and OK
/// on it straight away to writing nothing.
fn edit_and_ok(plates: &mut Plates, id: FeatureId, what: &str) {
    let Some(FeatureKind::OffsetFace(stored)) =
        (plates.doc.editor.document().feature(id)).map(|feature| feature.kind.clone())
    else {
        return;
    };
    plates.doc.look(Look::EditFeature(id));
    let Some(session) = plates.doc.motion.as_ref() else {
        return;
    };
    assert_eq!(session.feature, Some(id), "{what}");
    assert_eq!(session.offset_face(), Some(stored), "{what}");
    let revision = plates.doc.editor.revision();
    let document = plates.doc.editor.document().clone();
    plates.doc.update(Edit::CommitMotion);
    if plates.doc.motion.is_none() {
        assert_eq!(plates.doc.editor.revision(), revision, "{what}: wrote");
        assert_eq!(*plates.doc.editor.document(), document, "{what}");
    }
}

/// A join or a combine merging bodies: a disc joined across the first
/// box, or one body combined into another (the session's body into
/// another, or another into it, among them).
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
        if let Some(&body) = session.and_then(|session| session.bodies.first())
            && rng.below(2) == 0
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

/// The volume `offset` of `solid` should give, if `solid` is a box along
/// the axes and the offset can work: from its box, the sides its faces
/// move (by the plane all of their triangles' corners lie on) and the
/// signed distance.
fn wanted_volume(solid: &varde_kernel::Solid, offset: &OffsetFace) -> Option<f64> {
    let topology = solid.topology();
    let bounds = solid.bounds3()?;
    let size = bounds.max - bounds.min;
    let whole = size.x * size.y * size.z;
    if topology.regions().len() != 6 || (solid.volume() - whole).abs() > 1e-9 * whole {
        return None;
    }
    let (mut lo, mut hi) = (bounds.min, bounds.max);
    let distance = offset.signed_distance();
    for face in &offset.faces {
        let region = topology.face(solid, &face.key, face.near).ok()?;
        let tri = topology.regions()[region as usize].tris[0] as usize;
        let p = solid.mesh().patch(tri).p;
        let (axis, end) = (0..3)
            .flat_map(|axis| [(axis, 0), (axis, 1)])
            .find(|&(axis, end)| {
                let at = if end == 0 { bounds.min } else { bounds.max }[axis];
                p.iter().all(|corner| (corner[axis] - at).abs() < 1e-9)
            })?;
        if end == 0 {
            lo[axis] = bounds.min[axis] - distance;
        } else {
            hi[axis] = bounds.max[axis] + distance;
        }
    }
    let sides = hi - lo;
    (sides.min_element() > 0.0).then_some(sides.x * sides.y * sides.z)
}

/// Holds each offset the model shows working to the volume
/// [`wanted_volume`] gives.
fn check_volumes(plates: &Plates, what: &str) {
    let document = plates.doc.editor.document();
    let failed = plates.doc.feed.failed_features();
    for (at, feature) in document.features().iter().enumerate() {
        let FeatureKind::OffsetFace(offset) = &feature.kind else {
            continue;
        };
        if failed.iter().any(|failed| failed.feature == feature.id) {
            continue;
        }
        let body = offset.body().unwrap();
        let solid = |count: usize| {
            let mut cut = Editor::new(document.clone());
            while cut.document().features().len() > count {
                let last = cut.document().features().last().unwrap().id;
                cut.apply(Command::RemoveFeature(last)).unwrap();
            }
            let evaluation =
                varde_regen::evaluate(cut.document(), &mut varde_regen::Cache::default());
            (evaluation.bodies.iter())
                .find(|made| made.body == body)
                .map(|made| made.solid.clone())
        };
        let (Some(before), Some(after)) = (solid(at), solid(at + 1)) else {
            panic!("{what}: {offset:?} works on no body");
        };
        let wanted = wanted_volume(&before, offset)
            .unwrap_or_else(|| panic!("{what}: {offset:?} worked, giving {}", after.volume()));
        let got = after.volume();
        assert!(
            (got - wanted).abs() <= 1e-6 * before.volume().max(wanted),
            "{what}: {got} vs {wanted}"
        );
    }
}

/// The handle dragged to `signed` millimetres, as the viewport sends it
/// in the design's units.
fn drag_to(plates: &mut Plates, signed: f64) {
    let units = plates.doc.editor.document().units();
    let distance = varde_expr::format(signed.abs(), Some(varde_expr::Unit::Length(units)));
    plates.motion(MotionLook::OffsetBy {
        distance,
        inward: signed < 0.0,
    });
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0x3c6e_f372_fe94_f82b ^ (seed + 1).wrapping_mul(0x9e37_79b9));
    varde_regen::testing::offset_by_boxes();
    let mut plates = boxes(true);
    let base = plates.doc.editor.document().features().len();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let open = plates.doc.motion.is_some();
        let roll = match rng.below(4) {
            0 if !open => 0,
            _ => rng.below(52),
        };
        match roll {
            0 | 1 if open && rng.below(6) != 0 => plates.answer(),
            0 | 1 => plates.doc.look(Look::StartOffsetFace),
            2 => {
                let ids = offsets(&plates);
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
                // Quick clicks: several, nothing answered between them.
                for _ in 0..2 + rng.below(3) {
                    if let Some(pick) = random_pick(&plates, &mut rng) {
                        click(&mut plates, pick);
                    }
                    if rng.below(4) == 0 {
                        plates.doc.look(Look::StartOffsetFace);
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
            14..=16 => plates.input(MotionField::Distance, rng.pick(DISTANCES)),
            17..=19 => {
                // The handle dragged: a few steps, the preview answered
                // between them or not, the first face changed (another
                // clicked, or the first taken out) on the way now and
                // then.
                for _ in 0..1 + rng.below(4) {
                    drag_to(&mut plates, *rng.pick(DRAGS));
                    match rng.below(6) {
                        0 => plates.answer(),
                        1 => {
                            if let Some(pick) = random_pick(&plates, &mut rng) {
                                click(&mut plates, pick);
                            }
                        }
                        2 => {
                            let first = (plates.doc.motion.as_ref())
                                .and_then(|session| session.faces.refs.first().copied());
                            if let Some(first) = first {
                                plates.motion(MotionLook::DropFace(first));
                            }
                        }
                        _ => {}
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
                    (ready && session.kind == MotionKind::OffsetFace)
                        .then(|| (session.feature, session.offset_face()))
                });
                plates.doc.update(Edit::CommitMotion);
                if let Some((edited, Some(offset))) = before {
                    assert!(
                        plates.doc.motion.is_none(),
                        "{what}: not committed: {:?}",
                        plates.doc.edit_error
                    );
                    let id = edited.unwrap_or_else(|| plates.last_feature().0);
                    let stored = &plates.doc.editor.document().feature(id).unwrap().kind;
                    assert_eq!(*stored, FeatureKind::OffsetFace(offset), "{what}");
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
                    // The two features added: the sketch and its extrude.
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
                let ids = offsets(&plates);
                if !ids.is_empty() {
                    let id = *rng.pick(&ids);
                    edit_and_ok(&mut plates, id, &what);
                }
            }
            35 => {
                // An edit taking every face out, then its body merged.
                let ids = offsets(&plates);
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
                // A merge, undone at once.
                merge(&mut plates, &mut rng);
                if rng.below(2) == 0 {
                    plates.answer();
                }
                plates.doc.update(Edit::Undo);
            }
            37 => {
                // Offset face with a shell or a chamfer being set up.
                if rng.below(2) == 0 {
                    plates.doc.look(Look::StartShell);
                } else {
                    plates.doc.look(Look::StartChamfer);
                }
                if rng.below(2) == 0
                    && let Some(pick) = random_pick(&plates, &mut rng)
                {
                    click(&mut plates, pick);
                }
                if rng.below(2) == 0 {
                    plates.answer();
                }
                plates.doc.look(Look::StartOffsetFace);
            }
            38 | 39 => {
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

/// See the module's docs. `VARDE_OFFSET_SEEDS` runs more seeds
/// (`VARDE_OFFSET_FROM` the first).
#[test]
fn random_offset_face_sessions_hold() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let from = number("VARDE_OFFSET_FROM", 0);
    for seed in from..from.saturating_add(number("VARDE_OFFSET_SEEDS", 3)) {
        run(seed, 200);
    }
}
