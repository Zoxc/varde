//! The view options' choices (projection, shading, edges): their order,
//! labels and icons, shared by the settings page and the status bar's
//! View menu so the two name them alike.

use varde_render::{Projection, Shading};

use crate::Edges;
use crate::icons::Icon;

pub(crate) const PROJECTIONS: [Projection; 2] = [Projection::Orthographic, Projection::Perspective];

pub(crate) fn projection_said(projection: Projection) -> &'static str {
    match projection {
        Projection::Orthographic => "Orthographic",
        Projection::Perspective => "Perspective",
    }
}

pub(crate) fn projection_icon(projection: Projection) -> Icon {
    match projection {
        Projection::Orthographic => Icon::Ortho,
        Projection::Perspective => Icon::Persp,
    }
}

pub(crate) const SHADINGS: [Shading; 4] = [
    Shading::Regular,
    Shading::Flat,
    Shading::Metal,
    Shading::FlatMetal,
];

pub(crate) fn shading_said(shading: Shading) -> &'static str {
    match shading {
        Shading::Regular => "Regular",
        Shading::Flat => "Flat",
        Shading::Metal => "Metal",
        Shading::FlatMetal => "Flat metal",
    }
}

pub(crate) fn shading_icon(shading: Shading) -> Icon {
    match shading {
        Shading::Regular => Icon::ShadeRegular,
        Shading::Flat => Icon::ShadeFlat,
        Shading::Metal => Icon::ShadeMetal,
        Shading::FlatMetal => Icon::ShadeFlatMetal,
    }
}

pub(crate) const EDGES: [Edges; 3] = [Edges::Default, Edges::Wireframe, Edges::Tessellation];

pub(crate) fn edges_said(edges: Edges) -> &'static str {
    match edges {
        Edges::Default => "Features",
        Edges::Wireframe => "Patches",
        Edges::Tessellation => "Triangles",
    }
}

pub(crate) fn edges_icon(edges: Edges) -> Icon {
    match edges {
        Edges::Default => Icon::EdgesDefault,
        Edges::Wireframe => Icon::EdgesWireframe,
        Edges::Tessellation => Icon::EdgesTessellation,
    }
}
