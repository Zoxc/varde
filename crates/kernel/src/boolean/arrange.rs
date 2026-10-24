//! A face's arrangement in the boolean: [`edgeon`], the records of which
//! existing edges lie in a surface of the other operand, so that arcs
//! through them are pieces of them; [`coincident`], the realisation
//! of arcs that run along existing edges or other arcs, the vertices
//! records place on edges, and zero-length edges, all by record, before
//! the face is arranged in its UV; then [`fold`], its zero-area regions
//! dropped or folded and the folds checked.

pub mod coincident;
pub mod edgeon;
pub mod fold;
