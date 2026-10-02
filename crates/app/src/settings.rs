//! The user's settings, as stored: the theme chosen.
//!
//! The IO lane stores them (see [`varde_io::settings`]) and hands them over
//! once read at startup; until then the theme is the default, the system's.
//! The app keeps the theme in its [`ViewOptions`](varde_view::ViewOptions),
//! and this what's stored of it.

use varde_io::Request as IoRequest;
use varde_io::settings::{Settings as Stored, Theme};
use varde_view::ThemeChoice;

#[derive(Debug, Default)]
pub(crate) struct Settings {
    /// Whether the stored settings have arrived.
    loaded: bool,
    /// Whether the user chose a theme before they did: theirs wins.
    chosen: bool,
}

impl Settings {
    /// Takes the stored settings, returning the theme to use, the stored
    /// one unless the user chose another before they arrived, `current`,
    /// and if so the write storing it.
    #[must_use = "the write stores the settings"]
    pub(crate) fn loaded(
        &mut self,
        stored: Stored,
        current: ThemeChoice,
    ) -> (ThemeChoice, Option<IoRequest>) {
        self.loaded = true;
        if self.chosen {
            return (current, self.write(current));
        }
        (choice(stored.theme), None)
    }

    /// Records that the user chose `theme`, returning the write storing
    /// it, once the stored settings have arrived: before, writing would
    /// drop what else they hold.
    #[must_use = "the write stores the settings"]
    pub(crate) fn chose(&mut self, theme: ThemeChoice) -> Option<IoRequest> {
        self.chosen = true;
        self.write(theme)
    }

    fn write(&self, theme: ThemeChoice) -> Option<IoRequest> {
        self.loaded.then(|| IoRequest::WriteSettings {
            settings: Stored {
                theme: stored(theme),
            },
        })
    }
}

fn choice(theme: Theme) -> ThemeChoice {
    match theme {
        Theme::Auto => ThemeChoice::Auto,
        Theme::Light => ThemeChoice::Light,
        Theme::Dark => ThemeChoice::Dark,
    }
}

fn stored(theme: ThemeChoice) -> Theme {
    match theme {
        ThemeChoice::Auto => Theme::Auto,
        ThemeChoice::Light => Theme::Light,
        ThemeChoice::Dark => Theme::Dark,
    }
}

#[cfg(test)]
mod tests;
