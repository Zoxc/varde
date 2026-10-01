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
parallel map (`par`), below, and `Solid`, a checked mesh, with its
tessellation for drawing (`tessellate`) and its volume and area,
`extrude`, which sweeps a `Profile` into a solid, and `boolean` and
`touches` for solids of flat and curved patches (curved cuts exact where
planes meet planes or quadrics, traced and fitted elsewhere). Documents
store no geometry:
bodies are the outputs of the feature history, which `varde-regen`
evaluates into solids, extrudes making bodies or joining, cutting and
intersecting them, and draws (see "Bodies from the history").

## Patch math (`src/patch.rs`, `src/patch/`)

Pure math on one curve or one triangle, no mesh:

| file | holds |
|---|---|
| `patch.rs` | module docs, the limits, `Bounds<P>`, `PatchError` |
| `patch/conic.rs` | `Conic<P>` (`Conic2`, `Conic3`), the `Point` trait over `DVec2`/`DVec3`, exact arcs |
| `patch/triangle.rs` | `Patch`: net, blossom, evaluation, derivatives, sub-patches and curves over domain segments (`curve`), splits |
| `patch/fold.rs` | the normal's Bernstein coefficients, the fold check, `NormalCone` |
| `patch/strip.rs` | `cylinder_strip`: exact cylinder patches |
| `patch/tests.rs` | property tests; `src/test_rng.rs` is their seeded generator |

Angles go through `src/trig.rs` (see "Deterministic trigonometry").

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
  matching ellipse arc). Those take angles, through `trig::cos` and
  `trig::sin` (see "Deterministic trigonometry"); `Conic2::arc_between(center,
  r, a, b)` builds the same arc (under 180°) from its ends without them,
  by `+ − × ÷ √` only: with `m = a + b − 2·center`, `c = center + m·2r²/|m|²` and
  `w = |m|/2r`. Profiles from sketches are built with it.
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
renormalizes each half. A coordinate all three control points share (a
curve in a plane square to an axis, as CAD models are full of) is kept
exactly in the halves and in points of the curve (`Point::shared`):
homogeneous division alone rounds it, and the pieces of a flat cap's rim
came out an ulp off its plane, so the cap of one extrude and the flush
cap of the next no longer tied but nearly tied. Patches do the same in
`eval`, `curve` and the inner edges of their splits. The halves' weights lie between the parent's and 1
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
`Conic::arc` uses `cos` and `sin` from `trig`, which are the same code
everywhere, so its bits are the same too (the sketch takes its angles the
same way). Arcs whose ends are known as points can be built without
them: for ends `a`, `b` at radius `r` from the centre (sweep under 180°),
the control point is `centre + (a + b − 2·centre)·2r² / |a + b −
2·centre|²` and the weight `|a + b − 2·centre| / 2r`.

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

Every angle the kernel turns into a place or a decision goes through
`trig` (below), so it has the same bits on every platform too.

Tests use `par::assert_deterministic(f)`, which runs `f` on a 1-thread and
an 8-thread pool and compares the results' `Debug` text (an `f64` prints as
the shortest decimal that reads back to the same bits, so equal text is
equal bits). `on_threads(n, f)` runs `f` on an `n`-thread pool.

### Deterministic trigonometry (`src/trig.rs`)

`+ − × ÷ √` are correctly rounded, so they give the same bits everywhere;
std's `sin`, `cos`, `atan2`, `ln`, `powi`, ... are not: they come from the
platform's maths library (glibc natively on Linux, the system's own on
macOS and Windows), whose last bits differ. On `wasm32-unknown-unknown`
std's are the `libm` crate's code already. `varde_kernel::trig` wraps
`libm` (pure Rust, pinned by `Cargo.lock`): `sin`, `cos`, `sin_cos`,
`tan`, `asin`, `acos`, `atan2`, `unit(angle)` (`(cos, sin)` as a `DVec2`)
and `angle(v)` (`atan2(v.y, v.x)`). So natively the kernel gets the web's
bits, and every platform the same. `sin_cos` is two calls, never a joint
`sincos`. A `libm` upgrade can change bits, on every platform at once.

The kernel's decisions use only `+ − × ÷ √`, exact signs and `trig`.
`crates/kernel/clippy.toml` keeps it so: `disallowed-methods` refuses
std's `f64` trigonometry, exponentials, logarithms, `powf`, `powi`,
`cbrt` and `hypot`, and glam's methods built on them (`DVec2::from_angle`,
`to_angle`, `angle_to`, the vectors' `angle_between`, `rotate_*` and
`slerp`, and the angle and rotation constructors of `DMat2`, `DMat3`,
`DQuat`, `DAffine2` and `DAffine3`). Tests that use std as an
independent reference, or to build inputs, allow
`clippy::disallowed_methods` in their module (`src/test_rng.rs` too, so
its inputs keep their bits); tests comparing bits with the code use
`trig`. Users: `Conic::arc`, the clean-up's Delaunay flip test (the angles
opposite a diagonal, by `atan2`) and `NormalCone::angle`.

Checked across platforms when this went in: a scratch `cdylib` on the
kernel, the sketch and regen's profile step hashed the profiles, their
conics, extrudes and joins of random sketches (lines, circles, arcs,
closed splines) natively (x86_64 Linux) and as `wasm32-unknown-unknown`
under node. The hashes were equal, and equal to the web's before the
change (which only moved native bits); before it, native differed.
Checked again with chains of two to four features: polygons given
fillets, chamfers (by distance and by angle) and offsets as edits,
extruded on tilted frames and joined, cut and intersected in turn, and
the solver on angle and length dimensions (systems up to about a hundred
variables): equal natively and on wasm, and at 1 and 8 threads. On
sketches at exact angles (multiples of π/4, `±0` coordinates, radii from
0.1 to 1e5) no edit or profile changed its outcome from before, only
last bits. `libm`'s own per-architecture code (x86, aarch64, wasm32) is
`sqrt`, `fma`, rounding and the like, all exact, so it can't split the
platforms either.

## Meshes (`src/mesh.rs`, `src/mesh/`)

| file | holds |
|---|---|
| `mesh.rs` | `Mesh`, `Edge`, `Halfedge`, `Tri`, accessors |
| `mesh/face.rs` | `Face`, `FaceName`, `FacePart`, `Surface`, `Quadric` |
| `mesh/build.rs` | `MeshBuilder`: triangles by vertex id, paired up |
| `mesh/check.rs` | `Mesh::check`, `Mesh::check_faces`, `CheckError` |
| `mesh/bvh.rs` | `Bvh`: boxes, queries, self pairs |
| `mesh/hull.rs` | GJK (`apart`) and the three hull rules |
| `mesh/orient.rs` | invariant 5: shells, their volume signs, their nesting by rays |
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
   on a `Plane` has all six control points within the resolution of it; a
   patch on a `Quadric` has 15 points (a grid four steps along each edge)
   within the resolution to first order. A plane with a zero or non-finite
   normal fails. It is cheap next to 1 to 4 (release, measured): about 13%
   of their time single-threaded on extruded plates with 16 and 64 holes
   (1 004 and 4 012 patches), 6 to 10% on 7 threads, and 0.5% on a
   262 144-patch torus of `Free` faces.

Steps 2–3, 6 and the hull tests of 4 run per patch or per pair through
`par_map`, and step 5 per triangle and per shell. `check_counted` is
`check` returning how many patches step 5 integrated, for callers that
charge work; `check_embedding` (repair's) stops after step 4.

### Orientation (`mesh/orient.rs`)

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
In debug builds the result is checked in full, face tags aside: repair
refuses a wrong `Plane` tag on a patch it splits (see "Refinement"), but
other tags, and those of patches it doesn't split, are the input's claims,
which it neither checks nor promises. `Solid::new` checks them.

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
about 2.7 µs per patch, before the orientation (above) added about 10%.

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
forty random thin rings; 300 random nestings of boxes and cylinders up
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
the resolution; round octahedra touching fail once the pieces at the touch
are flat, and small ones once they are too small; a small budget runs out,
and bad topology and weights are refused; the shell's and the cylinder's
repairs are the same at 1 and 8 threads.
Constructors: 50 random boxes and cylinders (sizes `1e-2..1e3`, moved up to
`1e5`) pass `check` and lie on their faces within `1e-12`; faces are named
as an extrude names them; bad parameters are refused, and a box thinner
than the resolution fails `check`; boxes a hundred resolutions thick and up
to `2e7` times as long pass at every tolerance.

## Solids and tessellation (`src/solid.rs`, `src/tessellate.rs`)

`Solid` wraps a `Mesh` that passes `check`: `Solid::new(mesh, tol)` runs
the check and fails with `Invalid`, `Solid::cuboid` and `Solid::cylinder`
wrap the checked primitives, and `Solid::empty()` is the empty solid.
`mesh()`/`into_mesh()` give the patches back, `bounds3()` is the `f64`
box around the control points (which holds the solid) and `bounds()` the
same in `f32` (`None` when empty; rounding is monotonic, so it holds the
tessellation too).

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
- **Shared samples.** Each edge's points are evaluated once, along its
  first halfedge (the lowest), from its conic (`Conic3::eval`), and both
  patches beside it use those vertices, so neighbours share their boundary
  points to the bit: no cracks.
- **The inside of a patch.** A patch whose edges are all one step is one
  triangle. Otherwise, with `m = max(counts, 3)`, it is sampled at the
  points of the regular barycentric grid of `m` steps at least one step in
  from its boundary: `(i + 1, j + 1, k + 1) / m` with `i + j + k = l = m −
  3`, triangulated regularly (`l²` triangles). The ring between that inner
  grid and the boundary is three strips, one per edge, from the edge's `n`
  segments to the inner grid's side of `l`, split at the diagonals from
  each corner to the inner grid's corner next to it: `n + l` triangles
  each. A strip advances the side that makes the shorter new diagonal
  (the outer one on a tie), so its triangles follow the geometry where
  the grid runs skewed to the edges. With `l = 0` the ring is a fan round
  the single inner point.
- **Normals** are the patches' own (`Patch::normal`, normalized; the fold
  direction stands in should it vanish). Along an edge, the two sides'
  normals are compared at every sample: if they agree within 1° everywhere
  (dot at least `cos 1°`) the edge is **smooth**, its inner samples are one
  vertex each with the normalized mean normal; if not it is **split**,
  and each side gets its own vertices. Round a mesh vertex, the corners
  between two split edges are one vertex, with their normals summed and
  normalized; a vertex with no split edge is one vertex.
- **Feature edges** (`RenderMesh` edges, along the edge's samples): every
  split edge, and every edge between two faces of different names, where
  the walls of one profile curve's segments (`Side { curve, .. }` of one
  feature) count as one name. So a cylinder draws its two rims, not the
  seams between its four quarter walls, and a box its twelve edges, not
  the diagonals of its sides.
