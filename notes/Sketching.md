# Sketching

Goal: draw 2D profiles on a plane, pin them down with constraints and
dimensions, and have the geometry follow when a value changes. Sketches are
what later features (extrude, revolve, holes) are built from, so this is the
first step from "simple solids" to parametric modelling.

This note says what the user sees and does. How it's built comes after, in
its own section, once this part is agreed.

## What a sketch is

A sketch lives on a plane in the design and holds 2D geometry: points,
lines, circles, arcs and splines. Some of it can be marked as construction geometry,
drawn dashed: it helps place other geometry (a centre line, a bolt circle)
but never forms part of a profile.

Constraints say how the geometry relates (this line is horizontal, these two
circles are the same size); dimensions give it sizes (this line is 40 mm).
Together they describe the intent, and the geometry is solved to match. A
sketch is *fully constrained* when nothing is left to move, and
*under-constrained* while something still can.

Closed loops of geometry form *profiles*, the regions a later feature uses.

## Entering and leaving a sketch

- **New sketch.** `S`, or Sketch in the toolbar, then pick a plane: one of
  the origin planes (XY, XZ, YZ) to start with, a planar face of a body once
  faces can be picked. The view turns to look straight at the plane and
  zooms to fit it. The sketch appears in the Timeline as "Sketch 1", "Sketch
  2", ....
- **In the sketch.** The toolbar and the Alt bar switch to sketch tools. The
  3D model stays visible, as it was *before* this sketch in the Timeline,
  faded, so it can be referred to but not confused with the sketch. Left
  drag selects and moves geometry here, so orbiting uses the other ways
  (below); pan and zoom work as usual. `Home` (or the view cube) returns to
  looking straight at the sketch.
- **Side panel.** The Timeline tab gives way to a Sketch tab, split in
  two: the Geometry list above, the Constraints list below (see below), the
  split draggable. Objects stays as the other tab, and `Alt` peeks at it as
  it does outside, but for the Dimension tool, where `Alt` places reference
  dimensions (see When constraints disagree). Leaving the sketch brings the
  Timeline back.
- **Leaving.** `Esc` with no tool active, or the check by the sketch's name
  in the toolbar. Leaving never loses anything: every change was already
  committed as it was made.
- **Editing later.** Double-click the sketch in the Timeline, or select it
  and press `Enter`.
- An empty sketch is kept, as it is in the Timeline; delete it like any
  other feature.
- **Finished sketches** are seen along with the model, drawn as thin
  lines where they lie, hidden behind bodies in front of them. They're
  listed in the Objects tab with the bodies, each with its eye to hide and
  show it.

**Orbiting**, everywhere in the app, not only in sketches: hold the mouse
wheel and drag, or `Shift` and right drag. Left drag also orbits outside a
sketch, as today. Right drag pans, as today; the mouse wheel's drag moves
from panning to orbiting.

The status bar always says where the sketch stands: "Fully constrained ·
6 lines · 4 dimensions", or "4 degrees of freedom left", or what's wrong.

## Drawing

Every tool has a key, shows a hint in the status bar ("Click start point"),
and stays active after finishing a shape so the next can follow; `Esc` stops
the tool, a second `Esc` leaves the sketch.

| tool       | key | how |
|------------|-----|-----|
| Line       | `L` | Click, click, ... draws connected lines; clicking the first point closes the loop and ends the chain; `Esc` or a double-click ends an open one. |
| Rectangle  | `R` | Two opposite corners; a variant from the centre (`Q` switches). Comes with its horizontal and vertical constraints. |
| Circle     | `C` | Centre, then a point on the circle. |
| Arc        | `A` | Three points (start, end, a point on it); a variant from the centre. Starting at the end of a line offers a tangent arc. |
| Point      | `P` | A lone point, usually as a reference. |
| Polygon    | `G` | Centre, then a corner; the number of sides is typed. Comes with its equal-length constraints and a construction circle. |
| Spline     | `N` | Click points the curve passes through; double-click or `Enter` ends it, clicking the first point closes it; `Q` switches to control points. See Splines. |

