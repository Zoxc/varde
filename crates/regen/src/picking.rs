//! The picking tables a model's answer carries: which face and which edge
//! of which body each triangle and edge of its mesh draws, and what the
//! viewport says about and builds on each face and edge.
//!
//! Faces are the kernel's regions ([`Topology::regions`](varde_kernel::Topology::regions)): connected
//! triangles of one [`FaceKey`], so a circle's quarter walls, or flush
//! faces merged under one name, are one face, and a face cut in two by a
//! groove is two of one key. Edges are its chains ([`Topology::chains`](varde_kernel::Topology::chains)):
//! maximal runs of mesh edges between the same two faces. Each body's
//! tables are worked out with its mesh ([`Drawn`]) and the scene's are
//! theirs joined, in the shown bodies' order, as the mesh is.
//!
//! What a [`Picking`] guarantees is on it.

use std::fmt;
use std::mem::size_of_val;
use std::sync::Arc;

use glam::DVec3;
use serde::{Deserialize, Serialize};
use varde_document::BodyId;
use varde_kernel::mesh::{FaceKey, Form};
use varde_kernel::{Display, MeshError, RenderMesh, Solid};

/// What a face is, as far as the viewport needs to know: a plane's
/// outward unit normal `n` and offset `d` (the plane `n·x = d`), from
/// which a sketch's placement on it is worked out; a round face's axis
/// and size. Faces of other forms, or with numbers past
/// [`Picking::MAX_VALUE`], are [`Summary::Other`].
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
    Other,
}

impl Summary {
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
            Form::Unknown | Form::ConicCylinder { .. } | Form::Revolved { .. } => Summary::Other,
        };
        if summary.valid() {
            summary
        } else {
            Summary::Other
        }
    }

    /// Whether its numbers are finite and within [`Picking::MAX_VALUE`],
    /// its directions unit vectors (within [`Picking::UNIT`]), its sizes positive
    /// and a cone's cosine and sine within `(0, 1]`.
    pub fn valid(&self) -> bool {
        let value = |x: f64| x.abs() <= Picking::MAX_VALUE;
        let point = |p: [f64; 3]| p.into_iter().all(value);
        let unit = |v: [f64; 3]| (DVec3::from(v).length() - 1.0).abs() <= Picking::UNIT;
        let size = |x: f64| x > 0.0 && value(x);
        let ratio = |x: f64| x > 0.0 && x <= 1.0;
        match *self {
            Summary::Plane { n, d } => unit(n) && value(d),
            Summary::Cylinder {
                point: p,
                axis,
                radius,
            } => point(p) && unit(axis) && size(radius),
            Summary::Cone {
                apex,
                axis,
                cos,
                sin,
            } => point(apex) && unit(axis) && ratio(cos) && ratio(sin),
            Summary::Sphere { centre, radius } => point(centre) && size(radius),
            Summary::Torus {
                centre,
                axis,
                major,
                minor,
            } => point(centre) && unit(axis) && size(major) && size(minor),
            Summary::Other => true,
        }
    }
}

/// A face of the model: a region of a body's solid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickFace {
    pub body: BodyId,
    /// Its name, which references store.
    pub key: FaceKey,
    /// The keys merged into it, which name it too, sorted, without `key`.
    pub aliases: Vec<FaceKey>,
    pub summary: Summary,
}

/// An edge of the model: a chain of a body's solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PickChain {
    /// The faces either side, indices into [`Picking::faces`]: two
    /// different faces of one body.
    pub faces: [u32; 2],
    /// Whether it closes on itself (a hole's rim) rather than running
    /// from one corner to another.
    pub closed: bool,
}

/// The picking tables of a model's mesh: its faces and edges, and which
/// of them each triangle and edge of the mesh draws.
///
/// It's always consistent with the mesh it came with: one face per
/// triangle, one chain (or [`Picking::NONE`]) per edge, every index within
/// its table, each chain between two different faces of one body, every
/// summary [`Summary::valid`], each face's aliases sorted and apart from
/// its key, no more faces than triangles nor chains than edges. The
/// fields are private so that holds; one from the other side of the web
/// worker comes in through [`Picking::from_parts`], which checks it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Picking {
    faces: Vec<PickFace>,
    chains: Vec<PickChain>,
    triangles: Vec<u32>,
    edges: Vec<u32>,
}

