//! Answering a panicking request: a lane that ended would leave the UI
//! waiting forever, so each lane turns a panic into its failure response,
//! worded the same way in all of them. Natively the lane's thread catches
//! it, see `thread::spawn`. A Web Worker can't catch its panic, so its
//! panic hook uses `message` too, see `worker::serve`.

use std::any::Any;
#[cfg(not(target_arch = "wasm32"))]
use std::panic::{self, AssertUnwindSafe};

/// Runs `handle(request)`, answering a panic with `failed(message)`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn catch<R, S>(
    handle: impl FnOnce(R) -> S,
    request: R,
    failed: impl FnOnce(String) -> S,
) -> S {
    panic::catch_unwind(AssertUnwindSafe(|| handle(request)))
        .unwrap_or_else(|panic| failed(message(&*panic)))
}

/// The message of a panic's payload, as the default hook prints it.
pub(crate) fn message(payload: &(dyn Any + Send)) -> String {
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown error");
    format!("internal error: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caught(handle: impl FnOnce(()) -> String) -> String {
        catch(handle, (), |error| error)
    }

    #[test]
    fn answers_without_a_panic() {
        assert_eq!(caught(|()| "done".to_owned()), "done");
    }

    #[test]
    fn answers_a_panic_with_its_message() {
        assert_eq!(
            caught(|()| panic!("on purpose")),
            "internal error: on purpose"
        );
        let n = 3;
        assert_eq!(caught(|()| panic!("{n} times")), "internal error: 3 times");
        assert_eq!(
            caught(|()| std::panic::panic_any(3)),
            "internal error: unknown error"
        );
    }
}
