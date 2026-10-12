# Booleans: results, errors, tests and gaps

## Results that aren't manifolds

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

Two vertices at one place can also come from a zero-size handle that
the decisions of no single configuration build: a cluster of vertices at
one point whose star has one boundary loop (one fan, a manifold) but
Euler characteristic `−1`. A round's intersection with a bar whose seams
meet on the cut had 13 vertices at one point so, from first orders taken
from rounding (see "Ties" under the curved primitives); the clean-up
couldn't collapse them (`fan`) and the check failed on a fold. It is
named `NotManifold` too, wording only: a neck has two or more fans, a
handle one fan with `χ < 1`, which the star's link would tell apart.

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
at the first pair, which it hands back (the earlier vertex first): a
unit of work a vertex and one a vertex measured
against (points `d` apart fit about a hundred to 27 cells, so that's
bounded); `apart` is a union-find over the triangles' sides, a unit a
triangle. If either runs out of the budget, the error stays `Invalid`.
Whether there is a pair depends only on the positions; which pair, and
the work, on their order too. It isn't an early exit before
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

**Where it touches** (the evidence, see "Boolean evidence" under
"Limits, budgets and errors"). Named from a near pair, the failure keeps
what repair or the check named and adds the pair's two vertices as
points (one where they are at one place): on the shared edge or at the
shared corner for boxes. Named from separate shells, the two triangles
(or repair's pieces of them) are what it has. From the decisions, every
pair of the round that `pinched_line` refused, in order, gives its two
patches (`A`'s then `B`'s, pieces of the operands as refined that
round) and the names of the operands' faces they lie on, each once
(`pinch_failure`, whole pairs while the allowance and the caps last):
the walls along the line, which regen resolves to the bodies' faces. The error is the first refused pair's, as before:
the decisions of every pair are made anyway (in parallel), so naming
the others costs the operation nothing.

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

## Repair and the check near the change

Most of a boolean's result is its operands' triangles carried through
untouched, and the operands passed the check, so repair and the check
test those again for nothing (that was about 18 of the 25 units a
patch a small cut of a large body took). A triangle of the result is
**intact** (`mesh/intact.rs`, `tested`) when it is a triangle of an
operand that passed the check at the same resolution (`Solid` keeps it),
bit for bit: the same patch, corner for corner (so not turned over), on
corners mapping one to one onto that operand's (a map for each operand
built in triangle order, no vertex of the result the image of two; a
triangle that doesn't fit those before it isn't intact). Then its fold
check passes, and two intact triangles of one operand share the same
corners as they did there, so the hull rules decide them as the
operand's check did: they pass. The rest is tested: every triangle that
isn't intact, and the intact ones within the resolution of the box of
the other operand's intact ones (that box holds every one of them, so
no pair of the two operands' kept triangles whose boxes come that near
is missed), with every pair one of those is in.

