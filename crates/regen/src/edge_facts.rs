//! What a body's edges are, on request ([`Query::Topology`]): each
//! chain's form, ends and convexity, read from the body's solid. Not part
//! of the [`Picking`](crate::Picking) tables every model carries: only a client asking
//! about one body's edges (MCP's `body_topology`) needs them, so they're
//! worked out for that body when asked, beside the model, queued in order
//! as exports are.
//!
//! The answer lists the body's topology's chains in order
//! ([`Topology::chains`]), which is the order of the first edges of the
//! body's part in the mesh of a model of the same document, generation
//! and roll back: the facts at index `i` are those of the part's edge
//! `part.edges.start + i`. Each carries its two faces' keys, sorted as
//! [`Picking::edge_keys`](crate::Picking::edge_keys) gives them, so the pairing can be checked.
//!
//! [`Query::Topology`]: crate::Query::Topology

use glam::DVec3;
use serde::{Deserialize, Serialize};
use varde_kernel_compat::measure::{EdgeShape, edge_shape};
use varde_kernel_compat::mesh::FaceKey;
use varde_kernel_compat::topology::Chain;
use varde_kernel_compat::{RenderMesh, Solid, Topology};

use crate::bounds::point;
use crate::inspect::EdgeForm;

/// How an edge's two faces meet along it, seen from outside the solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Convexity {
    /// The solid's angle there is under 180°: an outside corner.
    Convex,
    /// Over 180°: an inside corner.
    Concave,
    /// The faces run on into each other within 1° (a fillet's rim).
    Smooth,
}

/// What an edge between two faces is: one of a chain of a body's
/// topology, see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeFacts {
    /// The keys of the regions either side, sorted, as an edge reference
    /// stores them and [`Picking::edge_keys`](crate::Picking::edge_keys) gives them.
    pub faces: [FaceKey; 2],
    /// What its curves make ([`edge_shape`], within its slack): a
    /// circle's or ellipse's `axis` turns the way the edge runs, its
    /// halfedges on the lower face's triangles, so that face is on its
    /// left seen from outside. One with numbers past
    /// [`Picking::MAX_VALUE`](crate::Picking::MAX_VALUE) is [`EdgeForm::Other`].
    pub form: EdgeForm,
    /// Its first and last ends, as it runs; `None` for a closed edge, or
    /// one with an end past [`Picking::MAX_VALUE`](crate::Picking::MAX_VALUE).
    pub ends: Option<[[f64; 3]; 2]>,
    /// `None` where no sample along it gave both faces a normal.
    pub convexity: Option<Convexity>,
}

impl EdgeFacts {
    /// `chain`'s, a chain of `topology`, `solid`'s topology, of shape
    /// `shape` ([`edge_shape`]).
    fn of(solid: &Solid, topology: &Topology, chain: &Chain, shape: &EdgeShape) -> EdgeFacts {
        let mesh = solid.mesh();
        let ends = match (
            chain.closed,
            chain.halfedges.first(),
            chain.halfedges.last(),
        ) {
            (false, Some(&first), Some(&last)) => {
                let verts = mesh.verts();
                let from = verts[mesh.halfedge(first).start as usize];
                let to = verts[mesh.end(last) as usize];
                [from, to]
                    .iter()
                    .all(|p| point(p.to_array()))
                    .then(|| [from.to_array(), to.to_array()])
            }
            _ => None,
        };
        let regions = topology.regions();
        let [a, b] = chain.regions.map(|r| regions[r as usize].key);
        EdgeFacts {
            faces: [a.min(b), a.max(b)],
            form: Some(EdgeForm::of(shape))
                .filter(EdgeForm::valid)
                .unwrap_or(EdgeForm::Other),
            ends,
            convexity: convexity(solid, chain),
        }
    }

    /// Whether its faces are two keys in order, its form's numbers finite
    /// and within [`Picking::MAX_VALUE`](crate::Picking::MAX_VALUE) (sizes positive, directions
    /// unit), its ends within [`Picking::MAX_VALUE`](crate::Picking::MAX_VALUE), and a straight one
    /// has ends.
    pub fn valid(&self) -> bool {
        self.faces[0] < self.faces[1]
            && self.form.valid()
            && self.ends.is_none_or(|[a, b]| point(a) && point(b))
            && !(matches!(self.form, EdgeForm::Line { .. }) && self.ends.is_none())
    }
}

/// Why a body's edge facts aren't sent: more than the wire takes.
pub(crate) const TOO_MANY_EDGES: &str = "the body has more edges than can be sent";

/// The facts of each of the chains of `topology`, `solid`'s topology, in
/// order: see the module docs. Fails where there are more than a mesh
/// may hold edges ([`RenderMesh::MAX_EDGE_POLYLINES`]), which the wire
/// would refuse.
pub(crate) fn body_edges(solid: &Solid, topology: &Topology) -> Result<Vec<EdgeFacts>, String> {
    let chains = topology.chains();
    if chains.len() > RenderMesh::MAX_EDGE_POLYLINES {
        return Err(TOO_MANY_EDGES.to_owned());
    }
    Ok((chains.iter())
        .map(|chain| EdgeFacts::of(solid, topology, chain, &edge_shape(solid, chain)))
        .collect())
}

/// `sin 1°`: how near parallel two faces' normals must be for the edge
/// between them to be [`Convexity::Smooth`], the angle
/// [`Topology::tangent_chains`] takes for edges running on smoothly.
const SMOOTH_SINE: f64 = 0.017_452_406_437_283_51;

/// How `chain`'s two faces meet, from the solid's outward normals either
/// side at the middle of one of its halfedges: the first, in the order it
/// runs, where both triangles' patches have a normal there. With `t` the
/// halfedge's chord (its curve's tangent at its middle, as its end
/// weights are 1), `a` the outward normal of the triangle it's on and `b`
/// that of the triangle across, the edge is smooth where `a·b > 0` and
/// `|a × b| ≤ sin 1° |a||b|`, else convex where `(a × b)·t > 0` and
/// concave where it's below. Decided by `+ − ×` on the patches' normals
/// ([`Patch::normal`](varde_kernel_compat::patch::Patch::normal), counter-clockwise seen from outside).
fn convexity(solid: &Solid, chain: &Chain) -> Option<Convexity> {
    let mesh = solid.mesh();
    let tris = mesh.tris().len();
    // The normal at the middle of halfedge `h`, of its triangle's patch.
    let normal = |h: u32| {
        let tri = h as usize / 3;
        if tri >= tris {
            return None;
        }
        let i = h as usize % 3;
        let mut u = [0.0; 3];
        u[i] = 0.5;
        u[(i + 1) % 3] = 0.5;
        let n = mesh.patch(tri).normal(DVec3::from(u));
        (n.is_finite() && n != DVec3::ZERO).then_some(n)
    };
    chain.halfedges.iter().find_map(|&h| {
        let a = normal(h)?;
        let b = normal(mesh.halfedge(h).pair)?;
        let curve = mesh.curve(h);
        let t = curve.p1 - curve.p0;
        let cross = a.cross(b);
        let side = cross.dot(t);
        if !side.is_finite() {
            return None;
        }
        let tangent = a.dot(b) > 0.0
            && cross.length_squared()
                <= SMOOTH_SINE * SMOOTH_SINE * a.length_squared() * b.length_squared();
        Some(if tangent {
            Convexity::Smooth
        } else if side > 0.0 {
            Convexity::Convex
        } else if side < 0.0 {
            Convexity::Concave
        } else {
            return None;
        })
    })
}

#[cfg(test)]
mod tests;
