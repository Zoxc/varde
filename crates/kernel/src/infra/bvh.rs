// SPDX-License-Identifier: AGPL-3.0-or-later AND Apache-2.0
// SPDX-FileCopyrightText: 2021 The Manifold Authors
//
// Derived from Manifold (https://github.com/elalish/manifold), commit 3ad9216b2b62f4faaf3ea5342b4c99cdca70dbd2, file src/collider.h,
//   `CreateRadixTree` (`PrefixLength`, `RangeEnd`, `FindSplit`), `FindCollision`,
//   `BuildInternalBoxes`, `SpreadBits3`, `Collider::MortonCode` and the node
//   numbering (lines 69-243, 345-353),
//   Copyright 2021 The Manifold Authors,
//   licensed under the Apache License, Version 2.0 (LICENSES/Apache-2.0.txt).
// Translated to Rust and modified: `f64` boxes in 2 or 3 dimensions (a
//   2D Morton code of 15 bits per axis beside the 3D one of 10), closed
//   overlap tests with empty boxes left out of the tree, the internal boxes
//   formed sequentially, a tree of one leaf queried, every query charged to
//   the operation's budget and counted before its pairs are collected, pairs
//   given by the caller's indices and sorted.
// See THIRD_PARTY.md, row L1.
//
// Derived from Manifold (https://github.com/elalish/manifold), commit 3ad9216b2b62f4faaf3ea5342b4c99cdca70dbd2, file src/sort.cpp,
//   `MortonCode` (lines 28-34) and the stable sort of `SortFaces` by Morton code
//   (lines 463-490),
//   Copyright 2021 The Manifold Authors,
//   licensed under the Apache License, Version 2.0 (LICENSES/Apache-2.0.txt).
// Translated to Rust and modified: centres that aren't finite clamped
//   instead of coded past the end; the sort keyed by code and the caller's
//   index; empty boxes, not removed faces, dropped from the end.
// See THIRD_PARTY.md, row L8.

//! A bounding volume hierarchy over axis-aligned `f64` boxes in two or
//! three dimensions: the broad phase of the operations, which lists the
//! pairs of pieces whose boxes meet so that only those are tested
//! exactly.
//!
//! **The tree** is a Karras radix tree over the leaves sorted by the
//! Morton code of their boxes' centres, as Manifold builds it (T. Karras,
//! *Maximizing parallelism in the construction of BVHs, octrees, and k-d
//! trees*, HPG 2012). The centres are quantised on the box of all centres,
//! 10 bits an axis in 3D and 15 in 2D, and interleaved into a 30-bit code;
//! the leaves are sorted by code, then by the caller's index, so equal
//! codes keep one order. Each internal node `i` covers the range of
//! sorted leaves around leaf `i` that share a longer prefix with it than
//! with their outside neighbours (equal codes are told apart by their
//! positions), split where the next bit changes: a pure function of the
//! sorted codes, so the nodes are formed in parallel. The internal boxes
//! are the unions of their children's, formed bottom-up. Nodes are
//! numbered as Manifold numbers them: leaf `l` is node `2l`, internal node
//! `i` node `2i + 1`, the root node 1.
//!
//! **Boxes** are closed: two meet when they do along every axis, sides
//! touching included. A box with a side `lo > hi` or a NaN is empty: it
//! meets nothing and the tree leaves it out. Sides may be infinite (a
//! hull with nothing finite certified on that side). The boxes are the
//! caller's, already rounded outward from certified bounds;
//! [`Aabb::inflated`] widens one by a band (`ε_dec`, an edge's `β_g`),
//! rounding outward again, and [`Aabb::union`] is exact. Which pairs
//! meet is then a fact of the boxes alone: the tree's shape, the sort and
//! the threads only change how fast they are found.
//!
//! **Budget.** Building charges each leaf before the work starts. A
//! query charges every box it tests as it walks, and only then, knowing
//! how many hits there are, charges their collection and collects them:
//! counted, then collected, so crowded boxes making pairs of nearly every
//! two run out of budget before any list is formed. [`Bvh::pairs`] does
//! this over a list of query boxes as two passes of the operation's
//! [`Work`]: one counting, by chunks of queries in their order, one
//! collecting once the counts fit. Pairs come as `[query, leaf]` in the
//! caller's indices, sorted, so the result, the work and where the budget
//! runs out are the same on any number of threads.

