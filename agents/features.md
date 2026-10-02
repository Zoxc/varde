# Features

The features after sketches and extrudes: their document types, checks and
commands, how regeneration evaluates them, and their UI. Extrudes are
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
  `Revolve` 2): the variant index is what files store.

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
  and the sketch's key (which holds where the axis line is and the
  plane). Booleans and touches are keyed by tool and body keys as an
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

None yet: the Timeline shows a revolve with the extrude's icon, its turn
("Full turn", "90°", "90° symmetric", "90° + 45°") and "Edit revolve" in
its menu, which does nothing; the status bar shows the turn and the
operation.
