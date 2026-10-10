# Booleans: cutting and assembly

## Cutting curved faces (`boolean/chain.rs`, `boolean/surface.rs`, `boolean/assemble/`)

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
  into the quadric patch (Newton's method from between the arc's ends'
  places in the patch, else from its middle: from the middle alone it
  strayed on the long thin patches of a nearly flat cone, half-angle
  89.5° from radius ½ to 20½, and a ruling through its axis was traced
  as a chord paced as a cylinder's, the triangles beside it `3e-7` off
  the cone, volumes `2e-6` off at 88°). If the guided arc gives none (halving ran
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
- **A plane through a cone's apex** cuts it in rulings: the section's
  straight case (both tangents along the chord) on a quadric whose
  gradient, affine along the chord, keeps its direction and vanishes
  beyond both ends (`surface::ruling`: at `s = −g₀·h/h·h`, `h = (A +
  Aᵀ)·(y − x)`, parallel to `g₀` to `1e-9`, within `MAX_APEX` = `10⁶`
  chords) is the cone's ruling, its control point at the geometric mean
  of the ends' distances from that apex (`cone_ruling`), which every
  triangle on the cone with that side needs; with the midpoint (a
  cylinder's ruling, what it was) half a countersink cut through its
  axis had four triangles `1e-5` off the cone on copies claiming no
  surface, volumes `1.2e-5` off. A cylinder's `h` is a rounding, which
  can point anywhere square to the axis, as `g₀` does: past `MAX_APEX`
  the segment.
- **A plane square to a cone's axis** (to `1e-12`), **or any plane
  through a sphere**, with the quadric patch on its form within the
  resolution: the section is a circle round the line through the apex
  or centre along the plane's normal, whose arcs come from their angles
  (`coaxial::square_axis`, `coaxial::parallel`, below), as an elliptic
  cylinder's do, kept if their points at `¼`, `½` and `¾` invert into the
  patch within the resolution; else the section as below.
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
- **Two quadrics of revolution on one axis** (`chain::parallel`; cylinders,
  cones and spheres by their faces' forms, each patch on its form and the
  axes one within the resolution: `coaxial::common_axis`; a sphere turns
  about the other's axis where its centre is that near it, two spheres
  aren't taken) meet in parallels. An edge of either patch from end to
  end lying on both forms (to `1e-12` of the coordinates: two turned
  walls joined end to end, a cylinder's rim on a cone of its radius
  there) is the cut, whole (`along_edge_on`); else, with the ends'
  heights and distances from the axis within the resolution of each
  other, the parallel's shorter arc between them (`coaxial::parallel`):
  weight `cos(Δφ/2) = |r̂x + r̂y|/2` and control point `m + tan²(Δφ/2)·(m
  − m̂)` (`r̂` the unit directions from the axis to the ends, `m` the
  chord's middle, `m̂` its foot on the axis, `tan²(Δφ/2) = |r̂x − r̂y|² /
  |r̂x + r̂y|²`), halved at the turn's middle direction (the ends' mean
  height and distance) past about 45°; kept if its points at `¼`, `½`
  and `¾` invert into both patches within the resolution (the argument
  of `chain::verified`: the material between it and the true cut is no
  thicker than a few resolutions). Exact, so its bands are built by the
  common-point construction; and either patch's curved edges lying on
  both forms get vertices at the cut's (`parallel_extras`), as a flush
  rim's do. Before, such cuts were traced and fitted.
- **Anything else** (quadric against quadric, `Free` faces): **traced**
  and **fitted** (`chain/trace.rs`). A point where the patches meet solves
  `P(u) = Q(v)` and lies on a given plane: four equations in `u0, u1, v0,
  v1`, Newton's method with Gaussian elimination (partial pivoting), at
  most 40 steps, accepted within `1e-11` of the patches' size. Marching
  starts at the `+` end along `n_P × n_Q` turned into both patches
  (`inward_sign`): of the sides the end lies on (barycentric coordinates
  within `1e-9` of 0, in either patch: two at a patch's corner, more
  where the end is on both patches' boundaries), the way that leaves none
  of them, which enters one; where neither way leaves one, the step along
  all of them within `ALONG_SIDE = 1e-6` of its largest component (the
  cut tangent to every side), the way towards the arc's other end; where
  both leave one, or the end is on no side, the old rule, the smallest
  coordinate of the patch the end was found on. Both ways leave a side
  only where the end lies on a side of its own patch and within `1e-9`
  of one of the other's that the cut leaves beside it: in the sweeps
  only at tiny arcs at fit `1e-5`, ends `5e-9` to `2e-6` apart along the
  tangent, where the chord's way decided the same operations, so the old
  rule (the side the end's crossing lies on) stays. That rule failed where
  seam rulings of two crossing cylinders meet on the cut: refinement puts
  a corner of a patch on the point, the cut is tangent to the seam
  there, and the first zero coordinate may be the seam's, across which
  the step is rounding (`6.3e-16` against `−1.14` across the corner's
  other side); or the end lies inside a side of one patch tangent to the
  cut (a straight ruling touching the other wall), and on a side of the
  other patch the rule never looked at; or the end is a tangency's double
  root placed some `1e-8` of the size off the touching point, the step
  across the side `7.2e-9` of its length (a sign, but noise: at a share
  of `1e-9` this one still failed). The trace started out of the patch
  and the arc was `Inconsistent`. The chord's way is the arc's at its
  start unless the arc turns back by more than a right angle there
  (fitting halves arcs turning past 45°, and the rounds split long
  ones); if it did, the trace would fail, an error. Ends within the
  march's first step of each other (about a hundredth of the pair's
  size) it joins at once, whichever rule picked the way, if the other
  end is ahead: the short way. A hairpin is caught by the fit (a plane
  nearly tangent to a cylinder cuts it in a parabola a unit deep whose
  ends are 0.009 apart: joined at once, no conic along the two
  tangents follows the curve, the trace fails and the fallbacks are
  checked against the true cut;
  `a_hairpin_whose_ends_are_within_a_step_is_followed`). The long way
  round, an arc turning back by more than a half turn to end beside its
  start, would pass, the short way being on the curve too; but a pair
  that can hold it isn't certified: with normals within cones apart
  round axes `a1`, `a2` (each under 90°), the projections of `n_P` and
  `n_Q` onto the axes' plane are never parallel, so `(n_P × n_Q)·(a1 ×
  a2) > 0` along the whole arc, its ends are apart along `a1 × a2` by a
  share of its length, and it can't come back beside its start. Coaxial
  walls' arcs are exact parallels, walls along one direction meet in
  lines, and pairs at the size floor are a few resolutions across. In
  the sweeps below every
  arc the chord rule started had its chord within 19° of the tangent
  (cosine at least 0.95), and a thin cylinder grazing a big one with a
  seam ruling touching it (the cut a closed oval touching that seam)
  came out right or refused. `ALONG_SIDE` against the tangencies' noise
  (sides' steps over the largest, measured over those sweeps): the
  double roots read up to `4e-6` near the origin, `5e-6` at fit `1e-5`
  and `2e-5` at `1e3`–`1e4` from it, where real crossings start at
  `1.5e-5`; those over `1e-6` go by their sign, as the old rule took
  them, and a share of `1e-4` decided the same operations there. So the
  share is kept; it catches the double roots near the origin, the case
  that needed it. With this rule and
  the first-order measure in "Ties", perpendicular cylinders drawn from
  circles on their frames' axis points with seams meeting on the cut (a
  fuzz of 320 operations a group, release; before them, once one tie
  measure and its exact retry had come in) went 133 → 294 on the world
  frame, 187 → 276 turned and 161 → 286 turned a hair, the control
  (seams elsewhere) 283 either way; cross holes through a box 109 → 140,
  110 → 134 and 124 → 134 of 150; random perpendicular pairs 865 →
  1 003 of 1 200; no wrong `Ok`, every identity within the fit times
  the operands' area over 5 (worst 0.032 of it). The rules alone lost 14
  operations that worked without them (refusals on the cut's thin
  triangles, mostly on turned or hair frames); the operation is tried
  again without the rules there ("Tried again without the seam rules"
  under "Pairs of faces"), and with that the sweeps (also at fits
  `1e-2` to `1e-5`) win 724 operations against the build without either
  rule and lose none. Marching then steps `h` along the tangent and corrects on
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
- **Volumes with fitted cuts.** A result with a fitted cut has its
  volume within the fit tolerance times its bands' area (the patches on
  claim-free faces), not to rounding: the rounds halve a band's cut
  while the band strays more than half the fit tolerance from its
  face's surface, and the round kept has every band within the fit
  tolerance at the samples, a finer grid where it came near (see
  "Rounds" below). Each conic is fitted
  only until it is within a quarter of the fit tolerance at its three
  samples, and lies in the crease's bisecting plane, so the conics bow
  to one side of the true curve, and the bands beside them with them.
  Both sides of a cut follow the same conic: `A ∪ B` keeps `B`'s wall
  outside `A` and `A ∩ B` its wall inside, both sag the same way, so
  `|A ∪ B| + |A ∩ B| = |A| + |B|` misses by the sum of the two results'
  errors, not their difference (a difference has its own conic, the
  crease turning with `B`). A bar of radius 0.25 across a round of 0.75
  at the default fit: every result 2–3e-5 off, the identity 4.7e-5,
  every patch within `1.8e-4` of the true surfaces, every volume within
  0.02 of the fit times its claim-free area
  (`a_thin_bar_across_a_round_is_within_its_bands`). The promise is the
  fit times the bands' area; the tests pin a tenth of it, as they do the
  volumes of crossing cylinders. The bands they hold only to the fit
  tolerance and the vertices to the fit too: a round may stop with a
  band anywhere under it, and a vertex a round adds on a band (not on
  the cut) is only on the band. Since the error
  grows with the cut's length and the bands' width, not with the
  volumes, a bound relative to the volumes is wrong for fitted cuts: a
  thin bar makes a long cut for a small volume. Fitting the cuts
  tighter (a sixteenth of the fit at the samples) cut the error 4× and
  cost 1.8× the patches; the fit tolerance is the user's setting, so it
  is not done.

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

**Curved sides bulging over the loops** (`triangulate::bulging`, on
planar layouts only: a curved patch's domain doesn't hold its curves'
bulges so): the triangulation works on the chords, so a curved side of
the loops whose bulge holds something of them, a vertex strictly inside
the triangle from its ends to its middle (inside the region between the
curve and its chord) or a straight side crossing into it, asks to be
split, as a closed corner's curve does. A curved side's chord crossing
another's bulge doesn't count (its own bulge holds that one's vertices,
and it is split). A cylinder's top cut by a coaxial cone in a circle
nearer the rim than a quarter arc's chord (radius 0.87 in 1) left a ring
whose rim chords cross the circle; it was fanned from a rim vertex past
the circle's tangent point, the circle halved there round after round
(`Invalid` in 4 of 12 such operations once the walls' pairs were
certified rather than refined). The seeded tallies didn't fall (flush
unions 59 → 60 of 60, flush operations 210 → 213 of 240, related 113 →
114 of 120).

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
operation fails as `TooComplex`. Two refinements of the samples
(`face::measured`, `Cutout::finer`):
- A triangle with a side of weight `w` over 2 is measured on a grid of
  `4·⌈√w⌉` steps a side (at most 32) instead of the 15. Such a side pulls
  the patch towards its control point over a strip about `1/√(2w)` of
  the domain wide, and its own points at `¼`, `½`, `¾` all crowd round
  the control point. Fitted conics reach weights of 20–32 where a cut
  turns sharply (a thin bar grazing a round, a cone crossing a
  cylinder's wall near a vertex); the bands beside them strayed up to
  1.03 of the fit off their cylinders between samples that found half
  that, a wrong `Ok`
  (`a_thin_bar_grazing_a_round_keeps_its_bands_within_the_fit`). On the
  finer grid they halve as any band: the fuzzing cases came out within
  0.3–0.5 of the fit, none refused that went through before.
- The round kept is measured again where a triangle strayed more than
  an eighth of the fit tolerance: on a grid of at least 16 steps a side.
  Bands of light weights strayed up to 1.7 times their 15 samples' worst
  (a ridge beside the cut, a ball's band), so a round that ran out of
  halvings with a band just under the fit at the samples could be past
  it; such a round fails as `TooComplex`. A round that stopped because
  nothing strayed past half the fit has its bands within about 0.85 of
  it, and this changes nothing there. The grid is still samples: the
  promise is the fit at them, not a certified bound. Halving can't bring every band within
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
into the way, a round later, so the rounds run until none is demoted.
One pass over the triangles (a unit for every 16) finds those under a
candidate and counts, for each of their corners, the triangles under no
candidate and the candidates with it as a corner; each round then looks
only at the triangles under a candidate (a unit each), and a demotion
moves the triangles under it to a child that is a candidate, or to none
(counted then). Demotion only ever takes triangles out from under
candidates, so the counts stay what flags over every triangle would
give, and the candidates come out the same (debug builds run the rounds
over every triangle too and assert it). On 48 000 refined triangles the
rounds took 14 s with sets, then 20 ms with flags over every triangle
each round.

## Assembly (`boolean/assemble.rs`)

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

## Triangulating a face's loops (`boolean/triangulate.rs`)

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

Loops that can't be triangulated fail the face, and the round, as
`Degenerate` (the first face in order that fails; a meter past its
limit, which also stops a triangulation with `Degenerate`, is
`TooComplex` first). Its evidence is the face's kept halfedges in
their sorted order, each as its curve record where it has one, else
straight, between its vertices' positions, and the operand's face by
name (`face::boundary_failure`, run on the failing face again after the
round: the halfedges are worked out by the same `boundary` as
`cut_face` uses, so they are its loops; positions rather than the layout put back, which differs only by
the rounding and snapping onto the domain's sides). Reached in the
seeded flush pairs: a wall left with loops of two vertices, along one
curve there and back, which no triangle takes and which aren't asked
to be split (on a curved patch only a cut's are).

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

## Clean-up (`boolean/cleanup.rs`)

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
  goes over the vertices in id order (only those that may fold: see
  "Work local to the change" below), a unit each (on a plate with 64
  square pockets, 8 flush bars and 8 holes it took under 1 % of the
  clean-up). Chained grid boxes (random half-grid
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

**Work local to the change** (`cleanup/index.rs`). Each pass above
changes something only at a triangle (or a vertex) of a class its own
state and its neighbours' tell, and asked of any other it says no and
changes nothing: the collapses' edges are sides no longer than `thin`
(`SHORT`); the flips' triangles no higher than it (`FLAT`); the seams'
triangles (straightened, unbent, triangulated again) have a curved side
whose neighbour across lies in their face's plane (`SEAM`); the
Delaunay flips need a sliver on a plane face, in the triangle or one
sharing a side with it on that face (`SLIVERS`); a closed curved corner
needs a plane face and a side with a record in the curves (`BENT`); and
a folded star a vertex whose triangles are all on plane faces, two of
them facing apart. The classes are worked out once for every triangle
and vertex (a pass over the corners, and one over the curves' records
for the classes that need a curved side: a seam also needs the far
corner of the triangle across within the short length of the plane),
charged a unit for every 16 triangles. Every change goes through a
triangle whose corners, face or sides' curves changed, or that was
taken out or made (`touch`: in `swap`, a collapse's triangles and all
round the vertex kept, straightened seams, unfolding's new faces,
merged faces, regions and stars triangulated again, the quality pass's
pieces), and before a class is read again every living triangle with a
corner among those triangles' corners, and those vertices, are classed
again: that reaches each triangle that shares a side with a changed
one. A triangle given other corners (`set_tri`) also has its old ones
classed again (`touch_before`), so that doesn't rest on its pieces
covering them. A record left where an edge went (no triangle has it)
changes no class, and is the only kind a pass that says no takes out;
a region's triangulation that fails after taking out a record some
triangle still has (one turned against the region's boundary) touches
that triangle. Each pass
then visits its class in id order as it is at each step (a triangle
joining it past the one being visited is visited in that round, as the
pass over every triangle would), the collapses' lists are made from the
`SHORT` triangles, the quality pass's from the triangles the boolean
made (and its closed corners from `BENT`'s), and each member is asked
what every triangle was. So the same changes happen in the same order,
and the results are the same bits, for a unit for each triangle or
vertex visited or classed again instead of the soup's size each round.
Debug builds assert, after the rounds and before the components are
dropped, that the classes are what working them out afresh gives.

A flip or a collapse classes a few dozen triangles again, so a round
that changes a tenth of the soup costs more than visiting all of it,
and clean-ups that flip back and forth all over the soup for every
round up to the last came to ten times its size a round (a plate of
round holes united with its square-holed version cut by it, at a fit of
`1e-4`: `TooComplex` where visiting every triangle gave its
`NotManifold`, in twice the time). So in the two loops of rounds, once a
round has classed again more triangles and vertices than the soup has
triangles, the passes visit every triangle and vertex from there to the
loop's end, as with no classes (the same questions in the same order),
charged a unit a triangle a round, and the classes are worked out afresh
(a unit a triangle) before they are next read (`Cleaner::adaptive`).

Integrating every curved patch to drop the empty components took most of
a boolean's time on a curved body (four 8 × 8 rules a patch, about
30 µs; 0.76 s of a 0.2 mm cut on a plate of 900 round holes). Each
component is now first told by its corner triangles' volume less the
cones over its curved sides' lunes, which is off what integrating gives
by no more than the patches' `lune_bound`s, the quadrature's error
(`QUADRATURE` of the integrand's scale, bounded by the patch's distance
from the component's root vertex times sixteen times its control net's
area) and rounding, as the orientation check tells a shell's sign
(`mesh/orient.rs`). Only a component within that of the short length
times its area is integrated, in the same order as before, so the same
components go (debug builds integrate them all and assert the
verdicts). That pass is 17 ms on that plate.
