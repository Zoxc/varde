# Solid rendering: edges, hidden edges, selection, transparency

Plan for:

- anti-aliased feature edges on solids, a bit thicker than today's;
- edges hidden by solids drawn striped (dashed), also anti-aliased;
- selecting faces, edges and vertices with no tool open, and hovering
  them where the context can select them: a hovered face drawn brighter,
  a bright outline around the outside of the hovered edge or the hovered
  face's edges, and vertices drawn round when hovered or selected;
- an opacity per body, set from the body's context menu.

All of it is built (`notes/SolidRenderingWork.md` tracks the steps).
This is the plan as it was made; where it differs from the code, the
code wins, and `agents/viewport.md` and `agents/kernel.md` describe what
was built. "Where things are now" below is how things were before it.
Picking and selecting ended up as built beside this plan on the main
line (`view/src/pick.rs`'s `PickIndex`, `view/src/select.rs`'s
`Selection`, kept by name across models, with selection modes and
tangent chains), with this plan's drawing of them, its vertices (only
where three faces or more meet) and its tables keyed by the mesh's ids.

## Where things are now

- `RenderMesh` (`crates/kernel/src/render_mesh.rs`) holds positions,
  normals, triangle indices and feature edges as `[u32; 2]` vertex pairs.
  `tessellate` emits each feature `Edge` record's samples as separate
  pairs. It has no faces, no edge polylines, no bodies: regen
  (`tessellate_scene`) appends the shown bodies' meshes into one.
- Regen sends picking tables with the mesh (`regen::Picking`): faces as
  the kernel topology's regions (body, key, aliases, form `Summary`) and
  edges as its chains (two faces, closed or not). Step 1 makes the mesh's
  faces and polylines those same regions and chains, so the tables are
  keyed by the mesh's face and edge ids and say only what the mesh
  doesn't (see "Bodies in the mesh").
- The renderer draws feature edges as hardware `LineList`: 1 physical
  pixel, aliased, with a fixed `pulled` depth offset. `agents/viewport.md`
  explains why they never became quads: a segment buffer of every edge's
  ends (`2 × [f32; 3]`) for `MAX_EDGES` (2²⁵) would be past the 256 MiB
  buffer bound.
- Sketch lines are already anti-aliased quads (`line_vertex`, `fs_line`),
  with joins, dashes along the polyline, and cuts at the near plane and
  the viewport. Feature edges should use the same code.
- The faded model (in a sketch) is a depth prepass, then only the nearest
  faces blended at `Colors::faded_alpha`. That is the only transparency so
  far.
- `viewport/pivot.rs` ray casts the mesh on the CPU (a middle click), and
  `viewport/extrude.rs`'s `hidden` tests the knobs against the triangles
  every frame up to 2¹⁸ triangles. These are what hover picking builds on.

## Decisions

### Edges as polylines in the mesh

`RenderMesh` replaces `edges: Vec<[u32; 2]>` with polylines, like
`RenderLines`:

- `edge_vertices: Vec<u32>`: the vertex indices, polyline after polyline;
- `edge_ends: Vec<u32>`: one past each polyline's last index;
- `edge_faces: Vec<[u32; 2]>`: the faces (below) on either side of each
  polyline.

Edges are the kernel topology's chains (`Topology::chains`: maximal runs
of mesh edges with the same two regions either side, open from corner to
corner or closed), the same edges regen's picking tables and edge
references name. `tessellate` draws one polyline per chain, in the
topology's order, along the halfedges on the chain's first region's
side, so within a solid's mesh polyline `i` is chain `i` and its
`edge_faces` are the chain's two regions. The creases, feature edges
inside one region where the normals split but no other face begins
(which no chain holds), follow the chains as polylines of their own with
that face on both sides; they're drawn, but aren't edges to pick. A
cylinder's circle split over several patches is then one edge for hover,
and the dash pattern runs on round it without restarting at every
record.

The kernel's `Limits::RENDER.edges` and `RenderMesh::MAX_EDGES` become a
bound on edge points, lowered so the GPU stream (below) fits one 256 MiB
buffer: 2²³ points at 20 bytes is 160 MiB. A const assert in the renderer
says it fits, as for the other parts. A mesh with more feature segments
than that is refused (`MeshError::TooLarge`), as one with too many
triangles is now. That's simpler than splitting edges over several
buffers.

### Faces and vertices in the mesh

