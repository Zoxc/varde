//! The [`Renderer`]: what every scene shares. Its parts are split by concern:
//! the frame description ([`frame`]), colours ([`color`]), pipeline building
//! ([`pipelines`]), GPU uploads ([`upload`]), render targets ([`targets`]),
//! the per-slot state and its preparation ([`prepare`]) and recording
//! ([`draw`]).

use std::any::Any;
use std::ops::Range;
use std::sync::{Arc, Mutex, Weak};

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use varde_kernel_compat::{
    Aabb, CREASE, EdgePoint, EdgeStream, MeshUpload, RenderLines, RenderMesh,
};
use wgpu::util::DeviceExt;

use crate::Camera;
use crate::highlight::{Highlights, VertexInstance};
use crate::scene::{self, GRID_FADE_HEIGHTS, GridPlane};
use crate::sketch::{FillVertex, LineInstance, PointInstance, SketchLayer, SketchScene, Srgba};

mod color;
mod draw;
mod frame;
mod pipelines;
mod prepare;
mod targets;
mod upload;

pub use color::*;
use draw::*;
pub use frame::*;
use pipelines::*;
pub use prepare::*;
use targets::*;
use upload::*;

/// Renders a scene into a caller-provided color target.
///
/// Holds only what every scene shares; what one scene draws is in a
/// [`Slot`]. Call [`Renderer::prepare`] to upload a frame into a slot, then
/// [`Renderer::render`] to record its draw commands.
pub struct Renderer {
    /// The samples a pixel of the scene takes, see [`sample_count`].
    samples: u32,
    /// The scene, resolved, over the frame's target, premultiplied.
    composite: wgpu::RenderPipeline,
    composite_layout: wgpu::BindGroupLayout,
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
    /// The marks again into the pattern's mask, where the faces show, so
    /// its edge isn't drawn where the face goes on but shows.
    hovered_face_shown: wgpu::RenderPipeline,
    selected_face_shown: wgpu::RenderPipeline,
    selected_edges_hidden: wgpu::RenderPipeline,
    hovered_face_hidden: wgpu::RenderPipeline,
    hovered_edges_hidden: wgpu::RenderPipeline,
    /// The rims of the hidden hovered and selected edges, dashed.
    outline_hidden: wgpu::RenderPipeline,
    selected_outline_hidden: wgpu::RenderPipeline,
    /// The hovered and selected faces' pattern as a mask into
    /// [`Slot::pattern_target`], and its edge from that over the frame.
    selected_face_pattern: wgpu::RenderPipeline,
    hovered_selected_face_pattern: wgpu::RenderPipeline,
    hovered_face_pattern: wgpu::RenderPipeline,
    pattern_edge: wgpu::RenderPipeline,
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
    /// The origin planes drawn over the model: [`OriginShown::planes_on_top`].
    origin_planes_on_top: wgpu::RenderPipeline,
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

impl Renderer {
    /// The format of the targets it draws to, as it was made for.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }
}

#[cfg(test)]
mod tests;
