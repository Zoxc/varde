//! Project and Intersect picking outside the sketch being edited.

use glam::DVec2;
use varde_document::Document;
use varde_document::{Command, FeatureId, FeatureKind, OriginPlane, OutsideRef, Plane};
use varde_render::{Camera, View};
use varde_sketch::{Curve, Id, Sketch};
use varde_view::{Look, Pick, Picked, Picks, SketchItem, Tool};

use std::cell::RefCell;
use std::rc::Rc;

use varde_regen::Request;
use varde_sketch::{LinkKind, LinkShape};
use varde_view::Edit;

use super::Answered;
use crate::doc::Doc;
use crate::tests::{SolveLane, answer, example};

/// The viewport the picks are worked out in, in logical pixels.
const SIZE: [f32; 2] = [400.0, 300.0];

/// From the top, the example's plate filling the view: 5 pixels a
/// millimetre, the origin in the middle.
fn top() -> Camera {
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    camera.set_target(glam::Vec3::new(0.0, 0.0, 5.0));
    camera.zoom(60.0 / camera.view_height());
    camera
}

/// What the cursor picks of the model at the world point (`x`, `y`)
/// from the top.
fn pick(doc: &Doc, x: f64, y: f64) -> Pick {
    let at = DVec2::new(200.0 + 5.0 * x, 150.0 - 5.0 * y);
    let index = doc.feed.pick_index();
    index.pick(&top(), SIZE, at, Picks::All).unwrap()
}

/// Adds a sketch on XY holding a line along y = `y`, off the plate: the
/// sketch and the line.
fn add_line_sketch(doc: &mut Doc, y: f64) -> (FeatureId, Id) {
    doc.apply(
        doc.editor
            .document()
            .add_sketch(Plane::Origin(OriginPlane::XY)),
    );
    let feature = doc.editor.document().features().last().unwrap().id;
    let mut sketch = Sketch::default();
    let a = sketch.add_point(DVec2::new(-25.0, y)).unwrap();
    let b = sketch.add_point(DVec2::new(25.0, y)).unwrap();
    let line = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    doc.apply(Command::SetSketch {
        feature,
        sketch: Box::new(sketch),
    });
    (feature, line)
}

/// The example, then a sketch with a line along y = 30 (`before`), an
/// empty sketch, edited with `tool`, and a sketch with a line along
/// y = -30 (`after`).
struct Setup {
    doc: Answered,
    before: (FeatureId, Id),
    after: (FeatureId, Id),
    requests: Rc<RefCell<Vec<Request>>>,
}

fn setup(tool: Tool) -> Setup {
    let (mut doc, requests) = example();
    let before = add_line_sketch(&mut doc, 30.0);
    doc.apply(
        doc.editor
            .document()
            .add_sketch(Plane::Origin(OriginPlane::XY)),
    );
    let edited = doc.editor.document().features().last().unwrap().id;
    let after = add_line_sketch(&mut doc, -30.0);
    doc.sync();
    answer(&mut doc, &requests);
    let lane = SolveLane::connect(&mut doc);
    let mut doc = Answered { doc, lane };
    doc.look(Look::EditFeature(edited));
    answer(&mut doc, &requests);
    doc.look(Look::SelectTool(tool));
    Setup {
        doc,
        before,
        after,
        requests,
    }
}

/// What the links of the sketch being edited come from, in order: what
/// the tools have picked outside it.
fn outside(doc: &Doc) -> Vec<OutsideRef> {
    let Some(feature) = doc.sketch.as_ref().map(|session| session.feature) else {
        return Vec::new();
    };
    match &doc.editor.document().feature(feature).unwrap().kind {
        FeatureKind::Sketch { sources, .. } => sources.iter().map(|from| from.source).collect(),
        _ => Vec::new(),
    }
}

fn item((sketch, item): (FeatureId, Id)) -> SketchItem {
    SketchItem { sketch, item }
}

