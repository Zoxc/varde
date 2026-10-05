# Building Varde

## Desktop

Install a Rust toolchain ([rustup](https://rustup.rs)), then from the
repository root:

```sh
cargo run             # debug build
cargo run --release   # optimized build
```

## Browser

The browser build uses WebGL2 and is built with [trunk](https://trunkrs.dev):

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk
cd crates/web && trunk serve
```

Then open the address trunk prints. `trunk build --release` in the same
directory writes a static site to `crates/web/dist`.

## Tests

The workspace's default member is only the desktop binary, so pass
`--workspace` to test everything:

```sh
cargo test --workspace
VARDE_TESTS=full cargo test --workspace   # the long runs: every fuzz seed, slow bounds checks
VARDE_TEST_SEED=7 cargo test -p varde-app random_fillet   # replay one fuzz seed
```

The default (`VARDE_TESTS` unset, or `quick`) runs a short prefix of each
fuzz test's seeds. Any other value fails the tests, so a typo can't
quietly run the quick suite. The variable is read at runtime: switching it
rebuilds nothing.

Rendering tests draw offscreen on the GPU and are skipped when no GPU
adapter is found.
