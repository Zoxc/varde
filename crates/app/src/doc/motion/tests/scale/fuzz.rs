//! Random sequences of what the user can do around the scale session:
//! starting and editing scales, picking and dropping bodies, switching
//! between Uniform, Per axis and Edge length, picking the point and the
//! edge on whatever the model shows (faces, edges, vertices, snap dots;
//! on the model shown or one gone by), the origin, fields clicked to pick
//! into them and clicked again, Along its axis only, factors and lengths
//! typed (out of range, overflowing, not numbers among them), units
//! changed, undo and redo, joins and combines merging bodies before the
//! scale, bodies added and features removed, models answered at any
//! point (mid-pick among them), commits and cancels.
//!
//! After each step: a session that's ready is whole, passes its own
//! check and the document's for its point and edge, has its edge on one
//! of its bodies, and is previewed as set up while bodies are picked;
//! committed, the document holds what it drafted; the edge lit is the
//! edge picked (by its keys) on the body drawing it; a scale edited
//! opens to what it stores and OK on it straight away writes nothing; a
//! session never outlives the scale it edits; the panel's texts never
//! panic; and the document passes its check.

use varde_document::{Axis3, BodyOp, Combine, FeatureId, Operation, Targets};
use varde_expr::LengthUnit;
use varde_view::PickIndex;

use super::*;
use crate::tests::two_sides;

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

const FACTORS: &[&str] = &[
    "2", "0.5", "1", "1000", "0.001", "1001", "0.0009", "0", "-2", "1e308", "1e400", "1/0", "3 mm",
    "abc", "",
];
const LENGTHS: &[&str] = &[
    "120", "20", "2.5 in", "0", "-5", "1e9", "1e308", "1e-9", "0.01", "abc", "",
];

/// The scales of `plates`' document.
fn scales(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::Scale(_)))
        .map(|feature| feature.id)
        .collect()
}

/// A point on face `face` of `index`'s model: a corner of it, else a
/// point on one of its edges, else the origin.
fn on_face(index: &PickIndex, face: u32) -> DVec3 {
    if let Some(&(_, at)) = index.snaps(Picked::Face(face)).first() {
        return at;
    }
    (0..index.mesh().edge_count() as u32)
        .find(|&edge| {
            index
                .edge_faces(edge)
                .is_some_and(|faces| faces.contains(&face))
        })
        .and_then(|edge| index.chain_point(edge))
        .unwrap_or(DVec3::ZERO)
}

/// A random click on the model shown: a face, an edge, a vertex, an
/// edge's snap dot or a corner's, mostly on a body the scale scales
/// while its edge is picked, mostly on the model shown, now and then on
/// one gone by.
fn random_pick(plates: &Plates, rng: &mut Rng) -> Option<Pick> {
    let index = plates.doc.feed.pick_index();
    let picking = index.picking();
    let session = plates.doc.motion.as_ref();
    let edge = session.is_some_and(|session| session.picking == MotionPick::Edge);
    let bodies = session
        .map(|session| session.bodies.clone())
        .unwrap_or_default();
    let wanted = |body: Option<BodyId>| match body {
        None => false,
        Some(body) => !edge || bodies.is_empty() || bodies.contains(&body),
    };
    let choosy = rng.below(4) != 0;
    let of = |count: usize, target: &dyn Fn(u32) -> Picked| -> Vec<u32> {
        (0..count as u32)
            .filter(|&k| !choosy || wanted(index.body(target(k))))
            .collect()
    };
    let kind = if edge && rng.below(3) != 0 {
        1
    } else {
        rng.below(5)
    };
    let stale = rng.below(10) == 0;
    let mut one = |list: Vec<u32>| (!list.is_empty()).then(|| *rng.pick(&list));
    let edges = index.mesh().edge_count();
    let corners = picking.corners();
    let (target, at, snap) = match kind {
        0 => {
            let face = one(of(picking.faces().len(), &Picked::Face))?;
            (Picked::Face(face), on_face(index, face), None)
        }
        1 => {
            let edge = one(of(edges, &Picked::Edge))?;
            (Picked::Edge(edge), index.chain_point(edge)?, None)
        }
        2 => {
            let dots = (of(edges, &Picked::Edge).into_iter())
                .filter(|&edge| index.snap_point(Snapped::EdgePoint(edge)).is_some())
                .collect();
            let snapped = Snapped::EdgePoint(one(dots)?);
            let Snapped::EdgePoint(edge) = snapped else {
                unreachable!()
            };
            (
                Picked::Edge(edge),
                index.snap_point(snapped)?,
                Some(snapped),
            )
        }
        3 => {
            let face = |corner: u32| Picked::Face(corners[corner as usize].faces[0]);
            let corner = one(of(corners.len(), &face))?;
            let snapped = Snapped::Corner(corner);
            (face(corner), index.snap_point(snapped)?, Some(snapped))
        }
        _ => {
            let vertex = one(of(index.mesh().positions().len(), &Picked::Vertex))?;
            (Picked::Vertex(vertex), index.corner_point(vertex)?, None)
        }
    };
    let body = index.body(target)?;
    let model = if stale {
        index.model().wrapping_sub(1)
    } else {
        index.model()
    };
    Some(Pick {
        model,
        target,
        body,
        at,
        snap,
    })
}

