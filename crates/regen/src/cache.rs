//! What regenerating worked out before, per feature, so an edit reruns
//! only the features it changes and those after that depend on them.
//!
//! Each result is filed under a [`Key`]: a hash of everything it depends
//! on, the feature's own settings, the tolerance, and the keys of its
//! inputs (an extrude's sketch and where it's placed, a boolean's
//! operands, the solid a sketch's face or a revolve's axis edge is on).
//! A feature that didn't change, and whose inputs didn't, has the same
//! key, and its result is taken as it was.
//! The cache lives in the lane (a thread natively, the worker on the web).
//!
//! It's bounded by size: each result records about how many bytes it
//! holds ([`Entry::bytes`]) and the request that last used it. When a
//! request begins ([`Cache::begin`]) and the results add up to more than
//! the budget ([`BUDGET`], 256 MiB natively and 64 MiB on the web, per
//! lane and so per open document) or there are more than
//! [`MAX_ENTRIES`], the least recently used go first, until it's within
//! both again. What the request before used is never evicted, so
//! dragging a distance back and forth, or a draft answered after the
//! committed model, finds everything else still there, whatever the
//! budget; that set may go over the budget on its own. Within the
//! budget, undo and redo, and an option changed and changed back, find
//! what they had. Eviction goes by a counter bumped on every use, never
//! by the map's order, so which results go is the same on every run. A
//! request can also use what it doesn't need ([`Cache::keep`]), such as
//! the boolean of a body taken out of a cut, which putting it back asks
//! for.
//!
//! A body's mesh is kept with its picking tables, counted with it. The
//! model's mesh and tables, the shown bodies' joined with the body of
//! each part, are one more kind of result, a scene ([`Cache::scene`]),
//! filed by the shown bodies and their mesh keys: a request whose shown
//! bodies and tolerance didn't change (a sketch edit no body depends on, a sketch hidden or left out, a
//! draft that fails, or the committed model asked again after a draft)
//! is answered with the very same `Arc`, so neither the join nor,
//! natively, the renderer's upload (which keys its buffers by the `Arc`)
//! is done again. Scenes aren't counted in [`Cache::counts`], which stay
//! counts of features, but in [`Cache::joins`]. Nor are the other results
//! that aren't a feature's: a solid's topology (made once for drawing it
//! and resolving picks on it), and what the measure tool's picks measure
//! (see `src/inspect.rs`). Besides what the request
//! before used, the scene of the last answer without a draft is never
//! evicted either: however long a draft is dragged, the committed model's
//! scene stays, and putting the draft away finds it.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem::{size_of, size_of_val};
use std::sync::Arc;

use glam::DVec3;
use serde::Serialize;
use varde_document::Placement;
use varde_kernel::{RenderMesh, Solid, Topology};
use varde_sketch::{Profiles, TooComplex};

use crate::error_geometry::{ErrorGeometry, KernelFailure};
use crate::history::Failed;
use crate::inspect::{Gap, Kept};
use crate::picking::{Drawn, Scene};

/// How many bytes of results a cache holds before it evicts the least
/// recently used: several requests' worth of a large model natively; less
/// on the web, where a worker's memory never shrinks once grown.
pub(crate) const BUDGET: usize = if cfg!(target_arch = "wasm32") {
    64 << 20
} else {
    256 << 20
};

/// The most results a cache holds before it evicts the least recently
/// used, so tiny ones (whether a sketch solves, errors) can't pile up
/// without limit under the byte budget.
pub(crate) const MAX_ENTRIES: usize = 1 << 16;

/// What a result costs besides its own data: the map's slot, the `Arc`
/// and the allocations' bookkeeping, about.
const OVERHEAD: usize = 128;

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

    /// A sketch's placement, by its numbers' bits.
    pub(crate) fn placement(&mut self, placement: &Placement) -> &mut Keyer {
        for v in [placement.origin, placement.x, placement.y, placement.normal] {
            for number in v.to_array() {
                self.number(number.to_bits());
            }
        }
        self
    }

    /// An input's key.
    pub(crate) fn key(&mut self, key: Key) -> &mut Keyer {
        self.number(key.0[0]).number(key.0[1])
    }

    pub(crate) fn finish(&self) -> Key {
        Key(self.0.each_ref().map(Hasher::finish))
    }
}