- **Construction.** `X` toggles the selection, or the tool's next shapes,
  between normal and construction geometry.
- **Typed values.** While drawing, `Tab` moves between fields for length,
  angle or radius next to the cursor; typing a value (or an expression, see
  Dimensions) fixes it and adds it as a dimension. `Enter` places the shape.
- **Snapping.** The cursor snaps to endpoints, midpoints, centres, points on
  lines and circles, and the sketch origin and axes, and infers horizontal,
  vertical, parallel, perpendicular and tangent while drawing. A snap shows
  its glyph next to the cursor before the click, and is added as a real
  constraint when the click lands, deleted like any other if it wasn't
  wanted. `Shift` held disables snapping for that click.
- The sketch origin and its two axes are always there, fixed, to snap and
  constrain to.

## Selecting and changing geometry

- Click to select, `Ctrl`-click (`Cmd` on macOS) to add, drag a box to
  select what's inside (left to right) or what it touches (right to left).
  `Delete` removes the selection and the constraints and dimensions on it.
  `Space` clears the selection. That holds everywhere in the app, in a
  sketch or not.
- **Geometry list.** Lists the sketch's points, lines, circles, arcs and
  splines by name ("Line 3", "Arc 1"), construction geometry marked as
  such, each coloured by its constraint state. Selection is shared with the
  view: selecting in one selects in the other, and hovering a row
  highlights its geometry.
- **Dragging.** Drag a point, a line, a circle's edge to move it. The rest of
  the sketch follows as the constraints demand, moving as little as it can.
  Something the constraints fix doesn't move; a drag towards a place the
  constraints can't reach stops at the nearest place they can. The whole
  drag is one undo step, and `Esc` during a drag puts everything back.
- Dragging stays at frame rate on sketches of a few hundred entities, on the
  desktop and in the browser.

## Splines

A spline is a smooth free-form curve, for shapes lines and arcs can't make
well: a handle, a cam, a styled outline.

- **Two ways to draw one.** Through points (the default): the curve passes
  through every clicked point. By control points: the clicked points form a
  polygon, drawn dashed, that the curve is pulled towards without touching;
  better for a fair, even curve. The Spline tool switches between them
  while drawing (`Q`), and a finished spline can be switched too (`Q` with
  it selected), keeping its shape as closely as it can.
- **Shaping.** Drag its points to reshape it. Each end, and any through
  point, can show a handle: its direction sets the tangent there, its
  length how strongly the curve follows it. A selected spline shows dashed
  where its ends' handles would be; `Shift H` makes them handles (or
  gives the through points selected handles), and again takes them away.
  Points can be added on the curve (double-click it) and deleted from it;
  the curve stays smooth through the change.
- **Open or closed.** A closed spline makes a loop on its own and can be a
  profile by itself.
- **Constraints.** Its points take constraints like any other points
  (coincident, fix, horizontal, dimensions between them). Its ends take
  tangent to a line, arc or another spline, and *smooth*: tangent and
  matching curvature too, so the join shows no kink in reflections. A point
  can lie on a spline. A handle's angle and length can be dimensioned.
- **Curvature.** Selecting a spline can show its curvature comb (`U`), to
  see where it flattens or tightens and judge how fair it is.
- Trim, extend, offset, mirror, snapping and profiles treat splines like
  any other curve, but that a spline extends along the way it leaves its
  end, and is offset alone (not in a chain with other curves), as a spline
  through points along its exact offset whose points slide along it.

## Constraints

| constraint    | applies to |
|---------------|------------|
| Coincident    | two points, or a point and a line, circle or spline (point on curve) |
| Horizontal / Vertical | a line, or two points |
| Parallel / Perpendicular | two lines |
| Tangent       | a line, arc, circle or spline end and another curve |
| Smooth        | a spline end and a line, arc or spline: tangent with matching curvature |
| Equal         | two or more lines (length) or arcs and circles (radius) |
| Concentric    | two arcs or circles, or a point and one |
| Midpoint      | a point and a line |
| Symmetric     | two points (or two like shapes) about a line |
| Fix           | anything: pins it where it is |

