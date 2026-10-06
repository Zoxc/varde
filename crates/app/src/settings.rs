//! The user's settings, as stored: the theme chosen and whether the
//! status bar shows the mouse's hints.
//!
//! The IO lane stores them (see [`varde_io::settings`]) and hands them over
//! once read at startup; until then they're the defaults, the system's
//! theme and the hints shown. The app keeps them in its
//! [`ViewOptions`], and this what's stored of them.

use varde_io::Request as IoRequest;
use varde_io::settings::{Settings as Stored, Theme};
use varde_view::{ThemeChoice, ViewOptions};

#[derive(Debug, Default)]
pub(crate) struct Settings {
    /// Whether the stored settings have arrived.
    loaded: bool,
    /// Whether the user chose a theme before they did: theirs wins.
    theme_chosen: bool,
    /// Whether the user turned the mouse's hints on or off before they
    /// did: theirs wins.
    hints_chosen: bool,
}

impl Settings {
    /// Takes the stored settings into `options`, but for those the user
    /// chose before they arrived, returning the write storing those if
    /// there are any.
    #[must_use = "the write stores the settings"]
    pub(crate) fn loaded(
        &mut self,
        stored: Stored,
        options: &mut ViewOptions,
    ) -> Option<IoRequest> {
        self.loaded = true;
        if !self.theme_chosen {
            options.theme = choice(stored.theme);
        }
        if !self.hints_chosen {
            options.mouse_hints = stored.mouse_hints;
        }
        (self.theme_chosen || self.hints_chosen)
            .then(|| self.write(*options))
            .flatten()
    }

    /// Takes `stored` into `options` ahead of [`Settings::loaded`], which
    /// stays the one that counts: so the first frame has the stored theme.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn preview(stored: Stored, options: &mut ViewOptions) {
        options.theme = choice(stored.theme);
        options.mouse_hints = stored.mouse_hints;
    }

    /// Records that the user chose the theme of `options`, returning the
    /// write storing it, once the stored settings have arrived: before,
    /// writing would drop what else they hold.
    #[must_use = "the write stores the settings"]
    pub(crate) fn chose_theme(&mut self, options: ViewOptions) -> Option<IoRequest> {
        self.theme_chosen = true;
        self.write(options)
    }

    /// Records that the user turned the mouse's hints of `options` on or
    /// off, as [`Settings::chose_theme`].
    #[must_use = "the write stores the settings"]
    pub(crate) fn chose_mouse_hints(&mut self, options: ViewOptions) -> Option<IoRequest> {
        self.hints_chosen = true;
        self.write(options)
    }

    fn write(&self, options: ViewOptions) -> Option<IoRequest> {
        self.loaded.then(|| IoRequest::WriteSettings {
            settings: Stored {
                theme: stored(options.theme),
                mouse_hints: options.mouse_hints,
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
