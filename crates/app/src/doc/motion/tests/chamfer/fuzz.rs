//! Random sequences of what the user can do around the chamfer session,
//! regeneration chamfering by prisms ([`varde_regen::testing`]):
//! starting and editing chamfers, edges clicked (on the model shown or
//! one gone by, faces and other bodies' edges among them, the edges a
//! preview made too) to pick them or take them out, rows' crosses,
//! Tangent chain, the types, Flip sides, distances and angles typed (out
//! of range, overflowing, not numbers among them), units changed, rows
//! hovered (past the last among them), undo and redo, joins and
//! combines merging bodies before the chamfer, bodies added and
//! features removed, models answered at any point, commits, Add anyway
//! and cancels.
//!
//! After each step: a session that's ready is whole, passes its own
//! check and the document's for its edges, its edges on one body that
//! the feature can name and that is the session's, and is previewed as
//! set up; one ready whose preview didn't fail commits, and the document
//! holds what it drafted; the edges lit are on the body drawing the
//! edges' body; a chamfer edited opens to what it stores and OK on it
//! straight away writes nothing; a session never outlives the chamfer
//! it edits; the panel's texts never panic; and the document passes its
//! check. Each chamfer the model shows working takes material off its
//! body.

use varde_document::{BodyOp, Combine, Command, Operation, Targets};
use varde_expr::LengthUnit;
use varde_view::{MotionField, PanelHover};

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

const DISTANCES: &[&str] = &[
    "1",
    "2",
    "0.5",
    "3 mm",
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
const ANGLES: &[&str] = &[
    "45", "30°", "60", "89.999", "90", "0", "-10", "1e308", "0.5 rad", "2 rad", "abc", "", "3 mm",
];

/// The chamfers of `plates`' document.
fn chamfers(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::Chamfer(_)))
        .map(|feature| feature.id)
        .collect()
}

/// A random click on the model shown: mostly an edge (of the session's
/// body, while it has one), now and then a face; mostly on the model
/// shown, now and then on one gone by.
fn random_pick(plates: &Plates, rng: &mut Rng) -> Option<Pick> {
    let index = plates.doc.feed.pick_index();
    let body = plates
        .doc
        .motion
        .as_ref()
        .and_then(|session| session.bodies.first().copied());
    let choosy = rng.below(3) != 0;
    let model = if rng.below(10) == 0 {
        index.model().wrapping_sub(1)
    } else {
        index.model()
    };
    if rng.below(8) == 0 {
        let count = index.picking().faces().len();
        if count == 0 {
            return None;
        }
        let face = rng.below(count) as u32;
        let target = Picked::Face(face);
        let at = (index.snaps(target).first()).map_or(DVec3::ZERO, |&(_, at)| at);
        return Some(Pick {
            model,
            target,
            body: index.body(target)?,
            at,
            snap: None,
        });
    }
    let edges: Vec<u32> = (0..index.mesh().edge_count() as u32)
        .filter(|&edge| !choosy || body.is_none() || index.body(Picked::Edge(edge)) == body)
        .collect();
    if edges.is_empty() {
        return None;
    }
    let edge = *rng.pick(&edges);
    let target = Picked::Edge(edge);
    Some(Pick {
        model,
        target,
        body: index.body(target)?,
        at: index.chain_point(edge)?,
        snap: None,
    })
}

