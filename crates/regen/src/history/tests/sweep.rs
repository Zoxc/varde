//! Sweeps in the history. The kernel's sweep isn't built yet (it fails as
//! too complex), so: a sweep reaching the kernel fails with the
//! too-complex message, making no body, and the rest of the history goes
//! on; regen's own refusals come first (its path's curves or edges gone,
//! its parts apart, a corner, a start off the profile's plane or not
//! square to it). With the kernel's sweep replaced by a stand-in
//! extruding along straight paths, or one recording what it's handed:
//! the path built from sketches and model edges, ordered and joined,
//! followed when it's edited; a helix's axis resolved; joins and cuts as
//! an extrude's. The analytic volumes of the feature with the kernel's
//! sweep (a pipe joined to a plate at its start, cut through a block, a
//! bead along a round rim following it, a spring joined to a plate) are
//! written out, ignored until the kernel's sweep is built.

use std::cell::RefCell;

use glam::DVec3;
use varde_document::{
    Axis3, AxisRef, CurveChain, EdgeRef, Helix, Orientation, PathPart, PathRef, Sweep,
};
use varde_kernel::measure::{EdgeShape, edge_shape};
use varde_kernel::sweep::{Path, Piece, SweepError};
use varde_kernel::{KernelError, Profile};
use varde_sketch::{Id, RegionRef};

use super::motion::{add, block, failure, set, the_plate};
use super::*;

/// Sweeps on this test's thread by the extruding stand-in.
fn with_extrudes() {
    super::super::sweep::sweep_by_extrude();
}

fn evaluated(document: &Document) -> Evaluation {
    evaluate(document, &mut Cache::default())
}

/// What the kernel's sweep says today.
const TOO_COMPLEX: &str = "sweeping its regions along its path is too complex to work out";

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

