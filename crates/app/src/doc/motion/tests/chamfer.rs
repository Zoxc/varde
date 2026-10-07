//! The chamfer session: `C` on the example plate, edges picked and
//! toggled with a click (lit, sorted, all on one body: another body's
//! neither lit nor taken), the kernel's stand-in failing as not built yet
//! in the panel and Add anyway keeping it; with regeneration chamfering
//! by prisms ([`varde_regen::testing`]), the cut previewed, the types,
//! Flip sides and Tangent chain drafted, OK as one undo step; editing
//! from the Timeline, Cancel and undo; an edge an undo takes away said
//! to be gone; the rail's and the toolbar's Chamfer; the edges selected
//! taken in.

use glam::DVec3;
use varde_document::{BodyId, Chamfer, ChamferSize, Document, Editor, FeatureId, FeatureKind};
use varde_regen::Summary;
use varde_view::{
    ChamferType, Edit, Look, MotionField, MotionKind, MotionLook, MotionPick, PanelHover, Pick,
    Picked, Picks, Selection, SelectionMode,
};

use super::holding;
use super::{Plates, character, enter, later_disc, near, plates};
use crate::tests::{key_in, screen_texts};

pub(super) fn shows(plates: &Plates, wanted: &str) -> bool {
    screen_texts(&plates.doc)
        .iter()
        .any(|text| text.contains(wanted))
}

pub(super) fn picking(plates: &Plates) -> MotionPick {
    plates.doc.motion.as_ref().expect("a session").picking
}

/// The chamfer the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<Chamfer> {
    match plates.last_draft()?.1 {
        FeatureKind::Chamfer(chamfer) => Some(chamfer),
        _ => None,
    }
}

/// The edges of the chamfer being set up.
pub(super) fn edges(plates: &Plates) -> Vec<varde_document::EdgeRef> {
    plates
        .doc
        .motion
        .as_ref()
        .expect("a session")
        .blend
        .edges
        .refs
        .clone()
}

/// The example's plate alone, "Body 1": 60 × 40 × 10 about the Z axis
/// from z 0 up, a hole of radius 8 through it about the Z axis.
pub(super) fn plate() -> (Plates, BodyId) {
    let document = Document::example();
    let plate = document.bodies()[0].id;
    let (doc, requests) = holding(document);
    let plates = Plates {
        doc,
        requests,
        bodies: [plate; 3],
    };
    (plates, plate)
}

/// The straight edge of `body` in the model shown from `a` to `b`
/// (either way round), if it's there.
pub(super) fn straight(plates: &Plates, body: BodyId, a: [f64; 3], b: [f64; 3]) -> Option<u32> {
    let (a, b) = (DVec3::from(a), DVec3::from(b));
    let index = plates.doc.feed.pick_index();
    (0..index.mesh().edge_count() as u32).find(|&edge| {
        index.body(Picked::Edge(edge)) == Some(body)
            && (index.chain_keys(edge))
                .and_then(|keys| index.edge_ends(edge, &keys))
                .is_some_and(|[from, to]| {
                    (near(from, a) && near(to, b)) || (near(from, b) && near(to, a))
                })
    })
}

/// The plate's top edge at the front, (-30, -20, 10) to (30, -20, 10).
pub(super) const FRONT: ([f64; 3], [f64; 3]) = ([-30.0, -20.0, 10.0], [30.0, -20.0, 10.0]);
/// The plate's top edge at the back.
pub(super) const BACK: ([f64; 3], [f64; 3]) = ([-30.0, 20.0, 10.0], [30.0, 20.0, 10.0]);
/// The plate's top edge on the right, along Y.
pub(super) const RIGHT: ([f64; 3], [f64; 3]) = ([30.0, -20.0, 10.0], [30.0, 20.0, 10.0]);

/// The pick of the plate's edge `ends` at its middle, on the model shown.
pub(super) fn edge_pick(plates: &Plates, body: BodyId, (a, b): ([f64; 3], [f64; 3])) -> Pick {
    let edge = straight(plates, body, a, b).expect("the edge shown");
    Pick {
        model: plates.doc.feed.pick_index().model(),
        target: Picked::Edge(edge),
        body,
        at: (DVec3::from(a) + DVec3::from(b)) / 2.0,
        snap: None,
    }
}

