# Kernel

`varde-kernel` (`crates/kernel`) is heading for solids that are closed,
oriented 2-manifolds of **rational quadratic triangle patches**, with
union, difference and intersection built the way
[Manifold](https://github.com/elalish/manifold) builds them for flat
triangles: every topological fact derived by counting from primitives that
are each computed once, so the result is always a valid manifold even when
a primitive is numerically off. Planes, circles, arcs and cylinders are
exact in this representation; everything is `f64`, and only drawing
goes to `f32` (`RenderMesh`).

What exists today: the patch math (`patch`), closed meshes of patches with
the check of their invariants, the BVH and the hull tests, red–green
refinement and repair, and box and cylinder meshes (`mesh`), the
tolerances (`Tolerance`), the limits, `Budget` and `KernelError`, and the
parallel map (`par`), below. Solids are still the analytic cuboid of
`Shape`, tessellated directly; `Solid` comes to wrap a checked `Mesh` when
tessellation lands.

## Patch math (`src/patch.rs`, `src/patch/`)

Pure math on one curve or one triangle, no mesh:

| file | holds |
|---|---|
| `patch.rs` | module docs, the limits, `Bounds<P>`, `PatchError` |
| `patch/conic.rs` | `Conic<P>` (`Conic2`, `Conic3`), the `Point` trait over `DVec2`/`DVec3`, exact arcs |
| `patch/triangle.rs` | `Patch`: net, blossom, evaluation, derivatives, sub-patches, splits |
| `patch/fold.rs` | the normal's Bernstein coefficients, the fold check, `NormalCone` |
| `patch/strip.rs` | `cylinder_strip`: exact cylinder patches |
| `patch/tests.rs` | property tests; `src/test_rng.rs` is their seeded generator |

### Curves

A `Conic<P>` runs from `p0` to `p1`, pulled towards the control point `c`
by the weight `w`:

```text
        (1-t)²·p0 + 2t(1-t)·w·c + t²·p1
C(t) = ─────────────────────────────────
         (1-t)² + 2t(1-t)·w + t²
```

This is the **standard form**: both end weights are 1. Homogeneously the
control points are `(p0, 1)`, `(w·c, w)`, `(p1, 1)` (a point and its weight
in one vector one coordinate longer, `P::Hom`), where the curve is a
polynomial and de Casteljau and blossoming are exact. The blossom is
`B(s, t) = (1-s)(1-t)·h0 + ((1-s)t + s(1-t))·h1 + st·h2`.

- A straight segment is `c` at the midpoint with weight 1 (`Conic::line`).
- An exact circular arc of angle `θ ≤ 90°` has its ends on the circle, `c`
  where the end tangents meet (at `r / cos(θ/2)` from the centre, on the
  bisector) and weight `cos(θ/2)` (`Conic2::arc`, `Conic3::arc`, which in
  3D takes the circle's plane as two orthonormal axes; other axes give the
  matching ellipse arc).
- Weights below 1 give ellipse arcs, 1 parabolas, above 1 hyperbolas.
  Reversing a curve keeps `c` and `w`, so an edge record needs no
  direction.

**Back to the standard form.** A homogeneous curve with end weights `wa`,
`wb` becomes standard by scaling each barycentric coordinate by `1/√w`:
the points stay, and the middle weight `w` becomes `w / (√wa·√wb)`. That
depends only on the edge's own three homogeneous points and is symmetric
in the ends, so both sides of an edge get the same bits. The roots are
taken before multiplying, so homogeneous points of any scale work (the
product of the end weights alone could overflow or go subnormal).

**Splitting** at `t` runs de Casteljau on the homogeneous points and
renormalizes each half. The halves' weights lie between the parent's and 1
(`w' = sqrt((1+w)/2)` at `½`), so they stay within bounds. At `½` the
computation is written symmetrically (`(h0+h1)/2`, `(h1+h2)/2`, their
mean): the reversed curve gives the same halves reversed and swapped, to
the bit, and `split(0.5)` is `split_half`. At other positions `1 - t`
rounds, so **a shared edge is split once and the halves are handed to both
sides** (`Patch::bisect_with`).

A piece's own standard parameter is a *projective* reparametrization of
the parent's: the piece between parent parameters `t0` and `t1` has at `s`
the parent point at the barycentric `(1-s)·(1-t0, t0)/√D(t0) + s·(1-t1,
t1)/√D(t1)`, `D` the parent's denominator. Tests compare points that way.

### Triangles

A `Patch` has corners `p[0..3]`, and edge `i` runs from corner `i` to
corner `i + 1` with control point `c[i]` and weight `w[i]` (edges 01, 12,
20). Corners run counter-clockwise seen from the side the normal points
to. Positions are barycentric `u = (u0, u1, u2)`, `u0 = 1` at corner 0.

The homogeneous **net** is symmetric: `H[i][i] = (p_i, 1)` and `H[i][j] =
H[j][i] = (w_ij·c_ij, w_ij)`. Then

```text
X(u) = Σ_ij ui·uj·H[i][j]          P(u) = X.xyz / X.w       D(u) = X.w
B(a, b) = Σ_ij ai·bj·H[i][j]       (the blossom; B(u, u) = X(u))
```

Along an edge (`uk = 0`) this is exactly that edge's curve, so two patches
sharing a vertex pair, control point and weight share the curve.

**Derivatives.** With `G_i(u) = Σ_j uj·H[i][j]` (half of `∂X/∂ui`), Euler
gives `X = Σ ui·G_i`. Along `u0` and `u1` with `u2 = 1 - u0 - u1`:
`X_u = 2(G0 - G2)`, `X_v = 2(G1 - G2)`, and `P_u = (X_u.xyz - P·X_u.w) /
X.w`, likewise `P_v` (`eval_derivs`). `P_u × P_v` is the normal.

**Sub-patches.** Over a domain triangle `d0, d1, d2` the homogeneous
corners are `B(di, di)` and the edges `B(di, dj)`, exact; renormalizing
(corner weight `Wi → 1`, edge weight `w → w / √(Wa·Wb)`) gives the
standard form (`sub`, `from_hom`). The child at its own `b` is the parent at
`Σ bi·di / √Wi` (projective again). A clockwise domain gives the reversed
patch.

**Splits.**

- `split4`: splits each edge at `½` with `Conic::split_half` and joins the
  midpoints. Children follow `Patch::SPLIT4_DOMAINS`: the corner triangles
  `(p0, m01, m20)`, `(m01, p1, m12)`, `(m20, m12, p2)`, then the middle
  `(m01, m12, m20)`, all counter-clockwise. The middle edges come from the
  blossom at the midpoints, normalized with `D` there, and each is one
  value shared by the two children on it.
- `bisect(i, t)`: splits edge `i` at `t` and joins the split point `m` to
  the opposite corner. With `a, b, o` corners `i, i+1, i+2`, the children
  are `(a, m, o)` and `(m, b, o)`. At `t = ½` a neighbour bisecting or
  4-splitting its side of the edge gets the same halves and `m`, to the
  bit (red–green refinement relies on this).
- `bisect_with(i, t, halves)` takes the halves of a shared edge split
  once at `t` (the neighbour passes them reversed, at `1 - t`). They must
  run from corner `i` through one shared point to corner `i + 1`, or
  they're refused (`Mismatch`); that they were split at `t` is the
  caller's promise. `split4` needs no such variant: it only splits at `½`,
  where both sides get the same bits anyway.
- The middle edges of a split can leave `[W_MIN, W_MAX]` for extreme
  parents (two edges bulging at `W_MAX`, the third at `W_MIN`); the split
  is then refused with `Weight`. As pieces shrink, their weights go to 1.

`hull()` is the six control points: with positive weights the patch lies
in their convex hull. `bounds()` is the box around them.

### The normal, the fold check and the normal cone

`X` is a quadratic form and each `G_i` is linear, so

```text
P_u × P_v = n(u) / D(u)³,      n(u) = 4·cross4(G0, G1, G2)
cross4(a, b, c) = a.w·(b×c) − b.w·(a×c) + c.w·(a×b)     (spatial part)
```

(`cross4` is the normal of the plane through the three points the
homogeneous vectors stand for; it is alternating and multilinear, and
`cross4(X, X_u, X_v) = ½·cross4(G0,G1,G2)·(u0+u1+u2)` by Euler.) So the
normal's numerator is a **cubic** with ten Bernstein coefficients
(`normal_coeffs`), ordered `(3,0,0), (2,1,0), (2,0,1), (1,2,0), (1,1,1),
(1,0,2), (0,3,0), (0,2,1), (0,1,2), (0,0,3)` (powers of `u0, u1, u2`),
with `n(u) = Σ 3!/(i!j!k!)·u0^i·u1^j·u2^k·c`. The corner coefficient is
the cross product of the two edge tangents leaving the corner,
`2w01(c01 − p0) × 2w20(c20 − p0)`. Each term `cross4(x, y, z)` of three
net entries is computed as `wx·wy·wz·(y − x) × (z − x)` of their points,
so its rounding is relative to the distances between those three points,
not to their distance from anything else. (Computed from the net moved to
`p0`, a pinched patch whose heavy control points lie close together but
far from `p0` got rounding bounds far too big, and failed the floor below
although its parent passed.)

**The fold check** (`fold_direction`) looks for a unit `d` with
`c·d > FOLD_MARGIN·|c|` for every coefficient. Every normal is a positive
combination of them, so with such a `d` the normal never vanishes or flips:
no fold. It tries the flat triangle's normal, then the mean of the
normalized coefficients, then the exact answer: the axis of the smallest
cone around the normalized coefficients. That cone is fixed by at most
three of them on its rim, so every single direction, pair bisector and
triple circumcentre on the sphere is tried (`smallest_cone`; ties keep the
first, so the result depends only on input order). A coefficient whose
length is at most `FOLD_FLOOR` times the summed size of the terms it came
from counts as zero and fails the check: a corner whose edges leave at 0°
or 180°, which no amount of splitting fixes (`degenerate_corner` names such
a corner, so repair can give up at once). A coefficient's rounding is
at most `16·ε` of that size (`ROUNDING`), so one past the floor points
within `ROUNDING / FOLD_FLOOR` (about `3.6e-7`) radians of its true
direction, less than the margin: a patch that passes doesn't fold (a
compile-time assertion keeps the constants that way).

