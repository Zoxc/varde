//! Fillets in the history. The kernel's fillet isn't built yet (it
//! fails as too complex), so: a fillet reaching the kernel fails with
//! the too-complex message, changing no body, and the rest of the
//! history goes on; regen's own refusals come first (an edge not found),
//! and an edge is found again after an upstream dimension changes. With
//! the kernel's fillet replaced by a stand-in rounding each straight
//! convex edge off by cutting away the corner less the round's circle,
//! what regeneration hands it and does with the result: a block's edges
//! and a prism's obtuse and acute edges by their volumes, the round a cylinder of the radius, a join
//! after it, tangent chains grown and named, refusals worded with their
//! edge drawn, rounds overlapping on a narrow rib and one running into a
//! wall refused (never a wrong solid from the stand-in), the cache. The analytic volumes of the kernel's fillet (a
//! block's edges, all twelve with their corner spheres, a hole's and a
//! boss's rims, a slot's rim as a tangent chain, the same bits twice) are
//! written out, ignored until the kernel's fillet is built.

use std::cell::RefCell;

use glam::DVec3;
use varde_document::{EdgeRef, Fillet};
use varde_kernel::mesh::{FaceKey, Form};
use varde_kernel::topology::blend_edge;
use varde_kernel::{BlendError, FilletChain, Topology};

use crate::picking::region_form;

use super::chamfer::{
    Face, LEFT, TOP, assert_near, cube, edge, evaluated, front_top, holed_or_bossed, sloped, slot,
    solid_of, top_loop,
};
use super::motion::{add, block, cylinder, failure, key_on, key_where, set};
use super::*;

/// Fillets on this test's thread by the stand-in.
fn with_arcs() {
    super::super::fillet::fillet_by_arcs();
}

/// A length of `text` for a fillet in `document`.
fn radius(document: &Document, text: &str) -> Value {
    Value::new(text, &Fillet::radius_ask(&document.design())).unwrap()
}

/// A fillet of `edges`, `size` in radius, tangent chains on.
fn fillet(document: &Document, mut edges: Vec<EdgeRef>, size: &str) -> Fillet {
    edges.sort_by(EdgeRef::order);
    Fillet {
        edges,
        radius: radius(document, size),
        chains: true,
    }
}

/// What the kernel's fillet says of `body` today.
fn too_complex(body: &str) -> String {
    format!("filleting {body} is too complex to work out")
}

/// What a round of radius `r` takes off a right-angled edge per length:
/// the corner square less the quarter disc.
fn corner(r: f64) -> f64 {
    r * r * (1.0 - PI / 4.0)
}

/// What two such rounds meeting at a corner of a block, their axes
/// crossing, both take off there (and so is taken off once):
/// `∫₀ʳ (r − √(r² − w²))² dw`.
fn overlap(r: f64) -> f64 {
    r * r * r * (5.0 / 3.0 - PI / 2.0)
}

/// The round faces of `solid`: their radii, to the nearest millionth.
fn round_radii(solid: &Solid) -> Vec<f64> {
    let topology = solid.topology();
    (topology.regions().iter())
        .filter_map(|region| match *region_form(solid, region) {
            Form::Cylinder { radius, .. } => Some((radius * 1e6).round() / 1e6),
            _ => None,
        })
        .collect()
}

