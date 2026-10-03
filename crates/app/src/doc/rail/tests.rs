use iced::keyboard;
use iced::time::Instant;
use varde_document::OriginPlane;
use varde_view::{Edit, Look, RailLook, RailOpen, RailSpot, Tool};

use super::*;
use crate::tests::{key_in, untitled};

fn letter(d: &str) -> keyboard::Key {
    keyboard::Key::Character(d.into())
}

#[test]
fn leaving_the_head_closes_the_list_after_a_moment_unless_the_cursor_reaches_the_list() {
    let mut rail = Rail::default();
    let start = Instant::now();
    rail.update(RailLook::Hover(RailSpot::Head(1), true), 2, |_| 3, start);
    assert_eq!(rail.open, Some(1));
    assert!(!rail.closing());

    // Off the head, on its way to the list: closing, but not yet.
    rail.update(RailLook::Hover(RailSpot::Head(1), false), 2, |_| 3, start);
    assert!(rail.closing());
    rail.tick(start + RAIL_CLOSE_DELAY / 2);
    assert_eq!(rail.open, Some(1));
    rail.update(RailLook::Hover(RailSpot::List, true), 2, |_| 3, start);
    rail.tick(start + 2 * RAIL_CLOSE_DELAY);
    assert_eq!(rail.open, Some(1));
    assert!(!rail.closing());

    // Entering the list before leaving the head, as layers can send them.
    let mut rail = Rail::default();
    rail.update(RailLook::Hover(RailSpot::Head(0), true), 2, |_| 3, start);
    rail.update(RailLook::Hover(RailSpot::List, true), 2, |_| 3, start);
    rail.update(RailLook::Hover(RailSpot::Head(0), false), 2, |_| 3, start);
    assert!(!rail.closing());
    rail.tick(start + 2 * RAIL_CLOSE_DELAY);
    assert_eq!(rail.open, Some(0));

    // Off the list, to nothing: it closes once the delay is up.
    rail.update(RailLook::Hover(RailSpot::List, false), 2, |_| 3, start);
    rail.tick(start + RAIL_CLOSE_DELAY - Duration::from_millis(1));
    assert_eq!(rail.open, Some(0));
    rail.tick(start + RAIL_CLOSE_DELAY);
    assert_eq!(rail.open, None);
    assert!(!rail.closing());
}

#[test]
fn a_tool_on_a_card_closes_the_list_at_once_and_another_head_opens_its_own() {
    let mut rail = Rail::default();
    let now = Instant::now();
    rail.update(RailLook::Hover(RailSpot::Head(0), true), 4, |_| 3, now);
    rail.update(RailLook::Hover(RailSpot::Head(0), false), 4, |_| 3, now);
    rail.update(RailLook::Hover(RailSpot::Tool, true), 4, |_| 3, now);
    assert_eq!(rail.open, None);
    rail.update(RailLook::Hover(RailSpot::Tool, false), 4, |_| 3, now);
    assert!(!rail.closing());
    rail.update(RailLook::Hover(RailSpot::Head(2), true), 4, |_| 3, now);
    assert_eq!(rail.open, Some(2));
    // A head past the mode's sets opens nothing.
    rail.update(RailLook::Hover(RailSpot::Head(4), true), 4, |_| 3, now);
    rail.update(RailLook::Open(7), 4, |_| 3, now);
    assert_eq!(rail.open, Some(2));
    rail.update(RailLook::Close, 4, |_| 3, now);
    assert_eq!(rail.open, None);
}

#[test]
fn set_keys_open_and_close_sets_and_picking_or_escape_closes_them() {
    let mut doc = untitled();
    key_in(&mut doc, letter("q"));
    assert_eq!(doc.rail.open, Some(0));
    key_in(&mut doc, letter("q"));
    assert_eq!(doc.rail.open, None);
    // The model has four sets: Create, Modify, Transform and Inspect.
    key_in(&mut doc, letter("w"));
    assert_eq!(doc.rail.open, Some(1));
    key_in(&mut doc, letter("t"));
    assert_eq!(doc.rail.open, Some(1), "T opens no fifth set");
    key_in(&mut doc, letter("e"));
    assert_eq!(doc.rail.open, Some(2));
    key_in(&mut doc, letter("r"));
    assert_eq!(doc.rail.open, Some(3));
    // I in the open Inspect set starts the measure tool, and closes it.
    key_in(&mut doc, keyboard::Key::Character("i".into()));
    assert!(doc.measure.is_some());
    assert_eq!(doc.rail.open, None);
    doc.look(Look::Escape);
    assert!(doc.measure.is_none());

    // S in the open Create set picks the plane, and closes it.
    key_in(&mut doc, letter("q"));
    key_in(&mut doc, keyboard::Key::Character("s".into()));
    assert!(doc.picking_plane.is_some());
    assert_eq!(doc.rail.open, None);

    // Esc closes the list before anything else.
    key_in(&mut doc, letter("q"));
    doc.look(Look::Escape);
    assert_eq!(doc.rail.open, None);
    assert!(doc.picking_plane.is_some());
}

