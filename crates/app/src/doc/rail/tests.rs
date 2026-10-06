use iced::keyboard;
use varde_document::OriginPlane;
use varde_view::{Edit, Look, RailLook, RailOpen, Tool};

use super::*;
use crate::tests::{key_in, untitled};

fn letter(d: &str) -> keyboard::Key {
    keyboard::Key::Character(d.into())
}

#[test]
fn hovering_a_head_peeks_at_its_list_till_the_cursor_leaves_it() {
    let mut rail = Rail::default();
    rail.update(RailLook::Hover(1, true), 3, |_| 3);
    assert_eq!(
        rail.state().map(|open| (open.set, open.held)),
        Some((1, false))
    );
    rail.update(RailLook::Hover(1, false), 3, |_| 3);
    assert_eq!(rail.open, None);

    // Entering the next head before leaving the last, as layers can send
    // them: the next stays open.
    rail.update(RailLook::Hover(0, true), 3, |_| 3);
    rail.update(RailLook::Hover(2, true), 3, |_| 3);
    rail.update(RailLook::Hover(0, false), 3, |_| 3);
    assert_eq!(rail.open, Some(2));
    // A head past the mode's sets opens nothing.
    rail.update(RailLook::Hover(3, true), 3, |_| 3);
    rail.update(RailLook::Open(7), 3, |_| 3);
    assert_eq!(rail.open, Some(2));
}

#[test]
fn a_list_opened_by_a_click_or_a_key_stays_as_the_cursor_leaves() {
    for opening in [RailLook::Open(1), RailLook::Toggle(1)] {
        let mut rail = Rail::default();
        rail.update(RailLook::Hover(1, true), 3, |_| 3);
        rail.update(opening, 3, |_| 3);
        assert!(rail.state().is_some_and(|open| open.held));
        rail.update(RailLook::Hover(1, false), 3, |_| 3);
        assert_eq!(rail.open, Some(1));
        // Another head doesn't change it.
        rail.update(RailLook::Hover(2, true), 3, |_| 3);
        rail.update(RailLook::Hover(2, false), 3, |_| 3);
        assert_eq!(rail.open, Some(1));
        // A click elsewhere closes it, and the next hover only peeks.
        rail.update(RailLook::Close, 3, |_| 3);
        assert_eq!(rail.open, None);
        rail.update(RailLook::Hover(0, true), 3, |_| 3);
        rail.update(RailLook::Hover(0, false), 3, |_| 3);
        assert_eq!(rail.open, None);
    }
    // The key on a peeked list holds it; again, closes it.
    let mut rail = Rail::default();
    rail.update(RailLook::Hover(0, true), 3, |_| 3);
    rail.update(RailLook::Toggle(0), 3, |_| 3);
    assert_eq!(rail.open, Some(0));
    rail.update(RailLook::Toggle(0), 3, |_| 3);
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
fn the_arrows_go_round_the_open_list_scrolling_it_to_the_row() {
    let mut rail = Rail::default();
    let update = |rail: &mut Rail, message| rail.update(message, 2, |set| [3, 5][set]);
    // Closed, the arrows do nothing.
    update(&mut rail, RailLook::Down);
    assert_eq!((rail.open, rail.take_scroll()), (None, None));

    update(&mut rail, RailLook::Toggle(1));
    assert_eq!(
        rail.state(),
        Some(RailOpen {
            set: 1,
            row: 0,
            held: true
        })
    );
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
    // Clicking the head of the set open keeps the row; another set
    // starts at its top.
    update(&mut rail, RailLook::Open(1));
    assert_eq!(rail.row, 3);
    update(&mut rail, RailLook::Open(0));
    assert_eq!(
        rail.state(),
        Some(RailOpen {
            set: 0,
            row: 0,
            held: true
        })
    );
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
