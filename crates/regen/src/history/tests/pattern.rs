//! Patterns in the history: rows and rings of pins against their
//! analytic volumes, copies touching end to end united, spokes
//! overlapping at the hub, a 10 × 10 grid of pins cut from a plate in
//! one difference, copies' faces named apart (copies of copies too, 64
//! by 64), copies touching along a line or on the original, a ring
//! about a far axis, refusals (out of range, a face of a copy that isn't
//! there), the patch bound, and what's cached.

use std::time::Instant;

use glam::DVec3;
use varde_document::{
    Axis3, AxisRef, BodyOp, Combine, EdgeRef, FaceRef, MAX_PATTERN_COUNT, Pattern, PatternKind,
};
use varde_kernel::measure::{EdgeShape, edge_shape};
use varde_kernel::mesh::{FaceKey, Form};

use super::combine::fuzz::truncated;
use super::*;
use crate::history::pattern::copies_fit;
use crate::picking::region_form;

fn count(document: &Document, text: &str) -> Value {
    Value::new(text, &Pattern::count_ask(&document.design())).unwrap()
}

/// A linear pattern of `bodies`, `n` copies `step` apart along `along`.
fn linear(document: &Document, bodies: &[BodyId], along: AxisRef, n: &str, step: &str) -> Pattern {
    Pattern {
        bodies: bodies.to_vec(),
        kind: PatternKind::Linear {
            along,
            count: count(document, n),
            spacing: Value::new(step, &Pattern::spacing_ask(&document.design())).unwrap(),
        },
        copies: Default::default(),
    }
}

/// A circular pattern of `bodies`, `n` copies over `span` about `about`.
fn circular(
    document: &Document,
    bodies: &[BodyId],
    about: AxisRef,
    n: &str,
    span: &str,
) -> Pattern {
    Pattern {
        bodies: bodies.to_vec(),
        kind: PatternKind::Circular {
            about,
            count: count(document, n),
            angle: Value::new(span, &Pattern::angle_ask(&document.design())).unwrap(),
        },
        copies: Default::default(),
    }
}

/// Adds `kind` as one edit: its id.
fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> FeatureId {
    editor
        .apply(editor.document().add_feature(kind.into()))
        .unwrap();
    editor.document().features().last().unwrap().id
}

/// Sets feature `feature` to `kind`.
fn set(editor: &mut Editor, feature: FeatureId, kind: impl Into<FeatureKind>) {
    editor
        .apply(Command::SetFeature {
            feature,
            kind: Box::new(kind.into()),
        })
        .unwrap();
}

/// A pin of radius 2 about `(x, y)`, 10 up from XY: its body.
fn pin(editor: &mut Editor, x: f64, y: f64) -> BodyId {
    add_body(editor, disc((x, y), 2.0), "10")
}

const PIN: f64 = PI * 4.0 * 10.0;

const X: AxisRef = AxisRef::Origin(Axis3::X);
const Z: AxisRef = AxisRef::Origin(Axis3::Z);

fn near(a: f64, b: f64, relative: f64) -> bool {
    (a - b).abs() <= relative * b.abs().max(1.0)
}

/// The volume and centre of mass of `solid`.
fn mass(solid: &Solid) -> (f64, DVec3) {
    let moments = solid.moments(&varde_kernel::Budget::DEFAULT).unwrap();
    (moments.volume, moments.centre.unwrap())
}

/// The keys of the cylinders of `solid`, sorted.
fn cylinders(solid: &Solid) -> Vec<FaceKey> {
    let topology = solid.topology();
    let mut keys: Vec<FaceKey> = (topology.regions().iter())
        .filter(|region| matches!(region_form(solid, region), Form::Cylinder { .. }))
        .map(|region| region.key)
        .collect();
    keys.sort_unstable();
    keys
}

