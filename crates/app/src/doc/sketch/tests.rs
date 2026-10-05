use std::cell::RefCell;
use std::collections::BTreeSet;

use glam::DVec2;
use iced::keyboard::{self, key};
use varde_document::{Command, Document, Editor, FeatureId, MAX_COORD, OriginPlane};
use varde_io::{Access, ReadOnly};
use varde_regen::Request;
use varde_sketch::{Curve, Id};
use varde_view::{Edit, Look, Tool, ToolClick};

use super::*;
use crate::doc::{Origin, Target};
use crate::tests::{SolveLane, answer, key_in, press_in, with_sketch};

mod faces;
mod outside;

/// A pixel's size in sketch units in the clicks here.
pub(super) const PIXEL: f64 = 0.1;

/// A document with a sketch being edited, whose solver lane the test
/// answers after each message it's given, as if at once. Looks like the
/// document otherwise.
pub(super) struct Answered {
    pub(super) doc: Doc,
    pub(super) lane: SolveLane,
}

impl std::ops::Deref for Answered {
    type Target = Doc;

    fn deref(&self) -> &Doc {
        &self.doc
    }
}

impl std::ops::DerefMut for Answered {
    fn deref_mut(&mut self) -> &mut Doc {
        &mut self.doc
    }
}

impl Answered {
    pub(super) fn update(&mut self, message: Edit) {
        self.doc.update(message);
        self.lane.answer(&mut self.doc);
    }

    pub(super) fn look(&mut self, message: Look) {
        self.doc.look(message);
        self.lane.answer(&mut self.doc);
    }

    /// Presses `key`, see [`key_in`].
    pub(super) fn key(&mut self, key: keyboard::Key) {
        key_in(&mut self.doc, key);
        self.lane.answer(&mut self.doc);
    }

    /// Presses the letter `key` with Shift, sending what it sends.
    pub(super) fn shift_key(&mut self, key: &str) {
        let press = crate::tests::press(letter(key), keyboard::Modifiers::SHIFT);
        match crate::keys::document_key((self.doc.keys(), press)) {
            Some(crate::Message::Ui(varde_view::Message::Edit(edit))) => self.update(edit),
            Some(crate::Message::Ui(varde_view::Message::Look(look))) => self.look(look),
            other => panic!("{key}: {other:?}"),
        }
    }
}

/// A document with an empty sketch on XY being edited, whose solver lane
/// answers at once, and whose regeneration requests wait for the test, and
/// the list they wait in.
pub(super) fn sketching() -> (Answered, FeatureId, std::rc::Rc<RefCell<Vec<Request>>>) {
    let (mut doc, feature, requests) = with_sketch();
    let lane = SolveLane::connect(&mut doc);
    let mut doc = Answered { doc, lane };
    doc.look(Look::EditFeature(feature));
    answer(&mut doc, &requests);
    (doc, feature, requests)
}

/// The sketch being edited, as the document holds it.
pub(super) fn sketch(doc: &Doc) -> &Sketch {
    doc.edited_sketch().unwrap().1
}

/// The sketch being edited as it's shown.
pub(super) fn shown(doc: &Doc) -> &Sketch {
    doc.sketch_state().unwrap().sketch
}

/// Clicks the tool in use at `x`, `y`, on nothing.
pub(super) fn click(doc: &mut Answered, x: f64, y: f64) {
    click_on(doc, x, y, None);
}

/// Clicks the tool in use at `x`, `y`, on `point`.
pub(super) fn click_on(doc: &mut Answered, x: f64, y: f64, point: Option<Id>) {
    doc.update(Edit::ToolClick(ToolClick {
        target: point.map(varde_view::Target::Point),
        hit: point,
        ..click_at(x, y)
    }));
}

/// A click of the tool in use at `x`, `y`, snapped to nothing and on
/// nothing, a pixel [`PIXEL`] across.
pub(super) fn click_at(x: f64, y: f64) -> ToolClick {
    ToolClick {
        at: DVec2::new(x, y),
        target: None,
        inference: None,
        hit: None,
        pixel: PIXEL,
        double: false,
        reference: false,
    }
}

pub(super) fn at(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// The key of the letter `c`.
pub(super) fn letter(c: &str) -> keyboard::Key {
    keyboard::Key::Character(c.into())
}

/// The tool in use.
pub(super) fn drawing(doc: &Doc) -> Option<&Drawing> {
    doc.sketch
        .as_ref()
        .and_then(|session| session.tool.as_ref())
}

/// The ends of each line, by where they are.
pub(super) fn lines(sketch: &Sketch) -> Vec<(Id, Id)> {
    sketch
        .curves
        .iter()
        .filter_map(|entry| match entry.curve {
            Curve::Line { start, end } => Some((start, end)),
            _ => None,
        })
        .collect()
}

pub(super) fn position(sketch: &Sketch, id: Id) -> DVec2 {
    sketch.point(id).unwrap().at
}

/// How many times the document can be undone until the sketch is as
/// `before`, undoing it that far.
pub(super) fn undo_to(doc: &mut Answered, before: &Sketch) -> usize {
    let mut steps = 0;
    while sketch(doc) != before {
        assert!(doc.editor.can_undo(), "never back to {before:?}");
        doc.update(Edit::Undo);
        steps += 1;
    }
    steps
}

#[test]
fn the_line_tool_draws_a_chain_sharing_points_and_closes_it() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    assert_eq!(drawing(&doc).map(|drawing| drawing.tool), Some(Tool::Line));

    // The first click commits nothing.
    click(&mut doc, 0.0, 0.0);
    assert!(sketch(&doc).points.is_empty());
    assert_eq!(drawing(&doc).unwrap().placed, [at(0.0, 0.0)]);
    click(&mut doc, 10.0, 0.0);
    click(&mut doc, 10.0, 10.0);
    let drawn = sketch(&doc).clone();
    let [(a, b), (c, d)] = lines(&drawn)[..] else {
        panic!("{drawn:?}");
    };
    assert_eq!(b, c, "chained lines share their point");
    assert_eq!(drawn.points.len(), 3);
    assert_eq!(position(&drawn, d), at(10.0, 10.0));
    assert_eq!(drawing(&doc).unwrap().placed, [at(10.0, 10.0)]);

    // Clicking the first point closes the loop and ends the chain.
    click_on(&mut doc, 0.1, 0.0, Some(a));
    let closed = sketch(&doc).clone();
    assert_eq!(lines(&closed).len(), 3);
    assert_eq!(lines(&closed)[2], (d, a));
    assert_eq!(closed.points.len(), 3);
    let tool = drawing(&doc).unwrap();
    assert_eq!(tool.tool, Tool::Line);
    assert!(tool.placed.is_empty() && tool.chain.is_none());

    // One undo step a line.
    assert_eq!(undo_to(&mut doc, &Sketch::default()), 3);
}

