//! The bytes that cross to a Web Worker and back.
//!
//! A worker shares no memory with the page, so requests and responses are
//! encoded, posted and decoded. Plain Rust, so it's tested natively; the
//! web [`lane`](crate::lane) moves the bytes.
//!
//! ```text
//! request = postcard(Request)
//! reply   = postcard(Head) | positions | normals | indices | face ends
//!                           | edge vertices | edge ends | edge faces
//!                           | corners | edge corners | part ends
//!                           | line points | line ends
//!         | postcard(Head) | postcard(Vec<ExportedBody>)
//!         | postcard(Head)
//! ```
//!
//! The document in a request is its [`Document::to_postcard`] bytes,
//! checked as it's decoded (see [`codec`]). A request is posted as one
//! `ArrayBuffer`, a reply as an array of them, one per part; both are
//! transferred, not copied. A [`Response`] crosses as its [`Head`], which
//! has no model: the model's parts follow only a [`Head::Regenerated`] and
//! are the bytes of the [`RenderMesh`]'s and the sketches' [`RenderLines`]'
//! vectors as they are in memory (little endian on wasm). The [`Picking`]
//! tables ride in the head.
//!
//! A request is copied out of its buffer only if it's within
//! `MAX_REQUEST_BYTES`. Replies are checked on receipt: the head is within
//! [`MAX_HEAD_BYTES`] and each model part is whole elements within the
//! [`RenderMesh`] and [`RenderLines`] limits, both before they're copied,
//! so a broken reply doesn't allocate without bound, and together the parts
//! make a [`RenderMesh`] by [`RenderMesh::from_parts`] and [`RenderLines`]
//! by [`RenderLines::from_parts`] and, with the head's tables, a
//! [`Picking`] by [`Picking::from_parts`], each part's body one the head
//! lists; the bodies' boxes in the head are finite with their corners in
//! order, and the merged bodies name each consumed body once, never as a
//! holder. The failed features' ids, the sketches that don't solve and
//! the bodies a draft or a feature touches are only marks, so they aren't
//! checked against a document. Malformed
//! bytes are refused, never a panic; see [`decode_request`] and
//! [`decode_reply`]. A request's draft isn't checked as it's decoded:
//! applying it goes through the document's checks.
//!
//! An export's bodies follow a [`Head::Exported`] that went as one part,
//! their postcard, copied only within [`MAX_EXPORT_BYTES`]; each
//! [`ManifoldMesh`](varde_kernel::ManifoldMesh) is checked again as it's
//! decoded, so the page only ever writes a checked manifold. Bodies that
//! fail to decode answer the export with the error.
//!
//! [`Document::to_postcard`]: varde_document::Document::to_postcard
//! [`codec`]: varde_document::codec

use std::borrow::Cow;
use std::fmt;
use std::mem::size_of;
use std::sync::Arc;

use glam::Vec3;
use serde::{Deserialize, Serialize};
use varde_document::{BodyId, DecodeError, FeatureId, Generation, codec};
use varde_kernel::{
    Aabb, LinesError, LinesPart, MeshError, MeshPart, MeshParts, RenderLines, RenderMesh,
};
use varde_lane::bytes::Buffer;

use crate::{Drafted, ExportedBody, PickFace, Picking, PickingError, Request, Response};

/// The most bytes a reply's head may have. A head is a generation, a few
/// feature ids, the failed features' messages, the bodies each join, cut
/// or intersect touches, a box per body and the picking tables (some 60
/// bytes a face and up to 6 an edge), or an error message, so this is far
/// more than any real one needs. A model whose head would be larger is
/// answered as failed ([`encode_reply`]). It's no larger because a few
/// bytes of a head can stand for many more on the page (a feature id is
/// 8 bytes there and as few as 1 here).
pub const MAX_HEAD_BYTES: usize = 1 << 26;

