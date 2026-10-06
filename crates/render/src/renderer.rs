use std::any::Any;
use std::ops::Range;
use std::sync::{Arc, Weak};

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use varde_kernel::{Aabb, RenderLines, RenderMesh};
use wgpu::util::DeviceExt;

use crate::Camera;
use crate::highlight::{Highlights, VertexInstance};
use crate::scene::{self, GRID_FADE_HEIGHTS, GridPlane};
use crate::sketch::{FillVertex, LineInstance, PointInstance, SketchLayer, SketchScene, Srgba};

/// The depth buffer's format on `device`, with a stencil: 32 bit float
/// depth if it has that, else 24 bits. The stencil marks which part less
/// than opaque is nearest at a pixel, and where a hidden error patch is
/// drawn already.
fn depth_format(device: &wgpu::Device) -> wgpu::TextureFormat {
    if device
        .features()
        .contains(wgpu::Features::DEPTH32FLOAT_STENCIL8)
    {
        wgpu::TextureFormat::Depth32FloatStencil8
    } else {
        wgpu::TextureFormat::Depth24PlusStencil8
    }
}

/// Which of the world's origin objects a [`Frame`] draws: the origin
/// marker, the X, Y and Z axis lines, and the XY, XZ and YZ planes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OriginShown {
    pub marker: bool,
    /// By axis: X, Y, Z.
    pub axes: [bool; 3],
    /// In the order of `PLANES` in the shader: XY, XZ, YZ.
    pub planes: [bool; 3],
    /// The one hovered, if one is: drawn whether it's shown or not, and
    /// emphasised, a plane more opaque, an axis's line wider, the marker
    /// as it is.
    pub hovered: Option<OriginPart>,
    /// Those selected, by [`OriginPart::index`]: drawn as the one hovered
    /// is.
    pub selected: [bool; OriginPart::COUNT],
}

/// One of the world's origin objects, as [`OriginShown`] has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginPart {
    Marker,
    /// X, Y or Z: 0, 1 or 2.
    Axis(usize),
    /// XY, XZ or YZ: 0, 1 or 2.
    Plane(usize),
}

impl OriginPart {
    /// How many there are.
    pub const COUNT: usize = 7;

    /// Where it is among them: the marker, the axes, then the planes.
    /// [`Self::COUNT`] or past for an axis or plane past 2.
    pub fn index(self) -> usize {
        match self {
            OriginPart::Marker => 0,
            OriginPart::Axis(axis) if axis < 3 => 1 + axis,
            OriginPart::Plane(plane) if plane < 3 => 4 + plane,
            _ => Self::COUNT,
        }
    }

    /// Each, by [`Self::index`].
    pub const ALL: [OriginPart; Self::COUNT] = [
        OriginPart::Marker,
        OriginPart::Axis(0),
        OriginPart::Axis(1),
        OriginPart::Axis(2),
        OriginPart::Plane(0),
        OriginPart::Plane(1),
        OriginPart::Plane(2),
    ];
}

impl OriginShown {
    /// The marker and the X and Y axes, not the Z axis nor the planes.
    pub const DEFAULT: OriginShown = OriginShown {
        marker: true,
        axes: [true, true, false],
        planes: [false; 3],
        hovered: None,
        selected: [false; OriginPart::COUNT],
    };
    /// None of them.
    pub const NONE: OriginShown = OriginShown {
        marker: false,
        axes: [false; 3],
        planes: [false; 3],
        hovered: None,
        selected: [false; OriginPart::COUNT],
    };

    /// Whether `part` is drawn emphasised: hovered or selected.
    fn emphasised(&self, part: OriginPart) -> bool {
        self.hovered == Some(part) || self.selected.get(part.index()).copied().unwrap_or(false)
    }

    /// These with the ones hovered and selected shown too.
    fn with_hovered(mut self) -> Self {
        let shown = self;
        for part in OriginPart::ALL
            .into_iter()
            .filter(|&part| shown.emphasised(part))
        {
            match part {
                OriginPart::Marker => self.marker = true,
                OriginPart::Axis(axis) => self.axes[axis] = true,
                OriginPart::Plane(plane) => self.planes[plane] = true,
            }
        }
        self
    }

    /// The mask the shader takes in `grid_origin.w`, drawing on `grid`:
    /// [`GRID_X`] and [`GRID_Y`] for the grid's own axis lines, which on
    /// the world's XY plane are the X and Y axes, else are always drawn;
    /// [`WORLD_Z`] for the Z axis's line, and [`ORIGIN_MARKER`]; and the
    /// hovered axis's bit shifted by [`HOVERED_SHIFT`], on the world's XY
    /// plane, where the grid's lines are the axes.
    fn mask(self, grid: &GridPlane) -> u32 {
        let world = *grid == GridPlane::XY;
        let bit = |on: bool, bit: u32| if on { bit } else { 0 };
        let hovered = (0..3)
            .filter(|&axis| (world || axis == 2) && self.emphasised(OriginPart::Axis(axis)))
            .fold(0, |mask, axis| mask | 1 << axis << HOVERED_SHIFT);
        bit(self.axes[0] || !world, GRID_X)
            | bit(self.axes[1] || !world, GRID_Y)
            | bit(self.axes[2], WORLD_Z)
            | bit(self.marker, ORIGIN_MARKER)
            | hovered
    }
}

impl Default for OriginShown {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Bits of [`OriginShown::mask`], as in the shader.
const GRID_X: u32 = 1;
const GRID_Y: u32 = 1 << 1;
const WORLD_Z: u32 = 1 << 2;
const ORIGIN_MARKER: u32 = 1 << 3;
/// How far a line's bit is shifted to say it's hovered, as in the shader.
const HOVERED_SHIFT: u32 = 4;

/// A quad per origin plane, an instance each: see `vs_origin_plane`.
const PLANE_VERTICES: u32 = 6;

/// How far an origin plane reaches from the origin along its positive
/// axes, in view heights, as `PLANE_REACH` in the shader: drawn the same size on
/// screen at any zoom.
pub const PLANE_REACH: f32 = 0.2;

/// Where an origin plane starts from the origin along its axes, of
/// [`PLANE_REACH`]: they're squares on one side of their axes
/// ([`PLANE_SIDES`]),
/// three faces of a cube cornered at the origin, kept apart by the gap.
pub const PLANE_GAP: f32 = 0.08;

/// Which way the origin planes run along each world axis, as
/// `PLANE_SIDES` in the shader: into the octant the default camera looks
/// from (front right, above), so it sees the cube's inside.
pub const PLANE_SIDES: Vec3 = Vec3::new(1.0, -1.0, 1.0);

/// A quad per marker, an instance each: the origin's and the pivot's. See
/// `vs_origin`.
const ORIGIN_VERTICES: u32 = 6;
const MARKERS: u32 = 2;

/// A quad per segment of a line, two triangles. See `line_vertex`.
const LINE_VERTICES: u32 = 6;

/// A quad per point of the sketch being edited. See `vs_point`.
const POINT_VERTICES: u32 = 6;

/// How wide [`Frame::sketches`] are drawn, in logical pixels.
pub const LINE_WIDTH: f32 = 1.5;

/// How wide the model's feature edges are drawn, in logical pixels.
pub const EDGE_WIDTH: f32 = 1.5;

/// How wide creases are drawn, in logical pixels: feature edges inside one
/// face (see [`RenderMesh::edge_faces`]), where its patches' normals part.
pub const CREASE_WIDTH: f32 = 1.0;

/// How opaque creases are drawn, from 0 to 1, of what the feature edges
/// are, seen and hidden alike: they're how a face was cut into patches,
/// not where it ends.
pub const CREASE_ALPHA: f32 = 0.35;

/// How wide the edges the model hides are drawn, in logical pixels: see
/// [`Frame::hidden_edges`].
pub const HIDDEN_EDGE_WIDTH: f32 = 1.0;

/// The dashes of the edges the model hides: how long a dash and a gap are
/// along the edge, in logical pixels at the target.
pub const HIDDEN_DASH: [f32; 2] = [4.0, 3.0];

/// How wide the hovered edges are drawn, in logical pixels, within their
/// rim: see [`Highlights::outlined`].
pub const HOVERED_EDGE_WIDTH: f32 = 2.5;

/// How wide the rim of [`Colors::hover_outline`] is around the hovered
/// edges and a hovered vertex, in logical pixels: see
/// [`Highlights::outlined`].
pub const HOVER_RIM: f32 = 1.5;

/// How opaque a hovered face drawn over what hides it is, of its hover:
/// see [`Frame::hover_through`].
pub const HOVER_THROUGH_ALPHA: f32 = 0.6;

/// How wide the faint rim around the selected edges and vertices is, in
/// logical pixels, for contrast with what's behind them: in
/// [`Colors::hover_outline`], translucent.
pub const SELECTED_RIM: f32 = 1.0;

/// How wide the selected edges are drawn, in logical pixels, over their
/// [`EDGE_WIDTH`]: see [`Highlights::selected_edges`].
pub const SELECTED_EDGE_WIDTH: f32 = 2.5;

/// The radius of a hovered or selected vertex's disc, in logical pixels,
/// within its rim: see [`Highlights::vertices`].
pub const VERTEX_RADIUS: f32 = 3.5;

/// How wide error geometry's curves are drawn, in logical pixels: see
/// [`Frame::errors`].
pub const ERROR_EDGE_WIDTH: f32 = 3.0;

/// How far the halo around error geometry reaches beyond it on every side,
/// in logical pixels: see [`Frame::errors`].
pub const ERROR_HALO: f32 = 12.0;

/// The radius of an error point's disc, in logical pixels: see
/// [`Frame::errors`].
pub const ERROR_POINT_RADIUS: f32 = 4.0;

/// How strongly error geometry is drawn where the model hides it, of its
/// full strength, core and halo alike.
const ERROR_HIDDEN: f32 = 0.4;

/// The format of the target the errors' halo is drawn into as coverage.
const HALO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// A rectangle on the render target, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Viewport {
    /// Width over height, at least a pixel each way, so it's positive and
    /// finite for the camera's projection however degenerate the viewport.
    pub fn aspect(&self) -> f32 {
        self.width.max(1.0) / self.height.max(1.0)
    }
}

/// A scissor rectangle on the render target, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClipRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// How the model's faces are lit: see `shaded` in the shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shading {
    /// Bright, low contrast shading, smooth across each face.
    #[default]
    Regular,
    /// Each triangle lit by its own plane's normal, so the tessellation
    /// shows.
    Flat,
    /// Polished metal, reflecting a studio fixed to the view.
    Metal,
    /// Metal, each triangle lit by its own plane's normal, as [`Self::Flat`].
    FlatMetal,
}

impl Shading {
    /// Its number in the shader: `SHADING_FLAT`, `SHADING_METAL` and
    /// `SHADING_FLAT_METAL`.
    fn code(self) -> f32 {
        match self {
            Shading::Regular => 0.0,
            Shading::Flat => 1.0,
            Shading::Metal => 2.0,
            Shading::FlatMetal => 3.0,
        }
    }
}

/// Everything needed to draw one frame.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    pub camera: &'a Camera,
    /// Only re-uploaded when it's another `Arc` than the last one prepared.
    pub mesh: &'a Arc<RenderMesh>,
    /// How opaque each of the mesh's parts is drawn, from 0 to 1, in the
    /// order of [`RenderMesh::parts`]: faces and edges, hidden ones too. A
    /// part with no entry, out of range or NaN is opaque, and so is one
    /// within half an 8 bit step of 1. Ignored while [`Self::faded`].
    /// Changing it re-uploads nothing.
    pub opacity: &'a [f32],
    /// The colour each of the mesh's parts is drawn in, in the same order:
    /// [`Colors::model`] tinted ([`Srgb::tinted`]) for one with a tint,
    /// as is for one with none or no entry. Faded too. Changing it
    /// re-uploads nothing of the mesh.
    pub tints: &'a [Option<BodyTint>],
    /// Finished sketches' curves, drawn with the model as lines
    /// [`LINE_WIDTH`] wide, hidden by what's in front of them. Only
    /// re-uploaded when it's another `Arc` than the last one prepared.
    pub sketches: &'a Arc<RenderLines>,
    /// The plane the grid is drawn on.
    pub grid: GridPlane,
    /// Whether the model is drawn faded, as it is behind a sketch being
    /// edited: see [`Colors::faded_alpha`].
    pub faded: bool,
    /// Whether the mesh's wires ([`RenderMesh::wires`]) are drawn with its
    /// edges, as creases are: [`CREASE_WIDTH`] wide at [`CREASE_ALPHA`].
    /// Changing it re-uploads nothing.
    pub wireframe: bool,
    /// Whether the edges of the mesh's triangles are drawn too, as wires
    /// are. The first frame with it of a mesh works them out and uploads
    /// them.
    pub tessellation: bool,
    /// How the faces are lit.
    pub shading: Shading,
    /// Whether the feature edges the model hides are drawn too, dashed
    /// ([`HIDDEN_DASH`]), [`HIDDEN_EDGE_WIDTH`] wide, at
    /// [`Colors::hidden_edge_alpha`]. Never while [`Self::faded`].
    pub hidden_edges: bool,
    /// The faces hovered, by their ids in the mesh (their runs of
    /// [`RenderMesh::face_ends`]): the one the cursor is over, or all of
    /// what it would select (a body's). Each is drawn again over itself
    /// towards [`Colors::hover_face`], as opaque as its part (opaque
    /// over the faded model, [`Self::faded`]); and where the model hides
    /// it, over everything, washed and striped, unless
    /// [`Self::hover_through`]. Not a face the mesh hasn't.
    pub hovered_faces: &'a [u32],
    /// The faces selected, by their ids likewise: drawn again over
    /// themselves, tinted with [`Colors::selected`], over the hover; and
    /// where the model hides them, over everything, washed and striped.
    pub selected_faces: &'a [u32],
    /// The faces in the second colour (the measure tool's B), by their
    /// ids likewise: tinted with [`Colors::second`] as the selected are
    /// with theirs, over them.
    pub second_faces: &'a [u32],
    /// The edges and vertices hovered and selected, drawn over the model.
    /// Only re-uploaded when it's another `Arc` than the last one
    /// prepared, or the mesh is. Drawn over the faded model too
    /// ([`Self::faded`]): what a sketch's tool picks of it.
    pub highlights: &'a Arc<Highlights>,
    /// Whether the hovered faces, outlined edges and hovered vertices are
    /// drawn again over everything, what hides them included: a face at
    /// [`HOVER_THROUGH_ALPHA`] of its hover, the edges and vertices as
    /// they are. For an item hovered in a list of what overlaps, often
    /// hidden. Not while [`Self::faded`].
    pub hover_through: bool,
    /// The sketch being edited, or the extrude being set up, if one is:
    /// drawn over everything else, or hidden by the model in front of it
    /// ([`SketchScene::depth_tested`]).
    pub sketch: Option<SketchScene<'a>>,
    /// The geometry of the failures shown, drawn after everything but
    /// [`Self::sketch`], faded or not: solid red ([`Colors::error`]) within a halo of
    /// [`Colors::error_halo`] reaching [`ERROR_HALO`] beyond it (only the
    /// halo of [`ErrorParts::halo_only`] ones), depth tested, and at
    /// about 40 % where the model hides it. Uploaded again
    /// only when its sources ([`ErrorParts::source`]) differ from the last
    /// frame prepared's.
    pub errors: &'a [ErrorParts<'a>],
    /// The point the camera orbits, if one was picked: marked over
    /// everything but the sketch being edited, unless it shows where the
    /// origin does.
    pub pivot: Option<Pivot>,
    /// Which of the world's origin, axes and planes are drawn. The X and
    /// Y axes are the grid's axis lines while it's the world's XY plane;
    /// on another plane the grid's are drawn whatever this says. Ignored
    /// while [`Self::faded`], in a sketch, whose grid's axis lines and
    /// origin marker are drawn, and nothing else of it.
    pub origin: OriginShown,
    /// Where to draw on the target.
    pub viewport: Viewport,
    /// Size of the whole render target in physical pixels.
    pub target_size: [u32; 2],
    /// Physical pixels per logical pixel, so on-screen sizes such as the
    /// origin marker and grid spacing follow the display scale.
    pub scale_factor: f32,
    /// Colours from the UI theme.
    pub colors: Colors,
}

/// The drawable parts of a failure's geometry, for [`Frame::errors`].
#[derive(Debug, Clone)]
pub struct ErrorParts<'a> {
    /// Patches, filled red and lit as faces are: only its triangles are
    /// drawn.
    pub mesh: &'a RenderMesh,
    /// Curves, [`ERROR_EDGE_WIDTH`] wide.
    pub lines: &'a RenderLines,
    /// Points, discs of radius [`ERROR_POINT_RADIUS`]. One not finite or
    /// past [`RenderLines::MAX_POSITION`] isn't drawn.
    pub points: &'a [[f32; 3]],
    /// What these are the parts of, kept unchanged while they are (the
    /// failure's `Arc`, whose type the renderer can't name): the renderer
    /// uploads the errors again only when one of them is another
    /// allocation than the last frame's, and holds the `Weak` so that the
    /// allocation isn't reused meanwhile.
    pub source: Weak<dyn Any + Send + Sync>,
    /// Whether only their halo is drawn, not the parts themselves:
    /// something else draws them (a sketch's failing curves, which the
    /// sketch draws in red at its own width).
    pub halo_only: bool,
}