#[test]
fn a_chain_is_closed_only_once_it_has_two_lines() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 0.0);
    click(&mut doc, 10.0, 0.0);
    let first = lines(sketch(&doc))[0].0;
    // Back over its only line would make no loop.
    click_on(&mut doc, 0.0, 0.0, Some(first));
    assert_eq!(lines(sketch(&doc)).len(), 1);
    // Another point there than the chain's first is just a click.
    click(&mut doc, 10.0, 10.0);
    click(&mut doc, 0.0, 10.0);
    let other = lines(sketch(&doc))[0].1;
    click_on(&mut doc, 10.0, 0.0, Some(other));
    assert_eq!(lines(sketch(&doc)).len(), 4);
    assert!(drawing(&doc).unwrap().chain.is_some());
}

#[test]
fn a_line_is_never_drawn_over_one_joining_the_same_points() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 0.0);
    click(&mut doc, 10.0, 0.0);
    click(&mut doc, 10.0, 10.0);
    let [(a, b), (_, c)] = lines(sketch(&doc))[..] else {
        panic!("{:?}", sketch(&doc));
    };
    // Back to the point before the last: over the line just drawn.
    click_on(&mut doc, 10.0, 0.0, Some(b));
    assert_eq!(lines(sketch(&doc)).len(), 2);
    assert_eq!(drawing(&doc).unwrap().chain.unwrap().last, c);
    // Nor from a line's end back to its start, outside a chain.
    doc.look(Look::Escape);
    click_on(&mut doc, 10.0, 0.0, Some(b));
    click_on(&mut doc, 0.0, 0.0, Some(a));
    assert_eq!(lines(sketch(&doc)).len(), 2);
}

#[test]
fn escape_and_a_double_click_end_an_open_chain() {
    let (mut doc, feature, _) = sketching();
    doc.key(keyboard::Key::Character("l".into()));
    click(&mut doc, 0.0, 0.0);
    click(&mut doc, 10.0, 0.0);
    // Escape ends the chain, then stops the tool, then leaves the sketch.
    doc.look(Look::Escape);
    let tool = drawing(&doc).unwrap();
    assert!(tool.placed.is_empty() && tool.chain.is_none());
    click(&mut doc, 20.0, 0.0);
    click(&mut doc, 30.0, 0.0);
    assert_eq!(sketch(&doc).points.len(), 4, "a new chain");

    // The second click of a double-click adds nothing and ends it.
    doc.update(Edit::ToolClick(ToolClick {
        double: true,
        ..click_at(30.0, 0.0)
    }));
    assert_eq!(lines(sketch(&doc)).len(), 2);
    assert!(drawing(&doc).unwrap().placed.is_empty());

    doc.look(Look::Escape);
    assert!(drawing(&doc).is_none());
    assert_eq!(
        doc.sketch.as_ref().map(|session| session.feature),
        Some(feature)
    );
    doc.look(Look::Escape);
    assert!(doc.sketch.is_none());
}

#[test]
fn undoing_a_chain_s_line_ends_the_chain() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 0.0);
    click(&mut doc, 10.0, 0.0);
    click(&mut doc, 10.0, 10.0);
    // Its last point is gone with the last line, so the tool starts
    // afresh, keeping the line before.
    doc.update(Edit::Undo);
    let tool = drawing(&doc).unwrap();
    assert!(tool.placed.is_empty() && tool.chain.is_none());
    assert_eq!(lines(sketch(&doc)).len(), 1);
    click(&mut doc, 5.0, 5.0);
    click(&mut doc, 6.0, 5.0);
    assert_eq!(lines(sketch(&doc)).len(), 2);
    assert_eq!(sketch(&doc).points.len(), 4);
}

#[test]
fn the_circle_tool_takes_a_centre_and_a_point_on_it() {
    let (mut doc, _, _) = sketching();
    doc.key(keyboard::Key::Character("c".into()));
    assert_eq!(drawing(&doc).unwrap().tool, Tool::Circle);
    click(&mut doc, 1.0, 2.0);
    // No radius is no circle.
    click(&mut doc, 1.0, 2.0 + PIXEL / 2.0);
    assert!(sketch(&doc).curves.is_empty());
    click(&mut doc, 4.0, 6.0);
    let drawn = sketch(&doc);
    let [entry] = &drawn.curves[..] else {
        panic!("{drawn:?}");
    };
    let Curve::Circle { center, radius } = entry.curve else {
        panic!("{entry:?}");
    };
    assert_eq!((position(drawn, center), radius), (at(1.0, 2.0), 5.0));
    assert!(!entry.construction);
    // The tool stays, for the next.
    assert!(drawing(&doc).unwrap().placed.is_empty());
    assert_eq!(undo_to(&mut doc, &Sketch::default()), 1);
}

#[test]
fn the_arc_tool_takes_its_ends_and_a_point_on_it() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Arc));
    // Over the top, counter-clockwise from the first; under the bottom,
    // from the second.
    for (through, start, end) in [
        (1.0, at(1.0, 0.0), at(-1.0, 0.0)),
        (-1.0, at(-1.0, 0.0), at(1.0, 0.0)),
    ] {
        let before = sketch(&doc).clone();
        click(&mut doc, 1.0, 0.0);
        click(&mut doc, -1.0, 0.0);
        click(&mut doc, 0.0, through);
        let drawn = sketch(&doc);
        let entry = drawn.curves.last().unwrap();
        let Curve::Arc {
            center,
            start: s,
            end: e,
        } = entry.curve
        else {
            panic!("{entry:?}");
        };
        assert!(position(drawn, center).abs_diff_eq(DVec2::ZERO, 1e-12));
        assert_eq!((position(drawn, s), position(drawn, e)), (start, end));
        let mut after = drawn.clone();
        after.delete(&[entry.id]);
        assert_eq!(after.points.len(), before.points.len(), "three new points");
    }
    assert!(drawing(&doc).unwrap().placed.is_empty());

    // Points on one line make no arc, and the tool waits for a better
    // third.
    let before = sketch(&doc).clone();
    click(&mut doc, 0.0, 0.0);
    click(&mut doc, 2.0, 0.0);
    click(&mut doc, 1.0, PIXEL / 2.0);
    click(&mut doc, 5.0, 0.0);
    assert_eq!(*sketch(&doc), before);
    assert_eq!(drawing(&doc).unwrap().placed.len(), 2);
    // Nor do ends at one place.
    doc.look(Look::Escape);
    click(&mut doc, 0.0, 0.0);
    click(&mut doc, 0.0, 0.0);
    assert_eq!(drawing(&doc).unwrap().placed.len(), 1);
}

