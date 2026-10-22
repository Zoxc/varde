//! Root isolation of a square polynomial system in Bernstein form on
//! `[0, 1]ᵈ` ([`isolate_nd`]): `d` forms ([`NdTensor`]) in `d` variables,
//! a patch against a curve (three) or two patches tangent at a point
//! (four), at one precision level. Certified isolating boxes for the
//! simple common roots, and clusters for what that level can't separate,
//! which the caller escalates or decides by its own rule.
//!
//! **Subdivision.** Dyadic boxes, each split at ½ in every variable some
//! form depends on ([`NdTensor::split_half`], the same bits under
//! reversal), depth first, so the work and the result follow from the
//! coefficients alone. A box is settled, and not split again, when it is
//!
//! - *empty*: some form certified of one sign on it (its coefficient
//!   hull), or Krawczyk's operator (below) disjoint from it;
//! - a *root*: Krawczyk's operator inside its interior, which certifies
//!   exactly one root in it, simple;
//! - *pending*: the operator contracts (`‖I − Y·J(X)‖∞ < ½`, so at most
//!   one root, simple) and narrows to a box touching a side (a root on or
//!   beside a split face, or on the domain's boundary), or the box is at
//!   the depth limit;
//! - *open*: every form certified within its caller's target `τᵢ` on the
//!   box (`|fᵢ| ≤ τᵢ`; a tangency, roots closer than the target resolves),
//!   every coefficient uncertain (splitting no longer helps at this
//!   precision), or at the depth limit.
//!
//! The target is a value, in each form's own units (a length, never a
//! parameter gap): with `τ = 0` only the precision floor and the depth
//! limit stop a tangency, deep down, so callers give their curved-sign
//! target.
//!
//! **Krawczyk's operator** `K(X) = c − Y·f(c) + (I − Y·J(X))·(X − c)`,
//! `c` the box's middle, `J(X)` an enclosure of the Jacobian on `X` and
//! `Y` the inverse of its middle (in floats; any real `Y` keeps every
//! root of `X` in `K(X)`), in interval arithmetic rounded outwards. Its
//! iterates `X ∩ K(X)` narrow a root; inside `X`'s interior it certifies
//! one root, unique in `X`, and every Jacobian on `X` regular, so the
//! sign of the Jacobian's determinant there is the root's orientation. A
//! box's narrowed sub-boxes live on the lattice of `2⁻⁵³` of the box's
//! own side, the middle the even of the two nearest lattice points, and
//! the Jacobian is the hull over the (at most two per variable) dyadic
//! cells covering the sub-box, split from the box by halves: so a form
//! reversed in a variable takes the mirrored steps to the bit.
//!
//! **Results.** A root's box is closed, so a root on a face shared by two
//! settled boxes would be in both: pending and open boxes that touch
//! (their closed boxes meet) are joined, a maximal set of them one result.
//! A set of pending boxes is one root when Krawczyk's operator certifies
//! a root on the hull of their narrowed boxes, inflated (in the smallest
//! dyadic cell holding the set, widened to its parents while the inflated
//! hull reaches past it), that box meeting no other unsettled box and no
//! other root's narrowed box: then the set's roots (each box holds at most
//! one, in its narrowed box) are that one. A box certified there holding a
//! root box's whole narrowed box instead holds that root, and the set none.
//! Anything else is a [`NdCluster`]. Root boxes are apart from every other
//! result but at points certified free of roots (their own boundary is),
//! and joined sets touch nothing else: so every root of the system on
//! `[0, 1]ᵈ` is in exactly one result. A root on the domain's boundary
//! (`0` or `1` in some variable) is a cluster reaching it: the pieces
//! meeting there settle it from that point's record, never by evaluation.
//!
//! **Symmetry.** A system reversed in a variable (`xₐ ↦ 1 − xₐ` in every
//! form) gives the mirrored boxes, kinds and orientations (the sign flips),
//! and its narrowed lattice boxes are the mirrored ones (the `f64` bounds
//! of [`NdRoot::near`] round outwards from them). Permuting the variables
//! gives the permuted result up to the last bits of the forms' splits,
//! which act on one variable after another.
//!
//! **Work.** Every halving of the subdivision, box test and Krawczyk step
//! (an evaluation, the Jacobian's restrictions, a `d × d` solve) is counted
//! ([`Work`]); the caller charges it and caps the halvings
//! ([`Limits::max_splits`], past which [`NdFail::OutOfWork`]).

use std::cmp::Ordering;

use malachite::Rational;
use smallvec::{SmallVec, smallvec};

use super::nd::NdTensor;
use super::roots::{
    Dyadic, Limits, MAX_ROOT_DEPTH, OutOfWork, Work, add, int_bounds, mul, pow2_neg, sub,
};
use super::scalar::Scalar;
use crate::num::bounded::add_up;
use crate::num::exact::rat;

/// The most variables [`isolate_nd`] takes, 6: a box splits into `2ᵈ`.
pub const MAX_VARIABLES: usize = 6;

/// A box's narrowed sub-boxes are on the lattice `2^-LATTICE` of its side.
pub(super) const LATTICE: u32 = 53;

/// A box's side in lattice units.
pub(super) const SIDE: u64 = 1 << LATTICE;

/// The most Krawczyk steps narrowing one box.
const NARROW_STEPS: u32 = 16;

/// The most widenings of a stalled box looking for a certificate: each
/// doubles it, so the last is `2¹²` times as wide.
const INFLATIONS: u32 = 12;

