//! One surface, one face: adjacent faces on the same plane or quadric
//! named as one.
//!
//! A boolean carries each operand's faces over, so where two operands
//! meet flush (stacked boxes, an L of two boxes, coaxial cylinders
//! stacked) one plane or cylinder stays two faces, and the tessellation
//! draws a line where they meet. An extrude makes a face per profile
//! segment, so two collinear lines are two faces of one plane, and a
//! circle's arcs four of one cylinder (one key already). [`Mesh::merge_faces`]
//! joins them: faces are one where they share an edge and each one's
//! patches there lie on the other's surface within `small`, an eighth of
//! the resolution.
//!
//! Only names change. Each set takes the name of its lowest face (a
//! member already of its key, such as another arc of the same circle,
//! keeps its own, which numbers the piece), and every member gets the
//! set's aliases: the other members' keys and their aliases, so
//! references to them still resolve. The members stay separate entries
//! of [`Mesh::faces`], each with its own surface and form: an arc's wall
//! is written in the arc's own coordinates, best conditioned near it,
//! and the exact paths of later booleans take it from there, so no tag
//! changes and nothing a boolean decides by tags or face indices does.
//! Faces are drawn, picked and referred to by key, so a set is one face
//! for all of those; several entries can carry one name, so faces are
//! counted by key or region, never by entry. A set merges only if every
//! patch of every member lies on the lowest one's surface within
//! `small`, so a chain of near-equal surfaces can't drift.

use glam::DVec3;

use std::collections::BTreeMap;

use super::check::on_surface;
use super::{FaceKey, FaceName, Mesh, Surface};
use crate::KernelError;
use crate::budget::Work;
use crate::par::par_map;

/// How close two planes' unit normals must be to be the same plane's,
/// as a dot product (the offsets are then measured on the patches).
const SAME_NORMAL: f64 = 1.0 - 1e-12;

impl Mesh {
    /// The mesh with adjacent faces on one surface named as one (see the
    /// [module](self) docs), `small` being `resolution / 8`. Faces
    /// claiming no surface never merge on geometry: a copy claiming none
    /// of a face that merged (the copies a boolean makes) takes the set's
    /// name and aliases too. Charged a unit a patch.
    ///
    /// Only names and aliases change: a checked mesh stays one.
    pub(crate) fn merge_faces(self, resolution: f64, work: &mut Work) -> Result<Mesh, KernelError> {
        work.spend(self.tris.len())?;
        let small = resolution / 8.0;
        let n = self.faces.len();
        let mut root: Vec<u32> = (0..n as u32).collect();
        fn find(root: &mut [u32], mut f: u32) -> u32 {
            while root[f as usize] != f {
                root[f as usize] = root[root[f as usize] as usize];
                f = root[f as usize];
            }
            f
        }
        let unit = self
            .faces
            .iter()
            .map(|face| match face.surface {
                Surface::Plane { n, .. } => n.try_normalize(),
                _ => None,
            })
            .collect::<Vec<_>>();
        // Candidates, edge by edge in halfedge order.
        for h in 0..(self.tris.len() * 3) as u32 {
            let p = self.halfedge(h).pair;
            if p < h {
                continue;
            }
            let (t, s) = (h / 3, p / 3);
            let (f, g) = (self.tris[t as usize].face, self.tris[s as usize].face);
            let (rf, rg) = (find(&mut root, f), find(&mut root, g));
            if rf == rg || !self.same_surface(h, p, &unit, small) {
                continue;
            }
            root[rf.max(rg) as usize] = rf.min(rg);
        }
        let root: Vec<u32> = (0..n as u32).map(|f| find(&mut root, f)).collect();
        if root.iter().enumerate().all(|(f, &r)| f as u32 == r) {
            return Ok(self);
        }
        // Each set validated once: every patch of every other member on
        // the root's surface.
        let moved: Vec<u32> = (0..self.tris.len() as u32)
            .filter(|&t| {
                let f = self.tris[t as usize].face;
                root[f as usize] != f
            })
            .collect();
        let on = par_map(&moved, |&t| {
            let r = root[self.tris[t as usize].face as usize];
            on_surface(
                &self.patch(t as usize),
                &self.faces[r as usize].surface,
                small,
            )
        });
        let mut holds = vec![true; n];
        for (&t, on) in moved.iter().zip(on) {
            if !on {
                holds[root[self.tris[t as usize].face as usize] as usize] = false;
            }
        }
        let into: Vec<u32> = (0..n)
            .map(|f| {
                let r = root[f];
                if holds[r as usize] { r } else { f as u32 }
            })
            .collect();
        if into.iter().enumerate().all(|(f, &r)| f as u32 == r) {
            return Ok(self);
        }
        Ok(self.rename(&into))
    }

