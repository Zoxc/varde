//! wgpu renderer for CAD models.
//!
//! This crate knows nothing about the UI toolkit. It records into a
//! caller-provided [`wgpu::CommandEncoder`] and target view, so it can be
//! hosted inside an iced shader widget, an offscreen texture for thumbnails,
//! or a headless test.

mod camera;
mod highlight;
mod renderer;
mod scene;
mod sketch;

pub use camera::{Camera, Projection, View};
pub use highlight::{Highlights, Vertex};
pub use renderer::{
    ClipRect, Colors, EDGE_WIDTH, Frame, HIDDEN_DASH, HIDDEN_EDGE_WIDTH, HOVER_RIM, LINE_WIDTH,
    Pivot, PrepareError, Renderer, SELECTED_EDGE_WIDTH, Slot, Srgb, VERTEX_RADIUS, Viewport,
};
pub use scene::{GRID_FADE_HEIGHTS, GridPlane};
pub use sketch::{LineStyle, PointStyle, SketchLayer, SketchScene, Space, Srgba};

pub use wgpu;
