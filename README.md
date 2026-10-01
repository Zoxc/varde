# Varde CAD

A CAD application written in Rust, on [iced](https://iced.rs) and
[wgpu](https://wgpu.rs). It runs as a desktop app and in the browser.

Varde is early work in progress. Today it draws lines, rectangles, circles,
arcs, polygons, splines and points in sketches on the origin planes, held in
place by constraints and sized by dimensions, and trims, extends, offsets and
mirrors them. It extrudes a sketch's regions into solids with exact curved
faces, as new bodies or joined to, cut from or intersected with the bodies
already there, and opens, edits and saves the designs, shown in a 3D
viewport. The file format may change between versions without a way to read
older files.

To build and run it yourself, see [BUILDING.md](BUILDING.md).

## Using it

The app opens on a welcome screen: start a new design (`N`), open one (`O`),
reopen a recent file, or recover unsaved changes left by a crash.

| action                      | input                                      |
|-----------------------------|--------------------------------------------|
| orbit                       | middle drag, `Shift` right drag, or left drag outside a sketch |
| orbit about a point         | middle click on the model, or off it on the grid (in a sketch, its plane), which pans it to the middle of the view; its marker shows for 2 seconds, and while the pointer is over the view cube; a middle click on neither, or Home, orbits the view's centre again |
| pan                         | right drag                                 |
| zoom                        | mouse wheel, towards the pointer           |
| look from a side, or Home   | click a face of the view cube, or the house under it |
| perspective or orthographic | the `…` at the status bar's right, which also turns the mouse's hints off or on |
| peek at the other side tab  | hold `Alt` (`Option` on macOS)             |
| tool rail                   | the cards at the view's left hold the tools in sets; point at a card's top (or click it, or press its key: `Q`, `W`, `E`, `R` from the top) to list its set, then click a tool, press the letter beside it, or move with `↑` `↓` and press `Enter`; `Esc` closes the list. The cards show as many of their tools as fit |
| new sketch                  | `S`, then pick the XY, XZ or YZ plane      |
| edit a sketch               | double-click it in the Timeline, or select it and press `Enter` |
| delete a feature            | select it in the Timeline and press `Delete`; if other features use it, a prompt lists everything that goes with it first (`Esc` cancels) |
| delete a body               | the bin by it in Objects, which deletes the extrude making it too |
| extrude                     | `E` (with a sketch selected in the Timeline, its regions), click the regions to extrude (again to take one out), then drag the arrow's knob or type the distance in the panel; `Enter` or OK adds it, `Esc` cancels |
| join, cut, intersect        | in the extrude panel, under Operation; it works on every body it touches, listed under Bodies, where unticking one leaves it alone (a join touching several bodies merges them into the first made, and the panel says so: "Joined into Body 1"; Objects still lists the others, marked "in Body 1", and they show and hide with it); a cut or intersect that would leave nothing of a body fails, and unticking that body gets past it; only a cut can go Through all |
| edit an extrude             | double-click it in the Timeline, or select it and press `Enter`; an extrude that fails shows red there, and hovering it says why |
| draw in a sketch            | `L` line, `B` rectangle, `C` circle, `A` arc, `G` polygon, `N` spline, `P` point, then click; `Esc` stops; clicks snap (see below), hold `Shift` not to; `Tab` to type sizes (see below) |
| line tool                   | click point after point; click the first point to close the loop, `Esc` or double-click to end; clicking a point already there joins it |
| rectangle                   | two opposite corners, or with `Z` the centre and a corner (`Z` again goes back); it comes held level and upright |
| circle / arc                | centre, then a point on it / start, end, then a point on it |
| polygon                     | centre, then a corner; six sides until you type how many (`Tab`); its sides are held equal, its corners on a construction circle |
| spline                      | click the points it passes through; double-click or `Enter` ends it, clicking its first point closes it; `Z` switches to placing control points, which it's pulled towards (see Splines below) |
| select in a sketch          | click, `Ctrl` click to add (`Cmd` on macOS), drag a box: left to right for what's inside, right to left for what it touches |
| clear the selection         | `Space`                                    |
| move geometry               | drag it: a point, a line or a spline moves, a circle or arc edge changes its radius, and the rest follows as the constraints demand; `Esc` puts it back |
| delete / construction       | `Delete` / `X` on the selection (`X` with a tool: its next shapes) |
| constrain the selection     | its key (below), or `K` for the Constrain tool, which lists what fits |
| dimension                   | `D`, then click what to measure and click where the label goes; type the value, `Enter` (see below) |
| trim / extend / offset / mirror / fillet / chamfer | `T` / `J` / `O` / `Shift M` / `F` / `Shift B` (see Changing shapes below) |
| leave a sketch              | `Esc`, or the check by the sketch's name |
| save / save as              | `Ctrl S` / `Ctrl Shift S` (`Cmd` on macOS) |
| close the file menu         | `Esc`                                      |
| design units                | the file menu: millimetres or inches       |
| design tolerance            | the file menu: 0.1 µm, 1 µm (the default) or 10 µm, how closely curved shapes are fitted |

### Snapping

While drawing, a click snaps to what's near the cursor, and a glyph by the
cursor shows what before you click: the ends, midpoints and centres of the
geometry, a circle's top, bottom, left and right, anywhere on a line,
circle or arc, and the sketch's origin and its two axes. Drawing a line, it
also snaps to running horizontal or vertical, perpendicular or parallel to
a line it starts from, tangent to an arc it starts from, or touching a
circle or arc; a dashed guide shows the direction. An arc started at the
end of a line or arc snaps to running on from it, tangent.

What a click snaps to becomes constraints, as if you'd added them: a line
drawn from a point already there joins it, a point snapped onto a line is
on it, a line snapped horizontal is horizontal. Snaps that only restate
what's already so are left out. Delete any you didn't want from the
Constraints list like any other. Hold `Shift` as you click to place the
point exactly where the cursor is, snapping to nothing.

The origin and the X and Y axes are always there, drawn in grey, and can be
selected, constrained to (a point coincident with the origin, a line
parallel to an axis) and dimensioned from, but not moved or deleted.

### Splines

A spline is a smooth curve for shapes lines and arcs can't make well. The
Spline tool (`N`) places a point per click, snapping as other tools do, and
shows the curve through them to the cursor; a double-click or `Enter` ends
it, clicking its first point again closes it into a loop (a profile of its
own), and `Esc` starts it afresh. It draws through the points clicked (fit
points) until `Z` switches it to control points, which the curve is pulled
towards without touching, but at its ends: a fairer curve, four points at
least. `Z` switches back, and the tool keeps the kind for the next spline.

- **Shaping.** Drag any of its points. Select a spline and dashed handles
  at its ends show which way it leaves them; `Shift H` makes them real
  handles (with fit points selected instead, handles at those), and again
  takes them away. A handle's direction sets the curve's tangent there and
  its length how strongly the curve follows it: drag its tip, or delete
  the tip to take the handle away. With the Dimension tool, click its tip
  for its angle from the X axis, or its point and its tip for its length.
- **Points.** Double-click a spline to add a point where you clicked: a
  fit point there, or by control points one more control point, the curve
  just as it was. `Delete` removes points; the curve stays smooth through
  the rest, and a spline with too few left goes.
- **Converting.** With splines selected, `Z` switches them between fit
  points and control points, keeping the shape: exactly to control points,
  through the same places back, straying a little between.
- **Control points** of a selected spline show as a dashed polygon.
- **Curvature.** `U` shows the curvature comb of the splines selected, and
  hides it: a tooth at each place along the curve, longer where it turns
  tighter, on the outside of the turn, so a fair curve's comb changes
  smoothly. It keeps its size on the screen as you zoom.
- A spline's end can be tangent or smooth (`Shift S`: tangent and curving
  alike) to a line, arc or another spline, and a point can lie on one
  (Coincident).

### Typed sizes

Once a shape has its first point, fields next to the cursor show its sizes
as you move: a line's length and angle (counter-clockwise from the X axis),
a rectangle's width and height, a circle's diameter, an arc's radius once
its ends are placed, a polygon's number of sides and diameter. `Tab` puts
the cursor in the first field, and again moves to the next. Type a value,
an expression as for dimensions (see below), and it's held: moving the
cursor only moves what isn't typed, such as which way a line of the length
typed runs. `Enter`, or a click, places the shape, with each value typed as
a dimension of it, in one step to undo. A value that isn't one is refused,
the field saying why; emptied, a field lets go of its value. While you
type, the keys are the field's, not shortcuts; `Esc` closes the field,
keeping what's typed, and `Esc` again starts the shape afresh. A polygon
has from 3 to 64 sides, and keeps the number for the next.

### Constraints

Constraints say how a sketch's geometry relates, and the geometry is solved
to keep them. Select the geometry, then press the constraint's key, or take
up the Constrain tool (`K`), which shows only the constraints that fit what's
selected, the most likely first.

| constraint    | key       | applies to |
|---------------|-----------|------------|
| Coincident    | `I`       | two or more points, or a point and a line, circle, arc or spline (point on it) |
| Horizontal    | `H`       | lines, or two points |
| Vertical      | `V`       | lines, or two points |
| Parallel      | `Shift P` | two or more lines |
| Perpendicular | `Shift R` | two lines |
| Tangent       | `Shift T` | a line, circle or arc and a circle or arc, or a spline's end and a line, circle, arc or spline |
| Smooth        | `Shift S` | a spline's end and a line, circle, arc or spline: tangent, and curving alike |
| Equal         | `Shift E` | two or more lines, or two or more circles and arcs |
| Concentric    | `Shift C` | circles and arcs, or a point and a circle or arc |
| Midpoint      | `M`       | a point and a line |
| Symmetric     | `Y`       | two points and the line they mirror about |
| Fix           | `Shift F` | anything: pins it where it is |

- Each constraint shows as a small glyph beside its geometry: hover it to see
  what it ties together, click it to select it, `Delete` removes it. The eye
  at the top of the Constraints list hides and shows the glyphs.
- The Constraints list, in the lower half of the Sketch tab, lists the
  constraints on the geometry selected, or all of them when no geometry is
  selected. Hovering a row highlights its geometry, clicking selects it,
  and `Ctrl` click adds it to the selection, as in the Geometry list.
- Geometry the constraints leave free is drawn in the sketch colour, what
  they fix in a darker one (fixed points filled), and anything in a conflict
  in red. The status bar says "Fully constrained", or how many degrees of
  freedom are left.
- An edit that the constraints can't allow, such as a constraint that
  contradicts or restates others, is refused: nothing changes, the status bar
  says why and the constraints it ran into are red until your next action.
  Edits are checked as you make them; while one is being checked it shows
  faded, and undo drops it.
- A sketch that doesn't solve, say from a file made by another version, is
  marked in the Timeline and the status bar, its conflicts in red, and an
  edit that mends it is accepted.

### Dimensions

Dimensions give the sketch its sizes: the geometry is solved to match them.
Take up the Dimension tool (`D`), click what to measure, then click where
its label goes:

| click                    | measures |
|--------------------------|----------|
| a line                   | its length; with the label off its ends, above or below them its horizontal extent, left or right of them its vertical one |
| two points               | the distance between them, or the same way their horizontal or vertical distance |
| a point and a line       | the point's distance from the line |
| two lines                | the distance between them if they're parallel, else the angle between them on the side the label is |
| a circle or an arc       | a circle's diameter, an arc's radius; `Tab` switches to the other before placing |

Anything selected when you take up the tool is where it starts. Placing the
label shows the current value, selected: type over it and press `Enter`, or
`Esc` to leave it. While you type, the keys are the field's, not shortcuts.

- **Values** are expressions: `40 / 2`, `1 in + 3 mm`, `(10 + 2.5) * 2`,
  `90 deg - 15`. Lengths take `mm`, `cm`, `m`, `in` and `ft`, angles `deg`
  and `rad`; a number without a unit is in the design's units, or degrees
  for an angle. A length is at least 0.001 mm, an angle above 0° and under
  360°. What isn't a value of the right kind is refused, the field saying
  why with the part it's about selected.
- **Changing a value.** Double-click a dimension's label, or its row in the
  Constraints list: the field shows the expression as you typed it. The
  geometry follows at once; a big change is made in steps, so the shape
  stays as it was drawn, and undo returns to before.
- **Moving a label.** Drag it; the dimension line follows.
- **Reference dimensions** measure without driving: shown in brackets and
  in the construction colour, they follow the sketch. Hold `Alt` (`Option`
  on macOS) as you click to place one; in the Dimension tool, `Alt` places
  references rather than peeking at the other tab. `Shift D` turns the
  dimensions selected between driving and reference. A driving dimension
  that would over-constrain the sketch is refused, and the status bar says
  how to add it as a reference instead.
- The Constraints list shows dimensions with the constraints, as "Length
  40 mm · Line 3". Clicking a label selects it, `Delete` removes it.
- **Units.** A new design is in millimetres. The file menu sets its units,
  millimetres or inches; dimensions show in them, and changing them changes
  how values show, not the geometry. It's one step to undo.

### Changing shapes

- **Trim** (`T`): click a piece of a line, circle, arc or spline between
  the curves crossing it, and it's taken away; the piece that would go
  shows red as you hover. A curve nothing crosses goes whole, a circle
  trimmed becomes an arc and a closed spline an open one. A spline cut in
  two becomes two splines keeping its shape, exactly by control points,
  closely through fit points, each new end with a handle. Construction
  curves cut too.
- **Extend** (`J`): click a line, an arc or a spline near the end to
  lengthen: it runs on to the next curve it meets, an arc round its
  circle, a spline along the way it leaves its end, curving on into it
  through its old end, shown as you hover. With nothing ahead, the status
  bar says so.
- **Offset** (`O`): click a line, arc or circle, and the chain it's in is
  picked: the curves joined to it end to end, a whole loop if they close
  (or select the chain first). A spline is offset alone, not in a chain
  with other curves: its copy is a spline through points along the exact
  offset, each held as far from it by the Offset dimension, which it
  follows within a ten-thousandth of its size; they can slide along the
  offset. Offsetting a spline further than it curves on that side would
  fold the copy, and is refused. Then click where the copy goes, or drag it
  there: it passes the cursor, on the cursor's side, shown as you move. Or
  press `Tab`, type the distance and `Enter` (or click, for the side). Each line is copied
  parallel, each arc about the same centre; where the copies part at a
  corner, lines run on to meet and arcs are joined by a round corner, and
  whatever the distance takes past a narrow part of the shape is left out.
  The copy is held at that distance by one Offset dimension, which you can
  change later: the whole copy follows, each other piece held as far off
  as the first by an Offset constraint. An open chain's copy can slide
  along at its two ends. Offsetting a loop inwards past its middle leaves
  nothing, and the status bar says so.
- **Mirror** (`Shift M`): select what to mirror first, or click it with the tool
  (a second click takes it out) and press `Enter`; then click the line to
  mirror about, a sketch line or an axis. The copies are held symmetric to
  the originals, so they follow them; points on the line are shared. A
  fillet or chamfer mirrored with its lines is one on the copies' corner.
- **Fillet** (`F`): click a corner where two lines meet, then click how far
  in the rounding goes (or drag there): an arc tangent to both lines, its
  middle where you let go, shown as you move. Or press `Tab`, type the
  radius and `Enter`. It's held by a radius dimension you can change
  later.
- **Chamfer** (`Shift B`): click a corner where two lines meet, then click how
  far in the cut goes: a line across it, as far back along both lines.
  Or press `Tab` and type the distance along the line nearer where you
  clicked the corner, then, if the cut isn't as far along both, `Tab` to
  the distance along the other or to its angle to the first, and `Enter`.
  It's held by dimensions of those distances, or the distance and the
  angle.

A fillet or chamfer is added to its corner rather than replacing it: the
corner's lines stay whole, drawn dashed from the corner to where the
rounding or cut meets them, still there to constrain and dimension to
(the overall length of a plate keeps measuring to the sharp corner), but
not part of a profile. It shows as "Fillet 1" or "Chamfer 1" in the
Geometry list; delete it and the sharp corner comes back, delete a line
and its fillets and chamfers go with it. Another on the same corner
replaces it. Trimming or extending a line away from its corner takes the
corner's fillet or chamfer away; trimming a fillet or chamfer deletes it.
Offset copies a corner's lines sharp, without their fillets or chamfers.

The new ends are held on the curves that cut them. Constraints and
dimensions stay where they still mean something: a trimmed line keeps its
horizontal, but not its length. Each change is one step to undo.

### Profiles

Closed loops of geometry that isn't construction are shaded lightly: the
profiles later features will use. Every enclosed area is a profile of its
own: a plate with bolt holes is the plate, with the holes cut out, and each
hole's inside. Crossing loops split into separate profiles. Hovering one
(with no tool, away from the geometry) highlights it. The status bar counts
them, "3 profiles"; a sketch whose curves cross too often to work them out
says so instead, and isn't shaded. Open ends that almost meet, a gap of a
few pixels at the zoom you're at, are marked with a red ring, so a loop
that doesn't quite close can be found: zoom in to see the gap.

Designs are saved as `.vrdp` files. Unsaved changes are auto-saved a few
seconds after you stop editing, to a hidden `.<name>.vrdp.autosave` file next
to the design, and offered back if the app crashes. A design that is already
open in another window opens read-only.

In the browser, Chromium-based browsers open and save your files in place.
Firefox and Safari open a copy and save by downloading. Auto-saves are kept in
the browser's site storage, and there is no recent files list yet.

## License

Varde is licensed under the GNU Affero General Public License, version 3 or
(at your option) any later version. See [LICENSE.txt](LICENSE.txt).
