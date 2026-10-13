//! Picking the model shown in the viewport: which face, edge, vertex and
//! body is under the cursor, on the CPU, against the mesh the viewport
//! draws and its picking tables ([`Picking`]); no GPU picking. A
//! [`PickIndex`] holds a bounding volume hierarchy over the mesh's
//! triangles, one over the segments of its edges between two faces (its
//! creases aren't picked) and one over its vertices, the corners where
//! three faces or more meet. The cursor's ray meets the nearest triangle
//! it sees the front of, whose face is picked; but an edge within
//! [`EDGE_REACH`] pixels of the cursor on screen, not hidden by what's in
//! front of it (a ray from the eye to its point nearest the cursor meets
//! nothing nearer by more than [`HIDDEN_PULL`] view heights), wins over
//! the face, and a vertex within [`VERTEX_REACH`] that isn't hidden over
//! both. A body is the body of the face, edge or vertex picked.
//!
//! The index also builds the [`ModelHighlight`] of what's hovered and
//! selected, by the mesh's faces, edges and corners.

use std::cmp::Ordering;
use std::ops::Range;
use std::sync::Arc;

use glam::{DVec2, DVec3, Vec3};
use varde_document::{BodyId, Placement};
use varde_kernel_compat::RenderMesh;
use varde_regen::{PickTables, Picking, Summary};
use varde_render::Camera;

use crate::hit::segment_nearest;
use crate::projection::Projector;

mod highlight;
mod resolve;
mod walk;

pub use highlight::ModelHighlight;
use walk::{Ray, Walk};
pub(crate) use walk::{aabb, ray_hits, through_box};

/// How near the cursor an edge must show to be picked, in pixels.
pub const EDGE_REACH: f64 = 6.0;

/// How near the cursor a vertex must show to be picked, in pixels: a
/// vertex within it wins over the edges it ends.
pub const VERTEX_REACH: f64 = 6.0;

/// How near the cursor a snap point of what it's over must show for the
/// cursor to take it, in pixels: as sketch snapping's reach.
pub const SNAP_REACH: f64 = 10.0;

/// How much nearer the eye than an edge's point, in view heights, what's
/// in front of it must be to hide it: as far as the renderer pulls the
/// edges towards the camera, so an edge the faces either side of it
/// would hide but for that pull isn't hidden here either.
const HIDDEN_PULL: f64 = 0.002;

/// The most searches of the mesh for what hides an edge or a vertex near
/// the cursor, looking for one that isn't hidden, nearest first: past
/// them the rest are as good as hidden. Those that a triangle found
/// hiding one before hides as well cost no search ([`MAX_OCCLUDERS`]),
/// so many edges hidden behind the same faces near the cursor don't use
/// them up.
const MAX_HIDDEN_SEARCHES: usize = 1024;

/// The most triangles found hiding an edge or a vertex near the cursor
/// that are tried first on the next: the latest ones.
const MAX_OCCLUDERS: usize = 16;

/// Within how many pixels of each other two edges or vertices count as
/// equally near the cursor, the nearer the eye going first: a corner seen
/// straight down an edge shows where the one behind it does, which the
/// hidden test can take for seen, its ray running along the faces there.
const SAME_PLACE: f64 = 0.5;

/// What the cursor picks in the model: faces, edges and vertices (a
/// vertex near the cursor winning over an edge, and an edge over a face),
/// only faces or only edges, or edges and faces (an edge winning), as a
/// move's axis is picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Picks {
    #[default]
    All,
    Faces,
    Edges,
    EdgesAndFaces,
}

impl Picks {
    /// Whether the cursor picks `target` in it.
    pub fn takes(self, target: Picked) -> bool {
        match target {
            Picked::Face(_) => self.faces(),
            Picked::Edge(_) => self.edges(),
            Picked::Vertex(_) => self.vertices(),
        }
    }

    /// Whether the cursor picks vertices in it.
    pub fn vertices(self) -> bool {
        match self {
            Picks::All => true,
            Picks::Faces | Picks::Edges | Picks::EdgesAndFaces => false,
        }
    }

    /// Whether the cursor picks edges in it.
    pub fn edges(self) -> bool {
        match self {
            Picks::All | Picks::Edges | Picks::EdgesAndFaces => true,
            Picks::Faces => false,
        }
    }

    /// Whether the cursor picks faces in it.
    pub fn faces(self) -> bool {
        match self {
            Picks::All | Picks::Faces | Picks::EdgesAndFaces => true,
            Picks::Edges => false,
        }
    }
}

