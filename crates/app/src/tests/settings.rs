//! The settings page: opening and closing it, what it stores, Reset to
//! defaults, and what it leaves be underneath.

use iced::keyboard::{self, key};
use varde_document::LengthUnit;
use varde_expr::AngleUnit;
use varde_io::Request as IoRequest;
use varde_io::settings::{Settings as Stored, Theme};
use varde_render::Projection;
use varde_render::Rotation;
use varde_view::{
    DragAction, Look, Message as Ui, PivotClick, RotationPoint, Setting, SettingChange,
    SettingsPage, SettingsState, SettingsTab, ThemeChoice, ViewOptions, Welcome as WelcomeUi,
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
            ..Stored::default()
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

/// What the app does where nothing's set is what it did before these
/// were settings.
#[test]
fn the_newer_settings_default_to_the_app_as_it_was() {
    let varde = Varde::new();
    assert_eq!(varde.scale_factor(), 1.0);
    let options = varde.options;
    assert_eq!(options.shown(), varde_expr::Shown::default());
    assert_eq!(options.mouse, varde_view::MouseOptions::default());
    assert_eq!(
        crate::auto_save_pause(&options),
        Some(crate::doc::AUTO_SAVE_IDLE)
    );
    assert_eq!(usize::from(options.recent_designs), varde_io::recent::MAX);
    assert!(varde_view::changed_settings(options).is_empty());
}

fn set(varde: &mut Varde, setting: Setting) {
    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::Set(setting))));
}

/// Each of the page's own settings is stored as set, and comes back as
/// the next session starts; values the page doesn't offer are ignored.
#[test]
fn the_pages_own_settings_are_stored() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(varde_io::Response::SettingsLoaded {
        settings: Stored::default(),
    }));
    let _ = sent(&requests);
    let steps = [
        Setting::InterfaceScale(150),
        Setting::AngleUnit(AngleUnit::Rad),
        Setting::Decimals(Some(2)),
        Setting::LeftDrag(DragAction::Pan),
        Setting::MiddleDrag(DragAction::Zoom),
        Setting::RightDrag(DragAction::Nothing),
        Setting::Rotation(Rotation::OverTheTop),
        Setting::RotationPoint(RotationPoint::Pointer),
        Setting::PivotClick(PivotClick::MiddleDouble),
        Setting::WheelDownZoomsIn(true),
        Setting::ZoomToPointer(false),
        Setting::SketchSnap(false),
        Setting::AutoSave(30),
        Setting::RecentDesigns(25),
    ];
    for step in steps {
        set(&mut varde, step);
    }
    let written = written(&requests);
    assert_eq!(written.len(), steps.len());
    let last = *written.last().unwrap();
    use varde_io::settings as io;
    assert_eq!(
        last,
        Stored {
            interface_scale: io::InterfaceScale::Larger,
            angle_unit: io::AngleUnit::Rad,
            decimals: io::Decimals::Places(2),
            left_drag: io::Drag::Pan,
            middle_drag: io::Drag::Zoom,
            right_drag: io::Drag::Nothing,
            rotation: io::Rotation::OverTheTop,
            rotation_point: io::RotationPoint::Pointer,
            set_rotation_point: io::SetRotationPoint::MiddleDoubleClick,
            zoom_direction: io::ZoomDirection::WheelDown,
            zoom_to_pointer: false,
            sketch_snap: false,
            auto_save: io::AutoSave::Seconds30,
            recent_designs: io::RecentDesigns::TwentyFive,
            ..Stored::default()
        }
    );
    // Not offered, not taken nor stored.
    for odd in [
        Setting::InterfaceScale(110),
        Setting::InterfaceScale(0),
        Setting::Decimals(Some(7)),
        Setting::Decimals(Some(u8::MAX)),
        Setting::AutoSave(5),
        Setting::AutoSave(u16::MAX),
        Setting::RecentDesigns(0),
        Setting::RecentDesigns(1000),
    ] {
        let before = varde.options;
        set(&mut varde, odd);
        assert_eq!(varde.options, before, "{odd:?}");
    }
    assert!(sent(&requests).is_empty());

    let (mut next, _) = with_files();
    let _ = next.update(Message::Io(varde_io::Response::SettingsLoaded {
        settings: last,
    }));
    assert_eq!(next.options, varde.options);
    assert_eq!(next.scale_factor(), 1.5);
}

/// Set, the interface scale is the app's scale factor, and values are
/// shown as the units' settings say from then on.
#[test]
fn the_scale_and_the_values_shown_follow_the_settings() {
    let (mut varde, _) = with_files();
    set(&mut varde, Setting::InterfaceScale(90));
    assert!((varde.scale_factor() - 0.9).abs() < 1e-6);
    set(&mut varde, Setting::AngleUnit(AngleUnit::Rad));
    set(&mut varde, Setting::Decimals(Some(1)));
    let shown = varde_expr::Shown::current();
    assert_eq!(
        shown.format(std::f64::consts::PI / 8.0, Some(AngleUnit::Deg.into())),
        "0.4 rad"
    );
    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::AskReset)));
    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::Reset)));
    assert_eq!(varde_expr::Shown::current(), varde_expr::Shown::default());
    assert_eq!(varde.scale_factor(), 1.0);
}

