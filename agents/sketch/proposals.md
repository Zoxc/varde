# Edits and proposals (`varde-sketch`, `edit.rs`, `propose.rs`)

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
new driving dimensions are involved is `Rejected::Driving`. The app then
proposes an `Add` of dimensions alone again with them as references,
holding what they measure (`Doc::as_reference`), and says so in a toast.
A dimension placed with the Dimension tool is first proposed driving at
what it measures (`Doc::probe`), not committed: accepted, the value field
opens; refused as driving, it's placed as a reference with no value
typed. `auto` constraints are tried together, and
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
  id, numbered as an arc, and a closed arc likewise stays one. With no cuts there (a circle cut once, by a
  tangent), the whole curve is deleted as `Delete` would. A new end is the
  cutting curve's end where that's there (the two share it, as a T's stem
  and bar; a link's end gets a new point `Coincident` with it instead,
  as no curve of the user's is made of a link's point), else a new point with a `PointOnCurve` on each curve cutting
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
