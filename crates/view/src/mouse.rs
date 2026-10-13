//! What the mouse does in the viewport, as the settings page's Mouse tab
//! sets it: see [`MouseOptions`].

pub use varde_render::Rotation;

/// What dragging with a mouse button does in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragAction {
    Orbit,
    Pan,
    Zoom,
    Nothing,
}

impl DragAction {
    pub const ALL: [DragAction; 4] = [
        DragAction::Orbit,
        DragAction::Pan,
        DragAction::Zoom,
        DragAction::Nothing,
    ];

    /// The hint for a drag of it: "Drag to orbit". `None` for
    /// [`DragAction::Nothing`].
    pub(crate) fn drag_said(self) -> Option<&'static str> {
        match self {
            DragAction::Orbit => Some("Drag to orbit"),
            DragAction::Pan => Some("Drag to pan"),
            DragAction::Zoom => Some("Drag to zoom"),
            DragAction::Nothing => None,
        }
    }
}

/// What the view orbits about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RotationPoint {
    /// The point last clicked on the model or grid, with
    /// [`MouseOptions::pivot_click`] (`Look::SetPivot`), else the view's
    /// middle.
    #[default]
    Clicked,
    /// The point on the model or grid under the pointer where each drag
    /// starts, else the view's middle.
    Pointer,
    /// The view's middle always.
    Centre,
}

impl RotationPoint {
    pub const ALL: [RotationPoint; 3] = [
        RotationPoint::Clicked,
        RotationPoint::Pointer,
        RotationPoint::Centre,
    ];
}

/// The click that sets the point the view orbits about, with
/// [`RotationPoint::Clicked`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PivotClick {
    /// Any middle click.
    #[default]
    Middle,
    /// A middle double-click.
    MiddleDouble,
    /// A middle click with `Shift` held.
    ShiftMiddle,
}

impl PivotClick {
    pub const ALL: [PivotClick; 3] = [
        PivotClick::Middle,
        PivotClick::MiddleDouble,
        PivotClick::ShiftMiddle,
    ];
}

/// The mouse's settings. The defaults are how the viewport has always
/// taken the mouse: the left button orbits outside a sketch, the middle
/// one orbits, the right one pans (orbits with `Shift`), a middle click
/// sets the point orbited about, the wheel zooms in turned up, towards
/// the pointer, and sketch tools snap unless `Shift` is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseOptions {
    /// Outside a sketch: in one the left button is the sketch's.
    pub left: DragAction,
    pub middle: DragAction,
    /// With [`Held::ORBIT`](crate::Held::ORBIT) held, the
    /// right button orbits whatever this says.
    pub right: DragAction,
    pub rotation: Rotation,
    pub rotation_point: RotationPoint,
    pub pivot_click: PivotClick,
    /// Whether turning the wheel down, rather than up, zooms in.
    pub wheel_down_zooms_in: bool,
    /// Whether the wheel zooms about the point under the pointer, rather
    /// than the view's middle.
    pub zoom_to_pointer: bool,
    /// Whether sketch tools snap unless
    /// [`Held::FREE`](crate::Held::FREE) is held; if not, they
    /// snap only while it is.
    pub sketch_snap: bool,
}

impl Default for MouseOptions {
    fn default() -> Self {
        MouseOptions {
            left: DragAction::Orbit,
            middle: DragAction::Orbit,
            right: DragAction::Pan,
            rotation: Rotation::Turntable,
            rotation_point: RotationPoint::Clicked,
            pivot_click: PivotClick::Middle,
            wheel_down_zooms_in: false,
            zoom_to_pointer: true,
            sketch_snap: true,
        }
    }
}
