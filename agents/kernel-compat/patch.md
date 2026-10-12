# Patch math (`src/patch.rs`, `src/patch/`)

Pure math on one curve or one triangle, no mesh: the value types profiles,
meshes, evidence and the regeneration lane's curves are made of.

| file | holds |
|---|---|
| `patch.rs` | module docs, the limits, `Bounds<P>`, `PatchError` |
| `patch/conic.rs` | `Conic<P>` (`Conic2`, `Conic3`), the `Point` trait over `DVec2`/`DVec3`, exact arcs, splits |
| `patch/triangle.rs` | `Patch`: construction, edges, evaluation, the normal, hull and bounds |

Angles go through `varde_decide::num::trig` (re-exported as `trig`), the
same bits on every platform.

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
  `trig::sin`; `Conic::arc_between(center, r, a, b)` builds the same arc
  (under 180°) from its ends without them, by `+ − × ÷ √` only: with
  `m = a + b − 2·center`, `c = center + m·2r²/|m|²` and `w = |m|/2r`, in
  the plane or in space (the circle in the plane of `center`, `a` and
  `b`). Profiles from sketches are built with it. For an arc under about
  14° (`64·|a − b|² < |m|²`) the control point is worked out from the
  ends' middle, `(a + b)/2 + m·|a − b|²/2|m|²` (the same point, as
  `|m|² + |a − b|² = 4r²`; wider arcs keep the centre's form, exact for
  quarter circles about grid centres), so its rounding is the ends', not
  the centre's: from the centre, a nearly straight arc (a centre far off)
  carried the centre's rounding into `c`, which the circle's centre found
  from the curve again multiplies by `(2r/|a − b|)²`: a 10 mm arc about a
  centre 1e5 off was 2.6e-11 off in `c`, its centre 0.01 off.
- Weights below 1 give ellipse arcs, 1 parabolas, above 1 hyperbolas.
  Reversing a curve keeps `c` and `w`, so an edge record needs no
  direction.

**Back to the standard form.** A homogeneous curve with end weights `wa`,
`wb` becomes standard by scaling each barycentric coordinate by `1/√w`:
the points stay, and the middle weight `w` becomes `w / (√wa·√wb)`
(`Conic::from_hom`). That depends only on the curve's own three
homogeneous points and is symmetric in the ends, so both directions give
the same bits. The roots are taken before multiplying, so homogeneous
points of any scale work.

**Splitting** at `t` (`split`, `split_half`, and `piece(s, t)` between two
parameters, from the blossom) runs de Casteljau on the homogeneous points
and renormalizes each half. A coordinate all three control points share
(a curve in a plane square to an axis) is kept exactly in the halves and
in points of the curve (`Point::shared`): homogeneous division alone
rounds it. The halves' weights lie between the parent's and 1
(`w' = sqrt((1+w)/2)` at `½`), so they stay within bounds. At `½` the
computation is written symmetrically: the reversed curve gives the same
halves reversed and swapped, to the bit, and `split(0.5)` is
`split_half`. At other positions `1 - t` rounds differently, so a curve
shared by two users must be split once and the halves handed to both.

## Triangles

A `Patch` has corners `p[0..3]`, and edge `i` runs from corner `i` to
corner `i + 1` with control point `c[i]` and weight `w[i]` (edges 01, 12,
20). Corners run counter-clockwise seen from the side the normal points
to. Positions are barycentric `u = (u0, u1, u2)`, `u0 = 1` at corner 0.

The homogeneous **net** is symmetric: `H[i][i] = (p_i, 1)` and `H[i][j] =
H[j][i] = (w_ij·c_ij, w_ij)`. Then

```text
X(u) = Σ_ij ui·uj·H[i][j]          P(u) = X.xyz / X.w
```

Along an edge (`uk = 0`) this is exactly that edge's curve (`edge(i)`), so
two patches sharing a vertex pair, control point and weight share the
curve. `eval(u)` keeps the coordinates the control points share, as the
curves do. `normal(u)` is the cross product of the derivatives along `u0`
and `u1` (with `u2 = 1 − u0 − u1`) times the cube of the point's
homogeneous weight, worked out about corner 0 so its rounding is the
patch's, not the origin's. `hull()` is the six control points: with
positive weights the patch lies in their convex hull. `bounds()` is the
box around them. `Patch::flat(p)` is the flat triangle on three points.

## Limits and errors

| constant | value | why |
|---|---|---|
| `W_MIN`, `W_MAX` | `1/64`, `64` | edge weights; outside, conics hug their chord or control polygon |
| `MAX_CONTROL` | `1e7` | control point coordinates: 10× `MAX_COORD`, so every product stays finite |
| arc sweep | `0 < |θ| ≤ 90°` (+1e-12 relative) | one exact segment; room for rounding of equal parts |
| arc start | `|start| ≤ 16π` | keeps `cos`/`sin` accurate |

`Conic` and `Patch` have open fields, so `check()` is the trust boundary;
`new`, `from_hom` and every split or constructor end in it. `PatchError`
is `Coordinate` (not finite, or past `MAX_CONTROL`), `Weight` (outside the
bounds, or a non-positive homogeneous weight), `Parameter` (split position
outside `(0, 1)`, arc radius or angles), `Mismatch` and `Degenerate`.
`Bounds<P>` is the `f64` box (`Bounds2`, `Bounds3`); the `f32` `Aabb`
stays the renderer's.