/// A click on the edge `ends` of `body`.
pub(super) fn click_edge(plates: &mut Plates, body: BodyId, ends: ([f64; 3], [f64; 3])) {
    let pick = edge_pick(plates, body, ends);
    plates.doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
}

/// Whether the model shown has a flat face of `body` facing `normal`.
pub(super) fn faces_way(plates: &Plates, body: BodyId, normal: DVec3) -> bool {
    let index = plates.doc.feed.pick_index();
    (index.body_faces(body)).any(|face| {
        matches!(index.picking().faces()[face as usize].summary,
            Summary::Plane { n, .. } if DVec3::from(n).distance(normal) < 1e-9)
    })
}

/// `C` with the example's only body: a chamfer picking edges, the
/// mock's panel; a face refused; edges clicked picked, lit and listed
/// sorted with their lengths, a second click taking one out; the
/// preview fails as not built yet (the kernel's chamfer isn't built),
/// shown in the panel; OK waits, Add anyway keeps it as one undo step.
#[test]
fn c_picks_and_toggles_edges_and_add_anyway_keeps_the_unbuilt_chamfer() {
    let (mut plates, plate) = plate();
    let before = plates.doc.editor.document().clone();
    key_in(&mut plates.doc, character("c"));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Chamfer);
    assert!(session.bodies.is_empty(), "its body is its edges'");
    assert_eq!(picking(&plates), MotionPick::Edges);
    for text in [
        "New chamfer",
        "Edges",
        "Click edges",
        "Type",
        "Equal",
        "Two distances",
        "Distance and angle",
        "Distance",
        "Tangent chain",
        "pick the edges to chamfer",
    ] {
        assert!(shows(&plates, text), "{text}");
    }
    // Equal is the same either way round.
    assert!(!shows(&plates, "Flip sides"));
    let picks = plates.doc.model_picking().expect("picking");
    assert_eq!(picks.picks, Picks::Edges);
    assert!(!plates.doc.motion_ready());
    assert!(plates.last_draft().is_none(), "nothing to chamfer yet");

    // A face is no edge.
    let top = plates.face(
        plate,
        |s| matches!(s, Summary::Plane { n, .. } if n[2] > 0.5),
    );
    plates.click_at(plate, Picked::Face(top), DVec3::new(0.0, 15.0, 10.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only an edge can be chamfered")
    );

    // The front edge hovered lights; clicked, it's picked, lit as
    // selected and listed with its length.
    let front = edge_pick(&plates, plate, FRONT);
    plates.doc.look(Look::Hover(Some(front)));
    let highlight = plates.doc.motion_highlight().expect("lit");
    assert!(!highlight.is_empty());
    click_edge(&mut plates, plate, FRONT);
    assert_eq!(edges(&plates).len(), 1);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate]);
    assert_eq!(plates.doc.blend_lit(), [front.target]);
    assert!(shows(&plates, "Edge 1"));
    assert!(shows(&plates, "60 mm"));
    assert!(plates.doc.motion_ready());
    let chamfer = drafted(&plates).expect("a chamfer's draft");
    let one = |text: &str| {
        varde_expr::Value::new(text, &Chamfer::distance_ask(&before.design())).unwrap()
    };
    assert!(matches!(&chamfer.distances, ChamferSize::Equal(d) if d.value == one("1 mm").value));
    assert!(chamfer.chains && !chamfer.flip);
    assert!(shows(&plates, "1 edge · Equal · 1 mm · Tangent chain"));
    // Its row hovered in the panel lights it as hovered too.
    plates.doc.look(Look::Hover(None));
    let picked = plates.doc.motion_highlight().expect("lit").clone();
    plates.doc.look(Look::HoverPanel(Some(PanelHover::Edge(0))));
    let hovered = plates.doc.motion_highlight().expect("lit").clone();
    assert_ne!(picked, hovered);
    plates.doc.look(Look::LeavePanel(PanelHover::Edge(0)));
    assert_eq!(plates.doc.motion_highlight(), Some(&picked));

    // Two more, at once (the preview of the first on its way), sorted as
    // the feature keeps them; the second click on the back edge takes it
    // out again.
    click_edge(&mut plates, plate, RIGHT);
    click_edge(&mut plates, plate, BACK);
    let picked = edges(&plates);
    assert_eq!(picked.len(), 3);
    assert!(
        picked
            .windows(2)
            .all(|pair| pair[0].order(&pair[1]).is_lt())
    );
    assert!(shows(&plates, "Edge 3") && shows(&plates, "40 mm"));
    click_edge(&mut plates, plate, BACK);
    assert_eq!(edges(&plates).len(), 2);
    assert!(!shows(&plates, "Edge 3"));
    assert_eq!(drafted(&plates).unwrap().edges, edges(&plates));

    // The stand-in fails as not built yet: the panel says so, and the body
    // is shown whole, its edges there to pick.
    plates.answer();
    let error = plates.doc.feed.draft_error().expect("the stand-in fails");
    assert!(error.contains("yet"), "{error}");
    assert!(shows(&plates, "Chamfer fails"));
    assert!(shows(&plates, "Add anyway"));
    let state = plates.doc.motion_state().unwrap();
    assert!(!state.ready && state.accept);
    assert!(straight(&plates, plate, FRONT.0, FRONT.1).is_some());
    let features = plates.doc.editor.document().features().len();
    key_in(&mut plates.doc, enter());
    assert_eq!(plates.doc.editor.document().features().len(), features);

    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    let FeatureKind::Chamfer(stored) = &kind else {
        panic!("a chamfer");
    };
    assert_eq!(stored.edges.len(), 2);
    let document = plates.doc.editor.document();
    assert_eq!(document.feature(id).unwrap().name, "Chamfer 1");
    assert_eq!(plates.doc.selected_feature, Some(id));
    plates.answer();
    assert!(
        (plates.doc.feed.failed_features().iter()).any(|failed| failed.feature == id),
        "the chamfer fails"
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// With regeneration chamfering by prisms: the front edge cut off at
/// 45° in the preview, gone from the model shown (its row's cross takes
/// it out); Two distances, Flip sides, Distance and angle (90° refused
/// under its field), Tangent chain off, each drafted; OK adds it as one
/// undo step.
#[test]
fn the_types_flip_and_chain_are_previewed_and_ok_adds_one_undo_step() {
    varde_regen::testing::chamfer_by_wedges();
    let (mut plates, plate) = plate();
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartChamfer);
    click_edge(&mut plates, plate, FRONT);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let slope = DVec3::new(0.0, -1.0, 1.0).normalize();
    assert!(faces_way(&plates, plate, slope), "the cut");
    assert!(straight(&plates, plate, FRONT.0, FRONT.1).is_none());
    assert!(plates.doc.blend_lit().is_empty(), "cut off");
    // Its row stays, unmeasured.
    assert!(shows(&plates, "Edge 1") && !shows(&plates, "60 mm"));
    // The edges round the cut, made by the chamfer itself, neither light
    // nor are taken, however quickly clicked.
    let cut = plates.face(plate, |summary| {
        matches!(*summary, Summary::Plane { n, .. } if DVec3::from(n).distance(slope) < 1e-9)
    });
    let index = plates.doc.feed.pick_index();
    let rims: Vec<u32> = (0..index.mesh().edge_count() as u32)
        .filter(|&edge| {
            index
                .edge_faces(edge)
                .is_some_and(|faces| faces.contains(&cut))
        })
        .collect();
    assert!(!rims.is_empty());
    for rim in rims {
        let pick = Pick {
            target: Picked::Edge(rim),
            ..edge_pick(&plates, plate, BACK)
        };
        assert!(!plates.doc.takes_reference(pick));
        plates.doc.look(Look::ClickModel {
            pick: Some(pick),
            add: false,
            double: false,
        });
        assert_eq!(edges(&plates).len(), 1);
    }

    plates.motion(MotionLook::ChamferType(ChamferType::Two));
    for text in ["Distance 1", "Distance 2", "Flip sides"] {
        assert!(shows(&plates, text), "{text}");
    }
    plates.input(MotionField::ChamferSecond, "3");
    let chamfer = drafted(&plates).expect("a draft");
    let ChamferSize::Two(a, b) = &chamfer.distances else {
        panic!("two distances: {:?}", chamfer.distances);
    };
    assert_eq!((a.value, b.value), (1.0, 3.0));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(!faces_way(&plates, plate, slope), "not at 45°");
    plates.motion(MotionLook::Flip);
    assert!(drafted(&plates).unwrap().flip);

    plates.motion(MotionLook::ChamferType(ChamferType::Angle));
    assert!(shows(&plates, "Angle"));
    plates.input(MotionField::ChamferAngle, "90");
    assert!(!plates.doc.motion_ready(), "under 90°");
    let state = plates.doc.motion_state().unwrap();
    let error = state.fields[MotionField::ChamferAngle.index()].error;
    assert!(error.is_some(), "{error:?}");
    assert!(
        shows(&plates, "under 90°"),
        "{:?}",
        screen_texts(&plates.doc)
    );
    plates.input(MotionField::ChamferAngle, "30");
    assert!(plates.doc.motion_ready());
    let chamfer = drafted(&plates).expect("a draft");
    assert!(matches!(&chamfer.distances,
        ChamferSize::Angle(d, a) if d.value == 1.0 && (a.value - 30f64.to_radians()).abs() < 1e-12));
    assert!(chamfer.flip);
    // Back to Equal, which is stored unflipped.
    plates.motion(MotionLook::ChamferType(ChamferType::Equal));
    assert!(!drafted(&plates).unwrap().flip);
    assert!(!shows(&plates, "Flip sides"));
    plates.motion(MotionLook::Chain);
    let chamfer = drafted(&plates).expect("a draft");
    assert!(!chamfer.chains);
    plates.answer();
    assert!(faces_way(&plates, plate, slope));
    assert!(shows(&plates, "1 edge · Equal · 1 mm"));

    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (_, kind) = plates.last_feature();
    assert_eq!(kind, FeatureKind::Chamfer(chamfer));
    plates.answer();
    assert!(faces_way(&plates, plate, slope));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
    plates.answer();

    // An edge cut off in the preview is taken out by its row's cross.
    plates.doc.look(Look::StartChamfer);
    click_edge(&mut plates, plate, FRONT);
    plates.answer();
    let edge = edges(&plates)[0];
    plates.motion(MotionLook::DropEdge(edge));
    assert!(edges(&plates).is_empty());
    assert!(plates.doc.motion.as_ref().unwrap().bodies.is_empty());
    assert!(shows(&plates, "pick the edges to chamfer"));
}

