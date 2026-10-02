# Features

The features after sketches and extrudes (revolves, combines), and sketches' planes on faces:
their document types, checks and commands, how regeneration evaluates them, and their UI. Extrudes are
described in `agents/kernel.md` ("The extrude feature" and "The extrude UI"); what
they share with the newer kinds is here. The kernel math of each is in
`agents/kernel.md`.

## Commands shared by every kind

`crates/document/src/editor.rs`.

- `Command::AddFeature { name, kind: Box<FeatureKind> }`, made by
  `Document::add_feature(kind)`, which names it one past the highest
  "Extrude N", "Revolve N" ... (`FeatureKind::noun`). It hides the sketch
  whose regions the feature takes (`FeatureKind::sketch`); one whose
  operation is `NewBody` also adds "Body N" with the next id, replacing
  whatever id the command held (`BodyId::NEW`). One undo step. A sketch
  kind is refused (`EditError::SketchKind`): sketches are added empty by
  `AddSketch` and set by `SetSketch`.
- `Command::SetFeature { feature, kind }` replaces a feature's kind,
  keeping its id, name and visibility. A missing feature is a no-op; a
  sketch feature, or a sketch kind, is refused (`EditError::SketchKind`).
  The kind may change (an extrude may become a revolve): what matters is
  the operation. A `NewBody` that stays one keeps its body (whatever id
  the command held); one that stops removes the body and drops it from
  the other features' excluded lists; one that starts adds one. The
  caller passes regions referenced afresh from the sketch as it is.
  Setting what's already there changes nothing (no new revision).
- Both run the document's whole check on the result, and then
  `check_new` on the added or set feature: what's required of a feature
  when the user makes or edits it, but not of one already in a document
  (a later edit of its sketch may break it, which regeneration reports).
  Today that's only a revolve's axis line.
- `FeatureKind` helpers: `noun`, `sketch` (the profile sketch),
  `operation` / `new_body` (an extrude's or revolve's `Operation`; a
  body's maker is checked by `new_body`), and `uses`, now a **list**
  (sorted, no repeats) of the features this one builds on, which
  `Document::removal` follows. Every extrude-only path that only cared
  about the operation (`drop_excluded`, the app's delete prompt and its
  join merges) goes through `operation()` so revolves get them too.
- `SetUnits` pins revolve angles by `Turn::ask` as it pins extrude
  distances by `Extent::ask` (angles' bare numbers are degrees whatever
  the units, so only lengths inside an angle's expression change).
- Kinds are **appended** to `FeatureKind` (`Sketch` 0, `Extrude` 1,
  `Revolve` 2, `Combine` 3): files store a kind by its variant name, and
  the variant index is what the workers' postcard holds.

## Sketch planes on faces

`crates/document/src/plane.rs`.

```rust
pub enum Plane { Origin(OriginPlane), Face(FaceRef) }      // Face appended
pub struct FaceRef { pub body: BodyId, pub key: FaceKey, pub near: DVec3 }
```

- **The reference**: the body, the face's key (the kernel's `FaceKey`,
  re-exported with `PartKey`; its `feature` is the number of the feature
  that made the face, `FaceRef::maker`) and the picked point, which
  chooses among several regions with the key, as the kernel's
  `Topology::face` resolves it. The document stores the reference, never
  a placement.
- `Plane::placement() -> Option<Placement>`: an origin plane's; `None`
  for a face, whose placement only regeneration finds. `Plane::name()`
  is "XY" ... or "a face"; `Plane::face()` the reference.
- **The placement rule**, `Placement::on_plane(n, d) -> Option<Placement>`
  for a flat face's form `n·p = d` with `n` out of the solid, one pure
  function regen and the app both call on the same bits:
  - normal `n̂ = n / |n|` (seen from outside; an extrude's positive side
    grows the body), origin `n̂ d / |n|` (the plane's point nearest the
    world origin, so thickening a plate moves the drawing only along the
    normal and a face's size moves nothing);
  - horizontal (`n̂x² + n̂y² ≤ 1e-18`, `HORIZONTAL`, a stated decision):
    `x` is world X less its part along `n̂`, normalized, `y = n̂ × x`;
    otherwise `y` is world Z less its part along `n̂` (its z as
    `n̂x² + n̂y²`, which doesn't cancel), normalized, `x = y × n̂` ("up
    stays up", what the Z-up camera shows facing the plane); the stored
    `normal` is `x × y`;
  - signed zeros made positive (`+ 0.0`), so a face parallel to an origin
    plane and facing its way gets that plane's axes to the bit (whatever
    `|n|`), and the bottom face `x` = X, `y` = −Y; axes don't depend on
    `d` (a moved face keeps them to the bit);
  - only `+ − × ÷ √` and one comparison; `None` for a zero or non-finite
    `n` (one whose length overflows or whose square underflows), a
    non-finite `d` or result.
- **Checks** (`Document::check`, `CheckError::SketchPlane(id,
  PlaneError)`): the point finite and within `MAX_COORD`
  (`FaceRef::check_own`, `PlaneError::Near`); the body, if it's there,
  made by a feature before the sketch (`Body`); the key's feature, if
  it's there, before the sketch (`Maker`; the sketch itself isn't). A
  body or feature that isn't there is allowed (the reference fails to
  resolve, as a region can), but only with an id below `next_id`: ids
  never come back, so it can't later name something after the sketch.
  One at or past `next_id` (only a file can hold one) is refused, as the
  edit that hands that id out would otherwise be, again and again.
