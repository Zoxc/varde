//! Random sequences of what the user can do around the pattern sessions:
//! starting and editing linear and circular patterns, their modes, Flip,
//! Join to original, counts and spreads (long and deeply nested texts among them), units
//! changed, undo and redo while editing, the pattern's kind swapped by
//! another edit, the direction picked, copy bodies picked, named by
//! later combines and mirrors, hidden and deleted, commits and cancels.
//! After each step: a session that's ready drafts and commits the
//! values its fields come to (worked out here), one held back by a later
//! feature naming a copy body it'd drop is neither ready nor previewed,
//! a pattern edited opens to the values it stores and OK on it straight
//! away writes nothing, a session never outlives the pattern it edits or
//! its kind, and the document passes its check.

use std::f64::consts::TAU;

use varde_document::{BodyId, Document};
use varde_expr::LengthUnit;

use super::*;

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
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const COUNTS: &[&str] = &[
    "2", "3", "4", "1 + 2", "5", "0", "1", "2.5", "1025", "abc", "",
];

/// Spreads: plain, with units, refused, past the limits, and long or
/// deeply nested texts at what an expression may hold.
fn spreads() -> Vec<String> {
    let mut texts: Vec<String> = [
        "15",
        "45",
        "0.5",
        "90",
        "120",
        "360",
        "72",
        "10 in",
        "1 rad",
        "-3",
        "0",
        "999999",
        "1e6",
        "abc",
        "180 + 180",
    ]
    .iter()
    .map(|text| (*text).to_owned())
    .collect();
    texts.push(format!("45{}", " + 0".repeat(63)));
    texts.push(format!("30{}", " + 0".repeat(63)));
    let deep = varde_expr::MAX_DEPTH - 1;
    texts.push(format!("{}20{}", "(".repeat(deep), ")".repeat(deep)));
    texts.push(format!(
        "{}20{}",
        "(".repeat(deep + 1),
        ")".repeat(deep + 1)
    ));
    texts
}

/// The patterns of `plates`' document, by index.
fn patterns(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::Pattern(_)))
        .map(|feature| feature.id)
        .collect()
}

/// The copy bodies of `plates`' document's patterns.
fn copy_bodies(plates: &Plates) -> Vec<BodyId> {
    (plates.doc.editor.document().features().iter())
        .filter_map(|feature| match &feature.kind {
            FeatureKind::Pattern(pattern) => Some(pattern.copy_bodies()),
            _ => None,
        })
        .flatten()
        .map(|(_, _, body)| body)
        .collect()
}

/// A linear pattern's spacing, or a circular one's angle.
fn spread_of(kind: &PatternKind) -> &Value {
    match kind {
        PatternKind::Linear { spacing, .. } => spacing,
        PatternKind::Circular { angle, .. } => angle,
    }
}

/// The values a pattern holds: its bodies, axis, count and spread.
fn values(pattern: &Pattern) -> (Vec<BodyId>, AxisRef, f64, f64) {
    (
        pattern.bodies.clone(),
        *pattern.kind.axis(),
        pattern.kind.count_value().value,
        spread_of(&pattern.kind).value,
    )
}

/// Whether `a` is `b` to a few roundings: the text stored, "(100) / 3"
/// in feet say, is worked out in its own order (the unit last), which
/// can come to an ulp off the spread divided here.
fn agrees(a: f64, b: f64) -> bool {
    (a - b).abs() <= 4.0 * f64::EPSILON * b.abs()
}

/// Holds a session that's ready to what its fields come to, worked out
/// here: the count as typed; a linear pattern's spacing the spread (a
/// Total's divided by the count less one), turned by Flip; a circular
/// one's angle a whole turn for Full 360°, the Total as typed, or a
/// Spacing's times the count less one ([`agrees`]).
fn check_ready(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    if !session.kind.pattern() || !plates.doc.motion_ready() {
        return;
    }
    let Some(FeatureKind::Pattern(pattern)) = session.kind() else {
        panic!("{what}: ready, but no pattern");
    };
    let design = plates.doc.editor.document().design();
    pattern
        .check_own(&design)
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    let count = session.fields[MotionField::Count.index()]
        .value
        .as_ref()
        .unwrap()
        .value;
    assert_eq!(pattern.kind.count_value().value, count, "{what}");
    let steps = count - 1.0;
    let spread = session.fields[MotionField::Spread.index()]
        .value
        .as_ref()
        .map(|value| value.value);
    match (&pattern.kind, session.mode) {
        (PatternKind::Linear { spacing, .. }, mode) => {
            let spread = spread.unwrap();
            let step = if mode == PatternMode::Total {
                spread / steps
            } else {
                spread
            };
            let step = if session.flip { -step } else { step };
            assert!(agrees(spacing.value, step), "{what}: {spacing:?} vs {step}");
        }
        (PatternKind::Circular { .. }, PatternMode::Full) => {
            assert!(pattern.full_turn(), "{what}");
            assert!((spread_of(&pattern.kind).value - TAU).abs() < 1e-12);
        }
        (PatternKind::Circular { angle, .. }, PatternMode::Total) => {
            assert_eq!(angle.value, spread.unwrap(), "{what}");
            assert!(!pattern.full_turn(), "{what}");
        }
        (PatternKind::Circular { angle, .. }, PatternMode::Spacing) => {
            let angle_wanted = spread.unwrap() * steps;
            assert!(agrees(angle.value, angle_wanted), "{what}: {angle:?}");
            assert!(!pattern.full_turn(), "{what}");
        }
    }
    // The draft previews it (while an axis is picked, the bodies stay
    // where they are, or for a new pattern nothing is previewed).
    if plates.doc.editable() && session.picking == MotionPick::Bodies {
        let draft = plates.doc.motion_draft().map(|(_, kind)| kind);
        assert_eq!(draft, Some(FeatureKind::Pattern(pattern)), "{what}");
    }
}

