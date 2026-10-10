# In the viewport (`crates/view/src/viewport/sketch.rs`)

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
ends through their fit points to their tips, in
`SketchColors::spline_handle` (their tips' and ends' rims too), in the
selection's with their spline or as a line; fit points without a handle show
none, selected or not; a spline by control points shows its control
polygon dashed in the construction colour while it or any of its control
points is selected.
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
redundant constraints and a refusal's, with what they tie together),
failing red too within the errors' halo (`failing`, below), and
faded while waiting on the solver. A dimension (its lines, arrows and
label) is free in the Dimension tools' icon colour
(`SketchColors::dimension`, `IconColors::dimension`'s line), or in the
driven colour if it's a reference (`SketchColors::driven`, as its
label's pill; `GlyphLook::dimension_color`),
and selected, in a conflict or waiting as the others. Colours are the palette's
`SketchColors`, light and dark; `conflict` is the theme's error red,
the renderer's `Colors::error`.

**Failing curves.** While a sketch is edited, the curves that the
failures of the features using it name (their `ErrorGeometry`'s
`sketch_curves`, as the model shown found them) are marked:
`SketchSession::failing`, picked again with the failures shown
(`Doc::refresh_errors`, `Doc::failing_curves` in `app/src/doc/errors.rs`)
and handed over as `SketchState::failing`. Every failed feature's,
always while its sketch is edited, not only on hover or selection as in
the model: the sketch is where it's mended, and a failure names few
curves. They are a profile's segments, or a revolve's axis line of no
length (regen's own failure, with the line's point). No draft fails meanwhile, as editing a sketch ends the operation
set up. Only curves the sketch as shown holds count: one deleted since
isn't found (the numbers are `Id::get`s, matched against its curves),
and the view keeps only ids it holds as curves. They're drawn in the
`conflict` red, and their halo is the failures' own: the base layer
keeps them flattened and placed in the world (`Base::failing`, a new
`Arc` with each base layer, so uploaded again only then), and the frame
adds them to `Frame::errors` (`SketchFrame::failing`) as their halo
alone (`ErrorParts::halo_only`), under the sketch's curve drawn red at
its own width after it; no error core under it, which would thicken it
and show around a selected one's blue. The model's copy of such a
failure, in the sketch's plane, is drawn without its curves meanwhile,
its points (where the profile touches itself, an open gap's ends, an
axis line of no length's place) still shown, for every failure marked (see `agents/viewport.md`, "Which
failures show").

- The **base** layer, the sketch and its selection (under it the regions
  of its profiles, each filled on its own in `SketchColors::region`, since
  a hole's inside is a region too and one even-odd fill of them all would
  leave it out; the origin's axes, as far as a sketch reaches, each two
  axis polylines (fading out pointing at the camera, as the grid's axis
  lines do) out from the origin (a segment with both ends that far away
  is cut to the near plane and viewport too inexactly, and lands pixels
  off the grid's axis line), and the origin, fixed, in `SketchColors::axis`, or selected or in a conflict),
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
  dimensions' arrowheads, a ring in `SketchColors::near_miss`
  (the Timeline's failed text's, `danger_strong`), `NEAR_MISS_RADIUS` (7)
  pixels out, between each pair of open ends within `NEAR_MISS_GAP` (6) pixels at the
  target (`Projector::pixel`), paired again only when the profiles or the
  zoom change (`Input::near`), and the tool's preview (the rubber-band line,
  the circle or arc through the cursor where it snaps, with what it snaps
  to highlighted, where it snaps, whatever to (`Snap::snapped`: a point,
  a curve, a direction inferred; a grid would be the exception), marked
  by a rimless disc of the points' colour at `SNAP_WASH` (15 %),
  `SNAP_RADIUS` (15) pixels out so it shows round the cursor, as is where
  a point dragged snaps, and the snap's guide, the points placed; the Dimension
  tool's picks and the dimension it would place with the label at the
  cursor, in the preview colour, or the driven colour while the
  reference modifier (`Held::REFERENCE`) is held, as a click then places
  a reference; or the one the value field places).

**Input.** `Input` keeps the cursor, the item under it (hover isn't app
state: only the viewport shows it), the left button held down and whether
it has moved past `DRAG_DISTANCE`, the last press of a tool for telling
a double-click, the last click without one (`last_select`: a second on a
spline in an editable sketch, soon and near, sends
`Edit::InsertSplinePoint` at the place pressed rather than a click), and
whether the button's held with a tool that clicks
where it's let go (`releasing`: Offset, Fillet or Chamfer placing); the
modifiers are the widget's. With Project or Intersect in use the left
button isn't the sketch's: it orbits when dragged and clicks the model
or another sketch as outside a sketch (`Program::picks_outside`), and
no overlaps are listed. A press on an item of an editable sketch drags
it once it moves; elsewhere, or read-only, it drags a box. Held still for
`HOLD_DELAY` over more than one item (`Press::lists_at`,
`Sketching::hold`), it ends and lists them (see "Overlaps" in
`agents/viewport.md`). `Esc` lets go
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
design's units coloured by state (a reference's measure, which a
driven size's, after an equals sign in a green pill as the Geometry
list notes one, outlined and coloured as a driving one's chip while
selected or in a conflict),
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
16 px square on a chip (`theme::glyph`, outlined red in a conflict and in the selection
colour when selected, `theme::selected_glyph`, as is a dimension's label; the
icon in the Constraint category's colours while free, as the rail
draws it, in the selection colour when selected, faded while waiting), a `mouse_area`
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
