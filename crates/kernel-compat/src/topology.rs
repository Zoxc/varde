//! The faces, edges and corners of a solid as users see them, derived
//! from its mesh and never stored, and resolving the names references
//! keep to them.
//!
//! A [`Topology`] has:
//!
//! - **regions**: connected sets of triangles of one [`FaceKey`] (joined
//!   across edges), each with the aliases of the faces in it;
//! - **chains**: maximal paths of mesh edges with the same two regions
//!   either side, open (from corner to corner) or closed;
//! - **corners**: vertices where three or more regions meet.
//!
//! A reference names a face by its key, an edge by the keys of the faces
//! either side, a corner by three of the faces meeting there, each with a
//! point near it. Resolving ([`Topology::face`], [`Topology::edge`],
//! [`Topology::corner`]) takes the regions, chains or corners with those
//! keys, by name or alias (a key absorbed when two faces on one surface
//! merged still names the face that took it in): one is taken whatever
//! the point says; of several, the nearest to the point, ties (within a
//! billionth of the solid's size) to the lowest index; none fails. Keys
//! come from what the features were given, never from the mesh, so a
//! regenerated solid with other dimensions, another tolerance or another
//! triangulation resolves the same references to the same faces.
//!
//! Points and directions (corners, edges' middles and centres, faces'
//! normals and axes, edges' directions) are resolved by the same names
//! ([`Topology::corner_point`], [`Topology::edge_direction`] and the
//! others in `topology/datums.rs`).
//!
//! Names derived from keys (a copy's instance, a blend's edge, a shell's
//! offset face) are a fixed 64-bit [`mix`]: names are stored in files, so
//! it must never change, and `std`'s hasher may.
//!
//! In the facade every solid is empty, so its topology is too and
//! nothing resolves; the Varde kernel's user topology takes over here.
//! Of several candidates with the keys, the old kernel took the nearest
//! to the point; the facade, which can measure no distance, finds none
//! (there are no candidates meanwhile).

use glam::DVec3;

use crate::Solid;
use crate::mesh::FaceKey;

mod datums;

pub use datums::{Unresolved, beside, runs_with};

/// The odd constant splitmix64 steps by: `2⁶⁴/φ`.
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// splitmix64's finalizer: a bijection of `u64` mixing every bit into
/// every other.
fn finalize(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The fixed mix of `parts` into one name: starting from the count of
/// parts, each part in turn as `h = finalize((h + GAMMA) ^ part)`, with
/// splitmix64's finalizer and constant (wrapping arithmetic). Names
/// derived from other names are made by it and stored in files, so it
/// never changes: the tests pin its values.
pub fn mix(parts: &[u64]) -> u64 {
    parts.iter().fold(parts.len() as u64, |h, &part| {
        finalize(h.wrapping_add(GAMMA) ^ part)
    })
}

/// The `edge` of the [`FacePart::Blend`](crate::mesh::FacePart::Blend)
/// faces a chamfer or fillet makes along the edge between the faces
/// `faces` (either order), the `ordinal`th of its references with that
/// pair: the [`mix`] of the two keys' [`FaceKey::mixed`] (the lower key
/// first) and the ordinal. So adding or removing another edge of the same
/// feature doesn't rename it.
pub fn blend_edge(faces: [FaceKey; 2], ordinal: u32) -> u64 {
    let [a, b] = faces;
    let (a, b) = (a.min(b), a.max(b));
    mix(&[a.mixed(), b.mixed(), u64::from(ordinal)])
}

/// A face as users see it: a connected set of triangles of one key.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub key: FaceKey,
    /// The aliases of its faces ([`Mesh::aliases`](crate::mesh::Mesh::aliases)), sorted.
    pub aliases: Vec<FaceKey>,
    /// Its triangles, ascending.
    pub tris: Vec<u32>,
}

impl Region {
    /// Whether `key` names it: its key or one of its aliases.
    pub fn named(&self, key: &FaceKey) -> bool {
        self.key == *key || self.aliases.binary_search(key).is_ok()
    }
}

/// An edge as users see it: a maximal path of mesh edges with the same
/// two regions either side.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    /// The regions either side, the lower index first.
    pub regions: [u32; 2],
    /// Its halfedges on the first region's triangles, end to end in the
    /// order they run: from one end to the other of an open chain, and
    /// from the lowest halfedge round a closed one.
    pub halfedges: Vec<u32>,
    /// Whether it closes on itself (a hole's rim) rather than ending at
    /// a vertex where other regions meet or the two meet again.
    pub closed: bool,
}

