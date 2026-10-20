//! The kernel's topology: so far [`brep_min`], the minimal store that
//! counting reads (vertices, edges, coedges with their p-curves and seam
//! bounds, loops and faces) and the bit-exact joins its ribbons need;
//! and [`arrange2d`], a face's arrangement in its UV on straight spans:
//! rotation order, face cycles, nesting by a containment certificate,
//! regions and their windings.

pub mod arrange2d;
pub mod brep_min;
