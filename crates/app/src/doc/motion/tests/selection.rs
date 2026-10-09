//! Operations started with something selected in the model, and the
//! toolbar offering those that take it: edges, faces, both, vertices,
//! bodies, or a sketch selected in the Timeline.

use glam::{DVec2, DVec3};
use varde_document::{AxisLine, AxisRef, BodyId, FeatureId, FeatureKind, PlaneRef};
use varde_regen::Summary;
use varde_view::{
    Look, MotionKind, MotionLook, MotionPick, Pick, Picked, RevolveLook, Selection, SelectionMode,
};

use super::Plates;
use super::chamfer::{FRONT, RIGHT, edge_pick, edges, plate};
use super::face_session::picked_faces;
use super::near;

/// Selects `picks` in the model, edges or faces as `mode` says.
fn select(plates: &mut Plates, mode: SelectionMode, picks: &[Pick]) {
    plates.doc.pick.selection = Selection::new(mode);
    for &pick in picks {
        plates.doc.look(Look::ClickModel {
            pick: Some(pick),
            add: true,
            double: false,
        });
    }
}

/// The plate's top face, as the cursor picks it.
fn top_face(plates: &Plates, plate: BodyId) -> Pick {
    super::face_session::face_pick(plates, plate, DVec3::Z, 10.0, DVec3::new(20.0, 10.0, 10.0))
}

/// The toolbar's labels, laid out wide enough for all of them.
fn bar(plates: &Plates) -> Vec<String> {
    use crate::tests::{shown, texts};
    let mut renderer = varde_view::probe::renderer();
    let size = iced::Size::new(2560.0, 800.0);
    let mut ui = shown(
        plates.doc.view_in(varde_view::Mode::Light),
        size,
        &mut renderer,
    );
    (texts(&mut ui, &renderer).into_iter())
        .filter(|t| t.bounds.y < 40.0)
        .map(|t| t.text)
        .collect()
}

/// Asserts the toolbar has the operations `has` between the context and
/// Undo, in that order, and Measure after them.
fn offers(plates: &Plates, has: &[&str]) {
    let bar = bar(plates);
    let ops: Vec<&str> = (bar.iter())
        .map(String::as_str)
        .filter(|text| OPS.contains(text))
        .collect();
    let mut wanted = has.to_vec();
    wanted.push("Measure");
    assert_eq!(ops, wanted, "{bar:?}");
}

/// Every operation the model's toolbars offer.
const OPS: [&str; 21] = [
    "Sketch",
    "Sketch on face",
    "Extrude",
    "Revolve",
    "Sweep",
    "Loft",
    "Offset",
    "Fillet",
    "Chamfer",
    "Shell",
    "Draft",
    "Combine",
    "Move",
    "Mirror",
    "Pattern",
    "Circular pattern",
    "Align",
    "Scale",
    "Split",
    "Edit sketch",
    "Measure",
];

/// The plate's hole's rim at the top, as the cursor picks it.
fn rim(plates: &Plates, plate: BodyId) -> Pick {
    let index = plates.doc.feed.pick_index();
    let rim = (0..index.mesh().edge_count() as u32)
        .find(|&edge| {
            index.body(Picked::Edge(edge)) == Some(plate)
                && (index.chain_keys(edge))
                    .is_some_and(|keys| index.edge_ends(edge, &keys).is_none())
                && index.chain_point(edge).is_some_and(|at| at.z > 5.0)
        })
        .expect("the hole's rim");
    Pick {
        model: index.model(),
        target: Picked::Edge(rim),
        body: plate,
        at: index.chain_point(rim).unwrap(),
        snap: None,
    }
}

/// The plate's hole's wall, as the cursor picks it.
fn wall(plates: &Plates, plate: BodyId) -> Pick {
    let index = plates.doc.feed.pick_index();
    let wall = (index.body_faces(plate))
        .find(|&face| {
            matches!(
                index.picking().faces()[face as usize].summary,
                Summary::Cylinder { .. }
            )
        })
        .expect("the hole");
    Pick {
        model: index.model(),
        target: Picked::Face(wall),
        body: plate,
        at: DVec3::new(8.0, 0.0, 5.0),
        snap: None,
    }
}

