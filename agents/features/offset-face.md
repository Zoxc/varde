# Offset face

`crates/document/src/offset_face.rs`.

```rust
pub struct OffsetFace {
    pub faces: Vec<FaceRef>,   // 1..=MAX_OFFSET_FACES (256), one body, FaceRef::order, no repeats
    pub distance: Value,       // a length as an extrude's, above zero
    pub inward: bool,          // into the body (shrinking it) rather than out
    pub tangent: bool,         // tangent faces taken in (the kernel grows them)
}
```

- **What it is**: the fourteenth variant (`FeatureKind::OffsetFace`,
  "Offset face N"). Its faces, all of one body, are moved along their
  normals by `distance` (`OffsetFace::distance_ask` = `Extent::ask`),
  out of the body or with `inward` into it, as an extrude's flip, so the
  handle can drag the distance through zero and the stored distance
  stays a positive length (`signed_distance`: negative inward). The
  body keeps its id and every face its name: a later reference to a
  moved face (a sketch on it, a chamfer's edge) still finds it, now
  where it moved. It has no body of its own: `body()` is the first
  face's (`None` only for none, which the document refuses). The list
  is kept in `FaceRef::order` without repeats, as a shell's.
- **Checks** (`CheckError::OffsetFace(id, OffsetFaceError)`):
  `OffsetFace::check_own(design)` (cheap): the faces by
  `check_face_list_own`, as a shell's (`Faces(FaceListError)`), but at
  least one (`NoFaces`) and all on one body; the distance by its ask
  (`Distance`).
  `Document::check_face_set(index, body, faces)` as a shell's, with
  the first face's body (`Set(FaceSetError)`).
- **Dependencies**: `FeatureKind::bodies()` is its faces' body, so
  removing it or its maker removes the offset. The features that made
  its faces are not followed: removing one leaves the offset failing
  ("its face wasn't found"), as every face reference.
- `SetUnits` pins its distance by its ask.

## Regeneration

`crates/regen/src/history/offset_face.rs`, in history order:

- The body needs a solid of its own (`own_solids`). Each face is found
  on the body's topology (`inspect::topology`, `Topology::face(key,
  near)`); one not found fails the offset before the kernel: "its face
  wasn't found", or with several "its face 2 of 3 wasn't found". The
  regions go to the kernel sorted, each once.
- **The offset**: `varde_kernel_compat::offset_faces(solid, topology, faces,
  signed distance, tangent, feature, tol, budget)`, cached as an
  `Entry::Solid` by the body's key, the feature, the fit tolerance, the
  signed distance's bits, the tangent flag and the regions; the result
  replaces the body's solid under that key. Since faces keep their
  names, a sketch on a moved face after the offset is placed on it
  where it went (`Evaluation::placements`), and follows a change of the
  distance or of a dimension upstream. Its refusals
  (`varde_kernel_compat::OffsetError`) are worded by `message::offset_refused`:
  past a neighbour "the face moves past a neighbouring face of Body 1:
  try a smaller distance", into the body "the face runs into another
  part of Body 1: try a smaller distance", a round to nothing "a round
  face of Body 1 shrinks to nothing: try a smaller distance", no surface
  "a face of Body 1 next to it has no surface to extend", a tangent
  neighbour "it is tangent to a face of Body 1 that isn't picked: pick
  it too, or turn on Tangent faces", a corner "faces of Body 1 meeting
  at a corner can't be offset together: try another distance", out of
  range "it moves Body 1 out of range" (the face or corner drawn where
  there is one); its failures as a boolean's ("offsetting faces of Body
  1 is too complex to work out" for `TooComplex`, "offsetting faces of
  Body 1 isn't supported yet" for `NotImplemented`,
  `message::offsetting`); an empty
  result "offsetting faces of Body 1 leaves nothing of it".
- **Kernel stand-in**: `varde_kernel_compat::offset_faces` (`kernel/src/shell.rs`,
  beside `shell`, with `OffsetError`) isn't built: it fails with
  `NotImplemented`, so every offset that finds its faces fails today
  with "offsetting faces of Body 1 isn't supported yet", the body left
  as it was, and the rest of the history goes on. The regen tests swap
  it (`testing::Overrides::offset`, per thread, the stand-in in `testing/stand_ins/offset_face.rs`; other crates' tests
  through the `testing` feature, `varde_regen::testing::offset_by_boxes`)
  for `by_boxes`: a box along the world's axes whose picked faces move
  by a scale along each axis and a move (`Motion::scale` then a
  translation, `Solid::transformed`), so every face keeps its name as
  the kernel's will; a face moved onto or past the one opposite it is
  `PastNeighbour`, a box past the coordinate limit `OutOfRange`, the
  rest too complex. The planned analytic tests of the kernel's offset
  face are written and `#[ignore = "kernel offset face not built"]`.
