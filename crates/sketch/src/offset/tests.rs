#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

use glam::DVec2;

use super::*;
use crate::testing::{
    self, DESIGN, arc, circle, constrain, dimension, line, near, point, propose_it, value,
};
use crate::{CurveEntry, DimensionEntry, Rejected, SketchEdit, SketchError, analyse, arc_through};

fn offset(chain: Vec<Id>, distance: &str, side: Side) -> SketchEdit {
    SketchEdit::Offset {
        chain,
        distance: value(distance, &Measure::Offset(Id::ORIGIN, Id::ORIGIN)),
        side,
    }
}

/// A closed polygon through `corners`, counter-clockwise, and its lines.
fn polygon(corners: &[(f64, f64)]) -> (Sketch, Vec<Id>) {
    let mut sketch = Sketch::default();
    let points = testing::points(&mut sketch, corners);
    let lines = (0..points.len())
        .map(|i| line(&mut sketch, points[i], points[(i + 1) % points.len()]))
        .collect();
    (sketch, lines)
}

/// A rectangle 10 by 6 from the origin.
fn rectangle() -> (Sketch, Vec<Id>) {
    polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 6.0), (0.0, 6.0)])
}

/// A slot: two lines 10 long, 4 apart, and a half circle at each end,
/// counter-clockwise from the bottom line: its curves and the arcs'
/// centres.
fn slot() -> (Sketch, [Id; 4], [Id; 2]) {
    let mut sketch = Sketch::default();
    let corners =
        [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (0.0, 4.0)].map(|(x, y)| point(&mut sketch, x, y));
    let centers = [(10.0, 2.0), (0.0, 2.0)].map(|(x, y)| point(&mut sketch, x, y));
    let bottom = line(&mut sketch, corners[0], corners[1]);
    let right = arc(&mut sketch, centers[0], corners[1], corners[2]);
    let top = line(&mut sketch, corners[2], corners[3]);
    let left = arc(&mut sketch, centers[1], corners[3], corners[0]);
    (sketch, [bottom, right, top, left], centers)
}

/// The curves `sketch` has that `before` hadn't.
fn new_curves<'a>(sketch: &'a Sketch, before: &Sketch) -> Vec<&'a CurveEntry> {
    let ids = sketch.curves.iter();
    ids.filter(|entry| before.curve(entry.id).is_none())
        .collect()
}

/// The ends of the new lines, each as its start and end.
fn new_lines(sketch: &Sketch, before: &Sketch) -> Vec<(DVec2, DVec2)> {
    new_curves(sketch, before)
        .iter()
        .filter_map(|entry| sketch.line(entry.id))
        .collect()
}

/// A line's ends, as coordinates.
type Ends = ((f64, f64), (f64, f64));

/// Whether `lines` are those from `expected`, each either way round, in
/// any order.
fn same_lines(lines: &[(DVec2, DVec2)], expected: &[Ends]) -> bool {
    lines.len() == expected.len()
        && expected.iter().all(|&((ax, ay), (bx, by))| {
            let (a, b) = (DVec2::new(ax, ay), DVec2::new(bx, by));
            lines
                .iter()
                .any(|&(p, q)| (near(p, a) && near(q, b)) || (near(p, b) && near(q, a)))
        })
}

fn kinds(sketch: &Sketch, before: &Sketch) -> Vec<&'static str> {
    let mut kinds: Vec<_> = sketch
        .constraints
        .iter()
        .filter(|entry| before.constraint(entry.id).is_none())
        .map(|entry| match entry.constraint {
            Constraint::Parallel(..) => "parallel",
            Constraint::EqualOffset { .. } => "equal offset",
            Constraint::Tangent { .. } => "tangent",
            _ => "other",
        })
        .collect();
    kinds.sort_unstable();
    kinds
}

/// The offset's dimension.
fn dimension_of(sketch: &Sketch) -> &DimensionEntry {
    sketch
        .dimensions
        .iter()
        .find(|entry| matches!(entry.dimension.measure, Measure::Offset(..)))
        .unwrap()
}

#[test]
fn a_rectangle_offsets_outwards_with_sharp_corners() {
    let (sketch, lines) = rectangle();
    // The bottom runs right, so outside is its right.
    let edit = offset(sketch.chain_of(lines[0]), "2", Side::Negative);
    let accepted = propose_it(&sketch, &edit).unwrap();
    let grown = &accepted.sketch;
    assert!(same_lines(
        &new_lines(grown, &sketch),
        &[
            ((-2.0, -2.0), (12.0, -2.0)),
            ((12.0, -2.0), (12.0, 8.0)),
            ((12.0, 8.0), (-2.0, 8.0)),
            ((-2.0, 8.0), (-2.0, -2.0)),
        ]
    ));
    // Four new points, shared by the lines.
    assert_eq!(grown.points.len(), 8);
    assert_eq!(
        kinds(grown, &sketch),
        [
            "equal offset",
            "equal offset",
            "equal offset",
            "parallel",
            "parallel",
            "parallel",
            "parallel"
        ]
    );
    let dimension = &dimension_of(grown).dimension;
    assert_eq!(dimension.value.value, 2.0);
    assert!(dimension.driving);
    assert_eq!(grown.measure(&dimension.measure, dimension.side), Some(2.0));
    // The copy adds no freedom: the rectangle's eight are all there are.
    assert_eq!(accepted.analysis.freedom, analyse(&sketch).freedom);
    assert_eq!(accepted.analysis.freedom, 8);
    // Each line copied the way it runs.
    let copies = new_curves(grown, &sketch);
    let (start, end) = grown.line(copies[0].id).unwrap();
    assert!(near(start, DVec2::new(-2.0, -2.0)) && near(end, DVec2::new(12.0, -2.0)));
}