/// The marker of the point the camera orbits: a ring facing the screen
/// with a dot at the point, like the origin's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pivot {
    pub at: Vec3,
    /// From 0, not drawn, to 1, as it fades out. Out of range or not
    /// finite, it isn't drawn.
    pub opacity: f32,
}

/// An opaque sRGB-encoded colour, like CSS and UI colours. The renderer
/// converts it for its target.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Srgb(pub [f32; 3]);

/// A colour of a body's own: a hue and a saturation, its lightness the
/// colour it's applied to's ([`Srgb::tinted`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BodyTint {
    /// In degrees round the colour wheel.
    pub hue: f32,
    /// From 0, grey, to 1.
    pub saturation: f32,
}

impl Srgb {
    /// This colour with `tint`'s hue and saturation, keeping its HSL
    /// lightness: how a body's own colour is drawn on the theme's model
    /// colour, lit as it is. The saturation sets the chroma
    /// ([`TINT_CHROMA`] at 1) rather than HSL's share of what the
    /// lightness allows, so a colour is as vivid on a pale model as on a
    /// dark one, up to what the lightness allows. A hue not finite is 0, a
    /// saturation out of range clamped to it (NaN is grey).
    pub fn tinted(self, tint: BodyTint) -> Srgb {
        let [r, g, b] = self
            .0
            .map(|c| if c.is_nan() { 0.0 } else { c.clamp(0.0, 1.0) });
        let lightness = (r.max(g).max(b) + r.min(g).min(b)) / 2.0;
        let hue = if tint.hue.is_finite() {
            tint.hue.rem_euclid(360.0)
        } else {
            0.0
        };
        let saturation = if tint.saturation.is_nan() {
            0.0
        } else {
            tint.saturation.clamp(0.0, 1.0)
        };
        let chroma = (TINT_CHROMA * saturation).min(1.0 - (2.0 * lightness - 1.0).abs());
        // Each channel's distance round the wheel from its peak, in sixths.
        let channel = |n: f32| {
            let k = (n + hue / 30.0).rem_euclid(12.0);
            lightness - chroma / 2.0 * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
        };
        Srgb([channel(0.0), channel(8.0), channel(4.0)].map(|c| c.clamp(0.0, 1.0)))
    }
}

/// The chroma of a [`BodyTint`] of saturation 1, where the lightness
/// allows it: tints are drawn at this share of it, whatever the model
/// colour's lightness ([`Srgb::tinted`]).
pub const TINT_CHROMA: f32 = 0.6;

/// Scene colours, from the UI theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    /// Background gradient, top to bottom.
    pub background_top: Srgb,
    pub background_bottom: Srgb,
    /// Base colour of model faces, before lighting.
    pub model: Srgb,
    /// How far the light on the faces is spread from its middle, as a
    /// share of the regular shading's own (1): higher has faces turned to
    /// the light brighter and the rest darker, 0 lights them all alike.
    /// Regular and flat [`Shading`] only, not metal. Out of range (0 to 4)
    /// or NaN is 1.
    pub contrast: f32,
    /// Feature edges, drawn over the faces [`EDGE_WIDTH`] wide.
    pub edge: Srgb,
    /// Grid lines.
    pub grid: Srgb,
    /// The X, Y and Z axes, for the grid's axis lines.
    pub axes: [Srgb; 3],
    /// The rims of the origin marker's ring and dot, and the pivot's.
    pub origin_outline: Srgb,
    /// The core of the pivot's ring and dot.
    pub pivot: Srgb,
    /// Finished sketches' curves.
    pub sketch: Srgb,
    /// How opaque the model's faces and edges are when [`Frame::faded`],
    /// from 0 to 1. Only its nearest faces are drawn, and they still hide
    /// what's behind them from the grid and the sketches.
    pub faded_alpha: f32,
    /// How opaque the edges the model hides are drawn, from 0 to 1, of
    /// [`Self::edge`]: see [`Frame::hidden_edges`].
    pub hidden_edge_alpha: f32,
    /// The hovered faces' colour, before lighting: brighter than
    /// [`Self::model`]. See [`Frame::hovered_faces`].
    pub hover_face: Srgb,
    /// The rim around the hovered edges and vertex, for contrast with
    /// them and what's behind them: see [`HOVER_RIM`]. The selected
    /// edges' and vertices' rim is in it too, translucent: see
    /// [`SELECTED_RIM`].
    pub hover_outline: Srgb,
    /// The accent: selected faces are tinted with it, by
    /// [`Self::selected_tint`], selected edges and vertices drawn in it,
    /// shaded by [`Self::selected_edge_shade`].
    pub selected: Srgb,
    /// How far a selected face is tinted towards [`Self::selected`], from
    /// 0 to 1. Out of range or NaN is 0.6.
    pub selected_tint: f32,
    /// How far the selected edges' and vertices' colour is from
    /// [`Self::selected`], so they stand out on a selected face: from -1,
    /// black, through 0, the accent itself, to 1, white, of the way in
    /// linear light. Out of range or NaN is 0.
    pub selected_edge_shade: f32,
    /// The second colour, for the second of two picks (the measure
    /// tool's B), where the first is in [`Self::selected`]: faces tinted
    /// with it as the selected are with theirs, and edges drawn in it,
    /// unshaded. See
    /// [`Frame::second_faces`] and [`Highlights::second_edges`].
    pub second: Srgb,
    /// Error geometry ([`Frame::errors`]): its patches (lit), curves and
    /// points.
    pub error: Srgb,
    /// The halo around error geometry, translucent: drawn once over the
    /// frame wherever any of it reaches, so overlaps don't darken. Alpha
    /// out of range or NaN is 0.3.
    pub error_halo: Srgba,
}

/// The scene shader's uniforms. `Uniforms` in `scene.wgsl` mirrors this
/// field for field, and the bind group layout's `min_binding_size` makes wgpu
/// check that it fits when each pipeline is built.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    /// The eye with w = 1, or in orthographic mode the direction towards the
    /// camera with w = 0, an eye at infinity.
    eye: [f32; 4],
    /// xyz: unit vector right on screen, w: half the visible width at the
    /// target in world units.
    right: [f32; 4],
    /// xyz: unit vector up on screen, w: half the visible height at the
    /// target in world units.
    up: [f32; 4],
    /// xyz: orbit target, w: how far in front of the eye a perspective
    /// view starts, [`Camera::near`]. Not `target`, which WGSL reserves.
    focus: [f32; 4],
    /// Unit vector towards the camera.
    backward: [f32; 4],
    /// xy: viewport size in physical pixels, z: physical pixels per logical
    /// pixel.
    viewport: [f32; 4],
    /// xy: the viewport's top left corner on the target, in physical
    /// pixels, where fragment positions count from; z: [`Frame::shading`]
    /// ([`Shading::code`]); w: [`Colors::contrast`].
    viewport_origin: [f32; 4],
    /// [`Colors`], converted to linear with w = 1, except `model`, whose
    /// w is how opaque the model is, and `edge`, whose w is
    /// [`Colors::hidden_edge_alpha`]. The errors' colours are in
    /// [`ErrorUniforms`].
    background_top: [f32; 4],
    background_bottom: [f32; 4],
    model: [f32; 4],
    edge: [f32; 4],
    grid: [f32; 4],
    axes: [[f32; 4]; 3],
    origin_outline: [f32; 4],
    sketch: [f32; 4],
    /// [`Frame::pivot`]: xyz where it is, w its opacity, 0 if it isn't
    /// drawn; and its core's colour.
    pivot: [f32; 4],
    pivot_color: [f32; 4],
    /// [`Frame::grid`]: xyz its origin, and unit x and y axes. The axes'
    /// w is [`axis_index`], for the colour of its axis line; the origin's
    /// is [`OriginShown::mask`], what's drawn of the axes and the marker.
    grid_origin: [f32; 4],
    grid_x: [f32; 4],
    grid_y: [f32; 4],
    /// [`SketchScene::plane`]: xyz its origin, and unit x and y axes; the
    /// w's [`Colors::second`]'s red, green and blue.
    sketch_origin: [f32; 4],
    sketch_x: [f32; 4],
    sketch_y: [f32; 4],
    /// [`Colors::hover_face`], with w [`Colors::selected_tint`],
    /// [`Colors::hover_outline`], with w = 1, and [`Colors::selected`],
    /// with w [`Colors::selected_edge_shade`].
    hover_face: [f32; 4],
    hover_outline: [f32; 4],
    selected: [f32; 4],
}

// WGSL lays out uniform structs in 16-byte steps.
const _: () = assert!(size_of::<Uniforms>().is_multiple_of(16));
// The limits tests' device takes buffers of 512 bytes at most, and the
// uniforms are at that: a new value has to go in an unused `w`, or be
// worked out from another (as the axis lines' colours are from `grid_x.w`).
const _: () = assert!(size_of::<Uniforms>() <= 512);

/// The errors' colours, for the pipelines drawing their halo's composite
/// and their core: `ErrorColors` in `scene.wgsl` mirrors it. Their own,
/// since [`Uniforms`] are full.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ErrorUniforms {
    /// [`Colors::error`], linear, w = 1.
    core: [f32; 4],
    /// [`Colors::error_halo`], linear, its alpha as it is.
    halo: [f32; 4],
}

/// The largest buffer the renderer relies on, in bytes: WebGPU's default
/// `maxBufferSize`, which the devices iced asks for have natively and on
/// the web. A larger buffer is a validation error, and wgpu panics on those.
const MAX_BUFFER_BYTES: usize = 256 << 20;

// Every mesh the kernel allows fits, one buffer per part: positions,
// normals and indices as they are; edges at most one and a half
// `EdgePoint`s per edge vertex (closed polylines get two more), plus one at
// each end.
const _: () = assert!(size_of::<[f32; 3]>() * RenderMesh::MAX_VERTICES <= MAX_BUFFER_BYTES);
const _: () = assert!(size_of::<u32>() * RenderMesh::MAX_INDICES <= MAX_BUFFER_BYTES);
const _: () =
    assert!(size_of::<EdgePoint>() * (RenderMesh::MAX_EDGE_POINTS / 2 * 3 + 2) <= MAX_BUFFER_BYTES);
// Lines are uploaded a segment of two points each, fewer than points.
const _: () = assert!(size_of::<Segment>() * RenderLines::MAX_POINTS <= MAX_BUFFER_BYTES);

/// A segment of a line as the GPU takes it: its two ends.
type Segment = [[f32; 3]; 2];

/// A point of the stream the feature edges are drawn from (see
/// [`EdgeStream`] and `agents/viewport.md`). The stream is bound to
/// [`EDGE_SLOTS`] vertex buffer slots a point apart, so the instance
/// drawing the segment from point `i + 1` to `i + 2` sees the points either
/// side too. See `edge_segment` in the shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct EdgePoint {
    pub(crate) position: [f32; 3],
    /// How far along its polyline it is, in world units.
    along: f32,
    /// Which polyline it's on.
    pub(crate) edge: u32,
}

/// The edge of the points at the ends of the stream: no polyline's.
const NO_EDGE: u32 = u32::MAX;

/// Set in [`EdgePoint::edge`] for a point that is only a neighbour of its
/// edge's segments, where a closed polyline joins itself. As
/// `NEIGHBOUR_ONLY` in the shader.
const NEIGHBOUR_ONLY: u32 = 1 << 31;

/// Set in [`EdgePoint::edge`] for a crease's points: an edge with one face
/// on both sides. As `CREASE` in the shader.
const CREASE: u32 = 1 << 30;
const _: () = assert!(RenderMesh::MAX_EDGE_POLYLINES <= CREASE as usize);

/// How many slots the edge stream is bound to: previous, start, end and
/// next point.
const EDGE_SLOTS: u32 = 4;

// Vertex buffer offsets are multiples of 4 bytes, and wgpu's GL backend
// (the browser's WebGL2) allows strides up to 255.
const _: () = assert!(size_of::<EdgePoint>() == 20);

/// The size of an entry of a slot's tints: a linear colour, w unused. See
/// [`Slot::tints`].
const TINT_SIZE: u64 = 16;

/// The most alphas a part can be drawn at, from 0 to 1: 8 bits' worth, as
/// fine as the target shows. See [`Alphas`].
const ALPHA_STEPS: u32 = 256;

/// What a draw of a part of the mesh takes besides [`Uniforms`]. `Part`
/// in `scene.wgsl` mirrors it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct PartUniforms {
    /// x: the part's alpha, multiplying the faces' and edges' own; yzw
    /// unused.
    alpha: [f32; 4],
}

/// The alphas the mesh's parts are drawn at: a uniform buffer, written
/// once, of a [`PartUniforms`] for each of [`ALPHA_STEPS`] steps from 0 to
/// 1 at the device's uniform offset alignment, picked per draw by dynamic
/// offset. WebGL2 has no push constants. A device whose buffers hold fewer
/// gets fewer, at least one, opaque.
struct Alphas {
    /// Group 1 of every pipeline.
    group: wgpu::BindGroup,
    /// How far apart the steps are in the buffer, in bytes.
    stride: u32,
    /// The last step, opaque.
    opaque: u32,
}

impl Alphas {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Alphas {
        let size = size_of::<PartUniforms>() as u32;
        let limits = device.limits();
        let stride = limits.min_uniform_buffer_offset_alignment.max(size);
        let fit = limits.max_buffer_size / u64::from(stride);
        let steps = u32::try_from(fit).unwrap_or(u32::MAX).clamp(1, ALPHA_STEPS);
        let opaque = steps - 1;
        let mut table = vec![0u8; (steps * stride) as usize];
        for (step, entry) in (0..steps).zip(table.chunks_exact_mut(stride as usize)) {
            let alpha = if step == opaque {
                1.0
            } else {
                step as f32 / opaque as f32
            };
            let part = PartUniforms {
                alpha: [alpha, 0.0, 0.0, 0.0],
            };
            entry[..size as usize].copy_from_slice(bytemuck::bytes_of(&part));
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde part alphas"),
            contents: &table,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("varde part alphas"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(u64::from(size)),
                }),
            }],
        });
        Alphas {
            group,
            stride,
            opaque,
        }
    }

    /// The step a part of opacity `alpha` is drawn at, see [`alpha_step`].
    fn step(&self, alpha: Option<f32>) -> u32 {
        alpha_step(alpha, self.opaque)
    }

    /// Sets the alpha the parts drawn next are drawn at to `step`'s.
    fn set(&self, pass: &mut wgpu::RenderPass<'_>, step: u32) {
        pass.set_bind_group(1, &self.group, &[step * self.stride]);
    }
}

/// The nearest step to `alpha` of a table of alphas from 0 to 1 whose last
/// step, `opaque`, is opaque; opaque if `alpha` is out of range or NaN.
/// Only 0 gets step 0, so on a short table (tiny buffers) a faint part is
/// drawn at the next step up rather than not at all.
fn alpha_step(alpha: Option<f32>, opaque: u32) -> u32 {
    match alpha {
        Some(0.0) => 0,
        // In range, so the cast is exact.
        Some(alpha) if (0.0..=1.0).contains(&alpha) => {
            ((alpha * opaque as f32).round() as u32).max(1).min(opaque)
        }
        _ => opaque,
    }
}

struct GpuMesh {
    positions: wgpu::Buffer,
    normals: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// The [`EdgePoint`] stream, if there are any edges or wires.
    edges: Option<wgpu::Buffer>,
    /// Whether the wires are drawn with the edges: [`Frame::wireframe`].
    wireframe: bool,
    /// The edges of the triangles, once a frame asked for them
    /// ([`Frame::tessellation`]), and whether it does.
    triangles: Triangles,
    tessellation: bool,
    /// The mesh's parts, as [`RenderMesh::parts`] gives them; one for the
    /// whole mesh if it has none.
    parts: Vec<GpuPart>,
    /// To fit the depth range to.
    bounds: Option<Aabb>,
}

impl GpuMesh {
    /// The indices of face `face` of `mesh`, the mesh uploaded, and the
    /// part it's of, if it has one.
    fn face(&self, mesh: &RenderMesh, face: u32) -> Option<(Range<u32>, usize)> {
        let indices = mesh.face_indices(usize::try_from(face).ok()?)?;
        let part = self.parts.partition_point(|part| part.faces.end <= face);
        // The kernel bounds indices well within `u32`.
        let to_u32 = |n| u32::try_from(n).ok();
        let indices = to_u32(indices.start)?..to_u32(indices.end)?;
        (part < self.parts.len()).then_some((indices, part))
    }
}