Which operand triangle a result's triangle may be is a hint, carried as
`Soup::source`: assembly sets it for the triangles kept whole (`whole`;
for curved operands the refined triangle's leaf, if it is a root of the
refinement, whose id is the operand's triangle), merging sets it for a
restored root, and none for `B`'s in a difference (turned over); the
clean-up clears it for the triangles it changes (collapses, flips,
unbending and Delaunay flips, straightened seams, unfolding, the quality
pass's pieces). `tested` verifies it, so a wrong hint costs a test,
never a pass. `Solid::finished_near` tells the tested triangles on the
built mesh for repair (`Mesh::repaired_from`), and again on the
repaired, merged mesh, with the hints mapped through repair's origins,
for the check (`Mesh::check_counted_near`): the check doesn't trust
repair's bookkeeping. Both give the whole pass's result and error, as
they test the same failing triangles and pairs in the same order: the
same mesh, error and evidence as testing everything, for less work.
Debug builds also run the whole check on every boolean's result and
assert that the two agree. Checked by hashing 402 results and errors
(a drilling chain, plates with tools at corners and flush, random boxes
and cylinders chained, pin grids) against the operation as it was: all
the same bits.

The charges: a unit for every 16 patches (`SCAN`) each time the tested
triangles are told, repair's unit a patch and its pairs, a unit a patch
(`CHECK_WHOLE_WORK`) for the check's passes over the whole mesh
(topology, bounds, the boxes' tree, orientation, face tags: about a fifth
of the whole check's time) and `CHECK_WORK`, 5, for each patch it
tests.

## Errors and budget

`KernelError::Boolean(BooleanError)`: `Inconsistent` (the decisions
don't fit together: near ties taken as ties, with curved operands; flat
operands are decided again exactly then, see "Decided again exactly";
also a winding number out of `0..=1`, see "Counting"; or a
crossing the search only placed isn't on the other operand; or a
cut neither exact nor traced whose fallback curve isn't on the true
cut, see "Chains"; each with the edge, vertex, pair or arc it is
about, see "Boolean evidence"),
`Degenerate` (a face's loops
couldn't be triangulated, or the triangles don't pair up; each with
what it is about, see "Boolean evidence" under "Limits, budgets and
errors"), `NotManifold`
(the result would touch itself along an edge or at a point, or come
closer to itself than the resolution: see "Results that aren't
manifolds"; with the pinch or the faces touching, see "Boolean
evidence"). `TooComplex` (no evidence)
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
"Triangulating"; an exact orientation 4 units), the clean-up's unit for
every 16 triangles and one for each triangle or vertex its passes visit
or class again ("Work local to the change"), merging's unit for every 16
triangles and one for each triangle under a candidate each round,
repair's own
(near the change, see "Repair and the check near the change"), a unit a
patch naming faces of one surface alike (`Mesh::merge_faces`), and for
the check that makes it a solid a unit a patch and 5 for each patch it
tests near the change (the whole check is about 2.7 µs a patch;
`CHECK_WORK`), spent before it, plus 32 for each patch
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

## Costs

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
tables (`Input::new`, the refiner's pieces), the
BVHs, the clean-up, merging and the check's topology are sequential.

A unit of work is about 0.2 to 0.7 µs on one thread across these and
across booleans that fail, so `MAX_WORK` (about 4.2 million) lets the
heaviest of them through with room to spare and stops a failing one
within about two seconds on one thread.

**Whole-body costs.** Some of an operation's work is still over
everything both operands hold, whatever the boolean touches: every
refinement round rebuilds and counts both operands, the soup is built,
merging names faces over the whole result, repair bounds every patch,
the check's topology, orientation and face tags run over all of it, and
the clean-up's first pass classes every triangle (see "Work local to the
change"). Repair and the check's fold and hull tests work near the
change only (see "Repair and the check near the change"), and so do the
clean-up's passes and merging's rounds; the vertex normals are worked
out only where asked for. Measured in release (units by bisecting the
budget, to 1%), a 0.2 mm box cut at the corner of a plate of round
holes, away from them: 5 532 patches 128 000 → 47 000 → 21 600 units
(as it was → with repair and the check near the change → with the
clean-up and merging too), 19 180 patches 442 000 → 158 000 → 70 700,
39 388 patches 950 000 → 362 000 → 144 000 (about 24 → 8.5 → 3.7 a
patch; plates of square holes 27 → 9.5 → 3.3 to 4.2, a pin 3.8 to 5.3
but 11 on the smallest, 2 060 patches); the last of 100 holes drilled
one at a time into a plate (10 242 patches) 240 000 → 81 000 → 44 500
(23.4 → 7.9 → 4.3 a patch), the hundred 16.0 → 12.3 → 4.2 s of CPU; 60
holes drilled one at a time into a 20 × 20 × 1 box 4.97 → 2.46 → 1.57
million units all told, 5.8 → 4.3 → 1.6 s of CPU (the earlier holes, on
a small body, cost mostly what they cut). Of the CPU, most went to
dropping the empty components, which integrated every curved patch:
the box at the corner of the plate of 900 round holes, 0.83 s of CPU on
one thread, now 0.22 s. The results and errors are the same bits. So a
body past about 1 000 000 patches can take no boolean within
`MAX_WORK`.
A grid of many holes cut at once is another matter: a 100 × 100 × 5 plate
less a 10 × 10 grid of pins assembled into one tool runs out (7 × 7
takes 2.9 million units, 8 × 8 runs out), not for the whole body but
for the cut faces: the plate's two caps are two triangles each, every
hole lies in one of them, and each round of cutting the faces
triangulates each such triangle's loops again by ear clipping, which
grows faster than linearly in their vertices (7 × 7: about 90 000 units
for each cap triangle each round, seven rounds; 10 × 10: 2.2 million
for the first round alone). On
the web the regen worker is one thread, and wasm runs slower than
native, so a boolean that fails at the budget holds the worker for
several seconds; drags queue behind it (latest wins, but a running
operation isn't stopped).

## Tests

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
motion in a turned plane against its normal; directions and gradients
with rounding in their zero components, measured where seams meet on a
cut), `orient2d` near a line
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
fitted); a thin bar across a thicker round, on the world frame and on
a fuzzing case's hair frames after a chain of joins (each result's
volume within a tenth of the fit times its claim-free area, measured
at 0.016–0.020; the bands within the fit of their cylinders, measured
at 0.18 of it; see "Volumes with fitted cuts"); a bar of radius 0.032
grazing a unit round's wall, whose cut has conics of weight 30 (every
patch within the fit of the true surfaces on a grid of 32 steps a side,
1.007 of it before bands with heavy sides were measured on a finer
grid; see "Rounds"); a thin boss from a drilled box's side ending in
the hole, beside conics of weight 31 (on a 64-step grid, the union's
bands 0.16 of the fit off, 1.003 without the finer measures, each
result's volume against a Simpson integral, or refused); a pin through a
plate's hole wall (upright cylinders meeting in
lines, exact); a boss joined flush on a plate; a block through the
plate's hole, and one whose side runs exactly through a vertex of the
plate's caps (a tie); a round octahedron cut through its middle and with
a hidden loop; the saddle above and below its saddle point; tangent
cylinders (union not a manifold, the rest the operands); a plate drilled,
joined and drilled again, fed on; merging back a ball and slab refined
and not cut; the same bits at 1 and 8 threads; random turned bars
against boxes, 24 for each of three seeds under `VARDE_TESTS=full`
(4 in quick runs; 200 for each of four in an ignored test), each result exact (every patch on its surface to
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
within the fit tolerance (`fit·(area A + area B)/5`, the scale of what
fitted cuts keep: see "Volumes with fitted cuts"; a bound relative to
the volumes would flag fit error on thin bars) and analytic volumes
where known, and points
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
release, 588 of 600, at least 97%); perpendicular cylinders, and a box
less a hole along `y` against a hole along `x`, drawn from circles on
their frames' axis points and placed so seam rulings of the two meet on
the cut, on the world frame and turned and moved, against the crossing's
volume by quadrature and every patch sampled within the fit of the
solids' surfaces (147 of 160, at least 90%; 94 before the first-order
measure and the trace start were mended); the same bits at 1 and 8 threads.
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
to 72 of its 96 operations and left the others as they were; measuring
every tie in space then took it to 78.

**The near-tangent test's refusals.** 8 (fit, gap) settings × 3
placements × 4 operations; 78 of the 96 work. The test holds each
operation to a rule (`tangent_may_fail`) instead of a share: every
intersection and difference must work, and so must any union at a gap
of at least the resolution. The 18 that fail are unions with `|gap|`
under the resolution: at gap 0 and ±`1e-9`, ±`1e-6` at fit 0.1 and
`1e-9` at fit 0.01 the union touches along a line, or has a neck or
parts closer than the resolution, which no manifold at the kernel's
resolution holds: `NotManifold`, or `Inconsistent` (two at gap `1e-6`).
Right to refuse; the test allows but doesn't require it, so non-manifold
results or better ties won't break it. Until heights were measured in
space, 6 intersections and differences at gap −`1e-6`, fit 0.1, on the
two placements whose seams lie on the tangent line were `Inconsistent`
too: the walls lay on one surface within the resolution, and yet the
pair had ends, because the count's heights along `UP` read some 16
times the normal distance on a wall nearly parallel to `UP`.

No wrong `Ok` among them: every result passed the volumes, tags and
sampled points.

Cones and coaxial walls (`curved_tests/cones.rs`): a countersink (a
revolved hole of radius ½ widening at 45° to 1½) cut into a plate,
upright and on a frame turned and moved, all four operations against the
closed form to `1e-12`, every patch on its plane or quadric to `1e-12`
relative and none claim-free; half of one, the plate's side through the
axis cutting the cone in rulings; slabs tilted through the cone at three
angles; a box's face square to a frustum's axis cutting it in a circle
that the face's diagonal crosses anywhere (24 placements, all four
results to `1e-11` and every patch on its surface); a cylinder and a
cone crossing on one axis (under 200 patches
for their intersection); cones through a cylinder's top near its rim; a
turned shaft of a cylinder, a cone and a cylinder joined end to end (36
patches, one wall each), then a cone overlapping all three, a V groove
and a centre drill (its fitted tip within the fit), against closed forms
from the radii (`turned_volume`, exact for walls linear in height); 40
random pairs of coaxial stacks (a frustum and a cylinder each, upright
and on random frames, 160 of 160 operations, every volume within `1e-9`
relative, the upright ones exact); ring tops sloping from `3e-6` to 1
cut by a cylinder on their axis (the nearly flat ones may be refused);
the same bits at 1 and 8 threads; frustums opening at 80° to 89.5°
from their axis (nearly flat) cut by half and a quarter of a plate
through the axis along their seams, upright and a thousand from the
origin, exact; coaxial shortcuts that fail tried again (a stack a hair
off a cone's axis, against the closed form; a ring a hundredth outside a
cone's wall; a ball through a ring's cones). Unit tests: parallels' arcs on their
circles to `1e-14`, cone rulings from sections at the geometric mean,
bulging curves split, walls leaning further than a quarter of their
slope not certified.

