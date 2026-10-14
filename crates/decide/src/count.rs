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
//! and the edge's ray record between them.

pub mod boxes;
pub mod degree;
pub mod pieces;