/// A point or a direction an align names, as found on its body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Datum {
    /// The point, or the direction (not zero, not unit).
    pub(crate) at: DVec3,
    /// For a direction, whether it points out of its body: a flat face's
    /// normal or a round edge's axis (a rim's). Two such meet opposed by
    /// default.
    pub(crate) outward: bool,
}

/// An edge a scale names, as measured on its body: its length (the
/// whole chain's), and its ends where it's straight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EdgeLength {
    pub(crate) length: f64,
    /// Where it starts and ends, for a straight edge.
    pub(crate) line: Option<[DVec3; 2]>,
}

/// A result kept.
#[derive(Clone)]
enum Entry {
    /// A sketch's profiles.
    Profiles(Arc<Result<Profiles, TooComplex>>),
    /// Where a sketch on a face is, or why it isn't anywhere (with what
    /// to draw of where: a face that isn't flat).
    Placement(Result<Placement, Failed>),
    /// Where a straight edge's ends are, in the order it runs, or why
    /// it's no line there (with what to draw of it: an edge that isn't
    /// straight).
    Edge(Result<[DVec3; 2], Failed>),
    /// Where a move's axis or a mirror's plane is, as a point and a
    /// direction, or why it isn't anywhere (with what to draw of where).
    Reference(Result<[DVec3; 2], Failed>),
    /// A point or direction an align names, or why it isn't anywhere.
    Datum(Result<Datum, Failed>),
    /// A scale's edge as measured, or why it isn't anywhere.
    Length(Result<EdgeLength, Failed>),
    /// Whether a sketch solves.
    Solves(bool),
    /// A feature's tool solid, or why it has none (with what to draw of
    /// where, where it's the kernel's).
    Solid(Result<Arc<Solid>, Failed>),
    /// Whether two solids touch, or the kernel's failure.
    Touches(Result<bool, Arc<KernelFailure>>),
    /// A boolean of two solids, or the kernel's failure.
    Boolean(Result<Arc<Solid>, Arc<KernelFailure>>),
    /// A solid drawn, with its picking tables.
    Drawn(Arc<Drawn>),
    /// The model's mesh and picking tables: the shown bodies' joined.
    Scene(Scene),
    /// A solid's regions, chains and corners.
    Topology(Arc<Topology>),
    /// What a pick of the measure tool measures.
    Measure(Arc<Kept>),
    /// The minimum distance between two picks.
    Distance(Result<Gap, String>),
}

impl Entry {
    /// About how many bytes it holds: its data's lengths and
    /// [`OVERHEAD`]. Capacities aren't counted: a vector grown by pushes
    /// (a solid's arrays, a scene of several bodies joined) may hold up
    /// to about twice its length, so what the cache really holds can be
    /// up to about twice its count. Results shared between entries (a solid both a
    /// feature's tool and a boolean's operand) count once in each. Saturating,
    /// since the sizes come from what the user drew.
    fn bytes(&self) -> usize {
        let data = match self {
            Entry::Profiles(profiles) => match &**profiles {
                Ok(profiles) => profiles_bytes(profiles),
                Err(TooComplex) => 0,
            },
            Entry::Solid(Ok(solid)) | Entry::Boolean(Ok(solid)) => solid_bytes(solid),
            Entry::Solid(Err(failed))
            | Entry::Placement(Err(failed))
            | Entry::Edge(Err(failed))
            | Entry::Reference(Err(failed))
            | Entry::Datum(Err(failed))
            | Entry::Length(Err(failed)) => (failed.message.len())
                .saturating_add(failed.geometry.as_deref().map_or(0, ErrorGeometry::bytes)),
            Entry::Touches(Err(failure)) | Entry::Boolean(Err(failure)) => failure.bytes(),
            Entry::Drawn(drawn) => mesh_bytes(&drawn.mesh).saturating_add(drawn.bytes()),
            Entry::Scene(scene) => mesh_bytes(&scene.mesh).saturating_add(scene.picking.bytes()),
            Entry::Topology(topology) => topology_bytes(topology),
            Entry::Measure(kept) => kept.as_ref().as_ref().err().map_or(0, String::len),
            Entry::Distance(gap) => gap.as_ref().err().map_or(0, String::len),
            Entry::Solves(_)
            | Entry::Placement(Ok(_))
            | Entry::Edge(Ok(_))
            | Entry::Reference(Ok(_))
            | Entry::Datum(Ok(_))
            | Entry::Length(Ok(_))
            | Entry::Touches(Ok(_)) => 0,
        };
        data.saturating_add(OVERHEAD)
    }
}

