# Solid rendering: implementation tracking

Work list for the plan in `notes/SolidRendering.md`. Each step runs three
stages in order, each by its own subagent:

1. **Implement**: build the step. A step may split this into several
   subagents (listed as 1a, 1b, …), run in order.
2. **Review**: a general review of the step's changes, looking for
   duplication to fold into shared code and for simplifications. Fixes
   what it finds.
3. **Bugs**: hunt for bugs in the step's changes (edge cases, overflow on
   user data, wasm/WebGL2 paths, stale state). Fixes what it finds, with
   tests.

After each stage: `cargo fmt --all`, commit, and tick the stage here.
Every stage leaves `cargo test --workspace`, `cargo clippy --workspace
--all-targets` and `cargo check --workspace --target
wasm32-unknown-unknown` passing.

## Step 1: Kernel faces, edge polylines, corners, parts

Plan sections: "Edges as polylines in the mesh", "Faces and vertices in
the mesh", "Bodies in the mesh"; steps list item 1.

- [x] Implement
  - 1a. `varde-kernel`: `RenderMesh` gets `face_ends`, `edge_vertices`,
    `edge_ends`, `edge_faces`, `corners`, `edge_corners`, `parts`, checked
    in `from_parts`, offset in `append`; the edge limit becomes a point
    count (2²³). `tessellate` orders triangles by topology region (face
    `i` is region `i`) and draws a polyline per topology chain (edge `i`
    is chain `i`), then the creases, with their corners. Kernel tests.
  - 1b. Consumers: `varde-regen` (wire with validation, cache byte count,
    the picking tables' body per part, faces and closed flags keyed by
    the mesh's ids), `varde-render` (expand polylines into
    pairs for the existing `LineList` until step 2; const asserts), and
    any other users. Wire round-trip test.
- [x] Review
- [x] Bugs

## Step 2: Anti-aliased, thicker edges

Plan section: "Edge drawing on the GPU"; steps list item 2.

- [x] Implement: the `EdgePoint` stream and its four-slot binding, an
  edge entry point on `line_vertex`/`fs_line` with the `pulled` depth,
  `EDGE_WIDTH` 1.5 logical px, drop the `LineList` pipeline, update the
  "still hardware lines" paragraph in `agents/viewport.md`. GPU tests.
- [x] Review
- [x] Bugs

## Step 3: Hidden edges, striped

Plan section: "Hidden edges, striped"; steps list item 3.

- [x] Implement: the `Greater` pass with dashes along `along`,
  `hidden_edge_alpha` in `Colors` and both themes, not while faded, the
  "Hidden edges" View option (on by default) kept like `mouse_hints`.
  GPU and app tests.
- [x] Review
- [x] Bugs

## Step 4: Body opacity

Plan sections: "Body opacity", "Pass order"; steps list item 4.

- [x] Implement
  - 4a. `varde-document`: `Opacity` (10–100 %, checked), `Body::opacity`,
    the command with undo, file round trip in `varde-io`. Tests.
  - 4b. `varde-render`: `Frame::opacity` per part, per-part bounds, the
    transparent passes and the new pass order. GPU tests.
  - 4c. `varde-view` / `varde-app`: the Opacity slider row in the body's
    context menu, preview while dragging, one command on release, Esc
    back; the view handing per-part opacity to the frame. Headless app
    tests.
- [x] Review
- [x] Bugs

## Step 5: Hover and selection

Plan section: "Hover and selection"; steps list item 5.

- [ ] Implement
  - 5a. `varde-view` picking: `Selectable`, `Picked`, vertex/edge/face
    picking with reach, hidden test and the 2¹⁸ cap, `Look::Hover`, on
    the mesh's faces (regions, by `face_ends`), chains (polylines whose
    two faces differ; creases aren't picked) and their corners, ids that
    are the `regen::Picking` tables' too. Unit tests.
  - 5b. `varde-app`: the picking tables kept with the mesh in
    `MeshFeed` (the face's body, `Summary`, an edge's keys by
    `Picking::edge_keys`, closed), hover and selection state in `Doc`, click within
    `CLICK_SLOP` vs orbit drag, `Ctrl`-click, `Space`, clearing on mesh
    change and mode change, exclusive with the Timeline's selection, the
    status bar's box. Headless app tests.
  - 5c. `varde-render` (and the view's frame building): `Frame::hover`
    and `Frame::selected_faces` drawn by index range with `Equal`
    (`@invariant` position), the edge and vertex layer with the outline
    and round vertices (a hovered face's outline: its part's edges
    whose `edge_faces` hold it), `hover_face`, `hover_outline`, `selected` in both
    themes. GPU tests.
- [ ] Review
- [ ] Bugs

## Step 6: Docs and browser check

Plan steps list item 6.

- [ ] Implement: `agents/viewport.md`, `agents/kernel.md`,
  `agents/file-format.md`, `agents/sketch.md` if touched, `README.md`;
  a headless Firefox run of the web build per `AGENTS.md`, fixing what
  it finds.
- [ ] Review
- [ ] Bugs