It is **preserved by splitting**: a sub-patch's cubic is a positive
constant times the parent's cubic at projectively mapped points, so its
Bernstein coefficients are positive multiples of blossoms of the parent's
cubic at points inside the domain, which are convex combinations of the
parent's coefficients. The cone `{c : c·d > ε|c|}` is convex, so the
parent's `d` passes for every child (tested for 4-way splits, bisections
and random sub-triangles). Invertible affine maps keep it too (normals
transform by the cofactor matrix). In floating point it holds only while
the pieces stay reasonably shaped: a sliver about `1/FOLD_FLOOR` times
longer than it is wide can't be told from a fold by rounding, and bisecting
again and again near an edge's end makes such slivers. Splits at `½` (what
red–green refinement uses) don't.

**The normal cone** (`normal_cone`) is that smallest cone, with each
coefficient's direction first turned away from the axis by its own
rounding bound (`ROUNDING` times its size over its length) plus `1e-9`
rad. It holds every normal of the patch. A coefficient that rounding may
have turned by a radian or more (one that should be zero but isn't, say)
gives `NormalCone::ALL`; one that is exactly zero with no terms behind it
is left out. `NormalCone::apart` says two cones share no direction either
way round (the angle between the axes exceeds the sum of the angles, and
so does its supplement), the certificate that two patches can't meet in a
closed loop.

A cone keeps its half-angle as a cosine and a sine, and the cone and
`apart` are worked out with `+ − × ÷ √` only: angles between directions
come from the pair `(a·b, |a×b|)`, compared through the sine of their
difference. `acos` of a dot product loses half the digits near 0 (angles
below about `1e-8` rad round to 0), which let a cone miss its patch's
normals and let two cones around the same axis come out apart; and its
last bits differ between platforms' maths libraries. `angle()` (by
`atan2`) is for showing and tests only.

