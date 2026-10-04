//! The body a chamfer or a fillet changes in place: its own solid as the
//! features before it leave it, that solid's topology and cache key,
//! and the result put in its place, the body keeping its id.

use std::sync::Arc;

use varde_document::{BodyId, Document};
use varde_kernel::{Solid, Topology};

use super::{Evaluation, Failed, own_solids};
use crate::cache::{Cache, Key};
use crate::inspect;

/// A body a feature changes in place: its solid as the features before
/// it leave it, and that solid's topology (the one drawing it keeps,
/// [`inspect::topology`]).
pub(super) struct OwnBody<'a> {
    pub(super) body: BodyId,
    /// Its name, for messages.
    pub(super) name: &'a str,
    pub(super) solid: Arc<Solid>,
    /// The solid's cache key.
    pub(super) key: Key,
    pub(super) topology: Arc<Topology>,
}

impl<'a> OwnBody<'a> {
    /// The body `body` of `evaluation`, which must have a solid of its own
    /// ([`own_solids`]: one a join or a combine consumed fails, naming the
    /// body holding it).
    pub(super) fn take(
        document: &'a Document,
        body: BodyId,
        evaluation: &Evaluation,
        cache: &mut Cache,
    ) -> Result<Self, Failed> {
        own_solids(document, std::iter::once(body), evaluation)?;
        let name = document
            .body(body)
            .map_or("a body", |body| body.name.as_str());
        let made = (evaluation.bodies.iter())
            .find(|made| made.body == body)
            .expect("the body has a solid of its own");
        Ok(OwnBody {
            body,
            name,
            solid: Arc::clone(&made.solid),
            key: made.key,
            topology: inspect::topology(made, cache),
        })
    }

    /// Gives the body `result`, cached under `key`, in `evaluation`: it
    /// keeps its id.
    pub(super) fn replace(&self, evaluation: &mut Evaluation, result: Arc<Solid>, key: Key) {
        if let Some(made) = (evaluation.bodies.iter_mut()).find(|made| made.body == self.body) {
            made.solid = result;
            made.key = key;
        }
    }
}
