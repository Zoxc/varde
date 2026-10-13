# Split

`crates/document/src/split.rs`.

```rust
pub struct Split {
    pub body: BodyId,                // made by a feature before it
    pub tool: SplitTool,
    #[serde(default)] pub original: Side,           // Front | Back: which piece keeps the id
    #[serde(default)] pub keep: Keep,               // Both | Front | Back
    #[serde(default)] pub new_body: Option<BodyId>, // Some when keep is Both; held while one side is
}
pub enum SplitTool {
    Plane(Plane),                                         // an origin plane or a flat face's
    Face(FaceRef),                                        // a face's surface, continued past the body
    Body(BodyId),                                         // another body, kept
    Regions { sketch: FeatureId, regions: Vec<RegionRef> },  // through the body both ways
    Chain { sketch: FeatureId, curves: Vec<Id> },          // one open chain, 1..=MAX_SPLIT_CURVES (256), sorted
}
```

- **What it is**: the tenth variant (`FeatureKind::Split(Split)`,
  "Split N", not boxed). The **front** is the part of the body inside
  the tool: on the side a plane's normal points to (out of a face's
  body), inside the closed solid a face's surface bounds, inside the
  tool body or the sketch's regions, left of the chain (which runs as
  its lowest curve by id does); the **back** is the rest. With `keep`
  `Both`, the piece `original` names keeps the body's id (so every later
  feature naming the body gets it) and the other becomes `new_body`, a
  body the split makes ("Body N", `created_by` the split). Keeping one
  side is a trim: that side keeps the id whatever `original` says
  (`Split::kept`), no new body. A side of several pieces is one body of
  several shells.
