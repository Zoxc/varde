//! A lane: requests handled away from the UI thread, one at a time, their
//! responses coming back as a stream for an iced `Subscription::run` to
//! yield as messages. The regeneration lanes (`varde-regen`) and the IO
//! lane (`varde-io`) are both built from what's here; each keeps its own
//! requests, the order it takes them in and its wire format.
//!
//! Natively a lane is a thread, see `thread`. On the web it's a Web
//! Worker: `page` is the page's side of one, `worker` the worker's, and
//! messages cross between them as bytes, copied out within a bound (see
//! `bytes`).
//! Either way the UI sends through a [`Transport`] and never waits, and
//! the target's [`Lane`] and [`Responses`] are re-exported here, so a lane
//! built from them names its types once for both.

pub mod bytes;
#[cfg(target_arch = "wasm32")]
mod message;
#[cfg(target_arch = "wasm32")]
pub mod page;
pub mod panic;
#[cfg(not(target_arch = "wasm32"))]
pub mod thread;
#[cfg(target_arch = "wasm32")]
pub mod worker;

#[cfg(target_arch = "wasm32")]
pub use page::{Lane, Responses};
#[cfg(not(target_arch = "wasm32"))]
pub use thread::{Lane, Responses};

/// Carries requests to wherever they're handled.
///
/// [`send`](Transport::send) never waits for the work. A transport used on
/// the web's main thread mustn't wait on a lock or a channel either, since
/// that traps there. The responses come back on the lane's stream, which
/// the app must expect late, out of order or, where the lane replaces
/// requests, not at all. Tests stand in for a lane with their own.
pub trait Transport<R> {
    /// Hands `request` over without waiting for it to be handled.
    fn send(&mut self, request: R);
}

/// Where a lane's requests wait to be handled: a queue, or a slot that a
/// newer request replaces the one in. Natively the lane's thread takes
/// them from here (see `thread`); on the web the page or the worker keeps
/// them.
pub trait Pending<R> {
    /// Takes `request`. Returns a request it replaced, or `request` itself
    /// if it's refused, for the caller to drop when it likes, e.g. outside
    /// a lock.
    fn push(&mut self, request: R) -> Option<R>;

    /// The request to handle next.
    fn pop(&mut self) -> Option<R>;

    fn is_empty(&self) -> bool;
}
