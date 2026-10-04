//! Stroke icons, and the logo for the window icon.
//!
//! The tools' icons are the default set of `notes/ui-mock-icons.html`, its
//! "outline with accents, line only" drawings, and the others the `I` table
//! of `notes/ui-mock/mock.js`. Icons are 24×24 stroke drawings. A tool's icon
//! has up to three layers, its own geometry, its accent marks and the
//! reference geometry it works from, each tinted through [`svg::Style`] in
//! its [`IconCategory`]'s [`IconTone`]; the others are drawn in one colour.

use std::sync::LazyLock;

use iced::widget::{Svg, stack, svg};
use iced::{Color, Element, Theme};

use crate::theme::{self, IconCategory, IconTone, Palette};

/// The logo: a folded "V" with a sketch point above it.
///
/// `assets/logo.svg` is the single source for the window icon and the
/// Windows `.ico`.
pub const LOGO_SVG: &str = include_str!("../../../assets/logo.svg");

/// Declares [`Icon`] and its SVG elements from one list, so each icon is
/// named once and [`Icon::ALL`] follows the declaration order.
macro_rules! icons {
    ($($name:ident => $paths:expr,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Icon {
            $($name,)*
        }

        impl Icon {
            /// Every icon, in declaration order, so `ALL[icon as usize] == icon`.
            pub(crate) const ALL: [Icon; [$(Icon::$name),*].len()] = [$(Icon::$name),*];

            /// The SVG elements, drawn in black. A tool's carry the icon
            /// mock's classes, which [`Layers::of`] sorts them by.
            fn paths(self) -> &'static str {
                match self {
                    $(Icon::$name => $paths,)*
                }
            }
        }
    };
}