#[test]
fn a_rectangle_offsets_inwards_until_nothing_is_left() {
    let (sketch, lines) = rectangle();
    let edit = offset(sketch.chain_of(lines[2]), "2", Side::Positive);
    let accepted = propose_it(&sketch, &edit).unwrap();
    assert!(same_lines(
        &new_lines(&accepted.sketch, &sketch),
        &[
            ((2.0, 2.0), (8.0, 2.0)),
            ((8.0, 2.0), (8.0, 4.0)),
            ((8.0, 4.0), (2.0, 4.0)),
            ((2.0, 4.0), (2.0, 2.0)),
        ]
    ));
    assert_eq!(accepted.analysis.freedom, 8);
    // Half its height and past it, nothing's left.
    for distance in ["3", "3.5", "20"] {
        let edit = offset(lines.clone(), distance, Side::Positive);
        assert_eq!(
            edit.apply(&sketch, &DESIGN),
            Err(EditError::NothingLeft),
            "{distance}"
        );
    }
    assert_eq!(
        sketch.offset_preview(&lines, 3.0, Side::Positive),
        Err(EditError::NothingLeft)
    );
}

#[test]
fn a_slot_offsets_as_lines_and_arcs_about_the_same_centres() {
    let (sketch, curves, centers) = slot();
    for (side, radius) in [(Side::Negative, 3.0), (Side::Positive, 1.0)] {
        let edit = offset(sketch.chain_of(curves[1]), "1", side);
        let accepted = propose_it(&sketch, &edit).unwrap();
        let offset = &accepted.sketch;
        let copies = new_curves(offset, &sketch);
        assert_eq!(copies.len(), 4);
        // Arcs about the slot's own centres, their radius one more or
        // less, the lines meeting them where they end.
        let arcs: Vec<_> = copies
            .iter()
            .filter_map(|entry| match entry.curve {
                Curve::Arc { center, .. } => Some((center, offset.round(entry.id).unwrap().1)),
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 2);
        for (center, found) in arcs {
            assert!(centers.contains(&center));
            assert!((found - radius).abs() < 1e-9, "{found}");
        }
        let lines = new_lines(offset, &sketch);
        let (low, high) = (2.0 - radius, 2.0 + radius);
        assert!(same_lines(
            &lines,
            &[((0.0, low), (10.0, low)), ((10.0, high), (0.0, high))]
        ));
        // Tangent where they meet, standing for all but one tie: the
        // dimension, between a line and its copy, holds the rest.
        let kinds = kinds(offset, &sketch);
        assert_eq!(kinds.len(), 5);
        assert_eq!(kinds.iter().filter(|kind| **kind == "tangent").count(), 4);
        assert!(matches!(
            dimension_of(offset).dimension.measure,
            Measure::Offset(a, _) if a == curves[0] || a == curves[2]
        ));
        assert_eq!(accepted.analysis.freedom, analyse(&sketch).freedom);
    }
    // Inwards by its half width, the arcs have nothing left, and the
    // lines are on each other.
    let edit = offset(curves.to_vec(), "2", Side::Positive);
    assert_eq!(edit.apply(&sketch, &DESIGN), Err(EditError::NothingLeft));
}

#[test]
fn an_open_chain_offsets_to_one_side_with_its_ends_free() {
    // An L: along x, then up.
    let mut sketch = Sketch::default();
    let [a, b, c] = [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)].map(|(x, y)| point(&mut sketch, x, y));
    let along = line(&mut sketch, a, b);
    let up = line(&mut sketch, b, c);
    let chain = sketch.chain_of(up);
    assert_eq!(chain.len(), 2);
    // Inside the corner, the copies cross and what's past goes.
    let edit = offset(vec![along, up], "1", Side::Positive);
    let accepted = propose_it(&sketch, &edit).unwrap();
    assert!(same_lines(
        &new_lines(&accepted.sketch, &sketch),
        &[((0.0, 1.0), (9.0, 1.0)), ((9.0, 1.0), (9.0, 5.0))]
    ));
    // Outside, the corner runs on to meet.
    let edit = offset(vec![along, up], "1", Side::Negative);
    let accepted = propose_it(&sketch, &edit).unwrap();
    assert!(same_lines(
        &new_lines(&accepted.sketch, &sketch),
        &[((0.0, -1.0), (11.0, -1.0)), ((11.0, -1.0), (11.0, 5.0))]
    ));
    // The copy's two free ends can slide along it.
    let before = analyse(&sketch).freedom;
    assert_eq!(accepted.analysis.freedom, before + 2);
    // Run from a line drawn down, its left is the other side.
    let mut down = Sketch::default();
    let [a, b, c] = [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)].map(|(x, y)| point(&mut down, x, y));
    let along = line(&mut down, a, b);
    let up = line(&mut down, c, b);
    let edit = offset(vec![up, along], "1", Side::Positive);
    let applied = edit.apply(&down, &DESIGN).unwrap();
    assert!(same_lines(
        &new_lines(&applied, &down),
        &[((0.0, -1.0), (11.0, -1.0)), ((11.0, -1.0), (11.0, 5.0))]
    ));
}

