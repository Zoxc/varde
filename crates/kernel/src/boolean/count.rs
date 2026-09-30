//! The counting: the broad phase, the primitives each worked out once and
//! stored by pair, the crossings of edges with faces by the identity, and
//! winding numbers from the layer counts.

use super::exact::counted;
use super::input::{Input, Side};
use super::{BooleanError, Primitives, UP, parts};
use crate::budget::Work;
use crate::mesh::Bvh;
use crate::par::par_map;
use crate::patch::Bounds3;
use crate::{KernelError, Tolerance};

/// Crossing `i` of an edge of one operand through a face of the other,
/// counting along the edge: `x` is +1 where the edge, run in its own
/// direction, enters the other solid there, −1 where it leaves, and `t`
/// where along the edge it is (0 at its start, 1 at its end; a position
/// only, never a decision), and whether the search for it `solved` it
/// (found the edge meeting the face there) or only placed it for the
/// count. The record `(edge, face, i)` is the new vertex there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Crossing {
    pub(super) edge: u32,
    pub(super) face: u32,
    pub(super) i: u32,
    pub(super) x: i8,
    pub(super) t: f64,
    pub(super) solved: bool,
}

/// What the counting decides.
#[derive(Debug)]
pub(super) struct Counts {
    /// The pairs (triangle of `A`, triangle of `B`) whose boxes meet,
    /// sorted.
    pub(super) pairs: Vec<[u32; 2]>,
    /// Edges of `A` through faces of `B`, sorted by edge, face and index.
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

/// How many crossing searches run between spending their work.
const SEARCH_CHUNK: usize = 1024;

/// The work of a sign worked out exactly (see [`counted`]): a tied
/// predicate's expansions in powers of the perturbation take up to some
/// ten microseconds, where a unit is about half of one. A flat torus of
/// 9 216 patches against itself, every primitive a tie, took 4.2 s on one
/// thread for 1.4 million units, and one of 18 432 ran 16 s before the
/// budget stopped it.
const EXACT_WORK: usize = 20;

/// Values stored by the pair they were worked out for, sorted by it.
struct Table<V> {
    keys: Vec<[u32; 2]>,
    values: Vec<V>,
}

impl<V: Copy + Send> Table<V> {
    /// `f` of every key of `keys` (sorted and deduplicated here), through
    /// `par_map`, one unit of work each before, and [`EXACT_WORK`] for
    /// each sign `f` worked out exactly after each chunk of
    /// [`SEARCH_CHUNK`] keys, so a table of ties stops within a chunk of
    /// the budget.
    fn new(
        mut keys: Vec<[u32; 2]>,
        f: impl Fn(u32, u32) -> V + Sync + Send,
        work: &mut Work,
    ) -> Result<Table<V>, KernelError> {
        keys.sort_unstable();
        keys.dedup();
        work.spend(keys.len())?;
        let mut values = Vec::with_capacity(keys.len());
        for chunk in keys.chunks(SEARCH_CHUNK) {
            let found = par_map(chunk, |&[i, j]| counted(|| f(i, j)));
            let exact: usize = found.iter().map(|x| x.1).fold(0, usize::saturating_add);
            work.spend(exact.saturating_mul(EXACT_WORK))?;
            values.extend(found.into_iter().map(|x| x.0));
        }
        Ok(Table { keys, values })
    }

