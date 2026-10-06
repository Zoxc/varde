//! What the viewport picks the model shown with, besides its mesh and
//! [`Picking`] tables: bounding volume hierarchies over the mesh's
//! triangles, the segments of its edges between two faces and its
//! vertices, the faces at each corner, and each tangent chain's edges.
//! Built with the model in the lane ([`PickTables::new`]), so the UI
//! thread only queries them (`varde_view::PickIndex`); on the web they
//! cross as bytes and are checked as they're decoded
//! ([`PickTables::from_parts`]).

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use varde_kernel::{Aabb, RenderMesh};

use crate::Picking;

/// The most triangles or edges in a leaf of a hierarchy.
const LEAF: usize = 4;

/// The picking hierarchies and groups of a model, see the module docs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PickTables {
    /// Over the mesh's triangles, by index.
    triangles: Bvh,
    /// Over the segments of the mesh's edges between two faces, each by
    /// where it starts in [`RenderMesh::edge_vertices`].
    segments: Bvh,
    /// Over the mesh's corners where three faces or more meet, by index.
    vertices: Bvh,
    /// The faces at each corner an edge between two faces ends at, as
    /// `(corner, face)`, sorted, each once.
    corner_faces: Vec<(u32, u32)>,
    /// Each tangent chain's edges, by its first.
    tangent_chains: Groups,
}

/// What [`PickTables`] are made of, as [`PickTables::to_parts`] gives them
/// and [`PickTables::from_parts`] takes them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PickTablesParts {
    pub triangles: (Vec<BvhNode>, Vec<u32>),
    pub segments: (Vec<BvhNode>, Vec<u32>),
    pub vertices: (Vec<BvhNode>, Vec<u32>),
    pub corner_faces: Vec<[u32; 2]>,
    pub chain_starts: Vec<u32>,
    pub chain_items: Vec<u32>,
}

/// Why parts don't make [`PickTables`] of a mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickTablesError {
    /// A hierarchy's nodes aren't a tree from the first one, each inner
    /// node's first child right after it, or a leaf's items aren't within
    /// its items, or its items name what the mesh hasn't.
    Tree,
    /// The corners' faces aren't sorted, each once, or name corners or
    /// faces the mesh hasn't.
    CornerFaces,
    /// The tangent chains' starts aren't one per edge and the end, in
    /// order to the end of the items, or an item isn't an edge.
    Chains,
}

impl std::fmt::Display for PickTablesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PickTablesError::Tree => "a picking hierarchy isn't a tree of the mesh",
            PickTablesError::CornerFaces => "the corners' faces don't go with the mesh",
            PickTablesError::Chains => "the tangent chains don't go with the mesh",
        })
    }
}

impl std::error::Error for PickTablesError {}

