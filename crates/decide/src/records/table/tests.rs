//! Records decided once, in sorted batches, and read thereafter: a
//! second request decides only the keys not yet stored, reads never
//! decide, a replay agrees with the stored answers (and finds one that
//! doesn't), answers that don't fit their batch are refused whole.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

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
    assert!(t.missing([4, 5, 4]).keys().iter().map(Fresh::key).eq([&4]));
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
    let batch = t.missing([1, 2]);
    assert_eq!(
        t.insert(batch, vec![1]),
        Err(TableError::Count { keys: 2, values: 1 })
    );
    assert!(t.is_empty());

    // Two batches made before either is stored: the second would decide
    // key 2 again.
    let first = t.missing([1, 2]);
    let second = t.missing([2, 3]);
    t.insert(first, vec![1, 4]).unwrap();
    assert_eq!(t.insert(second, vec![4, 9]), Err(TableError::Twice(2)));
    assert_eq!(t.len(), 2);
    assert!(!t.contains(&3));

    // A failed decision stores nothing.
    let failed = t.fill([5, 6], |_| Err("out of budget"));
    assert_eq!(failed, Err(FillError::Decide("out of budget")));
    assert!(!t.contains(&5));
}

#[test]
fn disjoint_batches_store_in_any_order() {
    let mut t: Table<u32, u64> = Table::new();
    let empty = t.missing([]);
    assert_eq!(t.insert(empty, vec![]), Ok(()));
    let empty = t.missing([]);
    assert_eq!(
        t.insert(empty, vec![1]),
        Err(TableError::Count { keys: 0, values: 1 })
    );
    let first = t.missing([10, 5]);
    let second = t.missing([7, 4, 4, 11]);
    t.insert(second, vec![16, 49, 121]).unwrap();
    t.insert(first, vec![25, 100]).unwrap();
    let all: Vec<(u32, u64)> = t.iter().map(|(&k, &v)| (k, v)).collect();
    assert_eq!(all, [(4, 16), (5, 25), (7, 49), (10, 100), (11, 121)]);
    assert_eq!(t.range(4..=4).count(), 1);
    assert_eq!(t.range(11..=11).count(), 1);
    assert_eq!(t.range(..4).count(), 0);
    assert_eq!(t.range(12..).count(), 0);
    let at = |k| (Bound::Excluded(k), Bound::Excluded(k));
    assert_eq!(t.range(at(5)).count(), 0);
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
