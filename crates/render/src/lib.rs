//! wgpu renderer for CAD models.
//!
//! This crate knows nothing about the UI toolkit. It records into a
//! caller-provided [`wgpu::CommandEncoder`] and target view, so it can be
//! hosted inside an iced shader widget, an offscreen texture for thumbnails,
//! or a headless test.

// The preview's read back callback holds a `wgpu::Buffer`, and asking
// whether it's `Send` goes deeper than the default limit.
#![recursion_limit = "256"]

mod camera;
mod highlight;
mod preview;
mod renderer;
mod scene;
mod sketch;

pub use camera::{Camera, Projection, View};
pub use highlight::{Highlights, Vertex};
pub use preview::{PreviewError, PreviewImage, PreviewShot, frame, render_preview};
pub use renderer::{
    BodyTint, CREASE_ALPHA, CREASE_WIDTH, ClipRect, Colors, EDGE_WIDTH, ERROR_EDGE_WIDTH,
    ERROR_HALO, ERROR_POINT_RADIUS, ErrorParts, Frame, HIDDEN_DASH, HIDDEN_EDGE_WIDTH, HOVER_RIM,
    HOVER_THROUGH_ALPHA, HOVERED_EDGE_WIDTH, LINE_WIDTH, OriginPart, OriginShown, PLANE_GAP,
    PLANE_REACH, PLANE_SIDES, PatternStyle, Pivot, PrepareError, Renderer, SELECTED_EDGE_WIDTH,
    SELECTED_RIM, Shading, Slot, Srgb, VERTEX_RADIUS, Viewport,
};
pub use scene::{GRID_FADE_HEIGHTS, GridPlane};
pub use sketch::{LineStyle, PointStyle, SketchLayer, SketchScene, Space, Srgba};

pub use wgpu;