impl PickTables {
    /// The tables of `mesh` and its `picking`, which came with it; none
    /// if they don't go together.
    pub fn new(mesh: &RenderMesh, picking: &Picking) -> PickTables {
        let fits = picking.bodies().len() == mesh.part_ends().len()
            && picking.faces().len() == mesh.face_count()
            && picking.tangents().len() == mesh.edge_count();
        if !fits {
            return PickTables::default();
        }
        let corners = |triangle: &[u32; 3]| triangle.map(|i| position(mesh, i));
        let triangles = mesh.indices().as_chunks::<3>().0;
        let boxes = triangles.iter().map(|t| bounds(&corners(t)));
        let chain = |edge: usize| matches!(mesh.edge_faces()[edge], [a, b] if a != b);
        let segments: Vec<u32> = (0..mesh.edge_count())
            .filter(|&edge| chain(edge))
            .flat_map(|edge| {
                let range = mesh.polyline_range(edge).unwrap_or_default();
                range.start..range.end.saturating_sub(1)
            })
            .filter_map(|start| u32::try_from(start).ok())
            .collect();
        let segment_boxes = segments.iter().map(|&start| {
            let ends = segment(mesh, start).unwrap_or_default();
            bounds(&ends)
        });
        let mut corner_faces: Vec<(u32, u32)> = (0..mesh.edge_count())
            .filter(|&edge| chain(edge))
            .flat_map(|edge| {
                let faces = mesh.edge_faces()[edge];
                (mesh.edge_corners()[edge].into_iter())
                    .flat_map(move |corner| faces.map(|face| (corner, face)))
            })
            .collect();
        corner_faces.sort_unstable();
        corner_faces.dedup();
        let vertices: Vec<u32> = vertex_runs(&corner_faces).map(|run| run[0].0).collect();
        let vertex_boxes = vertices.iter().map(|&corner| {
            let at = Vec3::from(mesh.corners()[corner as usize]);
            [at, at]
        });
        // A crease is in no tangent chain.
        let tangents: Vec<u32> = (picking.tangents().iter().enumerate())
            .map(|(edge, &first)| if chain(edge) { first } else { u32::MAX })
            .collect();
        PickTables {
            triangles: Bvh::new(boxes.collect()),
            segments: Bvh::with_items(segment_boxes.collect(), segments),
            vertices: Bvh::with_items(vertex_boxes.collect(), vertices),
            corner_faces,
            tangent_chains: Groups::new(mesh.edge_count(), &tangents),
        }
    }

    /// Tables of `mesh` made of `parts`, if they could be its: each
    /// hierarchy a tree whose items are what it's over, the corners'
    /// faces sorted and the mesh's, the chains one group per edge. Not
    /// that they're the very ones [`PickTables::new`] makes: they only
    /// pick wrong.
    pub fn from_parts(
        mesh: &RenderMesh,
        parts: PickTablesParts,
    ) -> Result<PickTables, PickTablesError> {
        let PickTablesParts {
            triangles,
            segments,
            vertices,
            corner_faces,
            chain_starts,
            chain_items,
        } = parts;
        let edge_points = mesh.edge_vertices().len();
        let tree = |(nodes, items), of: usize| {
            Bvh::from_parts(nodes, items, of).ok_or(PickTablesError::Tree)
        };
        let triangles = tree(triangles, mesh.triangle_count())?;
        let segments = tree(segments, edge_points)?;
        let vertices = tree(vertices, mesh.corners().len())?;
        let corner_faces: Vec<(u32, u32)> = corner_faces.into_iter().map(|[c, f]| (c, f)).collect();
        let sorted = corner_faces.windows(2).all(|pair| pair[0] < pair[1]);
        let within = (corner_faces.iter()).all(|&(corner, face)| {
            (corner as usize) < mesh.corners().len() && (face as usize) < mesh.face_count()
        });
        if !(sorted && within) {
            return Err(PickTablesError::CornerFaces);
        }
        let tangent_chains = Groups::from_parts(chain_starts, chain_items, mesh.edge_count())
            .ok_or(PickTablesError::Chains)?;
        Ok(PickTables {
            triangles,
            segments,
            vertices,
            corner_faces,
            tangent_chains,
        })
    }

    /// The parts the tables are made of, as [`PickTables::from_parts`]
    /// takes them.
    pub fn to_parts(&self) -> PickTablesParts {
        let tree = |bvh: &Bvh| (bvh.nodes.clone(), bvh.items.clone());
        PickTablesParts {
            triangles: tree(&self.triangles),
            segments: tree(&self.segments),
            vertices: tree(&self.vertices),
            corner_faces: self.corner_faces.iter().map(|&(c, f)| [c, f]).collect(),
            chain_starts: self.tangent_chains.starts.clone(),
            chain_items: self.tangent_chains.items.clone(),
        }
    }