/// Holds the session, if it's a chamfer's, to what the module's docs
/// say.
fn check_session(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    if session.kind != MotionKind::Chamfer {
        return;
    }
    let document = plates.doc.editor.document();
    if let Some(id) = session.feature {
        let kind = document.feature(id).map(|feature| &feature.kind);
        assert!(matches!(kind, Some(FeatureKind::Chamfer(_))), "{what}");
    }
    let edges = &session.blend.edges;
    assert!(
        edges.windows(2).all(|pair| pair[0].order(&pair[1]).is_lt()),
        "{what}: {edges:?}"
    );
    assert!(
        edges.iter().all(|edge| edge.body == edges[0].body),
        "{what}: {edges:?}"
    );
    assert_eq!(
        session.bodies,
        edges
            .first()
            .map(|edge| edge.body)
            .into_iter()
            .collect::<Vec<_>>(),
        "{what}"
    );
    check_lit(plates, what);
    if !plates.doc.motion_ready() {
        return;
    }
    let chamfer = session
        .chamfer()
        .unwrap_or_else(|| panic!("{what}: ready, not whole"));
    chamfer
        .check_own(&document.design())
        .unwrap_or_else(|why| panic!("{what}: {why}: {chamfer:?}"));
    let features = document.features();
    let index = (session.feature)
        .and_then(|id| features.iter().position(|feature| feature.id == id))
        .unwrap_or(features.len());
    document
        .check_chamfer_edges(index, &chamfer.edges)
        .unwrap_or_else(|why| panic!("{what}: {why}: {chamfer:?}"));
    let body = chamfer.body().unwrap();
    assert!(
        crate::doc::combine::pickable(document, body, session.feature),
        "{what}: {chamfer:?}"
    );
    assert_eq!(
        plates.doc.motion_draft(),
        Some((session.feature, FeatureKind::Chamfer(chamfer))),
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
    // A model of the document before a merge lights them where they
    // were.
    let current = plates.doc.feed.generation() == Some(plates.doc.editor.generation());
    if lit.is_empty() || !current {
        return;
    }
    let body = session
        .blend
        .body()
        .unwrap_or_else(|| panic!("{what}: lit {lit:?} with no edges"));
    let merged = plates.doc.feed.merged_bodies();
    let shown = (merged.iter())
        .find(|(consumed, _)| *consumed == body)
        .map_or(body, |&(_, holder)| holder);
    let index = plates.doc.feed.pick_index();
    for target in lit {
        assert_eq!(index.body(target), Some(shown), "{what}: lit {target:?}");
    }
}

/// Edits the chamfer `id`, holding the session to what it stores, and
/// OK on it straight away to writing nothing.
fn edit_and_ok(plates: &mut Plates, id: FeatureId, what: &str) {
    let Some(FeatureKind::Chamfer(stored)) =
        (plates.doc.editor.document().feature(id)).map(|feature| feature.kind.clone())
    else {
        return;
    };
    plates.doc.look(Look::EditFeature(id));
    let Some(session) = plates.doc.motion.as_ref() else {
        return;
    };
    assert_eq!(session.feature, Some(id), "{what}");
    assert_eq!(session.chamfer(), Some(stored), "{what}");
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
        if let Some(body) = session.and_then(|session| session.blend.body())
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

/// Holds each chamfer the model shows working to taking material off
/// its body: its volume in the document regenerated to just after it
/// is under its volume just before it.
fn check_volumes(plates: &Plates, what: &str) {
    let document = plates.doc.editor.document();
    let failed = plates.doc.feed.failed_features();
    for (at, feature) in document.features().iter().enumerate() {
        let FeatureKind::Chamfer(chamfer) = &feature.kind else {
            continue;
        };
        if failed.iter().any(|failed| failed.feature == feature.id) {
            continue;
        }
        let body = chamfer.body().unwrap();
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
            panic!("{what}: {chamfer:?} works on no body");
        };
        assert!(after < before, "{what}: {after} not under {before}");
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ (seed + 1).wrapping_mul(0x2545_f491));
    varde_regen::testing::chamfer_by_wedges();
    let mut plates = plates();
    let base = plates.doc.editor.document().features().len();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let open = plates.doc.motion.is_some();
        let roll = match rng.below(4) {
            0 if !open => 0,
            _ => rng.below(46),
        };
        match roll {
            0 | 1 if open && rng.below(6) != 0 => plates.answer(),
            0 | 1 => plates.doc.look(Look::StartChamfer),
            2 => {
                let ids = chamfers(&plates);
                if !ids.is_empty() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                }
            }
            3..=11 => {
                if let Some(pick) = random_pick(&plates, &mut rng) {
                    plates.doc.look(Look::ClickModel {
                        pick: Some(pick),
                        add: false,
                        double: false,
                    });
                }
            }
            12 => {
                let edges = (plates.doc.motion.as_ref())
                    .map(|session| session.blend.edges.clone())
                    .unwrap_or_default();
                if !edges.is_empty() {
                    plates.motion(MotionLook::DropEdge(*rng.pick(&edges)));
                }
            }
            13 => plates.motion(MotionLook::Chain),
            14 => {
                let kind = *rng.pick(&[ChamferType::Equal, ChamferType::Two, ChamferType::Angle]);
                plates.motion(MotionLook::ChamferType(kind));
            }
            15 => plates.motion(MotionLook::Flip),
            16 | 17 => plates.input(MotionField::ChamferDistance, rng.pick(DISTANCES)),
            18 => plates.input(MotionField::ChamferSecond, rng.pick(DISTANCES)),
            19 => plates.input(MotionField::ChamferAngle, rng.pick(ANGLES)),
            20 => {
                let units = *rng.pick(&LengthUnit::ALL);
                plates.doc.update(Edit::SetUnits(units));
            }
            21 => {
                let at = rng.below(4);
                if rng.below(2) == 0 {
                    plates
                        .doc
                        .look(Look::HoverPanel(Some(PanelHover::Edge(at))));
                } else {
                    plates.doc.look(Look::LeavePanel(PanelHover::Edge(at)));
                }
            }
            22 | 23 if rng.below(2) == 0 => {
                plates.doc.update(Edit::Undo);
                if plates.doc.editor.document().features().len() < base {
                    plates.doc.update(Edit::Redo);
                }
            }
            24 if rng.below(2) == 0 => plates.doc.update(Edit::Redo),
            25..=27 => {
                let before = plates.doc.motion.as_ref().and_then(|session| {
                    let ready =
                        plates.doc.motion_ready() && plates.doc.feed.draft_error().is_none();
                    (ready && session.kind == MotionKind::Chamfer)
                        .then(|| (session.feature, session.chamfer()))
                });
                plates.doc.update(Edit::CommitMotion);
                if let Some((edited, Some(chamfer))) = before {
                    assert!(
                        plates.doc.motion.is_none(),
                        "{what}: not committed: {:?}",
                        plates.doc.edit_error
                    );
                    let id = edited.unwrap_or_else(|| plates.last_feature().0);
                    let stored = &plates.doc.editor.document().feature(id).unwrap().kind;
                    assert_eq!(*stored, FeatureKind::Chamfer(chamfer), "{what}");
                }
            }
            // Add anyway, past a failed preview.
            28 => plates.doc.update(Edit::AcceptError),
            29 if rng.below(3) == 0 => plates.motion(MotionLook::Cancel),
            30 | 31 => merge(&mut plates, &mut rng),
            32 if rng.below(2) == 0 => {
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
            33 => {
                let ids = chamfers(&plates);
                if !ids.is_empty() {
                    let id = *rng.pick(&ids);
                    edit_and_ok(&mut plates, id, &what);
                }
            }
            34 => {
                // An edit taking every edge out, then its body merged.
                let ids = chamfers(&plates);
                if !ids.is_empty() && plates.doc.motion.is_none() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                    let edges = (plates.doc.motion.as_ref())
                        .map(|session| session.blend.edges.clone())
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

/// See the module's docs. `VARDE_CHAMFER_SEEDS` runs more seeds
/// (`VARDE_CHAMFER_FROM` the first).
#[test]
fn random_chamfer_sessions_hold() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let from = number("VARDE_CHAMFER_FROM", 0);
    for seed in from..from.saturating_add(number("VARDE_CHAMFER_SEEDS", 3)) {
        run(seed, 200);
    }
}
