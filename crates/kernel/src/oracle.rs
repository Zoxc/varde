//! The counting on whole solids checked against the high-precision ray
//! oracle (`varde_testkit::ray_oracle`): boxes, half cylinders and half
//! sphere zones on the kernel's plane, cylinder and sphere tiles, built
//! in the minimal store with perturbed p-curves ([`solids`]), counted
//! from their records ([`world`]) at random rigid placements and in the
//! adversarial families ([`runs`]), every per-vertex winding, `x12` and
//! `x21` compared with the oracle's casts against the counting surface,
//! ties against `A` moved by the perturbation.
//!
//! Quick by default; `VARDE_TESTS=full` runs more placements, and
//! `VARDE_ORACLE_RUNS=<n>` runs `n` placements (release build advised)
//! and prints the report.

mod families;
mod runs;
mod solids;
mod tests;
mod world;