- **Commands**: `AddSketch { name, plane }` takes either kind;
  `SetSketchPlane { feature, plane }` puts a sketch on another plane, one
  undo step, keeping its drawing in its own coordinates. Not a sketch,
  not there, or the plane it's on already: no change (no new revision).
- **No removal cascade**: `FeatureKind::uses` doesn't list the face
  body's maker, so removing that feature or the body (or setting its
  extrude to stop making the body) leaves the sketch, naming a body
  that's gone, for regeneration to fail ("its face's body is gone") until
  it's put on another plane. Removing the sketch still takes what was
  made from it.
- `SetUnits` leaves planes alone (no values in them).

**Regeneration** (`regen/src/history.rs`, `place_on_face`): the history
runs in order, so when it reaches a sketch on a face, the face's body is
as the features before the sketch left it. The sketch is placed:

- the body through `Evaluation::holder(face.body)`: a body a join
  consumed is followed to the body holding it, where its faces live on;
  one with no solid fails, "its face's body is gone" (its maker failed,
  was removed, or no longer makes it);
- the face on that solid's `Topology` by `Topology::face(solid, &key,
  near)` (name or alias, the nearest to `near` among several); none
  fails, "its face wasn't found";
- the region's form (its first triangle's face's, `picking::region_form`,
  the same the picking tables summarize, so the app's pick gives the
  same bits) must be `Form::Plane { n, d }`, else "its face isn't flat";
- `Placement::on_plane(n, d)`, refused unless `Placement::valid` (every
  number finite, axes unit and square within `Placement::SLACK` = 1e-9,
  `normal = x × y` within it, origin within `MAX_COORD` on each axis):
  "its face is too far out to sketch on" (a tilted face near the
  coordinate limit can have its origin past it).

A placed one is listed in `Evaluation::placements` (face sketches only,
in the document's order) and its extrudes and revolves run on that
frame; a failed one is drawn nowhere, its profiles are still worked out,
and every extrude or revolve made from it fails with "its sketch isn't
placed". No last plane is kept.

Limits, seen fuzzing whole histories: a tool built on a tilted face is
flush with it only to rounding (its frame is the face form's normal
renormalized, an ulp or so off the frame the face came from), so a
boss joined or a hole cut flush with a tilted face, or on the end of a
boss built on one, now and then fails in the kernel's boolean ("leaves
no clean solid": 12 of 200 random prisms, each with a wall boss or hole,
a second one on its end and three edits); square faces are exact and
don't. An error, never a wrong solid. And the origin
rule (the plane's point nearest the world origin) can put a far tilted
face's points more than `MAX_COORD` from the origin in the sketch's own
coordinates, where no point can be drawn; the face is placed, but only
its nearer part can be drawn on.

Caches: a sketch's profiles are keyed by the sketch alone (not the
plane: they're 2D, so the same drawing on another plane finds them);
the placement by the face's solid's key, the face's key and `near`'s
bits (`Entry::Placement`, the result or its message), so the topology is
worked out again only when that solid changes. That's all it depends
on: a solid's key holds everything the solid was made from, so the same
key is the same solid to the bit, the same topology, and the same region
for the same name and point (an upstream edit that leaves the solid's
key alone can't move the face, and one that moves it changes the key);
the body's id isn't in it, as the placement doesn't depend on which body
holds the solid; an extrude's or
revolve's tool key adds the placement's twelve numbers' bits
(`Keyer::placement`) to the sketch's key. An edit upstream changes the
body's key, which re-places the sketch, which changes the tool keys
after it: the sketch and what it made follow the face. Drafts need
nothing new: a draft runs the history in order as a commit does, so its
preview shows the sketches on its faces following.

The answer carries `placements: Vec<(FeatureId, Placement)>` (as
`Evaluation::placements`, with a draft that goes as the document with it
applied placed them); on the wire (`Head::Regenerated::placements`, the
four vectors as `[[f64; 3]; 4]`) each is checked `Placement::valid` and
each sketch listed once (`wire::Error::Placement`, the generation
answered as failed). `flatten_sketches(document, placements, exclude)`
draws an origin plane's sketch at the plane and a face sketch at its
placement, skipping one that isn't listed. `regen::Request::Regenerate`
boxes its draft (`Option<Box<Draft>>`): a feature kind is large next to
the rest of the request.

**The app** reads every placement through `Doc::placement(feature)`: an
origin plane's, or a face sketch's from the model shown (`MeshFeed`
keeps the answer's `placements`, given out as `MeshFeed::placement`
only while the model shown is of the document as it is, as the failed
features are, and only for a sketch on the plane it had in the document
that model is of: `MeshFeed` keeps the snapshots it asked about until
their answers can no longer come, at most 64, so after an undo or redo
of a change of plane the old face's placement isn't used while the
answer is on its way). Right after a face is picked, before any answer places
the new sketch, the app works the placement out itself: the face is
found in the model shown (`PickIndex::find_face`, a merged body's face
on the body holding it), its picking summary
must be `Summary::Plane { n, d }`, and `Placement::on_plane(n, d)` on
those bits (`PickIndex::face_placement`) is what regen will get, so
nothing jumps when the answer comes; it's kept as `Doc::placed` (the
sketch, the placement, the generation it was added at) until a model at
least that new shows, and refused if not `Placement::valid` ("That face
is too far out to sketch on"). The sketch session (`SketchSession::placement`,
kept in step by `Doc::follow_placement` after every sync and answer, and
kept as it was while `Doc::placement` knows none), so the viewport's
hit testing, snapping, grid and anchors, Home and the camera facing it,
reads it from there; the extrude and revolve sessions' candidates
(`RegionPick::candidates`, given `Doc::placement`, which their regions,
handle and axis are drawn on) leave out a sketch with none, and
`Doc::extrudable` counts only placed sketches. A sketch on a face that
failed to be placed is still listed in the Timeline, red with its
reason, and isn't drawn; entering it asks for another plane first (Change
plane, `agents/sketch.md`), which `SetSketchPlane` puts it on, a face's
placement worked out at the pick as for a new sketch. Before the first
answer, or read-only, it isn't entered: the status bar says "Can't edit
Sketch 2: <reason>" (`Doc::notice`, until the next thing asked).

`PickIndex::find_face` resolves a `FaceRef` as `Topology::face` does:
among the faces named by the key or an alias, the only one, or the
nearest to `near`, a later one only where it's nearer by more than a
billionth of the model's size, the faces numbered alike (the picking
tables list a body's faces in its topology's region order). It measures
to the drawn triangles (`f32`) rather than the exact patches; for a flat
face, the only kind sketched on, those lie in its plane, so the two
differ only by `f32` rounding, which can change the answer only where
the point picked is within that rounding of two faces with the same
key, and the answer's placement replaces the app's anyway. What does
differ for Change plane is the model: the app finds the face in the
final model, regen at the sketch's place in the history. A face a later
feature removed can't be picked, and one moved by a later feature would
be placed at the pick where it ends up and then where it was; faces of
a body a later join merged into another are named by the body they
were made on (`PlanePick::face_ref`: the body the feature naming the
face made, or else the body the cut, intersect or join naming it touched
(a join only its first, which it merges into), among those merged into
the one shown; `MeshFeed::touched_features` lists the holders as they
were then), which regen follows on to the holder wherever the join is,
so they're found before the join too. Where those are more than one body
at the sketch's place (a hole cut through two bodies a join after the
sketch merged; the merges before it are replayed from the joins that
worked with `note_merge`), which one the face is on can't be told:
Change plane refuses it ("Which body that face is on at Sketch 2 can't
be told: pick another"); a new sketch, at the end, takes it on the
holder.

## Revolve

`crates/document/src/revolve.rs`.

```rust
pub struct Revolve {
    pub sketch: FeatureId,          // an earlier sketch feature
    pub regions: Vec<RegionRef>,    // 1..=MAX_REVOLVE_REGIONS (256)
    pub axis: AxisLine,
    pub extent: Turn,
    pub flip: bool,
    pub operation: Operation,       // the extrude's: NewBody, Join, Cut, Intersect
}
pub enum AxisLine { Curve(Id), SketchX, SketchY }
pub enum Turn { Full, OneSide(Value), Symmetric(Value), TwoSides(Value, Value) }
```

- **Axis**: `Curve(id)` a line of the sketch, construction or not,
  directed from its start point to its end; `SketchX` / `SketchY` the
  sketch's own axes, directed along +x / +y. The sketch's built-in axis
  ids (`Id::X_AXIS`, `Id::Y_AXIS`) aren't curves: a pick of one is stored
  as `SketchX` / `SketchY`, and `Curve` of them is refused. An axis on a
  straight model edge (`AxisLine::Edge`) comes later, appended.
- **Direction**: positive angles turn right-handed about the axis's
  direction, starting from the sketch plane. `flip` turns one side and
  swaps two sides the other way, as an extrude's; full and symmetric
  ignore it.
- **Angles** are `varde_expr::Value`s asked by `Turn::ask`
  (`Ask::angle(units, TAU).positive()`): above zero, at most a turn, bare
  numbers in degrees, stored in radians. Two sides together at most a
  turn (the sum of two checked values, each at most `TAU`, can't
  overflow), to the rounding of degrees in radians: "0.5" and "359.5"
  come to an ulp over `TAU`, "1.1" and "358.9" an ulp under, and both
  are a turn (`TURN_ROUNDING`, 8 ε `TAU`, in the check and in `span()`). No smaller bound than "above zero": a sliver turn is the
  kernel's to refuse.
- `Revolve::span() -> Option<(f64, f64)>`: `(from, to)` in radians,
  `from < to`, the sketch plane at 0, positive right-handed about the
  axis; `None` for a whole turn, which is `Full` and also any part that
  comes to a turn (one side or symmetric of 360°, two sides adding up to
  it, to `TURN_ROUNDING`): the same solid, and the kernel's part turns
  need `to − from < 2π`.
  Of a checked revolve `0 < to − from < 2π` when `Some`.
- **Checks** (`Document::check`, `CheckError::Revolve(id,
  RevolveError)`): its sketch a sketch feature before it; 1..=256
  regions, each passing `RegionRef::check`; angles as above; its new
  body there and naming it, excluded bodies sorted and made earlier
  (shared with extrudes, `Document::check_uses`). `Revolve::check_own`
  is the part needing only the revolve and the design, cheap enough for
  a panel to run on every view.
- **Axis check**: only on `AddFeature` / `SetFeature`
  (`Revolve::check_axis`, `RevolveError::Axis(id)`): `Curve(id)` must be
  a line of the sketch as it is then. `Document::check` doesn't require
  it, so a sketch edit may delete the line or a file may name a missing
  one: regeneration fails the revolve then ("axis not found"), as it
  does a region it can't find. Setting such a revolve again needs a new
  axis.
- `removal`: removing its sketch removes it and its body; it's in
  `drop_excluded` like an extrude.

### Regeneration

`crates/regen/src/history.rs`. A revolve runs as an extrude does (`Run`,
with a `Shape` of either kind): its regions are found again and merged
into a kernel profile in the sketch's coordinates, then:

- **The axis** (`axis_line`): `SketchX` / `SketchY` the sketch's origin
  and `+x` / `+y`; `Curve(id)` the line's start and `end − start`. A
  curve that's gone or isn't a line fails the revolve with "axis not
  found" (a sketch edit may delete it: `Document::check` doesn't require
  it); a line with both ends at one point, "its axis line has no
  length".
- **The frame** (`axis_frame`): origin the axis's point, `y` along the
  axis (unit), `x` square to it in the sketch's plane toward the
  profile: the side of the profile point (segment ends and middles)
  farthest from the axis line. `y` is then chosen along or against the
  axis so that `x × y` is the sketch's normal: the 2D map is a rotation,
  so loops keep their turning (outer loops counter-clockwise, as the
  kernel wants). The kernel turns `x` toward `x × y`, which is
  right-handed about `−y`; the revolve turns right-handed about the
  axis's direction. So where the profile lies left of the axis (`y`
  against the axis) the kernel's `Sweep::Part { from, to }` is
  `span()` as it is, and where it lies right of it (`y` along the
  axis), `{ from: −to, to: −from }`. `span()` `None` is `Sweep::Full`.
- **On-axis rules**: the ends of the axis line's own segments (by curve
  id), every segment end at the same mapped point (bits), and the axis
  segments' control points are put at `x = 0` exactly; the kernel puts
  other ends within its resolution there and refuses anything reaching
  across (`CrossesAxis`: "its outline crosses the axis"), a vertex alone
  on the axis in a full turn or a segment coming within the resolution
  (`TouchesAxis`: "its outline touches the axis at a single point") and
  a part turn so nearly full its ends touch (`NearlyFullTurn`). Segments
  along the axis make no face.
- **Checked range**: the moved profile's points (ends and control
  points) must be finite and within `MAX_COORD` (they're up to about
  2.8 × `MAX_COORD` from an axis at the far side of the sketch), else
  "its regions are too far from the axis to revolve", before the
  kernel is asked. Angles are the document's, checked by `Turn::ask`.
- **The tool** is `varde_kernel::revolve(profile, frame, sweep, feature
  id, tolerance, Budget::DEFAULT)`, its faces named by the feature id as
  an extrude's. Kernel errors are worded as an extrude's with
  "revolve"/"revolved" for "extrude"/"extruded" (`message::tool`,
  `Making`).
- **Touches and booleans** are the extrude's code, unchanged: join, cut
  and intersect on the bodies made before it less those taken out, a
  join touching several merging them into the first, empty results
  failing, `Evaluation::touched` listing what it touches.
- **Cache key** of the tool: `"revolve"`, the feature id, the regions,
  the `AxisLine`, the fit tolerance's bits, `span()` (its bits, or none),
  the sketch's key (which holds where the axis line is) and the
  placement's bits. Booleans and touches are keyed by tool and body keys as an
  extrude's.

**Drafts** are of any kind: `regen::Draft { revision, feature, kind:
FeatureKind }`, applied as `AddFeature` (feature `None`) or `SetFeature`
would (so a draft may change its feature's kind; a missing feature is
refused with "the draft's feature isn't there", a sketch feature or kind
by the editor). The app's `MeshFeed::request_with` takes `(Option<FeatureId>,
FeatureKind)`; the extrude session passes its extrude `.into()`.
`Drafted.touched` is filled for a revolve's join, cut or intersect as for
an extrude's.

### UI

**The session** (`app/src/doc/revolve.rs`, `Doc::revolve`, a
`RevolveSession`) follows the extrude's (see "The extrude UI" in
`agents/kernel.md`), sharing its parts (`app/src/doc/regions.rs`):
`RegionPick` (the source sketch, the candidates' profiles found within
`REFRESH_WORK`, the regions picked and their references, found again
when the sketch changes, the edited feature's missing ones counted),
`BodyTargets` (the bodies taken out and those just put back, as the
Bodies list shows them, `Doc::body_targets`) and `TypedText` (a typed
value's text, last value and error, read for an `Ask`, pinned to the
design's units when they change). It's started by `Look::StartRevolve`
(see "Starting it" below; again, or `Esc`, cancels it; outside sketches and an extrude, in a
document that can be changed; the sketch selected in the Timeline is
the source) or by editing a revolve (`Look::EditFeature`, a
double-click or `Enter` on its row, or Edit revolve in its menu), and
never runs with a sketch session or an extrude session: editing an
extrude drops it, editing a revolve drops an extrude's, entering a
sketch drops either. A new one starts with a full turn, "180°" in the
first angle's field and "90°" in the second, a new body.

- **Picking** (`RevolveSession::picking`, `RevolvePick`): clicks pick
  regions, or the axis. A click on a region picks or un-picks it either
  way; a line of the source (construction or not) or one of its two
  axes picks the axis only while the axis is picked, and goes before a
  region there. The first region picked hands the clicks to the axis if
  there's none; picking the axis hands them back to regions; the
  panel's Profile and Axis rows choose (`RevolveLook::Picking`). Before
  there's a source, an axis picked sets it, as a region does
  (`RevolveLook::PickAxis { sketch, axis }`), and while an axis is
  picked, un-picking every region keeps the source.
- **The axis** is stored as the document wants it: a line as
  `AxisLine::Curve`, the sketch's axes as `SketchX` / `SketchY` (their
  built-in ids never as curves; `varde_view`'s `axis_of`). A curve that
  isn't a line isn't taken, nor a line with both ends at one point
  (regeneration would fail it, "its axis line has no length"): the
  viewport's `hit_axis` passes over it, and one a sketch edit shrinks to
  a point waits as a deleted one does. The session keeps the axis while the source
  doesn't have it (its line deleted by an edit, then undone): the
  revolve isn't whole then (no preview, no OK), and is again once it's
  back. An edited revolve whose line is gone opens without an axis,
  saying so ("The axis line wasn't found"), until another is picked.
- **Whole and ready**: a source, regions picked, an axis the source has,
  and the angles the extent takes as they last read
  (`RevolveSession::revolve`). Ready (`Doc::revolve_ready`) adds the
  extrude's conditions: editable, no sketch edits waiting on the solver,
  no field refused, and `Revolve::check_own` passing (two sides over a
  turn together shows its refusal in the panel at once, as
  `RevolveState::refused`). The axis is the source's, so the document's
  `check_axis` on `AddFeature` / `SetFeature` passes.
- **Preview**: the whole revolve is the request's draft
  (`Doc::revolve_draft`, after the extrude's in `Doc::request_model`),
  `NewBody(BodyId::NEW)` for a new body; its error shows in the panel.
- **Committing** (`Edit::CommitRevolve`: OK, `Enter` in an angle's field
  or the screen's `Enter`; `Doc::commit_feature`, the extrude's too)
  applies `AddFeature` ("Revolve N", hiding the
  sketch, adding the body) or `SetFeature`, one undo step, selects the
  new revolve and ends the session; OK with nothing changed writes
  nothing. `Esc` or Cancel drops it and its draft. A replacement of the
  whole document, read-only, or the edited revolve or the source gone
  end it (`Doc::prune_revolve`).
- **Random sequences** (`app/src/doc/revolve/tests/fuzz.rs`; more with
  `VARDE_FUZZ_SEEDS`, `VARDE_FUZZ_FROM`) drive both sessions with picks,
  typed angles and distances (bad ones too), keys, undo and redo, sketch
  edits deleting the axis or regions, replacements, read-only, deletes
  from the Timeline, unit and tolerance changes, answers late or out of
  order, and screens of several sizes, checking after every step: the
  sessions apart and none read-only, the draft last sent the session's
  and the panel's values, OK never with a refusal or a refused field,
  `Esc` leaving the document as it was, a commit one undo step, and the
  draft's error the newest answer's. The viewport's
  (`viewport/revolve/tests.rs`) clicks random sketches and cameras.

**The panel** (`view/src/revolve.rs`) is the extrude's floating panel
(`operation_panel`), built of the same parts (choices, ticks, typed
fields, the Bodies list, the footer's message, now in
`operation_panel.rs`): title "New revolve" or the revolve's name; a
Profile row (the region count, or "Click regions") and an Axis row
("Line 3", "X axis", or "Click a line or axis"), each a field outlined
while it's the one picking, a click on it making it so; what an edited
revolve lost; Extent (Full 360°, One side, Symmetric, Two sides), the
angle fields ("Angle", or "Side 1" and "Side 2"; the first is
`VALUE_FIELD`, focused as the session opens), Flip for one side and two
sides; Operation and Bodies as an extrude's; the refusal, the draft's
error or "Checking the sketch…". No handle in this plan.

**The viewport** (`view/src/viewport/revolve.rs`, `Revolving`, one of
`viewport::Operating`): the regions as an extrude's
(`viewport/regions.rs`, shared); while the axis is picked, the
candidates' lines (construction ones dashed) and their two axes
(dashed, in the sketch's axis colour, `axis_reach` either side of the
origin: a quarter past the sketch's farthest point, at least 10 mm)
drawn in the live layer on their planes, the one under the cursor
wider in the hover colour; hit testing is `hit::hit_axis` (lines within
6 px first, then the axes within their reach), the nearest by depth over
the candidates. The axis picked is drawn on the source in the selected
colour with an arrowhead (screen space) at the end positive angles turn
right-handed about: the line's end, or a built-in axis's +x / +y end,
and the other end when flipped for one side or two sides (as the
revolve's `span` does). The knobs' layer under the panel is an empty
placeholder for a revolve, so the panel's state keeps its place. A test
checks the turn against regeneration's: a quarter turn's preview lies
on the side right-handed about the line from its start to its end (or
the other side flipped). The 1 px jogs seen on a revolved cylinder's
silhouette (where the world X axis passes behind it in the screenshot
scenario) are its tessellation, not the axis's drawing or a crack: the
mesh is welded with no open edges, and the wall's inside vertices sit
at other angles than its rings', so its outline steps by up to a pixel.

The status bar says "New revolve · 1 region picked · about Line 3"
(or "pick the regions to revolve", "pick the axis"), with the hints
"Pick regions" or "Pick the axis", `Enter` OK and `Esc` Cancel; the
toolbar's tag "Revolve" or "Editing Revolve 1". While a revolve is set
up `S`, `X`, `B` and the selected feature's `Enter` and `Delete` don't act,
and the cursor doesn't pick the model.

**Starting it**: the toolbar's Revolve button after Extrude, the rail's
Create set (Sketch, Extrude, Revolve; its list's letter `O`) and the key
`O` all send `Look::StartRevolve` through one binding
(`shortcut::revolve_binding`, `Shortcut::REVOLVE`): enabled outside a
sketch and an extrude session, with a sketch to take regions of (the
extrude's `extrudable`: a visible sketch or the selected one) or a
revolve being set up, in a document that can be changed. The button and
the rail's entry are highlighted while a revolve is set up. `O` is the
UI mock's key: its model rail's sets open with `Q` .. `T` (five sets),
so `R` would open its fourth and Revolve takes `O`, Extrude `X` (the
app's model rail has three sets since Combine's Modify set, so `E` opens
the third and Extrude is `X` too). In a sketch `O` is the Offset tool's
and `X` construction's; outside one they're free, and the rail's set
keys reach `O` only with a ninth set. `X` is disabled while a revolve
is set up and `O` while an extrude is, and both while a combine is.

**The Timeline** shows a revolve with its own icon (`Icon::Revolve`,
the icon mock's: an open circle with an arrowhead about a dashed axis)
and, as its note, how far it turns in all (`panels::turn_note`: "360°",
"270°", "120°" for two sides of 100° and 20°). Selected, the status bar
says its turn, operation and axis (`feature_info`: "Full 360° · New
body · about Y axis", "One side 90° · Cut · about Line 3", "Symmetric
90° ...", "Two sides 100° + 20° ..."; the axis last, as the selection's
box clips what doesn't fit at 1280 px and the axis says least, and left
out while its sketch doesn't have it). Double-click, `Enter` or Edit revolve reopen it.

Not yet: a handle dragging the angle.

## Combine

`crates/document/src/combine.rs`.

```rust
pub struct Combine {
    pub target: BodyId,         // made by a feature before it
    pub tools: Vec<BodyId>,     // 1..=MAX_FEATURE_BODIES (256), sorted, no repeats, not the target
    pub op: BodyOp,             // Union, Subtract, Intersect
    pub keep_tools: bool,       // off: the tools are consumed into the target
}
```

- **What it is**: a timeline step working on bodies already made, not
  on a sketch: `FeatureKind::Combine` (stored by name in files; the
  workers' postcard has it as the fourth variant). It makes no body
  (`operation()`, `new_body()` and `sketch()` are `None`), takes no
  regions and hides nothing when added. `BodyOp::label` gives the
  panel's and Timeline's words ("Union", "Subtract", "Intersect", the UI
  mock's).
- **Checks** (`Document::check`, `CheckError::Combine(id,
  CombineError)`): `Combine::check_own` (cheap, for a panel on every
  view) wants 1 to 256 tools (`Tools(n)`), sorted without repeats
  (`ToolOrder`) and the target not among them (`TargetIsTool`); then
  every body named must be there and made by a feature before the
  combine (`Body(id)`). Unlike a sketch's face, a body that isn't there
  is refused: removing a body or its maker removes the combine with it
  (below), and `SetFeature` making the maker stop making the body (an
  extrude turned from a new body into a join) is refused while a combine
  names it (`EditError::Invalid(Combine(.., Body))`), rather than
  deleting the combine behind the user's back. `MAX_FEATURE_BODIES` is
  shared with the later body features (move, mirror, pattern).
- **Removal**: `FeatureKind::bodies()` lists the bodies a feature names
  (a combine's target, then tools); `Document::removal` takes a later
  feature that names a body made by a feature it's removing, as it
  takes one that uses a removed sketch (`uses()` stays features only:
  finding a body's maker takes the document). So removing a tool or the
  target (or its maker, or that maker's sketch) removes the combine, and
  the delete prompt lists it; removing the combine takes nothing else.
- `SetUnits` leaves it alone (no values). Commands are the shared
  `AddFeature` ("Combine N") and `SetFeature`.

### Regeneration

`crates/regen/src/history/combine.rs`, run in order as every feature
is, so it sees the bodies as the features before it leave them:

- **Bodies with no solid of their own fail it**: a target or tool an
  earlier join or combine consumed, "Body 3 is in Body 2 now: a feature
  before this one merged it in" (the user named the body as it was, not
  the one it went into: a combine doesn't follow a consumed body to its
  holder, as a face sketch does), or one whose maker failed, "Body 2 has
  no solid: the feature making it failed".
- **The steps**: the target's solid, then each tool's in the order the
  tools were made (the bodies' order), `kernel::boolean(running, tool,
  op)` cached under `boolean_key(doing, running key, tool key)` as the
  merge chain's steps are; a union step uses `Doing::Merging` (two
  bodies united), so a combine and a join merging the same bodies share
  it, a subtract `Doing::Cutting`, an intersect `Doing::Intersecting`.
  Toggling Keep tools changes no key: nothing is worked out again.
- **A union's order** (as a join's merge tries a second order): a step
  that fails is put aside and the rest go on; the ones put aside are
  tried once more at the end, in order, if any step worked after them
  (otherwise they'd be the cached failures again). That covers a tool
  meeting the target only along an edge or at a point (no clean solid
  alone) once a later tool has bridged them. A subtract or intersect
  step that fails fails the combine.
- **Messages** are worded as an extrude's booleans, the tool named in
  place of "it": "joining Body 2 to Body 1 leaves no clean solid: ...",
  "cutting Body 2 from Body 1 ...", "intersecting Body 1 with Body 3
  ..." (`message::combining`). A step that would leave nothing of the
  target fails it, nothing written back (no body is ever empty): a
  subtract "cutting Body 2 from Body 1 would leave nothing of Body 1:
  take Body 2 out of the tools, or delete Body 1", an intersect
  "intersecting Body 1 with Body 3 would leave nothing of Body 1: they
  don't overlap" (`message::combine_emptied`). A tool clear of the
  target is no error: a subtract takes nothing, a union is in pieces.
- **The result**: the target gets the last step's solid and key. Unless
  `keep_tools`, the tools are consumed exactly as a join's merged bodies
  are (`note_merge(merged, [target, tools...])`, removed from
  `Evaluation::bodies`, listed in `Evaluation::merged` as (tool,
  target), whatever the operation: a subtracted tool is "in Body 1" as a
  united one is), so later features find them in the target: a sketch
  on a consumed tool's face is placed on the target's solid
  (`Evaluation::holder`), a join over where it was touches the target,
  and the app's Objects shows it faint "in Body 1". With `keep_tools`
  they stay as they were, each its own body.
- A combine touches nothing an extrude's panel lists: it isn't in
  `Evaluation::touched`, and a combine draft's `Drafted::touched` is
  `None`. Drafts need nothing new (`Draft::kind` is any kind); the wire
  carries the kind in the document's postcard and `merged` as before.

### UI

**The session** (`app/src/doc/combine.rs`, `Doc::combine`, a
`CombineSession`) is started by `Look::StartCombine` (`B`, the toolbar's
Combine button after Revolve, the rail's Modify set; again, or `Esc`,
cancels it) or by editing a combine (`Look::EditFeature`: a
double-click or `Enter` on its row, Edit combine in its menu), in a
document that can be changed, outside sketches and the other
operations, as the revolve's: editing an extrude or a revolve drops it,
editing a combine drops theirs, entering a sketch drops it, and it
drops the measure tool. A new one takes its bodies from what's selected
in the model: the first item's body is the target, the others' bodies
the tools (as the UI mock's init takes the body selected); else it
starts empty, picking the target. A union using the tools up, to begin
with.

- **Picking**: while it's set up the cursor picks the model as outside
  the sessions (`Doc::picks` is true with a combine even while its
  preview shows: a combine draft makes no body, so the model's bodies
  are the document's), only faces whatever the selection's mode
  (`Doc::model_picking`), and a click on one picks its body
  (`Look::ClickModel` → `Doc::combine_click`); a body's row in
  Objects does too (`Look::ClickBody` → `Doc::combine_body`). What a
  click picks is `CombineSession::picking` (`CombinePick`): the target
  (taking it out of the tools if it's one, then handing the clicks to
  the tools), or a tool (added, kept sorted as the document wants, or
  taken out if it's one; the target is no tool, nor are tools added
  past `MAX_FEATURE_BODIES`). The panel's Target and Tools fields
  choose (`CombineLook::Picking`). A body an earlier join or consuming
  combine merged into another is picked as the body holding it
  (`MeshFeed::merged_before` up to the edited combine), which the model
  draws it as: the combine would fail naming it ("Body 3 is in Body 2
  now"). Only bodies made before the edited combine are picked
  (`combine::pickable`, the document's rule), and the session lets go
  of bodies the document no longer has (`CombineSession::prune`).
- **The highlight** is its own while it's set up, as the measure
  tool's: the target's faces in the selection's colour, the tools' in
  the second colour (a tool the preview uses up has no faces of its
  own), the body under the cursor hovered (`PickIndex::highlight_with`);
  the selection is kept, and not looked for in the preview's model.
- **Whole and ready**: a target and a tool (`CombineSession::combine`);
  ready (`Doc::combine_ready`) when editable, no sketch edits wait on
  the solver, and `Combine::check_own` passes.
- **Preview**: the whole combine is the request's draft
  (`Doc::combine_draft`, after the extrude's and revolve's in
  `Doc::request_model`); its error shows in the panel. A draft using its
  tools up merges them in the answer's `merged`, so Objects lists them
  faint, "in Body 1", as the committed combine will.
- **Committing** (`Edit::CommitCombine`: OK, the screen's `Enter`)
  applies `AddFeature` ("Combine N") or `SetFeature` through
  `Doc::commit_feature`, one undo step, selects the new combine and ends
  the session; OK with nothing changed writes nothing. `Esc` or Cancel
  drops it and its draft. A replacement of the whole document,
  read-only, or the edited combine gone end it (`Doc::prune_combine`).

**The panel** (`view/src/combine.rs`, in `operation_panel`): title "New
combine" or the combine's name, the tool count as its summary; Target
and Tools rows, each a field (`theme::pick_field`, outlined in the
accent while it's the one picking, a click on it making it so) holding
the bodies as chips (`theme::chip`: the name and a button taking it
out, `CombineLook::Drop`; dropping the target hands the clicks to it),
the tools one under another, and "Click a body" / "Click bodies" while
empty, "+ Click bodies" after the tools while they're picked; Operation
(Union, Subtract, Intersect: `BodyOp::label`); "Keep tool bodies" with
"Otherwise the tools are used up" under it; the footer's draft error,
"Checking the sketch…", or with fewer than two bodies "There’s only one
body: make another to combine with". The mock's warnings for a tool
clear of the target are left out: regen answers that case (a subtract
takes nothing, a union is in pieces, an intersect fails as emptying).

The status bar says "New combine · Body 1 with 2 tools · Union" (or
"pick the target body", "Body 1 · pick the tool bodies"), with the hints
"Pick the target" or "Pick tools", `Enter` OK and `Esc` Cancel; the
toolbar's tag "Combine" or "Editing Combine 1". `B` (`Shortcut::COMBINE`,
`shortcut::combine_binding`, the UI mock's key; the Rectangle tool's in a
sketch) is enabled outside a sketch and the other operations, with two
bodies or more (`Doc::combinable`, `DocumentKeys::combinable`) or a
combine being set up, in a document that can be changed. While it's set
up `S`, `X`, `O`, `I` and the selected feature's `Enter` and `Delete`
don't act.

**The Timeline** shows a combine with its own icon (`Icon::Combine`,
the mock's two overlapping boxes, in the Modify colours; the mock's
tinted overlap is a fill the line-only set leaves out) and its operation
as the note; selected, the status bar says "Body 1 with Body 2, Body 3 ·
Union · tools kept" (`feature_info`).

**Merges replayed in the app**: `MeshFeed::merged_before(document,
until)` replays, in the document's order, the joins the model shown
found merging and the combines using their tools up that it didn't find
failing (`MeshFeed::consumes`), by regen's rule (`note_merge`, a
combine as its target then its tools): the extrude's and revolve's
Bodies lists and the combine's picking use it, and the delete prompt's
warning replays combines alike. `feed/tests.rs`'s
`merged_before_agrees_with_regen_on_random_histories` holds the two to
the same answer on random histories of bodies, joins, combines (some
naming used-up bodies, so failing) and edits of earlier combines, for
the whole history and stopped before each feature
(`VARDE_MERGES_SEEDS`, 4 by default).

**An extrude or revolve making a combined body** can't stop making it
(the document refuses the edit, above). Its panel says so at once when
Join, Cut or Intersect is picked (`Doc::held`, `ExtrudeState::held`,
`RevolveState::held`): "Combine 1 combines Body 2, so this stays a new
body: take Body 2 out of Combine 1 or delete it first", in the footer
in place of the preview's error, and OK waits.

Tests: `app/src/doc/combine/tests.rs` (keys, viewport and Objects
picks, the chips' buttons through the panel, the draft, Objects' faint
rows, one undo step, `Esc` without a trace, editing, merged bodies
picked as their holder, the highlight, read-only, the other tools, a
body going, the held extrude, the Timeline row, the delete prompt);
`view/src/combine/tests.rs` (the panel's texts and layout); shot
scenario 23.

Known gaps: a kept tool overlapping the target's union shows both
bodies' faces in the same place, which z-fight (as two overlapping
bodies always do); the panel doesn't warn before the preview answers.
