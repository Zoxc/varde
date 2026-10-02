use super::*;

#[test]
fn toml_round_trip() {
    for theme in [Theme::Auto, Theme::Light, Theme::Dark] {
        let settings = Settings { theme };
        let toml = settings.serialize();
        assert_eq!(Settings::parse(&toml), settings, "{toml}");
    }
    assert_eq!(
        Settings { theme: Theme::Dark }.serialize(),
        "theme = \"dark\"\n"
    );
}

#[test]
fn missing_or_bad_keys_are_defaulted() {
    assert_eq!(Settings::parse(""), Settings::default());
    assert_eq!(Settings::parse("[[theme]\n"), Settings::default());
    assert_eq!(Settings::parse("theme = \"sepia\"\n"), Settings::default());
    assert_eq!(Settings::parse("theme = 3\n"), Settings::default());
    assert_eq!(Settings::default().theme, Theme::Auto);
}

/// A key this build doesn't know costs the others nothing.
#[test]
fn unknown_keys_are_left_out() {
    assert_eq!(
        Settings::parse("units = \"mm\"\ntheme = \"light\"\n[grid]\nsize = 5\n"),
        Settings {
            theme: Theme::Light
        }
    );
}
