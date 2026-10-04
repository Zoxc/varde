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
    };
    assert_eq!(written(settings.loaded(stored, &mut options)), None);
    assert_eq!(options.theme, ThemeChoice::Dark);
    assert!(!options.mouse_hints);
    options.theme = ThemeChoice::Auto;
    assert_eq!(
        written(settings.chose_theme(options)),
        Some(Stored {
            theme: Theme::Auto,
            mouse_hints: false,
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
    assert_eq!(theme(settings.chose_theme(options)), None);
    let stored = Stored {
        theme: Theme::Dark,
        mouse_hints: false,
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
        })
    );
}

/// So do the mouse's hints turned off early, the theme stored kept.
#[test]
fn hints_turned_off_early_win_once_loaded() {
    let mut settings = Settings::default();
    let mut options = ViewOptions {
        mouse_hints: false,
        ..ViewOptions::default()
    };
    assert_eq!(written(settings.chose_mouse_hints(options)), None);
    let stored = Stored {
        theme: Theme::Dark,
        mouse_hints: true,
    };
    let write = settings.loaded(stored, &mut options);
    assert!(!options.mouse_hints);
    assert_eq!(options.theme, ThemeChoice::Dark);
    assert_eq!(
        written(write),
        Some(Stored {
            theme: Theme::Dark,
            mouse_hints: false,
        })
    );
}
