//! Folded sheets whose two sides are triangulated differently.
//!
//! At a saddle vertex of a flush contact no perturbation direction leaves
//! by every face, so some face moves the wrong way, and where it is flush
//! with the other operand a sheet of zero thickness is left folded onto
//! the surface. Its two sides come one from each operand, each
//! triangulated on its own, so no collapse makes two of their triangles
//! the same ([`Cleaner::cancel_pairs`]): a vertex whose star lies in a
//! plane (or in two or three, on a crease) with proper triangles in one
//! plane facing both ways. The final check fails it (`VertexNeighbours`,
//! or repair's `Fold`).
//!
//! If every triangle round `v` lies within `small` of one of the star's
//! planes and the neighbour `w` lies in all of them, moving `v` to `w`
//! moves each triangle `v a b` through the tetrahedron `v w a b`, whose
//! four corners are within `small` of one plane: the winding numbers
//! change only within `small` of the star's planes, over the star, so the
//! oriented surface and the volume stay as they were (to that), and the
//! folded part cancels. This holds whatever order the planes take round
//! `v` (on a crease, `v` moves along the line where they meet). It is the
//! argument of the collapse inside a plane face, for stars facing both
//! ways and on a crease. Only plane faces take part: a triangle of a
//! curved face with straight sides lies in a plane only where it is thin,
//! and moving its corner along that plane would take it off its surface.
//! Triangles may turn over doing it, which the other collapses refuse, so
//! the rule is a last resort (only in a round where nothing else changed)
//! and has to leave less unsigned area round `v` than there was (turned
//! triangles cover theirs twice over): without that, a first try onto any
//! neighbour made a turned triangle overlapping others
//! outside the star, and broke a chain that worked before.

use std::cmp::Ordering;

use glam::DVec3;

use super::seams::Plane;
use super::{Cleaner, Turn};

/// A folded star: see [`Cleaner::folded`].
pub(super) struct Star {
    /// The planes, each through the star's vertex with the normal of the
    /// largest proper triangle in it.
    planes: Vec<Plane>,
    /// Whether proper triangles face both ways in each plane.
    folded: Vec<bool>,
    /// In each plane, the face of the largest proper triangle facing
    /// along its normal, and of the largest facing against it (one whose
    /// face is a plane facing the other way left out).
    faces: Vec<[Option<u32>; 2]>,
    /// Each triangle of the star and its plane, sorted.
    of: Vec<(u32, usize)>,
}

/// At most this many planes in a folded star: a crease of three faces.
const PLANES: usize = 3;

