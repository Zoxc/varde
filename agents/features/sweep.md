# Sweep

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

## Regeneration

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
  kernel's rule (`varde_kernel_compat::sweep::JOINT_SINE`, the sine both
  use), which the kernel's sweep keeps as a guard.
- **A helix**: its axis resolved as a move's turn's
  (`motion::resolve_axis`: its messages, "its axis edge wasn't found"
  and the rest), reversed with `flip`, noted in
  `Evaluation::references` (so a draft's reply carries it, for drawing),
  made unit ("its helix's axis has no direction"); `pitch × turns`
  within `MAX_COORD` (both checked, so finite), else "its helix is too
  long".
- **The kernel**: `varde_kernel_compat::sweep::sweep(profile, frame, path,
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
  is too complex to work out", not built "sweeping along a path isn't
  supported yet").
- **Kernel stand-in**: the kernel's sweep (`kernel/src/sweep/path.rs`)
  isn't built yet: it fails with `NotImplemented`. So every sweep whose
  path regen builds fails today with "sweeping along a path isn't
  supported yet", making no body (its new
  body has no solid; a feature naming it fails as for any), and the rest
  of the history goes on. The regen tests swap it (`testing::Overrides::sweep`, per
  thread, the stand-in in `testing/stand_ins/sweep.rs`; other crates' tests through the `testing` feature,
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

## UI

The move's session (`MotionSession`, above) with `MotionKind::Sweep`;
its own parts are in `app/src/doc/motion/sweep.rs` (`SweepSetup`) and
`view/src/motion/sweep.rs` (`SweepView`, `SweepPath`). The UI mock has
no sweep panel (the icon mock has only the tool, Sweep in its Create
group), so the panel is built in the style of its nearest one, the
revolve's: Profile, "Along" as tiles (Path, Helix), the path's fields,
Operation as tiles and the Bodies list.

- **Starting**: `Look::StartMotion(MotionKind::Sweep)` from the rail's Create set (after
  Revolve, as the mock's group orders it; its list's letter `P`, the
  label's first free one, as the mock's) through `motion_binding`, with
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
  tick, lit, `MotionLook::DropEdge`; kept in the sweep's own
  `SweepSetup::path_edges`, a `PathEdges` whose `tangent` is the tick),
  stored as one `PathPart::Edges`
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
  preview fails with "Sweep fails" over "sweeping along a path
  isn't supported yet", OK waits, and Add anyway keeps it,
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
Follow path · Twist 90° · New body" (`view/src/feature_text/sweep.rs`).

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
part, not built in the panel, Add anyway as one undo step, the
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