/// A face, an edge or a vertex of the model shown: a face or edge of its
/// mesh, so an index into its picking tables' [`Picking::faces`], or its
/// [`Picking::closed`] and [`Picking::tangents`], or a corner of its mesh
/// ([`RenderMesh::corners`]). Only edges between two faces are picked,
/// not creases, and only corners where three faces or more meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Picked {
    Face(u32),
    Edge(u32),
    Vertex(u32),
}

/// A point of the model shown that the cursor snaps to (the measure
/// tool's points): a corner, an index into [`Picking::corners`], or an
/// edge's point ([`Picking::snaps`]: a straight edge's middle, a round
/// edge's centre), by the edge's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Snapped {
    Corner(u32),
    EdgePoint(u32),
}

/// What's under the cursor in the model shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pick {
    /// Which model's tables `target` is an index into: the
    /// [`PickIndex::model`] of the index that picked it.
    pub model: u64,
    pub target: Picked,
    /// The body of the face, edge or vertex.
    pub body: BodyId,
    /// Where on it: where the cursor's ray meets the face, the edge's
    /// point that shows nearest the cursor, or the vertex. In world
    /// coordinates, as the mesh is drawn.
    pub at: DVec3,
    /// The snap point of `target` the cursor takes, where it's asked for
    /// and one shows within [`SNAP_REACH`] (see [`PickIndex::snap`]).
    pub snap: Option<Snapped>,
}

/// The model shown, made ready for picking: its mesh, its tables, and
/// hierarchies over the mesh's triangles, the segments of its edges
/// between two faces and its vertices.
#[derive(Debug)]
pub struct PickIndex {
    mesh: Arc<RenderMesh>,
    picking: Arc<Picking>,
    model: u64,
    /// Its hierarchies and groups, built with the model in the lane.
    tables: Arc<PickTables>,
    /// The mesh's bounds, kept: working them out walks every position,
    /// and each point tested for being hidden needs them.
    bounds: Option<varde_kernel_compat::Aabb>,
}

/// An index of no model, for tests that pick nothing in the model.
#[cfg(test)]
pub(crate) fn empty_index() -> &'static PickIndex {
    static EMPTY: std::sync::LazyLock<PickIndex> =
        std::sync::LazyLock::new(|| PickIndex::new(Arc::default(), Arc::default(), 0));
    &EMPTY
}

impl PickIndex {
    /// The index of `mesh` and its tables `picking`, which came with it;
    /// its picks carry `model`. Tables that don't go with the mesh pick
    /// nothing. Builds the [`PickTables`], which the lane does with the
    /// model: see [`PickIndex::with_tables`].
    pub fn new(mesh: Arc<RenderMesh>, picking: Arc<Picking>, model: u64) -> Self {
        let tables = Arc::new(PickTables::new(&mesh, &picking));
        Self::with_tables(mesh, picking, tables, model)
    }

    /// The index of `mesh`, its tables `picking` and `tables`, which came
    /// with it, as [`PickIndex::new`].
    pub fn with_tables(
        mesh: Arc<RenderMesh>,
        picking: Arc<Picking>,
        tables: Arc<PickTables>,
        model: u64,
    ) -> Self {
        Self {
            bounds: mesh.bounds(),
            mesh,
            picking,
            model,
            tables,
        }
    }

    /// Which model its picks name: see [`Pick::model`].
    pub fn model(&self) -> u64 {
        self.model
    }

    pub fn mesh(&self) -> &Arc<RenderMesh> {
        &self.mesh
    }

    pub fn picking(&self) -> &Arc<Picking> {
        &self.picking
    }

    /// The body of `target`, if it's in the tables.
    pub fn body(&self, target: Picked) -> Option<BodyId> {
        let face = match target {
            Picked::Face(face) => face,
            Picked::Edge(edge) => self.edge_faces(edge)?[0],
            Picked::Vertex(corner) => self.corner_faces(corner).first()?.1,
        };
        self.face_body(face)
    }

    /// The body of face `face`, if there's such a face.
    pub fn face_body(&self, face: u32) -> Option<BodyId> {
        self.picking.face_body(&self.mesh, face)
    }