- The draft's reply carries nothing new.

## UI

The move's session (`MotionSession`) with `MotionKind::OffsetFace`, its
faces picked by the face session (see the shell's UI: `faces.rs`,
`refs.rs`, `PickedFaces`); its own parts in
`app/src/doc/motion/offset_face.rs` and `view/src/motion/offset_face.rs`
(`OffsetFaceView`; the app's `FaceHandle`), its handle an operation
knob (`view/src/viewport/knobs.rs`). **The model mock has no offset face
panel** (only the `offset` icon and "Offset face" in a face's context
menu), so the panel is built in the style of the mock's shell panel.

- **Starting**: `Look::StartMotion(MotionKind::OffsetFace)` from the rail's Modify set
  (after Combine, before Split body, as the icon mock's Modify group
  orders `offset` after `combine`), again backing out; no key, as the
  mocks have none. **Not on the toolbar**, as Shell isn't: the bar
  already runs past its room at 1280 px with Shell. Or editing one
  (`Look::EditFeature`: double-click, `Enter`, "Edit offset face"),
  which opens with its faces, distance, side and Tangent faces. A new
  one takes the faces selected in the model shown that a click would
  take, the first one's body deciding. Nothing takes the focus: clicks
  pick faces.
- **Faces**: as a shell's (`MotionPick::Faces`): a click picks or takes
  out, named as of the feature, all on the first face's body
  ("An offset face's faces are all on one body: pick faces of Body 1"; the
  messages' article follows the noun, `words::capital_article`), an edge "Only a
  face can be moved", at most `MAX_OFFSET_FACES`, sorted, lit as
  selected, a row hovered lighting its face. Its body is only its
  faces' (`faces::takes_body` is the shell's alone): with none it has
  none, a body's row picks nothing ("... take them out to pick
  another" once it has faces), and the status bar says "pick the faces
  to move". Faces whose maker an undo took away are said to be gone ("A
  picked face is gone"), as a shell's.
- **The panel**: "New offset face" or its name; **Faces** (rows "Face
  2" with the face's kind, a cross taking it out, `Icon::SeFace`;
  "Click faces" while picking or with none), **Distance**
  (`MotionField::Distance`, read by `OffsetFace::distance_ask`: "1" of
  the design's units to begin with; the plan gives none), **Inward**
  (a tick, the session's `flip`, `MotionLook::Flip`, `Icon::TkFlip`,
  "Into the body, shrinking it") and **Tangent faces** (a tick, on to
  begin with, `MotionLook::TangentFaces`, `Icon::TkChain`, "Take in
  faces that run on smoothly"). The failure in the foot ("Offset
  face fails").
- **The handle** (question 21, decided: the extrude's arrow and knob
  *and* the typed distance): an arrow from the first face's point
  along its outward normal, its knob at the distance (negative
  inward): one of the operations' knobs (see Operation knobs below),
  drawn as the extrude's puck in the Modify colours. Where it stands
  (`offset_face::Anchor`) is found once on the model shown, once it
  answers what was asked last: the first face found there, the point
  nearest its stored point on the face's nearest triangle and the
  outward normal there (`PickIndex::face_point`: the mesh's corner
  normals, or the triangle's own; a flat face's exactly its summary's
  plane, since the mesh's single precision is micrometres off far out
  and a handle found on a preview moved far kept that), taken back by the distance the
  preview moved it when the model shows a working preview of this
  session. It's kept while the first face and the document's
  generation stay, so dragging moves only the knob. Dragged, the knob
  follows the cursor's ray where it passes nearest the arrow's line,
  snapped as the extrude's handle, **through zero to the other side**:
  `MotionLook::DragKnob` sets the field to the size and Inward to the side; zero
  itself is taken as nothing (no distance is zero), nor a distance the
  design's units show as zero (zoomed in far, the snap is finer than
  they show: "0 mm"). Dragged
  through zero to a distance or ticked Inward at it, the draft is the
  same. A typed distance, or Inward ticked, moves the
  knob. It takes the mouse ahead of picking faces (`Input::holds`).
  With the kernel's stand-in the preview fails, the body shown as it
  is, and the handle stands on the face as it is.
- **Whole and ready**: at least one face and a distance;
  `OffsetFace::check_own` refuses in the foot. **Preview**: the offset
  as set up is the draft while it's whole (as a shell's). **The
  kernel's offset face isn't built**, so today every preview fails with
  "Offset face fails" over "offsetting faces of Body 1 isn't supported
  yet", OK waits, and Add anyway keeps it, failing in the Timeline.
- **Committing**: OK (`Enter`, Add anyway) adds "Offset face N" or sets the
  edited one, one undo step; Cancel or `Esc` leaves no trace. The
  status bar says `offset_info` once whole ("1 face · 2 mm inward"),
  else what's next; the hint "Pick faces to move" (`chrome::step_hint`).

The Timeline shows the mock's offset face icon (`Icon::OffsetFace`, the
model mock's `offset`) and note (`view/src/feature_text/offset.rs`: "2 mm · 3
faces", "0.5 mm in · 1 face"); selected, the status bar says
`offset_info` ("2 faces · 2 mm outward").

Departures from the plan and mock: the panel is in the shell panel's
style (the mock has none); not on the toolbar; Inward and Tangent faces
as ticks. Known gaps: the handle stands on the first face only (as the
plan has it); on a curved first face the normal is the drawn mesh's
there, which for a cylinder is radial as the kernel's offset is; an
edited offset's handle is found on the whole history's model, so a
later feature changing the face's body other than rigidly (a scale)
leaves the knob off the face as it's dragged.

Tests: `document/src/offset_face/tests.rs` (added and undone, edited,
its own parts, bodies and makers, removal following the body and not
the faces, units pinned, round trip, wrong ones refused when read, the
fourteenth kind (the one test holding its index), errors),
`regen/src/history/tests/offset_face.rs` (the stub failing as too
complex with the history going on; a face gone after its maker is
removed, named by its place among others; a face found again after an
upstream dimension change; with the box stand-in: a sketch on the
moved top following it out, in and through an upstream change, a block
joined on it; out, in, two faces, opposite faces and every face by
their volumes; past the opposite face refused and just short of it;
recording: a face named twice handed over once, the signed distance
and tangent flag; every refusal worded and drawn; the cache; a
consumed body; the stand-in against sets of a box's faces (all under
`VARDE_TESTS=full`) moved,
out and in, by volume and names kept; ignored, the kernel's: a box's
face out and in and every face, names kept; a hexagonal prism's
slanted wall; a boss's top down and up and its wall; a hole's wall and
a plate's top with the hole extended, the hole closing refused; a
chamfered corner's neighbour pushed past it and a plate's side pulled
into a hole refused; a slot's flat side grown across its tangent ends,
refused without them, the same bits twice), `io/src/vrdp/tests.rs`
(through a file, tampered points and distances refused, damaged 2 000
ways refused or checked, faces out of order, repeated, none, on two
bodies, past the limit, named by the offset or later, and a distance
refused as read), `view/src/offset_face/tests.rs` (the notes),
`view/src/motion/tests.rs` (the panel's order, no Bodies or Direction,
the status text), `view/src/viewport/motion/tests.rs` (the handle drawn
only while there's one and the document can be changed; the knob
dragged out, past zero (not sent) and inward, snapped, holding the
cursor ahead of the model; zoomed in as far as the camera goes, no
distance sent that shows as "0 mm"), `rail/tests.rs` (Offset face in the Modify
set) and `app/src/doc/motion/tests/offset_face.rs` (the rail's Offset
face on the example plate: no body until a face, the panel, an edge
refused, the top picked, lit and listed, the stand-in's not-built
failure in the panel with the handle on the face, OK waiting and Add
anyway keeping it, failing in the Timeline, undo; with the boxes: the
moved top previewed, the handle on the face as it was, a typed distance
moving the knob and the preview, the handle's drag inward, Inward and
Tangent faces drafted, a distance of nothing refused, OK and the
Timeline's row, undo; editing from the Timeline, the handle where the
face was, Cancel, another distance, undo; another body's face and
row refused with why, the face taken out leaving nothing to commit;
Inward ticked and the handle dragged through zero giving the same
draft and preview, in millimetres and inches; the handle dragged while
the previews are on their way and the first face changed: none until
the model shown is of what was asked last, then on the new first face
as it was; the overlap list ticking, picking and taking out the
session's faces, its rows found again on each preview; a handle found
on a preview moved 10 000 mm out standing where the face was,
exactly), and
`app/src/doc/motion/tests/offset_face/fuzz.rs` (random offset face
sessions in the shell's fuzz's style, with handle drags (through zero,
previews on their way, the first face changed between them), quick
clicks, the overlap list, a shell or chamfer being set up when it
starts: each step's session whole when ready, checked, previewed and
committed as drafted, its handle's knob at the distance, and for a new
one whose working preview is shown on the first face as moved with the
handle on it as it is; each offset working giving its box's volume;
`VARDE_TESTS=full`). `regen/src/history/tests/offset_face/fuzz.rs`
is the shell's history fuzz with offsets of one to four faces, out and
in, each failing alike whole and cut short, one failing changing
nothing, one working giving its box's moved volume, every face keeping
its name and the other bodies untouched (`VARDE_TESTS=full`);
`document/src/offset_face/tests.rs` holds old "Offset N" names mixing
with "Offset face N" ones. The app's tests offset by boxes through
regen's `testing` feature (`varde_regen::testing::offset_by_boxes`).
