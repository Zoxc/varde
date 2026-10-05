//! The kernel's one parallel primitive, and the harness its tests use to
//! show results don't depend on the thread count.
//!
//! Every parallel step in the kernel is a pure map over input sorted by
//! stable keys, collected in input order. Floating-point reductions are
//! done afterwards, sequentially in key order, and ids are handed out in
//! one sequential pass over the collected results. So a result is the same
//! to the bit whatever the number of threads or how the work was split,
//! natively (rayon) and on the web, where the worker runs the same maps
//! sequentially. This is the only module that knows about rayon.

/// `items.map(f)`, collected in the order of `items`: on rayon's pool
/// natively, sequentially on the web. `f` must be a pure function of its
/// item, so the result can't depend on scheduling.
///
/// Handing a map to the pool means waking its threads, tens of
/// microseconds and far more on a loaded machine, which most maps (a few
/// dozen cheap items) never make up: a small boolean's thousands of them
/// spent most of its time waiting on hand-offs. So the items run here, in
/// order, until they have taken [`HAND_OFF`], and only the rest go to the
/// pool.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn par_map<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync + Send,
{
    use rayon::prelude::*;
    let start = std::time::Instant::now();
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        if start.elapsed() > HAND_OFF {
            out.par_extend(items[i..].par_iter().map(&f));
            break;
        }
        out.push(f(item));
    }
    out
}

/// How long [`par_map`] works through its items alone before handing the
/// rest to the pool: past what waking the pool costs.
#[cfg(not(target_arch = "wasm32"))]
const HAND_OFF: std::time::Duration = std::time::Duration::from_micros(100);

/// `items.map(f)`, collected in the order of `items`: on rayon's pool
/// natively, sequentially on the web. `f` must be a pure function of its
/// item, so the result can't depend on scheduling.
#[cfg(target_arch = "wasm32")]
pub(crate) fn par_map<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync + Send,
{
    items.iter().map(f).collect()
}

/// `f` run on a pool of `threads` threads.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn on_threads<R: Send>(threads: usize, f: impl FnOnce() -> R + Send) -> R {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("a thread pool")
        .install(f)
}

/// Runs `f` on a 1-thread pool and on an 8-thread pool, asserts the two
/// results are the same to the bit, and returns one.
///
/// The results are compared by their `Debug` text: Rust prints an `f64`
/// as the shortest decimal that reads back to the same bits, so two floats
/// print the same exactly when they are the same bits (NaNs aside, and
/// `-0.0` prints as such).
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn assert_deterministic<R: Send + std::fmt::Debug>(f: impl Fn() -> R + Sync) -> R {
    let one = on_threads(1, &f);
    let eight = on_threads(8, &f);
    assert_eq!(
        format!("{one:?}"),
        format!("{eight:?}"),
        "1 and 8 threads disagree"
    );
    one
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn maps_in_order() {
        let items: Vec<u32> = (0..10_000).collect();
        let doubled = on_threads(8, || par_map(&items, |&x| x * 2));
        assert!(doubled.iter().enumerate().all(|(i, &x)| x == 2 * i as u32));
    }

    #[test]
    fn slow_items_are_handed_to_the_pool_in_order() {
        // Items of 20 µs each: the first few run on the calling thread,
        // the rest on the pool's, collected in order all the same.
        let items: Vec<u32> = (0..64).collect();
        let ran = par_map(&items, |&x| {
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_micros(20) {}
            (x, rayon::current_thread_index().is_some())
        });
        assert!(ran.iter().enumerate().all(|(i, &(x, _))| x == i as u32));
        assert!(!ran[0].1 && ran[63].1, "{ran:?}");
    }

    #[test]
    fn a_pure_map_is_deterministic() {
        let items: Vec<f64> = (0..10_000).map(|i| i as f64 * 0.1).collect();
        let sum = assert_deterministic(|| {
            let roots = par_map(&items, |x| x.sqrt());
            roots.iter().sum::<f64>()
        });
        assert!(sum > 0.0);
    }

    #[test]
    #[should_panic(expected = "disagree")]
    fn the_harness_sees_a_difference() {
        assert_deterministic(rayon::current_num_threads);
    }
}
