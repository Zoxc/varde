# Solid rendering: edges, hidden edges, selection, transparency

Plan for:

- anti-aliased feature edges on solids, a bit thicker than before;
- edges hidden by solids drawn dashed, also anti-aliased;
- selecting faces, edges and vertices with no tool open, and hovering
  them where the context can select them: a hovered face drawn brighter,
  a bright outline around the hovered edge or the hovered face's edges,
  and vertices drawn round when hovered or selected;
- an opacity per body, set from the body's context menu.

All of it is built (`notes/SolidRenderingWork.md` tracks the steps).
`agents/viewport.md` and `agents/kernel.md` describe what was built, in
detail; this note keeps the decisions and why they were made. Where it
differs from the code, the code wins. Picking and selecting were built
beside this plan (`view/src/pick.rs`'s `PickIndex`, `view/src/select.rs`'s
`Selection`, kept by name across models, with selection modes and tangent
chains); this plan added their drawing, vertices (where three faces or
more meet) and tables keyed by the mesh's ids.

## Decisions

### Edges as polylines in the mesh

`RenderMesh` holds feature edges as polylines (`edge_vertices`,
`edge_ends`, `edge_faces`) rather than vertex pairs. The edges are the
kernel topology's chains, in its order, so within a solid's mesh edge
`i` is chain `i`, the same edges the picking tables and edge references
name; the creases follow, with one face on both sides, drawn but not
picked as edges. A circle split over several patches is then one edge to
hover, and dashes run round it without restarting at every record.

The edge limit is a count of points (`RenderMesh::MAX_EDGE_POINTS`, 2²³),
so the GPU stream fits one 256 MiB buffer (a const assert in the
renderer says so); a mesh past it is refused (`MeshError::TooLarge`).
That's simpler than splitting edges over several buffers.

### Faces and vertices in the mesh

Picking needs a triangle's face. `tessellate` orders the triangles region
by region in the topology's order and records `face_ends`, so face `i`
is region `i` and each face is one contiguous index range: picking maps
a triangle to its face by binary search, and the renderer draws a face
again by its range. No per-vertex or per-triangle face id is needed.

The polylines' ends at one mesh vertex are merged into `corners`, each
polyline recording its two (`edge_corners`); a closed edge that meets no
other has a corner where it starts.

### Bodies in the mesh

`RenderMesh::append` records a part per mesh appended (`part_ends`) and
offsets face, edge and corner ids, so they're unique in the joined mesh.
Regen's picking tables hold the body of each part and are keyed by the
mesh's ids, saying only what the mesh doesn't. A body's opacity is not
part of the mesh, so changing it never regenerates or re-uploads.

### Edge drawing on the GPU

Edges are uploaded as a stream of 20-byte points, bound to four vertex
buffer slots a point apart, step mode instance, so instance `i` sees the
point before, the segment's ends and the point after. Edges then get
what sketch lines have (`line_vertex`, `fs_line`: joins drawn once,
dashes along the polyline, near-plane and viewport cuts) with no buffer
of duplicated ends, no storage buffers and no textures, so WebGL2 works.
`EDGE_WIDTH` is 1.5 logical pixels, tuned by eye against the mock.

### Hidden edges

The same stream drawn again with `depth_compare: Greater` against the
opaque parts' depth, the same quads as the visible pass, so the two split
the pixels and a visible stretch meets a hidden one without a gap or an
overlap. Dashed, thin and faint (`HIDDEN_DASH`, `HIDDEN_EDGE_WIDTH`,
`Colors::hidden_edge_alpha`). Not drawn faded: in a sketch the model
already reads as see-through and dashes would clutter the sketch.
"Hidden edges" in the view options menu, on by default.

Known artefact: an edge's fringe is depth tested at the edge's depth, so
a back edge within a pixel of a silhouette leaks past it. Avoiding that
means sampling a copy of the depth buffer, which isn't worth it.

### Hover and selection

Picking is on the CPU (a GPU id pass would need an async readback out of
the iced primitive and lag a frame). Of what's in reach a vertex wins
over an edge and an edge over a face, so the smaller targets can be
reached where they lie on the larger.

Hover changes no colour but the hovered face's: the hovered edge keeps
its colour and the body its own; the outline is added around it.
Selection is in the accent. A hovered or selected face is drawn again by
its index range with `depth_compare: Equal` (the position `@invariant`),
a draw per face, which needs nothing per vertex. The hovered and
selected edges and vertices go to the renderer as one small layer,
re-uploaded only when it changes.

### Body opacity

`Body::opacity` is a whole percent from 10 to 100: a body can't be made
invisible this way, hiding it is for that. It's set by a slider in the
body's context menu that previews without touching the document and
commits one command on release, so a drag is one undo step.

No order-independent transparency (weighted blended OIT, depth peeling):
a body is one colour and one opacity, so blending its layers in any order
gives nearly the same result, only the shading differs. Transparent
parts are sorted far to near by their bounds' centres and each drawn
back faces then front faces; that's only wrong for bodies whose bounds
interleave. Edges hidden by glass are dashed at its alpha, so a body
nearly opaque hides edges as an opaque one does; that needs a stencil
per glass part. The pass order is in `agents/viewport.md`.

## Steps

Each step a commit or a few, building on both targets with the tests
passing.

1. **Kernel: faces, edge polylines, corners, parts**, and regen's wire,
   cache size and picking tables following.
2. **Anti-aliased, thicker edges**: the point stream and the four-slot
   binding, replacing the `LineList` pipeline.
3. **Hidden edges**: the `Greater` pass, `hidden_edge_alpha`, the view
   option.
4. **Body opacity**: `Opacity`, its command, the slider and its preview,
   `Frame::opacity` and the transparent passes.
5. **Hover and selection**: picking vertices, the faces' `Equal` redraw,
   the edge and vertex layer, the colours in both themes.
6. **Docs and a browser check**: the agents docs and `README.md`, and
   the web build run headless in Firefox to check the four-slot binding
   and the passes on WebGL2.

## Open questions

- **Faces' outlines.** Where no feature edge runs (a cylinder's
  silhouette, a transparent body's rim) the faces' edges stay aliased.
  Fixing that means MSAA into a target of the renderer's own and a
  resolve onto iced's frame, costly on WebGL2, or a post pass (FXAA).
- **What acts on the selection.** Nothing does yet; its first user
  (sketching on a selected planar face, measuring, a fillet on selected
  edges) will shape what it needs.
