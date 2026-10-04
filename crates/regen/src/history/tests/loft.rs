//! Lofts in the history. The kernel's loft isn't built yet (it fails as
//! too complex), so: a loft reaching the kernel fails with the
//! too-complex message, changing no body, and the rest of the history
//! goes on; regen's own refusals come first (a section's region or
//! point gone, its sketch not placed, holes, a start point off its
//! corners, two sections on one plane, a rail's curves). With the
//! kernel's loft replaced by a stand-in extruding the first of two
//! equal parallel sections to the second: a box by its volume, a
//! transition piece joined between two bosses, a section's sketch
//! edited and a section moved with the loft following, sections handed
//! over in order and placed, twisted starts, the cache. The kernel's
//! own analytic tests are in `varde_kernel::loft`'s, ignored until it's
//! built.

use std::cell::RefCell;

use glam::DVec3;
use varde_document::{CurveChain, FaceRef, Loft, LoftMode, Move, Section};
use varde_kernel::loft::{LoftError, Rail};
use varde_kernel::mesh::{FaceKey, PartKey};
use varde_kernel::{Budget, KernelError};

use super::*;

/// Lofts on this test's thread by the extrude stand-in.
fn with_extrudes() {
    super::super::loft::loft_by_extrude();
}

/// What [`recording`] was asked once: the sections, whether closed, the
/// rails.
type Asked = (Vec<varde_kernel::loft::Section>, bool, Vec<Rail>);

thread_local! {
    /// What [`recording`] was asked to loft, on this test's thread.
    static ASKED: RefCell<Vec<Asked>> =
        const { RefCell::new(Vec::new()) };
    /// What [`recording`] answers.
    static ANSWER: RefCell<Option<LoftError>> = const { RefCell::new(None) };
}