/// `‖I − Y·J(X)‖∞` below this: the box holds at most one root, simple,
/// and the iterates converge fast enough to settle it.
const CONTRACTING: f64 = 0.5;

pub(super) type Axes<T> = SmallVec<[T; 4]>;

/// A box on a cell's lattice: `[lo, hi]` per variable, in `0..=SIDE`.
pub(super) type Lat = Axes<[u64; 2]>;

/// A closed dyadic box of `[0, 1]ᵈ`: `[lo, hi]` per variable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NdBox {
    pub axes: Axes<[Dyadic; 2]>,
}

impl NdBox {
    /// Whether the two closed boxes meet (at a point at least).
    pub fn meets(&self, o: &NdBox) -> bool {
        self.axes
            .iter()
            .zip(&o.axes)
            .all(|(a, b)| a[0] <= b[1] && b[0] <= a[1])
    }

    /// Whether the box reaches the boundary of `[0, 1]ᵈ`.
    pub fn on_boundary(&self) -> bool {
        self.axes
            .iter()
            .any(|a| a[0] == Dyadic::ZERO || a[1] == Dyadic::ONE)
    }

    /// `[lo, hi]` per variable as `f64`s bounding the box outwards.
    pub fn bounds(&self) -> Axes<[f64; 2]> {
        self.axes
            .iter()
            .map(|a| [a[0].bounds()[0], a[1].bounds()[1]])
            .collect()
    }

    /// The smallest box holding all of `boxes` (not empty).
    fn hull<'a>(boxes: impl IntoIterator<Item = &'a NdBox>) -> NdBox {
        let mut it = boxes.into_iter();
        let mut h = it.next().expect("a hull of some boxes").clone();
        for b in it {
            for (x, y) in h.axes.iter_mut().zip(&b.axes) {
                if y[0] < x[0] {
                    x[0] = y[0];
                }
                if y[1] > x[1] {
                    x[1] = y[1];
                }
            }
        }
        h
    }

    /// The geometric order of results: lower corners, then upper,
    /// lexicographically.
    fn order(&self, o: &NdBox) -> Ordering {
        let lo = self
            .axes
            .iter()
            .map(|a| a[0])
            .cmp(o.axes.iter().map(|a| a[0]));
        lo.then_with(|| {
            self.axes
                .iter()
                .map(|a| a[1])
                .cmp(o.axes.iter().map(|a| a[1]))
        })
    }
}

/// A certified simple root: exactly one root of the system in the union of
/// `boxes`, of multiplicity 1 (the Jacobian regular there).
#[derive(Clone, Debug, PartialEq)]
pub struct NdRoot {
    /// One box certified by itself, or a set of touching boxes certified
    /// together (a root on or beside a face they share); in the geometric
    /// order.
    pub boxes: Vec<NdBox>,
    /// `[a, b]` per variable, `f64`s with the root between them: the
    /// narrowed box, rounded outwards.
    pub near: Axes<[f64; 2]>,
    /// The sign of the Jacobian's determinant at the root (the root's
    /// local degree): `Greater` where it is positive.
    pub orient: Ordering,
}

/// What one precision level can't separate: the roots in the union of
/// `boxes`, none certified.
#[derive(Clone, Debug, PartialEq)]
pub struct NdCluster {
    /// Touching boxes, in the geometric order.
    pub boxes: Vec<NdBox>,
    /// The smallest box holding them.
    pub hull: NdBox,
    /// `Some(m)` when every box contracts: at most `m` roots, each simple
    /// (on or near a face no certificate could place them on one side of,
    /// a domain boundary among them); `None` without a bound.
    pub simple: Option<u32>,
    /// Whether every form is certified within its target on every box
    /// (`|fᵢ| ≤ τᵢ`): a tie at the target, for the caller's rule.
    pub within_target: bool,
    /// Whether part of it reached the depth limit unsettled; if not, it is
    /// as far as subdivision gets at this precision and target.
    pub at_depth: bool,
    /// Whether it reaches the boundary of `[0, 1]ᵈ`, where a root may be.
    pub boundary: bool,
}

/// One result of [`isolate_nd`].
#[derive(Clone, Debug, PartialEq)]
pub enum NdFound {
    Root(NdRoot),
    Cluster(NdCluster),
}

impl NdFound {
    /// The result's boxes.
    pub fn boxes(&self) -> &[NdBox] {
        match self {
            NdFound::Root(r) => &r.boxes,
            NdFound::Cluster(c) => &c.boxes,
        }
    }
}

/// The roots of a system on `[0, 1]ᵈ`.
#[derive(Clone, Debug, PartialEq)]
pub struct NdRoots {
    /// In the geometric order of their hulls; every root is in exactly
    /// one result, and results meet only at points certified free of roots.
    pub found: Vec<NdFound>,
    pub work: Work,
}

/// Why [`isolate_nd`] gave no result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NdFail {
    /// Not `d` forms in the same `d ≤` [`MAX_VARIABLES`] variables with
    /// `d` targets, each `≥ 0`.
    Shape,
    /// The subdivision passed [`Limits::max_splits`] halvings. Its work is
    /// a bound from the cap alone, the same whichever side the halvings
    /// were on: the cap's halvings, `2·max_splits + 1` tests and that
    /// many tests' most Krawczyk steps.
    OutOfWork(OutOfWork),
}

