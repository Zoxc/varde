//! The sweep session: the rail's Sweep on the example plate with a
//! square to sweep on XY and a path up Z on XZ; the profile's regions
//! picked on its sketch, the path's parts by clicking a path sketch's
//! curves (a chain a part) and the model's edges, a helix about an origin
//! axis with its pitch, turns, Left-handed and Flip; Keep orientation,
//! the twist and the operation drafted; the kernel's stand-in failing as
//! too complex in the panel and Add anyway keeping it; with regeneration
//! sweeping straight paths by extruding ([`varde_regen::testing`]), the
//! preview and OK as one undo step; editing from the Timeline, Cancel and
//! undo; a path an edit takes away said to be gone.

use glam::{DVec2, DVec3};
use varde_document::{
    Axis3, AxisRef, BodyId, Command, Document, Editor, FeatureId, FeatureKind, Orientation,
    OriginPlane, PathPart, PathRef, Plane, Sweep,
};
use varde_regen::Summary;
use varde_sketch::{Curve, Id, Sketch};
use varde_view::{
    Edit, Look, MotionField, MotionKind, MotionLook, MotionPick, OperationKind, PanelHover, Picked,
    SweepPath,
};

use super::Plates;
use super::chamfer::{click_edge, picking, shows};
use crate::tests::{holding, key_in};

/// The plate's upright edge at its back right corner, from z 0 to 10.
const CORNER: ([f64; 3], [f64; 3]) = ([30.0, 20.0, 0.0], [30.0, 20.0, 10.0]);

/// Adds a sketch on `plane` drawn by `draw`: its id.
fn sketch_on(editor: &mut Editor, plane: OriginPlane, draw: impl FnOnce(&mut Sketch)) -> FeatureId {
    editor
        .apply(editor.document().add_sketch(Plane::Origin(plane)))
        .unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = Sketch::default();
    draw(&mut sketch);
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    feature
}

/// Draws the rectangle from `min` to `max`.
fn rectangle(min: (f64, f64), max: (f64, f64)) -> impl FnOnce(&mut Sketch) {
    move |sketch| {
        let corners = [
            (min.0, min.1),
            (max.0, min.1),
            (max.0, max.1),
            (min.0, max.1),
        ]
        .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
        for (k, &start) in corners.iter().enumerate() {
            let end = corners[(k + 1) % corners.len()];
            sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        }
    }
}

/// Draws lines through `points` in turn, an open polyline.
fn polyline(points: Vec<(f64, f64)>) -> impl FnOnce(&mut Sketch) {
    move |sketch| {
        let ids: Vec<Id> = (points.iter())
            .map(|&(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap())
            .collect();
        for pair in ids.windows(2) {
            let line = Curve::Line {
                start: pair[0],
                end: pair[1],
            };
            sketch.add_curve(line, false).unwrap();
        }
    }
}

/// The curves of sketch `feature` of `document`, sorted.
fn curves_of(document: &Document, feature: FeatureId) -> Vec<Id> {
    let FeatureKind::Sketch { sketch, .. } = &document.feature(feature).unwrap().kind else {
        panic!("a sketch");
    };
    let mut ids: Vec<Id> = sketch.curves.iter().map(|entry| entry.id).collect();
    ids.sort_unstable();
    ids
}

/// The example plate, a 10 × 10 square on XY at x 15 to 25 and y −5 to
/// 5 (clear of the plate's hole), a path up z 30 from its middle on XZ
/// in two lines (a chain), and a square on XY at the plate's back right
/// corner: the plates held, the plate, the profile's sketch, the path's
/// and the corner's.
struct Swept {
    plates: Plates,
    plate: BodyId,
    profile: FeatureId,
    path: FeatureId,
    corner: FeatureId,
}

fn swept() -> Swept {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let profile = sketch_on(
        &mut editor,
        OriginPlane::XY,
        rectangle((15.0, -5.0), (25.0, 5.0)),
    );
    let path = sketch_on(
        &mut editor,
        OriginPlane::XZ,
        polyline(vec![(20.0, 0.0), (20.0, 12.0), (20.0, 30.0)]),
    );
    let corner = sketch_on(
        &mut editor,
        OriginPlane::XY,
        rectangle((26.0, 16.0), (30.0, 20.0)),
    );
    let (doc, requests) = holding(editor.document().clone());
    Swept {
        plates: Plates {
            doc,
            requests,
            bodies: [plate; 3],
        },
        plate,
        profile,
        path,
        corner,
    }
}

/// The sweep the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<Sweep> {
    match plates.last_draft()?.1 {
        FeatureKind::Sweep(sweep) => Some(sweep),
        _ => None,
    }
}