icons! {
    Plus => r#"<path d="M12 5v14M5 12h14"/>"#,
    Folder => r#"<path d="M3 7a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z"/>"#,
    Chev => r#"<path d="M7 10l5 5 5-5"/>"#,
    Home => r#"<path d="M4 11l8-7 8 7"/><path d="M6 9.5V20h12V9.5"/>"#,
    Undo => r#"<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/>"#,
    Redo => r#"<path d="M15 14l5-5-5-5"/><path d="M20 9H10a6 6 0 0 0 0 12h3"/>"#,
    Search => r#"<circle cx="11" cy="11" r="6"/><path d="M20 20l-4.5-4.5"/>"#,
    Sun => r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#,
    Moon => r#"<path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/>"#,
    Contrast => r##"<circle cx="12" cy="12" r="8"/><path d="M12 4a8 8 0 0 1 0 16z" fill="#000"/>"##,
    Help => r##"<circle cx="12" cy="12" r="9"/><path d="M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .8-1 1.5V14"/><circle cx="12" cy="17" r=".6" fill="#000"/>"##,
    Close => r#"<path d="M6 6l12 12M18 6L6 18"/>"#,
    Body => r#"<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/>"#,
    Eye => r#"<path d="M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6z"/><circle cx="12" cy="12" r="2.5"/>"#,
    EyeOff => r#"<path d="M3 3l18 18"/><path d="M10.6 6.1A9 9 0 0 1 12 6c6 0 9.5 6 9.5 6a16 16 0 0 1-2.7 3.3M6.6 7.6C4 9.3 2.5 12 2.5 12s3.5 6 9.5 6a9 9 0 0 0 3.4-.7"/>"#,
    Trash => r#"<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>"#,
    Rollback => r#"<path d="M4 4v16"/><path d="M20 12H8M12 8l-4 4 4 4"/>"#,
    Save => r#"<path d="M5 4h11l3 3v13H5z"/><path class="a" d="M8 4v5h7V4M8 20v-6h8v6"/>"#,
    Export => r#"<path class="a" d="M12 15V3M8 7l4-4 4 4"/><path d="M4 16v4h16v-4"/>"#,
    Check => r#"<path d="M5 12.5l4.5 4.5L19 7"/>"#,
    // Not in the mocks: two sheets, for copying a value.
    Copy => r#"<rect x="8.5" y="8.5" width="11" height="11" rx="1.5"/><path d="M15.5 8.5V5.5a1 1 0 0 0-1-1h-9a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h3"/>"#,
    More => r##"<circle cx="5.5" cy="12" r=".9" fill="#000"/><circle cx="12" cy="12" r=".9" fill="#000"/><circle cx="18.5" cy="12" r=".9" fill="#000"/>"##,
    // Where a design is kept on the web, browser storage (a database's
    // drum) or a computer, and a download's arrow into a tray.
    Browser => r#"<ellipse cx="12" cy="6" rx="7" ry="2.8"/><path d="M5 6v12c0 1.5 3.1 2.8 7 2.8s7-1.3 7-2.8V6M5 12c0 1.5 3.1 2.8 7 2.8s7-1.3 7-2.8"/>"#,
    Computer => r#"<path d="M4 5h16v11H4zM2 19h20"/>"#,
    Download => r#"<path d="M12 3v12M8 11l4 4 4-4"/><path d="M4 16v4h16v-4"/>"#,
    // Not in the mocks: renaming, a name's field with the text cursor in it.
    Rename => r#"<path d="M12 8H5a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h7"/><path class="a" d="M16 5v14M14 5h4M14 19h4"/>"#,
    // The tools, from the icon mock.
    Sketch => r#"<path class="r" d="M4 20h16"/><path class="t" d="M14.5 4.5l5 5L9 20H4v-5z"/><path class="a" d="M6 13l5 5"/>"#,
    Extrude => r#"<path class="t" d="M4 15l8 4 8-4-8-4z"/><path class="a" d="M12 11V3M9 6l3-3 3 3"/>"#,
    Revolve => r#"<path d="M20 12a8 8 0 1 1-2.6-5.9"/><path class="a" d="M17.5 2.5v4h4"/><path class="r" d="M12 7v10" stroke-dasharray="2 2"/>"#,
    Plane => r#"<path class="r" d="M3 20l4-5h14l-4 5z"/><path class="t" d="M3 11l4-5h14l-4 5z"/><path class="a" d="M12 17.5V9.5M10 11.5l2-2 2 2"/>"#,
    Line => r#"<path d="M6 18L18 6"/><circle class="a" cx="5" cy="19" r="1.6"/><circle class="a" cx="19" cy="5" r="1.6"/>"#,
    Circle => r#"<circle class="t" cx="12" cy="12" r="8"/><path class="r" d="M12 12l5.66-5.66" stroke-dasharray="2 2"/><circle class="af" cx="12" cy="12" r="1.5"/><circle class="af" cx="17.66" cy="6.34" r="1.5"/>"#,
    Arc => r#"<path d="M4 18A9 9 0 0 1 20 18"/><circle class="af" cx="4" cy="18" r="1.5"/><circle class="af" cx="12" cy="10" r="1.5"/><circle class="af" cx="20" cy="18" r="1.5"/>"#,
    Rectangle => r#"<rect class="t" x="4" y="6" width="16" height="12" rx="1"/><circle class="af" cx="4" cy="6" r="1.5"/><circle class="af" cx="20" cy="18" r="1.5"/>"#,
    Polygon => r#"<path class="t" d="M12 3.5l8 5.8-3 9.2H7l-3-9.2z"/><path class="r" d="M12 12.2V5.5" stroke-dasharray="2 2"/><circle class="af" cx="12" cy="12.2" r="1.4"/><circle class="af" cx="12" cy="3.5" r="1.4"/>"#,
    Spline => r#"<path d="M3 17c3-9 6-9 9-5s6 4 9-5"/><circle class="af" cx="3" cy="17" r="1.4"/><circle class="af" cx="12" cy="12" r="1.4"/><circle class="af" cx="21" cy="7" r="1.4"/>"#,
    Point => r#"<path d="M7 7l3 3M17 7l-3 3M7 17l3-3M17 17l-3-3"/><circle class="af" cx="12" cy="12" r="2"/>"#,
    Constrain => r#"<rect class="t" x="5" y="11" width="14" height="9" rx="1.5"/><path class="a" d="M8 11V8a4 4 0 0 1 8 0v3"/>"#,
    Coincident => r#"<path d="M4 20L20 4M4 4l16 16"/><circle class="af" cx="12" cy="12" r="2.6"/>"#,
    Horizontal => r#"<path d="M7 5v14M17 5v14"/><path class="a" d="M7 12h10"/>"#,
    Vertical => r#"<path d="M5 5l7 14 7-14"/><path class="a" d="M12 4v9"/>"#,
    Parallel => r#"<path d="M6 20L12 4M13 20l6-16"/><path class="a" d="M9.5 7l2.5-3 1 3.7M16.5 7l2.5-3 1 3.7"/>"#,
    Perpendicular => r#"<path d="M4 20h16M12 20V5"/><path class="a" d="M12 15.5h4.5V20"/>"#,
    Tangent => r#"<circle cx="10" cy="13" r="6"/><path d="M3 7h18"/><circle class="af" cx="10" cy="7" r="2"/>"#,
    Smooth => r#"<path d="M3 18c6 0 9-12 18-12"/><path class="a" d="M7 17l-1.2-3M11 14l-2.4-2.2M15 10l-1.8-3"/>"#,
    Equal => r#"<path d="M5 9h14M5 15h14"/><path class="a" d="M11 6.5l2 5M11 12.5l2 5"/>"#,
    Concentric => r#"<circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="4.5"/><circle class="af" cx="12" cy="12" r="1.6"/>"#,
    Midpoint => r#"<path d="M3 17h18"/><path class="a ta" d="M12 16.5l-3.5-6h7z"/>"#,
    Symmetric => r#"<path class="r" d="M12 3v18" stroke-dasharray="2 2"/><circle cx="5.5" cy="12" r="2"/><circle cx="18.5" cy="12" r="2"/><path class="a" d="M8 9.5l1.5 2.5L8 14.5M16 9.5L14.5 12l1.5 2.5"/>"#,
    Fix => r#"<circle class="a ta" cx="12" cy="7" r="2.6"/><path d="M12 9.6V15M5 15h14"/><path class="r" d="M7.5 15l-2 4M11 15l-2 4M14.5 15l-2 4M18 15l-2 4"/>"#,
    Dimension => r#"<path class="r" d="M4 7v10M20 7v10"/><path d="M5 12h14"/><path class="a" d="M7.5 9.5L5 12l2.5 2.5M16.5 9.5L19 12l-2.5 2.5"/>"#,
    Trim => r#"<circle cx="6" cy="7" r="2.5"/><circle cx="6" cy="17" r="2.5"/><path class="a" d="M8.2 8.4L20 17M8.2 15.6L20 7"/>"#,
    Extend => r#"<path d="M3 19l7-7"/><path class="a" d="M10 12l6.5-6.5" stroke-dasharray="2 2.2"/><path class="r" d="M13 2l8 8"/><circle class="af" cx="16.5" cy="5.5" r="1.4"/>"#,
    Mirror => r#"<path class="r" d="M12 3v18" stroke-dasharray="2 2"/><path class="t" d="M9 9L3 19h6z"/><path d="M15 9l6 10h-6z"/><path class="a" d="M7 6.5C9 3 15 3 17 6.5M17.3 3.8L17 6.5l-2.6-.6"/>"#,
    Offset => r#"<path class="r" d="M4 20c2-7 7-12 16-13"/><path d="M3 13c2-4 6-7 11-7.8"/><path class="a" d="M10.5 14.5L8 11M7.5 13.3L8 11l2.4.2"/>"#,
    Fillet => r#"<path d="M5 20v-8M12 5h7"/><path class="r" d="M5 12V5h7" stroke-dasharray="1.6 2"/><path class="a" d="M5 12a7 7 0 0 1 7-7"/>"#,
    Chamfer => r#"<path d="M5 20v-9M13 5h6"/><path class="r" d="M5 11V5h8" stroke-dasharray="1.6 2"/><path class="a" d="M5 11l8-6"/>"#,
    Measure => r#"<path class="t" d="M3 17L17 3l4 4L7 21z"/><path class="a" d="M7 13l2 2M10 10l2 2M13 7l2 2"/>"#,
    // Two overlapping boxes; the mock's tinted overlap is a fill, which
    // the line-only set leaves out.
    Combine => r#"<rect x="3" y="3" width="11" height="11" rx="1.5"/><rect x="10" y="10" width="11" height="11" rx="1.5"/>"#,
    // The model mock's: a box with arrows four ways; a body mirrored
    // across a dashed plane.
    Move => r#"<rect class="t" x="9" y="9" width="6" height="6" rx="1"/><path class="a" d="M12 3v3.5M12 17.5V21M3 12h3.5M17.5 12H21M10 5l2-2 2 2M10 19l2 2 2-2M5 10l-2 2 2 2M19 10l2 2-2 2"/>"#,
    BMirror => r#"<path class="r" d="M12 3v18" stroke-dasharray="2 2"/><path class="t" d="M9 8L3.5 10.5v7L9 20z"/><path d="M15 8l5.5 2.5v7L15 20z"/><path class="a" d="M7 5.5C9 2.5 15 2.5 17 5.5M17.4 2.9L17 5.5l-2.6-.5"/>"#,
    // The mock's patterns: copies in a row along an arrow, the last
    // dashed; copies round a dashed circle, one dashed.
    LPattern => r#"<rect class="t" x="2.5" y="7" width="5" height="6" rx="1"/><rect x="9.5" y="7" width="5" height="6" rx="1"/><rect x="16.5" y="7" width="5" height="6" rx="1" stroke-dasharray="2 1.6"/><path class="a" d="M4 18h15M17 16l2 2-2 2"/>"#,
    CPattern => r#"<circle class="a" cx="12" cy="12" r="7.5" stroke-dasharray="2 2.2"/><rect class="t" x="10" y="2.5" width="4" height="4" rx="1"/><rect x="17.5" y="10" width="4" height="4" rx="1"/><rect x="10" y="17.5" width="4" height="4" rx="1"/><rect x="2.5" y="10" width="4" height="4" rx="1" stroke-dasharray="1.6 1.4"/>"#,
    // A box grown from a smaller one, an arrow out of its corner.
    Scale => r#"<rect class="r" x="3" y="11" width="10" height="10" rx="1"/><path class="a" d="M10 14l10-10M14 4h6v6"/>"#,
    // The icon mock's split body: a box cut by a dashed plane.
    // The model mock's chamfer: a block with its top edge cut off,
    // the new face in the accent.
    BChamfer => r#"<path d="M7.67 10L3.34 7.5v9L12 21.5v-4M3.34 7.5L12 2.5l4.33 2.5M12 21.5l8.66-5v-4"/><path class="a" d="M7.67 10L12 17.5M16.33 5l4.33 7.5M16.33 5L7.67 10M20.66 12.5L12 17.5"/>"#,
    Split => r#"<path class="t" d="M4 8l8-4 8 4v8l-8 4-8-4z"/><path class="a" d="M2 15L22 9" stroke-dasharray="2.5 2"/>"#,
    Align => r#"<path class="a" d="M4 3v18"/><rect class="t" x="7" y="5" width="13" height="5" rx="1"/><rect x="7" y="14" width="8" height="5" rx="1"/>"#,
    // Not in the icon mock: the offset constraint's nested squares, arrows
    // each way, a handle on a curve, a comb's teeth over one.
    OffsetConstraint => r#"<rect x="3" y="3" width="18" height="18" rx="2"/><rect x="8" y="8" width="8" height="8" rx="1"/>"#,
    Convert => r#"<path d="M4 8h14l-3-3M20 16H6l3 3"/>"#,
    Handles => r#"<path d="M3 19c3-8 8-12 18-13"/><path d="M4.5 9.5l10 4"/><circle cx="4.5" cy="9.5" r="1.5"/><circle cx="14.5" cy="13.5" r="1.5"/>"#,
    Comb => r#"<path d="M3 18c4-9 14-9 18 0"/><path d="M6.5 13.4L5 9.5M12 11.3V6.5M17.5 13.4L19 9.5"/>"#,
    // The tool sets on the rail, the mock's `cat-*`: what each leaves
    // behind.
    CatCreate => r#"<path class="r" d="M5 17.5a7 3 0 0 1 14 0" stroke-dasharray="1.6 1.8"/><path d="M5 6.5v11a7 3 0 0 0 14 0v-11"/><ellipse class="a" cx="12" cy="6.5" rx="7" ry="3"/>"#,
    CatDraw => r#"<path d="M5 20V10a7 7 0 0 1 14 0v10z"/><circle class="af" cx="5" cy="20" r="1.5"/><circle class="af" cx="19" cy="20" r="1.5"/><circle class="af" cx="12" cy="3" r="1.5"/>"#,
    CatModify => r#"<path d="M10.5 9H3.5v11h12v-6M3.5 9l5-5h7M20.5 9v6l-5 5M10.5 9l5-5M15.5 14l5-5"/><path class="a" d="M10.5 9a5 5 0 0 1 5 5M15.5 4a5 5 0 0 1 5 5"/>"#,
    CatSketchModify => r#"<path d="M2.5 9H15v12.5"/><path class="r" d="M19.5 9h2"/><path class="a" d="M17.25 5.5v7"/>"#,
    // The mock's group turned, each element turned instead, as an icon
    // is a list of empty tags.
    CatConstrain => r#"<rect transform="rotate(-18 12 12)" x="5" y="6" width="14" height="12" rx=".5"/><path class="a" transform="rotate(-18 12 12)" d="M5 13.5h4.5V18"/><circle class="af" transform="rotate(-18 12 12)" cx="19" cy="6" r="1.5"/>"#,
    CatTransform => r#"<path class="r" stroke-dasharray="1.6 1.8" d="M7.5 10l4.76 2.75v5.5l-4.76 2.75-4.76-2.75v-5.5z"/><path class="r" stroke-dasharray="1.6 1.8" d="M2.74 12.75l4.76 2.75 4.76-2.75M7.5 15.5v5.5"/><path d="M16.5 3l4.76 2.75v5.5l-4.76 2.75-4.76-2.75v-5.5z"/><path d="M11.74 5.75l4.76 2.75 4.76-2.75M16.5 8.5v5.5"/>"#,
    CatInspect => r#"<path d="M12 3l7.8 4.5v9L12 21l-7.8-4.5v-9zM4.2 7.5L12 12l7.8-4.5M12 12v9"/><path class="a" d="M15.1 10.2l-2.7 10.4M18 8.5l-2.8 10.5M19.8 11l-1.4 5.3"/>"#,
    CatDimension => r#"<rect x="3.5" y="13" width="17" height="7.5" rx="1"/><path class="r" d="M3.5 4.5v6.5M20.5 4.5v6.5"/><path class="a" d="M4.5 7.5h15M7 5.5l-2.5 2 2.5 2M17 5.5l2.5 2-2.5 2"/>"#,
    // The model mock's: a warning triangle, for what failed.
    Alert => r#"<path d="M12 3.5L21.5 20h-19z"/><path d="M12 10v4.5M12 17.2v.1"/>"#,
    // Not in the mocks: framing the camera on a failure, a viewfinder's
    // corners round a dot, and going back from it, an arrow back.
    Locate => r#"<path d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5"/><circle cx="12" cy="12" r="2.5"/>"#,
    Back => r#"<path d="M20 12H5M11 6l-6 6 6 6"/>"#,
    // The operation panel's head: Cancel and OK, at the mock's stroke of 2,
    // drawn 16 px. The check's arms both at 45°, on lines through pixel
    // centres at 16 and 32 px (a 16 px pixel is 1.5 units), as the cross's
    // are, so their edges are even all along.
    Cancel => r#"<path d="M6 6l12 12M18 6L6 18" stroke-width="2"/>"#,
    Confirm => r#"<path d="M5.25 12.75l4.5 4.5L19.5 7.5" stroke-width="2"/>"#,
    // The cross taking a pick out of the operation panel's field: heavy,
    // drawn small.
    Remove => r#"<path d="M6 6l12 12M18 6L6 18" stroke-width="2.8"/>"#,
    // What's picked, on its row in the operation panel: a sketch's
    // region, an axis.
    SeRegion => r#"<path class="fl" d="M4 6h10l6 6-6 6H4z"/><circle class="af" cx="4" cy="6" r="1.5"/><circle class="af" cx="20" cy="12" r="1.5"/><circle class="af" cx="4" cy="18" r="1.5"/>"#,
    SeAxis => r#"<path d="M4 20L20 4"/><circle class="af" cx="7.5" cy="16.5" r="1.7"/><circle class="af" cx="16.5" cy="7.5" r="1.7"/>"#,
    SePlane => r#"<path class="t" d="M3 17l4-10h14l-4 10z"/><path class="a" d="M12 15V7"/>"#,
    // The operation panel's choices, the model mock's `CHOICE_ICONS`:
    // a revolve's extents seen down its axis (the dot), the profile the
    // line out to the right; an extrude's, the profile as a slab and where
    // it goes; the booleans as circles, the body left and the tool right,
    // what's left filled, a tool taken away dashed.
    RvFull => r#"<path d="M12 12h8"/><circle class="a" cx="12" cy="12" r="8"/><circle cx="12" cy="12" r=".9"/>"#,
    RvOne => r#"<path d="M12 12h8"/><path class="a" d="M20 12A8 8 0 1 0 12 20"/><circle cx="12" cy="12" r=".9"/><circle class="af" cx="12" cy="20" r="1.6"/>"#,
    RvSym => r#"<path d="M12 12h8"/><path class="a" d="M12 4A8 8 0 0 1 12 20"/><circle cx="12" cy="12" r=".9"/><circle class="af" cx="12" cy="4" r="1.6"/><circle class="af" cx="12" cy="20" r="1.6"/>"#,
    RvTwo => r#"<path d="M12 12h8"/><path class="a" d="M8 5.07A8 8 0 0 1 17.14 18.13"/><circle cx="12" cy="12" r=".9"/><circle class="af" cx="8" cy="5.07" r="1.6"/><circle class="af" cx="17.14" cy="18.13" r="1.6"/>"#,
    ExOne => r#"<path class="fl" d="M3 19l4-3h14l-4 3z"/><path class="a" d="M12 16.5V5M9.5 7.5L12 5l2.5 2.5"/>"#,
    ExSym => r#"<path class="fl" d="M3 13.5l4-3h14l-4 3z"/><path class="a" d="M12 11V3.5M9.5 6L12 3.5 14.5 6M12 13.5V21M9.5 18.5L12 21l2.5-2.5"/>"#,
    ExTwo => r#"<path class="fl" d="M3 15l4-3h14l-4 3z"/><path class="a" d="M12 12.5V3M9.5 5.5L12 3l2.5 2.5M12 15v4.5M10 17.5l2 2 2-2"/>"#,
    ExThru => r#"<path class="r" d="M4 9h16v6H4z"/><path class="fl" d="M3 6l4-3h14l-4 3z"/><path class="a" d="M12 6v16M9.5 19.5L12 22l2.5-2.5"/>"#,
    BoNew => r#"<circle class="r" cx="7" cy="12" r="4.8"/><circle class="fl" cx="17" cy="12" r="4.8"/>"#,
    BoJoin => r#"<path class="fl" d="M12 6.8A6 6 0 1 0 12 17.2A6 6 0 1 0 12 6.8z"/>"#,
    BoCut => r#"<path class="fl" d="M12 6.8A6 6 0 1 0 12 17.2A6 6 0 0 1 12 6.8z"/><circle class="a" cx="15" cy="12" r="6" stroke-dasharray="2 1.6"/>"#,
    BoInt => r#"<circle class="r" cx="9" cy="12" r="6"/><circle class="r" cx="15" cy="12" r="6"/><path class="fl" d="M12 6.8A6 6 0 0 1 12 17.2A6 6 0 0 1 12 6.8z"/>"#,
    // The patterns' modes: the copies as squares, spacing measuring one
    // step and total the whole run (in the accent); round the axis (the
    // dot), a full turn, a step's arc, or the whole arc.
    LpSpacing => r#"<rect x="2.5" y="10" width="5" height="5" rx="1"/><rect x="9.5" y="10" width="5" height="5" rx="1"/><rect class="r" x="16.5" y="10" width="5" height="5" rx="1"/><path class="a" d="M5 6.5h7M5 5v3M12 5v3"/>"#,
    LpTotal => r#"<rect x="2.5" y="10" width="5" height="5" rx="1"/><rect x="9.5" y="10" width="5" height="5" rx="1"/><rect x="16.5" y="10" width="5" height="5" rx="1"/><path class="a" d="M5 6.5h14M5 5v3M19 5v3"/>"#,
    CpFull => r#"<circle cx="12" cy="12" r=".9"/><circle cx="12" cy="4.5" r="2"/><circle cx="19.5" cy="12" r="2"/><circle cx="12" cy="19.5" r="2"/><circle cx="4.5" cy="12" r="2"/><circle class="a" cx="12" cy="12" r="7.5" stroke-dasharray="1.6 1.8"/>"#,
    CpSpacing => r#"<circle cx="12" cy="12" r=".9"/><circle cx="19.5" cy="12" r="2"/><circle cx="12" cy="4.5" r="2"/><circle class="r" cx="4.5" cy="12" r="2"/><path class="a" d="M17.3 8.3A7.5 7.5 0 0 0 15.7 6.7"/><path class="a" d="M12 12L19.5 12M12 12L12 4.5" stroke-width="1"/>"#,
    CpTotal => r#"<circle cx="12" cy="12" r=".9"/><circle cx="19.5" cy="12" r="2"/><circle cx="12" cy="4.5" r="2"/><circle cx="4.5" cy="12" r="2"/><path class="a" d="M19.5 9A7.5 7.5 0 0 0 7.5 7.5"/>"#,
    // Not in the mocks: a scale's modes, drawn as the patterns' are: a
    // box grown into a dashed larger one every way, stretched along one
    // axis, and measured along an edge (the length in the accent).
    ScUniform => r#"<rect class="r" x="3" y="3" width="18" height="18" rx="1.5" stroke-dasharray="2 1.8"/><rect x="3" y="12" width="9" height="9" rx="1"/><path class="a" d="M10 14l6.5-6.5M12.5 7.5h4v4"/>"#,
    ScAxes => r#"<rect class="r" x="3" y="10" width="18" height="9" rx="1.5" stroke-dasharray="2 1.8"/><rect x="3" y="10" width="9" height="9" rx="1"/><path class="a" d="M14 5.5h6.5M18 3l2.5 2.5L18 8"/>"#,
    ScEdge => r#"<rect x="3" y="10" width="12" height="11" rx="1"/><path class="r" d="M15 10h6" stroke-dasharray="1.6 1.6"/><path class="a" d="M3 5.5h18M3 3.5v4M21 3.5v4"/>"#,
    // Not in the mock: a split's pieces, cut apart by a dashed line, the
    // ones kept drawn and filled, one left out dashed.
    SpBoth => r#"<rect class="fl" x="3" y="6" width="7.5" height="12" rx="1"/><rect class="fl" x="13.5" y="6" width="7.5" height="12" rx="1"/><path class="a" d="M12 3v18" stroke-dasharray="2 2"/>"#,
    SpFront => r#"<rect class="fl" x="3" y="6" width="7.5" height="12" rx="1"/><rect class="r" x="13.5" y="6" width="7.5" height="12" rx="1" stroke-dasharray="2 1.6"/><path class="a" d="M12 3v18" stroke-dasharray="2 2"/>"#,
    SpBack => r#"<rect class="r" x="3" y="6" width="7.5" height="12" rx="1" stroke-dasharray="2 1.6"/><rect class="fl" x="13.5" y="6" width="7.5" height="12" rx="1"/><path class="a" d="M12 3v18" stroke-dasharray="2 2"/>"#,
    // The options' icons, drawn in one colour: flip, keep the tools.
    TkFlip => r#"<path d="M4 8h15M15.5 4.5L19 8l-3.5 3.5M20 16H5M8.5 12.5L5 16l3.5 3.5"/>"#,
    TkKeep => r#"<circle cx="9" cy="12" r="6"/><circle cx="15" cy="12" r="6" stroke-dasharray="2 1.6"/>"#,
    TkCopy => r#"<rect x="3.5" y="3.5" width="11" height="11" rx="1.5"/><rect class="fl" x="9.5" y="9.5" width="11" height="11" rx="1.5"/>"#,
    TkJoin => r#"<rect x="3" y="7" width="8" height="10" rx="1.5"/><rect x="13" y="7" width="8" height="10" rx="1.5"/><path d="M8 12h8"/>"#,
    // Not in the mocks: the view options menu's shading, a ball lit four
    // ways. Smooth, shaded towards its lower right; flat, in facets;
    // metal, the horizon and a glint reflected in it; both.
    ShadeRegular => r#"<circle cx="12" cy="12" r="8"/><path d="M17.6 10.5a6 6 0 0 1-7.1 7.1"/>"#,
    ShadeFlat => r#"<path d="M12 4l5.66 2.34L20 12l-2.34 5.66L12 20l-5.66-2.34L4 12l2.34-5.66z"/><path d="M4 12h16M6.34 6.34L12 12l5.66-5.66M12 12v8"/>"#,
    ShadeMetal => r#"<circle cx="12" cy="12" r="8"/><path d="M4.3 13.5c4.5 2 10.9 2 15.4 0"/><path d="M9.5 5.8q.35 2.35 2.7 2.7-2.35.35-2.7 2.7-.35-2.35-2.7-2.7 2.35-.35 2.7-2.7z"/>"#,
    ShadeFlatMetal => r#"<path d="M12 4l5.66 2.34L20 12l-2.34 5.66L12 20l-5.66-2.34L4 12l2.34-5.66z"/><path d="M4 12l8 3 8-3"/><path d="M9.5 5.8q.35 2.35 2.7 2.7-2.35.35-2.7 2.7-.35-2.35-2.7-2.7 2.35-.35 2.7-2.7z"/>"#,
    // Its edges: a box with its edges, its patches' and its triangles'
    // finer.
    EdgesDefault => r#"<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/>"#,
    EdgesWireframe => r#"<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/><path d="M8 5.25v9M16 5.25v9M4 12l8 4.5 8-4.5" stroke-width="0.9"/>"#,
    EdgesTessellation => r#"<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/><path d="M4 16.5l8-4.5M20 16.5l-8-4.5M8 5.25l8 4.5" stroke-width="0.9"/>"#,
    // A menu item opening a submenu to its left.
    ChevLeft => r#"<path d="M14 7l-5 5 5 5"/>"#,
    // Nothing: room for an icon, beside items that have one.
    Blank => "",
}

