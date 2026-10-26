//! The kernel's topology: the B-rep store with every field a body keeps
//! ([`brep`]), its building and editing ([`build`]) and its check
//! ([`check`]: the invariants in order, the first failure in the
//! geometric order); [`brep_min`], the minimal store the snap and the
//! arrangement passes read (vertices, edges, coedges with their p-curves
//! and seam bounds, loops and faces) and the bit-exact joins its ribbons
//! need;
//! and [`arrange2d`], a face's arrangement in its UV on straight spans:
//! rotation order, face cycles, nesting by a containment certificate,
//! regions and their windings; [`adjacent`], the local certificates
//! of faces sharing an edge or a vertex (the check's invariant 9); and
//! [`continuity`], how smoothly an edge's faces meet, certified.

pub mod adjacent;
pub mod arrange2d;
pub mod brep;
pub mod brep_min;
// Its callers are the constructions and the assembly, as they land.
#[allow(dead_code, reason = "moved ahead of the operations that build bodies")]
pub(crate) mod build;
pub mod check;
pub(crate) mod regions;
// Its callers are the constructions and the assembly, as they land.
#[allow(dead_code, reason = "moved ahead of the operations that build edges")]
pub(crate) mod continuity;
