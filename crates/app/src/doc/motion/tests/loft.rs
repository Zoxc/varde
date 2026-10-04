//! The loft session: the rail's Loft on the example plate with a square
//! on XY and the same square on the plate's top, a third square on XZ, a
//! point on its own on XZ and a rail of two lines on YZ; sections picked
//! in order, moved up and down and taken out, start dots moved, Smooth
//! and Ruled, Closed, the rails; the kernel's stand-in failing as too
//! complex in the panel and Add anyway keeping it; with regeneration
//! lofting two equal parallel squares by extruding
//! ([`varde_regen::testing`]), the preview and OK as one undo step;
//! editing from the Timeline, Cancel and undo; a section's region an
//! edit takes away said to be gone.

use glam::{DVec2, DVec3};
use varde_document::{
    BodyId, Command, CurveChain, Document, Editor, FaceKey, FaceRef, FeatureId, FeatureKind, Loft,
    LoftMode, OriginPlane, PartKey, Plane, Section,
};
use varde_sketch::{Curve, Id, Sketch};
use varde_view::{Edit, Look, MotionKind, MotionLook, MotionPick, PanelHover};

use super::Plates;
use super::chamfer::{picking, shows};
use crate::tests::{holding, key_in};

/// Adds a sketch on `plane` drawn by `draw`: its id.
fn sketch_on(editor: &mut Editor, plane: Plane, draw: impl FnOnce(&mut Sketch)) -> FeatureId {
    editor.apply(editor.document().add_sketch(plane)).unwrap();
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

/// Draws the square from `min` to `max`.
fn square(min: (f64, f64), max: (f64, f64)) -> impl FnOnce(&mut Sketch) {
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

/// The sketch `feature` of `document`.
fn sketch_of(document: &Document, feature: FeatureId) -> &Sketch {
    let FeatureKind::Sketch { sketch, .. } = &document.feature(feature).unwrap().kind else {
        panic!("a sketch");
    };
    sketch
}

/// The example plate (60 × 40 × 10 about the origin, a hole in its
/// middle), a 10 × 10 square on XY at x 15 to 25 and y −5 to 5 (`low`),
/// the same on the plate's top (`top`), a square on XZ (`side`), a point
/// on its own on XZ (`apex`) and two lines end to end on YZ (`rail`).
struct Lofted {
    plates: Plates,
    plate: BodyId,
    low: FeatureId,
    top: FeatureId,
    side: FeatureId,
    apex: FeatureId,
    rail: FeatureId,
}

fn lofted() -> Lofted {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let maker = editor.document().features()[1].id;
    let low = sketch_on(
        &mut editor,
        Plane::Origin(OriginPlane::XY),
        square((15.0, -5.0), (25.0, 5.0)),
    );
    let top_face = FaceRef {
        body: plate,
        key: FaceKey {
            feature: maker.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::new(20.0, 0.0, 10.0),
    };
    // With a point on its own in the middle, which is none of the
    // region's corners.
    let top = sketch_on(&mut editor, Plane::Face(top_face), |sketch| {
        square((15.0, -5.0), (25.0, 5.0))(sketch);
        sketch.add_point(DVec2::new(20.0, 0.0)).unwrap();
    });
    let side = sketch_on(
        &mut editor,
        Plane::Origin(OriginPlane::XZ),
        square((15.0, 20.0), (25.0, 30.0)),
    );
    let apex = sketch_on(&mut editor, Plane::Origin(OriginPlane::XZ), |sketch| {
        sketch.add_point(DVec2::new(20.0, 40.0)).unwrap();
    });
    let rail = sketch_on(&mut editor, Plane::Origin(OriginPlane::YZ), |sketch| {
        let [a, b, c] = [(-5.0, 0.0), (-5.0, 5.0), (-5.0, 10.0)]
            .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
        sketch
            .add_curve(Curve::Line { start: a, end: b }, false)
            .unwrap();
        sketch
            .add_curve(Curve::Line { start: b, end: c }, false)
            .unwrap();
    });
    let (doc, requests) = holding(editor.document().clone());
    let mut lofted = Lofted {
        plates: Plates {
            doc,
            requests,
            bodies: [plate; 3],
        },
        plate,
        low,
        top,
        side,
        apex,
        rail,
    };
    // The sketch on the top is placed once the model is in.
    lofted.plates.answer();
    lofted
}

/// The loft the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<Loft> {
    match plates.last_draft()?.1 {
        FeatureKind::Loft(loft) => Some(loft),
        _ => None,
    }
}

/// The loft being set up.
fn set_up(plates: &Plates) -> &crate::doc::motion::loft::LoftSetup {
    &plates.doc.motion.as_ref().expect("a session").loft
}

/// Picks the only region of `sketch` as a section.
fn pick(plates: &mut Plates, sketch: FeatureId) {
    plates.motion(MotionLook::LoftRegion { sketch, region: 0 });
}

/// The sketches of the sections set up, in order.
fn order(plates: &Plates) -> Vec<FeatureId> {
    set_up(plates)
        .sections
        .iter()
        .map(Section::sketch)
        .collect()
}

/// The start of section `at` set up.
fn start(plates: &Plates, at: usize) -> Option<Id> {
    match &set_up(plates).sections[at] {
        Section::Region { start, .. } => *start,
        Section::Point { .. } => panic!("a region"),
    }
}

/// The point of sketch `feature` at `at`.
fn point_at(plates: &Plates, feature: FeatureId, at: (f64, f64)) -> Id {
    let sketch = sketch_of(plates.doc.editor.document(), feature);
    (sketch.points.iter())
        .find(|point| point.at == DVec2::new(at.0, at.1))
        .expect("a point there")
        .id
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

/// The rail's Loft opens its panel, picking its sections on the sketches
/// (not the model); sections are listed in the order they're picked, each
/// starting at a corner; the preview fails as too complex (the kernel's
/// loft isn't built), shown in the panel; OK waits, and Add anyway keeps
/// it as one undo step, "Loft 1" failing in the Timeline, the sections'
/// sketches hidden.
#[test]
fn the_rail_s_loft_picks_sections_in_order_and_add_anyway_keeps_it() {
    let mut lofted = lofted();
    let (low, top, side) = (lofted.low, lofted.top, lofted.side);
    let plates = &mut lofted.plates;
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartLoft);
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Loft);
    assert!(session.bodies.is_empty(), "a loft picks no bodies");
    assert_eq!(picking(plates), MotionPick::Regions);
    assert!(
        plates.doc.model_picking().is_none(),
        "the sketches', not the model"
    );
    for text in [
        "New loft",
        "Sections",
        "Click regions or sketch points",
        "Between sections",
        "Smooth",
        "Ruled",
        "Closed",
        "Rails",
        "Click sketch curves",
        "Operation",
        "pick the sections to loft",
        "Pick sections: regions or points",
    ] {
        assert!(shows(plates, text), "{text}");
    }
    let state = plates.doc.motion_state().unwrap();
    let view = state.loft.as_ref().unwrap();
    for sketch in [low, top, side] {
        assert!(
            (view.candidates.iter()).any(|candidate| candidate.feature == sketch),
            "{sketch:?}"
        );
    }
    assert!(plates.last_draft().is_none());

    pick(plates, low);
    assert!(shows(plates, "Section 1") && shows(plates, "pick the next section"));
    pick(plates, top);
    assert_eq!(order(plates), [low, top]);
    // Each starts at a corner: the first at its first, the second at the
    // one above it.
    assert_eq!(start(plates, 0), Some(point_at(plates, low, (15.0, -5.0))));
    assert_eq!(start(plates, 1), Some(point_at(plates, top, (15.0, -5.0))));
    let loft = drafted(plates).expect("previewed");
    assert_eq!(loft.sections.len(), 2);
    assert_eq!(loft.mode, LoftMode::Smooth);
    assert!(!loft.closed && loft.rails.is_empty());
    assert!(shows(plates, "Section 2"));
    assert!(shows(plates, "2 sections · Ruled · New body"));
    assert!(plates.doc.motion_ready());

    plates.answer();
    let error = plates.doc.feed.draft_error().expect("the stand-in fails");
    assert!(error.contains("too complex"), "{error}");
    assert!(shows(plates, "Loft fails"));
    assert!(shows(plates, "Add anyway"));
    let state = plates.doc.motion_state().unwrap();
    assert!(!state.ready && state.accept);
    let features = plates.doc.editor.document().features().len();
    key_in(&mut plates.doc, super::enter());
    assert_eq!(plates.doc.editor.document().features().len(), features);

    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    assert!(matches!(kind, FeatureKind::Loft(_)));
    let document = plates.doc.editor.document();
    assert_eq!(document.feature(id).unwrap().name, "Loft 1");
    assert!(!document.feature(low).unwrap().visible);
    assert!(!document.feature(top).unwrap().visible);
    assert!(document.feature(side).unwrap().visible, "not a section's");
    plates.answer();
    assert!(
        (plates.doc.feed.failed_features().iter()).any(|failed| failed.feature == id),
        "the loft fails"
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// Sections move up and down their list and come out by their crosses
/// or a click on their region again; a point on its own is a first or
/// last section, a third refused; a point moved into the middle is
/// refused as the foot.
#[test]
fn sections_reorder_and_come_out() {
    let mut lofted = lofted();
    let (low, top, side, apex) = (lofted.low, lofted.top, lofted.side, lofted.apex);
    let plates = &mut lofted.plates;
    plates.doc.look(Look::StartLoft);
    pick(plates, low);
    pick(plates, top);
    pick(plates, side);
    assert_eq!(order(plates), [low, top, side]);
    plates.motion(MotionLook::SectionUp(2));
    assert_eq!(order(plates), [low, side, top]);
    plates.motion(MotionLook::SectionUp(1));
    assert_eq!(order(plates), [side, low, top]);
    // Past either end, nothing.
    plates.motion(MotionLook::SectionUp(0));
    plates.motion(MotionLook::SectionUp(3));
    assert_eq!(order(plates), [side, low, top]);
    let drafted_order: Vec<FeatureId> = (drafted(plates).unwrap().sections.iter())
        .map(Section::sketch)
        .collect();
    assert_eq!(drafted_order, [side, low, top]);
    assert!(shows(plates, "3 sections · Smooth · New body"));

    // The row's hover goes with it.
    plates
        .doc
        .look(Look::HoverPanel(Some(PanelHover::Section(2))));
    assert_eq!(plates.doc.panel_hover(), Some(PanelHover::Section(2)));
    plates.motion(MotionLook::DropSection(2));
    assert_eq!(plates.doc.panel_hover(), None, "its row is gone");
    assert_eq!(order(plates), [side, low]);
    // A click on a section's region takes it out.
    pick(plates, side);
    assert_eq!(order(plates), [low]);
    assert!(!plates.doc.motion_ready());

    // A point on its own: last, then a second first.
    let point = sketch_of(plates.doc.editor.document(), apex).points[0].id;
    plates.motion(MotionLook::LoftPoint {
        sketch: apex,
        point,
    });
    assert_eq!(order(plates), [low, apex]);
    assert!(matches!(set_up(plates).sections[1], Section::Point { .. }));
    // A region after a last point goes before it.
    pick(plates, top);
    assert_eq!(order(plates), [low, top, apex]);
    // Moved into the middle, the point is refused as the foot.
    plates.motion(MotionLook::SectionUp(2));
    assert_eq!(order(plates), [low, apex, top]);
    assert!(!plates.doc.motion_ready());
    assert!(
        shows(plates, "only the first or last may be"),
        "{:?}",
        crate::tests::screen_texts(&plates.doc)
    );
    plates.motion(MotionLook::SectionUp(2));
    assert!(plates.doc.motion_ready());
    // The point clicked again comes out.
    plates.motion(MotionLook::LoftPoint {
        sketch: apex,
        point,
    });
    assert_eq!(order(plates), [low, top]);
}

/// A click on another corner of a section moves its start there; a
/// point that isn't one of its corners doesn't.
#[test]
fn a_click_on_a_corner_moves_the_start() {
    let mut lofted = lofted();
    let (low, top, apex) = (lofted.low, lofted.top, lofted.apex);
    let plates = &mut lofted.plates;
    plates.doc.look(Look::StartLoft);
    pick(plates, low);
    pick(plates, top);
    let far = point_at(plates, top, (25.0, 5.0));
    plates.motion(MotionLook::LoftStart {
        section: 1,
        point: far,
    });
    assert_eq!(start(plates, 1), Some(far));
    let Section::Region {
        start: drafted_start,
        ..
    } = &drafted(plates).unwrap().sections[1]
    else {
        panic!("a region");
    };
    assert_eq!(*drafted_start, Some(far));
    // The view draws the dot there.
    let state = plates.doc.motion_state().unwrap();
    let view = state.loft.as_ref().unwrap();
    assert_eq!(view.sections[1].shape.dot(), Some(DVec2::new(25.0, 5.0)));
    match &view.sections[1].shape {
        varde_view::LoftShape::Region { corners, .. } => assert_eq!(corners.len(), 4),
        _ => panic!("a region"),
    }
    // Not a corner of it: nothing.
    let middle = point_at(plates, top, (20.0, 0.0));
    plates.motion(MotionLook::LoftStart {
        section: 1,
        point: middle,
    });
    assert_eq!(start(plates, 1), Some(far));
    // Nor on a point section.
    let apex_point = sketch_of(plates.doc.editor.document(), apex).points[0].id;
    plates.motion(MotionLook::LoftPoint {
        sketch: apex,
        point: apex_point,
    });
    plates.motion(MotionLook::LoftStart {
        section: 2,
        point: far,
    });
    assert!(matches!(
        set_up(plates).sections[2],
        Section::Point { point, .. } if point == apex_point
    ));
    plates.motion(MotionLook::DropSection(2));
    // Picked again, the second starts at the corner nearest the first's
    // start.
    pick(plates, top);
    pick(plates, top);
    assert_eq!(start(plates, 1), Some(point_at(plates, top, (15.0, -5.0))));
}

/// Smooth and Ruled, Closed (three sections; the rails field goes, its
/// rails kept for when it's off) and the rails: a click on a curve adds
/// its chain, again takes it out, a row's cross too.
#[test]
fn modes_closed_and_rails() {
    let mut lofted = lofted();
    let (low, top, side, rail) = (lofted.low, lofted.top, lofted.side, lofted.rail);
    let plates = &mut lofted.plates;
    plates.doc.look(Look::StartLoft);
    pick(plates, low);
    pick(plates, top);
    plates.motion(MotionLook::LoftMode(LoftMode::Ruled));
    assert_eq!(drafted(plates).unwrap().mode, LoftMode::Ruled);

    // Rails.
    plates.motion(MotionLook::Picking(MotionPick::Path));
    assert_eq!(picking(plates), MotionPick::Path);
    assert!(shows(plates, "Pick the rails' curves"));
    assert!(plates.doc.model_picking().is_none());
    let curves: Vec<Id> = (sketch_of(plates.doc.editor.document(), rail).curves.iter())
        .map(|entry| entry.id)
        .collect();
    plates.motion(MotionLook::LoftRail {
        sketch: rail,
        curve: curves[1],
    });
    let mut chain = curves.clone();
    chain.sort_unstable();
    assert_eq!(
        drafted(plates).unwrap().rails,
        [CurveChain {
            sketch: rail,
            curves: chain.clone(),
        }],
        "the whole chain"
    );
    assert!(shows(plates, "2 curves"));
    assert!(shows(plates, "1 rail"));
    plates.motion(MotionLook::LoftRail {
        sketch: rail,
        curve: curves[0],
    });
    assert!(drafted(plates).unwrap().rails.is_empty());
    plates.motion(MotionLook::LoftRail {
        sketch: rail,
        curve: curves[0],
    });
    plates.motion(MotionLook::DropRail(0));
    assert!(drafted(plates).unwrap().rails.is_empty());
    plates.motion(MotionLook::LoftRail {
        sketch: rail,
        curve: curves[0],
    });

    // Closed with two sections is refused as the foot; with three it's
    // whole, its rails left out and their field gone.
    plates.motion(MotionLook::Closed);
    assert!(!plates.doc.motion_ready());
    assert!(shows(plates, "fewer than 3"));
    assert!(!shows(plates, "Rails"));
    plates.motion(MotionLook::Picking(MotionPick::Regions));
    pick(plates, side);
    let loft = drafted(plates).unwrap();
    assert!(loft.closed && loft.rails.is_empty());
    assert!(plates.doc.motion_ready());
    // No rails picked while closed.
    plates.motion(MotionLook::Picking(MotionPick::Path));
    assert_eq!(picking(plates), MotionPick::Regions);
    // Off again, the rail is back.
    plates.motion(MotionLook::Closed);
    assert_eq!(drafted(plates).unwrap().rails.len(), 1);
    assert!(shows(plates, "Rails"));
}

/// With regeneration lofting two equal parallel squares by extruding: the
/// square on XY to the one on the plate's top previews a 10 × 10 × 10
/// box, OK adds it as one undo step.
#[test]
fn two_squares_preview_and_ok_adds_one_undo_step() {
    varde_regen::testing::loft_by_extrude();
    let mut lofted = lofted();
    let (plate, low, top) = (lofted.plate, lofted.low, lofted.top);
    let plates = &mut lofted.plates;
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartLoft);
    pick(plates, low);
    pick(plates, top);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(boxed(plates, plate, [15.0, -5.0, 0.0], [25.0, 5.0, 10.0]));
    key_in(&mut plates.doc, super::enter());
    assert!(plates.doc.motion.is_none());
    let (_, kind) = plates.last_feature();
    assert!(matches!(kind, FeatureKind::Loft(_)));
    plates.answer();
    assert!(plates.doc.feed.failed_features().is_empty());
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// Editing a loft from the Timeline opens it with its sections, starts
/// and options (its sections' sketches hidden, their regions still
/// there); OK on it unchanged writes nothing; Cancel leaves no trace;
/// Ruled and OK sets it as one undo step.
#[test]
fn editing_a_loft_from_the_timeline_cancel_and_undo() {
    let mut lofted = lofted();
    let (low, top) = (lofted.low, lofted.top);
    let plates = &mut lofted.plates;
    plates.doc.look(Look::StartLoft);
    pick(plates, low);
    pick(plates, top);
    let far = point_at(plates, top, (25.0, 5.0));
    plates.motion(MotionLook::LoftStart {
        section: 1,
        point: far,
    });
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    let (id, FeatureKind::Loft(stored)) = plates.last_feature() else {
        panic!("a loft");
    };
    plates.answer();
    let before = plates.doc.editor.document().clone();

    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(
        (session.kind, session.feature),
        (MotionKind::Loft, Some(id))
    );
    assert_eq!(session.loft.sections, stored.sections);
    assert_eq!(picking(plates), MotionPick::Nothing);
    assert!(shows(plates, "Loft 1") && shows(plates, "Section 2"));
    assert!(!shows(plates, "gone"));
    let opened = drafted(plates).unwrap();
    assert_eq!(
        (&opened.sections, opened.mode, opened.closed, &opened.rails),
        (&stored.sections, stored.mode, stored.closed, &stored.rails),
        "as stored"
    );
    let state = plates.doc.motion_state().unwrap();
    let view = state.loft.as_ref().unwrap();
    assert!(view.sections.iter().all(|section| !section.gone));
    assert_eq!(view.sections[1].shape.dot(), Some(DVec2::new(25.0, 5.0)));
    plates.motion(MotionLook::Cancel);
    assert!(plates.doc.motion.is_none());
    assert_eq!(*plates.doc.editor.document(), before);

    plates.doc.look(Look::EditFeature(id));
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    assert_eq!(*plates.doc.editor.document(), before, "nothing changed");

    plates.doc.look(Look::EditFeature(id));
    plates.motion(MotionLook::LoftMode(LoftMode::Ruled));
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    let FeatureKind::Loft(edited) = &plates.doc.editor.document().feature(id).unwrap().kind else {
        panic!("a loft");
    };
    assert_eq!(edited.mode, LoftMode::Ruled);
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// A section's region an edit takes away is said to be gone, nothing
/// previewed or committed until it's taken out or back; an undo brings
/// it back.
#[test]
fn a_section_s_region_gone_is_said_to_be_gone() {
    let mut lofted = lofted();
    let (low, top, side) = (lofted.low, lofted.top, lofted.side);
    let plates = &mut lofted.plates;
    plates.doc.look(Look::StartLoft);
    pick(plates, low);
    pick(plates, top);
    pick(plates, side);
    assert!(plates.doc.motion_ready());
    plates.doc.apply(Command::SetSketch {
        feature: side,
        sketch: Box::new(Sketch::default()),
    });
    plates.doc.sync();
    plates.answer();
    assert!(shows(plates, "A section's sketch, region or point is gone"));
    assert!(shows(plates, "gone"));
    assert!(!plates.doc.motion_ready());
    assert!(plates.doc.motion_draft().is_none());
    plates.doc.update(Edit::Undo);
    plates.doc.sync();
    plates.answer();
    assert!(!shows(plates, "is gone"));
    assert!(plates.doc.motion_ready());
    // Gone again, and taken out: whole.
    plates.doc.apply(Command::SetSketch {
        feature: side,
        sketch: Box::new(Sketch::default()),
    });
    plates.doc.sync();
    plates.motion(MotionLook::DropSection(2));
    assert!(!shows(plates, "is gone"));
    assert!(plates.doc.motion_ready());
}

/// Adds a sketch on XY with a square from `min` to `max` and a circle
/// inside it, after the others: its id.
fn add_holed(plates: &mut Plates, min: (f64, f64), max: (f64, f64)) -> FeatureId {
    let document = plates.doc.editor.document();
    plates
        .doc
        .apply(document.add_sketch(Plane::Origin(OriginPlane::XY)));
    let feature = plates.doc.editor.document().features().last().unwrap().id;
    let mut sketch = Sketch::default();
    square(min, max)(&mut sketch);
    let center = sketch
        .add_point(DVec2::new((min.0 + max.0) / 2.0, (min.1 + max.1) / 2.0))
        .unwrap();
    let radius = (max.0 - min.0) / 4.0;
    sketch
        .add_curve(Curve::Circle { center, radius }, false)
        .unwrap();
    plates.doc.apply(Command::SetSketch {
        feature,
        sketch: Box::new(sketch),
    });
    plates.doc.sync();
    plates.answer();
    feature
}

/// A region with holes is no section, and a sketch after the edited
/// loft holds none.
#[test]
fn regions_with_holes_and_later_sketches_are_refused() {
    let mut lofted = lofted();
    let (low, top) = (lofted.low, lofted.top);
    let plates = &mut lofted.plates;
    let holed_sketch = add_holed(plates, (40.0, 40.0), (60.0, 60.0));
    plates.doc.look(Look::StartLoft);
    let found = &set_up(plates).regions;
    let profiles = &found.found(holed_sketch).expect("a candidate").profiles;
    let holed = (profiles.regions.iter())
        .position(|region| !region.holes.is_empty())
        .expect("the square less its circle");
    plates.motion(MotionLook::LoftRegion {
        sketch: holed_sketch,
        region: holed,
    });
    assert!(set_up(plates).sections.is_empty());
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("A section is one loop: pick a region without holes")
    );

    // A loft, then a sketch after it: editing the loft, it holds no
    // section.
    pick(plates, low);
    pick(plates, top);
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    let (id, _) = plates.last_feature();
    let later = add_holed(plates, (70.0, 70.0), (90.0, 90.0));
    plates.doc.look(Look::EditFeature(id));
    let state = plates.doc.motion_state().unwrap();
    let view = state.loft.as_ref().unwrap();
    assert!(!(view.candidates.iter()).any(|candidate| candidate.feature == later));
    plates.motion(MotionLook::Picking(MotionPick::Regions));
    plates.motion(MotionLook::LoftRegion {
        sketch: later,
        region: 0,
    });
    assert_eq!(set_up(plates).sections.len(), 2);
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a sketch made before the loft can hold its sections")
    );
}
