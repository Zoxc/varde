# Bodies from the history (`varde-document`, `varde-regen`)

A document's `Body` is `{ id, name, visible, created_by: FeatureId }`: no
shape and no position. Its geometry is whatever the feature history gives
it when regenerated. Body and feature ids share one counter. Only an
extrude makes bodies: `Document::check` wants every body's `created_by`
to be an extrude whose operation is `NewBody` of that body, and every
`NewBody` body to be there (so no two extrudes make one body). The Add
cube command, its toolbar button and `CUBE_SIZE` are gone.

## The extrude feature (`crates/document/src/extrude.rs`)

`FeatureKind::Extrude(Extrude { sketch, regions, extent, flip, taper,
operation })`:

- `sketch`: a sketch feature listed before it.
- `regions`: `1..=MAX_EXTRUDE_REGIONS` (256) `varde_sketch::RegionRef`s,
  each passing `RegionRef::check(MAX_COORD)`. They're made by
  `Profiles::reference` when picked; regen resolves them
  (`Profiles::resolve`) and merges them (`Profiles::merge`).
- `extent`: `OneSide(d)`, `Symmetric(d)` (the whole depth, half each
  side), `TwoSides(along, against)` or `ThroughAll`. Distances are
  `varde_expr::Value`s checked against `Extent::ask(design)`: a length
  from `MIN_LENGTH` (1 µm, as dimensions) to `MAX_COORD`, bare numbers in
  the design's units; two sides together at most `MAX_COORD`. Through all
  is only for `Cut`.
- `flip` swaps one side's direction and two sides' sides; symmetric and
  through all ignore it. `Extrude::span()` gives the `(from, to)` along
  the sketch plane's normal that `kernel::extrude` takes (`None` for
  through all, which regen works out from the bodies).
- `taper`: `Option<Value>`, the angle the walls lean by, checked
  against `Extrude::taper_ask` (an angle under 90° either way, bare
  numbers in degrees; else `ExtrudeError::Taper`). The panel stores no
  taper as `None`; a zero one (either sign, as a file may hold) is taken
  as none rather than refused: `Extrude::tapered()` gives the taper only
  where it isn't zero, and regen, its key and the notes go by it, so it
  is the untapered extrude exactly. Positive narrows the profile away from the sketch's plane, on
  both sides of it for two sides and symmetric (each side drafted away
  from the plane), negative widens it; the walls turn about where they
  meet the sketch's plane, also for a span that doesn't reach it
  (through all). `#[serde(default)]`: extrudes written before tapers
  read untapered. Written as nil where there's none, as every defaulted
  field is (the workspace never skips a field on writing: the workers'
  postcard is positional and would misread a skipped one).
- `operation`: `NewBody(BodyId)`, or `Join`, `Cut`, `Intersect` of
  `Targets { excluded, held }`, the bodies taken out (sorted without
  repeats, each made by an earlier feature) and the id of the body it
  made when it was a new body (`#[serde(default)]`, see "Commands shared
  by every kind" in `agents/features/commands.md`).

Commands:

- `AddFeature { name, kind }` (`Document::add_feature` names it
  "Extrude N"; the commands are shared by every kind, see
  `agents/features/commands.md`) adds it and hides its sketch; a `NewBody`
  extrude also adds "Body N" with the id after the feature's, replacing
  whatever id the command held (`BodyId::NEW` stands for it, an id no
  body has). One undo step.
- `SetFeature { feature, kind }` replaces it whole, the caller passing
  regions freshly referenced from the sketch as it is. A `NewBody` that
  stays one keeps its body; one that stops removes the body and drops it
  from other features' excluded lists, holding its id (`Targets::held`);
  one that starts gets the body back with the id it held, else adds a
  body.
- `RemoveOnly(ids)` applies `Document::breaking_removal`: just those
  features and their bodies, keeping the later ones that use them,
  naming what's gone (the delete prompt's Delete only).
