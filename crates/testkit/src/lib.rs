//! Test support for the Varde kernel, a dev-dependency only: oracles,
//! corpus generators and mutators. So far [`rng::Rng`], the seeded
//! generator the property tests draw their inputs from,
//! [`ray_oracle`], the exact counting oracle for the kernel's crossing
//! counts, [`sheets`], the snap's fixtures: pairs of bodies within a
//! small offset of each other, and [`rational_oracle`], the exact oracle
//! for them.

pub mod rational_oracle;
pub mod ray_oracle;
pub mod rng;
pub mod sheets;
