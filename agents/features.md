# Features

The features after sketches and extrudes (revolves, combines, moves, mirrors, patterns, aligns, scales, splits, chamfers, shells, fillets, offset faces,
drafts, sweeps and lofts), and sketches' planes on faces:
their document types, checks and commands, how regeneration evaluates them, and their UI. Extrudes are
described in `agents/kernel.md` ("The extrude feature" and "The extrude UI"); what
they share with the newer kinds is here. The kernel math of each is in
`agents/kernel.md`.

## Commands shared by every kind

`crates/document/src/editor.rs`.

- `Command::AddFeature { name, kind: Box<FeatureKind> }`, made by
  `Document::add_feature(kind)`, which names it one past the highest
  "Extrude N", "Revolve N" ... (`FeatureKind::noun`). It hides the
  sketches whose regions the feature takes (`FeatureKind::profile_sketches`:
  `FeatureKind::sketch`'s, or a loft's sections' sketches); one whose
  operation is `NewBody` also adds "Body N" with the next id, replacing
  whatever id the command held (`BodyId::NEW`); a pattern whose copies
  are bodies of their own adds one per copy, laid out as "Pattern"
  says; a split keeping both pieces adds one for its other piece
  (`Split::new_body`, filled in as its `keep` says, see "Split"). One
  undo step. A sketch
  kind is refused (`EditError::SketchKind`): sketches are added empty by
  `AddSketch` and set by `SetSketch`.
- `Command::SetFeature { feature, kind }` replaces a feature's kind,
  keeping its id, name and visibility. A missing feature is a no-op; a
  sketch feature, or a sketch kind, is refused (`EditError::SketchKind`).
  The kind may change (an extrude may become a revolve): what matters is
  the operation. A `NewBody` that stays one keeps its body (whatever id
  the command held; a split's new body likewise, `FeatureKind::new_body`
  / `new_body_mut`); one that stops removes the body and drops it from
  the other features' excluded lists, but **holds its id**
  (`FeatureKind::held_body`: a join's, cut's or intersect's
  `Targets::held`, a split's `new_body` while it keeps a side); one that
  starts gets the body back with the id it held (in its place in id
  order, named "Body N" afresh), else adds one with a new id. So what
  was named on the body (a sketch on its face, a later feature's
  reference) finds it again across edits (New body, Join, New body; a
  split keeping both, one side, both). The commands fill the id in
  whatever they held (`planned_new_body`: the old feature's body or
  held id, carried across kinds that have the slot, else `BodyId::NEW`
  for one to make; `AddFeature` never holds one), so a panel needn't.
  The document's check wants every held id below the next id, no body's
  and held by one feature only (`CheckError::Held`), so features added
  meanwhile never get it. A pattern's copy bodies go the same way, copy
  by copy (see "Pattern"), but aren't held.
  The caller passes regions referenced afresh from the sketch as it is.
  Setting what's already there changes nothing (no new revision).
- Both run the document's whole check on the result, and then
  `check_new` on the added or set feature: what's required of a feature
  when the user makes or edits it, but not of one already in a document
  (a later edit of its sketch may break it, which regeneration reports).
  Today that's a revolve's axis line, a split's line's curves, a
  sweep's path's curves and a loft's start points, point sections and
  rails' curves.
- `FeatureKind` helpers: `noun`, `sketch` (the profile sketch, or a
  split's regions' or line's sketch; a sweep's profile's, not its
  path's, so adding a sweep hides only its profile's sketch),
  `operation` / `new_body` (an extrude's or revolve's `Operation`, or a
  split's new body; a body's maker is checked by `new_body`, or a
  pattern's copy bodies),
  and `uses`, now a **list**
  (sorted, no repeats) of the features this one builds on (a sweep's
  profile's sketch and its path's sketches), which
  `Document::removal` follows. Every extrude-only path that only cared
  about the operation (`drop_excluded`, the app's delete prompt and its
  join merges) goes through `operation()` so revolves get them too.
- `SetUnits` pins revolve angles by `Turn::ask` as it pins extrude
  distances by `Extent::ask` (angles' bare numbers are degrees whatever
  the units, so only lengths inside an angle's expression change).
- Kinds are **appended** to `FeatureKind` (`Sketch` 0, `Extrude` 1,
  `Revolve` 2, `Combine` 3, `Move` 4, `Mirror` 5, `Pattern` 6, `Align` 7,
  `Scale` 8, `Split` 9, `Chamfer` 10, `Shell` 11, `Fillet` 12, `OffsetFace` 13,
  `FaceDraft` 14, `Sweep` 15, `Loft` 16): files store a kind by its variant name, and
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
  none; extrude, revolve and loft: none. With no operand face on a body that
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
    or round show themselves, as above; see "Move and mirror"); an
    align's reference gone, on a body that's gone or merged into the
    moved body, or its point out of range (one of the wrong kind, and a
    secondary parallel to its primary, show themselves: see "Align");
    a scale's point or edge not found, its edge too short, its length
    too far from the edge's, its edge not straight or not along an axis
    for a scale along it, a factor out of range, a body scaled out of
    range (see "Scale"); a sweep's path not found, its sketch not
    placed, its edge or its edge's body gone, its parts apart, a closed
    part with others, its start off the profile's plane or not square
    to it, its helix too long (a corner shows its joint: see "Sweep").
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

## Sketch links

A sketch's links (`agents/sketch.md`, Links) take geometry from outside
it: another sketch's point or curve, or a model edge, face or corner,
projected onto its plane or cut by it. The sketch holds what each link
made; the feature holds where each comes from (`FeatureKind::Sketch::
sources`, a `LinkSource { link, source: OutsideRef }` per link, in the
links' order). `OutsideRef` (`document/src/outside.rs`) is another
sketch's feature and item ids, or an `EdgeRef`, a `FaceRef` or a
`PointRef::Corner`, named as features name them; `OutsideRef::takes`
says which a link's kind can take (Project: edges, faces, corners,
sketch items; Intersect: faces, edges). It lives in the document, not the
sketch, because `varde-sketch` can't name the model (`document` depends
on `sketch`); the sketch holds only what the solver and editing need.

**Checks** (`Document::check_sources`): one source per link, in order,
of a kind it takes, its own parts right (`EdgeRef::check_own` and the
rest), and what it names made before the sketch, or, as a sketch's face,
not there with ids below the next id: a removed source leaves the link,
which then doesn't find it (`Document::removal` doesn't follow links). A
sketch source is a sketch feature before it; whether it holds the item is
regenerating's to find. `CheckError::SketchLink(feature, LinkError)`.

**Commands**: `Command::AddLink { feature, sketch, source }` sets the
sketch (with a new link) and records the link's source;
`Command::SetSketch` keeps the sources of the links the new sketch still
has and drops the rest, so deleting a link (a sketch edit) drops its
source; a sketch with a link the feature has no source for is refused.
`Editor::amend` applies a command folded into the change before it:
undo takes both back at once, the redo history stays, and the document
gets a new revision and generation.

### Regeneration

`regen/src/history/link.rs`, run in `walk` at each sketch with sources,
before it's listed among the sketches later features use. Each source is
found where the sketch is in the history (`link::find`): another sketch's
item on that sketch as it is, at its placement, projected through the
affine map from its plane into this one's (`Sketch::project_item`); a
model edge, face or corner on its body among those the features before
made, through `Evaluation::holder` and then the splits before (`on_body`,
as `place_on_face` follows a sketch's face), so a later split moving it
onto another body doesn't lose it, and a face or edge isn't looked for in
the final model. A model edge projected is sampled along its conics
(`varde_kernel::section::sample`, 32 places a conic) and fitted
(`LinkShape::fit`, to the resolution, splines within the fit tolerance);
a corner is a point; a face projected is its outline (`link::outline`):
every edge of its region with another region beside it, outer loop and
holes, sampled and projected so, fitted together as one link; an edge
intersected gives the points where it crosses the plane
(`section::crossings`), a face the curves where the plane cuts it
(`section::face_section`), fitted likewise, or its outline where it
lies in the plane. Like every link, a face's follows the model: its
loops changing (moved, resized, a hole added or gone) make the link
stale and relinked (below). What it finds
is cached by the solid's key, the source, the link's kind, the sketch's
placement and the tolerance (a sketch source by that sketch's key and
both placements). Found nothing is a broken link, with why (the words are
`link.rs`'s constants: its sketch isn't there or isn't placed, it isn't
in its sketch any more, the sketch isn't placed, its body is gone, its
edge, face or corner wasn't found, its edge doesn't cross the plane or
lies in it, the plane doesn't cut its face, too complex,
or a fit's refusal); it keeps what it holds.

`link::relink`: a link whose shape found isn't what it holds, to the
resolution, as relinking would give it (`Sketch::link_follows`), is stale; the sketch is proposed with
every stale link relinked (`SketchEdit::Relink`), solved so what's tied to
them follows, or relinked unsolved if it doesn't solve, cached by the
sketch's key and the shapes. That sketch goes into `Evaluation::relinked`;
broken links into `Evaluation::broken`. The regeneration itself goes on
with the sketch as the document holds it. The answer carries both
(`Response::Regenerated::relinked`, none with a draft: only the committed
document's links are followed; `broken`), and so does the wire
(`Head::Regenerated`, the sketches checked by the document's check when
the app folds one in; decoded within bounds, each list within a sketch's
limits and all of them within `wire::MAX_RELINKED_ITEMS`, the broken
links within `wire::MAX_BROKEN`, so a few bytes can't make the page
allocate far more; relinked sketches past the bound aren't sent, nor
broken links past theirs; and the relinked dropped, with the failures'
geometry, when the head is too large).

### Following the model (`app/src/doc/relink.rs`)

The app folds what regenerating found into the document: the answer's
relinked sketches are kept by `MeshFeed` with the generation they're of
and taken once (`MeshFeed::take_relinked`), only if that's the editor's
generation now (an older answer's document is gone; the next answer is
of the new one) and the document can be changed; each is set by
`Editor::amend`, folded into the change it follows from, so moving a
source and the links following are one undo step: undo takes both back
(the sketch then holds what it held, in step with the model it was
made with), redo gives both again, nothing waits on it, and following
never makes a step of its own, so it never fights undo or the redo
history. It's a new revision, so the file has unsaved changes, and a
proposal in flight is proposed again on it. A link just added holds
nothing; the next answer fills it, folded into the change that added it.
A link whose shape changed keeps what ids it can (`agents/sketch.md`,
Links); the constraints and dimensions on what it no longer makes go
with it, which the status bar says ("Sketch 2's links changed shape,
removing 1 constraint on what they no longer make").
Broken links are only shown (`MeshFeed::broken`), never written: a link
keeps what it last found, and finds it again once its source is back.
A relinked sketch the document refuses is left as it was, the links it
would have changed (all of them if none) shown broken with why ("What it
found was refused: ..."; `MeshFeed::mark_broken`) until the next answer;
the document unchanged, nothing more is asked, so it doesn't loop.
Only the committed document's links are followed: a draft's answer
relinks nothing. A file whose links are out of step with the model
(saved by an older build, say) is put in step once it's opened, with no
undo step.

The sections themselves are `varde_kernel::section`'s
(`agents/kernel.md`, Plane sections), called with the resolution as the
distance within which something lies in the plane and pieces weld.

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
  ignore it. The panel keeps a flip set under one side or two sides
  while another extent is picked, for switching back, but stores it only
  where the extent takes it: under one that ignores it, the flip the
  edited feature stored under such an extent, else none, so OK after
  flipping and switching back writes no revision (extrude's Symmetric
  and Through all alike; `stored_flip` in `doc/extrude.rs` and
  `doc/revolve.rs`).
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
`refresh_work()`, the regions picked and their references, found again
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
  `VARDE_TESTS=full`, one replayed with `VARDE_TEST_SEED`) drive both sessions with picks,
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
the sketch…". The angles are dragged by the handle in the viewport too.

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
sides (as the revolve's `span` does). **The handle**
(`RevolveState::handle`, `RevolveHandle`; drawn and hit tested as the
extrude's, `viewport/handle.rs`, see `agents/viewport.md`): once there
are regions and an axis, and the regions' area-weighted centre is off
the axis, a knob for each angle the extent has (none for a full turn),
at that centre turned about the axis to the angle: one side's at its
angle (negative flipped), symmetric's at half of it, two sides' one
each way (swapped flipped), as `span` turns. Each is a puck whose ring
lies in the end face it's on and whose arrow points along the way the
centre turns, on round past that end; its shaft is the arc round the
axis from the sketch plane to it, on the screen over the model (none
while the revolve's own check refuses it, as the extrude's), and its
rail, hovered or dragged, the arc on round either way, half a turn at
most. The handle is drawn over the model, so a knob on an end turned
away from the camera shows and grabs too. Pressing one sends
`RevolveLook::GrabHandle`; while it's grabbed the cursor's moves send
`DragHandle` with the angle about the axis where its ray meets the
plane through the centre square to the axis, the nearer way round from
the knob (so it turns on past half a turn), snapped as a move's ring
(`angle_step`: 1, 2, 5, 10, 15, 30, 45 or 90°, the first at least 6 px
along the arc), nothing with the plane edge on; letting go sends
`DropHandle`. The app types the angle into the field as degrees
(`RevolveSession::drag`): one side's how far round either way, back
past the plane flipping it; symmetric's twice it; two sides' each how
far its own way; an angle the field refuses (none, past a turn)
changes nothing, nor does a drag taking two sides further over a turn.
The labels' layer under the panel is an empty
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
accent with its icon and name ("New revolve" or "Revolve 1"; a name
past 32 characters, a file's, cut short with "…" so the bar fits at
1280 px) and OK
joined to it (disabled while OK waits), then Cancel (`Esc`) and only
what the operation picks (the origin axes or planes): as the mock's, not
the other operations, which left no room for those at 1280 px (their
keys still act); the extrude, the combine and the move's sessions
alike. While a revolve is set
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
  more with `VARDE_TESTS=full`, one replayed with `VARDE_TEST_SEED`): blocks and
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
  `VARDE_TESTS=full`, one replayed with `VARDE_TEST_SEED`) start it from `B`, Look and the
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
combine as its target then its tools); a combine naming a body merged in
before it merges nothing, as regen fails it, even before the model shown
knows (a session's bodies and edges don't follow into it): the extrude's and revolve's
Bodies lists and the combine's picking use it, and the delete prompt's
warning replays combines alike. `feed/tests.rs`'s
`merged_before_agrees_with_regen_on_random_histories` holds the two to
the same answer on random histories of bodies, joins, combines (some
naming used-up bodies, so failing) and edits of earlier combines, for
the whole history and stopped before each feature
(one seed by default, 4 with `VARDE_TESTS=full`).

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
  bodies, as removing an extrude's body removes the extrude. The app
  asks first there (`Doc::remove_now`): a body goes quietly only where
  nothing but its own feature goes with it, and a copy body takes the
  pattern's other copy bodies, which the user didn't pick; deleting the
  pattern itself takes its own bodies without asking.

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
  or united with anything; cached by `copy_key(body key, motion_k
  bits, the instance, fit)`, so a count raised finds the copies made
  before (a key of its own: `moved_key` with a copy files the solid
  assembled with its image, a mirror's). The copy bodies go into
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
lists refused from postcard; renamed, hidden and see-through copy
bodies kept across edits, units and a kind swap), `regen/src/history/tests/pattern/
separate.rs` (pins patterned unjoined, overlapping, each copy a pin of
its own with its wall named as its copy; the tick toggled and undone;
a combine cutting a copy body from a plate, a ring about a copy's wall,
a join merging copies into the original; the cache across a count
raised, for two bodies too; unjoined rings whole turn and part of one
by each copy's volume and centre), `regen/src/history/tests/pattern.rs` (a row of pins against its volume and box, names per copy, a
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
overflow; the cache), `regen/src/wire/tests.rs` (a draft and its axis;
a draft's list of copy bodies laid out again whatever it held, its
length tampered refused), `io/src/vrdp/tests.rs` (through a file;
tampered records refused, a copy list's length, ids and the count
among them), and
the motion fuzz (`motion/fuzz.rs`, `VARDE_TESTS=full`): random linear
and circular patterns of 2 to 4 copies about origin axes, edges and
round faces, half of them of bodies patterned already (copies of copies),
each copy's centre where `glam` places it and the whole as the copies
united one by one; a third of them unjoined, the originals left alone
and each copy body the copy itself where `glam` places it, later
features (moves, mirrors, patterns, combines, joins, faces as axes)
then taking copy bodies like any, and edits dropping a copy body a
later feature names refused; earlier patterns changed one way (count,
tick, kind swapped, a body added or taken out), each copy keeping its
body, name and visibility, new ones named apart; copy bodies hidden
or removed with their pattern.

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
a turn of 1e-7° about a far axis) are refused as nearly flush or, the
booleans having improved, united into the right solid (a pin 1e-9
apart from its copy now unites), and
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
  toolbar's "Pattern" after Move; the rail's Transform set as "Linear
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
  it first". No draft is sent while that holds (`Doc::motion_draft`,
  `copy_user`): neither the refused pattern nor the move by nothing
  previewing an edited one while its axis is picked (which has no copy
  bodies), so the model shows as committed rather than as "Pattern
  fails" over a later feature's missing body. **The overlap warning** (the mock's, linear only, as the
  mock has it, `Doc::motion_warning`): unticked, with a spacing shorter
  than one of the bodies is long along the direction, so its own copies
  overlap ("The copies overlap (10 mm long this way): tick Join to
  original to merge them"; each body on its own, so two bodies far
  apart whose copies miss aren't taken as one long one), the length
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
joining, and sending no draft while the direction is picked; too many
copy bodies refused; the warning for two bodies apart, in inches,
flipped and along the plate's edges; deleting a copy body asking
first), `pattern/fuzz.rs`
(`VARDE_TESTS=full`: random modes, Flip, counts, long and nested
spreads, units, undo and redo, kind swaps, commits, the direction
picked, copy bodies picked, named by later combines and mirrors,
hidden and deleted; edits held to the values their fields come to, a
held edit neither ready nor previewed);
`view/src/motion/tests.rs` (both panels' rows and modes, the infos).

Departures from the mock: "Join to original" starts ticked (the mock's
starts unticked). Kept: the user decided the joined copies are the
default and what older files read as, so a new pattern starts as the
stored default and as patterns did before the tick, and the mock's
unticked start (a new body per copy, each an Objects row) would make
the heavier, rarer choice the one taken by pressing Enter. The overlap
warning shows only where the panel has no other message (the mock
shows it under a failure too). Kept: the panel's footer holds one
message, a failure or refusal is the one to act on, and the warning
comes back once it's gone (it never blocks OK, and a failing draft is
added only through Add anyway). Counts up to 1024 (the mock's 100)
and the run limit the coordinate limit (the mock's 10 000 mm), with the
fields' own error words for other refusals; the axis may also be a
round edge or face, as a move's; no faded originals (the preview
replaces the model); Circular pattern isn't on the toolbar (the rail
has it; the mock's body bar does, the app has no such bar).

## Align

`crates/document/src/align.rs`.

```rust
pub struct Align {
    pub body: BodyId,             // moved in place, made before it (question 23: one body, in place)
    pub from: AlignRefs,          // on `body`, no origin reference
    pub to: AlignRefs,            // on other bodies made before it, or the origin's
    pub flip: bool,               // against the default (opposed for normals and rims)
    pub offset: Option<Value>,    // along the target's primary as found: Move::offset_ask
    pub turn: Option<Value>,      // about it, right-handed: Move::angle_ask (radians)
}
pub struct AlignRefs { pub point: PointRef, pub primary: Option<DirRef>, pub secondary: Option<DirRef> }
pub enum PointRef { Origin, Corner { body, faces: [FaceKey; 3], near }, Middle(EdgeRef), Centre(EdgeRef) }
pub enum DirRef { Origin(Axis3), Normal(FaceRef), Axis(AxisRef) }
```

- **What it is**: a timeline step on a body already made, the eighth
  variant (`FeatureKind::Align(Box<Align>)`, boxed as it's by far the
  largest kind; serde sees through the box; "Align N"). It makes no body; the body
  keeps its id and its faces their names (a move). Its motion is worked
  out from the references at every regeneration (`Motion::align`, see
  "Aligning" in `agents/kernel.md`), so the body follows either side's
  upstream edits. A point alone is a move between the points; a point
  and a primary pair also turn the primary onto the target's by the
  smallest rotation (a pin into a hole: its turn about its own axis
  doesn't matter); a secondary pair fixes the turn about it.
- **References**, resolved by the kernel's `topology/datums.rs`: a
  corner (three keys sorted and different, the vertex where faces of
  all three meet, nearest `near` among several), a straight edge's
  middle, a round edge's centre (a circle's or an ellipse's), the
  origin; a flat face's outward normal, a round face's axis
  (`DirRef::Axis(AxisRef::Face)`, as the form has it), a straight
  edge's direction (first key's face on its left from outside) or a
  round edge's axis (`DirRef::Axis(AxisRef::Edge)`: a flat neighbour's
  outward normal, else a round neighbour's axis), an origin axis
  (`DirRef::Origin`, or `DirRef::Axis(AxisRef::Origin)`, the same).
- **Flip**: the primaries meet **opposed** by default where each is a
  flat face's normal or a round edge's axis (a rim's; checked at
  regeneration, an edge being round or straight only there), so faces
  meet face to face and a pin's foot rim onto a hole's top rim puts the
  pin in the hole; the same way otherwise. `flip` turns the default
  round. Decided here: a normal on one side and a rim on the other is
  opposed too (both point out of their bodies), where the plan named
  only the like pairs. The offset and turn go along and about the
  target's primary as found, never as flipped.
- **Checks** (`CheckError::Align(id, AlignError)`): `Align::check_own
  (design)` (cheap, for a panel): a secondary only with a primary, and
  the sides paired, a primary on both or neither and a secondary
  likewise (`Unpaired`); flip, offset and turn only with primaries
  (`Options`); the offset a length `Move::offset_ask` takes (within
  `MAX_COORD` of zero: `Offset`), the turn an angle `Move::angle_ask`
  takes (within a turn either way: `Angle`); a corner's keys sorted and
  different (`Corner`), an edge's own check (`Edge`), a corner's or
  face's point finite and within `MAX_COORD` (`Near`); no origin
  reference on the moved side (`FromOrigin`: a reference there is on
  the body, as the plan has it). `Document::check` then wants the body
  there and made before it (`Body`), every moved-side reference on it
  (`FromBody`), no target-side reference on it (`OnMoved`), and each
  reference's body and faces' makers before it as a move's axis's
  (`RefBody`, `RefMaker`: one not there is allowed with ids no later
  body or feature can take). `Document::check_align_refs(index, refs)`
  checks one side's references by those last rules, for a panel.
- **Removal**: `FeatureKind::bodies()` is the moved body, so removing it
  or its maker removes the align; the target's bodies aren't listed
  (the align stays and fails until given another, as a move's axis).
- `SetUnits` pins the offset and the turn as a move's.

### Regeneration

`crates/regen/src/history/align.rs`, in history order:

- The body needs a solid of its own (`own_solids`, worded as a
  combine's).
- **Each side** is found in order (point, primary, secondary) on its
  bodies as the features before it leave them, through
  `Evaluation::holder`, by the kernel's `Topology::corner_point`,
  `middle`, `centre`, `normal`, `face_axis` and `edge_direction`, each
  cached (`Entry::Datum`: the point or direction and whether it points
  out of its body, keyed by the holder's key, the reference's kind,
  names and point, which reference on which side, and the fit
  tolerance). A target reference whose body a join or combine merged
  into the moved body fails it: aligning a body onto itself.
- **Noted** for the draft (`Evaluation::aligned`, `Drafted::datums`:
  `AlignDatums { moved, target, opposed }`, each side an `AlignFound {
  point, primary, secondary }` of `[f64; 3]`s, the moved side where it
  is before the align), whether or not the align goes on to work, each
  side only if all its references were found and the wire takes it
  (`AlignFound::fits`: finite, within `MAX_REFERENCE`, directions not
  zero). `opposed` is the flip the motion takes (the default turned by
  `flip`; just `flip` while a side isn't found). Not in `.vrdp`.
- **The motion**: `Motion::align(moved, target, AlignOptions { flip:
  opposed, offset, degrees })`, the turn's radians turned into degrees
  by `value / (π/180)` as a move's (quarter turns exact); then the body
  is placed as a move's bodies are (`motion::place`: refused if its box
  leaves `MAX_COORD`, "aligning Body 2 takes it out of range ...", then
  `Solid::transformed` cached by `Motion::bits()`).
- **Failures**, worded by which reference on which side
  (`message::align_ref`): "its point on the target wasn't found", "its
  first direction on the moved body is a face that isn't flat" (isn't
  round, an edge that isn't straight, isn't round, isn't straight or
  round, whose direction can't be told; these show the face's
  triangles or the edge's curves), "... is on a body that's gone", "its
  point on the target is in the moved body now: a feature before this
  one merged them; pick it on another body"; the motion's refusals:
  "its second direction on the target is parallel to its first: pick
  one across it" (the side found by aligning it onto itself; shows both
  its directions' faces or edges), "its point on the moved body is too
  far out to align by" (a nearly straight arc's centre past the limit;
  shows the edge), and a fallback for what the document refuses
  (`ALIGN_MALFORMED`).
- **Dedup with moves**: a move's round-face axis (`motion::face_axis`)
  now calls the kernel's `Topology::face_axis` (same result: the form's
  axis through the point nearest the face's point); a move's axis edge
  and a mirror's plane keep their own code, since a move's round edge
  turns as the edge runs (not the align's rim sign) and a mirror wants
  the plane's offset too.

### UI

The move's session (`MotionSession`, above, `app/src/doc/motion.rs`)
with `MotionKind::Align`; an align's own parts are in
`app/src/doc/motion/align.rs` (`AlignSetup`) and
`view/src/motion/align.rs` (`AlignView`). **The UI mock has no Align
panel**: only the tool, "Align" in its Transform group with no key and
the `align` icon (ui-mock-icons). The panel is built in the style of the
mock's nearest one, Move's: the same card, fields picked into, rows with
a cross, an icon toggle and typed fields.

- **Starting**: `Look::StartAlign` from the rail's Transform set
  ("Align", after Circular pattern; no key, as the mock has none:
  `shortcut::align_binding`, `Shortcut::NONE`, the rail's list letter
  `A`), again backing out; not on the toolbar (nearly full at 1280 px,
  as Circular pattern isn't); or editing one (`Look::EditFeature`:
  double-click, `Enter`, "Edit align"), which opens with its references,
  Flip, distance and angle, nothing picking. One body, in place
  (question 23): what's selected gives it (the first if several), else
  the model's only body; with one, clicks go straight to its point.
- **The body** is picked as a move's bodies (a click on a face of it,
  or its row in Objects, merged bodies as their holder, merges followed,
  only bodies made before it; "Missing body", "A picked body is gone"),
  but a click on another body replaces it: what's picked on the moved
  side on another body, and on the target side on the new one or a body
  merged into it before the align, is taken out (`AlignSetup::moved_to`).
- **Picking the references** (`MotionPick::Align(AlignSlot { side,
  role })`; a field clicked picks into it): after each pick clicks go on
  to the next one still needed, in the order the moved body's point, the
  target's point, the moved body's direction, the target's direction,
  then a second direction where the other side has one, else nothing
  (`MotionPick::Nothing`: clicks pick nothing until a field is
  clicked; `AlignSetup::next`). The field picking, clicked again, stops
  picking (`MotionPick::Nothing`), so an align with what it needs but
  not all it can take (points alone) shows as set up. Points as the
  measure tool picks them
  (`ModelPicking::snaps`, faces, edges and vertices): a snapped corner
  (`Naming::corner_ref`: the corner's three keys, sorted and different,
  at its exact point, refused as `Naming::edge_ref` refuses an edge) or
  the corner of a vertex clicked, an edge's snap point or the edge
  itself, its middle if straight (`PickIndex::edge_ends`), its centre if
  it has a snap point and isn't (a circle or an ellipse); an edge's
  point is named at a point on the edge (`PickIndex::chain_point`), never
  at the centre the dot is held at, so the dot and the edge name the same
  reference. **A round edge's centre also gives its side the rim's axis**
  as its direction while it has none, so a pin's rim and a hole's align
  the pin in two clicks (beyond the plan, decided here: the rim names
  both); a side's direction that is its point's rim's axis
  (`Side::rim_axis`, however it was picked) goes with the point: the
  point picked again replaces it (by the new rim's axis, or nothing),
  the point taken out takes it out. An edge picked for a direction is
  named at the same point on it as one picked for a point
  (`PickIndex::chain_point`, wherever it's clicked), so a rim picked by
  hand as the direction of the point at its centre is that point's rim's
  axis and goes with it too. Deliberate: a direction on the point's own
  rim is the point's axis however it was picked, so picking the point
  again elsewhere never leaves an axis on a rim no longer picked; a
  direction on any other edge or face stays. Directions on faces and edges
  (`Picks::EdgesAndFaces`): a flat face's normal (`DirRef::Normal`), a
  round face's axis (a cylinder's, cone's, torus's or revolved
  surface's, `DirRef::Axis(AxisRef::Face)`), a straight or round edge
  (`DirRef::Axis(AxisRef::Edge)`). Each named as of the feature
  (`Naming`, `checked_face_ref`, `edge_ref`). The moved side's must be
  on the body moved (a pick there with no body yet makes its body that
  one), and is named on it (`Taken::on`): a face made on a body a join or
  combine merged into it before the align is named on it, not on the body
  it was made on as `Naming` names it, since the document wants the moved
  side's references on the moved body's own id (regenerating finds the
  face by its keys on that body's solid). The target's must be on another
  body than the moved one there (the body holding it, `Merges::holder`).
  The target may be the origin:
  while its point is picked the toolbar offers "Origin"
  (`MotionLook::OriginPoint`), while a direction is, the X, Y and Z
  axes (`MotionLook::OriginAxis`, `DirRef::Origin`). Refused with a
  notice in the status bar, as a move's axis: "Only a corner, a straight
  edge's middle or a round edge's centre can be the point", "Only a flat
  or round face, or a straight or round edge, can give the direction",
  "Pick it on Body 2, the body aligned" (with that body gone, by an
  undo, "The body aligned is gone: pick the body to align first"),
  "Pick what it's aligned to on
  another body than the one aligned", `Naming`'s refusals ("Only an edge
  made before the align can be picked", ...), and an out of date model.
  A row's cross takes a reference out (`MotionLook::Clear`), and clicks go
  on to what's needed first (`AlignSetup::next`, nothing once it's whole)
  unless bodies are being picked.
- **Highlighting**: while bodies are picked, the body as a move's; while
  a reference is, what a click takes lit as hovered, and the faces and
  edges picked for directions on the model shown, the moved side's as
  selected and the target's in the second colour (`Colors::second`, the
  measure tool's B; `PickIndex::highlight_with`'s third list); part of
  the highlight's key. Each reference is marked on the model it was
  picked on; on another model shown (an edited align's references the
  first time they're picked again, a draft answered, an undo) it's found
  again by its names on the body drawing its body there
  (`AlignSetup::follow`, `PickIndex::find_face`, `find_edge`,
  `find_vertex`; once per model, from `refresh_motion_highlight`), so
  what's picked stays lit and its points drawn.
- **Fields**: Flip (`MotionLook::Flip`, `Icon::TkFlip`, "The directions
  meet the other way round"), stored only with directions; Offset's
  Distance (`MotionField::Distance`, `Move::offset_ask`) and Angle (the
  move's angle field, `Move::angle_ask`), each stored only where it
  isn't zero, and only with directions: one typed without them is said
  ("pick a direction on each side to offset or turn along").
- **Whole and ready** (`MotionSession::align`, `align_need`): the body,
  both points, the directions paired; else the status bar says what's
  next ("pick the body to align", "pick a point on the body: a corner,
  an edge's middle or a rim's centre", "pick the point to align it to",
  "pick the direction to align it to", "pick a direction on the body",
  the second directions alike; a second direction left without first
  ones, theirs taken out, "pick a direction on each side before a second
  one", as clicks go on to the first ones first). A point alone is ready (a move between
  the points). `Align::check_own` refuses as the panel's foot ("Align
  fails"); what the document checks of the references
  (`Document::check_align_refs`, each one alone at the feature's place)
  and whether its body is still held decide **gone**: kept, said as the
  mock says a move's axis is ("The point it's aligned to is gone: pick
  another", "The direction on the body is gone: pick another", ...),
  nothing previewed or committed until picked again or redone. A
  target's reference on a body a join or combine before the align
  merged into the body aligned (one added or redone since it was
  picked, `AlignSetup::merged_into`, from `MotionSession::follow`) is
  gone alike ("The point it's aligned to is in the body aligned now:
  pick another"), as regenerating would refuse it. The other way round,
  the body aligned merged into another (the target's, say) by a join or
  combine added or redone since: the body moved follows to the body
  holding it (`MotionSession::follow`, as a move's bodies do) and the
  moved side's references go with it, named on the holder as a pick
  there now would be (`AlignSetup::moved_merged`, `Taken::on`; their
  marks found again on the next model), rather than left on the body
  merged away, where regenerating wouldn't find them and nothing would
  ask for them again.
- **Preview**: the align as set up is the draft while nothing is
  picked; while a reference is picked the model is the history as of
  the feature (a new one sends no draft, an edited one a move of its
  body by nothing), so the faces and edges clicked are where the align
  finds them. The viewport (`Moving::align`) draws each side's point as
  a dot, its direction as an arrow across the bodies and its second
  direction shorter, dashed: the moved side's in the accent, the
  target's in the second colour, where the newest draft found them
  (`Drafted::datums`, `MeshFeed::draft_datums`; the moved side where it
  was picked, before the align); while picking, the points picked on
  the model shown, and the snap dots of what the cursor's over (drawn as
  the measure tool's, `viewport::measure::snap_dots`).
- **Committing**: OK (`Enter`, Add anyway) adds "Align N" or sets the
  edited one, one undo step, as a move's. The status bar says "New align
  · Body 2 to Body 1" once whole (`align_info`), else what's next, with
  the hints "Pick the body", "Pick the point", "Pick the direction",
  "Pick the second direction".

Tests: `app/src/doc/motion/tests/align.rs` (a pin picked by its foot
rim and the hole's top rim, 10 down, OK: its box in the hole, one undo
step; a plate aligned face to face by corners and faces with an offset,
the datums drawn; refused picks and the origin from the toolbar; edited
from the Timeline, the neutral preview while picking, another distance,
undo; a reference an undo takes away said to be gone, back on redo; a
rim picked again taking its axis along, cleared with it, and the dot and
the edge naming the same; picks on what a combine merged into the moved
body named on it, and a target merged into a body picked to move taken
out; an edited align's references lit and drawn on the model as of it;
a direction picked by hand on the point's rim going with the point; an
align of points alone, Flip and a distance left out of it; directions
asked for in the order clicks go to them, first ones before a second
one left alone; the example plate's hole split by a slot, its two top
arcs between the same faces, the far one's centre lit and found again
there on the next model), the session fuzz (`motion/tests/align/fuzz.rs`,
`VARDE_TESTS=full`: picks of faces, edges, vertices and snap dots on the
model shown or one gone by, the body switched, fields picked into and
clicked again, references taken out, the origin, Flip, distances and
angles, units, undo and redo, joins and combines merging bodies before
the align, bodies added and features removed, models answered at any
point; after each step a ready session whole, its references passing
the document's checks, the moved side's on its body and the target's
off it, previewed as set up, committed as drafted; what's lit naming
what's picked; the status bar asking for what clicks go to; an edited
align opening to what it stores and OK writing nothing),
`view/src/motion/tests.rs` (the panel's order, the status text),
`viewport/motion/tests.rs` (the points and directions drawn).

Departures from the mock (which has no Align panel): the panel is the
Move panel's style with this session's own fields (From, To, Flip,
Offset); refusals in the status bar, as a move's; the rail only, not
the toolbar.

Tests: `document/src/align/tests.rs` (pairing, values, every reference
check, removal, units, postcard and hostile bytes, the variant index),
`regen/src/history/tests/align.rs` (a pin of the hole's radius aligned
by its foot rim into the example plate's hole 10 down and joined: the
analytic volume, following the hole moved upstream, undo; the hole's
rim found after a join merged another body into the plate, and a target
on a merged body refused; a block aligned corner to corner face to face
exact to the bit, with an offset, a quarter turn, a secondary pair, a
flip and a point alone; each refusal with its geometry and no body
changed; a consumed body; the cache by the motion's bits; what's noted
against the topology; the moved side named on the plate holding a disc
combined into it, then the combine changed so the merge goes (its tool
kept, another tool, both, the combine gone: the same place, or not
found or on the moved body with nothing moved) and undone, and the same
through a join a block is taken out of; nearly straight arcs' centres
up to the coordinate limit, placed where the drawn centre takes them or
refused as out of range), `regen/src/wire/tests.rs` (datums on the wire,
bad ones refused), `io/src/vrdp/tests.rs` (through a file, tampered
records refused), and the motion fuzz (`VARDE_TESTS=full`): random
aligns of corners, middles and centres, with normals, face axes and
edge directions or none, onto other bodies or the origin, with
secondaries (mostly across), flips, offsets and turns, blocks among the
bodies with a shallow arc on top about a centre up to the coordinate
limit away, each one that works held to what the topology gives before
it (the noted datums to the bit), those datums to the geometry worked
out apart (a flat face's normal along its triangles' turn, a rim's
centre the circle through three of its points, its axis square to them,
a straight edge's middle and direction from its ends), the default
opposition told from what the primaries name, every vertex and the
centre of mass where the motion worked out with `glam` takes them (the
frames by Gram–Schmidt, else the smallest rotation by axis and angle,
the turn, the offset), and its point and primary found again on the
target's where their names are unique; one failing changes nothing.

The Timeline shows the mock's `align` icon (the tool icons' set), "to
Body 2" or "to the origin" as its note and "Body 2 to Body 1" as the
status info. Known gaps: the moved side's datums are drawn where they
were before the align, not on the body as previewed; a direction is
drawn only once the preview answers (while picking, only the face or
edge lit). A far centre (a nearly straight arc's) is refused only when
the motion is made, and noted for the draft only within the wire's
bounds; a sketch holds its points within the coordinate limit, so only
rounding takes a centre found from the curve past it. Such a centre is
as good as the arc's control point times `(2r/chord)²`: profiles build
it from the ends' middle (see `Conic::arc_between` in
`agents/kernel.md`) so it's the drawn centre within `1e-4` mm for a
10 mm arc a 1e5 away (it was 0.01 off).

## Scale

`crates/document/src/scale.rs`.

```rust
pub struct Scale {
    pub bodies: Vec<BodyId>,      // 1..=256, sorted, made before it (as a move's)
    pub about: PointRef,          // the origin, or a point on a body made before (align's)
    pub factor: ScaleFactor,
}
pub enum ScaleFactor {
    Uniform(Value),               // Scale::factor_ask: Ask::factor, 1e-3 ..= 1e3
    PerAxis([Value; 3]),          // X, Y, Z, each as Uniform's
    EdgeLength { edge: EdgeRef, length: Value, axis_only: bool },  // edge on a scaled body; length: Extent::ask
}
```

- **What it is**: a timeline step on bodies already made, the ninth
  variant (`FeatureKind::Scale(Scale)`, "Scale N"; not boxed, it isn't
  the largest). It makes no body; each body keeps its id and its faces
  their names (instance 0), `x ↦ c + S·(x − c)` along the world axes
  (`Motion::scale`, see "Transforms and assembly" in
  `agents/kernel.md`). Positive
  factors only (a negative one is a mirror, its own feature).
- **To an edge's length**: the factor is worked out at every
  regeneration as the typed length over the edge's measured one, so an
  upstream edit changing the edge keeps the typed length. Uniform by
  default (any edge, curved too); with `axis_only`, along the world
  axis a straight edge runs along only, the others 1.
- **`Ask::factor(units, max)`** (`varde_expr`, new): a plain number from
  `1/max` to `max`; `Scale::factor_ask` passes `MAX_SCALE_FACTOR` (1e3),
  so `1e-3 ..= 1e3` (both ends taken). `Scale::length_ask` is
  `Extent::ask` (a length from a micrometre to `MAX_COORD`).
- **Checks** (`CheckError::Scale(id, ScaleError)`): `Scale::check_own
  (design)` (cheap, for a panel): the body count and order (`Bodies`,
  `BodyOrder`), every factor as `factor_ask` takes it (`Factor`), the
  edge length as `length_ask` (`Length`), the point's own check
  (`About(AlignError)`: a corner's keys sorted and different, points
  finite within `MAX_COORD`), the edge's own check (`Edge`) and its
  body among the scaled ones (`EdgeBody`). `Document::check` then wants
  the bodies made before it (`Body`), and the point's and the edge's
  bodies and faces' makers before it (`RefBody`, `RefMaker`: as a
  move's axis; one not there is allowed with ids no later body or
  feature can take). `Document::check_scale_refs(index, scale)` checks
  those last rules, for a panel. The point may be on one of the scaled
  bodies (scaling about its own corner).
- **Removal**: `FeatureKind::bodies()` is the scaled bodies, so
  removing one (or its maker) removes the scale; the edge's body is one
  of them, so it's covered. The point's body isn't listed (the scale
  stays and fails until given another, as a move's axis).
- `SetUnits` pins the edge length as an extrude's distance, and runs
  `pin_units` on the factors too (as a pattern's count): a plain factor
  ("2", "25.4") is left as typed, and only a bare number added to a
  length inside one ("(3 + 1 mm) / 1 mm") gets the old units written in,
  so the factor's value never changes (decided here: the plan said
  factors are left alone, which they are, but a factor's expression
  can still hold a bare length).

### Regeneration

`crates/regen/src/history/scale.rs`, in history order:

- Every body needs a solid of its own (`own_solids`, as a move's).
- **The point** is found as an align's (`align::Found::point`, shared:
  `Found` now takes `message::Whose`, an align's reference on a side or
  the scale's point, so the messages read "its point wasn't found", "its
  point is on a body that's gone"; a point on a body merged into another
  is found on that one). Cached as an align's (`Entry::Datum`).
- **The edge** (an edge length's) is found on its body's topology by
  its names and point and measured by the measure tool's lengths
  (`measure::measure` with `Pick::Edge`: the whole chain's length, its
  ends where it's a line), cached (`Entry::Length(EdgeLength { length,
  line })`, keyed by the body's key, the names, the point and the fit
  tolerance), on the body's topology as drawing it keeps it
  (`inspect::topology`: the edge is picked on the model as of the
  scale, which drew that body). Then: the length must be above the resolution before the
  division ("its edge is too short to scale by: ..."), `f = L / ℓ` within
  `1e-3 ..= 1e3` ("the length is too far from the edge's: more than
  a thousand times longer or shorter"); along its axis only, the edge
  must be straight ("its edge isn't straight, so it can't scale along
  its axis only") and along a world axis: the sum of the squares of its
  direction's two smaller components at most `AXIS_SINE²` (`1e-18`) of
  its length's square (`along_axis`, `+ ×` only; the two are added as
  they are, never taken from the length's square, whose rounding would
  lose them and let a sine of up to about `1e-8` pass), else "its edge
  isn't along an axis any more, so it can't scale along it only". The
  edge's other components aren't scaled, so it gets `L` within about
  `1e-12` relative.
- **Every factor** is then checked within `1e-3 ..= 1e3` again (a typed
  one past the document: `SCALE_FACTOR`).
- **Noted** for the draft (`Evaluation::scaled`, `Drafted::scale`:
  `ScaleFound { centre, length, factors, fitted }`), whether or not the
  scale goes on to work: the point found, the edge's measured length
  (before the scale), the factors, each `None` where not found or where
  the wire wouldn't take it (`ScaleFound::fits`: the point within
  `MAX_REFERENCE`, the length finite and not negative, the factors
  finite and positive, at least one of the three there), and `fitted`,
  how many faces (by key, per body) of the scaled bodies claim no
  surface (`Surface::Free`), for the panel's note "3 fitted faces: their
  error grows × 25.4". Not in `.vrdp`.
- **The motion**: `Motion::scale(centre, factors)`; the bodies are then
  placed as a move's (`motion::place`): refused before the kernel if a
  box's image leaves `MAX_COORD` ("scaling Body 2 takes it out of range
  ..."), then `Solid::transformed` (which multiplies each face's
  `slack` by the largest factor above 1) cached by `Motion::bits()`, so
  an edge length giving the factor a typed one gave finds the same
  solid. A result `check` refuses (a scale down taking detail under the
  resolution, or rounding) is "scaling Body 1 leaves no clean solid:
  parts of it come too close together, or get too small, for the
  tolerance; try a finer tolerance". A scale up of fitted faces is kept
  (their slack records it).
- **A refused edge length shows its edge**: once the edge is found, any
  later refusal of the edge length (too short, too far, not straight,
  not along an axis, not measured) carries the edge's curves on its
  body as the failure's geometry (`edge_geometry`, as an align draws a
  refused reference), so the panel's error and the viewport show which
  edge; an edge not found shows nothing. Its topology is the cached one
  drawing keeps (a refused scale leaves its bodies as they were, which
  the model draws), so a length refused at every keystroke doesn't work
  out the body's topology each time.
- `along_axis` and `AXIS_SINE` are public (`varde_regen::along_axis`):
  the panel offers "Along its axis only" by the same test.

### UI

The move's session (`MotionSession`, above) with `MotionKind::Scale`;
a scale's own parts are in `app/src/doc/motion/scale.rs` (`ScaleSetup`)
and `view/src/motion/scale.rs` (`ScaleView`, `ScaleMode`). **The UI mock
has no Scale panel**: only the tool, "Scale" in its Modify group (after
Draft, before Combine) with no key and the `scale` icon. The panel is
built in the style of the mock's nearest one, Move's.

- **Starting**: `Look::StartScale` from the rail's Modify set ("Scale",
  before Combine as the mock orders them; no key: `scale_binding`,
  `Shortcut::NONE`, the rail's list letter `S`), again backing out; not
  on the toolbar (the mock's bar has no Scale, and it's nearly full at
  1280 px); or editing one (`Look::EditFeature`: double-click, `Enter`,
  "Edit scale"), which opens with its bodies, point, mode and values.
  Bodies from what's selected, else the model's only body, as a move's;
  the factor field (`VALUE_FIELD`) takes the focus. `Icon::Scale` is
  now in the Modify category, as the mock files it.
- **Bodies** are picked as a move's (a click on a face picks or
  un-picks its body, Objects' rows, merged bodies as their holder,
  "Missing body", "A picked body is gone").
- **The point** (the Point field, `MotionPick::Point`): the origin to
  begin with ("Origin", drawn at zero). Its field clicked picks it as an
  align's points are picked (`align::point_of`: a snapped corner or a
  vertex's corner, a straight edge's middle, a round edge's centre, at
  the measure tool's snap dots, `ModelPicking::snaps`), on any body made
  before the scale, one of the scaled ones too, named as of the feature
  (`Naming`); the toolbar offers "Origin" meanwhile
  (`MotionLook::OriginPoint`). A pick, or the field clicked again,
  hands the clicks back to the bodies. No cross: picking another
  replaces it. Refused as an align's ("Only a corner, a straight edge's
  middle or a round edge's centre can be the point", "Only a corner made
  before the scale can be picked", an out of date model).
- **How it scales**: three tiles under "Scale" (`MotionLook::ScaleMode`,
  icons `ScUniform`, `ScAxes`, `ScEdge`, not in the mock, drawn as the
  patterns' modes): Uniform (the Factor field), Per axis (X, Y and Z,
  `MotionField::AxisFactor`), Edge length. Factors read by
  `Scale::factor_ask` (`1e-3 ..= 1e3`, "1" to begin with), each mode
  keeping its own values while another is shown.
- **Edge length** (`MotionPick::Edge`): choosing it with no edge picks
  one next; edges only (`Picks::Edges`), any edge, named as of the
  feature (`Naming::edge_ref`) on the body holding it there
  (`Merges::holder`), which must be one scaled (a pick with no bodies
  yet picks its body: "Pick an edge of a body it scales" otherwise; a
  face, "Only an edge can be scaled to a length"). The edge row ("Edge
  of Body 1") shows its length now at its right, written in the
  design's units: what the draft answered (`ScaleFound::length`,
  `MeshFeed::draft_scale`), or, with no scale previewed or its preview
  failed, the edge measured on the model shown: the session asks the
  regeneration lane to measure it with every request
  (`Doc::scale_inspect`, an `InspectPick` of the edge's keys on the body
  drawing its body, through the measure tool's `Inspect`; the request
  takes the measure tool's picks first, then the scale's, then what's
  selected, and the three are never wanted together: the tool closes
  the session, and the selection is measured only with no operation
  open). The length is read only from an answer to the scale's own
  request (`MeshFeed::inspected_of`), never another edge's. The Length field
  (`Scale::length_ask`) starts empty, asking for nothing yet.
  "Along its axis only" (an icon toggle, `MotionLook::AxisOnly`) shows
  where the edge measured is a straight line along a world axis
  (`varde_regen::along_axis`, regenerating's own test), or while it's on
  (so an edge an upstream edit tilted can be turned back to uniform).
  The edge picked is lit in the second colour on its selected bodies,
  found again by its names on each model shown. A body merged into
  another once the model shows the merge (a join or combine redone, a
  pick made before the model knew it) takes the edge with it on to the
  holder, as the bodies follow (`MotionSession::follow`), so the scale
  stays whole.
- **Whole and ready**: bodies, the point, and the mode's values: a
  factor other than 1 ("enter a factor other than 1"), or an edge on a
  scaled body ("pick the edge to give a length", "pick an edge of a
  body it scales") and a length ("enter the length the edge is to
  have"); `Scale::check_own` refuses as the panel's foot ("Scale
  fails"). The point or edge the document no longer takes at the
  feature's place (`Document::check_scale_refs`) or whose body is gone
  is kept and said to be gone ("The point is gone: pick another", "The
  edge is gone: pick another"), nothing previewed or committed until
  picked again or redone.
- **Preview**: the scale as set up is the draft while nothing is picked;
  while the point or edge is, the model is the history as of the
  feature (a new one sends no draft, an edited one a move of its bodies
  by nothing). The viewport draws the point as a dot in the accent
  (`Moving`, `ScaleView::at`: where the draft found it, else where it
  was picked), the snap dots while it's picked. A refused edge length
  draws its edge as the failure's geometry (above). **The fitted-faces
  note**: where the draft scales fitted faces up (`ScaleFound::fitted`
  above zero, its largest factor above 1), the panel's foot warns "3
  fitted faces: their error grows × 25.4" ("1 fitted face: its error
  grows × 2"), in the warning's place (shown where nothing else is).
- **Committing**: OK (`Enter`, Add anyway) adds "Scale N" or sets the
  edited one, one undo step. The status bar says "New scale · Body 1
  ×2" once whole (`scale_info`), else what's next, with the hints "Pick
  bodies", "Pick the point", "Pick the edge".

The Timeline shows the mock's `scale` icon (the tool icons' set), the
note "×2", "×1 · 1 · 2" or "edge → 50 mm" (written as a pattern's "×4",
where the plan had "× 2") and "Body 1 ×2" as the status info
(`view/src/motion.rs`: `scale_note`, `scale_info`).

Departures from the mock (which has no Scale panel): the Move panel's
style with this session's own fields; the rail only, not the toolbar;
refusals in the status bar, as a move's. Known gaps: the length shown
for an edited scale whose preview failed is measured on the model
shown, which holds the features after the scale too (one changing the
edge shows its length after them); the point is drawn where it was
picked or found, before the scale.

Tests: `document/src/scale/tests.rs` (adding and undo, factors and
lengths at and past their bounds and tampered, bodies, the point and
the edge checked, removal, units, postcard and hostile bytes, the
variant index), `regen/src/history/tests/scale.rs` (a block × 2 about
the origin and about its top corner to the bit and × 1/2, undone; a
cylinder × 2 along Y: twice the volume, its rim an ellipse of semi-axes
10 and 5 whose length is the ellipse's perimeter to `1e-12`; a union
flush with a scaled block; a revolved torus × 25.4: the volume × 25.4³,
the fitted faces' slack × 25.4 and counted; a 10 × 20 × 5 block scaled
so an edge is 50, uniform (volume × 2.5³) and along Y or X only; the
example plate scaled so its hole's rim is 100 round (to `1e-12`) and
refused along its axis only; an upright edge kept 50 tall through the
plate extruded twice as far upstream and undo; refusals: faces that
don't meet, lengths past a thousand times either way (the bounds
themselves scale), out of range, a slanted edge along its axis only
(uniform it scales); a plate a micrometre thick × 0.001 refused; the
cache by the motion's bits; a draft answering what it found, a point on
a merged body, a point not found; a sphere × 2, 1, 1/2: an exact
quadric of the sphere's volume, then cut in half and sliced square to
each axis, each the ellipsoid's analytic volume within the fit over the
faces cut, or refused changing nothing; an elliptic cylinder cut along
a chord and joined to a block, a cone × 2 along X cut square to its
axis, the example plate × 2 along X with a disc tangent to its
elliptic hole and one crossing it joined: analytic volumes; a sphere
and a torus at factors 1000 and 1/1000, uniform and per axis: the
volume times the factors' product, the fitted faces' slack × the
largest factor, or refused as too small; along its axis only for
edges with a sine just under and over `1e-9`, and `along_axis` at any
length; a point on a scaled block merged into another; the edge
through an upstream edit making it taller, a join edit, and a cut
taking it away), `regen/src/wire/tests.rs` (what's
found on the wire, bad ones refused), `io/src/vrdp/tests.rs` (through a
file after an align, tampered records refused),
`expr/src/eval/tests.rs` (`Ask::factor`), `view/src/motion/tests.rs`
(the notes; the panel's order with each mode, the edge's length beside
it, Along its axis only where offered, the status text),
`viewport/motion/tests.rs` (the point drawn), the refusals' edge drawn
(`regen/src/history/tests/scale.rs`), and `app/src/doc/motion/tests/
scale.rs` (the example plate × 2 about the origin, the preview's box,
the status bar, OK one undo step; about a corner picked at its snap dot,
the toolbar's Origin, a face refused as a point; per axis, × 3 along Z;
to an edge's length: Edge length picking an edge, a face refused, the
edge lit, its length measured with the model and shown, Along its axis
only offered, 120 doubling the plate and along its axis only stretching
X alone, OK; the hole's rim: its length 2π·8 shown, no axis option, a
length a thousand times too far refused with the edge drawn; a revolved
ring × 25.4 noting its fitted face, none scaled down; editing from the
Timeline, a move by nothing while the point is picked, Esc, another
factor and undo; a point an undo takes away said to be gone, back on
redo; an edge picked on a disc a redone combine then merges into the
plate following it there, still whole and measured) with its session
fuzz (`motion/tests/scale/fuzz.rs`, `VARDE_TESTS=full`: picks of faces, edges, vertices and snap dots on the
model shown or one gone by as the point or the edge, modes switched,
Along its axis only, bodies picked and dropped, factors and lengths
typed out of range, overflowing or not numbers, the origin, units, undo
and redo, joins and combines merging bodies before it, bodies added and
features removed, models answered at any point; after each step a ready
session whole, passing its own and the document's checks with its edge
on one of its bodies, previewed as set up, committed as drafted; the
edge lit naming the edge picked on the body drawing it; an edited scale
opening to what it stores and OK writing nothing). The motion fuzz
(`VARDE_TESTS=full`) makes scales too: uniform, per axis or to the
length of a random edge (along its axis or not), about the origin or a
random point of any body; each that works is held to its noted point
(the topology's), its factors (typed, or the length typed over the
edge's measured before it), every body's volume times their product
and its centre of mass where the scale takes it, the edge then the
length typed; one failing changes nothing; an align's point on an
elliptic rim is checked against the conic through five of its
points.

## Split

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
    Plane(PlaneRef),                                      // an origin plane or a flat face's
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
  one side is kept (refused while a later feature names it, as an
  extrude's new body is) but keeps its id in `new_body`
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

### Regeneration

`crates/regen/src/history/split.rs`, in history order:

- The body needs a solid of its own, and so does a tool body
  (`own_solids`, as a combine's: consumed or failed fails the split).
- **The tool**, from the body's box (`Solid::bounds3`), cached as an
  `Entry::Solid` by the feature, the box's bits and the fit tolerance
  and, by kind: an origin plane, `varde_kernel::half_space(normal, 0)`;
  a flat face (found on its body's holder by key and point, as a
  mirror's plane: "its plane face's body is gone", "... wasn't found",
  "... isn't flat" with the face drawn), `half_space(n, d)` of its form,
  keyed by the holder's key and the reference; a face,
  `varde_kernel::surface_tool(form, on, bounds, ..)` with its region's
  form and a point inside it (the middle of its first patch, picking a
  cone's nappe; "its face's body is gone", "its face wasn't found";
  `ToolError::CantExtend` is "its face can't be extended to split with",
  the face drawn); a tool body, its solid and key as they are; a
  sketch's regions, found and merged as an extrude's and extruded by
  `varde_kernel::extrude` over the body's span along the sketch's
  normal with through all's margin (`through_all` of the body alone),
  its walls `Side { curve, .. }` of the split, keyed by the regions, the
  span's bits, the sketch's key and placement; a chain, ordered by
  `profile::chain` then `varde_kernel::chain_tool(segments, frame,
  bounds, RIM, ..)` (the rectangle round the shadow named as profile
  curve `RIM = 1 << 32`, past every sketch curve id), keyed by the
  curves, the sketch's key and placement. A sketch not placed is "its
  sketch isn't placed". The kernel's tool errors are worded by
  `message::split_tool`: too complex "extending its tool past Body 1 is
  too complex to work out", out of bounds "Body 1 is too near the edge
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
- **The split**: `varde_kernel::split(body, tool, tol, budget)` →
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
  and the sketch is placed on the first it's found on (a placement's
  cached failure keeps only its words, so "not found" is told by
  `FACE_NOT_FOUND`, which `place_on` says for that alone). The search
  goes back through splits too: a face named on a split's new body that
  isn't found there is looked for on the body split (and on through
  that body's splits), so a sketch on a face of the new body follows it
  when `original` is switched and the face goes to the body. Switching
  `original` swaps which body later features work on.
- **Kernel stand-in**: the kernel's `split`, `half_space`,
  `surface_tool` and `chain_tool` (`kernel/src/boolean/split.rs`) are
  not built yet: each has its planned signature and fails with
  `TooComplex`. So every split fails today with the too-complex message
  (a plane's, a face's or a line's tool first, as "extending its tool
  ..."; a tool body or a sketch's regions, whose tools the kernel
  already builds, as "splitting Body 1 is too complex to work out: ..."),
  its body is left whole and its new body has no solid (a feature naming
  it fails: "Body 3 has no solid: the feature making it failed"); the
  rest of the history goes on. The regen tests swap the kernel's split
  for two booleans (`a ∩ t`, `a − t`; `split::SPLITTER`, a thread local
  for tests, other crates' through the `testing` feature) to test what's done with the pieces; the planned
  analytic tests of the kernel's split are written out and
  `#[ignore = "kernel split not built"]`.
- The draft's reply carries nothing new: a split draft applied by
  `AddFeature` makes its new body as the command does, drawn as its own
  part (the app can tell its id by applying the same command to its
  copy of the document).

### UI

The move's session (`MotionSession`, above) with `MotionKind::Split`; a
split's own parts are in `app/src/doc/motion/split.rs` (`SplitSetup`)
and `view/src/motion/split.rs` (`SplitView`, `SplitMode`). **The UI mock
has no Split panel**: the icon mock has only the tool, "Split body" in
its Modify group (after Combine) with no key and its `splitbody` icon.
The panel is built in the style of the mock's nearest ones, Combine's
(one body as a picked field, choices as tiles) and Mirror's (a plane or
face picked, the origin planes on the toolbar).

- **Starting**: `Look::StartSplit` from the rail's Modify set ("Split
  body", after Combine as the icon mock orders them; no key:
  `split_binding`, `Shortcut::NONE`, the rail's list letter `P`), again
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
  body a later feature names is refused at once, as the document would
  ("Move 2 uses Body 4, the piece this split would no longer keep: keep
  both, or take Body 4 out of Move 2 or delete it first", the panel's
  foot, `Doc::split_held`), and not previewed. Keeping both again
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
  regeneration's too-complex message ("Split fails" over "extending its
  tool past Body 1 is too complex to work out", or "splitting Body 1 is
  too complex to work out: ..." for a tool body or regions), OK waits,
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
`unnamed(.., MotionKind::Split)`, the naming `Doc::motion_naming`.

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
holding its id and bringing it back with it, and refused while a scale
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
on the example plate by XY from the toolbar, failing as too complex in
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

## Chamfer

`crates/document/src/chamfer.rs`.

```rust
pub struct Chamfer {
    pub edges: Vec<EdgeRef>,       // 1..=MAX_BLEND_EDGES (256), one body, EdgeRef::order, no repeats
    pub distances: ChamferSize,
    pub chains: bool,              // take in each edge's tangent chain
    #[serde(default)] pub flip: bool,  // first faces are the second keys'
}
pub enum ChamferSize { Equal(Value), Two(Value, Value), Angle(Value, Value) }
```

- **What it is**: the eleventh variant (`FeatureKind::Chamfer`,
  "Chamfer N"). Its edges are cut off on the body they're on, which
  keeps its id; it makes no body. Sizes as the UI mock's choices:
  Equal (one distance along both faces), Two distances, Distance and
  angle (the cut's angle to the first face, above 0 and under 90°,
  `Chamfer::angle_ask`). Distances are lengths as an extrude's
  (`Chamfer::distance_ask` = `Extent::ask`). An edge's **first face**
  is the face of its reference's first key (`EdgeRef::faces[0]`, the
  lower), or the second's with `flip` (the mock's "Flip sides"): what
  Two's first distance and Angle's distance and angle run along.
  Edges are all on one body (decided here: as offset face's and draft's
  faces; one kernel call); the plan's struct had no `flip`, the mock's
  panel has it. The list is kept in `EdgeRef::order` (body, keys, the
  point's coordinates by `total_cmp`) without repeats, so a set of
  edges has one form; two picks of edges between the same two faces at
  different points are two edges.
- **Checks** (`CheckError::Chamfer(id, ChamferError)`):
  `Chamfer::check_own(design)` (cheap): its edges by
  `check_blend_edges_own` (`Edges(BlendEdgesError)`: 1..=256 edges,
  `Count`; each edge's own check, `Edge(EdgeError)`; in order without
  repeats, `Order`; one body, `Bodies`), distances (`Distance`) and the
  angle (`Angle`) by their asks. `Document::check_blend_edges(index,
  edges)` (public, for the panel; a fillet's edges are checked by the
  same two, `crates/document/src/blend.rs`): the body there and made
  before (`Body`: depended on, as a combine's bodies), and every key's
  feature before it, or not there with an id below the next
  (`RefMaker`, as a sketch's face's).
- **Dependencies**: `FeatureKind::bodies()` is the edges' body, so
  removing it or its maker removes the chamfer. The features that made
  its edges' faces are **not** followed: removing a join whose wall an
  edge ran along leaves the chamfer, which then fails ("its edge wasn't
  found"), to be edited (decided, as every face and edge reference).
- `SetUnits` pins its distances and angle by their asks.

### Regeneration

`crates/regen/src/history/chamfer.rs`, in history order:

- The body needs a solid of its own (`own_solids`). Its topology is the
  one drawing it keeps (`inspect::topology`), and each edge is found on
  it by `Topology::edge(faces, near)`; one not found fails the chamfer
  before the kernel: "its edge wasn't found", or with several "its edge
  2 of 3 wasn't found" (its place in the list).
- What the chamfer shares with the fillet is in
  `crates/regen/src/history/blend.rs` (`find_edges`, `plan`,
  `refused`): finding the edges, growing the chains, their first faces
  and names, the refusals' words and drawings; and the body, its solid,
  topology, cache key's start and the result put in its place, with the
  shell and offset face, in `history/in_place.rs` (`InPlace`).
- **Tangent chains** (with `chains`): each picked edge's chain takes in
  every chain with the same root in `Topology::tangent_chains` (edges
  running on into each other within 1°, from the curves' own end
  tangents). That's purely the topology, so it's done in regen rather
  than behind the kernel stub. A chain is chamfered once: the picked
  edges first, in the list's order, then the grown ones, each taken by
  the first edge reaching it (a tangent chain is gone through once,
  by the first picked edge on it: 256 edges on a 2000-edge rim take
  milliseconds).
- **First faces and names**: a picked edge's first face is the region
  its first key (second with `flip`) names (by the region's own key
  where aliases name both by both keys, else the lower region); a grown
  chain's is the region it shares with its picked edge's first face,
  else the region across from one it shares with the picked edge's other
  face, else its lower-keyed region. That last is a guess: a rim running
  on from a block's edge into a turned body's arc between a cone and a
  cylinder shares no face with it, and its first face should follow the
  side the picked edge's is on as the rim runs (the chains' directions
  through the vertices joining them); the kernel can't make that body
  yet (the join, flush on tangent faces, is too complex), so it waits,
  its test ignored. The kernel gets each chain as a
  `ChamferChain { chain, name, cut }`: `cut` in the chain's region order
  (`ChamferCut::Distances([d0, d1])`, or `Angle { on, distance, angle }`
  with `on` the first face's side), `name` the
  `FacePart::Blend { edge, .. }` edge, `blend_edge(pair, ordinal)` of a
  picked edge's reference keys or a grown chain's regions' keys, the
  ordinal counting chains before it with the same pair.
- **The chamfer**: `varde_kernel::chamfer(solid, topology, chains,
  feature, tol, budget)`, cached as an `Entry::Solid` by the body's key,
  the feature, the fit tolerance and each chain's index, name and cut
  bits; the result replaces the body's solid under that key (an empty
  result: "chamfering Body 1 leaves nothing of it"). Its refusals
  (`BlendError`) are worded by `message::blend_refused` with the edge
  drawn (its curves, as a scale's refused edge): flat "its edge is between
  faces that are nearly flat: there's nothing to chamfer", folded,
  "... turns from convex to concave along its length: chamfer its parts
  apart", too big "the chamfer doesn't fit along its edge 2: it runs past
  a face beside it", an edge grown into "an edge in its edge 2's tangent
  chain ...", a corner "edges of Body 1 meeting at a corner can't be
  chamfered together: chamfer them apart"; its failures as a boolean's
  ("chamfering Body 1 is too complex to work out" for `TooComplex`,
  `message::blending`), the evidence's faces on the body.
- **Kernel stand-in**: `varde_kernel::chamfer` (`kernel/src/blend.rs`,
  with `ChamferChain`, `ChamferCut`, `BlendError`) isn't built yet: it
  has its planned signature and fails with `TooComplex`. So every
  chamfer that finds its edges fails today with "chamfering Body 1 is
  too complex to work out", its body left whole; the rest of the history
  goes on. The regen tests swap it (`chamfer::CHAMFERER`, a thread local;
  other crates' tests through the `testing` feature,
  `varde_regen::testing::chamfer_by_wedges`) for `by_wedges`: each chain
  a straight, open, convex edge between two flat faces cut off by a
  triangular prism past its ends, one boolean each (right for a block's
  edges, too complex otherwise); and for recording and refusing
  stand-ins. The planned analytic tests of the kernel's chamfer are
  written out and `#[ignore = "kernel chamfer not built"]`.
- The draft's reply carries nothing new.

### UI

The move's session (`MotionSession`, above) with `MotionKind::Chamfer`;
**the edge session**, picking the edges a blend cuts, is its own part
(`app/src/doc/motion/blend.rs`, `BlendSetup`; `view/src/motion/blend.rs`,
`BlendEdges`, its Edges field and the Tangent chain tick), its picking
the part it shares with the face session below
(`app/src/doc/motion/refs.rs`: `Refs<R>`, the references picked and
where each is on the model shown, and the `Doc` methods clicking,
lighting, following and taking the selection, generic over `Ref`, which
`EdgeRef` and `FaceRef` implement), shared by
every kind that `MotionKind::blends()` (the chamfer's and the
fillet's, each with its own size); the chamfer's own parts are in
`app/src/doc/motion/chamfer.rs` and `view/src/motion/chamfer.rs`
(`ChamferView`, `ChamferType`). The panel is the model mock's chamfer
panel.

- **Starting**: `Look::StartChamfer` from `C` (`Shortcut::CHAMFER`,
  `chamfer_binding`, the mock's key; in sketches `C` is the Circle
  tool's), the toolbar's Chamfer after Revolve (where the mock has it,
  after Hole and Fillet, before Shell and Combine: Hole isn't built,
  Fillet and Shell don't fit at 1280 px) and the rail's Modify set
  (after Fillet, as the icon mock orders it: Fillet, Chamfer, Shell,
  Scale, Combine, Split body), again backing out; or editing
  one (`Look::EditFeature`: double-click, `Enter`, "Edit chamfer"),
  which opens with its edges, type, values, Flip sides and Tangent
  chain. A new one takes the edges selected in the model shown that a
  click would take (as the mock's takes a hole's rims selected), the
  first one's body deciding. Nothing takes the focus: clicks pick
  edges.
- **Edges** (`MotionPick::Edges`, the viewport picking edges only):
  a click on an edge picks it, named as of the feature
  (`Naming::edge_ref`) on the body holding it there (`Merges::holder`),
  made before it; a click on an edge picked (found again on the model
  shown by its names) takes it out. Once one is picked, the others must
  be on its body: another body's edges don't light under the cursor and
  a click says "A chamfer's edges are all on one body: pick edges of
  Body 1"; a face says "Only an edge can be chamfered". At most
  `MAX_BLEND_EDGES`. Kept in `EdgeRef::order`, so the list is sorted as
  stored. Picked edges are lit as selected, the one under the cursor as
  hovered, each with its tangent chain (`PickIndex::tangent_chain`, the
  topology's rule regen grows chains by) while Tangent chain is on, and
  a click on any edge of a picked edge's lit chain takes that edge out (every edge picked on that chain, where more were picked apart with Tangent chain off);
  a row hovered in the panel lights its edge (`PanelHover::Edge`). The session's bodies are the edges' body, never
  picked itself (Objects' rows pick nothing); a body merged into another
  before the chamfer takes the edges on to its holder. Picks wait only
  for a model of the document as it is (`Doc::refs_model_current`)
  with a draft of this session's run, or for a new chamfer none (not
  another session's preview just ended, whose bodies may be elsewhere),
  not for the preview of the last pick, so edges can be clicked one
  after another. Edges of faces the chamfer itself makes (on its
  preview) are refused by their names, as made later. The Edges field clicked turns picking off
  (`MotionPick::Nothing`) and on.
- **The rows**: "Edge 2" by the edge's place in the list (as
  regeneration's messages count them, "its edge 2 of 3 wasn't found"),
  the mock's `se-edge` icon (`Icon::SeEdge`), or its rim icon
  (`Icon::SeRim`) for a closed round edge; beside it what the model
  shown measures of it where it's found there: a straight edge's length
  ("60 mm"), a closed round one's diameter ("Ø16 mm"), an arc's radius
  ("R8 mm"); a cross takes it out (`MotionLook::DropEdge`). The mock
  numbers the body's edges ("Edge 7") and names rims by their hole;
  numbering by place was taken here.
- **Type**: three tiles (`MotionLook::ChamferType`, the mock's icons
  `ch-equal`, `ch-two`, `ch-angle`: `Icon::ChEqual`, `ChTwo`,
  `ChAngle`): Equal (Distance), Two distances (Distance 1, Distance 2)
  and Distance and angle (Distance, Angle), the fields
  `MotionField::ChamferDistance`, `ChamferSecond`, `ChamferAngle`
  (read by `Chamfer::distance_ask` and `angle_ask`: "1" and "2" of the
  design's units and "45°" to begin with, the mock's), each type
  keeping the others' values. **Flip sides** (`MotionLook::Flip`, the
  mock's `tk-flip`) under them for Two and Angle only, the mock's;
  Equal is stored unflipped. **Tangent chain** (`MotionLook::Chain`,
  `Icon::TkChain`, the mock's `tk-chain`, "Take in edges that run on
  smoothly"), on to begin with: **the mock has this tick on the
  fillet's panel only**; the plan has it on both, so the chamfer's
  panel has it too, last.
- **Whole and ready**: edges, and the type's values ("pick the edges
  to chamfer" otherwise); `Chamfer::check_own` refuses as the panel's
  foot ("Chamfer fails"). Edges the document no longer takes at the
  feature's place (`Document::check_chamfer_edges`, their body not held)
  are kept and said to be gone ("A picked edge is gone", the mock's),
  nothing previewed or committed until taken out or a redo brings them
  back.
- **Preview**: the chamfer as set up is the draft while it's whole,
  picking or not; so on a preview that cuts, the edges picked are gone
  from the model shown (cut off: their rows stay, unmeasured, and a
  row's cross takes one out). An edited one that isn't whole (its edges
  all taken out) is previewed as a move of nothing of its body, which
  shows the body as of the feature with its edges to pick. **The
  kernel's chamfer isn't built**, so today every preview fails with
  "Chamfer fails" over "chamfering Body 1 is too complex to work out",
  the body shown whole (its edges there to pick), OK waits, and Add
  anyway keeps it, failing in the Timeline. Regeneration's own
  refusals ("its edge 2 wasn't found", the flat, folded, too-big and
  corner refusals) show in the panel the same way, the failure's edge
  drawn.
- **Committing**: OK (`Enter`, Add anyway) adds "Chamfer N" or sets the
  edited one, one undo step; Cancel or `Esc` leaves no trace. The
  status bar says "2 edges · Equal · 1 mm · Tangent chain" once whole
  (`chamfer_info`), else what's next, with the hint "Pick edges".

The Timeline shows the model mock's chamfer icon (`Icon::BChamfer`;
`Icon::Chamfer` is the sketch tool's) and note (`view/src/chamfer.rs`:
"1 mm", "1 × 2" along the face of the edges' first key first, so Flip
sides swaps them, "3 mm 30°", as the mock's rows), the status bar's
info "2 edges · Equal · 1 mm · Tangent chain", "1 × 2 mm" for two
distances.

Departures from the mock: the Tangent chain tick (above); edges named
by their place; no bands drawn over the model (the mock's preview draws
each edge's chamfer as a band on the faces; here the preview is the
model regenerated with the chamfer). Known gaps: an edge cut off in the
preview can't be clicked to take it out (its row's cross does); the
rows' measures are the model shown's (an edited chamfer's preview holds
the features after it); two picked edges of one tangent chain are both
kept (regen chamfers the chain once).

Tests: `document/src/chamfer/tests.rs` (added and undone, edited, its
own parts, bodies and makers, removal following the body and not the
faces, units pinned, round trip, wrong chamfers refused when read, the
eleventh kind, errors), `regen/src/history/tests/chamfer.rs` (the stub
failing as too complex with the history going on; an edge gone after
its face's maker is removed; an edge found again after an upstream
dimension change, cut where it went; with the prism stand-in: two
distances, flipped, an angle on the right faces, a block's top loop and
all twelve edges by their volumes, the cache; recording: a slot's rim
taken in as four chains named apart with the top the first face all
round, only the one without chains; a refusal named and drawn; ignored:
the kernel's on a block's edges, a hole's and a boss's rims by Pappus,
determinism; a 2000-gon's rims grown from 256 edges in well under a
second; a rim sharing no face with the picked edge, ignored until the
kernel joins flush on tangent faces), `regen/src/history/tests/chamfer/fuzz.rs`
(random histories of blocks, discs, joins, cuts, combines and chamfers
by the stand-in of one to four edges of every size, now and then named
on a used-up body, chamfers edited, upstream extrudes changed, removals,
undo and redo, drafts: each chamfer failing alike whole and cut short,
one failing changing nothing, one working taking material off its body
alone; cache warm and cold alike; edits, bytes and the wire;
`VARDE_TESTS=full`), `io/src/vrdp/tests.rs` (through a file, a
tampered edge point refused, a damaged chamfers' record refused or
checked, edges out of order, repeated, on another body, none or past
the limit and values disagreeing refused as read), `view/src/chamfer/tests.rs` (the notes),
`view/src/motion/tests.rs` (the chamfer's panel: its order for each
type, the rows' measures beside them, no Flip sides for Equal, the
status text), `rail/tests.rs` (Chamfer's `C` in the Modify set) and
`app/src/doc/motion/tests/chamfer.rs` (`C` on the example plate: a face
refused, edges hovered and lit, picked and listed sorted with their
lengths, clicked again taken out, the row hovered lighting its edge, the
stand-in's too-complex failure in the panel, OK waiting and Add anyway
keeping it as one undo step; a slot's rim lit whole under the cursor
and picked, the line alone with Tangent chain off, a click on its arc
taking the line out; with the prisms, the cut previewed and the
edge gone from the model shown, Two distances, Flip sides, an angle of
90° refused under its field, 30°, Tangent chain off, OK and undo, a
row's cross; another body's edges neither lit nor taken, Objects' rows
picking nothing, a disc's rim with its diameter; editing from the
Timeline, Cancel, another distance, undo, and its edges all taken out
previewed as a move of nothing; an edge an undo takes away said to be
gone, back on redo; the edges selected taken in, `C` again backing
out; edges not following their body into a combine that fails for
naming a body merged in before it; the toolbar at 1280 px with a long
name cut short in the pill), `app/src/doc/motion/tests/chamfer/fuzz.rs`
(random sessions: edges clicked on the model shown or one gone by,
faces and other bodies' among them, rows' crosses, Tangent chain,
types, Flip sides, values out of range or not numbers, units, rows
hovered, undo and redo, merges, an edit taking every edge out then a
merge, commits, Add anyway, cancels; each step the session sorted on
one body, ready only when its checks pass and previewed as set up,
committed as drafted, edits opening to what they store, the edges lit
on the body drawing theirs; at the end each working chamfer takes
material off; `VARDE_TESTS=full`). The app's tests chamfer by prisms through regen's `testing`
feature (`varde_regen::testing::chamfer_by_wedges`).

## Shell

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
  `Shell::check_own(design)` (cheap): at most 256 faces (`Faces`), each
  face's own check (`Face(PlaneError::Near)`: its point finite and
  within `MAX_COORD`), in order without repeats (`FaceOrder`), all on
  the shell's body (`Bodies`), the thickness by its ask (`Thickness`).
  `Document::check_face_set(index, body, faces)` (public, for the
  panel; an offset face's faces too, its `FaceSetError` taken into
  each feature's error): the body there and made before (`Body`:
  depended on, as a combine's bodies), and every face key's feature
  before it, or not there with an id below the next (`RefMaker`, as a
  sketch's face's).
- **Dependencies**: `FeatureKind::bodies()` is its body, so removing it
  or its maker removes the shell. The features that made its open faces
  are **not** followed: removing a cut whose wall it opens leaves the
  shell, which then fails ("its open face wasn't found"), to be edited
  (decided, as every face and edge reference).
- `SetUnits` pins its thickness by its ask.

### Regeneration

`crates/regen/src/history/shell.rs`, in history order (the steps it
shares with chamfer, fillet and offset face, the body's own solid and topology,
its faces found, the cache key's start and the result put in place,
are `history/in_place.rs`'s `InPlace`, with `box_sides` for the
stand-ins):

- The body needs a solid of its own (`own_solids`: a body consumed by
  a join or combine fails it, "Body 2 is in Body 1 now: ..."). Its
  topology is the one drawing it keeps (`inspect::topology`), and each
  open face is found on it by `Topology::face(key, near)`; one not found
  fails the shell before the kernel: "its open face wasn't found", or
  with several "its open face 2 of 3 wasn't found" (its place in the
  list). The regions found go to the kernel sorted, each once.
- **The shell**: `varde_kernel::shell(solid, topology, open, thickness,
  outward, feature, tol, budget)`, cached as an `Entry::Solid` by the
  body's key, the feature, the fit tolerance, the thickness's bits, the
  direction and the regions; the result replaces the body's solid under
  that key (an empty result: "shelling Body 1 leaves nothing of it").
  Its refusals (`varde_kernel::ShellError`) are worded by
  `message::shell_refused`: a round shrinking to nothing "the shell is
  thicker than the smallest round of Body 1: try a thinner wall" (the
  face drawn), the offsets crossing "the shell is too thick for Body 1:
  its walls would run into each other", a corner of more than three
  faces "faces of Body 1 meeting at a corner can't be offset together:
  try another thickness" (the vertex drawn); its failures as a
  boolean's ("shelling Body 1 is too complex to work out" for
  `TooComplex`, `message::shelling`), the evidence's faces on the body.
- **Kernel stand-in**: `varde_kernel::shell` (`kernel/src/shell.rs`,
  with `ShellError`) isn't built yet: it has its planned signature and
  fails with `TooComplex`. So every shell that finds its faces fails
  today with "shelling Body 1 is too complex to work out", its body left
  whole; the rest of the history goes on. The regen tests swap it
  (`shell::SHELLER`, a thread local; other crates' tests through the
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

### UI

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
  today every preview fails with "Shell fails" over "shelling Body 1 is
  too complex to work out", the body shown whole (its faces there to
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
lighting its face, the stand-in's too-complex failure in the panel, OK
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

## Fillet

`crates/document/src/fillet.rs`.

```rust
pub struct Fillet {
    pub edges: Vec<EdgeRef>,   // as a chamfer's: 1..=MAX_BLEND_EDGES, one body, EdgeRef::order, no repeats
    pub radius: Value,         // a length as an extrude's
    pub chains: bool,          // take in each edge's tangent chain
}
```

- **What it is**: the thirteenth variant (`FeatureKind::Fillet`, "Fillet
  N"). Its edges are rounded off to one radius on the body they're on,
  which keeps its id; it makes no body. Convex edges lose material,
  concave ones gain it (the kernel's to tell). The radius is a length
  as an extrude's (`Fillet::radius_ask` = `Extent::ask`); one radius
  per fillet (constant-radius fillets, as decided), so unequal radii
  meet only across features. Its edges are as a chamfer's: all on one
  body, kept in `EdgeRef::order` without repeats; with no first faces
  there's no Flip. The plan's struct, as built.
- **Checks** (`CheckError::Fillet(id, FilletError)`):
  `Fillet::check_own(design)` (cheap): its edges by
  `check_blend_edges_own` (`Edges(BlendEdgesError)`, the chamfer's
  checks), the radius by its ask (`Radius`); `Document::check_blend_edges`
  for what they name, as a chamfer's.
- **Dependencies**: `FeatureKind::bodies()` is the edges' body, so
  removing it or its maker removes the fillet; the features that made
  its edges' faces are not followed (it then fails, "its edge wasn't
  found"), as a chamfer's.
- `SetUnits` pins its radius by its ask.

### Regeneration

`crates/regen/src/history/fillet.rs`, in history order:

- The body, its topology, the edges found on it ("its edge wasn't
  found", "its edge 2 of 3 wasn't found"), tangent chains grown and
  each chain's `FacePart::Blend` name: all as a chamfer's
  (`history/blend.rs`; a fillet has no use for the first faces, so
  they're worked out unflipped and left).
- **The fillet**: `varde_kernel::fillet(solid, topology, chains,
  radius, feature, tol, budget)`, each chain a `FilletChain { chain,
  name }`, cached as an `Entry::Solid` by the body's key, the feature,
  the fit tolerance, the radius's bits and each chain's index and name;
  the result replaces the body's solid under that key (an empty result:
  "filleting Body 1 leaves nothing of it"). Its refusals
  (`BlendError`) are worded by `message::blend_refused` with the edge
  drawn, as a chamfer's with its words: "its edge is between faces that
  are nearly flat: there's nothing to fillet", "... fillet its parts
  apart", "the fillet doesn't fit along its edge 2: it runs past a face
  beside it", and the fillet's own `BlendError::End`, "the fillet along
  its edge 2 runs into another face at its end"; a corner "edges of Body
  1 meeting at a corner can't be filleted together: fillet them apart";
  its failures as a boolean's ("filleting Body 1 is too complex to work
  out", `message::blending`).
- **Kernel stand-in**: `varde_kernel::fillet` (`kernel/src/blend.rs`,
  with `FilletChain`) isn't built yet: it has its planned signature and
  fails with `TooComplex`. So every fillet that finds its edges fails
  today with "filleting Body 1 is too complex to work out", its body
  left whole; the rest of the history goes on. The regen tests swap it
  (`fillet::FILLETER`, a thread local; other crates' tests through the
  `testing` feature, `varde_regen::testing::fillet_by_arcs`) for
  `by_arcs`: each chain a straight, open, convex edge between two flat
  faces, rounded off by one boolean taking away a prism past its ends
  whose section is the corner between the edge and the rails (at `r /
  tan(θ/2)` along each face, `θ` the angle between the faces) less the
  round's circle, widened outwards along each face's normal: right for
  a block's edges and a prism's obtuse and acute ones (square ends); a
  rail past its face's reach across is `TooBig`; anything else too
  complex. So that no test takes a wrong solid from it, it refuses what
  it can't do right: each boolean must take off exactly the corner's
  section along the edge, `r·b − r²(π − θ)/2` per length (`b` the
  rails' distance from the edge), else too complex (a prism running
  past an end into more of the body, a wall standing out past the
  edge's end); and two chains' strips on a face they share (from the
  edge to the rail) must keep apart: meeting at a corner too complex
  (a block's top loop, which the kernel mitres; the test takes that
  refusal or the mitred volume, never another), elsewhere `TooBig` for
  the later chain (a 3 mm rib's two top edges at R2, whose rounds
  would overlap on its top). The planned analytic tests of the
  kernel's fillet are written out and `#[ignore = "kernel fillet not
  built"]`.
- The draft's reply carries nothing new.

### UI

The move's session (`MotionSession`, above) with `MotionKind::Fillet`,
its edges picked by **the edge session** the chamfer's has (see
"Chamfer": `BlendSetup`, `Refs<EdgeRef>`, `BlendEdges`, its Edges field
and the Tangent chain tick; `MotionKind::blends()` is the chamfer's and
the fillet's). The fillet's own parts are in `app/src/doc/motion/fillet.rs`
and `view/src/motion/fillet.rs` (`FilletView`). The panel is the model
mock's fillet panel: Edges, Radius, Tangent chain.

- **Starting**: `Look::StartFillet` from `F` (`Shortcut::FILLET`,
  `fillet_binding`, the mock's key; in sketches `F` is the Fillet
  tool's) and the rail's Modify set (first, as the icon mock orders it:
  Fillet, Chamfer, Shell, Scale, Combine, Split body), again backing
  out; or editing one (`Look::EditFeature`: double-click, `Enter`,
  "Edit fillet"), which opens with its edges, radius and Tangent chain.
  **Not on the toolbar**: the mock's model bar has Fillet before
  Chamfer, but with it the bar runs past its room at 1280 px (to 1210
  px of 1160), as Shell would, so it waits for the toolbar's overflow
  handling. A new one takes the edges selected that a click would
  take, the first one's body deciding. Nothing takes the focus: clicks
  pick edges.
- **Edges**: as the chamfer's, in the fillet's words: a face clicked
  says "Only an edge can be filleted", another body's edge "A fillet's
  edges are all on one body: pick edges of Body 1"; rows "Edge 2" with
  the length, diameter or radius beside them, a cross taking one out,
  a row hovered lighting its edge, each with its tangent chain while
  Tangent chain is on. Picks wait only for a model of the document as it
  is with this session's draft (`Doc::refs_model_current`).
- **Radius** (`MotionField::Radius`, read by `Fillet::radius_ask`: "2"
  of the design's units to begin with, the mock's), and **Tangent chain**
  (`MotionLook::Chain`, on to begin with, the mock's `tk-chain` with its
  hint "Take in edges that run on smoothly"). No Type and no Flip sides
  (`MotionLook::Flip` does nothing for a fillet). The mock's "Too big for
  Edge 2: under 3 mm" under the field (its own estimate of the room
  beside each edge) is regeneration's here: the kernel's refusal ("the
  fillet doesn't fit along its edge 2: ..."), in the foot.
- **Whole and ready**: edges and a radius ("pick the edges to fillet"
  otherwise); `Fillet::check_own` refuses as the panel's foot ("Fillet
  fails"); edges the document no longer takes at the feature's place
  (`Document::check_blend_edges`, their body not held) kept and said to
  be gone ("A picked edge is gone"), nothing previewed or committed
  until taken out or a redo brings them back.
- **Preview**: the fillet as set up is the draft while it's whole,
  picking or not, so on a preview that rounds, the edges picked are gone
  from the model shown (their rows stay, unmeasured). An edited one with
  its edges all taken out is previewed as a move of nothing of its body.
  **The kernel's fillet isn't built**, so today every preview fails with
  "Fillet fails" over "filleting Body 1 is too complex to work out", the
  body shown whole, OK waits, and Add anyway keeps it, failing in the
  Timeline.
- **Committing**: OK (`Enter`, Add anyway) adds "Fillet N" or sets the
  edited one, one undo step; Cancel or `Esc` leaves no trace. The status
  bar says the mock's row info once whole ("2 edges · R2 mm · Tangent
  chain", `fillet_info`), else what's next, with the hint "Pick edges"
  (the chamfer's: a shared `chrome::step_hint` for what a click picks
  next isn't on this branch yet).

The Timeline shows the model mock's fillet icon (`Icon::BFillet`;
`Icon::Fillet` is the sketch tool's) and note ("R2", `view/src/fillet.rs`);
selected, the status bar says `fillet_info`.

Departures from the mock: no Fillet on the toolbar (above); edges named
by their place; no bands drawn over the faces (the mock's preview draws
each edge's round as a band; here the preview is the model regenerated
with the fillet); the mock's own "too big" estimate left to the kernel.
Known gaps: as the chamfer's (an edge rounded off in the preview can't
be clicked to take it out, its row's cross does; the rows' measures are
the model shown's; two picked edges of one tangent chain both kept).

Tests: `document/src/fillet/tests.rs` (added and undone, edited, its own
parts, bodies and makers, removal following the body and not the faces,
units pinned, round trip, wrong fillets refused when read, the
thirteenth kind, errors), `regen/src/history/tests/fillet.rs` (the stub
failing as too complex with the history going on; an edge gone after
its face's maker is removed, named by its place among others; an edge
found again after an upstream dimension change, rounded where it went;
with the stand-in: a block's edge (an exact cylinder of the radius) and
two opposite ones, its top loop mitred or refused, a prism's obtuse and
acute edges by `r²(cot(θ/2) − (π − θ)/2)` per length, joins after a
fillet clear of the round and across it by their volumes, the cache, a
radius past the faces refused, a 3 mm rib's two top edges at R2
refused as too big (alone, or at 1.4 mm, rounded), an edge running into
a wall standing out past its end refused or rounded along its own
length only; recording: a slot's rim taken in as four
chains named apart with the radius as typed, only the one without
chains; refusals (too big, an end running into a face) worded and
drawn; a consumed body; ignored: the kernel's on a block's edge, top
loop and all twelve (the rounded box), a hole's and a boss's rims and a
slot's rim by Pappus, determinism),
`regen/src/history/tests/fillet/fuzz.rs` (the chamfer's history fuzz
with fillets of every radius: each failing alike whole and cut short,
one failing changing nothing, one working taking material off its body
alone and leaving a round of its radius; `VARDE_TESTS=full`),
`io/src/vrdp/tests.rs` (through a
file, a tampered edge point and radius refused, a record's fillets
damaged 2 000 ways refused or checked, edges out of order, repeated, on
another body, none or past the limit, named by the fillet or later and
a radius refused as read), `view/src/fillet/tests.rs` (the notes),
`view/src/motion/tests.rs` (the fillet's panel: its order, the rows'
measures, no Type or Flip sides, the status text), `rail/tests.rs`
(Fillet's `F` first in the Modify set) and
`app/src/doc/motion/tests/fillet.rs` (`F` on the example plate: the
mock's panel, a face refused, edges picked, lit, listed sorted with
their lengths and taken out again, a row hovered lighting its edge, the
stand-in's too-complex failure in the panel, OK waiting and Add anyway
keeping it as one undo step, failing in the Timeline; with the
regeneration stand-in, the round previewed as a cylinder of the radius
and the edge gone from the model shown, another radius, a radius of
nothing refused under its field, Tangent chain off, Flip doing nothing,
OK and undo; editing from the Timeline, Cancel, another radius, undo,
its edges all taken out previewed as a move of nothing; an edge an undo
takes away said to be gone, back on redo; the edges selected taken in,
`F` again backing out; another body's edge refused in the fillet's
words; a slot's rim lit whole and the line alone with Tangent chain
off; the overlap list ticking and picking the fillet's edges),
`app/src/doc/motion/tests/fillet/fuzz.rs` (the chamfer's session fuzz
with radii, plus quick clicks with nothing answered, `F` while
measuring or with a combine or chamfer set up, and the overlap list
opened, hovered, ticked and chosen across models answered and undo;
`VARDE_TESTS=full`); the chamfer's toolbar test checks a fillet
session's bar fits and
that Fillet isn't on the idle bar. The app's tests fillet through
regen's `testing` feature (`varde_regen::testing::fillet_by_arcs`).

## Offset face

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
  `OffsetFace::check_own(design)` (cheap): at least one face
  (`NoFaces`) and at most 256 (`Faces`), each face's own check
  (`Face(PlaneError::Near)`), in order without repeats (`FaceOrder`),
  all on one body (`Bodies`), the distance by its ask (`Distance`).
  `Document::check_face_set(index, body, faces)` as a shell's, with
  the first face's body (`Body`, `RefMaker`).
- **Dependencies**: `FeatureKind::bodies()` is its faces' body, so
  removing it or its maker removes the offset. The features that made
  its faces are not followed: removing one leaves the offset failing
  ("its face wasn't found"), as every face reference.
- `SetUnits` pins its distance by its ask.

### Regeneration

`crates/regen/src/history/offset_face.rs`, in history order:

- The body needs a solid of its own (`own_solids`). Each face is found
  on the body's topology (`inspect::topology`, `Topology::face(key,
  near)`); one not found fails the offset before the kernel: "its face
  wasn't found", or with several "its face 2 of 3 wasn't found". The
  regions go to the kernel sorted, each once.
- **The offset**: `varde_kernel::offset_faces(solid, topology, faces,
  signed distance, tangent, feature, tol, budget)`, cached as an
  `Entry::Solid` by the body's key, the feature, the fit tolerance, the
  signed distance's bits, the tangent flag and the regions; the result
  replaces the body's solid under that key. Since faces keep their
  names, a sketch on a moved face after the offset is placed on it
  where it went (`Evaluation::placements`), and follows a change of the
  distance or of a dimension upstream. Its refusals
  (`varde_kernel::OffsetError`) are worded by `message::offset_refused`:
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
  1 is too complex to work out", `message::offsetting`); an empty
  result "offsetting faces of Body 1 leaves nothing of it".
- **Kernel stand-in**: `varde_kernel::offset_faces` (`kernel/src/shell.rs`,
  beside `shell`, with `OffsetError`) isn't built: it fails with
  `TooComplex`, so every offset that finds its faces fails today with
  "offsetting faces of Body 1 is too complex to work out", the body left
  as it was, and the rest of the history goes on. The regen tests swap
  it (`offset_face::OFFSETTER`, a thread local; other crates' tests
  through the `testing` feature, `varde_regen::testing::offset_by_boxes`)
  for `by_boxes`: a box along the world's axes whose picked faces move
  by a scale along each axis and a move (`Motion::scale` then a
  translation, `Solid::transformed`), so every face keeps its name as
  the kernel's will; a face moved onto or past the one opposite it is
  `PastNeighbour`, a box past the coordinate limit `OutOfRange`, the
  rest too complex. The planned analytic tests of the kernel's offset
  face are written and `#[ignore = "kernel offset face not built"]`.
- The draft's reply carries nothing new.

### UI

The move's session (`MotionSession`) with `MotionKind::OffsetFace`, its
faces picked by the face session (see the shell's UI: `faces.rs`,
`refs.rs`, `PickedFaces`); its own parts in
`app/src/doc/motion/offset_face.rs` and `view/src/motion/offset_face.rs`
(`OffsetFaceView`, `FaceHandle`), its handle in
`view/src/viewport/motion.rs`. **The model mock has no offset face
panel** (only the `offset` icon and "Offset face" in a face's context
menu), so the panel is built in the style of the mock's shell panel.

- **Starting**: `Look::StartOffsetFace` from the rail's Modify set
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
  messages' article follows the noun, `refs::article`), an edge "Only a
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
  `MotionLook::DragKnob` (or `OffsetBy { distance, inward }`, which
  tests send) sets the field to the size and Inward to the side; zero
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
  "Offset face fails" over "offsetting faces of Body 1 is too complex to work
  out", OK waits, and Add anyway keeps it, failing in the Timeline.
- **Committing**: OK (`Enter`, Add anyway) adds "Offset face N" or sets the
  edited one, one undo step; Cancel or `Esc` leaves no trace. The
  status bar says `offset_info` once whole ("1 face · 2 mm inward"),
  else what's next; the hint "Pick faces to move" (`chrome::step_hint`).

The Timeline shows the mock's offset face icon (`Icon::OffsetFace`, the
model mock's `offset`) and note (`view/src/offset_face.rs`: "2 mm · 3
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
refused, the top picked, lit and listed, the stand-in's too-complex
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

## Draft

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
  `FaceDraft::check_own(design)` (cheap): at least one face (`NoFaces`),
  at most 256 (`Faces`), each face's own check (`Face`), in order without
  repeats (`FaceOrder`), all on one body (`Bodies`), the neutral face's
  own check (`Neutral`), the angle by its ask (`Angle`).
  `Document::check_face_set` as an offset face's (`Body`, `RefMaker`),
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

### Regeneration

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
- **The draft**: `varde_kernel::draft_faces(solid, topology, faces,
  neutral point, pull, angle in radians, tangent, feature, tol,
  budget)`, cached as an `Entry::Solid` by the body's key, the feature,
  the fit tolerance, the neutral point's and pull's bits, the angle's
  bits, the tangent flag and the regions; the result replaces the
  body's solid under that key. Faces keep their names, so a sketch on a
  drafted face after it is placed on the face as turned, and follows
  the angle, the flip or a dimension upstream. Its refusals
  (`varde_kernel::DraftError`) are worded by `message::draft_refused`:
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
  work out", `message::drafting`); an empty result "drafting faces of
  Body 1 leaves nothing of it".
- **Kernel stand-in**: `varde_kernel::draft_faces` (`kernel/src/shell.rs`,
  beside `offset_faces`, with `DraftError`) isn't built: it fails with
  `TooComplex`, so every draft that finds its faces and neutral plane
  fails today with "drafting faces of Body 1 is too complex to work
  out", the body left as it was, and the rest of the history goes on.
  The regen tests swap it (`face_draft::DRAFTER`, a thread local; other
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

### UI

The move's session (`MotionSession`) with `MotionKind::Draft`, its faces
picked by the face session (see the shell's UI: `faces.rs`, `refs.rs`,
`PickedFaces`) and its neutral plane in the session's `plane`, picked as
a mirror's; its own parts in `app/src/doc/motion/face_draft.rs` and
`view/src/motion/draft.rs` (`DraftView`). **The model mock has no draft
panel** (only the icon mock's `draft` icon, in the Modify group after
Shell), so the panel is built in the style of the mock's shell panel.

- **Starting**: `Look::StartDraft` from the rail's Modify set (after
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
  is taken (`Doc::reference_of`, named as of the feature; anything else
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
  is too complex to work out", OK waits, and Add anyway keeps it,
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
too-complex failure in the panel, OK waiting and Add anyway keeping it,
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

## Sweep

`crates/document/src/sweep.rs`.

```rust
pub struct Sweep {
    pub sketch: FeatureId,              // the profile's sketch, before it
    pub regions: Vec<RegionRef>,        // 1..=MAX_SWEEP_REGIONS (256)
    pub path: PathRef,
    pub orientation: Orientation,       // FollowPath (default) | Keep; FollowPath with a helix
    pub twist: Option<Value>,           // an angle over the whole path, within 8 turns; none with a helix
    pub operation: Operation,           // an extrude's
}
pub enum PathRef { Chain(Vec<PathPart>), Helix(Helix) }             // parts 1..=MAX_PATH_PARTS (64)
pub enum PathPart { Curves(CurveChain), Edges { edges: Vec<EdgeRef>, tangent: bool } }
pub struct CurveChain { pub sketch: FeatureId, pub curves: Vec<Id> }  // sorted, no repeats
pub struct Helix { pub axis: AxisRef, pub pitch: Value, pub turns: Value, pub left_handed: bool, pub flip: bool }
```

- **What it is**: the sixteenth variant (`FeatureKind::Sweep`, "Sweep
  N"), appended after the draft (`a_sweep_is_the_sixteenth_kind` is
  the one test that writes its index down). The profile's regions, made as an extrude's,
  moved along the path into a tool solid that makes a new body or
  joins, cuts or intersects as an extrude's does (`operation()`,
  `new_body()`, excluded bodies dropped as theirs). The path is chains
  of other sketches' curves and of model edges, joined end to end by
  regeneration in whatever order they're listed, or a helix alone:
  about its axis (an origin axis, a straight or round edge, a round
  face, as a move's turn's), `pitch` a turn along it (a length as an
  extrude's distance), `turns` (`1e-3 ..= 1000`, a number), right-handed
  unless `left_handed`, `flip` reversing the axis. The profile's plane
  holds a helix's axis (each profile point runs on a helix of its own);
  for a chain it's square to the path at its start.
- **Checks** (`CheckError::Sweep(id, SweepError)`):
  `Sweep::check_own(design)` (cheap): 1..=256 regions each checked;
  for a chain 1..=64 parts, none empty (`EmptyPart`), at most 1 024
  curves and edges in all (`PathCurves`, summed checked as it goes), a
  part's curves sorted without repeats (`CurveOrder`), its edges each
  checked on their own (`Edge`), in `EdgeRef::order` without repeats
  (`EdgeOrder`), all on one body (`EdgeBodies`); for a helix,
  FollowPath and no twist (`HelixOptions`), its axis checked on its own
  as a move's (`Axis`), its pitch and turns by their asks
  (`Sweep::pitch_ask`, `turns_ask`); a twist by `twist_ask` (an angle
  within `MAX_TWIST_TURNS` turns either way). `Document::check` then
  wants the profile's sketch a sketch before it and the operation as an
  extrude's, and `Document::check_path(index, profile, path)` (public,
  for the panel): each part's sketch a sketch before it
  (`PathSketch`) other than the profile's (`OwnSketch`: a path in the
  profile's plane can't be square to it), each edge part's body there
  and made before it (`EdgeBody`: depended on) and its faces' makers
  before it or not there with ids no later feature can take
  (`EdgeMaker`), a helix axis's body likewise (`AxisBody`,
  `AxisMaker`). On add and set only (`check_new`): a part's curves are
  curves of its sketch (`Sweep::check_curves`, `Curve`); a later edit
  deleting one makes regeneration fail it ("path not found"). Several
  parts may name one sketch: joining them is regeneration's.
- **Dependencies**: `uses()` is the profile's sketch and every part's
  sketch, so removing a path's sketch removes the sweep (and the body
  it makes), as removing the profile's does; `bodies()` is the edge
  parts' bodies and a helix axis's body (an edge or a face), so
  removing them or their makers removes it too (the plan's dependency
  list; a move's axis body, by contrast, isn't followed). The features
  that made the edges' faces are not followed: removing one leaves the
  sweep, which then fails ("its path edge wasn't found"). Adding a
  sweep hides its profile's sketch, not its path's (see UI).
- `SetUnits` pins its twist and a helix's pitch by their asks (and its
  turns, which a bare number leaves as typed).

### Regeneration

`crates/regen/src/history/sweep.rs`, in history order, as part of the
extrude's and revolve's run (`Run`, `Shape::Sweep`): the tool, then a
new body, or touches and booleans exactly as an extrude's (the touched
bodies noted, excluded bodies kept, merges, "it doesn't touch any
body", the uncut note).

- **The profile**: its sketch placed (else "its sketch isn't placed"),
  its regions merged into a kernel `Profile` as an extrude's, on the
  `Frame` of the placement.
- **A sketch part**: its sketch must be placed ("its path's sketch
  isn't placed"); its curves ordered by `profile::path_chain` (the
  split's `profile::chain`, which now also takes a closed chain for a
  path: a circle alone, from its `+x` point round counter-clockwise, a
  closed spline alone, or curves joining into a loop, run as the lowest
  curve runs, its last conic ending on the first's start to the bit;
  ends joined within the resolution): "path not found" for a curve
  gone, "its path's curves don't join end to end into one line" for
  branches or pieces, "its path has a closed part and others" for a
  closed curve among others; then mapped into the world by the
  placement (control points mapped, weights kept: exact), one piece per
  curve: a line `Piece::Line`, an arc or circle `Piece::Arc` about its
  centre, right-handed about the sketch's normal where it turns
  counter-clockwise in the sketch and against it otherwise, a spline
  `Piece::Curve` with the sketch's normal.
- **An edge part**: its body's holder must have a solid ("its path
  edge's body is gone"); each edge found on the topology drawing it
  keeps (`inspect::topology`) by keys and point ("its path edge wasn't
  found", "its path edge 2 of 3 wasn't found"); with `tangent` every
  chain sharing a root in `Topology::tangent_chains` taken in (as a
  chamfer's chains grow); the chains ordered end to end by their
  vertices (mesh vertex ids, never distance): three ends at a vertex or
  chains in several pieces "its path's edges don't join end to end into
  one line", a closed chain (a rim) only alone; each chain one piece by
  `edge_shape`: a line, an arc about its circle's centre (its axis
  negated where the walk runs it backwards), anything else a curve of
  its conics with no plane.
- **Joining**: a closed part must be the only one (its joints, the
  closing one too where it's several pieces, checked: a piece closed on
  itself, a circle, a closed spline or a rim, has none; the start where
  the profile's plane crosses it is the kernel's to find). Otherwise the
  first part is the one with an end within the resolution of the
  profile's plane, of several the end nearest the middle of the
  profile's box (a choice by distance, not a merge, the profile made
  for it only then; none: "its path doesn't start on its profile's
  plane"),
  run from that end; each next the only part with an end within the
  resolution of the chain's end (none or several: "its path's parts
  don't join into one chain: there's a gap or a branch between them"),
  reversed as needed, the gap carried, not closed. Then every joint
  between pieces (between a sketch part's curves, an edge part's chains
  and the parts; not inside a piece, whose conics a traced chain fits
  only to the tolerance) must be tangent-continuous: the unit tangents
  (from the control points: `c − p0` leaving, `p1 − c` arriving)
  pointing the same way within a sine of `1e-6` (the solver holds
  tangent joints to `1e-10`), else "its path has a corner: sweep each
  side of it apart and join them", the joint drawn as a point; and the
  start's tangent square to the profile's plane within a sine of
  `1e-6`, else "its profile isn't square to its path where the path
  starts". These are regen's own refusals, before the kernel, by the
  kernel's rule (`varde_kernel::sweep::JOINT_SINE`, the sine both
  use), which the kernel's sweep keeps as a guard.
- **A helix**: its axis resolved as a move's turn's
  (`motion::resolve_axis`: its messages, "its axis edge wasn't found"
  and the rest), reversed with `flip`, noted in
  `Evaluation::references` (so a draft's reply carries it, for drawing),
  made unit ("its helix's axis has no direction"); `pitch × turns`
  within `MAX_COORD` (both checked, so finite), else "its helix is too
  long".
- **The kernel**: `varde_kernel::sweep::sweep(profile, frame, path,
  orientation, twist, feature, tol, budget)` (`Orientation::Follow` or
  `Keep`, the twist's value or 0), cached as an `Entry::Solid` by the
  feature, the regions, the fit tolerance, the profile sketch's key and
  placement, the orientation, the twist's bits and every number of the
  path as built (so an edit that leaves the path where it was, a model
  edge on a body changed elsewhere say, finds the tool again; building
  the path again is linear in its curves and edges, the topology cached
  by the body's key). Its refusals (`SweepError`) are worded by
  `message::sweep_refused`: a corner (drawn), off the start, not square,
  "its path bends tighter than its profile ...", "its path turns
  parallel to its profile: follow the path instead of keeping the
  orientation", "its profile's plane must hold its helix's axis", "its
  profile reaches its helix's axis", "its helix's pitch is smaller than
  its profile: neighbouring turns would meet", "the sweep runs into
  itself"; its failures as a tool's (`message::tool` with
  `Making::Sweep`: too complex is "sweeping its regions along its path
  is too complex to work out").
- **Kernel stand-in**: the kernel's sweep (`kernel/src/sweep/path.rs`)
  isn't built yet: it fails with `TooComplex`. So every sweep whose path
  regen builds fails today with that message, making no body (its new
  body has no solid; a feature naming it fails as for any), and the rest
  of the history goes on. The regen tests swap it (`sweep::SWEEPER`, a
  thread local; other crates' tests through the `testing` feature,
  `varde_regen::testing::sweep_by_extrude`) for `by_extrude`: a path of
  straight pieces along one line from the profile's plane, square to
  it, untwisted, extruded along it as far as it reaches (a box's volume
  for a rectangle); anything else too complex; and for a recording
  stand-in that checks the path handed over. The planned analytic tests
  of the feature with the kernel's sweep (a pipe joined to a plate at
  its start, cut through a block, a bead along a round rim following
  it, a spring joined to a plate) are written out in
  `history/tests/sweep.rs` and `#[ignore = "kernel sweep not built"]`.
- The draft's reply carries a helix's axis as `Drafted::reference`, as
  a move's (checked on the wire as theirs).
- **Where its handles stand** (`SweepFound`, noted in
  `Evaluation::swept` once the path is built, before the kernel is
  asked; the draft's reply carries it as `Drafted::sweep`, checked on
  the wire: points within `MAX_REFERENCE`, finite, directions unit): an
  open chain's end, its unit tangent and the profile's x carried there
  (`SweepFound::End`: rotation-minimizing frames by double reflection
  over the pieces' points, 16 a conic, following the path; the x, or
  the y where x runs along it, taken square to the end's tangent for
  Keep orientation); a helix's profile's middle, its foot on the axis
  and the axis as climbed (`SweepFound::Helix`). None for a closed
  chain. Tested in `history/tests/sweep.rs` (a straight path's and a
  quarter turn's ends, followed and kept, none for a ring; a helix's)
  and `wire/tests.rs`.

### UI

The move's session (`MotionSession`, above) with `MotionKind::Sweep`;
its own parts are in `app/src/doc/motion/sweep.rs` (`SweepSetup`) and
`view/src/motion/sweep.rs` (`SweepView`, `SweepPath`). The UI mock has
no sweep panel (the icon mock has only the tool, Sweep in its Create
group), so the panel is built in the style of its nearest one, the
revolve's: Profile, "Along" as tiles (Path, Helix), the path's fields,
Operation as tiles and the Bodies list.

- **Starting**: `Look::StartSweep` from the rail's Create set (after
  Revolve, as the mock's group orders it; its list's letter `P`, the
  label's first free one, as the mock's) through `sweep_binding`, with
  no key, as the mock has it; again, or another tool, backs out.
  **Not on the toolbar**: the mock's model bar has no Sweep. Editing one
  (`Look::EditFeature`: double-click, `Enter`, "Edit sweep") opens it
  with its regions, path and options. It picks no bodies
  (`MotionSession::bodies` stays empty: `kind()`, `need()`, `gone()`
  and the draft go by the sweep's own); clicks pick, nothing takes the
  focus.
- **Profile** (`MotionPick::Regions`): regions picked as an extrude's
  (`RegionPick`, the sketch selected in the Timeline as the source if
  one is), on its sketches in the viewport (the split's picking: the
  model isn't picked meanwhile, `picks_sketches`); rows "Region 1"
  with a cross, the edited sweep's regions not found counted. The
  first region picked hands the clicks to the path (or a helix's axis)
  while there's none.
- **Path** (`MotionPick::Path`): a click on a curve of a visible sketch
  before the sweep but the profile's (`Doc::sweep_lines`, drawn in the
  live layer while the path is picked, the chain under the cursor
  lit, each part's curves in the selected colour) adds the chain it's
  in (`Sketch::chain_of`, sorted) as a `CurveChain` part, or takes out
  the part holding it; the profile's own sketch is refused with a
  notice. Off the curves, the model's edges are picked as a blend's
  (the move leaving a curve is the model's picking's too, so an edge
  there lights at once, the frame drawing the curve away asked for
  after it, `Input::take_redraw`; `refs`: named as of the sweep, all on one body, the Tangent chain
  tick, lit, `MotionLook::DropEdge`), stored as one `PathPart::Edges`
  after the chains: so a session's edge parts are on one body (an
  edited sweep's other edge parts are kept as stored, listed as "Edges
  of Body 2", and the edited sweep's parts kept in the order it stored
  them while they're the same parts, so OK on it unchanged writes
  nothing). The path's limits hold while it's picked
  (`MotionSession::sweep_room`): a chain or a first edge past 64 parts,
  or a chain or an edge past 1 024 curves and edges in all, is refused
  with a notice ("A sweep's path takes at most 64 parts", "... at most
  1024 curves and edges in all"), nothing added. While the cursor is
  still and the camera moves (a frame drawn, `Moving::redraw`), the
  curve or region under it is worked out again: a curve brought under
  it is hovered and the model let go of (never an edge lit under a
  path curve, where a click takes the curve), one moved away let go of
  and the model under the cursor picked again; while the camera's
  dragged, the curve hovered is let go of (`Input::leave_sketches`);
  a path curve under the cursor holds the model's picking as a move's
  handle does (`Input::holds`). Rows: each chain "Sketch 3 · 2 curves" (`PanelHover::Part`
  lighting its curves), then "Edge 1" with its length; a cross on each.
  Then Tangent chain, Keep orientation and Twist (`MotionField::Twist`,
  "0°" to begin with; a twist of nothing is left out, unless the
  edited sweep stored one).
- **Helix**: the Axis field picked as a move's (`MotionPick::Reference`:
  the toolbar's X, Y and Z, a straight or round edge, a round face),
  Pitch ("10" of the design's units) and Turns ("5"), Left-handed and
  Flip (the session's `flip`; the axis is drawn as a move's, flipped);
  the orientation and twist aren't shown and aren't stored (a path's
  are kept for when its tile is back).
- **Operation and Bodies** as an extrude's (`BodyTargets`, the cut's
  uncut note as the panel's warning, `Doc::held` for a new body a
  combine names).
- **Whole and ready**: regions and a path (or the axis, pitch and
  turns); "pick the regions to sweep", "pick the path: sketch curves or
  model edges", "pick the helix's axis" otherwise. `Sweep::check_own`
  and the document's `check_path` and `check_curves` refuse as the
  foot ("Sweep fails"). What an edit or undo takes away is kept and
  said to be gone ("A path's sketch or curve is gone: take its part
  out", "A picked edge is gone", "The axis is gone: pick another"),
  nothing previewed or committed meanwhile; the profile's sketch gone
  puts its regions by ("The profile's sketch is gone: pick other
  regions"), the visible sketches' regions offered meanwhile, and picks
  them again if it comes back before others are picked, as a split's.
  What a path refused at its place says comes through `motion_held`,
  as a split's tool's: "Its path runs along …", the check's words
  starting "its" or "a" not given an "it".
- **Preview**: the sweep as set up is the draft while it's whole,
  picking or not. **The kernel's sweep isn't built**, so today every
  preview fails with "Sweep fails" over "sweeping its regions along its
  path is too complex to work out", OK waits, and Add anyway keeps it,
  failing in the Timeline. The app's tests sweep straight paths through
  regen's `testing` feature (`varde_regen::testing::sweep_by_extrude`).
- **Committing**: OK (`Enter`, Add anyway) adds "Sweep N" or sets the
  edited one, one undo step; Cancel or `Esc` leaves no trace. Adding
  hides the profile's sketch only (the document's): **path sketches
  stay shown**, so their curves can be clicked when the sweep is
  edited, and a path sketch often serves several sweeps.
- The status bar says "Along Sketch 3 · Follow path · New body" once
  whole (`sweep_info`), else what's next, with the hints "Pick
  regions", "Pick the path's curves or edges" or "Pick the axis"
  (`chrome::step_hint`).

The Timeline shows a sweep with its icon (the icon mock's sweep: a path
and a ring), the note "along Sketch 3", "along Body 1" (one edge part),
"along 3 parts" or "helix · 10 turns", the status bar "Along Sketch 3 ·
Follow path · Twist 90° · New body" (`view/src/sweep.rs`).

Handles: the twist's on a ring about the path's end, a helix's pitch
and turns along its axis (see Operation knobs).

Known gaps: no handle for a closed path's twist; a path sketch hidden by the user isn't offered
(its parts stay listed); edge parts on several bodies only as stored;
a helix's edge or face axis is drawn where the preview's regeneration
found it (an origin axis as it is), 10 mm either side of its point, as
a sweep has no bodies' box.

Tests: `app/src/doc/motion/tests/sweep.rs` (the knobs: the twist's
ring about the path's end found by the preview, dragged either way, to
none, past 8 turns refused; a helix's pitch and turns; the rail's Sweep: the
panel, regions then path, the profile's own sketch refused, a chain a
part, too complex in the panel, Add anyway as one undo step, the
profile's sketch hidden and the path's not; with the stand-in, a
straight path previewed as its box and OK; a part taken out by its
curve or its cross; model edges as a part, a face refused, Tangent
chain, the corner's square up the plate's edge previewed; a helix
about the toolbar's Z, its values, hand and flip drafted, never with
the path's options; Keep orientation, twist and the operation drafted;
editing from the Timeline, Cancel, undo; a path's curves taken away
said to be gone, the profile's sketch removed and put back; the
path's 64 parts and 1 024 curves and edges held while picked; an
edited sweep with an edge part first and two edge parts opening as
stored, OK writing nothing; a helix's axis on a body an undo takes away
gone through Path and Helix toggled, back on redo),
`app/src/doc/motion/tests/sweep/fuzz.rs` (random sessions on two boxes
with profile and path sketches: regions, curves (the profile's own,
ids not curves'), edges and quick clicks, rows' crosses, Path and
Helix, the axis from the toolbar or a click, pitch, turns and twist
typed (out of range, overflowing, not numbers), the options, operation
and bodies, units, hovers, overlap lists, undo and redo, a new profile
sketch mid-session undone, sketches removed, merges, a chamfer being
set up, commits, Add anyway, cancels, edits OK'd straight away: the
path always within its limits, each ready session whole, checked and
previewed as set up, committed as drafted, edges lit on their body's
holder, and each body a working sweep makes its profile extruded from
its plane; `VARDE_TESTS=full`), `regen/src/history/tests/sweep/fuzz.rs`
(random histories with sweeps along sketch chains, model edges and
helices, edited, path sketches redrawn, upstream changes, combines,
undo and redo: each failing alike whole and cut short, a failure
changing nothing, one working only on a path worked out apart from
regeneration to be one the stand-in sweeps and giving exactly an
extrude's bodies, cache warm and cold, bytes and wire;
`VARDE_TESTS=full`), `io/src/vrdp/tests.rs` (also the path's limits,
empty parts, curves out of order, edges on two bodies, a part's sketch
not a sketch, a helix's axis named by the sweep, refused as read),
`viewport/motion/tests.rs` (regions and path curves picked in their
sketches, the model's edges left to the model, an edge hovered by the
move off a curve onto it, the hover handed over between a curve and
the model as the camera moves and let go of while it's dragged),
`rail/tests.rs` (Sweep in the Create set, `P`).

## Loft

`crates/document/src/loft.rs`; `CurveChain` is the sweep's (`crates/document/src/sweep.rs`).

```rust
pub struct Loft {
    pub sections: Vec<Section>,   // 2..=MAX_LOFT_SECTIONS (64), in the order lofted
    pub mode: LoftMode,           // Smooth (default) | Ruled; two sections are ruled whatever
    pub closed: bool,             // the last section lofts back to the first, no caps
    pub rails: Vec<CurveChain>,   // 0..=MAX_LOFT_RAILS (4)
    pub operation: Operation,     // extrude's: NewBody, Join, Cut, Intersect
}
pub enum Section {
    Region { sketch: FeatureId, region: RegionRef, start: Option<Id> },  // one loop; start a sketch point on it
    Point { sketch: FeatureId, point: Id },                              // first or last only
}
pub struct CurveChain { pub sketch: FeatureId, pub curves: Vec<Id> }    // 1..=MAX_RAIL_CURVES (256), sorted
```

- **What it is**: the seventeenth variant (`FeatureKind::Loft`, "Loft
  N"), a solid through its sections in order, each a region of one loop
  of a sketch before it (named as an extrude's regions are) or, first
  or last only, a sketch point, made into a new body or joined, cut or
  intersected as an extrude's tool (`operation()`, `new_body()`). A
  region's `start` is the sketch point its outline starts from (`None`:
  the vertex nearest the previous section's start, the kernel's
  default). `CurveChain` is a sketch's curves making one chain, ordered
  by regeneration, shared with a sweep path's sketch parts.
- **Checks** (`CheckError::Loft(id, LoftError)`): `Loft::check_own()`
  (cheap): 2 to 64 sections (`Sections`), each region's own check
  (`Region`) and no holes in its reference (`Holes(index)`: a section is
  one loop), a point only first or last (`PointInside(index)`), not
  every section a point (`Points`); closed only with three or more
  sections (`ClosedSections`), no point (`ClosedPoint`) and no rails
  (`ClosedRails`: a decision, see below); at most 4 rails (`Rails`),
  each of 1 to 256 curves (`RailCurves`) sorted without repeats
  (`RailOrder`), no rail twice (`RailRepeated`). `Document::check`
  adds: every section's and rail's sketch a sketch feature before it
  (`Sketch`, `RailSketch`), and the operation as an extrude's
  (`NewBody`, `Excluded`, `ExcludedOrder`). On `AddFeature` and
  `SetFeature` only (`check_new`, `Loft::check_names`): a start point
  and a point section's point are points of their sketch (`Start`,
  `Point`), a rail's curves curves of its sketch (`RailCurve`); a later
  sketch edit may take them away, which regeneration reports.
- **Dependencies**: `uses()` is every section's and rail's sketch
  (sorted, no repeats), so removing any of them removes the loft and its
  body. `bodies()` is none (as an extrude's). `sketch()` is `None`;
  `profile_sketches()` is the sections' sketches, which adding the loft
  hides (the rails' sketches stay shown),
  and which the app's failure marks follow (`uses_edited_sketch`).
- `SetUnits` changes nothing: a loft has no values.
- **Decision**: a closed loft takes no rails (the document refuses
  them). A rail is an open chain through every section's matching
  vertex; a closed loft's vertex paths are closed, which an open chain
  can't follow and `profile::chain` doesn't make. The UI should
  turn Rails off with Closed.

### Regeneration

`crates/regen/src/history/loft.rs`, through extrude's `Run` (`Shape::Loft`,
its first section's sketch standing as `Run::sketch`, every sketch
before it in `Run::sketches`), in history order:

- **Sections**, each from its sketch as the features before the loft
  leave it, in the feature's order: a region found again in its
  sketch's profiles (`Profiles::resolve`; gone: "section 2 not found";
  the sketch too complex or the region unusable: "section 2 can't be
  used: ...") and made into a kernel profile (`profile`), one loop or
  "section 2 has holes: only sections with one loop can be lofted"; its
  start the first segment of the outline's piece whose start vertex is
  within the resolution of the sketch point (`varde_regen::loft_corner`,
  the rule the session's corners follow too, `loft_corners`: an arc's
  split or a spline's fitted joint is no corner; `profile_marked` gives
  each piece's first segment) ("section 2's start point wasn't found",
  "section 2's start point isn't one of its corners"); its frame its sketch's
  placement. A point section is the sketch point placed in the world
  ("section 1 not found" when gone). A section whose sketch isn't
  placed: "section 2's sketch isn't placed".
- **One plane**: two consecutive sections (and, closed, the last and
  the first) on one plane fail before the kernel, "sections 1 and 2 are
  on one plane" (a point on the plane of the section next to it:
  "section 3 is a point on section 2's plane: move it off the plane"),
  by the kernel's own rule (`varde_kernel::loft::on_one_plane`:
  each one's vertices within the resolution of the other's plane; a
  point on a loop's plane; two points never).
- **Rails**: each sketch chain ordered by `profile::chain` (as a
  split's line: lines, arcs of at most 90°, splines' fitted conics; the
  joints to the bit), mapped into the world by its sketch's placement
  as `Rail { conics }`; "rail 1 not found", "rail 1 is closed: a rail
  runs from the first section to the last", "rail 1's curves don't join
  end to end into one line", "rail 1 can't be used: ...", "rail 1's
  sketch isn't placed".
- **The loft**: `varde_kernel::loft::loft(sections, mode, closed, rails,
  feature, tol, budget)`, cached as an `Entry::Solid` by the feature,
  the fit tolerance, the mode, `closed`, and every section's and rail's
  sketch key, placement bits and what the feature names of it (an edit
  elsewhere finds it again; a section's sketch edited or moved makes it
  again). Its refusals (`LoftError`) are worded by
  `message::loft_refused`: "sections 2 and 3 are on one plane", "rail 1
  doesn't pass through section 2", "the loft twists: pick matching
  start points", "the loft runs into itself"; its failures by
  `message::lofting` ("lofting its sections is too complex to work
  out", else as an extrude's tool with the verb "loft"). Then the tool
  goes on as an extrude's: a new body, or touches and booleans (a join
  touching several bodies merges them).
- **Kernel stand-in**: `varde_kernel::loft::loft` (`kernel/src/loft.rs`)
  isn't built: it fails with `TooComplex`, so every loft that gets as
  far as the kernel fails today with "lofting its sections is too
  complex to work out", making no body, and the rest of the history
  goes on. The regen tests swap it (`loft::LOFTER`, a thread local;
  other crates' tests through the `testing` feature,
  `varde_regen::testing::loft_by_extrude`) for `by_extrude`: two open
  sections, no rails, each a loop of straight segments, the second the
  first moved along its normal vertex for vertex (either way round), is
  the first extruded to the second, faces named as an extrude's; a
  second start other than the first's moved is `LoftError::Twists`;
  anything else too complex. The planned analytic tests of the
  kernel's loft are written and `#[ignore = "kernel loft not built"]`
  (`kernel/src/loft/tests.rs`).
- The draft's reply carries nothing new; the wire carries the loft as
  any kind (postcard, validated by `Document::check`).

### UI

The move's session (`MotionSession`) with `MotionKind::Loft`; its own
parts are in `app/src/doc/motion/loft.rs` (`LoftSetup`) and
`view/src/motion/loft.rs` (`LoftView`, `LoftSection`, `LoftShape`). The
UI mock has no loft panel (the model mock has only the icon and "Loft"
in its Create group), so the panel is built in the style of its
nearest ones, the revolve's and the sweep's: Sections, "Between
sections" as tiles (Smooth, Ruled), Closed, Rails, Operation as tiles
and the Bodies list.

- **Starting**: `Look::StartLoft` from the rail's Create set (after
  Sweep, as the mock's group orders it; its list's letter `L`), through
  `loft_binding`, with no key, as the mock has it; again, or another
  tool, backs out. **Not on the toolbar**: the mock's model bar has no
  Loft. Editing one (`Look::EditFeature`: double-click, `Enter`, "Edit
  loft") opens it with its sections, rails and options, picking
  nothing. It picks no bodies; the model isn't picked while its
  sections or rails are (`MotionSession::picks_sketches`): the clicks go
  to the sketches, and off them the left button orbits.
- **Sections** (`MotionPick::Regions`): while they're picked, every
  visible sketch before the loft (and each section's own, shown or not:
  adding the loft hid them; `RegionPick::also`, with no source, kept
  even with no regions, worked out within a share of their own,
  `also_work()`, four times `MAX_WORK` in all, apart from the visible
  ones' `refresh_work()`: those past it are worked out on a later change,
  and meanwhile their sections aren't said to be gone) shows its regions on its
  plane, and every visible sketch's points on their own (no curve's) as
  dots. Under the cursor, in this order: a corner of a region section
  (a sketch point within the resolution of one of its outer loop's
  pieces' start vertices, by regeneration's own rule,
  `varde_regen::loft_corners`; drawn as a small dot), a point on its own, a region. A click on a region adds it as
  the last section (before a last point section), or takes out the
  section that is that region; a region with holes is refused ("A
  section is one loop: pick a region without holes"), as are one too
  thin to name, a sketch after the loft, and the 65th section. Its
  start is set as it's added: the corner nearest the previous section's
  start dot in the world, or the first corner for the first section (a
  region with no sketch point at a corner, a circle's, has none, and
  the kernel's default holds). A click on a point adds it as the last
  section, or the first if the last is a point already, or takes it
  out; "A loft takes a point only as its first or last section"
  otherwise. A click on a corner moves that section's start there
  (`MotionLook::LoftStart`). Where two sketch points are on one corner,
  the corner is the first of them; a start at the other (from a file,
  or an edit of the sketch) is drawn at that corner all the same
  (`shown_start`), as regeneration takes it.
- **Seam knobs**: each region section's start is a knob (the operation
  knobs' puck, in the count's teal, its arrow along the outer loop
  towards the next corner), picking or not, while the loft can be
  changed: it takes the mouse ahead of what the loft picks and the model
  (`viewport/motion.rs`, `SeamInput`, `seam_pucks`; a grab hand over
  it, the model's hover let go of); pressed and dragged, the section's
  start goes to its corner nearest the cursor on the screen, each corner
  reached sent once (`MotionLook::LoftStart`), its section's corners
  drawn meanwhile; a corner under the start is its knob's, not a
  click's. The camera moving under a still cursor works its hover out
  again.
- **Drawn**: each region section filled and outlined in the selected
  colour on its plane (the one whose row is hovered in the hovered
  colour), its seam knob, a point section's point as a dot in the
  accent, and each section's number in a chip by it (`Moving::labels`).
- **Rows**: "Section 1" with its sketch's name ("gone" once what it
  names is), an up and a down chevron (`ordered_row`; the first's up
  and the last's down faint) moving it past its neighbour
  (`MotionLook::SectionUp`), and a cross. Moving a point into the
  middle is allowed and refused as the foot ("Loft fails: its section
  2 is a point, which only the first or last may be").
- **Between sections**: Smooth (the default) or Ruled; two sections are
  ruled whatever it says (the status bar says Ruled). **Closed**: the
  last section lofted back to the first; while it's on, the Rails field
  isn't shown, its rails aren't stored and aren't picked (kept for when
  it's off: the document refuses rails on a closed loft); a curve
  clicked for a rail then is refused ("A closed loft takes no rails:
  turn Closed off to pick them") rather than added out of sight.
- **Rails** (`MotionPick::Path`): picked as a sweep's path's sketch
  parts: a click on a curve of a sketch before the loft (visible, or a
  section's or rail's own) adds the chain it's in (`Sketch::chain_of`,
  sorted), or takes out the rail holding it; at most 4 rails ("A loft
  takes at most 4 rails") of at most 256 curves. A chain that closes
  up (a circle, a closed spline, a loop of curves, as regen's `chain`
  finds it: `varde_regen::chain_closes`) is refused as it's clicked, in
  regen's words ("That line is closed: a rail runs from the first
  section to the last"). Rows "Sketch 3 · 2
  curves" (`PanelHover::Part`) with a cross. Unlike a sweep's path, a
  section's own sketch may hold a rail.
- **Operation and Bodies** as an extrude's (the cut's uncut note as the
  panel's warning, `Doc::held` for a new body a combine names).
- **Whole and ready**: two sections or more ("pick the sections to
  loft: regions or sketch points", "pick the next section").
  `Loft::check_own` refuses as the foot ("Loft fails"); `check_names`
  (start points, point sections, rail curves in their sketches) through
  `motion_held`. What an edit or undo takes away is kept and said to be
  gone ("A section's sketch, region, point or start is gone: take it
  out or move its start", "A rail's sketch or curve is gone: take it
  out"), nothing previewed or committed meanwhile; the section's row
  says "gone" (`MotionSession::section_gone`: its start too, gone or no
  longer at a corner).
- **Hover**: the corner, point, region or curve under a still cursor
  is worked out again as a frame is drawn (`Moving::redraw`: the camera
  moved), and let go of while the camera's dragged; a split picking in
  its sketches likewise.
- **Preview**: the loft as set up is the draft while it's whole,
  picking or not. **The kernel's loft isn't built**, so today every
  preview fails with "Loft fails" over "lofting its sections is too
  complex to work out", OK waits, and Add anyway keeps it, failing in
  the Timeline. The app's tests loft through regen's `testing` feature
  (`varde_regen::testing::loft_by_extrude`).
- **Committing**: OK (`Enter`, Add anyway) adds "Loft N" or sets the
  edited one, one undo step; Cancel or `Esc` leaves no trace. Adding
  hides the sections' sketches (`profile_sketches`), not the rails'.
- The status bar says "3 sections · Smooth · New body" once whole
  (`loft_info`), else what's next, with the hints "Pick sections:
  regions or points" or "Pick the rails' curves" (`chrome::step_hint`).

The Timeline shows a loft's row with the model mock's `loft` icon
(`Icon::Loft`, a slab and a disc joined by two rulings) and its note, "3
sections" (`view/src/loft.rs`, `loft_note`); the status bar says "3
sections · Smooth · Closed · 2 rails · New body" (`loft_info`).

Known gaps: no seam knob for a region with no start (a circle's);
the start dots follow sketch points only (a
corner where curves cross without a point can't be a start); a section
sketch hidden by the user is still offered (its regions are found to
draw the section).

### Tests

`document/src/loft/tests.rs` (adding, naming, hiding the sections'
sketches, undo; editing keeping or dropping the body; every own check;
what it names in other features and in its sketches, the latter only
on add; removal through any sketch it uses; units; postcard round trip
and refusals; its kind's index, held in `a_loft_is_the_seventeenth_kind`),
`regen/src/history/tests/loft.rs` (the stub's failure with the history
going on; by the stand-in a box, a transition piece joined between two
blocks merging them and following one moved, a section's sketch edited
and the loft following, undo; sections handed over in order and placed,
starts; regen's refusals; one plane, closed too; rails placed and
refused; the kernel's refusals worded; twisted starts; the cache; a
point on its neighbour's plane), `regen/src/history/tests/loft/fuzz.rs`
(random histories with lofts by the stand-in: warm and cold cache alike,
failures changing nothing, those that work an extrude's; bytes and the
wire; `VARDE_TESTS=full`, one replayed with `VARDE_TEST_SEED`),
`app/src/doc/motion/tests/loft.rs` (the session; deferred section
sketches through undo and redo) and its `fuzz.rs` (random sessions:
limits, a start not gone always drawn, ready only when the document
takes it and regeneration finds every section and start, commits as
drafted, edits reopening as stored; `VARDE_TESTS=full`),
`view/src/viewport/motion/tests.rs` (the seam knob dragged round its corners, none read-only; rails', regions' and corners'
hover worked out again as the camera moves),
`regen/src/wire/tests.rs` (a loft and its draft round trip),
`io/src/vrdp/tests.rs` (round trip, tampered and damaged records, a
loft's parts refused as read), `view/src/loft/tests.rs` (the notes).

## Operation knobs

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
sweep's twist or turns). Dragged, the viewport sends `MotionLook::DragKnob { knob,
value }`, the value snapped; the app (`Doc::drag_knob`) types it into
the field as the design's units (or degrees, or a bare factor) write
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
