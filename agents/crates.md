# Crates and build

Dependencies only point downwards in this list.

| crate      | package          | purpose |
|------------|------------------|---------|
| `kernel`   | `varde-kernel`   | Shapes (the stored recipe), the solids they build, tessellation to `RenderMesh`. Analytic primitives today, no booleans yet; intended to wrap [manifold](https://github.com/elalish/manifold) for them. |
| `sketch`   | `varde-sketch`   | 2D sketch entities and geometric constraints. Solver still to be written. |
| `render`   | `varde-render`   | wgpu scene renderer (background, model, infinite grid) and orbit `Camera` (orthographic by default, or perspective). No UI dependency; records into any encoder/target, so it can also serve offscreen thumbnails or a software fallback later. |
| `document` | `varde-document` | The document model plus `Editor`, which applies `Command`s and keeps undo/redo history, and how design files are named (`name`). No UI code. |
| `lane`     | `varde-lane`     | What the regeneration and IO lanes share: the thread natively, the page's and the worker's sides of a Web Worker on the web, the `Transport` requests are sent through, and answering a panic. Knows nothing of documents. |
| `regen`    | `varde-regen`    | Heavy document work (tessellation today) behind `Request`/`Response` messages tagged with the editor generation, and the `Transport` that carries them: a lane per open document, natively a thread and on the web a Web Worker (the `varde-regen-worker` binary), plus the wire format the worker speaks. No UI code. |
| `io`       | `varde-io`       | All file system access, off the UI thread: the IO lane, which owns the open documents and their lock files, opens, saves and auto-saves them, keeps new designs in the app's store until they're saved, recovers what a crash left, and keeps the recent files list; the `.vrdp` file format (`vrdp`). Natively a thread; on the web a Web Worker (the `varde-io-worker` binary) on the Origin Private File System, plus the wire format it speaks. Also the file pickers that feed the lane (`pick`), run on the UI thread or the page, never in the lane. |
| `view`     | `varde-view`     | iced widgets: the welcome screen and the document screen: toolbar, docked Timeline/Objects side panel, status bar and the 3D viewport (an iced `shader` widget wrapping `render`). Pure functions of state that emit `Message`s. |
| `app`      | `varde-app`      | Owns app state (`Editor`, `Camera`, the mesh feed of what it asked `regen` for and the newest mesh answered, recent files), handles the view's `Message`s, keys and its own messages from the lanes, dialogs and timers, sends file work to the IO lane, exposes `run()`. |
| `binary`   | `varde`          | Native executable; calls `varde_app::run()`. |
| `web`      | `varde-web`      | wasm build of the same app, built with trunk; does nothing natively. |

```
binary, web ─▶ app ─▶ view ─▶ render ─▶ kernel
                 │      ├───▶ document ─▶ kernel, sketch
                 │      └───▶ kernel
                 ├──────────▶ regen ───▶ document, lane, kernel
                 ├──────────▶ io ──────▶ document, lane
                 └──────────▶ render, document
```

## The web build

`trunk` builds three wasm binaries from `crates/web/index.html`: the app
(`varde-web`), the regeneration Web Worker (`varde-regen-worker`, a binary of
`varde-regen`, so the worker holds the regeneration side only, not iced and
wgpu) and the IO Web Worker (`varde-io-worker`, a binary of `varde-io`).
The workers are built with wasm-bindgen's `no-modules` target, and the app
starts them through `crates/web/worker_loader.js`, copied next to the page,
rather than trunk's loader shims: those leave a `.wasm` that fails to load
unnoticed by the page, so the worker's requests would never be answered.

## Look and assets

The look follows `notes/ui-mock.html`. The logo is `assets/logo.svg`; the
checked-in `assets/logo.ico` is embedded as the Windows executable icon by
`crates/binary/build.rs`, which has the command to regenerate it.

## Roadmap by crate

- **kernel**: manifold backend (`manifold3d` crate) for booleans; extrude/revolve from sketch profiles; later B-rep or an OCCT binding if exact geometry is needed.
- **sketch**: constraint solver (Newton / Levenberg–Marquardt over point coordinates), then sketch editing tools in `view`.
- **document**: parametric feature history (sketch → extrude → boolean) with regeneration, then command-based undo instead of snapshots.
- **render**: picking (ID buffer), selection highlight, silhouette lines, MSAA, view cube.
- **view**: property panel, feature tree, sketch mode, keyboard shortcuts.