#[test]
fn the_point_tool_places_lone_points() {
    let (mut doc, _, _) = sketching();
    doc.key(keyboard::Key::Character("p".into()));
    click(&mut doc, 3.0, 4.0);
    click(&mut doc, 5.0, 6.0);
    let drawn = sketch(&doc);
    assert_eq!(
        drawn
            .points
            .iter()
            .map(|point| point.at)
            .collect::<Vec<_>>(),
        [at(3.0, 4.0), at(5.0, 6.0)]
    );
    assert!(drawn.curves.is_empty());
    assert_eq!(undo_to(&mut doc, &Sketch::default()), 2);
}

#[test]
fn a_line_shorter_than_a_pixel_is_refused() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 0.0);
    click(&mut doc, PIXEL / 2.0, 0.0);
    assert!(sketch(&doc).points.is_empty());
    assert_eq!(drawing(&doc).unwrap().placed, [at(0.0, 0.0)]);
}

#[test]
fn a_shape_past_the_coordinate_limit_is_refused_and_says_why() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Circle));
    let max = f64::from(MAX_COORD);
    click(&mut doc, max, 0.0);
    click(&mut doc, -max, 0.0);
    assert!(sketch(&doc).curves.is_empty());
    assert!(doc.edit_error.is_some());
    // Nonsense from the view is ignored.
    click(&mut doc, f64::NAN, 0.0);
    assert_eq!(drawing(&doc).unwrap().placed, [at(max, 0.0)]);
}

#[test]
fn tools_are_taken_up_and_put_down() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 0.0);
    // Another tool starts afresh; the same one again puts it down.
    doc.look(Look::SelectTool(Tool::Circle));
    assert_eq!(drawing(&doc).unwrap().tool, Tool::Circle);
    assert!(drawing(&doc).unwrap().placed.is_empty());
    doc.look(Look::SelectTool(Tool::Circle));
    assert!(drawing(&doc).is_none());
    assert!(!doc.keys().unwrap().drawing);

    // Outside a sketch, or read-only, there are none.
    doc.look(Look::FinishSketch);
    doc.look(Look::SelectTool(Tool::Line));
    assert!(doc.sketch.is_none());
    let mut editor = Editor::new(Document::default());
    editor
        .apply(
            editor
                .document()
                .add_sketch(varde_document::Plane::Origin(OriginPlane::XY)),
        )
        .unwrap();
    let mut read_only = Doc::new(
        editor.document().clone(),
        Origin::new(
            Target::None,
            Access::ReadOnly(ReadOnly::InUse),
            "a".to_owned(),
        ),
    );
    let feature = read_only.editor.document().features()[0].id;
    read_only.look(Look::EditFeature(feature));
    read_only.look(Look::SelectTool(Tool::Line));
    assert!(drawing(&read_only).is_none());
    assert!(press_in(&read_only, keyboard::Key::Character("l".into())).is_none());
}

#[test]
fn x_makes_the_tool_s_next_shapes_construction() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    doc.key(keyboard::Key::Character("x".into()));
    assert!(drawing(&doc).unwrap().construction);
    click(&mut doc, 0.0, 0.0);
    click(&mut doc, 1.0, 0.0);
    assert!(sketch(&doc).curves[0].construction);
    // Switching doesn't commit anything.
    assert_eq!(undo_to(&mut doc, &Sketch::default()), 1);
}

/// A sketch being edited with a line from (0, 0) to (10, 0) and a circle
/// of radius 2 around (20, 0), and their ids: the line's ends, the line,
/// the centre and the circle.
pub(super) fn with_shapes() -> (Answered, [Id; 5]) {
    let (mut doc, feature, _) = sketching();
    let mut sketch = Sketch::default();
    let a = sketch.add_point(at(0.0, 0.0)).unwrap();
    let b = sketch.add_point(at(10.0, 0.0)).unwrap();
    let line = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let c = sketch.add_point(at(20.0, 0.0)).unwrap();
    let circle = sketch
        .add_curve(
            Curve::Circle {
                center: c,
                radius: 2.0,
            },
            false,
        )
        .unwrap();
    doc.editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    doc.sync();
    doc.lane.answer(&mut doc.doc);
    (doc, [a, b, line, c, circle])
}

pub(super) fn selection(doc: &Doc) -> Vec<Id> {
    doc.sketch
        .as_ref()
        .unwrap()
        .selection
        .iter()
        .copied()
        .collect()
}

#[test]
fn clicks_and_boxes_select_and_ctrl_adds() {
    let (mut doc, [a, b, line, c, circle]) = with_shapes();
    doc.look(Look::ClickGeometry {
        hit: Some(line),
        add: false,
    });
    assert_eq!(selection(&doc), [line]);
    doc.look(Look::ClickGeometry {
        hit: Some(a),
        add: false,
    });
    assert_eq!(selection(&doc), [a]);
    doc.look(Look::ClickGeometry {
        hit: Some(line),
        add: true,
    });
    assert_eq!(selection(&doc), [a, line]);
    // Again takes it out.
    doc.look(Look::ClickGeometry {
        hit: Some(a),
        add: true,
    });
    assert_eq!(selection(&doc), [line]);
    doc.look(Look::ClickGeometry {
        hit: None,
        add: true,
    });
    assert_eq!(selection(&doc), [line]);
    doc.look(Look::ClickGeometry {
        hit: None,
        add: false,
    });
    assert!(selection(&doc).is_empty());

    doc.look(Look::SelectBox {
        ids: vec![b, line],
        add: false,
    });
    assert_eq!(selection(&doc), [b, line]);
    doc.look(Look::SelectBox {
        ids: vec![c, circle],
        add: true,
    });
    assert_eq!(selection(&doc), [b, line, c, circle]);
    doc.look(Look::SelectBox {
        ids: vec![c],
        add: false,
    });
    assert_eq!(selection(&doc), [c]);
    assert!(doc.keys().unwrap().geometry_selected);
}

