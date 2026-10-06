use iced::advanced::clipboard;
use iced::widget::Space;

use super::*;
use crate::Look;
use crate::testing::Laid;

/// The window the tests lay out in.
const WINDOW: Size = Size::new(400.0, 300.0);

/// A 100 by 20 pixel area right-clicked for a menu 80 by 50, shown if
/// `open`.
fn area(open: bool) -> ContextMenu<'static> {
    let menu = Space::new().width(80).height(50);
    ContextMenu::new(
        Space::new().width(100).height(20),
        open.then(|| menu.into()),
        Message::Look(Look::CloseFileMenu),
        Message::Look(Look::CloseViewMenu),
    )
}

fn press(button: mouse::Button) -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(button))
}

/// What `laid` sends for `event` with the cursor `at`, and whether it
/// takes the event.
fn update(laid: &mut Laid<'_>, event: Event, at: Point) -> (Vec<Message>, bool) {
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    laid.element.as_widget_mut().update(
        &mut laid.tree,
        &event,
        Layout::new(&laid.node),
        mouse::Cursor::Available(at),
        &laid.renderer,
        &mut clipboard::Null,
        &mut shell,
        &Rectangle::with_size(WINDOW),
    );
    let captured = shell.is_event_captured();
    (messages, captured)
}

/// The open menu's overlay, laid out over [`WINDOW`] with the content
/// moved by `translation` (a scrolled list's), and what it sends for
/// `event` with the cursor `at`: the menu's bounds, the messages and
/// whether it takes the event.
fn overlay(
    laid: &mut Laid<'_>,
    translation: Vector,
    event: Event,
    at: Point,
) -> (Rectangle, Vec<Message>, bool) {
    let mut overlay = laid
        .element
        .as_widget_mut()
        .overlay(
            &mut laid.tree,
            Layout::new(&laid.node),
            &laid.renderer,
            &Rectangle::with_size(WINDOW),
            translation,
        )
        .expect("the menu's overlay");
    let overlay = overlay.as_overlay_mut();
    let node = overlay.layout(&laid.renderer, WINDOW);
    let layout = Layout::new(&node);
    // The group's, then the window's, then the menu's.
    let menu = layout.children().next().unwrap().children().next().unwrap();
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    overlay.update(
        &event,
        layout,
        mouse::Cursor::Available(at),
        &laid.renderer,
        &mut clipboard::Null,
        &mut shell,
    );
    let captured = shell.is_event_captured();
    (menu.bounds(), messages, captured)
}

#[test]
fn a_right_click_on_it_asks_for_the_menu() {
    let mut laid = Laid::new(area(false), WINDOW);
    let (sent, captured) = update(
        &mut laid,
        press(mouse::Button::Right),
        Point::new(30.0, 10.0),
    );
    assert!(matches!(sent[..], [Message::Look(Look::CloseFileMenu)]));
    assert!(captured);
    // Not a left click, nor one off it.
    let (sent, _) = update(
        &mut laid,
        press(mouse::Button::Left),
        Point::new(30.0, 10.0),
    );
    assert!(sent.is_empty());
    let (sent, _) = update(
        &mut laid,
        press(mouse::Button::Right),
        Point::new(130.0, 10.0),
    );
    assert!(sent.is_empty());
    // Nothing shows until it's open.
    let overlay = laid.element.as_widget_mut().overlay(
        &mut laid.tree,
        Layout::new(&laid.node),
        &laid.renderer,
        &Rectangle::with_size(WINDOW),
        Vector::ZERO,
    );
    assert!(overlay.is_none());
}

#[test]
fn the_menu_shows_where_it_was_clicked_within_the_window() {
    let mut laid = Laid::new(area(false), WINDOW);
    update(
        &mut laid,
        press(mouse::Button::Right),
        Point::new(30.0, 10.0),
    );
    laid.replace(area(true), WINDOW);
    let nowhere = Event::Mouse(mouse::Event::CursorLeft);
    let (bounds, ..) = overlay(&mut laid, Vector::ZERO, nowhere.clone(), Point::ORIGIN);
    assert_eq!(
        bounds,
        Rectangle::new(Point::new(30.0, 10.0), Size::new(80.0, 50.0))
    );
    // In a list scrolled 5 pixels down, the click was 5 pixels higher in
    // the window.
    let scrolled = Vector::new(0.0, -5.0);
    let (bounds, ..) = overlay(&mut laid, scrolled, nowhere.clone(), Point::ORIGIN);
    assert_eq!(bounds.position(), Point::new(30.0, 5.0));
    // Near the window's bottom right, it's moved to fit.
    let far = Vector::new(350.0, 280.0);
    let (bounds, ..) = overlay(&mut laid, far, nowhere, Point::ORIGIN);
    assert_eq!(bounds.position(), Point::new(320.0, 250.0));
}

#[test]
fn a_press_off_the_menu_closes_it_and_none_reaches_what_s_under_it() {
    let mut laid = Laid::new(area(false), WINDOW);
    update(
        &mut laid,
        press(mouse::Button::Right),
        Point::new(30.0, 10.0),
    );
    laid.replace(area(true), WINDOW);
    for button in [mouse::Button::Left, mouse::Button::Right] {
        let (_, sent, captured) = overlay(
            &mut laid,
            Vector::ZERO,
            press(button),
            Point::new(300.0, 200.0),
        );
        assert!(matches!(sent[..], [Message::Look(Look::CloseViewMenu)]));
        assert!(captured);
    }
    let (_, sent, captured) = overlay(
        &mut laid,
        Vector::ZERO,
        press(mouse::Button::Left),
        Point::new(50.0, 30.0),
    );
    assert!(sent.is_empty());
    assert!(captured);
}

#[test]
fn a_menu_opened_otherwise_shows_below_it() {
    let nowhere = Event::Mouse(mouse::Event::CursorLeft);
    let mut laid = Laid::new(area(true), WINDOW);
    let (bounds, ..) = overlay(&mut laid, Vector::ZERO, nowhere.clone(), Point::ORIGIN);
    assert_eq!(bounds.position(), Point::new(0.0, 20.0));
    // Right-clicked, then closed: opened again from the keyboard, it's
    // below the content again.
    laid.replace(area(false), WINDOW);
    update(
        &mut laid,
        press(mouse::Button::Right),
        Point::new(30.0, 10.0),
    );
    laid.replace(area(true), WINDOW);
    let (bounds, ..) = overlay(&mut laid, Vector::ZERO, nowhere.clone(), Point::ORIGIN);
    assert_eq!(bounds.position(), Point::new(30.0, 10.0));
    laid.replace(area(false), WINDOW);
    laid.replace(area(true), WINDOW);
    let (bounds, ..) = overlay(&mut laid, Vector::ZERO, nowhere, Point::ORIGIN);
    assert_eq!(bounds.position(), Point::new(0.0, 20.0));
}