/// The path's parts the last request previews.
fn parts(plates: &Plates) -> Vec<PathPart> {
    match drafted(plates).expect("a sweep's draft").path {
        PathRef::Chain(parts) => parts,
        PathRef::Helix(_) => panic!("a chain"),
    }
}

/// The first curve of sketch `feature` in the document shown.
fn first_curve(plates: &Plates, feature: FeatureId) -> Id {
    curves_of(plates.doc.editor.document(), feature)[0]
}

/// Starts a sweep, picks the square's region and the path's first
/// line's chain.
fn set_up(swept: &mut Swept) {
    let (profile, path) = (swept.profile, swept.path);
    let plates = &mut swept.plates;
    plates.doc.look(Look::StartSweep);
    plates.motion(MotionLook::SweepRegion {
        sketch: profile,
        region: 0,
    });
    let curve = first_curve(plates, path);
    plates.motion(MotionLook::SweepCurve {
        sketch: path,
        curve,
    });
}

/// Whether the model shown has a body other than `plate` whose box is
/// `low` to `high`.
fn boxed(plates: &Plates, plate: BodyId, low: [f64; 3], high: [f64; 3]) -> bool {
    let index = plates.doc.feed.pick_index();
    (plates.doc.feed.parts().iter())
        .filter(|&&body| body != plate)
        .filter_map(|&body| index.bodies_bounds(&[body]))
        .any(|[l, h]| l.distance(DVec3::from(low)) < 1e-6 && h.distance(DVec3::from(high)) < 1e-6)
}