/// The plate's top front left corner, as the cursor picks it.
fn corner(plates: &Plates, plate: BodyId) -> Pick {
    let index = plates.doc.feed.pick_index();
    let at = DVec3::new(-30.0, -20.0, 10.0);
    let corner = (0..10_000)
        .find(|&corner| {
            index
                .corner_point(corner)
                .is_some_and(|point| near(point, at))
        })
        .expect("the corner");
    Pick {
        model: index.model(),
        target: Picked::Vertex(corner),
        body: plate,
        at,
        snap: None,
    }
}

/// The toolbar offers the operations taking what's selected, those
/// fitting it first, as `toolbar::selection_bar` lists them.
#[test]
fn the_toolbar_offers_what_takes_the_selection() {
    let (mut plates, plate) = plate();
    offers(
        &plates,
        &[
            "Sketch", "Extrude", "Revolve", "Chamfer", "Combine", "Move", "Pattern",
        ],
    );
    let edges_mode = SelectionMode::Edges { tangent: false };
    // One straight edge: an axis or direction.
    let front = edge_pick(&plates, plate, FRONT);
    select(&mut plates, edges_mode, &[front]);
    offers(
        &plates,
        &[
            "Sketch", "Revolve", "Fillet", "Chamfer", "Sweep", "Move", "Pattern",
        ],
    );
    // Two: no Pattern.
    let right = edge_pick(&plates, plate, RIGHT);
    select(&mut plates, edges_mode, &[front, right]);
    offers(
        &plates,
        &["Sketch", "Revolve", "Fillet", "Chamfer", "Sweep", "Move"],
    );
    // A round edge: no Revolve; a rim, blended first.
    let rim = rim(&plates, plate);
    select(&mut plates, edges_mode, &[rim]);
    offers(
        &plates,
        &[
            "Fillet",
            "Chamfer",
            "Sketch",
            "Sweep",
            "Circular pattern",
            "Move",
        ],
    );
    select(&mut plates, edges_mode, &[front, rim]);
    offers(&plates, &["Sketch", "Fillet", "Chamfer", "Sweep", "Move"]);
    // A flat face.
    let top = top_face(&plates, plate);
    select(&mut plates, SelectionMode::Faces, &[top]);
    offers(
        &plates,
        &[
            "Sketch on face",
            "Extrude",
            "Offset",
            "Fillet",
            "Chamfer",
            "Shell",
            "Draft",
            "Move",
            "Mirror",
        ],
    );
    // A curved one: neither a sketch's plane nor a mirror's.
    let wall = wall(&plates, plate);
    select(&mut plates, SelectionMode::Faces, &[top, wall]);
    offers(
        &plates,
        &[
            "Extrude", "Offset", "Fillet", "Chamfer", "Shell", "Draft", "Move",
        ],
    );
    // Edges and faces.
    select(&mut plates, SelectionMode::Any, &[top, front]);
    offers(&plates, &["Fillet", "Chamfer", "Sketch", "Move"]);
    // A vertex.
    let corner = corner(&plates, plate);
    select(&mut plates, SelectionMode::Any, &[corner]);
    offers(&plates, &["Sketch", "Align", "Scale", "Move"]);
    // A body.
    select(&mut plates, SelectionMode::Bodies, &[top]);
    offers(
        &plates,
        &[
            "Sketch", "Move", "Mirror", "Pattern", "Combine", "Split", "Scale", "Shell",
        ],
    );
    // A sketch in the Timeline.
    let sketch = sketch_of(&plates);
    plates.doc.look(Look::SelectFeature(sketch));
    offers(
        &plates,
        &[
            "Extrude", "Revolve", "Sweep", "Loft", "Sketch", "Chamfer", "Combine", "Move",
            "Pattern",
        ],
    );
}

/// The example's sketch.
fn sketch_of(plates: &Plates) -> FeatureId {
    let document = plates.doc.editor.document();
    (document.features().iter())
        .find(|feature| matches!(feature.kind, FeatureKind::Sketch { .. }))
        .expect("the plate's sketch")
        .id
}

