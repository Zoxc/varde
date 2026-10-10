# Booleans (`src/boolean.rs`, `src/boolean/`)

`boolean(a, b, op, tol, budget)` gives `a ∪ b`, `a − b` or `a ∩ b`
(`Op::{Union, Difference, Intersection}`) as a `Solid`, and `touches(a,
b, tol, budget)` whether two solids meet, running only the broad phase
and one counting, with curved patches followed by a search for surfaces
within the resolution where the counting shows nothing (see "Touches"),
and nothing, answering false, for solids whose boxes are more than the
resolution apart, so asking it of far bodies is cheap. They follow
Manifold's `boolean3.cpp` and
`boolean_result.cpp`: every topological fact comes from a few
primitives, each worked out once and stored by the pair it is about,
through identities that hold whatever values the primitives take, so the
result is a closed manifold by construction and nothing is ever merged
because two points are close.

For **flat patches** (every edge straight within the resolution,
`hull::flat`; such a patch is taken as the triangle on its corners) the
primitives are exact and every cut is a straight segment. With a
**curved patch** in either operand the primitives are numerical (see
"Curved primitives"), pairs of faces are decided by certificates and
refinement (see "Pairs of faces"), and each decided arc is cut along a
chain of shared edges: exact where two planes or a plane and a quadric
meet (the faces' tags say), traced and fitted within the fit tolerance
elsewhere (see "Cutting curved faces").

| file | holds |
|---|---|
| `boolean.rs` | `Op`, `BooleanError`, `UP`, `Cross11`, the `Primitives` trait, `boolean`, `touches`, building the mesh (and what its builder refused, `built_evidence`), naming failed results that touch themselves (`pinched_named`, `pinched`, `apart`), `parts` (connected parts, for the rays and the clean-up) |
| `boolean/input.rs` | `Input`: an operand's tables (corners, edges' ends and triangles, boxes, patches, which edges are straight and which patches flat or planar), vertex normals, flat volume |
| `boolean/curved.rs` | `Curved`, the primitives with curved patches: ray-derived shadow crossings, layers above a vertex, crossings of an edge through a patch, ties |
| `boolean/curved/ray.rs` | the ray tests `ρ` (exact for straight edges and at every edge's ends) |
| `boolean/curved/arcs.rs` | where two edges' shadows cross: one conic written implicitly, the other put in, a quartic |
| `boolean/curved/solve.rs` | points of a patch above a vertex, an edge's crossings through a patch, and a certified distance to a patch: subdivision and Newton |
| `boolean/curved/bernstein.rs` | Bernstein polynomials: products, evaluation, root isolation |
| `boolean/pairs.rs` | each pair of faces' ends and arcs; for curved operands the certificates, the refinement loop (`refined`) and the fixed rules; the pairs touching along a line a union is refused for (`pinch_failure`) and a pair whose ends don't join (`pair_failure`); `crossing_of`, which crossing a new vertex id is |
| `boolean/near.rs` | `touches`' search for surfaces within the resolution: pairs of patch pieces split depth first until their hulls are apart or both are flat (`search`, `settled`, `near`) |
| `boolean/exact.rs` | exact signs: `Approx` (float with an error bound), `Exp` (expansions), `Poly` in `ε`, `Pred`, `sign`, `orient2d` |
| `boolean/flat.rs` | `Flat`, the primitives of flat operands, with the symbolic perturbation |
| `boolean/count.rs` | broad phase, the stored primitives, `x12`/`x21`, winding numbers; what an edge or vertex that doesn't fit shows (`edge_failure`, `vertex_failure`) |
| `boolean/count/tests.rs` | the counting's `Inconsistent` failures from primitives made to disagree (`Doubled`), and what they show |
| `boolean/evidence.rs` | `Gather`: the evidence of a boolean's own failures as it is gathered (an operand's edges, triangles or pairs of them with their faces' names, faces by name alone, points); its tests' `on_operands`, which the seeded suite runs on every `Inconsistent` |
| `boolean/surface.rs` | the exact paths: what a patch lies on (`Shape`), crossings solved again on planes and quadrics, a plane's conic on a quadric (`section`), a cone's rulings (`ruling`) |
| `boolean/coaxial.rs` | quadrics of revolution on one axis by their forms (`common_axis`): parallels as exact arcs (`parallel`), circles where a plane is square to a cone's axis or cuts a sphere (`square_axis`), the coaxial walls' certificate and their ends joined round the axis (`walls`, `along`) |
| `boolean/chain.rs` | each arc's chain of shared edges: straight, exact, or traced and fitted; halving its curves |
| `boolean/chain/trace.rs` | the point where two patches meet (Newton on four unknowns), marching along the cut, fitting conics, inverting a point into a patch |
| `boolean/assemble.rs` | new vertices, kept pieces of edges, cut edges, the rounds of cutting the faces, the faces' copies |
| `boolean/assemble/face.rs` | one face cut: its layout, loops, curved sides, triangles, their inner edges' curves (exact bands on quadrics); the boundary of one that can't be cut (`boundary_failure`) |
| `boolean/assemble/merge.rs` | merging refinement's pieces that came through whole |
| `boolean/triangulate.rs` | a face's kept loops in its parameter domain into triangles, curved sides' corners and bulges, Steiner points |
| `boolean/cleanup.rs` | collapsing and flipping the degenerate triangles flush operands leave |
| `boolean/cleanup/seams.rs` | curved edges between two triangles in one plane: straightened, regions triangulated again, the plane faces they joined merged (for the sliver flips after them) |
| `boolean/cleanup/fold.rs` | sheets folded onto a flush face, their two sides triangulated differently: the folded vertex moved within its star's planes; its tests on a box whose top is folded |
| `boolean/cleanup/quality.rs` | refining the plane faces the boolean cut for their triangles' shapes |
| `boolean/tests.rs` | boxes in every flush, edge-on and vertex-on configuration, tori, determinism |
| `boolean/curved_tests.rs` | cylinders and boxes (exact), crossing cylinders (also with their seams meeting on the cut, or nearly on a frame turned by a hair; cross holes so in `curved_tests/holes.rs`), a free surface, a saddle, extrudes, chains, merging, random bars, walls over arcs with level ends |
| `boolean/curved_tests/flush_seams.rs` | flush unions with curved rims in either order: bosses in and on plates, over holes and edges, overlapping, a flange at a shaft's foot, a slot, at millimetre scale and on a turned frame, a chain of flush joins, caps a hair apart, bosses on a rounded corner |
| `boolean/curved_tests/cones.rs` | cones and coaxial walls, exact: a countersink upright and turned, half of one (rulings), slabs tilted through a cone, a box's face square to a frustum's axis with its diagonal across the circle, a cylinder and a cone crossing on one axis, cones through a cylinder's cap, a turned shaft joined end to end and cut by a cone, a V groove and a centre drill, random coaxial frustums against closed forms, ring tops sloping down to nearly flat, nearly flat cones cut through their axis, coaxial shortcuts tried again |
| `boolean/curved_tests/one_face.rs` | faces on one surface after booleans: tops at a crease either side of the bar, flush stacks on turned frames far from the origin, chains of joins and cuts with every operand's names resolving, faces meeting only at a corner |
| `boolean/curved_tests/tangent.rs` | tangent contacts: cylinders against a plate's side from outside and inside, standing on it or through its top, slots ending in, beside and across a hole, a cylinder on a cylinder (in millimetres at the default tolerance, and at unit size at the finest), unions touching along a line refused at once (a pin plugging a hole it touches inside never named so), and solids tangent to a rounded edge or the faces it runs into; volumes against analytic ones relative to the operands' where cuts are exact, within a tenth of the fit times the claim-free area where a cut is fitted |
| `boolean/seeded_tests.rs` | the seeded random suite: related pairs, parts built in chains of twenty, turned solids, near tangencies, pins and coaxial cylinders, flush bosses, bosses sunk through drilled plates, seams meeting on the cut |

## The primitives

One fixed projection direction for everything, `UP = (2, 3, 32)`: nearly
`+z`, tilted off every axis so walls along the axes don't all project to
lines, with small integer coordinates so exact predicates take it as it
is. Heights, "above" and "below" are along it; "seen from `+UP`" is the
projection.

- `s02(v, f)`: the signed number of layers of face `f` of the other
  operand above vertex `v`: +1 where `f` faces up (`n·UP > 0`), −1 where
  it faces down, 0 where the ray from `v` along `UP` misses it. A flat
  triangle along `UP` (its projection a line) gives 0. For curved patches,
  hits where the patch folds over in projection cancel in pairs, so the
  signed count only changes where the vertex's projection crosses the
  patch's boundary or the vertex passes through it, as for flat ones.
- `s11(e, g)` (`Cross11`, for an edge `e` of `A` and `g` of `B`, each in
  its own direction): how their projections cross, as the two sums the
  counting reads. `a_under` sums over the crossings where `e` passes
  under `g` the sign `σ` of `det[g, e, UP]` (+1 where `e` crosses `g`
  from its right to its left, seen from `+UP`); `b_under` sums `−σ` (`g`
  crossing `e` from its right to its left) over those where `g` passes
  under `e`. Flat edges cross once at most, so one of them is ±1 or both
  0; projected conics can cross up to four times, and only the sums
  matter.
- `crossings(side, e, f, x)`: the crossings of edge `e` through face `f`
  whose signed number the counting gives as `x` (below): each one's sign
  and where along the edge it is (a position only, never a decision).
  Their signs add up to `x` whatever a search finds: the count wins.
  `searches(side, e, f)` says whether the pair is asked with `x = 0` too
  (an edge that may pass through a face and back: a curved edge, or a
  curved face).
- `order(side, e, c1, c2)`: the order of two of an edge's crossings along
  it.

Each is asked once per pair and stored in a sorted table (`count::Table`,
by `(vertex, face)` or `(edge of A, edge of B)`), which everything that
needs it reads. An edge's direction is that of its lower halfedge.
`Flat` (below) implements them exactly; `Curved` (see "Curved
primitives") for operands with curved patches.

## Exact predicates and the perturbation (`boolean/flat.rs`, `boolean/exact.rs`)

For flat operands the primitives are exact: signs of `3 × 3`
determinants and plane sides of the input coordinates. Ties, which CAD
geometry makes on purpose (flush faces, a vertex on a face, edges
meeting), are broken by **symbolic perturbation**: every vertex of `A`
moves by `ε·s·n_v + ε²·T2 + ε³·T3` for an infinitely small `ε`, with
`n_v` its direction out of `A`, `s = +1` for a union (`A` grows: flush faces overlap and
merge) and −1 for a difference or intersection (`A` shrinks: flush faces
cut cleanly), and `T2`, `T3` fixed generic translations for what the first
order leaves tied. `B` stays. This is Manifold's "expand P" rule made
into a real configuration: the perturbed operands are a genuine
arrangement in general position, so every decision is true of it and
the counting identities hold without exception.

`n_v` leaves by every triangle round the vertex (it is on the outer side
of each one's plane) wherever such a direction exists, so every face of
`A` moves outwards (or inwards) as a whole and flush faces part cleanly:
the normalized sum of the triangles' unit normals when it does, else the
axis of the smallest cone round their normals (`patch::smallest_cone`,
for up to 16 different normals). The sum alone fails at thin corners
(two faces nearly folded onto each other) and left slivers of zero
thickness where such a corner's face lay flush on the other operand.
At a saddle (faces round the vertex facing opposite ways) no direction
leaves by all, and the sum is kept: some face there moves the wrong way
and a flush contact leaves a sheet of zero thickness folded onto the
surface, which the clean-up takes out.

Each vertex's `n_v` is worked out when a decision first asks for it
(`Input::vertex_normal`, kept in a `OnceLock` a vertex by
`Flat::perturb`), from its own triangles: found round it through the
halfedges from one each vertex keeps, then taken in triangle order, so
the sum is the same bits as working out every vertex's from all the
triangles in order (`each_vertex_normal_is_the_one_from_every_triangle`).
Only `A`'s vertices near `B` are asked for; before, every refinement
round worked out all of `A`'s, a list of normals for each vertex.

Each predicate is a polynomial in the points' coordinates, and so in
`ε`; its sign is that of the first coefficient that isn't zero. The
constant term is first evaluated in floating point with a running error
bound (`Approx`: each `+ − ×` adds its operands' bounds, their products
with the other's value, and half an ulp of the result, inflated a
little), which decides almost every sign without allocating. Only when
the bound can't (a tie or a near one) is every coefficient worked out
exactly with floating-point expansions (`Exp`, Shewchuk's two-sum and
Dekker's two-product, no fused multiply-add) as polynomials (`Poly<Exp>`).
All of it is `+ − ×`, correctly rounded, so the signs are the same on
every platform. Dekker's product is exact only clear of underflow, so
coordinates and perturbation components below `2⁻¹⁰⁰` are taken as zero
(`exact::FLUSH`): products of up to six such numbers (the degree of
`Between`) then stay exact. Without it, points a `1e-160` apart gave
signs that disagreed with the same determinant asked in another order.
A predicate zero in every power (only if `T2` and `T3`
happen to lie in the tie's degenerate directions) takes a fixed sign.

The predicates: `Orient` `det[q − p, r − p, UP]` (which side of `p → q`
the point `r` projects), `Height` `det[a − c, d − c, b − a]` (with `σ`,
which edge is above at a crossing: the point of `e` is `λ` above that of
`g`, `λ = det[a − c, g, e] / det[g, e, UP]`), `Reach` `(t0 − x)·n` (the
side of a triangle's plane `x` is on, and the numerator of where a line
from `x` meets it), `Across` `(x1 − x0)·n` (its denominator) and
`Between` (which of two planes an edge meets first). A vertex is above
a face's plane along the ray when `(t0 − x)·n` has the sign of `n·UP`;
it projects into the triangle when it is on the interior side of all
three edges.

**Near ties are ties** (`exact::sign_tied`). The deciding predicates
(`Orient`, `Height`, `Reach`, and the ray tests' `Beside` and `Ahead`)
take a constant term within the **tie distance** (a 64th of the
resolution, `boolean::tie`) times the predicate's `scale` (how much it
changes per unit of distance from its tie: `|UP × (q − p)|` for
`Orient`, `min(|g × e|, TIES·|det[g, e, UP]| / |UP|)` for `Height`
(never below that product's own rounding, `4ε·|g|·|e|`: see "At every
order"), the triangle's normal's
length for `Reach`, `|RAY|·|(d − c)·ACROSS|` for `Ahead`) as zero, and go
on to the perturbation's powers. The scales make the tie one distance
for every predicate, the one the curved primitives use for heights (a
review found `Height`'s 32 times larger and `Orient`'s and `Ahead`'s
larger along steep or ray-wise edges), and one direction: **each tie is
the distance in space between the things the predicate is about**.
`Reach` measures a point's distance from a plane; `Orient` and the ray
tests a vertical line's from a line (the shadows' sides); `Height`'s
value is `(a − c)·(g × e)`, the lines' distance times `|g × e|`, so
with that scale it ties lines within the tie of each other, square to
both. Two edges in a common plane, each within the tie of it, then tie
for `Reach` and `Height` alike (see "In-plane crossings" for what
measuring `Height` along `UP` did). Capped: `TIES·|det[g, e, UP]| / |UP|`
is the scale of a height of a resolution along `UP` (`TIES` = 64 tie
distances), so a `Height` tie also needs the edges within the
resolution of each other along `UP` where their shadows cross. That
keeps every tie within the broad phase's margin (see "Counting"), and
edges whose shadows are nearly parallel (their common plane nearly
along `UP`), near in space but a resolution or more apart in height at
the crossing, are decided by their height as `Orient` decides their
shadows' sides; measured on hair frames, the cap changed one
result in 13 000 steps. A tied `Height` (scale at most `|g × e|`, so
the lines within the tie of each other) stands for a configuration
moved by under the tie: `e` moved along the lines' common normal by
their distance meets `g`, and every `Orient` of the four ends not
tied is more than a tie from its sign, so the moved shadows still cross
inside both edges. The perturbation then decides by `Height`'s later
orders, the same polynomial at points under a tie away. Only the
rounding floor ties lines further apart, where they are parallel to
rounding. So a configuration within the tie
distance of a tie is decided as the tie it stands for: faces flush in
exact arithmetic but turned and moved, every coordinate rounded, merge
or part cleanly as the unmoved ones do (of 96 random flush grid boxes'
operations turned and moved, 95 now work, 71 did with exact signs), and
the exact predicates and the curved primitives, which decide heights
that close as ties too, see one configuration: a cylinder `1e-9` off a
box's face was beside it for the exact tests and on it for the
numerical ones, and the winding numbers disagreed (then `InsideOut`,
now `Inconsistent`). The
positions (`Across`, `Between`, `crossing`) stay exact. It is used for
flat operands too.

**At every order.** Once the constant term is taken as a tie, each later
coefficient that is only rounding is taken as zero too. At the
exact tie a rounded configuration stands for, the first order is often
zero as well, and the tie is decided by `T2` or `T3`: two collinear
edges' `Height` (both first-order terms have parallel columns), a
vertex whose `n_v` lies in the other face's plane (on the edge between
a `+x` and a `+y` face, against a `z` face) for `Reach`. Rounded, that
coefficient is some `1e-16` of its terms, its sign noise, and taking
it decided each predicate as a different nearby configuration, so the
counting's ends didn't pair up (`Inconsistent`). Skipped, every
predicate is decided as the exact tie is: one configuration. Only
rounding means within what moving every number the coefficient is worked
out from by a share of itself can make of a zero: `exact::Moves`
evaluates `Σ |∂c/∂x|·|x|` over those numbers to first order (each
product's other factor weighed by its value, in floating point, only on
that slow path), and the share is `RHO = 2⁻³²` but at most the tie
distance over the largest of them, so no number moves by more than the
tie distance and the decision is still that of operands moved by less
than it, also far from the origin. (A first measure, the expression on
its terms' absolute values, grew as `|x|²` where the coefficient grows
as `|x|·L`: `2¹⁷` from the origin it took a vertex's direction 70° into
a face, or an edge `1e-3` above a parallel one, as rounding, and the
second order decided them the other way, and of the turned pairs below
moved by up to `1e5` or `1e6` it refused some 390 of 2 700; with
`Moves`, they come out as moved by up to 100, and its tallies here and
the seeded suite's are the same as the first measure's. `2⁻⁴⁶` decided
the same as `2⁻³²`.) With every later order only
rounding the sign is 0, as for a tie in every power. `sign` (tie 0)
stays exact. Before and after (release; turned grid boxes as in the
tests, seed 31, and chains of up to six, seed 77, each result fed on;
unturned grid-box chains, seed 5):

| run | before | at every order | and parallel shadows |
|---|---|---|---|
| 900 turned pairs × 3: right, `Inconsistent`, `Invalid` | 2 638, 13, 49 | 2 651, 2, 47 | 2 653, 0, 47 |
| 100 turned chains: right, `Inconsistent`, `Invalid` | 334 of 356, 12, 10 | 349 of 359, 0, 10 | 349 of 359, 0, 10 |
| 300 unturned chains of six: right, `Invalid` | 1 754, 46 | 1 754, 46 | 1 754, 46 |

No result was wrong. Of the `Invalid`s, 45 before and 46 after are
operations whose exact result isn't a manifold (one of the 13
`Inconsistent`s was too, now `Invalid`); the one other is pair 866's
union, a thin triangle lying across two faces that fails the hull
rules (a numerical oracle deciding every predicate on the configuration
moved by a real `ε` fails it too: not a sign).

**Parallel shadows.** `Height` is of second order where two edges are
collinear: its constant term, `det[g, e, UP]` and every first
derivative vanish there. Turned and rounded, `det[g, e, UP]` (its
scale) is only rounding, down to an exact 0, and with a zero scale
`sign_tied` fell back to exact signs, so the constant term's own
rounding decided (pair 100 of the turned pairs, its intersection and
difference `Inconsistent`, the last two above). Now `Height`'s scale
is never below that product's rounding, `4ε·|g|·|e|` (shadows within
`1e-15` of parallel, where a height at their crossing means nothing),
and a zero scale with a positive tie no longer drops the tie: only an
exact zero is then one, and the later orders that are only rounding are
still skipped. The first order needs no scale of its own (a separate
`ε¹` tie for `Height` was planned): with the rounding rule above every
manifold result of the 900 pairs is right but pair 866's.

**Shadows along each other.** Where both ends of `e` are on `g`'s
shadow to within the tie (`exact::is_tie` on their `Orient`s), where
the shadows cross and the side `σ` are the perturbation's, and the
configuration as decided has `e` in the plane through `g` along `UP`:
`det[g, e, UP]` is zero there, and so is `Height`'s constant term
`det[a − c, g, e]`, whatever the gap between the edges. `e_above` then
takes that constant term as a tie (`exact::sign_past_tie`), and the
perturbation decides by the gap. Taken as it came (the gap times only
rounding, or times a hair's angle), against a side that wasn't its
own, it put the edges either way round however far apart, and the
decisions still fitted together: turned grid boxes, the second moved
along `UP` off flush so that their flush edges' shadows lie on one line
(seed 5, 1 000 pairs × 3, also a million from the origin), gave a
union `1.8e-5` apart `4e-6` short and an intersection `2.8e-4` apart
`1.4e-5` over: wrong results, from before the rule at every order (the
retry never saw them). Now none is, and with the retry turned off none
is `Inconsistent` past the resolution. Edges stacked exactly along
`UP`, a direction off every axis, are rare in CAD geometry, but not
impossible.

**Decided again exactly.** Near ties taken as ties can still give
decisions no one configuration has, mostly where things are about the
tie distance apart. For flat operands (`boolean::flat_soup`; `touches`
the same for its counting) an `Inconsistent` from the counting, the
pairs' ends or the assembly is decided again with `tie` 0: exact signs
are those of the perturbed operands, a real configuration, so they fit
together, and the worst outcome is `Invalid`. The second try spends
from the same budget, only on that failure (`boolean::tied_or_exact`).
Operands with curved patches are decided again the same way
(`boolean::curved_decided` with `tie` 0, `touches` the same for its
one counting): the numerical primitives and the `Flat` inside
`Curved` together, so the two kinds still share one tie, now none.
That isn't a real configuration (the numerical heights go by their
signs as rounding has them), but no tie pulls the exact and the
numerical decisions apart, which is what most curved `Inconsistent`s
were: a box face a fraction of a tie off a cylinder's ruling, nearly
along `UP` (its normal 86° from it), has the box's diagonal and the
ruling tied in height (under a tie apart in space, 11 along `UP`),
decided as crossing, which the search through
the wall (the diagonal outside it) can't place; without ties neither
crosses. Measured on hair frames (cylinders seeds 201 and 202, mixed
300; each result fed on): `Inconsistent` 120, 81 and 143 → 3, 4 and 7,
right 4 048, 2 620 and 5 680 → 4 099, 2 654 and 5 722 (refused 679,
431 and 1 009 → 619, 406 and 959), most of the
rest now named for what they are (unions touching along a line
`NotManifold`, slivers `Invalid`), no wrong volume and no identity
past a hundredth of its bound; the near-tangent cylinders at the
coarsest tolerance (unions within the tie, below) refused as
`NotManifold` rather than `Inconsistent`. A retried cut can be traced
and fitted where the tied one was exact (a box face `1e-5` off a
cylinder's rulings, the box less it: 4.1e-9 off on a sliver of
3.3e-7, within the fit). (Planned as no retry, the `Flat` inside
`Curved` keeping the curved primitives' ties: see Deviations.) Turned grid boxes with one moved by `10^±1.5` tie distances
(seed 5, 3 000 operations): 11 `Inconsistent` with the ties (60 before
heights were measured in space, 100 before in-plane crossings, below,
74 before the flat broad phase's margin and crossings of edges with one
end in the plane), of them 9 right and 2 unions refused once retried
(`NotManifold`, `Invalid`), none `Inconsistent` and none wrong; 2 679
of the 3 000 right (2 629 with heights along `UP`, 2 617 before
in-plane crossings).

**The exact retry inside the shortcut retries.** An operation with
curved operands can be tried up to three times with fewer shortcuts
(every shortcut, then joins without the coaxial certificate, then
none: see "Tried again without the joins"), and each of those tries
decides with near ties and, on `Inconsistent`, again with `tie` 0
(`boolean::assembled` wraps `curved_decided` in `tied_or_exact` with
the try's shortcuts). So the order is: every shortcut tied, every
shortcut exact; then, if the operation took a shortcut and failed,
fewer shortcuts tied, fewer exact; and so on. The exact retry is the
inner one because its cause (ties that don't fit together) is the
narrower one, and decided within the same shortcuts it changes only
the ties: the shortcut retries then see that try's last failure, as
they did before, and run only where it still fails. The shortcuts
some pair took in either the tied or the exact try count as taken
(`curved_decided` adds them to `used`), so a tied try that failed
with a shortcut is still tried again without it. Both inner tries
spend from the outer try's work: the first from the operation's
budget, the later ones within their caps (what is left, or the work
so far but at least `AGAIN`), so the inner retry at most doubles a
try's decisions and never lifts the bound on the operation's work.

**In-plane crossings.** The tie was one distance, but not measured
the same way everywhere: `Reach` measured it square to the face's
plane, `Height` along `UP`. On a plane at angle `θ` to `UP`, two edges
a gap in `(tie·cos θ, tie)` apart square to it were tied for `Reach`
and not for `Height`. Flush faces a hair apart (boxes extruded on frames
turned and moved by `1e-11` to `1e-7`) put gaps there all the time: an
edge of `A` with both ends within the tie of a face of `B` (decided as
lying in its plane) still crossed it by the counting, the edge passing
under one of `B`'s by `Height`. `Height` now ties as the lines'
distance in space (see "Near ties are ties"), which removes the cause
for edges within the tie: such events (an edge with both ends' `Reach`
tied against a flat face whose shadow crosses a side of it with
`Height` not tied) fell from 31 418 to 43 in 12 600 hair-frame grid
steps, and from 145 356 to 4 on cylinders (counted with an instrumented
copy when the change was planned). The keeping below stays for
what is left: edges near the plane but not within the tie, and the
windows between measures (`Orient` across the shadows, heights past
the resolution along `UP`); it still empties some 42 intervals per
12 600 such steps (108 before). Before that change, the counting's
crossing of an in-plane edge went as follows. `crossing` gave where
the edge's line meets
the plane as rounding has it, anywhere along the line, here beyond the
edge and clamped to an end half a unit outside the triangle crossed. The
loop through two such crossings wound the wrong way, and the result
(the volume right, the release check passing) had the tool's side
filling the target's flush face, facing against its own plane tag
(about 1 operation in 750 on such frames; debug builds panicked in the
form check). So for an edge decided to lie in a flat face's plane (both
ends' `Reach` tied, `exact::is_tie`; only with a tie, and only against
a flat patch, `Input::flat`), `Flat::crossing` keeps the crossing to
the part of the edge inside the triangle seen along its normal, each
side `a → b` widened by the tie (`((b − a) × (x − a))·n ≥
−tie·|b − a|·|n|`, linear along the edge; `Flat::inside`). The vertex
moves along its own edge, so it stays on the faces beside the edge, and
within the tie of the crossed plane, so on its patch to the resolution;
only positions change, in floating point. If no part of the edge is
inside, the decisions fit no configuration: `crossings` gives
`Inconsistent`, which is decided again without ties (exactly on the
flat path). Without the widening, edges along a
triangle's side gave empty intervals from rounding (9 to 15 more
refusals per 4 000 turned chains). Measured on random chains of 5
steps, each result fed on (release): grid boxes on hair frames, 4 000
chains at each of three seeds, went from 22 to 26 results with a
triangle facing against its tag to none, with 41 to 51 more `Ok`s of
about 11 000; polygons on hair frames (2 500 chains) from 23 to none
and 15 more `Ok`s; cylinders on hair frames (1 500) from 8 to none and
2 fewer `Ok`s (`Inconsistent` 272 → 260); turned frames 6 and 7 more
`Ok`s; plain frames the same. No result had a wrong volume, and the
check's facing test refused none of them: these crossings were the only
source seen. Against the booleans' refusal of a triangle facing against
its form (which turned the same results into `Invalid` errors, 13 to 15
per hair grid seed), the keeping wins 56 to 64 `Ok`s per hair grid
seed, 25 on polygons and 21 on cylinders (`Inconsistent` 178 → 148),
and the seeded tallies are the same. The release check's facing test
(step 6 of `check`) backs this up wherever else a plane patch faces
against its tag.

Measuring every tie in space (`Height` square to both edges, capped at
the resolution along `UP`, and the curved heights alike), on the same
hunts (5-step chains fed on, release, the identities checked for
cylinders and prisms): refused manifold steps (the
cells' result a manifold; every cylinder or prism step counts) went

| run (chains) | refused, along `UP` → in space | `Ok`s |
|---|---|---|
| grid hair 101 / 102 / 103 (4 000 each) | 1 303 / 1 272 / 1 345 → 1 122 / 1 099 / 1 152 (10.6 % → 9.1 %) | +159 / +167 / +162 |
| grid hair 104 (2 000) | 654 → 575 | +81 |
| polygons hair 300 (2 500) | 1 111 → 1 017 (13.1 % → 12.1 %) | +61 |
| cylinders hair 201 (1 500) | 746 → 679 (15.7 % → 14.4 %); `Inconsistent` 140 → 120 | +38 |
| mixed hair 300 (2 000) | 1 088 → 1 009 (16.3 % → 15.1 %); `TooComplex` 28 → 11 | +81 |
| grid turned 101, grid plain 101 | the same bits | |

No result had a wrong volume or a triangle facing against its tag, and
every identity held within the fitted cuts' allowance `fit·(area A +
area B)/5` (the worst at 1.3 % of it, the same step before and after).
The seeded suite's tallies are the same but the near-tangent test's, 72
→ 78 of 96. The curved primitives do no more work: on the cylinder
hunt, 400 chains took 12.0 s against 14.6 s.

An edge with one end decided as on the plane (its `Reach` tied) and
the other within the resolution of it is nearly along the plane too, so
rounding still moves its crossing far along it: polygons on hair frames
gave a crossing 0.17 outside its triangle that way (the other end
`2.1e-8` off a plane, the tie `1.56e-8`), and the tool's side, touching
the target from outside, came out on the target's side facing against
its tag (refused by the check). Its crossing is kept inside the triangle
as well, where some part of the edge is; where none is, it stays where
rounding has it rather than fail (the curved path had no retry then). On hair
frames (with the flat broad phase's margin, see "Counting") that won
back 30 more `Ok`s over 11 runs (grid boxes +3 to +7 a seed of 1 000
chains, polygons +1 to +6 of 600, mixed +6, cylinders −1), refused none
for facing against a tag, and changed nothing on turned frames.
Putting the crossing at the tied end instead (the decided
configuration's place for it) did as well on mixed and polygon runs and
worse on grid boxes.

The seeded suite's tallies didn't move but for one step: the rule at
every order took chains from 203 to 202 of 240 (of its fed-on steps
the same inputs give two more right and one fewer,
`Invalid(VertexNeighbours)`; the rest follow from different results
fed on); parallel shadows, the retry and `first_sign`'s rounding rule
(which skipped 173 orders in the suite) changed none (related 112 of
120, chains 202 of 240 with 6 `Inconsistent`, turned 156 of 160,
tangent 72 of 96 with 11, coaxial 37 of 40, bosses 64 of 64, drilled
160 of 160). So curved operands, which share `sign_tied` through the
`Flat` inside `Curved` and the rays' `Beside` and `Ahead`, aren't
worse.

## Counting (`boolean/count.rs`)

1. **Broad phase**: a BVH over each operand's patch boxes; the pairs
   (triangle of `A`, triangle of `B`) whose boxes meet (`≤`, so touching
   boxes count: `A`'s boxes looked up in `B`'s BVH), counted against the
   budget before they are collected (`Bvh::hits_within`). The margin is
   the primitives' (`Primitives::margin`): 0 for exact ones (the flat
   primitives deciding again exactly, `tie` 0), the resolution for
   curved ones, which take edges and patches as straight or planar
   within it and decide heights within a 64th of it as ties, and the
   resolution for the flat ones deciding near ties as ties too.
   So every pair such a decision touches is counted: with margin 0, a
   cylinder's seam vertex `1e-9` from another's wall (at the coarsest
   tolerance) was decided as on it by its ray, no pair of triangles met
   to carry a crossing out, and the whole cylinder came out inside the
   other. The flat primitives had margin 0 after they took near ties as
   ties, and boxes at a hundredth of the grid's size off the origin,
   chained, left vertices of one an ulp inside the plane of the other's
   side through its middle: decided as on it and, by the perturbation,
   beyond it, their edges crossed that side, but the triangles on its
   near side had boxes an ulp short of it, so nothing counted a crossing,
   and the intersection came out empty and the difference whole, both
   `Ok`. A tie of `Reach` or `Height` is a tie distance at most in
   space, and a height tie reaches the resolution at most along `UP`
   (`Height`'s cap, and the curved heights'), but a shadow's along
   `UP`, on a face steep to it, reaches further, so the margin is the
   resolution, as for curved ones. From the pairs the
   candidate edge–face pairs of each operand, sorted.
2. **Layer counts** for each end of a candidate edge against the face,
   and for the first and last vertex of each connected part of an
   operand against every face of the other that a ray up from it may
   meet (the ray's
   box, which runs to the top of the other operand's box, looked up in
   the other's BVH with `Bvh::hits_within`: `pairs_within` for any
   query box).
3. **Edge against edge** for each candidate edge against the edges of
   the candidate face.
4. **Crossings** by Manifold's identity: for an edge `e` of `A` from `a`
   to `b` and a face `f` of `B`,
   `x12(e, f) = s02(b, f) − s02(a, f) − Σ S(e, h)` over `f`'s edges `h`
   as `f` runs them, where `S(e, h) = σ` when `h` passes above `e`
   (`a_under`; and `−σ` for `h` running the other way). Walking along `e`, the signed
   number of layers of `f` above changes by one each time `e` passes
   through `f` (entering `B` through it: +1) and each time `e`'s shadow
   passes under an edge of `f`, crossing it from its left to its right
   taking one away whichever way `f` faces. `x21(g, f)` for an edge of
   `B` through a face of `A` is the same with `S'(g, k) = −σ` when `k`
   (of `A`) passes above `g` (`b_under`). Then `crossings` turns each
   pair's count into **crossing records** `(edge, face, i)`, `i`
   counting along the edge, each with its sign `x` and position `t`
   (`count::Crossing`): the new vertices. A flat triangle and a segment
   meet once at most, so for flat operands any count but −1, 0 and 1 is
   `Inconsistent`; a curved edge and patch may meet several times, and in
   and out again where the count is 0.
5. **Winding numbers**: at each part's first vertex the sum of its layer
   counts (every layer above, signed, is the winding number of the other
   solid round it), and from there along the edges, each changing it by
   its crossings; then every edge is checked to agree (`Inconsistent`
   otherwise, never seen), the part's last vertex's own ray to give what
   its edges carried there (`Inconsistent` otherwise: a near tie decided
   at the first vertex against the crossings would put the whole part on
   the wrong side), and every winding number to be 0 or 1
   (`Inconsistent` otherwise: the operands are solids, whose winding
   numbers `check` makes 0 or 1 everywhere, invariant 5, so any other
   number means decisions that don't fit together, such as side-by-side
   cylinders nearly tangent). The operands' orientation isn't tested
   here or anywhere in the boolean: every `Solid` passed `check`.

Consequences, from the counting alone: a vertex's winding number and
its edges' crossings agree, and for every pair of faces (`p` of `A`, `q`
of `B`) `Σ x12(e, q)` over `p`'s edges equals `Σ x21(h, p)` over `q`'s
edges (each as its face runs it): the difference is minus the number of
signed crossings of the two projected boundaries, which is zero for two
closed curves. So each face pair's cut has as many ends going in as out;
for flat triangles, exactly one of each or none.

## Curved primitives (`boolean/curved.rs`, `boolean/curved/`)

With a curved patch in either operand, `Curved` gives the primitives:
numerical where what they are about is curved, exact (through `Flat`'s
predicates, `A` perturbed the same way) where it is straight or flat. An
edge is **straight** when its control point is within the resolution of
the line through its ends (`hull::straight`), and is then taken as that
segment; a patch is **flat** when its three edges are, and taken as its
corner triangle; a patch is **planar** when its control points are within
the resolution of its corners' plane (caps with curved edges are), and
which side of it a point is on is then decided exactly against that
plane. Each is judged once per edge or patch, so every primitive sees the
same.

**Why the shadow crossings are derived.** The counting's identities (a
face pair's ends balance; windings agree along edges) hold whatever the
values of `s02` and of the split of a crossing between `a_under` and
`b_under`, as long as for every pair of faces the signed number of
crossings of their projected boundaries is zero, as it is for two closed
curves. Crossings of curved edges found pair by pair can't promise it: one
near a shared vertex may be counted by both edges there, or neither. So,
as Manifold derives its edge crossings from shared vertex decisions, the
signed number of crossings of the shadows of `e` (from `a` to `b`, of
`A`) and `h` (from `c` to `d`, of `B`) is derived from **ray tests**:

```text
I(e, h) = ρ(b, h) − ρ(a, h) − ρ⁻(c, e) + ρ⁻(d, e)
```

`ρ(v, h)` is the signed number of times `h`'s shadow crosses the ray from
`v` along `RAY = (3, −2, 0)` (square to `UP`, square to no axis), +1
where `h` crosses it going left (towards `ACROSS = UP × RAY = (64, 96,
−13)`); `ρ⁻` the same for the ray behind the vertex. Walking a point `x`
along `e`, `ρ(x, h)` changes by `σ` where `x` crosses `h`, by `+1` where
`x` crosses the ray behind `c` going left and by `−1` for the ray behind
`d`, whatever way `h` runs there; that is the formula. Summed over the
edges of a face `q` the end terms cancel and what is left is the change,
from `a` to `b`, of `ω(v, q) = Σ ρ(v, h)`, the winding number of `q`'s
shadow's boundary round `v`, which sums to zero round a face of `A`. So
the balance holds by construction, for any values of `ρ`. Each `ρ` is
about one vertex and one edge of the other operand: which side of the
ray's line an end of the edge is on is exact (`(c − v)·ACROSS` with `A`'s
vertices perturbed, never 0, and asked as the same polynomial whoever
asks, so antisymmetric); a straight edge is exact throughout; a curved
one's shadow crosses the line at the roots of a quadratic in Bernstein
form whose end coefficients take the exact signs (so the number of roots
has the right parity), and whether each is ahead of `v` is decided in
floating point.

**`s11`.** `I(e, g)` as above. For two straight edges its crossing (one
at most) is above or below by `Flat`'s exact `Height`. Otherwise the
crossings are solved for (`curved/arcs.rs`): the curvier shadow (by
`|det M| / (|H0|·|H1|·|H2|)` of its homogeneous control points) written
implicitly as `λ1² = 4·λ0·λ2`, `λ = adj(M)·X` with the adjugate's rows
`H1 × H2, H2 × H0, H0 × H1`, the other put in to give a quartic in its
parameter (a straight shadow is its line, and gives a quadratic), roots
isolated in Bernstein form (Descartes' rule, halving by de Casteljau,
bisection; `curved/bernstein.rs`), the other parameter back from the
ratios of `λ`, kept if in `[0, 1]`. Each crossing's height difference
splits it into `a_under` or `b_under`. If their `σ` don't add up to `I`,
the missing ones go by the heights where the two edges' ends come closest
to the other (where they must be).

**`s02(v, f)`.** A flat triangle: exact (`Flat`). A planar patch:
`ω(v, f)` where its plane is above `v` (exact), 0 where not. A curved
patch: `ω(v, f)` when `v` is below all its control points, 0 when above;
otherwise the points of `f` straight above and below `v` are solved for
(`solve::hits`: the parameter triangle split by blossoming, pieces whose
control points' shadows' box misses `v` dropped, Newton on the shadow's
two coordinates in pieces whose normals all lean one way along `UP`; a
point Newton's method finds from one piece in another is kept too, if
it is in the triangle and the search ran out of pieces: on a steep
patch every piece's shadow holds the vertex's, the search ran out
before it got to the one holding the point, and the winding number's
point, taken to be above, gave every edge at the vertex a crossing
through the patch that wasn't there; where the search didn't run out,
that piece finds its own, and a second find near a fold, where Newton's
method converges loosely, would count it twice; and of those found from
other pieces, one facing a way is kept per point, those within `1e-4`
of the parameters of one already kept, facing the same way, merged:
seen nearly along a wall, the line along `UP` grazes it over a stretch
where the shadow is within Newton's residual of the vertex's, and each
piece's start ended somewhere else along it, ten finds `1e-9` apart),
their facings made to add up to `ω(v, f)` (a shadow covers a point as
often as its boundary winds round it, counted by facing) by adding the
nearest found just outside the triangle or dropping those inside nearest
its sides, and those above counted.

**Crossings.** A straight edge through a planar patch: at most one, at
`Flat`'s exact position on the corners' plane. Otherwise solved for
(`solve::edge_patch`: edge and triangle split together, pieces dropped by
their boxes, by a slab along the patch piece's normal, which a
tangency needs, and where their control points' hulls are apart
(`hull::apart`, GJK, in the search's frame scaled to the unit box; the
edge's and the patch's pieces lie in their control points' hulls, the
weights being positive), which a tall or long patch needs: a cap edge's
box held the whole width of a drill's wall 1 000 tall and 4 wide, so
only the height pruned pieces, and the row of pieces at the edge's
height was split across the width until the search ran out of pieces,
more than 1 024 against 23 to 33 now. "Apart" is by more than `1e-12`
(far above the rounding of the blossomed control points) plus
`2048 · PIECE_SLACK` (about `2e-6`) of the two pieces' sizes: Newton's
solutions count for a piece up to `PIECE_SLACK` outside it in its own
parameters, and one just past the patch's side or the edge's end (a
hit the count may want, see below) lies that far outside the hulls at
the weights' bounds; with `1e-12` alone, a crossing a twentieth of a
nanometre past a cylinder patch's side was found without the hulls and
lost with them. The boxes and the slab are apart by more than the same
(each holds the hull, so a point that near the hull is that near them
too, and the slab's margin is far under a curved piece's bulge until
pieces are a few millionths across): with `1e-12` alone, a crossing just
past a side square to the axes (a wall's level rim) was lost where the
edge's range was split between the rim's height and the crossing, or
where the edge ended just short of the wall, the edge's piece beyond the
rim parted from the patch's by its box, or by the slab. A patch's piece
more than 8 times longer than high across its longest side is halved
across that side rather than quartered (`MAX_ASPECT`): quartering a pin's
wall 1 000 tall and under 2 wide split it across its width as often as
along its height, and a hole's rim arc through it, whose control hull
holds the wall's width near the rim's height, had every piece of that row
to look at (965 pieces against under 200 halving). Newton on `E(t) = P(u)`; once it finds a crossing in a
piece, the rest of the edge's piece either side of it, less a thousandth
of it round the crossing, is searched again: an edge running through a
wall a little inside its rim, in and out within one small piece, lost
the second crossing, the count 0 dropped the first, and the edge was
taken for not crossing at all), and made to add up to the count the same
way; ones the search didn't find go where it found the two meeting but
kept no crossing (a hit the count overrules, as where the edge leaves a
vertex on the patch's corner: nearest the patch), else, on a planar
patch with just one end of the edge within the resolution of its
corners' triangle, at that end (`end_on_triangle`), else where they came
closest (the middle of the smallest pieces the search looked at, which
had put such a crossing a sixteenth of the edge from the vertex). The
end: a cylinder's seam edge a fraction of a tie off a box's side has its
rims and its wall's diagonal leave its ends on the side's plane, the
count crosses them there, and the search, its roots a rounding past the
ends, finds nothing; put where its closest pieces were, up to a 128th of
a quarter arc along the edge (thousands of resolutions at fit 0.001,
and the later root finding, its roots outside the edge, kept them
there), the cut ran through those points, and the result was some 40
resolutions times the cut's area off (a pin less a box, or, with walls
along one direction joined, a pin poking into a quarter tube's corner
at the tube's end).
Those are marked as not solved (`Crossing::solved`, from `pick`): on a
plane or a quadric they go to the edge's root on the patch crossed once
placed (see "Cutting curved faces"), and they must lie on the other
operand (see "Assembly").
Such pairs are searched with a count of 0 too, so an edge passing into a
face and back out gets both crossings. Crossings are ordered along an
edge by position, ties exactly (a straight edge through two planar
patches) or by face and index.

**Ties.** Heights within the tie distance are ties, measured as the
distance in space, as the exact `Reach` and `Height` measure theirs
(`curved::square`): a height `dh` along `UP` where two shadows cross
counts as `dh·|m̂·Û|`, `m = g' × e'` the two tangents' normal there (the
curves' distance square to both), and a patch's point over a vertex as
`dh·|n̂·Û|`, `n` the patch's normal there (the vertex's distance from
the tangent plane); either factor no less than `1/TIES`, so a tie
reaches the resolution at most along `UP`, as `Height`'s cap, and no
more than 1. For straight edges that is the exact `Height`'s tie:
`|dh|·|m̂·Û| ≤ tie` and `|dh| ≤ TIES·tie`. Where `m` or `n` has no
direction (zero, so small its square underflows, infinite or not a
number), `dh` as it is. Heights where shadows run along each other
(`along_above`, the parallel cases) stay along `UP`: there is no
crossing to measure square at. (Edges both steep in the vertical plane
their shadows share are closer in space there than in height, so a
window between the measures is left. Probed with cylinders whose caps
lie in a plane through `UP`, exactly or turned `1e-9` or `1e-5` off it,
coaxial, apart, or a hair off each other within that plane, flush or a
fraction of a tie apart, 3 456 operations: no wrong result, and tying
`along_above` by the curves' distance in space changed nothing, 1 204
right against 1 211. Such caps are refused far more than others: see
Known gaps.) `curved_layers`' shortcut (every point
of the patch's hull above the vertex, or below) compares with the
resolution rather than the tie, the furthest a tie reaches along `UP`.
Measured along `UP`, a cap a few ties along `UP` off a box's steep
face was a tie for the exact `Reach` and not for the heights, and the
two counted crossings no one configuration has (`Inconsistent`, then
with no retry on the curved path). Ties are decided as `A`'s
perturbation would (`A` moved by `ε·s·n_v + ε²·T2 + ε³·T3`), order by
order, each to first order (`first_sign`: the first order `δ`, then the
two translations, as the exact predicates take them; stopping at `δ`
decided a vertex on the line where two flush faces meet against the
exact predicates' second order): at an edge crossing, `e` (moved by `δ`
interpolated along it) rises over `g` by `δ·m / UP·m`, `m = g' × e'` the
two tangents' normal (the crossing slides along `g` as the shadows
shift); where the tangents are parallel (`|m|` within `1e-9` of theirs),
by `(T × δ)·(T × UP)`, `T` the tangent (the crossing slides along both);
a point of a patch with normal `n` there rises over a vertex of `A` by
`−n·δ / n·UP`, and over a vertex of `B` by `n·δ / n·UP` with `δ` the
patch's corners' interpolated. Each of these is linear in the motion,
and an order whose value is only rounding (within `RHO = 2⁻³²` of
`|d|·|∇f|`: the motion within an angle `RHO` of square to the
gradient) is taken as zero, as the exact predicates' later orders are
(see "At every order"): a vertex moving along a turned plane against a
patch on it gave `δ·n` some `1e-17`, its sign noise (the seeded suite's
tallies are the same with and without it). The measure is the sizes'
product, not the sum of the terms `Σ |d_i·f(e_i)|`, which sees only
cancellation between terms: computed directions (normals at refined
corners) and gradients (a patch's normal, `g′ × e′`) carry rounding in
their zero components, and its product with the other vector's large
component is a first order of rounding alone. Where seam rulings of two
crossing cylinders meet on the cut (below), every first order there is
zero exactly, yet `δ = (−1, 2.1e-15, 3.7e-15)` against a gradient
`(0, …, 0.009)`, and `δ = (1, 0, 0)` against `(−4.2e-17, 0, 0.077)`,
passed the sum as real: 12 decisions at two points of one round's
intersection with a bar taken from rounding, six of them against `T2`,
no single motion's, and the counting built a zero-size handle (see
"Results that aren't manifolds"). By the sizes every one goes to `T2`
and the intersection works (0.018 of the fit times its bands' area
off). A real first order down to `2.3e-10` of the vectors' sizes still
decides. Snapping the directions' tiny components to 0 instead wasn't
enough: the gradients' rounding still decided. Why the sizes bound the
rounding: computed directions and gradients (normals, tangents' cross
products, a domain step's map) carry an error in each component
relative to the vector's length, not to the component, so a computed
first order is off by about `u·|d|·|∇f|` and more (`u` a double's
rounding; Cauchy–Schwarz on `δ̃·∇̃f − δ·∇f`); `RHO` is some `2·10⁶` of
`u`, room for a few operations' worth. And `Σ |d_i·∇f_i| ≤ |d|·|∇f|`,
so the measure only adds ties: what was zero still is. The decisions
stay one configuration's: an order taken as zero is `δ`'s part along
that gradient, under `RHO·|δ|` (an angle), and the motion less that
part (`δ′`) is zero there exactly while every other first order
`δ·∇g` moves by at most `RHO·|δ|·|∇g|`, under what it must be to
count as real, so it keeps its sign: the decisions are those of
`ε·δ′ + ε²·T2`, the exact predicates' order after a zero first one
(two tied gradients nearly parallel would widen that part, the same as
for any threshold). The later orders' curvature terms (a vertex moving
along a curved wall) are left out, as they always were: the primitives
decide to first order in each power. Where every order is 0, by
`δ·UP`, as for horizontal surfaces. So flush planar faces between a curved and a flat
operand behave as between flat ones. Vertex directions of curved
patches are their normals at the corner. Beyond heights:

- **Rays through a vertex.** Where a curved edge's shadow crosses the
  ray's line at the vertex itself as far as rounding tells (the vertex an
  end of the edge, or on it: a pin's rim vertex on the hole's rim),
  whether the crossing is ahead of or behind it is the perturbation's:
  the vertex moves by `δ` relative to the edge, the crossing to
  `ε·(δ_across·T_along / T_across − δ_along)` along the ray (`ray::
  tied_ahead`, order by order). Taken from the rounded point it was
  noise, and a pin's rim vertex wound −1 round the plate.
- **Shadows along each other.** Where one edge's shadow lies on the
  other's conic (the same arc in both operands, or a piece of it:
  `arcs::cross` gives `None` when the polynomial is zero within `1e-10`
  of its terms), solving that polynomial gave random roots. Where the
  two edges also lie on each other in space over a stretch (`e`'s ends
  on `g` and `g`'s on `e`, within the tie, give it; three points
  between must be on `g` too: lifts of one conic shadow meeting at
  three points of it are one curve), the crossings are decided one by
  one (`Curved::along_crossings`): `A`'s perturbation `δ` takes `e`'s
  shadow to `g`'s left where `δ·(UP × g')` is positive, so the
  perturbed shadows cross where that changes sign, `σ` its sign after.
  With `e`'s tangent's homogeneous numerator `h` (a quadratic in
  Bernstein form: `w·(c − p0)`, `(p1 − p0)/2`, `w·(p1 − c)`) turned by
  `o = sign(e'·g')` for `g'`, `o·δ·(UP × h)` is a cubic (`δ` linear
  along the edge), whose roots in the stretch are the crossings, each
  above or below by its own rise (the parallel rule above), its `σ`
  the change of sign across it, from the signs at the midpoints
  between the roots; where one of those is zero to rounding (a
  near-double root found as two roots a rounding apart, or twice at
  one place), the change across both goes to the later root, none
  across a tangency (the sign after a root alone had counted a
  tangency as a crossing, and its pair the other way); zero to
  rounding (every coefficient within `1e-12` of its terms' size), `T2`
  and then `T3` take `δ`'s place. Roots within the tie of the stretch's
  ends (either side: rounding may put one just outside) are left to the
  count (as crossings at an end), and whatever the
  count has beyond those found goes by the rise at the first such root,
  or at an end of the stretch where the cubic is zero to rounding (a
  root there may come out just outside and not be found), else the old
  way. Such zeros are what a circle drawn through exact axis points
  gives on YZ: a rim vertex's `δ` lies in the upright plane of the
  tangent there to the bit, and the old way, the height a sixteenth
  along, split those crossings wrongly (identical cylinders failed as
  before). The old way is for every crossing the ray tests count to go
  the same way, by the heights
  where the two come nearest or, tied there, the parallel rule: still
  so where the curves are apart in height (a top rim seen along the
  axis over a bottom one). On XY a rim's perturbed shadow never crosses
  the other's (`δ`'s part off the circle is radial throughout), so
  nothing changed there; where a cap's plane lies nearly along `UP`
  (the XZ and YZ sketch planes, or any upright one) the rim's shadow is
  a thin ellipse round whose ends the perturbed shadows cross once or
  twice, `A` above at one crossing and below at another. One sample's
  height for all of them gave a cylinder's two coincident walls (`A`'s
  and `B`'s) ends, and a flush pin in its hole, a second extrude of one
  circle or a cylinder cut back on those planes failed as
  `Inconsistent`, every operation on YZ. Sampling the polynomial
  instead of solving it would miss close pairs of roots. Measured in
  release at the default tolerance with a sweep of 1 665 operations
  (a cylinder of radius 1 in 4 arcs over 0..1 against the same circle
  in 4 arcs or in 3 turned 0.7, over 9 spans; square prisms over the
  same spans; pins in a plate's hole over 5 spans; union both ways,
  intersection and differences both ways) on 9 frames (XY, XZ, YZ, XY
  moved, an upright one off the axes, and four turned ones): failures
  243 → 44, `Inconsistent` 235 → 34, none on XZ or YZ (94 → 0), none
  for cylinders or pins; with the circles through exact axis points, 46
  failures. Every volume came out within `1e-12` of its closed form. Of
  the 5 130 decisions it made, the crossings found added up to the ray
  tests' count in all; with exact axis points 90 differed, each by one
  crossing at an end of the stretch where the cubic is zero, and none
  went the old way. The seeded suite's pins and coaxial cylinders (now
  on five frames, 200 operations) 164 → 191; the other seeded tallies
  didn't move (related 113/120, turned 156/160, chains 203/240,
  tangent 72/96, bosses 64/64, drilled 160/160).