/// A loft that notes what it was asked and fails with [`ANSWER`] (too
/// complex unless set).
fn recording(
    sections: &[varde_kernel::loft::Section],
    _mode: varde_kernel::loft::LoftMode,
    closed: bool,
    rails: &[Rail],
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, LoftError> {
    ASKED.with_borrow_mut(|asked| asked.push((sections.to_vec(), closed, rails.to_vec())));
    Err(ANSWER
        .with_borrow(Clone::clone)
        .unwrap_or_else(|| LoftError::Failed(KernelError::TooComplex.into())))
}

/// Lofts on this test's thread by [`recording`], answering `answer`.
fn with_recording(answer: Option<LoftError>) {
    super::super::loft::LOFTER.set(Some(recording));
    ANSWER.set(answer);
    ASKED.with_borrow_mut(Vec::clear);
}

/// What [`recording`] was asked, taken.
fn asked() -> Vec<Asked> {
    ASKED.with_borrow_mut(std::mem::take)
}

const TOO_COMPLEX: &str = "lofting its sections is too complex to work out";

/// Adds a sketch on `plane` drawn by `draw`: its id and what `draw`
/// gave.
fn add_sketch<T>(
    editor: &mut Editor,
    plane: Plane,
    draw: impl FnOnce(&mut Sketch) -> T,
) -> (FeatureId, T) {
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = Sketch::default();
    let drawn = draw(&mut sketch);
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    (feature, drawn)
}

/// The sketch of sketch feature `feature`, changed by `change`.
fn edit(editor: &mut Editor, feature: FeatureId, change: impl FnOnce(&mut Sketch)) {
    let FeatureKind::Sketch { sketch, .. } = &editor.document().feature(feature).unwrap().kind
    else {
        panic!("a sketch");
    };
    let mut sketch = sketch.clone();
    change(&mut sketch);
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
}

/// Draws the square of side `side` about `(x, y)`: its corners, from
/// `-x -y` counter-clockwise.
fn square(side: f64, (x, y): (f64, f64)) -> impl FnOnce(&mut Sketch) -> [varde_sketch::Id; 4] {
    move |sketch| {
        let h = side / 2.0;
        let corners = [(-h, -h), (h, -h), (h, h), (-h, h)]
            .map(|(dx, dy)| sketch.add_point(DVec2::new(x + dx, y + dy)).unwrap());
        for (k, &start) in corners.iter().enumerate() {
            let end = corners[(k + 1) % 4];
            sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        }
        corners
    }
}

/// The region of sketch `feature`'s only region.
fn only_region(document: &Document, feature: FeatureId) -> RegionRef {
    let FeatureKind::Sketch { sketch, .. } = &document.feature(feature).unwrap().kind else {
        panic!("a sketch");
    };
    let profiles = sketch.profiles().unwrap();
    assert_eq!(profiles.regions.len(), 1, "one region");
    profiles.reference(0).unwrap()
}

/// The section of sketch `feature`'s only region, from `start`.
fn region(document: &Document, feature: FeatureId, start: Option<varde_sketch::Id>) -> Section {
    Section::Region {
        sketch: feature,
        region: only_region(document, feature),
        start,
    }
}

/// A ruled loft through `sections` doing `operation`.
fn loft(sections: Vec<Section>, operation: Operation) -> Loft {
    Loft {
        sections,
        mode: LoftMode::Ruled,
        closed: false,
        rails: Vec::new(),
        operation,
    }
}

fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> FeatureId {
    editor
        .apply(editor.document().add_feature(kind.into()))
        .unwrap();
    editor.document().features().last().unwrap().id
}

fn new_body() -> Operation {
    Operation::NewBody(BodyId::NEW)
}

/// The face of extrude `maker`'s body `body` as `part`, picked at `near`.
fn face(body: BodyId, maker: FeatureId, part: PartKey, near: [f64; 3]) -> FaceRef {
    FaceRef {
        body,
        key: FaceKey {
            feature: maker.get(),
            part,
            instance: 0,
        },
        near: DVec3::from(near),
    }
}

/// A block 20 × 20 about the z axis from z = 0 to 10 (body A), a square
/// of side 10 on XY (`low`) and one on A's top (`top`), both about the
/// axis: the editor, A, its extrude and the two sketches.
struct Stack {
    editor: Editor,
    block: BodyId,
    block_sketch: FeatureId,
    low: FeatureId,
    low_corners: [varde_sketch::Id; 4],
    top: FeatureId,
    top_corners: [varde_sketch::Id; 4],
}

fn stack() -> Stack {
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "10"));
    let block_maker = add_extrude(
        &mut editor,
        rectangle((-10.0, -10.0), (10.0, 10.0)),
        extent,
        new_body(),
    );
    let block_sketch = editor.document().features()[0].id;
    let block = editor.document().bodies()[0].id;
    let (low, low_corners) = add_sketch(
        &mut editor,
        Plane::Origin(OriginPlane::XY),
        square(10.0, (0.0, 0.0)),
    );
    let top_face = face(block, block_maker, PartKey::EndCap, [5.0, 5.0, 10.0]);
    let (top, top_corners) =
        add_sketch(&mut editor, Plane::Face(top_face), square(10.0, (0.0, 0.0)));
    Stack {
        editor,
        block,
        block_sketch,
        low,
        low_corners,
        top,
        top_corners,
    }
}

fn evaluated(document: &Document) -> Evaluation {
    evaluate(document, &mut Cache::default())
}

fn volume_of(evaluation: &Evaluation, body: BodyId) -> f64 {
    let made = (evaluation.bodies.iter())
        .find(|made| made.body == body)
        .unwrap_or_else(|| panic!("{body:?} has a solid: {:?}", evaluation.failed));
    made.solid.volume()
}

fn assert_near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} vs {b}");
}