/// The roots of the system `forms` (module docs): `d` forms in `d`
/// variables, `targets[i]` the value below which `|fᵢ|` is a tie for the
/// caller.
pub fn isolate_nd<S: Scalar>(
    forms: &[NdTensor<S>],
    targets: &[f64],
    limits: Limits,
) -> Result<NdRoots, NdFail> {
    let d = forms.first().map_or(0, NdTensor::dims);
    if d == 0
        || d > MAX_VARIABLES
        || forms.len() != d
        || targets.len() != d
        || forms.iter().any(|f| f.dims() != d)
        || targets.iter().any(|t| t.is_nan() || *t < 0.0)
    {
        return Err(NdFail::Shape);
    }
    let max_depth = limits.max_depth.min(MAX_ROOT_DEPTH);
    // A variable no form depends on never splits.
    let split: Axes<usize> = (0..d)
        .filter(|&a| forms.iter().any(|f| f.degrees()[a] > 0))
        .collect();
    let per_box = (1u32 << split.len()) - 1;
    let mut work = Work::default();
    let mut halvings = 0u32;
    let mut leaves = Vec::new();
    let mut stack = vec![(Cell::root(d), forms.to_vec())];
    while let Some((cell, f)) = stack.pop() {
        work.tests = work.tests.saturating_add(1);
        let can_split = split.first().is_some_and(|&a| cell.k[a] < max_depth);
        match classify(&f, targets, can_split, &mut work) {
            Verdict::Empty => {}
            Verdict::Split => {
                if halvings.saturating_add(per_box) > limits.max_splits {
                    let tests = limits.max_splits.saturating_mul(2).saturating_add(1);
                    return Err(NdFail::OutOfWork(OutOfWork {
                        work: Work {
                            splits: limits.max_splits,
                            tests,
                            steps: tests.saturating_mul(NARROW_STEPS + INFLATIONS),
                        },
                    }));
                }
                halvings += per_box;
                let mut parts = vec![(cell, f)];
                for &a in &split {
                    parts = parts
                        .into_iter()
                        .flat_map(|(c, f)| {
                            let (l, r): (Vec<_>, Vec<_>) = f
                                .iter()
                                .map(|g| {
                                    let [l, r] = g.split_half(a);
                                    (l, r)
                                })
                                .unzip();
                            [(c.child(a, 0), l), (c.child(a, 1), r)]
                        })
                        .collect();
                }
                // The first child on top.
                stack.extend(parts.into_iter().rev());
            }
            Verdict::Settled(kind) => leaves.push(Leaf { cell, kind }),
        }
    }
    work.splits = work.splits.saturating_add(halvings);
    let found = resolve(forms, leaves, &mut work);
    Ok(NdRoots { found, work })
}

/// A dyadic cell: per variable its level `k` and index `j`,
/// `[j/2ᵏ, (j + 1)/2ᵏ]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Cell {
    pub(super) k: Axes<u32>,
    pub(super) j: Axes<u64>,
}

impl Cell {
    pub(super) fn root(d: usize) -> Cell {
        Cell {
            k: smallvec![0; d],
            j: smallvec![0; d],
        }
    }

    /// The half `side` (0 low, 1 high) in variable `a`.
    pub(super) fn child(&self, a: usize, side: u64) -> Cell {
        let mut c = self.clone();
        c.k[a] += 1;
        c.j[a] = 2 * c.j[a] + side;
        c
    }

    pub(super) fn to_box(&self) -> NdBox {
        let dy = |j: u64, k: u32| Dyadic::new(j, k).expect("a cell is in [0, 1]");
        NdBox {
            axes: self
                .k
                .iter()
                .zip(&self.j)
                .map(|(&k, &j)| [dy(j, k), dy(j + 1, k)])
                .collect(),
        }
    }

    /// The lattice box `x` of this cell in `t`: bounds outwards
    /// (`outer`), else inwards.
    pub(super) fn bounds(&self, x: &Lat, outer: bool) -> Axes<[f64; 2]> {
        let at = |k: u32, j: u64, l: u64, high: bool| {
            let n = (u128::from(j) << LATTICE) + u128::from(l);
            let [lo, hi] = int_bounds(n);
            // Exact: `n·2^-(53 + k)` is 0 or at least `2⁻¹¹⁶`.
            (if high { hi } else { lo }) * pow2_neg(LATTICE + k)
        };
        self.k
            .iter()
            .zip(&self.j)
            .zip(x)
            .map(|((&k, &j), &[lo, hi])| [at(k, j, lo, !outer), at(k, j, hi, outer)])
            .collect()
    }
}

/// A settled box's kind.
#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// Exactly one root, simple, in the narrowed lattice box.
    Root { x: Lat, orient: Ordering },
    /// At most one root, simple, in the narrowed lattice box.
    Pending(Lat),
    /// Not separated.
    Open(Why),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    /// Every form within its target.
    Target,
    /// Every coefficient uncertain.
    Floor,
    /// At the depth limit.
    Depth,
}

struct Leaf {
    cell: Cell,
    kind: Kind,
}

enum Verdict {
    Empty,
    Split,
    Settled(Kind),
}