use std::cmp::Ordering::{Equal, Less};
use std::convert::Infallible;

use varde_decide::bezier::tensor::HullBox;
use varde_decide::num::schedule::{UNIT_PARTS, UNITS};

use super::budget::{Exhausted, Halt, Ledger, Pass, Step, Stop, Work};
use super::error::KernelError;
use super::par::par_map;

/// The parts of a unit ([`UNIT_PARTS`]) charged for each leaf of a tree
/// built: its centre and code, its place in the sort, its node's range
/// search and split, its box in the bottom-up union (the unit table's
/// row, [`BvhUnits::build`]).
///
/// [`BvhUnits::build`]: varde_decide::num::schedule::BvhUnits::build
pub(crate) const BUILD_PARTS: u64 = UNITS.bvh.build;

/// The parts of a unit charged for each box a query tests against a node,
/// the walk included ([`BvhUnits::test`]).
///
/// [`BvhUnits::test`]: varde_decide::num::schedule::BvhUnits::test
pub(crate) const TEST_PARTS: u64 = UNITS.bvh.test;

/// The parts of a unit charged for each hit a query collects
/// ([`BvhUnits::hit`]).
///
/// [`BvhUnits::hit`]: varde_decide::num::schedule::BvhUnits::hit
pub(crate) const HIT_PARTS: u64 = UNITS.bvh.hit;

/// How many queries one item of [`Bvh::pairs`]'s passes runs.
const CHUNK: usize = 64;

/// The node of the root, when the tree has an internal node.
const ROOT: u32 = 1;

/// A closed axis-aligned box in `D` dimensions: the points `p` with
/// `lo[k] ≤ p[k] ≤ hi[k]` for every axis `k`. Empty when a side has
/// `lo > hi` or a NaN.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Aabb<const D: usize> {
    pub(crate) lo: [f64; D],
    pub(crate) hi: [f64; D],
}

impl<const D: usize> Aabb<D> {
    /// The box of the single point `p`.
    pub(crate) fn point(p: [f64; D]) -> Self {
        Aabb { lo: p, hi: p }
    }

    /// Whether it holds no point: a side with `lo > hi`, or a NaN.
    pub(crate) fn is_empty(&self) -> bool {
        (0..D).any(|k| !matches!(self.lo[k].partial_cmp(&self.hi[k]), Some(Less | Equal)))
    }

    /// Whether the two boxes share a point: neither empty, and they meet
    /// along every axis, touching sides included.
    pub(crate) fn meets(&self, other: &Self) -> bool {
        !self.is_empty() && !other.is_empty() && self.touches(other)
    }

    /// [`Aabb::meets`] for boxes known not to be empty.
    fn touches(&self, other: &Self) -> bool {
        (0..D).all(|k| self.lo[k] <= other.hi[k] && other.lo[k] <= self.hi[k])
    }

    /// The least box holding both, of two boxes not empty: exact, its
    /// sides those of the one or the other (the first's where they are
    /// equal, so of `+0` and `-0` the first's).
    pub(crate) fn union(&self, other: &Self) -> Self {
        Aabb {
            lo: std::array::from_fn(|k| {
                if other.lo[k] < self.lo[k] {
                    other.lo[k]
                } else {
                    self.lo[k]
                }
            }),
            hi: std::array::from_fn(|k| {
                if other.hi[k] > self.hi[k] {
                    other.hi[k]
                } else {
                    self.hi[k]
                }
            }),
        }
    }

    /// The box widened by `margin ≥ 0` on every side, rounded outward:
    /// it holds every point within `margin` of the box along each axis.
    /// A margin of zero gives the box itself; a NaN margin an empty box.
    pub(crate) fn inflated(&self, margin: f64) -> Self {
        debug_assert!(margin >= 0.0 || margin.is_nan(), "a margin of {margin}");
        if margin == 0.0 {
            return *self;
        }
        Aabb {
            lo: std::array::from_fn(|k| (self.lo[k] - margin).next_down()),
            hi: std::array::from_fn(|k| (self.hi[k] + margin).next_up()),
        }
    }

