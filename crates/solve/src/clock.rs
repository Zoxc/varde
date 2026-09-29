//! The solver's clock, for [`Budget::expired`](varde_sketch::Budget):
//! natively [`Instant`](std::time::Instant), which `wasm32-unknown-unknown`
//! doesn't have; in the Web Worker `performance.now()`.

use std::time::Duration;

/// Whether `limit` has passed since this was called.
#[cfg(not(target_arch = "wasm32"))]
pub fn deadline(limit: Duration) -> impl Fn() -> bool {
    use std::time::Instant;

    // Past the end of time, it never passes.
    let end = Instant::now().checked_add(limit);
    move || end.is_some_and(|end| Instant::now() >= end)
}

/// Whether `limit` has passed since this was called. A worker without
/// `performance` has no clock, so there only the solver's iterations bound
/// the work.
#[cfg(target_arch = "wasm32")]
pub fn deadline(limit: Duration) -> impl Fn() -> bool {
    use wasm_bindgen::JsCast;
    use web_sys::WorkerGlobalScope;

    let performance = js_sys::global()
        .dyn_into::<WorkerGlobalScope>()
        .ok()
        .and_then(|scope| scope.performance());
    let end = performance
        .as_ref()
        .map(|performance| performance.now() + limit.as_secs_f64() * 1e3);
    move || {
        performance
            .as_ref()
            .zip(end)
            .is_some_and(|(performance, end)| performance.now() >= end)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn a_deadline_passes_after_its_limit() {
        assert!(deadline(Duration::ZERO)());
        let later = deadline(Duration::from_secs(3600));
        assert!(!later());
        assert!(!deadline(Duration::MAX)());
    }
}