#[test]
fn project_picks_earlier_sketches_and_edges_and_refuses_the_rest() {
    let Setup {
        mut doc,
        before,
        after,
        ..
    } = setup(Tool::Project);
    let edited = doc.sketch.as_ref().unwrap().feature;
    assert!(doc.picks());
    let picking = doc.model_picking().unwrap();
    assert_eq!(picking.picks, Picks::All);
    let offered: Vec<FeatureId> = picking.sketches.iter().map(|lines| lines.feature).collect();
    assert!(offered.contains(&before.0) && offered.contains(&after.0));
    assert!(!offered.contains(&edited), "its own geometry isn't picked");
    // An earlier sketch's line.
    doc.look(Look::ClickSketch {
        item: item(before),
        add: false,
    });
    let wanted = OutsideRef::Sketch {
        sketch: before.0,
        item: before.1,
    };
    assert_eq!(outside(&doc), [wanted]);
    assert_eq!(doc.model_picking().unwrap().marked, [item(before)]);
    // The plate's top edge along y = 20, lit.
    let edge = pick(&doc, 0.0, 20.0);
    assert!(matches!(edge.target, Picked::Edge(_)), "{edge:?}");
    doc.look(Look::ClickModel {
        pick: Some(edge),
        add: false,
        double: false,
    });
    assert!(matches!(outside(&doc)[..], [_, OutsideRef::Edge(_)]));
    let highlight = doc.highlight().unwrap();
    assert!(highlight.highlights.selected_edges.len() == 1);
    // Clicked again elsewhere on it, it's taken out.
    let again = pick(&doc, 10.0, 20.0);
    doc.look(Look::ClickModel {
        pick: Some(again),
        add: false,
        double: false,
    });
    assert_eq!(outside(&doc), [wanted]);
    // A later sketch's line, and a face, are refused, saying why.
    doc.look(Look::ClickSketch {
        item: item(after),
        add: false,
    });
    assert_eq!(outside(&doc), [wanted]);
    let name = doc.editor.document().feature(edited).unwrap().name.clone();
    assert_eq!(
        doc.notice.as_deref(),
        Some(format!("Only what's made before {name} can be projected").as_str())
    );
    doc.look(Look::ClickModel {
        pick: Some(pick(&doc, 20.0, 5.0)),
        add: false,
        double: false,
    });
    assert_eq!(outside(&doc), [wanted]);
    assert!(doc.notice.as_deref().unwrap().starts_with("Project takes"));
    // A click on nothing picks nothing.
    doc.look(Look::ClickModel {
        pick: None,
        add: false,
        double: false,
    });
    assert_eq!(outside(&doc), [wanted]);
    // The sketch's selection and the model's are left as they were.
    assert!(doc.sketch.as_ref().unwrap().selection.is_empty());
    assert!(doc.pick.selection.is_empty());
    // Each pick is a link committed as it's made: `Esc` lets go of the
    // tool, keeping them, and the model isn't picked any more.
    doc.look(Look::Escape);
    assert_eq!(outside(&doc), [wanted]);
    assert_eq!(doc.outside_tool(), None);
    assert!(!doc.picks());
    assert!(doc.model_picking().is_none());
}

#[test]
fn intersect_takes_faces_and_edges_and_refuses_a_sketch_s_curve() {
    let Setup {
        mut doc, before, ..
    } = setup(Tool::Intersect);
    doc.look(Look::ClickSketch {
        item: item(before),
        add: false,
    });
    assert!(outside(&doc).is_empty());
    assert!(
        doc.notice
            .as_deref()
            .unwrap()
            .starts_with("Intersect takes")
    );
    let face = pick(&doc, 20.0, 5.0);
    assert!(matches!(face.target, Picked::Face(_)));
    doc.look(Look::ClickModel {
        pick: Some(face),
        add: false,
        double: false,
    });
    assert!(matches!(outside(&doc)[..], [OutsideRef::Face(_)]));
    assert_eq!(doc.highlight().unwrap().selected_faces.len(), 1);
    // A vertex isn't taken.
    let corner = pick(&doc, -29.5, -19.5);
    assert!(matches!(corner.target, Picked::Vertex(_)), "{corner:?}");
    doc.look(Look::ClickModel {
        pick: Some(corner),
        add: false,
        double: false,
    });
    assert_eq!(outside(&doc).len(), 1);
    // Another tool keeps them, as links of the sketch, and doesn't
    // mark them: Project marks only its own.
    doc.look(Look::SelectTool(Tool::Project));
    assert_eq!(outside(&doc).len(), 1);
    assert!(doc.highlight().is_none_or(|h| h.selected_faces.is_empty()));
}

#[test]
fn project_refuses_a_body_made_after_the_sketch() {
    let (mut doc, requests) = example();
    // The plate's own sketch, before its extrude.
    let profile = doc.editor.document().features()[0].id;
    let lane = SolveLane::connect(&mut doc);
    let mut doc = Answered { doc, lane };
    doc.look(Look::EditFeature(profile));
    answer(&mut doc, &requests);
    doc.look(Look::SelectTool(Tool::Project));
    let edge = pick(&doc, 0.0, 20.0);
    doc.look(Look::ClickModel {
        pick: Some(edge),
        add: false,
        double: false,
    });
    assert!(outside(&doc).is_empty());
    let name = doc.editor.document().features()[0].name.clone();
    assert_eq!(
        doc.notice.as_deref(),
        Some(format!("Only what's made before {name} can be projected").as_str())
    );
    // A corner of it likewise.
    let corner = pick(&doc, -29.5, -19.5);
    doc.look(Look::ClickModel {
        pick: Some(corner),
        add: false,
        double: false,
    });
    assert!(outside(&doc).is_empty());
}

