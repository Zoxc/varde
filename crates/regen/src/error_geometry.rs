//! A failure's geometry made drawable: what a feature that failed, or a
//! draft that did, shows of where it fails.
//!
//! The kernel's [`Failure`] carries its [`Evidence`]: patches, curves and
//! points by value, the sketch curves of the profile segments it is about
//! and the operand faces by name. Regen turns that into what the renderer
//! uploads as it does the model's ([`ErrorGeometry`]): the patches
//! tessellated as a solid's faces are ([`Display::sample_patch`]) into a
//! [`RenderMesh`] of their own, the curves and the boundary of the
//! patches (the sides no two of them share) flattened as a solid's edges
//! are ([`Display::flatten`]) into [`RenderLines`], the points as they
//! are; and the box around it all, for framing it. That is done once per
//! failure, which the cache keeps made ([`KernelFailure`]). The operand
//! faces are resolved through the answer's
//! [`Picking`] to the mesh faces of the body each operand is drawn as,
//! once the model is drawn ([`ErrorGeometry::resolve`]); an operand that
//! isn't a drawn body (a feature's tool solid) names none.
//!
//! It is bounded ([`ErrorGeometry::MAX_VERTICES`] and the others): past a
//! bound the rest is left out and [`ErrorGeometry::truncated`] says so,
//! as it does when the evidence was. What doesn't make drawable geometry
//! (a patch or curve the kernel's checks refuse, a point past
//! [`ErrorGeometry::MAX_POSITION`]) is left out the same way.

use std::fmt;
use std::sync::Arc;

use glam::{DVec3, Vec3};
use serde::{Deserialize, Serialize};
use varde_document::{BodyId, FeatureId};
use varde_kernel::mesh::FaceKey;
use varde_kernel::patch::{Bounds3, Conic3, Patch};
use varde_kernel::{
    Aabb, Display, Evidence, Failure, KernelError, LinesError, MAX_EVIDENCE, MeshError, MeshParts,
    Operand, RenderLines, RenderMesh, Tolerance,
};

use crate::Picking;
use crate::picking::bounded::seq;

/// A feature that failed: which, why in words for the Timeline, and what
/// to draw of where.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureFailure {
    pub feature: FeatureId,
    /// Why, worded from the kernel's error where it's the kernel's.
    pub message: String,
    /// What to draw, ready for the renderer; `None` where the failure has
    /// no evidence (every failure but the kernel's, for now) or none of
    /// it can be drawn.
    pub geometry: Option<Arc<ErrorGeometry>>,
}

/// For tests, which mostly check the words: the feature and the message,
/// the geometry aside.
#[cfg(test)]
impl PartialEq<(FeatureId, String)> for FeatureFailure {
    fn eq(&self, (feature, message): &(FeatureId, String)) -> bool {
        self.feature == *feature && self.message == *message
    }
}

/// A kernel failure as the cache keeps it: its error, its evidence made
/// drawable once ([`ErrorGeometry`]), and the operand faces the evidence
/// names, which are placed on bodies only where it is used
/// ([`KernelFailure::geometry`]): the same boolean may be of different
/// bodies.
#[derive(Debug)]
pub(crate) struct KernelFailure {
    pub(crate) error: KernelError,
    geometry: Option<Arc<ErrorGeometry>>,
    faces: Vec<(Operand, FaceKey)>,
    /// Whether the evidence left some out, kept apart for a failure
    /// whose only evidence is operand faces (no geometry made).
    truncated: bool,
}

impl KernelFailure {
    /// `failure`, its evidence made drawable at the [`Display`] of
    /// `tolerance`, as the model is.
    pub(crate) fn new(failure: Failure, tolerance: &Tolerance) -> KernelFailure {
        let display = Display::new(tolerance);
        KernelFailure {
            error: failure.error,
            geometry: ErrorGeometry::new(&failure.evidence, &display).map(Arc::new),
            truncated: failure.evidence.truncated,
            faces: failure.evidence.faces,
        }
    }