Picking needs to know which face a triangle is on. Faces are the kernel
topology's regions (`Topology::regions`: connected triangles of one
`FaceKey`), so a circle's quarter walls, or flush faces merged under one
name, are one face, and a face cut in two by a groove is two faces of
one key. `tessellate` orders the triangles region by region, in the
topology's order, and records `face_ends: Vec<u32>`, one past each
face's last index, so within a solid's mesh face `i` is region `i`. Each
face is then one contiguous index range: picking maps a triangle to its
face by binary search, and the renderer draws a face again by drawing its
range (hover and selection, below). No per-vertex or per-triangle face id
is needed.

The model's vertices, where edges meet, are the polylines' ends (the
chains' ends, and the creases'): `tessellate` merges the ends at the same
mesh vertex (the same point to the bit, see `agents/kernel.md`) into
`corners: Vec<[f32; 3]>`, and each polyline records its two
(`edge_corners: Vec<[u32; 2]>`). A closed loop
that meets no other edge, such as a lone circle, has a corner where it
starts. Its two corner ids are then the same.

### Bodies in the mesh

`RenderMesh::append` records a part per appended mesh: `parts: Vec<Part>`
with the ends of its faces and edge polylines (and so its index range).
Face, edge and corner ids are offset on append, so they are unique within the
joined mesh. Regen's picking tables (`regen::Picking`, sent with the
mesh) hold the `BodyId` of each part in order (the order the scene key
already fixes), and are keyed by the mesh's own face and edge ids: each
face's key, aliases and form `Summary`, and whether each edge is a closed
chain. A face's body is its part's (`Picking::face_body`), an edge's two
faces are the mesh's `edge_faces`, its keys `Picking::edge_keys`. The
joined mesh stays cached by scene: a body's opacity is not part of the
mesh, so changing it never regenerates or re-uploads. `regen::wire` and
the cache's byte count follow the new fields.

### Edge drawing on the GPU

At upload, the renderer resolves the polylines into a point stream:
`EdgePoint { position: [f32; 3], along: f32, edge: u32 }`, 20 bytes,
where `along` is the world length along the polyline so far, with one
padding point at each end of the stream. The same buffer is bound to four
vertex buffer slots at offsets of 0, 1, 2 and 3 points, step mode
instance. Instance `i` then sees points `i` to `i + 3` as previous, start,
end and next. The segment exists if start and end have the same `edge`,
and the previous and next points are its neighbours if theirs match too.
Otherwise the instance's quad is moved off screen, as `line_vertex`
already does.

So every feature edge gets what sketch lines have: joins drawn once,
dashes running along the polyline, near-plane and viewport cuts, no
buffer of duplicated ends. The vertex stage uses 4 of WebGL2's 8 vertex
buffers and 12 of its 16 attributes. No storage buffers or textures are
needed, so the GL backend works.

`vs_edge` becomes an instance of `line_vertex`, with the edges' `pulled`
depth, as `vs_line` does for finished sketches. Edges are
`EDGE_WIDTH = 1.5` logical pixels wide (today's are one physical pixel,
thinner still on HiDPI), tuned by eye against the mock. Where two
different edges meet at a corner their fringes overlap, which thickens
the fringe a little. That's fine for opaque lines and barely shows on the
hidden ones.

Finished sketches (`RenderLines`) could move to the same stream later and
get joins. That's not part of this plan.

### Hidden edges, striped

A second pass draws every edge again with `depth_compare: Greater` against
the opaque bodies' depth: what the visible pass (`LessEqual`) didn't draw.
The two tests split the pixels between them, so a visible stretch and a
hidden stretch of the same edge meet without a gap or an overlap. Hidden
edges are dashed (`HIDDEN_DASH`, about 4 on and 3 off logical pixels, by
`along` scaled at the target like sketch dashes), 1 logical pixel wide, at
`Colors::hidden_edge_alpha` (about 0.45) of the edge colour.

Known artefact: an edge's anti-aliased fringe is depth tested at the
line's depth, so a back edge within a pixel of a silhouette leaks a
visible fringe past it. Avoiding that would mean sampling a copy of the
depth buffer, which isn't worth it here.

Hidden edges show outside a sketch only. While faded, the model already
reads as see-through, and dashes under the sketch would clutter it. Setting
up an extrude, they show.

They get a View option, "Hidden edges", next to "Mouse hints" in the
status bar's view options menu, kept in the app like `mouse_hints`, on by
default.

### Hover and selection

