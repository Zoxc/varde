//! The picking tables a model's answer carries: which body each part of
//! its mesh is of, and what the viewport says about and builds on each
//! face and edge of the mesh.
//!
//! The mesh's faces are the kernel's regions
//! ([`Topology::regions`](varde_kernel_compat::Topology::regions)): connected
//! triangles of one [`FaceKey`], so a circle's quarter walls, or flush
//! faces merged under one name, are one face, and a face cut in two by a
//! groove is two of one key. Its first edges in each part are the
//! chains ([`Topology::chains`](varde_kernel_compat::Topology::chains)): maximal
//! runs of mesh edges between the same two faces; the rest are creases
//! inside one face (see [`RenderMesh`]). So the tables here are indexed
//! by the mesh's own face and edge ids, and which face a triangle is on
//! and which faces an edge is between are the mesh's to say. Each body's
//! tables are worked out with its mesh ([`Drawn`]) and the scene's are
//! theirs joined, in the shown bodies' order, as the mesh is.
//!
//! What a [`Picking`] guarantees is on it.

use std::mem::size_of_val;
use std::sync::Arc;

use crate::PickTables;
use crate::bounded_seq;
use crate::bounds::{point, point_dvec, positive, ratio, unit, value};
use crate::cache::size::{HeapSize, total};
use glam::DVec3;
use serde::{Deserialize, Deserializer, Serialize};
use varde_document::BodyId;
use varde_kernel_compat::measure::{EdgeShape, edge_shape};
use varde_kernel_compat::mesh::{FaceKey, Form};
use varde_kernel_compat::topology::Region;
use varde_kernel_compat::{Display, MeshError, RenderMesh, Solid, Topology};

/// What a face is, as far as the viewport needs to know: a plane's
/// outward unit normal `n` and offset `d` (the plane `n·x = d`), from
/// which a sketch's placement on it is worked out; a round face's axis
/// and size; for the other known surfaces (a conic cylinder, a revolved
/// conic) which they are and their direction or axis. Faces of no known
/// form, or with numbers past [`Picking::MAX_VALUE`], are
/// [`Summary::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Summary {
    Plane {
        n: [f64; 3],
        d: f64,
    },
    /// A circular cylinder of `radius` around the line through `point`
    /// along the unit `axis`.
    Cylinder {
        point: [f64; 3],
        axis: [f64; 3],
        radius: f64,
    },
    /// A circular cone with its apex at `apex`, opening along the unit
    /// `axis`, its half-angle's cosine and sine.
    Cone {
        apex: [f64; 3],
        axis: [f64; 3],
        cos: f64,
        sin: f64,
    },
    Sphere {
        centre: [f64; 3],
        radius: f64,
    },
    /// A torus around the line through `centre` along the unit `axis`.
    Torus {
        centre: [f64; 3],
        axis: [f64; 3],
        major: f64,
        minor: f64,
    },
    /// A cylinder over a conic that isn't a circle (an ellipse, parabola
    /// or hyperbola arc), along the unit `along`.
    ConicCylinder {
        along: [f64; 3],
    },
    /// A conic turned about the line through `origin` along the unit
    /// `axis`.
    Revolved {
        origin: [f64; 3],
        axis: [f64; 3],
    },
    /// No known surface.
    Other,
}

impl Summary {
    /// Whether this is a surface of revolution with an axis: a cylinder,
    /// cone, torus or turned conic (not a sphere, whose every diameter is
    /// one).
    pub fn has_axis(&self) -> bool {
        matches!(
            self,
            Summary::Cylinder { .. }
                | Summary::Cone { .. }
                | Summary::Torus { .. }
                | Summary::Revolved { .. }
        )
    }

