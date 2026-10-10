use varde_document::LengthUnit;

use super::*;

fn written(request: Option<IoRequest>) -> Option<Stored> {
    match request {
        Some(IoRequest::WriteSettings { settings }) => Some(settings),
        None => None,
        Some(request) => panic!("not a settings write: {request:?}"),
    }
}

fn theme(request: Option<IoRequest>) -> Option<Theme> {
    written(request).map(|settings| settings.theme)
}

#[test]
fn the_stored_settings_are_taken_and_not_written_back() {
    let mut settings = Settings::default();
    let mut options = ViewOptions::default();
    let stored = Stored {
        theme: Theme::Dark,
        mouse_hints: false,
        names: true,
        projection: StoredProjection::Perspective,
        shading: StoredShading::Metal,
        edges: StoredEdges::Wireframe,
        length_unit: LengthUnit::Cm,
        ..Stored::default()
    };
    assert_eq!(written(settings.loaded(stored, &mut options)), None);
    assert_eq!(options.theme, ThemeChoice::Dark);
    assert!(!options.mouse_hints);
    assert!(options.names && options.hidden_edges);
    assert_eq!(options.projection, Projection::Perspective);
    assert_eq!(options.shading, Shading::Metal);
    assert_eq!(options.edges, Edges::Wireframe);
    assert_eq!(options.length_unit, LengthUnit::Cm);
    assert_eq!(stored_of(options), stored);
    options.theme = ThemeChoice::Auto;
    assert_eq!(
        written(settings.chose(Key::Theme, options)),
        Some(Stored {
            theme: Theme::Auto,
            ..stored
        })
    );
}

/// A theme chosen before the stored settings arrive isn't written over
/// them, and wins once they're there.
#[test]
fn a_theme_chosen_early_wins_once_loaded() {
    let mut settings = Settings::default();
    let mut options = ViewOptions {
        theme: ThemeChoice::Light,
        ..ViewOptions::default()
    };
    assert_eq!(theme(settings.chose(Key::Theme, options)), None);
    let stored = Stored {
        theme: Theme::Dark,
        mouse_hints: false,
        ..Stored::default()
    };
    let write = settings.loaded(stored, &mut options);
    assert_eq!(options.theme, ThemeChoice::Light);
    // The hints, not chosen, are as stored.
    assert!(!options.mouse_hints);
    assert_eq!(
        written(write),
        Some(Stored {
            theme: Theme::Light,
            mouse_hints: false,
            ..Stored::default()
        })
    );
}

/// So does every other setting chosen early, each on its own: the rest
/// are as stored.
#[test]
fn each_setting_chosen_early_wins_once_loaded() {
    let stored = Stored {
        theme: Theme::Dark,
        mouse_hints: false,
        hidden_edges: false,
        names: true,
        projection: StoredProjection::Perspective,
        shading: StoredShading::Flat,
        edges: StoredEdges::Tessellation,
        length_unit: LengthUnit::Ft,
        interface_scale: InterfaceScale::Small,
        angle_unit: StoredAngleUnit::Rad,
        decimals: Decimals::Places(1),
        left_drag: Drag::Pan,
        middle_drag: Drag::Zoom,
        right_drag: Drag::Nothing,
        rotation: StoredRotation::OverTheTop,
        rotation_point: StoredRotationPoint::Centre,
        set_rotation_point: SetRotationPoint::MiddleDoubleClick,
        zoom_direction: ZoomDirection::WheelDown,
        zoom_to_pointer: false,
        sketch_snap: false,
        auto_save: AutoSave::Seconds30,
        recent_designs: RecentDesigns::Fifty,
    };
    // Every setting differs from its default, so each key is seen.
    let defaults = options_of(Stored::default());
    for key in Key::ALL {
        let mut one = defaults;
        key.copy(options_of(stored), &mut one);
        assert_ne!(one, defaults, "{key:?}");
    }
    for key in Key::ALL {
        let mut settings = Settings::default();
        let mut options = ViewOptions::default();
        assert_eq!(written(settings.chose(key, options)), None);
        let write = settings.loaded(stored, &mut options);
        // The one chosen kept its default, the rest as stored.
        let mut expected = options_of(stored);
        key.copy(ViewOptions::default(), &mut expected);
        assert_eq!(options, expected, "{key:?}");
        assert_eq!(written(write), Some(stored_of(expected)), "{key:?}");
    }
}

/// Reset to defaults chooses them all: none of the stored ones arriving
/// after win.
#[test]
fn reset_early_wins_over_everything_stored() {
    let mut settings = Settings::default();
    let mut options = ViewOptions::default();
    assert_eq!(written(settings.chose_all(options)), None);
    let stored = Stored {
        theme: Theme::Dark,
        length_unit: LengthUnit::M,
        ..Stored::default()
    };
    let write = settings.loaded(stored, &mut options);
    assert_eq!(options, ViewOptions::default());
    assert_eq!(written(write), Some(Stored::default()));
}

/// The defaults stored are the app's defaults, and every value read back
/// as stored: what's stored of the options is what they were read from.
#[test]
fn defaults_and_every_value_round_trip() {
    assert_eq!(options_of(Stored::default()), ViewOptions::default());
    assert_eq!(stored_of(ViewOptions::default()), Stored::default());
    let mut all = Vec::new();
    for interface_scale in InterfaceScale::ALL {
        for auto_save in AutoSave::ALL {
            for recent_designs in RecentDesigns::ALL {
                all.push(Stored {
                    interface_scale,
                    auto_save,
                    recent_designs,
                    ..Stored::default()
                });
            }
        }
    }
    for drag in [Drag::Orbit, Drag::Pan, Drag::Zoom, Drag::Nothing] {
        for rotation_point in [
            StoredRotationPoint::Clicked,
            StoredRotationPoint::Pointer,
            StoredRotationPoint::Centre,
        ] {
            all.push(Stored {
                left_drag: drag,
                middle_drag: drag,
                right_drag: drag,
                rotation_point,
                ..Stored::default()
            });
        }
    }
    for decimals in std::iter::once(Decimals::Auto).chain((0..=6).map(Decimals::Places)) {
        all.push(Stored {
            decimals,
            ..Stored::default()
        });
    }
    for stored in all {
        assert_eq!(stored_of(options_of(stored)), stored);
    }
    // Values no setting offers are stored as the defaults.
    let odd = ViewOptions {
        interface_scale: 110,
        decimals: Some(9),
        auto_save: 5,
        recent_designs: 1000,
        ..ViewOptions::default()
    };
    assert_eq!(stored_of(odd), Stored::default());
}
