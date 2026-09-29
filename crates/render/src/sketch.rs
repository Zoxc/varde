//! The sketch being edited, as the renderer draws it over the scene: lines,
//! points and fills in layers the view builds.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::DVec2;
use tess2_rust::{ElementType, Tessellator, WindingRule};

use crate::GridPlane;
use crate::renderer::srgb_to_linear;

/// The sketch being edited, drawn over the scene without a depth test, so
/// the faded model never hides it: [`Self::base`] first, then
/// [`Self::live`], each its fills, then its lines, then its points.
#[derive(Debug, Clone, Copy)]
pub struct SketchScene<'a> {
    /// The sketch's plane: the sketch point (x, y) is at its origin plus x
    /// along its x axis and y along its y axis.
    pub plane: GridPlane,
    /// What changes with the sketch, the selection or the theme. Only
    /// re-uploaded when it's another `Arc` than the last one prepared, so
    /// moving the camera, which only changes uniforms, uploads nothing.
    pub base: &'a Arc<SketchLayer>,
    /// What changes as the cursor moves: hover, a tool's preview, a box
    /// being dragged. Rewritten every frame into buffers kept for it.
    pub live: &'a SketchLayer,
}

/// An sRGB-encoded colour with straight alpha, like CSS and UI colours.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Srgba(pub [f32; 4]);

impl Srgba {
    /// In linear colour, which the shader works in, alpha as it is.
    fn linear(self) -> [f32; 4] {
        let [r, g, b, a] = self.0;
        [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), a]
    }
}

/// Where a layer's coordinates are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    /// Sketch coordinates, on [`SketchScene::plane`], projected by the
    /// camera.
    Sketch,
    /// Logical pixels from the viewport's top left, y down.
    Screen,
}

impl Space {
    /// The flag saying so to the shader: [`SCREEN`], or none.
    fn flags(self) -> u32 {
        match self {
            Space::Sketch => 0,
            Space::Screen => SCREEN,
        }
    }
}

/// How a polyline is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineStyle {
    pub color: Srgba,
    /// In logical pixels, however far the camera is. A line whose width
    /// isn't finite and above zero isn't drawn.
    pub width: f32,
    /// The lengths of a dash and of the gap after it, in logical pixels,
    /// running on along the whole polyline; `None`, or either not above
    /// zero, for a solid line.
    pub dash: Option<[f32; 2]>,
}

/// How a point is drawn: a disc with a rim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointStyle {
    /// To the outside of the rim, in logical pixels. A point whose radius
    /// isn't finite and above zero isn't drawn.
    pub radius: f32,
    /// How wide the rim is, in logical pixels.
    pub rim_width: f32,
    pub rim: Srgba,
    /// Inside the rim.
    pub fill: Srgba,
    /// A fixed point is filled with the rim's colour instead.
    pub fixed: bool,
}

/// Lines, points and fills to draw, in the order they're added within
/// each kind. Lines and points keep their width and size on screen at any
/// zoom and display scale, and are anti-aliased; fills are drawn under
/// both, their edges left to the lines drawn along them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SketchLayer {
    pub(crate) lines: Vec<LineInstance>,
    pub(crate) points: Vec<PointInstance>,
    pub(crate) fills: Vec<FillVertex>,
}

/// A segment of a polyline as the GPU takes it: `vs_sketch_line` in the
/// scene shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct LineInstance {
    /// The segment's start and end.
    pub(crate) ends: [f32; 4],
    /// The point before the start and the one after the end, where
    /// [`Self::flags`] say there are.
    pub(crate) neighbours: [f32; 4],
    /// Linear, straight alpha.
    pub(crate) color: [f32; 4],
    /// The width, then a dash's and a gap's lengths (0 for a solid line),
    /// in logical pixels, then nothing.
    pub(crate) style: [f32; 4],
    /// How far along the polyline its start and end are, in the units of
    /// its [`Space`].
    pub(crate) along: [f32; 2],
    /// [`HAS_PREV`], [`HAS_NEXT`], [`SCREEN`].
    pub(crate) flags: u32,
    _pad: u32,
}

/// A point as the GPU takes it: `vs_point` in the scene shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct PointInstance {
    pub(crate) at: [f32; 2],
    /// The radius and the rim's width, in logical pixels.
    pub(crate) size: [f32; 2],
    pub(crate) rim: [f32; 4],
    pub(crate) fill: [f32; 4],
}

/// A corner of a fill's triangle as the GPU takes it: `vs_fill` in the
/// scene shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct FillVertex {
    pub(crate) at: [f32; 2],
    /// [`SCREEN`], or nothing.
    pub(crate) flags: u32,
    pub(crate) color: [f32; 4],
    _pad: u32,
}

impl FillVertex {
    /// A corner at `x`, `y` in the space `flags` says, in the linear
    /// `color`.
    fn new(x: f64, y: f64, flags: u32, color: [f32; 4]) -> FillVertex {
        FillVertex {
            at: [x as f32, y as f32],
            flags,
            color,
            _pad: 0,
        }
    }
}