    /// Its centre, halves summed so finite boxes never overflow (not
    /// finite for a side that isn't).
    fn centre(&self) -> [f64; D] {
        std::array::from_fn(|k| self.lo[k] * 0.5 + self.hi[k] * 0.5)
    }
}

impl From<HullBox> for Aabb<3> {
    fn from(hull: HullBox) -> Self {
        Aabb {
            lo: hull.lo,
            hi: hull.hi,
        }
    }
}

/// A bounding volume hierarchy over `D`-dimensional boxes (module docs),
/// `D` 2 or 3.
#[derive(Clone, Debug)]
pub(crate) struct Bvh<const D: usize> {
    /// Every node's box, by node number: leaf `l` at `2l`, internal node
    /// `i` at `2i + 1`.
    nodes: Vec<Aabb<D>>,
    /// The two children of each internal node, as node numbers.
    children: Vec<[u32; 2]>,
    /// The caller's index of each leaf, in the sorted order.
    leaves: Vec<u32>,
    /// How many boxes it was built over, empty ones included.
    len: usize,
}

impl<const D: usize> Bvh<D> {
    const DIMENSION: () = assert!(D == 2 || D == 3, "a tree of 2 or 3 dimensions");

    /// The tree over `boxes`, box `i` the caller's leaf `i`, charging
    /// [`BUILD_PARTS`] a box to `ledger` before building. Empty boxes are
    /// left out (they meet nothing). Fails only when the charge runs out.
    pub(crate) fn build(boxes: &[Aabb<D>], ledger: &mut Ledger<'_>) -> Result<Self, Exhausted> {
        let () = Self::DIMENSION;
        let parts = u64::try_from(boxes.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(BUILD_PARTS);
        if parts > 0 {
            ledger.charge(Step::Work, parts.div_ceil(UNIT_PARTS))?;
        }
        // The charge bounds the count far below `u32::MAX / 2`: at most
        // `MAX_WORK·UNIT_PARTS/BUILD_PARTS` boxes, so node numbers fit.
        debug_assert!(boxes.len() < (1 << 30), "a budget admits fewer boxes");
        Ok(Self::sorted_tree(boxes))
    }

    /// The tree over `boxes`, uncharged: for a tree built with what it
    /// indexes, as a surface's over its tiles is with the tiles (at most
    /// `2³⁰` boxes, so node numbers fit).
    pub(crate) fn sorted_tree(boxes: &[Aabb<D>]) -> Self {
        let () = Self::DIMENSION;
        let kept: Vec<u32> = (0..boxes.len() as u32)
            .filter(|&i| !boxes[i as usize].is_empty())
            .collect();
        let centres: Vec<[f64; D]> = kept.iter().map(|&i| boxes[i as usize].centre()).collect();
        let (lo, hi) = finite_range(&centres);
        let codes = par_map(&centres, |c| morton(c, &lo, &hi));
        // Code, then the caller's index: the order of `SortFaces`'s stable
        // sort, the same however the sort runs.
        let mut order: Vec<(u32, u32)> = codes.into_iter().zip(kept).collect();
        order.sort_unstable();
        let (codes, leaves): (Vec<u32>, Vec<u32>) = order.into_iter().unzip();
        let n = leaves.len();
        let mut nodes = vec![
            Aabb {
                lo: [0.0; D],
                hi: [0.0; D],
            };
            (2 * n).saturating_sub(1)
        ];
        for (l, &i) in leaves.iter().enumerate() {
            nodes[2 * l] = boxes[i as usize];
        }
        let internal: Vec<u32> = (0..n.saturating_sub(1) as u32).collect();
        let children = par_map(&internal, |&i| radix_node(&codes, i));
        let mut tree = Bvh {
            nodes,
            children,
            leaves,
            len: boxes.len(),
        };
        tree.internal_boxes();
        tree
    }

    /// The internal nodes' boxes, the union of their children's
    /// (`BuildInternalBoxes`): walking up from each leaf in order, a node
    /// is formed by the second of its children to arrive, so after both.
    /// Each union is taken in the children's order, so a box doesn't
    /// depend on which arrived first.
    fn internal_boxes(&mut self) {
        let n = self.leaves.len();
        if n < 2 {
            return;
        }
        let mut parent = vec![0u32; 2 * n - 1];
        for (i, &[a, b]) in self.children.iter().enumerate() {
            let node = 2 * i as u32 + 1;
            parent[a as usize] = node;
            parent[b as usize] = node;
        }
        let mut arrived = vec![false; n - 1];
        for leaf in 0..n {
            let mut node = 2 * leaf as u32;
            loop {
                node = parent[node as usize];
                let internal = (node / 2) as usize;
                if !std::mem::replace(&mut arrived[internal], true) {
                    break;
                }
                let [a, b] = self.children[internal];
                self.nodes[node as usize] = self.nodes[a as usize].union(&self.nodes[b as usize]);
                if node == ROOT {
                    break;
                }
            }
        }
    }

    /// How many boxes it was built over, empty ones included.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Whether it was built over no boxes.
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The box of all its boxes not empty; `None` if there are none.
    pub(crate) fn bounds(&self) -> Option<Aabb<D>> {
        match self.leaves.len() {
            0 => None,
            1 => Some(self.nodes[0]),
            _ => Some(self.nodes[ROOT as usize]),
        }
    }

    /// The caller's indices of the boxes meeting `query`, ascending:
    /// every box tested charged to `ledger` as the walk goes, then the
    /// hits' collection, before they are collected.
    pub(crate) fn hits(
        &self,
        query: &Aabb<D>,
        ledger: &mut Ledger<'_>,
    ) -> Result<Vec<u32>, Exhausted> {
        let mut meter = Meter::new(ledger);
        let (count, tests) = self.count(query, |_| true, &mut meter)?;
        meter.parts(collect_parts(tests, count))?;
        meter.finish()?;
        let mut out = Vec::with_capacity(count);
        self.walk(query, |_| true, || Ok(()), |leaf| out.push(leaf))?;
        out.sort_unstable();
        Ok(out)
    }

    /// The pairs `[q, l]` of a query box `queries[q]` (at most `u32::MAX`
    /// of them) and a box `l` of the tree that meet, sorted: two passes of `work` labelled `pass`, the
    /// first counting the hits of each chunk of queries, the second, once
    /// the counts fit, collecting them, each chunk charged its count's
    /// work again and its hits before collecting. Fails only with
    /// [`KernelError::OutOfBudget`].
    pub(crate) fn pairs(
        &self,
        queries: &[Aabb<D>],
        work: &mut Work,
        pass: Pass,
    ) -> Result<Vec<[u32; 2]>, KernelError> {
        self.pairs_where(queries, |_, _| true, work, pass)
    }

    /// The pairs `[i, j]`, `i < j`, of its own boxes that meet, sorted:
    /// [`Bvh::pairs`] of the tree's boxes against themselves, each pair
    /// once.
    pub(crate) fn self_pairs(
        &self,
        work: &mut Work,
        pass: Pass,
    ) -> Result<Vec<[u32; 2]>, KernelError> {
        // The empty boxes left out of the tree stay empty queries.
        let mut boxes = vec![
            Aabb {
                lo: [1.0; D],
                hi: [0.0; D],
            };
            self.len
        ];
        for (l, &i) in self.leaves.iter().enumerate() {
            boxes[i as usize] = self.nodes[2 * l];
        }
        self.pairs_where(&boxes, |q, l| l > q, work, pass)
    }

    /// [`Bvh::pairs`] of the pairs `keep(q, l)` takes.
    fn pairs_where(
        &self,
        queries: &[Aabb<D>],
        keep: impl Fn(u32, u32) -> bool + Sync,
        work: &mut Work,
        pass: Pass,
    ) -> Result<Vec<[u32; 2]>, KernelError> {
        debug_assert!(u32::try_from(queries.len()).is_ok(), "queries by u32 index");
        let chunks: Vec<u32> = (0..queries.len().div_ceil(CHUNK) as u32).collect();
        let chunk = |c: u32| {
            let from = c as usize * CHUNK;
            let to = (from + CHUNK).min(queries.len());
            (from as u32..to as u32).zip(&queries[from..to])
        };
        let counted = work
            .pass(pass, &chunks, |&c, ledger| {
                let mut meter = Meter::new(ledger);
                let (mut count, mut tests) = (0usize, 0u64);
                for (q, query) in chunk(c) {
                    let (n, t) = self.count(query, |l| keep(q, l), &mut meter)?;
                    count += n;
                    tests = tests.saturating_add(t);
                }
                meter.finish()?;
                Ok::<_, Stop<Infallible>>((count, tests))
            })
            .map_err(out_of_budget)?;
        let total = counted.iter().map(|&(count, _)| count).sum();
        let found = work
            .pass(pass, &chunks, |&c, ledger| {
                let (count, tests) = counted[c as usize];
                let mut meter = Meter::new(ledger);
                meter.parts(collect_parts(tests, count))?;
                meter.finish()?;
                let mut pairs = Vec::with_capacity(count);
                let mut hits = Vec::new();
                for (q, query) in chunk(c) {
                    hits.clear();
                    self.walk(query, |l| keep(q, l), || Ok(()), |l| hits.push(l))?;
                    hits.sort_unstable();
                    pairs.extend(hits.iter().map(|&l| [q, l]));
                }
                Ok::<_, Stop<Infallible>>(pairs)
            })
            .map_err(out_of_budget)?;
        let mut out = Vec::with_capacity(total);
        for pairs in found {
            out.extend(pairs);
        }
        Ok(out)
    }

    /// How many boxes meeting `query` `keep` takes, and how many box tests
    /// the walk made, each test charged to `meter` before it is made.
    fn count(
        &self,
        query: &Aabb<D>,
        keep: impl Fn(u32) -> bool,
        meter: &mut Meter<'_, '_>,
    ) -> Result<(usize, u64), Exhausted> {
        let (mut count, mut tests) = (0usize, 0u64);
        self.walk(
            query,
            keep,
            || {
                tests += 1;
                meter.parts(TEST_PARTS)
            },
            |_| count += 1,
        )?;
        Ok((count, tests))
    }

    /// Calls `hit` with the caller's index of every box meeting `query`
    /// that `keep` takes, in the tree's order (`FindCollision`), and
    /// `test` before each box test, stopping at its first failure. An
    /// empty query tests nothing.
    fn walk(
        &self,
        query: &Aabb<D>,
        keep: impl Fn(u32) -> bool,
        mut test: impl FnMut() -> Result<(), Exhausted>,
        mut hit: impl FnMut(u32),
    ) -> Result<(), Exhausted> {
        if query.is_empty() {
            return Ok(());
        }
        match self.leaves.len() {
            0 => return Ok(()),
            1 => {
                // A tree of one leaf has no internal node to start from.
                test()?;
                if self.nodes[0].touches(query) && keep(self.leaves[0]) {
                    hit(self.leaves[0]);
                }
                return Ok(());
            }
            _ => {}
        }
        // The depth is at most the 30 code bits and the 32 index bits:
        // the stack never grows past it.
        let mut stack: Vec<u32> = Vec::with_capacity(64);
        let mut node = ROOT;
        loop {
            let [a, b] = self.children[(node / 2) as usize];
            let mut visit = |child: u32| -> Result<bool, Exhausted> {
                test()?;
                let meets = self.nodes[child as usize].touches(query);
                if meets && child % 2 == 0 {
                    let leaf = self.leaves[(child / 2) as usize];
                    if keep(leaf) {
                        hit(leaf);
                    }
                }
                Ok(meets && child % 2 == 1)
            };
            let into_a = visit(a)?;
            let into_b = visit(b)?;
            node = match (into_a, into_b) {
                (true, true) => {
                    stack.push(b);
                    a
                }
                (true, false) => a,
                (false, true) => b,
                (false, false) => match stack.pop() {
                    Some(next) => next,
                    None => return Ok(()),
                },
            };
        }
    }
}

/// The parts charged to collect `count` hits found by `tests` box tests:
/// the walk again, and each hit.
fn collect_parts(tests: u64, count: usize) -> u64 {
    let hits = u64::try_from(count).unwrap_or(u64::MAX);
    tests
        .saturating_mul(TEST_PARTS)
        .saturating_add(hits.saturating_mul(HIT_PARTS))
}

/// The error of a pass that can only run out.
fn out_of_budget(halt: Halt<Infallible>) -> KernelError {
    match halt {
        Halt::OutOfBudget(error) => error,
        Halt::Failed { error, .. } => match error {},
    }
}

/// A ledger with the parts of a unit charged so far.
struct Meter<'l, 'p> {
    ledger: &'l mut Ledger<'p>,
    parts: u64,
}

impl<'l, 'p> Meter<'l, 'p> {
    fn new(ledger: &'l mut Ledger<'p>) -> Self {
        Meter { ledger, parts: 0 }
    }

