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

- **No file format versioning while the app is WIP.** The `.vrdp` format
  (`crates/io/src/vrdp.rs`) has a `version` field, but it stays at 1:
  don't bump it, and don't add migrations or backwards-compatibility code.
  The payload is postcard-encoded, so any change to the serialized types
  (`Document`, `Body`, `Shape`, `Sketch`, ...) breaks old files. That's fine
  for now. Revisit once the format needs to be stable.
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
- `agents/file-format.md`: the `.vrdp` format.
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
cargo test --workspace                      # all tests
cargo test -p varde-io                      # one crate
cargo test -p varde-io some_test_name       # one test (substring match)
cargo clippy --workspace --all-targets
cargo check --workspace --target wasm32-unknown-unknown   # web-only code paths
cargo doc --workspace --no-deps --document-private-items  # link checks, on both targets
cd crates/web && trunk serve                # browser build
```

GPU tests (`crates/render/tests`, viewport tests in `varde-view`) render
offscreen via `pollster` and print "no GPU adapter, skipping" and pass when
there is no adapter. Tests pass temporary directories (or none) to
`varde_io::Stores` so they never touch the user's config/data dirs.

## Architecture

Crates in `crates/`, dependencies only pointing down:
`binary`/`web` → `app` → `view` → `render` → `kernel`; `document` → `kernel`,
`sketch`; `regen`, `solve` and `io` → `document` + `lane` (`solve` also
`sketch`). `kernel`, `sketch`, `document`, `lane`, `regen`, `solve`, `io` have
no UI code; `render` has no iced dependency.

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
    and drops stale answers.
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
- **`.vrdp` format** (`crates/io/src/vrdp.rs`): append-only, checksummed,
  snappy-compressed postcard snapshots; its internals are `pub(crate)` to
  `varde-io` (only `to_bytes`, `from_bytes`, `Error` are public).

