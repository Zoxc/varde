# Meshes (`src/mesh.rs`, `src/mesh/`)

| file | holds |
|---|---|
| `mesh.rs` | `Mesh`, `Edge`, `Halfedge`, `Tri`, accessors |
| `mesh/face.rs` | `Face`, `FaceName`, `FacePart`, `FaceKey`, `PartKey`, `Surface`, `Quadric` |
| `mesh/form.rs` | `Form`: what surface a face was meant to be, distances to it, the circle test |
| `mesh/build.rs` | `MeshBuilder`: triangles by vertex id, paired up |
| `mesh/check.rs` | `Mesh::check`, `Mesh::check_faces`, `CheckError` |
| `mesh/bvh.rs` | `Bvh`: boxes, queries, self pairs |
| `mesh/hull.rs` | GJK (`apart`) and the three hull rules |
| `mesh/orient.rs` | invariant 5: shells, their volume signs, their nesting by rays |
| `mesh/refine.rs` | red–green refinement: leaves, pieces, the split rules |
| `mesh/repair.rs` | `Mesh::repair`: test, split what fails, test again |
| `mesh/merge.rs` | `Mesh::merge_faces`: adjacent faces on one plane or quadric named as one, their aliases |
| `mesh/primitive.rs` | `Mesh::cuboid`, `Mesh::cylinder` |
| `mesh/tests.rs` and `mesh/*/tests.rs` | hand-built solids, one bad mesh per rule, determinism |

## Structure

A halfedge mesh in the Manifold style. `verts: Vec<DVec3>` are the patch
corners. `edges: Vec<Edge>` holds, once per undirected edge, the middle
control point and weight (`Edge { ctrl, weight }`), shared by the two
halfedges that run along it: neighbours trace the same curve because they
read the same record, not because two copies agree. `tris: Vec<Tri>`, each
three `Halfedge { start, pair, edge }` and a `face`. Halfedge `h` is corner
`h % 3` of triangle `h / 3` and runs to the next corner (`Mesh::next`);
corners run counter-clockwise seen from outside. `Mesh::patch(t)` is
triangle `t` as a `Patch`: corners from the starts, edge `i` from halfedge
`i`'s record. All ids are `u32`; `MAX_PATCHES` (`1 << 22`) keeps three per
patch well inside.

`faces: Vec<Face>`: `Face { name: FaceName, surface: Surface, form: Form,
slack: f64 }` (the form and slack below, under "Forms").
`FaceName { feature: u64, part: FacePart, instance: u64 }` is stable
across regenerations (see "Topology and names"); `FacePart` is
`StartCap`, `EndCap`, `Side { curve, segment }`, `Split(n)` (a face an
operation made), and the parts later features will make (`Blend`,
`Corner`, `Offset`, `BackSide`, `Swept`, `Lofted`); `instance` is 0 but
on copies. `FaceName::new(feature, part)` is a name that isn't a copy.
Beside the faces, `aliases: Vec<(u32, FaceKey)>`: the keys of faces
merged into each face, sorted by face then key, without repeats or a
face's own key (`Mesh::with_aliases` puts them so, `Mesh::aliases` and
`Mesh::face_aliases(f)` read them). They are a table of their own so
`Face` stays `Copy`; invariant 1 checks that each names a face that
exists (`CheckError::Alias`), and everything that carries faces carries
them: `MeshBuilder::alias`, refinement and repair (the same face
indices), booleans ("Assembly"). `Surface` is the construction's
claim: `Plane { n, d }` (`n·x = d`, `n` any length), `Quadric`, or `Free`.
`Quadric { origin, a, b, c }` is `F(x) = y·(a·y) + 2b·y + c` with `y = x −
origin`: measuring from a point near the surface keeps the rounding of `F`
relative to the quadric's size. `Quadric::cylinder(point, axis, radius)`
builds a circular cylinder, `Quadric::sphere(centre, radius)` a sphere
(around its centre), `Quadric::cone(apex, axis, cos, sin)` a circular
cone of that half-angle (both nappes, `cos²·|y|² − (y·axis)²` from the
apex), and `Quadric::revolution(origin, axis, r0, r1, r2)` the quadric of
revolution `ρ² = r0 + r1·h + r2·h²` (`h` along the axis from `origin`). The distance to it is taken to first order,
`|F| / |∇F|`, and is infinite where the gradient overflows (a finite `F`
over it would put every point on the surface). A plane's `n` and `d` are
divided by `n`'s largest coordinate before measuring, so a normal of any
finite size gives the same distances (its length could overflow, putting
every point on the plane, or its square underflow).