**Determinism.** The fold check, the cone and `apart` use only correctly
rounded arithmetic, so they give the same bits natively and on wasm.
`Conic::arc` uses `cos` and `sin`, which come from the platform's maths
library and can differ in the last bit between platforms (so do the
sketch's own angles). Arcs whose ends are known as points can be built
without them: for ends `a`, `b` at radius `r` from the centre (sweep
under 180°), the control point is `centre + (a + b − 2·centre)·2r² / |a + b
− 2·centre|²` and the weight `|a + b − 2·centre| / 2r`.

### Exact cylinder strips

`cylinder_strip(bottom, offset)` covers the strip swept by moving a conic
`bottom` (`a0 → a1`) along `offset` with two patches, `(a0, a1, b1)` and
`(a0, b1, b0)`, where the top is `bottom.translated(offset)` (`b0 → b1`).

Why it is exact: a rational quadratic triangle lies on a quadric when its
three boundary conics do and their planes meet in one point `O` on the
quadric; the patch is then the image of a flat triangle under the inverse
of the projection from `O`, a quadratic map. A ruling's plane through `O`
cuts a cylinder in two parallel rulings, and the induced parametrization
of the ruling is linear whatever `O` is: control point at the midpoint,
weight 1. The diagonal is the bottom sheared along the offset, `x ↦ x +
h(x)·offset` with `h` affine and `0, ½, 1` at `a0, c, a1`: control point
`c + offset/2`, the bottom's weight, on the cylinder (translation along the
offset keeps points on it) and planar. Its plane meets the bottom's plane
where `h = 0`, a line through `a0` along the arc's conjugate direction,
which cuts the conic again at a point `O` away from the arc; so the first
patch's three planes meet at `O`, and the second's at `O + offset`.
Tests sample points on circular cylinders within `1e-12` relative, and on
cylinders over random ellipse, parabola and hyperbola arcs.

