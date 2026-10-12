# The MCP server

Varde serves MCP (the Model Context Protocol) so an agent can read the
design open in it. The server is `varde-mcp` (`crates/mcp`), which has no
UI code and depends on no other `varde-*` crate; the app's side, the
subscription, the session's state and the answers to tool calls, is
`crates/app/src/mcp.rs` and `crates/app/src/mcp/`. So far the tools are
the queries `design_summary`, `feature` and `body_topology`, and the
edits from `rename` to `set_rollback`, the sketch tools `add_sketch`,
`set_sketch_plane`, `add_link` and `edit_sketch`, `set_feature`, and the features `extrude`,
`revolve`, `combine`, `move_body`, `mirror`, `pattern`, `align`,
`scale`, `split`, `shell`, `offset_face`, `draft`, `chamfer`,
`fillet`, `sweep` and `loft` (see "Edits" below), and `undo`, which
takes back the session's own newest edits ("Undo" below).

The settings' *MCP* switch turns the feature on: the toolbar toggle and
the panel. The server runs only after the panel's Start (until its
Stop, or the feature turned off), once its token has come from the IO
lane (see "Settings and token" below); running isn't saved, so each
launch starts stopped;
the settings page's MCP tab shows it (`viewport.md`'s settings page),
and the document screen its chip, toggle and panel (below). The crate builds on every target, so the wasm
check compiles it, but only `server` and `relay` (the sockets) are native, and the
app's MCP state, message handling and subscription are native only.

## HTTP

`http.rs` is as much HTTP/1.1 as the server needs: `read_request` reads
one request off a stream, `write_response` writes one; the connection
loops. Everything is read within `http::Limits`: the request target at
most 1 KiB, at most 64 headers, the head (request line to blank line) at
most 16 KiB, read a byte at a time out of the buffer so nothing past it
is taken, and the body at most 4 MiB, checked against `Content-Length`
before anything is allocated. A body needs `Content-Length` (digits
alone, the same in every copy); `Transfer-Encoding` is refused, as is a
POST without a length (411). A request breaking a bound is an `Error`
with its status: malformed or a URL too long 400 (not 414), a head too
long or too many headers 431, a body too large 413; the connection is
closed after it, since what's left of the request can't be told from
the next. `Connection: close` (or HTTP/1.0 without `keep-alive`) closes
after the response.

The server only answers the statuses in `Status` (200, 202, 204, 400,
401, 403, 404, 405, 411, 413, 431). A `Response` is a status, a content
type sent only with a body, extra headers and the body; 204 goes with
neither body nor length. A header value with a line break is refused
before anything is written.

## The guard

`guard.rs` checks every request before its body is read, in this order:
`Host`, `Origin`, the path, `OPTIONS`, then the bearer token. Host and
Origin first, against DNS rebinding (a page elsewhere whose name points
at `127.0.0.1` sends its own name in both). The path and `OPTIONS`
before the token, so `/.well-known/oauth-*` gets 404 rather than 401 and
a client doesn't go looking for OAuth. A head that doesn't parse, or
declares a body without a length or past its bound, is refused (400,
431, 411, 413) before the guard runs. `Refusal` maps to the response:
no `Host` 400; a `Host` that isn't `127.0.0.1`, `localhost` (any case)
or `[::1]` with the bound port, or an `Origin` that isn't `http://` and
such a host (or is `null`), 403; a path other than `/mcp` 404; `OPTIONS`
405 with `Allow: POST, DELETE` (no CORS, so no preflight); no token or
the wrong one 401 with `WWW-Authenticate: Bearer`. Everything is matched
as text: `localhost` is never resolved, the port must be digits alone
with no leading zero. `Host`, `Origin` and `Authorization` sent more
than once are refused, so the guard and what reads after can't each see
a different one.

The `Token` is 32 random bytes (`getrandom`, the browser's
`crypto.getRandomValues` on the web), shown as 43 characters of
base64url without padding; only the canonical text parses. It's compared
in time that doesn't depend on where it differs, an `Authorization`
over 64 bytes isn't looked at, and its `Debug` doesn't show it.

## JSON-RPC

`rpc.rs` reads one message per POST body (`parse`) within
`rpc::Limits`: nesting at most 32 deep, counted over the raw bytes in a
loop before `serde_json` sees them, so its recursive parse and drop stay
shallow; then strings (keys too) at most 1 MiB, arrays at most 10 000
elements and objects at most 1 000 keys, checked over the parsed value
with a stack of its own. A batch (an array) is -32600, as MCP has it
since `2025-06-18`. An `Id` is an integer or a string of at most 256
bytes, never a fraction or `null`; it's read early so later errors can
be replied to it. A `Message` is a `Request`, a `Notification`, or a
client's `Response`, which is ignored (202). The codes sent are -32700
(not JSON), -32600 (not a request, or past the limits), -32601, -32602
and -32603. `success` and `error` serialize the replies.

## The session

`session.rs` holds the one client session in `State`, shared by the
connection threads behind a lock. `State::handle` decides each request
the guard passed, with the time passed in, so the idle end is tested
without waiting; it returns an `Outcome`: an `Answer` (a `Response` to
write now, or a `Dispatch` for the app) and the session `Event`s it
caused, in order (`Started` with the client's name cut to 64 bytes,
`Ended` with its `End`, `Cancelled`).