/// With Project, the list of what overlaps where the button was held
/// ticks what the tool has picked, and a row chosen or ticked acts as a
/// click on its item, refusals included.
#[test]
fn project_s_overlaps_tick_its_picks_and_their_rows_click() {
    use varde_view::{OverlapItem, OverlapItems, Overlaps};

    let Setup {
        mut doc,
        before,
        after,
        ..
    } = setup(Tool::Project);
    let edge = pick(&doc, 0.0, 20.0);
    assert!(matches!(edge.target, Picked::Edge(_)), "{edge:?}");
    let list = Overlaps {
        held: DVec2::ZERO,
        at: DVec2::ZERO,
        items: OverlapItems::Mixed(vec![
            OverlapItem::Sketch(item(before)),
            OverlapItem::Model(edge),
            OverlapItem::Sketch(item(after)),
        ]),
    };
    doc.look(Look::OpenOverlaps(list));
    assert!(doc.overlaps.is_some());
    assert_eq!(doc.overlap_ticked(), Some(vec![false, false, false]));
    doc.look(Look::HoverOverlap(Some(1)));
    assert_eq!(doc.pick.hover(), Some(edge));
    doc.look(Look::ToggleOverlap(0));
    doc.look(Look::ToggleOverlap(1));
    assert!(matches!(
        outside(&doc)[..],
        [OutsideRef::Sketch { .. }, OutsideRef::Edge(_)]
    ));
    assert_eq!(doc.overlap_ticked(), Some(vec![true, true, false]));
    // A later sketch's line is refused, saying why.
    doc.look(Look::ToggleOverlap(2));
    assert_eq!(outside(&doc).len(), 2);
    assert!(
        doc.notice
            .as_deref()
            .unwrap()
            .starts_with("Only what's made before")
    );
    // Chosen alone, the first is taken out and the list closes.
    doc.look(Look::ChooseOverlap {
        index: 0,
        add: false,
    });
    assert!(doc.overlaps.is_none());
    assert!(matches!(outside(&doc)[..], [OutsideRef::Edge(_)]));
}

/// Answers the models asked for, and the solver, until neither is asked
/// anything more: links found and folded in.
pub(super) fn settle(doc: &mut Answered, requests: &RefCell<Vec<Request>>) {
    for _ in 0..8 {
        if requests.borrow().is_empty() {
            break;
        }
        answer(doc, requests);
        let Answered { doc, lane } = doc;
        lane.answer(doc);
    }
}

/// The sketch being edited, as committed.
fn edited(doc: &Doc) -> &Sketch {
    let feature = doc.sketch.as_ref().unwrap().feature;
    match &doc.editor.document().feature(feature).unwrap().kind {
        FeatureKind::Sketch { sketch, .. } => sketch,
        _ => panic!("not a sketch"),
    }
}

/// The shape of the `index`th link of `sketch`, its two line ends sorted
/// by x.
fn line_ends(sketch: &Sketch, index: usize) -> [DVec2; 2] {
    let shape: LinkShape = sketch.link_shape(&sketch.links[index]).unwrap();
    assert!(
        matches!(shape.curves[..], [Curve::Line { .. }]),
        "{shape:?}"
    );
    let mut ends = [shape.points[0], shape.points[1]];
    ends.sort_by(|a, b| a.x.total_cmp(&b.x));
    ends
}

/// Project's clicks each commit a link, as one change each; the model
/// answered gives them their geometry, folded into that change: a line
/// of another sketch and an edge of the plate.
#[test]
fn project_commits_links_the_model_fills_and_undo_takes_back() {
    let Setup {
        mut doc,
        before,
        requests,
        ..
    } = setup(Tool::Project);
    doc.look(Look::ClickSketch {
        item: item(before),
        add: false,
    });
    let edge = pick(&doc, 0.0, 20.0);
    doc.look(Look::ClickModel {
        pick: Some(edge),
        add: false,
        double: false,
    });
    settle(&mut doc, &requests);
    let sketch = edited(&doc).clone();
    assert_eq!(sketch.links.len(), 2);
    assert!(
        sketch
            .links
            .iter()
            .all(|link| link.kind == LinkKind::Project)
    );
    let near = |a: DVec2, b: DVec2| a.distance(b) < 1e-9;
    let [a, b] = line_ends(&sketch, 0);
    assert!(near(a, DVec2::new(-25.0, 30.0)) && near(b, DVec2::new(25.0, 30.0)));
    let [a, b] = line_ends(&sketch, 1);
    assert!(near(a, DVec2::new(-30.0, 20.0)) && near(b, DVec2::new(30.0, 20.0)));
    // Construction, as they don't count for profiles, and fixed.
    let line = sketch.links[1].curves[0];
    assert!(sketch.curve(line).unwrap().construction);

    // One undo takes back the edge's link with its geometry: the found
    // geometry was no step of its own.
    doc.update(Edit::Undo);
    let undone = edited(&doc);
    assert_eq!(undone.links.len(), 1);
    assert!(undone.curve(line).is_none());
    doc.update(Edit::Redo);
    assert_eq!(edited(&doc), &sketch);
    // In step: answering again changes nothing.
    let revision = doc.editor.revision();
    settle(&mut doc, &requests);
    assert_eq!(doc.editor.revision(), revision);

    // Clicking the edge again takes its link out.
    doc.look(Look::ClickModel {
        pick: Some(pick(&doc, 5.0, 20.0)),
        add: false,
        double: false,
    });
    assert_eq!(edited(&doc).links.len(), 1);
    assert!(edited(&doc).curve(line).is_none());
}

