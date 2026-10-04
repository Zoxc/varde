//! Which triangles each pass of the clean-up may change, kept in step as
//! they change: the work local to the change.
//!
//! Each pass of the clean-up does something only to a triangle (or at a
//! vertex) of some class that its own state, and its neighbours', tells:
//! an edge is collapsed only from a triangle with a side no longer than
//! `thin`, a side flipped only out of a triangle no higher than that, a
//! seam made straight only where a triangle has one, a sliver flipped
//! only on a plane face in or beside a sliver, a folded star only at a
//! vertex whose triangles are all on plane faces and face apart. The
//! classes are worked out once for every triangle and vertex (a cheap
//! pass over the corners, and the curves' records for the classes that
//! need a curved side), then again only for the triangles round a
//! triangle that changed. Each pass then visits its class, in id order,
//! with the class as it is at each step, and asks the same question of
//! each member as the pass over every triangle did: a triangle outside
//! its class gives no to it, with nothing changed. So the passes change
//! the same triangles in the same order, and the result is the same,
//! bit for bit, for work that follows the triangles of those classes and
//! what changes, rather than the whole soup every round.
//!
//! A class depends on a triangle's corners, face and the curves of its
//! sides, and for [`SEAM`] and [`SLIVERS`] on those of the triangles
//! sharing a side with it. Every change goes through a triangle whose
//! corners, face or sides' curves changed (or that was taken out or
//! made), which is noted ([`Cleaner::touch`]); before a class is read
//! again ([`Cleaner::settle`]) every living triangle with a corner among
//! those triangles' corners is classed again, which covers its
//! neighbours, and so are those vertices. A record of an edge no triangle
//! has (one left where an edge went) changes no class.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use super::{Cleaner, SLIVER};

/// A side no longer than `thin` (or `small`, if more): the short edges,
/// the plane edges and those a tangency left are among its sides.
pub(super) const SHORT: u8 = 1 << 0;
/// No higher over its longest side than `thin` (or `small`): the flips.
pub(super) const FLAT: u8 = 1 << 1;
/// On a plane face in its plane with a seam: a curved side whose
/// neighbour across lies in that plane too
/// ([`Cleaner::own_plane`], [`Cleaner::seam`]).
pub(super) const SEAM: u8 = 1 << 2;
/// On a plane face, a sliver (see [`Cleaner::sliver`]) or sharing a side
/// with one on the same face: the Delaunay flips.
pub(super) const SLIVERS: u8 = 1 << 3;
/// On a plane face with a side that has a record in the curves: where a
/// curved corner may be closed.
pub(super) const BENT: u8 = 1 << 4;

/// A side that has a record in the curves, on any face: the triangles
/// whose patch isn't their corners' triangle. Kept as a flag only, with
/// no list of its members.
pub(super) const RECORDED: u8 = 1 << 5;

/// The classes with a list of members, each as its bit's index.
const CLASSES: usize = 5;

/// The classes of the clean-up's triangles and vertices: see the
/// [module](self) docs.
#[derive(Debug, Default)]
pub(super) struct Index {
    /// Each triangle's classes.
    bits: Vec<u8>,
    /// The members of each class, by bit index.
    sets: [BTreeSet<u32>; CLASSES],
    /// Whether each vertex's triangles are all on plane faces and two of
    /// them face apart: where a star may be folded ([`Cleaner::folded`]).
    folds: Vec<bool>,
    /// The vertices [`Self::folds`] marks.
    folded: BTreeSet<u32>,
    /// The triangles the boolean made ([`Soup::made`](super::Soup::made)),
    /// living or not.
    made: BTreeSet<u32>,
    /// Triangles changed since the classes were last brought up to date.
    touched: Vec<u32>,
    /// Vertices a changed triangle had as corners before, since then.
    left: Vec<u32>,
    /// Triangles and vertices classed or visited since last charged.
    pub(super) visits: usize,
}

/// The bit index of class `bit`.
fn slot(bit: u8) -> usize {
    bit.trailing_zeros() as usize
}