#[test]
fn a_circle_offsets_about_its_centre() {
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 3.0, 4.0);
    let round = circle(&mut sketch, center, 5.0);
    let edit = offset(sketch.chain_of(round), "2", Side::Negative);
    let accepted = propose_it(&sketch, &edit).unwrap();
    let grown = &accepted.sketch;
    let copy = new_curves(grown, &sketch)[0];
    assert_eq!(
        copy.curve,
        Curve::Circle {
            center,
            radius: 7.0
        }
    );
    assert_eq!(
        dimension_of(grown).dimension.measure,
        Measure::Offset(round, copy.id)
    );
    assert_eq!(accepted.analysis.freedom, analyse(&sketch).freedom);
    let edit = offset(vec![round], "2", Side::Positive);
    let shrunk = edit.apply(&sketch, &DESIGN).unwrap();
    assert!(matches!(
        shrunk.curves.last().unwrap().curve,
        Curve::Circle { radius, .. } if radius == 3.0
    ));
    for distance in ["5", "6"] {
        let edit = offset(vec![round], distance, Side::Positive);
        assert_eq!(edit.apply(&sketch, &DESIGN), Err(EditError::NothingLeft));
    }
}

#[test]
fn offsetting_past_a_notch_takes_away_what_falls_within_it() {
    // A plate 20 by 10 with a notch 4 wide cut 6 down from the top.
    let (sketch, lines) = polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 10.0),
        (12.0, 10.0),
        (12.0, 4.0),
        (8.0, 4.0),
        (8.0, 10.0),
        (0.0, 10.0),
    ]);
    // Inwards by 3: the notch's copy runs through the plate's, splitting
    // it in two, and its bottom's copy is gone.
    let edit = offset(lines.clone(), "3", Side::Positive);
    let accepted = propose_it(&sketch, &edit).unwrap();
    let split = &accepted.sketch;
    assert!(same_lines(
        &new_lines(split, &sketch),
        &[
            ((3.0, 3.0), (5.0, 3.0)),
            ((15.0, 3.0), (17.0, 3.0)),
            ((17.0, 3.0), (17.0, 7.0)),
            ((17.0, 7.0), (15.0, 7.0)),
            ((15.0, 7.0), (15.0, 3.0)),
            ((5.0, 3.0), (5.0, 7.0)),
            ((5.0, 7.0), (3.0, 7.0)),
            ((3.0, 7.0), (3.0, 3.0)),
        ]
    ));
    // The plate, with the two as holes, and each inside one.
    assert_eq!(split.profiles().unwrap().regions.len(), 3);
    assert_eq!(accepted.analysis.freedom, analyse(&sketch).freedom);
    // By 1, the notch is still one.
    let edit = offset(lines.clone(), "1", Side::Positive);
    let applied = edit.apply(&sketch, &DESIGN).unwrap();
    assert_eq!(new_lines(&applied, &sketch).len(), 8);
}

#[test]
fn a_corner_turning_away_sharply_is_rounded() {
    // A thin spike: out along x and back, turning by more than 150°.
    let (sketch, lines) = polygon(&[(0.0, 0.0), (10.0, 0.5), (0.0, 1.0)]);
    let edit = offset(lines, "1", Side::Negative);
    let accepted = propose_it(&sketch, &edit).unwrap();
    let offset = &accepted.sketch;
    let joins: Vec<_> = new_curves(offset, &sketch)
        .into_iter()
        .filter_map(|entry| match entry.curve {
            Curve::Arc { center, .. } => Some((center, offset.round(entry.id).unwrap().1)),
            _ => None,
        })
        .collect();
    // About the spike's tip, as far as the offset.
    let tip = sketch.points[1].id;
    assert_eq!(joins.len(), 1);
    assert_eq!(joins[0].0, tip);
    assert!((joins[0].1 - 1.0).abs() < 1e-9);
    assert_eq!(accepted.analysis.freedom, analyse(&sketch).freedom);
    // The round join is tangent to the lines either side.
    assert_eq!(
        kinds(offset, &sketch)
            .iter()
            .filter(|kind| **kind == "tangent")
            .count(),
        2
    );
}