/// Sets the drawing of `sketch` to what `draw` draws.
fn redraw(editor: &mut Editor, sketch: FeatureId, draw: impl FnOnce(&mut Sketch)) {
    let mut drawn = Sketch::default();
    draw(&mut drawn);
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
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

/// The ids of every curve of sketch `feature`, sorted.
fn curves_of(editor: &Editor, feature: FeatureId) -> Vec<Id> {
    let FeatureKind::Sketch { sketch, .. } = &editor.document().feature(feature).unwrap().kind
    else {
        panic!("{feature:?} is a sketch");
    };
    let mut ids: Vec<Id> = sketch.curves.iter().map(|entry| entry.id).collect();
    ids.sort();
    ids
}

/// The references of every region of sketch `feature`.
fn regions_of(editor: &Editor, feature: FeatureId) -> Vec<RegionRef> {
    let FeatureKind::Sketch { sketch, .. } = &editor.document().feature(feature).unwrap().kind
    else {
        panic!("{feature:?} is a sketch");
    };
    let profiles = sketch.profiles().unwrap();
    (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect()
}

/// The part of every curve of sketch `feature`.
fn curves_part(editor: &Editor, feature: FeatureId) -> PathPart {
    PathPart::Curves(CurveChain {
        sketch: feature,
        curves: curves_of(editor, feature),
    })
}

/// The 10 × 10 mm square on XY at x 15 to 25 and y −5 to 5 (clear of
/// the example plate's hole): its sketch's id.
fn square_profile(editor: &mut Editor) -> FeatureId {
    sketch_on(
        editor,
        OriginPlane::XY,
        rectangle((15.0, -5.0), (25.0, 5.0)),
    )
}

/// Adds the regions of `profile` swept along `parts`, doing
/// `operation`: its id.
fn add_swept(
    editor: &mut Editor,
    profile: FeatureId,
    parts: Vec<PathPart>,
    operation: Operation,
) -> FeatureId {
    let sweep = swept(editor, profile, parts, operation);
    add(editor, sweep)
}

/// The regions of `profile` swept along `parts`, doing `operation`.
fn swept(editor: &Editor, profile: FeatureId, parts: Vec<PathPart>, operation: Operation) -> Sweep {
    Sweep {
        sketch: profile,
        regions: regions_of(editor, profile),
        path: PathRef::Chain(parts),
        orientation: Orientation::FollowPath,
        twist: None,
        operation,
    }
}

/// A path on XZ up from (20, 0, 0) to `top` along z: its sketch's id.
fn upright(editor: &mut Editor, top: f64) -> FeatureId {
    sketch_on(
        editor,
        OriginPlane::XZ,
        polyline(vec![(20.0, 0.0), (20.0, top)]),
    )
}

/// Adds a sketch on XY, a square to sweep, a path up z `height` on XZ
/// and the sweep making a new body: the editor, the sweep's id, its
/// body's and its path's sketch's.
fn square_up(height: f64) -> (Editor, FeatureId, BodyId, FeatureId) {
    let mut editor = Editor::new(Document::example());
    let profile = square_profile(&mut editor);
    let path = upright(&mut editor, height);
    let parts = vec![curves_part(&editor, path)];
    let sweep = swept(&editor, profile, parts, Operation::NewBody(BodyId::NEW));
    let id = add(&mut editor, sweep);
    let body = editor.document().bodies().last().unwrap().id;
    (editor, id, body, path)
}

fn assert_close(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} is not {b}");
}

/// The kernel's sweep fails as too complex: the sweep makes no body and
/// later features still run.
#[test]
fn the_kernel_s_sweep_fails_as_too_complex_and_the_history_goes_on() {
    let (mut editor, id, body, _) = square_up(30.0);
    let pocket = add_pocket(&mut editor);
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the sweep fails");
    assert_eq!(failed.message, TOO_COMPLEX);
    assert!(evaluation.bodies.iter().all(|made| made.body != body));
    assert!(failure(&evaluation, pocket).is_none());
    assert_close(evaluation.bodies[0].solid.volume(), the_plate() - POCKET);
    // Its body is still the document's, with no solid.
    let FeatureKind::Sweep(sweep) = &editor.document().feature(id).unwrap().kind else {
        unreachable!()
    };
    assert_eq!(sweep.operation, Operation::NewBody(body));
}

/// With the stand-in, a square swept 30 up a straight path is a box,
/// and a cut along one through the plate takes its share.
#[test]
fn a_straight_path_sweeps_a_box_and_cuts_as_an_extrude() {
    with_extrudes();
    let (editor, _, body, _) = square_up(30.0);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(solid_of(&evaluation, body).volume(), 3000.0);
    // As a cut 12 up, through the plate's 10.
    let mut cut = Editor::new(Document::example());
    let profile = square_profile(&mut cut);
    let path = upright(&mut cut, 12.0);
    let parts = vec![curves_part(&cut, path)];
    let sweep = swept(&cut, profile, parts, Operation::Cut(Targets::default()));
    let cutting = add(&mut cut, sweep);
    let evaluation = evaluated(cut.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(evaluation.bodies[0].solid.volume(), the_plate() - 1000.0);
    let plate = cut.document().bodies()[0].id;
    assert!(
        (evaluation.touched.iter())
            .any(|(feature, bodies)| *feature == cutting && bodies == &[plate])
    );
}

/// Editing the path's sketch, the sweep follows; an unchanged history is
/// found in the cache, and undo goes back.
#[test]
fn the_sweep_follows_its_path_when_it_is_edited() {
    with_extrudes();
    let (mut editor, _, body, path) = square_up(30.0);
    let mut cache = Cache::default();
    let first = evaluate(editor.document(), &mut cache);
    assert_close(solid_of(&first, body).volume(), 3000.0);
    let (_, misses) = cache.counts();
    let again = evaluate(editor.document(), &mut cache);
    assert_eq!(cache.counts().1, misses, "nothing worked out again");
    assert!(Arc::ptr_eq(
        &solid_of_arc(&first, body),
        &solid_of_arc(&again, body)
    ));
    redraw(&mut editor, path, polyline(vec![(20.0, 0.0), (20.0, 45.0)]));
    let longer = evaluate(editor.document(), &mut cache);
    assert!(longer.failed.is_empty(), "{:?}", longer.failed);
    assert_close(solid_of(&longer, body).volume(), 4500.0);
    editor.undo();
    let back = evaluate(editor.document(), &mut cache);
    assert_close(solid_of(&back, body).volume(), 3000.0);
}

/// The solid of `body` in `evaluation`, shared.
fn solid_of_arc(evaluation: &Evaluation, body: BodyId) -> Arc<Solid> {
    let made = evaluation.bodies.iter().find(|made| made.body == body);
    Arc::clone(&made.unwrap().solid)
}

thread_local! {
    /// What the recording stand-in was handed.
    static HANDED: RefCell<Vec<Path>> = const { RefCell::new(Vec::new()) };
}

/// A stand-in that records the path it's handed, and fails.
#[allow(clippy::too_many_arguments)]
fn recording(
    _: &Profile,
    _: &varde_kernel::Frame,
    path: &Path,
    _: varde_kernel::sweep::Orientation,
    _: f64,
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, SweepError> {
    HANDED.with_borrow_mut(|handed| handed.push(path.clone()));
    Err(SweepError::Failed(KernelError::TooComplex.into()))
}

/// Records what the history hands the kernel's sweep.
fn handed(document: &Document) -> (Evaluation, Vec<Path>) {
    super::super::sweep::SWEEPER.set(Some(recording));
    HANDED.with_borrow_mut(Vec::clear);
    let evaluation = evaluated(document);
    super::super::sweep::SWEEPER.set(None);
    (evaluation, HANDED.with_borrow_mut(std::mem::take))
}

/// A path of two sketches' chains, the second listed first and drawn
/// the other way: joined in order from the end on the profile's plane,
/// each piece running on from the last; swept by the stand-in, the
/// whole length.
#[test]
fn a_path_of_two_sketches_is_ordered_and_joined() {
    let mut editor = Editor::new(Document::default());
    let profile = square_profile(&mut editor);
    // Up z from the origin to 10 on XZ, then on from 25 down to 10 on YZ.
    let low = sketch_on(
        &mut editor,
        OriginPlane::XZ,
        polyline(vec![(0.0, 0.0), (0.0, 10.0)]),
    );
    let high = sketch_on(
        &mut editor,
        OriginPlane::YZ,
        polyline(vec![(0.0, 25.0), (0.0, 10.0)]),
    );
    let parts = vec![curves_part(&editor, high), curves_part(&editor, low)];
    let sweep = add_swept(&mut editor, profile, parts, Operation::NewBody(BodyId::NEW));
    let (evaluation, paths) = handed(editor.document());
    assert_eq!(failure(&evaluation, sweep).unwrap().message, TOO_COMPLEX);
    let v = DVec3::new;
    assert_eq!(
        paths,
        [Path::Chain {
            pieces: vec![
                Piece::Line {
                    from: v(0.0, 0.0, 0.0),
                    to: v(0.0, 0.0, 10.0)
                },
                Piece::Line {
                    from: v(0.0, 0.0, 10.0),
                    to: v(0.0, 0.0, 25.0)
                },
            ],
            closed: false,
        }]
    );
    with_extrudes();
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let body = editor.document().bodies().last().unwrap().id;
    assert_close(solid_of(&evaluation, body).volume(), 2500.0);
}

/// A sketch's arcs and circles are arc pieces about their centres,
/// turning about the sketch's normal or against it; a circle alone is a
/// closed path.
#[test]
fn arcs_and_circles_are_arc_pieces() {
    let mut editor = Editor::new(Document::default());
    let profile = square_profile(&mut editor);
    // On XZ: up from the origin 10, a quarter turn toward +x of radius
    // 5 about (5, 10), on 5 along x.
    let bent = sketch_on(&mut editor, OriginPlane::XZ, |sketch| {
        let p = |sketch: &mut Sketch, x, y| sketch.add_point(DVec2::new(x, y)).unwrap();
        let [a, b, c, d] =
            [(0.0, 0.0), (0.0, 10.0), (5.0, 15.0), (10.0, 15.0)].map(|(x, y)| p(sketch, x, y));
        let center = p(sketch, 5.0, 10.0);
        sketch
            .add_curve(Curve::Line { start: a, end: b }, false)
            .unwrap();
        // Counter-clockwise from c to b in the sketch.
        let arc = Curve::Arc {
            center,
            start: c,
            end: b,
        };
        sketch.add_curve(arc, false).unwrap();
        sketch
            .add_curve(Curve::Line { start: c, end: d }, false)
            .unwrap();
    });
    let parts = vec![curves_part(&editor, bent)];
    add_swept(&mut editor, profile, parts, Operation::NewBody(BodyId::NEW));
    let ring = sketch_on(&mut editor, OriginPlane::XZ, disc((20.0, 0.0), 3.0));
    let parts = vec![curves_part(&editor, ring)];
    add_swept(&mut editor, profile, parts, Operation::NewBody(BodyId::NEW));
    let (evaluation, paths) = handed(editor.document());
    assert_eq!(evaluation.failed.len(), 2, "{:?}", evaluation.failed);
    let Path::Chain {
        pieces,
        closed: false,
    } = &paths[0]
    else {
        panic!("{:?}", paths[0]);
    };
    assert!(matches!(pieces[0], Piece::Line { .. }));
    let Piece::Arc {
        conics,
        centre,
        axis,
    } = &pieces[1]
    else {
        panic!("{:?}", pieces[1]);
    };
    assert_eq!(*centre, DVec3::new(5.0, 0.0, 10.0));
    // From (0, 0, 10) toward +x: right-handed about +y, the XZ sketch's
    // normal turned round (it's −y).
    assert_eq!(*axis, DVec3::Y);
    assert_eq!(conics[0].p0, DVec3::new(0.0, 0.0, 10.0));
    assert_eq!(conics.last().unwrap().p1, DVec3::new(5.0, 0.0, 15.0));
    assert!(matches!(pieces[2], Piece::Line { to, .. } if to == DVec3::new(10.0, 0.0, 15.0)));
    let Path::Chain {
        pieces,
        closed: true,
    } = &paths[1]
    else {
        panic!("{:?}", paths[1]);
    };
    let [Piece::Arc { conics, .. }] = pieces.as_slice() else {
        panic!("{pieces:?}");
    };
    assert_eq!(conics.len(), 4);
    assert_eq!(conics[0].p0, conics[3].p1);
}

/// What draws a path's sketch.
type Drawing = Box<dyn FnOnce(&mut Sketch)>;

/// Regen's own refusals, before the kernel: a start off the profile's
/// plane, a start not square to it, a corner (drawn), parts apart, a
/// closed part with others, curves branching.
#[test]
fn paths_that_cant_be_swept_are_refused_first() {
    let refused = |draw: Vec<Drawing>| -> FeatureFailure {
        let mut editor = Editor::new(Document::default());
        let profile = square_profile(&mut editor);
        let mut parts = Vec::new();
        for draw in draw {
            let path = sketch_on(&mut editor, OriginPlane::XZ, draw);
            parts.push(curves_part(&editor, path));
        }
        let sweep = add_swept(&mut editor, profile, parts, Operation::NewBody(BodyId::NEW));
        let (evaluation, paths) = handed(editor.document());
        assert!(paths.is_empty(), "handed {paths:?}");
        failure(&evaluation, sweep).unwrap().clone()
    };
    let up = |from: f64, to: f64| -> Drawing { Box::new(polyline(vec![(20.0, from), (20.0, to)])) };
    assert_eq!(
        refused(vec![up(5.0, 30.0)]).message,
        message::PATH_OFF_START
    );
    assert_eq!(
        refused(vec![Box::new(polyline(vec![(20.0, 0.0), (25.0, 30.0)]))]).message,
        message::PATH_NOT_SQUARE
    );
    let corner = refused(vec![Box::new(polyline(vec![
        (20.0, 0.0),
        (20.0, 10.0),
        (30.0, 10.0),
    ]))]);
    assert_eq!(corner.message, message::PATH_CORNER);
    let geometry = corner.geometry.expect("the corner drawn");
    assert_eq!(geometry.points(), [[20.0, 0.0, 10.0]]);
    assert_eq!(
        refused(vec![up(0.0, 10.0), up(11.0, 20.0)]).message,
        message::PATH_PARTS_APART
    );
    assert_eq!(
        refused(vec![up(0.0, 10.0), up(10.0, 20.0), up(10.0, 30.0)]).message,
        message::PATH_PARTS_APART
    );
    assert_eq!(
        refused(vec![up(0.0, 10.0), Box::new(disc((20.0, 20.0), 3.0))]).message,
        message::PATH_CLOSED_NOT_ALONE
    );
    assert_eq!(
        refused(vec![Box::new(|sketch: &mut Sketch| {
            polyline(vec![(20.0, 0.0), (20.0, 10.0), (20.0, 20.0)])(sketch);
            polyline(vec![(20.0, 10.0), (30.0, 10.0)])(sketch);
        })])
        .message,
        message::PATH_CURVES_BRANCH
    );
}

/// The path's curves deleted from its sketch: "path not found", the
/// rest of the history going on; drawn again, the sweep comes back.
/// Removing the path's sketch removes the sweep.
#[test]
fn a_path_whose_curves_are_gone_fails() {
    with_extrudes();
    let (mut editor, id, body, path) = square_up(30.0);
    let pocket = add_pocket(&mut editor);
    // Drawn again from scratch, a point first: the line has another id.
    redraw(&mut editor, path, |sketch| {
        sketch.add_point(DVec2::new(40.0, 40.0)).unwrap();
        polyline(vec![(20.0, 0.0), (20.0, 30.0)])(sketch);
    });
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        message::PATH_NOT_FOUND
    );
    assert!(failure(&evaluation, pocket).is_none());
    assert!(evaluation.bodies.iter().all(|made| made.body != body));
    editor.undo();
    let evaluation = evaluated(editor.document());
    assert_close(solid_of(&evaluation, body).volume(), 3000.0);
    editor.apply(Command::RemoveFeature(path)).unwrap();
    assert!(editor.document().feature(id).is_none());
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
}

/// The straight edge of `solid` (`body`'s) along z at `(x, y)`, as a
/// path's or an axis's reference.
fn upright_edge(solid: &Solid, body: BodyId, x: f64, y: f64) -> EdgeRef {
    let topology = solid.topology();
    for chain in topology.chains() {
        if let EdgeShape::Line { from, to } = edge_shape(solid, chain)
            && from.x == x
            && from.y == y
            && to.x == x
            && to.y == y
        {
            let mut faces = chain.regions.map(|r| topology.regions()[r as usize].key);
            faces.sort();
            return EdgeRef {
                body,
                faces,
                near: (from + to) * 0.5,
            };
        }
    }
    panic!("no edge up z at ({x}, {y})");
}

/// A 10 × 10 × 30 block at x 40 to 50, y 0 to 10, and a cut through it,
/// first a small hole well inside it: the editor, the block's body, the
/// cut's sketch.
fn block_with_hole() -> (Editor, BodyId, FeatureId) {
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 40.0, 0.0, 50.0, 10.0, "30");
    let extent = Extent::OneSide(length(editor.document(), "40"));
    add_extrude(
        &mut editor,
        rectangle((42.0, 2.0), (43.0, 3.0)),
        extent,
        Operation::Cut(Targets::default()),
    );
    let hole = editor.document().features()[3].id;
    (editor, body, hole)
}

/// A path along a model edge: the block's corner up z, found on its
/// topology; with the stand-in, the square swept 30 up. Cut away by an
/// edit before the sweep, "its path edge wasn't found"; an edge whose
/// faces don't meet likewise.
#[test]
fn a_path_along_a_model_edge_follows_it_or_fails_when_it_is_gone() {
    with_extrudes();
    let (mut editor, block_body, hole) = block_with_hole();
    let solid = solid_of_arc(&evaluated(editor.document()), block_body);
    let edge = upright_edge(&solid, block_body, 50.0, 10.0);
    let profile = sketch_on(
        &mut editor,
        OriginPlane::XY,
        rectangle((55.0, 0.0), (59.0, 4.0)),
    );
    let parts = vec![PathPart::Edges {
        edges: vec![edge],
        tangent: true,
    }];
    let sweep = add_swept(&mut editor, profile, parts, Operation::NewBody(BodyId::NEW));
    let made = editor.document().bodies().last().unwrap().id;
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(solid_of(&evaluation, made).volume(), 16.0 * 30.0);
    // The hole moved onto the corner: the edge is cut away.
    let cut_sketch = editor.document().features()[hole_sketch(&editor, hole)].id;
    redraw(
        &mut editor,
        cut_sketch,
        rectangle((48.0, 8.0), (52.0, 12.0)),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, sweep).unwrap().message,
        "its path edge wasn't found"
    );
    editor.undo();
    // Faces that don't meet, second of two edges.
    let other = upright_edge(&solid, block_body, 40.0, 0.0);
    let apart = EdgeRef {
        faces: [edge.faces[0], other.faces[1]],
        ..edge
    };
    let mut both = vec![edge, apart];
    both.sort_by(EdgeRef::order);
    let index = both.iter().position(|e| *e == apart).unwrap();
    let parts = vec![PathPart::Edges {
        edges: both,
        tangent: false,
    }];
    let kind = swept(&editor, profile, parts, Operation::NewBody(made));
    set(&mut editor, sweep, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, sweep).unwrap().message,
        format!("its path edge {} of 2 wasn't found", index + 1)
    );
}

