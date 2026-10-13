//! The [`ModelHighlight`] of what's hovered and selected, by the mesh's
//! faces, edges and corners.

use std::sync::Arc;

use varde_render::{Highlights, Vertex};

use super::{PickIndex, Picked};

/// What the viewport draws of what's hovered and selected over the model
/// it was built for, by the mesh's ids (see
/// [`varde_render::Frame::hovered_faces`]): the faces hovered, drawn
/// brighter, and selected, tinted; the hovered edges outlined, the
/// selected edges, and the hovered and selected vertices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelHighlight {
    pub hovered_faces: Vec<u32>,
    pub selected_faces: Vec<u32>,
    /// The faces in the second colour: the measure tool's B, where A is
    /// drawn as selected.
    pub second_faces: Vec<u32>,
    pub highlights: Arc<Highlights>,
    /// Whether the second colour is the exclusions' purple, for the
    /// edges a blend leaves out, rather than the measure tool's B.
    pub second_excluded: bool,
}

impl ModelHighlight {
    /// Whether it draws nothing.
    pub fn is_empty(&self) -> bool {
        let Highlights {
            outlined,
            selected_edges,
            second_edges,
            vertices,
        } = &*self.highlights;
        self.hovered_faces.is_empty()
            && self.selected_faces.is_empty()
            && self.second_faces.is_empty()
            && outlined.is_empty()
            && selected_edges.is_empty()
            && second_edges.is_empty()
            && vertices.is_empty()
    }
}

impl PickIndex {
    /// What's drawn of `hovered` and `selected`: the faces hovered
    /// brighter (their edges as they are), and those selected tinted; an
    /// edge hovered outlined, one selected drawn in the selection's
    /// colour; a vertex hovered or selected as a disc, the hovered one
    /// last. What the mesh hasn't is left out.
    pub fn highlight(&self, hovered: &[Picked], selected: &[Picked]) -> ModelHighlight {
        self.highlight_with(hovered, selected, &[])
    }

    /// [`PickIndex::highlight`], with the faces and edges of `second` in
    /// the second colour, as the measure tool draws its B (its A as
    /// selected). A vertex there is left out: the measure tool draws its
    /// points itself.
    pub fn highlight_with(
        &self,
        hovered: &[Picked],
        selected: &[Picked],
        second: &[Picked],
    ) -> ModelHighlight {
        let has = |target: &&Picked| {
            let (id, count) = match **target {
                Picked::Face(face) => (face, self.mesh.face_count()),
                Picked::Edge(edge) => (edge, self.mesh.edge_count()),
                Picked::Vertex(corner) => (corner, self.mesh.corners().len()),
            };
            (id as usize) < count
        };
        let hovered: Vec<Picked> = hovered.iter().filter(has).copied().collect();
        let selected: Vec<Picked> = selected.iter().filter(has).copied().collect();
        let second = second.iter().filter(has).copied();
        let mut highlight = ModelHighlight::default();
        let mut highlights = Highlights::default();
        for &target in &hovered {
            match target {
                Picked::Face(face) => highlight.hovered_faces.push(face),
                Picked::Edge(edge) => highlights.outlined.push(edge),
                Picked::Vertex(_) => {}
            }
        }
        for &target in &selected {
            match target {
                Picked::Face(face) => highlight.selected_faces.push(face),
                Picked::Edge(edge) => highlights.selected_edges.push(edge),
                Picked::Vertex(corner) => highlights.vertices.push(Vertex {
                    corner,
                    hovered: hovered.contains(&target),
                    selected: true,
                }),
            }
        }
        for target in second {
            match target {
                Picked::Face(face) => highlight.second_faces.push(face),
                Picked::Edge(edge) => highlights.second_edges.push(edge),
                Picked::Vertex(_) => {}
            }
        }
        // The hovered vertices last, over the others.
        for &target in &hovered {
            if let Picked::Vertex(corner) = target
                && !selected.contains(&target)
            {
                highlights.vertices.push(Vertex {
                    corner,
                    hovered: true,
                    selected: false,
                });
            }
        }
        highlight.highlights = Arc::new(highlights);
        highlight
    }
}