/// The kernel's loft isn't built: a loft fails as too complex, making
/// no body, and the features after it still run.
#[test]
fn a_loft_fails_as_too_complex_and_the_history_goes_on() {
    let Stack {
        mut editor,
        low,
        top,
        ..
    } = stack();
    let document = editor.document().clone();
    let id = add(
        &mut editor,
        loft(
            vec![region(&document, low, None), region(&document, top, None)],
            new_body(),
        ),
    );
    let made = editor.document().bodies().last().unwrap().id;
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let after = add_extrude(
        &mut editor,
        rectangle((30.0, 0.0), (40.0, 10.0)),
        extent,
        new_body(),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.failed, [(id, TOO_COMPLEX.to_owned())]);
    assert!(evaluation.bodies.iter().all(|body| body.body != made));
    assert_eq!(evaluation.bodies.len(), 2);
    assert!(
        evaluation
            .failed
            .iter()
            .all(|failed| failed.feature != after)
    );
}

/// Two equal squares 10 apart, by the stand-in: a box of 10 × 10 × 10,
/// its faces named for the loft. The sections the other way round make
/// the same box.
#[test]
fn two_equal_squares_make_a_box() {
    with_extrudes();
    let Stack {
        mut editor,
        low,
        top,
        ..
    } = stack();
    let document = editor.document().clone();
    let up = add(
        &mut editor,
        loft(
            vec![region(&document, low, None), region(&document, top, None)],
            new_body(),
        ),
    );
    let down = add(
        &mut editor,
        loft(
            vec![region(&document, top, None), region(&document, low, None)],
            new_body(),
        ),
    );
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let bodies = editor.document().bodies();
    let (a, b) = (bodies[1].id, bodies[2].id);
    assert_near(volume_of(&evaluation, a), 1000.0);
    assert_near(volume_of(&evaluation, b), 1000.0);
    let made = evaluation
        .bodies
        .iter()
        .find(|made| made.body == a)
        .unwrap();
    assert!(
        (made.solid.mesh().faces().iter()).all(|face| face.name.feature == up.get()),
        "named for the loft"
    );
    assert_eq!(bodies[2].created_by, down);
}

/// A block from z = 0 to 10 and another moved up to z = 20 to 30, a
/// square on the first's top and one on the second's underside, lofted
/// and joined: the transition piece merges the two blocks into the
/// first, 2 × 4000 + 1000. Moving the second block 10 further, the loft
/// follows: 2 × 4000 + 2000.
#[test]
fn a_transition_piece_joins_two_bosses() {
    with_extrudes();
    let Stack {
        mut editor,
        block,
        top,
        ..
    } = stack();
    let extent = Extent::OneSide(length(editor.document(), "10"));
    let upper_maker = add_extrude(
        &mut editor,
        rectangle((-10.0, -10.0), (10.0, 10.0)),
        extent,
        new_body(),
    );
    let upper = editor.document().bodies().last().unwrap().id;
    let ask = Move::offset_ask(&editor.document().design());
    let offset = |z: &str| {
        [
            Value::new("0", &ask).unwrap(),
            Value::new("0", &ask).unwrap(),
            Value::new(z, &ask).unwrap(),
        ]
    };
    let lift = add(
        &mut editor,
        Move {
            bodies: vec![upper],
            offset: offset("20"),
            turn: None,
        },
    );
    let under = face(upper, upper_maker, PartKey::StartCap, [5.0, 5.0, 20.0]);
    let (bottom, _) = add_sketch(&mut editor, Plane::Face(under), square(10.0, (0.0, 0.0)));
    let document = editor.document().clone();
    let id = add(
        &mut editor,
        loft(
            vec![
                region(&document, top, None),
                region(&document, bottom, None),
            ],
            Operation::Join(Targets::default()),
        ),
    );
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(evaluation.touched, [(id, vec![block, upper])]);
    assert_eq!(evaluation.merged, [(upper, block)]);
    assert_near(volume_of(&evaluation, block), 9000.0);
    // The upper block moved 10 further: the loft is 20 long.
    editor
        .apply(Command::SetFeature {
            feature: lift,
            kind: Box::new(
                Move {
                    bodies: vec![upper],
                    offset: offset("30"),
                    turn: None,
                }
                .into(),
            ),
        })
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_near(volume_of(&evaluation, block), 10000.0);
}

