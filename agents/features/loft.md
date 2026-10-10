# Loft

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

## Regeneration

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
  out", "lofting isn't supported yet" for `NotImplemented`, else as an extrude's tool with the verb "loft"). Then the tool
  goes on as an extrude's: a new body, or touches and booleans (a join
  touching several bodies merges them).
- **Kernel stand-in**: `varde_kernel::loft::loft` (`kernel/src/loft.rs`)
  isn't built: it fails with `NotImplemented`, so every loft that gets as
  far as the kernel fails today with "lofting isn't supported yet",
  making no body, and the rest of the history
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

## UI

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
- **Drawn**: each region section filled and outlined in the profile
  colour (`SketchColors::profile`) on its plane (the one whose row is hovered in the hovered
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

## Tests

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
