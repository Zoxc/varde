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

use std::ops::Range;
use std::sync::Arc;

use glam::{DVec2, DVec3, Vec3};
use varde_document::{BodyId, FaceRef, OriginPlane, Placement};
use varde_kernel::RenderMesh;
use varde_kernel::mesh::FaceKey;
use varde_regen::{Picking, Summary};
use varde_render::{Camera, Highlights, Vertex};

use crate::projection::Projector;

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

/// The most edges or vertices near the cursor looked at for one that
/// isn't hidden, nearest first: the rest are as good as hidden.
const MAX_EDGE_TESTS: usize = 64;

/// Within how many pixels of each other two edges or vertices count as
/// equally near the cursor, the nearer the eye going first: a corner seen
/// straight down an edge shows where the one behind it does, which the
/// hidden test can take for seen, its ray running along the faces there.
const SAME_PLACE: f64 = 0.5;

/// The most triangles or edges in a leaf of a hierarchy.
const LEAF: usize = 4;

/// What the cursor picks in the model: faces, edges and vertices (a
/// vertex near the cursor winning over an edge, and an edge over a face),
/// or only faces or only edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Picks {
    #[default]
    All,
    Faces,
    Edges,
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

/// What the viewport draws of what's hovered and selected over the model
/// it was built for, by the mesh's ids (see
/// [`varde_render::Frame::hovered_faces`]): the faces hovered, drawn
/// brighter, and selected, tinted; the hovered edges outlined, the
/// selected edges, and the hovered and selected vertices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelHighlight {
    pub hovered_faces: Vec<u32>,
    pub selected_faces: Vec<u32>,
    /// The faces in the second colour: the measure tool's B, where A is
    /// drawn as selected.
    pub second_faces: Vec<u32>,
    pub highlights: Arc<Highlights>,
}

impl ModelHighlight {
    /// Whether it draws nothing.
    pub fn is_empty(&self) -> bool {
        let Highlights {
            outlined,
            selected_edges,
            second_edges,
            vertices,
        } = &*self.highlights;
        self.hovered_faces.is_empty()
            && self.selected_faces.is_empty()
            && self.second_faces.is_empty()
            && outlined.is_empty()
            && selected_edges.is_empty()
            && second_edges.is_empty()
            && vertices.is_empty()
    }
}

/// The model shown, made ready for picking: its mesh, its tables, and
/// hierarchies over the mesh's triangles, the segments of its edges
/// between two faces and its vertices.
#[derive(Debug)]
pub struct PickIndex {
    mesh: Arc<RenderMesh>,
    picking: Arc<Picking>,
    model: u64,
    /// Over the mesh's triangles, by index.
    triangles: Bvh,
    /// Over the segments of the mesh's edges between two faces, each by
    /// where it starts in [`RenderMesh::edge_vertices`].
    segments: Bvh,
    /// Over the mesh's corners where three faces or more meet, by index.
    vertices: Bvh,
    /// The faces at each corner an edge between two faces ends at, as
    /// `(corner, face)`, sorted, each once.
    corner_faces: Vec<(u32, u32)>,
    /// Each tangent chain's edges, by its first.
    tangent_chains: Groups,
}