- **Limits.** Triangle, vertex and feature-edge counts are worked out
  from the segment counts before any point inside a patch is evaluated,
  and more than `RenderMesh::MAX_*` fails with `MeshError::TooLarge`.
  The limits are a parameter (`Limits`, `tessellate_within`) so tests
  reach them with small meshes: each part may be exactly its limit, one
  more fails. The parts go
  through `RenderMesh::from_parts`, so a position past `MAX_POSITION`
  (a mesh's control points may reach `MAX_CONTROL`) fails with `Values`.
- **Determinism.** Counts, normals, edge points and patches are pure maps
  through `par_map`; vertex numbering is one sequential pass (corner
  groups by vertex, then edge samples by edge, then patch interiors by
  triangle).

The chord target holds on the edges. Inside a patch the grid spacing
follows the largest count, but at a corner of a skewed patch (a cylinder
wall triangle, whose far corner is round the arc) the inner grid's corner
is two steps round from the patch's, and the triangle there is two steps
wide: on the test cylinder the worst triangle's middle is 1.85 chords off
the surface. Very skewed patches (a wall a hundredth of its arc
high) stitch into slivers whose face normals are far from their vertex
normals; shading uses the vertex normals, so it doesn't show. Positions
are `f32`: far from the origin, features smaller than an `f32` step
there (about 0.03 mm at 5e5 mm) collapse into degenerate or flipped
triangles, as in any `f32` mesh, but no cracks open: both sides of an
edge read the same rounded samples.
A flat torus of 262 144 patches tessellates in about 0.13 s
(release, several threads).

Tests: a cube is 12 triangles, 24 vertices and 12 edges with axis
normals; a cylinder's normals are radial on the wall and axial on the
caps, it encloses slightly less than `πr²h`, its triangles are within the
chord (2.5 chords at the skewed corners), only its rims are feature
edges, and a rim point is two vertices (wall and cap); round octahedra,
half cylinders, a torus, a box, and a repaired thin shell (pieces of mixed
sizes) are watertight: every triangle side is met by one running the other
way between the same positions, to the bit; edge counts meet the chord and
turn by dense sampling and one fewer wouldn't; the strip joins any two
counts; results are the same at 1 and 8 threads; far positions are
refused.

## Profiles and extrude (`src/profile.rs`, `src/extrude.rs`, `src/extrude/`)

| file | holds |
|---|---|
| `profile.rs` | `Profile`, `Loop`, `Segment`, `ProfileError`, `Profile::check`, signed areas |
| `extrude.rs` | `Frame`, `extrude`, building the mesh, the walls' surfaces |
| `extrude/chain.rs` | the segments the solid is built on: classifying, cusps, separating |
| `extrude/cap.rs` | the caps' triangulation and the rounds that mend it |
| `profile/tests.rs`, `extrude/tests.rs` | profile builders (exact arcs without trig, circles, rectangles), shapes with analytic volumes |

### Profiles

A `Profile` is loops of `Segment { conic: Conic2, curve: u64 }`: outer
loops counter-clockwise, holes clockwise, so the region is on the left of
every segment; the curve id names the wall. The kernel knows no sketch:
whoever builds a profile (regen) turns lines into `Conic2::line`, arcs
into exact conics of at most 90° and splines into fitted chains, and
merges regions (`varde_sketch::Profiles::merge`, exact, by the pieces'
shared vertices; see `agents/sketch.md`). A sketch piece's ends are put at
its vertices (`Profiles::vertices`), so segments close to the bit. `Profile::check` holds what needs no tolerance: at least
one loop, at most `MAX_PROFILE_SEGMENTS` (`1 << 16`) segments, loops of
two or more segments, each segment within the patch bounds with its ends
within `MAX_COORD` and apart, each segment starting **to the bit** where
the one before ends (closure is never decided by distance), and every
loop enclosing some area. `Loop::area` is signed: the polygon of the ends
plus each segment's bulge off its chord, `½∫ (P − p0) × P' dt`, by
Gauss–Legendre over pieces split off at `½` until their weights are within
`0.9..=1.1` (a weight far from 1 crowds a conic's points, which the rule
can't follow). Whether loops touch, cross or nest needs the resolution and
is the extrude's to find.

### Extrude

`extrude(profile, frame, from, to, feature, tol, budget)`: the profile on
`Frame { origin, x, y }` (unit axes, square within `1e-9`; the normal is
`x × y`), swept from `from` to `to` along the normal (`from < to`, both
within `MAX_COORD`, every corner of the solid within `MAX_COORD` of the
origin). Faces: `StartCap` (at `from`, facing back), `EndCap`, and
`Side { curve, segment }` per input segment, `segment` counting the
segments of that curve in profile order; pieces a segment is split into
share its face. Caps are tagged with their planes, straight walls with
theirs, curved walls with the cylinder over their conic (below). The
steps:

1. **Chain** (`Chain::new`). A segment whose control point is within the
   resolution of its chord (and between its ends) becomes
   `Conic2::line`; one on the chord's line past an end runs back on itself
   and is `Degenerate`. At each joint the tangents (towards the control
   points, or along a straight segment) must not leave the same way within
   a sine of `SIN_MIN` (`1e-3`): a `Cusp`, angle 0° or 360°. The curved
   segments of a two-segment loop are halved, so no two segments share
   both ends.
2. **Separation** (`Chain::separate`). Every two segments' control hulls,
   through a BVH over their boxes: two that share no end must be more than
   the resolution apart (GJK, as for patches), two in a row must be split
   by a line through their shared end with the resolution to spare (the
   hull of one's other control points and the other's mirrored through
   the end is more than the resolution from it). Failing curved segments
   are halved at `½` and the round runs again. Halving shrinks hulls, so
   passing pairs keep passing. Then no two segments cross or touch, the
   polygon of the chords is simple, and each curve's bulge off its chord
   lies in its own hull, clear of everything else. A failing pair that
   can't be halved (straight, halved 24 times, or less than `MIN_SPLIT` =
   64 resolutions across: such pieces can't keep the margin from their own
   neighbours, and halving them only makes more that fail) is `Touching`.
   This is what makes a thin ring's arcs short enough that the two
   circles' chords keep apart.
3. **Caps** (`cap::triangulate`). The constrained Delaunay triangulation
   (`spade`) of the chord polygon's vertices and the Steiner points, with
   the chords as constraints. Its triangles are classified by winding
   number, walked from the outer face across sides: crossing a chord from
   its right to its left adds one. The region is where it is 1; any other
   value than 0 or 1 is `Nesting` (a hole outside everything, an outer
   loop in material, a loop running the wrong way). Each triangle's patch
   takes the segments along its sides as curved edges; inner edges are
   straight. Then the corners are checked between the curves' tangents,
   not the chords (a corner is open when the cross product exceeds
   `SIN_MIN` times the lengths):
   - An **ear** (two of the loop's segments meeting at a corner of one
     triangle) whose tangents turn by 180° or more, like two arcs in a row
     of a circle, gets a Steiner point at the triangle's centroid.
   - A corner between a curved segment and an inner edge that isn't open
     gets that segment halved: the new chord leaves the corner closer to
     the curve's tangent, which halving keeps. For a
     segment bulging into the region (concave, control point on the left)
     an open corner at both ends means the control point, and so the whole
     bulge, lies inside the triangle.
   - A triangle whose corners are open but whose patch fails the fold
     check gets its curved segments halved. For weights up to 1 the fold
     check passes exactly when a bulge's control point is inside the
     triangle; above 1 a little before.
   - A Steiner point within the resolution of a concave segment's hull
     would leave the region once that segment is halved, so the segment is
     halved instead.
   Each round triangulates afresh (at most 32 rounds, segments halved at
   most 16 times all told here, and never below `MIN_SPLIT`; past these,
   `TooComplex`). Coordinates below `1e-30` are flushed to 0 for spade,
   which refuses tiny non-zero ones: its exact predicates then decide only
   which triangles there are, and moving points by that little changes
   only triangles with corners that close to a line, slivers along the
   convex hull outside the region (separation keeps every vertex far from
   the chords it doesn't end). The points go in in a fixed shuffled order
   (SplitMix64, the same everywhere), with spade's hierarchy for point
   location, and spade's vertex numbers are mapped back to ours: in the
   loops' order each vertex of a hole inside a fine outline took apart
   about half the triangles, `n²` flips before the budget was looked at
   (two circles of 32 768 sides: 7.5 s; now 0.2 s).
   - **Flat corners, on the second try** (step 5): a corner at a loop
     vertex that is obtuse with the vertex within 64 resolutions of the
     opposite side (short segments meeting nearly straight, such as a
     circle cut into uneven arcs) gets a Steiner point moved in from the
     vertex along the bisector of the tangents there, by half the shorter
     chord meeting there (or a quarter or a sixteenth of that): where the
     way in crosses no segment's hull, the point is more than half that
     distance and 4 resolutions from every hull and Steiner point, and no
     Steiner point is already within that distance of the vertex. Of two
     new points too close, the later vertex's is dropped. A flat ear
     whose corner gets one gets no centroid. Such a sliver comes within
     the resolution of the walls, and a flat ear's centroid lies as close
     to its sides as the ear is flat; the point takes the vertex's corner
     instead.
4. **Mesh** (`build`). The chain's vertices and the Steiner points at
   `from`, then again at `to` (placed there, not moved up by `offset =
   normal·(to − from)`: `from + (to − from)` can round away from `to`, and
   a solid extruded from `to` on, a boss on this one's top, would then
   stand a rounding off flush rather than on it). Each segment's
   wall is two patches, `(a0, a1, b1)` and `(a0, b1, b0)`, whose curved
   edges are set from `cylinder_strip` (bottom, top = bottom moved by
   `offset`, and the diagonal), so the caps share the walls' edge records
   (`MeshBuilder::wall` and `curved_wall`, which the box and cylinder use
   too).
   End cap triangles as triangulated, start cap reversed.
5. **Repair and check**: `repair_within` with the same work, then
   `Solid::new_within` checks it all (charging the patches the
   orientation step integrated). In every test so far repair finds nothing
   to split: the construction already passes. If steps 3 to 5 fail with
   `Invalid` or `TooComplex` and work is left, they run again from the
   separated chain with flat corners mended (step 3); if that fails too,
   the first error stands. Moving points in from every flat corner can
   line them up into slivers of their own (along a fine polygon), so it
   isn't the first try: the second only adds solids. A circle of radius
   0.01 to 1 000 cut at random angles into arcs of 0.06° to 86° failed
   at the default tolerance one time in three before, and now one in
   thirteen, all of radius under 0.11, whose shortest arcs are under a
   hundred resolutions long.

A curved wall's surface is the cylinder over its conic: with `λ` the
barycentric coordinates of a point's projection on the conic's control
triangle `p0, c, p1`, a conic of weight `w` is `λ1² = 4w²·λ0·λ2`. The `λ`
are affine in the point and constant along the normal (projected with the
dual axes of `x`, `y` and the normal, so it holds for axes a little off
square), which makes this a `Quadric`, written around `p0`. Arcs get the
circular cylinder this way, ellipse, parabola and hyperbola arcs their
cylinders. Straight walls: the plane through the segment, normal
`chord × normal` (out of the region).

Work: separation spends the segments plus the pairs each round (counted
before they are collected, see "BVH"), triangulation 8 per vertex before
triangulating plus the triangles each round, placing flat corners' points
the segments, Steiner points and candidates plus the ones found near, then
the patches, then repair, then 32 for each patch the check integrated
(`INTEGRATE_WORK`; about six for each cylinder-like wall). Segment counts past `MAX_PATCHES / 4` are
`TooComplex`.

Measured (release): the tests' 80 × 80 plate with 64 round holes 1.1
apart (260 segments) comes out with 940 vertices a cap and 4 012
patches; a 210 × 210 plate with 400 such holes, 22 620 patches in
0.47 s, on the second try (holes pass 0.1 from the plate's sides, which
have no vertices: the edges between the holes along a side keep leaving
their arcs along the tangent, and halving those arcs never ends); a
plate with four holes splits nothing (20 segments, 92 patches). A ring
of radius 10, 0.001 wide, needs 1 024 segments. 600 random plates with
holes and weights from 0.05 to 20, most refused as touching: the slowest
took 32 ms. A circle of 4 096 arcs of radius 100 gives about 22 300
patches in 0.13 s. A 16 384-gon of radius 100 takes 0.5 s; a 65 536-gon
fails as `Invalid` in 5 s (its sides turn by 2e-7 over 1e-2). A quarter
disc whose arc is 16 384 straight pieces, and two circles of 16 384
sides round each other, are `TooComplex` in about 1 s: their caps are
fans or strips of long thin triangles whose boxes overlap by the
thousand, more pairs than the budget. A square with one side a conic of
any weight from `1/64` to 64 bulging either way passes (with weight 20
or more and bulging well into the region, after halving that side once
for the fold check).

Tests: boxes, cylinders, a plate with holes, a slot, a half disc, a
lens, a thin ring, a rounded rectangle, a bitten square, an S, an
ellipse and a parabolic arch have their analytic volumes and areas and
pass `check_faces`; so do random outlines and random plates with holes at
every tolerance, or they are refused as touching, nesting wrongly or
cusped; a tilted frame far out, also with axes a little off square;
faces named per curve in profile order, the pieces of a halved segment
on its face; every refusal (extents, frames, overlapping, touching and
crossing loops, bad nesting, cusps, segments running back, too thin, out
of budget) with its error; the same bits at 1 and 8 threads. Circles cut
into uneven arcs at three tolerances (the second try, at 1 and 8
threads too); a star of 65 535 long chords refused within its budget
rather than collecting two billion pairs; two circles of 16 384 sides
triangulated in about a second unoptimized (over a minute in the loops'
order).

Known gaps:

- **Sharp or crowded curves at coarse tolerances** can come out
  `Invalid` from repair (a flat cap piece and a wall piece a vertex apart
  within the resolution) instead of `Touching`: seen only with conic
  weights far from 1 (0.05 to 20) meeting at narrow angles.
- **Repair of a cap patch along a concave curve**, should it ever be
  needed, splits with straight inner edges; a piece whose corner at the
  curve's midpoint turns inside out then fails with `TooComplex`. The
  construction keeps concave bulges inside their triangles and passes
  `check` without repair in every test.
- Flat cap triangles thinner than the resolution (nearly collinear
  vertices) fail as flat pairs in repair: `Invalid`. The second try
  mends those at loop vertices, but **fine polygons at coarse
  tolerances** (thousands of sides turning by about a resolution over
  their length, a 1 024-gon of radius 10 at fit 0.1) still fail: the
  moved-in points line up into slivers of their own, as their triangles
  between each other are as flat. Quality refinement of the caps
  (circumcentres of bad triangles) would mend it.
- **Caps of long thin triangles**: fans from one vertex (a sector whose
  arc is many straight pieces) and strips between two fine polygons have
  boxes overlapping by the thousand; past a few thousand segments their
  pairs exceed the budget (`TooComplex`). Also mended by refinement, or a
  finer broad phase than boxes.
- **Narrow corners where halving doesn't converge**: an inner edge along
  a curve's tangent between two curves that both come close to a
  straight side with no vertices near (a row of holes 0.1 from a plate's
  side) keeps coming back as the arcs are halved, until `MAX_CAP_DEPTH`;
  the second try's moved-in points mend the cases seen.

## Volume and area (`Solid::volume`, `Solid::area`, `src/quadrature.rs`)

The volume is a third of `∫ (P − o)·n` over the surface (divergence
theorem, `o` the middle of the bounds), the area `∫ |P_u × P_v|`, each
patch integrated over its parameter triangle cut into its four half-edge
pieces, each by the 8 × 8 Gauss–Legendre rule through the collapsed
square (`u0 = s`, `u1 = (1 − s)t`, Jacobian `1 − s`): 256 points. A patch
with a weight outside `0.7..=1.4` is split (`split4`, up to five times)
first. The tests hold it to `1e-12` relative on boxes, cylinders and
extrudes. Patch sums are added sequentially in patch order.
The Gauss nodes and weights are written out, not computed, so no
platform's `cos` decides them.

## Booleans (`src/boolean.rs`, `src/boolean/`)

`boolean(a, b, op, tol, budget)` gives `a ∪ b`, `a − b` or `a ∩ b`
(`Op::{Union, Difference, Intersection}`) as a `Solid`, and `touches(a,
b, tol, budget)` whether two solids meet, running only the broad phase
and the counting (and nothing, answering false, for solids whose boxes
are more than the resolution apart, so asking it of far bodies is
cheap). They follow Manifold's `boolean3.cpp` and
`boolean_result.cpp`: every topological fact comes from a few
primitives, each worked out once and stored by the pair it is about,
through identities that hold whatever values the primitives take, so the
result is a closed manifold by construction and nothing is ever merged
because two points are close.

For **flat patches** (every edge straight within the resolution,
`hull::flat`; such a patch is taken as the triangle on its corners) the
primitives are exact and every cut is a straight segment. With a
**curved patch** in either operand the primitives are numerical (see
"Curved primitives"), pairs of faces are decided by certificates and
refinement (see "Pairs of faces"), and each decided arc is cut along a
chain of shared edges: exact where two planes or a plane and a quadric
meet (the faces' tags say), traced and fitted within the fit tolerance
elsewhere (see "Cutting curved faces").

| file | holds |
|---|---|
| `boolean.rs` | `Op`, `BooleanError`, `UP`, `Cross11`, the `Primitives` trait, `boolean`, `touches`, building the mesh, `parts` (connected parts, for the rays and the clean-up) |
| `boolean/input.rs` | `Input`: an operand's tables (corners, edges' ends and triangles, boxes, patches, which edges are straight and which patches flat or planar), vertex normals, flat volume |
| `boolean/curved.rs` | `Curved`, the primitives with curved patches: ray-derived shadow crossings, layers above a vertex, crossings of an edge through a patch, ties |
| `boolean/curved/ray.rs` | the ray tests `ρ` (exact for straight edges and at every edge's ends) |
| `boolean/curved/arcs.rs` | where two edges' shadows cross: one conic written implicitly, the other put in, a quartic |
| `boolean/curved/solve.rs` | points of a patch above a vertex, and an edge's crossings through a patch: subdivision and Newton |
| `boolean/curved/bernstein.rs` | Bernstein polynomials: products, evaluation, root isolation |
| `boolean/pairs.rs` | each pair of faces' ends and arcs; for curved operands the certificates, the refinement loop (`refined`) and the fixed rules |
| `boolean/exact.rs` | exact signs: `Approx` (float with an error bound), `Exp` (expansions), `Poly` in `ε`, `Pred`, `sign`, `orient2d` |
| `boolean/flat.rs` | `Flat`, the primitives of flat operands, with the symbolic perturbation |
| `boolean/count.rs` | broad phase, the stored primitives, `x12`/`x21`, winding numbers |
| `boolean/surface.rs` | the exact paths: what a patch lies on (`Shape`), crossings solved again on planes and quadrics, a plane's conic on a quadric (`section`) |
| `boolean/chain.rs` | each arc's chain of shared edges: straight, exact, or traced and fitted; halving its curves |
| `boolean/chain/trace.rs` | the point where two patches meet (Newton on four unknowns), marching along the cut, fitting conics, inverting a point into a patch |
| `boolean/assemble.rs` | new vertices, kept pieces of edges, cut edges, the rounds of cutting the faces, the faces' copies |
| `boolean/assemble/face.rs` | one face cut: its layout, loops, curved sides, triangles, their inner edges' curves (exact bands on quadrics) |
| `boolean/assemble/merge.rs` | merging refinement's pieces that came through whole |
| `boolean/triangulate.rs` | a face's kept loops in its parameter domain into triangles, curved sides' corners, Steiner points |
| `boolean/cleanup.rs` | collapsing and flipping the degenerate triangles flush operands leave |
| `boolean/tests.rs` | boxes in every flush, edge-on and vertex-on configuration, tori, determinism |
| `boolean/curved_tests.rs` | cylinders and boxes (exact), crossing cylinders, a free surface, a saddle, extrudes, chains, merging, random bars, walls over arcs with level ends |
| `boolean/seeded_tests.rs` | the seeded random suite: related pairs, parts built in chains of twenty, turned solids, near tangencies, pins and coaxial cylinders, flush bosses |

### The primitives

One fixed projection direction for everything, `UP = (2, 3, 32)`: nearly
`+z`, tilted off every axis so walls along the axes don't all project to
lines, with small integer coordinates so exact predicates take it as it
is. Heights, "above" and "below" are along it; "seen from `+UP`" is the
projection.

- `s02(v, f)`: the signed number of layers of face `f` of the other
  operand above vertex `v`: +1 where `f` faces up (`n·UP > 0`), −1 where
  it faces down, 0 where the ray from `v` along `UP` misses it. A flat
  triangle along `UP` (its projection a line) gives 0. For curved patches,
  hits where the patch folds over in projection cancel in pairs, so the
  signed count only changes where the vertex's projection crosses the
  patch's boundary or the vertex passes through it, as for flat ones.
- `s11(e, g)` (`Cross11`, for an edge `e` of `A` and `g` of `B`, each in
  its own direction): how their projections cross, as the two sums the
  counting reads. `a_under` sums over the crossings where `e` passes
  under `g` the sign `σ` of `det[g, e, UP]` (+1 where `e` crosses `g`
  from its right to its left, seen from `+UP`); `b_under` sums `−σ` (`g`
  crossing `e` from its right to its left) over those where `g` passes
  under `e`. Flat edges cross once at most, so one of them is ±1 or both
  0; projected conics can cross up to four times, and only the sums
  matter.
- `crossings(side, e, f, x)`: the crossings of edge `e` through face `f`
  whose signed number the counting gives as `x` (below): each one's sign
  and where along the edge it is (a position only, never a decision).
  Their signs add up to `x` whatever a search finds: the count wins.
  `searches(side, e, f)` says whether the pair is asked with `x = 0` too
  (an edge that may pass through a face and back: a curved edge, or a
  curved face).
- `order(side, e, c1, c2)`: the order of two of an edge's crossings along
  it.

Each is asked once per pair and stored in a sorted table (`count::Table`,
by `(vertex, face)` or `(edge of A, edge of B)`), which everything that
needs it reads. An edge's direction is that of its lower halfedge.
`Flat` (below) implements them exactly; `Curved` (see "Curved
primitives") for operands with curved patches.

### Exact predicates and the perturbation (`boolean/flat.rs`, `boolean/exact.rs`)

For flat operands the primitives are exact: signs of `3 × 3`
determinants and plane sides of the input coordinates. Ties, which CAD
geometry makes on purpose (flush faces, a vertex on a face, edges
meeting), are broken by **symbolic perturbation**: every vertex of `A`
moves by `ε·s·n_v + ε²·T2 + ε³·T3` for an infinitely small `ε`, with
`n_v` its direction out of `A`, `s = +1` for a union (`A` grows: flush faces overlap and
merge) and −1 for a difference or intersection (`A` shrinks: flush faces
cut cleanly), and `T2`, `T3` fixed generic translations for what the first
order leaves tied. `B` stays. This is Manifold's "expand P" rule made
into a real configuration: the perturbed operands are a genuine
arrangement in general position, so every decision is true of it and
the counting identities hold without exception.

`n_v` leaves by every triangle round the vertex (it is on the outer side
of each one's plane) wherever such a direction exists, so every face of
`A` moves outwards (or inwards) as a whole and flush faces part cleanly:
the normalized sum of the triangles' unit normals when it does, else the
axis of the smallest cone round their normals (`patch::smallest_cone`,
for up to 16 different normals). The sum alone fails at thin corners
(two faces nearly folded onto each other) and left slivers of zero
thickness where such a corner's face lay flush on the other operand.
At a saddle (faces round the vertex facing opposite ways) no direction
leaves by all, and the sum is kept: some face there moves the wrong way
and a flush contact leaves a sheet of zero thickness folded onto the
surface, which the clean-up takes out.

Each predicate is a polynomial in the points' coordinates, and so in
`ε`; its sign is that of the first coefficient that isn't zero. The
constant term is first evaluated in floating point with a running error
bound (`Approx`: each `+ − ×` adds its operands' bounds, their products
with the other's value, and half an ulp of the result, inflated a
little), which decides almost every sign without allocating. Only when
the bound can't (a tie or a near one) is every coefficient worked out
exactly with floating-point expansions (`Exp`, Shewchuk's two-sum and
Dekker's two-product, no fused multiply-add) as polynomials (`Poly<Exp>`).
All of it is `+ − ×`, correctly rounded, so the signs are the same on
every platform. Dekker's product is exact only clear of underflow, so
coordinates and perturbation components below `2⁻¹⁰⁰` are taken as zero
(`exact::FLUSH`): products of up to six such numbers (the degree of
`Between`) then stay exact. Without it, points a `1e-160` apart gave
signs that disagreed with the same determinant asked in another order.
A predicate zero in every power (only if `T2` and `T3`
happen to lie in the tie's degenerate directions) takes a fixed sign.

The predicates: `Orient` `det[q − p, r − p, UP]` (which side of `p → q`
the point `r` projects), `Height` `det[a − c, d − c, b − a]` (with `σ`,
which edge is above at a crossing: the point of `e` is `λ` above that of
`g`, `λ = det[a − c, g, e] / det[g, e, UP]`), `Reach` `(t0 − x)·n` (the
side of a triangle's plane `x` is on, and the numerator of where a line
from `x` meets it), `Across` `(x1 − x0)·n` (its denominator) and
`Between` (which of two planes an edge meets first). A vertex is above
a face's plane along the ray when `(t0 − x)·n` has the sign of `n·UP`;
it projects into the triangle when it is on the interior side of all
three edges.

**Near ties are ties** (`exact::sign_tied`). The deciding predicates
(`Orient`, `Height`, `Reach`, and the ray tests' `Beside` and `Ahead`)
take a constant term within the **tie distance** (a 64th of the
resolution, `boolean::tie`) times the predicate's `scale` (how much it
changes per unit of distance from its tie: `|UP × (q − p)|` for
`Orient`, `|det[g, e, UP]| / |UP|` for `Height`, the triangle's normal's
length for `Reach`, `|RAY|·|(d − c)·ACROSS|` for `Ahead`) as zero, and go
on to the perturbation's powers. The scales make the tie one distance
for every predicate, the one the curved primitives use for heights (a
review found `Height`'s 32 times larger and `Orient`'s and `Ahead`'s
larger along steep or ray-wise edges). So a configuration within the tie
distance of a tie is decided as the tie it stands for: faces flush in
exact arithmetic but turned and moved, every coordinate rounded, merge
or part cleanly as the unmoved ones do (of 96 random flush grid boxes'
operations turned and moved, 94 now work, 71 did with exact signs), and
the exact predicates and the curved primitives, which decide heights
that close as ties too, see one configuration: a cylinder `1e-9` off a
box's face was beside it for the exact tests and on it for the
numerical ones, and the winding numbers disagreed (then `InsideOut`,
now `Inconsistent`). The
positions (`Across`, `Between`, `crossing`) stay exact. It is used for
flat operands too.

### Counting (`boolean/count.rs`)

1. **Broad phase**: a BVH over each operand's patch boxes; the pairs
   (triangle of `A`, triangle of `B`) whose boxes meet (`≤`, so touching
   boxes count: `A`'s boxes looked up in `B`'s BVH), counted against the
   budget before they are collected (`Bvh::hits_within`). The margin is
   the primitives' (`Primitives::margin`): 0 for exact ones, the
   resolution for curved ones, which take edges and patches as straight
   or planar within it and decide heights within a 64th of it as ties.
   So every pair such a decision touches is counted: with margin 0, a
   cylinder's seam vertex `1e-9` from another's wall (at the coarsest
   tolerance) was decided as on it by its ray, no pair of triangles met
   to carry a crossing out, and the whole cylinder came out inside the
   other. From the pairs the candidate edge–face pairs of each operand,
   sorted.
2. **Layer counts** for each end of a candidate edge against the face,
   and for the first and last vertex of each connected part of an
   operand against every face of the other that a ray up from it may
   meet (the ray's
   box, which runs to the top of the other operand's box, looked up in
   the other's BVH with `Bvh::hits_within`: `pairs_within` for any
   query box).
3. **Edge against edge** for each candidate edge against the edges of
   the candidate face.
4. **Crossings** by Manifold's identity: for an edge `e` of `A` from `a`
   to `b` and a face `f` of `B`,
   `x12(e, f) = s02(b, f) − s02(a, f) − Σ S(e, h)` over `f`'s edges `h`
   as `f` runs them, where `S(e, h) = σ` when `h` passes above `e`
   (`a_under`; and `−σ` for `h` running the other way). Walking along `e`, the signed
   number of layers of `f` above changes by one each time `e` passes
   through `f` (entering `B` through it: +1) and each time `e`'s shadow
   passes under an edge of `f`, crossing it from its left to its right
   taking one away whichever way `f` faces. `x21(g, f)` for an edge of
   `B` through a face of `A` is the same with `S'(g, k) = −σ` when `k`
   (of `A`) passes above `g` (`b_under`). Then `crossings` turns each
   pair's count into **crossing records** `(edge, face, i)`, `i`
   counting along the edge, each with its sign `x` and position `t`
   (`count::Crossing`): the new vertices. A flat triangle and a segment
   meet once at most, so for flat operands any count but −1, 0 and 1 is
   `Inconsistent`; a curved edge and patch may meet several times, and in
   and out again where the count is 0.
5. **Winding numbers**: at each part's first vertex the sum of its layer
   counts (every layer above, signed, is the winding number of the other
   solid round it), and from there along the edges, each changing it by
   its crossings; then every edge is checked to agree (`Inconsistent`
   otherwise, never seen), the part's last vertex's own ray to give what
   its edges carried there (`Inconsistent` otherwise: a near tie decided
   at the first vertex against the crossings would put the whole part on
   the wrong side), and every winding number to be 0 or 1
   (`Inconsistent` otherwise: the operands are solids, whose winding
   numbers `check` makes 0 or 1 everywhere, invariant 5, so any other
   number means decisions that don't fit together, such as side-by-side
   cylinders nearly tangent). The operands' orientation isn't tested
   here or anywhere in the boolean: every `Solid` passed `check`.

Consequences, from the counting alone: a vertex's winding number and
its edges' crossings agree, and for every pair of faces (`p` of `A`, `q`
of `B`) `Σ x12(e, q)` over `p`'s edges equals `Σ x21(h, p)` over `q`'s
edges (each as its face runs it): the difference is minus the number of
signed crossings of the two projected boundaries, which is zero for two
closed curves. So each face pair's cut has as many ends going in as out;
for flat triangles, exactly one of each or none.

