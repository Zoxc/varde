//! Random sequences of what the user can do around the split session,
//! regeneration splitting by two booleans: starting and editing splits,
//! picking and dropping its body, switching between the "Split with"
//! tiles, origin planes, faces and bodies clicked (on the model shown or
//! one gone by), regions and curves of the tools sketch picked and
//! un-picked (curves it doesn't have among them), the tool's field
//! clicked to pick and clicked again, Keeps Front or Back, Keep Both,
//! Front or Back, undo and redo (across a Keep change removing and
//! adding the new body again), joins and combines merging bodies before
//! the split (the tool body into the body split among them), bodies
//! added and features removed, models answered at any point, commits and
//! cancels.
//!
//! After each step: a session that's ready is whole, passes its own
//! check and the document's for its tool, its body one the feature can
//! name and not its tool body, and is previewed as set up when it picks
//! nothing; one ready whose preview didn't fail commits, and the
//! document holds what it drafted (its new body made exactly when it
//! keeps both, an edited one's id kept, held or made); a split edited opens to what it stores and OK on it
//! straight away writes nothing; a session never outlives the split it
//! edits; the panel's texts never panic; and the document passes its
//! check. Each split the model shows working cut its body in two: its
//! pieces' volumes add up to the body's before it.

use varde_document::{BodyOp, Combine, FeatureId, Operation, OriginPlane, Targets};
use varde_view::PickIndex;

use super::*;
use crate::tests::{add_disc, two_sides};

/// A small deterministic generator: xorshift64*.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// The splits of `plates`' document.
fn splits(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::Split(_)))
        .map(|feature| feature.id)
        .collect()
}

/// A point on face `face` of `index`'s model: a corner of it, else the
/// origin.
fn on_face(index: &PickIndex, face: u32) -> DVec3 {
    (index.snaps(Picked::Face(face)).first()).map_or(DVec3::ZERO, |&(_, at)| at)
}

/// A random click on a face of the model shown, now and then on one
/// gone by.
fn random_pick(plates: &Plates, rng: &mut Rng) -> Option<varde_view::Pick> {
    let index = plates.doc.feed.pick_index();
    let count = index.picking().faces().len();
    if count == 0 {
        return None;
    }
    let face = rng.below(count) as u32;
    let target = Picked::Face(face);
    let body = index.body(target)?;
    let model = if rng.below(10) == 0 {
        index.model().wrapping_sub(1)
    } else {
        index.model()
    };
    Some(varde_view::Pick {
        model,
        target,
        body,
        at: on_face(index, face),
        snap: None,
    })
}

/// Holds the session, if it's a split's, to what the module's docs say.
fn check_session(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    if session.kind != MotionKind::Split {
        return;
    }
    let document = plates.doc.editor.document();
    if let Some(id) = session.feature {
        let kind = document.feature(id).map(|feature| &feature.kind);
        assert!(matches!(kind, Some(FeatureKind::Split(_))), "{what}");
    }
    if !plates.doc.motion_ready() {
        return;
    }
    let split = session
        .split()
        .unwrap_or_else(|| panic!("{what}: ready, not whole"));
    split
        .check_own()
        .unwrap_or_else(|why| panic!("{what}: {why}: {split:?}"));
    let features = document.features();
    let index = (session.feature)
        .and_then(|id| features.iter().position(|feature| feature.id == id))
        .unwrap_or(features.len());
    document
        .check_split_tool(index, &split.tool)
        .unwrap_or_else(|why| panic!("{what}: {why}: {split:?}"));
    assert!(
        crate::doc::combine::pickable(document, split.body, session.feature),
        "{what}: {split:?}"
    );
    if session.picking == MotionPick::Nothing {
        assert_eq!(
            plates.doc.motion_draft(),
            Some((session.feature, FeatureKind::Split(split))),
            "{what}"
        );
    }
}

/// Holds each split the model shows working to cutting its body in two:
/// the pieces' volumes, in the document regenerated to just before it
/// and just after it, add up to the body's.
fn check_volumes(plates: &Plates, what: &str) {
    let document = plates.doc.editor.document();
    let features = document.features();
    let failed = plates.doc.feed.failed_features();
    for (at, feature) in features.iter().enumerate() {
        let FeatureKind::Split(split) = &feature.kind else {
            continue;
        };
        if failed.iter().any(|failed| failed.feature == feature.id) {
            continue;
        }
        let volume = |count: usize, body: BodyId| {
            let mut cut = Editor::new(document.clone());
            while let Some(last) = cut.document().features().get(count) {
                let last = cut.document().features().last().map_or(last.id, |f| f.id);
                cut.apply(Command::RemoveFeature(last)).unwrap();
            }
            let evaluation =
                varde_regen::evaluate(cut.document(), &mut varde_regen::Cache::default());
            (evaluation.bodies.iter())
                .find(|made| made.body == body)
                .map(|made| made.solid.volume())
        };
        let Some(before) = volume(at, split.body) else {
            continue;
        };
        let kept = volume(at + 1, split.body).unwrap_or_else(|| panic!("{what}: kept piece"));
        let other = match split.made_body() {
            Some(new) => volume(at + 1, new).unwrap_or_else(|| panic!("{what}: new piece")),
            None => continue,
        };
        assert!(
            (kept + other - before).abs() <= 1e-6 * before.abs().max(1.0),
            "{what}: {kept} + {other} isn't {before}: {split:?}"
        );
    }
}