/// A fillet the kernel can't do yet fails as too complex, leaving the
/// body as it was; the history goes on: a later join still works on the
/// body.
#[test]
fn a_fillet_the_kernel_cant_do_fails_as_too_complex() {
    let (mut editor, body, solid) = cube();
    let kind = fillet(editor.document(), vec![front_top(&solid, body)], "2");
    let id = add(&mut editor, kind);
    let extent = two_sides(editor.document(), "12", "1");
    let join = add_extrude(
        &mut editor,
        rectangle((8.0, 8.0), (12.0, 12.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the fillet fails");
    assert_eq!(failed.message, too_complex("Body 1"));
    assert!(failure(&evaluation, join).is_none());
    assert_eq!(evaluation.failed.len(), 1);
    let volume = solid_of(&evaluation, body).volume();
    assert_near(volume, 1000.0 + 16.0 * 13.0 - 40.0);
}

/// An edge whose faces stop meeting (the cut that made one of them
/// removed; the fillet stays, as it names its body only) isn't found:
/// the fillet fails before the kernel, the body left as it was; among
/// others, it's named by its place.
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
    let wall = key_where(
        solid,
        |form| matches!(*form, Form::Plane { n, d } if n.x < -0.5 && (d + 3.0).abs() < 1e-9),
    );
    let at = edge(body, top, wall, [3.0, 1.5, 10.0]);
    let kind = fillet(editor.document(), vec![at], "1");
    let id = add(&mut editor, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        too_complex("Body 1")
    );
    editor.apply(Command::RemoveFeature(notch)).unwrap();
    assert!(editor.document().feature(id).is_some());
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the fillet fails");
    assert_eq!(failed.message, "its edge wasn't found");
    assert!(failed.geometry.is_none());
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);
    let mut kind = fillet(editor.document(), vec![at], "1");
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
/// faces' names, and rounded where it went.
#[test]
fn an_edge_is_found_again_after_an_upstream_change() {
    let (mut editor, body, solid) = cube();
    let kind = fillet(editor.document(), vec![front_top(&solid, body)], "2");
    let id = add(&mut editor, kind);
    let maker = editor.document().body(body).unwrap().created_by;
    let twenty = Extent::OneSide(length(editor.document(), "20"));
    set_extrude(&mut editor, maker, |extrude| extrude.extent = twenty);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        too_complex("Body 1")
    );
    with_arcs();
    let evaluation = evaluated(editor.document());
    assert!(failure(&evaluation, id).is_none());
    let rounded = solid_of(&evaluation, body);
    assert_near(rounded.volume(), 2000.0 - corner(2.0) * 10.0);
    assert_near(rounded.bounds3().unwrap().max.z, 20.0);
    assert_eq!(round_radii(rounded), [2.0]);
}

/// Through the stand-in: one of a block's edges (the round an exact
/// cylinder of the radius), then two opposite ones; its top loop, whose
/// rounds meet at the corners, either mitred as the kernel's are or
/// refused (the stand-in finds their strips on the top meeting at a
/// corner: too complex for it), never another volume.
#[test]
fn a_block_s_edges_are_rounded() {
    with_arcs();
    let (mut editor, body, solid) = cube();
    let document = editor.document().clone();
    let id = add(
        &mut editor,
        fillet(&document, vec![front_top(&solid, body)], "2"),
    );
    let rounded = |editor: &Editor| {
        let evaluation = evaluated(editor.document());
        match failure(&evaluation, id) {
            None => Ok(solid_of(&evaluation, body).clone()),
            Some(failed) => Err(failed.message.clone()),
        }
    };
    let one = rounded(&editor).unwrap();
    assert_near(one.volume(), 1000.0 - corner(2.0) * 10.0);
    assert_eq!(round_radii(&one), [2.0]);
    let loop_ = top_loop(&solid, body);
    set(
        &mut editor,
        id,
        fillet(&document, vec![loop_[0], loop_[1]], "1"),
    );
    let two = rounded(&editor).unwrap();
    assert_near(two.volume(), 1000.0 - 2.0 * corner(1.0) * 10.0);
    assert_eq!(round_radii(&two), [1.0; 2]);
    set(&mut editor, id, fillet(&document, loop_, "1"));
    match rounded(&editor) {
        Ok(four) => assert_near(
            four.volume(),
            1000.0 - 4.0 * corner(1.0) * 10.0 + 4.0 * overlap(1.0),
        ),
        Err(message) => assert_eq!(message, too_complex("Body 1")),
    }
}

/// The edge of `body` (its solid `solid`) between the planes `a` and `b`
/// (each `n·x = d`), named at its first curve's middle.
fn edge_on_planes(solid: &Solid, body: BodyId, a: Face, b: Face) -> EdgeRef {
    let topology = solid.topology();
    let regions = topology.regions();
    let on = |region: u32, (n, d): Face| {
        matches!(*region_form(solid, &regions[region as usize]), Form::Plane { n: m, d: e }
            if m.abs_diff_eq(DVec3::from(n), 1e-12) && (e - d).abs() < 1e-9)
    };
    let chain = (topology.chains().iter())
        .find(|chain| {
            let [p, q] = chain.regions;
            (on(p, a) && on(q, b)) || (on(p, b) && on(q, a))
        })
        .expect("the faces meet");
    let keys = chain.regions.map(|r| regions[r as usize].key);
    let curve = solid.mesh().curve(chain.halfedges[0]);
    edge(body, keys[0], keys[1], ((curve.p0 + curve.p1) / 2.0).into())
}

/// A rib 3 mm wide, both its top edges rounded 2 mm: each round alone
/// fits, but on the top they'd take 4 mm of its 3, so the two together
/// are refused (the stand-in's rounds would overlap there, leaving a
/// ridge no fillet makes), never a solid.
#[test]
fn rounds_overlapping_on_a_narrow_rib_are_refused() {
    with_arcs();
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 3.0, 10.0, "10");
    let solid = evaluated(editor.document()).bodies[0].solid.clone();
    let left = edge_on_planes(&solid, body, TOP, LEFT);
    let right = edge_on_planes(&solid, body, TOP, ([1.0, 0.0, 0.0], 3.0));
    let document = editor.document().clone();
    let id = add(&mut editor, fillet(&document, vec![left], "2"));
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    assert_near(
        solid_of(&evaluation, body).volume(),
        300.0 - corner(2.0) * 10.0,
    );
    for size in ["2", "1.5"] {
        set(&mut editor, id, fillet(&document, vec![left, right], size));
        let evaluation = evaluated(editor.document());
        let failed = failure(&evaluation, id).expect("refused");
        assert!(
            failed.message.starts_with("the fillet doesn't fit"),
            "{}",
            failed.message
        );
        assert_near(solid_of(&evaluation, body).volume(), 300.0);
    }
    // 1.4 mm each leaves 0.2 mm of the top between them.
    set(&mut editor, id, fillet(&document, vec![left, right], "1.4"));
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    assert_near(
        solid_of(&evaluation, body).volume(),
        300.0 - 2.0 * corner(1.4) * 10.0,
    );
}

