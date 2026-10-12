//! The styles the viewport's layers draw in, shared by the sketch being
//! edited, the regions, the handles and the tools: lines, dots, region
//! fills and colours for the renderer.

use iced::Color;
use varde_render::{LineStyle, PointStyle, SketchLayer, Space, Srgba};
use varde_sketch::Region;

use crate::theme::SketchColors;

/// The dashes of construction geometry, and of a box touching what it
/// selects: on and off, in pixels.
const DASH: [f32; 2] = [6.0, 4.0];

/// A line `width` pixels wide in `color`, dashed if `dashed`.
pub(super) fn line(color: Color, width: f32, dashed: bool) -> LineStyle {
    LineStyle {
        color: srgba(color),
        width,
        dash: dashed.then_some(DASH),
    }
}

/// Fills `region` on `layer` in `color`, by its outline, holes left out.
pub(super) fn fill_region(layer: &mut SketchLayer, region: &Region, color: Color) {
    fill_region_in(layer, Space::Sketch, region, color);
}

/// Like [`fill_region`], with the region's coordinates in `space`.
pub(super) fn fill_region_in(layer: &mut SketchLayer, space: Space, region: &Region, color: Color) {
    let outline = region.outline.iter().map(Vec::as_slice);
    layer.fill(space, outline, srgba(color));
}

/// `color` for the renderer.
pub(super) fn srgba(color: Color) -> Srgba {
    Srgba([color.r, color.g, color.b, color.a])
}

/// A dot the tools mark a point with: `radius` pixels with a rim of
/// `rim` round the sketch's point fill, or filled with the rim if
/// `fixed`.
pub(super) fn mark_dot(radius: f32, rim: Color, colors: SketchColors, fixed: bool) -> PointStyle {
    PointStyle {
        radius,
        rim_width: 1.5,
        rim: srgba(rim),
        fill: srgba(colors.point_fill),
        fixed,
    }
}
