//! Face arrangements in a face's UV, on straight spans: the half-edge
//! structure, the rotation order at each vertex, the components and their
//! face cycles, the nesting of free components by a containment
//! certificate, and the regions with their outer and inner loops and
//! windings.
//!
//! **Input** ([`arrange`]). Vertices are UV points; edges are straight
//! spans between two distinct vertices, each with a winding step
//! ([`Edge::wind`]): the winding number left of the edge, in its
//! direction, minus the one right of it. Topology comes from the records
//! that made the input, never from here: the vertices are the records and
//! the old vertices, and edges meet only at shared end vertices. Nothing
//! here intersects edges or merges vertices by position: two vertices are
//! one only if they are one index. A vertex no edge uses is not part of
//! the arrangement. That edges are disjoint away from shared vertices is
//! certified elsewhere; what this module meets of a breach it refuses
//! ([`ArrangeError`]), never repairs.
//!
//! **Rotation order.** The edges leaving a vertex `v` are ordered
//! counter-clockwise from the direction `+u`, exactly on the stored UVs:
//! first by half-plane, compared on the coordinates (the far end above
//! `v`'s line, or on it to the right of `v`: the upper half; else the
//! lower), then within a half by the sign of `orient(v, a, b)`, an exact
//! flat predicate. No tolerance, no convergence. Two edges leaving `v` in
//! one direction (the orientation zero within a half) lie on one line:
//! coincident spans, which only a record may identify (they reach here as
//! one edge), so they are refused ([`ArrangeError::Coincident`]).
//!
//! **Collinear overlaps.** Two edges sharing no vertex that run along
//! one line over a positive length are coincident spans as well, met
//! nowhere in the rotation order: a missed record, refused
//! ([`ArrangeError::Overlap`]), never an arrangement. Each pair of edges
//! whose closed UV boxes meet, found by a sweep along `u`, is tested
//! exactly: both ends of one on the other's line (`orient` zero), and
//! their ranges along the line (along `u`, or `v` for a line of constant
//! `u`) overlapping in more than a point. Pairs sharing a vertex are
//! the rotation order's: overlapping, they leave it in one direction.
//! Edges touching at a point, or crossing, are not this test's (their
//! disjointness is certified elsewhere).
//!
//! **Face cycles.** Edge `e` has the half-edges `2e`, from `ends[0]` to
//! `ends[1]`, and `2e + 1` back; each half-edge bounds the face on its
//! left. The half-edge after `h` leaves `h`'s end vertex next clockwise
//! from `h`'s twin, so bounded face cycles run counter-clockwise. A
//! **component** is a connected part of the edge graph; it has exactly
//! one unbounded face cycle, running clockwise round it (a tree has only
//! that one): the cycle through the wedge at the component's least vertex
//! `q` that holds the direction `−u` (no vertex of the component lies at
//! a lesser `u`, so the ray from `q` along `−u` stays in the unbounded
//! face).
//!
//! **Order.** Vertices are ordered by `u`, then `v`, each compared by
//! [`f64::total_cmp`], then by index (only where the positions are the
//! same bits). Components are numbered in the order of their least
//! vertex; face cycles as found walking each component's vertices in
//! that order and each vertex's half-edges in rotation order; regions
//! follow their outer cycles. So the numbering depends on ids only at
//! exact ties.
//!
//! **Nesting** ([`LoopIn`]). Each component `k` is tested against every
//! bounded face cycle `l` of another component whose UV box (closed, of
//! its vertices) holds `k`'s least vertex `q`, so both directions are
//! asked (a cycle round an old hole as well as an island in an old loop).
//! The test counts the signed crossings of `l` by the half-open ray from
//! `q` along `+u`: a span of `l` counts where exactly one of its ends lies
//! strictly above `q`'s line (so a vertex of `l` on the ray counts for the
//! span above it, a span lying on the ray counts 0 and a touch at a vertex
//! counts 0), if it crosses the line right of `q`, decided by comparing
//! coordinates where they settle it and by `orient` exactly otherwise;
//! `+1` upwards. `k` lies inside `l` when the count is 1, outside when 0;
//! any other count means crossing edges, refused. Components are disjoint,
//! so the count is a fact, independent of the ray; `q` on `l` breaks that
//! and is refused ([`ArrangeError::Touch`]). The certificate is exact on
//! the stored UVs and never perturbed. A component's unbounded cycle never
//! contains anything. `k`'s **container** is the innermost cycle
//! containing it: the one whose component is itself inside every other
//! cycle containing `k`. Within one component at most one bounded cycle
//! may contain `k` (its bounded cycles bound disjoint faces).
//!
//! **Regions** ([`Region`]). Region 0 is the unbounded one: winding 0, its
//! holes the unbounded cycles of the components no cycle contains. Then
//! one region per bounded face cycle, in cycle order: the cycle its outer
//! loop, its holes the unbounded cycles of the components it is the
//! container of. **Windings** follow from the edges' winding steps
//! alone: a component's unbounded face takes the winding of the region
//! holding it, and crossing half-edge `h` from its right face to its left
//! one adds `h`'s step. Steps that don't add up the same along every path
//! (around a vertex or along a dangling edge) are refused
//! ([`ArrangeError::Winding`]).
//!
//! **Work.** Five passes on the operation's budget: the rotation order
//! (an item per vertex), the collinear overlaps (one item), the face
//! cycles (one item), the containment
//! tests (an item per pair asked, in `(component, cycle)` order) and the
//! regions (one item). Each orientation is charged at the 2D
//! orientation's row of the unit table for the stages it ran; each vertex sorted, edge, half-edge walked or
//! counted, pair of edges with meeting boxes and certificate read is
//! charged a unit, and the containment boxes and the sweep's boxes a
//! unit per 64 tested.