/// A box's verdict from its forms (on it as `[0, 1]ᵈ`).
fn classify<S: Scalar>(
    forms: &[NdTensor<S>],
    targets: &[f64],
    can_split: bool,
    work: &mut Work,
) -> Verdict {
    let hulls: Vec<_> = forms.iter().map(NdTensor::hull).collect();
    if hulls.iter().any(|h| h.lo > 0.0 || h.hi < 0.0) {
        return Verdict::Empty;
    }
    let d = forms.len();
    let full: Lat = smallvec![[0, SIDE]; d];
    let mut sys = Restrict::new(forms);
    let (x, verdict) = narrow(&mut sys, full.clone(), &full, true, work);
    match verdict {
        Narrowed::Empty => return Verdict::Empty,
        Narrowed::Certified { orient, .. } => {
            return Verdict::Settled(Kind::Root { x, orient });
        }
        Narrowed::Unsettled { contracting: true } => {
            let touches = x.iter().any(|&[lo, hi]| lo == 0 || hi == SIDE);
            return if touches || !can_split {
                Verdict::Settled(Kind::Pending(x))
            } else {
                Verdict::Split
            };
        }
        Narrowed::Unsettled { contracting: false } => {}
    }
    let within = hulls
        .iter()
        .zip(targets)
        .all(|(h, &t)| h.lo >= -t && h.hi <= t);
    if within {
        return Verdict::Settled(Kind::Open(Why::Target));
    }
    if forms
        .iter()
        .all(|f| f.coefs().iter().all(|c| c.sign().is_none()))
    {
        return Verdict::Settled(Kind::Open(Why::Floor));
    }
    if !can_split {
        return Verdict::Settled(Kind::Open(Why::Depth));
    }
    Verdict::Split
}

/// The results from the settled boxes (module docs, Results).
fn resolve<S: Scalar>(forms: &[NdTensor<S>], leaves: Vec<Leaf>, work: &mut Work) -> Vec<NdFound> {
    let boxes: Vec<NdBox> = leaves.iter().map(|l| l.cell.to_box()).collect();
    let mut found: Vec<(NdBox, NdFound)> = Vec::new();
    // Each root box's narrowed box, outwards.
    let roots: Vec<(usize, Axes<[f64; 2]>)> = leaves
        .iter()
        .enumerate()
        .filter_map(|(i, l)| match &l.kind {
            Kind::Root { x, .. } => Some((i, l.cell.bounds(x, true))),
            _ => None,
        })
        .collect();
    // The joined sets of the other boxes.
    let rest: Vec<usize> = (0..leaves.len())
        .filter(|&i| !matches!(leaves[i].kind, Kind::Root { .. }))
        .collect();
    let groups = touching(&rest, &boxes);
    let mut results = Vec::new();
    for g in &groups {
        let all_pending = g
            .iter()
            .all(|&i| matches!(leaves[i].kind, Kind::Pending(_)));
        let mut outcome = Joined::Cluster;
        if all_pending && let Some(c) = certify(forms, g, &leaves, work) {
            outcome = match c {
                Certify::Empty => Joined::Empty,
                Certify::Root(c) => {
                    let Certified { near, cert, orient } = *c;
                    judge(g, &groups, &boxes, &roots, &cert, near, orient)
                }
            };
        }
        results.push(outcome);
    }
    for (g, outcome) in groups.iter().zip(results) {
        let mut bs: Vec<NdBox> = g.iter().map(|&i| boxes[i].clone()).collect();
        bs.sort_by(NdBox::order);
        let hull = NdBox::hull(&bs);
        match outcome {
            // No root, or the root a root box holds.
            Joined::Empty | Joined::Holds => {}
            Joined::Root { near, orient } => found.push((
                hull,
                NdFound::Root(NdRoot {
                    boxes: bs,
                    near,
                    orient,
                }),
            )),
            Joined::Cluster => {
                let kinds = || g.iter().map(|&i| &leaves[i].kind);
                let simple = kinds()
                    .all(|k| matches!(k, Kind::Pending(_)))
                    .then(|| u32::try_from(g.len()).unwrap_or(u32::MAX));
                let within_target = kinds().all(|k| *k == Kind::Open(Why::Target));
                let at_depth = kinds().any(|k| *k == Kind::Open(Why::Depth));
                let boundary = hull.on_boundary();
                found.push((
                    hull.clone(),
                    NdFound::Cluster(NdCluster {
                        boxes: bs,
                        hull,
                        simple,
                        within_target,
                        at_depth,
                        boundary,
                    }),
                ));
            }
        }
    }
    for (i, near) in &roots {
        let Kind::Root { orient, .. } = leaves[*i].kind else {
            unreachable!("a root box")
        };
        found.push((
            boxes[*i].clone(),
            NdFound::Root(NdRoot {
                boxes: vec![boxes[*i].clone()],
                near: near.clone(),
                orient,
            }),
        ));
    }
    found.sort_by(|a, b| a.0.order(&b.0));
    found.into_iter().map(|(_, f)| f).collect()
}

/// What a joined set of boxes is.
enum Joined {
    /// No root (certified so on its narrowed boxes).
    Empty,
    /// No root: the box certified on it holds a root box's root.
    Holds,
    Root {
        near: Axes<[f64; 2]>,
        orient: Ordering,
    },
    Cluster,
}

/// A set's root certified on `cert` (bounds outwards and inwards), checked
/// against the other results: meeting another set's box, or part of a root
/// box's narrowed box, it is a cluster; holding a root box's whole
/// narrowed box, that root is the one, and the set has none.
fn judge(
    g: &[usize],
    groups: &[Vec<usize>],
    boxes: &[NdBox],
    roots: &[(usize, Axes<[f64; 2]>)],
    cert: &[Axes<[f64; 2]>; 2],
    near: Axes<[f64; 2]>,
    orient: Ordering,
) -> Joined {
    let [outer, inner] = cert;
    let meets = |b: &[[f64; 2]]| {
        outer
            .iter()
            .zip(b)
            .all(|(o, x)| o[0] <= x[1] && x[0] <= o[1])
    };
    let holds = |b: &[[f64; 2]]| {
        inner
            .iter()
            .zip(b)
            .all(|(i, x)| i[0] <= x[0] && x[1] <= i[1])
    };
    for other in groups {
        if other.as_slice() == g {
            continue;
        }
        if other.iter().any(|&i| meets(&boxes[i].bounds())) {
            return Joined::Cluster;
        }
    }
    for (_, n) in roots {
        if meets(n) {
            return if holds(n) {
                Joined::Holds
            } else {
                Joined::Cluster
            };
        }
    }
    Joined::Root { near, orient }
}