    /// Its geometry, the operand faces pending on the bodies `operands`
    /// are (`[a, b]`, `None` for an operand that isn't a body, such as a
    /// feature's tool) until [`ErrorGeometry::resolve`]; the very
    /// geometry kept where no face is pending.
    pub(crate) fn geometry(&self, operands: [Option<BodyId>; 2]) -> Option<Arc<ErrorGeometry>> {
        let pending: Vec<(BodyId, FaceKey)> = (self.faces.iter())
            .filter_map(|&(operand, key)| {
                let body = match operand {
                    Operand::A => operands[0],
                    Operand::B => operands[1],
                };
                Some((body?, key))
            })
            .collect();
        if pending.is_empty() {
            return self.geometry.clone();
        }
        let mut geometry = (self.geometry.as_deref().cloned()).unwrap_or_else(|| ErrorGeometry {
            truncated: self.truncated,
            ..ErrorGeometry::default()
        });
        geometry.pending = pending;
        Some(Arc::new(geometry))
    }

    /// About how many bytes it holds, for the cache.
    pub(crate) fn bytes(&self) -> usize {
        (size_of::<KernelFailure>())
            .saturating_add(size_of_val(&self.faces[..]))
            .saturating_add(self.geometry.as_deref().map_or(0, ErrorGeometry::bytes))
    }
}

/// A [`Failure`]'s [`Evidence`] made drawable: see the module docs.
///
/// Always drawable and within its bounds: the mesh has one part of one
/// face, or none, holding only triangles; every position, line point and
/// point is within [`ErrorGeometry::MAX_POSITION`]; each face named is a
/// face of the answer's mesh of the body named. The fields are private so
/// that holds; one from the other side of the web worker comes in through
/// [`ErrorGeometry::from_parts`], which checks it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ErrorGeometry {
    mesh: RenderMesh,
    lines: RenderLines,
    points: Vec<[f32; 3]>,
    bounds: Option<Aabb>,
    sketch_curves: Vec<u64>,
    faces: Vec<(BodyId, u32)>,
    truncated: bool,
    /// The operand faces not resolved yet, by the body each operand is
    /// and the face's name ([`ErrorGeometry::resolve`]).
    pending: Vec<(BodyId, FaceKey)>,
}

impl ErrorGeometry {
    /// The most vertices its mesh may have: thousands of patches as
    /// small as a failure's usually are.
    pub const MAX_VERTICES: usize = 1 << 18;
    /// The most triangle indices its mesh may have.
    pub const MAX_INDICES: usize = 3 << 19;
    /// The most points its lines may have.
    pub const MAX_LINE_POINTS: usize = 1 << 18;
    /// The most points it may have: as many as evidence holds.
    pub const MAX_POINTS: usize = MAX_EVIDENCE.points;
    /// The most sketch curves it may name: as many as evidence holds.
    pub const MAX_SKETCH_CURVES: usize = MAX_EVIDENCE.sketch_curves;
    /// The most mesh faces it may name: a face's name can stand for
    /// several (a face a groove cut in two).
    pub const MAX_FACES: usize = 4 * MAX_EVIDENCE.faces;
    /// The largest coordinate any of it may have: as far as a model's
    /// mesh reaches.
    pub const MAX_POSITION: f32 = RenderMesh::MAX_POSITION;