fn solid_bytes(solid: &Solid) -> usize {
    let mesh = solid.mesh();
    (size_of_val(mesh.verts()))
        .saturating_add(size_of_val(mesh.edges()))
        .saturating_add(size_of_val(mesh.tris()))
        .saturating_add(size_of_val(mesh.faces()))
        .saturating_add(size_of_val(solid))
}

fn topology_bytes(topology: &Topology) -> usize {
    let mut bytes = (size_of_val(topology))
        .saturating_add(size_of_val(topology.regions()))
        .saturating_add(size_of_val(topology.chains()))
        .saturating_add(size_of_val(topology.corners()))
        .saturating_add(topology.triangles().saturating_mul(size_of::<u32>()));
    for region in topology.regions() {
        bytes = (bytes.saturating_add(size_of_val(&region.tris[..])))
            .saturating_add(size_of_val(&region.aliases[..]));
    }
    for chain in topology.chains() {
        bytes = bytes.saturating_add(size_of_val(&chain.halfedges[..]));
    }
    for corner in topology.corners() {
        bytes = bytes.saturating_add(size_of_val(&corner.regions[..]));
    }
    bytes
}

fn mesh_bytes(mesh: &RenderMesh) -> usize {
    (size_of_val(mesh.positions()))
        .saturating_add(size_of_val(mesh.normals()))
        .saturating_add(size_of_val(mesh.indices()))
        .saturating_add(size_of_val(mesh.face_ends()))
        .saturating_add(size_of_val(mesh.edge_vertices()))
        .saturating_add(size_of_val(mesh.edge_ends()))
        .saturating_add(size_of_val(mesh.edge_faces()))
        .saturating_add(size_of_val(mesh.corners()))
        .saturating_add(size_of_val(mesh.edge_corners()))
        .saturating_add(size_of_val(mesh.wire_vertices()))
        .saturating_add(size_of_val(mesh.wire_ends()))
        .saturating_add(size_of_val(mesh.part_ends()))
        .saturating_add(size_of_val(mesh))
}

fn profiles_bytes(profiles: &Profiles) -> usize {
    let mut bytes = (size_of_val(profiles))
        .saturating_add(size_of_val(&profiles.regions[..]))
        .saturating_add(size_of_val(&profiles.vertices[..]))
        .saturating_add(size_of_val(&profiles.open_ends[..]));
    for region in &profiles.regions {
        bytes = bytes.saturating_add(size_of_val(&region.outer[..]));
        for hole in &region.holes {
            bytes =
                (bytes.saturating_add(size_of_val(hole))).saturating_add(size_of_val(&hole[..]));
        }
        for outline in &region.outline {
            bytes = (bytes.saturating_add(size_of_val(outline)))
                .saturating_add(size_of_val(&outline[..]));
        }
    }
    bytes
}

/// A result held, with its size and when it was last used.
struct Slot {
    entry: Entry,
    /// [`Entry::bytes`], worked out once.
    bytes: usize,
    /// The number of the request that last used it ([`Cache::begin`]).
    request: u64,
    /// When it was last used, by a counter bumped on every use: the
    /// order eviction goes in.
    used: u64,
}

/// The results kept, see the module's docs.
pub struct Cache {
    slots: HashMap<Key, Slot>,
    /// What the slots' sizes add up to.
    bytes: usize,
    /// How many bytes it holds before evicting.
    budget: usize,
    /// The number of the request being answered, bumped by
    /// [`Cache::begin`].
    request: u64,
    /// The use counter ([`Slot::used`]).
    used: u64,
    /// The scene the last answer without a draft used: never evicted.
    committed: Option<Key>,
    /// How many results were found and how many worked out, and how many
    /// scenes joined, for tests.
    hits: usize,
    misses: usize,
    joins: usize,
}

impl Default for Cache {
    fn default() -> Cache {
        Cache::with_budget(BUDGET)
    }
}

impl Cache {
    /// A cache holding `budget` bytes of results besides those it never
    /// evicts: 0 keeps only what the request before used.
    pub(crate) fn with_budget(budget: usize) -> Cache {
        Cache {
            slots: HashMap::new(),
            bytes: 0,
            budget,
            request: 0,
            used: 0,
            committed: None,
            hits: 0,
            misses: 0,
            joins: 0,
        }
    }

