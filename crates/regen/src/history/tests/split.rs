//! Splits in the history. The kernel's split and its tools aren't built
//! yet (they fail as too complex), so: each tool reaching the kernel
//! fails the split with the too-complex message, changing no body, and
//! the rest of the history goes on (a feature naming the new body fails
//! for want of a solid); regen's own refusals come first (a face not
//! found or not flat, a line that isn't one open chain). With the
//! kernel's split replaced by two booleans (the front `a ∩ t`, the back
//! `a − t`), what regeneration does with the pieces: the kept piece to
//! the body, the other to the new body or dropped, `original` and
//! `keep`, a side empty refused, a sketch on a face that went to the new
//! body following it there while a mirror's face doesn't, and a draft's
//! two pieces. The analytic volumes of the planned tools are written
//! out, ignored until the kernel's split is built.

use glam::{DVec2, DVec3};
use varde_document::{FaceRef, Keep, Mirror, Move, PlaneRef, Side, Split, SplitTool, Targets};
use varde_sketch::Id;

use super::motion::{add, block, failure, key_on, set};
use super::*;
use crate::Draft;

/// Splits on this test's thread by two booleans (the front `a ∩ t`, the
/// back `a − t`).
fn with_booleans() {
    super::super::split::split_by_booleans();
}

/// `body` split by `tool`, both sides kept, the front keeping the id.
fn split(body: BodyId, tool: SplitTool) -> Split {
    Split {
        body,
        tool,
        original: Side::Front,
        keep: Keep::Both,
        new_body: Some(BodyId::NEW),
    }
}

/// The new body of split `feature`.
fn new_body(editor: &Editor, feature: FeatureId) -> BodyId {
    match &editor.document().feature(feature).unwrap().kind {
        FeatureKind::Split(split) => split.new_body.unwrap(),
        other => panic!("{other:?}"),
    }
}

/// The 10 mm cube from the origin, and a block over its part past
/// `x = 6` (from 1 mm below to 2 mm above it, wider): the editor and
/// the two bodies.
fn cube_and_tool() -> (Editor, BodyId, BodyId) {
    let mut editor = Editor::new(Document::default());
    let cube = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let extent = two_sides(editor.document(), "12", "1");
    let new = Operation::NewBody(BodyId::NEW);
    add_extrude(
        &mut editor,
        rectangle((6.0, -1.0), (11.0, 11.0)),
        extent,
        new,
    );
    let tool = editor.document().bodies().last().unwrap().id;
    (editor, cube, tool)
}

