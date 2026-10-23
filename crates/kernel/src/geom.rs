//! The kernel's geometry: the [`tile`], one biquadratic rational patch
//! of a tiled surface, with its certified evaluation, splits, hull box
//! and normal cone; the [`form`]s a surface can be exactly, named by
//! their canonical keys, each parameter correctly rounded by its
//! [`formkey`] route; their
//! [`lattice`]s, the grids of tiles anchored to those keys, built by
//! each form's constructor ([`forms`]), and the tiled [`surface`]s on
//! them; the [`conic`]s and their exact rational
//! quadratic spans; the [`curve`]s of edges and coedges, chains
//! of rational quadratic spans in space and in a surface's UV; and the
//! [`certify`]d seam bound between a coedge's curve on its surface and
//! its edge's curve.

pub mod certify;
pub mod conic;
pub mod curve;
pub mod form;
pub mod formkey;
pub mod forms;
pub mod lattice;
mod round;
pub mod surface;
pub mod tile;
