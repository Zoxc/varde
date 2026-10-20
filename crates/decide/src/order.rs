//! The geometric order: the one order on vertices, points on edges,
//! edges, faces and snap clusters that a choice among them reads, so that
//! the choice doesn't depend on the entities' ids. Topology and names
//! must not change when an operand's ids are permuted; ids may order
//! work, and break the exact ties this order leaves, never anything else.
//!
//! **Coordinates.** Positions and parameters are stored or certified
//! values rounded to `f64`, compared exactly by [`f64::total_cmp`]
//! ([`Coord`]): `−0` comes before `+0`, so no id ever chooses between a
//! `−0` and a `+0` coordinate, and two coordinates tie only when their
//! bits are equal. A NaN is refused ([`NotANumber`]). No distance is
//! compared.
//!
//! **Face keys.** A vertex or edge whose geometry ties is ordered next by
//! the face keys of its faces, a face's key being its form key and its
//! name ([`FaceOrd`]), compared form first, a face without a form key
//! after every face with one. The order is generic over both types; a
//! caller without face keys yet passes `()`.
//!
//! **The order.**
//! - A vertex ([`VertKey`]): its position, lexicographic `(x, y, z)`; then
//!   the sorted list of its incident faces' keys.
//! - A point on an edge (a foot, an end, a cut point): its parameter from
//!   the edge's geometrically lesser end ([`Along`]), which is the stored
//!   start or the stored end: an edge is stored from its lower-id vertex,
//!   so the parameter's own direction is an id's choice, read only where
//!   the two ends tie exactly. A parameter's zero has one place: `−0` and
//!   `+0` are one point, and reversing a range (`t0 + t1 − t`) can turn
//!   one into the other, so both read as `+0`.
//! - An edge ([`EdgeKey`]): its lesser end, then its other end (a closed
//!   edge's ends both its one vertex), then its curve's point at the
//!   middle of its parameter range, which tells apart two arcs between
//!   one pair of vertices; then the sorted keys of its two faces.
//!
//! - A face ([`FaceOrderKey`]): its canonical vertex, the least of the
//!   vertices of all its loops in the vertex order above; then its face
//!   key; then its surface's point at the centre of its lattice rectangle,
//!   which tells apart two faces sharing their least vertex and key.
//! - A snap cluster, or a leaf within one ([`ClusterKey`]): the least
//!   corner of the union of its leaf boxes in world coordinates (rounded
//!   outward by whoever bounds them); then the least face key among its
//!   target faces; then its least face pair, a leaf's pair of faces in
//!   operand order (the first operand's face, then the second's), compared
//!   lexicographically. The least corner of a union of axis-aligned boxes
//!   is its lexicographically least point `(x, y, z)`: the least of the
//!   boxes' low corners, a corner of the union (the union's bounding box's
//!   low corner, taken coordinate by coordinate, needn't be in the union).
//!
//! Ids break only the ties of all of these ([`first`], [`sort`]: pairs of
//! a key and an id, the id last): bitwise-equal vertices with equal face
//! keys, coincident edges with the same middle point and face keys, points
//! at bitwise-equal parameters, faces with the same canonical vertex, key
//! and centre point, clusters with the same corner, least target and least
//! pair. There the choice changes no geometry and no name.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use smallvec::SmallVec;

use crate::ids::{EdgeId, VertId};
use crate::records::keys::Operand;

/// A coordinate or parameter that is a NaN, refused by the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotANumber;

/// Why a face's or cluster's key is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// A coordinate is a NaN.
    NotANumber,
    /// A list the key takes its least element from is empty: a face
    /// without a vertex, a cluster without a leaf box, target face or
    /// face pair.
    Empty,
}

impl From<NotANumber> for KeyError {
    fn from(_: NotANumber) -> KeyError {
        KeyError::NotANumber
    }
}

/// An `f64` that isn't a NaN, ordered by [`f64::total_cmp`]: `−0` before
/// `+0`, equal only when the bits are.
#[derive(Clone, Copy, Debug)]
pub struct Coord(f64);

impl Coord {
    /// `x`, unless it is a NaN.
    pub fn new(x: f64) -> Result<Coord, NotANumber> {
        if x.is_nan() {
            Err(NotANumber)
        } else {
            Ok(Coord(x))
        }
    }

    /// The value.
    pub fn get(self) -> f64 {
        self.0
    }
}

