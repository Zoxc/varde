//! Removing the degenerate triangles a boolean leaves where the operands
//! are flush.
//!
//! The counting decides the topology for slightly perturbed operands, so
//! where they touch exactly (flush faces, a vertex on a face) the result
//! has pieces of zero size: edges whose ends coincide, triangles whose
//! corners lie on a line. This is the flat version of Manifold's
//! clean-up. Short edges are collapsed, keeping the lower vertex id (the
//! operands' own vertices come first) when the collapse keeps a manifold
//! (every vertex round it keeps one fan, once two triangles it makes the
//! same but facing each other, a sheet of zero thickness, are taken out
//! together) and turns no proper triangle over; a triangle of zero height
//! whose edges aren't short has its longest edge flipped, which splits
//! the triangle beyond at the far corner and leaves the same surface.
//! Where nothing else changes, a vertex at which a folded sheet's two
//! sides are triangulated differently is moved within its star's planes
//! ([`fold`]). Components that enclose no volume go. It never decides
//! that two separate vertices are one: a collapse removes an edge,
//! keeping the surface a closed manifold. Then slivers left on plane
//! faces (each input triangle is cut on its own, and long thin ones leave
//! slivers) are flipped towards the Delaunay triangulation of their face,
//! and last the triangles the boolean made on plane faces are refined for
//! their shapes ([`quality`]).
//!
//! Curved edges (those whose record in [`Soup::curves`] bends by more
//! than the short length) are never collapsed or flipped: the clean-up
//! is about the flat triangles flush planar faces leave. Only a triangle
//! of straight sides is flipped, into one across of straight sides or on
//! a plane face (whose new inner side is straight in the plane), and only
//! if the new triangles' corners along curves stay open. A collapse
//! merges the two sides of each triangle it takes out, keeping a cut's
//! curve where they differ, and moves the curves of the edges it moves
//! onto the vertex kept. The exception is a curve between two triangles
//! in one plane, where flush caps meet along a rim: [`seams`] makes it
//! its chord, flips it away or triangulates its region again, and merges
//! the plane faces it joined.

use std::collections::BTreeMap;

use glam::DVec3;

use super::assemble::Curves;
use super::parts;
use crate::budget::Work;
use crate::mesh::{Edge, Face, Surface, off_surface, straight};
use crate::patch::Patch;
use crate::solid::patch_volume;
use crate::trig;
use crate::{KernelError, Tolerance};

mod fold;
mod quality;
mod seams;

#[cfg(test)]
pub(super) use seams::DISSOLVED;

/// How many rounds of collapses and flips at most.
const ROUNDS: usize = 64;

/// Triangles whose narrowest angle's sine is below this are slivers.
const SLIVER: f64 = 0.02;

/// A triangle soup being cleaned: positions, triangles on vertex ids,
/// each triangle's face, and the curves of its edges that aren't
/// straight (by the vertices they join, the lower first; records of edges
/// no triangle has any more are left, and each step making an edge drops
/// one left where it runs).
#[derive(Debug, Clone)]
pub(super) struct Soup {
    pub(super) pos: Vec<DVec3>,
    pub(super) tris: Vec<[u32; 3]>,
    pub(super) faces: Vec<u32>,
    pub(super) curves: Curves,
    /// Each face's source: itself, or for a copy claiming no surface the
    /// face it copies (the two are one face of the result, not a cut).
    pub(super) sources: Vec<u32>,
    /// Faces merged into others, as pairs of sources (merged, merged
    /// into), in the order they merged: see [`Soup::absorb`].
    pub(super) absorbed: Vec<(u32, u32)>,
    /// Whether each triangle is one this boolean made (cut, or changed
    /// since), not an operand's kept as it was: only those are refined
    /// for their shapes ([`quality`]).
    pub(super) made: Vec<bool>,
}

impl Soup {
    /// Records that triangles of face `from` went onto face `into`, both
    /// on one surface, as the two faces are one there: the key of
    /// `from`'s name, and its aliases, become aliases of `into`'s source,
    /// so references to it still find it (see
    /// [`Mesh::aliases`](crate::mesh::Mesh::aliases)).
    pub(super) fn absorb(&mut self, from: u32, into: u32) {
        let (from, into) = (self.sources[from as usize], self.sources[into as usize]);
        if from != into {
            self.absorbed.push((from, into));
        }
    }
}

/// What a collapse may turn over.
#[derive(Clone, Copy)]
enum Turn<'a> {
    /// No proper triangle: a short edge.
    Proper,
    /// No triangle at all, and their corners along curves stay open: a
    /// vertex inside a plane face.
    Strict,
    /// Proper triangles only in a plane of the folded star where they
    /// face both ways, each then on a face facing its way: see
    /// [`fold`].
    Fold(&'a fold::Star),
}

struct Cleaner<'a> {
    soup: &'a mut Soup,
    alive: Vec<bool>,
    /// Each vertex's triangles.
    around: Vec<Vec<u32>>,
    /// Whether each face is a plane: a triangle of one with curved sides
    /// may still be flipped into, its new inner side straight in the
    /// plane.
    planar: Vec<bool>,
    /// The triangles a collapse gave another curve, which may leave their
    /// face's surface.
    recurved: Vec<u32>,
    /// Each face's plane, if it is one: its unit normal and offset.
    planes: Vec<Option<(DVec3, f64)>>,
    /// Pairs of plane faces (the lower first) a seam joined, to be merged:
    /// see [`seams`].
    joined: Vec<(u32, u32)>,
    /// Edges no longer than this are short, triangles no higher are flat.
    small: f64,
    /// Triangles no higher than this are thin: flipped when the triangle
    /// across their longest side is on the same face.
    thin: f64,
}