/// The edges of a mesh's triangles, for [`Frame::tessellation`].
enum Triangles {
    /// Not worked out yet.
    Unbuilt,
    /// Not uploaded: they don't fit in a buffer.
    TooLarge,
    /// Each part's edges, in the order of [`GpuMesh::parts`]: their
    /// segments follow one another in `segments`.
    Built {
        segments: wgpu::Buffer,
        parts: Vec<Range<u32>>,
    },
    /// The mesh has none.
    Empty,
}

/// A face of the mesh drawn again over itself, hovered or selected: see
/// [`Frame::hovered_faces`].
#[derive(Debug, Clone)]
struct FaceDraw {
    indices: Range<u32>,
    /// Its part's alpha's step, see [`Alphas`].
    step: u32,
    /// Brightened as hovered, or tinted as selected or in the second
    /// colour.
    tint: Tint,
    /// Its part, for its colour ([`Slot::part_tints`]).
    part: usize,
    /// Whether it's both hovered and selected: its hidden stripes are
    /// then the selection's alone, from the hover's colour, as it's shown.
    hovered: bool,
}

/// How a [`FaceDraw`] is drawn again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tint {
    Hovered,
    Selected,
    Second,
}

/// What the renderer keeps of a part of the mesh to draw it on its own.
#[derive(Debug, Clone)]
struct GpuPart {
    /// Its faces' ids.
    faces: Range<u32>,
    /// Its triangles' indices.
    indices: Range<u32>,
    /// Its edges' points in the [`EdgePoint`] stream, neighbour only ones
    /// included. The parts' follow one another from the stream's second
    /// point, and all the parts' wires follow them.
    points: Range<u32>,
    /// Its wires' points in the stream, likewise.
    wires: Range<u32>,
    /// Its triangles' bounds, to sort parts less than opaque by.
    bounds: Option<Aabb>,
}

/// Which parts of a frame's mesh are drawn how, worked out in
/// [`Renderer::prepare`] from [`Frame::opacity`].
#[derive(Debug, Default)]
struct PartDraws {
    /// The runs of opaque parts, one after another, each drawn at once.
    opaque: Vec<Range<usize>>,
    /// The parts less than opaque, far to near, and their alphas' steps.
    transparent: Vec<(usize, u32)>,
}

impl PartDraws {
    /// How `parts` are drawn at `opacity`, at `alphas`' steps, in the
    /// colours of `tints`' entries ([`Slot::part_tints`]), seen from
    /// `camera`. A run of opaque parts is of one colour.
    fn new(
        parts: &[GpuPart],
        opacity: &[f32],
        tints: &[u32],
        alphas: &Alphas,
        camera: &Camera,
    ) -> PartDraws {
        let mut draws = PartDraws::default();
        let tint = |i: usize| tints.get(i).copied().unwrap_or(0);
        for i in 0..parts.len() {
            let step = alphas.step(opacity.get(i).copied());
            if step < alphas.opaque {
                draws.transparent.push((i, step));
            } else if let Some(run) =
                (draws.opaque.last_mut()).filter(|run| run.end == i && tint(run.start) == tint(i))
            {
                run.end = i + 1;
            } else {
                draws.opaque.push(i..i + 1);
            }
        }
        // Far to near by their bounds' centres along the view, the same in
        // either projection.
        let backward = camera.backward();
        let depth = |&(i, _): &(usize, u32)| {
            parts[i]
                .bounds
                .map_or(0.0, |bounds| bounds.center().dot(backward))
        };
        draws
            .transparent
            .sort_by(|a, b| depth(a).total_cmp(&depth(b)));
        draws
    }
}

struct GpuLines {
    segments: wgpu::Buffer,
    segment_count: u32,
    /// To fit the depth range to.
    bounds: Option<Aabb>,
}

struct DepthTarget {
    view: wgpu::TextureView,
    size: [u32; 2],
}

/// A pipeline's stencil state when it neither tests nor writes it.
const NO_STENCIL: wgpu::StencilState = wgpu::StencilState {
    front: wgpu::StencilFaceState::IGNORE,
    back: wgpu::StencilFaceState::IGNORE,
    read_mask: 0,
    write_mask: 0,
};

/// The stencil bits [`Renderer::draw_hidden_picks`] marks where the
/// hovered and the selected faces are the nearest of the model with, so
/// what they hide of themselves (a selected body's far side) isn't drawn
/// as hidden. Above the glass's references.
const PICK_MARKS: [u32; 2] = [0x40, 0x80];

/// The errors' halo's blending: each pixel keeps the most coverage drawn
/// there, so overlapping halos don't add up.
const MAX_BLENDING: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Max,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Max,
    },
};

/// What differs between the renderer's pipelines. They all share the scene
/// shader, the uniform layout and the depth buffer.
#[derive(Clone)]
struct Pass<'a> {
    label: &'a str,
    /// Vertex and fragment entry points in the scene shader.
    vs: &'a str,
    fs: &'a str,
    buffers: &'a [wgpu::VertexBufferLayout<'a>],
    depth_write: bool,
    depth_compare: wgpu::CompareFunction,
    blend: wgpu::BlendState,
    write_mask: wgpu::ColorWrites,
    topology: wgpu::PrimitiveTopology,
    cull_mode: Option<wgpu::Face>,
    stencil: wgpu::StencilState,
    /// Sets the shader's `SKETCH_DEPTH`: the sketch's layers are given
    /// their depth, pulled towards the camera, rather than drawn on top.
    sketch_depth: bool,
    /// The target's format if not the frame's: the errors' halo's.
    format: Option<wgpu::TextureFormat>,
    /// The layout if not the scene's: the errors' composite's and core's,
    /// which read their colours and the halo's coverage (group 0).
    layout: Option<&'a wgpu::PipelineLayout>,
}

impl<'a> Pass<'a> {
    /// Triangles drawn over the scene without writing depth, blended and
    /// not culled.
    const fn overlay(label: &'a str, vs: &'a str, fs: &'a str) -> Self {
        Pass {
            label,
            vs,
            fs,
            buffers: &[],
            depth_write: false,
            depth_compare: wgpu::CompareFunction::LessEqual,
            blend: wgpu::BlendState::ALPHA_BLENDING,
            write_mask: wgpu::ColorWrites::ALL,
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            stencil: NO_STENCIL,
            sketch_depth: false,
            format: None,
            layout: None,
        }
    }

    /// Like [`Self::on_top`], hidden by what's nearer, its depth from
    /// the shader with `SKETCH_DEPTH` set.
    const fn depth_tested(
        label: &'a str,
        vs: &'a str,
        fs: &'a str,
        buffers: &'a [wgpu::VertexBufferLayout<'a>],
    ) -> Self {
        Pass {
            buffers,
            sketch_depth: true,
            ..Pass::overlay(label, vs, fs)
        }
    }

    /// Like [`Self::overlay`], drawn over everything whatever its depth,
    /// taking a buffer of `buffers`.
    const fn on_top(
        label: &'a str,
        vs: &'a str,
        fs: &'a str,
        buffers: &'a [wgpu::VertexBufferLayout<'a>],
    ) -> Self {
        Pass {
            buffers,
            depth_compare: wgpu::CompareFunction::Always,
            ..Pass::overlay(label, vs, fs)
        }
    }
}

/// Why [`Renderer::prepare`] couldn't upload a frame's mesh or lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    /// A buffer of the mesh would take `bytes`, more than the device's
    /// `max_buffer_size` of `limit`.
    MeshTooLarge { bytes: u64, limit: u64 },
    /// The same for the sketches' lines.
    LinesTooLarge { bytes: u64, limit: u64 },
    /// The same for a layer of the sketch being edited.
    SketchTooLarge { bytes: u64, limit: u64 },
    /// The same for the hovered and selected edges and vertices.
    HighlightsTooLarge { bytes: u64, limit: u64 },
    /// The same for the errors' geometry ([`Frame::errors`]).
    ErrorsTooLarge { bytes: u64, limit: u64 },
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrepareError::MeshTooLarge { bytes, limit } => write!(
                f,
                "the mesh needs a {bytes}-byte buffer, but the device allows at most {limit} bytes"
            ),
            PrepareError::LinesTooLarge { bytes, limit } => write!(
                f,
                "the sketches need a {bytes}-byte buffer, but the device allows at most {limit} \
                 bytes"
            ),
            PrepareError::SketchTooLarge { bytes, limit } => write!(
                f,
                "the sketch being edited needs a {bytes}-byte buffer, but the device allows at \
                 most {limit} bytes"
            ),
            PrepareError::HighlightsTooLarge { bytes, limit } => write!(
                f,
                "what's hovered and selected needs a {bytes}-byte buffer, but the device allows \
                 at most {limit} bytes"
            ),
            PrepareError::ErrorsTooLarge { bytes, limit } => write!(
                f,
                "the errors' geometry needs a {bytes}-byte buffer, but the device allows at most \
                 {limit} bytes"
            ),
        }
    }
}

impl std::error::Error for PrepareError {}

/// Renders a scene into a caller-provided color target.
///
/// Holds only what every scene shares; what one scene draws is in a
/// [`Slot`]. Call [`Renderer::prepare`] to upload a frame into a slot, then
/// [`Renderer::render`] to record its draw commands.
pub struct Renderer {
    background: wgpu::RenderPipeline,
    grid: wgpu::RenderPipeline,
    mesh: wgpu::RenderPipeline,
    /// The faded model: its depth first, then its nearest faces blended.
    mesh_depth: wgpu::RenderPipeline,
    mesh_faded: wgpu::RenderPipeline,
    /// Parts less than opaque: all their back faces, then each one's front
    /// faces over their depth ([`Self::mesh_depth`]), blended, not writing
    /// depth.
    glass_back: wgpu::RenderPipeline,
    glass_front: wgpu::RenderPipeline,
    edges: wgpu::RenderPipeline,
    /// The edges again where the model hides them, dashed.
    hidden_edges: wgpu::RenderPipeline,
    /// A part less than opaque's front faces' depth, marking where it's
    /// the nearest in the stencil, and the edges it hides there, dashed.
    glass_depth: wgpu::RenderPipeline,
    hidden_by_glass: wgpu::RenderPipeline,
    /// Faces drawn again over themselves (`Equal`), hovered and selected.
    hover_face: wgpu::RenderPipeline,
    selected_face: wgpu::RenderPipeline,
    second_face: wgpu::RenderPipeline,
    /// The rim of the hovered edges' outline, the hovered edges within
    /// it, the selected edges, and the hovered and selected vertices.
    outline: wgpu::RenderPipeline,
    hovered_edges: wgpu::RenderPipeline,
    selected_outline: wgpu::RenderPipeline,
    selected_edges: wgpu::RenderPipeline,
    /// The edges in the second colour, with their rim.
    second_outline: wgpu::RenderPipeline,
    second_edges: wgpu::RenderPipeline,
    vertices: wgpu::RenderPipeline,
    /// The hovered faces, outlined edges and hovered vertices again over
    /// everything: [`Frame::hover_through`].
    hover_face_through: wgpu::RenderPipeline,
    outline_through: wgpu::RenderPipeline,
    hovered_edges_through: wgpu::RenderPipeline,
    vertices_through: wgpu::RenderPipeline,
    /// What the model hides of the selected and hovered faces and edges,
    /// over everything: the faces washed and striped, the edges dashed.
    selected_face_hidden: wgpu::RenderPipeline,
    hovered_selected_face_hidden: wgpu::RenderPipeline,
    /// Marking where a hovered or selected face is the nearest in the
    /// stencil (`PICK_MARKS`), which the hidden faces are drawn outside.
    hovered_face_mark: wgpu::RenderPipeline,
    selected_face_mark: wgpu::RenderPipeline,
    selected_edges_hidden: wgpu::RenderPipeline,
    hovered_face_hidden: wgpu::RenderPipeline,
    hovered_edges_hidden: wgpu::RenderPipeline,
    /// The rims of the hidden hovered and selected edges, dashed.
    outline_hidden: wgpu::RenderPipeline,
    selected_outline_hidden: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    /// The edges of the mesh's triangles: [`Frame::tessellation`].
    /// The finished sketches' lines again where glass is the nearest of
    /// the model (the stencil isn't 0), so what's in front of it isn't
    /// dimmed by it.
    lines_over_glass: wgpu::RenderPipeline,
    triangle_edges: wgpu::RenderPipeline,
    origin: wgpu::RenderPipeline,
    /// The origin planes shown: [`Frame::origin`].
    origin_planes: wgpu::RenderPipeline,
    /// The sketch being edited: its fills, lines and points, drawn over
    /// everything, and the same hidden by the model in front of them
    /// ([`SketchScene::depth_tested`]).
    sketch_on_top: SketchPipelines,
    sketch_depth_tested: SketchPipelines,
    /// The depth tested sketch again over glass, as [`Self::lines_over_glass`].
    sketch_over_glass: SketchPipelines,
    /// The errors' geometry: its halo, its composite and its core.
    errors: ErrorPipelines,
    bind_group_layout: wgpu::BindGroupLayout,
    alphas: Alphas,
    /// How far apart the entries of a slot's tints are, in bytes: see
    /// [`Slot::tints`].
    tint_stride: u32,
    depth_format: wgpu::TextureFormat,
    /// The target's format, which the pipelines are built for.
    format: wgpu::TextureFormat,
}

