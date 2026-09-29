use crate::par::par_map;
use crate::patch::Bounds3;

/// How many boxes a leaf holds at most.
const LEAF: usize = 4;

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
        if self.nodes.is_empty() {
            return;
        }
        let from = out.len();
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
                out.extend(
                    items
                        .iter()
                        .filter(|&&i| near(&self.boxes[i as usize], query, margin)),
                );
            }
        }
        out[from..].sort_unstable();
    }

    /// Every pair `[i, j]`, `i < j`, of boxes within `margin` of each
    /// other along every axis, sorted. The queries run in parallel.
    pub fn self_pairs(&self, margin: f64) -> Vec<[u32; 2]> {
        let found = par_map(&self.items_in_order(), |&i| {
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

    fn items_in_order(&self) -> Vec<u32> {
        (0..self.boxes.len() as u32).collect()
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