#[test]
fn changing_the_offset_s_value_moves_the_copy() {
    let (mut sketch, lines) = rectangle();
    for line in [lines[0], lines[2]] {
        constrain(&mut sketch, Constraint::Fix(line));
    }
    let edit = offset(lines.clone(), "2", Side::Negative);
    let grown = propose_it(&sketch, &edit).unwrap().sketch;
    let id = dimension_of(&grown).id;
    let measure = dimension_of(&grown).dimension.measure.clone();
    let edit = SketchEdit::SetDimension {
        id,
        value: value("3", &measure),
    };
    let moved = propose_it(&grown, &edit).unwrap();
    assert!(same_lines(
        &new_lines(&moved.sketch, &sketch),
        &[
            ((-3.0, -3.0), (13.0, -3.0)),
            ((13.0, -3.0), (13.0, 9.0)),
            ((13.0, 9.0), (-3.0, 9.0)),
            ((-3.0, 9.0), (-3.0, -3.0)),
        ]
    ));
    // The rectangle stays where it was.
    for line in lines {
        assert_eq!(moved.sketch.line(line), sketch.line(line));
    }
    // And a slot's arcs follow theirs.
    let (mut slot, curves, _) = slot();
    for arc in [curves[1], curves[3]] {
        constrain(&mut slot, Constraint::Fix(arc));
    }
    let edit = offset(curves.to_vec(), "1", Side::Negative);
    let grown = propose_it(&slot, &edit).unwrap().sketch;
    let entry = dimension_of(&grown);
    let edit = SketchEdit::SetDimension {
        id: entry.id,
        value: value("1.5", &entry.dimension.measure),
    };
    let moved = propose_it(&grown, &edit).unwrap().sketch;
    for entry in new_curves(&moved, &slot) {
        if let Some((_, radius)) = moved.round(entry.id) {
            assert!((radius - 3.5).abs() < 1e-9, "{radius}");
        }
    }
    assert!(same_lines(
        &new_lines(&moved, &slot),
        &[((0.0, -1.5), (10.0, -1.5)), ((10.0, 5.5), (0.0, 5.5))]
    ));
}

#[test]
fn what_isn_t_a_chain_or_a_distance_is_refused() {
    let (mut sketch, lines) = rectangle();
    let apart = [(20.0, 0.0), (30.0, 0.0)].map(|(x, y)| point(&mut sketch, x, y));
    let other = line(&mut sketch, apart[0], apart[1]);
    let center = point(&mut sketch, 50.0, 0.0);
    let round = circle(&mut sketch, center, 2.0);
    for chain in [
        vec![lines[0], other],
        vec![round, other],
        vec![lines[0], lines[2]],
        vec![],
    ] {
        let edit = offset(chain.clone(), "1", Side::Positive);
        assert_eq!(
            edit.apply(&sketch, &DESIGN),
            Err(EditError::NotAChain),
            "{chain:?}"
        );
    }
    let edit = offset(vec![center], "1", Side::Positive);
    assert_eq!(edit.apply(&sketch, &DESIGN), Err(EditError::Target(center)));
    // Three lines at a point.
    let from = sketch.points[0].id;
    let spur_end = point(&mut sketch, -5.0, -5.0);
    let spur = line(&mut sketch, from, spur_end);
    let edit = offset(vec![lines[0], lines[3], spur], "1", Side::Positive);
    assert_eq!(edit.apply(&sketch, &DESIGN), Err(EditError::NotAChain));
    // The chain a line is in stops where three meet, and goes on round
    // where two do.
    let mut chain = sketch.chain_of(lines[1]);
    assert_eq!(chain[0], lines[1]);
    chain.sort_unstable();
    assert_eq!(chain, lines);
    assert_eq!(sketch.chain_of(spur), [spur]);
    // No distance, or one past the limit.
    for distance in [0.0, -1.0, f64::NAN, 2e6] {
        let edit = SketchEdit::Offset {
            chain: vec![round],
            distance: varde_expr::Value {
                text: "1".into(),
                value: distance,
            },
            side: Side::Negative,
        };
        assert!(
            matches!(
                edit.apply(&sketch, &DESIGN),
                Err(EditError::OutOfRange { .. })
            ),
            "{distance}"
        );
    }
}

#[test]
fn the_side_and_the_preview_are_the_tool_s() {
    let (sketch, lines) = rectangle();
    let chain = sketch.chain_of(lines[0]);
    // The bottom runs right: above it, inside, is its left.
    assert_eq!(
        sketch.offset_side(&chain, DVec2::new(5.0, 1.5)),
        Some((1.5, Side::Positive))
    );
    assert_eq!(
        sketch.offset_side(&chain, DVec2::new(5.0, -2.0)),
        Some((2.0, Side::Negative))
    );
    // Nearest the right side.
    assert_eq!(
        sketch.offset_side(&chain, DVec2::new(13.0, 3.0)),
        Some((3.0, Side::Negative))
    );
    assert_eq!(sketch.offset_side(&[], DVec2::ZERO), None);
    let preview = sketch.offset_preview(&chain, 1.0, Side::Positive).unwrap();
    assert_eq!(preview.len(), 4);
    // Round the inside, 1 in.
    let on_edge = |p: &DVec2| {
        let (x, y) = ((p.x - 5.0).abs() - 4.0, (p.y - 3.0).abs() - 2.0);
        x.max(y).abs() < 1e-9
    };
    assert!(preview.iter().flatten().all(on_edge));
}

