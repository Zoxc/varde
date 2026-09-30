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
//! together) and turns no proper triangle over; a
//! triangle of zero height whose edges aren't short has its longest edge
//! flipped, which splits the triangle beyond at the far corner and leaves
//! the same surface. Components that enclose no volume go. It never
//! decides that two separate vertices are one: a collapse removes an
//! edge, keeping the surface a closed manifold.
//!
//! Curved edges (those with a record in [`Soup::curves`]) are never
//! collapsed or flipped, nor are triangles with one: the clean-up is
//! about the flat triangles flush planar faces leave. A collapse moves the
//! curves of the edges it moves onto the vertex kept.

use glam::DVec3;

use super::assemble::Curves;
use super::parts;
use crate::KernelError;
use crate::budget::Work;
use crate::mesh::{Edge, straight};

/// How many rounds of collapses and flips at most.
const ROUNDS: usize = 64;

/// A triangle soup being cleaned: positions, triangles on vertex ids,
/// each triangle's face, and the curves of its edges that aren't
/// straight (by the vertices they join, the lower first; records of edges
/// no triangle has any more are left, and ignored).
#[derive(Debug, Clone)]
pub(super) struct Soup {
    pub(super) pos: Vec<DVec3>,
    pub(super) tris: Vec<[u32; 3]>,
    pub(super) faces: Vec<u32>,
    pub(super) curves: Curves,
}

struct Cleaner<'a> {
    soup: &'a mut Soup,
    alive: Vec<bool>,
    /// Each vertex's triangles.
    around: Vec<Vec<u32>>,
    /// Whether each face is a plane: a triangle of one with curved sides
    /// may still be flipped into, its new inner side straight in the
    /// plane.
    planar: &'a [bool],
    /// Edges no longer than this are short, triangles no higher are flat.
    small: f64,
    /// Triangles no higher than this are thin: flipped when the triangle
    /// across their longest side is on the same face.
    thin: f64,
}

/// Cleans `soup`: see the module docs. `small` is the size below which
/// edges and heights count as zero, and `thin` the height below which a
/// triangle is flipped into its neighbour on the same face (which keeps
/// every triangle on its face).
pub(super) fn clean(
    soup: &mut Soup,
    planar: &[bool],
    small: f64,
    thin: f64,
    work: &mut Work,
) -> Result<(), KernelError> {
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
        small,
        thin,
    };
    for _ in 0..ROUNDS {
        work.spend(c.soup.tris.len())?;
        let mut changed = false;
        for [u, v] in c.short_edges() {
            // An earlier collapse may have taken the edge already: then no
            // triangle has both ends.
            if c.collapse(u, v) {
                changed = true;
            }
        }
        for t in 0..c.soup.tris.len() as u32 {
            if c.alive[t as usize] && c.thin(t) && c.flip(t) {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    c.drop_empty_components();
    let Cleaner { alive, soup, .. } = c;
    let mut keep = alive.iter();
    soup.faces.retain(|_| *keep.next().expect("a flag"));
    let mut keep = alive.iter();
    soup.tris.retain(|_| *keep.next().expect("a flag"));
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

    /// Collapses the edge `u`–`v` (`u < v`) onto `u`, if allowed: every
    /// vertex round it keeps one fan (the surface stays a manifold), and
    /// no proper triangle turns over. Two triangles the collapse makes
    /// the same but facing each other (a sheet of zero thickness folded
    /// onto the surface, which flush operands leave at vertices where
    /// the perturbation can't move every face outwards) both go.
    fn collapse(&mut self, u: u32, v: u32) -> bool {
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
            let between = |a: u32| {
                self.shared(a, w)
                    .iter()
                    .any(|&s| s != t && self.soup.faces[s as usize] != self.soup.faces[t as usize])
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
            || moved.iter().zip(&saved_tris).any(|(&t, &old)| {
                self.alive[t as usize]
                    && self.height(old).0 > self.small
                    && self
                        .normal(old)
                        .dot(self.normal(self.soup.tris[t as usize]))
                        <= 0.0
            });
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
        }
        true
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
        // The triangle across `a → b` runs `b → a`.
        let Some(&s) = self.shared(a, b).iter().find(|&&s| {
            let o = self.soup.tris[s as usize];
            s != t && (0..3).any(|j| o[j] == b && o[(j + 1) % 3] == a)
        }) else {
            return false;
        };
        let other = self.soup.tris[s as usize];
        let d = *other
            .iter()
            .find(|&&w| w != a && w != b)
            .expect("a third corner");
        let flat = self.straight_sides(other) || self.planar[self.soup.faces[s as usize] as usize];
        if d == c || self.neighbours(c).contains(&d) || !flat {
            return false;
        }
        let (n1, n2) = ([c, a, d], [c, d, b]);
        if !(self.open(n1) && self.open(n2)) {
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
            return false;
        }
        self.soup.tris[t as usize] = n1;
        self.soup.tris[s as usize] = n2;
        self.soup.faces[t as usize] = self.soup.faces[s as usize];
        self.around[a as usize].retain(|&x| x != s);
        self.around[b as usize].retain(|&x| x != t);
        self.around[c as usize].push(s);
        self.around[d as usize].push(t);
        true
    }

    /// Removes the connected components that enclose no volume: no more
    /// than `small` times their area.
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
            volume[root as usize] += a.dot(b.cross(c)) / 6.0;
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