/// Adds a sketch on XY drawn by `draw`: its id.
fn add_sketch(editor: &mut Editor, draw: impl FnOnce(&mut Sketch)) -> FeatureId {
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
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

/// The sketch feature `feature`'s sketch.
fn sketch_of(editor: &Editor, feature: FeatureId) -> &Sketch {
    match &editor.document().feature(feature).unwrap().kind {
        FeatureKind::Sketch { sketch, .. } => sketch,
        other => panic!("{other:?}"),
    }
}

/// The lines through `points` in turn, open: their ids, sorted.
fn polyline(editor: &mut Editor, points: &'static [(f64, f64)]) -> (FeatureId, Vec<Id>) {
    let sketch = add_sketch(editor, |sketch| {
        let ids: Vec<_> = (points.iter())
            .map(|&(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap())
            .collect();
        for pair in ids.windows(2) {
            let line = Curve::Line {
                start: pair[0],
                end: pair[1],
            };
            sketch.add_curve(line, false).unwrap();
        }
    });
    let mut curves: Vec<Id> = (sketch_of(editor, sketch).curves.iter())
        .map(|entry| entry.id)
        .collect();
    curves.sort_unstable();
    (sketch, curves)
}

/// The too-complex message of splitting `body` in the kernel.
fn too_complex(body: &str) -> String {
    format!(
        "splitting {body} is too complex to work out: they may meet on faces that are tangent \
         or nearly flush"
    )
}

/// The too-complex message of building a tool past `body`.
fn tool_too_complex(body: &str) -> String {
    format!("extending its tool past {body} is too complex to work out")
}

/// A split the kernel can't do yet fails as too complex, leaving the
/// body as it was and making no new body; the history goes on: a move
/// of the body moves it whole, a move of the new body fails for want of
/// a solid.
#[test]
fn a_split_the_kernel_cant_do_fails_as_too_complex() {
    let (mut editor, cube, tool) = cube_and_tool();
    let by_body = add(&mut editor, split(cube, SplitTool::Body(tool)));
    let piece = new_body(&editor, by_body);
    let offsets = ["5", "0", "0"]
        .map(|text| Value::new(text, &Move::offset_ask(&editor.document().design())).unwrap());
    let moved = add(
        &mut editor,
        Move {
            bodies: vec![cube],
            offset: offsets.clone(),
            turn: None,
        },
    );
    let lost = add(
        &mut editor,
        Move {
            bodies: vec![piece],
            offset: offsets,
            turn: None,
        },
    );
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, by_body).unwrap();
    assert_eq!(failed.message, too_complex("Body 1"));
    assert!(failure(&evaluation, moved).is_none());
    assert_eq!(
        failure(&evaluation, lost).unwrap().message,
        "Body 3 has no solid: the feature making it failed"
    );
    let moved = solid_of(&evaluation, cube);
    assert_near(moved.volume(), 1000.0);
    assert!(super::motion::boxed(
        moved,
        [5.0, 0.0, 0.0],
        [15.0, 10.0, 10.0]
    ));
    assert!(evaluation.bodies.iter().all(|made| made.body != piece));
    assert!(evaluation.splits.is_empty());
}

/// Each tool fails as the kernel's half-spaces, surfaces and chain tools
/// do for now (too complex), a sketch's regions (extruded by the
/// kernel's extrude) as its split does.
#[test]
fn every_tool_reaches_the_kernel_and_fails_as_too_complex() {
    let (mut editor, cube, _) = cube_and_tool();
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let side = FaceRef {
        body: cube,
        key: key_on(&solid, DVec3::X, 10.0),
        near: DVec3::new(10.0, 5.0, 5.0),
    };
    let (line_sketch, lines) = polyline(&mut editor, &[(5.0, -2.0), (5.0, 12.0)]);
    let regions_sketch = add_sketch(&mut editor, rectangle((2.0, 2.0), (4.0, 4.0)));
    let FeatureKind::Sketch { sketch, .. } =
        &editor.document().feature(regions_sketch).unwrap().kind
    else {
        unreachable!()
    };
    let region = sketch.profiles().unwrap().reference(0).unwrap();
    let tools = [
        SplitTool::Plane(PlaneRef::Origin(OriginPlane::YZ)),
        SplitTool::Plane(PlaneRef::Face(side)),
        SplitTool::Face(side),
        SplitTool::Chain {
            sketch: line_sketch,
            curves: lines,
        },
        SplitTool::Regions {
            sketch: regions_sketch,
            regions: vec![region],
        },
    ];
    let mut ids = Vec::new();
    for tool in tools {
        ids.push(add(&mut editor, split(cube, tool)));
    }
    let evaluation = evaluated(editor.document());
    let messages: Vec<&str> = (ids.iter())
        .map(|&id| failure(&evaluation, id).unwrap().message.as_str())
        .collect();
    let tool = tool_too_complex("Body 1");
    assert_eq!(
        messages,
        [&tool, &tool, &tool, &tool, &too_complex("Body 1")].map(String::as_str)
    );
    assert_near(solid_of(&evaluation, cube).volume(), 1000.0);
}

/// Regen's own refusals come before the kernel: a face that isn't there
/// or isn't flat, a face's body gone, a line that's closed, branches or
/// is in pieces, a curve gone since.
#[test]
fn faces_and_lines_are_refused_before_the_kernel() {
    let (mut editor, cube, tool) = cube_and_tool();
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let top = key_on(&solid, DVec3::Z, 10.0);
    // A face key no face has.
    let gone = FaceRef {
        body: cube,
        key: FaceKey { instance: 7, ..top },
        near: DVec3::new(5.0, 5.0, 10.0),
    };
    let plane_gone = add(
        &mut editor,
        split(cube, SplitTool::Plane(PlaneRef::Face(gone))),
    );
    let face_gone = add(&mut editor, split(cube, SplitTool::Face(gone)));
    // A disc's wall isn't flat.
    let disc_body = add_body(&mut editor, disc((30.0, 0.0), 2.0), "5");
    let disc_solid = solid_of(&evaluated(editor.document()), disc_body).clone();
    let wall = FaceRef {
        body: disc_body,
        key: super::motion::cylinder(&disc_solid),
        near: DVec3::new(32.0, 0.0, 2.0),
    };
    let not_flat = add(
        &mut editor,
        split(cube, SplitTool::Plane(PlaneRef::Face(wall))),
    );
    // Lines: a circle, three lines from a point, two apart, a loop.
    let (circle_sketch, circles) = {
        let sketch = add_sketch(&mut editor, disc((5.0, 5.0), 2.0));
        let curves = (sketch_of(&editor, sketch).curves.iter())
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        (sketch, curves)
    };
    let star = add_sketch(&mut editor, |sketch| {
        let middle = sketch.add_point(DVec2::new(5.0, 5.0)).unwrap();
        for (x, y) in [(-2.0, 5.0), (12.0, 5.0), (5.0, 12.0)] {
            let end = sketch.add_point(DVec2::new(x, y)).unwrap();
            let line = Curve::Line { start: middle, end };
            sketch.add_curve(line, false).unwrap();
        }
    });
    let mut star_curves: Vec<Id> = (sketch_of(&editor, star).curves.iter())
        .map(|entry| entry.id)
        .collect();
    star_curves.sort_unstable();
    let (apart, apart_curves) = {
        let sketch = add_sketch(&mut editor, |sketch| {
            for x in [3.0, 7.0] {
                let start = sketch.add_point(DVec2::new(x, -2.0)).unwrap();
                let end = sketch.add_point(DVec2::new(x, 12.0)).unwrap();
                sketch.add_curve(Curve::Line { start, end }, false).unwrap();
            }
        });
        let curves = (sketch_of(&editor, sketch).curves.iter())
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        (sketch, curves)
    };
    let (looped, loop_curves) = {
        let sketch = add_sketch(&mut editor, rectangle((2.0, 2.0), (4.0, 4.0)));
        let curves = (sketch_of(&editor, sketch).curves.iter())
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        (sketch, curves)
    };
    let chain = |sketch, curves| SplitTool::Chain { sketch, curves };
    let circle = add(&mut editor, split(cube, chain(circle_sketch, circles)));
    let branches = add(&mut editor, split(cube, chain(star, star_curves)));
    let pieces = add(&mut editor, split(cube, chain(apart, apart_curves)));
    let closed = add(&mut editor, split(cube, chain(looped, loop_curves.clone())));
    // A curve deleted from its sketch since.
    let mut edited = sketch_of(&editor, looped).clone();
    edited.delete(&[loop_curves[0]]);
    editor
        .apply(Command::SetSketch {
            feature: looped,
            sketch: Box::new(edited),
        })
        .unwrap();
    let evaluation = evaluated(editor.document());
    let message = |id| failure(&evaluation, id).unwrap().message.as_str();
    assert_eq!(message(plane_gone), "its plane face wasn't found");
    assert_eq!(message(face_gone), "its face wasn't found");
    assert_eq!(message(not_flat), "its plane face isn't flat");
    assert!(failure(&evaluation, not_flat).unwrap().geometry.is_some());
    assert_eq!(
        message(circle),
        "its line is closed: split with the region it encloses instead"
    );
    let branching = "its line's curves don't join end to end into one line";
    assert_eq!(message(branches), branching);
    assert_eq!(message(pieces), branching);
    assert_eq!(message(closed), "its line's curves weren't found");
    let _ = tool;
}

/// An open chain is ordered end to end, runs as its lowest curve does,
/// and its conics meet to the bit: an arc and two lines given out of
/// order.
#[test]
fn an_open_chain_is_joined_end_to_end() {
    let mut editor = Editor::new(Document::default());
    let sketch = add_sketch(&mut editor, |sketch| {
        let p = [(0.0, 0.0), (10.0, 0.0), (20.0, 10.0), (20.0, 20.0)]
            .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
        let center = sketch.add_point(DVec2::new(10.0, 10.0)).unwrap();
        // The arc first by id, counter-clockwise from (10, 0) to (20, 10).
        let arc = Curve::Arc {
            center,
            start: p[1],
            end: p[2],
        };
        sketch.add_curve(arc, false).unwrap();
        // Then the last line drawn backwards, and the first line.
        let last = Curve::Line {
            start: p[3],
            end: p[2],
        };
        sketch.add_curve(last, false).unwrap();
        let first = Curve::Line {
            start: p[0],
            end: p[1],
        };
        sketch.add_curve(first, false).unwrap();
    });
    let drawn = sketch_of(&editor, sketch);
    let ids: Vec<Id> = drawn.curves.iter().map(|entry| entry.id).collect();
    let segments = crate::profile::chain(drawn, &ids, 1e-9, 1e-3).unwrap();
    // The first line, the arc (one quarter), the last line.
    let curves: Vec<u64> = segments.iter().map(|s| s.curve).collect();
    let id = |k: usize| u64::from(ids[k].get());
    assert_eq!(curves, [id(2), id(0), id(1)]);
    for pair in segments.windows(2) {
        assert_eq!(pair[0].conic.p1, pair[1].conic.p0);
    }
    assert_eq!(segments[0].conic.p0, DVec2::ZERO);
    assert_eq!(segments[2].conic.p1, DVec2::new(20.0, 20.0));
    // A loop of lines is closed, a lone line its own chain.
    let looped = add_sketch(&mut editor, rectangle((0.0, 0.0), (1.0, 1.0)));
    let drawn = sketch_of(&editor, looped);
    let ids: Vec<Id> = drawn.curves.iter().map(|entry| entry.id).collect();
    let closed = crate::profile::chain(drawn, &ids, 1e-9, 1e-3);
    assert_eq!(closed.err(), Some(crate::profile::ChainError::Closed));
    let one = crate::profile::chain(drawn, &ids[..1], 1e-9, 1e-3).unwrap();
    assert_eq!(one.len(), 1);
}

/// The front, the part inside the tool body, keeps the body's id and
/// the back goes to the new body; with `original` Back they swap, and
/// keeping one side drops the other and makes no body. Volumes as the
/// booleans give them.
#[test]
fn the_pieces_go_to_the_body_and_the_new_body() {
    with_booleans();
    let (mut editor, cube, tool) = cube_and_tool();
    let id = add(&mut editor, split(cube, SplitTool::Body(tool)));
    let piece = new_body(&editor, id);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_near(solid_of(&evaluation, cube).volume(), 400.0);
    assert_near(solid_of(&evaluation, piece).volume(), 600.0);
    assert_near(solid_of(&evaluation, tool).volume(), 5.0 * 12.0 * 13.0);
    assert_eq!(evaluation.splits, [(cube, piece)]);
    // The new body comes last, made by the split.
    assert_eq!(evaluation.bodies.last().unwrap().body, piece);

    let back = Split {
        original: Side::Back,
        ..split(cube, SplitTool::Body(tool))
    };
    set(&mut editor, id, back);
    assert_eq!(new_body(&editor, id), piece);
    let evaluation = evaluated(editor.document());
    assert_near(solid_of(&evaluation, cube).volume(), 600.0);
    assert_near(solid_of(&evaluation, piece).volume(), 400.0);

    for (keep, volume) in [(Keep::Front, 400.0), (Keep::Back, 600.0)] {
        let trim = Split {
            keep,
            ..split(cube, SplitTool::Body(tool))
        };
        set(&mut editor, id, trim);
        assert!(editor.document().body(piece).is_none());
        let evaluation = evaluated(editor.document());
        assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
        assert_near(solid_of(&evaluation, cube).volume(), volume);
        assert_eq!(evaluation.bodies.len(), 2);
        assert!(evaluation.splits.is_empty());
    }
}

/// A sketch's regions are extruded through the body both ways: the
/// front is the part over them.
#[test]
fn a_sketch_region_splits_through_all() {
    with_booleans();
    let mut editor = Editor::new(Document::default());
    let cube = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let sketch = add_sketch(&mut editor, rectangle((6.0, -1.0), (11.0, 11.0)));
    let FeatureKind::Sketch { sketch: drawn, .. } =
        &editor.document().feature(sketch).unwrap().kind
    else {
        unreachable!()
    };
    let region = drawn.profiles().unwrap().reference(0).unwrap();
    let tool = SplitTool::Regions {
        sketch,
        regions: vec![region],
    };
    let id = add(&mut editor, split(cube, tool));
    assert!(!editor.document().feature(sketch).unwrap().visible);
    let piece = new_body(&editor, id);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_near(solid_of(&evaluation, cube).volume(), 400.0);
    assert_near(solid_of(&evaluation, piece).volume(), 600.0);
    // The walls inside the body are named by the split.
    let named = (solid_of(&evaluation, cube).mesh().faces().iter())
        .any(|face| face.name.feature == id.get());
    assert!(named);
}

/// A tool missing the body, or holding it all, leaves a side empty:
/// refused, changing nothing.
#[test]
fn a_side_left_empty_is_refused() {
    with_booleans();
    let mut editor = Editor::new(Document::default());
    let cube = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let apart = block(&mut editor, 20.0, 0.0, 30.0, 10.0, "10");
    let extent = two_sides(editor.document(), "12", "1");
    let new = Operation::NewBody(BodyId::NEW);
    add_extrude(
        &mut editor,
        rectangle((-1.0, -1.0), (11.0, 11.0)),
        extent,
        new,
    );
    let around = editor.document().bodies().last().unwrap().id;
    let missed = add(&mut editor, split(cube, SplitTool::Body(apart)));
    let held = add(&mut editor, split(cube, SplitTool::Body(around)));
    let evaluation = evaluated(editor.document());
    let one_side = "Body 1 lies all on one side: the tool doesn't cut it in two";
    assert_eq!(failure(&evaluation, missed).unwrap().message, one_side);
    assert_eq!(failure(&evaluation, held).unwrap().message, one_side);
    assert_near(solid_of(&evaluation, cube).volume(), 1000.0);
}

/// Later features naming the body get the piece that kept its id: a
/// mirror in the cube's face at x = 0, which went to the new body, fails
/// as a face not found; a sketch on that face follows it into the new
/// body, and one on the face at x = 10 stays on the body.
#[test]
fn a_sketch_on_a_face_follows_it_into_the_new_body() {
    with_booleans();
    let (mut editor, cube, tool) = cube_and_tool();
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let face = |n: DVec3, d: f64, near: DVec3| FaceRef {
        body: cube,
        key: key_on(&solid, n, d),
        near,
    };
    let left = face(DVec3::NEG_X, 0.0, DVec3::new(0.0, 5.0, 5.0));
    let right = face(DVec3::X, 10.0, DVec3::new(10.0, 5.0, 5.0));
    let id = add(&mut editor, split(cube, SplitTool::Body(tool)));
    let piece = new_body(&editor, id);
    let mirror = add(
        &mut editor,
        Mirror {
            bodies: vec![cube],
            plane: PlaneRef::Face(left),
            keep_original: false,
        },
    );
    editor
        .apply(editor.document().add_sketch(Plane::Face(left)))
        .unwrap();
    let on_left = editor.document().features().last().unwrap().id;
    editor
        .apply(editor.document().add_sketch(Plane::Face(right)))
        .unwrap();
    let on_right = editor.document().features().last().unwrap().id;
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, mirror).unwrap().message,
        "its mirror face wasn't found"
    );
    let placed = |sketch| {
        (evaluation.placements.iter())
            .find(|(id, _)| *id == sketch)
            .map(|(_, placement)| placement.origin)
    };
    assert!(failure(&evaluation, on_left).is_none());
    assert_eq!(placed(on_left).unwrap().x, 0.0);
    assert_eq!(placed(on_right).unwrap().x, 10.0);
    // The left face is on the new body only.
    let topology = solid_of(&evaluation, piece).topology();
    assert!(
        (topology.face(solid_of(&evaluation, piece), &left.key, left.near)).is_ok(),
        "the face went to the new body"
    );
    // Keeping only the front, the face is gone with the back.
    let trim = Split {
        keep: Keep::Front,
        ..split(cube, SplitTool::Body(tool))
    };
    set(&mut editor, id, trim);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, on_left).unwrap().message,
        "its face wasn't found"
    );
}