/// The most faces a reply's picking tables may have: past any real
/// model's, and few enough that decoding them can't take the page's
/// memory (a face is about 120 bytes there and as few as 5 in the head).
/// A model with more is answered as failed ([`encode_reply`]).
pub const MAX_FACES: usize = 1 << 20;

/// The bounded decoding of a head's picking tables: refused as soon as
/// they're past their bounds, mostly before any element is read.
mod bounded {
    use serde::{Deserialize, Deserializer};
    use varde_document::BodyId;
    use varde_kernel::RenderMesh;

    use super::MAX_FACES;
    use crate::picking::bounded::seq;
    use crate::{PickFace, Picking};

    /// At most [`MAX_FACES`] faces with at most [`Picking::MAX_ALIASES`]
    /// aliases together.
    pub(super) fn faces<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PickFace>, D::Error> {
        seq(
            d,
            MAX_FACES,
            |face: &PickFace| face.aliases.len(),
            Picking::MAX_ALIASES,
        )
    }

    /// At most [`RenderMesh::MAX_PARTS`] parts' bodies.
    pub(super) fn parts<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<BodyId>, D::Error> {
        seq(d, RenderMesh::MAX_PARTS, |_| 0, 0)
    }

    /// At most [`RenderMesh::MAX_EDGE_POLYLINES`] edges' entries: their
    /// closed flags or tangent chains.
    pub(super) fn edges<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        d: D,
    ) -> Result<Vec<T>, D::Error> {
        seq(d, RenderMesh::MAX_EDGE_POLYLINES, |_| 0, 0)
    }
}

/// The most bytes an export's bodies may have, see the module docs: far
/// more than a design meant for printing needs, and the bound keeps a
/// broken reply from being copied without end.
pub const MAX_EXPORT_BYTES: usize = 1 << 30;

/// The most bytes a request may have. Documents are far smaller; the
/// bound keeps a broken request from being copied without end.
#[cfg(target_arch = "wasm32")]
pub const MAX_REQUEST_BYTES: usize = 1 << 30;

/// Encodes `request` to post to a worker.
pub fn encode_request(request: &Request) -> Vec<u8> {
    postcard::to_stdvec(request).expect("requests always serialize")
}

/// Decodes a request, checking its document and refusing bytes after its
/// end. The page encodes requests from checked documents, so a request
/// refused here is a bug: the worker throws, and the page answers the
/// request as it does for any worker that stops.
pub fn decode_request(bytes: &[u8]) -> Result<Request, Error> {
    codec::from_postcard_exact(bytes).map_err(Error::Request)
}

/// What a reply is, in front of its mesh parts. That the worker is ready,
/// or panicked, isn't a reply: [`varde_lane`] says so.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    clippy::large_enum_variant,
    reason = "a head is decoded once per answer and taken apart at once"
)]
pub enum Head {
    /// A [`Response::Regenerated`], followed by its model's
    /// [`MODEL_PARTS`] parts.
    Regenerated {
        generation: Generation,
        exclude: Option<FeatureId>,
        draft: Option<Drafted>,
        /// The sketches that don't solve. Only marks, so an id naming no
        /// sketch of the document marks nothing, and isn't checked.
        unsolved: Vec<FeatureId>,
        /// The features that failed, likewise only marks.
        failed: Vec<(FeatureId, String)>,
        /// The bodies each join, cut or intersect touches, likewise only
        /// marks.
        touched: Vec<(FeatureId, Vec<BodyId>)>,
        /// Each consumed body and its holder, checked to name each
        /// consumed body once and none as a holder ([`Error::Merged`]);
        /// otherwise only marks.
        merged: Vec<(BodyId, BodyId)>,
        /// Each body's box, its least and greatest corner, checked to be
        /// finite and in order ([`Error::Bounds`]).
        bodies: Vec<(BodyId, [[f32; 3]; 2])>,
        /// The picking tables' body of each of the mesh's parts, each in
        /// `bodies` ([`Error::Picking`]), at most
        /// [`RenderMesh::MAX_PARTS`], refused as they're decoded.
        #[serde(deserialize_with = "bounded::parts")]
        parts: Vec<BodyId>,
        /// The picking tables' faces, at most [`MAX_FACES`] with at most
        /// [`Picking::MAX_ALIASES`] aliases together, refused as they're
        /// decoded.
        #[serde(deserialize_with = "bounded::faces")]
        faces: Vec<PickFace>,
        /// The picking tables' closed flags, one per edge, at most
        /// [`RenderMesh::MAX_EDGE_POLYLINES`].
        #[serde(deserialize_with = "bounded::edges")]
        closed: Vec<bool>,
        /// The picking tables' tangent chains, one per edge, at most
        /// [`RenderMesh::MAX_EDGE_POLYLINES`].
        #[serde(deserialize_with = "bounded::edges")]
        tangents: Vec<u32>,
    },
    /// A [`Response::Failed`].
    Failed {
        generation: Generation,
        exclude: Option<FeatureId>,
        draft: Option<u64>,
        error: String,
    },
    /// A [`Response::Exported`]: if `Ok`, followed by one part, the
    /// bodies' postcard.
    Exported {
        export: u64,
        result: Result<(), String>,
    },
}

