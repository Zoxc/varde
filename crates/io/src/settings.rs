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
//! interface_scale = 100 # percent: any multiple of 5 from 75 to 200
//! angle_unit = "deg" # how angles are shown: or "rad"
//! decimals = "auto" # places shown at most: or 0 to 6
//! left_drag = "orbit" # or "pan", "zoom", "nothing"; outside a sketch
//! middle_drag = "orbit" # as left_drag
//! right_drag = "pan" # as left_drag
//! rotation = "turntable" # or "over_the_top", "trackball"
//! rotation_point = "clicked" # or "pointer", "centre"
//! set_rotation_point = "middle_click" # or "middle_double_click", "shift_middle_click"
//! zoom_direction = "wheel_up" # the wheel's way that zooms in: or "wheel_down"
//! zoom_to_pointer = true # else about the view's middle
//! sketch_snap = true # whether sketch tools snap unless Shift is held
//! auto_save = 60 # seconds from the first change not auto-saved: or 300, 600, 0 (off)
//! recent_designs = 100 # how many the recent files list keeps: or 10, 25, 50
//! mcp = false # whether the MCP feature (its panel) is on (natively only)
//! mcp_port = 7311 # the port it listens on: 1024 to 65535
//! ```
//!
//! Values out of these sets (another number of decimals, a scale of 112
//! or 250) are gone bad, and so defaulted, like any other: so are the
//! auto-save pauses of before, 3, 10 and 30 seconds (0, off, stays off).
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
    pub interface_scale: InterfaceScale,
    pub angle_unit: AngleUnit,
    pub decimals: Decimals,
    pub left_drag: Drag,
    pub middle_drag: Drag,
    pub right_drag: Drag,
    pub rotation: Rotation,
    pub rotation_point: RotationPoint,
    pub set_rotation_point: SetRotationPoint,
    pub zoom_direction: ZoomDirection,
    /// Whether the wheel zooms about the point under the pointer, on
    /// unless turned off.
    pub zoom_to_pointer: bool,
    /// Whether sketch tools snap unless Shift is held, on unless turned
    /// off: off, they snap only while it's held.
    pub sketch_snap: bool,
    pub auto_save: AutoSave,
    pub recent_designs: RecentDesigns,
    /// Whether the MCP feature is on (*MCP*: its panel and toggle; the
    /// server is started from the panel, never saved), off unless turned
    /// on: natively only.
    pub mcp: bool,
    /// The port the MCP server listens on.
    pub mcp_port: McpPort,
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
            interface_scale: InterfaceScale::default(),
            angle_unit: AngleUnit::default(),
            decimals: Decimals::default(),
            left_drag: Drag::Orbit,
            middle_drag: Drag::Orbit,
            right_drag: Drag::Pan,
            rotation: Rotation::default(),
            rotation_point: RotationPoint::default(),
            set_rotation_point: SetRotationPoint::default(),
            zoom_direction: ZoomDirection::default(),
            zoom_to_pointer: true,
            sketch_snap: true,
            auto_save: AutoSave::default(),
            recent_designs: RecentDesigns::default(),
            mcp: false,
            mcp_port: McpPort::default(),
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

/// How large the UI is drawn, the model aside: a percentage of the
/// system's scale, a multiple of [`InterfaceScale::STEP`] from
/// [`InterfaceScale::MIN`] to [`InterfaceScale::MAX`]. Stored as the
/// percentage, `100`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct InterfaceScale(u16);

impl InterfaceScale {
    pub const MIN: u16 = 75;
    pub const MAX: u16 = 200;
    pub const STEP: u16 = 5;

    /// `percent`, if it's one of the scales offered.
    pub fn new(percent: u16) -> Option<InterfaceScale> {
        ((Self::MIN..=Self::MAX).contains(&percent) && percent.is_multiple_of(Self::STEP))
            .then_some(InterfaceScale(percent))
    }

    pub fn percent(self) -> u16 {
        self.0
    }
}

impl Default for InterfaceScale {
    fn default() -> Self {
        InterfaceScale(100)
    }
}

impl TryFrom<u16> for InterfaceScale {
    type Error = &'static str;

    fn try_from(percent: u16) -> Result<Self, Self::Error> {
        Self::new(percent).ok_or("not an interface scale")
    }
}

impl From<InterfaceScale> for u16 {
    fn from(scale: InterfaceScale) -> u16 {
        scale.percent()
    }
}

/// The unit angles are shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AngleUnit {
    #[default]
    Deg,
    Rad,
}