    /// Charges `p` parts of a unit before the work they stand for: the
    /// whole units they complete now, the rest with the next parts or at
    /// the end ([`Meter::finish`]).
    fn parts(&mut self, p: u64) -> Result<(), Exhausted> {
        self.parts = self.parts.saturating_add(p);
        let whole = self.parts / UNIT_PARTS;
        self.parts %= UNIT_PARTS;
        if whole > 0 {
            self.ledger.charge(Step::Work, whole)?;
        }
        Ok(())
    }

    /// The parts left over, rounded up to a unit.
    fn finish(&mut self) -> Result<(), Exhausted> {
        if self.parts > 0 {
            self.parts = 0;
            self.ledger.charge(Step::Work, 1)?;
        }
        Ok(())
    }
}

/// The least and greatest finite value of each axis over `points`
/// (`0` both where an axis has none).
fn finite_range<const D: usize>(points: &[[f64; D]]) -> ([f64; D], [f64; D]) {
    let mut lo = [f64::INFINITY; D];
    let mut hi = [f64::NEG_INFINITY; D];
    for p in points {
        for k in 0..D {
            if p[k].is_finite() {
                if p[k] < lo[k] {
                    lo[k] = p[k];
                }
                if p[k] > hi[k] {
                    hi[k] = p[k];
                }
            }
        }
    }
    for k in 0..D {
        if lo[k] > hi[k] {
            (lo[k], hi[k]) = (0.0, 0.0);
        }
    }
    (lo, hi)
}

/// The Morton code of `centre` on the box `[lo, hi]` (`MortonCode`):
/// each axis quantised to 10 bits in 3D, 15 in 2D, and interleaved
/// with the first axis highest. A centre below the box, NaN, or on an
/// axis of no extent quantises to 0, one above it to the top.
fn morton<const D: usize>(centre: &[f64; D], lo: &[f64; D], hi: &[f64; D]) -> u32 {
    let bits = if D == 3 { 10 } else { 15 };
    let cells = f64::from(1u32 << bits);
    let top = (1u32 << bits) - 1;
    let mut code = 0u32;
    for k in 0..D {
        // Halves, so the widest finite range doesn't overflow.
        let extent = hi[k] * 0.5 - lo[k] * 0.5;
        let t = cells * ((centre[k] * 0.5 - lo[k] * 0.5) / extent);
        let q = if extent > 0.0 && t > 0.0 {
            if t >= f64::from(top) { top } else { t as u32 }
        } else {
            0
        };
        let spread = if D == 3 { spread3(q) } else { spread2(q) };
        code |= spread << (D - 1 - k);
    }
    code
}

/// The low 10 bits of `v` spread to every third bit (`SpreadBits3`).
fn spread3(v: u32) -> u32 {
    let v = 0xFF00_00FF & v.wrapping_mul(0x0001_0001);
    let v = 0x0F00_F00F & v.wrapping_mul(0x0000_0101);
    let v = 0xC30C_30C3 & v.wrapping_mul(0x0000_0011);
    0x4924_9249 & v.wrapping_mul(0x0000_0005)
}

/// The low 15 bits of `v` spread to every second bit.
fn spread2(v: u32) -> u32 {
    let v = v & 0x7FFF;
    let v = (v | (v << 8)) & 0x00FF_00FF;
    let v = (v | (v << 4)) & 0x0F0F_0F0F;
    let v = (v | (v << 2)) & 0x3333_3333;
    (v | (v << 1)) & 0x5555_5555
}

/// The common prefix of sorted leaves `i` and `j` (`PrefixLength`), `-1`
/// for a `j` outside them; equal codes compare their positions, past the
/// code's 32 bits.
fn prefix(codes: &[u32], i: i64, j: i64) -> i32 {
    if j < 0 || j >= codes.len() as i64 {
        return -1;
    }
    let (a, b) = (codes[i as usize], codes[j as usize]);
    if a == b {
        32 + ((i as u32) ^ (j as u32)).leading_zeros() as i32
    } else {
        (a ^ b).leading_zeros() as i32
    }
}

/// The other end of the range of sorted leaves internal node `i` covers
/// (`RangeEnd`): from leaf `i` towards the neighbour it shares more with,
/// as far as the leaves share more than `i` does with its other
/// neighbour; found by an exponential, then a binary search.
fn range_end(codes: &[u32], i: i64) -> i64 {
    let dir = (prefix(codes, i, i + 1) - prefix(codes, i, i - 1)).signum() as i64;
    let common = prefix(codes, i, i - dir);
    let mut max_length: i64 = 128;
    while prefix(codes, i, i + dir * max_length) > common {
        max_length *= 4;
    }
    let mut length = 0;
    let mut step = max_length / 2;
    while step > 0 {
        if prefix(codes, i, i + dir * (length + step)) > common {
            length += step;
        }
        step /= 2;
    }
    i + dir * length
}

/// The last leaf of the first child of the range `first..=last`
/// (`FindSplit`): the furthest leaf sharing more than the range's common
/// prefix with `first`, by binary search.
fn find_split(codes: &[u32], first: i64, last: i64) -> i64 {
    let common = prefix(codes, first, last);
    let mut split = first;
    let mut step = last - first;
    loop {
        step = (step + 1) >> 1;
        let candidate = split + step;
        if candidate < last && prefix(codes, first, candidate) > common {
            split = candidate;
        }
        if step <= 1 {
            return split;
        }
    }
}

/// The children of internal node `i` of the radix tree over the sorted
/// `codes`, as node numbers (`CreateRadixTree`): a child covering one
/// leaf is that leaf's node.
fn radix_node(codes: &[u32], i: u32) -> [u32; 2] {
    let (mut first, mut last) = (i64::from(i), range_end(codes, i64::from(i)));
    if first > last {
        std::mem::swap(&mut first, &mut last);
    }
    let split = find_split(codes, first, last);
    let node = |at: i64, end: i64| {
        if at == end {
            2 * at as u32
        } else {
            2 * at as u32 + 1
        }
    };
    [node(split, first), node(split + 1, last)]
}

/// The tree's steps uncharged, for the calibration benchmark
/// (`crates/kernel/benches/calibrate.rs`), which times each against the
/// parts it is charged ([`BUILD_PARTS`], [`TEST_PARTS`], [`HIT_PARTS`]).
/// Not part of the kernel's interface.
#[doc(hidden)]
pub mod calibration {
    use super::{Aabb, Bvh};

