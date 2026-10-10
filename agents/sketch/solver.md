# The solver (`varde-sketch`, `solve/`)

Two pure functions, runnable anywhere: `solve(&Sketch, &Goal, &Budget)`
and `analyse(&Sketch)`. `notes/SketchImpl.md` ("Built so far (step 2a)",
"(step 7b)") has the details, and the benchmark under "Performance".

- **The system** (`system.rs`): the variables are the points' coordinates,
  circles' radii and a parameter per point on a spline, in `Slot`s; the
  origin and axes are constants, and so are a link's points and radii
  (with no equation of a link arc's own; see Links). What
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
  points and curves, the sized curves and the redundant (or, unsolved,
  conflicting) constraints. A line, circle or arc is sized
  (`Analysis::sized`) where the gradient of its size (a line's length,
  a radius) has no part in the null space, the motions the equations
  allow, nor on a variable no equation reads: the constraints set its
  size even where they leave it free to move.