- **Applying.** Select geometry, then pick the constraint from the toolbar,
  the Alt bar or its key. With the Constrain tool (`K`), selecting geometry
  lists only the constraints that fit it, most likely first.
- **Constraints list.** Lists the constraints and dimensions on the
  geometry selected, or every one in the sketch when no geometry is
  selected (selecting a constraint or dimension leaves the list as it
  is), each with what it ties together ("Tangent · Line 2, Arc 1",
  "Length 40 mm · Line 3"). Conflicts come first, in red. Hovering a row highlights its
  geometry, clicking selects it, `Delete` removes it, and a dimension's
  value can be edited in place.
- **Seeing them.** Each constraint is a small glyph next to its geometry.
  Hovering one highlights what it ties together; clicking selects it;
  `Delete` removes it. The glyphs can be hidden to see the drawing.
- Geometry is coloured by its state: under-constrained geometry in the
  sketch colour, fully constrained in a darker one, anything in a conflict
  in red. So the user can see what's still free without reading numbers.

## Dimensions

The Dimension tool (`D`) measures what is selected or clicked:

- one line: its length, or with the label off its ends, its horizontal or
  vertical extent (above or below the ends, left or right of them);
- two points (the same way), a point and a line, or two parallel lines:
  the distance;
- two lines at an angle: the angle, on the side the cursor is;
- a circle: its diameter; an arc: its radius (`Tab` switches either way
  while placing).

Clicking where the label goes places the dimension, showing its current
value, ready to overtype; `Enter` takes it, `Esc` leaves the dimension out.
While a value is typed, keys are the field's, not shortcuts. The label can
be dragged anywhere. Double-click a dimension, or its row in the
Constraints list, to change its value; the geometry follows at once.

**Units.** Each design has its units, millimetres by default, inches
offered: a new design starts in millimetres and the file menu sets its
units, then or later, as one undo step. Dimensions show in the design's
units; changing them changes how values are shown, not the geometry.
Angles are in degrees. Units chosen while an edit is still being checked
are set once it's done.

A length is at least 0.001 mm (a micrometre, well above what the solver
can tell from nothing anywhere in a design), an angle above 0° and under
360°. A distance between two points at the same place is refused (move
them apart first), as is a reference whose measure no dimension could
be, such as a distance past the size of the design, the status bar
saying why.

**Expressions.** Anywhere a value is typed, an expression works: `40 / 2`,
`1 in + 3 mm`, `(10 + 2.5) * 2`, `90 deg - 15 deg`, `0.5 rad`. Any
length unit (mm, cm, m, in, ft) and angle unit (deg, rad) can be mixed, and
a number without a unit is in the design's units. The dimension keeps the
expression as typed and shows its value; double-click shows the expression
again. Mixing lengths with angles, or anything that doesn't come out as the
kind of value asked for, is refused with the reason.

Values have to make sense: lengths above zero and within the size of the
design, angles within a turn. A value that would flip the shape inside out
(a distance through zero) is refused rather than solved into a surprise.

## When constraints disagree

The sketch in the document is always solved. An edit that can't be solved —
a constraint that contradicts others, a dimension that's redundant with
existing ones, a value the shape can't reach — is refused and nothing
changes:

- The status bar says why ("Would over-constrain the sketch") and the
  constraints and dimensions it conflicts with are highlighted in red until
  the next action.
- For a refused dimension, the hint says it can be added as a *reference*
  dimension instead, and how: placing it with `Alt` held, which in the
  Dimension tool places references rather than peeking at the other tab.
  A reference dimension is shown in brackets; it measures but doesn't
  drive, and updates as the sketch changes. Any dimension can be switched
  between driving and reference with `Shift D`, where the sketch allows: a
  reference made driving that would over-constrain the sketch stays one.