impl Picking {
    /// An edge on no chain: a crease inside one face.
    pub const NONE: u32 = varde_kernel::Picking::NONE;
    /// The largest number a summary may hold: past any plane's offset
    /// or point a solid within the kernel's limits has.
    pub const MAX_VALUE: f64 = 1e8;
    /// How far from 1 a summary's direction's length may be: a unit
    /// vector rounded is far nearer.
    pub const UNIT: f64 = 1e-9;

    /// The tables of `mesh` of these parts, if they make them; see
    /// [`Picking`] for what is checked.
    pub fn from_parts(
        faces: Vec<PickFace>,
        chains: Vec<PickChain>,
        triangles: Vec<u32>,
        edges: Vec<u32>,
        mesh: &RenderMesh,
    ) -> Result<Picking, PickingError> {
        if triangles.len() != mesh.triangle_count() || edges.len() != mesh.edges().len() {
            return Err(PickingError::Lengths);
        }
        // Every face and chain is drawn, so there are no more of them
        // than of what draws them.
        if faces.len() > triangles.len() || chains.len() > edges.len() {
            return Err(PickingError::Tables);
        }
        if triangles.iter().any(|&f| f as usize >= faces.len())
            || (edges.iter()).any(|&c| c != Self::NONE && c as usize >= chains.len())
        {
            return Err(PickingError::Index);
        }
        let chain_ok = |chain: &PickChain| {
            let [a, b] = chain.faces.map(|f| faces.get(f as usize));
            matches!((a, b), (Some(a), Some(b)) if chain.faces[0] != chain.faces[1] && a.body == b.body)
        };
        if !chains.iter().all(chain_ok) {
            return Err(PickingError::Chain);
        }
        let face_ok = |face: &PickFace| {
            face.summary.valid()
                && face.aliases.windows(2).all(|pair| pair[0] < pair[1])
                && !face.aliases.contains(&face.key)
        };
        if !faces.iter().all(face_ok) {
            return Err(PickingError::Face);
        }
        Ok(Picking {
            faces,
            chains,
            triangles,
            edges,
        })
    }

    /// The model's faces.
    pub fn faces(&self) -> &[PickFace] {
        &self.faces
    }

    /// The model's edges.
    pub fn chains(&self) -> &[PickChain] {
        &self.chains
    }

    /// One per triangle of the mesh: its face.
    pub fn triangles(&self) -> &[u32] {
        &self.triangles
    }

    /// One per edge of the mesh: its chain, or [`Picking::NONE`].
    pub fn edges(&self) -> &[u32] {
        &self.edges
    }

    /// The keys of the faces either side of chain `chain`, sorted, as an
    /// edge reference stores them.
    pub fn chain_keys(&self, chain: u32) -> [FaceKey; 2] {
        let [a, b] = self.chains[chain as usize]
            .faces
            .map(|f| self.faces[f as usize].key);
        [a.min(b), a.max(b)]
    }

    /// About how many bytes it holds, for the cache.
    pub(crate) fn bytes(&self) -> usize {
        let aliases = (self.faces.iter()).fold(0usize, |sum, face| {
            sum.saturating_add(size_of_val(&face.aliases[..]))
        });
        size_of_val(&self.faces[..])
            .saturating_add(aliases)
            .saturating_add(size_of_val(&self.chains[..]))
            .saturating_add(size_of_val(&self.triangles[..]))
            .saturating_add(size_of_val(&self.edges[..]))
            .saturating_add(size_of_val(self))
    }

