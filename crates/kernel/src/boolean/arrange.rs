//! A face's arrangement in the boolean: [`coincident`], the realisation
//! of arcs that run along existing edges or other arcs, the vertices
//! records place on edges, and zero-length edges, all by record, before
//! the face is arranged in its UV; then [`fold`], its zero-area regions
//! dropped or folded and the folds checked.

pub mod coincident;
pub mod fold;
