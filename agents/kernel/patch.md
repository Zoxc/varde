# Patch math (`src/patch.rs`, `src/patch/`)

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

## Curves

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
  `center`, `a` and `b`). Profiles from sketches are built with it. For
  an arc under about 14° (`64·|a − b|² < |m|²`) the control point is
  worked out from the ends' middle, `(a + b)/2 + m·|a − b|²/2|m|²` (the
  same point, as `|m|² + |a − b|² = 4r²`; wider arcs keep the centre's
  form, exact for quarter circles about grid centres), so its rounding
  is the ends', not the centre's: from the centre, a nearly
  straight arc (a centre far off) carried the centre's rounding into
  `c`, which the circle's centre found from the curve again (`circle_of`,
  measure's and align's) multiplies by `(2r/|a − b|)²`: a 10 mm arc
  about a centre 1e5 off was 2.6e-11 off in `c`, its centre 0.01 off.
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

## Triangles

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

## The normal, the fold check and the normal cone

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
2·centre|²` (from the ends' middle for narrow arcs, see above) and the weight
`|a + b − 2·centre| / 2r`.

## Exact cylinder strips

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

## Swept strips (`src/sweep.rs`)

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

### Fitted strips (`sweep/fit.rs`)

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

### Lathes, fitted bands and caps (`sweep/lathe.rs`)

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
131° arc. At `1e-2`, `1e-3` and `1e-4` (and `1e-5` in release builds
under `VARDE_TESTS=full`) on the axis and two random frames up to `1e3`
out (quick: each profile on one of them by turns), all pass `check` with no
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

## Limits and errors

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

## Tests

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