- `RemoveFeature` and `RemoveBody` apply `Document::removal(Removable)`:
  the feature (a body's maker, for a body) and every later feature using
  one removed (`FeatureKind::uses`, a list: an extrude's sketch), in timeline
  order, and the bodies they make; the removed bodies are dropped from
  the rest's excluded lists. One undo step. The app asks `removal` before
  sending the command to show what goes.
- `SetTolerance(Tolerance)`: the document's fit tolerance
  (`Document::tolerance()`, stored as its `f64`, checked with
  `Tolerance::new`, default 1 µm). `SetUnits` pins extrude distances
  and the taper (`Extrude::values_mut`, each with its `Ask`) as it pins
  dimensions (`Value::pin_units`).

`Document::example()` is a 60 × 40 mm plate with a hole of radius 8 on
XY, extruded 10 mm as "Body 1", made through the commands. New designs
still start from `Document::default()`, empty: the example is for tests.

## Regeneration (`crates/regen`)

`varde_regen::evaluate(document, cache)` (`src/history.rs`; the extrude/revolve/sweep/loft tool run
in `history/run.rs`, the revolve axis in `history/revolve.rs`) runs the
features in order and gives an `Evaluation`: each body's solid
(`BodySolid`, in the order they were made), the bodies joins merged into
others (`merged`, below) and the features that failed, with why and
what to draw of where (`failed`, `FeatureFailure`s in the document's
order; see "Failures and where they are" in `agents/features/failures.md`). A
failing feature changes no body, and the later ones still run.

- A **sketch** gives its `Profiles` (`Sketch::profiles`; too complex fails
  the extrudes using it).
- An **extrude** resolves its regions (`Profiles::resolve`; one gone is
  "region not found"), merges them (`Profiles::merge`), turns the loops
  into a kernel `Profile` (below), and calls `kernel::extrude` on the
  sketch plane's `Frame` (an origin plane's placement, or a face
  sketch's as regeneration placed it; one not placed fails it, see
  `agents/features/sketch-planes.md`), over `Extrude::span()`,
  with the document's tolerance and `Budget::DEFAULT`, faces named by
  `FeatureId::get()`: the tool solid. A tapered extrude calls
  `kernel::extrude_tapered` instead (`src/history/taper.rs`), its key
  the untapered one's plus `"taper"` and the angle's bits (an untapered
  extrude, or one of zero, keeps its key and its tool bit for bit). For
  through all the span is the bodies' with its margin, the walls still
  hinged at the sketch's plane: a narrowing taper that closes inside the
  margin past the last body is refused though the bodies end before it;
  its refusals read "the taper closes the
  profile before its end: try a smaller taper or a shorter distance",
  "the taper is too steep for this profile: try a smaller taper", "the
  taper widens it out of range", its running out of budget "tapering
  its walls is too complex to work out", the stand-in's `NotImplemented`
  "a tapered extrude isn't supported yet" (what it gives today: the
  extrude makes no body and changes none, and the history goes on),
  its other failures as an untapered tool's. Tests replace the kernel's
  by `testing::taper_by_frustum` (a rectangle on one side of its plane
  made a frustum of a pyramid by moving its box's corners): through it
  `src/history/tests/taper.rs` checks volumes (flipped, through all on a
  body away from the plane, steep and slight tapers), refusals changing
  nothing, a zero taper's key and tool, and warm against cold caches; the
  draft crosses the workers' wire (`src/wire/tests.rs`). A `NewBody` body gets it. Through
  all's span is worked out first (`through_all`): the extent along the
  normal of the boxes (`bounds3`, the control points') of every body made
  before it, excluded or not (so taking one out or putting it back
  keeps the tool, and what was worked out with it), plus
  1 % of that extent and 1 mm at each end, clamped to `MAX_COORD`; none
  is "there's no body to go through".
- A **join, cut or intersect** asks `kernel::touches` of every body made
  before it and not excluded, in the order they were made, against the
  tool: those are the touched bodies and the targets
  (`Evaluation::touched`, per feature, also when it fails after finding
  them). Excluded bodies aren't asked about (the panel lists them from
  the session anyway), so taking out a body `touches` can't tell gets
  past it without spending the budget on it again at every change; a
  body it can't tell that isn't excluded fails the feature and is listed
  last, so the panel offers to take it out. No target is "it doesn't
  touch any body", or "it doesn't touch any body not taken out of it"
  when it excludes some. A tool touching a body only along a line (a
  boss tangent to a round boss) touches it, so that body is a target
  and its boolean decides: a join fails naming the body, as the union
  of solids meeting along a line isn't a manifold ("joining it to Body
  1 leaves no clean solid…", "…can't be worked out…" or, where
  refining the line contact runs out, "…is too complex to work
  out…"); a tangent cut is the body unchanged where the kernel works it
  out (every tangent disc from the coarsest tolerance to `1e-4`, with
  the walls along one direction certified) and otherwise fails naming
  the body ("cutting it from Body 1 can't be worked out…", "…is too
  complex…"), changing nothing; a hole drilled tangent to a hole's wall
  from inside it is a no-op too, and one tangent to it from outside
  (the holes meeting along a line) "leaves no clean solid". `touches`
  answers a tangency within milliseconds (one counting and its search,
  see "Touches"), so a draft dragged with its tool tangent to a body
  asks it again for each tool and lists the body, and the draft is
  decided by its boolean, never failed by the touch test (it used to
  run out of the whole budget on every draft change and fail "finding
  where it meets Body 1"; `a_hole_drilled_tangent_to_a_hole_is_decided_by_its_cut`,
  `a_tangent_hole_dragged_reruns_its_touch_test_which_holds`). So a tool that meets one body and grazes another
  along a line fails as a whole (it used to skip the grazed body) until
  that body is taken out. Where a feature has more than one target, a
  target's failing boolean (or an intersect emptying it) ends its
  message with the way past: "…; untick Body 1 under Bodies to leave it
  out" (`message::leave_out`); a cut emptying a body says so already,
  and with one target unticking leaves nothing to work on. The
  excluded bodies' touch and boolean results are marked used
  (`Cache::keep`), so the budget evicts them last, for putting them
  back. A cut or intersect replaces each target by
  `kernel::boolean(body, tool, op)` with `Difference` or
  `Intersection`, and a join touching one body replaces it by its
  `Union` with the tool: the body always first (so the body's face names
  win where flush faces of one plane merge). Every target's
  result is worked out before any body changes, so one failing changes
  none. Bodies not touched or excluded keep their solids.
- A **join touching two or more bodies merges them**: the first made
  (the *holder*) gets the union of them all and the tool, and the others
  are *consumed*: left out of `Evaluation::bodies` and listed in
  `Evaluation::merged` as (consumed, holder), sorted by the consumed
  body's id (the document's order). A later join consuming a holder
  moves the entries naming it on to the new holder (`note_merge`, public
  so the app replays merges by the same rule), so every entry names
  a body in `bodies` (a `debug_assert` at the end of `evaluate` holds
  it); `Evaluation::holder(body)` follows one. The bodies stay the
  document's (Objects lists them); only their geometry moved. Objects
  shows a consumed body faint with its holder as its note ("in Body 1",
  `panels::consumed_note`, from `MeshFeed::merged_bodies`: the model
  shown's, with a draft as the draft merges them, none across a
  replacement) and no eye, since it's drawn as its holder is and the
  holder's flag decides; its bin removes it and its maker as for any
  body. An Objects row's buttons that show on hover keep their room while
  hidden, as the mock's do, so the note stays put: the hovered row is
  drawn over the plain one, which shows through the dark theme's
  highlight. Cuts and
  intersects stay per body, as other CAD systems keep bodies apart for
  those, and an excluded body isn't merged (unticking it is how to join
  to fewer bodies). A combine consumes its tools into its target by
  the same rule unless it keeps them (`agents/features/combine.md`, "Combine").
  The union is worked out in steps, each one
  `kernel::boolean(running, next, Union)` cached under
  `boolean_key(op, running key, next key)`, all or nothing (nothing is
  written back until the last step works):
  - **The bodies first, then the tool**: `t0 ∪ t1` (key name
    `Doing::Merging`), then `∪ t2`…, then `∪ tool` (`Doing::Joining`).
    That passes the bodies' own faces first, which keeps the patches
    few: with the example plate and a plate 10 mm clear of it bridged by
    a boss flush with their bottoms, the tool joined to the first plate
    first left the boss's round bottom flush with the second plate in the
    next union, which came out with 57,430 patches (0.8 s); bodies first
    gives 120. The bodies' union doesn't depend on the tool, so
    a join draft dragged reworks one boolean, not one per target.
  - **Else the tool first**: if a step fails (bodies meeting each other
    only along an edge or at a point leave no clean solid on their own,
    though the tool bridges them: `Invalid(Fold)` for two blocks sharing
    a vertical edge), `t0 ∪ tool` (the key a single target would use),
    then `∪ t1`, `∪ t2`… (`Doing::Merging`). If that fails too, its
    error is given: its first step as a join's with more than one
    target ("joining it to Body 1 …; untick Body 1 under Bodies to leave
    it out"), a later one as `message::merging` ("merging Body 2 into Body 1
    leaves no clean solid: …", the tails shared with `message::boolean`,
    then "; or untick Body 2 under Bodies to keep it apart"). Either way
    the message has one such hint.
    The failed bodies-first steps stay cached, so the fallback reruns
    only on the tool's changes.

  Merging fails more often than joining each body on its own did, and
  rightly: blocks that meet each other along an edge or at a corner the
  tool doesn't cover make a union that isn't a manifold, which no solid
  holds, though each block joined to the tool alone is fine. A seeded
  fuzz of 2–4 blocks on a 5 mm grid (flush, corner to corner, apart,
  overlapping) bridged by a bar (`history/tests/merging.rs`, 24 seeds;
  300 measured) gave every merge the exact union's volume, and failed
  exactly where that union isn't a manifold (43 of 300; joined one by
  one, none failed). With discs among them (450 measured) no volume was
  wrong either: merging failed in 70, joining one by one in 39 (24 of
  them both), and the merge failures looked at were such contacts (a
  disc tangent to a block's side among them); the slowest is the one
  timed under the gaps below.

  With one target the path and keys are as they were before merging
  existed. `touched` still lists every target (consumed ones too).
  Through all spans the consumed material, which is inside the holder.
  Excluded bodies keep, besides their touch and boolean, the keys of both
  orders' steps for the targets plus every excluded body (in made order,
  hashes only), so unticking one body of a merge and ticking it again
  works out at most the merged body's mesh (nothing within the budget).
  A later feature taking out a consumed body takes nothing out (it has
  no solid; its material is in the holder, which is worked on), as when
  it was taken out before a join upstream reached it; the panel lists it
  "in Body 1" so that shows (see the extrude UI). A sketch placed on a consumed
  body's face (once sketches can be placed on faces) is to follow it to
  its holder (`Evaluation::holder`), since the face lives on there.