- **Crossings at an end.** Crossings of the shadows solved at an end of
  either edge (within the tie of it: two edges from one vertex) are left
  to the count, which knows from the ray tests whether they are there;
  those the count has and the solve lacks go by the heights where the two
  come nearest (found by golden-section search between the samples, not
  the nearest sample, which put a vertex a sample's height above the
  edge it lay on).
- **Crossings at a patch's side.** A crossing of an edge through a patch
  within the tie of the patch's side (an edge of `A` lying in the plane
  where `B`'s wall meets its flush cap, say) is inside the patch or not
  as the perturbation moves it: the edge moved by `δ` relative to the
  patch moves the crossing by `δ − e'·(δ·n)/(e'·n)` on the surface, and
  it stays in if that heads across the side into the patch
  (`Curved::tie_inside`). Taken where rounding put it, a plate's
  diagonal crossed a boss's wall on neither side of its rim.
- **Crossings at an end lying on a plane.** A curved edge whose end lies
  in a planar face's plane (within the tie, inside the face further than
  a tie from its sides) crosses it near that end or not as the
  perturbation puts the end on the other side of the plane from the
  edge's inside next to it (`Curved::end_on_plane`, by `Flat::plane_side`,
  the predicate `s02` takes the plane's side from; an edge leaving the
  plane along it is left to the count). An edge with both ends in the
  plane and its inside on one side crosses near both ends or neither,
  which the count (how many more go in than out) can't tell apart; the
  search found the two, one a rounding outside the edge and one inside,
  and the count kept neither. So the cut of a boss's wall by a plate's
  top, between two wall vertices at its height joined by a curve dipping
  into the plate, ran in the triangle below the curve instead of the one
  above (`Invalid(Fold)`), and drilling a plate twice sometimes failed.
  It also came out wrong where the check couldn't see it: on a turned
  frame, a hole drilled through a ring sunk in a plate (its wall split at
  the plate's faces) took `1.8e-6` too little, a sliver left where the
  ring's wall meets the plate's bottom
  (`a_hole_through_a_sunk_ring_on_a_turned_frame`).
- **Edges in the surface.** An edge lying in the surface a patch's face
  claims (sampled within the resolution) isn't searched with a count of
  0: the perturbation takes it off to one side, and the search found
  crossings in and out where rounding had them.

## Pairs of faces (`boolean/pairs.rs`)

A pair (triangle `p` of `A`, `q` of `B`) is every pair whose boxes meet,
and any with ends. Its **ends** are the crossing records on its faces'
edges through the other face (sign seen from `A`); the counting leaves
as many of each sign. For flat operands they are two or none, joined into
one arc (else `Inconsistent`). With curved patches a pair is decided from
its ends only with a **certificate** that no closed loop hides in it: the
patches' normal cones apart (`NormalCone::apart`: no normal of one
parallel to one of the other, and a loop needs one), both planar, one
planar and the other on a **cylinder** (its face's quadric, unchanged
along a direction: `pairs::along`) with its normals within a half-space
(a plane cuts a cylinder in lines along it, which run out of the patch,
or in a conic round it, whose normals turn right round), or, with no
ends, their control hulls apart (GJK) or **walls along one direction**
that come near each other (below). Then no ends is no cut, two ends
are one arc, and more ends of two planar patches join in order along the
line their planes meet in (if they alternate). Without the cylinder
certificate a box's side against a boss's wall along it refined without
end. The argument holds for the exact plane and cylinder; the patches
are only within the resolution of them, so a loop the certificate
misses lies within about the resolution of both surfaces (a near
tangency),
and leaving it out moves the result by less than that.

