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
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn par_map<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync + Send,
{
    use rayon::prelude::*;
    items.par_iter().map(f).collect()
}

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
