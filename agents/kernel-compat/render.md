# Drawing and export meshes (`src/render_mesh.rs`, `src/render_lines.rs`, `src/mesh_upload.rs`, `src/manifold.rs`, `src/tessellate.rs`)

The value types the renderer, regen and the 3MF writer exchange. The
facade makes none of them from a solid (an empty solid tessellates to the
empty `RenderMesh` and gives `ManifoldError::Empty`), but they are built
and checked as before: regen builds `RenderLines` from sketches,
`RenderMesh::append` joins meshes, and meshes and lines that cross a lane
come in through `from_parts`, which checks them.

## `RenderMesh`

An indexed triangle mesh with per-vertex normals for drawing, its
triangles grouped by face (`face_ends`), feature edges as polylines
(`edge_vertices`, `edge_ends`) with the faces either side (`edge_faces`)
and the corners they end at (`corners`, `edge_corners`), wires (the rest
of the patches' edges, `wire_vertices`, `wire_ends`) and parts
(`part_ends`: one past each part's last face, edge, corner and wire;
`RenderMesh::append` adds a part per mesh). A solid's face `i` is region
`i` of its `Topology` and edge `i` chain `i`, then the creases (feature
edges inside one region, with that face twice). Always drawable: a normal
for every position, whole triangles, every index in range, every vector
within `RenderMesh::MAX_*`, every normal finite and every position within
`MAX_POSITION`; the fields are private, and `from_parts` checks a mesh
built elsewhere (`MeshError`).

`MeshUpload` (`RenderMesh::upload`, built once and kept with the mesh) is
what the renderer uploads beyond the vectors: the stream of `EdgePoint`s
the feature edges and wires are drawn from (each point's edge, with the
`CREASE` and `NEIGHBOUR_ONLY` flags; `NO_EDGE` at the stream's ends), where
each part's points are in it, and each part's bounds. Pure CPU work, so
the regeneration lane builds it and the UI thread only copies it.

`RenderLines` are polylines in world space, such as the finished sketches
drawn with the model: every polyline at least two points, at most
`MAX_POINTS` points, every point within `MAX_POSITION` (`LinesError`
otherwise, through `from_parts`), in parts (`LinesPart`).

`Display` holds how finely a solid is sampled (`Display::new(tol)`,
`Display::for_view(tol, chord)`, `chord(diagonal)`), which the Varde
kernel's tessellation will take. Loose geometry, a failure's evidence, is
drawn by `Display::flatten` (equal parameter steps, chords within
`chord(diagonal)` at their middles, at most `MAX_TURN_DEGREES` of turn
each, up to `MAX_SEGMENTS`) and `Display::sample_patch` (for now
nothing).

## Manifold meshes for export

`ManifoldMesh` is an indexed triangle mesh that is a closed, oriented
2-manifold, for files such as 3MF (`varde-io`, `agents/files.md`). Its
positions are `f32` values (held as `f64`) about an origin with
whole-numbered coordinates: file readers keep single precision, so the
mesh checked is the one they read.

`ManifoldMesh::new` is the only way to make one (deserializing goes
through it too) and checks, in this order, giving the first failure by
the lowest triangle, vertex or edge: the sizes (`TooLarge`), at least one
triangle (`Empty`), the origin whole-numbered within `MAX_POSITION` (`2 ×
MAX_COORD`; `Origin`), every coordinate finite, within `MAX_POSITION` and
an `f32` value (`Position`), indices in range, no vertex twice in a
triangle, no triangle whose corners are collinear (`Degenerate`), no two
vertices at the same position (`Coincident`, by sorting their bits),
every undirected edge used by exactly two triangles (`EdgeUse`) running
it opposite ways (`Orientation`), the triangles round each vertex one
cycle (`Fan`) and every vertex in one (`UnusedVertex`), and a positive
signed volume about the middle of its box (`InsideOut`).

Collinear is decided exactly: a component of `(p1 − p0) × (p2 − p0)` is
`Σ (a_i·b_j − a_j·b_i)` over the corner pairs `(p0, p1)`, `(p1, p2)`,
`(p2, p0)`; the positions are `f32` values, so each of the six products is
exact in `f64`, and their sum is added exactly as an expansion (Shewchuk's
grow-expansion over Knuth's two-sum, no FMA). The expansion is
nonoverlapping, so it is zero exactly when every component is. A
component whose factors make each product zero outright (equal
coordinates, as along a face square to an axis) is skipped.
`manifold/tests.rs` has a case the rounded differences call collinear and
the exact sum doesn't. It also mutates hand-built shells (a box, a faceted
cylinder, a torus) at random and checks `ManifoldMesh::new` against a
plain check in integers: every mesh it can tell is taken or refused alike.