    /// About how many bytes they take.
    pub(crate) fn bytes(&self) -> usize {
        let tree =
            |bvh: &Bvh| size_of_val(&bvh.nodes[..]).saturating_add(size_of_val(&bvh.items[..]));
        (tree(&self.triangles))
            .saturating_add(tree(&self.segments))
            .saturating_add(tree(&self.vertices))
            .saturating_add(size_of_val(&self.corner_faces[..]))
            .saturating_add(size_of_val(&self.tangent_chains.starts[..]))
            .saturating_add(size_of_val(&self.tangent_chains.items[..]))
            .saturating_add(size_of_val(self))
    }

    /// Over the mesh's triangles, by index.
    pub fn triangles(&self) -> &Bvh {
        &self.triangles
    }

    /// Over the segments of the mesh's edges between two faces, each by
    /// where it starts in [`RenderMesh::edge_vertices`].
    pub fn segments(&self) -> &Bvh {
        &self.segments
    }

    /// Over the mesh's corners where three faces or more meet, by index.
    pub fn vertices(&self) -> &Bvh {
        &self.vertices
    }

    /// The faces at each corner an edge between two faces ends at, as
    /// `(corner, face)`, sorted, each once.
    pub fn corner_faces(&self) -> &[(u32, u32)] {
        &self.corner_faces
    }

    /// Where each tangent chain's edges start in
    /// [`PickTables::chain_items`], by its first edge, and the end; empty
    /// if there are none.
    pub fn chain_starts(&self) -> &[u32] {
        &self.tangent_chains.starts
    }

    /// The tangent chains' edges, chain after chain.
    pub fn chain_items(&self) -> &[u32] {
        &self.tangent_chains.items
    }

    /// The edges of the tangent chain whose first edge is `first`, none
    /// if there's no such chain.
    pub fn tangent_chain(&self, first: u32) -> &[u32] {
        self.tangent_chains.get(first)
    }
}

/// The mesh's vertex `index`, or the origin if it has none.
fn position(mesh: &RenderMesh, index: u32) -> Vec3 {
    Vec3::from(
        mesh.positions()
            .get(index as usize)
            .copied()
            .unwrap_or_default(),
    )
}

/// The ends of `mesh`'s edge segment starting at `start` in its edge
/// vertices, if there's one.
fn segment(mesh: &RenderMesh, start: u32) -> Option<[Vec3; 2]> {
    let start = start as usize;
    let ends = mesh.edge_vertices().get(start..start.checked_add(2)?)?;
    Some([position(mesh, ends[0]), position(mesh, ends[1])])
}

/// The box around `points`.
fn bounds(points: &[Vec3]) -> [Vec3; 2] {
    let first = points.first().copied().unwrap_or_default();
    (points.iter()).fold([first; 2], |[min, max], &p| [min.min(p), max.max(p)])
}

/// The runs of `corner_faces`, sorted by corner, of each corner where
/// three faces or more meet: the vertices.
pub fn vertex_runs(corner_faces: &[(u32, u32)]) -> impl Iterator<Item = &[(u32, u32)]> {
    (corner_faces.chunk_by(|a, b| a.0 == b.0)).filter(|run| run.len() >= 3)
}

