//! Tangency clusters: the connected groups of leaves where the operands
//! run within `b_K` of each other, and the order they are processed in.
//!
//! **Clusters** ([`clusters`]). The input is every certified leaf of
//! every face pair (a box of a face of the first operand against a box
//! of a face of the second, [`ClusterLeaf`]: flat leaves and curved
//! leaves against a quadric alike, or both in one run as `&dyn
//! ClusterLeaf`). Only leaves certified Near (co-oriented) or Contact (opposed) take part; an Empty or Simple
//! leaf is never snapped, and two disjoint flat pieces always certify
//! Empty, however close, so faces apart form no cluster. A cluster is a
//! connected component of the Near leaves, or of the Contact leaves,
//! over all face pairs: two leaves of one kind are adjacent when their
//! boxes on one face **abut** in that face's UV, that is, the two closed
//! boxes share a point (a side, part of a side or a corner) in the
//! face's global UV (tile index plus the box's local extents, compared
//! exactly as integers over `2⁵²`). A Near and a Contact leaf never
//! join: where they abut, the two clusters stay apart. No distance is
//! compared, and nothing here depends on the order the leaves are given
//! in or on the faces' ids.
//!
//! **Order.** The geometric order of
//! [`ClusterKey`], never ids:
//! - a leaf's key: the least corner of its two pieces' world boxes (each
//!   rounded outward), then the lesser of its two faces' keys, then its
//!   face pair (the first operand's face key, then the second's);
//! - a cluster's key: the same over all its leaves, which is each part
//!   of the key the least over its leaves' (the least corner, the least
//!   face key, the least pair). Before a target is chosen every face of
//!   the cluster may become one, so the face keys of both operands'
//!   faces are its candidate targets.
//!
//! Leaves within a cluster are in leaf-key order, clusters in
//! cluster-key order. Ids break only exact ties of the keys: a leaf's
//! pair of boxes (operand, face id, tile and extents), then its position
//! in the input; a cluster's first leaf's. There the choice changes no
//! geometry and no name. The order is work order: disjoint clusters
//! commute, since the moved-vertex joins run after every snap.
//!
//! **Merging** ([`merge_sharing`]). Two clusters of one kind whose rim
//! windows (the elements a cluster's rewrite may place) share an element
//! are merged, so no element is placed by two clusters; the merge is
//! transitive. Two of different kinds are never merged: the later one in
//! the order reads the earlier one's placement as a fixed anchor. A
//! merged cluster holds its parts' leaves, in leaf-key order, and its
//! key is the least of its parts' keys, part by part.
//!
//! **Work.** Charged on the operation's budget in passes: the leaves'
//! keys (an item per leaf), the abutment (an item per face and kind,
//! charging a unit per box and per pair of boxes compared), the clusters
//! (an item per cluster, a unit per leaf); a merge is one item charging
//! a unit per leaf and per window element.

use std::collections::BTreeMap;

use varde_decide::bezier::{Dir, Dyadic};
use varde_decide::count::boxes::{BoxKey, Extent, MAX_BOX_DEPTH};
use varde_decide::ids::FaceId;
use varde_decide::leaves::flat::{self, Kind};
use varde_decide::leaves::near_quadric;
use varde_decide::order::{ClusterKey, KeyError};
use varde_decide::records::keys::Operand;

use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;

/// The kind of a cluster's leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tangency {
    /// Co-oriented: the normals in a common open hemisphere.
    Near,
    /// Opposed: the first operand's normals and the second's reversed in
    /// a common open hemisphere.
    Contact,
}

impl Tangency {
    /// The tangency of a leaf of kind `kind`, if it is Near or Contact.
    pub fn of(kind: &Kind) -> Option<Tangency> {
        match kind {
            Kind::Near(_) => Some(Tangency::Near),
            Kind::Contact(_) => Some(Tangency::Contact),
            Kind::Empty { .. } | Kind::Simple { .. } => None,
        }
    }
}

/// A certified leaf as [`clusters`] reads it.
pub trait ClusterLeaf: Sync {
    /// Its two boxes, the first operand's first (a leaf certified with
    /// the second operand's box first reads them swapped).
    fn boxes(&self) -> [BoxKey; 2];
    /// Its pieces' world boxes (each `[lo, hi]`, rounded outward or
    /// certified), in the order of [`ClusterLeaf::boxes`].
    fn world(&self) -> [[[f64; 3]; 2]; 2];
    /// Its tangency, if it is Near or Contact.
    fn tangency(&self) -> Option<Tangency>;
}

