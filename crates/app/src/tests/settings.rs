//! The settings page: opening and closing it, what it stores, Reset to
//! defaults, and what it leaves be underneath.

use iced::keyboard::{self, key};
use varde_document::LengthUnit;
use varde_io::Request as IoRequest;
use varde_io::settings::{Settings as Stored, Theme};
use varde_render::Projection;
use varde_view::{
    Message as Ui, SettingChange, SettingsPage, SettingsState, SettingsTab, ThemeChoice,
    ViewOptions, Welcome as WelcomeUi,
};

use super::{document, press, sent, shown, texts, with_files};
use crate::keys::{document_key, welcome_key};
use crate::{Message, Mode, Varde, escape_key, reset_key};

fn escape() -> keyboard::Event {
    press(keyboard::Key::Named(key::Named::Escape), Default::default())
}

fn ctrl_comma() -> keyboard::Event {
    press(
        keyboard::Key::Character(",".into()),
        keyboard::Modifiers::COMMAND,
    )
}

fn letter(letter: &str) -> keyboard::Event {
    press(keyboard::Key::Character(letter.into()), Default::default())
}

/// What `Esc` sends on `varde`'s screen, with no prompt over it.
fn escaped(varde: &Varde) -> Option<Message> {
    escape_key(((None, false, varde.page_escape()), escape()))
}

/// What `event` sends as a shortcut of `varde`'s document.
fn doc_key(varde: &Varde, event: keyboard::Event) -> Option<Message> {
    document_key((varde.doc_keys(), event))
}

fn toggle(varde: &mut Varde) {
    let _ = varde.update(Message::Ui(Ui::ToggleSettings));
}

/// The settings written, in order.
fn written(requests: &std::cell::RefCell<Vec<IoRequest>>) -> Vec<Stored> {
    sent(requests)
        .into_iter()
        .map(|request| match request {
            IoRequest::WriteSettings { settings } => settings,
            request => panic!("not a settings write: {request:?}"),
        })
        .collect()
}

/// The gear's key opens the page and closes it, on the welcome screen and
/// in a document, and `Esc` closes it; it stays open across opening a
/// design, being the app's.
#[test]
fn ctrl_comma_toggles_the_page_and_escape_closes_it() {
    let (mut varde, _) = with_files();
    assert!(matches!(
        welcome_key((true, ctrl_comma())),
        Some(Message::Ui(Ui::ToggleSettings))
    ));
    // Not behind a prompt.
    assert!(welcome_key((false, ctrl_comma())).is_none());
    assert!(
        escaped(&varde).is_none_or(|message| !matches!(message, Message::Ui(Ui::ToggleSettings)))
    );
    toggle(&mut varde);
    assert!(varde.page.open);
    assert!(matches!(
        escaped(&varde),
        Some(Message::Ui(Ui::ToggleSettings))
    ));
    // Tab, which backs out as Esc does elsewhere, does nothing here.
    let tab = press(keyboard::Key::Named(key::Named::Tab), Default::default());
    assert!(escape_key(((None, false, varde.page_escape()), tab)).is_none());
    // A prompt over it takes Esc first.
    assert!(matches!(
        escape_key((
            (
                Some(crate::doc::Dialog::Unsaved),
                false,
                varde.page_escape()
            ),
            escape()
        )),
        Some(Message::Ui(Ui::File(_)))
    ));

    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    assert!(varde.screen.doc().is_some());
    assert!(varde.page.open, "the page is the app's");
    assert!(matches!(
        doc_key(&varde, ctrl_comma()),
        Some(Message::Ui(Ui::ToggleSettings))
    ));
    toggle(&mut varde);
    assert!(!varde.page.open);
    assert!(matches!(
        doc_key(&varde, ctrl_comma()),
        Some(Message::Ui(Ui::ToggleSettings))
    ));
    // Closed, Esc backs out of what's open in the document again.
    assert!(matches!(
        escaped(&varde),
        Some(Message::Ui(Ui::Look(varde_view::Look::Escape)))
    ));
}