#[test]
fn a_row_of_the_overlaps_listed_is_hovered_and_chosen() {
    let (mut doc, [a, _, line, ..]) = with_shapes();
    let list = |ids| varde_view::Overlaps {
        held: DVec2::ZERO,
        at: DVec2::ZERO,
        items: varde_view::OverlapItems::Sketch(ids),
    };
    let hovered = |doc: &Doc| doc.sketch.as_ref().unwrap().hovered;
    doc.look(Look::ClickGeometry {
        hit: Some(line),
        add: false,
    });
    doc.look(Look::OpenOverlaps(list(vec![a, line])));
    doc.look(Look::HoverOverlap(Some(1)));
    assert_eq!(hovered(&doc), Some(line));
    // Moving up a row, the one entered tells it first.
    doc.look(Look::HoverOverlap(Some(0)));
    doc.look(Look::LeaveOverlap(1));
    assert_eq!(hovered(&doc), Some(a));
    doc.look(Look::ChooseOverlap {
        index: 0,
        add: false,
    });
    assert_eq!(selection(&doc), [a]);
    assert_eq!(hovered(&doc), None);
    assert!(doc.overlaps.is_none());

    // Closed by a click away, or `Esc`, the selection as it was; `Esc`
    // doesn't leave the sketch too.
    doc.look(Look::OpenOverlaps(list(vec![a, line])));
    doc.look(Look::HoverOverlap(Some(1)));
    doc.look(Look::CloseOverlaps);
    assert!(doc.overlaps.is_none());
    assert_eq!((selection(&doc), hovered(&doc)), (vec![a], None));
    doc.look(Look::OpenOverlaps(list(vec![a, line])));
    doc.look(Look::Escape);
    assert!(doc.overlaps.is_none() && doc.sketch.is_some());
    assert_eq!(selection(&doc), [a]);
    // With `Ctrl` held, chosen adds, the list kept open and its row
    // hovered; again it takes it out; its tick does the same.
    doc.look(Look::OpenOverlaps(list(vec![a, line])));
    doc.look(Look::HoverOverlap(Some(1)));
    doc.look(Look::ChooseOverlap {
        index: 1,
        add: true,
    });
    assert_eq!(selection(&doc), [a, line]);
    assert!(doc.overlaps.is_some());
    assert_eq!(hovered(&doc), Some(line));
    doc.look(Look::ChooseOverlap {
        index: 1,
        add: true,
    });
    assert_eq!(selection(&doc), [a]);
    doc.look(Look::ToggleOverlap(1));
    assert_eq!(selection(&doc), [a, line]);
    doc.look(Look::ToggleOverlap(0));
    assert_eq!(selection(&doc), [line]);
    assert!(doc.overlaps.is_some());
    // A click without it closes the list.
    doc.look(Look::ChooseOverlap {
        index: 0,
        add: false,
    });
    assert_eq!(selection(&doc), [a]);
    assert!(doc.overlaps.is_none());
}

#[test]
fn space_clears_the_selection_in_a_sketch_and_the_timeline_s_outside() {
    let (mut doc, [_, _, line, ..]) = with_shapes();
    let space = || keyboard::Key::Named(key::Named::Space);
    doc.look(Look::ClickGeometry {
        hit: Some(line),
        add: false,
    });
    doc.key(space());
    assert!(selection(&doc).is_empty());
    let feature = doc.sketch.as_ref().unwrap().feature;
    doc.look(Look::FinishSketch);
    assert_eq!(doc.selected_feature, Some(feature));
    doc.key(space());
    assert_eq!(doc.selected_feature, None);
    // Nothing selected, it does nothing.
    doc.key(space());
    assert_eq!(doc.selected_feature, None);
}

#[test]
fn space_puts_down_the_tool_in_a_sketch() {
    let (mut doc, [_, _, line, ..]) = with_shapes();
    let space = || keyboard::Key::Named(key::Named::Space);
    doc.look(Look::ClickGeometry {
        hit: Some(line),
        add: false,
    });
    doc.look(Look::SelectTool(Tool::Line));
    doc.key(space());
    let session = doc.sketch.as_ref().unwrap();
    assert!(session.tool.is_none());
    // The selection stays, and the next press clears it.
    assert_eq!(selection(&doc), [line]);
    doc.look(Look::ToggleConstrain);
    doc.key(space());
    assert!(!doc.sketch.as_ref().unwrap().constraining);
    doc.key(space());
    assert!(selection(&doc).is_empty());
}

#[test]
fn h_and_v_toggle_their_constraint_on_a_line() {
    let (mut doc, [_, _, line, ..]) = with_shapes();
    doc.look(Look::ClickGeometry {
        hit: Some(line),
        add: false,
    });
    let horizontal = |doc: &Doc| {
        (sketch(doc).constraints.iter())
            .filter(|entry| entry.constraint == varde_sketch::Constraint::Horizontal(line))
            .count()
    };
    doc.update(Edit::ToggleConstraint(
        varde_view::ConstraintKind::Horizontal,
    ));
    assert_eq!(horizontal(&doc), 1);
    doc.update(Edit::ToggleConstraint(
        varde_view::ConstraintKind::Horizontal,
    ));
    assert_eq!(horizontal(&doc), 0);
    doc.update(Edit::ToggleConstraint(
        varde_view::ConstraintKind::Horizontal,
    ));
    assert_eq!(horizontal(&doc), 1);
}

