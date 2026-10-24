//! The kernel's geometry: the [`tile`], one biquadratic rational patch
//! of a tiled surface, with its certified evaluation, splits, hull box
//! and normal cone; the [`form`]s a surface can be exactly, named by
//! their canonical keys, each parameter correctly rounded by its
//! [`formkey`] route; their
//! [`lattice`]s, the grids of tiles anchored to those keys, built by
//! each form's constructor ([`forms`]) with their weights
//! [`condition`]ed, and the tiled [`surface`]s on them; the [`conic`]s
//! and their exact rational quadratic spans; a fitted surface's
//! intent and exact rows ([`fit_types`]); the [`curve`]s of edges, chains of rational
//! quadratic spans in space, and the [`pcurve`]s of coedges, chains of
//! Q, G, linear and iso spans in a surface's UV split at its tile
//! lines, an edge's conditioned together, their [`pcurve_exact`]
//! constructions where a curve's trace on a form is rational in its
//! parameter, or fitted to its edge's curve with the same parameter
//! ([`pcurve_fit`]); and the
//! [`certify`]d seam bound between a coedge's curve on its surface and
//! its edge's curve, and a curve's distance to an implicit surface; each
//! tile's [`implicit`] polynomial, and the analytic [`inverse`] from a
//! point to its surface's UV; and the non-decision [`query`]s the
//! measures read: trim membership and extrema, which no decision of an
//! operation reads.

pub mod certify;
pub mod condition;
pub mod conic;
pub mod curve;
pub mod fit_types;
pub mod form;
pub mod formkey;
pub mod forms;
pub mod implicit;
pub mod inverse;
pub mod lattice;
pub mod pcurve;
pub mod pcurve_exact;
pub mod pcurve_fit;
#[allow(dead_code, reason = "moved ahead of its callers, the measures")]
pub mod query;
mod round;
#[cfg(test)]
mod samples;
pub mod surface;
#[cfg(test)]
mod tests;
pub mod tile;
