# Features

The features after sketches and extrudes (revolves, combines, moves, mirrors and patterns), and sketches' planes on faces:
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
  whatever id the command held (`BodyId::NEW`); a pattern whose copies
  are bodies of their own adds one per copy, laid out as "Pattern"
  says. One undo step. A sketch
  kind is refused (`EditError::SketchKind`): sketches are added empty by
  `AddSketch` and set by `SetSketch`.
- `Command::SetFeature { feature, kind }` replaces a feature's kind,
  keeping its id, name and visibility. A missing feature is a no-op; a
  sketch feature, or a sketch kind, is refused (`EditError::SketchKind`).
  The kind may change (an extrude may become a revolve): what matters is
  the operation. A `NewBody` that stays one keeps its body (whatever id
  the command held); one that stops removes the body and drops it from
  the other features' excluded lists; one that starts adds one. A
  pattern's copy bodies go the same way, copy by copy (see "Pattern").
  The caller passes regions referenced afresh from the sketch as it is.
  Setting what's already there changes nothing (no new revision).
- Both run the document's whole check on the result, and then
  `check_new` on the added or set feature: what's required of a feature
  when the user makes or edits it, but not of one already in a document
  (a later edit of its sketch may break it, which regeneration reports).
  Today that's only a revolve's axis line.
- `FeatureKind` helpers: `noun`, `sketch` (the profile sketch),
  `operation` / `new_body` (an extrude's or revolve's `Operation`; a
  body's maker is checked by `new_body`, or a pattern's copy bodies),
  and `uses`, now a **list**
  (sorted, no repeats) of the features this one builds on, which
  `Document::removal` follows. Every extrude-only path that only cared
  about the operation (`drop_excluded`, the app's delete prompt and its
  join merges) goes through `operation()` so revolves get them too.