    /// Where a sketch on face `face` is placed, if it's flat:
    /// [`Placement::on_plane`] of its plane, the bits regenerating places
    /// a sketch on it by. It may not be [`Placement::valid`] (a face too
    /// far out), which regenerating refuses.
    pub fn face_placement(&self, face: u32) -> Option<Placement> {
        match self.picking.faces().get(face as usize)?.summary {
            Summary::Plane { n, d } => Placement::on_plane(DVec3::from(n), d),
            _ => None,
        }
    }

    /// The faces either side of `edge`, if it's an edge between two
    /// faces, not a crease.
    pub fn edge_faces(&self, edge: u32) -> Option<[u32; 2]> {
        self.mesh.chain_faces(edge as usize)
    }

    /// What's under the screen position `at`, in logical pixels from the
    /// top left of a viewport `size` big seen by `camera`: a vertex
    /// showing within [`VERTEX_REACH`] of it that nothing hides, the
    /// nearest, or else such an edge within [`EDGE_REACH`], or else the
    /// face the cursor's ray first meets the front of; of those only what
    /// `picks` takes.
    pub fn pick(&self, camera: &Camera, size: [f32; 2], at: DVec2, picks: Picks) -> Option<Pick> {
        let projector = Projector::world(camera, size[0], size[1])?;
        let ray = self.ray(&projector, at)?;
        let vertex = || {
            picks
                .vertices()
                .then(|| self.vertex_near(&projector, &ray, at))
                .flatten()
                .map(|(corner, at)| (Picked::Vertex(corner), at))
        };
        let edge = || {
            picks
                .edges()
                .then(|| self.edge_near(&projector, &ray, at))
                .flatten()
                .map(|(chain, at)| (Picked::Edge(chain), at))
        };
        let face = || {
            let (t, triangle) = picks.faces().then(|| self.first_front(&ray)).flatten()?;
            Some((Picked::Face(self.triangle_face(triangle)?), ray.at(t)))
        };
        let (target, at) = vertex().or_else(edge).or_else(face)?;
        self.pick_of(target, at)
    }

    /// `target` picked at `at`, unsnapped, if it's of a body.
    fn pick_of(&self, target: Picked, at: DVec3) -> Option<Pick> {
        let body = self.body(target)?;
        Some(Pick {
            model: self.model,
            target,
            body,
            at,
            snap: None,
        })
    }

    /// Whether the model hides the world point `point` from `camera` in
    /// a viewport `size` big: something of it is in front of the point,
    /// nearer the eye by more than the edges are pulled towards it, as
    /// an edge is hidden from [`PickIndex::pick`]. So what lies on a face
    /// (a sketch on it) isn't hidden by it. A point behind the near
    /// plane of a perspective view, or no viewport, hides nothing.
    pub fn hides(&self, camera: &Camera, size: [f32; 2], point: DVec3) -> bool {
        let Some(projector) = Projector::world(camera, size[0], size[1]) else {
            return false;
        };
        let hidden = self.hidden(&projector, point, &mut Vec::new());
        hidden != Hidden::No
    }

    /// Everything at the screen position `at`, as [`PickIndex::pick`]
    /// takes it, hidden or not, that `picks` takes: the vertices, edges
    /// and faces showing within `pixels` of it, either side of them;
    /// vertices, then edges, each nearest the eye first, then faces, those
    /// the cursor's ray meets first, nearest the eye first, then the rest
    /// nearest the cursor first; each once, at most `most`. What
    /// overlaps there, for a list to choose from.
    pub fn overlaps(
        &self,
        camera: &Camera,
        size: [f32; 2],
        at: DVec2,
        picks: Picks,
        pixels: f64,
        most: usize,
    ) -> Vec<Pick> {
        let Some(projector) = Projector::world(camera, size[0], size[1]) else {
            return Vec::new();
        };
        let Some(ray) = self.ray(&projector, at) else {
            return Vec::new();
        };
        // Each item once, where it's nearest the cursor (to
        // [`SAME_PLACE`]), then the eye; by that, then the item.
        let nearest = |mut found: Vec<Near<Picked>>| {
            found.sort_by(|a, b| a.item.cmp(&b.item).then(a.order(b)));
            found.dedup_by(|later, first| later.item == first.item);
            found.sort_by(Near::order);
            found
        };
        let mut vertices = Vec::new();
        if picks.vertices() {
            vertices = (self.vertices_near(&projector, &ray, at, pixels).into_iter())
                .map(|near| near.by_depth(Picked::Vertex(near.item)))
                .collect();
        }
        let mut edges = Vec::new();
        if picks.edges() {
            edges = (self.segments_near(&projector, &ray, at, pixels).into_iter())
                .map(|near| near.by_depth(Picked::Edge(self.segment_edge(near.item))))
                .collect();
        }
        let mut faces = Vec::new();
        if picks.faces() {
            let grown = reach(&projector, pixels);
            self.tables.triangles().near(&ray, grown, |triangle| {
                let [a, b, c] = self.triangle_points(triangle)?;
                // Under the cursor, else showing within reach of it.
                let hit = ray_hits(ray.origin, ray.direction, [a, b, c])
                    .filter(|&t| t >= ray.from)
                    .map(|t| (0.0, ray.at(t)));
                let (distance, point) = hit.or_else(|| {
                    [(a, b), (b, c), (c, a)]
                        .into_iter()
                        .filter_map(|(u, v)| shown_near(&projector, u, v, at, pixels))
                        .min_by(|x, y| x.0.total_cmp(&y.0))
                })?;
                let face = Picked::Face(self.triangle_face(triangle)?);
                let depth = projector.world_depth(point);
                faces.push(Near {
                    distance,
                    depth,
                    item: face,
                    point,
                });
                Some(())
            });
        }
        [vertices, edges, faces]
            .into_iter()
            .flat_map(nearest)
            .filter_map(|near| self.pick_of(near.item, near.point))
            .take(most)
            .collect()
    }