/// Once an edge is picked, the edges of other bodies neither light nor
/// are taken, the status bar saying why; Objects' rows and the Edges
/// field don't pick bodies.
#[test]
fn a_chamfer_s_edges_are_all_on_one_body() {
    let mut plates = plates();
    let [plate, right, _] = plates.bodies;
    plates.doc.look(Look::StartChamfer);
    click_edge(&mut plates, plate, FRONT);
    // The right disc's rim, about (20, 0), 15 up.
    let index = plates.doc.feed.pick_index();
    let snaps = index.picking().snaps();
    let rim = (0..snaps.len() as u32)
        .find(|&edge| {
            index.body(Picked::Edge(edge)) == Some(right)
                && snaps[edge as usize]
                    .is_some_and(|at| DVec3::from(at).distance(DVec3::new(20.0, 0.0, 15.0)) < 1e-9)
        })
        .expect("the disc's top rim");
    let pick = Pick {
        model: index.model(),
        target: Picked::Edge(rim),
        body: right,
        at: DVec3::new(25.0, 0.0, 15.0),
        snap: None,
    };
    assert!(
        !plates.doc.refs_take::<varde_document::EdgeRef>(pick),
        "not lit"
    );
    plates.doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("A chamfer's edges are all on one body: pick edges of Body 1")
    );
    assert_eq!(edges(&plates).len(), 1);
    // A body's row picks nothing.
    plates.doc.look(Look::ClickBody {
        body: right,
        add: false,
    });
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate]);
    // With the plate's edge out, the disc's rim is taken, Ø10 beside it.
    let edge = edges(&plates)[0];
    plates.motion(MotionLook::DropEdge(edge));
    assert!(plates.doc.refs_take::<varde_document::EdgeRef>(pick));
    plates.doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [right]);
    assert!(shows(&plates, "Ø10 mm"));
    // The Edges field clicked stops picking, and again starts.
    plates.motion(MotionLook::Picking(MotionPick::Edges));
    assert_eq!(picking(&plates), MotionPick::Nothing);
    plates.motion(MotionLook::Picking(MotionPick::Bodies));
    assert_eq!(picking(&plates), MotionPick::Nothing);
    plates.motion(MotionLook::Picking(MotionPick::Edges));
    assert_eq!(picking(&plates), MotionPick::Edges);
}

