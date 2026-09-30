//! The counting: the broad phase, the primitives each worked out once and
//! stored by pair, the crossings of edges with faces by the identity, and
//! winding numbers from the layer counts.

use super::input::{Input, Side};
use super::{BooleanError, Cross11, Primitives, UP};
use crate::budget::Work;
use crate::mesh::Bvh;
use crate::par::par_map;
use crate::patch::Bounds3;
use crate::{KernelError, Tolerance};

/// An edge of one operand crossing a face of the other: `x` is +1 where
/// the edge, run in its own direction, enters the other solid there, −1
/// where it leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Crossing {
    pub(super) edge: u32,
    pub(super) face: u32,
    pub(super) x: i8,
}

/// What the counting decides.
#[derive(Debug)]
pub(super) struct Counts {
    /// Edges of `A` through faces of `B`, sorted by edge then face.
    pub(super) x12: Vec<Crossing>,
    /// Edges of `B` through faces of `A`, likewise.
    pub(super) x21: Vec<Crossing>,
    /// Each vertex of `A`'s winding number in `B`: 1 inside, 0 outside.
    pub(super) w03: Vec<i32>,
    /// Each vertex of `B`'s in `A`.
    pub(super) w30: Vec<i32>,
}

impl Counts {
    /// Whether the operands meet anywhere: an edge through a face, or a
    /// vertex inside the other.
    pub(super) fn meet(&self) -> bool {
        !self.x12.is_empty()
            || !self.x21.is_empty()
            || self.w03.iter().chain(&self.w30).any(|&w| w != 0)
    }
}

/// Values stored by the pair they were worked out for, sorted by it.
struct Table<V> {
    keys: Vec<[u32; 2]>,
    values: Vec<V>,
}

impl<V: Copy + Send> Table<V> {
    /// `f` of every key of `keys` (sorted and deduplicated here), through
    /// `par_map`, one unit of work each.
    fn new(
        mut keys: Vec<[u32; 2]>,
        f: impl Fn(u32, u32) -> V + Sync + Send,
        work: &mut Work,
    ) -> Result<Table<V>, KernelError> {
        keys.sort_unstable();
        keys.dedup();
        work.spend(keys.len())?;
        let values = par_map(&keys, |&[i, j]| f(i, j));
        Ok(Table { keys, values })
    }

    fn get(&self, key: [u32; 2]) -> V {
        let i = self.keys.binary_search(&key).expect("a stored pair");
        self.values[i]
    }
}

