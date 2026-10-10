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
        interface_scale: InterfaceScale::Larger,
        angle_unit: AngleUnit::Rad,
        decimals: Decimals::Places(0),
        left_drag: Drag::Nothing,
        middle_drag: Drag::Zoom,
        right_drag: Drag::Orbit,
        rotation: Rotation::OverTheTop,
        rotation_point: RotationPoint::Pointer,
        set_rotation_point: SetRotationPoint::ShiftMiddleClick,
        zoom_direction: ZoomDirection::WheelDown,
        zoom_to_pointer: false,
        sketch_snap: false,
        auto_save: AutoSave::Off,
        recent_designs: RecentDesigns::Ten,
    };
    assert_eq!(
        settings.serialize(),
        "theme = \"dark\"\nmouse_hints = false\nhidden_edges = false\nnames = true\n\
         projection = \"perspective\"\nshading = \"flat_metal\"\nedges = \"tessellation\"\n\
         length_unit = \"in\"\ninterface_scale = 150\nangle_unit = \"rad\"\ndecimals = 0\n\
         left_drag = \"nothing\"\nmiddle_drag = \"zoom\"\nright_drag = \"orbit\"\n\
         rotation = \"over_the_top\"\nrotation_point = \"pointer\"\n\
         set_rotation_point = \"shift_middle_click\"\nzoom_direction = \"wheel_down\"\n\
         zoom_to_pointer = false\nsketch_snap = false\nauto_save = 0\nrecent_designs = 10\n"
    );
    assert_eq!(Settings::parse(&settings.serialize()), settings);
    for length_unit in LengthUnit::ALL {
        let settings = Settings {
            length_unit,
            ..Settings::default()
        };
        assert_eq!(Settings::parse(&settings.serialize()), settings);
    }
    for rotation in [
        Rotation::Turntable,
        Rotation::OverTheTop,
        Rotation::Trackball,
    ] {
        let settings = Settings {
            rotation,
            ..Settings::default()
        };
        assert_eq!(Settings::parse(&settings.serialize()), settings);
    }
    assert_eq!(
        Settings::parse("rotation = \"trackball\"\n").rotation,
        Rotation::Trackball
    );
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

/// The defaults are the app as it was before these were settings: each
/// key's default written out, so a file without them changes nothing.
#[test]
fn the_defaults_are_as_before() {
    let defaults = Settings::default();
    assert_eq!(defaults.interface_scale.percent(), 100);
    assert_eq!(defaults.angle_unit, AngleUnit::Deg);
    assert_eq!(defaults.decimals, Decimals::Auto);
    assert_eq!(
        [
            defaults.left_drag,
            defaults.middle_drag,
            defaults.right_drag
        ],
        [Drag::Orbit, Drag::Orbit, Drag::Pan]
    );
    assert_eq!(defaults.rotation, Rotation::Turntable);
    assert_eq!(defaults.rotation_point, RotationPoint::Clicked);
    assert_eq!(defaults.set_rotation_point, SetRotationPoint::MiddleClick);
    assert_eq!(defaults.zoom_direction, ZoomDirection::WheelUp);
    assert!(defaults.zoom_to_pointer && defaults.sketch_snap);
    assert_eq!(defaults.auto_save.seconds(), 3);
    assert_eq!(
        usize::from(defaults.recent_designs.count()),
        crate::recent::MAX
    );
    assert!(defaults.serialize().contains("decimals = \"auto\"\n"));
}

/// Every value of the newer settings reads back as written.
#[test]
fn mouse_units_and_files_round_trip() {
    let mut all = Vec::new();
    for interface_scale in InterfaceScale::ALL {
        all.push(Settings {
            interface_scale,
            ..Settings::default()
        });
    }
    for decimals in std::iter::once(Decimals::Auto).chain((0..=Decimals::MAX).map(Decimals::Places))
    {
        all.push(Settings {
            decimals,
            angle_unit: AngleUnit::Rad,
            ..Settings::default()
        });
    }
    for drag in [Drag::Orbit, Drag::Pan, Drag::Zoom, Drag::Nothing] {
        all.push(Settings {
            left_drag: drag,
            middle_drag: drag,
            right_drag: drag,
            ..Settings::default()
        });
    }
    for rotation_point in [
        RotationPoint::Clicked,
        RotationPoint::Pointer,
        RotationPoint::Centre,
    ] {
        for set_rotation_point in [
            SetRotationPoint::MiddleClick,
            SetRotationPoint::MiddleDoubleClick,
            SetRotationPoint::ShiftMiddleClick,
        ] {
            all.push(Settings {
                rotation_point,
                set_rotation_point,
                ..Settings::default()
            });
        }
    }
    for auto_save in AutoSave::ALL {
        for recent_designs in RecentDesigns::ALL {
            all.push(Settings {
                auto_save,
                recent_designs,
                ..Settings::default()
            });
        }
    }
    for settings in all {
        let toml = settings.serialize();
        assert_eq!(Settings::parse(&toml), settings, "{toml}");
    }
}

/// Values out of their sets, of the wrong type or misspelt, are
/// defaulted, each on its own.
#[test]
fn bad_mouse_units_and_files_are_defaulted() {
    for bad in [
        "interface_scale = 110\n",
        "interface_scale = -100\n",
        "interface_scale = 1e9\n",
        "interface_scale = 99999999999\n",
        "interface_scale = \"100\"\n",
        "angle_unit = \"grad\"\n",
        "decimals = 7\n",
        "decimals = -1\n",
        "decimals = 255\n",
        "decimals = 3.0\n",
        "decimals = \"3\"\n",
        "decimals = \"Auto\"\n",
        "left_drag = \"spin\"\n",
        "middle_drag = true\n",
        "right_drag = \"Pan\"\n",
        "rotation = \"arcball\"\n",
        "rotation_point = \"center\"\n",
        "set_rotation_point = \"left_click\"\n",
        "zoom_direction = \"up\"\n",
        "zoom_to_pointer = \"yes\"\n",
        "sketch_snap = 1\n",
        "auto_save = 5\n",
        "auto_save = -3\n",
        "auto_save = \"off\"\n",
        "recent_designs = 0\n",
        "recent_designs = 1000\n",
        "recent_designs = 101\n",
    ] {
        assert_eq!(Settings::parse(bad), Settings::default(), "{bad}");
    }
    assert_eq!(
        Settings::parse(
            "decimals = 9\nauto_save = 30\nrotation = \"x\"\nzoom_to_pointer = false\n"
        ),
        Settings {
            auto_save: AutoSave::Seconds30,
            zoom_to_pointer: false,
            ..Settings::default()
        }
    );
}

/// The web worker's messages carry the settings in postcard, where
/// automatic decimals are a count no setting has.
#[test]
fn decimals_cross_postcard() {
    for decimals in std::iter::once(Decimals::Auto).chain((0..=Decimals::MAX).map(Decimals::Places))
    {
        let bytes = postcard::to_allocvec(&decimals).unwrap();
        assert_eq!(postcard::from_bytes::<Decimals>(&bytes).unwrap(), decimals);
    }
    assert!(postcard::from_bytes::<Decimals>(&[7]).is_err());
}
