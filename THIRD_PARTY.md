# Third-party code

Code in this repository derived from other projects: ported, translated
or adapted, so its copyright and license come with it. Each derived file
carries a `Derived from` header naming its origin and this table's row,
whose first line is `// Derived from <project> (<url>), commit <full
hash>, file <path>` (`sha256 <hash>` of the original file in place of the
commit for a source outside version control);
`tools/check_attribution.py` (run by `tools/ci.sh attribution`) checks
the two agree. The license texts are in `LICENSES/`. Algorithms only
taken as ideas, from papers or other code, are cited in the module docs
and not listed here.

| Row | File | Project | Pin | Original file | License | Copyright | Source | Taken | Changes |
|---|---|---|---|---|---|---|---|---|---|
| L2 | `crates/decide/src/records/shadow.rs` | Manifold | 3ad9216b2b62f4faaf3ea5342b4c99cdca70dbd2 | `src/boolean3.cpp` | Apache-2.0 (`LICENSES/Apache-2.0.txt`) | 2021 The Manifold Authors | https://github.com/elalish/manifold, commit 3ad9216b2b62f4faaf3ea5342b4c99cdca70dbd2 (no tag; `v3.5.1-66-g3ad9216b`); Manifold has no NOTICE file | lines 53–83: `Shadow01`, an edge's shadow against a vertex: the difference of the two end comparisons, then where the crossing lies along the ray | the end comparisons are point–vertex records decided once by `decide` under the perturbation, in a fixed `UP`/`r̂` frame with rays of opposite directions per operand; the crossing placed from those records, and from an exact point–line shadow row only where they don't settle it, instead of interpolating the edge; answers stored in record tables; no crossing point returned |
| L4 | `crates/decide/src/num/expansion.rs` | robust | 654f34cb8cdb3ae21bf59ef3472f92652942cd74 | `src/lib.rs` | MIT OR Apache-2.0, used under MIT (`LICENSES/MIT.txt`) | 2017 The Spade Developers; 2020 The GeoRust Project Developers | https://github.com/georust/robust, commit 654f34cb8cdb3ae21bf59ef3472f92652942cd74, released on crates.io as `robust-1.2.0.crate` (sha256 `4e27ee8bb91ca0adcf0ecb116293afa12d393f9c2b9b9cd54d33e8078fe19839`, packaged from that commit; its `src/lib.rs` is the commit's); a transcript of J. R. Shewchuk's public-domain `predicates.c` | lines 53, 1966–2030, 2061–2074, 2176–2186: `SPLITTER`, `scale_expansion_zeroelim`, `two_product`, `two_product_tail`, `split`, `two_product_presplit`, `two_sum`, `two_sum_tail`, `fast_two_sum_tail`, `fast_two_sum` | an `Exp` value over a smallvec instead of caller slices and lengths; zero as the empty expansion; empty operands handled; the two-sum tails inlined; the floating-point operations and their order unchanged |
| L5 | `crates/decide/src/num/expansion.rs` | Shewchuk's Robust Predicates | sha256 f8662c3f407d1c1c5dcd4dd49ea8b8ddd801a71827d65734206ddc3741792029 | `predicates.c` | public domain (placed there by its author in the file's header), no license text | none; author Jonathan Richard Shewchuk | https://www.cs.cmu.edu/~quake/robust.html, file `predicates.c` dated May 18, 1996, retrieved 2026-10-13 from https://www.cs.cmu.edu/afs/cs/project/quake/public/code/predicates.c (unversioned: the sha256 pins it) | `linear_expansion_sum_zeroelim` with the macros it uses, `Two_Sum` and `Fast_Two_Sum` | an `Exp` value instead of caller arrays and lengths; the merge read without reading past an operand's end; zero as the empty expansion; empty operands handled; the floating-point operations and their order unchanged |

Rows are numbered once and never reused.

## Dependencies

Crates the Varde kernel builds on, not derived code: each keeps its own
license, which `cargo deny` checks against `deny.toml`. The kernel
crates' direct dependencies:

| Crate | Version | License | Used for |
|---|---|---|---|
| `glam` | 0.30 | MIT OR Apache-2.0 | vectors and matrices |
| `libm` | 0.2.16 (pinned) | MIT | the trigonometry, the same bits on every platform |
| `rayon` | 1 | MIT OR Apache-2.0 | parallel maps (native builds) |
| `malachite` | 0.12.0 (pinned) | LGPL-3.0 | the big-float precision levels; the exact rounding of form keys and lattice control points; exact rationals in tests |
| `smallvec` | 1.16.2 (pinned) | MIT OR Apache-2.0 | the components of exact expansions |
| `geogram_predicates` | 0.4.0 (tests only) | LGPL-3.0 OR MIT, used under MIT | the expansion tests' oracle |
