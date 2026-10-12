# Deviations

- **The normal numerator is a cubic with 10 coefficients**, not a quartic
  with 15. The quartic form, `D·(N_u×N_v) + …`, is this cubic times
  `u0+u1+u2`; its 15 coefficients are the cubic's degree elevation. The
  cubic's coefficients are the ones splitting provably preserves (above),
  and they are cheaper. The mesh's fold invariant uses these 10.
- **The exact fold direction is the smallest enclosing cone**, found by
  enumerating its possible rims, instead of a 3-variable LP. It answers
  the same question for the Euclidean margin (an LP over a box would be
  conservative by up to √3) and gives the normal cone as well.
- **Exact cone triangles with geometric-mean rulings** (this replaces the
  earlier "no exact cone triangles", which assumed a linear ruling: that
  one leaves a strip `8e-4` to `3e-3` off a unit-sized cone, and its induced
  parametrization does depend on the projection point). A ruling of
  weight 1 with its control point at `√(da·db)` from the apex is the one
  parametrization every projection point induces, so the patches either
  side share one record exactly (see "Swept strips"). A strip next to a
  hyperbolic paraboloid strip (whose rulings must be linear) can't share
  such a ruling exactly and will be fitted there.
- **Forms as built** differ from the plan's sketch: a cylinder is `{
  point, axis, radius }` (no line type), a revolved conic's meridian is a
  `Conic2` in `(ρ, h)`, only a plane's form carries which way the face
  faces, and the variants for forms not made yet (general quadrics,
  drafted, swept, lofted walls) come with the work that makes them. The
  debug check also checks a plane form's direction against the patches'
  normals.
- **`revolution_strip` takes the axis.** The diagonal's plane holds
  `a1'`, `a1` half a turn round; found on the bottom's conic alone it
  rounds as `ε/θ²` of the radius for small pieces, from the axis as the
  coordinates.
- `Conic2`/`Conic3` are aliases of one generic `Conic<P>` over the sealed
  `Point` trait, and there is an `f64` box type, `Bounds<P>`.
- The weight bounds stay at `1/64 ..= 64`: nothing in the tests asked for
  other values.
- **Edge neighbours across a curved edge**: the plane through the edge's
  control points must have one patch's other control points more than the
  resolution off it and the other's no more than the resolution past it on
  the other side, not both strictly off it. Both strictly off would refuse
  every flat cap along a curved edge, since the cap lies in that edge's
  plane; with one side strict the two still meet only along the edge.
  Straight edges and vertex neighbours, whose plane is free, keep both
  sides strict.
- **Neighbours are told apart by the vertices two triangles share**
  (none, one, two, or three, which always fails), not stored adjacency.
- **The face-tag check** takes a plane's six control points, and samples 15
  points of a quadric patch against the first-order distance `|F|/|∇F|`.
  `Quadric` carries an `origin` its form is written around. Tags are
  checked in every build by `check`, and on their own by `check_faces`.
- **`Tolerance` lives in the kernel** (fit bounds `1e-5 ..= 1e-1` mm,
  default `1e-3`), and `check` takes it for the hull margin.
- **Flat faces are split with straight inner edges** (red and green), not
  by the exact blossom: see "Refinement". The region is the same; the
  exact split's curved inner edges would lie in the face's plane with both
  pieces, where no plane through the edge separates them. A flat leaf
  whose straight green bisector would leave it is split red instead,
  where its red children pass the fold check.
  The `Plane` tag is tested first on every input patch split or bisected
  this way (six control points against the resolution), and a wrong one
  fails with `Invalid(Face(t))` rather than be reshaped.
- **Refinement keeps leaves and pieces** rather than editing the halfedge
  mesh in place: leaves may have one hanging vertex per edge, rendered as
  green halves, and the mesh is rebuilt from the pieces with `MeshBuilder`
  (pairing by vertex id). Green pieces are never split themselves; their
  leaf is.
- **Repair splits both patches of a failing pair**, except a flat
  non-neighbour, and fails at once on a degenerate corner
  (`Patch::degenerate_corner`, new), on a whole leaf that is an affine
  triangle failing the fold check (planned for any piece flat within the
  resolution, which splitting can mend), on a failing pair of flat pieces
  (flat within a sixteenth of the resolution for non-neighbours), and on
  a failing non-neighbour pair whose surfaces it finds within the
  resolution (a witness), all with `Invalid`; with `Invalid` too, of the
  failure that asked, on a leaf to split under `MIN_SPLIT` = 64
  resolutions if flat or `MIN_CURVED_SPLIT` = 8 if curved (it was
  `TooComplex` at 64 for all: only curved pieces' sag shrinks as they
  are split, and a scale limit is detail too small for the tolerance,
  not complexity); with `TooComplex` at `MAX_REFINE_DEPTH`,
  `MAX_PATCHES` and the budget. A small leaf to split on a plane fails
  with the failure that asked, not the wrong-tag `Face` the refiner
  would have named on splitting it.
