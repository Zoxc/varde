# User rules

- Committing. Whatever is uncommitted when you start stays uncommitted.
- Don't reference an uncommitted file from a committed one.
- Commit messages. Keep the title brief -- a line, not a paragraph -- and put what needs
  saying under it, ideally in one short paragraph
- Adding a todo entry is not a request to do it.
- Adding to the todo needs the user's permission.
- Prefer TOML for files, not JSON.
- Prefer headless testing
- Checked math on user data. Anything that reaches arithmetic from a form, a file or the
  command line gets a bound or a checked operation, never a bare `+`/`*` that can overflow.

# Agent notes

- **No change to the `.vrdp` format without the user's permission.** That
  covers the framing in `crates/io/src/vrdp.rs` and the serialized types
  (`Document`, `Sketch`, the auto-save payload, ...). Ask first, even
  when a change keeps old files working.
- **No file format versioning while the app is WIP.** The `.vrdp` format
  (`crates/io/src/vrdp.rs`) has a `version` field, but it stays at 1:
  don't bump it, and don't add migrations. Prefer changes that keep older
  files working, as `vrdp.rs`'s module docs describe (defaulted fields,
  new variants, names never reused); one that can't breaks old files,
  which is fine for now. Those docs also hold the design for once the
  format must be stable.
- **No wildcard deletes.** `rm -f dir/*` and other globbed or recursive
  deletes trigger permission prompts. For headless Firefox runs, give each
  run fresh directories in the scratchpad rather than clearing old ones: a
  copy of `crates/web/dist` to serve, a profile whose `user.js` sets
  `browser.download.folderList` 2, `browser.download.dir` to a new
  downloads directory and `browser.helperApps.neverAsk.saveToDisk`
  `application/octet-stream`. Downloads are `<name>.vrdp` (`Untitled.vrdp`;
  Firefox adds ` (1)` to repeats): check or delete them by that exact name.
  Reset OPFS from the page (`(await navigator.storage.getDirectory())
  .removeEntry('designs', {recursive: true})`), not by deleting profile
  files, and stop the server and browser by the PIDs saved at launch.

# Project

Varde is a CAD application in Rust on iced 0.14 and wgpu 27, built natively and
for the browser (wasm, WebGL2). The detailed design notes are in `agents/`; read
the relevant one before changing that area, and keep it in step when behaviour
it describes changes:

- `agents/viewport.md`: the viewport widget, `MeshFeed`, the regeneration lanes.
- `agents/sketch.md`: the sketch model, editing it, drawing it and the input
  on it in the viewport, hit testing and the drawing tools.
- `agents/files.md`: the IO lane, sidecar locks, auto-save, recovery, saving,
  closing and quitting.
- `agents/web-files.md`: the web IO worker, OPFS, pickers, downloads.
- `agents/file-format.md`: the `.vrdp` format: its blocks, reading past
  damage, saving, previews, records and the document they hold.
- `agents/features.md`: the features after sketches and extrudes (revolve
  so far): their document types and checks, the commands every kind
  shares, their regeneration and UI.
- `agents/kernel.md`: the geometry kernel: rational quadratic curves and
  patches, their splits, the fold check, exact arcs and cylinders.
- `notes/Kernel.md`: the kernel's high-level overview (ideas, invariants,
  the boolean pipeline, limits); `agents/kernel.md` holds the detail.
