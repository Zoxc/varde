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

What exists today: the patch math (`patch`), below. Solids are still the
analytic cuboid of `Shape`, tessellated directly.

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
the points stay, and the middle weight `w` becomes `w / √(wa·wb)`. That
depends only on the edge's own three homogeneous points and is symmetric
in the ends, so both sides of an edge get the same bits.

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
`2w01(c01 − p0) × 2w20(c20 − p0)`. They are computed with the net moved to
`p0`, which keeps rounding relative to the patch.

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
or 180°, which no amount of splitting fixes.

It is **preserved by splitting**: a sub-patch's cubic is a positive
constant times the parent's cubic at projectively mapped points, so its
Bernstein coefficients are positive multiples of blossoms of the parent's
cubic at points inside the domain, which are convex combinations of the
parent's coefficients. The cone `{c : c·d > ε|c|}` is convex, so the
parent's `d` passes for every child (tested for 4-way splits, bisections
and random sub-triangles). Invertible affine maps keep it too (normals
transform by the cofactor matrix).

**The normal cone** (`normal_cone`) is that smallest cone, widened by
`1e-9` rad for rounding, or angle π when the coefficients don't fit in an
open half-space. It holds every normal of the patch. `NormalCone::apart`
says two cones share no direction either way round (the angle between the
axes exceeds the sum of the angles, and so does its supplement), the
certificate that two patches can't meet in a closed loop.

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
patches; the exact cone path when the quick directions fail; normal cones
holding sampled normals; the corner coefficients against the corners'
tangents; clockwise sub-patches facing the other way; bad weights
(homogeneous ones too), coordinates, arcs and split positions refused; cylinder strips on their cylinders, within their strip and sharing
their edges.

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