impl Head {
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_stdvec(self).expect("heads always serialize")
    }

    /// Decodes a head, refusing bytes after its end.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        codec::from_postcard_exact(bytes).map_err(Error::Head)
    }
}

/// How many parts follow a [`Head::Regenerated`]: the mesh's ten
/// ([`MeshParts`]' fields, in order), and the sketches' points and ends.
pub const MODEL_PARTS: usize = 12;

/// The reply answering `response`, the mirror of [`decode_reply`]: its
/// encoded head, and the parts following it: its model's as bytes if it
/// has one, see [`MODEL_PARTS`], or an export's bodies. A model whose
/// head would be over [`MAX_HEAD_BYTES`], or whose picking tables are
/// past [`MAX_FACES`] or [`Picking::MAX_ALIASES`], is answered as failed,
/// which the page would otherwise refuse with no generation to answer.
pub fn encode_reply(response: &Response) -> (Vec<u8>, Vec<Cow<'_, [u8]>>) {
    match response {
        Response::Regenerated {
            generation,
            exclude,
            draft,
            mesh,
            picking,
            sketches,
            unsolved,
            failed,
            touched,
            merged,
            bodies,
        } => {
            let head = Head::Regenerated {
                generation: *generation,
                exclude: *exclude,
                draft: draft.clone(),
                unsolved: unsolved.clone(),
                failed: failed.clone(),
                touched: touched.clone(),
                merged: merged.clone(),
                bodies: bodies
                    .iter()
                    .map(|(body, aabb)| (*body, [aabb.min.to_array(), aabb.max.to_array()]))
                    .collect(),
                parts: picking.bodies().to_vec(),
                faces: picking.faces().to_vec(),
                closed: picking.closed().to_vec(),
                tangents: picking.tangents().to_vec(),
            }
            .encode();
            let aliases = (picking.faces().iter())
                .fold(0usize, |sum, face| sum.saturating_add(face.aliases.len()));
            if head.len() > MAX_HEAD_BYTES
                || picking.faces().len() > MAX_FACES
                || aliases > Picking::MAX_ALIASES
            {
                let failed = Head::Failed {
                    generation: *generation,
                    exclude: *exclude,
                    draft: draft.as_ref().map(|draft| draft.revision),
                    error: "the model has more faces than can be sent".to_owned(),
                };
                return (failed.encode(), Vec::new());
            }
            (
                head,
                [
                    bytemuck::cast_slice(mesh.positions()),
                    bytemuck::cast_slice(mesh.normals()),
                    bytemuck::cast_slice(mesh.indices()),
                    bytemuck::cast_slice(mesh.face_ends()),
                    bytemuck::cast_slice(mesh.edge_vertices()),
                    bytemuck::cast_slice(mesh.edge_ends()),
                    bytemuck::cast_slice(mesh.edge_faces()),
                    bytemuck::cast_slice(mesh.corners()),
                    bytemuck::cast_slice(mesh.edge_corners()),
                    bytemuck::cast_slice(mesh.part_ends()),
                    bytemuck::cast_slice(sketches.points()),
                    bytemuck::cast_slice(sketches.ends()),
                ]
                .map(Cow::Borrowed)
                .into(),
            )
        }
        Response::Failed {
            generation,
            exclude,
            draft,
            error,
        } => (
            Head::Failed {
                generation: *generation,
                exclude: *exclude,
                draft: *draft,
                error: error.clone(),
            }
            .encode(),
            Vec::new(),
        ),
        Response::Exported { export, result } => {
            let bodies = result.as_ref().map(|bodies| {
                postcard::to_stdvec(bodies).expect("exported bodies always serialize")
            });
            let head = Head::Exported {
                export: *export,
                result: bodies.as_ref().map(|_| ()).map_err(|e| (*e).clone()),
            };
            (head.encode(), bodies.into_iter().map(Cow::Owned).collect())
        }
    }
}

