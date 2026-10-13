# Combine

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
  view) wants 1 to 256 tools, sorted without repeats (`check_bodies`
  in `bodies.rs`: `Tools(BodiesError::Count(n) | Order)`) and the target not among them (`TargetIsTool`); then
  every body named must be made by a feature before the combine, or
  not there with an id below `next_id`, as a sketch's face
  (`Body(id)`): removing a body or its maker removes the combine with
  it (below) unless the delete prompt's Delete only keeps it, failing
  ("a body it names isn't there"). `SetFeature` making the maker stop
  making the body (an extrude turned from a new body into a join) is
  allowed too: the combine stays, failing likewise. An edit is never
  refused for what later features name. `MAX_FEATURE_BODIES` is
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

## Regeneration

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

## UI

**The session** (`app/src/doc/combine.rs`, `Doc::mode` holding a
`CombineSession`) is started by `Look::StartCombine` (`B`, the toolbar's
Combine button after Revolve, the rail's Modify set; again, or `Esc`,
cancels it) or by editing a combine (`Look::EditFeature`: a
double-click or `Enter` on its row, Edit combine in its menu), in a
document that can be changed, outside sketches, as the revolve's: it
drops another operation being set up as they drop it (starting or
editing an extrude, revolve, move, ...), entering a sketch drops it, and it
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
(`ListRow`: the Body icon, the name and a cross taking it out,
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
Union · tools kept" (`combine_info`).

**Merges replayed in the app**: `MeshFeed::merged_before(document,
until)` replays, in the document's order, the joins the model shown
found merging and the combines using their tools up that it didn't find
failing (`feed::merged_in`), by regen's rule (`note_merge`, a
combine as its target then its tools); a combine naming a body merged in
before it merges nothing, as regen fails it, even before the model shown
knows (a session's bodies and edges don't follow into it). With no
`until` it stops at the rollback marker (`Document::insert_at`), where
a new feature goes: a combine past it isn't in the model, so a body it
uses up is still its own there (the MCP client's `move_body` of one
moved its target before). The extrude's and revolve's
Bodies lists and the combine's picking use it, and the delete prompt's
warning replays combines alike. It's `merged_in(document, until, marks)`
over the model shown's `MeshFeed::shown_marks` (merges, touched lists,
failures); the MCP client's body names and keys go through it over the
committed model's instead (see `mcp.md`). `feed/tests.rs`'s
`merged_before_agrees_with_regen_on_random_histories` holds the two to
the same answer on random histories of bodies, joins, combines (some
naming used-up bodies, so failing) and edits of earlier combines, for
the whole history and stopped before each feature
(one seed by default, 4 with `VARDE_TESTS=full`).

**An extrude or revolve making a combined body** may stop making it
(made a join, say): the combine stays and fails ("a body it names isn't
there") until it's given another.

Tests: `app/src/doc/combine/tests.rs` (keys, viewport and Objects
picks, the rows' crosses through the panel, the draft, Objects' faint
rows, one undo step, `Esc` without a trace, editing, merged bodies
picked as their holder, the highlight, read-only, the other tools, a
body going, the extrude that stops making a combined body, the Timeline row, the delete prompt);
`view/src/combine/tests.rs` (the panel's texts and layout); shot
scenario 23.

Known gaps: a kept tool overlapping the target's union shows both
bodies' faces in the same place, which z-fight (as two overlapping
bodies always do); the panel doesn't warn before the preview answers.