/// While the page shows, the viewport's keys are off but undo, redo and
/// save stay; the measure tool open stays so underneath, and is back
/// when the page closes. The page takes the viewport's place, the status
/// bar going with it.
#[test]
fn the_page_leaves_what_is_open_and_turns_off_the_viewports_keys() {
    let (mut varde, _) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let measure = doc_key(&varde, letter("i"));
    assert!(
        matches!(
            measure,
            Some(Message::Ui(Ui::Look(varde_view::Look::StartMeasure)))
        ),
        "{measure:?}"
    );
    let _ = varde.update(measure.unwrap());
    assert!(document(&varde).measure_state().is_some());

    toggle(&mut varde);
    // No tool keys, nor Space.
    for key in ["i", "s", "x", " "] {
        assert!(doc_key(&varde, letter(key)).is_none(), "{key:?}");
    }
    let undo = press(
        keyboard::Key::Character("z".into()),
        keyboard::Modifiers::COMMAND,
    );
    let save_as = press(
        keyboard::Key::Character("s".into()),
        keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
    );
    // Undo and redo, with something to undo and redo.
    let keys = varde.doc_keys().unwrap().with_history(true, true);
    assert!(varde_view::claimed(
        varde_view::document_bindings(keys),
        &keyboard::Key::Character("z".into()),
        keyboard::Modifiers::COMMAND,
    ));
    assert!(matches!(
        doc_key(&varde, save_as),
        Some(Message::Ui(Ui::File(varde_view::File::SaveAs)))
    ));
    assert!(matches!(
        document_key((Some(keys), undo)),
        Some(Message::Ui(Ui::Edit(varde_view::Edit::Undo)))
    ));

    // In the viewport's place: the page shows, the status bar doesn't.
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let page = SettingsState {
        options: varde.options,
        tab: SettingsTab::View,
        resetting: false,
        kept_in: varde_view::KeptIn::Browser,
    };
    let view = document(&varde).view_with(
        false,
        Mode::Light,
        varde.options,
        varde.files.offers(),
        Some(page),
    );
    let mut ui = shown(view, size, &mut renderer);
    let on_screen = texts(&mut ui, &renderer);
    let has = |text: &str| on_screen.iter().any(|shown| shown.text == text);
    assert!(has("Reset to defaults") && has("Hidden edges") && has("Perspective"));
    // Neither the status bar's hints nor the measure tool's panel, both
    // over the viewport.
    let hint = "Drag to orbit";
    let panel = "Click a face, edge, point or body";
    assert!(!has(hint) && !has(panel), "{on_screen:?}");
    // The toolbar stays.
    assert!(has("Not saved") && has("Model"), "{on_screen:?}");
    drop(ui);

    let view = document(&varde).view(false, Mode::Light, varde.options, varde.files.offers());
    let mut ui = shown(view, size, &mut renderer);
    let without = texts(&mut ui, &renderer);
    for text in [hint, panel] {
        assert!(without.iter().any(|shown| shown.text == text), "{text}");
    }
    drop(ui);

    toggle(&mut varde);
    assert!(
        document(&varde).measure_state().is_some(),
        "closing cancels nothing"
    );
    assert!(doc_key(&varde, letter("x")).is_some());
}

