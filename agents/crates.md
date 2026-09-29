# Crates and build

Dependencies only point downwards in this list.

| crate      | package          | purpose |
|------------|------------------|---------|
| `kernel`   | `varde-kernel`   | `Solid`s, closed meshes of rational quadratic patches that pass the mesh check, their tessellation to `RenderMesh` (per-edge segment counts, crack-free stitching, analytic normals shared where smooth, feature edges), their volume and area, and `RenderLines`, polylines drawn with the model. `extrude` sweeps a `Profile` (loops of 2D conics with curve ids, outer loops counter-clockwise, holes clockwise) placed on a `Frame` into an exact solid: caps from a constrained Delaunay triangulation (`spade`) with curved boundary edges, walls from cylinder strips, faces named and tagged. No booleans yet. The math of rational quadratic curves and triangles (`patch`: evaluation, blossoms, exact splits, the fold check and normal cones, exact arcs and cylinder strips), which solids are to be made of, and closed meshes of them (`mesh`: the halfedge structure with shared edge records, face names and surface tags, a builder, `check` of the invariants (topology, shared edges, folds, control hulls by GJK and the neighbour plane rules, face tags), a BVH over hull boxes, exact red–green refinement and `repair`, which splits what breaks the fold and hull rules until it passes, and box and cylinder meshes); the design's `Tolerance`; the limits, the work `Budget` and `KernelError`; `par_map`, rayon natively and sequential on the web, deterministic either way (`agents/kernel.md`). |
| `expr`     | `varde-expr`     | Typed values: expressions with units (`1 in + 3`, `90 deg - 15`) parsed from bounded text, checked for units (length, angle, number) and evaluated to model units with every step checked, against the caller's bounds; the stored `Value` (text and value) and re-checking it; writing the design's unit into a text's bare numbers (`pin_units`); showing a value in a unit (`format`). No dependencies (`serde` behind a feature, for the crates that store values). |
| `sketch`   | `varde-sketch`   | The sketch model: points, lines, circles and arcs with ids from the sketch's `next_id` and per-kind numbers for names, constraints, dimensions (driving or reference, values as typed expressions), deleting with what depends on it, `check`, the one place a sketch from a file is trusted, the constraint solver and its analysis (degrees of freedom, what's fixed, redundant constraints) as pure functions on faer, curves flattened to polylines for drawing, the geometry the drawing tools need (the arc through three points), where curves meet, and profiles: the regions the curves enclose, outer loops and holes as exact pieces and flattened, picking one, and near misses, bounded. |
| `render`   | `varde-render`   | wgpu scene renderer (background, model, optionally faded, infinite grid on any plane, finished sketches as fixed-width depth-tested lines, and over it all the sketch being edited: anti-aliased lines with dashes, points and fills, tessellated by tess2-rust, from layers the view builds) and orbit `Camera` (orthographic by default, or perspective). No UI dependency; records into any encoder/target, so it can also serve offscreen thumbnails or a software fallback later. |
| `document` | `varde-document` | The document model (bodies, which the feature history makes and which hold no geometry, features: sketches on origin planes, and the design's units) plus `Editor`, which applies `Command`s and keeps undo/redo history, and how design files are named (`name`). No UI code. |
| `lane`     | `varde-lane`     | What the regeneration, solver and IO lanes share: the thread natively, the page's and the worker's sides of a Web Worker on the web, the `Mailbox` a worker taking one request at a time keeps the rest in on the page and the page's side driving it (`mailbox::spawn`, a lane saying only how its messages cross, `Wire`), the `Transport` requests are sent through, and answering a panic. Knows nothing of documents. |
| `regen`    | `varde-regen`    | Heavy document work (evaluating the feature history into the bodies' solids and tessellating the visible ones, flattening the visible sketches and solving every sketch to tell those that don't solve, today) behind `Request`/`Response` messages tagged with the editor generation, and the `Transport` that carries them: a lane per open document, natively a thread and on the web a Web Worker (the `varde-regen-worker` binary), plus the wire format the worker speaks. No UI code. |
| `solve`    | `varde-solve`    | The sketch solver off the UI thread: `Request`s (proposals, drag steps, analyses) and `Response`s, answered by a `Solver` that keeps the drag in progress, proposals and analyses in order and drag steps latest wins, taking turns; a lane per open document, natively a thread and on the web a Web Worker (the `varde-solve-worker` binary), plus the wire format the worker speaks, checked on both sides. No UI code. |
| `io`       | `varde-io`       | All file system access, off the UI thread: the IO lane, which owns the open documents and their lock files, opens, saves and auto-saves them, keeps new designs in the app's store until they're saved, recovers what a crash left, and keeps the recent files list; the `.vrdp` file format (`vrdp`). Natively a thread; on the web a Web Worker (the `varde-io-worker` binary) on the Origin Private File System, plus the wire format it speaks. Also the file pickers that feed the lane (`pick`), run on the UI thread or the page, never in the lane. |
| `view`     | `varde-view`     | iced widgets: the welcome screen and the document screen: toolbar (with the sketch's tools and the Constrain tool), docked Timeline/Objects side panel (Sketch/Objects in a sketch, the Sketch tab's Geometry and Constraints lists drawing only the rows in view), status bar and the 3D viewport (an iced `shader` widget wrapping `render`), which in a sketch builds the layers the renderer draws of the sketch being edited, coloured by the constraints' analysis, and takes the left button for it, with its projection and hit testing, and the constraints' glyphs, dimensions' labels and the value field in layers of widgets anchored to the sketch over it. Which constraints fit a selection and what they make of it (`ConstraintKind`), and what the Dimension tool measures of what's picked (`dimension`). Pure functions of state that emit `Message`s. |
| `app`      | `varde-app`      | Owns app state (`Editor`, `Camera`, the mesh feed of what it asked `regen` for and the newest mesh and sketch lines answered, the sketch being edited (`SketchSession`: the tool, the selection, a drag session, the analyses, the value field) and the edits the sketch tools make, values typed read with `expr`, proposed through the document's solver lane (`Proposals`), recent files), handles the view's `Message`s, keys and its own messages from the lanes, dialogs and timers, sends file work to the IO lane, exposes `run()`. |
| `binary`   | `varde`          | Native executable; calls `varde_app::run()`. |
| `web`      | `varde-web`      | wasm build of the same app, built with trunk; does nothing natively. |

```
binary, web ─▶ app ─▶ view ─▶ render ─▶ kernel
                 │      ├───▶ document ─▶ kernel, sketch, expr
                 │      └───▶ kernel, sketch, expr
                 ├──────────▶ regen ───▶ document, lane, kernel, sketch
                 ├──────────▶ solve ───▶ document, lane, sketch, expr
                 ├──────────▶ io ──────▶ document, lane
                 └──────────▶ render, document, sketch, expr
sketch ─▶ expr
```

## The web build

`trunk` builds four wasm binaries from `crates/web/index.html`: the app
(`varde-web`), the regeneration Web Worker (`varde-regen-worker`, a binary of
`varde-regen`, so the worker holds the regeneration side only, not iced and
wgpu), the solver Web Worker (`varde-solve-worker`, a binary of
`varde-solve`) and the IO Web Worker (`varde-io-worker`, a binary of
`varde-io`).
The workers are built with wasm-bindgen's `no-modules` target, and the app
starts them through `crates/web/worker_loader.js`, copied next to the page,
rather than trunk's loader shims: those leave a `.wasm` that fails to load
unnoticed by the page, so the worker's requests would never be answered.

## Look and assets

The look follows `notes/ui-mock.html`. The logo is `assets/logo.svg`; the
checked-in `assets/logo.ico` is embedded as the Windows executable icon by
`crates/binary/build.rs`, which has the command to regenerate it.

## Roadmap by crate

- **kernel**: union, difference and intersection built the way Manifold builds them, by counting, on curved patches (`agents/kernel.md`); later revolve, taper and GPU evaluation of the patches.
- **sketch**: the shape tools' geometry (trim, offset, fillet).
- **document**: parametric feature history (sketch → extrude → boolean) with regeneration, then command-based undo instead of snapshots.
- **render**: picking (ID buffer), selection highlight, silhouette lines, MSAA for the model's faces, anti-aliased feature edges, view cube, a camera that can roll (for sketches on faces).
- **solve**: cancelling a running solve; a cheaper analysis for large sketches.
- **view**: property panel, the Alt bar, keyboard shortcuts.