Rulings are `Conic::line` and the top edge is `translated`, so strips over
neighbouring segments share their rulings and the caps share the top and
bottom edges to the bit. A counter-clockwise arc (seen from where `offset`
points) gives outward normals. A straight bottom gives the two flat
triangles of the parallelogram. An `offset` along a straight bottom or in a
curved bottom's plane (within a sine of `1e-9`) is refused.

### Limits and errors

| constant | value | why |
|---|---|---|
| `W_MIN`, `W_MAX` | `1/64`, `64` | edge weights; outside, conics hug their chord or control polygon |
| `MAX_CONTROL` | `1e7` | control point coordinates: 10× `MAX_COORD`, so every product stays finite |
| `FOLD_MARGIN` | `1e-6` | the fold check's `ε` |
| `FOLD_FLOOR` | `1e-8` | a normal coefficient smaller than this relative to its terms is zero |
| arc sweep | `0 < |θ| ≤ 90°` (+1e-12 relative) | one exact segment; room for rounding of equal parts |
| arc start | `|start| ≤ 16π` | keeps `cos`/`sin` accurate |

`Conic` and `Patch` have open fields, so `check()` is the trust boundary;
`new`, `from_hom` and every split or constructor end in it. `PatchError`
is `Coordinate` (not finite, or past `MAX_CONTROL`), `Weight` (outside the
bounds, or a non-positive homogeneous weight), `Parameter` (split position
outside `(0, 1)`, edge index, arc radius or angles), `Mismatch` and
`Degenerate`. `Bounds<P>` is the `f64` box (`Bounds2`, `Bounds3`); the
`f32` `Aabb` stays the renderer's.

### Tests

Property tests draw their inputs from `test_rng::Rng`, a SplitMix64 with a
fixed seed per test (no dependency, same inputs every run). They check:
arcs on their circles within `1e-12` relative, tangents along them;
derivatives against differences; the normal against `P_u × P_v · D³` and
against its Bernstein sum; splits and sub-patches reproducing the parent at
sampled points; halving symmetric to the bit; neighbours splitting a shared
edge (by `split4` or `bisect` at `½`, or halves passed to both) getting the
same bits; the fold check passing on every child of a parent that passed,
with the parent's direction; failing on folded, cusped and collinear
patches, with the cusped and collinear corners, and their pieces',
degenerate; the fold check surviving four levels of uneven splits, and a
pinched heavy piece; the exact cone path when the quick directions fail;
normal cones holding sampled normals, also of nearly flat patches whose
normals spread by less than `1e-8` rad; cones around the same axis (and
coplanar flat triangles) never apart; the corner coefficients against the
corners' tangents; clockwise sub-patches facing the other way; bad weights
(homogeneous ones too), coordinates, arcs and split positions refused;
`from_hom` giving the same bits at any scale; cylinder strips on their
cylinders, within their strip and sharing their edges.

## Tolerances (`src/tolerance.rs`)

`Tolerance` holds the design's **fit** tolerance (how far a fitted curve or
patch may be from the true surfaces), in mm, within `MIN_FIT ..= MAX_FIT`
(`1e-5 ..= 1e-1`), default `1e-3`; `new` refuses anything else. The
**resolution** is a thousandth of it: the margin the hull rules keep, and
the scale where refinement stops (repair splits nothing less than 64
resolutions across). Neither ever decides that two things are the same
vertex or edge.

## Parallel maps and determinism (`src/par.rs`)

`par_map(items, f)` is `items.iter().map(f).collect()`, on rayon's pool
natively (`par_iter`, an indexed collect, so in input order) and plain
sequential on wasm (rayon is a native-only dependency). It is the only code
that knows about rayon. The rules every parallel step follows:

- A pure map over input sorted by stable keys, collected in that order;
  never `for_each` into a shared sink.
- Floating-point reductions (sums, bounds) after collecting, sequentially in
  key order, never rayon's `sum`/`reduce`.
- Ids handed out in one sequential pass over the collected results.
- No iteration order from a hasher: sorted `Vec`s or `BTreeMap`s. A map
  only looked up in, never iterated, may be a `mesh::LookupMap` (a
  `HashMap` with a fixed hasher).
- Errors: the first in input order (collecting `Result`s does that).

Tests use `par::assert_deterministic(f)`, which runs `f` on a 1-thread and
an 8-thread pool and compares the results' `Debug` text (an `f64` prints as
the shortest decimal that reads back to the same bits, so equal text is
equal bits). `on_threads(n, f)` runs `f` on an `n`-thread pool.

## Meshes (`src/mesh.rs`, `src/mesh/`)

| file | holds |
|---|---|
| `mesh.rs` | `Mesh`, `Edge`, `Halfedge`, `Tri`, accessors |
| `mesh/face.rs` | `Face`, `FaceName`, `FacePart`, `Surface`, `Quadric` |
| `mesh/build.rs` | `MeshBuilder`: triangles by vertex id, paired up |
| `mesh/check.rs` | `Mesh::check`, `Mesh::check_faces`, `CheckError` |
| `mesh/bvh.rs` | `Bvh`: boxes, queries, self pairs |
| `mesh/hull.rs` | GJK (`apart`) and the three hull rules |
| `mesh/refine.rs` | red–green refinement: leaves, pieces, the split rules |
| `mesh/repair.rs` | `Mesh::repair`: test, split what fails, test again |
| `mesh/primitive.rs` | `Mesh::cuboid`, `Mesh::cylinder` |
| `mesh/tests.rs` and `mesh/*/tests.rs` | hand-built solids, one bad mesh per rule, determinism |

### Structure

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

`faces: Vec<Face>`: `Face { name: FaceName, surface: Surface }`.
`FaceName { feature: u64, part: FacePart }` is stable across
regenerations; `FacePart` is `StartCap`, `EndCap`, `Side { curve, segment }`
or `Split(n)` (a face an operation made). `Surface` is the construction's
claim: `Plane { n, d }` (`n·x = d`, `n` any length), `Quadric`, or `Free`.
`Quadric { origin, a, b, c }` is `F(x) = y·(a·y) + 2b·y + c` with `y = x −
origin`: measuring from a point near the surface keeps the rounding of `F`
relative to the quadric's size. `Quadric::cylinder(point, axis, radius)`
builds a circular cylinder. The distance to it is taken to first order,
`|F| / |∇F|`, and is infinite where the gradient overflows (a finite `F`
over it would put every point on the surface). A plane's `n` and `d` are
divided by `n`'s largest coordinate before measuring, so a normal of any
finite size gives the same distances (its length could overflow, putting
every point on the plane, or its square underflow).

The fields are private to `mesh` (its child modules, such as refinement,
edit them directly). `Mesh::from_parts` takes the four tables unchecked.
`MeshBuilder` takes vertices, faces, curved edges (`edge(a, b, ctrl, w)`,
either way round; others are straight) and triangles by vertex id, and
pairs halfedge `a → b` with `b → a`: topology, never a comparison of
positions. It numbers edges in the order their first halfedge comes and
refuses repeated or unpaired directed edges, bad triangles and curves no
triangle uses. What it builds still has to pass `check`.

### The invariants and `check`

`Mesh::check(&Tolerance)` is where a mesh becomes trusted; the empty mesh
passes. It returns the first failure in the order below, and within a rule
by the lowest halfedge, vertex, edge or triangle (triangle pairs
lexicographically), so a mesh always gives the same `CheckError`, in debug
and release builds alike (face tags, checked in debug only, come last).

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
5. **Face tags**, in debug builds only (`cfg!(debug_assertions)`, so tests
   too), and on demand with `check_faces` once `check` has passed: a patch
   on a `Plane` has all six control points within the resolution of it; a
   patch on a `Quadric` has 15 points (a grid four steps along each edge)
   within the resolution to first order. A plane with a zero or non-finite
   normal fails.

Steps 2–3, 5 and the hull tests of 4 run per patch or per pair through
`par_map`.