/// Items grouped by what they belong to: a chain's edges.
#[derive(Debug, Clone, Default, PartialEq)]
struct Groups {
    /// Where each group's items start in `items`, and the end.
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl Groups {
    /// The items of `groups` groups, item `i` in group `of[i]`, unless
    /// that's past the groups (as a crease's tangent chain is made out to
    /// be, `u32::MAX`). None if there are more items than `u32`s number,
    /// which no mesh has.
    fn new(groups: usize, of: &[u32]) -> Self {
        if u32::try_from(of.len()).is_err() {
            return Self::default();
        }
        let group = |g: u32| Some(g as usize).filter(|&g| g < groups);
        // Counts, then running sums: no more than `of.len()`, a `u32`.
        let mut starts = vec![0u32; groups + 1];
        for g in of.iter().filter_map(|&g| group(g)) {
            starts[g + 1] += 1;
        }
        for g in 0..groups {
            starts[g + 1] += starts[g];
        }
        let mut filled = starts.clone();
        let mut items = vec![0u32; starts[groups] as usize];
        for (item, g) in of.iter().enumerate() {
            if let Some(g) = group(*g) {
                items[filled[g] as usize] = item as u32;
                filled[g] += 1;
            }
        }
        Self { starts, items }
    }

    /// Groups of `starts` and `items`, if there's a start per group of
    /// `groups` and the end, in order to the end of the items, each item
    /// below `groups`; or none at all, as [`Groups::new`] gives past its
    /// bounds.
    fn from_parts(starts: Vec<u32>, items: Vec<u32>, groups: usize) -> Option<Self> {
        let none = starts.is_empty() && items.is_empty();
        let ordered = starts.first() == Some(&0)
            && starts.windows(2).all(|pair| pair[0] <= pair[1])
            && starts
                .last()
                .is_some_and(|&end| end as usize == items.len());
        let within = items.iter().all(|&item| (item as usize) < groups);
        (none || (starts.len() == groups.checked_add(1)? && ordered && within))
            .then_some(Self { starts, items })
    }

    /// The items of `group`, none if there's no such group.
    fn get(&self, group: u32) -> &[u32] {
        let group = group as usize;
        let (Some(&start), Some(&end)) = (self.starts.get(group), self.starts.get(group + 1))
        else {
            return &[];
        };
        self.items.get(start as usize..end as usize).unwrap_or(&[])
    }
}

/// A bounding volume hierarchy over items with boxes: split in two at the
/// median of their middles along the box's longest side, until a few
/// are left. Built sequentially, the same for the same items. Its nodes
/// are a tree from the first: an inner node's first child right after
/// it, its second further on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bvh {
    nodes: Vec<BvhNode>,
    /// The items in the leaves' order.
    items: Vec<u32>,
}

/// A node of a [`Bvh`]: its box, and its leaf's items or its children.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct BvhNode {
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// A leaf's first item in its hierarchy's items, or an inner node's
    /// second child; its first comes right after it.
    pub start: u32,
    /// A leaf's number of items, 0 for an inner node.
    pub count: u32,
}

impl Bvh {
    /// Over items `0..boxes.len()`, each in its box.
    fn new(boxes: Vec<[Vec3; 2]>) -> Self {
        let items = (0..boxes.len())
            .filter_map(|i| u32::try_from(i).ok())
            .collect();
        Self::with_items(boxes, items)
    }

    /// Over `items`, the `i`th in `boxes[i]`.
    fn with_items(boxes: Vec<[Vec3; 2]>, items: Vec<u32>) -> Self {
        // Each item with its box and middle, permuted in place as the tree
        // is built: sequential memory, not a lookup per comparison.
        let mut entries: Vec<Entry> = (boxes.iter().zip(&items))
            .map(|(&[min, max], &item)| Entry {
                min,
                max,
                middle: min + max,
                item,
            })
            .collect();
        let mut nodes = Vec::new();
        if !entries.is_empty() {
            build(&mut entries, 0, &mut nodes);
        }
        let items = entries.into_iter().map(|entry| entry.item).collect();
        Self { nodes, items }
    }

    /// A hierarchy of `nodes` and `items`, if the nodes are a tree from
    /// the first (each reached once, so a walk visits each once), no more
    /// than twice the items, each leaf's items within `items`, and the
    /// items no more than `of`, each below it.
    fn from_parts(nodes: Vec<BvhNode>, items: Vec<u32>, of: usize) -> Option<Self> {
        // As many nodes as a tree over its items has at most, and no
        // more items than what it's over.
        if items.len() > of || nodes.len() > items.len().saturating_mul(2) {
            return None;
        }
        let mut reached = vec![false; nodes.len()];
        if let Some(root) = reached.first_mut() {
            *root = true;
        }
        for (index, node) in nodes.iter().enumerate() {
            if node.count > 0 {
                let start = node.start as usize;
                if start.checked_add(node.count as usize)? > items.len() {
                    return None;
                }
                continue;
            }
            // Both children after it, so each walk goes only forward.
            let (first, second) = (index + 1, node.start as usize);
            if second <= first || second >= nodes.len() {
                return None;
            }
            for child in [first, second] {
                if std::mem::replace(&mut reached[child], true) {
                    return None;
                }
            }
        }
        let whole = reached.iter().all(|&r| r);
        let within = items.iter().all(|&item| (item as usize) < of);
        (whole && within).then_some(Self { nodes, items })
    }