    /// The picking corner at vertex `vertex`, its snap point
    /// ([`PickIndex::snaps`]), if it's in the tables.
    pub fn vertex_corner(&self, vertex: u32) -> Option<u32> {
        (self.snaps(Picked::Vertex(vertex)).into_iter()).find_map(|(snapped, _)| match snapped {
            Snapped::Corner(corner) => Some(corner),
            Snapped::EdgePoint(_) => None,
        })
    }

    /// The snap points of `target`, where the measure tool picks points:
    /// a face's corners (those naming it among their three faces), an
    /// edge's ends (the corners naming both its faces) and its own point,
    /// if it has one, a vertex's corner (the one at it whose faces meet
    /// there); each with where it is. None for what isn't in the tables.
    pub fn snaps(&self, target: Picked) -> Vec<(Snapped, DVec3)> {
        let corners = self.picking.corners().iter().enumerate();
        let corner = |(i, corner): (usize, &varde_regen::PickCorner)| {
            Some((
                Snapped::Corner(u32::try_from(i).ok()?),
                DVec3::from(corner.point),
            ))
        };
        match target {
            Picked::Face(face) => corners
                .filter(|(_, corner)| corner.faces.contains(&face))
                .filter_map(corner)
                .collect(),
            Picked::Edge(edge) => {
                let Some([a, b]) = self.edge_faces(edge) else {
                    return Vec::new();
                };
                let ends = corners
                    .filter(|(_, corner)| corner.faces.contains(&a) && corner.faces.contains(&b))
                    .filter_map(corner);
                let own = (self.picking.snaps().get(edge as usize).copied().flatten())
                    .map(|point| (Snapped::EdgePoint(edge), DVec3::from(point)));
                ends.chain(own).collect()
            }
            Picked::Vertex(vertex) => {
                let Some(at) = corner_position(&self.mesh, vertex) else {
                    return Vec::new();
                };
                let faces = self.corner_faces(vertex);
                let meets = |f: &u32| faces.iter().any(|&(_, face)| face == *f);
                // The mesh's corner is the solid's vertex rounded to f32.
                corners
                    .filter(|(_, corner)| {
                        corner.faces.iter().all(meets) && DVec3::from(corner.point).as_vec3() == at
                    })
                    .filter_map(corner)
                    .collect()
            }
        }
    }

    /// Where the snap point `snapped` is, if it's in the tables.
    pub fn snap_point(&self, snapped: Snapped) -> Option<DVec3> {
        match snapped {
            Snapped::Corner(corner) => {
                let corner = self.picking.corners().get(corner as usize)?;
                Some(DVec3::from(corner.point))
            }
            Snapped::EdgePoint(edge) => {
                Some(DVec3::from((*self.picking.snaps().get(edge as usize)?)?))
            }
        }
    }

