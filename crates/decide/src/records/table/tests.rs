//! Records decided once, in sorted batches, and read thereafter: a
//! second request decides only the keys neither stored nor claimed by a
//! batch in flight, reads never decide, a replay agrees with the stored
//! answers (and finds one that doesn't), answers that don't fit their
//! batch are refused whole, a dropped batch gives its keys back, a key
//! holding `NONE` is refused before anything is claimed.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::ids::{EdgeId, NONE};
use crate::records::keys::XKey;

/// Test keys: `k` names the record of edge `k` of `A` against edge 0 of
/// `B`, so `NONE` is a key holding the id `NONE`.
impl Key for u32 {
    fn record(self) -> RecordKey {
        RecordKey::X(XKey {
            e: EdgeId(self),
            g: EdgeId(0),
        })
    }
}

/// A record whose answer is a function of its key: the key's square.
fn square(k: &Fresh<u32>) -> u64 {
    u64::from(**k) * u64::from(**k)
}

/// Fills `t` with `wanted`, counting each key decided in `decided`.
fn fill(t: &mut Table<u32, u64>, wanted: &[u32], decided: &AtomicUsize) {
    t.fill(wanted.iter().copied(), |keys| {
        let sorted = keys.windows(2).all(|w| w[0].key() < w[1].key());
        assert!(sorted, "a batch is sorted, each key once");
        decided.fetch_add(keys.len(), Ordering::Relaxed);
        Ok::<_, ()>(keys.iter().map(square).collect())
    })
    .unwrap();
}

#[test]
fn each_key_is_decided_once_and_read_after() {
    let decided = AtomicUsize::new(0);
    let mut t = Table::new();
    fill(&mut t, &[7, 3, 7, 1, 3], &decided);
    assert_eq!(decided.load(Ordering::Relaxed), 3);
    // Asking again decides only what's new; the batch merges in order.
    fill(&mut t, &[3, 5, 1, 9], &decided);
    assert_eq!(decided.load(Ordering::Relaxed), 5);
    fill(&mut t, &[9, 7], &decided);
    assert_eq!(decided.load(Ordering::Relaxed), 5);
    let all: Vec<(u32, u64)> = t.iter().map(|(&k, &v)| (k, v)).collect();
    assert_eq!(all, [(1, 1), (3, 9), (5, 25), (7, 49), (9, 81)]);
    // Reads read: a missing key is no cue to decide it.
    assert_eq!(t.get(&5), Some(&25));
    assert_eq!(t.get(&4), None);
    assert_eq!(decided.load(Ordering::Relaxed), 5);
    assert!(
        t.missing([4, 5, 4])
            .unwrap()
            .keys()
            .iter()
            .map(Fresh::key)
            .eq([&4])
    );
    assert_eq!(t.replay(square), Ok(()));
}

#[cfg(debug_assertions)]
#[test]
fn reads_are_counted_and_survive_later_batches() {
    let decided = AtomicUsize::new(0);
    let mut t = Table::new();
    fill(&mut t, &[2, 4], &decided);
    for _ in 0..3 {
        assert_eq!(t.get(&4), Some(&16));
    }
    assert_eq!(t.range(3..).count(), 1);
    fill(&mut t, &[1, 3, 5], &decided);
    assert_eq!(t.reads(&4), Some(4));
    assert_eq!(t.reads(&2), Some(0));
    assert_eq!(t.reads(&3), Some(0));
    assert_eq!(t.reads(&6), None);
    // A replay decides again but reads nothing.
    assert_eq!(t.replay(square), Ok(()));
    assert_eq!(t.reads(&4), Some(4));
}

#[test]
fn a_replay_finds_the_first_answer_that_differs() {
    let decided = AtomicUsize::new(0);
    let mut t = Table::new();
    fill(&mut t, &[1, 2, 3, 4], &decided);
    let off = |k: &Fresh<u32>| if **k >= 3 { 0 } else { square(k) };
    assert_eq!(
        t.replay(off),
        Err(Mismatch {
            key: 3,
            stored: 9,
            replayed: 0
        })
    );
}