/// Any constraint applied again to what has it is taken off, rather than
/// restated and refused: perpendicular lines, one way round or the other.
#[test]
fn a_constraint_applied_again_toggles_off() {
    let (mut doc, feature, _) = sketching();
    let mut drawn = Sketch::default();
    let o = drawn.add_point(at(0.0, 0.0)).unwrap();
    let x = drawn.add_point(at(10.0, 1.0)).unwrap();
    let y = drawn.add_point(at(1.0, 10.0)).unwrap();
    let a = drawn
        .add_curve(Curve::Line { start: o, end: x }, false)
        .unwrap();
    let b = drawn
        .add_curve(Curve::Line { start: o, end: y }, false)
        .unwrap();
    // Already there, the other way round.
    drawn
        .add_constraint(varde_sketch::Constraint::Perpendicular(b, a))
        .unwrap();
    doc.editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(drawn),
        })
        .unwrap();
    doc.sync();
    doc.lane.answer(&mut doc.doc);
    doc.look(Look::SelectBox {
        ids: vec![a, b],
        add: false,
    });
    let perpendicular = |doc: &Doc| sketch(doc).constraints.len();
    let kind = varde_view::ConstraintKind::Perpendicular;
    doc.update(Edit::ToggleConstraint(kind));
    assert_eq!(perpendicular(&doc), 0);
    assert!(doc.sketch_state().unwrap().refusal.is_none());
    doc.update(Edit::ToggleConstraint(kind));
    assert_eq!(perpendicular(&doc), 1);
}

#[test]
fn the_geometry_list_folds_and_unfolds() {
    let (mut doc, [_, _, line, _, circle]) = with_shapes();
    doc.look(Look::ToggleExpanded(line));
    doc.look(Look::ToggleGroup(varde_view::GeometryGroup::Own));
    let state = doc.sketch_state().unwrap();
    assert!(state.expanded.contains(&line));
    assert!(state.folded.contains(&varde_view::GeometryGroup::Own));
    doc.look(Look::ToggleGroup(varde_view::GeometryGroup::Own));
    assert!(doc.sketch_state().unwrap().folded.is_empty());
    // A curve deleted is folded away with it.
    doc.look(Look::ToggleExpanded(circle));
    doc.look(Look::ClickGeometry {
        hit: Some(circle),
        add: false,
    });
    doc.update(Edit::DeleteSelection);
    assert!(!doc.sketch.as_ref().unwrap().expanded.contains(&circle));
}

#[test]
fn delete_removes_the_selection_and_what_depends_on_it() {
    let (mut doc, [a, b, line, c, circle]) = with_shapes();
    let before = sketch(&doc).clone();
    let delete = || keyboard::Key::Named(key::Named::Delete);
    assert!(press_in(&doc, delete()).is_none());
    doc.look(Look::ClickGeometry {
        hit: Some(line),
        add: false,
    });
    doc.key(delete());
    let after = sketch(&doc);
    assert!(after.kind(line).is_none() && after.kind(a).is_none() && after.kind(b).is_none());
    assert!(after.kind(c).is_some() && after.kind(circle).is_some());
    assert!(selection(&doc).is_empty());
    // Deleting a centre takes its circle.
    doc.look(Look::ClickGeometry {
        hit: Some(c),
        add: false,
    });
    doc.update(Edit::DeleteSelection);
    assert!(sketch(&doc).curves.is_empty());
    assert_eq!(undo_to(&mut doc, &before), 2);
}

/// What pressing Delete, then Backspace, in `doc` shown headless sends:
/// what its widgets send and the shortcuts for what they leave.
fn delete_keys(doc: &Doc) -> Vec<crate::Message> {
    let event = |named| {
        iced::Event::Keyboard(crate::tests::press(
            keyboard::Key::Named(named),
            keyboard::Modifiers::default(),
        ))
    };
    let keys = [event(key::Named::Delete), event(key::Named::Backspace)];
    let (sent, shortcuts) = crate::tests::pressed(doc, &keys, false);
    assert!(sent.is_empty(), "{sent:?}");
    shortcuts
}

#[test]
fn delete_and_backspace_reach_the_app_with_geometry_selected() {
    let (mut doc, [_, _, line, _, _]) = with_shapes();
    assert!(delete_keys(&doc).is_empty());
    doc.look(Look::ClickGeometry {
        hit: Some(line),
        add: false,
    });
    let sent = delete_keys(&doc);
    assert!(
        matches!(
            &sent[..],
            [
                crate::Message::Ui(varde_view::Message::Edit(Edit::DeleteSelection)),
                crate::Message::Ui(varde_view::Message::Edit(Edit::DeleteSelection)),
            ]
        ),
        "{sent:?}"
    );
}

#[test]
fn x_turns_the_selected_curves_construction_and_back() {
    let (mut doc, [a, _, line, _, circle]) = with_shapes();
    let before = sketch(&doc).clone();
    let construction = |doc: &Doc| {
        sketch(doc)
            .curves
            .iter()
            .map(|entry| entry.construction)
            .collect::<Vec<_>>()
    };
    doc.look(Look::SelectBox {
        ids: vec![a, line],
        add: false,
    });
    doc.update(Edit::ToggleConstruction);
    assert_eq!(construction(&doc), [true, false]);
    // Mixed, they all turn construction; all construction, all normal.
    doc.look(Look::ClickGeometry {
        hit: Some(circle),
        add: true,
    });
    doc.update(Edit::ToggleConstruction);
    assert_eq!(construction(&doc), [true, true]);
    doc.update(Edit::ToggleConstruction);
    assert_eq!(construction(&doc), [false, false]);
    assert_eq!(undo_to(&mut doc, &before), 0);
    doc.update(Edit::Undo);
    assert_eq!(construction(&doc), [true, true]);
}

#[test]
fn a_drag_shows_as_it_goes_and_commits_one_step_when_dropped() {
    let (mut doc, [a, b, line, ..]) = with_shapes();
    let before = sketch(&doc).clone();
    doc.look(Look::DragGeometry {
        id: b,
        from: at(10.0, 0.0),
        to: at(10.0, 1.0),
    });
    doc.look(Look::DragGeometry {
        id: b,
        from: at(10.0, 0.0),
        to: at(12.0, 3.0),
    });
    // Shown, not committed.
    assert_eq!(position(shown(&doc), b), at(12.0, 3.0));
    assert_eq!(*sketch(&doc), before);
    doc.update(Edit::DropGeometry);
    assert_eq!(position(sketch(&doc), b), at(12.0, 3.0));
    assert_eq!(undo_to(&mut doc, &before), 1);

    // A line moves with both ends.
    doc.look(Look::DragGeometry {
        id: line,
        from: at(5.0, 0.0),
        to: at(6.0, -1.0),
    });
    doc.update(Edit::DropGeometry);
    let moved = sketch(&doc);
    assert_eq!(
        (position(moved, a), position(moved, b)),
        (at(1.0, -1.0), at(11.0, -1.0))
    );
}

