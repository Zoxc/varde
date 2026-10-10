//! The user's settings, as stored: the theme chosen and the interface
//! scale, the view options (the status bar's menu's, on the settings page
//! too), the units new designs start in and how values are shown, what
//! the mouse does, and natively the auto-save pause and how many recent
//! designs are kept.
//!
//! The IO lane stores them (see [`varde_io::settings`]) and hands them over
//! once read at startup; until then they're the defaults, the system's
//! theme, the hints shown and so on. The app keeps them in its
//! [`ViewOptions`], and this what's stored of them.

use varde_expr::AngleUnit;
use varde_io::Request as IoRequest;
use varde_io::settings::{
    AngleUnit as StoredAngleUnit, AutoSave, Decimals, Drag, Edges as StoredEdges, InterfaceScale,
    Projection as StoredProjection, RecentDesigns, Rotation as StoredRotation,
    RotationPoint as StoredRotationPoint, SetRotationPoint, Settings as Stored,
    Shading as StoredShading, Theme, ZoomDirection,
};
use varde_render::{Projection, Rotation, Shading};
use varde_view::{
    DragAction, Edges, MouseOptions, PivotClick, RotationPoint, ThemeChoice, ViewOptions,
};

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
    InterfaceScale,
    AngleUnit,
    Decimals,
    LeftDrag,
    MiddleDrag,
    RightDrag,
    Rotation,
    RotationPoint,
    PivotClick,
    ZoomDirection,
    ZoomToPointer,
    SketchSnap,
    AutoSave,
    RecentDesigns,
}

impl Key {
    pub(crate) const ALL: [Key; 22] = [
        Key::Theme,
        Key::MouseHints,
        Key::HiddenEdges,
        Key::Names,
        Key::Projection,
        Key::Shading,
        Key::Edges,
        Key::LengthUnit,
        Key::InterfaceScale,
        Key::AngleUnit,
        Key::Decimals,
        Key::LeftDrag,
        Key::MiddleDrag,
        Key::RightDrag,
        Key::Rotation,
        Key::RotationPoint,
        Key::PivotClick,
        Key::ZoomDirection,
        Key::ZoomToPointer,
        Key::SketchSnap,
        Key::AutoSave,
        Key::RecentDesigns,
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
            Key::InterfaceScale => to.interface_scale = from.interface_scale,
            Key::AngleUnit => to.angle_unit = from.angle_unit,
            Key::Decimals => to.decimals = from.decimals,
            Key::LeftDrag => to.mouse.left = from.mouse.left,
            Key::MiddleDrag => to.mouse.middle = from.mouse.middle,
            Key::RightDrag => to.mouse.right = from.mouse.right,
            Key::Rotation => to.mouse.rotation = from.mouse.rotation,
            Key::RotationPoint => to.mouse.rotation_point = from.mouse.rotation_point,
            Key::PivotClick => to.mouse.pivot_click = from.mouse.pivot_click,
            Key::ZoomDirection => to.mouse.wheel_down_zooms_in = from.mouse.wheel_down_zooms_in,
            Key::ZoomToPointer => to.mouse.zoom_to_pointer = from.mouse.zoom_to_pointer,
            Key::SketchSnap => to.mouse.sketch_snap = from.mouse.sketch_snap,
            Key::AutoSave => to.auto_save = from.auto_save,
            Key::RecentDesigns => to.recent_designs = from.recent_designs,
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
        auto_save: stored.auto_save.seconds(),
        recent_designs: stored.recent_designs.count(),
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
        interface_scale: InterfaceScale::try_from(options.interface_scale).unwrap_or_default(),
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
        auto_save: AutoSave::try_from(options.auto_save).unwrap_or_default(),
        recent_designs: RecentDesigns::try_from(options.recent_designs).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests;
