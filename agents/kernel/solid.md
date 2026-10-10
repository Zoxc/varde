# Solids and tessellation (`src/solid.rs`, `src/tessellate.rs`)

`Solid` wraps a `Mesh` that passes `check`: `Solid::new(mesh, tol)` runs
the check and fails with `Invalid`, `Solid::cuboid` and `Solid::cylinder`
wrap the checked primitives, and `Solid::empty()` is the empty solid.
`mesh()`/`into_mesh()` give the patches back, `bounds3()` is the `f64`
box around the control points (which holds the solid) and `bounds()` the
same in `f32` (`None` when empty; rounding is monotonic, so it holds the
tessellation too). A solid also keeps the resolution its mesh passed the
check with (every constructor checks with the tolerance it is given;
not part of equality, which is the meshes'): a boolean at that
resolution doesn't test again the triangles it keeps of the solid as
they were (see "Repair and the check near the change" under Booleans).

`Solid::tessellate(&Display)` gives a `RenderMesh`. `Display::new(tol)`
(default: the default tolerance) sets the targets:

- **Per-edge counts.** Each `Edge` record gets a number of equal parameter
  steps from its own curve only (`segments`): the chord of each step
  within `max(fit, 1e-3 × the solid's diagonal)` of the curve at the
  step's middle, the tangent turning at most 10° along a step (the unit
  tangents' dot at least `cos 10°`, written as a literal so no platform's
  `cos` decides a count), at most 64 steps. It starts from the turn
  between the end tangents (the chord between their unit vectors over
  10° in radians, an underestimate) and grows by the square root of the
  chord error's ratio, at least one step at a time. Only `+ − × ÷ √`, so
  the counts are the same everywhere. A straight edge is one step.
