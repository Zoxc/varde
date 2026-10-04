//! Random sequences of what the user can do around the loft session,
//! regeneration lofting two equal parallel sections by extruding
//! ([`varde_regen::testing::loft_by_extrude`]), on the example plate and
//! a second box with section sketches (squares on XY under squares on the
//! plate's and the box's tops, a square on XZ, a point on its own, a
//! square whose corner has a second point on it, a region with a hole, a
//! circle) and a rail sketch: starting and editing lofts, regions clicked
//! on any sketch (regions not there among them), points clicked as
//! sections (points at both ends, ids not there), start dots moved to any
//! point of a section's sketch, rows moved up or taken out, Smooth and
//! Ruled, Closed (the rails kept hidden), rails clicked and taken out,
//! what picks toggled, the operation and its bodies, units and the
//! tolerance changed, a section's sketch redrawn with a start's point
//! moved a hair off its corner or onto another, rows and the model
//! hovered and clicked, the overlap list opened and its rows hovered,
//! ticked and chosen, undo and redo, a new section sketch added
//! mid-session and undone, sketches removed, joins and combines merging
//! bodies before the loft, commits, Add anyway and cancels.
//!
//! After each step: the loft set up is within the document's limits (its
//! sections and rails, each rail sorted, none twice); a section's start
//! that isn't said to be gone is drawn as its dot; a session that's ready
//! is whole, passes its own check and its names', the document takes it,
//! every region section is found in its sketch with its start at a
//! corner by regeneration's rule, and it's previewed as set up; a preview
//! answered for it never says a section or its start wasn't found; one
//! ready whose preview didn't fail commits, and the document holds what
//! it drafted; the overlap list is of the model shown and its ticks the
//! session's own; a loft edited opens to what it stores and OK on it
//! straight away writes nothing; the panel's texts never panic; and the
//! document passes its check. At the end, each loft making a body that
//! the model shows working is the box between its two sections.

use glam::DVec2;
use varde_document::{BodyOp, Combine, MAX_LOFT_RAILS, MAX_LOFT_SECTIONS, Operation, Targets};
use varde_expr::LengthUnit;
use varde_kernel::Tolerance;
use varde_view::{LoftShape, OperationKind, OverlapItems, Overlaps, Pick};

use super::super::chamfer::fuzz::Rng;
use super::super::face_session::{add_box, click, held};
use super::super::shell::fuzz::random_pick;
use super::*;
use crate::tests::{add_disc, screen_texts, two_sides};