/// The index of the sketch of extrude `extrude` in `editor`'s features.
fn hole_sketch(editor: &Editor, extrude: FeatureId) -> usize {
    let FeatureKind::Extrude(cut) = &editor.document().feature(extrude).unwrap().kind else {
        panic!("{extrude:?} is an extrude");
    };
    (editor.document().features().iter())
        .position(|feature| feature.id == cut.sketch)
        .unwrap()
}

/// A helix's axis is found as a move's turn's: noted for the draft's
/// reply (reversed with Flip), handed to the kernel unit, with the pitch,
/// turns and hand; an axis edge gone fails before the kernel.
#[test]
fn a_helix_s_axis_is_resolved() {
    let (mut editor, block_body, _) = block_with_hole();
    let solid = solid_of_arc(&evaluated(editor.document()), block_body);
    let edge = upright_edge(&solid, block_body, 50.0, 10.0);
    let profile = sketch_on(
        &mut editor,
        OriginPlane::XZ,
        rectangle((55.0, 0.0), (56.0, 1.0)),
    );
    let design = editor.document().design();
    let base = swept(
        &editor,
        profile,
        Vec::new(),
        Operation::NewBody(BodyId::NEW),
    );
    let helix = |axis: AxisRef, flip: bool| Sweep {
        path: PathRef::Helix(Helix {
            axis,
            pitch: Value::new("4", &Sweep::pitch_ask(&design)).unwrap(),
            turns: Value::new("2.5", &Sweep::turns_ask(&design)).unwrap(),
            left_handed: true,
            flip,
        }),
        ..base.clone()
    };
    let about_edge = add(&mut editor, helix(AxisRef::Edge(edge), false));
    let flipped = add(&mut editor, helix(AxisRef::Origin(Axis3::Z), true));
    let (evaluation, paths) = handed(editor.document());
    assert_eq!(
        failure(&evaluation, about_edge).unwrap().message,
        TOO_COMPLEX
    );
    let found = |feature: FeatureId| {
        (evaluation.references.iter())
            .find(|(id, _)| *id == feature)
            .map(|(_, found)| *found)
            .unwrap()
    };
    let [point, direction] = found(about_edge);
    assert_eq!((point.x, point.y), (50.0, 10.0));
    assert_eq!((direction.x, direction.y), (0.0, 0.0));
    assert_eq!(found(flipped), [DVec3::ZERO, -DVec3::Z]);
    let Path::Helix(handed_helix) = paths[0] else {
        panic!("{:?}", paths[0]);
    };
    assert_eq!(handed_helix.point, point);
    assert_eq!(handed_helix.axis, direction.normalize());
    assert_eq!(
        (handed_helix.pitch, handed_helix.turns, handed_helix.left),
        (4.0, 2.5, true)
    );
    assert_eq!(
        paths[1],
        Path::Helix(varde_kernel::sweep::Helix {
            point: DVec3::ZERO,
            axis: -DVec3::Z,
            pitch: 4.0,
            turns: 2.5,
            left: true,
        })
    );
    // The edge gone (faces that don't meet).
    let other = upright_edge(&solid, block_body, 40.0, 0.0);
    let apart = EdgeRef {
        faces: [edge.faces[0], other.faces[1]],
        ..edge
    };
    let made = (editor.document().feature(about_edge))
        .and_then(|feature| feature.kind.new_body())
        .unwrap();
    let gone = Sweep {
        operation: Operation::NewBody(made),
        ..helix(AxisRef::Edge(apart), false)
    };
    set(&mut editor, about_edge, gone);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, about_edge).unwrap().message,
        message::EDGE_NOT_FOUND
    );
}