**Where.** With no tool open (outside a sketch, an extrude and picking a
plane), faces, edges and vertices can be selected in the viewport, and
hover shows what a click would select. Hover only shows where what's
under the cursor can be selected. The app derives that from its mode
(`Selectable { faces, edges, vertices }`, with room for a filter such as
"planar faces only" for later contexts) and passes it to the viewport's
`Program`, which picks only those kinds and nothing when the set is
empty. With no tool open it's all three. In every other mode today it's
none.

**Picking runs on the CPU, in the viewport's `Program`**, as the pivot's
and the knobs' tests do, on each cursor move while something can be
selected and nothing is being dragged. Of what's in reach, a vertex wins
over an edge and an edge over a face, so the smaller targets can be
reached where they lie on the larger:

- Vertex: of the corners whose image is within `VERTEX_REACH` (6 logical
  pixels) of the cursor, the nearest on screen that isn't hidden.
- Edge: of the edge segments whose image comes within `EDGE_REACH` (4
  logical pixels), the nearest on screen that isn't hidden.
- Face: the nearest triangle hit by the cursor's ray (`Projector::ray`,
  `pivot.rs`'s `on_mesh`), mapped to its face through `face_ends`.
- Edges and vertices are the chains' polylines and their corners. A
  crease (an edge with the same face on both sides) is drawn but isn't
  picked as an edge: the face under it is.
- Not hidden: the ray from the point towards the eye meets no triangle in
  front of it, reusing the knobs' `hidden` test and its tolerances, so a
  point on the face the ray hits counts as seen.
- Past 2¹⁸ triangles, as for the knobs, nothing is picked rather than
  slowing every move. If that cap bites, the next step is a BVH built in
  the regen lane with the joined mesh, or a GPU id pass (draw ids to an
  `R32Uint` target scissored to the cursor, read back a frame later). The
  id pass matches the drawn image exactly but needs an async readback
  path out of the iced primitive. Neither is part of this plan.

The `Program` sends `Look::Hover(Option<Picked>)` when what's under the
cursor changes (`Picked::Face(u32)`, `Edge(u32)` or `Vertex(u32)`, ids in
the joined mesh, which are also the picking tables' ids). What the app
says about it comes from the tables: the face's body
(`Picking::face_body`), its `Summary` (a plane to sketch on, a cylinder's
axis and radius), an edge's two faces and keys (`edge_keys`) and whether
it's closed. `Doc` keeps it and clears it when the mesh `Arc`
changes, when the cursor leaves the viewport, when the context changes
what can be selected, and while the camera is dragged.

**Selecting** follows the sketch's rules (see "Selection" in
`agents/sketch.md`). A left click selects what's under the cursor alone,
or clears the selection off the model. `Ctrl`-click (`Cmd` on macOS) adds
it or takes it out. `Space` clears it. With no tool open the left button
also orbits, so a press counts as a click only if it's let go within
`CLICK_SLOP`, like the middle click that picks the pivot. A longer drag
orbits as now. The selection is a set of `Picked` in `Doc`, cleared when
the mesh `Arc` changes, since the ids are only the joined mesh's. Keeping
it across an edit needs stable names, which the picking tables already
give (a face's key and aliases, an edge's two faces' keys, as references
store them, which `Topology::face` and `Topology::edge` resolve again);
using them is left for when something acts on the selection.
For now nothing does. The status bar's selection box says what's
selected ("Face", "2 edges"), as it does for a Timeline feature.
Selecting geometry clears the Timeline's selection and the other way
round, so `Space` always clears the one that shows.

**Drawing.** Hover changes no colour but the hovered face's: a hovered
edge keeps its colour, and the rest of the body keeps its own. Selection
is drawn in the accent.

- Face: drawn again over itself by its index range, with the same vertex
  shader (its position `@invariant`, so `depth_compare: Equal` matches it
  exactly), blended towards `Colors::hover_face` when hovered (brighter
  than `model`, per theme) or `Colors::selected` when selected. It keeps
  its body's opacity. A set of selected faces is a draw per face, which
  needs nothing per vertex.
- Outline: the polylines to outline are the hovered edge, or every
  chain bordering the hovered face: the edges of its part whose
  `edge_faces` hold it, found by a pass over the part's edges when hover
  changes, so no other table is kept. A halo pipeline draws them after the edges, depth tested
  and pulled like them. Its fragment shader does what `fs_origin` does
  for core and rim: a core the edge's own colour and width, and outside
  it a rim `HOVER_RIM` (about 1.5 logical pixels) wide in
  `Colors::hover_outline`, bright, for contrast against both the face and
  the background. Only the visible part is outlined.
