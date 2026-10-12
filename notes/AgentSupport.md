# MCP support

Goal: an outside agent (Claude Code, Codex, an IDE) can read and edit
the open design over the Model Context Protocol. A first version, kept
small: Varde serves tools, the agent's edits apply to the design as the
user's would.

Steps 1–4 are built (`agents/mcp.md` describes them); resources, left
out of step 4, are for later. Where this plan differs from the code, the code wins.
A fuller design (a chat panel talking to the user's own model endpoint,
edits made on a fork and reviewed before applying) is on the
`agent-panel` branch; *Step 4* says why it stays there.

## Decisions

- **MCP only.** No chat panel and no model endpoints in Varde: the agent
  is the client's.
- **Native only.** A page can't listen: `varde-mcp` is built for native
  targets only (`cfg` in `app`; `web` never has it).
- **Edits apply directly,** as the user's do: the `Command`s a panel's OK
  would make, one undo step each, undone with the user's Undo like any
  other. No review, no fork, no lock.
- **No agent thread.** The work a call needs off the UI thread goes to
  the lanes already there (regen, solve, io); the server only listens.
- **Off by default.** The settings turn the feature on (the panel and
  its toolbar toggle); the server is started and stopped in the panel,
  and starts stopped at each launch.
- **One client session at a time;** a new one replaces it.
- **Never under the user's hands.** A client edit closes the sketch or
  panel the user has open, asking first if they have work unfinished in
  it (see *Editing while the user is*).