    /// Of `target`'s snap points ([`PickIndex::snaps`]), the one showing
    /// nearest the screen position `at` (as [`PickIndex::pick`] takes it)
    /// within [`SNAP_REACH`], if any: ties go to the first. A point
    /// behind the near plane of a perspective view doesn't show.
    pub fn snap(
        &self,
        camera: &Camera,
        size: [f32; 2],
        at: DVec2,
        target: Picked,
    ) -> Option<(Snapped, DVec3)> {
        let projector = Projector::world(camera, size[0], size[1])?;
        let mut best: Option<(f64, Snapped, DVec3)> = None;
        for (snapped, point) in self.snaps(target) {
            if projector.perspective() && projector.world_depth(point) < projector.near() {
                continue;
            }
            let distance = projector.show(point).distance(at);
            if distance <= SNAP_REACH && best.is_none_or(|(b, ..)| distance < b) {
                best = Some((distance, snapped, point));
            }
        }
        best.map(|(_, snapped, point)| (snapped, point))
    }

    /// The edges of `chain`'s tangent chain, ascending: the edges it runs
    /// on into smoothly, end to end, and itself. None if there's no such
    /// edge between two faces.
    pub fn tangent_chain(&self, chain: u32) -> &[u32] {
        if self.edge_faces(chain).is_none() {
            return &[];
        }
        let Some(&first) = self.picking.tangents().get(chain as usize) else {
            return &[];
        };
        self.tables.tangent_chain(first)
    }

    /// The box of `bodies`' faces in the model, its least and greatest
    /// corners, if any of them shows.
    pub fn bodies_bounds(&self, bodies: &[BodyId]) -> Option<[DVec3; 2]> {
        let mut bounds: Option<[Vec3; 2]> = None;
        for at in self.body_points(bodies) {
            bounds = Some(match bounds {
                None => [at, at],
                Some([low, high]) => [low.min(at), high.max(at)],
            });
        }
        bounds.map(|corners| corners.map(|corner| corner.as_dvec3()))
    }

    /// How far `bodies`' faces in the model reach along `direction` (unit
    /// or not), from the nearest point to the farthest, if any of them
    /// shows and the direction has a length: the mesh's points, so within
    /// its tolerance of the faces.
    pub fn bodies_extent(&self, bodies: &[BodyId], direction: DVec3) -> Option<f64> {
        let along = direction.try_normalize()?;
        let mut range: Option<(f64, f64)> = None;
        for at in self.body_points(bodies) {
            let x = at.as_dvec3().dot(along);
            range = Some(match range {
                None => (x, x),
                Some((low, high)) => (low.min(x), high.max(x)),
            });
        }
        range.map(|(low, high)| high - low)
    }

    /// The points of `bodies`' faces' triangles in the model, repeated
    /// where triangles share them.
    fn body_points<'a>(&'a self, bodies: &'a [BodyId]) -> impl Iterator<Item = Vec3> + 'a {
        let indices = self.mesh.indices();
        (bodies.iter())
            .flat_map(move |&body| self.body_faces(body))
            .flat_map(move |face| {
                (self.mesh.face_indices(face as usize))
                    .and_then(|range| indices.get(range))
                    .unwrap_or_default()
            })
            .map(|&vertex| position(&self.mesh, vertex))
    }

