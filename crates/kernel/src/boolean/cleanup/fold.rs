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
    /// of the star faces its way, it ends further off its face's plane
    /// than it was (or than `small`, if more), or it doesn't face along
    /// that plane's normal (a sliver no higher than about twice `small`
    /// can lie within `small` of the star's plane steeply, facing its
    /// way by the star's plane but not by its face's, a hair off): the
    /// collapse isn't made. A triangle of zero height keeps its face for
    /// the rounds to take out.
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
        let along = self.height(new).0 <= self.small
            || self.planes[to as usize].is_none_or(|(m, _)| self.normal(new).dot(m) > 0.0);
        (along && self.off_face(new, to) <= self.off_face(old, face).max(self.small)).then_some(to)
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

#[cfg(test)]
mod tests {
    use super::super::Soup;
    use super::*;
    use crate::boolean::assemble::Curves;

    // A unit box whose top, at z = 1, is a fan round `V` (at x = 1.3,
    // outside the box) over the pentagon D A B E C (E halfway along the
    // top edge of the wall at x = 1): the two triangles round `V` over
    // B E C face down, a folded sheet, and lie on face 1, a plane facing
    // down (the sheet's other side, from the other operand); the rest
    // of the top is face 0, facing up. Ids put D first, so the collapse
    // onto D (which turns the two over: every candidate leaves the
    // pentagon's area, and ties go by id) is tried first.
    const D: u32 = 0;
    const A: u32 = 1;
    const B: u32 = 2;
    const E: u32 = 3;
    const C: u32 = 4;
    const V: u32 = 5;
    const SMALL: f64 = 1e-9;

    fn folded_box() -> Soup {
        let p = DVec3::new;
        let pos = vec![
            p(0.0, 1.0, 1.0),
            p(0.0, 0.0, 1.0),
            p(1.0, 0.0, 1.0),
            p(1.0, 0.5, 1.0),
            p(1.0, 1.0, 1.0),
            p(1.3, 0.5, 1.0),
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(1.0, 1.0, 0.0),
            p(0.0, 1.0, 0.0),
        ];
        let (a, b, c, d) = (6, 7, 8, 9);
        let tris: Vec<([u32; 3], u32)> = vec![
            ([V, A, B], 0),
            ([V, B, E], 1),
            ([V, E, C], 1),
            ([V, C, D], 0),
            ([V, D, A], 0),
            ([b, c, E], 2),
            ([c, C, E], 2),
            ([b, E, B], 2),
            ([a, b, B], 3),
            ([a, B, A], 3),
            ([c, d, D], 4),
            ([c, D, C], 4),
            ([a, A, D], 5),
            ([a, D, d], 5),
            ([a, c, b], 6),
            ([a, d, c], 6),
        ];
        Soup {
            pos,
            faces: tris.iter().map(|t| t.1).collect(),
            made: vec![true; tris.len()],
            tris: tris.into_iter().map(|t| t.0).collect(),
            curves: Curves::new(),
            sources: (0..7).collect(),
            absorbed: Vec::new(),
        }
    }

    /// The planes of [`folded_box`]'s faces: the top's two, the walls'
    /// and the bottom's.
    fn planes() -> Vec<Option<Plane>> {
        let z = DVec3::Z;
        [
            (z, 1.0),
            (-z, -1.0),
            (DVec3::X, 1.0),
            (-DVec3::Y, 0.0),
            (DVec3::Y, 1.0),
            (-DVec3::X, 0.0),
            (-z, 0.0),
        ]
        .into_iter()
        .map(Some)
        .collect()
    }