/// The planned tests of the feature with the kernel's sweep, ignored
/// until it's built: analytic volumes of a pipe joined to a plate at its
/// start, cut through a block, a bead along a round rim following it
/// when the rim grows, and a spring joined to a plate.
mod with_the_kernel {
    use super::*;

    const PI: f64 = std::f64::consts::PI;

    fn assert_within(a: f64, b: f64, relative: f64) {
        assert!((a - b).abs() <= relative * b.abs(), "{a} is not {b}");
    }

    #[test]
    #[ignore = "kernel sweep not built"]
    fn a_pipe_joined_to_a_plate_at_its_start() {
        // A circle of radius 2 on the plate's underside, swept 20 down.
        let mut editor = Editor::new(Document::example());
        let profile = sketch_on(&mut editor, OriginPlane::XY, disc((20.0, 0.0), 2.0));
        let path = sketch_on(
            &mut editor,
            OriginPlane::XZ,
            polyline(vec![(20.0, 0.0), (20.0, -20.0)]),
        );
        let parts = vec![curves_part(&editor, path)];
        let sweep = add_swept(
            &mut editor,
            profile,
            parts,
            Operation::Join(Targets::default()),
        );
        let evaluation = evaluated(editor.document());
        assert!(
            failure(&evaluation, sweep).is_none(),
            "{:?}",
            evaluation.failed
        );
        assert_eq!(evaluation.bodies.len(), 1);
        assert_within(
            evaluation.bodies[0].solid.volume(),
            the_plate() + PI * 4.0 * 20.0,
            1e-5,
        );
    }