    /// The summary of a face of form `form`: [`Summary::Other`] for a form
    /// it has no variant for, or whose numbers wouldn't pass
    /// [`Summary::valid`].
    pub fn of(form: &Form) -> Summary {
        let a = DVec3::to_array;
        let summary = match *form {
            Form::Plane { n, d } => Summary::Plane { n: a(&n), d },
            Form::Cylinder {
                point,
                axis,
                radius,
            } => Summary::Cylinder {
                point: a(&point),
                axis: a(&axis),
                radius,
            },
            Form::Cone {
                apex,
                axis,
                cos,
                sin,
            } => Summary::Cone {
                apex: a(&apex),
                axis: a(&axis),
                cos,
                sin,
            },
            Form::Sphere { centre, radius } => Summary::Sphere {
                centre: a(&centre),
                radius,
            },
            Form::Torus {
                centre,
                axis,
                major,
                minor,
            } => Summary::Torus {
                centre: a(&centre),
                axis: a(&axis),
                major,
                minor,
            },
            Form::ConicCylinder { along, .. } => Summary::ConicCylinder { along: a(&along) },
            Form::Revolved { origin, axis, .. } => Summary::Revolved {
                origin: a(&origin),
                axis: a(&axis),
            },
            Form::Quadric(_) | Form::Unknown => Summary::Other,
        };
        if summary.valid() {
            summary
        } else {
            Summary::Other
        }
    }

    /// Whether its numbers are finite and within [`Picking::MAX_VALUE`],
    /// its directions unit vectors (within [`Picking::UNIT`]), its sizes
    /// positive and a cone's cosine and sine within `(0, 1]`.
    pub fn valid(&self) -> bool {
        match *self {
            Summary::Plane { n, d } => unit(n) && value(d),
            Summary::Cylinder {
                point: p,
                axis,
                radius,
            } => point(p) && unit(axis) && positive(radius),
            Summary::Cone {
                apex,
                axis,
                cos,
                sin,
            } => point(apex) && unit(axis) && ratio(cos) && ratio(sin),
            Summary::Sphere { centre, radius } => point(centre) && positive(radius),
            Summary::Torus {
                centre,
                axis,
                major,
                minor,
            } => point(centre) && unit(axis) && positive(major) && positive(minor),
            Summary::ConicCylinder { along } => unit(along),
            Summary::Revolved { origin, axis } => point(origin) && unit(axis),
            Summary::Other => true,
        }
    }
}

/// A face of the model: a region of a body's solid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickFace {
    /// Its name, which references store.
    pub key: FaceKey,
    /// The keys merged into it, which name it too, sorted, without `key`.
    /// At most [`Picking::MAX_ALIASES`] are decoded.
    #[serde(deserialize_with = "aliases")]
    pub aliases: Vec<FaceKey>,
    pub summary: Summary,
}

impl PickFace {
    /// Whether `key` names it, as its key or one of its aliases.
    pub fn named(&self, key: &FaceKey) -> bool {
        self.key == *key || self.aliases.binary_search(key).is_ok()
    }
}

/// A corner of the model: a vertex of a body's solid where three or more
/// of its faces meet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PickCorner {
    /// Three of the faces meeting there, the lowest regions, ascending:
    /// face ids of the mesh, of one part. Where more meet, the three
    /// still name it (with the point, among several of the same keys), as
    /// a corner reference does.
    pub faces: [u32; 3],
    /// Where it is, exactly the solid's vertex, within
    /// [`Picking::MAX_VALUE`].
    pub point: [f64; 3],
}

/// The picking tables of a model's mesh: the body of each of its parts,
/// and its faces' keys and summaries, which of its edges close on
/// themselves, which tangent chain each is in and its snap point, by the
/// mesh's face and edge ids, and its corners. What each edge is (its
/// form, ends and convexity) isn't here: it's asked for apart, see
/// [`EdgeFacts`](crate::EdgeFacts).
///
/// It's always consistent with the mesh it came with: one body per part,
/// one face per face of the mesh, one flag, one tangent chain and one
/// snap point or none per edge, a closed edge
/// between two different faces and starting and ending at one corner,
/// each edge's tangent chain's first an edge of its part between two
/// different faces, no later than it and its own first (a crease's
/// itself), a crease without a snap point, snap points
/// within [`Picking::MAX_VALUE`], each
/// corner between three faces of one part in ascending order with its
/// point within [`Picking::MAX_VALUE`], no more corners than vertices,
/// every summary [`Summary::valid`], each face's aliases sorted and apart
/// from its key.
/// The fields are private so that holds;
/// one from the other side of the web worker comes in through
/// [`Picking::from_parts`], which checks it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Picking {
    bodies: Vec<BodyId>,
    tables: FaceEdgeTables,
}