/// Every setting chosen on the page or in the menu is stored, the stored
/// ones taken as they arrive.
#[test]
fn the_settings_are_stored() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(varde_io::Response::SettingsLoaded {
        settings: Stored::default(),
    }));
    let _ = sent(&requests);
    let steps = [
        Ui::SetTheme(ThemeChoice::Dark),
        Ui::SetProjection(Projection::Perspective),
        Ui::SetShading(varde_render::Shading::Flat),
        Ui::SetEdges(varde_view::Edges::Wireframe),
        Ui::ToggleHiddenEdges,
        Ui::ToggleNames,
        Ui::ToggleMouseHints,
        Ui::Settings(SettingsPage::SetLengthUnit(LengthUnit::Cm)),
    ];
    for step in steps {
        let _ = varde.update(Message::Ui(step));
    }
    let written = written(&requests);
    assert_eq!(written.len(), 8);
    let last = *written.last().unwrap();
    assert_eq!(
        last,
        Stored {
            theme: Theme::Dark,
            mouse_hints: false,
            hidden_edges: false,
            names: true,
            projection: varde_io::settings::Projection::Perspective,
            shading: varde_io::settings::Shading::Flat,
            edges: varde_io::settings::Edges::Wireframe,
            length_unit: LengthUnit::Cm,
        }
    );
    assert_eq!(crate::settings::stored_of(varde.options), last);

    // Stored, they come back as the next session starts.
    let (mut next, _) = with_files();
    let _ = next.update(Message::Io(varde_io::Response::SettingsLoaded {
        settings: last,
    }));
    assert_eq!(next.options, varde.options);
}

/// Reset to defaults asks first, listing what it changes; `Esc` cancels
/// it, `Enter` resets and stores the defaults. With nothing to reset it
/// does nothing.
#[test]
fn reset_lists_what_it_changes_and_resets() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(varde_io::Response::SettingsLoaded {
        settings: Stored::default(),
    }));
    toggle(&mut varde);
    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::AskReset)));
    assert!(!varde.page.resetting, "nothing to reset");

    let _ = varde.update(Message::Ui(Ui::SetTheme(ThemeChoice::Light)));
    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::SetLengthUnit(
        LengthUnit::Ft,
    ))));
    let _ = sent(&requests);
    assert_eq!(
        varde_view::changed_settings(varde.options),
        [
            SettingChange {
                name: "Theme",
                from: "Light",
                to: "System",
            },
            SettingChange {
                name: "Length unit",
                from: "ft",
                to: "mm",
            },
        ]
    );
    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::AskReset)));
    assert!(varde.page.resetting);
    // Its own keys only, while it asks.
    assert!(varde.doc_keys().is_none());
    let Some(cancel) = escaped(&varde) else {
        panic!("Esc cancels");
    };
    let _ = varde.update(cancel);
    assert!(!varde.page.resetting && varde.page.open);
    assert_eq!(varde.options.theme, ThemeChoice::Light);
    assert!(sent(&requests).is_empty());

    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::AskReset)));
    let enter = press(keyboard::Key::Named(key::Named::Enter), Default::default());
    let Some(reset) = reset_key(enter) else {
        panic!("Enter resets");
    };
    let _ = varde.update(reset);
    assert!(!varde.page.resetting && varde.page.open);
    assert_eq!(varde.options, ViewOptions::default());
    assert_eq!(written(&requests), [Stored::default()]);
    // A stray reset, not asked for, does nothing.
    let _ = varde.update(Message::Ui(Ui::SetTheme(ThemeChoice::Dark)));
    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::Reset)));
    assert_eq!(varde.options.theme, ThemeChoice::Dark);
}

/// A new design starts in the length unit and projection of the
/// settings; one opened keeps its own units.
#[test]
fn a_new_design_takes_the_settings() {
    let (mut varde, _) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    assert_eq!(document(&varde).editor.document().units(), LengthUnit::Mm);
    let _ = varde.update(Message::Ui(Ui::File(varde_view::File::CloseDocument)));
    assert!(varde.screen.doc().is_none());

    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::SetLengthUnit(
        LengthUnit::In,
    ))));
    let _ = varde.update(Message::Ui(Ui::SetProjection(Projection::Perspective)));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let doc = document(&varde);
    assert_eq!(doc.editor.document().units(), LengthUnit::In);
    assert_eq!(doc.camera.projection(), Projection::Perspective);
    // Changing the setting leaves the design open in its units.
    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::SetLengthUnit(
        LengthUnit::Cm,
    ))));
    assert_eq!(document(&varde).editor.document().units(), LengthUnit::In);
    // The projection is every document's.
    let _ = varde.update(Message::Ui(Ui::SetProjection(Projection::Orthographic)));
    assert_eq!(
        document(&varde).camera.projection(),
        Projection::Orthographic
    );
}