/// Editing a section's sketch: the loft follows. One square made larger
/// alone fails (the stand-in only extrudes equal sections) and the
/// history goes on; both larger, a larger box; the region gone, "section
/// 2 not found".
#[test]
fn a_section_s_sketch_edited_and_the_loft_following() {
    with_extrudes();
    let Stack {
        mut editor,
        low,
        top,
        low_corners,
        top_corners,
        ..
    } = stack();
    let document = editor.document().clone();
    let id = add(
        &mut editor,
        loft(
            vec![region(&document, low, None), region(&document, top, None)],
            new_body(),
        ),
    );
    let made = editor.document().bodies().last().unwrap().id;
    let grow = |corners: [varde_sketch::Id; 4]| {
        move |sketch: &mut Sketch| {
            for (id, (x, y)) in
                corners
                    .into_iter()
                    .zip([(-6.0, -6.0), (6.0, -6.0), (6.0, 6.0), (-6.0, 6.0)])
            {
                sketch.point_mut(id).unwrap().at = DVec2::new(x, y);
            }
        }
    };
    edit(&mut editor, low, grow(low_corners));
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.failed, [(id, TOO_COMPLEX.to_owned())]);
    assert_eq!(evaluation.bodies.len(), 1);
    edit(&mut editor, top, grow(top_corners));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_near(volume_of(&evaluation, made), 12.0 * 12.0 * 10.0);
    // The top square's curves gone: its region isn't found.
    edit(&mut editor, top, |sketch| sketch.curves.clear());
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.failed, [(id, "section 2 not found".to_owned())]);
    // Undone: the box is back.
    editor.undo();
    let evaluation = evaluated(editor.document());
    assert_near(volume_of(&evaluation, made), 1440.0);
}

/// The kernel is handed the sections in the feature's order, each placed
/// by its sketch: a region as its outline on its sketch's frame, a point
/// in the world; a start point as the outline's segment starting there.
#[test]
fn sections_are_placed_in_order() {
    with_recording(None);
    let Stack {
        mut editor,
        low,
        top,
        top_corners,
        ..
    } = stack();
    let (apex_sketch, apex) = add_sketch(&mut editor, Plane::Origin(OriginPlane::XZ), |sketch| {
        sketch.add_point(DVec2::new(3.0, 25.0)).unwrap()
    });
    let document = editor.document().clone();
    let sections = vec![
        Section::Point {
            sketch: apex_sketch,
            point: apex,
        },
        region(&document, top, Some(top_corners[2])),
        region(&document, low, None),
    ];
    let id = add(&mut editor, loft(sections, new_body()));
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.failed, [(id, TOO_COMPLEX.to_owned())]);
    let asked = asked();
    let [(sections, closed, rails)] = asked.as_slice() else {
        panic!("asked once: {asked:?}");
    };
    assert!(!closed && rails.is_empty());
    use varde_kernel::loft::Section as Placed;
    let [
        Placed::Point(point),
        Placed::Loop {
            outline: high,
            frame: high_frame,
            start: high_start,
        },
        Placed::Loop {
            outline: low_outline,
            frame: low_frame,
            start: low_start,
        },
    ] = sections.as_slice()
    else {
        panic!("a point and two loops: {sections:?}");
    };
    // XZ's (3, 25) is the world's (3, 0, 25).
    assert_eq!(*point, DVec3::new(3.0, 0.0, 25.0));
    assert_eq!(high_frame.origin, DVec3::new(0.0, 0.0, 10.0));
    assert_eq!(low_frame.origin, DVec3::ZERO);
    assert_eq!((high.segments.len(), low_outline.segments.len()), (4, 4));
    // The top's start: its corner at (5, 5).
    let start = high_start.expect("a start");
    assert_eq!(high.segments[start].conic.p0, DVec2::new(5.0, 5.0));
    assert_eq!(*low_start, None);
}

