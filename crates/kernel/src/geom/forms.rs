//! The forms' constructors: each form's tiles on its lattice (the base
//! cell's net from the stored key and frame, and what the level cap
//! reads of it, [`super::lattice`]), and the canonical surfaces built of
//! them: the [`plane`], the circular and conic-chain cylinders
//! ([`cyl`]), the [`cone`], the [`sphere`], the ring and spindle
//! [`torus`], the [`revolved`] conic and [`affine`] images of every form;
//! and the [`ruled`] surfaces between two rails, which have no key and
//! no lattice.
//!
//! **Exact to rounding.** Every control point of a base tile is an
//! exact function of the stored key's values, the frame's stored values
//! and the cell, each coordinate rounded once to the nearest `f64`, ties
//! to even; weights are `1`, `√½` rounded, `½`, exact, or a spindle
//! torus's middle weights and a profile span's standard ones (ends 1,
//! [`super::condition`]), times `√½` on a revolved conic's corner rows,
//! rounded once. An affine image's base tile is its source's,
//! each stored coordinate mapped and rounded once more, weights kept. A
//! finer level's tile is its base tile's exact sub-patch rounded once
//! ([`super::lattice::Lattice::tile`]). So two constructions of one key
//! give the same bits, and a tile's distance from the form is the
//! rounding of its coordinates and of the frame (the module tests check
//! that against exact values and against the nets of OCCT's
//! `Convert_*ToBSplineSurface`).
//!
//! **Canonical surfaces.** A constructor builds the surface of the
//! form's canonical rectangle ([`super::lattice::Lattice::half`]) at the
//! base level: the plane's one tile, 180° of a cylinder or cone (two
//! angle cells by axial cell 0, which holds the coordinate box), a
//! conic-chain cylinder's whole profile by axial cell 0, 180° of a
//! sphere from pole to pole, 180° of a ring torus by half its tube; a
//! spindle torus's 180° by its whole outer part is at tube level 1; a
//! revolved conic's 180° by its whole profile; an affine image's its
//! source's mapped.

pub mod affine;
pub mod cone;
pub mod cyl;
pub mod plane;
pub mod revolved;
pub mod ruled;
pub mod sphere;
pub mod torus;

#[cfg(test)]
mod testing;
