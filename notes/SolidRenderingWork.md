# Solid rendering: implementation tracking

Work list for the plan in `notes/SolidRendering.md`. Each step ran three
stages in order, each by its own subagent: **Implement** (split into 1a,
1b, … where listed), **Review** (duplication to fold into shared code,
simplifications) and **Bugs** (edge cases, overflow on user data,
wasm/WebGL2 paths, stale state, with tests). After each stage: `cargo fmt
--all`, a commit, and a tick here, with `cargo test --workspace`, `cargo
clippy --workspace --all-targets` and `cargo check --workspace --target
wasm32-unknown-unknown` passing.

## Step 1: Kernel faces, edge polylines, corners, parts

- [x] Implement
  - 1a. `varde-kernel`: `RenderMesh`'s faces, edge polylines, corners
    and parts, checked in `from_parts`, offset in `append`; the edge
    limit a point count (2²³); `tessellate` by topology region and chain,
    then the creases.
  - 1b. Consumers: `varde-regen` (wire, cache byte count, picking tables
    keyed by the mesh's ids), `varde-render` (polylines expanded into
    pairs until step 2).
- [x] Review
- [x] Bugs

## Step 2: Anti-aliased, thicker edges

- [x] Implement: the `EdgePoint` stream and its four-slot binding on
  `line_vertex`/`fs_line`, `EDGE_WIDTH` 1.5, the `LineList` pipeline
  dropped. GPU tests.
- [x] Review
- [x] Bugs

## Step 3: Hidden edges

- [x] Implement: the `Greater` pass, dashed, `hidden_edge_alpha` in both
  themes, not while faded, the "Hidden edges" view option. GPU and app
  tests.
- [x] Review
- [x] Bugs

## Step 4: Body opacity

- [x] Implement
  - 4a. `varde-document`: `Opacity`, `Body::opacity`, the command; file
    round trip in `varde-io`.
  - 4b. `varde-render`: `Frame::opacity`, per-part bounds, the
    transparent passes. GPU tests.
  - 4c. `varde-view` / `varde-app`: the slider, its preview, one command
    on release. Headless app tests.
- [x] Review
- [x] Bugs

## Step 5: Hover and selection

- [x] Implement
  - 5a. `varde-view`: picking vertices beside faces and edges, by the
    mesh's ids. Unit tests.
  - 5b. `varde-app`: the picking tables with the mesh in `MeshFeed`,
    hover and selection in `Doc`, the status bar's box. Headless app
    tests.
  - 5c. `varde-render`: `Frame::hovered_faces` and `selected_faces`
    redrawn with `Equal`, the edge and vertex layer (outline, selected
    edges, round vertices), the colours in both themes. GPU tests.
- [x] Review
- [x] Bugs

## Step 6: Docs and browser check

- [x] Implement: the agents docs and `README.md`; a headless Firefox run
  of the web build per `AGENTS.md`, fixing what it found.
- [x] Review
- [x] Bugs