/// The parts of [`Picking`], as [`Picking::from_parts`] takes them and
/// [`Picking::to_parts`] gives them: see their accessors on [`Picking`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PickingParts {
    pub bodies: Vec<BodyId>,
    pub tables: FaceEdgeTables,
}

/// The tables by the mesh's face and edge ids, and its corners: a
/// [`Picking`]'s besides its bodies, and one body's drawing's
/// (`Drawn`). See their accessors on [`Picking`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FaceEdgeTables {
    /// Per face (region): its key, aliases and summary.
    pub faces: Vec<PickFace>,
    /// Per edge: whether it's a closed chain.
    pub closed: Vec<bool>,
    /// Per edge: its tangent chain's first edge, a crease itself.
    pub tangents: Vec<u32>,
    /// Per edge: its snap point, a crease none.
    pub snaps: Vec<Option<[f64; 3]>>,
    /// Per corner: three of its faces.
    pub corners: Vec<PickCorner>,
}

impl FaceEdgeTables {
    /// Appends `other`'s, its edge and face ids moved past these. Fails
    /// with [`MeshError::TooLarge`] where an id wouldn't fit.
    fn append(&mut self, other: &FaceEdgeTables) -> Result<(), MeshError> {
        let base = u32::try_from(self.tangents.len()).map_err(|_| MeshError::TooLarge)?;
        let mut tangents = Vec::with_capacity(other.tangents.len());
        for &first in &other.tangents {
            tangents.push(first.checked_add(base).ok_or(MeshError::TooLarge)?);
        }
        let faces = u32::try_from(self.faces.len()).map_err(|_| MeshError::TooLarge)?;
        let moved = |f: u32| f.checked_add(faces).ok_or(MeshError::TooLarge);
        let mut corners = Vec::with_capacity(other.corners.len());
        for corner in &other.corners {
            let [a, b, c] = corner.faces;
            corners.push(PickCorner {
                faces: [moved(a)?, moved(b)?, moved(c)?],
                point: corner.point,
            });
        }
        self.faces.extend_from_slice(&other.faces);
        self.closed.extend_from_slice(&other.closed);
        self.tangents.extend(tangents);
        self.snaps.extend_from_slice(&other.snaps);
        self.corners.extend(corners);
        Ok(())
    }
}

impl Picking {
    /// The largest number a summary may hold: past any plane's offset
    /// or point a solid within the kernel's limits has.
    pub const MAX_VALUE: f64 = 1e8;
    /// How far from 1 a summary's direction's length may be: a unit
    /// vector rounded is far nearer.
    pub const UNIT: f64 = 1e-9;
    /// The most aliases all of the faces in a reply from the web worker
    /// may have together, and so one face's (a key is 32 bytes on the
    /// page and as few as 3 in the reply). A model with more is answered
    /// as failed.
    pub const MAX_ALIASES: usize = 1 << 20;

