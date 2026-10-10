# Chamfer

`crates/document/src/chamfer.rs`.

```rust
pub struct Chamfer {
    pub edges: Vec<EdgeRef>,       // ..=MAX_BLEND_EDGES (256), one body, EdgeRef::order, no repeats
    #[serde(default)] pub faces: Vec<FaceRef>,  // ..=256, the same body, FaceRef::order, no repeats
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
- **Faces**: a face named stands for the edges around it. An edge named
  that is also around a face named (one of its keys the face's, on the
  same body: `blend_excludes`) is **left out**, an exclusion; there's no
  list of exclusions, it follows from the two lists. With faces, edges
  may be none; a face's edges' first face is the face (the one across
  with `flip`). Older files have no `faces` and read as none.
- **Checks** (`CheckError::Chamfer(id, ChamferError)`):
  `Chamfer::check_own(design)` (cheap): its edges by
  `check_blend_edges_own(edges, faces)` (`Edges(BlendEdgesError)`: one
  edge or face at least and at most 256 of each, `Count`; each edge's
  own check, `Edge(EdgeError)`, each face's, `Face(PlaneError)`; each
  list in order without repeats, `Order`; one body, `Bodies`), distances
  (`Distance`) and the angle (`Angle`) by their asks.
  `Document::check_blend_edges(index, edges, faces)` (public, for the
  panel; a fillet's edges are checked by the
  same two, `crates/document/src/blend.rs`): the body there and made
  before (`Body`: depended on, as a combine's bodies), and every key's
  feature before it, or not there with an id below the next
  (`RefMaker`, as a sketch's face's).
- **Dependencies**: `FeatureKind::bodies()` is the edges' and faces'
  body (`blend_body`: the first edge's, else the first face's), so
  removing it or its maker removes the chamfer. The features that made
  its edges' faces are **not** followed: removing a join whose wall an
  edge ran along leaves the chamfer, which then fails ("its edge wasn't
  found"), to be edited (decided, as every face and edge reference).
- `SetUnits` pins its distances and angle by their asks.

## Regeneration

`crates/regen/src/history/chamfer.rs`, in history order:

- The body needs a solid of its own (`own_solids`). Its topology is the
  one drawing it keeps (`inspect::topology`), and each edge is found on
  it by `Topology::edge(faces, near)`; one not found fails the chamfer
  before the kernel: "its edge wasn't found", or with several "its edge
  2 of 3 wasn't found" (its place in the list); each face as a region
  by `Topology::face` ("its face 2 of 3 wasn't found").
- **Faces and exclusions**: an edge named whose chain runs along a
  face named is left out and never grown into; the other edges named
  come first, then each face's chains in index order, named by their
  regions' keys (as a grown chain), first face the face. Everything
  named left out leaves nothing to blend, which fails ("every edge it
  names is left out: there's nothing left"), the body as it was. Refusals about a face's edge say "an edge of its
  face 2" ("an edge in the tangent chain of an edge of its face 2"
  grown).
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
  "chamfering Body 1 isn't supported yet" for `NotImplemented`,
  `message::blending`), the evidence's faces on the body.
- **Kernel stand-in**: `varde_kernel::chamfer` (`kernel/src/blend.rs`,
  with `ChamferChain`, `ChamferCut`, `BlendError`) isn't built yet: it
  has its planned signature and fails with `NotImplemented`. So every
  chamfer that finds its edges fails today with "chamfering Body 1
  isn't supported yet", its body left whole; the rest of the history
  goes on. The regen tests swap it (`chamfer::CHAMFERER`, a thread local;
  other crates' tests through the `testing` feature,
  `varde_regen::testing::chamfer_by_wedges`) for `by_wedges`: each chain
  a straight, open, convex edge between two flat faces cut off by a
  triangular prism past its ends, one boolean each (right for a block's
  edges, too complex otherwise); and for recording and refusing
  stand-ins. The planned analytic tests of the kernel's chamfer are
  written out and `#[ignore = "kernel chamfer not built"]`.
- The draft's reply carries nothing new.