impl Icon {
    /// What the icon stands for, as the icon mock files it, if it's a
    /// tool's or an object's; the rest are drawn in the text colour.
    pub fn category(self) -> Option<IconCategory> {
        Some(match self {
            Icon::Sketch
            | Icon::Line
            | Icon::Circle
            | Icon::Arc
            | Icon::Rectangle
            | Icon::Polygon
            | Icon::Spline
            | Icon::Point
            | Icon::Fillet
            | Icon::Chamfer
            | Icon::Convert
            | Icon::Handles
            | Icon::Comb
            | Icon::CatDraw
            | Icon::SeRegion => IconCategory::Sketch,
            Icon::Trim
            | Icon::Extend
            | Icon::Mirror
            | Icon::Offset
            | Icon::Combine
            | Icon::Scale
            | Icon::Split
            | Icon::BChamfer
            | Icon::ScUniform
            | Icon::ScAxes
            | Icon::ScEdge
            | Icon::SpBoth
            | Icon::SpFront
            | Icon::SpBack
            | Icon::CatModify
            | Icon::CatSketchModify => IconCategory::Modify,
            Icon::Constrain
            | Icon::Coincident
            | Icon::Horizontal
            | Icon::Vertical
            | Icon::Parallel
            | Icon::Perpendicular
            | Icon::Tangent
            | Icon::Smooth
            | Icon::Equal
            | Icon::Concentric
            | Icon::Midpoint
            | Icon::Symmetric
            | Icon::Fix
            | Icon::OffsetConstraint
            | Icon::CatConstrain => IconCategory::Constraint,
            Icon::Dimension | Icon::CatDimension => IconCategory::Dimension,
            Icon::Body
            | Icon::Extrude
            | Icon::Revolve
            | Icon::Move
            | Icon::BMirror
            | Icon::LPattern
            | Icon::CPattern
            | Icon::Align
            | Icon::CatCreate
            | Icon::CatTransform
            | Icon::RvFull
            | Icon::RvOne
            | Icon::RvSym
            | Icon::RvTwo
            | Icon::ExOne
            | Icon::ExSym
            | Icon::ExTwo
            | Icon::ExThru
            | Icon::BoNew
            | Icon::BoJoin
            | Icon::BoCut
            | Icon::BoInt
            | Icon::LpSpacing
            | Icon::LpTotal
            | Icon::CpFull
            | Icon::CpSpacing
            | Icon::CpTotal => IconCategory::Solid,
            Icon::Plane | Icon::SeAxis | Icon::SePlane => IconCategory::Construction,
            Icon::Measure | Icon::CatInspect => IconCategory::Inspect,
            Icon::Folder | Icon::Save | Icon::Export => IconCategory::File,
            _ => return None,
        })
    }