/// The maximal sets of the boxes `idx` linked by meeting, each in `idx`
/// order, the sets in the order of their first box.
fn touching(idx: &[usize], boxes: &[NdBox]) -> Vec<Vec<usize>> {
    let n = idx.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    // Sweep along the first variable.
    let mut by_lo: Vec<usize> = (0..n).collect();
    by_lo.sort_by(|&a, &b| boxes[idx[a]].axes[0][0].cmp(&boxes[idx[b]].axes[0][0]));
    for (s, &a) in by_lo.iter().enumerate() {
        let top = boxes[idx[a]].axes[0][1];
        for &b in &by_lo[s + 1..] {
            if boxes[idx[b]].axes[0][0] > top {
                break;
            }
            if boxes[idx[a]].meets(&boxes[idx[b]]) {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra != rb {
                    // The smaller index leads, so sets keep `idx` order.
                    let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
                    parent[hi] = lo;
                }
            }
        }
    }
    let mut sets: Vec<Vec<usize>> = Vec::new();
    let mut slot = vec![usize::MAX; n];
    for (i, &b) in idx.iter().enumerate() {
        let r = find(&mut parent, i);
        if slot[r] == usize::MAX {
            slot[r] = sets.len();
            sets.push(Vec::new());
        }
        sets[slot[r]].push(b);
    }
    sets
}

/// A joined set of pending boxes certified together.
enum Certify {
    /// None of them holds a root.
    Empty,
    /// One root.
    Root(Box<Certified>),
}

/// A set's certified root: in the box `cert` (outwards, inwards),
/// narrowed to `near`.
struct Certified {
    near: Axes<[f64; 2]>,
    cert: [Axes<[f64; 2]>; 2],
    orient: Ordering,
}

/// Krawczyk's operator on the inflated hull of the pending boxes `g`'
/// narrowed boxes, in the smallest dyadic cell holding it (module docs,
/// Results); `None` where it doesn't settle or a box lies more than 53
/// levels below that cell.
fn certify<S: Scalar>(
    forms: &[NdTensor<S>],
    g: &[usize],
    leaves: &[Leaf],
    work: &mut Work,
) -> Option<Certify> {
    let d = forms.len();
    let mut cell = Cell::root(d);
    let mut start: Lat = smallvec![[0, 0]; d];
    for a in 0..d {
        // The smallest cell holding every box along `a`.
        let mut k = g.iter().map(|&i| leaves[i].cell.k[a]).min()?;
        let at = |i: usize, k: u32| leaves[i].cell.j[a] >> (leaves[i].cell.k[a] - k);
        while g.iter().any(|&i| at(i, k) != at(g[0], k)) {
            k -= 1;
        }
        let j = at(g[0], k);
        // The hull of the narrowed boxes on its lattice.
        let (mut lo, mut hi) = (u128::MAX, 0u128);
        for &i in g {
            let Kind::Pending(x) = &leaves[i].kind else {
                unreachable!("a pending box")
            };
            let c = &leaves[i].cell;
            let deep = c.k[a] - k;
            if deep > LATTICE {
                return None;
            }
            let base = u128::from(c.j[a] - (j << deep)) << LATTICE;
            let l = (base + u128::from(x[a][0])) >> deep;
            let h = (base + u128::from(x[a][1])).div_ceil(1u128 << deep);
            lo = lo.min(l);
            hi = hi.max(h);
        }
        // Inflated, the cell widened while the inflated hull reaches past
        // it.
        let (mut k, mut j) = (k, j);
        let (mut lo, mut hi) = (lo as i128, hi as i128);
        loop {
            let pad = (hi - lo) / 2 + 1;
            let (l, h) = (lo - pad, hi + pad);
            let side = i128::from(SIDE);
            if (l >= 0 && h <= side) || k == 0 {
                start[a] = [l.max(0) as u64, h.min(side) as u64];
                break;
            }
            let shift = i128::from(j & 1) * side;
            lo = (lo + shift).div_euclid(2);
            hi = (hi + shift + 1).div_euclid(2);
            j >>= 1;
            k -= 1;
        }
        cell.k[a] = k;
        cell.j[a] = j;
    }
    let mut on = forms.to_vec();
    for a in 0..d {
        on = descend(&on, a, (0, 0), (cell.k[a], cell.j[a]), work);
    }
    let full: Lat = smallvec![[0, SIDE]; d];
    let mut sys = Restrict::new(&on);
    let (x, verdict) = narrow(&mut sys, start, &full, false, work);
    match verdict {
        Narrowed::Empty => Some(Certify::Empty),
        Narrowed::Certified { orient, cert } => Some(Certify::Root(Box::new(Certified {
            near: cell.bounds(&x, true),
            cert: [cell.bounds(&cert, true), cell.bounds(&cert, false)],
            orient,
        }))),
        Narrowed::Unsettled { .. } => None,
    }
}

/// The outcome of [`narrow`].
pub(super) enum Narrowed {
    /// No root in the starting box.
    Empty,
    /// Exactly one root, simple, in `cert` (and in the narrowed box), of
    /// this orientation.
    Certified { orient: Ordering, cert: Lat },
    /// Neither; whether the first step contracted.
    Unsettled { contracting: bool },
}