- A target whose result is the empty
  solid (a body cut away whole, or intersected with a tool that touches
  it without overlapping it: flush on a face, an edge on a face, a
  corner; the kernel gives all of these as `Ok(Solid::empty())`)
  **fails the feature**, so no body changes, with "cutting it from Body 1
  would leave nothing of it: untick it under Bodies to keep it as it is,
  or delete the body" or "intersecting it with Body 1 would leave
  nothing of it: they touch but don't overlap; flip it or move it to
  overlap" (`message::emptied`; flipping because the likely slip is a
  tool drawn on a face and extruded away from the body). Bodies are the
  document's, so an emptied one would stay listed in Objects with no
  geometry, and an edit upstream (the plate made smaller) could empty it
  with no feature turning red, while later features blamed the wrong one
  ("doesn't touch any body") or went through all of less. The body is in
  `touched`, so the panel offers to untick it; deliberately removing a
  body with a cut takes an untick and a body delete. The check follows
  the cache, so a result found there fails as well (a draft dragged
  across the point where the body goes, an undo bringing back the edit
  that emptied it); the empty result stays cached (its key is right; the
  check is cheap). Hence **no body in
  an `Evaluation` is empty**: a new body's extrude never is, a union of
  two non-empty solids isn't (a merged join checks anyway), and the rest
  fail (a `debug_assert` at the end of `evaluate` holds it).

