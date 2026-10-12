# Move and mirror

`crates/document/src/motion.rs`.

```rust
pub struct Move {
    pub bodies: Vec<BodyId>,              // 1..=MAX_FEATURE_BODIES, sorted, made before it
    pub offset: [Value; 3],               // world X, Y, Z: lengths within MAX_COORD of 0 (Move::offset_ask)
    pub turn: Option<(AxisRef, Value)>,   // the axis and an angle within a turn either way (Move::angle_ask)
}
pub struct Mirror { pub bodies: Vec<BodyId>, pub plane: PlaneRef, pub keep_original: bool }
pub enum AxisRef { Origin(Axis3), Edge(EdgeRef), Face(FaceRef) }   // Axis3: X, Y, Z
pub enum PlaneRef { Origin(OriginPlane), Face(FaceRef) }
```

- **What they are**: timeline steps on bodies already made, as a
  combine is: `FeatureKind::Move` and `FeatureKind::Mirror` (the
  workers' postcard has them as the fifth and sixth variants). They
  make no body and take no regions; "Move N", "Mirror N". A move turns
  each body about its axis by its angle (right-handed about the axis's
  direction), **then** shifts it by its offsets. A mirror reflects each
  body in its plane; without `keep_original` the body becomes its
  image, with it the body is itself and its image together (decided:
  copies stay in the source body). Bodies keep their ids; a move's
  bodies, and a mirror's without the original, keep their faces' names,
  so later references (a sketch on a face, an axis edge) follow them to
  where they went.
- **References**: an origin axis is the world axis through the origin,
  along its positive side. An edge (`EdgeRef`, as a revolve's axis
  edge) must be straight, the line through it directed as `EdgeRef`
  says (first key's face on its left from outside), or round (a circle
  or an arc of one: its axis through its centre, turning the way the
  edge runs as so directed). A face must be round: a cylinder's,
  cone's, torus's or revolved surface's axis, **directed as the face's
  form has it** (an extruded wall's along the extrude): the sign of the
  angle is the user's to flip. A mirror's face must be flat: its form's
  plane `n·x = d` gives the point `n·d` and normal `n` (exact for faces
  square to a world axis). Edges and faces are found on their body as
  the features before the move or mirror leave it, through
  `Evaluation::holder` (a body a join consumed is looked for in its
  holder); the reference's body may be one of the bodies moved (a body
  mirrored in its own face).
- **Checks** (`CheckError::Move` / `Mirror(id, MotionError)`):
  `Move::check_own(design)` / `Mirror::check_own()` (cheap, for a
  panel): 1 to 256 bodies (`Bodies(n)`), sorted without repeats
  (`BodyOrder`), each offset a length `Move::offset_ask` takes (within
  `MAX_COORD` of zero; zero and negative allowed: `Offset`), the angle
  one `Move::angle_ask` takes (within a turn either way: `Angle`), an
  axis edge's own check (`Edge`) and a face's point finite and within
  `MAX_COORD` (`Near`). `Document::check` then wants every body moved
  there and made by a feature before it (`Body(id)`, as a combine's: an
  edit making its maker stop making it is refused), and the axis's or
  plane's body and faces' makers before it, or not there with ids no
  later body or feature can take (`RefBody`, `RefMaker`), as a sketch's
  face and a revolve's edge are.
- **Removal**: `FeatureKind::bodies()` lists the bodies moved, so
  removing one or its maker removes the move or mirror. The axis's or
  plane's body isn't listed: removing it leaves the feature, which then
  fails to regenerate until given another (as a revolve's axis edge).
- `SetUnits` pins the offsets by `Move::offset_ask` and the angle by
  `Move::angle_ask`. Mirrors have no values.

## Regeneration

`crates/regen/src/history/motion.rs`, in history order:

- **Bodies with no solid of their own fail it**, worded as a combine's
  (`history::own_solids`, which the combine shares).
- **The motion**: one `varde_kernel::Motion` per feature. A move's is
  `Motion::turn(point, direction, degrees)` then
  `Motion::translation(offsets)`. The angle is stored in radians and
  turned back into degrees by `value / (π/180)`: a value typed in
  degrees comes back to the number typed for every multiple of 90°
  within a turn (checked), so quarter turns about world axes move
  coordinates to the bit. A mirror's is `Motion::mirror(point, normal)`.
  The axis or plane is resolved (`resolve_axis`, `resolve_plane`) and
  cached (`Entry::Reference`, keyed by the solid's key, the reference's
  names and point, and the fit tolerance).
- **Each body** (in the order made): first refused if the motion takes
  a corner of its box past `MAX_COORD` ("moving Body 1 takes it out of
  range: every part must stay within 1000000 mm of the origin"; the
  box's image holds the solid's, so a body passing is in range; a turn
  near the limit may be refused for its box's corners), then
  `Solid::transformed(motion, copy, tol, budget)`, cached under
  `moved_key(body key, Motion::bits, copy, fit)`: an edit leaving the
  motion's bits as they were (an offset typed another way, the axis's
  body edited elsewhere) finds every body again. Without the original
  the copy is `None` (names kept); with it the image is `Instance {
  feature: mirror id, index: 1 }` and `assemble([body, image])` puts
  the two together (side by side when apart, a union when they meet).
  All bodies are worked out before any changes: one failing fails the
  feature, changing none. Kernel failures: "moving Body 2 leaves no
  clean solid: rounding brings parts of it too close together; try a
  finer tolerance" (`message::moving`), "joining Body 1 to its mirror
  image ..." (`message::with_image`), with the kernel's evidence by
  value (no operand faces: the body is drawn where the history leaves
  it).
- **Reference failures**: "its axis edge's body is gone", "its axis
  edge wasn't found", "its axis edge isn't straight or round" (its
  curves), "its axis edge's direction can't be told" (its curves); "its
  axis face's body is gone", "its axis face wasn't found", "its axis
  face isn't round" (the face); "its mirror face's body is gone", "its
  mirror face wasn't found", "its mirror face isn't flat" (the face).
- Nothing is merged and nothing touched: `Evaluation::merged` and
  `touched` are as the features before left them, so the app's
  `merged_before` and the naming replay need nothing new. Drafts need
  nothing new either.

Tests: `document/src/motion/tests.rs` (checks, removal, units, postcard
and hostile bytes), `regen/src/history/tests/motion.rs` (a move then a
join at the new place, quarter turns exact, turns about a model edge, a
round face and a rim, mirrors with and without the original apart,
flush and overlapping, a body mirrored in its own face and a block in a
wedge's slanted face, wrong-shaped and missing references with their
geometry, out of range, a consumed body, a sketch following a moved
face, the cache), `regen/src/wire/tests.rs` (a move draft through the
wire), `io/src/vrdp/tests.rs` (through a file).

## UI

**The session** (`app/src/doc/motion.rs`, `Doc::operation` holding one
`MotionSession` for both and for patterns, aligns, scales, splits and
chamfers, its `MotionKind` saying which; their own parts are under
"Pattern", "Align", "Scale", "Split" and "Chamfer", "UI") is started by
`Look::StartMove` (`M`, `Shortcut::MOVE`, the UI mock's key, the
toolbar's Move after Combine, the rail's Transform set) or
`Look::StartMirror` (no key, as the mock has none: the rail only, as
the mock's model bar has no Mirror, which leaves room for Chamfer at
1280 px; `Shortcut::NONE`, a binding that's never
pressed and shows no key, so the rail's list gives it the first free
letter of its name, `I`), again (or `Esc`, Cancel) backing out of it and
one of the other kind replaced; or by editing one (`Look::EditFeature`: a
double-click or `Enter` on its row, Edit move / Edit mirror in its
menu). In a document that can be changed, outside sketches; it drops the
other operations as they drop it, and the measure tool; the combine's
`B` is off while one is set up, as it is during an extrude or revolve.
Both bindings want a body in the document (`DocumentKeys::bodies`). A
new one takes its bodies from what's selected in the model (each item's
body, a merged one as its holder), or with nothing selected the model's
only body if it has one (of those a new feature can name, not merged
into another: `Doc::only_body`), as the mock's init does. A new move
starts with "0 mm" offsets, "0°" and the Z axis (the mock's), clicks
picking bodies; a new mirror keeps the original (the mock's Create copy
on), its plane to pick, clicks picking the plane if it has bodies.

- **Bodies** are picked as a combine's: the cursor picks the model,
  preview included (a move's or mirror's draft makes no body, so the
  model's bodies are the document's), only faces, and a click on one
  picks or un-picks its body, as a row in Objects does
  (`Doc::motion_body`), merged bodies as their holder
  (`MeshFeed::merged_before` up to the feature), only those made before
  it (`combine::pickable`), at most `MAX_FEATURE_BODIES`, kept sorted.
  Bodies picked on a model that didn't show a merge yet follow it once
  it does (`MotionSession::follow`, `Doc::follow_motion_merges`, after
  each edit and answer, as the combine's `follow`). A body picked that
  the document no longer has, or that isn't made before the feature any
  more (an undo took it away), is kept, listed as the mock's "Missing
  body", and the panel's foot says "A picked body is gone"
  (`MotionSession::prune` notes them, `MotionSession::gone`): nothing is
  previewed or committed until it's taken out (its row's cross) or a
  redo brings it back.
- **The axis or plane** is picked once its panel field is clicked
  (`MotionPick::Reference`): the toolbar then offers the origin axes
  ("X axis", "Y axis", "Z axis") or planes after Measure, as picking a
  sketch's plane does (`MotionLook::OriginAxis`, `OriginPlane`; the
  viewport draws those planes and a click on one nearer than the model
  picks it as its button does, `ModelPicking::origin_planes`,
  `varde_view::motion_picks_origin_planes`), and the viewport picks
  edges and faces (`Picks::EdgesAndFaces` for a move, a
  pick that never takes a vertex; faces for a mirror). A move takes a
  straight edge (`PickIndex::edge_ends`) or a round one (one with a snap
  point, a circle's or arc's centre) and a round face (its summary a
  cylinder, cone, torus or revolved surface); a mirror a flat face. Each
  is named as a sketch's face or a revolve's edge is (`Naming`, the
  history stopped at the feature: `Naming::edge_ref` and the new
  `Naming::checked_face_ref`, which refuses as `edge_ref` does). A face
  of a mirror's image is named as the mirror's copy (its key's
  `instance`), so `Naming` also refuses a face (or an edge of one) whose
  copy no mirror keeping its original before the stop makes
  (`Naming::takes_key`, the copies replayed from those mirrors, images
  of images too, up to 4096 then any): a later mirror's image, shown
  when a sketch's plane or a revolve's edge is picked again with later
  features in the model, isn't there at the feature. Refused
  with a notice in the status bar, the app's stand-in for the mock's
  toast: "Only a straight or round edge, or a round face, can be the
  axis", "Only a flat face can be the mirror plane", "Only a face made
  before the move can be picked", "Which body that face is on at the
  move can't be told: pick another", and a pick on a model that doesn't
  answer what was asked last, what it measures aside ("The model shown
  is out of date: ...", `MeshFeed::answers_request`). Only what a click takes is lit while
  hovered. A pick hands the clicks back to the bodies. While the axis or
  plane is picked the model shown is the history as of the feature, so
  what's clicked is where the feature finds it: a new one sends no
  draft then, and an edited one a move of its bodies by nothing (turning
  them by nothing about a move's or pattern's axis, so regeneration
  still finds it; the bodies and axis that are gone left out).
  An axis or plane the document no longer takes at the feature's place
  (`Document::check_axis_ref`, `check_plane_ref`: an undo took its body
  or a face's maker away), or whose body the document no longer has, is
  kept, and said to be gone as the mock does (from the start when the
  feature edited already names one, its body removed): "The axis is gone: pick
  another" (only while the move turns: an angle of zero needs no axis),
  "The plane is gone: pick another"; nothing is previewed or committed
  until another is picked or a redo brings it back.
- **Typed fields** (a move's): the offsets read by `Move::offset_ask`,
  the angle by `Move::angle_ask` (`TypedText`, kept where the units
  change, as an extrude's). An angle of zero stores no turn
  (`Move::turn` is `None`); the axis is kept in the session for when it
  isn't.
- **Whole and ready**: bodies, and a move's values as they last read
  with an axis while the angle isn't zero, or a mirror's plane
  (`MotionSession::kind`); ready (`Doc::motion_ready`) when editable, no
  sketch edits wait on the solver, no field is refused, nothing it names
  is gone, nothing is still to do (`MotionSession::need`, the mock's words for the status bar: "pick
  the bodies to move", "enter a distance or an angle", "pick an axis to
  rotate about", "pick a plane: an origin plane or a planar face") and its
  own check passes (else its words in the panel's foot, "Move fails",
  without Add anyway).
- **Preview**: the feature as set up is the request's draft
  (`Doc::motion_draft`, after the others in `Doc::request_model`); a new
  move that moves nothing sends none. Its error shows in the panel as
  "Move fails" or "Mirror fails" with Add anyway. Regeneration's answer
  carries where it found the axis or plane (`Drafted::reference`,
  below), which the viewport draws.
- **Committing** (`Edit::CommitMotion`: OK, the screen's `Enter`, or
  `Edit::AcceptError` for Add anyway) applies `AddFeature` ("Move N",
  "Mirror N") or `SetFeature` through `Doc::commit_feature`, one undo
  step, selects it and ends the session. A replacement of the document,
  read-only, or the edited feature gone end it (`Doc::prune_motion`).

**The panel** (`view/src/motion.rs`, `MotionState`, in
`operation_panel`), the UI mock's: title "New move" / "New mirror" or the
feature's name, with the mock's `move` and `bmirror` icons
(`Icon::Move`, `Icon::BMirror`); a Bodies field (rows with the Body icon
and a cross, "Click bodies"); a move's "Translate" section with the X, Y
and Z fields (the X field is `VALUE_FIELD`, focused as it opens), then
"Rotate" with the Axis field ("Z axis", "Edge of Body 1", "Extrude 1's
side" as a row with the axis icon, "Click an axis or edge") and the
Angle field; a mirror's Plane field (`Icon::SePlane`, "XY plane",
"Extrude 1's end", "Click a plane or face") and Create copy, an icon
toggle (`Icon::TkCopy`) with "Keep the original too" as its tooltip,
which is `Mirror::keep_original`. The mock's Move panel also has Create
copy; the document's `Move` has no field for it, so it's left out (a
defaulted field later would keep files working, but it's a file format
change, waiting on the user's word). The axis or plane row has no cross
(as the revolve's axis row): picking another replaces it. Hovering it
lights the axis or plane in the viewport (`PanelHover::Axis`).

**The viewport** (`view/src/viewport/motion.rs`, `Moving`, one of
`viewport::Operating`): drawn on top of the model, as the measure tool
draws, the axis (a line across the bodies' box, `PickIndex::bodies_bounds`
on the model shown, a quarter past half its diagonal either side of the
point nearest its centre, at least 10 mm, with an arrowhead on the
screen at the end positive angles turn right-handed about) or the plane
(a square as wide across the box, dashed outline, a faint fill, and a
short line along its normal). An origin axis or plane is drawn as
known; an edge or face where the newest draft answered found it. A move
whose angle is zero shows its axis only while it's being picked. The
labels' layer is an empty placeholder: the handles are drawn by the
renderer with the rest.

**A move's handles** (`viewport/motion.rs`, question 13's decision: typed
fields and handles): at a pivot of the bodies (`MotionState::centre`):
the centre of their box in the model shown (`MotionState::bounds`,
merged bodies as their holders) found once, taken back through the move
that model shows to a point of the bodies before the move
(`Doc::follow_motion_pivot`, `Pivot`, once the model answers what was
asked last and its preview didn't fail; kept while the bodies and the
document stay), and shown where the move as set up takes that point. A
box's centre isn't where a turn takes it, so handles at the box centre
would jump as a ring is let go of and the next turn would be about
another point; these stay where a ring turned the bodies about, move
with typed or dragged offsets at once, and a ring turned back undoes its
turn. Before the pivot is known, the box's centre. A fixed size on
the screen, laid out there so the arrows and the rings' knobs keep apart
(`Handles::new`, "big orb, short arrows"): an arrow
along each world axis 50 px long on the screen, each flipped to whichever
end spreads the three most (the least angle between them on the screen
the widest, flipping as few as that allows) and left out pointing within
about 17° of the eye (shown shorter than 0.3 of its length), a shaft in
the axis's scene colour (`Colors::axes`) to a puck as the extrude
handle's, its ring in the axis's colour and its arrow in the handles'
accent; then a faint orb 104 px out (ink, at 0.25), and on it a knob for
each ring not seen nearer edge on than 0.09: the gaps between the
arrows on the orb are cut into slots 31 px clear of the arrows and 58
px apart, the rings put in them as most fit (then the largest least
scale), each ring's radius such that it runs through its slot's middle
on the orb, a puck there (its arrow the way positive angles turn) on the
stretch of the ring in the slot (at most 40° either way and the orb's
radius long, 12 px clear of the slot's edges). The one under the cursor
or dragged lighter, with an ink rail along its axis or round its ring.
Shafts, arcs and pucks are on the screen over the model. A test checks
from all round, in both projections, that no two knobs' grab areas meet
and no ring's knob or arc comes within reach of an arrow. Not for a mirror,
while the axis is picked, without the bodies' box, or in a document
that can't be changed; while the move turns about another axis than a
world axis only the arrows, while about a world axis only its ring
(`MotionState::origin_axis`). Drawn on top of the model, as the axis is.
They take the mouse ahead of picking the model (`Moving::mouse`,
`viewport::Interaction::motion`): over one, the cursor is a grab hand,
the model's hover is let go of and not worked out again until the cursor
leaves it; a press on one grabs it, anywhere else goes to picking and
the camera as before. Hit testing on the screen: an arrow's shaft within
6 px or its puck (the nearest arrow first), then the nearest ring's knob
(its puck, or its arc within 6 px). A drag keeps the centre where it was grabbed (an arrow's moving
with its offset). Dragging an arrow sets that axis's offset: the offset
as grabbed plus how far along the arrow's line the cursor's ray has
passed nearest it (`Projector::along_line`, which the extrude handle
drags by too), snapped absolute to the extrude handle's steps, of a
pixel's size at the camera's target as the extrude's are (`snap_step`;
the handles' size is a pixel's at their centre, the steps the same
wherever the bodies are), sent as `MotionLook::Input` with the text
formatted in the design's units (`MotionState::units`). Dragging a ring
measures the angle the cursor sweeps about the centre on the ring's plane
(right-handed about the axis, unwrapped across half turns), the angle as
grabbed (the angle field's value, zero if none) plus it snapped absolute
to round degrees (1, 2, 5, 10, 15, 30, 45 or 90, the first 6 px along
the orb: 5° at 104 px) and brought within a turn, and sends
`MotionLook::Turn { axis, angle, offset }`: the session's axis becomes
that world axis (`AxisRef::Origin`), and since a move turns about an axis
through the origin before it shifts, the offsets become the grabbed
ones turned about the centre by the angle's growth
(`R·(offset − centre) + centre`, the kernel's `Motion::turn` about the
centre, exact on quarter turns; the angle in degrees as regenerating
reads it, the radians divided by a degree's factor), so the bodies turn
in place. The app takes a `Turn` only as the handles offer it
(`Doc::motion_look`): a move's, picking bodies, turning by nothing yet
or about that world axis already; any other is dropped. A drag past the coordinate limit sends nothing. The texts go
to the fields, so the preview, refusals and OK are those of typed values,
one undo step. A ring dragged from a move about another world axis's
turn isn't offered (its ring isn't shown); a move about an edge or face
turns about its world axis only once its angle is typed back to zero.

The status bar says "New move · Body 2" ("· 30° about Z axis" with a
turn), "New mirror · Body 2 across XY plane", or what's still to do,
with the hints "Pick bodies", "Pick the axis" or "Pick the plane",
`Enter` OK and `Esc` Cancel; the toolbar's pill with OK.

**The Timeline**: the mock's icons and notes. A move's note is how far
it shifts in all and its angle, "82.462 mm 30°" (`motion::move_note`,
either left out when it's none, "0 mm" for a move doing nothing); a
mirror's is its plane, "XY" or "Extrude 1's end" (`plane_short`).
Selected, the status bar's info is the mock's: "Body 1 by -20, -80, 0
mm, 30° about Z axis" (`move_info`), "Body 1 across XY plane · copy"
(`mirror_info`).

**Regen's answer** carries the axis or plane found:
`Evaluation::references` lists, for each move turning about an axis and
each mirror, the point and direction (normal) its motion was made from
(a round face's axis through its point nearest the face's point, not its
form's own point: a nearly flat cone's apex can be far past the limit
the wire takes),
and a draft's `Drafted::reference` is its feature's, as
`[[f64; 3]; 2]`, on the wire too (checked on receipt: finite, within
`MAX_REFERENCE`, four times the coordinate limit, the direction not
zero; else the generation fails, `wire::Error::Reference`). Regenerating
notes only references that pass that check (`reference_fits`): one
that doesn't (an axis of no length) isn't drawn, natively as on the web,
rather than failing the whole reply there. Not in `.vrdp`.

Tests: `app/src/doc/motion/tests.rs` (`M` with the selection, typed
offsets moving the preview, `Enter` one undo step, the Timeline note and
status info; a round face's axis found by regeneration and a quarter turn
about it; a mirror across a picked face with copy, a curved face refused;
editing from the Timeline and undo, the neutral preview while the axis is
picked, `Esc`; a field refused, a preview failing out of range with Add
anyway; a straight edge as the axis and a face made after the move
refused); `view/src/motion/tests.rs` (the panels' order and texts, the
notes); `viewport/motion/tests.rs` (the axis and plane drawn, the
handles only for a move picking bodies, the Z arrow dragged to a snapped
offset, a ring a quarter turn about the centre, only the turn's ring
while turning, clicks off the handles picking the model and on them not,
none while the axis is picked); the app's tests also apply what the
handles send (an arrow's offset, a ring's turn keeping the box centre,
OK one undo step; a mirror ignoring a turn; the handles staying where a
lopsided pair of bodies turned about and turning back to no offset; a
ring after a turn about an edge typed back to zero), the only body
picked, picked bodies and an axis or plane an undo takes away said to be
gone, a body a redone combine merges followed;
`regen/src/wire/tests.rs` (the reference on the wire, bad ones refused);
`regen/src/history/tests/motion.rs` (a near-cylinder cone's axis end to
end through the wire; later features naming a mirror's image's faces;
values at their bounds) and `motion/fuzz.rs`: random histories of
bodies, joins, combines, moves (origin axes, straight edges, round
faces) and mirrors (origin planes, flat faces, with and without the
original), edits, removals, undo and redo, each move and mirror held to
the volume and centre of mass the motion worked out with `glam` gives
(a mirror with its original to the boolean identities with the image),
plus the combine fuzz's warm/cold, flipped-bytes, later-edits and wire
checks (more with `VARDE_TESTS=full`, one replayed with `VARDE_TEST_SEED`).

Known gaps: the moved bodies' old place isn't shown faded (the mock's
`fade`), as the preview replaces the model; a move with an angle of zero
draws no edge or face axis (its draft names none); a draft that fails
before its reference is resolved (a body consumed) draws none.
A ring's turn shifts the offsets to turn about the centre, so their
texts come out unround (rounded to the units' decimals) after a ring is
dragged by an angle other than a quarter turn; the handles have no look
in the UI mock, which shows none, so they take the extrude handle's.
Other departures from the mock: a move's axis may also be a round edge
or face (the mock's: an origin axis or a straight edge). A ring's
offsets are typed rounded to the units' decimals, so the pivot drifts by
that rounding (under a micrometre in millimetres) with each ring turn.