    /// Starts answering a request: if it holds more than its budget, or
    /// more results than its cap (2^16), what the request before didn't
    /// use goes, least recently used first, until it's within both.
    pub fn begin(&mut self) {
        self.request = self.request.saturating_add(1);
        if self.within() {
            return;
        }
        let previous = self.request.saturating_sub(1);
        let mut old: Vec<(u64, Key)> = (self.slots.iter())
            .filter(|&(key, slot)| slot.request < previous && Some(*key) != self.committed)
            .map(|(key, slot)| (slot.used, *key))
            .collect();
        // Each use has its own count, so this is a total order.
        old.sort_unstable_by_key(|&(used, _)| used);
        for (_, key) in old {
            if self.within() {
                break;
            }
            self.remove(key);
        }
    }

    fn within(&self) -> bool {
        self.bytes <= self.budget && self.slots.len() <= MAX_ENTRIES
    }

    fn remove(&mut self, key: Key) {
        if let Some(slot) = self.slots.remove(&key) {
            self.bytes = self.bytes.saturating_sub(slot.bytes);
        }
    }

    /// Files `entry` under `key`, used now.
    fn insert(&mut self, key: Key, entry: Entry) {
        self.used = self.used.saturating_add(1);
        let bytes = entry.bytes();
        let slot = Slot {
            entry,
            bytes,
            request: self.request,
            used: self.used,
        };
        if let Some(old) = self.slots.insert(key, slot) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        self.bytes = self.bytes.saturating_add(bytes);
    }

    /// The slot filed under `key`, if there is one, marked used now.
    fn touch(&mut self, key: Key) -> Option<&Slot> {
        let slot = self.slots.get_mut(&key)?;
        self.used = self.used.saturating_add(1);
        slot.request = self.request;
        slot.used = self.used;
        Some(slot)
    }

    /// The result filed under `key`, if there is one, marked used.
    fn find(&mut self, key: Key) -> Option<Entry> {
        let entry = self.touch(key)?.entry.clone();
        self.hits += 1;
        Some(entry)
    }

    /// Marks the result filed under `key`, if there is one, used, without
    /// using it: what an edit leaves unused but likely to be asked for
    /// again, such as the boolean of a body taken out of a cut, which
    /// putting it back asks for.
    pub(crate) fn keep(&mut self, key: Key) {
        self.touch(key);
    }

    /// The result filed under `key`, or `make`'s, filed.
    fn entry(&mut self, key: Key, make: impl FnOnce() -> Entry) -> Entry {
        self.find(key).unwrap_or_else(|| {
            self.misses += 1;
            let entry = make();
            self.insert(key, entry.clone());
            entry
        })
    }

    /// Whether a result is filed under `key`, without using it.
    pub(crate) fn holds(&self, key: Key) -> bool {
        self.slots.contains_key(&key)
    }

