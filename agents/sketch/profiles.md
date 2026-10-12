# Profiles (`varde-sketch`, `intersect.rs`, `profile.rs`)

`Sketch::profiles()` finds the regions the curves that aren't
construction enclose, each an outer loop and its holes, pure and
bounded; `notes/SketchImpl.md` ("Built so far (step 5a)") has the
details and the timings.

- **Where curves meet** (`intersect.rs`): each curve as a `Geom` (a
  segment, or a circle or an arc by centre, radius, start angle and
  sweep, or a spline as Bézier segments, `Geom::Spline(Arc<Path>)`, with
  a parameter `u` along it), and `meet` gives the pairs of
  parameters where two cross (twice even barely, where those places are
  further apart than the tolerance), touch (within the tolerance: one
  place) or where an end of either, or a circle's start, is on the
  other, which is where lines on one line or arcs on one circle start
  and stop overlapping. Trim and extend use `meet` too, with the same
  tolerance (`intersect::tolerance`).
- **Splines** (`spline/bezier.rs`, `Path`): a spline's Bézier segments
  exactly, flattened by Wang's formula to 5·10⁻⁴ of its size, at most
  `CIRCLE_SEGMENTS` per segment; exact span boxes, bulge and length by
  quadrature, winding by halving parts until the place is outside their
  control points' box, and the closest place by Newton's method kept
  within a bracket from samples (`Sketch::nearest_on` for the view). It
  meets a line or a circle by the roots of the other's implicit function
  along each segment in Bernstein form, another spline (and itself, for
  profiles, its segments halved) by halving pairs of pieces whose boxes
  overlap until both are within the tolerance of their chords (the
  larger halved first, flat or not: a straight piece is flat at once, but
  halving only the other kept every piece of it in the straight one's
  box, and a 2-point spline across a curved one ran out of steps),
  polished by Newton's method. All bounded by `MAX_MEET_STEPS` (subdivision
  steps; segments' boxes compared don't take one, so the cap and what's
  found don't depend on them). Where the steps run out places may be
  missing, so the work returned is `usize::MAX` and profiles are too
  complex: a missed crossing would join pieces that don't meet (two
  wobbling rings a hair out of phase did, before). Trim and extend refuse
  likewise (`Sketch::cuts` is `None`: no trim, `EditError::TooComplex`
  for an extend), as would an offset (`EditError::TooComplex`; a corner
  crossing that ran out is left to the cut that refuses it), though an
  offset of a spline meets nothing. Splines whose copies are closer than
  about 1/5000 of their length apart run out (crossings so shallow that
  boxes overlap all along). `meet` returns the work done, which profiles
  count, in a unit of about one box compared to another (a nanosecond or
  two): `MEET_COST` (128) for lines, circles and arcs (60–260 ns: an
  arc's places each take a sine and cosine or an arctangent), and
  `APART_COST` (24) where a circle or arc is further from the other
  than the tolerance everywhere, told by centres and radii alone (circles
  side by side or nested, a line outside or inside one), which `meet`
  then skips; for a spline every segment box compared (1), `STEP_COST`
  (16) a subdivision step or a step of halving Bernstein coefficients
  (kept in fixed arrays, not allocated), `BISECT_COST` (16 × 64) a root
  halved to, and for each curve's end tested against a spline its
  segments (1 each) and `CLOSEST_COST` (192) a segment searched for the
  nearest place (ends further than the tolerance from its box aren't
  searched). Winding numbers add `CHORD_COST` (16) per chord, or per
  part of a spline `wind` takes. Measured against time, release, these
  come to 1–2 ns a unit for each kind of meeting.
- **Pieces** (`profile.rs`): curves are cut where they meet, found by
  sweeping their boxes along x; places within the tolerance (10⁻⁹ of the
  sketch's size) are one vertex, found through cells the tolerance wide,
  sorted. Pieces joining the same vertices through the same middle
  (overlaps) count once, and dangling ones are pruned until none are.
  A spline is also cut where it crosses itself, and a piece whose ends
  are one vertex is kept where it goes round (`Geom::goes_round`: a
  circle cut once, a closed spline, a spline's loop).
  An arc whose end is off its start's circle by more than the tolerance
  (drawn with its radius changing, until solved) is left out, as are
  circles and arcs within the tolerance of their centre. A line a fillet
  or chamfer cuts back is also cut where it's met, and its ends past
  there aren't pieces (`Sketch::cut_back`): the fillet or chamfer bounds
  the region instead. A `Piece` is a
  curve and the parameters it runs from and to (a line's 0 to 1, a
  circle's or an arc's angle from its start), backwards when `to` is
  below `from`.
- **Faces** (`profile/graph.rs`) are traced round the planar graph of the pieces with the face
  on the left, the pieces leaving a vertex in order of direction and,
  leaving within 10⁻⁶ radians of each other, of where each is sideways
  halfway along the shortest of them, by its direction, curvature and
  offset from the vertex it was merged into (beyond rounding): tangent,
  by curvature; crossing barely, by direction, so the two vertices where
  they cross agree. Each walk
  is cut into simple loops, cutting out bridges and places it passes
  twice: counter-clockwise loops are faces' outer loops, clockwise ones
  holes, or a connected part's boundary from outside, which is a hole in
  the smallest face of another part winding round it (exact, by the
  pieces and their boxes): the faces whose box holds it are tried
  smallest first (the first made of equal ones, taken from a heap), so
  the first it's inside is the one, and rings nested a thousand deep
  take a winding or two each rather than one per ring round them. A
  counter-clockwise loop no wider than the tolerance is no face's outer
  loop: a walk round a part's outside can pass a vertex twice as
  rounding has it and give a sliver of a loop, which made the part's
  outside no hole in the face round it.
- **Checked**: where curves lie along each other a hair apart, or touch
  at a place found as several a hair over the tolerance apart, the
  pieces at those vertices can be sorted in ways that don't agree from
  one to the next, and walks take wrong turns: loops whose pieces don't
  join, holes running counter-clockwise or outside their face. Profiles
  are then `TooComplex` rather than wrong: every loop of a face (slivers
  too) and every part's outside that's a hole in one must join up
  (`Graph::joined`), holes must run clockwise and enclose less than
  their outer loop. Random sketches with copies a hair off and tangent
  circles and lines hit it at about 1.5 %, plain random ones (points on
  a half grid) about 0.2 %: a circle with a line tangent to it lying
  along another line, three curves touching at a place, a closed spline
  doubling back on itself. Each was wrong before, regions missing or of
  pieces that don't join.
- **Regions** are every face: a plate with bolt holes is the plate, with
  a hole per bolt, and each hole's inside a region of its own, as is an
  island in a hole, so regions never overlap. Slivers, faces or holes,
  are left out. A `Region` has its `outer` loop (counter-clockwise) and
  `holes` (clockwise) as pieces, for extrude, its `outline` flattened for
  drawing and picking (outer first), its `area` and `bounds`;
  `Profiles::region_at` picks one by the even-odd rule on the outline.
  A `Piece` also names the vertices it runs between (`start`, `end`,
  indices into `Profiles::vertices`, the merged places): loops join up by
  them exactly, where the curves' own places meet only within the
  tolerance, so whoever builds exact geometry (regen, for the kernel's
  profiles, whose loops must close to the bit) puts piece ends there.
- **Merging** (`profile/merge.rs`): `Profiles::merge(&[usize])` gives
  the loops bounding several regions together (repeats count once), the
  union on the left of each, so outer loops counter-clockwise and holes
  clockwise, unnested. Exact: regions never overlap, and pieces shared by
  two picked regions are the same edge (curve and parameters to the bit)
  run opposite ways, so they cancel; what's left is traced again by vertex,
  each piece followed by its successor in its own loop where that's still
  there, else the first left starting where it ends, and walks passing a
  vertex twice are cut there into loops that don't (regions meeting at a
  corner only: touching loops, which extrude refuses anyway). Picking a
  hole's inside with the region round it fills the hole. `MergeError`:
  `Empty`, `NoRegion(index)`, `Open` (hand-made profiles only: pieces
  that don't join up or name vertices the profiles lack).
- **References** (`profile/reference.rs`): a `RegionRef { curves, holes,
  inside }` names a region for a feature to keep (serde): the sorted
  curve ids of its outer loop and of each hole (holes' lists sorted too)
  and a point strictly inside. `Profiles::reference(index)` makes one,
  the point, of the middles of the widest span inside (that `region_at`
  agrees with) along each of 16 horizontal lines across the region's box,
  the one furthest from the outline: a line along an edge or through a
  corner puts a middle on the outline, which a hair's move of the sketch
  hands to the region beside (None for a region too thin, every middle
  on the outline, or of more than `MAX_REGION_CURVES`, twice
  `MAX_CURVES`, ids). `Profiles::resolve(&[RegionRef])` finds each: the
  one region with the same curve lists; with several (as for the two
  halves of a circle cut by a line), the one of them its point is in;
  with none, the region its point is in; else none (the feature fails
  with "region not found"). With several, a point in a region of other
  curves finds none rather than extruding that region unannounced. So a region survives
  curves added elsewhere and dimensions changed, and a region split in
  two resolves to the half its point is in. The regions' curve lists are
  sorted once (stably, by index within a list) and each reference found
  by bisection, not a scan of all regions per reference; only none or
  several matching costs a `region_at` scan. Measured (release, loaded
  machine), 256 references each naming a region: 200×200 line grid
  (39 601 regions) 11.7 → 1.8 ms, 100×100 grid 3.0 → 0.9 ms; 256 naming
  none, so all `region_at`: 22 → 14 ms and 4.6 → 3.5 ms; 1000 nested
  circles, unmatched, ~33 ms either way (`region_at` over long
  outlines). All well under the `profiles()` that precedes it (grid 200:
  ~80 ms). `profile/tests.rs` checks it against the old scan on fixed
  and random sketches. `RegionRef::check(max)` holds
  what a file could get wrong: at most `MAX_REGION_CURVES` ids, lists not
  empty and sorted without repeats, `inside` finite and within `max`.
- **Near misses**: `Profiles::open_ends` are the ends only one piece
  reaches; `Profiles::near_misses(gap)` pairs those within `gap`, which
  is the view's (a few pixels in sketch units), so profiles don't depend
  on the zoom. Bounded by `MAX_NEAR_MISSES` and `MAX_NEAR_PAIRS`.
- **Bounds**: more than `MAX_SPLITS` cuts or `MAX_WORK` (60 M) steps is
  `TooComplex`, never a long wait. Steps are weighed by cost (above),
  and each cut adds `CUT_COST` (512) up front for what follows from it
  (pieces, vertices, sorting round them, walks, areas, boxes, polylines:
  1–2 µs a cut, ~2.5 µs where pieces are of splines; `MAX_SPLITS` cuts
  fit), so `MAX_WORK` is about a tenth of a second whatever the sketch:
  a few times what dense sketches people draw take, as refusing one
  costs more than the wait (measured share: 300 letter outlines like an
  "o" 21 %, 20 splines all crossing each other 16 %, a plate with 900
  holes 2 %, 1000 concentric circles 25 %;
  `normal_sketches_take_a_fraction_of_the_work` holds a third). Besides
  it only what's linear in the sketch (splines' shapes, at most
  `MAX_POINTS` fit points, ~40 ms). Hostile sketches measured (release,
  loaded machine): nested 100-point splines, 3000 concentric circles,
  copies of a spline, 1000 lines across 20 splines, 1000–3000 short arcs
  on circles crossing each other (which took 0.3–1.1 s before
  `MEET_COST` was weighed by time) are `TooComplex` in 0.02–0.15 s
  (`hostile_sketches_are_too_complex_in_bounded_time`, run only with
  `VARDE_TESTS=full`: seconds in a debug build); the
  slowest found are 40 closed 50-point splines crossing each other
  (10 921 regions, ~110 ms, 75 % of the work) and a 200 × 200 line grid
  (~80 ms). `Sketch::profiles_spending(&mut left)` also stops at what's
  `left` of a budget shared over several sketches, and takes the work
  from it (all it was allowed, if too complex).
- **Where it runs**: in the app, once per sketch shown, not in the solver
  lane (a millisecond or two for sketches people draw), so no wire carries
  profiles. `Doc::refresh_profiles`, run by `Doc::sync` and after every
  `Look`, finds them for the sketch as it's shown (committed, with edits
  waiting, or a drag's step: `Doc::shown_sketch`) unless the session's
  `Profiled` is of that sketch already (compared by value), and keeps them
  as `Result<Arc<Profiles>, TooComplex>`, which `SketchState::profiles`
  hands the view. The status bar counts them ("2 profiles") in a sketch
  with curves, or says "Too complex for profiles", and nothing is shaded.
  So a hostile sketch costs the UI thread at most the bound above per
  sketch shown (a drag's step included). The extrude session finds the
  profiles of its source, or before there is one of every visible
  sketch, on the UI thread too: see "The extrude UI" in
  `agents/kernel/extrude-ui.md` for its cache and shared budget.
