//! Removing the degenerate triangles a boolean leaves where the operands
//! are flush.
//!
//! The counting decides the topology for slightly perturbed operands, so
//! where they touch exactly (flush faces, a vertex on a face) the result
//! has pieces of zero size: edges whose ends coincide, triangles whose
//! corners lie on a line. This is the flat version of Manifold's
//! clean-up. Short edges are collapsed, keeping the lower vertex id (the
//! operands' own vertices come first) when the collapse keeps a manifold
//! (the link condition: the ends' only common neighbours are the two
//! corners opposite the edge) and turns no proper triangle over; a
//! triangle of zero height whose edges aren't short has its longest edge
//! flipped, which splits the triangle beyond at the far corner and leaves
//! the same surface. Components that enclose no volume go. It never
//! decides that two separate vertices are one: a collapse removes an
//! edge, keeping the surface a closed manifold.

use glam::DVec3;

use super::parts;
use crate::KernelError;
use crate::budget::Work;

/// How many rounds of collapses and flips at most.
const ROUNDS: usize = 64;

/// A triangle soup being cleaned: positions, triangles on vertex ids,
/// and each triangle's face.
#[derive(Debug, Clone)]
pub(super) struct Soup {
    pub(super) pos: Vec<DVec3>,
    pub(super) tris: Vec<[u32; 3]>,
    pub(super) faces: Vec<u32>,
}

struct Cleaner<'a> {
    soup: &'a mut Soup,
    alive: Vec<bool>,
    /// Each vertex's triangles.
    around: Vec<Vec<u32>>,
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
        self.height(self.soup.tris[t as usize]).0 <= self.thin.max(self.small)
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
                if self.p(u).distance(self.p(v)) <= self.small {
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

    /// Collapses the edge `u`–`v` (`u < v`) onto `u`, if allowed.
    fn collapse(&mut self, u: u32, v: u32) -> bool {
        let shared = self.shared(u, v);
        if shared.len() != 2 {
            return false;
        }
        let mut opposite: Vec<u32> = shared
            .iter()
            .map(|&t| {
                let tri = self.soup.tris[t as usize];
                *tri.iter()
                    .find(|&&w| w != u && w != v)
                    .expect("a third corner")
            })
            .collect();
        opposite.sort_unstable();
        if opposite[0] == opposite[1] {
            return false;
        }
        let (nu, nv) = (self.neighbours(u), self.neighbours(v));
        let common: Vec<u32> = nu.iter().copied().filter(|w| nv.contains(w)).collect();
        if common != opposite {
            return false;
        }
        let moved: Vec<u32> = self.around[v as usize]
            .iter()
            .copied()
            .filter(|t| !shared.contains(t))
            .collect();
        for &t in &moved {
            let old = self.soup.tris[t as usize];
            let new = old.map(|w| if w == v { u } else { w });
            if self.height(old).0 > self.small && self.normal(old).dot(self.normal(new)) <= 0.0 {
                return false;
            }
        }
        for &t in &shared {
            self.alive[t as usize] = false;
            for w in self.soup.tris[t as usize] {
                self.around[w as usize].retain(|&s| s != t);
            }
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
        true
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
        if d == c || self.neighbours(c).contains(&d) {
            return false;
        }
        let (n1, n2) = ([c, a, d], [c, d, b]);
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