    /// The tables of `mesh` of these parts, if they make them; see
    /// [`Picking`] for what is checked.
    pub fn from_parts(parts: PickingParts, mesh: &RenderMesh) -> Result<Picking, PickingError> {
        let PickingParts { bodies, tables } = parts;
        let FaceEdgeTables {
            faces,
            closed,
            tangents,
            snaps,
            corners,
        } = &tables;
        if bodies.len() != mesh.part_ends().len()
            || faces.len() != mesh.face_count()
            || closed.len() != mesh.edge_count()
            || tangents.len() != mesh.edge_count()
            || snaps.len() != mesh.edge_count()
        {
            return Err(PickingError::Lengths);
        }
        // Every corner is a vertex of a triangle drawn.
        if corners.len() > mesh.positions().len() {
            return Err(PickingError::Corner);
        }
        let face_ok = |face: &PickFace| {
            face.summary.valid()
                && face.aliases.windows(2).all(|pair| pair[0] < pair[1])
                && !face.aliases.contains(&face.key)
        };
        if !faces.iter().all(face_ok) {
            return Err(PickingError::Face);
        }
        let closes = |(&[a, b], &[start, end]): (&[u32; 2], &[u32; 2])| a != b && start == end;
        let ends = mesh.edge_faces().iter().zip(mesh.edge_corners());
        if !(closed.iter().zip(ends)).all(|(&closed, edge)| !closed || closes(edge)) {
            return Err(PickingError::Closed);
        }
        // Each part's first edge, by edge.
        let mut part_start = 0;
        let mut part_ends = mesh.part_ends().iter();
        let mut part_end = part_ends.next().map_or(0, |&[_, edges, _, _]| edges);
        let chain = |e: u32| mesh.chain_faces(e as usize).is_some();
        for (e, &first) in tangents.iter().enumerate() {
            let e = e as u32;
            while e >= part_end {
                part_start = part_end;
                part_end = part_ends.next().map_or(u32::MAX, |&[_, edges, _, _]| edges);
            }
            let ok = if chain(e) {
                (part_start..=e).contains(&first)
                    && tangents[first as usize] == first
                    && chain(first)
            } else {
                first == e
            };
            if !ok {
                return Err(PickingError::Tangent);
            }
        }
        let snap_ok = |(e, snap): (usize, &Option<[f64; 3]>)| {
            snap.is_none_or(|p| chain(e as u32) && point(p))
        };
        if !snaps.iter().enumerate().all(snap_ok) {
            return Err(PickingError::Snap);
        }
        let part = |f: u32| mesh.part_of_face(f);
        let corner_ok = |corner: &PickCorner| {
            let [a, b, c] = corner.faces;
            a < b
                && b < c
                && part(c).is_some()
                && part(a) == part(b)
                && part(b) == part(c)
                && point(corner.point)
        };
        if !corners.iter().all(corner_ok) {
            return Err(PickingError::Corner);
        }
        Ok(Picking { bodies, tables })
    }

    /// Its parts, copied, as [`Picking::from_parts`] takes them.
    pub fn to_parts(&self) -> PickingParts {
        PickingParts {
            bodies: self.bodies.clone(),
            tables: self.tables.clone(),
        }
    }

    /// The body of each of the mesh's parts, in order: the shown bodies,
    /// in the order they were made.
    pub fn bodies(&self) -> &[BodyId] {
        &self.bodies
    }

    /// The mesh's faces' keys and summaries, by face id.
    pub fn faces(&self) -> &[PickFace] {
        &self.tables.faces
    }

    /// Whether each of the mesh's edges, by edge id, is a chain that
    /// closes on itself (a hole's rim) rather than running from one corner
    /// to another. A crease's is `false`.
    pub fn closed(&self) -> &[bool] {
        &self.tables.closed
    }

    /// Each of the mesh's edges' tangent chain, by edge id, as the
    /// lowest edge in it: the edges of its part it runs on into smoothly,
    /// end to end (see
    /// [`Topology::tangent_chains`](varde_kernel_compat::Topology::tangent_chains)).
    /// Itself if none, and a crease's itself.
    pub fn tangents(&self) -> &[u32] {
        &self.tables.tangents
    }

    /// Each of the mesh's edges' point, by edge id, where it has one: a
    /// straight edge's middle, a round (circular or elliptic) edge's
    /// centre ([`varde_kernel_compat::measure::edge_shape`]), within
    /// [`Picking::MAX_VALUE`]. A snap point for picking points. A
    /// crease's is `None`.
    pub fn snaps(&self) -> &[Option<[f64; 3]>] {
        &self.tables.snaps
    }

    /// The model's corners.
    pub fn corners(&self) -> &[PickCorner] {
        &self.tables.corners
    }

    /// The body face `face` of `mesh`, the mesh these tables came with,
    /// is of, if there's such a face.
    pub fn face_body(&self, mesh: &RenderMesh, face: u32) -> Option<BodyId> {
        self.bodies.get(mesh.part_of_face(face)?).copied()
    }

