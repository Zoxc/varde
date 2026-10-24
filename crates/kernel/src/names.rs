//! Names: what faces, edges and vertices are called, made only from what
//! the caller gave the operations, so they stay the same when a model is
//! rebuilt with other dimensions, another tolerance or other ids.
//!
//! - [`keys`]: a face's key ([`FaceKey`]: feature, copy, [`FacePart`]),
//!   the 128-bit [`Digest`]s by which keys refer to other names and the
//!   body's [`TermTable`] of what they stand for, and the tags
//!   ([`EdgeTag::Seam`], [`VertTag::Pole`], [`VertTag::Apex`]).
//! - [`derive`](mod@derive): edges' and vertices' names, derived from the keys of
//!   their faces ([`Between`], [`Meet`]).
//!
//! **Identity is not a name.** Ids order an operation's work and pair its
//! entities, but they change whenever an upstream change adds or removes
//! an entity, so no name holds one. The types say so: a name is made of
//! keys, digests, tags and the caller's numbers, and an id fits none of
//! them.
//!
//! ```
//! use varde_kernel::names::{Between, FaceKey, FacePart, Meet};
//! use varde_kernel::{FeatureId, OpCtx};
//!
//! let ctx = OpCtx::new(FeatureId(4));
//! let top = FaceKey::new(ctx, FacePart::Primitive(1));
//! let side = FaceKey::new(ctx, FacePart::Primitive(2));
//! let edge = Between::new(top, side);
//! let corner = Meet::new([top, side, FaceKey::new(ctx, FacePart::Primitive(3))]);
//! assert!(corner.has(&edge.keys()[0]));
//! ```
//!
//! A face's id is no part:
//!
//! ```compile_fail,E0308
//! use varde_kernel::names::FacePart;
//! use varde_kernel::topo::brep::FaceId;
//!
//! let _ = FacePart::Primitive(FaceId(1));
//! ```
//!
//! nor an edge's a vertex's name:
//!
//! ```compile_fail,E0271
//! use varde_kernel::names::Meet;
//! use varde_kernel::topo::brep::EdgeId;
//!
//! let _ = Meet::new([EdgeId(0), EdgeId(1)]);
//! ```
//!
//! nor a vertex's an edge's:
//!
//! ```compile_fail,E0308
//! use varde_kernel::names::Between;
//! use varde_kernel::topo::brep::VertId;
//!
//! let _ = Between::new(VertId(0), VertId(1));
//! ```
//!
//! nor any id a digest:
//!
//! ```compile_fail,E0308
//! use varde_kernel::names::Digest;
//! use varde_kernel::topo::brep::FaceId;
//!
//! let _ = Digest::from_u64(FaceId(1));
//! ```

pub mod derive;
pub mod keys;

pub use derive::{Between, EdgeName, Meet, Name, VertName, edge_name, edge_names, vert_names};
pub use keys::{
    Digest, DigestCollision, EdgeTag, FaceKey, FacePart, PoleEnd, Term, TermTable, VertTag,
};
