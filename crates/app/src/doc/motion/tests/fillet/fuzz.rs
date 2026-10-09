//! Random sequences of what the user can do around the fillet session,
//! regeneration filleting by arcs ([`varde_regen::testing`]): starting
//! and editing fillets (from idle, while measuring, with a combine or a
//! chamfer being set up), edges clicked (on the model shown or one gone
//! by, faces and other bodies' edges among them, the edges a preview
//! made too) to pick them or take them out, clicked one after another
//! without the models answered between them, rows' crosses, Tangent
//! chain, radii typed (out of range, overflowing, not numbers among
//! them), units changed, rows hovered (past the last among them), the
//! list of what overlaps opened on the model shown and its rows hovered,
//! ticked and chosen, held open across models answered (found again on
//! each), undo and redo,
//! joins and combines merging bodies before the fillet, bodies added and
//! features removed, commits, Add anyway and cancels.
//!
//! After each step: a session that's ready is whole, passes its own
//! check and the document's for its edges, its edges on one body that
//! the feature can name and that is the session's, and is previewed as
//! set up; one ready whose preview didn't fail commits, and the document
//! holds what it drafted; the edges lit are on the body drawing the
//! edges' body; the overlap list is of the model shown and its ticks are
//! the session's own; a fillet
//! edited opens to what it stores and OK on it straight away writes
//! nothing; a session never outlives the fillet it edits; the panel's
//! texts never panic; and the document passes its check. Each fillet the
//! model shows working takes material off its body.

use varde_document::{BodyId, BodyOp, Combine, Command, Editor, FeatureId, Operation, Targets};
use varde_expr::LengthUnit;
use varde_view::{OverlapItems, Overlaps, PanelHover, Pick};

use super::super::chamfer::fuzz::{Rng, random_pick};
use super::*;
use crate::tests::{add_disc, screen_texts, two_sides};

const RADII: &[&str] = &[
    "1",
    "2",
    "0.5",
    "3 mm",
    "4.9",
    "5",
    "0.1 in",
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

/// The fillets of `plates`' document.
fn fillets(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::Fillet(_)))
        .map(|feature| feature.id)
        .collect()
}

/// Holds the session, if it's a fillet's, to what the module's docs say.
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
    if session.kind != MotionKind::Fillet {
        return;
    }
    let document = plates.doc.editor.document();
    if let Some(id) = session.feature {
        let kind = document.feature(id).map(|feature| &feature.kind);
        assert!(matches!(kind, Some(FeatureKind::Fillet(_))), "{what}");
    }
    let (edges, faces) = (&session.blend.edges.refs, &session.faces.refs);
    assert!(
        edges.windows(2).all(|pair| pair[0].order(&pair[1]).is_lt()),
        "{what}: {edges:?}"
    );
    assert!(
        faces.windows(2).all(|pair| pair[0].order(&pair[1]).is_lt()),
        "{what}: {faces:?}"
    );
    let body = varde_document::blend_body(edges, faces);
    assert!(
        (edges.iter().map(|edge| edge.body))
            .chain(faces.iter().map(|face| face.body))
            .all(|other| Some(other) == body),
        "{what}: {edges:?} {faces:?}"
    );
    assert_eq!(
        session.bodies,
        body.into_iter().collect::<Vec<_>>(),
        "{what}"
    );
    check_lit(plates, what);
    if !plates.doc.motion_ready() {
        return;
    }
    let fillet = session
        .fillet()
        .unwrap_or_else(|| panic!("{what}: ready, not whole"));
    fillet
        .check_own(&document.design())
        .unwrap_or_else(|why| panic!("{what}: {why}: {fillet:?}"));
    let features = document.features();
    let index = (session.feature)
        .and_then(|id| features.iter().position(|feature| feature.id == id))
        .unwrap_or(features.len());
    document
        .check_blend_edges(index, &fillet.edges, &fillet.faces)
        .unwrap_or_else(|why| panic!("{what}: {why}: {fillet:?}"));
    let body = fillet.body().unwrap();
    assert!(
        crate::doc::combine::pickable(document, body, session.feature),
        "{what}: {fillet:?}"
    );
    assert_eq!(
        plates.doc.motion_draft(),
        Some((session.feature, FeatureKind::Fillet(fillet))),
        "{what}"
    );
}

/// Holds the edges lit on a model of the document as it is to the body
/// drawing the edges' body.
fn check_lit(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    let lit = plates.doc.blend_lit();
    let current = plates.doc.feed.generation() == Some(plates.doc.editor.generation());
    if lit.is_empty() || !current {
        return;
    }
    let body = varde_document::blend_body(&session.blend.edges.refs, &session.faces.refs)
        .unwrap_or_else(|| panic!("{what}: lit {lit:?} with no edges or faces"));
    let merged = plates.doc.feed.merged_bodies();
    let shown = (merged.iter())
        .find(|(consumed, _)| *consumed == body)
        .map_or(body, |&(_, holder)| holder);
    let index = plates.doc.feed.pick_index();
    for target in lit {
        assert_eq!(index.body(target), Some(shown), "{what}: lit {target:?}");
    }
}

