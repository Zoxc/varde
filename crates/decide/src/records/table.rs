//! A record table: the answers of one kind of record, stored by canonical
//! key, sorted, each decided once and read by every consumer.
//!
//! **Decided once.** A table never computes. Its keys are decided in
//! batches: [`Table::missing`] takes the keys a stage wants, sorts them,
//! drops repeats and those already stored, and gives them as a [`Batch`]
//! of [`Fresh`] keys; the kernel decides the batch in one charged pass
//! over those key-sorted items (each item charging its own work, the
//! costs summed in key order, so running out is the same on any number
//! of threads), and [`Table::insert`] stores the answers, refusing a key
//! already there. [`Table::fill`] does the three in one. A record's
//! deciding function takes a [`Fresh`] key, which only a table makes and
//! only for a key it doesn't hold, so a consumer holding the stage's
//! table can't decide a stored record again: it reads the table
//! ([`Table::get`], [`Table::range`]) and, for a key oriented the other
//! way, reads the stored answer from its side. (A second, empty table
//! would hand out the key again: the stage owning a kind's table keeps
//! that kind's deciding function to itself.)
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
//! A table belongs to one run of the stage that fills it; nothing in it
//! depends on the order its batches were decided in, only on the keys.

use std::ops::{Bound, Deref, RangeBounds};
#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicU64, Ordering};

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

impl<K> Deref for Fresh<K> {
    type Target = K;

    fn deref(&self) -> &K {
        &self.0
    }
}

/// The keys of one pass: sorted, without repeats, none in the table when
/// made. Decide them in order and hand the answers to [`Table::insert`].
#[derive(Debug, PartialEq, Eq)]
pub struct Batch<K> {
    keys: Vec<Fresh<K>>,
}

impl<K> Batch<K> {
    /// The keys, sorted: the pass's items.
    pub fn keys(&self) -> &[Fresh<K>] {
        &self.keys
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

/// Why a batch's answers weren't stored. Either is a bug of the caller;
/// the table is left as it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableError<K> {
    /// The answers don't match the batch's keys one for one.
    Count {
        /// The batch's keys.
        keys: usize,
        /// The answers given.
        values: usize,
    },
    /// The key was stored after the batch was made: it would be decided
    /// twice.
    Twice(K),
}

/// How [`Table::fill`] failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FillError<K, E> {
    /// Deciding the batch failed: nothing was stored.
    Decide(E),
    /// The answers couldn't be stored.
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
}

impl<K, V> Default for Table<K, V> {
    fn default() -> Self {
        Table {
            keys: Vec::new(),
            values: Vec::new(),
            #[cfg(debug_assertions)]
            reads: Vec::new(),
        }
    }
}

impl<K: Ord + Clone, V> Table<K, V> {
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

    /// The keys of `wanted` not yet stored, sorted, each once: the next
    /// batch to decide.
    pub fn missing(&self, wanted: impl IntoIterator<Item = K>) -> Batch<K> {
        let mut keys: Vec<K> = wanted.into_iter().filter(|k| !self.contains(k)).collect();
        keys.sort_unstable();
        keys.dedup();
        Batch {
            keys: keys.into_iter().map(Fresh).collect(),
        }
    }

    /// Stores `values`, the answers to `batch`'s keys in order. Refused,
    /// the table unchanged, unless there is one answer per key and no key
    /// was stored since the batch was made.
    pub fn insert(&mut self, batch: Batch<K>, values: Vec<V>) -> Result<(), TableError<K>> {
        if batch.keys.len() != values.len() {
            return Err(TableError::Count {
                keys: batch.keys.len(),
                values: values.len(),
            });
        }
        if let Some(k) = batch.keys.iter().find(|k| self.contains(k)) {
            return Err(TableError::Twice(k.0.clone()));
        }
        if batch.keys.is_empty() {
            return Ok(());
        }
        // Merge the two sorted runs; no key is in both.
        let n = self.keys.len() + batch.keys.len();
        let mut keys = Vec::with_capacity(n);
        let mut vals = Vec::with_capacity(n);
        let mut old = std::mem::take(&mut self.keys)
            .into_iter()
            .zip(std::mem::take(&mut self.values))
            .peekable();
        let mut new = batch.keys.into_iter().map(|k| k.0).zip(values).peekable();
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

    /// Decides the keys of `wanted` not yet stored, as one batch, and
    /// stores the answers: `decide` gets the batch's sorted keys (the
    /// pass's items) and gives an answer per key in order, or fails, in
    /// which case nothing is stored.
    pub fn fill<E>(
        &mut self,
        wanted: impl IntoIterator<Item = K>,
        decide: impl FnOnce(&[Fresh<K>]) -> Result<Vec<V>, E>,
    ) -> Result<(), FillError<K, E>> {
        let batch = self.missing(wanted);
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

    /// Decides every stored key again with `decide`, the function the
    /// table was filled with, and gives the first key, in key order, whose
    /// answer differs from the stored one. A check, for debug builds and
    /// tests: not a read, and nothing is stored.
    pub fn replay(&self, decide: impl Fn(&Fresh<K>) -> V) -> Result<(), Mismatch<K, V>>
    where
        V: PartialEq + Clone,
    {
        for (k, v) in self.keys.iter().zip(&self.values) {
            let again = decide(&Fresh(k.clone()));
            if again != *v {
                return Err(Mismatch {
                    key: k.clone(),
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

    #[inline]
    fn count_read(&self, _i: usize) {
        #[cfg(debug_assertions)]
        self.reads[_i].fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
