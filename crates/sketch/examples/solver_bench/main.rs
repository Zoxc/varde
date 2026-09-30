//! Times the sketch solver on a fully constrained plate of 342 curves (and
//! the same plate free to move), a chain of 100 tangent arcs, a chain of
//! ten splines joined smoothly with points on them, and one spline of a
//! hundred fit points with twenty: solving each from a little off,
//! analysing it, and a drag step on each. Also
//! finding profiles, of the plate, a plate of 6480 curves and a grid of
//! crossing lines.
//!
//! ```sh
//! cargo run --release -p varde-sketch --example solver_bench
//! ```
//!
//! The cases are in `bench.rs`, apart from the clock, so they can be timed
//! under wasm too; `notes/SketchImpl.md` has the numbers.

#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

mod bench;

use std::time::Instant;

fn main() {
    let start = Instant::now();
    let now = || start.elapsed().as_secs_f64() * 1000.0;
    bench::run(&now, &mut |name, ms| {
        let per = if name.ends_with("drag step") {
            ms / bench::DRAG_STEPS as f64
        } else {
            ms
        };
        println!("{name:<24} {per:>9.3} ms");
    });
}