## Known gaps

- **Seams meeting on the cut, what is left.** Of crossing cylinders and
  cross holes whose seam rulings meet on the cut (see the tracing in
  "Chains"), about one in ten operations is still refused: on the world
  frame the same refusals as with the seams elsewhere (`Hull`, `Fold`,
  `EdgeNeighbours`, a few `NotManifold`); on turned frames, cross holes
  where a hole's straight seam ruling touches the other's wall, whose
  tangency is solved as two crossings `1.1e-8` apart: the triangulation
  fans between them and the clean-up can't collapse the fan (`fan`),
  `NotManifold` or `VertexNeighbours` (the near-tangent class;
  `tangent_edges` collapses such pairs only along straight edges on
  plane faces). Far from the origin (moved by `1e3` and `1e4`) the
  turned groups work less often (crossing cylinders 40 and 33 of 60,
  cross holes 22 and 16 of 30), all `Invalid` on those thin triangles,
  none `Inconsistent`: the tangencies' noise over `ALONG_SIDE` there,
  which goes by its sign, refuses none of them (a share of `1e-4`
  decided the same). And at fit `1e-5` crossing cylinders' differences are
  refused `Inconsistent` about a quarter of the time wherever the seams
  are (43 of 60 work with the seams elsewhere): the trace arrives with
  three points and fitting fails, so the fallbacks aren't verified.
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
  hyperbolic cylinders and other quadrics (but a cone cut square to its
  axis and a sphere, whose circles take their angles' weights) a plane
  section's weight still
  comes from where the line from the chord's middle to the control point
  meets the quadric, noise for an arc bulging by a rounding, and an arc
  within `1e-9` of straight is a straight edge (up to `1e-9` of its
  length off). On a parabolic cylinder every section's weight is 1 (an
  affine image of a parabola arc), which isn't used yet. Of 800 random
  bars through boxes, two (seed 7, case 189; seed 1, case 97) fail their
  intersection and difference as `Degenerate`, as before.