/// The example plate with "Chamfer 1" of its front top edge, Equal 1 mm:
/// the editor and the chamfer.
fn chamfered() -> (Editor, FeatureId) {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartChamfer);
    click_edge(&mut plates, plate, FRONT);
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    let (id, _) = plates.last_feature();
    (Editor::new(plates.doc.editor.document().clone()), id)
}

/// Editing a chamfer from the Timeline opens it with its edges and
/// values, its preview the chamfer; Cancel leaves no trace; another
/// distance and OK sets it as one undo step; with its edges all taken
/// out, it shows the body as of the feature, by a move of nothing.
#[test]
fn editing_a_chamfer_from_the_timeline_cancel_and_undo() {
    let (editor, id) = chamfered();
    let (doc, requests) = holding(editor.document().clone());
    let plate = editor.document().bodies()[0].id;
    let mut plates = Plates {
        doc,
        requests,
        bodies: [plate; 3],
    };
    let before = plates.doc.editor.document().clone();
    let FeatureKind::Chamfer(stored) = before.feature(id).unwrap().kind.clone() else {
        panic!("a chamfer");
    };
    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Chamfer);
    assert_eq!(session.feature, Some(id));
    assert_eq!(session.blend.edges.refs, stored.edges);
    assert_eq!(session.bodies, [plate]);
    assert_eq!(picking(&plates), MotionPick::Edges);
    assert!(shows(&plates, "Chamfer 1"));
    assert_eq!(session.chamfer(), Some(stored.clone()));
    let (feature, kind) = plates.last_draft().expect("a draft");
    assert_eq!(
        (feature, kind),
        (Some(id), FeatureKind::Chamfer(stored.clone()))
    );
    plates.input(MotionField::ChamferDistance, "2");
    plates.motion(MotionLook::Cancel);
    assert!(plates.doc.motion.is_none());
    assert_eq!(*plates.doc.editor.document(), before);

    plates.doc.look(Look::EditFeature(id));
    plates.input(MotionField::ChamferDistance, "2");
    // OK waits on the stand-in's failure; Add anyway sets it.
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let FeatureKind::Chamfer(set) = &plates.doc.editor.document().feature(id).unwrap().kind else {
        panic!("a chamfer");
    };
    assert!(matches!(&set.distances, ChamferSize::Equal(d) if d.value == 2.0));
    assert_eq!(
        plates.doc.editor.document().features().len(),
        before.features().len()
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);

    // Its edges taken out: the body as of the feature, edges to pick.
    plates.doc.look(Look::EditFeature(id));
    plates.motion(MotionLook::DropEdge(stored.edges[0]));
    let (feature, kind) = plates.last_draft().expect("a draft");
    assert_eq!(feature, Some(id));
    let FeatureKind::Move(moved) = kind else {
        panic!("a move of nothing: {kind:?}");
    };
    assert_eq!(moved.bodies, [plate]);
    assert_eq!(moved.offset_vector(), DVec3::ZERO);
    assert!(!plates.doc.motion_ready());
}

