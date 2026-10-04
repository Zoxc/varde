//! Chamfers in the history. The kernel's chamfer isn't built yet (it
//! fails as too complex), so: a chamfer reaching the kernel fails with
//! the too-complex message, changing no body, and the rest of the
//! history goes on; regen's own refusals come first (an edge not found,
//! after the faces it was between stopped meeting), and an edge is found
//! again after an upstream dimension changes. With the kernel's chamfer
//! replaced by a stand-in cutting each block edge off by a prism, what
//! regeneration hands it and does with the result: the cut's distances
//! on the right faces (two distances, flipped, an angle), loops and all
//! twelve edges of a block by their volumes, tangent chains grown and
//! named, and a refusal worded with its edge drawn. The analytic volumes
//! of the kernel's chamfer (a block's edges, a hole's rim on an exact
//! cone, a boss's concave rim, two distances, the same bits twice) are
//! written out, ignored until the kernel's chamfer is built.

use std::cell::RefCell;

use glam::DVec3;
use varde_document::{Chamfer, ChamferSize, EdgeRef};
use varde_kernel::mesh::{FaceKey, Form};
use varde_kernel::topology::blend_edge;
use varde_kernel::{BlendError, ChamferChain, Topology};

use super::motion::{add, block, cylinder, failure, key_on, key_where, set};
use super::*;

/// Chamfers on this test's thread by the prism stand-in.
fn with_wedges() {
    super::super::chamfer::chamfer_by_wedges();
}

/// A length of `text` for a chamfer in `document`.
fn distance(document: &Document, text: &str) -> Value {
    Value::new(text, &Chamfer::distance_ask(&document.design())).unwrap()
}

fn angle(document: &Document, text: &str) -> Value {
    Value::new(text, &Chamfer::angle_ask(&document.design())).unwrap()
}

/// The edge of `body` (whose solid is `solid`) between the faces of
/// keys `a` and `b`, picked at `near`.
fn edge(body: BodyId, a: FaceKey, b: FaceKey, near: [f64; 3]) -> EdgeRef {
    let mut faces = [a, b];
    faces.sort();
    EdgeRef {
        body,
        faces,
        near: DVec3::from(near),
    }
}

/// The plane `n·x = d` of a block's face.
type Face = ([f64; 3], f64);

const TOP: Face = ([0.0, 0.0, 1.0], 10.0);
const BOTTOM: Face = ([0.0, 0.0, -1.0], 0.0);
const FRONT: Face = ([0.0, -1.0, 0.0], 0.0);
const BACK: Face = ([0.0, 1.0, 0.0], 10.0);
const LEFT: Face = ([-1.0, 0.0, 0.0], 0.0);
const RIGHT: Face = ([1.0, 0.0, 0.0], 10.0);

/// The edge of the block `body`, its solid `solid`, between its faces
/// `a` and `b`, picked at `near`.
fn block_edge(solid: &Solid, body: BodyId, a: Face, b: Face, near: [f64; 3]) -> EdgeRef {
    let key = |(n, d): Face| key_on(solid, DVec3::from(n), d);
    edge(body, key(a), key(b), near)
}

/// The solid of `body` in `evaluation`.
fn solid_of(evaluation: &Evaluation, body: BodyId) -> &Solid {
    &evaluation
        .bodies
        .iter()
        .find(|made| made.body == body)
        .unwrap()
        .solid
}

fn evaluated(document: &Document) -> Evaluation {
    evaluate(document, &mut Cache::default())
}

/// The 10 mm cube from the origin: the editor, its body and the solid
/// regenerated.
fn cube() -> (Editor, BodyId, Arc<Solid>) {
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let made = evaluated(editor.document());
    let solid = Arc::clone(&made.bodies[0].solid);
    (editor, body, solid)
}

/// A chamfer of `edges`, equal distances of `size`, tangent chains on.
fn chamfer(document: &Document, mut edges: Vec<EdgeRef>, size: &str) -> Chamfer {
    edges.sort_by(EdgeRef::order);
    Chamfer {
        edges,
        distances: ChamferSize::Equal(distance(document, size)),
        chains: true,
        flip: false,
    }
}