- **Nearly flat cones cut on their axis are often refused.** A cone too
  nearly flat to claim its quadric (`Free`, its form the cone) cut by a
  cylinder on its axis takes the coaxial path by the forms, but its
  crossings aren't solved again on any surface and the operation often
  fails (a ring's top rising `3e-6` over 100 at a fit of `1e-4`:
  `Invalid(VertexNeighbours)`; rising `1e-4`: `TooComplex`, as both did
  before). Of 360 operations on such rings (rises `1e-6` to `5e-3` over
  100, fits `1e-4` and `1e-3`, pins and a cone on the axis), 167 work
  (8 before this step), none wrong. Sloping a hundredth, the cone claims
  its quadric and the cut is exact.
- **Thin coaxial wedges cost patches.** Where a cone's wall meets a
  cylinder's at a small angle in the result (a cylinder less a cone
  crossing it at 18°, a cone inside a cylinder's wall within a hundredth
  of it), the hull rules between the two walls' bands are kept by repair
  splitting them: 476 to 1 068 patches where the cut faces have some 100,
  13 912 for a cone from radius 0.9999 to 0.9 (6.5 s), about as many as
  traced before this step (13 220). A cone starting within a thousandth
  of the cylinder's wall and crossing it at a slope of a few thousandths
  is refused (`Hull`), as it ran out of budget before.
