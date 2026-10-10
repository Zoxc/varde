# Scale

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
  `agents/kernel/transform.md`). Positive
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

## Regeneration

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

## UI

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