A POST is checked in this order: the body as JSON-RPC (400 with the
error reply if it isn't); `initialize`, which needs no session and
starts one, replacing any (`End::Replaced`); else `MCP-Session-Id` (none
400, unknown or ended 404), then `MCP-Protocol-Version` (one not spoken
400, none taken as the negotiated one); then the message. A request id
the session used already is -32600 (it remembers the last 4096, the
`initialize`'s among them, and every call in flight); then, before
`notifications/initialized`, only `ping` is answered (-32600 otherwise);
unknown methods -32601. `initialize`, `ping` and
`tools/list` are answered here; `tools/call` is parsed by `tools` and
handed on as a `Dispatch`, its id marked in flight until
`State::finish`. GET is 405 (no SSE stream: Varde never pushes); DELETE
ends the session (204, `End::Deleted`).

The versions spoken are `2025-11-25`, `2025-06-18` and `2025-03-26`:
the client's if it asks for one of them, else the newest. `initialize`
answers with the version, the `tools` capability, `serverInfo` and
`INSTRUCTIONS` (under 2 KB, as Claude Code cuts there; it asks clients to name what they create, and says `undo` takes back their own newest edits while nothing came after them), and the session
id, 16 random bytes in base64url, in `MCP-Session-Id`.

A session with no call in flight ends after 30 minutes unused
(`IDLE`, `End::Idle`), found when a request names it, on a new
`initialize`, or by the server's polling (`State::expire`).
`notifications/cancelled` for a call in flight is passed on once per
call, so the app's events stay bounded by the calls in flight; one for
anything else is dropped. `State::end` is the app's own end
(`End::Disconnected`).

## The tools

`tools.rs` has the static `tools/list` (each with an `inputSchema`, an
`outputSchema` for `design_summary`, `body_topology` and `sketch`, read-only
annotations for the queries and `readOnlyHint: false` for the edits:
`destructiveHint` on the removals (`remove_feature`, `remove_body`,
`remove_parameter`), `idempotentHint` on the rest but `add_parameter`,
`rename_parameter` and the feature tools), `parse_call`, which reads `tools/call`'s params into a
checked `Call`, and `ToolReply`. A call naming no tool of ours, or
params not shaped as `tools/call`'s, is -32602 (`CallError::Params`);
arguments that don't fit the tool are a tool result with `isError`
(`CallError::Arguments`), as MCP has input validation errors, so the
model reads why and tries again. Unknown argument keys are refused, so a
misspelt filter isn't ignored. Names are non-blank strings of at most
`NAME_BYTES` (1024, the document's own longest); names echoed back are
cut to 64 bytes. `offset` and `limit` are whole numbers through
`usize::try_from`; `limit` is at least 1 and cut down to `MAX_LIMIT`
(500). A `Direction` is `"+z"`, `"-x"`, `"z"` (unsigned: either way) or
three finite numbers not all zero, normalized after dividing by the
largest component so squaring can't overflow or underflow.

- `design_summary {offset?, limit?}`: `Call::DesignSummary(Page)`,
  default limit 30 per listing (`SUMMARY_LIMIT`).
- `feature {name, offset?, limit?}`: `Call::Feature { name, page }`,
  default limit 50 per list in the settings (`FEATURE_LIMIT`,
  `Page::FEATURE`); its description says a page is part of a list and
  isn't to be sent back to `set_feature` as the list, and
  `set_feature`'s says such a page is refused.
- `body_topology {body, faces?: {normal?}, edges?: {direction?,
  convex?}, offset?, limit?}`: `Call::BodyTopology(Topology)`, default
  limit 16 per listing (`LIMIT`).
- `sketch {name, offset?, limit?}`: `Call::Sketch(SketchQuery { name,
  page })`, default limit 20 per listing (`SKETCH_LIMIT`).
- `measure {a, b?}`: `Call::Measure(Measure { a, b })`, each a
  `Measured::Item` (`{"face"|"edge"|"corner": key}`) or
  `Measured::Body` (`{"body": name}`), names and keys within
  `NAME_BYTES`; read-only.
- `render {bodies?, from?, projection?, zoom?, highlight?, size?}`:
  `Call::Render(Render)`: `bodies` names (at most `MAX_ITEMS`, none for
  every visible one); `from` a `RenderFrom::View` (`"iso"`, the default,
  `"front"`, `"back"`, `"left"`, `"right"`, `"top"`, `"bottom"`, any case)
  or `RenderFrom::Angles { yaw, pitch }`, each typed text or a number
  (taken as its text, evaluated by the app); `projection`
  `"orthographic"` (default) or `"perspective"`; `zoom` a number from
  `MIN_ZOOM` 0.1 to `MAX_ZOOM` 10, default 1; `highlight` a list of
  `{"face": key}` and `{"edge": key}` (at most `MAX_ITEMS`); `size` whole
  pixels from `MIN_RENDER_SIZE` 64 to `MAX_RENDER_SIZE` 1024, default
  `RENDER_SIZE` 768. Read-only; its description states the size and PNG
  bounds.
- `rename {feature | body, to}`: `Call::Rename(Rename { target, to })`,
  the target a `Target::Feature` or `Target::Body` by name, exactly one
  of them given. `Call::edits` tells the edits from the queries.
- `set_visible {body, visible}`, `set_color {body, color}` (`color`
  required: `{hue, saturation}`, hue degrees in `0..360`, saturation in
  `0..=MAX_SATURATION`, 0.3, the document's `Tint::MAX_SATURATION` as a
  fraction; or `null` for the theme's), `set_opacity {body, opacity}`
  (`MIN_OPACITY`, 0.1, to 1). Numbers finite, in range, here.
- `remove_feature {feature}`, `remove_body {body}`.
- `add_parameter {name, value}`, `set_parameter {name, value}`,
  `rename_parameter {name, to}`, `remove_parameter {name}`; `value` an
  expression of at most `TEXT_BYTES` (256, `varde_expr::MAX_LEN`).
- `set_units {units}`: `"mm"` or `"in"`, the file menu's (`Units`).
- `set_tolerance {tolerance}`: text of at most `TEXT_BYTES`, read by the
  app.
- `set_rollback {before}`: a feature's name, or `null` for the end;
  required, so a forgotten argument doesn't roll forward.
- `undo {steps?}`: `Call::Undo { steps }`, a whole number from 1 (the
  default) to `MAX_UNDO_STEPS` (200, the editor's `MAX_UNDO`); an edit
  (`Call::edits`). Listed last (`undo_tool`), annotated destructive and
  not idempotent, no `outputSchema` (as the edits have none).
- Naming what's made: `add_sketch` and the sixteen feature tools take
  an optional `name` (a name as `Arguments::name` reads one: not empty
  once trimmed, at most `NAME_BYTES`), `AddSketch`'s `name` and the
  feature tools' `naming: Box<Naming>` (`Call::naming`). The tools that
  can make bodies take their names too (`making(tool)`, `NewBodies`):
  `body_name` for extrude, revolve, split, sweep and loft (one body),
  `body_names` for pattern (its separate copies, in the order the result
  lists them, at most `MAX_ITEMS`); the parser's `only_making` takes
  them beside the `*_FIELDS`, which stay the fields `feature` shows and
  `set_feature` patches. The schemas get `name`, `body_name` and
  `body_names` added at the end of `list` (a test checks they're the
  properties beyond `fields`). `set_feature` refuses a patch with
  `body_name`/`body_names` (`renaming`, also checked by `parse_patch`
  and the app's `set.rs`, `name` included), pointing to `rename`; its
  own `name` is the feature edited.
- `add_sketch {plane, name?}`: `Call::AddSketch`; `set_sketch_plane {sketch,
  plane}`: `Call::SetSketchPlane`, `plane` a `Reference<WorldPlane>`
  taking a face (`"xy"` in any case, or `{"face": key}`), required
  (`Arguments::sketch_plane`). `add_link {sketch, kind?, from}`:
  `Call::AddLink(AddLink)`, `kind` `"project"` (default) or
  `"intersect"` (`LinkKind`), `from` a `LinkFrom`: `{"face"|"edge"|
  "corner": key}` (`Item`), `{"sketch": name, "curve": id}` (`CurveIn`)
  or `{"sketch": name, "point": id}`. Their schemas are functions
  (`add_sketch_tool` ...), inserted after `set_rollback`, before
  `set_feature`; `add_sketch` and `add_link` annotated not idempotent.
  `edit_sketch {sketch, operations}`: `Call::EditSketch(EditSketch)`,
  read in `tools/draw.rs` (`Arguments::operations`): `SketchOp {label,
  construction, op}`, `Op` one variant per `op`, items `ItemRef` (an id
  or a label text), a shape's points `At` (`[x, y]` as text, or an
  item), sizes as text; the bounds, and each error prefixed
  `edit_sketch: operations[i]:`, there. One table, `OPS` (`OpSpec`:
  word, keys, those required, whether it takes `label` and
  `construction`; a point takes no `construction`), gives the parser's
  known keys, the op list in errors and each op's schema. After `add_link`, not
  idempotent.
- `set_feature {name, ...patch}`: `Call::SetFeature(SetFeature { name,
  patch })`, the patch the other arguments as sent (at least one; bounded
  by `rpc::Limits` only), for the app to read as the feature's tool's
  arguments: `fields(tool)` gives each feature tool's argument names
  (the `*_FIELDS` lists its parser's `only` checks against), and
  `parse_arguments(tool, map)`, which `parse_call` now delegates to,
  reads them as that tool's; `parse_patch(tool, map)` reads a patch,
  the arguments the tool requires and it doesn't give filled with
  placeholders the app never takes (a test checks each feature tool's
  `fields` are its schema's properties and an empty patch reads). Listed after `set_rollback`
  (`set_feature_tool`, inserted before `extrude`), `additionalProperties`
  true, annotated destructive and idempotent.
- `extrude {sketch, profiles, extent?, distance?, second_distance?,
  flip?, taper?, operation?, exclude?}`: `Call::Extrude(Extrude)`;
  `revolve {sketch, profiles, axis, extent?, angle?, second_angle?,
  flip?, operation?, exclude?}`: `Call::Revolve(Revolve)`; `combine
  {target, tools, operation?, keep_tools?}`: `Call::Combine(Combine)`.
  What each panel shows, values as text of at most `TEXT_BYTES`, `None`
  (or the enum's default) for the panel's default. `profiles` is
  `"all"` or a list (1 to `MAX_ITEMS`, 256, the document's
  `MAX_EXTRUDE_REGIONS`, `MAX_REVOLVE_REGIONS` and `MAX_FEATURE_BODIES`)
  of region indices and points `[x, y]`, each coordinate typed text or a
  number (`Profiles`, `Profile`). Extents and operations are words
  (`"one_side"`, `"through_all"`, `"new_body"`, `"subtract"`, ...); a
  revolve's `axis` `"x"` or `"y"` (the sketch's own), `{"line": id}` or
  `{"edge": key}` (`Axis`). `exclude` and `tools` are lists of names,
  `tools` at least one. Annotated not idempotent (asked twice, a
  second feature).
- `move_body {bodies, x?, y?, z?, angle?, axis?}`:
  `Call::MoveBody(MoveBody)`; `mirror {bodies, plane, keep_original?}`:
  `Call::Mirror(Mirror)`; `pattern {kind?, bodies, axis?, count?, mode?,
  spread?, flip?, join?}`: `Call::Pattern(Pattern)`, `kind` `"linear"`
  (default) or `"circular"`, `mode` `"spacing"`, `"total"` or `"full"`
  (`None` for the kind's default), `count` typed text or a whole number
  (taken as its text); `align {body?, from, to, flip?, distance?,
  angle?}`: `Call::Align(Box<Align>)`, each side `{point, direction?,
  second_direction?}` (`AlignSide`); `scale {bodies, point?, mode?,
  factor?, x?, y?, z?, edge?, length?, axis_only?}`:
  `Call::Scale(Box<Scale>)`, `mode` `"uniform"`, `"per_axis"` or
  `"edge_length"`, `edge` a key. `bodies` 1 to `MAX_ITEMS` names. What
  a motion refers to is a `Reference<W>`: a world word (`WorldAxis`
  `"x"`, `"y"`, `"z"`; `WorldPlane` `"xy"`, `"xz"`, `"yz"`; `"origin"`
  for a point, `W = ()`; each a `World`, whose `word` the call's row
  says, read off the same table the parser reads), in any case, or `{"face": key}`, `{"edge":
  key}` or `{"corner": key}` (`Item`, its `ItemKind`), only the kinds
  the argument takes (an axis or direction an edge or face, a point a
  corner or edge, a plane a face). Align and Scale are boxed, so `Call`
  stays small beside the others (clippy's `large_enum_variant`).
- `split {body, tool, original?, keep?}`: `Call::Split(Split)`, `tool`
  a `SplitWith`, the panel's four tiles: `"xy"`, `"xz"`, `"yz"` or
  `{"face": key}` (`Surface`), `{"body": name}`, `{"sketch": name,
  "profiles": ...}` (`Regions`, profiles as an extrude's) or
  `{"sketch": name, "curves": [id, ...]}` (`Line`, 1 to `MAX_ITEMS`
  ids); `original` `"front"` or `"back"` (`None` if not given, so it
  can be refused for a trim), `keep` `"both"` (default), `"front"` or
  `"back"`. `shell {body?, faces?, thickness?, direction?}`:
  `Call::Shell(Shell)`, `direction` `"inward"` (default) or
  `"outward"`; the only tool `{}` is a call of (the only body, closed).
  `offset_face {faces, distance?, inward?, tangent?}` and `draft {faces,
  neutral?, angle?, flip?, tangent?}`: `Call::OffsetFace`,
  `Call::Draft`, `tangent` on unless given, `neutral` a
  `Reference<WorldPlane>` taking a face. Faces are lists of keys as
  plain strings (as scale's `edge`), at most `MAX_ITEMS`, at least one
  for the offset and draft.
- `chamfer {edges?, faces?, type?, distance?, second_distance?, angle?,
  flip?, tangent?}`: `Call::Chamfer(Chamfer)`, `type` `"equal"`
  (default), `"two_distances"` or `"distance_angle"` (`ChamferType`);
  `fillet {edges?, faces?, radius?, tangent?}`: `Call::Fillet(Fillet)`.
  Edges and faces are lists of keys as a face tool's, at least one
  between them (`edges_and_faces`); `tangent` on unless given.
- `sweep {sketch, profiles, path | helix, keep_orientation?, twist?,
  operation?, exclude?}`: `Call::Sweep(Box<Sweep>)`, exactly one of
  `path` and `helix` (`SweepAlong`, the panel's Along tiles): `path
  {curves?, edges?, tangent?}`, at least one curve or edge, `curves` a
  list of `{"sketch": name, "curve": id}` (`CurveIn`, each naming the
  chain its curve is in, as a click does), `edges` keys; `helix {axis,
  pitch?, turns?, left_handed?, flip?}`, `axis` a
  `Reference<WorldAxis>` taking an edge or face (required), `turns`
  typed text or any number (`number_text`: part of a turn is a helix's
  too, which the field reads and bounds). Profiles, operation and exclude as an
  extrude's.
- `loft {sections, mode?, closed?, rails?, operation?, exclude?}`:
  `Call::Loft(Loft)`, `sections` 1 to `MAX_ITEMS` of `{"sketch":
  name, "region": index or [x, y], "start"?: point id}` or `{"sketch":
  name, "point": id}` (`Section`, `SectionShape`; a start only with a
  region), `mode` `"smooth"` (default) or `"ruled"`, `rails` a list of
  `CurveIn`. `Sweep` is boxed as `Align` is. The thirteen motion-kind
  tools' schemas are a function each (`move_body_tool` ... `loft_tool`,
  sharing `typed`, `bodies`, `profiles`, `tangent` and the like), which
  `list` appends to the literal of the rest: one `json!` literal with
  them passes the macro's recursion limit. Their parsers share
  `Arguments::flag`, `optional_name` and `optional_choice`.

The app's tests check `TEXT_BYTES`, `MIN_OPACITY` and `MAX_SATURATION`
against the constants they mirror (`varde-mcp` depends on no design
crate).

A `ToolReply` is text, optional structured content (`ToolReply::data`
sends a value as both `structuredContent` and its JSON text) and
`is_error`. `ToolReply::result(cap)` builds the `tools/call` result; the
cap is `ToolReply::CAP`, 16 KiB of text, well under the 10k tokens
Claude Code warns at with the structured copy counted. A text past it is
cut on a character boundary with a note saying how long it was and to
ask for less; cut data can no longer be structured content or meet the
`outputSchema`, so it goes as text alone with `isError`. A reply may also
carry a PNG (`ToolReply::png`, `with_png`), sent after the text as image
content (`{"type": "image", "data": base64, "mimeType": "image/png"}`),
bounded apart from the text's cap by `ToolReply::IMAGE_BYTES` (2 MiB,
under 3 MB as base64; past it the image is left out with a line saying
so and `isError`, a backstop: the app refuses one past it first).

## The server

`server.rs` (native only) is a listener thread on `127.0.0.1` and a
thread per connection; it isn't a `varde-lane` lane, as a lane's thread
waits on the app's requests and can't also wait on a socket. `start`
binds before the stream yields anything, so a port in use comes back as
`Event::BindFailed` (the first and last event), never another port; port
0 picks a free one. Nothing sets address reuse beyond std's own
`SO_REUSEADDR` on Unix; `SO_REUSEPORT` is never set. Otherwise the first
event is `Event::Ready(Handle)`, sent from the listener thread so it
comes before any connection's; `Handle` gives the bound address and
`end_session`.

The listener is nonblocking, polled every 50 ms (`POLL`). Dropping the
stream (`Events`) sets a stop flag and closes the listener itself (kept
in `Shared` as `Mutex<Option<TcpListener>>` and taken), so the port is
free when the drop returns and a restart on it binds at once; the
listener thread ends on its next poll. Each poll also expires an idle
session. At most 8 connections (`CONNECTIONS`) are served; one past it
is closed unanswered. A request's head must come within 5 s
(`HEAD_TIMEOUT`) of its first byte, and a connection's first byte within
5 s of the accept: only once one of its requests has passed the guard
may a connection wait up to 60 s (`READ_TIMEOUT`) for its next. The
guard runs on the head (`http::read_request_head`) before the body is
read; a refused request is answered with `Connection: close` and its
body never read. The body must come within 60 s of the head's end and
the response be taken within 10 s (`WRITE_TIMEOUT`). These are deadlines
over the stream (`Timed`), not per-read timeouts, so a peer trickling
bytes, without the token, holds a connection 5 s at most. A connection
is served until it closes, a read fails, or a response goes with
`Connection: close`; one open when the server stops ends after its next
request.

The guard and the session run on the connection thread, so only
`tools/call` reaches the app: as `Event::Call` with a `Reply`, sent
under the state's lock so events reach the app in the order they
happened (a call before any cancel or end of its session). Session
events come as `Event::Session`. The connection waits for the reply at
most 35 s (`REPLY`, past the app's own 30 s bound, see "Waiting
calls"); a `Reply` dropped
unanswered answers "Varde is closing" (`CLOSING`), a timeout "The call
timed out in Varde" (`TIMED_OUT`), and either way the call is finished
in the state, restarting the idle clock. `Reply` is cloneable, as the
app's messages are; the first `send` is taken, and `send` says whether
it was. Timed out, a reply sent as the wait ran out is still the answer
(`try_recv`), and the receiver is dropped at once, so a later `send`
says it wasn't taken (the panel's *Applied (client not told)*). It carries when the
request had come whole (`Reply::came`), what the app's 30 s bound runs
from, so the time the call queued for the app counts. Calls on different
connections can be in flight at once: keeping edits to one at a time
is the app's to do (see "Edits", *One at a time*).

## The relay

`relay.rs` (native only) is the stdio relay's core, for clients that
speak stdio alone: `relay::run(stdin, stdout, Target { port,
token_source })` reads newline-delimited JSON-RPC from stdin and POSTs
each message to `http://127.0.0.1:<port>/mcp`, writing each 200's body
to stdout as one line (any raw line breaks, only ever whitespace
between JSON tokens, become spaces) and nothing for a 202. It holds no design: only the session id from
`initialize`'s `MCP-Session-Id`, the negotiated `protocolVersion` (sent
as `MCP-Protocol-Version` after `initialize`), and the client's
`initialize` and `notifications/initialized` lines, kept from their
successful answers. `TokenSource` is anything giving
`Result<Token, String>` (a closure will do): read when first needed,
not at start, so a relay started before MCP is turned on works once it
is, and again after a 401; its `Err` is the words a request is answered
with. The token goes only into the `Authorization` header to 127.0.0.1;
nothing written to stdout holds it. Whatever listens on the port gets it:
while Varde isn't listening, another local process (another user's) can
take the port and be sent the token, an accepted risk the settings state.

An HTTP/1.1 client on `std::net` (a private `Connect` trait, so tests
plug in an in-memory fake): one request at a time on one kept
connection, a new one if the server closed it (`Connection: close`, or
the kept one found closed before an answer began, then tried once
more). Connect within 5 s, an answer within 60 s (past the server's
35 s `REPLY`), a write within 10 s. The answer's head is read with
`http::read_head` (now `pub(crate)`) and `httparse` within
`http::Limits`' head bytes and header count; `Transfer-Encoding` is
refused (as is a 200 whose `Content-Type` isn't `application/json`,
such as an event stream); an answer without `Content-Length` has none
read and its connection dropped, so what follows isn't taken for the
next answer; redirects aren't followed (a 3xx is a status like any);
`Content-Length` is read by `http::content_length`, shared with the
server; the body within `BODY_BYTES`, `ToolReply::IMAGE_BYTES` as
base64 plus 1 MiB for text, the tools list and JSON's escapes. A
session id or version is only kept if short visible ASCII. A stdin line
is bounded by `LINE_BYTES`, the server's body cap: a longer one is read
past and answered -32000 with a null id (its id can't be read without
it); blank lines are skipped.

A line is a request if it's an object with a `method` and an `id`, a
notification with a `method` alone; anything else (a batch, garbage)
goes as it is, for the server to refuse, and the server's JSON-RPC
error (a 400 with a body holding `error`) is passed on as it is. A
notification is never answered, whatever comes back. Failures answer a
request with -32000 (`rpc::Code::Unreachable`, written by `rpc::error`)
to its id as `rpc::Id` reads it, `null` for one the server would refuse
(past `i64`, a fraction, an object, a long string), never rounded; and the relay keeps going: connection refused gives
`NOT_RUNNING` (*Varde isn't running, or its MCP server isn't started:
Start it in Varde's MCP panel*), so the next message works once the
server's started; a 401 drops the token, reads it again and resends
once, a second 401 being an error; a 404 (the session ended) replays
the kept `initialize` (no session header) and
`notifications/initialized`, then resends once, so the stdio client
never sees a session end (a 404 within the replay fails it rather than
replaying again); 403 and other statuses say the code and
reason; an answer past its bound, a malformed one, or a broken
connection say so. A kept connection found closed before an answer
began sends the request again on a new one (`Attempt::Closed`): safe
only because the server never closes a connection without answering
once it has read a request's body, so a close seen then means the
request never reached it and can't run twice. stdin's end sends DELETE for the session, if any,
and `run` returns `Ok`; it fails only on stdin or stdout errors, and
sends the DELETE then too. Only an `initialize` answered with a
`result` replaces the kept session, version and line; a session id on
any other answer is ignored. The 60 s bound is per read, not a deadline
for the whole answer; stdin is read only between exchanges, so a
client's own timeout or `notifications/cancelled` waits on the answer
in flight, and EOF is seen after it.

`relay/tests.rs` runs it against an in-memory fake (forwarding and its
headers, 202, garbage and batches as they are, one line out, refused
then started, the 401 reread and a second 401, an unreadable token,
403, the 404 replay, oversize lines, bodies and heads, a closed
connection reopened, EOF's DELETE, odd ids, a notification answered
200 or 400, an event stream, a redirect, a 404 within the replay, a
failing stdout, a failed second `initialize`, a session id changed on a
later answer) and against `server::start_with` on
port 0 with a fixed token (a session with `tools/list` and a batch
refused ending in DELETE, the replay after `Handle::end_session`, a
changed token reread, refused then started on the same port, the tools
list well within `BODY_BYTES`).

### `varde mcp`

The `varde` binary (`crates/binary/src/mcp.rs`) takes `mcp` as its
first argument before `varde_app::run()`: no window, iced or wgpu.
`varde mcp [--port <n>]` (also `--port=<n>`, `-h`/`--help`); the port
is 1024 to 65535, else `settings.toml`'s `mcp_port` (bounded, its
default 7311 when missing or bad). It calls `relay::run` on the
process's stdin and stdout with `Stores::user()`'s files: the token
source is a closure over `Stores::read_mcp_token` (`varde-io`, native
only), which runs
`mcp_token::load`'s trust checks and never makes or rewrites the file;
missing or untrusted answers requests with `varde_io::NO_MCP_TOKEN`,
saying to turn MCP on in Varde's settings, never holding the token. Both
files are read off the IO lane: the relay is a process of its own and
holds no design. stdout is the protocol's; `log` output and errors go to
stderr. Exit codes: 0 when stdin ends, 1 when stdin or stdout fail, 2
for bad arguments (with the usage on stderr; `--help` prints it there
and exits 0). Anything else as the first argument starts the app.

On Windows a release build is a GUI-subsystem program: started by a
client with redirected pipes it inherits them as a console program
would, so the relay works; run by hand from a console it has no stdio
(reads EOF at once and exits 0, its usage errors unseen). A debug build
keeps the console.

Tested in `crates/binary/src/mcp/tests.rs`: the argument parsing, and
the entry (`serve`) against `server::start` on port 0 with the token and
settings in a temporary `Stores` (no token: the error; with it and
`--port` winning over the settings: `initialize` and `ping` through),
nothing listening, and a broken stdout's exit code.

## The app's side

`Message::Mcp(start, varde_mcp::server::Event)` carries the server's
events, tagged with the `Serve::start` of the server they came from;
`Varde::update` hands them to `Mcp::event` with the open `Doc`, if any.
`Mcp` lives on `Varde` (native only): `serve` (an `mcp::Serve`, port, token
and `start`, a count bumped on each start, set by `Mcp::configure`; tests may set it directly), `running`
(the panel's Start, never saved), the token,
the running `Handle`, a `Status` (`Off`, `Listening(address)`,
`BindFailed(why)`), the open session and its client's name, and the
log; the calls waiting are the open `Doc`'s (`Doc::mcp`, see "Waiting
calls"). `Varde::mcp_server` runs `Mcp::subscription` while the
app isn't quitting: `Subscription::run_with` keyed by the `Serve`, so a
new port or token restarts the server, and none on the web or while
`serve` is unset.

### Settings and token

The switch and port are settings (`varde_io::settings`: `mcp`, off, and
`mcp_port`, a `McpPort` from 1024 to 65535, default 7311, anything else
in the file read as the default), `ViewOptions::mcp` and `mcp_port`,
and `settings::Key::Mcp` and `McpPort`, so chosen, stored and reset as
the others are (Reset to defaults lists *MCP* and *MCP port*,
natively only). `Setting::Mcp` and `Setting::McpPort` set them; a port
outside the range is ignored.

The token is `mcp-token.toml` beside `settings.toml`, kept by the IO
lane (`Stores::mcp_token`, `varde-io`'s `src/native/mcp_token.rs`,
native only; `varde-io` depends on `varde-mcp` natively for `Token`):
asked for at start (`Request::LoadMcpToken`) whether the feature is on or
not, read if it can be trusted (a plain file, on Unix 0600 and owned by
the directory's owner, the same file when opened as when looked at, a
`token` key holding a canonical token), else made with
`Token::generate` and stored (created at 0600 as a temporary file,
synced, renamed into place). One that can't be stored is still used this
session. `SettingsPage::RegenerateMcpToken` sends
`Request::RegenerateMcpToken`, which makes and stores a new one (an
error, the old one kept, if it can't be stored). Both answer
`Response::McpToken` (never on the web worker's wire, `#[serde(skip)]`).
With no store (tests) the lane keeps the token in memory. `varde mcp` reads the token and port without the lane, never
writing (see "`varde mcp`" above).

`Varde::options_changed` and the token's arrival call `Mcp::configure`,
which sets `serve` while the feature is on, `Mcp::running` and the token
is known, leaving it (and the server running) when port and token are
the same; the feature off also clears `running` and closes the panel.
The panel's Start is `Mcp::start` (sets `running` and configures, or,
already running after a bind failure, `retry`, which bumps
`Serve::start` so the keyed subscription starts the server again), its
Stop `Mcp::stop` (clears `running`); neither touches the settings.
Stopped or restarted, the old server's session and calls are
forgotten at once (`restarted`), and stopped, or restarted while listening (the old address is gone), the status is `Off`; a `BindFailed` stays until then or the
next start's event, for the settings row to show. The settings' Disconnect ends the session (`Mcp::disconnect`,
`Handle::end_session`); the client goes from the tab when its `Ended`
comes. `Mcp::shown` gives the tab its `McpState`: the status (`Off`
while not started, Starting while started with no `Ready` or
`BindFailed` yet), the client and
when its session started (`Started`'s time, `when::now`), and the
token's text (not in its `Debug`). A regenerated token
ends the session at once (`Handle::end_session`; `Mcp::token_loaded`,
which then configures): its calls waiting are answered "The session
ended" (`Mcp::session_ended`, as an `Ended` does) and its client shown
gone, the log kept (`restarted` told to keep it), before the server
restarts with the new token, the subscription keyed by it (the old
server's `Ended`, of the old `start`, is dropped).

`restarted` forgets the server before: its session, for which no
`Ended` will come, and its log; its calls waiting are answered "The
server stopped" by the next `Mcp::sync`.
Events the old server had queued as it stopped still arrive after;
`Mcp::event` drops any whose `start` isn't the current `serve`'s (a
call answered with an error), so a late `Ready`, `BindFailed` or
`Started` can't show a stopped server as listening, failed or in a
session. `Started` and `Ended` track the session; an end
or a cancel answers the waiting calls of that session or id with an
error. Every query waits for the model to catch up with the design
(`Wait::Model`) and is then answered by `mcp::answer_now`;
`body_topology`, `measure`, `sketch` and `render` then wait on more
(below). With no design open
every call answers "no design is open".

### Waiting calls

`mcp/waiting.rs`: a call goes to the open `Doc` (`Doc::mcp_call`),
which answers it at once or puts it in its table, `Doc::mcp`
(`waiting::Waiting`), so it's the design's and goes with it. A
`Waiter` is keyed by an app-side tag (`Waiting::next_tag`, what a
lane's answer comes back with), holds where it came from
(`waiting::From`: its log serial, the server's `start`, session, id,
`Reply`, and its arrival time, `Reply::came`: at the server, so time
queued for the app counts) and what it waits for (`Wait`: a query or
an edit not taken up yet `Queued` behind the user's sketch edits or
waiting for the `Model`, `Topology`, `measure`'s `Measure`, `sketch`'s `Analysis`, `render`'s `Render` and `Encoding`, an edit `Asking`, waiting on the
popup, an edit `Solving` (its sketch edit with the solve lane), or an
edit applied waiting on `Regen`; see "Edits"). The table never sends: a call done goes to `finished`
with its reply, and `Mcp::sync`, run after every message in
`Varde::update` (and at once by `Mcp::end`, which a cancel, the
session's end, `tick` and `closing` share, and after a call comes),
sends it and marks its row; it first ends calls whose `start` isn't the
current server's ("The server stopped": stopped, the feature turned off, or restarted).

Each call is bounded by `waiting::BOUND`, 30 s from its arrival (the
server's 35 s `REPLY` is only a backstop): the one-second timer
(`platform::second_ticks`, `Message::Tick`, also auto-save's) runs
while `Doc::ticking()`, which holds while `Doc::mcp_waiting()`, and
`Mcp::tick` (natively) answers the calls at or past the bound
`TOOK_TOO_LONG`, an error saying the model took too long.
A cancel or the session's end answers those calls with why; the design
closing (`Varde::show`, `Mcp::closing`) answers `DESIGN_CLOSED`, and
quitting (`Varde::quit`, `Mcp::quitting`) the server's `CLOSING`, as
neither answers nor ticks act once quitting (`while_quitting`). Each
marks a query's row failed. An edit is answered by its wait
(`Wait::ended`): one queued *not applied* with why (at the bound
`NOT_APPLIED_BUSY`, or `NOT_APPLIED_MODEL` for one holding the
proposals' queue for its model, which `Doc::mcp_end` turns to waiting
for the model before ending it), its row not applied, failing its
session's edits behind it (see "Edits", *One at a time*); one waiting on the popup
*not applied, the user is editing ...* (but for the design closing and
the server stopping, which say so); one applied stays so,
answered with its result and `STILL_RUNNING` (*applied, regeneration
still running; see design_summary*), or `edit::UNDONE` if the user took
it back meanwhile (`Waiting::end` takes `Editor::is_done` from
`Doc::mcp_end`), its row applied. Ends go through
`Doc::mcp_end`, which also takes the ended calls' edits out of the
proposals' queue (`Proposals::retain_clients`), so they never apply
after. A `Done` carries the row's state (`Waiting::answer_as`).

**Waiting for the model.** A query, and an edit's names and keys, are
answered only on a model of the design they're about. How a call gets
there is in one place, `waiting.rs`'s `Doc` methods:
`Doc::client_take_up` queues it behind the user's sketch edits waiting
on the solver if any do (`Doc::proposing`; `Pending::Client` under its
tag, `Wait::Queued`); at its turn there (`Doc::client_queued_turn`,
those before it committed), or at once with none, `Doc::client_turn`
puts it waiting for the model, `Wait::Model { at }` with `at` the
editor's generation then, unless it's ready already: regen has
answered at or past `at` (`Doc::model_ready`: `MeshFeed::answered`,
model, draft or failure, as an applied edit's wait counts it) and no
earlier client edit it waits behind is still to be applied (`Queued` or
waiting for the model: `Waiter::holds`). An edit waits behind every
earlier client edit, a query only behind its own session's
(`Waiter::behind`; tags go up in arrival order). Ready, a query is
answered (`Doc::mcp_query`; `body_topology` then asks for its edge
facts of that model and waits as below), an edit made and applied
(`Doc::client_edit_now`). An edit coming while the popup is up waits on
it first (`ask.rs`), then is taken up as any.
`Doc::mcp_ready`, run by `Doc::mcp_end`, so after every message
(`Mcp::sync`), takes the ready ones one at a time in arrival order
(`Waiting::ready`). A query is answered on the model it waited for:
the user editing on meanwhile doesn't hold it, its `model` then saying
it's not `current`. An edit is taken up again, as
it's made on the design as it is now: first among what's queued on the
solver if anything is (`Proposals::wait_client_first`: it was at its
turn already), else as `client_turn`, waiting for the newer model if
the user edited on; `Doc::sync` runs after any edit is taken up (one
refused may still have closed what the user had open, whose model is
to be asked for). An edit reaching its turn in the proposals' queue
whose model isn't in, behind no earlier client edit, keeps its place
there (`Doc::client_holds_queue`, `client_queued_turn` returning true,
`send_proposal` putting it back first and stopping), so the user's
sketch edits queued after it wait and the undo order stays the order
things came; `mcp_ready` lets it go on once its model is in
(`Proposals::client_first`), and one ended there (the bound:
`NOT_APPLIED_MODEL`, as it waited for the model, also when
`client_queued_turn` finds it late with its model not in) lets what's
behind go on (`Doc::mcp_end` syncs when
`drop_ended_edits` took any). A query at its turn never holds the
queue. Counting a draft's or a
failure's answer keeps a panel left open (whose answers are drafts, the
committed model staying behind) from holding every call to the bound;
the answer then says the model isn't current, as before. A model
absent, or predating a `Replace`, is behind the editor's generation, so
it waits too (the replacing generation is the editor's,
`MeshFeed::replaced`); `NOT_BUILT` and `REBUILDING` stay as fallbacks
only (a draft answering first). An edit applied raises the `at` of its
session's calls after it waiting for the model to its generation
(`Doc::client_applied`, `Waiting::raise_after`), so a query coming after
the session's edit sees it. Queries don't wait behind an edit on the
popup (`Asking`). Ended (the bound, a cancel, ...), a query queued or
waiting for the model fails with why, an edit is *not applied*
(`NOT_APPLIED_BUSY` queued, `NOT_APPLIED_MODEL` waiting for the model,
at the bound: `Wait::ended`). A call found at its turn or ready past
the bound before its tick is ended as the tick would end it
(`Doc::unless_late`, the one check, for the popup's too).
`drop_proposals` ends the edits queued among the dropped proposals and
gives the queries there the generation then (`Doc::mcp_end_queued`).

### Edits

`mcp/edit.rs`: `Doc::mcp_call` hands an edit (`Call::edits`) to
`Doc::mcp_edit`. A read-only design refuses it (*not applied*, saying
so). The call is turned into the `Command`s a panel's OK would make
(`commands`) and what it's to make (`Made`), or fails (a name not found
or arguments the design's forms don't take: row failed; a refusal the
UI makes before the editor: row not applied); none (as it is already)
answers `NOTHING_TO_CHANGE` at once. Per tool:

- `rename`: `Document::rename`, as the rename field commits it, which
  numbers a name taken (its `taken` goes in the result's note).
- `set_visible`, `set_color`, `set_opacity`: `Command::SetVisible`,
  `SetColor`, `SetOpacity` as the Objects panel's eye, colour and
  opacity menus commit them, the values rounded as the sliders round
  them (`Tint::clamped`, `Opacity::clamped`, on values already in
  range). The editor adds no step for one as it is.
- `remove_feature`, `remove_body` (`edit::removal`): what
  `Document::breaking_removal` takes, as `Command::RemoveOnly` (the
  delete prompt's Delete only); refused if `Document::removal` (the
  cascade) takes more, naming those features in Timeline order. The
  delete prompt is never shown. A body takes its maker and the maker's
  other bodies (a pattern's copies), which the result lists.
- `add_parameter`, `set_parameter`, `rename_parameter`,
  `remove_parameter`: `Command::AddParam`, `SetParam`, `RenameParam`,
  `RemoveParam`, a field set or renamed by the popup's own
  `doc::params::param_command` (trimmed; the same text or name changes
  nothing); a removal of one in use refused first
  as the popup refuses it (`doc::params::uses`). A name taken, a name
  not a word, a value that would leave a feature's in error are the
  editor's refusals.
- `set_units`: `Command::SetUnits`. `set_tolerance`
  (`edit::tolerance`): micrometres (`"1 µm"`, `"um"` too, as
  `design_summary` shows it) or a length read by `expr` (bare numbers in
  the design's units) with an `Ask` from `Tolerance::MIN_FIT` to
  `MAX_FIT`, then `Tolerance::new`: any fit in its range, not only the
  menu's three. `set_rollback`: `Command::SetRollback`, as the
  Timeline's marker.
- `extrude`, `revolve`, `combine` (`mcp/make.rs`): set up in the
  panel's own session (`ExtrudeSession::for_client`,
  `RevolveSession::new`, `CombineSession::new`), so its defaults
  are the panel's (a full turn, 180° and 90°; one side, no taper, a new
  body; a union using the tools up), but for a new extrude's distance:
  the panel's follows the view, the client's is `CLIENT_DISTANCE`,
  10 mm, as the schema says. Values and choices go through the
  setters the panel's messages use (`ExtrudeSession::input`,
  `input_taper`, `set_extent`, `set_operation`;
  `RevolveSession::input`), a value its field refuses failing the call
  naming the argument;
  the feature is the session's own (`ExtrudeSession::extrude`,
  `RevolveSession::revolve`, `CombineSession::combine`), its sketch
  made before where it goes (else refused, as `pickable` refuses a
  body), added with
  `Document::add_feature` at the rollback marker, as OK adds it. What
  the panel leaves to the document (its own check, references) the
  editor refuses. Profiles are picked in the session's `RegionPick` as
  clicks pick them: an index into the sketch's regions in the order
  `Sketch::profiles` finds them (which `sketch` is to list), `"all"`,
  or a point read as a length per coordinate and found by
  `Profiles::region_at`; an index past them, a point in none, a region
  too thin for `Profiles::reference`, or a sketch too complex are
  refused. Only a cut goes through all (`set_extent` refusing it);
  `exclude` (bodies a join, cut or intersect leaves out, `BodyTargets`)
  only with those, each `pickable` (made before where the feature
  goes). A revolve's line must be a line of the sketch with length
  (`RevolveSession::pick_axis`, as a click picks it); an edge goes
  through `key::edge` (below), then `Doc::edge_axis`, the panel's
  check of a click on one (`axis_edge`: straight, in the sketch's
  plane; named by `Naming::edge_ref` at where the feature goes), and
  `RevolveSession::pick_edge_axis`. A combine's bodies by
  name, a merged one as the body holding it (as a click picks), each
  `pickable` (made before where it goes), the target not a tool and
  no tool twice (two names one body, a merged one's holder, too). The
  result (`Made::Added { name, notes }`): the feature's name and id and
  the bodies it makes, and a `note` for each name asked for that was
  taken.
- The names asked for (`Naming`, every feature tool): `edit::commands`
  passes the planned commands through `make::named`. The feature's
  name goes in the `Command::AddFeature` itself, by
  `Document::new_name` (the check `Document::rename` shares: line
  breaks and tabs as spaces, other controls and bidi marks dropped,
  trimmed, cut to `MAX_NAME_LEN`, empty refused, one any feature or body has
  numbered "Name (1)"). Body names are renames after it in the same
  undo step (`apply_client` applies them as one): worked out on a
  clone of the editor with the add applied (the ids it gives are the
  ones the real apply gives), the new bodies in document order, each
  `Document::rename` on the clone so far, so names in one call are kept
  apart too. None made (a join, a split keeping one side, a joined
  pattern) is refused saying what makes one; more names than bodies
  refused; fewer leave the rest their defaults. Those defaults, and a
  feature's, are numbered past the names of both sorts
  (`Document::next_number`), so a feature named "Body 2" makes the
  next new body "Body 3", never a clash. The commands are made at the
  edit's turn, on the design as it then is, and applied at once (a
  check made as an edit queues is only that), so the clone's ids hold.
- `move_body`, `mirror`, `pattern`, `align`, `scale` (`mcp/motion.rs`):
  set up in the motion panel's own session,
  `MotionSession::for_client` (the panel's `new` with a default camera,
  a linear pattern's spacing `CLIENT_SPACING`, 10 mm, where the panel's
  follows the view), so the defaults are the panel's (a move by 0 mm,
  0° about Z; a mirror keeping the original; a linear pattern along X,
  3, joined; a circular one about Z, 4, Full 360°; a scale by 1 about
  the origin). Bodies by name as clicks pick them (`make::picked_bodies`,
  which `combine` shares: a merged one as its holder, by the merges
  before the rollback marker only, each `pickable`),
  one named twice refused. Each tool is a closure setting up the session
  run by `motion::made` (the bodies picked, the kind's noun naming the
  feature in refusals) or `made_of` (bodies given: none, or a shell's
  only one), which builds the session, runs it, and adds the feature;
  the closure's `Wrong` is words the tool's name prefixes or a refusal
  whole (`?` takes both). Values go through `MotionSession::input`,
  the panel's `MotionLook::Input`, a field refusing one failing the call
  naming the argument (`field_error`); choices through the setters the
  panel's messages use (`origin_axis`, `origin_plane`, `set_mode`,
  `scale_mode`, `MotionSession::align_origin`); keys are taken back by
  `key::found` and handed to the click paths with the committed model's
  index (`Doc::reference_in` then `take_reference`, `Doc::pick_align`,
  `pick_scale_point`, `pick_scale_edge`; the click handlers run the
  same on `Doc::motion`, taken out meanwhile by `Doc::on_session`), so
  their refusals are the panel's notices ("Only a flat face can be the
  mirror plane", "Pick it on Body 2, the body aligned", ...), prefixed
  with the argument (`motion::by_key`: `key::found` then the click path,
  each refusal naming the argument). An align's slots are taken in the order the panel's
  clicks go to them, so a round edge's centre gives its side the rim's
  axis as a click does, a direction given replacing it. The feature is
  `MotionSession::built`, what the panel's OK takes (`ready` is it plus
  no field refused): its need ("enter a distance or an angle", "enter a
  factor other than 1", "pick the direction to align it to", ...), a
  pattern's spread errors and its own check refuse the call in the
  panel's words. Choices the panel doesn't offer are refused saying so:
  mode full for a linear pattern, a spread with full, Flip for a
  circular one, a scale's values of another mode, `axis_only` for an
  edge not straight along a world axis (told from the mesh's ends by
  `varde_regen::along_axis`; the panel's from regen's measure), an
  align's Flip without directions (the panel drops it unsaid), the
  origin on the body's side.

- `split`, `shell`, `offset_face`, `draft` (`mcp/modify.rs`): the
  motion session as the motion tools have it, sharing their `made`,
  `made_of`, `typed`, `by_key` and `keyed`; `for_client` sets a shell's thickness
  to `CLIENT_THICKNESS`, 2 mm, and an offset face's distance to
  `CLIENT_OFFSET`, 1 mm, where the panel's follow the view; a draft's 3°
  from XY and Tangent faces on are the panel's own. Faces by key go
  through `key::found` and `Doc::pick_face`, the face session's click
  path (`Doc::take_ref_in`, which `refs_click` runs on `Doc::motion`),
  so another body's face, one made later or past the limit is refused
  in the panel's words ("A shell's faces are all on one body: pick faces
  of Body 1"), prefixed with `faces[i]`; a face named twice (two keys
  of one face too) is refused, as a click again would take it out. A
  split's body as an align's (`make::picked_bodies`), then its tile
  (`MotionSession::split_mode`) and tool: an origin plane through
  `split_origin`, a face key through `Doc::pick_split_tool` (the
  panel's `split_tool` runs it: flat as its plane, curved as its
  surface), a body by name through `Doc::split_tool_body` (the body
  split refused, "That's the body being split ...") and
  `take_split_tool`, regions picked in its `RegionPick` by
  `make::pick` as an extrude's, a line's curves by id through
  `split_curve` (an id not in the sketch or named twice refused, as
  the panel's clicks can't make either); the sketch made before where
  the split goes (`make::sketch_named`). `original` only with keep
  both: the panel's Keeps tiles are off for a trim. A shell's body
  only with no faces (the panel's Objects row picks one only then;
  its body is its faces'), and with neither, the model's only body as
  the panel starts (`Doc::only_body`); none among several is the
  panel's need ("pick faces to remove, or the body to hollow"). A
  draft's neutral plane after its faces: an origin plane through
  `MotionSession::origin_plane` (the draft's toolbar buttons share it,
  handing the clicks back to the faces), a face key through `keyed`;
  `Doc::reference_in` holds the check against a face the draft
  tilts (`ref_picked_in` on the index the faces were picked on), so the client's key is refused as
  a click is ("That face is one the draft tilts ..."). The kernel
  builds none of the four, so each applies and its result reports
  regeneration's failure, as Add anyway does for the user.
- `chamfer`, `fillet` (`mcp/blend.rs`): the motion session as the
  motion tools have it; `for_client` sets a chamfer's distances to
  `CLIENT_CHAMFER`, 1 and 2 mm, and a fillet's radius to
  `CLIENT_RADIUS`, 2 mm (the mock's; the panel's follow the view); 45°,
  Equal and Tangent chain on are the panel's own. The Tangent chain tick
  (`BlendSetup::chains`) is set before the edges, which go through
  `key::found` and `Doc::pick_edge`: refused as a click is (another
  body's, made later, past the limit, in the panel's words, prefixed
  `edges[i]`), else taken as a click on one not picked takes it, a
  blend's with its tangent chain while the tick is on, each edge of its
  own (`Ref::unit`; the chamfer stores them, `chains` false, as a new
  one from the panel does); one a click would take out instead (named
  twice, or an edge of a chain taken already) is refused, "that edge is
  named twice, or is in the tangent chain of one named". `pick_edge`
  shares the panel's click code: `refs_click_unit` is now
  `refs_click_unit_in` on a session and index (the panel's runs it on
  `Doc::motion` through `on_session`), and `refs_unit_in` gives the
  unit `refs_click`, `refs_selected` and the client share. Faces go
  through `modify::faces`, as a face tool's. A chamfer's value of
  another type (`second_distance` without two distances, `angle`
  without distance and angle) and Flip with Equal (the panel shows it
  for the other two; Equal is stored unflipped) are refused saying so.
- `sweep`, `loft` (`mcp/sweep.rs`): `for_client` sets a helix's pitch
  to `CLIENT_PITCH`, 10 mm; 5 turns, no twist, Follow path and a new
  body are the panel's. A sweep's profile is picked in a fresh
  `RegionPick` of its sketch by `make::pick` (as split's regions), its
  tile set by `MotionSession::sweep_mode`; a path's chains by
  `sweep_curve` (the profile's own sketch refused in the panel's words;
  a curve of a chain already named would take it out, so it's refused),
  its edges by `blend::edges` (`Doc::pick_edge`, the sweep's tick stored
  as `PathPart::Edges`' `tangent`, nothing grown); a helix's axis
  through `MotionSession::origin_axis` (which now hands a sweep's clicks
  to nothing, as its toolbar buttons did inline) or `keyed`
  (`Doc::reference_in`, then `take_reference`, which sends a sweep's
  clicks to nothing: `picks_faces` is false for it). Keep orientation
  and the twist with a helix are refused (the panel shows them for a
  path). A loft's sections are added in order, each as a click adds it:
  a region (its index found by `make::pick` on a fresh one-region
  `RegionPick`; its sketch put in the loft's `RegionPick::also` so its
  profiles are found, shown or not, as a section's own are) through
  `Doc::loft_region_in` (the panel's `loft_region`, now on a session,
  run by the panel through `on_session`: holes, too thin, a later
  sketch, the 65th refused in its words), a start given then moved by
  `loft_start` and checked to have moved (a point not at a corner is
  refused), a point through `loft_point` (only first or last); a section
  that takes one out is named twice. Then they're put in the order
  given with `section_up`, as the rows' chevrons move them, so a point
  in the middle is the panel's foot ("its section 2 is a point ...").
  Closed before the rails, so a rail with it is `loft_rail`'s refusal;
  a rail taking one out is refused. Sketches by name made before the
  marker (`make::sketch_named`), curves and points by id
  (`modify::curve_in`, which split's line now shares, `point_in`).
  `made_of` also refuses what `Doc::motion_held_in` (the panel's
  `motion_held` on a session) says, as OK waits on it: a sweep's path or
  a loft's names the document refuses at its place. The kernel builds
  none of the four, so each applies and its result reports
  regeneration's failure.

- `add_sketch`, `set_sketch_plane` (`mcp/sketches.rs`): New sketch's
  command (`Document::add_sketch`, at the rollback marker, named as the
  UI names it) and Change plane's (`Command::SetSketchPlane`; the plane
  it's on already changes nothing, the editor adding no step), neither
  opening the sketch for the user (the UI's New sketch enters it). An
  origin plane by its word; a face by key through `key::found` on the
  committed model, then the plane pick's own check,
  `doc::face_placement_in` (`Doc::face_placement`, a click's on the
  model shown, now finds the face and calls it), with a `PlanePick`
  made of the committed model's marks (`new_sketch`, or `change` for the
  sketch moved), so a curved face, one made after the sketch, one too
  far out are refused in its words, prefixed `<tool>: plane:`. The
  client's doesn't set `Doc::placed` (the placement worked out at a
  pick, which only a session opened at once needs). The result
  (`Made::NewSketch`, `Made::Sketch`): `name` given is taken as a
  feature's is (`Document::new_name` in the `Command::AddSketch`, a
  clash's `note`); the sketch's name (`sketch`), id
  and plane (as `feature` shows it, `Shape::sketch_plane`).
- `add_link` (`mcp/sketches.rs`): the Project or Intersect tool's click,
  proposed to the solve lane as the user's link is, then committed as
  `Command::AddLink`. The sketch by name (a feature not a sketch
  refused); its source named as the tool names a click's, as of the
  sketch's place: a key through `key::found`, then
  `doc::outside_model_in` on the committed index with
  `Doc::naming_in(Some(sketch), committed marks)`; another sketch's
  curve or point by id (`modify::curve_in`, `sweep::point_in`), then
  `doc::outside_sketch_item_at` (both factored out of
  `Doc::outside_model` and `outside_sketch_item`, which the tool's
  clicks still run), so Intersect on a corner or a sketch's item, and
  what's made at or after the sketch, are refused in the tool's words,
  prefixed `add_link: from:`. What a link of the kind comes from
  already (by what its source finds on the committed model,
  `doc::outside_target_in`, as the tool tells a second click on it) is
  refused, naming the link: the tool's click would take that link out,
  which isn't what the client asked. Then checked as `propose_with`
  checks the user's (`SketchEdit::apply` on the committed sketch: at its
  turn nothing of the user's waits before it). These are `Plan::Propose`
  (`edit::plan`; the other tools are `Plan::Apply`, `commands`), so
  `client_edit_now`, after `clear_for_client`, hands it to
  `Doc::client_propose`: the call waits as `Wait::Solving` and
  `Proposals::propose_client` puts a client `Proposal` (`client:
  Some(tag)`) first in the queue (it's at its turn), then
  `send_proposal` sends it. See "Sketch edits through the solver" below.
  Its result (`Made::Link`): the sketch's as `sketch_made` gives it
  (name, id and plane, as `add_sketch`'s and `edit_sketch`'s), the
  link's id and kind, and the ids of the points and curves it made, as `sketch` lists
  them; those come with the next regeneration's relinking (folded into
  the edit's step), so the result is made again when regeneration
  answers (`Wait::Regen`'s `again`, merged over what it said as it
  applied).

- `edit_sketch` (`mcp/draw.rs`; arguments read in
  `varde_mcp::tools::draw`): a list of operations (`op`: `point`,
  `line`, `arc`, `circle`, `spline`, `rectangle`, `polygon`,
  `constraint`, `dimension`, `delete`, `move`), at most
  `MAX_OPERATIONS` (64), each list bounded (items `MAX_ITEMS`, a
  spline's points `SPLINE_POINTS`, the sketch's 100, a polygon's sides
  3..=64, a dimension's items 1..=2), labels at most `LABEL_BYTES`
  (64), unique, no `.`, not `origin`, `x_axis` or `y_axis` (which name
  the sketch's own). An item is an id (`id_in` on the sketch as the
  operations so far leave it), a label (the item it names alone: a
  point's, a line's, an arc's, a circle's, a spline's; a rectangle or a
  polygon has none) or a part of one (`l.start`, `l.end`; an arc's
  `start` and `end` are the points given so, whichever way it runs, and
  `center`; a circle's `center`; a spline's `point1`.., `start`, `end`;
  a rectangle's `side1`..`side4` from its corner along x and on round,
  `corner1`..`corner4`, `center` drawn from it; a polygon's `side`s,
  `corner`s, `center` and `circle`), still there. A shape's point is
  `[x, y]` (typed text or numbers, lengths within `MAX_COORD`, read as
  `make::pick` reads a profile's point) or a point by id or label, taken
  as the tool takes a snap to it (`edit::place`: the shape's own; the
  origin a new point coincident with it). Each operation is made on the
  sketch the ones before leave (applied, not solved), so an error
  names its index (`edit_sketch: operations[3]: ...` failed for
  arguments, ids and labels; `Not applied: operations[3]: ...` for
  what the sketch or the tool refuses). Shapes go through the drawing
  tools' own code: `doc::clicked` builds a `Drawing` from the clicks
  (`doc::Clicks`: the points placed with their snaps, the last click,
  a polygon's sides, a rectangle from its centre, a spline's kind and
  closing) and runs the tool's `step` (or `spline_add`, each point
  through the Spline tool's click refusals, `spline::refuses_point`), no values
  typed, the smallest shape `MIN_LENGTH` as a pixel. A constraint is
  `ConstraintKind::make` on the items as a selection (one that doesn't
  fit is refused, as the Constrain list wouldn't offer it); a dimension
  its `Measure` with the side `Sketch::held` gives, its value read by
  `Measure::ask` (the value field's bounds), driving at what it
  measures without one, or a reference (`driving: false`, no value);
  its side, and a value it measures, taken again by `held` on the
  solution its step is proposed on (`Step::measured`,
  `propose::remeasured`), as the planner's sketch has a moved point
  merely put there, the rest not following;
  a deletion `SketchEdit::Delete`; a move `SketchEdit::Move`, as a drag
  dropped. They're grouped into `doc::Step`s: shapes and constraints in
  a row one `Add` (each made on the sketch the last left, its
  placeholders going on from the group's, so they concatenate), moves
  in a row one `Move`, each deletion one, each dimension one alone (so
  `as_reference` takes it alone). Checked as it comes behind the
  sketch edits waiting on the solver (`edit::plan`'s `queued`) on
  `Doc::sketch_as_queued` (the committed sketch with every waiting
  proposal of it applied, by `propose::applied`, which
  `refresh_waiting` shares: a step in flight on its `from`, then the
  steps after it), made again at its turn on the committed
  sketch, which nothing waits before. `Plan::Propose` carries
  `sketches::Proposed { feature, what }`, `what` a link
  (`Proposing::Link`) or the steps (`Proposing::Steps`, with the
  sketch's `next_id` and `Made::Drawn`, its labels' ids). The result:
  the sketch's name, id and plane, `labels` (each label to its `id` and
  parts' ids), `degrees_of_freedom` (the last step's analysis), and,
  for dimensions placed as references, `references` (their operations'
  indices) and a `note` saying so. One undo step.

- `set_feature` (`mcp/set.rs`): the feature by name (a sketch refused:
  its geometry is the sketch tools'), its tool's `fields` checked
  first ("an extrude has no field "keep_tools": its fields are ..."),
  then null values (refused: give the value), then a page `feature`
  gave in place of a long list (`set::page_sent`: an object with
  `items`, `total` and `left_out`, as a field or in an object field such
  as `tool.profiles`; refused, naming the field, saying to leave it out
  or send the whole list), so a long feature read and sent back whole
  can't cut a list down to its page. A plain array is taken as the
  whole list, as any patch's is: refusing one that equals some page of
  the current list would refuse a client keeping just those on purpose,
  and the call is stateless (it can't know what the client read). A field equal to what
  `Shape::of` shows is dropped, so a feature read and sent back whole,
  unresolved references and all, is `NOTHING_TO_CHANGE`; with none left
  it is. One going with a field that changes is kept, as it's read anew
  with it (`set::GOES_WITH`: a new `sketch`'s `profiles` and `axis`, a
  new align `body`'s `from`). The fields left are read by the tool's parser (`tools::parse_patch`,
  its error under `set_feature:`) and taken, those
  given only (`make::Given::Only`), into the session opened on the
  feature as the panel's EditFeature opens it, but with the client's
  defaults for the values the feature has none for
  (`ExtrudeSession::editing_for_client`, `MotionSession::editing_for_client`:
  a through-all extrude made one side gets `CLIENT_DISTANCE`, an equal
  chamfer made two distances its second `CLIENT_CHAMFER`, a path sweep
  made a helix `CLIENT_PITCH`, never a length following the user's
  view; `Shape::of` reads through the same) (a combine's pruned as
  the panel prunes it) by the setters the creating tools share
  (`make::set_extrude`, `set_revolve`, `set_combine`: the creating tools
  are now a fresh session plus these with `Given::All`). Editing, the
  bodies, sketches and exclusions are checked against the feature's
  place, not the marker (`picked_bodies`, `sketch_named` and `excluded`
  take the feature edited or `None`, `Doc::edge_axis` with the session's feature; "made
  after the combine, which can only name what's before it"); a sketch
  other than its own needs `profiles` (the lost references dropped,
  `RegionPick::drop_missing`), a revolve's its `axis` too; a value of
  an extent the result doesn't show is refused ("second_distance goes
  with extent two_sides", "angle goes with extent one_side, symmetric
  or two_sides"), as the panel has no field for it; so is a `flip` with
  an extent that ignores it (symmetric, full, through all: "flip goes
  with extent one_side or two_sides"), and an `operation` other than cut
  for a through-all extrude without `extent` (the panel would make it
  one side unsaid). Keys go through
  the creating tool's paths, so a stale one is refused as there. The
  feature the session then holds is committed as
  `Command::SetFeature`, the panel's OK on an edited feature
  (`Doc::commit_feature`); the result (`Made::Set`) as an added one's.
  The thirteen motion-panel kinds go the same way (`set::motion`): a
  `MotionSession::editing` (the pattern's last shape from
  `Doc::pattern_shapes`), pruned as `Doc::edit_motion` prunes it, set
  up by the creating tools' own setters with the patch's `Given`
  (`motion::set_move`, `set_mirror`, `set_pattern`, `set_align`,
  `set_scale`, `modify::set_split`, `set_shell`, `set_offset_face`,
  `set_draft`, `blend::set_chamfer`, `set_fillet`, `sweep::set_sweep`,
  `set_loft`; each creating tool is `made`/`made_of` plus its setter
  with `Given::All`), then checked as `made_of` checks a new one
  (`MotionSession::built`, `Doc::motion_held_in`). A list given
  replaces the feature's own: bodies picked afresh
  (`picked_bodies`), faces and edges cleared on the session
  (`refs_body` after) before they're picked, as the clicks toggle; a
  split's `tool` clears its tile's tool and picks with the session
  picking its tool; an align's `from` or `to` clears that side's three
  slots (`AlignSetup::clear`), another `body` needing `from` with it;
  a sweep's `path` or `helix` clears its chains, stored edge parts and
  edges; a loft's `sections` and `rails` are cleared. A value of a
  mode the result doesn't show is refused in the creating tool's
  words, the mode the patch leaves it in (`Given::sent`: given for a
  patch, present or on for a new one): a scale's values of another
  mode, a chamfer's of another type and Flip with equal, a pattern's
  spread with full and Flip when circular, a sweep's Keep orientation
  and twist with a helix, a split's original unless both are kept, a
  shell's body while it has faces, a scale's `axis_only` kept from the
  feature for an `edge` given that isn't along a world axis. A pattern's `kind` can't change
  ("a pattern's kind stays as it is"), as the panel edits it as the
  kind it is. A patched pattern's shape isn't stored in
  `pattern_shapes` (nor is an added one's), so `feature` reads its
  mode back from what it stores.

**Sketch edits through the solver** (`add_link`, `edit_sketch`). A
`Proposal` carries `client: Option<u64>`, its call's tag (always `None`
on the web). A client's proposal, once made at its turn, is shown
waiting as the user's are (`Proposals::waiting`, which
`refresh_waiting` and `sketch_as_queued` read), so the user drawing
in the sketch behind it builds on its items rather than naming, by
the ids they were to get, what its commit takes first; a user's edit
made while the client's was still `Pending::Client` (unseen) is
refused at its turn if the committed sketch's `next_id` went past the
one it was made on and a client edit was applied since
(`Proposal::made_on`, `Proposals::client_edits`, `TAKEN_BY_CLIENT`; a
relink's amend alone moves `next_id` too, and refuses nothing). It isn't
the user's (`Pending::users`, `Proposals::users_in_flight`): Undo and
Redo go to the queue only for the user's, and `drop_newest` never drops
it, in flight or queued. It's sent on the committed sketch as any, and
proposed again if something was committed meanwhile. Answered,
`proposal_settled` takes the user's path as far as it's shared (a
`Rejected::Driving` dimension alone proposed again as `as_reference`
makes it, an edit in steps going on to its next step), then hands a
client's to `Doc::client_proposed` (through `client_proposal_settled`,
native only) rather than the user's session refusal, banner or toast:
accepted, the
commit `Doc::committing` builds (the user's path shares it: units
pinned, `AddLink` with the new link's source, else `SetSketch`) goes
through `client_apply`, the call then waiting on regeneration as any
edit; rejected or failed, the call is answered not applied with the
solver's words. A sketch gone before its turn (`send_proposal`) answers
*the sketch was removed before the edit's turn*. `Wait::Solving` holds
the calls behind it as `Queued` does (`Waiter::holds`, `unapplied`);
ended (the bound: `NOT_APPLIED_SOLVER`, *the sketch solver took over
30 s with the edit*; a cancel, the session's end, ...) it's not applied,
and `drop_ended_edits` takes its proposal out (`retain_clients` keeps a
client proposal only while its call is `Waiting::solving`; one in flight
is dropped, its answer ignored through `dropped`). `drop_proposals` ends
it with the queued ones (`mcp_end_queued`). The user may open the sketch
while it waits: it commits under them then, as a link's relinking does.

An `edit_sketch` goes in steps (`Proposals::propose_steps`): the
client `Proposal` carries `steps` (`Steps`: the steps after it, the
operations of the one proposed, those whose dimensions became
references, and the sketch's `next_id` they were made on). Accepted,
`proposal_settled` proposes the next first in the queue on the
solution (`next_step`: `from` that sketch at the answer's revision);
the last accepted is committed as one `SetSketch` (`Settled`: the
command, the analysis's freedom, the references), one undo step. A
step `Rejected::Driving` (a dimension alone) is proposed again as
`as_reference` makes it, its operation noted, told in the result, not
toasted. Any other refusal answers *not applied*, prefixed with the
step's operations (`operations[0] to [3]: it would over-constrain the
sketch`). `send_proposal` answers *the sketch changed while the edit
was solved* (`SKETCH_CHANGED`) for a first step whose committed sketch
has another `next_id` than it was made on (its ids would name other
items), or a later step once the committed sketch is no longer the one
the first step was proposed on (`Steps::on`, compared by content: the
user's edit or Undo of that sketch, or a relinking that moved its linked
geometry, which the solutions carry old and the one `SetSketch` would
undo). Anything else in the design changing between the steps (another
feature edited or undone, links re-found to the same geometry) leaves
the steps going on from their `from`, whatever the revision. A sketch
removed meanwhile answers `SKETCH_GONE`. `Wait::Solving` holds an `edit_sketch`'s `Made`,
which `client_proposed` completes from the `Settled`.

An editor refusal is worded by `doc::params::refusal` (the popup's
wording for parameters, the `EditError`'s own otherwise).
While anything waits on the solver (`Doc::proposing`) the edit queues
behind it as `Pending::Client { tag }` in
`doc/sketch/propose.rs` (the call itself waits in `Doc::mcp` as
`Wait::Queued` under the tag), so the undo order is the order things
happened. The user's own changes and panel OKs don't wait behind
it alone (`Doc::users_proposing`, not `proposing`): made at once, they
come before it in the history, as they came first, and it's made on
the design as it is at its turn. It's checked as it comes (a name not found, a refusal or
nothing to change answered at once, unless a client edit of any session
is still to be applied, `Waiting::any_unapplied`: it may make the name
this one gives), but its commands are made again in
its turn, from the call, on the design as it is then (`Doc::client_edit_now`):
what was queued before it may have removed or renamed its target or
moved a parameter's index; that check is skipped while the model trails
the editor's generation, its keys not yet resolvable. `send_proposal`'s
loop reaches it and hands it to `Doc::client_queued_turn`, which takes it
up only if its call still waits queued and hasn't waited `BOUND`
(answered `NOT_APPLIED_BUSY` if it has, `NOT_APPLIED_MODEL` with its model not in, though the tick that would end it
hasn't come yet). Taking it up (`Doc::client_turn`, also for one coming
with nothing on the solver, through `Doc::client_take_up`) waits for the
model (see "Waiting for the model", which also has how one at its turn
holds the queue until then) unless it's ready; then it's made and
applied at once (`Doc::client_edit_now`) and `Doc::sync` runs.
One the app takes up already past `BOUND` (its time queued before
counted) isn't applied at all: `NOT_APPLIED_LATE`.

**One at a time** (`notes/AgentSupport.md`, "Edits one at a time"). An edit
already waits behind every earlier client edit still to be applied
(`Waiter::behind`), so a session has one in flight; what bounds the
rest is `Waiting::edits_full`: the session's edits still to be applied
(`Waiter::unapplied`: those that `Waiter::holds` picks, plus those
waiting on the popup, `Asking`; one applied waiting on regen doesn't
count) past `QUEUED_EDITS`, 8, behind the one in flight refuse a new
one at once, `TOO_MANY_QUEUED`, its row not applied, failing none.
Eight lets a client send the next few steps of a plan (a sketch's
extrude, its fillet, a rename) without waiting for each answer, while
the popup may hold them; past that it's sending faster than the design
takes edits, and every one queued only grows what one refusal fails.

Queued edits fail together. Every edit ending *not applied* (or
failed: a name not found at its turn) ends as an `edit::Refused`
(`NotApplied` with the bare reason, formatted once by `Refused::answer`,
or `Failed` with its reply) through `Waiting::not_applied`: from
`Doc::edit_not_applied` (which then runs `drop_ended_edits` for the
proposals' queue) when refused by `commands`, read-only or the editor
(`client_edit_now`, `client_apply`, the checks on arrival in
`mcp_edit`) or by Keep editing (`mcp_keep_editing`); from
`Waiting::end_one` (`Wait::ended` gives `Err(Refused)` for any call not
applied) when late (`unless_late`) or ended through
`Waiting::end_where` (the bound, a cancel, the session's end, the
design closing, ...). It's answered, then the edits of its session
after it still to be applied are answered *Not applied: an earlier
edit of this session wasn't: <its reason>* (`Refused::reason`: the
bare reason, or a failure's text), at once, none
of them going on to fail others in turn (at the bound `end_where` ends
the calls one at a time, so those of a session timing out in the same
tick behind one not applied say it was the earlier one; the other ends
answer every call their own reason, the server stopping, say); a popup asking about one of
them shows cancelled. An edit applied, its regeneration failing or
not, or answering `NOTHING_TO_CHANGE`, fails none.

While an edit of a session waits on the popup, the session's queries
answered meanwhile carry `pending_edit` (`Waiting::answer`,
`with_pending_edit`): its tool and arguments as the panel shows them,
waiting for the user to let it apply, and what the user is doing. In
the structured content (and so the JSON text) of data answers, both
outputSchemas listing it; a line after the text of the others. It is
at most `waiting::PENDING_BYTES` (512) as JSON, the arguments and what
the user is doing each cut with an ellipsis (a 1 KB name in either
would otherwise push an answer at its budget past the cap).

Applying is `Doc::apply_client`, `Doc::apply`'s sibling: both go through
`doc::apply_as_one`, the same solve budget (`PROPOSAL_TIME`, shared by
the commands) and checks, the commands as one undo step (the first that
changes something applied, the rest amended into it,
`Editor::amend_within`); the client's made on a copy of the editor, all
or none; its `EditError` comes back to the
call (*Not applied: ...*), never to `Doc::edit_error`. A generation
unchanged answers `NOTHING_TO_CHANGE`. Applied, the call waits as
`Wait::Regen { generation, after, made }`: `made` the result's data
(`Made::result`: a renamed feature's name and id, a body's name, a note
with the name got when the one asked was taken; a body's name,
`visible`, `color` and `opacity` as drawn now; `removed` with the
features, bodies or parameter that went (names taken before, as they're
gone after); a parameter as `design_summary` lists it, `name` as
`parameter` (`query::parameter`); the units, tolerance and `rollback`
for the design's edits). `Doc::computed`, after
the feed takes an answer, has the links' amend move an edit's `after`
along (`Waiting::amended`) and answers the edits the answer reached
(`Doc::edits_regenerated`: `MeshFeed::answered`, the newest generation
applied, model or failure, at or past theirs): *applied, then undone by
the user* (`edit::UNDONE`) if `after` is neither current nor on the undo
history (`Editor::is_done`); else *applied* with a regeneration failure
(`MeshFeed::failure`), or the committed model's `failures` (feature
name and `FeatureFailure` message, not the draft's) and `not_built`
(the features from its `until` on), at most 50 listed. An answer that
was only a draft's says to see `design_summary`. Each applied edit's
row is applied.

Undo: while edits wait on the solver, Undo and Redo go to the queue
only when something of the user's own waits
(`Proposals::users_waiting`), and `drop_newest` passes over the client's
entries; with none of theirs waiting, Undo undoes the history and the
client's edit applies in its turn. New features go at the rollback
marker, as the user's do.

**Undo** (`mcp/undo.rs`, `notes/AgentSupport.md`, "The `undo` tool"):
`undo {steps}` takes back the session's own newest edits with
`Editor::undo`, once per step, so a link's amend goes with its edit and
ids wind back as for the user's Undo. An edit like the others: queued,
one at a time, read-only refused, behind the user's sketch edits on
the solver, the popup and the bound as any; it isn't checked on
arrival (`mcp_edit` skips the early check for it), only at its turn,
by `edit::plan` (`Plan::Undo`, through `undo::plan`) and again in
`Doc::client_undo` once `clear_for_client` closed what the user had
open. `undo::plan`: the session's newest `ClientStep` (its `start` and
`session` the call's) it hasn't undone must be the state now (through
`Waiting::amended_to`), else *the design changed since your last edit*
(`CHANGED_SINCE`: an edit, Undo, Redo or revert of the user's, a redo
of a step the session undid included, another session's edit); none,
`NOTHING_OF_YOURS` (a step it undid stays not its own once the user
redoes it). Then each of the newest
`steps` states of `Editor::history` must be such a step, else *only n
of the design's newest steps are this session's edits*, and have a
state before it, else *the undo history reaches back only n steps*.
All refusals are *not applied*, nothing undone. Applied, every step
goes in the one turn: each `ClientStep` taken back is marked
`undone` (never the session's to undo again) with `told_done` false,
and `Waiting::undone_to` records the state it made with the one now
(`Waiting::undos`, at most `STEPS`), both for `changes::since` (below);
no `ClientStep` is added for the undo itself. It waits as
`Wait::Regen` with `undid` (`waiting::Undid`: the afters undone, their
rows' words, the feature the newest named, from `ClientStep::feature`,
which `client_apply` now fills from the result's `id`), and answers as
an edit does: `undone`, the labels newest first (at most 20, each cut
to 200 bytes, then `{items, total}`), `result` (`applied`, or
`undo::REDONE` when the user redid any meanwhile, answered by regen or
at the bound; never *applied, then undone by the user*, as the user's
Undo since took back another step, not the undo), failures,
`not_built`, the revision. `Done::undid` carries it to the row.

**Never under the user's hands** (`mcp/ask.rs`). With the popup up
(not cancelled), an edit at its turn waits behind it (`Wait::Asking`, in
arrival order) before its commands are made (`client_edit_now`), as they
may name what the one asked about makes; so does one coming while the
popup is up (`mcp_edit`, the user's sketch edits waiting on the solver
or not), rather than queuing there ahead of the one asked about, which
queues only once answered, or being checked on the design before it.
Otherwise, once its commands are made (a name not found or nothing to
change is answered first, and nothing closes), `client_edit_now` hands
the edit to `Doc::clear_for_client`. With something unfinished open
(`Doc::unfinished`, which also gives the words after "you're"), the popup
asks (`Waiting::ask`, an `Ask`: the call's tag, what the user is doing,
the edit's tool and arguments as the log shows them, why Apply yours was
refused, whether it was cancelled), and the edit waits as
`Wait::Asking`. Unfinished means the rename field open (*renaming
Extrude 1*), a parameter being typed (`Doc::param_typing`), the sketch
with a value field, a label or a drag open or its tool started
(`Drawing::started`), or the extrude, revolve, combine or motion session
building another feature than it built as it opened
(`Doc::operation_changed`: `Doc::opened_with` is set by each opener,
`start_*` and `edit_*`, once its session is set up
(`Doc::opened_operation`) to `Doc::operation_built`, never by an opener
refused, which leaves the open session and its baseline as they were;
`Doc::operation_built` is the
`*_draft` kinds and the motion's `kind()`, kept rather than rebuilt as a
new extrude's distance depends on the camera). A field refused keeps the
value it read last, so it counts only when that differs. Otherwise
`close_for_client` closes what's open as Esc would: the operation
(`Doc::close_operation`, Esc's and Cancel's), the
sketch (`end_session`, as the caller syncs: the queued turn runs inside
`send_proposal`), picking a plane, the Timeline marker's drag, the
measure tool. Queries never ask.

The popup (`varde_view::McpAsk` in `McpShown::ask`, from the open
design's `Ask` and the client's name, the session's or the one gone;
`mcp::ask_popup`, a dialog over the whole document screen; for an
undo, `McpTool::Undo`, its title is *Claude Code wants
to undo its last edit while you're ...*, and cancelled *Claude Code's
undo was cancelled*) sends
`McpPanel::ApplyYours` and `KeepEditing` (also its Close once
cancelled), which `Varde` hands to `Doc::mcp_apply_yours` and
`mcp_keep_editing`. It's a `Dialog::McpAsk`, over the others in
`Doc::dialog`: no document shortcut works under it, and Esc sends
`KeepEditing`.
Apply yours (`finish_users`) commits the rename field and the parameter
typed (one refused: said), leaves the sketch, and commits a changed
operation with `Doc::commit_operation(false)`, its OK's (so a failing preview's OK isn't
taken); still open after, it says why (the editor's error, the failing
preview, the solver still busy, or not complete) and the call waits on.
Done, the popup goes, the edit asked about takes its turn (`edit_turn`:
taken up by `client_take_up`, answered `user_editing` past the bound) and so do those behind it, the
first finding something unfinished asking again. Those behind are taken
out of the table one at a time (`Waiting::asking` lists their tags,
`Waiting::take_asking` takes one still waiting on the popup), so one
refused at its turn fails its session's edits still behind it. Keep editing answers
*Not applied: the user is editing Extrude 1*. A call waiting on the
popup ended otherwise (the bound, a cancel, its session's end) answers
the same; the design closing or the server stopping answer theirs. The
call the popup asks about ending marks it cancelled
(`Waiting::end_where`): it says so with what the edit was, and only
Close, which lets those behind it go on; Apply yours then finishes the
user's work alone. A client edit coming while it shows cancelled
doesn't wait on it (`Doc::drop_cancelled_ask`, from `client_edit_now` and
`mcp_edit`'s queuing): the popup goes, those waiting behind it take
their turn first, in order, and the edit then takes its own, so with
something unfinished the popup asks about it, its buttons back; a
`client_edit_now` after any went makes its commands again, keeping its
place: first among what's queued on the solver if anything is
(`Proposals::wait_client_first`, as it was at its turn in
`send_proposal`'s loop, which reaches it again), else
`Doc::client_turn`. The popup is the design's, so it goes as the design
closes.

`body_topology` checks the call against the committed model
(`query::body_topology`, an error answered now) and asks regen for the
body's edge facts at the committed model's generation and `until`
(`Doc::ask_regen`, `MeshFeed::request_query`: a `Request::Query` of
`Query::Topology` tagged with the call's tag, with the document of that
generation, which the committed model keeps). `Doc::computed` (which
`Doc::answer` calls) hands `Response::Answered` to
`Doc::query_answered`, which finds the call by tag (an answer for a call ended meanwhile is dropped) and lists the
body (`query::listed`) only if the committed model still has that
generation and `until`. If it has moved on, the facts are asked again,
once, of the model now (the bound still from the call's arrival); moved
on again, the call answers `MOVED_ON` (ask again). The facts' error is
a tool error; facts that don't pair with the picking (see
`body_topology` below) an "internal error" answer, never a panic.

`measure` (`query/measure.rs`) resolves its items on the committed
model once it has caught up (`Wait::Model`, as the other queries): a
key through `key::found` (of this design, of its kind, what it names
in the model at its point; a stale one answered now with `a:` or `b:`
before why), a body by its Objects name, each turned into the
`InspectPick` the measure tool would send (the key's reference's body,
keys and point). It asks regen the same way, a `Query::Measure`
(`Doc::ask_regen`, the committed model's generation, `until` and
document), and waits (`Wait::Measure { generation, until, held }`);
`Doc::query_answered` answers it by tag (dropped for a call ended meanwhile). No asking
again: the answer says which generation it measured. Answered in the
measure tool's terms: per item its `kind` (`body <name>`, the view's
`face_kind`/`edge_kind`, `Corner`) and `values`, the view's
`measure_values` with labels in snake case (`area`, `box_size`, ΔX as
`dx`) and their shown text in the design's units; between two
`between_values` (`distance`, `dx`..`dz`, `angle`) or
`distance_error`; an item not found or not measurable as `error`; a
body a join merged into another, which regen measures as its holder
whole, with `merged_into` (the holder's name, from the committed
model's merges when asked, kept in `Wait::Measure`'s `held`) and a
`note` saying so; a `model` with `generation`, `current`,
`built_up_to` (and `rolled_back`), its earlier-state `note` saying the
values, not failures, may be out of date. Its row: `McpTool::Measure`, the measure tool's icon,
"Measured Body 1 to a face", the tooltip the distance and angle, else
the first value.

`render` (`query/render.rs`) resolves its bodies on the committed model
once it has caught up (`Wait::Model`): every body in the model shown by
default, else each by its Objects name, refused if none is named so or
it isn't in the model (hidden, not built, or merged by a join); none at
all is "nothing to draw". The camera is Home's (`Camera::default()`, the
iso view), turned to a named view (`Camera::look_from`), or to yaw and
pitch evaluated as angles in the design's units and parameters (the
pitch within ±90 degrees; yaw about Z from +X, front -90) through
`Camera::face`, in the projection asked. `varde_render::frame`
frames the asked bodies' parts within `size` square less an 8 pixel
margin, then the zoom is a factor on that framing (`PreviewShot::zoom`):
the view narrows by it about the bodies' centre, the camera kept where
the framing put it (`Camera::narrow`), so in perspective it never moves
inside them; below 1 shows more around them. Highlight keys go
through `key::found` (a face or edge on a body not drawn is refused,
with its index: `highlight 0: ...`) and `PickIndex::highlight`. The other
parts get opacity 0, the drawn ones their bodies' opacity and tint. The
call waits as `Wait::Render { drawn, data }` (`Doc::ask_render`), its
`PreviewRequest` for the viewport (see `viewport.md`, "Thumbnails"); the
request's callback sends the pixels on a oneshot the app awaits as a task
(`Waiting::take_renders`, `mcp_render` in `lib.rs`), coming back as
`ForDoc::McpRendered(tag, image)` to `Doc::mcp_rendered`, which asks the
IO lane to encode it (`Request::EncodePng` with the `DocId`'s number and
the tag, `files.md`) and waits as `Wait::Encoding { data }`;
`Response::PngEncoded` for this design goes to `Doc::mcp_encoded`, which
answers `data` (`bodies` drawn, the first `LISTED` 32 by name, each cut
to 64 bytes so the text stays within the cap however many there are,
`body_count`, `projection`, `size`, `highlighted`, `model`) with the PNG as image content, or refuses one past
`IMAGE_BYTES` (ask for a smaller size). A picture that couldn't be
drawn (its callback dropped uncalled: `render_preview` failed) answers
`DRAW_FAILED`; at the bound a call still waiting for its frame answers
`NOT_DRAWN` (no frame in 30 s: the window minimized, or no GPU to draw
with), one encoding `TOOK_TOO_LONG`. Answers for calls ended meanwhile
are dropped. `changes::attach` and `with_pending_edit` keep the PNG as
they rebuild a reply. Its row: `McpTool::Render`, the perspective icon,
"Rendered the model from iso", the tooltip the picture's size.

`sketch` reads the named sketch off the document (`query::sketch`, an
error answered now: no such feature, or one not a sketch) and asks the
solve lane for its degrees of freedom (`Doc::ask_analysis`:
`Request::Analyse` with the call's tag as its `query`, the committed sketch
and `Editor::revision`), queued in the lane's `Order` apart from the
user's analysis, so neither replaces the other. The call waits as
`Wait::Analysis { data }`, the listing read so far; `Doc::solved` hands
`Response::Analysed` with that `query` (or `Failed` with `Tag::Query`) to
`Doc::sketch_analysed`, which finds the call by tag (an answer for a
call ended meanwhile is dropped) and completes it (`query::analysed`).
Without a solve lane yet the call fails (`NO_SOLVER`); a lane replaced
(`Doc::lane_replaced`) ends the calls waiting on it
(`SOLVER_RESTARTED`, `Doc::mcp_end_analyses`).

### The panel

`varde_view::McpShown` (`view/src/mcp.rs`), from `Mcp::panel` through
`Varde::mcp_panel_shown` into `DocumentState::mcp` (`None` on the web,
so the web build shows none of it), drives three things
(`viewport.md` has where they sit): the status bar's chip while a client
is connected and the feature is on (`McpShown::chip`), the toolbar's
toggle while the feature is on (`McpShown::on`, `toggle_shown`), and the
panel itself while the feature is on and `Mcp::open`.
`Message::McpPanel` (`varde_view::McpPanel`) is the toggle (the chip
and ✕ too; it doesn't open the panel with the feature off), Start
(`Mcp::start`) and Stop (`Mcp::stop`); Disconnect is the settings'
`DisconnectMcp`.

The log is `Mcp::log`, a `Logged` per call (with the user's changes
its result told, below): its tool
(`varde_view::McpTool`), its arguments as typed text (`args`: names as
given, filters as `+z` or numbers, a page only where it isn't the
default, a `set_feature` as its feature and the fields it patches, `McpTool::SetFeature`;
a creating tool's `name` first, "Base: Sketch 1, all, 20", and its
body names as "making ..."), when it came, its state (`McpCallState`: waiting, answered,
failed) and a summary of what came of it for the row's tooltip
(`summary`: the listings' totals, a feature's kind and failure, an
edit's result, name and failures (`edit::summary`), or the error, cut
to 300 bytes). An undo's row is `McpTool::Undo` ("Undid:", however many
steps), the undo icon, its args "its last edit"
or "its last 3 edits" until applied, then (`undid_row`, from
`Done::undid`) the edit's row words lowercased ("Undid: renamed
feature Extrude 1 to Base") or "3 edits", the tooltip listing them
newest first; its `revert::Edited` keeps no states and the afters it
undid (`undid`), so `McpEdit::undo` gives a menu of Show in Timeline
alone, and it greys once the user redoes any of them. An edit's row (`McpTool::Rename`) says its state in
words: `McpCallState::Applied` or `NotApplied`, `Failed` for one whose
arguments didn't fit the design. The client's name (a blank one is none), the
arguments and the summary go through `plain` first: line breaks and
other controls become spaces or U+FFFD, and so do bidi marks,
overrides and isolates, so text off the network stays on one line and
can't turn the chip's or a row's text around. A call is logged waiting as it comes and
marked as it's answered (at once, for queries); one answered by a
cancel, a session's end or a restart is marked failed with that error.
At most `LOG_CAP` (200) rows, the oldest let go and counted
(`dropped`, a note above the rows): the panel builds a row per call on
every view; the edit rows' designs are bounded apart (see "Revert").
The log is the session's and the design's: cleared as the next
session starts, as the server stops or restarts (`restarted`), and as
the design closes (`Varde::show`, `Mcp::design_closed`); as a session
ends its rows stay, with a note that the client disconnected (`gone`).
The user's own edits are rows of their own (`McpRow::User`, *You:*
and the line), from the same diff the client is sent (see "The user's
changes"): each `Logged` keeps the lines its result told (`told`),
shown as a faint *Told: since its last call, you ...* line under it
(`McpCall::told`, cut to 300 bytes), and the rows before its own from
`told_rows` (`changes::Changes::panel`: the same but for the reverts
made from the panel, worked out again with each revert's revision a
step of its own, as the client's are, and dropped, so neither the
*reverted to ...* line nor what it changed is a row; the revert's own
row says it, below); the changes
since the last call show after the last row as they're made
(`Mcp::live`, worked out again in `Mcp::sync` only while the panel is
open and when the design's revision moved, `changes::Seen::is_now`). At most `changes::LINES` (20)
rows per call, the rest counted in one more. Text goes through `plain`
as the rest does. Greyed (`McpRow::UserReverted`) when taken back, as
the client's rows are (below): each `Logged` keeps the revision its
lines were worked out on (`told_at`: an edit's before, else the design
at the tell), and they grey while that state isn't in effect
(`Doc::in_effect`, below).

### Revert

`mcp/revert.rs`. Each applied edit's row keeps `revert::Edited`: the
design before it (`Editor::state()` taken in `Doc::client_apply` before
the commands, carried in `Wait::Regen::before` and `Done::edit`) and
after it, looked up by its after-revision once regeneration answered
(`Editor::state_of`, so a link's amend is in it), the after-revision,
the feature number its result names (`id`, for Show in Timeline) and
`not_told`: its reply wasn't taken (`Reply::send` now says whether) or
its session wasn't the open one, shown *Applied (client not told)*.
Bounded by `revert::SNAPSHOTS` (50) distinct documents (`Arc::as_ptr`)
over the whole log, the oldest rows letting their before, then their
after, go first (`Mcp::bound_snapshots`): each is a whole document,
free while the undo history holds it but the row's alone once the
history let go of it, and as an edit's after is the next one's before
50 is about the last 50 edits.

The row's context menu (`McpPanel::OpenMenu`/`CloseMenu`, `Mcp::menu`,
`view::mcp::edit_menu`): Show in Timeline (`Doc::mcp_show_feature`:
Timeline tab, `Look::SelectFeature`, and `Waiting::scroll` taken by
`Varde::update` into a `snap_to` of `TIMELINE_LIST`; gone, a toast says
so), Revert to here (offered while the after is kept, of the lineage
now, and isn't the design now, `Doc::shows_state`: its revision through
the amends, its document, or a revert to it with nothing since) and
Revert to before it (the before kept, of the lineage now). One that
would change nothing (`Editor::restores_nothing`) says so in a toast
(`revert::AS_IT_WAS`). A revert that would undo later changes
(to here always; before it unless the design is still its after) asks
first: `Waiting::revert_ask` (`Dialog::McpRevert`, Esc cancels;
`McpShown::revert`, shown by `ask_popup`'s `revert_popup`) lists the
differences, `changes::differences`, the diff of the design now to the
state, no author, at most 12 shown, worked out at the design's revision
kept with it (`RevertAsk::at`): the client edits on while it's up, so a
confirm on a design changed since works them out again and asks again
rather than reverting past what it didn't list. Revert/`ConfirmRevert` restores it
through `Editor::restore` (one undo step, each id counter, the
document's and each sketch's, the largest over the state and every state
of the lineage on the history, as a sketch removed since took its
counter out of the design now; `Doc::sync` keeps or drops
sessions by the lineage, as after an undo or a Replace) and logs a
`revert::Reverted` row (`McpRow::Revert`, *You: reverted to before
“...”*) between the calls, its tooltip (`tip`) the same words then what
it changed, `changes::differences` of the design to the state worked
out just before, a line each, at most `revert::TIP_CHANGES` (10), then
*and N more*, at most `LOG_CAP`, and its revision, the
state's and its words (`revert::Made`) in `Waiting::reverted` for
`changes::since` and greying, which names it in
`user_changes.reverted` (and a *reverted to ...* line) besides the diff.
Refused with a toast (`Doc::revert_refusal`): read-only; a client edit
between its arrival and applied or ended (`Waiting::any_unapplied`,
`revert::EDIT_IN_FLIGHT`), and a client's undo until it ends, waiting
on regeneration too (`Waiting::undoing`); the user's sketch edits with the solver; a
state of another lineage (`revert::REPLACED`). Checked again on
confirm.

Greyed, *Reverted*, worked out on each view, not stored, when the
state the row stands for isn't in effect (`Doc::in_effect`): an edit's
after-revision (through the amends, `Waiting::amended_to`), the user's
rows' `told_at`, a revert's own revision. By revision, which the editor
gives out in order (`Editor::made_before`): the newest revert in effect
made after the state decides (it brought back that state: in effect;
one older: not; one newer: asked again from there, of the reverts up to
it); with none, in effect unless undone (`Editor::is_undone`) or gone
with a branch undone past it (`Editor::is_gone`; one the history let go
of as too old counts as in effect). So an Undo of a revert ungreys what
it took back, a revert of a revert ungreys what the first took back,
and an edit undone then edited past stays grey. An edit's result
waiting on regeneration says *applied, then undone by the user* by the
same test, so a revert past it while it waited counts.

### The user's changes

`mcp/changes.rs`: each result of the session open (queries, edits,
refusals alike) carries what the user changed since the session's
previous call, `user_changes`, and the design's `revision`
(`changes::attach`, in `Mcp::sync` as each reply goes: in the
structured content of data, where the three outputSchemas list both;
else as `user_changes: {json}` and `revision: n` lines after the text,
only when there are changes). Calls of a session ended or replaced carry
neither.

`Mcp::seen` (`changes::Seen`) is the design as the previous call left
it: the `DocId`, the `Arc<Document>`, its revision and lineage and the
revisions of the history then (its ancestors). Set at a session's
`initialize` (`Event::Started`) to the design open then, so a new
session starts clean (none open: its first call sets it, telling
nothing), and after each result that carried them (`changes::fitted`:
the full form if the reply then fits `ToolReply::CAP`, else the brief
`{"bounded": ...}` saying to query, else none, the seen state kept so
the next result tells them; a reply cut anyway can't carry them).
`Mcp::tell` only works out what a reply carries; `Mcp::told` moves
`seen` and the steps' `told_done` on once `Reply::send` says the reply
was taken, so one whose connection gave up leaves them for the next.
Taken means the server took the reply, not that the client read it: if
the client dropped the connection while the call waited, the user's
changes that reply carried count as told and aren't reported again.
A cancelled call's reply (`Event::Cancelled`, through
`Mcp::sync_untold`) carries none, as its client ignores it, and its
edit's row counts as not told.
`changes::since` builds the changes:

- another `DocId` or lineage: `{"replaced": "the design was replaced:
  query it again"}` alone.
- the session's own edits the user undid or redid since it was told
  (`undid_your_edits`, `redid_your_edits`, by tool name and arguments,
  `waiting::label`; the panel's line says the edit as its row does,
  `McpTool::with`: "undid its edit: renamed feature Extrude 1 to Base"): `Waiting::steps`, a `ClientStep` per client edit
  applied (`Waiting::applied` from `client_apply`, its `after` moved by
  the links' amend in `Waiting::amended`, at most 200), each with
  `told_done`, which `Mcp::told` sets for the session's as it tells.
  A step the session took back with `undo` has `told_done` cleared
  then, so it isn't told as undone; the user's Redo of it is told as
  `redid_your_edits`.
- `Arc::ptr_eq` with the design now: nothing more.
- the seen revision, followed through the links' amends since and the
  session's own undos (`changes::followed`: `Waiting::undos`, while the
  state seen is still undone; the diff then starts from the state the
  undo came to, `Editor::state_of`)
  (`Waiting::amends`, recorded by `Waiting::amended`: a reply sent before
  the regeneration that relinked the state it saw; then walked from the
  amended state, the relinking not the user's), on the editor's history
  (`Editor::history`, the
  undo states then the current one, with their revisions): walked
  from it, the client's steps (any session's, by after-revision) split
  it into runs of the user's steps, each diffed alone, so the client's
  edits aren't reported back. A thing changed in two runs is one entry
  (`merge`: features by id, bodies and parameters by the name the
  earlier entry ends at): each `old` kept, each `new` taken, a field back
  where it was (a flag changed twice) dropped, and the entry with it if
  nothing's left. A field a client's step moved between the two runs
  (the later `old` isn't the earlier `new`, a flag set the same way
  twice) is said from where that left it, the later change alone.
- not there (undone past it): the two are diffed against the newest
  state on the history among the seen one's ancestors: an id names the
  same thing only if that state has it, so one made on each branch since
  (undo winds the counters back) is removed and added, not changed. When
  that state is the design now (undone back past it, nothing new since),
  that's no restriction; otherwise `note` says the user undid your
  edits, then edited (a step of the user's since that state, not only
  the client's). From that state the client's steps split the runs as
  above, the restriction on the first only. No such state (the history let it go): reported
  replaced.

A diff (`Said::diff`), by id: `features` `added` (kind, name, id, tool,
`settings` as `feature` gives them), `removed` (kind, name, id),
`changed` (name, id, and what changed: `renamed` old/new, `visible`,
`settings` with each differing field's `old` and `new`, by
`Shape::settings` (what `feature` gives) of `query::Shape::at` the old
and new documents, keys of the committed
model; a sketch summed up by its counts and plane, `geometry: changed`
when only its geometry did); a feature whose kind changed under its id
is removed and added. `bodies` added and removed by name, `changed`
with `renamed`, `visible`, `color` and `opacity` in `set_color`'s and
`set_opacity`'s terms. `parameters` by name (one renamed in place:
`renamed`), `expression` old/new, added as `design_summary` lists
them. `units`, `tolerance` and `rollback` (the feature's name or null),
old and new. Each also a line for the panel ("renamed Hole 2 to Bolt
hole", "hid Body 1", "set w to 3 cm", "undid its edit: renamed ...").
Bounded: past `CHANGES_BYTES` (4 KB of JSON) only the names of the
features added, removed or changed are given (`features_changed`, at
most 100 and as many as keep it in the 4 KB, with `left_out`; then the
labels of the client's edits undone or redone dropped from the end,
counted in `your_edits_left_out`; then the brief form), with `bounded`
saying to query them.

### Keys

`mcp/key.rs`: faces, edges and corners are named by opaque keys, the
postcard of `Key { doc, generation, item }` in base64url without
padding: the `DocId`'s number, the generation of the model it was read
from, and a `Ref` (a `FaceRef`, an `EdgeRef`, or a corner: its body,
three face keys sorted as `Picking::corner_keys` gives them, and its
point). A key from another design or generation can so tell. `decode`
refuses text over 256 bytes before decoding and takes nothing trailing.

`key::found` takes a key back as the `ItemKind` its argument wraps it
in: refused, saying to ask `body_topology` again, if it isn't a key, is
of another design, or the committed model (a `PickIndex` built of it)
has nothing its reference finds by its names (`PickIndex::find_face`,
`find_edge`, `find_vertex`, as regenerating finds a reference, so a key
of another generation is found again); one naming another kind is
refused ("that key names a face, not a corner"). Whatever its
generation, what's found is taken only if the key's point is finite,
within `MAX_COORD`, and on it within the design's resolution (and the
mesh's `f32` rounding): a face's triangles (`PickIndex::face_distance`,
which `find_face` ranks by), an edge's polyline (`edge_distance`), a
corner's exact point (its vertex's picking corner, `vertex_corner`):
that point is what the feature stores, and a key's text is the client's
to forge. Names and keys are resolved only once the model has caught
up with the design (see "Waiting for the model"), so a committed model
being rebuilt (`query::rebuilding`, as `body_topology` tells it) or none
yet is reached only as a fallback, refused to ask again shortly
(`query::committed` gives the model or says why not: `NOT_BUILT`,
`REBUILDING`).
Names and keys are resolved against the committed model throughout,
never the model shown: right after `clear_for_client` closes a panel the
model shown is still its preview until regeneration answers, whose
merges and failures (an earlier combine or join being edited) aren't the
design's. The click paths read the merges, touched lists and failures of
the model their index is of (`MeshFeed::marks_of`), and the body names
(`make::picked_bodies`, `make::excluded`, a shell's only body through
`Doc::only_body_in`) take `make::committed_marks` (merged through
`make::committed_merges`), which says the model
isn't built yet, or is being rebuilt, rather than fall back on the shown
one. It comes
back as `Found`: the index, the reference, and a `Pick` as a click on it
picks it (a face or edge at the key's point, a corner as its vertex with
its snap point), for the panels' click paths. `key::edge` is it for an
edge (the revolve's axis). The key holds no summary of what it named,
so "the same summary" is checked as the same item (its faces' keys) at
the same place; what a panel takes of it (a revolve's edge straight and
in the plane, a mirror's face flat, ...) is checked after.

### The queries

`mcp/query.rs` and `mcp/query/`, answered from the open `Doc` once the
model has caught up (see "Waiting calls"), checked then, not at arrival. Values
are shown as the UI shows them, in the design's units
(`varde_expr::Shown::current`); a feature's settings as their typed
text. The model is the committed one, `MeshFeed::committed` (what it
holds is in `viewport.md`). `model_state`
reports its generation, whether it's of the design as it is
(`current`, with a note if not: regeneration still running), and how
far it was built (`built_up_to`: before the rollback feature, or the
end of the Timeline).

- `design_summary`: units, tolerance, revision, the bodies (name,
  visible), the Timeline (kind, name, id, a failure from the committed
  model, `built: false` past where the model was cut short), the
  rollback feature's name, the parameters (name, expression, value or
  error, as the Parameters tab shows them) and `model`.
- `feature` (`query/feature.rs`): the named feature's kind, `tool`
  (`tool_of`, the tool that makes it; none for a sketch), id,
  visibility, failure and settings in its panel's shape, which is its
  creating tool's arguments (`Shape::of`), so a client can read one,
  change a field and send it back with `set_feature`. Values are the
  text the panel's fields show, read off a session opened on the
  feature as EditFeature opens it (`ExtrudeSession::editing`,
  `RevolveSession::editing`, `CombineSession::editing`,
  `MotionSession::editing` with `Doc::pattern_shapes`; now
  `pub(crate)`, as is `MotionSession::field`); choices in the tools'
  words, only those the panel shows for the mode (an extrude's
  `second_distance` with two sides alone, a pattern's `flip` when
  linear, a split's `original` while both are kept, a shell's `body`
  without faces, exclude without new body, a chamfer's and fillet's
  `tangent` the panel's tick, on as it opens); sketches and bodies by
  name; regions by index in their sketch's (`Profiles::resolve`); a
  sweep's path chain and a loft's rails by their first curve; faces,
  edges and corners by key of the committed model: the stored
  reference encoded at the committed generation and checked by
  `key::found` (tried again as the body a join merged it into), as a
  key sent back would be. One that can't be named (a body or sketch
  gone, a region its sketch no longer has, a reference the committed
  model hasn't: rolled past, failed, or moved since, as an align's own
  corner once the body is aligned) is `{"unresolved": why}` in its
  place. A sketch is summed up by its plane (a word or `{"face": key}`)
  and counts. Each list in the settings (at the top, or in an object
  there: a split's `tool.profiles`, a sweep's `path.edges`) is paged
  alone (`feature::paged`) and shortened to fit, as every listing is
  (`query::pages`, below), so 1 KB names fit too. A list given whole (offset 0, all of it) stays a plain array, so
  a small feature reads exactly as its tool takes it and round-trips;
  one that isn't is a page `{items, total, left_out}` (with `note` past
  the end) and the answer gets a `note` naming the lists paged and
  saying not to send a page back as the list.
- `body_topology`: the body's faces, edges and corners in the committed
  model, read off its picking tables (`Picking`, described in
  `viewport.md`). A body hidden or not built, or a model not built or being
  rebuilt, is an error saying so. Faces come from the picking tables
  with their form and its facts (`Summary`), area and bounds summed
  over the mesh's triangles, and a key whose point is the largest
  triangle's centroid; edges between two faces (form, ends or closed,
  convexity from the body's `EdgeFacts`, which a `Query::Topology`
  answers apart, see `viewport.md` and "Waiting calls": `facts[i]`
  pairs with edge `part.edges.start + i`, checked as `facts[i].faces ==
  picking.edge_keys(mesh, e)`, every edge past them a crease) with
  length along the polyline, their two faces' keys, and a key from
  `Picking::edge_keys` at the longest segment's middle; corners of the
  body from `Picking::corners` with their exact point. Key points are
  clamped within `MAX_COORD`. Filters: `faces.normal` keeps flat faces
  within half a degree (`FILTER_COS`) of it, either way if unsigned;
  `edges.direction` keeps straight edges along it either way, whatever
  its sign; `edges.convex` keeps convex or concave edges, smooth and
  unknown ones passing neither. Points and boxes are bare numbers in
  the design's units, lengths and areas typed text; the answer notes
  that mesh measures are approximate and, rolled back
  (`model.rolled_back`), that the features from the rollback on aren't
  built.
- `sketch` (`query/sketch.rs`): any sketch by its Timeline name, open
  or not, read off the committed document: its name, id, units, plane
  (a word or `{"face": key}`, as `feature` gives it), revision; its
  points (id, name, `at` as bare numbers in the design's units), curves
  (id, name, kind in lower case, `construction`, and by kind a line's
  `start` and `end`, a circle's `center` and `radius` as typed text, an
  arc's `center`, `start` and `end`, a spline's `points` and `closed`),
  constraints (id, `kind` in snake case, `items` the ids it ties),
  dimensions (id, `measure` in snake case, `items`, `value` its text as
  typed, `driving`, `measured` as the UI shows it), and its regions
  (`Sketch::profiles()`, in that order, each with its `index`, which
  `extrude`, `revolve`, `split`, `sweep` and `loft` take, as `make::pick`
  indexes the same profiles; its `area` as typed text, a point
  `inside` from `Region::inside`, now public, and its `outer` curves and
  `holes`, ids sorted, at most 50 curves a loop and 50 holes, the rest
  counted in `outer_left_out`, `holes_left_out` and
  `hole_curves_left_out`, so a page of one region fits the cap however
  many curves its loops have; a sketch too complex for its regions says so in
  place of the listing). Then, from the solve lane's client analysis
  (see "Waiting calls"), `degrees_of_freedom`, `solved`, and the
  `redundant` constraints and dimensions if any (at most 50, the rest
  counted in `redundant_left_out`). Each of the five listings is paged
  alone.

Each listing is paged alone (`page`): `items` from `offset`, at most
`limit`, with `total` and `left_out`, and a note when the offset is past
the end (`query::is_page` knows one, for `set::page_sent`). The defaults
are sized so a page of each listing fits under `ToolReply::CAP` (tested
for `body_topology` on the example's plate, and for `sketch` on one of
forty constrained, dimensioned squares). Past that, `query::pages`,
which `design_summary`, `feature`, `body_topology` and `sketch` all
build their answers through, shortens the pages, the one with the most
bytes first and never below one item (each item's bytes measured once,
as many cut at a time as the answer is over), until the answer's JSON
is within `query::BUDGET`: the cap less `changes::ROOM` (512 bytes,
tested to hold the brief `user_changes` and the largest revision) and
`waiting::PENDING_BYTES` (and its key) for a `pending_edit`, and
for `sketch` less 1 KB more for its analysis. No room is kept for the
full `user_changes`: `changes::fitted` falls back to the brief form when
they won't fit, as for any reply. An answer whose single items are still
over is cut as described above.

### Tests

`mcp/tests.rs` drives the queries on the example design with its regen
lane deferred and answered (`answer_failing` adds a failure; the
tests' own `answer` answers `body_topology`'s facts through
`varde_regen::handle`, and `respond` answers a `listening_with` app's
regen requests through `Varde::update`, for the waiting calls: answered,
timed out by a `Message::Tick` past the bound, ended, asked again, facts
spoilt; waiting for the model: a query behind a user's proposal seeing
it committed, one during regen answering its failures, one after a
client's edit waiting seeing it, `body_topology` asking for its facts
once the model is in, a query and an edit timed out, an edit's key
resolved once the model caught up, the user editing on while a query
waits, a query not held by another session's edit; the edits: applied and answered after regen, nothing to
change, read-only, queued behind a user's proposal in undo
order, made on the design of its turn, timed out while queued or
reaching its turn past the bound before the tick, regen slow, Undo passing over the
client's entry, undone before regen or before the bound, a rename
clashing, failures reported; each session kind and the plane pick,
marker drag and measure tool closed by an edit when clean, the popup for
a changed extrude, a started sketch tool, the rename field and a
parameter typed, Apply yours committing first (and an edit behind it
applied after, in undo order) or refused saying why, Keep editing, the
popup cancelled at the bound or by the client and closed with the
design, an edit taking over a cancelled popup, one at its turn past a
cancelled popup keeping its place before the user's next sketch edit,
queries never asking; one at a time: a session's edits past the bound
refused, an editor refusal, Keep editing and a timeout failing the
session's edits behind, an applied edit failing regen failing none, a
query during the popup carrying `pending_edit`, cut to its bound for
an edit of 1 KB of quotes; each tool through `edit`, read from JSON
by `parse_call`: applied, refused with why by the UI's checks and the
editor's (a parameter taken, not a word, or leaving the extrude's
distance in error), and its values' bounds), and feeds
calls through `Varde::update` as `Message::Mcp(Event::Call)`, with
`SessionId::for_tests` and `Reply::for_tests` (hidden from docs) to make
the event and read the reply off a channel; `mcp/tests/motion.rs` the
motion tools (each one's defaults and values given, keys by kind, a
face's or corner's key off it or stale after the plate is made
symmetric, values past their bounds, what only the panel refuses);
`mcp/tests/modify.rs` the split, shell, offset face and draft tools
(each tool and tile, defaults and values given, keys of another kind,
off their face or stale, faces of two bodies or named twice, a
draft's neutral plane among its faces, Keeps for a trim, a body made
after the marker); `mcp/tests/blend.rs` the chamfer and fillet tools
(defaults, each type and value, faces, a value of another type, Flip
with Equal, edges or faces of two bodies or named twice, keys of
another kind, off their edge or stale, values past their bounds, an
edge of a body after the marker, and on a slot beside the plate the
tangent chain taken in, each edge of its own, off with the tick, and
another edge of a chain refused); `mcp/tests/naming.rs` names given to `add_sketch`, `extrude` (feature
and body, undone in one step), `mirror` and `pattern` (copies' names),
clashes numbered with notes, body names refused where none or fewer
are made, an empty name refused by the app and an empty or oversize
one by the parser, and `set_feature` refusing names;
`mcp/tests/set.rs` `feature` and `set_feature` (each of the sixteen
kinds added by its tool, read in its tool's shape and parsed by it
where nothing is unresolved, and sent back changing nothing; a field
patched per kind taken, one undo step, for each motion-panel kind
too, the rest read back as it was; a fillet's edges and a move's
bodies replaced, not toggled; the refusals, a mode's among them; an
edge key in a revolve's and a move's patch taken, then stale; paging: a chamfer of 256 edges read page by page, each reply within the cap, its note, the page sent back refused; a combine's seventeen tools of 1 KB names shortened to fit; a short list read whole, a plain array); `mcp/tests/sweep.rs` the sweep and
loft tools (a path of a sketch's chain or model edges with its
options, a helix about Z or a round face, the profile's own sketch, a
chain's curve named again, a path's options with a helix, a flat face
as the axis, values past their bounds, a sketch after the marker; a
loft's sections in the order given with a start and a point, Ruled,
Closed, rails, and one section, one named twice, a region with holes,
a point in the middle or between points, a start off its corners,
rails with Closed, a closed rail, a rail named twice);
`mcp/tests/render.rs` `render` with the IO lane kept and the
viewport's frame stood in for (`ForDoc::McpRendered`; no GPU needed):
the request for the next frame (the committed mesh, framed to 768 from
Home, in `DocumentState::renders`), the PNG asked of the IO lane and
answered as image content; a named view, perspective, yaw and pitch and
zoom framing as asked; unknown and hidden bodies, bad angles and keys
refused, a highlight drawn selected; a picture not drawn, one past
`IMAGE_BYTES`, another design's answer dropped, and no frame within the
bound (`NOT_DRAWN`). The framing math is `varde-render`'s
(`preview/tests.rs`, and a GPU test in `tests/viewport.rs` of parts left
out and a selection drawn), the image content `tools/tests.rs`'s;
`mcp/tests/sketch.rs` `sketch` with the tests' `SolveLane` (the
example listed with its degrees of freedom, a region's index and point
from it picking the region in `extrude`, the client's analysis and the
user's both answered, waiting for the model first, paging and the cap,
the bound, a failed analysis, a lane replaced, no lane, a name not of
a sketch); `mcp/tests/sketches.rs` `add_sketch`, `set_sketch_plane`
and `add_link` (each origin plane, at the rollback marker, a flat face
by key, a curved face, a bad, wrong-kind and stale key, a plane changed
keeping the drawing and the same plane changing nothing, a face made
after the sketch, a link projecting an edge through the `SolveLane` and
answering its points and curves after relinking, one undo step, linked
already, another sketch's curve, the tools' refusals, the solver's
bound, the user's Undo passing over the waiting link; none opening the
sketch); `mcp/tests/draw.rs` `edit_sketch` with the `SolveLane` (a
rectangle by label taking a later dimension, one undo step and the
degrees of freedom; circles held equal by labels; every shape and its
parts; a point moved and deleted by id; a dimension placed as a
reference and said; an unsolvable edit not applied; bad labels, parts,
ids, kinds and values naming the operation; the user's sketch closed
or asked; an edit queued behind the user's, checked on their sketch);
`mcp/tests/undo.rs` `undo` (the newest edit taken back in one
history step, answered after regeneration, its row and the edit's
greyed, its menu, no user change told, the user's Redo told and
greying it; three steps newest first; refused past an edit of the
user's, after the user's Undo, after a revert, past or on another
session's edit, with nothing of its own, past the history's reach;
failing behind an edit refused on the popup; sent right behind its
edit, taking it back too, the edit behind it going after; refused at
its turn, failing the edit behind it; an amended edit undone with its
amend; the user's Undo meanwhile leaving it applied, their Redo before
the bound said; behind the user's sketch
proposal checked at its turn; the popup with a sketch's tool started,
Revert refused meanwhile; the 30 s bound on the popup);
`mcp/tests/changes.rs` the user's changes (each kind once with old and
new values, not again; the client's own edits left out; the user's
undo and redo of them; an id made again after an undo removed and
added; a design replaced; the bound; a new session starting clean; the
panel's rows and *Told* line); the panel's
tests there
check the chip, the toggle, the rows, the cap and the log's lifetime
through `Varde::update` and the screen headless, and
`view/src/mcp/tests.rs` the panel's own layout and the popup's. `varde-mcp`'s own tests
cover each module, the server's over real sockets with short timeouts
(`server::start_with`).