#[test]
fn a_circle_or_an_arc_dragged_by_its_edge_changes_radius() {
    let (mut doc, [.., c, circle]) = with_shapes();
    doc.look(Look::DragGeometry {
        id: circle,
        from: at(22.0, 0.0),
        to: at(20.0, 5.0),
    });
    doc.update(Edit::DropGeometry);
    let entry = sketch(&doc).curve(circle).unwrap();
    assert_eq!(
        entry.curve,
        Curve::Circle {
            center: c,
            radius: 5.0
        }
    );

    doc.look(Look::SelectTool(Tool::Arc));
    click(&mut doc, 1.0, 0.0);
    click(&mut doc, -1.0, 0.0);
    click(&mut doc, 0.0, 1.0);
    doc.look(Look::Escape);
    let arc = sketch(&doc).curves.last().unwrap().clone();
    let Curve::Arc { start, end, .. } = arc.curve else {
        panic!("{arc:?}");
    };
    doc.look(Look::DragGeometry {
        id: arc.id,
        from: at(0.0, 1.0),
        to: at(0.0, 3.0),
    });
    doc.update(Edit::DropGeometry);
    let drawn = sketch(&doc);
    assert!(position(drawn, start).abs_diff_eq(at(3.0, 0.0), 1e-9));
    assert!(position(drawn, end).abs_diff_eq(at(-3.0, 0.0), 1e-9));
}

#[test]
fn escape_or_an_undo_puts_a_drag_back() {
    let (mut doc, [_, b, ..]) = with_shapes();
    let before = sketch(&doc).clone();
    let drag = |doc: &mut Answered| {
        doc.look(Look::DragGeometry {
            id: b,
            from: at(10.0, 0.0),
            to: at(10.0, 4.0),
        });
    };
    drag(&mut doc);
    doc.look(Look::Escape);
    assert!(doc.sketch.is_some(), "Escape only puts the drag back");
    assert_eq!(*shown(&doc), before);
    doc.update(Edit::DropGeometry);
    assert_eq!(*sketch(&doc), before);

    drag(&mut doc);
    doc.update(Edit::Undo);
    assert_ne!(*sketch(&doc), before);
    assert_eq!(shown(&doc), sketch(&doc));
    doc.update(Edit::DropGeometry);
    assert_ne!(*sketch(&doc), before);
}

#[test]
fn a_drag_past_the_coordinate_limit_stops_short() {
    let (mut doc, [a, _, line, ..]) = with_shapes();
    let max = f64::from(MAX_COORD);
    doc.look(Look::DragGeometry {
        id: line,
        from: at(0.0, 0.0),
        to: at(max - 20.0, 0.0),
    });
    // The far end would go past it, so the line stays where it was.
    doc.look(Look::DragGeometry {
        id: line,
        from: at(0.0, 0.0),
        to: at(max - 5.0, 0.0),
    });
    assert_eq!(position(shown(&doc), a), at(max - 20.0, 0.0));
    doc.look(Look::DragGeometry {
        id: a,
        from: at(0.0, 0.0),
        to: at(f64::NAN, 0.0),
    });
    assert_eq!(position(shown(&doc), a), at(max - 20.0, 0.0));
}

#[test]
fn nothing_is_dragged_with_a_tool_or_read_only() {
    let (mut doc, [_, b, ..]) = with_shapes();
    doc.look(Look::SelectTool(Tool::Point));
    doc.look(Look::DragGeometry {
        id: b,
        from: at(10.0, 0.0),
        to: at(10.0, 4.0),
    });
    assert!(doc.sketch.as_ref().unwrap().drag.is_none());
    doc.look(Look::SelectTool(Tool::Point));
    doc.read_only = Some("read-only".to_owned());
    doc.look(Look::DragGeometry {
        id: b,
        from: at(10.0, 0.0),
        to: at(10.0, 4.0),
    });
    assert!(doc.sketch.as_ref().unwrap().drag.is_none());
    // Selecting still works.
    doc.look(Look::ClickGeometry {
        hit: Some(b),
        add: false,
    });
    assert_eq!(selection(&doc), [b]);
    doc.update(Edit::DeleteSelection);
    assert!(sketch(&doc).point(b).is_some());
}

#[test]
fn the_view_shows_the_tool_and_what_it_has_placed() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Arc));
    click(&mut doc, 1.0, 2.0);
    let state = doc.sketch_state().unwrap();
    let tool = state.tool.unwrap();
    assert_eq!((tool.tool, tool.placed), (Tool::Arc, &[at(1.0, 2.0)][..]));
    assert_eq!(state.selection, &BTreeSet::new());
    assert!(doc.keys().unwrap().drawing);
    let _ = doc.view_in(varde_view::Mode::Dark);
}

#[test]
fn entering_frames_the_curves_not_only_their_points() {
    // A circle's only point is its centre, but framing it takes all of it.
    let mut sketch = Sketch::default();
    let center = sketch.add_point(at(100.0, 0.0)).unwrap();
    let radius = 500.0;
    sketch
        .add_curve(Curve::Circle { center, radius }, false)
        .unwrap();
    let placement = OriginPlane::XY.placement();
    let camera = facing(varde_render::Projection::default(), placement, &sketch);
    assert!(camera.view_height() >= 2.0 * radius as f32);
    assert!(
        camera
            .target()
            .abs_diff_eq(glam::Vec3::new(100.0, 0.0, 0.0), 1e-3)
    );
}

#[test]
fn cancelling_a_drag_that_moved_nothing_stays_in_the_sketch() {
    let (mut doc, [_, b, ..]) = with_shapes();
    let before = sketch(&doc).clone();
    // Nothing dragged yet, as when the cursor never met the plane.
    doc.look(Look::CancelDrag);
    assert!(doc.sketch.is_some());
    doc.look(Look::DragGeometry {
        id: b,
        from: at(10.0, 0.0),
        to: at(10.0, 4.0),
    });
    doc.look(Look::CancelDrag);
    assert!(doc.sketch.is_some());
    assert_eq!(*shown(&doc), before);
    doc.update(Edit::DropGeometry);
    assert_eq!(*sketch(&doc), before);
}