/// An edge on a body an undo takes away is kept, said to be gone,
/// nothing previewed or committed, until a redo brings it back.
#[test]
fn an_edge_an_undo_takes_away_is_said_to_be_gone() {
    let mut plates = plates();
    let last = later_disc(&mut plates);
    plates.doc.look(Look::StartChamfer);
    // The later disc's rim, about (0, 30), 5 up.
    let index = plates.doc.feed.pick_index();
    let snaps = index.picking().snaps();
    let rim = (0..snaps.len() as u32)
        .find(|&edge| {
            index.body(Picked::Edge(edge)) == Some(last)
                && snaps[edge as usize]
                    .is_some_and(|at| DVec3::from(at).distance(DVec3::new(0.0, 30.0, 5.0)) < 1e-9)
        })
        .expect("the disc's top rim");
    plates.click_at(last, Picked::Edge(rim), DVec3::new(5.0, 30.0, 5.0));
    assert_eq!(edges(&plates).len(), 1);
    assert!(plates.doc.motion_ready());
    plates.doc.update(Edit::Undo);
    assert!(plates.doc.editor.document().body(last).is_none());
    assert_eq!(edges(&plates).len(), 1, "kept");
    assert!(!plates.doc.motion_ready());
    assert!(shows(&plates, "A picked edge is gone"));
    assert!(drafted(&plates).is_none());
    plates.doc.update(Edit::Redo);
    assert!(plates.doc.motion_ready());
    assert!(drafted(&plates).is_some());
}