    /// Its nodes, the root first: empty if it's over nothing.
    pub fn nodes(&self) -> &[BvhNode] {
        &self.nodes
    }

    /// The items in the leaves' order.
    pub fn items(&self) -> &[u32] {
        &self.items
    }

    /// A leaf's items.
    pub fn leaf(&self, node: &BvhNode) -> &[u32] {
        let start = node.start as usize;
        self.items
            .get(start..start.saturating_add(node.count as usize))
            .unwrap_or(&[])
    }
}

impl BvhNode {
    /// Its box, as an [`Aabb`].
    pub fn aabb(&self) -> Aabb {
        Aabb {
            min: Vec3::from(self.min),
            max: Vec3::from(self.max),
        }
    }
}

/// Moves the entries `left` says go left before the rest, in one pass
/// from both ends: how many go left.
fn partition(entries: &mut [Entry], left: impl Fn(&Entry) -> bool) -> usize {
    let (mut i, mut j) = (0, entries.len());
    loop {
        while i < j && left(&entries[i]) {
            i += 1;
        }
        while i < j && !left(&entries[j - 1]) {
            j -= 1;
        }
        if i >= j {
            return i;
        }
        entries.swap(i, j - 1);
    }
}

/// An item being built into a [`Bvh`]: its box, twice its middle, and
/// the item.
#[derive(Debug, Clone, Copy)]
struct Entry {
    min: Vec3,
    max: Vec3,
    middle: Vec3,
    item: u32,
}

/// Builds the nodes over `entries`, the items `offset..` of the whole,
/// onto `nodes`, reordering `entries` into the leaves' order. Ties along
/// the axis split by item, so the same items give the same tree.
fn build(entries: &mut [Entry], offset: usize, nodes: &mut Vec<BvhNode>) {
    let mut bounds = [Vec3::INFINITY, Vec3::NEG_INFINITY];
    let mut middles = [Vec3::INFINITY, Vec3::NEG_INFINITY];
    for entry in entries.iter() {
        bounds = [bounds[0].min(entry.min), bounds[1].max(entry.max)];
        middles = [middles[0].min(entry.middle), middles[1].max(entry.middle)];
    }
    let index = nodes.len();
    // Items and nodes number fewer than the mesh's triangles, whose
    // indices are `u32`s.
    let at = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    nodes.push(BvhNode {
        min: bounds[0].to_array(),
        max: bounds[1].to_array(),
        start: at(offset),
        count: at(entries.len()),
    });
    if entries.len() <= LEAF {
        return;
    }
    let axis = (middles[1] - middles[0]).max_position();
    // Split at the middle of the middles along the axis, one pass; where
    // that leaves a side with too few (bunched items), at the median.
    let split = (middles[0][axis] + middles[1][axis]) / 2.0;
    let mut half = partition(entries, |entry| entry.middle[axis] < split);
    if half < entries.len() / 4 || half > entries.len() - entries.len() / 4 {
        half = entries.len() / 2;
        entries.select_nth_unstable_by(half, |a, b| {
            (a.middle[axis].total_cmp(&b.middle[axis])).then(a.item.cmp(&b.item))
        });
    }
    let (left, right) = entries.split_at_mut(half);
    build(left, offset, nodes);
    let second = nodes.len();
    build(right, offset + half, nodes);
    nodes[index].start = at(second);
    nodes[index].count = 0;
}

#[cfg(test)]
mod tests;
