//! Curved edges between two triangles in one plane: seams.
//!
//! Where two operands' caps are flush and face the same way, the
//! perturbation keeps one cap whole and cuts the other along the first
//! one's rim, and the rim's sliver of wall collapses away. What is left is
//! a curve between two patches in one plane, or a cluster of zero-size
//! triangles there. No plane through such a curve has either patch off
//! it, so the hull rule can't hold across it, and repair split along it
//! down to flat pieces: 29 000 patches for a boss united with the plate it
//! stands in over one span, 115 000 at millimetre scale, whichever went
//! first when the two only overlapped.
//!
//! Three passes take them away, none moving the surface off its plane:
//!
//! - [`Cleaner::straighten`], in the rounds: the curve becomes its chord.
//!   Both triangles lie in the plane, so their union stays exactly what
//!   it was (the lens between curve and chord moves from one to the
//!   other), as long as both are still proper, face along the plane's
//!   normal, keep their curved corners open and don't fold.
//! - [`Cleaner::dissolve`], once after the rounds, for what that leaves:
//!   the region of triangles in the plane linked across seams, short
//!   edges and triangles of zero height, triangulated again from its
//!   boundary loops, its inner vertices dropped. Only when the new
//!   triangles are proper and open, add no point and no edge already
//!   there, and cover the same area.
//! - [`Cleaner::merge_joined`]: the plane faces such seams joined become
//!   one face (the lowest id, so the first operand's name stays), so the
//!   sliver flips after it, which stay within a face, reach across the
//!   old rim. Naming is left to `Mesh::merge_faces` after repair, which
//!   names every pair of adjacent faces on one surface alike: a
//!   straightened rim isn't drawn on the flat top as a polygon of chords.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};

use super::super::triangulate::{Bends, Meter, NO_CUT, Vert, triangulate};
use super::Cleaner;
use crate::KernelError;
use crate::budget::Work;
use crate::mesh::{Face, Surface};

/// How many steps of the triangulation one region may take, past which it
/// is left as it is (16 steps a unit of work, as the faces' cuts count).
pub(super) const REGION_STEPS: u64 = 1 << 20;

/// Steps of a region's triangulation a unit of work pays for.
pub(super) const STEPS_PER_UNIT: u64 = 16;

