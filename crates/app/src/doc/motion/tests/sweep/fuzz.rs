//! Random sequences of what the user can do around the sweep session,
//! regeneration sweeping straight paths by extruding
//! ([`varde_regen::testing::sweep_by_extrude`]), on two boxes with
//! profile sketches and path sketches (up a box's corner and on past it
//! on another plane, bent, a circle, down, one in the profile's plane):
//! starting and editing sweeps (from idle, or with a chamfer being set
//! up), regions clicked on any sketch (the profile's, others, ones made
//! later, regions not there), sketch curves clicked (the profile's own,
//! later sketches', curves not there) to add their chain or take it out,
//! model edges clicked (on the model shown or one gone by, faces among
//! them) to pick them or take them out, rows' crosses, what picks
//! toggled, Path and Helix toggled (a helix's edge axis gone by an undo
//! among them), the helix's axis picked from the toolbar or a click,
//! pitch, turns and twist typed (out of range, overflowing, not numbers
//! among them), Keep orientation, Left-handed, Flip, Tangent chain, the
//! operation and its bodies, units changed, rows and the model hovered,
//! the list of what overlaps opened and its rows hovered, ticked and
//! chosen, undo and redo, a new profile sketch added mid-session and
//! undone, the profile's or a path's sketch removed, joins and combines
//! merging bodies before the sweep, commits, Add anyway and cancels.
//!
//! After each step: the path set up is within the document's limits
//! (its parts and its curves and edges in all), each part naming some,
//! sorted, its edges on one body; a session that's ready is whole,
//! passes its own check and the document's for its path and curves, and
//! is previewed as set up; one ready whose preview didn't fail commits,
//! and the document holds what it drafted; the edges lit are on the body
//! drawing the edges' body; the overlap list is of the model shown and
//! its ticks are the session's own; a helix's axis drawn is finite; a
//! sweep edited opens to what it stores and OK on it straight away
//! writes nothing; a session never outlives the sweep it edits; the
//! panel's texts never panic; and the document passes its check. Each
//! sweep making a body that the model shows working is its profile
//! extruded along the profile's normal from its plane.

use glam::DVec2;
use varde_document::{
    Axis3, BodyOp, Combine, Document, Editor, FeatureId, MAX_PATH_CURVES, MAX_PATH_PARTS,
    Operation, Targets,
};
use varde_expr::LengthUnit;
use varde_view::{OverlapItems, Overlaps, Pick};

use super::super::chamfer::fuzz::Rng;
use super::super::face_session::{add_box, click, held};
use super::super::shell::fuzz::random_pick;
use super::*;
use crate::tests::{add_disc, screen_texts, two_sides};

const VALUES: &[&str] = &[
    "10", "5", "2.5", "0.001", "1000", "1001", "0", "-3", "1e6", "1e308", "1e400", "1/0", "abc",
    "", "90", "3000", "45 deg", "3 mm", "0.1 in",
];

/// The doc with two boxes ("Body 1" from (0, 0, 0) to (40, 30, 10),
/// "Body 2" from (60, 0, 0) to (80, 20, 10)), two square profiles on XY,
/// and path sketches: up Body 1's corner on the z axis to 20 on XZ, on
/// from 20 to 30 on YZ, bent on XZ, a circle on XZ, down on XZ, and a
/// line on XY.
fn sweeps_set_up() -> Plates {
    let mut editor = Editor::new(Document::default());
    add_box(&mut editor, DVec2::ZERO, DVec2::new(40.0, 30.0));
    add_box(&mut editor, DVec2::new(60.0, 0.0), DVec2::new(80.0, 20.0));
    sketch_on(
        &mut editor,
        OriginPlane::XY,
        rectangle((45.0, 40.0), (47.0, 42.0)),
    );
    sketch_on(
        &mut editor,
        OriginPlane::XY,
        rectangle((-10.0, -10.0), (-8.0, -8.0)),
    );
    let paths: [(OriginPlane, Vec<(f64, f64)>); 5] = [
        (OriginPlane::XZ, vec![(0.0, 0.0), (0.0, 10.0), (0.0, 20.0)]),
        (OriginPlane::YZ, vec![(0.0, 20.0), (0.0, 30.0)]),
        (
            OriginPlane::XZ,
            vec![(50.0, 0.0), (50.0, 5.0), (55.0, 10.0)],
        ),
        (OriginPlane::XZ, vec![(5.0, 0.0), (5.0, -10.0)]),
        (OriginPlane::XY, vec![(50.0, 50.0), (60.0, 50.0)]),
    ];
    for (plane, points) in paths {
        sketch_on(&mut editor, plane, polyline(points));
    }
    sketch_on(&mut editor, OriginPlane::XZ, |sketch| {
        let center = sketch.add_point(DVec2::new(50.0, 20.0)).unwrap();
        let circle = Curve::Circle {
            center,
            radius: 3.0,
        };
        sketch.add_curve(circle, false).unwrap();
    });
    held(editor.document().clone())
}

