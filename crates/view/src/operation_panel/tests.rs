use iced::widget::{Column, text};

use super::*;
use crate::probe::Shown;
use crate::testing::Laid;

/// A panel whose body is `rows` lines of text, "Row 1" on, with a message.
fn panel_of(rows: usize) -> Element<'static, Message> {
    panel_saying(rows, "Why OK waits")
}

/// A panel whose body is `rows` lines of text, "Row 1" on, with `message`.
fn panel_saying(rows: usize, message: &'static str) -> Element<'static, Message> {
    let body = Column::with_children((1..=rows).map(|k| text(format!("Row {k}")).size(12).into()))
        .spacing(4);
    operation_panel(Parts {
        title: "New thing",
        summary: Some(text("2 picked").size(12).into()),
        body: body.into(),
        message: Some(message_text(message, theme::muted_text)),
        ok: Some(Message::Look(crate::Look::Extrude(
            crate::ExtrudeLook::Cancel,
        ))),
        cancel: Message::Look(crate::Look::Extrude(crate::ExtrudeLook::Cancel)),
    })
}

fn find<'s>(shown: &'s [Shown], text: &str) -> &'s Shown {
    shown
        .iter()
        .find(|shown| shown.text == text)
        .unwrap_or_else(|| panic!("no {text:?} in {shown:?}"))
}

#[test]
fn a_short_body_takes_only_its_height() {
    let mut laid = Laid::new(panel_of(3), Size::new(400.0, 600.0));
    let size = laid.node.size();
    assert_eq!(size.width, PANEL_WIDTH);
    assert!(size.height < 300.0, "{size:?}");
    let shown = laid.texts();
    let ok = find(&shown, "OK");
    assert!(ok.bounds.y + ok.bounds.height <= size.height);
    // Nothing is scrolled out of sight.
    for shown in shown.iter().filter(|shown| shown.text.starts_with("Row")) {
        assert!(shown.whole(), "{shown:?}");
    }
}

#[test]
fn a_long_body_scrolls_between_the_header_and_the_buttons() {
    let height = 300.0;
    let mut laid = Laid::new(panel_of(60), Size::new(400.0, height));
    let size = laid.node.size();
    assert_eq!(size.height, height);
    let shown = laid.texts();
    let title = find(&shown, "New thing");
    let message = find(&shown, "Why OK waits");
    for button in ["OK", "Cancel"] {
        let button = find(&shown, button);
        // Whole, at the panel's bottom, under the message.
        assert!(button.bounds.height >= 12.0, "{button:?}");
        assert!(
            button.bounds.y + button.bounds.height <= height,
            "{button:?}"
        );
        assert!(button.bounds.y >= message.bounds.y + message.bounds.height);
    }
    let rows: Vec<&Shown> = shown
        .iter()
        .filter(|shown| shown.text.starts_with("Row"))
        .collect();
    assert_eq!(rows.len(), 60);
    // The rows show only between the title and the message, the first
    // ones whole, the last ones not at all.
    for row in &rows {
        if let Some(visible) = row.visible.filter(|visible| visible.height > 0.0) {
            assert!(visible.y >= title.bounds.y + title.bounds.height, "{row:?}");
            assert!(visible.y + visible.height <= message.bounds.y, "{row:?}");
        }
    }
    assert!(rows[0].whole(), "{:?}", rows[0]);
    assert!(rows[59].hidden(), "{:?}", rows[59]);
}

/// Scrolls the panel's body of `laid` to its end, as a part of it.
fn to_the_end(laid: &mut Laid<'_>) {
    use iced::advanced::widget::operation::scrollable::{RelativeOffset, snap_to};
    let snap = snap_to(
        PANEL_BODY,
        RelativeOffset {
            x: None,
            y: Some(1.0),
        },
    );
    run(laid, snap);
}

/// Scrolls the panel's body of `laid` `y` pixels down, as the wheel does,
/// past its end if that's further.
fn scrolled_by(laid: &mut Laid<'_>, y: f32) {
    use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, scroll_to};
    run(
        laid,
        scroll_to(
            PANEL_BODY,
            AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        ),
    );
}

fn run(laid: &mut Laid<'_>, mut operation: impl Operation) {
    laid.element.as_widget_mut().operate(
        &mut laid.tree,
        Layout::new(&laid.node),
        &laid.renderer,
        &mut operation,
    );
}

#[test]
fn scrolled_to_its_end_the_body_shows_its_last_row() {
    let height = 300.0;
    let mut laid = Laid::new(panel_of(60), Size::new(400.0, height));
    to_the_end(&mut laid);
    let shown = laid.texts();
    let last = find(&shown, "Row 60");
    assert!(last.whole(), "{last:?}");
    let ok = find(&shown, "OK");
    assert!(last.bounds.y + last.bounds.height < ok.bounds.y);
}

