use iced::widget::text;

use super::*;
use crate::chrome::mouse_hint;
use crate::icons::MouseButton;
use crate::probe::Shown;
use crate::testing::Laid;

/// The texts of `status` laid out over a screen `width` × 400.
fn laid(status: Status<'_>, width: f32) -> Vec<Shown> {
    Laid::new(status_bar(status), Size::new(width, 400.0)).texts()
}

fn find<'a>(shown: &'a [Shown], text: &str) -> &'a Shown {
    shown
        .iter()
        .find(|shown| shown.text == text)
        .unwrap_or_else(|| panic!("no {text:?} in {shown:?}"))
}

/// A status of `info` and the hints "Edit", "Delete" and "Pan", the last
/// the mouse's, with a feature selected and the view options menu's
/// button.
fn status(info: &str, mouse_hints: bool) -> Status<'_> {
    Status {
        selection: Some(text("Extrude 1").size(12).into()),
        info: Some(text(info).size(12).into()),
        hints: vec![
            key_hint(Shortcut::ENTER, "Edit"),
            key_hint(Shortcut::DELETE, "Delete"),
            mouse_hint(MouseButton::Right, "Pan"),
        ],
        mouse_hints,
        view_menu: Some(false),
    }
}

#[test]
fn the_bar_floats_at_the_bottom_right_in_one_line() {
    let shown = laid(status("Regenerating…", true), 1000.0);
    let pan = find(&shown, "Delete");
    // Right of everything else, and in the bar's height above its margin.
    assert!(pan.bounds.x + pan.bounds.width < 1000.0 - RIGHT, "{pan:?}");
    assert!(pan.bounds.y >= 400.0 - STATUS_BAR_ROOM, "{pan:?}");
    assert!(
        pan.bounds.y + pan.bounds.height <= 400.0 - BOTTOM,
        "{pan:?}"
    );
    let order: Vec<_> = [
        "Extrude 1",
        "Space",
        "Clear",
        "Regenerating…",
        "Enter",
        "Edit",
        "Delete",
    ]
    .map(|text| find(&shown, text).bounds.x)
    .into();
    assert!(order.is_sorted(), "{shown:?}");
    // Floating, not across the screen.
    assert!(find(&shown, "Extrude 1").bounds.x > 400.0, "{shown:?}");
}

/// The mouse's hints make room for the selection and what's going on:
/// they show only with neither.
#[test]
fn mouse_hints_show_only_with_no_selection_nor_status() {
    let pan = |selection: bool, info: bool| {
        let status = Status {
            selection: selection.then(|| text("Extrude 1").size(12).into()),
            info: info.then(|| text("Saving…").size(12).into()),
            ..status("", true)
        };
        laid(status, 1000.0).iter().any(|shown| shown.text == "Pan")
    };
    assert!(pan(false, false));
    assert!(!pan(true, false));
    assert!(!pan(false, true));
    assert!(!pan(true, true));
}

/// The step a click takes in the tool or operation open shows beside
/// what's going on and the selection, with the mouse's hints on; the
/// mouse's other hints make room.
#[test]
fn the_step_shows_beside_the_status() {
    let shows = |mouse_hints: bool| {
        let status = Status {
            hints: vec![
                crate::chrome::step_hint(MouseButton::Left, "Pick edges"),
                mouse_hint(MouseButton::Right, "Pan"),
                key_hint(Shortcut::ESCAPE, "Cancel"),
            ],
            ..status("Chamfer 1", mouse_hints)
        };
        let shown = laid(status, 1000.0);
        ["Pick edges", "Pan", "Cancel"].map(|text| shown.iter().any(|shown| shown.text == text))
    };
    assert_eq!(shows(true), [true, false, true]);
    assert_eq!(shows(false), [false, false, true]);
}

#[test]
fn without_mouse_hints_the_keys_stay() {
    let shown = laid(status("Saving…", false), 1000.0);
    assert!(!shown.iter().any(|shown| shown.text == "Pan"), "{shown:?}");
    find(&shown, "Edit");
}

#[test]
fn a_long_status_is_cut_short_and_the_hints_stay_whole() {
    let long = "Couldn't regenerate: the kernel ran out of room splitting the faces of a \
                body with very many curved faces; try a coarser tolerance";
    for width in [1000.0, 600.0, 300.0] {
        let shown = laid(status(long, true), width);
        for hint in ["Edit", "Delete"] {
            let hint = find(&shown, hint);
            let seen = hint.seen();
            assert!(
                seen.x >= 0.0 && seen.x + seen.width <= width,
                "{width}: {hint:?}"
            );
            assert!(hint.whole(), "{width}: {hint:?}");
        }
        // The status gives way first, cut where the hints start.
        let info = find(&shown, long);
        let edit = find(&shown, "Enter");
        assert!(
            info.seen().x + info.seen().width <= edit.bounds.x,
            "{width}: {info:?}"
        );
    }
    // Then the selection, which keeps its room before the status does.
    let shown = laid(status(long, true), 1000.0);
    let selected = find(&shown, "Extrude 1");
    assert!(selected.bounds.width > 40.0, "{selected:?}");
    let space = find(&shown, "Space");
    assert!(
        selected.bounds.x + selected.bounds.width <= space.bounds.x,
        "{shown:?}"
    );
    assert!(space.bounds.x < find(&shown, long).bounds.x, "{shown:?}");
}