impl PartialEq for Coord {
    fn eq(&self, other: &Coord) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Coord {}

impl PartialOrd for Coord {
    fn partial_cmp(&self, other: &Coord) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Coord {
    fn cmp(&self, other: &Coord) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl Hash for Coord {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

/// A position, ordered lexicographically by `(x, y, z)`, each by
/// [`Coord`]'s order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos([Coord; 3]);

impl Pos {
    /// `p`, unless a coordinate is a NaN.
    pub fn new(p: [f64; 3]) -> Result<Pos, NotANumber> {
        Ok(Pos([
            Coord::new(p[0])?,
            Coord::new(p[1])?,
            Coord::new(p[2])?,
        ]))
    }

    /// The coordinates.
    pub fn get(self) -> [f64; 3] {
        self.0.map(Coord::get)
    }
}

/// A face's key in the order: its form key `form` (`None` for a face
/// without one, a ruled or free surface) and its name `name`, compared
/// form first, `None` after every `Some`, then by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FaceOrd<F, N> {
    /// The form key of the face's surface, if it has one.
    pub form: Option<F>,
    /// The face's name.
    pub name: N,
}

impl<F: Ord, N: Ord> PartialOrd for FaceOrd<F, N> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<F: Ord, N: Ord> Ord for FaceOrd<F, N> {
    fn cmp(&self, other: &Self) -> Ordering {
        let form = match (&self.form, &other.form) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        form.then_with(|| self.name.cmp(&other.name))
    }
}

/// A vertex's place in the order: its position, then the sorted keys
/// `K` of its incident faces, one per face (two faces may share a key).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VertKey<K> {
    pos: Pos,
    faces: SmallVec<[K; 4]>,
}

impl<K: Ord> VertKey<K> {
    /// The key of a vertex at `pos` whose incident faces have keys
    /// `faces`, in any order; refused if a coordinate is a NaN.
    pub fn new(pos: [f64; 3], faces: impl IntoIterator<Item = K>) -> Result<Self, NotANumber> {
        let mut faces: SmallVec<[K; 4]> = faces.into_iter().collect();
        faces.sort_unstable();
        Ok(VertKey {
            pos: Pos::new(pos)?,
            faces,
        })
    }

    /// The position.
    pub fn pos(&self) -> Pos {
        self.pos
    }

    /// The incident faces' keys, sorted.
    pub fn faces(&self) -> &[K] {
        &self.faces
    }
}

/// Which way an edge's stored parameter runs from its geometrically
/// lesser end: the order of points on the edge (feet, ends, cut points)
/// is their parameter from that end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Along {
    /// The lesser end is the stored start: the order is increasing
    /// parameter. Also where the two ends tie exactly (a closed edge, or
    /// two bitwise-equal vertices with equal face keys), the only place
    /// the stored direction, an id's choice, is read.
    Stored,
    /// The lesser end is the stored end: the order is decreasing
    /// parameter.
    Reversed,
}

impl Along {
    /// The direction of an edge stored from the vertex with key `start`
    /// (at the start of its parameter range) to the one with key `end`.
    pub fn of<K: Ord>(start: &VertKey<K>, end: &VertKey<K>) -> Along {
        if end < start {
            Along::Reversed
        } else {
            Along::Stored
        }
    }

    /// The place in the order of the point at stored parameter `t`:
    /// `t`, or `−t` on a reversed edge (negation is exact and reverses
    /// the order), so it is the parameter from the lesser end up to a
    /// constant. A `−0` reads as `+0`: the two are one point, and an edge
    /// stored the other way round may hold either (`0 − 0` is `+0`, the
    /// negation of `+0` is `−0`), so telling them apart would make the
    /// order depend on the stored direction. Comparable only with other
    /// points of the same edge. Refused if `t` is a NaN.
    pub fn param(self, t: f64) -> Result<Coord, NotANumber> {
        let t = match self {
            Along::Stored => t,
            Along::Reversed => -t,
        };
        // `−0 + 0` is `+0`; every other value, NaN included, is kept.
        Coord::new(t + 0.0)
    }

    /// The order of the points at stored parameters `s` and `t` on the
    /// edge, from its lesser end. Refused if either is a NaN.
    pub fn cmp_params(self, s: f64, t: f64) -> Result<Ordering, NotANumber> {
        Ok(self.param(s)?.cmp(&self.param(t)?))
    }
}

/// An edge's place in the order: its lesser end, its other end, its
/// curve's point at the middle of its parameter range, then the sorted
/// keys of its two faces.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeKey<K> {
    lo: VertKey<K>,
    hi: VertKey<K>,
    mid: Pos,
    faces: [K; 2],
}

impl<K: Ord> EdgeKey<K> {
    /// The key of an edge with end vertices `ends`, in either order (a
    /// closed edge: its one vertex twice), its curve's point `mid` at the
    /// middle of its parameter range and its faces' keys `faces` (a seam:
    /// its one face's twice), in either order. Refused if a coordinate of
    /// `mid` is a NaN.
    pub fn new(ends: [VertKey<K>; 2], mid: [f64; 3], faces: [K; 2]) -> Result<Self, NotANumber> {
        let [a, b] = ends;
        let (lo, hi) = if b < a { (b, a) } else { (a, b) };
        let [f, g] = faces;
        let faces = if g < f { [g, f] } else { [f, g] };
        Ok(EdgeKey {
            lo,
            hi,
            mid: Pos::new(mid)?,
            faces,
        })
    }

    /// The lesser end.
    pub fn lo(&self) -> &VertKey<K> {
        &self.lo
    }

