//! Test support for the Varde kernel, a dev-dependency only: oracles,
//! corpus generators and mutators (plan §11). So far only [`rng::Rng`],
//! the seeded generator the property tests draw their inputs from.

pub mod rng;