/// Edits the split `id`, holding the session to what it stores, and OK
/// on it straight away to writing nothing.
fn edit_and_ok(plates: &mut Plates, id: FeatureId, what: &str) {
    let Some(FeatureKind::Split(stored)) =
        (plates.doc.editor.document().feature(id)).map(|feature| feature.kind.clone())
    else {
        return;
    };
    plates.doc.look(Look::EditFeature(id));
    let Some(session) = plates.doc.motion.as_ref() else {
        return;
    };
    assert_eq!(session.feature, Some(id), "{what}");
    let opened = session.split().map(|split| Split {
        new_body: stored.new_body,
        ..split
    });
    assert_eq!(opened, Some(stored), "{what}");
    let revision = plates.doc.editor.revision();
    let document = plates.doc.editor.document().clone();
    plates.doc.update(Edit::CommitMotion);
    if plates.doc.motion.is_none() {
        assert_eq!(plates.doc.editor.revision(), revision, "{what}: wrote");
        assert_eq!(*plates.doc.editor.document(), document, "{what}");
    }
}

/// A join or a combine merging bodies: a disc joined across the plate
/// and the right disc, or one body combined into another (the tool body
/// of a split being set up into the body it splits among them).
fn merge(plates: &mut Plates, rng: &mut Rng) {
    if rng.below(3) == 0 {
        let extent = two_sides(plates.doc.editor.document(), "12", "1");
        let operation = Operation::Join(Targets::default());
        add_disc(&mut plates.doc.editor, (14.0, 0.0), extent, operation);
    } else {
        let document = plates.doc.editor.document();
        let session = plates.doc.motion.as_ref();
        let pair = session
            .and_then(|session| Some((*session.bodies.first()?, session.split.body?)))
            .filter(|_| rng.below(2) == 0);
        let all: Vec<BodyId> = document.bodies().iter().map(|body| body.id).collect();
        let (target, tool) = match pair {
            Some(pair) => pair,
            None if all.len() >= 2 => (*rng.pick(&all), *rng.pick(&all)),
            None => return,
        };
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

/// [`plates`] with a sketch on XY of a disc about (20, 0) and an open
/// line of two lines, (-40, 12) to (40, 12) to (40, 30), splitting by
/// two booleans: the plates and the sketch.
fn fuzz_plates() -> (Plates, FeatureId) {
    varde_regen::testing::split_by_booleans();
    let plates = plates();
    let mut editor = Editor::new(plates.doc.editor.document().clone());
    let (sketch, line) = add_tools_sketch(&mut editor);
    let FeatureKind::Sketch { sketch: drawn, .. } =
        &editor.document().feature(sketch).unwrap().kind
    else {
        unreachable!()
    };
    let mut drawn = drawn.clone();
    let Curve::Line { end, .. } = drawn.curve(line).unwrap().curve else {
        unreachable!()
    };
    let top = drawn.add_point(glam::DVec2::new(40.0, 30.0)).unwrap();
    let up = Curve::Line {
        start: end,
        end: top,
    };
    drawn.add_curve(up, false).unwrap();
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let held = held(&editor);
    (
        Plates {
            bodies: plates.bodies,
            ..held
        },
        sketch,
    )
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ (seed + 1).wrapping_mul(0x2545_f491));
    let (mut plates, sketch) = fuzz_plates();
    // The plates' features, which no undo goes back past.
    let base: Vec<FeatureId> = (plates.doc.editor.document().features()[..6].iter())
        .map(|feature| feature.id)
        .collect();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let open = plates.doc.motion.is_some();
        let roll = match rng.below(4) {
            0 if !open => 0,
            _ => rng.below(44),
        };
        match roll {
            0 | 1 if open && rng.below(6) != 0 => plates.answer(),
            0 | 1 => plates.doc.look(Look::StartSplit),
            2 => {
                let ids = splits(&plates);
                if !ids.is_empty() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                }
            }
            3 => {
                let all: Vec<BodyId> = (plates.doc.editor.document().bodies().iter())
                    .map(|body| body.id)
                    .collect();
                if !all.is_empty() {
                    let body = *rng.pick(&all);
                    if rng.below(3) == 0 {
                        plates.motion(MotionLook::Drop(body));
                    } else {
                        plates.doc.look(Look::ClickBody { body, add: false });
                    }
                }
            }
            4 => plates.motion(MotionLook::Picking(MotionPick::Bodies)),
            5..=9 => {
                if let Some(pick) = random_pick(&plates, &mut rng) {
                    plates.doc.look(Look::ClickModel {
                        pick: Some(pick),
                        add: false,
                        double: false,
                    });
                }
            }
            10 | 11 => plates.motion(MotionLook::Picking(MotionPick::Tool)),
            12 | 13 => {
                // Mostly the tools the stand-in splits by.
                let mode = *rng.pick(&[
                    SplitMode::Face,
                    SplitMode::Body,
                    SplitMode::Body,
                    SplitMode::Regions,
                    SplitMode::Regions,
                    SplitMode::Line,
                ]);
                plates.motion(MotionLook::SplitWith(mode));
            }
            14 => {
                let plane = *rng.pick(&[OriginPlane::XY, OriginPlane::XZ, OriginPlane::YZ]);
                plates.motion(MotionLook::OriginPlane(plane));
            }
            15 | 16 => {
                let region = rng.below(3);
                plates.motion(MotionLook::SplitRegion { sketch, region });
            }
            17 | 18 => {
                // Its curves, and now and then a point's id.
                let document = plates.doc.editor.document();
                let ids: Vec<Id> = match document.feature(sketch).map(|feature| &feature.kind) {
                    Some(FeatureKind::Sketch { sketch, .. }) => (sketch.curves.iter())
                        .map(|entry| entry.id)
                        .chain(sketch.points.iter().map(|entry| entry.id).take(1))
                        .collect(),
                    _ => Vec::new(),
                };
                if !ids.is_empty() {
                    let curve = *rng.pick(&ids);
                    plates.motion(MotionLook::SplitCurve { sketch, curve });
                }
            }
            19 => plates.motion(MotionLook::Original(*rng.pick(&[Side::Front, Side::Back]))),
            20 | 21 => {
                let keep = *rng.pick(&[Keep::Both, Keep::Front, Keep::Back]);
                plates.motion(MotionLook::Keep(keep));
            }
            // Not back past the plates and the sketch.
            22 | 23 if rng.below(2) == 0 => {
                plates.doc.update(Edit::Undo);
                let document = plates.doc.editor.document();
                if !base.iter().all(|&id| document.feature(id).is_some()) {
                    plates.doc.update(Edit::Redo);
                }
            }
            24 if rng.below(2) == 0 => plates.doc.update(Edit::Redo),
            25 | 26 | 31 | 32 | 33 => {
                let before = plates.doc.motion.as_ref().and_then(|session| {
                    let ready =
                        plates.doc.motion_ready() && plates.doc.feed.draft_error().is_none();
                    (ready && session.kind == MotionKind::Split)
                        .then(|| (session.feature, session.split()))
                });
                // The new body the edited split has, held or made.
                let held = (before.as_ref())
                    .and_then(|(edited, _)| *edited)
                    .and_then(|id| plates.doc.editor.document().feature(id))
                    .map(|feature| match &feature.kind {
                        FeatureKind::Split(split) => (feature.id, split.new_body),
                        _ => (feature.id, None),
                    });
                plates.doc.update(Edit::CommitMotion);
                if let Some((edited, Some(split))) = before {
                    assert!(
                        plates.doc.motion.is_none(),
                        "{what}: not committed: {:?}",
                        plates.doc.edit_error
                    );
                    let id = edited.unwrap_or_else(|| plates.last_feature().0);
                    let stored = &plates.doc.editor.document().feature(id).unwrap().kind;
                    let FeatureKind::Split(stored) = stored else {
                        panic!("{what}: {stored:?}");
                    };
                    assert_eq!(stored.made_body().is_some(), split.keeps_both(), "{what}");
                    assert_ne!(stored.new_body, Some(BodyId::NEW), "{what}");
                    // An edited split keeps the new body it had, held or made.
                    if let Some((_, Some(had))) = &held {
                        assert_eq!(stored.new_body, Some(*had), "{what}: held");
                    }
                    let set = Split {
                        new_body: stored.new_body,
                        ..split
                    };
                    assert_eq!(*stored, set, "{what}");
                }
            }
            // Add anyway, past a failed preview.
            34 => plates.doc.update(Edit::AcceptError),
            27 if rng.below(3) == 0 => plates.motion(MotionLook::Cancel),
            28 => merge(&mut plates, &mut rng),
            29 if rng.below(2) == 0 => {
                if rng.below(2) == 0 {
                    later_disc(&mut plates);
                } else {
                    // A feature after the plates removed, with what it
                    // made: the tools sketch among them.
                    let features = plates.doc.editor.document().features();
                    if features.len() > 6 {
                        let id = features[6 + rng.below(features.len() - 6)].id;
                        plates.doc.apply(Command::RemoveFeature(id));
                        plates.doc.sync();
                    }
                }
            }
            30 => {
                let ids = splits(&plates);
                if !ids.is_empty() {
                    let id = *rng.pick(&ids);
                    edit_and_ok(&mut plates, id, &what);
                }
            }
            _ => plates.answer(),
        }
        if rng.below(3) == 0 {
            plates.answer();
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
const QUICK_STEPS: usize = 20;

/// See the module's docs. A few steps of the first seed by default, all
/// 200 steps of each seed with `VARDE_TESTS=full` or `VARDE_TEST_SEED`.
#[test]
fn random_split_sessions_hold() {
    for seed in varde_testing::seeds(1, 3) {
        run(seed, crate::tests::fuzz_steps(QUICK_STEPS, 200));
    }
}