/// A fillet or chamfer started with faces selected takes the faces,
/// which stand for the edges around them, and no edges of its own.
#[test]
fn a_blend_takes_the_faces_selected() {
    for start in [Look::StartChamfer, Look::StartFillet] {
        let (mut plates, plate) = plate();
        let pick = top_face(&plates, plate);
        select(&mut plates, SelectionMode::Faces, &[pick]);
        plates.doc.look(start);
        let session = plates.doc.motion.as_ref().unwrap();
        assert!(edges(&plates).is_empty());
        assert_eq!(session.faces.refs.len(), 1);
        assert_eq!(session.faces.refs[0].body, plate);
        assert_eq!(session.bodies, [plate]);
        assert!(session.kind().is_some(), "whole with a face alone");
    }
}

/// A sweep started with edges selected takes them as its path.
#[test]
fn a_sweep_takes_the_edges_selected_as_its_path() {
    let (mut plates, plate) = plate();
    let picks = [FRONT, RIGHT].map(|ends| edge_pick(&plates, plate, ends));
    select(&mut plates, SelectionMode::Edges { tangent: false }, &picks);
    plates.doc.look(Look::StartSweep);
    assert_eq!(plates.doc.motion.as_ref().unwrap().kind, MotionKind::Sweep);
    assert_eq!(edges(&plates).len(), 2);
}

/// A mirror started with a flat face selected mirrors its body across
/// it; one with a round face says why it can't.
#[test]
fn a_mirror_takes_the_face_selected_as_its_plane() {
    let (mut plates, plate) = plate();
    let pick = top_face(&plates, plate);
    select(&mut plates, SelectionMode::Faces, &[pick]);
    plates.doc.look(Look::StartMirror);
    let session = plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.bodies, [plate]);
    let Some(varde_document::PlaneRef::Face(face)) = session.plane else {
        panic!("a face plane: {:?}", session.plane);
    };
    assert_eq!(face.body, plate);
    assert!(plates.doc.motion_ready());
    plates.motion(MotionLook::Cancel);
    let index = plates.doc.feed.pick_index();
    let hole = (index.body_faces(plate))
        .find(|&face| {
            matches!(
                index.picking().faces()[face as usize].summary,
                Summary::Cylinder { .. }
            )
        })
        .expect("the hole");
    let pick = Pick {
        model: index.model(),
        target: Picked::Face(hole),
        body: plate,
        at: DVec3::new(8.0, 0.0, 5.0),
        snap: None,
    };
    select(&mut plates, SelectionMode::Faces, &[pick]);
    plates.doc.look(Look::StartMirror);
    assert_eq!(plates.doc.motion.as_ref().unwrap().plane, None);
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a flat face can be the mirror plane")
    );
}

/// A move or linear pattern started with a straight edge selected
/// takes it as its axis; with a face selected, the face says only which
/// body.
#[test]
fn a_move_or_pattern_takes_the_edge_selected_as_its_axis() {
    for start in [Look::StartMove, Look::StartPattern] {
        let (mut plates, plate) = plate();
        let front = edge_pick(&plates, plate, FRONT);
        select(
            &mut plates,
            SelectionMode::Edges { tangent: false },
            &[front],
        );
        plates.doc.look(start.clone());
        let session = plates.doc.motion.as_ref().unwrap();
        assert_eq!(session.bodies, [plate]);
        assert!(
            matches!(session.axis, Some(AxisRef::Edge(_))),
            "{:?}",
            session.axis
        );
    }
    let (mut plates, plate) = plate();
    let wall = wall(&plates, plate);
    select(&mut plates, SelectionMode::Faces, &[wall]);
    plates.doc.look(Look::StartCircularPattern);
    let session = plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.bodies, [plate]);
    assert_eq!(
        session.axis,
        Some(AxisRef::Origin(varde_document::Axis3::Z))
    );
}

/// A circular pattern started with a round edge selected turns about
/// it.
#[test]
fn a_circular_pattern_takes_the_round_edge_selected_as_its_axis() {
    let (mut plates, plate) = plate();
    let rim = rim(&plates, plate);
    select(&mut plates, SelectionMode::Edges { tangent: false }, &[rim]);
    plates.doc.look(Look::StartCircularPattern);
    let session = plates.doc.motion.as_ref().unwrap();
    assert!(
        matches!(session.axis, Some(AxisRef::Edge(_))),
        "{:?}",
        session.axis
    );
}

