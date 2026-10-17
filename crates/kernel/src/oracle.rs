//! The counting on whole solids checked against the high-precision ray
//! oracle (`varde_testkit::ray_oracle`): boxes, half cylinders and half
//! sphere zones on the kernel's plane, cylinder and sphere tiles, built
//! in the minimal store with perturbed p-curves ([`solids`]), counted
//! from their records ([`world`]) at random rigid placements and in the
//! adversarial families ([`families`]), every per-vertex winding, `x12`
//! and `x21` compared with the oracle's casts against the counting
//! surface, ties against `A` moved by the perturbation. Each adversarial
//! placement is also counted with `A` moving along `−d`: where it is tied
//! in the counting's own records (the family of exact ties on key
//! planes) the two counts must differ exactly where the oracle's answers
//! with `A` moved by `±2^-28·d` do.
//!
//! Quick by default; `VARDE_TESTS=full` runs more placements, and
//! `VARDE_ORACLE_RUNS=<n>` runs `n` placements (release build advised)
//! and prints the report; `VARDE_ORACLE_FLIP` adds the `±d` check to the
//! random placements.

mod families;
mod runs;
mod solids;
mod tests;
mod world;
