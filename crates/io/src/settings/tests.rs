use super::*;

#[test]
fn toml_round_trip() {
    for theme in [Theme::Auto, Theme::Light, Theme::Dark] {
        for mouse_hints in [false, true] {
            let settings = Settings {
                theme,
                mouse_hints,
                ..Settings::default()
            };
            let toml = settings.serialize();
            assert_eq!(Settings::parse(&toml), settings, "{toml}");
        }
    }
    let settings = Settings {
        theme: Theme::Dark,
        mouse_hints: false,
        hidden_edges: false,
        names: true,
        projection: Projection::Perspective,
        shading: Shading::FlatMetal,
        edges: Edges::Tessellation,
        length_unit: LengthUnit::In,
    };
    assert_eq!(
        settings.serialize(),
        "theme = \"dark\"\nmouse_hints = false\nhidden_edges = false\nnames = true\n\
         projection = \"perspective\"\nshading = \"flat_metal\"\nedges = \"tessellation\"\n\
         length_unit = \"in\"\n"
    );
    assert_eq!(Settings::parse(&settings.serialize()), settings);
    for length_unit in LengthUnit::ALL {
        let settings = Settings {
            length_unit,
            ..Settings::default()
        };
        assert_eq!(Settings::parse(&settings.serialize()), settings);
    }
}

/// A file from before the view options and units were stored reads with
/// them defaulted, its theme and hints kept.
#[test]
fn an_old_file_keeps_working() {
    assert_eq!(
        Settings::parse("theme = \"dark\"\nmouse_hints = false\n"),
        Settings {
            theme: Theme::Dark,
            mouse_hints: false,
            ..Settings::default()
        }
    );
}

#[test]
fn bad_view_options_and_units_are_defaulted() {
    for bad in [
        "hidden_edges = \"no\"\n",
        "names = 1\n",
        "projection = \"fisheye\"\n",
        "shading = \"FlatMetal\"\n",
        "edges = 2\n",
        "length_unit = \"MM\"\n",
        "length_unit = \"furlong\"\n",
        "length_unit = 25.4\n",
    ] {
        assert_eq!(Settings::parse(bad), Settings::default(), "{bad}");
    }
    assert_eq!(
        Settings::parse("length_unit = \"furlong\"\nnames = true\nedges = \"wireframe\"\n"),
        Settings {
            names: true,
            edges: Edges::Wireframe,
            ..Settings::default()
        }
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