/// Regen's own refusals, before the kernel: a start point off the
/// outline's corners, one taken out of its sketch, a point section gone,
/// a section with holes, a section's sketch not placed, two sections on
/// one plane (the last and the first too, closed).
#[test]
fn sections_regen_refuses() {
    with_recording(None);
    let Stack {
        mut editor,
        block_sketch,
        low,
        top,
        ..
    } = stack();
    let (apex_sketch, apex) = add_sketch(&mut editor, Plane::Origin(OriginPlane::XZ), |sketch| {
        sketch.add_point(DVec2::new(3.0, 25.0)).unwrap()
    });
    // A point apart in the low sketch, for a start off its corners.
    let mut stray = None;
    edit(&mut editor, low, |sketch| {
        stray = Some(sketch.add_point(DVec2::new(30.0, 30.0)).unwrap());
    });
    let stray = stray.unwrap();
    let document = editor.document().clone();
    let off_corner = add(
        &mut editor,
        loft(
            vec![
                region(&document, low, Some(stray)),
                region(&document, top, None),
            ],
            new_body(),
        ),
    );
    let to_point = add(
        &mut editor,
        loft(
            vec![
                region(&document, top, None),
                Section::Point {
                    sketch: apex_sketch,
                    point: apex,
                },
            ],
            new_body(),
        ),
    );
    let evaluation = evaluated(editor.document());
    let failed = |evaluation: &Evaluation, id: FeatureId| {
        (evaluation.failed.iter())
            .find(|failed| failed.feature == id)
            .map(|failed| failed.message.clone())
    };
    assert_eq!(
        failed(&evaluation, off_corner).as_deref(),
        Some("section 1's start point isn't one of its corners")
    );
    assert_eq!(failed(&evaluation, to_point).as_deref(), Some(TOO_COMPLEX));
    // The stray point and the apex taken out of their sketches.
    edit(&mut editor, low, |sketch| {
        sketch.points.retain(|p| p.id != stray)
    });
    edit(&mut editor, apex_sketch, |sketch| sketch.points.clear());
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failed(&evaluation, off_corner).as_deref(),
        Some("section 1's start point wasn't found")
    );
    assert_eq!(
        failed(&evaluation, to_point).as_deref(),
        Some("section 2 not found")
    );
    // A hole cut into the low square, clear of the point its region was
    // named by: found again, with its hole.
    let document = editor.document().clone();
    let holed = add(
        &mut editor,
        loft(
            vec![region(&document, low, None), region(&document, top, None)],
            new_body(),
        ),
    );
    let Section::Region { region: named, .. } = region(&document, low, None) else {
        unreachable!()
    };
    let inside = named.inside;
    edit(&mut editor, low, |sketch| {
        let at = if inside.x > 0.0 { -3.0 } else { 3.0 };
        let center = sketch.add_point(DVec2::new(at, at)).unwrap();
        let circle = Curve::Circle {
            center,
            radius: 1.0,
        };
        sketch.add_curve(circle, false).unwrap();
    });
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failed(&evaluation, holed).as_deref(),
        Some("section 1 has holes: only sections with one loop can be lofted")
    );
    // The block's square gone: the block fails, and the sketch on its
    // top isn't placed.
    edit(&mut editor, block_sketch, |sketch| sketch.curves.clear());
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failed(&evaluation, to_point).as_deref(),
        Some("section 1's sketch isn't placed")
    );
    assert!(
        asked().len() <= 1,
        "the kernel asked only for what got that far"
    );
}