/// A sketch on a face follows it through a chain of splits and merges:
/// the cube's face at x = 0 goes to the first split's new body, which a
/// combine merges into another block, which a second split cuts, its
/// new body (the front, `original` Back) getting the face; the sketch,
/// naming the cube, is placed on it there.
#[test]
fn a_sketch_on_a_face_follows_it_through_splits_and_merges() {
    with_booleans();
    let (mut editor, cube, tool) = cube_and_tool();
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let left = FaceRef {
        body: cube,
        key: key_on(&solid, DVec3::NEG_X, 0.0),
        near: DVec3::new(0.0, 5.0, 5.0),
    };
    let first = add(&mut editor, split(cube, SplitTool::Body(tool)));
    let piece = new_body(&editor, first);
    let post = block(&mut editor, 2.0, 2.0, 4.0, 4.0, "15");
    add(
        &mut editor,
        varde_document::Combine {
            target: post,
            tools: vec![piece],
            op: varde_document::BodyOp::Union,
            keep_tools: false,
        },
    );
    let extent = two_sides(editor.document(), "20", "1");
    add_extrude(
        &mut editor,
        rectangle((-1.0, -1.0), (3.0, 11.0)),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let cutter = editor.document().bodies().last().unwrap().id;
    let second = add(
        &mut editor,
        Split {
            original: Side::Back,
            ..split(post, SplitTool::Body(cutter))
        },
    );
    let last = new_body(&editor, second);
    editor
        .apply(editor.document().add_sketch(Plane::Face(left)))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(evaluation.splits, [(cube, piece), (post, last)]);
    assert_eq!(evaluation.holder(piece), Some(post));
    let placed = (evaluation.placements.iter())
        .find(|(id, _)| *id == sketch)
        .map(|(_, placement)| *placement)
        .expect("placed");
    assert_eq!(placed.origin.x, 0.0);
    assert_eq!(placed.normal, DVec3::NEG_X);
    // The face is on the second split's new body, and only there.
    for (body, there) in [(cube, false), (post, false), (last, true)] {
        let solid = solid_of(&evaluation, body);
        let found = solid.topology().face(solid, &left.key, left.near).is_ok();
        assert_eq!(found, there, "{body:?}");
    }
}

/// A split drafted is answered with both pieces drawn; with the kernel's
/// split, with the too-complex message.
#[test]
fn a_split_draft_is_answered() {
    let (editor, cube, tool) = cube_and_tool();
    let draft = Draft {
        revision: 1,
        feature: None,
        kind: split(cube, SplitTool::Body(tool)).into(),
    };
    let answer = crate::tests::answered(crate::handle(crate::tests::regenerate_with(
        &editor,
        Some(draft.clone()),
    )));
    let drafted = answer.draft.unwrap();
    assert_eq!(drafted.error, Some(too_complex("Body 1")));
    assert_eq!(answer.parts.len(), 2);
    with_booleans();
    let answer = crate::tests::answered(crate::handle(crate::tests::regenerate_with(
        &editor,
        Some(draft),
    )));
    assert_eq!(answer.draft.unwrap().error, None);
    assert_eq!(answer.parts.len(), 3);
}

/// An unchanged split is found in the cache: its tool and both pieces.
#[test]
fn an_unchanged_split_is_found_in_the_cache() {
    with_booleans();
    let (mut editor, cube, tool) = cube_and_tool();
    add(&mut editor, split(cube, SplitTool::Body(tool)));
    let mut cache = Cache::default();
    cache.begin();
    evaluate(editor.document(), &mut cache);
    let (_, misses) = cache.counts();
    cache.begin();
    let evaluation = evaluate(editor.document(), &mut cache);
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(cache.counts().1, misses, "nothing worked out again");
}

/// A line of the most curves a split takes, 256 lines zigzagging, every
/// other one drawn backwards, joins end to end at once (the joints
/// found by pairs of ends, 130 000 or so), its conics meeting to the bit
/// and running as the lowest curve does; and arcs run against their
/// way round stay on their circles.
#[test]
fn a_line_of_the_most_curves_joins_at_once() {
    use varde_document::MAX_SPLIT_CURVES;
    let mut editor = Editor::new(Document::default());
    let sketch = add_sketch(&mut editor, |sketch| {
        let points: Vec<Id> = (0..=MAX_SPLIT_CURVES)
            .map(|k| {
                let at = DVec2::new(k as f64, (k % 2) as f64);
                sketch.add_point(at).unwrap()
            })
            .collect();
        for (k, pair) in points.windows(2).enumerate() {
            let (start, end) = if k % 2 == 0 {
                (pair[0], pair[1])
            } else {
                (pair[1], pair[0])
            };
            sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        }
    });
    let drawn = sketch_of(&editor, sketch);
    let mut ids: Vec<Id> = drawn.curves.iter().map(|entry| entry.id).collect();
    ids.sort_unstable();
    assert_eq!(ids.len(), MAX_SPLIT_CURVES);
    let started = std::time::Instant::now();
    let segments = crate::profile::chain(drawn, &ids, 1e-9, 1e-3).unwrap();
    assert!(
        started.elapsed().as_secs_f64() < 1.0,
        "{:?}",
        started.elapsed()
    );
    assert_eq!(segments.len(), MAX_SPLIT_CURVES);
    assert_eq!(segments[0].conic.p0, DVec2::ZERO);
    for pair in segments.windows(2) {
        assert_eq!(pair[0].conic.p1, pair[1].conic.p0);
    }
    let last = MAX_SPLIT_CURVES as f64;
    assert_eq!(segments.last().unwrap().conic.p1, DVec2::new(last, 0.0));
    // One more is refused.
    let tool = SplitTool::Chain {
        sketch,
        curves: (0..=MAX_SPLIT_CURVES).map(|_| ids[0]).collect(),
    };
    let over = split(BodyId::NEW, tool).check_own();
    assert_eq!(over, Err(varde_document::SplitError::Curves(257)));

    // A line first by id, then a half circle about (10, 0) drawn
    // counter-clockwise from (20, 0) to (0, 0), so run backwards.
    let bent = add_sketch(&mut editor, |sketch| {
        let p = [(30.0, 0.0), (20.0, 0.0), (0.0, 0.0)]
            .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
        let center = sketch.add_point(DVec2::new(10.0, 0.0)).unwrap();
        let line = Curve::Line {
            start: p[0],
            end: p[1],
        };
        sketch.add_curve(line, false).unwrap();
        let arc = Curve::Arc {
            center,
            start: p[2],
            end: p[1],
        };
        sketch.add_curve(arc, false).unwrap();
    });
    let drawn = sketch_of(&editor, bent);
    let mut ids: Vec<Id> = drawn.curves.iter().map(|entry| entry.id).collect();
    ids.sort_unstable();
    let segments = crate::profile::chain(drawn, &ids, 1e-9, 1e-3).unwrap();
    // The line, then the half circle in two quarters, run clockwise
    // from (20, 0) through (10, -10) back along the way it's drawn.
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[2].conic.p1, DVec2::ZERO);
    for segment in &segments[1..] {
        for t in [0.25, 0.5, 0.75] {
            let at = segment.conic.eval(t);
            assert!(
                (at.distance(DVec2::new(10.0, 0.0)) - 10.0).abs() < 1e-9,
                "{at}"
            );
        }
    }
    assert!(segments[1].conic.p1.y < -9.0, "{:?}", segments[1].conic);
}

