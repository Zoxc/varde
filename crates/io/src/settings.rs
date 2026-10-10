//! The user's settings.
//!
//! Kept in `settings.toml`: natively in the platform config directory next
//! to the recent files list, e.g. `~/.config/varde-cad/settings.toml`; on
//! the web at the root of the Origin Private File System. A table of
//! plain keys:
//!
//! ```toml
//! theme = "auto" # or "light", "dark"
//! mouse_hints = true # whether the status bar shows the mouse's hints
//! hidden_edges = true # whether the edges the model hides show, dashed
//! names = false # whether the names overlay shows
//! projection = "orthographic" # or "perspective"
//! shading = "regular" # or "flat", "metal", "flat_metal"
//! edges = "default" # or "wireframe", "tessellation"
//! length_unit = "mm" # a new design's units: or "cm", "m", "in", "ft"
//! ```
//!
//! The file is user data, edited by hand perhaps: each key is read on its
//! own, so one missing or gone bad gets its default and costs no other.
//! Keys this build doesn't know are left out, and dropped by the next
//! write. Reading and writing it is `src/native/settings.rs`'s natively,
//! `src/web/worker/files.rs`'s on the web.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use varde_document::LengthUnit;

/// The most bytes of the file read: a larger one is taken as gone bad.
pub(crate) const MAX_BYTES: u64 = 64 * 1024;

/// The settings, each defaulted when the file doesn't say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub theme: Theme,
    /// Whether the status bar shows the hints of the mouse, on unless
    /// turned off.
    pub mouse_hints: bool,
    /// Whether the viewport shows the edges the model hides, dashed: on
    /// unless turned off.
    pub hidden_edges: bool,
    /// Whether the viewport shows the names overlay: off unless turned on.
    pub names: bool,
    pub projection: Projection,
    pub shading: Shading,
    pub edges: Edges,
    /// The units a new design starts in, millimetres unless chosen.
    /// Stored as the unit's symbol, "mm".
    #[serde(with = "length_unit")]
    pub length_unit: LengthUnit,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            mouse_hints: true,
            hidden_edges: true,
            names: false,
            projection: Projection::default(),
            shading: Shading::default(),
            edges: Edges::default(),
            length_unit: LengthUnit::default(),
        }
    }
}

/// Whether the UI is light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// As the system prefers, light if it doesn't say.
    #[default]
    Auto,
    Light,
    Dark,
}

/// How the viewport projects the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Projection {
    #[default]
    Orthographic,
    Perspective,
}

/// How the viewport lights the model's faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shading {
    #[default]
    Regular,
    Flat,
    Metal,
    FlatMetal,
}

/// Which of the model's edges the viewport draws besides the feature
/// edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edges {
    #[default]
    Default,
    Wireframe,
    Tessellation,
}

/// A length unit as its symbol: "mm". Any other text isn't one.
mod length_unit {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    use varde_document::LengthUnit;

    pub(super) fn serialize<S: Serializer>(unit: &LengthUnit, to: S) -> Result<S::Ok, S::Error> {
        to.serialize_str(unit.symbol())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<LengthUnit, D::Error> {
        let symbol = <std::borrow::Cow<'de, str>>::deserialize(from)?;
        super::length_unit_of(&symbol).ok_or_else(|| D::Error::custom("not a length unit"))
    }
}

/// The length unit whose symbol is `symbol`, if one's is.
fn length_unit_of(symbol: &str) -> Option<LengthUnit> {
    LengthUnit::ALL
        .into_iter()
        .find(|unit| unit.symbol() == symbol)
}

impl Settings {
    /// The settings `toml` holds, any of them missing or gone bad
    /// defaulted. Nothing in it is usable if it isn't a TOML table.
    pub(crate) fn parse(toml: &str) -> Self {
        let Ok(table) = toml.parse::<toml::Table>() else {
            return Self::default();
        };
        let key = |name: &str| table.get(name).cloned();
        let defaults = Self::default();
        Self {
            theme: key("theme")
                .and_then(|value| value.try_into().ok())
                .unwrap_or(defaults.theme),
            mouse_hints: key("mouse_hints")
                .and_then(|value| value.as_bool())
                .unwrap_or(defaults.mouse_hints),
            hidden_edges: key("hidden_edges")
                .and_then(|value| value.as_bool())
                .unwrap_or(defaults.hidden_edges),
            names: key("names")
                .and_then(|value| value.as_bool())
                .unwrap_or(defaults.names),
            projection: key("projection")
                .and_then(|value| value.try_into().ok())
                .unwrap_or(defaults.projection),
            shading: key("shading")
                .and_then(|value| value.try_into().ok())
                .unwrap_or(defaults.shading),
            edges: key("edges")
                .and_then(|value| value.try_into().ok())
                .unwrap_or(defaults.edges),
            length_unit: key("length_unit")
                .and_then(|value| value.as_str().and_then(length_unit_of))
                .unwrap_or(defaults.length_unit),
        }
    }

    pub(crate) fn serialize(&self) -> String {
        toml::to_string(self).expect("plain keys serialize")
    }
}

/// Where the settings are stored, if there is a config directory to put
/// them in.
#[cfg(not(target_arch = "wasm32"))]
pub fn store() -> Option<PathBuf> {
    crate::native::project_dirs().map(|dirs| dirs.config_dir().join("settings.toml"))
}

/// Reads the settings from [`store`] on the calling thread, defaulted
/// where they can't be read: for the theme the window first draws in,
/// before the IO lane has answered [`Request::LoadSettings`]. Everything
/// else asks the lane.
///
/// [`Request::LoadSettings`]: crate::Request::LoadSettings
#[cfg(not(target_arch = "wasm32"))]
pub fn read_now() -> Settings {
    store()
        .map(|store| crate::native::settings::load(&store))
        .unwrap_or_default()
}

/// On the web, `settings.toml` at the root of the Origin Private File
/// System.
#[cfg(target_arch = "wasm32")]
pub fn store() -> Option<PathBuf> {
    Some(PathBuf::from("settings.toml"))
}

#[cfg(test)]
mod tests;