    fn cleaner(soup: &mut Soup, planes: Vec<Option<Plane>>) -> Cleaner<'_> {
        let mut around = vec![Vec::new(); soup.pos.len()];
        for (t, tri) in soup.tris.iter().enumerate() {
            for &v in tri {
                around[v as usize].push(t as u32);
            }
        }
        Cleaner {
            alive: vec![true; soup.tris.len()],
            planar: planes.iter().map(Option::is_some).collect(),
            soup,
            around,
            recurved: Vec::new(),
            planes,
            joined: Vec::new(),
            small: SMALL,
            thin: SMALL,
        }
    }

    /// The living triangles on `face`, their corners sorted.
    fn on_face(c: &Cleaner, face: u32) -> Vec<[u32; 3]> {
        let mut out: Vec<[u32; 3]> = (0..c.soup.tris.len())
            .filter(|&t| c.alive[t] && c.soup.faces[t] == face)
            .map(|t| {
                let mut tri = c.soup.tris[t];
                tri.sort_unstable();
                tri
            })
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn a_folded_top_is_unfolded_and_turned_triangles_change_face() {
        let mut soup = folded_box();
        let mut c = cleaner(&mut soup, planes());
        assert!(c.folded(V).is_some());
        assert!(c.unfold());
        // Onto D: the two triangles that faced down turned up, onto the
        // top's face, which now has the pentagon, facing up.
        assert_eq!(on_face(&c, 0), [[D, A, B], [D, B, E], [D, E, C]]);
        assert!(on_face(&c, 1).is_empty());
        for t in 0..c.soup.tris.len() {
            if c.alive[t] && c.soup.faces[t] == 0 {
                assert!(c.normal(c.soup.tris[t]).z > 0.0);
            }
        }
        assert!(c.folded(D).is_none());
    }

    #[test]
    fn a_star_with_a_triangle_off_every_plane_face_is_left() {
        // The face facing down claims no plane (as a curved face's
        // triangle, straight-sided and thin, can lie in the star's
        // plane): moving along the plane could take it off its surface.
        let mut soup = folded_box();
        let mut planes = planes();
        planes[1] = None;
        let mut c = cleaner(&mut soup, planes);
        assert!(c.folded(V).is_none());
        assert!(!c.unfold());
        assert_eq!(on_face(&c, 1).len(), 2);
    }

    #[test]
    fn a_steep_sliver_never_stays_on_a_face_it_faces_against() {
        // A sliver 1.4 short lengths high, standing steeply within that
        // of the star's plane (z = 1), faces up by it, as the top's face
        // does, but against the top's face once that is tilted 0.2 rad
        // about x: it can't stay on it. Untilted, it stays.
        let p1 = DVec3::new(0.2, 0.2, 1.0);
        let (l, s) = (0.1, SMALL);
        let sliver = [10, 11, 12];
        for tilt in [-0.2f64, 0.0] {
            let (sin, cos) = crate::trig::sin_cos(tilt);
            let m = DVec3::new(0.0, sin, cos);
            let mut tilted = planes();
            tilted[0] = Some((m, m.dot(p1)));
            let mut soup = folded_box();
            soup.pos.extend([
                p1,
                p1 + DVec3::new(l, 0.0, 0.0),
                p1 + DVec3::new(l / 2.0, 0.1 * s, -1.4 * s),
            ]);
            let c = cleaner(&mut soup, tilted);
            let star = c.folded(V).expect("folded");
            assert!(c.height(sliver).0 > SMALL);
            assert!(c.normal(sliver).z > 0.0);
            assert_eq!(c.normal(sliver).dot(m) > 0.0, tilt == 0.0);
            // Triangle 0 ([V, A, B], on the top's face) as it.
            c.soup.tris[0] = sliver;
            let want = (tilt == 0.0).then_some(0);
            assert_eq!(c.retag(&star, 0, sliver), want, "tilted {tilt}");
        }
    }

    #[test]
    fn a_turned_triangle_never_goes_onto_a_face_a_hair_off() {
        // The top's face tilted about the edge D A by 10 times the short
        // length over the box: the triangles turned onto it by the
        // collapse onto D (or onto A) would end further off it than they
        // were off theirs, so the vertex goes onto B, where nothing turns
        // and the triangles kept on the top come no further off it.
        let s = 10.0 * SMALL;
        let n = DVec3::new(-s, 0.0, 1.0);
        let len = n.length();
        let mut planes = planes();
        planes[0] = Some((n / len, 1.0 / len));
        let mut soup = folded_box();
        let mut c = cleaner(&mut soup, planes);
        let before: f64 = (0..c.soup.tris.len() as u32)
            .filter(|&t| c.soup.faces[t as usize] == 0)
            .map(|t| c.off_face(c.soup.tris[t as usize], 0))
            .fold(0.0, f64::max);
        assert!(c.unfold());
        assert_eq!(on_face(&c, 0), [[D, A, B], [D, B, C]]);
        for t in 0..c.soup.tris.len() as u32 {
            if c.alive[t as usize] && c.soup.faces[t as usize] == 0 {
                assert!(c.off_face(c.soup.tris[t as usize], 0) <= before);
            }
        }
    }
}