`along` takes the null direction of the quadric's matrix along which
its linear part vanishes too: for a matrix of rank 2 (a circle, ellipse
or hyperbola swept) the rows' largest cross product; for rank 1 (a
parabola swept, from a conic segment of weight 1: the matrix is `g·gᵀ`,
whose null directions are a plane) the direction square to the largest
row and to the linear part. A plane cuts a parabolic or hyperbolic wall
in lines along it or in an open conic, so the plane-and-cylinder
certificate holds there too.

**Walls along one direction** (`parallel_walls`): both faces claim
cylinders (`along`) whose directions are parallel within the resolution
over the pair (`|d1 × d2|` times the diagonal of the pair's boxes at
most the resolution: lines along each drift apart by less than that
across the pair; extrudes from one normal agree to rounding). Such walls
meet only in lines along that direction (their cross-sections' common
points, swept), and a line runs out of both patches through their edges,
where the counting gives it ends: with no ends there is no cut. Sound
for any pair, but certified only where the walls come within
`PARALLEL_NEAR` = 64 resolutions of each other (a point sampled on
either patch, `samples`, that near the other's quadric). Walls further
apart are refined as before until their hulls part, though they don't
meet either, and later stages rely on that refinement: where a cap's
flat ring lies between two such walls (a cylinder inside a larger one
sharing its top, cylinders of radii 1 and 1.001 stacked, a coaxial one
0.004 larger over part of the other's span), the ring is cut along both
rims and triangulated from the walls' pieces; from unrefined walls,
whose quarter arcs bulge across a ring narrower than their sag, its
triangles fail the hull rules against the walls' (`Hull`), or, where a
rim's quarter runs through a cap vertex in line with its ends, the ear
cut there has three corners on a line and a curved side and folds.
Refined until their hulls part, the pieces bulge less than the ring is
wide. Walls within the resolution of each other (a tangency) never part:
before, they were refined until their pieces were flat (about
`√(8·R·resolution)` across), the pairs along the line doubling every
round, and a line contact of a few millimetres at the default tolerance
ran out of the budget (3 to 4 million units, 3.6 s and more). Now a
tangency's refinement stops once a sample of its pieces comes within 64
resolutions of the other wall: pieces about 0.1 across for walls of
radius 1 at the default tolerance. Walls closer than 64 resolutions
with a ring between them are left to fail (see Deviations for the round
gate this replaces).

**Coaxial walls** (`coaxial::walls`, `coaxial::along`): both patches lie
on cylinders or cones, by their faces' **forms** (not their tags: a cone
too nearly flat for its quadric claims none, though its patches lie on
it exactly), each patch within the resolution of its form at the sampled
points (not a fitted patch), on one axis within the resolution where the
pair is (the axes' nearest points within it at the patches' corners,
their directions apart by no more over the patches' reach from the axis:
a choice of path, not a decision); their meridians cross at slopes
`dρ/dh` at least `MIN_SLOPE` = `1e-3` apart (two cylinders never cross,
and are told before any sample), and by at least four times the sine of
the angle between the axes (the reach bounds the tilt only across the
axis, not along it: a thin wall long along it could otherwise lean
further than the slope, and the cut stop being one curve round it); and one patch lies on one side of a
plane through the axis (its control points do). Then the surfaces meet
in one curve going once round the axis, a parallel within the
resolution (wandering in height by up to the axes' offset over the
slope), which no patch on one
side of such a plane holds whole: no closed loop, every arc runs out
through ends. No ends is no cut; two ends one arc; more are joined in
turn round the axis (sorted by the tangent of their angle from that
patch's direction), each with the next, which must be of the other sign,
and all within `4·resolution/slope + resolution` in height (else the
pair is split as before). A swap of two ends of one arc in that order is
harmless and any swap across two arcs pairs two of one sign, which is
refused. Without it a cylinder and a cone on one axis crossing in a
circle were split until their normal cones parted (1 412 patches for
their union, their intersection and difference `Invalid`); with it, 98
to 476. Pairs of such walls with no ends are certified however far
apart, so a cap's ring between a cylinder's rim and a coaxial cone's
section comes whole to the triangulation, which is why curved sides are
split for their bulges (see "Curved sides when triangulating").

The certificate, with the cuts in parallels (`chain::parallel`), is a
**shortcut** (`pairs::Shortcuts::coaxial`), as joining lines along
walls is: pieces certified rather than refined stay as large as they
were, and some results that refinement and tracing got right failed
from them: a stack a ten-millionth off a cone's axis, its pieces
`1e-7` from the cone's seam vertex (`EdgeNeighbours`); a ring's corner a
hundredth outside a cone's wall, the cuts a fiftieth apart about it
(`Hull`); a ball through a ring's inner cones, the bands between the
parallel and the ball's polar cap fanned from the cap's corners with
nothing left to halve (`TooComplex` with budget to spare: a sphere has
no ruling to free a band tree's root). So an operation that took it and
fails is tried again without it (see "Tried again without the joins").
Of 1 200 random pairs of turned solids on one axis (upright, turned and
moved, some a hair apart), 14 results the build before this step got
were lost to it; tried again so, none are, and 45 more work than
before (of 1 200 boxes through turned solids, 26 more).

**Ends along one direction** (`along_generators`): a pair on such walls
that has ends and no certificate is joined line by line before it is
split. Each line's stretch inside both patches runs between two ends
where it leaves one or the other, at the same point of the
cross-section (`at − d·(at·d)`), with opposite signs, apart along `d`.
So the ends are grouped by their cross-section point (single linkage),
and if every group is exactly two such ends, each group is an arc; any
other grouping (one end, three, four: two lines a hair apart, a line
leaving and coming back; two ends at one place along `d`) leaves the
pair to be split as before. A line is joined only where it is **clear**:
at every end of the pair the walls cross at an angle `θ` with
`θ² ≥ 2·LENS·resolution·κ` (`LENS` = 1/4, `κ` the sum of the two
cross-sections' curvatures there, `tᵀ·H·t/|∇F|`; `sin θ` for `θ`). Two
curves crossing at `θ` bound a sliver that closes no sooner than `2θ/κ`
away and is at least `θ²/2κ` thick, so a clear line's sliver is at
least a quarter of a resolution thick (16 tie distances), wherever its
other line lies, in the pair or not. Thinner ones (walls overlapping by
less, down to crossings only the counting's ties make) are left to
refinement, whose later counts may drop their ends and give the
operands unchanged; joined in an early round they fold (at `LENS` =
1/2000, the tangent suite's differences at fit 0.1 overlapping by
`1e-6`, a hundredth of the resolution, did). For two walls of radius
`R` side by side a line is clear at least `√(R·resolution)` from the
other. Ends group within `θ/κ` (the pair's smallest angle, largest
`κ`): half the distance to another clear line. The joins are sound for
the same reason as the certificate: such walls meet only in lines along
`d`, and a group of two is one stretch of one line.

Measured on unit cylinders side by side overlapping by 0.02 to 30
resolutions at fits 0.1, 0.01 and 0.001 (four placements, four
operations: 384), `LENS` from 2 down to 1/8 turned no result into an
error against no joins at all, and every volume was right; 1/16 turned
one. Results: 61 without the joins (176 spending over a million units,
565 M units in all), 177 at `LENS` 2 (106, 356 M), 214 at 1/4 (65,
205 M), 229 at 1/16.

Probed again against exact volumes (the lens by Green's theorem from
the curves' crossings; each result to `1e-10`, or, where the lens is
thinner than the resolution, within a skin a resolution thick over
it): an ellipse's tip, flank and side (half-axes 2 × 0.5 and 3 × 0.3,
curvature up to 33) against a unit circle's wall; pins of radius 0.5
to 0.999 poking out of a hole's wall (curvatures nearly cancelling);
unit walls on the XY plane, on a frame turned off every axis and on
one some 3 700 from the origin, the second's span inside the first's,
flush with its top, `1e-7` over it, `3e-7` under it, or overlapping it
by `1e-5` or half a resolution; walls a hair off parallel, in one plane
(a V from a gap at one end to an overlap at the other) or skew (a long
loop); overlaps from a twentieth of a resolution to a thousand, at fits
0.1, 0.01 and 0.001 (3 204 operations). No result was wrong. Where
the lens is thinner than the resolution an intersection may come out
empty while the differences cut it, and caps within the tie of each
other move a result by up to a resolution times their area, as without
the joins. Against no joins: ellipses 405 → 578 work (545 M → 114 M
units), the spans 207 → 511 (932 M → 123 M), the tilted walls 4 and 4,
the pins 258 against at most 94; but some results that refinement
got failed, so an operation that fails (but for running out) after
joining some lines is tried again without the joins (below). The grouping
holds by construction: lines along `d` cross the cross-section at
points at least `2θ/κ` apart, a line's two ends lie on one of them (up
to where the counting put them), so groups within `θ/κ` are the
lines'. An end the counting only placed at a tie may lie off its line:
a rim flush within the tie with the other wall's put one `0.3·θ/κ`
along it; asking each end to lie within `θ²/4κ` of both walls (a
quarter of the grouping distance across) refused it and lost the
results the join got right, so ends aren't checked. Leaving steep
crossings to refinement (`sin θ` over 1/8 to 3/4) won back as many
results as it lost, so every clear line is joined.

**Tried again without the joins** (`boolean_within`; the shortcuts
are `pairs::Shortcuts`, the joins and the coaxial certificate): a pair's lines
joined in an early round leave the pieces beside them as large as they
were, and some results that refinement got right failed the hull,
neighbour or fold rules from them (16 of the 864 ellipse operations
above, against 189 won; 17 of the 1 080 spans, against 323). So when an
operation that joined some lines fails, except by running out of the
budget, it is tried again without joining any
(`checked_with(.., Shortcuts::NONE, ..)`, the pairs split as before),
within as much work again as the first try took, but at least `AGAIN`
= 150 000 units (and what is left of the budget). The second try's
result if it passes, else the first try's error, with its evidence (or
`TooComplex` where the budget, not the bound, stopped it). A first try
that took the coaxial shortcut (see "Coaxial walls") and fails, also
by `TooComplex` with budget left (bands that can't be halved within
the fit), is tried again first without it but joining lines, as
operations were before it, with what is left of the budget (refined
so, a ring against a cone took up to 400 000 units, more than `AGAIN`);
if that joined lines and fails, without either, as above. Unbounded, the second try ran
most of the refusals on to the budget for one result in fifteen
(ellipses 114 M → 322 M units, for 15); bounded by the first try's
work alone it lost the spans' fast folds (a sliver under the resolution,
folding in a few thousand units and refined in 50 000 to 850 000).
Measured (ellipses and spans as above, against the build with walls
along one direction neither certified nor joined): ellipses 591 work
(578 without the second try, 405 with neither), lost 3 against 16,
151 M units (114 M, 546 M); spans 537 (522, 216), lost 2 against 17,
189 M (124 M, 930 M). The three left need over a million units
refined. Results stay the same at 1 and 8 threads and on budget
ladders (a budget under what the result took gives `TooComplex`, never
another result). Each of these tries is itself decided with near ties
and, where they don't fit together, again exactly within its cap (see
"The exact retry inside the shortcut retries").