/// Decodes a reply from its head and the parts following it. A model that
/// fails its checks answers its generation with [`Response::Failed`]; only a
/// head that can't be decoded is an error, since there's no generation to
/// answer.
pub fn decode_reply(
    head: &(impl Buffer + ?Sized),
    parts: &[impl Buffer],
) -> Result<Response, Error> {
    let head = copy::<u8>(Part::Head, head, MAX_HEAD_BYTES)?;
    Ok(match Head::decode(&head)? {
        Head::Regenerated {
            generation,
            exclude,
            draft,
            unsolved,
            failed,
            touched,
            merged,
            bodies,
            parts: part_bodies,
            faces,
            closed,
            tangents,
        } => {
            let model = check_merged(&merged)
                .and_then(|()| decode_bodies(&bodies))
                .and_then(|bodies| {
                    let mut listed: Vec<BodyId> = bodies.iter().map(|&(body, _)| body).collect();
                    listed.sort_unstable();
                    if !(part_bodies.iter()).all(|body| listed.binary_search(body).is_ok()) {
                        return Err(Error::Picking(PickingError::Body));
                    }
                    let (mesh, sketches) = decode_model(parts)?;
                    let picking = Picking::from_parts(part_bodies, faces, closed, tangents, &mesh)
                        .map_err(Error::Picking)?;
                    Ok((bodies, mesh, sketches, picking))
                });
            match model {
                Ok((bodies, mesh, sketches, picking)) => Response::Regenerated {
                    generation,
                    exclude,
                    draft,
                    mesh: Arc::new(mesh),
                    picking: Arc::new(picking),
                    sketches: Arc::new(sketches),
                    unsolved,
                    failed,
                    touched,
                    merged,
                    bodies,
                },
                Err(error) => Response::Failed {
                    generation,
                    exclude,
                    draft: draft.map(|draft| draft.revision),
                    error: error.to_string(),
                },
            }
        }
        Head::Failed {
            generation,
            exclude,
            draft,
            error,
        } => Response::Failed {
            generation,
            exclude,
            draft,
            error,
        },
        Head::Exported { export, result } => Response::Exported {
            export,
            result: result.and_then(|()| decode_export(parts).map_err(|e| e.to_string())),
        },
    })
}

/// Decodes and checks the bodies following a [`Head::Exported`] that went.
pub fn decode_export(parts: &[impl Buffer]) -> Result<Vec<ExportedBody>, Error> {
    let [bodies] = parts else {
        return Err(Error::ExportParts(parts.len()));
    };
    let bytes = copy::<u8>(Part::Export, bodies, MAX_EXPORT_BYTES)?;
    codec::from_postcard_exact(&bytes).map_err(Error::Export)
}