#[test]
fn ranges_read_the_records_a_prefix_leads() {
    let decided = AtomicUsize::new(0);
    let mut t = Table::new();
    fill(&mut t, &[10, 11, 12, 20, 21, 30], &decided);
    let keys = |r: Vec<(&u32, &u64)>| r.into_iter().map(|(&k, _)| k).collect::<Vec<_>>();
    assert_eq!(keys(t.range(20..30).collect()), [20, 21]);
    assert_eq!(keys(t.range(..=11).collect()), [10, 11]);
    assert_eq!(keys(t.range(25..).collect()), [30]);
    assert_eq!(
        keys(t.range((Bound::Excluded(21), Bound::Unbounded)).collect()),
        [30]
    );
    assert!(t.range(13..20).next().is_none());
    let (lo, hi) = (30, 20);
    assert!(t.range(lo..hi).next().is_none());
}

#[test]
fn answers_that_dont_fit_their_batch_are_refused_whole() {
    let mut t: Table<u32, u64> = Table::new();
    let batch = t.missing([1, 2]).unwrap();
    assert!(t.in_flight(&1));
    assert_eq!(
        t.insert(batch, vec![1]),
        Err(TableError::Count { keys: 2, values: 1 })
    );
    assert!(t.is_empty());
    // The refused batch gave its keys back: the next request claims them.
    assert!(!t.in_flight(&1));
    assert_eq!(t.missing([2, 1]).unwrap().len(), 2);

    // A failed decision stores nothing and gives its keys back.
    let failed = t.fill([5, 6], |_| Err("out of budget"));
    assert_eq!(failed, Err(FillError::Decide("out of budget")));
    assert!(!t.contains(&5) && !t.in_flight(&5));

    // A batch is stored only by the table that made it.
    let mut other: Table<u32, u64> = Table::new();
    let batch = other.missing([7]).unwrap();
    assert_eq!(t.insert(batch, vec![49]), Err(TableError::Foreign));
    assert!(!t.contains(&7) && !other.in_flight(&7));
}

/// Two batches made before either is stored: the second leaves the key
/// the first claimed to it and waits for it, so the key is decided once,
/// whichever is stored first.
#[test]
fn a_key_in_flight_is_waited_for_never_decided_again() {
    for first_stored in [true, false] {
        let decided = AtomicUsize::new(0);
        let decide = |keys: &[Fresh<u32>]| -> Vec<u64> {
            decided.fetch_add(keys.len(), Ordering::Relaxed);
            keys.iter().map(square).collect()
        };
        let mut t: Table<u32, u64> = Table::new();
        let first = t.missing([1, 2]).unwrap();
        let second = t.missing([3, 2, 2]).unwrap();
        assert!(second.keys().iter().map(Fresh::key).eq([&3]));
        assert_eq!(second.waits(), [2]);
        let (a, b) = (decide(first.keys()), decide(second.keys()));
        if first_stored {
            t.insert(first, a).unwrap();
            t.insert(second, b).unwrap();
        } else {
            t.insert(second, b).unwrap();
            // The waited key isn't readable until its batch is stored.
            assert_eq!(t.get(&2), None);
            assert!(t.in_flight(&2));
            t.insert(first, a).unwrap();
        }
        assert_eq!(decided.load(Ordering::Relaxed), 3);
        let all: Vec<(u32, u64)> = t.iter().map(|(&k, &v)| (k, v)).collect();
        assert_eq!(all, [(1, 1), (2, 4), (3, 9)]);
        assert!(!t.in_flight(&2));
        assert!(t.missing([1, 2, 3]).unwrap().is_empty());
    }

    // A waited key whose batch is dropped (its pass failed) is free
    // again: the next request decides it, once.
    let mut t: Table<u32, u64> = Table::new();
    let first = t.missing([1, 2]).unwrap();
    let second = t.missing([2, 3]).unwrap();
    assert_eq!(second.waits(), [2]);
    drop(first);
    let values = second.keys().iter().map(square).collect();
    t.insert(second, values).unwrap();
    let third = t.missing([1, 2, 3]).unwrap();
    assert!(third.keys().iter().map(Fresh::key).eq([&1, &2]));
    assert!(third.waits().is_empty());
}

