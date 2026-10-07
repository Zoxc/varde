//! Links following what they come from, folded into the change it's of.

use std::cell::RefCell;

use glam::DVec2;
use varde_document::{Command, FeatureId, FeatureKind, OriginPlane, OutsideRef, Plane, Sketch};
use varde_regen::Request;
use varde_sketch::{Constraint, Curve, LinkKind, SketchEdit};
use varde_view::Edit;

use crate::doc::Doc;
use crate::tests::{answer, example};

fn sketch_of(doc: &Doc, feature: FeatureId) -> &Sketch {
    match &doc.editor.document().feature(feature).unwrap().kind {
        FeatureKind::Sketch { sketch, .. } => sketch,
        _ => panic!("not a sketch"),
    }
}

/// Answers the models asked for until none is.
fn settle(doc: &mut Doc, requests: &RefCell<Vec<Request>>) {
    for _ in 0..8 {
        if requests.borrow().is_empty() {
            break;
        }
        answer(doc, requests);
    }
}

/// The example with a sketch on XY projecting the first corner of the
/// plate's sketch, found: the sketch and the plate's sketch.
fn projecting() -> (
    Doc,
    std::rc::Rc<RefCell<Vec<Request>>>,
    FeatureId,
    FeatureId,
) {
    projecting_item(|plate| plate.points[0].id)
}

/// The example with a sketch on XY projecting what `item` picks of the
/// plate's sketch, found: the sketch and the plate's sketch.
fn projecting_item(
    item: impl Fn(&Sketch) -> varde_sketch::Id,
) -> (
    Doc,
    std::rc::Rc<RefCell<Vec<Request>>>,
    FeatureId,
    FeatureId,
) {
    let (mut doc, requests) = example();
    let plate = doc.editor.document().features()[0].id;
    doc.apply(
        doc.editor
            .document()
            .add_sketch(Plane::Origin(OriginPlane::XY)),
    );
    let feature = doc.editor.document().features().last().unwrap().id;
    let corner = item(sketch_of(&doc, plate));
    let design = doc.editor.document().design();
    let added = SketchEdit::AddLink {
        kind: LinkKind::Project,
    }
    .apply(sketch_of(&doc, feature), &design)
    .unwrap();
    let link = added.links[0].id;
    doc.apply(Command::AddLink {
        feature,
        sketch: Box::new(added),
        source: varde_document::LinkSource {
            link,
            source: OutsideRef::Sketch {
                sketch: plate,
                item: corner,
            },
        },
    });
    doc.sync();
    settle(&mut doc, &requests);
    (doc, requests, feature, plate)
}

#[test]
fn a_moved_source_relinks_in_the_same_undo_step_keeping_ids() {
    let (mut doc, requests, feature, plate) = projecting();
    let link = sketch_of(&doc, feature).links[0].clone();
    assert_eq!(link.points.len(), 1);
    let corner = sketch_of(&doc, plate).points[0].at;
    let point = link.points[0];
    assert_eq!(sketch_of(&doc, feature).point(point).unwrap().at, corner);
    // The link and its point came in one change.
    doc.update(Edit::Undo);
    assert!(sketch_of(&doc, feature).links.is_empty());
    doc.update(Edit::Redo);
    assert_eq!(sketch_of(&doc, feature).links[0], link);

    // The plate's corner moved: the link's point follows, the same point.
    let mut moved = sketch_of(&doc, plate).clone();
    moved.points[0].at += DVec2::new(-4.0, 0.0);
    doc.apply(Command::SetSketch {
        feature: plate,
        sketch: Box::new(moved),
    });
    doc.sync();
    settle(&mut doc, &requests);
    assert_eq!(sketch_of(&doc, feature).links[0], link);
    let at = sketch_of(&doc, feature).point(point).unwrap().at;
    assert_eq!(at, corner + DVec2::new(-4.0, 0.0));
    // One undo takes both back.
    doc.update(Edit::Undo);
    assert_eq!(sketch_of(&doc, plate).points[0].at, corner);
    assert_eq!(sketch_of(&doc, feature).point(point).unwrap().at, corner);
    // And answering the model undone changes nothing more: it's in step.
    let revision = doc.editor.revision();
    doc.sync();
    settle(&mut doc, &requests);
    assert_eq!(doc.editor.revision(), revision);
    assert!(doc.editor.can_redo());
}

