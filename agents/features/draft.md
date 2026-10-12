# Draft

`crates/document/src/face_draft.rs`.

```rust
pub struct FaceDraft {
    pub faces: Vec<FaceRef>,   // 1..=MAX_DRAFT_FACES (256), one body, FaceRef::order, no repeats
    pub neutral: PlaneRef,     // an origin plane, or a flat face of a body made before
    pub angle: Value,          // above 0 and under 90° (FaceDraft::angle_ask)
    pub flip: bool,            // the pull against the neutral plane's normal
    pub tangent: bool,         // tangent faces taken in (the kernel grows them)
}
```

- **What it is**: the fifteenth variant (`FeatureKind::FaceDraft`,
  "Draft N"; the type isn't `Draft`, which is regeneration's preview).
  Its faces, all of one body, are turned by `angle` about their
  **hinges**, where each meets the neutral plane, so that each face's
  outward normal leans towards the **pull**: the neutral plane's normal
  (a face's outward normal, an origin plane's axis: +Z for XY),
  reversed by `flip`. The part narrows along the pull (as it would come
  out of a mould pulled that way); a face that doesn't reach the
  neutral plane turns about the hinge line all the same, outside it.
  The neutral plane only (no parting lines, question decided); planes
  not facing the pull and walls along the pull are drafted, the rest
  refused (decided). The angle is `Chamfer::angle_ask`'s kind,
  `FaceDraft::angle_ask` (above 0, under a right angle, bare numbers in
  degrees). The body keeps its id and every face its name. It has no
  body of its own: `body()` is the first face's.
- **Checks** (`CheckError::FaceDraft(id, FaceDraftError)`):
  `FaceDraft::check_own(design)` (cheap): the faces as an offset face's
  (`Faces(FaceListError)`), the neutral face's
  own check (`Neutral`), the angle by its ask (`Angle`).
  `Document::check_face_set` as an offset face's (`Set`),
  and `Document::check_neutral_plane(index, neutral)` (public, for the
  panel): a face's own check as `check_own`'s, its body there and made
  by a feature before the draft
  (`NeutralBody`), its key's maker before it or not there with an id no
  later feature can take (`NeutralMaker`). An origin plane needs
  nothing.
- **Dependencies**: `FeatureKind::bodies()` is its faces' body and the
  neutral face's body (sorted, once), as the plan has it, so removing
  either or its maker removes the draft (unlike a mirror's or a split's
  plane face, which leaves the feature to fail). The features that made
  its faces or its neutral face are not followed: removing one leaves
  the draft failing ("its face wasn't found", "its neutral face wasn't
  found").
- `SetUnits` pins its angle by its ask.

## Regeneration

`crates/regen/src/history/face_draft.rs`, in history order:

- The body needs a solid of its own (`InPlace::of`, as the other
  in-place features). Each face is found on its topology; one not found
  fails the draft before the kernel: "its face wasn't found", or "its
  face 2 of 3 wasn't found". The neutral plane is resolved as a
  mirror's (`motion::resolve_plane`, now taking the feature's words,
  `PlaneWords`: `MIRROR_PLANE`, `NEUTRAL_PLANE`): an origin plane
  through the origin, a face found on its body (`holding`: its holder if
  merged) as the features before leave it, which must be flat; its
  failures "its neutral face's body is gone", "its neutral face wasn't
  found", "its neutral face isn't flat" (the face drawn). The plane
  found is noted for the session to draw even when a face isn't found
  (that failure still comes first). The pull is the plane's unit
  normal, negated by `flip`.
- **The draft**: `varde_kernel_compat::draft_faces(solid, topology, faces,
  neutral point, pull, angle in radians, tangent, feature, tol,
  budget)`, cached as an `Entry::Solid` by the body's key, the feature,
  the fit tolerance, the neutral point's and pull's bits, the angle's
  bits, the tangent flag and the regions; the result replaces the
  body's solid under that key. Faces keep their names, so a sketch on a
  drafted face after it is placed on the face as turned, and follows
  the angle, the flip or a dimension upstream. Its refusals
  (`varde_kernel_compat::DraftError`) are worded by `message::draft_refused`:
  facing the pull "a face of Body 1 faces the pull direction: nothing
  to draft", can't be drafted "a face of Body 1 can't be drafted: only
  flat faces and walls along the pull can", past a neighbour "the face
  turns past a neighbouring face of Body 1: try a smaller angle", into
  the body "the face runs into another part of Body 1: try a smaller
  angle", a round to nothing "a round face of Body 1 narrows to
  nothing: try a smaller angle", no surface and a tangent neighbour as
  an offset face's, a corner "faces of Body 1 meeting at a corner can't
  be drafted together: try another angle", out of range "it moves Body
  1 out of range" (the face or corner drawn where there is one); its
  failures as a boolean's ("drafting faces of Body 1 is too complex to
  work out" for `TooComplex`, "drafting faces of Body 1 isn't supported
  yet" for `NotImplemented`, `message::drafting`); an empty result
  "drafting faces of Body 1 leaves nothing of it".
