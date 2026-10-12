# The solver lane (`varde-solve`)

Proposals, drag steps and analyses run off the UI thread in a lane per
open document, natively a thread (`thread.rs`) and on the web a Web Worker
(`worker.rs`, the web module in the role `solve`), built on `varde-lane` like
the regeneration lane. `notes/SketchImpl.md` ("Built so far (step 2c)")
has the details.

- **Requests and responses.** `Propose { base, sketch, edit }` answers
  `Accepted` or `Rejected`, tagged with `base`; `Drag { session, sketch,
  points, radii }` answers `Dragged { session, solution }` only when the
  step converged; `Analyse { query, revision, sketch }` answers
  `Analysed { query, revision, analysis }`: the user's with `query`
  `None`, tagged `Tag::Analyse(revision)`, or a client's with
  `Some(id)` (the MCP `sketch` tool's degrees of freedom, `id` the
  waiting call's tag, any sketch, open or not), tagged `Tag::Query(id)`
  (`Tag::analysis`). One path does both, on each side. Each
  carries the design's `units` and resolved `params` too (the document's
  `params_shared` when sent), which sketches are checked in
  (`Request::design`); a `DragSession` keeps a copy of them for its
  steps. A
  request that panics, or that a worker died on, is `Failed` with its
  `Tag`.
- **The `Solver`** answers them (`Solver::handle`), keeping the drag
  session: a step of the session in progress goes on from its last
  solution and ignores the sketch it carries; another session replaces
  it, starting from its sketch. Proposals get `PROPOSAL_TIME` per solve, drag
  steps `DRAG_TIME`, on the platform's clock (`clock.rs`). Tests answer
  requests with a `Solver` of their own.
- **Order** (`order.rs`): proposals and analyses queue in order, a
  newer analysis of the user's replacing one waiting; a client's queues as a
  proposal does, apart from the user's analysis: it neither replaces the
  one waiting nor is replaced by it (or by another query), as its call
  waits for it. Drag steps are latest wins in a slot of
  their own, and a step of a session older than one sent is dropped. The
  queue and the slot take turns, so a proposal waits for one drag step at
  most, and a drag step for one proposal.
- **On the web** the requests wait on the page in a `varde_lane::mailbox`
  holding the same `Order`, one with the worker at a time. The page posts
  a drag session's sketch only with the first step a worker gets of it, and
  checks every reply against the request it had (`wire.rs`). The
  parameters cross as each one's name and text (`Params::sources`), with
  the drag session's sketch only for a drag, and the worker resolves them
  again (`Params::evaluate`) once their count, names' and texts' lengths
  are within `MAX_PARAMS`, `MAX_NAME_LEN` and `MAX_LEN`: sketches
  checked, ids named in an analysis or a rejection those of the sketch
  sent or of the one the edit makes, an analysis of the revision (and a
  client's of the id) it was sent, a drag solution a place per point and
  a radius per circle. The worker checks what it's sent the same way.
- **In the app** `Varde::solve_lane` starts the lane for the open
  document and hands it to `Doc::solver`; what's asked of it is under
  Editing.
- **Unsolved sketches.** The regeneration lane solves every sketch warm
  started (`regen::unsolved`, a settle within the default iterations) and
  names those that don't solve with its model (`Regenerated::unsolved`,
  on the wire in the head). The Timeline marks them "Doesn't solve"; the
  status bar says so in a sketch until its analysis comes. Across a
  replacement of the whole document (see Editing) the ids of a model of
  before may name other features, so `MeshFeed::replaced` (from
  `Doc::sync`) has it give out no unsolved or failed features until a
  model of that generation or newer is shown.
