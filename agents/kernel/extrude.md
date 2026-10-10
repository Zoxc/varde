# Profiles and extrude (`src/profile.rs`, `src/extrude.rs`, `src/extrude/`)

| file | holds |
|---|---|
| `profile.rs` | `Profile`, `Loop`, `Segment`, `ProfileError`, `Profile::check`, signed areas |
| `extrude.rs` | `Frame`, `extrude`, building the mesh, the walls' surfaces |
| `extrude/chain.rs` | the segments the solid is built on: classifying, cusps, separating |
| `extrude/cap.rs` | the caps' triangulation and the rounds that mend it |
| `extrude/cap/quality.rs` | refinement of the caps for quality and for crowding |
| `mesh/shape.rs` | the angle bound and circumcentre the caps' refinement and the boolean's shape points share |
| `profile/tests.rs`, `extrude/tests.rs` | profile builders (exact arcs without trig, circles, rectangles), shapes with analytic volumes |

## Profiles

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

## Extrude

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
over another conic, along the normal.

**Tapered extrudes** (`src/extrude/taper.rs`, not built yet):
`extrude_tapered(profile, frame, from, to, taper, feature, tol, budget)
-> Result<Solid, TaperError>` is a stand-in with its planned signature:
a zero taper is `extrude` exactly, any other fails with
`TaperError::Failed(NotImplemented)`. The plan: the walls the untapered
extrude's, drafted with the neutral plane the profile's plane (height 0)
and the pull along the normal on each side of it, so a positive taper
(radians, under a right angle either way, its sine and cosine from
`trig`) offsets the profile inward by `|h| tan taper` at height `h` on
both sides of the plane, a negative one outward; lines give planes,
circular arcs exact cones, other conics fitted walls; for a span across
height 0 each wall in two pieces split there (the ones behind
`BackSide { curve, segment }`, a face part not added yet); a span not
reaching height 0 turns its walls about their hinge on the plane all
the same. Refusals: `Closes` (narrowing, the walls meet before the
span's end: a narrow slot, a circle shrunk to nothing), `TooSteep`
(corners whose drafted walls don't meet in a point, a wall's form that
can't lean that far), `OutOfRange` (widening past `MAX_COORD`),
`Failed`. The planned analytic tests (pyramid and cone frustums, a
tapered slot's planes and cones, a pocket and a cut through a plate,
two sides and symmetric, a negative taper, the refusals, thread counts)
are in `src/extrude/taper/tests.rs`, `#[ignore = "kernel taper not
built"]`; see "Deviations".

The steps:

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
are refined for quality, with what it failed with plain (the sets
below are `VARDE_TESTS=full`'s; quick runs a smaller set of each, such
as one corner cut a corner and the first seed of the cut circles):

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
halved), the 16 384-gon's 78 560 (under `VARDE_TESTS=full`), and the
patches of `circles_cut_unevenly`'s circles (30 092, every fourth 7 152
in quick runs; 18 872 plain) and `random_plates_with_holes`' plates
(1 636; 1 150 plain) all told, pinned in those tests. The
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