### Control hulls

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
    other's no more than the resolution past it on the other side. A point
    of a patch is a Bernstein-weighted mean of its control points, so the
    strict patch meets the plane only along the shared edge, and the two
    can meet only there. The lax side is what lets a flat cap meet a curved
    wall along a curved edge: the cap lies in the edge's plane. It may
    cross the plane by up to the resolution, so near the edge the two may
    overlap by that much: below the resolution, where nothing is told
    apart anyway, and what rounding of a cap built in the plane needs.
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

### BVH

`Bvh::new(boxes)` builds sequentially: a leaf holds up to 4 boxes; an inner
node splits its boxes at the median of their centres along the longest axis
of the centres' box, with ties broken by index (`select_nth_unstable_by` on
a total key). `query(box, margin, out)` appends every box within `margin`
of the query along every axis (a box gap can only underestimate the true
distance, and rounding the sums is monotone, so nothing within the margin
is missed), sorted ascending. `self_pairs(margin)` queries every box
through `par_map` and returns the pairs `[i, j]`, `i < j`, sorted. Results
never depend on the tree's shape or the thread count.

### Refinement (`mesh/refine.rs`)

Red–green, exact, every split at `½`. The mesh under refinement is a set of
**leaves**: the input's patches and the pieces red splits (`split4`) made of
them, each with its level (red splits from the input). The leaves need not
be conforming: a leaf may have a neighbour one level finer across an edge,
whose split left a **hanging** midpoint on it. The mesh they make, the
**pieces**, is conforming: a leaf with no hanging vertex is one piece, and
one with a hanging vertex is two green pieces, `bisect_with` at it. Two
rules keep that so:

- Before a leaf is split, every coarser neighbour is (levels across an edge
  differ by at most one, so a hanging vertex is one split deep).
- A leaf left with two or three hanging vertices is split too.

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
the face's plane with both pieces, and the edge-neighbour rule can't hold
there: the plane through a curved edge's control points is the face's
plane, and neither piece is off it. Straight inner edges fall under the
straight-edge rule, and the pieces cover exactly the region the parent did
(it is the plane). The boundary halves are the exact ones, shared with the
neighbours on other faces. A patch that is flat but tagged `Free` gets the
exact split and may then not pass.

A leaf is not split past `MAX_REFINE_DEPTH` levels, when its control
points span less than the refiner's minimum size along every axis, or when
there would be more than `MAX_PATCHES` leaves (every leaf is at least one
piece, so that round would fail anyway; stopping there bounds what one
round makes before it is counted): `TooComplex`.

### Repair (`mesh/repair.rs`)

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
  the input triangle the piece came from.
- A failing pair: the leaves of both, except for **flat** pieces, whose
  edges are all straight within the resolution (`hull::flat`), so that
  each is its own hull. If both are flat, repair fails at once with
  `Invalid` and the pair's error, naming the input triangles the pieces
  came from: the rules on flat triangles are exact, and their pieces keep
  the angles at shared corners and edges and the gaps, only smaller next
  to the margin, so no split mends them. If one is, and they are
  non-neighbours, only the other is split: splitting a flat piece brings
  no piece of it further away.

The rounds end when nothing fails, or with `TooComplex` at
`MAX_REFINE_DEPTH`, at pieces less than `MIN_SPLIT` (64) resolutions across,
past `MAX_PATCHES` or out of budget. Pieces a few resolutions across can't
keep the hull margin from their own neighbours, and splitting them only
makes more that fail (two tetrahedra touching corner to corner went from 6
failing pieces a round to thousands at about 8 resolutions); a surface that
keeps clear of itself passes long before, since a patch of size `s` on a
curve of radius `R` sags by about `s²/8R`. Round surfaces that touch stop
sooner, once the pieces at the touch are flat (at `s ≈ √(8R·resolution)`,
above `MIN_SPLIT` for `R` over about 500 resolutions). Touching flat faces
fail at once: two boxes face to face, or two cylinders side by side or end
to end, used to be split to the end of the budget (12 to 27 s) and now fail
in 2 to 80 ms. Round surfaces overlapping over an area within the
resolution (a shell thinner than it) still run out the budget, about 15 s.
In debug builds the result is checked in full, face tags aside: those are
the input's claims, which repair neither checks nor promises.

Work, from the budget: a unit per piece tested for folds, per pair tested,
per leaf split, and the number of pieces each round (the BVH and the
pieces). All is counted in sequential passes, so running out doesn't depend
on the thread count. The fold and pair tests run through `par_map` over
sorted lists; the splits, the pieces and the BVH are sequential.