use std::cmp::Ordering;
use std::collections::VecDeque;

use varde_decide::num::exact::{MAX_INPUT, Pred, value_sign};
use varde_decide::num::schedule::{UNIT_PARTS, UNITS};

use super::brep_min::planar::Orient;
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;

pub use varde_decide::ids::NONE;

/// An edge of the input: the straight span from `ends[0]` to `ends[1]`
/// (vertex indices).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    /// Its start and end vertex.
    pub ends: [u32; 2],
    /// The winding number left of it minus the one right of it.
    pub wind: i32,
}

/// One containment certificate (module docs): whether component
/// `component` lies inside the bounded face cycle `cycle` of another
/// component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopIn {
    /// The component tested, by its least vertex.
    pub component: u32,
    /// The bounded face cycle tested against.
    pub cycle: u32,
    /// Whether the component lies inside the cycle.
    pub inside: bool,
}

/// A region of the arrangement (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    /// Its outer loop, a bounded face cycle; [`NONE`] for the unbounded
    /// region.
    pub outer: u32,
    /// Its inner loops: the unbounded face cycles of the components it
    /// holds directly, in component order.
    pub holes: Vec<u32>,
    /// Its winding number.
    pub winding: i64,
}

/// Why the arrangement can't be built: input it doesn't take, a breach of
/// its input contract (module docs), or running out of budget.
#[derive(Clone, Debug, PartialEq)]
pub enum ArrangeError {
    /// More vertices or edges than `u32` half-edge ids hold.
    TooMany,
    /// The edge names a vertex not there.
    Index(u32),
    /// The edge's two ends are one vertex.
    SelfLoop(u32),
    /// The vertex has a coordinate that isn't finite or lies beyond the
    /// exact predicates' range ([`MAX_INPUT`]).
    Coord(u32),
    /// The edge's two ends lie at one point.
    ZeroLength(u32),
    /// Two edges (ascending) leave the vertex in one direction.
    Coincident {
        /// The vertex.
        vertex: u32,
        /// The edges.
        edges: [u32; 2],
    },
    /// Two edges (ascending) sharing no vertex run along one line over a
    /// positive length: coincident spans no record linked.
    Overlap {
        /// The edges.
        edges: [u32; 2],
    },
    /// The component's least vertex lies on the bounded face cycle of
    /// another component.
    Touch {
        /// The component.
        component: u32,
        /// The cycle.
        cycle: u32,
    },
    /// The cycle winds round the component's least vertex neither 0 nor 1
    /// times: edges cross.
    Crossing {
        /// The component.
        component: u32,
        /// The cycle.
        cycle: u32,
    },
    /// The cycles containing the component have no innermost one, or two
    /// bounded cycles of one component contain it.
    Nesting(u32),
    /// The winding steps don't add up across the edge.
    Winding(u32),
    /// A winding number leaves `i64`.
    WindingRange,
    /// The budget ran out, or a fixed limit was reached
    /// ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// A face's arrangement (module docs): half-edges, rotation order, face
/// cycles, components, containment certificates and regions, as columns
/// indexed by `u32` ids.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrangement {
    /// Per half-edge: its start vertex.
    origin: Vec<u32>,
    /// Per half-edge: the next one round its face cycle.
    next: Vec<u32>,
    /// Per half-edge: its face cycle.
    cycle_of: Vec<u32>,
    /// Per vertex, the range of `rot` holding its outgoing half-edges
    /// (`rot_start[v]..rot_start[v + 1]`).
    rot_start: Vec<u32>,
    /// The outgoing half-edges of each vertex in rotation order.
    rot: Vec<u32>,
    /// Per cycle: its first half-edge (where it was found).
    cycle_first: Vec<u32>,
    /// Per cycle: its component.
    cycle_comp: Vec<u32>,
    /// Per cycle: the region left of it.
    cycle_region: Vec<u32>,
    /// Per component: its least vertex.
    comp_least: Vec<u32>,
    /// Per component: its unbounded face cycle.
    comp_outer: Vec<u32>,
    /// Per component: the cycle containing it innermost, or [`NONE`].
    comp_container: Vec<u32>,
    /// The certificates, in `(component, cycle)` order.
    loop_in: Vec<LoopIn>,
    /// The regions, the unbounded one first.
    regions: Vec<Region>,
}