#[cfg(test)]
thread_local! {
    /// Tests only: how many regions [`Cleaner::dissolve`] triangulated
    /// again, counted on the thread that cleans.
    pub(in crate::boolean) static DISSOLVED: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// A plane: unit normal and offset.
pub(super) type Plane = (DVec3, f64);

impl Cleaner<'_> {
    /// Whether triangle `t` lies in the plane `(n, d)`: its corners and
    /// the control points of its curved sides within `small` of it.
    pub(super) fn in_plane(&self, t: u32, (n, d): Plane) -> bool {
        let tri = self.soup.tris[t as usize];
        let near = |p: DVec3| (p.dot(n) - d).abs() <= self.small;
        (0..3).all(|i| {
            let (u, v) = (tri[i], tri[(i + 1) % 3]);
            near(self.p(u))
                && self
                    .soup
                    .curves
                    .get(&(u.min(v), u.max(v)))
                    .is_none_or(|e| near(e.ctrl))
        })
    }

    /// The plane of triangle `t`'s face, if `t` has a curved side, its
    /// face is a plane and it lies in it: where a seam may be.
    pub(super) fn own_plane(&self, t: u32) -> Option<Plane> {
        let plane = self.planes[self.soup.faces[t as usize] as usize]?;
        let tri = self.soup.tris[t as usize];
        let curved = (0..3).any(|i| self.curved(tri[i], tri[(i + 1) % 3]));
        (curved && self.in_plane(t, plane)).then_some(plane)
    }

    /// The neighbour across the curved side `u → v` of triangle `t`, in
    /// `plane`: a seam.
    pub(super) fn seam(&self, t: u32, u: u32, v: u32, plane: Plane) -> Option<u32> {
        if !self.curved(u, v) {
            return None;
        }
        self.across(t, u, v).filter(|&s| self.in_plane(s, plane))
    }

    /// Whether `tri`, a triangle in a plane of normal `up`, is proper
    /// there: higher than `small`, facing along `up`, its curved corners
    /// open, and its patch free of folds.
    pub(super) fn proper_in(&self, tri: [u32; 3], up: DVec3) -> bool {
        self.height(tri).0 > self.small
            && self.normal(tri).dot(up) > 0.0
            && self.open(tri)
            && self
                .curved_patch(tri)
                .is_none_or(|patch| patch.fold_direction().is_some())
    }

    /// Makes each seam of triangle `t` (on a plane face, in its plane) its
    /// chord, where both triangles stay proper (see the
    /// [module](self) docs). Gives whether it made any.
    pub(super) fn straighten(&mut self, t: u32) -> bool {
        let Some(plane) = self.own_plane(t) else {
            return false;
        };
        let tri = self.soup.tris[t as usize];
        let mut any = false;
        for i in 0..3 {
            let (u, v) = (tri[i], tri[(i + 1) % 3]);
            let Some(s) = self.seam(t, u, v, plane) else {
                continue;
            };
            let k = (u.min(v), u.max(v));
            let Some(saved) = self.soup.curves.remove(&k) else {
                continue;
            };
            let other = self.soup.tris[s as usize];
            if self.proper_in(tri, plane.0) && self.proper_in(other, plane.0) {
                self.rejoin(t, s);
                any = true;
            } else {
                self.soup.curves.insert(k, saved);
            }
        }
        any
    }

    /// Whether face `f` is a plane, the same as `(n, d)` (facing the same
    /// way, its offset within `small`).
    fn same_plane(&self, f: u32, (n, d): Plane) -> bool {
        self.planes[f as usize]
            .is_some_and(|(m, e)| m.dot(n) > 1.0 - 1e-12 && (e - d).abs() <= self.small)
    }

    /// Triangle `s` lies in the plane of `t`'s face, and a seam between
    /// them went: a face of `s` that is the same plane is recorded as
    /// joined to `t`'s (to be merged), any other (a remnant of a wall at
    /// the rim, of zero height in the plane) moves onto `t`'s, whose plane
    /// it lies in, rather than keep claiming a surface it has left.
    pub(super) fn rejoin(&mut self, t: u32, s: u32) {
        let (ft, fs) = (self.soup.faces[t as usize], self.soup.faces[s as usize]);
        let plane = self.planes[ft as usize].expect("`t` is on a plane face");
        if !self.same_plane(fs, plane) {
            self.soup.faces[s as usize] = ft;
        } else if fs != ft {
            self.joined.push((ft.min(fs), ft.max(fs)));
        }
    }

    /// Merges the plane faces seams joined, each set onto its lowest id:
    /// a face whose triangles all lie in the plane of that lowest one
    /// moves onto it, its key an alias of it from then on
    /// ([`Soup::absorb`](super::Soup::absorb)), with the copies of it
    /// claiming no surface (which take its name and form). A face whose
    /// triangles don't stays as it is.
    pub(super) fn merge_joined(&mut self, faces: &mut [Face]) {
        if self.joined.is_empty() {
            return;
        }
        let mut joined = std::mem::take(&mut self.joined);
        joined.sort_unstable();
        joined.dedup();
        let mut root: BTreeMap<u32, u32> = BTreeMap::new();
        let find = |root: &BTreeMap<u32, u32>, mut f: u32| {
            while let Some(&up) = root.get(&f) {
                if up == f {
                    break;
                }
                f = up;
            }
            f
        };
        for &(f, g) in &joined {
            let (rf, rg) = (find(&root, f), find(&root, g));
            let (low, high) = (rf.min(rg), rf.max(rg));
            root.insert(low, low);
            root.insert(high, low);
        }
        let members: Vec<(u32, u32)> = root
            .keys()
            .map(|&f| (f, find(&root, f)))
            .filter(|&(f, r)| f != r)
            .collect();
        let mut on: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (t, &face) in self.soup.faces.iter().enumerate() {
            if self.alive[t] && root.contains_key(&face) {
                on.entry(face).or_default().push(t as u32);
            }
        }
        for (f, r) in members {
            let Some(plane) = self.planes[r as usize] else {
                continue;
            };
            let tris = on.get(&f).map_or(&[][..], Vec::as_slice);
            let facing = self.planes[f as usize].is_some_and(|(m, _)| m.dot(plane.0) > 0.0);
            if !facing || !tris.iter().all(|&t| self.in_plane(t, plane)) {
                continue;
            }
            for &t in tris {
                self.soup.faces[t as usize] = r;
            }
            // Its key names `r` from now on.
            self.soup.absorb(f, r);
            // Copies of it claiming no surface go with it.
            for c in 0..faces.len() {
                if c as u32 != f
                    && self.soup.sources[c] == f
                    && matches!(faces[c].surface, Surface::Free)
                {
                    self.soup.sources[c] = r;
                    faces[c].name = faces[r as usize].name;
                    faces[c].form = faces[r as usize].form;
                }
            }
        }
    }

    /// Triangulates again each region of triangles in one plane that
    /// still holds a seam: see the [module](self) docs. A region that
    /// can't be is left as it is.
    pub(super) fn dissolve(&mut self, work: &mut Work) -> Result<(), KernelError> {
        let count = self.soup.tris.len();
        let mut done = vec![false; count];
        for seed in 0..count as u32 {
            if !self.alive[seed as usize] || done[seed as usize] {
                continue;
            }
            let Some(plane) = self.own_plane(seed) else {
                continue;
            };
            let tri = self.soup.tris[seed as usize];
            let seamed = (0..3).any(|i| self.seam(seed, tri[i], tri[(i + 1) % 3], plane).is_some());
            if !seamed {
                continue;
            }
            let region = self.region(seed, plane);
            for &t in &region {
                done[t as usize] = true;
            }
            work.spend(region.len())?;
            let budget = work.left().min(REGION_STEPS / STEPS_PER_UNIT);
            let meter = Meter::new(budget.saturating_mul(STEPS_PER_UNIT));
            let made = self.retriangulate(&region, plane, &meter);
            work.spend(usize::try_from(meter.used() / STEPS_PER_UNIT).unwrap_or(usize::MAX))?;
            if let Some(made) = made {
                self.replace(&region, made);
                // A later region may reach the new triangles: they are
                // done, as seeds go.
                done.resize(self.soup.tris.len(), true);
                #[cfg(test)]
                DISSOLVED.set(DISSOLVED.get() + 1);
            }
        }
        Ok(())
    }

    /// The triangles in `plane` reached from `seed` across seams, edges no
    /// longer than `small` and any side of a triangle no higher than
    /// `small`, in the order found.
    fn region(&self, seed: u32, plane: Plane) -> Vec<u32> {
        let mut region = vec![seed];
        let mut inside = BTreeSet::from([seed]);
        let mut k = 0;
        while k < region.len() {
            let t = region[k];
            k += 1;
            let tri = self.soup.tris[t as usize];
            let flat_t = self.height(tri).0 <= self.small;
            for i in 0..3 {
                let (u, v) = (tri[i], tri[(i + 1) % 3]);
                let Some(s) = self.across(t, u, v) else {
                    continue;
                };
                if inside.contains(&s) || !self.in_plane(s, plane) {
                    continue;
                }
                let link = self.curved(u, v)
                    || flat_t
                    || self.p(u).distance(self.p(v)) <= self.small
                    || self.height(self.soup.tris[s as usize]).0 <= self.small;
                if link {
                    inside.insert(s);
                    region.push(s);
                }
            }
        }
        region
    }

    /// The triangles covering `region` (in `plane`) again from its
    /// boundary, on the same vertices less the inner ones, if every check
    /// passes (see [`Self::dissolve`]). The records of the region's inner
    /// edges are taken out then (and of any edge no triangle has where a
    /// new one runs), and only then.
    pub(super) fn retriangulate(
        &mut self,
        region: &[u32],
        (n, _): Plane,
        meter: &Meter,
    ) -> Option<Vec<[u32; 3]>> {
        let inside: BTreeSet<u32> = region.iter().copied().collect();
        // The boundary: sides whose triangle across is outside, by start.
        let mut next: BTreeMap<u32, u32> = BTreeMap::new();
        let mut sides = BTreeSet::new();
        let mut inner = BTreeSet::new();
        let mut area = 0.0;
        for &t in region {
            let tri = self.soup.tris[t as usize];
            area += self.normal(tri).dot(n);
            for i in 0..3 {
                let (u, v) = (tri[i], tri[(i + 1) % 3]);
                if self.across(t, u, v).is_some_and(|s| inside.contains(&s)) {
                    inner.insert((u.min(v), u.max(v)));
                    continue;
                }
                // A vertex the boundary passes twice (a pinch), or a side
                // about zero long (a cluster on the boundary): not here.
                if next.insert(u, v).is_some() || self.p(u).distance(self.p(v)) <= self.small {
                    return None;
                }
                sides.insert((u, v));
            }
        }
        // A frame in the plane, the region scaled to about unit size, as
        // the triangulation's lengths assume.
        let e1 = n.any_orthonormal_vector();
        let e2 = n.cross(e1);
        let o = self.p(*next.keys().next()?);
        let flat = |p: DVec3| DVec2::new((p - o).dot(e1), (p - o).dot(e2));
        let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
        for &v in next.keys() {
            let q = flat(self.p(v));
            lo = lo.min(q);
            hi = hi.max(q);
        }
        let scale = (hi - lo).max_element();
        if !(scale > 0.0 && scale.is_finite()) {
            return None;
        }
        let at = |v: u32| (flat(self.p(v)) - lo) / scale;
        let towards = |d: DVec3| DVec2::new(d.dot(e1), d.dot(e2)) / scale;
        let mut loops = Vec::new();
        let mut seen = BTreeSet::new();
        for &start in next.keys() {
            let mut lp = Vec::new();
            let mut v = start;
            while seen.insert(v) {
                lp.push(Vert {
                    id: v,
                    at: at(v),
                    sides: 0,
                    cuts: [NO_CUT; 2],
                });
                v = *next.get(&v)?;
            }
            if lp.is_empty() {
                continue;
            }
            if v != start {
                return None;
            }
            loops.push(lp);
        }
        let mut bends = Bends::new();
        for &(u, v) in &sides {
            if self.curved(u, v) {
                let c = self.soup.curves[&(u.min(v), u.max(v))].ctrl;
                bends.insert((u, v), [towards(c - self.p(u)), towards(c - self.p(v))]);
            }
        }
        let first = u32::try_from(self.soup.pos.len()).ok()?;
        let made = triangulate(loops, &bends, first, None, meter).ok()?;
        if meter.over() || !made.split.is_empty() || !made.steiner.is_empty() {
            return None;
        }
        // No new inner side may be an edge outside the region already.
        for tri in &made.tris {
            for i in 0..3 {
                let (u, v) = (tri[i], tri[(i + 1) % 3]);
                if !sides.contains(&(u, v)) && self.shared(u, v).iter().any(|t| !inside.contains(t))
                {
                    return None;
                }
            }
        }
        // The new inner sides are straight: the old inner edges' records go,
        // and any left from an edge no triangle has any more.
        let saved: Vec<_> = inner
            .iter()
            .filter_map(|k| self.soup.curves.remove(k).map(|e| (*k, e)))
            .collect();
        for tri in &made.tris {
            for i in 0..3 {
                let (u, v) = (tri[i], tri[(i + 1) % 3]);
                if !sides.contains(&(u, v)) && !inner.contains(&(u.min(v), u.max(v))) {
                    self.soup.curves.remove(&(u.min(v), u.max(v)));
                }
            }
        }
        // Every new triangle proper, and the corner triangles' signed areas
        // adding up alike (the curved sides inside cancel in pairs, the
        // boundary's are the same).
        let mut new_area = 0.0;
        let mut proper = true;
        for &tri in &made.tris {
            proper &= self.proper_in(tri, n);
            new_area += self.normal(tri).dot(n);
        }
        if proper && area > 0.0 && (new_area - area).abs() <= 1e-9 * area {
            return Some(made.tris);
        }
        for (k, e) in saved {
            self.soup.curves.insert(k, e);
        }
        None
    }

    /// Puts the triangles `made` in place of `region`'s, on the face of
    /// its first (the seed's), recording the region's other faces in that
    /// plane as joined to it and absorbed by it
    /// ([`Soup::absorb`](super::Soup::absorb)).
    pub(super) fn replace(&mut self, region: &[u32], made: Vec<[u32; 3]>) {
        let seed = region[0];
        let face = self.soup.faces[seed as usize];
        let plane = self.planes[face as usize].expect("the seed is on a plane face");
        for &t in region {
            let f = self.soup.faces[t as usize];
            if f != face && self.same_plane(f, plane) {
                self.joined.push((face.min(f), face.max(f)));
                self.soup.absorb(f, face);
            }
            self.kill(t);
        }
        for tri in made {
            let t = u32::try_from(self.soup.tris.len()).expect("fewer triangles than u32::MAX");
            self.soup.tris.push(tri);
            self.soup.faces.push(face);
            self.alive.push(true);
            self.soup.made.push(true);
            for v in tri {
                self.around[v as usize].push(t);
            }
        }
    }
}