- `notes/Threading.md`: the threading plan (built steps and what's still open).
- `agents/crates.md`: the crate table and dependency graph, the web build's
  worker binaries, assets, and the roadmap by crate.

`README.md` is for users only: what the app is and how to use it. Build
instructions for people are in `BUILDING.md`.
Where any of these differs from the code, the code wins.

## Commands

The workspace's `default-members` is only `crates/binary`, so plain `cargo test`
or `cargo clippy` covers almost nothing; pass `--workspace`.

```sh
cargo run                                   # native app
cargo test --workspace                      # all tests, quick mode
VARDE_TESTS=full cargo test --workspace     # long runs: all fuzz seeds, slow bounds checks
VARDE_TEST_SEED=7 cargo test -p varde-app some_fuzz_test   # replay one seed
cargo test -p varde-io                      # one crate
cargo test -p varde-io some_test_name       # one test (substring match)
cargo clippy --workspace --all-targets
cargo check --workspace --target wasm32-unknown-unknown   # web-only code paths
cargo doc --workspace --no-deps --document-private-items  # link checks, on both targets
cd crates/web && trunk serve                # browser build
```

GPU tests (`crates/render/tests`, viewport tests in `varde-view`) render
offscreen via `pollster` and print "no GPU adapter, skipping" and pass when
there is no adapter.
`VARDE_TESTS` (unset/`quick` or `full`, anything else panics) and
`VARDE_TEST_SEED` are read by the `varde-testing` dev-dependency
(`crates/testing`); fuzz tests take their seeds from `varde_testing::seeds`.
Tests pass temporary directories (or none) to `varde_io::Stores` so they never
touch the user's config/data dirs.

The dev profile builds `varde-kernel` at `opt-level = 1` (its tests run about
8 times slower at 0), and `varde-sketch` too (over twice as slow at 0). To
debug one unoptimized, pass
`--config 'profile.dev.package.varde-kernel.opt-level=0'` (or `varde-sketch`).
Dev builds carry line tables only (dependencies none); for variables in a
debugger, add `--config 'profile.dev.debug="full"'`.

## Architecture

Crates in `crates/`, dependencies only pointing down:
`binary`/`web` → `app` → `view` → `render` → `kernel`; `document` → `kernel`,
`sketch`; `regen`, `solve` and `io` → `document` + `lane` (`solve` also
`sketch`, `io` also `kernel` for 3MF export); `view` also → `regen`, for the
picking tables. `kernel`, `sketch`, `document`, `lane`, `regen`, `solve`, `io`
have no UI code; `render` has no iced dependency.

- **State and update**: all app state lives in `varde-app` and is mutated only
  in iced's `update` on the UI thread. `document::Editor` applies `Command`s and
  keeps undo/redo; `Editor::generation()` keys derived data. `view` is pure
  functions of state emitting `Message`s.
- **Lanes** (`varde-lane`): heavy work runs off the UI thread as request/response
  lanes started by iced subscriptions whose stream first yields the sender, then
  responses as messages. Natively a thread; on the web a Web Worker with the
  same API (iced's wasm executor is the main thread, so `Task::perform` doesn't
  help there).
  - `regen`: one lane per open document, latest-wins single slot, responses
    tagged with the editor generation. The app's `MeshFeed` owns the transport
    and drops stale answers. Exports (welding the visible bodies for 3MF)
    queue in order beside the slot and are never replaced.
  - `solve`: one lane per open document for the sketch solver: proposals and
    analyses queued in order, drag steps latest wins, the two taking turns; the
    lane keeps the drag session. Every sketch edit is proposed through it and
    committed once accepted (`Doc::propose`, see `agents/sketch.md`).
  - `io`: one ordered-queue lane for the whole app; *all* file system access
    goes through it. It owns open `DocumentFile`s (app holds `FileId`s), the
    `.design.vrdp.autosave` sidecar lock, auto-save, crash recovery, the store of
    new designs and `recent.toml`. File pickers (`io::pick`) run on the UI
    thread/page, never in the lane. Web side: OPFS (`io/src/opfs.rs`,
    `io/src/web.rs`) and the File System Access API or download fallback.
- **Web workers**: trunk builds four wasm binaries from `crates/web/index.html`:
  `varde-web`, `varde-regen-worker` (a bin of `varde-regen`),
  `varde-solve-worker` (a bin of `varde-solve`) and `varde-io-worker` (a bin of
  `varde-io`), `no-modules` target, started via
  `crates/web/worker_loader.js` rather than trunk's shims. Page and worker share
  no memory: messages are postcard bytes in transferred `ArrayBuffer`s, validated
  on receipt (`regen::wire`, `solve::wire`, `io::wire`).
- **Rendering**: the viewport is an iced `shader` widget whose primitive calls
  `varde_render::Renderer`, compositing onto iced's frame; GPU objects stay on the
  UI thread, workers only produce CPU-side `RenderMesh`es.
- **`.vrdp` format** (`crates/io/src/vrdp.rs`, `agents/file-format.md`): an
  append-only chain of checksummed blocks, one snappy-compressed MessagePack
  record per save, read past damage where it can be; its internals are
  `pub(crate)` to `varde-io` (only the whole-file `to_bytes` and
  `from_bytes` and the types around them, such as `Error`, `Tail`,
  `Report` and `Preview`, are public).