#[test]
fn offset_pairs_are_checked() {
    let (mut sketch, lines) = rectangle();
    let center = point(&mut sketch, 5.0, 3.0);
    let round = circle(&mut sketch, center, 1.0);
    let other = circle(&mut sketch, center, 2.0);
    assert_eq!(
        sketch.offset_pair([lines[0], lines[2]]),
        Some(OffsetPair::Lines)
    );
    assert_eq!(sketch.offset_pair([round, other]), Some(OffsetPair::Rounds));
    assert_eq!(sketch.offset_pair([center, other]), Some(OffsetPair::Join));
    assert_eq!(sketch.offset_pair([lines[0], round]), None);
    assert_eq!(sketch.offset_pair([round, round]), None);
    assert!(sketch.offset_pair([lines[1], lines[0]]).is_some());
    let corner = sketch.points[0].id;
    assert_eq!(sketch.offset_pair([corner, other]), None);
    // A dimension of an offset pair, and an equal offset of two, with a
    // first in common.
    let mut fits = sketch.clone();
    dimension(&mut fits, Measure::Offset(center, round), "1");
    constrain(
        &mut fits,
        Constraint::EqualOffset {
            a: [lines[0], lines[2]],
            b: [lines[0], lines[1]],
        },
    );
    for (constraint, why) in [
        (
            Constraint::EqualOffset {
                a: [lines[0], round],
                b: [round, other],
            },
            SketchError::Unfit(Id(fits.next_id)),
        ),
        (
            Constraint::EqualOffset {
                a: [round, other],
                b: [round, other],
            },
            SketchError::Repeated {
                from: Id(fits.next_id),
                to: other,
            },
        ),
    ] {
        let mut refused = fits.clone();
        refused.add_constraint(constraint).unwrap();
        assert_eq!(refused.check(&DESIGN), Err(why));
    }
    let mut refused = fits.clone();
    let measure = Measure::Offset(corner, round);
    refused
        .add_dimension(Dimension {
            measure: measure.clone(),
            value: value("1", &measure),
            driving: false,
            label: DVec2::ZERO,
            side: Side::Positive,
        })
        .unwrap();
    assert_eq!(
        refused.check(&DESIGN),
        Err(SketchError::Unfit(Id(fits.next_id)))
    );
}

/// A rectangle 10 by 6 from the origin with its corners rounded by
/// `radius`, held tangent: its lines and arcs, counter-clockwise from the
/// bottom.
fn rounded(radius: f64) -> (Sketch, Vec<Id>) {
    let (w, h, r) = (10.0, 6.0, radius);
    let mut sketch = Sketch::default();
    let ends = [
        ((r, 0.0), (w - r, 0.0)),
        ((w, r), (w, h - r)),
        ((w - r, h), (r, h)),
        ((0.0, h - r), (0.0, r)),
    ];
    let centers = [(w - r, r), (w - r, h - r), (r, h - r), (r, r)];
    let ends: Vec<(Id, Id)> = ends
        .iter()
        .map(|&((x0, y0), (x1, y1))| (point(&mut sketch, x0, y0), point(&mut sketch, x1, y1)))
        .collect();
    let mut curves = Vec::new();
    for i in 0..4 {
        curves.push(line(&mut sketch, ends[i].0, ends[i].1));
        let (x, y) = centers[i];
        let center = point(&mut sketch, x, y);
        curves.push(arc(&mut sketch, center, ends[i].1, ends[(i + 1) % 4].0));
    }
    for i in 0..8 {
        let tangent = sketch.tangent(curves[i], curves[(i + 1) % 8]).unwrap();
        constrain(&mut sketch, tangent);
    }
    (sketch, curves)
}

#[test]
fn a_rounded_rectangle_offsets_round_or_sharp_past_its_corners() {
    let (sketch, curves) = rounded(1.0);
    let before = analyse(&sketch).freedom;
    // Outwards, the corners stay round, about the same centres.
    let edit = offset(curves.clone(), "1", Side::Negative);
    let accepted = propose_it(&sketch, &edit).unwrap();
    let copies = new_curves(&accepted.sketch, &sketch);
    assert_eq!(copies.len(), 8);
    for entry in &copies {
        if let Some((_, radius)) = accepted.sketch.round(entry.id) {
            assert!((radius - 2.0).abs() < 1e-9, "{radius}");
        }
    }
    assert_eq!(accepted.analysis.freedom, before);
    // Inwards past the corners' radius, they have no copy: the lines run
    // on to meet.
    let edit = offset(curves, "2", Side::Positive);
    let accepted = propose_it(&sketch, &edit).unwrap();
    assert!(same_lines(
        &new_lines(&accepted.sketch, &sketch),
        &[
            ((2.0, 2.0), (8.0, 2.0)),
            ((8.0, 2.0), (8.0, 4.0)),
            ((8.0, 4.0), (2.0, 4.0)),
            ((2.0, 4.0), (2.0, 2.0)),
        ]
    ));
    assert_eq!(new_curves(&accepted.sketch, &sketch).len(), 4);
    assert_eq!(accepted.analysis.freedom, before);
}