/// Two sections on one plane are refused by the kernel's rule before
/// it's asked: two sketches on XY; closed, the last and the first.
#[test]
fn sections_on_one_plane_are_refused() {
    with_recording(None);
    let Stack {
        mut editor,
        low,
        top,
        ..
    } = stack();
    let (flat, _) = add_sketch(
        &mut editor,
        Plane::Origin(OriginPlane::XY),
        square(4.0, (20.0, 0.0)),
    );
    let (side, _) = add_sketch(
        &mut editor,
        Plane::Origin(OriginPlane::XZ),
        square(4.0, (0.0, 5.0)),
    );
    let document = editor.document().clone();
    let id = add(
        &mut editor,
        loft(
            vec![region(&document, low, None), region(&document, flat, None)],
            new_body(),
        ),
    );
    let closed = add(
        &mut editor,
        Loft {
            closed: true,
            ..loft(
                vec![
                    region(&document, low, None),
                    region(&document, side, None),
                    region(&document, top, None),
                    region(&document, flat, None),
                ],
                new_body(),
            )
        },
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [
            (id, "sections 1 and 2 are on one plane".to_owned()),
            (closed, "sections 4 and 1 are on one plane".to_owned()),
        ]
    );
    assert!(asked().is_empty());
}

/// A point on the plane of the section next to it is refused as that,
/// first or last: the square on XY to a point on XY, and back.
#[test]
fn a_point_on_its_neighbour_s_plane_is_refused_as_that() {
    with_recording(None);
    let Stack {
        mut editor, low, ..
    } = stack();
    let (apex, point) = add_sketch(&mut editor, Plane::Origin(OriginPlane::XY), |sketch| {
        sketch.add_point(DVec2::new(20.0, 20.0)).unwrap()
    });
    let document = editor.document().clone();
    let tip = Section::Point {
        sketch: apex,
        point,
    };
    let up = add(
        &mut editor,
        loft(vec![region(&document, low, None), tip.clone()], new_body()),
    );
    let down = add(
        &mut editor,
        loft(vec![tip, region(&document, low, None)], new_body()),
    );
    let evaluation = evaluated(editor.document());
    let words = "a point on section 1's plane: move it off the plane";
    assert_eq!(
        evaluation.failed,
        [
            (up, format!("section 2 is {words}")),
            (down, format!("section 1 is {}", words.replace('1', "2"))),
        ]
    );
    assert!(asked().is_empty());
}

/// A rail's curves ordered into one chain and placed in the world; gone,
/// "rail 1 not found"; a closed one refused.
#[test]
fn rails_are_placed() {
    with_recording(None);
    let Stack {
        mut editor,
        low,
        top,
        ..
    } = stack();
    let (rail_sketch, rail) = add_sketch(&mut editor, Plane::Origin(OriginPlane::XZ), |sketch| {
        let [a, b, c] = [(5.0, 0.0), (5.0, 4.0), (5.0, 10.0)]
            .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
        let mut curves = vec![
            sketch
                .add_curve(Curve::Line { start: b, end: c }, false)
                .unwrap(),
            sketch
                .add_curve(Curve::Line { start: a, end: b }, false)
                .unwrap(),
        ];
        curves.sort_unstable();
        curves
    });
    let document = editor.document().clone();
    let id = add(
        &mut editor,
        Loft {
            rails: vec![CurveChain {
                sketch: rail_sketch,
                curves: rail.clone(),
            }],
            ..loft(
                vec![region(&document, low, None), region(&document, top, None)],
                new_body(),
            )
        },
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.failed, [(id, TOO_COMPLEX.to_owned())]);
    let asked = asked();
    let [(_, _, rails)] = asked.as_slice() else {
        panic!("asked once");
    };
    let [placed] = rails.as_slice() else {
        panic!("one rail");
    };
    let ends: Vec<[DVec3; 2]> = placed.conics.iter().map(|c| [c.p0, c.p1]).collect();
    assert_eq!(ends.len(), 2);
    let run = ends.concat();
    // End to end, along x = 5 in XZ, from z = 0 to 10 one way or the
    // other.
    assert_eq!(run[1], run[2]);
    let mut zs = [run[0].z, run[3].z];
    zs.sort_by(f64::total_cmp);
    assert_eq!(zs, [0.0, 10.0]);
    assert!(run.iter().all(|p| p.x == 5.0 && p.y == 0.0));
    // Its curves gone: not found.
    let mut circle = None;
    edit(&mut editor, rail_sketch, |sketch| {
        sketch.curves.clear();
        let center = sketch.add_point(DVec2::new(0.0, 5.0)).unwrap();
        let curve = Curve::Circle {
            center,
            radius: 2.0,
        };
        circle = Some(sketch.add_curve(curve, false).unwrap());
    });
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.failed, [(id, "rail 1 not found".to_owned())]);
    // The circle as the rail: closed.
    let FeatureKind::Loft(made) = &editor.document().feature(id).unwrap().kind else {
        unreachable!()
    };
    let mut closed = made.clone();
    closed.rails[0].curves = vec![circle.unwrap()];
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(closed.into()),
        })
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(
            id,
            "rail 1 is closed: a rail runs from the first section to the last".to_owned()
        )]
    );
}