/// Checks the merged bodies of a [`Head::Regenerated`]: each consumed
/// body is listed once, and none holds another (a holder has a solid, a
/// consumed body none).
fn check_merged(merged: &[(BodyId, BodyId)]) -> Result<(), Error> {
    let mut consumed: Vec<BodyId> = merged.iter().map(|&(consumed, _)| consumed).collect();
    consumed.sort_unstable();
    let repeated = consumed.windows(2).any(|pair| pair[0] == pair[1]);
    let held = (merged.iter()).any(|(_, holder)| consumed.binary_search(holder).is_ok());
    if repeated || held {
        Err(Error::Merged)
    } else {
        Ok(())
    }
}

/// The bodies' boxes of a [`Head::Regenerated`], each checked to be
/// finite with its least corner below its greatest.
fn decode_bodies(bodies: &[(BodyId, [[f32; 3]; 2])]) -> Result<Vec<(BodyId, Aabb)>, Error> {
    bodies
        .iter()
        .map(|&(body, [min, max])| {
            let (min, max) = (Vec3::from(min), Vec3::from(max));
            if min.is_finite() && max.is_finite() && min.cmple(max).all() {
                Ok((body, Aabb { min, max }))
            } else {
                Err(Error::Bounds)
            }
        })
        .collect()
}

/// Decodes and checks the model's parts following a [`Head::Regenerated`]:
/// the mesh and the sketches' lines.
pub fn decode_model(parts: &[impl Buffer]) -> Result<(RenderMesh, RenderLines), Error> {
    let Ok([mesh @ .., points, ends]) = <&[_; MODEL_PARTS]>::try_from(parts) else {
        return Err(Error::Parts(parts.len()));
    };
    Ok((decode_mesh(mesh)?, decode_lines([points, ends])?))
}

/// Decodes and checks a mesh's parts, [`MeshParts`]' fields in order.
fn decode_mesh<B: Buffer>(
    [
        positions,
        normals,
        indices,
        face_ends,
        edge_vertices,
        edge_ends,
        edge_faces,
        corners,
        edge_corners,
        part_ends,
    ]: &[B; MODEL_PARTS - 2],
) -> Result<RenderMesh, Error> {
    use RenderMesh as M;
    let part = Part::RenderMesh;
    RenderMesh::from_parts(MeshParts {
        positions: copy(part(MeshPart::Positions), positions, M::MAX_VERTICES)?,
        normals: copy(part(MeshPart::Normals), normals, M::MAX_VERTICES)?,
        indices: copy(part(MeshPart::Indices), indices, M::MAX_INDICES)?,
        face_ends: copy(part(MeshPart::FaceEnds), face_ends, M::MAX_FACES)?,
        edge_vertices: copy(
            part(MeshPart::EdgeVertices),
            edge_vertices,
            M::MAX_EDGE_POINTS,
        )?,
        edge_ends: copy(part(MeshPart::EdgeEnds), edge_ends, M::MAX_EDGE_POLYLINES)?,
        edge_faces: copy(part(MeshPart::EdgeFaces), edge_faces, M::MAX_EDGE_POLYLINES)?,
        corners: copy(part(MeshPart::Corners), corners, M::MAX_CORNERS)?,
        edge_corners: copy(
            part(MeshPart::EdgeCorners),
            edge_corners,
            M::MAX_EDGE_POLYLINES,
        )?,
        part_ends: copy(part(MeshPart::PartEnds), part_ends, M::MAX_PARTS)?,
    })
    .map_err(Error::RenderMesh)
}

/// Decodes and checks lines' points and ends.
fn decode_lines<B: Buffer + ?Sized>([points, ends]: [&B; 2]) -> Result<RenderLines, Error> {
    RenderLines::from_parts(
        copy(
            Part::RenderLines(LinesPart::Points),
            points,
            RenderLines::MAX_POINTS,
        )?,
        copy(
            Part::RenderLines(LinesPart::Ends),
            ends,
            RenderLines::MAX_POLYLINES,
        )?,
    )
    .map_err(Error::RenderLines)
}