/// A draft started with two faces or more selected takes the first, if
/// flat, as its neutral plane and drafts the rest; with one, drafts it.
#[test]
fn a_draft_takes_the_first_face_selected_as_its_neutral_plane() {
    let (mut plates, plate) = plate();
    let top = top_face(&plates, plate);
    let front = front_face(&plates, plate);
    select(&mut plates, SelectionMode::Faces, &[top, front]);
    plates.doc.look(Look::StartDraft);
    let session = plates.doc.motion.as_ref().unwrap();
    assert!(
        matches!(session.plane, Some(PlaneRef::Face(_))),
        "{:?}",
        session.plane
    );
    let faces = picked_faces(&plates);
    assert_eq!(faces.len(), 1);
    assert!(faces[0].near.y < -19.0, "the front: {faces:?}");
    // One face selected: it's drafted.
    plates.motion(MotionLook::Cancel);
    select(&mut plates, SelectionMode::Faces, &[front]);
    plates.doc.look(Look::StartDraft);
    assert_eq!(picked_faces(&plates).len(), 1);
    assert_eq!(
        plates.doc.motion.as_ref().unwrap().plane,
        Some(PlaneRef::Origin(varde_document::OriginPlane::XY))
    );
}

/// The plate's front side, as the cursor picks it.
fn front_face(plates: &Plates, plate: BodyId) -> Pick {
    super::face_session::face_pick(plates, plate, -DVec3::Y, 20.0, DVec3::new(0.0, -20.0, 5.0))
}

/// A split started with a face alone selected cuts with it, its body
/// picked next.
#[test]
fn a_split_takes_the_face_selected_as_its_tool() {
    let (mut plates, plate) = plate();
    let top = top_face(&plates, plate);
    select(&mut plates, SelectionMode::Faces, &[top]);
    plates.doc.look(Look::StartSplit);
    let session = plates.doc.motion.as_ref().unwrap();
    assert!(session.bodies.is_empty());
    assert_eq!(session.picking, MotionPick::Bodies);
    assert!(session.split.tool().is_some());
}

/// An align or a scale started with a vertex selected takes it as its
/// point.
#[test]
fn an_align_or_scale_takes_the_vertex_selected_as_its_point() {
    for start in [Look::StartAlign, Look::StartScale] {
        let (mut plates, plate) = plate();
        let corner = corner(&plates, plate);
        select(&mut plates, SelectionMode::Any, &[corner]);
        plates.doc.look(start.clone());
        let session = plates.doc.motion.as_ref().unwrap();
        assert_eq!(session.bodies, [plate]);
        let picked = match start {
            Look::StartAlign => session.align.marks[0][0].is_some(),
            _ => session.scale.about != varde_document::PointRef::Origin,
        };
        assert!(picked, "{start:?}");
        assert_eq!(plates.doc.notice, None);
    }
}

/// A revolve started with a straight edge selected takes it as its
/// axis once its profile is picked.
#[test]
fn a_revolve_takes_the_edge_selected_as_its_axis_once_its_profile_is_picked() {
    let (mut plates, plate) = plate();
    let bottom = edge_pick(&plates, plate, ([-30.0, -20.0, 0.0], [30.0, -20.0, 0.0]));
    select(
        &mut plates,
        SelectionMode::Edges { tangent: false },
        &[bottom],
    );
    let sketch = sketch_of(&plates);
    // Shown, its regions are picked.
    (plates.doc.editor)
        .apply(varde_document::Command::SetFeatureVisible(sketch, true))
        .unwrap();
    plates.doc.look(Look::StartRevolve);
    assert_eq!(plates.doc.revolve.as_ref().unwrap().axis, None);
    let Some(FeatureKind::Sketch { sketch: drawn, .. }) = plates
        .doc
        .editor
        .document()
        .feature(sketch)
        .map(|f| &f.kind)
    else {
        panic!("a sketch");
    };
    let region = (drawn.profiles().unwrap())
        .region_at(DVec2::new(20.0, 0.0))
        .unwrap();
    plates
        .doc
        .look(Look::Revolve(RevolveLook::PickRegion { sketch, region }));
    let axis = plates.doc.revolve.as_ref().unwrap().axis;
    assert!(
        matches!(axis, Some(AxisLine::Edge(_))),
        "{axis:?} {:?}",
        plates.doc.notice
    );
}