/// The arrangement of the edges `edges` on the UV points `verts` (module
/// docs), charged to `work`; or the first input it doesn't take (vertices
/// first, then edges, in index order), else the first refusal in pass
/// order.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the face arrangements of the operations call it as they land"
    )
)]
pub(crate) fn arrange(
    verts: &[[f64; 2]],
    edges: &[Edge],
    work: &mut Work,
) -> Result<Arrangement, ArrangeError> {
    validate(verts, edges)?;
    let n = verts.len();
    let input = Input { verts, edges };
    // Outgoing half-edges per vertex, in half-edge order: the counts fit,
    // as the half-edges are fewer than `NONE`.
    let mut rot_start = vec![0u32; n + 1];
    for e in edges {
        for &v in &e.ends {
            rot_start[v as usize + 1] += 1;
        }
    }
    for v in 0..n {
        rot_start[v + 1] += rot_start[v];
    }
    let mut fill = rot_start.clone();
    let mut out = vec![NONE; 2 * edges.len()];
    for h in 0..out.len() as u32 {
        let v = input.origin(h) as usize;
        out[fill[v] as usize] = h;
        fill[v] += 1;
    }
    let vs: Vec<u32> = (0..n as u32).collect();
    let rotations = run(work, "rotation", &vs, |&v, m| {
        let range = rot_start[v as usize] as usize..rot_start[v as usize + 1] as usize;
        rotate(&input, v, &out[range], m)
    })?;
    let rot: Vec<u32> = rotations.into_iter().flatten().collect();
    let mut rot_pos = vec![0u32; rot.len()];
    for v in 0..n {
        for (i, &h) in rot[rot_start[v] as usize..rot_start[v + 1] as usize]
            .iter()
            .enumerate()
        {
            rot_pos[h as usize] = i as u32;
        }
    }
    run(work, "collinear overlaps", &[()], |_, m| {
        overlaps(&input, m)
    })?;
    let star = Star {
        rot_start: &rot_start,
        rot: &rot,
        rot_pos: &rot_pos,
    };
    // One result per item.
    let topo = run(work, "face cycles", &[()], |_, m| cycles(&input, &star, m))?.swap_remove(0);
    let tests = run(work, "containment", &topo.asked, |&(k, l), m| {
        contains(&input, &topo, k, l, m)
    })?;
    let loop_in: Vec<LoopIn> = topo
        .asked
        .iter()
        .zip(tests)
        .map(|(&(component, cycle), inside)| LoopIn {
            component,
            cycle,
            inside,
        })
        .collect();
    let nest = run(work, "regions", &[()], |_, m| {
        regions(&input, &topo, &loop_in, m)
    })?
    .swap_remove(0);
    Ok(Arrangement {
        origin: (0..out.len() as u32).map(|h| input.origin(h)).collect(),
        next: topo.next,
        cycle_of: topo.cycle_of,
        rot_start,
        rot,
        cycle_first: topo.cycle_first,
        cycle_comp: topo.cycle_comp,
        cycle_region: nest.cycle_region,
        comp_least: topo.comp_least,
        comp_outer: topo.comp_outer,
        comp_container: nest.comp_container,
        loop_in,
        regions: nest.regions,
    })
}

impl Arrangement {
    /// The number of half-edges, twice the edges'; their ids are `0..n`.
    pub fn half_edge_count(&self) -> usize {
        self.origin.len()
    }

    /// The edge of half-edge `h`.
    pub fn edge(&self, h: u32) -> u32 {
        h / 2
    }

    /// Whether half-edge `h` runs its edge's way.
    pub fn forward(&self, h: u32) -> bool {
        h % 2 == 0
    }

    /// The other half-edge of `h`'s edge.
    pub fn twin(&self, h: u32) -> u32 {
        h ^ 1
    }

    /// The vertex half-edge `h` leaves.
    pub fn origin(&self, h: u32) -> u32 {
        self.origin[h as usize]
    }

    /// The vertex half-edge `h` reaches.
    pub fn dest(&self, h: u32) -> u32 {
        self.origin[(h ^ 1) as usize]
    }

