# Pattern

`crates/document/src/pattern.rs`.

```rust
pub struct Pattern {
    pub bodies: Vec<BodyId>,                 // as a move's
    pub kind: PatternKind,
    #[serde(default)] pub copies: Copies,    // "Join to original"
}
pub enum PatternKind {
    Linear { along: AxisRef, count: Value, spacing: Value },   // count 2..=1024 whole; spacing a length, not 0
    Circular { about: AxisRef, count: Value, angle: Value },   // angle above 0, at most a turn
}
pub enum Copies { Joined /* default */, Separate(Vec<BodyId>) }  // the copy bodies, by copy then body
```

- **What it is**: a timeline step on bodies already made, as a move is:
  `FeatureKind::Pattern` (the seventh variant on the workers' postcard;
  `Linear` 0, `Circular` 1), "Pattern N", no body made, no regions.
  Body patterns only (decided). Each body becomes itself and `count − 1`
  copies of itself (the count includes the original), **kept in the
  body** by default (`Copies::Joined`, the panel's "Join to original"
  ticked; decided, and the user's later word made it an option): side
  by side where apart, united where they meet. Unticked
  (`Copies::Separate`) **each copy is a new body of its own** (below).
  The body keeps its id and its own faces their names; copy `k`
  (`1 ≤ k < count`) names its faces as `FaceName::copy(pattern id, k)`
  (instance `mix(parent instance, feature, k)`, so a pattern of a
  pattern's or a mirror's copies stays unique), joined or not, and
  later features name a copy's faces by those keys (on the copy's own
  body when it has one).
- **Copy bodies** (`Copies::Separate`, the user's request after the
  plan; a defaulted stored field, so files from before read as joined;
  no version bump). Ordinary document bodies ("Body N", visible, made
  by the pattern: `Body::created_by` is the pattern), listed in the
  pattern in a fixed layout: copy `k` of its `i`th body (in its sorted
  bodies) at `(k − 1) · n + i`, `n` the body count
  (`Pattern::copy_body`, `copy_bodies`), so a higher count adds bodies
  at the end. **Identity**: `Command::AddFeature` and `SetFeature` lay
  the list out whatever it held (as an extrude's `BodyId::NEW`,
  `command/planning.rs`'s `planned_copies`): each copy, by its original's id and
  its `k`, keeps the body the feature it replaces made of it (a spacing
  or axis edited, a count raised, a body added or taken out, the kind
  swapped between linear and circular: the copies left keep their
  bodies), the others get new ids and names numbered on from the
  bodies; bodies of copies it no longer makes (a lower count, a body
  taken out, ticked again, set to another kind) are removed and dropped
  from excluded lists; a later feature naming one keeps naming it, and
  fails (as with an extrude stopping making a body a combine names;
  `Document::copies_dropped` says which). Ticked again
  and unticked, the copies get new bodies. At most `MAX_PATTERN_BODIES`
  = 1024 copy bodies per pattern (bodies × (count − 1), a checked
  product: each is an Objects row; joined, only the count is bounded).
  Separate bodies **may overlap** each other and their originals: they
  are never united (the panel warns, below).
- **Placement**: linear copy `k` is moved `k · spacing` along the
  axis's direction (only the direction counts; a negative spacing runs
  the other way, which is how a panel's Flip is stored). Circular copy
  `k` is turned right-handed about the axis by `k · span / steps`
  degrees: **a whole turn shares its ends** (angle 360°, or any that
  comes to a turn within the rounding of degrees, `Pattern::full_turn`:
  span 360, steps = count, so 4 copies are 90° apart, exactly), and **a
  span short of a turn has a copy at each end** (span = angle, steps =
  count − 1: 3 over 90° are at 0°, 45° and 90°). This was open in the
  plan (its kernel helper spread the count over the span either way);
  the mock's Circular pattern has Full 360°, Spacing (one step) and
  Total (the whole arc, a copy at each end) modes, which this stores
  as: Full → 360, Total → the angle, Spacing → spacing × (count − 1)
  (the mock refuses that reaching a turn). `Pattern::span_steps` /
  `step_degrees` give it. Each copy's motion is made directly
  (`Motion::pattern_step`, `Motion::pattern_turn(point, axis, span, k,
  steps)`), never by composing steps.
- **Axes**: `AxisRef` as a move's turn has it (origin axes, straight or
  round model edges, round faces, found on the bodies as the features
  before it leave them).
