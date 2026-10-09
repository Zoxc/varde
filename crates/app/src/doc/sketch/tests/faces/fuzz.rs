//! Random sequences of what the user can do around sketches on faces:
//! new sketches on origin planes and on flat faces (tops, tilted walls,
//! a revolve's ends, a cut's faces), entering and leaving them, drawing,
//! extruding and revolving from them, changing their planes, removing
//! and joining what they're on, undo and redo anywhere, the document
//! replaced or read-only, answers late or out of order, checking after
//! each step what must always hold.

use varde_document::{AxisLine, BodyId, Operation, Revolve, Targets, Turn};
use varde_regen::Response;
use varde_view::RevolveLook;

use super::*;

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

/// A closed polygon through `corners`.
fn polygon(corners: &[(f64, f64)]) -> varde_sketch::Sketch {
    let mut sketch = varde_sketch::Sketch::default();
    let ids: Vec<_> = (corners.iter())
        .map(|&(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap())
        .collect();
    for (k, &start) in ids.iter().enumerate() {
        let end = ids[(k + 1) % ids.len()];
        sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    }
    sketch
}

/// Adds a sketch on `plane` holding `drawn`: its id.
fn add_sketch(editor: &mut Editor, plane: Plane, drawn: varde_sketch::Sketch) -> FeatureId {
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let feature = editor.document().features().last().unwrap().id;
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(drawn),
        })
        .unwrap();
    feature
}

/// The first region of the sketch `id`.
fn first_region(editor: &Editor, id: FeatureId) -> varde_sketch::RegionRef {
    let FeatureKind::Sketch { sketch, .. } = &editor.document().feature(id).unwrap().kind else {
        unreachable!();
    };
    sketch.profiles().unwrap().reference(0).unwrap()
}

fn length(editor: &Editor, text: &str) -> Value {
    Value::new(text, &Extent::ask(&editor.document().design())).unwrap()
}

/// The scene: the example's plate (Body 1, its top at z = 10) and a
/// 3 mm plate under it (Body 2); a pocket cut up into Body 1's bottom,
/// 5 mm deep (a flat cut face at z = 5 facing down); a prism along Y
/// whose walls slant at 45° (Body 3); a quarter turn of a rectangle about
/// Z (Body 4), whose ends are flat.
fn scene() -> Document {
    let (mut editor, _) = crate::tests::two_plates();
    let xy = Plane::Origin(OriginPlane::XY);
    let xz = Plane::Origin(OriginPlane::XZ);

    let square = [(-25.0, 5.0), (-15.0, 5.0), (-15.0, 15.0), (-25.0, 15.0)];
    let pocket = add_sketch(&mut editor, xy, polygon(&square));
    let cut = Extrude {
        taper: None,
        sketch: pocket,
        regions: vec![first_region(&editor, pocket)],
        extent: Extent::OneSide(length(&editor, "5")),
        flip: false,
        operation: Operation::Cut(Targets::default()),
    };
    editor
        .apply(editor.document().add_feature(cut.into()))
        .unwrap();

    let triangle = [(40.0, 0.0), (60.0, 0.0), (50.0, 10.0)];
    let end = add_sketch(&mut editor, xz, polygon(&triangle));
    let prism = Extrude {
        taper: None,
        sketch: end,
        regions: vec![first_region(&editor, end)],
        extent: Extent::Symmetric(length(&editor, "20")),
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(prism.into()))
        .unwrap();

    let rectangle = [(80.0, 0.0), (90.0, 0.0), (90.0, 10.0), (80.0, 10.0)];
    let profile = add_sketch(&mut editor, xz, polygon(&rectangle));
    let ask = Turn::ask(&editor.document().design());
    let turn = Revolve {
        sketch: profile,
        regions: vec![first_region(&editor, profile)],
        axis: AxisLine::SketchY,
        extent: Turn::OneSide(Value::new("90", &ask).unwrap()),
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(turn.into()))
        .unwrap();
    editor.document().clone()
}

/// The sketch features of `document`.
fn sketches(document: &Document) -> Vec<FeatureId> {
    (document.features().iter())
        .filter(|f| matches!(f.kind, FeatureKind::Sketch { .. }))
        .map(|f| f.id)
        .collect()
}