    /// The faces of `body`, ascending.
    pub fn body_faces(&self, body: BodyId) -> impl Iterator<Item = u32> + '_ {
        let starts = std::iter::once(0).chain(self.mesh.part_ends().iter().map(|&[f, _, _, _]| f));
        (self.picking.bodies().iter())
            .zip(starts.zip(self.mesh.part_ends()))
            .filter(move |(part, _)| **part == body)
            .flat_map(|(_, (start, &[end, _, _, _]))| start..end)
    }

    /// The face triangle `triangle` is of, if there's such a triangle.
    fn triangle_face(&self, triangle: u32) -> Option<u32> {
        let start = u64::from(triangle) * 3;
        let face = (self.mesh.face_ends()).partition_point(|&end| u64::from(end) <= start);
        (face < self.mesh.face_count()).then_some(face as u32)
    }

    /// The triangles of face `face`, none if there's no such face.
    fn face_triangles(&self, face: u32) -> Range<u32> {
        // Within the mesh's indices, `u32`s.
        let indices = self.mesh.face_indices(face as usize).unwrap_or_default();
        (indices.start / 3) as u32..(indices.end / 3) as u32
    }

    /// Where each segment of edge `edge` starts in the mesh's edge
    /// vertices, none if there's no such edge.
    fn edge_segments(&self, edge: u32) -> Range<u32> {
        let range = self.mesh.polyline_range(edge as usize).unwrap_or_default();
        // Within the mesh's edge points, a `u32`.
        range.start as u32..range.end.saturating_sub(1) as u32
    }

    /// The corners of the mesh's triangle `triangle`, if it has one.
    fn corners(&self, triangle: u32) -> Option<[u32; 3]> {
        let start = (triangle as usize).checked_mul(3)?;
        let corners = self.mesh.indices().get(start..start.checked_add(3)?)?;
        Some([corners[0], corners[1], corners[2]])
    }

    /// Where the mesh's corners `corners` are, widened to `f64`.
    fn points(&self, corners: [u32; 3]) -> [DVec3; 3] {
        corners.map(|i| position(&self.mesh, i).as_dvec3())
    }

    /// Where the corners of the mesh's triangle `triangle` are, if it has one.
    fn triangle_points(&self, triangle: u32) -> Option<[DVec3; 3]> {
        Some(self.points(self.corners(triangle)?))
    }

    /// The cursor's ray at `at`, from where the model may show along it.
    fn ray(&self, projector: &Projector, at: DVec2) -> Option<Ray> {
        let (origin, direction) = projector.ray(at)?;
        if projector.perspective() {
            // At the target's depth `t` is 1, so a depth `t` times the
            // camera's distance.
            return Some(Ray {
                origin,
                direction,
                from: projector.near() / projector.distance(),
            });
        }
        // An orthographic view sees what's behind its eye too: from far
        // enough back that the whole mesh is ahead.
        let direction = direction.try_normalize()?;
        let back = self.back(origin);
        Some(Ray {
            origin: origin - direction * back,
            direction,
            from: 0.0,
        })
    }

    /// How far back from `origin` a ray must start to have the whole mesh
    /// ahead of it, whichever way it goes.
    fn back(&self, origin: DVec3) -> f64 {
        self.bounds.map_or(0.0, |bounds| {
            let (min, max) = (bounds.min.as_dvec3(), bounds.max.as_dvec3());
            (max - min).length() + (origin - (min + max) / 2.0).length()
        })
    }

    /// Where along `ray` between `from` and `to` it first meets a
    /// triangle, and which, if it does.
    fn first_hit(&self, ray: &Ray, from: f64, to: f64) -> Option<(f64, u32)> {
        (self.tables.triangles())
            .nearest(ray, from, to, |triangle| self.triangle_hit(ray, triangle))
    }

    /// Where along `ray` it meets the triangle `triangle`, front or back,
    /// if it does.
    fn triangle_hit(&self, ray: &Ray, triangle: u32) -> Option<f64> {
        let corners = self.triangle_points(triangle)?;
        ray_hits(ray.origin, ray.direction, corners)
    }

    /// Where along `ray`, from its `from` on, it first meets the front of a
    /// triangle, and which, if it does: what's drawn there, the backs of
    /// the triangles being culled.
    fn first_front(&self, ray: &Ray) -> Option<(f64, u32)> {
        (self.tables.triangles()).nearest(ray, ray.from, f64::INFINITY, |triangle| {
            let [a, b, c] = self.triangle_points(triangle)?;
            // Its corners run counterclockwise seen from its front.
            let facing = (b - a).cross(c - a).dot(ray.direction) < 0.0;
            facing.then(|| ray_hits(ray.origin, ray.direction, [a, b, c]))?
        })
    }

    /// The vertex showing nearest `at` within [`VERTEX_REACH`] that
    /// nothing hides ([`PickIndex::nearest_shown`]): its corner and where
    /// it is.
    fn vertex_near(&self, projector: &Projector, ray: &Ray, at: DVec2) -> Option<(u32, DVec3)> {
        let near = self.vertices_near(projector, ray, at, VERTEX_REACH);
        self.nearest_shown(projector, near)
    }

    /// The vertices showing within `pixels` of `at`, hidden or not, as
    /// how far from it they show, how deep, their corner and where.
    fn vertices_near(
        &self,
        projector: &Projector,
        ray: &Ray,
        at: DVec2,
        pixels: f64,
    ) -> Vec<Near<u32>> {
        let mut near = Vec::new();
        (self.tables.vertices()).near(ray, reach(projector, pixels), |corner| {
            let point = self.corner_point(corner)?;
            let (point, _) = projector.in_front(point, point)?;
            let distance = projector.show(point).distance(at);
            if distance.is_nan() || distance > pixels {
                return None;
            }
            near.push(Near {
                distance,
                depth: projector.world_depth(point),
                item: corner,
                point,
            });
            Some(())
        });
        near
    }

    /// The edge showing nearest `at` within [`EDGE_REACH`] that nothing
    /// hides ([`PickIndex::nearest_shown`]), and its point showing
    /// nearest `at`: its chain and that point.
    fn edge_near(&self, projector: &Projector, ray: &Ray, at: DVec2) -> Option<(u32, DVec3)> {
        let near = self.segments_near(projector, ray, at, EDGE_REACH);
        let (start, point) = self.nearest_shown(projector, near)?;
        Some((self.segment_edge(start), point))
    }

    /// The edge segments showing within `pixels` of `at`, hidden or not,
    /// as how far from it they show, how deep their point showing nearest
    /// it is, where they start and that point.
    fn segments_near(
        &self,
        projector: &Projector,
        ray: &Ray,
        at: DVec2,
        pixels: f64,
    ) -> Vec<Near<u32>> {
        let mut near = Vec::new();
        (self.tables.segments()).near(ray, reach(projector, pixels), |start| {
            let (distance, point) = self.segment_near(projector, start, at, pixels)?;
            near.push(Near {
                distance,
                depth: projector.world_depth(point),
                item: start,
                point,
            });
            Some(())
        });
        near
    }

    /// How far from `at` the edge segment starting at `start` shows, and
    /// its point showing nearest `at`, if it shows within `reach` pixels.
    fn segment_near(
        &self,
        projector: &Projector,
        start: u32,
        at: DVec2,
        reach: f64,
    ) -> Option<(f64, DVec3)> {
        let [a, b] = segment(&self.mesh, start)?;
        shown_near(projector, a.as_dvec3(), b.as_dvec3(), at, reach)
    }

    /// The edge whose vertices the segment starting at `start` is among.
    fn segment_edge(&self, start: u32) -> u32 {
        (self.mesh.edge_ends()).partition_point(|&end| end <= start) as u32
    }

    /// Of the items `near` the cursor, as how far from it they show, how
    /// deep, which and where, the nearest that nothing hides, sorted by
    /// distance (to [`SAME_PLACE`]), depth and item: none once the mesh
    /// has been searched [`MAX_HIDDEN_SEARCHES`] times for what hides
    /// them.
    fn nearest_shown(
        &self,
        projector: &Projector,
        mut near: Vec<Near<u32>>,
    ) -> Option<(u32, DVec3)> {
        near.sort_by(Near::order);
        let mut occluders = Vec::new();
        let mut searches = 0;
        for Near { item, point, .. } in near {
            match self.hidden(projector, point, &mut occluders) {
                Hidden::No => return Some((item, point)),
                Hidden::Known => {}
                Hidden::Searched => {
                    searches += 1;
                    if searches >= MAX_HIDDEN_SEARCHES {
                        return None;
                    }
                }
            }
        }
        None
    }

    /// The faces at `corner`, as `(corner, face)`, ascending.
    fn corner_faces(&self, corner: u32) -> &[(u32, u32)] {
        let corner_faces = self.tables.corner_faces();
        let from = corner_faces.partition_point(|&(at, _)| at < corner);
        let to = corner_faces.partition_point(|&(at, _)| at <= corner);
        &corner_faces[from..to]
    }

    /// Whether something of the mesh is in front of the world point
    /// `point`, nearer the eye by more than [`HIDDEN_PULL`] view heights
    /// and the mesh's `f32` rounding: one of `occluders` (triangles found
    /// hiding a point before), or one the mesh is searched for, which
    /// joins them (the latest [`MAX_OCCLUDERS`]).
    fn hidden(&self, projector: &Projector, point: DVec3, occluders: &mut Vec<u32>) -> Hidden {
        let scale = self.bounds.map_or(0.0, |bounds| {
            f64::from(bounds.min.abs().max(bounds.max.abs()).max_element())
        });
        let slack = HIDDEN_PULL * projector.view_height() + 8.0 * f64::from(f32::EPSILON) * scale;
        let (eye, backward) = projector.eye();
        let (ray, from, to) = if projector.perspective() {
            // At `point` `t` is 1, so a depth `t` times its depth.
            let depth = projector.world_depth(point);
            if depth.is_nan() || depth <= slack {
                return Hidden::No;
            }
            let ray = Ray {
                origin: eye,
                direction: point - eye,
                from: 0.0,
            };
            (ray, projector.near() / depth, 1.0 - slack / depth)
        } else {
            let back = self.back(point);
            let ray = Ray {
                origin: point + backward * back,
                direction: -backward,
                from: 0.0,
            };
            (ray, 0.0, back - slack)
        };
        let within = |t: f64| t >= from && t <= to;
        if (occluders.iter()).any(|&triangle| self.triangle_hit(&ray, triangle).is_some_and(within))
        {
            return Hidden::Known;
        }
        let Some((_, triangle)) = self.first_hit(&ray, from, to) else {
            return Hidden::No;
        };
        if occluders.len() >= MAX_OCCLUDERS {
            occluders.remove(0);
        }
        occluders.push(triangle);
        Hidden::Searched
    }
}