#[test]
fn a_source_gone_leaves_the_link_broken_with_what_it_held() {
    let (mut doc, requests, feature, plate) = projecting();
    let held = sketch_of(&doc, feature).clone();
    let link = held.links[0].id;
    let mut gone = sketch_of(&doc, plate).clone();
    let corner = gone.points[0].id;
    gone.delete(&[corner]);
    doc.apply(Command::SetSketch {
        feature: plate,
        sketch: Box::new(gone),
    });
    doc.sync();
    settle(&mut doc, &requests);
    assert_eq!(sketch_of(&doc, feature), &held);
    assert_eq!(
        doc.feed.broken(feature, link),
        Some("it isn't in its sketch any more")
    );
}

#[test]
fn a_source_changing_form_keeps_what_ids_it_can_and_says_what_went() {
    let (mut doc, requests, feature, plate) = projecting_item(|plate| plate.curves[0].id);
    let link = sketch_of(&doc, feature).links[0].clone();
    assert_eq!((link.points.len(), link.curves.len()), (2, 1));
    // The user's point on the projected line, and coincident with its
    // start.
    let mut own = sketch_of(&doc, feature).clone();
    let start = own.point(link.points[0]).unwrap().at;
    let free = own.add_point(start).unwrap();
    let on_line = own
        .add_constraint(Constraint::PointOnCurve {
            point: free,
            curve: link.curves[0],
        })
        .unwrap();
    let on_start = own
        .add_constraint(Constraint::Coincident(free, link.points[0]))
        .unwrap();
    doc.apply(Command::SetSketch {
        feature,
        sketch: Box::new(own),
    });
    doc.sync();
    settle(&mut doc, &requests);

    // The plate's line made an arc through its ends.
    let mut bent = sketch_of(&doc, plate).clone();
    let Curve::Line { start, end } = bent.curves[0].curve else {
        panic!("not a line");
    };
    let middle = (bent.point(start).unwrap().at + bent.point(end).unwrap().at) / 2.0;
    let center = bent.add_point(middle).unwrap();
    bent.curves[0].curve = Curve::Arc { center, start, end };
    doc.apply(Command::SetSketch {
        feature: plate,
        sketch: Box::new(bent),
    });
    doc.sync();
    settle(&mut doc, &requests);

    let relinked = sketch_of(&doc, feature);
    let now = relinked.links[0].clone();
    assert_eq!(now.id, link.id);
    // The arc's ends keep the line's ends' ids, by where they are.
    let arc = &relinked.curve(now.curves[0]).unwrap().curve;
    assert_eq!(arc.ends(), Some([link.points[0], link.points[1]]));
    assert!(relinked.curve(link.curves[0]).is_none());
    assert!(relinked.constraint(on_start).is_some());
    // What was on the line went with it, said so.
    assert!(relinked.constraint(on_line).is_none());
    assert_eq!(
        doc.notice.as_deref(),
        Some("Sketch 2's links changed shape, removing 1 constraint on what they no longer make")
    );
    // In step from then on: answering again changes nothing.
    let revision = doc.editor.revision();
    doc.sync();
    settle(&mut doc, &requests);
    assert_eq!(doc.editor.revision(), revision);
}

#[test]
fn a_relinked_sketch_refused_shows_its_links_broken_and_asks_no_more() {
    let (mut doc, requests, feature, plate) = projecting();
    let held = sketch_of(&doc, feature).clone();
    let link = held.links[0].id;
    let mut moved = sketch_of(&doc, plate).clone();
    moved.points[0].at += DVec2::new(-4.0, 0.0);
    doc.apply(Command::SetSketch {
        feature: plate,
        sketch: Box::new(moved),
    });
    doc.sync();
    // The model's answer relinks the sketch as the document refuses it:
    // with a link it has no source for.
    let copy = doc.editor.document().clone();
    let design = copy.design();
    let mut refused = false;
    for request in requests.take() {
        let mut response = varde_regen::handle(request);
        if let varde_regen::Response::Regenerated { relinked, .. } = &mut response {
            for (_, sketch) in relinked.iter_mut() {
                let more = SketchEdit::AddLink {
                    kind: LinkKind::Project,
                }
                .apply(sketch, &design)
                .unwrap();
                *sketch = std::sync::Arc::new(more);
                refused = true;
            }
        }
        doc.computed(response);
    }
    assert!(refused);
    assert_eq!(sketch_of(&doc, feature), &held);
    let why = doc
        .feed
        .broken(feature, link)
        .expect("shown broken")
        .to_owned();
    assert!(why.starts_with("what it found was refused: "), "{why}");
    // Nothing more is asked: the document is as it was.
    let revision = doc.editor.revision();
    doc.sync();
    assert!(requests.borrow().is_empty());
    assert_eq!(doc.editor.revision(), revision);
    assert_eq!(doc.feed.broken(feature, link).map(str::to_owned), Some(why));
}