/// Where regenerating `document` places each sketch on a face.
fn regen_placements(request: Request) -> Option<Vec<(FeatureId, Placement)>> {
    match varde_regen::handle(request) {
        Response::Regenerated { placements, .. } => Some(placements),
        _ => None,
    }
}

/// A fresh request for the document as it is, without a draft.
fn fresh_request(doc: &Doc) -> Request {
    Request::Regenerate {
        sight: None,
        generation: doc.editor.generation(),
        document: doc.editor.snapshot(),
        exclude: None,
        until: None,
        draft: None,
        inspect: None,
    }
}

/// Where the sketch `id` of `document` goes by `placements` (regen's).
fn expected(
    document: &Document,
    placements: &[(FeatureId, Placement)],
    id: FeatureId,
) -> Option<Placement> {
    match &document.feature(id)?.kind {
        FeatureKind::Sketch {
            plane: Plane::Origin(plane),
            ..
        } => Some(plane.placement()),
        FeatureKind::Sketch { .. } => (placements.iter())
            .find(|(placed, _)| *placed == id)
            .map(|&(_, placement)| placement),
        _ => None,
    }
}

/// Whether the model shown is of the document as it is, a model of it
/// and nothing older: what's picked in it names what the document holds.
fn current(doc: &Doc) -> bool {
    doc.feed.generation() == Some(doc.editor.generation()) && doc.picks()
}

/// The command putting the sketch picking is for, or a new one, on
/// `plane`.
fn command_for(doc: &Doc, plane: Plane) -> Command {
    match doc
        .picking_plane
        .as_ref()
        .and_then(|p| p.pick.sketch.clone())
    {
        Some((feature, _)) => Command::SetSketchPlane { feature, plane },
        None => doc.editor.document().add_sketch(plane),
    }
}

fn check(doc: &mut Doc, requests: &Requests, last_sent: &Option<Request>, at: &str) {
    // Picking only in a document that can be edited, outside a sketch
    // and an operation.
    if doc.picking_plane.is_some() {
        assert!(doc.editable(), "{at}: picking in a read-only document");
        assert!(doc.sketch.is_none() && !doc.operating(), "{at}");
    }
    assert!(!(doc.operating() && doc.sketch.is_some()), "{at}");

    // Offered exactly where the document takes the sketch.
    if let Some(picking) = doc.model_picking()
        && let Some(planes) = picking.planes
        && current(doc)
    {
        let index = doc.feed.pick_index();
        for (face, near) in faces(doc, |_| true) {
            let takes = planes.takes(index, face);
            let allowed = index.face_placement(face).is_some_and(|p| p.valid())
                && planes.face_ref(index, face, near).is_some_and(|face| {
                    let mut editor = Editor::new(doc.editor.document().clone());
                    editor.apply(command_for(doc, Plane::Face(face))).is_ok()
                });
            assert_eq!(takes, allowed, "{at}: face {face} offered {takes}");
        }
    }

    // Once everything asked is answered, every sketch is where
    // regenerating the document places it, and the one edited too.
    if requests.borrow().is_empty()
        && let Some(sent) = last_sent
        && sent.generation() == Some(doc.editor.generation())
        && doc.feed.generation() == Some(doc.editor.generation())
    {
        let placements = regen_placements(sent.clone()).expect("regenerated");
        let document = doc.editor.document().clone();
        for id in sketches(&document) {
            assert_eq!(
                doc.placement(id),
                expected(&document, &placements, id),
                "{at}: sketch {id:?} placed elsewhere"
            );
        }
        if let Some(session) = &doc.sketch
            && let Some(placement) = expected(&document, &placements, session.feature)
        {
            assert_eq!(session.placement, placement, "{at}: edited elsewhere");
        }
    }

    // In a sketch, the camera faces it, from either side of its plane
    // (whichever the view was on, see `facing_turn`).
    if let Some(session) = &doc.sketch {
        let normal = session.placement.normal.as_vec3();
        settle_camera(doc);
        let backward = doc.camera.backward();
        assert!(
            backward.abs_diff_eq(normal, 1e-4) || backward.abs_diff_eq(-normal, 1e-4),
            "{at}: the camera looks along {} at a sketch facing {normal}",
            doc.camera.backward()
        );
    }
}

