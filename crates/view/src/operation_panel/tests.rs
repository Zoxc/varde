use iced::widget::{Column, text};

use super::*;
use crate::testing::{Laid, Shown};

/// A panel whose body is `rows` lines of text, "Row 1" on, with a message.
fn panel_of(rows: usize) -> Element<'static, Message> {
    let body = Column::with_children((1..=rows).map(|k| text(format!("Row {k}")).size(12).into()))
        .spacing(4);
    operation_panel(Parts {
        title: "New thing",
        summary: Some(text("2 picked").size(12).into()),
        body: body.into(),
        message: Some(message_text("Why OK waits", theme::muted_text)),
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

#[test]
fn scrolled_to_its_end_the_body_shows_its_last_row() {
    use iced::advanced::widget::operation::scrollable::{RelativeOffset, snap_to};
    let height = 300.0;
    let mut laid = Laid::new(panel_of(60), Size::new(400.0, height));
    let mut snap = snap_to(
        PANEL_BODY,
        RelativeOffset {
            x: None,
            y: Some(1.0),
        },
    );
    laid.element.as_widget_mut().operate(
        &mut laid.tree,
        Layout::new(&laid.node),
        &laid.renderer,
        &mut snap,
    );
    let shown = laid.texts();
    let last = find(&shown, "Row 60");
    assert!(last.whole(), "{last:?}");
    let ok = find(&shown, "OK");
    assert!(last.bounds.y + last.bounds.height < ok.bounds.y);
}
