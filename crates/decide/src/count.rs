//! Counting: how many times a point's column passes a face, edge or
//! crossing, from the records. So far [`boxes`], the dyadic boxes a
//! face's parameter domain is cut into, each certified outside the face's
//! trim, inside it and clear of every trimming curve, or meeting trims:
//! then a strip box of one trimming curve (or a slit's), a corner box of a
//! vertex, or a thin box that refines; and [`degree`], the winding of a
//! box's shadow boundary about the query point's shadow, from certified
//! angle increments of its sides, so the degrees of any partition of a
//! face sum to its own; and [`pieces`], the strip pieces: a run of strip
//! boxes with its trimming curve's ribbon, their degree from the free
//! sides inside the trim, the cuts certified off the query point's ray,
//! and the edge's ray record between them, the corner pieces likewise,
//! and where an edge's crossing in a corner piece is placed; and [`s02`],
//! a face's layers above or below a point, from its pieces refined until
//! height-clear and, near the point, its layers' height records; and
//! [`cross`], the edge–face crossing counts `x12`, `x21` from those and
//! the edge pairs' `s11` by identity; and [`winding`], each vertex's
//! winding number in the other operand, from one seed per connected
//! component carried along every edge by its crossings.

pub mod boxes;
pub mod cross;
pub mod degree;
pub mod pieces;
pub mod s02;
pub mod winding;
pub mod xrec;

#[cfg(test)]
mod testing;