/// Edits the fillet `id`, holding the session to what it stores, and OK
/// on it straight away to writing nothing.
fn edit_and_ok(plates: &mut Plates, id: FeatureId, what: &str) {
    let Some(FeatureKind::Fillet(stored)) =
        (plates.doc.editor.document().feature(id)).map(|feature| feature.kind.clone())
    else {
        return;
    };
    plates.doc.look(Look::EditFeature(id));
    let Some(session) = plates.doc.motion.as_ref() else {
        return;
    };
    assert_eq!(session.feature, Some(id), "{what}");
    assert_eq!(session.fillet(), Some(stored), "{what}");
    let revision = plates.doc.editor.revision();
    let document = plates.doc.editor.document().clone();
    plates.doc.update(Edit::CommitMotion);
    if plates.doc.motion.is_none() {
        assert_eq!(plates.doc.editor.revision(), revision, "{what}: wrote");
        assert_eq!(*plates.doc.editor.document(), document, "{what}");
    }
}

/// A join or a combine merging bodies: a disc joined across the plate
/// and the right disc, or one body combined into another (the
/// session's body into another, or another into it, among them).
fn merge(plates: &mut Plates, rng: &mut Rng) {
    if rng.below(3) == 0 {
        let extent = two_sides(plates.doc.editor.document(), "12", "1");
        let operation = Operation::Join(Targets::default());
        add_disc(&mut plates.doc.editor, (14.0, 0.0), extent, operation);
    } else {
        let document = plates.doc.editor.document();
        let all: Vec<BodyId> = document.bodies().iter().map(|body| body.id).collect();
        if all.len() < 2 {
            return;
        }
        let mut target = *rng.pick(&all);
        let mut tool = *rng.pick(&all);
        let session = plates.doc.motion.as_ref();
        if let Some(body) = session.and_then(|session| session.blend.edges.body())
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
        held: glam::DVec2::ZERO,
        at: glam::DVec2::ZERO,
        items: OverlapItems::Model(picks),
    })
}