    /// `evidence` made drawable at `display`, or `None` if there's nothing
    /// to draw or name. Its operand faces are left to
    /// [`KernelFailure::geometry`], which knows the bodies the operands
    /// are.
    fn new(evidence: &Evidence, display: &Display) -> Option<ErrorGeometry> {
        if evidence.is_empty() {
            return None;
        }
        let mut geometry = ErrorGeometry {
            truncated: evidence.truncated,
            ..ErrorGeometry::default()
        };
        let diagonal = diagonal(evidence);
        geometry.add_patches(&evidence.patches, display, diagonal);
        let mut curves = evidence.curves.clone();
        curves.extend(boundary(&evidence.patches));
        geometry.add_curves(&curves, display, diagonal);
        for &point in &evidence.points {
            match placed(point) {
                Some(point) if geometry.points.len() < Self::MAX_POINTS => {
                    geometry.points.push(point)
                }
                _ => geometry.truncated = true,
            }
        }
        geometry.sketch_curves = (evidence.sketch_curves.iter().copied())
            .take(Self::MAX_SKETCH_CURVES)
            .collect();
        geometry.truncated |= evidence.sketch_curves.len() > Self::MAX_SKETCH_CURVES;
        geometry.bounds = geometry.bounds_in(None);
        (!geometry.is_empty()).then_some(geometry)
    }

    /// Adds `patches`' triangles to the mesh, each patch sampled on its
    /// own, until one doesn't fit; a patch that fails its check, or
    /// reaches past [`ErrorGeometry::MAX_POSITION`], is left out.
    fn add_patches(&mut self, patches: &[Patch], display: &Display, diagonal: f64) {
        let mut parts = MeshParts::default();
        for patch in patches {
            if patch.check().is_err() {
                self.truncated = true;
                continue;
            }
            let samples = display.sample_patch(patch, diagonal);
            let positions: Option<Vec<[f32; 3]>> =
                samples.points.iter().map(|&p| placed(p)).collect();
            let base = u32::try_from(parts.positions.len()).ok();
            let fits = (parts.positions.len()).saturating_add(samples.points.len())
                <= Self::MAX_VERTICES
                && (parts.indices.len()).saturating_add(samples.indices.len()) <= Self::MAX_INDICES;
            if !fits {
                self.truncated = true;
                break;
            }
            let (Some(positions), Some(base)) = (positions, base) else {
                self.truncated = true;
                continue;
            };
            parts.positions.extend(positions);
            parts
                .normals
                .extend((samples.normals.iter()).map(|n| n.as_vec3().to_array()));
            // Within `MAX_VERTICES`, checked above.
            parts
                .indices
                .extend(samples.indices.iter().map(|&v| base + v));
        }
        match mesh_of(parts.positions, parts.normals, parts.indices) {
            Ok(mesh) => self.mesh = mesh,
            // Every part is whole and within bounds, so this isn't
            // reached; nothing is drawn of the patches if it is.
            Err(_) => self.truncated = true,
        }
    }

    /// Adds `curves` to the lines, flattened, until one doesn't fit; a
    /// curve that fails its check, or reaches past
    /// [`ErrorGeometry::MAX_POSITION`], is left out.
    fn add_curves(&mut self, curves: &[Conic3], display: &Display, diagonal: f64) {
        for curve in curves {
            if curve.check().is_err() {
                self.truncated = true;
                continue;
            }
            let points = display.flatten(curve, diagonal);
            let placed: Option<Vec<Vec3>> = (points.iter())
                .map(|&p| placed(p).map(Vec3::from))
                .collect();
            if (self.lines.points().len()).saturating_add(points.len()) > Self::MAX_LINE_POINTS {
                self.truncated = true;
                break;
            }
            let pushed = placed.is_some_and(|placed| self.lines.push(placed).is_ok());
            self.truncated |= !pushed;
        }
    }

