//! Finding again what a reference names in the model shown: faces by
//! their keys, edges by the faces either side, vertices by the faces
//! meeting there, the nearest of those named alike.

use glam::{DVec3, Vec3};
use varde_document::{BodyId, EdgeRef, FaceRef};
use varde_kernel_compat::mesh::FaceKey;
use varde_regen::{Summary, vertex_runs};

use super::{PickIndex, corner_position, position, segment};
use crate::hit::segment_distance_3d;

/// How far `p` is from the triangle `[a, b, c]`: from the plane it's
/// in, where `p` is over it, otherwise from its nearest side.
fn triangle_distance(p: DVec3, [a, b, c]: [DVec3; 3]) -> f64 {
    let normal = (b - a).cross(c - a);
    let area = normal.length_squared();
    if area > 0.0 {
        // `p` over the triangle: each side has it on the inside.
        let inside = [(a, b), (b, c), (c, a)]
            .iter()
            .all(|&(u, v)| (v - u).cross(p - u).dot(normal) >= 0.0);
        if inside {
            return (p - a).dot(normal).abs() / area.sqrt();
        }
    }
    [(a, b), (b, c), (c, a)]
        .iter()
        .map(|&(u, v)| segment_distance_3d(p, u, v))
        .fold(f64::INFINITY, f64::min)
}

impl PickIndex {
    /// The reference to face `face` picked at `near`, as a sketch plane
    /// stores it, if there's such a face.
    pub fn face_ref(&self, face: u32, near: DVec3) -> Option<FaceRef> {
        Some(FaceRef {
            body: self.face_body(face)?,
            key: self.face_key(face)?,
            near,
        })
    }

    /// The faces of the body `face` is on keyed as it is, ascending, it
    /// among them: the pieces a face named once is left in.
    pub fn faces_keyed_as(&self, face: u32) -> Vec<u32> {
        let (Some(body), Some(key)) = (self.face_body(face), self.face_key(face)) else {
            return Vec::new();
        };
        (self.body_faces(body))
            .filter(|&other| self.face_key(other) == Some(key))
            .collect()
    }

    /// The key that names mesh face `face`, if there's such a face.
    pub fn face_key(&self, face: u32) -> Option<FaceKey> {
        Some(self.picking.faces().get(face as usize)?.key)
    }

    /// The keys of the faces either side of `chain`, sorted, as an edge
    /// reference keeps them, if it's an edge between two faces.
    pub fn chain_keys(&self, chain: u32) -> Option<[FaceKey; 2]> {
        let faces = self.edge_faces(chain)?;
        let [a, b] = faces.map(|face| self.face_key(face));
        let (a, b) = (a?, b?);
        Some([a.min(b), a.max(b)])
    }

    /// The keys that name `corner`, a vertex: the lowest three of those of
    /// the faces meeting there, sorted, if three faces or more do.
    pub fn vertex_keys(&self, corner: u32) -> Option<[FaceKey; 3]> {
        let mut keys: Vec<FaceKey> = (self.corner_faces(corner).iter())
            .filter_map(|&(_, face)| self.face_key(face))
            .collect();
        keys.sort_unstable();
        keys.dedup();
        match keys[..] {
            [a, b, c, ..] => Some([a, b, c]),
            _ => None,
        }
    }

    /// Where `corner` is, if the mesh has it.
    pub fn corner_point(&self, corner: u32) -> Option<DVec3> {
        Some(corner_position(&self.mesh, corner)?.as_dvec3())
    }

    /// A point on `chain`: the middle of its first segment.
    pub fn chain_point(&self, chain: u32) -> Option<DVec3> {
        let start = self.mesh.polyline_range(chain as usize)?.start;
        let [a, b] = segment(&self.mesh, u32::try_from(start).ok()?)?;
        Some(((a + b) / 2.0).as_dvec3())
    }

