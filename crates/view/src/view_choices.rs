//! The settings' enumerated values (theme, projection, shading, edges,
//! units, mouse actions): their order, labels and icons, shared by the
//! settings page and the status bar's View menu so the two name them
//! alike.

use varde_expr::{AngleUnit, LengthUnit};
use varde_render::{Projection, Rotation, Shading};

use crate::icons::Icon;
use crate::mouse::{DragAction, PivotClick, RotationPoint};
use crate::{Edges, ThemeChoice};

/// A setting's value chosen from a few: offered in [`Choice::ALL`]'s
/// order, each named and maybe drawn.
pub(crate) trait Choice: Copy + PartialEq + 'static {
    /// The values offered, in order.
    const ALL: &'static [Self];

    /// Its name, as a segment, a menu item and Reset to defaults say it.
    fn said(self) -> &'static str;
}

/// A [`Choice`] drawn beside its name.
pub(crate) trait Pictured: Choice {
    fn icon(self) -> Icon;
}

impl Choice for ThemeChoice {
    const ALL: &'static [Self] = &ThemeChoice::ALL;

    fn said(self) -> &'static str {
        match self {
            ThemeChoice::Auto => "System",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }
}

impl Pictured for ThemeChoice {
    fn icon(self) -> Icon {
        match self {
            ThemeChoice::Auto => Icon::Contrast,
            ThemeChoice::Light => Icon::Sun,
            ThemeChoice::Dark => Icon::Moon,
        }
    }
}

impl Choice for Projection {
    const ALL: &'static [Self] = &[Projection::Orthographic, Projection::Perspective];

    fn said(self) -> &'static str {
        match self {
            Projection::Orthographic => "Orthographic",
            Projection::Perspective => "Perspective",
        }
    }
}

impl Pictured for Projection {
    fn icon(self) -> Icon {
        match self {
            Projection::Orthographic => Icon::Ortho,
            Projection::Perspective => Icon::Persp,
        }
    }
}

impl Choice for Shading {
    const ALL: &'static [Self] = &[
        Shading::Regular,
        Shading::Flat,
        Shading::Metal,
        Shading::FlatMetal,
    ];

    fn said(self) -> &'static str {
        match self {
            Shading::Regular => "Regular",
            Shading::Flat => "Flat",
            Shading::Metal => "Metal",
            Shading::FlatMetal => "Flat metal",
        }
    }
}

impl Pictured for Shading {
    fn icon(self) -> Icon {
        match self {
            Shading::Regular => Icon::ShadeRegular,
            Shading::Flat => Icon::ShadeFlat,
            Shading::Metal => Icon::ShadeMetal,
            Shading::FlatMetal => Icon::ShadeFlatMetal,
        }
    }
}

impl Choice for Edges {
    const ALL: &'static [Self] = &[Edges::Default, Edges::Wireframe, Edges::Tessellation];

    fn said(self) -> &'static str {
        match self {
            Edges::Default => "Features",
            Edges::Wireframe => "Patches",
            Edges::Tessellation => "Triangles",
        }
    }
}

impl Pictured for Edges {
    fn icon(self) -> Icon {
        match self {
            Edges::Default => Icon::EdgesDefault,
            Edges::Wireframe => Icon::EdgesWireframe,
            Edges::Tessellation => Icon::EdgesTessellation,
        }
    }
}

impl Choice for LengthUnit {
    const ALL: &'static [Self] = &LengthUnit::ALL;

    fn said(self) -> &'static str {
        self.symbol()
    }
}

impl Choice for AngleUnit {
    const ALL: &'static [Self] = &AngleUnit::ALL;

    fn said(self) -> &'static str {
        match self {
            AngleUnit::Deg => "Degrees",
            AngleUnit::Rad => "Radians",
        }
    }
}

impl Choice for DragAction {
    const ALL: &'static [Self] = &DragAction::ALL;

    fn said(self) -> &'static str {
        match self {
            DragAction::Orbit => "Orbit",
            DragAction::Pan => "Pan",
            DragAction::Zoom => "Zoom",
            DragAction::Nothing => "Nothing",
        }
    }
}

impl Choice for Rotation {
    const ALL: &'static [Self] = &[
        Rotation::Turntable,
        Rotation::OverTheTop,
        Rotation::Trackball,
    ];

    fn said(self) -> &'static str {
        match self {
            Rotation::Turntable => "Turntable",
            Rotation::OverTheTop => "Turntable, over the top",
            Rotation::Trackball => "Trackball",
        }
    }
}

impl Choice for RotationPoint {
    const ALL: &'static [Self] = &RotationPoint::ALL;

    fn said(self) -> &'static str {
        match self {
            RotationPoint::Clicked => "Clicked",
            RotationPoint::Pointer => "Under the pointer",
            RotationPoint::Centre => "View centre",
        }
    }
}

impl Choice for PivotClick {
    const ALL: &'static [Self] = &PivotClick::ALL;

    fn said(self) -> &'static str {
        match self {
            PivotClick::Middle => "Middle click",
            PivotClick::MiddleDouble => "Middle double-click",
            PivotClick::ShiftMiddle => "Shift + middle click",
        }
    }
}
