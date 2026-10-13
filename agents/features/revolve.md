# Revolve

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
  and Through all alike; `SideFields::stored_flip` in `doc/regions.rs`,
  which holds both sessions' fields and flip and drags their knobs,
  `drag_side`).
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
  (shared with extrudes, sweeps and lofts, `Document::check_uses`, as
  `RevolveError::Operation(OperationError)`). `Revolve::check_own`
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
  (`EdgeMaker`), as a sketch on a face's (`Document::refers_before`,
  `body_before`, shared): one that isn't there is allowed with an id
  below `next_id`, one no later body or feature can take. Whether the
  edge is there, straight and in the plane is regeneration's.
- `removal`: removing its sketch removes it and its body; it's in
  `drop_excluded` like an extrude. Its axis edge's body or faces' makers
  don't cascade: removing them leaves the revolve failing ("its axis
  edge's body is gone", "wasn't found"), to be edited.

## Regeneration

`crates/regen/src/history/revolve.rs` (the tool run itself in
`history/run.rs`). A revolve runs as an extrude does (`Run`,
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
- **The tool** is `varde_kernel_compat::revolve(profile, frame, sweep, feature
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

## UI

**The session** (`app/src/doc/revolve.rs`, `Doc::mode` holding a
`RevolveSession`) follows the extrude's (see "The extrude UI" in
`agents/kernel/extrude-ui.md`), sharing its parts (`app/src/doc/regions.rs`):
`RegionPick` (the source sketch, the candidates' profiles found within
`refresh_work()`, the regions picked and their references, found again
when the sketch changes, the edited feature's missing ones kept: listed
after those picked as "Missing region" rows (`RowTone::Failed`, the name in
the strong danger colour, as a failed feature's in the Timeline), each
with a cross
(`RegionPick::drop_missing`), and named by the feature until taken out,
so its preview fails and Add anyway keeps it; found again as the sketch
changes),
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
  `regen/src/naming.rs`, in the document's order: the joins the model
  shown found working and the combines using their tools up that it didn't find failing, as
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
"Pick regions" or "Pick the axis", "Ctrl: Hide preview", `Enter` OK
and `Esc` Cancel. Ctrl held (`Held::BEFORE`, `Cmd` on macOS;
`Doc::operation_before`) shows the model before the operation rather
than its preview, so the profile under it shows: no draft, a new one's
the document as it is, an edited one's rolled back to before it
(`Doc::request_model`). The draft is still kept, but not sent or
applied (`MeshFeed::hide_draft`): its answer, the error the panel says
and OK waiting on it, stays; a draft changed while hidden waits for OK
and Add anyway both till it shows again. Every operation does so (the extrude, the
combine, the move's sessions), but for a fillet or chamfer picking its
edges, which has its own (`Doc::blend_before`). Letting go previews it
again. The
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
and, as its note, how far it turns in all (`feature_text::revolve::turn_note`: "360°",
"270°", "120°" for two sides of 100° and 20°). Selected, the status bar
says its turn, operation and axis (`revolve_info`: "Full 360° · New
body · about Y axis", "One side 90° · Cut · about Line 3", "Symmetric
90° ...", "Two sides 100° + 20° ..."; the axis last, as the selection's
box clips what doesn't fit at 1280 px and the axis says least, and left
out while its sketch doesn't have it). Double-click, `Enter` or Edit revolve reopen it
(a double-click on a sketch's row, in the Timeline or Objects, enters
the sketch).