/// The rail's Sweep: the profile's regions picked first on its sketches
/// (not the model), then the path; a click on a curve of the path's
/// sketch adds the chain it's in as one part, listed by its sketch; the
/// preview fails as too complex (the kernel's sweep isn't built), shown
/// in the panel; OK waits, and Add anyway keeps it as one undo step,
/// "Sweep 1" failing in the Timeline, its profile's sketch hidden and
/// its path's not.
#[test]
fn the_rail_s_sweep_picks_a_profile_and_a_path_and_add_anyway_keeps_it() {
    let mut swept = swept();
    let (profile, path) = (swept.profile, swept.path);
    let plates = &mut swept.plates;
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartSweep);
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Sweep);
    assert!(session.bodies.is_empty(), "a sweep picks no bodies");
    assert_eq!(picking(plates), MotionPick::Regions);
    assert!(
        plates.doc.model_picking().is_none(),
        "the sketches', not the model"
    );
    for text in [
        "New sweep",
        "Profile",
        "Click regions",
        "Along",
        "Path",
        "Helix",
        "Click sketch curves or edges",
        "Tangent chain",
        "Keep orientation",
        "Twist",
        "Operation",
        "pick the regions to sweep",
    ] {
        assert!(shows(plates, text), "{text}");
    }
    for text in ["Pitch", "Turns", "Left-handed"] {
        assert!(!shows(plates, text), "{text}");
    }
    let state = plates.doc.motion_state().unwrap();
    let view = state.sweep.as_ref().unwrap();
    assert!((view.candidates.iter()).any(|candidate| candidate.feature == profile));
    assert!(!(view.candidates.iter()).any(|candidate| candidate.feature == path));
    assert!(plates.last_draft().is_none());

    plates.motion(MotionLook::SweepRegion {
        sketch: profile,
        region: 0,
    });
    assert_eq!(picking(plates), MotionPick::Path, "the path next");
    assert!(shows(plates, "Region 1"));
    assert!(shows(plates, "pick the path"));
    let state = plates.doc.motion_state().unwrap();
    let view = state.sweep.as_ref().unwrap();
    assert!((view.lines.iter()).any(|lines| lines.feature == path));
    assert!(!(view.lines.iter()).any(|lines| lines.feature == profile));
    // The profile's own sketch is no path.
    let own = first_curve(plates, profile);
    plates.motion(MotionLook::SweepCurve {
        sketch: profile,
        curve: own,
    });
    assert!(
        (plates.doc.notice.as_deref()).is_some_and(|notice| notice.contains("own sketch")),
        "{:?}",
        plates.doc.notice
    );
    let curve = first_curve(plates, path);
    plates.motion(MotionLook::SweepCurve {
        sketch: path,
        curve,
    });
    let chain = curves_of(plates.doc.editor.document(), path);
    assert_eq!(chain.len(), 2);
    let drafted_parts = parts(plates);
    let [PathPart::Curves(part)] = drafted_parts.as_slice() else {
        panic!("one part: {drafted_parts:?}");
    };
    assert_eq!(
        (part.sketch, &part.curves),
        (path, &chain),
        "the whole chain"
    );
    let name = plates
        .doc
        .editor
        .document()
        .feature(path)
        .unwrap()
        .name
        .clone();
    assert!(shows(plates, &name) && shows(plates, "2 curves"));
    assert!(plates.doc.motion_ready());
    let sweep = drafted(plates).unwrap();
    assert_eq!(sweep.orientation, Orientation::FollowPath);
    assert_eq!(sweep.twist, None, "no twist of nothing stored");
    assert!(shows(
        plates,
        &format!("Along {name} · Follow path · New body")
    ));

    plates.answer();
    let error = plates.doc.feed.draft_error().expect("the stand-in fails");
    assert!(error.contains("too complex"), "{error}");
    assert!(shows(plates, "Sweep fails"));
    assert!(shows(plates, "Add anyway"));
    let state = plates.doc.motion_state().unwrap();
    assert!(!state.ready && state.accept);
    let features = plates.doc.editor.document().features().len();
    key_in(&mut plates.doc, super::enter());
    assert_eq!(plates.doc.editor.document().features().len(), features);

    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    assert!(matches!(kind, FeatureKind::Sweep(_)));
    let document = plates.doc.editor.document();
    assert_eq!(document.feature(id).unwrap().name, "Sweep 1");
    assert!(
        !document.feature(profile).unwrap().visible,
        "profile hidden"
    );
    assert!(document.feature(path).unwrap().visible, "path kept shown");
    plates.answer();
    assert!(
        (plates.doc.feed.failed_features().iter()).any(|failed| failed.feature == id),
        "the sweep fails"
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// With regeneration sweeping straight paths by extruding: the square
/// swept up the path previews a 10 × 10 × 30 box, OK adds it as one undo
/// step; a click on the path's curve again takes its part out; a row's
/// cross too.
#[test]
fn a_straight_path_previews_and_ok_adds_one_undo_step() {
    varde_regen::testing::sweep_by_extrude();
    let mut swept = swept();
    set_up(&mut swept);
    let (plate, path) = (swept.plate, swept.path);
    let plates = &mut swept.plates;
    let before = plates.doc.editor.document().clone();
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(boxed(plates, plate, [15.0, -5.0, 0.0], [25.0, 5.0, 30.0]));

    // A curve of the part clicked again takes it out; clicked back in.
    let last = *curves_of(plates.doc.editor.document(), path)
        .last()
        .unwrap();
    plates.motion(MotionLook::SweepCurve {
        sketch: path,
        curve: last,
    });
    assert!(plates.doc.motion.as_ref().unwrap().sweep.chains.is_empty());
    assert!(!plates.doc.motion_ready());
    assert!(shows(plates, "pick the path"));
    plates.motion(MotionLook::SweepCurve {
        sketch: path,
        curve: last,
    });
    plates.doc.look(Look::HoverPanel(Some(PanelHover::Part(0))));
    assert_eq!(plates.doc.panel_hover(), Some(PanelHover::Part(0)));
    plates.motion(MotionLook::DropPart(0));
    assert_eq!(plates.doc.panel_hover(), None, "its row is gone");
    plates.motion(MotionLook::SweepCurve {
        sketch: path,
        curve: last,
    });
    plates.answer();
    assert!(boxed(plates, plate, [15.0, -5.0, 0.0], [25.0, 5.0, 30.0]));

    key_in(&mut plates.doc, super::enter());
    assert!(plates.doc.motion.is_none());
    let (_, kind) = plates.last_feature();
    assert!(matches!(kind, FeatureKind::Sweep(_)));
    plates.answer();
    assert!(plates.doc.feed.failed_features().is_empty());
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// A model edge picked for the path is listed as an edge and stored as
/// one part of edges with the Tangent chain tick, beside the sketch's
/// chain; a face is refused; with the stand-in, the corner's square
/// swept up the plate's upright corner edge alone previews its box.
#[test]
fn model_edges_are_path_parts_beside_sketch_chains() {
    varde_regen::testing::sweep_by_extrude();
    let mut swept = swept();
    set_up(&mut swept);
    let (plate, corner) = (swept.plate, swept.corner);
    let plates = &mut swept.plates;
    plates.answer();
    let picks = plates
        .doc
        .model_picking()
        .expect("the model's edges picked");
    assert_eq!(picks.picks, varde_view::Picks::Edges);
    let top = plates.face(
        plate,
        |s| matches!(s, Summary::Plane { n, .. } if n[2] > 0.5),
    );
    plates.click_at(plate, Picked::Face(top), DVec3::new(0.0, 15.0, 10.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only an edge or a sketch's curve can be on the path")
    );
    click_edge(plates, plate, CORNER);
    assert!(shows(plates, "Edge 1") && shows(plates, "10 mm"));
    let all = parts(plates);
    assert_eq!(all.len(), 2);
    assert!(matches!(&all[1], PathPart::Edges { edges, tangent: true } if edges.len() == 1));
    plates.motion(MotionLook::Chain);
    assert!(matches!(
        &parts(plates)[1],
        PathPart::Edges { tangent: false, .. }
    ));
    // The edge alone, with the corner's square.
    plates.motion(MotionLook::DropPart(0));
    let [PathPart::Edges { .. }] = parts(plates).as_slice() else {
        panic!("the edge alone: {:?}", parts(plates));
    };
    let profile = swept.profile;
    let plates = &mut swept.plates;
    plates.motion(MotionLook::Picking(MotionPick::Regions));
    plates.motion(MotionLook::SweepRegion {
        sketch: profile,
        region: 0,
    });
    assert_eq!(
        plates.doc.motion_state().unwrap().sweep.unwrap().source,
        None
    );
    plates.motion(MotionLook::SweepRegion {
        sketch: corner,
        region: 0,
    });
    assert_eq!(drafted(plates).unwrap().sketch, corner);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(boxed(plates, plate, [26.0, 16.0, 0.0], [30.0, 20.0, 10.0]));
    let edge = (plates.doc.motion.as_ref().unwrap().blend.edges.refs)[0];
    plates.motion(MotionLook::DropEdge(edge));
    assert!(!plates.doc.motion_ready(), "no path left");
}

/// A helix: its tiles show its axis, pitch, turns, Left-handed and Flip
/// and hide the path's orientation and twist; the toolbar's Z axis picks
/// its axis; its values, hand and flip are drafted, never with Keep
/// orientation or a twist, which a path keeps for when it's back.
#[test]
fn a_helix_is_set_up_about_an_origin_axis() {
    let mut swept = swept();
    let profile = swept.profile;
    let plates = &mut swept.plates;
    plates.doc.look(Look::StartSweep);
    plates.motion(MotionLook::KeepOrientation);
    plates.input(MotionField::Twist, "90");
    plates.motion(MotionLook::SweepPath(SweepPath::Helix));
    for text in [
        "Axis",
        "Click an axis or edge",
        "Pitch",
        "Turns",
        "Left-handed",
        "Flip",
    ] {
        assert!(shows(plates, text), "{text}");
    }
    for text in ["Keep orientation", "Twist", "Tangent chain"] {
        assert!(!shows(plates, text), "{text}");
    }
    assert_eq!(picking(plates), MotionPick::Regions, "the profile first");
    plates.motion(MotionLook::SweepRegion {
        sketch: profile,
        region: 0,
    });
    assert_eq!(picking(plates), MotionPick::Reference, "then the axis");
    assert!(shows(plates, "pick the helix's axis"));
    assert!(shows(plates, "Z"), "the toolbar's axes");
    plates.motion(MotionLook::OriginAxis(Axis3::Z));
    assert_eq!(picking(plates), MotionPick::Nothing);
    assert!(shows(plates, "Z axis"));
    let PathRef::Helix(helix) = drafted(plates).unwrap().path else {
        panic!("a helix");
    };
    assert_eq!(helix.axis, AxisRef::Origin(Axis3::Z));
    assert_eq!((helix.pitch.value, helix.turns.value), (10.0, 5.0));
    assert!(!helix.left_handed && !helix.flip);
    let sweep = drafted(plates).unwrap();
    assert_eq!(sweep.orientation, Orientation::FollowPath);
    assert_eq!(sweep.twist, None);
    plates.input(MotionField::Pitch, "4");
    plates.input(MotionField::Turns, "2.5");
    plates.motion(MotionLook::LeftHanded);
    plates.motion(MotionLook::Flip);
    let PathRef::Helix(helix) = drafted(plates).unwrap().path else {
        panic!("a helix");
    };
    assert_eq!((helix.pitch.value, helix.turns.value), (4.0, 2.5));
    assert!(helix.left_handed && helix.flip);
    let line = plates
        .doc
        .motion_state()
        .unwrap()
        .line
        .expect("the axis drawn");
    assert_eq!(line[1], DVec3::NEG_Z, "flipped");
    // Turns past the limit refused under the field.
    plates.input(MotionField::Turns, "5000");
    assert!(!plates.doc.motion_ready());
    plates.input(MotionField::Turns, "3");
    assert!(plates.doc.motion_ready());
    // Back to a path: its options as they were.
    plates.motion(MotionLook::SweepPath(SweepPath::Path));
    assert!(plates.doc.motion.as_ref().unwrap().sweep.keep_orientation);
    assert!(!plates.doc.motion_ready(), "no path yet");
}

/// Keep orientation, a twist and the operation are drafted; a twist of
/// nothing is left out, one past 8 turns refused.
#[test]
fn the_path_s_options_and_the_operation_are_drafted() {
    let mut swept = swept();
    set_up(&mut swept);
    let plates = &mut swept.plates;
    plates.motion(MotionLook::KeepOrientation);
    assert_eq!(drafted(plates).unwrap().orientation, Orientation::Keep);
    assert!(shows(plates, "Keep orientation · New body"));
    plates.input(MotionField::Twist, "90");
    let twist = drafted(plates).unwrap().twist.expect("a twist");
    assert!((twist.value - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!(shows(plates, "Twist 90°"));
    plates.input(MotionField::Twist, "3000");
    assert!(!plates.doc.motion_ready());
    plates.input(MotionField::Twist, "0");
    assert_eq!(drafted(plates).unwrap().twist, None);
    plates.motion(MotionLook::Operation(OperationKind::Cut));
    assert!(matches!(
        drafted(plates).unwrap().operation,
        varde_document::Operation::Cut(_)
    ));
    assert!(shows(plates, "Cut"));
}

/// Editing a sweep from the Timeline opens it with its profile, path
/// and options; Cancel leaves no trace; another twist and OK sets it as
/// one undo step.
#[test]
fn editing_a_sweep_from_the_timeline_cancel_and_undo() {
    let mut swept = swept();
    set_up(&mut swept);
    let (profile, path) = (swept.profile, swept.path);
    let plates = &mut swept.plates;
    plates.motion(MotionLook::KeepOrientation);
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    let (id, FeatureKind::Sweep(stored)) = plates.last_feature() else {
        panic!("a sweep");
    };
    plates.answer();
    let before = plates.doc.editor.document().clone();

    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(
        (session.kind, session.feature),
        (MotionKind::Sweep, Some(id))
    );
    assert_eq!(session.sweep.regions.source, Some(profile));
    assert_eq!(session.sweep.chains.len(), 1);
    assert_eq!(session.sweep.chains[0].sketch, path);
    assert!(session.sweep.keep_orientation);
    assert!(shows(plates, "Sweep 1") && shows(plates, "Region 1"));
    let opened = drafted(plates).expect("previewed");
    assert_eq!(
        (
            &opened.regions,
            &opened.path,
            opened.orientation,
            &opened.twist
        ),
        (
            &stored.regions,
            &stored.path,
            stored.orientation,
            &stored.twist
        ),
        "as stored"
    );
    plates.motion(MotionLook::Cancel);
    assert!(plates.doc.motion.is_none());
    assert_eq!(*plates.doc.editor.document(), before);

    plates.doc.look(Look::EditFeature(id));
    plates.input(MotionField::Twist, "45");
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let FeatureKind::Sweep(edited) = &plates.doc.editor.document().feature(id).unwrap().kind else {
        panic!("a sweep");
    };
    assert!(edited.twist.is_some());
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// A path's curves an edit takes away are said to be gone, nothing
/// previewed or committed until its part is taken out; the profile's
/// sketch deleted puts its regions by, said to be gone, and an undo
/// brings them back.
#[test]
fn a_path_an_edit_takes_away_is_said_to_be_gone() {
    let mut swept = swept();
    set_up(&mut swept);
    let (profile, path) = (swept.profile, swept.path);
    let plates = &mut swept.plates;
    assert!(plates.doc.motion_ready());
    plates.doc.apply(Command::SetSketch {
        feature: path,
        sketch: Box::new(Sketch::default()),
    });
    plates.doc.sync();
    plates.answer();
    assert!(shows(plates, "A path's sketch or curve is gone"));
    assert!(!plates.doc.motion_ready());
    plates.doc.update(Edit::Undo);
    plates.doc.sync();
    plates.answer();
    assert!(!shows(plates, "is gone"));
    assert!(plates.doc.motion_ready());
    plates.motion(MotionLook::DropPart(0));
    assert!(!plates.doc.motion_ready());
    let curve = first_curve(plates, path);
    plates.motion(MotionLook::SweepCurve {
        sketch: path,
        curve,
    });
    assert!(plates.doc.motion_ready());
    // The profile's sketch removed: its regions are put by, said to be
    // gone, nothing previewed meanwhile.
    plates.doc.apply(Command::RemoveFeature(profile));
    plates.doc.sync();
    plates.answer();
    let session = plates.doc.motion.as_ref().expect("still set up");
    assert_eq!(session.sweep.regions.source, None);
    assert!(session.sweep.regions.picked.is_empty());
    assert!(shows(plates, "The profile's sketch is gone"));
    assert!(!plates.doc.motion_ready());
    assert!(plates.doc.motion_draft().is_none());
    // Back by an undo, before others are picked: picked again.
    plates.doc.update(Edit::Undo);
    plates.doc.sync();
    plates.answer();
    let session = plates.doc.motion.as_ref().expect("still set up");
    assert_eq!(session.sweep.regions.source, Some(profile));
    assert_eq!(session.sweep.regions.picked.len(), 1);
    assert!(!shows(plates, "is gone"));
    assert!(plates.doc.motion_ready());
}

/// A check's words are said of the feature: "it" before a verb, but not
/// before "its" or "a".
#[test]
fn refusals_read_as_sentences() {
    use crate::doc::motion::said;
    assert_eq!(said("sweeps 0 regions"), "it sweeps 0 regions");
    assert_eq!(
        said("its path runs along feature 3"),
        "its path runs along feature 3"
    );
    assert_eq!(
        said("a face it opens is on another body"),
        "a face it opens is on another body"
    );
}
