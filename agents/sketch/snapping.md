# Snapping (`crates/view/src/snap.rs`)

`snap(sketch, tool, cursor, pixel)`, pure: where a drawing tool's next
click goes and why (`Snap { at, target, inference }`), from candidates
within `SNAP_TOLERANCE` (8) pixels of the cursor, the first kind found
winning, the nearest of it: a point of the sketch's or the origin (not,
with the Line tool, one at the line's start, which would make no line); a
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
the tangent arc; the Dimension tool doesn't snap. A spline being drawn
snaps to itself (`Target::Own`): to the points it has placed, before the
sketch's points (never its last, its first only where a click closes it,
`Drawing::closes`), and to its curve through them, as the preview
draws it short of the cursor, where that's nearer than the sketch's
curves. A point snapped to one it placed is a new point, `auto`
coincident with it; one on its curve is only placed there, untied, as a
point on its own curve is so whatever (`Constraint::own_point`). A rectangle's corners
(or centre) and a polygon's centre and corner snap as a shape's points,
with nothing inferred.

The viewport computes the snap as the cursor moves, unless `Held::FREE`
(`Shift`) is held (with the settings' Snapping off, only while it's
held: `Sketching::snaps`, `MouseOptions::sketch_snap`; a point dragged
alike, and the status bar's hint says "Snap" then rather than "Don't
snap"), and sends `Look::Snap` when it changes (on every move
along a curve, since the place moves), so the app has it for the glyph;
the click carries the snap it had. While the shape has fields it sends
`Look::Aim` instead, and with the cursor off the sketch sends nothing, so
the preview (drawn at the app's aim then), the glyph and `Enter` stay
where it was last aimed. The live layer draws the preview to the snapped
place, highlights what it snapped to, and draws the snap's
guide dashed in `SketchColors::guide` (`Snap::guide`: from the start on
past the place along a direction, or from a circle's centre to its
quadrant).