#[test]
fn a_loop_of_arcs_alone_frees_a_copy_s_centre() {
    // A lens: two arcs meeting at (0, -3) and (0, 3).
    let mut sketch = Sketch::default();
    let [low, high] = [(0.0, -3.0), (0.0, 3.0)].map(|(x, y)| point(&mut sketch, x, y));
    let [left, right] = [(-4.0, 0.0), (4.0, 0.0)].map(|(x, y)| point(&mut sketch, x, y));
    let bulging_right = arc(&mut sketch, left, low, high);
    let bulging_left = arc(&mut sketch, right, high, low);
    let before = analyse(&sketch).freedom;
    // Outwards, round joins at its tips: four arcs each tangent to the
    // next, one about a centre of its own at its original's.
    let edit = offset(vec![bulging_right, bulging_left], "1", Side::Negative);
    let accepted = propose_it(&sketch, &edit).unwrap();
    let grown = &accepted.sketch;
    let copies = new_curves(grown, &sketch);
    assert_eq!(copies.len(), 4);
    let centers: Vec<Id> = copies
        .iter()
        .filter_map(|entry| match entry.curve {
            Curve::Arc { center, .. } => Some(center),
            _ => None,
        })
        .collect();
    let own: Vec<&Id> = centers
        .iter()
        .filter(|&&center| sketch.point(center).is_none())
        .collect();
    assert_eq!(own.len(), 1);
    let at_original = [left, right].map(|id| sketch.point(id).unwrap().at);
    assert!(
        at_original
            .iter()
            .any(|&p| near(p, grown.point(*own[0]).unwrap().at))
    );
    assert_eq!(
        kinds(grown, &sketch)
            .iter()
            .filter(|kind| **kind == "tangent")
            .count(),
        4
    );
    assert_eq!(accepted.analysis.freedom, before);
}

/// A random generator, the same every run.
fn random(mut seed: u64) -> impl FnMut() -> f64 {
    move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as f64 / (1u64 << 31) as f64
    }
}

/// Whether `curves` of `sketch` meet only at their ends.
fn meet_at_ends(sketch: &Sketch, curves: &[Id]) -> bool {
    let geoms: Vec<Geom> = curves
        .iter()
        .map(|&id| Geom::of(sketch, &sketch.curve(id).unwrap().curve).unwrap())
        .collect();
    let at_end = |geom: &Geom, u: f64| {
        !geom.has_ends() || geom.length(0.0, u) < 1e-6 || geom.length(u, geom.last()) < 1e-6
    };
    let mut found = Vec::new();
    for (i, a) in geoms.iter().enumerate() {
        for b in &geoms[i + 1..] {
            found.clear();
            meet(a, b, 1e-9, &mut found);
            if found.iter().any(|&(u, v)| !(at_end(a, u) && at_end(b, v))) {
                return false;
            }
        }
    }
    true
}

#[test]
fn random_loops_offset_into_closed_copies_that_never_cross() {
    let mut rand = random(12345);
    let mut offset_some = 0;
    for case in 0..varde_testing::pick(200, 400) {
        // Round the origin, a line or an arc from each corner to the next.
        let n = 3 + (rand() * 10.0) as usize;
        let corners: Vec<DVec2> = (0..n)
            .map(|i| {
                let angle = TAU * (i as f64 + rand() * 0.5) / n as f64;
                DVec2::from_angle(angle) * (1.0 + rand() * 9.0)
            })
            .collect();
        let mut sketch = Sketch::default();
        let points: Vec<Id> = corners
            .iter()
            .map(|p| point(&mut sketch, p.x, p.y))
            .collect();
        let mut curves = Vec::new();
        for i in 0..n {
            let (a, b) = (points[i], points[(i + 1) % n]);
            let (pa, pb) = (corners[i], corners[(i + 1) % n]);
            let bulge = (rand() - 0.5) * pa.distance(pb);
            let through = pa.midpoint(pb) + (pb - pa).perp().normalize() * bulge;
            match arc_through(pa, pb, through).filter(|_| rand() < 0.35) {
                Some(found) => {
                    let center = point(&mut sketch, found.center.x, found.center.y);
                    let (start, end) = if found.start == pa { (a, b) } else { (b, a) };
                    curves.push(arc(&mut sketch, center, start, end));
                }
                None => curves.push(line(&mut sketch, a, b)),
            }
        }
        if !meet_at_ends(&sketch, &curves) {
            continue;
        }
        let side = if rand() < 0.5 {
            Side::Positive
        } else {
            Side::Negative
        };
        let distance = format!("{:.3}", 0.1 + rand() * 4.0);
        let open = rand() < 0.3;
        let chain = if open { &curves[..n - 1] } else { &curves[..] };
        let edit = offset(chain.to_vec(), &distance, side);
        let accepted = match propose_it(&sketch, &edit) {
            Ok(accepted) => accepted,
            Err(Rejected::Edit(EditError::NothingLeft)) => continue,
            Err(why) => panic!("case {case}: {why:?}"),
        };
        offset_some += 1;
        let copies: Vec<Id> = new_curves(&accepted.sketch, &sketch)
            .iter()
            .map(|entry| entry.id)
            .collect();
        assert!(meet_at_ends(&accepted.sketch, &copies), "case {case}");
        if open {
            continue;
        }
        // A closed loop's copy is closed loops, held by the one dimension.
        let mut ends: BTreeMap<Id, usize> = BTreeMap::new();
        for &copy in &copies {
            if let Curve::Line { start, end } | Curve::Arc { start, end, .. } =
                accepted.sketch.curve(copy).unwrap().curve
            {
                *ends.entry(start).or_default() += 1;
                *ends.entry(end).or_default() += 1;
            }
        }
        assert!(ends.values().all(|&count| count == 2), "case {case}");
        assert_eq!(
            accepted.analysis.freedom,
            analyse(&sketch).freedom,
            "case {case}"
        );
    }
    // About two in three cases offset.
    assert!(offset_some > varde_testing::pick(100, 200), "{offset_some}");
}