/// One scene's state on the GPU, from [`Renderer::prepare`] to
/// [`Renderer::render`]: its uniforms, mesh, lines and depth buffer.
///
/// Each scene drawn in the same frame needs its own slot, since every
/// `prepare` may run before any `render`. Keeping a slot across frames
/// keeps its mesh uploaded.
pub struct Slot {
    uniforms: wgpu::Buffer,
    /// Group 0: the uniforms, and the tints at a dynamic offset.
    bind_group: wgpu::BindGroup,
    /// The colours the mesh's parts are drawn in, linear, an entry each
    /// [`Renderer::tint_stride`] apart: the first [`Colors::model`], then
    /// one for each other colour of [`Frame::tints`]. Written each
    /// prepare; made again, with the bind group, when it needs more room.
    tints: wgpu::Buffer,
    /// How many entries `tints` holds.
    tint_room: u32,
    /// The entry of `tints` each part of the mesh is drawn in.
    part_tints: Vec<u32>,
    mesh: Option<GpuMesh>,
    /// The [`Frame::mesh`] `mesh` was uploaded from, or skipped for. Holding
    /// it keeps the allocation from being reused for another mesh, and
    /// `Arc::get_mut` from changing that mesh in place.
    source: Weak<RenderMesh>,
    lines: Option<GpuLines>,
    /// The [`Frame::sketches`] `lines` were uploaded from, like `source`.
    lines_source: Weak<RenderLines>,
    /// The layers of [`Frame::sketch`], and the base layer's `Arc` like
    /// `source`. The live layer's buffers are kept to be written again.
    sketch_base: SketchBuffers,
    sketch_source: Weak<SketchLayer>,
    sketch_live: SketchBuffers,
    /// Whether the frame has a sketch to draw, and whether it's depth
    /// tested.
    sketching: bool,
    sketch_depth: bool,
    depth: Option<DepthTarget>,
    viewport: Viewport,
    faded: bool,
    /// [`Frame::hover_through`], never while faded.
    hover_through: bool,
    /// How the mesh's parts are drawn, by [`Frame::opacity`].
    draws: PartDraws,
    /// Whether the edges the model hides are drawn: [`Frame::hidden_edges`]
    /// and not [`Frame::faded`].
    hidden_edges: bool,
    /// What's drawn of [`Frame::origin`]: while [`Frame::faded`], in a
    /// sketch, only the grid's axis lines and the marker, the sketch's.
    origin: OriginShown,
    /// The faces hovered and selected, the hovered first: none while
    /// [`Frame::faded`].
    faces: Vec<FaceDraw>,
    /// [`Frame::highlights`] as uploaded, and its `Arc` like `source`.
    highlights: HighlightBuffers,
    highlights_source: Weak<Highlights>,
    /// [`Frame::errors`] as uploaded, and their sources like `source`.
    errors: ErrorBuffers,
    error_sources: Vec<Weak<dyn Any + Send + Sync>>,
    /// The halo's target and the errors' colours, made the first time
    /// there are errors and again when the target is resized while there
    /// are; kept while there are none, so showing and hiding them (a
    /// hover) makes no texture.
    error_target: Option<ErrorTarget>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = |label| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/scene.wgsl").into()),
            })
        };
        let shader = module("varde scene");
        // The same for the sketch's depth tested pipelines, which differ
        // from the others only by `SKETCH_DEPTH`: wgpu's GL backend (the
        // browser's WebGL2) caches a program by its module and entry
        // points, not its pipeline constants, so on one module they'd get
        // the on-top program.
        let depth_shader = module("varde scene, sketch depth tested");

        // A uniform buffer of at least `size` bytes, at a dynamic offset
        // if `dynamic`, seen by both stages.
        let uniform_layout = |label, size: usize, dynamic| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: dynamic,
                        min_binding_size: wgpu::BufferSize::new(size as u64),
                    },
                    count: None,
                }],
            })
        };
        // The uniforms, and at binding 3 (the errors' group 0 has 1 and 2)
        // the colour of the part drawn, at a dynamic offset into the
        // slot's tints: see `Slot::tints`. Group 0 rather than a group of
        // its own: iced asks the device for two bind groups only.
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varde uniforms"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<Uniforms>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(TINT_SIZE),
                    },
                    count: None,
                },
            ],
        });
        let tint_stride =
            (device.limits().min_uniform_buffer_offset_alignment).max(TINT_SIZE as u32);
        // How opaque the part drawn is, at a dynamic offset into the
        // alphas' buffer: see `Alphas`. Every pipeline has it, so
        // it's bound once for all of them.
        let part_layout = uniform_layout("varde part", size_of::<PartUniforms>(), true);
        let alphas = Alphas::new(device, &part_layout);
        let depth_format = depth_format(device);

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varde scene"),
            bind_group_layouts: &[&bind_group_layout, &part_layout],
            push_constant_ranges: &[],
        });
        // Group 0 of the halo's composite and the errors' core: the scene's
        // uniforms as everywhere, their colours (`ErrorUniforms`), and the
        // halo's coverage, read texel by texel (`textureLoad`, no sampler)
        // by the composite. In group 0 rather than a group of their own:
        // iced asks the device for two bind groups only.
        let errors_group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varde errors"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<Uniforms>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<ErrorUniforms>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let errors_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varde errors"),
            bind_group_layouts: &[&errors_group, &part_layout],
            push_constant_ranges: &[],
        });

        // Overrides in the scene shader. `ENCODE_SRGB` is set if the target
        // stores what the shader writes as is, so the shader must sRGB-encode
        // its linear output.
        // `SKETCH_DEPTH` is set for the sketch's depth tested layers.
        let constants = |sketch_depth: bool| {
            [
                ("GRID_FADE_HEIGHTS", f64::from(GRID_FADE_HEIGHTS)),
                ("LINE_WIDTH", f64::from(LINE_WIDTH)),
                ("EDGE_WIDTH", f64::from(EDGE_WIDTH)),
                ("HIDDEN_EDGE_WIDTH", f64::from(HIDDEN_EDGE_WIDTH)),
                ("CREASE_WIDTH", f64::from(CREASE_WIDTH)),
                ("CREASE_ALPHA", f64::from(CREASE_ALPHA)),
                ("HIDDEN_DASH", f64::from(HIDDEN_DASH[0])),
                ("HIDDEN_GAP", f64::from(HIDDEN_DASH[1])),
                ("HOVERED_EDGE_WIDTH", f64::from(HOVERED_EDGE_WIDTH)),
                ("HOVER_RIM", f64::from(HOVER_RIM)),
                ("HOVER_THROUGH_ALPHA", f64::from(HOVER_THROUGH_ALPHA)),
                ("SELECTED_RIM", f64::from(SELECTED_RIM)),
                ("SELECTED_EDGE_WIDTH", f64::from(SELECTED_EDGE_WIDTH)),
                ("VERTEX_RADIUS", f64::from(VERTEX_RADIUS)),
                ("ERROR_EDGE_WIDTH", f64::from(ERROR_EDGE_WIDTH)),
                ("ERROR_HALO", f64::from(ERROR_HALO)),
                ("ERROR_POINT_RADIUS", f64::from(ERROR_POINT_RADIUS)),
                ("ENCODE_SRGB", if format.is_srgb() { 0.0 } else { 1.0 }),
                ("SKETCH_DEPTH", if sketch_depth { 1.0 } else { 0.0 }),
            ]
        };

        let pipeline = |pass: Pass<'_>| {
            let constants = constants(pass.sketch_depth);
            let shader = if pass.sketch_depth {
                &depth_shader
            } else {
                &shader
            };
            let compilation_options = wgpu::PipelineCompilationOptions {
                constants: &constants,
                ..Default::default()
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(pass.label),
                layout: Some(pass.layout.unwrap_or(&layout)),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(pass.vs),
                    buffers: pass.buffers,
                    compilation_options: compilation_options.clone(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(pass.fs),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: pass.format.unwrap_or(format),
                        blend: Some(pass.blend),
                        write_mask: pass.write_mask,
                    })],
                    compilation_options: compilation_options.clone(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: pass.topology,
                    cull_mode: pass.cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth_format,
                    depth_write_enabled: pass.depth_write,
                    depth_compare: pass.depth_compare,
                    stencil: pass.stencil,
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };

        // Positions in slot 0 and normals in slot 1, each a buffer of its
        // own. Lines take a segment per instance, its ends in slots 0 and 1.
        let positions = wgpu::VertexBufferLayout {
            array_stride: size_of::<[f32; 3]>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
        };
        let normals = wgpu::VertexBufferLayout {
            array_stride: size_of::<[f32; 3]>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![1 => Float32x3],
        };
        let segments = wgpu::VertexBufferLayout {
            array_stride: size_of::<Segment>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
        };
        // The sketch being edited: a segment or a point per instance, and
        // fills as triangles.
        let sketch_lines = wgpu::VertexBufferLayout {
            array_stride: size_of::<LineInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4,
                4 => Float32x4, 5 => Float32x2, 6 => Uint32,
            ],
        };
        let sketch_points = wgpu::VertexBufferLayout {
            array_stride: size_of::<PointInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4,
                4 => Float32, 5 => Uint32,
            ],
        };
        let sketch_fills = wgpu::VertexBufferLayout {
            array_stride: size_of::<FillVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2, 1 => Uint32, 2 => Float32x4, 3 => Float32,
            ],
        };
        // The feature edges: the `EdgePoint` stream in slots 0 to 3, a
        // point apart, an instance per segment. Only the start needs
        // `along`, for the dashes. 9 attributes and 4 slots, within
        // WebGL2's 16 and 8.
        let attribute = |format, offset: usize, shader_location| wgpu::VertexAttribute {
            format,
            offset: offset as u64,
            shader_location,
        };
        let position = |location| {
            attribute(
                wgpu::VertexFormat::Float32x3,
                std::mem::offset_of!(EdgePoint, position),
                location,
            )
        };
        let along = |location| {
            attribute(
                wgpu::VertexFormat::Float32,
                std::mem::offset_of!(EdgePoint, along),
                location,
            )
        };
        let edge = |location| {
            attribute(
                wgpu::VertexFormat::Uint32,
                std::mem::offset_of!(EdgePoint, edge),
                location,
            )
        };
        let edge_attributes: [&[_]; EDGE_SLOTS as usize] = [
            &[position(0), edge(1)],
            &[position(2), along(3), edge(4)],
            &[position(5), edge(6)],
            &[position(7), edge(8)],
        ];
        let edge_points = edge_attributes.map(|attributes| wgpu::VertexBufferLayout {
            array_stride: size_of::<EdgePoint>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        });
        // The hovered and selected vertices, a disc per instance.
        let vertices = wgpu::VertexBufferLayout {
            array_stride: size_of::<VertexInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32],
        };
        let faces = [positions, normals];
        let mesh = Pass {
            label: "varde mesh",
            vs: "vs_mesh",
            fs: "fs_mesh",
            buffers: &faces,
            depth_write: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            blend: wgpu::BlendState::REPLACE,
            write_mask: wgpu::ColorWrites::ALL,
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: Some(wgpu::Face::Back),
            stencil: NO_STENCIL,
            sketch_depth: false,
            format: None,
            layout: None,
        };
        // Tested against the stencil reference by `compare`, and on a pass
        // of both tests, `pass` done to it.
        let tagged = |compare, pass| {
            let face = wgpu::StencilFaceState {
                compare,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: pass,
            };
            wgpu::StencilState {
                front: face,
                back: face,
                read_mask: 0xff,
                write_mask: 0xff,
            }
        };
        // Tested and written as `tagged`, but only in the stencil bit
        // `bit`, the reference.
        use wgpu::StencilOperation::{Keep, Replace};
        let marked = |bit: u32, compare, pass| {
            let face = wgpu::StencilFaceState {
                compare,
                fail_op: Keep,
                depth_fail_op: Keep,
                pass_op: pass,
            };
            wgpu::StencilState {
                front: face,
                back: face,
                read_mask: bit,
                write_mask: bit,
            }
        };
        // Where glass is the nearest of the model: its parts' references
        // are never 0, which is drawn with as the reference.
        let over_glass = tagged(
            wgpu::CompareFunction::NotEqual,
            wgpu::StencilOperation::Keep,
        );
        // Lines drawn from the `EdgePoint` stream, over the scene.
        // Pulled in by their distance from the edge, as the highlights'
        // lines below are.
        let edge_pass = |label, vs| Pass {
            buffers: &edge_points,
            ..Pass::overlay(label, vs, "fs_highlight_line")
        };
        // The hover and the selection's lines (`highlight_slope` in the
        // shader).
        let highlight_pass = |label, vs| edge_pass(label, vs);
        // Over the faces' own pixels and no others: the same vertex
        // shader, its position invariant, at exactly their depth.
        let redrawn = |label, fs| {
            pipeline(Pass {
                label,
                fs,
                blend: wgpu::BlendState::ALPHA_BLENDING,
                depth_write: false,
                depth_compare: wgpu::CompareFunction::Equal,
                ..mesh.clone()
            })
        };

        // The errors' geometry where it shows and where the model hides
        // it: the same programs, so the two split the pixels between them.
        let (seen, hidden) = (
            wgpu::CompareFunction::LessEqual,
            wgpu::CompareFunction::Greater,
        );

        Self {
            background: pipeline(Pass {
                blend: wgpu::BlendState::REPLACE,
                ..Pass::overlay("varde background", "vs_fullscreen", "fs_background")
            }),
            grid: pipeline(Pass::overlay("varde grid", "vs_fullscreen", "fs_grid")),
            mesh_depth: pipeline(Pass {
                label: "varde faded mesh depth",
                write_mask: wgpu::ColorWrites::empty(),
                ..mesh.clone()
            }),
            // Marking the part's pixels where it's the nearest of the model
            // so far with the stencil reference.
            glass_depth: pipeline(Pass {
                label: "varde glass depth",
                write_mask: wgpu::ColorWrites::empty(),
                stencil: tagged(
                    wgpu::CompareFunction::Always,
                    wgpu::StencilOperation::Replace,
                ),
                ..mesh.clone()
            }),
            // Drawn over its own depth, so only the nearest faces pass.
            mesh_faded: pipeline(Pass {
                label: "varde faded mesh",
                blend: wgpu::BlendState::ALPHA_BLENDING,
                ..mesh.clone()
            }),
            // Hidden by the opaque parts, and leaving the depth to them.
            glass_back: pipeline(Pass {
                label: "varde glass, back faces",
                blend: wgpu::BlendState::ALPHA_BLENDING,
                depth_write: false,
                cull_mode: Some(wgpu::Face::Front),
                ..mesh.clone()
            }),
            glass_front: pipeline(Pass {
                label: "varde glass, front faces",
                blend: wgpu::BlendState::ALPHA_BLENDING,
                depth_write: false,
                ..mesh.clone()
            }),
            hover_face: redrawn("varde hovered face", "fs_hover_face"),
            selected_face: redrawn("varde selected face", "fs_selected_face"),
            second_face: redrawn("varde second face", "fs_second_face"),
            // Over everything, either side: entry points of their own.
            hover_face_through: pipeline(Pass {
                label: "varde hovered face through",
                fs: "fs_hover_face_through",
                blend: wgpu::BlendState::ALPHA_BLENDING,
                depth_write: false,
                depth_compare: wgpu::CompareFunction::Always,
                cull_mode: None,
                ..mesh.clone()
            }),
            // Where something hides it, either side, but not where the
            // faces so tinted are the nearest, as marked.
            selected_face_hidden: pipeline(Pass {
                label: "varde selected face hidden",
                fs: "fs_selected_face_hidden",
                blend: wgpu::BlendState::ALPHA_BLENDING,
                depth_write: false,
                depth_compare: wgpu::CompareFunction::Greater,
                cull_mode: None,
                stencil: marked(PICK_MARKS[1], wgpu::CompareFunction::NotEqual, Keep),
                ..mesh.clone()
            }),
            hovered_selected_face_hidden: pipeline(Pass {
                label: "varde hovered selected face hidden",
                fs: "fs_hovered_selected_face_hidden",
                blend: wgpu::BlendState::ALPHA_BLENDING,
                depth_write: false,
                depth_compare: wgpu::CompareFunction::Greater,
                cull_mode: None,
                stencil: marked(PICK_MARKS[1], wgpu::CompareFunction::NotEqual, Keep),
                ..mesh.clone()
            }),
            hovered_face_hidden: pipeline(Pass {
                label: "varde hovered face hidden",
                fs: "fs_hovered_face_hidden",
                blend: wgpu::BlendState::ALPHA_BLENDING,
                depth_write: false,
                depth_compare: wgpu::CompareFunction::Greater,
                cull_mode: None,
                stencil: marked(PICK_MARKS[0], wgpu::CompareFunction::NotEqual, Keep),
                ..mesh.clone()
            }),
            hovered_face_mark: pipeline(Pass {
                label: "varde hovered face mark",
                write_mask: wgpu::ColorWrites::empty(),
                depth_write: false,
                depth_compare: wgpu::CompareFunction::Equal,
                cull_mode: None,
                stencil: marked(PICK_MARKS[0], wgpu::CompareFunction::Always, Replace),
                ..mesh.clone()
            }),
            selected_face_mark: pipeline(Pass {
                label: "varde selected face mark",
                write_mask: wgpu::ColorWrites::empty(),
                depth_write: false,
                depth_compare: wgpu::CompareFunction::Equal,
                cull_mode: None,
                stencil: marked(PICK_MARKS[1], wgpu::CompareFunction::Always, Replace),
                ..mesh.clone()
            }),
            mesh: pipeline(mesh),
            // Entry points of their own, which wgpu's GL backend keys
            // programs by.
            outline: pipeline(highlight_pass("varde hover outline", "vs_outline")),
            hovered_edges: pipeline(highlight_pass("varde hovered edges", "vs_hovered_edge")),
            selected_outline: pipeline(highlight_pass(
                "varde selected outline",
                "vs_selected_outline",
            )),
            selected_edges: pipeline(highlight_pass("varde selected edges", "vs_selected_edge")),
            // Exactly the pixels the selected edges didn't draw, as the
            // hidden edges are the visible ones'.
            selected_edges_hidden: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                ..highlight_pass("varde selected edges hidden", "vs_selected_edge_hidden")
            }),
            outline_hidden: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                ..highlight_pass("varde hover outline hidden", "vs_outline_hidden")
            }),
            selected_outline_hidden: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                ..highlight_pass(
                    "varde selected outline hidden",
                    "vs_selected_outline_hidden",
                )
            }),
            hovered_edges_hidden: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                ..highlight_pass("varde hovered edges hidden", "vs_hovered_edge_hidden")
            }),
            second_outline: pipeline(highlight_pass("varde second outline", "vs_second_outline")),
            second_edges: pipeline(highlight_pass("varde second edges", "vs_second_edge")),
            vertices: pipeline(Pass {
                buffers: std::slice::from_ref(&vertices),
                ..Pass::overlay("varde vertices", "vs_vertex", "fs_highlight_point")
            }),
            outline_through: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Always,
                ..highlight_pass("varde hover outline through", "vs_outline_through")
            }),
            hovered_edges_through: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Always,
                ..highlight_pass("varde hovered edges through", "vs_hovered_edge_through")
            }),
            vertices_through: pipeline(Pass {
                buffers: std::slice::from_ref(&vertices),
                depth_compare: wgpu::CompareFunction::Always,
                ..Pass::overlay(
                    "varde hovered vertices through",
                    "vs_vertex_through",
                    "fs_highlight_point",
                )
            }),
            edges: pipeline(edge_pass("varde edges", "vs_edge")),
            // Exactly the pixels the visible edges didn't draw: the same
            // quads at the same depth, tested the other way round. Its own
            // entry point, which wgpu's GL backend keys programs by.
            hidden_edges: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                ..edge_pass("varde hidden edges", "vs_hidden_edge")
            }),
            // The same where the stencil has the reference: behind the
            // part less than opaque nearest there.
            hidden_by_glass: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: tagged(wgpu::CompareFunction::Equal, wgpu::StencilOperation::Keep),
                ..edge_pass("varde edges hidden by glass", "vs_hidden_edge")
            }),
            lines: pipeline(Pass {
                buffers: std::slice::from_ref(&segments),
                ..Pass::overlay("varde sketch lines", "vs_line", "fs_line")
            }),
            lines_over_glass: pipeline(Pass {
                buffers: std::slice::from_ref(&segments),
                stencil: over_glass.clone(),
                ..Pass::overlay("varde sketch lines over glass", "vs_line", "fs_line")
            }),
            triangle_edges: pipeline(Pass {
                buffers: &[segments],
                ..Pass::overlay("varde triangle edges", "vs_triangle_edge", "fs_line")
            }),
            origin: pipeline(Pass::overlay("varde origin", "vs_origin", "fs_origin")),
            origin_planes: pipeline(Pass::overlay(
                "varde origin planes",
                "vs_origin_plane",
                "fs_origin_plane",
            )),
            sketch_on_top: SketchPipelines {
                fills: pipeline(Pass::on_top(
                    "varde sketch fills",
                    "vs_fill",
                    "fs_fill",
                    std::slice::from_ref(&sketch_fills),
                )),
                lines: pipeline(Pass::on_top(
                    "varde sketch being edited",
                    "vs_sketch_line",
                    "fs_line",
                    std::slice::from_ref(&sketch_lines),
                )),
                points: pipeline(Pass::on_top(
                    "varde sketch points",
                    "vs_point",
                    "fs_point",
                    std::slice::from_ref(&sketch_points),
                )),
            },
            sketch_depth_tested: SketchPipelines {
                fills: pipeline(Pass::depth_tested(
                    "varde sketch fills, depth tested",
                    "vs_fill",
                    "fs_fill",
                    std::slice::from_ref(&sketch_fills),
                )),
                lines: pipeline(Pass::depth_tested(
                    "varde sketch lines, depth tested",
                    "vs_sketch_line",
                    "fs_line",
                    std::slice::from_ref(&sketch_lines),
                )),
                points: pipeline(Pass::depth_tested(
                    "varde sketch points, depth tested",
                    "vs_point",
                    "fs_point",
                    std::slice::from_ref(&sketch_points),
                )),
            },
            sketch_over_glass: SketchPipelines {
                fills: pipeline(Pass {
                    stencil: over_glass.clone(),
                    ..Pass::depth_tested(
                        "varde sketch fills, over glass",
                        "vs_fill",
                        "fs_fill",
                        std::slice::from_ref(&sketch_fills),
                    )
                }),
                lines: pipeline(Pass {
                    stencil: over_glass.clone(),
                    ..Pass::depth_tested(
                        "varde sketch lines, over glass",
                        "vs_sketch_line",
                        "fs_line",
                        std::slice::from_ref(&sketch_lines),
                    )
                }),
                points: pipeline(Pass {
                    stencil: over_glass.clone(),
                    ..Pass::depth_tested(
                        "varde sketch points, over glass",
                        "vs_point",
                        "fs_point",
                        std::slice::from_ref(&sketch_points),
                    )
                }),
            },
            errors: ErrorPipelines {
                halo: [seen, hidden].map(|depth_compare| {
                    let halo = |label, vs, fs, buffers| {
                        pipeline(Pass {
                            label,
                            buffers,
                            depth_compare,
                            blend: MAX_BLENDING,
                            format: Some(HALO_FORMAT),
                            ..Pass::overlay(label, vs, fs)
                        })
                    };
                    ErrorLayer {
                        faces: halo(
                            "varde error halo, faces",
                            "vs_error_face",
                            "fs_halo_face",
                            &faces,
                        ),
                        lines: halo(
                            "varde error halo, lines",
                            "vs_error_halo_line",
                            "fs_halo_line",
                            &edge_points,
                        ),
                        points: halo(
                            "varde error halo, points",
                            "vs_error_halo_point",
                            "fs_halo_point",
                            std::slice::from_ref(&vertices),
                        ),
                    }
                }),
                composite: pipeline(Pass {
                    depth_compare: wgpu::CompareFunction::Always,
                    layout: Some(&errors_layout),
                    ..Pass::overlay(
                        "varde error halo composite",
                        "vs_fullscreen",
                        "fs_error_halo",
                    )
                }),
                core: [seen, hidden].map(|depth_compare| {
                    let core_pass = |label, vs, fs, buffers| Pass {
                        label,
                        buffers,
                        depth_compare,
                        layout: Some(&errors_layout),
                        ..Pass::overlay(label, vs, fs)
                    };
                    let core = |label, vs, fs, buffers| pipeline(core_pass(label, vs, fs, buffers));
                    // Hidden, patches overlapping would blend twice and
                    // darken: only the first drawn at a pixel is, marked
                    // in the stencil, cleared to 0, the reference.
                    let stencil = if depth_compare == hidden {
                        tagged(
                            wgpu::CompareFunction::Equal,
                            wgpu::StencilOperation::IncrementClamp,
                        )
                    } else {
                        NO_STENCIL
                    };
                    ErrorLayer {
                        faces: pipeline(Pass {
                            stencil,
                            ..core_pass(
                                "varde error faces",
                                "vs_error_face",
                                "fs_error_face",
                                &faces,
                            )
                        }),
                        lines: core(
                            "varde error lines",
                            "vs_error_line",
                            "fs_highlight_line",
                            &edge_points,
                        ),
                        points: core(
                            "varde error points",
                            "vs_error_point",
                            "fs_highlight_point",
                            std::slice::from_ref(&vertices),
                        ),
                    }
                }),
                layout: errors_group,
            },
            bind_group_layout,
            alphas,
            tint_stride,
            depth_format,
            format,
        }
    }

    /// The format of the targets it draws to, as it was made for.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// A slot's tints' buffer with room for `room` entries, and its group
    /// 0 binding it with `uniforms`.
    fn tints(
        &self,
        device: &wgpu::Device,
        uniforms: &wgpu::Buffer,
        room: u32,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let tints = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("varde tints"),
            size: u64::from(room) * u64::from(self.tint_stride),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("varde uniforms"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &tints,
                        offset: 0,
                        size: wgpu::BufferSize::new(TINT_SIZE),
                    }),
                },
            ],
        });
        (tints, bind_group)
    }

    /// Writes the colours `frame`'s parts are drawn in to `slot`'s tints,
    /// making room for them if they need it and the device has it; a
    /// part whose colour finds none is drawn in the model's.
    fn write_tints(
        &self,
        slot: &mut Slot,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame<'_>,
    ) {
        let parts = slot.mesh.as_ref().map_or(0, |mesh| mesh.parts.len());
        let model = frame.colors.model;
        let mut colors = vec![linear(model)];
        slot.part_tints.clear();
        let most = (device.limits().max_buffer_size / u64::from(self.tint_stride)).max(1);
        for i in 0..parts {
            let Some(tint) = frame.tints.get(i).copied().flatten() else {
                slot.part_tints.push(0);
                continue;
            };
            let color = linear(model.tinted(tint));
            let entry = match colors.iter().position(|&c| c == color) {
                Some(at) => at,
                None if (colors.len() as u64) < most => {
                    colors.push(color);
                    colors.len() - 1
                }
                None => 0,
            };
            // At most the parts' count plus one, well within u32.
            slot.part_tints.push(entry as u32);
        }
        // Within `most`, which fits a buffer.
        let needed = colors.len() as u32;
        if needed > slot.tint_room {
            let room = needed.next_power_of_two().min(most as u32).max(needed);
            (slot.tints, slot.bind_group) = self.tints(device, &slot.uniforms, room);
            slot.tint_room = room;
        }
        let stride = self.tint_stride as usize;
        let mut bytes = vec![0u8; colors.len() * stride];
        for (color, entry) in colors.iter().zip(bytes.chunks_exact_mut(stride)) {
            entry[..TINT_SIZE as usize].copy_from_slice(bytemuck::bytes_of(color));
        }
        queue.write_buffer(&slot.tints, 0, &bytes);
    }

    /// Binds `slot`'s group 0 with `part`'s colour, the model's if it's
    /// none of the mesh's parts.
    fn set_tint(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot, part: usize) {
        let entry = slot.part_tints.get(part).copied().unwrap_or(0);
        pass.set_bind_group(0, &slot.bind_group, &[entry * self.tint_stride]);
    }

    /// Creates a slot to draw a scene with, empty until it's prepared.
    pub fn slot(&self, device: &wgpu::Device) -> Slot {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("varde uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (tints, bind_group) = self.tints(device, &uniforms, 1);
        Slot {
            uniforms,
            bind_group,
            tints,
            tint_room: 1,
            part_tints: Vec::new(),
            mesh: None,
            source: Weak::new(),
            lines: None,
            lines_source: Weak::new(),
            sketch_base: SketchBuffers::default(),
            sketch_source: Weak::new(),
            sketch_live: SketchBuffers::default(),
            sketching: false,
            sketch_depth: false,
            depth: None,
            viewport: Viewport::default(),
            faded: false,
            hover_through: false,
            draws: PartDraws::default(),
            hidden_edges: false,
            origin: OriginShown::NONE,
            faces: Vec::new(),
            highlights: HighlightBuffers::default(),
            highlights_source: Weak::new(),
            errors: ErrorBuffers::default(),
            error_sources: Vec::new(),
            error_target: None,
        }
    }

    /// Uploads what `frame` needs to draw into `slot`.
    ///
    /// Fails with [`PrepareError::MeshTooLarge`] when the frame's mesh
    /// doesn't fit in the device's buffers, which can be smaller than the
    /// 256 MiB every mesh the kernel allows fits in. The frame is then
    /// drawn without it, and it isn't tried again until the slot's frame
    /// has another mesh, so this is only reported once per mesh. The same
    /// for the frame's sketches with [`PrepareError::LinesTooLarge`], and
    /// the layers of the sketch being edited with
    /// [`PrepareError::SketchTooLarge`], its live layer reported each frame
    /// it's too large, and the errors' geometry with
    /// [`PrepareError::ErrorsTooLarge`], tried again only once their
    /// sources change; should several fail at once, the mesh's error is
    /// the one returned, then the sketches'.
    pub fn prepare(
        &self,
        slot: &mut Slot,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame<'_>,
    ) -> Result<(), PrepareError> {
        slot.viewport = frame.viewport;
        slot.faded = frame.faded;
        slot.hover_through = frame.hover_through && !frame.faded;
        slot.hidden_edges = frame.hidden_edges && !frame.faded;
        // In a sketch, the grid's axis lines and the marker are the
        // sketch's.
        slot.origin = if frame.faded {
            OriginShown {
                marker: true,
                axes: [true, true, false],
                ..OriginShown::NONE
            }
        } else {
            frame.origin.with_hovered()
        };

        let mut result = Ok(());
        if !std::ptr::eq(slot.lines_source.as_ptr(), Arc::as_ptr(frame.sketches)) {
            slot.lines_source = Arc::downgrade(frame.sketches);
            slot.lines = upload_lines(device, frame.sketches).unwrap_or_else(|error| {
                result = Err(error);
                None
            });
        }
        // The regen lane hands an unchanged model back as the same `Arc`,
        // so this skips its upload. A failed upload isn't tried again for
        // the same `Arc`: it can only fail on the device's buffer limit,
        // which the same mesh would hit again.
        let new_mesh = !std::ptr::eq(slot.source.as_ptr(), Arc::as_ptr(frame.mesh));
        if new_mesh {
            slot.source = Arc::downgrade(frame.mesh);
            slot.mesh = upload_mesh(device, frame.mesh).unwrap_or_else(|error| {
                result = Err(error);
                None
            });
        }
        // Faded, every part is drawn alike.
        let opacity = if frame.faded { &[] } else { frame.opacity };
        if let Some(mesh) = &mut slot.mesh {
            mesh.wireframe = frame.wireframe;
            mesh.tessellation = frame.tessellation;
            if frame.tessellation && matches!(mesh.triangles, Triangles::Unbuilt) {
                // Like the mesh's, a failure isn't tried again.
                mesh.triangles = match triangle_edges(device, frame.mesh, &mesh.parts) {
                    Ok(triangles) => triangles,
                    Err(error) => {
                        result = result.and(Err(error));
                        Triangles::TooLarge
                    }
                };
            }
        }
        self.write_tints(slot, device, queue, frame);
        let parts = slot.mesh.as_ref().map_or(&[][..], |mesh| &mesh.parts);
        slot.draws = PartDraws::new(parts, opacity, &slot.part_tints, &self.alphas, frame.camera);

        // What's hovered and selected names the mesh's faces, edges and
        // vertices, so it's written again with the mesh too.
        if new_mesh
            || !std::ptr::eq(
                slot.highlights_source.as_ptr(),
                Arc::as_ptr(frame.highlights),
            )
        {
            slot.highlights_source = Arc::downgrade(frame.highlights);
            let written = slot
                .highlights
                .write(device, queue, frame.mesh, frame.highlights);
            result = result.and(written);
        }
        // The failures shown keep their `Arc`s while they're unchanged.
        let same_errors = slot.error_sources.len() == frame.errors.len()
            && (slot.error_sources.iter())
                .zip(frame.errors)
                .all(|(kept, error)| Weak::ptr_eq(kept, &error.source));
        if !same_errors {
            slot.error_sources = frame.errors.iter().map(|e| e.source.clone()).collect();
            let written = slot.errors.write(device, queue, frame.errors);
            result = result.and(written);
        }

        slot.faces.clear();
        if let Some(gpu) = &slot.mesh {
            let tinted = [
                (frame.hovered_faces, Tint::Hovered),
                (frame.selected_faces, Tint::Selected),
                (frame.second_faces, Tint::Second),
            ];
            let faces = (tinted.into_iter())
                .flat_map(|(faces, tint)| faces.iter().map(move |&face| (face, tint)));
            for (face, tint) in faces {
                if let Some((indices, part)) = gpu.face(frame.mesh, face) {
                    slot.faces.push(FaceDraw {
                        indices,
                        step: self.alphas.step(opacity.get(part).copied()),
                        tint,
                        part,
                        hovered: frame.hovered_faces.contains(&face)
                            && frame.selected_faces.contains(&face),
                    });
                }
            }
        }

        slot.sketching = frame.sketch.is_some();
        slot.sketch_depth = frame.sketch.is_some_and(|sketch| sketch.depth_tested);
        if let Some(sketch) = &frame.sketch {
            let mut sketch_result = Ok(());
            if !std::ptr::eq(slot.sketch_source.as_ptr(), Arc::as_ptr(sketch.base)) {
                slot.sketch_source = Arc::downgrade(sketch.base);
                if let Err(error) = slot.sketch_base.write(device, queue, sketch.base) {
                    sketch_result = Err(error);
                }
            }
            if let Err(error) = slot.sketch_live.write(device, queue, sketch.live) {
                sketch_result = sketch_result.and(Err(error));
            }
            result = result.and(sketch_result);
        }

        // A depth tested sketch is hidden by what's in front of it, and
        // so needs the depth range to cover it.
        let sketch_bounds = frame
            .sketch
            .filter(|sketch| sketch.depth_tested)
            .map(|sketch| {
                [
                    sketch.base.bounds(&sketch.plane),
                    sketch.live.bounds(&sketch.plane),
                ]
            });
        let bounds = [
            slot.mesh.as_ref().and_then(|m| m.bounds),
            slot.lines.as_ref().and_then(|l| l.bounds),
            slot.errors.bounds,
            planes_bounds(frame.camera, slot.origin),
        ]
        .into_iter()
        .chain(sketch_bounds.into_iter().flatten());
        let (camera, colors, grid) = (frame.camera, frame.colors, &frame.grid);
        let sketch_plane = frame.sketch.map_or(*grid, |sketch| sketch.plane);
        let aspect = frame.viewport.aspect();
        let half = camera.half_extents(aspect);
        // Out of range, the model is drawn opaque rather than not at all.
        let alpha = if frame.faded && (0.0..=1.0).contains(&colors.faded_alpha) {
            colors.faded_alpha
        } else {
            1.0
        };
        let with_alpha = |color, alpha| {
            let [r, g, b, _] = linear(color);
            [r, g, b, alpha]
        };
        let faded = |color| with_alpha(color, alpha);
        // Out of range, as faint as can be rather than solid.
        let selected_tint = if (0.0..=1.0).contains(&colors.selected_tint) {
            colors.selected_tint
        } else {
            0.6
        };
        let selected_edge_shade = if (-1.0..=1.0).contains(&colors.selected_edge_shade) {
            colors.selected_edge_shade
        } else {
            0.0
        };
        let contrast = if (0.0..=4.0).contains(&colors.contrast) {
            colors.contrast
        } else {
            1.0
        };
        let hidden_alpha = if (0.0..=1.0).contains(&colors.hidden_edge_alpha) {
            colors.hidden_edge_alpha
        } else {
            0.0
        };
        let second = linear(colors.second);
        let uniforms = Uniforms {
            view_proj: scene::view_projection(camera, aspect, grid, bounds.flatten())
                .to_cols_array_2d(),
            eye: camera.eye_homogeneous().to_array(),
            right: camera.right().extend(half.x).to_array(),
            up: camera.up().extend(half.y).to_array(),
            focus: camera.target().extend(camera.near()).to_array(),
            backward: camera.backward().extend(0.0).to_array(),
            viewport: [
                frame.viewport.width,
                frame.viewport.height,
                frame.scale_factor,
                0.0,
            ],
            viewport_origin: [
                frame.viewport.x,
                frame.viewport.y,
                frame.shading.code(),
                contrast,
            ],
            background_top: linear(colors.background_top),
            background_bottom: linear(colors.background_bottom),
            model: faded(colors.model),
            edge: with_alpha(colors.edge, hidden_alpha),
            grid: linear(colors.grid),
            axes: colors.axes.map(linear),
            origin_outline: linear(colors.origin_outline),
            sketch: linear(colors.sketch),
            pivot: frame
                .pivot
                .filter(|pivot| pivot.at.is_finite() && (0.0..=1.0).contains(&pivot.opacity))
                .map_or([0.0; 4], |pivot| pivot.at.extend(pivot.opacity).to_array()),
            pivot_color: linear(colors.pivot),
            grid_origin: (grid.origin())
                .extend(slot.origin.mask(grid) as f32)
                .to_array(),
            grid_x: grid.x().extend(axis_index(grid.x())).to_array(),
            grid_y: grid.y().extend(axis_index(grid.y())).to_array(),
            sketch_origin: sketch_plane.origin().extend(second[0]).to_array(),
            sketch_x: sketch_plane.x().extend(second[1]).to_array(),
            sketch_y: sketch_plane.y().extend(second[2]).to_array(),
            hover_face: with_alpha(colors.hover_face, selected_tint),
            hover_outline: linear(colors.hover_outline),
            selected: with_alpha(colors.selected, selected_edge_shade),
        };
        queue.write_buffer(&slot.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let size = frame.target_size.map(|s| s.max(1));
        if slot.depth.as_ref().map(|d| d.size) != Some(size) {
            slot.depth = Some(create_depth(device, self.depth_format, size));
        }
        if slot.errors.any() {
            if slot.error_target.as_ref().map(|t| t.size) != Some(size) {
                slot.error_target = Some(ErrorTarget::new(
                    device,
                    &self.errors.layout,
                    &slot.uniforms,
                    size,
                ));
            }
            if let Some(target) = &slot.error_target {
                let [r, g, b, a] = colors.error_halo.linear();
                // Out of range, as faint as the theme's.
                let a = if (0.0..=1.0).contains(&a) { a } else { 0.3 };
                let uniforms = ErrorUniforms {
                    core: linear(colors.error),
                    halo: [r, g, b, a],
                };
                queue.write_buffer(&target.uniforms, 0, bytemuck::bytes_of(&uniforms));
            }
        }
        result
    }

    /// Records draw commands for the frame last prepared into `slot` into
    /// `encoder`, compositing over `target`, within `clip`. The order of
    /// the draws, and why, is in `agents/viewport.md`.
    pub fn render(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: ClipRect,
    ) {
        self.record(slot, encoder, target, clip, true);
    }

    /// Records the frame last prepared into `slot` as [`Self::render`]
    /// does, with the background, the grid, the finished sketches and the
    /// origin and pivot markers only if `backdrop`: without, the model
    /// alone, over what `target` holds (a preview's, see `preview.rs`).
    pub(crate) fn record(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: ClipRect,
        backdrop: bool,
    ) {
        let Some(depth) = &slot.depth else { return };
        if clip.width == 0 || clip.height == 0 {
            return;
        }
        // The errors are drawn in passes of their own after the scene's,
        // depth tested against the model, so its depth is kept for them,
        // and the sketch being edited after them, so they don't hide it.
        let errors = slot.error_target.as_ref().filter(|_| slot.errors.any());
        let depth_store = if errors.is_some() {
            wgpu::StoreOp::Store
        } else {
            wgpu::StoreOp::Discard
        };
        let mut pass = self.begin(
            slot,
            encoder,
            ("varde scene", target, wgpu::LoadOp::Load),
            depth,
            (wgpu::LoadOp::Clear(1.0), depth_store),
            clip,
        );
        self.draw_scene(&mut pass, slot, backdrop);
        if let Some(errors) = errors {
            drop(pass);
            self.draw_errors(slot, encoder, target, depth, errors, clip);
        } else {
            self.draw_sketch(&mut pass, slot);
        }
    }

    /// Begins a pass of `slot`'s frame drawing into the colour target
    /// `target` (its label, view and load) and `depth`, its depth loaded
    /// and stored as `depth_ops` say (the stencil cleared, never kept),
    /// within `clip`, with the uniforms bound and parts drawn opaque.
    fn begin<'p>(
        &self,
        slot: &Slot,
        encoder: &'p mut wgpu::CommandEncoder,
        (label, target, load): (&str, &wgpu::TextureView, wgpu::LoadOp<wgpu::Color>),
        depth: &DepthTarget,
        (depth_load, depth_store): (wgpu::LoadOp<f32>, wgpu::StoreOp),
        clip: ClipRect,
    ) -> wgpu::RenderPass<'p> {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth.view,
                depth_ops: Some(wgpu::Operations {
                    load: depth_load,
                    store: depth_store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Discard,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let vp = slot.viewport;
        pass.set_viewport(vp.x, vp.y, vp.width, vp.height, 0.0, 1.0);
        pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
        pass.set_bind_group(0, &slot.bind_group, &[0]);
        self.alphas.set(&mut pass, self.alphas.opaque);
        pass
    }

    /// Records drawing `slot`'s errors over what's in `target`, with
    /// `depth` holding the model's depth: their halo's coverage into
    /// `error_target`'s, seen and hidden, then composited over the target
    /// once, then the errors themselves, a kind at a time, hidden then
    /// seen; and last the sketch being edited.
    fn draw_errors(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &DepthTarget,
        error_target: &ErrorTarget,
        clip: ClipRect,
    ) {
        let steps = [self.alphas.opaque, self.alphas.step(Some(ERROR_HIDDEN))];
        let errors = &slot.errors;
        let load = (wgpu::LoadOp::Load, wgpu::StoreOp::Store);
        let mut pass = self.begin(
            slot,
            encoder,
            (
                "varde error halo",
                &error_target.view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            ),
            depth,
            load,
            clip,
        );
        self.draw_error_layers(&mut pass, &self.errors.halo, steps, errors, None);
        drop(pass);

        let load = (wgpu::LoadOp::Load, wgpu::StoreOp::Discard);
        let target = ("varde errors", target, wgpu::LoadOp::Load);
        let mut pass = self.begin(slot, encoder, target, depth, load, clip);
        pass.set_pipeline(&self.errors.composite);
        pass.set_bind_group(0, &error_target.group, &[]);
        pass.draw(0..3, 0..1);
        // Hidden first, so what shows of each kind goes over it.
        let [seen, hidden] = &self.errors.core;
        let [opaque, dimmed] = steps;
        let layers = [hidden, seen];
        let cores = Some(errors.cores);
        self.draw_error_layers(&mut pass, layers, [dimmed, opaque], errors, cores);
        self.alphas.set(&mut pass, opaque);
        pass.set_bind_group(0, &slot.bind_group, &[0]);
        self.draw_sketch(&mut pass, slot);
    }

    /// Records drawing `errors` with `layers` at the alphas' steps `steps`,
    /// their faces with each, then their lines, then their points: all of
    /// them, or only `cores`, those of the parts drawn whole.
    fn draw_error_layers<'l>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        layers: impl IntoIterator<Item = &'l ErrorLayer> + Clone,
        steps: [u32; 2],
        errors: &ErrorBuffers,
        cores: Option<Cores>,
    ) {
        let with_steps = || layers.clone().into_iter().zip(steps);
        // Between the stream's two ends.
        let all = Cores {
            corners: errors.positions.count,
            edges: errors.edges.count.saturating_sub(1),
            points: errors.points.count,
        };
        let drawn = cores.unwrap_or(all);
        if let (Some(positions), Some(normals)) = (errors.positions.drawn(), errors.normals.drawn())
            && drawn.corners > 0
        {
            pass.set_vertex_buffer(0, positions);
            pass.set_vertex_buffer(1, normals);
            for (layer, step) in with_steps() {
                self.alphas.set(pass, step);
                pass.set_pipeline(&layer.faces);
                pass.draw(0..drawn.corners, 0..1);
            }
        }
        if let Some(edges) = errors.edges.held() {
            for (layer, step) in with_steps() {
                self.alphas.set(pass, step);
                draw_stream(pass, &layer.lines, edges, 1..drawn.edges);
            }
        }
        if let Some(points) = errors.points.drawn()
            && drawn.points > 0
        {
            pass.set_vertex_buffer(0, points);
            for (layer, step) in with_steps() {
                self.alphas.set(pass, step);
                pass.set_pipeline(&layer.points);
                pass.draw(0..POINT_VERTICES, 0..drawn.points);
            }
        }
    }

    /// Records drawing `slot`'s scene into `pass`: everything but the
    /// errors and the sketch being edited ([`Self::draw_sketch`]), the
    /// backdrop only if `backdrop` (see [`Self::record`]).
    fn draw_scene(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot, backdrop: bool) {
        if backdrop {
            pass.set_pipeline(&self.background);
            pass.draw(0..3, 0..1);
        }

        let mesh = slot.mesh.as_ref();
        let draws = &slot.draws;
        if let Some(mesh) = mesh {
            bind_faces(pass, mesh);
            if slot.faded {
                let all = 0..mesh.parts.len();
                pass.set_pipeline(&self.mesh_depth);
                draw_faces(pass, mesh, all.clone());
                // Faded, every part is opaque to `draws`, in runs of a
                // colour.
                pass.set_pipeline(&self.mesh_faded);
                for run in &draws.opaque {
                    self.set_tint(pass, slot, run.start);
                    draw_faces(pass, mesh, run.clone());
                }
                pass.set_bind_group(0, &slot.bind_group, &[0]);
                // What a sketch's tool picks of the model, over it.
                self.draw_picked_faces(pass, &slot.faces, false);
                draw_edges(pass, &self.edges, mesh, all.clone());
                self.draw_triangle_edges(pass, mesh, all);
            } else {
                pass.set_pipeline(&self.mesh);
                for run in &draws.opaque {
                    self.set_tint(pass, slot, run.start);
                    draw_faces(pass, mesh, run.clone());
                }
                pass.set_bind_group(0, &slot.bind_group, &[0]);
                // Before anything else writes depth where they are.
                self.draw_picked_faces(pass, &slot.faces, false);
                for run in &draws.opaque {
                    draw_edges(pass, &self.edges, mesh, run.clone());
                    self.draw_triangle_edges(pass, mesh, run.clone());
                }
            }
        }

        // Drawn after the model so they blend over the background and are
        // occluded by geometry. The sketches go over the grid, since they
        // often lie on it.
        if backdrop {
            pass.set_pipeline(&self.grid);
            pass.draw(0..3, 0..1);
        }

        // Under the finished sketches, which often lie on them.
        if backdrop {
            pass.set_pipeline(&self.origin_planes);
            // The hovered one's instance is 3 on, see `vs_origin_plane`.
            for (plane, _) in (0..).zip(slot.origin.planes).filter(|(_, shown)| *shown) {
                let hovered = slot.origin.emphasised(OriginPart::Plane(plane as usize));
                let instance = if hovered { plane + 3 } else { plane };
                pass.draw(0..PLANE_VERTICES, instance..instance + 1);
            }
        }

        if let Some(lines) = slot.lines.as_ref().filter(|_| backdrop) {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, lines.segments.slice(..));
            pass.draw(0..LINE_VERTICES, 0..lines.segment_count);
        }

        // Only the opaque parts have written depth so far, so the hidden
        // edges here are those they hide; what glass hides is dashed below.
        // Then the glass's visible edges, under the faces drawn next.
        if !slot.faded
            && let Some(mesh) = mesh
        {
            if slot.hidden_edges {
                for run in &draws.opaque {
                    draw_edges(pass, &self.hidden_edges, mesh, run.clone());
                }
                self.draw_glass_edges(pass, &self.hidden_edges, mesh, draws);
            }
            self.draw_glass_edges(pass, &self.edges, mesh, draws);
        }

        // The extrude's layers under the glass in front of them, if there
        // is any, and so under the origin marker too.
        let glass = mesh.is_some() && !draws.transparent.is_empty();
        let layers = [&slot.sketch_base, &slot.sketch_live];
        if slot.sketching && slot.sketch_depth && glass {
            for layer in layers {
                self.sketch_depth_tested.draw(pass, layer);
            }
        }

        // The glass: the hover and selection under it, to show through
        // dimmed; its faces far to near; its depth a part at a time, each
        // followed by the edges it hides, dashed; then its picked faces and
        // its edges again, undimmed on the glass they lie on.
        if let Some(mesh) = mesh.filter(|_| glass) {
            if !slot.faded {
                self.draw_highlights(pass, slot);
            }
            bind_faces(pass, mesh);
            pass.set_pipeline(&self.glass_back);
            for &(part, step) in &draws.transparent {
                self.alphas.set(pass, step);
                self.set_tint(pass, slot, part);
                draw_faces(pass, mesh, part..part + 1);
            }
            // Each part's front faces over their own depth, so only the
            // nearest are blended, not whichever come last.
            for &(part, step) in &draws.transparent {
                self.alphas.set(pass, step);
                self.set_tint(pass, slot, part);
                for pipeline in [&self.mesh_depth, &self.glass_front] {
                    pass.set_pipeline(pipeline);
                    draw_faces(pass, mesh, part..part + 1);
                }
            }
            for (i, &(part, step)) in draws.transparent.iter().enumerate() {
                // Never 0, what the stencil is cleared to, and under
                // `PICK_MARKS`; repeating only past 63 parts less than opaque.
                pass.set_stencil_reference(i as u32 % 63 + 1);
                bind_faces(pass, mesh);
                pass.set_pipeline(&self.glass_depth);
                draw_faces(pass, mesh, part..part + 1);
                if slot.hidden_edges {
                    self.draw_hidden_by_glass(pass, mesh, draws, step);
                }
            }
            // What's in front of the glass, drawn under it above, again
            // over it where it's the nearest.
            pass.set_stencil_reference(0);
            pass.set_bind_group(0, &slot.bind_group, &[0]);
            self.alphas.set(pass, self.alphas.opaque);
            if let Some(lines) = slot.lines.as_ref().filter(|_| backdrop) {
                pass.set_pipeline(&self.lines_over_glass);
                pass.set_vertex_buffer(0, lines.segments.slice(..));
                pass.draw(0..LINE_VERTICES, 0..lines.segment_count);
            }
            if slot.sketching && slot.sketch_depth {
                for layer in layers {
                    self.sketch_over_glass.draw(pass, layer);
                }
            }
            bind_faces(pass, mesh);
            self.draw_picked_faces(pass, &slot.faces, true);
            self.draw_glass_edges(pass, &self.edges, mesh, draws);
            for &(part, step) in &draws.transparent {
                self.alphas.set(pass, step);
                self.draw_triangle_edges(pass, mesh, part..part + 1);
            }
        }

        // The hover and the selection over everything of the model,
        // faded or not.
        if mesh.is_some() {
            self.draw_highlights(pass, slot);
        }
        if let Some(mesh) = mesh.filter(|_| slot.hover_through) {
            self.draw_hover_through(pass, slot, mesh);
        }
        if let Some(mesh) = mesh {
            self.draw_hidden_picks(pass, slot, mesh);
        }

        if backdrop {
            pass.set_pipeline(&self.origin);
            // The pivot's marker is the second instance.
            let first = if slot.origin.marker { 0 } else { 1 };
            pass.draw(0..ORIGIN_VERTICES, first..MARKERS);
        }
    }

    /// Records drawing the sketch being edited into `pass`, after the
    /// scene and the errors: over everything, the origin marker included,
    /// since its points often lie on it, and the errors, so they don't
    /// hide what's edited; or depth tested, unless [`Self::draw_scene`]
    /// drew it under the glass.
    fn draw_sketch(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot) {
        let glass = slot.mesh.is_some() && !slot.draws.transparent.is_empty();
        if slot.sketching && !(slot.sketch_depth && glass) {
            let pipelines = if slot.sketch_depth {
                &self.sketch_depth_tested
            } else {
                &self.sketch_on_top
            };
            for layer in [&slot.sketch_base, &slot.sketch_live] {
                pipelines.draw(pass, layer);
            }
        }
    }

    /// Records drawing `faces` again over themselves, hovered or selected,
    /// those of parts less than opaque if `transparent`, else the opaque
    /// ones', with the mesh's buffers bound ([`bind_faces`]).
    fn draw_picked_faces(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        faces: &[FaceDraw],
        transparent: bool,
    ) {
        for face in faces {
            if (face.step < self.alphas.opaque) != transparent || face.indices.is_empty() {
                continue;
            }
            let pipeline = match face.tint {
                Tint::Hovered => &self.hover_face,
                Tint::Selected => &self.selected_face,
                Tint::Second => &self.second_face,
            };
            pass.set_pipeline(pipeline);
            self.alphas.set(pass, face.step);
            pass.draw_indexed(face.indices.clone(), 0, 0..1);
        }
    }

    /// Records drawing the edges of the triangles of `mesh`'s `parts`,
    /// which follow one another, if [`Frame::tessellation`] asked for them.
    fn draw_triangle_edges(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        mesh: &GpuMesh,
        parts: Range<usize>,
    ) {
        let Triangles::Built {
            segments,
            parts: of,
        } = &mesh.triangles
        else {
            return;
        };
        let (Some(first), Some(last)) = (of.get(parts.start), of.get(parts.end.wrapping_sub(1)))
        else {
            return;
        };
        if !mesh.tessellation || first.start >= last.end {
            return;
        }
        pass.set_pipeline(&self.triangle_edges);
        pass.set_vertex_buffer(0, segments.slice(..));
        pass.draw(0..LINE_VERTICES, first.start..last.end);
    }

    /// Records drawing `slot`'s hovered edges within their outline,
    /// selected edges, and hovered and selected vertices.
    fn draw_highlights(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot) {
        let highlights = &slot.highlights;
        self.alphas.set(pass, self.alphas.opaque);
        if let Some(edges) = highlights.edges.held() {
            let outlined = highlights.outlined.clone();
            draw_stream(pass, &self.outline, edges, outlined.clone());
            draw_stream(pass, &self.hovered_edges, edges, outlined);
            let selected = highlights.selected.clone();
            draw_stream(pass, &self.selected_outline, edges, selected.clone());
            draw_stream(pass, &self.selected_edges, edges, selected);
            let second = highlights.second.clone();
            draw_stream(pass, &self.second_outline, edges, second.clone());
            draw_stream(pass, &self.second_edges, edges, second);
        }
        if let Some(vertices) = highlights.vertices.drawn() {
            pass.set_pipeline(&self.vertices);
            pass.set_vertex_buffer(0, vertices);
            pass.draw(0..POINT_VERTICES, 0..highlights.vertices.count);
        }
    }

    /// Records drawing what the model hides of `slot`'s hovered and
    /// selected faces and edges over everything: the faces washed and
    /// striped, the edges dashed; the selection over the hover. The hover
    /// not while [`Frame::hover_through`] draws it whole.
    fn draw_hidden_picks(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot, mesh: &GpuMesh) {
        let highlights = &slot.highlights;
        let hover = (!slot.hover_through).then_some((
            Tint::Hovered,
            PICK_MARKS[0],
            [&self.hovered_face_mark, &self.hovered_face_hidden],
            [&self.outline_hidden, &self.hovered_edges_hidden],
            &highlights.outlined,
        ));
        let selection = Some((
            Tint::Selected,
            PICK_MARKS[1],
            [&self.selected_face_mark, &self.selected_face_hidden],
            [&self.selected_outline_hidden, &self.selected_edges_hidden],
            &highlights.selected,
        ));
        for (tint, mark, [marking, hidden], edges, drawn) in hover.into_iter().chain(selection) {
            // Marking where the faces are the nearest, then striping
            // where they're hidden, outside that: what they hide of
            // themselves, as a selected body's far side, isn't striped.
            // The stripes are in the colour the face is shown in, so each
            // in its part's tint; a face both hovered and selected is
            // striped by the selection alone, from the hover's colour.
            bind_faces(pass, mesh);
            pass.set_stencil_reference(mark);
            let tinted = || (slot.faces.iter()).filter(|f| f.tint == tint && !f.indices.is_empty());
            pass.set_pipeline(marking);
            for face in tinted() {
                self.alphas.set(pass, face.step);
                pass.draw_indexed(face.indices.clone(), 0, 0..1);
            }
            for face in tinted() {
                let pipeline = match (tint, face.hovered) {
                    (Tint::Hovered, true) => continue,
                    (Tint::Selected, true) => &self.hovered_selected_face_hidden,
                    _ => hidden,
                };
                self.set_tint(pass, slot, face.part);
                self.alphas.set(pass, face.step);
                pass.set_pipeline(pipeline);
                pass.draw_indexed(face.indices.clone(), 0, 0..1);
            }
            pass.set_bind_group(0, &slot.bind_group, &[0]);
            pass.set_stencil_reference(0);
            self.alphas.set(pass, self.alphas.opaque);
            if let Some(stream) = highlights.edges.held() {
                for pipeline in edges {
                    draw_stream(pass, pipeline, stream, drawn.clone());
                }
            }
        }
    }

    /// Records drawing `slot`'s hovered faces, outlined edges and hovered
    /// vertices again over everything: [`Frame::hover_through`].
    fn draw_hover_through(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot, mesh: &GpuMesh) {
        bind_faces(pass, mesh);
        pass.set_pipeline(&self.hover_face_through);
        for face in &slot.faces {
            if face.tint == Tint::Hovered && !face.indices.is_empty() {
                self.alphas.set(pass, face.step);
                pass.draw_indexed(face.indices.clone(), 0, 0..1);
            }
        }
        let highlights = &slot.highlights;
        self.alphas.set(pass, self.alphas.opaque);
        if let Some(edges) = highlights.edges.held() {
            let outlined = highlights.outlined.clone();
            draw_stream(pass, &self.outline_through, edges, outlined.clone());
            draw_stream(pass, &self.hovered_edges_through, edges, outlined);
        }
        if let Some(vertices) = highlights.vertices.drawn() {
            pass.set_pipeline(&self.vertices_through);
            pass.set_vertex_buffer(0, vertices);
            pass.draw(0..POINT_VERTICES, 0..highlights.vertices.count);
        }
    }

    /// Records drawing the edges of `mesh`'s parts less than opaque, as
    /// `draws` has them, each at its alpha, with `pipeline`.
    fn draw_glass_edges(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipeline: &wgpu::RenderPipeline,
        mesh: &GpuMesh,
        draws: &PartDraws,
    ) {
        for &(part, step) in &draws.transparent {
            self.alphas.set(pass, step);
            draw_edges(pass, pipeline, mesh, part..part + 1);
        }
    }

    /// Records drawing, dashed, the edges of every part of `mesh` hidden by
    /// the glass whose depth and stencil reference were just written, at
    /// its alpha's step `step` times their own part's.
    fn draw_hidden_by_glass(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        mesh: &GpuMesh,
        draws: &PartDraws,
        step: u32,
    ) {
        let opaque = self.alphas.opaque;
        let runs = draws.opaque.iter().map(|run| (run.clone(), step));
        let glass = draws
            .transparent
            .iter()
            .map(|&(part, own)| (part..part + 1, product_step(step, own, opaque)));
        for (parts, step) in runs.chain(glass) {
            if step > 0 {
                self.alphas.set(pass, step);
                draw_edges(pass, &self.hidden_by_glass, mesh, parts);
            }
        }
    }
}