    /// Appends one body's drawing's tables, its faces given to `body`.
    /// Fails with [`MeshError::TooLarge`] where an index wouldn't fit,
    /// which a mesh within its limits never reaches.
    pub(crate) fn append(&mut self, body: BodyId, drawn: &Drawn) -> Result<(), MeshError> {
        let base = |len: usize| u32::try_from(len).map_err(|_| MeshError::TooLarge);
        let (faces, chains) = (base(self.faces.len())?, base(self.chains.len())?);
        let moved = |i: u32, by: u32| i.checked_add(by).filter(|&i| i != Self::NONE);
        let moved = |i: u32, by: u32| moved(i, by).ok_or(MeshError::TooLarge);
        let mut triangles = Vec::with_capacity(drawn.picking.triangles.len());
        for &r in &drawn.picking.triangles {
            triangles.push(moved(r, faces)?);
        }
        let mut edges = Vec::with_capacity(drawn.picking.edges.len());
        for &c in &drawn.picking.edges {
            edges.push(if c == Self::NONE {
                c
            } else {
                moved(c, chains)?
            });
        }
        let mut new_chains = Vec::with_capacity(drawn.chains.len());
        for chain in &drawn.chains {
            let [a, b] = chain.faces;
            new_chains.push(PickChain {
                faces: [moved(a, faces)?, moved(b, faces)?],
                closed: chain.closed,
            });
        }
        self.faces
            .extend(drawn.faces.iter().map(|(key, aliases, summary)| PickFace {
                body,
                key: *key,
                aliases: aliases.clone(),
                summary: *summary,
            }));
        self.chains.extend(new_chains);
        self.triangles.extend(triangles);
        self.edges.extend(edges);
        Ok(())
    }
}

/// One body's solid drawn, with its picking tables: what the cache keeps
/// per body. Its faces have no body yet: the same solid may be shown as
/// several.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Drawn {
    pub(crate) mesh: RenderMesh,
    /// Per region: its key, aliases and summary.
    pub(crate) faces: Vec<(FaceKey, Vec<FaceKey>, Summary)>,
    /// Per chain: its regions, as faces of this body.
    pub(crate) chains: Vec<PickChain>,
    pub(crate) picking: varde_kernel::Picking,
}

impl Drawn {
    /// `solid` drawn within `display`, with its topology's regions and
    /// chains.
    pub(crate) fn new(solid: &Solid, display: &Display) -> Result<Drawn, MeshError> {
        let topology = solid.topology();
        let (mesh, picking) = solid.tessellate_picking(display, &topology)?;
        let faces = solid.mesh().faces();
        let tris = solid.mesh().tris();
        let faces = (topology.regions().iter())
            .map(|region| {
                // A region's faces lie on one surface: its first's form
                // stands for them.
                let face = &faces[tris[region.tris[0] as usize].face as usize];
                (region.key, region.aliases.clone(), Summary::of(&face.form))
            })
            .collect();
        let chains = (topology.chains().iter())
            .map(|chain| PickChain {
                faces: chain.regions,
                closed: chain.closed,
            })
            .collect();
        Ok(Drawn {
            mesh,
            faces,
            chains,
            picking,
        })
    }

    /// About how many bytes it holds, for the cache.
    pub(crate) fn bytes(&self) -> usize {
        let aliases = (self.faces.iter()).fold(0usize, |sum, (_, aliases, _)| {
            sum.saturating_add(size_of_val(&aliases[..]))
        });
        size_of_val(&self.faces[..])
            .saturating_add(aliases)
            .saturating_add(size_of_val(&self.chains[..]))
            .saturating_add(size_of_val(&self.picking.triangles[..]))
            .saturating_add(size_of_val(&self.picking.edges[..]))
    }
}

/// A model's mesh with its picking tables, as a scene is kept.
#[derive(Debug, Clone)]
pub(crate) struct Scene {
    pub(crate) mesh: Arc<RenderMesh>,
    pub(crate) picking: Arc<Picking>,
}

/// Why parts don't make a [`Picking`] of a mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickingError {
    /// There isn't one face per triangle and one chain per edge.
    Lengths,
    /// There are more faces than triangles or more chains than edges.
    Tables,
    /// A triangle's face or an edge's chain is past its table.
    Index,
    /// A chain's faces are past the table, the same face, or of two
    /// bodies.
    Chain,
    /// A face's summary isn't valid, or its aliases aren't sorted apart
    /// from its key.
    Face,
}

impl fmt::Display for PickingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PickingError::Lengths => "the picking tables don't match the mesh",
            PickingError::Tables => "the picking tables are larger than the mesh",
            PickingError::Index => "a picking index is past its table",
            PickingError::Chain => "a picked edge's faces aren't two of one body",
            PickingError::Face => "a picked face's summary or aliases aren't valid",
        })
    }
}

impl std::error::Error for PickingError {}