- **Coaxial solids a hair off one axis are mostly refused**: two turned
  stacks, one moved by `1e-7` to `3e-6` or turned by up to `4e-7`, work
  in about a quarter of the operations, as before this step (none wrong;
  their traced cuts keep the identities to the fit).
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
  resolution; curved operands still can be (2 of the seeded suite's 96
  near-tangent operations, 11 while heights were ties along `UP`).
  Three thresholds meet there: pieces are taken as planar within the
  resolution and `one_surface` holds within it, both in normal
  distance, and the count's tie is a 64th of the resolution, now a
  distance in space too (it was a height along `UP`, the normal
  distance over `|n̂·ÛP|`, about 0.06 on a wall nearly parallel to
  `UP`, so an overlap of about 0.06 to 1 tie distance passed
  `one_surface` and still counted as a crossing). Overlaps between the
  tie and the resolution can still give decisions that don't fit:
  near-tangent cylinders overlapping by `1.5e-6` at fit 0.1 (their seams
  on the tangent line), intersected, are `Inconsistent` at a vertex.
  Far from the origin the rounding rule's
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
  fail about 1 operation in 11 (9 %; 10.6 % before ties were measured in
  space, see "In-plane crossings"), polygons 12 %, cylinders 13 % (14 %
  before the curved path was decided again without ties). Ties
  measured along `UP` were about a seventh of those refusals, nearly all
  of those whose contacts lie within the tie. Most of the rest are faces
  between the tie and the resolution apart (hair frames reach some 22
  ties), decided by their real geometry and refused by the check as
  closer than the resolution; the rate rises with the gap, to about a
  quarter of the steps whose frames are more than 16 ties apart. A
  larger tie would take some of them. The curved path's `Inconsistent`s
  (2.5 % of cylinder steps) are decided again without ties now: about
  0.1 % are left, a third to a half of the rest come out right and most of the
  others are named for what they are (`NotManifold`, `Invalid`; see
  "Decided again exactly"). About 1 in 750 used to come
  out with a triangle facing against its plane tag (an in-plane crossing
  placed outside its triangle, see "In-plane crossings"); none does now,
  and `check` refuses any that would. What is left of the window: edges
  near a face's plane with neither end within the tie of it still have
  crossings placed up to well beyond the resolution outside their
  triangles (some 850 per
  12 600 hair-frame steps before edges with one end in the plane were
  kept in; none gave a false tag in the hunts), so do edges with one end
  in it and no part inside the triangle, planar
  patches with curved sides aren't `Input::flat` and are left alone, and
  on the curved path an edge missing its triangle fails as
  `Inconsistent` and is decided again without ties. Unit cylinders tangent along a
  line on a far turned frame, one's top a tie above the other's, gave a
  difference with a cap triangle against its form; it is now refused
  (`Invalid(EdgeNeighbours)`).
- **Caps in a plane through `UP`.** Cylinders whose caps lie in a
  plane containing `UP` (to within some `1e-5` rad: the rims' shadows
  are one line, or thin ellipses on it) are refused for most flush or
  overlapping operations, mostly `Inconsistent` after the retry without
  ties too: 1 211 of 3 456 right in a probe (frames with `x` along
  `RAY`, `y` along `UP`, two unit cylinders stacked, coaxial, offset or
  a hair apart within the cap's plane; 985 with heights along `UP` and
  no curved retry), none wrong. E.g. on the frame with origin
  `(0.3, −0.2, 0.7)`, `x = RAY/|RAY|`, `y = UP/|UP|`, circles round
  `(0.5, 0.2)` of radius 1 extruded `0..1` and `1..2`: the union is
  `Inconsistent`. Such planes need a normal square to `(2, 3, 32)`,
  which no sketch plane on the axes has.