- **No call waits long.** Clients drop a call on their side (Codex after
  60 s, the TypeScript SDK's 60 s, Claude Code after 5 min of silence),
  so every wait in Varde ends within 30 s, the popup's included. The
  bound is the app's, not the socket's, and runs from the request's
  arrival, queued time included (clients time out from their send): at
  30 s the app ends the call, takes its edit out of any queue (the
  server's, and the proposals queue's client `Pending`; a sketch edit
  already with the solve lane is marked dropped, as `dropped` marks the
  user's, so its answer is ignored, never committed) and answers *not
  applied*; an edit
  applies only while its call is live, and once applied its answer is
  always *applied*. The client isn't told to retry: what comes next is
  the user's to say.

## Constraints

What the rest of the app already decides:

- **State lives in `Varde`** and changes only in `update`, on the UI
  thread. The server never holds a document: each tool call goes to the
  UI thread as a `Message`, is answered from the app's state or the
  lanes, and replied to through a channel.
- **No blocking on the UI thread.** Heavy work rides the existing lanes:
  regeneration in the regen lane, sketch solving and analysis in the
  solve lane, PNG encoding in the io lane.
- **Checked math on user data.** Everything from the network is
  untrusted: sizes bounded; typed values (`"20 mm"`) parsed through
  `expr` (`Value::new` with the field's `Ask`) against the bounds the
  forms use; JSON numbers checked finite and in the same bounds before
  use; never a bare `+`/`*` on them.
- **No `.vrdp` change.** Nothing of a session is saved. The new lane
  requests and tags (regen, solve) and the editor's new methods are
  in memory or on the lanes' wires only.
- **TOML for files.** The settings are `settings.toml` keys; the token is
  `mcp-token.toml`.

## The server

A new crate, `varde-mcp`, beside `regen` and `solve`: no UI code. It
depends on `serde`, `serde_json`, `httparse`, `futures` and `getrandom`
(`wasm_js` on the web, as `varde-io` has it), and on `document` and
`expr` once tools take arguments in the design's terms (step 3). It
builds on every target (the workspace's members are `crates/*`, so the
wasm check compiles it); only `server.rs`, the sockets, is native. `app` starts it, runs the tools
and owns the session's state.

- **Transport:** MCP's Streamable HTTP, by hand on `std::net` with
  `httparse` for the request head, JSON through `serde_json`; on
  `127.0.0.1` only, path `/mcp`, port from the settings (default 7311).
  POST only: each request is answered with one `application/json` body,
  a notification (or a JSON-RPC response, which Varde never asks for)
  with 202 and none; GET gets 405 (Varde never pushes to the client, so
  no SSE); DELETE ends the session (204). `Accept` isn't enforced: the
  reply is JSON whatever it says. No stdio in the app: it's a GUI, not
  launched by the client; clients that speak only stdio go through
  `varde mcp`, a relay to the port (*Step 4*).
- **Lifecycle:** `initialize` (the token checked as for any request)
  returns an `MCP-Session-Id` (random, from `getrandom`), `serverInfo`
  (`varde`, a title, the version), `capabilities.tools` alone (no
  `listChanged`), and `instructions`: at most 2 KB, the key points first,
  as Claude Code loads only names and instructions until a tool's used
  and cuts them at 2 KB (Varde is a CAD app; the tools act on the one
  open design; call `design_summary` first and again when a result
  reports the user's changes; refer to things by their Timeline and
  Objects names and `body_topology`'s keys; values are typed text in the
  design's units, bare numbers in them; each edit is one undo step the
  user can undo; an edit may come back applied but failing, fixed by a
  further edit; one may come back refused because the user is editing;
  `render` to check by eye). `initialize` and `tools/list` are answered
  by the server lane itself, never waiting on the UI thread (Codex gives
  `initialize` 10 s, opencode `tools/list` 5 s); `tools/list` is
  static. Versions `2025-11-25`, `2025-06-18` and `2025-03-26`: the
  client's if one of them, else the latest. Before
  `notifications/initialized`, only `initialize` and `ping`. `ping`
  anytime. Unknown notifications get 202 and are ignored; unknown
  methods -32601 (`logging/setLevel` too: no logging capability).
- **Requests:** checked in this order: Host and Origin, then the token
  (401 before anything about sessions, so a 404 never tells whether one
  exists), then the session: none but on `initialize` 400, unknown or
  ended 404 (the client starts over). An unsupported
  `MCP-Protocol-Version` gets 400; none, the negotiated one. No batches
  (an array is -32600). An id the session already used is refused
  (-32600). JSON-RPC errors for malformed requests (-32700, -32600),
  unknown methods (-32601) and tools (-32602); everything a tool refuses
  or fails at is a tool result with `isError`, so the model reads it and
  recovers. `notifications/cancelled` for a waiting call ends it: its
  popup says so, and its edit doesn't apply if it hadn't yet. A
  connection closed without a cancel doesn't end the call (the spec's
  rule); a reply with no one to take it is dropped, the panel's row
  showing what happened all the same.
- **Tools' shape:** plain JSON Schema objects (`"type": "object"`,
  nothing exotic). Annotations: queries `readOnlyHint`; edits not, with
  `destructiveHint` on the removals and `set_feature`, `idempotentHint`
  where it holds (`set_parameter`, `set_visible`). Results that are data
  (`design_summary`, `body_topology`, `measure`, an edit's) carry
  `structuredContent` under an `outputSchema` and the same JSON as a
  text block, for clients that pass text alone; errors are text alone.
- **Edits one at a time.** One edit in flight; the next waits behind
  it, a bounded few, then refused. A query waits for the session's
  earlier edits to apply and for their model (see *Queries*), but not
  behind one waiting on the popup: it's answered meanwhile, saying an
  edit of the session's is still waiting (`pending_edit`). Queued edits fail together: any edit that ends *not applied*
  (refused by the editor, *Keep editing*, cancelled, the 30 s bound)
  fails those queued behind it with its reason, as they were written
  assuming it; one applied, failing or not, doesn't. One popup at a
  time. `pending_edit` comes with edits (step 3); before, queries omit
  it.
- **Auth:** a bearer token on every request, the whole `Authorization`
  value compared in constant time after a length bound. Generated on
  first start and kept in `mcp-token.toml` beside `settings.toml`,
  created readable by the user only (0600, at creation, then renamed into
  place), written through the io lane. The server is loopback only, so a
  keyring guards nothing a file the user owns doesn't: anything that can
  read it runs as the user already. A missing or wrong token gets 401
  with `WWW-Authenticate: Bearer`, and `/.well-known/oauth-*` 404, so
  clients don't try OAuth. `Host` is required and must be `127.0.0.1`,
  `localhost` or `[::1]` with the port; an Origin, if sent, must be
  loopback (else 403), against DNS rebinding; no CORS, `OPTIONS`
  refused. A port in use fails Start, said in the settings: never another port
  silently. Bound without `SO_REUSEPORT`, so nothing else can listen on the
  port beside it (std's `SO_REUSEADDR` on Unix only lets a port in
  `TIME_WAIT` be bound again). Accepted risk, said in the settings: while Varde
  isn't listening, another local process (another user's) can take the
  port, and a client sends its token to whatever answers there. The
  token: 32 bytes from `getrandom`, base64url; its file created with an
  exclusive create at 0600 in the same directory, synced, then renamed
  into place; one found not the user's own or not 0600, or a symlink, is
  made anew. `Origin: null` is refused; Host's port must be the bound
  one; `localhost` is matched as text, never resolved. Regenerate during
  a call ends it as a session's end does.
- **Listening:** not a `varde-lane` lane: a lane's thread waits for the
  app's requests and can't also wait on a socket. A listener thread, and
  one thread per connection up to a bound; events reach the app on a
  `futures` channel, a stream started as the io lane's is (it yields
  `Message::Mcp(event)`, the first event `Ready(handle)` or
  `BindFailed`). Each tool call carries its own reply slot, cloneable as `Message`
  derives `Clone` (a `SyncSender`, or a oneshot in an
  `Arc<Mutex<Option<..>>>`); the connection thread waits on it with
  `recv_timeout` at 35 s, a backstop behind the app's own 30 s. A slot
  dropped unanswered (quitting, which drops what `while_quitting` lets
  by, or the design closed) answers *Varde is closing* or *the design
  closed*. The `Mcp*` messages are native only. The session's state (id, version, used ids,
  initialized) is shared by the connection threads, so `initialize`,
  `tools/list` and `ping` never reach the app. Started by a subscription
  while the setting is on and the app isn't quitting, keyed by the port
  (a new port restarts it); bound before it says ready, so a port in use
  comes back as an event. Dropping the stream stops it: the drop closes
  the listener at once, so a restart can bind the same port. The parts
  that don't touch sockets (JSON-RPC, the checks) build everywhere, so
  the wasm check covers them; the sockets are native only. It does no
  design work of its own.
- **One session.** An `initialize` with the token while a session is open
  replaces it (the token is the user's; clients often go without
  ending theirs): the old one's id gets 404, its waiting call ends (its
  popup says so), the panel says the client changed. A session idle past 30 minutes (no call in flight; the clock restarts
  as a call ends) is ended; the settings' Disconnect ends
  it at once. Either way the client starts over at its next call.