/// Flags of a [`LineInstance`] and a [`FillVertex`], as in the scene
/// shader: whether the segment has a neighbour before and after it, and
/// whether its coordinates are [`Space::Screen`]'s.
pub(crate) const HAS_PREV: u32 = 1;
pub(crate) const HAS_NEXT: u32 = 2;
pub(crate) const SCREEN: u32 = 4;

impl SketchLayer {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.points.is_empty() && self.fills.is_empty()
    }

    /// Adds the polyline through `points` in `space`. A point repeating
    /// the one before it is dropped, and a polyline through three points or
    /// more ending where it starts is closed: its first and last segments
    /// join like the rest. Nothing
    /// is added for a polyline with a point that isn't finite, or of fewer
    /// than two distinct points.
    pub fn polyline(&mut self, space: Space, points: &[DVec2], style: LineStyle) {
        if !(visible(style.width) && points.iter().all(|p| p.is_finite())) {
            return;
        }
        let mut points = points.to_vec();
        points.dedup();
        if points.len() < 2 {
            return;
        }
        let closed = points.len() > 3 && points.first() == points.last();
        let last = points.len() - 2;
        let color = style.color.linear();
        let [on, off] = style.dash.unwrap_or([0.0; 2]);
        let dash = if on > 0.0 && off > 0.0 {
            [on, off]
        } else {
            [0.0; 2]
        };
        let space = space.flags();
        let mut along = 0.0f64;
        for (i, pair) in points.windows(2).enumerate() {
            let (start, end) = (pair[0], pair[1]);
            let prev = match i {
                0 if closed => Some(points[last]),
                0 => None,
                i => Some(points[i - 1]),
            };
            let next = match points.get(i + 2) {
                Some(&next) => Some(next),
                None if closed => Some(points[1]),
                None => None,
            };
            let length = start.distance(end);
            let flag = |neighbour: Option<DVec2>, flag| neighbour.map_or(0, |_| flag);
            let flags = space | flag(prev, HAS_PREV) | flag(next, HAS_NEXT);
            let pair = |a: DVec2, b: DVec2| [a.x as f32, a.y as f32, b.x as f32, b.y as f32];
            self.lines.push(LineInstance {
                ends: pair(start, end),
                neighbours: pair(prev.unwrap_or(start), next.unwrap_or(end)),
                color,
                style: [style.width, dash[0], dash[1], 0.0],
                along: [along as f32, (along + length) as f32],
                flags,
                _pad: 0,
            });
            along += length;
        }
    }

    /// Adds a point at the sketch point `at`, unless `at` isn't finite.
    /// A rim wider than the radius fills it.
    pub fn point(&mut self, at: DVec2, style: PointStyle) {
        if !(visible(style.radius) && at.is_finite()) {
            return;
        }
        let fill = if style.fixed { style.rim } else { style.fill };
        self.points.push(PointInstance {
            at: at.as_vec2().to_array(),
            size: [style.radius, style.rim_width],
            rim: style.rim.linear(),
            fill: fill.linear(),
        });
    }

    /// Fills what `contours` enclose in `space` in `color`, by the
    /// even-odd rule, so a contour inside another is a hole. Contours of
    /// fewer than three points are left out; nothing is added if any point
    /// isn't finite, or the contours can't be triangulated.
    pub fn fill<'p>(
        &mut self,
        space: Space,
        contours: impl IntoIterator<Item = &'p [DVec2]>,
        color: Srgba,
    ) {
        let mut tessellator = Tessellator::new();
        for contour in contours {
            if !contour.iter().all(|p| p.is_finite()) {
                return;
            }
            if contour.len() >= 3 {
                let flat: Vec<f64> = contour.iter().flat_map(|p| p.to_array()).collect();
                tessellator.add_contour(2, &flat);
            }
        }
        if !tessellator.tessellate(WindingRule::Odd, ElementType::Polygons, 3, 2, None) {
            return;
        }
        let vertices = tessellator.vertices();
        let flags = space.flags();
        let color = color.linear();
        let corner = |index: u32| {
            let i = usize::try_from(index).ok()?.checked_mul(2)?;
            let at = vertices.get(i..i.checked_add(2)?)?;
            Some(FillVertex::new(at[0], at[1], flags, color))
        };
        for triangle in tessellator.elements().as_chunks::<3>().0 {
            // Polygons of fewer corners pad with an index past the end.
            if let [Some(a), Some(b), Some(c)] = triangle.map(corner) {
                self.fills.extend([a, b, c]);
            }
        }
    }

    /// Fills the triangle `corners` in `space` in `color`, as a fill of
    /// its outline would, without triangulating it: for many small shapes
    /// rebuilt often, such as dimensions' arrowheads. Nothing is added if
    /// a corner isn't finite.
    pub fn triangle(&mut self, space: Space, corners: [DVec2; 3], color: Srgba) {
        if !corners.iter().all(|corner| corner.is_finite()) {
            return;
        }
        let flags = space.flags();
        let color = color.linear();
        self.fills
            .extend(corners.map(|at| FillVertex::new(at.x, at.y, flags, color)));
    }
}

/// Whether a line of width `size`, or a point of radius `size`, shows.
fn visible(size: f32) -> bool {
    size > 0.0 && size.is_finite()
}

#[cfg(test)]
mod tests;
