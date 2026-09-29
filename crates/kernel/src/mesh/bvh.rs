use crate::KernelError;
use crate::budget::Work;
use crate::par::par_map;
use crate::patch::Bounds3;

/// How many boxes a leaf holds at most.
const LEAF: usize = 4;

/// How many boxes [`Bvh::pairs_within`] counts the pairs of at a time
/// before collecting them.
const CHUNK: usize = 256;

/// A bounding volume hierarchy over boxes, such as the boxes around
/// patches' control hulls. Built sequentially, by median splits along the
/// longest axis of the box centres, with ties broken by index. Queries
/// return indices in ascending order, so their results don't depend on
/// the tree's shape.
#[derive(Debug, Clone, Default)]
pub struct Bvh {
    boxes: Vec<Bounds3>,
    nodes: Vec<Node>,
    /// Box indices, in leaf order.
    items: Vec<u32>,
}

/// A leaf (`count > 0`) holds `items[start..start + count]`; an inner
/// node (`count == 0`) has its children at the next index and at `start`.
#[derive(Debug, Clone, Copy)]
struct Node {
    bounds: Bounds3,
    start: u32,
    count: u32,
}

impl Bvh {
    /// The hierarchy over `boxes`, which must be finite and number at most
    /// `u32::MAX`.
    pub fn new(boxes: Vec<Bounds3>) -> Bvh {
        assert!(u32::try_from(boxes.len()).is_ok(), "too many boxes");
        let mut bvh = Bvh {
            items: (0..boxes.len() as u32).collect(),
            nodes: Vec::with_capacity(2 * boxes.len() / LEAF + 1),
            boxes,
        };
        if !bvh.boxes.is_empty() {
            bvh.build(0, bvh.items.len());
        }
        bvh
    }

    /// Builds the node over `items[lo..hi]`, returning its index.
    fn build(&mut self, lo: usize, hi: usize) -> u32 {
        let index = self.nodes.len();
        let items = &mut self.items[lo..hi];
        let boxes = &self.boxes;
        let bounds = items[1..]
            .iter()
            .fold(boxes[items[0] as usize], |b, &i| b.union(boxes[i as usize]));
        self.nodes.push(Node {
            bounds,
            start: lo as u32,
            count: (hi - lo) as u32,
        });
        if hi - lo <= LEAF {
            return index as u32;
        }
        // Twice the centres, which orders them the same.
        let centre = |i: u32| boxes[i as usize].min + boxes[i as usize].max;
        let (mut min, mut max) = (centre(items[0]), centre(items[0]));
        for &i in &items[1..] {
            (min, max) = (min.min(centre(i)), max.max(centre(i)));
        }
        let extent = max - min;
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };
        let mid = (hi - lo) / 2;
        items.select_nth_unstable_by(mid, |&a, &b| {
            centre(a)[axis].total_cmp(&centre(b)[axis]).then(a.cmp(&b))
        });
        self.nodes[index].count = 0;
        self.build(lo, lo + mid);
        let right = self.build(lo + mid, hi);
        self.nodes[index].start = right;
        index as u32
    }

    /// How many boxes it holds.
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    /// Box `i`.
    pub fn bounds(&self, i: u32) -> Bounds3 {
        self.boxes[i as usize]
    }

    /// Appends to `out`, in ascending order, the indices of the boxes that
    /// come within `margin` of `query` along every axis: every box whose
    /// contents could be within `margin` of the contents of `query`.
    pub fn query(&self, query: &Bounds3, margin: f64, out: &mut Vec<u32>) {
        let from = out.len();
        self.visit(query, margin, |i| {
            out.push(i);
            true
        });
        out[from..].sort_unstable();
    }

    /// Calls `f` with the index of every box within `margin` of `query`
    /// along every axis, in the tree's order, for as long as it returns
    /// true.
    fn visit(&self, query: &Bounds3, margin: f64, mut f: impl FnMut(u32) -> bool) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0u32];
        while let Some(n) = stack.pop() {
            let node = self.nodes[n as usize];
            if !near(&node.bounds, query, margin) {
                continue;
            }
            if node.count == 0 {
                stack.push(node.start);
                stack.push(n + 1);
            } else {
                let items = &self.items[node.start as usize..(node.start + node.count) as usize];
                for &i in items {
                    if near(&self.boxes[i as usize], query, margin) && !f(i) {
                        return;
                    }
                }
            }
        }
    }

    /// Every pair `[i, j]`, `i < j`, of boxes within `margin` of each
    /// other along every axis, sorted. The queries run in parallel.
    pub fn self_pairs(&self, margin: f64) -> Vec<[u32; 2]> {
        let ids: Vec<u32> = (0..self.boxes.len() as u32).collect();
        let found = par_map(&ids, |&i| {
            let mut near = Vec::new();
            self.query(&self.boxes[i as usize], margin, &mut near);
            near.retain(|&j| j > i);
            near
        });
        let mut pairs = Vec::with_capacity(found.iter().map(Vec::len).sum());
        for (i, near) in found.into_iter().enumerate() {
            pairs.extend(near.into_iter().map(|j| [i as u32, j]));
        }
        pairs
    }
}