impl Cleaner<'_> {
    /// The star of `v` if it is folded: every triangle round it is on a
    /// plane face, has straight sides and lies within `small` of one of at
    /// most [`PLANES`] planes through `v` (each a proper triangle's, largest
    /// first), and in some plane proper triangles face both ways.
    pub(super) fn folded(&self, v: u32) -> Option<Star> {
        let around = &self.around[v as usize];
        if around
            .iter()
            .any(|&t| self.planes[self.soup.faces[t as usize] as usize].is_none())
        {
            return None;
        }
        let normals: Vec<(DVec3, u32)> = around
            .iter()
            .map(|&t| (self.normal(self.soup.tris[t as usize]), t))
            .collect();
        // Triangles facing both ways in one plane have normals pointing
        // apart: most vertices are turned away here, before their curves
        // are looked up.
        let apart = normals
            .iter()
            .enumerate()
            .any(|(i, a)| normals[i + 1..].iter().any(|b| a.0.dot(b.0) < 0.0));
        if !apart
            || around
                .iter()
                .any(|&t| !self.straight_sides(self.soup.tris[t as usize]))
        {
            return None;
        }
        let mut order: Vec<(f64, u32)> =
            normals.into_iter().map(|(n, t)| (n.length(), t)).collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let p = self.p(v);
        let mut star = Star {
            planes: Vec::new(),
            folded: Vec::new(),
            faces: Vec::new(),
            of: Vec::new(),
        };
        let mut ways: Vec<[bool; 2]> = Vec::new();
        for (_, t) in order {
            let tri = self.soup.tris[t as usize];
            let proper = self.height(tri).0 > self.small;
            let i = match star.planes.iter().position(|&pl| self.in_plane(t, pl)) {
                Some(i) => i,
                None if proper && star.planes.len() < PLANES => {
                    let n = self.normal(tri).normalize();
                    star.planes.push((n, n.dot(p)));
                    star.faces.push([None, None]);
                    ways.push([false; 2]);
                    star.planes.len() - 1
                }
                None => return None,
            };
            star.of.push((t, i));
            if proper {
                let n = star.planes[i].0;
                let way = self.normal(tri).dot(n) > 0.0;
                let k = usize::from(!way);
                ways[i][k] = true;
                let face = self.soup.faces[t as usize];
                let agrees =
                    self.planes[face as usize].is_some_and(|(m, _)| (m.dot(n) > 0.0) == way);
                if agrees && star.faces[i][k].is_none() {
                    star.faces[i][k] = Some(face);
                }
            }
        }
        star.folded = ways.iter().map(|w| w[0] && w[1]).collect();
        if !star.folded.contains(&true) {
            return None;
        }
        star.of.sort_unstable();
        Some(star)
    }

    /// The face triangle `t` of `star`, `old` before its vertex moved,
    /// goes on: its own, or where it turned over, or now faces against
    /// its face's plane, a face of the star in its plane facing its way.
    /// `None` where it turned over in a plane that isn't folded, no face
    /// of the star faces its way, or it ends further off its face's plane
    /// than it was (or than `small`, if more): the collapse isn't made. A
    /// triangle of zero height keeps its face for the rounds to take out.
    pub(super) fn retag(&self, star: &Star, t: u32, old: [u32; 3]) -> Option<u32> {
        let face = self.soup.faces[t as usize];
        let new = self.soup.tris[t as usize];
        let i = match star.of.binary_search_by_key(&t, |x| x.0) {
            Ok(k) => star.of[k].1,
            Err(_) => return Some(face),
        };
        let to = if self.height(new).0 <= self.small {
            face
        } else {
            let n = star.planes[i].0;
            let way = self.normal(new).dot(n) > 0.0;
            let over =
                self.height(old).0 > self.small && self.normal(old).dot(self.normal(new)) <= 0.0;
            if over && !star.folded[i] {
                return None;
            }
            let against = self.planes[face as usize].is_some_and(|(m, _)| (m.dot(n) > 0.0) != way);
            if over || against {
                star.faces[i][usize::from(!way)]?
            } else {
                face
            }
        };
        (self.off_face(new, to) <= self.off_face(old, face).max(self.small)).then_some(to)
    }

    /// How far the furthest corner of `tri` is off the plane of `face`
    /// (every face of a folded star is a plane).
    fn off_face(&self, tri: [u32; 3], face: u32) -> f64 {
        self.planes[face as usize].map_or(0.0, |(n, d)| {
            tri.iter()
                .map(|&v| (n.dot(self.p(v)) - d).abs())
                .fold(0.0, f64::max)
        })
    }

    /// Collapses each folded star's vertex (in id order) onto the
    /// neighbour in every plane of the star leaving the least unsigned
    /// area round it, if that is less than before by more than `small²`
    /// and the collapse can be made (each candidate in turn, by area and
    /// id); triangles that turned over go on a face of their way (see
    /// [`Self::retag`]). Whether any vertex moved.
    pub(super) fn unfold(&mut self) -> bool {
        let mut changed = false;
        for v in 0..self.soup.pos.len() as u32 {
            let Some(star) = self.folded(v) else {
                continue;
            };
            let area = |c: &Self, tri: [u32; 3]| c.normal(tri).length() / 2.0;
            let round: Vec<(u32, [u32; 3])> = self.around[v as usize]
                .iter()
                .map(|&t| (t, self.soup.tris[t as usize]))
                .collect();
            let before: f64 = round.iter().map(|&(_, tri)| area(self, tri)).sum();
            let mut candidates: Vec<(f64, u32)> = self
                .neighbours(v)
                .into_iter()
                .filter(|&w| {
                    star.planes
                        .iter()
                        .all(|&(n, d)| (n.dot(self.p(w)) - d).abs() <= self.small)
                })
                .map(|w| {
                    let after = round
                        .iter()
                        .filter(|(_, tri)| !tri.contains(&w))
                        .map(|&(_, tri)| area(self, tri.map(|x| if x == v { w } else { x })))
                        .sum::<f64>();
                    (after, w)
                })
                .collect();
            candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            for (after, w) in candidates {
                if (before - after).partial_cmp(&(self.small * self.small))
                    != Some(Ordering::Greater)
                {
                    break;
                }
                if self.collapse(w, v, Turn::Fold(&star)) {
                    for &(t, old) in &round {
                        if !self.alive[t as usize] {
                            continue;
                        }
                        if let Some(face) = self.retag(&star, t, old)
                            && face != self.soup.faces[t as usize]
                        {
                            self.soup.faces[t as usize] = face;
                            self.soup.made[t as usize] = true;
                        }
                    }
                    changed = true;
                    break;
                }
            }
        }
        changed
    }
}