/// The measure tool started with one or two things selected measures
/// them.
#[test]
fn measure_takes_what_is_selected() {
    let (mut plates, plate) = plate();
    let top = top_face(&plates, plate);
    let front = edge_pick(&plates, plate, FRONT);
    select(&mut plates, SelectionMode::Any, &[top, front]);
    plates.doc.look(Look::StartMeasure);
    let measure = plates.doc.measure.as_ref().unwrap();
    assert!(measure.inspect().is_some_and(|(_, b)| b.is_some()));
}

/// A fillet or chamfer started with edges and faces selected takes both:
/// an edge selected around a face selected is left out, listed and lit
/// in the exclusions' purple.
#[test]
fn a_blend_takes_edges_and_faces_selected_together() {
    let (mut plates, plate) = plate();
    let top = top_face(&plates, plate);
    let rim = rim(&plates, plate);
    select(&mut plates, SelectionMode::Any, &[rim, top]);
    plates.doc.look(Look::StartFillet);
    assert_eq!(edges(&plates).len(), 1);
    let session = plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.faces.refs.len(), 1);
    let listed = plates.doc.blend_edges(session);
    assert_eq!(listed.faces.len(), 1);
    assert!(listed.edges[0].excluded, "{listed:?}");
    let [taken, out] = plates.doc.blend_lit_parts();
    assert_eq!(out, [rim.target]);
    assert!(!taken.contains(&rim.target));
    assert!(taken.contains(&top.target));
    plates.doc.refresh_motion_highlight();
    let highlight = plates.doc.motion_highlight().unwrap();
    assert!(highlight.second_excluded);
    assert!(
        highlight
            .highlights
            .second_edges
            .contains(&match rim.target {
                Picked::Edge(edge) => edge,
                other => panic!("{other:?}"),
            })
    );
}

/// The plate with a sketch on XZ beside it: a rectangle from (10, 0) to
/// (20, 10), a line along its Y axis from (0, -5) to (0, 15), and an open
/// chain of two lines from (30, 0) by (40, 0) to (40, 10). The sketch,
/// the rectangle's lines, the line and the chain's.
struct Drawn {
    plates: Plates,
    sketch: FeatureId,
    rectangle: [varde_sketch::Id; 4],
    line: varde_sketch::Id,
    chain: [varde_sketch::Id; 2],
}

fn drawn() -> Drawn {
    use varde_document::{Command, OriginPlane, Plane};
    use varde_sketch::Curve;
    let (mut plates, _) = plate();
    let document = plates.doc.editor.document();
    plates
        .doc
        .apply(document.add_sketch(Plane::Origin(OriginPlane::XZ)));
    let sketch = plates.doc.editor.document().features().last().unwrap().id;
    let mut drawn = varde_sketch::Sketch::default();
    let mut at = |x: f64, y: f64| drawn.add_point(DVec2::new(x, y)).unwrap();
    let corners = [(10.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 10.0)].map(|(x, y)| at(x, y));
    let [low, high] = [(0.0, -5.0), (0.0, 15.0)].map(|(x, y)| at(x, y));
    let [c, d, e] = [(30.0, 0.0), (40.0, 0.0), (40.0, 10.0)].map(|(x, y)| at(x, y));
    let mut line = |start, end| drawn.add_curve(Curve::Line { start, end }, false).unwrap();
    let rectangle = [0, 1, 2, 3].map(|k| line(corners[k], corners[(k + 1) % 4]));
    let axis = line(low, high);
    let chain = [line(c, d), line(d, e)];
    plates.doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn),
    });
    plates.doc.sync();
    plates.answer();
    Drawn {
        plates,
        sketch,
        rectangle,
        line: axis,
        chain,
    }
}

/// Selects the curves `curves` of `sketch` in the model.
fn select_curves(plates: &mut Plates, sketch: FeatureId, curves: &[varde_sketch::Id]) {
    plates.doc.pick.selection = Selection::new(SelectionMode::Any);
    for &item in curves {
        plates.doc.look(Look::ClickSketch {
            item: varde_view::SketchItem { sketch, item },
            add: true,
        });
    }
}