#[test]
fn replacing_the_document_lets_go_of_ids_in_the_sketch() {
    let (mut doc, [a, b, ..]) = with_shapes();
    let feature = doc.sketch.as_ref().unwrap().feature;
    // The same ids, which in a document from elsewhere may name anything.
    let mut moved = sketch(&doc).clone();
    for point in &mut moved.points {
        point.at += at(100.0, 0.0);
    }
    let mut other = Editor::new(doc.editor.document().clone());
    other
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(moved),
        })
        .unwrap();
    let replacement = other.document().clone();

    let select_and_chain = |doc: &mut Answered| {
        doc.look(Look::ClickGeometry {
            hit: Some(a),
            add: false,
        });
        doc.look(Look::SelectTool(Tool::Line));
        click(doc, 0.0, 5.0);
        click(doc, 0.0, 8.0);
        assert!(drawing(doc).unwrap().chain.is_some());
        assert_eq!(selection(doc), [a]);
    };
    let let_go = |doc: &Doc| {
        assert_eq!(selection(doc), []);
        let drawing = drawing(doc).unwrap();
        assert_eq!((drawing.chain, drawing.placed.len()), (None, 0));
    };

    select_and_chain(&mut doc);
    doc.apply(Command::Replace(Box::new(replacement)));
    doc.sync();
    let_go(&doc);
    assert!(sketch(&doc).point(b).is_some());

    // Undo and redo across the replacement too.
    doc.look(Look::SelectTool(Tool::Line));
    select_and_chain(&mut doc);
    // Back past the chain's line, to the replacement, then before it.
    doc.update(Edit::Undo);
    doc.look(Look::ClickGeometry {
        hit: Some(a),
        add: false,
    });
    doc.update(Edit::Undo);
    let_go(&doc);
    doc.look(Look::ClickGeometry {
        hit: Some(a),
        add: false,
    });
    doc.update(Edit::Redo);
    let_go(&doc);
}

/// The profiles of the sketch shown, as the view has them.
fn found_profiles(doc: &Doc) -> Arc<varde_sketch::Profiles> {
    match doc.sketch_state().unwrap().profiles {
        Some(Ok(profiles)) => profiles.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_sketch_too_complex_for_profiles_is_shown_quickly() {
    let (mut doc, feature, _) = sketching();
    // Circles round one place, the innermost last: every pair meets.
    let mut sketch = Sketch::default();
    let center = sketch.add_point(at(0.0, 0.0)).unwrap();
    for k in (1..=3000).rev() {
        let radius = k as f64 * 0.01;
        sketch
            .add_curve(Curve::Circle { center, radius }, false)
            .unwrap();
    }
    doc.editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let started = std::time::Instant::now();
    doc.sync();
    let took = started.elapsed().as_secs_f64();
    // A tenth of a second or so released, some twenty times that
    // unoptimised and more on a loaded machine; it took seconds released.
    let bound = if cfg!(debug_assertions) { 30.0 } else { 1.0 };
    assert!(took < bound, "{took}");
    let state = doc.sketch_state().unwrap();
    assert!(
        matches!(state.profiles, Some(Err(_))),
        "{:?}",
        state.profiles
    );
}

#[test]
fn profiles_are_found_once_per_sketch_shown() {
    let (mut doc, [_, b, ..]) = with_shapes();
    let first = found_profiles(&doc);
    // The circle's disc; the line bounds nothing.
    assert_eq!(first.regions.len(), 1);
    // Looking around isn't another sketch.
    doc.look(Look::Zoom {
        factor: 2.0,
        x: 0.0,
        y: 0.0,
    });
    doc.look(Look::HoverItem(Some(b)));
    doc.look(Look::HoverItem(None));
    assert!(Arc::ptr_eq(&first, &found_profiles(&doc)));

    // A drag's steps are, and so is letting go of it.
    doc.look(Look::DragGeometry {
        id: b,
        from: at(10.0, 0.0),
        to: at(10.0, 3.0),
    });
    let dragged = found_profiles(&doc);
    assert!(!Arc::ptr_eq(&first, &dragged));
    let open_end = |profiles: &varde_sketch::Profiles, end| {
        profiles.open_ends.iter().any(|open| open.at == end)
    };
    assert!(open_end(&dragged, at(10.0, 3.0)));
    doc.look(Look::CancelDrag);
    let back = found_profiles(&doc);
    assert!(open_end(&back, at(10.0, 0.0)));

    // A shape waiting on the solver is shaded as it waits: the rectangle
    // and the disc.
    doc.look(Look::SelectTool(Tool::Rectangle));
    doc.doc.update(Edit::ToolClick(click_at(-10.0, -10.0)));
    doc.doc.update(Edit::ToolClick(click_at(-4.0, -6.0)));
    assert!(doc.proposing());
    assert_eq!(found_profiles(&doc).regions.len(), 2);
    let waiting = found_profiles(&doc);
    // Accepted as drawn, it's the same sketch.
    doc.lane.answer(&mut doc.doc);
    assert!(!doc.proposing());
    assert_eq!(sketch(&doc).curves.len(), 6);
    assert_eq!(found_profiles(&doc).regions.len(), 2);
    assert!(Arc::ptr_eq(&waiting, &found_profiles(&doc)));

    // Undone, it goes, and leaving the sketch lets go of them.
    doc.update(Edit::Undo);
    assert_eq!(found_profiles(&doc).regions.len(), 1);
    doc.look(Look::FinishSketch);
    assert!(doc.sketch_state().is_none());
}

#[test]
fn a_sketch_too_complex_for_profiles_says_so() {
    let (mut doc, feature, _) = sketching();
    // 230 lines across 230, each crossing cutting two.
    let lines = 230;
    assert!(2 * lines * lines > varde_sketch::MAX_SPLITS);
    let mut crossing = Sketch::default();
    for i in 0..lines {
        let along = i as f64;
        let reach = lines as f64;
        for (start, end) in [
            (at(along, -1.0), at(along, reach)),
            (at(-1.0, along), at(reach, along)),
        ] {
            let start = crossing.add_point(start).unwrap();
            let end = crossing.add_point(end).unwrap();
            crossing
                .add_curve(Curve::Line { start, end }, false)
                .unwrap();
        }
    }
    doc.editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(crossing),
        })
        .unwrap();
    doc.sync();
    let state = doc.sketch_state().unwrap();
    assert!(matches!(
        state.profiles,
        Some(Err(varde_sketch::TooComplex))
    ));
}