- **Kernel stand-in**: `varde_kernel_compat::draft_faces` (`kernel/src/shell.rs`,
  beside `offset_faces`, with `DraftError`) isn't built: it fails with
  `NotImplemented`, so every draft that finds its faces and neutral
  plane fails today with "drafting faces of Body 1 isn't supported
  yet", the body left as it was, and the rest of the history goes on.
  The regen tests swap it (`testing::Overrides::draft`, per thread, the stand-in in `testing/stand_ins/face_draft.rs`; other
  crates' tests through the `testing` feature,
  `varde_regen::testing::draft_by_boxes`) for `by_boxes`: a box along
  the world's axes of eight corners, the pull along a world axis; each
  picked side along the pull turns about its hinge by moving its four
  corners across the pull by `tan α` times their height above the
  neutral plane (the faces around stay on their planes, the turned
  one's form and claim become the turned plane), rebuilt and checked by
  `Solid::new`, every face keeping its name; a top or bottom picked is
  `FacingPull`, sides meeting the ones opposite at the box's top or
  bottom `PastNeighbour`, past the coordinate limit `OutOfRange`, the
  rest too complex. The planned analytic tests of the kernel's draft
  are written and `#[ignore = "kernel draft not built"]`.
- The draft's reply carries nothing new.

Tests: `document/src/face_draft/tests.rs` (added and undone, edited,
its own parts, bodies and makers with the neutral face's, removal
following both bodies and not the faces, units pinned, round trip,
wrong ones refused when read, the fifteenth kind (the one test holding
its index), errors), `regen/src/history/tests/face_draft.rs` (the stub
failing as too complex with the history going on; a face gone after
its maker is removed, named by its place, the plane still noted; the
neutral face gone ("its
neutral face wasn't found") and a round one not flat; with the box
stand-in: a draft surviving an upstream height change, flip and a
neutral face on top by their volumes and undo, four sides from the
foot (a frustum), from a plane through the middle and from another
body's face, a sketch on a drafted face following the angle and an
upstream change, sides meeting and a face facing the pull refused, the
cache, a consumed body; recording: a face named twice handed over
once, the neutral point, pull, angle and tangent flag; every refusal
worded and drawn; the stand-in against sets of a box's faces (all
under `VARDE_TESTS=full`) drafted from XY and from its top, flipped or not, by volume and names
kept; ignored, the kernel's: a box's sides from the foot and the top,
names kept, the same bits twice; hinges inside the faces; a boss's
wall an exact cone and a hole's widening; a rounded slot across its
tangent faces and refused without them; a plate with holes drafted as a
whole; an elliptic wall fitted; a sphere and a narrow slot closing
refused), `regen/src/history/tests/face_draft/fuzz.rs` (random
histories with drafts about origin planes and faces (flat or not, a
drafted one, another body's, one merged by a combine before it):
failing alike in the whole history and the one ending with it, a
failure changing nothing, every one that works of a box with its pull
along an axis and of the volume its cross-sections give, names kept,
cache warm and cold, bytes and wire; `VARDE_TESTS=full`), `io/src/vrdp/tests.rs` (through a file, tampered points and
angles refused, damaged 2 000 ways refused or checked, faces out of
order, repeated, none, on two bodies, past the limit, named by the
draft or later, the neutral face named by the draft or on a body not
there, angles of 0 and 90° refused as read), `view/src/face_draft/tests.rs`
(the notes).

The Timeline shows the icon mock's draft icon (`Icon::Draft`) and note
(`view/src/face_draft.rs`: "3° · 4 faces"); selected, the status bar
says `draft_info` ("4 faces · 3° from XY", ", flipped").

## UI

The move's session (`MotionSession`) with `MotionKind::Draft`, its faces
picked by the face session (see the shell's UI: `faces.rs`, `refs.rs`,
`PickedFaces`) and its neutral plane in the session's `plane`, picked as
a mirror's; its own parts in `app/src/doc/motion/face_draft.rs` and
`view/src/motion/draft.rs` (`DraftView`). **The model mock has no draft
panel** (only the icon mock's `draft` icon, in the Modify group after
Shell), so the panel is built in the style of the mock's shell panel.

- **Starting**: `Look::StartMotion(MotionKind::Draft)` from the rail's Modify set (after
  Shell, before Scale, as the icon mock's Modify group orders it; its
  letter D), again backing out; no key, as the mocks have none. **Not
  on the toolbar**, as Shell and Offset face aren't. Or editing one
  (`Look::EditFeature`: double-click, `Enter`, "Edit draft"), which
  opens with its faces, neutral plane, angle, Flip and Tangent faces. A
  new one takes the faces selected in the model shown that a click
  would take, the first one's body deciding.
- **Faces**: as an offset face's (`MotionPick::Faces`): a click picks or
  takes out, all on the first face's body ("A draft's faces are all on
  one body: pick faces of Body 1"), an edge "Only a face can be
  drafted", at most `MAX_DRAFT_FACES`, sorted, lit, a row hovered
  lighting its face; with none the status bar says "pick the faces to
  draft". Faces whose maker an undo took away are said to be gone.
- **The neutral plane**: XY to begin with (the plan names none; the pull
  then up, so the part narrows upwards), shown as the panel's "Neutral
  plane" row (a mirror's plane row: "XY plane", "Extrude 1's end").
  Clicking the row makes clicks pick it (`MotionPick::Reference`, the
  hint "Pick the neutral plane"), and again hands them back to the
  faces; while it picks, the toolbar offers the origin planes
  (`MotionLook::OriginPlane`, taken only then) and a flat face clicked
  is taken (`Doc::reference_in`, named as of the feature; anything else
  "Only a flat face can be the neutral plane"), the clicks going back to
  the faces either way. One of the faces drafted is refused, "That face
  is one the draft tilts: pick a face it doesn't, or an origin plane":
  the preview shows it tilted, while the plane named would be the face
  as before the draft (and regenerating refuses a neutral plane among
  the faces drafted anyway); it doesn't light under the cursor either. A face neutral plane an undo takes away (its
  body or its maker, checked by `Document::check_neutral_plane`) is kept
  and said to be gone, "The neutral plane is gone: pick another",
  nothing previewed or committed until another is picked or a redo
  brings it back.