/// Holds the session, if it's a scale's, to what the module's docs say.
fn check_session(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    if session.kind != MotionKind::Scale {
        return;
    }
    let document = plates.doc.editor.document();
    // A session edits a scale that's there.
    if let Some(id) = session.feature {
        let kind = document.feature(id).map(|feature| &feature.kind);
        assert!(matches!(kind, Some(FeatureKind::Scale(_))), "{what}");
    }
    check_lit(plates, what);
    if !plates.doc.motion_ready() {
        return;
    }
    let scale = session
        .scale()
        .unwrap_or_else(|| panic!("{what}: ready, not whole"));
    scale
        .check_own(&document.design())
        .unwrap_or_else(|why| panic!("{what}: {why}"));
    let features = document.features();
    let index = (session.feature)
        .and_then(|id| features.iter().position(|feature| feature.id == id))
        .unwrap_or(features.len());
    document
        .check_scale_refs(index, &scale)
        .unwrap_or_else(|why| panic!("{what}: {why}: {scale:?}"));
    if let Some(edge) = scale.factor.edge() {
        assert!(scale.bodies.contains(&edge.body), "{what}: {scale:?}");
    }
    // Previewed as set up while bodies are picked.
    if session.picking == MotionPick::Bodies {
        let draft = plates.doc.motion_draft();
        assert_eq!(
            draft,
            Some((session.feature, FeatureKind::Scale(scale))),
            "{what}"
        );
    }
}

/// Holds the edge lit to the one picked: by its keys, on the body
/// drawing its body.
fn check_lit(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    let lit = plates.doc.scale_lit();
    if lit.is_empty() {
        return;
    }
    let edge = session
        .scale
        .edge
        .unwrap_or_else(|| panic!("{what}: lit {lit:?} with no edge"));
    let index = plates.doc.feed.pick_index();
    let merged = plates.doc.feed.merged_bodies();
    let shown = (merged.iter())
        .find(|(consumed, _)| *consumed == edge.body)
        .map_or(edge.body, |&(_, holder)| holder);
    let picking = index.picking();
    for target in lit {
        let Picked::Edge(e) = target else {
            panic!("{what}: lit {target:?}");
        };
        let [a, b] = index.edge_faces(e).expect("an edge's faces");
        let face = |f: u32| &picking.faces()[f as usize];
        assert!(
            (face(a).named(&edge.faces[0]) && face(b).named(&edge.faces[1]))
                || (face(a).named(&edge.faces[1]) && face(b).named(&edge.faces[0])),
            "{what}: lit edge {e}, not {edge:?}"
        );
        assert_eq!(
            index.body(target),
            Some(shown),
            "{what}: lit on another body"
        );
    }
}

/// Edits the scale `id`, holding the session to what it stores, and OK
/// on it straight away to writing nothing.
fn edit_and_ok(plates: &mut Plates, id: FeatureId, what: &str) {
    let Some(FeatureKind::Scale(stored)) =
        (plates.doc.editor.document().feature(id)).map(|feature| feature.kind.clone())
    else {
        return;
    };
    plates.doc.look(Look::EditFeature(id));
    let Some(session) = plates.doc.motion.as_ref() else {
        return;
    };
    assert_eq!(session.feature, Some(id), "{what}");
    assert_eq!(session.scale(), Some(stored), "{what}");
    let revision = plates.doc.editor.revision();
    let document = plates.doc.editor.document().clone();
    plates.doc.update(Edit::CommitMotion);
    if plates.doc.motion.is_none() {
        assert_eq!(plates.doc.editor.revision(), revision, "{what}: wrote");
        assert_eq!(*plates.doc.editor.document(), document, "{what}");
    }
}

