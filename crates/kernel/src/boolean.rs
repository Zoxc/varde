//! The boolean operation. So far the broad phase ([`broad`]: the pairs
//! of leaves whose boxes meet, and the projection column); the snap ([`snap`]): tangency clusters
//! formed from the leaves that certify Near or Contact, rewritten onto
//! one side; the face arrangements' coincident arcs and zero-length
//! edges ([`arrange`]); and the label audit of a planar face's regions
//! ([`classify`]).

pub mod arrange;
// Its callers are the counting and the pair stages, as they land.
#[allow(dead_code, reason = "the boolean's stages call it as they land")]
pub(crate) mod broad;
pub mod classify;
pub mod snap;