/// Batches in flight at once, each decided on its own thread: every key
/// any of them wants is decided exactly once, whatever the number of
/// threads and the order the batches are stored in.
#[test]
fn overlapping_batches_on_many_threads_decide_each_key_once() {
    for threads in [1usize, 2, 4, 8] {
        let decided: Vec<AtomicUsize> = (0..64).map(|_| AtomicUsize::new(0)).collect();
        let mut t: Table<u32, u64> = Table::new();
        // Batch `i` wants keys `4i .. 4i + 12`: each overlaps the next two.
        let batches: Vec<Batch<u32>> = (0..threads as u32)
            .map(|i| t.missing(4 * i..4 * i + 12).unwrap())
            .collect();
        let answers: Vec<Vec<u64>> = std::thread::scope(|s| {
            let handles: Vec<_> = batches
                .iter()
                .map(|b| {
                    let decided = &decided;
                    s.spawn(move || {
                        b.keys()
                            .iter()
                            .map(|k| {
                                decided[**k as usize].fetch_add(1, Ordering::Relaxed);
                                square(k)
                            })
                            .collect()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for (b, v) in batches.into_iter().zip(answers).rev() {
            t.insert(b, v).unwrap();
        }
        let wanted = 4 * threads + 8;
        assert_eq!(t.len(), wanted);
        for (k, n) in decided.iter().enumerate() {
            let n = n.load(Ordering::Relaxed);
            assert_eq!(n, usize::from(k < wanted), "key {k} on {threads} threads");
        }
        assert_eq!(t.replay(square), Ok(()));
    }
}

/// A key holding the id `NONE` is no record's key: the request is refused
/// whole, nothing claimed, decided or stored.
#[test]
fn a_key_holding_none_is_refused() {
    let mut t: Table<u32, u64> = Table::new();
    assert_eq!(
        t.missing([1, NONE, 2]).unwrap_err(),
        TableError::NoneId(NONE)
    );
    assert!(!t.in_flight(&1));
    let decided = AtomicUsize::new(0);
    let out = t.fill([NONE - 1, NONE], |keys| {
        decided.fetch_add(keys.len(), Ordering::Relaxed);
        Ok::<_, ()>(keys.iter().map(square).collect())
    });
    assert_eq!(out, Err(FillError::Table(TableError::NoneId(NONE))));
    assert_eq!(decided.load(Ordering::Relaxed), 0);
    assert!(t.is_empty() && !t.in_flight(&(NONE - 1)));
    // The highest id there is, `NONE - 1`, is a key like any.
    let decided = AtomicUsize::new(0);
    fill(&mut t, &[NONE - 1], &decided);
    assert_eq!(t.get(&(NONE - 1)), Some(&square(&Fresh(NONE - 1))));
}

/// Reads from many threads at once all count, whatever the interleaving.
#[cfg(debug_assertions)]
#[test]
fn parallel_reads_all_count() {
    let decided = AtomicUsize::new(0);
    let mut t = Table::new();
    fill(&mut t, &[4, 5, 6], &decided);
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                for _ in 0..1000 {
                    assert_eq!(t.get(&5), Some(&25));
                    assert_eq!(t.range(4..=5).count(), 2);
                }
            });
        }
    });
    assert_eq!(t.reads(&5), Some(16_000));
    assert_eq!(t.reads(&4), Some(8_000));
    assert_eq!(t.reads(&6), Some(0));
}