**Error texts** (`src/why.rs`). What the Timeline's tooltip and the
panel show is worded for the user, not the kernel: an extrude's own
`KernelError` becomes "its regions are too complex to extrude: try
fewer or simpler curves" for `TooComplex`, "its regions have parts too
thin or too close together for this tolerance: try a finer tolerance"
for `Invalid`, and the profile errors say what's wrong with the outline
(touching or crossing itself, loops that don't nest, a cusp, a loop of
no area; for `TooFine`, "its outline has detail too small for this
tolerance: try a finer tolerance"). At the finest tolerance there is
none finer to try, so those two say "…to extrude, even at the finest
tolerance" instead (a strip 1e-7 wide is 10 resolutions there and fails
`Invalid`). A message names the tolerance only where a finer one is the
remedy, or to say there is none: `TooComplex` is the budget or a limit,
which a coarser tolerance doesn't change, so none suggests a coarser
one; `Triangulation` ("its end faces couldn't be made") and regen's own
`ProfileError::Fit` ("a spline couldn't be fitted: it stops or turns
back on itself") name none, as no finer one is known to mend them. A
boolean's error names the body and what was being done
("joining it to Body 2 leaves no clean solid: the result would touch
itself along an edge or at a point, or come too close to itself; move
it to overlap more or to clear it" for `NotManifold`, said outright;
"… leaves no clean solid: parts would be too thin or too close
together, as where faces are tangent; if so, move it to overlap more or
to clear it" for the other `Invalid` failures, zero-angle corners where
faces are tangent and thin triangles beside a hole's rim among them, so
the cause is hedged and no tolerance is offered, a finer one mending
neither;
"… can't be worked out: they meet on faces too nearly flush or tangent
to tell apart; move it a little" for `Inconsistent`; "… is too complex
to work out…" for `TooComplex`; "… isn't supported yet" for the
unbuilt stand-ins' `NotImplemented`; for a result that's empty, the
`emptied` texts above). Every message starts in lower case,
the Timeline putting it after the feature's name. Body names are looked up when the message is
made, not kept in the cache.

**Pieces to conics** (`src/profile.rs`, `profile(sketch, profiles, loops,
fit)`). Each piece becomes segments named by its curve's `Id::get()`, its
ends put at its vertices (`Profiles::vertices`), so loops close to the
bit:

- A line piece is `Conic2::line` between its vertices.
- A circle or arc piece (sweep `|to − from|`, counter-clockwise when `to >
  from`) is halved until each part is at most 90° (with `1e-9` slack; at
  most twice, four parts for a whole circle). The middle of a part from `a`
  to `b` about `c` is `c + r·d/|d|` with `d` the chord `b − a` turned a
  quarter clockwise (anticlockwise for a clockwise piece), or `−(a − c + b −
  c)` for sweeps over 270°, where the chord vanishes. Each part is exact,
  without `cos` or `sin` (`Conic2::arc_between`). `r` is the circle's radius (an arc's start's distance
  from its centre); the parts share their middles to the bit.