    /// Resolves the operand faces it names to the faces of `mesh` (the
    /// model's, as answered) of the body each operand is drawn as, by
    /// `picking` (`mesh`'s tables): the face's name is a face's key or
    /// one of its aliases, on the body `holder` gives for the operand's
    /// (the body holding it now, `None` for one with no solid). Faces of
    /// bodies not shown name none. Those faces' triangles, as the model
    /// draws them, join its mesh, so they are drawn as its patches are,
    /// and the box holds them too.
    pub(crate) fn resolve(
        &mut self,
        mesh: &RenderMesh,
        picking: &Picking,
        holder: impl Fn(BodyId) -> Option<BodyId>,
    ) {
        let mut wanted: Vec<(BodyId, FaceKey)> = std::mem::take(&mut self.pending)
            .into_iter()
            .filter_map(|(body, key)| Some((holder(body)?, key)))
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
        if wanted.is_empty() {
            return;
        }
        let names = |body: BodyId, key: &FaceKey| wanted.binary_search(&(body, *key)).is_ok();
        let first = self.faces.len();
        'parts: for (part, &body) in mesh.parts().zip(picking.bodies()) {
            if !(wanted.iter()).any(|&(wanted, _)| wanted == body) {
                continue;
            }
            for f in part.faces {
                let face = &picking.faces()[f];
                if !(names(body, &face.key) || face.aliases.iter().any(|a| names(body, a))) {
                    continue;
                }
                if self.faces.len() >= Self::MAX_FACES {
                    self.truncated = true;
                    break 'parts;
                }
                // A mesh's face ids fit `u32`.
                self.faces.push((body, f as u32));
            }
        }
        self.add_model_faces(mesh, first);
        self.bounds = self.bounds_in(Some(mesh));
    }

    /// Adds the triangles of the faces it names in `model` (the answer's
    /// mesh), from the `first`, to its mesh, their vertices and normals as
    /// the model has them, and each face's outline to its lines
    /// ([`outline`]), as a patch's boundary is: face by face, each whole
    /// (its triangles and its outline), until one doesn't fit.
    fn add_model_faces(&mut self, model: &RenderMesh, first: usize) {
        let Some(faces) = self.faces.get(first..).filter(|f| !f.is_empty()) else {
            return;
        };
        let mut positions = self.mesh.positions().to_vec();
        let mut normals = self.mesh.normals().to_vec();
        let mut indices = self.mesh.indices().to_vec();
        for &(_, face) in faces {
            let Some(range) = model.face_indices(face as usize) else {
                continue;
            };
            let corners = &model.indices()[range];
            // Its vertices, numbered after those there.
            let mut ids: std::collections::BTreeMap<u32, u32> = Default::default();
            let mut added = Vec::new();
            for &v in corners {
                let next = positions.len().saturating_add(added.len());
                if let std::collections::btree_map::Entry::Vacant(e) = ids.entry(v) {
                    // Within `MAX_VERTICES` once checked below, so it fits.
                    e.insert(u32::try_from(next).unwrap_or(u32::MAX));
                    added.push(v);
                }
            }
            let lines = outline(model.positions(), corners);
            let points = (lines.iter()).fold(0usize, |sum, line| sum.saturating_add(line.len()));
            let fits = positions.len().saturating_add(added.len()) <= Self::MAX_VERTICES
                && indices.len().saturating_add(corners.len()) <= Self::MAX_INDICES
                && self.lines.points().len().saturating_add(points) <= Self::MAX_LINE_POINTS;
            if !fits {
                self.truncated = true;
                break;
            }
            for v in added {
                positions.push(model.positions()[v as usize]);
                normals.push(model.normals()[v as usize]);
            }
            indices.extend(corners.iter().map(|v| ids[v]));
            for line in lines {
                // Within `MAX_POSITION`, as the model's positions are, and
                // within `MAX_LINE_POINTS`, checked above.
                self.truncated |= self.lines.push(line).is_err();
            }
        }
        match mesh_of(positions, normals, indices) {
            Ok(mesh) => self.mesh = mesh,
            // The model's triangles are whole and within bounds, as are
            // its own, so this isn't reached; its mesh stays as it was.
            Err(_) => self.truncated = true,
        }
    }

    /// [`ErrorGeometry::resolve`] on a shared `geometry`, taking a copy of
    /// its own only if it has operand faces pending, so geometry with
    /// none (a feature's tool's, or a failure the cache keeps without
    /// faces) stays the very one kept from answer to answer; geometry
    /// that then draws and names nothing is dropped.
    pub(crate) fn resolve_shared(
        geometry: &mut Option<Arc<ErrorGeometry>>,
        mesh: &RenderMesh,
        picking: &Picking,
        holder: impl Fn(BodyId) -> Option<BodyId>,
    ) {
        if let Some(shared) = geometry
            && !shared.pending.is_empty()
        {
            Arc::make_mut(shared).resolve(mesh, picking, holder);
        }
        if geometry.as_deref().is_some_and(ErrorGeometry::is_empty) {
            *geometry = None;
        }
    }

    /// Whether it draws and names nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.mesh.triangle_count() == 0
            && self.lines.points().is_empty()
            && self.points.is_empty()
            && self.sketch_curves.is_empty()
            && self.faces.is_empty()
            && self.pending.is_empty()
    }

    /// About how many bytes it holds, for the cache.
    pub(crate) fn bytes(&self) -> usize {
        let mesh = &self.mesh;
        (size_of::<ErrorGeometry>())
            .saturating_add(size_of_val(mesh.positions()))
            .saturating_add(size_of_val(mesh.normals()))
            .saturating_add(size_of_val(mesh.indices()))
            .saturating_add(size_of_val(self.lines.points()))
            .saturating_add(size_of_val(self.lines.ends()))
            .saturating_add(size_of_val(&self.points[..]))
            .saturating_add(size_of_val(&self.sketch_curves[..]))
            .saturating_add(size_of_val(&self.faces[..]))
            .saturating_add(size_of_val(&self.pending[..]))
    }

    /// The box around its mesh, lines and points, and its faces in
    /// `model` (the answer's mesh) if given.
    fn bounds_in(&self, model: Option<&RenderMesh>) -> Option<Aabb> {
        let mut bounds: Option<Aabb> = None;
        let mut take = |p: [f32; 3]| {
            let p = Vec3::from(p);
            bounds = Some(bounds.map_or(Aabb { min: p, max: p }, |b| Aabb {
                min: b.min.min(p),
                max: b.max.max(p),
            }));
        };
        // The triangles' corners: positions no triangle uses (which
        // parts from the other side may hold) aren't drawn.
        let positions = self.mesh.positions();
        (self.mesh.indices().iter()).for_each(|&v| take(positions[v as usize]));
        self.lines.points().iter().copied().for_each(&mut take);
        self.points.iter().copied().for_each(&mut take);
        if let Some(model) = model {
            for &(_, face) in &self.faces {
                let Some(range) = model.face_indices(face as usize) else {
                    continue;
                };
                for &v in &model.indices()[range] {
                    take(model.positions()[v as usize]);
                }
            }
        }
        bounds
    }

    /// The patches, tessellated: one part of one face, or no part if
    /// there are none. Its triangles are all there is: no edges, corners
    /// or wires (the patches' boundaries are in [`ErrorGeometry::lines`]).
    pub fn mesh(&self) -> &RenderMesh {
        &self.mesh
    }

    /// The curves, then the patches' boundary, flattened; then, once
    /// resolved, the operand faces' outlines.
    pub fn lines(&self) -> &RenderLines {
        &self.lines
    }

    /// The points: a pinch, a cusp, a gap's ends.
    pub fn points(&self) -> &[[f32; 3]] {
        &self.points
    }

    /// The box around all of it (its faces in the answer's mesh too),
    /// for framing it; `None` if it draws nothing, only names sketch
    /// curves.
    pub fn bounds(&self) -> Option<Aabb> {
        self.bounds
    }

    /// The sketch curves of the profile segments it is about
    /// ([`Segment::curve`](varde_kernel::Segment::curve)), for the sketch
    /// editor to mark. Only marks: an id naming no curve marks nothing.
    pub fn sketch_curves(&self) -> &[u64] {
        &self.sketch_curves
    }

    /// The operand faces it is about, as the body and the face of the
    /// answer's mesh ([`Picking`]'s face ids). Resolved once the model
    /// is drawn: empty in an [`Evaluation`](crate::Evaluation) on its
    /// own.
    pub fn faces(&self) -> &[(BodyId, u32)] {
        &self.faces
    }

    /// Whether some of the evidence was left out: by the kernel, or here,
    /// past a bound or not drawable.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// What crosses to the page, see [`ErrorGeometry::from_parts`].
    pub fn to_parts(&self) -> GeometryParts {
        GeometryParts {
            positions: self.mesh.positions().to_vec(),
            normals: self.mesh.normals().to_vec(),
            indices: self.mesh.indices().to_vec(),
            line_points: self.lines.points().to_vec(),
            line_ends: self.lines.ends().to_vec(),
            points: self.points.clone(),
            sketch_curves: self.sketch_curves.clone(),
            faces: self.faces.clone(),
            truncated: self.truncated,
        }
    }

    /// The geometry of these parts, if they make one with `mesh` (the
    /// answer's) and `picking` (its tables): every count within its
    /// bound, the triangles whole and their indices in range, every
    /// coordinate finite and within [`ErrorGeometry::MAX_POSITION`],
    /// every normal finite, each polyline of two points or more, each
    /// face one of `mesh`'s of the body named. Its box is worked out
    /// again here.
    pub fn from_parts(
        parts: GeometryParts,
        mesh: &RenderMesh,
        picking: &Picking,
    ) -> Result<ErrorGeometry, GeometryError> {
        let GeometryParts {
            positions,
            normals,
            indices,
            line_points,
            line_ends,
            points,
            sketch_curves,
            faces,
            truncated,
        } = parts;
        if positions.len() > Self::MAX_VERTICES
            || indices.len() > Self::MAX_INDICES
            || line_points.len() > Self::MAX_LINE_POINTS
            || points.len() > Self::MAX_POINTS
            || sketch_curves.len() > Self::MAX_SKETCH_CURVES
            || faces.len() > Self::MAX_FACES
        {
            return Err(GeometryError::TooLarge);
        }
        let within = |p: &[f32; 3]| p.iter().all(|x| x.abs() <= Self::MAX_POSITION);
        if !(positions.iter().chain(&line_points).chain(&points)).all(within) {
            return Err(GeometryError::Position);
        }
        let error_mesh = mesh_of(positions, normals, indices).map_err(GeometryError::Mesh)?;
        let lines =
            RenderLines::from_parts(line_points, line_ends).map_err(GeometryError::Lines)?;
        let face_ok = |&(body, face): &(BodyId, u32)| {
            (face as usize) < mesh.face_count() && picking.face_body(mesh, face) == Some(body)
        };
        if !faces.iter().all(face_ok) {
            return Err(GeometryError::Face);
        }
        let mut geometry = ErrorGeometry {
            mesh: error_mesh,
            lines,
            points,
            bounds: None,
            sketch_curves,
            faces,
            truncated,
            pending: Vec::new(),
        };
        geometry.bounds = geometry.bounds_in(Some(mesh));
        Ok(geometry)
    }
}

