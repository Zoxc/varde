//! The user's settings, as stored: the theme chosen, the view options
//! (the status bar's menu's, on the settings page too) and the units new
//! designs start in.
//!
//! The IO lane stores them (see [`varde_io::settings`]) and hands them over
//! once read at startup; until then they're the defaults, the system's
//! theme, the hints shown and so on. The app keeps them in its
//! [`ViewOptions`], and this what's stored of them.

use varde_io::Request as IoRequest;
use varde_io::settings::{
    Edges as StoredEdges, Projection as StoredProjection, Settings as Stored,
    Shading as StoredShading, Theme,
};
use varde_render::{Projection, Shading};
use varde_view::{Edges, ThemeChoice, ViewOptions};

/// A setting, each chosen on its own: the one chosen before the stored
/// settings arrive wins over what they say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    Theme,
    MouseHints,
    HiddenEdges,
    Names,
    Projection,
    Shading,
    Edges,
    LengthUnit,
}

impl Key {
    pub(crate) const ALL: [Key; 8] = [
        Key::Theme,
        Key::MouseHints,
        Key::HiddenEdges,
        Key::Names,
        Key::Projection,
        Key::Shading,
        Key::Edges,
        Key::LengthUnit,
    ];

    /// Takes this setting of `from` into `to`.
    fn copy(self, from: ViewOptions, to: &mut ViewOptions) {
        match self {
            Key::Theme => to.theme = from.theme,
            Key::MouseHints => to.mouse_hints = from.mouse_hints,
            Key::HiddenEdges => to.hidden_edges = from.hidden_edges,
            Key::Names => to.names = from.names,
            Key::Projection => to.projection = from.projection,
            Key::Shading => to.shading = from.shading,
            Key::Edges => to.edges = from.edges,
            Key::LengthUnit => to.length_unit = from.length_unit,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Settings {
    /// Whether the stored settings have arrived.
    loaded: bool,
    /// The settings the user chose before they did, by [`Key`] in
    /// [`Key::ALL`]'s order: theirs win.
    chosen: [bool; Key::ALL.len()],
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
        let stored = options_of(stored);
        for (key, chosen) in Key::ALL.into_iter().zip(self.chosen) {
            if !chosen {
                key.copy(stored, options);
            }
        }
        (self.chosen.contains(&true))
            .then(|| self.write(*options))
            .flatten()
    }

    /// Takes `stored` into `options` ahead of [`Settings::loaded`], which
    /// stays the one that counts: so the first frame has the stored theme.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn preview(stored: Stored, options: &mut ViewOptions) {
        *options = options_of(stored);
    }

    /// Records that the user chose the setting `key` of `options`,
    /// returning the write storing it, once the stored settings have
    /// arrived: before, writing would drop what else they hold.
    #[must_use = "the write stores the settings"]
    pub(crate) fn chose(&mut self, key: Key, options: ViewOptions) -> Option<IoRequest> {
        if let Some(at) = Key::ALL.iter().position(|&each| each == key)
            && let Some(chosen) = self.chosen.get_mut(at)
        {
            *chosen = true;
        }
        self.write(options)
    }

    /// Records that the user chose every setting of `options`, as Reset
    /// to defaults does, as [`Settings::chose`].
    #[must_use = "the write stores the settings"]
    pub(crate) fn chose_all(&mut self, options: ViewOptions) -> Option<IoRequest> {
        self.chosen = [true; Key::ALL.len()];
        self.write(options)
    }

    fn write(&self, options: ViewOptions) -> Option<IoRequest> {
        self.loaded.then(|| IoRequest::WriteSettings {
            settings: stored_of(options),
        })
    }
}

/// The settings `stored` holds.
fn options_of(stored: Stored) -> ViewOptions {
    ViewOptions {
        theme: match stored.theme {
            Theme::Auto => ThemeChoice::Auto,
            Theme::Light => ThemeChoice::Light,
            Theme::Dark => ThemeChoice::Dark,
        },
        mouse_hints: stored.mouse_hints,
        hidden_edges: stored.hidden_edges,
        names: stored.names,
        projection: match stored.projection {
            StoredProjection::Orthographic => Projection::Orthographic,
            StoredProjection::Perspective => Projection::Perspective,
        },
        shading: match stored.shading {
            StoredShading::Regular => Shading::Regular,
            StoredShading::Flat => Shading::Flat,
            StoredShading::Metal => Shading::Metal,
            StoredShading::FlatMetal => Shading::FlatMetal,
        },
        edges: match stored.edges {
            StoredEdges::Default => Edges::Default,
            StoredEdges::Wireframe => Edges::Wireframe,
            StoredEdges::Tessellation => Edges::Tessellation,
        },
        length_unit: stored.length_unit,
    }
}

/// What's stored of `options`.
pub(crate) fn stored_of(options: ViewOptions) -> Stored {
    Stored {
        theme: match options.theme {
            ThemeChoice::Auto => Theme::Auto,
            ThemeChoice::Light => Theme::Light,
            ThemeChoice::Dark => Theme::Dark,
        },
        mouse_hints: options.mouse_hints,
        hidden_edges: options.hidden_edges,
        names: options.names,
        projection: match options.projection {
            Projection::Orthographic => StoredProjection::Orthographic,
            Projection::Perspective => StoredProjection::Perspective,
        },
        shading: match options.shading {
            Shading::Regular => StoredShading::Regular,
            Shading::Flat => StoredShading::Flat,
            Shading::Metal => StoredShading::Metal,
            Shading::FlatMetal => StoredShading::FlatMetal,
        },
        edges: match options.edges {
            Edges::Default => StoredEdges::Default,
            Edges::Wireframe => StoredEdges::Wireframe,
            Edges::Tessellation => StoredEdges::Tessellation,
        },
        length_unit: options.length_unit,
    }
}

#[cfg(test)]
mod tests;