- **A cylinder inside a box, tangent to a steep face within the tie.**
  The union (the box) is refused (`Invalid`, `NotManifold`) where the
  face's normal is far from `UP` (box from `cylinder_and_tilted_box`,
  `r` 1, the face `0.3` tie off the wall, `φ` 0.3 or 1): the box's
  edge and the cylinder's ruling, a tie apart in space, are a `Height`
  tie, decided as touching, which the curved searches don't see.
  Heights along `UP` decided them apart (11 ties) and the union was
  right. A gap or overlap of a tie between a box's steep face and a
  wall also runs out of budget for some operations (`TooComplex`).
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
      threads (under `VARDE_TESTS=full`).
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
- **Cross holes through a box**: two round holes through a box square
  to each other (a user's 2 × 2 × 2 box drilled from its front, then
  from its side by a hole crossing the first near its mouth;
  `a_cross_hole_near_another_holes_mouth`, in the kernel and through the
  history). Refining the drilled box where the holes cross reached its
  caps beside the first rim and bisected cap triangles there by straight
  edges leaving them (see "Refinement", Flat faces): every cut near the
  user's case was refused. With those split red, sweeps round the case
  (release, fit `1e-3` unless said; each result held to the analytic
  volume within its patches' integrated distance from the true surfaces,
  and every sampled point within the fit of one): radii and heights
  `± 0.02` 108 of 180 operations work (30 before; differences 52 of 60,
  none before), centres off the lines by up to 0.05 72 of 120 (25),
  radii 0.4–0.9 and heights `1 ± 0.08` 154 of 180 (142), fits `1e-2`,
  `1e-3`, `1e-4` round the case 46 of 90 (13); none wrong. Six
  intersections that worked before are refused now (176 won): the extra
  split moves the refinement near the rims, and these sit in the class
  left over. What is left: `Hull` between a small piece at the cut and a
  long band triangle from the cut to a far rim (as in "Long cap
  triangles and cuts passing close to their sides"), `VertexNeighbours`
  between slivers along the traced cut, `TooComplex`, and at fits
  `1e-4` and `1e-5` `Inconsistent` for the user's intersection and
  union. They grow where the holes' bottoms or tops are nearly level
  (their seam rulings nearly meet on the cut): with the nearer gap under
  0.08, 17–58% of operations are refused; over it, 6%. After the exact
  cuts on cones and coaxial surfaces, three of the six lost
  intersections are still lost (of the radii and heights sweep), two
  fail before the rule too, and one works again. All the cap triangles
  split for the rule there are ones the intersection throws away, so
  not splitting a leaf the operation discards (no pair, every corner on
  the side not kept, from the round's counts) would give the
  intersections back as they were before the rule; but the user's own
  intersection, refused before the rule on the walls, works only with
  it, so the rule stays as it is.
  A wider fuzz (scratch, two seeds of 150 cases: the drilled box with a
  cross hole, a slot (level or upright) across the hole, a boss on the
  front cap beside the rim, crossing it or clear, flush or sunk, or one
  from the side ending near the hole's wall; turned at random in 60%
  of cases, a third moved about 100 and a third about 10 000 from the
  origin; fits `1e-2` to `1e-4`; every operation at 1 and 8 threads,
  held to its analytic volume within `fit · (area A + area B) / 5` and
  the identities):
  700 of 896 operations work (663 before the rule; none lost), the
  same bits at 1 and 8 threads, every volume within 0.012 of that
  bound. Left over besides the class above: intersections with a slot
  across the hole `Invalid(Fold)` (64 of 124), and `Inconsistent` in 18
  of the 300 cases, 17 of them about 10 000 from the origin (of 93 so
  far out), turned or not (those tried near the origin work), all before
  the rule too. One union of a boss from the side whose end stands in
  the hole just under its top has a fitted band on the boss's wall with
  an edge of weight 31 straying `1.27·fit` from the cylinder, its volume
  and the identities right, before the rule too.
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

  Repros: `cargo test -p varde-kernel-compat --release
  near_tangent_cylinders_at_the_default_tolerance -- --ignored
  --nocapture` prints each operation's result, units and time (`NEAR`)
  and the list over a million units; the regen cases evaluate those
  documents with `evaluate` and a fresh `Cache`.
