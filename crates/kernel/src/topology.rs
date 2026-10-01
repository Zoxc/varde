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
//! the point says; of several, the nearest to the point, ties to the
//! lowest index; none fails. Keys come from what the features were given,
//! never from the mesh, so a regenerated solid with other dimensions,
//! another tolerance or another triangulation resolves the same
//! references to the same faces.
//!
//! Names derived from keys (a copy's instance, a blend's edge, a shell's
//! offset face) are a fixed 64-bit [`mix`]: names are stored in files, so
//! it must never change, and `std`'s hasher may.
//!
//! Everything here is sequential and in index order, so the same mesh
//! gives the same topology and the same answers on any thread count. The
//! work is linear in the mesh, as tessellating is (which isn't budgeted
//! either: the mesh passed `check`, which bounds it), and resolving's
//! distance searches share a fixed allowance on top of a pass over the
//! candidates' boxes and corners.

use glam::DVec3;

use crate::Solid;
use crate::mesh::{FaceKey, Mesh};

pub(crate) mod distance;

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
    /// The aliases of its faces ([`Mesh::aliases`]), sorted.
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
    /// Its regions, chains and corners: see [`Topology`].
    pub fn topology(&self) -> Topology {
        Topology::new(self)
    }
}

impl Topology {
    /// The topology of `solid`, whose mesh passes `check`: regions
    /// numbered by their lowest triangle, chains by their lowest
    /// halfedge, corners by vertex.
    pub fn new(solid: &Solid) -> Topology {
        let mesh = solid.mesh();
        let (regions, region_of) = regions(mesh);
        let chains = chains(mesh, &region_of);
        let corners = corners(mesh, &region_of);
        Topology {
            regions,
            chains,
            corners,
            region_of,
        }
    }

    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    pub fn chains(&self) -> &[Chain] {
        &self.chains
    }

    pub fn corners(&self) -> &[Corner] {
        &self.corners
    }

    /// The region of triangle `tri`.
    pub fn region_of(&self, tri: u32) -> u32 {
        self.region_of[tri as usize]
    }

    /// The region named `key` (by name or alias), the nearest to `near`
    /// (the distance to its patches) among several. `solid` is the one
    /// the topology was made from.
    pub fn face(&self, solid: &Solid, key: &FaceKey, near: DVec3) -> Result<u32, NotFound> {
        let found = (0..self.regions.len() as u32).filter(|&r| self.regions[r as usize].named(key));
        let mesh = solid.mesh();
        nearest(found, near, |r, below, left| {
            let tris = &self.regions[r as usize].tris;
            let patches = tris.iter().map(|&t| mesh.patch(t as usize));
            distance::to_patches(near, patches, below, left)
        })
        .ok_or(NotFound::Face)
    }

    /// The chain between regions named `faces` (by name or alias, either
    /// way round), the nearest to `near` (the distance to its curves)
    /// among several.
    pub fn edge(&self, solid: &Solid, faces: [FaceKey; 2], near: DVec3) -> Result<u32, NotFound> {
        let named = |r: u32, key: &FaceKey| self.regions[r as usize].named(key);
        let found = (0..self.chains.len() as u32).filter(|&c| {
            let [r0, r1] = self.chains[c as usize].regions;
            (named(r0, &faces[0]) && named(r1, &faces[1]))
                || (named(r0, &faces[1]) && named(r1, &faces[0]))
        });
        let mesh = solid.mesh();
        nearest(found, near, |c, below, left| {
            let halfedges = &self.chains[c as usize].halfedges;
            distance::to_curves(near, halfedges.iter().map(|&h| mesh.curve(h)), below, left)
        })
        .ok_or(NotFound::Edge)
    }

    /// The corner where regions named by each of `faces` (by name or
    /// alias) meet, the nearest to `near` among several.
    pub fn corner(&self, solid: &Solid, faces: [FaceKey; 3], near: DVec3) -> Result<u32, NotFound> {
        let found = (0..self.corners.len() as u32).filter(|&c| {
            let regions = &self.corners[c as usize].regions;
            faces
                .iter()
                .all(|key| regions.iter().any(|&r| self.regions[r as usize].named(key)))
        });
        let verts = solid.mesh().verts();
        nearest(found, near, |c, _, _| {
            verts[self.corners[c as usize].vertex as usize].distance(near)
        })
        .ok_or(NotFound::Corner)
    }
}

/// Of the candidates `found`, in ascending order: the only one, or the
/// one at the least `distance` from `near` (given the least so far, below
/// which it must come to count, it may stop early above it; and the
/// search allowance all candidates share), the lowest on a tie; the
/// lowest where `near` isn't finite. `None` for none.
fn nearest(
    found: impl Iterator<Item = u32>,
    near: DVec3,
    mut distance: impl FnMut(u32, f64, &mut distance::Allowance) -> f64,
) -> Option<u32> {
    let found: Vec<u32> = found.collect();
    if found.len() == 1 || !near.is_finite() {
        return found.first().copied();
    }
    let mut left = distance::Allowance::new();
    let mut best: Option<(f64, u32)> = None;
    for i in found {
        let below = best.map_or(f64::INFINITY, |b| b.0);
        let d = distance(i, below, &mut left);
        if best.is_none_or(|b| d < b.0) {
            best = Some((d, i));
        }
    }
    best.map(|b| b.1)
}