    /// The half-edge after `h` round its face cycle.
    pub fn next(&self, h: u32) -> u32 {
        self.next[h as usize]
    }

    /// The half-edges leaving vertex `v` in rotation order, counter-
    /// clockwise from `+u`; none for a vertex no edge uses.
    pub fn rotation(&self, v: u32) -> &[u32] {
        let v = v as usize;
        &self.rot[self.rot_start[v] as usize..self.rot_start[v + 1] as usize]
    }

    /// The face cycle of half-edge `h`.
    pub fn cycle_of(&self, h: u32) -> u32 {
        self.cycle_of[h as usize]
    }

    /// The number of face cycles; their ids are `0..n`.
    pub fn cycle_count(&self) -> usize {
        self.cycle_first.len()
    }

    /// The half-edges of cycle `c` in order, from the one it was found at.
    pub fn cycle(&self, c: u32) -> impl Iterator<Item = u32> + '_ {
        let first = self.cycle_first[c as usize];
        std::iter::successors(Some(first), move |&h| {
            let n = self.next[h as usize];
            (n != first).then_some(n)
        })
    }

    /// The component of cycle `c`.
    pub fn cycle_component(&self, c: u32) -> u32 {
        self.cycle_comp[c as usize]
    }

    /// Whether cycle `c` is a bounded face cycle (not its component's
    /// unbounded one).
    pub fn is_bounded(&self, c: u32) -> bool {
        self.comp_outer[self.cycle_comp[c as usize] as usize] != c
    }

    /// The region left of cycle `c`: the one it is the outer loop of, or,
    /// for an unbounded cycle, the one it is a hole of.
    pub fn cycle_region(&self, c: u32) -> u32 {
        self.cycle_region[c as usize]
    }

    /// The number of components; their ids are `0..n`.
    pub fn component_count(&self) -> usize {
        self.comp_least.len()
    }

    /// Component `k`'s least vertex.
    pub fn least_vertex(&self, k: u32) -> u32 {
        self.comp_least[k as usize]
    }

    /// Component `k`'s unbounded face cycle.
    pub fn outer_cycle(&self, k: u32) -> u32 {
        self.comp_outer[k as usize]
    }

    /// The cycle containing component `k` innermost, or `None` if no cycle
    /// contains it.
    pub fn container(&self, k: u32) -> Option<u32> {
        let c = self.comp_container[k as usize];
        (c != NONE).then_some(c)
    }

    /// The containment certificates, in `(component, cycle)` order.
    pub fn loop_in(&self) -> &[LoopIn] {
        &self.loop_in
    }

    /// The regions, the unbounded one first, then one per bounded face
    /// cycle in cycle order.
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }
}

/// The input, read by half-edge.
struct Input<'a> {
    verts: &'a [[f64; 2]],
    edges: &'a [Edge],
}

impl Input<'_> {
    fn origin(&self, h: u32) -> u32 {
        self.edges[(h / 2) as usize].ends[(h % 2) as usize]
    }

    fn dest(&self, h: u32) -> u32 {
        self.origin(h ^ 1)
    }

    fn pos(&self, v: u32) -> [f64; 2] {
        self.verts[v as usize]
    }

    /// The winding step of half-edge `h`: its left face's winding minus
    /// its right face's.
    fn wind(&self, h: u32) -> i64 {
        let w = i64::from(self.edges[(h / 2) as usize].wind);
        if h % 2 == 0 { w } else { -w }
    }
}

/// The rotation order, by vertex and by half-edge.
struct Star<'a> {
    rot_start: &'a [u32],
    rot: &'a [u32],
    /// Per half-edge: its place in its origin's rotation.
    rot_pos: &'a [u32],
}

impl Star<'_> {
    fn at(&self, v: u32) -> &[u32] {
        let v = v as usize;
        &self.rot[self.rot_start[v] as usize..self.rot_start[v + 1] as usize]
    }

    /// The half-edge after `h`: leaving `h`'s end next clockwise from
    /// `h`'s twin.
    fn next(&self, input: &Input<'_>, h: u32) -> u32 {
        let around = self.at(input.dest(h));
        let i = self.rot_pos[(h ^ 1) as usize] as usize;
        around[(i + around.len() - 1) % around.len()]
    }
}

/// The face cycles' pass: components, cycles and the pairs to test.
struct Topo {
    next: Vec<u32>,
    cycle_of: Vec<u32>,
    cycle_first: Vec<u32>,
    cycle_comp: Vec<u32>,
    comp_least: Vec<u32>,
    comp_outer: Vec<u32>,
    /// The `(component, cycle)` pairs to test, in that order.
    asked: Vec<(u32, u32)>,
}