- **Along rulings** (`along_rulings`). On a patch of a cylinder or cone
  (`Form::Cylinder`, `ConicCylinder`, `Cone`) with exactly one straight
  edge (its control point on the line through its ends, within `1e-12`
  of its span or `1e-14` of its largest coordinate, `in_line`: built or
  moved far out, a straight edge keeps its control point on its line only
  to the rounding there, and a short one then lost the evened counts),
  that edge is a ruling and the other two curved edges get
  the larger of their counts, through every chain of such edges (a
  union–find over the edges, each chain at its most; the counts only
  grow, whatever the order): an extruded wall's bottom, its patches'
  diagonals and its top. A diagonal is the bottom sheared along the wall
  with the same parameters, so with the bottom's count its samples lie
  on the bottom's rulings, and so do the inner grid's points (the
  patch's rulings are its parameters' lines parallel to the ruling
  edge): the triangles run between neighbouring rulings, and the welded
  wall is a prism, creased only along them. Counted from its own curve,
  a tall wall's diagonal bends little and got fewer segments than the
  arc (3 to the arc's 9 on a 5 × 200 cylinder, 1 on a 0.5 × 200 needle):
  the triangles near it crossed the rulings, creases of up to 30°
  winding round the wall, which a 3MF reader shading by the faces shows
  as a twisted line, and on the needle triangles folded over the
  diagonal, facing inwards. A cylinder wall gains a few triangles (1344
  to 1384 on a 5 × 100 one).
- **Shared samples.** Each edge's points are evaluated once, along its
  first halfedge (the lowest), from its conic (`Conic3::eval`), and both
  patches beside it use those vertices, so neighbours share their boundary
  points to the bit: no cracks.
- **The inside of a patch.** A patch whose edges are all one step is one
  triangle. A ruled strip (below) has no inner points. Otherwise, with
  `m = max(counts, 3)`, it is sampled at the
  points of the regular barycentric grid of `m` steps at least one step in
  from its boundary: `(i + 1, j + 1, k + 1) / m` with `i + j + k = l = m −
  3`, triangulated regularly (`l²` triangles). The ring between that inner
  grid and the boundary is three strips, one per edge, from the edge's `n`
  segments to the inner grid's side of `l`, split at the diagonals from
  each corner to the inner grid's corner next to it: `n + l` triangles
  each. A strip advances the side whose new diagonal keeps closer to the
  patch at its middle (in parameters, along the patch's normal there),
  and where the two are as close (within a millionth of the longer
  diagonal, as on a plane) the side that makes the shorter one, the
  outer one on a tie; so its triangles follow the geometry where the
  grid runs skewed to the edges. Length alone was fooled on tall walls:
  there the heights of the inner points differ by more than a step round
  the arc, and the strips paired points three steps apart, slivers near
  the caps 3 chords inside a 5 × 100 cylinder with their faces 43° off
  their vertex normals (a visible pinch). Then each corner's diagonal is
  flipped, to join the edges' samples either side of the corner, when
  that diagonal keeps closer to the patch by the same rule, both strips
  end there in an edge segment and the quad is convex in parameters: on
  a cylinder wall triangle the inner grid's corner is two steps round
  the arc from the patch's corner. Convex is strictly, by more than
  `1e-10` of twice each new triangle's area in parameters (they are
  fractions of at most 256, so three samples not in line make at least
  `256⁻³ ≈ 6e-8`, and rounding is about `1e-16`): on a grid of twice the
  corner's sides' steps the inner grid's corner lies on the line between
  the edges' samples either side, and at exactly 0 the rounding let a
  flip make a triangle of three samples in line (a coarse part torus).
  With `l = 0` the ring is a fan round the single inner point.
- **Ruled strips.** A patch on a cylinder or cone (circular or not; not a
  plane, not curved both ways) with exactly one edge of one segment, its
  **straight side**, is drawn without inner points when the lines across
  it parallel to that side are straight (`ruled`: the side itself is a
  line, its middle within a billionth of its length of the line through
  its ends or the rounding there, as for the rulings above; a disc
  5e-4 thick moved 2e5 out lost its strips to that and drew twice the
  triangles; and the lines a quarter, half and three quarters of the way
  to the opposite corner each have the patch at their middle within a
  sixteenth of the chord of the line through their ends; only `+ − × ÷
  √`). The side is held to a line, not to the chord: a cut piece of a
  wall whose one-segment side was a short arc (an eighteenth of a chord
  off its line) passed at a sixteenth, and its strip fanned round the arc
  to 1.14 chords. A cylinder's strip patches (`cylinder_strip`, the
  revolve's cylinder strips) are straight along their rulings to the
  bit; a cone's are only nearly straight along them (the planes through
  the patch's projection point that hold its lines across don't hold the
  apex), a few hundredths of a chord on small ones, so most keep their
  grids; on a cylinder or cone the only straight lines are rulings, so a
  patch of any other shape (a cut piece) fails. The strip (`Level::strip`)
  joins the two other sides, which start together at the corner
  opposite the straight side, merged by their parameters (sample `s + 1`
  of `n_a` before sample `t + 1` of `n_b` when `(s + 1)·n_b < (t + 1)·n_a`,
  in integers; on a tie by the shorter diagonal), after the triangle at
  that corner: `n_a + n_b − 1` triangles, worked out from the counts
  before anything is made. Every triangle runs between points on the
  same or neighbouring lines across, within one step of one side's arc,
  so within that side's chord (at most 1.0 chord densely sampled on
  circles, an ellipse and parabola and hyperbola arcs, walls a thousandth
  to a thousand times their arcs high). On an extruded wall the two
  curved sides have one count (along rulings, above), so the strip pairs
  their samples ruling by ruling, and its quads between neighbouring
  rulings are flat (two rulings of a cylinder are parallel, of a cone
  meet at its apex): the same prism as the grid's, creased only along
  the rulings, with no points inside and 4 to 7 times fewer wall
  triangles (a cylinder of radius 3 and 5 high 1 696 triangles to 976, a
  5 × 100 one 1 384 to 808, a plate of 400 holes 200 624 to 150 304). The
  normals inside come from the edges alone, exact on cylinders (constant
  along a ruling). Planes are left out: their grids stay exactly as they
  were, and none of the test solids' plane patches passed the test
  anyway.