    #[test]
    #[ignore = "kernel sweep not built"]
    fn a_pipe_cut_through_a_block() {
        let mut editor = Editor::new(Document::default());
        let body = block(&mut editor, 15.0, -5.0, 25.0, 5.0, "6");
        let profile = sketch_on(&mut editor, OriginPlane::XY, disc((20.0, 0.0), 2.0));
        let path = upright(&mut editor, 10.0);
        let parts = vec![curves_part(&editor, path)];
        let sweep = add_swept(
            &mut editor,
            profile,
            parts,
            Operation::Cut(Targets::default()),
        );
        let evaluation = evaluated(editor.document());
        assert!(
            failure(&evaluation, sweep).is_none(),
            "{:?}",
            evaluation.failed
        );
        assert_within(
            solid_of(&evaluation, body).volume(),
            600.0 - PI * 4.0 * 6.0,
            1e-5,
        );
    }

    /// The volume of a cylinder of radius `big` and height 10 with a
    /// bead of radius `r` round its top rim: the cylinder, the bead's
    /// torus (Pappus), less the quarter of the bead inside the cylinder.
    fn beaded(big: f64, r: f64) -> f64 {
        let cylinder = PI * big * big * 10.0;
        let torus = PI * r * r * 2.0 * PI * big;
        let inside = PI * r * r / 4.0 * 2.0 * PI * (big - 4.0 * r / (3.0 * PI));
        cylinder + torus - inside
    }

