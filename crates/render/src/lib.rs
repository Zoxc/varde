//! wgpu renderer for CAD models.
//!
//! This crate knows nothing about the UI toolkit. It records into a
//! caller-provided [`wgpu::CommandEncoder`] and target view, so it can be
//! hosted inside an iced shader widget, an offscreen texture for thumbnails,
//! or a headless test.

mod camera;
mod renderer;
mod scene;

pub use camera::{Camera, Projection, View};
pub use renderer::{ClipRect, Colors, Frame, PrepareError, Renderer, Slot, Srgb, Viewport};

pub use wgpu;