- **Bounds:** per socket read and write timeouts; open connections;
  URL length, header count and total header bytes; request body size
  (`Content-Length` required; chunked bodies refused, 411); JSON
  depth, string and array lengths; queued calls; 30 s for any wait on
  regen, the solver, a frame or the popup; result counts in queries; the
  picture's pixels and its base64 size (Claude Code counts it against
  its 25k token output limit); reply sizes going out, under the 10k tokens Claude Code warns at (`body_topology` and the user's changes above all).
  Every listing has a count cap and paging (`offset`, `limit`) and says
  how many it left out; `sketch` pages its curves and constraints; the
  user's changes past their bound name only what changed, then only how
  many; a whole reply past its byte cap is cut and says so; a cut data reply
  drops its structured content and is marked `isError` (clients check a
  reply without it against the tool's `outputSchema`), its text saying
  to page or filter.
- **Checked math, field by field:** `Content-Length` a `u64` capped
  before reading; JSON-RPC ids an integer or a string of bounded length,
  the used ones a bounded set (oldest forgotten); indices, offsets and
  limits through `usize::try_from` and in range; counts (a pattern's,
  a polygon's sides, `edit_sketch`'s operations and labels, a patch's
  keys) bounded as their forms bound them, labels unique and of bounded
  length, unknown patch keys refused; `size` an integer in its range,
  width × height × 4 and the base64 length (`checked_mul`, 4/3 of the
  PNG) checked against their caps before anything is allocated; `zoom`
  finite in 1/16..64; yaw finite and taken modulo a turn, pitch strictly
  inside ±90°; colours and opacity in 0..1; tolerance in its form's
  range; filters' normals and directions finite, not zero, normalized.

## The tools

In the user's terms, not the code's: names as the Timeline and Objects
show them (feature and body names are unique together, parameters among
themselves), values as typed text in the design's units (`"20 mm"`,
`"90 deg"`; a bare number in them), read by `expr` exactly as a field
would. Faces, edges and corners by the opaque keys `body_topology`
gives, taken back verbatim. Each tool has a JSON Schema for its
arguments (the one place JSON is unavoidable), parsed into a typed
struct and checked.

The design is the open one: the app holds one at a time (`Varde`'s
`Screen::Document`). With none open every call says so. Each call is
tied to the open `Doc` by its `DocId` (`doc/mod.rs`); if `show` replaces
it while a call waits (another file opened, back to the welcome screen,
a recovery), the call fails saying the design closed.

### Queries

Never change anything. A query reads the design as it stands: one
coming while the user's sketch edits wait on the solver, or while the
committed model trails the editor's generation, waits (bounded as every
wait is) until those are committed and regen has answered at or past
that generation, then answers; so do an edit's keys and names before
they're taken. Clients never see a model older than the design they
asked about; at worst the call times out.

- `design_summary` (bodies, the Timeline and parameters each paged by
  `offset` and `limit`, 30 by default): units, tolerance, bodies (name, visible; volumes
  aren't kept, `measure` gives them), the Timeline (each feature's kind,
  name, id, failure if any), the rollback marker, parameters, the
  revision. Failures are those of the last model that isn't a draft, the one
  `body_topology` reads too (each `FeatureFailure`'s message; none while
  the model predates a `Replace`), which may trail an edit and miss
  features past what that answer built (its `until`): the result says
  which generation they're of and how far it got. New: `MeshFeed`'s
  committed model (`Committed`, `feed.rs`) keeps its failures, `until`,
  `picking` and generation, which it doesn't today
  (`failed_features` and `until()` are the shown model's, which may
  carry a draft). *The revision* is `Editor::revision()`; keys carry the
  generation.
- `feature`: one feature's settings, as its panel shows them: in step
  1 a plain listing of the document type's fields (typed text where
  they're values, keys where they're references); the panel's own shape
  comes with `set_feature`, which patches it.
- `sketch`: a sketch's points and curves with their ids, constraints,
  dimensions; its regions (the `Profiles`: each with an index, its area,
  a point inside, its outer curves and its holes), for the kinds that
  take profiles; and its degrees of freedom from the solve lane's
  analysis (waits for it; any sketch, not only one open).