    /// The icon's colours: its category's, or the text colour throughout.
    pub fn tone(self, palette: &Palette) -> IconTone {
        match self.category() {
            Some(category) => palette.icons.tone(category),
            None => IconTone {
                line: palette.text,
                accent: palette.text,
                reference: palette.text,
            },
        }
    }

    /// The icon's colour, drawn in one: its geometry's.
    pub fn tint(self, palette: &Palette) -> Color {
        self.tone(palette).line
    }

    fn handle(self, frame: Frame, layer: Layer) -> svg::Handle {
        static HANDLES: LazyLock<Vec<svg::Handle>> = LazyLock::new(|| {
            let mut handles = Vec::new();
            for margin in [0.0, BUTTON_MARGIN] {
                for layer in Layer::ALL {
                    for icon in Icon::ALL {
                        handles.push(svg::Handle::from_memory(
                            Layers::of(icon.paths()).svg(layer, margin).into_bytes(),
                        ));
                    }
                }
            }
            handles
        });
        let frame = match frame {
            Frame::None => 0,
            Frame::Button => 1,
        };
        HANDLES[(frame * Layer::ALL.len() + layer as usize) * Icon::ALL.len() + self as usize]
            .clone()
    }

    /// Whether the icon draws anything in `layer`.
    fn draws(self, layer: Layer) -> bool {
        static DRAWS: LazyLock<Vec<[bool; Layer::ALL.len()]>> = LazyLock::new(|| {
            Icon::ALL
                .iter()
                .map(|icon| Layer::ALL.map(|layer| Layers::of(icon.paths()).draws(layer)))
                .collect()
        });
        DRAWS[self as usize][layer as usize]
    }
}