/// The rail's Modify set and the toolbar offer Chamfer, as the mock's;
/// it starts with the edges selected that it takes; `C` again backs out.
#[test]
fn chamfer_takes_the_edges_selected_and_c_backs_out() {
    let (mut plates, plate) = plate();
    plates.doc.pick.selection = Selection::new(SelectionMode::Edges { tangent: false });
    for ends in [FRONT, RIGHT] {
        let pick = edge_pick(&plates, plate, ends);
        plates.doc.look(Look::ClickModel {
            pick: Some(pick),
            add: true,
            double: false,
        });
    }
    assert!(shows(&plates, "Chamfer"));
    plates.doc.look(Look::StartChamfer);
    assert_eq!(edges(&plates).len(), 2);
    assert!(drafted(&plates).is_some());
    key_in(&mut plates.doc, character("c"));
    assert!(plates.doc.motion.is_none());
}

/// A slot 10 long and 6 wide (lines joined by half circles they run on
/// into smoothly), 5 tall from z 0, as "Body 1".
pub(super) fn slot() -> (Plates, BodyId) {
    use varde_document::{Command, Extent, Extrude, Operation, OriginPlane, Plane};
    use varde_sketch::Curve;
    let mut editor = Editor::new(Document::default());
    let plane = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = varde_sketch::Sketch::default();
    let mut at = |x: f64, y: f64| sketch.add_point(glam::DVec2::new(x, y)).unwrap();
    let [a, b, c, d] = [(0.0, -3.0), (10.0, -3.0), (10.0, 3.0), (0.0, 3.0)].map(|(x, y)| at(x, y));
    let [left, right] = [(0.0, 0.0), (10.0, 0.0)].map(|(x, y)| at(x, y));
    for curve in [
        Curve::Line { start: a, end: b },
        Curve::Arc {
            center: right,
            start: b,
            end: c,
        },
        Curve::Line { start: c, end: d },
        Curve::Arc {
            center: left,
            start: d,
            end: a,
        },
    ] {
        sketch.add_curve(curve, false).unwrap();
    }
    let profiles = sketch.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let ask = Extent::ask(&editor.document().design());
    let extrude = Extrude {
        taper: None,
        sketch: feature,
        regions,
        extent: Extent::OneSide(varde_expr::Value::new("5", &ask).unwrap()),
        operation: Operation::NewBody(BodyId::NEW),
        flip: false,
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    let body = editor.document().bodies()[0].id;
    let (doc, requests) = holding(editor.document().clone());
    let plates = Plates {
        doc,
        requests,
        bodies: [body; 3],
    };
    (plates, body)
}

/// With Tangent chain on, the slot's top front line lights with the
/// rest of its rim under the cursor and once picked, and a click on an
/// arc of that rim takes the line out; off, the line alone.
#[test]
fn a_tangent_chain_lights_whole_and_a_click_on_it_takes_its_edge_out() {
    let (mut plates, body) = slot();
    plates.doc.look(Look::StartChamfer);
    let line = edge_pick(&plates, body, ([0.0, -3.0, 5.0], [10.0, -3.0, 5.0]));
    let Picked::Edge(edge) = line.target else {
        unreachable!()
    };
    let rim = plates.doc.feed.pick_index().tangent_chain(edge).to_vec();
    assert_eq!(rim.len(), 4, "the top rim: two lines and two arcs");
    let outlined = |plates: &Plates| {
        let highlight = plates.doc.motion_highlight().expect("lit");
        let mut edges = highlight.highlights.outlined.clone();
        edges.sort_unstable();
        edges
    };
    let selected = |plates: &Plates| {
        let highlight = plates.doc.motion_highlight().expect("lit");
        let mut edges = highlight.highlights.selected_edges.clone();
        edges.sort_unstable();
        edges
    };
    plates.doc.look(Look::Hover(Some(line)));
    assert_eq!(outlined(&plates), rim);
    plates.doc.look(Look::ClickModel {
        pick: Some(line),
        add: false,
        double: false,
    });
    assert_eq!(edges(&plates).len(), 1);
    assert_eq!(selected(&plates), rim);
    // Off, the line alone.
    plates.motion(MotionLook::Chain);
    assert_eq!(selected(&plates), [edge]);
    assert_eq!(outlined(&plates), [edge]);
    plates.motion(MotionLook::Chain);
    // A click on an arc of its chain takes the line out.
    let arc = *rim
        .iter()
        .find(|&&other| {
            other != edge && {
                let index = plates.doc.feed.pick_index();
                index
                    .chain_keys(other)
                    .and_then(|keys| index.edge_ends(other, &keys))
                    .is_none()
            }
        })
        .expect("an arc");
    let pick = Pick {
        target: Picked::Edge(arc),
        at: DVec3::new(13.0, 0.0, 5.0),
        ..line
    };
    plates.doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
    assert!(edges(&plates).is_empty());
}

/// Right after a move set up to lift the plate is cancelled, the model
/// shown is still its preview: an edge clicked there is refused as out
/// of date (its point would be the lifted one) and doesn't light; once
/// the document's model shows, it's picked where it is.
#[test]
fn a_chamfer_s_edges_wait_for_another_session_s_preview_to_go() {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartMove);
    plates.input(MotionField::Offset(varde_document::Axis3::Z), "100");
    plates.answer();
    plates.motion(MotionLook::Cancel);
    plates.doc.look(Look::StartChamfer);
    let up = ([-30.0, 20.0, 110.0], [30.0, 20.0, 110.0]);
    let pick = edge_pick(&plates, plate, up);
    assert!(!plates.doc.takes_reference(pick));
    click_edge(&mut plates, plate, up);
    assert!(edges(&plates).is_empty(), "refused");
    assert!(shows(&plates, "out of date"));
    plates.answer();
    assert!(straight(&plates, plate, up.0, up.1).is_none());
    click_edge(&mut plates, plate, BACK);
    let picked = edges(&plates);
    assert_eq!(picked.len(), 1);
    assert_eq!(picked[0].near.z, 10.0);
}