/// Runs `steps` random steps from `seed`.
fn run(seed: u64, steps: usize) {
    let first = scene();
    let (mut doc, requests) = crate::tests::holding(first.clone());
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut renderer = varde_view::probe::renderer();
    let mut last_sent: Option<Request> = None;
    // Face references picked from models shown before, to come late.
    let mut old_faces: Vec<FaceRef> = Vec::new();
    let note = |last_sent: &mut Option<Request>, requests: &Requests| {
        let requests = requests.borrow();
        let found =
            (requests.iter().rev()).find(|request| matches!(request, Request::Regenerate { .. }));
        if let Some(found) = found {
            *last_sent = Some(found.clone());
        }
    };
    for step in 0..steps {
        note(&mut last_sent, &requests);
        let document = doc.editor.document().clone();
        let revision = doc.editor.revision();
        let all_sketches = sketches(&document);
        let features: Vec<FeatureId> = document.features().iter().map(|f| f.id).collect();
        let sketch = (!all_sketches.is_empty()).then(|| *rng.pick(&all_sketches));
        let shown_faces = if doc.feed.generation().is_some() {
            faces(&doc, |_| true)
        } else {
            Vec::new()
        };
        let mut picking_for = doc
            .picking_plane
            .as_ref()
            .map(|p| p.pick.sketch.as_ref().map(|(id, _)| *id));
        let mut current_model = current(&doc);
        let mut escaped = false;
        // A plane taken: a new sketch, or the sketch picking was for.
        let mut taken: Option<Plane> = None;
        // The document and revision just before it was taken: getting the
        // pick ready (finishing a sketch, answers relinking) can change
        // the document before, folding into the change before the pick.
        let mut picked_from = None;
        let roll = rng.below(46);
        let what = format!("seed {seed} step {step}: roll {roll}");
        match roll {
            0..=2 => key_in(&mut doc, letter("s")),
            3 => doc.look(Look::PickPlane),
            4..=8 => {
                // Picking started, if it isn't, for a new sketch or
                // another plane for one; then a click on a face of the
                // model shown, as the viewport sends it: only one that
                // takes the sketch picks. Mostly one that does.
                if doc.picking_plane.is_none() {
                    if doc.sketch.is_some() && rng.below(3) > 0 {
                        doc.look(Look::FinishSketch);
                    }
                    if doc.operating() {
                        doc.look(Look::Escape);
                    }
                    if doc.read_only.is_some() && rng.below(2) == 0 {
                        doc.read_only = None;
                        doc.sync();
                    }
                    if rng.below(2) == 0 {
                        answer(&mut doc, &requests);
                    }
                    match sketch {
                        Some(id) if rng.below(2) == 0 => doc.look(Look::ChangePlane(id)),
                        _ => key_in(&mut doc, letter("s")),
                    }
                    picking_for = (doc.picking_plane.as_ref())
                        .map(|p| p.pick.sketch.as_ref().map(|(id, _)| *id));
                    current_model = current(&doc);
                }
                let takes = |face: u32| {
                    doc.model_picking().is_some_and(|picking| {
                        picking
                            .planes
                            .is_some_and(|planes| planes.takes(picking.index, face))
                    })
                };
                let taking: Vec<_> = (shown_faces.iter())
                    .filter(|(face, _)| takes(*face))
                    .copied()
                    .collect();
                let from = if taking.is_empty() || rng.below(4) == 0 {
                    &shown_faces
                } else {
                    &taking
                };
                if let Some(&(face, near)) = (!from.is_empty()).then(|| rng.pick(from)) {
                    let index = doc.feed.pick_index();
                    if let Some(face_ref) = index.face_ref(face, near) {
                        old_faces.push(face_ref);
                    }
                    let sent = doc.model_picking().and_then(|picking| {
                        let planes = picking.planes?;
                        planes
                            .takes(index, face)
                            .then(|| planes.face_ref(index, face, near))
                            .flatten()
                    });
                    if let Some(face) = sent {
                        taken = Some(Plane::Face(face));
                        picked_from = Some((doc.editor.document().clone(), doc.editor.revision()));
                        doc.update(Edit::FacePicked(face));
                    }
                }
            }
            9 => {
                // A face asked for anyway: of the model shown, or of one
                // shown before.
                let face = if rng.below(2) == 0 || shown_faces.is_empty() {
                    (!old_faces.is_empty()).then(|| *rng.pick(&old_faces))
                } else {
                    let &(face, near) = rng.pick(&shown_faces);
                    doc.feed.pick_index().face_ref(face, near)
                };
                if let Some(face) = face {
                    taken = Some(Plane::Face(face));
                    picked_from = Some((doc.editor.document().clone(), doc.editor.revision()));
                    doc.update(Edit::FacePicked(face));
                }
            }
            10 | 11 => {
                let plane = *rng.pick(&[OriginPlane::XY, OriginPlane::XZ, OriginPlane::YZ]);
                taken = Some(Plane::Origin(plane));
                picked_from = Some((doc.editor.document().clone(), doc.editor.revision()));
                doc.update(Edit::PlanePicked(plane));
            }
            12..=14 => {
                escaped = true;
                doc.look(Look::Escape);
            }
            15 | 16 => {
                if let Some(id) = sketch {
                    doc.look(Look::EditFeature(id));
                }
            }
            17 => doc.look(Look::FinishSketch),
            18 | 19 => {
                // Change plane: from the Timeline, or the Sketch tab's
                // button while editing.
                let id = match &doc.sketch {
                    Some(session) if rng.below(2) == 0 => Some(session.feature),
                    _ => sketch,
                };
                if let Some(id) = id {
                    doc.look(Look::ChangePlane(id));
                }
            }
            20 | 21 => {
                // Something drawn in a sketch, the edited one if there
                // is one: a square or a circle, on its face or off it.
                let id = doc.sketch.as_ref().map(|s| s.feature).or(sketch);
                if let Some(id) = id.filter(|id| first.feature(*id).is_none()) {
                    let (cx, cy) =
                        *rng.pick(&[(0.0, 0.0), (20.0, 10.0), (3.0, -2.0), (-50.0, 40.0)]);
                    let drawn = if rng.below(2) == 0 {
                        polygon(&[
                            (cx - 2.0, cy - 2.0),
                            (cx + 2.0, cy - 2.0),
                            (cx + 2.0, cy + 2.0),
                            (cx - 2.0, cy + 2.0),
                        ])
                    } else {
                        let mut drawn = varde_sketch::Sketch::default();
                        let center = drawn.add_point(DVec2::new(cx, cy)).unwrap();
                        drawn
                            .add_curve(
                                Curve::Circle {
                                    center,
                                    radius: 3.0,
                                },
                                false,
                            )
                            .unwrap();
                        drawn
                    };
                    doc.apply(Command::SetSketch {
                        feature: id,
                        sketch: Box::new(drawn),
                    });
                    doc.sync();
                }
            }
            22 | 23 => {
                // An extrude set up from a sketch and committed, or not.
                if doc.sketch.is_some() {
                    doc.look(Look::FinishSketch);
                }
                if doc.revolve.is_some() {
                    doc.look(Look::Escape);
                }
                if doc.extrude.is_none() {
                    doc.look(Look::StartExtrude);
                }
                if let Some(id) = sketch {
                    doc.look(Look::Extrude(ExtrudeLook::PickRegion {
                        sketch: id,
                        region: 0,
                    }));
                }
                let kind = *rng.pick(&OperationKind::ALL);
                doc.look(Look::Extrude(ExtrudeLook::Operation(kind)));
                if rng.below(2) == 0 {
                    doc.update(Edit::CommitExtrude);
                }
            }
            24 => {
                // A revolve about the sketch's y axis.
                if doc.sketch.is_some() {
                    doc.look(Look::FinishSketch);
                }
                if doc.extrude.is_some() {
                    doc.look(Look::Escape);
                }
                if doc.revolve.is_none() {
                    doc.look(Look::StartRevolve);
                }
                if let Some(id) = sketch {
                    doc.look(Look::Revolve(RevolveLook::PickRegion {
                        sketch: id,
                        region: 0,
                    }));
                    let axis = *rng.pick(&[AxisLine::SketchX, AxisLine::SketchY]);
                    doc.look(Look::Revolve(RevolveLook::PickAxis { sketch: id, axis }));
                }
                if rng.below(2) == 0 {
                    doc.update(Edit::CommitRevolve);
                }
            }
            25..=27 => doc.update(Edit::Undo),
            28 | 29 => doc.update(Edit::Redo),
            30 if !features.is_empty() => {
                doc.update(Edit::RemoveFeature(*rng.pick(&features)));
                if doc.deleting.is_some() {
                    if rng.below(3) == 0 {
                        doc.look(Look::CancelDelete);
                    } else {
                        doc.update(if rng.below(2) == 0 {
                            Edit::ConfirmDelete
                        } else {
                            Edit::ConfirmDeleteOnly
                        });
                    }
                }
            }
            31 if doc.editable() => {
                // A join through both plates, merging Body 2 into Body 1.
                doc.apply(document.add_sketch(Plane::Origin(OriginPlane::XY)));
                let disc = doc.editor.document().features().last().unwrap().id;
                let mut drawn = varde_sketch::Sketch::default();
                let center = drawn.add_point(DVec2::new(20.0, 0.0)).unwrap();
                (drawn.add_curve(
                    Curve::Circle {
                        center,
                        radius: 5.0,
                    },
                    false,
                ))
                .unwrap();
                let region = drawn.profiles().unwrap().reference(0).unwrap();
                doc.apply(Command::SetSketch {
                    feature: disc,
                    sketch: Box::new(drawn),
                });
                let join = Extrude {
                    taper: None,
                    sketch: disc,
                    regions: vec![region],
                    extent: crate::tests::two_sides(&document, "15", "5"),
                    flip: false,
                    operation: Operation::Join(Targets::default()),
                };
                doc.apply(doc.editor.document().add_feature(join.into()));
                doc.sync();
            }
            32 => {
                // The plate made thicker or thinner.
                let plate = document.features().get(1).map(|f| f.id);
                if let Some(plate) = plate
                    && let Some(FeatureKind::Extrude(extrude)) =
                        document.feature(plate).map(|f| f.kind.clone())
                {
                    let text = *rng.pick(&["10", "12", "4", "30"]);
                    let ask = Extent::ask(&document.design());
                    let changed = Extrude {
                        extent: Extent::OneSide(Value::new(text, &ask).unwrap()),
                        ..extrude
                    };
                    doc.apply(Command::SetFeature {
                        feature: plate,
                        kind: Box::new(changed.into()),
                    });
                    doc.sync();
                }
            }
            33 => {
                let by = if rng.below(2) == 0 {
                    first.clone()
                } else {
                    document.clone()
                };
                doc.apply(Command::Replace(Box::new(by)));
                doc.sync();
            }
            34 => {
                doc.read_only = match doc.read_only {
                    Some(_) => None,
                    None => Some("read-only".to_owned()),
                };
                doc.sync();
            }
            35 | 36 => answer(&mut doc, &requests),
            37 => {
                let mut taken = requests.take();
                taken.reverse();
                for request in taken {
                    doc.computed(varde_regen::handle(request));
                }
            }
            38 => {
                let mut waiting = requests.borrow_mut();
                let oldest = (!waiting.is_empty()).then(|| waiting.remove(0));
                drop(waiting);
                if let Some(request) = oldest {
                    doc.computed(varde_regen::handle(request));
                }
            }
            39 => {
                let sizes = [
                    (1280.0, 800.0),
                    (640.0, 400.0),
                    (300.0, 900.0),
                    (1920.0, 300.0),
                ];
                let (w, h) = *rng.pick(&sizes);
                let mode = *rng.pick(&[Mode::Light, Mode::Dark]);
                let ui = shown(doc.view_in(mode), iced::Size::new(w, h), &mut renderer);
                drop(ui);
            }
            40 => {
                // A face selected, then `S`.
                if let Some(&(face, near)) =
                    (!shown_faces.is_empty()).then(|| rng.pick(&shown_faces))
                    && doc.picks()
                {
                    let index = doc.feed.pick_index();
                    let body = index.face_body(face);
                    if let Some(body) = body {
                        let pick = varde_view::Pick {
                            model: doc.feed.model(),
                            target: Picked::Face(face),
                            body,
                            at: near,
                            snap: None,
                        };
                        doc.look(Look::ClickModel {
                            pick: Some(pick),
                            add: false,
                            double: false,
                        });
                    }
                }
                key_in(&mut doc, letter("s"));
            }
            41 => {
                if let Some(id) = sketch {
                    doc.look(Look::SelectFeature(id));
                }
            }
            42 => doc.update(crate::tests::an_edit(&doc)),
            43 => {
                // A key: Enter, Delete or Escape.
                let named = *rng.pick(&[
                    iced::keyboard::key::Named::Enter,
                    iced::keyboard::key::Named::Delete,
                ]);
                key_in(&mut doc, iced::keyboard::Key::Named(named));
                if doc.deleting.is_some() {
                    doc.look(Look::CancelDelete);
                }
            }
            44 => {
                doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
            }
            _ => {}
        }
        let after = doc.editor.document().clone();
        if escaped {
            assert_eq!(after, document, "{what}: Esc changed the document");
            assert_eq!(doc.editor.revision(), revision, "{what}");
        }
        if let (Some(plane), Some(picking_for), Some((document, revision))) =
            (taken, picking_for, picked_from)
            && doc.editor.revision() != revision
        {
            // One undo step; a change of plane keeps the drawing.
            match picking_for {
                Some(id) => {
                    assert_eq!(super::plane(&doc, id), plane_of(&after, id), "{what}");
                    assert_eq!(
                        drawing_in(&document, id),
                        drawing_in(&after, id),
                        "{what}: drawing changed"
                    );
                }
                None => assert_eq!(after.features().len(), document.features().len() + 1),
            }
            // Picked in a model of the document as it was: placed there
            // at once, as regenerating places it.
            if let Plane::Face(_) = plane
                && current_model
            {
                let id = match picking_for {
                    Some(id) => id,
                    None => after.features().last().unwrap().id,
                };
                let placements = regen_placements(fresh_request(&doc)).unwrap();
                let wanted = expected(&after, &placements, id);
                assert!(wanted.is_some(), "{what}: picked face fails in regen");
                assert_eq!(doc.placement(id), wanted, "{what}: pick placed elsewhere");
            }
            doc.update(Edit::Undo);
            assert_eq!(*doc.editor.document(), document, "{what}: undo");
            doc.update(Edit::Redo);
            assert_eq!(*doc.editor.document(), after, "{what}: redo");
        }
        note(&mut last_sent, &requests);
        check(&mut doc, &requests, &last_sent, &what);
    }
}