impl Bvh {
    /// [`Self::self_pairs`], one unit of `work` each, failing with
    /// [`KernelError::TooComplex`] as soon as they would number more than
    /// `work` has left: see [`Self::pairs_within`].
    pub(crate) fn self_pairs_within(
        &self,
        margin: f64,
        work: &mut Work,
    ) -> Result<Vec<[u32; 2]>, KernelError> {
        let ids: Vec<u32> = (0..self.boxes.len() as u32).collect();
        self.pairs_within(&ids, margin, |i, j| j > i, work)
    }

    /// For each box `i` of `ids`, the pairs `[i, j]` with the boxes `j`
    /// within `margin` of it along every axis that `keep(i, j)` takes, in
    /// order: by `ids`, then by `j`. They take one unit of `work` each,
    /// and fail with [`KernelError::TooComplex`] as soon as they would
    /// number more than `work` has left: they are counted, a chunk of
    /// `ids` at a time, before they are collected, as boxes crowding each
    /// other can make pairs of nearly every two. Whether it fails depends
    /// only on how many there are, not on the threads.
    pub(crate) fn pairs_within(
        &self,
        ids: &[u32],
        margin: f64,
        keep: impl Fn(u32, u32) -> bool + Sync,
        work: &mut Work,
    ) -> Result<Vec<[u32; 2]>, KernelError> {
        let mut pairs = Vec::new();
        for chunk in ids.chunks(CHUNK) {
            // More than this many, from any one box, is already too many.
            let most = usize::try_from(work.left()).unwrap_or(usize::MAX);
            let counts = par_map(chunk, |&i| {
                let mut count = 0usize;
                self.visit(&self.boxes[i as usize], margin, |j| {
                    count += usize::from(keep(i, j));
                    count <= most
                });
                count
            });
            work.spend(counts.into_iter().fold(0, usize::saturating_add))?;
            let found = par_map(chunk, |&i| {
                let mut near = Vec::new();
                self.query(&self.boxes[i as usize], margin, &mut near);
                near.retain(|&j| keep(i, j));
                near
            });
            for (&i, near) in chunk.iter().zip(found) {
                pairs.extend(near.into_iter().map(|j| [i, j]));
            }
        }
        Ok(pairs)
    }
}

/// Whether the boxes come within `margin` of each other along every axis.
/// Rounding the sums to the nearest float is monotone, so a box truly
/// within `margin` is never missed.
fn near(a: &Bounds3, b: &Bounds3, margin: f64) -> bool {
    a.min.cmple(b.max + margin).all() && b.min.cmple(a.max + margin).all()
}

#[cfg(test)]
mod tests;