/// Whether something hides a point, and how that was found
/// ([`PickIndex::hidden`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hidden {
    /// Nothing does.
    No,
    /// A triangle found hiding a point before does.
    Known,
    /// One the mesh was searched for does.
    Searched,
}

/// An item showing near the cursor: how far from it it shows, in pixels,
/// how deep, which and where.
#[derive(Debug, Clone, Copy)]
struct Near<T> {
    distance: f64,
    depth: f64,
    item: T,
    point: DVec3,
}

impl<T: Ord> Near<T> {
    /// Nearest the cursor (to [`SAME_PLACE`]) first, then the eye, then
    /// by item.
    fn order(&self, other: &Self) -> Ordering {
        (same_place(self.distance).total_cmp(&same_place(other.distance)))
            .then(self.depth.total_cmp(&other.depth))
            .then(self.item.cmp(&other.item))
    }

    /// As `item`, its distance dropped: ordered by depth alone.
    fn by_depth<U>(self, item: U) -> Near<U> {
        Near {
            distance: 0.0,
            depth: self.depth,
            item,
            point: self.point,
        }
    }
}

/// `distance` in pixels, rounded to [`SAME_PLACE`]: items as far from the
/// cursor by it are in the same place.
fn same_place(distance: f64) -> f64 {
    (distance / SAME_PLACE).round()
}