/// How many shells `solid` is: its triangles' groups joined across
/// their edges.
fn shells(solid: &Solid) -> usize {
    let tris = solid.mesh().tris();
    let mut group: Vec<usize> = (0..tris.len()).collect();
    fn root(group: &mut [usize], mut t: usize) -> usize {
        while group[t] != t {
            group[t] = group[group[t]];
            t = group[t];
        }
        t
    }
    for (t, tri) in tris.iter().enumerate() {
        for halfedge in tri.halfedges {
            let (a, b) = (
                root(&mut group, t),
                root(&mut group, halfedge.pair as usize / 3),
            );
            group[a] = b;
        }
    }
    (0..tris.len())
        .filter(|&t| root(&mut group, t) == t)
        .count()
}

fn failure(evaluation: &Evaluation, feature: FeatureId) -> Option<&str> {
    (evaluation.failed.iter())
        .find(|f| f.feature == feature)
        .map(|f| f.message.as_str())
}

#[test]
fn a_row_of_pins_holds_each_copy() {
    let mut editor = Editor::new(Document::default());
    let body = pin(&mut editor, 0.0, 0.0);
    let maker = editor.document().features()[1].id;
    let pattern = linear(editor.document(), &[body], X, "5", "10");
    let id = add(&mut editor, pattern);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = solid_of(&evaluation, body);
    let (volume, centre) = mass(solid);
    assert!(near(volume, 5.0 * PIN, 1e-9), "{volume}");
    assert!(
        centre.abs_diff_eq(DVec3::new(20.0, 0.0, 5.0), 1e-9),
        "{centre}"
    );
    let bounds = solid.bounds3().unwrap();
    assert!(bounds.min.abs_diff_eq(DVec3::new(-2.0, -2.0, 0.0), 1e-12));
    assert!(bounds.max.abs_diff_eq(DVec3::new(42.0, 2.0, 10.0), 1e-12));
    // The original's wall keeps its name; copy k's is named as copy k of
    // the pattern.
    let wall = cylinders(&evaluated(&truncated(editor.document(), 2)).bodies[0].solid)[0];
    assert_eq!(wall.feature, maker.get());
    let mut expected: Vec<FaceKey> = std::iter::once(wall)
        .chain((1..5).map(|k| wall.copy(id.get(), k)))
        .collect();
    expected.sort_unstable();
    assert_eq!(cylinders(solid), expected);
    // The axis is noted for the app to draw.
    assert_eq!(evaluation.references, [(id, [DVec3::ZERO, DVec3::X])]);

    // A negative spacing runs the other way.
    let kind = linear(editor.document(), &[body], X, "5", "-10");
    set(&mut editor, id, kind);
    let evaluation = evaluated(editor.document());
    let bounds = solid_of(&evaluation, body).bounds3().unwrap();
    assert!(bounds.min.abs_diff_eq(DVec3::new(-42.0, -2.0, 0.0), 1e-12));
    assert!(bounds.max.abs_diff_eq(DVec3::new(2.0, 2.0, 10.0), 1e-12));
}

/// Copies end to end along the pin's own axis, its round face, are
/// united into one longer pin.
#[test]
fn copies_touching_end_to_end_are_united() {
    let mut editor = Editor::new(Document::default());
    let body = pin(&mut editor, 3.0, 4.0);
    let wall = cylinders(solid_of(&evaluated(editor.document()), body))[0];
    let along = AxisRef::Face(FaceRef {
        body,
        key: wall,
        near: DVec3::new(5.0, 4.0, 5.0),
    });
    let pattern = linear(editor.document(), &[body], along, "3", "10");
    let id = add(&mut editor, pattern);
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, id), None);
    let solid = solid_of(&evaluation, body);
    let (volume, centre) = mass(solid);
    assert!(near(volume, 3.0 * PIN, 1e-9), "{volume}");
    assert!(
        centre.abs_diff_eq(DVec3::new(3.0, 4.0, 15.0), 1e-9),
        "{centre}"
    );
    let bounds = solid.bounds3().unwrap();
    assert!(bounds.max.abs_diff_eq(DVec3::new(5.0, 6.0, 30.0), 1e-9));
    // One shell: the copies are joined, not side by side.
    assert_eq!(shells(solid), 1);
}