/// A step: a block 5 mm high with a wall 10 mm high joined along its
/// back half, standing 2 mm out past its left. The lower top's left
/// edge runs into the wall: the
/// stand-in's prism, running past the edge's ends, would cut a groove
/// into the wall, so it's refused, or rounded as a fillet would be (the
/// corner taken off along the edge's 5 mm alone), never another volume.
#[test]
fn a_round_running_into_a_wall_cuts_nothing_past_it() {
    with_arcs();
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "5");
    let extent = Extent::OneSide(length(editor.document(), "10"));
    add_extrude(
        &mut editor,
        rectangle((-2.0, 5.0), (10.0, 10.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let solid = evaluated(editor.document()).bodies[0].solid.clone();
    assert_near(solid.volume(), 850.0);
    let at = edge_on_planes(&solid, body, ([0.0, 0.0, 1.0], 5.0), LEFT);
    let document = editor.document().clone();
    let id = add(&mut editor, fillet(&document, vec![at], "1"));
    let evaluation = evaluated(editor.document());
    match failure(&evaluation, id) {
        None => assert_near(
            solid_of(&evaluation, body).volume(),
            850.0 - corner(1.0) * 5.0,
        ),
        Some(_) => assert_near(solid_of(&evaluation, body).volume(), 850.0),
    }
}

/// A quad's prism, its sloping top meeting its walls at about 104° and
/// 76°: an obtuse and an acute edge rounded by the stand-in, each taking
/// off the corner between the faces' tangent lines and the arc,
/// `r² (cot(θ/2) − (π − θ)/2)` per length.
#[test]
fn obtuse_and_acute_edges_are_rounded() {
    with_arcs();
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XZ)))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let mut drawn = Sketch::default();
    sloped(&mut drawn);
    let region = drawn.profiles().unwrap().reference(0).unwrap();
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let extrude = Extrude {
        taper: None,
        sketch,
        regions: vec![region],
        extent: Extent::OneSide(length(editor.document(), "10")),
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    add(&mut editor, extrude);
    let body = editor.document().bodies()[0].id;
    let solid = evaluated(editor.document()).bodies[0].solid.clone();
    // The quad's area is 40.
    assert_near(solid.volume(), 400.0);
    let slope = key_where(
        &solid,
        |form| matches!(*form, Form::Plane { n, .. } if n.z > 0.1 && n.z < 0.99),
    );
    let right = key_where(
        &solid,
        |form| matches!(*form, Form::Plane { n, d } if n.x > 0.99 && (d - 10.0).abs() < 1e-9),
    );
    let left = key_where(
        &solid,
        |form| matches!(*form, Form::Plane { n, d } if n.x < -0.99 && (d + 2.0).abs() < 1e-9),
    );
    // The slope runs (−8, 2) from (10, 4): its angle to the wall going
    // down, and the other's at (2, 6).
    let cos_right = -2.0 / 68f64.sqrt();
    for (wall, cos) in [(right, cos_right), (left, -cos_right)] {
        let at = edge_between(&solid, body, slope, wall);
        let id = add(&mut editor, fillet(&Document::default(), vec![at], "1"));
        let evaluation = evaluated(editor.document());
        assert!(
            failure(&evaluation, id).is_none(),
            "{:?}",
            evaluation.failed
        );
        let theta = f64::acos(cos);
        let taken = (1.0 / (theta / 2.0).tan() - (PI - theta) / 2.0) * 10.0;
        let volume = solid_of(&evaluation, body).volume();
        assert!(
            (volume - (400.0 - taken)).abs() < 1e-9,
            "{volume} vs {taken}"
        );
        editor.apply(Command::RemoveFeature(id)).unwrap();
    }
}