- **`Budget` is a limit and `Work` its counter**: operations take `&Budget`
  as planned and count down a `Work` shared by their steps. `MAX_WORK` is
  `1 << 22` (about 4.2 million units, two seconds on one thread; it was
  `1 << 26` until the booleans' charges were measured). `MAX_TRACE_STEPS`
  is left to tracing.
- **The box and cylinder are `Mesh` constructors** (`Mesh::cuboid`,
  `Mesh::cylinder`) that take the feature id and the tolerance and always
  run `check`; `Solid::cuboid` and `Solid::cylinder` wrap them.
- **GJK's triangle and tetrahedron projections use cross and triple
  products** instead of the Gram matrix (a fix: see "Control hulls").
- **Stitching by the shorter diagonal**, not a pattern fixed by the
  counts alone: pairing points by parameter made triangles two steps wide
  along skewed cylinder walls. Both are crack-free, since the boundary
  points are shared either way.
- **Walls of one profile curve are one face for feature edges**: the
  segments of a curve (`Side { curve, segment }`) are separate faces, and
  drawing every face boundary would draw a circle's seams. The rule is
  now by face key (`FaceName::key`), which is that and more.
- **`Solid::bounds` is an `Option`** (`None` for the empty solid), and
  `Solid::bounds3` gives the `f64` box. `Display` holds only the fit
  tolerance; the other targets are its constants.
- **The chord target is kept inside patches too**, by the ring's choice
  of diagonals, its corner flips and ruled strips, and by measuring and
  refining the grids of every patch but a plane's or a strip's (see
  "Solids and tessellation"). The plan counted every part from the
  segment counts alone; grids that bend are now measured first, still
  before any vertex is made, in rounds bounded by the limits, and a
  ruled strip's test reads the patch, also before.
- **Ruled strips on cylinders and cones only.** Wall patches with a
  ruling side are drawn as strips between their curved sides, without
  inner points: a wall's triangles fall 4 to 7 times. They leave out
  planes, which the plan didn't: a plane's triangles stay exactly as they
  were, and no plane patch of the test solids passed the test. The test
  holds the straight side to a line and the lines across to a sixteenth
  of the chord, not all of them to the plan's eighth: a short arc of one
  segment passed as a straight side and fanned a cut wall's strip round
  it, and a nearly ruled cone patch adds at most a sixteenth to its
  strip's error.
- **`Shape` is gone**, not kept as a test helper: `Solid::cuboid` and
  `Solid::cylinder` are the test solids, so `Shape`, `ShapeError` and
  `position_in_range` were removed from the kernel.
- **`Symmetric` is the whole depth**, half each side, and extrude
  distances are at least `MIN_LENGTH` (1 µm), as dimensions are, rather
  than only above zero.
- **`BodyId::NEW`** stands for the body an extrude not yet added makes;
  `AddFeature` and `SetFeature` give the body its id whatever the
  command held, so callers never guess ids. Bodies are named by the
  command ("Body N").
- **`Document::removal` takes a `Removable`** (`Feature` or `Body`) and
  returns `Removal { features, bodies }`; only an extrude's sketch counts
  as a use, and excluded lists are dropped from rather than cascading.
- **Caps are triangulated with `spade`** (constrained Delaunay, exact
  predicates, builds for wasm; the kernel's only new dependency), then
  mended in rounds (Steiner points at ears, halving curves at narrow
  corners and folds), rather than by ear-clipping. Only its insertion
  (with its hierarchy for point location), constraints and face walks
  are used, with point location and constraint removal for the
  triangulation kept from round to round; its refinement and its bulk
  loading, whose hash sets could be iterated in a random order, are not.
  The points go in in a fixed shuffled order, which bounds the flips.
- **Flat corners are mended on a second try** of the caps, when the
  first fails, rather than always: moved-in points can line up into
  slivers of their own, so doing it first lost some solids the plain
  caps give. The second try resumes from the first round of the first
  that found a flat corner, and isn't made when there was none: the
  same results as starting over, for less work.
- **Caps are refined for quality on the first try**, not as a third try
  after the plain and flat-corner ones: plain caps that pass keep long
  thin triangles (a plate's fans out of a corner to holes on a common
  tangent) that a later cut along them fails on, so every cap triangle
  with an angle under 5° is refined, at the cost of patches (see "Cap
  quality"). Both tries refine; the plain caps' own two tries come
  after them, the second from the plain first try's fork, unrefined
  for crowding too (extrude step 5), so what they extruded still
  extrudes unless refinement spent the budget first. Refinement goes
  before the ears' centroids when nothing is to be halved, halves an
  encroached chord again while the asking point still encroaches on a
  half, keeps its points in rounds that halve, and a mending halving
  refused as too small is left to refinement rather than failing the
  round: each found
  needed on fine circles of arcs, perforated plates or spline chains.
  The flat-corner second try stays: measured against refined caps
  then plain ones alone, it still mends 9 and 14 of 500 plates of small
  holes at fit 0.1 and one of 500 at 0.01, short curved segments whose
  chords refinement leaves alone (see "Cap quality"); nothing else
  measured needed it.
- **Mending on the tries that refine stops halving a segment at
  `MAX_MEND_DEPTH`** (6) and leaves the corner to refinement, rather
  than halving on to `MAX_CAP_DEPTH` (16): a narrow corner along an
  arc's tangent to a far vertex doesn't open however often the arc is
  halved, and the rounds spent there pushed the 400-hole plate to the
  second try and limited `k × k` plates to `k = 51`; now `k = 53`, on the
  first try, with fewer patches. Results changed only where a segment
  reached the depth (perforated plates); no case measured was lost.
- **An extrude's mesh is checked before it is repaired**
  (`Solid::new_repaired_within`), not repaired, merged and then checked:
  the construction nearly always passes, repair would keep it as it is
  and merging changes only names, so the result is the same for one pass
  over the pairs of patches rather than two. The check is charged as
  repair's first pass was, so for meshes that pass budgets run out where
  they did; one that fails after the fold check (274 of some 4 400 tries
  over the random plates measured) pays the pass twice, and may run out
  where it didn't (`TooComplex` for `Invalid`, both errors the next try
  is made on).
- **The caps' refinement shares only its bound and circumcentre with
  the boolean's shape points** (`mesh/shape.rs`: `SIN_SHAPE`,
  `circumcentre_from`), not the queue, walk, clearance and flips: the
  caps insert into spade's constrained Delaunay triangulation, which
  locates, flips and keeps the faces, test region membership by corner
  triples and clearance by BVHs over control hulls, and may halve
  chords; the cut faces keep their own triangle list and edge map, walk
  across sides with exact orientations, test clearance by flooding the
  triangles within it, flip only under curved-corner rules (`flippable`)
  and halve nothing. A shared core would be a triangulation abstraction
  over both with their rules as callbacks, more code than either loop.
- **The caps' triangulation is kept from round to round**, points
  inserted and halved chords replaced in it, rather than made afresh
  each round: rounds of mending and of refinement each paid 8 units a
  vertex, which limited plates of `k × k` holes to `k = 28` once refined
  (38 plain) and ran the 16 384 fan out of budget; kept, they fit to
  `k = 51` and the fan passes. Where four points lie on a circle the
  triangulation is one of the Delaunay ones rather than the one the
  shuffled order makes, so the patch counts of such caps changed; caps
  under 64 vertices are triangulated afresh after a round that changed
  them, which keeps small solids (and the seeded boolean tallies) as
  they were. The fork the second try resumes from copies the kept
  triangulation. A halving asked for within half a circumradius of an
  earlier ask in the run isn't asked again. Refinement inserts its
  points one by one from a queue (the plan's sequential Ruppert), not in
  batches a round spaced half a circumradius apart; its halvings wait
  for the queue to empty. The crowding gate (the plan's count after the
  first triangulation, before any refinement) runs once a try when
  nothing is left to mend, since refinement for quality runs on every
  try now and leaves only the fans its exemptions keep crowded, and
  not on the plain caps' tries, which make what they made before; it
  refines with the same 5° bound (radius-edge ratio about 5.7, the
  plan's `B` between 3 and 5 being about 6° to 10°) and `c` = ½.
- **`extrude` takes a `Frame`** (origin and axes) and the extent as
  `from < to` along its normal; flipping and sides are the caller's.
- **`KernelError::Profile(ProfileError)`** carries a profile's own
  errors: `Empty`, `TooManySegments`, `Short`, `Segment`, `Degenerate`,
  `Open`, `Area`, `Cusp`, `Touching`, `Nesting(loop)`, `Triangulation`,
  `TooFine` (a curved segment the caps need halved below `MIN_SPLIT`
  resolutions: detail too small for the tolerance).
- **Curved walls are tagged with the conic's own cylinder**
  (`λ1² = 4w²λ0λ2`), not `Quadric::cylinder`: it holds for every conic
  weight, and for arcs it is the circular cylinder up to scale.
- **`Solid::area`** is new next to `Solid::volume`.
- **`Profiles::merge` returns the loops** (`Vec<Vec<Piece>>`, outer
  counter-clockwise, holes clockwise, unnested) rather than a merged
  `Region`, and takes region indices; `Piece` gained its vertices
  (`start`, `end`, into the new `Profiles::vertices`) so loops join up
  exactly. `Profiles::resolve` takes a slice of `RegionRef`s (the curve
  lists are worked out once) and `Profiles::reference` makes them.
  `MAX_REGION_CURVES` is twice `MAX_CURVES` (a curve can bound several
  loops), and `RegionRef::check` takes the coordinate limit, since the
  sketch crate doesn't know `MAX_COORD`.
- **The regeneration cache is a least-recently-used cache bounded by
  bytes on top of keeping what the last request used**, not two
  generations: what the request before used is never evicted, and older
  results stay while they fit the budget (256 MiB natively, 64 MiB on the
  web); the touch tests and booleans of bodies a join, cut or intersect
  takes out are marked used (`Cache::keep`), for putting them back. Not
  weighted by cost: timing per entry would make eviction depend on the
  machine. It also keeps meshes and whether sketches solve, and
  keys are 128-bit hashes of the values' postcard encodings, not their
  `Hash` (sketches hold `f64`s).
- **A failing draft is answered with the committed model** and the
  draft's error (`Drafted`), rather than the draft applied without its
  body. `touched` is in `Drafted`, not beside it on the response.
- **A join touching several bodies merges them; cut and intersect work
  on each target body on its own.** Consuming a body is worked out at
  regeneration, not stored in the document: targets are found by
  `touches` there, so a stored consumption would go stale on any edit
  upstream. A consumed body stays in the document and in Objects, marked
  "in Body 1" there; regen says where its geometry went
  (`Evaluation::merged`, on the answer and the wire). Unticking all but
  one body joins to that one alone.
- **Through all spans every earlier body**, excluded ones too, so taking
  one out or putting it back doesn't change the tool.
- **A join, cut or intersect that touches no target fails** ("it doesn't
  touch any body"), rather than doing nothing silently.
- **`FeatureId::get` and `Id::get`** give the numbers face names and wall
  curves carry.
- **A reference whose curves several regions share resolves only among
  them**: its point inside picks one of those regions, and a point in a
  region of other curves (the disc it was cut from moved away) finds
  none, "region not found", instead of that region being extruded
  without a word. With no region of its curves, the point decides as
  planned.
- **The Extrude tool is enabled while any sketch is visible** (or one is
  selected), not only one with regions: finding every visible sketch's
  profiles on each change was more than the button needs. A session
  without regions to pick says so in its panel.
- **Editing an extrude keeps its sketch**: the session only picks among
  that sketch's regions, so `SetFeature` never moves an extrude to
  another sketch, and nothing needs hiding. References the edit can't
  find again are dropped on OK, not kept failing.
- **Booleans on flat patches first, with exact predicates**: the
  counting skeleton decides flat operands with exact signs and symbolic
  perturbation (`A`'s vertices moved along their normals, out for a
  union and in otherwise, then by two generic translations), rather than
  plain floating point: with rounded decisions, flush boxes (the commonest
  CAD boolean) came out as zero-thickness slivers instead of clean
  results.
- **`KernelError::Boolean(BooleanError)`** is new: `Inconsistent`,
  `Degenerate` (the planned `InsideOut` went once `check` covered
  orientation, below).
- **Winding numbers are propagated along edges** from one ray per
  connected part rather than summed for every vertex: the same numbers
  by the counting identity (and checked on every edge), without a ray
  through the whole other solid from every vertex (with the float filter
  not allocating, counting the big tori went from 0.65 s to 0.1 s).
- **Cut faces are ear-clipped, then flipped towards Delaunay**, not
  triangulated by `spade`: flush operands give loops whose points
  coincide, which a constrained Delaunay triangulation merges.
- **A clean-up collapses and flips degenerate triangles** before repair
  (repair only splits, and fails at once on flat pieces breaking the hull
  rules). Collapses keep the lower vertex id and the link condition; they
  remove edges of zero length and never identify separate vertices.
- **Operands face out by `check`, not by a test of the boolean's**: the
  booleans used to test each operand's volume sign (every patch a unit,
  32 per patch integrated) and fail `InsideOut`, which only caught a
  wholly inverted solid: a stray inverted shell, or an outward one nested
  in another, passed and gave wrong results. `check` now covers every
  shell's sign and nesting (invariant 5), so the booleans trust their
  operands; `BooleanError::InsideOut` is gone, a winding number out of
  `0..=1` in the counting is `Inconsistent` (the only way left to get
  one, as with nearly tangent side-by-side cylinders), and the result's
  check charges the patches it integrated. The seeded suite and a chain
  fuzzer (60 chains of 15 app-like steps) gave the same results before
  and after, bit for bit.
- **`Bvh::hits_within`** generalizes `pairs_within` to any query box.
- **Curved shadow crossings are derived from ray tests**, not taken from
  solving each pair of projected conics: `I(e, h) = ρ(b, h) − ρ(a, h) −
  ρ⁻(c, e) + ρ⁻(d, e)`, so every face pair's ends balance whatever the
  numerical answers (see "Curved primitives"). The quartic is still
  solved, for where the crossings are and which edge is above.
- **Layers above a vertex and edge crossings on curved patches are found
  by subdivision and Newton's method**, not by a closed-form 2×2
  quadratic system or a degree-8 polynomial, and then made to fit the
  counts (a vertex's layers add up to the patch's shadow's winding number
  round it; an edge's crossings to the counted `x`). Edge–face pairs that
  may pass in and out again are searched with a count of 0 too.
- **Pairs are refined by refining both whole operands and counting
  again**, not pair by pair: every new vertex and edge gets its own
  primitives through the same counting. Besides normal cones apart, two
  planar patches and (with no ends) hulls apart certify no loops, and
  pieces flat within the resolution count as planar, which stops
  refinement at tangencies well above the floor.
- **Ties in curved primitives** are heights within a 64th of the
  resolution, decided the way `A`'s perturbation would move things,
  order by order (its first order, then the two translations).
- **Near ties are ties for the exact predicates too** (`sign_tied`):
  within the same tie distance, a deciding predicate's constant term is
  taken as zero, and then every later coefficient that is only rounding
  (within what moving every number it is worked out from by `2⁻³²` of
  itself, but by no more than the tie distance, makes of a zero:
  `exact::Moves`; the plan's measure, the coefficient on its terms'
  absolute values, grew as `|x|²` and far from the origin skipped real
  orders, refusing 390 of 2 700 turned pairs moved by up to `1e5`, none
  now). The plan has flat
  operands decided exactly; with rounded coordinates (turned and moved
  flush solids) exact signs broke the perturbation's intent, and beside
  curved operands they disagreed with the curved primitives' ties.
  Flat operands whose near ties don't fit together are decided again
  exactly (`Inconsistent` only; `touches` too, which the plan for the
  retry didn't name). For parallel shadows the plan had a second scale
  for `Height`'s first order; the rounding rule already decides that
  order, and what was left was the constant term at a scale that
  rounded to 0: `Height`'s scale is floored at its own rounding
  instead. The curved primitives' `first_sign` skips orders that are
  only rounding as well (relative to `Σ |d_i·f(e_i)|`, not a `Moves`
  evaluation: `f` is a closure in floating point, and linear in a
  motion, a direction, so with no tie distance to cap).
- **Two patches on one quadric, and a plane against a cylinder patch
  whose normals keep within a half-space, are certificates** of no hidden
  loop, beside the plan's normal cones apart.
- **The clean-up flips slivers on plane faces towards Delaunay**, across
  the pieces of different input triangles, which cutting each triangle
  on its own can't do (see "Clean-up").
- **The clean-up merges coplanar faces along curves** where it can (a
  curved edge between two triangles in one plane is flipped away), and
  moves triangles a collapse gives an off-surface curve to their face's
  copy claiming no surface.
- **Fitted cut conics lie in the plane bisecting the result's crease**
  (spanned by the cut's tangent and the sum of the result's outward
  normals), not the curve's osculating plane, and take the weight of the
  patches' own curves between their ends where that stays within the
  tolerance: otherwise the hull rule fails across the cut and the bands
  leave the surface (see "Cutting curved faces").
- **Exact bands on quadrics are chosen over a spanning tree** of each
  cut face's triangles, rooted at one with a ruling; the common-point
  construction gives each triangle its edge to its parent. Triangles it
  can't make exact, and fitted bands, go on a **copy of their face
  claiming no surface** (same name), so the face tags stay true claims.
- **Cut faces are cut in rounds**: curves whose triangles fold or stray
  are halved (cuts' chains, and operands' curved edges by a vertex added
  on them that both faces get), with Steiner points where two arcs of one
  curve meet, as the extrude's caps are mended; planar patches with curved
  edges are triangulated in their plane with straight inner edges, curved
  patches in their domain.
- **Crossing positions are solved again** on planes (exactly) and
  quadrics (Newton), and curved edges are split by blossoming, for the
  exact cuts.
- **Faces of `B` join no two vertices of one cut by a diagonal**, so the
  two faces of a pair can't both add the same edge.
- **The clean-up carries curves**: it never collapses a curved edge nor
  flips across a curve, and a collapse keeps the cut's curve where two
  sides merge.
- **Merging over-refined patches restores nodes of the pair decisions'
  refinement tree** whose pieces all came through whole; there is no
  merging of the result's own triangles (nor of repair's splits).
- **`MAX_TRACE_STEPS` is 4096** per cut, in `lib.rs`; a cut that runs out
  falls back to one conic along its ends' tangents, or a straight edge.
- **The kernel has its own trig module**, `varde_kernel_compat::trig`, beside
  the sketch's `angle` module, rather than one helper shared by both: the
  sketch doesn't depend on the kernel, and both wrap the same `libm`
  functions, so they agree to the bit. A crate of its own for a dozen
  one-line wrappers, or the sketch depending on the kernel, wasn't worth
  it. New kernel code takes angles only from `trig`.
- **Orientation is invariant 5, before the face tags (now 6)**, run in
  every build and kept out of `check_embedding`, repair's check.
- **The nesting rays are perturbed, not retried**: the ray's start is
  moved symbolically (the booleans' `exact` numbers, a fixed `NUDGE`
  then `T2`, `T3`), so no edge or corner is ever grazed, rather than
  trying a list of tilted directions and refusing when all graze.
- **Corner volumes carry error bounds** (Shewchuk's `orient3d` bound per
  triangle, the sum's own), and are worked out exactly where those can't
  tell the sign. A cruder bound (`n·ε` times `|a|·|b|·|c|` summed) was
  too loose to tell the sign of a box 2e7 times as long as thick.
- **A tighter lune bound** (the booleans' operand test, which shared it,
  is gone since, see "Operands face out by `check`"): the
  prism over the convex hull of the control points' shadows on the
  triangle's plane, times two, rather than four times the largest
  offset times the square of the spread. It is never larger, and it
  leaves an extruded plate with 900 holes nothing to integrate.
- **No valid ring has a corner volume of the wrong sign**: four outer
  quarter arcs against a fine hole break the hull rules (the hole's
  wall crosses the outer arcs' hulls), and the extrude splits them until
  the corner volume is positive again. The thin ring's test checks that
  it is told by integrating instead.
- **The inverted-operand tests** of the booleans now assert `Solid::new`
  refuses the operand, since it can't be built any more.
- **A fitted run's shoulder is set by height off the chord**: the
  furthest sample's distance from the chord over the control point's,
  not its projection on the line from the chord's middle to the control
  point. The projection overshoots for a lopsided run (a chord end itself
  projects ahead of the middle) and kept an exact parabola cut into eight
  Béziers from being one conic; at a conic's shoulder its height is
  largest, so the height is exact for a run that is a conic. On the wavy
  corpus it left 2 sketches that extruded before failing against 4 for
  the projection, with fewer segments.
- **A fitted line must run along the spline's tangents**: a run, or a
  single Bézier, becomes a line only if its end tangents are within 45°
  of its chord as well as within a quarter of the tolerance of it; a
  single Bézier that fails only the tangents and fits no conic is still
  a line. Straight within the tolerance alone let two runs of a sliver
  narrower than the tolerance double back on each other (no area, or a
  cusp) where span-by-span fitting kept it a region. It changes no
  refusal on the corpus above (segments at 1e-1: wavy +19, blob +7); of
  173 slivers 0.002 to 0.2 across at a fit of 0.1, those span by span
  extruded and runs refused went from 28 to 2 (two lobes about 0.004
  across, each of their two spans straight within the tolerance and no
  conic's, so two lines; span by span happened to fit one of them with
  conics, walking it the other way).
- **The regen lane's joined scenes are cache entries with one more
  protected key**: the scene of the last answer without a draft is never
  evicted, besides what the request before used, so the committed model
  asked again after any number of draft revisions isn't joined again,
  even with no budget. Scenes are counted in `joins`, not `counts`, so
  `counts` stay feature counts. (They were a slot of two scenes before
  the cache was bounded by bytes.)
- **What the rounds of cutting keep is bounded by the fit tolerance**,
  including a round that finished: every triangle held to it (along a
  cut, from the patch; off a quadric onto the copy claiming no surface,
  from the quadric) must be within it, else `TooComplex`. The plan
  refused only when the rounds ran out with bands along a cut past it; a
  round can also finish with a triangle off its quadric that has no
  side on the face's boundary to halve. Such triangles ask for
  their boundary curves to be halved from half the tolerance, as those
  along a cut do, and the clean-up's moves onto the copies are held to
  the same bound.
- **A crossing only placed is certified against the other operand's
  patches near it**, not only the one it crosses: a crossing at a cap
  edge's end lands a micrometre past the patch crossed, in the next one
  of the same plane, and the plan's check refused it (a boss's rim
  tangent to cap edges between holes). The foot of the perpendicular is
  tried before the search, which ran into its cap on a thin triangle
  holding the point. Without the two, 8 of the 160 drilled plates'
  operations were refused.
- **Crossings only placed go to a root checked on the patch**, by a
  certified distance within the resolution (`solve::near_patch`), not
  by inverting the point into the patch with a slack: that is what the
  check after it asks, and a crossing at a side two patches share lands
  a rounding past either. A root whose tangent lies in the surface (a
  double root) fits either sign. Where no root fits, the placement is
  what it was before (a plane's nearest root, a quadric's within
  `1e-6`) and the check decides, so the change can't add a refusal
  there. Crossings the search solved keep their placement (a plane's
  nearest root, unchecked), except a quadric root further than `1e-6`,
  which is now taken if it passes the same checks. The test switch that
  skipped the check (`UNCERTIFIED`) became `LOOSE`, which also leaves
  crossings only placed where they were, so the backstops can still be
  tested. The bars through boxes were exact to `1e-9` of their size, not
  `1e-12`, until section arcs' weights were taken from the angle; now
  `1e-12`.
- **Section weights by angle on elliptic cylinders too, and the control
  point by angle**: the plan took the weight from the angle on circular
  cylinders only, keeping the control point where the tangents meet and
  a plane within `1e-9` of the rulings giving a straight edge. An
  elliptic cylinder (a wall over an ellipse arc) is an affine image of a
  circular one, so the same formula in its own metric gives its arcs'
  weights; without it a box turned a thousandth of a radian across such
  a wall stayed `5.6e-7` off in volume. The tangents of a nearly
  straight arc meet only roughly and the straight edge was up to
  `5.3e-11` off the cylinder (the plan's measured residue), so the
  control point comes from the angle as well, and on these cylinders an
  arc is a straight edge only between ends on one ruling. Parabolic and
  hyperbolic cylinders and other quadrics are as before.
- **The points of a patch above a vertex keep Newton's method's from any
  piece where the search ran out of pieces**: found while looking at a
  fuzzing refusal (a small cylinder
  `1e-3` through a steep wall), not in the plan. The vertex's winding
  number had a point the search, run out of pieces, didn't find, and
  guessing it above gave every edge at the vertex a crossing that isn't
  there, which the check on crossings only placed refused
  (`Inconsistent`); the operation is still refused, now by the result's
  hull rules (`Invalid(VertexNeighbours)`), with the decisions
  consistent. Only where the search ran out: where it didn't, the
  piece holding the point found it itself, and a second find from
  another piece near a fold (Newton's method converging loosely, two
  finds further apart than they are merged) would have counted it
  twice.
  Finds from other pieces are themselves merged, one per point and
  facing: on a strip of a cylinder seen `1e-4` off its axis they were
  up to ten finds of one point, which the winding number's fit had to
  drop.
- **Empty components by their curved volume**: not in the plan, found
  fuzzing boxes against cylinders. A face along a wall's rulings
  cutting off a sliver thicker than the fit tolerance gave an empty
  `Ok` (`a − b`) or lost the sliver (`a ∩ b`), since the clean-up
  measured components by their triangles' corners, all on the cutting
  plane. The fix turns two of the sliver test's 32 operations into
  `Invalid(Fold)` refusals (slivers on turned boxes); before, they were
  wrong. A box tilted `1e-5` or less off the rulings and tangent to the
  wall at its middle leaves slivers a few resolutions thick, whose
  bands (within the fit, claiming no surface) graze the wall: their
  volume was off by more than their own, `5e-6` on a `3e-7` sliver,
  with every point within `1e-4` of the true surface: the cut round the
  section's tip was a straight chord, which the check on fallback
  chords now refuses (see "Chains").
- **Crossing searches drop pieces whose control hulls are apart**
  (`hull::apart`), as the plan for tall walls has it, by more than
  `1e-12` in the search's unit frame, as planned, plus `2048 ·
  PIECE_SLACK` of the pieces' sizes, which the plan didn't have (so a
  solution Newton would count just past the patch's side or the edge's
  end is never dropped; see "Crossings"; the boxes and the slab are
  given the same margin, which they didn't have before either; and,
  which the plan left as an option for searches still at their cap
  after the hulls, patch pieces more than 8 times longer than high are
  halved across their longest side, not quartered: curved edges whose
  control hulls hold a tall wall's width, a rim arc through a pin or a
  tool's rim along a tall wall, still ran to the cap), and the charge went from 4
  pieces a unit to 2 (a piece measured twice as dear where every piece's hulls
  touch; single thread, release: random edges and patches 0.3 → 1.0 µs
  a piece but 5 times fewer pieces, 0.2 s → 0.13 s for 20 000
  searches; a cap edge across a drill 1 010 tall 1 047 → under 100
  pieces). The test switch `assemble::LOOSE` also turns the hull test
  off, so the backstop tests (two bars through boxes, a cylinder across
  a 75° wall) still see their searches run out of pieces and their
  crossings placed off the surface: with the hulls the searches find
  those crossings and the loose results came back `Ok` and exact.
  Before and after, release (after the exact roots on quadrics and the
  certification of crossings only placed had already won back most of
  them): the drilled 60 × 40 plate, 47 drill positions at 7 heights
  from 10 to 10 000, 44, 45, 45, 45, 45 of 47 from 100 tall up (errors
  `Invalid`) → all 329 (and the box drilled the same, 329 both), 7.2 s
  → 3.5 to 4.4 s of user time; a box notched into a tall cylinder's
  side, 60 of 60 both; the arc fuzzers (walls 300 cases, seeds 21 and
  5; turned tools 200, seeds 21 and 5): walls unchanged (65 and 59
  refused, none wrong), turned 17 → 17 and 16 → 13 refused; the
  seeded suite unchanged (related 112/120, chains 203/240, turned
  156/160, tangent 72/96, coaxial 37/40, bosses 64/64, drilled
  160/160).
- **Fallback chains are checked against the true cut, and the other arc
  of a plane section is tried** (see "Chains"), as planned, with one
  addition: where the conic along the end tangents fails the check, the
  straight edge between the ends is checked too and kept if it passes,
  before the boolean is refused. Before and after, release, default
  tolerance: grazing planes against cylinders of 3 to 8 arcs (tilts 0
  to 0.5 radian off the rulings, two thirds of the offsets within a
  fraction of the radius of tangent; seeds 1 to 4, 200 cases each, 3 200
  operations), `Ok` 2 200 → 2 180, refused 1 000 → 1 020
  (`Inconsistent` 65 → 160), `Ok`s off by more than `1e-7` in volume
  16 → 2. 18 `Ok`s became `Inconsistent`: 14 of those 16 (up to
  `1.1e-5` off, among them the sliver that lost its tip), and 4 within
  `1.5e-7` whose chords were up to `1.2e-3` from the cut, or within
  `2e-6` of both surfaces where they meet at `1e-6` radian (so up to
  1.9 from the cut, by Newton's method). Trying the other arc turned 6
  `Ok`s into `Invalid` or `TooComplex` and 4 refusals into `Ok`s (the
  hull rules seeing exact cuts where they saw fitted ones). The two
  left off keep the cylinder at a tilt of `1e-6`, within `1e-12` of
  tangent: `4.9e-7` and `2.4e-7` on volumes of 92 and 65, inside the
  sliver's depth, unchanged. Walls over arcs (300 cases, seeds 21 and
  5) unchanged, 65 and 59 refused, none wrong; tools turned across
  them (200 cases) 17 → 15 and 13 → 13 refused. The seeded suite
  unchanged (related 112/120, turned 156/160, tangent 72/96, coaxial
  37/40, bosses 64/64, drilled 160/160, walls 112/120, bars 22 of 24 at
  each of three seeds) but for parts built in chains, 203 → 201 of 240:
  three operations refused at two steps, cuts between cylinders
  crossing at a slant whose tracing failed at a point where the cut
  touches itself, and whose straight chords were `3.6e-2` and `1.6e-2`
  off it (36 and 16 times the fit), which the suite's volume checks (a
  fifth of the fit times the area) let pass; one operation a step later
  came through, fed on another part.
  In app-like cuts (a plane half a radian off a boss's rulings, 24
  offsets) the plane–cylinder chains not exact go from about 40 of 48
  operations to none.
  Bug hunt, release: grazing planes, seeds 1 to 8 at fits `1e-3`,
  `1e-4` and `1e-5` (6 400 operations at each), checked by volume, by the
  identities and by the result's extent along the axis against the true
  sliver's (sampled along every patch side): none wrong. Against the
  kernel before the change, at `1e-3` seeds 5 to 8 and `1e-4` seeds 1 to
  4, 26 and 34 `Ok`s became `Inconsistent` (volume errors before from
  `1e-12` to `1.1e-5`), among them both of the old kernel's wrong `Ok`s
  there (a sliver's tip dropped, its extent short by 0.5 and 2.2). Every
  plane–cylinder chain was measured against the true section both ways
  (where the plane is at least `1e-5` off the rulings): exact arcs,
  either guide, within half the fit. The other arc can lie partly off
  the patch (its middle within the `−0.25` the test allows, its end
  `0.36` out), seen only where a plane touches along a ruling and the
  arc's end itself is off the patch, the operation refused. Crossing
  cylinders near tangency (square axes, the gap or overlap `0` to
  `1e-3` of the radius, at fits `1e-3` and `1e-4`, 900 cases, the half
  not moved measured): each chain kept from a fallback was within half
  the fit of the true cut loop, both ways; all of them were in
  differences. Boxes nearly parallel to the caps: none wrong but slivers under
  the resolution, as before. 1 and 8 threads give the same bits on all
  three. Parts built in chains (seed 5, 15 chains): 10 of 1 200
  operations refused that were right before (their first chords between
  cylinders `3e-3` to `3e-2` off the cut, which the rounds used to halve
  onto it).
- **`touches` doesn't refine.** On curved operands it counts once and,
  where that shows no meeting, searches for surfaces within the
  resolution (`boolean/near.rs`, "Touches") instead of running the
  boolean's refinement to the end. Its charge is a unit per visit (the
  plan guessed one to two, measured 0.24 to 0.44 µs a visit). This also
  covers the plan of stopping `touches` at the first round whose
  counting shows a meeting: it counts only that round. A loop no edge
  crossing shows still counts, found by the search (it used to run the
  pair decisions for that). The search is `pub(crate)` with a visitor,
  for the minimum distances to bound.
- **Seams: straightened first, regions as the fallback, faces merged by
  the seams that joined them.** The plan for flush unions blowing up was
  to triangulate again each region of triangles in one plane linked
  across seams. Measured, nearly every seam needs no more than making
  it its chord (both triangles in one plane, the lens moves from one to
  the other), so that runs in the clean-up's rounds, before the flip
  `unbend` tries, and the regions run once after as the fallback, for
  clusters of zero-size triangles at a rim (grown across short edges and
  flat triangles too, not across seams alone). "In one plane" is tested
  on the neighbour's geometry, not its tag: a seam's other triangle is
  often a remnant of the wall at the rim, tagged with the cylinder,
  which then moves onto the plane face. A straightened triangle must
  also pass the fold check (not in the plan; cheap, and only seams it
  can't straighten are left to repair). Merging the plane faces is done
  here, as a union of the faces the mended seams joined, onto the
  lowest id; a general pass merging any adjacent faces of one surface
  after a boolean (coaxial walls, plane faces meeting along straight
  edges) isn't built yet and would take this over. The tests are in
  `boolean/curved_tests/flush_seams.rs` (a new module beside
  `curved_tests.rs`, which is long), with patch bounds from the measured
  counts with slack rather than twice the plate-first count; the seeded
  `flush_unions_either_order` holds unions to 95% and all four
  operations to 86% (what fails there are intersections and
  differences keeping a cap corner where a straight side runs on into
  an arc at a tangent, as before). The clean-up no longer keeps a
  record left from an edge no triangle has where a new side runs
  (`flip`, `unbend` and a collapse moving an edge onto the vertex kept;
  `delaunay` already dropped it).
- **Topology and names, as built** (the solid-operations plan's names,
  keys, aliases and resolving): `FaceKey`'s fields are public (they are
  deserialized from files anyway); `FacePart` keeps its order (`Split`
  stays fourth) with the new parts after it, and `PartKey` follows it.
  Resolving lives on `Topology` and takes keys and a point; the
  document's `FaceRef`/`EdgeRef` (with the body) come with the features
  that store them. A failed reference is `topology::NotFound`, not a
  `KernelError`. Candidates are measured by a linear pass over their
  patches nearest box first, not through the BVH, with one search
  allowance shared by the whole resolve. A corner matches when each of
  its three keys names some region there (two keys may name one region
  through an alias). Aliases are recorded by the flush seams' face merge
  (whole faces) and by `unbend` for the triangles it moves (the moved
  face may live on elsewhere under its own key: then a key names both,
  and the point picks); the general merge pass (one surface, one face,
  `Mesh::merge_faces`) records them on the mesh after repair. Not in the
  plan: flush caps one of which the perturbation drops whole (no merge
  happens, so no pass records it) are aliased after the boolean where a
  triangle middle of the dropped face lies on a result face of its plane
  or one of that face's on it (`boolean::covered`); names only, never
  the geometry. Ties between candidates go to the lowest index within a
  billionth of the solid's size, not only exact ones.
  Transforms don't exist yet; the table is on `Mesh`, so they carry it
  by keeping the faces.
- **Fitted strips are least squares, halved by what station 0 says.** The
  plan fitted the diagonal by Gauss–Newton until every sample was within
  half the fit tolerance, halving either way. Built: Levenberg–Marquardt
  on the signed distance at a grid of points, then the worst error
  *measured* by climbing from the grid's peaks (`deviation`, a found
  maximum, not a certified bound: accepted up to half the fit tolerance
  less a 64th of it), and
  halved along the meridian or round the axis by which gains more at
  station 0 (round the axis is the caller's: the band asks for a halved
  lathe). The tori come out with fewer patches than the plan estimated (768
  at the default tolerance against about 1 000, 2 560 at `1e-4` against 4
  000).
- **Rings at turns: the cylinder rule.** Not in the plan, found building
  tori: where a meridian's tangent is square to the axis the surface
  touches the parallel's plane all along it, and the edge rule (a plane
  through the shared curved edge's control points with the two sides on
  opposite sides of it) can't pass there, so `check` refused every ring
  at a turn: a full circle split at its top, a flat face meeting a round
  tangentially, two arcs tangent at a top. Repair rescued some only at
  coarse fits and absurd cost (a puck at `1e-2`: 64 → 80 976 patches) and
  ran out of budget from `1e-3`, so every revolve of a profile with a
  rounded edge would have failed. Bands first kept their own rings off
  turns (they still do, being cheaper), but a profile vertex at a turn is
  the boundary between two faces and can't move. Fixed by a second
  certificate for curved edges in `check` and repair: the two patches on
  opposite sides of the cylinder over the edge's conic, by the signs of
  `F = λC² − 4w²·λP·λQ`'s degree-four Bernstein coefficients (see
  "Control hulls"). A reviewed alternative, one side in the plane and the
  other's control points under it with the fitted diagonal held in the
  tangent plane, covers only a flat side against a round (not two rounds,
  an S or a concave fillet) and needs a constrained fit: not instead of
  the cylinder, but revolve found it needed beside it (a wall straight
  down from a concave round's bottom stands on the cylinder, so only the
  plane can part them, and only with the band's diagonal in the plane),
  so bands now hold the diagonal there where a piece ends at a turn
  ("At a turn" under "Fitted strips"). A
  pencil of the plane and the cylinder (`α·F + β·plane·W`, a two-variable
  LP) would cover creases where both sides leave the edge the same way
  at different angles; revolve's creases needed it, and it is now the
  third edge rule ("The pencil" under "Control hulls"). Bands and caps keep the
  plane rule alone (with the cylinder they chose coarser strips that
  failed the vertex rule). An existing test's pair, two patches across a
  sideways curved edge, now passes (it really is embedded); the folded
  case is one curved up out of their common plane. With the cylinder,
  bands leave a turn within `1e-2` of a piece's end (in its parameter)
  to the ring there instead of balancing it (`TURN_NEAR_END`, before
  `1e-6`): the piece over the turn that balancing cut was a sliver
  (`2·1e-5` of a quarter arc turning `1e-5` rad past its end), which the
  band halved round the axis until `TooComplex`, so a profile whose arc
  went `3e-6` to `3e-4` rad past a turn was refused at every fit.
- **Caps don't grow the angular split.** The plan raised `k` when a cap
  wanted an arc halved; halving the cap's meridian toward the pole alone
  brings it within the tolerance (the error falls as `δ²` on a sphere,
  as the length on a cone), so caps only halve their meridian, and the
  rest of the meridian comes back in pieces growing at most 16 times a
  ring.
- **Lathe calls take a `Budget` each.** `fitted_band` and `pole_cap` start
  their own `Work` from the budget; revolve, which makes many of them in
  one operation, should share one through `fitted_band_with` and
  `pole_cap_with` (crate-internal, taking `&mut Work`).
- **One surface, one face names faces, it doesn't merge them.** The plan
  moved the members' triangles onto the lowest face and dropped the
  rest, giving them its tag. Done that way, an extruded circle's four
  arcs claimed the first arc's quadric, written in its coordinates, and
  a boss on a plate drilled with eight holes round its rim came back
  `Inconsistent` (`boss_rim_tangent_to_cap_edges_between_holes`). So
  the pass changes names and aliases only: each member keeps its entry
  and surface, takes the root's name unless it already has its key, and
  gets the set's aliases on itself (a later boolean may cut the root
  away and keep a member). Validation measures against the widest
  member's surface, not the root's, and leaves out only the members off
  it, not the whole set (see "Structure"). The pass runs on the finished mesh, not on
  the clean-up's soup, so aliases go straight onto the mesh rather than
  through `Soup::absorb`. The clean-up's seam merge (`merge_joined`)
  stays: the Delaunay flips after it flip only within one face index.
- **The plane faces' quality pass departs from its plan in five ways.**
  The bound is 5°, not 10°: measured on the nine 60-hole grids, 5°
  failed fewer steps (none, against 1 at 10° and at 20°) with a third
  fewer patches than 10° (half of 20°), and 10° lost bosses sunk
  through drilled plates and flush unions to their patch counts. The
  queue takes the worst sine first, not the smallest circumradius, as
  the points for shapes on curved layouts do. A face where some
  triangle's curved corner is closed isn't skipped whole (a 64-hole
  plate's top always has one): its closed corners are flipped open
  where they can be, and only points near the curves that stay closed
  are refused. Three steps were added, each for a case that looped or
  cascaded: inner vertices of plane faces at bad triangles are taken
  out where their stars triangulate better (seam leftovers a tenth of a
  millimetre off a cap's edge), the made triangles are flipped towards
  Delaunay first (thin right triangles otherwise split into copies of
  themselves), and every point is checked against the diametral circles
  of the constrained sides of the triangles it splits, a free side's
  halving point too. And triangles' corners along curves are judged
  from the curve's tangent: a corner the chords made 23° wide but the
  tangent 1.85° came out of one boolean and, halved by the next one's
  refinement, closed (a flush boss on a drilled plate). The shapes
  points' code in `triangulate.rs` works on one input triangle's layout
  with its sides fixed; this pass shares its in-circle test, the bound
  and the circumcentre (`mesh/shape.rs`, with the extrude caps' quality
  refinement), and the seams' triangulation of a region, but
  walks and flips on the soup, where a side between two plane faces can
  be split in both.
- **Revolve as built.** `revolve(profile, frame, sweep, feature, tol,
  budget)` takes the extrude's `Frame` (`y` the axis) and `Sweep::{Full,
  Part { from, to }}`; a part turn's angles must be within `8π` of `x`.
  Faces are flat where a line's ends are within the resolution of one
  height, cylinders (as a form; the strips are the cone's) within it of
  one radius, spheres where an arc's centre is within `m·w/4` of the
  axis (`m` the resolution, `w` the arc's weight): choices of a face's
  kind, its tag checked, like the extrude's straightening. A segment
  coming within the resolution of the axis inside is refused in part
  turns too (the plan refused only a vertex alone, in full turns), and a
  part turn too close to full is `ProfileError::NearlyFullTurn` (new,
  with `CrossesAxis` and `TouchesAxis`). Where the ends' caps want a
  profile piece halved, that piece is halved and the round runs again
  (the plan grew `k`, which halves only round the axis); flat faces'
  caps wanting a ring's arc halved grow `k` as planned. `Lathe::turned`
  keeps points on the axis (as `on_axis` has them) where they are, so
  meridians ending at a pole keep it to the bit at every station. Bands
  hold a fitted diagonal in a turn's plane where a piece ends at a turn
  ("At a turn"), which the groove needed. The faces' and ends' caps
  are the plain caps (`Mode::PLAIN`, then `Mode::FLAT_CORNERS_PLAIN`),
  without the extrude's quality refinement.
- **The fold rule falls back to the clean-up without it.** Its plan
  kept the rule safe by running it last and only where the area round
  the vertex drops; on faces a hair off each other that still lost a
  result the clean-up without it gave (1 in about 440 000 steps of
  chains on such frames), so a result that fails the check after the
  rule fired is made again without it ("Unfold"). The soup is copied
  before every clean-up for it. This fallback is inside each of the
  booleans' tries (`checked_with`), the one joining walls' lines and the
  one without the joins ("Tried again without the joins"), so a try's
  error is the one its clean-up with the rule gave.
- **Scales as built.** `Motion::scale` takes factors within
  `1/MAX_SCALE ..= MAX_SCALE` (`1e6`), a kernel bound beside the
  feature's `1e-3 ..= 1e3`. Where the plan made every circular cylinder
  scaled per axis a cylinder over an ellipse and every cone a quadric, a
  cylinder or cone the scale keeps round (equal stretches across its
  axis, a cone's also square to it) keeps its circular form: a
  `ConicCylinder` is never over a circle. A cone's `Form::Quadric` is
  both nappes. `slack` multiplies on every face scaled up, not only
  fitted ones (an exact face is on its form to rounding, so it costs
  nothing), by the motion's stretch, which for compositions is the
  product of their stretches (an upper bound of the largest). Picking
  summarizes a `Form::Quadric` as `Other` ("Curved") for now.
- **Creases: the pencil rule.** Edge neighbours were to be parted by a
  plane through the shared edge; the cylinder over a curved edge's conic
  came second (rings at turns, above), and revolve's creases into one
  quadrant of the meridian plane needed a third certificate: a member of
  the pencil of the two, `α·F + β·P`, found by a two-variable search and
  checked with its own rounding bounds ("The pencil" under "Control
  hulls"). The three-member family `α·F + β·P + γ·P²` (a free curvature
  in the meridian plane) would also part thin creases where curvature
  decides; it changed nothing on random revolves, so it waits for a
  case that needs it.
- **Measuring's curve pieces and box search go further than the plan.**
  The plan integrated a conic's length over pieces whose weights are
  within `0.97..=1.03`; that left `1.6e-7` on lopsided pieces of nearly
  half ellipses, so pieces also turn at most 45° and keep their legs
  within a factor 1.2 (see "Volume, area and measuring"). The box's
  search runs Newton's method during the search rather than only at the
  end (330 000 visits down to 2 400), and skips patches on planes and
  ruled quadrics, whose extremes are on their edges.
- **Minimum distances go further than the plan's hulls and Newton.** The
  plan bounded pairs by their hulls' distance alone, over the broad
  phase's pairs. Coaxial walls (a tube's, a pin in a hole) are equally
  far everywhere, and a pin's wall from the hole's rim, so hulls only
  drop their pairs once split to `√(8·eps·r)`: 408 000 visits for a pin
  through a tube, refused at the finest tolerance. So rounds bound pairs
  too (the distance from a cylinder's axis or a sphere's or circle's
  centre, ranged over a piece by its Bernstein coefficients). The broad
  phase is a descent of two box trees in the search itself, since a
  margin from a first bound made `n·m` pairs for bodies far apart.
  Newton's method steps back up from edges it overshot onto, and runs on
  a bounded number of pairs within `eps` of the best, so answers are to
  rounding in practice rather than within `eps`. A body's distance is its
  surface's (a body inside another is not 0): the simplest reading, as a
  containment test would need the booleans' counting.
- **Walls along one direction are certified by nearness, not from a
  round on.** The plan certified such pairs with no ends from round 2
  of `refined` (a gate measured from 2 to 4), because certifying them
  from round 0 lost results: refinement of walls that don't meet was
  helping later stages. Diagnosed, those were all caps' flat rings
  between two parallel walls a little apart (`flush_rims_take_the_halved_cuts_vertices`:
  an ear with three corners in line and a quarter arc for a side,
  which folds; `stacked_cylinders_a_step_apart_keep_the_step`,
  `a_cylinder_inside_a_larger_one_over_one_span`, seeded `related`
  coaxial walls 0.0038 apart: ring triangles failing the hull rules
  against unrefined walls bulging across the ring), which need the walls
  refined until their hulls part. No round gate keeps them all: at
  round 2 `related` lost 2 (113 → 111) and the stacked step ran out,
  from round 4 the step still failed, and from round 5 a tangency's
  refinement no longer fitted 200 000 units. Certifying only walls
  that come within 64 resolutions of each other keeps every refinement
  that parts hulls (walls further apart) and stops a tangency's: every
  test passes, every tally holds (`related` 113, bosses in drilled
  plates 148 → 149), and the probes match the plan's gate 2. Rings
  narrower than 64 resolutions between such walls are left to fail.
- **Ends along one direction join where the crossing is clear, not by a
  grouping distance from the larger radius.** The plan grouped a pair's
  ends within the planarity size `√(8·R·resolution)` (`R` the larger
  radius) and left closer lines to refinement. That sees the other line
  only when the same pair holds it, and the larger radius undercounts
  a pin nearly the size of its hole (relative curvature `1/r − 1/R`).
  Each end's crossing angle bounds the sliver beside its line wherever
  the other line lies, so the test is `θ² ≥ 2·LENS·resolution·κ` with
  `κ` the sum of the curvatures, and ends group within `θ/κ`. At
  `LENS` = 2 it is the plan's distance for equal radii; measured from 2
  down to 1/16 on 384 overlaps of 0.02 to 30 resolutions, no value down
  to 1/8 turned a result into an error and every one below 2 worked
  more (177 at 2, 194 at 1/2, 214 at 1/4 and 1/8, 229 at 1/16, which
  turned one). The tangent tally holds from 8 down to 1/32 and every
  tally at 2 and 1/4; at 1/2000 the tangent tally fell from 72 to 70,
  as with the plan's 64-resolution prototype. `LENS` is 1/4. On wider
  probes it does turn some results into errors, far fewer than it
  wins, and most of those come back from a second try without the
  joins.
- **An operation that joined lines and fails is tried again without
  them.** Not in the plan, which joined or split each pair once. The
  second try is bounded by the first's work (at least 150 000 units),
  so a refusal takes at most about twice as long; it wins back all but
  a few of the results joining lost ("Tried again without the joins").
  The rules tried before (a larger `LENS`, steep crossings left
  to refinement) lost as many results as it won back.
- **Every `Solid` is refused with a triangle facing against its face's
  plane form or plane tag** (`check`'s step 6, `FacesAgainst`), in every
  build, where only debug builds checked forms: caps a tie apart gave
  such results (see "Forms"), one of them only with walls' lines joined,
  and flush faces a hair apart one against its tag. The plan for the
  tags added the test to `check` for tags only, beside the booleans' own
  look at the forms; the forms joined it, one test in every build for
  every producer, so debug builds refuse these results as release ones
  do instead of panicking first. Both cases seen were on faces claiming
  their plane; faces claiming no surface (copies of faces with triangles
  off their plane, inputs moved without their tags) keep the plane form,
  which only a test of forms sees.
- **In-plane crossings also for edges with one end in the plane.** The
  plan kept only crossings of edges with both ends within the tie of a
  flat face's plane inside its triangle. An edge with one end tied and
  the other within the resolution is kept inside as well, where some
  part of it is inside, and left where rounding has it where none is
  (never `Inconsistent`, so the curved path loses nothing): such an
  edge, nearly along the plane, gave the one result the hunts still
  refused for facing against its tag ("In-plane crossings").
- **The flat primitives' broad phase pairs boxes within the
  resolution** when they decide near ties as ties (0 when exact). Their
  margin stayed 0 from before they took near ties as ties, and chained
  small boxes gave a wrong empty intersection and a whole difference
  ("Counting").
- **Results that touch themselves are named by a second rule too, and
  a classification out of budget keeps `Invalid`.** The plan named a
  failed result `NotManifold` only where its mesh before repair has two
  vertices within the clean-up's short length. Tangent unions of
  curved operands (a disc joined beside a round body, touching along a
  line) leave both shells uncut and fail `Hull` with no near vertices,
  so a `Hull` failure between separate shells is named so too (see
  "Results that aren't manifolds"). Where telling runs out of the
  budget, the plan would give `TooComplex`; the error stays `Invalid`
  instead, equally bounded, so out-of-budget classification never
  changes which kind of failure the measurements count.
- **Corners of 180° at tangencies are mended by collapsing the pair of
  crossings, not by flips or Steiner points.** The plan blamed the
  clean-up's collapse of a zero-length side for leaving two arcs of one
  smooth curve at a corner, and proposed judging corners across such
  sides when triangulating, or flipping or adding a point with straight
  inner edges on plane faces after the clean-up. Measured, the cases it
  named (a cylinder inside a plate touching its side, intersected) work
  at unit size and the default tolerance already; they fail where the
  two crossings the double root leaves are further apart than the short
  length (at the finest tolerance, or in millimetres), and then a strip
  of the touched face between the wall's two halves fails the hull rules
  too, which no flip or point on the top mends. So the clean-up
  collapses such an edge up to four resolutions long where it leaves
  that corner or that strip, moving a vertex only onto a point on all
  its faces' surfaces ("Collapse what a tangency leaves").
- **Unions touching along a line are named by the decisions for any
  ends whose walls aren't clear and face opposite ways, with a slit
  where they bend into each other**, not for a
  group of four ends within the tie distance. Where the line falls on
  the patches' edges (on the seams, the probe's placement at `z0` 0.5)
  a pair holds two ends, and the four-end rule named one of the probe's
  13 runaway unions; a union of walls overlapping by under a quarter of
  the resolution, whose lines aren't clear either, ran out the same way.
  So the test is the joins' own clearness at every end, with the faces'
  directions telling solids touching from either side (no manifold)
  from one inside the other (a manifold union, refined as before), and
  a slit at least a sixteenth of the resolution deep telling walls
  bending into each other with a gap from a pin plugging a smaller hole
  it touches inside (a manifold union too: the counting's ties gave its
  pairs ends, which were named so until the slit was asked).
- **Coaxial cuts are certified, not only made exact.** The plan asked
  for two quadrics of revolution on one axis to be cut in parallels
  exactly; without a certificate their pairs were still split until
  their normal cones parted (1 412 patches for a cylinder and a cone,
  two of four results `Invalid`), so cylinders and cones on one axis
  whose meridians cross at slopes at least `1e-3` apart are certified
  and their ends joined round the axis ("Coaxial walls"). Spheres take
  the exact parallels but not the certificate (their meridian isn't
  linear in height, and a sphere can touch a cylinder along its
  equator). Certified with no ends however far apart, these walls leave
  a cap's ring between them whole, so curved sides bulging over the
  loops are split (planar layouts), which the plan didn't have either.
  Both the certificate and the parallels are a shortcut a failed
  operation is tried again without, so they lose no result the
  traced cuts got ("Tried again without the joins").
- **The curved path is decided again without ties too.** The exact
  retry was planned for flat operands only, the `Flat` inside `Curved`
  keeping the curved primitives' ties, and the curved path left to
  fail on `Inconsistent`. Once ties were measured in space, a few more
  curved operations came out `Inconsistent` than before (5 to 8 a hair
  run), mostly where an exact tie (a box's edge a tie off a cylinder's
  ruling) met a search that ties nothing. Deciding the whole curved path
  again with `tie` 0, exact and numerical primitives together, keeps
  them on one tie and wins nearly all of the curved `Inconsistent`s
  back as results or named refusals, with no wrong result in the hunts
  ("Decided again exactly"). It runs inside each of the tries with
  fewer shortcuts, within their caps ("The exact retry inside the
  shortcut retries").
- **Repair and the check near a boolean's change keep the results and
  errors bit for bit, and the solids keep their resolution.** The plan
  verified the kept triangles once, when the mesh is built, and let the
  check select pairs as repair does. Here `tested` (`mesh/intact.rs`)
  verifies them on the mesh repair is given and again, independently,
  on the repaired mesh the check gets, and the pairs of both first
  passes come in the order testing everything gives them, so the
  failures found and the error named don't change either (the boolean
  names some failures `NotManifold` from the pair the check names).
  `Solid` keeps the resolution it was checked with, which the plan left
  out: an operand checked at a finer resolution passed hull rules a
  coarser one may break, so only operands checked at the operation's
  own resolution count as checked. The check's passes over the whole
  mesh (topology, bounds, orientation, face tags) stay, charged a unit
  a patch (`CHECK_WHOLE_WORK`), which the plan's "≤ 8 units a patch"
  didn't count: a small cut of a large plate now takes about 8 to 10 a
  patch (17 for a pin in the smallest plate measured, 2 060 patches).
- **The clean-up visits classes of triangles, not the triangles a
  boolean changed.** The plan seeded the clean-up's rounds with the
  triangles that aren't an operand's kept whole and those a change
  reaches, and planned a harness to show that the clean-up over every
  triangle changes no kept one. Here each pass visits the class of
  triangles it can change at all, by their own state and their
  neighbours' (see "Work local to the change" under "Clean-up"), worked
  out by one cheap pass over every triangle and kept in step: the same
  results bit for bit whatever the operands hold (an extrude's cap can
  hold slivers the Delaunay flips take up), so the harness wasn't
  needed; debug builds assert the classes instead. That first pass is
  charged a unit for every 16 triangles, and the members of the classes
  an operand brings along are visited too: the plane triangles with a
  side that has a curve (the caps round the holes of a plate) once by the
  quality pass's closed-corner scan, about 7 700 of the 70 650 units a
  small cut of the 20 × 20 plate of round holes takes. Dropping the
  empty components integrated every curved patch of the result; it now
  integrates only a component its bound can't tell (not in the plan:
  it was most of a boolean's time on a curved body, and charged
  nothing). `faces_out`, whose unit a patch the plan wanted charged once,
  is no longer in the code. Rounds that change so much that keeping the
  classes costs more than visiting every triangle give them up to the
  loop's end (found while hunting bugs: a rise into `TooComplex`), so a
  clean-up's rounds never cost much more than visiting every triangle
  each round did.
- **An operation the seam rules changed is tried again without them
  where it fails the check.** Not in the plan, which took the
  first-order measure and the trace's start into both patches alone and
  left the operations they lose (2 in its sweeps, 14 once rebased onto
  one tie measure: refusals on a near tangency's thin triangles,
  `Invalid` or `NotManifold`) as the thin-triangle class. Their
  decisions differ from the old rules' only at near ties, where either
  is a configuration within the tie, so the result without them is as
  good: the operation is tried again without the rules where they
  changed a decision and every try failed so (or ran into a bound),
  within three times the
  work the tries took ("Tried again without the seam rules"). All 14
  come back and none are lost against the build without the rules; in
  the hunts off the family (revolves, cones, skew crossings, turned
  flush shapes) none either.
- **Align's signs and options.** The plan had a straight edge run "from
  the end whose corner's sorted keys come first"; it runs as the
  existing edge references do (the first key's face on its left seen
  from outside), since a chain can end where no corner is and the rule
  was already the revolve's and the move's. A round edge's axis
  follows the plan (a flat neighbour's normal, else a round
  neighbour's axis), which differs from what a move's turn about a
  round edge takes (the circle's axis as the edge runs): align puts a
  pin in a hole face to face by it, and the move's stays as it was.
  Ellipses (rims after a scale per axis) give centres and axes too. The
  offset and the turn go along and about the target's primary as
  given, not as flipped. The fixed direction for the smallest rotation
  takes over within `1e-8` of parallel or opposite, not only at exactly
  opposite, where the cross product is rounding.
- **Split built above a stand-in.** The split feature's document and
  regeneration came before the kernel's split (the user put kernel work
  off): `boolean/split.rs` holds `split`, `half_space`, `surface_tool`
  (with `ToolError`) and `chain_tool` with their planned signatures,
  each failing with `KernelError::NotImplemented`, to be replaced whole
  by the real ones (one arrangement for both pieces; the half-space box,
  the extended surfaces and the open chain's tool from the body's box).
  Until then every split fails as not implemented (see "Split" in
  `agents/features/split.md`).
- **Chamfer built above a stand-in.** The chamfer feature's document
  and regeneration came before the kernel's chains, spines, rails and
  chamfer (kernel work put off): `blend.rs` holds `chamfer(solid,
  topology, chains: &[ChamferChain], feature, tol, budget) ->
  Result<Solid, BlendError>` with its planned signature, failing with
  `BlendError::Failed(NotImplemented)`, to be replaced by the real one (the
  rest of `blend/`). The caller hands it chains of the solid's
  `Topology`, each with the `FacePart::Blend` edge name its faces get
  and a `ChamferCut` in the chain's region order (two distances, or a
  distance and an angle on one side); the kernel checks the dihedrals
  (`BlendError::Flat`, `Folded`, `Mixed`), the fit (`TooBig`) and
  corners (`Corner`). Growing edges along tangent chains, planned with
  the kernel's chains, is regeneration's instead, from the
  existing `Topology::tangent_chains`: it needs only the topology and
  the curves' end tangents. See "Chamfer" in `agents/features/chamfer.md`.
- **Fillet built above a stand-in.** The fillet feature's document and
  regeneration came before the kernel's notch, strips, sectors, mitres
  and vertex blends (kernel work put off): `blend.rs` holds
  `fillet(solid, topology, chains: &[FilletChain], radius, feature,
  tol, budget) -> Result<Solid, BlendError>`, failing with
  `BlendError::Failed(NotImplemented)`, to be replaced by the real one. The
  caller hands it chains of the solid's `Topology`, each once, with the
  `FacePart::Blend` edge name its faces get (grown along tangent chains
  and named as a chamfer's), and one radius for all (constant-radius
  fillets only; unequal radii can only meet across features). Beside
  the chamfer's refusals, `BlendError::End { chain }`: the notch runs
  into another face at an end of the chain rather than ending on one
  end face. See "Fillet" in `agents/features/fillet.md`.
- **Shell built above a stand-in.** The shell feature's document and
  regeneration came before the kernel's offset solid and shell (kernel
  work put off): `shell.rs` holds `shell(solid, topology, open: &[u32],
  thickness, outward, feature, tol, budget) -> Result<Solid,
  ShellError>`, failing with `ShellError::Failed(NotImplemented)`, to be
  replaced by the real one (and `shell/`, the offset solid with a
  distance per region that offset face and draft share). The caller
  hands it the regions of the solid's `Topology` to open, sorted and
  each once; the kernel names the inner faces `FacePart::Offset { of }`
  and refuses with `ShellError::RoundTooSmall` (a round face's radius at
  or under the thickness), `TooThick` (the offsets crossing: the hollow
  fails the hull rules) and `Corner` (more than three faces whose
  offsets miss one point). Departing from the plan, it takes `outward`
  (the UI mock's Direction): the result is then the offset solid with
  the kept faces by `+thickness` and the open ones by zero, less the
  body with its open faces pushed outwards past it, the same offset
  solid's two uses. The plan's `rounded` (task of rounded shells) is not
  in the signature yet. See "Shell" in `agents/features/shell.md`.
- **Offset face built above a stand-in.** As the shell's: `shell.rs`
  also holds `offset_faces(solid, topology, faces: &[u32], distance,
  tangent, feature, tol, budget) -> Result<Solid, OffsetError>`,
  failing with `OffsetError::Failed(NotImplemented)`, to be replaced by the
  offset solid with the picked regions by `distance` and the rest by
  zero (rebuilt only where a corner or chain moved, no boolean, every
  face keeping its name). The caller hands it regions of the solid's
  `Topology`, sorted, each once, at least one, and the distance signed
  (positive out of the solid, negative into it, never zero); growing
  across tangent faces is the kernel's (`tangent`), as the plan has it,
  not regeneration's as the chamfer's chains are. `OffsetError` is its
  own enum beside `ShellError` (the plan's names): it shares the round
  shrinking to nothing (`RoundTooSmall { region }`), the corner of more
  than three faces (`Corner { vertex }`) and `Failed`, and adds
  `PastNeighbour { region }`, `IntoBody`, `NoSurface { region }`,
  `TangentNeighbour { region }` and `OutOfRange`. See "Offset face" in
  `agents/features/offset-face.md`.
- **Draft built above a stand-in.** As offset face's: `shell.rs` also
  holds `draft_faces(solid, topology, faces: &[u32], neutral: DVec3,
  pull: DVec3, angle, tangent, feature, tol, budget) -> Result<Solid,
  DraftError>`, failing with `DraftError::Failed(NotImplemented)`, to be
  replaced by the offset solid generalized to a new form per region
  (the drafted forms: planes turned about their hinges, cylinders along
  the pull to cones, cones' half-angles grown, other walls along the
  pull fitted constant-slope surfaces; the rest kept). The plan's
  `neutral: Plane` is a point of the neutral plane and the unit `pull`,
  its normal as the caller directs it (the plane's normal, reversed by
  the feature's flip), since the pull is always square to it; the angle
  is in radians, above zero and under a right angle, the kernel taking
  its sine and cosine from `trig`. `DraftError` is its own enum: offset
  face's refusals (`PastNeighbour`, `IntoBody`, `RoundTooSmall`,
  `NoSurface`, `TangentNeighbour`, `Corner`, `OutOfRange`, `Failed`)
  and its own, `FacingPull { region }` (a plane square to the pull: no
  hinge) and `CannotDraft { region }` (neither flat nor a wall along
  the pull). See "Draft" in `agents/features/draft.md`.
- **Sweep built above a stand-in.** The sweep feature's document and
  regeneration came before the kernel's path sweep (kernel work put
  off; the plan's sweep in one plane, 3D paths and helices are not
  built): `sweep/path.rs` holds the kernel's path types and
  `sweep(profile, frame, path, orientation, twist, feature, tol,
  budget) -> Result<Solid, SweepError>` with its planned signature,
  failing with `SweepError::Failed(NotImplemented)`, to be replaced whole by
  the real one (with what it needs beside it under `sweep/`). Reached
  as `varde_kernel_compat::sweep::sweep` and `varde_kernel_compat::sweep::{Path,
  Piece, Helix, Orientation, SweepError}`, not re-exported at the
  crate's root, where revolve's `Sweep` (its span) already is. A
  `Path` is `Chain { pieces, closed }` or `Helix(Helix { point, axis,
  pitch, turns, left })`; a `Piece` is `Line { from, to }`, `Arc {
  conics, centre, axis }` (each conic at most a quarter turn, right-handed
  about the unit axis) or `Curve { conics, normal }` (the plane's normal
  where it's a sketch's, none for a model edge); the refusals are
  `Corner { at }`, `OffStart`, `NotSquare`, `TooTight { piece }`,
  `Parallel`, `HelixPlane`, `ReachesAxis`, `Pitch` and `IntoItself`.
  Departing from the plan, regeneration orders and joins the parts
  and checks the joints' tangents, an open path's start on the
  profile's plane and its squareness (the plan has the kernel check
  those too; it still should, as a guard, with the same rules: the
  sine is `sweep::JOINT_SINE`, which regeneration uses, the resolution
  the gap at a joint and the start's distance from the plane, and a
  piece closed on itself has no joint); a closed path's start, where
  the profile's plane crosses it, stays the kernel's. The planned analytic tests (paths in one plane, 3D paths,
  helices, refusals, thread counts) are in `sweep/path/tests.rs`,
  `#[ignore = "kernel sweep not built"]`. See "Sweep" in
  `agents/features/sweep.md`.
- **Loft built above a stand-in.** The loft feature's document and
  regeneration came before the kernel's loft (kernel work put off; the
  plan's sections and correspondence, ruled and fitted strips, smooth
  and closed lofts and rails are not built): `loft.rs` holds the
  kernel's loft types and `loft(sections, mode, closed, rails, feature,
  tol, budget) -> Result<Solid, LoftError>` with its planned signature,
  failing with `LoftError::Failed(NotImplemented)`, to be replaced whole by
  the real one (with what it needs beside it under `loft/`). Reached as
  `varde_kernel_compat::loft::{loft, Section, LoftMode, Rail, LoftError,
  on_one_plane, MAX_SECTIONS, MAX_RAILS}`, not re-exported at the
  crate's root. A `Section` is `Loop { outline: Loop, frame: Frame,
  start: Option<usize> }` (one loop, no holes, the start the index of
  the segment starting at the start vertex, `None` for the plan's
  default nearest the previous start) or `Point(DVec3)`; a `Rail` is
  `{ conics: Vec<Conic3> }`, an open chain in the world. The refusals
  are `OnePlane { section }` (sections `section` and the next, the first
  after the last of a closed loft), `RailMisses { rail, section }`,
  `Twists` and `IntoItself`; the plan's "a section with holes" can't
  reach the kernel (a section is one `Loop`): regeneration refuses it,
  and the document refuses a region reference with holes. Departing
  from the plan, regeneration refuses two consecutive sections on one
  plane before the kernel, by the kernel's own built rule
  `on_one_plane` (every vertex of each within the resolution of the
  other's plane; a point on a loop's plane; two points never), which
  the kernel should guard with too; and a closed loft takes no rails
  (the document refuses them: a rail is an open chain). The face names
  the plan gives a loft's walls (`Lofted { curve, span, segment }`)
  aren't in `PartKey` yet: they come with the kernel's loft, and become
  part of the format then. The planned analytic tests (frustum,
  hyperbolic paraboloids, oblique cone, cone to a point, square to a
  circle against its sections' integral, three sections ruled and
  smooth, a closed frame, a rail, the refusals, thread counts) are in
  `loft/tests.rs`, `#[ignore = "kernel loft not built"]`; their
  references are checked against closed forms by a test that runs. See
  "Loft" in `agents/features/loft.md`.
- **Extrude taper built above a stand-in.** The taper's document,
  regeneration and panel came before the kernel's (kernel work put off):
  `extrude/taper.rs` holds `extrude_tapered(profile, frame, from, to,
  taper, feature, tol, budget) -> Result<Solid, TaperError>`, a zero
  taper the extrude exactly and any other `TaperError::Failed(NotImplemented)`,
  to be replaced by the extrude's walls (split at height 0 for a span
  across it) drafted by the draft kernel. The plan has the extrude's
  walls drafted by `draft_faces` after `extrude`; a separate entry
  beside `extrude` keeps the untapered extrude untouched and lets the
  kernel build the split walls and name them, and its own
  `TaperError` (`Closes`, `TooSteep`, `OutOfRange`, `Failed`) words the
  draft's refusals for the extrude ("the taper closes the profile
  before its end", "the taper is too steep for this profile"). The
  `BackSide { curve, segment }` face part for a two-sided taper's walls
  behind the sketch isn't added yet: it comes with the kernel's taper.