    /// Whether the faces either side of halfedge `h` (its pair `p`) are
    /// on one surface there: both planes, their unit normals (`unit`)
    /// alike and each triangle on the other's plane, or both quadrics,
    /// the two patches facing alike at the edge's middle and each on the
    /// other's quadric; within `small`.
    fn same_surface(&self, h: u32, p: u32, unit: &[Option<DVec3>], small: f64) -> bool {
        let (t, s) = (h as usize / 3, p as usize / 3);
        let (f, g) = (self.tris[t].face as usize, self.tris[s].face as usize);
        let (sf, sg) = (self.faces[f].surface, self.faces[g].surface);
        let (pt, ps) = (self.patch(t), self.patch(s));
        let crossed = || on_surface(&pt, &sg, small) && on_surface(&ps, &sf, small);
        match (sf, sg) {
            (Surface::Plane { .. }, Surface::Plane { .. }) => {
                let (Some(nf), Some(ng)) = (unit[f], unit[g]) else {
                    return false;
                };
                nf.dot(ng) > SAME_NORMAL && crossed()
            }
            (Surface::Quadric(_), Surface::Quadric(_)) => {
                // The middle of edge `h % 3` of `t`, and of `p % 3` of `s`.
                let middle = |i: usize| {
                    let mut u = DVec3::ZERO;
                    u[i] = 0.5;
                    u[(i + 1) % 3] = 0.5;
                    u
                };
                let nt = pt.normal(middle(h as usize % 3));
                let ns = ps.normal(middle(p as usize % 3));
                nt.dot(ns) > 0.0 && crossed()
            }
            _ => false,
        }
    }

    /// The mesh with each face `f` named as face `into[f]` (`into` maps
    /// each face to itself or to a lower face that maps to itself), and
    /// copies claiming no surface of a face so renamed too, every face of
    /// a set with the set's aliases.
    fn rename(mut self, into: &[u32]) -> Mesh {
        let n = self.faces.len();
        let mut own: Vec<Vec<FaceKey>> = vec![Vec::new(); n];
        for &(f, key) in &self.aliases {
            own[f as usize].push(key);
        }
        // Each set's aliases, at its root: the members' keys and aliases.
        let mut taken: Vec<Vec<FaceKey>> = vec![Vec::new(); n];
        for f in 0..n {
            let r = into[f] as usize;
            if r != f {
                taken[r].push(self.faces[f].name.key());
                taken[r].extend_from_slice(&own[f]);
            }
        }
        for r in 0..n {
            if !taken[r].is_empty() {
                let root = std::mem::take(&mut own[r]);
                taken[r].extend(root);
            }
        }
        // The set of each name of a member, the lowest set's for a name
        // two have.
        let mut sets: BTreeMap<FaceName, u32> = BTreeMap::new();
        for f in 0..n {
            let r = into[f] as usize;
            if r != f || !taken[r].is_empty() {
                sets.entry(self.faces[f].name).or_insert(r as u32);
            }
        }
        // The members, and copies claiming no surface of one.
        let set: Vec<Option<u32>> = (0..n)
            .map(|f| {
                let r = into[f] as usize;
                if r != f || !taken[r].is_empty() {
                    return Some(r as u32);
                }
                let free = matches!(self.faces[f].surface, Surface::Free);
                free.then(|| sets.get(&self.faces[f].name).copied())
                    .flatten()
            })
            .collect();
        let mut aliases = std::mem::take(&mut self.aliases);
        for (f, r) in set.into_iter().enumerate() {
            let Some(r) = r else {
                continue;
            };
            // A piece already of the root's key keeps its own name.
            let name = self.faces[r as usize].name;
            if self.faces[f].name.key() != name.key() {
                self.faces[f].name = name;
            }
            aliases.extend(taken[r as usize].iter().map(|&key| (f as u32, key)));
        }
        self.with_aliases(aliases)
    }
}

#[cfg(test)]
mod tests;