/// A join or a combine merging bodies before the scale (a new one; one
/// edited has them after it): a disc joined across the plate and the
/// right disc, or a disc combined into the plate.
fn merge(plates: &mut Plates, rng: &mut Rng) {
    let [plate, right, left] = plates.bodies;
    if rng.below(2) == 0 {
        let extent = two_sides(plates.doc.editor.document(), "12", "1");
        let operation = Operation::Join(Targets::default());
        crate::tests::add_disc(&mut plates.doc.editor, (14.0, 0.0), extent, operation);
    } else {
        let document = plates.doc.editor.document();
        let tool = *rng.pick(&[right, left]);
        if document.body(tool).is_none() || document.body(plate).is_none() {
            return;
        }
        let combine = Combine {
            target: plate,
            tools: vec![tool],
            op: BodyOp::Union,
            keep_tools: rng.below(3) == 0,
        };
        let add = document.add_feature(combine.into());
        plates.doc.apply(add);
    }
    plates.doc.sync();
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ (seed + 1).wrapping_mul(0x2545_f491));
    let mut plates = super::super::plates();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let open = plates.doc.motion.is_some();
        let roll = match rng.below(4) {
            0 if !open => 0,
            _ => rng.below(44),
        };
        match roll {
            0 | 1 if open && rng.below(6) != 0 => plates.answer(),
            0 | 1 => plates.doc.look(Look::StartScale),
            2 => {
                let ids = scales(&plates);
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
            5..=11 | 30..=35 => {
                if let Some(pick) = random_pick(&plates, &mut rng) {
                    plates.doc.look(Look::ClickModel {
                        pick: Some(pick),
                        add: false,
                        double: false,
                    });
                }
            }
            12 | 36 | 37 => plates.motion(MotionLook::Picking(MotionPick::Point)),
            38 | 39 => plates.motion(MotionLook::Picking(MotionPick::Edge)),
            13 => {
                let mode = *rng.pick(&[
                    ScaleMode::Uniform,
                    ScaleMode::PerAxis,
                    ScaleMode::EdgeLength,
                ]);
                plates.motion(MotionLook::ScaleMode(mode));
            }
            14 => plates.motion(MotionLook::AxisOnly),
            15 => plates.motion(MotionLook::OriginPoint),
            16 => plates.input(MotionField::Factor, rng.pick(FACTORS)),
            17 => {
                let axis = *rng.pick(&Axis3::ALL);
                plates.input(MotionField::AxisFactor(axis), rng.pick(FACTORS));
            }
            18 | 19 => plates.input(MotionField::Length, rng.pick(LENGTHS)),
            20 if rng.below(2) == 0 => plates.doc.update(Edit::Undo),
            21 if rng.below(2) == 0 => plates.doc.update(Edit::Redo),
            22 | 40 => {
                let before = plates.doc.motion.as_ref().and_then(|session| {
                    let ready =
                        plates.doc.motion_ready() && plates.doc.feed.draft_error().is_none();
                    (ready && session.kind == MotionKind::Scale)
                        .then(|| (session.feature, session.scale()))
                });
                plates.doc.update(Edit::CommitMotion);
                if let Some((edited, Some(scale))) = before {
                    assert!(
                        plates.doc.motion.is_none(),
                        "{what}: not committed: {:?}",
                        plates.doc.edit_error
                    );
                    let id = edited.unwrap_or_else(|| plates.last_feature().0);
                    let stored = &plates.doc.editor.document().feature(id).unwrap().kind;
                    assert_eq!(*stored, FeatureKind::Scale(scale), "{what}");
                }
            }
            23 if rng.below(3) == 0 => plates.motion(MotionLook::Cancel),
            24 => merge(&mut plates, &mut rng),
            25 if rng.below(2) == 0 => {
                if rng.below(2) == 0 {
                    super::super::later_disc(&mut plates);
                } else {
                    // A feature after the plates removed, with what it made.
                    let features = plates.doc.editor.document().features();
                    if features.len() > 6 {
                        let id = features[6 + rng.below(features.len() - 6)].id;
                        plates.doc.apply(Command::RemoveFeature(id));
                        plates.doc.sync();
                    }
                }
            }
            26 => {
                let units = *rng.pick(&LengthUnit::ALL);
                plates.doc.update(Edit::SetUnits(units));
            }
            27 => {
                let ids = scales(&plates);
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
}

/// See the module's docs. `VARDE_SCALE_SEEDS` runs more seeds
/// (`VARDE_SCALE_FROM` the first).
#[test]
fn random_scale_sessions_hold() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let from = number("VARDE_SCALE_FROM", 0);
    for seed in from..from.saturating_add(number("VARDE_SCALE_SEEDS", 3)) {
        run(seed, 200);
    }
}
