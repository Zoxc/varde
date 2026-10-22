//! A record table: the answers of one kind of record, stored by canonical
//! key, sorted, each decided once and read by every consumer.
//!
//! **Decided once.** A table never computes. Its keys are decided in
//! batches: [`Table::missing`] takes the keys a stage wants, sorts them,
//! drops repeats and those already stored, and gives them as a [`Batch`]
//! of [`Fresh`] keys; the kernel decides the batch in one charged pass
//! over those key-sorted items (each item charging its own work, the
//! costs summed in key order, so running out is the same on any number
//! of threads), and [`Table::insert`] stores the answers. [`Table::fill`]
//! does the three in one. A record's deciding function takes a [`Fresh`]
//! key, which only a table makes and only for a key it doesn't hold, so a
//! consumer holding the stage's table can't decide a stored record again:
//! it reads the table ([`Table::get`], [`Table::range`]) and, for a key
//! oriented the other way, reads the stored answer from its side. (A
//! second, empty table would hand out the key again: one table per kind
//! lives for the whole operation, every rung, and the owner of a kind's
//! table keeps that kind's deciding function to itself.)
//!
//! **In flight.** A key is decided and charged once, when first
//! requested: a batch claims its keys until it is stored or dropped. A
//! request made meanwhile for a claimed key doesn't decide (or charge)
//! it again: its batch lists the key as one it waits for
//! ([`Batch::waits`]), read once the claiming batch is stored. A batch
//! dropped unstored (its pass failed or ran out) gives its keys back, so
//! a later request decides them; a stored record is never decided again.
//!
//! **Keys.** A table's keys are record [`Key`]s; one holding the id
//! [`NONE`](crate::ids::NONE) for an entity is no record's key, and a
//! request holding one is refused whole ([`TableError::NoneId`]) before
//! anything is claimed or decided.
//!
//! **Cost.** Keys and answers are two sorted vectors: a read is a binary
//! search, a range a contiguous run. Storing a batch merges it in, linear
//! in the table's size, so a stage fills a table in a few large batches
//! (one per pass), not key by key.
//!
//! **Replay.** [`Table::replay`] decides every stored key again and
//! compares: the answer must be a function of the key alone, the
//! premise of reading instead of recomputing. Debug builds also count
//! each record's reads ([`Table::reads`]), so a test can see that every
//! consumer read the one stored answer, and that deciding happened once
//! per key.
//!
//! Nothing in a table depends on the order its batches were decided or
//! stored in, only on the keys.

use std::collections::BTreeSet;
use std::ops::{Bound, Deref, RangeBounds};
#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::records::keys::{Key, RecordKey};

/// A key the table didn't hold when it was handed out: what a record's
/// deciding function takes, so only a table's batches decide records.
/// Made by [`Table::missing`] and [`Table::replay`] only.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fresh<K>(K);

impl<K> Fresh<K> {
    /// The key.
    pub fn key(&self) -> &K {
        &self.0
    }
}

impl<K: Key> Fresh<K> {
    /// The record the key is of: what evidence names.
    pub fn record(&self) -> RecordKey {
        self.0.record()
    }
}

impl<K> Deref for Fresh<K> {
    type Target = K;

    fn deref(&self) -> &K {
        &self.0
    }
}

/// The keys a table's batches have claimed and not yet stored or given
/// back, shared with the batches so a dropped one gives its keys back.
type Flight<K> = Arc<Mutex<BTreeSet<K>>>;

/// The keys of one pass: sorted, without repeats, none in the table or
/// claimed by another batch when made; they stay claimed until the batch
/// is stored or dropped. Decide them in order and hand the answers to
/// [`Table::insert`].
#[derive(Debug)]
pub struct Batch<K: Ord> {
    keys: Vec<Fresh<K>>,
    waits: Vec<K>,
    flight: Flight<K>,
}

impl<K: Ord> Batch<K> {
    /// The keys, sorted: the pass's items.
    pub fn keys(&self) -> &[Fresh<K>] {
        &self.keys
    }

    /// The keys of the request another batch had claimed, sorted: being
    /// decided there, not here; readable once that batch is stored.
    pub fn waits(&self) -> &[K] {
        &self.waits
    }

    /// The number of keys.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether there is nothing to decide.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

impl<K: Ord> Drop for Batch<K> {
    /// Gives back the keys of a batch dropped unstored.
    fn drop(&mut self) {
        if self.keys.is_empty() {
            return;
        }
        let mut flight = self.flight.lock().unwrap_or_else(PoisonError::into_inner);
        for k in &self.keys {
            flight.remove(&k.0);
        }
    }
}

/// Why a request or a batch's answers were refused. Each is a bug of the
/// caller; the table is left as it was and the batch's keys given back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableError<K> {
    /// The answers don't match the batch's keys one for one.
    Count {
        /// The batch's keys.
        keys: usize,
        /// The answers given.
        values: usize,
    },
    /// The batch was made by another table.
    Foreign,
    /// The key holds the id `NONE` for an entity: no record's key.
    NoneId(K),
}