- `body_topology`: a body's faces, edges and corners, bounded in count,
  so the client can name what to chamfer, fillet or sketch on, with
  filters to keep it small (`faces: {normal: "+z"}`, `edges: {direction: "z", convex: true}`; an edge's direction is matched
  either way, as which way it runs is the faces' order, not the
  client's). Each has an opaque `key` (the
  serialized `FaceRef`, `EdgeRef`, or for a corner, which no document
  type names, its three face keys from `Picking`'s `corner_keys` and its
  point, as the other two carry their `near`: postcard, in text). Keys are text through `base64` (url-safe, a new dependency, also for
  `render`): postcard of `(DocId, Generation, the FaceRef, EdgeRef or
  corner)`, so a key from another design is refused. A key carries the
  generation it was read at, and is taken
  only if what it names is in the current non-draft model with the same
  summary (form, normal, area within tolerance): a face key can name
  another face after the user changes a sketch, as `FaceRef` follows
  the features; otherwise it's refused saying to ask `body_topology`
  again. A key read from a model older than a regen answer still to come is
  checked against that answer once it arrives. A
  face carries its form, its normal if flat, its area and bounding box;
  an edge its form, ends or axis, length, its two faces' keys and
  whether it's convex; a corner its point. An edge's form, axis and
  convexity aren't in `Picking` or the mesh: regen computes them from
  the solid on demand, an ordered request of its own beside exports
  (`Request::Topology`, one body at the generation the call read), so
  `body_topology` waits for it and the UI's `Picking` and regen wire
  carry none of it (a regen wire change, not a `.vrdp` one). They were
  in `Picking` first; that cost every regeneration, and on the web
  about 120 bytes an edge on the wire, for MCP alone.
  Areas, lengths and bounding boxes come from the tessellation, `f32`:
  approximate, said so, in the design's units. A face key's `near` is a
  point inside one of its triangles (a centroid can fall outside a face
  that isn't convex); a corner's body is its first face's
  (`face_body`). `Picking` has the keys and
  summaries; points, areas and lengths come from the mesh (`face_ends`,
  `indices`, the edge polylines). Read from the feed's last model that
  isn't a draft; pick indices name a mesh, not a design, and never leave
  the app. The model is as the regen answer built it: past its `until`,
  features aren't built, and the result says so.
- `measure`: the measure tool's inspection on one or two faces, edges or
  bodies by key (an `Inspect` holds two picks at most). New: an ordered
  regen request of its own, `Request::Measure { tag, document, until,
  inspect }`, regenerating and measuring, with its own `Response`, its
  place beside exports in `newest.rs` and its variants in `regen::wire`.
  The regeneration slot's inspect is the user's (selection, measure
  tool), and the two mustn't replace each other; nor can it ride the
  export path, which only welds.
- `render`: a picture of the model, PNG, as MCP's image content (base64,
  `image/png`), its size limit in the tool's description. Arguments, all
  optional: `bodies` (by name; all visible ones by default), `from` (a
  named view, `"front"`, `"iso"`, ..., or yaw and pitch as typed angles,
  `"30 deg"`; the iso view by default), `projection` (`orthographic`,
  the default, or `perspective`), `zoom` (a factor on the framing, 1 by
  default; above 1 closes in), `highlight` (face and edge keys, drawn
  selected, so the client can check its picks before a chamfer) and
  `size` (pixels, bounded, default 768 on the long side). It frames the
  bodies asked for, cropped to them as a thumbnail is; the others aren't
  drawn. Built on the thumbnail's path (`varde_render::frame` and
  `render_preview`), run by the viewport's primitive (only it has the
  GPU), so the result waits for the next frame drawn; none comes while
  the window is minimized, so the wait is bounded as every wait is. New:
  a request of its own beside `PreviewRequest` (which draws light and
  dark for the save's preview, in the app's one thumbnail slot,
  `doc/thumbnail.rs`), drawing once in the current theme with the
  highlight; `frame` taking only the bodies' positions (through the
  mesh's `face_ends` and `indices`, the sketch lines left out) and a
  perspective framing (it's orthographic only: the bounds' sphere
  fitted to the field of view); and an io request encoding the PNG, the
  thumbnail's encoder factored out of `io::thumbnail::encode` (which
  wraps it in a `vrdp` `Preview` capped at `MAX_PREVIEW`). No GPU
  adapter: the call fails saying so.

### Edits

- **Sketches:** `add_sketch` (`plane`: `"XY"`, `"XZ"` or `"YZ"`, or a
  face key), `set_sketch_plane`, and `edit_sketch`: a list of
  operations, in order. A new point or curve may carry a `label`, local
  to the call, which later operations name it by (a line's ends
  `"<label>.start"` and `"<label>.end"`, a circle's centre
  `"<label>.center"`), as they name existing ones by id. Shape
  operations (`rectangle {corner, size}`, `circle {center, diameter}`,
  `polygon`, ...) expand into points, curves and constraints as the
  UI's tools do, so the client needn't keep points' books. Its result
  maps each label to the id it got and gives the sketch's degrees of
  freedom after. `add_link` (Project, Intersect).
- **Features,** one tool per kind, each taking only what its panel
  shows, as typed text and keys, with the panel's defaults: `extrude`,
  `revolve`, `combine`, `move_body`, `mirror`, `pattern` (linear and
  circular), `align`, `scale`, `split`, `chamfer`, `fillet`, `shell`,
  `offset_face`, `draft`, `sweep`, `loft` (the 16 kinds besides
  sketches; `FeatureKind`). Profiles by the region indices `sketch`
  lists, `"all"`, or a point inside; the server builds the `RegionRef`s
  (`Profiles::reference`). Chamfer and fillet are listed though the
  kernel's are stubs today (`agents/features/chamfer.md`, `fillet.md`):
  they apply and fail with *isn't supported yet*, as for the user, until
  it has them.
- `set_feature`: a patch of a feature's settings, as `feature` shows
  them (`{"distance": "15 mm"}`), merged into it.
- `rename` (a feature or a body), `set_visible`, `set_color`,
  `set_opacity`.
- `remove_feature`, `remove_body`: without the user's cascade, as
  `Command::RemoveOnly`. One that others depend on is refused, naming
  them, for the client to remove those too or not; the user's delete
  prompt is never shown to the client's ends.
- **Parameters:** `add_parameter`, `set_parameter`, `rename_parameter`,
  `remove_parameter`.
- **The design:** `set_units`, `set_tolerance`, `set_rollback` (the
  Timeline's marker).

`Command::Replace` has no tool: the client edits the design, it doesn't
swap it.

Each is the `Command`s a panel's OK would make, applied as `Doc::apply`
does (the solve budget, the same checks), with three differences, so a
new sibling of it rather than it: its `EditError` comes back to the
call and never touches `edit_error` (the user's own error showing); a
read-only design refuses it saying so (`Doc::edit` drops edits silently
there); and it queues behind the user's sketch edits still waiting on the solver, as `Doc::change` does (a new `Pending` variant in `doc/sketch/propose.rs`, carrying the
commands and the reply slot, made in `send_proposal`'s loop where
`Pending::Change` is made through `Doc::make`, but through the client's
apply path), so the undo order is the order things happened. At its
turn it holds the queue's head until its model is in (see *Queries*),
the user's later sketch edits waiting behind it: usually a moment, at
most until its call's 30 s run out. A command that changes nothing (the generation
unchanged) answers *nothing to change* at once. New features go where
the user's would: at the rollback marker.