impl PickIndex {
    /// The index of `mesh` and its tables `picking`, which came with it;
    /// its picks carry `model`. Tables that don't go with the mesh pick
    /// nothing.
    pub fn new(mesh: Arc<RenderMesh>, picking: Arc<Picking>, model: u64) -> Self {
        let fits = picking.bodies().len() == mesh.part_ends().len()
            && picking.faces().len() == mesh.face_count()
            && picking.tangents().len() == mesh.edge_count();
        let (triangles, segments, vertices, corner_faces, tangent_chains) = if fits {
            let corners = |triangle: &[u32; 3]| triangle.map(|i| position(&mesh, i));
            let triangles = mesh.indices().as_chunks::<3>().0;
            let boxes = triangles.iter().map(|t| bounds(&corners(t)));
            let chain = |edge: usize| matches!(mesh.edge_faces()[edge], [a, b] if a != b);
            let segments: Vec<u32> = (0..mesh.edge_count())
                .filter(|&edge| chain(edge))
                .flat_map(|edge| {
                    let range = mesh.polyline_range(edge).unwrap_or_default();
                    range.start..range.end.saturating_sub(1)
                })
                .filter_map(|start| u32::try_from(start).ok())
                .collect();
            let segment_boxes = segments.iter().map(|&start| {
                let ends = segment(&mesh, start).unwrap_or_default();
                bounds(&ends)
            });
            let mut corner_faces: Vec<(u32, u32)> = (0..mesh.edge_count())
                .filter(|&edge| chain(edge))
                .flat_map(|edge| {
                    let faces = mesh.edge_faces()[edge];
                    (mesh.edge_corners()[edge].into_iter())
                        .flat_map(move |corner| faces.map(|face| (corner, face)))
                })
                .collect();
            corner_faces.sort_unstable();
            corner_faces.dedup();
            let vertices: Vec<u32> = vertex_runs(&corner_faces).map(|run| run[0].0).collect();
            let vertex_boxes = vertices.iter().map(|&corner| {
                let at = corner_position(&mesh, corner).unwrap_or_default();
                [at, at]
            });
            // A crease is in no tangent chain.
            let tangents: Vec<u32> = (picking.tangents().iter().enumerate())
                .map(|(edge, &first)| if chain(edge) { first } else { u32::MAX })
                .collect();
            (
                Bvh::new(boxes.collect()),
                Bvh::with_items(segment_boxes.collect(), segments),
                Bvh::with_items(vertex_boxes.collect(), vertices),
                corner_faces,
                Groups::new(mesh.edge_count(), &tangents),
            )
        } else {
            Default::default()
        };
        Self {
            mesh,
            picking,
            model,
            triangles,
            segments,
            vertices,
            corner_faces,
            tangent_chains,
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

    /// The reference to face `face` picked at `near`, as a sketch plane
    /// stores it, if there's such a face.
    pub fn face_ref(&self, face: u32, near: DVec3) -> Option<FaceRef> {
        Some(FaceRef {
            body: self.face_body(face)?,
            key: self.picking.faces().get(face as usize)?.key,
            near,
        })
    }

    /// The faces either side of `edge`, if it's an edge between two
    /// faces, not a crease.
    pub fn edge_faces(&self, edge: u32) -> Option<[u32; 2]> {
        let [a, b] = *self.mesh.edge_faces().get(edge as usize)?;
        (a != b).then_some([a, b])
    }

    /// What's under the screen position `at`, in logical pixels from the
    /// top left of a viewport `size` big seen by `camera`: a vertex
    /// showing within [`VERTEX_REACH`] of it that nothing hides, the
    /// nearest, or else such an edge within [`EDGE_REACH`], or else the
    /// face the cursor's ray first meets the front of; of those only what
    /// `picks` takes.
    pub fn pick(&self, camera: &Camera, size: [f32; 2], at: DVec2, picks: Picks) -> Option<Pick> {
        let placement = OriginPlane::XY.placement();
        let projector = Projector::new(camera, placement, size[0], size[1])?;
        let ray = self.ray(camera, &projector, at)?;
        let vertex = || {
            (picks == Picks::All)
                .then(|| self.vertex_near(camera, &projector, &ray, at))
                .flatten()
                .map(|(corner, at)| (Picked::Vertex(corner), at))
        };
        let edge = || {
            (picks != Picks::Faces)
                .then(|| self.edge_near(camera, &projector, &ray, at))
                .flatten()
                .map(|(chain, at)| (Picked::Edge(chain), at))
        };
        let face = || {
            let (t, triangle) = (picks != Picks::Edges)
                .then(|| self.first_front(&ray))
                .flatten()?;
            Some((Picked::Face(self.triangle_face(triangle)?), ray.at(t)))
        };
        let (target, at) = vertex().or_else(edge).or_else(face)?;
        let pick = Pick {
            model: self.model,
            target,
            body: self.body(target)?,
            at,
            snap: None,
        };
        Some(pick)
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
        let placement = OriginPlane::XY.placement();
        let projector = Projector::new(camera, placement, size[0], size[1])?;
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
        self.tangent_chains.get(first)
    }

    /// The faces of `body`, ascending.
    pub fn body_faces(&self, body: BodyId) -> impl Iterator<Item = u32> + '_ {
        let starts = std::iter::once(0).chain(self.mesh.part_ends().iter().map(|&[f, _, _, _]| f));
        (self.picking.bodies().iter())
            .zip(starts.zip(self.mesh.part_ends()))
            .filter(move |(part, _)| **part == body)
            .flat_map(|(_, (start, &[end, _, _, _]))| start..end)
    }

    /// The keys of the faces either side of `chain`, sorted, as an edge
    /// reference keeps them, if it's an edge between two faces.
    pub fn chain_keys(&self, chain: u32) -> Option<[FaceKey; 2]> {
        let faces = self.edge_faces(chain)?;
        let [a, b] = faces.map(|f| self.picking.faces().get(f as usize).map(|face| face.key));
        let (a, b) = (a?, b?);
        Some([a.min(b), a.max(b)])
    }

    /// The keys that name `corner`, a vertex: the lowest three of those of
    /// the faces meeting there, sorted, if three faces or more do.
    pub fn vertex_keys(&self, corner: u32) -> Option<[FaceKey; 3]> {
        let mut keys: Vec<FaceKey> = (self.corner_faces(corner).iter())
            .filter_map(|&(_, face)| Some(self.picking.faces().get(face as usize)?.key))
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

    /// The face of `body` that `key` names (its key or an alias), the
    /// nearest to `near` among several, as the kernel resolves a face
    /// reference: one is taken wherever `near` is; of several, a later
    /// one only where it comes nearer by more than a billionth of the
    /// model's size, so ties go to the lowest. Measured to the mesh as
    /// drawn, which is all the view has.
    pub fn find_face(&self, body: BodyId, key: &FaceKey, near: DVec3) -> Option<u32> {
        let found = (self.body_faces(body)).filter(|&face| self.named(face, key));
        self.nearest(found, near, |face| {
            (self.face_triangles(face))
                .filter_map(|triangle| self.corners(triangle))
                .map(|corners| {
                    let corners = corners.map(|i| position(&self.mesh, i).as_dvec3());
                    triangle_distance(near, corners)
                })
                .fold(f64::INFINITY, f64::min)
        })
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
        self.nearest(found, near, |chain| {
            (self.edge_segments(chain))
                .filter_map(|start| segment(&self.mesh, start))
                .map(|[a, b]| segment_distance_3d(near, a.as_dvec3(), b.as_dvec3()))
                .fold(f64::INFINITY, f64::min)
        })
    }

    /// The vertex of `body` where faces that `keys` name meet (each by key
    /// or alias, among the faces there), the nearest to `near` among
    /// several, as [`PickIndex::find_face`] finds faces.
    pub fn find_vertex(&self, body: BodyId, keys: [FaceKey; 3], near: DVec3) -> Option<u32> {
        let found = vertex_runs(&self.corner_faces)
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
            .mesh
            .bounds()
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

    /// What's drawn of `hovered` and `selected`: the faces hovered
    /// brighter (their edges as they are), and those selected tinted; an
    /// edge hovered outlined, one selected drawn in the selection's
    /// colour; a vertex hovered or selected as a disc, the hovered one
    /// last. What the mesh hasn't is left out.
    pub fn highlight(&self, hovered: &[Picked], selected: &[Picked]) -> ModelHighlight {
        self.highlight_with(hovered, selected, &[])
    }

    /// [`PickIndex::highlight`], with the faces and edges of `second` in
    /// the second colour, as the measure tool draws its B (its A as
    /// selected). A vertex there is left out: the measure tool draws its
    /// points itself.
    pub fn highlight_with(
        &self,
        hovered: &[Picked],
        selected: &[Picked],
        second: &[Picked],
    ) -> ModelHighlight {
        let has = |target: &&Picked| {
            let (id, count) = match **target {
                Picked::Face(face) => (face, self.mesh.face_count()),
                Picked::Edge(edge) => (edge, self.mesh.edge_count()),
                Picked::Vertex(corner) => (corner, self.mesh.corners().len()),
            };
            (id as usize) < count
        };
        let hovered: Vec<Picked> = hovered.iter().filter(has).copied().collect();
        let selected: Vec<Picked> = selected.iter().filter(has).copied().collect();
        let second = second.iter().filter(has).copied();
        let mut highlight = ModelHighlight::default();
        let mut highlights = Highlights::default();
        for &target in &hovered {
            match target {
                Picked::Face(face) => highlight.hovered_faces.push(face),
                Picked::Edge(edge) => highlights.outlined.push(edge),
                Picked::Vertex(_) => {}
            }
        }
        for &target in &selected {
            match target {
                Picked::Face(face) => highlight.selected_faces.push(face),
                Picked::Edge(edge) => highlights.selected_edges.push(edge),
                Picked::Vertex(corner) => highlights.vertices.push(Vertex {
                    corner,
                    hovered: hovered.contains(&target),
                    selected: true,
                }),
            }
        }
        for target in second {
            match target {
                Picked::Face(face) => highlight.second_faces.push(face),
                Picked::Edge(edge) => highlights.second_edges.push(edge),
                Picked::Vertex(_) => {}
            }
        }
        // The hovered vertices last, over the others.
        for &target in &hovered {
            if let Picked::Vertex(corner) = target
                && !selected.contains(&target)
            {
                highlights.vertices.push(Vertex {
                    corner,
                    hovered: true,
                    selected: false,
                });
            }
        }
        highlight.highlights = Arc::new(highlights);
        highlight
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

    /// The cursor's ray at `at`, from where the model may show along it.
    fn ray(&self, camera: &Camera, projector: &Projector, at: DVec2) -> Option<Ray> {
        let (origin, direction) = projector.ray(at)?;
        if projector.perspective() {
            // At the target's depth `t` is 1, so a depth `t` times the
            // camera's distance.
            let distance = f64::from(camera.distance());
            return Some(Ray {
                origin,
                direction,
                from: projector.near() / distance,
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
        self.mesh.bounds().map_or(0.0, |bounds| {
            let (min, max) = (bounds.min.as_dvec3(), bounds.max.as_dvec3());
            (max - min).length() + (origin - (min + max) / 2.0).length()
        })
    }

    /// Where along `ray` between `from` and `to` it first meets a
    /// triangle, and which, if it does.
    fn first_hit(&self, ray: &Ray, from: f64, to: f64) -> Option<(f64, u32)> {
        self.triangles.nearest(ray, from, to, |triangle| {
            let corners = self.corners(triangle)?;
            let corners = corners.map(|i| position(&self.mesh, i).as_dvec3());
            ray_hits(ray.origin, ray.direction, corners)
        })
    }

    /// Where along `ray`, from its `from` on, it first meets the front of a
    /// triangle, and which, if it does: what's drawn there, the backs of
    /// the triangles being culled.
    fn first_front(&self, ray: &Ray) -> Option<(f64, u32)> {
        self.triangles
            .nearest(ray, ray.from, f64::INFINITY, |triangle| {
                let corners = self.corners(triangle)?;
                let [a, b, c] = corners.map(|i| position(&self.mesh, i).as_dvec3());
                // Its corners run counterclockwise seen from its front.
                let facing = (b - a).cross(c - a).dot(ray.direction) < 0.0;
                facing.then(|| ray_hits(ray.origin, ray.direction, [a, b, c]))?
            })
    }

    /// The vertex showing nearest `at` within [`VERTEX_REACH`] that
    /// nothing hides, of those [`MAX_EDGE_TESTS`] nearest: its corner and
    /// where it is.
    fn vertex_near(
        &self,
        camera: &Camera,
        projector: &Projector,
        ray: &Ray,
        at: DVec2,
    ) -> Option<(u32, DVec3)> {
        let mut near = Vec::new();
        self.vertices
            .near(ray, reach(projector, VERTEX_REACH), |corner| {
                let point = self.corner_point(corner)?;
                let (point, _) = projector.in_front(point, point)?;
                let distance = projector.show(point).distance(at);
                if distance.is_nan() || distance > VERTEX_REACH {
                    return None;
                }
                near.push((distance, projector.world_depth(point), corner, point));
                Some(())
            });
        self.nearest_shown(camera, projector, near)
    }

    /// The edge showing nearest `at` within [`EDGE_REACH`] that nothing
    /// hides, of those [`MAX_EDGE_TESTS`] nearest, and its point showing
    /// nearest `at`: its chain and that point.
    fn edge_near(
        &self,
        camera: &Camera,
        projector: &Projector,
        ray: &Ray,
        at: DVec2,
    ) -> Option<(u32, DVec3)> {
        let mut near = Vec::new();
        self.segments
            .near(ray, reach(projector, EDGE_REACH), |start| {
                let [a, b] = segment(&self.mesh, start)?;
                let (a, b) = projector.in_front(a.as_dvec3(), b.as_dvec3())?;
                let (pa, pb) = (projector.show(a), projector.show(b));
                let (distance, s) = segment_distance(at, pa, pb);
                if distance.is_nan() || distance > EDGE_REACH {
                    return None;
                }
                // Perspective divides by depth, so the point showing `s` of
                // the way along isn't `s` of the way along in the world.
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
                let point = a + (b - a) * u.clamp(0.0, 1.0);
                near.push((distance, projector.world_depth(point), start, point));
                Some(())
            });
        let (start, point) = self.nearest_shown(camera, projector, near)?;
        // The edge whose vertices the segment starts among.
        let edge = (self.mesh.edge_ends()).partition_point(|&end| end <= start);
        Some((edge as u32, point))
    }

    /// Of the items `near` the cursor, as how far from it they show, how
    /// deep, which and where, the nearest that nothing hides, of the
    /// [`MAX_EDGE_TESTS`] nearest: sorted by distance (to [`SAME_PLACE`]),
    /// depth and item.
    fn nearest_shown(
        &self,
        camera: &Camera,
        projector: &Projector,
        mut near: Vec<(f64, f64, u32, DVec3)>,
    ) -> Option<(u32, DVec3)> {
        let place = |distance: f64| (distance / SAME_PLACE).round();
        near.sort_by(|a, b| {
            (place(a.0).total_cmp(&place(b.0)))
                .then(a.1.total_cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
        let view_height = f64::from(camera.view_height());
        near.into_iter()
            .take(MAX_EDGE_TESTS)
            .find(|&(_, _, _, point)| !self.hidden(projector, point, view_height))
            .map(|(_, _, item, point)| (item, point))
    }

    /// The faces at `corner`, as `(corner, face)`, ascending.
    fn corner_faces(&self, corner: u32) -> &[(u32, u32)] {
        let from = (self.corner_faces).partition_point(|&(at, _)| at < corner);
        let to = (self.corner_faces).partition_point(|&(at, _)| at <= corner);
        &self.corner_faces[from..to]
    }

    /// Whether something of the mesh is in front of the world point
    /// `point`, nearer the eye by more than [`HIDDEN_PULL`] view heights
    /// and the mesh's `f32` rounding.
    fn hidden(&self, projector: &Projector, point: DVec3, view_height: f64) -> bool {
        let scale = self.mesh.bounds().map_or(0.0, |bounds| {
            f64::from(bounds.min.abs().max(bounds.max.abs()).max_element())
        });
        let slack = HIDDEN_PULL * view_height + 8.0 * f64::from(f32::EPSILON) * scale;
        let (eye, backward) = projector.eye();
        if projector.perspective() {
            // At `point` `t` is 1, so a depth `t` times its depth.
            let depth = projector.world_depth(point);
            if depth.is_nan() || depth <= slack {
                return false;
            }
            let ray = Ray {
                origin: eye,
                direction: point - eye,
                from: 0.0,
            };
            let from = projector.near() / depth;
            let to = 1.0 - slack / depth;
            self.first_hit(&ray, from, to).is_some()
        } else {
            let back = self.back(point);
            let ray = Ray {
                origin: point + backward * back,
                direction: -backward,
                from: 0.0,
            };
            self.first_hit(&ray, 0.0, back - slack).is_some()
        }
    }
}

/// A ray: the points `origin + direction * t`, those with `t` at least
/// `from` showing.
#[derive(Debug, Clone, Copy)]
struct Ray {
    origin: DVec3,
    direction: DVec3,
    from: f64,
}

impl Ray {
    fn at(&self, t: f64) -> DVec3 {
        self.origin + self.direction * t
    }
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

/// The runs of `corner_faces`, sorted by corner, of each corner where
/// three faces or more meet: the vertices.
fn vertex_runs(corner_faces: &[(u32, u32)]) -> impl Iterator<Item = &[(u32, u32)]> {
    (corner_faces.chunk_by(|a, b| a.0 == b.0)).filter(|run| run.len() >= 3)
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

/// How far `p` is from the segment from `a` to `b`.
fn segment_distance_3d(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let length = ab.length_squared();
    let s = if length > 0.0 {
        ((p - a).dot(ab) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * s)
}

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

/// The box around `points`.
fn bounds(points: &[Vec3]) -> [Vec3; 2] {
    let first = points.first().copied().unwrap_or_default();
    (points.iter()).fold([first; 2], |[min, max], &p| [min.min(p), max.max(p)])
}

/// How far `p` is from the segment from `a` to `b`, and where along it
/// the nearest point is, as a fraction of the way from `a`.
fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> (f64, f64) {
    let ab = b - a;
    let length = ab.length_squared();
    let s = if length > 0.0 {
        ((p - a).dot(ab) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.distance(a + ab * s), s)
}

/// How far along the ray from `origin` along `direction` it meets the
/// triangle `corners`, either side of it, if it does (Möller–Trumbore).
pub(crate) fn ray_hits(origin: DVec3, direction: DVec3, [a, b, c]: [DVec3; 3]) -> Option<f64> {
    let (ab, ac) = (b - a, c - a);
    let p = direction.cross(ac);
    let determinant = ab.dot(p);
    if determinant.abs() <= f64::EPSILON * ab.length() * ac.length() * direction.length() {
        return None;
    }
    let s = origin - a;
    let u = s.dot(p) / determinant;
    let q = s.cross(ab);
    let v = direction.dot(q) / determinant;
    let inside = (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0;
    let t = ac.dot(q) / determinant;
    (inside && t.is_finite()).then_some(t)
}

/// The corners of `bounds`, as [`through_box`] takes them.
pub(crate) fn aabb(bounds: varde_kernel::Aabb) -> [DVec3; 2] {
    [bounds.min.as_dvec3(), bounds.max.as_dvec3()]
}

/// Where the ray from `origin` along `direction` is inside the box `min`
/// to `max`, as how far along it it goes in and comes out, within `from`
/// and `to`, if it is.
pub(crate) fn through_box(
    origin: DVec3,
    direction: DVec3,
    [min, max]: [DVec3; 2],
    from: f64,
    to: f64,
) -> Option<(f64, f64)> {
    let (mut near, mut far) = (from, to);
    for axis in 0..3 {
        let (o, d) = (origin[axis], direction[axis]);
        if d == 0.0 {
            if o < min[axis] || o > max[axis] {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((min[axis] - o) / d, (max[axis] - o) / d);
        near = near.max(t0.min(t1));
        far = far.min(t0.max(t1));
    }
    (near <= far).then_some((near, far))
}

/// Items grouped by what they belong to: a face's triangles, a chain's
/// edges.
#[derive(Debug, Default)]
struct Groups {
    /// Where each group's items start in `items`, and the end.
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl Groups {
    /// The items of `groups` groups, item `i` in group `of[i]`, unless
    /// that's past the groups (as a crease's tangent chain is made out to
    /// be, `u32::MAX`). None if there are more items than `u32`s number,
    /// which no mesh has.
    fn new(groups: usize, of: &[u32]) -> Self {
        if u32::try_from(of.len()).is_err() {
            return Self::default();
        }
        let group = |g: u32| Some(g as usize).filter(|&g| g < groups);
        // Counts, then running sums: no more than `of.len()`, a `u32`.
        let mut starts = vec![0u32; groups + 1];
        for g in of.iter().filter_map(|&g| group(g)) {
            starts[g + 1] += 1;
        }
        for g in 0..groups {
            starts[g + 1] += starts[g];
        }
        let mut filled = starts.clone();
        let mut items = vec![0u32; starts[groups] as usize];
        for (item, g) in of.iter().enumerate() {
            if let Some(g) = group(*g) {
                items[filled[g] as usize] = item as u32;
                filled[g] += 1;
            }
        }
        Self { starts, items }
    }

    /// The items of `group`, none if there's no such group.
    fn get(&self, group: u32) -> &[u32] {
        let group = group as usize;
        let (Some(&start), Some(&end)) = (self.starts.get(group), self.starts.get(group + 1))
        else {
            return &[];
        };
        self.items.get(start as usize..end as usize).unwrap_or(&[])
    }
}

/// A bounding volume hierarchy over items with boxes: split in two at the
/// median of their middles along the box's longest side, until a few
/// are left. Built sequentially, the same for the same items.
#[derive(Debug, Default)]
struct Bvh {
    nodes: Vec<Node>,
    /// The items in the leaves' order.
    items: Vec<u32>,
}

/// A node of a [`Bvh`]: its box, and its leaf's items or its children.
#[derive(Debug, Clone, Copy)]
struct Node {
    min: Vec3,
    max: Vec3,
    /// A leaf's first item in [`Bvh::items`], or an inner node's second
    /// child; its first comes right after it.
    start: u32,
    /// A leaf's number of items, 0 for an inner node.
    count: u32,
}

impl Bvh {
    /// Over items `0..boxes.len()`, each in its box.
    fn new(boxes: Vec<[Vec3; 2]>) -> Self {
        let items = (0..boxes.len())
            .filter_map(|i| u32::try_from(i).ok())
            .collect();
        Self::with_items(boxes, items)
    }

    /// Over `items`, the `i`th in `boxes[i]`.
    fn with_items(boxes: Vec<[Vec3; 2]>, items: Vec<u32>) -> Self {
        // Each item with its box and middle, permuted in place as the tree
        // is built: sequential memory, not a lookup per comparison.
        let mut entries: Vec<Entry> = (boxes.iter().zip(&items))
            .map(|(&[min, max], &item)| Entry {
                min,
                max,
                middle: min + max,
                item,
            })
            .collect();
        let mut nodes = Vec::new();
        if !entries.is_empty() {
            build(&mut entries, 0, &mut nodes);
        }
        let items = entries.into_iter().map(|entry| entry.item).collect();
        Self { nodes, items }
    }

    /// The item `hit` says the ray meets first between `from` and `to`,
    /// and where: `hit` gives where along the ray an item is met, if it
    /// is.
    fn nearest(
        &self,
        ray: &Ray,
        from: f64,
        to: f64,
        mut hit: impl FnMut(u32) -> Option<f64>,
    ) -> Option<(f64, u32)> {
        let mut best: Option<(f64, u32)> = None;
        let mut stack = Vec::new();
        if !self.nodes.is_empty() {
            stack.push(0usize);
        }
        while let Some(index) = stack.pop() {
            let node = self.nodes[index];
            let limit = best.map_or(to, |(t, _)| t);
            let Some(_) = through_box(ray.origin, ray.direction, node.bounds(), from, limit) else {
                continue;
            };
            if node.count > 0 {
                for &item in self.leaf(node) {
                    if let Some(t) = hit(item).filter(|&t| t >= from && t <= to)
                        && best.is_none_or(|(b, i)| (t, item) < (b, i))
                    {
                        best = Some((t, item));
                    }
                }
                continue;
            }
            // The nearer child first, so the further is more often cut.
            let (first, second) = (index + 1, node.start as usize);
            let enter = |child: usize| {
                let bounds = self.nodes[child].bounds();
                through_box(ray.origin, ray.direction, bounds, from, limit).map(|(t, _)| t)
            };
            let (a, b) = (enter(first), enter(second));
            match (a, b) {
                (Some(a), Some(b)) if b < a => stack.extend([first, second]),
                _ => stack.extend([second, first]),
            }
        }
        best
    }

    /// Visits each item whose box, grown by `reach` of the box, the ray
    /// passes through from its `from` on: `visit` says nothing back that
    /// matters.
    fn near(
        &self,
        ray: &Ray,
        reach: impl Fn(DVec3, DVec3) -> f64,
        mut visit: impl FnMut(u32) -> Option<()>,
    ) {
        let mut stack = Vec::new();
        if !self.nodes.is_empty() {
            stack.push(0usize);
        }
        while let Some(index) = stack.pop() {
            let node = self.nodes[index];
            let [min, max] = node.bounds();
            let grow = DVec3::splat(reach(min, max));
            let grown = [min - grow, max + grow];
            if through_box(ray.origin, ray.direction, grown, ray.from, f64::INFINITY).is_none() {
                continue;
            }
            if node.count > 0 {
                for &item in self.leaf(node) {
                    let _ = visit(item);
                }
            } else {
                stack.extend([node.start as usize, index + 1]);
            }
        }
    }

    /// A leaf's items.
    fn leaf(&self, node: Node) -> &[u32] {
        let start = node.start as usize;
        self.items
            .get(start..start.saturating_add(node.count as usize))
            .unwrap_or(&[])
    }
}

impl Node {
    fn bounds(&self) -> [DVec3; 2] {
        [self.min.as_dvec3(), self.max.as_dvec3()]
    }
}

/// Moves the entries `left` says go left before the rest, in one pass
/// from both ends: how many go left.
fn partition(entries: &mut [Entry], left: impl Fn(&Entry) -> bool) -> usize {
    let (mut i, mut j) = (0, entries.len());
    loop {
        while i < j && left(&entries[i]) {
            i += 1;
        }
        while i < j && !left(&entries[j - 1]) {
            j -= 1;
        }
        if i >= j {
            return i;
        }
        entries.swap(i, j - 1);
    }
}

/// An item being built into a [`Bvh`]: its box, twice its middle, and
/// the item.
#[derive(Debug, Clone, Copy)]
struct Entry {
    min: Vec3,
    max: Vec3,
    middle: Vec3,
    item: u32,
}

/// Builds the nodes over `entries`, the items `offset..` of the whole,
/// onto `nodes`, reordering `entries` into the leaves' order. Ties along
/// the axis split by item, so the same items give the same tree.
fn build(entries: &mut [Entry], offset: usize, nodes: &mut Vec<Node>) {
    let mut bounds = [Vec3::INFINITY, Vec3::NEG_INFINITY];
    let mut middles = [Vec3::INFINITY, Vec3::NEG_INFINITY];
    for entry in entries.iter() {
        bounds = [bounds[0].min(entry.min), bounds[1].max(entry.max)];
        middles = [middles[0].min(entry.middle), middles[1].max(entry.middle)];
    }
    let index = nodes.len();
    // Items and nodes number fewer than the mesh's triangles, whose
    // indices are `u32`s.
    let at = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    nodes.push(Node {
        min: bounds[0],
        max: bounds[1],
        start: at(offset),
        count: at(entries.len()),
    });
    if entries.len() <= LEAF {
        return;
    }
    let axis = (middles[1] - middles[0]).max_position();
    // Split at the middle of the middles along the axis, one pass; where
    // that leaves a side with too few (bunched items), at the median.
    let split = (middles[0][axis] + middles[1][axis]) / 2.0;
    let mut half = partition(entries, |entry| entry.middle[axis] < split);
    if half < entries.len() / 4 || half > entries.len() - entries.len() / 4 {
        half = entries.len() / 2;
        entries.select_nth_unstable_by(half, |a, b| {
            (a.middle[axis].total_cmp(&b.middle[axis])).then(a.item.cmp(&b.item))
        });
    }
    let (left, right) = entries.split_at_mut(half);
    build(left, offset, nodes);
    let second = nodes.len();
    build(right, offset + half, nodes);
    nodes[index].start = at(second);
    nodes[index].count = 0;
}

#[cfg(test)]
pub(crate) mod tests;