#[test]
fn a_ring_of_pins_whole_turn_and_part_of_one() {
    let mut editor = Editor::new(Document::default());
    let body = pin(&mut editor, 20.0, 0.0);
    let pattern = circular(editor.document(), &[body], Z, "4", "360");
    let id = add(&mut editor, pattern);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = solid_of(&evaluation, body);
    let (volume, centre) = mass(solid);
    assert!(near(volume, 4.0 * PIN, 1e-9), "{volume}");
    assert!(
        centre.abs_diff_eq(DVec3::new(0.0, 0.0, 5.0), 1e-9),
        "{centre}"
    );
    // Quarter turns about Z are exact: the ring's box to the bit as the
    // pin's turned.
    let pin_box = solid_of(&evaluated(&truncated(editor.document(), 2)), body)
        .bounds3()
        .unwrap();
    let bounds = solid.bounds3().unwrap();
    assert_eq!(bounds.max.x, pin_box.max.x);
    assert_eq!(bounds.min.x, -pin_box.max.x);
    assert_eq!(bounds.max.y, pin_box.max.x);
    assert_eq!(bounds.min.y, -pin_box.max.x);

    // Three over 90°: a copy at each end, 45° apart.
    let kind = circular(editor.document(), &[body], Z, "3", "90");
    set(&mut editor, id, kind);
    let evaluation = evaluated(editor.document());
    let (volume, centre) = mass(solid_of(&evaluation, body));
    assert!(near(volume, 3.0 * PIN, 1e-9), "{volume}");
    let half = 20.0 * std::f64::consts::FRAC_1_SQRT_2;
    let expected = DVec3::new(20.0 + half, half + 20.0, 15.0) / 3.0;
    assert!(centre.abs_diff_eq(expected, 1e-9), "{centre} vs {expected}");
    // Right-handed about Z: the last copy is on +Y.
    let bounds = solid_of(&evaluation, body).bounds3().unwrap();
    assert!(near(bounds.max.y, 22.0, 1e-12), "{bounds:?}");
}

/// Three bars through the hub, a whole turn of them, overlap there: one
/// solid of the area `3·60·4 − 3·(4²/sin 60°) + hexagon` (inclusion and
/// exclusion: each two cross in a rhombus, all three in a regular
/// hexagon of inradius 2).
#[test]
fn spokes_overlapping_at_the_hub_are_united() {
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, -30.0, -2.0, 30.0, 2.0, "5");
    let pattern = circular(editor.document(), &[body], Z, "3", "360");
    let id = add(&mut editor, pattern);
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, id), None);
    let solid = solid_of(&evaluation, body);
    let root3 = 3f64.sqrt();
    let area = 720.0 - 3.0 * 32.0 / root3 + 8.0 * root3;
    let (volume, centre) = mass(solid);
    assert!(near(volume, area * 5.0, 1e-9), "{volume} vs {}", area * 5.0);
    assert!(
        centre.abs_diff_eq(DVec3::new(0.0, 0.0, 2.5), 1e-9),
        "{centre}"
    );
    assert_eq!(shells(solid), 1);
}

/// Adds the block from `(x0, y0)` to `(x1, y1)`, `height` up from XY: its
/// body.
fn block(editor: &mut Editor, x0: f64, y0: f64, x1: f64, y1: f64, height: &str) -> BodyId {
    add_body(editor, rectangle((x0, y0), (x1, y1)), height)
}