/// Ends join where they're one point or within the resolution, and not
/// past it: two lines whose ends are a little apart join below it and
/// are in pieces above it.
#[test]
fn a_line_s_ends_join_within_the_resolution_only() {
    let mut editor = Editor::new(Document::default());
    for (gap, joins) in [(0.5e-6, true), (2e-6, false)] {
        let sketch = add_sketch(&mut editor, move |sketch| {
            let p = [(0.0, 0.0), (10.0, 0.0), (10.0, gap), (10.0, 10.0)]
                .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
            for (start, end) in [(p[0], p[1]), (p[2], p[3])] {
                sketch.add_curve(Curve::Line { start, end }, false).unwrap();
            }
        });
        let drawn = sketch_of(&editor, sketch);
        let mut ids: Vec<Id> = drawn.curves.iter().map(|entry| entry.id).collect();
        ids.sort_unstable();
        let joined = crate::profile::chain(drawn, &ids, 1e-6, 1e-3);
        if joins {
            let segments = joined.unwrap();
            assert_eq!(segments[0].conic.p1, segments[1].conic.p0);
            assert_eq!(segments[1].conic.p0, DVec2::new(10.0, 0.0));
        } else {
            assert_eq!(joined.err(), Some(crate::profile::ChainError::Branches));
        }
    }
}