- **The new body** goes through the commands as an extrude's
  `NewBody` does: `AddFeature` makes one for a split keeping both
  (whatever `new_body` held: `planned_new_body` sets it to
  `BodyId::NEW` or `None` from `keep` first, so a panel needn't keep the
  two in step), `SetFeature` keeps it while both are kept (switching
  `original` keeps the same body, now the other piece), removes it when
  one side is kept (a later feature naming it stays, failing, as with
  an extrude's new body) but keeps its id in `new_body`
  (`Split::held_body`; `Split::made_body` is the body it makes, only
  while both are kept), and when both are kept again, in the same edit
  or a later one, brings the body back with that id, so what named it
  (a sketch on its face) finds it again. A split that never kept both
  holds none and gets a new id when it first does; so does every split
  written before ids were held (a trim with `new_body` none).
- **Checks** (`CheckError::Split(id, SplitError)`): `Split::check_own()`
  (cheap): the new body there when both are kept (`NoNewBody`), a tool body not the body (`ToolIsBody`), a face's
  point in bounds (`Face(PlaneError)`), 1..=256 regions each checked
  (`Regions`, `Region`), 1..=256 curves sorted without repeats
  (`Curves`, `CurveOrder`). `Document::check` then wants the body made
  before (`Body`), a tool body there and made before (`ToolBody`), a
  face tool's body there and made before (`FaceBody`: depended on), a
  plane face's body and any face's maker as a mirror's plane's
  (`RefBody`, `RefMaker`: one not there is allowed with ids no later
  body or feature can take), a sketch tool's sketch a sketch before it
  (`Sketch`), and the new body, while both are kept, a body it makes
  (`NewBody`); a held id is checked as every feature's
  (`CheckError::Held`, above). On add and
  set only (`check_new`): a chain's curves are curves of its sketch
  (`Split::check_curves`, `Curve`); a later edit deleting one makes
  regeneration fail it, as a revolve's axis line.
- **Dependencies**: `FeatureKind::bodies()` is the body, a tool body and
  a face tool's body (sorted, no repeats), so removing any of them (or
  its maker) removes the split, and with it its new body and whatever
  names that; removing the new body removes the split (its maker). A
  plane face's body isn't listed (the split stays and fails, as a
  mirror's plane). `uses()` and `sketch()` give a sketch tool's sketch,
  which adding the split hides, as an extrude does. A held id is no
  body: it's in no body list (`Document::bodies`, so not in Objects),
  `RemoveBody` of it removes nothing, and it goes with its split; a
  reference to it (a sketch on its face) passes the check as one to a
  body that isn't there with an id below the next id does, and fails
  in regeneration until the body is back.
- `SetUnits`: nothing to pin.

## Regeneration

`crates/regen/src/history/split.rs`, in history order:

- The body needs a solid of its own, and so does a tool body
  (`own_solids`, as a combine's: consumed or failed fails the split).
- **The tool**, from the body's box (`Solid::bounds3`), cached as an
  `Entry::Solid` by the feature, the box's bits and the fit tolerance
  and, by kind: an origin plane, `varde_kernel_compat::half_space(normal, 0)`;
  a flat face (found on its body's holder by key and point, as a
  mirror's plane: "its plane face's body is gone", "... wasn't found",
  "... isn't flat" with the face drawn), `half_space(n, d)` of its form,
  keyed by the holder's key and the reference; a face,
  `varde_kernel_compat::surface_tool(form, on, bounds, ..)` with its region's
  form and a point inside it (the middle of its first patch, picking a
  cone's nappe; "its face's body is gone", "its face wasn't found";
  `ToolError::CantExtend` is "its face can't be extended to split with",
  the face drawn); a tool body, its solid and key as they are; a
  sketch's regions, found and merged as an extrude's and extruded by
  `varde_kernel_compat::extrude` over the body's span along the sketch's
  normal with through all's margin (`through_all` of the body alone),
  its walls `Side { curve, .. }` of the split, keyed by the regions, the
  span's bits, the sketch's key and placement; a chain, ordered by
  `profile::chain` then `varde_kernel_compat::chain_tool(segments, frame,
  bounds, RIM, ..)` (the rectangle round the shadow named as profile
  curve `RIM = 1 << 32`, past every sketch curve id), keyed by the
  curves, the sketch's key and placement. A sketch not placed is "its
  sketch isn't placed". The kernel's tool errors are worded by
  `message::split_tool`: too complex "extending its tool past Body 1 is
  too complex to work out", not built "its tool past Body 1 can't be
  made: a half-space split tool isn't implemented yet", out of bounds "Body 1 is too near the edge
  of the space to split", a profile touching itself "the line doesn't
  split Body 1: it crosses itself once extended".
- **The chain** (`profile::chain(sketch, curves, join, fit)`): each curve
  whole with two ends (a line, an arc, an open spline; a circle or
  closed spline is `Closed`), ends joined two at a time where they're
  one point or within the resolution, into one chain with two free ends
  (three ends at a point or pieces apart: `Branches`; a loop:
  `Closed`; a curve gone: `Missing`), run the way its lowest curve runs;
  each curve's conics made as a region's pieces are (lines, arcs of at
  most 90° from `trig::angle`, splines fitted), from where the one
  before ended, so the conics meet to the bit. Messages: "its line's
  curves weren't found", "its line is closed: split with the region it
  encloses instead", "its line's curves don't join end to end into one
  line".
- **The split**: `varde_kernel_compat::split(body, tool, tol, budget)` →
  `(front, back)`, cached as `Entry::Split` (both pieces, or the
  `KernelFailure`) by the body's key and the tool's; its failure is
  worded as a boolean's, "splitting Body 1 ...", the evidence's faces on
  the body and a tool body. A side empty fails it: "Body 1 lies all on
  one side: the tool doesn't cut it in two" (no body is ever empty).
- **The pieces**: the one `Split::kept` names replaces the body's solid
  (keyed `split piece` of the split's key and the side); the other, with
  both kept, is pushed as the new body's (after the bodies made before:
  the split makes it), and `(body, new_body)` noted in
  `Evaluation::splits` (in the document's order; not on the wire).
- **References after a split** (decided as proposed): later features
  naming the body get the piece that kept its id, and their face and
  edge references resolve on it only: one that went to the other piece
  fails as not found (a mirror's face: "its mirror face wasn't found"),
  to be picked again. **A sketch on a face** is the exception, as for
  merged bodies: `place_on_face` looks for the face on the holder, and
  where it isn't found, on the holders of the new bodies of splits of
  it (in `Evaluation::splits`' order), and on those of splits of those,
  and the sketch is placed on the first it's found on ("not found" is
  told by `Failed::missing`, which `place_on` sets for that alone, as a
  link's by `Broken`'s, never by the words). The search
  goes back through splits too: a face named on a split's new body that
  isn't found there is looked for on the body split (and on through
  that body's splits), so a sketch on a face of the new body follows it
  when `original` is switched and the face goes to the body. Switching
  `original` swaps which body later features work on.
- **Kernel stand-in**: the kernel's `split`, `half_space`,
  `surface_tool` and `chain_tool` (`kernel/src/boolean/split.rs`) are
  not built yet: each has its planned signature and fails with
  `NotImplemented`. So every split fails today with the not-built
  message (a plane's, a face's or a line's tool first, as "its tool past
  Body 1 can't be made: a half-space split tool isn't implemented yet";
  a tool body or a sketch's regions, whose tools the kernel already
  builds, as "splitting Body 1 isn't supported yet"),
  its body is left whole and its new body has no solid (a feature naming
  it fails: "Body 3 has no solid: the feature making it failed"); the
  rest of the history goes on. The regen tests swap the kernel's split
  for two booleans (`a ∩ t`, `a − t`; `testing::Overrides::split`, per thread, the stand-in in `testing/stand_ins/split.rs`
  for tests, other crates' through the `testing` feature) to test what's done with the pieces; the planned
  analytic tests of the kernel's split are written out and
  `#[ignore = "kernel split not built"]`.
- The draft's reply carries nothing new: a split draft applied by
  `AddFeature` makes its new body as the command does, drawn as its own
  part (the app can tell its id by applying the same command to its
  copy of the document).

## UI

The move's session (`MotionSession`, above) with `MotionKind::Split`; a
split's own parts are in `app/src/doc/motion/split.rs` (`SplitSetup`)
and `view/src/motion/split.rs` (`SplitView`, `SplitMode`). **The UI mock
has no Split panel**: the icon mock has only the tool, "Split body" in
its Modify group (after Combine) with no key and its `splitbody` icon.
The panel is built in the style of the mock's nearest ones, Combine's
(one body as a picked field, choices as tiles) and Mirror's (a plane or
face picked, the origin planes on the toolbar).

- **Starting**: `Look::StartMotion(MotionKind::Split)` from the rail's Modify set ("Split
  body", after Combine as the icon mock orders them; no key:
  `motion_binding`, no `Shortcut`, the rail's list letter `P`), again
  backing out; not on the toolbar (the mock's bar has none). Or editing
  one (`Look::EditFeature`: double-click, `Enter`, "Edit split"), which
  opens with its body, tool and options, picking nothing. A new one
  takes its body from what's selected, else the model's only body, as
  an align's (one body); with one it picks the tool next, else the body.
  Nothing takes the focus: clicks pick in the viewport.
- **The body** (the Body field, "Click a body") is picked as an align's:
  a click on a body replaces it (`Doc::split_body`; the tool body if
  it's that is let go of), clicks going on to the tool if there's none.
  A body's click is named as the feature names it (`Doc::named_body`): a
  join's merged body as its holder before the feature, and a piece an
  edited split (or one after the feature) made as the body it split.
- **Split with** (`MotionPick::Tool`): four tiles, `MotionLook::SplitWith`
  (icons `SePlane`, `Body`, `SeRegion`, `Line`): **Face**, a plane or
  face (the toolbar offers "XY plane", "XZ plane", "YZ plane" while it's
  picked, `MotionLook::OriginPlane`; a face clicked is named as of the
  feature, `Naming::checked_face_ref`, and is `SplitTool::Plane` of the
  face where its summary is a plane, else `SplitTool::Face`, its surface
  continued); **Body**, another body clicked (any made before the split
  but the one split: "That's the body being split: pick another body to
  split with"; the body hovered lights whole); **Region**, regions of a
  sketch, picked as an extrude's (`RegionPick`, at most
  `MAX_EXTRUDE_REGIONS`): the visible sketches' regions shaded on their
  planes until one is picked, then the source's, those picked filled
  (`viewport/regions.rs`), a click on one picking or un-picking it
  (`MotionLook::SplitRegion`); **Line**, the curves of an open line:
  the visible sketches' curves drawn (construction ones dashed), the
  line's sketch's only once a curve is picked, a click within 6 px of
  one (`hit::hit_curve` on each sketch's plane, the nearest by depth)
  picking or un-picking it (`MotionLook::SplitCurve`, sorted, at most
  `MAX_SPLIT_CURVES`, of one sketch), those picked in the selected
  colour. Only sketches before the feature are offered. While regions
  or curves are picked the model isn't (`MotionSession::picks_sketches`:
  no `ModelPicking`), and the left button goes to them, off them to the
  camera. The region or curve under a still cursor is worked out again
  as a frame is drawn (`Moving::redraw`: the camera moved) and let go
  of while the camera's dragged. Each tile keeps its own tool while
  another is shown. A face,
  plane or body picked hands the clicks to nothing (the preview shows);
  regions and curves keep picking until the field is clicked again. The
  tool's row: "XY plane", "Extrude 1's end", "Body 2", or the sketch's
  name with "2 regions" or "3 curves" beside it; no cross (picking
  another replaces it, a region or curve is clicked again to take it
  out). Refusals in the status bar, as a move's ("Only a face made
  before the split can be picked", "Which body that face is on at the
  split can't be told: pick another", an out of date model). Which faces
  regeneration can't continue (`Summary::Other` holds both those it can,
  spline walls and quadrics, and those it can't, canal fillets and
  traced blends) isn't told here: such a face is taken and the preview
  fails with regeneration's "its face can't be extended to split with".
- **Keeps Body 1** (the body's name): tiles Front and Back
  (`MotionLook::Original`, icons `SpFront`, `SpBack`, not in the mock),
  which piece keeps the body's id; for a trim they're disabled, showing
  the side kept, which keeps it. Under them, in the warning's colour,
  **the later features' warning**: for an edited split, the features
  after it naming the body (`FeatureKind::bodies`), "2 later features
  use Body 1: they'll get the back piece" ("1 later feature uses Body 1:
  it'll get the front piece"), following Keeps and Keep as they change.
- **Keep**: tiles Both, Front, Back (`MotionLook::Keep`, `SpBoth`,
  `SpFront`, `SpBack`). Keeping one side of an edited split whose new
  body a later feature names is allowed: that feature stays, failing
  ("a body it names isn't there"). Keeping both again
  (in a later edit too) brings the new body back with the id the split
  held, the commands filling it in: the panel sends `BodyId::NEW` or
  none, a new split getting a new id; the preview labels it "New body"
  until committed, as a body the document doesn't hold.
- **Whole and ready**: a body and the tool of the tile shown
  (`MotionSession::split`, its new body `BodyId::NEW` while both are
  kept, which the commands fill in or keep), else what's next for the
  status bar ("pick the body to split", "pick a plane or a face to split
  with", "pick a body to split with", "pick the regions of a sketch to
  split with", "pick the curves of a line to split with");
  `Split::check_own` refuses as the panel's foot. A tool the document no
  longer takes at the feature's place (`Document::check_split_tool`, its
  body not held, its sketch gone, a curve of its line gone) is kept and
  said to be gone ("The plane or face is gone: pick another", "The tool
  body is gone: pick another", "The regions' sketch is gone: pick other
  regions", "The line is gone: pick its curves again"), nothing
  previewed or committed until another is picked or a redo brings it
  back (`SplitSetup::gone`, worked out again as the document changes
  and as the tiles are switched, each tile's own). While a line is
  gone, every visible sketch's curves are offered again, and a click
  picks afresh, keeping the line's curves still in its sketch. Regions
  whose sketch is gone are put by (`SplitSetup::stale_regions`) and the
  visible sketches' regions offered in their place; the regions come
  back if their sketch does before others are picked. A tool body
  merged into another before the split follows it; one merged with the
  body split (into it, or it into the tool body) is let go of, to be
  picked again, as a body can't split itself.
- **Preview**: the split as set up is the draft; none while a face or
  body is picked as the tool (the model shown is then the document's: a
  new split's is the history as of it, an edited one's with the split as
  stored, its pieces named as the body split, below), nor while it isn't
  whole or its tool is gone. Regions and curves are previewed as they're
  picked. An origin plane is drawn as a mirror's (`MotionState::line`);
  a face's plane isn't (regeneration doesn't answer where it found it).
  **The kernel's split isn't built**, so every preview fails today with
  regeneration's not-built message ("Split fails" over "its tool past
  Body 1 can't be made: a half-space split tool isn't implemented yet",
  or "splitting Body 1 isn't supported yet" for a tool body or
  regions), OK waits,
  and Add anyway keeps it, failing in the Timeline. Once the preview
  splits, **the pieces are tinted apart and labelled**: the body's faces
  as selected, the other piece's in the second colour
  (`Doc::split_lit`), and a chip at the middle of each piece's box with
  its body's name, the one keeping the id in the accent, the other "New
  body" for a body the document doesn't hold yet (`SplitPiece`,
  `Moving::labels`, in the labels' layer). The new piece is the part of
  the model shown whose body the document doesn't hold, or an edited
  split's stored new body.
- **Committing**: OK (`Enter`, Add anyway) adds "Split N" (and its new
  body, "Body N") or sets the edited one, one undo step; Cancel or `Esc`
  leaves no trace. The status bar says "New split · Body 1 by XY" once
  whole (`split_info`), else what's next, with the hints "Pick the body",
  "Pick the plane or face", "Pick the tool body", "Pick regions", "Pick
  curves".

**Naming after a split** (`Naming::before`, `Naming::unsplit`): the new
body of each split at or after the feature named is noted with the body
it split, and `Naming::body_of` takes a face shown on such a piece as on
that body (through splits of splits), where it is at the feature: so a
face of an edited split's new piece (or of a later split's) names the
body split, not a body made after the feature, which was refused. A
body a later join or combine merged into the new piece still names its
own faces: `body_of` takes a body held by the body shown or by the body
it was split from. The status bar's refusals are the shared
`unnamed(.., MotionKind::Split)`, the naming `Doc::naming_in`.

The Timeline shows its icon (the icon mock's split body, `Icon::Split`,
in the Modify set) and note: "by XY", "by Body 3", "by Extrude 1's end",
"by Sketch 2", with " · front only" or " · back only" for a trim; the
status bar "Body 1 by XY".

Departures from the mock (which has no Split panel): Combine's and
Mirror's style with this session's own fields; the rail only, not the
toolbar; refusals in the status bar, as a move's. Known gaps: a face
that can't be extended isn't told apart while picking (above); a split's
plane on a face, and a face's surface, aren't drawn; a region or curve
of a sketch on a face that isn't placed can't be picked; the line's
curves aren't checked to join into one open line until regeneration
says so.

Tests: `document/src/split/tests.rs` (a new body made and undone, a
trim making none, editing what's kept adding and removing the new body,
holding its id and bringing it back with it, and allowed while a scale
names it; a sketch on the new body's face across a trim and both again,
with undo and redo and features added meanwhile getting ids of their
own; held ids checked when read, sketch tools hiding and using their
sketch, every check, removal along the bodies named, units, postcard
round trip, wrong splits refused when read, the tenth kind),
`regen/src/history/tests/split.rs` (the stand-in failing as too complex
with the history going on, every tool reaching the kernel, regen's
refusals before it, the chain joined end to end, a line of 256 curves
joined at once, an arc run against its way round, ends joined within
the resolution only; with the booleans: a sketch on the new body's
face found again once both are kept again in a later edit, and
following its face back to the body when the pieces swap; the
pieces to the body and the new body with `original` and `keep`, a
sketch region through all, a side empty, a sketch on a face following
into the new body while a mirror's face doesn't, and on through a
merge and a second split, a draft, the cache;
ignored: a box by XY, by a cylinder face, an L by its own step's plane,
by another body, by an open line, determinism),
`regen/src/history/tests/split/fuzz.rs` (random histories of blocks,
joins, cuts, combines, sketches on faces and splits by every tool,
earlier splits edited, removals, undo and redo: each split that works
gives its body and new body the booleans' pieces as `original` and
`keep` say and leaves the rest alone, one that fails changes nothing,
each sketch on a face placed on that face where it followed it, the
cache warm and cold alike, later edits possible, flipped bytes;
`VARDE_TESTS=full` runs more),
`io/src/vrdp/tests.rs` (through a file, a tampered face point refused,
held ids through a file, older records with none read, held ids changed
on disk refused,
every tool's split damaged on disk refused or checked),
`view/src/motion/tests.rs` (the notes; the panel's order, the tool's
count beside it, where to click, the later features' warning, the
status text; a trim's kept side), `viewport/motion/tests.rs` (a line's
curve and a region picked from the top in their sketch, off them left
to the camera; the pieces' labels), `rail/tests.rs` (Split body's
letter) and `app/src/doc/motion/tests/split.rs` (the rail's Split body
on the example plate by XY from the toolbar, failing as not built in
the panel, OK waiting and Add anyway keeping it as one undo step; a
flat face as its plane and the hole's wall as its surface, unpreviewed
while picked; with the booleans, another body as the tool, the body
itself refused, the pieces tinted and labelled, Back, a trim committed;
a sketch's region through all and an open line; editing from the
Timeline, Cancel, Back and OK, undo; a sketch on the new body's face,
the split kept to the front and OK, then both and OK: the body back
with its id and the sketch placed; the later features' warning and a
named new body held; a tool body an undo takes away said to be gone,
back on redo; an edited split's new piece's face named on the body
split, and a click on it picking that body; a tool body merged with the
body split let go of; a line or regions whose sketch is gone picked
again from any sketch, the regions back on undo; each tile's tool told
gone on its own) and `app/src/doc/motion/tests/split/fuzz.rs` (a
split session fuzz: tiles switched, picks on the model shown and on one
gone by, regions and curves, Keeps and Keep, undo and redo, merges,
removals, commits and edits held to what they set up (an edited
split keeping its new body's id, held or made), and each split
working at the end cutting its body's volume in two; `VARDE_TESTS=full`
runs more). The app's tests split by
two booleans through regen's `testing` feature
(`varde_regen::testing::split_by_booleans`, the thread local the regen
tests use, behind a feature for other crates' tests), as the kernel's
split isn't built.
