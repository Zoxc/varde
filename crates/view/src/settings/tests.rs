use super::*;

#[test]
fn nothing_changed_by_default() {
    assert!(changed_settings(ViewOptions::default()).is_empty());
}

/// Each setting not its default is listed, in the page's order, from
/// what it is to the default.
#[test]
fn changes_are_listed_from_what_they_are_to_the_defaults() {
    let options = ViewOptions {
        theme: ThemeChoice::Dark,
        names: true,
        shading: Shading::FlatMetal,
        length_unit: LengthUnit::In,
        ..ViewOptions::default()
    };
    let change = |name, from, to| SettingChange { name, from, to };
    assert_eq!(
        changed_settings(options),
        [
            change("Theme", "Dark", "System"),
            change("Shading", "Flat metal", "Regular"),
            change("Names", "On", "Off"),
            change("Length unit", "in", "mm"),
        ]
    );
}

/// The newer settings are listed too, said as the page says them.
#[test]
fn mouse_units_and_files_changes_are_listed() {
    let options = ViewOptions {
        interface_scale: 125,
        angle_unit: AngleUnit::Rad,
        decimals: Some(0),
        mouse: crate::MouseOptions {
            left: DragAction::Nothing,
            rotation: Rotation::OverTheTop,
            rotation_point: RotationPoint::Pointer,
            pivot_click: PivotClick::ShiftMiddle,
            wheel_down_zooms_in: true,
            zoom_to_pointer: false,
            sketch_snap: false,
            ..crate::MouseOptions::default()
        },
        auto_save: 0,
        recent_designs: 25,
        ..ViewOptions::default()
    };
    let change = |name, from, to| SettingChange { name, from, to };
    assert_eq!(
        changed_settings(options),
        [
            change("Interface scale", "125%", "100%"),
            change("Angle unit", "Radians", "Degrees"),
            change("Decimal places", "0", "Auto"),
            change("Left drag", "Nothing", "Orbit"),
            change("Rotation", "Turntable, over the top", "Turntable"),
            change("Rotation point", "Under the pointer", "Clicked"),
            change(
                "Set rotation point with",
                "Shift + middle click",
                "Middle click"
            ),
            change("Zoom direction", "Wheel down", "Wheel up"),
            change("Zoom to pointer", "Off", "On"),
            change("Snapping", "Off", "On"),
            change("Auto-save", "Off", "3 s"),
            change("Recent designs", "25", "100"),
        ]
    );
}

/// Every tab shows, its rows laid out, and the Units tab's sample shows
/// values as the settings do; set rotation point shows only for a
/// clicked one.
#[test]
fn every_tab_shows_its_rows() {
    let texts = |options: ViewOptions, tab| {
        let state = SettingsState {
            options,
            tab,
            resetting: false,
            kept_in: KeptIn::Browser,
        };
        let mut laid = crate::testing::Laid::new(page(state), iced::Size::new(1100.0, 900.0));
        laid.texts()
            .into_iter()
            .map(|shown| shown.text)
            .collect::<Vec<_>>()
    };
    let has = |texts: &[String], text: &str| texts.iter().any(|shown| shown == text);
    let defaults = ViewOptions::default();
    for (tab, rows) in [
        (
            SettingsTab::Appearance,
            &["Theme", "Interface scale", "125%"][..],
        ),
        (
            SettingsTab::Units,
            &["Length", "Angle", "Decimal places", "Radians", "Auto"],
        ),
        (
            SettingsTab::Mouse,
            &[
                "Left drag",
                "Middle drag",
                "Right drag",
                "Rotation",
                "Over the top",
                "Rotation point",
                "Set rotation point with",
                "Zoom direction",
                "Zoom to pointer",
                "Snapping",
            ],
        ),
        (
            SettingsTab::Files,
            &["Auto-save", "Recent designs", "30 s", "100"],
        ),
    ] {
        let shown = texts(defaults, tab);
        for row in rows {
            assert!(has(&shown, row), "{tab:?} {row}: {shown:?}");
        }
    }
    let note = |options| {
        texts(options, SettingsTab::Units)
            .into_iter()
            .find(|text| text.starts_with("Length for new designs"))
            .unwrap()
    };
    assert!(
        note(defaults).ends_with("Shown as 12.346 mm · 22.5°."),
        "{}",
        note(defaults)
    );
    let set = ViewOptions {
        angle_unit: AngleUnit::Rad,
        decimals: Some(1),
        length_unit: LengthUnit::Cm,
        ..defaults
    };
    assert!(
        note(set).ends_with("Shown as 1.2 cm · 0.4 rad."),
        "{}",
        note(set)
    );
    let centre = ViewOptions {
        mouse: crate::MouseOptions {
            rotation_point: RotationPoint::Centre,
            ..crate::MouseOptions::default()
        },
        ..defaults
    };
    assert!(!has(
        &texts(centre, SettingsTab::Mouse),
        "Set rotation point with"
    ));
}