/// Over the top, the document's camera turns on past straight down;
/// back to a turntable, it's set right side up and stops there again.
#[test]
fn the_rotation_set_turns_the_camera() {
    let (mut varde, _) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let orbit = |varde: &mut Varde| {
        for _ in 0..4 {
            let _ = varde.update(Message::Ui(Ui::Look(Look::Orbit {
                yaw: 0.0,
                pitch: 0.6,
                about: None,
            })));
        }
    };
    orbit(&mut varde);
    assert!(!document(&varde).camera.upside_down());
    let at_the_top = document(&varde).camera;
    orbit(&mut varde);
    assert_eq!(document(&varde).camera, at_the_top, "a turntable stops");

    set(&mut varde, Setting::Rotation(Rotation::OverTheTop));
    orbit(&mut varde);
    assert!(document(&varde).camera.upside_down());
    let eye = document(&varde).camera.eye();
    set(&mut varde, Setting::Rotation(Rotation::Turntable));
    let camera = document(&varde).camera;
    assert!(!camera.upside_down());
    assert!(camera.eye().abs_diff_eq(eye, 1e-3));
}

/// A clicked rotation point is forgotten once the point is set to be
/// another, and an orbit turns about the point a drag names.
#[test]
fn the_rotation_point_set_is_the_one_turned_about() {
    let (mut varde, _) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let pivot = glam::Vec3::new(5.0, 0.0, 0.0);
    let _ = varde.update(Message::Ui(Ui::Look(Look::SetPivot(Some(pivot)))));
    assert!(document(&varde).pivot_marker().is_some());
    set(&mut varde, Setting::RotationPoint(RotationPoint::Centre));
    assert!(document(&varde).pivot_marker().is_none());

    // Named by the drag, the point stays where it shows; unnamed, the
    // view turns about its middle, the target.
    let about = glam::Vec3::new(-3.0, 2.0, 1.0);
    let before = document(&varde).camera;
    let _ = varde.update(Message::Ui(Ui::Look(Look::Orbit {
        yaw: 0.3,
        pitch: 0.2,
        about: Some(about),
    })));
    let after = document(&varde).camera;
    let along = |camera: &varde_render::Camera| {
        let offset = about - camera.target();
        [camera.right(), camera.up()].map(|axis| offset.dot(axis))
    };
    let [a, b] = [along(&before), along(&after)];
    assert!((a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3);
    let target = after.target();
    let _ = varde.update(Message::Ui(Ui::Look(Look::Orbit {
        yaw: 0.3,
        pitch: 0.2,
        about: None,
    })));
    assert_eq!(document(&varde).camera.target(), target);
}

/// Fewer recent designs kept trims the list, stored so.
#[test]
fn fewer_recent_designs_trims_the_list() {
    let (mut varde, requests) = with_files();
    let entries = (0..30)
        .map(|i| varde_io::recent::Listed {
            entry: varde_io::recent::RecentFile {
                path: format!("/d/{i}.vrdp").into(),
                opened: varde_io::UnixSeconds(1000 - i),
            },
            available: true,
        })
        .collect();
    let _ = varde.update(Message::Io(varde_io::Response::RecentLoaded {
        entries,
        home: None,
    }));
    let _ = varde.update(Message::Io(varde_io::Response::SettingsLoaded {
        settings: Stored::default(),
    }));
    let _ = sent(&requests);
    set(&mut varde, Setting::RecentDesigns(10));
    assert_eq!(varde.files.recent.entries().len(), 10);
    let recent = sent(&requests)
        .into_iter()
        .find_map(|request| match request {
            IoRequest::WriteRecent { entries } => Some(entries),
            _ => None,
        })
        .expect("the list stored");
    assert_eq!(recent.len(), 10);
    assert_eq!(recent[0].path, std::path::PathBuf::from("/d/0.vrdp"));
    // Stored settings keeping fewer trim it as they arrive too.
    let _ = varde.update(Message::Io(varde_io::Response::SettingsLoaded {
        settings: Stored::default(),
    }));
    assert_eq!(varde.files.recent.entries().len(), 10, "none come back");
}

/// The Mouse tab's hints follow the buttons in the status bar.
#[test]
fn the_status_bar_hints_follow_the_mouse_settings() {
    let (mut varde, _) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let mut renderer = varde_view::probe::renderer();
    let size = iced::Size::new(1400.0, 800.0);
    let hints = |varde: &Varde, renderer: &mut iced::Renderer| {
        let view = document(varde).view(false, Mode::Light, varde.options, varde.files.offers());
        let mut ui = shown(view, size, renderer);
        texts(&mut ui, renderer)
            .into_iter()
            .map(|shown| shown.text)
            .collect::<Vec<_>>()
    };
    let before = hints(&varde, &mut renderer);
    assert!(before.iter().any(|text| text == "Drag to orbit"));
    assert!(before.iter().any(|text| text == "Click to set pivot"));
    set(&mut varde, Setting::LeftDrag(DragAction::Zoom));
    set(&mut varde, Setting::RotationPoint(RotationPoint::Pointer));
    let after = hints(&varde, &mut renderer);
    assert!(after.iter().any(|text| text == "Drag to zoom"), "{after:?}");
    assert!(!after.iter().any(|text| text == "Drag to orbit"));
    assert!(!after.iter().any(|text| text == "Click to set pivot"));
}
