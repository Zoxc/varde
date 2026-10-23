//! The forms' constructors: each form's tiles on its lattice (the base
//! cell's net from the stored key and frame, and what the level cap
//! reads of it, [`super::lattice`]), and the canonical surfaces built of
//! them, so far for the [`plane`], the circular and conic-chain
//! cylinders ([`cyl`]) and the [`cone`].
//!
//! **Exact to rounding.** Every control point of a base tile is an
//! exact function of the stored key's values, the frame's stored values
//! and the cell, each coordinate rounded once to the nearest `f64`, ties
//! to even; weights are `1`, `√½` rounded, `½`, or a profile's stored
//! weights, exact. A finer level's tile is its base tile's exact
//! sub-patch rounded once ([`super::lattice::Lattice::tile`]). So two
//! constructions of one key give the same bits, and a tile's distance
//! from the form is the rounding of its coordinates and of the frame
//! (the module tests check that against exact values and against the
//! nets of OCCT's `Convert_*ToBSplineSurface`).
//!
//! **Canonical surfaces.** A constructor builds the surface of the
//! form's canonical rectangle ([`super::lattice::Lattice::half`]) at the
//! base level: the plane's one tile, 180° of a cylinder or cone (two
//! angle cells by axial cell 0, which holds the coordinate box), a
//! conic-chain cylinder's whole profile by axial cell 0.

pub mod cone;
pub mod cyl;
pub mod plane;

#[cfg(test)]
mod testing;