### Curved primitives (`boolean/curved.rs`, `boolean/curved/`)

With a curved patch in either operand, `Curved` gives the primitives:
numerical where what they are about is curved, exact (through `Flat`'s
predicates, `A` perturbed the same way) where it is straight or flat. An
edge is **straight** when its control point is within the resolution of
the line through its ends (`hull::straight`), and is then taken as that
segment; a patch is **flat** when its three edges are, and taken as its
corner triangle; a patch is **planar** when its control points are within
the resolution of its corners' plane (caps with curved edges are), and
which side of it a point is on is then decided exactly against that
plane. Each is judged once per edge or patch, so every primitive sees the
same.

**Why the shadow crossings are derived.** The counting's identities (a
face pair's ends balance; windings agree along edges) hold whatever the
values of `s02` and of the split of a crossing between `a_under` and
`b_under`, as long as for every pair of faces the signed number of
crossings of their projected boundaries is zero, as it is for two closed
curves. Crossings of curved edges found pair by pair can't promise it: one
near a shared vertex may be counted by both edges there, or neither. So,
as Manifold derives its edge crossings from shared vertex decisions, the
signed number of crossings of the shadows of `e` (from `a` to `b`, of
`A`) and `h` (from `c` to `d`, of `B`) is derived from **ray tests**:

```text
I(e, h) = ρ(b, h) − ρ(a, h) − ρ⁻(c, e) + ρ⁻(d, e)
```

`ρ(v, h)` is the signed number of times `h`'s shadow crosses the ray from
`v` along `RAY = (3, −2, 0)` (square to `UP`, square to no axis), +1
where `h` crosses it going left (towards `ACROSS = UP × RAY = (64, 96,
−13)`); `ρ⁻` the same for the ray behind the vertex. Walking a point `x`
along `e`, `ρ(x, h)` changes by `σ` where `x` crosses `h`, by `+1` where
`x` crosses the ray behind `c` going left and by `−1` for the ray behind
`d`, whatever way `h` runs there; that is the formula. Summed over the
edges of a face `q` the end terms cancel and what is left is the change,
from `a` to `b`, of `ω(v, q) = Σ ρ(v, h)`, the winding number of `q`'s
shadow's boundary round `v`, which sums to zero round a face of `A`. So
the balance holds by construction, for any values of `ρ`. Each `ρ` is
about one vertex and one edge of the other operand: which side of the
ray's line an end of the edge is on is exact (`(c − v)·ACROSS` with `A`'s
vertices perturbed, never 0, and asked as the same polynomial whoever
asks, so antisymmetric); a straight edge is exact throughout; a curved
one's shadow crosses the line at the roots of a quadratic in Bernstein
form whose end coefficients take the exact signs (so the number of roots
has the right parity), and whether each is ahead of `v` is decided in
floating point.

**`s11`.** `I(e, g)` as above. For two straight edges its crossing (one
at most) is above or below by `Flat`'s exact `Height`. Otherwise the
crossings are solved for (`curved/arcs.rs`): the curvier shadow (by
`|det M| / (|H0|·|H1|·|H2|)` of its homogeneous control points) written
implicitly as `λ1² = 4·λ0·λ2`, `λ = adj(M)·X` with the adjugate's rows
`H1 × H2, H2 × H0, H0 × H1`, the other put in to give a quartic in its
parameter (a straight shadow is its line, and gives a quadratic), roots
isolated in Bernstein form (Descartes' rule, halving by de Casteljau,
bisection; `curved/bernstein.rs`), the other parameter back from the
ratios of `λ`, kept if in `[0, 1]`. Each crossing's height difference
splits it into `a_under` or `b_under`. If their `σ` don't add up to `I`,
the missing ones go by the heights where the two edges' ends come closest
to the other (where they must be).

**`s02(v, f)`.** A flat triangle: exact (`Flat`). A planar patch:
`ω(v, f)` where its plane is above `v` (exact), 0 where not. A curved
patch: `ω(v, f)` when `v` is below all its control points, 0 when above;
otherwise the points of `f` straight above and below `v` are solved for
(`solve::hits`: the parameter triangle split by blossoming, pieces whose
control points' shadows' box misses `v` dropped, Newton on the shadow's
two coordinates in pieces whose normals all lean one way along `UP`),
their facings made to add up to `ω(v, f)` (a shadow covers a point as
often as its boundary winds round it, counted by facing) by adding the
nearest found just outside the triangle or dropping those inside nearest
its sides, and those above counted.

**Crossings.** A straight edge through a planar patch: at most one, at
`Flat`'s exact position on the corners' plane. Otherwise solved for
(`solve::edge_patch`: edge and triangle split together, pieces dropped by
their boxes and by a slab along the patch piece's normal, which a
tangency needs; Newton on `E(t) = P(u)`; once it finds a crossing in a
piece, the rest of the edge's piece either side of it, less a thousandth
of it round the crossing, is searched again: an edge running through a
wall a little inside its rim, in and out within one small piece, lost
the second crossing, the count 0 dropped the first, and the edge was
taken for not crossing at all), and made to add up to the count the same
way; ones the search didn't find go where it found the two meeting but
kept no crossing (a hit the count overrules, as where the edge leaves a
vertex on the patch's corner: nearest the patch), else where they came
closest (the middle of the smallest pieces the search looked at, which
had put such a crossing a sixteenth of the edge from the vertex).
Such pairs are searched with a count of 0 too, so an edge passing into a
face and back out gets both crossings. Crossings are ordered along an
edge by position, ties exactly (a straight edge through two planar
patches) or by face and index.

**Ties.** Heights within the tie distance are ties, decided as `A`'s
perturbation would (`A` moved by `ε·s·n_v + ε²·T2 + ε³·T3`), order by
order, each to first order (`first_sign`: the first order `δ`, then the
two translations, as the exact predicates take them; stopping at `δ`
decided a vertex on the line where two flush faces meet against the
exact predicates' second order): at an edge crossing, `e` (moved by `δ`
interpolated along it) rises over `g` by `δ·m / UP·m`, `m = g' × e'` the
two tangents' normal (the crossing slides along `g` as the shadows
shift); where the tangents are parallel (`|m|` within `1e-9` of theirs),
by `(T × δ)·(T × UP)`, `T` the tangent (the crossing slides along both);
a point of a patch with normal `n` there rises over a vertex of `A` by
`−n·δ / n·UP`, and over a vertex of `B` by `n·δ / n·UP` with `δ` the
patch's corners' interpolated. Where every order is 0, by `δ·UP`, as for
horizontal surfaces. So flush planar faces between a curved and a flat
operand behave as between flat ones. Vertex directions of curved
patches are their normals at the corner. Beyond heights:

- **Rays through a vertex.** Where a curved edge's shadow crosses the
  ray's line at the vertex itself as far as rounding tells (the vertex an
  end of the edge, or on it: a pin's rim vertex on the hole's rim),
  whether the crossing is ahead of or behind it is the perturbation's:
  the vertex moves by `δ` relative to the edge, the crossing to
  `ε·(δ_across·T_along / T_across − δ_along)` along the ray (`ray::
  tied_ahead`, order by order). Taken from the rounded point it was
  noise, and a pin's rim vertex wound −1 round the plate.
- **Shadows along each other.** Where one edge's shadow lies on the
  other's conic (the same arc in both operands, or a piece of it:
  `arcs::cross` gives `None` when the polynomial is zero within `1e-10`
  of its terms), every crossing the ray tests count goes the same way,
  by the heights where the two come nearest or, tied there, the
  parallel rule above. Solving that polynomial gave random roots.
- **Crossings at an end.** Crossings of the shadows solved at an end of
  either edge (within the tie of it: two edges from one vertex) are left
  to the count, which knows from the ray tests whether they are there;
  those the count has and the solve lacks go by the heights where the two
  come nearest (found by golden-section search between the samples, not
  the nearest sample, which put a vertex a sample's height above the
  edge it lay on).
- **Crossings at a patch's side.** A crossing of an edge through a patch
  within the tie of the patch's side (an edge of `A` lying in the plane
  where `B`'s wall meets its flush cap, say) is inside the patch or not
  as the perturbation moves it: the edge moved by `δ` relative to the
  patch moves the crossing by `δ − e'·(δ·n)/(e'·n)` on the surface, and
  it stays in if that heads across the side into the patch
  (`Curved::tie_inside`). Taken where rounding put it, a plate's
  diagonal crossed a boss's wall on neither side of its rim.
- **Edges in the surface.** An edge lying in the surface a patch's face
  claims (sampled within the resolution) isn't searched with a count of
  0: the perturbation takes it off to one side, and the search found
  crossings in and out where rounding had them.

### Pairs of faces (`boolean/pairs.rs`)

A pair (triangle `p` of `A`, `q` of `B`) is every pair whose boxes meet,
and any with ends. Its **ends** are the crossing records on its faces'
edges through the other face (sign seen from `A`); the counting leaves
as many of each sign. For flat operands they are two or none, joined into
one arc (else `Inconsistent`). With curved patches a pair is decided from
its ends only with a **certificate** that no closed loop hides in it: the
patches' normal cones apart (`NormalCone::apart`: no normal of one
parallel to one of the other, and a loop needs one), both planar, one
planar and the other on a **cylinder** (its face's quadric, unchanged
along a direction: `pairs::along`) with its normals within a half-space
(a plane cuts a cylinder in lines along it, which run out of the patch,
or in a conic round it, whose normals turn right round), or, with no
ends, their control hulls apart (GJK). Then no ends is no cut, two ends
are one arc, and more ends of two planar patches join in order along the
line their planes meet in (if they alternate). Without the cylinder
certificate a box's side against a boss's wall along it refined without
end. The argument holds for the exact plane and cylinder; the patches
are only within the resolution of them, so a loop the certificate
misses lies within about the resolution of both surfaces (a near
tangency),
and leaving it out moves the result by less than that.