    /// The ends of `chain`, if it's a straight edge between two faces, in
    /// the order an edge reference with the keys `faces` runs: with the
    /// face its first key names on its left seen from outside, as
    /// regenerating directs it ([`EdgeRef::runs_with`]); the mesh's edge
    /// runs with its first face on its left. None if the keys don't tell
    /// which face is which. Straight as the mesh draws it: not closed, its
    /// snap point (a straight edge's middle, a round one's centre) at
    /// the middle of its ends, and every point of its polyline on the
    /// line through them, within the rounding of the mesh's `f32`
    /// points and a millionth of its length. Regenerating decides from
    /// the exact curves.
    pub fn edge_ends(&self, chain: u32, faces: &[FaceKey; 2]) -> Option<[DVec3; 2]> {
        let [left, right] = self.edge_faces(chain)?.map(|face| {
            let face = self.picking.faces().get(face as usize)?;
            Some((&face.key, &face.aliases[..]))
        });
        if *self.picking.closed().get(chain as usize)? {
            return None;
        }
        let snap = DVec3::from((*self.picking.snaps().get(chain as usize)?)?);
        let points: Vec<DVec3> = (self.mesh.polyline(chain as usize)?.iter())
            .map(|&i| position(&self.mesh, i).as_dvec3())
            .collect();
        let (&from, &to) = (points.first()?, points.last()?);
        let chord = to - from;
        let along = chord.try_normalize()?;
        let scale = (points.iter())
            .map(|p| p.abs().max_element())
            .fold(snap.abs().max_element(), f64::max);
        let slack = 1e-6 * chord.length() + 8.0 * f64::from(f32::EPSILON) * scale;
        let off = |p: DVec3| {
            let d = p - from;
            (d - along * d.dot(along)).length()
        };
        if snap.distance((from + to) / 2.0) > slack || points.iter().any(|&p| off(p) > slack) {
            return None;
        }
        Some(if EdgeRef::runs_with(faces, left?, right?)? {
            [from, to]
        } else {
            [to, from]
        })
    }

    /// The face of `body` that `key` names (its key or an alias), the
    /// nearest to `near` among several, as the kernel resolves a face
    /// reference: one is taken wherever `near` is; of several, a later
    /// one only where it comes nearer by more than a billionth of the
    /// model's size, so ties go to the lowest. Measured to the mesh as
    /// drawn, which is all the view has.
    pub fn find_face(&self, body: BodyId, key: &FaceKey, near: DVec3) -> Option<u32> {
        let found = (self.body_faces(body)).filter(|&face| self.named(face, key));
        self.nearest(found, near, |face| self.face_distance(face, near))
    }

    /// How far `point` is from face `face`'s triangles in the mesh:
    /// infinite if it has none.
    pub fn face_distance(&self, face: u32, point: DVec3) -> f64 {
        (self.face_triangles(face))
            .filter_map(|triangle| self.triangle_points(triangle))
            .map(|points| triangle_distance(point, points))
            .fold(f64::INFINITY, f64::min)
    }

    /// Where face `face` is nearest `near` as drawn, and its outward
    /// normal there (unit): `near` taken onto the plane of the face's
    /// triangle nearest it, the normal that triangle's corners' (the
    /// mesh's analytic ones), or its own where those cancel. A flat
    /// face's is its plane's, exactly as its summary has it, not as the
    /// mesh's single precision draws it (far out, that's micrometres
    /// off). `None` for no such face, or one with no triangle that isn't
    /// degenerate.
    pub fn face_point(&self, face: u32, near: DVec3) -> Option<(DVec3, DVec3)> {
        if let Some(Summary::Plane { n, d }) =
            (self.picking.faces().get(face as usize)).map(|face| &face.summary)
            && let Some(n) = DVec3::from(*n).try_normalize()
            && self.face_triangles(face).next().is_some()
        {
            let point = near - n * (n.dot(near) - d);
            return point.is_finite().then_some((point, n));
        }
        let mut best: Option<(f64, [u32; 3], [DVec3; 3])> = None;
        for triangle in self.face_triangles(face) {
            let Some(corners) = self.corners(triangle) else {
                continue;
            };
            let points = self.points(corners);
            let distance = triangle_distance(near, points);
            if distance.is_finite() && best.is_none_or(|(nearest, ..)| distance < nearest) {
                best = Some((distance, corners, points));
            }
        }
        let (_, corners, [a, b, c]) = best?;
        let own = (b - a).cross(c - a).try_normalize()?;
        let normals = self.mesh.normals();
        let summed = (corners.iter())
            .filter_map(|&i| normals.get(i as usize))
            .map(|&n| Vec3::from(n).as_dvec3())
            .fold(DVec3::ZERO, |sum, n| sum + n);
        let normal = summed
            .try_normalize()
            .filter(|n| n.dot(own) > 0.0)
            .unwrap_or(own);
        let point = near - own * (near - a).dot(own);
        point.is_finite().then_some((point, normal))
    }