impl ClusterLeaf for flat::Leaf {
    fn boxes(&self) -> [BoxKey; 2] {
        [self.a, self.b]
    }

    fn world(&self) -> [[[f64; 3]; 2]; 2] {
        self.world
    }

    fn tangency(&self) -> Option<Tangency> {
        Tangency::of(&self.kind)
    }
}

/// A reference reads as its leaf, so `&dyn ClusterLeaf` mixes kinds.
impl<T: ClusterLeaf + ?Sized> ClusterLeaf for &T {
    fn boxes(&self) -> [BoxKey; 2] {
        (**self).boxes()
    }

    fn world(&self) -> [[[f64; 3]; 2]; 2] {
        (**self).world()
    }

    fn tangency(&self) -> Option<Tangency> {
        (**self).tangency()
    }
}

/// A leaf against a quadric: its boxes in the operands' order (the
/// quadric face's box is the first operand's or the second's), its world
/// boxes alike.
impl<S: Sync> ClusterLeaf for near_quadric::Leaf<S> {
    fn boxes(&self) -> [BoxKey; 2] {
        if self.a.operand() == Operand::B && self.b.operand() == Operand::A {
            [self.b, self.a]
        } else {
            [self.a, self.b]
        }
    }

    fn world(&self) -> [[[f64; 3]; 2]; 2] {
        let [wa, wb] = self.world;
        if self.a.operand() == Operand::B && self.b.operand() == Operand::A {
            [wb, wa]
        } else {
            [wa, wb]
        }
    }

    fn tangency(&self) -> Option<Tangency> {
        match self.kind {
            near_quadric::Kind::Near(_) => Some(Tangency::Near),
            near_quadric::Kind::Contact(_) => Some(Tangency::Contact),
            near_quadric::Kind::Empty(_) | near_quadric::Kind::Simple { .. } => None,
        }
    }
}

/// A leaf's place in its cluster's order: its key, then its boxes and
/// its input position, which break only exact ties.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LeafAt<K> {
    key: ClusterKey<K>,
    boxes: [BoxKey; 2],
    index: u32,
}

/// A tangency cluster (module docs) with face keys `K`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cluster<K> {
    kind: Tangency,
    key: ClusterKey<K>,
    /// In leaf-key order; never empty.
    leaves: Vec<LeafAt<K>>,
}

impl<K: Ord + Clone> Cluster<K> {
    /// Its leaves' kind.
    pub fn kind(&self) -> Tangency {
        self.kind
    }

    /// Its place in the geometric order.
    pub fn key(&self) -> &ClusterKey<K> {
        &self.key
    }

    /// Its leaves, as positions in the input of [`clusters`], in the
    /// geometric order.
    pub fn leaves(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        self.leaves.iter().map(|l| l.index)
    }

    /// The key of the leaf at position `k` of [`Cluster::leaves`].
    pub fn leaf_key(&self, k: usize) -> Option<&ClusterKey<K>> {
        self.leaves.get(k).map(|l| &l.key)
    }

    /// Whether one of its leaves is the pair of boxes `boxes` (the first
    /// operand's first).
    pub fn holds(&self, boxes: &[BoxKey; 2]) -> bool {
        self.leaves.iter().any(|l| l.boxes == *boxes)
    }

    /// The tie-broken order of clusters: the key, then the first leaf's
    /// boxes and position.
    fn order(&self) -> (&ClusterKey<K>, &[BoxKey; 2], u32) {
        let first = &self.leaves[0];
        (&self.key, &first.boxes, first.index)
    }
}