/// Krawczyk's iterates `X ∩ K(X)` from `x` while they shrink, at most
/// [`NARROW_STEPS`], certifying a root where `K(X)` falls inside `X`'s
/// interior, or, once they stop shrinking, inside the interior of `X`
/// inflated, doubled up to [`INFLATIONS`] times (within `clip`). Every root of `x` stays in the narrowed box.
/// For a whole box (`leaf`), a first step that neither certifies nor
/// contracts stops it: only then does a later certificate on a sub-box
/// mean one root in the whole box.
pub(super) fn narrow<S: Scalar>(
    sys: &mut Restrict<'_, S>,
    mut x: Lat,
    clip: &Lat,
    leaf: bool,
    work: &mut Work,
) -> (Lat, Narrowed) {
    let d = x.len();
    let mut cert: Option<(Ordering, Lat)> = None;
    let mut contracting = false;
    let mut inflated = false;
    for n in 0..NARROW_STEPS {
        let Some(s) = step(sys, &x, work) else { break };
        if s.disjoint() {
            return (x, Narrowed::Empty);
        }
        if n == 0 {
            contracting = s.q < CONTRACTING;
        }
        if cert.is_none() && s.inside() {
            cert = Some((orient(&s.mid, d), x.clone()));
        }
        if leaf && cert.is_none() && !contracting {
            break;
        }
        let Some(nx) = s.meet(&x) else {
            return (x, Narrowed::Empty);
        };
        if nx != x {
            x = nx;
            continue;
        }
        if cert.is_some() || inflated {
            break;
        }
        inflated = true;
        // Widen until the operator's own rounding fits inside.
        let mut y = x.clone();
        for _ in 0..INFLATIONS {
            let z = inflate(&y, clip);
            if z == y {
                break;
            }
            y = z;
            let Some(t) = step(sys, &y, work) else { break };
            if t.disjoint() {
                break;
            }
            if t.inside() {
                cert = Some((orient(&t.mid, d), y.clone()));
                if let Some(ny) = t.meet(&y) {
                    x = ny;
                }
                break;
            }
        }
        if cert.is_none() {
            break;
        }
    }
    let verdict = match cert {
        Some((orient, cert)) => Narrowed::Certified { orient, cert },
        None => Narrowed::Unsettled { contracting },
    };
    (x, verdict)
}

/// `x` widened by half its width and one lattice unit on each side,
/// within `clip`.
fn inflate(x: &Lat, clip: &Lat) -> Lat {
    x.iter()
        .zip(clip)
        .map(|(&[lo, hi], &[cl, ch])| {
            let pad = (hi - lo) / 2 + 1;
            [
                lo.saturating_sub(pad).max(cl),
                hi.saturating_add(pad).min(ch),
            ]
        })
        .collect()
}

/// One Krawczyk step on a lattice box: `K(X) − c` and `X − c` per
/// variable, `c` the middle, the contraction bound and the middle
/// Jacobian.
struct Step {
    off: Axes<[f64; 2]>,
    rel: Axes<[f64; 2]>,
    c: Axes<u64>,
    q: f64,
    mid: Vec<f64>,
}

impl Step {
    /// `K(X)` and `X` apart: no root in `X`.
    fn disjoint(&self) -> bool {
        self.off
            .iter()
            .zip(&self.rel)
            .any(|(k, x)| k[1] < x[0] || k[0] > x[1])
    }

    /// `K(X)` inside `X`'s interior: exactly one root in `X`.
    fn inside(&self) -> bool {
        self.off
            .iter()
            .zip(&self.rel)
            .all(|(k, x)| k[0] > x[0] && k[1] < x[1])
    }

    /// `X ∩ K(X)` on the lattice (`K` rounded outwards to it); `None` when
    /// empty.
    fn meet(&self, x: &Lat) -> Option<Lat> {
        let mut out = Lat::new();
        for ((&[lo, hi], k), &c) in x.iter().zip(&self.off).zip(&self.c) {
            let l = to_lattice(k[0], c, false).max(i128::from(lo));
            let h = to_lattice(k[1], c, true).min(i128::from(hi));
            if l > h {
                return None;
            }
            out.push([l as u64, h as u64]);
        }
        Some(out)
    }
}

/// `c + v·2⁵³` rounded down (or `up`) to an integer, clamped far outside
/// the lattice for a value that isn't finite.
fn to_lattice(v: f64, c: u64, up: bool) -> i128 {
    const FAR: f64 = (1u64 << 60) as f64;
    let s = v * SIDE as f64;
    if s.is_nan() {
        return if up { i128::MAX } else { i128::MIN };
    }
    let r = if up { s.ceil() } else { s.floor() };
    let r = r.clamp(-FAR, FAR);
    r as i128 + i128::from(c)
}

/// The even of the lattice points nearest the middle of `[lo, hi]`: the
/// mirrored box's is the mirrored point.
fn middle(lo: u64, hi: u64) -> u64 {
    let s = lo + hi;
    let m = s / 2;
    if s % 2 == 0 || m % 2 == 0 { m } else { m + 1 }
}