/// The regions, numbered in the order of their lowest triangle, and each
/// triangle's region.
fn regions(mesh: &Mesh) -> (Vec<Region>, Vec<u32>) {
    let tris = mesh.tris();
    let key = |t: usize| mesh.faces()[tris[t].face as usize].name.key();
    let mut region_of = vec![u32::MAX; tris.len()];
    let mut regions = Vec::new();
    let mut stack = Vec::new();
    for seed in 0..tris.len() {
        if region_of[seed] != u32::MAX {
            continue;
        }
        let r = regions.len() as u32;
        let k = key(seed);
        region_of[seed] = r;
        stack.push(seed);
        let mut members = Vec::new();
        while let Some(t) = stack.pop() {
            members.push(t as u32);
            for he in tris[t].halfedges {
                let s = he.pair as usize / 3;
                if region_of[s] == u32::MAX && key(s) == k {
                    region_of[s] = r;
                    stack.push(s);
                }
            }
        }
        members.sort_unstable();
        let mut faces: Vec<u32> = members.iter().map(|&t| tris[t as usize].face).collect();
        faces.sort_unstable();
        faces.dedup();
        let mut aliases: Vec<FaceKey> = faces.iter().flat_map(|&f| mesh.face_aliases(f)).collect();
        aliases.sort_unstable();
        aliases.dedup();
        aliases.retain(|&a| a != k);
        regions.push(Region {
            key: k,
            aliases,
            tris: members,
        });
    }
    (regions, region_of)
}

/// The chains between the regions `region_of` gives the triangles, in the
/// order of their lowest halfedge.
fn chains(mesh: &Mesh, region_of: &[u32]) -> Vec<Chain> {
    let nh = 3 * mesh.tris().len() as u32;
    let region = |h: u32| region_of[h as usize / 3];
    // A chain's halfedges run on its lower region's side.
    let lower = |h: u32| region(h) < region(mesh.halfedge(h).pair);
    // How many chain edges meet at each vertex, and how many of their
    // halfedges start and end there (with one of each): a vertex with two
    // edges, one running in and one out, is inside a chain, which runs on
    // through it.
    let nv = mesh.verts().len();
    let mut meeting = vec![0u32; nv];
    let mut from = vec![(0u32, 0u32); nv];
    let mut to = vec![(0u32, 0u32); nv];
    for h in (0..nh).filter(|&h| lower(h)) {
        let (u, v) = (mesh.halfedge(h).start as usize, mesh.end(h) as usize);
        meeting[u] += 1;
        meeting[v] += 1;
        from[u] = (from[u].0 + 1, h);
        to[v] = (to[v].0 + 1, h);
    }
    let through =
        |v: usize, side: &[(u32, u32)]| (meeting[v] == 2 && side[v].0 == 1).then_some(side[v].1);
    let next = |h: u32| through(mesh.end(h) as usize, &from);
    let prev = |h: u32| through(mesh.halfedge(h).start as usize, &to);
    let mut seen = vec![false; nh as usize];
    let mut chains = Vec::new();
    for h in 0..nh {
        if seen[h as usize] || !lower(h) {
            continue;
        }
        // Back to an end, or round to `h` (the lowest of its chain, as
        // the first not yet seen).
        let mut start = h;
        let mut closed = false;
        for _ in 0..nh {
            match prev(start) {
                Some(p) if p == h => {
                    closed = true;
                    break;
                }
                Some(p) => start = p,
                None => break,
            }
        }
        let start = if closed { h } else { start };
        let mut halfedges = vec![start];
        seen[start as usize] = true;
        let mut at = start;
        while let Some(n) = next(at) {
            if n == start || seen[n as usize] {
                break;
            }
            seen[n as usize] = true;
            halfedges.push(n);
            at = n;
        }
        chains.push(Chain {
            regions: [region(start), region(mesh.halfedge(start).pair)],
            halfedges,
            closed,
        });
    }
    chains
}

/// The vertices where three or more regions meet, ascending.
fn corners(mesh: &Mesh, region_of: &[u32]) -> Vec<Corner> {
    let mut at: Vec<(u32, u32)> = mesh
        .tris()
        .iter()
        .zip(region_of)
        .flat_map(|(tri, &r)| tri.halfedges.map(|he| (he.start, r)))
        .collect();
    at.sort_unstable();
    at.dedup();
    at.chunk_by(|a, b| a.0 == b.0)
        .filter(|group| group.len() >= 3)
        .map(|group| Corner {
            vertex: group[0].0,
            regions: group.iter().map(|&(_, r)| r).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