/// Intersect on a sketch on XZ, through the hole's axis, cuts its wall
/// into two lines up it.
#[test]
fn intersect_cuts_the_hole_s_wall_into_two_lines() {
    use varde_kernel::mesh::Form;

    let (mut doc, requests) = example();
    doc.apply(
        doc.editor
            .document()
            .add_sketch(Plane::Origin(OriginPlane::XZ)),
    );
    let feature = doc.editor.document().features().last().unwrap().id;
    doc.sync();
    answer(&mut doc, &requests);
    let lane = SolveLane::connect(&mut doc);
    let mut doc = Answered { doc, lane };
    doc.look(Look::EditFeature(feature));
    answer(&mut doc, &requests);
    doc.look(Look::SelectTool(Tool::Intersect));
    // The hole's wall, named as the tool names what it picks.
    let evaluation =
        varde_regen::evaluate(&Document::example(), &mut varde_regen::Cache::default());
    let made = &evaluation.bodies[0];
    let topology = made.solid.topology();
    let wall = (0..topology.regions().len() as u32)
        .find(|&r| matches!(topology.form(&made.solid, r), Form::Cylinder { .. }))
        .unwrap();
    let region = &topology.regions()[wall as usize];
    let near = made
        .solid
        .mesh()
        .patch(region.tris[0] as usize)
        .eval(glam::DVec3::splat(1.0 / 3.0));
    let face = varde_document::FaceRef {
        body: made.body,
        key: region.key,
        near,
    };
    assert!(doc.propose_link(LinkKind::Intersect, OutsideRef::Face(face)));
    doc.lane.answer(&mut doc.doc);
    settle(&mut doc, &requests);
    let sketch = edited(&doc);
    assert_eq!(sketch.links.len(), 1);
    assert_eq!(sketch.links[0].kind, LinkKind::Intersect);
    let shape = sketch.link_shape(&sketch.links[0]).unwrap();
    assert_eq!(shape.curves.len(), 2, "{shape:?}");
    assert!(shape.points.iter().all(|p| (p.x.abs() - 8.0).abs() < 1e-9));
    assert!(doc.feed.broken(feature, sketch.links[0].id).is_none());
}

#[test]
fn a_second_click_while_the_first_is_with_the_solver_takes_it_back() {
    let Setup {
        mut doc, before, ..
    } = setup(Tool::Project);
    let wanted = OutsideRef::Sketch {
        sketch: before.0,
        item: before.1,
    };
    // Two clicks on the same line, the solver answering neither yet.
    let click = Look::ClickSketch {
        item: item(before),
        add: false,
    };
    doc.doc.look(click.clone());
    // Shown picked while it waits.
    assert_eq!(doc.model_picking().unwrap().marked, [item(before)]);
    doc.doc.look(click.clone());
    doc.lane.answer(&mut doc.doc);
    assert_eq!(outside(&doc), []);
    assert!(edited_links(&doc).is_empty());
    assert!(doc.model_picking().unwrap().marked.is_empty());
    // Once more picks it; twice more takes it out and picks it again.
    doc.doc.look(click.clone());
    doc.lane.answer(&mut doc.doc);
    assert_eq!(outside(&doc), [wanted]);
    assert_eq!(edited_links(&doc).len(), 1);
    doc.doc.look(click.clone());
    doc.doc.look(click);
    doc.lane.answer(&mut doc.doc);
    assert_eq!(outside(&doc), [wanted]);
    assert_eq!(edited_links(&doc).len(), 1);
}

/// The links of the sketch being edited, as committed.
fn edited_links(doc: &Doc) -> Vec<varde_sketch::Link> {
    let feature = doc.sketch.as_ref().unwrap().feature;
    match &doc.editor.document().feature(feature).unwrap().kind {
        FeatureKind::Sketch { sketch, .. } => sketch.links.clone(),
        _ => Vec::new(),
    }
}