/// The regions' pass.
struct Nest {
    cycle_region: Vec<u32>,
    comp_container: Vec<u32>,
    regions: Vec<Region>,
}

/// An item's ledger, with the parts of a unit not yet charged.
struct Meter<'l, 'p> {
    ledger: &'l mut Ledger<'p>,
    parts: u64,
}

type Res<T> = Result<T, Stop<ArrangeError>>;

impl Meter<'_, '_> {
    fn units(&mut self, n: u64) -> Res<()> {
        self.ledger.charge(Step::Work, n)?;
        Ok(())
    }

    fn parts(&mut self, p: u64) -> Res<()> {
        self.parts = self.parts.saturating_add(p);
        let whole = self.parts / UNIT_PARTS;
        self.parts %= UNIT_PARTS;
        if whole > 0 {
            self.units(whole)?;
        }
        Ok(())
    }

    /// The exact sign of `orient(p[0], p[1], p[2])`, `p[0]` vertex `at`:
    /// its filter charged before, any further stage after.
    fn orient(&mut self, p: [[f64; 2]; 3], at: u32) -> Res<Ordering> {
        let pred = Orient(p);
        let filter = UNITS.record.of(pred.kind()).filter;
        self.parts(filter)?;
        let (s, w) = value_sign(&pred);
        self.parts(w.parts(&UNITS.record).saturating_sub(filter))?;
        // Validated coordinates are finite.
        s.map_err(|_| Stop::Failed(ArrangeError::Coord(at)))
    }
}

/// Runs a pass of `f` over `items` at rung 0.
fn run<T, R, F>(
    work: &mut Work,
    stage: &'static str,
    items: &[T],
    f: F,
) -> Result<Vec<R>, ArrangeError>
where
    T: Sync,
    R: Send,
    F: Fn(&T, &mut Meter<'_, '_>) -> Res<R> + Sync + Send,
{
    work.pass(Pass { stage, rung: 0 }, items, |item, ledger| {
        f(item, &mut Meter { ledger, parts: 0 })
    })
    .map_err(|halt| match halt {
        Halt::Failed { error, .. } => error,
        Halt::OutOfBudget(e) => ArrangeError::OutOfBudget(e),
    })
}

fn validate(verts: &[[f64; 2]], edges: &[Edge]) -> Result<(), ArrangeError> {
    if verts.len() >= NONE as usize || edges.len() > (NONE as usize - 1) / 2 {
        return Err(ArrangeError::TooMany);
    }
    for (i, p) in verts.iter().enumerate() {
        if !p.iter().all(|x| x.is_finite() && x.abs() <= MAX_INPUT) {
            return Err(ArrangeError::Coord(i as u32));
        }
    }
    for (i, e) in edges.iter().enumerate() {
        let i = i as u32;
        let [a, b] = e.ends;
        if a as usize >= verts.len() || b as usize >= verts.len() {
            return Err(ArrangeError::Index(i));
        }
        if a == b {
            return Err(ArrangeError::SelfLoop(i));
        }
        let (p, q) = (verts[a as usize], verts[b as usize]);
        if p[0] == q[0] && p[1] == q[1] {
            return Err(ArrangeError::ZeroLength(i));
        }
    }
    Ok(())
}

/// Whether the far end `d` of an edge leaving `p` lies in the upper half
/// (module docs): above `p`'s line, or on it to the right.
fn upper(p: [f64; 2], d: [f64; 2]) -> bool {
    d[1] > p[1] || (d[1] == p[1] && d[0] > p[0])
}

/// The half-edges `out` leaving `v`, in rotation order, by binary
/// insertion on the exact order (module docs).
fn rotate(input: &Input<'_>, v: u32, out: &[u32], m: &mut Meter<'_, '_>) -> Res<Vec<u32>> {
    m.units(1)?;
    let p = input.pos(v);
    let mut sorted: Vec<u32> = Vec::with_capacity(out.len());
    for &h in out {
        let (mut lo, mut hi) = (0, sorted.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let s = sorted[mid];
            let (ds, dh) = (input.pos(input.dest(s)), input.pos(input.dest(h)));
            let ord = match (upper(p, ds), upper(p, dh)) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                // `h` left of `v → s`: after `s` counter-clockwise.
                _ => m.orient([p, ds, dh], v)?.reverse(),
            };
            match ord {
                Ordering::Less => lo = mid + 1,
                Ordering::Greater => hi = mid,
                Ordering::Equal => {
                    let (a, b) = (s / 2, h / 2);
                    let edges = if a < b { [a, b] } else { [b, a] };
                    return Err(Stop::Failed(ArrangeError::Coincident { vertex: v, edges }));
                }
            }
        }
        sorted.insert(lo, h);
    }
    Ok(sorted)
}

