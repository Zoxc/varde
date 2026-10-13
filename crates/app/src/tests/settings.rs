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
/// in a document, and `Esc` closes it; New design closes it.
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

    // A new design closes it, so the design shows.
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    assert!(varde.screen.doc().is_some());
    assert!(!varde.page.open);
    assert!(matches!(
        doc_key(&varde, ctrl_comma()),
        Some(Message::Ui(Ui::ToggleSettings))
    ));
    toggle(&mut varde);
    assert!(varde.page.open);
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
    let keys = varde_view::DocumentKeys {
        undo: true,
        redo: true,
        ..varde.doc_keys().unwrap()
    };
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
        scale_dragged: None,
        kept_in: varde_view::KeptIn::Browser,
        mcp: Default::default(),
    };
    let view = document(&varde).view_with(
        false,
        Mode::Light,
        varde.options,
        varde.files.offers(),
        Some(page),
        None,
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
        Ui::Settings(varde_view::SettingsPage::Set(varde_view::Setting::Theme(
            ThemeChoice::Dark,
        ))),
        Ui::SetProjection(Projection::Perspective),
        Ui::SetShading(varde_render::Shading::Flat),
        Ui::SetEdges(varde_view::Edges::Wireframe),
        Ui::ToggleHiddenEdges,
        Ui::ToggleNames,
        Ui::ToggleMouseHints,
        Ui::Settings(SettingsPage::Set(Setting::LengthUnit(LengthUnit::Cm))),
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

    let _ = varde.update(Message::Ui(Ui::Settings(varde_view::SettingsPage::Set(
        varde_view::Setting::Theme(ThemeChoice::Light),
    ))));
    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::Set(
        Setting::LengthUnit(LengthUnit::Ft),
    ))));
    let _ = sent(&requests);
    assert_eq!(
        varde_view::changed_settings(varde.options),
        [
            SettingChange {
                name: "Theme",
                from: "Light".into(),
                to: "System".into(),
            },
            SettingChange {
                name: "Length unit",
                from: "ft".into(),
                to: "mm".into(),
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
    let _ = varde.update(Message::Ui(Ui::Settings(varde_view::SettingsPage::Set(
        varde_view::Setting::Theme(ThemeChoice::Dark),
    ))));
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

    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::Set(
        Setting::LengthUnit(LengthUnit::In),
    ))));
    let _ = varde.update(Message::Ui(Ui::SetProjection(Projection::Perspective)));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let doc = document(&varde);
    assert_eq!(doc.editor.document().units(), LengthUnit::In);
    assert_eq!(doc.camera.projection(), Projection::Perspective);
    // Changing the setting leaves the design open in its units.
    let _ = varde.update(Message::Ui(Ui::Settings(SettingsPage::Set(
        Setting::LengthUnit(LengthUnit::Cm),
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
        crate::auto_save_wait(&options),
        Some(crate::doc::AUTO_SAVE_WAIT)
    );
    assert_eq!(usize::from(options.recent_designs), varde_io::recent::MAX);
    assert!(varde_view::changed_settings(options).is_empty());
}

pub(crate) fn set(varde: &mut Varde, setting: Setting) {
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
        Setting::InterfaceScale(155),
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
        Setting::AutoSave(5),
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
            interface_scale: io::InterfaceScale::new(155).unwrap(),
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
            auto_save: io::AutoSave::Minutes5,
            recent_designs: io::RecentDesigns::TwentyFive,
            ..Stored::default()
        }
    );
    // Not offered, not taken nor stored.
    for odd in [
        Setting::InterfaceScale(112),
        Setting::InterfaceScale(70),
        Setting::InterfaceScale(205),
        Setting::InterfaceScale(0),
        Setting::InterfaceScale(u16::MAX),
        Setting::Decimals(Some(7)),
        Setting::Decimals(Some(u8::MAX)),
        Setting::AutoSave(3),
        Setting::AutoSave(30),
        Setting::AutoSave(60),
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
    assert!((next.scale_factor() - 1.55).abs() < 1e-6);
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

/// The scale's slider only moves its label while dragged, and sets the
/// scale once let go: as the last place dragged to, stored then and only
/// then. A drag left on the page or a tab changed sets nothing.
#[test]
fn the_scale_slider_sets_the_scale_when_let_go() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(varde_io::Response::SettingsLoaded {
        settings: Stored::default(),
    }));
    let _ = varde.update(Message::Ui(Ui::ToggleSettings));
    let _ = sent(&requests);
    let page = |varde: &mut Varde, message| {
        let _ = varde.update(Message::Ui(Ui::Settings(message)));
    };
    for percent in [80, 135, 200] {
        page(&mut varde, SettingsPage::DragScale(percent));
        assert_eq!(varde.page.scale_dragged, Some(percent));
        assert_eq!(varde.options.interface_scale, 100);
        assert_eq!(varde.scale_factor(), 1.0);
    }
    assert!(written(&requests).is_empty());
    page(&mut varde, SettingsPage::DragScale(125));
    page(&mut varde, SettingsPage::ReleaseScale);
    assert_eq!(varde.page.scale_dragged, None);
    assert_eq!(varde.options.interface_scale, 125);
    assert!((varde.scale_factor() - 1.25).abs() < 1e-6);
    let written = written(&requests);
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].interface_scale.percent(), 125);
    // A release with nothing dragged sets nothing.
    page(&mut varde, SettingsPage::ReleaseScale);
    assert!(sent(&requests).is_empty());
    // A value not offered isn't taken as dragged.
    page(&mut varde, SettingsPage::DragScale(112));
    assert_eq!(varde.page.scale_dragged, None);
    // Dragged, then the tab changed or the page closed: nothing set.
    page(&mut varde, SettingsPage::DragScale(75));
    page(&mut varde, SettingsPage::Tab(SettingsTab::View));
    page(&mut varde, SettingsPage::ReleaseScale);
    page(&mut varde, SettingsPage::DragScale(75));
    let _ = varde.update(Message::Ui(Ui::ToggleSettings));
    page(&mut varde, SettingsPage::ReleaseScale);
    assert_eq!(varde.options.interface_scale, 125);
    assert!(sent(&requests).is_empty());
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