Two patches on **one surface** (their faces claim quadrics and points
sampled on each lie on the other's within the resolution: a pin in a
hole cut by the same circle, cylinders of one radius stacked or
overlapping) don't meet at all once `A` is perturbed off it, so they have
no cut, and ends there are `Inconsistent`. Such pairs had no certificate
and were refined for a minute or two before running out of budget. The
test is within the resolution, not exact: surfaces a hair apart or
tilted pass too. Where those cross, the counting gives ends and the
pair is `Inconsistent`; a loop they hide lies within the resolution of
both, and leaving it out moves the result by less than that.

Any other pair is **refined**: both patches, where larger than the floor
(`MIN_SPLIT` resolutions across their control points' box), are split by
the red–green `Refiner` (so neighbours across split edges are too, flat
faces with straight inner edges), and everything is counted again, every
new vertex and edge with primitives of its own (`refined`: one refiner
per operand kept across rounds, so green pieces are never bisected
again; rounds up to `2·MAX_REFINE_DEPTH`, else `TooComplex`). A pair of
pieces both at the floor is decided by **fixed rules**: no certificate
means no loop, and the ends, by angle round their middle in the plane of
`p`'s corners (a pseudo-angle, `+ − ÷` only), join each + to a later −
as parentheses match, starting after the lowest running sum, so the
arcs don't cross. Pieces flat within the resolution certify each other
as planar, which is what ends refinement at a tangency long before the
floor (at pieces about `√(8·R·resolution)` across).

`refined` returns the operands as refined (the same surfaces, split; the
operands' vertices first, then refinement's), the tree of each operand's
red splits (`mesh::Node`: corners, patch, parent) and each refined
triangle's leaf in it, the counts and every pair's arcs (`Arc { tris,
plus, minus }`, by end vertex id): what cutting the curved faces starts
from.

### Cutting curved faces (`boolean/chain.rs`, `boolean/surface.rs`, `boolean/assemble/`)

With curved operands, `boolean` takes `pairs::refined`'s operands,
counts and arcs, builds the operands' tables again and `Curved`'s
primitives (for `order`), and assembles as for flat operands, with these
additions.

**Crossings on curved edges.** Each crossing's parameter is solved again
where the face crossed is a plane (the quadratic of the edge's conic
against it, exactly: the root in the edge nearest the count's position,
however far, so the vertex lies on the plane its triangles are tagged
with even where the search found no crossing and the position is only
where the two came closest, once `1e-4` off) or a quadric (Newton's
method on `F(C(t))`: the root within `1e-6` of the count's position,
else the position stays), when the edge is curved or the face isn't
planar (`surface::polish`). A root a rounding outside the edge
(`1e-9`) is at its end, and one within `1e-12` of an end is put there
exactly (`surface::at_end`), so a crossing at a vertex lying on the
other surface is at the vertex to the bit: a root just past the end
was dropped for the edge's other root, and the vertex went `1.6e-4` off
the plane it claimed. A vertex on an exactly straight edge (weight 1,
control point at the middle: `lined`) is interpolated as before; on any
other edge it is the conic's point from its blossom `B(t, t)`, and the
edge is split into pieces between its crossings by blossoming
(`[B(s,s), B(s,t), B(t,t)]`), each piece one record both faces beside
the edge read. An arc only straight within the resolution keeps its
curve (a straight piece of it was `3e-8` off a cylinder); its crossings
are found along the segment between its ends, so their parameters are
solved again on the conic (from its point nearest the segment's, then
against the face crossed): a nearly straight cut whose control point is
far from its chord's middle runs at another pace, and the segment's
parameter put a vertex `2e-3` along it from the plane it crossed.

**Chains** (`chain::chain`, per arc, through `par_map`): the cut from an
arc's `+` end to its `−` end as vertices and curves, and every vertex's
position in both patches' domains.

- **Two planar patches**: one straight edge, exact.
- **A plane and a quadric** (a planar patch, its face's `Plane` tag or
  its corners' plane, against a patch on a `Quadric` face): where an
  edge of either patch runs from one end of the arc to the other (at
  their very places) on the plane and the quadric, that edge whole (a
  cap's rim on the other's flush cap: `chain::along_edge`); else the
  conic the plane cuts the quadric in, exact (`surface::section`). The
  tangents at the ends are `n × ∇F`, the control point where they meet
  in the plane, and the weight from where the line from the chord's
  middle `M` to the control point `C` meets the quadric, at `σ` of the
  way (a quadratic): the conic's middle is `(M + w·C)/(1 + w)`, so `w =
  σ/(1 − σ)`. Which of the conic's two arcs between the ends is chosen by
  a guide point (the quadric patch's point halfway between the ends in
  its domain), and arcs that would turn by more than about 45°
  (`MAX_TURN_COS` 0.7), or need a weight under ½, are halved at the
  conic's point on that line (at most 6 times); a plane along a
  cylinder's rulings gives a straight edge. The arcs are kept only if
  their middles invert into the quadric patch.
- **Anything else** (quadric against quadric, `Free` faces): **traced**
  and **fitted** (`chain/trace.rs`). A point where the patches meet solves
  `P(u) = Q(v)` and lies on a given plane: four equations in `u0, u1, v0,
  v1`, Newton's method with Gaussian elimination (partial pivoting), at
  most 40 steps, accepted within `1e-11` of the patches' size. Marching
  starts at the `+` end along `n_P × n_Q` turned to leave the patch whose
  side the end is on inwards, steps `h` along the tangent and corrects on
  the plane square to it through the predicted point; a step is taken
  when the tangent turns by under about 20°, the point moved on and lies
  within half a step of the prediction, and within half a barycentric
  unit of both triangles, `h` growing ×1.5 on easy steps and halving on
  failures, down to `1e-9` of the size; it ends at the `−` end once that
  is within 1.5 steps ahead, and gives up past `MAX_TRACE_STEPS` (4096)
  steps or 16 times the patches' size of length. Fitting takes the whole
  run as one conic and halves it at its middle traced point (or at the
  curve's point on the chord's bisecting plane) until each conic is
  within a quarter of the fit tolerance of the curve at `¼`, `½` and `¾`
  (each projected onto the curve by the same Newton solve), turns by
  under about 45°, and has a weight within `2·W_MIN ..= W_MAX/2`: at most
  16 halvings. If tracing or fitting fails, the arc falls back to one
  conic along the end tangents, else a straight edge: only the geometry
  suffers.
- **The fitted conics' plane.** A conic's control points span its plane,
  and the hull rule between the two patches beside a curved edge wants
  one on either side of it. A planar face's cut lies in its plane (the
  other face leaves it: the cap-and-wall case). Between two curved faces
  the conic is put in the plane through its chord that **bisects the
  crease** the result has there: spanned by the tangent and the sum of
  the result's outward normals (`n_P + n_Q`, or `n_P − n_Q` where `B`'s
  faces turn over, for a difference): the two bands then lie on either
  side. Fitted in the curve's own osculating plane, crossing cylinders'
  bands failed the edge-neighbour rule and repair split them into a
  hundred thousand patches in vain.
- **The fitted conics' weight.** The triangles beside a cut take the
  patch's own curves as their other sides, and an edge weight far from
  the one those follow reparametrizes the triangle and pulls its inside
  off the surface (a weight of 0.69 on a short, nearly straight cut
  where the patch's own curve had 0.9998 put a thin band 0.007 off a
  surface of size 1). So the weight tried first is the geometric mean of
  the curved patches' own weights over the straight domain segments
  between the conic's ends, and only if that strays past the tolerance
  the one through the curve's point on the line from `M` to `C` (found
  on the plane holding that line and square to the conic's plane; the
  perpendicular bisector's point is the middle only of symmetric arcs).

The chains' vertices get ids after the crossings (arc by arc), and their
curves are records; a chain that isn't exact marks its edges fitted.

**Faces** are cut in one of three layouts (`face::Layout`):

- **Flat** patches (straight edges): the corner triangle's affine
  coordinates, as for flat operands (vertices on sides placed exactly,
  others projected and moved into the domain).
- **Planar** patches with curved edges (caps): the same affine
  coordinates of the plane, vertices on curved sides where they project
  (tagged with their side, not moved onto the corner triangle), inner
  edges straight, as refinement makes them on planes.
- **Curved** patches: the parameter domain. Vertices on sides at their
  parameters (a patch's side is its edge's conic in the same parameter),
  others from the chain's domain positions (Newton's inversions),
  moved into the domain, and onto a side they are within `1e-12` of (the
  inversion's rounding: a cut along a flush rim inverts to points on the
  side, each a rounding inside it or not, and ear clipping took a fan
  from the corner across them for proper, three corners on the rim, which
  folds; on the side they lie on one line and move inwards alike). Inner
  edges are the patch's own curves over the
  straight domain segments (blossoms `B(a, b)` normalized with `B(a,a).w`
  and `B(b,b).w`, exact), except on a quadric (below).

**Curved sides when triangulating.** The loops' curved sides (a cut's
curves, and on flat or planar patches pieces of the operand's curved
edges) are given to the triangulation as their tangents at their ends in
the layout (`Bends`; on a curved patch the 3D tangent mapped into the
domain by least squares). A triangle's corner between a curved side and
another side must be **open** (the curve's tangent strictly inside the
angle, by a sine of `1e-3`), else the curve bulges out of the triangle
and the patch folds. The ear clipping ranks ears: coincident corners,
proper, proper with a closed curved corner, zero area, anything else
(with curves, "zero area" relative: twice the area within `1e-9` of the
longest side's square, and with four vertices left an ear whose
remaining triangle is of zero area counts as one). Then, in up to four
rounds, a triangle with a closed corner between **two** curved sides
(two arcs of one smooth curve meeting at a vertex: any triangle with
both folds there) gets a Steiner point at its centroid and is split in
three (flips then improve them, keeping corners open); one with a closed
corner between a curved and a straight side asks for that curve to be
**split**, which no point inside mends. Sides of about zero length (a tie
left vertices at one place) aren't judged. A loop of two vertices (a lens
between two curves) asks for its curves to be split and is left out
until they are.

**Rounds** (`assemble`, at most `SPLIT_ROUNDS` = 6): every face is cut;
the curves asked for are halved — a chain's curve by `Chain::split` (an
exact curve by `split_half`, still exact; a fitted one at the curve's
point on the plane square to it through its middle, both halves fitted
again), an operand's curved edge by a **vertex added on it** at the
middle parameter of the piece asked for, which both faces beside the
edge get (a face kept whole with such a vertex on an edge is cut too, its
boundary triangulated again); and every face is cut again. A triangle
along a cut on a curved face also asks for the cut's curve beside it to
be halved when its patch strays from the face's patch by more than half
the fit tolerance at the 15 sample points (each inverted into the patch
from its domain position): a cut whose domain preimage bends far from
the straight domain segment leaves the band's inside off the surface.

**Cuts along a flush rim.** Where a plane meets a quadric exactly along
an operand's curved edge lying on both (a boss's rim on a plate's flush
cap), the band between the edge and the cut is of zero width, and the
cut's vertices (where the plate's cap triangles' edges cross the wall)
aren't the edge's. Halving curves never makes the two meet (every round
doubled them: a round took 15 s). So before the rounds, the edge gets a
vertex at each of the cut's vertices lying on it (`flush_extras`, as the
rounds add them), and again at the new vertices of every cut halved in
a round: the edge and the cut then come in the same pieces, lying on
each other, which the clean-up merges. (Without the halves' vertices, a
cylinder inside a larger one sharing its top kept a triangle with three
corners on the rim, which folds.)

**Exact bands on quadrics** (`face::exact_bands`). A rational quadratic
triangle lies on a quadric when its three sides are conics on it whose
planes meet in one point `O` on it; a cylinder's ruling lies in a plane
through any point, with the same (linear) curve whatever the point. The
patch's own curves all lie in planes through its common point, so its
blossom sub-triangles are exact; a plane cut's conic lies in its cutting
plane, which generally doesn't hold that point. So the inner edges are
chosen over a spanning tree of the face's triangles (across inner edges),
rooted at a triangle with a ruling or a fitted side (which needs nothing):
from the leaves in, each triangle whose other two sides are conics in
planes meeting at their shared vertex `V` takes `O` where the line those
planes meet in leaves the quadric again (`surface::second_point`) and
makes the edge to its parent the quadric's conic in the plane through the
edge's ends and `O` (`section`), kept only as one arc and only if neither
triangle on it then fails the fold check (a far `O` can reparametrize the
curve badly). `O` may be at infinity: a parabolic cylinder's curves (a
wall over a spline's piece, weight 1) all have planes along its axis,
which meets it nowhere else. So `second_point` gives the reciprocal of
the distance to `O`, the root nearest 0 of the quadratic in it (well
conditioned, 0 at infinity), and the plane's normal is `(x − V) × (y −
V)` times it plus `(y − x) ×` the line's direction. Solved for the
distance itself, the root was a quadratic's huge one whose leading term
was all rounding; the plane went through a point far off in a random
direction, and big triangles of the wall came out up to `1e-3` off it on
a `Free` copy of the face: unions and differences up to `0.1` off in
volume, each `Ok`. An unrefined cylinder strip cut by planes always has a
ruling in each region, so a cylinder through a box comes out exact to
rounding. A triangle on a `Quadric` face still off it by more than half
the resolution at the sample points (a fitted cut's band, the tree's
root) goes on a **copy of its face claiming no surface** (`Surface::Free`,
same name, so no feature edge), keeping the face tag check true.

**Diagonals along a cut.** Faces of `B` join no two vertices of one cut
by a diagonal (as no face joins two vertices on one domain side): the
face of `A` across may join them too, which would make two edges between
the same vertices.

**Ties** leave vertices at one place (a crossing at an edge's end, where
a vertex of one operand lies on a face of the other, as refinement's
midpoints often do). The clean-up collapses the zero-length edges; for it
to merge the two sides of a zero-width triangle, an inner edge whose
ends lie at the very places a boundary edge's do takes that edge's curve,
and a diagonal left by cutting an ear with two corners at one place, one
of its sides curved, takes that curve's tangents (and no flip moves it),
standing for it in the triangulation.

**Merging over-refined patches** (`assemble/merge.rs`). The pair
decisions split the operands (red–green) wherever they couldn't decide a
pair; pieces far from any cut come through whole. A node of the
refinement tree whose pieces all came through whole is restored as its
own patch, largest first, unless a vertex inside it (a midpoint the
refinement made) is still a corner of a triangle outside it (a
neighbour across its edge still split finer): then its children are
tried instead. A restored node is within one face, with its own corners
and edge records: exactly the operand's surface. A ball just inside a
slab, whose pairs were refined to rule out loops, comes back as the slab's
12 patches and the ball's 8. Demoting a candidate can bring another's
into the way, a round later, so the rounds run until none is demoted,
each looking at every triangle (flags by node and by vertex, a unit of
work each): on 48 000 refined triangles they took 14 s with sets, and
now take 20 ms.

### Assembly (`boolean/assemble.rs`)

- **What is kept**: union keeps what of each is outside the other
  (winding 0), intersection what is inside (1), `A − B` what of `A` is
  outside `B` and what of `B` is inside `A`, turned over (its plane tags
  too).
- **New vertices are records**: "edge `e` of `A` through face `f` of `B`"
  (the `x12` list, sorted by edge then face) and the same for `B`'s edges;
  ids after both operands' vertices. Positions come from `crossing`,
  on an exactly straight edge interpolated from the edge's nearer end
  (exactly the end at 0 and 1; on a curved one see "Cutting curved
  faces"),
  made non-decreasing along each edge in the order `order` gave, and
  put exactly on the crossed face where it lies in a plane square to an
  axis (so a result's flush faces stay flush when it is fed on): each
  coordinate that all six control points of the patch crossed share is
  set to theirs, which only takes off rounding (a debug assertion holds
  the move within the resolution). The corners alone won't do: a wall
  over an arc whose ends are level (a chord along an axis) has three
  corners at one height and bulges off it, and crossings put on the
  corners' plane landed off the cylinder, failing as folds or, on
  shallow arcs, coming out with the wrong volume. `crossing` is the
  limit of `num/den` as `ε → 0`: the constant terms' ratio in floating
  point only when both are known to a relative `1e-12`; else they are
  worked out exactly (a near tie: two tiny numbers that are all rounding
  put vertices off the result, with the wrong volume), and for an exact
  tie the ratio of the first powers of `ε` that aren't zero.
- **Edges**: each edge's crossings are ordered along it (by insertion,
  which can't fail: by their places where they are more than the
  resolution apart, else by `order`), and its pieces kept by the winding
  number running from its start: both faces beside it read the same
  pieces. Crossings of a solid's boundary go in and out in turn on any
  path, so where one would take the winding number out of `0..=1`, the
  next one with the sign wanted is brought forward if it is at the same
  place (`alternate`: a tie, whose order the positions can't give, as a
  refinement midpoint on the other operand's plane crossed by two of its
  faces at once), or the edge between the two grazes both faces'
  surfaces (within the resolution there and midway, and no more than
  `MIN_SPLIT` resolutions long: a tangency, where a crossing's place
  along the edge is as good as unknown: a plate's cap edge tangent to a
  boss's rim at the boss's own vertex had its in and out crossings a
  micrometre apart in the wrong order). A crossing in and the next out
  (or out and in) a grazing stretch apart go to one place, where the
  edge is nearest both surfaces (either, midway, or an end of the edge
  within reach), so the clean-up collapses the piece between; left
  apart they left triangles of zero width at the rim no split mends.
  Crossings apart in the wrong order, which would put a vertex off the
  face it crosses, are `Inconsistent`.
- **Cut edges**: each face pair's arcs (from `pairs`; for flat operands
  its two ends, joined), each along its chain (for two planar patches one
  straight edge), with signs seen from `A`
  (the crossing of an edge of `A` as the face of `A` runs it, and minus
  the crossing of an edge of `B` as the face of `B` runs it). Keeping
  the outside of `B`, a face of `A` runs its cut from the +1 end to the
  −1 end (it leaves its edge where the edge enters `B`); keeping the
  inside, the other way; faces of `B` the other way round from that. The
  two faces of a pair then run their shared cut edge opposite ways.
- **Faces**: a face with no cut is kept whole or dropped by its first
  corner's winding number. A cut face's kept halfedges (pieces of its
  edges and its cuts) leave each vertex once, so its loops follow from
  the topology alone. They are triangulated in the patch's parameter
  domain (corners `(0,0)`, `(1,0)`, `(0,1)`; vertices on a side placed by
  their parameter exactly on it and tagged with that side, interior ones
  solved from the plane and moved onto the side they lie on or beyond by
  rounding or a tie; coordinates below `2⁻⁶⁴` are zero, and points of
  the side `u + v = 1` sum to one exactly), then turned into triangles
  on the records.
  Faces are cut in parallel (`par_map`), the rest sequentially.
- The result's faces are `A`'s then `B`'s (turned over for a
  difference), then the copies claiming no surface, less those no
  triangle is on any more, so chained booleans don't pile up faces;
  halfedges pair up by vertex id in `MeshBuilder`, never by position,
  and every triangle side with a curve record gets it.

### Triangulating a face's loops (`boolean/triangulate.rs`)

The loops are right for the perturbed operands however close their
points are in fact: flush faces give loops of zero width whose points
coincide, and interior vertices may sit exactly on the domain's sides.
A constrained Delaunay triangulation (`spade`) merges coincident points,
so ear clipping is used: it always completes. Interior vertices (on no
side) are inside the face for the perturbed operands however close to a
side they are placed, so every orientation test takes them as moved
towards `(¼, ¼)` by an infinitely small fraction of the way
(`exact::orient2d_towards`): a hole touching the domain's side, or a
cut vertex a rounding beyond it, is then triangulated as the thin region
it is rather than folded over. Loops touching the
domain's sides are outer loops; the others are outer or holes by their
signed area. Each hole goes to the smallest outer loop around one of its
points and is bridged in from its rightmost vertex to the nearest vertex
it sees (inside the angle there, crossing no side); then ears are cut in
this order of preference: an ear with two corners at one position
(within `1e-9` in the layout, on a side or not: tied vertices whose
positions came by different roundings, and a cut's vertex a moment off
the side it lies on; so zero-width loops come apart into zero-width
triangles along zero-length sides), a proper triangle with no other vertex in or on it (the best
shaped one, for polygons up to 64 vertices; the first found beyond; a
vertex at one of its corners' positions, such as a bridge's other end,
only blocks it if one of its sides leaves into it or along its sides),
a zero-area ear with no vertex on it, then any (with curved sides a
proper ear whose curved corners aren't open ranks between the proper
and the zero-area ones: see "Cutting curved faces"). No diagonal joins
two vertices on one side of the domain (it would lie along the side, and
the patch across could add the same one), in a face of `B` two vertices
of one cut, or repeats an edge. Then diagonals are flipped towards the
Delaunay triangulation (the far corner inside the near triangle's
circle, the quadrilateral convex, the new diagonal allowed and the
curved corners open; at most 8 flips per triangle), which removes the
thin triangles greedy ear cutting leaves. With curved sides, Steiner
points follow (see "Cutting curved faces"), in no triangle of zero width
(corners on a line: a band between two curves lying on each other, which
no point or split mends; asking for splits there doubled the curves
every round).

The faces of a round are triangulated in parallel and count their
steps together (`triangulate::Meter`: a vertex tested against an ear, a
triangle looked at by the flips and mending, the sides looked at
bridging a hole, and 64 for each orientation floating point can't tell,
worked out exactly: some hundred times the work), up to what the budget
has left (16 steps a unit), past which every triangulation stops and the
round fails with `TooComplex`; what they took is spent after. Whether
they get past it depends only on the total, not on the order the threads
count in. The exact orientations are counted per thread (a face is
triangulated on one thread, start to end). A face whose vertices lie
along lines (cuts along straight edges) decides nearly every orientation
exactly, and ear clipping without proper ears is cubic: one of 105
vertices took 1.3 s a round, counted as 0.08 s of work before, and a
single round could have run for minutes before the budget stopped it.

### Clean-up (`boolean/cleanup.rs`)

The flat version of Manifold's degenerate clean-up, on the triangle soup
before the mesh is built, at most 64 rounds:

- **Collapse** edges no longer than an eighth of the resolution onto
  their lower vertex id (the operands' own vertices come first, so they
  stay where they are), when every vertex round the edge keeps one fan
  afterwards (the surface stays a manifold) and no proper triangle
  turns over. Two triangles the collapse makes the same but facing each
  other both go: a sheet of zero thickness folded onto the surface,
  which flush contacts at saddle vertices leave. A collapse that fails
  the check is undone. An edge up to four resolutions long with an end
  inside a plane face (every triangle round it on that face, every edge
  from it straight) is collapsed onto its other end too, the same way,
  as long as no triangle moved turns over or gets a closed curved
  corner: moving such a vertex within the plane leaves the surface as
  it is (these rounds, and again with the Delaunay flips below). A
  crossing a tie left a micrometre along a cap edge from where the cut
  passes (a cap edge tangent to a boss's rim) left a triangle of zero
  width there whose corner at the rim was closed.
- **Flip** the longest side of a triangle whose height over it is no more
  than an eighth of the resolution, or no more than four resolutions when
  the triangle across is on the same face (so every triangle stays on its
  face's surface) and the flip leaves nothing thinner. The far corner of a
  flat triangle lies on that side, so the two new triangles cover the one
  across exactly.
- **Drop** connected parts enclosing no volume (at most an eighth of the
  resolution times their area): what is left of flush faces meeting.
- A face and its copy claiming no surface (see "Exact bands on
  quadrics") are one face here (`Soup::sources`): a cut is only between
  two faces of different sources.

With curves (the soup's records by vertex pair): an edge with a curve
(its control point more than an eighth of the resolution off its chord)
is never collapsed, and only triangles of straight sides are flipped,
into a neighbour of straight sides or on a plane face, and only if the
curved corners of the two new triangles stay open (checked in 3D). A
collapse merges each gone triangle's two sides from its far corner: where
their curves differ, the one between two faces (a cut, which lies on
both) is kept, and if neither or both are, the collapse isn't made. The
curves of the other edges moved onto the kept vertex go with them; where
an edge from each end of the collapsed edge runs to one vertex (not
across a gone triangle), the two become one edge, and the collapse is
made only if their curves are one (else a plane face took a cylinder's
inner edge there, 0.02 off its plane, and repair trusted the tag). A
triangle a collapse gives another curve off its face's surface (a
fitted cut's, merged onto the face's own curve) goes on the face's copy
claiming no surface, as fitted bands do (`leave_surfaces`).

A triangle of zero height (no more than an eighth of the resolution) and
straight sides may also be flipped into a neighbour with curved sides:
the new side from its far corner, which lies on the neighbour's side, is
the neighbour's own curve from there to its far corner (the neighbour
bisected there by blossoming, `inner_curve`), so the two new triangles
are exactly its pieces, on its surface. Collinear triangles left on
curved faces failed the fold check.

A curved edge between two triangles in one plane (two plane faces of
one plane meeting along a curve: a pin filling its hole, united with the
plate) is flipped away when the two make a convex quadrilateral whose
curved corners stay open (`unbend`): the new triangles cover the same
region whatever the curve between them, and both go on the lower of the
two faces, which merge there. No plane through a curve between two
patches in one plane has either patch off it, so the hull rule can't
hold there, and repair split along it down to flat pieces: 114 000
patches for the filled plate, 36 now.

Last, **slivers on plane faces** go (`delaunay`, after the rounds above,
in rounds of its own): a straight side between two triangles of one
plane face, either of them a sliver (the sine of its narrowest angle
under `SLIVER` = 0.02), is flipped towards the Delaunay triangulation
(the two angles facing it add up to more than π) when both new
triangles are proper and their curved corners open. Each input triangle
is cut on its own, so a long thin one (a plate's cap between its far
corners and a hole) leaves slivers when a second hole's rim crosses it
at a glancing angle; at a far corner they were under `1e-7` radians
wide, which breaks the vertex rule of the hulls and no split mends. Of
160 operations drilling plates hole after hole, 148 worked before and
all do now; drilling a 20 × 20 box in a grid of 60 holes, 12 steps
failed before and 2 now.

Collapsing removes an edge and keeps a closed manifold; it never decides
that two separate vertices are one. What the clean-up can't mend fails the
final check.

### Results that aren't manifolds

Where the exact result isn't a manifold (two boxes touching along an
edge or at a corner, united; a box less a solid touching its skin from
inside at a point or along a line), the perturbation gives parts a zero
distance apart, and the result fails `check` with `Invalid`. The same
operands intersected, or subtracted the other way, work.

### Errors and budget

`KernelError::Boolean(BooleanError)`: `Inconsistent` (the decisions
don't fit together: with near ties taken as ties, flat operands too can,
rarely; also a winding number out of `0..=1`, see "Counting"),
`Degenerate` (a face's loops
couldn't be triangulated, or the triangles don't pair up). `TooComplex`
past the budget or `MAX_PATCHES`, `Invalid` when the result fails
`check` (a result with a shell facing the wrong way among the others is
`Invalid(InsideOut)`, never a wrong `Ok`). Work: the broad phase's
pairs and the rays' hits (counted
before collecting), one unit per stored primitive and per candidate
crossing, and 20 more for each sign or ratio a primitive or a crossing
worked out exactly (a tie's expansions in powers of the perturbation,
some ten microseconds: counted per thread by `exact::counted` round each
primitive, which shares no work out, and spent after each chunk of 1 024;
a flat torus of 18 432 patches against itself, every primitive a tie,
ran 16 s before running out, and stops in 1.6 s now), the square of each
edge's crossings (ordering them), a unit
per cut face and its triangulation's steps over 16 (the `Meter`, see
"Triangulating"; an exact orientation 4 units), the soup's size per
clean-up round, the triangles per round of merging, repair's own, and 5
units per patch of the result for the check that makes it a solid (about
2.7 µs a patch; `CHECK_WORK`), spent before it, plus 32 for each patch
whose volume the check integrated to tell which way the shells face
(about 17 µs a patch; `INTEGRATE_WORK`), which `Mesh::check_counted`
reports and `Solid::new_within` spends (extrude's check too), after the
check: a result can pass it and still be `TooComplex`. The operands
cost nothing for their orientation, which their own check settled. With curved patches also each edge–face search a unit per 4
pieces it looked at, at least 16: the 16 spent before it runs, the rest
after each chunk of 1 024 searches (a search running to its cap of
1 024 pieces, as where two surfaces lie along each other, is 256; at
about 300 pieces a search took some 70 µs), a
unit per pair decided, and per refinement split and piece, every round;
`MAX_TRACE_STEPS / 64` per arc not between two planar patches, a unit per
curve of the chains, and a unit per curve halved in the rounds of cutting
the faces (each of which counts its ear clipping again).

### Costs

Release, several threads: two flat tori of 36 864 patches each, crossing
each other: 0.13 s to the mesh before repair, 0.34 s with repair and the
check (their union is 70 784 patches). Ties go to the exact path, which
allocates; flush boxes are dominated by it but tiny.

Curved pair decisions, release: a cylinder through a box, crossing
cylinders, a hidden loop in the round octahedron, a saddle, each a few
rounds, 10–30 ms. Two cylinders tangent along a line refine along it to
pieces flat within the resolution: about 2.5 s at the default tolerance
(13 000 pieces after 10 rounds), since each round counts everything
again and the searches near the tangency run to their caps.

Curved booleans, release, several threads: a cylinder through a box, 4 ms
(64 patches); crossing cylinders, 20–26 ms (700–820 patches, most of them
the fitted bands at the default tolerance); the hidden loop in the round
octahedron, 14 ms; a pin through a plate's hole wall, 20 ms (308
patches); tangent cylinders, 0.1 s at the coarsest tolerance and 0.8 s at
the default. 200 random turned bars against boxes, the four operations
each: 3.7 s all told.

**Threads.** Release, one thread and seven (the budget's units in
brackets): two flat tori of 36 864 patches each united, 0.71 s and
0.45 s (0.72 million); a plate with 144 holes less a slab across them,
0.35 s and 0.15 s (0.34 million); the plate joined to a boss across 38
of them, 0.72 s and 0.28 s (1.9 million); crossing cylinders, 31 ms and
24 ms (62 000). The seeded suite, its tests one after another: 83 s and
37 s. So rayon gives 1.3 to 2.6 times, about 2.2 over the suite: the
counting's primitives, the searches, the chains, the faces' cuts and
repair's tests run in parallel, but rebuilding each refinement round's
tables (`Input::new`, the vertex normals, the refiner's pieces), the
BVHs, the clean-up, merging and the check's topology are sequential.

A unit of work is about 0.2 to 0.7 µs on one thread across these and
across booleans that fail, so `MAX_WORK` (about 4.2 million) lets the
heaviest of them through with room to spare and stops a failing one
within about two seconds on one thread.

**Whole-body costs.** Much of an operation's work is over everything
both operands hold, whatever the boolean touches: every refinement
round rebuilds and counts both operands, the clean-up looks at the whole
soup each round, repair tests every pair of the result near each other
and the check spends five units a patch. So a small hole drilled into a
plate of 6 588 patches costs about 0.15 million units (0.1 s), into
one of 17 596 about 0.38 million (0.24 s), and into a box already
drilled with 60 holes (5 156 patches) about 0.48 million: 0.21 million
in the pair decisions' rounds, 0.16 million in repair, 0.06 million in
the clean-up. At roughly 20 to 100 units a patch, a body past some
50 000 to 200 000 patches can take no boolean within `MAX_WORK`. On
the web the regen worker is one thread, and wasm runs slower than
native, so a boolean that fails at the budget holds the worker for
several seconds; drags queue behind it (latest wins, but a running
operation isn't stopped).

### Tests

Boxes against boxes both ways round, for the three operations, against
analytic volumes (every result passes `check`, its patches lie on their
faces and plane faces face the way their triangles do): overlapping at a
corner and askew, one inside, one through the other, crossing, apart;
flush: the same box, a pocket sharing one face's plane, boxes sharing
four, two or one face planes, a slab flush with two sides, one inside
at a corner; face to face over a whole face, part of one, offset,
standing on top, and a face larger than the other's; touching along an
edge and at a corner (union `Invalid`); a diamond prism with its four
long edges on four faces of the box (the box less it is four prisms
touching: `Invalid`), a shorter one inside, one with its side edges on
the box's top edges; octahedra with their middle vertices on the box's
top edges, touching the top face from below and from above with a
vertex, and poking through it; a box turned 45° through another;
40 random pairs of boxes in general position, turned and moved; flat
tori of 2 304 patches, one upright through the other's hole crossing its
tube on both sides, and one through a box, where `|A ∪ B| + |A ∩ B| =
|A| + |B|` and `|A − B| = |A| − |A ∩ B|`; results fed on as inputs
(steps joined flush, a hole, a half cut away, filled back in); face names
of both operands kept, and faces no triangle uses dropped; `touches`; empty operands; refusals (inside
out, out of budget); the same bits at 1 and 8 threads. Unit
tests: expansions against known values, the float filter never
contradicting the exact sign, `orient2d` near a line and far out,
triangulating a square with a hole, a concave loop, a zero-width loop and
a vertex landing on the domain's side (no diagonal along a side); with
curved sides, a point added where two arcs of one curve meet, and a curve
asked to be split where it closes a corner with a straight side.

Curved (`curved/tests.rs`, `pairs/tests.rs`): the ray-derived shadow
crossings against dense polylines of random curves, straight and curved
(and the solved crossings' parameters, signs and heights); straight edges'
exact rays against the numerical ones; the points of random patches above
a point adding up, by facing, to the winding number of their shadow's
boundary; edge crossings on both the edge and the patch, and a line
through a cylinder crossing it once each way where it should to `1e-12`;
picking crossings to fit a count; Bernstein roots. Pair decisions: a
cylinder through a box both ways round (two closed curves, every end on
both surfaces to `1e-9`, windings 0), a blind hole (one curve, the bar's
end inside), crossing cylinders (two curves on both cylinders), an arc
passing through a turned face and back (two crossings counted 0), a hidden
loop the counting alone can't see (a face cutting a small cap off a
round-octahedron patch: found by refinement, one curve on the plane), a
saddle cut above and below its saddle point (the four ends on its patch
joined by the side of the saddle point they are on, which the ends alone
don't say), tangent cylinders (decided, deterministic), `touches`, the
budget, joining ends round a pair, and the same bits at 1 and 8 threads.

Curved booleans (`curved_tests.rs`), all four operations both ways where
it matters, every result checked with its face tags, volumes against
analytic ones (or the identities `|A ∪ B| + |A ∩ B| = |A| + |B|`, `|A −
B| = |A| − |A ∩ B|` where there are none): a cylinder through a box, a
blind hole, a thin bar within one triangle of each face, and a bar turned
off every axis through a slab (every patch on its plane or cylinder to
`1e-12`, no fitted patch); crossing cylinders (volumes against a Simpson
integral within a tenth of the fit tolerance times the area, the cut's
vertices on both cylinders within a quarter of it, only the bands
fitted); a pin through a plate's hole wall (upright cylinders meeting in
lines, exact); a boss joined flush on a plate; a block through the
plate's hole, and one whose side runs exactly through a vertex of the
plate's caps (a tie); a round octahedron cut through its middle and with
a hidden loop; the saddle above and below its saddle point; tangent
cylinders (union not a manifold, the rest the operands); a plate drilled,
joined and drilled again, fed on; merging back a ball and slab refined
and not cut; the same bits at 1 and 8 threads; 24 random turned bars
against boxes, each result right or refused; found by fuzzing:
cylinders side by side `1e-9` apart at the coarsest tolerance (right or
refused), a box's face through a bar's refinement midpoints (its plane
tag true), and a tilted bar's arc crossing a plate's cap where the
search misses it (on the cap's plane). Walls over arcs whose ends are
level (a 10 × 10 square whose top side is the arc, extruded): 60°
concave and convex, 20°, and 20° in three pieces (the middle one's ends
level), on frames where the ends are level in `x`, `y` and `z`, against
a box across the arc and a half-space across the wall, all four
operations exact with volumes in closed form; 40 random boxes across 20°,
45° and 60° such walls, each result right by its closed-form volume or
refused, at least 85% going through (93% do; the rest are convex walls'
cap pieces folding); a circle of six arcs with two level walls against a
coaxial cylinder and a slab; and 0.5° and 2° convex arches against
boxes, whose unions and differences came out 0.011 off in volume when
crossings were put on the corners' plane. Walls over a parabola (a
spline's piece) on three frames, one tilted, against boxes across them,
each result right by its closed-form volume or refused (their bands' far
point at infinity along the axis). A box cut from a wall over a very
shallow hyperbola, cut again across its cap's nearly straight edge, on
an axis frame and a tilted one (the identities; the crossings on the
edge's conic, not at the segment's parameter). Unit tests: exact ellipse
arcs of a tilted plane through a cylinder, crossings solved exactly on a
plane and a cylinder, the second point of a line on a cylinder and at
infinity on a parabolic cylinder, tracing
crossing cylinders and fitting at two tolerances, inverting a point into
a patch.

Found by fuzzing, with regression tests: random boxes on a half grid
(flush faces, shared edges and corners everywhere) against the cells
they fill, alone and fed on in chains, where a result that is a manifold
must come out with its volume and one that isn't may only fail as
invalid; tetrahedra sharing a face askew to the axes, the same corners
in both; crossings where the perturbed edges cross (checked against the
operands moved by a small `ε` in floating point); the same boxes turned
and moved, whose results must have the right volume or fail; signs of
points `1e-160` apart asked in every order. Fuzzed without a test
(release, a minute each): octahedra and boxes on the half grid, and
extruded star polygons on three frames, by the volume identities; none
came out wrong. The rest that fail are mostly exact results that aren't
manifolds.

**The seeded suite** (`seeded_tests.rs`). Booleans of extruded and
primitive solids in the configurations CAD makes on purpose and at
random, each of a pair's four results (`A ∪ B`, `A ∩ B`, `A − B`,
`B − A`) right or failed, never wrong: a result passes `check` (as every
`Solid` does) and its face tags, the four keep the volume identities
within the fit tolerance and analytic volumes where known, and points
sampled round the operands (away from their surfaces) are inside the
result exactly when the operation says, by the winding numbers of the
operands' and the result's tessellations (the result's exactly 0 or
1), and every shell of the result faces the right way for where it
lies, by an oracle that shares nothing with `check`: shells found
again, each one's volume by quadrature, and the other shells' winding
number at one of its first corners clear of them by solid angles of
their tessellations (0 for a shell facing out, 1 for a void). The
volume identities can't see a whole shell of an operand classified
wrong, since the union loses what the intersection gains and the
difference keeps it turned over; the oracle can (a unit test gives it
hand-built wrong shells). About 180 results of the suite have several
shells. Failures are counted and each
test holds a floor on the share that works. Its tests: coaxial, stacked
flush, nested, crossing and across pairs on random frames, on a grid
and off it; parts built in chains of twenty (plates, bosses, slots,
rounded blocks, plates with holes on the sketch planes, joined, cut and
now and then intersected, each result fed on); solids turned and moved
at random against boxes and bars; cylinders side by side with gaps and
overlaps of `1e-9` to `1e-3` at two tolerances; plates drilled hole
after hole (in rows, or anywhere on a grid); pins in holes of their
own circle and cylinders of one radius stacked and overlapping; bosses
flush on plates; the same bits at 1 and 8 threads. In release it runs
in about 25 s (37 s one test after another, 83 s on one thread); debug
builds run one case of each. Unit tests for the step: near ties decided
as ties (`sign_tied`), crossings at one place put in turn (`alternate`),
shadows along each other told apart, crossings at an edge's end put at
it, curves and patches keeping the coordinates their control points
share, and extrudes' tops at `to` exactly. Found hunting bugs, with
tests: a boss whose rim runs tangent to a cap edge between eight
symmetric holes, bosses joined flush on drilled plates, a cylinder
inside a larger one sharing its top, an edge grazing a cylinder found
crossing twice, operands facing in told without integrating every
patch, and the exact signs of a triangulation and of the counting
charged.

Fuzzed without a test (release): the seeded suite's generators at
larger counts (about 2 000 operations: `related` pairs 14 % refused,
parts in chains 20 %, random turned solids 4 %, most of the rest
tangencies, below), turned flush grid boxes (900 operations, 875 right,
the rest refused), tangent cylinders at three tolerances and seven
offsets (564 of 882 right, the rest refused), and every result right by
the volume identities, face tags and sampled points, and the same bits
at 1 and 8 threads. Those counts came before the tie distance was made
one distance for every predicate, which took the tangent test from 74
to 72 of its 96 operations and left the others as they were.

### Known gaps

- **Tangencies leave cusps.** Where a plane or a cylinder touches a
  cylinder along a line (a boss tangent to a plate's edge, a slot's side
  on a hole), the exact result's faces meet in a corner of zero angle,
  which no patch holds (its corner would be degenerate): such results
  fail as `Invalid` (the fold rule), or, where splitting the thin slivers
  there converges, come out right with many patches (a cylinder inscribed
  in a square prism, united with it: over 100 000). Unions of solids
  touching along a line aren't manifolds and fail as `Invalid`, as boxes
  touching along an edge do. Most of the operations the seeded suite's
  generators refuse are these.
- **Coplanar faces meeting along a curve** (a flush boss the first
  operand of a union with the plate it stands in, both over one span)
  keep curved edges between patches in one plane, which the hull rule
  can't pass; the clean-up flips them away where the two triangles make
  a convex quadrilateral (a pin filling its hole, united: 36 patches),
  and where they don't, repair splits along the curve down to flat
  pieces (right, about 30 000 patches). With the plate as the first
  operand the union is the plate.
- **Curved cuts near arcs fail as invalid now and then**: a planar cap's
  triangle whose arc bulges out of it after the rounds, or a flat sliver
  along a cut next to a curve that no flip may take (the boolean's own
  cap triangles aren't fold-checked). None came out wrong. Boxes cut
  across a convex wall over an arc meet it most: 8 of 120 random box
  operations across 20°–60° walls fail (unions and differences, all on
  convex walls), and a box whose face runs along the arc's chord (inside
  the bulge) fails its union and difference. Plane-against-cylinder cuts
  that should be exact can be off by 1–4e-6 where a band triangle fell
  back to a copy claiming no surface; small boxes across walls over arcs
  (a tenth wide, z 2..7) up to `1.5e-5` in volume, the copies up to
  `1.3e-4` off the wall (within half the fit tolerance, which bounds
  only the triangles along a cut).
- **Fitted bands leave their face's claim**: triangles along a fitted
  cut on a quadric (quadric against quadric, a quadric against a free
  surface), and an exact band tree's root where no ruling frees it, go on
  a copy of the face claiming no surface; a later boolean then traces
  and fits where they are cut again instead of cutting exactly.
- **Fitted chains are dense**: each conic within a quarter of the fit
  tolerance and turning at most 45°, and bands straying past half of it
  halved, so crossing cylinders at the default tolerance come out with
  some 700 patches, most along the cut.
- **Ties are decided to first order in each power of the perturbation**,
  by a tie distance (a 64th of the resolution) and heights, positions
  and sides worked out in floating point near them: consistent in the
  flush, coaxial, stacked and tangent cases the suite tries, and where
  they aren't the operation fails (`Inconsistent`), never
  wrong. Near ties at about the tie distance itself (things a 64th of
  the resolution apart) decide one way or the other by rounding. Flat
  operands' exact predicates take near ties as ties too, so they can now
  (rarely) be `Inconsistent`.
- A tangency along a line reads as not touching (`touches` says false for
  two cylinders side by side): no crossing shows it, and the fixed rules
  take no certificate as no loop. Flat solids touching do meet.
- Each refinement round counts both operands again from scratch, and a
  search stops at 1 024 pieces (placing a crossing it didn't find where
  it found the two meeting, else where they came closest): pairs a
  certificate can't settle (two cylinders tangent or crossing at a
  slant) refine for many rounds, and
  parts built in long chains occasionally run out of budget there.
- **Coplanar faces facing each other, triangulated differently**: a
  folded sheet whose two sides don't share their triangles can't be
  collapsed away (seen once in about 3 600 chained grid-box booleans).
- **Flush faces after rounding**: flat solids flush in exact arithmetic
  but turned and moved now mostly work (94 of 96 turned grid boxes'
  operations); a few still fail as `Invalid` or `Inconsistent`.
- **Long cap triangles and cuts passing close to their sides**: a cap
  triangulated once (an extrude's, or a cut face's) keeps long thin
  triangles from far corners to rims. Drilling a second hole of the same
  size beside the first, in line with it, on a large plate (a 20 × 20
  box, holes 2.4 apart) leaves a band a few tenths of a millimetre wide
  between the new rim and such a triangle's side, 16 mm long: the
  triangles across it reach from the side's far ends to the rim, and
  those from either end whose line grazes the rim close a corner there
  that halving the rim never opens (a fold), or, with the rim split at
  its point nearest the side, are so thin their hulls come within the
  resolution (2 of 60 steps drilling such a box fail as `Invalid`). A
  small box cut at a corner of a plate with 400 holes crosses two long
  edges from the plate's corner 3 µm apart, with the same result. Mending
  it needs vertices on the long side, graded along the band (quality
  refinement of the caps): one vertex under the rim moved the grazing
  lines elsewhere, and failed more flush bosses and drilled plates than
  it mended.
- **Flush bosses on drilled plates**: of 150 random plates with two holes
  and a boss, each of the four operations, 15, 6, 5 and 13 fail (18, 7,
  10 and 13 before the fixes below), mostly where the boss is flush
  with the plate's bottom too or overlaps a hole: tangencies and the
  long cap triangles above. None came out wrong. Fixed: a boss whose rim
  runs along a cap edge between symmetric holes, tangent to it at a
  vertex both have (`Inconsistent`: crossings a micrometre apart in the
  wrong order, then zero-width triangles at the rim); a boss's wall cut
  along its flush rim getting a triangle with three corners on the rim
  (cut vertices inverted into the wall's domain a rounding either side
  of its side); a cap edge passing through a boss's rim a little inside
  it taken as not crossing (the search found one crossing of two).
- Triangles thinner than the resolution across two faces (a cut passing
  within a resolution or two of a vertex) aren't flipped, and fail the
  hull rules.
- Ear clipping is quadratic to cubic in a face's cut vertices; faces cut
  by thousands of edges run out of budget, now counted as they go.
- **Failing operations run past two seconds on one thread**: a unit of
  work in the counting of refinement rounds is about 0.8 µs on one
  thread (native), so an operation that runs out of budget while
  refining a tangency takes about 3.3 s; on the web's single worker,
  slower still. The steps that ran far past what they were charged are
  counted now (the triangulations' and the counting's exact signs, the
  patches the result's check integrates), so none runs unbounded; the price is that an
  operation full of ties runs out sooner (a flat torus of 9 216 patches
  united with itself, 4.2 s on one thread, is `TooComplex` now).
- Merging restores only whole nodes of the refinement tree with no finer
  neighbour: pieces next to a cut stay as refined.

## Bodies from the history (`varde-document`, `varde-regen`)

A document's `Body` is `{ id, name, visible, created_by: FeatureId }`: no
shape and no position. Its geometry is whatever the feature history gives
it when regenerated. Body and feature ids share one counter. Only an
extrude makes bodies: `Document::check` wants every body's `created_by`
to be an extrude whose operation is `NewBody` of that body, and every
`NewBody` body to be there (so no two extrudes make one body). The Add
cube command, its toolbar button and `CUBE_SIZE` are gone.

### The extrude feature (`crates/document/src/extrude.rs`)

`FeatureKind::Extrude(Extrude { sketch, regions, extent, flip,
operation })`:

- `sketch`: a sketch feature listed before it.
- `regions`: `1..=MAX_EXTRUDE_REGIONS` (256) `varde_sketch::RegionRef`s,
  each passing `RegionRef::check(MAX_COORD)`. They're made by
  `Profiles::reference` when picked; regen resolves them
  (`Profiles::resolve`) and merges them (`Profiles::merge`).
- `extent`: `OneSide(d)`, `Symmetric(d)` (the whole depth, half each
  side), `TwoSides(along, against)` or `ThroughAll`. Distances are
  `varde_expr::Value`s checked against `Extent::ask(design)`: a length
  from `MIN_LENGTH` (1 µm, as dimensions) to `MAX_COORD`, bare numbers in
  the design's units; two sides together at most `MAX_COORD`. Through all
  is only for `Cut`.
- `flip` swaps one side's direction and two sides' sides; symmetric and
  through all ignore it. `Extrude::span()` gives the `(from, to)` along
  the sketch plane's normal that `kernel::extrude` takes (`None` for
  through all, which regen works out from the bodies).
- `operation`: `NewBody(BodyId)`, or `Join`, `Cut`, `Intersect` of
  `Targets { excluded }`, the bodies taken out (sorted without repeats,
  each made by an earlier feature).

Commands:

- `AddExtrude { name, extrude }` (`Document::add_extrude` names it
  "Extrude N") adds it and hides its sketch; a `NewBody` extrude also adds
  "Body N" with the id after the feature's, replacing whatever id the
  command held (`BodyId::NEW` stands for it, an id no body has). One undo
  step.
- `SetExtrude { feature, extrude }` replaces it whole, the caller passing
  regions freshly referenced from the sketch as it is. A `NewBody` that
  stays one keeps its body; one that stops removes the body and drops it
  from other features' excluded lists; one that starts adds a body.
- `RemoveFeature` and `RemoveBody` apply `Document::removal(Removable)`:
  the feature (a body's maker, for a body) and every later feature using
  one removed (`FeatureKind::uses`: an extrude's sketch), in timeline
  order, and the bodies they make; the removed bodies are dropped from
  the rest's excluded lists. One undo step. The app asks `removal` before
  sending the command to show what goes.
- `SetTolerance(Tolerance)`: the document's fit tolerance
  (`Document::tolerance()`, stored as its `f64`, checked with
  `Tolerance::new`, default 1 µm). `SetUnits` pins extrude distances as
  it pins dimensions (`Value::pin_units`).

`Document::example()` is a 60 × 40 mm plate with a hole of radius 8 on
XY, extruded 10 mm as "Body 1", made through the commands. New designs
still start from `Document::default()`, empty: the example is for tests.

### Regeneration (`crates/regen`)

`varde_regen::evaluate(document, cache)` (`src/history.rs`) runs the
features in order and gives an `Evaluation`: each body's solid
(`BodySolid`, in the order they were made) and the features that failed,
with why (`failed`, in the document's order). A failing feature changes no
body, and the later ones still run.

- A **sketch** gives its `Profiles` (`Sketch::profiles`; too complex fails
  the extrudes using it).
- An **extrude** resolves its regions (`Profiles::resolve`; one gone is
  "region not found"), merges them (`Profiles::merge`), turns the loops
  into a kernel `Profile` (below), and calls `kernel::extrude` on the
  sketch plane's `Frame` (from `Plane::placement`), over `Extrude::span()`,
  with the document's tolerance and `Budget::DEFAULT`, faces named by
  `FeatureId::get()`: the tool solid. A `NewBody` body gets it. Through
  all's span is worked out first (`through_all`): the extent along the
  normal of the boxes (`bounds3`, the control points') of every body made
  before it, excluded or not (so taking one out or putting it back
  keeps the tool, and what was worked out with it), plus
  1 % of that extent and 1 mm at each end, clamped to `MAX_COORD`; none
  is "there's no body to go through".
- A **join, cut or intersect** asks `kernel::touches` of every body made
  before it and not excluded, in the order they were made, against the
  tool: those are the touched bodies and the targets
  (`Evaluation::touched`, per feature, also when it fails after finding
  them). Excluded bodies aren't asked about (the panel lists them from
  the session anyway), so taking out a body `touches` can't tell gets
  past it without spending the budget on it again at every change; a
  body it can't tell that isn't excluded fails the feature and is listed
  last, so the panel offers to take it out. No target is "it doesn't
  touch any body", or "it doesn't touch any body not taken out of it"
  when it excludes some. The excluded bodies' touch and boolean results
  from the request before are kept (`Cache::keep`) for putting them
  back. Each target is replaced by
  `kernel::boolean(body, tool, op)` with `Union`, `Difference` or
  `Intersection`, the body always first (a flush boss put first in a
  union came out right but with some 30,000 patches). Every target's
  result is worked out before any body changes, so one failing changes
  none. **Bodies never merge**: a join touching two bodies adds the
  tool to each, so they overlap, since bodies are the document's and a
  merged one would leave the other without geometry. Bodies not touched
  or excluded keep their solids; a body cut away whole, or intersected
  with only a flush face, is left with the empty solid (drawn as nothing,
  no box, touching nothing after), without an error: it's what was
  asked for, and the body stays listed in Objects.

**Error texts** (`src/message.rs`). What the Timeline's tooltip and the
panel show is worded for the user, not the kernel: an extrude's own
`KernelError` becomes "its regions are too complex to extrude…" for
`TooComplex`, "its regions have parts too thin or too close together to
extrude at this tolerance, as where curves touch tangentially" for
`Invalid`, and the profile errors say what's wrong with the outline
(touching or crossing itself, loops that don't nest, a cusp, a loop of
no area); a boolean's error names the body and what was being done
("joining it to Body 2 leaves no clean solid: they meet only along an
edge, at a point, or on tangent faces; move it to overlap more or to
clear it" for `Invalid`, which is what edge-touching unions and tangent
contacts give; "… can't be worked out: they meet on faces too nearly
flush or tangent to tell apart; move it a little" for `Inconsistent`;
"… is too complex to work out…" for `TooComplex`). Every message starts in lower case, the Timeline putting it
after the feature's name. Body names are looked up when the message is
made, not kept in the cache.

**Pieces to conics** (`src/profile.rs`, `profile(sketch, profiles, loops,
fit)`). Each piece becomes segments named by its curve's `Id::get()`, its
ends put at its vertices (`Profiles::vertices`), so loops close to the
bit:

- A line piece is `Conic2::line` between its vertices.
- A circle or arc piece (sweep `|to − from|`, counter-clockwise when `to >
  from`) is halved until each part is at most 90° (with `1e-9` slack; at
  most twice, four parts for a whole circle). The middle of a part from `a`
  to `b` about `c` is `c + r·d/|d|` with `d` the chord `b − a` turned a
  quarter clockwise (anticlockwise for a clockwise piece), or `−(a − c + b −
  c)` for sweeps over 270°, where the chord vanishes. Each part is exact,
  without `cos` or `sin` (`Conic2::arc_between`). `r` is the circle's radius (an arc's start's distance
  from its centre); the parts share their middles to the bit.
- A spline piece (`src/profile/fit.rs`) is the open `BSpline::piece`
  between its parameters, cut at its breaks into cubic Béziers from the
  points and derivatives there (each break evaluated once, so neighbours
  share it), reversed for a piece running backwards. Each Bézier is fitted
  by one conic along its end tangents (so the chain turns smoothly), its
  weight putting the conic's shoulder where the cubic crosses the line from
  the chord's middle to the control point, or else weight 1; accepted when
  the tangents meet ahead of both ends, turn by under 90°, the weight is
  within `0.25..=4`, and 15 samples of the cubic lie in the control
  triangle within half the fit tolerance of the conic (by its implicit
  form over its gradient, `λ1² = 4w²·λ0·λ2`). Otherwise a Bézier within a
  quarter of the tolerance of its chord becomes a straight conic, and any
  other is halved (at most 24 times; past that `ProfileError::Fit`). At
  most `MAX_PROFILE_SEGMENTS` segments are made (`TooManySegments`).
  Measured on a closed spline about 10 across: 5, 47 and 409 segments at
  fits of 0.1, 1e-3 and 1e-5 mm.

**Cache** (`src/cache.rs`). Every result is filed under a 128-bit key (two
SipHash runs, one salted, over the length-prefixed parts): a sketch's
profiles by its plane and sketch (postcard-encoded), whether it solves by
the sketch, an extrude's solid (or error) by its feature id,
the regions, the tolerance's bits, its span's bits and its sketch's key
(not the operation, the extent or the excluded bodies, so toggling those
finds the tool; the span stands for the extent and flip), whether a body touches a tool by the two solids' keys, a
boolean's result (or `KernelError`) by the operation and the two solids'
keys, which then keys the body's solid, and a body's mesh by its solid's
key and the tolerance. Editing an earlier extrude changes its body's key
and so reruns every boolean after it on that body. The regenerator keeps what the
request being answered and the one before used (`Cache::begin` drops the
rest), so an unrelated edit, or a draft dragged, reruns only what changed.
A join, cut or intersect also keeps (`Cache::keep`) whether the tool
touches each body it excludes and their boolean, if the request before
had them, so taking a body out and putting it back only draws it again.
The lane owns it: the native thread's closure, or the worker's `serve`.

**Drafts.** `Request::Regenerate` has `draft: Option<Draft { revision,
feature, extrude }>`: an extrude being set up (`feature: None`, applied as
`AddExtrude`, the body `BodyId::NEW`) or edited (`SetExtrude`), applied to
a copy of the document through an `Editor`, so its checks apply. The
answer carries `Drafted { revision, error, touched }`; a draft the
document refuses, or whose feature fails, is answered with the committed
model and its error. `touched` is the draft's touched bodies less the
excluded ones, `None` where the touch test didn't run (a new body, a draft
failing before its tool exists, one the document refuses) and `Some` of
an empty list where it touched nothing; there even when it fails after
finding them, with the body `touches` couldn't tell last, so the panel
can list a body to take out that makes it fail. `Response::Failed` carries
the draft's revision too.

**The answer.** `Response::Regenerated` adds `failed` and `bodies:
Vec<(BodyId, Aabb)>` (each body with a solid, shown or not, from
`Solid::bounds`). `tessellate(document, evaluation, cache)` draws the
visible bodies' solids at `Display::new(&document.tolerance())`, joined
by `RenderMesh::append`; a mesh past `RenderMesh`'s limits fails the
generation with the `MeshError`, as before. On the web the reply's head
carries `draft`, `failed` and the boxes as corner arrays, checked finite
and in order on receipt (`wire::Error::Bounds`); `MAX_HEAD_BYTES` is 64
MiB. The draft's touched bodies cross in the head as marks, unchecked.

**Gaps.** Every join, cut or intersect asks `touches` of every body
before it on each edit that changes the tool (cached otherwise; bodies
whose boxes are apart are answered at once).
`touches` says false for a tangency along a line, so a boss tangent to a
body only there is "it doesn't touch any body". An operation that runs
out of budget takes about 2–3.5 s on one native thread and holds the
single-threaded web worker longer, with drafts queued behind it (latest
wins, so only the newest waits). The cache keeps only what the last
request used (and excluded bodies' booleans): switching the operation
away and back, or an edit undone after two requests, reruns the
booleans.

## The extrude UI (`crates/view`, `crates/app`)

**The session** (`app/src/doc/extrude.rs`, `Doc::extrude`, an
`ExtrudeSession`) is started by the Extrude tool (`E`, `Look::StartExtrude`,
outside sketches, where `E` is Equal's; again, or `Esc`, cancels it) or by
editing an extrude (`Look::EditFeature` on one: a double-click in the
Timeline, or `Enter` with it selected). It never runs with a sketch
session, nor in a read-only document. It holds: the extrude edited, if
any; the source sketch (the one selected in the Timeline, the edited
extrude's, or else the one the first region picked is in, which un-picking
every region lets go of again); the profiles of the source, or before
there is one of every visible sketch with regions (each found once, and
again when its sketch changes); the regions picked, as indices and as
`Profiles::reference`s made as they're picked (a region too thin for a
reference can't be picked); the extent kind, the two distance fields
(text, last good `Value`, error, read with `Extent::ask`), flip, the
operation and the excluded bodies (kept from the edited extrude). When
the source sketch changes under it (undo), the picked regions are found
again by their references (`Profiles::resolve`); while its regions can't
be found none are picked, and the references wait for them. An edited extrude's
references that aren't found are counted (`missing`, shown in the panel)
and dropped: `SetExtrude` gets fresh references of what's picked. Editing
never changes the extrude's sketch, so `SetExtrude` doesn't hide one.
A replacement of the whole document (restoring recovered changes, or
undoing or redoing that: `Editor::lineage` changes from the one `Doc`
saw at its last sync) ends the session and its draft, as its ids
(the extrude edited, the source, the excluded bodies) may name other
things: OK would otherwise write the values read before over whatever
extrude the id names now. The Timeline's selection, which an extrude
being edited sets to it, is let go of across a replacement as well
(`Doc::prune`). A `Replace` equal to the document is no change
and keeps the session.

**The preview** is the session's extrude sent as the request's draft
(`Doc::request_model` after every edit and look): whole once a region is
picked and the extent's distances have read at least once, with
`NewBody(BodyId::NEW)` for a new body. See `agents/viewport.md` for how
`MeshFeed` numbers drafts and keeps the newest answer; the draft's error
(`Drafted.error`) shows in the panel while it's the latest's.

**Committing** (`Edit::CommitExtrude`: OK, `Enter` in a distance field or,
outside one, the screen's `Enter`) needs `Doc::extrude_ready`: the
document editable, **no sketch edits waiting on the solver**
(`Doc::proposing`), and the session ready (`ExtrudeSession::ready`): an
extrude whole, no distance refused, and passing the extrude's own check
against the document's design (`Extrude::check_own`, public for this:
two sides together over `MAX_COORD`, say). The own check's refusal shows
in the panel at once (`ExtrudeState.refused`, in place of the draft's
error). The solver wait is because edits made in a sketch just left are
committed only once the solver accepts them: an extrude committed before
would be of the sketch without them (its regions found again afterwards,
maybe another one than the preview showed) and would come before them in
the undo history. Once they're answered, `prune_extrude` finds the picked
regions again in the new sketch and the draft is asked for again, so
what OK commits is what was previewed; undo in the window drops them and
frees OK. So do the other ways the wait ends (tested): a rejected edit
(the picks stay), the sketch deleted (its edits dropped once answered,
the session gone with it), restoring recovered changes (drops them; one
that changes the document ends the session, above), a units change in
the window (set after the answers, then followed by the session); read-only in the window ends the session. A solver lane not
started yet keeps OK waiting, as saving waits. After `CHECKING` (100 ms) the panel says "Checking the
sketch…" (`ExtrudeState.checking`, from `Proposals::slow`) when
there's neither a refusal nor a draft error to show. OK, the screen's
`Enter` binding and the hint take `ExtrudeState.ready`, which is
`extrude_ready`; `commit_extrude` checks `extrude_ready` itself, since a
field's `Enter` sends `CommitExtrude` whatever the state. A draft whose
feature fails can still be committed; it shows red in the Timeline. It
applies `AddExtrude` (the document's name "Extrude N", adding the body
and hiding the sketch) or `SetExtrude`, one undo step, selects the new
extrude and ends the session; refused by the document (left to the
cross-references, which the session keeps valid, so no test reaches
it), the session stays and the edit error shows. `Esc` or Cancel drops
the session and its draft whatever its state, and the model is asked
for again without it.

**The panel** (`view/src/extrude.rs`) floats at the viewport's right: the
title and region count, the extents (Through all only while Cut is
chosen, else disabled with "Only a cut goes through all"; choosing
another operation while through all goes back to one side), the distance fields (the first is `VALUE_FIELD`, which
takes the focus as the session opens, all selected; `Esc` in it cancels),
Flip for one side and two sides, the operations, for Join, Cut and
Intersect a "Bodies" list with a checkbox per body (`ExtrudeTarget`: the
draft's touched bodies as the newest answer of the current run of drafts
that ran the touch test gave them, `MeshFeed::draft_touched`: kept while a
changed draft is on its way, and while one fails before its tool exists
or makes a new body; a run starts when a draft is asked for after none,
or for another feature, and lists nothing until its first such answer,
so a new session never shows another's bodies; then the excluded ones,
in the order they were made; ticked unless excluded;
`ExtrudeLook::Target` toggles, keeping the session's `excluded` sorted and
only taking bodies made before the extrude edited; bodies undone away
drop out, and aren't taken out again when redone: undo gives the ids
back, so a new edit may give theirs to other bodies), the refusal, the draft's error or "Checking the sketch…",
Cancel and OK. The handle and region picking are in `agents/viewport.md`.
Dragging a knob types its distance (one side past the plane flips; a knob
on the plane changes nothing) as the design's units format it. A knob
stops where its field would refuse the distance, or where the extrude's
own check would refuse the extrude and didn't before (two sides together
over `MAX_COORD`; one side and symmetric only stop where the field
would). Two sides typed over the limit already, a knob only moves back
towards it. If the design's units change while the session is open, each
distance's value is pinned as the document pins its own
(`Value::pin_units` with the units it was read in,
`ExtrudeSession::follow_units`, from `prune_extrude` as the change is
applied), so a bare "20" typed in millimetres becomes "20 mm" rather
than disagreeing with the document, and OK, checked against the
document's design, isn't blocked by the change; a refused text stays as
typed.

**The Timeline** (`view/src/panels.rs`) shows an extrude with the extrude
icon and its distances as the note, in the design's units
(`extent_note`: "10 mm", "10 mm symmetric", "10 mm + 5 mm", "Through
all"). A feature in the answer's `failed` (which `MeshFeed` keeps with
the model shown, `failed_features`) has its name in the danger colour
and tells why in a tooltip; a sketch that doesn't solve is marked as
before. Double-clicking an extrude opens its session.

**Deleting** (`app/src/doc/delete.rs`): `Edit::RemoveFeature` (`Delete`
on the Timeline's selection) and `Edit::RemoveBody` (Objects' bin) ask
`Document::removal` what goes. Joins, cuts and intersects don't depend on
the bodies they touch (they're found again when regenerating), so a
body's removal takes only its maker (and what uses that); a later join
left touching nothing fails in the Timeline. For a body the prompt asks
"Delete *Body N* and M features with it?", counting its maker. If that's one feature (a feature and its
own bodies, or a body and the feature making it) the command applies at
once. Otherwise the app keeps a `Deleting` (the target, the `Removal`,
the editor's generation) and the view shows `DeletePrompt` over the
whole screen, on the unsaved-changes prompt's scrim: "Delete *name* and
N features that depend on it?", the features in timeline order with
their icons, then the bodies, scrolling past about ten rows, Cancel
(`Look::CancelDelete`, also `Esc`: `Doc::dialog` tells the escape key
which prompt is up, the unsaved one first) and Delete
(`Edit::ConfirmDelete`, danger style, no `Enter`). While it's up
`Doc::keys` is `None`, so no shortcut acts behind it. Delete applies the
same command, so exactly the listed set goes, one undo step. The prompt
is dropped in `sync` once the generation moves (undo, recovery), and a
stale one is neither shown nor applied. A delete asked for while edits
wait on the solver waits behind them and asks when made, in its turn:
nothing behind it moves until it's answered, and Delete makes it at
once (`agents/sketch.md`, Proposals). Confirmed while edits wait
otherwise, the delete waits behind them too, and is made then if the
same set goes, else asks again (`Doc::remove_now`). A read-only
document asks nothing and deletes nothing.

**Tolerance** (`view/src/toolbar.rs`): the file menu has a "Tolerance"
heading under Units with 0.1 µm, 1 µm and 10 µm, each sending
`Edit::SetTolerance` (`Command::SetTolerance`, one undo step, which
regenerates everything as the cache keys hold the tolerance). A value
from a file that isn't one of those shows as a fourth, unticked row,
named in µm, or exactly in mm where its rounded name would be one of the
offered ones' (`tolerance_choices`).

## Limits, budgets and errors (`src/lib.rs`, `src/budget.rs`, `src/error.rs`)

| constant | value | why |
|---|---|---|
| `MAX_PATCHES` | `1 << 22` | patches in a mesh; ids and counts fit a `u32` |
| `MAX_REFINE_DEPTH` | 24 | red splits from an input patch: `2^24` times smaller |
| `MAX_TRACE_STEPS` | 4096 | steps tracing one cut of a boolean; past them the cut falls back to a simpler curve |
| `SPLIT_ROUNDS` (boolean) | 6 | rounds of halving curves while cutting faces |
| `MAX_TURN_COS` (boolean) | 0.7 | the most a cut's conic turns (about 45°) |
| `MEND_ROUNDS` (boolean) | 4 | rounds of Steiner points in one face's triangulation |
| `MAX_WORK` | `1 << 22` | work units in one operation: about two seconds on one thread at most; the heaviest booleans measured take about half of it |
| `MIN_SPLIT` (repair) | 64 resolutions | the smallest piece repair splits, and the smallest profile segment an extrude halves |
| `MAX_PROFILE_SEGMENTS` | `1 << 16` | segments in a profile |
| `SIN_MIN` (extrude) | `1e-3` | cusps between segments; the narrowest cap patch corner |
| `MAX_SPLIT_DEPTH` (extrude) | 24 | how often a profile segment may be halved |
| `MAX_ROUNDS`, `MAX_CAP_DEPTH` (caps) | 32, 16 | rounds of mending the caps, and the halvings all told past which the caps halve a segment no more |

`Budget` is a limit (`Budget::new(work)`, at most `MAX_WORK`;
`Budget::DEFAULT`); an operation counts it down in a `Work` its steps share
(`repair_within` takes one), and running out is `TooComplex`. A unit is
about a patch or a pair of patches tested or split: repair measured about
0.5 µs a unit on one thread and 0.3 µs on seven. `KernelError` is
`TooComplex`, `Invalid(CheckError)` (the input breaks an invariant the
operation can't restore, or the result would), `Patch(PatchError)` (a
parameter, or a split outside the patch bounds),
`Profile(ProfileError)` (a profile that can't be extruded), and
`Boolean(BooleanError)` (see "Booleans").

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
  checked in every build by `check`, and on their own by `check_faces`.
- **`Tolerance` lives in the kernel** (fit bounds `1e-5 ..= 1e-1` mm,
  default `1e-3`), and `check` takes it for the hull margin.
- **Flat faces are split with straight inner edges** (red and green), not
  by the exact blossom: see "Refinement". The region is the same; the
  exact split's curved inner edges would lie in the face's plane with both
  pieces, where no plane through the edge separates them.
  The `Plane` tag is tested first on every input patch split or bisected
  this way (six control points against the resolution), and a wrong one
  fails with `Invalid(Face(t))` rather than be reshaped.
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
  `1 << 22` (about 4.2 million units, two seconds on one thread; it was
  `1 << 26` until the booleans' charges were measured). `MAX_TRACE_STEPS`
  is left to tracing.
- **The box and cylinder are `Mesh` constructors** (`Mesh::cuboid`,
  `Mesh::cylinder`) that take the feature id and the tolerance and always
  run `check`; `Solid::cuboid` and `Solid::cylinder` wrap them.
- **GJK's triangle and tetrahedron projections use cross and triple
  products** instead of the Gram matrix (a fix: see "Control hulls").
- **Stitching by the shorter diagonal**, not a pattern fixed by the
  counts alone: pairing points by parameter made triangles two steps wide
  along skewed cylinder walls. Both are crack-free, since the boundary
  points are shared either way.
- **Walls of one profile curve are one face for feature edges**: the
  segments of a curve (`Side { curve, segment }`) are separate faces, and
  drawing every face boundary would draw a circle's seams.
- **`Solid::bounds` is an `Option`** (`None` for the empty solid), and
  `Solid::bounds3` gives the `f64` box. `Display` holds only the fit
  tolerance; the other targets are its constants.
- **The chord target is kept on edges only**; inside a patch it can be
  about twice off at the corners of skewed patches (see "Solids and
  tessellation").
- **`Shape` is gone**, not kept as a test helper: `Solid::cuboid` and
  `Solid::cylinder` are the test solids, so `Shape`, `ShapeError` and
  `position_in_range` were removed from the kernel.
- **`Symmetric` is the whole depth**, half each side, and extrude
  distances are at least `MIN_LENGTH` (1 µm), as dimensions are, rather
  than only above zero.
- **`BodyId::NEW`** stands for the body an extrude not yet added makes;
  `AddExtrude` and `SetExtrude` give the body its id whatever the
  command held, so callers never guess ids. Bodies are named by the
  command ("Body N").
- **`Document::removal` takes a `Removable`** (`Feature` or `Body`) and
  returns `Removal { features, bodies }`; only an extrude's sketch counts
  as a use, and excluded lists are dropped from rather than cascading.
- **Caps are triangulated with `spade`** (constrained Delaunay, exact
  predicates, builds for wasm; the kernel's only new dependency), then
  mended in rounds (Steiner points at ears, halving curves at narrow
  corners and folds), rather than by ear-clipping. Only its insertion
  (with its hierarchy for point location), constraints and face walks
  are used; its refinement and its bulk loading, whose hash sets could
  be iterated in a random order, are not. The points go in in a fixed
  shuffled order, which bounds the flips.
- **Flat corners are mended on a second try** of the caps, when the
  first fails, rather than always: moved-in points can line up into
  slivers of their own, so doing it first lost some solids the plain
  caps give.
- **`extrude` takes a `Frame`** (origin and axes) and the extent as
  `from < to` along its normal; flipping and sides are the caller's.
- **`KernelError::Profile(ProfileError)`** carries a profile's own
  errors: `Empty`, `TooManySegments`, `Short`, `Segment`, `Degenerate`,
  `Open`, `Area`, `Cusp`, `Touching`, `Nesting`, `Triangulation`.
- **Curved walls are tagged with the conic's own cylinder**
  (`λ1² = 4w²λ0λ2`), not `Quadric::cylinder`: it holds for every conic
  weight, and for arcs it is the circular cylinder up to scale.
- **`Solid::area`** is new next to `Solid::volume`.
- **`Profiles::merge` returns the loops** (`Vec<Vec<Piece>>`, outer
  counter-clockwise, holes clockwise, unnested) rather than a merged
  `Region`, and takes region indices; `Piece` gained its vertices
  (`start`, `end`, into the new `Profiles::vertices`) so loops join up
  exactly. `Profiles::resolve` takes a slice of `RegionRef`s (the curve
  lists are worked out once) and `Profiles::reference` makes them.
  `MAX_REGION_CURVES` is twice `MAX_CURVES` (a curve can bound several
  loops), and `RegionRef::check` takes the coordinate limit, since the
  sketch crate doesn't know `MAX_COORD`.
- **The regeneration cache keeps what the last request used**, not two
  generations: a draft dragged or an edit only ever reuses the request
  before's results, plus the touch tests and booleans of bodies a join,
  cut or intersect takes out (`Cache::keep`), for putting them back. It also keeps meshes and whether sketches solve, and
  keys are 128-bit hashes of the values' postcard encodings, not their
  `Hash` (sketches hold `f64`s).
- **A failing draft is answered with the committed model** and the
  draft's error (`Drafted`), rather than the draft applied without its
  body. `touched` is in `Drafted`, not beside it on the response.
- **Join, cut and intersect work on each target body on its own**: a
  join never merges bodies, it adds the tool to every body it touches,
  so two bodies a join bridges overlap. The document has no way for a
  feature to consume a body (only `NewBody` makes one), and a merged
  body would leave the other listed without geometry; choosing a single
  body to join to is the user's way round it (take the others out).
- **Through all spans every earlier body**, excluded ones too, so taking
  one out or putting it back doesn't change the tool.
- **A join, cut or intersect that touches no target fails** ("it doesn't
  touch any body"), rather than doing nothing silently.
- **`FeatureId::get` and `Id::get`** give the numbers face names and wall
  curves carry.
- **A reference whose curves several regions share resolves only among
  them**: its point inside picks one of those regions, and a point in a
  region of other curves (the disc it was cut from moved away) finds
  none, "region not found", instead of that region being extruded
  without a word. With no region of its curves, the point decides as
  planned.
- **The Extrude tool is enabled while any sketch is visible** (or one is
  selected), not only one with regions: finding every visible sketch's
  profiles on each change was more than the button needs. A session
  without regions to pick says so in its panel.
- **Editing an extrude keeps its sketch**: the session only picks among
  that sketch's regions, so `SetExtrude` never moves an extrude to
  another sketch, and nothing needs hiding. References the edit can't
  find again are dropped on OK, not kept failing.
- **Booleans on flat patches first, with exact predicates**: the
  counting skeleton decides flat operands with exact signs and symbolic
  perturbation (`A`'s vertices moved along their normals, out for a
  union and in otherwise, then by two generic translations), rather than
  plain floating point: with rounded decisions, flush boxes (the commonest
  CAD boolean) came out as zero-thickness slivers instead of clean
  results.
- **`KernelError::Boolean(BooleanError)`** is new: `Inconsistent`,
  `Degenerate` (the planned `InsideOut` went once `check` covered
  orientation, below).
- **Winding numbers are propagated along edges** from one ray per
  connected part rather than summed for every vertex: the same numbers
  by the counting identity (and checked on every edge), without a ray
  through the whole other solid from every vertex (with the float filter
  not allocating, counting the big tori went from 0.65 s to 0.1 s).
- **Cut faces are ear-clipped, then flipped towards Delaunay**, not
  triangulated by `spade`: flush operands give loops whose points
  coincide, which a constrained Delaunay triangulation merges.
- **A clean-up collapses and flips degenerate triangles** before repair
  (repair only splits, and fails at once on flat pieces breaking the hull
  rules). Collapses keep the lower vertex id and the link condition; they
  remove edges of zero length and never identify separate vertices.
- **Operands face out by `check`, not by a test of the boolean's**: the
  booleans used to test each operand's volume sign (every patch a unit,
  32 per patch integrated) and fail `InsideOut`, which only caught a
  wholly inverted solid: a stray inverted shell, or an outward one nested
  in another, passed and gave wrong results. `check` now covers every
  shell's sign and nesting (invariant 5), so the booleans trust their
  operands; `BooleanError::InsideOut` is gone, a winding number out of
  `0..=1` in the counting is `Inconsistent` (the only way left to get
  one, as with nearly tangent side-by-side cylinders), and the result's
  check charges the patches it integrated. The seeded suite and a chain
  fuzzer (60 chains of 15 app-like steps) gave the same results before
  and after, bit for bit.
- **`Bvh::hits_within`** generalizes `pairs_within` to any query box.
- **Curved shadow crossings are derived from ray tests**, not taken from
  solving each pair of projected conics: `I(e, h) = ρ(b, h) − ρ(a, h) −
  ρ⁻(c, e) + ρ⁻(d, e)`, so every face pair's ends balance whatever the
  numerical answers (see "Curved primitives"). The quartic is still
  solved, for where the crossings are and which edge is above.
- **Layers above a vertex and edge crossings on curved patches are found
  by subdivision and Newton's method**, not by a closed-form 2×2
  quadratic system or a degree-8 polynomial, and then made to fit the
  counts (a vertex's layers add up to the patch's shadow's winding number
  round it; an edge's crossings to the counted `x`). Edge–face pairs that
  may pass in and out again are searched with a count of 0 too.
- **Pairs are refined by refining both whole operands and counting
  again**, not pair by pair: every new vertex and edge gets its own
  primitives through the same counting. Besides normal cones apart, two
  planar patches and (with no ends) hulls apart certify no loops, and
  pieces flat within the resolution count as planar, which stops
  refinement at tangencies well above the floor.
- **Ties in curved primitives** are heights within a 64th of the
  resolution, decided the way `A`'s perturbation would move things,
  order by order (its first order, then the two translations).
- **Near ties are ties for the exact predicates too** (`sign_tied`):
  within the same tie distance, a deciding predicate's constant term is
  taken as zero. The plan has flat operands decided exactly; with
  rounded coordinates (turned and moved flush solids) exact signs broke
  the perturbation's intent, and beside curved operands they disagreed
  with the curved primitives' ties.
- **Two patches on one quadric, and a plane against a cylinder patch
  whose normals keep within a half-space, are certificates** of no hidden
  loop, beside the plan's normal cones apart.
- **The clean-up flips slivers on plane faces towards Delaunay**, across
  the pieces of different input triangles, which cutting each triangle
  on its own can't do (see "Clean-up").
- **The clean-up merges coplanar faces along curves** where it can (a
  curved edge between two triangles in one plane is flipped away), and
  moves triangles a collapse gives an off-surface curve to their face's
  copy claiming no surface.
- **`touches` on curved solids runs the pair decisions too**, so a
  loop no edge crossing shows still counts.
- **Fitted cut conics lie in the plane bisecting the result's crease**
  (spanned by the cut's tangent and the sum of the result's outward
  normals), not the curve's osculating plane, and take the weight of the
  patches' own curves between their ends where that stays within the
  tolerance: otherwise the hull rule fails across the cut and the bands
  leave the surface (see "Cutting curved faces").
- **Exact bands on quadrics are chosen over a spanning tree** of each
  cut face's triangles, rooted at one with a ruling; the common-point
  construction gives each triangle its edge to its parent. Triangles it
  can't make exact, and fitted bands, go on a **copy of their face
  claiming no surface** (same name), so the face tags stay true claims.
- **Cut faces are cut in rounds**: curves whose triangles fold or stray
  are halved (cuts' chains, and operands' curved edges by a vertex added
  on them that both faces get), with Steiner points where two arcs of one
  curve meet, as the extrude's caps are mended; planar patches with curved
  edges are triangulated in their plane with straight inner edges, curved
  patches in their domain.
- **Crossing positions are solved again** on planes (exactly) and
  quadrics (Newton), and curved edges are split by blossoming, for the
  exact cuts.
- **Faces of `B` join no two vertices of one cut by a diagonal**, so the
  two faces of a pair can't both add the same edge.
- **The clean-up carries curves**: it never collapses a curved edge nor
  flips across a curve, and a collapse keeps the cut's curve where two
  sides merge.
- **Merging over-refined patches restores nodes of the pair decisions'
  refinement tree** whose pieces all came through whole; there is no
  merging of the result's own triangles (nor of repair's splits).
- **`MAX_TRACE_STEPS` is 4096** per cut, in `lib.rs`; a cut that runs out
  falls back to one conic along its ends' tangents, or a straight edge.
- **The kernel has its own trig module**, `varde_kernel::trig`, beside
  the sketch's `angle` module, rather than one helper shared by both: the
  sketch doesn't depend on the kernel, and both wrap the same `libm`
  functions, so they agree to the bit. A crate of its own for a dozen
  one-line wrappers, or the sketch depending on the kernel, wasn't worth
  it. New kernel code takes angles only from `trig`.
- **Orientation is invariant 5, before the face tags (now 6)**, run in
  every build and kept out of `check_embedding`, repair's check.
- **The nesting rays are perturbed, not retried**: the ray's start is
  moved symbolically (the booleans' `exact` numbers, a fixed `NUDGE`
  then `T2`, `T3`), so no edge or corner is ever grazed, rather than
  trying a list of tilted directions and refusing when all graze.
- **Corner volumes carry error bounds** (Shewchuk's `orient3d` bound per
  triangle, the sum's own), and are worked out exactly where those can't
  tell the sign. A cruder bound (`n·ε` times `|a|·|b|·|c|` summed) was
  too loose to tell the sign of a box 2e7 times as long as thick.
- **A tighter lune bound** (the booleans' operand test, which shared it,
  is gone since, see "Operands face out by `check`"): the
  prism over the convex hull of the control points' shadows on the
  triangle's plane, times two, rather than four times the largest
  offset times the square of the spread. It is never larger, and it
  leaves an extruded plate with 900 holes nothing to integrate.
- **No valid ring has a corner volume of the wrong sign**: four outer
  quarter arcs against a fine hole break the hull rules (the hole's
  wall crosses the outer arcs' hulls), and the extrude splits them until
  the corner volume is positive again. The thin ring's test checks that
  it is told by integrating instead.
- **The inverted-operand tests** of the booleans now assert `Solid::new`
  refuses the operand, since it can't be built any more.