/// Krawczyk's operator on the lattice box `x` of `sys`'s cell.
fn step<S: Scalar>(sys: &mut Restrict<'_, S>, x: &Lat, work: &mut Work) -> Option<Step> {
    work.steps = work.steps.saturating_add(1);
    let d = x.len();
    let unit = pow2_neg(LATTICE);
    let c: Axes<u64> = x.iter().map(|&[lo, hi]| middle(lo, hi)).collect();
    let like = &sys.forms[0].coefs()[0];
    // Lattice points are floats exactly, `1 − c` too.
    let pt: Vec<S> = c
        .iter()
        .map(|&ci| S::from_f64(ci as f64 * unit, like))
        .collect();
    let fc: Vec<[f64; 2]> = sys
        .forms
        .iter()
        .map(|f| f.eval(&pt).expect("d variables").enclose())
        .collect();
    let jac = sys.jacobian(x, work);
    let mid: Vec<f64> = jac.iter().map(|&[lo, hi]| 0.5 * lo + 0.5 * hi).collect();
    let y = invert(&mid, d)?;
    // `M = I − Y·J`.
    let mut m = vec![[0.0, 0.0]; d * d];
    for i in 0..d {
        for j in 0..d {
            let mut s = [0.0, 0.0];
            for k in 0..d {
                s = add(s, mul([y[i * d + k]; 2], jac[k * d + j]));
            }
            let e = if i == j { 1.0 } else { 0.0 };
            m[i * d + j] = sub([e, e], s);
        }
    }
    let mag = |v: [f64; 2]| {
        let (a, b) = (v[0].abs(), v[1].abs());
        if b > a { b } else { a }
    };
    let mut q = 0.0;
    for i in 0..d {
        let row = (0..d).fold(0.0, |acc, j| add_up(acc, mag(m[i * d + j])));
        // A NaN row is no bound.
        if row.is_nan() {
            q = f64::INFINITY;
        } else if row > q {
            q = row;
        }
    }
    // Lattice differences are exact.
    let rel: Axes<[f64; 2]> = x
        .iter()
        .zip(&c)
        .map(|(&[lo, hi], &ci)| {
            [
                (lo as f64 - ci as f64) * unit,
                (hi as f64 - ci as f64) * unit,
            ]
        })
        .collect();
    let off = (0..d)
        .map(|i| {
            let mut yf = [0.0, 0.0];
            for k in 0..d {
                yf = add(yf, mul([y[i * d + k]; 2], fc[k]));
            }
            let mut s = sub([0.0, 0.0], yf);
            for j in 0..d {
                s = add(s, mul(m[i * d + j], rel[j]));
            }
            s
        })
        .collect();
    Some(Step {
        off,
        rel,
        c,
        q,
        mid,
    })
}

/// The inverse of the `d × d` matrix `a` (row-major) by Gauss–Jordan
/// elimination with partial pivoting, in floats; `None` for a zero pivot or
/// a value that isn't finite. Negating a column negates the inverse's row,
/// to the bit.
fn invert(a: &[f64], d: usize) -> Option<Vec<f64>> {
    if a.iter().any(|x| !x.is_finite()) {
        return None;
    }
    let mut m = a.to_vec();
    let mut inv = vec![0.0; d * d];
    for i in 0..d {
        inv[i * d + i] = 1.0;
    }
    for col in 0..d {
        let mut p = col;
        for r in col + 1..d {
            if m[r * d + col].abs() > m[p * d + col].abs() {
                p = r;
            }
        }
        if m[p * d + col] == 0.0 {
            return None;
        }
        if p != col {
            for j in 0..d {
                m.swap(p * d + j, col * d + j);
                inv.swap(p * d + j, col * d + j);
            }
        }
        let piv = m[col * d + col];
        for j in 0..d {
            m[col * d + j] /= piv;
            inv[col * d + j] /= piv;
        }
        for r in 0..d {
            let f = m[r * d + col];
            if r == col || f == 0.0 {
                continue;
            }
            for j in 0..d {
                m[r * d + j] -= f * m[col * d + j];
                inv[r * d + j] -= f * inv[col * d + j];
            }
        }
    }
    inv.iter().all(|x| x.is_finite()).then_some(inv)
}

/// The sign of the determinant of the `d × d` float matrix `m`, exactly.
fn orient(m: &[f64], d: usize) -> Ordering {
    let mut a: Vec<Rational> = m.iter().map(|&x| rat(x)).collect();
    let mut sign = Ordering::Greater;
    for col in 0..d {
        let Some(p) = (col..d).find(|&r| a[r * d + col] != 0u32) else {
            return Ordering::Equal;
        };
        if p != col {
            for j in 0..d {
                a.swap(p * d + j, col * d + j);
            }
            sign = sign.reverse();
        }
        let piv = a[col * d + col].clone();
        if piv < 0u32 {
            sign = sign.reverse();
        }
        for r in col + 1..d {
            let f = &a[r * d + col] / &piv;
            for j in col..d {
                let t = &f * &a[col * d + j];
                a[r * d + j] -= t;
            }
        }
    }
    sign
}

/// Forms restricted to a sub-cell (per variable: level, index).
type Piece<S> = (Axes<(u32, u64)>, Vec<NdTensor<S>>);

/// The forms of a cell and their restrictions to sub-boxes of it, keeping
/// the last step's pieces for the next.
pub(super) struct Restrict<'a, S> {
    forms: &'a [NdTensor<S>],
    /// The last step's pieces: per variable the cell (level, index below
    /// `forms`' cell) and the forms restricted to them.
    pieces: Vec<Piece<S>>,
}

impl<'a, S: Scalar> Restrict<'a, S> {
    pub(super) fn new(forms: &'a [NdTensor<S>]) -> Self {
        Restrict {
            forms,
            pieces: Vec::new(),
        }
    }