/// Why clusters can't be formed or merged.
#[derive(Clone, Debug, PartialEq)]
pub enum ClusterError {
    /// More leaves than `u32` positions.
    TooMany,
    /// The leaf at this position doesn't have a box of the first operand
    /// then one of the second.
    Operands(u32),
    /// A world box coordinate of the leaf at this position is a NaN.
    NotANumber(u32),
    /// The windows given aren't one per cluster.
    Windows,
    /// There is no leaf at this position, or it isn't of the cluster's
    /// kind.
    Kind(u32),
    /// The budget ran out ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// A box's sides in its face's global UV, exactly: `[[u_lo, u_hi],
/// [v_lo, v_hi]]` as integers over `2^`[`MAX_BOX_DEPTH`].
pub(super) type UvBox = [[i128; 2]; 2];

/// The global coordinate of the local dyadic `t` in tile `i`, times
/// `2^`[`MAX_BOX_DEPTH`]: exact, `|i| < 2⁶³` and the shift below `2⁵³`.
fn global(i: i64, t: Dyadic) -> i128 {
    (i128::from(i) << MAX_BOX_DEPTH) + (i128::from(t.num()) << (MAX_BOX_DEPTH - t.exp()))
}

pub(super) fn uv_box(b: &BoxKey) -> UvBox {
    let side = |dir: Dir, i: i64| {
        let e: Extent = b.extent(dir);
        [global(i, e.lo()), global(i, e.hi())]
    };
    let [i, j] = b.tile();
    [side(Dir::U, i), side(Dir::V, j)]
}

/// Whether the closed boxes `a` and `b` share a point.
fn abut(a: &UvBox, b: &UvBox) -> bool {
    (0..2).all(|k| a[k][0] <= b[k][1] && b[k][0] <= a[k][1])
}

/// The boxes of the leaves of one kind on one face, for the abutment
/// pass.
struct FaceBoxes {
    /// Each box with its leaf's position, sorted by box, then position.
    boxes: Vec<(UvBox, u32)>,
}

/// Runs a pass of `f` over `items` at rung 0.
fn run<T, R, F>(
    work: &mut Work,
    stage: &'static str,
    items: &[T],
    f: F,
) -> Result<Vec<R>, ClusterError>
where
    T: Sync,
    R: Send,
    F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<ClusterError>> + Sync + Send,
{
    work.pass(Pass { stage, rung: 0 }, items, f)
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => ClusterError::OutOfBudget(e),
        })
}

/// Charges `units` of plain work.
fn units(ledger: &mut Ledger<'_>, units: u64) -> Result<(), Stop<ClusterError>> {
    ledger.charge(Step::Work, units).map_err(Stop::from)
}

/// The tangency clusters of `leaves` (module docs), each face's key read
/// by `face_key` (it must be the face's geometric key: its form key and
/// name, never its id), in the geometric order; charged to
/// `work`. Refused if a leaf's boxes aren't of the first operand then
/// the second, or a world box holds a NaN (the first such leaf by
/// position).
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn clusters<L, K>(
    leaves: &[L],
    face_key: impl Fn(Operand, FaceId) -> K + Sync,
    work: &mut Work,
) -> Result<Vec<Cluster<K>>, ClusterError>
where
    L: ClusterLeaf,
    K: Ord + Clone + Send + Sync,
{
    let Ok(n) = u32::try_from(leaves.len()) else {
        return Err(ClusterError::TooMany);
    };
    // Each leaf checked, and the tangent leaves' keys.
    let all: Vec<u32> = (0..n).collect();
    let checked = run(work, "snap clusters: leaves", &all, |&i, ledger| {
        units(ledger, 1)?;
        leaf_at(i, &leaves[i as usize], &face_key).map_err(Stop::Failed)
    })?;
    let (at, kinds): (Vec<LeafAt<K>>, Vec<Tangency>) = checked.into_iter().flatten().unzip();

    // The boxes per kind and face, then the abutting pairs within each.
    let mut faces: BTreeMap<(Tangency, Operand, FaceId), FaceBoxes> = BTreeMap::new();
    for (k, (l, &t)) in at.iter().zip(&kinds).enumerate() {
        for b in &l.boxes {
            faces
                .entry((t, b.operand(), b.face()))
                .or_insert_with(|| FaceBoxes { boxes: Vec::new() })
                .boxes
                // Fits: fewer tangent leaves than leaves.
                .push((uv_box(b), k as u32));
        }
    }
    let faces: Vec<FaceBoxes> = faces
        .into_values()
        .map(|mut f| {
            f.boxes.sort_unstable();
            f
        })
        .collect();
    let joins: Vec<Vec<[u32; 2]>> = run(work, "snap clusters: abutment", &faces, |f, ledger| {
        abutting(&f.boxes, ledger)
    })?;

    // Components, by union–find over the tangent leaves.
    let mut parent: Vec<u32> = (0..at.len() as u32).collect();
    for [a, b] in joins.into_iter().flatten() {
        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
        if ra != rb {
            // The lesser root stays: the result doesn't depend on the
            // order of the joins.
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            parent[hi as usize] = lo;
        }
    }
    let mut members: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for k in 0..at.len() as u32 {
        let r = root(&mut parent, k);
        members.entry(r).or_default().push(k);
    }
    let groups: Vec<Vec<u32>> = members.into_values().collect();
    let mut out: Vec<Cluster<K>> = run(work, "snap clusters: order", &groups, |g, ledger| {
        units(ledger, g.len() as u64)?;
        let kind = kinds[g[0] as usize];
        let leaves: Vec<LeafAt<K>> = g.iter().map(|&k| at[k as usize].clone()).collect();
        Ok(cluster(kind, leaves))
    })?;
    out.sort_unstable_by(|a, b| a.order().cmp(&b.order()));
    Ok(out)
}