/// A vertex where three or more regions meet.
#[derive(Debug, Clone, PartialEq)]
pub struct Corner {
    pub vertex: u32,
    /// The regions meeting there, ascending.
    pub regions: Vec<u32>,
}

/// Why a reference doesn't resolve: no region, chain or corner has its
/// keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotFound {
    Face,
    Edge,
    Corner,
}

impl std::fmt::Display for NotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NotFound::Face => "face not found",
            NotFound::Edge => "edge not found",
            NotFound::Corner => "corner not found",
        })
    }
}

impl std::error::Error for NotFound {}

/// The regions, chains and corners of a solid: see the [module](self)
/// docs. Derived from one solid's mesh; resolving takes that solid again
/// for the geometry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Topology {
    regions: Vec<Region>,
    chains: Vec<Chain>,
    corners: Vec<Corner>,
    /// Each triangle's region.
    region_of: Vec<u32>,
}

impl Solid {
    /// The solid's faces, edges and corners: see [`Topology`].
    pub fn topology(&self) -> Topology {
        Topology::new(self)
    }
}

impl Topology {
    /// The topology of `solid`: empty, as every solid of the facade is.
    pub fn new(_solid: &Solid) -> Topology {
        Topology::default()
    }

    /// The regions, in the order of their lowest triangle.
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    /// The chains, in the order of their lowest halfedge.
    pub fn chains(&self) -> &[Chain] {
        &self.chains
    }

    /// The corners, ascending by vertex.
    pub fn corners(&self) -> &[Corner] {
        &self.corners
    }

    /// The number of triangles it covers.
    pub fn triangles(&self) -> usize {
        self.region_of.len()
    }

    /// Triangle `tri`'s region, or `u32::MAX` for a triangle it doesn't
    /// have.
    pub fn region_of(&self, tri: u32) -> u32 {
        self.region_of
            .get(tri as usize)
            .copied()
            .unwrap_or(u32::MAX)
    }

    /// The region named `key` (by key or alias): the only one, else
    /// [`NotFound::Face`].
    pub fn face(&self, _solid: &Solid, key: &FaceKey, _near: DVec3) -> Result<u32, NotFound> {
        only((0..self.regions.len() as u32).filter(|&r| self.regions[r as usize].named(key)))
            .ok_or(NotFound::Face)
    }

    /// The chain between the regions named `faces` (either way round):
    /// the only one, else [`NotFound::Edge`].
    pub fn edge(&self, _solid: &Solid, faces: [FaceKey; 2], _near: DVec3) -> Result<u32, NotFound> {
        let named = |r: u32, key: &FaceKey| self.regions[r as usize].named(key);
        only((0..self.chains.len() as u32).filter(|&c| {
            let [r0, r1] = self.chains[c as usize].regions;
            (named(r0, &faces[0]) && named(r1, &faces[1]))
                || (named(r0, &faces[1]) && named(r1, &faces[0]))
        }))
        .ok_or(NotFound::Edge)
    }

    /// The corner where the regions named `faces` (three different keys)
    /// meet: the only one, else [`NotFound::Corner`].
    pub fn corner(
        &self,
        _solid: &Solid,
        faces: [FaceKey; 3],
        _near: DVec3,
    ) -> Result<u32, NotFound> {
        let [a, b, c] = &faces;
        if a == b || b == c || a == c {
            return Err(NotFound::Corner);
        }
        only((0..self.corners.len() as u32).filter(|&c| {
            let regions = &self.corners[c as usize].regions;
            faces
                .iter()
                .all(|key| regions.iter().any(|&r| self.regions[r as usize].named(key)))
        }))
        .ok_or(NotFound::Corner)
    }

    /// Each chain's tangent chain: the lowest chain of those running on
    /// smoothly into each other. In the facade every chain is its own.
    pub fn tangent_chains(&self, _solid: &Solid) -> Vec<u32> {
        (0..self.chains.len() as u32).collect()
    }
}

/// The one item of `found`, or `None` for none or several.
fn only(mut found: impl Iterator<Item = u32>) -> Option<u32> {
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests;
