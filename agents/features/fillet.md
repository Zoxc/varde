# Fillet

`crates/document/src/fillet.rs`.

```rust
pub struct Fillet {
    pub edges: Vec<EdgeRef>,   // as a chamfer's: ..=MAX_BLEND_EDGES, one body, EdgeRef::order, no repeats
    #[serde(default)] pub faces: Vec<FaceRef>,  // as a chamfer's: each stands for its edges
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
  body, kept in `EdgeRef::order` without repeats, and its faces too
  (each standing for the edges around it, an edge named around one left
  out, as "Chamfer" says); with no first faces there's no Flip. The plan's struct, as built.
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

## Regeneration

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
  out" for `TooComplex`, "filleting Body 1 isn't supported yet" for
  `NotImplemented`, `message::blending`).
- **Kernel stand-in**: `varde_kernel::fillet` (`kernel/src/blend.rs`,
  with `FilletChain`) isn't built yet: it has its planned signature and
  fails with `NotImplemented`. So every fillet that finds its edges
  fails today with "filleting Body 1 isn't supported yet", its body
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

## UI

The move's session (`MotionSession`, above) with `MotionKind::Fillet`,
its edges picked by **the edge session** the chamfer's has (see
"Chamfer": `BlendSetup`, `Refs<EdgeRef>`, `BlendEdges`, its Edges field
and the Tangent chain tick; `MotionKind::blends()` is the chamfer's and
the fillet's). The fillet's own parts are in `app/src/doc/motion/fillet.rs`
and `view/src/motion/fillet.rs` (`FilletView`). The panel is the model
mock's fillet panel: Edges, Radius, Tangent chain.

- **Starting**: `Look::StartFillet` from `F` (`Shortcut::FILLET`,
  `motion_binding`, the mock's key; in sketches `F` is the Fillet
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
- **Edges**: as the chamfer's (Ctrl held showing the model before it
  too), in the fillet's words: a face clicked
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
- **Whole and ready**: edges or faces and a radius ("pick the edges to fillet"
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
  "Fillet fails" over "filleting Body 1 isn't supported yet", the
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
Known gaps: as the chamfer's (the rows' measures are
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
stand-in's not-built failure in the panel, OK waiting and Add anyway
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