    /// The other end.
    pub fn hi(&self) -> &VertKey<K> {
        &self.hi
    }

    /// The curve's point at the middle of the parameter range.
    pub fn mid(&self) -> Pos {
        self.mid
    }

    /// The faces' keys, sorted.
    pub fn faces(&self) -> &[K; 2] {
        &self.faces
    }
}

/// A face's place in the order: its canonical vertex (the least vertex
/// of all its loops), its face key, then its surface's point at the
/// centre of its lattice rectangle.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceOrderKey<K> {
    canonical: VertKey<K>,
    face: K,
    centre: Pos,
}

impl<K: Ord> FaceOrderKey<K> {
    /// The key of a face whose loops have the vertices `verts`, in any
    /// order and with repeats (every loop's, a vertex listed once per
    /// visit or once in all), whose own face key is `face` and whose
    /// surface is at `centre` at the centre of its lattice rectangle.
    /// Refused if `verts` is empty or a coordinate of `centre` is a NaN.
    pub fn new(
        verts: impl IntoIterator<Item = VertKey<K>>,
        face: K,
        centre: [f64; 3],
    ) -> Result<Self, KeyError> {
        let centre = Pos::new(centre)?;
        let canonical = verts.into_iter().min().ok_or(KeyError::Empty)?;
        Ok(FaceOrderKey {
            canonical,
            face,
            centre,
        })
    }

    /// The canonical vertex: the least vertex of all the face's loops.
    pub fn canonical(&self) -> &VertKey<K> {
        &self.canonical
    }

    /// The face's own face key.
    pub fn face(&self) -> &K {
        &self.face
    }

    /// The surface's point at the centre of the lattice rectangle.
    pub fn centre(&self) -> Pos {
        self.centre
    }
}

/// A snap cluster's place in the order, or a leaf's within its cluster:
/// the least corner of its leaf boxes' union, the least face key `K`
/// among its target faces, then its least face pair (the first operand's
/// face key, then the second's).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClusterKey<K> {
    corner: Pos,
    target: K,
    pair: [K; 2],
}

impl<K: Ord> ClusterKey<K> {
    /// The key of a cluster (or leaf) with world-space leaf boxes
    /// `boxes`, each given by two opposite corners in either order and
    /// rounded outward, target faces with keys `targets` and leaves'
    /// face pairs `pairs`, each in operand order; all three in any order.
    /// Refused if any of them is empty or a box coordinate is a NaN.
    pub fn new(
        boxes: impl IntoIterator<Item = [[f64; 3]; 2]>,
        targets: impl IntoIterator<Item = K>,
        pairs: impl IntoIterator<Item = [K; 2]>,
    ) -> Result<Self, KeyError> {
        let mut corner: Option<Pos> = None;
        for [a, b] in boxes {
            let (a, b) = (Pos::new(a)?, Pos::new(b)?);
            // The box's low corner, coordinate by coordinate.
            let low = Pos([0, 1, 2].map(|k| a.0[k].min(b.0[k])));
            corner = Some(match corner {
                Some(c) if c <= low => c,
                _ => low,
            });
        }
        Ok(ClusterKey {
            corner: corner.ok_or(KeyError::Empty)?,
            target: targets.into_iter().min().ok_or(KeyError::Empty)?,
            pair: pairs.into_iter().min().ok_or(KeyError::Empty)?,
        })
    }

    /// The least corner of the leaf boxes' union: its lexicographically
    /// least point.
    pub fn corner(&self) -> Pos {
        self.corner
    }

    /// The least target face key.
    pub fn target(&self) -> &K {
        &self.target
    }

    /// The least face pair.
    pub fn pair(&self) -> &[K; 2] {
        &self.pair
    }
}

/// The geometric order of an operand's vertices and edges: what a choice
/// among them reads.
pub trait EntityOrder {
    /// The face keys compared ([`FaceOrd`]'s, or `()`).
    type Key: Ord + Clone;

    /// Edge `g` of `operand`'s ends, its stored start first.
    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2];

    /// Vertex `v` of `operand`'s place in the order.
    fn vert_key(&self, operand: Operand, v: VertId) -> VertKey<Self::Key>;

    /// Edge `g` of `operand`'s place in the order.
    fn edge_key(&self, operand: Operand, g: EdgeId) -> EdgeKey<Self::Key>;
}

/// The id of the first entity in geometric order among `items`, pairs of
/// a geometric key and an id, the id deciding only among exact ties of
/// the key; `None` if there are none.
pub fn first<G: Ord, I: Ord>(items: impl IntoIterator<Item = (G, I)>) -> Option<I> {
    items.into_iter().min().map(|(_, id)| id)
}

/// Sorts `items`, pairs of a geometric key and an id, into geometric
/// order, the ids breaking exact ties of the keys.
pub fn sort<G: Ord, I: Ord>(items: &mut [(G, I)]) {
    items.sort_unstable();
}

#[cfg(test)]
mod tests;