### Boxes and cylinders (`mesh/primitive.rs`)

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

Parameters are refused with `KernelError::Patch` (every point within
`MAX_COORD` of the origin and finite, sizes above zero), and a solid the
tolerance can't hold (a box thinner than the resolution) with `Invalid`.

**Slivers.** A flat triangle about `3e7` times longer than it is wide is
where `check` stops working: GJK runs out of digits on hulls that long and
that close (the closest point of a simplex rounds relative to its far
points, and turning `v` by that much moves `v·w` by more than the margin),
and from about `7e7` a corner's normal coefficient falls under the fold
check's floor. A box a hundred resolutions thick passes up to `2e7` times
as long. Caps and walls should be triangulated well within that.

### Costs

`check` on a flat torus of 262 144 triangles takes about 0.7 s on one
thread and 0.27 s on eight (release; the topology pass is sequential);
about 2.7 µs per patch.

Repair of the thin shell in the tests (radius 10, both sides split evenly;
release, seven threads, one in brackets): 0.2 thick, 1 024 patches in 4 ms
(7 ms); 0.01 thick, 16 384 in 52 ms (111 ms); 0.001 thick, 262 144 in
0.88 s (1.9 s), about 16 units of work per patch. The refinement, the
pieces and the BVH are sequential, which is why seven threads give about
2×: of the 0.88 s, splitting takes 0.35 s, and rebuilding the mesh 0.2 s.
The refiner's and the builder's maps are hashed (`LookupMap`); as
`BTreeMap`s they made repair half as slow again.

### Tests

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
half), edge neighbours folded flat onto each other and across a sideways
curved edge (and passing when it curves outwards), crossing vertex
neighbours, triangles on the same corners, and wrong plane and cylinder
tags. GJK is tested against boxes a known gap apart (face to face and
corner to corner, randomly rotated and moved), point clouds either side of
a plane, flat, collinear and repeated points, the long thin hulls of a
611 × 0.066 × 0.187 box's corner in any order and rotation, and a support
point square to the closest point from every starting point; the BVH
against brute force. Plane tags with normals from `1e-200` to `1e300` long
measure the same, and a quadric whose gradient overflows fails. A mesh breaking two rules gives the earlier invariant's error
(bounds before a fold, hulls before a face tag, in debug builds too).
`check`, the BVH's pairs, and the first failure of a jittered torus are the
same at 1 and 8 threads.

Refinement: a red split bisects its three neighbours (the red children are
`split4`'s, the green ones `bisect`'s); splitting a green piece splits its
leaf; splitting at a corner again and again stays graded (a bounded number
of new pieces per level, and the mesh stays closed); a flat cap's pieces
have straight inner edges and pass `check_faces`; too deep and too small
leaves aren't split, nor any past the most leaves there may be. Repair: a thin shell (two round octahedra 0.2 and 0.05
apart, the inner facing in) is split evenly until it passes; a cylinder
with a box beside it, 0.1 to 0.001 off the wall, is split only near the
box, keeps every piece on its cylinder or plane within `1e-12` and the box
whole; a tetrahedron bulging so far its patches fail the fold check passes
at 16 patches; a mesh that passes comes back unchanged; a wrong face tag
is carried through (the result fails only `check_faces`); a cusp fails at
once, as do tetrahedra corner to corner and boxes face to face closer than
the resolution; round octahedra touching fail once the pieces at the touch
are flat, and small ones once they are too small; a small budget runs out,
and bad topology and weights are refused; the shell's and the cylinder's
repairs are the same at 1 and 8 threads.
Constructors: 50 random boxes and cylinders (sizes `1e-2..1e3`, moved up to
`1e5`) pass `check` and lie on their faces within `1e-12`; faces are named
as an extrude names them; bad parameters are refused, and a box thinner
than the resolution fails `check`; boxes a hundred resolutions thick and up
to `2e7` times as long pass at every tolerance.

## Limits, budgets and errors (`src/lib.rs`, `src/budget.rs`, `src/error.rs`)

| constant | value | why |
|---|---|---|
| `MAX_PATCHES` | `1 << 22` | patches in a mesh; ids and counts fit a `u32` |
| `MAX_REFINE_DEPTH` | 24 | red splits from an input patch: `2^24` times smaller |
| `MAX_WORK` | `1 << 26` | work units in one operation: about half a minute on one thread |
| `MIN_SPLIT` (repair) | 64 resolutions | the smallest piece repair splits |