/// The edge of `body` (its solid `solid`) between the faces of keys `a`
/// and `b`, named at its first curve's middle.
fn edge_between(solid: &Solid, body: BodyId, a: FaceKey, b: FaceKey) -> EdgeRef {
    let topology = solid.topology();
    let regions = topology.regions();
    let chain = (topology.chains().iter())
        .find(|chain| {
            let keys = chain.regions.map(|r| regions[r as usize].key);
            keys == [a, b] || keys == [b, a]
        })
        .expect("the faces meet");
    let curve = solid.mesh().curve(chain.halfedges[0]);
    edge(body, a, b, ((curve.p0 + curve.p1) / 2.0).into())
}

/// A join after a fillet, through the stand-in: a block standing over
/// the rounded edge, its wall across the round, adds what it holds
/// outside the rounded block.
#[test]
fn a_join_after_a_fillet_works() {
    with_arcs();
    let (mut editor, body, solid) = cube();
    let document = editor.document().clone();
    let id = add(
        &mut editor,
        fillet(&document, vec![front_top(&solid, body)], "2"),
    );
    let rounded = 1000.0 - corner(2.0) * 10.0;
    // Through the top in the middle, clear of the round, 3 mm above
    // and 1 mm below.
    let extent = two_sides(editor.document(), "13", "1");
    let clear = add_extrude(
        &mut editor,
        rectangle((3.0, 4.0), (7.0, 8.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_near(solid_of(&evaluation, body).volume(), rounded + 64.0);
    editor.apply(Command::RemoveFeature(clear)).unwrap();
    // Over the round: x 3 to 7, y −1 to 3, z −1 to 13. It shares with
    // the rounded block y 0 to 3, z 0 to 10 less the round's corner.
    let extent = two_sides(editor.document(), "13", "1");
    let over = add_extrude(
        &mut editor,
        rectangle((3.0, -1.0), (7.0, 3.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert!(failure(&evaluation, id).is_none());
    assert!(
        failure(&evaluation, over).is_none(),
        "{:?}",
        evaluation.failed
    );
    let shared = 4.0 * (30.0 - corner(2.0));
    assert_near(
        solid_of(&evaluation, body).volume(),
        rounded + 224.0 - shared,
    );
}

thread_local! {
    /// The chains and radius the recording stand-in was last handed.
    static HANDED: RefCell<(Vec<FilletChain>, f64)> = const { RefCell::new((Vec::new(), 0.0)) };
}

/// A stand-in that notes what it's handed and gives the solid back as
/// it was.
fn recording(
    solid: &Solid,
    _: &Topology,
    chains: &[FilletChain],
    radius: f64,
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, BlendError> {
    HANDED.with_borrow_mut(|handed| *handed = (chains.to_vec(), radius));
    Ok(solid.clone())
}

/// With tangent chains on, an edge of a slot's rim takes in the whole
/// rim (four chains, each named apart, the picked one by its
/// reference's keys); off, only itself. The radius goes as typed.
#[test]
fn tangent_chains_take_in_the_rim() {
    super::super::fillet::FILLETER.set(Some(recording));
    let mm = Document::default();
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    add_extrude(&mut editor, slot, extent, Operation::NewBody(BodyId::NEW));
    let body = editor.document().bodies()[0].id;
    let solid = solid_of(&evaluated(editor.document()), body).clone();
    let top = key_on(&solid, DVec3::Z, 5.0);
    let front = key_on(&solid, DVec3::NEG_Y, 3.0);
    let at = edge(body, top, front, [5.0, -3.0, 5.0]);
    let id = add(&mut editor, fillet(&mm, vec![at], "0.75"));
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    let (handed, radius) = HANDED.with_borrow(Clone::clone);
    assert_eq!(radius, 0.75);
    assert_eq!(handed.len(), 4);
    assert_eq!(handed[0].name, blend_edge(at.faces, 0));
    let mut names: Vec<u64> = handed.iter().map(|chain| chain.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 4);
    let mut chains: Vec<u32> = handed.iter().map(|chain| chain.chain).collect();
    chains.sort_unstable();
    chains.dedup();
    assert_eq!(chains.len(), 4);
    set(
        &mut editor,
        id,
        Fillet {
            chains: false,
            ..fillet(&mm, vec![at], "0.75")
        },
    );
    evaluated(editor.document());
    assert_eq!(HANDED.with_borrow(|handed| handed.0.len()), 1);
}

/// A stand-in refusing the first chain it's handed as too big.
fn too_big(
    _: &Solid,
    _: &Topology,
    chains: &[FilletChain],
    _: f64,
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, BlendError> {
    Err(BlendError::TooBig {
        chain: chains[0].chain,
    })
}

/// A stand-in refusing the last chain it's handed for running into
/// another face at its end.
fn runs_into(
    _: &Solid,
    _: &Topology,
    chains: &[FilletChain],
    _: f64,
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, BlendError> {
    Err(BlendError::End {
        chain: chains.last().unwrap().chain,
    })
}

/// The kernel refusing an edge is worded with the edge, which is drawn;
/// the stand-in's own too big (a radius past the faces) too.
#[test]
fn a_refused_edge_is_named_and_drawn() {
    super::super::fillet::FILLETER.set(Some(too_big));
    let (mut editor, body, solid) = cube();
    let kind = fillet(editor.document(), vec![front_top(&solid, body)], "1");
    let id = add(&mut editor, kind);
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).unwrap();
    assert_eq!(
        failed.message,
        "the fillet doesn't fit along its edge: it runs past a face beside it"
    );
    assert!(failed.geometry.is_some());
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);
    super::super::fillet::FILLETER.set(Some(runs_into));
    let document = editor.document().clone();
    set(
        &mut editor,
        id,
        fillet(&document, top_loop(&solid, body), "1"),
    );
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).unwrap();
    assert_eq!(
        failed.message,
        "the fillet along its edge 4 runs into another face at its end"
    );
    assert!(failed.geometry.is_some());
    with_arcs();
    set(
        &mut editor,
        id,
        fillet(&document, vec![front_top(&solid, body)], "12"),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "the fillet doesn't fit along its edge: it runs past a face beside it"
    );
}

/// A fillet that worked is cached: the same document again takes it
/// from the cache; another radius doesn't.
#[test]
fn a_fillet_that_worked_is_cached() {
    with_arcs();
    let (mut editor, body, solid) = cube();
    let kind = fillet(editor.document(), vec![front_top(&solid, body)], "1");
    let id = add(&mut editor, kind);
    let mut cache = Cache::default();
    let first = evaluate(editor.document(), &mut cache);
    let again = evaluate(editor.document(), &mut cache);
    assert!(Arc::ptr_eq(&first.bodies[0].solid, &again.bodies[0].solid));
    assert_near(first.bodies[0].solid.volume(), 1000.0 - corner(1.0) * 10.0);
    let document = editor.document().clone();
    set(
        &mut editor,
        id,
        fillet(&document, vec![front_top(&solid, body)], "2"),
    );
    let other = evaluate(editor.document(), &mut cache);
    assert_near(other.bodies[0].solid.volume(), 1000.0 - corner(2.0) * 10.0);
}

/// A body a join consumed before the fillet fails it, naming the body
/// holding it.
#[test]
fn a_consumed_body_fails_it() {
    let (mut editor, body, solid) = cube();
    let extent = Extent::OneSide(length(editor.document(), "5"));
    add_extrude(
        &mut editor,
        rectangle((20.0, 0.0), (30.0, 10.0)),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let other = editor.document().bodies()[1].id;
    let combine = varde_document::Combine {
        target: other,
        tools: vec![body],
        op: varde_document::BodyOp::Union,
        keep_tools: false,
    };
    add(&mut editor, combine);
    let kind = fillet(editor.document(), vec![front_top(&solid, body)], "1");
    let id = add(&mut editor, kind);
    let evaluation = evaluated(editor.document());
    let message = &failure(&evaluation, id).unwrap().message;
    assert!(message.starts_with("Body 1 is in Body 2 now"), "{message}");
}

// The kernel's fillet, analytically: ignored until it's built.

/// The section a round of radius `r` takes off a right-angled edge, and
/// its centroid's distance from the edge along either face:
/// `r (5/6 − π/4) / (1 − π/4)`.
fn corner_centroid(r: f64) -> f64 {
    r * (5.0 / 6.0 - PI / 4.0) / (1.0 - PI / 4.0)
}

/// One edge, the top loop (mitred), all twelve of a block (spheres at
/// the corners: the rounded box `(a − 2r)³ + 6r(a − 2r)² + 3πr²(a − 2r)
/// + 4πr³/3`).
#[test]
#[ignore = "kernel fillet not built"]
fn the_kernel_rounds_a_block_s_edges() {
    let (mut editor, body, solid) = cube();
    let document = editor.document().clone();
    let id = add(
        &mut editor,
        fillet(&document, vec![front_top(&solid, body)], "1"),
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
    assert_near(volume(&editor), 1000.0 - corner(1.0) * 10.0);
    set(
        &mut editor,
        id,
        fillet(&document, top_loop(&solid, body), "1"),
    );
    assert_near(
        volume(&editor),
        1000.0 - 4.0 * corner(1.0) * 10.0 + 4.0 * overlap(1.0),
    );
    set(
        &mut editor,
        id,
        fillet(&document, super::chamfer::all_twelve(&solid, body), "1"),
    );
    let (a, r): (f64, f64) = (10.0, 1.0);
    let inner = a - 2.0 * r;
    let rounded_box = inner.powi(3)
        + 6.0 * r * inner * inner
        + 3.0 * PI * r * r * inner
        + 4.0 * PI * r.powi(3) / 3.0;
    assert!((volume(&editor) - rounded_box).abs() < 1e-6 * rounded_box);
}

/// A hole's rim (convex: a torus taken off) and a boss's concave rim (a
/// torus added), 1 mm: the corner's section swept round at its centroid
/// (Pappus), within the fit's tolerance.
#[test]
#[ignore = "kernel fillet not built"]
fn the_kernel_rounds_rims() {
    let ring = 2.0 * PI * (3.0 + corner_centroid(1.0)) * corner(1.0);
    for hole in [true, false] {
        let (mut editor, body, solid, before) = holed_or_bossed(hole);
        let top = key_on(&solid, DVec3::Z, if hole { 10.0 } else { 5.0 });
        let near = if hole {
            [13.0, 10.0, 10.0]
        } else {
            [13.0, 10.0, 5.0]
        };
        let rim = edge(body, top, cylinder(&solid), near);
        let id = add(&mut editor, fillet(&Document::default(), vec![rim], "1"));
        let evaluation = evaluated(editor.document());
        assert!(
            failure(&evaluation, id).is_none(),
            "{:?}",
            evaluation.failed
        );
        let volume = solid_of(&evaluation, body).volume();
        let wanted = if hole { before - ring } else { before + ring };
        assert!(
            (volume - wanted).abs() < 1e-4 * wanted,
            "{volume} vs {wanted}"
        );
    }
}

/// A slot's top rim as one tangent chain, 1 mm: the corner's section
/// along the two sides and round the two ends at its centroid, inside
/// the ends' radius of 3.
#[test]
#[ignore = "kernel fillet not built"]
fn the_kernel_rounds_a_slot_s_rim() {
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    add_extrude(&mut editor, slot, extent, Operation::NewBody(BodyId::NEW));
    let body = editor.document().bodies()[0].id;
    let solid = solid_of(&evaluated(editor.document()), body).clone();
    let before = solid.volume();
    let top = key_on(&solid, DVec3::Z, 5.0);
    let front = key_on(&solid, DVec3::NEG_Y, 3.0);
    let at = edge(body, top, front, [5.0, -3.0, 5.0]);
    let id = add(&mut editor, fillet(&Document::default(), vec![at], "1"));
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    let taken = corner(1.0) * (20.0 + 2.0 * PI * (3.0 - corner_centroid(1.0)));
    let volume = solid_of(&evaluation, body).volume();
    assert!(
        (volume - (before - taken)).abs() < 1e-4 * before,
        "{volume} vs {}",
        before - taken
    );
}

/// The same fillet twice, in fresh caches: the same bits.
#[test]
#[ignore = "kernel fillet not built"]
fn the_kernel_rounds_alike_every_time() {
    let (mut editor, body, solid) = cube();
    let document = editor.document().clone();
    add(&mut editor, fillet(&document, top_loop(&solid, body), "1"));
    let a = evaluated(editor.document());
    let b = evaluated(editor.document());
    let (a, b) = (solid_of(&a, body), solid_of(&b, body));
    assert_eq!(a.volume().to_bits(), b.volume().to_bits());
    assert_eq!(a.mesh().tris().len(), b.mesh().tris().len());
}

mod fuzz;
