# How sketching works

What the user sees is in `notes/Sketching.md`, the plan in
`notes/SketchImpl.md`. This note says how what's built works: the model,
the solver and its lane, profiles, how it's edited, drawing it and the
input on it in the viewport, dimensions included. Every edit is proposed to the solver
lane and committed once it's accepted, solved.

## The model (`varde-sketch`)

A `Sketch` holds points, curves (`Line`, `Circle`, `Arc`, `Spline`), constraints
and dimensions in vectors sorted by id, with `next_id` handing out ids
shared by all four, so an id names one thing and never changes.
`add_point`, `add_curve`, `add_constraint` and `add_dimension` give the
next id (running out is `OutOfIds`) and a number
per kind for the name ("Line 3"). Curves name their points by id, so lines
drawn in a chain share the point between them. An arc runs
counter-clockwise from `start` to `end` around `center`; until the solver
holds its radii equal, the radius changes evenly along it.

**Angles** (`angle.rs`): every `sin`, `cos`, `tan`, `atan2`, `acos`,
`exp`, `ln` and `hypot` in the crate goes through `crate::angle`, which
wraps the `libm` crate (`from_angle(a)` is `(cos a, sin a)`, `to_angle(v)`
is `atan2(v.y, v.x)`, `between(a, b)` is glam's `angle_to` unsigned and
`angle_to(a, b)` signed as glam's, by `a.perp_dot(b)`), not
std's or glam's, which come from the platform's maths library. So an arc's
start angle, the places `Geom::at` gives (the profile vertices at arc ends
and crossings), and the edit-time results have the same bits natively and
on the web (where std's are this code already), and the kernel's `trig`
agrees with them to the bit. `crates/sketch/clippy.toml` refuses std's
versions (and `powi`, `powf`, and glam's angle and rotation methods)
through `disallowed-methods`; tests using std as an independent
reference, or to build inputs, allow it in their module.

The module is public (`varde_sketch::angle`, with `log10` and `pow` too)
because `varde-view` makes saved values of its own: a line's end from a
typed angle and the polygon's corners (`typed.rs`), tangent points
(`snap.rs`), the length an extrude drag snaps to (`extrude::snap_step`:
its decade by `log10`, the step itself read from its decimal text, as
`pow(10, k)` can be an ulp off for negative `k`), and
`dimension::sector_holds`, which picks the side a dimension is placed on
and whether a snap lands on an arc's run. That one turns by `arc_sweep`
(so by `atan2`), not by `acos` of the cosine as glam's `angle_to` does:
the turn to the very vector an arc's sweep was made to is that sweep to
the bit, so an arc's own ends are on its run (by `acos`, a quarter of
random arcs lost their end by up to 10⁻⁸ radians), a direction along the
start is no turn, and one of no length is in no sector. They take these
helpers, so the same typed edit gives the same bits natively and on the
web; `crates/view/clippy.toml` bans the same
`f64` and glam `D*` methods. Display only, and on std: the view's `f32`
maths (the view cube's letters, the zoom per wheel step) is not listed,
and the drawn arc of an angle dimension
(`viewport/sketch/dimensions.rs`) allows the lint. Tests build inputs
with the helpers and check saved bits against `libm` directly.

A **spline** (`spline.rs`, `Curve::Spline(Spline)`, "Spline 1") is a
cubic non-rational B-spline, open (clamped) or `closed` (periodic), its
parameter from 0 to 1. `SplineKind::Through` passes through its
`points`, fit points, at their chord-length parameters (`chord_params`),
found again whenever they move; `Handle { at, tip }` puts a handle at a
fit point, the tip an ordinary point, setting the derivative there to
`3 / h (tip - at)` (`h` the mean parameter span beside it), so a handle
a third of the chord long is the pull it'd have anyway.
`SplineKind::Control` is pulled towards its control points, touching an
open one's ends, over `knots` it keeps (as `BSpline::clamped` or
`BSpline::periodic` take them, at least `MIN_KNOT_GAP` apart;
`control_knots` makes them from the control polygon). The math is in
`spline/basis.rs`: `BSpline` (evaluation with two derivatives,
curvature, `breaks`, `piece` and `with_knot` by Boehm's knot insertion,
a closed one's laid out over several periods), and `Interpolation`, the
control points as a linear map of the fit points and tips for parameters
fixed beforehand (what the solver holds through a solve): knots at the
parameters, two a third of the way either side for a point with a handle
so it stays C², natural ends without handles. `Sketch::spline_shape`
gives a spline's `BSpline`. `Curve::points` gives a spline's points then
its tips; `Curve::ends` an open one's first and last. A handle's length
and angle can be dimensioned (a distance between its fit point and tip,
an angle naming the tip: `Sketch::handle` finds a handle by its tip,
`Sketch::direction` a line or a handle as what an angle is measured
along).

Splines are shaped by edits of their own (`spline.rs`):
`SketchEdit::Convert` (`Sketch::convert_spline`: to control points
exactly; to fit points through its places at its knots, with handles at
an open one's ends giving the tangents it had, straying a little
between; an open one keeps its end points), `AddHandles` (a handle at
each fit point named, on every spline through fit points it's one of
without one, its tip at `Sketch::handle_tip`, where the spline keeps the
tangent it has there, so the shape changes only by the knots a handle
brings) and `InsertPoint { spline, near }` (through fit points a fit
point at the place nearest `near`; by control points a knot there,
`BSpline::with_knot`, exact, the control points either side new, those
unchanged kept, the ones replaced going with what's on them). A handle
goes by deleting its tip. `Sketch::curvature_comb` gives places along a
spline, evenly by parameter in each segment, at most `MAX_COMB_TEETH`
(1000), each with its curvature times its left normal, for the view to
draw; `flatten_spline` a spline of places not yet in a sketch, for the
Spline tool's preview.

Constraints (`constraint.rs`) name points and curves: coincident, point
on curve (a spline's too), horizontal and vertical (a line, or two
points), parallel, perpendicular, tangent, smooth, equal (lengths or
radii), concentric, midpoint, symmetric about a line, fix, and equal
offset (two offset pairs as far apart as each other, which only Offset
makes; see below). A tangent stores its `Side`, the mirror image it was
made on (`Sketch::tangent` picks it from the geometry), so the solver
can't reach the other. `Constraint::items` gives each item with
the `Role` it plays (a point, a line, a circle or an arc, any curve, ...),
`Constraint::fits` says whether their kinds go together, and
`Constraint::own_point` finds a curve named with one of its own points.

A tangent with a spline (`at: Some`) and a smooth join (`Smooth`) hold at
a spline's end, the point `at`, named with them (`joint.rs`): `Joint`
says which is the spline (the first of two), and whether `at` is the
other's end too (`shared`: they touch there, as a spline drawn on from a
line's end does) or is to be held on it (a line's endless line, a
circle's or an arc's circle); two splines always share it. Their `Side`
says whether the two run the same way there, each its own way along: a
line from its start to its end, a circle or an arc counter-clockwise, a
spline as its parameter runs (`Sketch::heading` gives that way and how
it curves). Smooth adds curving alike, the same way round, which a spline
through fit points can't at an end without a handle (straight there
whatever moves, `Joint::curves`), so it's refused there. `Sketch::joint`
picks the end (the one shared, else the spline's end nearer the other)
and the side the geometry is on, for `Sketch::tangent` and
`Sketch::smooth`.

Dimensions (`dimension.rs`) measure a distance (two points, a point and
a line, two lines), a horizontal or vertical distance, a length, an angle
(between lines, or a spline's handle named by its tip, from its fit
point to its tip), a radius or a diameter, or an offset pair's offset
(`Measure`, `Measure::fits` for what roles can't say), with the
value as typed
(`varde_expr::Value`: the text and what it came to in millimetres or
radians), whether it's driving (an equation of the solver's) or a
reference, a label offset from its anchor (`Sketch::anchor`) and a `Side`
choosing the mirror image it holds: which side of a line, which way along
an axis, which of the angles between two lines. `Sketch::side` gives the
side the geometry is on, `Sketch::measure` what a dimension measures now,
which is what a reference shows (it isn't stored or analysed, a pure
function of the sketch).

`Sketch::check` is the one place a sketch from outside is trusted: counts
bounded, ids sorted and unique across the lists, references of the right
kind (or role), a constraint's items going together and neither a
constraint nor a dimension naming a curve and one of its own points,
coordinates within `MAX_COORD` and radii above zero, splines' counts
(`SplineKind::least` to `MAX_SPLINE_POINTS`, 100), handles (on its fit
points, one each, none by control points) and knots (`Spline::fits`,
`SketchError::Spline`), a tangent with a spline or a smooth join at a
`Joint` (smooth with a handle there through fit points), an angle's
points handles' tips (`Sketch::tips`), labels within
bounds, and every dimension's expression re-evaluated, in the design's
units, to its stored value, which must be what its measure asks
(`Measure::ask`: a length at least `MIN_LENGTH`, a micrometre, and
within the limit, an angle above zero and under a turn). So it takes a `Design { max, units }`, the document's from
`Document::design`. The document's editor runs it on every edit, so a
sketch in the document always passes it.

`Sketch::delete` removes items with what depends on them: curves made from
a deleted point, the fillets and chamfers on a deleted line, points only
deleted curves used (a lone point stays), and constraints and dimensions
on anything deleted. A spline losing points keeps the rest if it still
has `SplineKind::least` (two fit points open, three closed; four control
points open, three closed), its handles at them or whose tips go going
with their tips, and by control points its knots found anew
(`control_knots`); else it goes whole. What no longer fits once a
handle has gone goes too: a smooth join that needed it, an angle naming
a tip that's still there as another curve's point.

`Sketch::pin_units` writes the design's unit in after every bare number
of the dimensions' expressions, for `Command::SetUnits`, which changes
the design's units (`Document::units`, millimetres by default) without
changing a value.

Geometry for drawing and the tools: `Sketch::line` and `Sketch::round`
give a line's ends and a circle's or an arc's centre and radius,
`Sketch::flatten` turns a curve into a polyline (`flatten_circle` and
`flatten_arc` do the same for a circle or an arc not in a sketch, for a
tool's preview), and `arc_through` finds the arc through three points
with its ends ordered counter-clockwise (`geometry.rs`, which also has
`arc_sweep`, `foot` and `crossing`, shared with the dimensions' anchors
and the view's snapping and glyphs). The shape tools' previews are
`Sketch::trim_piece` (the piece trimming would take away, as a polyline)
and `Sketch::extension` (what extending would add), with
`Sketch::nearer_end` picking a line's or an arc's end by a click
(`shape.rs`), and Offset's `Sketch::chain_of` (the chain a curve is in),
`Sketch::offset_side` (how far a place is from a chain, on which side)
and `Sketch::offset_preview` (the copy as polylines) (`offset.rs`); see
Edits and proposals.

An **offset pair** (`Sketch::offset_pair`, `OffsetPair`) is a curve and
its copy some way off: two lines (the copy's midpoint from the line
through the first, left positive), two circles or arcs (the copy's radius
less the first's), a point and a circle or arc about it, a round join
(its radius), or a spline and a point not its own (the point's distance
from the place on the spline nearest it, left of the way it runs
positive: a fit point of a spline's offset copy). `Measure::Offset(a, b)`
measures one, signed by its side;
`Constraint::EqualOffset { a, b }` holds two as far apart as each other,
either side (its equation is `|offset a| - |offset b|`, so no side is
stored: the dimension keeps both away from zero). The pairs of one may
share their first (a line whose copy was cut in two, a spline's copy's
points), and a round join's pair names the arc's own centre, which
`own_point` lets be; so their items' roles are any geometry, and
`fits` says which go together.

A **fillet** or a **chamfer** (`corner.rs`) is an arc or a line whose
`CurveEntry::corner` is a `Corner { a, b, at, equal }`: the lines `a` and
`b` ending at the point `at`, which stay whole. It runs from its start, on
`a`, to its end, on `b`, a fillet counter-clockwise the short way (so the
edit orders the lines), and it implies equations of its own, as an arc
implies its radius's (see the solver). It's named "Fillet 1", "Chamfer
1" (`CurveEntry::noun`, numbered apart from arcs and lines) and is
otherwise an arc or a line to everything else: dimensioned, constrained,
snapped to, dragged, hit. `Sketch::cut_back` gives each line a corner cuts
back the part of it kept (parameters from 0 at its start to 1 at its end,
up to where the fillet or chamfer meets it), and `cut_line` splits a line
by it into the part kept and the ends cut off, which are drawn dashed and
left out of profiles and of finished sketches' lines. `check` refuses a
corner on anything but two lines (not chamfers) ending at its point, a
circle, an `equal` fillet, one made from its lines' points, and two on
the same corner of the same lines (`SketchError::Corner`). `Sketch::delete`
takes a line's fillets and chamfers with it; deleting a fillet takes its
points and radius and gives the sharp corner back.

A sketch is a feature of the document (`FeatureKind::Sketch`) on a `Plane`,
whose `placement` maps sketch coordinates into the world.

**The origin and axes** (`origin.rs`) are built into every sketch rather
than stored: reserved ids at the top of the counter (`Id::ORIGIN`,
`Id::X_AXIS`, `Id::Y_AXIS`, `Id::is_builtin`), which `next_id` never
reaches (adding past them is `OutOfIds`, a file's `next_id` past them
`SketchError::NextIdReserved`). `Sketch::kind` knows them (a point and two
lines), `Sketch::point` gives the origin at zero, `Sketch::line` each axis
as the origin and a unit along it, and `Sketch::name` names them
("Origin", "X axis"); `Sketch::curve` doesn't give the axes, which are
made of no points. So constraints and dimensions name them like any point
or line, and everything that walks the lists (the Geometry list, counts,
box selection, flattening) never meets them; `Sketch::delete` ignores
them, and an `Add` resolves them as themselves. `check` refuses them used
as they can't be (`SketchError::Builtin`): a curve made from the origin (a
point snapped there is a point of its own, coincident with it), a
constraint or dimension on them alone, or one taking an axis as a segment
(its midpoint, its length, equal to a line, a distance from its middle):
`Constraint::fits_builtins`, `Measure::fits_builtins`.

## The solver (`varde-sketch`, `solve/`)

Two pure functions, runnable anywhere: `solve(&Sketch, &Goal, &Budget)`
and `analyse(&Sketch)`. `notes/SketchImpl.md` ("Built so far (step 2a)",
"(step 7b)") has the details, and the benchmark under "Performance".

- **The system** (`system.rs`): the variables are the points' coordinates,
  circles' radii and a parameter per point on a spline, in `Slot`s; the
  origin and axes are constants. What
  a `Fix` pins is a constant when solving (`Fixing::Constants`) and a
  variable held by equations of the `Fix`'s own when analysing
  (`Fixing::Equations`), so the analysis can name it. Each constraint
  gives one or two equations, each driving dimension one, each arc one
  more (its end at its start's distance from its centre), tagged with the
  id they come from. A tangent where the two curves meet at an end of each
  (an arc drawn on from a line) is the radius there at right angles to the
  line, or along the other arc's radius, not the distance form, which
  would be flat about the tangency and so no equation to the analysis. An
  arc whose three points are each `Symmetric` about one line with a point
  of an arc with a lower id (centre with centre, each end with the other's
  other end, as Mirror makes them) has no radius equation of its own: a
  mirror image keeps distances, so it would restate the other's, and every
  mirrored arc would be refused as redundant (`mirrored_arcs`, by
  `image_of`). A fillet or chamfer (`System::corner`) gives its start on
  the endless line `a` and its end on `b`; a fillet its radius at each end
  at right angles to the line, towards the inside of the corner, as a
  `Residual::Angle` of ±90° from the way along the line from the corner,
  which is no number at its mirror image, so it can't turn inside out; an
  `equal` chamfer its ends as far from the corner. One mirrored with its
  lines, its points, its corner's and its lines' ends each `Symmetric`
  with one of lower id's (or the same point, on the mirror line), and as
  `equal`, has none: they'd restate the other's (`mirrored_corners`). The
  points it shares with the other (its corner's, a line's that's its own
  image) are held on the line instead, as the mirror relation implies, by
  *implied* equations (`Equation::implied`): the analysis counts one only
  where it adds to the rank of the rest, so it's never redundant with
  what already holds the point there (a line on the axis drawn from the
  origin, the tie Mirror adds), and yet holds it once that's deleted. A dimension's
  equation is signed by its side, and an angle's is `2 sin(δ / 2)` of its
  difference from the value, zero only at it and no number half a turn
  off, so no value reaches a mirror image. `components` splits them by the
  variables they share.
- **Equations** (`equation.rs`): a `Residual` each, a length, written once
  over `Real` (`real.rs`) and evaluated with `f64` for the value and
  `Dual` for the gradient. Adding a kind of equation means a `Residual`
  variant, its `slots`, its `eval`, and a line in the finite-difference
  test.
- **Splines** in the solver: each one an equation reads is a
  `SplineSlots`, its points' slots (fit points then tips, or control
  points) and a `SplineMap` (`spline/basis.rs`): its knots and each
  control point as weights of the points (the `Interpolation`'s through
  fit points, weights under 10⁻¹⁵ left out), so it's linear in its
  points. A point on one is two equations (`OnSpline`) at a parameter of
  its own, a variable read as a length along the spline so it's scaled
  like the rest, started at the place nearest the point (a point dragged,
  nearest its target), kept within an open spline's ends (one at an end
  that a step would push past is held there and the step found again
  without it), taken round a closed one. A tangent or a smooth join with
  a spline is `Along` (the two ways along, as an angle's `2 sin(δ / 2)` of
  none, signed by the side) and, smooth, `Curving` (curvatures less each
  other, times the ways' lengths), each curve's way a `Heading`, with
  `at` held on the other (`OnLine`, `OnRound`) unless it's shared. An
  offset pair of a spline and a point (`PairRead::Spline`, in
  `PairOffset` and `EqualOffset`) reads the spline at a parameter variable
  of the point's own per equation naming it, held at the nearest place by
  a `Nearest` (the point's distance along the spline's way from there), so
  each adds a variable and an equation, no freedom. These are
  differentiated by dual numbers over their own few variables and what
  they read of the spline (a place and derivatives), and on by the chain
  rule through the map's weights: `MAX_INPUTS` stays 16 whatever the
  spline's size. A spline through fit points is held with the parameters
  their places gave while it solves; once solved, they're found anew, the
  points on it slid to where they're nearest, and it solves on until the
  equations hold as the spline is. The Jacobian's pattern changes as a
  parameter passes a knot, so the normal matrix is made again when it
  does.
- **Solving**: each component (only those a drag touches) by damped
  least-norm Gauss-Newton, the normal matrix sparse (`normal.rs`, faer),
  warm started from the sketch, until every residual is within 10⁻¹⁰ of the
  sketch's size. A drag puts its points (or radii) at their targets and
  weighs them 10⁶ times the rest. `Budget` bounds the steps and asks a
  caller's callback for the time. Any component failing fails it all
  (`Failure`), with the constraints involved where they can be told.
- **Analysis** (`analysis.rs`): per component, faer's dense column-pivoted
  QR of the Jacobian's transpose gives the degrees of freedom, the fixed
  points and curves and the redundant (or, unsolved, conflicting)
  constraints.

## Profiles (`varde-sketch`, `intersect.rs`, `profile.rs`)

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
  overlap until both are within the tolerance of their chords, polished
  by Newton's method. All bounded by `MAX_MEET_STEPS`; `meet` returns the
  steps, which profiles count as work.
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
- **Faces** are traced round the planar graph of the pieces with the face
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
  pieces and their boxes).
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
  two resolves to the half its point is in. `RegionRef::check(max)` holds
  what a file could get wrong: at most `MAX_REGION_CURVES` ids, lists not
  empty and sorted without repeats, `inside` finite and within `max`.
- **Near misses**: `Profiles::open_ends` are the ends only one piece
  reaches; `Profiles::near_misses(gap)` pairs those within `gap`, which
  is the view's (a few pixels in sketch units), so profiles don't depend
  on the zoom. Bounded by `MAX_NEAR_MISSES` and `MAX_NEAR_PAIRS`.
- **Bounds**: more than `MAX_SPLITS` cuts or `MAX_WORK` steps is
  `TooComplex`, never a long wait.
- **Where it runs**: in the app, once per sketch shown, not in the solver
  lane (a millisecond or two for sketches people draw), so no wire carries
  profiles. `Doc::refresh_profiles`, run by `Doc::sync` and after every
  `Look`, finds them for the sketch as it's shown (committed, with edits
  waiting, or a drag's step: `Doc::shown_sketch`) unless the session's
  `Profiled` is of that sketch already (compared by value), and keeps them
  as `Result<Arc<Profiles>, TooComplex>`, which `SketchState::profiles`
  hands the view. The status bar counts them ("2 profiles") in a sketch
  with curves, or says "Too complex for profiles", and nothing is shaded.

## Edits and proposals (`varde-sketch`, `edit.rs`, `propose.rs`)

What the user does is a `SketchEdit`, an intent applied to whichever
sketch it's proposed on: `Add` (points, curves, constraints, dimensions
and `auto` constraints, the snaps), `Delete` (with what depends on it,
through `Sketch::delete`), `Move` (points and circles' radii, the end of a
drag), `SetConstruction`, `SetDimension` (a new value), `SetDriving` (made
driving, a dimension holds what it measures now) and `MoveLabel`.
`SketchEdit::constrain` is an `Add` of constraints alone. The shape tools
(`shape.rs`, `offset.rs`, `corner.rs`) add `Trim`, `Extend`, `Offset`,
`Mirror`, `Fillet` and `Chamfer`, below.

An `Add`'s new items name each other by placeholder ids: `Add::new`
records the sketch's `next_id` as `first`, and `Add::point` and
`Add::curve` hand out `first`, `first + 1`, ... Applied to a sketch whose
`next_id` has moved on (an edit accepted meanwhile), each placeholder
moves along by as much (`Add::resolve`); ids below `first` are the
sketch's own. `SketchEdit::apply(&sketch, &design)` makes the new sketch,
which passes `Sketch::check` against `design`, or says why not
(`EditError`).

`propose(&sketch, &edit, &design, &budget)` applies, solves (a `Move` as
a drag from the sketch before it, so what's fixed stays; a driving
dimension's new value in steps from the old, each solved from the last,
so the geometry stays on its branch, and a new driving dimension's from
what it measures where it's placed), checks the solution against
`design` and analyses it: `Accepted` with the sketch and its analysis, or
`Rejected` (the edit doesn't apply, a constraint is redundant, or it
doesn't solve) naming the constraints and dimensions involved; one whose
new driving dimensions are involved is `Rejected::Driving`, which could be
added as a reference instead. `auto` constraints are tried together, and
if that fails one by one, each kept only if the sketch still solves with
nothing redundant.

`DragSession` is a drag: each `step` solves the targets from the last
solution that converged, and keeps it if the new one doesn't.

**The shape tools** (`shape.rs`). Every stored curve cuts, construction
ones too (as in other CAD); the origin's axes don't. What a tool ties its
new ends with is `auto`, like a snap: kept if it holds with the rest,
dropped if it restates or contradicts it (`apply_marked` hands their ids
to `propose` as it does an `Add`'s).

- `Trim { curve, near }`: `meet` with every other curve gives the cuts
  along it (places within the tolerance one, the curves cutting there,
  and a cutting curve's end there if one is); those at its own ends cut
  nothing. The span between the cuts either side of the place nearest
  `near` goes: a line or an arc keeps its id for what's before it, gets a
  new curve of its kind for what's after (sharing an arc's centre), or
  just a new end; a circle needs two cuts and becomes an arc, keeping its
  id, numbered as an arc. With no cuts there (a circle cut once, by a
  tangent), the whole curve is deleted as `Delete` would. A new end is the
  cutting curve's end where that's there (the two share it, as a T's stem
  and bar), else a new point with a `PointOnCurve` on each curve cutting
  there. A line cut in two gets the second part's points on the first
  (one line), an arc `Equal` radii (one circle), and the tangents at the
  end that moved to the new curve go with it (`hand_over`). An end no
  curve is made from any more is deleted with what's on it; a line's
  equal lengths, midpoints, driving length and driving distance from its
  midpoint to a line go (`reshaped`), what's on the endless line through
  it stays (parallel, a point on it, an angle, a reference length);
  constraints and dimensions naming a curve and its own point, which a
  shared end can make of a point on it, go. At a tangent point the new
  end's tie restates the tangency to the solver and is dropped, the
  tangent holding it there.
- A **spline** trimmed keeps what's before the first cut
  with its id and gets a new spline for what's after the second, each
  a piece of it: by control points `BSpline::piece`, exact, its control
  points where they stay kept and the rest new;
  through fit points through the fit points between (none within the
  tolerance of an end), their handles kept, and at a new end a handle
  giving the tangent it had there, straying a little between (under
  0.04% of its size in the tests). A closed one needs two cuts and
  keeps what's from the second round to the first, open. A point held on
  it stays on the part it's on (moved to the new spline's constraint) and
  loses the tie where it's on the part taken away; tangents and smooth
  joins at the end the new spline has are handed over.
- `Extend { curve, end }`: a line runs on from `end` (for 4 × the design's
  limit), an arc round the rest of its circle, which is met with every
  other curve; the first place past the end (not back at an arc's other
  end) is where it goes, as a trim's new end: the curve cutting there's
  end, or a new point on it. The old end is deleted if nothing else is
  made from it, else left to the curves sharing it. Nothing ahead is
  `EditError::NothingAhead`. A line's length constraints go as for Trim.
  A spline looks along its tangent at `end` (as a line would) and gets
  the new end as a point more: through fit points a
  fit point, curving on into it through the old end, which stays a fit
  point, with its handle if it had one; by control points a control point
  more, its old knots squeezed into the share of the parameter its old
  polygon's length takes and one more where the extension starts (or
  `control_knots` anew where that leaves knots too close). Tangents and
  smooth joins at `end` naming it go, as it no longer ends there;
  `Sketch::extension` shows the spline as it would be past its old end.
- `Mirror { ids, about }`: the points and curves among `ids` (not
  `about`, nor constraints, dimensions or the origin) reflected in the
  line or axis `about`. A point on the line (within the tolerance) is its
  own image and shared with the copies, held on the line by an `auto`
  `PointOnCurve` unless it's the line's own; a curve all of whose points
  are is its own image and isn't copied. An arc's points always get
  images, even there, so its copy's radius equation follows from the
  symmetry (see the solver). Each new point is `Symmetric` with its
  original, each copied circle `Equal` to it, and an arc's copy runs from
  the image of its end to that of its start, counter-clockwise. The
  constraints among what's mirrored aren't copied: the symmetry holds the
  copies already, so each would be redundant and the edit refused.
  Nothing to mirror is `EditError::NothingToMirror`.
- `Offset { chain, distance, side }` (`offset.rs`): the curves of `chain`
  as a chain (`Sketch::chained`: joined end to end, no point the end of
  more than two, run so the first goes from its start, an arc
  counter-clockwise; or a circle alone; else `EditError::NotAChain`), each
  copied `distance` to `side` (the left of the way it runs positive): a
  line along itself, an arc or circle about its own centre point (shared).
  Where two copies part at a corner, lines run on to meet (up to a turn of
  `MITER_TURN`, 150°), else a round join, an arc about the corner point;
  where they cross, both end where they cross nearest the corner. An arc
  with no radius left has no copy, and circles about its joined ends stand
  in. Every raw curve is cut where its distance from the chain can reach
  `distance` (where it meets another raw curve, the copies either side of
  each piece, or circles round each piece's ends), and each part is kept
  if its middle is no nearer any piece than `distance`, on `side` of the
  piece it's nearest (or of the corner, whose convex side alone is nearest
  it), and not inside a corner run on to meet; parts lying on each other
  both go. That's exact, where clipper2's offset of the flattened chain
  would be only to its flattening (see "Built so far (step 6b)" in
  `notes/SketchImpl.md`). Nothing left is `EditError::NothingLeft`, too much
  work (`MAX_OFFSET_WORK`) `EditError::TooComplex`, a distance that's no
  length `EditError::OutOfRange`. Parts' ends near one place (100 times the
  profiles' tolerance, `NEAR`) are one new point, and so are two loose
  ends (each one part's alone) within `SLIVER` tolerances, the ends of a
  sliver kept where a copy touches another piece's band. Ties: a line's copy
  `Parallel` to it, every copy's offset pair either the one driving
  `Measure::Offset` of `distance` (the lead: a line's copy if one keeps
  its offset, else an arc's, else a round join's) or `EqualOffset` to the
  lead's, and copies meeting tangent at a shared point `Tangent`, which
  stands for one of their ties (`untie`: a matching of tangents to ties, a
  line giving up its offset then its parallel, an arc its offset, the
  lead keeping its offset), as the point where two tangent curves meet
  is no place the solver can tell along them. Round a loop of arcs alone,
  each tangent to the next, the radii that make each tangent to the next
  make the last tangent too, so one arc's copy is made about a new centre
  point of its own, which its two tangents hold. So a closed chain's copy
  adds no freedom; an open chain's has its two ends free along it.
  Chains don't pass through fillets or chamfers (their ends aren't the
  lines'), so a corner's lines are copied sharp. A spline alone is copied
  as a spline of its own (`offset/spline.rs`): through fit points along its
  exact offset (each place moved `distance` along the normal), first where
  its segments meet, then halfway into each span where the copy through
  them strays from the exact offset (compared at a quarter, half and three
  quarters of each span) by more than `SPLINE_FIT` (10⁻⁴) of its size,
  until none do or there are `MAX_SPLINE_POINTS`. Offset towards where it
  turns by as much as its radius of curvature anywhere, the copy would
  fold: `EditError::TooTight`. The first fit point's pair with the spline
  is the driving `Measure::Offset`, every other's `EqualOffset` to it, so
  each fit point slides along the offset (one freedom each), and the copy
  follows the spline reshaped at its fit points, within the fit between.
  A spline in a chain with other curves is `NotAChain`; the preview is
  the exact offset.
- `Fillet { at, lines, radius }` (`corner.rs`): the corner at `at` of
  `lines` (two lines ending there, not chamfers, not parallel, else
  `EditError::NoCorner`) rounded by an arc of `radius` tangent to both,
  from new points, as far back along each as the radius over the tangent
  of half the corner's angle (past a line's other end is
  `EditError::NoRoom`), with a driving `Measure::Radius`. `Chamfer { at,
  lines, setback }` cuts it with a line as far back as the `Setback` says:
  `Equal(d)` along both (an `equal` corner, one `Distance` from the corner
  to its start), `Two(d_a, d_b)` (a `Distance` each), or `Angle(d,
  angle)`, `d` along the first and at `angle` to it inside the corner (a
  `Distance` and a `Measure::Angle` between the first line and the
  chamfer, whichever way round measures `angle`; past half a turn with the
  corner's angle is `NoRoom`). Either replaces a fillet or chamfer on the
  same corner. `Sketch::corner_lines(at, toward)` picks a corner's two
  lines, the one running nearest `toward` first; `fillet_preview`,
  `chamfer_preview` give what the edits make, and `fillet_through`,
  `chamfer_through` the size that passes where the cursor is.
- **With the other tools.** Trimming a fillet or chamfer deletes it;
  extending one is `Target`. Trimming or extending a line so its corner's
  point is no longer an end of both lines deletes the corner's fillet or
  chamfer (`drop_broken_corners`), and a line trimmed in two hands those
  at its end to the part keeping it (`hand_over`, with the tangents
  there). A line's new end is never a point of a fillet or chamfer on it,
  which would stop being one: trimmed between a crossing and where its
  fillet meets it, it ends at a new point there. Mirror copies a
  fillet or chamfer mirrored with its lines (or a line on the mirror
  line, or the mirror line itself, its own image) as a corner of the copies, a fillet's lines
  swapped since it runs the other way; its equations are left out (see
  the solver).

## The solver lane (`varde-solve`)

Proposals, drag steps and analyses run off the UI thread in a lane per
open document, natively a thread (`thread.rs`) and on the web a Web Worker
(`worker.rs`, the `varde-solve-worker` binary), built on `varde-lane` like
the regeneration lane. `notes/SketchImpl.md` ("Built so far (step 2c)")
has the details.

- **Requests and responses.** `Propose { base, sketch, edit }` answers
  `Accepted` or `Rejected`, tagged with `base`; `Drag { session, sketch,
  points, radii }` answers `Dragged { session, solution }` only when the
  step converged; `Analyse { revision, sketch }` answers `Analysed`. Each
  carries the design's `units` too, which sketches are checked in. A
  request that panics, or that a worker died on, is `Failed` with its
  `Tag`.
- **The `Solver`** answers them (`Solver::handle`), keeping the drag
  session: a step of the session in progress goes on from its last
  solution and ignores the sketch it carries; another session replaces
  it, starting from its sketch. Proposals get `PROPOSAL_TIME` per solve, drag
  steps `DRAG_TIME`, on the platform's clock (`clock.rs`). Tests answer
  requests with a `Solver` of their own.
- **Order** (`order.rs`): proposals and analyses queue in order, a newer
  analysis replacing one waiting; drag steps are latest wins in a slot of
  their own, and a step of a session older than one sent is dropped. The
  queue and the slot take turns, so a proposal waits for one drag step at
  most, and a drag step for one proposal.
- **On the web** the requests wait on the page in a `varde_lane::mailbox`
  holding the same `Order`, one with the worker at a time. The page posts
  a drag session's sketch only with the first step a worker gets of it, and
  checks every reply against the request it had (`wire.rs`): sketches
  checked, ids named in an analysis or a rejection those of the sketch
  sent or of the one the edit makes, a drag solution a place per point and
  a radius per circle. The worker checks what it's sent the same way.
- **In the app** `Varde::solve_lane` starts the lane for the open
  document and hands it to `Doc::solver`; what's asked of it is under
  Editing.
- **Unsolved sketches.** The regeneration lane solves every sketch warm
  started (`regen::unsolved`, a settle within the default iterations) and
  names those that don't solve with its model (`Regenerated::unsolved`,
  on the wire in the head). The Timeline marks them "Doesn't solve"; the
  status bar says so in a sketch until its analysis comes.

## Editing

The sketch being edited is `Doc::sketch`, a `SketchSession`
(`crates/app/src/doc/sketch.rs`): the feature, the tool in use (`Drawing`)
or the Constrain tool, the selection (a `BTreeSet<Id>` shared by the
viewport and the lists), the item hovered in a list or by its glyph, a
drag in progress, the sketch with the edits waiting on the solver applied,
the analyses, the last refusal, whether glyphs show, the lists' scroll
offsets, the value field and a dimension's label grabbed. `Doc::prune`,
run by `Doc::sync` after every change, keeps it to what the sketch as
worked on holds: ids gone drop out of the selection and the hover, the
value field, a label grabbed and the Dimension tool's picks go with what
they're on, so do a shape tool's, and Offset's chain once it's no chain,
Fillet's or Chamfer's corner once its lines no longer make it
(`shape::still_picked`), a drag started from another generation of the document is put
back, and a Line chain whose last point is gone (undone) starts afresh.
Across a replacement of the whole document (restoring recovered changes,
or undoing or redoing that: `Editor::lineage` changes) ids may name other
items, so the selection is cleared and the tool starts its shape afresh;
restoring drops the edits waiting on the solver, as undo does. The sketch
session only resets, since it reads the sketch its id names now and never
writes back what it read before; an extrude session holds values read
before the replacement, so it ends instead (`Doc::prune_extrude`). A
document that can't be edited, as a Save As can leave it, has no tool and no drag;
the Save As answer syncs for that.

Every change to the sketch is a `SketchEdit`, proposed to the solver lane
(`Doc::propose`, `doc/sketch/propose.rs`) and committed once it's
accepted as the whole new sketch, solved, one `Command::SetSketch`, one
undo step. One that can't even be applied to the sketch as it's worked on,
such as one past the coordinate limit, is refused at once and the status
bar says why (`EditError::Sketch`).

- **Proposals** (`Proposals`, in `Doc`): one is with the lane at a time,
  `Propose { base, sketch, edit }` on the sketch committed now, and the
  rest queue behind it in order. While any wait, the session's `Waiting`
  is the committed sketch with them applied (unsolved): what's shown, with
  the items they add faded, and what the tools draw on, so a chain's next
  line goes on from a point still waiting. An `Add` made on that names the
  waiting items by their ids to be, which apply only once those are
  accepted (`Add::apply` refuses an id past the sketch's own). `Accepted`
  commits and keeps the analysis it carries for the new revision;
  `Rejected` changes nothing and keeps the refusal in the session
  (`Refusal`): the status bar says why ("Would over-constrain the
  sketch") and the constraints involved, with what they tie together, are
  red, until the next action (an edit, a click, a tool, `Esc`). A lane
  that fails on one (`Failed`) says so the same way. An answer for a
  revision that's no longer the document's (something else was committed
  meanwhile) is proposed again. Undo while any wait drops them all instead
  of undoing, and the answer to the one with the lane is ignored when it
  comes (`dropped` counts them: the lane answers proposals in order).
  Redo does nothing while any wait: they come after what's undone, as a
  new edit would, and an `Add` names the items before it by id. After ~100 ms of waiting (`CHECKING`, told by frames while waiting) the
  status bar says "Checking…". Until the lane has started, proposals wait
  in the app.
- **Saving and closing wait for them**: Save and Save As asked for while
  proposals wait are kept (`Saves::waiting`, which counts as saving) and
  sent once they're answered or dropped (`Doc::proposals_settled`), so
  what's on screen is saved; leaving waits for them first
  (`Step::Proposing`), then asks about unsaved changes as usual, unless
  the user already chose to discard. Auto-save doesn't wait: it saves
  what's committed. An extrude's OK waits too (`Doc::extrude_ready`),
  since its regions are of the sketch the edits change: an `Enter` in
  the window is dropped, and the panel says "Checking the sketch…" after
  `CHECKING`.
- **Analysis**: on entering a sketch, and whenever its revision has none
  (undo, redo), the app asks `Analyse` of the committed sketch, one at a
  time (answers name only the revision, which two sketches share); an
  accepted proposal brings its own. The session keeps the last eight by
  revision (`Analyses`). It colours the sketch, gives the status bar
  "Fully constrained", "N degrees of freedom left", "Doesn't solve" or
  "Over-constrained", and marks conflicts red in the lists and viewport.
- **Tools** (`Tool` in `varde-view`, keys `L`, `R`, `C`, `A`, `G`, `N`,
  `P`, and `D` for the Dimension tool under Dimensions, or the toolbar): the
  viewport sends a `ToolClick` per press, with the sketch point snapped (see Snapping
  below) and what it snapped to (`target`, `inference`), the point or
  curve under the cursor if any (what the Dimension tool picks), a pixel's
  size there, whether it's a double-click and whether the reference
  modifier is held. `Drawing::placed` keeps the points placed so far until
  the shape has all it needs, and `Drawing::targets` what each snapped to:
  - Line: each click after the first adds a line from the last point,
    sharing it (`Chain`); clicking the chain's first point once it has two
    lines or more closes the loop and ends the chain; `Esc` or a
    double-click ends an open one.
  - Circle: centre, then a point on it.
  - Arc: start, end, then a point on it; stored counter-clockwise with its
    centre computed (`arc_through`).
  - Rectangle: two opposite corners, or with `Drawing::centered` (`Q`,
    `Look::ToggleCentered`, kept while the tool is, shown in the toolbar's
    tag) the centre and a corner. Four lines sharing four corners, with
    `Horizontal`, `Vertical`, `Horizontal`, `Vertical` constraints (real
    ones, not `auto`); from the centre, a construction diagonal between
    two opposite corners with the centre point as its `Midpoint`. The
    first corner (or the centre) and the one clicked are placed as a
    shape's points (`place`), the other two are new. Four degrees of
    freedom either way.
  - Polygon: centre, then a corner. A construction circle made from the
    centre point (so no concentric), `Drawing::sides` corners on it
    (`PointOnCurve` each), the first where the click snapped, and lines
    between them, each `Equal` to the first: four degrees of freedom.
  - Point: a lone point per click; none where the sketch has one.
  - Spline (`N`, `doc/sketch/spline.rs`): each click places a point
    (`Drawing::placed`, what it snapped to in `targets`), none within a
    pixel of the last, on a point of the sketch's it has already, or past
    `MAX_SPLINE_POINTS`; nothing is proposed until it ends. A click on its
    first point (within `SNAP_TOLERANCE` pixels, `ActiveTool::closes`)
    once it has three ends it closed; a double-click, or `Enter`
    (`Edit::PlaceShape`, `Doc::end_spline_here`), once it has enough
    (`ActiveTool::spline_ends`: two fit points, four control points) ends
    it open where it is. The spline is an `Add` of its points placed as
    any shape's (`place`: a point of the sketch's its own, else a new one
    with the snap's `auto` ties) and the curve, by control points with
    `control_knots` of the places. `Q` (`Look::ToggleSplineKind`,
    `Drawing::control`, kept from one spline to the next, shown in the
    toolbar's tag) switches between through fit points and by control
    points, the points placed kept. The preview (`typed::outline`'s
    `Outline::Spline`) is the spline through the points placed and the
    cursor (`flatten_spline`), straight between them while too few, with
    its control polygon dashed by control points. It has no fields.
  - Trim (`T`), Extend (`J`), Offset (`O`), Mirror (`W`), Fillet (`F`) and
    Chamfer (`B`), the shape tools
    (`doc/sketch/shape.rs`, `Doc::shape_click`), don't snap (`Tool::draws`
    is false) and click what's under the cursor (`ToolClick::hit`): Trim
    proposes `Trim` of the curve clicked there, Extend `Extend` of the end
    of the line or arc clicked nearer the click (`Sketch::nearer_end`),
    both staying in use. Mirror picks (`Drawing::picked`): taken up with
    points or curves selected it has them and waits for the line
    (`Drawing::about`); else each click picks what it's on or takes it out
    again, and `Enter` (`Look::MirrorAbout`, one binding with `PlaceShape`
    as a drawing tool's fields never go with it) goes on to the line. The
    click on a line or axis proposes `Mirror` and starts it afresh. `Esc`
    lets go of the picks first. Extend is `J` and Mirror `W`: `E` is
    Equal's and `M` Midpoint's, and those are free letters. Offset, Fillet
    and Chamfer pick, then place (`Tool::places`): a click picks
    (`Tool::pick`, into `Drawing::picked`), and once picked, a click
    places what they make of it (`Doc::placed`), drawn through the cursor
    meanwhile. Offset picks the chain the curve clicked is in
    (`Sketch::chain_of`; taken up with a chain, or a curve, selected, that
    one, `shape::offsettable`); then it has a field,
    `Field::Distance` (`ActiveTool::fields`), so the viewport sends
    `Look::Aim` as the cursor moves, and a click (sent where the button's
    let go, `Input::releasing`, so it can be dragged there; `Esc` while
    it's held sends none), or `Enter` at the aim, proposes `Offset` of the
    chain on the side of it the click is (`Sketch::offset_side`), as far
    off as the click, as the design's units show it, or the distance
    typed; less than a pixel off is no distance meant. It then starts
    afresh, picking. The equal offsets are
    `ConstraintKind::Offset`, listed and glyphed (on the copy each ties)
    but not among `ConstraintKind::ALL`, what's applied to a selection:
    it has no key. Fillet and Chamfer (`Tool::corners`) pick a corner the
    way Offset picks a chain: the click's `hit` is the
    point where lines make one (`hit::hit_corner`), and `picked` is it and
    its lines from `Sketch::corner_lines`, the one nearer the click first
    (taken up with two lines selected that make one, those,
    `shape::cornered`). Then a click where the button's let go, or `Enter`
    at the aim, proposes `Fillet` or `Chamfer` through there
    (`typed::corner_outline`), or as typed in its fields: the fillet's
    `Radius`; the chamfer's `Distance` along its first line, then
    `SecondDistance` or `Angle` (one typed lets go of the other,
    `typed::setback`), less than a pixel being no size meant. It then
    starts afresh. `B` for Chamfer: `C` is the Circle tool's.

  The shapes are worked out by `varde_view::typed::outline`, which the
  viewport's preview draws too, so what's shown is what's placed.

  What a click snapped to goes into the `Add` (`edit.rs`, `place`): a
  point of the sketch's is the shape's own (a line from it joins it; the
  Line tool joins existing points this way), anything else a new point
  with `auto` constraints tying it there: coincident with the origin, a
  midpoint, on a curve or an axis, a quadrant as on the circle and level
  with its centre. A circle's rim or an arc's last point on a point passes
  through it (point on curve), and what the line inferred is a horizontal,
  vertical, parallel, perpendicular or tangent (`tangent_between`, on the
  side the geometry is) of the new line or arc. The solver drops the
  `auto` ones that restate the rest (see Edits and proposals), so a snap
  never fails an edit. The session keeps where the next click snaps
  (`SketchSession::snap`, from `Look::Snap`, or `Look::Aim` while the
  shape has fields, see Typed values) for the glyph, until a click or
  another tool; what it snapped to that an undo takes is let go of
  (`Snap::within`, in `Doc::prune`), the place kept.

  A shape smaller than a pixel (a line or radius shorter than one, an arc
  whose third point is within a pixel of the line through its ends), or a
  line over one joining the same points (back to the point before, or
  closing a chain of one line), is refused: nothing is committed and the
  tool waits for a better click.
  Tools stay in use after a shape; `Esc` drops the shape being drawn, then
  the tool, then leaves the sketch. Tools only exist while the document
  can be edited.
- **Typed values** (`doc/sketch/typed.rs`, `varde_view::typed`): once a
  drawing tool's shape has a point, it has fields (`typed::fields`: a
  line's `Length` and `Angle`, a rectangle's `Width` and `Height`, a
  circle's `Diameter`, an arc's `Radius` once its ends are placed, a
  polygon's `Sides` and `Diameter`). While it has, the viewport sends
  `Look::Aim` with the click it would take on every move (not only when
  the snap changes; it carries the snap too), which the session keeps
  (`SketchSession::aim`, also set by each click; with the cursor off the
  sketch, gone from the viewport or over a widget on it, the aim, its snap
  and the preview stay where they were) so the fields show by it
  (`Sketching::fields`, a fifth `Anchors` layer, `Anchors::beside` the
  cursor), each with the value typed or what it measures of the preview.
  `Tab` (`Look::NextField`; one binding with the Dimension tool's
  `SwitchRound`, as the two never go together) opens the value field
  (`ValueTarget::Field`, the step 3c field: `VALUE_FIELD`, `Doc::take_focus`,
  `OnEscape`) on the first field, or takes the text of the one open
  (`Doc::take_field`: `typed::read` for the field's `ask` in the design's
  units, an arc's radius at least half its chord, the sides a whole number
  from 3 to 64) and moves on; empty text lets go of the field's value
  (the sides keep theirs). A value refused keeps the field open with why,
  its span selected, as a dimension's does. Values taken are
  `Drawing::typed`, dropped with the shape (`restart`); the sides are
  `Drawing::sides`, kept from shape to shape. A click, or `Enter`
  (`Edit::SubmitValue` from the field, `Edit::PlaceShape` without it,
  both at the aim), takes the open field's text first (refused, the click
  places nothing), then `typed::aim` moves the click to where the values
  hold the shape's point (a line's end, a point on a circle, the arc's
  middle, the rectangle's far corner, the polygon's first corner),
  dropping what it snapped to wherever a value is typed, and how it was
  inferred to run where an angle or radius is. The values go into the
  shape's `Add` as driving dimensions (`typed_dimensions`: a line's
  `Length` and `Angle` from `Id::X_AXIS`, `Side::Positive`, the
  rectangle's `Length`s of its sides at the first corner, a `Diameter` or
  `Radius`), their labels `LABEL_GAP` pixels off, anchored in the sketch
  the `Add` makes. So a shape with its sizes is one proposal, one undo
  step. The snap's glyph shows only what the values typed leave of the
  snap.
- **Splines** (`doc/sketch/spline.rs`, what's done to them pure in
  `varde_view::spline`): `Q` with splines selected and no drawing tool
  (`Edit::ConvertSplines`, one `switch_binding` with the Rectangle
  tool's and the Spline tool's, as they never go together) proposes a
  `Convert` to the other kind for each (`spline::conversions`), a
  proposal each. `Shift H` (`Edit::ToggleHandles`, `spline::handles`)
  gives the fit points selected (of splines through fit points) handles,
  or failing any, the ends of the open splines through fit points
  selected; if they all have them, it deletes their tips. A
  double-click on a spline without a tool (`Edit::InsertSplinePoint`)
  proposes `InsertPoint` there. `U` (`Look::ToggleComb`,
  `SketchSession::comb`, `SketchState::comb`) shows the curvature comb of
  the splines selected, or hides it, in any sketch. With splines selected
  and no drawing tool the toolbar offers Convert, Handles and Comb. The
  Dimension tool takes a handle's tip alone as its angle from the X axis
  (`Measure::Angle(Id::X_AXIS, tip)`); with its fit point, as two points,
  its length.
- **Construction**: `X` turns the tool's next shapes into construction
  geometry, or without a tool drawing shapes (the Dimension tool draws
  none) the curves selected: all construction unless they all are, then
  all normal.
- **Delete** removes the selection through `Sketch::delete`.
- **Dragging** (`Look::DragGeometry`, then `Edit::DropGeometry`) is a
  drag session of the lane's: each step sends `Drag { session, sketch,
  points, radii }` with the `Move` to where the cursor is (a point or a
  line with both ends by the cursor's movement; a circle or an arc grabbed
  by its edge takes the cursor's distance from its centre as its radius,
  an arc's ends moving along their directions from the centre), and the
  view shows the last solution that converged (`Dragged`), the committed
  sketch until the first. A step that would put anything past `MAX_COORD`
  is left out. Dropping proposes the `Move` on that last solution (`from`,
  while the document is still at the revision the drag started from), so
  it commits what was shown, solved, as one undo step. `Esc`, an undo, any
  other edit or taking up a tool ends it with nothing to put back. Only
  the grabbed item is moved; the rest follows as the constraints demand. A
  drag doesn't start while proposals wait. Solving drag steps inline was
  measured and not taken, see "Dragging" in `notes/Threading.md`.
- **Constraints**: the selection's geometry is constrained by a
  `ConstraintKind` (`varde-view`, `constrain.rs`): `make` builds the
  constraints it makes of the selection (several items tie the first to
  each of the rest where the kind takes two, or one each; a tangent's side
  from `Sketch::tangent`; nothing naming a curve's own point), proposed as
  one `SketchEdit::constrain`. Keys: Coincident `I`, Horizontal `H`,
  Vertical `V`, Parallel `Shift P`, Perpendicular `Shift R`, Tangent
  `Shift T`, Smooth `Shift S`, Equal `E`, Concentric `Shift C`, Midpoint
  `M`, Symmetric `Y`, Fix `Shift F`, bound only while they fit
  (`DocumentKeys::constraints`); Shift where a tool has, or will have, the
  letter (Trim `T`, Fillet `F`, Point `P`, Circle `C`, Rectangle `R`, New
  sketch `S`). A spline takes a point on it (Coincident), a tangent or a
  smooth join at an end (`Sketch::joint`) and a fix; with a spline
  selected nothing else is offered. The Constrain tool (`K`,
  `SketchSession::constraining`, never with a drawing tool) lists in the
  toolbar the kinds that fit the selection (`ConstraintKind::fitting`),
  the most likely first: those tying items together first, and of
  horizontal or vertical, parallel or perpendicular, concentric or not,
  the one the geometry is nearer to; applying one clears the selection.
  `Esc` puts it down.
- **Selection**: a click selects what it hits alone, or nothing;
  `Ctrl`-click (`Cmd` on macOS) adds or takes out. A box selects what's
  inside it dragged left to right, what it touches right to left, `Ctrl`
  adding to the selection. `Space` clears the selection everywhere: the
  sketch's in a sketch, else the Timeline's. A row of the Geometry or
  Constraints list, or a glyph, sends `Look::ClickRow`, which the app
  turns into a click adding with the command modifier held (tracked from
  `ModifiersChanged`, `Message::CommandHeld`). Hovering one sends
  `Look::HoverItem`, kept in the session (`hovered`), which the viewport
  highlights: geometry itself, a constraint by what it ties together. The
  selection and hover may name items still waiting on the solver.
- **The Constraints list** (lower half of the Sketch tab) shows the
  constraints and dimensions on the points and curves selected, or
  selected themselves, or all when no point or curve is; while only
  constraints and dimensions are selected it keeps listing on the points
  and curves it did (`SketchSession::listed_on`, kept by
  `Doc::list_selection` and `Doc::prune`), so a row clicked, or
  double-clicked, stays where it is; those in conflict first, in red,
  then by id; each as "Tangent · Line 2, Arc 1" or "Length 40 mm · Line
  3" (a reference's value in brackets); only the rows in view laid out,
  like the Geometry list. Double-clicking a driving dimension's row opens the value
  field in it. Its header's eye hides and shows the glyphs
  (`Look::ToggleGlyphs`).
- **Dimensions** (`doc/sketch/dimension.rs`). The Dimension tool (`D`,
  `Tool::Dimension`) picks with each click (`ToolClick::hit`) rather than
  placing points: `varde_view::dimension` says what the picks measure
  (`joins`, `measure`: a line's length or, the label off its ends, its
  horizontal or vertical extent; two points the same way; a point and a
  line; two lines' distance if parallel, else the angle whose corner, or
  the one across from it, the label is in; a circle's diameter, an arc's
  radius, `Tab` switching, `Look::SwitchRound`). A click on something that
  joins what's picked picks it, on something else starts afresh from it,
  and on nothing, on a pick, or with two picked places the dimension with
  its label there. The selection is picked when the tool is taken up, if
  it measures something. Placing opens the value field
  (`SketchSession::value`, a `ValueEdit`: its `ValueTarget`, the text, the
  error, whether it's in the Constraints list) with what it measures now;
  double-clicking a driving dimension's label or row opens it on that one
  with its expression as typed. The app asks iced to focus it with its
  text selected (`Doc::take_focus`, `varde_view::VALUE_FIELD`). `Enter`
  (`Edit::SubmitValue`) reads it with `varde_expr` for the measure's `ask`:
  a new dimension is an `Add` (driving), a changed one a `SetDimension`;
  text that isn't such a value keeps the field open with why, the span
  selected. `Esc` (`Look::CancelValue`, the field's `OnEscape`) closes it,
  as does any other edit or click in the sketch. With the reference
  modifier (`Alt`) held, the placing click adds a reference at once, no
  field, holding what it measures (`Sketch::held`, as `SetDriving` does);
  in the Dimension tool `Alt` doesn't peek (`Doc::peeks`). Placing a
  distance between two points at one place (`Sketch::same_place`: no
  telling which way to move them apart, so a driving one can't solve) is
  refused saying so (`EditError::SamePlace`, also refused by
  `SketchEdit::apply` for a driving one added or made driving), as is a
  reference, or one made driving, whose measure no dimension can be,
  such as a distance past the limit (`EditError::OutOfRange`); the picks
  start afresh.
  `Shift D` (`Edit::ToggleReference`) turns the dimensions selected
  between driving and reference, a `SetDriving` each. Labels are dragged
  (`Look::PressLabel`, `Look::DragLabel`, `Edit::DropLabel`) into a
  `MoveLabel`, shown meanwhile as an offset. A refused driving dimension
  (`Rejected::Driving`) says in the status bar how to have it as a
  reference.
- **Units**: the file menu sets the design's (`Edit::SetUnits`, a
  `Command::SetUnits`); dimensions show in them (`varde_expr::format`).
  Chosen while edits wait on the solver, whose values were read in the
  units before, they're kept (`Doc::units_waiting`) and set once the
  edits are all answered; undo dropping the edits drops them too.

## In the viewport (`crates/view/src/viewport/sketch.rs`)

The sketch being edited is drawn by the renderer, over everything else,
and its input is the viewport's `shader::Program`'s (`agents/viewport.md`
has the renderer's side and how events are shared with the camera).
`Sketching` is the sketch as the viewport shows it; `Input` what the
widget keeps of it.

**Drawing.** The view builds two `SketchLayer`s in the `Program`'s
`draw`: curves as polylines in sketch coordinates (flattened on the CPU,
`Sketch::flatten`), projected by the renderer, so orbiting redraws nothing
on the CPU. Construction curves are dashed, points are discs with a rim,
and what's selected is drawn over the rest in the selection colour.
The ends of lines a fillet or chamfer cuts off are dashed too
(`Sketch::cut_back`, `cut_line`). A spline's handles are lines from their
fit points to their tips, in its colour; a selected one
shows where its ends' handles would be where it has none, dashed in the
preview colour with a point at the tip (`Sketch::handle_tip`), and by
control points its control polygon dashed in the construction colour.
Dimensions' lines (`viewport/sketch/dimensions.rs`, pure: extension lines
out to the dimension line through the label, reaching it if it's off the
end; an arc around an angle's corner; a radius or diameter out to the
label; a line extended to a foot past its end) go with the sketch;
their arrowheads, a fixed size on screen, are made each frame where their
ends project (`SketchLayer::triangle`), from ends worked out with the
base layer and kept with it, a hovered dimension's in the hover colour. Each
item is coloured by its state (`States`): free in the sketch colour, fixed
by the analysis darker (`SketchColors::fixed`, a point filled with
`PointStyle::fixed`), in a conflict red (`conflict`: the analysis's
redundant constraints and a refusal's, with what they tie together), and
faded while waiting on the solver. Colours are the palette's
`SketchColors`, light and dark.

- The **base** layer, the sketch and its selection (under it the regions
  of its profiles, each filled on its own in `SketchColors::region`, since
  a hole's inside is a region too and one even-odd fill of them all would
  leave it out; the origin's axes, as far as a sketch reaches, and the
  origin, fixed, in `SketchColors::axis`, or selected or in a conflict),
  is built only when the sketch, the selection, the states or the colours
  change (compared by value with what it was built from, kept in
  `Input`), or the profiles (compared by `Arc`, as the app finds them
  again only for another sketch), and handed to the renderer in the same
  `Arc` otherwise, so it isn't uploaded again. A drag
  shows the solver's solution in place of the document's sketch, so while
  dragging the base layer is built at each solved step.
- The splines' **curvature combs**, while `SketchState::comb` is on, of
  the splines selected: their teeth (`Sketch::curvature_comb`) are worked
  out with the base layer and kept with it (`Base::combs`, the flag in
  `Drawn`), and drawn in the live layer each frame (`combs`) in the guide
  colour: each tooth from its place away from where the spline turns, the
  longest `COMB_LENGTH` (48) pixels at the target (`Projector::pixel`),
  the rest in proportion, and a line through their tips, so zooming keeps
  its size on the screen.
- The **live** layer, rewritten every frame: the box being dragged (a
  translucent fill and its outline in screen space, dashed when it selects
  what it touches), the item under the cursor and what's hovered in a list
  or by its glyph or label (a dimension with what it measures), a shape
  tool's preview (`Sketching::shape_preview`: Trim's piece to go, in
  `SketchColors::conflict`, over the curve hovered; Extend's extension in
  the preview colour; Offset's chain hovered, then picked as selected with
  its copy through the cursor, or the distance typed, in the preview
  colour (`placed`); Mirror's picks as selected and, choosing its line,
  their mirror images in the line hovered; Fillet's and Chamfer's corner
  hovered with its lines, then picked as selected with the fillet or
  chamfer through the cursor, or as typed, in the preview colour), the
  dimensions' arrowheads, without a tool the region under the cursor when
  no item is (`Profiles::region_at`, filled in
  `SketchColors::region_hovered`; the cursor moving asks for a redraw only
  when it's another, `Input::region`), a ring in `SketchColors::near_miss`
  between each pair of open ends within `NEAR_MISS_GAP` (6) pixels at the
  target (`Projector::pixel`), paired again only when the profiles or the
  zoom change (`Input::near`), and the tool's preview (the rubber-band line,
  the circle or arc through the cursor where it snaps, with what it snaps
  to highlighted and the snap's guide, the points placed; the Dimension
  tool's picks and the dimension it would place with the label at the
  cursor, or the one the value field places).

**Input.** `Input` keeps the cursor, the item under it (hover isn't app
state: only the viewport shows it), the left button held down and whether
it has moved past `DRAG_DISTANCE`, the last press of a tool for telling
a double-click, the last click without one (`last_select`: a second on a
spline in an editable sketch, soon and near, sends
`Edit::InsertSplinePoint` at the place pressed rather than a click), and
whether the button's held with a tool that clicks
where it's let go (`releasing`: Offset, Fillet or Chamfer placing); the
modifiers are the widget's. A press on an item of an editable sketch drags
it once it moves; elsewhere, or read-only, it drags a box. `Esc` lets go
of what the button holds and keeps the key from the
app: a box is only the viewport's, and a drag of geometry sends
`Look::CancelDrag` for the app to put it back, which does nothing else,
even if the drag hasn't moved anything yet. The cursor is a crosshair
with a tool, a pointer over an item to select, and grabbing while
geometry is dragged.

**Widgets over it** (`crates/view/src/anchors.rs`): `Anchors`, a layer of
widgets each centred where a sketch point shows (or with
`Anchors::beside` its top left corner off it), laid out only if that's
inside the viewport, and taking nothing off its widgets. Five are
stacked: the glyphs, nudged apart; dimensions' labels
(`Sketching::labels`), each where it's put, a chip with the value in the
design's units (a reference's measure in brackets) coloured by state,
pressed to select and grab it, double-clicked to edit it; the value
field (`Sketching::field`) at the label it edits or places, in a layer of
its own so its widget state stays its own; the glyph of where a drawing
tool snaps (`Sketching::snap_glyph`: the icons of the constraints it
makes, what the point is on then how the shape runs, `SNAP_OFFSET` down
and right of the place); and a drawing tool's fields beside where its
click would go (`Sketching::fields`, see Typed values), a layer of their
own for the same reason as the value field's, since the snap's glyph
comes and goes as the field is typed in. The first holds the
constraints' glyphs (`Sketching::glyphs`, unless hidden): each an SVG icon
on a chip (`theme::glyph`, outlined red in a conflict; the icon in the
selection colour when selected, faded while waiting), a `mouse_area`
selecting it on a click and hovering it. Where it goes is
`viewport/sketch/glyphs.rs`: at the point a coincident, point-on-curve or
midpoint is about, between two points, at a line's middle or a circle's
upper right, at the corner of perpendicular lines (between their middles
if they meet far off), where a tangent's curves touch (with a spline, and
a smooth join, at the end it's named at), at a concentric's
centre, one on each of two parallel or equal items, and an equal
offset's on the copy it ties.
`Anchors::nudged` centres each `GLYPH_OFFSET` up and right of its anchor
and moves one that would overlap a glyph laid out before it rightwards
until it doesn't (a grid of cells keeps the test local, at most 16
moves).

A label's press is its widget's, so the viewport follows a grabbed label
(`SketchState::label_drag`) from where the cursor last was: past
`DRAG_DISTANCE` it sends `Look::DragLabel`, and the release
`Edit::DropLabel`.

## Snapping (`crates/view/src/snap.rs`)

`snap(sketch, tool, cursor, pixel)`, pure: where a drawing tool's next
click goes and why (`Snap { at, target, inference }`), from candidates
within `SNAP_TOLERANCE` (8) pixels of the cursor, the first kind found
winning, the nearest of it: a point of the sketch's or the origin; a
line's midpoint or a circle's or an arc's quadrant; with the Line tool a
tangent point on a circle or an arc from the line's start, or with the
Arc tool's last click the arc through its ends tangent at one to the
line or arc ending there; where a curve or an axis crosses a direction
inferred from the line's start; the nearest place on a curve, the
sketch's before the axes (on a spline, `Sketch::nearest_on`); a
direction inferred alone. The directions,
preferred in this order where as near: horizontal, vertical, tangent to
an arc ending at the start (or a circle or arc it's on), perpendicular
then parallel to a line ending there (or it's on); none within `MIN_RUN`
pixels of the start. A click that isn't one of the shape's points (a
circle's rim, an arc's last point) snaps only to points, and for an arc
the tangent arc; the Dimension tool doesn't snap. A rectangle's corners
(or centre) and a polygon's centre and corner snap as a shape's points,
with nothing inferred.

The viewport computes the snap as the cursor moves, unless `Held::FREE`
(`Shift`) is held, and sends `Look::Snap` when it changes (on every move
along a curve, since the place moves), so the app has it for the glyph;
the click carries the snap it had. While the shape has fields it sends
`Look::Aim` instead, and with the cursor off the sketch sends nothing, so
the preview (drawn at the app's aim then), the glyph and `Enter` stay
where it was last aimed. The live layer draws the preview to the snapped
place, highlights what it snapped to, and draws the snap's
guide dashed in `SketchColors::guide` (`Snap::guide`: from the start on
past the place along a direction, or from a circle's centre to its
quadrant).

## Hit testing (`crates/view/src/hit.rs`)

Pure functions, tested headless:

- `Projector::cursor` intersects the cursor's ray with the sketch plane:
  `None` when the ray runs along the plane (the camera's axes are `f32`, so
  within `PARALLEL` of it), meets it behind the eye in perspective, or past
  `MAX_COORD`. It also gives a pixel's size there, in sketch units, which
  turns the pixel tolerance (`HIT_TOLERANCE`, 6 pixels) into sketch units
  and is the size under which the tools refuse a shape.
- `hit` finds the nearest point within the tolerance, the origin
  included, and failing one the nearest curve as drawn (lines and circles
  exactly, arcs and splines by their polyline), and failing one the
  nearest axis.
  With Trim or Extend, or Offset picking its chain, the viewport hits
  curves alone (`hit_curve`), so a click by a line's end is the line, and
  Mirror choosing its line lines and axes alone (`hit_line`), Fillet and
  Chamfer picking a corner the points where lines make one, within 12
  pixels (`hit_corner`); Offset, Fillet and Chamfer placing hit nothing.
  Pressing the origin or an axis without a tool selects it but never
  drags it.
- `in_box` projects points and flattened curves and tests them against the
  box on the screen: wholly inside, or touching (any segment meeting it).
  What's behind the eye is never inside.
