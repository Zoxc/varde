# Editing

Entering a sketch shows the Sketch tab, and leaving it shows the tab
shown before (`Doc::panel_before_sketch`), unless Objects was picked in
the sketch, which stays.

The sketch being edited is `Doc::mode`'s `Sketch`, a `SketchSession`
(`crates/app/src/doc/sketch.rs`): the feature, the tool in use (`Drawing`)
or the Constrain tool, the selection (a `BTreeSet<Selectable>` shared by
the viewport and the lists, see below), the item hovered in a list or by its glyph, a
drag in progress, the sketch with the edits waiting on the solver applied,
the analyses, the last refusal, whether glyphs show, the lists' scroll
offsets, the value field and a dimension's label grabbed. `Doc::prune`,
run by `Doc::sync` after every change, keeps it to what the sketch as
worked on holds: ids gone drop out of the selection and the hover, the
value field, a label grabbed and the Dimension tool's picks go with what
they're on, so do a shape tool's, and Offset's chain once it's no chain,
Fillet's or Chamfer's corner once its lines no longer make it
(`shape::still_picked`), a drag started from another generation of the document is put
back, and a Line chain whose last point is gone (undone) starts afresh.
Across a replacement of the whole document (restoring recovered changes,
or undoing or redoing that: `Editor::lineage` changes from the one `Doc`
saw at its last sync, which then tells every prune) ids may name other
items, so the selection, the hover and the last refusal (what the solver
ran into) are cleared and the tool starts its shape afresh;
restoring drops the edits waiting on the solver, and the changes waiting
behind them. The sketch session only resets, since it reads the sketch its id names now and never
writes back what it read before; an extrude session holds values read
before the replacement, so it ends instead (`Doc::prune_extrude`). The
feature selected in the Timeline is let go of too, lest Delete or
Extrude act on another feature with its id, unless it's the sketch being
edited and its id still names a sketch, which stays selected with the
session; an id naming something else now ends the session. A
document that can't be edited, as a Save As can leave it, has no tool and no drag;
the Save As answer syncs for that.

**A new sketch** (`S`, the toolbar's or the rail's Sketch, enabled
outside a sketch in a document that can be changed, an operation being
set up dropped for it) first asks for its plane (`Look::PickPlane`, `Doc::mode`'s `PickingPlane` (`doc/plane.rs`) holding a
`PlanePick`): the toolbar offers XY, XZ and YZ (`Edit::PlanePicked`, keys `1`, `2`, `3`,
`shortcut::plane_binding`),
and the viewport picks flat
faces (only faces, `ModelPicking::planes`): a flat face hovered is
highlighted with the pointer cursor, a curved one isn't and the status
bar says "Only flat faces can be sketched on" (`CURVED_FACE`), and a
click on a flat face sends `Edit::FacePicked(FaceRef)` (its body, key
and the point clicked), a click elsewhere nothing; `Esc` backs out. With
a face alone selected in the model (`Selection::single_face`,
`DocumentKeys::face_selected`), `S` and the button, then labelled
"Sketch on face", send `Edit::SketchOnSelection` instead. Either way
`Doc::new_sketch` works the placement out from the model shown first
(see `agents/features/sketch-planes.md`): a curved face, one not in the model shown or
one too far out is refused with the reason in the status bar
(`Doc::notice`), and nothing is added; else one `AddSketch` with
`Plane::Face`, one undo step, and the sketch is entered, the camera
facing the face as the camera turns on entering any sketch (below).
A face refused (curved, not the document's: a model shown from before
an edit removed its feature, or too far out) keeps picking, the reason
in the status bar. A sketch is entered only once it's placed
(`Doc::enter_sketch`); the session keeps its placement
(`SketchSession::placement`), which moves with its face as answers come
(`Doc::follow_placement`) and stays as it was while none is known; an
undo or redo in the sketch that puts it on another plane turns the
camera to face it once its placement is known.

Entering a sketch, or its plane turning under it, turns the camera to
look straight at the plane changing the view least
(`Doc::turn_to_sketch`, `facing_turn`): from the side of the plane the
view is on (the placement's normal when looking along it), and with the
sketch axis (`±x`, `±y`) nearest the view's up put up on screen, in
quarter turns so the axes stay square. The camera keeps world Z up
unless it looks straight down or up, so that turn only counts for a
plane square to Z; a side plane shows world Z up either way. It frames
what's drawn of the sketch; an empty one keeps the view's zoom and
looks at its target brought onto the plane, so a new sketch stays where
the view was. Home in a sketch (`Doc::sketch_camera`)
resets instead: from the placement's normal, its `y` up.

**Change plane** (`Look::ChangePlane(feature)`): from a sketch's
Timeline row menu, or the Sketch tab's row naming the plane ("on XY",
"on Extrude 1's end") while it's edited, which leaves the sketch for it
(`PickingPlane::enter`, to come back). The same picking as for a new
sketch, with `PlanePick::change`: only faces of bodies made by features
before the sketch and named by such a feature (the rule
`Document::check` holds a face plane to, which a face of a later body
would break), so `PlanePick::refusal` says "Sketch 1 can only go on a
face made before it" for the others, which aren't highlighted nor
clickable; the toolbar's tag reads "Sketch 2's plane". The plane
picked is one `SetSketchPlane` (`Change::SetPlane`, waiting behind
edits on the solver like any change; a face as `PlanePick::face_ref`
names it, by the body it was made on if a later join merged that into
another, see `agents/features/sketch-planes.md`), the drawing kept in its own
coordinates; a face's placement is worked out at the pick and kept as
`Doc::placed` (with the plane, so an undo that takes it off drops it) as
for a new sketch. A face refused at the pick keeps picking, the reason
in the status bar. `Esc` (or `S`) backs out leaving the document as it
was, and enters the sketch again if it was left for it. The picking is
kept in step with the document (`Doc::prune_plane_pick`): which faces
take the sketch is worked out again after every edit, and it ends if
the sketch goes, the document is replaced or can't be edited any more
(backed out of, as `Esc`); after every answer too,
which may merge bodies, and the reason a failed sketch is asked about
goes once the model shown places it.