/// The most decimal places values are shown with: each unit's own
/// ([`LengthUnit::decimals`]) unless a count is chosen. Stored as
/// `"auto"` or the count, 0 to [`Decimals::MAX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Decimals {
    #[default]
    Auto,
    /// At most [`Decimals::MAX`].
    Places(u8),
}

impl Decimals {
    pub const MAX: u8 = 6;

    /// `count` places, if it's at most [`Decimals::MAX`].
    pub fn places(count: u8) -> Option<Decimals> {
        (count <= Self::MAX).then_some(Decimals::Places(count))
    }
}

/// In TOML, `"auto"` or the count; elsewhere (the web worker's postcard
/// messages, which can't tell a text from a number unasked) the count, or
/// `u8::MAX` for auto.
impl Serialize for Decimals {
    fn serialize<S: serde::Serializer>(&self, to: S) -> Result<S::Ok, S::Error> {
        match (*self, to.is_human_readable()) {
            (Decimals::Auto, true) => to.serialize_str("auto"),
            (Decimals::Auto, false) => to.serialize_u8(u8::MAX),
            (Decimals::Places(count), _) => to.serialize_u8(count),
        }
    }
}

impl<'de> Deserialize<'de> for Decimals {
    fn deserialize<D: serde::Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        use serde::de::{Error, Unexpected, Visitor};

        struct Of;
        impl Visitor<'_> for Of {
            type Value = Decimals;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("\"auto\" or a count of decimal places, 0 to 6")
            }

            fn visit_str<E: Error>(self, text: &str) -> Result<Decimals, E> {
                match text {
                    "auto" => Ok(Decimals::Auto),
                    _ => Err(E::invalid_value(Unexpected::Str(text), &self)),
                }
            }

            fn visit_u64<E: Error>(self, count: u64) -> Result<Decimals, E> {
                u8::try_from(count)
                    .ok()
                    .and_then(Decimals::places)
                    .ok_or_else(|| E::invalid_value(Unexpected::Unsigned(count), &self))
            }

            fn visit_i64<E: Error>(self, count: i64) -> Result<Decimals, E> {
                u64::try_from(count)
                    .map_err(|_| E::invalid_value(Unexpected::Signed(count), &self))
                    .and_then(|count| self.visit_u64(count))
            }
        }

        if from.is_human_readable() {
            from.deserialize_any(Of)
        } else {
            match u8::deserialize(from)? {
                u8::MAX => Ok(Decimals::Auto),
                count => Of.visit_u64(count.into()),
            }
        }
    }
}

/// What dragging with a mouse button does in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Drag {
    Orbit,
    Pan,
    Zoom,
    Nothing,
}

/// How dragging turns the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    /// About the world's Z, and up and down short of over the top.
    #[default]
    Turntable,
    /// The same, but on over the top, upside down.
    OverTheTop,
    /// Free, about the view's own axes, so the view can roll.
    Trackball,
}

/// What the view orbits about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RotationPoint {
    /// The point last clicked, see [`SetRotationPoint`].
    #[default]
    Clicked,
    /// The point under the pointer where each drag starts.
    Pointer,
    /// The middle of the view.
    Centre,
}

/// The click that sets the point the view orbits about, with
/// [`RotationPoint::Clicked`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetRotationPoint {
    #[default]
    MiddleClick,
    MiddleDoubleClick,
    ShiftMiddleClick,
}

/// Which way of the wheel zooms in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomDirection {
    #[default]
    WheelUp,
    WheelDown,
}

/// How long after the first change not auto-saved an open design is
/// auto-saved, if it is: natively only. Stored as the seconds, `60`, 0
/// for off; the pauses of before (3, 10, 30) aren't among them, so read
/// as the default, while off stays off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub enum AutoSave {
    Off,
    #[default]
    Minute,
    Minutes5,
    Minutes10,
}

impl AutoSave {
    pub const ALL: [AutoSave; 4] = [
        AutoSave::Off,
        AutoSave::Minute,
        AutoSave::Minutes5,
        AutoSave::Minutes10,
    ];

    /// The wait in minutes, 0 for off.
    pub fn minutes(self) -> u16 {
        match self {
            AutoSave::Off => 0,
            AutoSave::Minute => 1,
            AutoSave::Minutes5 => 5,
            AutoSave::Minutes10 => 10,
        }
    }

    /// The wait in seconds, as stored, 0 for off.
    pub fn seconds(self) -> u16 {
        match self {
            AutoSave::Off => 0,
            AutoSave::Minute => 60,
            AutoSave::Minutes5 => 300,
            AutoSave::Minutes10 => 600,
        }
    }
}