`edit_sketch` goes through the proposals the user's sketch edits take
(`Doc::proposals`, solved on the solve lane, committed with
`Command::SetSketch` or `AddLink` once accepted), which already work by
feature id and go on after the sketch is left. Only the entry
(`propose_with`'s `editable_sketch` gate) wants the sketch open: the
client's proposal is added past it, with a reply slot, the lane, its
order and `solve::wire` unchanged. Past the gate it's checked as `propose_with` checks (`edit.apply`), on
the sketch it'll be proposed on: with no session that's the committed
sketch with its queued proposals applied, built for it (`editable_sketch`
is the session's); its error to the call, never the session's
`refusal` or `drag`. Its commit goes through the client's apply path (above), not
`Doc::apply`, which would set `edit_error`. Every end answers the call:
`proposal_settled`'s refusal (not to the session's refusal or a toast),
a sketch gone meanwhile (`send_proposal` skips it today, unsaid), and a
dimension that would over-constrain, placed as a reference instead
(`as_reference`: said to the client, not toasted). The queue waits on
the user's delete prompt; only the 30 s bound ends that. Analyses can't do
the same: `Order` keeps one waiting `Analyse` whatever its sketch, and
answers are filed by revision into the open session's. So the
client's is `Analyse` with a `query` id (`None` for the user's), tagged
`Tag::Query(id)`, queued in `order.rs` apart from the user's analyses
as a proposal is, through `solve::wire`'s existing analysis checks.
(Not *Inspect*: that's regen's measure type.)

An edit's result names what it made (the feature's name and id, a new
body's name, a sketch edit's labels' ids) and waits for what
regeneration made of it: the first regen answer of its generation or a
later one (the regen slot is latest wins, and the user may edit
meanwhile, so the edit's own may never come; `MeshFeed`'s answered
generation, made public, at or past it, or a failure applied), after
the solve lane's acceptance for a sketch edit. Past 30 s it answers
*applied, regeneration still running; see `design_summary`*. A pending
call table in `Doc`, keyed by generation and checked as answers arrive.
The result reports the answer's failures (`FeatureFailure`'s message;
not the draft's, which `failed_features` holds too), so the client sees
what broke and corrects it; features past the answer's `until` (the
rollback marker, or a feature the user opened since) aren't built, and
the result says so. A failing edit stays applied, as a user's would;
Undo or a further edit fixes it. One the editor refuses (`EditError`: it
fails `Document::check`, a sketch won't solve or runs out of time, a
parameter would leave a value in error, ids ran out, ...) changes
nothing, as a panel's refused OK doesn't, and its result says why. A
rename that clashes comes back with the name it got (`(2)`).

**Undo.** While edits wait on the solver, the user's Undo drops the
newest waiting entry (`Doc::update`, `drop_newest`), as it's their last
action shown. The client's waiting edit isn't shown until committed, so
Undo passes over it and drops the user's own newest one; with none of
theirs waiting it undoes the history, and the client's edit commits in
its turn. New code, not today's: `drop_newest` pops the back of the
queue, and Undo goes to the queue whenever anything waits
(`proposing()`); it becomes *a user's entry waits*, and `drop_newest`
skips the client's entries, queued or in flight. Once committed, Undo takes it back like any step.

Each edit call is one undo step on the design's own history
(the links a sketch's change finds again amend into its step, as for
the user's). The user's Undo and Redo take them back like their own. The client takes back its own newest edits with `undo` (*Step 4*),
or corrects by editing again.

**Revert.** Each edit call keeps the design before and after it, for the
panel's Revert: the editor's `Arc<Document>`s (shared with the undo
history, so cheap while it holds them), each with its `Revision` and
lineage, together a new `EditorState` (`Editor::state()`; `document::Snapshot` is taken, an `Arc<Document>` the regen wire uses, and `State` is the editor's
private one). The after is taken when the result is, after regen
answered, so a link's amend is in it. Bounded by the snapshots' own
size, distinct `Arc`s only (`Arc::ptr_eq`), the oldest rows losing
Revert first.

A revert is one undoable edit through a new `Editor::restore`, not
`Command::Replace`. Undo winds the documents' id counters back
(`Document::next_id`, each `Sketch::next_id`), so after an undo and a
new edit a kept snapshot's ids may name other things than now. So
`restore` keeps the larger of each counter (the document's, and each
sketch's present in both), and keeps the lineage only when the snapshot's
revision is still on the editor's history (a new query: the undo or
redo stack, or current; the undo stack holds `MAX_UNDO`, 200, so older
rows fall to the other case): then `Doc::sync` keeps the selection and
sessions, as after an undo. Otherwise it starts a new lineage, as
`Replace` does: `sync` drops the selection, sessions and picks and
resends the whole model. A snapshot of another lineage (before a
`Replace`) can't be restored; its row loses Revert. The client's next
result says so, as any change of the user's (below).

**After the fact.** Once applied, an edit stays applied whatever comes
after: a cancel, the session replaced, the server stopped, the design
closed. Its row says *Applied (client not told)* where no one took the
reply. A new session's diff starts from the document at its
`initialize`, so such an edit isn't listed to it. A result whose edit
the user took back while it waited on regen (an Undo, a Revert) says
*applied, then undone by the user*: its after-revision is neither
current nor on the undo stack. From a client edit's arrival until it's applied or ends, Revert is
refused, saying so; once applied and waiting on regen, it's allowed.
The user's Undo and Redo aren't refused.

**Edits at the same time.** The user can go on editing while a client
works; both edit the same design, as two people would. A call refers to
things by name and key, so a step the user removed fails saying so, and
the client queries again.

**Editing while the user is.** No client edit lands under an open
session: the sketch the user is in, or one of the four operation
sessions (`extrude`, `revolve`, `combine`, and `motion`, which is move,
mirror, pattern, align, scale, split, chamfer, fillet, shell, offset
face, draft, sweep and loft), new or opened on a feature. A panel keeps
a working copy its OK writes whole, which would undo the client's edit
unsaid; a sketch's edits are in the document already, but it rolls the
model back to it and is left out of it. So an edit call first closes
it:

- **Nothing unfinished:** it closes as Esc would (a panel's session
  dropped, `self.extrude = None` and so on; the sketch through
  `finish_sketch`), and the edit applies. A sketch has something
  unfinished with a `value`, `label` or `drag` open or its tool started
  (`started()`, private today); its proposals still waiting aren't (they outlive
  `finish_sketch`, and the client's edit queues behind them). A panel
  has, when its feature differs from what it opened with: new, each
  session keeps that baseline (built as `session.extrude()` and the
  others build theirs; a new extrude's default distance depends on the
  camera, so it's kept, not rebuilt).
- **Something unfinished:** a popup asks: *Claude Code wants to change
  the design while you're editing Sketch 2.* **Apply yours** finishes
  the user's work as its OK would (`commit_extrude`, `commit_revolve`,
  `commit_combine`, `commit_motion`; a failing preview's OK isn't taken
  for the user, so that refuses too) or leaving the sketch would, closes
  it, then applies the client's edit. An OK refused (not ready, a
  failing preview, refused by the editor; the `commit_*` return nothing,
  so it's read as the session still open) leaves it open and the popup
  saying why; the call goes on waiting. **Keep editing**
  refuses the call, saying the user is editing Sketch 2.
- **Past 30 s** the call returns: not applied, the user is editing. The
  popup stays, marked *Claude Code's edit was cancelled* (with what it
  was), its buttons gone but Close: the client isn't asked to retry,
  the user tells it if they want. The same when the call is cancelled or its session ends; *Apply yours*
  clicked after then finishes the user's work alone. A client edit
  coming while it shows doesn't wait on it: the popup asks about that
  edit instead, its buttons back.

