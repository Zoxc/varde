# Align

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
  "Aligning" in `agents/kernel/transform.md`), so the body follows either side's
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

## Regeneration

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

## UI

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
  `shortcut::motion_binding`, `Shortcut::NONE`, the rail's list letter
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
`agents/kernel/patch.md`) so it's the drawn centre within `1e-4` mm for a
10 mm arc a 1e5 away (it was 0.01 off).