#[test]
fn random_rounded_loops_keep_their_freedom() {
    let mut rand = random(777);
    let mut offset_some = 0;
    for case in 0..varde_testing::pick(40, 300) {
        // A polygon round the origin, each corner rounded by a tangent arc.
        let n = 3 + (rand() * 8.0) as usize;
        let corners: Vec<DVec2> = (0..n)
            .map(|i| {
                let angle = TAU * (i as f64 + rand() * 0.5) / n as f64;
                DVec2::from_angle(angle) * (3.0 + rand() * 7.0)
            })
            .collect();
        let mut rounds = Vec::new();
        for i in 0..n {
            let (before, at, after) = (corners[(i + n - 1) % n], corners[i], corners[(i + 1) % n]);
            let (back, on) = ((before - at).normalize(), (after - at).normalize());
            let half = back.angle_to(on).abs() / 2.0;
            let radius = 0.2 + rand() * 1.5;
            let along = radius / half.tan();
            if half < 0.05 || along > 0.45 * before.distance(at).min(after.distance(at)) {
                break;
            }
            let center = at + (back + on).normalize() * (radius / half.sin());
            rounds.push((at + back * along, at + on * along, center));
        }
        if rounds.len() < n {
            continue;
        }
        let mut sketch = Sketch::default();
        let ends: Vec<(Id, Id)> = rounds
            .iter()
            .map(|(from, to, _)| {
                (
                    point(&mut sketch, from.x, from.y),
                    point(&mut sketch, to.x, to.y),
                )
            })
            .collect();
        let mut curves = Vec::new();
        for (i, &(from, to, center)) in rounds.iter().enumerate() {
            let at = point(&mut sketch, center.x, center.y);
            let (start, end) = if arc_sweep(from - center, to - center) < PI {
                ends[i]
            } else {
                (ends[i].1, ends[i].0)
            };
            curves.push(arc(&mut sketch, at, start, end));
            curves.push(line(&mut sketch, ends[i].1, ends[(i + 1) % n].0));
        }
        for i in 0..2 * n {
            let tangent = sketch
                .tangent(curves[i], curves[(i + 1) % (2 * n)])
                .unwrap();
            sketch.add_constraint(tangent).unwrap();
        }
        let before = analyse(&sketch);
        assert!(before.redundant.is_empty(), "case {case}");
        let side = if rand() < 0.5 {
            Side::Positive
        } else {
            Side::Negative
        };
        let distance = format!("{:.3}", 0.05 + rand() * 3.0);
        let edit = offset(curves, &distance, side);
        match propose_it(&sketch, &edit) {
            Ok(accepted) => {
                offset_some += 1;
                assert_eq!(accepted.analysis.freedom, before.freedom, "case {case}");
            }
            Err(Rejected::Edit(EditError::NothingLeft)) => {}
            Err(why) => panic!("case {case}: {why:?}"),
        }
    }
    assert!(offset_some > varde_testing::pick(25, 100), "{offset_some}");
}

#[test]
fn a_chain_too_long_to_work_out_is_refused() {
    let n = 2500;
    let corners: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let angle = TAU * f64::from(i) / f64::from(n);
            (100.0 * angle.cos(), 100.0 * angle.sin())
        })
        .collect();
    // Most of the time is spending all of `MAX_OFFSET_WORK`, whatever
    // the length past it.
    let (sketch, lines) = polygon(&corners);
    assert_eq!(
        sketch.offset_preview(&lines, 1.0, Side::Positive),
        Err(EditError::TooComplex)
    );
}

#[test]
fn making_ends_one_counts_against_the_work() {
    // A thousand parts, each end loose and far from the rest.
    let places: Vec<DVec2> = (0..2000).map(|i| DVec2::new(f64::from(i), 0.0)).collect();
    let mut ends: Vec<Option<(usize, usize)>> =
        (0..1000).map(|i| Some((2 * i, 2 * i + 1))).collect();
    let mut work = Work::default();
    assert_eq!(loose_ends(&mut ends, &places, 0.5, &mut work), Ok(()));
    // Two thousand ends compared pairwise, about two million steps.
    assert!(work.0 >= 2000 * 1999 / 2, "{work:?}");
    let mut work = Work(MAX_OFFSET_WORK - 1000);
    assert_eq!(
        loose_ends(&mut ends, &places, 0.5, &mut work),
        Err(EditError::TooComplex)
    );
}

/// A spline through a gentle wave from (0, 0) to (30, 0), open or
/// `closed` round an oval about (15, 0), and its id.
fn wavy(closed: bool) -> (Sketch, Id) {
    let mut sketch = Sketch::default();
    let places: &[(f64, f64)] = if closed {
        &[(0.0, 0.0), (15.0, -8.0), (30.0, 0.0), (15.0, 8.0)]
    } else {
        &[
            (0.0, 0.0),
            (8.0, 4.0),
            (16.0, 1.0),
            (24.0, -3.0),
            (30.0, 0.0),
        ]
    };
    let (spline, _) = testing::spline(&mut sketch, places, closed);
    (sketch, spline)
}

