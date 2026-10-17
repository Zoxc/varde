//! Test support for the Varde kernel, a dev-dependency only: oracles,
//! corpus generators and mutators. So far [`rng::Rng`], the seeded
//! generator the property tests draw their inputs from, and
//! [`ray_oracle`], the exact counting oracle for the kernel's crossing
//! counts.

pub mod ray_oracle;
pub mod rng;
