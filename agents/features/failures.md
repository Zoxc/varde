# Failures and where they are

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