/// How far from `distance` the places along the copy `copy` are from
/// the spline `spline`, at most.
fn off_by(sketch: &Sketch, spline: Id, copy: Id, distance: f64) -> f64 {
    let copy = testing::geom(sketch, copy);
    (0..=200)
        .map(|i| {
            let place = copy.at(copy.last() * i as f64 / 200.0);
            let (reach, _) = sketch.spline_side(spline, place).unwrap();
            (reach - distance).abs()
        })
        .fold(0.0, f64::max)
}

#[test]
fn a_spline_offsets_as_a_spline_along_it_tied_by_one_dimension() {
    for closed in [false, true] {
        let (sketch, spline) = wavy(closed);
        // Outside the oval is its right, running counter-clockwise.
        let side = if closed {
            Side::Negative
        } else {
            Side::Positive
        };
        let preview = sketch.offset_preview(&[spline], 2.0, side).unwrap();
        assert_eq!(preview.len(), 1);
        let accepted = propose_it(&sketch, &offset(vec![spline], "2", side)).unwrap();
        let offset = accepted.sketch;
        let copy = new_curves(&offset, &sketch)[0].clone();
        let Curve::Spline(copied) = &copy.curve else {
            panic!("a spline");
        };
        assert_eq!(
            (copied.kind, copied.closed),
            (crate::SplineKind::Through, closed)
        );
        assert!(copied.points.len() >= 4 && copied.points.len() <= crate::MAX_SPLINE_POINTS);
        // Along the exact offset, within its fit.
        let most = off_by(&offset, spline, copy.id, 2.0);
        assert!(most < 1e-4 * 40.0, "{closed}: {most}");
        // One dimension, and each fit point's offset equal to its.
        let dimensions: Vec<&DimensionEntry> = offset.dimensions.iter().collect();
        assert_eq!(dimensions.len(), 1);
        assert_eq!(
            dimensions[0].dimension.measure,
            Measure::Offset(spline, copied.points[0])
        );
        let equal = offset
            .constraints
            .iter()
            .filter(|entry| matches!(entry.constraint, Constraint::EqualOffset { .. }))
            .count();
        assert_eq!(equal, copied.points.len() - 1);
        // Its fit points slide along the offset, one freedom each.
        let freedom = analyse(&sketch).freedom;
        assert_eq!(accepted.analysis.freedom, freedom + copied.points.len());
        assert!(accepted.analysis.redundant.is_empty());

        // The dimension changed, the copy follows.
        let id = dimensions[0].id;
        let wider = SketchEdit::SetDimension {
            id,
            value: value("3", &Measure::Offset(Id::ORIGIN, Id::ORIGIN)),
        };
        let wider = propose_it(&offset, &wider).unwrap().sketch;
        let most = off_by(&wider, spline, copy.id, 3.0);
        assert!(most < 0.01, "{closed}: {most}");
        // And the spline reshaped, the copy's fit points follow it.
        let second = offset.spline(spline).unwrap().points[1];
        let moved = SketchEdit::Move {
            points: vec![(
                second,
                offset.point(second).unwrap().at + DVec2::new(0.8, 1.2),
            )],
            radii: Vec::new(),
        };
        let reshaped = propose_it(&offset, &moved).unwrap().sketch;
        for &point in &copied.points {
            let place = reshaped.point(point).unwrap().at;
            let (reach, _) = reshaped.spline_side(spline, place).unwrap();
            assert!((reach - 2.0).abs() < 1e-6, "{closed}: {reach}");
        }
    }
}

#[test]
fn a_spline_offset_past_where_it_curves_folds_and_is_refused() {
    let (sketch, spline) = wavy(true);
    // Inside the oval, past its ends' radius of curvature.
    let edit = offset(vec![spline], "6", Side::Positive);
    assert_eq!(edit.apply(&sketch, &DESIGN), Err(EditError::TooTight));
    assert_eq!(
        sketch.offset_preview(&[spline], 6.0, Side::Positive),
        Err(EditError::TooTight)
    );
    // A spline in a chain with a line isn't a chain an offset copies.
    let (mut sketch, spline) = wavy(false);
    let [_, end] = sketch.curve(spline).unwrap().curve.ends().unwrap();
    let beyond = point(&mut sketch, 40.0, 0.0);
    let on = line(&mut sketch, end, beyond);
    assert_eq!(sketch.chain_of(spline).len(), 2);
    assert!(!sketch.is_chain(&[spline, on]));
    assert!(sketch.is_chain(&[spline]));
    assert_eq!(
        offset(vec![spline, on], "1", Side::Positive).apply(&sketch, &DESIGN),
        Err(EditError::NotAChain)
    );
    // Which side a place is on, and how far.
    let (reach, side) = sketch.offset_side(&[spline], DVec2::new(8.0, 7.0)).unwrap();
    assert_eq!(side, Side::Positive);
    assert!(reach > 2.0 && reach < 3.5, "{reach}");
}