/// The plate's top as a face to sketch on, about `near`: its extrude
/// `maker`'s end cap.
fn top_of(body: BodyId, maker: FeatureId, near: (f64, f64)) -> Plane {
    Plane::Face(FaceRef {
        body,
        key: FaceKey {
            feature: maker.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::new(near.0, near.1, 10.0),
    })
}

/// The example plate ("Body 1", 60 × 40 × 10 about the origin) and a box
/// from (40, −10) to (60, 10), 10 high ("Body 2"); squares on XY under
/// matching squares on their tops (one on the plate's with a point in its
/// middle), a square on XZ, a point on its own on XZ, a square on XY whose
/// first corner has a point of its own drawn before the square's, under a
/// square on the plate's top, a square with a circle in it, a circle on
/// XZ, and a rail of two lines on YZ.
fn lofts_set_up() -> (Plates, Vec<[FeatureId; 2]>) {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let plate_maker = editor.document().features()[1].id;
    add_box(&mut editor, DVec2::new(40.0, -10.0), DVec2::new(60.0, 10.0));
    let block = editor.document().bodies()[1].id;
    let block_maker = editor.document().features().last().unwrap().id;
    let xy = Plane::Origin(OriginPlane::XY);
    let xz = Plane::Origin(OriginPlane::XZ);
    let low = sketch_on(&mut editor, xy, square((15.0, -5.0), (25.0, 5.0)));
    let top = sketch_on(&mut editor, top_of(plate, plate_maker, (20.0, 0.0)), |s| {
        square((15.0, -5.0), (25.0, 5.0))(s);
        s.add_point(DVec2::new(20.0, 0.0)).unwrap();
    });
    sketch_on(&mut editor, xz, square((15.0, 20.0), (25.0, 30.0)));
    sketch_on(&mut editor, xz, |s| {
        s.add_point(DVec2::new(20.0, 40.0)).unwrap();
    });
    // A corner with two points on it, the square's lines using the second.
    let dup = sketch_on(&mut editor, xy, |s| {
        s.add_point(DVec2::new(-25.0, -5.0)).unwrap();
        square((-25.0, -5.0), (-15.0, 5.0))(s);
    });
    let top2 = sketch_on(
        &mut editor,
        top_of(plate, plate_maker, (-20.0, 0.0)),
        square((-25.0, -5.0), (-15.0, 5.0)),
    );
    let block_low = sketch_on(&mut editor, xy, square((47.0, -3.0), (53.0, 3.0)));
    let block_top = sketch_on(
        &mut editor,
        top_of(block, block_maker, (50.0, 0.0)),
        square((47.0, -3.0), (53.0, 3.0)),
    );
    sketch_on(&mut editor, xy, |s| {
        square((-60.0, -60.0), (-40.0, -40.0))(s);
        let center = s.add_point(DVec2::new(-50.0, -50.0)).unwrap();
        s.add_curve(
            Curve::Circle {
                center,
                radius: 4.0,
            },
            false,
        )
        .unwrap();
    });
    sketch_on(&mut editor, xz, |s| {
        let center = s.add_point(DVec2::new(-20.0, 30.0)).unwrap();
        s.add_curve(
            Curve::Circle {
                center,
                radius: 3.0,
            },
            false,
        )
        .unwrap();
    });
    sketch_on(&mut editor, Plane::Origin(OriginPlane::YZ), |s| {
        let [a, b, c] = [(-5.0, 0.0), (-5.0, 5.0), (-5.0, 10.0)]
            .map(|(x, y)| s.add_point(DVec2::new(x, y)).unwrap());
        s.add_curve(Curve::Line { start: a, end: b }, false)
            .unwrap();
        s.add_curve(Curve::Line { start: b, end: c }, false)
            .unwrap();
    });
    let mut plates = held(editor.document().clone());
    // The sketches on the tops are placed once the model is in.
    plates.answer();
    (
        plates,
        vec![[low, top], [dup, top2], [block_low, block_top]],
    )
}

/// The sketches of the document, in order.
fn sketch_ids(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::Sketch { .. }))
        .map(|feature| feature.id)
        .collect()
}

/// The lofts of the document.
fn lofts(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::Loft(_)))
        .map(|feature| feature.id)
        .collect()
}

/// The sketch of `feature` in `document`, if it's a sketch.
fn drawn(document: &Document, feature: FeatureId) -> Option<&Sketch> {
    match &document.feature(feature)?.kind {
        FeatureKind::Sketch { sketch, .. } => Some(sketch),
        _ => None,
    }
}

/// A point of sketch `sketch` at random, now and then a curve's id (no
/// point's).
fn random_point(plates: &Plates, sketch: FeatureId, rng: &mut Rng) -> Option<Id> {
    let drawn = drawn(plates.doc.editor.document(), sketch)?;
    match (drawn.points.as_slice(), drawn.curves.first()) {
        (_, Some(curve)) if rng.below(12) == 0 => Some(curve.id),
        ([], _) => None,
        (points, _) => Some(rng.pick(points).id),
    }
}

/// A curve of a random sketch, or an id that isn't a curve's.
fn random_curve(plates: &Plates, rng: &mut Rng) -> Option<(FeatureId, Id)> {
    let ids = sketch_ids(plates);
    if ids.is_empty() {
        return None;
    }
    let sketch = *rng.pick(&ids);
    let drawn = drawn(plates.doc.editor.document(), sketch)?;
    let curve = match (drawn.curves.as_slice(), drawn.points.first()) {
        ([], _) => return None,
        (_, Some(point)) if rng.below(12) == 0 => point.id,
        (curves, _) => rng.pick(curves).id,
    };
    Some((sketch, curve))
}