// The planned tests of the kernel's split, with analytic volumes: ignored
// until it's built (they fail as too complex now).

/// `body`'s solid in `editor`'s document, regenerated, with nothing
/// failed.
fn solid_after(editor: &Editor, body: BodyId) -> Arc<Solid> {
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let made = evaluation.bodies.iter().find(|made| made.body == body);
    Arc::clone(&made.unwrap().solid)
}

/// The cube from z = -5 split by XY: the front (above) and the back
/// (below), half each, put back together the cube.
#[test]
#[ignore = "kernel split not built"]
fn kernel_a_box_split_by_xy() {
    let mut editor = Editor::new(Document::default());
    let extent = two_sides(editor.document(), "5", "5");
    add_extrude(
        &mut editor,
        rectangle((0.0, 0.0), (10.0, 10.0)),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let cube = editor.document().bodies()[0].id;
    let id = add(
        &mut editor,
        split(cube, SplitTool::Plane(PlaneRef::Origin(OriginPlane::XY))),
    );
    let piece = new_body(&editor, id);
    let (front, back) = (solid_after(&editor, cube), solid_after(&editor, piece));
    assert_near(front.volume(), 500.0);
    assert_near(back.volume(), 500.0);
    assert!(super::motion::boxed(&front, [0.0; 3], [10.0, 10.0, 5.0]));
    assert!(super::motion::boxed(
        &back,
        [0.0, 0.0, -5.0],
        [10.0, 10.0, 0.0]
    ));
}

/// The cube split by a cylinder's wall extended (a disc of radius 3
/// about x = y = 5 elsewhere): a core and a ring.
#[test]
#[ignore = "kernel split not built"]
fn kernel_a_box_split_by_a_cylinder_face() {
    let mut editor = Editor::new(Document::default());
    let cube = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let rod = add_body(&mut editor, disc((5.0, 5.0), 3.0), "30");
    let rod_solid = solid_after(&editor, rod);
    let wall = FaceRef {
        body: rod,
        key: super::motion::cylinder(&rod_solid),
        near: DVec3::new(8.0, 5.0, 20.0),
    };
    let id = add(&mut editor, split(cube, SplitTool::Face(wall)));
    let piece = new_body(&editor, id);
    let core = PI * 9.0 * 10.0;
    let fit = editor.document().tolerance().fit();
    let near = |a: f64, b: f64| (a - b).abs() <= 2.0 * PI * 3.0 * 10.0 * fit;
    assert!(near(solid_after(&editor, cube).volume(), core));
    assert!(near(solid_after(&editor, piece).volume(), 1000.0 - core));
}

/// The cube split by the plane of an L-shaped body's step, flush with
/// the step: the two arms.
#[test]
#[ignore = "kernel split not built"]
fn kernel_a_body_split_by_its_own_face_plane() {
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 20.0, 10.0, "5");
    let extent = Extent::OneSide(length(editor.document(), "10"));
    let join = Operation::Join(Targets::default());
    add_extrude(
        &mut editor,
        rectangle((0.0, 0.0), (10.0, 10.0)),
        extent,
        join,
    );
    let solid = solid_after(&editor, body);
    let step = FaceRef {
        body,
        key: key_on(&solid, DVec3::Z, 5.0),
        near: DVec3::new(15.0, 5.0, 5.0),
    };
    let id = add(
        &mut editor,
        split(body, SplitTool::Plane(PlaneRef::Face(step))),
    );
    let piece = new_body(&editor, id);
    assert_near(solid_after(&editor, body).volume(), 500.0);
    assert_near(solid_after(&editor, piece).volume(), 1000.0);
}