The other open states: the rename field and a parameter being typed
count as unfinished (asked); the plane being picked for a new sketch,
the Timeline marker being dragged and the measure tool close silently. So
does the delete prompt: the edit dismisses it, as any change of the
design does.
Queries never ask.

**The user's changes since the client's last call** go with each call's
result, so the client's picture of the design doesn't go stale: what the
user added, removed or changed (features with old and new values,
sketches, renames, shown or hidden, parameters, units, the rollback
marker), their undo or redo of the client's edits, and a revert. Built
by diffing the document as the client's previous call left it (an
`Arc`, kept; `Arc::ptr_eq` short-cuts no change) with the document now,
by id within a lineage, in the tools' terms; a new lineage is reported as *the design was replaced: query it
again*. If the previous document is no longer on the history (the user
undid past the client's edits, then edited), the result says *the user
undid your edits* first, and an id that names something new (another
kind, or made at another revision: undo winds the counters back) is
reported removed and added, not changed. The panel's lines of the
user's own edits come from the same diff. The client's own edits
are told apart by their after-revisions, which the session keeps.
Bounded: past a size it names only which features changed and leaves
the client to query them. The result also carries the design's
revision.

## The UI

- **Settings:** an **MCP** tab on the settings page (desktop only, as
  Files is): *MCP* (a switch, off: the feature, its panel and toggle, not the
  server), the port (a field,
  1024–65535; one in use makes Start fail, said in the row), the client
  connected (with since when, and Disconnect), the token (hidden, with
  show, Copy and Regenerate, which ends the session), and, with Copy,
  carrying the token: the `claude mcp add` line, Codex's
  `~/.codex/config.toml` entry (`[mcp_servers.varde]`, `url` and
  `http_headers`: `codex mcp add` has no header option) and opencode's
  `opencode.json` entry (`"oauth": false`). Switch and port are
  `settings.toml` keys (`settings::Key`), with row resets and in Reset
  to defaults' list.
- **Panel:** while the feature is on, a toggle in the toolbar opens a
  panel on the right (the viewport shrinks). Its head: the MCP chip
  (green with a client), the client's name, Disconnect, Start/Stop, ✕
  (hides only). Its body: the client's calls, newest last, in the user's
  words, args as typed. Flat rows as the Timeline's, each leading with
  the tool's own icon, then how long ago it came and its state; the
  result (for an edit, *Applied* and what regeneration made of it; a
  failing edit says so and stays applied) shows in its tooltip. Edit
  rows offer, in their context menu (right-click), Show in Timeline and
  Revert to here / to before it. A revert that would undo later changes
  (anyone's) asks first, listing them as differences (the diff above,
  of the snapshot against now, with no author: the editor keeps no
  labels of steps).
  A row is greyed, marked *Reverted*, when what it did is taken back: its after-revision is
  on the redo stack (undone), or a revert whose own after-revision is
  current or on the undo stack took it back. Worked out each time from
  the history and the reverts' rows, not stored, so an Undo of the revert
  ungreys them. Revert to here is offered only when the row's after isn't
  the design now.
- **The user's own edits** show in the panel between the client's calls
  (*You: renamed Hole 2 to Bolt hole*), from the same diff the client is
  sent, greyed as the client's when taken back; a revert made from the panel is one row of its own, what it changed listed in its tooltip (at most 10, then *and N more*), not as rows of the user's own; and a call whose result
  told the client of them says so under it (*Told: since its last call,
  you renamed ...*), the one line besides the row itself. Stopped, it says so with Start; a port in use
  makes Start fail, said there. Not a review surface: nothing waits on
  it.
- **Popup:** the one asking the user to apply their open sketch or panel
  first, and saying so when the client's edit was cancelled (above).
- **Status:** while a client is connected, a chip in the status bar
  (*MCP · Claude Code*), its tooltip saying edits may come from it; a
  click opens the panel.
- **Edits show as edits:** in the Timeline, the viewport and the undo
  history like the user's; beyond the panel and the popup, nothing marks
  them.

## Lifecycle

- The design closing (another opened, the welcome screen): waiting calls
  fail, their queued edits dropped (never applied to the next design),
  their popup closed with it; later ones say no design is open.
- The server stopping (switched off, a new port, quitting): waiting calls answer *server stopped* where their socket
  lives, their queued edits dropped, the popup marked cancelled; the
  session ends.
- Saving and auto-save are as for any edit.
- Regenerating the token ends the session, as Disconnect does: waiting
  calls end so, and the server goes on listening with the new token.

## Step 4

Two parts, each standing alone: an `undo` tool and the stdio relay.
Neither changes a Decision: edits still apply directly, the server
still only listens and never pushes, one session, native only, no
`.vrdp` change. Resources (MCP's `resources/*`, for clients that attach
context by `@` mention) are left for later.

### The `undo` tool

`undo {steps?}`: takes back the session's own newest edits, as the
user's Undo would, `steps` (1 by default) steps of the design's history
(`Editor::undo`, once per step). Not an inverse built by Varde: the
history's own steps, so a link's amend goes with its edit and ids wind
back as the user's Undo winds them.

- **Only its own.** Each step taken back must be a `ClientStep` of this
  session (its `session` and `start` the open one's), the newest done
  one at its turn, its `after` (through the links' amends,
  `Waiting::amended_to`) the revision then. If any of the `steps`
  newest steps isn't the session's (an edit of the user's, a revert,
  another session's edit, or the user's Undo leaving the design short
  of the session's last edit), nothing
  is undone: *not undone: the design changed since your last edit*,
  with the user's changes as every result carries them, so the client
  edits again. The user's Undo then Redo of the client's edits doesn't stop it: the
  design is again as the client left it, and the client may take its
  edits back over the user's Redo. It never reaches past its session: an edit of a session
  before (the client reconnected) isn't its own. `steps` is bounded by
  the user's Undo's own reach (`MAX_UNDO`, 200), and by how many of the
  newest steps are the session's.
- **No `redo` tool.** The user's Redo can bring an undone edit back; the
  client edits again.
- **An edit like the others.** It goes through *Edits one at a time*
  (queued behind the session's edits; queued ones behind a refused undo
  fail with it), the client's apply path's checks (read-only refused;
  behind the user's sketch edits waiting on the solver, its turn checked
  as above when it comes, not on arrival), *Never under the user's
  hands* (an open sketch or panel closed or asked about first, the popup
  saying *Claude Code wants to undo its last edit*), the 30 s bound and
  *After the fact*. All its steps go in one turn, or none. It answers as
  an edit does, once regen has answered at or past its generation:
  `undone` (each edit's label, newest first), the regen failures and
  `not_built` of the model now, and the revision.
- **The user's changes.** The client's own undo isn't the user's, so it
  isn't reported back as *the user undid your edits*: each `ClientStep`
  undone is marked undone by its session, and `changes::since` passes
  over it. A Redo by the user of such a step is the user's, and
  reported (`redid_your_edits`).
- **Revert and greying** need nothing new: an undone edit's after is on
  the redo stack, so its row greys (*Reverted*) as for the user's Undo.
  From an undo's arrival until it ends, Revert is refused, as for any
  client edit.
- **Tool shape:** `destructiveHint`, not idempotent; `outputSchema` as
  the edits'. `instructions` gain half a line (*`undo` takes back your
  own newest edits while nothing came after them*), under 2 KB.
- **Panel:** a row like an edit's, the undo icon, *Undid: renamed
  feature Extrude 1 to Base* (or *Undid 3 edits*, the rest in its
  tooltip); its context menu Show in Timeline only.

Tested headless in `varde-app`: undo of the newest edit (one history
step, the row greyed, the next result telling no user change); `steps`
of 3; refused when a user edit sits within the steps, after a user Undo,
a revert, another session's edit, with nothing of its own, past the
history's reach; queued behind a waiting edit and failing with it;
behind the user's sketch proposal; the popup with a sketch open; the
user's Redo of it reported; the 30 s bound.

### The stdio relay

`varde mcp`: the native binary run as a stdio MCP server forwarding each
message to the app's port. For clients that speak stdio only, and to
keep the token out of clients' configs. It holds no design and no
session state beyond the HTTP session's id; the app stays the server.

- **Where:** `varde-mcp`'s `relay.rs` (native only, beside `server.rs`):
  an HTTP/1.1 client on `std::net` reusing `http.rs`'s limits and
  `httparse` for the response head. The `varde` binary
  (`crates/binary`) takes `mcp` as its first argument before
  `varde_app::run()`: no window, no iced, no wgpu; logging to stderr
  (stdout is the protocol's). `crates/binary` gains `varde-mcp` and
  `varde-io`.
- **Port and token:** the port from `settings.toml` (`mcp_port`), or
  `--port <n>` (1024–65535). The token from `mcp-token.toml` by a new
  read-only `varde_io::read_mcp_token()` running `mcp_token::load`'s
  trust checks; it never makes or rewrites the file: missing or
  untrusted is an error saying to turn MCP on in Varde's settings. Read
  at start and again after a 401 (the token regenerated), once.
- **Framing:** newline-delimited JSON-RPC on stdin, each line within the
  server's body cap, forwarded as a POST to `http://127.0.0.1:<port>/mcp`
  with `Authorization`, `Host`, `Content-Length`, the `MCP-Session-Id`
  from `initialize`'s answer and `MCP-Protocol-Version` once
  negotiated. A 200's body (bounded: the image cap as base64 plus the
  text cap) goes to stdout as one line; a 202 sends nothing. One request
  at a time on one connection; no threads but a stdin reader. EOF on
  stdin sends DELETE and exits 0.
- **Errors:** connection refused (Varde not running, or its server not
  started) answers the request with JSON-RPC error -32000 *Varde isn't
  running, or its MCP server isn't started: Start it in Varde's MCP
  panel*, and the relay keeps running, so the next call works once it
  is started; notifications are dropped then. A 401 after the reread,
  403 and other 4xx become a -32000 saying what. A 404 (the session
  ended: replaced, idle, Disconnect, the app restarted) replays the
  client's `initialize` and `notifications/initialized`, kept from the
  start, and sends the request again once: the stdio client never sees
  a session end, its next result's `user_changes` starting fresh as any
  new session's. A response past its bound becomes a -32000.
- **Security:** nothing the app's guard doesn't check already: the relay
  is one more local client presenting the token, sent only to 127.0.0.1
  on the configured port. It writes nothing to disk and doesn't echo
  the token in errors.
- **UI:** the settings tab's client lines put a stdio form first, with
  Copy and no token in it: `claude mcp add varde -- <path
  to varde> mcp` (the running binary's `std::env::current_exe`), Codex's
  `command`/`args` entry, opencode's `"type": "local"`; the HTTP lines
  stay below them. The panel and chip see the relay's `clientInfo` as
  the client's.

Tested headless: `relay.rs` against in-memory and port-0 loopback
servers (`server::start` with a fixed token): forwarding, 202, the
session header, the 401 reread, refused then started, the 404 replay,
oversize lines and bodies, EOF's DELETE; and the read-only token read
(missing, wrong mode, symlink: refused, never written).

### The fuller design on `agent-panel`

That branch plans a chat panel in Varde talking to an OpenAI-compatible
endpoint with the user's key, every agent edit on a fork owned by an
agent thread, the live design locked while a fork is open, a review
card (Apply as one undo step, Discard), MCP sessions bound to a
document with Ask first batches, and the token in the OS keyring. It
contradicts *MCP only*, *Edits apply directly* and *No agent thread*;
taking it up is a new decision, not a step of this plan. Step 4 takes
from it only the relay and an agent undoing its own steps only.

## Steps

1. `varde-mcp` (`http.rs`: the request head and bounds; `guard.rs`:
   Host, Origin, then the token; `rpc.rs`: JSON-RPC and its bounds;
   `session.rs`: the session, versions, `instructions`; `tools.rs`: the
   static list and typed arguments; `server.rs`: the listener, its
   stream and stopping), with a fixed token, exercised by tests only (the switch comes with the
   settings in step 2), and in `app` (`mcp.rs`, native only) the
   subscription and the queries `design_summary`,
   `feature` (the plain listing) and `body_topology`, `MeshFeed`'s committed model keeping its failures, `until`, `picking`
   and generation; regen's `Picking` carrying edges' form, axis and
   convexity. `tools/list` lists what's built. A tool's reply: text, the
   structured content, `isError`, cut at the byte cap with a note. Tested headless:
   the pipeline on in-memory streams (the 401/404/400 order, ids,
   versions, sizes, Host and Origin); one loopback round trip on port 0 (`initialize`,
   `notifications/initialized`, `tools/list`, a call the test answers
   itself, a reply dropped unanswered, the listener stopped when its
   stream is dropped); and app tests driving `Varde::update` with calls, on a design opened
   in a `Varde` by a new helper with its feed connected to a `Deferred`
   (as `deferred()` does for a bare `Doc`) and answered through
   `answer` and `varde_regen::handle`. New dependencies `httparse`, `serde_json` and `base64` (`app` also
   takes `postcard`, native only); the constant-time compare by hand.
2. The settings tab (switch and port as `settings::Key`s and `Settings`
   fields, `#[serde(default)]`, the port bounded on load; the token file
   through the io lane, `Stores::mcp_token`, `None` in tests meaning in
   memory), the status chip and the panel.
3. First the edge facts out of `Picking` into `Request::Topology`
   (above), `body_topology` the first call that waits. Edits: features, parameters and the rest through the new apply path,
   waiting for regen, with the popup, and the app's 30 s bound (a tick
   subscription while a call waits, `only_if(doc.mcp_waiting(), ..)`,
   as the toast and auto-save ticks are); then `edit_sketch` through the
   proposals, and the solve lane's client analysis for `sketch`;
   `measure` and `render`; Revert with `Editor::restore`. Last, `feature` paged as `design_summary` is.
4. The `undo` tool (the session's own newest edits, `steps` of them,
   through the edits' queue, popup and bound, kept out of the user's
   changes) and the stdio relay (`relay.rs`, `varde mcp` in
   `crates/binary`, the read-only token read, the settings' stdio lines
   first). Resources later; `agent-panel`'s panel and fork stay out
   (*Step 4*).

Each step keeps `agents/` in step: a new `agents/mcp.md` once step 1
lands, `agents/crates.md`'s table, and the MCP tab into
`notes/Settings.md` and `agents/viewport.md`'s settings page.
