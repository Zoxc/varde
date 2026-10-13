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
pub mod style;

pub use camera::{Camera, Projection, Rotation, View};
pub use highlight::{Highlights, Vertex};
pub use preview::{
    PreviewError, PreviewImage, PreviewScene, PreviewSelection, PreviewShot, frame, render_preview,
};
pub use renderer::{
    BodyTint, ClipRect, Colors, ErrorParts, Frame, OriginPart, OriginShown, PatternStyle, Pivot,
    PrepareError, Renderer, Shading, Slot, Srgb, Srgba, Upload, Viewport,
};
pub use scene::GridPlane;
pub use sketch::{LineStyle, PointStyle, SketchLayer, SketchScene, Space};

pub use wgpu;
