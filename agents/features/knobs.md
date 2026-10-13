# Operation knobs

The operations set up in the move's session that have a value to drag
(offset face, shell, draft, chamfer, fillet, scale, align, patterns, sweep) have a
handle of knobs, as the extrude's and revolve's are drawn
(`agents/viewport.md`): `MotionState::knobs`, `OpKnob`
(`view/src/motion/knobs.rs`), worked out by the app
(`app/src/doc/motion/knobs.rs`, `Doc::motion_knobs`) and drawn and
dragged by the viewport (`view/src/viewport/knobs.rs`). A knob names
the field it types into, its path (a line through a point along a
direction, or round an axis through a centre from a radial, its radius
in millimetres or pixels), its value in the field's own units
(millimetres, radians, a factor; an offset face's signed by its side),
how a value maps onto the path (times a factor, or a slider so many
pixels a unit), what it snaps to (lengths as the extrude's handle,
angles as a move's ring, factors 1, 2 or 5 × 10ⁿ, each at least 6 px
along the path, counts whole), its arrow's way (the way the value
grows), where its shaft starts if it has one (a value, zero but for a
pattern's count's, from the original), and its colours (Create's,
Modify's, or a count's teal beside another knob: a pattern's count, a
sweep's twist or turns). Dragged, the viewport sends `MotionLook::DragKnob { field,
snap, value, step }`, the knob's field and snap with the value snapped; the
app (`Doc::drag_knob`), without rebuilding the knobs, checks the field is
one the session's knobs could have and types the value into it as the design's units (or degrees, or a bare factor) write
it, where the field takes it: none at or below zero but an offset
face's (its sign its side, zero nothing), an align's and a sweep's
twist, no draft of a quarter turn or more. None in a document that can't be changed.

- **Offset face**: along the first face's outward normal (its
  `Anchor`, above), at the distance.
- **Shell**: from the first face removed into the body (out of it for
  Outward walls), at the thickness.
- **Draft**: on the first face, turning about its hinge, where it meets
  the neutral plane along the face's way up towards the pull, by the
  angle, so the face's top goes into the body and its normal leans
  towards the pull; none for a face square to the pull or whose point
  is on its hinge.
- **Chamfer**: from the first edge along the bisector of its faces for
  Equal (half the bisector's sum a millimetre of distance, so the knob
  is on the chamfer's middle); along each face for Two distances (the
  first face as Flip sides takes it); along the first face for
  Distance and angle.
- **Fillet**: along the bisector, on the round's middle, `r (1 / sin(φ
  / 2) − 1)` from the edge for faces at an angle `φ`.
- **Scale**: a slider from its point, 100 px a factor of 1: towards the
  bodies' box centre for Uniform, along each world axis towards it for
  Per axis; none for Edge length.
- **Align**: along the target's primary direction from its point at the
  offset, and on a ring 60 px out about it there at the turn, from the
  target's second direction.
- **Linear pattern** ("rail above"): the spacing's on the axis through
  the bodies' box's middle from the original's start (the box's end the
  copies go away from, there whether the box holds the copies or not)
  to the first copy's, the total's to the last's in Total; the count's,
  teal, on a rail above the copies (a quarter of the box's diagonal
  above its top, square to the axis towards +Z), a step a copy, at the
  last copy, its shaft from the original. A count dragged is whole and
  two or more.
- **Circular pattern** ("running on"): the span's on the arc through
  the copies (the bodies' box centre turned about the axis) at the last
  copy, the step's in Spacing (a step a copy less one), none for Full
  360°; the count's, teal, on a slider running on along the arc's
  tangent past its end, 8 px a copy. The original is found on the
  document's model (`MeshFeed::committed`), so only a new pattern has
  knobs.
- **Sweep**: where regenerating its draft found them
  (`Drafted::sweep`, `SweepFound`, worked out from the path as built,
  before the kernel is asked, so there while the kernel's sweep fails).
  Along a path ("end ring"): the twist's, teal, on a ring 60 px out
  about the path's end, right-handed about its tangent there, from where
  no twist leaves the profile's x (carried by rotation-minimizing frames,
  double reflection over the pieces' points, 16 a conic; for Keep
  orientation the x taken square to the end's tangent, the y where x
  runs along it), the twist any way round or none; none along a closed
  path. Along a helix ("climb"): the pitch's on the axis from the
  foot of the profile's box's middle, as climbed (flipped with Flip);
  the turns', teal, on a rail up from the profile's middle, a pitch a
  turn, so at the helix's end, snapping as a factor.

A shell's, draft's, chamfer's and fillet's knobs stand where their
first face or edge is before the feature changes it (`KnobAnchor`):
found on the model shown while it's the document's alone (no draft
shown, of the document as it is), so for a new feature only, kept
while the first face or edge and the document stay; the point and
normal of `PickIndex::face_point`, an edge's point taken onto both its
faces. An edited one has no knob (the model shown has it changed).