    #[test]
    #[ignore = "kernel sweep not built"]
    fn a_bead_along_a_round_rim_follows_it() {
        let mut editor = Editor::new(Document::default());
        let extent = Extent::OneSide(length(editor.document(), "10"));
        add_extrude(
            &mut editor,
            disc((0.0, 0.0), 8.0),
            extent,
            Operation::NewBody(BodyId::NEW),
        );
        let body = editor.document().bodies()[0].id;
        let cylinder_sketch = editor.document().features()[0].id;
        let solid = solid_of_arc(&evaluated(editor.document()), body);
        let topology = solid.topology();
        let rim = (topology.chains().iter())
            .find(|chain| match edge_shape(&solid, chain) {
                EdgeShape::Circle { centre, .. } => centre.z == 10.0,
                _ => false,
            })
            .expect("the top rim");
        let mut faces = rim.regions.map(|r| topology.regions()[r as usize].key);
        faces.sort();
        let edge = EdgeRef {
            body,
            faces,
            near: DVec3::new(8.0, 0.0, 10.0),
        };
        // A circle of radius 0.5 on XZ round the rim's point on +x.
        let profile = sketch_on(&mut editor, OriginPlane::XZ, disc((8.0, 10.0), 0.5));
        let parts = vec![PathPart::Edges {
            edges: vec![edge],
            tangent: true,
        }];
        let sweep = add_swept(
            &mut editor,
            profile,
            parts,
            Operation::Join(Targets::default()),
        );
        let evaluation = evaluated(editor.document());
        assert!(
            failure(&evaluation, sweep).is_none(),
            "{:?}",
            evaluation.failed
        );
        assert_within(solid_of(&evaluation, body).volume(), beaded(8.0, 0.5), 1e-4);
        // The cylinder made wider: the rim moves out, and the profile with
        // it would be off the rim, so the bead fails rather than float.
        redraw(&mut editor, cylinder_sketch, disc((0.0, 0.0), 9.0));
        let evaluation = evaluated(editor.document());
        assert!(failure(&evaluation, sweep).is_some());
        // Its profile moved out too, it follows.
        redraw(&mut editor, profile, disc((9.0, 10.0), 0.5));
        let evaluation = evaluated(editor.document());
        assert!(
            failure(&evaluation, sweep).is_none(),
            "{:?}",
            evaluation.failed
        );
        assert_within(solid_of(&evaluation, body).volume(), beaded(9.0, 0.5), 1e-4);
    }

