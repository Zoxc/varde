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
