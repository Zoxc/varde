//! The panic a session recorded, on the welcome screen: its note, the
//! whole of it in a dialog, and discarding it.

use varde_io::{Panic, UnixSeconds};

use super::damaged::{shows, welcome, welcome_texts};
use super::*;
use crate::doc::Dialog;

/// The welcome screen with a panic from 2 h ago loaded.
fn with_panic() -> (Varde, Rc<RefCell<Vec<IoRequest>>>, Panic) {
    let (mut varde, requests) = with_files();
    let panic = Panic {
        time: Some(UnixSeconds(when::now().0 - 2 * 3600)),
        version: Some("1.2.3".to_owned()),
        thread: Some("main".to_owned()),
        message: "index out of bounds\nsecond line".to_owned(),
        location: Some("src/lib.rs:1:2".to_owned()),
        backtrace: Some("0: here".to_owned()),
    };
    let _ = varde.update(Message::Io(IoResponse::PanicLoaded {
        panic: Some(panic.clone()),
    }));
    sent(&requests);
    (varde, requests, panic)
}

fn click(varde: &mut Varde, message: WelcomeUi) {
    let _ = varde.update(Message::Ui(Ui::Welcome(message)));
}

#[test]
fn the_panic_recorded_shows_until_discarded() {
    let (mut varde, requests, panic) = with_panic();
    let texts = welcome_texts(&varde);
    assert!(
        shows(&texts, "ran into an internal error · 2 h ago"),
        "{texts:?}"
    );
    // Its first line alone, till its details are asked for.
    assert!(shows(&texts, "index out of bounds"));
    assert!(!shows(&texts, "second line"));
    assert_eq!(welcome(&varde).dialog(), None);

    click(&mut varde, WelcomeUi::ShowPanic);
    assert_eq!(welcome(&varde).dialog(), Some(Dialog::Panic));
    let texts = welcome_texts(&varde);
    assert!(shows(&texts, &panic.report()), "{texts:?}");
    assert!(shows(&texts, "Copy"));
    // `Esc` closes it.
    let escape = press(keyboard::Key::Named(key::Named::Escape), Default::default());
    assert!(matches!(
        escape_key((welcome(&varde).dialog(), escape)),
        Some(Message::Ui(Ui::Welcome(WelcomeUi::ClosePanic)))
    ));
    click(&mut varde, WelcomeUi::ClosePanic);
    assert_eq!(welcome(&varde).dialog(), None);
    assert!(sent(&requests).is_empty());

    click(&mut varde, WelcomeUi::ShowPanic);
    click(&mut varde, WelcomeUi::DiscardPanic);
    assert_eq!(welcome(&varde).dialog(), None);
    assert!(matches!(
        &sent(&requests)[..],
        [IoRequest::DiscardPanic { panic: discarded }] if *discarded == panic
    ));
    assert!(!shows(&welcome_texts(&varde), "internal error"));
    // Nothing to show any more.
    click(&mut varde, WelcomeUi::ShowPanic);
    assert_eq!(welcome(&varde).dialog(), None);
}

/// One from moments ago was the session before this one.
#[test]
fn a_panic_just_now_was_last_session() {
    let (mut varde, _requests, mut panic) = with_panic();
    panic.time = Some(when::now());
    let _ = varde.update(Message::Io(IoResponse::PanicLoaded { panic: Some(panic) }));
    let texts = welcome_texts(&varde);
    assert!(shows(&texts, "internal error · Last session"), "{texts:?}");
}

#[test]
fn no_panic_recorded_shows_nothing() {
    let (mut varde, _requests) = with_files();
    let _ = varde.update(Message::Io(IoResponse::PanicLoaded { panic: None }));
    assert!(!shows(&welcome_texts(&varde), "internal error"));
    click(&mut varde, WelcomeUi::ShowPanic);
    assert_eq!(welcome(&varde).dialog(), None);
}

/// A long report scrolls in its dialog, which keeps its buttons on screen.
#[test]
fn a_long_report_scrolls() {
    let (mut varde, _requests, mut panic) = with_panic();
    panic.backtrace = Some((0..300).map(|n| format!("{n}: frame\n")).collect());
    let _ = varde.update(Message::Io(IoResponse::PanicLoaded {
        panic: Some(panic.clone()),
    }));
    click(&mut varde, WelcomeUi::ShowPanic);
    let mut renderer = varde_view::probe::renderer();
    let size = iced::Size::new(1280.0, 800.0);
    let view = welcome(&varde).view(&varde.files, Mode::Light, varde.options.theme);
    let mut ui = shown(view, size, &mut renderer);
    let texts = texts(&mut ui, &renderer);
    let report = (texts.iter())
        .find(|text| text.text == panic.report())
        .expect("the report shown");
    assert!(!report.whole() && !report.hidden(), "{report:?}");
    assert!(report.seen().height <= 360.0, "{report:?}");
    for button in ["Copy", "Close"] {
        let shown = (texts.iter().rev())
            .find(|text| text.text == button)
            .expect(button);
        assert!(
            shown.bounds.y + shown.bounds.height <= size.height,
            "{shown:?}"
        );
    }
}

/// A file dropped over the dialog showing the panic opens nothing, doesn't
/// light the drop zone, and is let go of.
#[test]
fn a_file_dropped_over_a_dialog_is_ignored() {
    let (mut varde, requests, _) = with_panic();
    click(&mut varde, WelcomeUi::ShowPanic);
    let _ = varde.update(Message::FileDragged(true));
    let Screen::Welcome(welcome) = &varde.screen else {
        panic!("not on the welcome screen");
    };
    assert!(!welcome.dragging());
    let dropped = picked(4, PickedFrom::Input);
    let _ = varde.update(Message::FileDropped(Some(Chosen::File(dropped.clone()))));
    assert!(sent(&requests).is_empty());
    assert!(varde.screen.doc().is_none());
    assert_eq!(
        crate::welcome::FORGOTTEN.with_borrow(Clone::clone),
        [dropped]
    );
}