/// A trackball rolls the document's camera as a drag goes round; back to
/// a turntable, the camera, and where it's turning to, are set upright,
/// the eye kept, and over the top unrolled.
#[test]
fn a_trackball_rolls_and_a_turntable_rights_it() {
    let (mut varde, _) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    set(&mut varde, Setting::Rotation(Rotation::Trackball));
    let round = |varde: &mut Varde| {
        for (yaw, pitch) in [(0.6, 0.0), (0.0, 0.6), (-0.6, 0.0), (0.0, -0.6)] {
            let _ = varde.update(Message::Ui(Ui::Look(Look::Orbit {
                yaw,
                pitch,
                about: None,
            })));
        }
    };
    round(&mut varde);
    let rolled = document(&varde).camera;
    assert!(rolled.roll().abs() > 0.05, "{rolled:?}");

    // Over the top takes the roll away, the eye kept.
    set(&mut varde, Setting::Rotation(Rotation::OverTheTop));
    let unrolled = document(&varde).camera;
    assert_eq!(unrolled.roll(), 0.0);
    assert!(unrolled.eye().abs_diff_eq(rolled.eye(), 1e-3));

    set(&mut varde, Setting::Rotation(Rotation::Trackball));
    round(&mut varde);
    round(&mut varde);
    let rolled = document(&varde).camera;
    assert!(rolled.roll().abs() > 0.05);
    // Mid-turn to the top, from the rolled view.
    let _ = varde.update(Message::Ui(Ui::Look(Look::LookFrom(
        varde_render::View::Top,
    ))));
    let animation = document(&varde).animation.as_ref().unwrap();
    assert_ne!(animation.from.roll(), 0.0);
    assert_eq!(animation.to.roll(), 0.0);
    set(&mut varde, Setting::Rotation(Rotation::Turntable));
    let doc = document(&varde);
    assert_eq!(doc.camera.roll(), 0.0);
    assert!(!doc.camera.upside_down());
    assert!(doc.camera.eye().abs_diff_eq(rolled.eye(), 1e-3));
    let animation = doc.animation.as_ref().unwrap();
    assert_eq!(animation.from.roll(), 0.0);
    assert!(!animation.from.upside_down());
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

/// Open or not is the app's, but every move to another view closes it so
/// what's moved to shows: New design, Open (a picked file, a recent
/// design, a recovered one, a sample) and Close document. A cancelled
/// pick, a close cancelled at the unsaved-changes prompt and a failed
/// open leave it open.
#[test]
fn the_page_closes_on_every_move_to_another_view() {
    use crate::tests::{an_edit, last_open, opened};
    use varde_io::Response as IoResponse;
    use varde_view::{File, Unsaved};
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(IoResponse::RecentLoaded {
        entries: vec![],
        home: None,
    }));
    let close = |varde: &mut Varde| {
        let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
        // A design never saved to a file asks first.
        if varde.screen.doc().is_some() {
            let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Discard))));
        }
        assert!(varde.screen.doc().is_none());
    };

    // A picked file, after a cancelled pick.
    toggle(&mut varde);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::Open)));
    let _ = varde.update(Message::Picked(None));
    assert!(varde.screen.doc().is_none());
    assert!(varde.page.open, "a cancelled pick moves nowhere");
    let _ = varde.update(Message::Picked(Some(varde_io::Chosen::Path(
        "/d/picked.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(opened(id, path, 0, varde_io::Access::Edit));
    assert!(varde.screen.doc().is_some());
    assert!(!varde.page.open);

    // Close document, after a close cancelled at the prompt.
    toggle(&mut varde);
    let edit = an_edit(document(&varde));
    let _ = varde.update(Message::Ui(Ui::Edit(edit)));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Cancel))));
    assert!(varde.screen.doc().is_some());
    assert!(varde.page.open, "a cancelled close moves nowhere");
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Discard))));
    assert!(varde.screen.doc().is_none());
    assert!(!varde.page.open);

    // A recent design (opened by its path), after a failed open.
    toggle(&mut varde);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/gone.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: Some(path),
        result: Err("no such file".to_owned()),
    }));
    assert!(varde.screen.doc().is_none());
    assert!(varde.page.open, "a failed open moves nowhere");
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/part.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(opened(id, path, 1, varde_io::Access::Edit));
    assert!(varde.screen.doc().is_some());
    assert!(!varde.page.open);
    close(&mut varde);

    // A recovered design, left in the store by a crash.
    toggle(&mut varde);
    let stored = std::path::PathBuf::from("/data/designs/a.vrdp");
    let _ = sent(&requests);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenStored(
        stored.clone(),
    ))));
    let sent_now = sent(&requests);
    let [IoRequest::OpenRecovered { id, .. }] = &sent_now[..] else {
        panic!("not opened: {sent_now:?}");
    };
    let _ = varde.update(opened(*id, stored, 2, varde_io::Access::Edit));
    assert!(varde.screen.doc().is_some());
    assert!(!varde.page.open);
    close(&mut varde);

    // A sample, where the build has them.
    if !crate::samples::SAMPLES.is_empty() {
        toggle(&mut varde);
        let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenSample(0))));
        assert!(varde.screen.doc().is_some());
        assert!(!varde.page.open);
        close(&mut varde);
    }

    // New design.
    toggle(&mut varde);
    assert!(welcome_key((true, letter("n"))).is_some());
    let _ = varde.update(welcome_key((true, letter("n"))).unwrap());
    assert!(varde.screen.doc().is_some());
    assert!(!varde.page.open);
}