- **Grids that bend**: patches curved both ways (faces whose form is a
  sphere, torus, revolved conic, ellipsoid or unknown: not a plane,
  cylinder or cone, circular or not, which are straight along their
  rulings; a quadric form is a scaled sphere or cone, and the cone is the
  one written about its apex, with no linear or constant term), and the
  grids of cylinders and cones (patches that aren't ruled strips), get a
  finer grid where `m` leaves a triangle more than the chord off the
  patch; planes and ruled strips are never measured. `Plan::refine`
  measures in rounds: each patch still open is triangulated in `f64` at
  its level and measured (`level_error`: the patch at each triangle's
  middle, in parameters, and at the middle of each side that isn't an
  edge's own segment, each from the triangle's point at the same mix of
  its corners along the patch's normal there; a side inside the patch is
  two triangles', and is measured from the one it runs up in (by the
  samples' numbers), which reads the same and saves about a quarter of
  the time. The triangles measured are exactly those drawn and welded
  (`Sampled`): each sample numbered once, the corners too, so the corner
  flips, which compare the two strips' end triangles by their samples'
  numbers, are made as in drawing (with each corner numbered once per
  side, as before, they were never made in the measuring, and round
  patches were refined for corners the drawing had flipped); and their
  diagonals chosen from the same `f64` points, the mesh vertices at the
  corners and the edges' curves along the sides (`Plan::sample_point`),
  not the patch at those parameters, which rounds differently. The full distance would
  also count the patch drifting along the surface where its parameters
  run unevenly, which refined a scaled ball that was already within; the
  distance from the triangle's plane misses a triangle steep to the
  patch, as ring corners are on fat tori and cut balls, where the patch
  sits near the plane but well outside the triangle: up to 2.7 chords
  passed as within). A patch that is a single triangle is measured at
  its middle and, if too far, gets a grid of 3 steps (one inner point)
  with its edges unchanged. One too far moves to `m·√(error / chord)` steps,
  at least one more in the first four rounds and a quarter more after
  (so at most about two dozen rounds), at most `4 × 64 = 256`. The
  square law undershoots near the chord, since the ring's triangles keep
  their edge sides, and stepping by one there lands on the smallest grid
  within it on every solid below (a quarter more at once overshot by up
  to a fifth). The edges' counts, and so the shared samples, never
  change, whatever kind of face is on the other side: only the grid and
  the ring's inner sides do, and the strips join any two counts. The
  rounds stop as soon as the plan is past the limits (it is then
  refused), so each round's work is bounded by them; the counts only
  grow, so when the plan fits, every round fitted and the levels depend
  only on the mesh and the chord: the drawn and welded tessellations
  agree. A NaN error counts as within. The four samples per triangle read
  a few percent under a dense sampling (a turned ellipse's worst is 1.03
  chords, next to an edge segment; random revolves, cuts and stretched
  copies up to about 1.06). With the ring's diagonals chosen against
  the patch and its corners flipped, and measured so, the refinement is
  left to the patches that need it. At the default tolerance a ball of
  radius 2 has 4 304 triangles unrefined (worst 1.20 chords, densely
  sampled along the patches' normals) and 5 168 refined (0.99; 6 720 when
  the measuring missed the flips and saw its ring corners at 2.55), a
  hollow ball 8 656 (was 12 736), a part torus 5 628 (6 396), a spindle
  torus's outside 3 072 (6 016), a turned ellipse 7 382 (12 566), the
  round octahedron 800 to 1 280 (2 144); a full torus (major 10, minor 2)
  is within at 0.53 and keeps its 10 496. Over random revolves, their
  cuts and stretched copies no inner grid came near 256 steps nor any
  edge past its 64 segments' chord: the chord is at least a thousandth
  of the solid's diagonal, which a conic within it meets in about two
  dozen segments, so only the turn rule reaches 64, where the chord
  error is far below the chord. None of the test solids' grids on
  cylinders and cones is refined (the ring keeps them within), and
  planes and ruled strips keep exactly their levels. The measuring and
  the ring's choices take a few milliseconds on a round solid (release,
  one thread: a ball 3.5 ms, 4.4 when the measuring missed the flips; a
  hollow ball 5.3 against 9.7; a turned ellipse 3.3 against 9.2; a torus
  about 7.5 either way), and strips make walls faster (64 extruded
  cylinders 8.6 ms against 14.6).
