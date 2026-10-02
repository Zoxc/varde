use super::*;

fn written(request: Option<IoRequest>) -> Option<Theme> {
    match request {
        Some(IoRequest::WriteSettings { settings }) => Some(settings.theme),
        None => None,
        Some(request) => panic!("not a settings write: {request:?}"),
    }
}

#[test]
fn the_stored_theme_is_taken_and_not_written_back() {
    let mut settings = Settings::default();
    let (theme, write) = settings.loaded(Stored { theme: Theme::Dark }, ThemeChoice::Auto);
    assert_eq!(theme, ThemeChoice::Dark);
    assert_eq!(written(write), None);
    assert_eq!(
        written(settings.chose(ThemeChoice::Auto)),
        Some(Theme::Auto)
    );
}

/// A theme chosen before the stored settings arrive isn't written over
/// them, and wins once they're there.
#[test]
fn a_theme_chosen_early_wins_once_loaded() {
    let mut settings = Settings::default();
    assert_eq!(written(settings.chose(ThemeChoice::Light)), None);
    let (theme, write) = settings.loaded(Stored { theme: Theme::Dark }, ThemeChoice::Light);
    assert_eq!(theme, ThemeChoice::Light);
    assert_eq!(written(write), Some(Theme::Light));
}
