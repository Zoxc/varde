//! The constructions: bodies built from the caller's dimensions, exact
//! forms on their lattices, named from the caller's feature. So far the
//! [`primitives`]; for tests, bodies from closed triangle meshes
//! (`polyhedron`, built with the `testkit` feature).

#[cfg(any(test, feature = "testkit"))]
pub mod polyhedron;
pub mod primitives;