- **Normals** are the patches' own (`Patch::normal`, normalized; the fold
  direction stands in should it vanish). Along an edge, the two sides'
  normals are compared at every sample: if they agree within 1° everywhere
  (dot at least `cos 1°`) the edge is **smooth**, its inner samples are one
  vertex each with the normalized mean normal; if not it is **split**,
  and each side gets its own vertices. Round a mesh vertex, the corners
  between two split edges are one vertex, with their normals summed and
  normalized; a vertex with no split edge is one vertex.
- **Faces.** The triangles go face by face (`RenderMesh::face_ends`), a
  face per region of the solid's `Topology` (connected triangles of one
  face key, see "Topology and names"), in the topology's order (by their
  lowest triangle), each face's triangles in the order of their patches.
  So face `i` of a solid's mesh is region `i`.
- **Feature edges** (along the edge's samples): every split edge, and
  every edge between two faces of different keys (`FaceName::key`, which
  drops the `segment` that numbers one wall's pieces). So a cylinder
  draws its two rims, not the seams between its four quarter walls, and
  a box its twelve edges, not the diagonals of its sides; a blend's or a
  revolve's pieces will draw as one face too. Flush joins draw no line
  where the two pieces of a plane or cylinder meet, since
  `Mesh::merge_faces` gave them one key ("Structure"). They're polylines
  (`RenderMesh::edge_vertices`, `edge_ends`): first the topology's
  chains, in its order, so edge `i` of a solid's mesh is chain `i`
  (chains run between regions of different keys, so all their edges are
  feature edges), along the halfedges on its first region's side, its
  two regions its faces (`edge_faces`); then the creases, feature edges
  inside one region where the normals split but no other face begins,
  with that face twice, chained through the mesh vertices where exactly
  two creases of the region meet, open ones from the lowest corner
  first, closed ones from their lowest edge. Every mesh vertex an edge
  ends at is a corner (`corners`, `edge_corners`), and an edge that
  closes without one gets a corner where it starts. Where a crease ends
  on a chain, the chain's halfedges either side may have their own
  vertices at the same position (the corner groups split at the crease):
  the polyline keeps the first, and an edge ending at its start ends on
  its first vertex.
- **Wires** (`RenderMesh::wire_vertices`, `wire_ends`, `wires`): every
  other edge record, along its first halfedge's samples, one polyline
  each in the records' order, for a wireframe. They're left out (none,
  the mesh otherwise the same) if they'd take the feature edges' points
  past the limit, which bounds the two together.
- **Parts.** A solid's mesh is one part (`part_ends`: one past its last
  face, edge, corner and wire), even empty; `RenderMesh::append` adds a part per mesh. `Solid::tessellate`
  works out the topology itself; `Solid::tessellate_with(display,
  &topology)` takes one already made.
- **Limits.** Triangle, vertex and feature-edge point counts are worked out
  from the segment counts and the refined levels before any vertex is
  made, and more than `RenderMesh::MAX_*` fails with `MeshError::TooLarge`.
  The limits are a parameter (`Limits`, `tessellate_within`) so tests
  reach them with small meshes: each part may be exactly its limit, one
  more fails. The parts go
  through `RenderMesh::from_parts`, so a position past `MAX_POSITION`
  (a mesh's control points may reach `MAX_CONTROL`) fails with `Values`.
- **Loose geometry.** A failure's evidence isn't a solid, so it's drawn
  piece by piece: `Display::flatten(curve, diagonal)` cuts a curve as an
  edge of a solid `diagonal` across is (`segments`, ends included), and
  `Display::sample_patch(patch, diagonal)` samples one patch on its own
  (`PatchSamples`: points, unit normals, triangles), its sides cut so,
  its grid refined in rounds as a face of no known form is (the step
  rule shared with `Plan::refine`, `finer`).
- **Determinism.** Counts, normals, edge points and patches are pure maps
  through `par_map`; vertex numbering is one sequential pass (corner
  groups by vertex, then edge samples by edge, then patch interiors by
  triangle).

The chord target holds on the edges, and inside patches curved both
ways (within about 1.05 chords densely sampled, as the edges are). On
planes, cylinders and cones the grid spacing follows the largest count,
and their grids are rarely refined; the ring's choice of diagonals keeps
them near the chord: on the test cylinder the worst triangle's middle
is 0.77 chords off the surface (1.85 when the strips chose by length
and the corners weren't flipped), densely sampled 0.86 (about 3 before);
on 5 × 50 to 5 × 200 cylinders within the chord densely sampled (4
before), their faces within 8° of their vertex normals. Their wall
patches are ruled strips now, with the same welded prism. Shading uses the vertex normals, so slivers whose face normals are far
from them don't show; a wall a fortieth of its arc high (20 × 0.5) no
longer makes them (its faces within 4° of their vertex normals, 84°
before). Choosing the diagonals against the patch takes two patch
evaluations per strip step, up to about 1.8 times the time to draw a
round solid (whose rounds triangulate each level), a few milliseconds;
the flat torus below is unchanged. Positions
are `f32`: far from the origin, features smaller than an `f32` step
there (about 0.03 mm at 5e5 mm) collapse into degenerate or flipped
triangles, as in any `f32` mesh, but no cracks open: both sides of an
edge read the same rounded samples.
A flat torus of 262 144 patches tessellates in about 0.13 s
(release, several threads).

Tests: a cube is 12 triangles, 24 vertices and 12 edges with axis
normals; a cylinder's normals are radial on the wall and axial on the
caps, it encloses slightly less than `πr²h`, its triangles' middles are
within the chord, the skewed corners too, only its rims are feature
edges, and a rim point is two vertices (wall and cap); the walls of tall
cylinders (5 × 50 to 5 × 200) are within the chord densely sampled, their
faces within 8° of their vertex normals, and welded, cylinders from 10 ×
20 to a 0.5 × 200 needle crease only along their rulings and rims;
round octahedra,
half cylinders, a torus, a box, and a repaired thin shell (pieces of mixed
sizes) are watertight: every triangle side is met by one running the other
way between the same positions, to the bit; edge counts meet the chord and
turn by dense sampling and one fewer wouldn't; the strip joins any two
counts; results are the same at 1 and 8 threads; far positions are
refused. Patches curved both ways (a ball, a hollow ball, a torus and a
part torus, a spindle torus's outside, a turned ellipse, the round
octahedron, at the default tolerance and a coarse one) are within 1.05
chords of their patches along the normals on a 12-step grid of every
triangle, their triangles tile each patch's parameter triangle
(positively oriented, areas summing to the domain's), and they are
watertight and weld into a `ManifoldMesh`, the same at 1 and 8 threads;
no patch curved both ways is a ruled strip; a cylinder, a drilled plate,
a thin disc and a revolved tube keep the levels their counts give, grids
or ruled strips (every wall patch of the cylinder, disc and tube, some of
the drilled hole's cut wall, never a plane), and so do a cone and a tube
scaled into quadric and conic forms, while the scaled ball's patches are
measured; walls extruded from a circle, an ellipse and parabola and
hyperbola arcs, a thousandth to a thousand times their arcs high, in a
tilted frame, are ruled strips wherever a patch has one side of one
segment, a triangle per segment less one, within 1.05 chords densely
sampled, tiling their patches, watertight and welded; only rulings make
strips (not a short arc of one segment, not a ball's patch); a cylinder
less a cylinder, less a slot and a spindle's cones, and cylinders 0.05
to 500 high drawn on grids as if they had no ruling side, are within
1.05 chords by the ring's diagonals alone, the same at 1 and 8 threads;
a thin half cylinder's quadric walls tile; a thin disc and a turned tube
moved 2e5 out have the counts, strips and triangles they have at the
origin; the round solids' and a cut
cylinder's triangles drawn, welded and measured are the same, to the
bit; a refined level counts what it makes; a part torus meets
its exact limits and fails one under each. Faces and edges: a box's 6 faces of 2 triangles are its 6
regions and its 12 edges its 12 chains; a cylinder's quarter walls are
one region and its rims two closed chains, closing on their first
vertex; two flush boxes joined are 6 regions and 12 chains, the top's
triangles of both boxes' faces one region; a flat torus's creases are
on no chain, all after the chains with one face twice; every face's
vertices lie on its region's form and every chain's on both of its
regions'; given the topology or not, the same mesh, at 1 and 8 threads.

## Manifold meshes for export (`src/manifold.rs`, `tessellate::weld`)

`Solid::manifold_mesh(&Display)` gives a `ManifoldMesh`, an indexed
triangle mesh that is a closed, oriented 2-manifold, for files such as
3MF. Its positions are `f32` values (held as `f64`) about an origin with
whole-numbered coordinates, the middle of the patch mesh's box rounded:
file readers keep single precision, so the mesh checked is the one they
read, and measuring from the middle keeps a body far out as precise as
one at the origin. Each sample is the `f64` point less the origin,
rounded to the nearest `f32`. Drawing (with picking or without, one
`tessellate::draw`) and welding work from one plan (`tessellate::Plan`:
the edges' first halfedges, curves and segment counts, the patches'
levels and inner offsets, the counts and the limit check) and one
triangulation of a patch (`Plan::patch_triangles`), so they have the
same samples and triangles, the strips of both choosing their diagonals
from the `f64` points (when drawing read its `f32` points and welding
its rounded ones about the origin, far-out and finely refined solids
differed in a few diagonals). A test holds the three together on boxes, cylinders,
curved and flat-patched solids and a joined plate at two tolerances:
the drawn mesh the same with picking, a picking entry per drawn
triangle and edge, and the welded triangles the drawn ones in order,
corner for corner within the rounding. The
welding is by identity, never by distance: a mesh vertex is one vertex
(numbered as in the patch mesh, every one used, whatever its normals),
an edge's inner samples are numbered once along its first halfedge and
read by both patches beside it, and each patch's inner points follow,
so the vertex count is exactly the drawn tessellation's lower bound.
Limits are the `RenderMesh`'s (`Limits::EXPORT`: `2^24` vertices and
triangles), counted before sampling (`TooLarge`).

`ManifoldMesh::new` is the only way to make one (deserializing goes
through it too) and checks, in this order, giving the first failure by
the lowest triangle, vertex or edge: at least one triangle (`Empty`), the
sizes, the origin whole-numbered within `MAX_POSITION` (`2 × MAX_COORD`;
`Origin`), every coordinate finite, within `MAX_POSITION` and an `f32`
value (`Position`), indices in range, no vertex twice in a triangle,
no triangle whose corners are collinear, exactly, by the boolean's exact
sign of each component of `(b − a) × (c − a)` (`Degenerate`), no two
vertices at the same position (`Coincident`, by sorting their bits), every
undirected edge used by exactly two triangles (`EdgeUse`) running it
opposite ways (`Orientation`), the triangles round each vertex one cycle
(`Fan`) and every vertex in one (`UnusedVertex`), and a positive signed
volume about the middle of its box (`InsideOut`). A shell bounding a void
faces in, so the volume is checked in total, not per shell; the solid's
own check already made every shell face the right way. A checked solid
gives a mesh that passes unless sampling breaks it: two samples rounding
to one `f32` point (a tiny curved edge, or a thin pin far from the middle
of the body it's part of) are `Coincident` or make a `Degenerate`
triangle, refused rather than written.

Tests: the check's failures one by one on a tetrahedron; a box welds to
its 8 corners and 12 triangles, volume 24; a cylinder's rim point is one
vertex; round octahedra (one far out), half cylinders, a torus, a repaired
thin shell round a void, a plate with holes, the plate joined to a boss,
and crossed cylinders united, subtracted and intersected (traced cuts),
each at the default and a fine tolerance: as many triangles as drawn,
every drawn position a welded one rounded, the volume within the chord
times the area of the solid's and equal to the drawn mesh's to `f32`
rounding, the same at 1 and 8 threads; plates with a hole 10^5 and 10^6
out weld about their own middle; a 0.01 pin 10^5 from its block is
refused (its samples round together); an origin past `MAX_POSITION`, a
position that isn't an `f32` value and the empty solid refused; limits
exact. The 3MF writer is in `varde-io`
(`agents/files.md`).