/// At 1280 px wide everything on the toolbar shows whole, apart: the
/// model's operations (Chamfer among them, Mirror on the rail as the
/// mock's bar has it) but the last, Parameters, which the bar cuts off,
/// and in a session Cancel and the origins it picks rather than the
/// operations, as the mock's.
#[test]
fn the_toolbar_fits_at_1280_px() {
    use crate::tests::{shown, texts};
    use varde_view::Mode;
    let (mut plates, _) = plate();
    let fits = |plates: &Plates| {
        let mut renderer = varde_view::probe::renderer();
        let size = iced::Size::new(1280.0, 800.0);
        let mut ui = shown(plates.doc.view_in(Mode::Light), size, &mut renderer);
        let mut on: Vec<_> = (texts(&mut ui, &renderer).into_iter())
            .filter(|t| t.bounds.y < 40.0)
            .collect();
        on.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x));
        // Parameters, last on the model bar, is past what 1280 px leaves:
        // the bar cuts it off rather than dropping it.
        on.retain(|t| t.text != "Parameters");
        let mut end = 0.0;
        for t in &on {
            assert!(t.bounds.width > 2.0, "{} squeezed: {on:?}", t.text);
            assert!(t.bounds.x >= end, "{} overlaps: {on:?}", t.text);
            end = t.bounds.x + t.bounds.width;
        }
        // Undo, Redo, a rule and the theme's button (28, 28, 9 and 28
        // px, with the gaps) are right of them, all whole.
        assert!(end < 1280.0 - 120.0, "{on:?}");
        on.into_iter().map(|t| t.text).collect::<Vec<_>>()
    };
    let idle = fits(&plates);
    assert!(idle.iter().any(|t| t == "Chamfer") && idle.iter().any(|t| t == "Measure"));
    plates.doc.look(Look::StartMove);
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    let moving = fits(&plates);
    assert!(moving.iter().any(|t| t == "Z axis") && moving.iter().any(|t| t == "Cancel"));
    plates.motion(MotionLook::Cancel);
    plates.doc.look(Look::StartChamfer);
    let chamfering = fits(&plates);
    assert!(chamfering.iter().any(|t| t == "New chamfer"));
    assert!(!chamfering.iter().any(|t| t == "Combine"));
    // Fillet isn't on the bar (it wouldn't fit): its session's bar is
    // Cancel too.
    assert!(!idle.iter().any(|t| t == "Fillet"));
    plates.doc.look(Look::StartFillet);
    let filleting = fits(&plates);
    assert!(filleting.iter().any(|t| t == "New fillet"));
    assert!(filleting.iter().any(|t| t == "Cancel"));

    // A chamfer with a long name (a file's: the app names them
    // "Chamfer 1") edited: its name in the pill is cut short to fit.
    let (editor, id) = chamfered();
    let bytes = editor.document().to_postcard();
    let was = b"\x09Chamfer 1";
    let at = (bytes.windows(was.len()))
        .position(|window| window == was)
        .expect("the chamfer's name");
    let long = "Chamfer of the front top edge of the base plate, before the holes and the \
                pockets are cut into it";
    assert!(long.len() < 128);
    let mut changed = bytes[..at].to_vec();
    changed.push(long.len() as u8);
    changed.extend_from_slice(long.as_bytes());
    changed.extend_from_slice(&bytes[at + was.len()..]);
    let document = Document::from_postcard(&changed).unwrap();
    assert_eq!(document.feature(id).unwrap().name, long);
    let (doc, requests) = holding(document);
    let plate = editor.document().bodies()[0].id;
    let mut plates = Plates {
        doc,
        requests,
        bodies: [plate; 3],
    };
    plates.doc.look(Look::EditFeature(id));
    let shown = fits(&plates);
    assert!(
        shown
            .iter()
            .any(|t| t.starts_with("Chamfer of the") && t.ends_with('…')),
        "{shown:?}"
    );
}