/// A part of an icon drawn in one colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    /// All of it, for an icon drawn in one colour.
    All,
    /// The tool's own geometry: unclassed elements.
    Line,
    /// Its accent marks: the mock's `a` (strokes) and `af` (dots, drawn
    /// hollow).
    Accent,
    /// The geometry it works from: the mock's `r`.
    Reference,
}

impl Layer {
    const ALL: [Layer; 4] = [Layer::All, Layer::Line, Layer::Accent, Layer::Reference];
}

/// An icon's SVG elements sorted into [`Layer`]s.
#[derive(Debug, Default, PartialEq)]
struct Layers {
    line: String,
    accent: String,
    reference: String,
    /// The accent dots, outlined.
    dots: String,
    /// The dots' insides, which hide what's drawn under them as the mock's
    /// fill in the panel colour does, so the dots stay hollow on any
    /// background.
    holes: String,
}

impl Layers {
    /// `paths` sorted by class. The elements must each be one empty tag
    /// (`<path .../>`), which the icon tests check.
    fn of(paths: &str) -> Layers {
        let mut layers = Layers::default();
        for element in paths.split_inclusive("/>") {
            let element = element.trim();
            assert!(
                element.starts_with('<') && element.ends_with("/>") && !element[1..].contains('<'),
                "not one empty tag: {element:?}"
            );
            let (classes, element) = match element.split_once(r#" class=""#) {
                Some((before, rest)) => {
                    let (classes, after) = rest.split_once('"').expect("a closed class");
                    (classes, format!("{before}{after}"))
                }
                None => ("", element.to_owned()),
            };
            let has = |class| classes.split_whitespace().any(|c| c == class);
            // A light fill, the mock's `fl`: the layer's colour, faint.
            let element = if has("fl") {
                format!(
                    r##"{} fill="#000" fill-opacity=".18"/>"##,
                    &element[..element.len() - 2]
                )
            } else {
                element
            };
            let open = &element[..element.len() - 2];
            if has("af") {
                layers.dots += &format!(r#"{open} stroke-width="1.5"/>"#);
                layers.holes += &format!(r##"{open} fill="#000" stroke="none"/>"##);
            } else if has("a") {
                layers.accent += &element;
            } else if has("r") {
                layers.reference += &element;
            } else {
                layers.line += &element;
            }
        }
        layers
    }

    /// Whether anything is drawn in `layer`.
    fn draws(&self, layer: Layer) -> bool {
        match layer {
            Layer::All => true,
            Layer::Line => !self.line.is_empty(),
            Layer::Accent => !self.accent.is_empty() || !self.dots.is_empty(),
            Layer::Reference => !self.reference.is_empty(),
        }
    }

    /// The SVG document for `layer`, with `margin` icon units of space
    /// around the icon.
    fn svg(&self, layer: Layer, margin: f32) -> String {
        let (masked, dots) = match layer {
            Layer::All => (
                format!("{}{}{}", self.reference, self.line, self.accent),
                self.dots.as_str(),
            ),
            Layer::Line => (self.line.clone(), ""),
            Layer::Accent => (self.accent.clone(), self.dots.as_str()),
            Layer::Reference => (self.reference.clone(), ""),
        };
        let body = if self.holes.is_empty() || masked.is_empty() {
            masked
        } else {
            format!(
                r##"<mask id="holes" maskUnits="userSpaceOnUse" x="-24" y="-24" width="72" height="72"><rect x="-24" y="-24" width="72" height="72" fill="#fff" stroke="none"/>{}</mask><g mask="url(#holes)">{masked}</g>"##,
                self.holes
            )
        };
        let side = 24.0 + 2.0 * margin;
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{} {} {side} {side}" fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">{body}{dots}</svg>"##,
            -margin, -margin,
        )
    }
}

/// Empty space drawn around an icon, so the [`Svg`] can fill a button and
/// its hover status matches the button's.
#[derive(Debug, Clone, Copy)]
enum Frame {
    None,
    /// [`INLINE`] in [`BUTTON_SIZE`].
    Button,
}

/// Side of an icon-only toolbar button.
pub const BUTTON_SIZE: f32 = 26.0;

/// Side of an icon beside text, and inside a [`BUTTON_SIZE`] button.
pub const INLINE: f32 = 16.0;

/// The margin in icon units (24 per icon).
const BUTTON_MARGIN: f32 = (BUTTON_SIZE - INLINE) / 2.0 * 24.0 / INLINE;

/// An icon `size` pixels square in its own colours: a tool's geometry,
/// accents and reference geometry each in its [`Icon::tone`].
pub fn icon<'a, Message: 'a>(icon: Icon, size: f32) -> Element<'a, Message> {
    if !icon.draws(Layer::Accent) && !icon.draws(Layer::Reference) {
        return tinted(icon, size, move |palette| icon.tint(palette)).into();
    }
    let layer = |layer: Layer, color: fn(IconTone) -> Color| {
        icon.draws(layer).then(|| {
            Svg::new(icon.handle(Frame::None, layer))
                .width(size)
                .height(size)
                .style(move |theme: &Theme, _| svg::Style {
                    color: Some(color(icon.tone(theme::palette(theme)))),
                })
        })
    };
    // The accents over the rest, as the mock draws them last.
    stack![
        layer(Layer::Reference, |tone| tone.reference),
        layer(Layer::Line, |tone| tone.line),
        layer(Layer::Accent, |tone| tone.accent),
    ]
    .into()
}