/// Holds each fillet the model shows working to taking material off its
/// body: its volume in the document regenerated to just after it is
/// under its volume just before it.
fn check_volumes(plates: &Plates, what: &str) {
    let document = plates.doc.editor.document();
    let failed = plates.doc.feed.failed_features();
    for (at, feature) in document.features().iter().enumerate() {
        let FeatureKind::Fillet(fillet) = &feature.kind else {
            continue;
        };
        if failed.iter().any(|failed| failed.feature == feature.id) {
            continue;
        }
        let body = fillet.body().unwrap();
        let volume = |count: usize| {
            let mut cut = Editor::new(document.clone());
            while cut.document().features().len() > count {
                let last = cut.document().features().last().unwrap().id;
                cut.apply(Command::RemoveFeature(last)).unwrap();
            }
            let evaluation =
                varde_regen::evaluate(cut.document(), &mut varde_regen::Cache::default());
            (evaluation.bodies.iter())
                .find(|made| made.body == body)
                .map(|made| made.solid.volume())
        };
        let (Some(before), Some(after)) = (volume(at), volume(at + 1)) else {
            panic!("{what}: {fillet:?} works on no body");
        };
        assert!(after < before, "{what}: {after} not under {before}");
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0x51_7cc1_b727_220a ^ (seed + 1).wrapping_mul(0x2545_f491));
    varde_regen::testing::fillet_by_arcs();
    let mut plates = plates();
    let base = plates.doc.editor.document().features().len();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let open = plates.doc.motion.is_some();
        let roll = match rng.below(4) {
            0 if !open => 0,
            _ => rng.below(50),
        };
        match roll {
            0 | 1 if open && rng.below(6) != 0 => plates.answer(),
            0 | 1 => plates.doc.look(Look::StartFillet),
            2 => {
                let ids = fillets(&plates);
                if !ids.is_empty() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                }
            }
            3..=9 => {
                if let Some(pick) = random_pick(&plates, &mut rng) {
                    plates.doc.look(Look::ClickModel {
                        pick: Some(pick),
                        add: false,
                        double: false,
                    });
                }
            }
            10 => {
                // Quick clicks: several, nothing answered between them.
                for _ in 0..2 + rng.below(3) {
                    if let Some(pick) = random_pick(&plates, &mut rng) {
                        plates.doc.look(Look::ClickModel {
                            pick: Some(pick),
                            add: false,
                            double: false,
                        });
                    }
                    if rng.below(4) == 0 {
                        plates.doc.look(Look::StartFillet);
                    }
                }
            }
            11 => {
                let edges = (plates.doc.motion.as_ref())
                    .map(|session| session.blend.edges.refs.clone())
                    .unwrap_or_default();
                if !edges.is_empty() {
                    plates.motion(MotionLook::DropEdge(*rng.pick(&edges)));
                }
            }
            12 => plates.motion(MotionLook::Chain),
            13 => plates.motion(MotionLook::Flip),
            14..=16 => plates.input(MotionField::Radius, rng.pick(RADII)),
            17 => {
                let units = *rng.pick(&LengthUnit::ALL);
                plates.doc.update(Edit::SetUnits(units));
            }
            18 => {
                let at = rng.below(4);
                if rng.below(2) == 0 {
                    plates
                        .doc
                        .look(Look::HoverPanel(Some(PanelHover::Edge(at))));
                } else {
                    plates.doc.look(Look::LeavePanel(PanelHover::Edge(at)));
                }
            }
            19 | 20 if rng.below(2) == 0 => {
                plates.doc.update(Edit::Undo);
                if plates.doc.editor.document().features().len() < base {
                    plates.doc.update(Edit::Redo);
                }
            }
            21 if rng.below(2) == 0 => plates.doc.update(Edit::Redo),
            22..=24 => {
                let before = plates.doc.motion.as_ref().and_then(|session| {
                    let ready =
                        plates.doc.motion_ready() && plates.doc.feed.draft_error().is_none();
                    (ready && session.kind == MotionKind::Fillet)
                        .then(|| (session.feature, session.fillet()))
                });
                plates.doc.update(Edit::CommitMotion);
                if let Some((edited, Some(fillet))) = before {
                    assert!(
                        plates.doc.motion.is_none(),
                        "{what}: not committed: {:?}",
                        plates.doc.edit_error
                    );
                    let id = edited.unwrap_or_else(|| plates.last_feature().0);
                    let stored = &plates.doc.editor.document().feature(id).unwrap().kind;
                    assert_eq!(*stored, FeatureKind::Fillet(fillet), "{what}");
                }
            }
            // Add anyway, past a failed preview.
            25 => plates.doc.update(Edit::AcceptError),
            26 if rng.below(3) == 0 => plates.motion(MotionLook::Cancel),
            27 | 28 => merge(&mut plates, &mut rng),
            29 if rng.below(2) == 0 => {
                if rng.below(2) == 0 {
                    later_disc(&mut plates);
                } else {
                    // A feature after the plates removed, with what it made.
                    let features = plates.doc.editor.document().features();
                    if features.len() > base {
                        let id = features[base + rng.below(features.len() - base)].id;
                        plates.doc.apply(Command::RemoveFeature(id));
                        plates.doc.sync();
                    }
                }
            }
            30 => {
                let ids = fillets(&plates);
                if !ids.is_empty() {
                    let id = *rng.pick(&ids);
                    edit_and_ok(&mut plates, id, &what);
                }
            }
            31 => {
                // An edit taking every edge out, then its body merged.
                let ids = fillets(&plates);
                if !ids.is_empty() && plates.doc.motion.is_none() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                    let edges = (plates.doc.motion.as_ref())
                        .map(|session| session.blend.edges.refs.clone())
                        .unwrap_or_default();
                    for edge in edges {
                        plates.motion(MotionLook::DropEdge(edge));
                    }
                    if rng.below(2) == 0 {
                        plates.answer();
                    }
                    merge(&mut plates, &mut rng);
                }
            }
            32 | 33 => {
                // `F` (or the rail's Fillet) while measuring, or with a
                // combine or a chamfer being set up.
                match rng.below(3) {
                    0 => plates.doc.look(Look::StartMeasure),
                    1 => plates.doc.look(Look::StartCombine),
                    _ => plates.doc.look(Look::StartChamfer),
                }
                if rng.below(2) == 0
                    && let Some(pick) = random_pick(&plates, &mut rng)
                {
                    plates.doc.look(Look::ClickModel {
                        pick: Some(pick),
                        add: false,
                        double: false,
                    });
                }
                if rng.below(2) == 0 {
                    plates.answer();
                }
                key_in(&mut plates.doc, character("f"));
            }
            34 | 35 => {
                if let Some(list) = random_overlaps(&plates, &mut rng) {
                    plates.doc.look(Look::OpenOverlaps(list));
                }
            }
            36..=38 => {
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

/// The steps of the quick run, see [`crate::tests::fuzz_steps`].
const QUICK_STEPS: usize = 22;

/// See the module's docs. A few steps of the first seed by default, all
/// 200 steps of each seed with `VARDE_TESTS=full` or `VARDE_TEST_SEED`.
#[test]
fn random_fillet_sessions_hold() {
    for seed in varde_testing::seeds(1, 3) {
        run(seed, crate::tests::fuzz_steps(QUICK_STEPS, 200));
    }
}