/// How [`Table::fill`] failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FillError<K, E> {
    /// Deciding the batch failed: nothing was stored.
    Decide(E),
    /// The request or the answers were refused.
    Table(TableError<K>),
}

/// A stored answer that deciding its key again doesn't give.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch<K, V> {
    /// The key.
    pub key: K,
    /// The stored answer.
    pub stored: V,
    /// The answer deciding it again gave.
    pub replayed: V,
}

/// The answers of one kind of record, by key, sorted by key (module docs).
#[derive(Debug)]
pub struct Table<K, V> {
    keys: Vec<K>,
    values: Vec<V>,
    /// Each record's reads, in debug builds.
    #[cfg(debug_assertions)]
    reads: Vec<AtomicU64>,
    /// The keys claimed by batches not yet stored or dropped.
    flight: Flight<K>,
}

impl<K, V> Default for Table<K, V> {
    fn default() -> Self {
        Table {
            keys: Vec::new(),
            values: Vec::new(),
            #[cfg(debug_assertions)]
            reads: Vec::new(),
            flight: Flight::default(),
        }
    }
}

impl<K: Key, V> Table<K, V> {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of records stored.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether no record is stored.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Whether `key`'s record is stored. Not a read: it says whether the
    /// key is decided, not what it was decided to.
    pub fn contains(&self, key: &K) -> bool {
        self.keys.binary_search(key).is_ok()
    }

    /// Whether `key` is claimed by a batch not yet stored or dropped:
    /// being decided, and not to be decided again.
    pub fn in_flight(&self, key: &K) -> bool {
        self.flight().contains(key)
    }

    /// The keys of `wanted` neither stored nor claimed, sorted, each once:
    /// the next batch to decide, its keys claimed until it is stored or
    /// dropped; the claimed ones it [waits](Batch::waits) for. Refused,
    /// nothing claimed, if a key holds the id `NONE`.
    pub fn missing(
        &mut self,
        wanted: impl IntoIterator<Item = K>,
    ) -> Result<Batch<K>, TableError<K>> {
        let mut want: Vec<K> = wanted.into_iter().filter(|k| !self.contains(k)).collect();
        if let Some(&k) = want.iter().find(|k| k.record().holds_none()) {
            return Err(TableError::NoneId(k));
        }
        want.sort_unstable();
        want.dedup();
        let mut flight = self.flight();
        let (waits, keys): (Vec<K>, Vec<K>) = want.into_iter().partition(|k| flight.contains(k));
        flight.extend(keys.iter().copied());
        drop(flight);
        Ok(Batch {
            keys: keys.into_iter().map(Fresh).collect(),
            waits,
            flight: Arc::clone(&self.flight),
        })
    }

    /// Stores `values`, the answers to `batch`'s keys in order, and ends
    /// their claim. Refused, the table unchanged and the keys given back,
    /// unless there is one answer per key and the batch is this table's.
    pub fn insert(&mut self, mut batch: Batch<K>, values: Vec<V>) -> Result<(), TableError<K>> {
        if !Arc::ptr_eq(&batch.flight, &self.flight) {
            return Err(TableError::Foreign);
        }
        if batch.keys.len() != values.len() {
            return Err(TableError::Count {
                keys: batch.keys.len(),
                values: values.len(),
            });
        }
        if batch.keys.is_empty() {
            return Ok(());
        }
        let batch_keys = std::mem::take(&mut batch.keys);
        {
            let mut flight = self.flight();
            for k in &batch_keys {
                flight.remove(&k.0);
            }
        }
        // Merge the two sorted runs; no key is in both: the batch's were
        // claimed, so no other batch stored them.
        let n = self.keys.len() + batch_keys.len();
        let mut keys = Vec::with_capacity(n);
        let mut vals = Vec::with_capacity(n);
        let mut old = std::mem::take(&mut self.keys)
            .into_iter()
            .zip(std::mem::take(&mut self.values))
            .peekable();
        let mut new = batch_keys.into_iter().map(|k| k.0).zip(values).peekable();
        #[cfg(debug_assertions)]
        let mut old_reads = std::mem::take(&mut self.reads).into_iter();
        #[cfg(debug_assertions)]
        let mut reads = Vec::with_capacity(n);
        loop {
            let (from_old, next) = match (old.peek(), new.peek()) {
                (Some((a, _)), Some((b, _))) if a < b => (true, old.next()),
                (_, Some(_)) => (false, new.next()),
                (Some(_), None) => (true, old.next()),
                (None, None) => break,
            };
            let Some((k, v)) = next else { break };
            #[cfg(debug_assertions)]
            reads.push(match from_old {
                true => old_reads.next().unwrap_or_default(),
                false => AtomicU64::new(0),
            });
            #[cfg(not(debug_assertions))]
            let _ = from_old;
            keys.push(k);
            vals.push(v);
        }
        self.keys = keys;
        self.values = vals;
        #[cfg(debug_assertions)]
        {
            self.reads = reads;
        }
        Ok(())
    }

