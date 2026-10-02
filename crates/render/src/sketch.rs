//! The sketch being edited, as the renderer draws it over the scene: lines,
//! points and fills in layers the view builds.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{DVec2, Vec3};
use tess2_rust::{ElementType, Tessellator, WindingRule};
use varde_kernel::{Aabb, RenderLines};

use crate::GridPlane;
use crate::renderer::srgb_to_linear;

/// The sketch being edited, or the extrude being set up, drawn over the
/// scene: [`Self::base`] first, then [`Self::live`], each its fills, then
/// its lines, then its points.
#[derive(Debug, Clone, Copy)]
pub struct SketchScene<'a> {
    /// The sketch's plane: the sketch point (x, y) is at its origin plus x
    /// along its x axis and y along its y axis.
    pub plane: GridPlane,
    /// Whether the model hides what's behind it, as it does an extrude's
    /// regions and handle: depth tested, pulled towards the camera like
    /// the edges so a face it lies on doesn't hide it. Otherwise drawn
    /// over everything, so the faded model behind a sketch being edited
    /// never hides it. What's in [`Space::Screen`] is over everything
    /// either way.
    pub depth_tested: bool,
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
    pub(crate) fn linear(self) -> [f32; 4] {
        let [r, g, b, a] = self.0;
        [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), a]
    }
}

/// Where a layer's coordinates are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Space {
    /// Sketch coordinates, on [`SketchScene::plane`], projected by the
    /// camera.
    Sketch,
    /// Logical pixels from the viewport's top left, y down.
    Screen,
    /// Coordinates on another plane than the scene's, as [`Space::Sketch`]
    /// is on its plane: placed in the world as they're added, so a layer
    /// can draw on several planes. Not for points: a point in the world
    /// is added by [`SketchLayer::world_point`].
    On(GridPlane),
}

impl Space {
    /// The flag saying so to the shader: [`SCREEN`], [`WORLD`], or none.
    fn flags(self) -> u32 {
        match self {
            Space::Sketch => 0,
            Space::Screen => SCREEN,
            Space::On(_) => WORLD,
        }
    }

    /// Where `at` goes in the GPU's coordinates: as it is, but for
    /// [`Space::On`] the world point, its z apart. `None` if that isn't
    /// finite in `f32`.
    fn place(self, at: DVec2) -> Option<(DVec2, f32)> {
        let (xy, z) = match self {
            Space::Sketch | Space::Screen => (at, 0.0),
            Space::On(plane) => {
                let world = plane.origin().as_dvec3()
                    + plane.x().as_dvec3() * at.x
                    + plane.y().as_dvec3() * at.y;
                (world.truncate(), world.z)
            }
        };
        let gpu = xy.as_vec2().extend(z as f32);
        gpu.is_finite().then_some((xy, z as f32))
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
    /// With [`WORLD`], the world z of the start, the end and the points
    /// before and after them, whose x and y are in [`Self::ends`] and
    /// [`Self::neighbours`]; otherwise nothing.
    pub(crate) z: [f32; 4],
    /// Linear, straight alpha.
    pub(crate) color: [f32; 4],
    /// The width, then a dash's and a gap's lengths (0 for a solid line),
    /// in logical pixels, then nothing.
    pub(crate) style: [f32; 4],
    /// How far along the polyline its start and end are, in the units of
    /// its [`Space`].
    pub(crate) along: [f32; 2],
    /// [`HAS_PREV`], [`HAS_NEXT`], [`SCREEN`], [`WORLD`].
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
    /// With [`WORLD`], the world z, whose x and y are [`Self::at`].
    pub(crate) z: f32,
    /// [`WORLD`], or nothing for a sketch point.
    pub(crate) flags: u32,
}

/// A corner of a fill's triangle as the GPU takes it: `vs_fill` in the
/// scene shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct FillVertex {
    pub(crate) at: [f32; 2],
    /// [`SCREEN`], [`WORLD`], or nothing.
    pub(crate) flags: u32,
    pub(crate) color: [f32; 4],
    /// With [`WORLD`], the world z, whose x and y are [`Self::at`].
    pub(crate) z: f32,
}