/// With a sketch's curves selected, the toolbar offers what takes the
/// regions they bound or the chain they make, and Edit sketch; with a
/// line alone, Revolve about it rather than Extrude.
#[test]
fn the_toolbar_offers_what_takes_the_sketch_curves_selected() {
    let mut drawn = drawn();
    select_curves(&mut drawn.plates, drawn.sketch, &drawn.rectangle);
    offers(
        &drawn.plates,
        &["Extrude", "Revolve", "Sweep", "Split", "Edit sketch"],
    );
    select_curves(&mut drawn.plates, drawn.sketch, &[drawn.line]);
    offers(&drawn.plates, &["Revolve", "Sweep", "Split", "Edit sketch"]);
}

/// An extrude started with a sketch's curves selected takes the regions
/// they bound.
#[test]
fn an_extrude_takes_the_regions_the_curves_selected_bound() {
    let mut drawn = drawn();
    select_curves(&mut drawn.plates, drawn.sketch, &drawn.rectangle);
    drawn.plates.doc.look(Look::StartExtrude);
    let regions = &drawn.plates.doc.extrude.as_ref().unwrap().regions;
    assert_eq!(regions.source, Some(drawn.sketch));
    assert_eq!(regions.picked.len(), 1);
}

/// A revolve started with a sketch's curves selected takes the regions
/// they bound and the line among them bounding none as its axis; with a
/// line alone, that as its axis, its sketch the source.
#[test]
fn a_revolve_takes_the_curves_selected_as_its_regions_and_axis() {
    let mut drawn = drawn();
    let mut curves = drawn.rectangle.to_vec();
    curves.push(drawn.line);
    select_curves(&mut drawn.plates, drawn.sketch, &curves);
    drawn.plates.doc.look(Look::StartRevolve);
    let session = drawn.plates.doc.revolve.as_ref().unwrap();
    assert_eq!(session.regions.picked.len(), 1);
    assert_eq!(session.axis, Some(AxisLine::Curve(drawn.line)));
    drawn.plates.doc.look(Look::Revolve(RevolveLook::Cancel));
    select_curves(&mut drawn.plates, drawn.sketch, &[drawn.line]);
    drawn.plates.doc.look(Look::StartRevolve);
    let session = drawn.plates.doc.revolve.as_ref().unwrap();
    assert_eq!(session.regions.source, Some(drawn.sketch));
    assert!(session.regions.picked.is_empty());
    assert_eq!(session.axis, Some(AxisLine::Curve(drawn.line)));
}

/// A sweep started with a sketch's closed curves selected takes the
/// regions they bound as its profile; with an open chain, the chain as
/// its path.
#[test]
fn a_sweep_takes_the_curves_selected_as_its_profile_or_path() {
    let mut drawn = drawn();
    select_curves(&mut drawn.plates, drawn.sketch, &drawn.rectangle);
    drawn.plates.doc.look(Look::StartSweep);
    let session = drawn.plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.sweep.regions.picked.len(), 1);
    assert!(session.sweep.chains.is_empty());
    drawn.plates.motion(MotionLook::Cancel);
    select_curves(&mut drawn.plates, drawn.sketch, &drawn.chain);
    drawn.plates.doc.look(Look::StartSweep);
    let session = drawn.plates.doc.motion.as_ref().unwrap();
    assert!(session.sweep.regions.picked.is_empty());
    assert_eq!(session.sweep.chains.len(), 1, "one chain through both");
}

/// A split started with a sketch's curves selected cuts along them, the
/// model's only body split.
#[test]
fn a_split_takes_the_curves_selected_as_its_line() {
    let mut drawn = drawn();
    select_curves(&mut drawn.plates, drawn.sketch, &drawn.chain);
    drawn.plates.doc.look(Look::StartSplit);
    let session = drawn.plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.split.mode, varde_view::SplitMode::Line);
    let (sketch, curves) = session.split.chain.clone().unwrap();
    assert_eq!(sketch, drawn.sketch);
    assert_eq!(curves.len(), 2);
    assert_eq!(session.bodies.len(), 1);
    assert_eq!(session.picking, MotionPick::Nothing);
}

/// Edit sketch, offered for a sketch's items selected, opens it.
#[test]
fn edit_sketch_opens_the_sketch_whose_items_are_selected() {
    let mut drawn = drawn();
    select_curves(&mut drawn.plates, drawn.sketch, &[drawn.line]);
    assert!(bar(&drawn.plates).iter().any(|t| t == "Edit sketch"));
    drawn.plates.doc.look(Look::EditFeature(drawn.sketch));
    assert!(drawn.plates.doc.sketch.is_some());
}