/// Counts `a` against `b` with `prims`.
pub(super) fn count(
    a: &Input,
    b: &Input,
    prims: &impl Primitives,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Counts, KernelError> {
    let na = a.tris.len() as u32;
    let bvh = Bvh::new(a.boxes.iter().chain(&b.boxes).copied().collect());

    // Broad phase: triangle pairs whose boxes meet.
    let ids: Vec<u32> = (0..na).collect();
    let pairs = bvh.pairs_within(&ids, 0.0, |_, j| j >= na, work)?;
    let mut ef_a = Vec::with_capacity(3 * pairs.len());
    let mut ef_b = Vec::with_capacity(3 * pairs.len());
    for &[p, q] in &pairs {
        let q = q - na;
        ef_a.extend(a.tri_edges[p as usize].map(|(e, _)| [e, q]));
        ef_b.extend(b.tri_edges[q as usize].map(|(g, _)| [g, p]));
    }
    for ef in [&mut ef_a, &mut ef_b] {
        ef.sort_unstable();
        ef.dedup();
    }

    // Layer counts: each part's first vertex against every face its ray up
    // may meet, for the winding numbers, and each end of a candidate edge
    // against the face, for the crossings.
    let s02 = layers(Side::A, a, b, &bvh, &ef_a, prims, tol, work)?;
    let s20 = layers(Side::B, b, a, &bvh, &ef_b, prims, tol, work)?;

    // Edge against edge: each candidate edge against the other's face's
    // edges, stored as (edge of A, edge of B).
    let mut keys = Vec::new();
    for &[e, q] in &ef_a {
        keys.extend(b.tri_edges[q as usize].map(|(h, _)| [e, h]));
    }
    for &[g, p] in &ef_b {
        keys.extend(a.tri_edges[p as usize].map(|(k, _)| [k, g]));
    }
    let s11 = Table::new(keys, |e, g| prims.s11(e, g), work)?;

    // x12(e, f) = s02(end, f) − s02(start, f) − Σ over f's edges h (as f
    // runs them) of the times e's shadow crosses under h.
    let x12 = crossings(&ef_a, a, b, &s02, work, |e, h| match s11.get([e, h]) {
        Some(Cross11 { sigma, a_above }) if !a_above => sigma,
        _ => 0,
    })?;
    let x21 = crossings(&ef_b, b, a, &s20, work, |g, k| match s11.get([k, g]) {
        Some(Cross11 { sigma, a_above }) if a_above => -sigma,
        _ => 0,
    })?;

    let w03 = windings(a, &x12, &s02);
    let w30 = windings(b, &x21, &s20);
    for (input, x, w) in [(a, &x12, &w03), (b, &x21, &w30)] {
        agree(input, x, w)?;
        if w.iter().any(|&w| !(0..=1).contains(&w)) {
            return Err(KernelError::Boolean(BooleanError::InsideOut));
        }
    }
    Ok(Counts { x12, x21, w03, w30 })
}

/// The layer counts of `other`'s faces above `input`'s vertices that the
/// winding numbers and crossings read.
#[allow(clippy::too_many_arguments)]
fn layers(
    side: Side,
    input: &Input,
    other: &Input,
    bvh: &Bvh,
    ef: &[[u32; 2]],
    prims: &impl Primitives,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Table<i8>, KernelError> {
    let nb = other.tris.len() as u32;
    let mut keys = Vec::new();
    // Rays: from the first vertex of each connected part along UP to the
    // top of `other`'s box.
    if let Some(top) = other.boxes.iter().map(|b| b.max.z).reduce(f64::max) {
        let verts = seeds(input);
        let ray = |v: u32| {
            let p = input.pos(v);
            let reach = ((top - p.z) / UP.z).max(0.0);
            Bounds3::point(p).include(p + UP * reach)
        };
        // `other`'s faces in the BVH: after `A`'s for `B`'s, first for
        // `A`'s.
        let (lo, hi) = match side {
            Side::A => (bvh.len() as u32 - nb, bvh.len() as u32),
            Side::B => (0, nb),
        };
        let hits = bvh.hits_within(
            &verts,
            ray,
            tol.resolution(),
            |_, j| (lo..hi).contains(&j),
            work,
        )?;
        keys.extend(hits.into_iter().map(|[v, j]| [v, j - lo]));
    }
    for &[e, f] in ef {
        keys.extend(input.edges[e as usize].map(|v| [v, f]));
    }
    Table::new(keys, |v, f| prims.s02(side, v, f), work)
}

/// The crossings of the candidate edges of `input` with faces of
/// `other`, by the identity, keeping those that aren't zero.
fn crossings(
    ef: &[[u32; 2]],
    input: &Input,
    other: &Input,
    s02: &Table<i8>,
    work: &mut Work,
    shadow: impl Fn(u32, u32) -> i8 + Sync,
) -> Result<Vec<Crossing>, KernelError> {
    work.spend(ef.len())?;
    let x = par_map(ef, |&[e, f]| {
        let [start, end] = input.edges[e as usize];
        let mut x = i32::from(s02.get([end, f])) - i32::from(s02.get([start, f]));
        for (h, forward) in other.tri_edges[f as usize] {
            let s = i32::from(shadow(e, h));
            x -= if forward { s } else { -s };
        }
        x
    });
    let mut out = Vec::new();
    for (&[edge, face], x) in ef.iter().zip(x) {
        match x {
            0 => {}
            -1 | 1 => out.push(Crossing {
                edge,
                face,
                x: x as i8,
            }),
            // A straight edge meets a flat face once at most.
            _ => return Err(KernelError::Boolean(BooleanError::Inconsistent)),
        }
    }
    Ok(out)
}

/// The lowest vertex of each connected part of `input`, in order.
fn seeds(input: &Input) -> Vec<u32> {
    let mut part: Vec<u32> = (0..input.mesh.verts().len() as u32).collect();
    fn root(part: &mut [u32], mut v: u32) -> u32 {
        while part[v as usize] != v {
            part[v as usize] = part[part[v as usize] as usize];
            v = part[v as usize];
        }
        v
    }
    for &[s, e] in &input.edges {
        let (x, y) = (root(&mut part, s), root(&mut part, e));
        part[x.max(y) as usize] = x.min(y);
    }
    (0..part.len() as u32)
        .filter(|&v| root(&mut part, v) == v)
        .collect()
}

/// Each vertex's winding number in the other solid: at each part's
/// first vertex the sum of its layer counts, and from there along the
/// edges, each changing it by its crossings (the same sum, by the
/// counting identity).
fn windings(input: &Input, x: &[Crossing], s02: &Table<i8>) -> Vec<i32> {
    let nv = input.mesh.verts().len();
    let mut change = vec![0i32; input.edges.len()];
    for c in x {
        change[c.edge as usize] += i32::from(c.x);
    }
    // Each vertex's edges, by edge index.
    let mut start = vec![0usize; nv + 1];
    for &[s, e] in &input.edges {
        start[s as usize + 1] += 1;
        start[e as usize + 1] += 1;
    }
    for v in 0..nv {
        start[v + 1] += start[v];
    }
    let mut fill = start.clone();
    let mut edges = vec![0u32; 2 * input.edges.len()];
    for (e, &[s, t]) in input.edges.iter().enumerate() {
        for v in [s, t] {
            edges[fill[v as usize]] = e as u32;
            fill[v as usize] += 1;
        }
    }
    let mut w = vec![0i32; nv];
    let mut seen = vec![false; nv];
    let mut queue = std::collections::VecDeque::new();
    for seed in seeds(input) {
        let lo = s02.keys.partition_point(|k| k[0] < seed);
        let hi = s02.keys.partition_point(|k| k[0] <= seed);
        w[seed as usize] = s02.values[lo..hi].iter().map(|&s| i32::from(s)).sum();
        seen[seed as usize] = true;
        queue.push_back(seed);
        while let Some(v) = queue.pop_front() {
            for &e in &edges[start[v as usize]..start[v as usize + 1]] {
                let [s, t] = input.edges[e as usize];
                let (other, dw) = if s == v {
                    (t, change[e as usize])
                } else {
                    (s, -change[e as usize])
                };
                if !seen[other as usize] {
                    seen[other as usize] = true;
                    w[other as usize] = w[v as usize] + dw;
                    queue.push_back(other);
                }
            }
        }
    }
    w
}

/// Checks that along every edge the winding number changes by its
/// crossings: it does whenever the primitives are consistent.
fn agree(input: &Input, x: &[Crossing], w: &[i32]) -> Result<(), KernelError> {
    let mut change = vec![0i32; input.edges.len()];
    for c in x {
        change[c.edge as usize] += i32::from(c.x);
    }
    for (e, &[start, end]) in input.edges.iter().enumerate() {
        if w[start as usize] + change[e] != w[end as usize] {
            return Err(KernelError::Boolean(BooleanError::Inconsistent));
        }
    }
    Ok(())
}