/// An icon `size` pixels square in one colour, which `color` picks from
/// the palette.
pub fn tinted(icon: Icon, size: f32, color: impl Fn(&Palette) -> Color + 'static) -> Svg<'static> {
    framed(icon, Frame::None, size, move |palette, _| color(palette))
}

/// The content of an icon-only button [`BUTTON_SIZE`] square, with no
/// padding, in its own colours (see [`icon`]) if `enabled` and it has a
/// [`Icon::category`], else in the faint colour if disabled or `color`'s.
pub fn button_content<'a, Message: 'a>(
    icon: Icon,
    enabled: bool,
    color: impl Fn(&Palette, bool) -> Color + 'static,
) -> Element<'a, Message> {
    if enabled && icon.category().is_some() {
        iced::widget::container(self::icon(icon, INLINE))
            .center(BUTTON_SIZE)
            .into()
    } else {
        button_icon(
            icon,
            move |p, hovered| if enabled { color(p, hovered) } else { p.faint },
        )
        .into()
    }
}

/// The content of an icon-only button [`BUTTON_SIZE`] square, with no
/// padding, in one colour. `color` gets whether the button is hovered.
pub fn button_icon(icon: Icon, color: impl Fn(&Palette, bool) -> Color + 'static) -> Svg<'static> {
    framed(icon, Frame::Button, BUTTON_SIZE, color)
}

fn framed(
    icon: Icon,
    frame: Frame,
    size: f32,
    color: impl Fn(&Palette, bool) -> Color + 'static,
) -> Svg<'static> {
    Svg::new(icon.handle(frame, Layer::All))
        .width(size)
        .height(size)
        .style(move |theme: &Theme, status| svg::Style {
            color: Some(color(theme::palette(theme), status == svg::Status::Hovered)),
        })
}