    #[test]
    #[ignore = "kernel sweep not built"]
    fn a_spring_joined_to_a_plate() {
        // A circle of radius 1 on XZ, 20 out from the z axis, sunk 0.5
        // into the plate's top: three turns of pitch 4 climbing from it.
        let mut editor = Editor::new(Document::example());
        let profile = sketch_on(&mut editor, OriginPlane::XZ, disc((20.0, 10.5), 1.0));
        let design = editor.document().design();
        let spring = Sweep {
            path: PathRef::Helix(Helix {
                axis: AxisRef::Origin(Axis3::Z),
                pitch: Value::new("4", &Sweep::pitch_ask(&design)).unwrap(),
                turns: Value::new("3", &Sweep::turns_ask(&design)).unwrap(),
                left_handed: false,
                flip: false,
            }),
            ..swept(
                &editor,
                profile,
                Vec::new(),
                Operation::Join(Targets::default()),
            )
        };
        let sweep = add(&mut editor, spring);
        let evaluation = evaluated(editor.document());
        assert!(
            failure(&evaluation, sweep).is_none(),
            "{:?}",
            evaluation.failed
        );
        assert_eq!(evaluation.bodies.len(), 1);
        let coil = PI * 2.0 * PI * 20.0 * 3.0;
        let volume = evaluation.bodies[0].solid.volume();
        assert!(volume > the_plate() + 0.5 * coil && volume < the_plate() + coil);
    }
}

