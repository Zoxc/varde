//! Counting: how many times a point's column passes a face, edge or
//! crossing, from the records. So far [`boxes`], the dyadic boxes a
//! face's parameter domain is cut into, each certified outside the face's
//! trim, inside it and clear of every trimming curve, or meeting trims:
//! then a strip box of one trimming curve (or a slit's), a corner box of a
//! vertex, or a thin box that refines.

pub mod boxes;