/// The sketches of the document, in order.
fn sketch_ids(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::Sketch { .. }))
        .map(|feature| feature.id)
        .collect()
}

/// The sweeps of the document.
fn sweeps(plates: &Plates) -> Vec<FeatureId> {
    (plates.doc.editor.document().features().iter())
        .filter(|feature| matches!(feature.kind, FeatureKind::Sweep(_)))
        .map(|feature| feature.id)
        .collect()
}

/// A random click on an edge of the model shown, now and then a face,
/// now and then on one gone by.
fn random_edge_pick(plates: &Plates, rng: &mut Rng) -> Option<Pick> {
    let index = plates.doc.feed.pick_index();
    if rng.below(6) == 0 {
        return random_pick(plates, rng);
    }
    let count = index.mesh().edge_count();
    if count == 0 {
        return None;
    }
    let target = Picked::Edge(rng.below(count) as u32);
    let snaps = index.snaps(target);
    let at = if snaps.is_empty() {
        DVec3::ZERO
    } else {
        rng.pick(&snaps).1
    };
    let model = if rng.below(10) == 0 {
        index.model().wrapping_sub(1)
    } else {
        index.model()
    };
    Some(Pick {
        model,
        target,
        body: index.body(target)?,
        at,
        snap: None,
    })
}