/// The step of a table of alphas whose last step, `opaque`, is opaque,
/// nearest the product of steps `a`'s and `b`'s alphas.
fn product_step(a: u32, b: u32, opaque: u32) -> u32 {
    if opaque == 0 {
        return 0;
    }
    let product = u64::from(a) * u64::from(b) + u64::from(opaque / 2);
    // At most `opaque` for steps within the table.
    u32::try_from(product / u64::from(opaque)).map_or(opaque, |step| step.min(opaque))
}

/// Binds `mesh`'s buffers for drawing its faces.
fn bind_faces(pass: &mut wgpu::RenderPass<'_>, mesh: &GpuMesh) {
    pass.set_vertex_buffer(0, mesh.positions.slice(..));
    pass.set_vertex_buffer(1, mesh.normals.slice(..));
    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
}

/// The range from `of` the first of `parts` to `of` the last, which
/// follow one another; empty if there are none.
fn span(mesh: &GpuMesh, parts: Range<usize>, of: fn(&GpuPart) -> &Range<u32>) -> Range<u32> {
    let parts = &mesh.parts[parts];
    match (parts.first(), parts.last()) {
        (Some(first), Some(last)) => of(first).start..of(last).end,
        _ => 0..0,
    }
}

/// Records drawing the faces of `mesh`'s `parts`, which follow one another,
/// with the pipeline set and its buffers bound ([`bind_faces`]).
fn draw_faces(pass: &mut wgpu::RenderPass<'_>, mesh: &GpuMesh, parts: Range<usize>) {
    let indices = span(mesh, parts, |part| &part.indices);
    if !indices.is_empty() {
        pass.draw_indexed(indices, 0, 0..1);
    }
}

