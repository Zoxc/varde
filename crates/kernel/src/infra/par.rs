//! The kernel's one parallel primitive, and the harness its tests use to
//! show results don't depend on the thread count.
//!
//! Every parallel step in the kernel is a pure map over input sorted by
//! stable keys, collected in input order. Floating-point reductions are
//! done afterwards, sequentially in key order, and ids are handed out in
//! one sequential pass over the collected results. Work is charged the
//! same way: each item charges its own ledger, and the charges are summed
//! in key order after collecting (`Work::pass` in the budget module). So a result is the same
//! to the bit whatever the number of threads or how the work was split,
//! natively (rayon) and on the web, where the worker runs the same maps
//! sequentially. This is the only module that knows about rayon.
//!
//! So it is also the only one allowed rayon's own scheduling, its thread
//! numbers and the clock (clippy.toml bans them everywhere else): the
//! clock only decides where an item runs, never what it gives.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "the kernel's one parallel primitive: rayon and the hand-off's clock live here"
)]

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
    par_map_indexed(items, |_, item| f(item))
}

/// [`par_map`] with each item's position in `items` passed to `f`
/// beside it.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn par_map_indexed<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync + Send,
{
    #[cfg(test)]
    match MODE.get() {
        Mode::AsUsual => {}
        Mode::AllOnPool => {
            use rayon::prelude::*;
            return items
                .par_iter()
                .enumerate()
                .map(|(i, item)| f(i, item))
                .collect();
        }
        Mode::Reversed => {
            let mut out: Vec<R> = items
                .iter()
                .enumerate()
                .rev()
                .map(|(i, item)| f(i, item))
                .collect();
            out.reverse();
            return out;
        }
    }
    let start = std::time::Instant::now();
    map_then_hand_off(items, f, |_| start.elapsed() > HAND_OFF)
}

/// [`par_map_indexed`] given when to hand the rest to the pool:
/// `hand_off(i)`, asked before each item `i` but the first, which always
/// runs here (so a thread preempted before it still makes progress).
#[cfg(not(target_arch = "wasm32"))]
fn map_then_hand_off<T, R, F>(items: &[T], f: F, mut hand_off: impl FnMut(usize) -> bool) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync + Send,
{
    use rayon::prelude::*;
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        if i > 0 && hand_off(i) {
            out.par_extend(
                items[i..]
                    .par_iter()
                    .enumerate()
                    .map(|(j, item)| f(i + j, item)),
            );
            break;
        }
        out.push(f(i, item));
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

/// [`par_map`] with each item's position in `items` passed to `f`
/// beside it.
#[cfg(target_arch = "wasm32")]
pub(crate) fn par_map_indexed<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync + Send,
{
    items
        .iter()
        .enumerate()
        .map(|(i, item)| f(i, item))
        .collect()
}

/// How [`par_map`] runs its items on this thread, for
/// [`assert_deterministic`]: fast items never take long enough to reach
/// the pool, so it hands them all over, or runs them backwards, to show
/// the result doesn't depend on where or in which order they run.
#[cfg(all(test, not(target_arch = "wasm32")))]
#[derive(Clone, Copy)]
enum Mode {
    AsUsual,
    AllOnPool,
    Reversed,
}

#[cfg(all(test, not(target_arch = "wasm32")))]
thread_local! {
    static MODE: std::cell::Cell<Mode> = const { std::cell::Cell::new(Mode::AsUsual) };
}

/// `f` run with [`par_map`] in `mode` on this thread.
#[cfg(all(test, not(target_arch = "wasm32")))]
fn in_mode<R>(mode: Mode, f: impl FnOnce() -> R) -> R {
    struct Restore(Mode);
    impl Drop for Restore {
        fn drop(&mut self) {
            MODE.set(self.0);
        }
    }
    let _restore = Restore(MODE.replace(mode));
    f()
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

/// Runs `f` on a 1-thread pool, on an 8-thread pool with [`par_map`]
/// handing all its items to the pool, and on a 1-thread pool with
/// `par_map` running its items backwards; asserts the three results are
/// the same to the bit, and returns the first. (`par_map`s nested in
/// another's items run as usual on the pool's other threads.)
///
/// The results are compared by their `Debug` text: Rust prints an `f64`
/// as the shortest decimal that reads back to the same bits, so two floats
/// print the same exactly when they are the same bits (NaNs aside, and
/// `-0.0` prints as such).
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn assert_deterministic<R: Send + std::fmt::Debug>(f: impl Fn() -> R + Sync) -> R {
    let one = on_threads(1, &f);
    let eight = on_threads(8, || in_mode(Mode::AllOnPool, &f));
    assert_eq!(
        format!("{one:?}"),
        format!("{eight:?}"),
        "1 and 8 threads disagree"
    );
    let reversed = on_threads(1, || in_mode(Mode::Reversed, &f));
    assert_eq!(
        format!("{one:?}"),
        format!("{reversed:?}"),
        "in order and backwards disagree"
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
    fn the_rest_is_handed_to_the_pool_in_order() {
        // Handing off before item 5: items 0 to 4 run on the calling
        // thread (not one of the pool's), the rest on the pool's,
        // collected in order all the same, each with its own position.
        let items: Vec<usize> = (0..64).collect();
        let ran = map_then_hand_off(
            &items,
            |i, &x| (i, x, rayon::current_thread_index().is_some()),
            |i| i == 5,
        );
        assert!(
            ran.iter()
                .enumerate()
                .all(|(k, &(i, x, on_pool))| i == k && x == k && on_pool == (k >= 5)),
            "{ran:?}"
        );
    }

    #[test]
    fn the_first_item_runs_here_and_slow_items_reach_the_pool() {
        // Items of 20 µs each: past HAND_OFF, the rest go to the pool,
        // whose last item runs there however long the calling thread was
        // preempted; the first runs here even if the clock has already
        // passed HAND_OFF before it.
        let items: Vec<usize> = (0..64).collect();
        let ran = par_map_indexed(&items, |i, &x| {
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_micros(20) {}
            (i, x, rayon::current_thread_index().is_some())
        });
        assert!(
            ran.iter()
                .enumerate()
                .all(|(k, &(i, x, _))| i == k && x == k)
        );
        assert!(!ran[0].2 && ran[63].2, "{ran:?}");
        let ran = map_then_hand_off(&items, |_, _| rayon::current_thread_index(), |_| true);
        assert!(
            ran[0].is_none() && ran[1..].iter().all(Option::is_some),
            "{ran:?}"
        );
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
    fn the_harness_sees_a_map_that_depends_on_the_order() {
        // Fast items never take long enough to reach the pool by
        // themselves, so they run in order on any number of threads; the
        // harness must still see ids handed out inside the map.
        use std::sync::atomic::{AtomicU32, Ordering};
        let items: Vec<u32> = (0..64).collect();
        assert_deterministic(|| {
            let next = AtomicU32::new(0);
            par_map(&items, |_| next.fetch_add(1, Ordering::Relaxed))
        });
    }

    #[test]
    #[should_panic(expected = "disagree")]
    fn the_harness_sees_a_difference() {
        assert_deterministic(rayon::current_num_threads);
    }
}