    /// The result filed under `key`, or `make`'s, filed, without
    /// counting it in [`Cache::counts`]: what isn't a feature's result
    /// (a topology, a measure).
    fn uncounted(&mut self, key: Key, make: impl FnOnce() -> Entry) -> Entry {
        if let Some(slot) = self.touch(key) {
            return slot.entry.clone();
        }
        let entry = make();
        self.insert(key, entry.clone());
        entry
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

    pub(crate) fn placement(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<Placement, Failed>,
    ) -> Result<Placement, Failed> {
        match self.entry(key, || Entry::Placement(make())) {
            Entry::Placement(placement) => placement,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    pub(crate) fn edge(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<[DVec3; 2], Failed>,
    ) -> Result<[DVec3; 2], Failed> {
        match self.entry(key, || Entry::Edge(make())) {
            Entry::Edge(ends) => ends,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    pub(crate) fn reference(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<[DVec3; 2], Failed>,
    ) -> Result<[DVec3; 2], Failed> {
        match self.entry(key, || Entry::Reference(make())) {
            Entry::Reference(found) => found,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    pub(crate) fn datum(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<Datum, Failed>,
    ) -> Result<Datum, Failed> {
        match self.entry(key, || Entry::Datum(make())) {
            Entry::Datum(found) => found,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    pub(crate) fn length(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<EdgeLength, Failed>,
    ) -> Result<EdgeLength, Failed> {
        match self.entry(key, || Entry::Length(make())) {
            Entry::Length(found) => found,
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
        make: impl FnOnce() -> Result<Solid, Failed>,
    ) -> Result<Arc<Solid>, Failed> {
        match self.entry(key, || Entry::Solid(make().map(Arc::new))) {
            Entry::Solid(solid) => solid,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    pub(crate) fn touches(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<bool, KernelFailure>,
    ) -> Result<bool, Arc<KernelFailure>> {
        match self.entry(key, || Entry::Touches(make().map_err(Arc::new))) {
            Entry::Touches(touches) => touches,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    pub(crate) fn boolean(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<Solid, KernelFailure>,
    ) -> Result<Arc<Solid>, Arc<KernelFailure>> {
        match self.entry(key, || {
            Entry::Boolean(make().map(Arc::new).map_err(Arc::new))
        }) {
            Entry::Boolean(solid) => solid,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    /// A solid drawn, or `make`'s error, which isn't kept.
    pub(crate) fn mesh<E>(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<Drawn, E>,
    ) -> Result<Arc<Drawn>, E> {
        match self.find(key) {
            Some(Entry::Drawn(drawn)) => Ok(drawn),
            Some(_) => unreachable!("keys of different kinds differ"),
            None => {
                self.misses += 1;
                let drawn = Arc::new(make()?);
                self.insert(key, Entry::Drawn(Arc::clone(&drawn)));
                Ok(drawn)
            }
        }
    }

    /// A solid's topology, filed under `key`.
    pub(crate) fn topology(&mut self, key: Key, make: impl FnOnce() -> Topology) -> Arc<Topology> {
        match self.uncounted(key, || Entry::Topology(Arc::new(make()))) {
            Entry::Topology(topology) => topology,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    /// What a pick of the measure tool measures, filed under `key`.
    pub(crate) fn measure(&mut self, key: Key, make: impl FnOnce() -> Kept) -> Arc<Kept> {
        match self.uncounted(key, || Entry::Measure(Arc::new(make()))) {
            Entry::Measure(kept) => kept,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    /// The minimum distance between two picks, filed under `key`.
    pub(crate) fn distance(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Result<Gap, String>,
    ) -> Result<Gap, String> {
        match self.uncounted(key, || Entry::Distance(make())) {
            Entry::Distance(gap) => gap,
            _ => unreachable!("keys of different kinds differ"),
        }
    }

    /// The model's mesh and picking tables filed under the scene key
    /// `key`, or `join`'s, or its error, which isn't kept. Unless the answer is a draft's
    /// (`drafted`), its scene becomes the committed one, which is never
    /// evicted. Not counted in [`Cache::counts`].
    pub(crate) fn scene<E>(
        &mut self,
        key: Key,
        drafted: bool,
        join: impl FnOnce(&mut Cache) -> Result<Scene, E>,
    ) -> Result<Scene, E> {
        let scene = match self.touch(key).map(|slot| &slot.entry) {
            Some(Entry::Scene(scene)) => scene.clone(),
            Some(_) => unreachable!("keys of different kinds differ"),
            None => {
                let scene = join(self)?;
                self.joins += 1;
                self.insert(key, Entry::Scene(scene.clone()));
                scene
            }
        };
        if !drafted {
            self.committed = Some(key);
        }
        Ok(scene)
    }

    /// How many results were found filed, and how many were worked out,
    /// since it was made.
    pub fn counts(&self) -> (usize, usize) {
        (self.hits, self.misses)
    }

    /// How many model meshes were joined since it was made: requests whose
    /// scene was found don't count.
    pub fn joins(&self) -> usize {
        self.joins
    }

    /// About how many bytes of results it holds.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// How many results it holds, scenes included.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The size of the committed scene, which is never evicted, for tests.
    #[cfg(test)]
    pub(crate) fn committed_bytes(&self) -> usize {
        (self.committed)
            .and_then(|key| self.slots.get(&key))
            .map_or(0, |slot| slot.bytes)
    }

    /// The slots' sizes added up afresh, and what the request before
    /// used, for tests: [`Cache::bytes`] must equal the first.
    #[cfg(test)]
    pub(crate) fn audit(&self) -> (usize, usize) {
        let total = (self.slots.values()).fold(0usize, |sum, slot| sum.saturating_add(slot.bytes));
        let previous = self.request.saturating_sub(1);
        let protected = (self.slots.iter())
            .filter(|&(key, slot)| slot.request >= previous || Some(*key) == self.committed)
            .fold(0usize, |sum, (_, slot)| sum.saturating_add(slot.bytes));
        (total, protected)
    }
}