/// The leaf `l` at position `i` checked, and its place in a cluster's
/// order and its tangency if it is Near or Contact.
fn leaf_at<L: ClusterLeaf, K: Ord + Clone>(
    i: u32,
    l: &L,
    face_key: &(impl Fn(Operand, FaceId) -> K + ?Sized),
) -> Result<Option<(LeafAt<K>, Tangency)>, ClusterError> {
    let [a, b] = l.boxes();
    if a.operand() != Operand::A || b.operand() != Operand::B {
        return Err(ClusterError::Operands(i));
    }
    let world = l.world();
    if world.iter().flatten().flatten().any(|x| x.is_nan()) {
        return Err(ClusterError::NotANumber(i));
    }
    let Some(t) = l.tangency() else {
        return Ok(None);
    };
    let (ka, kb) = (
        face_key(Operand::A, a.face()),
        face_key(Operand::B, b.face()),
    );
    let key = ClusterKey::new(world, [ka.clone(), kb.clone()], [[ka, kb]]).map_err(|e| {
        debug_assert_eq!(e, KeyError::NotANumber, "a leaf has its boxes and faces");
        ClusterError::NotANumber(i)
    })?;
    let at = LeafAt {
        key,
        boxes: [a, b],
        index: i,
    };
    Ok(Some((at, t)))
}

/// The cluster of kind `kind` made of the leaves at `positions` of
/// `leaves` (each of that kind; keys read by `face_key` as in
/// [`clusters`]), its leaves in the geometric order; `None` if there are
/// none. Not charged: the caller charges a unit per leaf.
pub(crate) fn of_leaves<L, K>(
    kind: Tangency,
    positions: &[u32],
    leaves: &[L],
    face_key: &(impl Fn(Operand, FaceId) -> K + ?Sized),
) -> Result<Option<Cluster<K>>, ClusterError>
where
    L: ClusterLeaf,
    K: Ord + Clone,
{
    let mut at = Vec::with_capacity(positions.len());
    for &i in positions {
        let l = leaves.get(i as usize).ok_or(ClusterError::Kind(i))?;
        match leaf_at(i, l, face_key)? {
            Some((a, t)) if t == kind => at.push(a),
            _ => return Err(ClusterError::Kind(i)),
        }
    }
    Ok((!at.is_empty()).then(|| cluster(kind, at)))
}

/// The cluster of `leaves` (non-empty) of kind `kind`: its leaves sorted,
/// its key the least of theirs part by part.
fn cluster<K: Ord + Clone>(kind: Tangency, mut leaves: Vec<LeafAt<K>>) -> Cluster<K> {
    leaves.sort_unstable();
    let key = least(leaves.iter().map(|l| &l.key));
    Cluster { kind, key, leaves }
}

/// The least of `keys` (non-empty) part by part: the least corner, the
/// least target, the least pair.
fn least<'a, K: Ord + Clone + 'a>(
    keys: impl Iterator<Item = &'a ClusterKey<K>> + Clone,
) -> ClusterKey<K> {
    let corner = |k: &ClusterKey<K>| {
        let c = k.corner().get();
        [c, c]
    };
    ClusterKey::new(
        keys.clone().map(corner),
        keys.clone().map(|k| k.target().clone()),
        keys.map(|k| k.pair().clone()),
    )
    .expect("a cluster has leaves, and its keys' corners aren't NaN")
}

