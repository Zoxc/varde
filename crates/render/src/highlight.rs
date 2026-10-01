//! What's hovered and selected in the model, drawn over it: faces'
//! triangles shaded as the model is but in the hover's or the
//! selection's hue, and edges as wide lines, both hidden by what's in
//! front of them and pulled towards the camera like the feature edges, so
//! the face or edge they cover doesn't hide them.

use bytemuck::{Pod, Zeroable};
use glam::{DVec2, Vec3};

use crate::renderer::{Colors, srgb_to_linear};
use crate::sketch::{LineStyle, SketchLayer};

/// How wide a highlighted edge is drawn, in logical pixels.
pub const HIGHLIGHT_WIDTH: f32 = 3.0;

/// Which of the highlight's colours something is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Emphasis {
    /// Under the cursor.
    Hovered,
    Selected,
}

/// Faces and edges of the model to draw over it, each in its
/// [`Emphasis`]'s colour from [`Colors`]. The view builds it from the
/// model's mesh: its triangles are the mesh's, so they lie on what they
/// cover.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Highlight {
    pub(crate) triangles: Vec<Triangle>,
    pub(crate) edges: Vec<(Emphasis, Vec<Vec3>)>,
}

/// A triangle of a highlighted face: its corners and their normals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Triangle {
    pub(crate) emphasis: Emphasis,
    pub(crate) corners: [Vec3; 3],
    pub(crate) normals: [Vec3; 3],
}

/// A corner of a highlighted face's triangle as the GPU takes it:
/// `vs_highlight` in the scene shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct HighlightVertex {
    pub(crate) position: [f32; 3],
    pub(crate) normal: [f32; 3],
    /// Linear, straight alpha.
    pub(crate) color: [f32; 4],
}

impl Highlight {
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty() && self.edges.is_empty()
    }

    /// Adds the triangle `corners`, whose normals there are `normals`,
    /// unless a corner or normal isn't finite.
    pub fn triangle(&mut self, emphasis: Emphasis, corners: [Vec3; 3], normals: [Vec3; 3]) {
        if corners.iter().chain(&normals).all(|v| v.is_finite()) {
            self.triangles.push(Triangle {
                emphasis,
                corners,
                normals,
            });
        }
    }

    /// Adds the polyline through `points`, an edge of the model, drawn
    /// [`HIGHLIGHT_WIDTH`] wide. A polyline through three points or more
    /// ending where it starts is closed. Nothing is added for one with a
    /// point that isn't finite, or of fewer than two distinct points.
    pub fn edge(&mut self, emphasis: Emphasis, points: Vec<Vec3>) {
        let mut points = points;
        points.dedup();
        if points.len() >= 2 && points.iter().all(|p| p.is_finite()) {
            self.edges.push((emphasis, points));
        }
    }

    /// Its faces' corners as the GPU takes them, in `colors`.
    pub(crate) fn vertices(&self, colors: &Colors) -> Vec<HighlightVertex> {
        let color = |emphasis| {
            let [r, g, b] = match emphasis {
                Emphasis::Hovered => colors.hovered_face,
                Emphasis::Selected => colors.selected_face,
            }
            .0
            .map(srgb_to_linear);
            [r, g, b, 1.0]
        };
        self.triangles
            .iter()
            .flat_map(|triangle| {
                let color = color(triangle.emphasis);
                (0..3).map(move |i| HighlightVertex {
                    position: triangle.corners[i].to_array(),
                    normal: triangle.normals[i].to_array(),
                    color,
                })
            })
            .collect()
    }

    /// Its edges as lines of a layer, in `colors`.
    pub(crate) fn lines(&self, colors: &Colors) -> SketchLayer {
        let mut layer = SketchLayer::default();
        for (emphasis, points) in &self.edges {
            let color = match emphasis {
                Emphasis::Hovered => colors.hovered_edge,
                Emphasis::Selected => colors.selected_edge,
            };
            let style = LineStyle {
                color,
                width: HIGHLIGHT_WIDTH,
                dash: None,
            };
            layer.world_polyline(points, style);
        }
        layer
    }
}

/// `point`'s x and y, and its z apart, as a layer's world lines take them.
pub(crate) fn split(point: Vec3) -> (DVec2, f32) {
    (point.truncate().as_dvec2(), point.z)
}
