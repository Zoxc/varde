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

/// The settings `stored` holds.
pub(crate) fn options_of(stored: Stored) -> ViewOptions {
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
        interface_scale: stored.interface_scale.percent(),
        angle_unit: match stored.angle_unit {
            StoredAngleUnit::Deg => AngleUnit::Deg,
            StoredAngleUnit::Rad => AngleUnit::Rad,
        },
        decimals: match stored.decimals {
            Decimals::Auto => None,
            Decimals::Places(count) => Some(count),
        },
        mouse: MouseOptions {
            left: drag_of(stored.left_drag),
            middle: drag_of(stored.middle_drag),
            right: drag_of(stored.right_drag),
            rotation: match stored.rotation {
                StoredRotation::Turntable => Rotation::Turntable,
                StoredRotation::OverTheTop => Rotation::OverTheTop,
                StoredRotation::Trackball => Rotation::Trackball,
            },
            rotation_point: match stored.rotation_point {
                StoredRotationPoint::Clicked => RotationPoint::Clicked,
                StoredRotationPoint::Pointer => RotationPoint::Pointer,
                StoredRotationPoint::Centre => RotationPoint::Centre,
            },
            pivot_click: match stored.set_rotation_point {
                SetRotationPoint::MiddleClick => PivotClick::Middle,
                SetRotationPoint::MiddleDoubleClick => PivotClick::MiddleDouble,
                SetRotationPoint::ShiftMiddleClick => PivotClick::ShiftMiddle,
            },
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

fn drag_of(stored: Drag) -> DragAction {
    match stored {
        Drag::Orbit => DragAction::Orbit,
        Drag::Pan => DragAction::Pan,
        Drag::Zoom => DragAction::Zoom,
        Drag::Nothing => DragAction::Nothing,
    }
}

fn stored_drag(drag: DragAction) -> Drag {
    match drag {
        DragAction::Orbit => Drag::Orbit,
        DragAction::Pan => Drag::Pan,
        DragAction::Zoom => Drag::Zoom,
        DragAction::Nothing => Drag::Nothing,
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
        // Values the page doesn't offer, which it never sets, are stored
        // as the defaults.
        interface_scale: InterfaceScale::new(options.interface_scale).unwrap_or_default(),
        angle_unit: match options.angle_unit {
            AngleUnit::Deg => StoredAngleUnit::Deg,
            AngleUnit::Rad => StoredAngleUnit::Rad,
        },
        decimals: options
            .decimals
            .and_then(Decimals::places)
            .unwrap_or_default(),
        left_drag: stored_drag(options.mouse.left),
        middle_drag: stored_drag(options.mouse.middle),
        right_drag: stored_drag(options.mouse.right),
        rotation: match options.mouse.rotation {
            Rotation::Turntable => StoredRotation::Turntable,
            Rotation::OverTheTop => StoredRotation::OverTheTop,
            Rotation::Trackball => StoredRotation::Trackball,
        },
        rotation_point: match options.mouse.rotation_point {
            RotationPoint::Clicked => StoredRotationPoint::Clicked,
            RotationPoint::Pointer => StoredRotationPoint::Pointer,
            RotationPoint::Centre => StoredRotationPoint::Centre,
        },
        set_rotation_point: match options.mouse.pivot_click {
            PivotClick::Middle => SetRotationPoint::MiddleClick,
            PivotClick::MiddleDouble => SetRotationPoint::MiddleDoubleClick,
            PivotClick::ShiftMiddle => SetRotationPoint::ShiftMiddleClick,
        },
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