/// A mouse button, for [`mouse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    /// The wheel, or the middle button.
    Wheel,
}

impl MouseButton {
    /// The highlight drawn over the mouse outline for this button.
    fn handle(self) -> &'static svg::Handle {
        static LEFT: LazyLock<svg::Handle> =
            LazyLock::new(|| mouse_handle(r#"<path d="M5 .5A4.5 4.5 0 0 0 .5 5v1H5z"/>"#));
        static RIGHT: LazyLock<svg::Handle> =
            LazyLock::new(|| mouse_handle(r#"<path d="M5 .5A4.5 4.5 0 0 1 9.5 5v1H5z"/>"#));
        static WHEEL: LazyLock<svg::Handle> = LazyLock::new(|| {
            mouse_handle(r#"<rect x="4.1" y="2.2" width="1.8" height="3" rx=".9"/>"#)
        });
        match self {
            MouseButton::Left => &LEFT,
            MouseButton::Right => &RIGHT,
            MouseButton::Wheel => &WHEEL,
        }
    }
}

/// A small mouse with `button` highlighted, for input hints.
pub fn mouse<'a, Message: 'a>(button: MouseButton) -> Element<'a, Message> {
    static OUTLINE: LazyLock<svg::Handle> = LazyLock::new(|| {
        mouse_handle(
            r##"<rect x=".5" y=".5" width="9" height="13" rx="4.5" fill="none" stroke="#000"/>"##,
        )
    });

    let part = |handle: &svg::Handle, color: fn(&Palette) -> Color| {
        Svg::new(handle.clone())
            .width(11)
            .height(15)
            .style(move |theme: &Theme, _| svg::Style {
                color: Some(color(theme::palette(theme))),
            })
    };
    stack![
        part(&OUTLINE, |p| p.muted),
        part(button.handle(), |p| p.accent),
    ]
    .into()
}

fn mouse_handle(content: &str) -> svg::Handle {
    svg::Handle::from_memory(
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 14">{content}</svg>"#)
            .into_bytes(),
    )
}

#[cfg(test)]
mod tests;