/// The `T`s the `part` holds, copied out of it if it's at most `max` of
/// them.
fn copy<T: bytemuck::Pod>(
    part: Part,
    buffer: &(impl Buffer + ?Sized),
    max: usize,
) -> Result<Vec<T>, Error> {
    let mut out = vec![T::zeroed(); count::<T>(part, buffer, max)?];
    buffer.copy_into(bytemuck::cast_slice_mut(&mut out));
    Ok(out)
}

/// How many `T`s the `part` holds, if it holds whole ones and not more
/// than `max`.
fn count<T>(part: Part, buffer: &(impl Buffer + ?Sized), max: usize) -> Result<usize, Error> {
    let len = buffer.byte_len();
    if len.div_ceil(size_of::<T>()) > max {
        return Err(Error::TooLarge { part, len });
    }
    if !len.is_multiple_of(size_of::<T>()) {
        return Err(Error::Partial { part, len });
    }
    Ok(len / size_of::<T>())
}

/// A part of a reply: its head, or one of its model's parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Head,
    RenderMesh(MeshPart),
    RenderLines(LinesPart),
    /// An export's bodies.
    Export,
}

impl fmt::Display for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Part::Head => f.write_str("head"),
            Part::RenderMesh(part) => part.fmt(f),
            Part::RenderLines(part) => part.fmt(f),
            Part::Export => f.write_str("exported bodies"),
        }
    }
}

/// Why bytes from the other side were refused.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// A request, or the document in it, couldn't be decoded.
    Request(DecodeError),
    /// The head of a reply couldn't be decoded.
    Head(DecodeError),
    /// A model came in this many parts instead of [`MODEL_PARTS`].
    Parts(usize),
    /// The head or a model part is larger than its bound.
    TooLarge { part: Part, len: usize },
    /// The part's length isn't a whole number of its elements.
    Partial { part: Part, len: usize },
    /// The parts don't make a mesh.
    RenderMesh(MeshError),
    /// The parts don't make lines.
    RenderLines(LinesError),
    /// A body's box isn't finite, or its corners are out of order.
    Bounds,
    /// A consumed body is listed twice, or as a holder.
    Merged,
    /// The picking tables don't go with the mesh, or name a body the head
    /// doesn't list.
    Picking(PickingError),
    /// An export's bodies came in this many parts instead of one.
    ExportParts(usize),
    /// An export's bodies couldn't be decoded, or a mesh among them
    /// isn't a manifold.
    Export(DecodeError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Request(e) => write!(f, "couldn't decode the request: {e}"),
            Error::Head(e) => write!(f, "couldn't decode the reply: {e}"),
            Error::Parts(n) => write!(f, "model in {n} parts instead of {MODEL_PARTS}"),
            Error::TooLarge { part, len } => write!(f, "reply {part} too large: {len} bytes"),
            Error::Partial { part, len } => {
                write!(f, "reply {part} of {len} bytes aren't whole elements")
            }
            Error::RenderMesh(e) => e.fmt(f),
            Error::RenderLines(e) => e.fmt(f),
            Error::Bounds => f.write_str("a body's box isn't one"),
            Error::Merged => f.write_str("a merged body is listed twice or holds another"),
            Error::Picking(e) => e.fmt(f),
            Error::ExportParts(n) => write!(f, "exported bodies in {n} parts instead of 1"),
            Error::Export(e) => write!(f, "couldn't decode the exported bodies: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Request(e) | Error::Head(e) | Error::Export(e) => Some(e),
            Error::Parts(_)
            | Error::TooLarge { .. }
            | Error::Partial { .. }
            | Error::RenderMesh(_)
            | Error::RenderLines(_)
            | Error::Bounds
            | Error::Merged
            | Error::Picking(_)
            | Error::ExportParts(_) => None,
        }
    }
}

#[cfg(test)]
mod tests;