/// A combine into a body merged into another before it fails, as
/// regeneration has it: the edges picked don't follow their body into
/// it, even before the model shown knows it fails.
#[test]
fn edges_dont_follow_their_body_into_a_combine_that_fails() {
    use varde_document::{BodyOp, Combine};
    let mut plates = plates();
    let [plate, right, left] = plates.bodies;
    let combine = |target, tool| Combine {
        target,
        tools: vec![tool],
        op: BodyOp::Union,
        keep_tools: false,
    };
    let add = plates
        .doc
        .editor
        .document()
        .add_feature(combine(left, right).into());
    plates.doc.apply(add);
    plates.doc.sync();
    plates.answer();
    plates.doc.look(Look::StartChamfer);
    click_edge(&mut plates, plate, FRONT);
    assert_eq!(edges(&plates)[0].body, plate);
    // Right is in Left now: this one fails.
    let add = plates
        .doc
        .editor
        .document()
        .add_feature(combine(right, plate).into());
    plates.doc.apply(add);
    plates.doc.sync();
    assert_eq!(edges(&plates)[0].body, plate, "before the answer");
    plates.answer();
    assert_eq!(edges(&plates)[0].body, plate);
    assert!(plates.doc.motion_ready());
}

pub(super) mod fuzz;

/// A chamfer as its session made it, [`chamfered`], held as plates
/// are, and its id.
pub(super) fn made() -> (Plates, FeatureId) {
    let (editor, id) = chamfered();
    let plate = editor.document().bodies()[0].id;
    let (doc, requests) = holding(editor.document().clone());
    let plates = Plates {
        doc,
        requests,
        bodies: [plate; 3],
    };
    (plates, id)
}