/// The closed UV box of edge `e`, `[lo, hi]` per axis.
fn edge_box(input: &Input<'_>, e: u32) -> [[f64; 2]; 2] {
    let [a, b] = input.edges[e as usize].ends.map(|v| input.pos(v));
    let mut lo = a;
    let mut hi = a;
    for k in 0..2 {
        if b[k] < lo[k] {
            lo[k] = b[k];
        }
        if b[k] > hi[k] {
            hi[k] = b[k];
        }
    }
    [lo, hi]
}

/// The collinear overlaps' pass (module docs): the first pair of edges
/// sharing no vertex that overlap along one line, in sweep order (by
/// their boxes' low `u`, then index), refused.
fn overlaps(input: &Input<'_>, m: &mut Meter<'_, '_>) -> Res<()> {
    let n = input.edges.len();
    m.units(n as u64)?;
    let boxes: Vec<[[f64; 2]; 2]> = (0..n as u32).map(|e| edge_box(input, e)).collect();
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by(|&x, &y| {
        boxes[x as usize][0][0]
            .total_cmp(&boxes[y as usize][0][0])
            .then(x.cmp(&y))
    });
    // The edges swept whose boxes reach the current one's low `u`.
    let mut active: Vec<u32> = Vec::new();
    for &e in &order {
        let be = boxes[e as usize];
        m.units(1 + active.len() as u64 / 64)?;
        active.retain(|&f| boxes[f as usize][1][0] >= be[0][0]);
        for &f in &active {
            let bf = boxes[f as usize];
            let lo = [0, 1].map(|k| {
                if be[0][k] > bf[0][k] {
                    be[0][k]
                } else {
                    bf[0][k]
                }
            });
            let hi = [0, 1].map(|k| {
                if be[1][k] < bf[1][k] {
                    be[1][k]
                } else {
                    bf[1][k]
                }
            });
            // Boxes apart in `v`, or meeting in one point only.
            if lo[1] > hi[1] || (lo[0] == hi[0] && lo[1] == hi[1]) {
                continue;
            }
            m.units(1)?;
            let (ee, ef) = (input.edges[e as usize].ends, input.edges[f as usize].ends);
            if ee.iter().any(|v| ef.contains(v)) {
                continue;
            }
            if overlap(input, ee, ef, m)? {
                let edges = if e < f { [e, f] } else { [f, e] };
                return Err(Stop::Failed(ArrangeError::Overlap { edges }));
            }
        }
        active.push(e);
    }
    Ok(())
}

/// Whether the spans `x` and `y` (vertex pairs) lie on one line and
/// overlap along it over a positive length, exactly.
fn overlap(input: &Input<'_>, x: [u32; 2], y: [u32; 2], m: &mut Meter<'_, '_>) -> Res<bool> {
    let [a, b] = x.map(|v| input.pos(v));
    let [c, d] = y.map(|v| input.pos(v));
    for (p, v) in [(c, y[0]), (d, y[1])] {
        if m.orient([a, b, p], v)? != Ordering::Equal {
            return Ok(false);
        }
    }
    // On one line, the order along it is the order along `u`, or along
    // `v` where the line has one `u`.
    let k = usize::from(a[0] == b[0]);
    let range = |p: [f64; 2], q: [f64; 2]| {
        if p[k] <= q[k] {
            (p[k], q[k])
        } else {
            (q[k], p[k])
        }
    };
    let ((xl, xh), (yl, yh)) = (range(a, b), range(c, d));
    let lo = if xl > yl { xl } else { yl };
    let hi = if xh < yh { xh } else { yh };
    Ok(lo < hi)
}

/// The geometric order of vertices (module docs).
fn vert_order(input: &Input<'_>, a: u32, b: u32) -> Ordering {
    let (p, q) = (input.pos(a), input.pos(b));
    p[0].total_cmp(&q[0])
        .then(p[1].total_cmp(&q[1]))
        .then(a.cmp(&b))
}