**One surface, one face** (`mesh/merge.rs`). A face as users see it (a
key: drawn, picked and referred to) is an edge-connected region of one
surface: adjacent faces on the same plane or quadric carry one key.
`Mesh::merge_faces(resolution, work)` makes it so; booleans run it after
repair, extrude after its repair. Two faces are one where they share an
edge and both are planes (unit normals' dot above `1 − 1e-12`) or both
quadrics (the two patches' normals at the edge's middle facing alike),
the triangles at the edge lying on the other face's surface within
`small` (`resolution / 8`, `on_surface`: a plane's six control points, a
quadric's 15 samples): for planes each on the other's, for quadrics one
on the other's. Halfedges are taken in order, sets joined by union–find
with the lower index as root. Each set is then validated once: every
patch of every member but one on that one's surface within `small`, so
a chain of near-equal surfaces can't drift. That one is the widest
member (its vertices' box), not the root: a quadric is written in its
own face's coordinates (`conic_cylinder` round the arc), and a short
arc's, met across the circle, rounds off by many times `small` (a circle
of radius 300 drawn as separate arcs, one of them 2e-3 of a turn, at the
finest tolerance: `3e-8` off at a bar of `1.25e-9`), which also fails
the pair test the other way round, so a circle of separate arcs stayed
as many faces. A member off the widest one's surface stays as it was,
and the rest join again (a second unit a patch) across the edges between
them, so faces that met only through it stay apart. A merged set takes
the root's name (a
member already of its key, another arc of the same circle, keeps its own
name, which numbers the piece), and every member gets the set's aliases:
the members' keys and their aliases (`Mesh::with_aliases` drops a face's
own key). Faces claiming no surface (a boolean's copies) never merge on
geometry, but a copy whose name was a member's takes the set's name and
aliases. Only names change: the members stay their own entries of
`faces`, each with its own surface and form, and vertices, edges, patches and
triangles' face indices are untouched. A circle's arcs' walls are
written in each arc's own coordinates (`conic_cylinder`), best
conditioned near it; giving the opposite arc the first one's quadric
moved later booleans' exact paths off (a plate's eight drilled holes
under a boss's rim came back `Inconsistent`), and the clean-up's rules
read face indices. So nothing a boolean decides by tags or indices
changes, and a set is one face wherever keys count. Several entries of
`faces` can so carry one name: whatever counts faces (tessellation,
topology's regions, picking) counts keys or regions, never entries.
Faces a real step
apart stay two (box tops `1e-3` apart, radii 1 and 1.001 stacked; tops
meeting at a crease of `2e-7` a unit, which leaves them `2e-7` apart
across a unit face, draw the crease's line). Faces meeting only at a
vertex don't merge (a U's two prongs' tops), and flush stacks on turned
frames `3e4` from the origin merge as upright ones do. A unit of work a
patch, the patch tests a parallel map over the members' triangles in
order: deterministic.

The fields are private to `mesh` (its child modules, such as refinement,
edit them directly). `Mesh::from_parts` takes the four tables unchecked.
`MeshBuilder` takes vertices, faces, curved edges (`edge(a, b, ctrl, w)`,
either way round; others are straight) and triangles by vertex id, and
pairs halfedge `a → b` with `b → a`: topology, never a comparison of
positions. It numbers edges in the order their first halfedge comes and
refuses repeated or unpaired directed edges, bad triangles and curves no
triangle uses. What it builds still has to pass `check`.

## The invariants and `check`

`Mesh::check(&Tolerance)` is where a mesh becomes trusted; the empty mesh
passes. It returns the first failure in the order below, and within a rule
by the lowest halfedge, vertex, edge or triangle (triangle pairs
lexicographically), so a mesh always gives the same `CheckError`, in debug
and release builds alike (face tags come last).

1. **Topology** (sequential): at most `MAX_PATCHES` triangles; no more
   vertices than halfedges, exactly half as many edges as halfedges, and at
   most `MAX_PATCHES` faces (`Counts`); every index in range (`Index`);
   `pair(pair(h)) = h ≠ pair(h)`, with the pair running between the same
   two vertices the other way (`Pair`); no halfedge from a vertex to itself
   (`Loop`); directed edges unique (`DirectedEdge`); every vertex starts
   some halfedge, and walking round it by `next(pair(h))` from its first
   halfedge visits all of them before coming back: one fan (`Fan`). Faces
   no triangle uses are allowed.
2. **Shared edges**: `edge(h) = edge(pair(h))` (`SharedEdge`), each edge
   used by exactly two halfedges (`EdgeUse`), and every patch within the
   coordinate and weight bounds of `Patch::check` (`Patch`).
3. **Fold**: every patch has a `fold_direction` (`Fold`).
4. **Control hulls** (below): `Hull`, `EdgeNeighbours`,
   `VertexNeighbours`, and `SameCorners` for two triangles on the same
   three vertices, which no plane can split.
5. **Orientation** (below), in every build: every shell faces out, or in
   where it bounds a void, so the winding number is 0 or 1 everywhere
   (`InsideOut(t)`, `t` the lowest triangle of the first bad shell).
6. **Face tags**, in every build (so every `Solid`'s tags are true
   claims), and on their own with `check_faces` on a mesh that passes the
   rest: a patch
   on a `Plane` has all six control points within the resolution of it
   (`Face`); a
   patch on a `Quadric` has 15 points (a grid four steps along each edge)
   within the resolution to first order; and a patch whose face claims a
   `Plane` or has a plane form (also a face claiming no surface, a copy
   the boolean made of a face with triangles off its plane, or an input
   moved without its tags) has its normal at its middle (barycentric
   `(⅓, ⅓, ⅓)`) along each of those planes' `n`, which points out of the
   solid (`FacesAgainst`; NaN fails). A plane with a zero or non-finite
   normal fails. It is cheap next to 1 to 4 (release, measured): about 13%
   of their time single-threaded on extruded plates with 16 and 64 holes
   (1 004 and 4 012 patches), 6 to 10% on 7 threads, and 0.5% on a
   262 144-patch torus of `Free` faces. The facing test costs about 40 ns
   a patch, under 1% of `check` (0.15 of 16.6 ms on a plate with 64 holes,
   3 756 patches). Before it, only the debug form check looked at which
   way a plane face faced (and booleans, at the forms only): flush faces a
   hair apart could give an `Ok` whose triangle sat on the other
   operand's face, facing against its tag (see "Exact predicates and the
   perturbation"), and caps a tie apart one facing against its form
   (see "Forms"). Booleans give such results near ties, so it is a
   refusal in every build, not the debug panic a construction's bug is.

Debug builds then check the faces' forms (see "Forms"): a triangle with a
sample further than the fit tolerance from its face's form panics. A form
is the construction's promise, so that is a bug, not an input to refuse
(and release builds can't tell it, so `check`'s errors stay the same in
both). Which way a plane form faces is step 6's, in every build.

Steps 2–3, 6 and the hull tests of 4 run per patch or per pair through
`par_map`, and step 5 per triangle and per shell. `check_counted` is
`check` returning how many patches step 5 integrated, for callers that
charge work; `check_counted_within` is that, charged as repair's first
pass over the mesh would be (two units a patch, one a pair of boxes
within the resolution, counted before they are collected, so crowded
boxes run out of budget rather than memory), for
`Solid::new_repaired_within`, which checks a mesh built to pass (an
extrude's) before repairing it, merges its faces (`Mesh::merge_faces`,
which changes only names, so only `check_topology` runs again), and
repairs, merges and checks it again only if the check fails: repair
keeps a mesh that passes as it is, so the solid is the same, for one
pass over the pairs rather than two (a mesh failing after the fold
check pays that pass twice, the check's and repair's);
`check_embedding` (repair's) stops after step 4. `check_counted_near(tol,
tested)` runs the fold check only on the triangles `tested` marks and the
hull rules only on the pairs with one of them (`Bvh::self_pairs_touching`,
in `self_pairs`' order), the rest whole: for a boolean's result, whose
other triangles and pairs are known to pass (see "Repair and the check
near the change" under Booleans), it gives the whole check's result and
error. Where they fail as
`Invalid`, `new_repaired_within` and `Solid::finished` return a
`Failure` with what the error names, of the mesh that failed: the
pieces repair names, or the triangles the check does (see "Check
evidence" under "Limits, budgets and errors").

## Forms (`mesh/form.rs`)

`Face::form` is what surface the construction meant the face to be, with
its parameters; `surface` stays the claim the kernel cuts by and checks to
the resolution. Offsets, fillets, measuring and picking will read forms;
fitted faces are on theirs only within the fit tolerance, and a copy a
boolean makes claim-free keeps it (it is intent, not a claim).

```rust
pub enum Form {
    Unknown,
    Plane { n: DVec3, d: f64 },                     // n unit, out of the solid
    Cylinder { point: DVec3, axis: DVec3, radius: f64 },
    ConicCylinder { conic: Conic3, along: DVec3 },  // over a non-circular conic
    Cone { apex: DVec3, axis: DVec3, cos: f64, sin: f64 },  // one nappe, axis into it
    Sphere { centre: DVec3, radius: f64 },
    Torus { centre: DVec3, axis: DVec3, major: f64, minor: f64 },
    Revolved { origin: DVec3, axis: DVec3, meridian: Conic2 },  // meridian in (ρ, h)
    Quadric(Quadric),   // none of the above: a cone or sphere scaled per axis
}
```

Axes are unit; a half-angle is its cosine and sine (no trig). Only a
plane's form says which way the face faces (`n` out of the solid, as its
tag's); a curved form is the same surface either way, and which side is
out is the patches' normals'. So `Form::flipped` turns a plane round and
leaves the rest. `Form::plane(n, d)` takes any length of `n` (dividing
both by it), `Unknown` for a zero or non-finite one. `Form::distance` is
exact for planes, cylinders, cones (behind the apex, the distance to it),
spheres and tori, and first order for the conic forms (`|F|/|∇F|` of
`λ1² − 4w²·λ0·λ2` on the conic's control triangle, along the cylinder or
in the meridian half-plane) and quadrics (`|F|/|∇F|`, as their claims
are measured), 0 for `Unknown`. `Form::signed` gives the
signed distance with the unit direction it grows along, which fitted
strips are fitted against (`None` for `Unknown` and where the direction
isn't defined, on an axis or a tube's centre circle).

Who sets them: the box (planes), the cylinder (planes, `Cylinder` along
`+z`), extrude (planes for caps and straight walls; a curved wall a
`Cylinder` along the normal if its conic is a circle's arc, else a
`ConicCylinder`). Booleans keep every face's form, the copies claiming no
surface included (they copy the face), and a difference turns `B`'s
round with its tags. The circle test (`circle_of`) asks the control point
to be off the chord and equally far from the ends, and the weight to be
half the chord over that distance, within `1e-10` of the arc's size plus
64 roundings of its coordinates (a tiny arc far out still counts; one
whose sagitta is under that is a line to rounding); the centre is `c +
(m − c)·|c − p0|²/|c − m|²` (`m` the chord's middle), the radius the mean
distance to the ends. It names intent only; no topology depends on it.

Checked in debug builds by `check` (above) within the fit tolerance
times the face's `slack`, so every construction and boolean in the debug
test suites holds its forms. `slack` is 1 as built; a scale up multiplies
it by the motion's stretch (see "Transforms and assembly"), since it maps
fitted patches exactly but stretches their distance from the form with
them. Booleans copy it with the face, one more on a fitted face
(claiming no surface) that they cut: a piece along a cut strays from the
patch it is cut from by up to the fit tolerance (the last round's limit),
the patch from the form by `slack × fit` (a ball's cap cut by a turned
L strayed `1.03e-3` from its sphere at fit `1e-3`); the seams' merge of plane faces
gives the face kept the larger of the two (the merge pass of one surface,
one face keeps every entry's own). A later fit against a form starts
`slack × fit` from it.
Tests (`mesh/form/tests.rs`): plane normalizing and flipping, distances
against points placed off each form, conic forms to first order on both
arcs of the conic, circles told from ellipse arcs and from a weight off by
`1e-6` (and small arcs `9e5` out still circles), forms moved rigidly
measuring the same; the forms extrude, the box and the cylinder give (a
D of a line and an ellipse arc with a round hole, also tilted); booleans
keeping forms and a difference turning the tools' round; a form the
patches are off panicking in debug builds, and one within the fit
passing; a plane form facing in refused by `check` in every build,
claiming its plane or no surface (and in `mesh/tests.rs`).

A boolean's result with a triangle on a face with a plane form that
faces against the form's normal at its middle is refused, in every
build, by `check`'s step 6 (`Invalid(FacesAgainst)`; it was a separate
look after the check, `boolean::facing`, before step 6 tested forms,
which also made debug builds panic on these first): faces within a tie
of each other gave such results in release builds, the triangles of one cap
flush with the other's carrying the other's name (a unit cylinder less
a tangent one whose top is a tie over its own, on a frame turned and
some 3 700 from the origin; a cylinder with half its upper part cut
away by a plane a tie off its axis, less a cylinder 100 resolutions
into its wall across that plane and a tie under its top, where walls joined along their
lines: almost half the top named after the other's cap; boxes on a grid turned off the axes,
moved a tie along the counting's `UP`, one less the other). An
operation that joined lines and so fails is tried again without them,
as for any other failure. The refusal is part of each try's check,
so a result refused by it after the fold rule fired is also made again
without the rule ("Unfold"). The cases seen so far had the bad triangle
on a face claiming its plane; a copy claiming no surface (keeping the
plane form) is held to it alike.

## Orientation (`mesh/orient.rs`)

Topology makes each connected shell consistently oriented, so what is
left is one sign per shell and how the shells nest. Since shells don't
meet (invariant 4), the winding number is 0 or 1 everywhere exactly when,
for every shell `S`, the other shells' winding number at a point of `S`
is 0 if `S` faces out and 1 if it faces in: every region of space
borders some shell, and near `S` its two sides have the others' winding
number and that plus `S`'s sign.

- **Shells** are the components of the triangles by halfedge pairs,
  numbered by their lowest triangle. A vertex has one fan, so shells share
  no vertex, and every patch of one is a non-neighbour of every patch of
  another: their hulls are more than the resolution apart.
- **Sign**: the shell's volume, from its first corner `o`. The corner
  triangles' volumes, each `det[a − o, b − o, c − o]` in floating point
  with Shewchuk's error bound for `orient3d` (`(7 + 56ε)ε` times the
  permanent), summed in triangle order carrying each addition's error
  (Ogita, Rump and Oishi's `Sum2`, whose own bound doesn't grow with the
  number of triangles). Then the
  difference each patch makes to its triangle's (`solid::patch_volume`,
  with an allowance for the quadrature of `1e-9` times the integral of
  the integrand's absolute value, which the same quadrature gives, so
  an integrand that cancels over the patch doesn't shrink it) is integrated for
  the patches that could move it most, in batches through `par_map` but
  added one by one, until the volume is further from zero than what the
  rest could still move it by (`lune_bound`) plus the rounding. An
  integrated patch's difference includes the cones from `o` over its
  three lunes (`lune_cones`): each edge's lune, the flat piece between
  the curve and its chord, has area vector `S(w)·(pᵢ₊₁ − pᵢ) × (cᵢ −
  pᵢ)/2`, `S(w)` the conic segment's share of its control triangle
  (`segment_share`: halving the curve gives `S(w) = w/(1 + w) +
  w/(1 + w)²·S(√((1 + w)/2))`, the weights going to 1 a quarter of the
  way each time, then `S(1 + ε) ≈ 2/3 + 4ε/15`; only `+ − × ÷ √`). So
  what it adds is the volume of the closed surface of the patch, its
  triangle turned over and its lunes, the same from any `o`. Without
  them an integrated patch beside one that isn't (a flat face's patches
  never are) left its lunes' cones in the sum, which from a far `o` can
  outweigh the whole volume: a thin disc bounded by quarter arcs on top
  of a long rod was refused, and turned inside out it passed. The bound:
  the patch, its triangle and the lunes between its curved edges and
  their chords (shared by the two patches beside an edge, turned opposite
  ways, so they cancel in a shell) lie in its control points' hull, which
  lies in the prism over the convex hull of the control points' shadows
  on the triangle's plane, as deep as they lie either side of it; twice
  that, for safety. Where the corner triangles' rounding is what leaves
  the sign open, their volume is worked out exactly once
  (`exact::sum_value`, expansions, in parallel chunks of 4 096
  triangles added in order: about 3.4 µs a triangle on one thread, a
  little more than the rest of `check`, and not charged; only near-flat
  shells, which invariant 4 all but rules out, need it). A shell still too close to zero to tell
  fails.
- **Nesting**: the other shells' winding number at `o`, taken only when
  another shell's box holds `o` (from a BVH over the shells' boxes; a
  shell's winding number is 0 outside its box), and then only over those
  shells: the signed number of their **corner triangles** a ray from `o`
  along `(2, 3, 32)` passes through, up to the top of their boxes (the
  patch BVH gives the triangles near the ray). That equals the curved
  surfaces' winding number: moving each patch straight onto its corner
  triangle, and each curved edge onto its chord (which both patches
  beside it do alike), keeps every point in the patch's control hull,
  more than the resolution from `o`, so the winding number round `o`
  doesn't change on the way. A triangle is crossed when
  `det[a − o, b − o, RAY]` and its two turns all have the sign of
  `D = det[a − o, b − o, c − o]`, and counts `sign(D)` (+1 leaving by its
  outer side). All are exact signs (`boolean::exact`) with `o` moved an
  infinitely small way along a fixed `NUDGE` and then `T2` and `T3`: with
  those three spanning space, an edge's sign is zero in every power only
  for an edge along the ray (the ray then runs alongside the triangle's
  plane, off it: no crossing) and `D` only for a triangle whose corners
  are on a line (which the ray misses). So there is no grazing case, no
  list of fallback directions and no refusal for want of a clean ray.

Cost (release, loaded machine, embedding means steps 1 to 4): the flat
torus of 262 144 triangles, one shell, about 25 ms against 270 ms for the
rest of `check` on seven threads (70 ms against 0.9 s on one), about 10%
and 7%; an extruded plate with 900 holes (40 140 patches, nothing to
integrate) 3 ms against 50 ms, about 5%. Cylinders need integrating
(their walls' bounds add up to more than their corner volume): six
patches each, some 20 µs a patch on one thread, so a thousand separate
small boxes and cylinders (14 000 patches) take 12 ms against 12 ms for
the rest on seven threads, and a plate with twenty voids (292 patches)
0.4 ms against 0.6 ms.

## Control hulls

A patch lies in the convex hull of its six control points (weights are
positive). Hull pairs come from a BVH over the patches' boxes with the
resolution as margin; two patches sharing a vertex share a control point, so
every neighbour pair is among them. Each pair is classified by the vertices
its triangles share, which is topology:

- **None: non-neighbours.** Their hulls are more than the resolution apart
  (GJK below).
- **Two: edge neighbours** (the topology check makes them share the edge
  between the two, run opposite ways). Take the shared edge's control points
  `P`, `C`, `Q` and each patch's other three control points (opposite
  corner, the two other edges' control points).
  - A **straight** edge (`C` within the resolution of the line `PQ`)
    leaves the plane free to turn about the line. Both sides must clear it
    by more than the resolution: project everything along the line; the
    best plane clears `a`'s points and `b`'s mirrored ones (`−x`) by the
    distance from the origin to their hull.
  - A **curved** edge fixes the plane through `P`, `C`, `Q`. One patch's
    points must be more than the resolution off it on one side, and the
    other's no more than the resolution past it on the other side, and
    the strict side must clear the lax side's highest point (or the
    plane, if higher) by more than the resolution too. A point of a patch
    is a Bernstein-weighted mean of its control points, so the strict
    patch meets the plane only along the shared edge. The lax side is
    what lets a flat cap meet a curved wall along a curved edge: the cap
    lies in the edge's plane. It may cross the plane by less than the
    resolution (what rounding of a cap built in the plane needs), and the
    two may then overlap by that much: below the resolution, where
    nothing is told apart anyway. That overlap isn't confined to the
    edge's neighbourhood: a strict side nearly parallel to the plane
    leaves a band under the resolution across the whole pair. The extra
    condition keeps the two sides' far control points more than the
    resolution apart, as for non-neighbours: a lax side leaning in by `c`
    needs the strict side `resolution + c` off the plane. Without it a
    lax side leaning in by 0.99 resolutions passed against a strict one
    1.01 off the plane, a fin a fiftieth of a resolution thick. Caps
    against walls, the pairs the lax side is for, lean in by at most a
    hundredth of a resolution (rounding), and their walls clear them by 4
    or more, as measured over the kernel's tests with the rule
    instrumented. There the extra condition changed 5 pair tests, all in
    the chains of twenty: a lax side leaning in by 0.6 to 0.8 resolutions
    against a strict one 1.1 to 1.6 off the plane, now split rather than
    passed; no result moved. On the seeded boolean suites and the
    random-plate stress it changed no result (measured with the flat fold
    stop in "Repair"). Random pairs (2M, heights within 4 resolutions,
    random layouts and weights) that the rule passes always have their
    far control points more than the resolution apart.
  - **The cylinder** (`hull::cylinder_apart`), for a curved edge the
    plane fails on, in `check` and repair (`check_pair`, through
    `edge_neighbours_parted`). Where the surface touches the edge's
    plane all along it (a ring at a turn of a solid of revolution: a
    torus's top, a flat face meeting a round tangentially, two arcs
    tangent at a top, a concave fillet onto a plate) both patches hold
    `P`, `C`, `Q` and lie on one side of every plane through them, but
    leaving the edge across it one goes in towards the conic's centre and
    the other out: they lie either side of the cylinder over the edge's
    conic, square to its plane. With `λP, λC, λQ` the barycentric
    coordinates of a point's projection onto the triangle `P, C, Q`
    (computed relative to `C` by cross products with `n = (P − C) × (Q −
    C)`), `F = λC² − 4w²·λP·λQ` is a quadratic function of space whose
    zero set is that cylinder (`ρ² − ρ0²` up to a factor for a circle; the
    form `Form`'s conic distance uses). On a patch `F = N/W²`, both of
    degree four; their Bernstein coefficients are sums over the 21 pairs
    of the six homogeneous control points of `F`'s polar form
    `λC(X)λC(Y) − 2w²(λP(X)λQ(Y) + λQ(X)λP(Y))` (and of the weights'
    product), times whole multinomial factors (1, 2, 4 or 8; the quartic
    multinomial is cancelled in the ratio), summed in a fixed order. With
    the shared edge as row 0 (the far corner's exponent 0), row 0 is the
    edge itself, where `F` vanishes: its coefficients are exactly zero (in
    floating point too, the edge's own coordinates being exactly `(1, 0,
    0)`, `(0, w, 0)` and `(0, 0, 1)`), so they aren't computed, and the
    rule asks instead that both patches hold the same edge, bit for bit,
    as a mesh's neighbours and refinement's pieces always do (edge
    records). It asks every ratio `N_γ/W²_γ` in rows 1 to 4 to be past the
    threshold plus its rounding bound, with one sign on one patch and the
    other on the other; anything not finite, a straight edge or a control
    triangle flatter than `|n|² ≤ 1e-24·|e1|²|e2|²` is refused. Then `F`,
    a positive mean of the ratios, has one sign on each patch but on the
    edge, so the two meet only there (the plane rule's argument one degree
    up). Splitting keeps it in exact arithmetic: a half of the edge lies
    on the same conic, so its `F` is the parent's times a positive
    constant, and a piece's ratios are weighted means of its parent's
    (blossoms). The computed bound, and off a circle the threshold, are a
    piece's own, so pieces are tested again like any other (measured: the
    refined profiles below pass). The **threshold** is `margin · 4w² /
    max(hP, hQ)`, `hP` and `hQ` the far ends' heights above the lines
    through the other end and `C`: `|∇F|` at the edge's end where it is
    smaller (`4w²/hQ` at `P`), which on a circle is `|∇F|` all along the
    edge; on other conics it may dip between the ends, so it is a scale,
    not a bound. So the margin bounds the coefficients of `F` against its
    gradient, about a resolution of distance from the cylinder at the
    edge, not the distance of control points from a plane as the plane
    rule's does. **Rounding** is bounded explicitly, not left to the
    threshold: on a flat control triangle (a short arc, or a long edge
    barely curved) the normal `n` is known only to about `ε·κ` of its
    length (`κ = |e1||e2|/|n| = 1/sin φ`, `φ` the angle at `C`), so a
    control point far off the edge's plane gets coordinates off by its
    height over the triangle's times that, and terms of `F` cancel. Each
    point's coordinates carry an error bound `64·ε·κ·(d·reach + m + ω)`
    (`d` its distance from `C`, `d·reach` bounding `|λP|, |λQ|`, `m` its
    largest `|λ|`, `ω` its weight), and each ratio's is `(1 + 4w²)·Σ
    k·(ex·my + mx·ey + ex·ey + 16·ε·mx·my) / W²_γ` (derivations at
    `LAMBDA_ROUNDING` and `CYLINDER_ROUNDING`). Without it, a long edge
    barely curved (5e-8 off a chord 147 long) in a tilted frame with two
    patches folded up out of its plane, both truly outside the cylinder,
    passed (test `rounding_does_not_make_up_a_cylinder`). Checked against
    exact rational arithmetic on f64 inputs (scratch, not a test): over
    flat triangles (arcs down to `1e-6` rad, long edges barely curved),
    weights at `W_MIN`/`W_MAX`, frames tilted and up to `1e5` out and far
    corners off the plane, the true error was at most 0.009 of the bound;
    on the profiles below every ratio that passes clears the threshold
    plus the bound at least 5 000 times over. Bands and caps
    (`Lathe::strip`, `pole_cap`) keep the plane rule alone: its failure on
    a fitted diagonal is what tells them a strip is too coarse. With the
    cylinder there too, a torus split 0.3 rad off its turns at `1e-1` took
    8 pieces round the axis instead of 16, and those strips failed the
    vertex rule (repaired 96 → 832 patches). Refinement's flat-face
    splitting doesn't use it either. Measured on random pairs sharing a
    random conic edge (200 000, 195 parted): `F` evaluated at 465
    points of each patch away from the edge always has the coefficients'
    sign (test). Across the kernel's other tests the rule passes 132 pairs
    in boolean repair (chains of twenty, related, nicks, turned solids),
    all certified by exact arithmetic too, with the same refusals as
    without it. Hunted further (scratch): 3.4 million adversarial pairs
    (circular arcs down to `1e-7` rad, edges bulging `1e-12` of their
    chord, any conic with weights at `W_MIN` and `W_MAX`, frames tilted
    and up to `1e4` out, far corners from `1e-14` of the edge's size off
    the cylinder, either side or both on one, and up to `1e2` off its
    plane, margins `1e-12` to `1e-5` of the size), 50 844 parted, every
    one certified by exact rational arithmetic, the true error at most
    0.012 of the bound; pieces of parted pairs split down their edge
    (`split4`, three levels) fail again now and then, with margins near
    the coefficients up to 1.5 % a level, other conics about three times
    as often as circles (whose threshold doesn't grow; under the plane
    rule too, pieces near the edge are closer to it): repair splits on; booleans of the profiles below with boxes and
    cylinders through their rounds match Pappus volumes of the clipped
    profile and the identities, deterministic at 1 and 8 threads.
  - **The pencil** (`hull::pencil_member`), for a curved edge both the
    plane and the cylinder fail on, in `check` and repair (third in
    `edge_neighbours_parted`). At a **crease** (two faces meeting at an
    angle along a curved edge, a revolved profile's corner) both faces
    may leave the edge on one side of its plane and on one side of the
    cylinder over its conic, or one along it: in the meridian plane,
    with the faces leaving along `(ρ, h)` directions, the plane fails
    when both `h` have one sign and the cylinder when both `ρ` do (or
    one is zero). So a corner whose two sides lie in one quadrant of the
    axes through it gets neither: a triangle's inner corners against a
    wall, two lines leaving a corner up and out, a D (an arc from the
    bottom of its circle out and up against a wall), a round just past
    its turn against a wall straight down, a lens's tips, a thin wedge,
    a nearly flat top against a wall. Every member `G = α·F + β·P` of
    the pencil of the cylinder's `F` and the plane's signed distance `P`
    vanishes on the edge and is quadratic in space (for a circle `G = 0`
    is a paraboloid of revolution through the ring), and near the edge,
    in the plane square to it, the mixtures turn the zero line through
    every direction: to first order some member parts any two faces
    leaving the edge in different directions. On a patch `G = (α·N_F +
    β·N_P)/W²`; `P·W` is quadratic with control values `ν = n·s/|n|` for
    a homogeneous control point `(s, ω)` (one more coordinate beside the
    cylinder's `λ`s), so `P·W²`'s polar form is `(ν(X)·ω(Y) +
    ν(Y)·ω(X))/2`, summed over the same pairs in the same pass
    (`CurvedEdge::coefficients`, shared with the cylinder rule, whose
    test is unchanged bit for bit: its threshold keeps the old order of
    operations, `margin·4·w²/h`, and its answers match the rule before
    the pencil on a million adversarial pairs). Row 0 is exactly zero for both (the
    edge's control points lie on the cylinder and in the plane), so the
    20 ratios `rF_γ`, `rP_γ` of rows 1 to 4 of both patches decide.
    **Search**: `F` scaled by `g` (the cylinder's `|∇F|`) so `F/g` and `P`
    have unit, mutually square gradients at the edge; a unit `u = (α',
    β)` parts them when `s_γ·(α'·rF_γ/g + β·rP_γ) > m_γ` for all 20 (`s =
    +1` on `a`, `−1` on `b`, `m_γ` the margin plus both rounding bounds),
    which is the distance from the origin to the hull of the points
    `s_γ·(rF_γ/g, rP_γ)/m_γ` exceeding 1 (the straight-edge plane test in
    two dimensions). The best `u` points at a vertex or a segment's foot,
    so the 20 vertex directions and the outer normals of the 190 segments
    are tried in a fixed order (210 directions against 20 points, no
    trigonometry; the other normal scores at most 0). It refuses at once
    if a point lies within 1 of the origin, drops a direction whose own
    point can't beat the best before normalising it and stops scoring one
    at its first point under the best, so a pair costs a few microseconds
    (on a loaded machine about two and a half times the cylinder's test,
    over 100 000 adversarial pairs); `edge_neighbours_parted` computes the
    coefficients once for the cylinder and the pencil (`CurvedPair`).
    **Check**: the chosen member is checked with its own bounds, every
    `s_γ·r_γ > margin + |α'|·eF_γ/g + |β|·eP_γ + 4ε·(|α'·rF_γ/g| +
    |β·rP_γ|)`, so nothing rests on the search. **Soundness**: the
    cylinder's argument for another quadric through the edge (`F` and `P`
    are the members `β = 0`, `α = 0`): `N_G = Σ B_γ·W²_γ·r_γ` with row 0
    zero is positive on `a` and negative on `b` but on the edge.
    **Splitting** keeps it in exact arithmetic: a piece of the edge has
    the parent's `F` times a positive constant and the parent's `P`, so
    the parent's member is in the piece's pencil and the piece's ratios
    are means of the parent's (under the margin they may fail, as the
    other rules' do). **Threshold**: `margin` times `|∇G|` at the edge
    (exactly so on a circle, a scale on other conics), not a distance of
    control points. **Rounding**: `ν` carries `NU_ROUNDING·κ·|s|`
    (`32·ε`; `n` tilts by about `7·u·κ` under rounding, as for the `λ`s),
    and `P·W²`'s ratios `Σ k·((eνX·ωY + eνY·ωX)/2 + 16·ε·(|νX|·ωY +
    |νY|·ωX))/W²_γ`. The bounds are load-bearing: with them zeroed, 64
    adversarial pairs (flat control triangles far out, weights at the
    limits) passed with members wrong in exact arithmetic; with them all
    are refused (test `rounding_does_not_make_up_a_pencil` keeps one).
    Bands and caps keep the plane rule alone, as for the cylinder.
    Measured: random pairs sharing a random conic edge, creases into one
    quadrant and anything (100 000, 8 527 parted, 5 741 by the pencil
    alone): `G` at 465 points of each away from the edge always has the
    promised signs (test; quick runs the first 20 000). Hunted (scratch): 2 million adversarial pairs
    in five layouts (the cylinder's hunt plus crease layouts `1e-9` rad
    to a quadrant apart, sometimes exactly in the plane or on the
    cylinder; tangent joins leaning anywhere; crease angles within
    `1e-12` to `1e-2` of 180°; one face nearly in the plane and the
    other nearly on the cylinder; edges just over the straight-edge
    threshold), 705 765 pencil passes, every one true in exact
    rational arithmetic (`|n|` bracketed to `2^-160`), the true error at
    most 0.037 of the bound, the exact `G` clearing the margin times
    `|∇G|` on every pair; either order of a pair gives the same answer
    but where the straight-edge test itself flips with the order (its
    threshold, as for the other rules). The review hunted 1.1 million more
    in five other layouts (patches up to `1e6` times the edge's size,
    the edge `1e-4` of it and `1e8` out; control triangles flat to `κ`
    about `1e11` with the margin just under the bulge; margins down to
    `1e-18` of the size; axis-aligned frames with corners exactly in the
    plane or on the cylinder; every far control point a hair, `1e-14` to
    `1e-6` of the size, off one member's zero set): 243 770 passes, all
    true in exact arithmetic, the error at most 0.036 of the bound.
    Pieces of 34 658 pencil-only
    pairs split down their edge (`split4`, three levels) fail about one
    in a thousand for other conics and one or two in 100 000 for
    circles; repair splits on. Revolved creases (eight crease profiles and a dovetail control,
    full and part turns, `Frame::Z` and frames out to `1.6e4`, fits
    `1e-1` to `1e-5`): all right, none refused, a few hundred patches
    (the triangle 304 at every fit, the wedge 912, the lenses up to 2 816
    at `1e-5`, 2 to 400 ms each) where repair made 14 192 to 229 232 or
    ran out of budget. Random revolves (`revolve/tests/random.rs`'s
    120): 116 right against 103 (the others `TooComplex`), 36 022
    patches against 410 224 over the cases both build, none more. Nearly
    flat tops and bottoms (slopes `1e-4` to `1e-3`) against walls, 60
    random profiles at `1e-4` and `1e-5` (scratch): all right, against 8
    (the rest `TooComplex` after 2 to 22 s). Over the kernel's tests the
    rule passes 13 567 distinct pairs, all true in exact arithmetic (the
    error at most 0.02 of the bound, the exact `G` clearing the margin 5
    times over); the booleans refuse the same operations with the same
    kinds (two refusals in a chain name a triangle 90 lower). End to end
    (scratch hunt): random crease profiles (triangles, spiked polygons,
    lenses of 1° to 80°, D shapes, sloped tops on and off the axis,
    rounds past their turns, rounded and chamfered rectangles, conic
    stars), scales `1e-2` to `1e2`, fits `1e-2` to `1e-5`, frames out to
    `5e5`, full and part turns: 1 400 revolves checked by `check`, face
    tags, Pappus and sampled points, none wrong; against the code
    before, on 709 of them, 660 right against 486 (169 `TooComplex`),
    1.1 million patches against 11 million, none more on any case; the
    same bits at 1 and 8 threads, and under every budget that passes. Booleans of
    those with boxes and drills through a crease, cylinders round the
    axis through its ring and other crease revolves (820 cases, all
    three operations, 120 of them at 1 and 8 threads too): every result
    checked with its tags, the volume identities and sampled points,
    none wrong; 210 000 distinct pencil passes in these revolves and
    booleans, all true in exact arithmetic (the error at most 0.037 of
    the bound). Over the cases both build, 31 operations
    work that were refused and 7 are refused that worked (small parts
    at coarse fits, and a cylinder brushing a ring, where the finer
    rings happened to pass). What repair is left there is not the edge
    rule:
    vertex neighbours at a full turn's stations (a cone strip's diagonal
    triangle a quarter turn wide shares only a station with the wall's,
    and no plane through it parts them though they are apart: repaired,
    the triangle's full turn to 304 patches; its part turn needs none,
    14), non-neighbours across thin
    tips, and on thin creases at fine fits edge pairs where the faces'
    curvature, not their direction, decides (the pencil plus `γ·P²`, a
    free curvature, would part those: kept for if fillets need it).
- **One: vertex neighbours.** A plane through the shared vertex `V` with
  the other five control points of each more than the resolution to either
  side. With unit normal `n` that is `n·x > margin` for every `x` in
  `{a_i − V} ∪ {V − b_i}`, and the best `n` clears them by the distance
  from the origin to their hull, so it is GJK again.
- **Three:** `SameCorners`, always a failure.

For flat triangles, which are their own hulls, the three rules say exactly
that the mesh is embedded (two flat triangles meeting only at a vertex
span pointed cones, which a plane through the vertex always splits).

**GJK** (`hull::apart(a, b, margin)`): on the Minkowski difference of the
two point sets, measured from `a[0]` so rounding is relative to the hulls'
size. Each step has `v`, the closest point of the current simplex (a point
of the difference, so `|v|` bounds the distance from above), and the
support point `w` furthest along `−v`, so `v·w / |v|` bounds it from below
for any `v`. It answers **apart** only once that lower bound exceeds the
margin, and **not apart** once `|v|` is within the margin, when the
simplex holds the origin, when a step makes no progress, or after 64
steps: conservative both ways it can be. The closest point of a simplex of
up to four points is found by trying every face (subset) of it: the
origin's projection onto the face's affine hull, solved from the Gram
matrix for a segment, from the cross product of the edges for a triangle
(`μ0 = −n·(x0 × e1)/n²`, `μ1 = −n·(e0 × x0)/n²` with `n = e0 × e1`, and
the point itself `n·(n·x0)/n²`) and by Cramer's rule on the edges for a
tetrahedron, kept when its barycentric coordinates are non-negative; the
nearest kept one is the closest point. Only faces holding the support
point just added are tried: it was added because it comes closer along
`−v`, so the new closest point is on a face with it, and the old faces are
no closer than `v`. Trying them too dropped a support point square to `v`
and far out (the top and a long side of a 2e-4 × 7951 × 2e-4 box): the
segment to it comes closer by less than the rounding of a squared length,
so the old point won, the next step found the same support point, and GJK
gave up. The Gram matrix squares the
conditioning: on a long thin triangle of the difference (the top and a
side of a 611 × 0.066 × 0.187 box) it lost eight digits, the point came
out off the closest, and GJK stopped short and called hulls 0.03 apart
not apart, in one order of the points and not another. Faces flatter than
`1e-12` (the sine between two edges, or a volume over its edges' lengths)
are skipped: their points lie within that of a smaller face. So flat and
collinear sets (coplanar cap triangles) need no special case. Ties keep the
smaller face, then the first.

## BVH

`Bvh::new(boxes)` builds sequentially: a leaf holds up to 4 boxes; an inner
node splits its boxes at the median of their centres along the longest axis
of the centres' box, with ties broken by index (`select_nth_unstable_by` on
a total key). `query(box, margin, out)` appends every box within `margin`
of the query along every axis (a box gap can only underestimate the true
distance, and rounding the sums is monotone, so nothing within the margin
is missed), sorted ascending. `self_pairs(margin)` queries every box
through `par_map` and returns the pairs `[i, j]`, `i < j`, sorted. Results
never depend on the tree's shape or the thread count.

Boxes that crowd each other (long thin triangles, or a profile's long
chords all crossing one spot) can make pairs of nearly every two, and
collecting them would run out of memory long before an operation's
budget. So operations with a budget take `pairs_within(ids, margin, keep,
work)` (and `self_pairs_within`): for a chunk of 256 ids at a time it
first counts the pairs `keep` takes (each query stopping once past what
`work` has left), spends them, and only then collects them, failing with
`TooComplex` as soon as they would be more than the work left. The
outcome depends only on how many pairs there are. Repair's pair search
and the extrude's separation use it; `check`, which has no budget, still
takes `self_pairs` (after a repair its pairs are ones repair counted).
`count_pairs_up_to(ids, margin, keep, most)` is the count alone, which
fails nothing: chunk by chunk, each query stopping past `most`, it
returns the number of pairs if at most `most`, else `most + 1`, the same
at any thread count (the counting pass above is the same code). The
extrude's crowding gate uses it.

## Refinement (`mesh/refine.rs`)

Red–green, exact, every split at `½`. The mesh under refinement is a set of
**leaves**: the input's patches and the pieces red splits (`split4`) made of
them, each with its level (red splits from the input). The leaves need not
be conforming: a leaf may have a neighbour one level finer across an edge,
whose split left a **hanging** midpoint on it. The mesh they make, the
**pieces**, is conforming: a leaf with no hanging vertex is one piece, and
one with a hanging vertex is two green pieces, `bisect_with` at it. These
rules keep that so:

- Before a leaf is split, every coarser neighbour is (levels across an edge
  differ by at most one, so a hanging vertex is one split deep).
- A leaf left with two or three hanging vertices is split too.
- A leaf on a plane face left with one, whose straight bisection would
  fold, is split too where it can be ("Flat faces" below).

Splitting a green piece splits its leaf, so green pieces are never bisected
again: every piece is a red descendant of an input patch, or half of one,
and pieces keep their shapes however deep they go (bisecting again and
again would make slivers, which fail the fold check).

Everything is keyed by vertex ids, never positions: edge records by their
ends (smaller id first), the midpoint of each split edge, the
whole edge each half came from, and which leaf has each directed edge as a
side, in `LookupMap`s, which are never iterated. An edge is split the
first time a leaf on either side splits, with
`Conic3::split_half` on its record, and both sides read the halves and
midpoint from the table. A red child's corners and boundary halves come
from the table and its inner edges from `split4`; in debug builds each
child is checked equal to `split4`'s. The output mesh is built with
`MeshBuilder` from the pieces, the input's vertices first, then the
midpoints in the order they were made.

**Flat faces.** A leaf on a `Plane` face is split with **straight inner
edges** (control point at the midpoint, weight 1), red and green. An exact
split of a flat patch with a curved side has curved inner edges lying in
the face's plane with both pieces, and the edge-neighbour plane rule
can't hold there: the plane through a curved edge's control points is
the face's plane, and neither piece is off it (the cylinder over the
edge, which `check` tries next, might part them; straight inner edges
came first and stay). Straight inner edges fall under the
straight-edge rule, and the pieces cover exactly the region the parent did
(it is the plane) as long as no straight inner edge crosses a curved side.
Where one does, the pieces cover it only up to sign: a child's corner at a
curved side's midpoint `M` lies between the curve's tangent there and a
straight edge to another side's midpoint, and if that midpoint is on the
wrong side of the tangent the corner is inside out and the child fails the
fold check. Every later piece keeping `M` has the same tangent and edge
direction there (red corner children keep both, a green bisector from `M`
lies inside the inverted angle), so splitting never mends it. Measured
(scratch sweeps on random planar patches, the straight split copied,
failing pieces followed 14 levels): a triangle with one curved side that
passes the fold check never folds this way, concave or convex, weights
0.05 to 20 (0 of 8 703, and about 35 000 on a grid); with two or three
curved sides 39 of 8 330 did for good with weights in `0.25..4` (1 of
10 227 with weights up to 1, none when every curved side was concave),
each already at the first split. See "Repair of a cap patch along a
concave curve" under Profiles. The boundary halves are the exact ones,
shared with the neighbours on other faces. A patch that is flat but
tagged `Free` gets the exact split and may then not pass.

That covered red children and a green bisector **from** a curved side's
midpoint. A green bisector from a curved side's **end**, to the middle
of the side opposite it, is a different case: where the curve bulges
into the leaf (a cap triangle beside a hole's rim) and that midpoint is
beyond the curve's tangent at the end, the straight edge leaves the
leaf, and the piece between it and the curve is inside out at the end
(repair then splits it to flat pieces that still overlap there and
refuses: `VertexNeighbours`). Measured on random cap triangles with one
circular arc side bulging in and passing the fold check (57 209): the
bisector from the arc's start folds in 12.2%, from its end in 11.4%,
from its middle never; by the arc's angle from 0.2% (under 10°) to
43.9% (80–90°). It needs the refinement to reach the cap triangle from
the far side, which is rare in the seeded suites (3 times in the whole
`boolean::` suite) and nearly certain for a cross hole near another
hole's mouth (a user's box drilled twice: three cap triangles in every
operation, the cut refused). So `split`, when a same-level neighbour
gets its first hanging vertex, also splits it red if it is a plane leaf
that passes the fold check, can be split (below `MAX_REFINE_DEPTH`, not
under the minimum size, with room for three more leaves), has a
straight green piece that fails the fold check or can't be built, and
has red children that all pass it (`forced_red`, a unit of work per
leaf that gets as far as the fold checks; a wrong plane tag isn't split
for, `pieces` names it). Red inner edges join midpoints, the case
measured safe above: with one curved side the straight red split of
every one of the 13 476 folding triangles of that sweep has no piece
failing the fold check, and the children are tested the same way when
they get a hanging vertex. With two or three curved sides a red
child's straight inner edge can cross one (above), and the split would
fold as the bisection does: of seeded plane triangles with two or three
conic sides (weights `0.25..4`) given a hanging vertex, the straight
bisector folds in 3 943 of 30 000 (the test's full run; quick runs the
first 3 000), and the red children fail too in 18
of those (about one in 220; none of 3 082 cap triangles with an arc
bulging in and a second arc). Such a leaf was split red at first all
the same, which only moved the refinement (11 times in the repair of
the ignored `a_hole_of_sharply_weighted_conics_at_every_tolerance`,
once in `a_cap_whose_straight_split_folds_extrudes`; the same errors
and results either way, and the folded red pieces were about as likely
to be mended by splitting further as the folded halves, about one in
six each). So the
children are tested first (`red_children`, built as `split_leaf` builds
them without making them), and such a leaf keeps the straight bisector,
as before the rule. A leaf that can't be split keeps it too and fails
in repair as before. Every leaf gets a hanging vertex
only there (a new leaf has none: its coarser neighbours were split
first), so the test sees each such leaf once, when it gets it, and
depends on that leaf and its records only; `split` is sequential, so the
order the stack takes them in, forced leaves behind their coarser
neighbours, is the same at any thread count. The unit of work is charged
before the leaf's own fold check, which is work as the pieces' are (in
the repair of `the_second_try_is_charged_only_from_where_it_resumes`,
whose totals the test moved by 8 units, measured, every leaf tested
passes it, so charging after would give the same totals). Room is
tested for the leaf's own three children only; what its split takes
with it (coarser neighbours first, neighbours left with two hanging
vertices) counts against `MAX_PATCHES` as any split's does, and so can
run out of room after the test, failing the round with `TooComplex`
where the bisection would have left it. That can't change an
operation's outcome: a round reaching `MAX_PATCHES` leaves has as many
pieces, and spending a unit a piece on them on top of the work before
overruns `MAX_WORK`, which is `MAX_PATCHES` and the most any budget
holds, so the round fails `TooComplex` either way. The same cascade can
reach a neighbour too small or too deep to split; none did in the
kernel's suites or the cross-hole fuzz below. A straight half
can't fail to build (its edges are records or straight between
vertices); if one did, it would count as folding, so the test never
fails where `pieces` wouldn't. The exact bisection
(`bisect_with`, its curved inner edge in the plane) was the other
option: it moves nothing elsewhere, but its halves meet across a curved
edge in one plane, which the edge-neighbour rules often can't part (216
of the 13 476 failed `EdgeNeighbours` at once), so repair had to split
them again. The seeded booleans' tallies didn't move; for the cross
holes see Known gaps under Booleans ("Cross holes through a box").

Repair's refiner applies the rule too, where its splits reach such a
leaf (in the kernel's suites 32 times: the revolve fuzz,
`a_cap_folding_when_refined_is_right_or_refused` and the sharply
weighted hole).
There it only brings the split forward: without it the folded half
fails the fold check and asks for its leaf's split a round later, the
same red split, and in those suites every repair came out the same,
triangle for triangle. Where the folded half also fails a pair rule
against its neighbours, they are split along with it: a drilled box
with a round post just in front of its cap
(`repair_splits_a_rim_triangle_red_beside_one_it_splits`) is repaired
into 118 triangles with the rule and 124 without.

The `Plane` tag isn't trusted for this. Before an input leaf (level 0) is
split or bisected as planar, its six control points are tested against the
plane with the resolution, the same test `check_faces` makes; one off it
fails with `Invalid(Face(t))`, `t` its input triangle, rather than be
reshaped (with the tag trusted, a cylinder wall tagged as a plane came out
of repair with pieces 7.6e-2 off the cylinder). Deeper leaves aren't tested
again: a straight split keeps the pieces' control points in the parent's
hull, and testing them could fail from rounding at exactly the resolution.
A patch within the resolution of its plane is still split straight, so its
pieces may be up to that far off it, as `check` allows.

A leaf is not split past `MAX_REFINE_DEPTH` levels, when its control
points span less than the refiner's minimum size along every axis, or when
there would be more than `MAX_PATCHES` leaves (every leaf is at least one
piece, so that round would fail anyway; stopping there bounds what one
round makes before it is counted): `TooComplex`.

## Repair (`mesh/repair.rs`)

`Mesh::repair(tol, budget)` restores invariants 3 and 4. The input must
pass the topology and shared-edge checks and have its patches within the
patch bounds (`KernelError::Invalid` otherwise); a mesh that already passes
comes back as it is. Each round tests the pieces that are new or changed
(a leaf that got a hanging vertex changes its pieces): the fold check on
each, and the hull rules on every pair with at least one of them, from a
BVH over all pieces queried from the changed ones (each pair once). A pair
of unchanged pieces passed in an earlier round, with the same corners and
so the same rule. What fails is split:

- A fold failure: its leaf. If a corner's normal coefficient is below the
  floor (`Patch::degenerate_corner`: its edges leave at 0° or 180°), no
  split mends it, as every piece keeping the corner has the same edge
  directions there, and repair fails at once with `Invalid(Fold(t))`, `t`
  the input triangle the piece came from. So does a whole leaf (not a
  green half) failing the fold check that is an affine triangle (`affine`:
  each edge's control point at its middle and its weight 1, within
  `1e-12` relative): its red pieces are similar to it and the check
  doesn't see scale, so they fail as it does, up to rounding, and of the
  leaves its splits make, those at the deepest level made in the middle
  of their parent have only their siblings as neighbours, so are whole
  pieces and fail at any depth. What fails there is a sliver whose
  normal coefficients `f64` can't tell apart (see "Slivers" below): a
  flat triangle 8e7 or 9e7 times longer than wide, its corners not yet
  degenerate and every pair passing, was split until its pieces failed
  the vertex rule (9 984 units, `Invalid(VertexNeighbours)`); it now
  fails in 8 units as `Fold`. Being flat within the resolution
  (`hull::flat`) isn't enough: a sliver a few resolutions wide whose
  control points sit off its edges' middles by a good part of its width
  (or whose edges are weighted) gets straighter for its size with each
  split, and of 65 889 random such slivers (64 to 400 resolutions long,
  0.5 to 6 wide) failing the fold check with no corner degenerate, 15 781
  had red pieces that all passed it, and 2 166 more after a second split.
  Nor is a green half enough: its leaf, of another shape, may pass where
  it fails, and so then do the leaf's red pieces. (Narrowed to affine
  whole leaves from any flat piece, the stop moved no outcome or error
  kind in the seeded boolean suites; one `Fold` names another triangle.)
  Measured with the curved-edge rule's extra condition (see "Control
  hulls"), release: on the seeded boolean suites no outcome moved between
  `Ok` and an error (related 112 of 120, chains 203 of 240, turned 156 of
  160, tangent 72 of 96, coaxial 37 of 40, bosses 64 of 64, drilled 160
  of 160), and only the related suite's errors changed kind (2 `Hull`, 1
  `VertexNeighbours` and 1 `EdgeNeighbours` became `Fold`, now 7 `Fold`
  and 1 `EdgeNeighbours`); 188 s of CPU against 187 s before. The random-plate
  stress (seeds 1 to 30, 600 cases each, at 0.1, 0.05 and 0.01) gave the
  same result for every case (`Ok` 8 596, 8 738 and 8 851 of 18 000).
- A failing pair: the leaves of both, except for **flat** pieces, whose
  edges are all straight within a threshold (`hull::flat`), so that each
  is its own hull up to it. Neighbours (the edge and vertex rules) take
  the resolution as the threshold: if both are flat, repair fails at once
  with `Invalid` and the pair's error, naming the input triangles the
  pieces came from, as the rules on flat triangles are exact and their
  pieces keep the angles at shared corners and edges. Splitting halves
  their clearances while it quarters a flat piece's overshoot, so going
  deeper gains nothing. Non-neighbours (the hull rule) take
  `FLAT_STOP` = 1/16 of it: a piece flat within the resolution of a
  curved surface still has a hull up to about half a resolution off the
  surface, so stopping there refused surfaces up to about 1.3
  resolutions apart that a couple more splits pass. Both flat within a
  sixteenth fails at once the same way; if one is, only the other is
  split, as splitting a flat piece brings no piece of it further away.
- A **witness** for a failing non-neighbour pair: points of the two
  surfaces closer than the resolution (`repair/witness.rs`). Then no
  split mends the pair and repair fails at once with its `Invalid(Hull)`.
  The argument: the pieces refinement makes of a leaf cover it (red and
  green splits are exact), so every later mesh has a piece holding the
  first point, inside the first piece's leaf and with the point in its
  hull, and likewise for the second. If the two leaves share no vertex
  (the pieces are conforming, so leaves that touch share a vertex of
  their pieces: the halves of a green leaf count all four of its
  vertices), those later pieces share none either, so they are
  non-neighbours whose hulls are closer than the resolution at any
  depth. A leaf on a `Plane` face is split with straight inner edges,
  which keep its boundary and their control points in the hull of its
  pieces. If its pieces face one way along the plane's normal (every
  normal coefficient leaning on it, or every one away, by more than the
  fold check's margin, both halves alike), seen along the normal they are
  one-to-one locally and the same way round, so each point inside the
  view of the boundary is covered as often as the boundary winds round
  it, by the later pieces too, which have the same boundary. So a point
  and the one over it in a later piece differ by at most that hull's
  thickness along the normal (`thickness`, the piece and its other half;
  infinite for pieces that don't face one way): it comes off the limit,
  as does rounding, `128·ε` times the largest coordinate of the two
  leaves' pieces (`witness::ROUNDING`; a few ulps a split over
  `MAX_REFINE_DEPTH` splits, evaluation, and GJK). A witness only ever
  adds an error where no `Ok` was possible; a search that misses only
  sends the pair back to splitting.
  The search (`surfaces_within`): the closest pair among the 15 face-tag
  samples of each piece, then damped Gauss–Newton (Levenberg–Marquardt)
  on the four barycentric coordinates, kept in the triangles, at most 30
  steps, each stretched by doubling up to 64 times while that comes
  closer (at a tangential touch the plain steps fall short by a steady
  factor, about a third). Pure `f64` with `+ − × ÷`, the same bits on
  any thread count and on wasm. It runs once a round for each pair of
  input triangles with failing non-neighbour pieces not both flat: on
  the pair whose corners' centroids are nearest (the first of equals),
  and only on pairs before the first failure that can't be mended
  (which names the error anyway). The choice is made and charged
  (`WITNESS_WORK` = 8 units a search) in a sequential pass. Searching
  every such pair instead would charge the 262 144-piece shell below
  110 504 searches, more than its budget has left (it uses 4 189 136
  units of 4 194 304): it would run out.
  Of the pairs, the first in pair order with a failure that can't be
  mended or a witness names the error.

The rounds end when nothing fails, or with `TooComplex` at
`MAX_REFINE_DEPTH`, past `MAX_PATCHES` or out of budget. A leaf to split
that is too small to fails the repair with `Invalid` of the failure that
asked for its split (the fold check's `Fold`, or the pair's `CheckError`;
of the first such leaf by id, and its first failure, folds then pairs in
order): splitting can't mend it at this tolerance, and a finer one, with
pieces larger in resolutions, may. Too small is under `MIN_SPLIT` (64)
resolutions across (the longest side of the leaf's control-point box, the
box the refiner measures) for a leaf flat within the resolution (`flat`),
and under `MIN_CURVED_SPLIT` (8) for one that isn't. Pieces a few
resolutions across can't keep the hull margin from their own neighbours,
and splitting them only makes more that fail (two tetrahedra touching
corner to corner went from 6 failing pieces a round to thousands at about
8 resolutions); a surface that keeps clear of itself passes long before,
since a patch of size `s` on a curve of radius `R` sags by about `s²/8R`.
A flat piece's failure doesn't shrink as it is split, but a curved
piece's sag does, and it is flat once `s` is under about `√(8R)`
resolutions: on surfaces of radius under about 500 resolutions curved
pieces reach 64 resolutions still curved, and small round surfaces a few
resolutions apart need smaller pieces to pass. So only curved pieces go
below `MIN_SPLIT`, a bounded number of levels (three at most), ending by
flatness or the lower floor. The stop is decided in `failures` after all
its other `Invalid` returns (a degenerate corner, flat pairs, a witness
take precedence), and only for leaves it asks to split; the refiner's own
floor, `MIN_CURVED_SPLIT`, stays a `TooComplex` backstop for leaves its
conformity rules split with them. Measured (release, coarsest tolerance,
resolution `m`): two cylinders of radius 20 to 200 `m`, 1.5 to 3 `m`
apart, which gave `TooComplex`, repair in 1 000 to 11 000 units (68 to
520 patches, 4 to 10 ms); of radius 5 or 10 `m`, 1.5 `m` apart,
`Invalid(Hull)` in 570 to 1 600 units, where a tenth of the fit
repairs them; a bulging tetrahedron a resolution across fails the fold
check as `Invalid(Fold)`, and 32 resolutions across it is split and
passes. On the seeded boolean suites no outcome or error kind moved
(related 112 of 120, chains 203 of 240, turned 156 of 160, tangent 72
of 96, coaxial 37 of 40, bosses and drilled all, before and after;
69 s of CPU either way). In
the random-plate stress (seeds 1 to 30, 600 cases each, every case
extruded at one fit): at 0.1, 9 `TooComplex` became `Invalid` (100 →
109) and at 0.05, 6 (58 → 64), the `Ok` counts unchanged (8 596 and
8 738 of 18 000) and nothing at 0.01; each of the 11 cases extrudes at
0.01 or 1e-3. Touching flat faces fail at
once: two boxes face to face, or two cylinders side by side or end to end,
used to be split to the end of the budget (12 to 27 s) and now fail in 2
to 80 ms. Round surfaces that touch fail on a witness, mostly in the first
round (measured, release, default tolerance, resolution `m`: two unit
cylinders side by side at 30° or 17°, 0.5 to 0.99 `m` apart, 570 units of
work, where splitting until flat took 672k to 764k; a unit cylinder with a
box corner 0.5 to 0.99 `m` off its wall, 370 against 67k; unit round
octahedra corner to corner 232 against 6k; small ones at the coarsest
tolerance 232 against `TooComplex` at `MIN_SPLIT`). A shell thinner than
the resolution (`shell(10, 0.5 m)` to `0.99 m`) fails after 600 units in
a few ms, where it ran out the whole budget (`TooComplex`, 1 to 4 s), and
a blind void with a wall half a resolution thick cut by a boolean in 2 to
15 ms, where that took about a second. Surfaces just over the resolution
apart pass from about 1.03 `m` (the box at 1.01 `m`; the cylinders at
1.05 `m`), where they were refused up to 1.1 to 1.25 `m`; such pairs split
deeper (the cylinders at 1.05 `m` take 1.2M to 2.7M units, 0.4 to 1.5 s,
and at 1.01 `m` 2.7M to 3.1M before `Invalid`). Valid shells a few
resolutions thick still need more pieces than the budget allows
(`TooComplex`), as before. On the seeded boolean suites the witness and
`FLAT_STOP` changed no outcome between `Ok` and an error (related 112 of
120, chains 200 of 240, turned 156 of 160, tangent 72 of 96, coaxial 37
of 40, bosses 64 of 64, drilled 160 of 160, before and after); only the
errors' kinds moved: coaxial 2 `EdgeNeighbours` and chains 1 `TooComplex`,
2 `EdgeNeighbours` and 2 `VertexNeighbours` became `Invalid(Hull)`, and
the suites took 61.5 s of CPU against 69.4 s.
Each stop was then tested against the same repair with it switched off
(release; 5 900 random meshes: two cylinders, cylinder and box, two
round octahedra turned at random, thin shells, a lying cylinder over a
planar cap with arc edges, a cylinder's cap facing a wall, an octahedron
over a cap, a box tilted over a cap; radii 5 to 10⁶ resolutions, gaps
0.1 to 50, at both fit limits, near the origin and 9e5 out, turned at
random, the second body tilted by 1e-7 to 1e-2): no witness, no affine
fold stop and no curved-edge condition refused a mesh that repairs
without it, and no `Ok` failed `check`; nor did the affine stop on
12 000 random slivers (aspects 1e5 to 3e8, some 9e5 out).
What did refuse such meshes: `FLAT_STOP`, as above, near the origin up
to about 1.02 resolutions, and at the finest tolerance 9e5 out, where no
witness can be claimed, up to about 1.05 (a ulp there is about a
hundredth of a resolution); and the floors: 84 of 1 641 failing cases of
radius 5 to 1 000 resolutions (and 2 of 10⁴, 1.02 apart) repair with
floors of 2 and 16 instead.
Splitting only a pair's leaf that can be split, rather than failing,
would mend 6 of them, so it isn't done. Where both fail, the curved
floor costs up to 4 times the work a floor of 64 does (330k units for
thin shells of 500 resolutions 9e5 out at the finest tolerance; under
25k near the origin), every case under 0.3 s. Results and work are the
same on 1 and 8 threads (383 of the meshes, and the seeded suites).
In debug builds the result is checked in full, face tags aside: repair
refuses a wrong `Plane` tag on a patch it splits (see "Refinement"), but
other tags, and those of patches it doesn't split, are the input's claims,
which it neither checks nor promises. `Solid::new` checks them.

Work, from the budget: a unit per piece tested for folds, per pair tested,
per leaf split, the number of pieces each round (the BVH and the pieces),
and `WITNESS_WORK` per witness search. All is counted in sequential
passes, so running out doesn't depend on the thread count. The fold and
pair tests and the witness searches run through `par_map` over sorted
lists; the splits, the pieces and the BVH are sequential.

Repair works on the mesh by reference (`Mesh::repaired`: the repaired
mesh, or `None` if it passes as it is; `repair_within` is it taking the
mesh), so a caller can keep what it gave: `Solid::finished_or_unfinished`
does, for the boolean's pinch test and for evidence. A refusal
(`Refusal`) is the error with the pieces it names, one or two, as they
were when it failed: the piece failing the fold check, or the pair
failing the hull or neighbour rules (no split mends them, the witness
found their surfaces within the margin, or a leaf too small to split
was asked for by them). The error still names their input triangles;
the pieces lie within those (a piece's hull lies in its triangle's),
and where repair split first they are smaller, which is what the
evidence shows (see "Check evidence" under "Limits, budgets and
errors"). None for errors naming no piece (the input's topology or
bounds, a wrong `Plane` tag, running out). The same result, error and
work.

`Mesh::repaired_from(tested, …)` is `repaired` whose first round tests
only the triangles `tested` marks, and the pairs with one of them, for a
mesh whose other triangles and pairs are known to pass the fold and
hull rules (a boolean's result, see "Repair and the check near the
change"); `None` marks all. That first round's pairs are put in the
order testing every piece gives them (`[p, q]`, `p < q`, sorted: what
`pairs_within` gives when every piece changed), so the failures it
finds, the leaves it splits, the witness it looks for and the error it
names are the whole first round's, and so are the later rounds, which
start from the same changed pieces: the same result and error, for
work near the change only (one unit a patch for the patches stays). A
repaired mesh comes with the triangle of the input each of its
triangles came from (`Piece::origin`).

## Boxes and cylinders (`mesh/primitive.rs`)

Built as an extrude builds its solids, and checked (`check` with the given
tolerance, always) before they are returned:

- `Mesh::cuboid(min, size, feature, tol)`: the box's bottom rectangle
  extruded along `+z`, two flat triangles to a side: 8 vertices, 12
  patches. The bottom is the `StartCap` and the top the `EndCap` of
  `feature`, the sides `Side { curve: 0..4, segment: 0 }` round the
  rectangle counter-clockwise seen from `+z`, from the one along `−y`. All
  plane faces.
- `Mesh::cylinder(base, radius, height, feature, tol)`: a circle of four
  exact quarter arcs (control point where the tangents meet, weight `√½`)
  extruded along `+z`; each wall a `cylinder_strip`, each cap four quarter
  discs around its centre: 10 vertices, 16 patches. Caps as for the box,
  walls `Side { curve: 0, segment: 0..4 }` counter-clockwise from `+x`,
  tagged with the cylinder.

Forms: the tags' planes, and the cylinder's walls `Form::Cylinder` along
`+z`.

Parameters are refused with `KernelError::Patch` (every point within
`MAX_COORD` of the origin and finite, sizes above zero), and a solid the
tolerance can't hold (a box thinner than the resolution) with `Invalid`.

**Slivers.** A flat triangle about `3e7` times longer than it is wide is
where `check` stops working, as `f64` runs out at about `1/√ε`. It shows
first in the vertex rule: GJK there works on points up to the triangle's
length from the shared vertex, and runs out of digits on hulls that long
and that close (the closest point of a simplex rounds relative to its far
points, and turning `v` by that much moves `v·w` by more than the margin),
failing `VertexNeighbours` from about `3e7` to `7e7`. From about `7.6e7` the
fold check fails too (a corner's normal coefficient, about the sine of
its angle, under the floor relative to its terms), and from about `1e8`
`degenerate_corner` names the corner; repair refuses such an affine
triangle failing the fold check at once either way. A box a hundred resolutions thick
passes up to `2e7` times as long. Only features of a few nm on a 100 mm
part reach this (a sliver also needs to be a couple of resolutions wide
to pass the hull rules at all), and the error is immediate. Caps and
walls should be triangulated well within that.

## Costs

`check` on a flat torus of 262 144 triangles takes about 0.7 s on one
thread and 0.27 s on eight (release; the topology pass is sequential);
about 2.7 µs per patch, before the orientation (above) added about 10%.

Repair of the thin shell in the tests (radius 10, both sides split evenly;
release, seven threads, one in brackets): 0.2 thick, 1 024 patches in 4 ms
(7 ms); 0.01 thick, 16 384 in 52 ms (111 ms); 0.001 thick, 262 144 in
0.88 s (1.9 s), about 16 units of work per patch. The refinement, the
pieces and the BVH are sequential, which is why seven threads give about
2×: of the 0.88 s, splitting takes 0.35 s, and rebuilding the mesh 0.2 s.
The refiner's and the builder's maps are hashed (`LookupMap`); as
`BTreeMap`s they made repair half as slow again. That shell's repair
looks for a witness 224 times over its rounds (one pair per pair of input
triangles a round), 1 792 units of work; its instruction count (callgrind,
one thread, both builds with one codegen unit) rose 0.31% with the
witness, and a passing repair of a 262 144-patch flat torus 0.39% (with
the default codegen units both read about 1.4%, layout alone). Repair of
that shell uses 4 190 928 units of the 4 194 304 budget: valid shells
much finer than it run out.

## Tests

Hand-built solids pass `check` and `check_faces`, at the origin and moved
by about `3e5`: a tetrahedron, a box (`Mesh::cuboid`), a flat octahedron, a
round octahedron (every edge a quarter circle, weight `√½`), a half
cylinder (exact quarter-circle walls from `cylinder_strip`, quarter-disc
caps with curved edges, a flat side; plane and cylinder tags), and a flat
torus of 2 304 triangles. One bad mesh per rule is caught with the expected
error: indices and counts, unpaired and self-paired halfedges, a loop,
repeated directed edges, two fans at a vertex and an unused vertex,
halfedges naming different edges and an edge used twice over, bad weights
and coordinates, a zero corner normal, overlapping tetrahedra (and two
corner to corner, passing at twice the resolution apart and failing at
half), edge neighbours folded flat onto each other across a straight
edge and across one curved up out of their plane (both inside the
cylinder over it), passing across a sideways curved edge (parted by the
cylinder) and one curving outwards (by the plane), crossing vertex
neighbours, triangles on the same corners, wrong plane and cylinder
tags, and a plane tag or plane form on its face but facing in
(`FacesAgainst`, also through `Solid::new`, and for a form on a face
claiming no surface; a NaN normal is against); the orientation fixtures turn plane tags with the
shells they turn into voids. The cylinder rule (`mesh/hull/tests.rs`) parts fitted strips at
rings at turns (a torus's top, round into flat, flat into a concave
fillet, an S) where the plane can't, both orders, and refuses them once
one patch's edge is off by `1e-3` of its weight or a bit of its weight
or control point; it refuses two rounds folded out of a top, a wall
lying on the cylinder under a round (which the plane parts), a straight
edge, and a long edge barely curved in a tilted frame whose patches
rounding showed on opposite sides of the cylinder, and is sound on
200 000 random pairs (`F` at points against the coefficients' signs). GJK is tested against boxes a known gap apart (face to face and
corner to corner, randomly rotated and moved), point clouds either side of
a plane, flat, collinear and repeated points, the long thin hulls of a
611 × 0.066 × 0.187 box's corner in any order and rotation, and a support
point square to the closest point from every starting point; the BVH
against brute force. Plane tags with normals from `1e-200` to `1e300` long
measure the same, and a quadric whose gradient overflows fails. A mesh breaking two rules gives the earlier invariant's error
(bounds before a fold, hulls before a face tag).
Orientation (`mesh/orient/tests.rs`): a box and a cylinder turned inside
out; a 10 mm cube with a second shell turned in far off (box, cylinder)
or beside it, facing out inside it (clear of its walls, anywhere, a
cylinder), and a void inside a void, each refused naming the bad
shell's first triangle; a void, a second body, a void with an island,
a box less a cylinder inside it, a pin joined into a void, a ring with
a disc in its hole, crossing cylinders and a drilled plate pass, and
turned over are refused; rays leaving along the edge between two
triangles, through a box's corner and from the plane of a face (a box
in an L's notch) are decided and right; a ring a twentieth thick is
told by integrating a few patches; a tetrahedron whose corner volume
floating point can't tell is worked out exactly; a plate with twenty
voids cut by booleans passes and refuses one void turned over; the
float bound on corner volumes never contradicts the exact sign; the
lune bound holds on curved patches and on random ones over the whole
weight range (185 000 came to at most a third of it), and what an integrated patch adds
is the same measured from anywhere; the segment shares match the
circular segments' areas and Simpson's rule; a thin disc bounded by
quarter arcs on a long rod (its first corner at the rod's foot) passes
and turned over is refused, as do a hundred of random proportions and
forty random thin rings (a third of each in quick runs); 300 random
nestings (100 quick) of boxes and cylinders up
to four deep, on a grid that sends the rays through edges and corners,
some shells turned wrong, match the nesting they were built with (the
first wrong shell named, a right mesh's volume the shells' signed
sum); slabs as thin as the fold rule allows, tilted and far out at the
finest tolerance, pass with a void and refuse it turned; and the
result and the count are the same at 1 and 8 threads. A mesh whose only fault
is a wrong `Plane` or `Quadric` tag is refused by `Solid::new` (before, in
release it passed).
`check`, the BVH's pairs, and the first failure of a jittered torus are the
same at 1 and 8 threads.

Refinement: a red split bisects its three neighbours (the red children are
`split4`'s, the green ones `bisect`'s); splitting a green piece splits its
leaf; splitting at a corner again and again stays graded (a bounded number
of new pieces per level, and the mesh stays closed); a flat cap's pieces
have straight inner edges and pass `check`; a face tagged as a plane only
one of its triangles is on fails at the first neighbour bisected or split
straight, naming it; too deep and too small leaves aren't split, nor any
past the most leaves there may be. Repair: a thin shell (two round octahedra 0.2 and 0.05
apart, the inner facing in) is split evenly until it passes; a cylinder
with a box beside it, 0.1 to 0.001 off the wall, is split only near the
box, keeps every piece on its cylinder or plane within `1e-12` and the box
whole; a tetrahedron bulging so far its patches fail the fold check passes
at 16 patches; a mesh that passes comes back unchanged; a cylinder wall tagged
as a plane fails with `Invalid(Face(t))` naming a wall triangle, and caps
tagged half a resolution off their planes are still split straight and
pass `check`, where one and a half resolutions off fails naming a cap; a
wall tagged as a wrong cylinder comes through repair (quadric patches are
split exactly, trusting nothing) and `Solid::new` refuses it, the same at
1 and 8 threads; a cusp fails at
once, as do tetrahedra corner to corner and boxes face to face closer than
the resolution; round octahedra touching, small ones too, cylinders side
by side at 30° and 17° within the resolution and a cylinder with a box
corner 0.9 resolutions off fail with `Invalid(Hull)` in under 1 000 units
of work, as does a shell thinner than the resolution (also moved by
`1e5`), where small cylinders 1.5 resolutions apart still reach
`MIN_SPLIT`; the box at 1.05 and the cylinders at 1.1 resolutions pass
and check (the cylinders, over a second, under `VARDE_TESTS=full` only); a small budget runs out, and bad topology and weights are
refused; the shell's and the cylinder's repairs, and failures on a
witness with their work, are the same at 1 and 8 threads. A witness
needs the pieces' leaves apart and comes off by a planar leaf's
thickness (and none for a plane that isn't one). The witness search
(`repair/witness/tests.rs`): flat triangles half a resolution apart are
found and 1.01 or 1.5 apart not, at the origin and far from it; the
cylinders' touch between the samples is found by the steps (the closest
samples are a thousand resolutions apart), and not at 1.01 or 1.5
resolutions; round octahedra corner to corner and edge to edge, turned
and moved within `3e5` (`3e4` at the finest tolerance) and of radius
`1e-2..1e2`, are found at half the limit repair gives and never at 1.01
resolutions; degenerate patches (a point, a line) and limits that are
zero, negative, NaN or infinite give no NaN; the search stays in the
triangles. A boolean leaving a blind void with a wall half a resolution
thick (`boolean/tests.rs`) fails with `Invalid(Hull)` within a budget of
100 000.
Constructors: 50 random boxes and cylinders (sizes `1e-2..1e3`, moved up to
`1e5`) pass `check` and lie on their faces within `1e-12`; faces are named
as an extrude names them; bad parameters are refused, and a box thinner
than the resolution fails `check`; boxes a hundred resolutions thick and up
to `2e7` times as long pass at every tolerance.
