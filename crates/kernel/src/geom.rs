//! The kernel's geometry: the [`tile`], one biquadratic rational patch
//! of a tiled surface, with its certified evaluation, splits, hull box
//! and normal cone; the [`form`]s a surface can be exactly (plane,
//! cylinder, sphere so far), named by their canonical keys; and their
//! [`lattice`]s, the grids of tiles anchored to those keys.

pub mod form;
pub mod lattice;
mod round;
pub mod tile;