/// Edits the pattern `id`, holding the session to the values it stores,
/// and OK on it straight away to writing nothing.
fn edit_and_ok(plates: &mut Plates, id: FeatureId, what: &str) {
    let Some(FeatureKind::Pattern(stored)) =
        (plates.doc.editor.document().feature(id)).map(|feature| feature.kind.clone())
    else {
        return;
    };
    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("editing it");
    assert_eq!(session.feature, Some(id), "{what}");
    let opened = session.pattern();
    let Ok(Some(opened)) = opened else {
        panic!("{what}: {stored:?} opened as {opened:?}");
    };
    assert_eq!(values(&opened), values(&stored), "{what}");
    assert_eq!(opened.full_turn(), stored.full_turn(), "{what}");
    let revision = plates.doc.editor.revision();
    let document = plates.doc.editor.document().clone();
    plates.doc.update(Edit::CommitMotion);
    if plates.doc.motion.is_none() {
        assert_eq!(
            plates.doc.editor.revision(),
            revision,
            "{what}: OK with nothing changed wrote {:?} over {stored:?}",
            plates.doc.editor.document().feature(id).map(|f| &f.kind)
        );
        assert_eq!(*plates.doc.editor.document(), document, "{what}");
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ (seed + 1).wrapping_mul(0x2545_f491));
    let mut plates = plates();
    let bodies = plates.bodies;
    let spreads = spreads();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        // What the session edits and its kind, before the step.
        match rng.below(25) {
            0 => plates.doc.look(Look::StartPattern),
            1 => plates.doc.look(Look::StartCircularPattern),
            2 | 3 => {
                // The plates, or any body there now, copy bodies among
                // them.
                let all: Vec<BodyId> = (plates.doc.editor.document().bodies().iter())
                    .map(|body| body.id)
                    .collect();
                let body = if rng.below(2) == 0 || all.is_empty() {
                    *rng.pick(&bodies)
                } else {
                    *rng.pick(&all)
                };
                plates.doc.look(Look::ClickBody {
                    body,
                    add: rng.below(2) == 0,
                });
            }
            4 | 5 => {
                let text = rng.pick(COUNTS).to_string();
                plates.input(MotionField::Count, &text);
            }
            6 | 7 => {
                let text = rng.pick(&spreads).clone();
                plates.input(MotionField::Spread, &text);
            }
            8 if rng.below(2) == 0 => plates.motion(MotionLook::Flip),
            8 => plates.motion(MotionLook::Join),
            9 => {
                let mode =
                    *rng.pick(&[PatternMode::Spacing, PatternMode::Total, PatternMode::Full]);
                plates.motion(MotionLook::Mode(mode));
            }
            10 => {
                let units = *rng.pick(&LengthUnit::ALL);
                plates.doc.update(Edit::SetUnits(units));
            }
            11 => plates.doc.update(Edit::Undo),
            12 => plates.doc.update(Edit::Redo),
            13 | 14 => {
                let before = plates.doc.motion.as_ref().and_then(|session| {
                    // OK waits on a preview that failed.
                    let ready =
                        plates.doc.motion_ready() && plates.doc.feed.draft_error().is_none();
                    (ready && session.kind.pattern()).then(|| (session.feature, session.kind()))
                });
                plates.doc.update(Edit::CommitMotion);
                if let Some((edited, Some(kind))) = before {
                    // Committed: the document holds what it drafted.
                    assert!(
                        plates.doc.motion.is_none(),
                        "{what}: not committed: {:?} {:?} {:?}",
                        plates.doc.feed.draft_error(),
                        plates.doc.edit_error,
                        plates.doc.motion.as_ref().map(|s| (s.feature, s.kind()))
                    );
                    let id = edited.unwrap_or_else(|| plates.last_feature().0);
                    let mut stored = plates
                        .doc
                        .editor
                        .document()
                        .feature(id)
                        .unwrap()
                        .kind
                        .clone();
                    // Unjoined, the document laid the copy bodies out.
                    if let FeatureKind::Pattern(pattern) = &mut stored
                        && !pattern.joins()
                    {
                        let made = pattern.copy_bodies().count();
                        assert_eq!(Some(made), pattern.separate_count(), "{what}");
                        pattern.copies = varde_document::Copies::Separate(Vec::new());
                    }
                    assert_eq!(stored, kind, "{what}");
                }
            }
            15 => plates.motion(MotionLook::Cancel),
            16 => {
                let ids = patterns(&plates);
                if !ids.is_empty() {
                    let id = *rng.pick(&ids);
                    if rng.below(3) == 0 {
                        plates.doc.pattern_shapes.clear();
                    }
                    edit_and_ok(&mut plates, id, &what);
                }
            }
            17 => {
                let ids = patterns(&plates);
                if !ids.is_empty() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                }
            }
            18 => {
                // Another edit swaps a pattern's kind, as a file could.
                let ids = patterns(&plates);
                if !ids.is_empty() {
                    let id = *rng.pick(&ids);
                    let document = plates.doc.editor.document();
                    let design = document.design();
                    let FeatureKind::Pattern(mut pattern) =
                        document.feature(id).unwrap().kind.clone()
                    else {
                        unreachable!()
                    };
                    let count = pattern.kind.count_value().clone();
                    pattern.kind = match pattern.kind {
                        PatternKind::Linear { along, .. } => PatternKind::Circular {
                            about: along,
                            count,
                            angle: Value::new("90", &Pattern::angle_ask(&design)).unwrap(),
                        },
                        PatternKind::Circular { about, .. } => PatternKind::Linear {
                            along: about,
                            count,
                            spacing: Value::new("-12", &Pattern::spacing_ask(&design)).unwrap(),
                        },
                    };
                    plates.doc.apply(Command::SetFeature {
                        feature: id,
                        kind: Box::new(FeatureKind::Pattern(pattern)),
                    });
                    plates.doc.sync();
                }
            }
            20 | 21 => {
                // Another edit names a copy body: a combine of it into the
                // plate, or a mirror of it.
                let copies = copy_bodies(&plates);
                if !copies.is_empty() {
                    let body = *rng.pick(&copies);
                    let kind: FeatureKind = if rng.below(2) == 0 {
                        varde_document::Combine {
                            target: bodies[0],
                            tools: vec![body],
                            op: varde_document::BodyOp::Union,
                            keep_tools: rng.below(2) == 0,
                        }
                        .into()
                    } else {
                        varde_document::Mirror {
                            bodies: vec![body],
                            plane: varde_document::PlaneRef::Origin(
                                varde_document::OriginPlane::YZ,
                            ),
                            keep_original: rng.below(2) == 0,
                        }
                        .into()
                    };
                    let add = plates.doc.editor.document().add_feature(kind);
                    plates.doc.apply(add);
                    plates.doc.sync();
                }
            }
            22 => {
                // A copy body removed (its pattern with it), or hidden or
                // shown.
                let copies = copy_bodies(&plates);
                if !copies.is_empty() {
                    let body = *rng.pick(&copies);
                    let document = plates.doc.editor.document();
                    let maker = document.body(body).unwrap().created_by;
                    if rng.below(2) == 0 {
                        plates.doc.apply(Command::RemoveBody(body));
                        let document = plates.doc.editor.document();
                        assert!(document.feature(maker).is_none(), "{what}");
                        assert!(document.body(body).is_none(), "{what}");
                    } else {
                        let visible = document.body(body).unwrap().visible;
                        plates.doc.apply(Command::SetVisible(body, !visible));
                    }
                    plates.doc.sync();
                }
            }
            23 => plates.motion(MotionLook::Picking(MotionPick::Reference)),
            _ => plates.answer(),
        }
        // An edit held back by a later feature naming a copy body it'd
        // drop isn't ready, and isn't previewed.
        if plates.doc.motion_held().is_some() {
            assert!(!plates.doc.motion_ready(), "{what}");
            assert_eq!(plates.doc.motion_draft(), None, "{what}");
        }
        // A session edits a pattern of its own kind that's there.
        if let Some(session) = &plates.doc.motion
            && let Some(id) = session.feature
        {
            let feature = plates.doc.editor.document().feature(id);
            let kind = feature.and_then(|feature| MotionKind::of(&feature.kind));
            assert_eq!(kind, Some(session.kind), "{what}");
        }
        check_ready(&plates, &what);
        // What it shows never panics.
        let _ = screen_texts(&plates.doc);
        plates
            .doc
            .editor
            .document()
            .check()
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        let _ = Document::clone(plates.doc.editor.document());
    }
}

/// The steps of the quick run, see [`crate::tests::fuzz_steps`].
const QUICK_STEPS: usize = 24;

/// See the module's docs. A few steps of the first seed by default, all
/// 80 steps of each seed with `VARDE_TESTS=full` or `VARDE_TEST_SEED`.
#[test]
fn random_pattern_sessions_hold() {
    for seed in varde_testing::seeds(1, 3) {
        run(seed, crate::tests::fuzz_steps(QUICK_STEPS, 80));
    }
}