- Changing a dimension far from its current value may take the geometry
  through a different solution than the one intended (an arc flipping to the
  other side). The solver prefers the solution nearest to the current shape;
  if the result is still not what was meant, undo returns exactly to before.

A file whose sketch doesn't solve (made by a different version, say) still
opens: the sketch is marked as failed in the Timeline and the status bar,
its conflicting constraints are red, and any edit that fixes it is accepted.

## Changing shapes

| tool    | key | what it does |
|---------|-----|--------------|
| Trim    | `T` | Click a piece of a line or curve between intersections to remove it. |
| Extend  | `J` | Click a line, arc or spline near its end to lengthen it to the next thing it meets. |
| Offset  | `O` | Select a chain or loop, drag or type a distance: a parallel copy, tied to the original by that distance. |
| Mirror  | `W` | Select geometry (or click it with the tool, then `Enter`) and click a line: a mirrored copy, symmetric about it. |
| Fillet  | `F` | Click a corner where two lines meet, type a radius: a tangent arc rounds it. |
| Chamfer | `B` | Click a corner where two lines meet, type a distance (or two, or a distance and an angle): a line cuts it. |

Constraints on what's left are kept where they still mean something and
dropped where they don't; the result is solved like any other edit.

A fillet or chamfer is added to its corner rather than replacing it. The
corner's lines stay whole, drawn dashed from the corner to where the
rounding or cut meets them: still there to constrain and dimension to (the
overall 80 mm of a plate keeps measuring to the sharp corner), but not part
of a profile. The fillet or chamfer can be selected and deleted like any
other shape, and the sharp corner comes back.

## Profiles

Closed loops of non-construction geometry are shaded lightly, so the user
sees what will be extrudable before leaving the sketch:

- A loop inside another makes a hole in it, and its inside is a region of
  its own (a plate with bolt holes is the plate, with a hole per bolt, and
  each hole's inside: five profiles). An island inside a hole is one too.
- Crossing loops split into separate regions, each pickable on its own.
- Hovering a region highlights it.
- Open chains and gaps are left unshaded; the ends of a chain that almost
  closes are marked so the gap can be found.
- The status bar counts them: "2 profiles".

Picking a profile belongs to the feature that uses it (Extrude and friends,
planned separately); a sketch only shows them.

## Undo, history and files

- Every action — a shape drawn, a constraint or dimension added, a value
  changed, a drag — is one undo step, and undo/redo work inside a sketch as
  anywhere else.
- Sketches are saved in the design file with everything else, and
  auto-saved and recovered the same way.
- Both builds, desktop and browser, get the same sketching.

## Later, not in this plan

- Ellipses.
- Projecting edges of the model into the sketch, and constraining to them.
- Named parameters in expressions (`width / 2`).
- Text, images as tracing references, DXF import and export.
- 3D sketches.

## Order of delivery

Each step is usable on its own:

1. **Sketch mode.** New sketch on an origin plane, lines, circles, arcs and
   points, construction toggle, select, delete, drag free geometry, the
   Geometry list, `Space` to clear the selection, undo,
   saved in the file, listed in the Timeline.
2. **Constraints.** The constraint set above, glyphs, colouring by state,
   the status bar count, the Constraints list, dragging that respects constraints, refusal of
   conflicting constraints.
3. **Dimensions.** The Dimension tool, editing values, reference dimensions,
   design units, expressions.
4. **Drawing aids.** Snapping with automatic constraints, typed values while
   drawing, rectangle and polygon.
5. **Profiles.** Shaded regions, holes, the profile count.
6. **Changing shapes.** Trim, extend, offset, mirror, fillet, chamfer.
7. **Splines.** Through points and control points, handles, smooth
   joins, curvature comb, and the shape tools and profiles working on them.
8. **Sketch on a face**, once faces can be picked in the viewport.