    /// The edge of `body` between faces that `faces` name (either way
    /// round, by key or alias), the nearest to `near` among several, as
    /// [`PickIndex::find_face`] finds faces.
    pub fn find_edge(&self, body: BodyId, faces: [FaceKey; 2], near: DVec3) -> Option<u32> {
        let named =
            |face: u32, key: &FaceKey| self.face_body(face) == Some(body) && self.named(face, key);
        let found = (0..self.mesh.edge_count())
            .filter_map(|edge| u32::try_from(edge).ok())
            .filter(|&edge| {
                self.edge_faces(edge).is_some_and(|[a, b]| {
                    (named(a, &faces[0]) && named(b, &faces[1]))
                        || (named(a, &faces[1]) && named(b, &faces[0]))
                })
            });
        self.nearest(found, near, |chain| self.edge_distance(chain, near))
    }

    /// How far `point` is from the edge chain `chain`'s polyline in the
    /// mesh: infinite if it has none.
    pub fn edge_distance(&self, chain: u32, point: DVec3) -> f64 {
        (self.edge_segments(chain))
            .filter_map(|start| segment(&self.mesh, start))
            .map(|[a, b]| segment_distance_3d(point, a.as_dvec3(), b.as_dvec3()))
            .fold(f64::INFINITY, f64::min)
    }

    /// The vertex of `body` where faces that `keys` name meet (each by key
    /// or alias, among the faces there), the nearest to `near` among
    /// several, as [`PickIndex::find_face`] finds faces.
    pub fn find_vertex(&self, body: BodyId, keys: [FaceKey; 3], near: DVec3) -> Option<u32> {
        let found = vertex_runs(self.tables.corner_faces())
            .filter(|run| self.face_body(run[0].1) == Some(body))
            .filter(|run| {
                (keys.iter()).all(|key| run.iter().any(|&(_, face)| self.named(face, key)))
            })
            .map(|run| run[0].0);
        self.nearest(found, near, |corner| {
            self.corner_point(corner)
                .map_or(f64::INFINITY, |at| at.distance(near))
        })
    }

    /// Whether `key` names face `face`, as its key or an alias.
    fn named(&self, face: u32, key: &FaceKey) -> bool {
        (self.picking.faces().get(face as usize)).is_some_and(|face| face.named(key))
    }

    /// Of `found`, ascending, the only one, or the one at the least
    /// `distance` from `near`, a later one counting only where it comes
    /// nearer by more than a billionth of the mesh's size; the first where
    /// `near` isn't finite.
    fn nearest(
        &self,
        found: impl Iterator<Item = u32>,
        near: DVec3,
        distance: impl Fn(u32) -> f64,
    ) -> Option<u32> {
        let found: Vec<u32> = found.collect();
        if found.len() <= 1 || !near.is_finite() {
            return found.first().copied();
        }
        let size = self
            .bounds
            .map_or(0.0, |bounds| f64::from((bounds.max - bounds.min).length()));
        let slack = 1e-9 * size;
        let mut best: Option<(f64, u32)> = None;
        for item in found {
            let d = distance(item);
            if best.is_none_or(|(b, _)| d < b - slack) {
                best = Some((d, item));
            }
        }
        best.map(|(_, item)| item)
    }
}
