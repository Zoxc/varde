use super::*;

#[test]
fn toml_round_trip() {
    for theme in [Theme::Auto, Theme::Light, Theme::Dark] {
        for mouse_hints in [false, true] {
            let settings = Settings { theme, mouse_hints };
            let toml = settings.serialize();
            assert_eq!(Settings::parse(&toml), settings, "{toml}");
        }
    }
    let settings = Settings {
        theme: Theme::Dark,
        mouse_hints: false,
    };
    assert_eq!(
        settings.serialize(),
        "theme = \"dark\"\nmouse_hints = false\n"
    );
}

#[test]
fn missing_or_bad_keys_are_defaulted() {
    assert_eq!(Settings::parse(""), Settings::default());
    assert_eq!(Settings::parse("[[theme]\n"), Settings::default());
    assert_eq!(Settings::parse("theme = \"sepia\"\n"), Settings::default());
    assert_eq!(Settings::parse("theme = 3\n"), Settings::default());
    assert_eq!(Settings::parse("mouse_hints = 0\n"), Settings::default());
    assert_eq!(Settings::default().theme, Theme::Auto);
    assert!(Settings::default().mouse_hints);
    // One gone bad costs the other nothing.
    assert_eq!(
        Settings::parse("theme = 3\nmouse_hints = false\n"),
        Settings {
            mouse_hints: false,
            ..Settings::default()
        }
    );
}

/// A key this build doesn't know costs the others nothing.
#[test]
fn unknown_keys_are_left_out() {
    assert_eq!(
        Settings::parse("units = \"mm\"\ntheme = \"light\"\n[grid]\nsize = 5\n"),
        Settings {
            theme: Theme::Light,
            ..Settings::default()
        }
    );
}