    /// The keys of the faces either side of edge `edge` of `mesh`, the
    /// mesh these tables came with, sorted, as an edge reference stores
    /// them; `None` for a crease, which is inside one face, or if there's
    /// no such edge.
    pub fn edge_keys(&self, mesh: &RenderMesh, edge: u32) -> Option<[FaceKey; 2]> {
        let [a, b] = *mesh.edge_faces().get(usize::try_from(edge).ok()?)?;
        let key = |f: u32| Some(self.tables.faces.get(usize::try_from(f).ok()?)?.key);
        let (a, b) = (key(a)?, key(b)?);
        (a != b).then(|| [a.min(b), a.max(b)])
    }

    /// The keys of three of the faces meeting at corner `corner`, sorted,
    /// as a corner reference stores them; `None` if there's no such
    /// corner.
    pub fn corner_keys(&self, corner: u32) -> Option<[FaceKey; 3]> {
        let corner = self.tables.corners.get(usize::try_from(corner).ok()?)?;
        let mut keys = corner.faces.map(|f| self.tables.faces[f as usize].key);
        keys.sort_unstable();
        Some(keys)
    }

    /// Appends one body's drawing's tables, its parts (one, as a solid is
    /// drawn) given to `body`, as its mesh is appended to the scene's.
    /// Fails with [`MeshError::TooLarge`] where an edge id wouldn't fit,
    /// which a mesh within its limits never reaches.
    pub(crate) fn append(&mut self, body: BodyId, drawn: &Drawn) -> Result<(), MeshError> {
        self.tables.append(&drawn.tables)?;
        let parts = drawn.mesh.part_ends().len();
        self.bodies.extend(std::iter::repeat_n(body, parts));
        Ok(())
    }
}

/// One face's aliases: at most [`Picking::MAX_ALIASES`].
fn aliases<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<FaceKey>, D::Error> {
    bounded_seq::at_most(d, Picking::MAX_ALIASES)
}

/// The form of `region`, a region of `solid`'s topology: a region's
/// faces lie on one surface, so its first triangle's face's form stands
/// for them. What the picking tables summarize and what a sketch on the
/// face is placed by, so the two agree to the bit.
pub(crate) fn region_form<'a>(solid: &'a Solid, region: &Region) -> &'a Form {
    let mesh = solid.mesh();
    let tri = &mesh.tris()[region.tris[0] as usize];
    &mesh.faces()[tri.face as usize].form
}

/// One body's solid drawn, with its picking tables: what the cache keeps
/// per body. Its parts have no body yet: the same solid may be shown as
/// several.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Drawn {
    /// Its faces are the solid's regions and its first edges its chains.
    pub(crate) mesh: RenderMesh,
    /// Its tables, by its own face and edge ids: a corner's faces are
    /// its regions, as faces of this body.
    pub(crate) tables: FaceEdgeTables,
}

impl Drawn {
    /// `solid` drawn within `display`, with its topology's (`topology`,
    /// [`Solid::topology`]) regions, chains and corners.
    pub(crate) fn new(
        solid: &Solid,
        topology: &Topology,
        display: &Display,
    ) -> Result<Drawn, MeshError> {
        let mesh = solid.tessellate_with(display, topology)?;
        let faces = (topology.regions().iter())
            .map(|region| PickFace {
                key: region.key,
                aliases: region.aliases.clone(),
                summary: Summary::of(region_form(solid, region)),
            })
            .collect();
        let chains = topology.chains();
        let closed = (0..mesh.edge_count())
            .map(|e| chains.get(e).is_some_and(|chain| chain.closed))
            .collect();
        // The chains are the first edges, in the topology's order; the
        // creases after them are their own.
        let mut tangents = topology.tangent_chains(solid);
        tangents.extend(chains.len() as u32..mesh.edge_count() as u32);
        let shapes: Vec<EdgeShape> = (chains.iter())
            .map(|chain| edge_shape(solid, chain))
            .collect();
        let snaps = (0..mesh.edge_count())
            .map(|e| {
                let snap = shapes.get(e)?.point();
                snap.filter(|&p| point_dvec(p)).map(|p| p.to_array())
            })
            .collect();
        let verts = solid.mesh().verts();
        let corners = (topology.corners().iter())
            .filter_map(|corner| {
                let point = verts[corner.vertex as usize];
                let &[a, b, c, ..] = &corner.regions[..] else {
                    return None;
                };
                point_dvec(point).then(|| PickCorner {
                    faces: [a, b, c],
                    point: point.to_array(),
                })
            })
            .collect();
        let tables = FaceEdgeTables {
            faces,
            closed,
            tangents,
            snaps,
            corners,
        };
        Ok(Drawn { mesh, tables })
    }
}