/// The cube's front top edge.
fn front_top(solid: &Solid, body: BodyId) -> EdgeRef {
    block_edge(solid, body, TOP, FRONT, [5.0, 0.0, 10.0])
}

/// What the kernel's chamfer says of `body` today.
fn too_complex(body: &str) -> String {
    format!("chamfering {body} is too complex to work out")
}

fn assert_near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} vs {b}");
}

/// A chamfer the kernel can't do yet fails as too complex, leaving the
/// body as it was; the history goes on: a later join still works on the
/// body.
#[test]
fn a_chamfer_the_kernel_cant_do_fails_as_too_complex() {
    let (mut editor, body, solid) = cube();
    let kind = chamfer(editor.document(), vec![front_top(&solid, body)], "1");
    let id = add(&mut editor, kind);
    let extent = two_sides(editor.document(), "12", "1");
    let join = add_extrude(
        &mut editor,
        rectangle((8.0, 8.0), (12.0, 12.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the chamfer fails");
    assert_eq!(failed.message, too_complex("Body 1"));
    assert!(failure(&evaluation, join).is_none());
    assert_eq!(evaluation.failed.len(), 1);
    // The cube and the join's block, 4 × 4 × 13 less the 2 × 2 × 10
    // shared.
    let volume = solid_of(&evaluation, body).volume();
    assert_near(volume, 1000.0 + 16.0 * 13.0 - 40.0);
}

/// An edge whose faces stop meeting (the cut that made one of them
/// removed; the chamfer stays, as it names its body only) isn't found:
/// the chamfer fails before the kernel, the body left as it was.
#[test]
fn an_edge_that_is_gone_is_not_found() {
    let (mut editor, body, _) = cube();
    let extent = two_sides(editor.document(), "11", "1");
    let notch = add_extrude(
        &mut editor,
        rectangle((-1.0, -1.0), (3.0, 3.0)),
        extent,
        Operation::Cut(Targets::default()),
    );
    let notched = evaluated(editor.document());
    let solid = solid_of(&notched, body);
    let top = key_on(solid, DVec3::Z, 10.0);
    // The notch's wall at x = 3, facing −x.
    let wall = key_where(
        solid,
        |form| matches!(*form, Form::Plane { n, d } if n.x < -0.5 && (d + 3.0).abs() < 1e-9),
    );
    let at = edge(body, top, wall, [3.0, 1.5, 10.0]);
    let kind = chamfer(editor.document(), vec![at], "1");
    let id = add(&mut editor, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        too_complex("Body 1")
    );
    editor.apply(Command::RemoveFeature(notch)).unwrap();
    assert!(editor.document().feature(id).is_some());
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the chamfer fails");
    assert_eq!(failed.message, "its edge wasn't found");
    assert!(failed.geometry.is_none());
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);
    // Among others, it's named by its place.
    let mut kind = chamfer(editor.document(), vec![at], "1");
    kind.edges
        .push(front_top(&solid_of(&evaluation, body).clone(), body));
    kind.edges.sort_by(EdgeRef::order);
    let place = kind.edges.iter().position(|e| *e == at).unwrap();
    set(&mut editor, id, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        format!("its edge {} of 2 wasn't found", place + 1)
    );
}

/// An upstream dimension change moves the edge; it's found again by its
/// faces' names, and the chamfer cut along it where it went.
#[test]
fn an_edge_is_found_again_after_an_upstream_change() {
    let (mut editor, body, solid) = cube();
    let kind = chamfer(editor.document(), vec![front_top(&solid, body)], "1");
    let id = add(&mut editor, kind);
    let maker = editor.document().body(body).unwrap().created_by;
    let twenty = Extent::OneSide(length(editor.document(), "20"));
    set_extrude(&mut editor, maker, |extrude| extrude.extent = twenty);
    // The kernel's: found, then too complex.
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        too_complex("Body 1")
    );
    // The stand-in's: cut where the edge now is.
    with_wedges();
    let evaluation = evaluated(editor.document());
    assert!(failure(&evaluation, id).is_none());
    let chamfered = solid_of(&evaluation, body);
    assert_near(chamfered.volume(), 2000.0 - 0.5 * 10.0);
    let bounds = chamfered.bounds3().unwrap();
    assert_near(bounds.max.z, 20.0);
}

/// The least `axis` coordinate of the corners of the patches of
/// `solid`'s face on the plane `face`.
fn face_min(solid: &Solid, face: Face, axis: usize) -> f64 {
    let topology = solid.topology();
    let key = key_on(solid, DVec3::from(face.0), face.1);
    let region = (topology.regions().iter())
        .find(|region| region.key == key)
        .unwrap();
    let mesh = solid.mesh();
    (region.tris.iter())
        .flat_map(|&t| {
            let patch = mesh.patch(t as usize);
            [DVec3::X, DVec3::Y, DVec3::Z].map(|corner| patch.eval(corner)[axis])
        })
        .fold(f64::INFINITY, f64::min)
}

/// The greatest, as [`face_min`].
fn face_max(solid: &Solid, face: Face, axis: usize) -> f64 {
    let topology = solid.topology();
    let key = key_on(solid, DVec3::from(face.0), face.1);
    let region = (topology.regions().iter())
        .find(|region| region.key == key)
        .unwrap();
    let mesh = solid.mesh();
    (region.tris.iter())
        .flat_map(|&t| {
            let patch = mesh.patch(t as usize);
            [DVec3::X, DVec3::Y, DVec3::Z].map(|corner| patch.eval(corner)[axis])
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Two distances go on the faces they name: the first along the first
/// key's face (the top, an end cap, before the wall), flipped along the
/// other; an angled cut's distance along the first face and its angle
/// to it.
#[test]
fn distances_go_on_the_faces_they_name() {
    with_wedges();
    let (mut editor, body, solid) = cube();
    let at = front_top(&solid, body);
    // The top's key comes first.
    assert_eq!(at.faces[0], key_on(&solid, DVec3::Z, 10.0));
    let document = editor.document().clone();
    let two = Chamfer {
        distances: ChamferSize::Two(distance(&document, "1"), distance(&document, "3")),
        ..chamfer(&document, vec![at], "1")
    };
    let id = add(&mut editor, two.clone());
    let cut = |editor: &Editor| {
        let evaluation = evaluated(editor.document());
        assert!(
            failure(&evaluation, id).is_none(),
            "{:?}",
            evaluation.failed
        );
        let solid = solid_of(&evaluation, body).clone();
        // How far the top starts from the front, and the front's top.
        let top_from = face_min(&solid, TOP, 1);
        let front_up_to = face_max(&solid, FRONT, 2);
        (solid.volume(), top_from, front_up_to)
    };
    let (volume, top_from, front_up_to) = cut(&editor);
    assert_near(volume, 1000.0 - 0.5 * 3.0 * 10.0);
    assert_near(top_from, 1.0);
    assert_near(front_up_to, 7.0);
    set(&mut editor, id, Chamfer { flip: true, ..two });
    let (volume, top_from, front_up_to) = cut(&editor);
    assert_near(volume, 1000.0 - 0.5 * 3.0 * 10.0);
    assert_near(top_from, 3.0);
    assert_near(front_up_to, 9.0);
    // 2 mm along the top, at 30° to it: tan 30° × 2 down the front.
    let angled = Chamfer {
        distances: ChamferSize::Angle(distance(&document, "2"), angle(&document, "30")),
        ..chamfer(&document, vec![at], "1")
    };
    set(&mut editor, id, angled);
    let (volume, top_from, front_up_to) = cut(&editor);
    let down = 2.0 * (30f64).to_radians().tan();
    assert!((volume - (1000.0 - 0.5 * 2.0 * down * 10.0)).abs() < 1e-9);
    assert_near(top_from, 2.0);
    assert!((front_up_to - (10.0 - down)).abs() < 1e-9);
}

/// The cube's four top edges, each named at its middle.
fn top_loop(solid: &Solid, body: BodyId) -> Vec<EdgeRef> {
    vec![
        block_edge(solid, body, TOP, FRONT, [5.0, 0.0, 10.0]),
        block_edge(solid, body, TOP, BACK, [5.0, 10.0, 10.0]),
        block_edge(solid, body, TOP, LEFT, [0.0, 5.0, 10.0]),
        block_edge(solid, body, TOP, RIGHT, [10.0, 5.0, 10.0]),
    ]
}

/// All twelve edges of the cube.
fn all_twelve(solid: &Solid, body: BodyId) -> Vec<EdgeRef> {
    let mut edges = top_loop(solid, body);
    edges.extend([
        block_edge(solid, body, BOTTOM, FRONT, [5.0, 0.0, 0.0]),
        block_edge(solid, body, BOTTOM, BACK, [5.0, 10.0, 0.0]),
        block_edge(solid, body, BOTTOM, LEFT, [0.0, 5.0, 0.0]),
        block_edge(solid, body, BOTTOM, RIGHT, [10.0, 5.0, 0.0]),
        block_edge(solid, body, FRONT, LEFT, [0.0, 0.0, 5.0]),
        block_edge(solid, body, FRONT, RIGHT, [10.0, 0.0, 5.0]),
        block_edge(solid, body, BACK, LEFT, [0.0, 10.0, 5.0]),
        block_edge(solid, body, BACK, RIGHT, [10.0, 10.0, 5.0]),
    ]);
    edges
}

/// The volume of the 10 mm cube with its four top edges chamfered by
/// 1 mm: at `h` into the top millimetre the section is a square
/// `10 − 2h` wide, so `∫₀¹ 100 − (10 − 2h)² dh = 56/3` is taken away.
const TOP_LOOP: f64 = 1000.0 - 56.0 / 3.0;

/// With all twelve: by symmetry eight times the part of the 5 mm octant
/// cube where `u + v`, `v + w`, `u + w` (from the centre) are at most
/// 9; the parts past each pair are 2.5 each, two of them together 1/3,
/// all three 1/4, so `8 × (125 − (7.5 − 1 + 0.25)) = 946`.
const ALL_TWELVE: f64 = 946.0;

/// Loops and all twelve edges of a block through the stand-in: the
/// chains handed over, one each, and the volumes.
#[test]
fn a_block_s_edges_are_chamfered() {
    with_wedges();
    let (mut editor, body, solid) = cube();
    let document = editor.document().clone();
    let id = add(&mut editor, chamfer(&document, top_loop(&solid, body), "1"));
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    assert_near(solid_of(&evaluation, body).volume(), TOP_LOOP);
    set(
        &mut editor,
        id,
        chamfer(&document, all_twelve(&solid, body), "1"),
    );
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    assert_near(solid_of(&evaluation, body).volume(), ALL_TWELVE);
}

thread_local! {
    /// The chains the recording stand-in was last handed.
    static HANDED: RefCell<Vec<ChamferChain>> = const { RefCell::new(Vec::new()) };
}

/// A stand-in that notes the chains it's handed and gives the solid
/// back as it was.
fn recording(
    solid: &Solid,
    _: &Topology,
    chains: &[ChamferChain],
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, BlendError> {
    HANDED.with_borrow_mut(|handed| *handed = chains.to_vec());
    Ok(solid.clone())
}

/// A stand-in refusing the first chain it's handed as too big.
fn too_big(
    _: &Solid,
    _: &Topology,
    chains: &[ChamferChain],
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, BlendError> {
    Err(BlendError::TooBig {
        chain: chains[0].chain,
    })
}

/// Draws a slot: two lines 10 mm long, 6 mm apart, joined by half
/// circles at each end, which they run on into smoothly.
fn slot(sketch: &mut Sketch) {
    let at = |sketch: &mut Sketch, x: f64, y: f64| sketch.add_point(DVec2::new(x, y)).unwrap();
    let [a, b, c, d] =
        [(0.0, -3.0), (10.0, -3.0), (10.0, 3.0), (0.0, 3.0)].map(|(x, y)| at(sketch, x, y));
    let [left, right] = [(0.0, 0.0), (10.0, 0.0)].map(|(x, y)| at(sketch, x, y));
    sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let arc = Curve::Arc {
        center: right,
        start: b,
        end: c,
    };
    sketch.add_curve(arc, false).unwrap();
    sketch
        .add_curve(Curve::Line { start: c, end: d }, false)
        .unwrap();
    let arc = Curve::Arc {
        center: left,
        start: d,
        end: a,
    };
    sketch.add_curve(arc, false).unwrap();
}

/// With tangent chains on, an edge of a slot's rim takes in the whole
/// rim (four chains, each named apart, the picked one by its
/// reference's keys); off, only itself. The picked edge's first face is
/// the top all round.
#[test]
fn tangent_chains_take_in_the_rim() {
    super::super::chamfer::CHAMFERER.set(Some(recording));
    let mm = Document::default();
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    add_extrude(&mut editor, slot, extent, Operation::NewBody(BodyId::NEW));
    let body = editor.document().bodies()[0].id;
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let top = key_on(&solid, DVec3::Z, 5.0);
    let front = key_on(&solid, DVec3::NEG_Y, 3.0);
    let at = edge(body, top, front, [5.0, -3.0, 5.0]);
    let id = add(&mut editor, chamfer(&mm, vec![at], "1"));
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    let handed = HANDED.with_borrow(Clone::clone);
    assert_eq!(handed.len(), 4);
    assert_eq!(handed[0].name, blend_edge(at.faces, 0));
    let mut names: Vec<u64> = handed.iter().map(|chain| chain.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 4);
    let topology = solid.topology();
    let top_region = (topology.regions().iter())
        .position(|region| region.key == top)
        .unwrap() as u32;
    for chain in &handed {
        let sides = topology.chains()[chain.chain as usize].regions;
        assert!(sides.contains(&top_region));
        // Equal distances either way.
        assert_eq!(chain.cut, varde_kernel::ChamferCut::Distances([1.0; 2]));
    }
    // Two distances: the top's first all round.
    let two = Chamfer {
        distances: ChamferSize::Two(distance(&mm, "1"), distance(&mm, "2")),
        ..chamfer(&mm, vec![at], "1")
    };
    set(&mut editor, id, two.clone());
    evaluated(editor.document());
    for chain in HANDED.with_borrow(Clone::clone) {
        let sides = topology.chains()[chain.chain as usize].regions;
        let wanted = if sides[0] == top_region {
            [1.0, 2.0]
        } else {
            [2.0, 1.0]
        };
        assert_eq!(chain.cut, varde_kernel::ChamferCut::Distances(wanted));
    }
    set(
        &mut editor,
        id,
        Chamfer {
            chains: false,
            ..two
        },
    );
    evaluated(editor.document());
    assert_eq!(HANDED.with_borrow(Vec::len), 1);
}

/// The kernel refusing an edge is worded with the edge, which is drawn.
#[test]
fn a_refused_edge_is_named_and_drawn() {
    super::super::chamfer::CHAMFERER.set(Some(too_big));
    let (mut editor, body, solid) = cube();
    let kind = chamfer(editor.document(), vec![front_top(&solid, body)], "1");
    let id = add(&mut editor, kind);
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).unwrap();
    assert_eq!(
        failed.message,
        "the chamfer doesn't fit along its edge: it runs past a face beside it"
    );
    assert!(failed.geometry.is_some());
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);
}

/// A chamfer that worked is cached: the same document again takes it
/// from the cache.
#[test]
fn a_chamfer_that_worked_is_cached() {
    with_wedges();
    let (mut editor, body, solid) = cube();
    let kind = chamfer(editor.document(), vec![front_top(&solid, body)], "1");
    add(&mut editor, kind);
    let mut cache = Cache::default();
    let first = evaluate(editor.document(), &mut cache);
    let again = evaluate(editor.document(), &mut cache);
    assert!(Arc::ptr_eq(&first.bodies[0].solid, &again.bodies[0].solid));
    assert_near(first.bodies[0].solid.volume(), 995.0);
}

// The kernel's chamfer, analytically: ignored until it's built.

/// One edge, a loop of four, all twelve of a block.
#[test]
#[ignore = "kernel chamfer not built"]
fn the_kernel_chamfers_a_block_s_edges() {
    let (mut editor, body, solid) = cube();
    let document = editor.document().clone();
    let id = add(
        &mut editor,
        chamfer(&document, vec![front_top(&solid, body)], "1"),
    );
    let volume = |editor: &Editor| {
        let evaluation = evaluated(editor.document());
        assert!(
            failure(&evaluation, id).is_none(),
            "{:?}",
            evaluation.failed
        );
        solid_of(&evaluation, body).volume()
    };
    assert_near(volume(&editor), 995.0);
    set(
        &mut editor,
        id,
        chamfer(&document, top_loop(&solid, body), "1"),
    );
    assert_near(volume(&editor), TOP_LOOP);
    set(
        &mut editor,
        id,
        chamfer(&document, all_twelve(&solid, body), "1"),
    );
    assert_near(volume(&editor), ALL_TWELVE);
    // Two distances, 1 and 2 mm.
    let two = Chamfer {
        distances: ChamferSize::Two(distance(&document, "1"), distance(&document, "2")),
        ..chamfer(&document, vec![front_top(&solid, body)], "1")
    };
    set(&mut editor, id, two);
    assert_near(volume(&editor), 1000.0 - 10.0);
}

/// A 20 mm square plate 10 mm thick with a round hole of radius 3
/// through it, or a round boss of radius 3 on it 5 mm thick: the
/// editor, the body and the solid.
fn holed_or_bossed(hole: bool) -> (Editor, BodyId, Arc<Solid>, f64) {
    let mut editor = Editor::new(Document::default());
    let height = if hole { "10" } else { "5" };
    let body = block(&mut editor, 0.0, 0.0, 20.0, 20.0, height);
    let (extent, operation) = if hole {
        (
            two_sides(editor.document(), "11", "1"),
            Operation::Cut(Targets::default()),
        )
    } else {
        (
            two_sides(editor.document(), "8", "1"),
            Operation::Join(Targets::default()),
        )
    };
    add_extrude(&mut editor, disc((10.0, 10.0), 3.0), extent, operation);
    let evaluation = evaluated(editor.document());
    let solid = Arc::clone(&evaluation.bodies[0].solid);
    let volume = solid.volume();
    (editor, body, solid, volume)
}

/// A hole's rim (an exact cone) and a boss's concave rim, 1 mm: the
/// triangle of section ½ mm² swept round at its centroid, `r + 1/3`
/// from the axis (Pappus), taken away or added.
#[test]
#[ignore = "kernel chamfer not built"]
fn the_kernel_chamfers_rims() {
    let ring = 2.0 * PI * (3.0 + 1.0 / 3.0) * 0.5;
    for hole in [true, false] {
        let (mut editor, body, solid, before) = holed_or_bossed(hole);
        let top = key_on(&solid, DVec3::Z, if hole { 10.0 } else { 5.0 });
        let near = if hole {
            [13.0, 10.0, 10.0]
        } else {
            [13.0, 10.0, 5.0]
        };
        let rim = edge(body, top, cylinder(&solid), near);
        let mm = Document::default();
        let id = add(&mut editor, chamfer(&mm, vec![rim], "1"));
        let evaluation = evaluated(editor.document());
        assert!(
            failure(&evaluation, id).is_none(),
            "{:?}",
            evaluation.failed
        );
        let volume = solid_of(&evaluation, body).volume();
        let wanted = if hole { before - ring } else { before + ring };
        assert!(
            (volume - wanted).abs() < 1e-6 * wanted,
            "{volume} vs {wanted}"
        );
    }
}

/// The same chamfer twice, in fresh caches: the same bits.
#[test]
#[ignore = "kernel chamfer not built"]
fn the_kernel_chamfers_alike_every_time() {
    let (mut editor, body, solid) = cube();
    let document = editor.document().clone();
    add(
        &mut editor,
        chamfer(&document, all_twelve(&solid, body), "1"),
    );
    let a = evaluated(editor.document());
    let b = evaluated(editor.document());
    let (a, b) = (solid_of(&a, body), solid_of(&b, body));
    assert_eq!(a.volume().to_bits(), b.volume().to_bits());
    assert_eq!(a.mesh().tris().len(), b.mesh().tris().len());
}