fn plane_of(document: &Document, id: FeatureId) -> Plane {
    match &document.feature(id).unwrap().kind {
        FeatureKind::Sketch { plane, .. } => *plane,
        _ => panic!("not a sketch"),
    }
}

/// What the user drew in the sketch `id`: its sketch less the sketch
/// face's link, which follows the plane (`Command::SetSketchPlane`), and
/// with `next_id` cleared, as that link coming or going spends ids.
fn drawing_in(document: &Document, id: FeatureId) -> varde_sketch::Sketch {
    let FeatureKind::Sketch {
        plane,
        sketch,
        sources,
    } = &document.feature(id).unwrap().kind
    else {
        panic!("not a sketch")
    };
    let mut drawing = match varde_document::sketch_face(plane, sketch, sources) {
        Some(link) => varde_sketch::SketchEdit::Delete(vec![link])
            .apply(sketch, &document.design())
            .expect("the sketch face's link deletes"),
        None => sketch.clone(),
    };
    drawing.next_id = 0;
    drawing
}

/// The steps of the quick run, see [`crate::tests::fuzz_steps`].
const QUICK_STEPS: usize = 100;

#[test]
fn random_face_sketching_keeps_its_invariants() {
    for seed in varde_testing::seeds(1, 4) {
        run(seed, crate::tests::fuzz_steps(QUICK_STEPS, 150));
    }
}