/// The mesh's vertex `index`, or the origin if it has none.
fn position(mesh: &RenderMesh, index: u32) -> Vec3 {
    Vec3::from(
        mesh.positions()
            .get(index as usize)
            .copied()
            .unwrap_or_default(),
    )
}

/// Where `mesh`'s corner `corner` is, if it has it.
fn corner_position(mesh: &RenderMesh, corner: u32) -> Option<Vec3> {
    Some(Vec3::from(*mesh.corners().get(corner as usize)?))
}

/// How far a box of the world, from `min` to `max`, must be grown for
/// what's in it showing within `pixels` of the cursor to be within it
/// grown: that many pixels at its deepest, as far from the ray.
fn reach(projector: &Projector, pixels: f64) -> impl Fn(DVec3, DVec3) -> f64 + '_ {
    move |min, max| {
        let (_, backward) = projector.eye();
        let half = (max - min) / 2.0;
        let deepest = projector.world_depth((min + max) / 2.0) + half.dot(backward.abs());
        pixels * projector.pixel_at(deepest)
    }
}

/// The ends of `mesh`'s edge segment starting at `start` in its edge
/// vertices, if there's one.
fn segment(mesh: &RenderMesh, start: u32) -> Option<[Vec3; 2]> {
    let start = start as usize;
    let ends = mesh.edge_vertices().get(start..start.checked_add(2)?)?;
    Some([position(mesh, ends[0]), position(mesh, ends[1])])
}

/// How far from `at` the world segment from `a` to `b` shows, the part of
/// it in front of the eye, and its point showing nearest `at`, if it shows
/// within `reach` pixels.
fn shown_near(
    projector: &Projector,
    a: DVec3,
    b: DVec3,
    at: DVec2,
    reach: f64,
) -> Option<(f64, DVec3)> {
    let (a, b) = projector.in_front(a, b)?;
    let (pa, pb) = (projector.show(a), projector.show(b));
    let (distance, s) = segment_nearest(at, pa, pb);
    if distance.is_nan() || distance > reach {
        return None;
    }
    // Perspective divides by depth, so the point showing `s` of the way
    // along isn't `s` of the way along in the world.
    let u = if projector.perspective() {
        let (da, db) = (projector.world_depth(a), projector.world_depth(b));
        let denominator = (1.0 - s) * db + s * da;
        if denominator > 0.0 {
            s * da / denominator
        } else {
            s
        }
    } else {
        s
    };
    Some((distance, a + (b - a) * u.clamp(0.0, 1.0)))
}

#[cfg(test)]
pub(crate) mod tests;