    /// A tree over 3D boxes.
    pub struct Tree(Bvh<3>);

    impl Tree {
        /// The tree over the boxes `[lo, hi]`, built uncharged.
        pub fn new(boxes: &[[[f64; 3]; 2]]) -> Tree {
            let boxes: Vec<Aabb<3>> = boxes.iter().map(|&[lo, hi]| Aabb { lo, hi }).collect();
            Tree(Bvh::sorted_tree(&boxes))
        }

        /// A query's count, as its charged walk counts it: the boxes
        /// meeting `[lo, hi]` and the box tests made.
        pub fn count(&self, [lo, hi]: [[f64; 3]; 2]) -> (usize, u64) {
            let (mut count, mut tests) = (0usize, 0u64);
            let _ = self.0.walk(
                &Aabb { lo, hi },
                |_| true,
                || {
                    tests += 1;
                    Ok(())
                },
                |_| count += 1,
            );
            (count, tests)
        }

        /// A query's collection, as its charged walk collects it: the
        /// boxes meeting `[lo, hi]`, sorted.
        pub fn hits(&self, [lo, hi]: [[f64; 3]; 2]) -> Vec<u32> {
            let mut out = Vec::new();
            let _ = self
                .0
                .walk(&Aabb { lo, hi }, |_| true, || Ok(()), |l| out.push(l));
            out.sort_unstable();
            out
        }
    }
}

#[cfg(test)]
mod tests;
