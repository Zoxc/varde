//! How the renderer draws: line widths, rims, halos and alphas, in
//! logical pixels or of full strength, and the sizes of the origin planes
//! and the grid's fade.

use glam::Vec3;

/// How many view heights from the target the grid fades out within. The
/// renderer passes it to `fs_grid` as a pipeline constant.
pub const GRID_FADE_HEIGHTS: f32 = 6.0;

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

/// How wide [`Frame::sketches`](crate::Frame::sketches) are drawn, in logical pixels.
pub const LINE_WIDTH: f32 = 1.5;

/// How wide the model's feature edges are drawn, in logical pixels.
pub const EDGE_WIDTH: f32 = 1.5;

/// How wide creases are drawn, in logical pixels: feature edges inside one
/// face (see [`RenderMesh::edge_faces`](varde_kernel_compat::RenderMesh::edge_faces)), where its patches' normals part.
pub const CREASE_WIDTH: f32 = 1.0;

/// How opaque creases are drawn, from 0 to 1, of what the feature edges
/// are, seen and hidden alike: they're how a face was cut into patches,
/// not where it ends.
pub const CREASE_ALPHA: f32 = 0.35;

/// How wide the edges the model hides are drawn, in logical pixels: see
/// [`Frame::hidden_edges`](crate::Frame::hidden_edges).
pub const HIDDEN_EDGE_WIDTH: f32 = 1.0;

/// The dashes of the edges the model hides: how long a dash and a gap are
/// along the edge, in logical pixels at the target.
pub const HIDDEN_DASH: [f32; 2] = [4.0, 3.0];

/// How wide the hovered edges are drawn, in logical pixels, within their
/// rim: see [`Highlights::outlined`](crate::Highlights::outlined).
pub const HOVERED_EDGE_WIDTH: f32 = 2.5;

/// How wide the rim of [`Colors::hover_outline`](crate::Colors::hover_outline) is around the hovered
/// edges and a hovered vertex, in logical pixels: see
/// [`Highlights::outlined`](crate::Highlights::outlined).
pub const HOVER_RIM: f32 = 1.5;

/// How opaque a hovered face drawn over what hides it is, of its hover:
/// see [`Frame::hover_through`](crate::Frame::hover_through).
pub const HOVER_THROUGH_ALPHA: f32 = 0.6;

/// How wide the faint rim around the selected edges and vertices is, in
/// logical pixels, for contrast with what's behind them: in
/// [`Colors::hover_outline`](crate::Colors::hover_outline), translucent.
pub const SELECTED_RIM: f32 = 1.0;

/// How wide the selected edges are drawn, in logical pixels, over their
/// [`EDGE_WIDTH`]: see [`Highlights::selected_edges`](crate::Highlights::selected_edges).
pub const SELECTED_EDGE_WIDTH: f32 = 2.5;

/// The radius of a hovered or selected vertex's disc, in logical pixels,
/// within its rim: see [`Highlights::vertices`](crate::Highlights::vertices).
pub const VERTEX_RADIUS: f32 = 3.5;

/// How wide error geometry's curves are drawn, in logical pixels: see
/// [`Frame::errors`](crate::Frame::errors).
pub const ERROR_EDGE_WIDTH: f32 = 3.0;

/// How far the halo around error geometry reaches beyond it on every side,
/// in logical pixels: see [`Frame::errors`](crate::Frame::errors).
pub const ERROR_HALO: f32 = 12.0;

/// The radius of an error point's disc, in logical pixels: see
/// [`Frame::errors`](crate::Frame::errors).
pub const ERROR_POINT_RADIUS: f32 = 4.0;

/// How strongly error geometry is drawn where the model hides it, of its
/// full strength, core and halo alike.
pub(crate) const ERROR_HIDDEN: f32 = 0.4;
