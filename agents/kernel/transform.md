# Transforms and assembly (`src/transform.rs`)

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
  composed, so no error piles up; the pattern feature passes `count` as
  the steps the span is split into: the count for a whole turn, one less
  for a part turn, a copy at each end, see `agents/features/pattern.md`).
  **Angles are in degrees**: `%` on
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
- `bits()`: its numbers' bits (`L`, `N`, the offset, the stretch, then
  the mirror and uniform flags as 0 or 1), what regen keys a moved body
  by: motions with the same bits move every solid alike.

Constructors give `None` for input that isn't finite, a zero axis or
normal, a factor out of range, or an offset that overflows (`k·spacing`,
`c − S·c`); points the motion
takes past `MAX_COORD` are refused when it is applied.

**Aligning** (`transform/align.rs`): `Motion::align(moved, target,
options)` takes each side's `Datum { point, primary, secondary }`
(directions any length; a secondary needs a primary, and the sides must
pair: both primaries or neither, both secondaries or neither) and
`AlignOptions { flip, offset, degrees }`. A point alone is the move
between the points (an offset or a turn without primaries is refused;
a flip is ignored). Otherwise each side's frame is Gram–Schmidt's: `f1`
the unit primary (the target's negated with `flip`), `f2` the unit
secondary less its part along `f1`, taken off twice, `f3 = f1 × f2`; a
secondary within `PARALLEL = 1e-9` (the sine) of its primary is refused
(`AlignError::Parallel`, "the second direction is parallel to the
first", the one decision on geometry). Without secondaries both sides
take `f_m × f_t` as theirs, the axis the smallest rotation turns about,
so it stays; within `1e-8` (the sine) of parallel or opposite that cross
product is lost in rounding and a direction square to the target's
primary takes its place, chosen as a sketch placement's x axis (world X
less its part along it within a nanoradian of world Z, else `Z × d`):
parallel, the rotation is the smallest to within the angle between them;
opposite, the half turn about that direction. `R = Σ t_i·m_iᵀ`, the
identity where the frames agree to the bit. Then the turn `T` about the
target's primary as given (`Motion::turn`'s matrix: quarter turns
exact), and one offset worked out from the one matrix: `x ↦ T·R·(x −
p_m) + p_t + offset·d_t`, `d_t` the target's primary unflipped (so a
positive offset is a gap either way the two face, and the turn goes the
same way round). Units divide by the length after dividing by the
largest coordinate, never multiply by an inverse, so a direction along a
world axis is that axis exactly; then frames are signed permutations, `R`
and quarter turns have only `0` and `±1`, and the moved point lands to
the bit wherever the offset's arithmetic is exact (to a few ulps
otherwise). `−0` becomes `+0`, so equal motions have equal `bits()`.
Points finite and within `MAX_COORD`, the offset within it, the turn
finite, directions finite and not zero (`AlignError::{Point, Options,
Direction, Unpaired}`). Tests (`transform/align/tests.rs`): every pair of
signed axes with and without flip (point and primary to the bit, a
signed permutation, the cross product fixed), quarter-turned frames with
secondaries exact, a box corner to corner face to face (every vertex to
the bit), 500 random tilted frames (point to `1e-12`, primary to `1e-15`,
secondaries' square parts agreeing, a rotation), frames that agree giving
the identity, flips exactly and nearly opposite or parallel, offset and
turn (quarter exact, 30° against `Motion::turn`), every refusal, and
determinism.

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