- Selected edges: drawn again, their core in `Colors::selected`.
- Vertices are drawn only when hovered or selected: round, a disc with a
  rim like a sketch point (`fs_point`), its radius `VERTEX_RADIUS` (about
  3.5 logical pixels), depth tested and pulled like the edges. Hovered,
  the disc is in the edge colour with the bright `hover_outline` rim.
  Selected, it's filled with `Colors::selected`.

The selected and hovered edges and vertices go to the renderer as one
small layer, an `Arc` the view rebuilds when hover or the selection
changes, re-uploaded only then, like the sketch's base layer. The faces'
ids go in `Frame::hover` and `Frame::selected_faces`.

### Body opacity

**What's stored:** `Body` gains `opacity: Opacity`, a newtype over a
whole percent from `Opacity::MIN` (10) to 100. A body can't be made
invisible this way: hiding it is for that. The default is 100, opaque.
`Document::check` refuses a value out of range, so one read from a file
is bounded too. Adding the field changes the `.vrdp` format, which is
fine while it's WIP: no version bump, no migration (see `AGENTS.md`). An
editor `Command` sets it, with undo.

**Where it's set:** the body's context menu in the side panel (the one
with Hide and Show) gets an Opacity row: a slider from 10 to 100 % in
steps of 5, with the value beside it. Moving the slider previews the
opacity in the viewport without touching the document. Letting go
commits it as one command, so a drag is one undo step. Esc or closing
the menu mid-drag goes back to the stored value. The value is clamped
to the range before it becomes an `Opacity`. A row whose body is less
than opaque could show it, for example as the percentage greyed beside
its name. That's a detail for when it's built.