/// An [`ErrorGeometry`] as it crosses the web worker's boundary, decoded
/// within its bounds (refused as soon as a sequence is past one) and
/// checked by [`ErrorGeometry::from_parts`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GeometryParts {
    #[serde(deserialize_with = "bounded::vertices")]
    pub positions: Vec<[f32; 3]>,
    #[serde(deserialize_with = "bounded::vertices")]
    pub normals: Vec<[f32; 3]>,
    #[serde(deserialize_with = "bounded::indices")]
    pub indices: Vec<u32>,
    #[serde(deserialize_with = "bounded::line_points")]
    pub line_points: Vec<[f32; 3]>,
    #[serde(deserialize_with = "bounded::line_points")]
    pub line_ends: Vec<u32>,
    #[serde(deserialize_with = "bounded::points")]
    pub points: Vec<[f32; 3]>,
    #[serde(deserialize_with = "bounded::sketch_curves")]
    pub sketch_curves: Vec<u64>,
    #[serde(deserialize_with = "bounded::faces")]
    pub faces: Vec<(BodyId, u32)>,
    pub truncated: bool,
}

/// The bounded decoding of [`GeometryParts`].
mod bounded {
    use serde::{Deserialize, Deserializer};

    use super::{ErrorGeometry, seq};

    fn at_most<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        d: D,
        max: usize,
    ) -> Result<Vec<T>, D::Error> {
        seq(d, max, |_| 0, 0)
    }

    pub(super) fn vertices<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<[f32; 3]>, D::Error> {
        at_most(d, ErrorGeometry::MAX_VERTICES)
    }

    pub(super) fn indices<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u32>, D::Error> {
        at_most(d, ErrorGeometry::MAX_INDICES)
    }

    pub(super) fn line_points<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        d: D,
    ) -> Result<Vec<T>, D::Error> {
        at_most(d, ErrorGeometry::MAX_LINE_POINTS)
    }

    pub(super) fn points<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<[f32; 3]>, D::Error> {
        at_most(d, ErrorGeometry::MAX_POINTS)
    }

    pub(super) fn sketch_curves<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u64>, D::Error> {
        at_most(d, ErrorGeometry::MAX_SKETCH_CURVES)
    }

    pub(super) fn faces<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<(varde_document::BodyId, u32)>, D::Error> {
        at_most(d, ErrorGeometry::MAX_FACES)
    }
}

