# Volume, area and measuring (`Solid::volume`, `Solid::area`, `src/quadrature.rs`, `src/measure.rs`)

The volume is a third of `∫ (P − o)·n` over the surface (divergence
theorem, `o` the middle of the bounds), the area `∫ |P_u × P_v|`, each
patch integrated over its parameter triangle cut into its four half-edge
pieces, each by the 8 × 8 Gauss–Legendre rule through the collapsed
square (`u0 = s`, `u1 = (1 − s)t`, Jacobian `1 − s`): 256 points. A patch
with a weight outside `0.7..=1.4` is split (`split4`, up to five times)
first. A flat patch (every weight 1, every control point its edge's
midpoint, to the bit) skips the rule for its tetrahedron with `o`,
`a·(b × c)/6`, which the rule gives too, to rounding (`flat_flux`; the
integrand keeps one sign over a plane, so its size is the value's).
The tests hold it to `1e-12` relative on boxes, cylinders and
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
  tables' snap points). A flat face whose region borders exactly four
  chains, each straight (`edge_shape`), open and between two regions,
  making one loop of vertices, each side square to the next to a cosine
  of `1e-6`, is a rectangle (`measure::rectangle`, the `FaceMeasure`'s
  `rectangle`: two sides from one corner); a hole's rim or a side
  split where another face meets it makes more chains, and none. A
  straight edge's point is its
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