impl Cleaner<'_> {
    /// Whether `tri` is a sliver: the sine of its narrowest angle no more
    /// than [`SLIVER`] (as [`Cleaner::delaunay`] tells).
    pub(super) fn sliver(&self, tri: [u32; 3]) -> bool {
        self.narrowest(tri).partial_cmp(&SLIVER) != Some(Ordering::Greater)
    }

    /// The classes of triangle `t` that its own corners and face tell:
    /// [`SHORT`], [`FLAT`], and whether it is a sliver on a plane face.
    fn own_classes(&self, t: u32) -> (u8, bool) {
        let tri = self.soup.tris[t as usize];
        let mut bits = 0;
        let most = self.thin.max(self.small);
        let within = |x: f64| x.partial_cmp(&most) != Some(Ordering::Greater);
        if (0..3).any(|i| within(self.p(tri[i]).distance(self.p(tri[(i + 1) % 3])))) {
            bits |= SHORT;
        }
        if within(self.height(tri).0) {
            bits |= FLAT;
        }
        let plane = self.planes[self.soup.faces[t as usize] as usize].is_some();
        (bits, plane && self.sliver(tri))
    }

    /// Whether triangle `t` is in [`SEAM`]: what [`Cleaner::straighten`]
    /// and [`Cleaner::unbend`] look for.
    fn seamed(&self, t: u32) -> bool {
        let Some(plane) = self.own_plane(t) else {
            return false;
        };
        let tri = self.soup.tris[t as usize];
        (0..3).any(|i| self.seam(t, tri[i], tri[(i + 1) % 3], plane).is_some())
    }

    /// Whether some side of triangle `t` has a record in the curves.
    fn recorded(&self, t: u32) -> bool {
        let tri = self.soup.tris[t as usize];
        (0..3).any(|i| {
            let (u, v) = (tri[i], tri[(i + 1) % 3]);
            self.soup.curves.contains_key(&(u.min(v), u.max(v)))
        })
    }

    /// Triangle `t`'s classes: see the [module](self) docs.
    pub(super) fn classes(&self, t: u32) -> u8 {
        if !self.alive[t as usize] {
            return 0;
        }
        let (mut bits, sliver) = self.own_classes(t);
        let recorded = self.recorded(t);
        if recorded {
            bits |= RECORDED;
        }
        let face = self.soup.faces[t as usize];
        if self.planes[face as usize].is_none() {
            return bits;
        }
        let tri = self.soup.tris[t as usize];
        let beside = || {
            (0..3).any(|i| {
                self.shared(tri[i], tri[(i + 1) % 3]).into_iter().any(|s| {
                    s != t
                        && self.soup.faces[s as usize] == face
                        && self.sliver(self.soup.tris[s as usize])
                })
            })
        };
        if sliver || beside() {
            bits |= SLIVERS;
        }
        if recorded {
            bits |= BENT;
            if self.seamed(t) {
                bits |= SEAM;
            }
        }
        bits
    }

    /// Whether vertex `v` may be the vertex of a folded star: it has
    /// triangles, all on plane faces, and two of them face apart (as
    /// [`Cleaner::folded`] first asks).
    pub(super) fn may_fold(&self, v: u32) -> bool {
        let around = &self.around[v as usize];
        let Some(&first) = around.first() else {
            return false;
        };
        let normal = |t: u32| self.normal(self.soup.tris[t as usize]);
        // All within about 41° of the first: no two are more than 83°
        // apart, and none face apart (by far more than rounding).
        let first = normal(first);
        let reach = first.length();
        let mut cone = true;
        for &t in around {
            if self.planes[self.soup.faces[t as usize] as usize].is_none() {
                return false;
            }
            let n = normal(t);
            cone &= n.dot(first) > 0.75 * n.length() * reach;
        }
        if cone {
            return false;
        }
        let normals: Vec<_> = around.iter().map(|&t| normal(t)).collect();
        normals
            .iter()
            .enumerate()
            .any(|(i, a)| normals[i + 1..].iter().any(|b| a.dot(*b) < 0.0))
    }

    /// Sets triangle `t`'s classes to `bits`.
    fn set_classes(&mut self, t: u32, bits: u8) {
        let index = &mut self.index;
        let old = index.bits[t as usize];
        if old == bits {
            return;
        }
        for (k, set) in index.sets.iter_mut().enumerate() {
            let bit = 1 << k;
            match (old & bit != 0, bits & bit != 0) {
                (false, true) => {
                    set.insert(t);
                }
                (true, false) => {
                    set.remove(&t);
                }
                _ => {}
            }
        }
        index.bits[t as usize] = bits;
    }

    /// Sets whether vertex `v` [`may_fold`](Self::may_fold).
    fn set_fold(&mut self, v: u32, fold: bool) {
        let index = &mut self.index;
        if index.folds[v as usize] != fold {
            index.folds[v as usize] = fold;
            if fold {
                index.folded.insert(v);
            } else {
                index.folded.remove(&v);
            }
        }
    }

    /// Works out every triangle's and vertex's classes: a pass over the
    /// corners, and over the curves' records for [`BENT`] and [`SEAM`]
    /// (only a triangle with a side some record names can be in either,
    /// and a seam needs the triangle across within `small` of the plane).
    pub(super) fn index_all(&mut self) {
        let n = self.soup.tris.len();
        let mut bits = vec![0u8; n];
        let mut slivers = Vec::new();
        for t in 0..n as u32 {
            if !self.alive[t as usize] {
                continue;
            }
            let (own, sliver) = self.own_classes(t);
            bits[t as usize] = own;
            if sliver {
                slivers.push(t);
            }
        }
        // A sliver on a plane face, and those sharing a side with it on
        // the same face.
        for &t in &slivers {
            bits[t as usize] |= SLIVERS;
            let tri = self.soup.tris[t as usize];
            let face = self.soup.faces[t as usize];
            for i in 0..3 {
                for s in self.shared(tri[i], tri[(i + 1) % 3]) {
                    if s != t && self.soup.faces[s as usize] == face {
                        bits[s as usize] |= SLIVERS;
                    }
                }
            }
        }
        // Plane faces' triangles with a side a record names; of those, the
        // seams.
        let keys: Vec<(u32, u32)> = self.soup.curves.keys().copied().collect();
        let mut bent = Vec::new();
        for (u, v) in keys {
            let on = self.shared(u, v);
            for &t in &on {
                bits[t as usize] |= RECORDED;
                let face = self.soup.faces[t as usize];
                let Some((m, d)) = self.planes[face as usize] else {
                    continue;
                };
                bits[t as usize] |= BENT;
                // The triangle across a seam lies in the plane: its far
                // corner too.
                let near = on.iter().any(|&s| {
                    s != t
                        && self.soup.tris[s as usize].iter().any(|&w| {
                            w != u && w != v && (self.p(w).dot(m) - d).abs() <= self.small
                        })
                });
                if near {
                    bent.push(t);
                }
            }
        }
        bent.sort_unstable();
        bent.dedup();
        for t in bent {
            if self.seamed(t) {
                bits[t as usize] |= SEAM;
            }
        }
        let mut index = Index {
            bits,
            folds: vec![false; self.soup.pos.len()],
            made: (0..n as u32)
                .filter(|&t| self.soup.made[t as usize])
                .collect(),
            ..Index::default()
        };
        for (t, &b) in index.bits.iter().enumerate() {
            for (k, set) in index.sets.iter_mut().enumerate() {
                if b & (1 << k) != 0 {
                    set.insert(t as u32);
                }
            }
        }
        self.index = index;
        for v in 0..self.soup.pos.len() as u32 {
            if self.may_fold(v) {
                self.set_fold(v, true);
            }
        }
    }

    /// Notes that triangle `t` changed: its corners, face, made flag or
    /// the curves of its sides, or it was taken out or made.
    pub(super) fn touch(&mut self, t: u32) {
        self.index.touched.push(t);
    }

    /// [`Self::touch`] for triangle `t` about to take other corners: its
    /// corners now are classed again with its new ones.
    pub(super) fn touch_before(&mut self, t: u32) {
        let old = self.soup.tris[t as usize];
        self.index.left.extend(old);
        self.index.touched.push(t);
    }

    /// Brings the classes up to date with the triangles touched since:
    /// those triangles, every living one with a corner among their
    /// corners (now, or before [`Self::touch_before`]), and those vertices
    /// are classed again.
    pub(super) fn settle(&mut self) {
        if self.index.touched.is_empty() {
            return;
        }
        let left = std::mem::take(&mut self.index.left);
        let mut touched = std::mem::take(&mut self.index.touched);
        touched.sort_unstable();
        touched.dedup();
        // New triangles (and vertices, from the refinement) since.
        let n = self.soup.tris.len();
        self.index.bits.resize(n, 0);
        let verts = self.soup.pos.len();
        self.index.folds.resize(verts, false);
        let mut corners: Vec<u32> = touched
            .iter()
            .flat_map(|&t| self.soup.tris[t as usize])
            .chain(left)
            .filter(|&v| (v as usize) < verts)
            .collect();
        corners.sort_unstable();
        corners.dedup();
        let mut again: Vec<u32> = touched.clone();
        for &v in &corners {
            again.extend_from_slice(&self.around[v as usize]);
        }
        again.sort_unstable();
        again.dedup();
        for &t in &touched {
            if self.soup.made[t as usize] {
                self.index.made.insert(t);
            }
        }
        self.index.visits = self
            .index
            .visits
            .saturating_add(again.len())
            .saturating_add(corners.len());
        for t in again {
            let bits = self.classes(t);
            self.set_classes(t, bits);
        }
        for v in corners {
            let fold = self.may_fold(v);
            self.set_fold(v, fold);
        }
    }

    /// The first member of `class` from `from` on, below `end`, the classes
    /// brought up to date first; counted as a visit.
    pub(super) fn next_in(&mut self, class: u8, from: u32, end: u32) -> Option<u32> {
        self.settle();
        let t = *self.index.sets[slot(class)].range(from..end).next()?;
        self.index.visits = self.index.visits.saturating_add(1);
        Some(t)
    }

    /// The members of `class`, up to date, in id order; counted as
    /// visits.
    pub(super) fn members(&mut self, class: u8) -> Vec<u32> {
        self.settle();
        let out: Vec<u32> = self.index.sets[slot(class)].iter().copied().collect();
        self.index.visits = self.index.visits.saturating_add(out.len());
        out
    }

    /// The first vertex from `from` on, below `end`, that
    /// [`may_fold`](Self::may_fold), the classes brought up to date
    /// first; counted as a visit.
    pub(super) fn next_fold(&mut self, from: u32, end: u32) -> Option<u32> {
        self.settle();
        let v = *self.index.folded.range(from..end).next()?;
        self.index.visits = self.index.visits.saturating_add(1);
        Some(v)
    }

    /// The triangles made by the boolean that are alive, in id order, the
    /// classes brought up to date first; counted as visits.
    pub(super) fn made_alive(&mut self) -> Vec<u32> {
        self.settle();
        let out: Vec<u32> = self
            .index
            .made
            .iter()
            .copied()
            .filter(|&t| self.alive[t as usize])
            .collect();
        self.index.visits = self.index.visits.saturating_add(out.len());
        out
    }

    /// Whether triangle `t` has a side with a record in the curves, the
    /// classes brought up to date.
    pub(super) fn has_record(&self, t: u32) -> bool {
        debug_assert!(self.index.touched.is_empty());
        self.index.bits[t as usize] & RECORDED != 0
    }

    /// The visits counted since last taken.
    pub(super) fn visits(&mut self) -> usize {
        std::mem::take(&mut self.index.visits)
    }

    /// Debug builds: asserts the classes are what working them out
    /// afresh for every triangle and vertex gives.
    pub(super) fn assert_indexed(&mut self) {
        if !cfg!(debug_assertions) {
            return;
        }
        self.settle();
        for t in 0..self.soup.tris.len() as u32 {
            assert_eq!(
                self.index.bits[t as usize],
                self.classes(t),
                "the clean-up's classes of triangle {t}"
            );
            assert_eq!(
                self.index.made.contains(&t),
                self.soup.made[t as usize],
                "whether triangle {t} is made"
            );
        }
        for v in 0..self.soup.pos.len() as u32 {
            assert_eq!(
                self.index.folds[v as usize],
                self.may_fold(v),
                "whether vertex {v} may fold"
            );
        }
    }
}