- A spline piece (`src/profile/fit.rs`) is the open `BSpline::piece`
  between its parameters, cut at its breaks into cubic Béziers from the
  points and derivatives there (each break evaluated once, so neighbours
  share it). It is always fitted in the spline's own direction, its ends
  put at the vertices at its parameter ends; a piece running backwards
  then reverses the chain and each conic (`Conic2::reversed`, exact), so
  the two regions beside a piece get the same conics to the bit and
  their walls coincide. The Béziers are fitted **in runs**: from each one
  on, the run `i..=j` grows while one conic fits it and stops at the
  first `j` that doesn't (no scanning past a failure), and the longest
  run that fits gives one conic. A run fits:
  - **as a line** first, if every control point of every Bézier in it is
    within a quarter of the fit tolerance of the chord and projects
    between its ends (rigorous, by the convex hull), and the run's end
    tangents are within 45° of the chord. Without that, a sliver
    narrower than the tolerance (a closed spline 0.01 across at a fit of
    0.1) came out as two lines there and back, or a needle triangle:
    within the tolerance, but no area or a cusp, which fitting span by
    span never gave. With it, a line turns by under 45° where it meets a
    curved conic (which leaves along the spline's tangent) and under
    90° where it meets another line;
  - else **as a conic** along the run's end tangents (from its first and
    last Bézier; they must meet ahead of both ends and turn by under
    90°), its weight `k/(1−k)` putting the conic's shoulder as far from
    the chord as the furthest sample (`k` that height over the control
    point's; a conic is furthest from its chord at its shoulder), or else
    weight 1, within `0.25..=4`, accepted when 15 samples of each Bézier
    and each Bézier's end lie in the control triangle within half the fit
    tolerance of the conic (by its implicit form over its gradient,
    `λ1² = 4w²·λ0·λ2`).

  A Bézier no run fits, not even alone, is fitted on its own: a line if
  its inner control points are within a quarter of the tolerance of its
  chord and its end tangents within 45° of it, else one conic as above
  with its shoulder where the cubic at ½ crosses the line from the
  chord's middle to the control point, else a line if straight within
  the tolerance whatever its tangents (a sliver's tip, where span by
  span gave the same line; halving there only makes conics too thin to
  tell apart), else halved (at most 24 times; past that `ProfileError::Fit`). Curved
  conics meet along the spline's tangent; a line meets its neighbours
  with a kink of about the tolerance over its length where the spline's
  segments there are long (invisible at the tolerance; the profile test
  of a smooth spline bounds it by twice that), and under 45° (under 90°
  against another line) in any case, but for the last-resort line. A piece
  of `n` Béziers costs at most about `16·n²` point evaluations (`n` is
  about `MAX_SPLINE_POINTS` = 100 at most): the worst corpus spline fits
  in 1.2 ms, and a hostile sketch of 299 such splines (as many as
  `MAX_POINTS` allows), each a 60° arc one conic fits whole, the worst
  case for runs, profiles in 0.7 s at most (release). At most `MAX_PROFILE_SEGMENTS` segments are made
  (`TooManySegments`), counted as the piece is fitted. Measured on a
  closed spline through five points about 10 across: 5, 29 and 78
  segments at fits of 0.1, 1e-3 and 1e-5 mm (no run takes more than one
  of its five spans, so the counts are as span by span).

  **Measured** before and after runs (a scratch corpus on the real
  crates, each sketch extruded 10 mm on the XY frame at fits 1e-5 to 1e-1
  mm; refused counts `before → after` at 1e-3, 1e-2, 1e-1; segment totals
  at 1e-1). "Wavy": a plate W × W/2 whose top is an open spline through
  `n` ∈ {10, 30, 100} evenly spaced fit points jittered by ±slope·spacing/2,
  W ∈ {1, 5, 20, 100}, slope ∈ {0.1, 0.3, 1}, 10 seeds (360). "Control":
  the same top by control points (60). "Blob": closed splines through 5
  to 100 points on a perturbed circle of radius 0.2 to 100 (420). "Cut":
  a blob cut by a line, each region (48). "Hole": a blob as a square's
  hole, each region (48).

  | set | refused at 1e-3 | at 1e-2 | at 1e-1 | segments at 1e-1 |
  |---|---|---|---|---|
  | wavy | 5 → 5 | 17 → 15 | 52 → 5 | 21 772 → 12 510 |
  | control | 0 → 0 | 0 → 0 | 9 → 0 | 2 972 → 1 092 |
  | blob | 0 → 0 | 0 → 0 | 0 → 0 | 16 602 → 4 339 |
  | cut | 0 → 0 | 0 → 0 | 0 → 0 | 1 140 → 344 |
  | hole | 0 → 0 | 0 → 0 | 0 → 0 | 2 180 → 590 |

  Nothing failed at 1e-5 or 1e-4 before or after. The worst sampled fit
  error stays 0.68 of the fit at 1e-3 and coarser; the worst fit time
  fell from 2.8 to 1.2 ms. Every refusal left is the cap residue at real
  detail near the tolerance (slope 1, see "Known gaps" under Profiles
  and extrude), and two sketches that extruded before now fail with it
  (wavy n = 30, W = 1, slope 1, seed 0 at 1e-2, `TooComplex`; n = 100,
  W = 20, slope 1, seed 5 at 1e-2, `Invalid(VertexNeighbours)`): their
  chains changed by a few segments (53 → 50, 228 → 226) and the caps'
  fragility flipped, as it flips the other way for others.

**Cache** (`src/cache.rs`). Every result is filed under a 128-bit key (two
SipHash runs, one salted, over the length-prefixed parts): a sketch's
profiles by the sketch alone (postcard-encoded; not its plane), whether
it solves by the sketch, a face sketch's placement (or why it has none,
with a face that isn't flat drawn) by the face's solid's key, the face
reference and the fit tolerance, an extrude's solid (or error) by its feature
id, the regions, the tolerance's bits, its span's bits, its sketch's key
and its placement's bits (not the operation, the extent or the excluded bodies, so toggling those
finds the tool; the span stands for the extent and flip), whether a body touches a tool by the two solids' keys, a
boolean's result (or the kernel's failure, its evidence made drawable,
`KernelFailure`) by the operation and the two solids'
keys, which then keys the body's solid (a merge step's likewise, by the
running solid's key and the next operand's), and a body's mesh by its solid's
key and the tolerance. Editing an earlier extrude changes its body's key
and so reruns every boolean after it on that body. The cache is bounded
by size: each entry records its approximate heap size (`Entry::bytes`, from each
type's `HeapSize` in `cache/size.rs` or beside its fields: a
solid's four mesh arrays, a render mesh's four, profiles' regions, pieces,
outlines, vertices and open ends, an error string's length, plus 128 B
for the map slot and the `Arc`; saturating sums, since the sizes come
from what the user drew; lengths, not capacities, so a vector grown by
pushes, such as a scene of several bodies joined, may really take up to
about twice its count), the request that last used it and a use
counter. `Cache::begin` starts a request and, only while the total is
over the budget (`BUDGET`: 256 MiB natively, 64 MiB on wasm32, per lane
and so per open document, since a worker's memory never shrinks) or
there are more than `MAX_ENTRIES` (2^16, so tiny entries can't pile up),
evicts what the request before didn't use, least recently used first by
the use counter (a total order: eviction never depends on the map's
order, so counts are the same on every run). What the request before
used is never evicted, nor is the scene of the last answer without a
draft, so an unrelated edit, or a draft dragged, reruns
only what changed whatever the budget (that set may exceed the budget on
its own); within the budget, undo, redo and an option changed and
changed back (join, cut, join; a distance typed and typed back) find
what they had. One request on the example with a pocket holds about
90 kB, on a 100 mm plate with 36 holes about 1.6 MB. A join, cut or
intersect also marks used (`Cache::keep`) whether the tool touches each
body it excludes and their boolean, and a join the steps of its merge
with the excluded bodies put back, so taking a body out and putting it
back only draws it again even under a small budget.
`Cache::with_budget(0)` (tests) keeps only what the request before used,
which was the policy before the budget. The lane owns it: the native
thread's closure, or the worker's `serve`.

**Drafts.** `Request::Regenerate` has `draft: Option<Draft { revision,
feature, kind }>`: a feature of any kind (an extrude, a revolve) being set
up (`feature: None`, applied as `AddFeature`, the body `BodyId::NEW`) or
edited (`SetFeature`, which may change its kind), applied to
a copy of the document through an `Editor`, so its checks apply. The
answer carries `Drafted { revision, error, geometry, touched }`; a draft
the document refuses, or whose feature fails, is answered with the
committed model and its error (and where its feature fails, what to
draw of where). `touched` is the draft's touched bodies less the
excluded ones, `None` where the touch test didn't run (a new body, a draft
failing before its tool exists, one the document refuses) and `Some` of
an empty list where it touched nothing; there even when it fails after
finding them, with the body `touches` couldn't tell last, so the panel
can list a body to take out that makes it fail. `Response::Failed` carries
the draft's revision too, in its `Asked`.

**The answer.** `Response::Regenerated` adds `failed`, `merged` (the
evaluation's, (consumed, holder) pairs) and `bodies:
Vec<(BodyId, Aabb)>` (each body with a solid, shown or not, from
`Solid::bounds`; a consumed body has none). `tessellate_scene` draws the
visible bodies' solids at `Display::new(&document.tolerance())`, joined
by `RenderMesh::append` into an `Arc<RenderMesh>`, a part per body in
the order they were made; a mesh past `RenderMesh`'s limits fails the
generation with the `MeshError` (and isn't kept). Each body is drawn
with its picking tables (`Drawn`, made by `Solid::tessellate_with` with
the body's `Topology`: its faces' keys, aliases and summaries and its
edges' closed flags; see "The document mesh" in `agents/viewport.md`),
and the answer's `picking` is the shown bodies' joined, with the body of
each part. The joined mesh and tables are kept in the cache under a
scene key (`"scene"`, then each shown body's id and mesh key in order,
which holds the tolerance, then the count), as one more kind of entry under the same budget, except that
the scene the last answer without a draft used (a failing draft's answer
is one) is never evicted either, so the committed model's scene survives
any number of draft revisions, even when a revision's scene is an older
one still held. Scenes aren't in `counts` (those count features;
`Cache::joins` counts joins, for tests). A request whose scene didn't change (a sketch edit no body
depends on, a sketch hidden or left out, a failing draft, the committed
model after a draft was dragged and put away, the model just after a
draft is committed) gets the same `Arc`, so natively the renderer, keyed
by the `Arc`, skips the upload; a scene found keeps its bodies' meshes in
the cache (`Cache::keep`) for the next scene that changes one. The
renderer doesn't try an upload of the same `Arc` again after it failed;
the only failure is a part past the device's buffer limit, which the same
mesh would hit again, so it is logged once. On the web the mesh still crosses the wire whole each time.
On the web the reply's head
carries `draft`, `failed` (each failure's geometry as `GeometryParts`,
checked by `ErrorGeometry::from_parts` against the model,
`wire::Error::Geometry`), `touched`, `merged` and the boxes as corner
arrays, checked finite and in order on receipt (`wire::Error::Bounds`);
`MAX_HEAD_BYTES` is 64 MiB (the head carries the picking tables too:
the parts' bodies, at most `RenderMesh::MAX_PARTS`, the faces, at most
`MAX_FACES` 2²⁰ with `Picking::MAX_ALIASES` 2²⁰ aliases among them, and
the edges' closed flags and tangent chains, at most
`RenderMesh::MAX_EDGE_POLYLINES` each, all refused as soon as they're
past their bound, since a face is some 120 bytes
on the page and as few as 5 in the head; a model whose head would be
larger or whose faces are past those is answered as failed). The draft's and each feature's touched bodies
cross in the head as marks, unchecked; `merged` is checked to name each
consumed body once and none as a holder (`wire::Error::Merged`), and is
otherwise display only. Either failing answers the generation with
`Response::Failed`.

**Gaps.** Every join, cut or intersect asks `touches` of every body
before it on each edit that changes the tool (cached otherwise; bodies
whose boxes are apart are answered at once).
A boss tangent to a body only along a line touches it now, so the
join reaches the union, which refuses a line contact or runs out
refining it (the union's own gap); before, `touches` called it "it
doesn't touch any body". Measured, release, a loaded machine, a disc
of radius 1 cut or joined 1 mm tall beside a round body of radius 1
along a line at three angles: `touches` 0.8 to 4.3 ms at every
tolerance from `1e-1` to `1e-4`; the whole regen 0.2 to 0.45 s at
`1e-1` (refused or a no-op), 1.1 to 2.4 s at `1e-2` (a no-op, or
refused for a face too thin), 1.4 to 4.1 s at the default and 4.9 to
7.6 s at `1e-4`, there the boolean running out (`TooComplex`). So the
time moved from `touches` (3.6 to 11 s to `false`, `TooComplex` or
`Inconsistent`) to the boolean, whose answer is cached. With walls
along one direction certified near each other, every such cut is a
no-op, in 20 to 320 ms at every tolerance, and the joins are refused
in 15 to 170 ms at `1e-1` to `1e-2` off the seam, and at the default
off it in 0.1 to 2 s (most of it repairing the refused union); a
join with the line on the seam (`Inconsistent` at `1e-1` and `1e-2`,
`TooComplex` from the default) and every join at `1e-4` (1.5 to 2.8
s, `TooComplex`) are as before: the pairs along the line have ends
there, and joining them line by line leaves those of a union at a tie
alone (measured since, on a loaded machine: the seam joins "can't be
worked out" in 0.2 to 0.55 s at `1e-1` and `1e-2`, run out in 2.4 s at
`1e-3`, and every join at `1e-4` runs out in 3.3 to 4.3 s, the cuts
no-ops in 7 to 490 ms). Since the decisions name a union touching
along a line (see "Results that aren't manifolds" in the booleans),
every one of these joins is refused as "the result would touch
itself" in 20 to 830 ms, at every fit and angle. Holes drilled tangent to the
example plate's hole (radius 8; a disc of radius 3 through it, at
angles 0, 0.3 and 0.7, release): from inside, no-ops in 0.06 to 2 s;
from outside, refused as "no clean solid" in 0.3 to 0.6 s off the
seam, and with the line on the seam (angle 0) out of budget in 4 s, a
difference at a tie on a seam that refinement doesn't settle. In each
the touch test is milliseconds, so the time per draft change is the
boolean's, cached per tool. An operation
that runs out of budget takes about 2–3.5 s on one native thread and holds the
single-threaded web worker longer, with drafts queued behind it (latest
wins, so only the newest waits). The cache's sizes are estimates
(shared `Arc`s count once per entry holding them), and eviction is plain
least recently used, not weighted by what an entry took to work out. On
the web an unchanged model
mesh is still copied over the wire, checked and uploaded again with each
answer. Merged bodies grow, so later booleans on them cost more (the
budget scales with the whole body), and a later cut that worked on the
bodies apart can fail on the merged one. The tool-first fallback can
meet the flush blow-up the bodies-first order avoids (57,430 patches in
the case above), and a bodies-first step that runs out of budget before
the fallback runs about doubles the worst case, once (both are cached).
Measured: four bodies, two of them overlapping cylinders, all flush on
top with a disc tool (cylinder caps flush, the boolean's own blow-up):
bodies first ran 0.9 s to 26,864 patches, then the tool's step 3.6 s to
`TooComplex`; the tool first then 4.6 s (72,296 patches), 8.3 s in all
(release, an idle machine) against 3.1 s joining each on its own. A
join draft dragged there pays the last bodies-first step and the whole
fallback on each step.
The merge's keys kept for excluded bodies cover putting back all of
them at once or the only one: with two taken out, putting back one
reworks its merge. Merged plates flush on each other keep the line
where they met on their sides: the union doesn't merge the two plates'
coplanar side faces.
