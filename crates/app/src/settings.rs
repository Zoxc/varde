//! The user's settings, as stored: the theme chosen and the interface
//! scale, the view options (the status bar's menu's, on the settings page
//! too), the units new designs start in and how values are shown, what
//! the mouse does, and natively the auto-save pause, how many recent
//! designs are kept, and whether the MCP feature is on and the port its
//! server uses.
//!
//! The IO lane stores them (see [`varde_io::settings`]) and hands them over
//! once read at startup; until then they're the defaults, the system's
//! theme, the hints shown and so on. The app keeps them in its
//! [`ViewOptions`], and this what's stored of them.

use varde_expr::AngleUnit;
use varde_io::Request as IoRequest;
use varde_io::settings::{
    AngleUnit as StoredAngleUnit, AutoSave, Decimals, Drag, Edges as StoredEdges, InterfaceScale,
    McpPort, Projection as StoredProjection, RecentDesigns, Rotation as StoredRotation,
    RotationPoint as StoredRotationPoint, SetRotationPoint, Settings as Stored,
    Shading as StoredShading, Theme, ZoomDirection,
};
use varde_render::{Projection, Rotation, Shading};
use varde_view::{
    DragAction, Edges, MouseOptions, PivotClick, RotationPoint, Setting, ThemeChoice, ViewOptions,
};