    /// Decides the keys of `wanted` neither stored nor claimed, as one
    /// batch, and stores the answers: `decide` gets the batch's sorted
    /// keys (the pass's items) and gives an answer per key in order, or
    /// fails, in which case nothing is stored and the keys are given
    /// back. Keys claimed by another batch are left to it.
    pub fn fill<E>(
        &mut self,
        wanted: impl IntoIterator<Item = K>,
        decide: impl FnOnce(&[Fresh<K>]) -> Result<Vec<V>, E>,
    ) -> Result<(), FillError<K, E>> {
        let batch = self.missing(wanted).map_err(FillError::Table)?;
        if batch.is_empty() {
            return Ok(());
        }
        let values = decide(batch.keys()).map_err(FillError::Decide)?;
        self.insert(batch, values).map_err(FillError::Table)
    }

    /// `key`'s stored answer; `None` if it isn't decided (a consumer
    /// reading a record its stage didn't ask for: a bug, never a cue to
    /// decide it).
    pub fn get(&self, key: &K) -> Option<&V> {
        let i = self.keys.binary_search(key).ok()?;
        self.count_read(i);
        Some(&self.values[i])
    }

    /// The stored records whose keys are in `range`, in key order: the
    /// records of one point, say, when it leads the key.
    pub fn range<R: RangeBounds<K>>(&self, range: R) -> impl Iterator<Item = (&K, &V)> {
        let lo = match range.start_bound() {
            Bound::Included(k) => self.keys.partition_point(|x| x < k),
            Bound::Excluded(k) => self.keys.partition_point(|x| x <= k),
            Bound::Unbounded => 0,
        };
        let hi = match range.end_bound() {
            Bound::Included(k) => self.keys.partition_point(|x| x <= k),
            Bound::Excluded(k) => self.keys.partition_point(|x| x < k),
            Bound::Unbounded => self.keys.len(),
        };
        let hi = hi.max(lo);
        (lo..hi).map(|i| {
            self.count_read(i);
            (&self.keys[i], &self.values[i])
        })
    }

    /// Every stored record, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.range(..)
    }

    /// The stored keys from `from` on, in key order: which records are
    /// stored, not a read of any.
    pub fn keys_from<'a>(&'a self, from: &K) -> impl Iterator<Item = &'a K> + 'a {
        let lo = self.keys.partition_point(|x| x < from);
        self.keys[lo..].iter()
    }

    /// Takes the stored records of `keys` out of the table, giving them
    /// in key order (keys not stored skipped): the ladder's precision
    /// step, which decides a key again at a finer target, every record
    /// reading it with it. Only the owner of a kind's table, which keeps
    /// its deciding function, takes records out; the next request hands
    /// their keys out again.
    pub fn take(&mut self, keys: &[K]) -> Vec<(K, V)> {
        let mut out: Vec<K> = keys.iter().copied().filter(|k| self.contains(k)).collect();
        out.sort_unstable();
        out.dedup();
        if out.is_empty() {
            return Vec::new();
        }
        let n = self.keys.len();
        let mut keys_kept = Vec::with_capacity(n - out.len());
        let mut vals_kept = Vec::with_capacity(n - out.len());
        let mut taken = Vec::with_capacity(out.len());
        #[cfg(debug_assertions)]
        let mut reads = Vec::with_capacity(n - out.len());
        #[cfg(debug_assertions)]
        let mut old_reads = std::mem::take(&mut self.reads).into_iter();
        let old = std::mem::take(&mut self.keys)
            .into_iter()
            .zip(std::mem::take(&mut self.values));
        for (k, v) in old {
            #[cfg(debug_assertions)]
            let r = old_reads.next().unwrap_or_default();
            if out.binary_search(&k).is_ok() {
                taken.push((k, v));
            } else {
                keys_kept.push(k);
                vals_kept.push(v);
                #[cfg(debug_assertions)]
                reads.push(r);
            }
        }
        self.keys = keys_kept;
        self.values = vals_kept;
        #[cfg(debug_assertions)]
        {
            self.reads = reads;
        }
        taken
    }

    /// Decides every stored key again with `decide`, the function the
    /// table was filled with, and gives the first key, in key order, whose
    /// answer differs from the stored one. A check, for debug builds and
    /// tests: not a read, and nothing is stored.
    pub fn replay(&self, decide: impl Fn(&Fresh<K>) -> V) -> Result<(), Mismatch<K, V>>
    where
        V: PartialEq + Clone,
    {
        for (k, v) in self.keys.iter().zip(&self.values) {
            let again = decide(&Fresh(*k));
            if again != *v {
                return Err(Mismatch {
                    key: *k,
                    stored: v.clone(),
                    replayed: again,
                });
            }
        }
        Ok(())
    }

    /// How many times `key`'s record was read; `None` if it isn't stored.
    /// Debug builds only.
    #[cfg(debug_assertions)]
    pub fn reads(&self, key: &K) -> Option<u64> {
        let i = self.keys.binary_search(key).ok()?;
        Some(self.reads[i].load(Ordering::Relaxed))
    }

    fn flight(&self) -> std::sync::MutexGuard<'_, BTreeSet<K>> {
        self.flight.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[inline]
    fn count_read(&self, _i: usize) {
        #[cfg(debug_assertions)]
        self.reads[_i].fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