#[test]
fn entering_or_leaving_a_sketch_closes_the_list_and_its_letters_pick_its_tools() {
    let mut doc = untitled();
    doc.look(Look::Rail(RailLook::Open(0)));
    doc.update(Edit::PlanePicked(OriginPlane::XY));
    assert!(doc.sketch.is_some());
    assert_eq!(doc.rail.open, None);

    // Modify, then J for Extend.
    key_in(&mut doc, letter("w"));
    assert_eq!(doc.rail.open, Some(1));
    key_in(&mut doc, keyboard::Key::Character("j".into()));
    let tool = doc
        .sketch
        .as_ref()
        .and_then(|s| s.tool.as_ref())
        .map(|t| t.tool);
    assert_eq!(tool, Some(Tool::Extend));
    assert_eq!(doc.rail.open, None);

    // The sketch has four sets.
    key_in(&mut doc, letter("r"));
    assert_eq!(doc.rail.open, Some(3));
    doc.look(Look::FinishSketch);
    assert_eq!(doc.rail.open, None);
}

#[test]
fn a_list_waiting_to_close_takes_frames() {
    let mut doc = untitled();
    assert!(!doc.animating());
    doc.look(Look::Rail(RailLook::Hover(RailSpot::Head(0), true)));
    doc.look(Look::Rail(RailLook::Hover(RailSpot::Head(0), false)));
    assert!(doc.animating());
    doc.rail.tick(Instant::now() + RAIL_CLOSE_DELAY);
    assert!(!doc.animating());
    assert_eq!(doc.rail.open, None);
}

#[test]
fn the_arrows_go_round_the_open_list_scrolling_it_to_the_row() {
    let mut rail = Rail::default();
    let now = Instant::now();
    let update = |rail: &mut Rail, message| rail.update(message, 2, |set| [3, 5][set], now);
    // Closed, the arrows do nothing.
    update(&mut rail, RailLook::Down);
    assert_eq!((rail.open, rail.take_scroll()), (None, None));

    update(&mut rail, RailLook::Toggle(1));
    assert_eq!(rail.state(), Some(RailOpen { set: 1, row: 0 }));
    assert_eq!(rail.take_scroll(), Some(0.0));
    update(&mut rail, RailLook::Up);
    assert_eq!(rail.row, 4);
    assert_eq!(rail.take_scroll(), Some(1.0));
    update(&mut rail, RailLook::Down);
    update(&mut rail, RailLook::Down);
    assert_eq!(rail.row, 1);
    assert_eq!(rail.take_scroll(), Some(0.25));
    assert_eq!(rail.take_scroll(), None);
    // Pointing at a row puts the keys there, past the end does nothing.
    update(&mut rail, RailLook::Row(3));
    update(&mut rail, RailLook::Row(5));
    assert_eq!(rail.row, 3);
    // Hovering the head of the set open keeps the row; another set
    // starts at its top.
    update(&mut rail, RailLook::Hover(RailSpot::Head(1), true));
    assert_eq!(rail.row, 3);
    update(&mut rail, RailLook::Hover(RailSpot::Head(0), true));
    assert_eq!(rail.state(), Some(RailOpen { set: 0, row: 0 }));
    update(&mut rail, RailLook::Up);
    assert_eq!(rail.row, 2);
}

#[test]
fn enter_picks_the_row_the_keys_are_on() {
    let mut doc = untitled();
    key_in(&mut doc, letter("q"));
    key_in(
        &mut doc,
        keyboard::Key::Named(keyboard::key::Named::ArrowDown),
    );
    key_in(
        &mut doc,
        keyboard::Key::Named(keyboard::key::Named::ArrowUp),
    );
    key_in(&mut doc, keyboard::Key::Named(keyboard::key::Named::Enter));
    assert!(doc.picking_plane.is_some());
    assert_eq!(doc.rail.open, None);
}
