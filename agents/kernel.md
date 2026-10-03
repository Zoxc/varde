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
`extrude`, which sweeps a `Profile` into a solid, the swept strips
(`sweep`: exact patches on cones and quadrics of revolution, fitted
bands and caps), `revolve`, which turns a `Profile` about an axis into a
solid (not yet a feature), and `boolean` and
`touches` for solids of flat and curved patches (curved cuts exact where
planes meet planes or quadrics, traced and fitted elsewhere), and a
solid's `Topology` (its faces, edges and corners as users see them, and
resolving the names references keep to them). Documents
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
  `trig::sin` (see "Deterministic trigonometry"); `Conic::arc_between(center,
  r, a, b)` builds the same arc (under 180°) from its ends without them,
  by `+ − × ÷ √` only: with `m = a + b − 2·center`, `c = center + m·2r²/|m|²` and
  `w = |m|/2r`, in the plane or in space (the circle in the plane of
  `center`, `a` and `b`). Profiles from sketches are built with it.
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

### Swept strips (`src/sweep.rs`)

A strip runs between a **bottom** curve `a0 → a1` and a **top** `b0 →
b1`, joined by a **left** edge `a0 → b0` and a **right** one `a1 → b1`;
its patches are `(a0, a1, b1)` and `(a0, b1, b0)`, as `cylinder_strip`'s,
and the **diagonal** `a0 → b1` is the only curve a strip makes, so strips
beside each other share the other four (one record each).
`MeshBuilder::strip(a, b, patches, face)` adds the two triangles with all
five curves. The exactness argument is the cylinder's: three conic sides
on a quadric whose planes meet in one point `O` of it.

- **Cone rulings** (`cone_ruling(p, q, apex)`): straight, weight 1, the
  control point on the ruling at the **geometric mean** `√(dp·dq)` of the
  ends' distances from the apex, written symmetrically
  (`apex + ((p − apex)·√(dq/dp) + (q − apex)·√(dp/dq))/2`, so the ruling
  either way round has the same bits). The plane through `O` and a ruling
  cuts the cone in that ruling and the one through `O`, which meet at the
  apex, so the projection from `O` takes the ruling through the apex
  twice over: its distance from the apex along the ruling is a perfect
  square, `((1 − t)·√dp + t·√dq)²`, whatever `O` is. That is this conic,
  so the two patches beside a ruling (each with its own `O`) share one
  record exactly. A cylinder's rulings, the apex at infinity, have the
  control point at the midpoint; with linear rulings a quarter turn of a
  45° cone from 1 to 2 high is `3e-3` off it (test
  `linear_rulings_miss_the_cone`).
- **The diagonal** lies in the plane through `a0` and `b1` along the
  bottom's bisecting direction `m = c − (a0 + a1)/2` (on a parallel: square
  to the axis, through the arc's middle). It meets the bottom's plane in
  the line through `a0` along `m`, which cuts the bottom conic again at
  `a1'` (on a parallel, half a turn round from `a1`), and the top's plane in
  the line through `b1` along `m`, cutting the top at `b0'`. So the first
  patch's planes meet at `a1'` when the right edge's plane holds it (a
  cone's ruling: any plane; a meridian of a surface of revolution: its
  plane holds the axis and so `a1'`), the second's at `b0'`. The cylinder
  strip's sheared diagonal lies in this plane too.
- `cone_strip(bottom, top, apex)`: any cone over a conic (circular,
  oblique, elliptic; `top` the bottom scaled about the apex, as the caller
  promises): rulings as above, the diagonal the bottom projected from the
  apex onto the plane (homogeneously linear, so the bottom's homogeneous
  control points map to the diagonal's; its ends then set to `a0` and `b1`
  exactly). Any plane through `a0` and `b1` clear of the apex works
  (tested for random ones); the bisecting one is used.
- `revolution_strip(bottom, top, left, right, origin, axis)`: a quadric of
  revolution (sphere, ellipsoid, paraboloid, hyperboloids, and cones and
  cylinders) between two parallels (circular arcs square to the axis) and
  two meridians (plane sections through the axis; on a cone, geometric
  mean rulings). The diagonal is the conic in the plane through `a0`,
  `b1` and `a1'` that touches the surface at `a0` and `b1` (its tangent
  planes there spanned by the edges' tangents: control point where the
  two tangent lines in the plane meet) and passes through `a1'` (weight
  `|β1|/2√(β0·β2)` from `a1'`'s barycentric coordinates on the control
  triangle, which every point of the conic satisfies). `a1'` comes from
  the axis (`a1` turned half a turn about it): found again on the
  bottom's conic along `m` it was good only to about `ε/θ²` of the radius
  for a piece of angle `θ` (`m` is a sagitta), which tilted the plane and
  left sphere strips of `0.45°` `2e-13` relative off; from the axis they
  are within `1e-15`.

Refusals (`PatchError`): edges not meeting at the corners to the bit
(`Mismatch`); a straight bottom for `cone_strip` (a flat strip), an apex
too close to the diagonal's plane, a zero axis or `a1` on it, tangents
parallel or `a1'` on the arc itself (`Degenerate`). Poles and apexes (a
strip with a meridian ending on the axis) have no exact strips: no common
point exists there, so they are fitted caps (below).

Tests (`sweep/tests.rs`), half on axes through the origin (bounds
relative to the size) and half up to `1e3` out (relative to the
coordinates), pieces from 90° down to a third of a degree: sphere strips
within `1e-12` of their spheres (measured `7e-16`), cone strips by both
constructions for any diagonal plane (`4e-16`), oblique and elliptic
cones, ellipsoids, paraboloids and hyperboloids of one and two sheets,
every patch passing the fold check; rulings the same bits either way
round, and two cone strips side by side making the same ruling with both
neighbours on the cone; refusals. Closed solids of revolution built from
them (rings of 4 to 256 pieces, flat caps of arc-bounded sectors) pass
`check`, orientation and face tags included, and turned inside out are
refused: frustums widening and narrowing (by both constructions), sphere
zones of one to four bands, and a lathe profile of a cone, a cylinder, a
sphere and an ellipsoid on axes tilted and `3e5` out, with volume (and
area where it has a closed form) within `1e-10`; the same bits at 1 and 8
threads.

#### Fitted strips (`sweep/fit.rs`)

Where no strip is exact (a torus, another conic turned about an axis, and
later sweeps and blends) `fitted_strip(bottom, top, left, right, form)`
keeps the four given edges, which are exact where the surface allows
(parallels and meridians are circles on a torus), and fits the
**diagonal** to the face's `Form`: its homogeneous middle point `(w·c,
w)`, four numbers, by damped Gauss–Newton (Levenberg–Marquardt) on the
form's signed distance (`Form::signed`: exact for planes, cylinders,
cones, spheres and tori, first order for the conic forms, with the unit
direction it grows along) at a grid of 8 steps a side on both patches
(the points where the diagonal counts). Each patch point is `(A + k·h)`
homogeneously with `A` fixed and `k = 2·ui·uj` of the diagonal's
corners, so the Jacobian rows are `k/D·(n, −n·P)`, everything relative to
the strip's middle. It starts from the parabola through `a0`, `b1` and
the form's point nearest the strip's middle (four steps along the
normal), takes at most 40 steps of up to 12 tries each (the damping
times 8 after a failed try, a quarter after a good one), and stops once a
step gains under a millionth: the same input gives the same bits. It
settles in about five steps. Least squares, not minimax: the worst error
is what is then measured, and it came out below the planners' minimax
estimates (see the table below). Where an exact diagonal exists the fit
finds one (sphere strips within `1e-12` relative); a cone has one in
every plane through `a0` and `b1`, so the fit lands on some, whose
patches may fold on wide pieces (cones take their exact strips anyway).

**At a turn** (`fitted_strip_touching`, crate-internal): bands fit a
strip whose piece ends at a turn of its height (the meridian's tangent
square to the axis there, to `1e-9` of its length: `Lathe::touch`) with
the diagonal's control point in the ring's plane, so the diagonal leaves
that ring in it, as an exact one does (the surface touches the plane all
along the ring). The constraint is linear in the homogeneous middle
point, `g·h = 0` with `g = (n, −(p − o)·n)`: the start is projected onto
it and each damped step is the least one in it (a Lagrange multiplier,
`s − M⁻¹g·(g·s)/(g·M⁻¹g)`), and the final control point projected again
against rounding. Unconstrained, the fitted control point dipped under
the plane by up to the fit error, so the band was no lax side of the
plane rule against a face beyond the plane, and a face that the cylinder
can't part from it either (a wall straight down from a concave round's
bottom, on the cylinder over the ring) was repaired until the band's
pieces were within the resolution of the plane: a revolved groove at
`1e-2` came out with 55 040 patches, now 320. On tori split at their turns
the strips fit in half the pieces round the axis and twice as many
along, the same patches all told.

**Measuring** (`deviation(patch, form)`): the largest `Form::distance`
over the patch, found rather than sampled: a grid of 12 steps a side (91
points, coordinates whole steps so the ones on an edge are zero exactly),
then from the grid's four farthest peaks (points no grid neighbour beats,
so two separate maxima each get a climb rather than four starts round
one; the farthest other points fill in where there are fewer) a compass
search along the grid's six directions, its step starting at a grid step and halved 20 times when no
move gains (at most 400 steps a climb), staying on the triangle with its
edges. A fitted patch's error is smooth, zero on its exact edges and with
a few extrema the size of the patch, so the climbs reach its maxima to
about `2⁻²⁰` of a grid step: on 40 random torus strips it is never under
a grid of 300 steps a side (45 451 points) and never over it by more than
`0.1 %`. (With `1 − a − b` for the third coordinate, a grid point on the
diagonal had it at `−6e-17`, so the climb could never move along the
edge where the maximum was, and fell short of it by `0.05 %`.) It is the
maximum found, not a certified bound (a maximum no grid point leads to
would be missed; bounding the error between grid points from the
patch's derivatives is as large as the tolerance unless the grid is
several times finer), so bands and caps accept a measured error up to
half the fit tolerance less a 64th of it (`MEASURE_MARGIN`; no torus
count above changed). Checked on 24 000 patches of fitted tori (five
proportions, three tolerances), elliptic tori and sphere and cone caps,
and 1 200 random torus and elliptic strips, against grids of 200 to 400
steps a side plus 2 000 random points each: the dense maximum was never
above the measured one.

#### Lathes, fitted bands and caps (`sweep/lathe.rs`)

A `Lathe` is the stations of a surface of revolution: the line through
`origin` along a unit `axis`, a full turn or a part (`0 < sweep < 2π`),
in `pieces` equal pieces of at most 90° (up to `MAX_PIECES` = 4096).
Station `k` turns the right-hand way about the axis (from `x` towards
`axis × x`; revolve's frame, turning `x` towards `x × y`, passes `−y`),
its cosine and sine from `trig`, exact at whole quarter turns: for a full
turn where `4k` is a multiple of the pieces (`2π·11/44` rounds off a
quarter turn), for a part where the angle is within 8 roundings of one
(so a sweep given as 270° has them), never a whole turn.
`turned(p, k)` is `p` itself at station 0 (and a full turn's last) and
for a point on the axis; `parallel(p, k)` is the exact arc between two
stations (`arc_between` about the foot); `meridian(m, k)` turns a
meridian's ends and control point and keeps its weight, so strips either
side of a station share it to the bit; `halved()` doubles the pieces.

**Bands** (`fitted_band(lathe, meridian, form, tol, budget)`): the
meridian (drawn at station 0) in fitted strips within half the fit
tolerance. A piece is fitted at station 0 first; if it fits, at every
station (a parallel map), and every strip is measured and must be sound:
both patches pass the fold check and the edge rule between them at the
resolution. Otherwise it is halved, whichever way station 0 says gains
more: the piece (each half then on its own, so only where needed), or
round the axis, which changes every face of the lathe and so is the
caller's: `Ok(None)` asks for the band again on `lathe.halved()`. Where
neither halving gives a sound strip yet, the longer way (the parallel's
chord against the meridian's). At most 16 halvings of a piece; 256 work
units a strip (about a hundred microseconds; the `Budget`'s units are
about half a microsecond).

**Rings at turns.** A ring's parallel is the shared edge of the
strips either side of it, and the edge rule first takes the plane through
its control points, square to the axis: one side must clear it, the other
not cross it by more than the resolution. Where the meridian's tangent is
square to the axis (a turn of its height: a torus's top and bottom) the
surface touches that plane all along the parallel, so a ring there has
both strips under it (or one in it), and no plane through a curved edge
parts them (both hulls hold the edge's three control points). `check`
then parts them by the cylinder over the ring (see "Control hulls"): one
strip leaves the ring inwards, the other outwards. So rings at turns are
solids (test `rings_at_turns_are_solids` and the profiles below). Bands
still keep their own rings off turns, as it is cheaper: rings put at the
turns themselves pass too but cost up to half as many patches again (a
torus off its turns at `1e-4`: 3 072 against 2 048, and a repair at
`1e-1`), and a strip over the turn whose ends differ in height dips under
the plane of its higher ring, which the bands' plane rule refuses. A
piece whose height turns inside it (the roots in `[1e-2, 1 − 1e-2]` of the
derivative's numerator, a quadratic with Bernstein coefficients `w·(hc −
h0)`, `(h1 − h0)/2`, `w·(h1 − hc)`) is first cut where its far side comes
back to the height of the end nearer the turn's (bisection to the bit),
so the piece over the turn ends at one height (within a quarter of the
resolution) and lies on one side of both its rings' planes, while its
neighbours fall away from the turn and clear them. Halving such a piece
makes three: the middle one over the turn, from half way to the turn to
where it comes back to that height. A turn within `1e-2` of a piece's
end (`TURN_NEAR_END`, in the piece's parameter) is left to the ring
there, which the cylinder parts (one strip leaves it inwards, the other
outwards): balancing it would cut a piece about twice that long over the
turn, and slivers that thin (a quarter arc turning `1e-5` rad past its
end: `1e-5` of it) the band halved round the axis until `TooComplex`.
Measured on a tube split, a puck's round going over its top and an S
whose joint is past its turn, by `3e-7` to `0.1` rad, at fits `1e-2` to
`1e-4` on three frames up to `1e4` out: 55 of 495 refused before,
none after, volumes within the slack; `1e-3` passed them too, `1e-4`
not. Caps keep `1e-6` for the turns that bound them. A piece ending at
a turn fits its diagonal leaving the ring in its plane (see "At a turn"
under "Fitted strips"). Pieces are made from the meridian's
blossom between parameters (`Conic::piece`, which the boolean's kept
edge pieces use too), so neighbours share their end's bits. Revolve may
build profile vertices at turns (a flat face tangent to a round at its
top, two arcs meeting tangentially there, a fillet onto a plate) and
split full circles anywhere.

**Profiles with rings at turns** (`sweep/lathe/tests/turns.rs`), each
revolved on lathes from 4 pieces, arcs as fitted bands, lines along the
axis as exact cylinder strips, across it as discs or annuli (straight
diagonals, the lathe halved until they pass the fold check): a puck
`R 10` with an `r 2` round on top, and rounded top and bottom; tori `R
20, r 2` and `R 10, r 3` split at their outside, top, inside and bottom;
two arcs tangent at a top; an S (convex, then concave) turning at the
joint; a boss with a concave `r 2` fillet onto a plate; a lip (half a
torus on a cup's wall); a thin round (`R 50, r 0.5`); a rounded hole
edge; a torus split 0.3 rad off its turns; a round over its top in one
131° arc. At `1e-2`, `1e-3` and `1e-4` (and `1e-5` in release builds) on
the axis and two random frames up to `1e3` out, all pass `check` with no
repair, volume within the area times half the fit tolerance of Pappus
(`π∮ρ² dh`), area within `4·A·fit/2` over the smallest radius, refused
inside out; every triangle split once at `1e-2` still passes; the same
bits at 1 and 8 threads. Turns just past rings
(`rings_just_past_turns_are_solids`): the tube, puck and S above `1e-5`,
`1e-4` and `1e-3` rad past them, at `1e-2` and `1e-3` on two frames. Counts: the puck at `1e-3` 16 pieces round the
axis and 192 patches; the torus `R 20, r 2` at its turns 64 and 1 024 at
`1e-3`, 128 and 2 048 at `1e-4` (an ordinary torus's patches; the
diagonals held in the turns' planes take half the pieces round). The thin
round's random frames stop at `1e-4`: at `1e-5` its disc's 2 048 sectors,
50 long and 0.15 wide, fail the hull rule against the wall far out (the
plane through a ring arc of bulge `1.2e-4` tilts by about `1e-9` rad by
rounding, putting the disc's centre `5e-8` off it), which is no ring at
a turn and the same without the cylinder; revolve's discs are to be caps
from their rings, not fans of slivers.

**Caps** (`pole_cap(lathe, meridian, pole, form, tol, budget)`): the
triangles of two meridians from the pole and the parallel between them,
one per piece of the lathe. Two meridians meeting on the axis have no
common point on the surface (on a cone the exact rulings would have
their control points on the apex, a corner the fold check can't pass), so
the cap is fitted: its meridians are the meridian's own piece turned
(exact arcs on a sphere; a cone passes a straight `Conic::line`, linear
rulings), and the piece is halved toward the pole until every triangle
is within half the fit tolerance, passes the fold check and the edge rule
with its neighbour. Measured: a sphere cap of angle `δ` in sectors of `φ`
is about `R·δ²·φ²/64` off (within 25 %, test), a quarter each halving; a
cone's in proportion to the cap's length, a half each halving, and to the
ruling's control point's distance from the apex (with it at a fraction
`λ` of the ruling, `λ·L·f(φ)`; `λ = 0` is exact but fails the fold check).
So the cap alone fits: caps never ask for more pieces round the axis. The
rest of the meridian comes back in pieces whose rings are at most 16
times further out (in its parameter) than the one before, so the strips
the caller builds on them (exact sphere or cone strips, or fitted) don't
thin into slivers; the triangles go on a claim-free copy of the face.
Triangles are `(pole, b1, b0)` for a pole at the meridian's start and
`(a0, a1, pole)` at its end: the strips' layout with one side collapsed,
facing the way the strips beside them do.

A cap never reaches past half way to where the meridian's height first
turns (an apple's meridian dips below its pole before it widens; a
spindle torus's too): a cap over the turn lies on both sides of its
rim's plane and the strips beyond it on one, and the edge rule between
them was refused at coarse tolerances. Its rest's rings stay at or
below that point, and one last piece runs over the turn to the
meridian's end for the band to balance (test
`caps_stop_short_of_a_turn`). A cap whose rim has come within the
resolution of the axis stops with `TooComplex` (no triangles that small
pass the hull rules, and the parallel of a point on the axis is no arc:
`Lathe::parallel` refuses it). Part turns close with flat ends through
the axis, the spheres' caps and both ends meeting at the poles (test
`part_turns_are_closed_by_flat_ends`, with `Mesh::repair` where the
coarse caps' hulls cross the ends').

**Measured tori** (`R` major, `r` minor radius; the lathe starting at 4
pieces and the tube in four quarters from 45°, halved as above; the error
is the worst `deviation`):

| torus | fit | round × along | patches | worst | the plan's estimate |
|---|---|---|---|---|---|
| `R 20, r 2` | `1e-1` | 16 × 4 | 128 | `1.8e-2` | |
| | `1e-2` | 32 × 4 | 256 | `3.3e-3` | |
| | `1e-3` | 64 × 6 | 768 | `3.4e-4` | 32 × 16 strips, 1 024 patches (`3.6e-4`) |
| | `1e-4` | 128 × 10 | 2 560 | `4.0e-5` | 64 × 32, 4 096 (`3.6e-5`) |
| | `1e-5` | 512 × 10 | 10 240 | `2.5e-6` | |
| `R 10, r 3` | `1e-3` | 32 × 10 | 640 | `3.2e-4` | |
| | `1e-4` | 128 × 10 | 2 560 | `2.1e-5` | |
| `R 50, r 1` | `1e-3` | 128 × 4 | 1 024 | `4.7e-4` | |
| | `1e-4` | 256 × 6 | 3 072 | `4.9e-5` | |

On uniform grids this fit is `2.1e-4` off at 32 × 16 and `1.4e-5` at 64
× 32, against the estimate's `3.6e-4` and `3.6e-5`; halving both ways
divides the error by about 16 (the fourth power of the size), halving
one way by 4 to 5. All pass `check` with the volume within the area times
the worst error of `2π²Rr²` (off by `1e-3` to `6e-2`, the fitted surface
lying either side). Fitting the `1e-3` torus is a few hundred thousand
work units.

Tests (`sweep/lathe/tests.rs`, `mesh/form/tests.rs`): stations exact at
quarter turns (in 8 and 44 pieces, and a part turn of 270° in 3),
station 0 and points on the axis kept to the bit, part turns; bad lathes
and fitted strips refused, and a band reaching the axis; `deviation`
against dense grids, two separate maxima both climbed; fitted strips exact on spheres and cones; signed distances
against distances and their growth along the normal; the pole error
falling with the cap (sphere by four, cone by two, the sphere's against
`R·δ²·φ²/64`); spheres with both poles capped and cones with capped
apexes (exact strips between, a flat base) at three tolerances, tori
anywhere, elliptic tori (`Form::Revolved`), and tubes split at any angle
(the pieces over turns ending at one height) passing `check` with
volume within the area times half the fit tolerance and area within
`4·A·fit/2` over the smallest radius of curvature (exact for the volume
of exact faces), and refused turned inside out; the torus counts above
at `1e-2`, `1e-3` and `1e-4` pinned, with dense spot checks; rings at
turns (above); caps off the axis and bands past their budget refused; the
same bits at 1 and 8 threads.

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
`check_embedding` (repair's) stops after step 4. Where they fail as
`Invalid`, `new_repaired_within` and `Solid::finished` return a
`Failure` with what the error names, of the mesh that failed: the
pieces repair names, or the triangles the check does (see "Check
evidence" under "Limits, budgets and errors").

### Forms (`mesh/form.rs`)

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
    promised signs (test). Hunted (scratch): 2 million adversarial pairs
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
`count_pairs_up_to(ids, margin, keep, most)` is the count alone, which
fails nothing: chunk by chunk, each query stopping past `most`, it
returns the number of pairs if at most `most`, else `most + 1`, the same
at any thread count (the counting pass above is the same code). The
extrude's crowding gate uses it.

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
shared with the neighbours on other faces. A patch that is flat but tagged `Free` gets the
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
`BTreeMap`s they made repair half as slow again. That shell's repair
looks for a witness 224 times over its rounds (one pair per pair of input
triangles a round), 1 792 units of work; its instruction count (callgrind,
one thread, both builds with one codegen unit) rose 0.31% with the
witness, and a passing repair of a 262 144-patch flat torus 0.39% (with
the default codegen units both read about 1.4%, layout alone). Repair of
that shell uses 4 190 928 units of the 4 194 304 budget: valid shells
much finer than it run out.

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
the resolution; round octahedra touching, small ones too, cylinders side
by side at 30° and 17° within the resolution and a cylinder with a box
corner 0.9 resolutions off fail with `Invalid(Hull)` in under 1 000 units
of work, as does a shell thinner than the resolution (also moved by
`1e5`), where small cylinders 1.5 resolutions apart still reach
`MIN_SPLIT`; the box at 1.05 and the cylinders at 1.1 resolutions pass
and check; a small budget runs out, and bad topology and weights are
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
- **Patches curved both ways** (faces whose form is a sphere, torus,
  revolved conic, ellipsoid or unknown: not a plane, cylinder or cone,
  circular or not, which are straight along their rulings; a quadric
  form is a scaled sphere or cone, and the cone is the one written about
  its apex, with no linear or constant term) get a finer grid where `m`
  leaves a triangle more than the chord off the patch. `Plan::refine`
  measures in rounds: each patch still open is triangulated in `f64` at
  its level and measured (`level_error`: the patch at each triangle's
  middle, in parameters, and at the middle of each side that isn't an
  edge's own segment, each from the triangle's point at the same mix of
  its corners along the patch's normal there. The full distance would
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
  copies up to about 1.06). On today's solids the inner grid
  alone is within the chord: the refinement is driven by the ring's
  corners, where the diagonal from a patch corner to the inner grid's
  corner spans a step along both edges (about 2.5 chords at `m =
  max(counts)`); a finer grid brings that corner in. At the default
  tolerance: a ball of radius 2 goes from 4 304 to 6 720 triangles (worst
  2.55 to 0.99 chords, densely sampled along the patches' normals), a
  hollow ball 8 656 to 12 736, a part torus 5 628 to 6 396, a spindle
  torus's outside 3 200 to 6 016, a turned ellipse 7 382 to 12 566, the
  round octahedron 800 to 2 144; a full torus (major 10, minor 2) is
  within at 0.78 and keeps its 10 496. Over random revolves, their cuts
  and stretched copies the refinement adds about a tenth to the
  triangles, at most 3.7 times (a long thin lemon, whose quarter patches
  have few steps round it), and no inner grid came near 256 steps nor
  any edge past its 64 segments' chord: the chord is at least a
  thousandth of the solid's diagonal, which a conic within it meets in
  about two dozen segments, so only the turn rule reaches 64, where the
  chord error is far below the chord. Planes, cylinders, cones and
  extruded walls keep exactly their levels. The measuring about doubles
  the time to draw a round solid (tens of
milliseconds for these, release); a 400-hole plate is unchanged.
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
but at a corner of a skewed patch (a cylinder wall triangle, whose far
corner is round the arc) the inner grid's corner is two steps round from
the patch's, and the triangle there is two steps wide: on the test
cylinder the worst triangle's middle is 1.85 chords off the surface, and
densely sampled about 3 (4 on tall walls). Those patches are not
refined; a better ring (flipping the corner diagonals, choosing the
strips' diagonals by deviation) would mend them without more triangles,
and would also let the refinement of round patches stop sooner. Very skewed patches (a wall a hundredth of its arc
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
refused. Patches curved both ways (a ball, a hollow ball, a torus and a
part torus, a spindle torus's outside, a turned ellipse, the round
octahedron, at the default tolerance and a coarse one) are within 1.05
chords of their patches along the normals on a 12-step grid of every
triangle, their triangles tile each patch's parameter triangle
(positively oriented, areas summing to the domain's), and they are
watertight and weld into a `ManifoldMesh`, the same at 1 and 8 threads;
a cylinder, a drilled plate, a thin disc and a revolved tube keep exactly the levels
their counts give, and so do a cone and a tube scaled into quadric and
conic forms, while the scaled ball's patches are measured; a refined level counts what it makes; a part torus meets
its exact limits and fails one under each. Faces and edges: a box's 6 faces of 2 triangles are its 6
regions and its 12 edges its 12 chains; a cylinder's quarter walls are
one region and its rims two closed chains, closing on their first
vertex; two flush boxes joined are 6 regions and 12 chains, the top's
triangles of both boxes' faces one region; a flat torus's creases are
on no chain, all after the chains with one face twice; every face's
vertices lie on its region's form and every chain's on both of its
regions'; given the topology or not, the same mesh, at 1 and 8 threads.

### Manifold meshes for export (`src/manifold.rs`, `tessellate::weld`)

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

## Profiles and extrude (`src/profile.rs`, `src/extrude.rs`, `src/extrude/`)

| file | holds |
|---|---|
| `profile.rs` | `Profile`, `Loop`, `Segment`, `ProfileError`, `Profile::check`, signed areas |
| `extrude.rs` | `Frame`, `extrude`, building the mesh, the walls' surfaces |
| `extrude/chain.rs` | the segments the solid is built on: classifying, cusps, separating |
| `extrude/cap.rs` | the caps' triangulation and the rounds that mend it |
| `extrude/cap/quality.rs` | refinement of the caps for quality and for crowding |
| `mesh/shape.rs` | the angle bound and circumcentre the caps' refinement and the boolean's shape points share |
| `profile/tests.rs`, `extrude/tests.rs` | profile builders (exact arcs without trig, circles, rectangles), shapes with analytic volumes |

### Profiles

A `Profile` is loops of `Segment { conic: Conic2, curve: u64 }`: outer
loops counter-clockwise, holes clockwise, so the region is on the left of
every segment; the curve id names the wall. The kernel knows no sketch:
whoever builds a profile (regen) turns lines into `Conic2::line`, arcs
into exact conics of at most 90° and splines into fitted chains (in
runs of Bézier spans, see "Pieces to conics" below), and
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
share its face. Walls on one surface that meet (two collinear lines, a
circle's arcs) are one face: after repair the extrude runs
`Mesh::merge_faces` ("Structure"), so the second line's wall takes the
first's name, its own key an alias, and faces don't depend on whether a
boolean has touched the body. Caps are tagged with their planes, straight
walls with theirs, curved walls with the cylinder over their conic
(below). Their forms are the planes, and for curved walls
`Form::Cylinder` over a circle's arc (`circle_of`), `Form::ConicCylinder`
over another conic, along the normal. The steps:

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
   loop in material, a loop running the wrong way), naming the loop to
   blame: the walk keeps, for each face winding otherwise, the loop of
   the chord it crossed from a face winding 0 or 1 into such faces (a
   face reached across an edge whose two sides disagree names that
   edge's chord's loop; no face winding once names loop 0). Each triangle's patch
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
   - **Refinement for quality** (`cap/quality.rs`), on every try, in a
     round that has nothing to halve and no flat corners to place: before
     the ears' centroids, as its points break up ears too (a fine
     circle's ears all share one circumcircle, whose centre takes them
     all), where a centroid, as close to the loop as the ear is thin,
     made every triangle at it thin and refinement halve the chords near
     it over and over (a circle of 4 096 arcs ran out of budget that
     way). A triangle whose narrowest angle (between chords and inner
     edges) is under 5° is bad, unless exempt: its narrowest angle at a
     loop vertex whose region corner, between the tangents, is under 60°
     (the small-input-angle rule, which keeps Ruppert's refinement finite
     with corners down to `SIN_MIN`); its circumradius under `MIN_SPLIT`
     resolutions; or its shortest side a chord under twice that (graded
     towards a chord that can't be halved for long, the points ran away,
     as on circles cut into uneven arcs; exempting any side that short,
     inner edges too, lost the polygons a resolution off their chords,
     whose inner edges span a few chords). A run of refinement queues the
     bad triangles (a binary heap, smallest circumradius first, then
     circumcentre and corners: a total order) and takes each while the
     triangulation still has it (its spade face still has its corners: a
     face spade changes gets the new vertex as a corner). For its
     circumcentre, in order: if its way in from the triangle's centroid
     meets a segment's control hull (at margin 0: a thin ear's own
     centroid is within the resolution of its chords), or it lies in a
     chord's diametral circle, the lowest such segment is asked to be
     halved, unless a point that asked before in the run lies within
     half its circumradius (that halving may well take this triangle
     too, which the next round tells: without the rule the bad triangles
     along a chord each asked for their own, a plate of 4.75 × 2.75 with
     two holes getting six halvings rather than two); else it is inserted into the round's triangulation if it
     lies inside a region face and is more than `MIN_CLEAR` (4)
     resolutions from every hull (a concave one it comes nearer is asked
     to be halved instead) and every Steiner point, and the bad
     triangles around the new vertex join the queue. Region faces
     without a re-flood: every face an insertion makes has the new point
     as a corner and lies in the region with it, so a face is in the
     region if it was at the run's start (sorted corner triples, rotated
     to the lowest first) or has a corner the run added. A segment that
     can't be halved (under `MIN_SPLIT`, or at `MAX_CAP_DEPTH`) drops the
     point. When the queue is empty the halvings asked for are made,
     straight segments too (their halves stay straight, on the line and
     face of their side), each half halved again while a point that
     asked lies in its diametral circle (what runs would do one halving
     at a time as a long chord near a feature keeps being encroached
     on), and the next round refines again. Points added stay in the
     region whatever is halved with them: they keep clear of the hulls,
     which the halves lie in. A run that asks for no halving leaves the
     triangulation settled: refinement runs again only after a round of
     mending changes it, or a small cap's is made afresh. Runs that ask for anything count apart from the
     mending rounds, at most 64 (`MAX_QUALITY_ROUNDS`), after which the
     caps stand as they are. Fine polygons' fans and ears and a plate's
     fans out of a corner to its holes' common tangent points become
     graded triangles, which the walls and later cuts keep clear of.
   - **Crowded caps** (`crowded`), once a try that refines for
     quality, when nothing is left to mend: the region's triangles'
     boxes are counted in pairs within the resolution, up to
     `max(32·triangles, 65 536)`, through
     `Bvh::count_pairs_up_to` (nothing collected, nothing failing), and
     spent. Past that the caps are crowded, a fan or strip of long thin
     triangles whose pairs repair would count first and run out of
     budget on, with no second try after: a fan the exemptions leave
     (the chords of a 65 536-gon at fit 0.1 are too short to refine at).
     The plain caps' tries aren't counted: they make the caps they made
     before refinement. Refined for crowding, 6 of the 5 500 fuzzed
     profiles of step 5 (cut circles with a run of 1 000 fine straight
     pieces, as `a_run_of_fine_pieces_in_a_cut_circle_extrudes`, and
     sharply weighted splines) failed there that repair had mended
     unrefined, while 3 splines of 6 000 sharply weighted pieces that
     the plain caps ran out of budget on passed; counting the pairs at
     all would spend work the plain caps' own check needs. Crowded caps
     get a run of
     refinement with every triangle under 5° bad, no exemptions and no
     halving, a circumcentre inserted only if inside a region face, more
     than half its circumradius (and 4 resolutions) from every hull and
     4 resolutions from every Steiner point; then the round is mended
     again. A point its triangle sees (no way in is tested here) is at
     least the circumradius from the vertices it is joined to, so it
     makes no side shorter than the triangle's shortest (at 5° the
     circumradius is over 5.7 times that); one across a chord from its
     triangle keeps only 4 resolutions from the Steiner points there.
     Every point keeps that from the others and from the hulls, so a
     run ends without the exemptions, short of the budget. Refined caps have
     about 6 pairs a triangle; no cap measured so far but the 65 536-gon
     is crowded.
   The first round triangulates the chain and the Steiner points; the
   triangulation is kept (`cap::Live`, with spade's vertices mapped to
   ours both ways), and each round after adds what the one before asked
   for: Steiner points inserted, and a halved segment's chord constraint
   removed (spade's `remove_constraint_edge`, which restores Delaunay
   there), its pieces' inner vertices inserted and their chords made
   constraints. The pieces lie in the segment's control hull, which every
   other segment and every Steiner point keeps clear of, so their chords
   cross nothing; a chord that doesn't come out a constraint is
   `Triangulation`. Our numbers are rebuilt after a halving (the chain's
   vertices in order, then the Steiner points), and the region's
   winding flooded each round. Without four points on a circle that is
   the triangulation made afresh; with them (fine regular polygons,
   plates of equal holes) it is one of theirs. So a round costs what it
   changes and a flood, not a triangulation afresh (a plate of 28 × 28
   holes ran 31 rounds of about 90 000 units each that way). Caps of
   fewer than 64 vertices (`FRESH`) are still triangulated afresh in a
   round after one that changed them, at most 512 units: their
   triangulations, often of points on a circle (a rectangle's corners,
   a circle's arcs), stay those of the fixed shuffled order, so small
   solids, and the booleans on them, are as they were (kept, one chain
   of the seeded suite lost a step). There are
   at most 32 rounds of mending, segments halved at most 16 times all
   told here, and never below `MIN_SPLIT`. A
   segment to halve that is already under `MIN_SPLIT` resolutions, or,
   on the tries that refine, was halved 6 times all told
   (`MAX_MEND_DEPTH`, separation's halvings counting), is
   left to refinement, whose points can take its corner apart (a narrow
   corner along a nearly straight chain of short pieces, or an arc whose
   corner with an inner edge along its tangent to a far vertex stays
   narrow however often it is halved, as along a row of holes 0.1 from
   a plate's side, which has no vertices near: refinement's points
   encroach on the side, and its halvings give the arcs vertices near);
   if refinement has nothing to add and such halvings are left, the
   profile fails with `ProfileError::TooFine` naming its input segment
   (detail too small for the tolerance, which a finer one mends) or, for
   one only halved too often, `TooComplex`. Past the rounds or
   `MAX_CAP_DEPTH` it is `TooComplex` at once, the limits against mending that doesn't
   converge. `Chain::halved` (and `split`, built on it) hands the pieces it refuses, all of
   them in order, to the caller, so a small one is `TooFine` wherever it
   comes among them; separation's own call halves only splittable pieces
   and can't refuse. Coordinates below `1e-30` are flushed to 0 for spade,
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
5. **Repair and check**: `Solid::new_repaired_within` with the same
   work: the mesh is checked first (charged as repair's first pass
   would be), then `Mesh::merge_faces` (a unit a patch) names the faces
   on one surface alike, then the patches the orientation step
   integrated are charged; only if the check fails is the mesh repaired
   (`repair_within`), merged and checked again (`Solid::new_within`).
   That gives what repairing, merging and checking did, for one pass
   over the pairs of patches rather than two; a mesh that fails after
   the fold check pays the pass again in repair. Nearly always the
   construction already passes: over 2 400 random plates with holes
   and 2 000 plates with small holes of sharply weighted conics or
   uneven arcs (fits 0.01 and 0.1), 274 tries' meshes failed the check
   and went on to repair. If steps 3 to 5 fail with
   `Invalid`, `TooComplex` or `ProfileError::TooFine` and work is left,
   they run again from the
   separated chain with flat corners mended (step 3); if that fails too,
   the first error stands, with the first try's evidence (the triangles
   its error names: each try's `Solid::new_repaired_within` gives a
   `Failure`, kept with the error, never a later try's). Moving points in from every flat corner can
   line them up into slivers of their own (along a fine polygon), so it
   isn't the first try: the second only adds solids. Flat corners are
   all the two tries do differently (the first finds them too, with the
   same margin, and drops them; with none, placing points spends nothing
   and every ear gets its centroid as usual), so up to the first round
   that finds one they are the same, work included. The first try keeps
   the state that round starts from (`cap::Rounds`: the chain as halved,
   with its segments' halving depths, the Steiner points, a copy of the
   kept triangulation, which a fresh one from the same points can differ
   from where four lie on a circle, and the round's number), cloned
   before the round changes anything, and the second
   resumes there, counting rounds on towards `MAX_ROUNDS`: the same caps
   as starting over, for less work. With no flat corner found the second
   try would repeat the first with less work left, which can only fail
   the same way or run out, so it isn't made. Results are the same as
   starting over except where that ran out of budget. Anything else made
   to depend on the flag in a round would break this; a test compares
   the resumed caps with those made from the start. A circle of radius
   0.01 to 1 000 cut at random angles into arcs of 0.06° to 86° failed
   at the default tolerance one time in three before, and now one in
   thirteen, all of radius under 0.11, whose shortest arcs are under a
   hundred resolutions long. The first two tries refine for quality
   (with the fork's refinement rounds counted on too); if both fail and
   work is left, the last two are the plain caps' own two tries, neither
   refined, made the same way: refining a sliver of the region thinner
   than the pieces the chain may be halved into (a closed spline 0.001
   high at fit 0.1) halved its sides into pieces that failed where the
   plain caps passed, and a spline of sharply weighted pieces (weights
   1/64 to 64) round two holes, at fit 1e-5, ran out of budget refined
   and plain but passed with the plain caps' flat corners
   (`the_plain_caps_second_try_is_made`). The plain first try is made
   from the separated chain if the first try did anything it wouldn't
   (its refinement asked for anything, or its mending left a halving at
   `MAX_MEND_DEPTH` to refinement that it would make), and keeps its own
   fork; without that the first try was the plain one, fork included.
   The plain second try resumes from that fork, if there is one and the
   refined second try did anything the plain one wouldn't: else it would
   repeat it. Again the first error stands. A profile that fails every
   try pays for up to four: 500 plates of small holes at fit 0.1 (74
   refused) took 20 s rather than 13 s with three tries, and none more
   passed there. Over 5 500 fuzzed profiles at fits 1e-5 to 0.1
   (jittered fine polygons and outlines, conics of weights 1/64 to 64,
   tiny notches and holes in outlines up to 1e5, fine splines of
   sharply weighted pieces, cut circles beside runs of fine straight
   pieces, rows of holes near a plate's side; some on turned frames)
   the four tries refuse one profile the plain caps alone extruded (a
   spline of 6 000 sharply weighted pieces at fit 1e-5, out of budget
   on the refined first try) and extrude 478 they refused; each `Ok`
   passed `check_faces` with the straightened area's volume, and 790
   profiles came out the same at 1 and 8 threads.

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
before they are collected, see "BVH"); the caps 8 per vertex for the
first triangulation, then 8 for each vertex a round inserts (Steiner
points, the vertices between a halved segment's pieces) and the
triangles each round; placing flat corners' points the segments,
Steiner points and candidates plus the ones found near; a run of
refinement the triangles, the bad ones queued at the start, twice the
segments plus the triangles and Steiner points for its BVHs and region
list, the hits of each queued triangle's lookups, the points tried for
clearance and those their groups were rebuilt over, and for each point
inserted 8 plus the faces around it; its halvings the segments once,
plus the pieces made; the crowding count the triangles plus the pairs
counted (at most its limit plus one); then the patches, then twice
the patches and the pairs of boxes for the check (and repair's own pass
too if it fails), a unit a patch for merging the faces, then 32 for each
patch the check integrated (`INTEGRATE_WORK`; about six for each
cylinder-like wall), through `Solid::new_repaired_within` (revolves and
booleans go through `Solid::finished`, which always repairs, then
charges 5 a patch, `CHECK_WORK`, for the check). Vertex counts past `MAX_PATCHES / 4` (the segments
and the Steiner points) are `TooComplex`, at a round's start and as a
run of refinement inserts its points.

Measured (release, load average 20 to 30 on 7 cores, so times are
rough; the fastest of three): the tests' 80 × 80 plate with 64 round
holes 1.1 apart (260 segments) comes out with 4 292 patches (4 012 with
the plain caps) in 0.03 s; a 210 × 210 plate with 400 such holes,
23 112 patches on the first try in 0.22 s (holes pass 0.1 from the
plate's sides, which have no vertices: the edges between the holes
along a side keep leaving their arcs along the tangent, and halving
those arcs never ends; with halving on to `MAX_CAP_DEPTH` the first try
gave up and the second, with flat corners from its tenth round, made
23 896 in 0.30 s); square plates of `k × k` such holes fit the budget
up to `k = 53` (153 216 patches, about 4 s; 51 with mending halving on
to `MAX_CAP_DEPTH`, 38 with the plain caps triangulated afresh each
round, 28 refined that way). Profiles that find flat corners in round
0 and fail anyway still pay for both tries;
a plate with four holes splits nothing (20 segments, 116 patches). A
ring of radius 10, 0.001 wide, needs 1 024 segments to separate, and
its caps refined, a strip whose triangles are as long as the arcs,
49 152 patches in all (0.35 s). 600 random plates with holes and
weights from 0.05 to 20, most refused as touching: the slowest took
32 ms (plain caps). A circle of 4 096 arcs of radius 100 gives 19 552
patches in about 0.14 s (22 300 plain). A 16 384-gon of radius 100
gives 78 560 patches in 0.6 s (65 532 plain, 0.5 s); a 65 536-gon
is `TooComplex` in 0.5 s (plain: `Invalid` in 5 s, its sides
turning by 2e-7 over 1e-2): its chords are under twice `MIN_SPLIT`
resolutions, which exempts the fan its caps keep from their centre;
the crowding gate finds that fan crowded, and counting its pairs and
refining it run out of budget. A quarter disc whose arc is 16 384
straight pieces gives 78 724 patches (1 s), two circles of
16 384 sides round each other 157 156 (1.5 s; their volume is right
to `1.3e-12` relative, the rounding of that many terms). A square with one side a conic of
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
of budget) with its error; small holes of sharply weighted conics
extruded at every fit (a narrow corner at a piece too small to halve,
`TooFine` with the plain caps, and holes whose plain caps repair
couldn't split, `Invalid`), and the caps' refused halvings `TooFine`
for a small piece wherever it comes among them, `TooComplex` for one
halved too often, halving none; the same
bits at 1 and 8 threads; a cap triangle of two curved sides that extrudes
but folds when split straight (see the known gaps). Circles cut
into uneven arcs at three tolerances (the second try, at 1 and 8
threads too); the second try's caps resumed from the first's fork the
same as made from the start (a 10 × 10 square with a small hole of
sharply weighted conics at fit 1e-2, forking in round 5; a fine
polygon, in round 0; the cut circles and random plates), and that
square within the budget only resuming takes, and with no second try
when its first runs out within the fork round; a 210 × 30 strip with
40 holes, a row 0.1 from its side, on the first try (its narrow
corners left to refinement past `MAX_MEND_DEPTH`; the second try's
before);
a star of 65 535 long chords refused within its budget
rather than collecting two billion pairs; two circles of 16 384 sides
triangulated in about a second unoptimized (over a minute in the loops'
order).

Cap quality (`extrude/quality_tests.rs`): profiles whose plain caps
held triangles too thin or too crowded for the mesh's rules, each
extruded right (volume within `1e-12`, `check_faces`) now that the caps
are refined for quality, with what it failed with plain:

- fine regular polygons of radius 10 (1 024, 2 048 and 4 096 sides at
  fit 0.1, 4 096 at 1e-2, 8 192 at 1e-3): all five `Invalid`, at 1 and 8
  threads;
- fine outlines at fit 0.1: a fine ellipse, an outline of uneven radius
  and sides, and fine straight pieces next to an exact quarter arc
  (`Invalid`), a plate with a fine hole and a fine ring;
- a 65 536-gon of radius 100 at fit 0.1: `TooComplex` (as plain), within
  30 s (0.5 to 3 s);
- fans of 4 096 and 16 384 thin triangles (a quarter disc whose arc is
  that many straight pieces) and the strip between two 8 192-gons
  (radii 100 and 50), and in release two 16 384-gons (volume within
  `1e-11`): `TooComplex`, repair's box pairs; the 4 096 fan at 1 and 8
  threads; the 16 384 fan at a budget of `2^18` `TooComplex` quickly;
  the 4 096 fan's plain caps (the plain tries') left as they are, a fan
  with no Steiner points;
- regular 64-, 256- and 1 024-gons whose vertices are 1, 2 or 3
  resolutions off their neighbours' chord, at fits 1e-3 and 0.1: all 18
  `Invalid`;
- circles cut unevenly at fit 0.1 (seed 3) whose shortest chord is 32
  resolutions or more (3 `Invalid`), at fit 1e-2 over seeds 1 000 to
  1 039 (1 600 cases, 32 `Invalid` in 18 seeds), and seed 1003 cases 2
  and 104, 4000 case 23 and 517 case 70 (fit 1e-3) at 1 and 8 threads;
- corner cuts (boxes 0.3 to 2 across, flush or 0.1 in, at each corner)
  on plates of `k × k` holes 10 apart, extruded: 15, 12 and 20 of 32
  refused for `k = 10` at radii 2, 3 and 4, all 32 for `k = 14` at
  radius 2, now none; and the next hole drilled in line with one to
  three extruded ones: 2 of 36, now none. The plain caps fanned out of
  the plate's corners to the holes' lowest and leftmost points, on
  common tangent lines, and a cut along such a fan's long sides failed;
- in regen, steep wavy spline edges (100 fit points, slope 1, widths 1,
  5 and 20, seeds 0 to 9, fits 1e-2 and 0.1, and 30 points over 1 mm at
  1e-2): 9 of the 10 at width 5 and 1e-2 (`TooFine` and `Invalid`)
  and the 30 points (`TooFine`), now none.

Still ignored: a
10 × 10 plate with a hole of twelve conics of weights up to 13.7
(`Invalid` at fits 1e-2 down to 1e-5, refined at 5° or 10° alike: repair
finds two pieces of one patch within the resolution at a vertex).

Pinned, refined: the 64-hole plate's 4 292 patches, the four-hole
plate's 116, a 100 × 1 rib's 100 (both its plain triangles have a 0.6°
corner, and no small input angle exempts them, so its long sides are
halved), the 16 384-gon's 78 560 (release), and the patches of
`circles_cut_unevenly`'s circles (30 092; 18 872 plain) and
`random_plates_with_holes`' plates (1 636; 1 150 plain) all told. The
kept triangulation is the one made afresh from the last round's chords
and points for outlines of 100 to 300 uneven pieces round random holes
of conics, on every try.

Measured (release, load average 20 to 30 on 7 cores, the fastest of
three runs), plain caps against the caps refined at 5° as built (points
inserted one by one into the kept triangulation, mending's halvings
left to refinement past `MAX_MEND_DEPTH`), with the extrude's time, and
from a scratch prototype that refined in batches after mending,
triangulated afresh each round and halved one level a round, at 10° and
20°:

| | plain | 5° | time | 10° | 20° |
|---|---|---|---|---|---|
| 64-hole 80 × 80 plate | 4 012 | 4 292 | 0.03 s | 4 836 | 5 516 |
| 400-hole 210 × 210 plate | 22 620 | 23 112 | 0.22 s | 24 688 | 26 884 |
| 400 holes of radius 2 | 17 596 | 19 180 | 0.13 s | 21 324 | 30 876 |
| four-hole plate | 92 | 116 | | 140 | 280 |
| 100 × 1 rib | 12 | 100 | | 196 | 388 |
| corner cuts refused, `k = 10`, r 2/3/4 | 15/12/20 of 32 | 0 | | 0 | 0 |
| corner cuts refused, `k = 14`, r 2 | 32 | 0 | | 0 | 0 |
| holes drilled in line, refused | 2 of 36 | 0 | | 1 | 0 |
| 1 024-gon, r 10, fit 0.1 | `Invalid` | 4 872 | | 7 404 | 11 492 |
| 4 096-gon, fit 0.1 or 1e-2 | `Invalid` | 19 552 | 0.14 s | 30 936 | 47 988 |
| 8 192-gon, fit 1e-3 | `Invalid` | 39 284 | 0.27 s | 62 008 | 95 152 |
| 16 384-gon, r 100 | 65 532 | 78 560 | 0.6 s | | |
| fan of 4 096 | `TooComplex` | 19 704 | 0.14 s | 28 824 | 40 984 |
| fan of 16 384 | `TooComplex` | 78 724 | 1 s | | |
| two 8 192-gons | `TooComplex` | 78 612 | 0.6 s | 124 460 | `TooComplex` |
| two 16 384-gons | `TooComplex` | 157 156 | | | |
| largest `k × k` plate in budget | 38 | 53 | 4 s | | |
| 38 × 38 plate | | 80 176 | 0.75 s | | |
| cut circles, 1 600 at fit 1e-2, refused | 32 | 0 | | 0 | 0 |
| 1 200 cut circles (seeds 0 to 4), all told | | | 2.4 s | | |

Before `MAX_MEND_DEPTH` (mending halving on to `MAX_CAP_DEPTH`) and
with repair before the check, the 5° column had 4 532, 23 896 (on the
second try), 20 140 and `k = 51`, the times 0.06, 0.30, 0.17, 0.15,
0.28, 0.69, 0.14, 0.93, 0.62, 1.08 and 3.4 s down the table, the
corner-cut plates 64 to 320 patches more; every other count and
outcome, here and in the corpora below, is the same.

**The flat-corner try stays.** Refining every try, it was measured
whether it still mends anything, against the first (refined) try then
the plain caps alone: 2 400 random plates with holes (weights 0.05 to
20, at fits 1e-5, 1e-3 and 0.1), 1 200 and 1 600 cut circles, 30 random
plates, the plates, polygons, fans and rings above, the corner cuts and
holes drilled in line, and regen's spline corpora (360 steep wavy
edges at fits 1e-5 to 0.1, 1 260 closed blobs, 540 wavy plates) come
out the same without it. Plates of small holes (0.05 to 1 mm, random
loops of sharply weighted conics or circles cut into uneven arcs) do
not: of 500 at fit 0.1, 403 and 422 extrude (seeds 1 and 3), 394 and
408 without it; of 500 at fit 0.01, 466 and 459 (seeds 4 and 2), 465
and 459. Those are the short curved segments at coarse tolerances of
the known gaps, whose chords, under twice `MIN_SPLIT` resolutions,
refinement leaves alone, where a point moved in from a flat corner
takes the sliver apart. So it stays, resumed from the first try's
fork, with the plain caps' two tries as the last two.

The seeded boolean suite's tallies are those of the plain caps but
for one more of the chains (201 of 240). 5° was taken: 10° moved a
coincidence (the third hole drilled on the 60 plate, radius 1, pitch 3:
"patch 137 may fold") and costs more patches, 20° costs too much (the
rings run out, the radius-2 plate nearly doubles). The rib and the
four-hole plate change at every bound. With the resolution as the
margin of the way-in test an ear's own centroid lies within the
resolution of its chords at coarse fits, hence margin 0.

Known gaps:

- **Short curved segments at coarse tolerances** fail although the
  profile doesn't touch itself: small holes (0.5 mm and up) whose
  segments get halved down to pieces of 30 to 70 resolutions. Random
  plates with holes and weights from 0.05 to 20 (2 400 cases at fit
  0.1): 32 `TooFine`, the caps asking to halve a piece already under
  `MIN_SPLIT` at a narrow corner, mostly next to a sliver from the short
  chord to an apex 0.5 to 1.6 mm away; 19 `Invalid`, repair finding two
  flat pieces (thin cap triangles meeting, or one against a wall piece)
  within the resolution; 1 that was `TooComplex`, repair's own floor on
  the pieces it splits, and is `Invalid` now (seed 6 case 146; `Invalid`
  at 0.05 and 0.01 too, `Ok` at 1e-3). Weights of
  exactly 1 fail too, if less often; nothing fails this way at 1e-3. The
  cases checked extrude at a finer tolerance (seed 2 case 11 at 0.01),
  which is what the `TooFine` and `Invalid` messages suggest; so did
  every `TooFine` and `Invalid` of 10 800 more cases (seeds 13 to 30),
  of small holes (0.05 to 1 mm, random loops or circles cut unevenly
  into arcs, 4 000 cases at 1e-2 and 1 600 at 0.1) and of wavy spline
  edges fitted at 1e-5 to 0.1, with one exception: a hole of sharply
  weighted conics whose cap triangulation (the same at every tolerance)
  holds a sliver that repair splits into pieces meeting at a vertex
  within the resolution, `Invalid` down to 1e-5. Repair's floor gave
  the last `TooComplex` at 0.1 and 0.05 (seed 24 case 293; seed 18
  case 539 at 0.05) too, now `Invalid`, whose message suggests the
  finer tolerance that mends them (0.01). With the caps refined for
  quality (no sliver with a far apex) the cases above (seed 2 case 11,
  seed 6 case 146, seed 24 case 293, seed 18 case 539) extrude at every
  fit; the stress itself wasn't run again.
  One `Invalid` is a real 0.8° notch between two conics whose sides
  pass `apart_at_joint` by a hair: that test isn't monotone under
  halving (a halved piece at the joint moves its hull towards it), so
  "halving only shrinks hulls" holds for pairs that aren't joints.
- **Repair of a cap patch along a concave curve**: a cap triangle with
  two or three curved sides, in practice one of them concave with a
  weight above 1 (rarely less; in the app only spline fits make weights
  above 1, arcs have `w ≤ 1`), can pass the fold check and yet split
  with straight inner edges into pieces that fold for good (see "Flat
  faces" under Refinement). The extrude itself passes `check` without
  repair, so it never splits such a cap; but repair after a boolean, and
  a boolean's pair refinement (`pairs::refined_with`, the same `Refiner`),
  do, whenever a cap piece meets a patch it has no certificate against
  (a horizontal cylinder across the cap). The folded piece goes into the
  result, and repair keeps splitting it until its pieces are flat and
  meet a flat neighbour, failing at once as
  `Invalid(EdgeNeighbours(t, t))` (or `Hull`, `VertexNeighbours`), or,
  for small curves, reach the split floor first (`Invalid` of the failure
  that asked for the split) or run out of budget (`TooComplex`).
  Never a wrong `Ok`: the folded pieces overlap their neighbours, so
  `check` can't pass them. Checked on variants of the pinned profile
  (concave weights 1 to 5, the other curved side convex or concave,
  cylinders across the cap and through the walls, on three frames, two
  tilted): 1 311 of 1 440 operations went through, each right by its
  volume against numerical integration, the rest refused, alike on every
  frame. Rare: about 0.2% of end-cap patches with two
  or more curved sides of random 3-segment profiles with one concave
  side of weight `1.2..4` fold after one split. Pinned by
  `a_cap_whose_straight_split_folds_extrudes` (a 3-segment profile, a
  concave hyperbola of `w ≈ 3.53`, a line and a convex hyperbola; the
  cap's straight pieces still fold four levels down, and with a parabola
  for the concave side none does) and `a_cap_folding_when_refined_is_right_or_refused`
  (that prism and a cylinder along `x` across its top: every operation
  was `Invalid`, both orders; the clean-up's quality pass now takes out
  the folded piece's inner corner in the intersections and `b − a`,
  which come out right by the identities, the rest still `Invalid`;
  with the parabola, all `Ok`). The mend, not built:
  where a planar patch with curved sides is made (`cap.rs` `mend`,
  `boolean/triangulate.rs`), also require its four straight children to
  pass the fold check, and give one that fails a Steiner point at its
  centroid (or halve the concave segment where the centroid would come
  within the resolution of its hull), leaving pieces with at most one
  curved side.
- **Refined caps cost patches.** Every cap triangle with an angle under
  5° is refined, so plates with holes gain 10 to 20%, a thin rib or ring
  far more (a 100 × 1 rib 12 → 100 patches, a ring 0.001 wide 49 152).
  With the triangulation kept from round to round, square plates of
  `k × k` holes fit the budget up to `k = 53` (38 plain).
- **Caps past about 65 000 segments**: a 65 536-gon at fit 0.1 keeps a
  fan from its centre (its chords are too short to refine at), which the
  crowding gate finds and refines, but counting its pairs and refining
  it take the budget (`TooComplex`); its solid would have some 400 000
  patches anyway. Fans and strips of 16 384 pieces pass, at 80 000 to
  160 000 patches; a hole of radius 5 drilled through the 16 384 fan
  went through (79 352 patches, 13 s on a loaded machine), but larger
  operations on bodies that size come near the budget.
- **Narrow corners where halving doesn't converge**: an inner edge along
  a curve's tangent between two curves that both come close to a
  straight side with no vertices near (a row of holes 0.1 from a plate's
  side) keeps coming back as the arcs are halved. Past `MAX_MEND_DEPTH`
  such corners are left to refinement, which mends the cases seen on
  the first try; the plain caps of the last tries still halve on to
  `MAX_CAP_DEPTH`.
- **Fitted spline chains with real detail near the tolerance** (a dense
  zigzag through 100 fit points whose wiggles are about the fit across)
  were refused with the plain caps: `Invalid` from flat cap triangles as
  for fine polygons, or `TooFine` from the narrow-corner rule halving a
  concave curved segment whose tangent at a loop vertex runs along an
  inner edge to a vertex further along the nearly straight chain, which
  halving doesn't move, until the piece is under `MIN_SPLIT`. Regen's
  run fitting (see "Pieces to conics") removed the sub-tolerance cases;
  the caps refined for quality, with such corners left to refinement,
  take the steep set the regen tests hold. Not fuzzed further.

## Revolve (`src/revolve.rs`, `src/revolve/`)

| file | holds |
|---|---|
| `revolve.rs` | `Sweep`, `revolve`, the frame (`Turn`), the axis rules, the rounds, flat faces and ends, the assembly |
| `revolve/kind.rs` | what face each profile segment turns into: its tag and form |
| `revolve/tests.rs`, `revolve/tests/` | shapes against Pappus, names and forms, part turns, refusals, random profiles |

`revolve(profile, frame, sweep, feature, tol, budget)`: the profile on
`Frame { origin, x, y }` with `y` along the axis and `x` towards the
profile (its points have `x ≥ 0`), turned about the axis `Full` or
`Part { from, to }` (radians, turning `x` towards `x × y`, `0` at `x`;
finite, within `8π` of `x`, `0 < to − from < 2π`, `PatchError::Parameter`
otherwise). Faces: `Side { curve, segment }` per input segment (numbered
per curve in profile order, as the extrude's walls), a part turn's
`StartCap` (at `from`, facing back) and `EndCap`; segments along the axis
make none. The steps:

1. **Checks.** `Profile::check`, `Frame::check`, the sweep. The frame is
   made square to the bit (`y` normalized, `x` made square to it), and
   `x` turned by `from` (cosine and sine exact at whole quarter turns, as
   the lathe's stations), so station 0 is where the sweep starts.
2. **Onto the axis.** A segment end within the resolution of the axis is
   put on it (`x = 0`), and a control point too if both its segment's ends
   are on it and it is that close: a decision by distance (with the
   rule on segments coming that close inside, below), on input, the same for both segments sharing an end, so ends
   stay shared to the bit. `Profile::check` again.
3. **Axis rules**, by exact signs: a segment's distance from the axis is
   `N(t)/D(t)` with `D > 0` and `N` the quadratic of Bernstein
   coefficients `x0`, `w·cx`, `x1`, which reaches below 0 where an end
   does or the middle coefficient is negative with its square over
   `x0·x1` (`ProfileError::CrossesAxis`, checked for every segment first).
   A segment with both ends off the axis that comes within the resolution
   `m` of it inside is `ProfileError::TouchesAxis` (in any turn: the face
   would pinch there), decided the same way on `N − m·D`, of coefficients
   `x0 − m`, `w·(cx − m)`, `x1 − m`: a reach to 0 inside where the middle
   one is negative and its square at least the ends' product. (Exactly
   touching, the square equal to `x0·x1`, never happens in floating
   point: a circle drawn tangent to the axis misses it by a rounding,
   which repair then split until it ran out.) In a full turn a vertex on the axis with no segment
   along the axis either side is `TouchesAxis` too (the solid would pinch
   to a point there); in a part turn it is a solid (both walls end in
   apex caps at the vertex, which both ends share).
4. **The region**: the extrude's `Chain::new` and `Chain::separate` (so
   a region valid in the open half-plane revolves into an embedded
   solid), and the caps' winding rule (`cap::nests`, on the chords) for
   the nesting, which a full turn would otherwise never run. A part turn
   whose ends come within the resolution of each other at the profile's
   farthest control point (`2·ρmax·sin((2π − θ)/2)`) is
   `ProfileError::NearlyFullTurn`.
5. **Kinds** (`revolve/kind.rs`), per input segment, from its chain side
   (straightened within the resolution, as the extrude's):
   - **along the axis** (straight, both ends at `x = 0`): no face;
   - **flat**: straight with its ends' heights within the resolution: a
     plane square to the axis facing out (`−y` where the segment runs out
     from the axis, the region being on its left), tagged with it;
   - **straight** otherwise: a cone, exact. Its form is `Cone` (apex where
     the line meets the axis, the axis into the nappe, the half-angle's
     cosine and sine from the segment), or `Cylinder` (the mean radius)
     where the ends' radii are within the resolution; its tag is
     `Quadric::cone` about the apex where the segment is at least as wide
     as it is tall (the apex is then near), else `Quadric::revolution`
     written about the segment's foot on the axis, `ρ² = (ρ0 + s·h)²` with
     `s` the slope (well conditioned up to the cylinder, `s = 0`, where an
     apex would be far out). A cone too nearly flat for its quadric
     claims no surface (`held`): on a tilted axis `F` sums terms of `|y|²`
     (`y` from the quadric's origin) to a value whose gradient,
     `2·|y|·cos·sin`, vanishes with the slope, so its first-order distance
     rounds to about `ε·(|y|²·Σ|aᵢⱼ| + 2·|b|·|y| + |c|)/|∇F|`, largest at
     an end of the segment; four times that must stay under a quarter of
     the resolution (a ring's top a few hundred-millionths off flat, 250
     across at a fit of `1e-4`, was refused by the tag check). Its strips
     are still exact and its form the cone;
   - **an arc centred within `m·w/4` of the axis** (`circle_of`, `w` the
     arc's weight, `m` the resolution): a sphere, exact, tagged
     `Quadric::sphere`. Its strips, built on the arc as drawn, stray from
     the sphere on the axis by about `δ·(1 + 1/2w)` for a centre `δ` off
     it (measured: the diagonal of a wide arc's strip strays furthest,
     5.8 times `δ` at 170°), so within `3m/8`, which the tag check holds;
     taking every centre within `m` refused a 170° band from `m/5` off;
   - **any other circle's arc** with its centre off the axis on the
     profile's side: `Form::Torus` (a spindle torus's where the centre is
     nearer the axis than the radius), fitted, claiming no surface; other
     conics, and arcs of circles centred across the axis (a lemon):
     `Form::Revolved` with the segment's conic as meridian, fitted.

   Ellipse and other conic arcs symmetric about the axis would be exact
   quadrics of revolution (`revolution_strip` makes them), but no sketch
   curve gives one, and telling them is left until one does.
6. **Rounds.** One lathe for the whole solid (`Lathe` about `−y`, so its
   stations turn `x` towards `x × y`), in 4 pieces for a full turn and
   `⌈θ/90°⌉` for a part. Each round builds every face on it:
   - **Walls.** Each profile piece's meridian is the piece run backwards
     (the lathe's `(ρ, h)` has `h` along `−y`, so the profile's
     counter-clockwise loops run clockwise there; backwards, the strips
     face out). A meridian ending on the axis gets a fitted cap round the
     pole (`pole_cap_with`, on the face's copy claiming no surface; a
     cone's with a straight `Conic::line` meridian, whose linear rulings
     the cap is fitted with, since the exact ones would have their control
     points on the apex), halved first if both its ends are on the axis.
     The rest of the meridian (the cap's `rest`, or the whole) is exact
     strips (`revolution_strip`: a sphere's arc pieces; a cone's straight
     pieces with their control point at the geometric mean, written
     `(p0·√ρ1 + p1·√ρ0)/(√ρ0 + √ρ1)` from the ends' radii: the same point
     as `cone_ruling`'s with no apex to round on, the midpoint for equal
     radii), or fitted bands (`fitted_band_with`) for fitted kinds. A band
     asking for more pieces round the axis halves the lathe, and the round
     starts again.
   - **Flat faces**: the region between their two rings in their plane
     (the outer ring's arcs counter-clockwise, the inner ring's clockwise,
     or none for a disc, whose centre then isn't a vertex), and in a part
     turn the sector closed by the piece at both ends (a disc's sector
     through its centre on the axis), triangulated by the extrude's caps
     (`Chain`, separation, `cap::triangulate`) in coordinates along `x`
     and `x × y`. If the caps want a ring's arc halved, the lathe is
     halved (all arcs are alike): the plan's growing `k`. Their own
     errors are `ProfileError::TooFine` naming the flat segment.
   - **Ends** of a part turn: the profile's region again, its boundary the
     walls' meridian pieces at station 0 (caps' meridians, bands' pieces,
     exact rows, axis and flat pieces) taken back into `(x, y)`,
     triangulated by the extrude's caps; the end at the last station is
     the same triangles turned. Where the caps want a piece halved, that
     piece of the profile is halved the same way (the halving tree, from
     the depths the caps' chain gives each piece, at `½` as theirs) and
     the round runs again. Profile errors name the input segment the
     piece came from.

   At most 64 rounds, and `Lathe::MAX_PIECES` (4 096) pieces round the
   axis; past either `TooComplex`. Every piece's strips are counted
   against `MAX_PATCHES` and charged before they are made.
7. **Assembly.** Rings are found by the bits of their point at station 0,
   so faces built apart share them; a point on the axis is one vertex at
   every station (`Lathe::turned` keeps a point `on_axis` as it is, so
   meridians ending there keep it to the bit). Edges are the strips',
   caps' and parallels' curves (flat faces and ends share the walls'
   records; axis segments and flat pieces are straight). Then repair, the
   merge pass (two collinear lines, or one cone drawn as two segments,
   are one face), and `Solid::new_within`. As the extrude, a first try
   without and a second with Steiner points moved in from flat corners,
   the second only where a triangulation found such a corner and the
   first failed with `Invalid`, `TooComplex` or `TooFine`; unlike the
   extrude's, the second starts over rather than resuming where the first
   found the corner, since a round triangulates several faces. If the
   second fails too, the first try's error stands, with the triangles it
   names (`Solid::finished` gives a `Failure`; see "Check evidence"
   under "Limits, budgets and errors").

The meshes are closed by construction: every strip, cap and flat
triangle names its corners by ring and station, and the face tags are
checked as an extrude's. Measured (release, the tests' shapes on the
`z` axis at `1e-3`, a full turn and 270°): disc 20 and 16 patches, washer
64 and 52, cylinder 20 and 16, cone 34 and 32, hollow frustum 64 and 52,
sphere 40 and 44, hollow ball 240 and 208, torus `R 10, r 2` 512 and 412,
the lathe profile 298 and 242, the groove 384 and 296, the ring with a
round hole 1 024 and 784; 1 to 55 ms each. The plan estimated 160
patches plus caps for a lathe profile of 20 segments without tori.

Tests (`revolve/tests.rs`): a disc, a washer, a cylinder, a cone, a
hollow frustum, a sphere, a hollow ball, a torus, a lathe profile of
lines and arcs (a chamfer, a round, a ball end), a groove (a concave
round from its turn down a wall), a ring with a round hole (a toroidal
void), a double cone hollowed out between two axis segments, and a
diamond touching the axis at a vertex (part turns only), each turned
fully, through 30°, 90° and 270°, and from −45° to 60°, on the `z` axis
and on random frames up to `1e3` out at `1e-2` to `1e-4`: `check` with
face tags, volume by Pappus within `1e-10` relative (plus the area times
half the fit tolerance with fitted faces), area within `1e-10` relative
(or `4·A·fit/2` over the smallest radius), refused turned inside out,
drawn; exact shapes with no face claiming no surface; ends in the planes
at `from` and `to`, facing out; faces named per curve (the axis none,
collinear lines one face with the second's key an alias, a circle of one
curve one face, four separate curves four), segment numbers per curve;
forms (planes, cylinders, the cone's apex and half-angle, the torus, the
sphere); lemons and spindles (arcs whose circles reach across the axis);
a 170° band of a sphere drawn round centres a hair off the axis (a
sphere within `m·w/4`, fitted beyond);
an ellipse's, a parabola's and a hyperbola's arcs (`Form::Revolved`);
vertices a hair off the axis put on it; cones a few hundred-millionths
off flat on tilted frames claiming no quadric; the lathe growing for tori and
not for cylinders; the same bits at 1 and 8 threads; refusals
(`revolve/tests/refusals.rs`): across the axis (a side, a bulge, beyond
the resolution), touching it at a vertex in a full turn and inside a
segment in any (a parabola exactly, a lens's arc within rounding and
within the resolution), nearly full turns, bad sweeps and frames, touching and
badly nested loops, the budget; random profiles of lines and conics, off
the axis and fanned from it, on random frames and sweeps
(`revolve/tests/random.rs`, release only): right by Pappus, volume and
area, or refused,
never wrong (116 of 120 right; the rest refused for the profile, or a
part turn far out `TooComplex` before repair); creases only the pencil
parts (a triangle's inner corners, its flip onto an outer wall, two
lines into one quadrant, a 10° wedge, lenses of 40° and 10°, a D, a
round past its turn against a wall, and a dovetail as a control), full
and part turns on `Frame::Z` and a random frame at `1e-2` to `1e-4`
(`1e-5` in release) through the same checks, under a ceiling of about
twice the measured patches (the triangle 304), the same bits at 1 and 8
threads; the same crease profiles turned fully and cut by a box through
a crease's ring, a cylinder round the axis through it and a drill across
it, all three operations (release; the first three with the box in
debug), each result checked with its tags and the volumes adding up (72
of the 81 work, the rest `Invalid`); and a spindle's arcs closed by a
wall straight down.

Known gaps:

- **Repair left at creases.** Creases into one quadrant of the
  meridian plane (an acute corner against a wall, two sides leaving a
  corner up and out, a D, a round past its turn against a wall, a lens's
  tips) are parted by the pencil rule ("Control hulls"), so they cost a
  few hundred patches at any fit, where repair once split their rings
  until the arcs were straight to the resolution (the triangle `(2, 0)`,
  `(5, 1)`, `(2, 2)`: 14 192 patches at `1e-1`, 229 232 at `1e-3`,
  `TooComplex` finer). Some repair is left, none of it the edge rule: in
  a full turn a cone strip's diagonal triangle a quarter turn wide shares
  only a station with the wall's ring triangle, and no plane through the
  station parts them though they are apart, so repair splits them (the
  triangle comes out with 304 patches, its part turn with 14); thin tips
  (a 10° wedge, a thin lens) have non-neighbours closer than the
  resolution across them; and on thin creases at fine fits the faces'
  curvature, not their direction, decides some edge pairs (the 10° lens
  at `1e-5`: 2 816 patches). Repair is charged a unit a pair tested and
  a unit a split, which measured (on a loaded machine) about 3 to 5 µs a
  unit at 57 000 patches against the half microsecond the units stand
  for: its pair tests near creases cost several units each.
- **Arcs tangent to the axis at a pole** (a horn torus's piece, in a
  part turn; a full turn makes them a cusp or a lone vertex): the pole
  is a zero-angle apex its cap can't be fitted to, `TooComplex`, or
  `CrossesAxis` where the control point rounds across the axis.
- **Thin wedges**: a part turn under about `0.06°` (the sine of the
  sector's corner under `SIN_MIN`) can't triangulate a disc's or ring's
  sector: `TooFine` for the flat segment.
- **A pole next to a turn**: an arc ending on the axis whose height turns
  within about twice the resolution of it (a ball drawn round a centre
  `0.1` to `2` resolutions off the axis, at fits of `1e-1` and `1e-2`)
  leaves its cap no room before the turn (the cap stops half way to it):
  `TooComplex`. Further off it is a spindle's, fitted, and passes.
- **Necks a hair off the axis**: a vertex just beyond the resolution of
  the axis (not put on it) makes a neck or a hole that thin; its cone
  strips' weights pass the patch bounds (`PatchError::Weight`) or caps
  can't take it (`TooFine`): refused, with a message about the patch
  rather than the profile.
- **Quadrics of revolution other than spheres and cones** (an ellipse
  arc centred on the axis with an axis along it) are fitted, not exact
  (above).

## Volume, area and measuring (`Solid::volume`, `Solid::area`, `src/quadrature.rs`, `src/measure.rs`)

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

**Measuring** (`measure.rs`, for the measure tool) works on a solid as
built, with `+ − × ÷ √` and `trig` only, each part charged to a `Budget`
(past it `MeasureError::TooComplex`, "too complex to measure"), the same
bits at any thread count (integrals per patch by `par_map`, summed in
patch order; searches sequential). A `Pick` is `Body`, or a region,
chain or corner of the solid's `Topology` by index, or a chain's point
(`EdgePoint`: its `EdgeMeasure::point`, a straight edge's middle or a
round one's centre, `NotFound` for an edge of another shape), measured
as `Measured::Point` as a corner is and a single point in a distance; a `Target` is a pick
with its solid and topology, and `measure(target, tol, budget)` gives a
`Measured` (a pick naming nothing, or a topology that isn't the
solid's, is `MeasureError::NotFound`, never a panic). `distance(a, b,
tol, budget)` gives the minimum distance between two targets (below).

- **Lengths** of chains, curve by curve in chain order: a curve whose
  control point is on the segment between its ends (to 64 roundings of
  its coordinates) is that segment, its length the distance (it runs
  one way along it, whatever its weight); a circle's arc (`circle_of`,
  now generic over the plane and space) is `r·θ`, `θ/2 =
  atan2(rise, half chord)` (the tangent–chord angle); any other conic
  8-point Gauss–Legendre over pieces halved (`split_half`) until their
  weights are within `0.97..=1.03`, their control legs turn at most 45°
  and neither leg is over 1.2 times the other, at most 24 halvings
  (`TooComplex` past). The weights alone (the plan's rule) left `1.6e-7`
  on random conics: a nearly half ellipse (`w = 0.017`) halves into
  lopsided pieces whose speed changes too fast for the rule. With the
  three: `2e-14` worst on 300 random conics of weights `1/64 ..= 64`
  against an adaptive rule, the parabola `y = x²` over `[−1, 1]` exact
  to the bit, a `10 × 4` ellipse's quarter `1e-13` against `a·E(e)`
  (by the arithmetic-geometric mean), arcs to rounding. A unit a piece.
- **Areas, volumes, centres of mass**: `Solid::area`'s rule (the patch
  split by `pieces` as `Solid::volume` splits it), over a region's
  triangles for a face; `Solid::moments` (volume and centre) with the
  centre from `∫ x_i dV = ½ ∮ (x − o)_i² n_i dA`, `o` the middle of the
  control box. The pieces are counted first (`piece_count`, a unit a
  patch) and charged `INTEGRATE_WORK` each before any is integrated.
  Boxes, cylinders, a half cylinder's centroid (`4r/3π`), a tilted
  extrude's, a frustum's (exact cone strips) to `1e-12`; a sphere zone
  (exact strips) to `1e-10`.
- **The tight box** (`Solid::tight_bounds`, and the body's): the
  vertices; each edge's extremes along the axes, where the numerator of
  a coordinate's derivative, `w(c − x0)(1 − t)² + (x1 − x0)t(1 − t) +
  w(x1 − c)t²`, vanishes (with `s = t/(1 − t)` a quadratic, solved
  stably), exact to rounding; and the insides of patches by a depth-first
  search over pieces of the patch's domain (`quarters`, control points
  by blossoming), the control points the upper bound and the piece's
  middle the lower one, dropping pieces that can't come more than the
  resolution above the best so far, with Newton's method on the
  coordinate's gradient (second derivatives by central differences)
  from the patch's middle and from new bests of pieces a quarter of the
  patch across or smaller, four tries a patch. A patch is searched only
  where its hull reaches more than the resolution past the best, and
  never on a plane or a cylinder, conic cylinder or cone tagged as such
  (`ruled`): a coordinate is linear along each ruling, which runs on to
  the patch's boundary, so the extremes are on the edges, to the
  resolution of the tag. Each side is a point of the solid, within the
  resolution of the extreme, kept within the control points' box
  (`Solid::bounds3`): a coordinate's extreme on an edge is a point whose
  other coordinates may round an ulp past it (fuzzing found one at an
  extrude's top). Without Newton a sphere zone's box cost
  330 000 visits at the finest tolerance (centres creep up to an
  isolated maximum as slowly as hulls come down on it); with it, 1 100
  to 2 400 at any tolerance, its sides within `3e-15` of the sphere's.
  Booleans of crossing cylinders (most patches claim-free bands) cost
  1 300 to 17 000 visits; the integrals dominate (32 units a piece).
- **Forms, edges, points, directions**: a face's `FaceMeasure` is its
  area and the form of its first triangle's face (a cone's half-angle
  by `atan2(sin, cos)`); a plane's normal is a direction, a cylinder's,
  cone's, torus's or revolved surface's axis and a conic cylinder's
  `along` are lines. An edge's `EdgeShape` is a `Line` (every curve
  straight, every point within `1e-9` of the chain's size of the line
  through its ends), a `Circle` (from `circle_of` of the curve of least
  weight, the axis the sum of the curves' control-leg crosses, five
  points of every curve on the circle and in its plane: five points fix
  a conic) or an `Ellipse` (the same from the least weight's ellipse:
  centre `c + (m − c)/(1 − w²)`, conjugate semi-diameters `(c − O)·w`
  and `(p1 − p0)/(2√(1 − w²))`, semi-axes `(√(S + 2P) ± √(S − 2P))/2`
  with `S` their squares' sum and `P` their cross's length; not for
  `1 − w² < 1e-6`), else `Other` (`measure::edge_shape(solid, chain)` gives it without
  the length, cheap enough for every edge of a model: the picking
  tables' snap points). A straight edge's point is its
  middle, a round one's its centre; a corner's its vertex; a body's its
  centre of mass. `angle(a, b)` is `atan2(|a × b|, a · b)`, folded to
  `[0, π/2]` (`|a · b|`) where either is a line.

**Minimum distances** (`measure/distance.rs`): `distance(a, b, tol,
budget)` gives a `Distance { distance, points }` between two targets
(faces, edges, corners, bodies; one solid or two; a body's is its
surface's, so a body inside another is its surface's gap from the
other's; the empty solid's body is `MeasureError::Empty`). The picks are
cut into elements (patches, a chain's curves, a corner's point), and the
search is `near::search` (now generic over what it visits, its visit
taking the item `&mut` and the `Work`), over pairs of nodes of a box tree
of each pick's elements (median splits, at most four a leaf, the larger
node split, children nearest first), then of their elements' pieces
(`split4`, a curve's halves; a point never splits). The best (closest
points found) drops a pair whose lower bound clears it less the
resolution `eps`; the search stops once the best is within `eps` of 0.
Lower bounds: boxes' gap, GJK on control hulls (`apart`), for flat pieces
the projections on their corners' normals (`near::apart_along`), and
**rounds**: a cylindrical face's axis, a spherical one's centre, a
circular edge's axis and centre (`circle_of`); the distance `f` from one
moves no faster than the point, so the gap between its ranges over two
pieces bounds their distance. A piece's range is from the Bernstein
coefficients of `|Q_⊥|²` and `W²` (both quartics, `Q/W` the piece about
the centre, across the axis): the ratio lies between their least and
greatest coefficient ratios, which on a piece of the round are all `r²`
(exact to rounding) and on a piece ending on it (a cap at a circular
edge) the least is. The gap is taken less 64 roundings of how far the
pieces' boxes reach from the centre (`Q_⊥` rounds by a few ulps of `x −
centre`, a difference within half an ulp of its result however large its
terms; that reach along a cylinder's axis may be far more than the
distance across). An allowance by the coordinates instead (the first
version) was more than the finest resolution near the coordinate limit
(`64 ε · 2·10⁶ ≈ 3·10⁻⁸`), so pieces along a line of closest points were
never dropped by their rounds there: a pin off centre in a tube at
`10⁶` cost 3 million units at the finest fit, under 200 000 now. Upper bounds: the pieces' corners and middles, and
Newton's method on the squared distance over both pieces' parameters
(`closest`: second derivatives by central differences,
Levenberg–Marquardt on Gauss–Newton's matrix damped by the gradient's size
where Newton's isn't positive definite, steps at most a piece across and
halved while they move the points apart by more than rounding); a step
leaving a piece stops on its boundary and goes on along that edge, then at
that corner, stepping back up when moving into the piece (or along the
other edge at a corner) brings the points nearer, at most four times.
Newton runs at every pair of elements' first visit, at pairs of flat
pieces every second split down a lineage, at up to 64 pairs that can beat
the best only by less than `eps` (and by more than `1e-9` of it: ties
don't count; the nodes they are in are gone down while tries are left),
and on the best at the end. A pair is dropped too when both pieces are
within `eps/2` across (their corners are then within `eps`). So the
answer is two points of the picks within `eps` of the least distance (up
to the splits' rounding, as `near`'s), and to rounding in practice:
random points against a cylinder at all three tolerances `1e-14`
relative (inside, outside, past the rims), random boxes exact, random
skew rods `2e-15`, a point on a cylinder's axis `r`. Where they touch
along a line on curved faces (rods side by side), the squared distance
grows as the fourth power of the way round, so the points settle to about
`√ε` and the distance is about `1e-13`; crossing, `1e-16`.

Costs (release, one thread; a unit is a visit, 32 a Newton search, one
more for rounds' ranges: measured 0.4 to 0.57 µs a unit where the search
dominates): skew rods 1 800 units (0.7 ms); a pin through a tube or off
centre in a plate's hole, bodies, 700 to 1 500 (about 1 ms, the ends'
pieces at the hole's rim dropped by the pin's round); the pin's wall
alone along the hole's wall (a line of closest points, which only
splitting resolves) 46 000 (18 ms), and 660 000 (0.28 s) at the finest
tolerance; two holes' walls 43 000 (25 ms); a plate with 64 holes (3 724
patches) against a box over it 15 000 (1.6 ms), the trees dropping the
rest. Without rounds the pin through the tube cost 408 000 visits (0.3
s), and ten times that at the finest tolerance (refused); a broad phase
by `Bvh::hits_within` within a first bound (the first version) made
`n·m` pairs for bodies far apart.

Tests (`measure/distance/tests.rs`): two boxes face to face, edge to
edge, corner to corner, exact, by bodies, faces, edges and corners;
picks of one box; a point and a cylinder (off the wall, past the rim, on
the axis); skew cylinders; touching and crossing boxes and rods; a tube
and a pin (rounds, and their cost); a pin along a hole's wall, two holes'
walls and rims; concentric spheres; revolved balls (`revolve`) to
each other, their sphere faces, a lid over one (its top off its poles,
to rounding), a lid over the other's pole (within its cap's fit), and a
box's corner and edge; random points and a cylinder, boxes
and skew rods; a plate with 64 holes against boxes near and far, and rods
far out; a pin off centre in a tube near the coordinate limit at the
finest fit (its walls' gaps, and their cost); rounds' ranges holding samples of random patches; the same bits
at 1 and 8 threads; out of budget refused; picks naming nothing and the
empty body.

Tests (`measure/tests.rs`): a box's twelve lengths, six areas and
normals, volume, centre and box; a cylinder's rims (circles, `2πr`,
centres, axes), walls and caps; a half cylinder's centroid; an ellipse
quarter's length alone and as a tilted extrude's rims (semi-axes,
centre, axis); a tilted box's tight box equal to its corners' (and to
the control box) and a tilted cylinder's against `r·√(1 − a_k²)` beyond
its rims' centres, well inside its control box; a fuzz-found extrude
whose tight box stays within its control box; a sphere zone's sides
inside its patches and its centroid; a frustum's centroid, half-angle
and wall area; angles; random conics' lengths and edge extremes; the
same bits at 1 and 8 threads; out of budget refused; picks naming
nothing. Revolved (`revolve`, on the `z` axis and a tilted one off the
origin): a cone's and a hemisphere's volume, area and centroid (`h/4`,
`3r/8`), the cone's half-angle, axis and wall area, the hemisphere's
box and sphere face; a torus's area and volume, its tight box against
`R·√(1 − a_k²) + r` and its form. Each within what half the fit
tolerance allows of a shell over its area: the volume within `fit/2·A`,
the area within `2·fit·A/r` (`r` the least radius of its fitted
faces), the centre within `fit·A·size/V` (measured: the cone, exact but
for its apex cap, `1e-9` relative; the hemisphere `1e-7`; the torus
`1.2e-5`, its box within the fit).

## Topology and names (`src/topology.rs`, `src/topology/`)

`Topology::new(&solid)` (or `Solid::topology()`) is the solid's B-rep as
users see it, derived from the mesh and never stored:

- **Regions** (`Region { key, aliases, tris }`): connected sets of
  triangles of one `FaceKey`, joined across mesh edges, numbered by their
  lowest triangle; `aliases` is the sorted union of their faces'
  aliases. A region is what users see, pick and name as a face: a
  circle's four quarter walls are one, a face cut in two by a groove is
  two of one key. `region_of(tri)` maps triangles to regions.
- **Chains** (`Chain { regions, halfedges, closed }`): maximal paths of
  mesh edges with the same two regions either side, the lower region
  first, its halfedges on that region's triangles end to end in the
  order they run. A chain runs on through a vertex where exactly its two
  edges meet, one running in and one out (regions' boundaries are
  oriented loops, so that is the case wherever only two regions meet,
  once each); it ends anywhere else (a corner, or two regions meeting
  twice at one vertex). A closed one starts at its lowest halfedge.
  Numbered by their lowest halfedge.
- **Corners** (`Corner { vertex, regions }`): vertices where three or
  more regions meet, by vertex.

Everything is one sequential pass in index order: the same mesh gives
the same topology at any thread count; linear in the mesh (a sort for the
corners), like tessellating, and not budgeted: `check` bounds the mesh.

**Names.** `FaceName { feature, part, instance }` is made only from what
a feature was given (curve ids, references), never from mesh indices or
positions, so a regenerated solid with other dimensions, another
tolerance or another triangulation has the same names. `FaceKey {
feature, part: PartKey, instance }` (`FaceName::key`) drops what numbers
the pieces of one surface: `segment` (and a sweep's `piece`); a loft's
`span` stays, since a ruled loft's spans meet at creases. Keys are what
references store (they derive `serde`; their fields and variants' order
are fixed). Names derived from other names are the fixed 64-bit
`topology::mix(parts)`: from the count of parts, each part as `h =
f((h + γ) ^ part)` with splitmix64's finalizer `f` and constant `γ =
0x9e37_79b9_7f4a_7c15`, wrapping; a test pins its values, worked out
apart from the code. `FaceKey::mixed` is the mix of the feature, the
part's place in `PartKey` and its two fields (0 where it has fewer), and
the instance; a copy's instance is `mix(parent's instance, feature,
index)` (`FaceName::copy`), so copies of copies stay unique; a blend's
`edge` is `blend_edge(faces, ordinal)`, the mix of the two keys' mixes
(lower first) and the ordinal among the feature's references with that
pair.

**Aliases.** Where two faces on one surface become one, the merged
face's key (and its aliases) become aliases of the face that took it in
(the lower index, so the first operand's name stays), through
`Soup::absorb`. The clean-up's merge of plane faces joined by mended
seams does this for whole faces (a boss's top flush with the plate's
merges into it, and the boss's top key then names only the plate's top,
through its alias). `unbend` does it too for the triangles it flips onto
the lower of two faces of one plane, where the higher face may keep
other triangles under its own key: then the key names both regions, and
the point picks. Whether `unbend` fires depends on the triangulation, so
such a partial alias can come and go with the tolerance; it only ever
adds a candidate in the same plane. Any later pass that moves triangles
between faces of one surface must call `Soup::absorb` the same way.
After repair, `Mesh::merge_faces` names adjacent faces of one surface
(planes and quadrics) alike, each member of a set taking the others'
keys and aliases as its own aliases ("Structure"): two coaxial walls
stacked, flush plates side by side, collinear lines' walls in an
extrude. So a key absorbed anywhere still resolves, to the region it is
part of now.
Flush caps often never meet as triangles: the perturbation keeps one
whole and drops the other (a plate first, a boss standing in it flush
on top: the boss's top is gone, with no seam to mend). So after a union
or an intersection (`boolean::covered`), an operand's plane face whose
key or aliases no face or alias of the result names any more gives
those names as aliases to the lowest result face of the same plane
(unit normals within `1e-12`, offsets within the short length) that it
meets: the middle of a triangle of one lies within the short length of
the other, either way round, since a large face's middles can all miss
a small face over part of it (looking only at the dropped face's first
triangle lost a plate's top when a boss inside it came first in an
intersection, so the names differed with the order). A point on the
result's boundary facing the same way as the dropped face is a point
where it lay flush. Only names: the geometry is untouched, and a face
that was cut away or lay inside the other operand gets none; a flush
face kept in part keeps its key there and gets no alias for the part
dropped. The middles are looked up through a box hierarchy of each
face's triangles, charged a unit a triangle, and each middle a unit and
one a patch it is measured against. A difference makes none (the
tool's faces become walls of their own). A key can be a face's and
another's alias at once (one feature's two bosses, one flush and merged
into the plate's top, the other standing): it names both, and the
point picks, which is right for a reference to either boss.
Transforms will carry the table with the faces, as booleans do.

**Tangent chains.** `Topology::tangent_chains(solid)` gives each chain
the lowest chain of its tangent chain: chains joined end to end through
vertices where one runs on smoothly into another, their curves' tangents
leaving the vertex (towards the control point, or along the chord where
it's on the end) opposite within 1° (`u·v < 0` and `(u·v)² ≥ cos²1°
|u|²|v|²`, `cos 1°` a literal). Closed chains have no ends and stay their
own. The ends at a vertex are compared pairwise (sorted by vertex and
chain) and joined by a union-find under the lowest index, so it's one
sequential pass, the same at any thread count, and independent of the
tolerance (the curves, not the drawn segments). The edge sessions'
"tangent chain" and the viewport's selection use it through regen's
`Picking::tangents`.

**Resolving.** `Topology::face(solid, key, near)`: the regions named by
`key` (their key, or an alias); `Topology::edge(solid, [a, b], near)`:
the chains between regions named by `a` and `b`, either way round;
`Topology::corner(solid, [a, b, c], near)`: the corners where some
region is named by each, the three keys all different (a key twice
would let any corner of its face answer; a key and its alias may still
name one region, as a reference stored before two faces merged does,
and the point decides). One is taken whatever `near` says (even NaN);
of several the nearest to `near`, a later one counting only where it
comes nearer by more than a billionth of the solid's size (about what
the searches are accurate to), so ties go to the lowest index whatever
the rounding; none is
`NotFound::{Face, Edge, Corner}` ("face not found", ...). The distance
(`topology/distance.rs`) is a best-first search over pieces by
blossoming, the box of a piece's control points (which holds it) the
lower bound, corners, middles and on patches the foot of the
perpendicular by Newton's method (second derivatives by central
differences, Gauss–Newton's step where that isn't a minimum's; plain
Gauss–Newton doesn't converge off a curved surface by its radius) the
upper bounds; it stops when no piece can come within a billionth of the
patch's size of the best, or at 256 pieces a patch or curve. The
candidates' patches are taken nearest box first and only while their
box comes nearer than the best so far, across candidates too (a later
one must come nearer by more than the billionth). One resolve's searches share a fixed
allowance of 2²⁰ evaluations (a Newton foot 80, a patch's piece 25, a
curve's 7); past it, the patches and curves still to look at count by
their corners and ends alone, an upper bound. So resolving costs a
bounded search plus a pass over the candidates' boxes and corners, not
256 pieces a patch: a point at the middle of a sphere cut in pieces is
as near every patch, and at the patch limit that would be minutes. The
allowance only runs out where many patches are about equally near,
where any candidate is as good. A `near` that isn't finite takes the
lowest candidate without measuring. It only chooses among candidates of
one name: nothing is decided by distance.

## Transforms and assembly (`src/transform.rs`)

`Motion` is an affine map `x ↦ L·x + t`, kept with `N`, the map of
normals and of planes' and quadrics' coefficients (`L`'s inverse
transpose, kept rather than worked out, so a turn's is its own matrix to
the bit), and whether it mirrors (an odd number of mirrors: a flag, not
the determinant's sign), its `stretch` (at least the most it stretches a
length: 1 for rigid motions, a scale's largest factor, the product for a
composition) and whether it is `uniform` (`L` is `stretch` times an
orthogonal matrix: rigid motions, uniform scales and their compositions).
The constructors are the only way to make one:

- `translation(t)`; `pattern_step(direction, spacing, k)`, the move by
  `k·spacing` along the unit direction, placed directly;
- `turn(point, axis, degrees)`, Rodrigues' `cos·I + sin·[k]× + (1 −
  cos)·k·kᵀ` about the unit axis, offset `point − L·point`;
  `pattern_turn(point, axis, degrees, k, count)` turns by
  `k·degrees/count`, placed directly (copy `k` is never `k` steps
  composed, so no error piles up). **Angles are in degrees**: `%` on
  floats is exact, so the angle is reduced to `[−180°, 180°]` exactly
  and a multiple of 90° takes its sine and cosine as `0` and `±1`; about
  a coordinate axis the matrix then has only those entries and maps
  coordinates to the bit. Other angles go through `trig::sin_cos` of
  the reduced angle in radians.
- `mirror(point, normal)`: `I − 2·n·nᵀ/|n|²` (symmetric and
  orthogonal, its own `N`), offset `2·(n·point)/|n|²·n`, `n` the normal
  divided by its largest coordinate first (so `|n|²` can't overflow or
  underflow, and a normal along an axis is that axis exactly): exact in
  planes square to a coordinate axis whatever the normal's length.
- `scale(centre, factors)`: `x ↦ c + S·(x − c)`, `S = diag(factors)`
  along the world axes (`DVec3::splat(f)` for a uniform one), `N =
  S⁻¹`, offset `c − S·c`. Each factor within `1/MAX_SCALE ..=
  MAX_SCALE` (`1e6`: the feature allows `1e3`; it keeps a quadric's
  coefficients, which grow by `1/s²`, far inside `f64`), positive (a
  negative one is a mirror). Exact for powers of two about a centre they
  scale exactly.
- `then(next)` composes (`self` first); `point`, `vector`, `normal` apply
  it.

Constructors give `None` for input that isn't finite, a zero axis or
normal, a factor out of range, or an offset that overflows (`k·spacing`,
`c − S·c`); points the motion
takes past `MAX_COORD` are refused when it is applied.

`Solid::transformed(motion, copy, tol, budget)` maps every vertex and edge
control point (weights stay: an affine image of a rational curve is the
curve of the mapped control points, same weights), refusing one past
`MAX_COORD` (`KernelError::Patch`). Claims map with the motion: a plane
`n·x = d` to `N·n`, `d + (N·n)·t`; a quadric's origin as a point, `A` to
`N·A·Nᵀ`, `b` to `N·b`, `c` kept (on the image `y = M·y'` with `M = L⁻¹ =
Nᵀ`), exact for a scale by powers of two. Under a uniform motion forms
keep their kinds: points as points, axes as unit directions, radii, a
torus's radii and a revolved face's meridian (drawn in distance from the
axis and height along it) times the stretch, half-angles unchanged, a
plane's normal by `N`, so it still points out. Under any other (a scale
per axis, alone or composed) circles become ellipses: planes stay planes
and `Quadric` forms map as claims; a circular cylinder's quarter circle
square to its axis is mapped and projected along the mapped axis onto the
plane square to it (an affine map: the conic of the mapped control
points, same weight), a `Cylinder` again if that stays round (equal
stretches across the axis, judged within `1e-12`, naming intent only),
else a `ConicCylinder` over it; a conic cylinder's conic is mapped and
projected the same way; a cone whose circles stay round and square to
its axis (a scale along it) stays a `Cone` with `tan` times the stretch
across over the stretch along, any other cone (both nappes) and a sphere
become `Form::Quadric`; tori and revolved conics become `Unknown`. Each
face's `slack` is multiplied by the stretch when it is above 1. A mirror reverses every triangle: corners `[a, b, c]`
become `[a, c, b]`, halfedge `i` becomes `2 − i` with its start the old
next corner and the same edge record, and its pair the old pair's image;
the patch is the same surface, facing the other way. Names: with `copy:
Some(Instance { feature, index })` every face's name becomes
`FaceName::copy(feature, index)` and every alias `FaceKey::copy`, the same
mix of the parent instance, the feature and the index; `None` keeps the
names (a move). The result goes through `check` (rounding can bring hulls
a hair closer, so a solid at the margin can fail with `Invalid`, as a
scale down taking detail under the resolution does), charged
5 units a patch plus the volumes the check integrates.

`assemble(parts, tol, budget)` makes one solid of several, such as a
pattern's copies. Empty parts are dropped. Pairs of parts whose boxes
come within the resolution (a BVH over the parts' boxes, a unit a pair)
are linked unless they can go side by side: no vertex of either within
the other's box (grown by the resolution), and every pair of their
patches whose boxes come that close has hulls more than the resolution
apart (GJK, as `check`'s non-neighbours; a unit a vertex, a patch and a
pair). The vertex rule is what keeps nested parts from being put side by
side: a part inside another's material has its surface apart from the
other's and every vertex in its box. Each connected group of linked
parts is unioned pairwise in a balanced tree by index (`0 ∪ 1`, `2 ∪ 3`,
…, then those results), each `boolean` with `budget`; the groups'
results, in the order of their lowest parts, are concatenated (vertices,
edges, triangles, faces and aliases numbered on, more than `MAX_PATCHES`
patches `TooComplex`) and checked within `budget` (5 units a patch and
the integration). One result is returned as it is. Side by side can't
be wrong: a part in the material of several others has each vertex in
the material, so the box, of one of them, and is linked; a part in a void
that only several others close (in none of their boxes) goes beside
them, which is right, its shell facing out where nothing is solid. The
check confirms it (orientation, and hulls apart after the unions, whose
fitted pieces could stray by the fit), so a mistake is an error, never a
wrong solid. Spaced copies cost a pass over their vertices and patches
and the check: linear.

Tests (`transform/tests.rs`), on an L-shaped extrude with a round hole
and a half-ellipse top (planes, a cylinder, a conic cylinder): moves
shifting every vertex to the bit and keeping names; quarter turns about
each axis exact (four of them the solid to the bit, `(x, y, z) ↦ (−y, x,
z)` about z) and the sine and cosine of multiples of 90° either side of a
turn; turns about tilted lines keeping volume and area to `1e-12`, tags
and forms, and turning back to `1e-13`; mirrors facing out (volume
positive, plane forms and tags along the patches' normals), twice the
same triangles; two mirrors a half turn to the bit; mirrors in planes
square to an axis exact for normals of any length (`1e-300` to
`1e300`), slanted ones the same mirror to rounding; copies' names and
aliases renamed and resolving; a mirrored copy apart (side by side, the
two meshes as they were), flush and overlapping its source with analytic
volumes; 100 copies of a pin within a budget linear in the patches; a
row of touching cubes one box of six faces; a cube inside another and a
ring of pins by quarter turns; a cube floating in the void that six
flush plates close (the plates unioned, the cube beside them, volume
57); spokes overlapping at a hub against the
unions chained; out of bounds refused (and non-finite motions not made);
the budget; determinism at 1 and 8 threads. Revolved cones and spheres
moved, turned and mirrored (volume, area, tags, the forms' apex, axis,
half-angle, centre and radius). Scales (`transform/tests/scale.rs`): a
box, a revolved cylinder, cone and sphere scaled uniformly (volumes `f³`
times to `1e-12`, tags, forms' kinds and sizes) and per axis (volumes
`sx·sy·sz` times, cylinders circular or conic, cones circular when
scaled along their axis, else quadrics, spheres quadrics); a cylinder
off the axes scaled and turned; a sphere scaled per axis an ellipsoid to
`1e-12` on its exact strips, its fitted caps within the stretched fit;
powers of two exact to the bit (points, weights, plane and quadric
tags) and back; a torus scaled × 25.4 (slack 25.4, the debug form check
failing without it, slacks multiplying and kept on scales down; per axis
`Unknown`); scales down under the resolution refused; factors and
bounds; determinism; per-axis and uniform scales composed with turns and
slanted mirrors over the L, a cone, a sphere, a torus and a spheroid
(revolved half-ellipse): volumes `|det|` times, tags, plane forms out,
slack times the stretch, kinds kept by the uniform ones, nothing left
round by the others.

## Booleans (`src/boolean.rs`, `src/boolean/`)

`boolean(a, b, op, tol, budget)` gives `a ∪ b`, `a − b` or `a ∩ b`
(`Op::{Union, Difference, Intersection}`) as a `Solid`, and `touches(a,
b, tol, budget)` whether two solids meet, running only the broad phase
and one counting, with curved patches followed by a search for surfaces
within the resolution where the counting shows nothing (see "Touches"),
and nothing, answering false, for solids whose boxes are more than the
resolution apart, so asking it of far bodies is cheap. They follow
Manifold's `boolean3.cpp` and
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
| `boolean.rs` | `Op`, `BooleanError`, `UP`, `Cross11`, the `Primitives` trait, `boolean`, `touches`, building the mesh, naming failed results that touch themselves (`pinched_named`, `pinched`, `apart`), `parts` (connected parts, for the rays and the clean-up) |
| `boolean/input.rs` | `Input`: an operand's tables (corners, edges' ends and triangles, boxes, patches, which edges are straight and which patches flat or planar), vertex normals, flat volume |
| `boolean/curved.rs` | `Curved`, the primitives with curved patches: ray-derived shadow crossings, layers above a vertex, crossings of an edge through a patch, ties |
| `boolean/curved/ray.rs` | the ray tests `ρ` (exact for straight edges and at every edge's ends) |
| `boolean/curved/arcs.rs` | where two edges' shadows cross: one conic written implicitly, the other put in, a quartic |
| `boolean/curved/solve.rs` | points of a patch above a vertex, an edge's crossings through a patch, and a certified distance to a patch: subdivision and Newton |
| `boolean/curved/bernstein.rs` | Bernstein polynomials: products, evaluation, root isolation |
| `boolean/pairs.rs` | each pair of faces' ends and arcs; for curved operands the certificates, the refinement loop (`refined`) and the fixed rules |
| `boolean/near.rs` | `touches`' search for surfaces within the resolution: pairs of patch pieces split depth first until their hulls are apart or both are flat (`search`, `settled`, `near`) |
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
| `boolean/cleanup/seams.rs` | curved edges between two triangles in one plane: straightened, regions triangulated again, the plane faces they joined merged (for the sliver flips after them) |
| `boolean/cleanup/fold.rs` | sheets folded onto a flush face, their two sides triangulated differently: the folded vertex moved within its star's planes; its tests on a box whose top is folded |
| `boolean/cleanup/quality.rs` | refining the plane faces the boolean cut for their triangles' shapes |
| `boolean/tests.rs` | boxes in every flush, edge-on and vertex-on configuration, tori, determinism |
| `boolean/curved_tests.rs` | cylinders and boxes (exact), crossing cylinders, a free surface, a saddle, extrudes, chains, merging, random bars, walls over arcs with level ends |
| `boolean/curved_tests/flush_seams.rs` | flush unions with curved rims in either order: bosses in and on plates, over holes and edges, overlapping, a flange at a shaft's foot, a slot, at millimetre scale and on a turned frame, a chain of flush joins, caps a hair apart, bosses on a rounded corner |
| `boolean/curved_tests/one_face.rs` | faces on one surface after booleans: tops at a crease either side of the bar, flush stacks on turned frames far from the origin, chains of joins and cuts with every operand's names resolving, faces meeting only at a corner |
| `boolean/curved_tests/tangent.rs` | tangent contacts: cylinders against a plate's side from outside and inside, standing on it or through its top, slots ending in, beside and across a hole, a cylinder on a cylinder (in millimetres at the default tolerance, and at unit size at the finest), unions touching along a line refused at once (a pin plugging a hole it touches inside never named so), and solids tangent to a rounded edge or the faces it runs into |
| `boolean/seeded_tests.rs` | the seeded random suite: related pairs, parts built in chains of twenty, turned solids, near tangencies, pins and coaxial cylinders, flush bosses, bosses sunk through drilled plates |

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
`Orient`, `|det[g, e, UP]| / |UP|` for `Height` (never below that
product's own rounding, `4ε·|g|·|e|`: see "At every order"), the
triangle's normal's
length for `Reach`, `|RAY|·|(d − c)·ACROSS|` for `Ahead`) as zero, and go
on to the perturbation's powers. The scales make the tie one distance
for every predicate, the one the curved primitives use for heights (a
review found `Height`'s 32 times larger and `Orient`'s and `Ahead`'s
larger along steep or ray-wise edges). So a configuration within the tie
distance of a tie is decided as the tie it stands for: faces flush in
exact arithmetic but turned and moved, every coordinate rounded, merge
or part cleanly as the unmoved ones do (of 96 random flush grid boxes'
operations turned and moved, 95 now work, 71 did with exact signs), and
the exact predicates and the curved primitives, which decide heights
that close as ties too, see one configuration: a cylinder `1e-9` off a
box's face was beside it for the exact tests and on it for the
numerical ones, and the winding numbers disagreed (then `InsideOut`,
now `Inconsistent`). The
positions (`Across`, `Between`, `crossing`) stay exact. It is used for
flat operands too.

**At every order.** Once the constant term is taken as a tie, each later
coefficient that is only rounding is taken as zero too. At the
exact tie a rounded configuration stands for, the first order is often
zero as well, and the tie is decided by `T2` or `T3`: two collinear
edges' `Height` (both first-order terms have parallel columns), a
vertex whose `n_v` lies in the other face's plane (on the edge between
a `+x` and a `+y` face, against a `z` face) for `Reach`. Rounded, that
coefficient is some `1e-16` of its terms, its sign noise, and taking
it decided each predicate as a different nearby configuration, so the
counting's ends didn't pair up (`Inconsistent`). Skipped, every
predicate is decided as the exact tie is: one configuration. Only
rounding means within what moving every number the coefficient is worked
out from by a share of itself can make of a zero: `exact::Moves`
evaluates `Σ |∂c/∂x|·|x|` over those numbers to first order (each
product's other factor weighed by its value, in floating point, only on
that slow path), and the share is `RHO = 2⁻³²` but at most the tie
distance over the largest of them, so no number moves by more than the
tie distance and the decision is still that of operands moved by less
than it, also far from the origin. (A first measure, the expression on
its terms' absolute values, grew as `|x|²` where the coefficient grows
as `|x|·L`: `2¹⁷` from the origin it took a vertex's direction 70° into
a face, or an edge `1e-3` above a parallel one, as rounding, and the
second order decided them the other way, and of the turned pairs below
moved by up to `1e5` or `1e6` it refused some 390 of 2 700; with
`Moves`, they come out as moved by up to 100, and its tallies here and
the seeded suite's are the same as the first measure's. `2⁻⁴⁶` decided
the same as `2⁻³²`.) With every later order only
rounding the sign is 0, as for a tie in every power. `sign` (tie 0)
stays exact. Before and after (release; turned grid boxes as in the
tests, seed 31, and chains of up to six, seed 77, each result fed on;
unturned grid-box chains, seed 5):

| run | before | at every order | and parallel shadows |
|---|---|---|---|
| 900 turned pairs × 3: right, `Inconsistent`, `Invalid` | 2 638, 13, 49 | 2 651, 2, 47 | 2 653, 0, 47 |
| 100 turned chains: right, `Inconsistent`, `Invalid` | 334 of 356, 12, 10 | 349 of 359, 0, 10 | 349 of 359, 0, 10 |
| 300 unturned chains of six: right, `Invalid` | 1 754, 46 | 1 754, 46 | 1 754, 46 |

No result was wrong. Of the `Invalid`s, 45 before and 46 after are
operations whose exact result isn't a manifold (one of the 13
`Inconsistent`s was too, now `Invalid`); the one other is pair 866's
union, a thin triangle lying across two faces that fails the hull
rules (a numerical oracle deciding every predicate on the configuration
moved by a real `ε` fails it too: not a sign).

**Parallel shadows.** `Height` is of second order where two edges are
collinear: its constant term, `det[g, e, UP]` and every first
derivative vanish there. Turned and rounded, `det[g, e, UP]` (its
scale) is only rounding, down to an exact 0, and with a zero scale
`sign_tied` fell back to exact signs, so the constant term's own
rounding decided (pair 100 of the turned pairs, its intersection and
difference `Inconsistent`, the last two above). Now `Height`'s scale
is never below that product's rounding, `4ε·|g|·|e|` (shadows within
`1e-15` of parallel, where a height at their crossing means nothing),
and a zero scale with a positive tie no longer drops the tie: only an
exact zero is then one, and the later orders that are only rounding are
still skipped. The first order needs no scale of its own (a separate
`ε¹` tie for `Height` was planned): with the rounding rule above every
manifold result of the 900 pairs is right but pair 866's.

**Shadows along each other.** Where both ends of `e` are on `g`'s
shadow to within the tie (`exact::is_tie` on their `Orient`s), where
the shadows cross and the side `σ` are the perturbation's, and the
configuration as decided has `e` in the plane through `g` along `UP`:
`det[g, e, UP]` is zero there, and so is `Height`'s constant term
`det[a − c, g, e]`, whatever the gap between the edges. `e_above` then
takes that constant term as a tie (`exact::sign_past_tie`), and the
perturbation decides by the gap. Taken as it came (the gap times only
rounding, or times a hair's angle), against a side that wasn't its
own, it put the edges either way round however far apart, and the
decisions still fitted together: turned grid boxes, the second moved
along `UP` off flush so that their flush edges' shadows lie on one line
(seed 5, 1 000 pairs × 3, also a million from the origin), gave a
union `1.8e-5` apart `4e-6` short and an intersection `2.8e-4` apart
`1.4e-5` over: wrong results, from before the rule at every order (the
retry never saw them). Now none is, and with the retry turned off none
is `Inconsistent` past the resolution. Edges stacked exactly along
`UP`, a direction off every axis, are rare in CAD geometry, but not
impossible.

**Decided again exactly.** Near ties taken as ties can still give
decisions no one configuration has, mostly where things are about the
tie distance apart. For flat operands (`boolean::flat_soup`; `touches`
the same for its counting) an `Inconsistent` from the counting, the
pairs' ends or the assembly is decided again with `tie` 0: exact signs
are those of the perturbed operands, a real configuration, so they fit
together, and the worst outcome is `Invalid`. The second try spends
from the same budget, only on that failure. The curved path has no
retry: the `Flat` inside `Curved` must keep the curved primitives'
ties. Turned grid boxes with one moved by `10^±1.5` tie distances
(seed 5, 3 000 operations): 60 `Inconsistent` with the ties (100 before
in-plane crossings, below, 74 before the flat broad phase's margin and
crossings of edges with one end in the plane), of them 34 right and 26
`Invalid` (parts closer than the resolution) once retried, none
`Inconsistent` and none wrong; 2 624 of the 3 000 right (2 617 before
in-plane crossings).

**In-plane crossings.** The tie is one distance, but not measured the
same way everywhere: `Reach` measures it square to the face's plane,
`Height` along `UP`. On a plane at angle `θ` to `UP`, two edges a gap
in `(tie·cos θ, tie)` apart square to it are tied for `Reach` and not
for `Height`. Flush faces a hair apart (boxes extruded on frames turned
and moved by `1e-11` to `1e-7`) put gaps there all the time: an edge of
`A` with both ends within the tie of a face of `B` (decided as lying
in its plane) still crossed it by the counting, the edge passing under
one of `B`'s by `Height`. `crossing` gave where the edge's line meets
the plane as rounding has it, anywhere along the line, here beyond the
edge and clamped to an end half a unit outside the triangle crossed. The
loop through two such crossings wound the wrong way, and the result
(the volume right, the release check passing) had the tool's side
filling the target's flush face, facing against its own plane tag
(about 1 operation in 750 on such frames; debug builds panicked in the
form check). So for an edge decided to lie in a flat face's plane (both
ends' `Reach` tied, `exact::is_tie`; only with a tie, and only against
a flat patch, `Input::flat`), `Flat::crossing` keeps the crossing to
the part of the edge inside the triangle seen along its normal, each
side `a → b` widened by the tie (`((b − a) × (x − a))·n ≥
−tie·|b − a|·|n|`, linear along the edge; `Flat::inside`). The vertex
moves along its own edge, so it stays on the faces beside the edge, and
within the tie of the crossed plane, so on its patch to the resolution;
only positions change, in floating point. If no part of the edge is
inside, the decisions fit no configuration: `crossings` gives
`Inconsistent`, which the flat path decides again exactly (the curved
path, with no retry, fails). Without the widening, edges along a
triangle's side gave empty intervals from rounding (9 to 15 more
refusals per 4 000 turned chains). Measured on random chains of 5
steps, each result fed on (release): grid boxes on hair frames, 4 000
chains at each of three seeds, went from 22 to 26 results with a
triangle facing against its tag to none, with 41 to 51 more `Ok`s of
about 11 000; polygons on hair frames (2 500 chains) from 23 to none
and 15 more `Ok`s; cylinders on hair frames (1 500) from 8 to none and
2 fewer `Ok`s (`Inconsistent` 272 → 260); turned frames 6 and 7 more
`Ok`s; plain frames the same. No result had a wrong volume, and the
check's facing test refused none of them: these crossings were the only
source seen. Against the booleans' refusal of a triangle facing against
its form (which turned the same results into `Invalid` errors, 13 to 15
per hair grid seed), the keeping wins 56 to 64 `Ok`s per hair grid
seed, 25 on polygons and 21 on cylinders (`Inconsistent` 178 → 148),
and the seeded tallies are the same. Making the tie one measure for every
predicate (`Height` square to the edges' plane, say) would remove the
cause, but changes every flat decision near a tie and would leave such a
window between some pair of predicates whatever is chosen; the release
check's facing test (step 6 of `check`) backs this up wherever else a
plane patch faces against its tag.

An edge with one end decided as on the plane (its `Reach` tied) and
the other within the resolution of it is nearly along the plane too, so
rounding still moves its crossing far along it: polygons on hair frames
gave a crossing 0.17 outside its triangle that way (the other end
`2.1e-8` off a plane, the tie `1.56e-8`), and the tool's side, touching
the target from outside, came out on the target's side facing against
its tag (refused by the check). Its crossing is kept inside the triangle
as well, where some part of the edge is; where none is, it stays where
rounding has it rather than fail (the curved path has no retry). On hair
frames (with the flat broad phase's margin, see "Counting") that won
back 30 more `Ok`s over 11 runs (grid boxes +3 to +7 a seed of 1 000
chains, polygons +1 to +6 of 600, mixed +6, cylinders −1), refused none
for facing against a tag, and changed nothing on turned frames.
Putting the crossing at the tied end instead (the decided
configuration's place for it) did as well on mixed and polygon runs and
worse on grid boxes.

The seeded suite's tallies didn't move but for one step: the rule at
every order took chains from 203 to 202 of 240 (of its fed-on steps
the same inputs give two more right and one fewer,
`Invalid(VertexNeighbours)`; the rest follow from different results
fed on); parallel shadows, the retry and `first_sign`'s rounding rule
(which skipped 173 orders in the suite) changed none (related 112 of
120, chains 202 of 240 with 6 `Inconsistent`, turned 156 of 160,
tangent 72 of 96 with 11, coaxial 37 of 40, bosses 64 of 64, drilled
160 of 160). So curved operands, which share `sign_tied` through the
`Flat` inside `Curved` and the rays' `Beside` and `Ahead`, aren't
worse.

### Counting (`boolean/count.rs`)

1. **Broad phase**: a BVH over each operand's patch boxes; the pairs
   (triangle of `A`, triangle of `B`) whose boxes meet (`≤`, so touching
   boxes count: `A`'s boxes looked up in `B`'s BVH), counted against the
   budget before they are collected (`Bvh::hits_within`). The margin is
   the primitives' (`Primitives::margin`): 0 for exact ones (the flat
   primitives deciding again exactly, `tie` 0), the resolution for
   curved ones, which take edges and patches as straight or planar
   within it and decide heights within a 64th of it as ties, and the
   resolution for the flat ones deciding near ties as ties too.
   So every pair such a decision touches is counted: with margin 0, a
   cylinder's seam vertex `1e-9` from another's wall (at the coarsest
   tolerance) was decided as on it by its ray, no pair of triangles met
   to carry a crossing out, and the whole cylinder came out inside the
   other. The flat primitives had margin 0 after they took near ties as
   ties, and boxes at a hundredth of the grid's size off the origin,
   chained, left vertices of one an ulp inside the plane of the other's
   side through its middle: decided as on it and, by the perturbation,
   beyond it, their edges crossed that side, but the triangles on its
   near side had boxes an ulp short of it, so nothing counted a crossing,
   and the intersection came out empty and the difference whole, both
   `Ok`. A tie of `Reach` or `Height` is a tie distance at most, but a
   shadow's along `UP`, on a face steep to it, reaches further, so the
   margin is the resolution, as for curved ones. From the pairs the
   candidate edge–face pairs of each operand, sorted.
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
two coordinates in pieces whose normals all lean one way along `UP`; a
point Newton's method finds from one piece in another is kept too, if
it is in the triangle and the search ran out of pieces: on a steep
patch every piece's shadow holds the vertex's, the search ran out
before it got to the one holding the point, and the winding number's
point, taken to be above, gave every edge at the vertex a crossing
through the patch that wasn't there; where the search didn't run out,
that piece finds its own, and a second find near a fold, where Newton's
method converges loosely, would count it twice; and of those found from
other pieces, one facing a way is kept per point, those within `1e-4`
of the parameters of one already kept, facing the same way, merged:
seen nearly along a wall, the line along `UP` grazes it over a stretch
where the shadow is within Newton's residual of the vertex's, and each
piece's start ended somewhere else along it, ten finds `1e-9` apart),
their facings made to add up to `ω(v, f)` (a shadow covers a point as
often as its boundary winds round it, counted by facing) by adding the
nearest found just outside the triangle or dropping those inside nearest
its sides, and those above counted.

**Crossings.** A straight edge through a planar patch: at most one, at
`Flat`'s exact position on the corners' plane. Otherwise solved for
(`solve::edge_patch`: edge and triangle split together, pieces dropped by
their boxes, by a slab along the patch piece's normal, which a
tangency needs, and where their control points' hulls are apart
(`hull::apart`, GJK, in the search's frame scaled to the unit box; the
edge's and the patch's pieces lie in their control points' hulls, the
weights being positive), which a tall or long patch needs: a cap edge's
box held the whole width of a drill's wall 1 000 tall and 4 wide, so
only the height pruned pieces, and the row of pieces at the edge's
height was split across the width until the search ran out of pieces,
more than 1 024 against 23 to 33 now. "Apart" is by more than `1e-12`
(far above the rounding of the blossomed control points) plus
`2048 · PIECE_SLACK` (about `2e-6`) of the two pieces' sizes: Newton's
solutions count for a piece up to `PIECE_SLACK` outside it in its own
parameters, and one just past the patch's side or the edge's end (a
hit the count may want, see below) lies that far outside the hulls at
the weights' bounds; with `1e-12` alone, a crossing a twentieth of a
nanometre past a cylinder patch's side was found without the hulls and
lost with them. The boxes and the slab are apart by more than the same
(each holds the hull, so a point that near the hull is that near them
too, and the slab's margin is far under a curved piece's bulge until
pieces are a few millionths across): with `1e-12` alone, a crossing just
past a side square to the axes (a wall's level rim) was lost where the
edge's range was split between the rim's height and the crossing, or
where the edge ended just short of the wall, the edge's piece beyond the
rim parted from the patch's by its box, or by the slab. A patch's piece
more than 8 times longer than high across its longest side is halved
across that side rather than quartered (`MAX_ASPECT`): quartering a pin's
wall 1 000 tall and under 2 wide split it across its width as often as
along its height, and a hole's rim arc through it, whose control hull
holds the wall's width near the rim's height, had every piece of that row
to look at (965 pieces against under 200 halving). Newton on `E(t) = P(u)`; once it finds a crossing in a
piece, the rest of the edge's piece either side of it, less a thousandth
of it round the crossing, is searched again: an edge running through a
wall a little inside its rim, in and out within one small piece, lost
the second crossing, the count 0 dropped the first, and the edge was
taken for not crossing at all), and made to add up to the count the same
way; ones the search didn't find go where it found the two meeting but
kept no crossing (a hit the count overrules, as where the edge leaves a
vertex on the patch's corner: nearest the patch), else, on a planar
patch with just one end of the edge within the resolution of its
corners' triangle, at that end (`end_on_triangle`), else where they came
closest (the middle of the smallest pieces the search looked at, which
had put such a crossing a sixteenth of the edge from the vertex). The
end: a cylinder's seam edge a fraction of a tie off a box's side has its
rims and its wall's diagonal leave its ends on the side's plane, the
count crosses them there, and the search, its roots a rounding past the
ends, finds nothing; put where its closest pieces were, up to a 128th of
a quarter arc along the edge (thousands of resolutions at fit 0.001,
and the later root finding, its roots outside the edge, kept them
there), the cut ran through those points, and the result was some 40
resolutions times the cut's area off (a pin less a box, or, with walls
along one direction joined, a pin poking into a quarter tube's corner
at the tube's end).
Those are marked as not solved (`Crossing::solved`, from `pick`): on a
plane or a quadric they go to the edge's root on the patch crossed once
placed (see "Cutting curved faces"), and they must lie on the other
operand (see "Assembly").
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
patch's corners' interpolated. Each of these is linear in the motion,
and an order whose value is only rounding (within `RHO = 2⁻³²` of its
terms, `Σ |d_i·f(e_i)|`, the motion square to the gradient but for
that) is taken as zero, as the exact predicates' later orders are (see
"At every order"): a vertex moving along a turned plane against a patch
on it gave `δ·n` some `1e-17`, its sign noise (the seeded suite's
tallies are the same with and without it). Where every order is 0, by
`δ·UP`, as for horizontal surfaces. So flush planar faces between a curved and a flat
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
  of its terms), solving that polynomial gave random roots. Where the
  two edges also lie on each other in space over a stretch (`e`'s ends
  on `g` and `g`'s on `e`, within the tie, give it; three points
  between must be on `g` too: lifts of one conic shadow meeting at
  three points of it are one curve), the crossings are decided one by
  one (`Curved::along_crossings`): `A`'s perturbation `δ` takes `e`'s
  shadow to `g`'s left where `δ·(UP × g')` is positive, so the
  perturbed shadows cross where that changes sign, `σ` its sign after.
  With `e`'s tangent's homogeneous numerator `h` (a quadratic in
  Bernstein form: `w·(c − p0)`, `(p1 − p0)/2`, `w·(p1 − c)`) turned by
  `o = sign(e'·g')` for `g'`, `o·δ·(UP × h)` is a cubic (`δ` linear
  along the edge), whose roots in the stretch are the crossings, each
  above or below by its own rise (the parallel rule above), its `σ`
  the change of sign across it, from the signs at the midpoints
  between the roots; where one of those is zero to rounding (a
  near-double root found as two roots a rounding apart, or twice at
  one place), the change across both goes to the later root, none
  across a tangency (the sign after a root alone had counted a
  tangency as a crossing, and its pair the other way); zero to
  rounding (every coefficient within `1e-12` of its terms' size), `T2`
  and then `T3` take `δ`'s place. Roots within the tie of the stretch's
  ends (either side: rounding may put one just outside) are left to the
  count (as crossings at an end), and whatever the
  count has beyond those found goes by the rise at the first such root,
  or at an end of the stretch where the cubic is zero to rounding (a
  root there may come out just outside and not be found), else the old
  way. Such zeros are what a circle drawn through exact axis points
  gives on YZ: a rim vertex's `δ` lies in the upright plane of the
  tangent there to the bit, and the old way, the height a sixteenth
  along, split those crossings wrongly (identical cylinders failed as
  before). The old way is for every crossing the ray tests count to go
  the same way, by the heights
  where the two come nearest or, tied there, the parallel rule: still
  so where the curves are apart in height (a top rim seen along the
  axis over a bottom one). On XY a rim's perturbed shadow never crosses
  the other's (`δ`'s part off the circle is radial throughout), so
  nothing changed there; where a cap's plane lies nearly along `UP`
  (the XZ and YZ sketch planes, or any upright one) the rim's shadow is
  a thin ellipse round whose ends the perturbed shadows cross once or
  twice, `A` above at one crossing and below at another. One sample's
  height for all of them gave a cylinder's two coincident walls (`A`'s
  and `B`'s) ends, and a flush pin in its hole, a second extrude of one
  circle or a cylinder cut back on those planes failed as
  `Inconsistent`, every operation on YZ. Sampling the polynomial
  instead of solving it would miss close pairs of roots. Measured in
  release at the default tolerance with a sweep of 1 665 operations
  (a cylinder of radius 1 in 4 arcs over 0..1 against the same circle
  in 4 arcs or in 3 turned 0.7, over 9 spans; square prisms over the
  same spans; pins in a plate's hole over 5 spans; union both ways,
  intersection and differences both ways) on 9 frames (XY, XZ, YZ, XY
  moved, an upright one off the axes, and four turned ones): failures
  243 → 44, `Inconsistent` 235 → 34, none on XZ or YZ (94 → 0), none
  for cylinders or pins; with the circles through exact axis points, 46
  failures. Every volume came out within `1e-12` of its closed form. Of
  the 5 130 decisions it made, the crossings found added up to the ray
  tests' count in all; with exact axis points 90 differed, each by one
  crossing at an end of the stretch where the cubic is zero, and none
  went the old way. The seeded suite's pins and coaxial cylinders (now
  on five frames, 200 operations) 164 → 191; the other seeded tallies
  didn't move (related 113/120, turned 156/160, chains 203/240,
  tangent 72/96, bosses 64/64, drilled 160/160).
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
- **Crossings at an end lying on a plane.** A curved edge whose end lies
  in a planar face's plane (within the tie, inside the face further than
  a tie from its sides) crosses it near that end or not as the
  perturbation puts the end on the other side of the plane from the
  edge's inside next to it (`Curved::end_on_plane`, by `Flat::plane_side`,
  the predicate `s02` takes the plane's side from; an edge leaving the
  plane along it is left to the count). An edge with both ends in the
  plane and its inside on one side crosses near both ends or neither,
  which the count (how many more go in than out) can't tell apart; the
  search found the two, one a rounding outside the edge and one inside,
  and the count kept neither. So the cut of a boss's wall by a plate's
  top, between two wall vertices at its height joined by a curve dipping
  into the plate, ran in the triangle below the curve instead of the one
  above (`Invalid(Fold)`), and drilling a plate twice sometimes failed.
  It also came out wrong where the check couldn't see it: on a turned
  frame, a hole drilled through a ring sunk in a plate (its wall split at
  the plate's faces) took `1.8e-6` too little, a sliver left where the
  ring's wall meets the plate's bottom
  (`a_hole_through_a_sunk_ring_on_a_turned_frame`).
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
ends, their control hulls apart (GJK) or **walls along one direction**
that come near each other (below). Then no ends is no cut, two ends
are one arc, and more ends of two planar patches join in order along the
line their planes meet in (if they alternate). Without the cylinder
certificate a box's side against a boss's wall along it refined without
end. The argument holds for the exact plane and cylinder; the patches
are only within the resolution of them, so a loop the certificate
misses lies within about the resolution of both surfaces (a near
tangency),
and leaving it out moves the result by less than that.

`along` takes the null direction of the quadric's matrix along which
its linear part vanishes too: for a matrix of rank 2 (a circle, ellipse
or hyperbola swept) the rows' largest cross product; for rank 1 (a
parabola swept, from a conic segment of weight 1: the matrix is `g·gᵀ`,
whose null directions are a plane) the direction square to the largest
row and to the linear part. A plane cuts a parabolic or hyperbolic wall
in lines along it or in an open conic, so the plane-and-cylinder
certificate holds there too.

**Walls along one direction** (`parallel_walls`): both faces claim
cylinders (`along`) whose directions are parallel within the resolution
over the pair (`|d1 × d2|` times the diagonal of the pair's boxes at
most the resolution: lines along each drift apart by less than that
across the pair; extrudes from one normal agree to rounding). Such walls
meet only in lines along that direction (their cross-sections' common
points, swept), and a line runs out of both patches through their edges,
where the counting gives it ends: with no ends there is no cut. Sound
for any pair, but certified only where the walls come within
`PARALLEL_NEAR` = 64 resolutions of each other (a point sampled on
either patch, `samples`, that near the other's quadric). Walls further
apart are refined as before until their hulls part, though they don't
meet either, and later stages rely on that refinement: where a cap's
flat ring lies between two such walls (a cylinder inside a larger one
sharing its top, cylinders of radii 1 and 1.001 stacked, a coaxial one
0.004 larger over part of the other's span), the ring is cut along both
rims and triangulated from the walls' pieces; from unrefined walls,
whose quarter arcs bulge across a ring narrower than their sag, its
triangles fail the hull rules against the walls' (`Hull`), or, where a
rim's quarter runs through a cap vertex in line with its ends, the ear
cut there has three corners on a line and a curved side and folds.
Refined until their hulls part, the pieces bulge less than the ring is
wide. Walls within the resolution of each other (a tangency) never part:
before, they were refined until their pieces were flat (about
`√(8·R·resolution)` across), the pairs along the line doubling every
round, and a line contact of a few millimetres at the default tolerance
ran out of the budget (3 to 4 million units, 3.6 s and more). Now a
tangency's refinement stops once a sample of its pieces comes within 64
resolutions of the other wall: pieces about 0.1 across for walls of
radius 1 at the default tolerance. Walls closer than 64 resolutions
with a ring between them are left to fail (see Deviations for the round
gate this replaces).

**Ends along one direction** (`along_generators`): a pair on such walls
that has ends and no certificate is joined line by line before it is
split. Each line's stretch inside both patches runs between two ends
where it leaves one or the other, at the same point of the
cross-section (`at − d·(at·d)`), with opposite signs, apart along `d`.
So the ends are grouped by their cross-section point (single linkage),
and if every group is exactly two such ends, each group is an arc; any
other grouping (one end, three, four: two lines a hair apart, a line
leaving and coming back; two ends at one place along `d`) leaves the
pair to be split as before. A line is joined only where it is **clear**:
at every end of the pair the walls cross at an angle `θ` with
`θ² ≥ 2·LENS·resolution·κ` (`LENS` = 1/4, `κ` the sum of the two
cross-sections' curvatures there, `tᵀ·H·t/|∇F|`; `sin θ` for `θ`). Two
curves crossing at `θ` bound a sliver that closes no sooner than `2θ/κ`
away and is at least `θ²/2κ` thick, so a clear line's sliver is at
least a quarter of a resolution thick (16 tie distances), wherever its
other line lies, in the pair or not. Thinner ones (walls overlapping by
less, down to crossings only the counting's ties make) are left to
refinement, whose later counts may drop their ends and give the
operands unchanged; joined in an early round they fold (at `LENS` =
1/2000, the tangent suite's differences at fit 0.1 overlapping by
`1e-6`, a hundredth of the resolution, did). For two walls of radius
`R` side by side a line is clear at least `√(R·resolution)` from the
other. Ends group within `θ/κ` (the pair's smallest angle, largest
`κ`): half the distance to another clear line. The joins are sound for
the same reason as the certificate: such walls meet only in lines along
`d`, and a group of two is one stretch of one line.

Measured on unit cylinders side by side overlapping by 0.02 to 30
resolutions at fits 0.1, 0.01 and 0.001 (four placements, four
operations: 384), `LENS` from 2 down to 1/8 turned no result into an
error against no joins at all, and every volume was right; 1/16 turned
one. Results: 61 without the joins (176 spending over a million units,
565 M units in all), 177 at `LENS` 2 (106, 356 M), 214 at 1/4 (65,
205 M), 229 at 1/16.

Probed again against exact volumes (the lens by Green's theorem from
the curves' crossings; each result to `1e-10`, or, where the lens is
thinner than the resolution, within a skin a resolution thick over
it): an ellipse's tip, flank and side (half-axes 2 × 0.5 and 3 × 0.3,
curvature up to 33) against a unit circle's wall; pins of radius 0.5
to 0.999 poking out of a hole's wall (curvatures nearly cancelling);
unit walls on the XY plane, on a frame turned off every axis and on
one some 3 700 from the origin, the second's span inside the first's,
flush with its top, `1e-7` over it, `3e-7` under it, or overlapping it
by `1e-5` or half a resolution; walls a hair off parallel, in one plane
(a V from a gap at one end to an overlap at the other) or skew (a long
loop); overlaps from a twentieth of a resolution to a thousand, at fits
0.1, 0.01 and 0.001 (3 204 operations). No result was wrong. Where
the lens is thinner than the resolution an intersection may come out
empty while the differences cut it, and caps within the tie of each
other move a result by up to a resolution times their area, as without
the joins. Against no joins: ellipses 405 → 578 work (545 M → 114 M
units), the spans 207 → 511 (932 M → 123 M), the tilted walls 4 and 4,
the pins 258 against at most 94; but some results that refinement
got failed, so an operation that fails (but for running out) after
joining some lines is tried again without the joins (below). The grouping
holds by construction: lines along `d` cross the cross-section at
points at least `2θ/κ` apart, a line's two ends lie on one of them (up
to where the counting put them), so groups within `θ/κ` are the
lines'. An end the counting only placed at a tie may lie off its line:
a rim flush within the tie with the other wall's put one `0.3·θ/κ`
along it; asking each end to lie within `θ²/4κ` of both walls (a
quarter of the grouping distance across) refused it and lost the
results the join got right, so ends aren't checked. Leaving steep
crossings to refinement (`sin θ` over 1/8 to 3/4) won back as many
results as it lost, so every clear line is joined.

**Tried again without the joins** (`boolean_within`): a pair's lines
joined in an early round leave the pieces beside them as large as they
were, and some results that refinement got right failed the hull,
neighbour or fold rules from them (16 of the 864 ellipse operations
above, against 189 won; 17 of the 1 080 spans, against 323). So when an
operation that joined some lines fails, except by running out of the
budget, it is tried again without joining any
(`checked_with(.., join: false, ..)`, the pairs split as before),
within as much work again as the first try took, but at least `AGAIN`
= 150 000 units (and what is left of the budget). The second try's
result if it passes, else the first try's error, with its evidence (or
`TooComplex` where the budget, not the bound, stopped it). Unbounded, the second try ran
most of the refusals on to the budget for one result in fifteen
(ellipses 114 M → 322 M units, for 15); bounded by the first try's
work alone it lost the spans' fast folds (a sliver under the resolution,
folding in a few thousand units and refined in 50 000 to 850 000).
Measured (ellipses and spans as above, against the build with walls
along one direction neither certified nor joined): ellipses 591 work
(578 without the second try, 405 with neither), lost 3 against 16,
151 M units (114 M, 546 M); spans 537 (522, 216), lost 2 against 17,
189 M (124 M, 930 M). The three left need over a million units
refined. Results stay the same at 1 and 8 threads and on budget
ladders (a budget under what the result took gives `TooComplex`, never
another result).

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
floor (at pieces about `√(8·R·resolution)` across), where walls along
one direction don't end it sooner.

`refined` returns the operands as refined (the same surfaces, split; the
operands' vertices first, then refinement's), the tree of each operand's
red splits (`mesh::Node`: corners, patch, parent) and each refined
triangle's leaf in it, the counts and every pair's arcs (`Arc { tris,
plus, minus }`, by end vertex id): what cutting the curved faces starts
from.

### Touches (`boolean.rs`, `boolean/near.rs`)

`touches` on flat operands is one counting (tied as a union would be,
counted again exactly where the ties don't fit together) and its
`meet()`: an edge through a face or a vertex inside. With curved patches
it is one counting, `pairs::counted`, the very call `refined` makes each
round, so it is `refined`'s first round to the bit, and `true` on
`meet()`; otherwise a search for surfaces within the resolution `r`
over the broad phase's pairs, whose margin is `r` too (`near::near`),
whose answer it gives (both in `near::touching`). No refinement, no
Newton and no fallback.

Refinement isn't needed to tell whether two solids meet: it joins
crossing ends into arcs and finds loops no edge crossing shows, and a
loop no crossing shows is still a place where the surfaces come within
`r`, which the search finds. Before, `touches` ran the boolean's whole
refinement and read its last counting, which answered false for a
tangency along a line (cylinders side by side, a pin against a hole's
wall, a boss against a rounded corner: no edge crosses, no vertex is
inside, and the fixed rules take a pair with no certificate as no loop)
after running most of the budget, or out of it: 3.6 to 11 s at the
default tolerance, `false`, `TooComplex` or `Inconsistent` by where the
tangent line fell on the seams. A plane against a cylinder was already
certified. Stopping `touches` at the first round whose counting shows
a meeting (proposed on its own against a failing touch test costing a
whole budget on every draft change) is part of this: a meeting the
counting shows is `true` at once, and only that round is counted.

The search (`near::search`) runs depth first over pairs of pieces, a
stack per broad-phase pair in pair order, each pair's pieces visited in
a fixed order (the first's four `split4` pieces, each against the
second's), sequentially, so the answer and the work spent are the same
at any thread count and on wasm (no trig). A visit drops the pair, stops
the search or splits one piece or both. `near`'s visit drops a pair
whose control hulls are more than `r` apart (`mesh::apart`, GJK), stops
on a pair both `settled` (flat within `r/4`: control points within
`r/4` of the corners' plane, `input::planar`, and each edge's control
point within `r/4` of its chord, `mesh::flat`; or no larger than
`MIN_SPLIT·r` across), and splits the pieces that aren't. A settled
pair GJK calls near is dropped after all if the hulls are more than `r`
apart along the normal of either piece's corners' plane
(`near::apart_across`): GJK's direction to the closest points rounds
relative to the hulls' size over their distance, and on the round
octahedron's small pieces by a slab's large face at the finest
tolerance it stopped short at some 3 resolutions, `true` one way round
and `false` the other; near a tangency the pieces' planes are the
direction it was after.

Why it holds: a patch and its pieces lie in their control hulls
(positive weights), so surfaces within `r` are never dropped, and
`false` means they are more than `r` apart everywhere (up to the splits'
rounding, a few ulps of the coordinates a split, which nears `r` only at
the finest tolerance some `1e5` from the origin); with the counting
showing no crossing and no vertex inside, neither solid is inside the
other, so they don't meet. A `true` means two flat pieces' hulls come
within `r` (up to GJK's rounding where neither piece's plane
separates them), and a flat piece's hull is within `r/√2` of the piece
(`r/2` along the normal, both within `r/4` of the corners' plane, and
`r/2` across it, the piece covering its corners' triangle but for a
band `r/4` wide along its sides), so the surfaces come within
`(1 + √2)·r`, about `2.4r`; floor pieces only happen on surfaces
curving tighter than some hundreds of `r`. So curved solids within the
resolution touch, and up to about `2.4r` apart may, where flat ones
touch only within the tie distance (`r/64`), which is below anything a
user can place. `touches` only picks what an operation works on
(regen's targets), and every boolean decides for itself, so a `true`
can't make a wrong solid; at worst a body that grazes the tool is a
target whose boolean fails or is a no-op.

The search is what minimum distances run on (`measure::distance`, see
"Volume, area and measuring"): generic over what it visits (pairs of
patches here, tree nodes and pieces of patches, curves and points
there), with a visit that may change the item before it is split and
spend work of its own. Cost: a unit per visit (`NEAR_WORK`, measured 0.24
to 0.44 µs a visit, release, one thread), spent each 256 visits and at
the end; a piece `split4` can't split is `TooComplex`. Measured,
release, one thread, the counting included: tangencies 1.5 to 4 ms (up
to about 1 100 visits); near misses (gap `1.5r` to `10r`) at the default
tolerance 10 to 20 ms for cylinders side by side (30 000 to 45 000
visits, the same for 2 and 100 long ones), 12 to 45 ms for a pin off a
hole's wall (51 000 to 174 000), and under 6 ms at the coarsest
tolerance. The near-miss band (surfaces just over `r` apart along a
long line) is the dear case, bounded by the line's length over
`√(2Rr)` pieces, and charged: a hostile one runs out. At the finest
tolerance cylinders of radius 1 side by side cost 341 000 visits (about
0.13 s) at gaps of `2.3r` and more, and up to 2.6 million (about 1 s,
over half the default budget) at `1.2r`, where every pair must split
until its hulls clear `r`; the pin off a hole's wall 0.9 to 2.2
million. Such gaps can't come from rounding a tangency (far below
`r`), only from placing the tool `1e-8` off.

### Cutting curved faces (`boolean/chain.rs`, `boolean/surface.rs`, `boolean/assemble/`)

With curved operands, `boolean` takes `pairs::refined_with`'s operands,
counts and arcs, builds the operands' tables again and `Curved`'s
primitives (for `order`), and assembles as for flat operands, with these
additions.

**Crossings on curved edges.** Each crossing's parameter is solved again
where the face crossed is a plane or a quadric, when the edge is curved
or the face isn't planar (`surface::polish`), at a root of the edge
against the surface, exactly: a quadratic for a plane (the edge's
conic's Bernstein form against it) or for an exactly straight edge on a
quadric (`F(p₀ + s·d)`, with a discriminant a rounding below 0 taken as
a double root: a touching edge touches), else a quartic, `F(C(t))` times
the square of the conic's denominator, in Bernstein form (the
homogeneous control points put in the quadric's 4 × 4 form about the
edge's first end), its roots isolated by `curved/bernstein.rs` and each
given up to two Newton steps that bring `F` nearer 0, and with them the
quartic's levels (roots of its derivative) on the quadric to `1e-12` of
the edge's size: a curved edge touching the quadric is a double root,
which the isolation gives as none or two as rounding has it. Which
root:

- Solved by the search: the nearest on a plane (however far: the search
  found a point of the patch, which is the plane; where the search
  found no crossing and the position is only where the two came
  closest, it was once `1e-4` off), or on a quadric within `1e-6` of
  the count's position.
- Only placed (see "Curved primitives"), or solved with its quadric
  root further: the nearest root **with the crossing's sign** (the
  surface's gradient, turned to the patch's outward normal at its
  middle, against the edge's tangent there; a tangent root fits either)
  **whose point is within the resolution of the patch crossed**
  (`solve::near_patch`, certified): this pair's crossing, not the
  edge's way back out or another patch's. The search runs out of pieces
  (see "Crossings") and places such a crossing up to `1.6e-3` of its
  edge from the root: a bar turned through a box had a vertex `5.6e-4`
  off the cylinder and 22 to 35 patches of each result on copies
  claiming no surface; and a plane crossed twice by a curved edge took
  the root of the other sign when it was nearer.
- No root fits: as it was before (a plane's nearest root for a crossing
  only placed, a quadric's within `1e-6`, else the position), and the
  check on crossings only placed decides (see "Assembly").

Faces claiming no surface keep the position. A root a rounding outside
the edge (`1e-9`, and the quartic's past an end found by Newton from
it) is at its end, and one within `1e-12` of an end is put there
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
position in both patches' domains. Ends at one place, or within the tie
of each other (one place by different roundings), give one straight
edge of about zero length, which the clean-up collapses: the exact
section through two points `1e-16` apart (a plate's cap corner on its
hole's rim, against the wall of a pin in the hole) ran round the
circle the long way, outside both triangles, and the union failed
(`Invalid(Fold)`; with circles through exact axis points, a cylinder
over `0..1` and one over `0..2`, `Invalid(Hull)` too).

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
  cylinder's rulings (both tangents along the chord within `1e-9`)
  gives a straight edge. The arcs are kept only if their middles invert
  into the quadric patch. If the guided arc gives none (halving ran
  out) or isn't kept, the arc on the other side of the chord is tried
  (`Guide::Away` of the same point), and kept on the same test; at most
  one of the two lies on the patch, and the guided one goes first. The
  guide is on the chord, or nearly, where the arc turns back within the
  patch (a plane nearly along the rulings cuts in the tip of a long
  ellipse) or where the plane nearly touches the patch along it (the
  patch's bulge is then square to the plane): its side of the chord is
  a rounding's. A plane half a radian off a boss's rulings, cutting it
  at 24 offsets, took the wrong arc near the silhouette in about 40 of
  48 operations, which were then traced and fitted: right, but not
  exact.
- **On an elliptic or circular cylinder** (`surface::elliptic_cylinder`:
  the matrix's symmetric part `S` takes an axis to 0 and is definite
  square to it, the linear part square to the axis, both to `1e-12`
  relative; the centre solves `S·y = −b` square to the axis, as an
  extruded cylinder's origin is off its axis) **with both ends on it**
  (to `1e-12` of their distance from the axis or from the quadric's
  origin), the weight and the control point come from the arc's angle
  instead (`surface::elliptic_arc`). Seen along the axis a plane section
  is an affine image of the cross-section, itself one of a circle, and
  affine maps keep weights and control points: with `r̂` the vectors
  from the axis to the ends made unit in the cylinder's metric (`±v·S·v`;
  on a circle, plain lengths square to the axis), `w = cos(Δφ/2) = |r̂x +
  r̂y| / 2` and `C = M − tan²(Δφ/2)·(Ĉ − M)`, `Ĉ` the plane's point on
  the axis and `tan²(Δφ/2) = |r̂x − r̂y|² / |r̂x + r̂y|²`: only `+ − × ÷
  √`, exact in relative terms however straight the arc. `σ` is noise
  where the arc bulges by a rounding (`1.000135` on an ellipse arc,
  whose weight is under 1, pulled a sliver beside it `1.7e-6` off the
  cylinder onto a copy claiming no surface; a box turned a thousandth of
  a radian across a wall over an ellipse arc came out `5.6e-7` off in
  volume), and so is where nearly parallel tangents meet. So an arc
  straight only within the `1e-9` is that arc here, not a straight edge
  (one was `5.3e-11` off the cylinder), and only ends on one ruling give
  a straight edge. Ends off the quadric keep `σ`: there it made up for
  them, and the angle's weight made the bands worse (a vertex `5.6e-4`
  off a cylinder before crossings went to their roots: 580 patches on
  copies claiming no surface, unions and intersections `Invalid`).
  Parabolic and hyperbolic cylinders and other quadrics keep `σ`.
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
  conic along the end tangents, else a straight edge, each kept only if
  it follows the true cut (`chain::verified`): at `¼`, `½` and `¾` of
  the curve its point is within the resolution of both patches (by
  `invert` and `eval`: a tie's or a line contact's chord, which is the
  cut), or the patches' cut on the plane square to the curve there
  (`Pair::solve` from the domain positions interpolated between the
  ends) is within half the fit tolerance of it; NaN fails. If neither
  is, the boolean is `Inconsistent` before the rounds of cutting. The
  first test needs no Newton step: where the faces meet at a small
  angle `θ` a point within the resolution of both may be about
  `resolution/θ` from the cut (Newton's method lands anywhere along
  that band, or nowhere at a line contact), but the material between
  it and the cut is only a few resolutions thick. On planes and
  quadrics the implicit value along a chord is a quadratic in its
  parameter, along a conic a quartic over the weight's square, both 0
  at the ends, so the three samples bound it along the whole curve; a
  chord skips about the resolution times the area between it and the
  cut.
  Nothing else measures where such a curve is: the bands along it are
  checked only against their own faces, and a chord in the wrong place
  passes wherever the faces are within the fit of each other along it.
  A box face `1e-4` off a cylinder's rulings, `1.75e-4` inside its wall
  at mid-height, cuts it in a U 3.4 long round the tip of a long
  ellipse; neither arc survived halving (the tip needs about
  `log2(√(2h/(r·α)))` halvings, `h` the wall's height, `α` the tilt),
  tracing and the conic along the end tangents (a turn over 45°) both
  failed, and the chord across the U lay on the plane and within
  `1.7e-4` of the wall: the intersection, the sliver, came back `Ok`
  without its tip, 1.69 long and a tenth of its volume. Traced chains
  are fitted to the trace, and the rounds halve fitted curves at points
  on the true cut (`Chain::split`), so neither is checked.
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

Before those rounds, a face laid out in a curved patch's parameter
domain takes points for its triangles' **shapes** (`triangulate::shape`).
A cut inside one of the patch's triangles is joined to that triangle's
corners, far from the cut's short pieces: fans of thin bands (aspect 300
to 1 000 on a cross hole through a round boss, with long curved sides),
which fail the check's neighbour rules against the bands across the cut,
and which repair's red quartering keeps thin until its budget runs out.
So, worst first, a triangle whose smallest angle in the layout is under
5° (`SIN_SHAPE`, the extrude caps' bound too, with `circumcentre_from`:
`mesh/shape.rs`) takes a point at its circumcentre (in the layout), where
that lies inside the domain triangle, at least half the circumradius
(`SHAPE_CLEAR`) from every side of the loops (diagonals the ear clipping
fixed and the holes' bridges included), from every vertex and from the
domain's sides, and strictly inside a triangle whose three pieces have
their curved corners open; the triangle holding it is split there, and
the sides facing the point are flipped by `improve`'s rules
(`flippable`) as long as they improve, as in an incremental Delaunay
insertion. A point refused leaves its triangle as it is, and the
triangle isn't looked at again. Nor does a triangle whose circumradius
is under `MIN_CURVED_SPLIT` resolutions take one (in the layout, over
the patch's longest side): repair splits nothing that small, and there
the points only stepped towards a loop side a tie long, each halving
the last one's circumradius (eight in one face of a seeded chain, down
to `1e-7` in the layout). Nor does a face with any triangle whose
corners aren't all open take any: there a curved side leaves its
triangle, bulging over the triangles beyond, and a point in one of
those could land between the side's chord and its curve, outside the
face (a fuzz of random bent loops found such points; the face is
mended or asks for splits instead, and the last round's faces go to
repair as they are). Why a point can't land outside the face
otherwise: the holder is a straight triangle reached without crossing
a side of the loops; a curved side of the loops whose corners in its
triangle are open lies in that triangle (the curve's control triangle
does: its tangents at both ends point into it), the pieces holding a
curved side and the flips' triangles are only made with those corners
open, so every curved side stays in its own triangle. The
clearance search finds every side and vertex within the clearance, as
the disc is convex: a segment from the centre to any point of it
crosses only sides within it. The worst comes
from a priority queue (entries whose triangle changed are passed over),
the triangle holding the point by a walk from the thin one that never
crosses a side of the loops (one that would, or ends on a side, refuses
the point), and the clearance by looking at the triangles that come
within it, from the holder across their sides within it (a side of the
loops within it refuses the point; a vertex within it is a corner of one
of those triangles): all local, each step counted by the meter, at most
four points per vertex of the loops and 16 more. The points are placed on
the patch, as the mending's are, so the triangles round them are the
patch's own pieces. Only on curved layouts: on flat and planar ones the
same points gave flat cap triangles with an arc side more corners to fan
to, and two more seeded turned operations failed as folds. Measuring the
angles in 3D (the layout mapped by the patch's derivatives) was worse
than in the layout. Plane faces are refined as a whole in the clean-up
instead (see "Clean-up"), where a point near the face's edge can halve
that edge in both faces beside it, which a face's layout can't.

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
A triangle off its face's quadric (see "Exact bands on quadrics") by
more than half the fit tolerance at the samples asks for its sides on
the face's boundary (cut or edge pieces) to be halved the same way. Each face reports the largest of these two distances over its
triangles (`Cutout::stray`, NaN as infinite), and the round the largest
of all; **the round kept must be within the fit tolerance**, else the
operation fails as `TooComplex`. Halving can't bring every band within
it: a vertex off the surface keeps the triangles at it that far however
small they get (bars through boxes at the finest tolerance: 5.6e-4 off
in every round, their union and `bar − box` kept at 56 times the
tolerance before), a triangle with no side on the boundary has nothing
to halve, and a fitted chain's stray only halves with its
curves. Between half the tolerance and the tolerance, a result that
ran out of rounds is kept: within the contract. `TooComplex`'s advice,
a coarser tolerance, holds: the leftover is of a fixed size.

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
corners on the rim, which folds.) It works either way round: the quadric
triangle's curved edges lying in the other's plane (the boss's rim), and
the planar triangle's curved edges lying on the other's quadric (sampled
at `¼`, `½` and `¾` within the resolution of both): a cap's rim on the
wall of a cylinder of its radius on its axis, standing on it or running
past it, or a profile extruded again as a join over a longer span (a
plate's outline and its hole, rounded corners, elliptic and parabolic
sides: "make it taller", the hole's wall concave). There `A`, grown for
a union, keeps a ring of its cap of zero
width between its rim and `B`'s wall cut in the cap's plane; with only
the first rule the rim's 4 arcs faced a cut refined to hundreds, the
ring's triangles fanned across the disc and every same-radius union with
a seam failed (`Invalid(EdgeNeighbours)`, 0.3 to 2 s each). Each extra
lands on a cut vertex, so there are no more pieces than the cut has, and
merging brings the refinement's back: such unions now come out in 20 to
60 patches in milliseconds. Release, before → after: the seeded suite's
pins and coaxial cylinders 37 → 39 of 40 (the one left is a pin standing
on the plate over its hole, touching only along the rim: no manifold),
related 112 → 113 of 120 (with the twins below), the rest unchanged
(turned 156/160, chains 203/240, tangent 72/96, bosses 64/64, drilled
160/160); a sweep of a cylinder of radius 1 over 0..1 against one of radius 1 or
0.5 in 3, 4 or 6 arcs turned 0, 0.3, 1.1 or 2.9 radians, over 8 spans
each, all three operations: 47 of 96 rows with a failure → none, every
volume within `1e-12` of its closed form.

**Cuts beside a side from end to end.** Where a plane passes through
both ends of a curved side of a quadric triangle without the side lying
in it, the plane's conic on the quadric runs from end to end beside the
side, and the two bound a band (`Cutting::bands`). A boss sunk through a
plate twice its height thick meets it so: near the holes refinement splits
the boss's wall at the middle of its rulings, which is the plate's top,
and joins those points with the wall's curves over straight domain
segments, which bulge out of that plane (0.04 over a 45° piece of a wall
of radius 1.1; a cylinder's patch isn't affine in height). With no
vertex on the side the band was fanned from its tips across the cut's
vertices into slivers whose three corners lie on the plane's circle
(`Invalid(Hull)`), and the rounds halved the cut down to vertices `1e-5`
apart doing nothing for it. So the side gets vertices across from the
cut's (`Band::across`: in the triangle's domain, from its corner opposite
the side, for the cut's vertices whose weights at the side's ends sum to
at least ½), first and with every halving, as the flush rim's extras;
those nearer a crossing already on the side than a quarter of the way to
the next across or the side's end are left out (`Band::extras`; the
crossing stands for them; kept, one came `3e-7` from where the walls of a hole and of the
boss crossed on it, and left a sliver). A cut that is one curve from end
to end has nothing across, and the band would be one triangle with a
straight angle where the side is halved (it fails the fold check), so it
is halved once before the rounds. Every vertex added lies on the side's
own curve, which both triangles beside it get, and splits a piece of the
side between two of its crossings, both halves kept or dropped as it
was: the counting's decisions stand, and no wrong result can come of
it, at worst more pieces. Release, the seeded drilled plates with a boss
(below) 567 → 588 of 600 with the end crossings' rule in "Ties"; the
other seeded tallies didn't move.

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
same name, so no feature edge), keeping the face tag check true. Such a
triangle is still held to the fit tolerance from the quadric, as those
along a cut are from the patch (see "Rounds"): before, nothing bounded
the triangles no ruling freed and no cut ran along, and at a crossing
placed 1.3e-4 off a small cylinder's wall they came out that far off
at every tolerance.

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
standing for it in the triangulation. Ends within the clean-up's short
length (an eighth of the resolution) count too, where the clean-up will
collapse them: in a triangle with two corners that near each other, an
inner side from the third corner to one of them takes the curve of a
boundary edge from that corner to a vertex as near the first (the
boundary edges at the corner in key order, the first that fits). Two
vertices of one tie can come by different roundings, `1e-16` apart (a
cap's inner edge crossing at a rim vertex, and a wall's diagonal
crossing the cap's plane): the triangle's two sides from the far corner
were two different conics on the wall, the clean-up refused to merge
two cut edges, and the lens folded (a cylinder of radius 1 in 4 arcs
over 0..1, united with one in 3 arcs over 0..2). Twinned edges are fixed
for the exact bands, as the bit-equal ones are. Only an inner edge's
curve changes (to a boundary curve from its far end to within the short
length of its near end): the counting decided the topology already, and
the rule joins nothing that wasn't joined.

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
  tie the ratio of the first powers of `ε` that aren't zero. An edge
  decided to lie in a flat face's plane has its crossing kept inside the
  triangle (widened by the tie), and one with no part inside fails as
  `Inconsistent` (see "In-plane crossings").
- **Crossings only placed are certified** (`Cutting::certify`): a
  crossing the search didn't solve (see "Curved primitives") lies on its
  edge, and on the other surface where it went to a root on the patch
  crossed (see "Crossings on curved edges"), but otherwise only by luck
  (a face claiming no surface, no root of its sign on the patch), so
  once placed it must be
  within the resolution of the patch it crosses, or of another patch of
  the other operand whose box comes that near (a crossing through the
  side two patches share lands on either), by `solve::near_patch`: the
  foot of the perpendicular (Gauss–Newton) moved into the patch's
  triangle, else a search splitting the triangle by blossoming, pieces
  whose control points' box is farther dropped, nearest box and then
  nearest middle first, up to 512 pieces. Every distance it finds is to
  a point of the patch, so a `Some` is certified; not found, or past
  the cap, the operation fails as `Inconsistent`. Before, such a vertex
  4 resolutions off a cylinder (a curved rim against a crossing
  cylinder's wall, search capped) came out in four `Ok` results, its
  bands on a copy claiming no surface, and one 5.6e-4 off (bars through
  boxes) likewise.
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
  difference: plane tags and forms), then the copies claiming no
  surface (with their faces' forms), less those no
  triangle is on any more, so chained booleans don't pile up faces;
  halfedges pair up by vertex id in `MeshBuilder`, never by position,
  and every triangle side with a curve record gets it. Each face keeps
  the aliases of its source (the operand's face it is, or copies):
  the operand's own, and, where the clean-up moved triangles of one face
  onto another of the same surface (`Soup::absorb`, a pair of sources),
  the moved face's key and aliases, through any chain of such moves
  (`boolean::aliases`, passes over the merges until nothing changes,
  each merge charged a unit and one a key of the two sets). A face cut
  away takes its aliases with it, but an operand's plane face dropped
  under the other's flush face is an alias of it (`boolean::covered`;
  see "Topology and names").

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
only blocks it if one of its sides leaves into it or along its sides;
nor one within `1e-9` of its diagonal's length off that diagonal,
outside it, which would leave a pocket of zero width between the two:
a cut along the domain's side whose inner vertices lie on it, moved
inwards alike, and one of whose ends lies on the next side a few ulps
in, gave the diagonal from that end, then a triangle with all three
corners on the cut in the patch, of curved sides no flip takes away,
which folds: stacked coaxial cylinders on turned frames; only with
curved sides, since among straight ones the clean-up flips the pocket
away and the rule cost chained turned grid boxes a step),
a zero-area ear with no vertex on it, then any (with curved sides a
proper ear whose curved corners aren't open ranks between the proper
and the zero-area ones: see "Cutting curved faces"). No diagonal joins
two vertices on one side of the domain (it would lie along the side, and
the patch across could add the same one), in a face of `B` two vertices
of one cut, or repeats an edge. Then diagonals are flipped towards the
Delaunay triangulation (the far corner inside the near triangle's
circle, the quadrilateral convex, no two of its corners at one place,
the new diagonal allowed and the curved corners open; at most 8 flips
per triangle: a flip out of a triangle with two corners at one place
made a straight diagonal there, which the clean-up's collapse of the
zero-length side then made that triangle's curve, two arcs of one
smooth curve meeting at a corner of a proper triangle, which folds: a
rounded rectangle over `0.5..2` intersected with itself over `0..1` on a
turned frame), which removes the
thin triangles greedy ear cutting leaves. On a curved patch's layout,
points for the triangles' shapes follow, and with curved sides, Steiner
points at corners (both in "Cutting curved faces"), in no triangle of zero width
(corners on a line: a band between two curves lying on each other, which
no point or split mends; asking for splits there doubled the curves
every round).

The faces of a round are triangulated in parallel and count their
steps together (`triangulate::Meter`: a vertex tested against an ear, a
triangle looked at by the flips and mending, a triangle queued, walked
through or looked at for clearance and a side looked at by the points
for shapes, the sides looked at
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
- **Collapse what a tangency leaves** (`tangent_edges`). A wall tangent
  to a face along a line, or to a face's straight edge at a point, is
  solved to cross it twice where the exact result has one place: the
  root is double, so the two crossings land about `1e-8` of the part's
  size apart (`7e-9` at unit size, `2e-7` at 30 mm). Within an eighth of
  the resolution they collapse as above; past it (a fine tolerance, a
  part in millimetres) they left a piece of the face's edge between two
  arcs of the wall's one smooth curve, so a triangle on the face with a
  corner of 180° (`Fold`), and, along a line contact, a strip of the
  touched face as wide as the pairs are apart between the wall's two
  halves, which the hull rules refuse. So a straight edge longer than
  the short length and no longer than four resolutions is collapsed
  where, in a triangle on a plane face, a curved side leaves one end
  back the way the edge came (that end moved onto the other), or where
  it is the shortest side of a triangle of straight sides whose two
  other sides both border other faces (either end, the higher id
  first) — but only onto a point within the short length of the
  surface of every face round the moved vertex, so no vertex leaves a
  surface it claims (the pair's two crossings both lie on the line
  where the faces meet, and the vertex moves along the curve's own
  tangent). The collapse is the short edges' one with two changes: no
  corner along a curve that was open may close, and where a gone
  triangle's two sides from its far corner are both inner edges of one
  face (two arcs of the wall to the two crossings, as far apart as they
  are), the kept end's curve stays, but only where the two are one curve
  but for those ends (both straight, or control points within twice four
  resolutions and weights alike): else the triangle beyond the moved
  end's would take a curve far from its own, such as a long arc's chord,
  which a face claiming no surface wouldn't show. Whether the kept end
  lies on the moved one's surfaces is asked again before each collapse,
  since one before it may have moved another vertex's triangles onto
  it. In millimetres at the default
  tolerance the intersections of a cylinder inside a plate touching its
  side (the cylinder, or cut to the plate's height), of a slot beside a
  hole tangent to it, and the plate less a slot across a hole tangent
  to both its sides, now work; at unit size the same at the finest
  tolerance. Moving such a vertex moves the triangles round it by up to
  four resolutions along the surfaces. Past about `5e5` times the fit
  tolerance in size (5 units at the finest, 500 at the default) the
  pairs can lie further apart than that, and those still fail.
- **Flip** the longest side of a triangle whose height over it is no more
  than an eighth of the resolution, or no more than four resolutions when
  the triangle across is on the same face (so every triangle stays on its
  face's surface) and the flip leaves nothing thinner. The far corner of a
  flat triangle lies on that side, so the two new triangles cover the one
  across exactly.
- **Unfold** a vertex whose star is folded (`cleanup/fold.rs`), last in
  a round and only where nothing else in it changed. A sheet folded onto
  a flush face at a saddle has one side from each operand, each
  triangulated on its own, so no collapse makes two of its triangles the
  same and cancels them: its vertices have every triangle round them on
  a plane face, of straight sides and within an eighth of the resolution
  of one of at most three planes through the vertex (each the normal of
  its largest proper triangle, one plane, or two or three on a crease),
  with proper triangles in some plane facing both ways (two triangles
  round it whose normals point apart are looked for first, which turns
  away nearly every vertex before its curves are looked up). Moving such
  a vertex `v` onto a neighbour `w` lying in every plane of its star
  moves each triangle `v a b` through the tetrahedron `v w a b`, whose
  corners lie within the short length of one plane, so the winding
  numbers change only in that thin slab over the star, whatever order
  the planes take round `v`: the oriented surface and the volume stay as
  they were and the folded part cancels. Curved faces are left out: a
  triangle of one with straight sides lies in a plane only where it is
  thin, and moving its corner along the plane would take it off its
  surface (before they were left out, the rule fired on such stars in
  two seeded refusals, a flush boss and a related pair, changing only
  how they failed).
  The candidates go by the unsigned area left round the vertex, then id,
  and one is taken only if that area is less than before by more than
  the short length squared (turned triangles cover theirs twice over, so
  the area is what bounds the rule: a first try onto any neighbour, and
  before the other rules, made a turned triangle overlapping others
  outside the star and broke a chain that worked). The collapse is the
  same as the others (one fan round every vertex, the pairs made the
  same cancelled), except that a proper triangle may turn over, only in
  a plane of the star whose triangles face both ways; one that turned,
  or that now faces against its plane face's normal, goes on the face of
  the star's largest proper triangle in its plane facing its way, and if
  there is none the collapse isn't made, so plane tags stay true for
  repair; nor is it where a triangle ends further off its face's plane
  than it was (or than the short length), as one could on a face a hair
  off the star's plane, nor where a proper triangle ends facing against
  the normal of its face's plane (a sliver no higher than about twice
  the short length can lie that close to the star's plane steeply,
  facing its way by the star's plane but not by its face's). The scan
  goes over the vertices in id order and is charged the soup's size, as
  a round is (on a plate with 64 square pockets, 8 flush bars and 8
  holes it took under 1 % of the clean-up). Chained grid boxes (random half-grid
  boxes, chains of five, as `grid_boxes_chained`; seeds 1 and 3–29,
  140 000 steps): 11 failed on a manifold result before, 1 now (a hull
  failure, not a fold: see Known gaps), none new and no wrong volume;
  the `Invalid` counts where the result isn't a manifold are unchanged.
  The seeded chains of parts went from 201 to 203 of 240, every other
  seeded tally as before.
  The rule needs plane tags, so the turned grid boxes of the tests,
  which claim no surfaces, never meet it; boxes extruded on a turned or
  tilted frame keep theirs, flush only to rounding, and there it mends
  about 20 of 12 700 chained steps that failed. On boxes each turned a
  hair (`1e-11` to `1e-7` rad) off the others it fires in 1 step in 13
  and mends about 370 of 12 600. There it once left a soup the check
  refused (`VertexNeighbours`) that the clean-up without it would have
  mended: a triangle faced against its plane face on each side of a
  vertex, unfolding took out one, and the Delaunay flips that took out
  both no longer could. So a boolean whose result fails the check
  (`Invalid`) after the rule fired is cleaned again without it, from a
  copy of the soup taken before the clean-up, and the first error
  stands if that fails too: the rule never loses a result the clean-up
  without it gives (budget allowing). Over 680 000 steps of chains on
  such frames (boxes, cylinders on a quarter grid, prisms over lattice
  polygons; at the origin, far off, turned, tilted, a hair off, at a
  hundredth and a hundred times the size), the results with the rule
  were within `4e-8` of the volumes without it where both worked, kept
  the volume identities and the cells' volumes, left no plane face's
  triangle further off its plane or facing against it more often than
  without it, and were the same bits at 1 and 8 threads; every budget
  gave the full result or `TooComplex`.
- **Drop** connected parts enclosing no volume (at most an eighth of the
  resolution times their area): what is left of flush faces meeting.
  The volume counts the triangles' curved sides (each triangle with one
  integrated as a patch): a sliver cut off a wall along its rulings has
  every corner on the cutting plane, so by its corners it enclosed
  nothing, and a box cutting a cylinder `3e-3` deep gave an empty `Ok`.
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
claiming no surface, as fitted bands do (`leave_surfaces`), held to the
fit tolerance as they are: one further off than it (its control points
from a plane, its samples from a quadric) fails the operation as
`TooComplex`.

A triangle of zero height (no more than an eighth of the resolution) and
straight sides may also be flipped into a neighbour with curved sides:
the new side from its far corner, which lies on the neighbour's side, is
the neighbour's own curve from there to its far corner (the neighbour
bisected there by blossoming, `inner_curve`), so the two new triangles
are exactly its pieces, on its surface. Collinear triangles left on
curved faces failed the fold check.

**Seams** (`cleanup/seams.rs`): a curved edge between two triangles in
one plane. Where two operands' caps are flush and face the same way,
the perturbation keeps one cap whole and cuts the other along the
first's rim, whose sliver of wall then collapses away; what is left is
a curve between two patches in one plane (both on plane faces, or one
on a remnant of the wall at the rim, of zero height in the plane), or a
cluster of zero-size triangles there. No plane through such a curve
has either patch off it, so the hull rule can't hold across it, and
repair split along it down to flat pieces: a boss united with the plate
it stands in over one span, the boss first, gave 28 912 patches, 115 024
at millimetre scale (a 60 × 40 × 10 plate, an r8 boss), a flange at a
shaft's foot 28 846, two overlapping bosses of one height 9 000 to
47 000 whichever went first; past the budget, the next boolean on such a
body failed as `TooComplex`. "In the plane" is geometric: the corners
and the control points of the curved sides within an eighth of the
resolution of the triangle's own face's plane (`in_plane`), whatever the
neighbour's tag. In the rounds, for each triangle on a plane face in
its plane, each seam is first made its chord (`straighten`): both
triangles lie in the plane, so their union stays exactly what it was
(the lens between the curve and the chord moves from one to the other),
taken only if both stay proper (higher than the short length, facing
along the plane's normal, curved corners open, the patch passing the
fold check). Failing that, it is flipped away when the two make a
convex quadrilateral whose new triangles are proper the same way
(`unbend`): the new
triangles cover the same region whatever the curve between them. A
neighbour on a face that isn't the same plane (the wall's remnant, or a
plane it lies in only along a line) moves onto the triangle's face,
whose plane it lies in, rather than keep claiming a surface it has
left; a face of the same plane across is recorded as joined. After
the rounds, each region of triangles in one plane that still holds a
seam (grown across seams, edges no longer than the short length and any
side of a triangle no higher than it) is triangulated again from its
boundary loops with the faces' ear clipping (`dissolve`), its inner
vertices dropped and the boundary as it was: only if the boundary
passes no vertex twice and has no side about zero long, and the new
triangles add no point, split no side, repeat no edge outside the
region and are all proper, their corner triangles' areas adding up to
the old ones' (any triangulation of the same loops does: with every
triangle positive, that is covering it once). The new triangles go on
the seed's face. Each region's triangulation may take up to 2²⁰ steps
(16 a unit of work) before it is left as it is. Last, the plane faces
seams joined are merged, each set onto its lowest id (the first
operand's faces come first, so the body's name stays), a face moving
only if every triangle of it lies in that one's plane and its plane
faces the same way; copies claiming no surface go with it and take its
name and form; each merged face's key (and its aliases) becomes an alias of the
face it merged onto, as does the higher face's where `unbend` puts both
new triangles on the lower (`Soup::absorb`; see "Topology and names").
Faces of one plane meeting along straight edges, or along no seam
the clean-up mended, stay apart here; `Mesh::merge_faces` names them
alike after repair ("Structure"), which covers what this merge does for
the names. This one stays because it moves the triangles onto one face
before the Delaunay flips, which flip only within a face. Drawn lines
go where face keys differ, so without either a straightened rim would
show on the flat top as a polygon of chords. The pin filling its hole, united with the
plate, now 36 patches as before; the boss first 52, at millimetre scale
52, the flange 64, the overlapping bosses 178 to 180, a boss half over
a plate's hole 202 and 238, one over its edge 56, a slot of a cylinder
and a box tangent to it 84 (it folded before), each with its top one
face. Of 160 random flush unions of bosses, plates and holes (half at
millimetre scale), 112 had over 20 times their operands' patches (up to
132 712) and 23 failed (most past the budget); now 2 had (up to 1 166)
and none failed, intersections and differences as they were. A cluster
of four vertices at one place at a slot's arc joint, on a turned frame,
is the one region triangulated again in these runs (the union folded
before). The fallback also takes caps a hair apart, closer than the
short length (a boss whose top is 2e-8 above a plate's): the two are
one plane to the clean-up, and the rim's clusters between them are
triangulated again, moving the surface by no more than that hair
(always under an eighth of the resolution, so the face tags hold).
Caps further apart than the short length but within a few resolutions,
or tilted off each other by 1e-9 rad and more, mostly fail as `Invalid`,
as they did before. Chains of flush joins (a drilled plate, a boss
through it filling a hole, one flush with both caps, one standing on
it, one through it flush with the last) come out exact at every step,
their caps one face each.

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

Last, **the plane faces the boolean cut are refined for their
triangles' shapes** (`cleanup/quality.rs`). The slivers' flips add no
vertex, so a box's cap cut by a hole stayed fanned from its far corners
to the rim (triangles 19 mm long, the worst of sine `3.75e-3`), and the
next hole in line with the first passed along their long sides (see
Known gaps, "Long cap triangles…"). The pass is a Delaunay refinement
(Ruppert's) on each plane face as a whole, in 3D, of the triangles this
boolean made (`Soup::made`: all but the operands' triangles kept whole
and the refinement's pieces merged back into them; a piece of a made
triangle is made). A triangle is bad when it is made, on a plane face,
proper, its sides longer than the short length, its circumradius at
least `MIN_SPLIT` resolutions, and the sine of a corner under `5°`
(`QUALITY_SIN`, the bound the points for shapes on curved layouts use),
a corner along a curve measured from the curve's tangent too (a
straight side leaving along a curve is narrow however wide the chords
make it, and a later split of the triangle there closes the corner),
but not where some corner lies between two constrained sides meeting
at under 60° (the boundary makes that angle; refining towards it only
halves, and spreads to the faces beside). A side is free when it is
straight between two triangles of one face; the others are constrained.
Where no triangle is bad, nothing past the first step changes. In
order:

- Triangles on plane faces whose curved corner is closed have a free
  side flipped where both new triangles are proper with their corners
  open; the curves of those left may leave their triangles, and no new
  point goes within the box of their control triangles.
- A vertex inside a plane face (every triangle round it on the face,
  every edge from it straight) at a bad triangle's corner is taken out
  where its star triangulated again on its boundary (as the seams'
  regions are) is better shaped: what is left of a seam inside a cap two
  flush caps made one, a vertex a tenth of a millimetre off the cap's
  edge, which the refinement otherwise grades towards with 120 points
  (a slot of a box and a cylinder tangent to it: 84 patches, 328 with
  them).
- The made triangles' free sides are flipped towards Delaunay (the far
  corner in the circle, with the margin `triangulate::in_circle` uses):
  the refinement's guarantees assume it, and without it a wall's thin
  right triangles split into pieces of their own shape without end.
- Then, worst first (a queue by the sine's bits, a closed corner's sine
  of zero or less counted as zero, and the sorted corners; entries whose
  triangle changed are passed over), a triangle of
  straight sides takes a point at its circumcentre (worked out in the
  face's plane and put on it), found by a walk across free sides; a
  point within `MIN_SPLIT` resolutions of a free side halves that side
  there; a point beyond a constrained side, or within the diametral
  circle of a straight constrained side of the triangles it would
  split, halves that side at its middle instead, where it is longer than
  twice `MIN_SPLIT` resolutions and the triangle across is on a plane
  face too (a cap's edge with a wall: both faces get the vertex);
  curved sides are never split. A triangle with a curved side flips
  the straight side at a narrow corner on the curve where that makes
  both triangles better, or, where the narrow corner is across the
  curve (a fan onto a rim), halves its longest free side, which brings
  the far corner nearer. After each change the free sides facing the
  new vertex are flipped towards Delaunay.

Every change keeps the region: a point goes strictly inside a proper
triangle, a side is halved at a point on it, a flip replaces two
triangles of one plane by two covering the same quadrilateral, a star
is triangulated again on its own boundary, and every new triangle is
proper (higher than the short length, facing along the plane's normal,
narrowest sine above `1e-6`, its curved corners open and the patch
passing the fold check), so each curve stays inside its own triangle
(the argument in "Cutting curved faces"). Its curves must lie in the
plane too (their control points within the short length): a fitted
cut's curve a collapse put on the face stays on the triangle that has
it, the one the clean-up moves to the face's copy claiming no surface
(by id, `leave_surfaces`), and a star holding one isn't taken out. A
halving on a cap's edge keeps each piece on the face of the triangle
it came from, the new vertex on both planes (a lerp between two points
on both). New points are on the plane
up to rounding (circumcentres projected onto it, midpoints lerped),
their ids after all others. It is sequential with total orders, at
most 4 points per made triangle and 64 more (reaching that only stops
it), every step charged (a unit per walk, flip and queue step, and the
stars' triangulations as the seams' regions are). Between points it
ends too: the flips after a point are at most 256 and Delaunay ones
(which never undo each other), a flip at a curve's narrow corner widens
the pair's narrowest corner strictly (so no set of triangles comes
back), and the closed corners' flips, which have no such test, are each
tried once before the refinement and once after it; no flip makes a
closed corner, so none undoes one of those.

Results, release: one hole in the 20 × 20 × 1 box, its caps' made
triangles all at least `5.7°` (`cut_caps_are_well_shaped`; 80 of them,
32 before); the box drilled twice in line works either way
(`a_box_drilled_twice_in_line`, `holes_in_line_on_a_large_plate`); the
60-hole grids drilled one at a time, 1 of 540 steps fails (12 before)
and 0 of the in-line test's 180 (4 before), see Known gaps.
Measured bounds on the nine grids: 5° fails none of 540 at 6 388 to
9 296 patches; 10° fails 1 at 8 284 to 13 414, and lost 5 of the 600
bosses sunk through drilled plates and 6 flush unions to their patch
counts; 20° fails 1 at 13 720 to 20 334. (Those runs had neither the
vertex removal nor the Delaunay flips first, and skipped faces with a
closed corner; as built, at 5°, 1 of 540 at 5 688 to 8 568.) Every seeded tally is as before or better (turned
156 → 158 of 160, bosses in drilled plates 148 → 149 of 160) and
`a_cap_folding_when_refined_is_right_or_refused` now has three of its
six operations right (see "Profiles and extrude").

Collapsing removes an edge and keeps a closed manifold; it never decides
that two separate vertices are one. What the clean-up can't mend fails the
final check.

### Results that aren't manifolds

Where the exact result isn't a manifold (two boxes touching along an
edge or at a corner, united; a box less a solid touching its skin from
inside at a point or along a line), the perturbation gives parts a zero
distance apart: for a union `A` grows by `ε·n_v`, so the perturbed
operands overlap in an infinitesimal prism along the shared edge, and
the counting builds a manifold with a zero-width neck, whose vertices
coincide at `ε = 0`. The clean-up collapses short edges only where every
vertex keeps one fan, which the neck can't, so it stays and fails
repair or `check` (fold, hull or neighbour rules). The same operands
intersected, or subtracted the other way, work.

Such a failure is named `BooleanError::NotManifold` rather than left as
`Invalid`, where repair or the check (not `facing`) fails with
`Invalid` and either
- the mesh before repair (the cleaned mesh, `Unfinished::given`) has
  two distinct vertices within the clean-up's short length
  (`resolution / 8`): the neck above; or
- the failure is `Hull(t, u)` and triangles `t` and `u` lie on separate
  shells (`apart`): two parts touching or closer than the resolution
  whose shells the operation left uncut, as cylinders tangent along a
  line, united, where the counting finds no crossing and the result is
  both operands as they were, their hulls meeting along the line, with
  no near vertices at all. Repair names triangles by those of its input
  (pieces keep their origin), so for its `Hull` the shells are those
  of the cleaned mesh; for the check's, after repair, those of the
  repaired mesh.

Both meshes come back with the error from
`Solid::finished_or_unfinished` (`Unfinished::Repair` holds the
cleaned mesh repair refused; `Unfinished::Check` the mesh the check
refused, and the cleaned mesh where repair changed it, else the two
differ only in face names), kept in `Failed` until the last word: no
copy of the positions is made up front.

Then the operation's last error becomes `NotManifold` (`pinched_named`,
`pinched` and `apart` in `boolean.rs`). Only the
error that is returned is classified, at the very end, after the
unfolding and joining retries (both keep the first try's error, so its
positions are the ones measured, and its evidence), so no `Ok` ever becomes an error;
`TooComplex`, `Inconsistent` and `Degenerate` stay as they are (the
budget is gone, or no mesh was built). `pinched` is a hash grid of cells
`resolution / 8` wide keyed by `floor(p / d)` as `i64` (bounded: within
`MAX_COORD` at the finest tolerance keys stay under about `1e15`; `as`
and the neighbour offsets saturate, which only puts more in a cell),
each vertex measured against those before it in its 27 cells, stopping
at the first pair: a unit of work a vertex and one a vertex measured
against (points `d` apart fit about a hundred to 27 cells, so that's
bounded); `apart` is a union-find over the triangles' sides, a unit a
triangle. If either runs out of the budget, the error stays `Invalid`. The
answer depends only on the positions. It isn't an early exit before
repair: short curved edges are never collapsed and can pass `check`, so
a near pair alone proves nothing. It names parts closer than the
resolution (curved operands overlapping by less) too, which is the same
thing at the kernel's resolution and mended the same way; and rarely a
manifold result that fails for another reason with two vertices that
near (rounding residue on flush faces): wording only.

The decisions give `NotManifold` before any mesh is built for unions of
walls along one direction that touch along a line from either side
(`pinched_line` in `boolean/pairs.rs`). Walls tangent within the tie
distance, `A` grown by the perturbation, cross in two lines infinitely
close, whose ends (two or four to a pair, as the patches' edges fall)
aren't clear (see "Ends along one direction"), so no join takes them,
and refinement split the pairs along the line until the budget ran out.
So in a union (`grow`), a pair on walls along one direction
(`parallel_generators`) whose ends aren't joined fails the operation as
`NotManifold` where at every end the walls face opposite ways (each
wall's normal there, the quadric's gradient, turned the way its patch
faces at its middle: within 60° of it, else the rule doesn't apply; a
negative dot) and aren't clear of each other (`θ² <
2·LENS·resolution·κ`, `κ` the sizes of the cross-sections' curvatures
`tᵀ·H·t/|∇F|` added: tangent, or crossing at so small an angle). Solids
touching along a line from either side unite into no manifold. Walls
crossing at such an angle unite into a crease, or leave a slit, whose
two sides stay within the resolution of each other for `resolution/θ`
beside it, more than `√(2·resolution/κ)`, half the width of a piece
bent by `κ` that is flat to the resolution: the result comes closer to
itself than the resolution, which the error names. For solids side by
side that is an overlap under a quarter of the resolution; for a pin of
radius 0.99 against the wall of a hole of radius 1, whose gap opens
only by the difference of the curvatures, overlaps of up to 50
resolutions. Refinement never made such a union (measured below).
Walls facing the same way (a solid inside another, touching its skin
from inside) unite into the outer one, a manifold, and are refined as
before (a cylinder inside another touching it, united either way round,
is never named so). Walls bending into each other (the curvatures, each
signed by the way its wall faces, adding up to `κ' < 0`: a pin against
the wall of a smaller hole) overlap all round the line but for a slit
`θ²/2|κ'|` deep, and are named so only where that is at least `SLIT`,
a sixteenth of the resolution (four tie distances). With no slit the
pin plugs the hole, a manifold, but the counting's ties give its pairs
ends where the walls come within a tie of each other, at most a tie
deep, which the rule named `NotManifold` until the slit was asked
(`a_pin_plugging_a_hole_it_touches_inside_is_no_pinch`: on the seam it
works; off it, refined, it runs out at the default tolerance, as it did
before the rule). An error
from the decisions comes before any line is joined, so the operation
returns it without the second try. Only ends of a first try (joining
lines) are looked at, and only unions: differences and intersections
whose walls touch from inside (a cylinder less one inside it touching
its wall, which isn't a manifold either) still refine as before.

Measured: on the default-tolerance probe
(`near_tangent_cylinders_at_the_default_tolerance`) the 13 unions at
gaps `0`, `±1e-12`, `±1e-9` and `−1e-7` (both placements) fail as
`NotManifold` in 2 400 to 4 200 units, where 11 ran out after 2.7 to
4.2 million and two were refused after 2.7 to 3.3 million; the probe's
work 84.7 → 42.6 million units, those over a million 23 → 12 (the thin
overlaps' differences and intersections), every result as before
otherwise. The regen's tangent discs join as "the result would touch
itself" at every fit and angle in 20 to 830 ms (was "too complex" at
fit `1e-4` and on the seam at `1e-3`, 1.5 to 6.4 s). On the seeded
suites every tally is as before; four refusals change kind (three
`Inconsistent` unions and one `TooComplex`, gaps of `±1e-9` and
`±1e-6` at fits 0.1 and 0.01, become `NotManifold`).
A sweep of 1152 unions (release; unit cylinders side by side, radii 1
and 1 or 1 and 0.3; pins of radius 0.5, 0.9 and 0.99 through a hole of
radius 1, crossing its wall, and of 1.01, 1.1 and 1.5 leaving a slit;
overlaps or slits of −1 to 100 resolutions; fits 0.1, 0.01 and 0.001;
the line on the seam and turned 0.3 off it; both orders), with the rule
and without: no result lost or wrong (136 right either way), the
sweep's time 2640 → 490 s, `NotManifold` 81 → 690 (the rest
`Inconsistent`, `TooComplex` or `Invalid` either way). Asking for the
slit took back only the six plugging unions (turned, fits 0.01 and
0.001: `TooComplex` again, as without the rule) and 28 of the 36 slits
of 0.05 resolutions.

Measured on the release kernel suites (the default ones, and the slow
bosses-in-drilled-plates, bars-through-boxes and drilled-grid ones):
every tally the same as before, as it must be. Of the 117 refusals the
seeded suites print, 95 were `Invalid`; 38 of those were `NotManifold`
when this naming came (34 by the near pair, 4 more by separate shells;
42 refusals are now, with the decisions' rule above): tangent and
near-tangent cylinder unions at gaps 0, ±1e-9 and −1e-6, pins against a
hole's wall, chained app-like steps. The flat touching cases (boxes on
an edge or a corner, the diamond and octahedron differences) are all
named so, and the grid boxes give `NotManifold` or `Invalid` only where
the cells aren't a manifold. What stays `Invalid` is mostly `Fold` at
cusps where faces are tangent (a boss standing in a plate tangent to
its side, `a_cusp_where_faces_are_tangent_stays_invalid`), and thin
triangles.

### Errors and budget

`KernelError::Boolean(BooleanError)`: `Inconsistent` (the decisions
don't fit together: near ties taken as ties, with curved operands; flat
operands are decided again exactly then, see "Decided again exactly";
also a winding number out of `0..=1`, see "Counting"; or a
crossing the search only placed isn't on the other operand; or a
cut neither exact nor traced whose fallback curve isn't on the true
cut, see "Chains"),
`Degenerate` (a face's loops
couldn't be triangulated, or the triangles don't pair up), `NotManifold`
(the result would touch itself along an edge or at a point, or come
closer to itself than the resolution: see "Results that aren't
manifolds"). `TooComplex`
past the budget or `MAX_PATCHES`, or with triangles still off their face
by more than the fit tolerance after the rounds of cutting or the
clean-up; `Invalid` when the result fails
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
clean-up round, the triangles per round of merging, repair's own, a unit a patch
naming faces of one surface alike (`Mesh::merge_faces`), and 5
units per patch of the result for the check that makes it a solid (about
2.7 µs a patch; `CHECK_WORK`), spent before it, plus 32 for each patch
whose volume the check integrated to tell which way the shells face
(about 17 µs a patch; `INTEGRATE_WORK`), which `Mesh::check_counted`
reports and `Solid::new_within` spends (revolve's check too, through
`Solid::finished`; `new_repaired_within` for extrude's), after the
check: a result can pass it and still be `TooComplex`. The operands
cost nothing for their orientation, which their own check settled. With curved patches also each edge–face search a unit per 2
pieces it looked at, at least 16: the 16 spent before it runs, the rest
after each chunk of 1 024 searches (a search running to its cap of
1 024 pieces, as where two surfaces lie along each other, is 512; a
piece is about 1 µs, 2.3 µs where the edge runs along the patch and
every piece's hulls touch, twice what it was before the hulls were
tested, hence 2 pieces a unit, not 4), a
unit per pair decided, and per refinement split and piece, every round;
`MAX_TRACE_STEPS / 64` per arc not between two planar patches, a unit per
curve of the chains, a unit per curve halved in the rounds of cutting
the faces (each of which counts its ear clipping again), a unit per 4
pieces of the patch crossed looked at checking a crossing's roots on it
(see "Crossings on curved edges"), and for each crossing only placed a
unit per 4 pieces certifying it looked at, and one per 64 of the other
operand's boxes, where the patch crossed wasn't near enough. Flat
operands decided again exactly after an `Inconsistent` pay for both
tries from the one budget. `touches` with curved patches: the one
counting, then a unit per pair of pieces its search visits.

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

**The quality pass on plane faces** (release, several threads): the 60
holes drilled one at a time into the 20 × 20 × 1 box, three chains in
line (`drilled_grids_in_line`, one thread for the test), 40 s before and
46 s with it, the last body 5 454 → 6 172 patches at pitch 2.4 (5 922 →
6 566 at 2.2, 6 814 → 6 736 at 2.3); the nine grids 4 272–7 520 →
5 688–8 568 patches. The 6 × 6 grid of holes cut at once, 2 150 → 3 428
patches, 0.59 s → 0.53 s. The 150 plates of bosses sunk through
drilled plates, 117 s → 113 s; the release kernel suite 218 s → 262 s,
`drilled_grids_in_line` now in it.
On the 210 × 210 × 5 plate with 400 holes (23 112 patches), cuts beside
them (a box at a corner, a hole and a boss in a web, a pocket across
four holes) take 1 to 3 s each, the pass 6 to 50 ms of that, adding at
most 40 patches. Fuzzed in release, about 6 000 operations: plates
drilled and pocketed one feature at a time (holes of many sizes and
pitches, slots, rounded and spline pockets, ends flush with the faces or
`1e-6` and `1e-9` past them) on frames far from the origin (`1e3`,
`3e4`), turned and tilted, at fit tolerances `1e-2` to `1e-4`, and
plates grooved across round and spline holes (fitted cuts) then
pocketed, drilled and bossed across the bands: no result wrong by
closed-form volume, the identities or sampled points, no curve off a
plane on a triangle keeping its tag, 1 and 8 threads alike, `TooComplex`
no more often; 5 to 10 % more of the drilling chains' steps work with
the pass (549 against 523 of 768 at `1e-3`, 229 against 205 of 288 at
`1e-2`). The exception is a tie: a
box whose face runs exactly along a groove's rim on a plate's top and
through a hole's quarter points fails 10 more of 400 operations with
the pass (the face moved `1e-9` to `1e-3` either way fails with and
without it).

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
(steps joined flush, a hole, a half cut away, filled back in); turned
flush grid boxes (450 operations in a release build, every one whose
exact result is a manifold right and the rest `Invalid`; eight pairs
once `Inconsistent` for a first order that was only rounding and one
whose edges' shadows rounded parallel, all right now; chains of them fed
on, never `Inconsistent`); the same boxes, one moved by about the tie
distance, whose near ties don't fit together, decided again exactly
(right, both tries paid for); grid boxes extruded on frames a hair
apart whose in-plane crossings landed outside their triangles (a union
and difference chain once leaving the tool's side on the target's flush
face facing against its tag, and a difference whose in-plane edge
misses its triangle, decided again exactly, once `Invalid(Fold)`); a
box and a prism a hair apart less a box touching them from outside,
whose edge with one end in its flush plane crossed it outside the
triangle (once `Invalid(FacesAgainst)`); grid boxes at a hundredth of
the size, chained, whose vertices an ulp inside a side's plane had no
triangle paired with it by the flat broad phase (the intersection once
empty and the difference whole, both `Ok`); the
turned grid boxes, one moved along `UP` by 1
to `10⁷` tie distances, near the origin and a million from it, every
result the moved boxes' volume (two were wrong while edges whose
shadows lie along each other took their height as it came); a hexagonal prism upright and on six
random turned and moved frames with boxes extruded on a frame on its
slanted wall, joined flush (also at the wall's ends, sharing the
prism's edge lines, and in its corner, flush with the top), pocketed
flush, a slot the wall's height and straddling it, every operation the
analytic volume (with only the constant term tied, 5 of the turned
frames' 216 were `Inconsistent`);
face names
of both operands kept, and faces no triangle uses dropped; `touches`; empty operands; refusals (inside
out, out of budget); the same bits at 1 and 8 threads. Unit
tests: expansions against known values, the float filter never
contradicting the exact sign, ties whose first order is zero (collinear
edges' `Height`, also where their shadows round exactly parallel, a
vertex moving along a face's plane for `Reach`) moved by rounding and
decided as the exact tie in every draw, real first orders `2¹⁷` from
the origin (a vertex's direction into a face, an edge `1e-3` above a
parallel one) still deciding, edges whose shadows lie along each other
under the one above wherever they are decided to cross, the curved primitives'
`first_sign` skipping a motion's first order that is only rounding (a
motion in a turned plane against its normal), `orient2d` near a line
and far out,
triangulating a square with a hole, a concave loop, a zero-width loop and
a vertex landing on the domain's side (no diagonal along a side); with
curved sides, a point added where two arcs of one curve meet, and a curve
asked to be split where it closes a corner with a straight side.

Curved (`curved/tests.rs`, `pairs/tests.rs`): the ray-derived shadow
crossings against dense polylines of random curves, straight and curved
(and the solved crossings' parameters, signs and heights); straight edges'
exact rays against the numerical ones; the points of random patches above
a point adding up, by facing, to the winding number of their shadow's
boundary, and the point below a vertex `1e-3` off a steep wall's patch
found; edge crossings on both the edge and the patch, and a line
through a cylinder crossing it once each way where it should to `1e-12`;
picking crossings to fit a count (those only placed marked so);
points on random patches certified near them, and the same off them
along the normal or on the surface past a side not; Bernstein roots. Pair decisions: a
cylinder through a box both ways round (two closed curves, every end on
both surfaces to `1e-9`, windings 0), a blind hole (one curve, the bar's
end inside), crossing cylinders (two curves on both cylinders), an arc
passing through a turned face and back (two crossings counted 0), a hidden
loop the counting alone can't see (a face cutting a small cap off a
round-octahedron patch: found by refinement, one curve on the plane), a
saddle cut above and below its saddle point (the four ends on its patch
joined by the side of the saddle point they are on, which the ends alone
don't say), tangent cylinders (decided, deterministic; they touch), `touches`, the
budget, joining ends round a pair, and the same bits at 1 and 8 threads.

`touches`' search (`near/tests.rs`), each within a budget of 200 000
units (less than the old refinement took on any of them) and both ways
round, at the default and the coarsest tolerance: cylinders side by
side on the seams and off them, as tall and shorter, rods of radii 1
and 0.7 along a direction off every axis, a pin against a hole's wall
through the plate and inside it, and a boss beside a plate past both
its faces tangent to its rounded corner or a flat side, each touching
at gaps of 0 and half a resolution and not at three; a tool tangent to
a hole from outside (seen by the counting at once); a box corner in a
cylinder's control hull 0.13 off its wall (not touching); the round
octahedron's cap cut 1e-4 deep inside one patch (touching); the same
answer and work at 1 and 8 threads on a tangency and a near miss;
a near miss one unit short of its work `TooComplex`; and the round
octahedron by a slab square to the diagonal its patch reaches furthest
along, at the finest, default and coarsest tolerances, at the gaps
above and at `2.5r`.

Curved booleans (`curved_tests.rs`), all four operations both ways where
it matters, every result checked with its face tags, volumes against
analytic ones (or the identities `|A ∪ B| + |A ∩ B| = |A| + |B|`, `|A −
B| = |A| − |A ∩ B|` where there are none): a cylinder through a box, a
blind hole, a thin bar within one triangle of each face, and a bar turned
off every axis through a slab (every patch on its plane or cylinder to
`1e-12`, no fitted patch); cylinders of one radius on one axis, stacked,
overlapping and one inside the other flush at an end, the second circle
drawn as the first or from another start in 3 arcs, and `Solid::cylinder`s
stacked (all four operations and the unions both ways, volumes to
`1e-9`, unions in at most 64 patches), on the side planes and on turned
and upright frames too (stacks of the circle in 3 arcs turned, a pin
through a plate's hole, circles from cosines and sines and through
exact axis points); a profile extruded again over longer and
overlapping spans (a plate with a hole, a rounded rectangle, an outline
with elliptic and parabolic sides; on XY, XZ and YZ; all five
operations against area × span, unions in at most four times the
second's patches and 16), and the app's example plate joined again
taller in the regen tests; crossing cylinders (volumes against a Simpson
integral within a tenth of the fit tolerance times the area, the cut's
vertices on both cylinders within a quarter of it, only the bands
fitted); a pin through a plate's hole wall (upright cylinders meeting in
lines, exact); a boss joined flush on a plate; a block through the
plate's hole, and one whose side runs exactly through a vertex of the
plate's caps (a tie); a round octahedron cut through its middle and with
a hidden loop; the saddle above and below its saddle point; tangent
cylinders (union not a manifold, the rest the operands); a plate drilled,
joined and drilled again, fed on; merging back a ball and slab refined
and not cut; the same bits at 1 and 8 threads; random turned bars
against boxes, 24 for each of three seeds (200 for each of four in an
ignored test), each result exact (every patch on its surface to
`1e-11`, none claiming no surface) or refused, with the volume
identities to `1e-11`; a hole sketched on a triangular prism's slanted
wall, upright and turned, cut square to it through the prism and out of
the base's wall in ellipses, all four exact to `1e-12` with volumes in
closed form; found by fuzzing:
cylinders side by side `1e-9` apart at the coarsest tolerance (right or
refused); walls tangent along a line at the default tolerance
(cylinders side by side, a pin of radius 0.5 against a plate's hole of
radius 1, off the seams): the differences both ways the first operand
to `1e-9` in volume and the intersections empty, within 200 000 units,
the same at 1 and 8 threads; an ellipse's tip (half-axes 2 × 0.5)
poking `3e-5` and `1e-3` into a unit circle's wall, all four exact to
`1e-12` against the lens by Green's theorem; a box's face through a
bar's refinement midpoints (its plane
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
edge's conic, not at the segment's parameter). Crossings the search only
placed, and bands past the fit tolerance: a curved rim against a
crossing cylinder's wall (a crossing 4 resolutions off it, four `Ok`s
before, then refused), all four `Ok` with every new vertex on both
surfaces; two bars turned through boxes (crossings `5.6e-4` and `2e-4`
off the bar) and a small cylinder across a 75° wall in three pieces
(`1.3e-4`, on triangles along no cut), at up to three tolerances, all
four exact (no patch claiming no surface, every patch on its surface
to `1e-12`) with the volume identities to `1e-11`; a third bar, whose
nearly straight section arcs' weights came from a rounding's worth of
bulge (a sliver `1.7e-6` off on a copy claiming no surface), and a box
turned a thousandth of a radian across a wall over an ellipse arc
(`5.6e-7` off in volume), the same; and through a test-only switch (`assemble::LOOSE`: crossings
only placed left where they were and not checked, and the crossing
searches not dropping pieces by their hulls, so they run out of pieces
on those bars and walls as they used to) each result refused
or its claim-free patches within the tolerance of their walls, the
union (and `bar − box`) refused as too complex at the finest tolerances
(the small cylinder's union at `1e-5` as inconsistent: the cut from the
crossing off its wall isn't traced, and its fallback is off the cut),
and all four through at the default one, where the bands are between
half the tolerance and the tolerance, their volumes right. Tall walls:
the 60 × 40 plate with its hole of radius 8, 100 to 10 000 tall, drilled
through by a cylinder of radius 3 at four places, and a box drilled
where its cap's diagonal crosses the drill, 300 and 1 000 tall, each to
`1e-9` of the closed-form volume (`drilling_a_tall_plate`; regen's
`a_tall_plate_is_drilled_through_all` the same through the example
document), and a cap edge across a drill 1 010 tall crossing it twice,
each search looking at no more than 100 pieces, at the closed-form
parameters to `1e-12`, and a crossing `5e-11` past a cylinder patch's
side (off the axes, so the boxes don't part the pieces) found with the
hulls as without them, and one `5e-11` above a wall's level top rim,
once with the edge's range split between the rim's height and the
crossing, once `2.5e-11` past the edge's end; a hole's rim arc through
a pin 1 010 tall and 2 wide crossing it twice, at the circles' meeting
points to `1e-12`, each search under 200 pieces, and pins across the
1 000 tall plate's hole, less and joined, to `1e-9` of the closed-form
lens (`pins_across_a_tall_plates_hole`). Plane sections round a tip
(boxes grazing a cylinder of 5 or 6 arcs, each result right to `1e-9`
by its closed-form volume, the caps' antiderivative along the height,
or refused): a face `1e-4` off the rulings `1.75e-4` inside the wall,
whose cut is a U round an ellipse's tip (the sliver reaching z 4.25
when `Ok`; its intersection refused by the check on fallback chords,
seen by a test counter of chains refused), and one `1e-5` off and
tangent on a seam between two arcs, both ways round, no patch claiming
no surface; a plane half a radian off a boss's rulings at 24 offsets
across it, all 48 intersections and differences exact (a test counter
of plane–quadric chains not exact stays 0) and within `1e-9` of their
closed-form volumes. An upright cylinder nicked by a level one a few
`1e-5` short of touching it: in the differences the short arcs of the
cut loop fall back to the conic along their end tangents (fitting fails
there with the second operand's faces turned over), which the check
keeps (none refused, by the test counter), each result through within
`1e-9` of the closed-form volume (the two chords' product integrated
across) or refused later. Unit tests:
exact ellipse arcs of a tilted plane through a cylinder, crossings
solved exactly on a plane and a cylinder, crossings only placed going to
their root on a tilted cylinder's patch from `1e-3` and `0.05` off (a
segment and an ellipse arc, to `1e-15`; the other sign, a root off the
patch or none: the position stays), a line grazing the wall `1e-5` off
a ruling placed a third of the edge off, a tangent line (a double
root), a plane crossed twice (each sign its own root wherever placed),
the second point of a line on a cylinder and at infinity on a parabolic
cylinder, tracing crossing cylinders and fitting at two tolerances,
inverting a point into a patch; elliptic and circular cylinders told
from their quadrics (centre and axis; cones, spheres, paraboloids,
hyperbolic and parabolic cylinders not), and plane sections of both, from
`1e-9` to 1 radian off the rulings and a thousandth to three long, with
weights the eccentric angle's to `1e-15` and middles where it is halved.

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
test holds a floor on the share that works, but for the near-tangent
cylinders, which say per operation which may fail (below). Its tests: coaxial, stacked
flush, nested, crossing and across pairs on random frames, on a grid
and off it; parts built in chains of twenty (plates, bosses, slots,
rounded blocks, plates with holes on the sketch planes, joined, cut and
now and then intersected, each result fed on); solids turned and moved
at random against boxes and bars; cylinders side by side with gaps and
overlaps of `1e-9` to `1e-3` at two tolerances (fits 0.1 and 0.01
only: at the default fit their tangencies run out of budget); plates drilled hole
after hole (in rows, or anywhere on a grid); pins in holes of their
own circle and cylinders of one radius stacked and overlapping, on the
three sketch planes, a frame turned off every axis and one whose caps
are upright off the axes (at least 97% must work: 195 of 200 do, the pin standing on the plate over its hole refused on each); bosses
flush on plates, the union also with the boss first; flush caps with
curved rims from one sketch plane (plates, drilled or not, or circles,
rounded squares and slots, against such an outline over the same span,
sharing one cap's plane, standing through or flush with the top; at
millimetre and unit scale, on sketch planes and turned frames; all four
operations both ways round), where a union over 10 times its operands'
patches counts as failed (the heaviest that works is under 7 times; 59
of the 60 unions work, at least 95% must, and 210 of the 240
operations, at least 86%; before the seams were mended 38 and 189);
bosses on plates drilled twice, standing on them, sunk from their bottom
up twice their thickness or through them flush with both caps, against
closed forms of the shared area (the disc less the holes, integrated
exactly between where the circles meet), a result over 20 times its
operands' patches counted as failed (the first 40 cases, which hold
every failure of the 150, 148 of 160, at least 92%; all 150 in the
ignored `many_bosses_sunk_through_drilled_plates`, about 25 s alone in
release, 588 of 600, at least 97%); the same bits at 1 and 8 threads.
Each test prints its tally (`TALLY name: ok of total`) and each refusal
(`REFUSED`), seen with `--nocapture`. In release it runs in about 25 s
(37 s one test after another, 83 s on one thread) and the 40 drilled
plates with bosses, about 7 s more, on top; debug builds
run one case of each. Unit tests for the step: near ties decided
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

**The near-tangent test's refusals.** 8 (fit, gap) settings × 3
placements × 4 operations; 72 of the 96 work. The test holds each
operation to a rule (`tangent_may_fail`) instead of a share: any gap
of at least the resolution must work, and so must every intersection
and difference but the 6 below; the 24 that fail are of two kinds:

- 18 unions with `|gap|` under the resolution: at gap 0 and ±`1e-9`,
  ±`1e-6` at fit 0.1 and `1e-9` at fit 0.01 the union touches along a
  line, or has a neck or parts closer than the resolution, which no
  manifold at the kernel's resolution holds: `Invalid` (`Hull`,
  `VertexNeighbours`), `Inconsistent`, and at gap −`1e-9` once
  `TooComplex` (the tangency's refinement out of budget). Right to
  refuse; the test allows but doesn't require it, so non-manifold
  results or better ties won't break it.
- 6 intersections and differences at gap −`1e-6`, fit 0.1, on the two
  placements whose seams lie on the tangent line (not the one turned
  0.3): `Inconsistent` from `pair_decision`'s coincidence shortcut.
  The pieces there are refined to the floor and planar within the
  resolution, `one_surface` says the two walls are one surface within
  the resolution, and yet the pair has ends, because the flat
  predicates count the overlap by heights along `UP`, which on a wall
  nearly parallel to `UP` reads 16 times the normal distance (see "Ties
  are decided to first order", in Known gaps). A wrong refusal: the tie
  gives the operands unchanged, as gap 0 and the turned placement do.
  Unreachable in the app at the default fit, where such tangencies run
  out of budget first; they wait on a redesign of ties at tangencies.

No wrong `Ok` among them: every result passed the volumes, tags and
sampled points.

### Known gaps

- **Coaxial unions on turned frames.** After per-crossing decisions for
  edges lying on each other, what the coaxial sweep above still refuses:
  34 `Inconsistent`, all square prisms flush on an XY frame moved off
  the origin or turned about `z` (flat near ties: the exact predicates'
  `Height` and the order after a tied constant, not the curved
  primitives); and a pin standing on a plate over its hole, touching
  only along the rim (no manifold), on every frame. The 10 unions and
  one difference on turned or upright frames that failed as `Invalid`
  (stacks `1..2` and `−1..0` of the circle in 3 arcs turned, `0..2`, a
  pin `−1..2`) weren't the flush rims' extras or near twins: a diagonal
  grazing a cut's vertices and a chain through ends a rounding apart
  (see "Triangulating a face's loops" and "Chains"); all go through
  now: the sweep 44 → 37 failures, 46 → 37 with exact axis points, all
  37 the square prisms; the seeded coaxial tally 191 → 195 of 200. A
  wider sweep (ellipses of half axes 1.6 and 1 in 4 arcs against 3
  turned, over 7 spans; a rounded rectangle as a pin in its hole and
  stacked; a union fed on to a cross hole through the seam and a flat
  cut across it; 9 frames, 1 170 operations) fails none, against 8
  before (the same folds, and the rounded rectangle's flipped zero-width
  triangle).
- **Coaxial walls of a smooth chain of conics.** A profile whose curved
  segments are different conics meeting smoothly (a spline's fit; here
  nine conics through `r = 1 + 0.08 cos 3θ`) extruded twice on one axis
  (stacked, a pin in its own hole, joined taller) runs out of budget
  (`TooComplex` after 3 to 20 s, `Inconsistent` on YZ): where two
  segments meet, one operand's wall is tangent to
  the other's next wall along the ruling there, two quadrics touching
  along a line. With a corner at each joint instead it works, in 500 to
  1 000 patches; a chain of circular arcs (one quadric) in some 60.
- **Coaxial walls a little apart.** Cylinders on one axis whose radii
  differ by `1e-5` (ten times the resolution) refine their walls' pairs
  until the hulls part and run out of budget (`TooComplex` after 6 to
  25 s each); by `1e-7`, under the resolution, they fail as
  `TooComplex`, `Invalid` or `Inconsistent` (on YZ), or come out with the
  two walls as one, volumes within the gap's.
- **A cross hole across a joined body's wall seam** can fold: where the
  hole's circle passes through a wall triangle's diagonal at one of its
  own vertices, the piece of the diagonal inside the circle is a chord;
  the clean-up collapses the tie beside it and gives the chord the arc's
  curve, two arcs of one circle meeting at a corner of the triangle
  below (`Invalid(Fold)`; a plate with a slot joined over `−3..12`, then
  intersected with a hole of radius 1 at `(3, 10.5)` across it).
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
- **Coplanar faces meeting along a curve** (flush caps with curved
  rims, see "Seams") are mended by the clean-up but for regions whose
  boundary passes a vertex twice or has a side about zero long (a
  cluster on the region's edge), and regions whose new triangles
  wouldn't all be proper: those keep their seams, and repair splits
  along them down to flat pieces as before (right, heavy). None was
  seen in unions; two differences of a boss less a drilled plate have
  such a cluster where opposite-facing caps meet and fail as
  `Invalid(Fold)`, as before. Faces of one plane that
  no mended seam joined keep their own names (a box first against a
  flush plate, a boss crossing a plate's edge with the plate first), and
  the line between them is drawn.
- **Curved cuts near arcs fail as invalid now and then**: a planar cap's
  triangle whose arc bulges out of it after the rounds, or a flat sliver
  along a cut next to a curve that no flip may take (the boolean's own
  cap triangles aren't fold-checked). None came out wrong. Boxes cut
  across a convex wall over an arc meet it most: 8 of 120 random box
  operations across 20°–60° walls fail (unions and differences, all on
  convex walls), and a box whose face runs along the arc's chord (inside
  the bulge) fails its union and difference. These are the boolean's own
  cap triangles, not refinement: in the 8 the refined operands have no
  piece failing the fold check, and the result before repair has a cap
  triangle with one curved side (the cut piece of the arc, `w < 1`) fanned
  to a far corner of the square, the arc bulging across its straight
  side (`Invalid(EdgeNeighbours)`), and in 6 of them also a flat sliver of
  three points in line along the box's face, a degenerate corner that
  repair stops on (`Invalid(Fold)`). Folds from refinement are the cap
  gap under Profiles ("Repair of a cap patch along a concave curve"). Results off by more than
  `1e-7` in volume (up to `6.6e-5`) came from crossings the search only
  placed, off the surface they cross, with the triangles at them on
  copies claiming no surface: they are refused now (below).
- **Fitted bands leave their face's claim**: triangles along a fitted
  cut on a quadric (quadric against quadric, a quadric against a free
  surface), and an exact band tree's root where no ruling frees it, go on
  a copy of the face claiming no surface; a later boolean then traces
  and fits where they are cut again instead of cutting exactly.
- **Section weights from `σ` off elliptic cylinders**: on parabolic and
  hyperbolic cylinders and other quadrics a plane section's weight still
  comes from where the line from the chord's middle to the control point
  meets the quadric, noise for an arc bulging by a rounding, and an arc
  within `1e-9` of straight is a straight edge (up to `1e-9` of its
  length off). On a parabolic cylinder every section's weight is 1 (an
  affine image of a parabola arc), which isn't used yet. Of 800 random
  bars through boxes, two (seed 7, case 189; seed 1, case 97) fail their
  intersection and difference as `Degenerate`, as before.
- **Plane sections nearly along a cylinder's rulings trace near the
  tip, or are refused**: a plane `α` off the rulings cuts a cylinder of
  radius `r` and height `h` in the tip of an ellipse whose curvature
  radius there is `r·sin α`; halving an arc at its parametric middle
  halves the distance to the tip each time, so it takes about
  `log2(√(2h/(r·α)))` halvings, past the 6 allowed below `α` of about
  `1e-3`. Those arcs are traced, or, where tracing and the conic along
  the end tangents fail too, their chord is checked against the cut and
  the boolean mostly refused (`Inconsistent`). Halving where the tangent
  bisects the end tangents (a quadratic in the parameter) would make
  them exact, but needs a floor for tip arcs under the resolution, which
  fail otherwise (`Degenerate`, `Invalid`, `TooComplex`). And a section
  end a little off the cylinder (a crossing near a double root, the
  plane a hair from tangent: one was `1.1e-7` off) takes its weight from
  `σ`, noise for such flat arcs, so both arcs fail and the chord is
  checked as well. The app makes such planes only from a sketch line
  drawn within a milliradian of the axis direction, placed within
  micrometres of tangent to a boss.
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
  the resolution apart) decide one way or the other by rounding, but
  that isn't what made flat operands `Inconsistent`: scaling the tie by
  1/64 to 4 changed none of them. A first order that is only rounding
  where the exact tie's is zero did, and is now skipped (see "At every
  order"; the curved primitives' `first_sign` skips such orders too).
  Flat operands decided with near ties that don't fit together are
  decided again exactly, so they aren't `Inconsistent` (none seen), but refused as
  `Invalid` where the exact result has parts closer than the
  resolution; curved operands still can be (11 of the seeded suite's 96
  near-tangent operations). Three thresholds meet there with no order
  between them: pieces are taken as planar within the resolution and
  `one_surface` holds within it, both in normal distance, while the
  count's tie is a 64th of the resolution in height along `UP`. On a
  wall nearly parallel to `UP` a height is the normal distance over
  `|n̂·ÛP|` (about 0.06), so an overlap of about 0.06 to 1 tie distance
  passes `one_surface` and still counts as a crossing: the pair has ends
  and is `Inconsistent` (6 of those 11, measured at fit 0.1 with the
  seams on the tangent line; turned off it, overlaps up to 10 ties
  work). Far from the origin the rounding rule's
  share reaches some tens of tie distances in a coefficient of many
  terms: edges stacked along `UP` 10 to 125 tie distances apart, `1e3`
  to `1e6` from the origin, are decided as touching (near it, up to
  8), still within the resolution.
- `touches` takes curved solids within the resolution (and up to about
  2.4 times it) as touching, flat ones within the tie distance. A tangency
  along a line now touches, and the boolean it leads to is where line
  contacts are decided (a union touching along a line isn't a manifold
  and is refused; parallel walls with no certificate still refine,
  slowly, see "Failing operations").
- Each refinement round counts both operands again from scratch, and a
  search stops at 1 024 pieces (placing a crossing it didn't find where
  it found the two meeting, else where they came closest; since pieces
  whose hulls are apart are dropped and long pieces halved, tall and
  long patches seldom reach it, mostly edges running along a surface,
  as a tool's straight edge a hair from a tall wall and parallel to its
  rulings): pairs a
  certificate can't settle (two cylinders tangent or crossing at a
  slant) refine for many rounds, and
  parts built in long chains occasionally run out of budget there.
- **Crossings only placed are refused where no root holds them**: on a
  face claiming no surface, or where no root of the crossing's sign lies
  on the patch crossed, a crossing the search only placed is checked
  and refused if it isn't on the other operand (`Inconsistent`). A
  curved edge touching a quadric is a double root of the quartic, which
  the isolation gives as rounding has it (none, or two close together);
  the quartic's levels on the quadric are candidates too, so such a
  crossing goes to the touching point. Placing
  them at checked roots on planes and quadrics won back more than the
  check cost. The seeded suite's tallies didn't move through either
  change (bars 22 of 24 pairs, walls 112 of 120, coaxial 37 of 40,
  bosses 64 of 64, drilled 160 of 160, tangent 72 of 96, chains 203 of
  240, turned 156 of 160, related 112 of 120), and the five bosses
  joined flush over drilled holes all work again. In a fuzzer of walls
  over arcs, conics and circles cut by boxes and cylinders, alone and
  fed on (about 2 050 operations a seed, default tolerance), and of
  1 000 boxes turned across such walls, refusals (of which
  `Inconsistent`), and results off in volume by more than `1e-7` (most
  within the tolerance):

  | run | before the check | with the check | placed at roots | weights by angle |
  |---|---|---|---|---|
  | walls, seed 21 | 101 (6), 47 off | 149 (94), 0 off | 73 (9), 0 off | 68 (2), 0 off |
  | walls, seed 5 | 104 (1), 31 off | 136 (83), 0 off | 59 (1), 0 off | 59 (1), 0 off |
  | turned, seed 21 | 27 (0), 9 off | 37 (20), 3 off | 17 (0), 3 off | 17 (0), 0 off |
  | turned, seed 5 | 18 (0), 3 off | 23 (5), 0 off | 18 (0), 0 off | 16 (0), 0 off |

  At the finest tolerance (`1e-5`, seed 21) 79 refusals (21
  `Inconsistent`), none off. The last column also has the points of a
  patch above a vertex that Newton's method finds from another piece
  (see "Curved primitives"): the fed-on `Inconsistent`s (a wall over a
  parabola cut by a box, then a small cylinder `1e-3` through the wall;
  a small box on a circle's wall) went with it, refused now by the hull
  rules (`Invalid(VertexNeighbours)`) where they are refused; the
  `Inconsistent`s left failed so before either change. The three off,
  one turned case (a box turned a thousandth of a radian across a wall
  over an ellipse arc), went from `3.6e-6` to `5.6e-7`, and to `1e-13`
  with section weights by angle on elliptic cylinders.
- **Bands past the fit tolerance are refused, not mended**: halving
  can't move bands at a vertex off the surface, and a triangle off its
  quadric with no side on the face's boundary has nothing to halve; at the finest tolerances such cases are `TooComplex`. Fuzzing
  walls at fits `1e-4` and `1e-5` found no result where the bound on
  triangles off their quadric (not only those along a cut) changed the
  outcome: it is a backstop.
- **Chained flush grid boxes**: 1 of 140 000 steps fails on a manifold
  result, `Invalid(Hull)` (in half-grid units: the box at `[2,2,1]` of
  size `[2,5,4]`, less `[0,3,3]`+`[5,2,2]`, intersected with
  `[2,3,1]`+`[1,4,2]`, united with `[3,0,2]`+`[4,6,2]`). Folded sheets
  whose two sides are triangulated differently are unfolded by the
  clean-up ("Unfold"); one where every collapse onto the folded vertex
  leaves a pinch (two vertex ids at one point) would still fail, as one
  did before other fixes, but none was seen in these runs.
- **Flush faces a hair off each other.** Grid boxes extruded each on
  its own frame, the frames turned alike but for `1e-11` to `1e-7` rad,
  fail about 1 operation in 9 (`Invalid`). About 1 in 750 used to come
  out with a triangle facing against its plane tag (an in-plane crossing
  placed outside its triangle, see "In-plane crossings"); none does now,
  and `check` refuses any that would. What is left of the window: edges
  near a face's plane with neither end within the tie of it, decided by
  the same mismatch of `Reach` and `Height`, still have crossings placed
  up to well beyond the resolution outside their triangles (some 850 per
  12 600 hair-frame steps before edges with one end in the plane were
  kept in; none gave a false tag in the hunts), so do edges with one end
  in it and no part inside the triangle, planar
  patches with curved sides aren't `Input::flat` and are left alone, and
  on the curved path an edge missing its triangle fails as
  `Inconsistent` with no exact retry. Unit cylinders tangent along a
  line on a far turned frame, one's top a tie above the other's, gave a
  difference with a cap triangle against its form; it is now refused
  (`Invalid(EdgeNeighbours)`).
- **Flush faces after rounding**: flat solids flush in exact arithmetic
  but turned and moved work (2 653 of 2 700 turned grid boxes'
  operations, 349 of 359 steps of turned chains); the rest fail as
  `Invalid`, all but one where the exact result isn't a manifold (pair
  866's union: a thin triangle across two faces fails the hull rules).
  A box joined flush in the top corner of a hexagonal prism's slanted
  wall (sharing the prism's vertical edge line and its top), on random
  turned frames, fails the same way as `Invalid(Hull)`, 8 of 60 frames;
  upright it works. Turned grid boxes moved off flush by `1e-4` to
  `1e-2` leave slivers that thin whose triangles fail the hull rules
  (`Invalid`) some 5 % of the time.
- **Long cap triangles and cuts passing close to their sides**: a cap
  triangulated once keeps long thin triangles from far corners to rims.
  Drilling a second hole of the same size beside the first, in line with
  it, on a large plate (a 20 × 20 box, holes 2.4 apart) left a band a
  few tenths of a millimetre wide between the new rim and such a
  triangle's side, 16 mm long: the triangles across it reach from the
  side's far ends to the rim, and those from either end whose line
  grazes the rim close a corner there that halving the rim never opens
  (a fold), or, with the rim split at its point nearest the side, are so
  thin their hulls come within the resolution. One vertex under the rim
  moved the grazing lines elsewhere, and failed more flush bosses and
  drilled plates than it mended. The clean-up now refines the plane
  faces a boolean cuts (see "Clean-up"), so a cut face's fans are gone
  before the next operation; an extrude's caps aren't refined yet: a
  small box cut at a corner of a plate with 400 holes crosses two long
  edges from the plate's corner 3 µm apart, and fails the same way.
  - Tests (release, default tolerance; before → with the clean-up's
    pass):
    - `a_box_drilled_twice_in_line` and `holes_in_line_on_a_large_plate`
      (`curved_tests/holes.rs`): the 20 × 20 × 1 box, pins of radius 0.5
      at (1, 1), then (3.4, 1) or (1, 3.4); every step must work, its
      volume `400 − k·π/4` within `1e-9`, the same at 1 and 8 threads.
      Both second holes failed, `Invalid(Hull)`; they work.
    - `cut_caps_are_well_shaped`: after the first hole, the triangles the
      cut made on plane faces (not an operand's as it was) must have no
      angle under 5° (the pass's bound), but where the narrowest corner
      lies between two constrained sides (curved, or on another face)
      meeting at under 60°, the circumradius is under `MIN_SPLIT`
      resolutions, or a side is no longer than an eighth of the
      resolution. 24 of the 32 were under 10°, the worst of sine
      `3.75e-3` (fans from the box's far corners to the rim); now none
      of the 80 is under 5.7°.
    - `holes_in_line_on_moved_frames`: the same three holes sketched on
      frames far from the origin, turned and tilted, each step against
      the plate's volume less `k·π/4`, at 1 and 8 threads.
    - `fitted_grooves_across_drilled_plates`: a plate with a round and a
      spline hole on a tilted frame, grooved through both (fitted cuts),
      then pocketed and drilled across the bands, each step against its
      intersection within the fit tolerance's allowance, at 1 and 8
      threads.
    - `drilled_grids_in_line` and `boxes_drilled_in_grids`
      (`seeded_tests.rs`, the second `#[ignore]`d as slow): 60 holes
      drilled one at a time in rows of 8 from (1, 1), a failed step
      skipped, every step that works checked against `400 − k·π·r²`.
      Failed steps (0-based) and the last body's patches, before:

      | r \ pitch | 2.4 | 2.3 | 2.2 | 1.3 |
      |---|---|---|---|---|
      | 0.25 | 39; 4 806 | – | 3, 24, 48; 4 272 | none; 4 844 |
      | 0.5 | 1, 8; 5 454 | none; 6 814 | 3, 24; 5 922 | 3, 5, 24, 40; 4 594 |
      | 0.6 | none; 6 032 | – | none; 7 034 | none; 7 520 |

      and with the pass:

      | r \ pitch | 2.4 | 2.3 | 2.2 | 1.3 |
      |---|---|---|---|---|
      | 0.25 | none; 5 954 | – | none; 5 990 | none; 5 688 |
      | 0.5 | none; 6 172 | none; 6 736 | none; 6 566 | none; 7 592 |
      | 0.6 | none; 5 918 | – | none; 6 378 | 29; 8 568 |

      12 of the nine grids' 540 steps failed, 4 of the in-line test's
      180, all `Invalid(Hull)` or `Invalid(VertexNeighbours)`, most of
      them the first or second hole in line with an earlier one; now 1
      and none. The one left (r 0.6, pitch 1.3, step 29, its hole 0.1
      from the last) fails as `Invalid(VertexNeighbours)` between two
      tiny triangles of the new hole's wall where it meets the plate's
      bottom at the seam of its arcs, not on a plane face.
    - `larger_grids_of_holes_cut_at_once` (`#[ignore]`d): a 100 × 100 ×
      10 plate cut through at once by an `n × n` grid of discs of radius
      2 at `100·(k + ½)/n` (one profile, 1.1 past either face, as the
      app's through-all cut), checked against `100 000 − 40π·n²`: 8 × 8
      fails as `Invalid(VertexNeighbours)` (2.4 s), 10 × 10 as
      `TooComplex` (2.8 s); 6 × 6 works
      (`a_six_by_six_grid_of_holes_cut_at_once`, 2 150 patches, 3 428
      with the pass). The 10 × 10 runs out before the clean-up (and
      with eight times the budget). The 8 × 8 is the operation's own
      triangulation: a fan from a rim vertex of one hole to two of
      another 60 mm away, the side to one of them within `1e-3` radians
      of that rim's tangent there, so its corner is closed; no flip
      opens it (every vertex across lies on nearly the same line) and
      no point inside does.
    - `a_boss_through_a_drilled_plate_across_a_hole` (`#[ignore]`d): the
      6 × 4 × 1 plate drilled at (−1.1, 1.15) r 0.45 and (1.95, 0.25) r
      0.6, a boss of radius 0.65 at (−0.8, 0.6) through it flush with
      both faces, its wall crossing the first hole's (see "Flush bosses
      on drilled plates"): all four operations fail (`Invalid(Hull)`,
      `VertexNeighbours`), with the pass too, checked against the
      closed-form area of the disc less the hole.
  - Baselines a change to the triangulation of plane faces must not lose
    (release), before → with the pass: the seeded tallies bosses 80 of
    80, flush unions 59 of 60, flush operations 210 of 240, coaxial 195
    of 200, turned 156 → 158 of 160, drilled
    (`plates_drilled_hole_after_hole`) 160 of 160, bosses in drilled
    plates 148 → 149 of 160, related 113 of 120, chains 200 of 240,
    tangent 72 of 96; the 150 plates of
    `many_bosses_sunk_through_drilled_plates` 588 of 600 (3 unions, 4
    intersections and 5 differences fail).
- **Cross holes through round bosses**: a round boss (an r10 circle
  extruded on XY, 10 tall) cut through by a circle sketched on YZ and
  extruded across it (radius 0.3 to 4, placed at random, 30 cases)
  failed 7 of the 30 differences: `TooComplex` once repair ran out of
  budget, 1.0 to 1.6 s each on one thread. The unions and intersections,
  and a 60 × 8 plate drilled the same way, all worked; none came out
  wrong. The points for shapes in cut faces on curved patches (see
  "Cutting curved faces") mend all seven
  (`a_cross_hole_through_a_round_boss`).
  - Why: a cut face is triangulated on its input triangle's corners and
    the cut's vertices only (see "Triangulating a face's loops"), so a
    hole inside one of the wall's strip triangles (7.65 wide, 10 tall)
    fans to corners 5 to 8 away from rim pieces about 0.02 long:
    claim-free bands of aspect 300 to 1 000, with angles about `1e-3` at
    the far corner and long sides following the cylinder (control points
    about 1 off their chords). The drill's wall inside the boss is cut
    the same way, into bands 8 long from the rim to its refinement line.
    Where a boss band and a drill band share a rim edge, the pair fails
    the edge-neighbour rule (3 pairs in the first round of the case
    looked at). In a union the drill's wall kept is outside the boss,
    and the same boss bands pass the check at once. Red splits keep the
    bands' shapes, so repair quartered them round after round (about 500
    pieces to 6 000 to 10 000, over 16 to 49 rounds) until the budget
    ran out.
  - Measured (release, default tolerance): the app sweep (90 operations
    on each part), the horizontal drills through walls 10 tall (a box,
    an r10 cylinder and arches, 90 operations each), boxes turned across
    walls (750 operations), the walls fuzzer (420), the seeded suites
    and the longer walls and turned fuzzers (seeds 21 and 5: 2 070 and
    1 000 operations each). The first rows are a spike's scratch
    prototypes.

    | | boss differences | drills: cylinder, arch | turned | walls |
    |---|---|---|---|---|
    | before | 7 | 7, 3 | 8 | 2 |
    | bisecting thin pieces in repair | 6 | 4, 4 | 7 | 3 |
    | prototype points, under 10° | 0 | 2, 0 | 8 | 2 |
    | prototype points, under 5° | 0 | 2, 0 | 8 | 2 |
    | as built (5°) | 0 | 2, 0 | 8 | 2 |

    - As built: 1 053 patches per boss result, against 1 206 for the
      operations that worked before (1 231 at 10°, 1 612 at 20° in the
      prototype); the volumes keep the identities (union and
      intersection, difference and intersection) within 2 % of the bound
      `fit · area / 5`. The plate's results are unchanged (74 patches
      against 73). The app sweep takes about half the time it did (the
      seven failures took most of it).
    - Unchanged: the seeded tallies (coaxial 195 of 200, bosses 64 of
      64, tangent 72 of 96, chains 200 of 240, drilled 160 of 160,
      related 113 of 120, turned 156 of 160), the long-slot and scale
      sweeps through tall plates (slots refused 6, 3 and 1 of 120 at 10,
      300 and 3 000 tall), and the longer fuzzers (walls 62 and 54
      refused, turned 15 and 13, none wrong; the prototype at 10° and
      at 5° alike).
    - Bosses 1 000 tall: 10 of 90 failed before, 6 now, with 989
      patches per result against 831.
    - With the circumradius floor (none under `MIN_CURVED_SPLIT`
      resolutions) every sweep above comes out the same, patch counts
      too, and the longer walls fuzzers refuse 55 and 52 (with the
      clean-up's coplanar seams, which came in between).
  - Bisecting in repair (not built): each failing piece more than 8
    times longer than high halved across its longest side, its neighbour
    across that side first if that is the neighbour's longest (Rivara),
    before the red–green rounds. Such bisection keeps a band's smallest
    angle (at best halves it), so the fans stay fans. The propagation
    also leaves cap slivers along the fan's rays, three corners nearly in
    line, 13 resolutions high, which fail the hull rules as flat pieces
    at once (`Invalid(Hull)`, `VertexNeighbours`). In the case looked at,
    it took 37 rounds and 470 → 4 154 pieces before the red–green rounds.
  - The prototype scanned the face for the worst triangle and for the one
    holding each point, and flipped the whole face after each: quadratic.
    As built, the queue, the walk and the clearance search are local,
    and the flips only round the new point; it refuses a point the walk
    can't reach without crossing a side of the loops (the prototype took
    any triangle holding it), and one whose pieces would have a closed
    curved corner. The counts above are the prototype's all the same.
  - Left: 2 of the 90 cylinder drills (one `Invalid`, one `TooComplex`)
    and 6 of the 90 bosses 1 000 tall (the height-driven fans, where a
    face's far corners are hundreds of times further than the hole is
    wide).
  - Checked against closed forms (release, about 3 300 operations): the
    app's way on round bosses of radius 2 to 60 and 0.3 to 4 times as
    tall (extruded circles and the cylinder primitive, the boss turned
    about its axis or the pair moved anywhere, scaled by 0.02 to 100),
    drills grazing the wall, floor or top from `1e-9` to `3e-2` either
    side, through stacked and boss-on-plate unions across their seams,
    through arches, and chains of ten in a boss at separate heights or
    crossing. Every result's volume within 3 % of the bound of the true
    one (the hole's part by a one-dimensional integral), points round the
    drills inside exactly when they should be, and the results the same
    at 1 and 8 threads: none wrong. Against main the app's way failed 23
    of 180 → 11, chains 20 and 28 of 48 → 14 and 15, scaled by 100 43 of
    90 → 18; tangent drills 61 → 59 of 180.
- **Flush bosses on drilled plates**: of 150 random plates 1 thick with
  two holes and a boss standing on it, sunk from its bottom up 2 or through
  it flush with both caps (the seeded `bosses_sunk_through_drilled_plates`),
  each of the four operations, 3, 4, 3 and 2 fail (11, 6, 5 and 11 before
  the bands above and the end crossings' rule, 15, 6, 5 and 13 before the
  coplanar seams were mended); none came out wrong, and no result is
  heavy any more. What is left: a boss through the plate whose wall
  crosses a hole's wall, both caps flush (the two walls meet in a ruling
  from cap to cap, and the crossings of their edges with it come `1.2e-5`
  apart; the wall triangle fanned from a far corner to the two is a
  sliver whose neighbours' hulls meet, `Invalid(Hull)`); a boss rim
  tangent to a hole's rim (see tangent contacts); and a hole's rim vertex
  lying exactly on the boss's circle, or the boss's on a hole's (an
  operand's vertex on the other's surface, `Invalid(Fold)` or the
  neighbour rules). Built by extruding the plate with its holes (or on
  other frames), a few more sunk bosses fail where a band beside a wall's
  curve ends at a hole's wall close to the curve: the face past the band
  has the curve's piece from the vertex across from the band's end to its
  far end as one long side, and the band's end next to it, so ear
  clipping leaves a sliver there (leaving that vertex out moves the
  sliver, failing as many). On a frame tilted a degree or so, such a band
  ending at a hole's wall fails the hull rules at the curve's end instead:
  the wall triangle above the curve and the one below the cut share only
  that corner, with the band's last triangle a narrow wedge between
  them. And where a boss's span puts the split of its wall a hair below
  the plate's face (`1e-10`, inside the tie), the face's cut is put in the
  triangle below the split's curve, outside it; the vertex across from it
  then leaves a fold there, where without it the cut was pulled onto the
  curve. These fail a few operations the bands' rule had not (2 to 4 of
  600 a scan) against 5 to 11 it mends. A boss whose bottom or split is
  `1e-7` off the plate's face (under the resolution) comes out flush with
  it, the volume within the gap's, as before; from `1e-10` to `1e-6` off,
  a third of such operations fail (as before, a few less). Fuzzed app chains (random extrudes
  on the origin planes, joined, cut and intersected) fail as before, about
  one operation in six, mostly at tangencies; the same chains made body
  first on six frames (the origin planes, turned, tilted, far from the
  origin) over drilled plates and round plates, with bosses, rings and
  slots sunk, through or standing, fail 12 to 16% (main 18 to 21%), none
  wrong against exact volumes. Earlier fixes here: a boss
  whose rim runs along a cap edge between symmetric holes, tangent to it
  at a vertex both have (`Inconsistent`: crossings a micrometre apart in
  the wrong order, then zero-width triangles at the rim); a boss's wall
  cut along its flush rim getting a triangle with three corners on the
  rim (cut vertices inverted into the wall's domain a rounding either
  side of its side); a cap edge passing through a boss's rim a little
  inside it taken as not crossing (the search found one crossing of
  two). With the extrude's caps refined for quality, one such chain on
  a turned frame (`a_hole_through_a_sunk_ring_on_a_turned_frame`: a
  two-hole plate, a ring joined −1..3, a slot cut, a hole drilled −1..1)
  is refused at its last cut (`Invalid(Hull)` where the hole meets the
  ring's wall), where with the plain caps it came out right: the test
  takes right or refused. The refined plate has its bottom side halved
  (vertices at x = 0 and 0.75), so its top cap's triangle under the
  drill differs; the drill's pair refinement then leaves a vertex of
  the plate's top 2.7e-3 outside the drill's circle, beside an arc of
  it (chord 0.13, sag 3.8e-3, bulging past the vertex), and the cut
  face's triangle there, that vertex and the arc's ends, folds
  (`Fold` before repair, `Hull` after). A boolean weakness (a cut arc
  passing within its sag of an operand's vertex), not the caps'.
- Triangles thinner than the resolution across two faces (a cut passing
  within a resolution or two of a vertex) aren't flipped, and fail the
  hull rules. Counted after the clean-up (`cleanup::thin_across`, tests
  only: higher than an eighth of the resolution, no higher than 4
  resolutions, the triangle across the longest side on another face by
  source), over the release `boolean::` suite without the test below:
  of about 5 700 operations about 400 fail, 109 of those with such a triangle
  (one operation that worked had one too), and of those only one with
  straight sides and its far corner inside a plane (every triangle round
  it on one plane, every edge from it straight), the one case splitting
  the long side at the corner's foot and collapsing the corner onto it
  would mend (a box face `1e-5` off a cylinder's rulings, tangent to its
  wall on a seam between arcs, in
  `a_box_tangent_on_a_seam_is_exact_or_refused`: a tangency). With the
  clean-up's quality pass on plane faces the same: of 5 792 operations
  397 fail, 110 with such a triangle (one that worked had one), the same
  one with its far corner inside a plane. `thin_triangles_across_two_faces`
  (`boolean/tests.rs`): a 10 × 10 × 2 plate of four squares joined, cut
  by a slanted wall 0.2 to 4 resolutions from the vertex where the four
  meet, as a boss and a pocket: all 40 operations fail, 20 with thin
  triangles across two faces, none with the far corner inside a plane
  (the far corner is a cut vertex, the long side on the seam between two
  of the top's faces); the test holds that count at zero.
- Ear clipping is quadratic to cubic in a face's cut vertices; faces cut
  by thousands of edges run out of budget, now counted as they go.
- **Failing operations run past two seconds on one thread**: a unit of
  work in the counting of refinement rounds is about 0.8 µs on one
  thread (native), so an operation that runs out of budget while
  refining a tangency takes about 3.3 s; on the web's single worker,
  slower still. Since walls along one direction are certified near each
  other, differences and intersections of walls tangent along a line
  with no ends there take 3 000 to 90 000 units (milliseconds) where
  they ran out (cylinders side by side off the seams, a pin against its
  hole's wall, both orders): on a default-tolerance probe of two
  cylinders of radius 1 side by side at gaps 0 to ±`1e-5` (54
  operations) 29 work where 16 did, 25 spend over a million units
  where 41 did, total work 147 → 89 million, no result lost; the
  regen's tangent discs (a disc beside a round body along a line, at
  angles 0, 0.3 and 0.7, fits `1e-1` to `1e-4`) cut as a no-op at every
  angle and fit in 20 to 320 ms (was 3 of 12, 0.24 to 5.2 s), and join
  as "no clean solid" off the seams at fits down to `1e-3` in 15 ms to
  2 s (the repair of the refused union) instead of 0.3 to 2 s and
  `TooComplex` at `1e-3`. Since ends along one direction join line by
  line, walls overlapping by a quarter of the resolution or more cut in
  thousands of units where they refined for millions: the
  default-tolerance probe (`near_tangent_cylinders_at_the_default`,
  ignored: gaps and overlaps `1e-12` to `1e-4`, two placements, four
  operations, 88 in all) works 67, every overlap of `1e-5` and `1e-4`
  among them (the `1e-5` ones were refused after 2.6 to 4.2 million
  units), and 23 spend over a million units. Unit cylinders and pins
  overlapping by 0.02 to 30 resolutions at fits 0.1 to 0.001 (768
  operations, two sweeps): 107 → 431 work, operations over a million
  units 536 → 221, work 1.8 G → 0.75 G, no result lost. Unions at a
  tie (the walls tangent within the tie distance, `A` grown two lines
  infinitely close) ran out too: 11 of the probe's 23, every regen join
  at `1e-4` and those with the line on a seam at `1e-3`; they now fail
  at once as `NotManifold` (see "Results that aren't manifolds"). What
  still runs out: differences and intersections of walls overlapping by
  under a quarter of the resolution (`1e-9`, `1e-7` at the default
  tolerance: the probe's other 12), left to refinement (in "Thin
  overlaps run out", below, with repros); and coaxial walls `64` resolutions to a few
  thousandths apart over a millimetre or more (refined until their
  hulls part, as before: radii 1 and 1.0001 or 1.001 over 1 mm run out,
  and 1.004 the union; radii 1 and 1.00001, within the 64 resolutions,
  now cut where it ran out). The steps that ran far past what they were charged are
  counted now (the triangulations' and the counting's exact signs, the
  patches the result's check integrates), so none runs unbounded; the price is that an
  operation full of ties runs out sooner (a flat torus of 9 216 patches
  united with itself, 4.2 s on one thread, is `TooComplex` now).
- **Thin lenses between walls fail after their cut is decided**: walls
  overlapping by some resolutions, joined along their lines at once,
  still give results some stage refuses. A cylinder over the middle of
  another (`1e-5` deep, turned 0.1 to 0.7 about its axis off the other's
  seam): the lens cap's corner at the seam is two arcs of one circle,
  the triangulation adds a point at the cap's centroid, 3.3 resolutions
  from that corner (`VertexNeighbours`, `EdgeNeighbours`); `1e-4` deep,
  cut vertices along a line from `A`'s crossings and `B`'s land
  `5e-5` apart and their triangles fail the hull rules; pins poking out
  of a wall by 10 to 30 resolutions fold in unions and differences.
  These failed before too (out of budget, or the same refusals after
  millions of units).
- **Joining lines on large pieces loses a few results**: joined in an
  early round, a pair's lines leave the pieces beside them as large as
  they were, and some results that refinement gets right fail the hull
  or neighbour rules or fold; the operation is then tried again without
  the joins within a bound (see "Tried again without the joins"), which
  wins back all but those that need more than that: of the probe's
  ellipses 3 of 864 (an ellipse of half-axes 2 × 0.5 whose tip pokes 100
  resolutions into a unit circle's wall at fit 0.001: union and `A − B`,
  1.3 to 1.4 million units refined; 3 × 0.3 poking 1 000 resolutions in,
  the intersection), of the spans 2 of 1 080 (a cylinder turned off the
  axes over the middle of another, overlapping by 0.3 resolutions at fit
  0.1, `A − B`: 850 000 units). A refusal that joined lines takes up to
  twice its work, at least 150 000 units more.
- Merging restores only whole nodes of the refinement tree with no finer
  neighbour: pieces next to a cut stay as refined.
- **Thin overlaps run out**: what joining ends along one direction
  leaves refining to the budget (release, a loaded machine, the default
  tolerance unless said). Unions at a tie (walls tangent within the tie
  distance, and overlapping by under a quarter of the resolution) did
  too, 2.7 to 4.2 million units each on the probe below and 1.5 to 6.4 s
  in the regen's tangent discs; they now fail at once as `NotManifold`
  (see "Results that aren't manifolds"). What is left:
  - *Overlaps under a quarter of the resolution.* Differences and
    intersections of walls overlapping by less than a clear line needs
    (`1e-9` and `1e-7` at the default tolerance) have ends that aren't
    joined, and refinement settles them slowly or not at all: the
    probe's other 12 over a million units. At `−1e-9` all six fail
    (`TooComplex` on the seam placement after 1.7 to 4.1 s,
    `Inconsistent` on the turned one after 2.9 million units); at
    `−1e-7` five work after 2.7 to 3.7 million units (1.2 to 3.9 s,
    volumes right) and `a less b` on the seam placement runs out. Not
    covered by the union's rule (the operands do overlap); they wait on
    a redesign of ties at tangencies.
  - *A cylinder inside another, touching its wall.* Radius 0.5 inside
    radius 1, the line on both seams, at fits `1e-3` and `1e-5`: the
    intersection (the inner one) and the outer less the inner (not a
    manifold) run out after 7 to 10 s; `A` shrunk crosses `B` in two
    lines infinitely close, as a union at a tie does, but the union's
    rule takes only walls facing opposite ways in a union. At fit 0.1
    both are refused after 2 to 3.5 s. Naming the difference
    `NotManifold` the same way (walls facing the same way, `A` the
    outer) would need the operation, not just `grow`, in the decisions.
  - *A hole drilled tangent to a hole from outside, on the seam*: the
    example plate (60 × 40 × 10, a hole of radius 8 at the origin) cut
    through by a disc of radius 3 centred 11 from the origin at angle 0
    (`add_drilled(&mut editor, 11.0, 0.0, 3.0)` in the regen's history
    tests) runs out in 4 s; at angles 0.3 and 0.7 it is refused as "no
    clean solid" in 0.3 to 0.6 s. Not diagnosed further.

  Repros: `cargo test -p varde-kernel --release
  near_tangent_cylinders_at_the_default_tolerance -- --ignored
  --nocapture` prints each operation's result, units and time (`NEAR`)
  and the list over a million units; the regen cases evaluate those
  documents with `evaluate` and a fresh `Cache`.

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

- `AddFeature { name, kind }` (`Document::add_feature` names it
  "Extrude N"; the commands are shared by every kind, see
  `agents/features.md`) adds it and hides its sketch; a `NewBody`
  extrude also adds "Body N" with the id after the feature's, replacing
  whatever id the command held (`BodyId::NEW` stands for it, an id no
  body has). One undo step.
- `SetFeature { feature, kind }` replaces it whole, the caller passing
  regions freshly referenced from the sketch as it is. A `NewBody` that
  stays one keeps its body; one that stops removes the body and drops it
  from other features' excluded lists; one that starts adds a body.
- `RemoveFeature` and `RemoveBody` apply `Document::removal(Removable)`:
  the feature (a body's maker, for a body) and every later feature using
  one removed (`FeatureKind::uses`, a list: an extrude's sketch), in timeline
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
(`BodySolid`, in the order they were made), the bodies joins merged into
others (`merged`, below) and the features that failed, with why
(`failed`, in the document's order). A failing feature changes no body,
and the later ones still run.

- A **sketch** gives its `Profiles` (`Sketch::profiles`; too complex fails
  the extrudes using it).
- An **extrude** resolves its regions (`Profiles::resolve`; one gone is
  "region not found"), merges them (`Profiles::merge`), turns the loops
  into a kernel `Profile` (below), and calls `kernel::extrude` on the
  sketch plane's `Frame` (an origin plane's placement, or a face
  sketch's as regeneration placed it; one not placed fails it, see
  `agents/features.md`), over `Extrude::span()`,
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
  when it excludes some. A tool touching a body only along a line (a
  boss tangent to a round boss) touches it, so that body is a target
  and its boolean decides: a join fails naming the body, as the union
  of solids meeting along a line isn't a manifold ("joining it to Body
  1 leaves no clean solid…", "…can't be worked out…" or, where
  refining the line contact runs out, "…is too complex to work
  out…"); a tangent cut is the body unchanged where the kernel works it
  out (every tangent disc from the coarsest tolerance to `1e-4`, with
  the walls along one direction certified) and otherwise fails naming
  the body ("cutting it from Body 1 can't be worked out…", "…is too
  complex…"), changing nothing; a hole drilled tangent to a hole's wall
  from inside it is a no-op too, and one tangent to it from outside
  (the holes meeting along a line) "leaves no clean solid". `touches`
  answers a tangency within milliseconds (one counting and its search,
  see "Touches"), so a draft dragged with its tool tangent to a body
  asks it again for each tool and lists the body, and the draft is
  decided by its boolean, never failed by the touch test (it used to
  run out of the whole budget on every draft change and fail "finding
  where it meets Body 1"; `a_hole_drilled_tangent_to_a_hole_is_decided_by_its_cut`,
  `a_tangent_hole_dragged_reruns_its_touch_test_which_holds`). So a tool that meets one body and grazes another
  along a line fails as a whole (it used to skip the grazed body) until
  that body is taken out. Where a feature has more than one target, a
  target's failing boolean (or an intersect emptying it) ends its
  message with the way past: "…; untick Body 1 under Bodies to leave it
  out" (`message::leave_out`); a cut emptying a body says so already,
  and with one target unticking leaves nothing to work on. The
  excluded bodies' touch and boolean results are marked used
  (`Cache::keep`), so the budget evicts them last, for putting them
  back. A cut or intersect replaces each target by
  `kernel::boolean(body, tool, op)` with `Difference` or
  `Intersection`, and a join touching one body replaces it by its
  `Union` with the tool: the body always first (so the body's face names
  win where flush faces of one plane merge). Every target's
  result is worked out before any body changes, so one failing changes
  none. Bodies not touched or excluded keep their solids.
- A **join touching two or more bodies merges them**: the first made
  (the *holder*) gets the union of them all and the tool, and the others
  are *consumed*: left out of `Evaluation::bodies` and listed in
  `Evaluation::merged` as (consumed, holder), sorted by the consumed
  body's id (the document's order). A later join consuming a holder
  moves the entries naming it on to the new holder (`note_merge`, public
  so the app replays merges by the same rule), so every entry names
  a body in `bodies` (a `debug_assert` at the end of `evaluate` holds
  it); `Evaluation::holder(body)` follows one. The bodies stay the
  document's (Objects lists them); only their geometry moved. Objects
  shows a consumed body faint with its holder as its note ("in Body 1",
  `panels::consumed_note`, from `MeshFeed::merged_bodies`: the model
  shown's, with a draft as the draft merges them, none across a
  replacement) and no eye, since it's drawn as its holder is and the
  holder's flag decides; its bin removes it and its maker as for any
  body. An Objects row's buttons that show on hover keep their room while
  hidden, as the mock's do, so the note stays put: the hovered row is
  drawn over the plain one, which shows through the dark theme's
  highlight. Cuts and
  intersects stay per body, as other CAD systems keep bodies apart for
  those, and an excluded body isn't merged (unticking it is how to join
  to fewer bodies). A combine consumes its tools into its target by
  the same rule unless it keeps them (`agents/features.md`, "Combine").
  The union is worked out in steps, each one
  `kernel::boolean(running, next, Union)` cached under
  `boolean_key(op, running key, next key)`, all or nothing (nothing is
  written back until the last step works):
  - **The bodies first, then the tool**: `t0 ∪ t1` (key name
    `Doing::Merging`), then `∪ t2`…, then `∪ tool` (`Doing::Joining`).
    That passes the bodies' own faces first, which keeps the patches
    few: with the example plate and a plate 10 mm clear of it bridged by
    a boss flush with their bottoms, the tool joined to the first plate
    first left the boss's round bottom flush with the second plate in the
    next union, which came out with 57,430 patches (0.8 s); bodies first
    gives 120. The bodies' union doesn't depend on the tool, so
    a join draft dragged reworks one boolean, not one per target.
  - **Else the tool first**: if a step fails (bodies meeting each other
    only along an edge or at a point leave no clean solid on their own,
    though the tool bridges them: `Invalid(Fold)` for two blocks sharing
    a vertical edge), `t0 ∪ tool` (the key a single target would use),
    then `∪ t1`, `∪ t2`… (`Doing::Merging`). If that fails too, its
    error is given: its first step as a join's with more than one
    target ("joining it to Body 1 …; untick Body 1 under Bodies to leave
    it out"), a later one as `message::merging` ("merging Body 2 into Body 1
    leaves no clean solid: …", the tails shared with `message::boolean`,
    then "; or untick Body 2 under Bodies to keep it apart"). Either way
    the message has one such hint.
    The failed bodies-first steps stay cached, so the fallback reruns
    only on the tool's changes.

  Merging fails more often than joining each body on its own did, and
  rightly: blocks that meet each other along an edge or at a corner the
  tool doesn't cover make a union that isn't a manifold, which no solid
  holds, though each block joined to the tool alone is fine. A seeded
  fuzz of 2–4 blocks on a 5 mm grid (flush, corner to corner, apart,
  overlapping) bridged by a bar (`history/tests/merging.rs`, 24 seeds;
  300 measured) gave every merge the exact union's volume, and failed
  exactly where that union isn't a manifold (43 of 300; joined one by
  one, none failed). With discs among them (450 measured) no volume was
  wrong either: merging failed in 70, joining one by one in 39 (24 of
  them both), and the merge failures looked at were such contacts (a
  disc tangent to a block's side among them); the slowest is the one
  timed under the gaps below.

  With one target the path and keys are as they were before merging
  existed. `touched` still lists every target (consumed ones too).
  Through all spans the consumed material, which is inside the holder.
  Excluded bodies keep, besides their touch and boolean, the keys of both
  orders' steps for the targets plus every excluded body (in made order,
  hashes only), so unticking one body of a merge and ticking it again
  works out at most the merged body's mesh (nothing within the budget).
  A later feature taking out a consumed body takes nothing out (it has
  no solid; its material is in the holder, which is worked on), as when
  it was taken out before a join upstream reached it; the panel lists it
  "in Body 1" so that shows (see the extrude UI). A sketch placed on a consumed
  body's face (once sketches can be placed on faces) is to follow it to
  its holder (`Evaluation::holder`), since the face lives on there.
- A target whose result is the empty
  solid (a body cut away whole, or intersected with a tool that touches
  it without overlapping it: flush on a face, an edge on a face, a
  corner; the kernel gives all of these as `Ok(Solid::empty())`)
  **fails the feature**, so no body changes, with "cutting it from Body 1
  would leave nothing of it: untick it under Bodies to keep it as it is,
  or delete the body" or "intersecting it with Body 1 would leave
  nothing of it: they touch but don't overlap; flip it or move it to
  overlap" (`message::emptied`; flipping because the likely slip is a
  tool drawn on a face and extruded away from the body). Bodies are the
  document's, so an emptied one would stay listed in Objects with no
  geometry, and an edit upstream (the plate made smaller) could empty it
  with no feature turning red, while later features blamed the wrong one
  ("doesn't touch any body") or went through all of less. The body is in
  `touched`, so the panel offers to untick it; deliberately removing a
  body with a cut takes an untick and a body delete. The check follows
  the cache, so a result found there fails as well (a draft dragged
  across the point where the body goes, an undo bringing back the edit
  that emptied it); the empty result stays cached (its key is right; the
  check is cheap). Hence **no body in
  an `Evaluation` is empty**: a new body's extrude never is, a union of
  two non-empty solids isn't (a merged join checks anyway), and the rest
  fail (a `debug_assert` at the end of `evaluate` holds it).

**Error texts** (`src/message.rs`). What the Timeline's tooltip and the
panel show is worded for the user, not the kernel: an extrude's own
`KernelError` becomes "its regions are too complex to extrude: try
fewer or simpler curves" for `TooComplex`, "its regions have parts too
thin or too close together for this tolerance: try a finer tolerance"
for `Invalid`, and the profile errors say what's wrong with the outline
(touching or crossing itself, loops that don't nest, a cusp, a loop of
no area; for `TooFine`, "its outline has detail too small for this
tolerance: try a finer tolerance"). At the finest tolerance there is
none finer to try, so those two say "…to extrude, even at the finest
tolerance" instead (a strip 1e-7 wide is 10 resolutions there and fails
`Invalid`). A message names the tolerance only where a finer one is the
remedy, or to say there is none: `TooComplex` is the budget or a limit,
which a coarser tolerance doesn't change, so none suggests a coarser
one; `Triangulation` ("its end faces couldn't be made") and regen's own
`ProfileError::Fit` ("a spline couldn't be fitted: it stops or turns
back on itself") name none, as no finer one is known to mend them. A
boolean's error names the body and what was being done
("joining it to Body 2 leaves no clean solid: the result would touch
itself along an edge or at a point, or come too close to itself; move
it to overlap more or to clear it" for `NotManifold`, said outright;
"… leaves no clean solid: parts would be too thin or too close
together, as where faces are tangent; if so, move it to overlap more or
to clear it" for the other `Invalid` failures, zero-angle corners where
faces are tangent and thin triangles beside a hole's rim among them, so
the cause is hedged and no tolerance is offered, a finer one mending
neither;
"… can't be worked out: they meet on faces too nearly flush or tangent
to tell apart; move it a little" for `Inconsistent`; "… is too complex
to work out…" for `TooComplex`; for a result that's empty, the
`emptied` texts above). Every message starts in lower case,
the Timeline putting it after the feature's name. Body names are looked up when the message is
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
  share it). It is always fitted in the spline's own direction, its ends
  put at the vertices at its parameter ends; a piece running backwards
  then reverses the chain and each conic (`Conic2::reversed`, exact), so
  the two regions beside a piece get the same conics to the bit and
  their walls coincide. The Béziers are fitted **in runs**: from each one
  on, the run `i..=j` grows while one conic fits it and stops at the
  first `j` that doesn't (no scanning past a failure), and the longest
  run that fits gives one conic. A run fits:
  - **as a line** first, if every control point of every Bézier in it is
    within a quarter of the fit tolerance of the chord and projects
    between its ends (rigorous, by the convex hull), and the run's end
    tangents are within 45° of the chord. Without that, a sliver
    narrower than the tolerance (a closed spline 0.01 across at a fit of
    0.1) came out as two lines there and back, or a needle triangle:
    within the tolerance, but no area or a cusp, which fitting span by
    span never gave. With it, a line turns by under 45° where it meets a
    curved conic (which leaves along the spline's tangent) and under
    90° where it meets another line;
  - else **as a conic** along the run's end tangents (from its first and
    last Bézier; they must meet ahead of both ends and turn by under
    90°), its weight `k/(1−k)` putting the conic's shoulder as far from
    the chord as the furthest sample (`k` that height over the control
    point's; a conic is furthest from its chord at its shoulder), or else
    weight 1, within `0.25..=4`, accepted when 15 samples of each Bézier
    and each Bézier's end lie in the control triangle within half the fit
    tolerance of the conic (by its implicit form over its gradient,
    `λ1² = 4w²·λ0·λ2`).

  A Bézier no run fits, not even alone, is fitted on its own: a line if
  its inner control points are within a quarter of the tolerance of its
  chord and its end tangents within 45° of it, else one conic as above
  with its shoulder where the cubic at ½ crosses the line from the
  chord's middle to the control point, else a line if straight within
  the tolerance whatever its tangents (a sliver's tip, where span by
  span gave the same line; halving there only makes conics too thin to
  tell apart), else halved (at most 24 times; past that `ProfileError::Fit`). Curved
  conics meet along the spline's tangent; a line meets its neighbours
  with a kink of about the tolerance over its length where the spline's
  segments there are long (invisible at the tolerance; the profile test
  of a smooth spline bounds it by twice that), and under 45° (under 90°
  against another line) in any case, but for the last-resort line. A piece
  of `n` Béziers costs at most about `16·n²` point evaluations (`n` is
  about `MAX_SPLINE_POINTS` = 100 at most): the worst corpus spline fits
  in 1.2 ms, and a hostile sketch of 299 such splines (as many as
  `MAX_POINTS` allows), each a 60° arc one conic fits whole, the worst
  case for runs, profiles in 0.7 s at most (release). At most `MAX_PROFILE_SEGMENTS` segments are made
  (`TooManySegments`), counted as the piece is fitted. Measured on a
  closed spline through five points about 10 across: 5, 29 and 78
  segments at fits of 0.1, 1e-3 and 1e-5 mm (no run takes more than one
  of its five spans, so the counts are as span by span).

  **Measured** before and after runs (a scratch corpus on the real
  crates, each sketch extruded 10 mm on the XY frame at fits 1e-5 to 1e-1
  mm; refused counts `before → after` at 1e-3, 1e-2, 1e-1; segment totals
  at 1e-1). "Wavy": a plate W × W/2 whose top is an open spline through
  `n` ∈ {10, 30, 100} evenly spaced fit points jittered by ±slope·spacing/2,
  W ∈ {1, 5, 20, 100}, slope ∈ {0.1, 0.3, 1}, 10 seeds (360). "Control":
  the same top by control points (60). "Blob": closed splines through 5
  to 100 points on a perturbed circle of radius 0.2 to 100 (420). "Cut":
  a blob cut by a line, each region (48). "Hole": a blob as a square's
  hole, each region (48).

  | set | refused at 1e-3 | at 1e-2 | at 1e-1 | segments at 1e-1 |
  |---|---|---|---|---|
  | wavy | 5 → 5 | 17 → 15 | 52 → 5 | 21 772 → 12 510 |
  | control | 0 → 0 | 0 → 0 | 9 → 0 | 2 972 → 1 092 |
  | blob | 0 → 0 | 0 → 0 | 0 → 0 | 16 602 → 4 339 |
  | cut | 0 → 0 | 0 → 0 | 0 → 0 | 1 140 → 344 |
  | hole | 0 → 0 | 0 → 0 | 0 → 0 | 2 180 → 590 |

  Nothing failed at 1e-5 or 1e-4 before or after. The worst sampled fit
  error stays 0.68 of the fit at 1e-3 and coarser; the worst fit time
  fell from 2.8 to 1.2 ms. Every refusal left is the cap residue at real
  detail near the tolerance (slope 1, see "Known gaps" under Profiles
  and extrude), and two sketches that extruded before now fail with it
  (wavy n = 30, W = 1, slope 1, seed 0 at 1e-2, `TooComplex`; n = 100,
  W = 20, slope 1, seed 5 at 1e-2, `Invalid(VertexNeighbours)`): their
  chains changed by a few segments (53 → 50, 228 → 226) and the caps'
  fragility flipped, as it flips the other way for others.

**Cache** (`src/cache.rs`). Every result is filed under a 128-bit key (two
SipHash runs, one salted, over the length-prefixed parts): a sketch's
profiles by the sketch alone (postcard-encoded; not its plane), whether
it solves by the sketch, a face sketch's placement by the face's solid's
key and the face reference, an extrude's solid (or error) by its feature
id, the regions, the tolerance's bits, its span's bits, its sketch's key
and its placement's bits (not the operation, the extent or the excluded bodies, so toggling those
finds the tool; the span stands for the extent and flip), whether a body touches a tool by the two solids' keys, a
boolean's result (or `KernelError`) by the operation and the two solids'
keys, which then keys the body's solid (a merge step's likewise, by the
running solid's key and the next operand's), and a body's mesh by its solid's
key and the tolerance. Editing an earlier extrude changes its body's key
and so reruns every boolean after it on that body. The cache is bounded
by size: each entry records its approximate heap size (`Entry::bytes`: a
solid's four mesh arrays, a render mesh's four, profiles' regions, pieces,
outlines, vertices and open ends, an error string's length, plus 128 B
for the map slot and the `Arc`; saturating sums, since the sizes come
from what the user drew; lengths, not capacities, so a vector grown by
pushes, such as a scene of several bodies joined, may really take up to
about twice its count), the request that last used it and a use
counter. `Cache::begin` starts a request and, only while the total is
over the budget (`BUDGET`: 256 MiB natively, 64 MiB on wasm32, per lane
and so per open document, since a worker's memory never shrinks) or
there are more than `MAX_ENTRIES` (2^16, so tiny entries can't pile up),
evicts what the request before didn't use, least recently used first by
the use counter (a total order: eviction never depends on the map's
order, so counts are the same on every run). What the request before
used is never evicted, nor is the scene of the last answer without a
draft, so an unrelated edit, or a draft dragged, reruns
only what changed whatever the budget (that set may exceed the budget on
its own); within the budget, undo, redo and an option changed and
changed back (join, cut, join; a distance typed and typed back) find
what they had. One request on the example with a pocket holds about
90 kB, on a 100 mm plate with 36 holes about 1.6 MB. A join, cut or
intersect also marks used (`Cache::keep`) whether the tool touches each
body it excludes and their boolean, and a join the steps of its merge
with the excluded bodies put back, so taking a body out and putting it
back only draws it again even under a small budget.
`Cache::with_budget(0)` (tests) keeps only what the request before used,
which was the policy before the budget. The lane owns it: the native
thread's closure, or the worker's `serve`.

**Drafts.** `Request::Regenerate` has `draft: Option<Draft { revision,
feature, kind }>`: a feature of any kind (an extrude, a revolve) being set
up (`feature: None`, applied as `AddFeature`, the body `BodyId::NEW`) or
edited (`SetFeature`, which may change its kind), applied to
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

**The answer.** `Response::Regenerated` adds `failed`, `merged` (the
evaluation's, (consumed, holder) pairs) and `bodies:
Vec<(BodyId, Aabb)>` (each body with a solid, shown or not, from
`Solid::bounds`; a consumed body has none). `tessellate(document, evaluation, cache)` draws the
visible bodies' solids at `Display::new(&document.tolerance())`, joined
by `RenderMesh::append` into an `Arc<RenderMesh>`, a part per body in
the order they were made; a mesh past `RenderMesh`'s limits fails the
generation with the `MeshError` (and isn't kept). Each body is drawn
with its picking tables (`Drawn`, made by `Solid::tessellate_with` with
the body's `Topology`: its faces' keys, aliases and summaries and its
edges' closed flags; see "The document mesh" in `agents/viewport.md`),
and the answer's `picking` is the shown bodies' joined, with the body of
each part. The joined mesh and tables are kept in the cache under a
scene key (`"scene"`, then each shown body's id and mesh key in order,
which holds the tolerance, then the count), as one more kind of entry under the same budget, except that
the scene the last answer without a draft used (a failing draft's answer
is one) is never evicted either, so the committed model's scene survives
any number of draft revisions, even when a revision's scene is an older
one still held. Scenes aren't in `counts` (those count features;
`Cache::joins` counts joins, for tests). A request whose scene didn't change (a sketch edit no body
depends on, a sketch hidden or left out, a failing draft, the committed
model after a draft was dragged and put away, the model just after a
draft is committed) gets the same `Arc`, so natively the renderer, keyed
by the `Arc`, skips the upload; a scene found keeps its bodies' meshes in
the cache (`Cache::keep`) for the next scene that changes one. The
renderer doesn't try an upload of the same `Arc` again after it failed;
the only failure is a part past the device's buffer limit, which the same
mesh would hit again, so it is logged once. On the web the mesh still crosses the wire whole each time.
On the web the reply's head
carries `draft`, `failed`, `touched`, `merged` and the boxes as corner
arrays, checked finite and in order on receipt (`wire::Error::Bounds`);
`MAX_HEAD_BYTES` is 64 MiB (the head carries the picking tables too:
the parts' bodies, at most `RenderMesh::MAX_PARTS`, the faces, at most
`MAX_FACES` 2²⁰ with `Picking::MAX_ALIASES` 2²⁰ aliases among them, and
the edges' closed flags and tangent chains, at most
`RenderMesh::MAX_EDGE_POLYLINES` each, all refused as soon as they're
past their bound, since a face is some 120 bytes
on the page and as few as 5 in the head; a model whose head would be
larger or whose faces are past those is answered as failed). The draft's and each feature's touched bodies
cross in the head as marks, unchecked; `merged` is checked to name each
consumed body once and none as a holder (`wire::Error::Merged`), and is
otherwise display only. Either failing answers the generation with
`Response::Failed`.

**Gaps.** Every join, cut or intersect asks `touches` of every body
before it on each edit that changes the tool (cached otherwise; bodies
whose boxes are apart are answered at once).
A boss tangent to a body only along a line touches it now, so the
join reaches the union, which refuses a line contact or runs out
refining it (the union's own gap); before, `touches` called it "it
doesn't touch any body". Measured, release, a loaded machine, a disc
of radius 1 cut or joined 1 mm tall beside a round body of radius 1
along a line at three angles: `touches` 0.8 to 4.3 ms at every
tolerance from `1e-1` to `1e-4`; the whole regen 0.2 to 0.45 s at
`1e-1` (refused or a no-op), 1.1 to 2.4 s at `1e-2` (a no-op, or
refused for a face too thin), 1.4 to 4.1 s at the default and 4.9 to
7.6 s at `1e-4`, there the boolean running out (`TooComplex`). So the
time moved from `touches` (3.6 to 11 s to `false`, `TooComplex` or
`Inconsistent`) to the boolean, whose answer is cached. With walls
along one direction certified near each other, every such cut is a
no-op, in 20 to 320 ms at every tolerance, and the joins are refused
in 15 to 170 ms at `1e-1` to `1e-2` off the seam, and at the default
off it in 0.1 to 2 s (most of it repairing the refused union); a
join with the line on the seam (`Inconsistent` at `1e-1` and `1e-2`,
`TooComplex` from the default) and every join at `1e-4` (1.5 to 2.8
s, `TooComplex`) are as before: the pairs along the line have ends
there, and joining them line by line leaves those of a union at a tie
alone (measured since, on a loaded machine: the seam joins "can't be
worked out" in 0.2 to 0.55 s at `1e-1` and `1e-2`, run out in 2.4 s at
`1e-3`, and every join at `1e-4` runs out in 3.3 to 4.3 s, the cuts
no-ops in 7 to 490 ms). Since the decisions name a union touching
along a line (see "Results that aren't manifolds" in the booleans),
every one of these joins is refused as "the result would touch
itself" in 20 to 830 ms, at every fit and angle. Holes drilled tangent to the
example plate's hole (radius 8; a disc of radius 3 through it, at
angles 0, 0.3 and 0.7, release): from inside, no-ops in 0.06 to 2 s;
from outside, refused as "no clean solid" in 0.3 to 0.6 s off the
seam, and with the line on the seam (angle 0) out of budget in 4 s, a
difference at a tie on a seam that refinement doesn't settle. In each
the touch test is milliseconds, so the time per draft change is the
boolean's, cached per tool. An operation
that runs out of budget takes about 2–3.5 s on one native thread and holds the
single-threaded web worker longer, with drafts queued behind it (latest
wins, so only the newest waits). The cache's sizes are estimates
(shared `Arc`s count once per entry holding them), and eviction is plain
least recently used, not weighted by what an entry took to work out. On
the web an unchanged model
mesh is still copied over the wire, checked and uploaded again with each
answer. Merged bodies grow, so later booleans on them cost more (the
budget scales with the whole body), and a later cut that worked on the
bodies apart can fail on the merged one. The tool-first fallback can
meet the flush blow-up the bodies-first order avoids (57,430 patches in
the case above), and a bodies-first step that runs out of budget before
the fallback runs about doubles the worst case, once (both are cached).
Measured: four bodies, two of them overlapping cylinders, all flush on
top with a disc tool (cylinder caps flush, the boolean's own blow-up):
bodies first ran 0.9 s to 26,864 patches, then the tool's step 3.6 s to
`TooComplex`; the tool first then 4.6 s (72,296 patches), 8.3 s in all
(release, an idle machine) against 3.1 s joining each on its own. A
join draft dragged there pays the last bodies-first step and the whole
fallback on each step.
The merge's keys kept for excluded bodies cover putting back all of
them at once or the only one: with two taken out, putting back one
reworks its merge. Merged plates flush on each other keep the line
where they met on their sides: the union doesn't merge the two plates'
coplanar side faces.

## The extrude UI (`crates/view`, `crates/app`)

**The session** (`app/src/doc/extrude.rs`, `Doc::extrude`, an
`ExtrudeSession`) is started by the Extrude tool (`X`, `Look::StartExtrude`,
outside sketches, where `X` turns geometry into construction; with no
sketch yet too, the panel waiting for one; a revolve or combine set up
is dropped for it; again, or `Esc`, cancels it) or by
editing an extrude (`Look::EditFeature` on one: a double-click in the
Timeline, or `Enter` with it selected). It never runs with a sketch
session, nor in a read-only document. It holds: the extrude edited, if
any; the source sketch (the one selected in the Timeline, the edited
extrude's, or else the one the first region picked is in, which un-picking
every region lets go of again); the profiles of the source, or before
there is one of every visible sketch with regions (each found once, and
again when its sketch changes; those with none to pick, too complex
within the whole of `MAX_WORK` or empty, are kept too, by value, so a
change elsewhere doesn't work them out again; the visible sketches worked
out in one refresh share `REFRESH_WORK`, twice `MAX_WORK`, through
`Sketch::profiles_spending`, and those past it have none to pick and
aren't worked out at all, not even their splines' shapes, so a file of
many hostile sketches costs the UI thread a bounded time; skipped so, or
too complex for the less than `MAX_WORK` that was left, they aren't kept
but tried again on the next change, when those found are kept and spend
nothing, so each sketch gets its whole `MAX_WORK` within a few changes);
the regions picked, as indices and as
`Profiles::reference`s made as they're picked (a region too thin for a
reference can't be picked); the extent kind, the two distance fields
(text, last good `Value`, error, read with `Extent::ask`), flip, the
operation and the excluded bodies (kept from the edited extrude). When
the source sketch changes under it (undo), the picked regions are found
again by their references (`Profiles::resolve`); while its regions can't
be found none are picked, and the references wait for them. An edited extrude's
references that aren't found are counted (`missing`, shown in the panel)
and dropped: `SetFeature` gets fresh references of what's picked. Editing
never changes the extrude's sketch, so `SetFeature` doesn't hide one.
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
applies `AddFeature` (the document's name "Extrude N", adding the body
and hiding the sketch) or `SetFeature`, one undo step, selects the new
extrude and ends the session; refused by the document (left to the
cross-references, which the session keeps valid, so no test reaches
it), the session stays and the edit error shows. `Esc` or Cancel drops
the session and its draft whatever its state, and the model is asked
for again without it.

**The panel** (`view/src/extrude.rs`) floats at the viewport's right in
the operation panel (`view/src/operation_panel.rs`, meant to hold every
operation's panel, with the parts the extrude and the revolve share:
`Candidate`, `TypedField`, `OperationKind`, `BodyTarget`, the choices,
ticks, typed fields, Bodies list and footer message; the app's session
parts they share are in `app/src/doc/regions.rs`, see "UI" under
Revolve in `agents/features.md`): a fixed header (the title on one line, clipped, and
the region count), a body that scrolls (`PANEL_BODY`) when the panel would
run past the viewport's bottom margin, and a fixed footer (the message and
Cancel and OK), so OK and Cancel show and take clicks however many bodies
are listed or however short the window; nothing is drawn past the panel,
and a body's name, the field errors and the message break inside words
that don't fit (`Wrapping::WordOrGlyph`), the message scrolling on its
own past about five lines. Its body holds the extents (Through all only while Cut is
chosen, else disabled with "Only a cut goes through all"; choosing
another operation while through all goes back to one side), the distance fields (the first is `VALUE_FIELD`, which
takes the focus as the session opens, all selected; `Esc` in it cancels),
Flip for one side and two sides, the operations, for Join, Cut and
Intersect a "Bodies" list with a checkbox per body (`BodyTarget`: the
draft's touched bodies as the newest answer of the current run of drafts
that ran the touch test gave them, `MeshFeed::draft_touched`: kept while a
changed draft is on its way, and while one fails before its tool exists
or makes a new body; a run starts when a draft is asked for after none,
or for another feature, and lists nothing until its first such answer,
so a new session never shows another's bodies; then the excluded ones,
and those ticked again (the session's `reticked`, each with the newest
draft revision given out when it was, `MeshFeed::revision`) until a
touch test of a later draft answers (`MeshFeed::draft_touched_revision`),
so a body taken out and put back doesn't drop out of the list while its
answer is on its way; a body that a join before the extrude merged
into another is never touched, so it's listed only while taken out or
just ticked again, with "in Body 1" after it (`BodyTarget::holder`,
faint): taking it out does nothing then (its material is in its
holder), and the row shows that and lets it be put back, after which
it drops out once the touch test answers. Which bodies are merged there
is `MeshFeed::merged_before`, replaying the joins before it that the
model shown has working and touching two or more, from
`touched_features` and `failed_features`, as `feed::Merges` by regen's
`note_merge`, so the two can't disagree on a prefix; the final `merged`
won't do, as a join after the extrude may consume a body it rightly
lists; all in the order they were made; ticked unless excluded;
`ExtrudeLook::Target` toggles, keeping the session's `excluded` sorted and
only taking bodies made before the extrude edited; bodies undone away
drop out, and aren't taken out again when redone: undo gives the ids
back, so a new edit may give theirs to other bodies), and under it, for a
join ticked for two or more (not counting one merged away before),
"Joined into Body 1", the first ticked (`extrude::joined_into`), which holds them all once it's committed; its footer the
refusal, the draft's error or "Checking the sketch…", then Cancel and OK.
While the draft's error shows, OK and `Enter` wait, and an "Accept error"
button left of OK, in the danger fill, commits anyway (`Edit::AcceptError`,
`Parts::accept`, `Doc::commit_by`; every operation's panel, never a key):
the feature is kept with its error, marked failed in the Timeline, to fix
later. The error is the newest draft's, none while it's unanswered, so OK
doesn't wait on the preview.
Errors that stand alone, the field errors, the refusal and the draft's
error here and a failed feature's tooltip, are shown as sentences,
capitalised by the view (`chrome::sentence`): the messages themselves
(`varde_expr`'s, `ExtrudeError`'s, `regen::message`'s) start in lower
case to follow a colon, as in the status bar's "Couldn't regenerate: …".
The Bodies list has no scrollable of its own: the body scrolls as a
whole, keeping its offset while rows come and go (clamped to what's left,
so no empty stretch shows) and while the handle's knobs come and go (the
knobs' layer stays, empty, so the panel's keeps its place in the
viewport's stack and its state), and the wheel over it scrolls it rather
than the camera. Giving the first field the focus (a session started, or
another extrude edited while a panel is open) also scrolls the body back
to its top, where the field is. The panel starts 150 px down
(`PANEL_TOP`, clear of the camera controls) unless the viewport leaves
it less than 200 px (`PANEL_ROOM`) below that; then it rises, at most to
12 px from the top, over the controls (`operation_panel::placed`). Where
even the header and footer don't fit, the footer keeps its height and
the header gives way. Below about 550 px of window width (the
side panel's 256, the panel's 264 and its margins) the panel narrows
with the viewport and its text wraps; in a much narrower window its
buttons squeeze away, as the window has no minimum size. The handle and region picking are in `agents/viewport.md`.
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
before. Neither is given out from a model of before a replacement of
the whole document (`MeshFeed::replaced`), whose ids may name other
features. Double-clicking an extrude opens its session. Right-clicking
a row selects its feature and opens its context menu (`Look::OpenMenu`
with `RowMenu::Feature`, `Doc::row_menu`; the widget is
`view/src/context_menu.rs`, an overlay at the click, the scrolled list's
offset allowed for): Edit sketch or Edit extrude, and Delete if the
document is editable. The rows of Objects have one too, selecting
nothing (`RowMenu::Body`, `RowMenu::Sketch`): Edit sketch for a sketch,
Hide or Show where the row has an eye, a body's Opacity (not a merged
one's, drawn as its holder is), and Delete. Opacity is a slider from 10
to 100 % in steps of 5 with the percentage beside it, wrapped in
`MouseOnly` (`view/src/mouse_only.rs`) so it's only dragged: iced's
slider also steps on arrow keys and `Ctrl`-wheel, with no release to
commit after. Dragging sends `Look::PreviewOpacity` (the value through
`Opacity::clamped`), kept in `Doc::opacity_preview` while the body's
menu is open and the document editable, and drawn and shown in the row
in place of the body's own (`DocumentState::opacity_preview`,
`shown_opacity`) without touching the document. Until it's let go the
shortcuts are off (`Doc::keys` is `None`) and the peek key doesn't swap
the tab (`Doc::peeks`), which would take the slider and its release with
it. Letting go sends `Edit::CommitOpacity`, one `Command::SetOpacity`
(none if unchanged), and leaves the menu open. The preview goes whenever
the menu does (`Doc::prune_preview`), so `Esc` mid-drag drops it. `Esc` or a
press off a menu closes it alone; any other message closes it too, and
it goes with what its row lists. Outside a
sketch or a session the floating status bar (`view/src/status.rs`, as
the mock's; see `agents/viewport.md`) shows the feature selected in a
box of its own, its icon, name and `feature_info` (`view/src/document.rs`:
"Distance 10 mm · New body", "Symmetric 4 mm · Cut", a sketch's
"4 lines · 1 circle · 5 points · on XY"), and nothing of the model with
none; then regenerating, failures and saving, cut short before the
hints, so a long failure never pushes them off the screen. The bodies
are counted as the joins leave them, a merged one with its holder, in
the Objects group's count (`panels::bodies_after_joins`).

**Deleting** (`app/src/doc/delete.rs`): `Edit::RemoveFeature` (`Delete`
on the Timeline's selection) and `Edit::RemoveBody` (Objects' bin) ask
`Document::removal` what goes. Joins, cuts and intersects don't depend on
the bodies they touch (they're found again when regenerating), so a
body's removal takes only its maker (and what uses that); a later join
left touching nothing fails in the Timeline. So the prompt warns of
them: every join, cut or intersect that stays and touched only bodies
that go (one that also touched a body that stays goes on working on
that), as the model shown found (`Response::Regenerated::touched`, the
history's `Evaluation::touched`, kept by `MeshFeed::touched_features`
like the failed features, and not given out across a replacement until
a newer model; one the model shown doesn't know, such as a cut committed
before its model comes back, is taken to touch every body made before
it that it doesn't take out, so it's warned of when they all go, as it
then surely has nothing to work on; a body that holds bodies joins
before the feature merged into it counts as those too, replayed as for
the Bodies list, since one of them staying takes its place: deleting
Body 1 that Body 2 was merged into leaves a later cut of Body 1 working
on Body 2), is listed under the
prompt in the warning colour (the mock's for panel warnings, its construction orange): "Extrude 2 works on Body 1 and stays, so
it may fail with nothing to work on." (`Doc::worked`, `delete_warning`).
One that took the body out isn't (excluded bodies aren't asked
about, so it touched none). A removal with
such a feature asks even if only its own feature goes, so a body's
prompt shows. For a body the prompt asks
"Delete *Body N* with the M features and K bodies that go with it?",
counting its maker and the other bodies that makes, or, when only its
maker goes with it, "Delete *Body N* and *Extrude M*, which makes it?". If that's one
feature (a feature and its own bodies, or a body and the feature making
it) and nothing's warned of, the command applies at once. Otherwise the app keeps a `Deleting` (the target, the `Removal`,
the editor's generation) and the view shows `DeletePrompt` over the
whole screen, on the unsaved-changes prompt's scrim: "Delete *name* with
the N features and K bodies that depend on it?" (`delete_question`,
leaving out a count of none), the features in timeline order with
their icons, then the bodies, scrolling past about ten rows, Cancel
(`Look::CancelDelete`, also `Esc`: `Doc::dialog` tells the escape key
which prompt is up, the unsaved one first) and Delete
(`Edit::ConfirmDelete`, danger style, no `Enter`). While it's up
`Doc::keys` is `None`, so no shortcut acts behind it, and the status
bar's hints are only "Esc Cancel". Delete applies the
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

## Limits, budgets and errors (`src/lib.rs`, `src/budget.rs`, `src/error.rs`, `src/failure.rs`)

| constant | value | why |
|---|---|---|
| `MAX_PATCHES` | `1 << 22` | patches in a mesh; ids and counts fit a `u32` |
| `MAX_REFINE_DEPTH` | 24 | red splits from an input patch: `2^24` times smaller |
| `MAX_TRACE_STEPS` | 4096 | steps tracing one cut of a boolean; past them the cut falls back to a simpler curve |
| `SPLIT_ROUNDS` (boolean) | 6 | rounds of halving curves while cutting faces; what the last keeps must be within the fit tolerance |
| `MAX_NEAR_NODES` (boolean) | 512 | pieces of a patch looked at certifying a crossing only placed, or checking a root of an edge is on it |
| `MAX_TURN_COS` (boolean) | 0.7 | the most a cut's conic turns (about 45°) |
| `MEND_ROUNDS` (boolean) | 4 | rounds of Steiner points in one face's triangulation |
| `SIN_SHAPE` (`mesh/shape.rs`) | sin 5° | the smallest angle under which an extrude cap's triangle is refined (see "Cap quality"), and, in a curved patch's layout, under which a cut face's triangle takes a point at its circumcentre; at most 4 per loop vertex and 16 more, none in a triangle whose circumradius is under `MIN_CURVED_SPLIT` resolutions |
| `MAX_WORK` | `1 << 22` | work units in one operation: about two seconds on one thread at most; the heaviest booleans measured take about half of it |
| `MIN_SPLIT` (repair) | 64 resolutions | the smallest flat piece repair splits, and the smallest profile segment an extrude halves |
| `MIN_CURVED_SPLIT` (repair) | 8 resolutions | the smallest curved piece repair splits; the refiner's floor in repair |
| `FLAT_STOP` (repair) | 1/16 | how flat, in resolutions, failing non-neighbour pieces must both be for repair to stop splitting them |
| `MAX_PROFILE_SEGMENTS` | `1 << 16` | segments in a profile |
| `SIN_MIN` (extrude) | `1e-3` | cusps between segments; the narrowest cap patch corner |
| `MAX_SPLIT_DEPTH` (extrude) | 24 | how often a profile segment may be halved |
| `MAX_ROUNDS`, `MAX_CAP_DEPTH` (caps) | 32, 16 | rounds of mending the caps, and the halvings all told past which the caps halve a segment no more |
| `MAX_MEND_DEPTH`, `MAX_QUALITY_ROUNDS` (caps) | 6, 64 | the halvings all told past which mending on the tries that refine leaves a corner to refinement, and the runs of refinement that ask for anything |
| `CROWDED`, `MIN_CROWDED` (caps) | 32, 65 536 | pairs of the caps' triangles' boxes within the resolution, per triangle and at least, past which the caps are refined for crowding |
| `MAX_EVIDENCE` | 4096 patches, 4096 curves, 256 points, 4096 sketch curves, 256 faces | items of each kind one failure's `Evidence` holds; past them the first ones and `truncated` |
| `EVIDENCE_WORK` | `1 << 16` | work units gathering one failure's evidence may take, apart from the operation's budget (about 30 ms on one thread) |

`Budget` is a limit (`Budget::new(work)`, at most `MAX_WORK`;
`Budget::DEFAULT`); an operation counts it down in a `Work` its steps share
(`repair_within` takes one), and running out is `TooComplex`. A unit is
about a patch or a pair of patches tested or split: repair measured about
0.5 µs a unit on one thread and 0.3 µs on seven. `KernelError` is
`TooComplex` (the budget, or a limit such as `MAX_PATCHES`,
`MAX_REFINE_DEPTH` or a cap of rounds: never detail too small for the
tolerance, which is `Invalid` or `ProfileError::TooFine`),
`Invalid(CheckError)` (the input breaks an invariant the operation
can't restore, or the result would, as for a solid too thin for its
resolution), `Patch(PatchError)` (a
parameter, or a split outside the patch bounds),
`Profile(ProfileError)` (a profile that can't be extruded), and
`Boolean(BooleanError)` (see "Booleans").

**Failures and evidence.** `KernelError` stays the small `Copy` enum
every internal step returns and matches on (extrude's and revolve's
retries, `pinched_named`, the boolean's flat retry). The public
operations that make or combine solids, `extrude`, `revolve`,
`boolean`, `touches`, `assemble` and `Solid::transformed`, return
`Result<_, Failure>`: `Failure { error: KernelError, evidence:
Box<Evidence> }` (boxed so the `Result` stays small: five vectors
would make every `Err` past clippy's `result_large_err`), with
`From<KernelError>` (empty evidence) so `?` converts,
and `Display` the error's. Each is a thin wrapper over a private
function returning `KernelError` (`extruded`, `revolved`, and
`boolean_within`, `touches_within`, `transformed_within`, which take the
`Work`); `assemble`
returns its unions' failures less their operand faces (`of_parts` in
`transform.rs`: those name the union's operands, not any of
`assemble`'s, so they're dropped and `truncated` set). Other public functions that
can fail (`Solid::new`, `cuboid`, `cylinder`, `Mesh::repair`,
`Solid::moments`) keep `KernelError`. `Evidence` is the geometry the
error is about, by value and in the operation's world coordinates
(extrude and revolve place the profile by their frame): `patches`
(`Patch`), `curves` (`Conic<DVec3>`), `points`, `sketch_curves` (the
`Segment::curve` ids of profile segments) and `faces` (`(Operand,
FaceKey)`, `Operand::A` or `B` for a boolean's or `touches`' first or
second operand; the boolean's own `Side` converts into it), plus
`truncated`. Only a step holding both the error and its geometry fills
it.

**Profile evidence** (`profile/evidence.rs`). Every `ProfileError` names
what it is about by indices into the profile as given (revolve's
`remap` maps the ends' pieces back; loops alone keep their index, the
ends' loops being the profile's), so `extrude` and `revolve` gather it
in their public wrappers from the error they return, after every
retry, and the profile: the evidence is the returned error's by
construction and can't change the outcome. A `Gather` holds the
profile, its frame (only if `Frame::check` passes: a profile is checked
before its frame, so without one only the sketch curves are given), the
evidence, the operation's resolution and its `EVIDENCE_WORK` allowance
(a unit a segment placed, `NEAREST_WORK` for a nearest pair, a unit per
4 segments scanned for the axis's extent). Segments are placed on the
frame at height 0 (`Frame::conic`), the sketch's own plane, where its
curves are mended (extrude's `from` and `to` don't move them; a
revolve's frame is the sketch's plane), as `Conic<DVec3>` passing
`Conic3::check` with every control point in range, else left out; each gives its `Segment::curve` once (a
`BTreeSet` of those given). By error:

| error | evidence |
|---|---|
| `Empty` | none |
| `TooManySegments`, `Triangulation` | every segment, loop by loop, up to the caps and the allowance |
| `Short(l)`, `Area(l)`, `Nesting(l)` | loop `l` |
| `Segment`, `Degenerate`, `TooFine` | the segment |
| `Open(l, s)` | the gap's two ends (`s`'s end, the next one's start) and both segments |
| `Cusp(l, s)` | the vertex where `s` starts, and the segments before and at it |
| `Touching([a, b])` | both segments, and where they come nearest: one point half way between where they are within the resolution (touching, as far as the operation can tell), else the point on each |
| `CrossesAxis(l, s)` | the segment, and the axis as a line from the least to the greatest `y` of the profile (each segment's at its ends and where its `y` turns, `measure::turns`: control points can reach past the curve) |
| `TouchesAxis(l, s)` | a vertex on the axis (`s`'s start within the resolution of it) as a cusp is given; else the point of `s` nearest the axis, and `s` |
| `NearlyFullTurn` | the whole profile on the frames at both ends of the turn (`x` turned to `from` and to `to`, as `Turn` turns it) |

Where two segments come nearest is `measure::distance`'s search between
two edges (`curves_distance`, the placed curves as two one-curve
elements, to the resolution), from its own `NEAREST_WORK` (4 096 units;
arcs crossing, touching or nested take about a hundred) taken out of
the allowance: past it, no point and `truncated`. The chain's
`separate`, which raises `Touching`, finds only two pieces whose hulls
aren't apart, not a point, and hands up indices alone, so the search is
run again on the whole segments. (Two in a row always meet at their shared end, so
for them that end is the point; `separate` raises `Touching` on such a
pair only when nothing else of theirs fails first, which in practice is
a sharp corner.) The axis's nearest point
(`nearest_axis`) is exact: the segment's ends and where its `x` turns
(`measure::turns`, the roots of the derivative's numerator), the least
`|x|` of those; `axis_rules` decides by signs and finds no place
either.

**Check evidence** (`failure/check.rs`). An `Invalid(CheckError)` of a
mesh an operation built names its triangles: `Failure::of_mesh` gives
them as patches of that mesh, from a fresh `EVIDENCE_WORK` allowance (a
unit a triangle looked at): one for `Fold`, `Face` and `FacesAgainst`;
two for `Hull`, `EdgeNeighbours`, `VertexNeighbours` and `SameCorners`
(one where both name one triangle); for `InsideOut` the shell whose
lowest triangle it names, walked outward from that triangle breadth
first through the halfedges' pairs (not numbered among all shells as
the check does, so the work stays within the allowance), up to
`MAX_EVIDENCE` and then `truncated`; for the structural errors the
triangle named (`Patch`) or the halfedge's (`Index`, `Pair`, `Loop`,
`DirectedEdge`, `SharedEdge`) if its indices are in range; none for
`Fan`, `EdgeUse`, `Alias`, `Counts` and `TooManyPatches`. Patches are
as the mesh holds them; the receiver draws those it can. Where repair
refused the mesh, the evidence is the pieces it names (`Refusal`,
`Failure::of_patches`; see "Repair"): within the input triangles the
error names, and smaller where repair split first, so a failure in a
corner of a long wall strip shows the corner, not the strip; repair's
errors naming no piece give the input's triangles. Where the check
refused the repaired and merged mesh, its triangles. `Unfinished`
holds the mesh that failed (`Solid::finished_or_unfinished`) and makes
the failure (`Unfinished::failure`); `Solid::new_repaired_within`
(extrude's, from repair on) and `Solid::finished` (revolve's) are that
function with and without the check's `CHECK_WORK`, and return the
`Failure`; the boolean takes the `Unfinished` itself, gathers the
evidence in `checked`, keeps the mesh for `pinched_named`, and keeps the
failure with the error through its retries: unfolding again (the first
try's error and evidence stand), the second try without line joins (the
first's again), and `pinched_named`, whose `NotManifold` keeps the
check's evidence. Extrude and revolve keep each try's `Failure` whole,
so the evidence returned is that of the first try, whose error they
return. `TooComplex` past the check, and `assemble`'s and the
transforms' own checks, carry none.

Its rules: evidence never changes an outcome (no `Ok` becomes an error
or the reverse, and the error is the one returned without it; tests
compare errors with the evidence stripped, through the test-only
`Stripped` trait); it is bounded, the adders (`Evidence::add_patches`,
`add_curves`, `add_points`, `add_sketch_curves`, `add_faces`) keeping the first items in
order up to `MAX_EVIDENCE` and setting `truncated` past it (the fields
are open, so a receiver checks `within_caps`); it is gathered from a
fresh `EVIDENCE_WORK` allowance (`failure::evidence_work`, spent through
`Evidence::afford`, which sets `truncated` once it runs out), never the
operation's budget, so finding it can't make an error `TooComplex`;
it is deterministic; and it goes with the error returned, so where an
operation retries and returns an earlier try's error, that try's
evidence is kept with it (`(KernelError, Evidence)` together, not a
side slot on `Work`). `TooComplex` carries none. Regen makes the
evidence drawable once and keeps that in its cache
(`ErrorGeometry`, with `Display::flatten` and `Display::sample_patch`;
see `agents/features.md`, "Failures and where they are").

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
- **Exact cone triangles with geometric-mean rulings** (this replaces the
  earlier "no exact cone triangles", which assumed a linear ruling: that
  one leaves a strip `8e-4` to `3e-3` off a unit-sized cone, and its induced
  parametrization does depend on the projection point). A ruling of
  weight 1 with its control point at `√(da·db)` from the apex is the one
  parametrization every projection point induces, so the patches either
  side share one record exactly (see "Swept strips"). A strip next to a
  hyperbolic paraboloid strip (whose rulings must be linear) can't share
  such a ruling exactly and will be fitted there.
- **Forms as built** differ from the plan's sketch: a cylinder is `{
  point, axis, radius }` (no line type), a revolved conic's meridian is a
  `Conic2` in `(ρ, h)`, only a plane's form carries which way the face
  faces, and the variants for forms not made yet (general quadrics,
  drafted, swept, lofted walls) come with the work that makes them. The
  debug check also checks a plane form's direction against the patches'
  normals.
- **`revolution_strip` takes the axis.** The diagonal's plane holds
  `a1'`, `a1` half a turn round; found on the bottom's conic alone it
  rounds as `ε/θ²` of the radius for small pieces, from the axis as the
  coordinates.
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
  (`Patch::degenerate_corner`, new), on a whole leaf that is an affine
  triangle failing the fold check (planned for any piece flat within the
  resolution, which splitting can mend), on a failing pair of flat pieces
  (flat within a sixteenth of the resolution for non-neighbours), and on
  a failing non-neighbour pair whose surfaces it finds within the
  resolution (a witness), all with `Invalid`; with `Invalid` too, of the
  failure that asked, on a leaf to split under `MIN_SPLIT` = 64
  resolutions if flat or `MIN_CURVED_SPLIT` = 8 if curved (it was
  `TooComplex` at 64 for all: only curved pieces' sag shrinks as they
  are split, and a scale limit is detail too small for the tolerance,
  not complexity); with `TooComplex` at `MAX_REFINE_DEPTH`,
  `MAX_PATCHES` and the budget. A small leaf to split on a plane fails
  with the failure that asked, not the wrong-tag `Face` the refiner
  would have named on splitting it.
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
  drawing every face boundary would draw a circle's seams. The rule is
  now by face key (`FaceName::key`), which is that and more.
- **`Solid::bounds` is an `Option`** (`None` for the empty solid), and
  `Solid::bounds3` gives the `f64` box. `Display` holds only the fit
  tolerance; the other targets are its constants.
- **The chord target is kept on edges, and inside patches curved both
  ways only**; inside planes, cylinders and cones it can be about three
  times off at the corners of skewed patches (see "Solids and
  tessellation"). The plan counted every part from the segment counts
  alone; patches curved both ways are now measured first, still before
  any vertex is made, in rounds bounded by the limits.
- **`Shape` is gone**, not kept as a test helper: `Solid::cuboid` and
  `Solid::cylinder` are the test solids, so `Shape`, `ShapeError` and
  `position_in_range` were removed from the kernel.
- **`Symmetric` is the whole depth**, half each side, and extrude
  distances are at least `MIN_LENGTH` (1 µm), as dimensions are, rather
  than only above zero.
- **`BodyId::NEW`** stands for the body an extrude not yet added makes;
  `AddFeature` and `SetFeature` give the body its id whatever the
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
  are used, with point location and constraint removal for the
  triangulation kept from round to round; its refinement and its bulk
  loading, whose hash sets could be iterated in a random order, are not.
  The points go in in a fixed shuffled order, which bounds the flips.
- **Flat corners are mended on a second try** of the caps, when the
  first fails, rather than always: moved-in points can line up into
  slivers of their own, so doing it first lost some solids the plain
  caps give. The second try resumes from the first round of the first
  that found a flat corner, and isn't made when there was none: the
  same results as starting over, for less work.
- **Caps are refined for quality on the first try**, not as a third try
  after the plain and flat-corner ones: plain caps that pass keep long
  thin triangles (a plate's fans out of a corner to holes on a common
  tangent) that a later cut along them fails on, so every cap triangle
  with an angle under 5° is refined, at the cost of patches (see "Cap
  quality"). Both tries refine; the plain caps' own two tries come
  after them, the second from the plain first try's fork, unrefined
  for crowding too (extrude step 5), so what they extruded still
  extrudes unless refinement spent the budget first. Refinement goes
  before the ears' centroids when nothing is to be halved, halves an
  encroached chord again while the asking point still encroaches on a
  half, keeps its points in rounds that halve, and a mending halving
  refused as too small is left to refinement rather than failing the
  round: each found
  needed on fine circles of arcs, perforated plates or spline chains.
  The flat-corner second try stays: measured against refined caps
  then plain ones alone, it still mends 9 and 14 of 500 plates of small
  holes at fit 0.1 and one of 500 at 0.01, short curved segments whose
  chords refinement leaves alone (see "Cap quality"); nothing else
  measured needed it.
- **Mending on the tries that refine stops halving a segment at
  `MAX_MEND_DEPTH`** (6) and leaves the corner to refinement, rather
  than halving on to `MAX_CAP_DEPTH` (16): a narrow corner along an
  arc's tangent to a far vertex doesn't open however often the arc is
  halved, and the rounds spent there pushed the 400-hole plate to the
  second try and limited `k × k` plates to `k = 51`; now `k = 53`, on the
  first try, with fewer patches. Results changed only where a segment
  reached the depth (perforated plates); no case measured was lost.
- **An extrude's mesh is checked before it is repaired**
  (`Solid::new_repaired_within`), not repaired, merged and then checked:
  the construction nearly always passes, repair would keep it as it is
  and merging changes only names, so the result is the same for one pass
  over the pairs of patches rather than two. The check is charged as
  repair's first pass was, so for meshes that pass budgets run out where
  they did; one that fails after the fold check (274 of some 4 400 tries
  over the random plates measured) pays the pass twice, and may run out
  where it didn't (`TooComplex` for `Invalid`, both errors the next try
  is made on).
- **The caps' refinement shares only its bound and circumcentre with
  the boolean's shape points** (`mesh/shape.rs`: `SIN_SHAPE`,
  `circumcentre_from`), not the queue, walk, clearance and flips: the
  caps insert into spade's constrained Delaunay triangulation, which
  locates, flips and keeps the faces, test region membership by corner
  triples and clearance by BVHs over control hulls, and may halve
  chords; the cut faces keep their own triangle list and edge map, walk
  across sides with exact orientations, test clearance by flooding the
  triangles within it, flip only under curved-corner rules (`flippable`)
  and halve nothing. A shared core would be a triangulation abstraction
  over both with their rules as callbacks, more code than either loop.
- **The caps' triangulation is kept from round to round**, points
  inserted and halved chords replaced in it, rather than made afresh
  each round: rounds of mending and of refinement each paid 8 units a
  vertex, which limited plates of `k × k` holes to `k = 28` once refined
  (38 plain) and ran the 16 384 fan out of budget; kept, they fit to
  `k = 51` and the fan passes. Where four points lie on a circle the
  triangulation is one of the Delaunay ones rather than the one the
  shuffled order makes, so the patch counts of such caps changed; caps
  under 64 vertices are triangulated afresh after a round that changed
  them, which keeps small solids (and the seeded boolean tallies) as
  they were. The fork the second try resumes from copies the kept
  triangulation. A halving asked for within half a circumradius of an
  earlier ask in the run isn't asked again. Refinement inserts its
  points one by one from a queue (the plan's sequential Ruppert), not in
  batches a round spaced half a circumradius apart; its halvings wait
  for the queue to empty. The crowding gate (the plan's count after the
  first triangulation, before any refinement) runs once a try when
  nothing is left to mend, since refinement for quality runs on every
  try now and leaves only the fans its exemptions keep crowded, and
  not on the plain caps' tries, which make what they made before; it
  refines with the same 5° bound (radius-edge ratio about 5.7, the
  plan's `B` between 3 and 5 being about 6° to 10°) and `c` = ½.
- **`extrude` takes a `Frame`** (origin and axes) and the extent as
  `from < to` along its normal; flipping and sides are the caller's.
- **`KernelError::Profile(ProfileError)`** carries a profile's own
  errors: `Empty`, `TooManySegments`, `Short`, `Segment`, `Degenerate`,
  `Open`, `Area`, `Cusp`, `Touching`, `Nesting(loop)`, `Triangulation`,
  `TooFine` (a curved segment the caps need halved below `MIN_SPLIT`
  resolutions: detail too small for the tolerance).
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
- **The regeneration cache is a least-recently-used cache bounded by
  bytes on top of keeping what the last request used**, not two
  generations: what the request before used is never evicted, and older
  results stay while they fit the budget (256 MiB natively, 64 MiB on the
  web); the touch tests and booleans of bodies a join, cut or intersect
  takes out are marked used (`Cache::keep`), for putting them back. Not
  weighted by cost: timing per entry would make eviction depend on the
  machine. It also keeps meshes and whether sketches solve, and
  keys are 128-bit hashes of the values' postcard encodings, not their
  `Hash` (sketches hold `f64`s).
- **A failing draft is answered with the committed model** and the
  draft's error (`Drafted`), rather than the draft applied without its
  body. `touched` is in `Drafted`, not beside it on the response.
- **A join touching several bodies merges them; cut and intersect work
  on each target body on its own.** Consuming a body is worked out at
  regeneration, not stored in the document: targets are found by
  `touches` there, so a stored consumption would go stale on any edit
  upstream. A consumed body stays in the document and in Objects, marked
  "in Body 1" there; regen says where its geometry went
  (`Evaluation::merged`, on the answer and the wire). Unticking all but
  one body joins to that one alone.
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
  that sketch's regions, so `SetFeature` never moves an extrude to
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
  taken as zero, and then every later coefficient that is only rounding
  (within what moving every number it is worked out from by `2⁻³²` of
  itself, but by no more than the tie distance, makes of a zero:
  `exact::Moves`; the plan's measure, the coefficient on its terms'
  absolute values, grew as `|x|²` and far from the origin skipped real
  orders, refusing 390 of 2 700 turned pairs moved by up to `1e5`, none
  now). The plan has flat
  operands decided exactly; with rounded coordinates (turned and moved
  flush solids) exact signs broke the perturbation's intent, and beside
  curved operands they disagreed with the curved primitives' ties.
  Flat operands whose near ties don't fit together are decided again
  exactly (`Inconsistent` only; `touches` too, which the plan for the
  retry didn't name). For parallel shadows the plan had a second scale
  for `Height`'s first order; the rounding rule already decides that
  order, and what was left was the constant term at a scale that
  rounded to 0: `Height`'s scale is floored at its own rounding
  instead. The curved primitives' `first_sign` skips orders that are
  only rounding as well (relative to `Σ |d_i·f(e_i)|`, not a `Moves`
  evaluation: `f` is a closure in floating point, and linear in a
  motion, a direction, so with no tie distance to cap).
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
- **A fitted run's shoulder is set by height off the chord**: the
  furthest sample's distance from the chord over the control point's,
  not its projection on the line from the chord's middle to the control
  point. The projection overshoots for a lopsided run (a chord end itself
  projects ahead of the middle) and kept an exact parabola cut into eight
  Béziers from being one conic; at a conic's shoulder its height is
  largest, so the height is exact for a run that is a conic. On the wavy
  corpus it left 2 sketches that extruded before failing against 4 for
  the projection, with fewer segments.
- **A fitted line must run along the spline's tangents**: a run, or a
  single Bézier, becomes a line only if its end tangents are within 45°
  of its chord as well as within a quarter of the tolerance of it; a
  single Bézier that fails only the tangents and fits no conic is still
  a line. Straight within the tolerance alone let two runs of a sliver
  narrower than the tolerance double back on each other (no area, or a
  cusp) where span-by-span fitting kept it a region. It changes no
  refusal on the corpus above (segments at 1e-1: wavy +19, blob +7); of
  173 slivers 0.002 to 0.2 across at a fit of 0.1, those span by span
  extruded and runs refused went from 28 to 2 (two lobes about 0.004
  across, each of their two spans straight within the tolerance and no
  conic's, so two lines; span by span happened to fit one of them with
  conics, walking it the other way).
- **The regen lane's joined scenes are cache entries with one more
  protected key**: the scene of the last answer without a draft is never
  evicted, besides what the request before used, so the committed model
  asked again after any number of draft revisions isn't joined again,
  even with no budget. Scenes are counted in `joins`, not `counts`, so
  `counts` stay feature counts. (They were a slot of two scenes before
  the cache was bounded by bytes.)
- **What the rounds of cutting keep is bounded by the fit tolerance**,
  including a round that finished: every triangle held to it (along a
  cut, from the patch; off a quadric onto the copy claiming no surface,
  from the quadric) must be within it, else `TooComplex`. The plan
  refused only when the rounds ran out with bands along a cut past it; a
  round can also finish with a triangle off its quadric that has no
  side on the face's boundary to halve. Such triangles ask for
  their boundary curves to be halved from half the tolerance, as those
  along a cut do, and the clean-up's moves onto the copies are held to
  the same bound.
- **A crossing only placed is certified against the other operand's
  patches near it**, not only the one it crosses: a crossing at a cap
  edge's end lands a micrometre past the patch crossed, in the next one
  of the same plane, and the plan's check refused it (a boss's rim
  tangent to cap edges between holes). The foot of the perpendicular is
  tried before the search, which ran into its cap on a thin triangle
  holding the point. Without the two, 8 of the 160 drilled plates'
  operations were refused.
- **Crossings only placed go to a root checked on the patch**, by a
  certified distance within the resolution (`solve::near_patch`), not
  by inverting the point into the patch with a slack: that is what the
  check after it asks, and a crossing at a side two patches share lands
  a rounding past either. A root whose tangent lies in the surface (a
  double root) fits either sign. Where no root fits, the placement is
  what it was before (a plane's nearest root, a quadric's within
  `1e-6`) and the check decides, so the change can't add a refusal
  there. Crossings the search solved keep their placement (a plane's
  nearest root, unchecked), except a quadric root further than `1e-6`,
  which is now taken if it passes the same checks. The test switch that
  skipped the check (`UNCERTIFIED`) became `LOOSE`, which also leaves
  crossings only placed where they were, so the backstops can still be
  tested. The bars through boxes were exact to `1e-9` of their size, not
  `1e-12`, until section arcs' weights were taken from the angle; now
  `1e-12`.
- **Section weights by angle on elliptic cylinders too, and the control
  point by angle**: the plan took the weight from the angle on circular
  cylinders only, keeping the control point where the tangents meet and
  a plane within `1e-9` of the rulings giving a straight edge. An
  elliptic cylinder (a wall over an ellipse arc) is an affine image of a
  circular one, so the same formula in its own metric gives its arcs'
  weights; without it a box turned a thousandth of a radian across such
  a wall stayed `5.6e-7` off in volume. The tangents of a nearly
  straight arc meet only roughly and the straight edge was up to
  `5.3e-11` off the cylinder (the plan's measured residue), so the
  control point comes from the angle as well, and on these cylinders an
  arc is a straight edge only between ends on one ruling. Parabolic and
  hyperbolic cylinders and other quadrics are as before.
- **The points of a patch above a vertex keep Newton's method's from any
  piece where the search ran out of pieces**: found while looking at a
  fuzzing refusal (a small cylinder
  `1e-3` through a steep wall), not in the plan. The vertex's winding
  number had a point the search, run out of pieces, didn't find, and
  guessing it above gave every edge at the vertex a crossing that isn't
  there, which the check on crossings only placed refused
  (`Inconsistent`); the operation is still refused, now by the result's
  hull rules (`Invalid(VertexNeighbours)`), with the decisions
  consistent. Only where the search ran out: where it didn't, the
  piece holding the point found it itself, and a second find from
  another piece near a fold (Newton's method converging loosely, two
  finds further apart than they are merged) would have counted it
  twice.
  Finds from other pieces are themselves merged, one per point and
  facing: on a strip of a cylinder seen `1e-4` off its axis they were
  up to ten finds of one point, which the winding number's fit had to
  drop.
- **Empty components by their curved volume**: not in the plan, found
  fuzzing boxes against cylinders. A face along a wall's rulings
  cutting off a sliver thicker than the fit tolerance gave an empty
  `Ok` (`a − b`) or lost the sliver (`a ∩ b`), since the clean-up
  measured components by their triangles' corners, all on the cutting
  plane. The fix turns two of the sliver test's 32 operations into
  `Invalid(Fold)` refusals (slivers on turned boxes); before, they were
  wrong. A box tilted `1e-5` or less off the rulings and tangent to the
  wall at its middle leaves slivers a few resolutions thick, whose
  bands (within the fit, claiming no surface) graze the wall: their
  volume was off by more than their own, `5e-6` on a `3e-7` sliver,
  with every point within `1e-4` of the true surface: the cut round the
  section's tip was a straight chord, which the check on fallback
  chords now refuses (see "Chains").
- **Crossing searches drop pieces whose control hulls are apart**
  (`hull::apart`), as the plan for tall walls has it, by more than
  `1e-12` in the search's unit frame, as planned, plus `2048 ·
  PIECE_SLACK` of the pieces' sizes, which the plan didn't have (so a
  solution Newton would count just past the patch's side or the edge's
  end is never dropped; see "Crossings"; the boxes and the slab are
  given the same margin, which they didn't have before either; and,
  which the plan left as an option for searches still at their cap
  after the hulls, patch pieces more than 8 times longer than high are
  halved across their longest side, not quartered: curved edges whose
  control hulls hold a tall wall's width, a rim arc through a pin or a
  tool's rim along a tall wall, still ran to the cap), and the charge went from 4
  pieces a unit to 2 (a piece measured twice as dear where every piece's hulls
  touch; single thread, release: random edges and patches 0.3 → 1.0 µs
  a piece but 5 times fewer pieces, 0.2 s → 0.13 s for 20 000
  searches; a cap edge across a drill 1 010 tall 1 047 → under 100
  pieces). The test switch `assemble::LOOSE` also turns the hull test
  off, so the backstop tests (two bars through boxes, a cylinder across
  a 75° wall) still see their searches run out of pieces and their
  crossings placed off the surface: with the hulls the searches find
  those crossings and the loose results came back `Ok` and exact.
  Before and after, release (after the exact roots on quadrics and the
  certification of crossings only placed had already won back most of
  them): the drilled 60 × 40 plate, 47 drill positions at 7 heights
  from 10 to 10 000, 44, 45, 45, 45, 45 of 47 from 100 tall up (errors
  `Invalid`) → all 329 (and the box drilled the same, 329 both), 7.2 s
  → 3.5 to 4.4 s of user time; a box notched into a tall cylinder's
  side, 60 of 60 both; the arc fuzzers (walls 300 cases, seeds 21 and
  5; turned tools 200, seeds 21 and 5): walls unchanged (65 and 59
  refused, none wrong), turned 17 → 17 and 16 → 13 refused; the
  seeded suite unchanged (related 112/120, chains 203/240, turned
  156/160, tangent 72/96, coaxial 37/40, bosses 64/64, drilled
  160/160).
- **Fallback chains are checked against the true cut, and the other arc
  of a plane section is tried** (see "Chains"), as planned, with one
  addition: where the conic along the end tangents fails the check, the
  straight edge between the ends is checked too and kept if it passes,
  before the boolean is refused. Before and after, release, default
  tolerance: grazing planes against cylinders of 3 to 8 arcs (tilts 0
  to 0.5 radian off the rulings, two thirds of the offsets within a
  fraction of the radius of tangent; seeds 1 to 4, 200 cases each, 3 200
  operations), `Ok` 2 200 → 2 180, refused 1 000 → 1 020
  (`Inconsistent` 65 → 160), `Ok`s off by more than `1e-7` in volume
  16 → 2. 18 `Ok`s became `Inconsistent`: 14 of those 16 (up to
  `1.1e-5` off, among them the sliver that lost its tip), and 4 within
  `1.5e-7` whose chords were up to `1.2e-3` from the cut, or within
  `2e-6` of both surfaces where they meet at `1e-6` radian (so up to
  1.9 from the cut, by Newton's method). Trying the other arc turned 6
  `Ok`s into `Invalid` or `TooComplex` and 4 refusals into `Ok`s (the
  hull rules seeing exact cuts where they saw fitted ones). The two
  left off keep the cylinder at a tilt of `1e-6`, within `1e-12` of
  tangent: `4.9e-7` and `2.4e-7` on volumes of 92 and 65, inside the
  sliver's depth, unchanged. Walls over arcs (300 cases, seeds 21 and
  5) unchanged, 65 and 59 refused, none wrong; tools turned across
  them (200 cases) 17 → 15 and 13 → 13 refused. The seeded suite
  unchanged (related 112/120, turned 156/160, tangent 72/96, coaxial
  37/40, bosses 64/64, drilled 160/160, walls 112/120, bars 22 of 24 at
  each of three seeds) but for parts built in chains, 203 → 201 of 240:
  three operations refused at two steps, cuts between cylinders
  crossing at a slant whose tracing failed at a point where the cut
  touches itself, and whose straight chords were `3.6e-2` and `1.6e-2`
  off it (36 and 16 times the fit), which the suite's volume checks (a
  fifth of the fit times the area) let pass; one operation a step later
  came through, fed on another part.
  In app-like cuts (a plane half a radian off a boss's rulings, 24
  offsets) the plane–cylinder chains not exact go from about 40 of 48
  operations to none.
  Bug hunt, release: grazing planes, seeds 1 to 8 at fits `1e-3`,
  `1e-4` and `1e-5` (6 400 operations at each), checked by volume, by the
  identities and by the result's extent along the axis against the true
  sliver's (sampled along every patch side): none wrong. Against the
  kernel before the change, at `1e-3` seeds 5 to 8 and `1e-4` seeds 1 to
  4, 26 and 34 `Ok`s became `Inconsistent` (volume errors before from
  `1e-12` to `1.1e-5`), among them both of the old kernel's wrong `Ok`s
  there (a sliver's tip dropped, its extent short by 0.5 and 2.2). Every
  plane–cylinder chain was measured against the true section both ways
  (where the plane is at least `1e-5` off the rulings): exact arcs,
  either guide, within half the fit. The other arc can lie partly off
  the patch (its middle within the `−0.25` the test allows, its end
  `0.36` out), seen only where a plane touches along a ruling and the
  arc's end itself is off the patch, the operation refused. Crossing
  cylinders near tangency (square axes, the gap or overlap `0` to
  `1e-3` of the radius, at fits `1e-3` and `1e-4`, 900 cases, the half
  not moved measured): each chain kept from a fallback was within half
  the fit of the true cut loop, both ways; all of them were in
  differences. Boxes nearly parallel to the caps: none wrong but slivers under
  the resolution, as before. 1 and 8 threads give the same bits on all
  three. Parts built in chains (seed 5, 15 chains): 10 of 1 200
  operations refused that were right before (their first chords between
  cylinders `3e-3` to `3e-2` off the cut, which the rounds used to halve
  onto it).
- **`touches` doesn't refine.** On curved operands it counts once and,
  where that shows no meeting, searches for surfaces within the
  resolution (`boolean/near.rs`, "Touches") instead of running the
  boolean's refinement to the end. Its charge is a unit per visit (the
  plan guessed one to two, measured 0.24 to 0.44 µs a visit). This also
  covers the plan of stopping `touches` at the first round whose
  counting shows a meeting: it counts only that round. A loop no edge
  crossing shows still counts, found by the search (it used to run the
  pair decisions for that). The search is `pub(crate)` with a visitor,
  for the minimum distances to bound.
- **Seams: straightened first, regions as the fallback, faces merged by
  the seams that joined them.** The plan for flush unions blowing up was
  to triangulate again each region of triangles in one plane linked
  across seams. Measured, nearly every seam needs no more than making
  it its chord (both triangles in one plane, the lens moves from one to
  the other), so that runs in the clean-up's rounds, before the flip
  `unbend` tries, and the regions run once after as the fallback, for
  clusters of zero-size triangles at a rim (grown across short edges and
  flat triangles too, not across seams alone). "In one plane" is tested
  on the neighbour's geometry, not its tag: a seam's other triangle is
  often a remnant of the wall at the rim, tagged with the cylinder,
  which then moves onto the plane face. A straightened triangle must
  also pass the fold check (not in the plan; cheap, and only seams it
  can't straighten are left to repair). Merging the plane faces is done
  here, as a union of the faces the mended seams joined, onto the
  lowest id; a general pass merging any adjacent faces of one surface
  after a boolean (coaxial walls, plane faces meeting along straight
  edges) isn't built yet and would take this over. The tests are in
  `boolean/curved_tests/flush_seams.rs` (a new module beside
  `curved_tests.rs`, which is long), with patch bounds from the measured
  counts with slack rather than twice the plate-first count; the seeded
  `flush_unions_either_order` holds unions to 95% and all four
  operations to 86% (what fails there are intersections and
  differences keeping a cap corner where a straight side runs on into
  an arc at a tangent, as before). The clean-up no longer keeps a
  record left from an edge no triangle has where a new side runs
  (`flip`, `unbend` and a collapse moving an edge onto the vertex kept;
  `delaunay` already dropped it).
- **Topology and names, as built** (the solid-operations plan's names,
  keys, aliases and resolving): `FaceKey`'s fields are public (they are
  deserialized from files anyway); `FacePart` keeps its order (`Split`
  stays fourth) with the new parts after it, and `PartKey` follows it.
  Resolving lives on `Topology` and takes keys and a point; the
  document's `FaceRef`/`EdgeRef` (with the body) come with the features
  that store them. A failed reference is `topology::NotFound`, not a
  `KernelError`. Candidates are measured by a linear pass over their
  patches nearest box first, not through the BVH, with one search
  allowance shared by the whole resolve. A corner matches when each of
  its three keys names some region there (two keys may name one region
  through an alias). Aliases are recorded by the flush seams' face merge
  (whole faces) and by `unbend` for the triangles it moves (the moved
  face may live on elsewhere under its own key: then a key names both,
  and the point picks); the general merge pass (one surface, one face,
  `Mesh::merge_faces`) records them on the mesh after repair. Not in the
  plan: flush caps one of which the perturbation drops whole (no merge
  happens, so no pass records it) are aliased after the boolean where a
  triangle middle of the dropped face lies on a result face of its plane
  or one of that face's on it (`boolean::covered`); names only, never
  the geometry. Ties between candidates go to the lowest index within a
  billionth of the solid's size, not only exact ones.
  Transforms don't exist yet; the table is on `Mesh`, so they carry it
  by keeping the faces.
- **Fitted strips are least squares, halved by what station 0 says.** The
  plan fitted the diagonal by Gauss–Newton until every sample was within
  half the fit tolerance, halving either way. Built: Levenberg–Marquardt
  on the signed distance at a grid of points, then the worst error
  *measured* by climbing from the grid's peaks (`deviation`, a found
  maximum, not a certified bound: accepted up to half the fit tolerance
  less a 64th of it), and
  halved along the meridian or round the axis by which gains more at
  station 0 (round the axis is the caller's: the band asks for a halved
  lathe). The tori come out with fewer patches than the plan estimated (768
  at the default tolerance against about 1 000, 2 560 at `1e-4` against 4
  000).
- **Rings at turns: the cylinder rule.** Not in the plan, found building
  tori: where a meridian's tangent is square to the axis the surface
  touches the parallel's plane all along it, and the edge rule (a plane
  through the shared curved edge's control points with the two sides on
  opposite sides of it) can't pass there, so `check` refused every ring
  at a turn: a full circle split at its top, a flat face meeting a round
  tangentially, two arcs tangent at a top. Repair rescued some only at
  coarse fits and absurd cost (a puck at `1e-2`: 64 → 80 976 patches) and
  ran out of budget from `1e-3`, so every revolve of a profile with a
  rounded edge would have failed. Bands first kept their own rings off
  turns (they still do, being cheaper), but a profile vertex at a turn is
  the boundary between two faces and can't move. Fixed by a second
  certificate for curved edges in `check` and repair: the two patches on
  opposite sides of the cylinder over the edge's conic, by the signs of
  `F = λC² − 4w²·λP·λQ`'s degree-four Bernstein coefficients (see
  "Control hulls"). A reviewed alternative, one side in the plane and the
  other's control points under it with the fitted diagonal held in the
  tangent plane, covers only a flat side against a round (not two rounds,
  an S or a concave fillet) and needs a constrained fit: not instead of
  the cylinder, but revolve found it needed beside it (a wall straight
  down from a concave round's bottom stands on the cylinder, so only the
  plane can part them, and only with the band's diagonal in the plane),
  so bands now hold the diagonal there where a piece ends at a turn
  ("At a turn" under "Fitted strips"). A
  pencil of the plane and the cylinder (`α·F + β·plane·W`, a two-variable
  LP) would cover creases where both sides leave the edge the same way
  at different angles; revolve's creases needed it, and it is now the
  third edge rule ("The pencil" under "Control hulls"). Bands and caps keep the
  plane rule alone (with the cylinder they chose coarser strips that
  failed the vertex rule). An existing test's pair, two patches across a
  sideways curved edge, now passes (it really is embedded); the folded
  case is one curved up out of their common plane. With the cylinder,
  bands leave a turn within `1e-2` of a piece's end (in its parameter)
  to the ring there instead of balancing it (`TURN_NEAR_END`, before
  `1e-6`): the piece over the turn that balancing cut was a sliver
  (`2·1e-5` of a quarter arc turning `1e-5` rad past its end), which the
  band halved round the axis until `TooComplex`, so a profile whose arc
  went `3e-6` to `3e-4` rad past a turn was refused at every fit.
- **Caps don't grow the angular split.** The plan raised `k` when a cap
  wanted an arc halved; halving the cap's meridian toward the pole alone
  brings it within the tolerance (the error falls as `δ²` on a sphere,
  as the length on a cone), so caps only halve their meridian, and the
  rest of the meridian comes back in pieces growing at most 16 times a
  ring.
- **Lathe calls take a `Budget` each.** `fitted_band` and `pole_cap` start
  their own `Work` from the budget; revolve, which makes many of them in
  one operation, should share one through `fitted_band_with` and
  `pole_cap_with` (crate-internal, taking `&mut Work`).
- **One surface, one face names faces, it doesn't merge them.** The plan
  moved the members' triangles onto the lowest face and dropped the
  rest, giving them its tag. Done that way, an extruded circle's four
  arcs claimed the first arc's quadric, written in its coordinates, and
  a boss on a plate drilled with eight holes round its rim came back
  `Inconsistent` (`boss_rim_tangent_to_cap_edges_between_holes`). So
  the pass changes names and aliases only: each member keeps its entry
  and surface, takes the root's name unless it already has its key, and
  gets the set's aliases on itself (a later boolean may cut the root
  away and keep a member). Validation measures against the widest
  member's surface, not the root's, and leaves out only the members off
  it, not the whole set (see "Structure"). The pass runs on the finished mesh, not on
  the clean-up's soup, so aliases go straight onto the mesh rather than
  through `Soup::absorb`. The clean-up's seam merge (`merge_joined`)
  stays: the Delaunay flips after it flip only within one face index.
- **The plane faces' quality pass departs from its plan in five ways.**
  The bound is 5°, not 10°: measured on the nine 60-hole grids, 5°
  failed fewer steps (none, against 1 at 10° and at 20°) with a third
  fewer patches than 10° (half of 20°), and 10° lost bosses sunk
  through drilled plates and flush unions to their patch counts. The
  queue takes the worst sine first, not the smallest circumradius, as
  the points for shapes on curved layouts do. A face where some
  triangle's curved corner is closed isn't skipped whole (a 64-hole
  plate's top always has one): its closed corners are flipped open
  where they can be, and only points near the curves that stay closed
  are refused. Three steps were added, each for a case that looped or
  cascaded: inner vertices of plane faces at bad triangles are taken
  out where their stars triangulate better (seam leftovers a tenth of a
  millimetre off a cap's edge), the made triangles are flipped towards
  Delaunay first (thin right triangles otherwise split into copies of
  themselves), and every point is checked against the diametral circles
  of the constrained sides of the triangles it splits, a free side's
  halving point too. And triangles' corners along curves are judged
  from the curve's tangent: a corner the chords made 23° wide but the
  tangent 1.85° came out of one boolean and, halved by the next one's
  refinement, closed (a flush boss on a drilled plate). The shapes
  points' code in `triangulate.rs` works on one input triangle's layout
  with its sides fixed; this pass shares its in-circle test, the bound
  and the circumcentre (`mesh/shape.rs`, with the extrude caps' quality
  refinement), and the seams' triangulation of a region, but
  walks and flips on the soup, where a side between two plane faces can
  be split in both.
- **Revolve as built.** `revolve(profile, frame, sweep, feature, tol,
  budget)` takes the extrude's `Frame` (`y` the axis) and `Sweep::{Full,
  Part { from, to }}`; a part turn's angles must be within `8π` of `x`.
  Faces are flat where a line's ends are within the resolution of one
  height, cylinders (as a form; the strips are the cone's) within it of
  one radius, spheres where an arc's centre is within `m·w/4` of the
  axis (`m` the resolution, `w` the arc's weight): choices of a face's
  kind, its tag checked, like the extrude's straightening. A segment
  coming within the resolution of the axis inside is refused in part
  turns too (the plan refused only a vertex alone, in full turns), and a
  part turn too close to full is `ProfileError::NearlyFullTurn` (new,
  with `CrossesAxis` and `TouchesAxis`). Where the ends' caps want a
  profile piece halved, that piece is halved and the round runs again
  (the plan grew `k`, which halves only round the axis); flat faces'
  caps wanting a ring's arc halved grow `k` as planned. `Lathe::turned`
  keeps points on the axis (as `on_axis` has them) where they are, so
  meridians ending at a pole keep it to the bit at every station. Bands
  hold a fitted diagonal in a turn's plane where a piece ends at a turn
  ("At a turn"), which the groove needed. The faces' and ends' caps
  are the plain caps (`Mode::PLAIN`, then `Mode::FLAT_CORNERS_PLAIN`),
  without the extrude's quality refinement.
- **The fold rule falls back to the clean-up without it.** Its plan
  kept the rule safe by running it last and only where the area round
  the vertex drops; on faces a hair off each other that still lost a
  result the clean-up without it gave (1 in about 440 000 steps of
  chains on such frames), so a result that fails the check after the
  rule fired is made again without it ("Unfold"). The soup is copied
  before every clean-up for it. This fallback is inside each of the
  booleans' tries (`checked_with`), the one joining walls' lines and the
  one without the joins ("Tried again without the joins"), so a try's
  error is the one its clean-up with the rule gave.
- **Scales as built.** `Motion::scale` takes factors within
  `1/MAX_SCALE ..= MAX_SCALE` (`1e6`), a kernel bound beside the
  feature's `1e-3 ..= 1e3`. Where the plan made every circular cylinder
  scaled per axis a cylinder over an ellipse and every cone a quadric, a
  cylinder or cone the scale keeps round (equal stretches across its
  axis, a cone's also square to it) keeps its circular form: a
  `ConicCylinder` is never over a circle. A cone's `Form::Quadric` is
  both nappes. `slack` multiplies on every face scaled up, not only
  fitted ones (an exact face is on its form to rounding, so it costs
  nothing), by the motion's stretch, which for compositions is the
  product of their stretches (an upper bound of the largest). Picking
  summarizes a `Form::Quadric` as `Other` ("Curved") for now.
- **Creases: the pencil rule.** Edge neighbours were to be parted by a
  plane through the shared edge; the cylinder over a curved edge's conic
  came second (rings at turns, above), and revolve's creases into one
  quadrant of the meridian plane needed a third certificate: a member of
  the pencil of the two, `α·F + β·P`, found by a two-variable search and
  checked with its own rounding bounds ("The pencil" under "Control
  hulls"). The three-member family `α·F + β·P + γ·P²` (a free curvature
  in the meridian plane) would also part thin creases where curvature
  decides; it changed nothing on random revolves, so it waits for a
  case that needs it.
- **Measuring's curve pieces and box search go further than the plan.**
  The plan integrated a conic's length over pieces whose weights are
  within `0.97..=1.03`; that left `1.6e-7` on lopsided pieces of nearly
  half ellipses, so pieces also turn at most 45° and keep their legs
  within a factor 1.2 (see "Volume, area and measuring"). The box's
  search runs Newton's method during the search rather than only at the
  end (330 000 visits down to 2 400), and skips patches on planes and
  ruled quadrics, whose extremes are on their edges.
- **Minimum distances go further than the plan's hulls and Newton.** The
  plan bounded pairs by their hulls' distance alone, over the broad
  phase's pairs. Coaxial walls (a tube's, a pin in a hole) are equally
  far everywhere, and a pin's wall from the hole's rim, so hulls only
  drop their pairs once split to `√(8·eps·r)`: 408 000 visits for a pin
  through a tube, refused at the finest tolerance. So rounds bound pairs
  too (the distance from a cylinder's axis or a sphere's or circle's
  centre, ranged over a piece by its Bernstein coefficients). The broad
  phase is a descent of two box trees in the search itself, since a
  margin from a first bound made `n·m` pairs for bodies far apart.
  Newton's method steps back up from edges it overshot onto, and runs on
  a bounded number of pairs within `eps` of the best, so answers are to
  rounding in practice rather than within `eps`. A body's distance is its
  surface's (a body inside another is not 0): the simplest reading, as a
  containment test would need the booleans' counting.
- **Walls along one direction are certified by nearness, not from a
  round on.** The plan certified such pairs with no ends from round 2
  of `refined` (a gate measured from 2 to 4), because certifying them
  from round 0 lost results: refinement of walls that don't meet was
  helping later stages. Diagnosed, those were all caps' flat rings
  between two parallel walls a little apart (`flush_rims_take_the_halved_cuts_vertices`:
  an ear with three corners in line and a quarter arc for a side,
  which folds; `stacked_cylinders_a_step_apart_keep_the_step`,
  `a_cylinder_inside_a_larger_one_over_one_span`, seeded `related`
  coaxial walls 0.0038 apart: ring triangles failing the hull rules
  against unrefined walls bulging across the ring), which need the walls
  refined until their hulls part. No round gate keeps them all: at
  round 2 `related` lost 2 (113 → 111) and the stacked step ran out,
  from round 4 the step still failed, and from round 5 a tangency's
  refinement no longer fitted 200 000 units. Certifying only walls
  that come within 64 resolutions of each other keeps every refinement
  that parts hulls (walls further apart) and stops a tangency's: every
  test passes, every tally holds (`related` 113, bosses in drilled
  plates 148 → 149), and the probes match the plan's gate 2. Rings
  narrower than 64 resolutions between such walls are left to fail.
- **Ends along one direction join where the crossing is clear, not by a
  grouping distance from the larger radius.** The plan grouped a pair's
  ends within the planarity size `√(8·R·resolution)` (`R` the larger
  radius) and left closer lines to refinement. That sees the other line
  only when the same pair holds it, and the larger radius undercounts
  a pin nearly the size of its hole (relative curvature `1/r − 1/R`).
  Each end's crossing angle bounds the sliver beside its line wherever
  the other line lies, so the test is `θ² ≥ 2·LENS·resolution·κ` with
  `κ` the sum of the curvatures, and ends group within `θ/κ`. At
  `LENS` = 2 it is the plan's distance for equal radii; measured from 2
  down to 1/16 on 384 overlaps of 0.02 to 30 resolutions, no value down
  to 1/8 turned a result into an error and every one below 2 worked
  more (177 at 2, 194 at 1/2, 214 at 1/4 and 1/8, 229 at 1/16, which
  turned one). The tangent tally holds from 8 down to 1/32 and every
  tally at 2 and 1/4; at 1/2000 the tangent tally fell from 72 to 70,
  as with the plan's 64-resolution prototype. `LENS` is 1/4. On wider
  probes it does turn some results into errors, far fewer than it
  wins, and most of those come back from a second try without the
  joins.
- **An operation that joined lines and fails is tried again without
  them.** Not in the plan, which joined or split each pair once. The
  second try is bounded by the first's work (at least 150 000 units),
  so a refusal takes at most about twice as long; it wins back all but
  a few of the results joining lost ("Tried again without the joins").
  The rules tried before (a larger `LENS`, steep crossings left
  to refinement) lost as many results as it won back.
- **Every `Solid` is refused with a triangle facing against its face's
  plane form or plane tag** (`check`'s step 6, `FacesAgainst`), in every
  build, where only debug builds checked forms: caps a tie apart gave
  such results (see "Forms"), one of them only with walls' lines joined,
  and flush faces a hair apart one against its tag. The plan for the
  tags added the test to `check` for tags only, beside the booleans' own
  look at the forms; the forms joined it, one test in every build for
  every producer, so debug builds refuse these results as release ones
  do instead of panicking first. Both cases seen were on faces claiming
  their plane; faces claiming no surface (copies of faces with triangles
  off their plane, inputs moved without their tags) keep the plane form,
  which only a test of forms sees.
- **In-plane crossings also for edges with one end in the plane.** The
  plan kept only crossings of edges with both ends within the tie of a
  flat face's plane inside its triangle. An edge with one end tied and
  the other within the resolution is kept inside as well, where some
  part of it is inside, and left where rounding has it where none is
  (never `Inconsistent`, so the curved path loses nothing): such an
  edge, nearly along the plane, gave the one result the hunts still
  refused for facing against its tag ("In-plane crossings").
- **The flat primitives' broad phase pairs boxes within the
  resolution** when they decide near ties as ties (0 when exact). Their
  margin stayed 0 from before they took near ties as ties, and chained
  small boxes gave a wrong empty intersection and a whole difference
  ("Counting").
- **Results that touch themselves are named by a second rule too, and
  a classification out of budget keeps `Invalid`.** The plan named a
  failed result `NotManifold` only where its mesh before repair has two
  vertices within the clean-up's short length. Tangent unions of
  curved operands (a disc joined beside a round body, touching along a
  line) leave both shells uncut and fail `Hull` with no near vertices,
  so a `Hull` failure between separate shells is named so too (see
  "Results that aren't manifolds"). Where telling runs out of the
  budget, the plan would give `TooComplex`; the error stays `Invalid`
  instead, equally bounded, so out-of-budget classification never
  changes which kind of failure the measurements count.
- **Corners of 180° at tangencies are mended by collapsing the pair of
  crossings, not by flips or Steiner points.** The plan blamed the
  clean-up's collapse of a zero-length side for leaving two arcs of one
  smooth curve at a corner, and proposed judging corners across such
  sides when triangulating, or flipping or adding a point with straight
  inner edges on plane faces after the clean-up. Measured, the cases it
  named (a cylinder inside a plate touching its side, intersected) work
  at unit size and the default tolerance already; they fail where the
  two crossings the double root leaves are further apart than the short
  length (at the finest tolerance, or in millimetres), and then a strip
  of the touched face between the wall's two halves fails the hull rules
  too, which no flip or point on the top mends. So the clean-up
  collapses such an edge up to four resolutions long where it leaves
  that corner or that strip, moving a vertex only onto a point on all
  its faces' surfaces ("Collapse what a tangency leaves").
- **Unions touching along a line are named by the decisions for any
  ends whose walls aren't clear and face opposite ways, with a slit
  where they bend into each other**, not for a
  group of four ends within the tie distance. Where the line falls on
  the patches' edges (on the seams, the probe's placement at `z0` 0.5)
  a pair holds two ends, and the four-end rule named one of the probe's
  13 runaway unions; a union of walls overlapping by under a quarter of
  the resolution, whose lines aren't clear either, ran out the same way.
  So the test is the joins' own clearness at every end, with the faces'
  directions telling solids touching from either side (no manifold)
  from one inside the other (a manifold union, refined as before), and
  a slit at least a sixteenth of the resolution deep telling walls
  bending into each other with a gap from a pin plugging a smaller hole
  it touches inside (a manifold union too: the counting's ties gave its
  pairs ends, which were named so until the slit was asked).