/// Why parts don't make an [`ErrorGeometry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryError {
    /// A count is past its bound.
    TooLarge,
    /// A coordinate isn't finite or is past
    /// [`ErrorGeometry::MAX_POSITION`].
    Position,
    /// The triangles don't make a mesh.
    Mesh(MeshError),
    /// The lines' points and ends don't make lines.
    Lines(LinesError),
    /// A face isn't one of the answer's mesh, or not of the body named.
    Face,
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeometryError::TooLarge => f.write_str("a failure's geometry is too large"),
            GeometryError::Position => f.write_str("a failure's geometry is out of bounds"),
            GeometryError::Mesh(e) => write!(f, "a failure's geometry: {e}"),
            GeometryError::Lines(e) => write!(f, "a failure's geometry: {e}"),
            GeometryError::Face => f.write_str("a failure's face isn't one of the model"),
        }
    }
}

impl std::error::Error for GeometryError {}

/// The mesh of these triangles: one part of one face, or none without
/// triangles.
fn mesh_of(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
) -> Result<RenderMesh, MeshError> {
    let faces = u32::try_from(indices.len()).map_err(|_| MeshError::TooLarge)?;
    let (face_ends, part_ends) = if indices.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        (vec![faces], vec![[1, 0, 0, 0]])
    };
    RenderMesh::from_parts(MeshParts {
        positions,
        normals,
        indices,
        face_ends,
        part_ends,
        ..MeshParts::default()
    })
}