- **Checks** (`CheckError::Pattern(id, PatternError)`: its own
  `Count`, `Spacing`, `Separate`, `CopyBodies`, and a move's
  `Motion(MotionError)` for the rest):
  `Pattern::check_own(design)`: bodies as a move's (`Bodies(BodiesError)`), the count by `Pattern::count_ask` (`Ask::number(…,
  1024).whole().at_least(2)`: `Count`), a linear spacing by
  `Pattern::spacing_ask` (a length within `MAX_COORD` of zero) and not
  zero (`Spacing`), a circular angle by `Pattern::angle_ask` (above zero,
  at most a turn: `Angle`), unjoined at most `MAX_PATTERN_BODIES` copy
  bodies (`Separate(n)`), the axis's own parts (`Ref(RefError)`).
  `Document::check` then wants the bodies made before it (`Body`) and
  the axis's body and makers before it (`RefBody`, `RefMaker`), as a
  move's, and its copy bodies as laid out (`CopyBodies`): joined, no
  body made by it; unjoined, one listed per copy, none repeated, each a
  body whose `created_by` is the pattern, and every body made by it
  listed (bodies made by a pattern are counted in the body loop, so the
  check stays linear). `Pattern::count()` reads a checked count as a
  `u32`.
- **Removal and units** as a move's: `FeatureKind::bodies()` lists the
  bodies; `SetUnits` pins the spacing by its ask (the count and angle
  have no length unit to pin, so stay as typed). Copy bodies are the
  pattern's (`created_by`), so **removing the pattern removes them**,
  and every later feature naming one (`Document::removal`); removing
  one copy body removes its maker, the pattern, with all its copy
  bodies, as removing an extrude's body removes the extrude. The app
  asks first there (`Doc::remove_now`): a body goes quietly only where
  nothing but its own feature goes with it, and a copy body takes the
  pattern's other copy bodies, which the user didn't pick; deleting the
  pattern itself takes its own bodies without asking.

## Regeneration

`crates/regen/src/history/pattern.rs`, in history order:

- Bodies with no solid of their own fail it (`own_solids`); the axis is
  resolved as a move's (`resolve_axis`, cached) and noted in
  `Evaluation::references` for the draft (the point and direction it
  placed by).
- **Bounded before anything is copied**, per body: `count × patches`
  by a checked multiply (`copies_fit`) within `MAX_PATCHES` ("1024
  copies of Body 1 are too many to work out: it has N patches, and a
  body may have 4194304 in all; use fewer copies"), and every copy's
  box within `MAX_COORD` (each copy is checked: a circular pattern's
  farthest copy isn't its last) ("patterning Body 1 takes it out of
  range ...").
- **Copies**: `Solid::transformed(motion_k, Some(Instance { feature:
  pattern id, index: k }))` each, then `assemble([body, copy 1, ...])`
  (side by side where boxes and hulls are apart, balanced unions where
  they meet). The whole is cached under `pattern_key(body key, pattern
  id, every copy's Motion::bits, fit)`: an edit leaving the motions as
  they were (a spacing typed another way) finds it again. Kernel
  failures: "patterning Body 1 ..." (`message::moving`), "joining Body 1
  to its copies ..." (`message::with_copies`), with the kernel's
  evidence by value, as a move's. All bodies are worked out before any
  changes.
- **Unjoined**, the same bounds hold (`count × patches` within
  `MAX_PATCHES`, which also bounds the copies' memory together), the
  bodies are left as they are, and each copy is
  `Solid::transformed(motion_k, Some(Instance { feature, index: k }))`
  alone, the solid of its body (`Pattern::copy_body`), never assembled
  or united with anything; cached by `copy_key(body key, motion_k
  bits, the instance, fit)`, so a count raised finds the copies made
  before (a key of its own: `moved_key` with a copy files the solid
  assembled with its image, a mirror's). The copy bodies go into
  `Evaluation::bodies` after the others, in id order (the order made:
  a later join touching copies merges them into the first made, the
  original if it touches it too). A failing pattern leaves its copy
  bodies with no solid: a later feature naming one fails as on any
  body with none.
- Nothing merged or touched, as a move. Later joins, cuts, combines,
  moves, mirrors and patterns take copy bodies as any body (the app's
  `merged_before` follows them merged, from the touched lists and
  combines, unchanged).
- **Naming** (`varde_view::Naming`, `naming.rs`): a face made on
  body `B` by its maker and copied by an unjoined pattern is on that
  pattern's copy body, not on `B`. `Naming::before` keeps, per unjoined
  pattern before the stop (`Separated`), the body each body whose faces
  it copies holds them in (its sources, and the bodies merged into them
  before it, by `merged_before` as replayed so far) and its copy
  bodies; `instances_before` keeps how each copy instance was made
  (parent instance, feature, index: `Copied`). `Naming::body_of` walks
  a face key's instance back to the original and on through each
  unjoined pattern it passed, so a face of a copy of a copy is named on
  the last copy body, even where a later join merged that into the
  original. `made` (the body a feature made, by its number) lists only
  extrudes' and revolves' new bodies. Past `MAX_INSTANCES` copies the
  body of a copy can't be told: with any unjoined pattern before the
  stop such a face is refused as unclear.

Tests: `document/src/pattern/tests.rs` (checks, spacing rules, removal (the
axis's body removed leaves the pattern),
units, postcard and hostile values read back), `document/src/pattern/
tests/separate.rs` (copy bodies laid out and named, kept by copy across
spacing, count, body and join edits and undo, a later combine naming one
refusing the edits that drop it, removal both ways, the limit, wrong
lists refused from postcard; renamed, hidden and see-through copy
bodies kept across edits, units and a kind swap), `regen/src/history/tests/pattern/
separate.rs` (pins patterned unjoined, overlapping, each copy a pin of
its own with its wall named as its copy; the tick toggled and undone;
a combine cutting a copy body from a plate, a ring about a copy's wall,
a join merging copies into the original; the cache across a count
raised, for two bodies too; unjoined rings whole turn and part of one
by each copy's volume and centre), `regen/src/history/tests/pattern.rs` (a row of pins against its volume and box, names per copy, a
negative spacing; copies end to end along the pin's own round face
united into one; a ring of pins, whole turn exact at quarter turns and
three over 90° by its centre of mass; three bars through a hub against
the inclusion–exclusion area; a pin grid cut from a plate in one
difference, 4 × 4 and 10 × 10, timed; a copy's face as a later axis,
kept by its name as the count changes, and a copy that isn't there; a
pattern of a pattern 64 by 64, an edge of the last copy of the last
copy found; discs touching along a line refused, a hair closer or
farther right; copies on the original to the bit; a ring about a disc's
wall near the coordinate limit; out of range; the patch bound with
overflow; the cache), `regen/src/wire/tests.rs` (a draft and its axis;
a draft's list of copy bodies laid out again whatever it held, its
length tampered refused), `io/src/vrdp/tests.rs` (through a file;
tampered records refused, a copy list's length, ids and the count
among them), and
the motion fuzz (`motion/fuzz.rs`, `VARDE_TESTS=full`): random linear
and circular patterns of 2 to 4 copies about origin axes, edges and
round faces, half of them of bodies patterned already (copies of copies),
each copy's centre where `glam` places it and the whole as the copies
united one by one; a third of them unjoined, the originals left alone
and each copy body the copy itself where `glam` places it, later
features (moves, mirrors, patterns, combines, joins, faces as axes)
then taking copy bodies like any, and edits dropping a copy body a
later feature names refused; earlier patterns changed one way (count,
tick, kind swapped, a body added or taken out), each copy keeping its
body, name and visibility, new ones named apart; copy bodies hidden
or removed with their pattern.

Known gaps: the 10 × 10 grid of pins (100 holes) cut from a plate in
**one** difference runs out of work (`MAX_WORK`) in about 2 s (release);
the difference's cost grows faster than the pins (a 7 × 7 grid takes 2
to 3 million of the 4.2 million units, from 8 × 8 on it fails), so hole
patterns past about 50 holes fail with "too complex" until the boolean's
work is local to the change. A combine step running out of work with a
tool of more than one piece says so rather than blaming tangent faces
(`message::combining`, the pieces counted by `combine::shells` only
then): "cutting Body 2 from Body 1 is too complex to work out at once:
Body 2 is 64 separate pieces; use fewer, or split them over more than
one combine". Cutting the 100 pins one at a time works
but takes about 40 s. Copies that only touch (discs a diameter apart)
fail to join ("leaves no clean solid"), as any union touching along a
line does: the kernel's `NotManifold`, right, since the result would
touch itself along the line. Copies a hair apart (a spacing of 1e-9,
a turn of 1e-7° about a far axis) are refused as nearly flush or, the
booleans having improved, united into the right solid (a pin 1e-9
apart from its copy now unites), and
from about 1e-4 to 1e-3 mm apart a union of them can take seconds to
run out of work. `Naming` takes faces of patterns' copies made before the
feature (`instances_before` replays patterns as it does mirrors), up to
4096 copies in all (a pattern of a pattern 64 by 64; a mirrored row of
1024 mirrored again); past that it takes a face of any copy, and one
that isn't there yet at the feature fails it when regenerated ("its axis
face wasn't found") rather than being refused as it's picked.

## UI

The move's session (`MotionSession`, above) with `MotionKind::
LinearPattern` and `CircularPattern`, following the UI mock's
`lpattern` and `cpattern`.

- **Starting**: `Look::StartMotion(MotionKind::LinearPattern)` (`P`, `Shortcut::PATTERN`, the
  mock's key, outside sketches where `P` is the Point tool's; the
  toolbar's "Pattern" after Move; the rail's Transform set as "Linear
  pattern") and `Look::StartMotion(MotionKind::CircularPattern)` (no key, as the mock has
  none: the rail's "Circular pattern"; not on the toolbar, which the
  mock's model bar leaves it off too and which is nearly full at
  1280 px), again backing out, the bindings as Move's
  (`motion_binding`); or editing one
  (`Look::EditFeature`: double-click, `Enter`, "Edit pattern"). Bodies
  as a move's (the selection, else the only body; picked in the
  viewport or Objects; merges followed; gone bodies said "A picked body
  is gone"). A new linear one starts as the mock's: the X axis, count
  3, Spacing 100 (of the design's units); a circular one the Z axis,
  count 4, Full 360° (its angle field holding 90° for the other modes).
  The count field (`VALUE_FIELD`) takes the focus.
- **The axis** (linear: "Direction") is picked as a move's
  (`MotionPick::Reference`: the toolbar's origin axes, a straight or
  round edge or a round face named by `Naming`, the neutral preview of
  an edited one while it's picked; a linear one refuses others with
  "Only a straight or round edge, or a round face, can give the
  direction"), and drawn as a move's axis, always (a flipped linear
  one's arrow pointing the way its copies go). Gone: "The direction is
  gone: pick another" / "The axis is gone: pick another" (an undo took
  its body or maker away, or the body was removed: the pattern stays
  and fails until another is picked); while another is picked for an
  edited one, the neutral preview leaves the gone axis (and any gone
  body) out, so the model shown is still the bodies at the pattern.
- **Fields and modes** (`MotionField::Count`, `MotionField::Spread`,
  `MotionLook::Mode(PatternMode)`, `MotionLook::Flip`): the count by
  `Pattern::count_ask`; the spread a positive length within the
  coordinate limit (linear: `spacing_ask().positive()`, the sign is
  Flip's) or an angle by `Pattern::angle_ask`. Stored
  (`MotionSession::pattern`, `app/src/doc/motion/pattern.rs`): linear Spacing as typed, Total as
  `"(total) / (n − 1)"`, Flip as `"-(...)"` round either (a negative
  spacing); circular Full 360° as `"360°"`, Total as typed, Spacing as
  `"(spacing) * (n − 1)"`. Texts, not rounded numbers, so the values are
  exact and follow unit changes; where the text wrapped would be past
  what an expression holds (`MAX_LEN`, `MAX_DEPTH`: a spread typed near
  the 256 bytes), the value it comes to is stored instead, written
  exactly (`varde_expr::exact`), rather than refusing a text that reads. The mock's errors under the field
  (`MotionState::spread_error`, OK off): "The pattern runs past 1000000
  mm" (spacing × (n − 1) past `MAX_COORD`; the mock's limit is its
  10 000 mm), "4 copies 120° apart go past a full turn" (a Spacing
  coming to a turn or more, within the rounding `full_turn` allows),
  "A whole turn puts the last copy on the first: use Full 360°" (a
  Total of a turn).
- **Mode kept**: the stored values can't tell a Total from a Spacing
  (or a circular Spacing from a Total), so `Doc::pattern_shapes` keeps,
  per pattern committed in this run, the mode, Flip and spread as typed
  (`PatternShape`); editing takes them again only if they still give
  the values stored (an undo, another edit or another document leave
  them out). Otherwise the rule: a linear pattern opens in Spacing with
  its spacing's size (Flip on for a negative one, its text with the
  sign taken off where that gives the value exactly, else
  `"-(text)"`, else, too long for that, the size written exactly); a
  circular one in Full 360° for a whole turn, else in Total with its
  angle. The shape is kept under the feature edited, or the one added;
  one whose mode the pattern's kind doesn't offer (a file swapped the
  kind) isn't taken. While the mode, Flip and spread field are as an
  edited pattern opened with them, the pattern keeps the spacing or
  angle it stores, text and all (`Opened`): OK with nothing changed
  writes nothing even where the session would write the same value
  another way ("-15" read with Flip on comes back "-(15)", "360" in
  Full 360° as "360°").
  An undo setting another kind of feature in the edited one's place
  (`MotionKind::of` no longer the session's) ends the session.
- **Preview, OK, status**: the draft is the pattern as set up (none for
  a new one while its axis is picked); OK one undo step ("Pattern N");
  the status bar says "New linear pattern · Body 2 · 4 × 12 mm along X
  axis · joined" (", flipped"; " · joined" while ticked), "· 4 × 90°
  about Z axis" (`varde_view::pattern_copies`, by
  `Pattern::step_degrees`), or "pick
  the bodies to pattern", with "Pick the direction" / "Pick the axis".
- **The panel** (`view/src/motion.rs`): title "New linear pattern" /
  "New circular pattern" with the mock's `lpattern` and `cpattern`
  icons (`Icon::LPattern`, `CPattern`); Bodies; Direction (linear) or
  Axis; Flip direction (`Icon::TkFlip`, linear only); "Copies": Count,
  the mode tiles (`lp-spacing`, `lp-total`; `cp-full`, `cp-spacing`,
  `cp-total`: `Icon::LpSpacing`, `LpTotal`, `CpFull`, `CpSpacing`,
  `CpTotal`), the Spacing or Total field (none for Full 360°), and Join
  to original.
- **Join to original** (`MotionLook::Join`, `MotionSession::join`):
  the mock's tick on both panels, last, with its icon (`Icon::TkJoin`,
  the mock's `tk-join`) and note "One body; otherwise each copy is its
  own". **Ticked to begin with** (the user's decision; the mock starts
  unticked); an edited pattern opens as it's stored. Unticked, the
  pattern goes out as `Copies::Separate` of no bodies, which the
  document lays out (keeping an edited one's copy bodies). An edit
  dropping a copy body a later feature names (fewer copies, a body
  taken out, ticked again) is allowed: that feature stays and fails.
  The preview never shows that: editing rolls the model back to just
  after the feature edited (`Doc::rollback`). **The overlap warning** (the mock's, linear only, as the
  mock has it, `Doc::motion_warning`): unticked, with a spacing shorter
  than one of the bodies is long along the direction, so its own copies
  overlap ("The copies overlap (10 mm long this way): tick Join to
  original to merge them"; each body on its own, so two bodies far
  apart whose copies miss aren't taken as one long one), the length
  from their faces in the model shown (`PickIndex::bodies_extent`, the
  mesh's points; the direction an origin axis's or, for an edge or
  face, the draft's reference). It doesn't stop OK (separate bodies may
  overlap) and shows only where the panel has no other message (the
  mock shows it under a failure too).
- **Timeline**: the mock's icons by kind, the note "×4"; selected, the
  status bar says "Body 1 · 4 × 25 mm along X axis, flipped · joined",
  "Body 1 · 6 × 60° about Z axis" (`pattern_info`, the mock's " ·
  joined" for joined copies; the session's status text too). Copy
  bodies show in Objects as any body.

Tests: `app/src/doc/motion/tests/pattern.rs` (`P`, typing a count and a
spacing previews the row, `Enter` one undo step, the note; Total stores
its spacing, Flip turns it, the mode kept on editing and the rule
without it; a circular Full 360° against the copies' box; the mock's
errors past a turn and the limit; editing from the Timeline and undo;
spreads too long to wrap stored as their values, and a stored spacing
whose text turned is too long; an undo swapping the kind; a direction
gone by an undo and by its body's removal, the neutral preview without
it; the axis's body and a picked body taken mid-pick; OK on a pattern
opened writing nothing however it was typed), `pattern/separate.rs`
(the tick ticked to begin with, unticked a body per copy committed,
listed and undone, the overlap warning and when it goes; editing the
tick, and a later combine's copy body holding back fewer copies or
joining, and sending no draft while the direction is picked; too many
copy bodies refused; the warning for two bodies apart, in inches,
flipped and along the plate's edges; deleting a copy body asking
first), `pattern/fuzz.rs`
(`VARDE_TESTS=full`: random modes, Flip, counts, long and nested
spreads, units, undo and redo, kind swaps, commits, the direction
picked, copy bodies picked, named by later combines and mirrors,
hidden and deleted; edits held to the values their fields come to, a
held edit neither ready nor previewed);
`view/src/motion/tests.rs` (both panels' rows and modes, the infos).

Departures from the mock: "Join to original" starts ticked (the mock's
starts unticked). Kept: the user decided the joined copies are the
default and what older files read as, so a new pattern starts as the
stored default and as patterns did before the tick, and the mock's
unticked start (a new body per copy, each an Objects row) would make
the heavier, rarer choice the one taken by pressing Enter. The overlap
warning shows only where the panel has no other message (the mock
shows it under a failure too). Kept: the panel's footer holds one
message, a failure or refusal is the one to act on, and the warning
comes back once it's gone (it never blocks OK, and a failing draft is
added only through Add anyway). Counts up to 1024 (the mock's 100)
and the run limit the coordinate limit (the mock's 10 000 mm), with the
fields' own error words for other refusals; the axis may also be a
round edge or face, as a move's; no faded originals (the preview
replaces the model); Circular pattern isn't on the toolbar (the rail
has it; the mock's body bar does, the app has no such bar).