#[derive(Debug, Default)]
pub(crate) struct Settings {
    /// Whether the stored settings have arrived.
    loaded: bool,
    /// The settings the user chose before they did, by
    /// [`Setting::index`]: theirs win.
    chosen: [bool; Setting::COUNT],
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
        for (setting, chosen) in Setting::of(&stored).into_iter().zip(self.chosen) {
            if !chosen {
                setting.apply(options);
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

    /// Records that the user chose `setting`, as `options` have it,
    /// returning the write storing it, once the stored settings have
    /// arrived: before, writing would drop what else they hold.
    #[must_use = "the write stores the settings"]
    pub(crate) fn chose(&mut self, setting: Setting, options: ViewOptions) -> Option<IoRequest> {
        if let Some(chosen) = self.chosen.get_mut(setting.index()) {
            *chosen = true;
        }
        self.write(options)
    }

    /// Records that the user chose every setting of `options`, as Reset
    /// to defaults does, as [`Settings::chose`].
    #[must_use = "the write stores the settings"]
    pub(crate) fn chose_all(&mut self, options: ViewOptions) -> Option<IoRequest> {
        self.chosen = [true; Setting::COUNT];
        self.write(options)
    }

    fn write(&self, options: ViewOptions) -> Option<IoRequest> {
        self.loaded.then(|| IoRequest::WriteSettings {
            settings: stored_of(options),
        })
    }
}

/// Declares each pair of a stored enum and its options twin once, as
/// `fn of(Stored) -> Options / fn stored { Stored = Options, .. }`, and
/// makes both directions' functions from it, so the two can't disagree.
macro_rules! mirror {
    ($(fn $of:ident($stored:ident) -> $shown:ident / fn $back:ident {
        $($a:ident = $b:ident),+ $(,)?
    })*) => {$(
        fn $of(stored: $stored) -> $shown {
            match stored {
                $($stored::$a => $shown::$b,)+
            }
        }

        fn $back(shown: $shown) -> $stored {
            match shown {
                $($shown::$b => $stored::$a,)+
            }
        }
    )*};
}

mirror! {
    fn theme_of(Theme) -> ThemeChoice / fn stored_theme {
        Auto = Auto, Light = Light, Dark = Dark,
    }
    fn projection_of(StoredProjection) -> Projection / fn stored_projection {
        Orthographic = Orthographic, Perspective = Perspective,
    }
    fn shading_of(StoredShading) -> Shading / fn stored_shading {
        Regular = Regular, Flat = Flat, Metal = Metal, FlatMetal = FlatMetal,
    }
    fn edges_of(StoredEdges) -> Edges / fn stored_edges {
        Default = Default, Wireframe = Wireframe, Tessellation = Tessellation,
    }
    fn angle_unit_of(StoredAngleUnit) -> AngleUnit / fn stored_angle_unit {
        Deg = Deg, Rad = Rad,
    }
    fn drag_of(Drag) -> DragAction / fn stored_drag {
        Orbit = Orbit, Pan = Pan, Zoom = Zoom, Nothing = Nothing,
    }
    fn rotation_of(StoredRotation) -> Rotation / fn stored_rotation {
        Turntable = Turntable, OverTheTop = OverTheTop, Trackball = Trackball,
    }
    fn rotation_point_of(StoredRotationPoint) -> RotationPoint / fn stored_rotation_point {
        Clicked = Clicked, Pointer = Pointer, Centre = Centre,
    }
    fn pivot_click_of(SetRotationPoint) -> PivotClick / fn stored_pivot_click {
        MiddleClick = Middle, MiddleDoubleClick = MiddleDouble, ShiftMiddleClick = ShiftMiddle,
    }
}

/// The settings `stored` holds.
pub(crate) fn options_of(stored: Stored) -> ViewOptions {
    ViewOptions {
        theme: theme_of(stored.theme),
        mouse_hints: stored.mouse_hints,
        hidden_edges: stored.hidden_edges,
        names: stored.names,
        projection: projection_of(stored.projection),
        shading: shading_of(stored.shading),
        edges: edges_of(stored.edges),
        length_unit: stored.length_unit,
        interface_scale: stored.interface_scale.percent(),
        angle_unit: angle_unit_of(stored.angle_unit),
        decimals: match stored.decimals {
            Decimals::Auto => None,
            Decimals::Places(count) => Some(count),
        },
        mouse: MouseOptions {
            left: drag_of(stored.left_drag),
            middle: drag_of(stored.middle_drag),
            right: drag_of(stored.right_drag),
            rotation: rotation_of(stored.rotation),
            rotation_point: rotation_point_of(stored.rotation_point),
            pivot_click: pivot_click_of(stored.set_rotation_point),
            wheel_down_zooms_in: stored.zoom_direction == ZoomDirection::WheelDown,
            zoom_to_pointer: stored.zoom_to_pointer,
            sketch_snap: stored.sketch_snap,
        },
        auto_save: stored.auto_save.minutes(),
        recent_designs: stored.recent_designs.count(),
        mcp: stored.mcp,
        mcp_port: stored.mcp_port.get(),
    }
}

/// What's stored of `options`.
pub(crate) fn stored_of(options: ViewOptions) -> Stored {
    Stored {
        theme: stored_theme(options.theme),
        mouse_hints: options.mouse_hints,
        hidden_edges: options.hidden_edges,
        names: options.names,
        projection: stored_projection(options.projection),
        shading: stored_shading(options.shading),
        edges: stored_edges(options.edges),
        length_unit: options.length_unit,
        // Values the page doesn't offer, which it never sets, are stored
        // as the defaults.
        interface_scale: InterfaceScale::new(options.interface_scale).unwrap_or_default(),
        angle_unit: stored_angle_unit(options.angle_unit),
        decimals: options
            .decimals
            .and_then(Decimals::places)
            .unwrap_or_default(),
        left_drag: stored_drag(options.mouse.left),
        middle_drag: stored_drag(options.mouse.middle),
        right_drag: stored_drag(options.mouse.right),
        rotation: stored_rotation(options.mouse.rotation),
        rotation_point: stored_rotation_point(options.mouse.rotation_point),
        set_rotation_point: stored_pivot_click(options.mouse.pivot_click),
        zoom_direction: if options.mouse.wheel_down_zooms_in {
            ZoomDirection::WheelDown
        } else {
            ZoomDirection::WheelUp
        },
        zoom_to_pointer: options.mouse.zoom_to_pointer,
        sketch_snap: options.mouse.sketch_snap,
        auto_save: AutoSave::ALL
            .into_iter()
            .find(|wait| wait.minutes() == options.auto_save)
            .unwrap_or_default(),
        recent_designs: RecentDesigns::try_from(options.recent_designs).unwrap_or_default(),
        mcp: options.mcp,
        mcp_port: McpPort::new(options.mcp_port).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests;