    fn get(&self, key: [u32; 2]) -> V {
        let i = self.keys.binary_search(&key).expect("a stored pair");
        self.values[i]
    }
}

impl Table<i8> {
    /// The layer counts stored for vertex `v`, summed: its winding
    /// number, where its ray's faces are among them.
    fn layers(&self, v: u32) -> i32 {
        let lo = self.keys.partition_point(|k| k[0] < v);
        let hi = self.keys.partition_point(|k| k[0] <= v);
        self.values[lo..hi].iter().map(|&s| i32::from(s)).sum()
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
    let (bvh_a, bvh_b) = (Bvh::new(a.boxes.clone()), Bvh::new(b.boxes.clone()));

    // Broad phase: triangle pairs whose boxes meet, or come within the
    // primitives' margin.
    let ids: Vec<u32> = (0..a.tris.len() as u32).collect();
    let pairs = bvh_b.hits_within(
        &ids,
        |p| a.boxes[p as usize],
        prims.margin(),
        |_, _| true,
        work,
    )?;
    let mut ef_a = Vec::with_capacity(3 * pairs.len());
    let mut ef_b = Vec::with_capacity(3 * pairs.len());
    for &[p, q] in &pairs {
        ef_a.extend(a.tri_edges[p as usize].map(|(e, _)| [e, q]));
        ef_b.extend(b.tri_edges[q as usize].map(|(g, _)| [g, p]));
    }
    for ef in [&mut ef_a, &mut ef_b] {
        ef.sort_unstable();
        ef.dedup();
    }

    // Layer counts: each part's first and last vertex against every face
    // its ray up may meet, for the winding numbers and their check, and
    // each end of a candidate edge against the face, for the crossings.
    let ([seeds_a, checks_a], [seeds_b, checks_b]) = (seeds(a), seeds(b));
    let rays = |seeds: &[u32], checks: &[u32]| {
        let mut rays = [seeds, checks].concat();
        rays.sort_unstable();
        rays.dedup();
        rays
    };
    let s02 = layers(
        Side::A,
        a,
        b,
        &bvh_b,
        &rays(&seeds_a, &checks_a),
        &ef_a,
        prims,
        tol,
        work,
    )?;
    let s20 = layers(
        Side::B,
        b,
        a,
        &bvh_a,
        &rays(&seeds_b, &checks_b),
        &ef_b,
        prims,
        tol,
        work,
    )?;

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
    let x12 = crossings(Side::A, &ef_a, a, b, &s02, prims, work, |e, h| {
        s11.get([e, h]).a_under
    })?;
    let x21 = crossings(Side::B, &ef_b, b, a, &s20, prims, work, |g, k| {
        s11.get([k, g]).b_under
    })?;

    let w03 = windings(a, &seeds_a, &x12, &s02);
    let w30 = windings(b, &seeds_b, &x21, &s20);
    for (input, x, w, checks, s) in [
        (a, &x12, &w03, &checks_a, &s02),
        (b, &x21, &w30, &checks_b, &s20),
    ] {
        agree(input, x, w)?;
        // A second ray in each part, which must find what the crossings
        // carried there from the first: a near tie decided one way at
        // the first vertex, with nothing crossing to show it, would put
        // the whole part on the wrong side. And the operands are solids,
        // whose winding numbers are 0 or 1 everywhere (`check` makes sure
        // of it), so any other number is decisions that don't fit.
        if checks.iter().any(|&v| w[v as usize] != s.layers(v))
            || w.iter().any(|&w| !(0..=1).contains(&w))
        {
            return Err(KernelError::Boolean(BooleanError::Inconsistent));
        }
    }
    Ok(Counts {
        pairs,
        x12,
        x21,
        w03,
        w30,
    })
}

/// The layer counts of `other`'s faces (`bvh` over their boxes) above
/// `input`'s vertices that the winding numbers and crossings read: every
/// face the ray up from each of `rays` may meet, and each end of the
/// candidate edges `ef` against their face.
#[allow(clippy::too_many_arguments)]
fn layers(
    side: Side,
    input: &Input,
    other: &Input,
    bvh: &Bvh,
    rays: &[u32],
    ef: &[[u32; 2]],
    prims: &impl Primitives,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Table<i8>, KernelError> {
    let mut keys = Vec::new();
    // Rays: from the given vertices along UP to the top of `other`'s
    // box.
    if let Some(top) = other.boxes.iter().map(|b| b.max.z).reduce(f64::max) {
        let ray = |v: u32| {
            let p = input.pos(v);
            let reach = ((top - p.z) / UP.z).max(0.0);
            Bounds3::point(p).include(p + UP * reach)
        };
        keys = bvh.hits_within(rays, ray, tol.resolution(), |_, _| true, work)?;
    }
    for &[e, f] in ef {
        keys.extend(input.edges[e as usize].map(|v| [v, f]));
    }
    Table::new(keys, |v, f| prims.s02(side, v, f), work)
}

/// The crossings of the candidate edges of `input` (on `side`) with
/// faces of `other`: how many by the identity, and where from `prims`
/// (which also looks for crossings in and out again where it may).
#[allow(clippy::too_many_arguments)]
fn crossings(
    side: Side,
    ef: &[[u32; 2]],
    input: &Input,
    other: &Input,
    s02: &Table<i8>,
    prims: &impl Primitives,
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
    // Where they are: the pairs crossed, and those the edge may cross in
    // and out of.
    let asked: Vec<([u32; 2], i32)> = ef
        .iter()
        .zip(x)
        .filter(|&(&[e, f], x)| x != 0 || prims.searches(side, e, f))
        .map(|(&ef, x)| (ef, x))
        .collect();
    // A chunk at a time: each search's least work spent before, the rest
    // after, so a round of searches that run long stops within a chunk
    // of the budget.
    let least = prims.search_work();
    let mut found = Vec::with_capacity(asked.len());
    for chunk in asked.chunks(SEARCH_CHUNK) {
        work.spend(chunk.len().saturating_mul(least))?;
        let here = par_map(chunk, |&([e, f], x)| {
            counted(|| prims.crossings(side, e, f, x))
        });
        let mut more = 0usize;
        for (result, exact) in here {
            let (crossings, cost) = result.map_err(KernelError::Boolean)?;
            more = more
                .saturating_add(cost.saturating_sub(least))
                .saturating_add(exact.saturating_mul(EXACT_WORK));
            found.push(crossings);
        }
        work.spend(more)?;
    }
    let mut out = Vec::new();
    for (&([edge, face], _), found) in asked.iter().zip(found) {
        for (i, (x, t, solved)) in found.into_iter().enumerate() {
            out.push(Crossing {
                edge,
                face,
                i: i as u32,
                x,
                t,
                solved,
            });
        }
    }
    Ok(out)
}

/// The lowest vertex of each connected part of `input`, in order, and
/// the highest.
fn seeds(input: &Input) -> [Vec<u32>; 2] {
    let part = parts(input.mesh.verts().len(), input.edges.iter().copied());
    let lowest: Vec<u32> = (0..part.len() as u32)
        .filter(|&v| part[v as usize] == v)
        .collect();
    let mut highest = vec![0u32; part.len()];
    for (v, &p) in part.iter().enumerate() {
        highest[p as usize] = v as u32;
    }
    let highest = lowest.iter().map(|&p| highest[p as usize]).collect();
    [lowest, highest]
}

/// Each vertex's winding number in the other solid: at each part's
/// first vertex (`seeds`) the sum of its layer counts, and from there
/// along the edges, each changing it by its crossings (the same sum, by
/// the counting identity).
fn windings(input: &Input, seeds: &[u32], x: &[Crossing], s02: &Table<i8>) -> Vec<i32> {
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
    for &seed in seeds {
        w[seed as usize] = s02.layers(seed);
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