impl TryFrom<u16> for AutoSave {
    type Error = &'static str;

    fn try_from(seconds: u16) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|wait| wait.seconds() == seconds)
            .ok_or("not an auto-save wait")
    }
}

impl From<AutoSave> for u16 {
    fn from(wait: AutoSave) -> u16 {
        wait.seconds()
    }
}

/// How many files the recent files list keeps, at most
/// [`recent::MAX`](crate::recent::MAX). Stored as the count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub enum RecentDesigns {
    Ten,
    TwentyFive,
    Fifty,
    #[default]
    Hundred,
}

impl RecentDesigns {
    pub const ALL: [RecentDesigns; 4] = [
        RecentDesigns::Ten,
        RecentDesigns::TwentyFive,
        RecentDesigns::Fifty,
        RecentDesigns::Hundred,
    ];

    pub fn count(self) -> u16 {
        match self {
            RecentDesigns::Ten => 10,
            RecentDesigns::TwentyFive => 25,
            RecentDesigns::Fifty => 50,
            RecentDesigns::Hundred => 100,
        }
    }
}

impl TryFrom<u16> for RecentDesigns {
    type Error = &'static str;

    fn try_from(count: u16) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|kept| kept.count() == count)
            .ok_or("not a count of recent designs")
    }
}

impl From<RecentDesigns> for u16 {
    fn from(kept: RecentDesigns) -> u16 {
        kept.count()
    }
}

/// The port the MCP server listens on on `127.0.0.1`: from
/// [`McpPort::MIN`] to [`McpPort::MAX`], [`McpPort::DEFAULT`] unless
/// chosen. Stored as the number; one outside the range, as a port under
/// 1024 (privileged on Unix) or past `u16`, reads as the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct McpPort(u16);

impl McpPort {
    pub const MIN: u16 = 1024;
    pub const MAX: u16 = u16::MAX;
    pub const DEFAULT: u16 = 7311;

    /// `text` as a port, if it's digits alone (no sign or spaces) making
    /// one in the range offered.
    pub fn parse(text: &str) -> Option<McpPort> {
        if text.is_empty() || text.len() > 5 || !text.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        text.parse::<u16>().ok().and_then(Self::new)
    }

    /// `port`, if it's in the range offered.
    pub fn new(port: u16) -> Option<McpPort> {
        (Self::MIN..=Self::MAX)
            .contains(&port)
            .then_some(McpPort(port))
    }

    pub fn get(self) -> u16 {
        self.0
    }
}

impl Default for McpPort {
    fn default() -> Self {
        McpPort(Self::DEFAULT)
    }
}

impl TryFrom<u16> for McpPort {
    type Error = &'static str;

    fn try_from(port: u16) -> Result<Self, Self::Error> {
        Self::new(port).ok_or("not a port from 1024 to 65535")
    }
}

impl From<McpPort> for u16 {
    fn from(port: McpPort) -> u16 {
        port.get()
    }
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
        let defaults = Self::default();
        // Each key read on its own: one missing or gone bad gets its
        // default. The key is the field's name, as `serialize` writes it.
        macro_rules! read {
            ($($field:ident),* $(,)?) => {
                Self {
                    $($field: table
                        .get(stringify!($field))
                        .cloned()
                        .and_then(|value| value.try_into().ok())
                        .unwrap_or(defaults.$field),)*
                    length_unit: table
                        .get("length_unit")
                        .and_then(|value| value.as_str())
                        .and_then(length_unit_of)
                        .unwrap_or(defaults.length_unit),
                }
            };
        }
        read!(
            theme,
            mouse_hints,
            hidden_edges,
            names,
            projection,
            shading,
            edges,
            interface_scale,
            angle_unit,
            decimals,
            left_drag,
            middle_drag,
            right_drag,
            rotation,
            rotation_point,
            set_rotation_point,
            zoom_direction,
            zoom_to_pointer,
            sketch_snap,
            auto_save,
            recent_designs,
            mcp,
            mcp_port,
        )
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
    crate::Stores {
        settings: store(),
        ..crate::Stores::default()
    }
    .read_settings_now()
}

/// On the web, `settings.toml` at the root of the Origin Private File
/// System.
#[cfg(target_arch = "wasm32")]
pub fn store() -> Option<PathBuf> {
    Some(PathBuf::from("settings.toml"))
}

#[cfg(test)]
mod tests;
