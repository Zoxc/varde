# Kernel

`varde-kernel` (`crates/kernel`) is Varde's geometry kernel. Its solids are
closed, oriented 2-manifolds of **rational quadratic triangle patches**. It
extrudes sketch profiles exactly, and it unites, subtracts and intersects
solids the way [Manifold](https://github.com/elalish/manifold) does for flat
triangle meshes: the booleans work on the curved patches directly, never on
a flattened copy, and always give a valid manifold or an error. Drawing
works on WebGL2: the GPU only ever sees triangles the kernel tessellated.

This is the overview: what the kernel is, its main ideas and invariants, and
how an operation runs. `agents/kernel.md` has the detail (every rule, limit,
measurement and known gap, section by section, with the files that hold
them); where the two differ, `agents/kernel.md` wins, and the code wins over
both. Varde ports Manifold's structure; it doesn't link it.

## What exists

- **Patch math** (`patch`): rational quadratic curves (`Conic`) and
  triangles (`Patch`), exact splits, the fold check and normal cones, exact
  arcs and cylinder strips.
- **Meshes** (`mesh`): the halfedge structure with shared edge records,
  faces with names, surface tags and forms, `check` of the invariants, a
  BVH, exact red–green refinement and `repair`, box and cylinder meshes.
- **Solids** (`Solid`): a mesh that passed `check`; its tessellation for
  drawing (`RenderMesh`), volume and area.
- **Extrude** (`extrude`): a `Profile` (closed loops of 2D conics) placed on
  a `Frame` swept into an exact solid.
- **Swept strips and lathes** (`sweep`): exact strips on cones and quadrics
  of revolution, fitted strips, bands and pole caps for other surfaces of
  revolution (tori and the like). These are the walls revolve will be built
  from; revolve itself isn't a feature yet.
- **Booleans** (`boolean`, `touches`): union, difference and intersection of
  solids of flat and curved patches, and whether two solids meet.
- **Topology** (`Topology`): a solid's faces, edges and corners as users see
  them, with names that survive regeneration, and resolving references to
  them.
- **Infrastructure**: the design's `Tolerance`, the limits, the work
  `Budget`, `KernelError`, the deterministic parallel map (`par`) and
  platform-independent trigonometry (`trig`).

Documents store no geometry. A body is the output of the feature history,
which `varde-regen` evaluates in its lane: sketches give profiles, extrudes
make bodies or join, cut and intersect them, and the visible bodies are
tessellated and sent to the renderer.

## Main ideas

- **Rational quadratic triangles everywhere.** Each edge carries a middle
  control point and a weight; corner weights are 1 (the standard form). An
  edge's curve depends only on its own edge record, which both triangles
  beside it read, so the surface is watertight by construction. Planes,
  circles, arcs, cylinders, cones and quadrics of revolution are exact;
  flat triangles are the case where every edge is straight with weight 1.
- **Topology from counting, never from distance.** Every topological fact
  of a boolean comes from a few primitives, each worked out once and stored
  by the pair it is about, through identities that hold whatever values
  the primitives take. Nothing is ever merged because two points are
  within some ε. This is Manifold's guarantee: valid topology, approximate
  geometry. A numerically wrong primitive gives a slightly wrong solid,
  still a manifold, or an error; never an invalid mesh.
- **`check` is the trust boundary.** A mesh becomes a `Solid` only by
  passing `check`. Every operation ends in it, so every solid in the system
  satisfies the invariants below.
- **Derived, never stored.** The `.vrdp` file holds the feature recipe;
  solids, meshes and tessellations are rebuilt from it and cached.
- **`f64` in the kernel, `f32` only for drawing.** `RenderMesh` is the
  boundary to the renderer.
- **Deterministic.** The same input gives the same bits at any thread count,
  natively and on the web.
- **Bounded.** Every operation runs within a work budget and fixed limits,
  and an input past them is an error (`TooComplex`), never a hang. Distances
  and counts that come from forms or files are bounded or use checked
  arithmetic.
- **Never a wrong `Ok`.** Where the kernel can't do an operation right it
  fails; the error kinds say whether a finer tolerance, a coarser one, or a
  different model would help.

## The patch

In homogeneous form, with corner weights 1 and edge weights `w01, w12, w20`:

```text
        u²·P0 + v²·P1 + w²·P2 + 2uv·w01·C01 + 2vw·w12·C12 + 2wu·w20·C20
P  =  ─────────────────────────────────────────────────────────────────
             u² + v² + w² + 2uv·w01 + 2vw·w12 + 2wu·w20
```

- **Exact splits.** De Casteljau and blossoming run on the homogeneous
  control points (`w·C, w`), which are polynomial. Renormalizing a piece to
  the standard form turns an edge weight into `w / (√wa·√wb)`, which
  depends only on that edge's own data, so the two patches beside a split
  edge get the same bits. Halving at `½` is written symmetrically; at other
  positions a shared edge is split once and the halves handed to both sides.
- **The fold check.** The normal's numerator is a cubic with ten Bernstein
  coefficients. A patch passes when some direction has a positive dot
  product (with a margin) with all ten; then the normal never vanishes or
  flips. The check is preserved by splitting, and the smallest cone round
  the coefficients is also the patch's **normal cone**, whose apartness
  certifies that two patches can't meet in a closed loop.
- **Control hulls.** With positive weights a patch lies in the convex hull
  of its six control points, which the BVH, the hull rules and the
  certificates rely on.
- **Exact quadric patches.** A rational quadratic triangle lies on a
  quadric when its three boundary conics do and their planes meet in one
  point of the quadric. That is how an extruded arc becomes an exact
  cylinder strip (a straight ruling, the arc, and a sheared diagonal),
  how cone strips get exact rulings (control point at the geometric mean of
  the ends' distances from the apex), and how a boolean rebuilds the
  triangles of a cut cylinder exactly.
- **Weights** stay in `[1/64, 64]`; arcs are at most 90° per segment, with
  weight `cos(θ/2)`. Arcs whose ends are known are built from points with
  `+ − × ÷ √` only (`arc_between`).

## The mesh and its invariants

A halfedge mesh in Manifold's style: vertices (patch corners), one `Edge`
record per undirected edge (control point and weight, shared by both
halfedges), triangles of three halfedges and a face. Ids are `u32`, at most
`MAX_PATCHES` (`1 << 22`) patches.

Each **face** carries:

- a **name** (`FaceName { feature, part, instance }`), stable across
  regenerations (see "Topology and names");
- a **surface tag** (`Plane`, `Quadric` or `Free`): the construction's
  claim, which booleans cut by and `check` verifies against the patches;
- a **form**: what surface it was meant to be (plane, cylinder, conic
  cylinder, cone, sphere, torus, revolved conic), with parameters. Fitted
  faces are on their form only within the fit tolerance; debug builds check
  every face against its form;
- **aliases**: the keys of faces merged into it, so references to them
  still resolve.

`check` holds six invariants, in order, with deterministic first errors:

1. **Topology**: halfedges paired, one fan per vertex, counts in range.
2. **Shared edges**: both halfedges of an edge read one record; every patch
   within the coordinate and weight bounds.
3. **Fold**: every patch passes the fold check.
4. **Control hulls**: non-neighbours' hulls more than the resolution apart
   (GJK); edge and vertex neighbours separated by a plane through what they
   share, with rules for straight and curved shared edges; across a
   curved edge the plane fails on, by the cylinder over the edge's conic
   (the signs of a quadratic that vanishes on it, one per patch). For flat
   triangles this says exactly that the mesh is embedded.
5. **Orientation**: every shell faces out, or in where it bounds a void, so
   the winding number is 0 or 1 everywhere (shell volume signs with error
   bounds and exact fallbacks, nesting by exact, perturbed rays).
6. **Face tags**: patches lie within the resolution of their claimed plane
   or quadric.

**Refinement** is red–green and exact, every split at `½`, so neighbours
across a split edge are bisected to the bit and pieces keep their shapes.
Plane faces are split with straight inner edges. **Repair** restores
invariants 3 and 4 by splitting what fails, round after round, and fails at
once where no split can mend it: a degenerate corner, two flat pieces
failing a hull rule, a pair of surfaces found closer than the resolution (a
witness), or pieces at the size floor.

## Tolerances

The design's **fit tolerance** (`Tolerance`, in mm, `1e-5 ..= 1e-1`, default
`1e-3`, i.e. 1 µm) is how far a fitted curve or patch may be from the true
surface. The **resolution** is a thousandth of it: the margin the hull rules
keep and the scale where refinement stops. The booleans take heights within
a 64th of the resolution (the tie distance) as ties. None of these ever
decides that two things are the same vertex.

## Building solids

**Primitives.** `Mesh::cuboid` and `Mesh::cylinder` are built as an extrude
builds its solids (the cylinder from four exact quarter arcs). They are for
tests; documents only make bodies by extruding.

**Profiles.** The kernel knows no sketch. Regen turns sketch regions into a
`Profile`: loops of `Segment { conic, curve }`, outer loops counter-clockwise
and holes clockwise, closing to the bit. Lines are exact, arcs and circles
exact (at most 90° a segment, no trig), and splines are the one
approximation: their Bézier spans are fitted in runs by lines and conics
within the fit tolerance, tangent-continuous where the spline is.

**Extrude** sweeps a profile along its frame's normal between two distances:

1. **Chain**: near-straight segments become lines; cusps are refused.
2. **Separation**: segments' control hulls kept more than the resolution
   apart (curved ones halved until they are), so the chord polygon is simple
   and each bulge lies clear of everything else.
3. **Caps**: a constrained Delaunay triangulation (`spade`) of the chords,
   classified by winding number, with curved boundary edges; rounds of
   Steiner points and halvings until every corner is open and every patch
   passes the fold check, and refinement for quality: Steiner points at
   the circumcentres of triangles with an angle under 5° (Ruppert's way,
   inserted into the triangulation kept from round to round), chords
   halved where those would encroach, and caps whose triangles' boxes
   still crowd each other refined once more. A second try mends flat
   corners if the first fails, and the last two are the plain, unrefined
   caps' own two tries.
4. **Walls**: two patches per segment from the exact cylinder strip, sharing
   their top and bottom edges with the caps. Faces: `StartCap`, `EndCap`,
   and `Side { curve, segment }` per profile segment, tagged with their
   planes or the cylinder over their conic.
5. **Check, and repair only if that fails**; faces on one surface merged.

**Swept strips and lathes.** A strip is two patches between a bottom and a
top curve and two side curves; only its diagonal is new. Cone strips and
strips on quadrics of revolution (sphere, ellipsoid, paraboloid,
hyperboloids) between parallels and meridians are exact. Where no exact
strip exists (a torus, another conic turned about an axis), a fitted strip
keeps its four exact edges and fits the diagonal to the face's form by
damped Gauss–Newton. A `Lathe` turns a meridian about an axis into bands of
such strips, halving along the meridian or round the axis until every strip
is within half the fit tolerance and sound, and closes poles and apexes with
fitted caps. At a turn of the meridian's height no plane through the
parallel separates the strips beside it, but the cylinder over it does,
so profile vertices may sit at turns; bands still keep their own rings
off turns (cheaper), but for turns within 1 % of a piece's end.

## The boolean pipeline

`boolean(a, b, op, tol, budget)` follows Manifold's `boolean3.cpp` and
`boolean_result.cpp`. Both operands are solids, so their invariants hold.

1. **Broad phase.** BVHs over each operand's patch boxes give candidate
   pairs, counted against the budget before they are collected.
2. **Primitives, each computed once**, along one fixed projection direction
   `UP = (2, 3, 32)`, tilted off every axis:
   - `s02(v, f)`: the signed number of layers of face `f` above vertex `v`
     (+1 facing up, −1 facing down; folds in projection cancel);
   - `s11(e, g)`: how two edges' shadows cross, and which is above;
   - the crossings of an edge through a face, and their order along it.
3. **Counting.** Manifold's identity gives each edge–face crossing count:
   `x12(e, f) = s02(b, f) − s02(a, f) − Σ s11(e, h)` over `f`'s edges.
   Winding numbers propagate from one ray per connected part along the
   edges, and must agree everywhere and be 0 or 1, else `Inconsistent`.
   Each face pair then has as many cut ends going in as out.
4. **Pairs of faces.** Flat pairs have two ends or none: one straight arc.
   A curved pair is decided from its ends only with a **certificate** that
   no closed loop hides in it (normal cones apart, both planar, a plane
   against a cylinder patch, or hulls apart with no ends). Two patches on
   one surface have no cut. Any other pair is **refined**: both operands
   are split red–green where the pair is, and everything is counted again;
   pairs at the size floor are decided by fixed rules (no certificate means
   no loop; ends joined in order round their middle).
5. **Chains.** Each arc becomes a chain of shared edge records: a straight
   edge between two planes; the exact conic where a plane cuts a quadric;
   elsewhere (quadric against quadric, free surfaces) **traced**
   (predictor–corrector on `P_A = P_B`) and **fitted** as conics within a
   quarter of the fit tolerance. A failed trace falls back to a simpler
   curve between the same ends, kept only if it is verified to follow the
   true cut; otherwise the operation is `Inconsistent`.
6. **Assembly.** New vertices are records ("edge `e` through face `f`,
   crossing `i`"), and halfedges pair by record, never by position. Each
   part is kept by its winding number for the operation (`A − B` keeps `B`'s
   inside of `A` turned over). Each cut face is triangulated in its own
   layout (affine coordinates for flat and planar patches, the parameter
   domain for curved ones) by ear clipping, its inner edges the patch's own
   curves, so the pieces lie exactly on it; on quadrics, inner edges are
   chosen to make exact bands. Rounds halve curves until triangles are
   sound and within the fit tolerance of their surface. A triangle that is
   only within the fit tolerance of a quadric goes on a copy of its face
   claiming no surface, so the tags stay true. Refined pieces far from any
   cut are merged back into the operand's original patches.
7. **Clean-up.** The curved version of Manifold's degenerate clean-up on the
   triangle soup: collapse edges of about zero length, flip triangles of
   zero height, drop parts enclosing no volume, straighten and dissolve
   seams where flush caps with curved rims meet in one plane (merging the
   faces, their names becoming aliases), and flip slivers on plane faces
   towards Delaunay.
8. **Repair and check**, then the result is a `Solid`.

**Ties and flush geometry.** CAD makes ties on purpose (flush faces, a
vertex on a face). For flat geometry the predicates are exact (float with
error bounds, then expansions) under a **symbolic perturbation**: `A`'s
vertices move by `ε·s·n_v + ε²·T2 + ε³·T3`, outward for a union (flush faces
overlap and merge) and inward for a difference or intersection (flush faces
part cleanly). The perturbed operands are a real configuration in general
position, so every decision fits the others. Near ties within the tie
distance are decided as the exact tie they stand for, and flat operands that
come out `Inconsistent` are decided again with exact signs. Curved
primitives decide their ties by the same perturbation to first order in each
power. Curved shadow crossings are derived from shared ray tests rather than
solved pair by pair, so the counting's balance holds by construction.

**`touches`** runs the broad phase and one counting; with curved patches,
where the counting shows nothing, a depth-first search for surfaces within
the resolution decides. Regen uses it to pick the bodies a join, cut or
intersect works on.

**Results that aren't manifolds** (boxes sharing only an edge, united) fail
with `Invalid`; the same operands intersected or subtracted the other way
work.

## Exact and fitted

| geometry | how it is represented |
|---|---|
| lines, planes, arcs, circles | exact |
| extruded walls over lines and conics | exact planes and cylinders over the conic |
| cone strips, quadrics of revolution between parallels and meridians | exact |
| tori and other surfaces of revolution, poles and apexes | fitted within half the fit tolerance |
| spline profiles | fitted chains of conics within the fit tolerance |
| plane ∩ plane, plane ∩ quadric (boolean cuts) | exact lines and conics |
| quadric ∩ quadric, anything on a `Free` face | traced and fitted within a quarter of the fit tolerance |
| bands beside a cut | exact on planes and quadrics where the common-point construction allows, else within the fit tolerance on a claim-free copy of the face |

Exactness only improves geometry. Topology comes from the same counting
either way.

## Drawing

`Solid::tessellate` makes a `RenderMesh` on the CPU, in the regen lane:

- Each edge record gets a number of equal parameter steps from its own curve
  (chord error within the fit tolerance or a thousandth of the solid's
  diagonal, whichever is larger; at most 10° of turn a step; at most 64), and
  its samples are evaluated once, so neighbours share boundary points to the
  bit: no cracks.
- Patch interiors are a regular barycentric grid stitched to the edges.
- Normals are the patches' analytic ones, shared across an edge where the
  two sides agree within 1°, split otherwise.
- Feature edges are split edges and edges between faces of different keys,
  so a cylinder draws its rims, not its quarter-wall seams.
- Counts are worked out before evaluating, and meshes past `RenderMesh`'s
  limits fail rather than allocate.

The renderer, wire format and `MeshFeed` are unaware of patches.
Evaluating patches in the vertex shader (a barycentric grid, control data in
a float texture, per-edge levels) is a possible later step if CPU
tessellation gets too slow.

## Measuring

`Solid::volume` and `Solid::area` integrate over each patch's parameter
triangle by fixed Gauss–Legendre rules (patches with weights far from 1
split first), summed in patch order. The tests hold them to `1e-12` relative
on boxes, cylinders and extrudes.

## Topology and names

`Topology::new(&solid)` derives what users see, never stored:

- **Regions**: connected triangles of one face key (a circle's four quarter
  walls are one region; a face cut in two by a groove is two of one key).
- **Chains**: maximal paths of edges between the same two regions.
- **Corners**: vertices where three or more regions meet.

A `FaceName { feature, part, instance }` is made only from what a feature
was given (its id, the sketch curve ids, references), never from mesh
indices or positions, so it survives regeneration with other dimensions,
another tolerance or another triangulation. `FacePart` is `StartCap`,
`EndCap`, `Side { curve, segment }`, `Split(n)`, with parts reserved for
later features (blends, offsets, sweeps, lofts). A `FaceKey` drops what
numbers the pieces of one surface; keys are what references store. Where
faces merge (flush caps joined, or a face dropped as flush in a union or
intersection), their keys become **aliases** of the face that took them
in. `Topology::face`, `edge` and `corner` resolve keys to regions, chains
and corners, choosing among several candidates of one name by distance to
a point.

## Determinism

`par_map` is the only code that knows about rayon (natively; on wasm it is
sequential). Every parallel step is a pure map over input sorted by stable
keys, collected in order; floating-point reductions are done afterwards,
sequentially in key order; new ids are handed out in one sequential pass; no
iteration order comes from a hasher. Angles go through `trig`, which wraps
the pure-Rust `libm`, and the crate's `clippy.toml` refuses std's platform
maths, so native and web builds give the same bits. Tests compare results
at 1 and 8 threads, and native against wasm hashes were checked when this
went in.

## Budgets, limits and errors

An operation takes a `Budget` (at most `MAX_WORK`, `1 << 22` units, about
two seconds on one thread) and counts it down in sequential passes, so
running out doesn't depend on the thread count. Fixed limits bound patch
counts, refinement depth (`MAX_REFINE_DEPTH` 24), trace steps
(`MAX_TRACE_STEPS` 4096), rounds of cutting and mending, and profile size
(`MAX_PROFILE_SEGMENTS`). Coordinates are within `MAX_COORD` (`1e6`).

`KernelError` is:

- `TooComplex`: the budget or a limit ran out;
- `Invalid(CheckError)`: the input or result breaks an invariant the
  operation can't restore (a solid too thin for its resolution, a result
  that isn't a manifold);
- `Patch`: a bad parameter;
- `Profile`: a profile that can't be extruded (touching, crossing, nesting
  wrongly, cusped, or detail too fine for the tolerance: `TooFine`);
- `Boolean`: `Inconsistent` (decisions that don't fit together) or
  `Degenerate` (a face that can't be triangulated).

## In the app

`varde-regen` evaluates the history in its lane: a sketch's profiles, then
each extrude's tool solid. A new body takes the tool; a join, cut or
intersect asks `touches` of every earlier body not excluded and runs the
boolean against each one touched, the body first so its face names win
(a join touching several bodies merges them into the first). A failing
feature changes no body, and later features still run. Every result
(profiles, tool solids, touch answers, booleans, meshes, the drawn scene) is
cached under a 128-bit key of its inputs, bounded by size (256 MiB natively,
64 MiB on the web), so an edit reruns only what depends on it. Drafts (an
extrude being set up) are regenerated the same way. See `agents/kernel.md`
("Bodies from the history") and `notes/Threading.md`.

## Limits and open areas

- **Tangencies.** Surfaces touching along a line leave corners of zero
  angle that no patch holds, and unions of solids meeting only along a line
  aren't manifolds: such operations fail (`Invalid`), or where refinement
  along the line converges come out right with many patches. Coaxial walls
  a little apart, or of different conics meeting smoothly, refine until
  they run out of budget.
- **Cap quality.** Refined caps cost patches (plates with holes 10 to
  20% more, thin ribs and rings far more); caps past about 65 000
  segments run out of budget; short curved segments at coarse tolerances
  can still leave slivers refinement doesn't reach (`Invalid`); and a cap
  triangle with two curved sides (one concave with weight above 1) can
  fold when a later boolean splits it.
- **Fitted bands lose their claim.** Triangles along a fitted cut go on a
  face copy claiming no surface, so later booleans trace and fit there
  instead of cutting exactly.
- **Plane sections** nearly along a cylinder's rulings, and on quadrics
  other than elliptic cylinders, take less exact paths (tracing, or weights
  from a sampled point).
- **Cost.** Much of a boolean's work scales with both whole operands
  (refinement rounds, clean-up, repair, the check), so bodies past some
  50 000 to 200 000 patches can take no boolean within the budget. A
  failing operation runs for seconds, and on the web the regen worker is
  single-threaded and a running operation isn't interrupted.
- **Not built yet**: revolve as a feature (the strips and lathes are
  ready), taper, sketches on faces, fillets and other features the reserved
  face parts are for, picking by face, a batch boolean of many operands,
  GPU patch evaluation, and wasm threads (which would need cross-origin
  isolation; results would be identical either way).

`agents/kernel.md` lists the known gaps of each part in detail, with
reproductions and measurements.