/// Records drawing the feature edges of `mesh`'s `parts`, which follow one
/// another, with `pipeline`: their points follow one another in the
/// stream, so [`draw_stream`] draws them at once; and their wires likewise
/// in a wireframe.
fn draw_edges(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    mesh: &GpuMesh,
    parts: Range<usize>,
) {
    let Some(edges) = &mesh.edges else { return };
    let points = span(mesh, parts.clone(), |part| &part.points);
    draw_stream(pass, pipeline, edges, points);
    if mesh.wireframe {
        draw_stream(pass, pipeline, edges, span(mesh, parts, |part| &part.wires));
    }
}

/// Records drawing the polylines of the [`EdgePoint`] stream in `edges`
/// whose points are `points`, with `pipeline`, an instance per segment.
/// The points either side must be there, and of other polylines or of
/// none, so no segment joins them.
fn draw_stream(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    edges: &wgpu::Buffer,
    points: Range<u32>,
) {
    // A segment needs two points; and there's a point before them.
    if points.start == 0 || points.end.saturating_sub(points.start) < 2 {
        return;
    }
    pass.set_pipeline(pipeline);
    let stride = size_of::<EdgePoint>() as u64;
    let from = u64::from(points.start - 1);
    for slot in 0..EDGE_SLOTS {
        pass.set_vertex_buffer(slot, edges.slice((from + u64::from(slot)) * stride..));
    }
    pass.draw(0..LINE_VERTICES, 0..points.end - points.start - 1);
}