**What's drawn:** the view gives the frame each part's opacity
(`Frame::opacity: &[f32]`, by part, from the picking tables' body per
part and the document, or the slider's preview), so changing it only changes what's
drawn. A part below 1 is drawn in the transparent passes with that
alpha, its edges too. The renderer draws each part's index and edge
ranges in the passes below. Each part's bounds are worked out at upload,
to sort by.

Order-independent transparency (weighted blended OIT, depth peeling)
isn't needed: a body is one colour and one opacity, so blending its
layers over each other gives nearly the same result in any order. Only
the shading differs from face to face. Sorting transparent bodies far to
near by the depth of their bounds' centre, and drawing each body's back
faces before its front faces, is enough. Bodies of different opacities
overlapping on screen are where order errors would show most, and the
sort handles those as long as their bounds don't interleave.

### Pass order

Outside a sketch:

1. Background.
2. Opaque parts' faces, writing depth, then the hovered and selected
   ones of them again (`Equal`), before anything else writes depth
   there.
3. Opaque parts' edges, visible (`LessEqual`, pulled).
4. Grid; finished sketches' lines.
5. Hidden edges of every part, dashed (`Greater` against the opaque
   depth only; an edge behind glass is dashed in step 9).
6. Transparent parts' edges, visible against the opaque depth. They go
   before the transparent faces, so the faces in front of them dim them.
7. The extrude's depth tested layers, if any, likewise dimmed by glass in
   front of them.
8. Transparent parts, far to near: back faces then front faces, blended,
   depth tested, not writing depth.
9. Transparent parts' front faces to depth only, far to near, each
   marking in the stencil where it's nearest and followed by the edges
   it hides there, dashed at its alpha over what it dimmed of them, so a
   body nearly opaque hides edges as an opaque one does; then their
   hovered and selected faces (`Equal`), then their edges again
   (`LessEqual`). The
   edges on the nearest surface show crisp on top of the glass they lie
   on.
10. Selected edges, the hover outline, then hovered and selected
    vertices.
11. Origin marker and pivot.
12. The sketch being edited, on top.

In a sketch (`Frame::faded`): as now, the depth prepass then the nearest
faces blended, with the faded edges as quads and no hidden-edge pass,
hover or selection. Transparent bodies are drawn faded like the rest.

## Steps

Each step is a commit (or a few), builds on both targets and keeps the
tests passing.

1. **Kernel: faces, edge polylines, corners, parts.** `tessellate` orders
   triangles by topology region, draws a polyline per topology chain
   and then the creases, with their two faces and two corners; `RenderMesh` gets `face_ends`, `edge_vertices`,
   `edge_ends`, `edge_faces`, `corners`, `edge_corners`, `parts`, checked
   in `from_parts`; `append` offsets them;
   the edge limit becomes a point count. Regen's wire, cache size and
   answer (the picking tables' body per part) follow. Until step 2, the renderer expands the
   polylines into the old pairs for `LineList`. Tests: a box has six faces
   and twelve edge polylines, each with two different faces, and eight
   corners, each the end of three polylines; a cylinder's circles are one
   polyline each, closed on one corner; `append` offsets ids; `from_parts` refuses bad ends and faces; wire
   round trip.
2. **Anti-aliased, thicker edges.** The `EdgePoint` stream, the
   four-slot binding, the edge entry point on `line_vertex`/`fs_line`,
   `EDGE_WIDTH`. Drop the `LineList` pipeline and update the "still
   hardware lines" paragraph in `agents/viewport.md`. GPU tests in
   `crates/render/tests/viewport.rs`: an edge has partly covered pixels at
   its sides; its width on screen follows `EDGE_WIDTH` and the scale
   factor; `edges_stay_in_front_of_faces_zoomed_into_a_large_scene` still
   passes; a polyline's joins aren't darker than its middle.
3. **Hidden edges.** The `Greater` pass with dashes, `hidden_edge_alpha`
   in `Colors` and the theme, the View option. Tests: a cube in front of
   another shows the back one's edge dashed (coverage alternating along
   it) where it's behind, solid where it isn't; nothing dashed with the
   option off or while faded.
4. **Body opacity.** `Body::opacity` and `Opacity`, its command, the
   context menu's slider with its preview, `Frame::opacity`, the passes
   above. Tests: document (the command and undo, a file round trip, an
   out-of-range value refused); app, headless (the slider previews
   without an edit, letting go is one undo step, Esc goes back); GPU: a
   body behind a 30 % one shows through, tinted; the transparent body's
   back edges show dimmed and its front edges crisp; an opaque body in
   front of a transparent one still hides it; the order of two
   transparent bodies barely changes the pixels.
5. **Hover and selection.** `Selectable` from the app's mode (all
   three kinds with no tool open, none otherwise), picking in `viewport`
   against the mesh's faces and chains, the picking tables kept with the
   mesh in `MeshFeed` for what a pick is (its body, keys, summary; a
   crease isn't an edge to pick) (unit tests in the view crate: cursor over a face, near an edge, near
   a vertex where its edges and faces are also in reach, near an edge
   hidden behind a face, off the model, past the cap, nothing picked of a
   kind the context can't select), `Look::Hover`, clicks and `Ctrl`-clicks
   within `CLICK_SLOP` while a drag still orbits, `Space`, the selection
   in `Doc` and its clearing, the status bar's box, `Frame::hover` and
   `Frame::selected_faces`, the faces' `Equal` redraw, the edge and
   vertex layer, `hover_face`, `hover_outline` and `selected` in both
   themes. App tests, headless: click, `Ctrl`-click, a drag that orbits
   and selects nothing, `Space`, a model change clearing the selection,
   no hover in a sketch or an extrude. GPU tests: the hovered face is
   brighter than the same face unhovered, and the hovered edge's core
   keeps its colour; the outline's pixels lie outside the edge's core
   and are brighter than the face; a vertex shows only when hovered or
   selected, and round; a selected face behind a transparent body is
   still tinted.
6. **Docs and a browser check.** `agents/viewport.md` (pass order, edges,
   hover, selection), `agents/kernel.md` (`RenderMesh`), `agents/file-format.md`
   (the body's opacity), `README.md` (selecting faces, edges and vertices, the hidden edges option,
   body opacity). Run the web build headless in Firefox (as in
   `AGENTS.md`) to check the four-slot binding and the passes on WebGL2,
   and `cargo check --workspace --target wasm32-unknown-unknown`.

## Open questions

- **"Solid edges" taken as feature edges.** The faces' own outlines,
  where no feature edge runs (a cylinder's silhouette, a transparent
  body's rim), stay aliased. Fixing those means MSAA into a target of the
  renderer's own and a resolve onto iced's frame, which
  `agents/viewport.md` calls costly on WebGL2, or a post pass (FXAA). Left
  out unless wanted.
- **What acts on the selection.** Nothing does yet. Its first user
  (sketching on a selected planar face, measuring, a fillet on selected
  edges) decides whether selection needs stable names across edits.

Settled: opacity is per body, set from its context menu; hidden edges
are on by default; hover keeps edge and body colours (only the hovered
face brightens, and the outline is added); hover shows only where the
context can select what's under the cursor; with no tool open, faces,
edges and vertices can be selected; vertices are hoverable, drawn round.