`Budget` is a limit (`Budget::new(work)`, at most `MAX_WORK`;
`Budget::DEFAULT`); an operation counts it down in a `Work` its steps share
(`repair_within` takes one), and running out is `TooComplex`. A unit is
about a patch or a pair of patches tested or split: repair measured about
0.5 µs a unit on one thread and 0.3 µs on seven. `KernelError` is
`TooComplex`, `Invalid(CheckError)` (the input breaks an invariant the
operation can't restore, or the result would), and `Patch(PatchError)` (a
parameter, or a split outside the patch bounds). `MAX_TRACE_STEPS` comes
with tracing.

## Deviations

- **The normal numerator is a cubic with 10 coefficients**, not a quartic
  with 15. The quartic form, `D·(N_u×N_v) + …`, is this cubic times
  `u0+u1+u2`; its 15 coefficients are the cubic's degree elevation. The
  cubic's coefficients are the ones splitting provably preserves (above),
  and they are cheaper. The mesh's fold invariant uses these 10.
- **The exact fold direction is the smallest enclosing cone**, found by
  enumerating its possible rims, instead of a 3-variable LP. It answers
  the same question for the Euclidean margin (an LP over a box would be
  conservative by up to √3) and gives the normal cone as well.
- **No exact cone triangles.** A triangle on a true cone (not a cylinder)
  can't have a straight ruling parametrized linearly: the ruling's induced
  parametrization depends on the projection point, so the two patches on a
  ruling would need different edge records. Built with a linear ruling
  anyway, a unit cone patch is off by about `8e-4` inside. Taper is out of
  scope, so only `cylinder_strip` exists; cones need a different edge
  scheme when they come.
- `Conic2`/`Conic3` are aliases of one generic `Conic<P>` over the sealed
  `Point` trait, and there is an `f64` box type, `Bounds<P>`.
- The weight bounds stay at `1/64 ..= 64`: nothing in the tests asked for
  other values.
- **Edge neighbours across a curved edge**: the plane through the edge's
  control points must have one patch's other control points more than the
  resolution off it and the other's no more than the resolution past it on
  the other side, not both strictly off it. Both strictly off would refuse
  every flat cap along a curved edge, since the cap lies in that edge's
  plane; with one side strict the two still meet only along the edge.
  Straight edges and vertex neighbours, whose plane is free, keep both
  sides strict.
- **Neighbours are told apart by the vertices two triangles share**
  (none, one, two, or three, which always fails), not stored adjacency.
- **The face-tag check** takes a plane's six control points, and samples 15
  points of a quadric patch against the first-order distance `|F|/|∇F|`.
  `Quadric` carries an `origin` its form is written around. Tags are
  checked in debug builds by `check` and on demand by `check_faces`.
- **`Tolerance` lives in the kernel** (fit bounds `1e-5 ..= 1e-1` mm,
  default `1e-3`), and `check` takes it for the hull margin.
- `Solid` still wraps `Shape`; it becomes a checked `Mesh` with
  tessellation.
- **Flat faces are split with straight inner edges** (red and green), not
  by the exact blossom: see "Refinement". The region is the same; the
  exact split's curved inner edges would lie in the face's plane with both
  pieces, where no plane through the edge separates them.
- **Refinement keeps leaves and pieces** rather than editing the halfedge
  mesh in place: leaves may have one hanging vertex per edge, rendered as
  green halves, and the mesh is rebuilt from the pieces with `MeshBuilder`
  (pairing by vertex id). Green pieces are never split themselves; their
  leaf is.
- **Repair splits both patches of a failing pair**, except a flat
  non-neighbour, and fails at once on a degenerate corner
  (`Patch::degenerate_corner`, new) and on a failing pair of flat pieces,
  and on pieces under `MIN_SPLIT` = 64 resolutions as well as at
  `MAX_REFINE_DEPTH` and the budget.
- **`Budget` is a limit and `Work` its counter**: operations take `&Budget`
  as planned and count down a `Work` shared by their steps. `MAX_WORK` is
  `1 << 26`, about half a minute of repair on one thread. `MAX_TRACE_STEPS`
  is left to tracing.
- **The box and cylinder are `Mesh` constructors** (`Mesh::cuboid`,
  `Mesh::cylinder`) that take the feature id and the tolerance and always
  run `check`; wrapping them in `Solid` comes with tessellation.
- **GJK's triangle and tetrahedron projections use cross and triple
  products** instead of the Gram matrix (a fix: see "Control hulls").