/// A pin patterned `n` along X, that row `n` along Y (copies of
/// copies), cut from a plate `10·n` square in one combine: the grid of
/// pins (checked whole: volume, centre, a shell each) and whether the
/// difference worked, `None` if it ran out of work (as one difference of
/// 64 or more pins from the plate's two-triangle cap does), or the
/// failing message otherwise. Prints the time each step takes.
fn pin_grid(n: usize) -> Option<Result<(), String>> {
    let side = 10.0 * n as f64;
    let mut editor = Editor::new(Document::default());
    let plate = block(&mut editor, 0.0, 0.0, side, side, "5");
    let extent = two_sides(editor.document(), "6", "1");
    add_extrude(
        &mut editor,
        disc((5.0, 5.0), 2.0),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let pins = editor.document().bodies().last().unwrap().id;
    let n_text = n.to_string();
    let row = linear(editor.document(), &[pins], X, &n_text, "10");
    let row = add(&mut editor, row);
    let y = AxisRef::Origin(Axis3::Y);
    let grid = linear(editor.document(), &[pins], y, &n_text, "10");
    let grid = add(&mut editor, grid);
    let combine = add(
        &mut editor,
        Combine {
            target: plate,
            tools: vec![pins],
            op: BodyOp::Subtract,
            keep_tools: false,
        },
    );
    let mut cache = Cache::default();
    let mut timed = |end: usize| {
        let start = Instant::now();
        cache.begin();
        let evaluation = evaluate(&truncated(editor.document(), end), &mut cache);
        (evaluation, start.elapsed())
    };
    let (_, pins_made) = timed(4);
    let (_, row_made) = timed(5);
    let (patterned, grid_made) = timed(6);
    let (evaluation, cut) = timed(7);
    let count = (n * n) as f64;
    let middle = DVec3::new(side / 2.0, side / 2.0, 2.5);
    for feature in [row, grid] {
        assert_eq!(failure(&evaluation, feature), None);
    }
    let grid_solid = solid_of(&patterned, pins);
    let (volume, centre) = mass(grid_solid);
    assert!(near(volume, count * PI * 4.0 * 7.0, 1e-9), "{volume}");
    assert!(centre.abs_diff_eq(middle, 1e-9), "{centre}");
    assert_eq!(shells(grid_solid), n * n);
    let outcome = match failure(&evaluation, combine) {
        None => {
            // A hole through the plate for each pin.
            let solid = solid_of(&evaluation, plate);
            let (volume, centre) = mass(solid);
            let expected = side * side * 5.0 - count * PI * 4.0 * 5.0;
            assert!(near(volume, expected, 1e-9), "{volume} vs {expected}");
            assert!(centre.abs_diff_eq(middle, 1e-9), "{centre}");
            assert_eq!(cylinders(solid).len(), n * n);
            assert_eq!(evaluation.merged, [(pins, plate)]);
            Some(Ok(()))
        }
        Some(message) if message.contains("too complex to work out") => {
            // Said as too many pieces at once, not as faces nearly flush.
            let pieces = n * n;
            assert_eq!(
                message,
                format!(
                    "cutting Body 2 from Body 1 is too complex to work out at once: Body 2 is \
                     {pieces} separate pieces; use fewer, or split them over more than one \
                     combine"
                )
            );
            // Failing, it changed nothing.
            assert_eq!(evaluation.bodies.len(), 2);
            None
        }
        Some(message) => Some(Err(message.to_owned())),
    };
    eprintln!(
        "{n} x {n} pins: pins {pins_made:?}, row {row_made:?}, grid {grid_made:?}, \
         difference {cut:?}: {outcome:?}"
    );
    outcome
}

/// The hole pattern patterns are for: a grid of pins cut from a plate
/// in one difference. A 4 × 4 grid is cut; the 10 × 10 one, as measured
/// when patterns came (about 2 s in a release build, 3.3 s in a test
/// one), runs out of work in the difference: the whole-body budget,
/// which a 7 × 7 grid already takes 2 to 3 million units of. Either way
/// never a wrong solid.
#[test]
fn a_pin_grid_is_cut_from_a_plate_in_one_difference() {
    assert_eq!(pin_grid(4), Some(Ok(())));
    let ten = pin_grid(10);
    assert!(matches!(ten, None | Some(Ok(()))), "{ten:?}");
}
/// A copy's face is named as that copy: a later pattern turns about the
/// third pin's wall, still found as the row's count changes (copy `k`
/// keeps its name), and one naming a copy the row doesn't have fails.
#[test]
fn a_copy_s_face_is_found_by_its_name() {
    let mut editor = Editor::new(Document::default());
    let body = pin(&mut editor, 0.0, 0.0);
    let other = pin(&mut editor, 0.0, 50.0);
    let wall = cylinders(solid_of(&evaluated(editor.document()), body))[0];
    let row = linear(editor.document(), &[body], X, "3", "10");
    let row = add(&mut editor, row);
    let about = |k: u64, x: f64| {
        AxisRef::Face(FaceRef {
            body,
            key: wall.copy(row.get(), k),
            near: DVec3::new(x + 2.0, 0.0, 5.0),
        })
    };
    let ring = circular(editor.document(), &[other], about(2, 20.0), "2", "360");
    let ring = add(&mut editor, ring);
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, ring), None);
    let [(_, [point, direction])] = evaluation.references[1..] else {
        panic!("{:?}", evaluation.references);
    };
    assert!(
        point.abs_diff_eq(DVec3::new(20.0, 0.0, 5.0), 1e-9),
        "{point}"
    );
    assert!(direction.normalize().abs().abs_diff_eq(DVec3::Z, 1e-12));
    // The second pin turned half a turn about (20, 0): its copy at
    // (40, -50).
    let bounds = solid_of(&evaluation, other).bounds3().unwrap();
    assert!(bounds.min.abs_diff_eq(DVec3::new(-2.0, -52.0, 0.0), 1e-9));
    assert!(bounds.max.abs_diff_eq(DVec3::new(42.0, 52.0, 10.0), 1e-9));

    // More copies in the row: copy 2 keeps its name, and the ring finds
    // it as before; copy 3 is there now.
    let kind = linear(editor.document(), &[body], X, "5", "10");
    set(&mut editor, row, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, ring), None);
    assert!(near(evaluation.bodies[0].solid.volume(), 5.0 * PIN, 1e-9));
    let again = solid_of(&evaluation, other).bounds3().unwrap();
    assert_eq!(again, bounds);
    let kind = circular(editor.document(), &[other], about(3, 30.0), "2", "360");
    set(&mut editor, ring, kind.clone());
    assert_eq!(failure(&evaluated(editor.document()), ring), None);
    // Fewer: copy 3 is gone.
    let kind = linear(editor.document(), &[body], X, "3", "10");
    set(&mut editor, row, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, ring),
        Some("its axis face wasn't found")
    );
    assert!(near(evaluation.bodies[1].solid.volume(), PIN, 1e-12));
}