/// The cube split by another body keeps the tool body as it was.
#[test]
#[ignore = "kernel split not built"]
fn kernel_a_box_split_by_another_body() {
    let (mut editor, cube, tool) = cube_and_tool();
    let id = add(&mut editor, split(cube, SplitTool::Body(tool)));
    let piece = new_body(&editor, id);
    assert_near(solid_after(&editor, cube).volume(), 400.0);
    assert_near(solid_after(&editor, piece).volume(), 600.0);
    assert_near(solid_after(&editor, tool).volume(), 5.0 * 12.0 * 13.0);
}

/// The cube split by an open line across it, a bend of two lines: the
/// front on the line's left.
#[test]
#[ignore = "kernel split not built"]
fn kernel_a_box_split_by_an_open_line() {
    let mut editor = Editor::new(Document::default());
    let cube = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    // Up x = 4 to y = 5, then right along y = 5: its left is where
    // x < 4 or y > 5.
    let (sketch, curves) = polyline(&mut editor, &[(4.0, -2.0), (4.0, 5.0), (12.0, 5.0)]);
    let id = add(
        &mut editor,
        split(cube, SplitTool::Chain { sketch, curves }),
    );
    let piece = new_body(&editor, id);
    let left = (4.0 * 10.0 + 6.0 * 5.0) * 10.0;
    assert_near(solid_after(&editor, cube).volume(), left);
    assert_near(solid_after(&editor, piece).volume(), 1000.0 - left);
}

/// Splitting is deterministic: the pieces' meshes are the same to the
/// bit, run twice.
#[test]
#[ignore = "kernel split not built"]
fn kernel_splits_are_deterministic() {
    let (mut editor, cube, tool) = cube_and_tool();
    add(&mut editor, split(cube, SplitTool::Body(tool)));
    let a = evaluated(editor.document());
    let b = evaluated(editor.document());
    assert!(a.failed.is_empty(), "{:?}", a.failed);
    assert_eq!(a.bodies.len(), 3);
    for (x, y) in a.bodies.iter().zip(&b.bodies) {
        assert_eq!(x.solid.mesh().verts(), y.solid.mesh().verts());
    }
}

mod fuzz;