/// A section's sketch (or any sketch) redrawn: one of its points that a
/// curve uses handed over to a new point at its place, and the old one
/// moved off by a hair or more, so a start at it is at its corner by
/// some tolerances and not others; now and then a point added on a
/// corner instead, or a square's corner moved.
fn redraw(plates: &mut Plates, rng: &mut Rng) {
    let section_sketches: Vec<FeatureId> = (plates.doc.motion.as_ref())
        .map(|session| (session.loft.sections.iter().map(Section::sketch)).collect())
        .unwrap_or_default();
    let sketch = if !section_sketches.is_empty() && rng.below(4) != 0 {
        *rng.pick(&section_sketches)
    } else {
        let ids = sketch_ids(plates);
        if ids.is_empty() {
            return;
        }
        *rng.pick(&ids)
    };
    let document = plates.doc.editor.document();
    let Some(mut redrawn) = drawn(document, sketch).cloned() else {
        return;
    };
    let used: Vec<Id> = (redrawn.curves.iter())
        .flat_map(|entry| entry.curve.points())
        .filter(|id| redrawn.point(*id).is_some())
        .collect();
    if used.is_empty() {
        return;
    }
    let old = *rng.pick(&used);
    let at = redrawn.point(old).unwrap().at;
    let hair = *rng.pick(&[0.0, 5e-9, 5e-7, 5e-6, 5e-5, 5e-4, 0.5]);
    match rng.below(4) {
        0 => {
            // Another point on the corner, the curves keeping theirs.
            let _ = redrawn.add_point(at + DVec2::new(hair, 0.0));
        }
        1 => {
            // The corner moved, the curves with it.
            if let Some(point) = redrawn.points.iter_mut().find(|p| p.id == old) {
                point.at.y += hair.max(0.25);
            }
        }
        _ => {
            let Ok(fresh) = redrawn.add_point(at) else {
                return;
            };
            for entry in &mut redrawn.curves {
                if let Curve::Line { start, end } = &mut entry.curve {
                    for end in [start, end] {
                        if *end == old {
                            *end = fresh;
                        }
                    }
                }
            }
            if let Some(point) = redrawn.points.iter_mut().find(|p| p.id == old) {
                point.at.x += hair;
            }
        }
    }
    plates.doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(redrawn),
    });
    plates.doc.sync();
}

