//! What a frame shows: the origin, the viewport and clip, shading, the
//! [`Frame`] description and the public line widths and alphas.

use super::*;

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
    /// Whether the planes are drawn over the model rather than hidden by
    /// it: while one is picked, so each can be seen and picked.
    pub planes_on_top: bool,
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
        planes_on_top: false,
    };
    /// What a sketch shows of the origin, whatever the frame's: the
    /// marker and its plane's X and Y axes, never hovered.
    pub(super) const SKETCH: OriginShown = OriginShown::DEFAULT;
    /// None of them.
    pub const NONE: OriginShown = OriginShown {
        marker: false,
        axes: [false; 3],
        planes: [false; 3],
        hovered: None,
        selected: [false; OriginPart::COUNT],
        planes_on_top: false,
    };

    /// Whether `part` is drawn emphasised: hovered or selected.
    pub(super) fn emphasised(&self, part: OriginPart) -> bool {
        self.hovered == Some(part) || self.selected.get(part.index()).copied().unwrap_or(false)
    }

    /// These with the ones hovered and selected shown too.
    pub(super) fn with_hovered(mut self) -> Self {
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
    pub(super) fn mask(self, grid: &GridPlane) -> u32 {
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
pub(super) const GRID_X: u32 = 1;
pub(super) const GRID_Y: u32 = 1 << 1;
pub(super) const WORLD_Z: u32 = 1 << 2;
pub(super) const ORIGIN_MARKER: u32 = 1 << 3;
/// How far a line's bit is shifted to say it's hovered, as in the shader.
pub(super) const HOVERED_SHIFT: u32 = 4;
/// Draws the grid's axis lines alone, not its lines: the Z axis fading
/// over a sketch's grid on the XY plane.
pub(super) const AXES_ONLY: u32 = 1 << 7;

/// A quad per origin plane, an instance each: see `vs_origin_plane`.
pub(super) const PLANE_VERTICES: u32 = 6;

/// A quad per marker, an instance each: the origin's and the pivot's. See
/// `vs_origin`.
pub(super) const ORIGIN_VERTICES: u32 = 6;
pub(super) const MARKERS: u32 = 2;

/// A quad per segment of a line, two triangles. See `line_vertex`.
pub(super) const LINE_VERTICES: u32 = 6;

/// A quad per point of the sketch being edited. See `vs_point`.
pub(super) const POINT_VERTICES: u32 = 6;

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
    pub(super) fn code(self) -> f32 {
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
    /// within half an 8 bit step of 1; one of exactly 0 isn't drawn at all
    /// (a preview of some parts, see [`crate::frame`]). Ignored
    /// while faded ([`Self::fade`] at 1).
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
    /// How faded the model is drawn, from 0 (not at all) to 1 (as it is
    /// behind a sketch being edited, [`Colors::faded_alpha`]), between
    /// the two as it fades going into a sketch or out of one: each part
    /// then goes from its own [`Self::opacity`] towards the faded model's,
    /// and the edges the model hides fade out. NaN is 0.
    pub fade: f32,
    /// While [`Self::fade`] moves, the plane of the sketch the grid fades
    /// onto or off, from its first frame to its last: it's drawn on that plane at the fade's share and
    /// on the world's XY plane at the rest, each with its own axis lines.
    /// The XY plane itself keeps the grid, the Z axis fading over it.
    /// `None` draws it on [`Self::grid`] alone.
    pub fading_grid: Option<GridPlane>,
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
    /// [`Colors::hidden_edge_alpha`]. Never while faded ([`Self::fade`] at 1).
    pub hidden_edges: bool,
    /// The faces hovered, by their ids in the mesh (their runs of
    /// [`RenderMesh::face_ends`]): the one the cursor is over, or all of
    /// what it would select (a body's). Each is drawn again over itself
    /// towards [`Colors::hover_face`], as opaque as its part (opaque
    /// over the model at [`Self::fade`] 1); and where the model hides
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
    /// (faded, [`Self::fade`] at 1): what a sketch's tool picks of it.
    pub highlights: &'a Arc<Highlights>,
    /// Whether the hovered faces, outlined edges and hovered vertices are
    /// drawn again over everything, what hides them included: a face at
    /// [`HOVER_THROUGH_ALPHA`] of its hover, the edges and vertices as
    /// they are. For an item hovered in a list of what overlaps, often
    /// hidden. Not while faded ([`Self::fade`] at 1).
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
    /// while faded ([`Self::fade`] at 1), in a sketch, whose grid's axis lines and
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

impl<'a> Frame<'a> {
    /// A frame of `mesh` and `sketches` from `camera` filling a target of
    /// `target_size`, with nothing else: no fade, overlays, faces hovered
    /// or selected, failures, sketch being edited, pivot or origin, the
    /// grid on the world's XY plane, the mesh's parts opaque and untinted,
    /// lit as [`Shading::Regular`]. Other fields are set on the result.
    pub fn plain(
        camera: &'a Camera,
        mesh: &'a Arc<RenderMesh>,
        sketches: &'a Arc<RenderLines>,
        highlights: &'a Arc<Highlights>,
        target_size: [u32; 2],
        scale_factor: f32,
        colors: Colors,
    ) -> Self {
        Frame {
            camera,
            mesh,
            opacity: &[],
            tints: &[],
            sketches,
            grid: GridPlane::XY,
            fade: 0.0,
            fading_grid: None,
            wireframe: false,
            tessellation: false,
            shading: Shading::Regular,
            hidden_edges: false,
            hovered_faces: &[],
            selected_faces: &[],
            second_faces: &[],
            highlights,
            hover_through: false,
            sketch: None,
            errors: &[],
            pivot: None,
            origin: OriginShown::NONE,
            viewport: Viewport {
                x: 0.0,
                y: 0.0,
                width: target_size[0] as f32,
                height: target_size[1] as f32,
            },
            target_size,
            scale_factor,
            colors,
        }
    }
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