**Tried again without the seam rules** (`boolean_within` around
`tries`, `curved::SeamRules`). The first-order measure (`|δ|·|∇f|`, see
"Ties" under the curved primitives) and the trace's start into both
patches (`inward_sign`, see "Chains") are the rules for seams of the
operands meeting on the cut. Where the seams meet exactly they make the
decisions there one configuration's; on frames turned by a hair, where
they meet only nearly, the rules are one more way of deciding a near
tie (a first order of `1e-9` of `|δ|·|∇f|`, real but under `RHO`, goes
to `T2`; an arc starts into both patches and is traced where before it
failed and a checked conic stood in, and refinement goes another way),
and the thin triangles of a tangency's two crossings there pass the
check or not by chance either way. In the seam sweeps 14 operations
(of 3 440) that worked without the rules failed with them, all
`Invalid` or `NotManifold`, 12 on hair or turned frames, 2 on the
world frame with radii and centres off the grid. So every decision
the rules take is compared with the old one (`first_sign_by` with the
terms' sum; the smallest coordinate of the end's own patch), a
`SeamRules` per operation noting any that differs (an `AtomicBool` set
from the parallel maps; which decisions are taken is a pure function of
the operands, so the flag is the same at any thread count); and where
all the tries (every shortcut, then fewer, each tied then exact) fail
with `Invalid` or `NotManifold`, or `TooComplex` with budget left (a
bound: a ball cut by a box round its pole ran into one, which the hunts
found), and the rules changed a decision, they
are all made again without the rules, within three times the work
they took or `AGAIN` (and what is left). The second tries' result if it
passes, else the first tries' error with its evidence. Those that
worked in the sweeps took 0.1 to 2.1 times the first tries' work
(cross holes up to 1.7); of 158 retries there, 14 worked, and those are
the 14 operations the rules had lost (every one won back, none lost
against the build without the rules). A retry costs a failed
operation about its work again, only where a rule decided. A retry cut
off by the operation's budget (not its cap) gives `TooComplex`, as the
shortcut tries do, so a smaller budget never gives another result: of
480 operations on skew crossing cylinders, 2 that had spent over a
quarter of the budget failing now end so, not `Invalid`.