/// A join or a combine merging bodies: a disc joined across the plate and
/// the box, or one body combined into another.
fn merge(plates: &mut Plates, rng: &mut Rng) {
    if rng.below(3) == 0 {
        let extent = two_sides(plates.doc.editor.document(), "12", "1");
        let operation = Operation::Join(Targets::default());
        add_disc(&mut plates.doc.editor, (35.0, 15.0), extent, operation);
    } else {
        let document = plates.doc.editor.document();
        let all: Vec<BodyId> = document.bodies().iter().map(|body| body.id).collect();
        if all.len() < 2 {
            return;
        }
        let target = *rng.pick(&all);
        let tool = *rng.pick(&all);
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

/// The list of two to four things of the model shown.
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

/// Where `section`'s region's corners are by regeneration's rule, in
/// `document` as it is: `None` if its region or start isn't found there
/// (a point section's point likewise).
fn found_by_regen(document: &Document, section: &Section) -> Option<()> {
    let sketch = drawn(document, section.sketch())?;
    match section {
        Section::Point { point, .. } => sketch.point(*point).map(drop),
        Section::Region { region, start, .. } => {
            let profiles = sketch.profiles().ok()?;
            let index = profiles.resolve(std::slice::from_ref(region))[0]?;
            let loops = profiles.merge(&[index]).ok()?;
            let Some(start) = start else {
                return Some(());
            };
            let resolution = document.tolerance().resolution();
            varde_regen::loft_corner(sketch, &profiles, &loops[0], *start, resolution).map(drop)
        }
    }
}

/// Holds the session, if it's a loft's, to what the module's docs say.
fn check_session(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
    if let (Some(ticks), Some(listed)) = (plates.doc.overlap_ticks(), &plates.doc.overlaps)
        && let OverlapItems::Model(picks) = &listed.list.items
    {
        for (&tick, &pick) in ticks.iter().zip(picks) {
            assert_eq!(Some(tick), plates.doc.motion_has(pick), "{what}");
        }
    }
    if session.kind != MotionKind::Loft {
        return;
    }
    let document = plates.doc.editor.document();
    if let Some(id) = session.feature {
        let kind = document.feature(id).map(|feature| &feature.kind);
        assert!(matches!(kind, Some(FeatureKind::Loft(_))), "{what}");
    }
    let setup = &session.loft;
    assert!(setup.sections.len() <= MAX_LOFT_SECTIONS, "{what}");
    assert!(setup.rails.len() <= MAX_LOFT_RAILS, "{what}");
    for (at, rail) in setup.rails.iter().enumerate() {
        assert!(
            !rail.curves.is_empty() && rail.curves.windows(2).all(|pair| pair[0] < pair[1]),
            "{what}: {rail:?}"
        );
        assert!(!setup.rails[..at].contains(rail), "{what}: {rail:?} twice");
    }
    assert!(session.bodies.is_empty(), "{what}: {:?}", session.bodies);
    let state = plates.doc.motion_state().expect("a session's state");
    let view = state.loft.as_ref().expect("a loft's view");
    assert_eq!(view.sections.len(), setup.sections.len(), "{what}");
    for (shown, section) in view.sections.iter().zip(&setup.sections) {
        if let (false, Section::Region { start: Some(_), .. }) = (shown.gone, section)
            && let LoftShape::Region {
                region: Some(_), ..
            } = shown.shape
        {
            assert!(
                shown.shape.dot().is_some(),
                "{what}: {section:?}'s start isn't drawn"
            );
        }
    }
    if !plates.doc.motion_ready() {
        return;
    }
    let loft = session
        .loft()
        .unwrap_or_else(|| panic!("{what}: ready, not whole"));
    loft.check_own()
        .unwrap_or_else(|why| panic!("{what}: {why}: {loft:?}"));
    loft.check_names(|id| drawn(document, id))
        .unwrap_or_else(|why| panic!("{what}: {why}: {loft:?}"));
    for section in &loft.sections {
        assert!(
            found_by_regen(document, section).is_some(),
            "{what}: ready, but regeneration won't find {section:?}"
        );
    }
    let mut editor = plates.doc.editor.clone();
    let command = match session.feature {
        Some(feature) => Command::SetFeature {
            feature,
            kind: Box::new(FeatureKind::Loft(loft.clone())),
        },
        None => document.add_feature(FeatureKind::Loft(loft.clone())),
    };
    editor
        .apply(command)
        .unwrap_or_else(|why| panic!("{what}: ready, refused: {why:?}: {loft:?}"));
    assert_eq!(
        plates.doc.motion_draft(),
        Some((session.feature, FeatureKind::Loft(loft))),
        "{what}"
    );
}

/// Holds the answer to the preview of a ready session as set up never to
/// say a section or its start wasn't found or isn't a corner.
fn check_answer(plates: &Plates, what: &str) {
    if !plates.doc.motion_ready() || plates.last_draft() != plates.doc.motion_draft() {
        return;
    }
    if let Some(error) = plates.doc.feed.draft_error() {
        for words in [
            "not found",
            "wasn't found",
            "isn't one of its corners",
            "holes",
        ] {
            assert!(
                !error.contains(words),
                "{what}: ready, previewed as {error}"
            );
        }
    }
}

/// Edits the loft `id`, holding the session to what it stores, and OK on
/// it straight away to writing nothing.
fn edit_and_ok(plates: &mut Plates, id: FeatureId, what: &str) {
    let Some(FeatureKind::Loft(stored)) =
        (plates.doc.editor.document().feature(id)).map(|feature| feature.kind.clone())
    else {
        return;
    };
    plates.doc.look(Look::EditFeature(id));
    let Some(session) = plates.doc.motion.as_ref() else {
        return;
    };
    assert_eq!(session.feature, Some(id), "{what}");
    // A body it makes is the document's to number.
    let opened = (session.loft()).map(|opened| Loft {
        operation: match (&opened.operation, &stored.operation) {
            (Operation::NewBody(_), Operation::NewBody(body)) => Operation::NewBody(*body),
            _ => opened.operation.clone(),
        },
        ..opened
    });
    assert_eq!(opened, Some(stored), "{what}");
    let revision = plates.doc.editor.revision();
    let document = plates.doc.editor.document().clone();
    plates.doc.update(Edit::CommitMotion);
    if plates.doc.motion.is_none() {
        assert_eq!(plates.doc.editor.revision(), revision, "{what}");
        assert_eq!(*plates.doc.editor.document(), document, "{what}");
    }
}

/// Holds each loft making a body that works to the box between its two
/// sections, in the history ending with it: its volume its box's, and its
/// box that of the sections' corners.
fn check_volumes(plates: &Plates, what: &str) {
    let document = plates.doc.editor.document();
    for (index, feature) in document.features().iter().enumerate() {
        let FeatureKind::Loft(loft) = &feature.kind else {
            continue;
        };
        let Operation::NewBody(body) = loft.operation else {
            continue;
        };
        let mut editor = Editor::new(document.clone());
        for later in document.features()[index + 1..].iter().rev() {
            if editor.document().feature(later.id).is_some() {
                editor.apply(Command::RemoveFeature(later.id)).unwrap();
            }
        }
        let evaluation =
            varde_regen::evaluate(editor.document(), &mut varde_regen::Cache::default());
        if evaluation.failed.iter().any(|f| f.feature == feature.id) {
            continue;
        }
        let made = (evaluation.bodies.iter())
            .find(|made| made.body == body)
            .unwrap_or_else(|| panic!("{what}: {loft:?} made nothing"));
        let bounds = made.solid.bounds3().expect("a body");
        let size = bounds.max - bounds.min;
        let volume = made.solid.volume();
        assert!(
            (volume - size.x * size.y * size.z).abs() <= 1e-9 * volume,
            "{what}: {loft:?} isn't a box"
        );
        let mut low = DVec3::INFINITY;
        let mut high = DVec3::NEG_INFINITY;
        for section in &loft.sections {
            let Section::Region { sketch, region, .. } = section else {
                panic!("{what}: lofted a point: {loft:?}");
            };
            let placement = plates.doc.placement(*sketch).expect("placed");
            let drawn = drawn(document, *sketch).unwrap();
            let profiles = drawn.profiles().unwrap();
            let index = profiles.resolve(std::slice::from_ref(region))[0].unwrap();
            for piece in &profiles.regions[index].outer {
                let at = placement.to_world(profiles.vertices[piece.start]);
                low = low.min(at);
                high = high.max(at);
            }
        }
        assert!(
            bounds.min.distance(low) < 1e-6 && bounds.max.distance(high) < 1e-6,
            "{what}: {loft:?} not between its sections: {bounds:?} vs {low} {high}"
        );
    }
}

/// OK on the session: one ready whose preview didn't fail commits, the
/// document holding what it drafted.
fn commit(plates: &mut Plates, what: &str) {
    let before = plates.doc.motion.as_ref().and_then(|session| {
        let ready = plates.doc.motion_ready() && plates.doc.feed.draft_error().is_none();
        (ready && session.kind == MotionKind::Loft).then(|| (session.feature, session.loft()))
    });
    plates.doc.update(Edit::CommitMotion);
    if let Some((edited, Some(loft))) = before {
        assert!(
            plates.doc.motion.is_none(),
            "{what}: not committed: {:?}",
            plates.doc.edit_error
        );
        let id = edited.unwrap_or_else(|| plates.last_feature().0);
        let stored = &plates.doc.editor.document().feature(id).unwrap().kind;
        let mut drafted = loft;
        if let (FeatureKind::Loft(stored), Operation::NewBody(_)) = (stored, &drafted.operation) {
            // A new one's body is the document's to number.
            drafted.operation = stored.operation.clone();
        }
        assert_eq!(*stored, FeatureKind::Loft(drafted), "{what}");
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0x5be0_cd19_137e_2179 ^ (seed + 1).wrapping_mul(0x9e37_79b9));
    varde_regen::testing::loft_by_extrude();
    let (mut plates, pairs) = lofts_set_up();
    let base = plates.doc.editor.document().features().len();
    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        let open = plates.doc.motion.is_some();
        let roll = match rng.below(4) {
            0 if !open => 0,
            _ => rng.below(56),
        };
        match roll {
            0 | 1 if open && rng.below(6) != 0 => plates.answer(),
            0 | 1 => plates.doc.look(Look::StartLoft),
            2 => {
                let ids = lofts(&plates);
                if !ids.is_empty() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                }
            }
            3..=8 => {
                let ids = sketch_ids(&plates);
                if !ids.is_empty() {
                    let sketch = *rng.pick(&ids);
                    let region = if rng.below(4) == 0 { rng.below(3) } else { 0 };
                    plates.motion(MotionLook::LoftRegion { sketch, region });
                }
            }
            9 | 10 => {
                let ids = sketch_ids(&plates);
                if !ids.is_empty() {
                    let sketch = *rng.pick(&ids);
                    if let Some(point) = random_point(&plates, sketch, &mut rng) {
                        plates.motion(MotionLook::LoftPoint { sketch, point });
                    }
                }
            }
            11..=13 => {
                let sections = (plates.doc.motion.as_ref())
                    .map(|session| session.loft.sections.clone())
                    .unwrap_or_default();
                let section = rng.below(sections.len() + 1);
                let sketch = sections.get(section).map(Section::sketch).or_else(|| {
                    let ids = sketch_ids(&plates);
                    (!ids.is_empty()).then(|| *rng.pick(&ids))
                });
                if let Some(sketch) = sketch
                    && let Some(point) = random_point(&plates, sketch, &mut rng)
                {
                    plates.motion(MotionLook::LoftStart { section, point });
                }
            }
            14 | 15 => plates.motion(MotionLook::SectionUp(rng.below(5))),
            16 => plates.motion(MotionLook::DropSection(rng.below(5))),
            17 | 18 => {
                if let Some((sketch, curve)) = random_curve(&plates, &mut rng) {
                    plates.motion(MotionLook::LoftRail { sketch, curve });
                }
            }
            19 => plates.motion(MotionLook::DropRail(rng.below(3))),
            20 => {
                let mode = *rng.pick(&[LoftMode::Smooth, LoftMode::Ruled]);
                plates.motion(MotionLook::LoftMode(mode));
            }
            21 => plates.motion(MotionLook::Closed),
            22 => {
                let picking = *rng.pick(&[
                    MotionPick::Regions,
                    MotionPick::Path,
                    MotionPick::Reference,
                    MotionPick::Nothing,
                ]);
                plates.motion(MotionLook::Picking(picking));
            }
            23 => {
                let kind = *rng.pick(&[
                    OperationKind::NewBody,
                    OperationKind::Join,
                    OperationKind::Cut,
                    OperationKind::Intersect,
                ]);
                plates.motion(MotionLook::Operation(kind));
            }
            24 => {
                let bodies: Vec<BodyId> = (plates.doc.editor.document().bodies().iter())
                    .map(|body| body.id)
                    .collect();
                if !bodies.is_empty() {
                    plates.motion(MotionLook::Target(*rng.pick(&bodies)));
                }
            }
            25 => {
                let units = *rng.pick(&LengthUnit::ALL);
                plates.doc.update(Edit::SetUnits(units));
            }
            26 | 27 => {
                let fit = *rng.pick(&[1e-5, 1e-4, 1e-3, 1e-2, 0.1]);
                plates
                    .doc
                    .update(Edit::SetTolerance(Tolerance::new(fit).unwrap()));
            }
            28 | 29 => redraw(&mut plates, &mut rng),
            30 => {
                let at = rng.below(4);
                let hover = *rng.pick(&[PanelHover::Section(at), PanelHover::Part(at)]);
                if rng.below(2) == 0 {
                    plates.doc.look(Look::HoverPanel(Some(hover)));
                } else {
                    plates.doc.look(Look::LeavePanel(hover));
                }
            }
            31 => {
                let pick = random_pick(&plates, &mut rng);
                if rng.below(2) == 0 {
                    plates.doc.look(Look::Hover(pick));
                } else if let Some(pick) = pick {
                    click(&mut plates, pick);
                }
            }
            32 | 33 if rng.below(2) == 0 => {
                plates.doc.update(Edit::Undo);
                if plates.doc.editor.document().features().len() < base {
                    plates.doc.update(Edit::Redo);
                }
            }
            34 if rng.below(2) == 0 => plates.doc.update(Edit::Redo),
            35..=38 => commit(&mut plates, &what),
            39 | 40 => plates.doc.update(Edit::AcceptError),
            41 if rng.below(3) == 0 => plates.motion(MotionLook::Cancel),
            42 | 43 => merge(&mut plates, &mut rng),
            44 => {
                // A new section sketch added mid-session, picked now and
                // then, undone now and then.
                let mut editor = plates.doc.editor.clone();
                let x = 100.0 + 10.0 * rng.below(3) as f64;
                sketch_on(
                    &mut editor,
                    Plane::Origin(OriginPlane::XY),
                    square((x, 0.0), (x + 2.0, 2.0)),
                );
                let document = editor.document().clone();
                let added = &document.features()[plates.doc.editor.document().features().len()..];
                for feature in added {
                    let add = plates
                        .doc
                        .editor
                        .document()
                        .add_feature(feature.kind.clone());
                    plates.doc.apply(add);
                }
                plates.doc.sync();
                if rng.below(2) == 0 {
                    plates.answer();
                    if let Some(&sketch) = sketch_ids(&plates).last() {
                        plates.motion(MotionLook::LoftRegion { sketch, region: 0 });
                    }
                }
                if rng.below(2) == 0 {
                    plates.doc.update(Edit::Undo);
                    if rng.below(2) == 0 {
                        plates.doc.update(Edit::Redo);
                    }
                }
            }
            45 => {
                // A sketch or a feature after the bodies removed, with
                // what depends on it.
                let features = plates.doc.editor.document().features();
                if features.len() > 4 {
                    let id = features[4 + rng.below(features.len() - 4)].id;
                    plates.doc.apply(Command::RemoveFeature(id));
                    plates.doc.sync();
                }
            }
            46 => {
                let ids = lofts(&plates);
                if !ids.is_empty() {
                    let id = *rng.pick(&ids);
                    edit_and_ok(&mut plates, id, &what);
                }
            }
            47 | 48 => {
                if let Some(list) = random_overlaps(&plates, &mut rng) {
                    plates.doc.look(Look::OpenOverlaps(list));
                }
            }
            49 | 50 => {
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
            52 | 53 => {
                // Two sections that loft, by regions clicked: every
                // section taken out first now and then.
                if rng.below(2) == 0 {
                    for _ in 0..8 {
                        plates.motion(MotionLook::DropSection(0));
                    }
                    for _ in 0..MAX_LOFT_RAILS {
                        plates.motion(MotionLook::DropRail(0));
                    }
                    if (plates.doc.motion.as_ref()).is_some_and(|session| session.loft.closed) {
                        plates.motion(MotionLook::Closed);
                    }
                }
                let mut pair = *rng.pick(&pairs);
                if rng.below(2) == 0 {
                    pair.reverse();
                }
                for sketch in pair {
                    plates.motion(MotionLook::LoftRegion { sketch, region: 0 });
                }
            }
            51 => {
                // A loft with a chamfer being set up.
                plates.doc.look(Look::StartChamfer);
                plates.doc.look(Look::StartLoft);
            }
            _ => {
                plates.answer();
                check_answer(&plates, &what);
            }
        }
        if rng.below(3) == 0 {
            plates.answer();
            check_answer(&plates, &what);
            // A loft that works is committed now and then.
            if plates.doc.motion_ready()
                && plates.doc.feed.draft_error().is_none()
                && rng.below(4) == 0
            {
                commit(&mut plates, &what);
            }
        }
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

/// See the module's docs. `VARDE_LOFT_SEEDS` runs more seeds
/// (`VARDE_LOFT_FROM` the first).
#[test]
fn random_loft_sessions_hold() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let from = number("VARDE_LOFT_FROM", 0);
    for seed in from..from.saturating_add(number("VARDE_LOFT_SEEDS", 3)) {
        run(seed, 200);
    }
}
