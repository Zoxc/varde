//! Random sequences of what the user can do around the align session:
//! starting and editing aligns, picking the body (another one switching
//! it), picking points and directions on whatever the model shows (faces,
//! edges, vertices, snap dots; on the model shown or one gone by), the
//! origin and its axes, fields clicked to pick into them and clicked
//! again, references taken out, Flip, distances and angles typed, units
//! changed, undo and redo, joins and combines merging bodies before the
//! align, bodies added and features removed, models answered at any
//! point (mid-pick among them), commits and cancels.
//!
//! After each step: a session that's ready is whole, passes its own
//! check and the document's for each side, has the moved side's
//! references on its body and the target's off it, and is previewed as
//! set up while nothing is picked; committed, the document holds what
//! it drafted; what's lit names what's picked (a face by its key, an
//! edge by its keys) on the body drawing it; an align edited opens to
//! what it stores and OK on it straight away writes nothing; a session
//! never outlives the align it edits; the panel's texts never panic; and
//! the document passes its check.

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

const DISTANCES: &[&str] = &["5", "-10", "0", "2.5 in", "1e9", "abc", ""];
const ANGLES: &[&str] = &["90", "-45", "0", "30", "720", "1 rad", "x"];

/// The aligns of `plates`' document.
fn aligns(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::Align(_)))
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
/// edge's snap dot or a corner's, mostly on the body the slot being
/// picked wants (the moved one, or another for the target), mostly on
/// the model shown, now and then on one gone by.
fn random_pick(plates: &Plates, rng: &mut Rng) -> Option<Pick> {
    let index = plates.doc.feed.pick_index();
    let picking = index.picking();
    let session = plates.doc.motion.as_ref();
    let moved = session.and_then(|session| session.bodies.first().copied());
    let side = match session.map(|session| session.picking) {
        Some(MotionPick::Align(slot)) => Some(slot.side),
        _ => None,
    };
    let wanted = |body: Option<BodyId>| match (side, moved, body) {
        (_, _, None) => false,
        (Some(AlignSide::Moved), Some(moved), Some(body)) => body == moved,
        (Some(AlignSide::Target), Some(moved), Some(body)) => body != moved,
        _ => true,
    };
    let choosy = rng.below(4) != 0;
    let of = |count: usize, target: &dyn Fn(u32) -> Picked| -> Vec<u32> {
        (0..count as u32)
            .filter(|&k| !choosy || wanted(index.body(target(k))))
            .collect()
    };
    let (kind, stale) = (rng.below(5), rng.below(10) == 0);
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

/// A random one of an align's six slots.
fn random_slot(rng: &mut Rng) -> AlignSlot {
    let side = *rng.pick(&[AlignSide::Moved, AlignSide::Target]);
    AlignSlot::new(side, *rng.pick(&AlignRole::ALL))
}

/// Holds the session, if it's an align's, to what the module's docs say.
fn check_session(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    if session.kind != MotionKind::Align {
        return;
    }
    let document = plates.doc.editor.document();
    // A session edits an align that's there.
    if let Some(id) = session.feature {
        let kind = document.feature(id).map(|feature| &feature.kind);
        assert!(matches!(kind, Some(FeatureKind::Align(_))), "{what}");
    }
    // What's lit names what's picked, on the body drawing it.
    check_lit(plates, what);
    // What the status bar asks for next is what clicks go on to.
    if let Some(need) = session.align.need() {
        use AlignRole::{Point, Primary, Secondary};
        use AlignSide::{Moved, Target};
        let wanted: &[(AlignSide, AlignRole)] = match need {
            "pick the point to align it to" => &[(Target, Point)],
            "pick the direction to align it to" => &[(Target, Primary)],
            "pick a direction on the body" => &[(Moved, Primary)],
            "pick the second direction to align it to" => &[(Target, Secondary)],
            "pick a second direction on the body" => &[(Moved, Secondary)],
            "pick a direction on each side before a second one" => {
                &[(Moved, Primary), (Target, Primary)]
            }
            _ => &[(Moved, Point)],
        };
        let next = session.align.next();
        assert!(
            (wanted.iter())
                .any(|&(side, role)| next == MotionPick::Align(AlignSlot::new(side, role))),
            "{what}: asks to {need}, clicks go to {next:?}"
        );
    }
    if !plates.doc.motion_ready() {
        return;
    }
    let align = session
        .align()
        .unwrap_or_else(|| panic!("{what}: ready, not whole"));
    align
        .check_own(&document.design())
        .unwrap_or_else(|why| panic!("{what}: {why}"));
    let features = document.features();
    let index = (session.feature)
        .and_then(|id| features.iter().position(|feature| feature.id == id))
        .unwrap_or(features.len());
    for refs in [&align.from, &align.to] {
        document
            .check_align_refs(index, refs)
            .unwrap_or_else(|why| panic!("{what}: {why}: {refs:?}"));
    }
    let bodies = |refs: &AlignRefs| -> Vec<Option<BodyId>> {
        std::iter::once(refs.point.body())
            .chain(refs.directions().map(DirRef::body))
            .collect()
    };
    for body in bodies(&align.from) {
        assert_eq!(body, Some(align.body), "{what}: {align:?}");
    }
    for body in bodies(&align.to).into_iter().flatten() {
        assert_ne!(body, align.body, "{what}: {align:?}");
    }
    // Previewed as set up while nothing is picked.
    if matches!(session.picking, MotionPick::Nothing | MotionPick::Bodies) {
        let draft = plates.doc.motion_draft();
        assert_eq!(
            draft,
            Some((session.feature, FeatureKind::from(align))),
            "{what}"
        );
    }
}

/// Holds what's lit of the align being set up to naming what's picked:
/// a face by its key, an edge by its keys, each on the body drawing the
/// reference's body.
fn check_lit(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    let index = plates.doc.feed.pick_index();
    let merged = plates.doc.feed.merged_bodies();
    let shown = |body: BodyId| {
        (merged.iter())
            .find(|(consumed, _)| *consumed == body)
            .map_or(body, |&(_, holder)| holder)
    };
    let model = plates.doc.feed.model();
    let picking = index.picking();
    for (side, marks) in session.align.marks.iter().enumerate() {
        for (role, mark) in marks.iter().enumerate().skip(1) {
            let Some(mark) = mark.filter(|mark| mark.model == model) else {
                continue;
            };
            let Some(target) = mark.target else {
                continue;
            };
            let slot = AlignSlot::new(
                [AlignSide::Moved, AlignSide::Target][side],
                AlignRole::ALL[role],
            );
            let taken = session.align.taken(slot);
            let Some(Taken::Direction(direction)) = taken else {
                panic!("{what}: {slot:?} lit as {target:?} but holds {taken:?}");
            };
            let body = direction.body().map(shown);
            match (direction, target) {
                (DirRef::Normal(face) | DirRef::Axis(AxisRef::Face(face)), Picked::Face(f)) => {
                    let named = picking.faces()[f as usize].named(&face.key);
                    assert!(named, "{what}: {slot:?} lit as face {f}, not {face:?}");
                }
                (DirRef::Axis(AxisRef::Edge(edge)), Picked::Edge(e)) => {
                    let keys = index.chain_keys(e);
                    assert!(
                        keys.is_some() && {
                            let [a, b] = index.edge_faces(e).unwrap();
                            let face = |f: u32| &picking.faces()[f as usize];
                            (face(a).named(&edge.faces[0]) && face(b).named(&edge.faces[1]))
                                || (face(a).named(&edge.faces[1]) && face(b).named(&edge.faces[0]))
                        },
                        "{what}: {slot:?} lit as edge {e} ({keys:?}), not {edge:?}"
                    );
                }
                _ => panic!("{what}: {slot:?} lit as {target:?} for {direction:?}"),
            }
            assert_eq!(
                index.body(target),
                body,
                "{what}: {slot:?} lit on another body"
            );
        }
    }
}

/// Edits the align `id`, holding the session to what it stores, and OK
/// on it straight away to writing nothing.
fn edit_and_ok(plates: &mut Plates, id: FeatureId, what: &str) {
    let Some(FeatureKind::Align(stored)) =
        (plates.doc.editor.document().feature(id)).map(|feature| feature.kind.clone())
    else {
        return;
    };
    plates.doc.look(Look::EditFeature(id));
    let Some(session) = plates.doc.motion.as_ref() else {
        // Not editable now (the panel can't open).
        return;
    };
    assert_eq!(session.feature, Some(id), "{what}");
    assert_eq!(session.align(), Some(*stored.clone()), "{what}");
    let revision = plates.doc.editor.revision();
    let document = plates.doc.editor.document().clone();
    plates.doc.update(Edit::CommitMotion);
    if plates.doc.motion.is_none() {
        assert_eq!(plates.doc.editor.revision(), revision, "{what}: wrote");
        assert_eq!(*plates.doc.editor.document(), document, "{what}");
    }
}

/// A join or a combine merging bodies before the align (a new one; one
/// edited has them after it): a disc joined across the plate and the
/// right disc, or a disc combined into the plate.
fn merge(plates: &mut Plates, rng: &mut Rng) {
    let [plate, right, left] = plates.bodies;
    if rng.below(2) == 0 {
        let extent = two_sides(plates.doc.editor.document(), "12", "1");
        let operation = Operation::Join(Targets::default());
        add_disc_of(&mut plates.doc.editor, (14.0, 0.0), 7.0, extent, operation);
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
        // Sessions are started often, and left rarely, so they get far.
        let open = plates.doc.motion.is_some();
        let roll = match rng.below(4) {
            0 if !open => 0,
            _ => rng.below(42),
        };
        match roll {
            0 | 1 if open && rng.below(6) != 0 => plates.answer(),
            0 | 1 => plates.doc.look(Look::StartAlign),
            2 => {
                let ids = aligns(&plates);
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
                    plates.doc.look(Look::ClickBody { body, add: false });
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
            12 | 36..=38 => {
                let slot = random_slot(&mut rng);
                plates.motion(MotionLook::Picking(MotionPick::Align(slot)));
            }
            13 => plates.motion(MotionLook::Picking(MotionPick::Nothing)),
            14 => plates.motion(MotionLook::Clear(random_slot(&mut rng))),
            15 => plates.motion(MotionLook::OriginPoint),
            16 => plates.motion(MotionLook::OriginAxis(*rng.pick(&Axis3::ALL))),
            17 => plates.motion(MotionLook::Flip),
            18 => plates.input(MotionField::Distance, rng.pick(DISTANCES)),
            19 => plates.input(MotionField::Angle, rng.pick(ANGLES)),
            20 if rng.below(2) == 0 => plates.doc.update(Edit::Undo),
            21 if rng.below(2) == 0 => plates.doc.update(Edit::Redo),
            22 => {
                let before = plates.doc.motion.as_ref().and_then(|session| {
                    let ready =
                        plates.doc.motion_ready() && plates.doc.feed.draft_error().is_none();
                    (ready && session.kind == MotionKind::Align)
                        .then(|| (session.feature, session.align()))
                });
                plates.doc.update(Edit::CommitMotion);
                if let Some((edited, Some(align))) = before {
                    assert!(
                        plates.doc.motion.is_none(),
                        "{what}: not committed: {:?}",
                        plates.doc.edit_error
                    );
                    let id = edited.unwrap_or_else(|| plates.last_feature().0);
                    let stored = &plates.doc.editor.document().feature(id).unwrap().kind;
                    assert_eq!(*stored, FeatureKind::from(align), "{what}");
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
                let ids = aligns(&plates);
                if !ids.is_empty() {
                    let id = *rng.pick(&ids);
                    edit_and_ok(&mut plates, id, &what);
                }
            }
            _ => plates.answer(),
        }
        // Models mostly arrive soon after, now and then much later.
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

/// See the module's docs. `VARDE_ALIGN_SEEDS` runs more seeds
/// (`VARDE_ALIGN_FROM` the first).
#[test]
fn random_align_sessions_hold() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let from = number("VARDE_ALIGN_FROM", 0);
    for seed in from..from.saturating_add(number("VARDE_ALIGN_SEEDS", 3)) {
        run(seed, 200);
    }
}
