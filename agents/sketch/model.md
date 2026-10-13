# The model (`varde-sketch`)

A `Sketch` holds points, curves (`Line`, `Circle`, `Arc`, `Spline`), constraints
and dimensions in vectors sorted by id, with `next_id` handing out ids
shared by all four, so an id names one thing and never changes.
`add_point`, `add_curve`, `add_constraint` and `add_dimension` give the
next id (running out is `OutOfIds`) and a number
per kind for the name ("Line 3"); `Sketch::point_name` names a point of a
circle, arc or spline by control points by its role ("Centre of Arc 1",
"Control point 2 of Spline 1"), which `Sketch::name` and the Geometry
list use. Curves name their points by id, so lines
drawn in a chain share the point between them. An arc runs
counter-clockwise from `start` to `end` around `center`; until the solver
holds its radii equal, the radius changes evenly along it. A closed arc
(`Curve::closed_arc`) has its `end` its `start`: it runs all the way
round (`arc_sweep` of a vector to itself is a full turn), a circle with a
point on it. Its point counts once in `Curve::points`, it has no
`ends` (as a circle or a closed spline has none, so nothing joins it end
to end, Extend and tangents at an end don't take it), and the solver has
no radius equation for it, its one point setting the radius.
`SketchEdit::CloseArc` (`close.rs`, `Sketch::closable`: an open arc of
the sketch's own, no fillet) makes one: the end's other curves are made
from the start, the end goes with what's on it, and what no longer fits
(a tangent at the end) goes too. A closed arc trims as a circle does,
needing two cuts, and is an open arc after, its point gone unless kept.
`SketchEdit::CloseSpline` (`Sketch::spline_closable`: an open spline
of the sketch's own with enough points to close) runs a spline on from
its last point round to its first; where its ends meet (a coincident
ties them, or they're within a millionth of their distance from the
origin, as a drag's solve leaves them) the last is merged into the
first as an arc's end is (`merge_into`), its handle going to the first
if that has none. `SketchEdit::OpenSpline { spline, at }`
(`Sketch::spline_openable`) starts a closed one at its point `at`,
ending it at a new point there, tied to nothing. By control points
both find its knots anew (`control_knots`); both drop what no longer
fits.

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
(`snap.rs`), the length an extrude drag snaps to (`handle_snap::snap_step`:
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
found again whenever they move; `Handle { at, tip, end }` puts a handle
at a fit point, the tip an ordinary point, setting the derivative there
to `3 / h (tip - at)` (`h` the mean parameter span beside it), so a
handle a third of the chord long is the pull it'd have anyway. Its end
is an ordinary point too, mirroring the tip in the fit point, which the
solver holds (`Residual::Mirrored`, one per coordinate, left out where
all three are fixed; fixing a spline fixes its fit points and tips, not
the ends the mirror holds); new handles take both (`Sketch::new_handle`).
A handle read from before ends were points has `end` defaulted to
`Id::MISSING` (an id never given out); `Unchecked::check` gives each one
a new point first (`Sketch::add_handle_ends`), so old files read.
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
its handles' tips and ends; `Curve::ends` an open one's first and last.
A handle's length and angle can be dimensioned (its whole line's
length, from its end to its tip, as `Measure::Length` of its tip,
`Sketch::span` giving the line, or as two points' distance; an angle
naming the tip: `Sketch::handle` finds a handle by its tip,
`Sketch::direction` a line or a handle as what an angle is measured
along).

Splines are shaped by edits of their own (`spline.rs`):
`SketchEdit::Convert` (`Sketch::convert_spline`: to control points
exactly; to fit points through its places at its knots (or, where the
knots are those of a spline with a handle at every fit point, two per
point, at those fit points: `handled_params`, so converting round and
back doesn't double its points each time), straying a hair
between, with handles (`through_tips`) at an open one's ends giving the
tangents it had, or at every fit point, their tips fitted by least
squares to places along it (`Interpolation::nearest_tips`, rematched to
where the new spline comes nearest a few rounds), whichever keeps nearer
it: the first is exact for a spline without handles converted and back,
the second keeps a drawn one, whose handles' knots the first can't
follow, exactly when they're read back as handles' knots; an open one keeps its
end points), `AddHandles` (a handle at
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
draw, a curvature under `COMB_FLAT` (10⁻⁹) over the spline's size taken
as zero, so a straight spline shows no noise of rounding;
`handle_tips` where `handle_tip` would put a handle at each of places
not yet a spline, for the Spline tool; `flatten_spline` a spline of places not yet in a sketch, for the
Spline tool's preview.

Constraints (`constraint.rs`) name points and curves: coincident, point
on curve (a spline's too), horizontal and vertical (a line, or two
points), parallel, perpendicular (these four and horizontal and vertical
of one also take a spline's handle, named by its tip, `Role::LineOrHandle`,
`Sketch::direction`, as an angle dimension does), tangent, smooth, equal (lengths or
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
point to its tip), a radius or a diameter, an offset pair's offset, or
the gap from a circle's or an arc's edge (taken whole, as a circle) to
a point, a line or another circle's or arc's edge (`EdgeDistance(round,
other)`: its side outside or inside for a point, the side of a line the
centre is on, apart or the other inside for a circle; `Sketch::edge_ends`
where the gap runs, its anchor halfway; `fits` refuses a spline and the
round's own points; the solver's `Residual::EdgeGap`)
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
within the limit, an angle above zero and under a turn). So it takes a `Design { max, units, params }`, the document's from
`Document::design`. The document's editor runs it on every edit, so a
sketch in the document always passes it. A design's parameters
(`params`, see "Parameters" in `agents/features/parameters.md`) are what names in
dimensions read: `Measure::ask(design)` passes them on, so a dimension
`width` is checked to come to its stored value with `width` as it is, and
one naming no parameter, or one in error, fails the check.

**Dimensions and parameters.** A dimension's value may name parameters
(`width / 2`), typed in the value field as any value (whose names are
offered as they're typed, see Suggestions in `agents/features/parameters.md`).
When a parameter command changes what one comes to, the document solves
the sketch again then and there: `revalue(&sketch, &design, &budget)`
(`propose.rs`) evaluates each dimension naming a parameter again, moves
the driving ones that change from their old values to the new in steps
as a `SetDimension` does (all together, `continuation`), then settles
and checks the result; `None` where nothing changes, `Unrevalued::Value`
for a dimension in error, `Unrevalued::Rejected` for a sketch that won't
solve. `Document::with_params` runs it on every sketch within the
budget the command was applied with, in the same command as the
parameter change, so one undo step takes back both; a refusal refuses
the command (`EditError::Value`, `EditError::Unsolved`).
`Editor::apply` gives the solver's default iterations and no clock;
the app applies every command through `Editor::apply_within` with a
deadline of `varde_solve::PROPOSAL_TIME` on iced's clock
(`iced::time::Instant`, which the page has too), as it runs on the UI
thread: one running out is `EditError::Unsolved(id,
Rejected::Unsolved(Failure::OutOfTime))`, the command refused. A rename rewrites the dimensions' texts
(`varde_expr::rename`) without solving, the values unchanged. Sketch edits
waiting on the solver go first: parameter commands wait behind them
(`Change::Param`), so a proposal is solved with the parameters it was
made with.

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
whose `placement` maps sketch coordinates into the world: an origin
plane's, or for a sketch on a flat face of a body the one regenerating
finds for it (`agents/features/sketch-planes.md`; the app reads either through
`Doc::placement`).

**Links** (`link.rs`, `Link`, `Sketch::links`) are geometry a sketch
takes from outside it: projected square onto its plane (`LinkKind::Project`)
or where its plane cuts something (`LinkKind::Intersect`). A link's points
and curves are ordinary items in the sketch's lists, with ids from its
counter, so constraints, dimensions, snapping, hit testing and drawing
take them as any; the `Link` (its own id from the same counter, naming
no item, so `Sketch::kind` knows nothing of it) lists them in the order
its `LinkShape` has them, and says whether its curves count for profiles
(`profiles`; they're construction geometry where they don't, which
`check` holds). `SketchEdit::AddLink` (the Project and Intersect tools)
adds one counting for profiles; the sketch face starts construction. What a link comes from isn't the sketch's: the document
keeps it beside the sketch (`FeatureKind::Sketch::sources`, a
`LinkSource` per link, see `agents/features/sketch-links.md`). A `LinkShape` is the
geometry without ids (points, and curves naming them by placeholders
numbering them from 0); `Sketch::link_shape` gives what a link holds,
`same_form` says whether two have as many points and the same curves
made from them alike, `close_to` whether they're the same within a
distance, `fits` whether a lane's shape is one a sketch could take.
`SketchEdit::AddLink` adds an empty link, `SketchEdit::Relink` gives
links the shapes found for them (`Sketch::relink`), keeping what ids it
can (`Sketch::follow`): each curve found keeps the id of the one held of
its kind as many before it of that kind (the third line found the third
line held; a spline found with more points is the same spline
reshaped), each point that of the point in the same role on a curve so
kept (an end, a center, a spline's point by its place), the points left
those of the points held left, the nearest pairs first. What's kept is
moved in place, keeping what's on it; what's held and not kept is
deleted, with the constraints and dimensions on it; what's new is added.
The same form keeps every id. The link lists the ids kept first,
increasing, then the new ones in the order found, so its shape is the
one found reordered, and `Sketch::link_follows` (whether relinking would
change nothing, to a distance) tells a stale link rather than comparing
shapes in order. What names an id that went shows it: a revolve's axis,
a region, a path's curve fail their feature with why, another sketch's
link of it is broken ("it isn't in its sketch any more"), and the app
says the constraints and dimensions that went in the status bar
(`agents/features/sketch-links.md`, Following the model). `SketchEdit::SetLinkProfiles`
sets `profiles` and its curves' construction flag, and deleting a link's
id deletes it with what it made. Every other edit leaving a link's items
as they were is checked after it's applied (`Sketch::links_kept`):
moving, trimming, extending, filleting, converting a link's geometry or
making it construction or not is `EditError::Linked`, and so is deleting
a link's item but by deleting the link. The solver takes a link's points
and radii as constants whatever `Fixing` (as the origin's), with no
equation for a link arc's radii, so the analysis has them fixed and a
drag never moves them. `check` holds links in id order, at most
`MAX_LINKS`, each naming points and curves of its own (at most
`MAX_LINK_POINTS` and `MAX_LINK_CURVES`), in order, its curves made of
its points alone, no fillet, chamfer or spline with handles among them,
and no curve of the user's made from a link's point (a shape drawn
snapping to one gets a point of its own, coincident with it, as at the
origin; a trim or extend ending at one likewise, and Mirror gives a
link's point on the mirror line an image of its own, held symmetric,
rather than sharing it): `SketchError::Link`.

Links' geometry is made here from what regenerating finds
(`agents/features/sketch-links.md`): `Sketch::project_item` projects another sketch's
point or curve through the affine map from its plane into this one's,
exactly where it can be (a point, a line, what fillets and chamfers
leave of it (`cut_back_of`), or a point where the line is square to this
plane; a circle or an arc where the map keeps lengths, the
planes parallel, an arc reversed where the map reflects; a spline by its
control points mapped, with its knots, which an affine map keeps exact),
else by `LinkShape::fit` on places along it; `LinkShape::fit` tells what
exact places along a curve (`SampledChain`) make, within `exact` (the
resolution): a point, a line (between the two places farthest apart,
from the one nearer the chain's first place, so a circle seen edge on is
the line it covers), a circle or an arc (Kåsa's
least squares circle, counter-clockwise, its ends on it), else a spline
through fit points among the places, doubling them until it passes
within `fit` (the fit tolerance) of every place (each measured from
the nearest place on it, `Path::closest`), at most `MAX_SPLINE_POINTS`
(`FitError::TooComplex`).

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