/// A piece closed on itself has no joint: a traced rim, its conics
/// tangent only to its fit, isn't refused where its last conic meets
/// its first, but two pieces closing a loop there at the same turn are.
#[test]
fn a_piece_closed_on_itself_has_no_corner() {
    use crate::history::sweep::{Part, join};
    use varde_kernel::patch::Conic3;
    let tolerance = Tolerance::DEFAULT;
    let v = DVec3::new;
    // Four quarters round the origin on XY of radius 10, the last's
    // control point off by 1e-4: where it meets the first, the tangents
    // turn by 1e-5.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let conic = |p0: DVec3, c: DVec3, p1: DVec3| Conic3 { p0, c, w, p1 };
    let quarters = vec![
        conic(v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0), v(0.0, 10.0, 0.0)),
        conic(v(0.0, 10.0, 0.0), v(-10.0, 10.0, 0.0), v(-10.0, 0.0, 0.0)),
        conic(v(-10.0, 0.0, 0.0), v(-10.0, -10.0, 0.0), v(0.0, -10.0, 0.0)),
        conic(
            v(0.0, -10.0, 0.0),
            v(10.0 + 1e-4, -10.0, 0.0),
            v(10.0, 0.0, 0.0),
        ),
    ];
    let curve = |conics: &[Conic3]| Piece::Curve {
        conics: conics.to_vec(),
        normal: None,
    };
    let start = (DVec3::ZERO, DVec3::Y);
    let one = Part {
        pieces: vec![curve(&quarters)],
        closed: true,
    };
    let path = join(vec![one], start, || DVec3::ZERO, &tolerance);
    assert!(
        matches!(&path, Ok(Path::Chain { closed: true, .. })),
        "{:?}",
        path.err().map(|failed| failed.message)
    );
    // Halves of it as two pieces: their joint where the loop closes is
    // a corner past the sine.
    let two = Part {
        pieces: vec![curve(&quarters[..2]), curve(&quarters[2..])],
        closed: true,
    };
    let failed = join(vec![two], start, || DVec3::ZERO, &tolerance).unwrap_err();
    assert_eq!(failed.message, message::PATH_CORNER);
}