#[test]
fn copies_out_of_range_are_refused_before_copying() {
    let mut editor = Editor::new(Document::default());
    let body = pin(&mut editor, 0.0, 0.0);
    let pattern = linear(editor.document(), &[body], X, "3", "500000");
    let id = add(&mut editor, pattern);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id),
        Some(
            "patterning Body 1 takes it out of range: every part must stay within 1000000 mm \
             of the origin"
        )
    );
    assert!(near(evaluation.bodies[0].solid.volume(), PIN, 1e-12));
    // Within the limit, the last copy's wall touching it.
    let kind = linear(editor.document(), &[body], X, "3", "499998.5");
    set(&mut editor, id, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, id), None);
    let bounds = evaluation.bodies[0].solid.bounds3().unwrap();
    assert_eq!(bounds.max.x, 999999.0);
}

/// `count × patches` is checked, overflow and all, before anything is
/// copied.
#[test]
fn copies_are_bounded_by_the_patches_they_make() {
    let max = varde_kernel::MAX_PATCHES;
    assert!(copies_fit(MAX_PATTERN_COUNT, max / 1024));
    assert!(!copies_fit(MAX_PATTERN_COUNT, max / 1024 + 1));
    assert!(copies_fit(2, max / 2));
    assert!(!copies_fit(3, max / 2));
    assert!(!copies_fit(u32::MAX, usize::MAX));
    assert!(!copies_fit(2, usize::MAX / 2 + 1));
    assert!(copies_fit(2, 0));
}