/// A sketch put on a face by a command isn't placed until regenerating
/// places it: it isn't opened for editing until then, and the status bar
/// says why.
#[test]
fn a_sketch_on_a_face_is_edited_once_placed() {
    let (mut doc, requests) = crate::tests::example();
    let xy = varde_document::Plane::Origin(OriginPlane::XY);
    doc.apply(doc.editor.document().add_sketch(xy));
    let document = doc.editor.document();
    let feature = document.features().last().unwrap().id;
    // The plate's top.
    let face = varde_document::FaceRef {
        body: document.bodies()[0].id,
        key: varde_document::FaceKey {
            feature: document.features()[1].id.get(),
            part: varde_document::PartKey::EndCap,
            instance: 0,
        },
        near: glam::DVec3::new(20.0, 0.0, 10.0),
    };
    doc.apply(Command::SetSketchPlane {
        feature,
        plane: varde_document::Plane::Face(face),
    });
    assert!(matches!(
        doc.editor.document().feature(feature).unwrap().kind,
        FeatureKind::Sketch {
            plane: varde_document::Plane::Face(_),
            ..
        }
    ));
    doc.look(Look::EditFeature(feature));
    assert!(doc.sketch.is_none());
    assert!(doc.sketch_state().is_none());
    assert_eq!(
        doc.notice.as_deref(),
        Some("Can't edit Sketch 2: it isn't placed yet")
    );
    doc.sync();
    answer(&mut doc, &requests);
    doc.look(Look::EditFeature(feature));
    assert_eq!(doc.notice, None);
    let placement = doc.sketch_state().unwrap().placement;
    assert_eq!(placement.origin, glam::DVec3::new(0.0, 0.0, 10.0));
    doc.look(Look::FinishSketch);

    doc.apply(Command::SetSketchPlane {
        feature,
        plane: varde_document::Plane::Origin(OriginPlane::XZ),
    });
    doc.look(Look::EditFeature(feature));
    assert!(doc.sketch.is_some());
    assert_eq!(
        doc.sketch_state().unwrap().placement,
        OriginPlane::XZ.placement()
    );
}

/// At 1280 px wide everything on the sketch toolbar shows whole, apart
/// and clear of Undo, Redo and the theme's button: the tools the mock's
/// bar has (the rest are on the rail, all with their keys), with a tool
/// in use, and while constraining or with splines selected.
#[test]
fn the_sketch_toolbar_fits_at_1280_px() {
    use crate::tests::{shown as laid_out, texts};
    use varde_view::Mode;
    let fits = |doc: &Doc| {
        let size = iced::Size::new(1280.0, 800.0);
        let mut on: Vec<_> = crate::tests::with_renderer(|renderer| {
            let mut ui = laid_out(doc.view_in(Mode::Light), size, renderer);
            texts(&mut ui, renderer)
        })
        .into_iter()
        .filter(|t| t.bounds.y < 40.0)
        .collect();
        on.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x));
        let mut end = 0.0;
        for t in &on {
            assert!(t.bounds.width > 2.0, "{} squeezed: {on:?}", t.text);
            assert!(t.bounds.x >= end, "{} overlaps: {on:?}", t.text);
            end = t.bounds.x + t.bounds.width;
        }
        // Undo, Redo, a rule and the theme's button (28, 28, 9 and 28
        // px, with the gaps) are right of them, all whole.
        assert!(end < 1280.0 - 120.0, "{end}: {on:?}");
        on.into_iter().map(|t| t.text).collect::<Vec<_>>()
    };
    let (mut doc, _, _) = sketching();
    let idle = fits(&doc);
    for label in ["Line", "Rectangle", "Dimension", "Constrain"] {
        assert!(idle.iter().any(|t| t == label), "{label}: {idle:?}");
    }
    // Fillet is on the rail, with its key: in use, its tag says so.
    assert!(!idle.iter().any(|t| t == "Fillet"));
    // Any tool in use, its tag beside the sketch's name.
    for tool in Tool::ALL {
        doc.look(Look::SelectTool(tool));
        let using = fits(&doc);
        assert!(
            using.iter().any(|t| t.starts_with(tool.label())),
            "the tag: {using:?}"
        );
        doc.look(Look::Escape);
    }

    // Two lines and a point of each: the most constraints offered.
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 0.0);
    click(&mut doc, 10.0, 1.0);
    doc.look(Look::Escape);
    click(&mut doc, 0.0, 5.0);
    click(&mut doc, 10.0, 7.0);
    doc.look(Look::Escape);
    doc.look(Look::Escape);
    let edited = sketch(&doc).clone();
    let [(a, b), (c, d)] = lines(&edited)[..] else {
        panic!("{edited:?}");
    };
    let [p, q] = (edited.curves.iter())
        .filter(|entry| matches!(entry.curve, Curve::Line { .. }))
        .map(|entry| entry.id)
        .collect::<Vec<_>>()[..]
    else {
        panic!("{edited:?}");
    };
    doc.look(Look::ToggleConstrain);
    let mut most = 0;
    for picked in [
        vec![p],
        vec![p, q],
        vec![a, b],
        vec![a, c],
        vec![a, p],
        vec![a, q],
        vec![a, b, c],
        vec![a, b, p],
        vec![a, c, p],
        vec![a, d, q],
        vec![a, b, c, d],
    ] {
        doc.look(Look::SelectBox {
            ids: picked,
            add: false,
        });
        let shown = fits(&doc);
        most = most.max(shown.len());
    }
    // The constraints that fit two lines' ends, and more, offered whole.
    assert!(most >= 6, "{most}");
    doc.look(Look::ToggleConstrain);

    // With a spline selected: its switch, handles and comb.
    doc.key(letter("n"));
    for (x, y) in [(0.0, 20.0), (5.0, 24.0), (10.0, 21.0)] {
        click(&mut doc, x, y);
    }
    doc.update(Edit::ToolClick(ToolClick {
        double: true,
        ..click_at(15.0, 20.0)
    }));
    doc.look(Look::Escape);
    let spline = (sketch(&doc).curves.iter())
        .find(|entry| matches!(entry.curve, Curve::Spline(_)))
        .unwrap()
        .id;
    doc.look(Look::ClickGeometry {
        hit: Some(spline),
        add: false,
    });
    let splines = fits(&doc);
    assert!(splines.iter().any(|t| t == "Convert"), "{splines:?}");
}
