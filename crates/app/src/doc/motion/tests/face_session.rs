//! What the face sessions' tests share (shell, offset face, draft): the
//! doc holding a document, boxes extruded on XY, a box's flat faces
//! found and clicked in the model shown, what the screen says, and what
//! the session picks.

use glam::{DVec2, DVec3};
use varde_document::{
    BodyId, Command, Document, Editor, Extent, Extrude, FaceRef, Operation, OriginPlane, Plane,
};
use varde_regen::Summary;
use varde_sketch::{Curve, Sketch};
use varde_view::{Look, MotionPick, Pick, Picked};

use super::Plates;
use crate::tests::{holding, screen_texts};

/// Whether some text on the screen holds `wanted`.
pub(super) fn shows(plates: &Plates, wanted: &str) -> bool {
    screen_texts(&plates.doc)
        .iter()
        .any(|text| text.contains(wanted))
}

/// What the session being set up picks.
pub(super) fn picking(plates: &Plates) -> MotionPick {
    plates.doc.motion.as_ref().expect("a session").picking
}

/// The faces of the session being set up.
pub(super) fn picked_faces(plates: &Plates) -> Vec<FaceRef> {
    (plates.doc.motion.as_ref().expect("a session").faces.refs).clone()
}

/// The doc holding `document`, its bodies in order.
pub(super) fn held(document: Document) -> Plates {
    let bodies = document.bodies();
    let at = |i: usize| bodies.get(i).or(bodies.first()).expect("a body").id;
    let bodies = [at(0), at(1), at(2)];
    let (doc, requests) = holding(document);
    Plates {
        doc,
        requests,
        bodies,
    }
}

/// Adds a sketch on XY holding the rectangle from `a` to `b`, extruded
/// 10 up as a new body.
pub(super) fn add_box(editor: &mut Editor, a: DVec2, b: DVec2) {
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let mut drawn = Sketch::default();
    let corners = [(a.x, a.y), (b.x, a.y), (b.x, b.y), (a.x, b.y)]
        .map(|(x, y)| drawn.add_point(DVec2::new(x, y)).unwrap());
    for k in 0..4 {
        let line = Curve::Line {
            start: corners[k],
            end: corners[(k + 1) % 4],
        };
        drawn.add_curve(line, false).unwrap();
    }
    let profiles = drawn.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let extrude = Extrude {
        taper: None,
        sketch,
        regions,
        extent: Extent::OneSide(crate::tests::length(editor.document(), "10")),
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
}

/// A box from (0, 0, 0) to (40, 30, 10), "Body 1", and with `two` another
/// from (60, 0, 0) to (80, 20, 10), "Body 2".
pub(super) fn boxes(two: bool) -> Plates {
    let mut editor = Editor::new(Document::default());
    add_box(&mut editor, DVec2::ZERO, DVec2::new(40.0, 30.0));
    if two {
        add_box(&mut editor, DVec2::new(60.0, 0.0), DVec2::new(80.0, 20.0));
    }
    held(editor.document().clone())
}

/// The flat face of `body` in the model shown facing `normal` at `d`
/// along it, if it's there.
pub(super) fn flat(plates: &Plates, body: BodyId, normal: DVec3, d: f64) -> Option<u32> {
    let index = plates.doc.feed.pick_index();
    (index.body_faces(body)).find(|&face| {
        matches!(index.picking().faces()[face as usize].summary,
            Summary::Plane { n, d: at } if DVec3::from(n).distance(normal) < 1e-9 && (at - d).abs() < 1e-9)
    })
}

/// The pick of `body`'s flat face facing `normal` at `d`, at `at`.
pub(super) fn face_pick(plates: &Plates, body: BodyId, normal: DVec3, d: f64, at: DVec3) -> Pick {
    let face = flat(plates, body, normal, d).expect("the face shown");
    Pick {
        model: plates.doc.feed.pick_index().model(),
        target: Picked::Face(face),
        body,
        at,
        snap: None,
    }
}

/// A click on `pick`.
pub(super) fn click(plates: &mut Plates, pick: Pick) {
    plates.doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
}