/// The kernel's refusals, worded for the Timeline.
#[test]
fn the_kernel_s_refusals_are_worded() {
    let Stack {
        mut editor,
        low,
        top,
        ..
    } = stack();
    let document = editor.document().clone();
    let id = add(
        &mut editor,
        loft(
            vec![region(&document, low, None), region(&document, top, None)],
            new_body(),
        ),
    );
    for (refusal, words) in [
        (
            LoftError::RailMisses {
                rail: 0,
                section: 1,
            },
            "rail 1 doesn't pass through section 2",
        ),
        (
            LoftError::Twists,
            "the loft twists: pick matching start points",
        ),
        (LoftError::IntoItself, "the loft runs into itself"),
        (
            LoftError::OnePlane { section: 1 },
            "sections 2 and 1 are on one plane",
        ),
        (
            LoftError::Failed(KernelError::TooComplex.into()),
            TOO_COMPLEX,
        ),
    ] {
        with_recording(Some(refusal));
        let evaluation = evaluated(editor.document());
        assert_eq!(evaluation.failed, [(id, words.to_owned())]);
    }
}

/// Starts that don't match twist the loft: the stand-in refuses them as
/// the kernel would.
#[test]
fn mismatched_starts_twist() {
    with_extrudes();
    let Stack {
        mut editor,
        low,
        top,
        low_corners,
        top_corners,
        ..
    } = stack();
    let document = editor.document().clone();
    let matched = add(
        &mut editor,
        loft(
            vec![
                region(&document, low, Some(low_corners[0])),
                region(&document, top, Some(top_corners[0])),
            ],
            new_body(),
        ),
    );
    let twisted = add(
        &mut editor,
        loft(
            vec![
                region(&document, low, Some(low_corners[0])),
                region(&document, top, Some(top_corners[2])),
            ],
            new_body(),
        ),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(
            twisted,
            "the loft twists: pick matching start points".to_owned()
        )]
    );
    let body = (editor.document().bodies().iter())
        .find(|body| body.created_by == matched)
        .unwrap()
        .id;
    assert_near(volume_of(&evaluation, body), 1000.0);
}

/// An unchanged loft is taken from the cache; an edit elsewhere leaves it
/// there; its section's sketch edited makes it again.
#[test]
fn a_loft_is_cached_by_its_sections() {
    with_recording(None);
    let Stack {
        mut editor,
        low,
        top,
        low_corners,
        ..
    } = stack();
    let document = editor.document().clone();
    add(
        &mut editor,
        loft(
            vec![region(&document, low, None), region(&document, top, None)],
            new_body(),
        ),
    );
    let mut cache = Cache::default();
    evaluate(editor.document(), &mut cache);
    evaluate(editor.document(), &mut cache);
    assert_eq!(asked().len(), 1);
    let extent = Extent::OneSide(length(editor.document(), "5"));
    add_extrude(
        &mut editor,
        rectangle((30.0, 0.0), (40.0, 10.0)),
        extent,
        new_body(),
    );
    evaluate(editor.document(), &mut cache);
    assert_eq!(asked().len(), 0);
    edit(&mut editor, low, |sketch| {
        sketch.point_mut(low_corners[0]).unwrap().at = DVec2::new(-6.0, -5.0);
    });
    evaluate(editor.document(), &mut cache);
    assert_eq!(asked().len(), 1);
}

