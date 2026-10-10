# Tolerances, parallel maps and trigonometry

# Tolerances (`src/tolerance.rs`)

`Tolerance` holds the design's **fit** tolerance (how far a fitted curve or
patch may be from the true surfaces), in mm, within `MIN_FIT ..= MAX_FIT`
(`1e-5 ..= 1e-1`), default `1e-3`; `new` refuses anything else. The
**resolution** is a thousandth of it: the margin the hull rules keep, and
the scale where refinement stops (repair splits nothing less than 64
resolutions across). Neither ever decides that two things are the same
vertex or edge.

# Parallel maps and determinism (`src/par.rs`)

`par_map(items, f)` is `items.iter().map(f).collect()`, on rayon's pool
natively (`par_iter`, an indexed collect, so in input order) and plain
sequential on wasm (rayon is a native-only dependency). Natively it works
through the items on the calling thread until they have taken `HAND_OFF`
(100 µs), and only then hands the rest to the pool: waking the pool costs
tens of microseconds and far more on a loaded machine, which most maps
never made up (a small boolean's thousands of them waited on hand-offs
for most of its time). It is the only code that knows about rayon. The rules every parallel step follows:

- A pure map over input sorted by stable keys, collected in that order;
  never `for_each` into a shared sink.
- Floating-point reductions (sums, bounds) after collecting, sequentially in
  key order, never rayon's `sum`/`reduce`.
- Ids handed out in one sequential pass over the collected results.
- No iteration order from a hasher: sorted `Vec`s or `BTreeMap`s. A map
  only looked up in, never iterated, may be a `mesh::LookupMap` (a
  `HashMap` with a fixed hasher).
- Errors: the first in input order (collecting `Result`s does that).

Every angle the kernel turns into a place or a decision goes through
`trig` (below), so it has the same bits on every platform too.

Tests use `par::assert_deterministic(f)`, which runs `f` on a 1-thread and
an 8-thread pool and compares the results' `Debug` text (an `f64` prints as
the shortest decimal that reads back to the same bits, so equal text is
equal bits). `on_threads(n, f)` runs `f` on an `n`-thread pool.

## Deterministic trigonometry (`src/trig.rs`)

`+ − × ÷ √` are correctly rounded, so they give the same bits everywhere;
std's `sin`, `cos`, `atan2`, `ln`, `powi`, ... are not: they come from the
platform's maths library (glibc natively on Linux, the system's own on
macOS and Windows), whose last bits differ. On `wasm32-unknown-unknown`
std's are the `libm` crate's code already. `varde_kernel::trig` wraps
`libm` (pure Rust, pinned by `Cargo.lock`): `sin`, `cos`, `sin_cos`,
`tan`, `asin`, `acos`, `atan2`, `unit(angle)` (`(cos, sin)` as a `DVec2`)
and `angle(v)` (`atan2(v.y, v.x)`). So natively the kernel gets the web's
bits, and every platform the same. `sin_cos` is two calls, never a joint
`sincos`. A `libm` upgrade can change bits, on every platform at once.

The kernel's decisions use only `+ − × ÷ √`, exact signs and `trig`.
`crates/kernel/clippy.toml` keeps it so: `disallowed-methods` refuses
std's `f64` trigonometry, exponentials, logarithms, `powf`, `powi`,
`cbrt` and `hypot`, and glam's methods built on them (`DVec2::from_angle`,
`to_angle`, `angle_to`, the vectors' `angle_between`, `rotate_*` and
`slerp`, and the angle and rotation constructors of `DMat2`, `DMat3`,
`DQuat`, `DAffine2` and `DAffine3`). Tests that use std as an
independent reference, or to build inputs, allow
`clippy::disallowed_methods` in their module (`src/test_rng.rs` too, so
its inputs keep their bits); tests comparing bits with the code use
`trig`. Users: `Conic::arc`, the clean-up's Delaunay flip test (the angles
opposite a diagonal, by `atan2`) and `NormalCone::angle`.

Checked across platforms when this went in: a scratch `cdylib` on the
kernel, the sketch and regen's profile step hashed the profiles, their
conics, extrudes and joins of random sketches (lines, circles, arcs,
closed splines) natively (x86_64 Linux) and as `wasm32-unknown-unknown`
under node. The hashes were equal, and equal to the web's before the
change (which only moved native bits); before it, native differed.
Checked again with chains of two to four features: polygons given
fillets, chamfers (by distance and by angle) and offsets as edits,
extruded on tilted frames and joined, cut and intersected in turn, and
the solver on angle and length dimensions (systems up to about a hundred
variables): equal natively and on wasm, and at 1 and 8 threads. On
sketches at exact angles (multiples of π/4, `±0` coordinates, radii from
0.1 to 1e5) no edit or profile changed its outcome from before, only
last bits. `libm`'s own per-architecture code (x86, aarch64, wasm32) is
`sqrt`, `fma`, rounding and the like, all exact, so it can't split the
platforms either.