/// `p` as drawn, if it's within [`ErrorGeometry::MAX_POSITION`].
fn placed(p: DVec3) -> Option<[f32; 3]> {
    let p = p.as_vec3();
    (p.is_finite() && p.abs().max_element() <= ErrorGeometry::MAX_POSITION).then(|| p.to_array())
}

/// The sides of `patches` that aren't shared with another of them, in
/// order: the boundary of the region they make, so a shell's or a
/// neighbourhood's inside isn't drawn as lines. Two patches share a side
/// where one's runs back along the other's (a mesh's neighbours hold
/// the same corners, control point and weight); patches failing their
/// check are left out, as they aren't drawn.
fn boundary(patches: &[Patch]) -> Vec<Conic3> {
    // A side's bits, `-0.0` taken as `0.0`.
    let bits = |p: DVec3| (p + DVec3::ZERO).to_array().map(f64::to_bits);
    let key = |c: &Conic3| (bits(c.p0), bits(c.p1), bits(c.c), (c.w + 0.0).to_bits());
    let sides: Vec<Conic3> = (patches.iter())
        .filter(|patch| patch.check().is_ok())
        .flat_map(|patch| [0, 1, 2].map(|i| patch.edge(i)))
        .collect();
    let mut keys: Vec<_> = sides.iter().map(key).collect();
    keys.sort_unstable();
    let back = |c: &Conic3| Conic3 {
        p0: c.p1,
        p1: c.p0,
        ..*c
    };
    // A side drawn once, however many patches hold it the same way
    // round (a patch given twice).
    let mut drawn = Vec::new();
    (sides.iter())
        .filter(|side| keys.binary_search(&key(&back(side))).is_err())
        .filter(|side| {
            let at = drawn.partition_point(|k| *k < key(side));
            let new = drawn.get(at) != Some(&key(side));
            if new {
                drawn.insert(at, key(side));
            }
            new
        })
        .copied()
        .collect()
}