/// The same pattern (a spacing typed another way) finds every body in
/// the cache; another spacing copies them again.
#[test]
fn an_unchanged_pattern_is_found_in_the_cache() {
    let mut editor = Editor::new(Document::default());
    let body = pin(&mut editor, 0.0, 0.0);
    let pattern = linear(editor.document(), &[body], X, "4", "10");
    let id = add(&mut editor, pattern);
    let mut cache = Cache::default();
    cache.begin();
    let first = evaluate(editor.document(), &mut cache);
    let mut again = |editor: &mut Editor, step: &str| {
        set(editor, id, linear(editor.document(), &[body], X, "4", step));
        cache.begin();
        evaluate(editor.document(), &mut cache)
    };
    let same = again(&mut editor, "5 + 5");
    assert!(Arc::ptr_eq(&first.bodies[0].solid, &same.bodies[0].solid));
    let other = again(&mut editor, "11");
    assert!(!Arc::ptr_eq(&first.bodies[0].solid, &other.bodies[0].solid));
    let (volume, _) = mass(&other.bodies[0].solid);
    assert!(near(volume, 4.0 * PIN, 1e-9));
}

/// A pattern of a pattern at the copies `Naming` tells apart, 64 by 64:
/// 4096 unit blocks side by side (volume and centre), and a later
/// pattern along an edge of the last copy of the last copy finds it.
#[test]
fn a_pattern_of_a_pattern_names_its_last_copy() {
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 1.0, 1.0, "1");
    let other = block(&mut editor, -10.0, -10.0, -9.0, -9.0, "1");
    let solid = solid_of(&evaluated(editor.document()), body).clone();
    let topology = solid.topology();
    // The block's top edge along X at y = 0.
    let faces = (topology.chains().iter())
        .find_map(|chain| match edge_shape(&solid, chain) {
            EdgeShape::Line { from, to }
                if from.z == 1.0 && to.z == 1.0 && from.y == 0.0 && to.y == 0.0 =>
            {
                Some(chain.regions.map(|r| topology.regions()[r as usize].key))
            }
            _ => None,
        })
        .expect("the top edge");
    let row = linear(editor.document(), &[body], X, "64", "2");
    let row = add(&mut editor, row);
    let y = AxisRef::Origin(Axis3::Y);
    let grid = linear(editor.document(), &[body], y, "64", "2");
    let grid = add(&mut editor, grid);
    let last = faces.map(|key| key.copy(row.get(), 63).copy(grid.get(), 63));
    let along = AxisRef::Edge(EdgeRef {
        body,
        faces: [last[0].min(last[1]), last[0].max(last[1])],
        near: DVec3::new(126.5, 126.0, 1.0),
    });
    let later = linear(editor.document(), &[other], along, "2", "20");
    let later = add(&mut editor, later);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let (volume, centre) = mass(solid_of(&evaluation, body));
    assert!(near(volume, 4096.0, 1e-9), "{volume}");
    assert!(
        centre.abs_diff_eq(DVec3::new(63.5, 63.5, 0.5), 1e-9),
        "{centre}"
    );
    let (_, [point, direction]) = *(evaluation.references.iter())
        .find(|(id, _)| *id == later)
        .unwrap();
    assert!(
        (point.y - 126.0).abs() < 1e-9 && (point.z - 1.0).abs() < 1e-9,
        "{point}"
    );
    assert!(direction.normalize().abs().abs_diff_eq(DVec3::X, 1e-12));
    let (volume, _) = mass(solid_of(&evaluation, other));
    assert!(near(volume, 2.0, 1e-12), "{volume}");
}

/// Copies touching along a line, discs side by side (radius 5, 10
/// apart), can't be one clean solid: refused as the kernel finds it
/// (the union would touch itself along an edge), changing nothing. A
/// hair closer they overlap and are united; a hair farther they're side
/// by side. Each against its analytic volume.
#[test]
fn copies_touching_along_a_line_are_refused() {
    let lens = |d: f64| {
        // The area two discs of radius 5 share, centres `d` apart.
        let r: f64 = 5.0;
        2.0 * r * r * (d / (2.0 * r)).acos() - d / 2.0 * (4.0 * r * r - d * d).sqrt()
    };
    for (step, expected) in [
        ("10", None),
        ("9.5", Some(4.0 * 25.0 * PI - 3.0 * lens(9.5))),
        ("10.5", Some(4.0 * 25.0 * PI)),
    ] {
        let mut editor = Editor::new(Document::default());
        let body = add_body(&mut editor, disc((0.0, 0.0), 5.0), "10");
        let pattern = linear(editor.document(), &[body], X, "4", step);
        let id = add(&mut editor, pattern);
        let evaluation = evaluated(editor.document());
        let (volume, _) = mass(solid_of(&evaluation, body));
        match expected {
            None => {
                assert_eq!(
                    failure(&evaluation, id),
                    Some(
                        "joining Body 1 to its copies leaves no clean solid: the result would \
                         touch itself along an edge or at a point, or come too close to \
                         itself; move it to overlap more or to clear it"
                    )
                );
                assert!(near(volume, 250.0 * PI, 1e-12), "{volume}");
            }
            Some(area) => {
                assert_eq!(failure(&evaluation, id), None, "{step}");
                assert!(near(volume, area * 10.0, 1e-6), "{step}: {volume}");
            }
        }
    }
}