- `SetUnits` pins revolve angles by `Turn::ask` as it pins extrude
  distances by `Extent::ask` (angles' bare numbers are degrees whatever
  the units, so only lengths inside an angle's expression change).
- Kinds are **appended** to `FeatureKind` (`Sketch` 0, `Extrude` 1,
  `Revolve` 2, `Combine` 3, `Move` 4, `Mirror` 5, `Pattern` 6): files store a kind by its variant name, and
  the variant index is what the workers' postcard holds.

## Failures and where they are

`crates/regen/src/history.rs`, `crates/regen/src/error_geometry.rs`.
A feature that fails is a `FeatureFailure { feature, message, geometry:
Option<Arc<ErrorGeometry>> }` in `Evaluation::failed` and
`Response::Regenerated::failed`, in the document's order; a failing
draft's `Drafted` carries the same `geometry` beside its `error`. The
message is worded from the kernel's `failure.error` as before
(`src/message.rs`).

- **Carried**: the cache keeps a kernel failure as a `KernelFailure`
  (`Entry::Touches`, `Entry::Boolean`): its `error`, its evidence made
  drawable once (`ErrorGeometry::of_evidence`, below) and the operand faces the
  evidence names, so a failure found again is neither made nor drawn
  again. Inside the history a failure is `Failed { message, geometry }`
  (a `FeatureFailure` without its feature; a tool's `Entry::Solid` keeps
  it whole). `Failed::kernel` takes the geometry from
  `KernelFailure::geometry(operands)`, the bodies each of the operands
  `a` and `b` holds (`[&[BodyId]; 2]`; a face an operand names is looked
  for on each of its bodies and drawn on every one it's found on: the
  kernel names a face by key alone, and a feature that made faces on
  several bodies, as a cut through two, gives them all one key, so which
  it meant isn't known; at most `ErrorGeometry::MAX_PENDING` (16 ×
  `MAX_EVIDENCE.faces`) pairs of body and face are pending, in the
  evidence's order, the rest left out and `truncated` set): a join's, cut's or intersect's boolean:
  the body, then the tool, which is none (a feature's tool is drawn as no
  body); a merge step (`Run::merge`): the first body and the bodies
  merged into it so far (the tool joined to it is none), then the body
  being merged, or for the tool's own step the first body, then none; a
  combine step: the target and the tools combined into it so far
  (`combine::Running::held`, which a step that works extends with the
  running solid itself, so the two can't part through the retry pass;
  the running solid holds their faces, and while the combine fails each
  is still its own body), then the tool's body; `touches`: the body, then
  none; extrude and revolve: none. With no operand face on a body that
  is the very `Arc` kept, so an unchanged failure is the same `Arc` from
  one answer to the next; with some, a copy with those faces pending
  (keeping the evidence's `truncated` even when its faces are all there
  is). Failures that aren't the kernel's convert from their words
  (`From<String>`), with no geometry, except the two of regen's own that
  have some (below), made by the same `ErrorGeometry::of_evidence` from
  evidence regen fills itself, within the same caps. A face sketch's placement keeps its `Failed`
  (`Entry::Placement`), keyed by the fit tolerance too since the face is
  drawn at it. `Entry::bytes` counts the geometry.
- **Made drawable** (`ErrorGeometry::of_evidence`, at the `Display` of the
  document's tolerance): patches tessellated each on its own as a face of
  no known form is (`Display::sample_patch`) into one `RenderMesh` of one
  part and one face with triangles only; the curves, then the patches'
  boundary (their sides no two of them share: a side matches another
  running back along it with the same control point and weight, as a
  mesh's neighbours' do; a side held the same way round by several
  patches, as a patch given twice, is drawn once, and two patches back
  to back close on each other and draw none), flattened as a solid's edges are
  (`Display::flatten`) into `RenderLines`; points as `[f32; 3]`; the
  sketch curves as ids; all relative to the diagonal of the box of the
  evidence that may be drawn (a patch or curve its check refuses, or a
  point past `MAX_POSITION`, doesn't coarsen the rest). Bounded
  (`MAX_VERTICES` 2^18, `MAX_INDICES` 3·2^19, `MAX_LINE_POINTS` 2^18,
  points and sketch curves as `MAX_EVIDENCE`, `MAX_FACES` 4 ×
  `MAX_EVIDENCE.faces`), stopping at the first patch or curve that
  doesn't fit; what's past a bound, a patch or curve failing the
  kernel's check, a coordinate past `MAX_POSITION` (`RenderMesh`'s) is
  left out and `truncated` set, as it is when the evidence was. No
  evidence, or nothing drawn or named, is `None`. The renderer can
  upload its mesh and lines as it does the model's and the sketches'.
- **Operand faces** are pending until the model is drawn
  (`Regenerator::draw`): each operand's `FaceKey` is looked for, by key
  or alias, among the faces of the scene's parts of the body holding the
  operand's body (`Evaluation::holder`), giving `(BodyId, face id)` of
  the answer's mesh. Those faces' triangles join the geometry's mesh,
  their vertices and normals copied from the model's
  (`ErrorGeometry::add_model_faces`, face by face, each whole with its
  outline, while within `MAX_VERTICES`, `MAX_INDICES` and
  `MAX_LINE_POINTS`, else `truncated` and no more), so a named face is
  drawn red with the rest, and each face's outline joins its lines as a
  patch's boundary does (`outline`: the sides of the face's triangles no
  other runs back along, matched by position so a seam doesn't show,
  joined into polylines); the box then takes
  them in. A
  draft's failure is resolved on the committed model answered with it.
  `ErrorGeometry::resolve_shared` takes a copy of its own only when
  faces are pending, so geometry with none stays the very `Arc` the
  cache keeps across answers. Geometry left empty is dropped. In an
  `Evaluation` on its own
  (export, tests) the faces stay unresolved.
- **On the wire** a `Head::Regenerated` carries each failure as
  `(FeatureId, String, Option<GeometryParts>)` and the draft's as
  `draft_geometry` (`Drafted::geometry` is `serde(skip)`); the parts are
  decoded within their bounds and checked by `ErrorGeometry::from_parts`
  against the model (coordinates finite within `MAX_POSITION`, the
  triangles and lines whole, each face one of the mesh's of the body
  named; the box worked out again from the triangles' corners, the
  lines and the points, so a position no triangle uses doesn't stretch
  it), one that fails answering the
  generation as failed (`wire::Error::Geometry`). A head too large with
  its geometry is sent without it.
- **What has geometry so far**: an extrude's or revolve's tool failing
  on its profile (`KernelError::Profile`): the kernel gives the
  segments the error names, placed by the frame regen passes, which is
  the sketch's placement (a revolve's turned to its axis, still in the
  sketch's plane), the points it is about and the sketch curves
  (`Segment::curve`, the sketch curve's `Id::get`), so a draft's
  `Drafted::geometry` and a committed feature's `FeatureFailure` show
  the segments where the sketch has them (tested end to end in
  `history/tests/profile_evidence.rs`). A tool, join, cut or combine
  whose solid fails the kernel's check or repair (`Invalid`, and a
  boolean's `Invalid` named `NotManifold`) gives the triangles the
  error names (or the pieces of them repair couldn't mend), drawn as a
  patch mesh (`history/tests/check_evidence.rs`:
  a slot too thin, two boxes joined along an edge). A boolean's own
  errors give where they are (`history/tests/boolean_evidence.rs`): a
  pinch's two vertices as points, walls touching along a line as their
  patches and the faces they lie on, a face that can't be triangulated
  as its loops, and decisions that don't fit together
  (`Inconsistent`) as the edge, vertex, pair of patches, crossing or
  arc they are about, or a cut face's boundary that doesn't close (a
  rod intersected with a cylinder whose end plane holds the rod's axis:
  the boundary's pieces as lines, the two vertices where they stop as
  points, the rod's face resolved to the body's faces). A join's merge
  step and a combine's step carry their boolean's evidence the same way,
  the faces on the bodies their operands hold (`history/tests/merging.rs`:
  the pinch where a second body meets the first along an edge the tool
  doesn't cover; `history/tests/combine.rs`: two discs' walls touching
  along a line, the first united with the target before, each wall
  found on its own disc's body). The kernel's errors with none are
  `TooComplex`, `Patch`, an empty profile and `assemble`'s and the
  transforms' own checks.
  Regen's own failures, where cheap:
  - "its face isn't flat" (`place_on_face`, `face_geometry`): the face
    found, the region's triangles as patches of the solid as the
    features before the sketch leave it (by value, so later features
    don't move it; the first `MAX_EVIDENCE.patches`, then `truncated`),
    drawn with their outline; it costs what drawing a kernel failure's
    patches does, once per placement worked out (`history/tests/faces.rs`, the example
    plate's hole wall);
  - "its axis line has no length" (`Run::axis_failed` on
    `AxisError::NoLength`, a typed reason, so the wording can change):
    the line's point
    placed by the sketch, and the line as a sketch curve, so the sketch
    editor marks it (`history/tests/revolve.rs`);
  - none for the rest, which have nothing to show: "its face's body is
    gone", "its face wasn't found", "its face is too far out to sketch
    on" (the face is flat, its plane just out of reach), "its sketch
    isn't placed" (the sketch's own failure says why), "its sketch isn't
    there", "axis not found", "its regions are too far from the axis to
    revolve", "region not found", a sketch too complex, a through all
    with no body, a combine's body with no solid of its own or consumed
    before, a boolean that would leave nothing, "it doesn't touch any
    body", a move's, mirror's or pattern's body taken out of range (or a
    pattern's copies too many patches), its axis's or
    plane's body gone or face not found (a mirror face that isn't flat,
    an axis face that isn't round and an axis edge that isn't straight
    or round show themselves, as above; see "Move and mirror").
- **The app** keeps `MeshFeed::failed_features` as `FeatureFailure`s
  and the draft's `Drafted` (`MeshFeed::draft_geometry`, beside
  `draft_error`). The viewport draws the geometry of the draft's
  failure while an operation is set up, and a failed feature's while
  its Timeline row is hovered or selected or its panel is open (the
  draft's wins while editing it), and while a sketch is edited the
  points of every failure naming its curves (the sketch marks the
  curves), nothing otherwise (`Doc::shown_errors`, see "Error geometry"
  in `agents/viewport.md`). A failed feature's Timeline row has its
  name in the strong danger colour (`Palette::danger_strong`, the UI
  mock's `--danger-text`) with an alert (`Icon::Alert`) right after it,
  its note staying at the right, and its message as the row's tooltip
  (`chrome::failure_tip`: the alert, "Extrude fails: " in semibold in
  the strong danger colour, as the operation panel's failure box titles
  it, then the message in the text's colour; its spans take the colours
  of the document's `Mode` as they're made); selected, the status bar's
  selection box shows the alert in the danger colour, its name and the
  message muted. The row has no Show button: a failure is framed from
  the operation panel's failure box, where Show, left of Add anyway,
  frames the camera on the draft's geometry's box
  (`Look::ShowFailure`) and turns into Go back
  (`Look::BackFromFailure`); it's there only where the geometry has
  one.

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
  same bits) must be `Form::Plane { n, d }`, else "its face isn't flat",
  which shows the face (see "What has geometry so far" above);
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
handle and axis are drawn on) leave out a sketch with none. A sketch on a face that
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
pub enum AxisLine { Curve(Id), SketchX, SketchY, Edge(EdgeRef) }   // appended
pub struct EdgeRef { pub body: BodyId, pub faces: [FaceKey; 2], pub near: DVec3 }  // faces sorted
pub enum Turn { Full, OneSide(Value), Symmetric(Value), TwoSides(Value, Value) }
```

- **Axis**: `Curve(id)` a line of the sketch, construction or not,
  directed from its start point to its end; `SketchX` / `SketchY` the
  sketch's own axes, directed along +x / +y. The sketch's built-in axis
  ids (`Id::X_AXIS`, `Id::Y_AXIS`) aren't curves: a pick of one is stored
  as `SketchX` / `SketchY`, and `Curve` of them is refused.
  `Edge(EdgeRef)` is a straight edge of a body as the features before
  the revolve leave it (`crates/document/src/edge.rs`): the body, the
  keys of the faces either side, sorted and different, and the picked
  point (finite, within `MAX_COORD`), which chooses among several edges
  between faces of those keys, as a face reference's does. Its
  direction is the way the edge runs with the face of its first key on
  its left seen from outside the body: a face's own boundary runs round
  it that way (its halfedges, and the mesh's drawn edge, which runs
  along its first face), so the direction follows the faces and an edit
  keeping them keeps it.
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
- **Edge check** (`Document::check`, for every document): an `Edge`
  axis's reference passes `EdgeRef::check_own` (`RevolveError::Edge`:
  keys sorted and different, point in bounds); its body is made before
  the revolve (`EdgeBody`) and both keys' features come before it
  (`EdgeMaker`), as a sketch on a face's (`Document::made_before`,
  `body_before`, shared): one that isn't there is allowed with an id
  below `next_id`, one no later body or feature can take. Whether the
  edge is there, straight and in the plane is regeneration's.
- `removal`: removing its sketch removes it and its body; it's in
  `drop_excluded` like an extrude. Its axis edge's body or faces' makers
  don't cascade: removing them leaves the revolve failing ("its axis
  edge's body is gone", "wasn't found"), to be edited.

### Regeneration

`crates/regen/src/history.rs`. A revolve runs as an extrude does (`Run`,
with a `Shape` of either kind): its regions are found again and merged
into a kernel profile in the sketch's coordinates, then:

- **The axis** (`axis_line`): `SketchX` / `SketchY` the sketch's origin
  and `+x` / `+y`; `Curve(id)` the line's start and `end − start`. A
  curve that's gone or isn't a line fails the revolve with "axis not
  found" (a sketch edit may delete it: `Document::check` doesn't require
  it), showing nothing; a line with both ends at one point, "its axis
  line has no length", showing that point and marking the line
  (`Run::axis_failed`). The reasons are `AxisError`s (`NotFound`,
  `NoLength`, and `TooFar` from `axis_frame`), worded by
  `AxisError::message`.
- **A model edge** (`edge_axis`, before the tool's cache key): the
  edge's body through `Evaluation::holder` as of the revolve (one with
  no solid: "its axis edge's body is gone"); on its solid's
  `Topology::edge` by the two keys (name or alias) and the point ("its
  axis edge wasn't found"); `measure::edge_shape` must be a `Line` ("its
  axis edge isn't straight", showing the edge's curves, by value as a
  face that isn't flat shows the face), its ends ordered as `EdgeRef`
  runs (`edge_ends`: as the chain's halfedges, which run on its first
  region's triangles, where `EdgeRef::runs_with` says that region is
  the first key's, else reversed; a mirrored solid's triangles are
  reversed, so the rule holds on it too). `runs_with` takes a face as
  the first key's where that key names it and the second names the
  other; aliases naming both faces by both keys fall back to the faces'
  own keys, and where those don't tell either, "its axis edge's
  direction can't be told", showing the edge. The model shown directs
  an edge by the same function, so the arrow drawn is the way the
  revolve turns. Both ends must be within the
  tolerance's resolution of the sketch's plane, a decision on geometry
  stated as one ("its axis edge isn't in the sketch's plane", showing
  the edge and its two ends): an edge of the face the sketch is on is
  in its plane to the bit on faces square to the world's axes, to
  rounding on tilted ones. The axis is then the line from the first end
  to the second mapped into the sketch, `curve: None` (no ends snapped
  to `x = 0`: the kernel puts a profile edge along it within its
  resolution there, as any other). The ends, or the failure, are cached
  (`Entry::Edge`) by the solid's key, the keys, the point's bits and the
  fit tolerance (which a curved edge's failure is drawn at).
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
  the `AxisLine` (for a model edge, the resolved axis's `at` and
  `along` bits instead, so the tool follows the body under it and the
  edge picked again elsewhere finds it), the fit tolerance's bits,
  `span()` (its bits, or none), the sketch's key (which holds where the
  axis line is) and the placement's bits. Booleans and touches are keyed by tool and body keys as an
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
(see "Starting it" below; again, or `Esc`, cancels it; outside sketches, in a
document that can be changed; the sketch selected in the Timeline is
the source) or by editing a revolve (`Look::EditFeature`, a
double-click or `Enter` on its row, or Edit revolve in its menu), and
never runs with a sketch session or another operation: starting or
editing an extrude or combine drops it, starting or editing a revolve
drops theirs, and entering a sketch or picking a new one's plane drops
any. A new one starts with a full turn, "180°" in the
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
- **A model edge as the axis** (`RevolveLook::PickEdge { model, edge,
  at }`, `Doc::pick_axis_edge`): once there's a source, a straight edge
  of the model shown in the source's plane (`varde_view::axis_edge`:
  `PickIndex::edge_ends`, straight as the mesh draws it, and in the
  plane as regeneration tells it, within the document's tolerance's
  resolution (`RevolveState::resolution`): the edge's snap point, the
  exact ends' middle, within it, and the mesh's `f32` ends within it
  and their rounding. So every edge regeneration takes is taken, and
  one it refuses is taken only if tilted across the plane by less than
  the `f32` rounding, which regeneration then reports). It's named as a sketch on a face is
  (`varde_view::Naming`, `PlanePick`'s naming with the history stopped
  at the revolve: the edited one's place, or the end): the two faces'
  keys sorted, the point clicked, and the body its first face is on as
  of the revolve, the merges before it replayed (`Naming::before`, in
  the document's order: the joins the model shown found working and the
  combines using their tools up that it didn't find failing, as
  `MeshFeed::merged_before` does; an edge between a target's face and a
  used-up tool's is on the target). Refused, with a notice (the status
  bar's, the app's stand-in for the mock's toast on a refused pick)
  saying why: before there's a source ("Pick the profile first, then
  its axis"), a round edge ("Only a straight edge can be the axis"), one
  off the plane ("That edge isn't in the sketch's plane"), one a feature
  at or after the revolve made, the revolve's own included ("Only an
  edge made before the revolve can be its axis"), one whose faces may be
  on several bodies there ("Which body that edge is on at the revolve
  can't be told: pick another"), and a pick of a model no longer shown.
  The session keeps the edge's ends where the model shown had it when
  picked (`RevolveSession::edge_ends`), or, editing, where it shows it
  then (`Doc::shown_edge`, on the body holding the reference's), for the
  arrow only, moved to where each model answered shows it
  (`Doc::follow_edge_axis`: an undo may move it; kept where it was if
  the model doesn't show it). An edge the document no longer takes at
  the revolve's place (`Document::check_edge`: an undo took its body or
  a face's maker away) is dropped as the document changes
  (`RevolveSession::prune_edge`) and the axis is picked again, so
  what's set up never names what the document can't hold; an edited
  revolve's stored edge, whose body or maker is gone with ids below
  `next_id`, the document holds (failing), and it stays. A model edge
  is always the source's axis
  (`axis_holds`), and an edited revolve's isn't "missing": regeneration
  says when it's gone.
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
  nor with the draft's error (Add anyway only with it),
  `Esc` leaving the document as it was, a commit one undo step, and the
  draft's error the newest answer's. The viewport's
  (`viewport/revolve/tests.rs`) clicks random sketches and cameras.

**The panel** (`view/src/revolve.rs`) is the extrude's floating panel
(`operation_panel`), built of the same parts (tiles, icon toggles,
fields picked into, typed fields, the Bodies list, the foot's message,
in `operation_panel.rs`; their look is in `agents/viewport.md`): title
"New revolve" or the revolve's name; a Profile field (the regions
picked as rows, "Region 1" with a cross taking it out, then "Click
regions" while it's the one picking or empty) and an Axis field ("Line
3", "X axis", "Edge of Body 1" as a row, or "Click a line, axis or
edge"; the mock's revolve has no edges, so the wording joins its Move
axis's "Click an axis or edge"), each outlined while
it's the one picking, a click on it making it so; what an edited
revolve lost; Extent tiles (Full 360°, One side, Symmetric, Two sides,
seen down the axis), the angle fields ("Angle", or "Side 1" and "Side
2"; the first is `VALUE_FIELD`, focused as the session opens), Flip
for one side and two sides; Operation and Bodies as an extrude's; the
refusal ("Revolve fails", with no Add anyway), the draft failing
("Revolve fails", Add anyway) or "Checking
the sketch…". No handle in this plan.

**The viewport** (`view/src/viewport/revolve.rs`, `Revolving`, one of
`viewport::Operating`): the regions as an extrude's
(`viewport/regions.rs`, shared); while the axis is picked, the
candidates' lines (construction ones dashed) and their two axes
(dashed, in the sketch's axis colour, `axis_reach` either side of the
origin: a quarter past the sketch's farthest point, at least 10 mm)
drawn in the live layer on their planes, the one under the cursor
wider in the hover colour; hit testing is `hit::hit_axis` (lines within
6 px first, then the axes within their reach), the nearest by depth over
the candidates. Once there's a source, the model's edges that can be
the axis (`axis_edge`) are drawn too, as world lines, the one under the
cursor wider in the hover colour; the model picks them as it picks
edges (`PickIndex::pick` with `Picks::Edges`), after the sketch's lines
and before the regions, and a click on another edge where there's no
region sends it to be refused. The axis picked is drawn on the source in the selected
colour with an arrowhead (screen space) at the end positive angles turn
right-handed about: the line's end, a built-in axis's +x / +y end, or a
model edge's end as its reference runs (its ends mapped onto the
source's plane), and the other end when flipped for one side or two
sides (as the revolve's `span` does). The knobs' layer under the panel is an empty
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
"Pick regions" or "Pick the axis", `Enter` OK and `Esc` Cancel. The
toolbar shows it as it shows a sketch being edited: a pill on the soft
accent with its icon and name ("New revolve" or "Revolve 1") and OK
joined to it (disabled while OK waits), then Cancel (`Esc`) leading the
operations; the extrude and the combine alike. While a revolve is set
up `B` and the selected feature's `Enter` and `Delete` don't act, `S`
and `X` drop it for a new sketch or an extrude, and the cursor doesn't
pick the model.

**Starting it**: the toolbar's Revolve button after Extrude, the rail's
Create set (Sketch, Extrude, Revolve; its list's letter `O`) and the key
`O` all send `Look::StartRevolve` through one binding
(`shortcut::revolve_binding`, `Shortcut::REVOLVE`): enabled outside a
sketch, in a document that can be changed, as Sketch and Extrude are:
with no sketch yet too (the panel waits for one, "Click regions"), and
while another operation is set up, which it drops. The button and
the rail's entry are highlighted while a revolve is set up. `O` is the
UI mock's key: its model rail's sets open with `Q` .. `T` (five sets),
so `R` would open its fourth and Revolve takes `O`, Extrude `X` (the
app's model rail has four sets since the Transform set, so `E` and `R`
open the third and fourth and Extrude is `X` too). In a sketch `O` is the Offset tool's
and `X` construction's; outside one they're free, and the rail's set
keys reach `O` only with a ninth set. `X` while a revolve is set up,
`O` while an extrude is, and both and `S` while a combine is, swap it
for theirs.

**The Timeline** shows a revolve with its own icon (`Icon::Revolve`,
the icon mock's: an open circle with an arrowhead about a dashed axis)
and, as its note, how far it turns in all (`panels::turn_note`: "360°",
"270°", "120°" for two sides of 100° and 20°). Selected, the status bar
says its turn, operation and axis (`feature_info`: "Full 360° · New
body · about Y axis", "One side 90° · Cut · about Line 3", "Symmetric
90° ...", "Two sides 100° + 20° ..."; the axis last, as the selection's
box clips what doesn't fit at 1280 px and the axis says least, and left
out while its sketch doesn't have it). Double-click, `Enter` or Edit revolve reopen it
(a double-click on a sketch's row, in the Timeline or Objects, enters
the sketch).

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
  step that fails fails the combine. A failing step shows its boolean's
  evidence, the running solid's faces looked for on the target and the
  tools combined into it so far, the tool's on the tool's body (see
  "Failures and where they are").
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
- **Random histories** (`regen/src/history/tests/combine/fuzz.rs`;
  more with `VARDE_COMBINE_SEEDS`, `VARDE_COMBINE_FROM`): blocks and
  discs on a grid (overlapping and flush), joins, cuts and intersects,
  combines of every operation naming any bodies (used up ones too),
  edits of earlier combines and upstream extrudes, removals, undo and
  redo, combine drafts. After each step the cache warm and cold give the
  same bits; each combine that worked equals the kernel's booleans on
  the bodies the history before it leaves (every step held to the
  boolean identities against the intersection's volume), its tools used
  up into the target or kept as they were and other bodies left alone,
  and one that failed changed nothing; every feature and body can still
  be removed, every feature set as it is and each combine given every
  operation and keep; postcard and MessagePack by name round trip, and
  with bits flipped never panic (what's still taken regenerates and can
  be edited); requests with a combine draft and their answers cross the
  wire as they went.

### UI

**The session** (`app/src/doc/combine.rs`, `Doc::combine`, a
`CombineSession`) is started by `Look::StartCombine` (`B`, the toolbar's
Combine button after Revolve, the rail's Modify set; again, or `Esc`,
cancels it) or by editing a combine (`Look::EditFeature`: a
double-click or `Enter` on its row, Edit combine in its menu), in a
document that can be changed, outside sketches and the other
operations, as the revolve's: editing an extrude or a revolve drops it,
editing a combine drops theirs, entering a sketch drops it, and it
drops the measure tool (entering a sketch whose face is gone, which
asks for a plane first, drops it too). A new one takes its bodies from what's selected
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
  of bodies the document no longer has (`CombineSession::prune`). Bodies
  picked on a model that didn't show a merge yet (a join committed and
  not answered) move on to the body holding them once the model shows
  it, after each edit and answer (`CombineSession::follow`,
  `Doc::follow_merges`), a tool landing on the target taken out, and the
  preview is asked for again: otherwise the combine would fail naming
  them.
- **The highlight** is its own while it's set up, as the measure
  tool's: the target's faces in the selection's colour, the tools' in
  the second colour (a tool the preview uses up has no faces of its
  own), the body under the cursor hovered (`PickIndex::highlight_with`),
  and the body whose row in the panel is hovered lit hovered even if it's
  the target or a tool;
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
- **Random sequences** (`app/src/doc/combine/tests/fuzz.rs`; more with
  `VARDE_FUZZ_SEEDS`, `VARDE_FUZZ_FROM`) start it from `B`, Look and the
  Timeline, pick in the viewport (stale picks too) and Objects, drop
  chips, switch the fields, operation and Keep, cancel, `Esc`, commit,
  undo and redo, add bodies and joins, delete features and bodies,
  replace the document, go read-only, start the other tools mid-way and
  answer late or out of order, checking after every step: no other
  session or plane picking beside it and none read-only, its bodies
  ones it can name, the tools sorted without the target, the panel
  showing the session, the draft last sent the session's, `Esc` leaving
  no trace, a commit one undo step, and once all is answered the error
  shown the newest's.

**The panel** (`view/src/combine.rs`, in `operation_panel`): title "New
combine" or the combine's name; Target and Tools fields
(`operation_panel::pick_field`, outlined in the accent while it's the
one picking, a click on it making it so) holding the bodies as rows
(`picked_row`: the Body icon, the name and a cross taking it out,
`CombineLook::Drop`; dropping the target hands the clicks to it), the
tools one under another, and "Click a body" / "Click bodies" while
empty, "Click bodies" after the tools while they're picked; Operation
tiles (Union, Subtract, Intersect: `BodyOp::label`, the booleans'
circles); "Keep tool bodies" an icon toggle with "Otherwise the tools
are used up" as its tooltip; the foot's draft failing ("Combine fails",
Add anyway), "Checking the sketch…", or with fewer than two bodies
"There’s only one body: make another to combine with". The mock's warnings for a tool
clear of the target are left out: regen answers that case (a subtract
takes nothing, a union is in pieces, an intersect fails as emptying).

The status bar says "New combine · Body 1 with 2 tools · Union" (or
"pick the target body", "Body 1 · pick the tool bodies"), with the hints
"Pick the target" or "Pick tools", `Enter` OK and `Esc` Cancel; the
toolbar's pill "New combine" or "Combine 1" with OK. `B` (`Shortcut::COMBINE`,
`shortcut::combine_binding`, the UI mock's key; the Rectangle tool's in a
sketch) is enabled outside a sketch and the other operations, with two
bodies or more (`Doc::combinable`, `DocumentKeys::combinable`) or a
combine being set up, in a document that can be changed. While it's set
up `I` and the selected feature's `Enter` and `Delete` don't act; `S`,
`X` and `O` drop it for theirs.

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
picks, the rows' crosses through the panel, the draft, Objects' faint
rows, one undo step, `Esc` without a trace, editing, merged bodies
picked as their holder, the highlight, read-only, the other tools, a
body going, the held extrude, the Timeline row, the delete prompt);
`view/src/combine/tests.rs` (the panel's texts and layout); shot
scenario 23.

Known gaps: a kept tool overlapping the target's union shows both
bodies' faces in the same place, which z-fight (as two overlapping
bodies always do); the panel doesn't warn before the preview answers.

## Move and mirror

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

### Regeneration

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

### UI

**The session** (`app/src/doc/motion.rs`, `Doc::motion`, one
`MotionSession` for both and for patterns, its `MotionKind` saying
which; the patterns' own parts are under "Pattern", "UI") is started by
`Look::StartMove` (`M`, `Shortcut::MOVE`, the UI mock's key, the
toolbar's Move after Combine, the rail's Transform set) or
`Look::StartMirror` (no key, as the mock has none: the toolbar's
Mirror after Move, the rail; `Shortcut::NONE`, a binding that's never
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
  sketch's plane does (`MotionLook::OriginAxis`, `OriginPlane`), and the
  viewport picks edges and faces (`Picks::EdgesAndFaces` for a move, a
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
  answer what was asked last ("The model shown is out of date: ...",
  `MeshFeed::answers_request`). Only what a click takes is lit while
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
knobs' layer is an empty placeholder: the handles are drawn by the
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
the screen, an arrow along each world axis (100 px, the extrude handle's
2 px shaft in the axis's scene colour, `Colors::axes`, with its knob at
the end, a 7 px accent disc in a 2 px rim of the points' fill) and a ring
square to each (70 px across, 64 segments, the axis's colour); the one
under the cursor or dragged in the hovered colour. Not for a mirror,
while the axis is picked, without the bodies' box, or in a document
that can't be changed; while the move turns about another axis than a
world axis only the arrows, while about a world axis only its ring
(`MotionState::origin_axis`). Drawn on top of the model, as the axis is.
They take the mouse ahead of picking the model (`Moving::mouse`,
`viewport::Interaction::motion`): over one, the cursor is a grab hand,
the model's hover is let go of and not worked out again until the cursor
leaves it; a press on one grabs it, anywhere else goes to picking and
the camera as before. Hit testing on the screen: an arrow's shaft within
6 px or its knob (the nearest arrow first; one shown shorter than 12 px,
nearly along the view, can't be grabbed), then the nearest ring within
6 px. A drag keeps the centre where it was grabbed (an arrow's moving
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
the ring: 5° at 70 px) and brought within a turn, and sends
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
checks (`VARDE_MOTION_SEEDS`, `VARDE_MOTION_FROM`).

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

## Pattern

`crates/document/src/pattern.rs`.

```rust
pub struct Pattern {
    pub bodies: Vec<BodyId>,                 // as a move's
    pub kind: PatternKind,
    #[serde(default)] pub copies: Copies,    // "Join to original"
}
pub enum PatternKind {
    Linear { along: AxisRef, count: Value, spacing: Value },   // count 2..=1024 whole; spacing a length, not 0
    Circular { about: AxisRef, count: Value, angle: Value },   // angle above 0, at most a turn
}
pub enum Copies { Joined /* default */, Separate(Vec<BodyId>) }  // the copy bodies, by copy then body
```

- **What it is**: a timeline step on bodies already made, as a move is:
  `FeatureKind::Pattern` (the seventh variant on the workers' postcard;
  `Linear` 0, `Circular` 1), "Pattern N", no body made, no regions.
  Body patterns only (decided). Each body becomes itself and `count − 1`
  copies of itself (the count includes the original), **kept in the
  body** by default (`Copies::Joined`, the panel's "Join to original"
  ticked; decided, and the user's later word made it an option): side
  by side where apart, united where they meet. Unticked
  (`Copies::Separate`) **each copy is a new body of its own** (below).
  The body keeps its id and its own faces their names; copy `k`
  (`1 ≤ k < count`) names its faces as `FaceName::copy(pattern id, k)`
  (instance `mix(parent instance, feature, k)`, so a pattern of a
  pattern's or a mirror's copies stays unique), joined or not, and
  later features name a copy's faces by those keys (on the copy's own
  body when it has one).
- **Copy bodies** (`Copies::Separate`, the user's request after the
  plan; a defaulted stored field, so files from before read as joined;
  no version bump). Ordinary document bodies ("Body N", visible, made
  by the pattern: `Body::created_by` is the pattern), listed in the
  pattern in a fixed layout: copy `k` of its `i`th body (in its sorted
  bodies) at `(k − 1) · n + i`, `n` the body count
  (`Pattern::copy_body`, `copy_bodies`), so a higher count adds bodies
  at the end. **Identity**: `Command::AddFeature` and `SetFeature` lay
  the list out whatever it held (as an extrude's `BodyId::NEW`,
  `editor.rs`'s `planned_copies`): each copy, by its original's id and
  its `k`, keeps the body the feature it replaces made of it (a spacing
  or axis edited, a count raised, a body added or taken out, the kind
  swapped between linear and circular: the copies left keep their
  bodies), the others get new ids and names numbered on from the
  bodies; bodies of copies it no longer makes (a lower count, a body
  taken out, ticked again, set to another kind) are removed and dropped
  from excluded lists, **refused while a later feature names one** (as
  an extrude stopping making a body a combine names;
  `Document::copies_dropped` says which, for the panel). Ticked again
  and unticked, the copies get new bodies. At most `MAX_PATTERN_BODIES`
  = 1024 copy bodies per pattern (bodies × (count − 1), a checked
  product: each is an Objects row; joined, only the count is bounded).
  Separate bodies **may overlap** each other and their originals: they
  are never united (the panel warns, below).
- **Placement**: linear copy `k` is moved `k · spacing` along the
  axis's direction (only the direction counts; a negative spacing runs
  the other way, which is how a panel's Flip is stored). Circular copy
  `k` is turned right-handed about the axis by `k · span / steps`
  degrees: **a whole turn shares its ends** (angle 360°, or any that
  comes to a turn within the rounding of degrees, `Pattern::full_turn`:
  span 360, steps = count, so 4 copies are 90° apart, exactly), and **a
  span short of a turn has a copy at each end** (span = angle, steps =
  count − 1: 3 over 90° are at 0°, 45° and 90°). This was open in the
  plan (its kernel helper spread the count over the span either way);
  the mock's Circular pattern has Full 360°, Spacing (one step) and
  Total (the whole arc, a copy at each end) modes, which this stores
  as: Full → 360, Total → the angle, Spacing → spacing × (count − 1)
  (the mock refuses that reaching a turn). `Pattern::span_steps` /
  `step_degrees` give it. Each copy's motion is made directly
  (`Motion::pattern_step`, `Motion::pattern_turn(point, axis, span, k,
  steps)`), never by composing steps.
- **Axes**: `AxisRef` as a move's turn has it (origin axes, straight or
  round model edges, round faces, found on the bodies as the features
  before it leave them).
- **Checks** (`CheckError::Pattern(id, MotionError)`):
  `Pattern::check_own(design)`: bodies as a move's (`Bodies`,
  `BodyOrder`), the count by `Pattern::count_ask` (`Ask::number(…,
  1024).whole().at_least(2)`: `Count`), a linear spacing by
  `Pattern::spacing_ask` (a length within `MAX_COORD` of zero) and not
  zero (`Spacing`), a circular angle by `Pattern::angle_ask` (above zero,
  at most a turn: `Angle`), unjoined at most `MAX_PATTERN_BODIES` copy
  bodies (`Separate(n)`), the axis's own parts (`Edge`, `Near`).
  `Document::check` then wants the bodies made before it (`Body`) and
  the axis's body and makers before it (`RefBody`, `RefMaker`), as a
  move's, and its copy bodies as laid out (`CopyBodies`): joined, no
  body made by it; unjoined, one listed per copy, none repeated, each a
  body whose `created_by` is the pattern, and every body made by it
  listed (bodies made by a pattern are counted in the body loop, so the
  check stays linear). `Pattern::count()` reads a checked count as a
  `u32`.
- **Removal and units** as a move's: `FeatureKind::bodies()` lists the
  bodies; `SetUnits` pins the spacing by its ask (the count and angle
  have no length unit to pin, so stay as typed). Copy bodies are the
  pattern's (`created_by`), so **removing the pattern removes them**,
  and every later feature naming one (`Document::removal`); removing
  one copy body removes its maker, the pattern, with all its copy
  bodies, as removing an extrude's body removes the extrude.

### Regeneration

`crates/regen/src/history/pattern.rs`, in history order:

- Bodies with no solid of their own fail it (`own_solids`); the axis is
  resolved as a move's (`resolve_axis`, cached) and noted in
  `Evaluation::references` for the draft (the point and direction it
  placed by).
- **Bounded before anything is copied**, per body: `count × patches`
  by a checked multiply (`copies_fit`) within `MAX_PATCHES` ("1024
  copies of Body 1 are too many to work out: it has N patches, and a
  body may have 4194304 in all; use fewer copies"), and every copy's
  box within `MAX_COORD` (each copy is checked: a circular pattern's
  farthest copy isn't its last) ("patterning Body 1 takes it out of
  range ...").
- **Copies**: `Solid::transformed(motion_k, Some(Instance { feature:
  pattern id, index: k }))` each, then `assemble([body, copy 1, ...])`
  (side by side where boxes and hulls are apart, balanced unions where
  they meet). The whole is cached under `pattern_key(body key, pattern
  id, every copy's Motion::bits, fit)`: an edit leaving the motions as
  they were (a spacing typed another way) finds it again. Kernel
  failures: "patterning Body 1 ..." (`message::moving`), "joining Body 1
  to its copies ..." (`message::with_copies`), with the kernel's
  evidence by value, as a move's. All bodies are worked out before any
  changes.
- **Unjoined**, the same bounds hold (`count × patches` within
  `MAX_PATCHES`, which also bounds the copies' memory together), the
  bodies are left as they are, and each copy is
  `Solid::transformed(motion_k, Some(Instance { feature, index: k }))`
  alone, the solid of its body (`Pattern::copy_body`), never assembled
  or united with anything; cached as a mirror's image is
  (`moved_key(body key, motion_k bits, the instance, fit)`), so a count
  raised finds the copies made before. The copy bodies go into
  `Evaluation::bodies` after the others, in id order (the order made:
  a later join touching copies merges them into the first made, the
  original if it touches it too). A failing pattern leaves its copy
  bodies with no solid: a later feature naming one fails as on any
  body with none.
- Nothing merged or touched, as a move. Later joins, cuts, combines,
  moves, mirrors and patterns take copy bodies as any body (the app's
  `merged_before` follows them merged, from the touched lists and
  combines, unchanged).
- **Naming** (`varde_view::Naming`, `plane_pick.rs`): a face made on
  body `B` by its maker and copied by an unjoined pattern is on that
  pattern's copy body, not on `B`. `Naming::before` keeps, per unjoined
  pattern before the stop (`Separated`), the body each body whose faces
  it copies holds them in (its sources, and the bodies merged into them
  before it, by `merged_before` as replayed so far) and its copy
  bodies; `instances_before` keeps how each copy instance was made
  (parent instance, feature, index: `Copied`). `Naming::body_of` walks
  a face key's instance back to the original and on through each
  unjoined pattern it passed, so a face of a copy of a copy is named on
  the last copy body, even where a later join merged that into the
  original. `made` (the body a feature made, by its number) lists only
  extrudes' and revolves' new bodies. Past `MAX_INSTANCES` copies the
  body of a copy can't be told: with any unjoined pattern before the
  stop such a face is refused as unclear.

Tests: `document/src/pattern/tests.rs` (checks, spacing rules, removal (the
axis's body removed leaves the pattern),
units, postcard and hostile values read back), `document/src/pattern/
tests/separate.rs` (copy bodies laid out and named, kept by copy across
spacing, count, body and join edits and undo, a later combine naming one
refusing the edits that drop it, removal both ways, the limit, wrong
lists refused from postcard), `regen/src/history/tests/pattern/
separate.rs` (pins patterned unjoined, overlapping, each copy a pin of
its own with its wall named as its copy; the tick toggled and undone;
a combine cutting a copy body from a plate, a ring about a copy's wall,
a join merging copies into the original; the cache across a count
raised), `regen/src/history/tests/pattern.rs` (a row of pins against its volume and box, names per copy, a
negative spacing; copies end to end along the pin's own round face
united into one; a ring of pins, whole turn exact at quarter turns and
three over 90° by its centre of mass; three bars through a hub against
the inclusion–exclusion area; a pin grid cut from a plate in one
difference, 4 × 4 and 10 × 10, timed; a copy's face as a later axis,
kept by its name as the count changes, and a copy that isn't there; a
pattern of a pattern 64 by 64, an edge of the last copy of the last
copy found; discs touching along a line refused, a hair closer or
farther right; copies on the original to the bit; a ring about a disc's
wall near the coordinate limit; out of range; the patch bound with
overflow; the cache), `regen/src/wire/tests.rs` (a draft and its axis),
`io/src/vrdp/tests.rs` (through a file; tampered records refused), and
the motion fuzz (`motion/fuzz.rs`, `VARDE_MOTION_SEEDS`): random linear
and circular patterns of 2 to 4 copies about origin axes, edges and
round faces, half of them of bodies patterned already (copies of copies),
each copy's centre where `glam` places it and the whole as the copies
united one by one; a third of them unjoined, the originals left alone
and each copy body the copy itself where `glam` places it, later
features (moves, mirrors, patterns, combines, joins, faces as axes)
then taking copy bodies like any, and edits dropping a copy body a
later feature names refused.

Known gaps: the 10 × 10 grid of pins (100 holes) cut from a plate in
**one** difference runs out of work (`MAX_WORK`) in about 2 s (release);
the difference's cost grows faster than the pins (a 7 × 7 grid takes 2
to 3 million of the 4.2 million units, from 8 × 8 on it fails), so hole
patterns past about 50 holes fail with "too complex" until the boolean's
work is local to the change. A combine step running out of work with a
tool of more than one piece says so rather than blaming tangent faces
(`message::combining`, the pieces counted by `combine::shells` only
then): "cutting Body 2 from Body 1 is too complex to work out at once:
Body 2 is 64 separate pieces; use fewer, or split them over more than
one combine". Cutting the 100 pins one at a time works
but takes about 40 s. Copies that only touch (discs a diameter apart)
fail to join ("leaves no clean solid"), as any union touching along a
line does: the kernel's `NotManifold`, right, since the result would
touch itself along the line. Copies a hair apart (a spacing of 1e-9,
a turn of 1e-7° about a far axis) are refused as nearly flush, and
from about 1e-4 to 1e-3 mm apart a union of them can take seconds to
run out of work. `Naming` takes faces of patterns' copies made before the
feature (`instances_before` replays patterns as it does mirrors), up to
4096 copies in all (a pattern of a pattern 64 by 64; a mirrored row of
1024 mirrored again); past that it takes a face of any copy, and one
that isn't there yet at the feature fails it when regenerated ("its axis
face wasn't found") rather than being refused as it's picked.

### UI

The move's session (`MotionSession`, above) with `MotionKind::
LinearPattern` and `CircularPattern`, following the UI mock's
`lpattern` and `cpattern`.

- **Starting**: `Look::StartPattern` (`P`, `Shortcut::PATTERN`, the
  mock's key, outside sketches where `P` is the Point tool's; the
  toolbar's "Pattern" after Mirror; the rail's Transform set as "Linear
  pattern") and `Look::StartCircularPattern` (no key, as the mock has
  none: the rail's "Circular pattern"; not on the toolbar, which the
  mock's model bar leaves it off too and which is nearly full at
  1280 px), again backing out, the bindings as Move's
  (`pattern_binding`, `circular_pattern_binding`); or editing one
  (`Look::EditFeature`: double-click, `Enter`, "Edit pattern"). Bodies
  as a move's (the selection, else the only body; picked in the
  viewport or Objects; merges followed; gone bodies said "A picked body
  is gone"). A new linear one starts as the mock's: the X axis, count
  3, Spacing 100 (of the design's units); a circular one the Z axis,
  count 4, Full 360° (its angle field holding 90° for the other modes).
  The count field (`VALUE_FIELD`) takes the focus.
- **The axis** (linear: "Direction") is picked as a move's
  (`MotionPick::Reference`: the toolbar's origin axes, a straight or
  round edge or a round face named by `Naming`, the neutral preview of
  an edited one while it's picked; a linear one refuses others with
  "Only a straight or round edge, or a round face, can give the
  direction"), and drawn as a move's axis, always (a flipped linear
  one's arrow pointing the way its copies go). Gone: "The direction is
  gone: pick another" / "The axis is gone: pick another" (an undo took
  its body or maker away, or the body was removed: the pattern stays
  and fails until another is picked); while another is picked for an
  edited one, the neutral preview leaves the gone axis (and any gone
  body) out, so the model shown is still the bodies at the pattern.
- **Fields and modes** (`MotionField::Count`, `MotionField::Spread`,
  `MotionLook::Mode(PatternMode)`, `MotionLook::Flip`): the count by
  `Pattern::count_ask`; the spread a positive length within the
  coordinate limit (linear: `spacing_ask().positive()`, the sign is
  Flip's) or an angle by `Pattern::angle_ask`. Stored
  (`MotionSession::pattern`): linear Spacing as typed, Total as
  `"(total) / (n − 1)"`, Flip as `"-(...)"` round either (a negative
  spacing); circular Full 360° as `"360°"`, Total as typed, Spacing as
  `"(spacing) * (n − 1)"`. Texts, not rounded numbers, so the values are
  exact and follow unit changes; where the text wrapped would be past
  what an expression holds (`MAX_LEN`, `MAX_DEPTH`: a spread typed near
  the 256 bytes), the value it comes to is stored instead, written
  exactly (`varde_expr::exact`), rather than refusing a text that reads. The mock's errors under the field
  (`MotionState::spread_error`, OK off): "The pattern runs past 1000000
  mm" (spacing × (n − 1) past `MAX_COORD`; the mock's limit is its
  10 000 mm), "4 copies 120° apart go past a full turn" (a Spacing
  coming to a turn or more, within the rounding `full_turn` allows),
  "A whole turn puts the last copy on the first: use Full 360°" (a
  Total of a turn).
- **Mode kept**: the stored values can't tell a Total from a Spacing
  (or a circular Spacing from a Total), so `Doc::pattern_shapes` keeps,
  per pattern committed in this run, the mode, Flip and spread as typed
  (`PatternShape`); editing takes them again only if they still give
  the values stored (an undo, another edit or another document leave
  them out). Otherwise the rule: a linear pattern opens in Spacing with
  its spacing's size (Flip on for a negative one, its text with the
  sign taken off where that gives the value exactly, else
  `"-(text)"`, else, too long for that, the size written exactly); a
  circular one in Full 360° for a whole turn, else in Total with its
  angle. The shape is kept under the feature edited, or the one added;
  one whose mode the pattern's kind doesn't offer (a file swapped the
  kind) isn't taken. While the mode, Flip and spread field are as an
  edited pattern opened with them, the pattern keeps the spacing or
  angle it stores, text and all (`Opened`): OK with nothing changed
  writes nothing even where the session would write the same value
  another way ("-15" read with Flip on comes back "-(15)", "360" in
  Full 360° as "360°").
  An undo setting another kind of feature in the edited one's place
  (`MotionKind::of` no longer the session's) ends the session.
- **Preview, OK, status**: the draft is the pattern as set up (none for
  a new one while its axis is picked); OK one undo step ("Pattern N");
  the status bar says "New linear pattern · Body 2 · 4 × 12 mm along X
  axis · joined" (", flipped"; " · joined" while ticked), "· 4 × 90°
  about Z axis" (`varde_view::pattern_copies`, by
  `Pattern::step_degrees`), or "pick
  the bodies to pattern", with "Pick the direction" / "Pick the axis".
- **The panel** (`view/src/motion.rs`): title "New linear pattern" /
  "New circular pattern" with the mock's `lpattern` and `cpattern`
  icons (`Icon::LPattern`, `CPattern`); Bodies; Direction (linear) or
  Axis; Flip direction (`Icon::TkFlip`, linear only); "Copies": Count,
  the mode tiles (`lp-spacing`, `lp-total`; `cp-full`, `cp-spacing`,
  `cp-total`: `Icon::LpSpacing`, `LpTotal`, `CpFull`, `CpSpacing`,
  `CpTotal`), the Spacing or Total field (none for Full 360°), and Join
  to original.
- **Join to original** (`MotionLook::Join`, `MotionSession::join`):
  the mock's tick on both panels, last, with its icon (`Icon::TkJoin`,
  the mock's `tk-join`) and note "One body; otherwise each copy is its
  own". **Ticked to begin with** (the user's decision; the mock starts
  unticked); an edited pattern opens as it's stored. Unticked, the
  pattern goes out as `Copies::Separate` of no bodies, which the
  document lays out (keeping an edited one's copy bodies). Where the
  edit would drop a copy body a later feature names (fewer copies, a
  body taken out, ticked again), the panel refuses it at once
  (`Doc::motion_held`, OK off): "Combine 1 uses Body 5, a copy this
  pattern would no longer make: take Body 5 out of Combine 1 or delete
  it first". **The overlap warning** (the mock's, linear only, as the
  mock has it, `Doc::motion_warning`): unticked, with a spacing shorter
  than the bodies are long along the direction ("The copies overlap (10
  mm long this way): tick Join to original to merge them"), the length
  from their faces in the model shown (`PickIndex::bodies_extent`, the
  mesh's points; the direction an origin axis's or, for an edge or
  face, the draft's reference). It doesn't stop OK (separate bodies may
  overlap) and shows only where the panel has no other message (the
  mock shows it under a failure too).
- **Timeline**: the mock's icons by kind, the note "×4"; selected, the
  status bar says "Body 1 · 4 × 25 mm along X axis, flipped · joined",
  "Body 1 · 6 × 60° about Z axis" (`pattern_info`, the mock's " ·
  joined" for joined copies; the session's status text too). Copy
  bodies show in Objects as any body.

Tests: `app/src/doc/motion/tests/pattern.rs` (`P`, typing a count and a
spacing previews the row, `Enter` one undo step, the note; Total stores
its spacing, Flip turns it, the mode kept on editing and the rule
without it; a circular Full 360° against the copies' box; the mock's
errors past a turn and the limit; editing from the Timeline and undo;
spreads too long to wrap stored as their values, and a stored spacing
whose text turned is too long; an undo swapping the kind; a direction
gone by an undo and by its body's removal, the neutral preview without
it; the axis's body and a picked body taken mid-pick; OK on a pattern
opened writing nothing however it was typed), `pattern/separate.rs`
(the tick ticked to begin with, unticked a body per copy committed,
listed and undone, the overlap warning and when it goes; editing the
tick, and a later combine's copy body holding back fewer copies or
joining), `pattern/fuzz.rs`
(`VARDE_PATTERN_SEEDS`: random modes, Flip, counts, long and nested
spreads, units, undo and redo, kind swaps, commits, edits held to the
values their fields come to);
`view/src/motion/tests.rs` (both panels' rows and modes, the infos).

Departures from the mock: "Join to original" starts ticked (the mock's
starts unticked; the user's decision), and its overlap warning shows
only without another message; counts up to 1024 (the mock's 100)
and the run limit the coordinate limit (the mock's 10 000 mm), with the
fields' own error words for other refusals; the axis may also be a
round edge or face, as a move's; no faded originals (the preview
replaces the model); Circular pattern isn't on the toolbar (the rail
has it; the mock's body bar does, the app has no such bar).