/// The outline of a face of the model drawn as the triangles `corners`
/// (indices into `positions`): the sides no other of its triangles runs
/// back along, by position (so a seam's vertices at one place are one),
/// each once, joined end to end into polylines, a loop closed where it
/// comes round, in the triangles' order. A triangle with two corners at
/// one place is left out: its sides run there and back, and would hide
/// a side of the outline they lie along.
fn outline(positions: &[[f32; 3]], corners: &[u32]) -> Vec<Vec<Vec3>> {
    // A position's bits, `-0.0` taken as `0.0`.
    let key = |v: u32| positions[v as usize].map(|x| (x + 0.0).to_bits());
    let sides: Vec<([u32; 3], [u32; 3])> = (corners.as_chunks::<3>().0.iter())
        .map(|t| t.map(key))
        .filter(|[a, b, c]| a != b && b != c && c != a)
        .flat_map(|[a, b, c]| [(a, b), (b, c), (c, a)])
        .collect();
    let mut sorted = sides.clone();
    sorted.sort_unstable();
    let mut open: Vec<([u32; 3], [u32; 3])> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for &(p, q) in &sides {
        if sorted.binary_search(&(q, p)).is_err() && seen.insert((p, q)) {
            open.push((p, q));
        }
    }
    // The sides by where they start, to follow each to the next.
    let mut starts: Vec<([u32; 3], usize)> = (open.iter().enumerate())
        .map(|(i, &(p, _))| (p, i))
        .collect();
    starts.sort_unstable();
    let mut used = vec![false; open.len()];
    let point = |k: [u32; 3]| Vec3::from(k.map(f32::from_bits));
    let mut lines = Vec::new();
    for i in 0..open.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let (start, mut end) = open[i];
        let mut line = vec![point(start), point(end)];
        while end != start {
            let from = starts.partition_point(|&(k, _)| k < end);
            let next = (starts[from..].iter())
                .take_while(|&&(k, _)| k == end)
                .find(|&&(_, j)| !used[j]);
            let Some(&(_, j)) = next else {
                break;
            };
            used[j] = true;
            end = open[j].1;
            line.push(point(end));
        }
        lines.push(line);
    }
    lines
}

/// The diagonal of the box around `evidence`'s patches, curves and points
/// (their control points'), what they are flattened relative to, as a
/// solid's edges are to its box; 0 if there's none or it isn't finite
/// (the chord is then the fit tolerance). Only what may be drawn counts:
/// a patch or curve its check refuses, or a point past
/// [`ErrorGeometry::MAX_POSITION`], would coarsen the rest.
fn diagonal(evidence: &Evidence) -> f64 {
    let patches = (evidence.patches.iter()).filter(|patch| patch.check().is_ok());
    let curves = (evidence.curves.iter()).filter(|curve| curve.check().is_ok());
    let points = (evidence.points.iter()).filter(|&&p| placed(p).is_some());
    let boxes = (patches.map(Patch::bounds))
        .chain(curves.map(Conic3::bounds))
        .chain(points.map(|&p| Bounds3::point(p)));
    let Some(bounds) = boxes.reduce(Bounds3::union) else {
        return 0.0;
    };
    let diagonal = (bounds.max - bounds.min).length();
    if diagonal.is_finite() { diagonal } else { 0.0 }
}

#[cfg(test)]
mod tests;