**A sketch whose face is gone** (regen failed to place it: its face
wasn't found, isn't flat, its body is gone) is red in the Timeline with
regen's reason as its tip. Entering it (`Doc::enter_sketch`) in a
document that can be edited starts Change plane for it instead, the
status bar saying "Sketch 2: its face wasn't found. Pick a plane for
it"; the plane picked puts it there and enters it, `Esc` leaves it as
it was. Read-only, or before the first answer places it, it isn't
entered and the status bar says why (`Doc::notice`).

**The Timeline note** of a sketch names its plane
(`varde_view::plane_note`): "XY", or the face by the feature that made
it and the part of it ("on Extrude 1's end", "start", "side"; "on a
face of Revolve 1" for other parts), by its body if that feature is
gone ("on Body 1"), else "on a face". The status bar names neither the
sketch nor its plane: it says the sketch's standing and profile count,
both hidden while anything is selected, then notes and refusals.

**The selection's box** in the status bar shows in a sketch while no
tool is in use (nor Constrain, nor the value field): one item by name
("Circle 1") or how many, and what they measure together as a dimension
of them would (`dimension::selected`, the Dimension tool's `measure`
with its label nowhere: "Length 40 mm", "Diameter 6 mm", "Angle 30°"),
or for four lines closing a loop with square corners (their points
selected too or not) "Rectangle" and its width, the side nearer the
horizontal, and height (`dimension::rectangle`), with `Space` Clear,
which the sketch's hints then leave out.

Every change to the sketch is a `SketchEdit`, proposed to the solver lane
(`Doc::propose`, `doc/sketch/propose.rs`) and committed once it's
accepted as the whole new sketch, solved, one `Command::SetSketch`, one
undo step. One that can't even be applied to the sketch as it's worked on,
such as one past the coordinate limit, is refused at once and the status
bar says why (`EditError::Sketch`).

- **Proposals** (`Proposals`, in `Doc`): one is with the lane at a time,
  `Propose { base, sketch, edit }` on the sketch committed now, and the
  rest queue behind it in order. While any wait, the session's `Waiting`
  is the committed sketch with them applied (unsolved): what's shown, with
  the items they add faded once they've waited `feed::SLOW` (250 ms, so
  quick answers don't flicker them), and what the tools draw on, so a chain's next
  line goes on from a point still waiting. An `Add` made on that names the
  waiting items by their ids to be, which apply only once those are
  accepted (`Add::apply` refuses an id past the sketch's own). `Accepted`
  commits and keeps the analysis it carries for the new revision;
  `Rejected` changes nothing and keeps the refusal in the session
  (`Refusal`): the status bar says why ("Would over-constrain the
  sketch") and the constraints involved, with what they tie together, are
  red, until the next action (an edit, a click, a tool, `Esc`). A lane
  that fails on one (`Failed`) says so the same way. One refused after
  its sketch was left (`Esc` before the answer) has no status bar to say
  so: `Doc::refused_edit` keeps the sketch and the refusal, and a warning
  banner over the viewport says "An edit of *Sketch 1* wasn't kept —
  *why*" (`RefusedEdit`, `refused_banner`; the same text as the status
  bar's, or "Couldn't check the edit: …") until Dismiss
  (`Edit::DismissRefusedEdit`), going back into that sketch, the sketch
  going, or the document being replaced whole. An answer for a
  revision that's no longer the document's (something else was committed
  meanwhile) is proposed again; nothing the user does commits meanwhile
  (below), so that's a safeguard. After 250 ms of waiting (`feed::SLOW`,
  told by frames while waiting) the status bar says "Checking…". Until
  the lane has started, proposals wait in the app.
- **Other changes wait behind them** (`Doc::change`, `Change`): while any
  proposal waits, every other change to the document (deleting a feature
  or body, confirming the delete prompt, toggling visibility, a new
  sketch, units, tolerance) queues behind it in `Proposals`, in order,
  and is made, on the document as it is then, once those before it are
  answered: toggling twice toggles back, and an edit of a sketch deleted
  before it is dropped. A delete asks then if more goes with it than
  was confirmed, and is asked in its turn: while its prompt is up
  nothing behind it moves (`Proposals::asking`, which counts as waiting,
  so Save, closing and quitting wait for the answer too, and
  "Checking…" doesn't show); Delete makes it at once and Cancel or `Esc`
  drops it, and the queue goes on, so two deletes that both ask are
  asked one after the other. An edit's values are read in the units
  shown when it was made, which new units waiting before it may have
  changed by the time it's proposed: it's proposed in its own units
  (`Proposal::units`) and its values pinned to them when committed, as
  setting the units pins the values already there. So what's committed,
  and so the undo history, keeps the order the user made it in. An MCP
  client's edit made meanwhile queues the same way
  (`Pending::Client`, made from its call on the design of its turn and
  applied through the client's apply path if its call still waits; see `agents/mcp.md`, "Edits"),
  and its `add_link` and `edit_sketch` are proposals of their own
  (`Proposal::client`, its call's tag; an `edit_sketch` in steps, each
  on the solution of the one before, committed together), checked and
  proposed in its turn, then shown waiting as the user's are
  (`Proposals::waiting`), so an edit the user makes in the sketch
  behind it builds on it rather than naming by the ids they were to get
  what its commit takes first. One the user made in the sketch while
  the client's was still `Pending::Client`, unseen, is refused at its
  turn when the committed sketch's `next_id` has gone past the one it
  was made on and a client edit was applied since (`Proposal::made_on`,
  `Proposals::client_edits`, `TAKEN_BY_CLIENT`, in the status bar or the
  banner): the ids it names may be the client's items now. Gone past
  with no client edit, it was a relink's amend (new ids for a link's new
  shape), which takes nothing the edit names: it's proposed on the
  sketch relinked. A client's sketch edit with the lane that times out
  leaves the user's edits behind it to go on: an `Add` is placed on the
  sketch without it, one naming the client's items is rejected, as one
  built on the user's own proposal dropped is.
  The client's are committed or refused through the client's apply path
  (`Doc::committing` builds the commit, `AddLink` or `SetSketch`, for
  the user's and the client's alike). Undo
  while something of the user's own waits (`Proposals::users_waiting`)
  takes back their newest waiting item
  (`Doc::drop_newest`, passing over the client's entries, which aren't
  theirs), the last one of theirs queued, else the question a delete
  from the queue asks (cancelling it), else the proposal with the lane
  if it's theirs,
  whose answer is ignored when it comes (`dropped` counts them: the lane
  answers proposals in order); those before it never depend on it. A
  solver lane started again in place of one that went gets the proposal
  and analysis the old one had, and no answers to dropped ones are
  awaited from it (`Doc::lane_replaced`). A document turned read-only
  makes none of what waits. Redo does nothing while any of the user's wait: what
  waits comes after what's undone, as a new edit would, and an `Add` names
  the items before it by id; what undo dropped from the queue isn't
  redone. The other rule considered, committing other changes at once and
  having undo drop proposals only while they're the newest change, lets
  an answer land on top of a later change (the history out of order) and
  a proposal committed then clear the redo of a change undone after it;
  queueing has neither. The cost: a change waits as long as the solver
  does, with "Checking…" showing. An extrude's OK isn't queued: it
  waits, see below. Only the user's own hold a change or an OK
  (`Doc::users_proposing`): an MCP client's edit waiting alone (queued,
  holding the queue for its model, or its proposal with the lane) isn't
  applied yet and is made at its turn on the design as it is then, so
  the user's change made at once comes before it in the undo history,
  as it came first; a client's proposal answered for an older revision
  is proposed again, or refused if its sketch went or changed under its
  steps.
- **Saving and closing wait for them**: Save and Save As asked for while
  proposals wait are kept (`Saves::waiting`, which counts as saving) and
  sent once they're answered or dropped (`Doc::proposals_settled`), so
  what's on screen is saved; leaving waits for them first
  (`Step::Proposing`), then asks about unsaved changes as usual, unless
  the user already chose to discard. Auto-save doesn't wait: it saves
  what's committed. An extrude's OK waits too (`Doc::extrude_ready`;
  likewise a revolve's, a combine's, a move's), for the user's own,
  since its regions are of the sketch the edits change: an `Enter` in
  the window is dropped, and the panel says "Checking the sketch…" after
  `feed::SLOW`.
- **Analysis**: on entering a sketch, and whenever its revision has none
  (undo, redo), the app asks `Analyse` of the committed sketch, one at a
  time (answers name only the revision, which two sketches share); an
  accepted proposal brings its own. The session keeps the last eight by
  revision (`Analyses`). It colours the sketch, gives the status bar
  "Fully constrained", "N degrees of freedom left", "Doesn't solve" or
  "Over-constrained", and marks conflicts red in the lists and viewport.
  An MCP client's `sketch` call asks an `Analyse` of its own (with a `query` id), any sketch,
  tagged by the call and queued apart, so it never touches the
  session's analyses (`agents/mcp.md`, "Waiting calls").
- **The toolbar's pill** names the sketch, its button leaving it: a
  check once the sketch differs from what it was when entered
  (`SketchSession::entered`, `SketchState::modified`; undoing back to it
  counts as unchanged), else an X, "Leave sketch, unchanged".
- **Tools** (`Tool` in `varde-view`, keys `L`, `B`, `C`, `A`, `G`, `N`,
  `P`, and `D` for the Dimension tool under Dimensions, the rail, or the
  toolbar, which shows the UI mock's sketch bar, `Tool::BAR`: Line,
  Rectangle, Circle, Arc, Trim, Offset and Dimension, then Constrain,
  each button's key in its tooltip, so it fits at 1280 px wide with a
  tool's tag beside the sketch's name; the other tools are on the rail
  only): the
  viewport sends a `ToolClick` per press, with the sketch point snapped (see Snapping
  below) and what it snapped to (`target`, `inference`), the point or
  curve under the cursor if any (what the Dimension tool picks), a pixel's
  size there, whether it's a double-click and whether the reference
  modifier is held. `Drawing::placed` keeps the points placed so far until
  the shape has all it needs, and `Drawing::targets` what each snapped to:
  - Line: each click after the first adds a line from the last point,
    sharing it (`Chain`); clicking the chain's first point once it has two
    lines or more closes the loop and ends the chain, as `Shift O`
    (`Shortcut::CLOSE`, `Edit::ToggleClosed`, `Doc::close_chain`, the
    chain's length in `ActiveTool::chained`) does; `Esc` or a
    double-click ends an open one.
  - Circle: centre, then a point on it.
  - Arc: start, end, then a point on it; stored counter-clockwise with its
    centre computed (`arc_through`).
  - Rectangle: two opposite corners, or with `Drawing::centered` (`Z`,
    `Look::ToggleCentered`, kept while the tool is, shown in the toolbar's
    tag) the centre and a corner. Four lines sharing four corners, with
    `Horizontal`, `Vertical`, `Horizontal`, `Vertical` constraints (real
    ones, not `auto`); from the centre, a construction diagonal between
    two opposite corners with the centre point as its `Midpoint`. The
    first corner (or the centre) and the one clicked are placed as a
    shape's points (`place`), the other two are new. Four degrees of
    freedom either way.
  - Polygon: centre, then a corner. A construction circle made from the
    centre point (so no concentric), `Drawing::sides` corners on it
    (`PointOnCurve` each), the first where the click snapped, and lines
    between them, each `Equal` to the first: four degrees of freedom.
  - Point: a lone point per click; none where the sketch has one.
  - Spline (`N`, `doc/sketch/spline.rs`): each click places a point
    (`Drawing::placed`, what it snapped to in `targets`), none within a
    pixel of the last, on a point of the sketch's it has already, or past
    `MAX_SPLINE_POINTS`; nothing is proposed until it ends. A click on its
    first point (within `SNAP_TOLERANCE` pixels, `ActiveTool::closes`)
    once it has three ends it closed, as `Shift O` does
    (`ActiveTool::can_close`, which also has the toolbar offer Close
    after Constrain and the status bar its key); a double-click, or `Enter`
    (`Edit::PlaceShape`, `Doc::end_spline_here`), once it has enough
    (`ActiveTool::spline_ends`: two fit points, four control points) ends
    it open where it is. The spline is an `Add` of its points placed as
    any shape's (`place`: a point of the sketch's its own, else a new one
    with the snap's `auto` ties) and the curve, through fit points with a
    handle at every one (`handle_tips`, the shape as without them; `Shift
    H` or deleting a tip takes one away), by control points with
    `control_knots` of the places. `Z` (`Look::ToggleSplineKind`,
    `Drawing::control`, kept from one spline to the next, shown in the
    toolbar's tag) switches between through fit points and by control
    points, the points placed kept. The preview (`typed::outline`'s
    `Outline::Spline`) is the spline through the points placed and the
    cursor (`flatten_spline`), straight between them while too few, with
    its control polygon dashed by control points; with the cursor
    snapped to the first point once it can close, the closed spline the
    click there makes (`Outline::Spline::closed`), its snap's glyph
    Close's icon rather than Coincident's. It has no fields.
  - The MCP client's `edit_sketch` draws with these tools' own code:
    `doc::clicked` (`edit.rs`) makes a `Drawing` of `doc::Clicks` (the
    points placed with their snaps, the last click, the sides, centred,
    a spline's kind and closing) and runs `step`, or `spline_add`, no
    values typed, its pixel `MIN_LENGTH` (see `agents/mcp.md`); a
    spline's points each go through the click's refusals
    (`spline::refuses_point`, which `spline_click` shares: within a pixel
    of the last, a sketch point again, on the first).
  - Trim (`T`), Extend (`J`), Offset (`O`), Mirror (`Shift M`), Fillet
    (`F`) and Chamfer (`Shift B`), the shape tools
    (`doc/sketch/shape.rs`, `Doc::shape_click`), don't snap (`Tool::draws`
    is false) and click what's under the cursor (`ToolClick::hit`): Trim
    proposes `Trim` of the curve clicked there, Extend `Extend` of the end
    of the line or arc clicked nearer the click (`Sketch::nearer_end`),
    both staying in use. Mirror picks (`Drawing::picked`): taken up with
    points or curves selected it has them and waits for the line
    (`Drawing::about`); else each click picks what it's on or takes it out
    again, and `Enter` (`Look::MirrorAbout`, one binding with `PlaceShape`
    as a drawing tool's fields never go with it) goes on to the line. The
    click on a line or axis proposes `Mirror` and starts it afresh. `Esc`
    lets go of the picks first. Extend is `J` and Mirror `Shift M`: `E` and
    `W` open the rail's sets and `M` is Midpoint's, and `J` is free. Offset, Fillet
    and Chamfer pick, then place (`Tool::places`): a click picks
    (`Tool::pick`, into `Drawing::picked`), and once picked, a click
    places what they make of it (`Doc::placed`), drawn through the cursor
    meanwhile. Offset picks the chain the curve clicked is in
    (`Sketch::chain_of`; taken up with a chain, or a curve, selected, that
    one, `shape::offsettable`); then it has a field,
    `Field::Distance` (`ActiveTool::fields`), so the viewport sends
    `Look::Aim` as the cursor moves, and a click (sent where the button's
    let go, `Input::releasing`, so it can be dragged there; `Esc` while
    it's held sends none), or `Enter` at the aim, proposes `Offset` of the
    chain on the side of it the click is (`Sketch::offset_side`), as far
    off as the click, as the design's units show it, or the distance
    typed; less than a pixel off is no distance meant. It then starts
    afresh, picking. The equal offsets are
    `ConstraintKind::Offset`, listed and glyphed (on the copy each ties)
    but not among `ConstraintKind::ALL`, what's applied to a selection:
    it has no key. Fillet and Chamfer (`Tool::corners`) pick a corner the
    way Offset picks a chain: the click's `hit` is the
    point where lines make one (`hit::hit_corner`), and `picked` is it and
    its lines from `Sketch::corner_lines`, the one nearer the click first
    (taken up with two lines selected that make one, those,
    `shape::cornered`). Then a click where the button's let go, or `Enter`
    at the aim, proposes `Fillet` or `Chamfer` through there
    (`typed::corner_outline`), or as typed in its fields: the fillet's
    `Radius`; the chamfer's `Distance` along its first line, then
    `SecondDistance` or `Angle` (one typed lets go of the other,
    `typed::setback`), less than a pixel being no size meant. It then
    starts afresh. `Shift B` for Chamfer (bevel): `C` is the Circle tool's, `B` the
    Rectangle tool's (a box: `R` opens the rail's fourth set).

  - Project and Intersect (`Tool::picks_outside`, on the rail's Modify
    set after Chamfer, no key: `P` is the Point tool's, `I`
    Coincident's, `Shift P` Parallel's; `doc/sketch/outside.rs`) pick
    outside the sketch: while one is in use the cursor picks the model
    (`Doc::picks` holds then, `ModelPicking` with `Picks::All`), and the
    curves and points of the other visible sketches with it
    (`Doc::outside_sketches`, later ones too), and the sketch's own
    geometry isn't hit (`Sketching::hit`), so its selection and box
    don't take the left button: the viewport sends `Look::ClickModel`
    and `Look::ClickSketch` as on the model (see "Sketches picked with
    the model" in `agents/viewport.md`), which the app turns into
    `Doc::outside_click`. Project takes edges, faces (the face's outline:
    its outer loop and holes, one link), corners (a vertex as the
    `PointRef::Corner` of its picking corner) and other sketches' curves
    and points; Intersect faces and edges. What it takes is named as a
    feature at the sketch's place names it (`Naming::before`:
    `edge_ref`, `checked_face_ref`, `corner_ref`) into an
    `OutsideRef`, and proposed as a new link of the tool's kind from it
    (`Doc::propose_link`: `SketchEdit::AddLink`, whose proposal carries
    the source, committed by `Command::AddLink` with the new link's id,
    one undoable change); a click on what a link of the tool's kind
    already comes from (a model pick by the item it finds on the model
    shown, a sketch's by its id) proposes deleting that link, but the
    sketch face's (below), refused with a notice. The link
    holds nothing until the next model answers it, its geometry then
    folded into the change that added it (`agents/features/sketch-links.md`, Sketch
    links). Anything else is refused with why in the status bar (`Doc::notice`):
    "Intersect takes faces and edges, cut with the sketch's
    plane", "Only what's made before Sketch 3 can be projected" (a later
    sketch, or a body or face made later). What's picked is drawn: the
    model's as selected (`Doc::outside_highlight`, found on each model
    shown; the hover only where the tool takes its kind), drawn over
    the faded model, and sketches' items in the selected colour on the
    sketch's live layer (`ModelPicking::marked`): the sources of the
    sketch's links of the tool's kind (`Doc::outside_links`), as it's
    worked on: a link waiting on the solver to be added counts
    (`Waiting::sources`), so a second click on its source before it's
    committed proposes deleting it, after the add, as the other tools'
    edits go on from those waiting, rather than adding it twice. `Esc` lets
    go of the tool; the links stay. A shape drawn snapping to a link's
    point gets a point of its own, coincident with it (`place`), the
    viewport grabs no link geometry to drag, and deleting a selection
    (`Delete`/`Backspace`, `Doc::delete_selection`) holding any of a
    link's geometry (a curve or point of it, or all of it, its row in
    the Sketch tab selected) deletes the whole link with the rest of
    the selection, one undoable change, as only the link can go
    (deleting a link's item alone is `EditError::Linked` to the edit).
    The sketch face's geometry is left out of it (nothing else
    selected, a notice says it stays).

  **The sketch face**: a sketch on a face (`Plane::Face`) has a Project
  link of that face, its outline (`varde_document::sketch_face`: the
  first Project link whose source is the sketch's own face, told by the
  source alone, nothing more stored). The document gives it one
  (`agents/features/sketch-links.md`, Sketch links); the app names it "Sketch face"
  in a group of its own in the Geometry list, before Projected
  (`GeometryGroup::SketchFace`, `LinkRow::sketch_face`). Its menu has
  only "Construction", ticked while it's out of profiles
  (`Edit::SetLinkProfiles`), and `X` turns it so too while any of it is
  selected (`Doc::toggle_construction`, beside the other curves
  selected); it starts construction. Any other link with some of it
  selected turns whole under `X` too, with the curves selected, all
  construction unless they all are (a link's curve alone can't). It can't be removed: no Remove,
  `Edit::RemoveLink` and the Project tool's second click refused with a
  notice (`links::SKETCH_FACE_STAYS`), deleting a selection leaves it,
  and the document refuses any sketch set without it
  (`EditError::SketchFace`).

  The shapes are worked out by `varde_view::typed::outline`, which the
  viewport's preview draws too, so what's shown is what's placed.

  What a click snapped to goes into the `Add` (`edit.rs`, `place`): a
  point of the sketch's is the shape's own (a line from it joins it; the
  Line tool joins existing points this way), anything else a new point
  with `auto` constraints tying it there: coincident with the origin, a
  midpoint, on a curve or an axis, a quadrant as on the circle and level
  with its centre. A circle's rim or an arc's last point on a point passes
  through it (point on curve), and what the line inferred is a horizontal,
  vertical, parallel, perpendicular or tangent (`tangent_between`, on the
  side the geometry is) of the new line or arc. The solver drops the
  `auto` ones that restate the rest (see Edits and proposals), so a snap
  never fails an edit. The session keeps where the next click snaps
  (`SketchSession::snap`, from `Look::Snap`, or `Look::Aim` while the
  shape has fields, see Typed values) for the glyph, until a click or
  another tool; what it snapped to that an undo takes is let go of
  (`Snap::within`, in `Doc::prune`), the place kept.

  A shape smaller than a pixel (a line or radius shorter than one, an arc
  whose third point is within a pixel of the line through its ends), or a
  line over one joining the same points (back to the point before, or
  closing a chain of one line), is refused: nothing is committed and the
  tool waits for a better click.
  Tools stay in use after a shape; `Esc` drops the shape being drawn, then
  the tool, then leaves the sketch. Tools only exist while the document
  can be edited. An MCP client's edit leaves the sketch as `Esc` would
  when nothing is unfinished in it, and asks first when a value field,
  a label or a drag is open or the tool has started (`Drawing::started`;
  `mcp.md`'s "Edits"); its proposals still waiting don't count, as they
  outlive the sketch.
- **Typed values** (`doc/sketch/typed.rs`, `varde_view::typed`): once a
  drawing tool's shape has a point, it has fields (`typed::fields`: a
  line's `Length` and `Angle`, a rectangle's `Width` and `Height`, a
  circle's `Diameter`, an arc's `Radius` once its ends are placed, a
  polygon's `Sides` and `Diameter`). While it has, the viewport sends
  `Look::Aim` with the click it would take on every move (not only when
  the snap changes; it carries the snap too), which the session keeps
  (`SketchSession::aim`, also set by each click; with the cursor off the
  sketch, gone from the viewport or over a widget on it, the aim, its snap
  and the preview stay where they were) so the fields show by it
  (`Sketching::fields`, a fifth `Anchors` layer, `Anchors::beside` the
  cursor), each with the value typed or what it measures of the preview.
  `Tab` (`Look::NextField`; one binding with the Dimension tool's
  `SwitchRound`, as the two never go together; elsewhere `Tab` alone
  backs out as `Esc` does, the app's `escape_key` leaving it to these
  bindings where they claim it, `Varde::tab_taken`) opens the value field
  (`ValueTarget::Field`, the step 3c field: `VALUE_FIELD`, `Doc::take_focus`,
  `OnEscape`) on the first field, or takes the text of the one open
  (`Doc::take_field`: `typed::read` for the field's `ask` in the design's
  units, an arc's radius at least half its chord, the sides a whole number
  from 3 to 64) and moves on; empty text lets go of the field's value
  (the sides keep theirs). A value refused keeps the field open with why,
  its span selected, as a dimension's does. Values taken are
  `Drawing::typed`, dropped with the shape (`restart`); the sides are
  `Drawing::sides`, kept from shape to shape. A click, or `Enter`
  (`Edit::SubmitValue` from the field, `Edit::PlaceShape` without it,
  both at the aim), takes the open field's text first (refused, the click
  places nothing), then `typed::aim` moves the click to where the values
  hold the shape's point (a line's end, a point on a circle, the arc's
  middle, the rectangle's far corner, the polygon's first corner),
  dropping what it snapped to wherever a value is typed, and how it was
  inferred to run where an angle or radius is. The values go into the
  shape's `Add` as driving dimensions (`typed_dimensions`: a line's
  `Length` and `Angle` from `Id::X_AXIS`, `Side::Positive`, the
  rectangle's `Length`s of its sides at the first corner, a `Diameter` or
  `Radius`), their labels `LABEL_GAP` pixels off, anchored in the sketch
  the `Add` makes. So a shape with its sizes is one proposal, one undo
  step. The snap's glyph shows only what the values typed leave of the
  snap.
- **Splines** (`doc/sketch/spline.rs`, what's done to them pure in
  `varde_view::spline`): `Z` with splines selected and no drawing tool
  (`Edit::ConvertSplines`, one `switch_binding` with the Rectangle
  tool's and the Spline tool's, as they never go together) proposes a
  `Convert` to the other kind for each (`spline::conversions`), a
  proposal each. `Shift H` (`Edit::ToggleHandles`, `spline::handles`)
  gives the fit points selected (of splines through fit points) handles,
  or failing any, the ends of the open splines through fit points
  selected; if they all have them, it deletes their tips. A
  double-click on a spline without a tool (`Edit::InsertSplinePoint`)
  proposes `InsertPoint` there; hovering one without a tool rings the
  place it would go (`Sketch::nearest_on`), and while the sketch has a
  spline the status bar says so. Add point (`Look::ToggleInsertPoint`,
  `SketchSession::inserting`, no key) makes one click do it, until
  `Esc`, a tool or the Constrain tool puts it down; the status bar then
  asks for the click. Remove point (`Edit::RemoveSplinePoints`, no key)
  deletes the spline points selected as one `Delete`, offered while any
  are (`spline::removals`) and disabled, saying why in its tooltip,
  where one is on a curve not a spline of its own (deleting it would
  take that curve) or a spline would be left with fewer than
  `SplineKind::least`, which would delete it whole. `U` (`Look::ToggleComb`,
  `SketchSession::comb`, `SketchState::comb`) shows the curvature comb of
  the splines selected, or hides it, in any sketch. With splines selected
  and no drawing tool the toolbar offers Convert, Handles, Comb and Add
  point (and Constrain) in place of the tools, Add point staying while
  it's on, and Remove point with spline points selected. `Shift O` (`Edit::ToggleClosed`,
  `spline::closings`) closes the open splines and arcs selected that
  can be (`CloseSpline`, `CloseArc`), or failing any, opens the closed
  splines selected, or one of whose points is, at that point or else
  their first (`OpenSpline`), and the closed arcs selected (`Detach` of
  their point), a proposal each; the toolbar offers it as Close or Open
  wherever that's possible, arcs too, and a curve's or point's Geometry
  row's context menu likewise (`Edit::ToggleClosedItem`, the selection
  if the row's among it). The
  Dimension tool takes a handle alone (as a line, or by its tip or end)
  as a line: its whole length (`Measure::Length(tip)`, end to tip), or
  placed off its ends its horizontal or vertical extent from its end to
  its tip; two of its points, as two points, their distance.
- **Construction**: `X` turns the tool's next shapes into construction
  geometry, or without a tool drawing shapes (the Dimension tool draws
  none) the curves selected: all construction unless they all are, then
  all normal.
- **Delete** removes the selection through `Sketch::delete`.
- **Dragging** (`Look::DragGeometry`, then `Edit::DropGeometry`) is a
  drag session of the lane's: each step sends `Drag { session, sketch,
  points, radii }` with the `Move` to where the cursor is (a point or a
  line with both ends by the cursor's movement, the handles at the fit
  points moved going with them, their tips moved as far; a circle or an arc grabbed
  by its edge takes the cursor's distance from its centre as its radius,
  an arc's ends moving along their directions from the centre), and the
  view shows the last solution that converged (`Dragged`), the committed
  sketch until the first. A step that would put anything past `MAX_COORD`
  is left out. Dropping proposes the `Move` on that last solution (`from`,
  while the document is still at the revision the drag started from), so
  it commits what was shown, solved, as one undo step. `Esc`, an undo, any
  other edit or taking up a tool ends it with nothing to put back. Only
  the grabbed item is moved; the rest follows as the constraints demand. A
  drag doesn't start while proposals wait. Solving drag steps inline was
  measured and not taken, see "Dragging" in `notes/Threading.md`.
  A point dragged snaps, unless `Held::FREE` (`Shift`) is held
  (`snap::snap_drag`, in the viewport): to another point or the origin,
  a midpoint or quadrant, then a curve, then an axis, leaving out what's
  on its own curves but the other end of an arc it's an end of
  (`snap::closing`) and every other fit or control point of a spline of
  three points or more it's one of (`own_snaps`); snapped, it goes
  to the snap rather than keep the offset it was grabbed at. The drag
  carries what it snapped to (`DragGeometry::target`, kept in `Drag`),
  and dropping ties the point there with a drawing tool's `auto`
  constraints (`edit.rs`, `snapped`: coincident, midpoint, on a curve or
  an axis, a quadrant's): an `Add` proposed on the drag's last solution
  in place of the `Move`, so still one undo step (with no solution yet,
  after the `Move`, a step of its own). On an arc's other end it's
  `CloseArc` instead, as two coincident ends of an arc are redundant;
  on an open spline's other end `CloseSpline`, its ends made one, where
  it keeps enough points to close (more than three) and isn't linked,
  else its two points are coincident (`spline::close_on_drop` decides,
  for the drop and its preview alike); while it's dragged there the viewport's base layer draws
  the sketch as the drop leaves it (`spline::closed_by_drop`,
  `Sketching::drag_closing`): the curve closed, the point merged away,
  its region filled from the closed sketch's profiles. A circle's rim dragged snaps
  likewise, but only to points, as a circle's rim drawn does
  (`snap::snap_rim`: another point or the origin, not its centre), its
  radius reaching there; dropped, the point is on the circle
  (`rim_snapped`, an `auto` `PointOnCurve`), the same one undo step.
- **Constraints**: the selection's geometry is constrained by a
  `ConstraintKind` (`varde-view`, `constrain.rs`): `make` builds the
  constraints it makes of the selection (several items tie the first to
  each of the rest where the kind takes two, or one each; a tangent's side
  from `Sketch::tangent`; nothing naming a curve's own point), proposed as
  one `SketchEdit::constrain`. Keys: Coincident `I`, Horizontal `H`,
  Vertical `V`, Parallel `Shift P`, Perpendicular `Shift R`, Tangent
  `Shift T`, Smooth `Shift S`, Equal `Shift E`, Concentric `Shift C`, Midpoint
  `M`, Symmetric `Y`, Fix `Shift F`, bound only while they fit
  (`DocumentKeys::constraints`); the keys and the toolbar's and rail's
  buttons toggle (`Edit::ToggleConstraint`, `Doc::toggle_constraint`),
  deleting the constraint where everything selected has it already, of
  that kind on the same items in any order (`same_constraint`), rather
  than restate it, the redundant edit the solver refuses (as
  `Edit::Constrain` still does); Shift where a tool or the rail's sets
  have the letter (Trim `T`, Fillet `F`, Point `P`, Circle `C`, New
  sketch `S`, the sets `E` and `R`). A spline takes a point on it (Coincident), a tangent or a
  smooth join at an end (`Sketch::joint`) and a fix; with a spline
  selected nothing else is offered. A handle selected as a line
  (`Selectable::HandleLine`, below), alone or with lines only
  (`Picked::with_handles`), takes horizontal, vertical, parallel and
  perpendicular, named by its tip; its tip or end selected is a point as
  any. The Constrain tool (`K`,
  `SketchSession::constraining`, never with a drawing tool) lists in the
  toolbar, after its own button and in place of the tools, the kinds that
  fit the selection (`ConstraintKind::fitting`),
  the most likely first: those tying items together first, and of
  horizontal or vertical, parallel or perpendicular, concentric or not,
  the one the geometry is nearer to; applying one clears the selection.
  `Esc` puts it down, as `Space` does it and any drawing tool
  (`Look::PutDownTool`, `DocumentKeys::tool`; a focused value field takes
  `Space` as text). Without the tool and with no drawing tool, anything
  selected has the toolbar list the same kinds after the Constrain
  button, in place of the tools (with splines among it, Convert, Handles
  and Comb before them), but the selection stays as one is applied.
- **The tool rail** (`varde-view`'s `rail.rs`, the app's `doc/rail.rs`):
  the same tools in sets, over the viewport's left (see
  `agents/viewport.md`). In a sketch: Draw (the drawing tools), Modify
  (Trim to Chamfer, then Project and Intersect), Constraints (the
  Constrain tool, then every kind with a key) and Dimension; outside
  one: Create (Sketch, Extrude, Revolve), Modify (Combine), Transform
  (Move, Mirror, Linear pattern, Circular pattern) and Inspect
  (Measure); a set with nothing the app has yet isn't shown. An entry
  sends what the toolbar's button does (its binding, `Entry::binding`), enabled where
  that is. The top row's letters, `Q`, `W`, `E`, `R`, ... (`rail::SET_KEYS`),
  open the mode's sets' lists, or close the one open (`RailLook::Toggle`,
  last in `document_bindings`): no tool has one of them (so the Rectangle
  tool is `B`, Chamfer `Shift B`, Mirror `Shift M`, Equal `Shift E`, the
  Rectangle's and Spline's switch `Z`), and outside a sketch, with four
  sets, `Q` to `R` do, so Extrude is `X`, the UI mock's key; while one is open
  (`DocumentKeys::rail`, `Doc::rail`), its letters come before every other
  binding (`rail::letter_bindings`), and claim their key even while their
  tool can't be used (`Binding::claiming`), so a constraint that doesn't
  fit does nothing rather than take up the tool with its letter. A letter
  is the tool's own key where that's a letter alone (Line is `L` there
  too), else the first free one of its key's letter (Shift dropped:
  Parallel is `P`) and its label's (Mirror, with no key, is `I`), never a set's key, so another set
  opens from an open list (Equal is `U`), all from `rail::letters`, which
  the list's letters show. The keys are on a row of the open list (`RailOpen::row`, the
  first as a set opens): `Up` and `Down` move them, round from one end
  to the other (`RailLook::Up`, `Down`), and `Enter` picks it, claimed
  like the letters, so it doesn't edit the feature selected or commit an
  extrude while the list is open. Picking a tool, from the list or not (`SelectTool`,
  `ToggleConstrain`, `Constrain`, `PickPlane`, `StartExtrude`,
  `EditFeature`), closes the list; so does `Esc`, first in the backing-out
  chain, and entering or leaving a sketch, whose sets differ. A focused
  field takes letters as text.
- **Selection**: a click selects what it hits alone, or nothing;
  `Ctrl`-click (`Cmd` on macOS) adds or takes out. A box selects what's
  inside it dragged left to right, what it touches right to left, `Ctrl`
  adding to the selection. The button held still over items that
  overlap lists them to choose one from ("Overlaps" in
  `agents/viewport.md`). `Space`, but where it puts a tool down, clears
  the selection everywhere: the
  sketch's in a sketch, else the model's and the Timeline's (see
  "Selecting" in `agents/viewport.md`). A row of the Geometry or
  Constraints list, or a glyph, sends `Look::ClickRow`, which the app
  turns into a click adding with the command modifier held (tracked from
  `ModifiersChanged`, `Message::CommandHeld`). Hovering one sends
  `Look::HoverItem`, kept in the session (`hovered`), which the viewport
  highlights: geometry itself, a constraint by what it ties together.
  Leaving sends `Look::LeaveItem(id)`, which only lets go of `id` if it
  is still the one hovered: moving up a list, the row entered tells
  before the one left (links likewise, `Look::LeaveLink`). The
  selection and hover may name items still waiting on the solver.
- **The Geometry list** (upper half of the Sketch tab, `panels/sketch_tab.rs`,
  `geometry_rows`) is a tree of groups, a header row each, in one
  virtual list (only the rows in view laid out). A header click folds
  its group (`Look::ToggleGroup`, `SketchSession::folded`). Geometry
  holds the sketch's own curves (what no link made), each unfolding on
  its chevron (`Look::ToggleExpanded`, `SketchSession::expanded`, pruned
  to curves the sketch holds) to the points it's made of
  (`Curve::points`; a point shared is under each), then the points no
  curve has. Under an unfolded spline, a handle's tip ("End 1 of Handle 1",
  `Sketch::point_name` less the spline the row is under,
  `geometry_point_name`, numbered by `Spline::handle_number` in its fit
  points' order) is followed by its end ("End 2 of Handle 1"), each a
  point's row. A folded curve with one of its points selected shows a round
  dot in a selected row's colour after its name. A curve's note is its size (`dimension::size_note`: a
  line's length, a circle's diameter, an arc's radius, or the other of
  the two a driving dimension measures), faint, unless it's driven: in
  `Analysis::sized` with no driving dimension of its own measuring it,
  the constraints setting it, it's after an equals sign in a green pill
  (`driven_note`, `theme::driven_pill`, `SketchColors::driven`), as a
  reference dimension's label is over the viewport; a construction curve's
  icon is in the construction colour, with a rail of three short faded
  dashes left of it (`construction_rail`). A point's note is where it is. Its context menu
  (`RowMenu::Item`) deletes it (`Edit::DeleteItem`), or the selection if
  it's among it; a point two curves or more are made from, or a closed
  arc's (`Sketch::detachable`: not the origin's nor a link's) has Detach
  above (`Edit::DetachPoint`, `SketchEdit::Detach`, `Sketch::detach`):
  each curve but the first gets a point of its own at its place, tied
  back by a real `Coincident`, so the joint shows in the Constraints list
  and comes apart once that's deleted; its constraints and dimensions
  stay with the first curve. A closed arc's end gets a point of its own
  there too, opening it, tied by nothing (two coincident ends of an arc
  would be redundant). The Objects tab folds its Bodies
  and Sketches groups the same way (`Look::ToggleObjectGroup`,
  `Doc::objects_folded`). After Geometry come Sketch face (above),
  Projected and Intersected, the links
  of each kind, a row each (`LinkRow`, made by `Doc::refresh_links` on
  every sync and model answered: its kind's icon, what it comes from by
  name, `links::source_name`: "Line 3 of Sketch 2", "Edge of Body 1",
  "Face of Body 1", "Corner of Body 1", what's gone said so; "In
  profiles" where its curves count for them; broken, in the danger
  colour with why as the model shown found, or why the document
refused what it found, `MeshFeed::broken`). An
  empty group isn't listed, but Geometry with "Nothing drawn yet."
  while there's nothing at all. A link's row selects what it made
  (`Look::ClickLink`; it shows selected while all of that is), lights
  what it comes from on the model shown while hovered
  (`Look::HoverLink`, `SketchSession::link_hover`,
  `ModelPick::link_highlight`, drawn over whatever else is lit; a
  sketch's item isn't lit), and its context menu (`RowMenu::Link`, only
  in a sketch) has "Use in profiles" or "Leave out of profiles"
  (`Edit::SetLinkProfiles`) and "Remove" (`Edit::RemoveLink`, deleting
  the link with what it made; the sketch face's has neither but
  "Construction", above), both proposed, acting only while the
  document can be changed. In the viewport points the constraints leave
  free have their rims in `SketchColors::point`, the orange the drawing
  tools' icons draw points in; a link's curves and points'
  rims are in `SketchColors::link`, purple,
  curves dashed unless they count for profiles, as construction ones.
- **The Constraints list** (lower half of the Sketch tab) shows the
  constraints and dimensions on the points and curves selected (a
  curve's points included), or
  selected themselves, or all when no point or curve is; while only
  constraints and dimensions are selected it keeps listing on the points
  and curves it did (`SketchSession::listed_on`, kept by
  `Doc::list_selection` and `Doc::prune`), so a row clicked, or
  double-clicked, stays where it is; those in conflict first, in red,
  then by id; each as "Tangent · Line 2, Arc 1" or "Length 40 mm · Line
  3" (a reference's value in brackets); only the rows in view laid out,
  like the Geometry list. Double-clicking a driving dimension's row opens the value
  field in it. Its header reads "Constraints for selection" while it
  lists on points and curves. Its header's eye hides and shows the glyphs
  (`Look::ToggleGlyphs`).
- **Dimensions** (`doc/sketch/dimension.rs`). The Dimension tool (`D`,
  `Tool::Dimension`) picks with each click (`ToolClick::hit`) rather than
  placing points: `varde_view::dimension` says what the picks measure
  (`joins`, `measure`: a line's length or, the label off its ends, its
  horizontal or vertical extent; two points the same way; a point and a
  line; two lines' distance if parallel, else the angle whose corner, or
  the one across from it, the label is in; a circle's diameter, an arc's
  radius, `Tab` switching, `Look::SwitchRound`; a circle or an arc
  with a point, a line or another, the gap from its edge, the one
  holding the other first: picking the circle itself, not its centre,
  which measures from the centre as any point). A click on something that
  joins what's picked picks it, on something else starts afresh from it,
  and on nothing, on a pick, or with two picked places the dimension with
  its label there. The selection is picked when the tool is taken up, if
  it measures something. Placing opens the value field
  (`SketchSession::value`, a `ValueEdit`: its `ValueTarget`, the text, the
  error, whether it's in the Constraints list) with what it measures now;
  double-clicking a driving dimension's label or row opens it on that one
  with its expression as typed. The app asks iced to focus it with its
  text selected (`Doc::take_focus`, `varde_view::VALUE_FIELD`). `Enter`
  (`Edit::SubmitValue`) reads it with `varde_expr` for the measure's `ask`:
  a new dimension is an `Add` (driving), a changed one a `SetDimension`;
  text that isn't such a value keeps the field open with why, the span
  selected. `Esc` (`Look::CancelValue`, the field's `OnEscape`) closes it,
  as does any other edit or click in the sketch. With the reference
  modifier (`Alt`) held, the placing click adds a reference at once, no
  field, holding what it measures (`Sketch::held`, as `SetDriving` does);
  in the Dimension tool `Alt` doesn't peek (`Doc::peeks`). Placing a
  distance between two points at one place (`Sketch::same_place`: no
  telling which way to move them apart, so a driving one can't solve) is
  refused saying so (`EditError::SamePlace`, also refused by
  `SketchEdit::apply` for a driving one added or made driving), as is a
  reference, or one made driving, whose measure no dimension can be,
  such as a distance past the limit (`EditError::OutOfRange`); the picks
  start afresh.
  `Shift D` (`Edit::ToggleReference`) turns the dimensions selected
  between driving and reference, a `SetDriving` each. Labels are dragged
  (`Look::PressLabel`, `Look::DragLabel`, `Edit::DropLabel`) into a
  `MoveLabel`, shown meanwhile as an offset. A refused driving dimension
  (`Rejected::Driving`) says in the status bar how to have it as a
  reference.
- **Units**: the file menu sets the design's (`Edit::SetUnits`, a
  `Command::SetUnits`); dimensions show in them (`varde_expr::format`).
  Chosen while edits wait on the solver, whose values were read in the
  units before, they wait behind them like any other change (see
  Proposals).
