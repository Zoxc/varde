//! The boolean operation. So far the snap ([`snap`]): tangency clusters
//! formed from the leaves that certify Near or Contact, rewritten onto
//! one side; the face arrangements' coincident arcs and zero-length
//! edges ([`arrange`]); and the label audit of a planar face's regions
//! ([`classify`]).

pub mod arrange;
pub mod classify;
pub mod snap;