/// The pipelines drawing the errors' geometry ([`Frame::errors`]), each
/// kind where it shows (`[0]`, depth `LessEqual`) and where the model hides
/// it (`[1]`, `Greater`, drawn at [`ERROR_HIDDEN`]).
struct ErrorPipelines {
    /// The halo, as coverage into a [`HALO_FORMAT`] target of its own,
    /// blended by [`MAX_BLENDING`].
    halo: [ErrorLayer; 2],
    /// The halo's coverage, once over the frame in
    /// [`Colors::error_halo`].
    composite: wgpu::RenderPipeline,
    /// The geometry itself, over that.
    core: [ErrorLayer; 2],
    /// Group 0 of the composite and the core: [`ErrorTarget::group`].
    layout: wgpu::BindGroupLayout,
}

/// The pipelines drawing one way the errors' patches, curves and points.
struct ErrorLayer {
    faces: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    points: wgpu::RenderPipeline,
}

/// The pipelines drawing the sketch's layers, one way or the other: see
/// [`SketchScene::depth_tested`].
struct SketchPipelines {
    fills: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    points: wgpu::RenderPipeline,
}

impl SketchPipelines {
    /// Records drawing a layer of the sketch: its fills, then its lines,
    /// then its points.
    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, layer: &SketchBuffers) {
        if let Some(buffer) = layer.fills.drawn() {
            pass.set_pipeline(&self.fills);
            pass.set_vertex_buffer(0, buffer);
            pass.draw(0..layer.fills.count, 0..1);
        }
        if let Some(buffer) = layer.lines.drawn() {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, buffer);
            pass.draw(0..LINE_VERTICES, 0..layer.lines.count);
        }
        if let Some(buffer) = layer.points.drawn() {
            pass.set_pipeline(&self.points);
            pass.set_vertex_buffer(0, buffer);
            pass.draw(0..POINT_VERTICES, 0..layer.points.count);
        }
    }
}

/// Which world axis `axis`, a unit vector, lies along, 0 to 2, or 3 if
/// none, for its axis line's colour: of [`Colors::axes`], or the grid's.
/// The box the origin planes [`OriginShown`] has drawn take, seen with
/// `camera`, if it has any: see [`PLANE_REACH`].
fn planes_bounds(camera: &Camera, origin: OriginShown) -> Option<Aabb> {
    let reach = PLANE_REACH * camera.view_height();
    (origin.planes.contains(&true) && reach.is_finite()).then(|| Aabb {
        min: Vec3::splat(-reach),
        max: Vec3::splat(reach),
    })
}

fn axis_index(axis: Vec3) -> f32 {
    let along = axis.abs();
    (0..3)
        .find(|&i| along[i] > 1.0 - 1e-6)
        .map_or(3.0, |i| i as f32)
}

/// Converts a colour to linear, which the shader works in.
fn linear(Srgb(rgb): Srgb) -> [f32; 4] {
    let [r, g, b] = rgb.map(srgb_to_linear);
    [r, g, b, 1.0]
}

pub(crate) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Uploads `mesh`, or nothing if it is empty.
fn upload_mesh(device: &wgpu::Device, mesh: &RenderMesh) -> Result<Option<GpuMesh>, PrepareError> {
    if mesh.indices().is_empty() {
        return Ok(None);
    }
    // Meshes are bounded where they're built, but the device's limit can be
    // lower, and a buffer past it is a validation error, which wgpu panics
    // on.
    let limit = device.limits().max_buffer_size;
    // The kernel's limits keep every part within `MAX_BUFFER_BYTES`, so
    // this doesn't saturate.
    let bytes = |len: usize, size: usize| len.saturating_mul(size) as u64;
    let (edge_points, part_points) =
        if mesh.edge_vertices().is_empty() && mesh.wire_vertices().is_empty() {
            (None, Vec::new())
        } else {
            let (points, parts) = edge_stream(mesh);
            (Some(points), parts)
        };
    let largest = [
        bytes(mesh.positions().len(), size_of::<[f32; 3]>()),
        bytes(mesh.indices().len(), size_of::<u32>()),
        bytes(
            edge_points.as_ref().map_or(0, Vec::len),
            size_of::<EdgePoint>(),
        ),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    if largest > limit {
        return Err(PrepareError::MeshTooLarge {
            bytes: largest,
            limit,
        });
    }
    let edges = edge_points.map(|points| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh edges"),
            contents: bytemuck::cast_slice(&points),
            usage: wgpu::BufferUsages::VERTEX,
        })
    });
    // The kernel's limits keep these well within `u32`.
    let to_u32 = |range: Range<usize>| {
        let at = |n| u32::try_from(n).expect("kernel bounds indices");
        at(range.start)..at(range.end)
    };
    let part =
        |faces: Range<usize>, indices: Range<usize>, [points, wires]: [Range<u32>; 2]| GpuPart {
            bounds: bounds_of(mesh.positions(), &mesh.indices()[indices.clone()]),
            faces: to_u32(faces),
            indices: to_u32(indices),
            points,
            wires,
        };
    // With no edges or wires, none of the parts has points.
    let points = |i: usize| part_points.get(i).cloned().unwrap_or([1..1, 1..1]);
    let mut parts: Vec<GpuPart> = (mesh.parts().enumerate())
        .map(|(i, part_of)| part(part_of.faces, part_of.indices, points(i)))
        .collect();
    if parts.is_empty() {
        let (faces, indices) = (0..mesh.face_count(), 0..mesh.indices().len());
        parts.push(part(faces, indices, [1..1, 1..1]));
    }

    Ok(Some(GpuMesh {
        positions: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh positions"),
            contents: bytemuck::cast_slice(mesh.positions()),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        // As many as positions, so the same size.
        normals: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh normals"),
            contents: bytemuck::cast_slice(mesh.normals()),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh indices"),
            contents: bytemuck::cast_slice(mesh.indices()),
            usage: wgpu::BufferUsages::INDEX,
        }),
        edges,
        wireframe: false,
        triangles: Triangles::Unbuilt,
        tessellation: false,
        parts,
        bounds: mesh.bounds(),
    }))
}

/// The edges of `mesh`'s triangles, each once in its part of `parts`, as
/// segments uploaded to a buffer. Fails with [`PrepareError::MeshTooLarge`]
/// where they might not fit in one.
fn triangle_edges(
    device: &wgpu::Device,
    mesh: &RenderMesh,
    parts: &[GpuPart],
) -> Result<Triangles, PrepareError> {
    // A triangle's three edges, each shared with at most one other in its
    // part: at most as many as its indices.
    let bytes = (mesh.indices().len() as u64).saturating_mul(size_of::<Segment>() as u64);
    let limit = device.limits().max_buffer_size;
    if bytes > limit {
        return Err(PrepareError::MeshTooLarge { bytes, limit });
    }
    let positions = mesh.positions();
    let mut segments: Vec<Segment> = Vec::new();
    let mut ranges = Vec::with_capacity(parts.len());
    let mut edges: Vec<(u32, u32)> = Vec::new();
    for part in parts {
        let indices = &mesh.indices()[part.indices.start as usize..part.indices.end as usize];
        edges.clear();
        for &[a, b, c] in indices.as_chunks::<3>().0 {
            for (a, b) in [(a, b), (b, c), (c, a)] {
                edges.push((a.min(b), a.max(b)));
            }
        }
        edges.sort_unstable();
        edges.dedup();
        // The kernel bounds indices well within `u32`.
        let start = u32::try_from(segments.len()).expect("kernel bounds indices");
        segments
            .extend((edges.iter()).map(|&(a, b)| [positions[a as usize], positions[b as usize]]));
        let end = u32::try_from(segments.len()).expect("kernel bounds indices");
        ranges.push(start..end);
    }
    if segments.is_empty() {
        return Ok(Triangles::Empty);
    }
    Ok(Triangles::Built {
        segments: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde triangle edges"),
            contents: bytemuck::cast_slice(&segments),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        parts: ranges,
    })
}