/// Holds the session, if it's a sweep's, to what the module's docs say.
fn check_session(plates: &Plates, what: &str) {
    let Some(session) = &plates.doc.motion else {
        return;
    };
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
    if session.kind != MotionKind::Sweep {
        return;
    }
    let document = plates.doc.editor.document();
    if let Some(id) = session.feature {
        let kind = document.feature(id).map(|feature| &feature.kind);
        assert!(matches!(kind, Some(FeatureKind::Sweep(_))), "{what}");
    }
    let (parts, curves) = session.path_counts();
    assert!(parts <= MAX_PATH_PARTS, "{what}: {parts} parts");
    assert!(curves <= MAX_PATH_CURVES, "{what}: {curves} curves");
    for chain in &session.sweep.chains {
        assert!(!chain.curves.is_empty(), "{what}");
        assert!(
            chain.curves.windows(2).all(|pair| pair[0] < pair[1]),
            "{what}: {chain:?}"
        );
    }
    let edges = &session.blend.edges.refs;
    assert!(
        edges.windows(2).all(|pair| pair[0].order(&pair[1]).is_lt()),
        "{what}: {edges:?}"
    );
    assert!(
        edges.iter().all(|edge| edge.body == edges[0].body),
        "{what}: {edges:?}"
    );
    assert!(session.bodies.is_empty(), "{what}: {:?}", session.bodies);
    check_lit(plates, what);
    if let Some(state) = plates.doc.motion_state()
        && let Some([at, way]) = state.line
    {
        assert!(at.is_finite() && way.is_finite(), "{what}: {at} {way}");
    }
    if !plates.doc.motion_ready() {
        return;
    }
    let sweep = session
        .sweep()
        .unwrap_or_else(|| panic!("{what}: ready, not whole"));
    sweep
        .check_own(&document.design())
        .unwrap_or_else(|why| panic!("{what}: {why}: {sweep:?}"));
    let features = document.features();
    let index = (session.feature)
        .and_then(|id| features.iter().position(|feature| feature.id == id))
        .unwrap_or(features.len());
    document
        .check_path(index, sweep.sketch, &sweep.path)
        .unwrap_or_else(|why| panic!("{what}: {why}: {sweep:?}"));
    sweep
        .check_curves(|id| crate::doc::revolve::sketch_of(document, id))
        .unwrap_or_else(|why| panic!("{what}: {why}: {sweep:?}"));
    assert_eq!(
        plates.doc.motion_draft(),
        Some((session.feature, FeatureKind::Sweep(sweep))),
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
    let body = (session.blend.edges.refs.first())
        .map(|edge| edge.body)
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

/// Edits the sweep `id`, holding the session to what it stores, and OK
/// on it straight away to writing nothing.
fn edit_and_ok(plates: &mut Plates, id: FeatureId, what: &str) {
    let Some(FeatureKind::Sweep(stored)) =
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
    let opened = (session.sweep()).map(|opened| Sweep {
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

/// A join or a combine merging bodies: a disc joined across both boxes,
/// or one body combined into another.
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
        if let Some(mut pick) = random_edge_pick(plates, rng) {
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

/// A curve of a random sketch, or an id that isn't a curve's.
fn random_curve(plates: &Plates, rng: &mut Rng) -> Option<(FeatureId, Id)> {
    let ids = sketch_ids(plates);
    if ids.is_empty() {
        return None;
    }
    let sketch = *rng.pick(&ids);
    let document = plates.doc.editor.document();
    let curves = curves_of(document, sketch);
    let FeatureKind::Sketch { sketch: drawn, .. } = &document.feature(sketch)?.kind else {
        return None;
    };
    let curve = match (curves.as_slice(), drawn.points.first()) {
        ([], _) => return None,
        // A point's id: no curve.
        (_, Some(point)) if rng.below(12) == 0 => point.id,
        _ => *rng.pick(&curves),
    };
    Some((sketch, curve))
}

/// Holds each sweep making a body that the model shows working to its
/// profile extruded along the profile's normal from its plane (on an
/// origin plane): a profile of lines (a rectangle) a box over it, a
/// circle a cylinder on it.
fn check_volumes(plates: &Plates, what: &str) {
    let document = plates.doc.editor.document();
    let evaluation = varde_regen::evaluate(document, &mut varde_regen::Cache::default());
    for feature in document.features() {
        let FeatureKind::Sweep(sweep) = &feature.kind else {
            continue;
        };
        let Operation::NewBody(body) = sweep.operation else {
            continue;
        };
        if evaluation.failed.iter().any(|f| f.feature == feature.id) {
            continue;
        }
        let Some(made) = evaluation.bodies.iter().find(|made| made.body == body) else {
            continue;
        };
        let FeatureKind::Sketch {
            sketch,
            plane: varde_document::Plane::Origin(plane),
            ..
        } = &document.feature(sweep.sketch).unwrap().kind
        else {
            continue;
        };
        let placement = plane.placement();
        let up = placement.normal.abs().max_position();
        let bounds = made.solid.bounds3().expect("a body");
        let size = bounds.max - bounds.min;
        let volume = made.solid.volume();
        assert!(
            bounds.min[up] == 0.0 || bounds.max[up] == 0.0,
            "{what}: {sweep:?} not from its plane: {bounds:?}"
        );
        let world: Vec<DVec3> = (sketch.points.iter())
            .map(|point| placement.to_world(point.at))
            .collect();
        let low = world.iter().fold(DVec3::INFINITY, |a, &p| a.min(p));
        let high = world.iter().fold(DVec3::NEG_INFINITY, |a, &p| a.max(p));
        match sketch.curves.as_slice() {
            curves
                if curves
                    .iter()
                    .all(|entry| matches!(entry.curve, Curve::Line { .. })) =>
            {
                assert!(
                    (volume - size.x * size.y * size.z).abs() <= 1e-9 * volume,
                    "{what}: {sweep:?} isn't a box"
                );
                for axis in (0..3).filter(|&axis| axis != up) {
                    assert!(
                        (bounds.min[axis] - low[axis]).abs() < 1e-9
                            && (bounds.max[axis] - high[axis]).abs() < 1e-9,
                        "{what}: {sweep:?} not over its profile: {bounds:?}"
                    );
                }
            }
            [entry] => {
                let Curve::Circle { radius, .. } = entry.curve else {
                    continue;
                };
                let wanted = std::f64::consts::PI * radius * radius * size[up];
                assert!(
                    (volume - wanted).abs() <= 1e-6 * wanted,
                    "{what}: {sweep:?} isn't its cylinder: {volume} vs {wanted}"
                );
            }
            _ => {}
        }
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(0xa54f_f53a_5f1d_36f1 ^ (seed + 1).wrapping_mul(0x9e37_79b9));
    varde_regen::testing::sweep_by_extrude();
    let mut plates = sweeps_set_up();
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
            0 | 1 => plates.doc.look(Look::StartSweep),
            2 => {
                let ids = sweeps(&plates);
                if !ids.is_empty() {
                    plates.doc.look(Look::EditFeature(*rng.pick(&ids)));
                }
            }
            3..=6 => {
                let ids = sketch_ids(&plates);
                if !ids.is_empty() {
                    plates.motion(MotionLook::SweepRegion {
                        sketch: *rng.pick(&ids),
                        region: rng.below(3),
                    });
                }
            }
            7..=11 => {
                if let Some((sketch, curve)) = random_curve(&plates, &mut rng) {
                    plates.motion(MotionLook::SweepCurve { sketch, curve });
                }
            }
            12..=15 => {
                if let Some(pick) = random_edge_pick(&plates, &mut rng) {
                    click(&mut plates, pick);
                }
            }
            16 => {
                // Quick clicks: several, nothing answered between them.
                for _ in 0..2 + rng.below(3) {
                    if let Some(pick) = random_edge_pick(&plates, &mut rng) {
                        click(&mut plates, pick);
                    }
                }
            }
            17 => plates.motion(MotionLook::DropPart(rng.below(4))),
            18 => {
                let edges = (plates.doc.motion.as_ref())
                    .map(|session| session.blend.edges.refs.clone())
                    .unwrap_or_default();
                if !edges.is_empty() {
                    plates.motion(MotionLook::DropEdge(*rng.pick(&edges)));
                }
            }
            19 => {
                let picking = *rng.pick(&[
                    MotionPick::Regions,
                    MotionPick::Path,
                    MotionPick::Reference,
                    MotionPick::Nothing,
                ]);
                plates.motion(MotionLook::Picking(picking));
            }
            20 | 21 => {
                let path = *rng.pick(&[SweepPath::Path, SweepPath::Helix]);
                plates.motion(MotionLook::SweepPath(path));
            }
            22 => {
                let axis = *rng.pick(&[Axis3::X, Axis3::Y, Axis3::Z]);
                plates.motion(MotionLook::OriginAxis(axis));
            }
            23 => {
                // The helix's axis clicked.
                plates.motion(MotionLook::Picking(MotionPick::Reference));
                if let Some(pick) = random_edge_pick(&plates, &mut rng) {
                    click(&mut plates, pick);
                }
            }
            24..=26 => {
                let field =
                    *rng.pick(&[MotionField::Pitch, MotionField::Turns, MotionField::Twist]);
                plates.input(field, rng.pick(VALUES));
            }
            27 => {
                let look = rng.pick(&[
                    MotionLook::KeepOrientation,
                    MotionLook::LeftHanded,
                    MotionLook::Flip,
                    MotionLook::Chain,
                ]);
                plates.motion(look.clone());
            }
            28 => {
                let kind = *rng.pick(&[
                    OperationKind::NewBody,
                    OperationKind::Join,
                    OperationKind::Cut,
                    OperationKind::Intersect,
                ]);
                plates.motion(MotionLook::Operation(kind));
            }
            29 => {
                let bodies: Vec<BodyId> = (plates.doc.editor.document().bodies().iter())
                    .map(|body| body.id)
                    .collect();
                if !bodies.is_empty() {
                    plates.motion(MotionLook::Target(*rng.pick(&bodies)));
                }
            }
            30 => {
                let units = *rng.pick(&LengthUnit::ALL);
                plates.doc.update(Edit::SetUnits(units));
            }
            31 => {
                let at = rng.below(4);
                let hover = *rng.pick(&[PanelHover::Part(at), PanelHover::Edge(at)]);
                if rng.below(2) == 0 {
                    plates.doc.look(Look::HoverPanel(Some(hover)));
                } else {
                    plates.doc.look(Look::LeavePanel(hover));
                }
            }
            32 => {
                if let Some(pick) = random_edge_pick(&plates, &mut rng) {
                    plates.doc.look(Look::Hover(Some(pick)));
                } else {
                    plates.doc.look(Look::Hover(None));
                }
            }
            33 | 34 if rng.below(2) == 0 => {
                plates.doc.update(Edit::Undo);
                if plates.doc.editor.document().features().len() < base {
                    plates.doc.update(Edit::Redo);
                }
            }
            35 if rng.below(2) == 0 => plates.doc.update(Edit::Redo),
            36..=38 => {
                let before = plates.doc.motion.as_ref().and_then(|session| {
                    let ready =
                        plates.doc.motion_ready() && plates.doc.feed.draft_error().is_none();
                    (ready && session.kind == MotionKind::Sweep)
                        .then(|| (session.feature, session.sweep()))
                });
                plates.doc.update(Edit::CommitMotion);
                if let Some((edited, Some(sweep))) = before {
                    assert!(
                        plates.doc.motion.is_none(),
                        "{what}: not committed: {:?}",
                        plates.doc.edit_error
                    );
                    let id = edited.unwrap_or_else(|| plates.last_feature().0);
                    let stored = &plates.doc.editor.document().feature(id).unwrap().kind;
                    let mut drafted = sweep;
                    if let (FeatureKind::Sweep(stored), Operation::NewBody(_)) =
                        (stored, &drafted.operation)
                    {
                        // A new one's body is the document's to number.
                        drafted.operation = stored.operation.clone();
                    }
                    assert_eq!(*stored, FeatureKind::Sweep(drafted), "{what}");
                }
            }
            39 => plates.doc.update(Edit::AcceptError),
            40 if rng.below(3) == 0 => plates.motion(MotionLook::Cancel),
            41 | 42 => merge(&mut plates, &mut rng),
            43 => {
                // A new profile sketch added mid-session, undone now and
                // then (the regions put by finding their sketch again).
                let mut editor = plates.doc.editor.clone();
                let x = 100.0 + 10.0 * rng.below(3) as f64;
                sketch_on(
                    &mut editor,
                    OriginPlane::XY,
                    rectangle((x, 0.0), (x + 2.0, 2.0)),
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
                        plates.motion(MotionLook::SweepRegion { sketch, region: 0 });
                    }
                }
                if rng.below(2) == 0 {
                    plates.doc.update(Edit::Undo);
                    if rng.below(2) == 0 {
                        plates.doc.update(Edit::Redo);
                    }
                }
            }
            44 => {
                // A sketch or a feature after the boxes removed, with
                // what depends on it.
                let features = plates.doc.editor.document().features();
                if features.len() > 4 {
                    let id = features[4 + rng.below(features.len() - 4)].id;
                    plates.doc.apply(Command::RemoveFeature(id));
                    plates.doc.sync();
                }
            }
            45 => {
                let ids = sweeps(&plates);
                if !ids.is_empty() {
                    let id = *rng.pick(&ids);
                    edit_and_ok(&mut plates, id, &what);
                }
            }
            46 => {
                // A sweep with a chamfer being set up.
                plates.doc.look(Look::StartChamfer);
                if rng.below(2) == 0
                    && let Some(pick) = random_edge_pick(&plates, &mut rng)
                {
                    click(&mut plates, pick);
                }
                plates.doc.look(Look::StartSweep);
            }
            47 | 48 => {
                if let Some(list) = random_overlaps(&plates, &mut rng) {
                    plates.doc.look(Look::OpenOverlaps(list));
                }
            }
            49..=51 => {
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
const QUICK_STEPS: usize = 95;

/// See the module's docs. A few steps of the first seed by default, all
/// 200 steps of each seed with `VARDE_TESTS=full` or `VARDE_TEST_SEED`.
#[test]
fn random_sweep_sessions_hold() {
    for seed in varde_testing::seeds(1, 3) {
        run(seed, crate::tests::fuzz_steps(QUICK_STEPS, 200));
    }
}
