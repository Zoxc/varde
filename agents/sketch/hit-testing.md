# Hit testing (`crates/view/src/hit.rs`)

Pure functions, tested headless:

- `Projector::cursor` intersects the cursor's ray with the sketch plane:
  `None` when the ray runs along the plane (the camera's axes are `f32`, so
  within `PARALLEL` of it), meets it behind the eye in perspective, or past
  `MAX_COORD`. It also gives a pixel's size there, in sketch units, which
  turns the pixel tolerance (`HIT_TOLERANCE`, 6 pixels) into sketch units
  and is the size under which the tools refuse a shape.
- `hit` finds the nearest point within the tolerance, the origin
  included, and failing one the nearest handle end within twice the
  tolerance over a handle's arm (`END_REACH`; its tip or end, points,
  `handle_ends`, so nearing an end along its arm, or another handle's
  arm across it, the end is hit), failing one the nearest spline handle as drawn, both
  arms, as a line of its own (`Selectable::HandleLine(tip)`, before the curves, so
  where a spline runs along its handle by the fit point the handle is
  hit), failing one the nearest curve as drawn (lines and circles
  exactly, arcs and splines by their polyline), and failing one the
  nearest axis. What the view hovers and selects is a
  `varde_sketch::Selectable` (`crates/sketch/src/selectable.rs`, next to
  the naming, so `Sketch::selectable_name` and `Sketch::selectable` know
  it): `Item(id)` (built-ins too; ordered first, so items sort as their
  ids) or `HandleLine(tip)`. It's never stored: the
  hit results, the selection, the hover, the overlaps list, the tools'
  picks (`Drawing::picked`) and the `Look`/`Edit` messages carrying
  them (`ClickGeometry`, `SelectBox`, `ClickRow`, `HoverItem`,
  `LeaveItem`, `DragGeometry`, `DeleteItem`, `RowMenu::Item`) take it,
  and only ids reach `SketchEdit`s: a handle by its tip
  (`Selectable::id`), as what's made of a handle names it.
  `selectable_name` calls it "Handle of Spline 1". Ids from `LAST_ID` (2³¹ − 3)
  up are still never given out (`OutOfIds`, `NextIdReserved` past it),
  where they ended when handles took ids of their own, so the sketches
  `check` accepts stay the same (`LAST_ID` itself is `Id::MISSING`).
  Deleting a handle, or its tip or end, deletes both; dragging its line
  turns it about its fit point, its length kept, its end mirroring its
  tip (`handle_dragged`); dragging its tip or end moves the other the
  other way; the Dimension tool picks it as a line (`pickable`), alone
  its length as a line's, with a line or another handle the angle
  between them.
  With Trim or Extend, or Offset picking its chain, the viewport hits
  curves alone (`hit_curve`), so a click by a line's end is the line, and
  Mirror choosing its line lines and axes alone (`hit_line`), Fillet and
  Chamfer picking a corner the points where lines make one, within 12
  pixels (`hit_corner`); Offset, Fillet and Chamfer placing hit nothing.
  Pressing the origin or an axis without a tool selects it but never
  drags it.
- `overlaps` finds everything within the tolerance, as `hit` would
  each: points, the origin included, then handles, then curves, then
  axes, each
  nearest first. A press held still lists them.
- `in_box` projects points and flattened curves and tests them against the box on the screen: wholly
  inside, or touching (any segment meeting it).
  What's behind the eye is never inside.