impl FillVertex {
    /// A corner at `at` in `space`, in the linear `color`, unless it isn't
    /// finite there.
    fn new(space: Space, at: DVec2, color: [f32; 4]) -> Option<FillVertex> {
        let (xy, z) = space.place(at)?;
        Some(FillVertex {
            at: xy.as_vec2().to_array(),
            flags: space.flags(),
            color,
            z,
        })
    }
}

/// Flags of a [`LineInstance`] and a [`FillVertex`], as in the scene
/// shader: whether the segment has a neighbour before and after it,
/// whether its coordinates are [`Space::Screen`]'s or the world's
/// ([`Space::On`]), and whether it fades out pointing at the camera
/// ([`SketchLayer::axis_polyline`]).
pub(crate) const HAS_PREV: u32 = 1;
pub(crate) const HAS_NEXT: u32 = 2;
pub(crate) const SCREEN: u32 = 4;
pub(crate) const WORLD: u32 = 8;
pub(crate) const FADES: u32 = 16;

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
        self.polyline_flagged(space, 0, points, style);
    }

    /// Adds the polyline through `points` in `space`, as
    /// [`Self::polyline`] does, fading out each segment as it turns to
    /// point at the camera, as the grid's axis lines do: for a sketch's
    /// axes, which lie on them. Not for [`Space::Screen`], where it's
    /// added as [`Self::polyline`] adds it.
    pub fn axis_polyline(&mut self, space: Space, points: &[DVec2], style: LineStyle) {
        self.polyline_flagged(space, FADES, points, style);
    }

    /// [`Self::polyline`] with `flags` too.
    fn polyline_flagged(&mut self, space: Space, flags: u32, points: &[DVec2], style: LineStyle) {
        if !(visible(style.width) && points.iter().all(|p| p.is_finite())) {
            return;
        }
        let mut points = points.to_vec();
        points.dedup();
        if points.len() < 2 {
            return;
        }
        let Some(placed) = points
            .iter()
            .map(|&p| space.place(p))
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let closed = points.len() > 3 && points.first() == points.last();
        let lengths = points.windows(2).map(|pair| pair[0].distance(pair[1]));
        self.push_lines(space.flags() | flags, &placed, lengths, closed, style);
    }

    /// Adds the polyline through the world `points`, as [`Self::polyline`]
    /// adds one in [`Space::On`]: for what's drawn on the model in
    /// the world, such as a measured distance.
    pub fn world_polyline(&mut self, points: &[Vec3], style: LineStyle) {
        if !(visible(style.width) && points.iter().all(|p| p.is_finite())) {
            return;
        }
        let mut points = points.to_vec();
        points.dedup();
        if points.len() < 2 {
            return;
        }
        let placed: Vec<_> = (points.iter())
            .map(|p| (p.truncate().as_dvec2(), p.z))
            .collect();
        let closed = points.len() > 3 && points.first() == points.last();
        let lengths = (points.windows(2)).map(|pair| f64::from(pair[0].distance(pair[1])));
        self.push_lines(WORLD, &placed, lengths, closed, style);
    }

    /// Adds the segments between the `placed` points, as the GPU takes
    /// them, flagged `space`, each as long as `lengths` says in turn,
    /// joined at the ends if `closed`: two or more points, the last the
    /// first again if `closed`.
    fn push_lines(
        &mut self,
        space: u32,
        placed: &[(DVec2, f32)],
        lengths: impl Iterator<Item = f64>,
        closed: bool,
        style: LineStyle,
    ) {
        let last = placed.len() - 2;
        let color = style.color.linear();
        let [on, off] = style.dash.unwrap_or([0.0; 2]);
        let dash = if on > 0.0 && off > 0.0 {
            [on, off]
        } else {
            [0.0; 2]
        };
        let mut along = 0.0f64;
        for (i, length) in lengths.enumerate().take(placed.len() - 1) {
            // By index into `placed`.
            let prev = match i {
                0 if closed => Some(last),
                0 => None,
                i => Some(i - 1),
            };
            let next = match i + 2 {
                next if next < placed.len() => Some(next),
                _ if closed => Some(1),
                _ => None,
            };
            let flag = |neighbour: Option<usize>, flag| neighbour.map_or(0, |_| flag);
            let flags = space | flag(prev, HAS_PREV) | flag(next, HAS_NEXT);
            let [a, b] = [i, i + 1].map(|i| placed[i]);
            let [p, n] = [prev.map_or(a, |i| placed[i]), next.map_or(b, |i| placed[i])];
            let pair = |a: DVec2, b: DVec2| [a.x as f32, a.y as f32, b.x as f32, b.y as f32];
            self.lines.push(LineInstance {
                ends: pair(a.0, b.0),
                neighbours: pair(p.0, n.0),
                z: [a.1, b.1, p.1, n.1],
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
            z: 0.0,
            flags: 0,
        });
    }

    /// Adds a point at the world point `at`, as [`Self::point`] adds one
    /// at a sketch point: for points on the model, such as the measure
    /// tool's. Nothing is added where `at` isn't finite in `f32`.
    pub fn world_point(&mut self, at: Vec3, style: PointStyle) {
        if !(visible(style.radius) && at.is_finite()) {
            return;
        }
        let fill = if style.fixed { style.rim } else { style.fill };
        self.points.push(PointInstance {
            at: at.truncate().to_array(),
            size: [style.radius, style.rim_width],
            rim: style.rim.linear(),
            fill: fill.linear(),
            z: at.z,
            flags: WORLD,
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
        let color = color.linear();
        let corner = |index: u32| {
            let i = usize::try_from(index).ok()?.checked_mul(2)?;
            let at = vertices.get(i..i.checked_add(2)?)?;
            FillVertex::new(space, DVec2::new(at[0], at[1]), color)
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
        let color = color.linear();
        if let [Some(a), Some(b), Some(c)] = corners.map(|at| FillVertex::new(space, at, color)) {
            self.fills.extend([a, b, c]);
        }
    }

    /// The box around what it draws in the world with its sketch on
    /// `plane`, leaving out what's on the screen and what's past
    /// [`RenderLines::MAX_POSITION`]: for the depth range to cover it
    /// when it's depth tested.
    pub(crate) fn bounds(&self, plane: &GridPlane) -> Option<Aabb> {
        let on_plane = |x: f32, y: f32| {
            let at = plane.origin().as_dvec3()
                + plane.x().as_dvec3() * f64::from(x)
                + plane.y().as_dvec3() * f64::from(y);
            at.as_vec3()
        };
        let world = |x: f32, y: f32, z: f32, flags: u32| {
            if flags & SCREEN != 0 {
                None
            } else if flags & WORLD != 0 {
                Some(Vec3::new(x, y, z))
            } else {
                Some(on_plane(x, y))
            }
        };
        let lines = self.lines.iter().flat_map(|line| {
            let [ax, ay, bx, by] = line.ends;
            [
                world(ax, ay, line.z[0], line.flags),
                world(bx, by, line.z[1], line.flags),
            ]
        });
        let points = (self.points.iter()).map(|p| world(p.at[0], p.at[1], p.z, p.flags));
        let fills = self
            .fills
            .iter()
            .map(|f| world(f.at[0], f.at[1], f.z, f.flags));
        let limit = Vec3::splat(RenderLines::MAX_POSITION);
        lines
            .chain(points)
            .chain(fills)
            .flatten()
            .filter(|p| p.abs().cmple(limit).all())
            .fold(None, |bounds: Option<Aabb>, p| {
                Some(bounds.map_or(Aabb { min: p, max: p }, |b| Aabb {
                    min: b.min.min(p),
                    max: b.max.max(p),
                }))
            })
    }
}

/// Whether a line of width `size`, or a point of radius `size`, shows.
fn visible(size: f32) -> bool {
    size > 0.0 && size.is_finite()
}

#[cfg(test)]
mod tests;