/// The pairs of leaves whose boxes in `boxes` (one face, sorted) abut:
/// leaves sharing a box joined to its first, then a sweep along `u` over
/// the distinct boxes.
fn abutting(
    boxes: &[(UvBox, u32)],
    ledger: &mut Ledger<'_>,
) -> Result<Vec<[u32; 2]>, Stop<ClusterError>> {
    let mut out = Vec::new();
    let mut distinct: Vec<(UvBox, u32)> = Vec::new();
    for &(b, k) in boxes {
        units(ledger, 1)?;
        match distinct.last() {
            Some(&(d, first)) if d == b => out.push([first, k]),
            _ => distinct.push((b, k)),
        }
    }
    // Sorted by box, so by `u_lo` first: each box meets only the boxes
    // after it that start at or before its `u_hi`.
    for (n, (a, ka)) in distinct.iter().enumerate() {
        for (b, kb) in &distinct[n + 1..] {
            if b[0][0] > a[0][1] {
                break;
            }
            units(ledger, 1)?;
            if abut(a, b) {
                out.push([*ka, *kb]);
            }
        }
    }
    Ok(out)
}

/// The root of `k`, halving the path.
fn root(parent: &mut [u32], mut k: u32) -> u32 {
    while parent[k as usize] != k {
        let p = parent[k as usize];
        parent[k as usize] = parent[p as usize];
        k = parent[k as usize];
    }
    k
}

/// A merged cluster and the positions, ascending, of the clusters it was
/// made of among those given to [`merge_sharing`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merged<K> {
    /// The cluster.
    pub cluster: Cluster<K>,
    /// Its parts' positions.
    pub parts: Vec<u32>,
}

/// `clusters` (in the geometric order) with every two of one kind whose
/// windows share an element merged, transitively (module docs);
/// `windows` holds each cluster's window elements, one list per cluster
/// in the same order, in any order within a list. The result is in the
/// geometric order; charged to `work`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn merge_sharing<K, E>(
    clusters: Vec<Cluster<K>>,
    windows: &[Vec<E>],
    work: &mut Work,
) -> Result<Vec<Merged<K>>, ClusterError>
where
    K: Ord + Clone + Send + Sync,
    E: Ord + Sync,
{
    if windows.len() != clusters.len() {
        return Err(ClusterError::Windows);
    }
    if u32::try_from(clusters.len()).is_err() {
        return Err(ClusterError::TooMany);
    }
    let parents = run(work, "snap clusters: merge", &[()], |_, ledger| {
        // Fits: checked above.
        let mut parent: Vec<u32> = (0..clusters.len() as u32).collect();
        // Per kind and element, the first cluster holding it.
        let mut first: BTreeMap<(Tangency, &E), u32> = BTreeMap::new();
        for (k, (c, w)) in clusters.iter().zip(windows).enumerate() {
            units(ledger, c.leaves.len() as u64)?;
            let k = k as u32;
            for e in w {
                units(ledger, 1)?;
                let held = *first.entry((c.kind, e)).or_insert(k);
                let (ra, rb) = (root(&mut parent, held), root(&mut parent, k));
                if ra != rb {
                    let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
                    parent[hi as usize] = lo;
                }
            }
        }
        for k in 0..clusters.len() as u32 {
            root(&mut parent, k);
        }
        Ok(parent)
    })?;
    let parent = parents.into_iter().next().expect("one item");
    let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (k, &r) in parent.iter().enumerate() {
        groups.entry(r).or_default().push(k as u32);
    }
    let mut slots: Vec<Option<Cluster<K>>> = clusters.into_iter().map(Some).collect();
    let mut out: Vec<Merged<K>> = groups
        .into_values()
        .map(|parts| {
            let mut taken = parts
                .iter()
                .map(|&k| slots[k as usize].take().expect("each once"));
            let first = taken.next().expect("a group has a cluster");
            let cluster = if parts.len() == 1 {
                first
            } else {
                let kind = first.kind;
                let mut leaves = first.leaves;
                for c in taken {
                    leaves.extend(c.leaves);
                }
                cluster(kind, leaves)
            };
            Merged { cluster, parts }
        })
        .collect();
    out.sort_unstable_by(|a, b| a.cluster.order().cmp(&b.cluster.order()));
    Ok(out)
}

#[cfg(test)]
pub(super) mod tests;