/// Copies so close they're the original to the bit (a spacing of
/// 1e-20, a turn of 1e-300°) come out as the original; a hair apart
/// (1e-9) they're refused. Never a wrong solid.
#[test]
fn copies_on_the_original_are_it_or_refused() {
    for (n, step, circular_one) in [
        ("3", "1e-20", false),
        ("3", "1e-300", true),
        ("2", "1e-9", false),
    ] {
        let mut editor = Editor::new(Document::default());
        let body = pin(&mut editor, 20.0, 0.0);
        let pattern = if circular_one {
            circular(editor.document(), &[body], Z, n, step)
        } else {
            linear(editor.document(), &[body], X, n, step)
        };
        let id = add(&mut editor, pattern);
        let evaluation = evaluated(editor.document());
        let (volume, centre) = mass(solid_of(&evaluation, body));
        assert!(near(volume, PIN, 1e-9), "{step}: {volume}");
        assert!(centre.abs_diff_eq(DVec3::new(20.0, 0.0, 5.0), 1e-9));
        if step == "1e-9" {
            assert!(failure(&evaluation, id).is_some());
        } else {
            assert_eq!(failure(&evaluation, id), None, "{step}");
        }
    }
}

/// A circular pattern about the wall of a disc near the coordinate
/// limit: a block turned a thousandth of a degree about it lands where
/// the turn takes it (17 mm round), a whole turn is refused before
/// anything is copied, and the disc patterned about its own wall is
/// itself.
#[test]
fn a_ring_about_a_far_axis() {
    let x = 999_990.0;
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let far = add_body(&mut editor, disc((x, 0.0), 5.0), "10");
    let wall = cylinders(solid_of(&evaluated(editor.document()), far))[0];
    let about = AxisRef::Face(FaceRef {
        body: far,
        key: wall,
        near: DVec3::new(x + 5.0, 0.0, 5.0),
    });
    let pattern = circular(editor.document(), &[body], about, "2", "0.001");
    let id = add(&mut editor, pattern);
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, id), None);
    let (volume, centre) = mass(solid_of(&evaluation, body));
    assert!(near(volume, 2000.0, 1e-9), "{volume}");
    let turn = glam::DQuat::from_rotation_z(0.001f64.to_radians());
    let axis = DVec3::new(x, 0.0, 5.0);
    let copy = turn * (DVec3::new(5.0, 5.0, 5.0) - axis) + axis;
    let expected = (DVec3::new(5.0, 5.0, 5.0) + copy) / 2.0;
    assert!(centre.abs_diff_eq(expected, 1e-6), "{centre} vs {expected}");

    let whole = circular(editor.document(), &[body], about, "4", "360");
    set(&mut editor, id, whole);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id),
        Some(
            "patterning Body 1 takes it out of range: every part must stay within 1000000 mm \
             of the origin"
        )
    );
    let own = circular(editor.document(), &[far], about, "4", "360");
    set(&mut editor, id, own);
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, id), None);
    let (volume, centre) = mass(solid_of(&evaluation, far));
    assert!(near(volume, 250.0 * PI, 1e-9), "{volume}");
    assert!(
        centre.abs_diff_eq(DVec3::new(x, 0.0, 5.0), 1e-6),
        "{centre}"
    );
}

mod separate;