/// A model's mesh with its picking tables and hierarchies, as a scene is
/// kept. Its mesh's [`RenderMesh::upload`] is built.
#[derive(Debug, Clone)]
pub(crate) struct Scene {
    pub(crate) mesh: Arc<RenderMesh>,
    pub(crate) picking: Arc<Picking>,
    pub(crate) tables: Arc<PickTables>,
}

impl Scene {
    /// The scene of `mesh` and its `picking`, its hierarchies and the
    /// mesh's upload built: the work the UI thread would do with it
    /// otherwise.
    pub(crate) fn new(mesh: RenderMesh, picking: Picking) -> Scene {
        mesh.upload();
        let tables = PickTables::new(&mesh, &picking);
        Scene {
            mesh: Arc::new(mesh),
            picking: Arc::new(picking),
            tables: Arc::new(tables),
        }
    }
}

/// Why parts don't make a [`Picking`] of a mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PickingError {
    /// There isn't one body per part, one face per face and one flag and
    /// tangent chain per edge of the mesh.
    #[error("the picking tables don't match the mesh")]
    Lengths,
    /// A face's summary isn't valid, or its aliases aren't sorted apart
    /// from its key.
    #[error("a picked face's summary or aliases aren't valid")]
    Face,
    /// An edge said to close is a crease, or doesn't start and end at one
    /// corner.
    #[error("a closed edge isn't one")]
    Closed,
    /// An edge's tangent chain's first isn't an edge of its part between
    /// two faces, no later than it and its own first, or a crease's isn't
    /// itself.
    #[error("a picked edge's tangent chain isn't one")]
    Tangent,
    /// An edge's snap point is past the bound, or a crease has one.
    #[error("a picked edge's snap point isn't valid")]
    Snap,
    /// A corner's faces are past the table, out of order, or of two
    /// parts, or its point is past the bound, or there are more corners
    /// than vertices.
    #[error("a picked corner's faces or point aren't valid")]
    Corner,
}

impl HeapSize for FaceEdgeTables {
    /// Its lists, the faces' aliases too.
    fn heap_bytes(&self) -> usize {
        let aliases = (self.faces.iter()).fold(0usize, |sum, face| {
            sum.saturating_add(size_of_val(&face.aliases[..]))
        });
        size_of_val(&self.faces[..])
            .saturating_add(aliases)
            .saturating_add(size_of_val(&self.closed[..]))
            .saturating_add(size_of_val(&self.tangents[..]))
            .saturating_add(size_of_val(&self.snaps[..]))
            .saturating_add(size_of_val(&self.corners[..]))
    }
}

impl HeapSize for Picking {
    fn heap_bytes(&self) -> usize {
        (self.tables.heap_bytes()).saturating_add(size_of_val(&self.bodies[..]))
    }
}

impl HeapSize for Drawn {
    fn heap_bytes(&self) -> usize {
        (self.mesh.heap_bytes()).saturating_add(self.tables.heap_bytes())
    }
}

impl HeapSize for Scene {
    /// Its mesh with its upload, and its tables, each behind its pointer.
    fn heap_bytes(&self) -> usize {
        (total(&*self.mesh))
            .saturating_add(self.mesh.upload().heap_bytes())
            .saturating_add(total(&*self.picking))
            .saturating_add(total(&*self.tables))
    }
}