## UI

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
  chain. A new one takes the edges and faces selected in the model
  shown that a click would take (an edge selected around a face
  selected so left out) (as the mock's takes a hole's rims selected), the
  first one's body deciding. Nothing takes the focus: clicks pick
  edges.
- **Edges** (`MotionPick::Edges`, the viewport picking edges and faces):
  a click on an edge picks it, named as of the feature
  (`Naming::edge_ref`) on the body holding it there (`Merges::holder`),
  made before it; a click on an edge picked (found again on the model
  shown by its names) takes it out. Once one is picked, the others must
  be on its body: another body's edges don't light under the cursor and
  a click says "A chamfer's edges are all on one body: pick edges of
  Body 1"; a face picks a face (below). At most
  `MAX_BLEND_EDGES`. Kept in `EdgeRef::order`, so the list is sorted as
  stored. The Tangent chain tick only says what a click picks: on, a
  click on an edge picks every edge of its tangent chain
  (`PickIndex::tangent_chain`, the topology's rule regen grows chains
  by), each an edge of its own (`Ref::unit`, `Doc::refs_click`), or
  takes them all out once all are picked; off, the edge alone. Edges
  selected as the session starts are grown alike, never taking any out.
  A new blend stores `chains` false, its edges being the chains'; an
  edited one keeps the `chains` it had (`BlendSetup::stored`), which
  also lights each of its edges with its chain. Picked edges are lit
  as selected, the one under the cursor as hovered with what a click
  on it picks;
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
- **Faces**: from the selection the session starts with, or the
  edited feature's, and picked by clicks beside the edges (the
  viewport picking `Picks::EdgesAndFaces` while a blend's edges are
  picked; a click on a face picked takes it out, as an edge's does,
  through `Refs<FaceRef>`), all on the edges' body; a face's row's
  cross takes it out too (`MotionLook::DropFace`). The field's
  placeholder and the hint say "Click edges or faces", "Pick edges or
  faces". Faces are kept in
  `MotionSession::faces` (the face sessions' list), lit as selected
  while found on the model shown. An edge picked around a face of the
  list (`blend_excludes`) is left out: lit alone (no tangent chain) in
  the exclusions' purple, the Create tools' handles' colour
  (`sketching.handle`, an extrude's), drawn as the
  highlight's second colour (`ModelHighlight::second_excluded` turns
  the viewport's second colour purple), and its row's name purple
  (`theme::excluded_text`). A click on it takes it out of the list,
  which brings the edge back in.
- **Ctrl held** (`Held::BEFORE`, `Cmd` on macOS; `Doc::blend_before`,
  `Doc::hold_command` from `Message::CommandHeld`) while picking edges
  shows the model before the blend instead of its preview: a new one's
  the document as it is, an edited one's a move of nothing of its body
  (`MotionSession::unmoved`, as when it isn't whole), so the edges the
  preview blends away light and click again, named as of the feature: a
  picked face's edge to leave it out (purple), a picked edge to take it
  out. `refs_model_current` takes that model (no draft for a new one, a
  draft of the session's run for an edited one). Letting go shows the
  preview again. The status bar hints "Ctrl: Hide preview", as every
  operation's does. Ctrl means nothing else here: a click in a
  session ignores the add key, which only the selection outside
  sessions (and the overlap list's choice, passed on as a click) uses.
- **The rows**: the faces first, "Face 2" by place (`Icon::SeFace`),
  then "Edge 2" by the edge's place in the list (as
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
- **Whole and ready**: edges or faces, and the type's values ("pick the edges
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
  "Chamfer fails" over "chamfering Body 1 isn't supported yet",
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
info "2 edges · Equal · 1 mm · Tangent chain" (with faces "1 face ·
1 edge · 2 left out ...", `blend_count`), "1 × 2 mm" for two
distances.

Departures from the mock: the Tangent chain tick (above); edges named
by their place; no bands drawn over the model (the mock's preview draws
each edge's chamfer as a band on the faces; here the preview is the
model regenerated with the chamfer). Known gaps: the
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
stand-in's not-built failure in the panel, OK waiting and Add anyway
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