/// Cleans `soup`: see the module docs. `small` is the size below which
/// edges and heights count as zero, and `thin` the height below which a
/// triangle is flipped into its neighbour on the same face (which keeps
/// every triangle on its face). Fails as too complex where a collapse
/// leaves a triangle further than the fit tolerance off its face.
pub(super) fn clean(
    soup: &mut Soup,
    faces: &mut Vec<Face>,
    small: f64,
    thin: f64,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<(), KernelError> {
    let planar = faces
        .iter()
        .map(|f| matches!(f.surface, Surface::Plane { .. }))
        .collect();
    let planes = faces
        .iter()
        .map(|f| match f.surface {
            Surface::Plane { n, d } => {
                let len = n.length();
                (len > 0.0 && len.is_finite()).then(|| (n / len, d / len))
            }
            _ => None,
        })
        .collect();
    let mut around = vec![Vec::new(); soup.pos.len()];
    for (t, tri) in soup.tris.iter().enumerate() {
        for &v in tri {
            around[v as usize].push(t as u32);
        }
    }
    let mut c = Cleaner {
        alive: vec![true; soup.tris.len()],
        soup,
        around,
        planar,
        recurved: Vec::new(),
        planes,
        joined: Vec::new(),
        small,
        thin,
    };
    for _ in 0..ROUNDS {
        work.spend(c.soup.tris.len())?;
        let mut changed = false;
        for [u, v] in c.short_edges() {
            // An earlier collapse may have taken the edge already: then no
            // triangle has both ends.
            if c.collapse(u, v, Turn::Proper) {
                changed = true;
            }
        }
        for [u, v] in c.plane_edges() {
            if c.inside_plane(v) && c.collapse(u, v, Turn::Strict) {
                changed = true;
            }
        }
        for t in 0..c.soup.tris.len() as u32 {
            if c.alive[t as usize] && c.thin(t) && c.flip(t) {
                changed = true;
            }
        }
        // Curves between two triangles in one plane: straightened, else
        // flipped away.
        for t in 0..c.soup.tris.len() as u32 {
            if c.alive[t as usize] && (c.straighten(t) | c.unbend(t)) {
                changed = true;
            }
        }
        // Last resort, where nothing else changed: folded sheets whose
        // two sides are triangulated differently.
        if !changed {
            work.spend(c.soup.tris.len())?;
            changed = c.unfold();
        }
        if !changed {
            break;
        }
    }
    // What is left of them, triangulated again by regions, and the plane
    // faces they joined made one.
    c.dissolve(work)?;
    c.merge_joined(faces);
    // Then slivers on plane faces, flipped towards Delaunay.
    for _ in 0..ROUNDS {
        work.spend(c.soup.tris.len())?;
        let mut changed = false;
        for t in 0..c.soup.tris.len() as u32 {
            if c.alive[t as usize] && c.delaunay(t) {
                changed = true;
            }
        }
        for [u, v] in c.plane_edges() {
            if c.inside_plane(v) && c.collapse(u, v, Turn::Strict) {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Then the triangles made on plane faces refined for their shapes.
    c.quality(tol.resolution(), work)?;
    c.drop_empty_components();
    c.leave_surfaces(faces, tol)?;
    let Cleaner { alive, soup, .. } = c;
    let mut keep = alive.iter();
    soup.faces.retain(|_| *keep.next().expect("a flag"));
    let mut keep = alive.iter();
    soup.tris.retain(|_| *keep.next().expect("a flag"));
    let mut keep = alive.iter();
    soup.made.retain(|_| *keep.next().expect("a flag"));
    Ok(())
}

impl Cleaner<'_> {
    fn p(&self, v: u32) -> DVec3 {
        self.soup.pos[v as usize]
    }

    fn normal(&self, [a, b, c]: [u32; 3]) -> DVec3 {
        (self.p(b) - self.p(a)).cross(self.p(c) - self.p(a))
    }

    /// Its height over its longest side, and that side's index.
    fn height(&self, tri: [u32; 3]) -> (f64, usize) {
        let lengths = [0, 1, 2].map(|i| self.p(tri[(i + 1) % 3]).distance(self.p(tri[i])));
        let i = (0..3)
            .max_by(|&i, &j| lengths[i].total_cmp(&lengths[j]).then(j.cmp(&i)))
            .expect("three sides");
        let h = if lengths[i] > 0.0 {
            self.normal(tri).length() / lengths[i]
        } else {
            0.0
        };
        (h, i)
    }

    fn thin(&self, t: u32) -> bool {
        let tri = self.soup.tris[t as usize];
        self.straight_sides(tri) && self.height(tri).0 <= self.thin.max(self.small)
    }

    /// Whether the edge between `u` and `v` is curved: its record's
    /// control point more than `small` off the line through its ends (or
    /// off the point both are at).
    fn curved(&self, u: u32, v: u32) -> bool {
        let Some(edge) = self.soup.curves.get(&(u.min(v), u.max(v))) else {
            return false;
        };
        let (p, q) = (self.p(u), self.p(v));
        if p.distance(q) <= self.small {
            edge.ctrl.distance(p) > self.small
        } else {
            !straight(p, edge.ctrl, q, self.small)
        }
    }

    /// Whether the corners of `tri` along its curved sides are open: the
    /// curve's tangent there (towards its control point) strictly inside
    /// the angle, as seen along the triangle's normal.
    fn open(&self, tri: [u32; 3]) -> bool {
        let normal = self.normal(tri);
        (0..3).all(|i| {
            let (v, next, prev) = (tri[i], tri[(i + 1) % 3], tri[(i + 2) % 3]);
            let (out, back) = (self.curved(v, next), self.curved(prev, v));
            if !out && !back {
                return true;
            }
            let dir = |w: u32, curved: bool| {
                let p = self.p(v);
                if curved {
                    self.soup.curves[&(v.min(w), v.max(w))].ctrl - p
                } else {
                    self.p(w) - p
                }
            };
            let (l, b) = (dir(next, out), dir(prev, back));
            l.cross(b).dot(normal) > 1e-3 * l.length() * b.length() * normal.length()
        })
    }

    fn straight_sides(&self, tri: [u32; 3]) -> bool {
        (0..3).all(|i| !self.curved(tri[i], tri[(i + 1) % 3]))
    }

    /// The short edges of living triangles, each once, sorted.
    fn short_edges(&self) -> Vec<[u32; 2]> {
        let mut out = Vec::new();
        for (t, tri) in self.soup.tris.iter().enumerate() {
            if !self.alive[t] {
                continue;
            }
            for i in 0..3 {
                let (u, v) = (tri[i], tri[(i + 1) % 3]);
                if self.p(u).distance(self.p(v)) <= self.small && !self.curved(u, v) {
                    out.push([u.min(v), u.max(v)]);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Whether `v` lies inside a plane face: every living triangle round
    /// it is on that one face, and every edge from it straight. Moving it
    /// within the plane leaves the surface as it is.
    fn inside_plane(&self, v: u32) -> bool {
        let around = &self.around[v as usize];
        let Some(&first) = around.first() else {
            return false;
        };
        let face = self.soup.faces[first as usize];
        self.planes[face as usize].is_some()
            && around.iter().all(|&t| self.soup.faces[t as usize] == face)
            && self.neighbours(v).into_iter().all(|w| !self.curved(v, w))
    }

    /// The straight edges of living triangles longer than `small` and no
    /// longer than `thin`, with an end inside a plane face (see
    /// [`Self::inside_plane`]): as `[u, v]`, `v` that end, to be moved onto
    /// `u`. Such a vertex, a crossing a tie put a little way along an edge
    /// from where the cut passes (an edge of a cap tangent to a boss's rim
    /// crossed in and out a micrometre apart), leaves triangles no split
    /// mends: one of zero width between its two sides and the rim, whose
    /// corner at the rim is closed.
    fn plane_edges(&self) -> Vec<[u32; 2]> {
        let mut out = Vec::new();
        for (t, tri) in self.soup.tris.iter().enumerate() {
            if !self.alive[t] || self.planes[self.soup.faces[t] as usize].is_none() {
                continue;
            }
            for i in 0..3 {
                let (u, v) = (tri[i], tri[(i + 1) % 3]);
                let length = self.p(u).distance(self.p(v));
                if length <= self.small || length > self.thin || self.curved(u, v) {
                    continue;
                }
                for [u, v] in [[u, v], [v, u]] {
                    if self.inside_plane(v) {
                        out.push([u, v]);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The living triangles with both `u` and `v` as corners.
    fn shared(&self, u: u32, v: u32) -> Vec<u32> {
        self.around[u as usize]
            .iter()
            .copied()
            .filter(|&t| self.soup.tris[t as usize].contains(&v))
            .collect()
    }

    /// The vertices joined to `v` by an edge, sorted.
    fn neighbours(&self, v: u32) -> Vec<u32> {
        let mut out: Vec<u32> = self.around[v as usize]
            .iter()
            .flat_map(|&t| self.soup.tris[t as usize])
            .filter(|&w| w != v)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Collapses the edge `u`–`v` onto `u`, if allowed: every vertex round
    /// it keeps one fan (the surface stays a manifold), and no triangle
    /// turns over that `turn` doesn't allow. Two triangles the collapse
    /// makes the same but facing each other (a sheet of zero thickness
    /// folded onto the surface, which flush operands leave at vertices
    /// where the perturbation can't move every face outwards) both go.
    fn collapse(&mut self, u: u32, v: u32, turn: Turn) -> bool {
        let shared = self.shared(u, v);
        if shared.len() != 2 {
            return false;
        }
        // Each triangle going takes its two sides from the far corner into
        // one. Where their curves differ, the one between two faces (a
        // cut, which lies on both) stays; if that isn't one side, the
        // collapse can't be made.
        let mut merged: Vec<(u32, Option<Edge>)> = Vec::new();
        for &t in &shared {
            let tri = self.soup.tris[t as usize];
            let w = *tri
                .iter()
                .find(|&&x| x != u && x != v)
                .expect("a third corner");
            let record = |a: u32| self.soup.curves.get(&(a.min(w), a.max(w))).copied();
            let (ru, rv) = (record(u), record(v));
            if self.same_curve(u, v, w) {
                continue;
            }
            let source = |t: u32| self.soup.sources[self.soup.faces[t as usize] as usize];
            let between = |a: u32| {
                self.shared(a, w)
                    .iter()
                    .any(|&s| s != t && source(s) != source(t))
            };
            match (between(u), between(v)) {
                (true, false) => merged.push((w, ru)),
                (false, true) => merged.push((w, rv)),
                _ => {
                    return false;
                }
            }
        }
        let moved: Vec<u32> = self.around[v as usize]
            .iter()
            .copied()
            .filter(|t| !shared.contains(t))
            .collect();
        // Edges from `u` and from `v` to one vertex, other than the gone
        // triangles' sides, become one edge: only if they are one curve.
        // Else one of the two faces there would take the other's curve,
        // off its surface (a plane face once took a cylinder's inner
        // edge, 0.02 off the plane, and repair trusted its tag).
        let far: Vec<u32> = shared
            .iter()
            .flat_map(|&t| self.soup.tris[t as usize])
            .collect();
        let clash = self.neighbours(v).into_iter().any(|w| {
            !far.contains(&w) && !self.shared(u, w).is_empty() && !self.same_curve(u, v, w)
        });
        if clash {
            return false;
        }
        // Edges from `v` to where `u` has none: a record `u` has there is
        // one left from an edge gone, and gives way to `v`'s (or to none).
        let fresh: Vec<u32> = self
            .neighbours(v)
            .into_iter()
            .filter(|&w| w != u && self.shared(u, w).is_empty())
            .collect();
        let mut affected: Vec<u32> = [u, v]
            .into_iter()
            .chain(
                self.around[u as usize]
                    .iter()
                    .chain(&self.around[v as usize])
                    .flat_map(|&t| self.soup.tris[t as usize]),
            )
            .collect();
        affected.sort_unstable();
        affected.dedup();
        let saved_around: Vec<Vec<u32>> = affected
            .iter()
            .map(|&w| self.around[w as usize].clone())
            .collect();
        let saved_tris: Vec<[u32; 3]> = moved.iter().map(|&t| self.soup.tris[t as usize]).collect();

        for &t in &shared {
            self.kill(t);
        }
        for &t in &moved {
            for w in &mut self.soup.tris[t as usize] {
                if *w == v {
                    *w = u;
                }
            }
            self.around[u as usize].push(t);
        }
        self.around[v as usize].clear();
        let cancelled = self.cancel_pairs(u);

        let turned = cancelled.is_none()
            || moved
                .iter()
                .zip(&saved_tris)
                .any(|(&t, &old)| self.alive[t as usize] && self.turned(t, old, turn));
        if turned || affected.iter().any(|&w| w != v && !self.one_fan(w)) {
            // Undo.
            for &t in shared.iter().chain(cancelled.iter().flatten()) {
                self.alive[t as usize] = true;
            }
            for (&t, &old) in moved.iter().zip(&saved_tris) {
                self.soup.tris[t as usize] = old;
            }
            for (&w, list) in affected.iter().zip(saved_around) {
                self.around[w as usize] = list;
            }
            return false;
        }
        // The curves of the edges moved from `v` to `u`; where `u` had
        // that edge already, its own stays, unless the merge chose.
        for w in fresh {
            self.soup.curves.remove(&(u.min(w), u.max(w)));
        }
        for w in affected {
            if let Some(edge) = self.soup.curves.remove(&(v.min(w), v.max(w)))
                && w != u
            {
                self.soup.curves.entry((u.min(w), u.max(w))).or_insert(edge);
            }
        }
        for (w, edge) in merged {
            let k = (u.min(w), u.max(w));
            match edge {
                Some(edge) => self.soup.curves.insert(k, edge),
                None => self.soup.curves.remove(&k),
            };
            let on: Vec<u32> = self.shared(u, w);
            self.recurved.extend(on);
        }
        true
    }

    /// Whether triangle `t`, `old` before a collapse, turned over in a way
    /// `turn` doesn't allow.
    fn turned(&self, t: u32, old: [u32; 3], turn: Turn) -> bool {
        let new = self.soup.tris[t as usize];
        let over = self.normal(old).dot(self.normal(new)) <= 0.0;
        match turn {
            Turn::Proper => self.height(old).0 > self.small && over,
            Turn::Strict => over || !(self.height(new).0 > self.small && self.open(new)),
            Turn::Fold(star) => self.retag(star, t, old).is_none(),
        }
    }

    /// Moves the triangles a collapse gave a curve off their face's
    /// surface (a fitted cut's, where it took the place of the face's own
    /// curve) to a copy of the face claiming no surface, as the cuts' own
    /// bands go, so face tags stay true claims; one further off than the
    /// fit tolerance fails the operation as too complex, as the cuts'
    /// bands do.
    fn leave_surfaces(
        &mut self,
        faces: &mut Vec<Face>,
        tol: &Tolerance,
    ) -> Result<(), KernelError> {
        let mut touched = std::mem::take(&mut self.recurved);
        touched.sort_unstable();
        touched.dedup();
        let mut copies: BTreeMap<u32, u32> = BTreeMap::new();
        for t in touched {
            if !self.alive[t as usize] {
                continue;
            }
            let face = self.soup.faces[t as usize];
            let surface = faces[face as usize].surface;
            if matches!(surface, Surface::Free) {
                continue;
            }
            let tri = self.soup.tris[t as usize];
            let side = |i: usize| {
                let (u, v) = (tri[i], tri[(i + 1) % 3]);
                match self.soup.curves.get(&(u.min(v), u.max(v))) {
                    Some(e) => (e.ctrl, e.weight),
                    None => ((self.p(u) + self.p(v)) * 0.5, 1.0),
                }
            };
            let sides = [side(0), side(1), side(2)];
            let patch = Patch::new(
                tri.map(|v| self.p(v)),
                sides.map(|s| s.0),
                sides.map(|s| s.1),
            );
            if let Ok(patch) = &patch {
                // The control points bound a plane's distance, the samples
                // a quadric's (as the cuts' bands are measured).
                let off = off_surface(patch, &surface);
                if off <= tol.resolution() {
                    continue;
                }
                if off > tol.fit() {
                    return Err(KernelError::TooComplex);
                }
            }
            let copy = *copies.entry(face).or_insert_with(|| {
                faces.push(Face {
                    surface: Surface::Free,
                    ..faces[face as usize]
                });
                self.soup.sources.push(self.soup.sources[face as usize]);
                self.planar.push(false);
                faces.len() as u32 - 1
            });
            self.soup.faces[t as usize] = copy;
        }
        Ok(())
    }

    /// Whether the edges from `u` and from `v` (at one place) to `w`
    /// trace the same curve, within `small`.
    fn same_curve(&self, u: u32, v: u32, w: u32) -> bool {
        let record = |a: u32| self.soup.curves.get(&(a.min(w), a.max(w))).copied();
        match (record(u), record(v)) {
            (None, None) => true,
            (Some(_), None) => !self.curved(u, w),
            (None, Some(_)) => !self.curved(v, w),
            (Some(a), Some(b)) => {
                a.ctrl.distance(b.ctrl) <= self.small && (a.weight - b.weight).abs() <= 1e-6
            }
        }
    }

    /// Takes triangle `t` out.
    fn kill(&mut self, t: u32) {
        self.alive[t as usize] = false;
        for w in self.soup.tris[t as usize] {
            self.around[w as usize].retain(|&s| s != t);
        }
    }

    /// Takes out the pairs of triangles round `u` on the same corners
    /// facing each other, and gives them; `None` (and nothing taken out)
    /// when two are the same facing the same way.
    fn cancel_pairs(&mut self, u: u32) -> Option<Vec<u32>> {
        // Each triangle's corners from its lowest, and whether that is
        // the way it runs or the other.
        let mut keyed: Vec<([u32; 3], bool, u32)> = self.around[u as usize]
            .iter()
            .map(|&t| {
                let tri = self.soup.tris[t as usize];
                let i = (0..3).min_by_key(|&i| tri[i]).expect("three corners");
                let (a, b, c) = (tri[i], tri[(i + 1) % 3], tri[(i + 2) % 3]);
                if b < c {
                    ([a, b, c], true, t)
                } else {
                    ([a, c, b], false, t)
                }
            })
            .collect();
        keyed.sort_unstable();
        let mut out = Vec::new();
        for same in keyed.chunk_by(|x, y| x.0 == y.0) {
            match same {
                [_] => {}
                [(_, x, s), (_, y, t)] if x != y => out.extend([*s, *t]),
                _ => return None,
            }
        }
        for &t in &out {
            self.kill(t);
        }
        Some(out)
    }

    /// Whether the living triangles round `w` make one fan: their sides
    /// across `w` join up into a single loop (or there are none).
    fn one_fan(&self, w: u32) -> bool {
        let mut link: Vec<(u32, u32)> = self.around[w as usize]
            .iter()
            .map(|&t| {
                let tri = self.soup.tris[t as usize];
                let i = tri.iter().position(|&x| x == w).expect("a corner");
                (tri[(i + 1) % 3], tri[(i + 2) % 3])
            })
            .collect();
        if link.is_empty() {
            return true;
        }
        link.sort_unstable();
        if link.windows(2).any(|p| p[0].0 == p[1].0) {
            return false;
        }
        let mut ends: Vec<u32> = link.iter().map(|l| l.1).collect();
        ends.sort_unstable();
        if ends.windows(2).any(|p| p[0] == p[1]) {
            return false;
        }
        let first = link[0].0;
        let mut at = first;
        for step in 1..=link.len() {
            let Ok(i) = link.binary_search_by_key(&at, |l| l.0) else {
                return false;
            };
            at = link[i].1;
            if at == first {
                return step == link.len();
            }
        }
        false
    }

    /// Flips the longest side of the thin triangle `t`, if allowed: one
    /// no higher than `small` into any neighbour, one no higher than
    /// `thin` into a neighbour on its face, leaving no triangle thinner.
    fn flip(&mut self, t: u32) -> bool {
        let tri = self.soup.tris[t as usize];
        let (h, i) = self.height(tri);
        let [a, b, c] = [tri[i], tri[(i + 1) % 3], tri[(i + 2) % 3]];
        let Some(s) = self.across(t, a, b) else {
            return false;
        };
        let other = self.soup.tris[s as usize];
        let d = *other
            .iter()
            .find(|&&w| w != a && w != b)
            .expect("a third corner");
        let flat = self.straight_sides(other) || self.planar[self.soup.faces[s as usize] as usize];
        if d == c || self.neighbours(c).contains(&d) {
            return false;
        }
        // Into a curved neighbour only one of zero height, whose far corner
        // splits the neighbour's side: the new side is the neighbour's own
        // curve from there to its far corner, so the two new triangles are
        // exactly its pieces.
        let inner = if flat {
            None
        } else if h <= self.small {
            match self.inner_curve(other, [b, a], c) {
                Some(edge) => Some(edge),
                None => return false,
            }
        } else {
            return false;
        };
        let k = (c.min(d), c.max(d));
        // No triangle has the edge `c`–`d`: a record of it is one left from
        // an edge gone. The new side is the neighbour's inner curve, or
        // straight.
        self.soup.curves.remove(&k);
        if let Some(edge) = inner {
            self.soup.curves.insert(k, edge);
        }
        let (n1, n2) = ([c, a, d], [c, d, b]);
        if !(self.open(n1) && self.open(n2)) {
            if inner.is_some() {
                self.soup.curves.remove(&k);
            }
            return false;
        }
        if h > self.small
            && (self.soup.faces[s as usize] != self.soup.faces[t as usize]
                || self.height(n1).0.min(self.height(n2).0) <= h)
        {
            return false;
        }
        let before = self.normal(other);
        if self.height(other).0 > self.small
            && (self.normal(n1).dot(before) <= 0.0 || self.normal(n2).dot(before) <= 0.0)
        {
            if inner.is_some() {
                self.soup.curves.remove(&k);
            }
            return false;
        }
        self.swap(t, s, [a, b], c, d);
        self.soup.faces[t as usize] = self.soup.faces[s as usize];
        true
    }

    /// The living triangle across the side `u → v` of triangle `t`: the
    /// one running `v → u`.
    fn across(&self, t: u32, u: u32, v: u32) -> Option<u32> {
        self.shared(u, v).into_iter().find(|&s| {
            let o = self.soup.tris[s as usize];
            s != t && (0..3).any(|j| o[j] == v && o[(j + 1) % 3] == u)
        })
    }

    /// Flips the side `u → v` of triangle `t`, whose far corner is `a`,
    /// and of `s` across it, whose far corner is `b`: they become `[a, u,
    /// b]` and `[a, b, v]`.
    fn swap(&mut self, t: u32, s: u32, [u, v]: [u32; 2], a: u32, b: u32) {
        self.soup.tris[t as usize] = [a, u, b];
        self.soup.tris[s as usize] = [a, b, v];
        self.soup.made[t as usize] = true;
        self.soup.made[s as usize] = true;
        self.around[u as usize].retain(|&x| x != s);
        self.around[v as usize].retain(|&x| x != t);
        self.around[a as usize].push(s);
        self.around[b as usize].push(t);
    }

    /// The sine of the narrowest angle of `tri` (its corners' triangle).
    fn narrowest(&self, tri: [u32; 3]) -> f64 {
        let mut l = [0, 1, 2].map(|i| self.p(tri[(i + 1) % 3]).distance(self.p(tri[i])));
        l.sort_by(f64::total_cmp);
        let d = l[1] * l[2];
        if d > 0.0 {
            self.normal(tri).length() / d
        } else {
            0.0
        }
    }

    /// Flips a straight side of triangle `t` on a plane face, shared with a
    /// triangle of the same face, towards the Delaunay triangulation (the
    /// angles facing the side add up to more than π), where either of the
    /// two is a sliver ([`SLIVER`]) and the two new triangles are proper
    /// with their curved corners open. Cutting each input triangle on its
    /// own leaves slivers where the triangles were long and thin, as a
    /// plate's caps are between its corners and a hole: a second hole's
    /// rim then crosses their sides at a glancing angle, and triangles
    /// under 1e-7 radians wide at a far corner broke the hull rule there,
    /// which no split mends.
    fn delaunay(&mut self, t: u32) -> bool {
        let tri = self.soup.tris[t as usize];
        let face = self.soup.faces[t as usize];
        let Some((up, _)) = self.planes[face as usize] else {
            return false;
        };
        for i in 0..3 {
            let (u, v, a) = (tri[i], tri[(i + 1) % 3], tri[(i + 2) % 3]);
            if self.curved(u, v) {
                continue;
            }
            let Some(s) = self
                .across(t, u, v)
                .filter(|&s| self.soup.faces[s as usize] == face)
            else {
                continue;
            };
            let other = self.soup.tris[s as usize];
            let b = *other
                .iter()
                .find(|&&w| w != u && w != v)
                .expect("a third corner");
            if a == b || self.neighbours(a).contains(&b) {
                continue;
            }
            if self.narrowest(tri).min(self.narrowest(other)) > SLIVER {
                continue;
            }
            let angle = |x: u32, p: u32, q: u32| {
                let (e, f) = (self.p(p) - self.p(x), self.p(q) - self.p(x));
                trig::atan2(e.cross(f).length(), e.dot(f))
            };
            if angle(a, u, v) + angle(b, v, u) <= std::f64::consts::PI + 1e-9 {
                continue;
            }
            let (n1, n2) = ([a, u, b], [a, b, v]);
            let proper =
                |n: [u32; 3]| self.height(n).0 > self.small && self.normal(n).dot(up) > 0.0;
            if !(proper(n1) && proper(n2) && self.open(n1) && self.open(n2)) {
                continue;
            }
            self.swap(t, s, [u, v], a, b);
            // The new side is straight: no record from an edge there before.
            self.soup.curves.remove(&(a.min(b), a.max(b)));
            return true;
        }
        false
    }

    /// Flips a curved side of triangle `t` whose neighbour across it lies
    /// in the same plane (two plane faces of one plane meeting along a
    /// curve: a pin filling its hole, united with the plate), if the two
    /// make a convex quadrilateral whose new triangles are proper (as
    /// [`Self::straighten`] asks, the fold check included): the two
    /// new triangles cover the same region, whatever the curve between
    /// them, and no curve between two patches in one plane is left (no
    /// plane through it has either patch off it, so the hull rule can't
    /// hold there, and repair split along it down to flat pieces: 100 000
    /// patches for a plate). Tried where making the curve its chord
    /// ([`Self::straighten`]) leaves a triangle that isn't proper. Both new
    /// triangles go on the lower of the two faces where both are the same
    /// plane (and the faces are merged after the rounds), on `t`'s where
    /// the one across isn't (a wall's remnant at the rim, flat in the
    /// plane).
    fn unbend(&mut self, t: u32) -> bool {
        let tri = self.soup.tris[t as usize];
        let Some(plane) = self.own_plane(t) else {
            return false;
        };
        for i in 0..3 {
            let (u, v, a) = (tri[i], tri[(i + 1) % 3], tri[(i + 2) % 3]);
            let Some(s) = self.seam(t, u, v, plane) else {
                continue;
            };
            let b = *self.soup.tris[s as usize]
                .iter()
                .find(|&&w| w != u && w != v)
                .expect("a third corner");
            if a == b || self.neighbours(a).contains(&b) {
                continue;
            }
            // No triangle has the edge `a`–`b`: a record of it is one left
            // from an edge gone, and the new side is straight.
            self.soup.curves.remove(&(a.min(b), a.max(b)));
            let (n1, n2) = ([a, u, b], [a, b, v]);
            if !(self.proper_in(n1, plane.0) && self.proper_in(n2, plane.0)) {
                continue;
            }
            // `s` is on `t`'s face after this, or on one of the same plane.
            self.rejoin(t, s);
            let (ft, fs) = (self.soup.faces[t as usize], self.soup.faces[s as usize]);
            let face = ft.min(fs);
            self.soup.absorb(ft.max(fs), face);
            self.swap(t, s, [u, v], a, b);
            self.soup.faces[t as usize] = face;
            self.soup.faces[s as usize] = face;
            self.soup.curves.remove(&(u.min(v), u.max(v)));
            return true;
        }
        false
    }

    /// The curve from `c`, on the straight side `[b, a]` of the triangle
    /// `tri` (as `tri` runs it), to the triangle's corner across that
    /// side: its patch's own curve there, which splits it into two exact
    /// pieces (by blossoming, as bisecting a patch does). `None` if `c`
    /// isn't strictly inside the side or the patch can't be split there.
    fn inner_curve(&self, tri: [u32; 3], [b, a]: [u32; 2], c: u32) -> Option<Edge> {
        let j = (0..3).find(|&j| tri[j] == b && tri[(j + 1) % 3] == a)?;
        let side = |i: usize| {
            let (u, v) = (tri[i], tri[(i + 1) % 3]);
            match self.soup.curves.get(&(u.min(v), u.max(v))) {
                Some(e) => (e.ctrl, e.weight),
                None => ((self.p(u) + self.p(v)) * 0.5, 1.0),
            }
        };
        let sides = [side(0), side(1), side(2)];
        let patch = Patch::new(
            tri.map(|v| self.p(v)),
            sides.map(|s| s.0),
            sides.map(|s| s.1),
        )
        .ok()?;
        let (pb, pa) = (self.p(b), self.p(a));
        let along = pa - pb;
        let t = (self.p(c) - pb).dot(along) / along.length_squared();
        if !(t > 0.0 && t < 1.0) {
            return None;
        }
        let [first, _] = patch.bisect(j, t).ok()?;
        // The first piece runs corner `j`, the split point, the far
        // corner: its side from the split point is the curve.
        Some(Edge {
            ctrl: first.c[1],
            weight: first.w[1],
        })
    }

    /// The patch of triangle `tri` with its sides' curves, where some side
    /// is curved and the patch can be built.
    fn curved_patch(&self, tri: [u32; 3]) -> Option<Patch> {
        let sides = [0, 1, 2].map(|i| {
            let (u, v) = (tri[i], tri[(i + 1) % 3]);
            self.soup.curves.get(&(u.min(v), u.max(v))).copied()
        });
        if sides.iter().all(Option::is_none) {
            return None;
        }
        let side = |i: usize| {
            sides[i].map_or_else(
                || ((self.p(tri[i]) + self.p(tri[(i + 1) % 3])) * 0.5, 1.0),
                |e| (e.ctrl, e.weight),
            )
        };
        let sides = [side(0), side(1), side(2)];
        Patch::new(
            tri.map(|v| self.p(v)),
            sides.map(|s| s.0),
            sides.map(|s| s.1),
        )
        .ok()
    }

    /// Removes the connected components that enclose no volume: no more
    /// than `small` times their area. The volume counts the triangles'
    /// curved sides: a sliver cut off a wall along its rulings has every
    /// corner on the cutting plane, and by its corners alone encloses
    /// nothing.
    fn drop_empty_components(&mut self) {
        let n = self.soup.pos.len();
        let living = || {
            self.soup
                .tris
                .iter()
                .zip(&self.alive)
                .filter(|(_, alive)| **alive)
                .map(|(tri, _)| *tri)
        };
        let part = parts(n, living().flat_map(|[a, b, c]| [[a, b], [a, c]]));
        let mut volume = vec![0.0f64; n];
        let mut area = vec![0.0f64; n];
        for tri in living() {
            let root = part[tri[0] as usize];
            let o = self.soup.pos[root as usize];
            let [a, b, c] = tri.map(|v| self.soup.pos[v as usize] - o);
            volume[root as usize] += self.curved_patch(tri).map_or_else(
                || a.dot(b.cross(c)) / 6.0,
                |patch| patch_volume(&patch, o).0,
            );
            area[root as usize] += (b - a).cross(c - a).length() / 2.0;
        }
        for (tri, alive) in self.soup.tris.iter().zip(&mut self.alive) {
            let root = part[tri[0] as usize] as usize;
            if *alive && volume[root].abs() <= self.small * area[root] {
                *alive = false;
            }
        }
    }
}

/// The thin triangles left across two faces in a cleaned `soup`: a
/// triangle higher than `small` but no higher than `thin` over its longest
/// side, the triangle across that side on another face (by source). The
/// first count is all of them; the second only those with straight sides
/// whose far corner lies inside a plane (every triangle round it on a
/// plane face, the planes one within `small`, every edge from it
/// straight), which splitting the long side at the corner's foot and
/// collapsing the corner onto it would take away without moving the
/// surface. For measuring only.
#[cfg(test)]
pub(super) fn thin_across(soup: &Soup, faces: &[Face], small: f64, thin: f64) -> (usize, usize) {
    let p = |v: u32| soup.pos[v as usize];
    let mut across = BTreeMap::new();
    let mut around = vec![Vec::new(); soup.pos.len()];
    for (t, tri) in soup.tris.iter().enumerate() {
        for i in 0..3 {
            across.insert((tri[i], tri[(i + 1) % 3]), t);
            around[tri[i] as usize].push(t);
        }
    }
    let curved = |u: u32, v: u32| {
        soup.curves
            .get(&(u.min(v), u.max(v)))
            .is_some_and(|edge| !straight(p(u), edge.ctrl, p(v), small))
    };
    let plane = |t: usize| match faces[soup.faces[t] as usize].surface {
        Surface::Plane { n, d } => {
            let len = n.length();
            (len > 0.0 && len.is_finite()).then(|| (n / len, d / len))
        }
        _ => None,
    };
    let inside_plane = |c: u32| {
        let round = &around[c as usize];
        let Some(first) = round.first().and_then(|&t| plane(t)) else {
            return false;
        };
        round.iter().all(|&t| {
            plane(t)
                .is_some_and(|(n, d)| n.dot(first.0) >= 1.0 - 1e-12 && (d - first.1).abs() <= small)
        }) && round
            .iter()
            .flat_map(|&t| soup.tris[t])
            .all(|w| w == c || !curved(c, w))
    };
    let (mut all, mut mendable) = (0, 0);
    for (t, &tri) in soup.tris.iter().enumerate() {
        let lengths = [0, 1, 2].map(|i| p(tri[(i + 1) % 3]).distance(p(tri[i])));
        let i = (0..3)
            .max_by(|&i, &j| lengths[i].total_cmp(&lengths[j]).then(j.cmp(&i)))
            .expect("three sides");
        let area = (p(tri[1]) - p(tri[0]))
            .cross(p(tri[2]) - p(tri[0]))
            .length();
        let h = if lengths[i] > 0.0 {
            area / lengths[i]
        } else {
            0.0
        };
        if h <= small || h > thin {
            continue;
        }
        let (a, b, c) = (tri[i], tri[(i + 1) % 3], tri[(i + 2) % 3]);
        let Some(&s) = across.get(&(b, a)) else {
            continue;
        };
        let source = |t: usize| soup.sources[soup.faces[t] as usize];
        if source(s) == source(t) {
            continue;
        }
        all += 1;
        if !curved(a, b) && !curved(b, c) && !curved(c, a) && inside_plane(c) {
            mendable += 1;
        }
    }
    (all, mendable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_collapse_drops_a_record_left_where_an_edge_moves() {
        // A double pyramid over a ring of seven, two of the ring's
        // vertices (`u` = 0 and `v` = 1) a hair apart. Collapsing `v` onto
        // `u` moves `v`'s edge to vertex 2 onto `u`, which had none there
        // but a record left from an edge gone: the new edge is straight,
        // as `v`'s was, not that curve.
        let mut pos = vec![DVec3::X, DVec3::new(1.0, 1e-12, 0.0)];
        for i in 2..7 {
            let a = std::f64::consts::TAU * f64::from(i) / 7.0;
            let (sin, cos) = trig::sin_cos(a);
            pos.push(DVec3::new(cos, sin, 0.0));
        }
        let (top, bottom) = (7, 8);
        pos.extend([DVec3::Z, -DVec3::Z]);
        let mut tris = Vec::new();
        for i in 0..7 {
            let j = (i + 1) % 7;
            tris.push([top, i, j]);
            tris.push([bottom, j, i]);
        }
        let mut curves = Curves::new();
        curves.insert(
            (0, 2),
            Edge {
                ctrl: DVec3::new(0.0, 0.0, 0.5),
                weight: 1.0,
            },
        );
        let mut soup = Soup {
            pos,
            faces: vec![0; tris.len()],
            tris,
            curves,
            sources: vec![0],
            absorbed: Vec::new(),
            made: vec![true; 14],
        };
        let mut around = vec![Vec::new(); soup.pos.len()];
        for (t, tri) in soup.tris.iter().enumerate() {
            for &v in tri {
                around[v as usize].push(t as u32);
            }
        }
        let mut c = Cleaner {
            alive: vec![true; soup.tris.len()],
            soup: &mut soup,
            around,
            planar: vec![false],
            recurved: Vec::new(),
            planes: vec![None],
            joined: Vec::new(),
            small: 1e-9,
            thin: 1e-9,
        };
        assert!(c.collapse(0, 1, Turn::Proper));
        assert_eq!(c.shared(0, 2).len(), 2);
        assert!(!c.curved(0, 2));
    }
}