/// The face cycles' pass (module docs): components in the order of their
/// least vertex, the cycles as found from them, each component's
/// unbounded cycle, and the pairs the containment test asks.
fn cycles(input: &Input<'_>, star: &Star<'_>, m: &mut Meter<'_, '_>) -> Res<Topo> {
    let n = input.verts.len();
    let halves = 2 * input.edges.len();
    m.units(n as u64 + input.edges.len() as u64)?;
    let mut order: Vec<u32> = (0..n as u32).filter(|&v| !star.at(v).is_empty()).collect();
    order.sort_by(|&a, &b| vert_order(input, a, b));
    // Components, by search from each least vertex in order.
    let mut comp_of = vec![NONE; n];
    let mut comp_least = Vec::new();
    let mut stack = Vec::new();
    for &v in &order {
        if comp_of[v as usize] != NONE {
            continue;
        }
        let k = comp_least.len() as u32;
        comp_least.push(v);
        comp_of[v as usize] = k;
        stack.push(v);
        while let Some(w) = stack.pop() {
            for &h in star.at(w) {
                let d = input.dest(h);
                if comp_of[d as usize] == NONE {
                    comp_of[d as usize] = k;
                    stack.push(d);
                }
            }
        }
    }
    let mut members: Vec<Vec<u32>> = vec![Vec::new(); comp_least.len()];
    for &v in &order {
        members[comp_of[v as usize] as usize].push(v);
    }
    let next: Vec<u32> = (0..halves as u32).map(|h| star.next(input, h)).collect();
    let mut cycle_of = vec![NONE; halves];
    let (mut cycle_first, mut cycle_comp) = (Vec::new(), Vec::new());
    for (k, vs) in members.iter().enumerate() {
        for &v in vs {
            for &start in star.at(v) {
                if cycle_of[start as usize] != NONE {
                    continue;
                }
                let c = cycle_first.len() as u32;
                cycle_first.push(start);
                cycle_comp.push(k as u32);
                let mut h = start;
                loop {
                    m.units(1)?;
                    cycle_of[h as usize] = c;
                    h = next[h as usize];
                    if h == start {
                        break;
                    }
                }
            }
        }
    }
    // The unbounded cycle: through the wedge at the least vertex holding
    // `−u`, which follows the last half-edge of the upper half (the last
    // of all when one half is empty).
    let comp_outer: Vec<u32> = comp_least
        .iter()
        .map(|&q| {
            let around = star.at(q);
            let p = input.pos(q);
            let ups = around
                .iter()
                .filter(|&&h| upper(p, input.pos(input.dest(h))))
                .count();
            cycle_of[around[(ups + around.len() - 1) % around.len()] as usize]
        })
        .collect();
    // The UV box of each bounded cycle's vertices, `[lo, hi]` per axis.
    let mut boxes: Vec<(u32, [[f64; 2]; 2])> = Vec::new();
    for (c, &first) in cycle_first.iter().enumerate() {
        let c = c as u32;
        if comp_outer[cycle_comp[c as usize] as usize] == c {
            continue;
        }
        let p = input.pos(input.origin(first));
        let mut b = [p, p];
        let mut h = first;
        loop {
            m.units(1)?;
            let p = input.pos(input.origin(h));
            for a in 0..2 {
                if p[a] < b[0][a] {
                    b[0][a] = p[a];
                }
                if p[a] > b[1][a] {
                    b[1][a] = p[a];
                }
            }
            h = next[h as usize];
            if h == first {
                break;
            }
        }
        boxes.push((c, b));
    }
    let mut asked = Vec::new();
    for (k, &q) in comp_least.iter().enumerate() {
        m.units(1 + boxes.len() as u64 / 64)?;
        let p = input.pos(q);
        for &(c, b) in &boxes {
            let held = (0..2).all(|a| b[0][a] <= p[a] && p[a] <= b[1][a]);
            if held && cycle_comp[c as usize] != k as u32 {
                asked.push((k as u32, c));
            }
        }
    }
    Ok(Topo {
        next,
        cycle_of,
        cycle_first,
        cycle_comp,
        comp_least,
        comp_outer,
        asked,
    })
}

/// The containment certificate of component `k` in bounded cycle `l`
/// (module docs).
fn contains(input: &Input<'_>, topo: &Topo, k: u32, l: u32, m: &mut Meter<'_, '_>) -> Res<bool> {
    let q = input.pos(topo.comp_least[k as usize]);
    let above = |p: [f64; 2]| p[1] > q[1];
    let first = topo.cycle_first[l as usize];
    let mut count = 0i64;
    let mut h = first;
    loop {
        m.units(1)?;
        let av = input.origin(h);
        let (a, b) = (input.pos(av), input.pos(input.dest(h)));
        let crosses = above(a) != above(b);
        let (lo, hi) = if a[0] <= b[0] {
            (a[0], b[0])
        } else {
            (b[0], a[0])
        };
        let (blo, bhi) = if a[1] <= b[1] {
            (a[1], b[1])
        } else {
            (b[1], a[1])
        };
        let in_box = lo <= q[0] && q[0] <= hi && blo <= q[1] && q[1] <= bhi;
        if crosses || in_box {
            let right = if crosses && q[0] < lo {
                true
            } else if crosses && q[0] > hi {
                false
            } else {
                let o = m.orient([a, b, q], av)?;
                if o == Ordering::Equal && in_box {
                    return Err(Stop::Failed(ArrangeError::Touch {
                        component: k,
                        cycle: l,
                    }));
                }
                // Right of `q` where `q` lies left of the span directed
                // upwards.
                if b[1] > a[1] {
                    o == Ordering::Greater
                } else {
                    o == Ordering::Less
                }
            };
            if crosses && right {
                count += if above(b) { 1 } else { -1 };
            }
        }
        h = topo.next[h as usize];
        if h == first {
            break;
        }
    }
    match count {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Stop::Failed(ArrangeError::Crossing {
            component: k,
            cycle: l,
        })),
    }
}