Two patches on **one surface** (their faces claim quadrics and points
sampled on each lie on the other's within the resolution: a pin in a
hole cut by the same circle, cylinders of one radius stacked or
overlapping) don't meet at all once `A` is perturbed off it, so they have
no cut, and ends there are `Inconsistent`. Such pairs had no certificate
and were refined for a minute or two before running out of budget. The
test is within the resolution, not exact: surfaces a hair apart or
tilted pass too. Where those cross, the counting gives ends and the
pair is `Inconsistent`; a loop they hide lies within the resolution of
both, and leaving it out moves the result by less than that.

Any other pair is **refined**: both patches, where larger than the floor
(`MIN_SPLIT` resolutions across their control points' box), are split by
the red–green `Refiner` (so neighbours across split edges are too, flat
faces with straight inner edges), and everything is counted again, every
new vertex and edge with primitives of its own (`refined`: one refiner
per operand kept across rounds, so green pieces are never bisected
again; rounds up to `2·MAX_REFINE_DEPTH`, else `TooComplex`). A pair of
pieces both at the floor is decided by **fixed rules**: no certificate
means no loop, and the ends, by angle round their middle in the plane of
`p`'s corners (a pseudo-angle, `+ − ÷` only), join each + to a later −
as parentheses match, starting after the lowest running sum, so the
arcs don't cross. Pieces flat within the resolution certify each other
as planar, which is what ends refinement at a tangency long before the
floor (at pieces about `√(8·R·resolution)` across), where walls along
one direction don't end it sooner.

`refined` returns the operands as refined (the same surfaces, split; the
operands' vertices first, then refinement's), the tree of each operand's
red splits (`mesh::Node`: corners, patch, parent) and each refined
triangle's leaf in it, the counts and every pair's arcs (`Arc { tris,
plus, minus }`, by end vertex id): what cutting the curved faces starts
from.

## Touches (`boolean.rs`, `boolean/near.rs`)

`touches` on flat operands is one counting (tied as a union would be,
counted again exactly where the ties don't fit together) and its
`meet()`: an edge through a face or a vertex inside. With curved patches
it is one counting, `pairs::counted`, the very call `refined` makes each
round, so it is `refined`'s first round to the bit, and `true` on
`meet()`; otherwise a search for surfaces within the resolution `r`
over the broad phase's pairs, whose margin is `r` too (`near::near`),
whose answer it gives (both in `near::touching`). No refinement, no
Newton and no fallback.

Refinement isn't needed to tell whether two solids meet: it joins
crossing ends into arcs and finds loops no edge crossing shows, and a
loop no crossing shows is still a place where the surfaces come within
`r`, which the search finds. Before, `touches` ran the boolean's whole
refinement and read its last counting, which answered false for a
tangency along a line (cylinders side by side, a pin against a hole's
wall, a boss against a rounded corner: no edge crosses, no vertex is
inside, and the fixed rules take a pair with no certificate as no loop)
after running most of the budget, or out of it: 3.6 to 11 s at the
default tolerance, `false`, `TooComplex` or `Inconsistent` by where the
tangent line fell on the seams. A plane against a cylinder was already
certified. Stopping `touches` at the first round whose counting shows
a meeting (proposed on its own against a failing touch test costing a
whole budget on every draft change) is part of this: a meeting the
counting shows is `true` at once, and only that round is counted.

The search (`near::search`) runs depth first over pairs of pieces, a
stack per broad-phase pair in pair order, each pair's pieces visited in
a fixed order (the first's four `split4` pieces, each against the
second's), sequentially, so the answer and the work spent are the same
at any thread count and on wasm (no trig). A visit drops the pair, stops
the search or splits one piece or both. `near`'s visit drops a pair
whose control hulls are more than `r` apart (`mesh::apart`, GJK), stops
on a pair both `settled` (flat within `r/4`: control points within
`r/4` of the corners' plane, `input::planar`, and each edge's control
point within `r/4` of its chord, `mesh::flat`; or no larger than
`MIN_SPLIT·r` across), and splits the pieces that aren't. A settled
pair GJK calls near is dropped after all if the hulls are more than `r`
apart along the normal of either piece's corners' plane
(`near::apart_across`): GJK's direction to the closest points rounds
relative to the hulls' size over their distance, and on the round
octahedron's small pieces by a slab's large face at the finest
tolerance it stopped short at some 3 resolutions, `true` one way round
and `false` the other; near a tangency the pieces' planes are the
direction it was after.

Why it holds: a patch and its pieces lie in their control hulls
(positive weights), so surfaces within `r` are never dropped, and
`false` means they are more than `r` apart everywhere (up to the splits'
rounding, a few ulps of the coordinates a split, which nears `r` only at
the finest tolerance some `1e5` from the origin); with the counting
showing no crossing and no vertex inside, neither solid is inside the
other, so they don't meet. A `true` means two flat pieces' hulls come
within `r` (up to GJK's rounding where neither piece's plane
separates them), and a flat piece's hull is within `r/√2` of the piece
(`r/2` along the normal, both within `r/4` of the corners' plane, and
`r/2` across it, the piece covering its corners' triangle but for a
band `r/4` wide along its sides), so the surfaces come within
`(1 + √2)·r`, about `2.4r`; floor pieces only happen on surfaces
curving tighter than some hundreds of `r`. So curved solids within the
resolution touch, and up to about `2.4r` apart may, where flat ones
touch only within the tie distance (`r/64`), which is below anything a
user can place. `touches` only picks what an operation works on
(regen's targets), and every boolean decides for itself, so a `true`
can't make a wrong solid; at worst a body that grazes the tool is a
target whose boolean fails or is a no-op.

The search is what minimum distances run on (`measure::distance`, see
"Volume, area and measuring"): generic over what it visits (pairs of
patches here, tree nodes and pieces of patches, curves and points
there), with a visit that may change the item before it is split and
spend work of its own. Cost: a unit per visit (`NEAR_WORK`, measured 0.24
to 0.44 µs a visit, release, one thread), spent each 256 visits and at
the end; a piece `split4` can't split is `TooComplex`. Measured,
release, one thread, the counting included: tangencies 1.5 to 4 ms (up
to about 1 100 visits); near misses (gap `1.5r` to `10r`) at the default
tolerance 10 to 20 ms for cylinders side by side (30 000 to 45 000
visits, the same for 2 and 100 long ones), 12 to 45 ms for a pin off a
hole's wall (51 000 to 174 000), and under 6 ms at the coarsest
tolerance. The near-miss band (surfaces just over `r` apart along a
long line) is the dear case, bounded by the line's length over
`√(2Rr)` pieces, and charged: a hostile one runs out. At the finest
tolerance cylinders of radius 1 side by side cost 341 000 visits (about
0.13 s) at gaps of `2.3r` and more, and up to 2.6 million (about 1 s,
over half the default budget) at `1.2r`, where every pair must split
until its hulls clear `r`; the pin off a hole's wall 0.9 to 2.2
million. Such gaps can't come from rounding a tangency (far below
`r`), only from placing the tool `1e-8` off.
