# Revolve (`src/revolve.rs`, `src/revolve/`)

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
(`1e-5` too under `VARDE_TESTS=full`; quick, each shape at one fit and
turn by turns) through the same checks, under a ceiling of about
twice the measured patches (the triangle 304), the same bits at 1 and 8
threads; the same crease profiles turned fully and cut by a box through
a crease's ring, a cylinder round the axis through it and a drill across
it, all three operations (under `VARDE_TESTS=full`; quick, the first
three with the box), each result checked with its tags and the volumes adding up (72
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