#[test]
fn rows_going_after_a_scroll_leave_no_empty_room() {
    let max = Size::new(400.0, 300.0);
    let mut laid = Laid::new(panel_of(60), max);
    scrolled_by(&mut laid, 10_000.0);
    // Fewer rows than fit: the panel shrinks to them, all shown whole.
    laid.replace(panel_of(3), max);
    assert!(laid.node.size().height < 200.0, "{:?}", laid.node.size());
    let shown = laid.texts();
    for row in ["Row 1", "Row 3"] {
        let row = find(&shown, row);
        assert!(row.whole(), "{row:?}");
    }
    // Still more than fit: the last row ends at the body's bottom, not
    // above an empty stretch.
    laid.replace(panel_of(60), max);
    scrolled_by(&mut laid, 10_000.0);
    laid.replace(panel_of(40), max);
    let shown = laid.texts();
    let last = find(&shown, "Row 40");
    let message = find(&shown, "Why OK waits");
    assert!(last.whole(), "{last:?}");
    assert!(
        message.bounds.y - (last.bounds.y + last.bounds.height) < 40.0,
        "{last:?} {message:?}"
    );
}

#[test]
fn a_long_message_scrolls_above_the_buttons() {
    let long: &'static str = "a word or two of what went wrong ".repeat(20).leak();
    let height = 400.0;
    let mut laid = Laid::new(panel_saying(60, long), Size::new(400.0, height));
    let shown = laid.texts();
    let message = shown
        .iter()
        .find(|shown| shown.text == long)
        .expect("the message");
    // Taller than it may show, so it's in a scrollable of its own, which
    // shows at most `MESSAGE_HEIGHT` of it.
    assert!(message.bounds.height > MESSAGE_HEIGHT, "{message:?}");
    let seen = message.visible.expect("the message scrolls");
    assert!(seen.height <= MESSAGE_HEIGHT, "{message:?}");
    assert!(seen.height >= MESSAGE_HEIGHT / 2.0, "{message:?}");
    for button in ["OK", "Cancel"] {
        let button = find(&shown, button);
        assert!(button.whole() && button.bounds.height >= 12.0, "{button:?}");
        assert!(button.bounds.y >= seen.y + seen.height, "{button:?}");
        assert!(button.bounds.y + button.bounds.height <= height);
    }
}

#[test]
fn placed_starts_below_the_camera_controls_unless_the_viewport_is_short() {
    for (height, top) in [
        (800.0, PANEL_TOP),
        (PANEL_TOP + PANEL_BOTTOM + PANEL_ROOM, PANEL_TOP),
        (300.0, 300.0 - PANEL_BOTTOM - PANEL_ROOM),
        (150.0, PANEL_MARGIN),
        (0.0, PANEL_MARGIN),
    ] {
        let max = Size::new(800.0, height);
        let mut laid = Laid::new(placed(panel_of(60)), max);
        assert_eq!(laid.node.size(), max);
        let panel = laid.node.children()[0].bounds();
        assert_eq!(panel.y, top, "{height}");
        assert_eq!(panel.x + panel.width, 800.0 - PANEL_MARGIN, "{height}");
        // A long body takes all the height down to the status bar's room.
        if height >= 300.0 {
            assert_eq!(panel.y + panel.height, height - PANEL_BOTTOM, "{height}");
            let shown = laid.texts();
            let ok = find(&shown, "OK");
            assert!(ok.bounds.height >= 12.0, "{height}: {ok:?}");
        }
    }
    // A short body stays short, at the usual top.
    let mut laid = Laid::new(placed(panel_of(3)), Size::new(800.0, 800.0));
    let panel = laid.node.children()[0].bounds();
    assert_eq!(panel.y, PANEL_TOP);
    assert!(panel.height < 300.0, "{panel:?}");
    let _ = laid.texts();
}

#[test]
fn without_room_for_the_header_too_the_buttons_keep_theirs() {
    let height = 70.0;
    let mut laid = Laid::new(panel_of(60), Size::new(400.0, height));
    assert_eq!(laid.node.size().height, height);
    let shown = laid.texts();
    for button in ["OK", "Cancel"] {
        let button = find(&shown, button);
        assert!(button.bounds.height >= 12.0, "{button:?}");
        assert!(
            button.bounds.y + button.bounds.height <= height,
            "{button:?}"
        );
    }
}

#[test]
fn the_body_s_scroller_is_faint() {
    let size = iced::Size::new(400, 300);
    let mut laid = Laid::new(panel_of(60), Size::new(400.0, 300.0));
    let pixels = laid.pixels(size);
    let faint = crate::Mode::Light.palette().faint;
    let want = faint.into_rgba8();
    // Down the middle of the scroller, in the right padding.
    let x = (PANEL_WIDTH - SIDE / 2.0) as usize;
    let near =
        |at: usize| (0..3).all(|k| (i32::from(pixels[at + k]) - i32::from(want[k])).abs() <= 2);
    let hits = (0..size.height as usize)
        .filter(|&y| near((y * size.width as usize + x) * 4))
        .count();
    assert!(hits >= 10, "{hits} faint pixels");
}