/// A start is a piece's start vertex with a sketch point within the
/// resolution, by the rule the session picks starts by
/// ([`crate::loft_corners`]): on a "D" (a line, and a half circle the
/// profile splits into two quarters), the arc's far end is a corner and
/// starts at its piece's first segment, as does a point a hair off the
/// line's start; a point at the arc's middle, where the profile splits
/// it, is no corner.
#[test]
fn starts_are_the_pieces_corners() {
    with_recording(None);
    let mut editor = Editor::new(Document::default());
    let (d, [left, right, middle, near]) =
        add_sketch(&mut editor, Plane::Origin(OriginPlane::XY), |sketch| {
            let left = sketch.add_point(DVec2::new(-5.0, 0.0)).unwrap();
            let right = sketch.add_point(DVec2::new(5.0, 0.0)).unwrap();
            let center = sketch.add_point(DVec2::ZERO).unwrap();
            sketch
                .add_curve(
                    Curve::Line {
                        start: left,
                        end: right,
                    },
                    false,
                )
                .unwrap();
            let arc = Curve::Arc {
                center,
                start: right,
                end: left,
            };
            sketch.add_curve(arc, false).unwrap();
            let middle = sketch.add_point(DVec2::new(0.0, 5.0)).unwrap();
            let near = sketch.add_point(DVec2::new(5.0 + 1e-9, 0.0)).unwrap();
            [left, right, middle, near]
        });
    let (apex_sketch, apex) = add_sketch(&mut editor, Plane::Origin(OriginPlane::XZ), |sketch| {
        sketch.add_point(DVec2::new(0.0, 20.0)).unwrap()
    });
    let FeatureKind::Sketch { sketch, .. } = &editor.document().feature(d).unwrap().kind else {
        unreachable!()
    };
    let profiles = sketch.profiles().unwrap();
    let resolution = editor.document().tolerance().resolution();
    let outer = &profiles.regions[0].outer;
    let corners: Vec<_> = crate::loft_corners(sketch, &profiles, outer, resolution)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(corners.len(), 2, "the line's ends: {corners:?}");
    assert!(corners.contains(&left) && corners.contains(&right));
    for (point, corner) in [(left, true), (right, true), (near, true), (middle, false)] {
        let found = crate::loft_corner(sketch, &profiles, outer, point, resolution);
        assert_eq!(found.is_some(), corner, "point {point:?}");
    }
    let document = editor.document().clone();
    let apex = Section::Point {
        sketch: apex_sketch,
        point: apex,
    };
    let lofts = [left, near, middle].map(|start| {
        add(
            &mut editor,
            loft(
                vec![region(&document, d, Some(start)), apex.clone()],
                new_body(),
            ),
        )
    });
    let evaluation = evaluated(editor.document());
    let failed = |id: FeatureId| {
        (evaluation.failed.iter())
            .find(|failed| failed.feature == id)
            .map(|failed| failed.message.clone())
    };
    assert_eq!(failed(lofts[0]).as_deref(), Some(TOO_COMPLEX));
    assert_eq!(failed(lofts[1]).as_deref(), Some(TOO_COMPLEX));
    assert_eq!(
        failed(lofts[2]).as_deref(),
        Some("section 1's start point isn't one of its corners")
    );
    let asked = asked();
    assert_eq!(asked.len(), 2, "the two with corners reach the kernel");
    for ((sections, ..), want) in asked
        .iter()
        .zip([DVec2::new(-5.0, 0.0), DVec2::new(5.0, 0.0)])
    {
        let varde_kernel::loft::Section::Loop { outline, start, .. } = &sections[0] else {
            panic!("a loop first");
        };
        assert_eq!(outline.segments.len(), 3, "a line and two quarters");
        let start = start.expect("a start");
        assert_eq!(outline.segments[start].conic.p0, want);
    }
}

mod fuzz;
