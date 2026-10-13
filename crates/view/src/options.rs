//! The user's settings as the app keeps them ([`ViewOptions`]), and the
//! bounds of those the settings page offers and the app checks when
//! reading them.

use varde_expr::LengthUnit;
use varde_render::{Projection, Shading};

use crate::{MouseOptions, ThemeChoice};

/// The user's settings: what the view options menu turns on and off and
/// the settings page sets, kept by the app for every document. Both hints
/// and hidden edges on to start with, the edges, shading and projection
/// the defaults, the theme the system's and new designs in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewOptions {
    /// Whether the status bar shows the hints of the mouse.
    pub mouse_hints: bool,
    /// Whether the viewport shows the edges the model hides, dashed.
    pub hidden_edges: bool,
    /// Whether the viewport shows the names overlay.
    pub names: bool,
    /// Which of the model's edges the viewport draws.
    pub edges: Edges,
    /// How the viewport lights the model's faces.
    pub shading: Shading,
    /// How the viewport projects the model: every document's camera
    /// takes it.
    pub projection: Projection,
    pub theme: ThemeChoice,
    /// The units a new design starts in. A design opened keeps its own.
    pub length_unit: LengthUnit,
    /// How large the UI is drawn, the model aside, in percent of the
    /// system's scale: a multiple of [`INTERFACE_SCALE_STEP`] from
    /// [`INTERFACE_SCALE_MIN`] to [`INTERFACE_SCALE_MAX`], see
    /// [`interface_scale_offered`].
    pub interface_scale: u16,
    /// How angles are shown, in every design: see [`varde_expr::Shown`].
    pub angle_unit: varde_expr::AngleUnit,
    /// The most decimal places values are shown with, in every design,
    /// up to [`varde_expr::Shown::MAX_DECIMALS`], or `None` for each
    /// unit's own: see [`varde_expr::Shown`].
    pub decimals: Option<u8>,
    /// What the mouse does in the viewport.
    pub mouse: MouseOptions,
    /// The minutes after the first change not auto-saved that a design
    /// is auto-saved, one of [`AUTO_SAVE_MINUTES`], 0 for never: natively
    /// only.
    pub auto_save: u16,
    /// How many recent designs are kept, one of [`RECENT_DESIGNS`]:
    /// natively only.
    pub recent_designs: u16,
    /// Whether the MCP feature is on (*MCP*: its panel and toggle, not
    /// the server, which is started from the panel): natively only.
    pub mcp: bool,
    /// The port the MCP server listens on, from [`MCP_PORT_MIN`] to
    /// [`MCP_PORT_MAX`]: natively only.
    pub mcp_port: u16,
}

/// The lowest port the MCP server may listen on: those below are
/// privileged on Unix.
pub const MCP_PORT_MIN: u16 = 1024;
/// The highest port the MCP server may listen on.
pub const MCP_PORT_MAX: u16 = u16::MAX;
/// The port the MCP server listens on unless another is chosen.
pub const MCP_PORT_DEFAULT: u16 = 7311;

/// The smallest interface scale the settings offer, in percent.
pub const INTERFACE_SCALE_MIN: u16 = 75;
/// The largest interface scale the settings offer, in percent.
pub const INTERFACE_SCALE_MAX: u16 = 200;
/// The steps the interface scale's slider takes, in percent.
pub const INTERFACE_SCALE_STEP: u16 = 5;

/// Whether `percent` is an interface scale the settings offer.
pub fn interface_scale_offered(percent: u16) -> bool {
    (INTERFACE_SCALE_MIN..=INTERFACE_SCALE_MAX).contains(&percent)
        && percent.is_multiple_of(INTERFACE_SCALE_STEP)
}

/// The auto-save waits the settings offer, in minutes, 0 for off.
pub const AUTO_SAVE_MINUTES: [u16; 4] = [0, 1, 5, 10];
/// The counts of recent designs the settings offer.
pub const RECENT_DESIGNS: [u16; 4] = [10, 25, 50, 100];

impl ViewOptions {
    /// How values are shown in these settings.
    pub fn shown(&self) -> varde_expr::Shown {
        varde_expr::Shown {
            angle: self.angle_unit,
            decimals: self.decimals,
        }
    }

    /// The interface scale as a factor, 1 for 100 %.
    pub fn scale_factor(&self) -> f32 {
        f32::from(self.interface_scale) / 100.0
    }
}

impl Default for ViewOptions {
    fn default() -> Self {
        ViewOptions {
            mouse_hints: true,
            hidden_edges: true,
            names: false,
            edges: Edges::Default,
            shading: Shading::Regular,
            projection: Projection::default(),
            theme: ThemeChoice::default(),
            length_unit: LengthUnit::default(),
            interface_scale: 100,
            angle_unit: varde_expr::AngleUnit::Deg,
            decimals: None,
            mouse: MouseOptions::default(),
            auto_save: 1,
            recent_designs: 100,
            mcp: false,
            mcp_port: MCP_PORT_DEFAULT,
        }
    }
}

/// Which of the model's edges the viewport draws, besides the feature
/// edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edges {
    /// Only the feature edges.
    #[default]
    Default,
    /// Every patch's edges too, faint: [`varde_render::Frame::wireframe`].
    Wireframe,
    /// Every triangle's edges too, faint:
    /// [`varde_render::Frame::tessellation`].
    Tessellation,
}