/// The regions' pass (module docs): each component's container from the
/// certificates, the windings by component from the outside in, and the
/// regions.
fn regions(input: &Input<'_>, topo: &Topo, loop_in: &[LoopIn], m: &mut Meter<'_, '_>) -> Res<Nest> {
    let comps = topo.comp_least.len();
    let inside = |k: u32, l: u32| {
        loop_in
            .binary_search_by(|r| (r.component, r.cycle).cmp(&(k, l)))
            .is_ok_and(|i| loop_in[i].inside)
    };
    let mut comp_container = vec![NONE; comps];
    let mut depth = vec![0usize; comps];
    let mut at = 0;
    for k in 0..comps as u32 {
        let from = at;
        while at < loop_in.len() && loop_in[at].component == k {
            at += 1;
        }
        let held: Vec<u32> = loop_in[from..at]
            .iter()
            .filter(|r| r.inside)
            .map(|r| r.cycle)
            .collect();
        m.units(1 + (held.len() as u64).saturating_mul(held.len() as u64))?;
        // Cycles of one component are numbered together, so a component
        // holding `k` twice has two neighbours here.
        if held
            .windows(2)
            .any(|w| topo.cycle_comp[w[0] as usize] == topo.cycle_comp[w[1] as usize])
        {
            return Err(Stop::Failed(ArrangeError::Nesting(k)));
        }
        let innermost: Vec<u32> = held
            .iter()
            .copied()
            .filter(|&l| {
                let inner = topo.cycle_comp[l as usize];
                held.iter().all(|&o| o == l || inside(inner, o))
            })
            .collect();
        match innermost[..] {
            [] if held.is_empty() => {}
            [l] => comp_container[k as usize] = l,
            _ => return Err(Stop::Failed(ArrangeError::Nesting(k))),
        }
        depth[k as usize] = held.len();
    }
    // Windings, containers before what they contain.
    let cycles = topo.cycle_first.len();
    let mut winding: Vec<Option<i64>> = vec![None; cycles];
    let mut by_depth: Vec<u32> = (0..comps as u32).collect();
    by_depth.sort_by_key(|&k| (depth[k as usize], k));
    let mut queue = VecDeque::new();
    for &k in &by_depth {
        let container = comp_container[k as usize];
        let base = if container == NONE {
            0
        } else {
            winding[container as usize].unwrap_or(0)
        };
        let outer = topo.comp_outer[k as usize];
        winding[outer as usize] = Some(base);
        queue.push_back(outer);
        while let Some(c) = queue.pop_front() {
            let w = winding[c as usize].unwrap_or(0);
            let first = topo.cycle_first[c as usize];
            let mut h = first;
            loop {
                m.units(1)?;
                let right = topo.cycle_of[(h ^ 1) as usize];
                let want = w
                    .checked_sub(input.wind(h))
                    .ok_or(Stop::Failed(ArrangeError::WindingRange))?;
                match winding[right as usize] {
                    None => {
                        winding[right as usize] = Some(want);
                        queue.push_back(right);
                    }
                    Some(have) if have != want => {
                        return Err(Stop::Failed(ArrangeError::Winding(h / 2)));
                    }
                    Some(_) => {}
                }
                h = topo.next[h as usize];
                if h == first {
                    break;
                }
            }
        }
    }
    let mut regions = vec![Region {
        outer: NONE,
        holes: Vec::new(),
        winding: 0,
    }];
    let mut cycle_region = vec![NONE; cycles];
    for c in 0..cycles as u32 {
        if topo.comp_outer[topo.cycle_comp[c as usize] as usize] != c {
            cycle_region[c as usize] = regions.len() as u32;
            regions.push(Region {
                outer: c,
                holes: Vec::new(),
                winding: winding[c as usize].unwrap_or(0),
            });
        }
    }
    for k in 0..comps {
        let outer = topo.comp_outer[k];
        let container = comp_container[k];
        let r = if container == NONE {
            0
        } else {
            cycle_region[container as usize]
        };
        cycle_region[outer as usize] = r;
        regions[r as usize].holes.push(outer);
    }
    Ok(Nest {
        cycle_region,
        comp_container,
        regions,
    })
}

#[cfg(test)]
mod tests;