/// The bounds of the positions `indices` refer to, or `None` if there are
/// none.
fn bounds_of(positions: &[[f32; 3]], indices: &[u32]) -> Option<Aabb> {
    let mut points = indices.iter().map(|&i| Vec3::from(positions[i as usize]));
    let first = points.next()?;
    let (min, max) = points.fold((first, first), |(min, max), p| (min.min(p), max.max(p)));
    Some(Aabb { min, max })
}

/// The [`EdgePoint`] stream of `mesh`'s feature edges, then its wires,
/// and where each part's edges' and wires' points are in it. Creases and
/// wires are marked [`CREASE`]; a wire is numbered after every edge.
fn edge_stream(mesh: &RenderMesh) -> (Vec<EdgePoint>, Vec<[Range<u32>; 2]>) {
    let points = (mesh.edge_vertices().len()).saturating_add(mesh.wire_vertices().len());
    let mut stream = EdgeStream::with_capacity(points);
    // The kernel bounds the edges and wires together well within `u32`,
    // below `CREASE`.
    let to_u32 = |n: usize| u32::try_from(n).expect("kernel bounds edges");
    let mut edge_points = Vec::with_capacity(mesh.part_ends().len());
    let mut polylines = mesh.polylines().zip(mesh.edge_faces());
    for part in mesh.parts() {
        let start = stream.len();
        for (edge, (polyline, [a, b])) in part.edges.zip(polylines.by_ref()) {
            let id = if a == b {
                to_u32(edge) | CREASE
            } else {
                to_u32(edge)
            };
            stream.push(id, mesh.positions(), polyline);
        }
        edge_points.push(start..stream.len());
    }
    let edges = to_u32(mesh.edge_count());
    let mut wires = mesh.wires();
    let parts = (mesh.parts().zip(edge_points))
        .map(|(part, edge_points)| {
            let start = stream.len();
            for (wire, polyline) in part.wires.zip(wires.by_ref()) {
                stream.push((edges + to_u32(wire)) | CREASE, mesh.positions(), polyline);
            }
            [edge_points, start..stream.len()]
        })
        .collect();
    (stream.finish(), parts)
}

/// An [`EdgePoint`] stream being built: polylines one after another, from
/// a point of no edge, which ends it too.
pub(crate) struct EdgeStream {
    points: Vec<EdgePoint>,
    /// A polyline's points, repeats in a row left out: a segment of no
    /// length between two others would keep them from joining.
    kept: Vec<[f32; 3]>,
}

impl EdgeStream {
    /// A stream with room for `points` points of polylines.
    pub(crate) fn with_capacity(points: usize) -> EdgeStream {
        let mut stream = EdgeStream {
            points: Vec::with_capacity(points.saturating_add(2)),
            kept: Vec::new(),
        };
        stream.separate();
        stream
    }

    /// Appends the polyline through `polyline`'s vertices of `positions`
    /// as edge `edge`, below [`NEIGHBOUR_ONLY`].
    pub(crate) fn push(&mut self, edge: u32, positions: &[[f32; 3]], polyline: &[u32]) {
        self.push_points(
            edge,
            polyline.iter().map(|&vertex| positions[vertex as usize]),
        );
    }

    /// Appends the polyline through `polyline` as edge `edge`, likewise.
    pub(crate) fn push_points(&mut self, edge: u32, polyline: impl IntoIterator<Item = [f32; 3]>) {
        let (points, kept) = (&mut self.points, &mut self.kept);
        kept.clear();
        for position in polyline {
            if kept.last() != Some(&position) {
                kept.push(position);
            }
        }
        let neighbour = |position| EdgePoint {
            position,
            along: 0.0,
            edge: edge | NEIGHBOUR_ONLY,
        };
        // Closed, round three segments or more; two would be one there and
        // back, joined at its turns already.
        let closed = kept.len() >= 4 && kept.first() == kept.last();
        if closed {
            points.push(neighbour(kept[kept.len() - 2]));
        }
        // Summed in `f64`, so a long polyline of short segments doesn't
        // drift. Positions are bounded, so it stays finite.
        let mut along = 0.0f64;
        for (i, &position) in kept.iter().enumerate() {
            if let Some(&last) = i.checked_sub(1).and_then(|i| kept.get(i)) {
                along += f64::from(Vec3::from(position).distance(Vec3::from(last)));
            }
            points.push(EdgePoint {
                position,
                along: along as f32,
                edge,
            });
        }
        if closed {
            points.push(neighbour(kept[1]));
        }
        // One left of the polyline, all its points the same, is a dot.
        if let [point] = kept[..] {
            points.push(EdgePoint {
                position: point,
                along: 0.0,
                edge,
            });
        }
    }

    /// Appends a point of no edge, which no segment joins.
    pub(crate) fn separate(&mut self) {
        self.points.push(EdgePoint {
            position: [0.0; 3],
            along: 0.0,
            edge: NO_EDGE,
        });
    }

    /// How many points it has.
    pub(crate) fn len(&self) -> u32 {
        // The kernel's limits keep the stream well within `u32`.
        u32::try_from(self.points.len()).expect("kernel bounds edges")
    }

    /// The stream, ended by a point of no edge.
    pub(crate) fn finish(mut self) -> Vec<EdgePoint> {
        self.separate();
        self.points
    }
}

/// Uploads `lines` a segment each, or nothing if there are none. See
/// [`upload_mesh`] for the limit.
fn upload_lines(
    device: &wgpu::Device,
    lines: &RenderLines,
) -> Result<Option<GpuLines>, PrepareError> {
    if lines.segment_count() == 0 {
        return Ok(None);
    }
    let limit = device.limits().max_buffer_size;
    // The kernel's limits keep this within `MAX_BUFFER_BYTES`.
    let bytes = lines.segment_count().saturating_mul(size_of::<Segment>()) as u64;
    if bytes > limit {
        return Err(PrepareError::LinesTooLarge { bytes, limit });
    }
    let segments: Vec<Segment> = lines
        .polylines()
        .flat_map(|polyline| polyline.windows(2).map(|pair| [pair[0], pair[1]]))
        .collect();
    let segment_count = u32::try_from(segments.len()).expect("kernel bounds points");

    Ok(Some(GpuLines {
        segments: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde sketch lines"),
            contents: bytemuck::cast_slice(&segments),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        segment_count,
        bounds: lines.bounds(),
    }))
}

/// A layer of the sketch being edited on the GPU: a buffer for each kind
/// of thing it draws.
#[derive(Default)]
struct SketchBuffers {
    lines: Instances,
    points: Instances,
    fills: Instances,
}

impl SketchBuffers {
    /// Replaces what the buffers hold with `layer`. On failing, holds
    /// nothing.
    fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &SketchLayer,
    ) -> Result<(), PrepareError> {
        let too_large = |bytes, limit| PrepareError::SketchTooLarge { bytes, limit };
        let (d, q) = (device, queue);
        let written = (self.lines)
            .write(d, q, "varde sketch lines", &layer.lines, too_large)
            .and_then(|()| {
                (self.points).write(d, q, "varde sketch points", &layer.points, too_large)
            })
            .and_then(|()| (self.fills).write(d, q, "varde sketch fills", &layer.fills, too_large));
        if written.is_err() {
            *self = SketchBuffers::default();
        }
        written
    }
}

/// A vertex buffer written again and again, kept while what's written fits
/// in it, and how many things it holds now.
#[derive(Default)]
struct Instances {
    buffer: Option<wgpu::Buffer>,
    count: u32,
}

impl Instances {
    /// The smallest buffer made, in bytes, so a layer growing a little at a
    /// time doesn't make a buffer each time.
    const MIN_BYTES: u64 = 4096;

    /// Replaces what it holds with `items`, in a larger buffer if they
    /// don't fit. Past the device's buffer size, holds nothing and fails
    /// with what `too_large` makes of the bytes needed and that size.
    fn write<T: Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        items: &[T],
        too_large: fn(u64, u64) -> PrepareError,
    ) -> Result<(), PrepareError> {
        let bytes: &[u8] = bytemuck::cast_slice(items);
        let size = bytes.len() as u64;
        let limit = device.limits().max_buffer_size;
        let count = u32::try_from(items.len()).ok().filter(|_| size <= limit);
        let Some(count) = count else {
            self.count = 0;
            return Err(too_large(size, limit));
        };
        self.count = count;
        if items.is_empty() {
            return Ok(());
        }
        if self.buffer.as_ref().is_none_or(|b| b.size() < size) {
            // Within the limit, which `size` is.
            let capacity = size
                .checked_next_power_of_two()
                .unwrap_or(size)
                .max(Self::MIN_BYTES)
                .min(limit)
                .max(size);
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = &self.buffer {
            queue.write_buffer(buffer, 0, bytes);
        }
        Ok(())
    }

    /// The part of the buffer to draw from, if it holds anything.
    fn drawn(&self) -> Option<wgpu::BufferSlice<'_>> {
        Some(self.held()?.slice(..))
    }

    /// The buffer, if it holds anything.
    fn held(&self) -> Option<&wgpu::Buffer> {
        self.buffer.as_ref().filter(|_| self.count > 0)
    }
}

/// [`Frame::highlights`] on the GPU: the edges as an [`EdgePoint`] stream,
/// written again as they change, which of its points are of the outlined
/// edges and which of the selected ones, and the vertices.
#[derive(Default)]
struct HighlightBuffers {
    edges: Instances,
    outlined: Range<u32>,
    selected: Range<u32>,
    second: Range<u32>,
    vertices: Instances,
}

impl HighlightBuffers {
    /// Replaces what the buffers hold with `highlights` in `mesh`. On
    /// failing, holds nothing.
    fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mesh: &RenderMesh,
        highlights: &Highlights,
    ) -> Result<(), PrepareError> {
        let built = highlights.build(mesh);
        let too_large = |bytes, limit| PrepareError::HighlightsTooLarge { bytes, limit };
        let (d, q) = (device, queue);
        let written = (self.edges)
            .write(d, q, "varde highlighted edges", &built.edges, too_large)
            .and_then(|()| {
                (self.vertices).write(d, q, "varde vertices", &built.vertices, too_large)
            });
        *self = match written {
            Ok(()) => HighlightBuffers {
                outlined: built.outlined,
                selected: built.selected,
                second: built.second,
                ..std::mem::take(self)
            },
            Err(_) => HighlightBuffers::default(),
        };
        written
    }
}

/// [`Frame::errors`] on the GPU, all of them together: the patches'
/// triangles a corner at a time (positions and normals), the curves as an
/// [`EdgePoint`] stream, a polyline each, and the points. Of each kind,
/// those of parts drawn whole come first, then those drawn only as their
/// halo ([`ErrorParts::halo_only`]).
#[derive(Default)]
struct ErrorBuffers {
    positions: Instances,
    normals: Instances,
    edges: Instances,
    points: Instances,
    /// How many of each kind are drawn whole, see [`Cores`].
    cores: Cores,
    /// To fit the depth range to.
    bounds: Option<Aabb>,
}

impl ErrorBuffers {
    /// Replaces what the buffers hold with `errors`. On failing, holds
    /// nothing.
    fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        errors: &[ErrorParts<'_>],
    ) -> Result<(), PrepareError> {
        let built = BuiltErrors::new(errors);
        let too_large = |bytes, limit| PrepareError::ErrorsTooLarge { bytes, limit };
        let (d, q) = (device, queue);
        let written = (self.positions)
            .write(d, q, "varde error faces", &built.positions, too_large)
            .and_then(|()| {
                (self.normals).write(d, q, "varde error normals", &built.normals, too_large)
            })
            .and_then(|()| (self.edges).write(d, q, "varde error lines", &built.edges, too_large))
            .and_then(|()| {
                (self.points).write(d, q, "varde error points", &built.points, too_large)
            });
        *self = match written {
            Ok(()) => ErrorBuffers {
                bounds: built.bounds,
                cores: built.cores,
                ..std::mem::take(self)
            },
            Err(_) => ErrorBuffers::default(),
        };
        written
    }

    /// Whether there's anything to draw.
    fn any(&self) -> bool {
        [&self.positions, &self.edges, &self.points]
            .iter()
            .any(|buffer| buffer.held().is_some())
    }
}

/// How many of the errors' corners, [`EdgePoint`]s and points are of
/// parts drawn whole, rather than only as their halo: the corners and
/// points `0..`, the stream's points `1..edges`.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Cores {
    corners: u32,
    edges: u32,
    points: u32,
}

/// [`Frame::errors`] as the renderer draws them, see [`ErrorBuffers`].
struct BuiltErrors {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    edges: Vec<EdgePoint>,
    points: Vec<VertexInstance>,
    cores: Cores,
    bounds: Option<Aabb>,
}

impl BuiltErrors {
    fn new(errors: &[ErrorParts<'_>]) -> BuiltErrors {
        let mut built = BuiltErrors {
            positions: Vec::new(),
            normals: Vec::new(),
            edges: Vec::new(),
            points: Vec::new(),
            cores: Cores::default(),
            bounds: None,
        };
        let mut stream = EdgeStream::with_capacity(0);
        // Each polyline a number of its own, so none joins the next; past
        // `CREASE` they start again, which only the next must differ from.
        let mut polyline = 0u32;
        // Those drawn whole first, then those drawn as their halo alone,
        // counting the first.
        let mut ordered: Vec<&ErrorParts<'_>> = errors.iter().collect();
        ordered.sort_by_key(|error| error.halo_only);
        let mut counted = false;
        for error in ordered {
            if error.halo_only && !counted {
                built.cores = built.counts(&stream);
                counted = true;
            }
            let mesh = error.mesh;
            let (positions, normals) = (mesh.positions(), mesh.normals());
            // A corner at a time: the kernel's meshes index their own
            // positions and normals, in whole triangles.
            for &i in mesh.indices() {
                built.positions.push(positions[i as usize]);
                built.normals.push(normals[i as usize]);
            }
            for line in error.lines.polylines() {
                stream.push_points(polyline, line.iter().copied());
                polyline = (polyline + 1) % CREASE;
            }
            let mut boxes: Vec<Aabb> = [mesh.bounds(), error.lines.bounds()]
                .into_iter()
                .flatten()
                .collect();
            let shown =
                |point: &&[f32; 3]| (point.iter()).all(|x| x.abs() <= RenderLines::MAX_POSITION);
            for &position in error.points.iter().filter(shown) {
                built.points.push(VertexInstance { position, flags: 0 });
                let at = Vec3::from(position);
                boxes.push(Aabb { min: at, max: at });
            }
            built.bounds = (boxes.into_iter().chain(built.bounds)).reduce(|a, b| Aabb {
                min: a.min.min(b.min),
                max: a.max.max(b.max),
            });
        }
        if !counted {
            built.cores = built.counts(&stream);
        }
        // Without a curve, nothing rather than the stream's two ends, so
        // that errors of none don't count as some to draw.
        if stream.len() > 1 {
            built.edges = stream.finish();
        } else {
            built.cores.edges = 0;
        }
        built
    }

    /// How many corners, stream points and points are built so far.
    fn counts(&self, stream: &EdgeStream) -> Cores {
        // Past `u32`, writing them fails, and nothing is drawn.
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Cores {
            corners: count(self.positions.len()),
            edges: stream.len(),
            points: count(self.points.len()),
        }
    }
}

/// What the errors are drawn with beside the scene's own: the target
/// their halo is drawn into (see [`HALO_FORMAT`]) and their colours.
struct ErrorTarget {
    view: wgpu::TextureView,
    /// [`ErrorUniforms`], written each frame there are errors.
    uniforms: wgpu::Buffer,
    /// Group 0 of the composite and the core: the scene's uniforms, the
    /// colours and the halo.
    group: wgpu::BindGroup,
    size: [u32; 2],
}

impl ErrorTarget {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        scene: &wgpu::Buffer,
        size: [u32; 2],
    ) -> ErrorTarget {
        let [width, height] = size;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("varde error halo"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HALO_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("varde error colours"),
            size: size_of::<ErrorUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("varde errors"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: scene.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        ErrorTarget {
            view,
            uniforms,
            group,
            size,
        }
    }
}

fn create_depth(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    [width, height]: [u32; 2],
) -> DepthTarget {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("varde depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });

    DepthTarget {
        view: texture.create_view(&Default::default()),
        size: [width, height],
    }
}

#[cfg(test)]
mod tests;