- **The panel**: "New draft" or its name; **Faces** (rows "Face 2" with
  the face's kind), **Neutral plane**, **Angle** (`MotionField::Angle`,
  read by `FaceDraft::angle_ask`: "3" (degrees) to begin with; 90° or
  more, or nothing, refused under the field), **Flip** (a tick, the
  session's `flip`, `Icon::TkFlip`, "Pull against the plane's normal")
  and **Tangent faces** (a tick, on to begin with). The failure in the
  foot ("Draft fails").
- **In the viewport**: the neutral plane drawn as a mirror's, through
  the point of the plane nearest the middle of the body's box, with the
  pull as an axis's arrow through it (`face_draft::neutral_line`; an
  origin plane's from its axis, a face's from regeneration's answer,
  which notes the plane found as a mirror's, unflipped). Its knob turns
  the first face about its hinge (see Operation knobs).
- **Preview**: the draft as set up is the draft while it's whole, picking
  faces or the plane. **The kernel's draft isn't built**, so today
  every preview fails with "Draft fails" over "drafting faces of Body 1
  isn't supported yet", OK waits, and Add anyway keeps it,
  failing in the Timeline.
- **Committing**: OK (`Enter`, Add anyway) adds "Draft N" or sets the
  edited one, one undo step; Cancel or `Esc` leaves no trace. The
  status bar says `draft_info` once whole ("1 face · 3° from XY"), else
  what's next; the hint "Pick faces to draft" (`chrome::step_hint`).

Departures from the plan and mock: the panel is in the shell panel's
style (the mock has none); not on the toolbar; Flip and Tangent faces as
ticks; the neutral plane starts as XY. Known gaps: the neutral plane is
drawn only where its line is known (a face's once the preview answered);
a plane picked goes back to picking faces even if their picking was
turned off before.

UI tests: `view/src/motion/tests.rs` (the panel's order, no Bodies or
Direction, the status text), `rail/tests.rs` (Draft in the Modify set,
its letter) and `app/src/doc/motion/tests/face_draft.rs` (the rail's
Draft on the example plate: no body until a face, XY to begin with, the
panel, an edge refused, a side picked, lit and listed, the stand-in's
not-built failure in the panel, OK waiting and Add anyway keeping it,
failing in the Timeline, undo; with the boxes: the front drafted and
previewed, the plane drawn by the body along the pull, the neutral
plane's row toggling the picking, an edge refused as the plane, the top
picked as the plane and previewed leaning out, Flip, XZ from the toolbar
refused by regeneration (the front faces the pull), an origin plane
taken only while the plane picks, another angle and 90° refused,
Tangent faces, OK and the Timeline's row, undo; editing from the
Timeline, Cancel, another angle, undo; a neutral face on another body
an undo takes away said to be gone, back on redo; the overlap list
ticking the faces, not while the plane picks, a row then taken as the
plane; a neutral face on a body a combine merges into a third before
the draft, mid-session, found on its holder, previewed and committed;
the neutral face one of the faces drafted, refused as facing the pull;
the units changed mid-session keeping the 3° and a typed angle) and
`app/src/doc/motion/tests/face_draft/fuzz.rs` (random sessions in the
offset face fuzz's style: picks, the plane's row and toolbar planes,
angles, Flip, Tangent faces, units, undo and redo, merges of the
session's or the neutral face's body, overlap lists while either
picks; each ready session whole and checked, committed as set up, the
plane drawn finite, and each draft that works by its volume;
`VARDE_TESTS=full`). What the face sessions' tests share (boxes, flat faces found
and clicked, the screen's text) is `app/src/doc/motion/tests/face_session.rs`.
