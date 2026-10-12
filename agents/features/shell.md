# Shell

`crates/document/src/shell.rs`.

```rust
pub struct Shell {
    pub body: BodyId,                    // made by a feature before it
    pub open: Vec<FaceRef>,              // 0..=MAX_SHELL_FACES (256), on `body`, FaceRef::order, no repeats
    pub thickness: Value,                // a length as an extrude's
    #[serde(default)] pub outward: bool, // walls outside the faces (the mock's Direction: Outward)
}
```

- **What it is**: the twelfth variant (`FeatureKind::Shell`, "Shell
  N"). Its body is hollowed to walls `thickness` thick and keeps its id;
  it makes no body. The faces in `open` (the mock's "faces to remove")
  are opened; none gives a closed hollow body, its void inside (the
  mock's "No faces removed: the body becomes closed and hollow"). The
  thickness is a length as an extrude's (`Shell::thickness_ask` =
  `Extent::ask`). `outward` (the mock's Direction, Inward / Outward)
  grows the walls outside the body's faces instead, the body becoming
  the hollow; the plan's struct had no direction, the mock's panel has
  it, and it's `#[serde(default)]` (inward). The plan's `rounded` (round
  inner edges) is left out until rounded shells are built; it'll be a
  defaulted field then. The list is kept in `FaceRef::order` (body, key,
  the point's coordinates by `total_cmp`) without repeats; two picks of
  one key at different points are two faces (a face cut in two).
- **Checks** (`CheckError::Shell(id, ShellError)`):
  `Shell::check_own(design)` (cheap): the faces by `check_face_list_own`
  (`plane.rs`, shared with the offset face and the draft; its
  `FaceListError` in `Faces`): at most 256 (`Count`), each face's own
  check (`Face(PlaneError::Near)`: its point finite and within
  `MAX_COORD`), in order without repeats (`Order`), all on the shell's
  body (`Bodies`); then the thickness by its ask (`Thickness`).
  `Document::check_face_set(index, body, faces)` (public, for the
  panel; an offset face's faces too, its `FaceSetError` in
  each feature's error's `Set`): the body there and made before (`Body`:
  depended on, as a combine's bodies), and every face key's feature
  before it, or not there with an id below the next (`RefMaker`, as a
  sketch's face's).
- **Dependencies**: `FeatureKind::bodies()` is its body, so removing it
  or its maker removes the shell. The features that made its open faces
  are **not** followed: removing a cut whose wall it opens leaves the
  shell, which then fails ("its open face wasn't found"), to be edited
  (decided, as every face and edge reference).
- `SetUnits` pins its thickness by its ask.

## Regeneration

`crates/regen/src/history/shell.rs`, in history order (the steps it
shares with chamfer, fillet and offset face, the body's own solid and topology,
its faces found, the cache key's start, the result put in place and
the refusals' drawing and kernel failures (`refused`, `kernel_failed`),
are `history/in_place.rs`'s `InPlace`, with `box_sides` for the
stand-ins):

- The body needs a solid of its own (`own_solids`: a body consumed by
  a join or combine fails it, "Body 2 is in Body 1 now: ..."). Its
  topology is the one drawing it keeps (`inspect::topology`), and each
  open face is found on it by `Topology::face(key, near)`; one not found
  fails the shell before the kernel: "its open face wasn't found", or
  with several "its open face 2 of 3 wasn't found" (its place in the
  list). The regions found go to the kernel sorted, each once.
- **The shell**: `varde_kernel_compat::shell(solid, topology, open, thickness,
  outward, feature, tol, budget)`, cached as an `Entry::Solid` by the
  body's key, the feature, the fit tolerance, the thickness's bits, the
  direction and the regions; the result replaces the body's solid under
  that key (an empty result: "shelling Body 1 leaves nothing of it").
  Its refusals (`varde_kernel_compat::ShellError`) are worded by
  `message::shell_refused`: a round shrinking to nothing "the shell is
  thicker than the smallest round of Body 1: try a thinner wall" (the
  face drawn), the offsets crossing "the shell is too thick for Body 1:
  its walls would run into each other", a corner of more than three
  faces "faces of Body 1 meeting at a corner can't be offset together:
  try another thickness" (the vertex drawn); its failures as a
  boolean's ("shelling Body 1 is too complex to work out" for
  `TooComplex`, "shelling Body 1 isn't supported yet" for
  `NotImplemented`, `message::shelling`), the evidence's faces on the body.
- **Kernel stand-in**: `varde_kernel_compat::shell` (`kernel/src/shell.rs`,
  with `ShellError`) isn't built yet: it has its planned signature and
  fails with `NotImplemented`. So every shell that finds its faces fails
  today with "shelling Body 1 isn't supported yet", its body left
  whole; the rest of the history goes on. The regen tests swap it
  (`testing::Overrides::shell`, per thread, the stand-in in `history/shell/stand_in.rs`; other crates' tests through the
  `testing` feature, `varde_regen::testing::shell_by_boxes`) for
  `by_boxes`: a box along the world's axes (six planar faces square to
  them, its volume its box's) hollowed by one boolean with another box
  (inward: less its box shrunk by the thickness and pushed out past the
  open faces; outward: its box grown but at the open faces, less its
  box pushed out past them), too thick where the inner box would
  reach no way into the body (walls meeting, or a floor under an open
  face as thick as the body is across: never the body left whole as
  if shelled), too complex otherwise; and for recording and refusing
  stand-ins. The planned analytic tests of the kernel's shell are
  written out and `#[ignore = "kernel shell not built"]`.
- The draft's reply carries nothing new.

## UI

The move's session (`MotionSession`, above) with `MotionKind::Shell`;
**the face session**, picking faces of one body, is its own part
(`app/src/doc/motion/faces.rs`; `view/src/motion/faces.rs`,
`PickedFaces` and its field), its faces a `Refs<FaceRef>` picked as the
edge session's edges are (`refs.rs`), shared by every kind that
`MotionKind::picks_faces()` (an offset face's and a draft's take it
with their own values: each adds itself to
`picks_faces`, `faces::limit` and `verb`, `MotionSession::faces_need`
and `prune_faces`; `faces::takes_body` is the shell's alone, so theirs
start with no body and have only their faces'); the shell's own parts are in
`app/src/doc/motion/shell.rs` and `view/src/motion/shell.rs`
(`ShellView`, `ShellDirection`). The panel is the model mock's shell
panel.

- **Starting**: `Look::StartShell` from the rail's Modify set (after
  Chamfer, as the icon mock orders it), again backing out; no key, as
  the mocks have none. **Not on the toolbar**: the model mock's bar has
  Shell after Chamfer, but with it the bar runs past its room at 1280
  px wide (to 1199 px of 1160), so it waits for the toolbar's overflow
  handling. Or editing one (`Look::EditFeature`: double-click, `Enter`,
  "Edit shell"), which opens with its body, faces, thickness and
  direction. A new one takes the faces selected in the model shown that
  a click would take (as the mock's takes the face selected), the first
  one's body deciding; else its body is the body selected, or the
  model's only one. Nothing takes the focus: clicks pick faces.
- **The body**: the faces' body while there are faces; with none, the
  body picked: as it starts (above), by a row in Objects
  (`Doc::motion_body`, only while no face is picked; with faces another
  body's row says "A shell's faces are all on one body: take them out to
  pick another"), or the body of the first face clicked. A body picked
  that a join or combine before the shell merges (a redo bringing it
  back) moves on to its holder, as the faces do. Taking every face out leaves the body, a
  closed shell of it. With no body (a new one in a model of several,
  nothing selected) the status bar says "pick faces to remove, or the
  body to hollow". The panel has no Body row, as the mock's.
- **Faces** (`MotionPick::Faces`, the viewport picking faces only): a
  click on a face picks it, named as of the feature
  (`Naming::checked_face_ref`) on the body holding it there
  (`Merges::holder`), made before it; a click on a face picked (found
  again on the model shown by its names, `PickIndex::find_face`) takes
  it out. Once one is picked, the others must be on its body: another
  body's faces don't light under the cursor and a click says "A shell's
  faces are all on one body: pick faces of Body 1"; an edge says "Only
  a face can be removed". At most `MAX_SHELL_FACES`. Kept in
  `FaceRef::order`, so the list is sorted as stored. Picked faces are
  lit as selected, the one under the cursor as hovered; a row hovered
  in the panel lights its face (`PanelHover::Face`). Picks wait only for
  a model of the document as it is with a draft of this session's run,
  or for a new shell none (`Doc::refs_model_current`, the edge
  session's rule), so faces can be clicked one after another. Faces the
  shell itself makes (on its preview, the hollow's) are refused by
  their names, as made later. The Remove field clicked turns picking off
  (`MotionPick::Nothing`) and on.
- **The rows**: "Face 2" by the face's place in the list (as
  regeneration's messages count them, "its open face 2 of 3 wasn't
  found"), the mock's `se-face` icon (`Icon::SeFace`); beside it what
  kind of face it is on the model shown where it's found there
  (`face_kind`: "Planar face", "Cylindrical face"); a cross takes it out
  (`MotionLook::DropFace`). The mock names faces ("Upright top") with
  their area beside them; numbering by place was taken here, as for the
  chamfer's edges, and the kind rather than the area, which the picking
  tables have at hand (an area is the regeneration lane's to measure).
  On a preview that opens it, what's left of a face removed (the walls'
  ends, which keep its name) is found and lit as it, and a click there
  takes the face out: it's the face the shell names, as regeneration
  finds it on the body before the shell, so lighting it is kept (the
  chamfer's cut-off edges, which leave nothing, can't be clicked out).
  Left in pieces (the top opened with two opposite sides is two
  strips), every piece on the shell's own preview lights as the face
  and a click on any takes it out (`Ref::grown`,
  `PickIndex::faces_keyed_as`), rather than the one found nearest
  lighting and a click on another naming the face again. A face already
  in pieces before the shell, one of them removed, lights there with
  its other pieces too (the model shown can't tell them apart).
- **Thickness** (`MotionField::Thickness`, read by
  `Shell::thickness_ask`: "2" of the design's units to begin with, the
  mock's), and **Direction**: two tiles (`MotionLook::ShellDirection`,
  the mock's icons `sh-in` and `sh-out`: `Icon::ShIn`, `ShOut`), Inward
  to begin with.
- **Whole and ready**: a body and the thickness; `Shell::check_own`
  refuses as the panel's foot ("Shell fails"). No faces is whole (a
  closed hollow body), with the mock's warning in the foot, "No faces
  removed: the body becomes closed and hollow", where no failure is
  shown. Faces the document no longer takes at the feature's place
  (`Document::check_shell_faces`: an undo took a face's maker away) are
  kept and said to be gone ("A picked face is gone", the mock's; with no
  faces, the body gone is "A picked body is gone"), nothing previewed or
  committed until taken out or a redo brings them back. The mock's "Too
  thick" under the field (from the mock's own walls) is regeneration's
  here: the kernel's refusal, "the shell is too thick for Body 1: its
  walls would run into each other", in the foot.
- **Preview**: the shell as set up is the draft while it's whole,
  picking or not (a new one with its body found starts previewed,
  closed). An edited one with nothing it names there is previewed as a
  move of nothing of its body. **The kernel's shell isn't built**, so
  today every preview fails with "Shell fails" over "shelling Body 1 isn't
  supported yet", the body shown whole (its faces there to
  pick), OK waits, and Add anyway keeps it, failing in the Timeline.
  Regeneration's own refusals show in the panel the same way.
- **Committing**: OK (`Enter`, Add anyway) adds "Shell N" or sets the
  edited one, one undo step; Cancel or `Esc` leaves no trace. The status
  bar says the mock's row info once whole ("2 faces removed · 2 mm
  inward", "Closed · 1 mm outward", `shell_info`), else what's next,
  with the hint "Pick faces to remove" (the mock's; mouse hints show
  only while the bar has nothing else to say).

The Timeline shows the model mock's shell icon (`Icon::Shell`) and note
(`view/src/shell.rs`: the thickness, "2 mm"); selected, the status bar
says `shell_info`.

Departures from the mock: no Shell on the toolbar (above); faces named
by their place with their kind; no bands drawn over the faces (the
mock's preview draws each face's wall as a band; here the preview is
the model regenerated with the shell). Known gaps: the rows' kinds are
the model shown's (an edited shell's preview holds the features after
it); with no body picked nothing in the panel says so (the status bar
does).

Tests: `document/src/shell/tests.rs` (added and undone, a closed one,
edited, its own parts, bodies and makers, removal following the body
and not the faces, units pinned, round trip, wrong shells refused when
read, the twelfth kind, errors), `regen/src/history/tests/shell.rs` (the
stub failing as too complex with the history going on; an open face
gone after its maker is removed, named by its place among others; an
open face found again after an upstream dimension change, opened where
it went; with the box stand-in: closed, open at one and two faces and
outward by their volumes, too thick refused and just under it, the
cache; recording: a face named twice handed over once, the thickness
and direction as stored; refusals worded and drawn; a consumed body;
the stand-in against the volumes it should give for sets of a box's
faces opened (six that give each axis each of its states; all 64 under
`VARDE_TESTS=full`), inward and outward, at thicknesses about half a side
and past one; `shell/fuzz.rs`, random histories with shells,
`VARDE_TESTS=full`: each that works was of a box and has the volume
its open sides, thickness and direction give, each failing changes
nothing, warm and cold caches alike, bytes and wire;
ignored: the kernel's on a box open, closed and outward, a slot-shaped
plate's round ends offset exactly, a boss on a plate open underneath,
too thick, determinism), `io/src/vrdp/tests.rs` (through a file, a
tampered face point and thickness refused, a record's shells damaged
2 000 ways refused or checked, faces out of order, repeated, on another
body, past the limit, named by the shell or later, a body made later
and a thickness refused as read), `view/src/shell/tests.rs`
(the notes), `view/src/motion/tests.rs` (the shell's panel: its order,
the rows' kinds beside them, the place to click, the closed warning in
the foot, the status text), `rail/tests.rs` (Shell in the Modify set)
and `app/src/doc/motion/tests/shell.rs` (the rail's Shell on the
example plate: the only body taken, closed and previewed with the
warning, an edge refused, faces hovered and lit, picked and listed
sorted with their kinds, clicked again taken out, the row hovered
lighting its face, the stand-in's not-built failure in the panel, OK
waiting and Add anyway keeping it as one undo step; with the boxes, the
closed hollow and the top opened previewed (the walls' ends lit as the
face removed, the hollow's faces refused), another face opened and its
row's cross, another thickness, Outward grown but at the open face, too
thick refused, a thickness of nothing refused, OK and undo; another
body's faces neither lit nor taken, a body's row picking the body only
while no face is, the face taken out leaving a closed shell; editing
from the Timeline, Cancel, another thickness and Outward, undo, its
faces all taken out drafting a closed one; a face whose maker an undo
takes away said to be gone, back on redo; the face selected taken in,
Shell again backing out; faces selected on two bodies, the first one's
body taking them; a face and a body picked following their body a
redone combine merges; the toolbar fitting at 1280 px in a shell
session; the top left in two strips by its opposite sides opened, both
lit and either clicked taking it out; 256 faces, the next refused
with why; `shell/fuzz.rs`, random shell sessions, `VARDE_TESTS=full`,
each shell the model shows working holding to its volume). The app's
tests shell by boxes through
regen's `testing` feature (`varde_regen::testing::shell_by_boxes`).
