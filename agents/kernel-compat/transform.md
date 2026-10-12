# Motions (`src/transform.rs`, `src/transform/align.rs`)

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
  cos)·k·kᵀ` about the unit axis, offset `point − L·point`. Both
  make the direction unit by dividing it by its largest coordinate,
  then by its length, so one along a coordinate axis is that axis
  exactly whatever its length;
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
`c − S·c`).

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
secondaries exact, 500 random tilted frames (point to `1e-12`, primary to `1e-15`,
secondaries' square parts agreeing, a rotation), frames that agree giving
the identity, flips exactly and nearly opposite or parallel, offset and
turn (quarter exact, 30° against `Motion::turn`) and every refusal.

`Solid::transformed(motion, copy, tol, budget)` moves the empty solid to
itself and fails with `KernelError::NotImplemented` for any other (the
facade has none); `assemble(parts, tol, budget)` gives the empty solid
for no parts or only empty ones, and the same error otherwise.
`FaceKey::copy(feature, index)` renames a key as `FaceName::copy` does
(see `mesh.md`, "Names").