/// A changed setting's reset sets just it back to its default, stored as
/// choosing it is; the others stay as they are.
#[test]
fn a_rows_reset_resets_just_its_setting() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(varde_io::Response::SettingsLoaded {
        settings: Stored::default(),
    }));
    let _ = varde.update(Message::Ui(Ui::Settings(varde_view::SettingsPage::Set(
        varde_view::Setting::Theme(ThemeChoice::Dark),
    ))));
    set(&mut varde, Setting::AngleUnit(AngleUnit::Rad));
    set(&mut varde, Setting::Decimals(Some(2)));
    set(&mut varde, Setting::InterfaceScale(150));
    set(&mut varde, Setting::PivotClick(PivotClick::ShiftMiddle));
    set(&mut varde, Setting::AutoSave(10));
    let _ = varde.update(Message::Ui(Ui::ToggleNames));
    let _ = sent(&requests);
    let changed = varde.options;
    let defaults = ViewOptions::default();
    // What each row's reset sends (see `varde_view`'s settings tests),
    // and the setting as it is after.
    type Case = (Ui, fn(&mut ViewOptions));
    let cases: [Case; 7] = [
        (
            Ui::Settings(varde_view::SettingsPage::Set(varde_view::Setting::Theme(
                defaults.theme,
            ))),
            |options| options.theme = ViewOptions::default().theme,
        ),
        (
            Ui::Settings(SettingsPage::Set(Setting::AngleUnit(AngleUnit::Deg))),
            |options| options.angle_unit = AngleUnit::Deg,
        ),
        (
            Ui::Settings(SettingsPage::Set(Setting::Decimals(None))),
            |options| options.decimals = None,
        ),
        (
            Ui::Settings(SettingsPage::Set(Setting::InterfaceScale(100))),
            |options| options.interface_scale = 100,
        ),
        (
            Ui::Settings(SettingsPage::Set(Setting::PivotClick(PivotClick::Middle))),
            |options| options.mouse.pivot_click = PivotClick::Middle,
        ),
        (
            Ui::Settings(SettingsPage::Set(Setting::AutoSave(1))),
            |options| options.auto_save = 1,
        ),
        // Set, not toggled, so twice is still the default.
        (
            Ui::Settings(SettingsPage::Set(Setting::Names(false))),
            |options| options.names = false,
        ),
    ];
    for (reset, after) in cases {
        varde.options = changed;
        let _ = varde.update(Message::Ui(reset.clone()));
        let _ = varde.update(Message::Ui(reset.clone()));
        let mut expected = changed;
        after(&mut expected);
        assert_eq!(varde.options, expected, "{reset:?}");
        assert_eq!(
            written(&requests).last().copied(),
            Some(crate::settings::stored_of(expected)),
            "{reset:?}"
        );
    }
}
