//! The kernel's topology: the B-rep store with every field a body keeps
//! ([`brep`]); [`brep_min`], the minimal store that counting reads
//! (vertices, edges, coedges with their p-curves and seam bounds, loops
//! and faces) and the bit-exact joins its ribbons need;
//! and [`arrange2d`], a face's arrangement in its UV on straight spans:
//! rotation order, face cycles, nesting by a containment certificate,
//! regions and their windings.

pub mod arrange2d;
// Its rows are added by the constructions and the assembly, as they land.
#[allow(dead_code, reason = "moved ahead of the operations that build bodies")]
pub mod brep;
pub mod brep_min;
