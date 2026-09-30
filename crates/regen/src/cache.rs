//! What regenerating worked out before, per feature, so an edit reruns
//! only the features it changes and those after that depend on them.
//!
//! Each result is filed under a [`Key`]: a hash of everything it depends
//! on, the feature's own settings, the tolerance, and the keys of its
//! inputs (an extrude's sketch, a boolean's operands). A feature that didn't change, and whose
//! inputs didn't, has the same key, and its result is taken as it was.
//! The cache lives in the lane (a thread natively, the worker on the web)
//! and keeps what the last two requests used ([`Cache::begin`]), so
//! dragging a distance back and forth, or a draft answered after the
//! committed model, finds everything else still there.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use serde::Serialize;
use varde_kernel::{KernelError, RenderMesh, Solid};
use varde_sketch::{Profiles, TooComplex};

/// A hash of what a result depends on, 128 bits: two SipHash runs with
/// fixed keys over the same input, one of them salted, so different
/// inputs sharing a key is too unlikely to matter. Only ever compared
/// within one lane's run, so the hasher needn't be stable across builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Key([u64; 2]);

/// Builds a [`Key`] from the parts written in, in order, each told apart
/// from the next by its length.
pub(crate) struct Keyer([DefaultHasher; 2]);

impl Keyer {
    /// Starts a key for results of `kind`, so keys of different kinds
    /// never meet.
    pub(crate) fn new(kind: &str) -> Keyer {
        let mut salted = DefaultHasher::new();
        salted.write_u8(0x5a);
        let mut keyer = Keyer([DefaultHasher::new(), salted]);
        keyer.bytes(kind.as_bytes());
        keyer
    }

    pub(crate) fn bytes(&mut self, bytes: &[u8]) -> &mut Keyer {
        for hasher in &mut self.0 {
            bytes.hash(hasher);
        }
        self
    }

    /// `value` by its postcard encoding, as files and the web lane
    /// encode it.
    pub(crate) fn value(&mut self, value: &impl Serialize) -> &mut Keyer {
        let bytes = postcard::to_stdvec(value).expect("document values always serialize");
        self.bytes(&bytes)
    }

    pub(crate) fn number(&mut self, number: u64) -> &mut Keyer {
        self.bytes(&number.to_le_bytes())
    }

    /// An input's key.
    pub(crate) fn key(&mut self, key: Key) -> &mut Keyer {
        self.number(key.0[0]).number(key.0[1])
    }

    pub(crate) fn finish(&self) -> Key {
        Key(self.0.each_ref().map(Hasher::finish))
    }
}

/// A result kept.
#[derive(Clone)]
enum Entry {
    /// A sketch's profiles.
    Profiles(Arc<Result<Profiles, TooComplex>>),
    /// Whether a sketch solves.
    Solves(bool),
    /// An extrude's solid, or why it has none.
    Solid(Result<Arc<Solid>, String>),
    /// Whether two solids touch.
    Touches(Result<bool, KernelError>),
    /// A boolean of two solids.
    Boolean(Result<Arc<Solid>, KernelError>),
    /// A solid drawn.
    Mesh(Arc<RenderMesh>),
}

/// The results of the last two requests, see the module's docs.
#[derive(Default)]
pub struct Cache {
    /// Those of the request being answered.
    current: HashMap<Key, Entry>,
    /// Those of the request before, moved to `current` as they're used
    /// again.
    previous: HashMap<Key, Entry>,
    /// How many results were found and how many worked out, for tests.
    hits: usize,
    misses: usize,
}

impl Cache {
    /// Starts answering a request: what the one before last used goes.
    pub fn begin(&mut self) {
        self.previous = std::mem::take(&mut self.current);
    }

    /// The result filed under `key`, if there is one, then kept for the
    /// next request.
    fn find(&mut self, key: Key) -> Option<Entry> {
        let entry = match self.current.get(&key) {
            Some(entry) => entry.clone(),
            None => {
                let entry = self.previous.remove(&key)?;
                self.current.insert(key, entry.clone());
                entry
            }
        };
        self.hits += 1;
        Some(entry)
    }

    /// The result filed under `key`, or `make`'s, filed.
    fn entry(&mut self, key: Key, make: impl FnOnce() -> Entry) -> Entry {
        self.find(key).unwrap_or_else(|| {
            self.misses += 1;
            let entry = make();
            self.current.insert(key, entry.clone());
            entry
        })
    }

    pub(crate) fn profiles(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<Profiles, TooComplex>,
    ) -> Arc<Result<Profiles, TooComplex>> {
        match self.entry(key, || Entry::Profiles(Arc::new(make()))) {
            Entry::Profiles(profiles) => profiles,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    pub(crate) fn solves(&mut self, key: Key, make: impl FnOnce() -> bool) -> bool {
        match self.entry(key, || Entry::Solves(make())) {
            Entry::Solves(solves) => solves,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    pub(crate) fn solid(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<Solid, String>,
    ) -> Result<Arc<Solid>, String> {
        match self.entry(key, || Entry::Solid(make().map(Arc::new))) {
            Entry::Solid(solid) => solid,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    pub(crate) fn touches(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<bool, KernelError>,
    ) -> Result<bool, KernelError> {
        match self.entry(key, || Entry::Touches(make())) {
            Entry::Touches(touches) => touches,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    pub(crate) fn boolean(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<Solid, KernelError>,
    ) -> Result<Arc<Solid>, KernelError> {
        match self.entry(key, || Entry::Boolean(make().map(Arc::new))) {
            Entry::Boolean(solid) => solid,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    /// A mesh, or `make`'s error, which isn't kept.
    pub(crate) fn mesh<E>(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<RenderMesh, E>,
    ) -> Result<Arc<RenderMesh>, E> {
        match self.find(key) {
            Some(Entry::Mesh(mesh)) => Ok(mesh),
            Some(_) => unreachable!("keys of different kinds differ"),
            None => {
                self.misses += 1;
                let mesh = Arc::new(make()?);
                self.current.insert(key, Entry::Mesh(Arc::clone(&mesh)));
                Ok(mesh)
            }
        }
    }

    /// How many results were found filed, and how many were worked out,
    /// since it was made.
    pub fn counts(&self) -> (usize, usize) {
        (self.hits, self.misses)
    }
}