    /// `J(X)` for the lattice box `x`, row-major (form by variable): per
    /// variable the cells of the deepest level holding `x` in at most two
    /// ([`cover`]), the forms restricted to every combination of them (by
    /// halves, from the last step's piece holding it where there is one, so
    /// a narrowing box descends once), the hull of their partial
    /// derivatives there, scaled to the cell.
    pub(super) fn jacobian(&mut self, x: &Lat, work: &mut Work) -> Vec<[f64; 2]> {
        let d = x.len();
        let covers: Axes<(u32, u64, u64)> = x.iter().map(|&[lo, hi]| cover(lo, hi)).collect();
        // Every combination of the cells, the first variable slowest.
        let mut combos: Vec<Axes<(u32, u64)>> = vec![Axes::new()];
        for &(l, first, last) in &covers {
            combos = combos
                .into_iter()
                .flat_map(|c| {
                    (first..=last).map(move |i| {
                        let mut c = c.clone();
                        c.push((l, i));
                        c
                    })
                })
                .collect();
        }
        // Their common ancestor, from the deepest kept piece holding it.
        let anc: Axes<(u32, u64)> = covers
            .iter()
            .map(|&(l, a, b)| {
                let up = 64 - (a ^ b).leading_zeros();
                (l - up, a >> up)
            })
            .collect();
        let base: Piece<S> = (anc.clone(), self.restricted(&anc, None, work));
        let mut pieces = Vec::with_capacity(combos.len() + 1);
        for cells in combos {
            let f = self.restricted(&cells, Some(&base), work);
            pieces.push((cells, f));
        }
        let mut jac = vec![[f64::INFINITY, f64::NEG_INFINITY]; d * d];
        for (_, p) in &pieces {
            for (i, form) in p.iter().enumerate() {
                for (j, &(l, _, _)) in covers.iter().enumerate() {
                    let h = form.derivative(j).hull();
                    // `∂/∂x = 2ˡ·∂/∂(the cell's own variable)`, exactly.
                    let s = pow2(l);
                    let e = &mut jac[i * d + j];
                    if h.lo * s < e[0] {
                        e[0] = h.lo * s;
                    }
                    if h.hi * s > e[1] {
                        e[1] = h.hi * s;
                    }
                }
            }
        }
        pieces.push(base);
        self.pieces = pieces;
        jac
    }

    /// The forms restricted to `cells`, descended from the deepest of the
    /// kept pieces and `other` holding them, else from the forms.
    fn restricted(
        &self,
        cells: &Axes<(u32, u64)>,
        other: Option<&Piece<S>>,
        work: &mut Work,
    ) -> Vec<NdTensor<S>> {
        let held = |from: &Axes<(u32, u64)>| from.iter().zip(cells).all(|(&c, &n)| holds(c, n));
        let depth = |from: &Axes<(u32, u64)>| from.iter().map(|c| c.0).sum::<u32>();
        let mut best: Option<&Piece<S>> = None;
        for p in self.pieces.iter().chain(other) {
            if held(&p.0) && best.is_none_or(|b| depth(&p.0) > depth(&b.0)) {
                best = Some(p);
            }
        }
        let root: Axes<(u32, u64)> = smallvec![(0, 0); cells.len()];
        let (from, mut f) = match best {
            Some((from, f)) => (from.clone(), f.clone()),
            None => (root, self.forms.to_vec()),
        };
        for a in 0..cells.len() {
            f = descend(&f, a, from[a], cells[a], work);
        }
        f
    }
}

/// `2ˡ` exactly, `l ≤ 1023`.
fn pow2(l: u32) -> f64 {
    f64::from_bits((1023 + u64::from(l)) << 52)
}

/// Whether the cell `c` (level, index) holds the cell `n`.
fn holds(c: (u32, u64), n: (u32, u64)) -> bool {
    n.0 >= c.0 && n.1 >> (n.0 - c.0) == c.1
}

/// The deepest level `l ≤ 53` whose cells of width `2^(53 − l)` lattice
/// units hold `[lo, hi]` in at most two adjacent ones, and those cells'
/// first and last index: the cells of the floor of `lo` and the ceiling of
/// `hi` less one, either order (a point on a cells' boundary takes both),
/// so the mirrored interval takes the mirrored cells.
pub(super) fn cover(lo: u64, hi: u64) -> (u32, u64, u64) {
    let w = hi - lo;
    let mut l = LATTICE;
    while l > 0 && w > 1u64 << (LATTICE - l) {
        l -= 1;
    }
    let s = LATTICE - l;
    let a = i128::from(lo >> s);
    let b = i128::from(hi.div_ceil(1u64 << s)) - 1;
    let top = (1i128 << l) - 1;
    let clamp = |v: i128| v.clamp(0, top) as u64;
    (l, clamp(a.min(b)), clamp(a.max(b)))
}

/// The forms of cell `from` (level, index; below the forms' own cell)
/// restricted to its sub-cell `to` in variable `axis`, by halves.
fn descend<S: Scalar>(
    forms: &[NdTensor<S>],
    axis: usize,
    from: (u32, u64),
    to: (u32, u64),
    work: &mut Work,
) -> Vec<NdTensor<S>> {
    debug_assert!(holds(from, to));
    let mut f = forms.to_vec();
    for t in from.0..to.0 {
        let high = (to.1 >> (to.0 - 1 - t)) & 1 == 1;
        work.splits = work.splits.saturating_add(1);
        f = f.iter().map(|g| g.half(axis, high)).collect();
    }
    f
}

#[cfg(test)]
mod tests;
