# Threading

Goal: constraint solving, mesh operations (tessellation, booleans,
regeneration of the feature history) and file system access (open, save,
auto-save, recent files) run off the UI thread. Orbit, pan, zoom, camera
animation, panels and menus keep running at frame rate while that work is in
flight, on native and on the web build.

## Status

Plan steps 1-6 are built: a regeneration lane per open document and one IO
lane for the app, natively threads and on the web Web Workers, the IO lane
with saving, auto-save, recovery and the sidecar lock, over OPFS on the web.
`agents/` says how they work (`viewport.md`, `files.md`,
`web-files.md`). Of step 7, the sketch solver (`varde-sketch`), the
solver lane (`varde-solve`, a lane per open document, natively a thread and
on the web a Web Worker) and the app's side are built: every sketch edit is
proposed and committed once accepted, one in flight and the rest queued
(other changes queue behind them), dropped newest first by undo, waited
for by Save and closing; drags are sessions of the lane's, committed on
release as a proposed move; drag steps stay in the lane (see
"Dragging"). `agents/sketch.md` says how. Per-feature caching and
cancellation are still open, see step 8 of the plan; manifold isn't wired
in yet. The design below is
kept as it was decided, including for what's built: where the code differs,
the code wins.

Why the lanes exist: iced's executor is `thread-pool` (futures
`ThreadPool`) on native. On wasm it is `wasm_bindgen_futures::spawn_local`,
i.e. **the main thread**, so `Task::perform` moves work off the UI thread on
native but not on the web.

## Constraints

**iced.** State lives in `Varde` and is only mutated in `update`, on the UI
thread. Background work gets results back as `Message`s, either from a
`Task` or a `Subscription` stream. No shared mutable app state.

**wgpu.** The device, queue and surface stay on the UI thread (on the web the
WebGL2 context is bound to it). Workers produce CPU-side data (`RenderMesh`); the
upload stays in `Primitive::prepare`. Big uploads could later be chunked but
that's a render concern, not a threading one.
 
**wasm.** `wasm32-unknown-unknown` is single threaded by default. The ways
out:

- *Web Workers without shared memory.* A second wasm instance in a worker,
  talking by `postMessage`. Works on stable Rust and on any host. No shared
  state: everything crossing is copied, or transferred as an `ArrayBuffer`
  (zero copy, ownership moves).
- *wasm threads* (`+atomics,+bulk-memory`, `-Z build-std`, nightly) with
  `SharedArrayBuffer`. Same code as native (`std::thread` via
  `wasm_thread`, rayon via `wasm-bindgen-rayon`). Needs the page to be
  cross-origin isolated (`Cross-Origin-Opener-Policy: same-origin`,
  `Cross-Origin-Embedder-Policy: require-corp`), which restricts embedding
  and third party resources. And the main thread **may not block**:
  `Atomics.wait` throws there, so `Mutex::lock`, `Receiver::recv` and
  `JoinHandle::join` on the UI thread trap or panic. Only `try_*` and
  lock-free structures on the UI side.
- The C++ manifold (`manifold3d`) doesn't build for `wasm32-unknown-unknown`
  easily in any case. Either a Rust boolean kernel, or a web-only path. Out of
  scope here, but it argues for keeping the kernel behind a message boundary
  that doesn't assume shared memory.

## Design

### A compute boundary, not shared state

Put all heavy work behind one message interface. The UI sends *requests*
tagged with the editor generation, which grows with every change, undo and
redo included; the worker answers with *results* tagged with the same
generation. The UI never waits.

```rust
/// Sent from the UI.
enum Request {
    /// Validates `command` on top of the committed document at `base`, see
    /// "Only valid edits are accepted". One in flight per document.
    /// `until` stops the history before a feature, for editing a sketch.
    Propose { base: u64, document: Snapshot, command: Command, until: Option<FeatureId> },
    /// Rebuilds the mesh of a committed document, e.g. after undo or on
    /// entering a sketch. Supersedes any earlier request.
    Regenerate { generation: Generation, document: Snapshot, until: Option<FeatureId> },
    /// Interactive solve of one sketch while dragging a point.
    Drag { session: u64, sketch: Sketch, point: PointId, to: DVec2 },
    Cancel { up_to: u64 },
}

/// Sent back to the UI as a `Message`.
enum Response {
    /// The edit is valid. `solutions` are the sketches it moved; `mesh` is
    /// the document with the edit and the solutions applied.
    Accepted { base: u64, solutions: Vec<Solution>, mesh: Option<Arc<RenderMesh>> },
    /// The edit is invalid and was not applied.
    Rejected { base: u64, reason: Invalid },
    Regenerated { generation: Generation, mesh: Arc<RenderMesh>, sketches: Arc<RenderLines> },
    Dragged { session: u64, solution: Solution },
    Failed { generation: Generation, exclude: Option<FeatureId>, error: String },
    Progress { generation: Generation, done: u32, total: u32 },
}

struct Solution {
    sketch: SketchId,
    points: Vec<DVec2>,
}

enum Invalid {
    /// Over-constrained or inconsistent, naming the constraints involved.
    Sketch { sketch: SketchId, conflicts: Vec<ConstraintId> },
    /// The solver didn't converge from the current points.
    NoConvergence { sketch: SketchId },
    /// A later feature can't be rebuilt, e.g. a sketch that references
    /// geometry the edit removed.
    Feature { feature: FeatureId, error: String },
}
```

`Command` crosses to the Web Worker, so it becomes `Serialize`.

The crate `varde-regen` (no iced) holds `Request`/`Response`, the pure
`handle(Request) -> Response` logic and the lanes that carry them; the
`Transport` trait they're sent through is in `varde-lane`, see "Lanes".
`app` wires it into iced. Only `Regenerate` exists so far, with
`exclude: Option<FeatureId>` in place of `until`: it leaves the one sketch
out of the flattened sketch lines. `until` replaces it once features build
on each other.

### UI rules

- The `Editor` is the truth and only ever holds valid, committed states.
  Edits that can be invalid are committed once the worker accepts them
  (see below); the rest are committed at once as today. The mesh is a
  *derived, possibly stale* view of the committed document.
- The app's `MeshFeed` keeps the last completed mesh and its generation, plus
  the generation it has asked for. It holds the lane's transport and sends
  nothing until the lane has started, then a request only if
  `editor.generation()` is newer than the last one.
- A `Response` is applied only if its generation is newer than what's shown.
  Out of order or superseded results are dropped.
- Camera messages (`Orbit`, `Pan`, `Zoom`, `AnimationFrame`) never touch the
  compute path, so they stay smooth whatever the worker is doing.
- Status bar shows "Regenerating…" while shown generation < editor
  generation, and the error if the last result failed (keep showing the
  last good mesh).
- Undo/redo and further edits during a run just send a newer request.
- Things that need the *current* geometry (picking, triangle count, fit to
  view) use the shown mesh and accept it may be one generation behind.

### Latest wins, coalescing and cancellation

Edits can arrive faster than regeneration. The worker keeps only the newest
pending request (a single slot, not a queue), so a burst of edits costs one
run after the current one. Cancellation of the running job is cooperative:
long loops in the kernel/solver take a `&dyn Fn() -> bool` (or a
`CancelToken`) and check it between steps.

- Native: `Arc<AtomicU64>` "wanted generation"; a job whose generation is
  below it stops. Not built yet: today the slot only refuses a request older than
  one already sent, and a running job always finishes.
- Web Worker without shared memory: the worker can't see new messages while
  a synchronous job runs. Options: check nothing and let it finish (fine
  while jobs are short); `terminate()` and respawn the worker (tens of ms to
  re-instantiate wasm, loses caches); or split jobs into steps and yield to
  the worker's event loop between them (`await` a zero timeout) so a
  `Cancel` can land.
- Web with shared memory: same atomic as native.

### Only valid edits are accepted

An edit that over-constrains a sketch, can't be solved, or breaks a later
feature is refused, not accepted and flagged. So validity is checked
*before* an edit reaches the `Editor`, and the solver's point moves are
part of that same edit. The committed document is therefore always valid
and solved, which is what makes saving simple (see "Files").

**Edits that can be invalid are proposals.** Adding or changing a
constraint or dimension, deleting sketch geometry, or changing a feature
that later features build on is sent as `Propose { base, command }`, where
`base` is the committed revision. The worker applies it to its copy of the
snapshot, solves the sketches it touches (warm started from their stored
points), regenerates what depends on it, and answers:

- `Accepted`: the UI applies the command and the `solutions` as one
  `Editor` change: one revision bump, one undo entry. Constraints and the
  point moves they cause are undone together, and undo needs no re-solve
  since each snapshot holds solved positions.
- `Rejected`: nothing changes in the `Editor` and there's no undo entry.
  The UI says why ("Would over-constrain the sketch") and highlights the
  conflicting constraints or the failing feature. There's nothing to
  revert, since nothing was applied, so it can't race with other edits.

Edits that can't be invalid (visibility, renaming, adding an unconstrained
point, a new body) skip this and commit at once. Undo and redo too: they
move between committed states, which were valid.

**The worker never owns the document.** The `Editor` stays the only writer;
the worker only returns verdicts.

**Which lane.** An edit inside the sketch being edited is checked by the
solver lane. The 3D view shows the model before the sketch while editing it
(see "Dragging"), so nothing downstream needs rebuilding yet and the check
is a solve: a few ms round trip for typical sketches. Edits to features
(e.g. an extrude a later sketch references) go to the regeneration lane,
which rebuilds everything after the edit. Its `Accepted` carries the mesh,
so acceptance and the new model arrive together.

**Pending edits.** Between proposing and the answer, the edit is shown as
pending (e.g. the new constraint drawn faded, a "Checking…" status if it
takes more than ~100 ms). Camera and UI carry on. Edits are ordered, so
unlike the other requests this isn't latest-wins: one proposal is in
flight per document, further edits queue behind it, and each is proposed
against the committed state the previous one produced. If one is rejected,
the queued ones are still proposed and may be rejected too. Other changes
to the document made meanwhile queue behind them and are made once
they're answered, so the history keeps the order they were made in. Undo
while anything is pending drops the newest pending item, the natural
meaning of undoing an edit the user just made. A `base` that no longer
matches the committed revision can't happen with one proposal in flight,
but is still checked, and the proposal is re-sent if it does.

**Long checks.** Rebuilding a large history to validate an upstream edit
can take seconds. The edit stays pending for that long; the document is
never in a half-accepted state. If that feels bad, the alternative is to
accept feature edits that break later features and mark those features as
failed, as most parametric CAD does. Sketch edits are always checked.

**Opening a file.** A file only ever contains committed states, so it's
valid when written. It is still checked on open (a different build's
solver could disagree): regeneration solves every sketch warm started from
the stored points, which normally converges at once. A failure can't be
refused, since the file is what it is, so here the sketch or feature is
marked as failed, and edits that fix it are accepted.

### Dragging

Dragging a sketch point is latency bound and produces many intermediate
states that shouldn't enter history. So a drag doesn't write to the
`Editor` until it ends:

- Pressing starts a drag session in `Doc` (transient UI state, like the
  camera), holding the sketch as of the press and a session id.
- Each move sends `Drag { session, sketch, point, to }`. The worker solves
  from the last drag solution (warm start). Latest wins: newer drag targets
  overwrite each other in the slot, so a slow solve skips intermediate
  positions instead of queueing them.
- `Dragged` results update the session's preview points only. The viewport
  draws the preview: the dragged point under the cursor right away, the
  rest at the last solved positions, so dragging stays at frame rate even
  if solving lags by a frame or two.
- The preview only ever shows converged solutions. If a drag target can't
  be reached (a point dragged where the constraints don't allow), the
  preview stays at the last solution that converged.
- On release, the final target is solved, and the last converged solution
  is committed as one `Command::SetSketchPoints { sketch, points }`: one
  undo entry per drag. It was validated by the drag solve, so it commits
  at once, as long as the committed revision is still the one the drag
  started from. Built otherwise: the app can't tell which answer is the
  final target's (a step that doesn't converge isn't answered), so release
  proposes the move to the final target on the last converged solution, a
  short solve on the branch the drag was on, committed as one
  `Command::SetSketch` when accepted, with its analysis.
- Any other edit to the sketch during the drag (e.g. undo by keyboard)
  cancels the session. Escape cancels it too; nothing to roll back since
  the document was never touched.
- No live 3D updates. While a sketch is being edited, the 3D view shows
  the model as it is *before the sketch* in the feature history: the
  features the sketch may reference, not the ones built from it. A sketch
  can only reference earlier features, so there are no cycles, and nothing
  the sketch sees changes while it is edited. Entering a sketch sends
  `Regenerate` with `until` set to it; with the per-feature cache that
  prefix is usually cached already. During a drag only the sketch preview
  moves. Downstream features are shown again on leaving the sketch; their
  regeneration may start in the background after each commit so that's
  quick. That background work runs in the regeneration lane, so it never
  blocks drag solves, see below.

### Lanes

A Web Worker is a single thread, so a long regeneration would stall drag
solves behind it, and a slow save would stall both. Use three workers:

- a **solver lane**: drag solves only; small messages, carrying just the
  one sketch;
- a **regeneration lane**: `Regenerate`, which does its own solving as
  above;
- an **IO lane**: all file system access, see "Files".

Natively the same lanes are three threads. Solving the same sketch in both lanes is fine: drag
results never touch the document, and `Regenerated` solutions are checked
against `base`.

What a lane is, apart from its requests, lives in one crate, `varde-lane`:
natively the thread, whose pending requests are the lane's own (a slot or a
queue), which answers a request that panics and which drains or stops once
nobody listens; on the web the page's side of the worker (starting and
stopping it, the stream that posts requests) and the worker's (the panic
hook), the ready and panicked messages between them, and how bytes cross:
framed as transferred buffers and copied out within a bound. A new lane
brings its requests, their order and its wire format, not another copy of
the plumbing.

Small sketches will likely solve in well under a millisecond; it may turn
out simplest to solve drag steps inline in `update` with an iteration cap
and only fall back to the solver lane above a size or time budget. Measure
first, but keep the solver API pure (`fn solve(&Sketch, drag, cancel) ->
Solution`, warm starting from the sketch's own points) so either placement
works. Built so (`varde_sketch::solve(&Sketch, &Goal, &Budget)`, with
`Budget::expired` as the cancel): on a 342-curve plate a drag step takes
about 1.3 ms natively and 2 ms in wasm, see Performance in
`notes/SketchImpl.md`.

**Decided (step 2e): drag steps stay in the lane.** Measured natively
(release, 30 steps of a drag of a plate's corner, the next step sent once
the last was answered): a step inline (`DragSession::step`) against the
round trip through the native lane (`varde_solve::lane`):

| sketch | inline | through the lane |
|--------|--------|------------------|
| plate, 1 cell (fixed) | 0.002 ms | 0.015 ms |
| plate, 3 × 3 (fixed) | 0.10 ms | 0.11 ms |
| plate, 9 × 9 (fixed, 342 curves) | 1.6 ms | 1.7 ms |
| plate, 9 × 9 (free) | 1.1 ms | 1.4 ms |

Inline would be fast enough for typical sketches (a frame is 16 ms, and
wasm steps are about 2 ms at the plate's size) and simpler: no session ids,
no `Dragged` answers to match. But the lane costs little over the solve
itself, a step can take up to `DRAG_TIME` (100 ms) on a sketch the solver
struggles with, which inline would stall the UI and the camera for, and the
lane is there anyway for proposals, whose answers a drag must be ordered
with. So the lane path is kept and the inline one isn't built. The web's
worker round trip (a `postMessage` each way, the sketch only with a
session's first step) wasn't measured separately; the headless Firefox run
of step 2c answered drag steps through it.

### Snapshots

Sending the document per request needs cheap snapshots.

- Native: `Snapshot` is `Arc<Document>` (`Editor::snapshot`), the same
  `Arc` the editor and its undo stack hold, so a snapshot is a refcount
  bump. Parts of the document aren't shared between revisions yet: bodies
  and sketches as `Arc<Body>` etc. (or `im`/`rpds` persistent vectors)
  would do that, and would also cut the snapshot undo stack's memory use.
- Web Worker: the snapshot is `postcard` bytes of `Document`
  (`Document::to_postcard` in `document::codec`); a `.vrdp` record holds
  MessagePack by name instead. A few KB to MB, fine per edit. If it gets big,
  send `Command`s instead and keep a replica document in the worker (needs
  `Command: Serialize` and undo/redo as commands too).

Results: `RenderMesh` is `Vec<[f32; 3]>`/`Vec<u32>`; on the web they're cast
with `bytemuck` to bytes and the `ArrayBuffer`s *transferred*. On receipt
they're validated before use (`regen::wire`): lengths are multiples of the
element size, index and edge values < vertex count, counts within a bound.
Checked math on sizes, since the data ultimately comes from a file.

### Caching

Per-feature/per-body results keyed by a content hash of their inputs, held
in the worker. Regeneration then only recomputes what changed and the
final merge. Living in the worker means the cache survives between
requests without crossing the boundary. (On the web a `terminate()` loses
it, another reason to prefer step-and-yield cancellation there.)

### Files

All file system access goes through the IO lane: open, save, save as,
auto-save, the recent files list, and later thumbnails and export.

**A dedicated lane, not iced's thread pool.** `Task::perform` with blocking
IO works natively, but it ties up threads of the pool that also drives
subscriptions, including the ones delivering compute results; a save
blocked on a file lock would delay meshes. And on the web it's the main
thread. Unlike the compute lanes the IO lane is an ordered queue, not
latest-wins, since writes to one file must land in order. It coalesces per
target instead: a save queued behind another save of the same file replaces
it.

**The lane owns the open files.** `DocumentFile` holds the tail it last
read or wrote, which is its conflict check, so two saves must never use it
at once. The UI holds a `FileId`; the IO lane keeps the `DocumentFile`s.

The original sketch of the messages follows. The real ones grew from it and
differ (e.g. `access: Access` for `mode`, `Close { file, discard }`,
`Opened::recovered` as an `Offer`): see `varde_io::Request`, `Response` and
`Opened`.

```rust
enum IoRequest {
    Open { path: PathBuf },
    /// Creates and locks an app store entry for a new design.
    New,
    Save { file: FileId, revision: u64, document: Snapshot },
    SaveAs { file: FileId, path: PathBuf, revision: u64, document: Snapshot },
    /// To the sidecar, or the app store entry for a new design.
    AutoSave { file: FileId, revision: u64, document: Snapshot },
    Close { file: FileId },
    WriteRecent { entries: Vec<RecentFile> },
}

enum IoResponse {
    Opened { path: Option<PathBuf>, result: Result<Opened, Error> },
    Saved { file: FileId, revision: u64, result: Result<(), Error> },
    AutoSaved { file: FileId, revision: u64, result: Result<(), Error> },
    // ...
}

struct Opened {
    file: FileId,
    document: Document,
    /// `ReadOnly` if another editor holds the lock.
    mode: Mode,
    /// A newer unsaved state left by a crashed session, if there is one.
    recovered: Option<Document>,
}
```

**Save.**

- The UI sends a snapshot and the revision it's of: the same cheap `Arc`
  snapshot as regeneration natively; on the web the postcard bytes already
  encoded for the regeneration lane, posted to both workers. Encoding
  (MessagePack, snappy, XXH3), locking and writing all happen in the lane.
- `saved_revision` is set from the response, to the snapshot's revision,
  not the current one: edits made while saving keep the document marked
  edited. The title shows "Saving…" while a save is in flight.
- Editing, camera and UI carry on during the save.
- Only committed `Editor` state is saved, and that is always valid (see
  "Only valid edits are accepted"). A pending proposal or a drag in
  progress isn't part of it, so no file, sidecar or store entry ever holds
  an invalid or unsolved sketch.
- Explicit Save waits for pending proposals to be answered, then
  snapshots: the user expects what's on screen to be saved, and a pending
  edit may still be rejected. The wait is normally a few ms; for a long
  check, "Saving…" shows until it's done. A drag in progress isn't waited
  for; it's saved by the next save.
- Auto-save doesn't wait. It snapshots the committed state as it is;
  pending edits are picked up by the next auto-save once accepted.
- `Conflict` comes back as a message and the UI offers Save As, overwrite
  or reload. Other errors are shown and nothing is lost, the document is
  still in memory.

**Auto-save.** A subscription timer: once the document is edited and has
been idle for a few seconds (and at most every few minutes while edits keep
coming), send `AutoSave`. The same machinery as Save, but it writes to a
separate file, never the user's `.vrdp`, so explicit Save keeps its meaning
and a file that's synced (Dropbox, git) isn't touched on every edit.

- **Next to the document:** `design.vrdp` auto-saves to a sidecar
  `.design.vrdp.autosave` in the same directory (hidden: dot prefix on
  Unix, hidden attribute on Windows). Same `.vrdp` format and the same
  `DocumentFile` code, so appends are crash-safe. An explicit Save
  truncates it to empty, as does undoing back to the saved state
  (`DiscardRecovery`); it only grows between saves.
- **New designs** have no directory yet, so they live in the app store
  (`<data dir>/designs/<id>.vrdp`, same format) until the first Save As.
  That writes the user's file and deletes the store entry; from then on
  the sidecar applies. New designs are auto-saved like any other, so an
  Untitled design survives a crash too.
- **Recovery.** A clean close deletes the sidecar (or the store entry, if
  the user discards a new design). One that's still there with a record
  in it when the document is next opened is from a crashed session: `Open`
  returns it as `recovered`, and the UI offers to restore it. Store entries
  left behind show up on the welcome screen as recovered designs.

**One editor per document.** The sidecar doubles as the lock file. Opening
a document for editing creates it if needed and takes an exclusive,
non-blocking OS lock on it (`File::try_lock`: `flock` / `LockFileEx`), held
by the IO lane for as long as the document is open. Store entries are locked
the same way.

- If the lock is taken, another editor (another window, process or user
  on a shared drive) has the document open. `Open` succeeds anyway with
  `mode: ReadOnly`: the document is shown with a read-only banner, edit
  commands are disabled, and Save As is offered to continue from a copy.
  Nothing is auto-saved. The IO lane can retry the lock every few seconds
  and offer to switch to editing once the other editor lets go (reloading
  first, since the file will likely have changed).
- If the sidecar can't be created (a read-only directory or medium), the
  document opens read-only too, with the reason in the banner.
- OS locks are released when a process dies, so a crash never leaves a
  stale lock, unlike presence-only lock files. A sidecar that exists but
  isn't locked just means recovery, see above.
- Deleting the sidecar on close races with another editor that opened it
  just before: it would lock the unlinked file. After locking, check that
  the path still refers to the locked file (same file id/inode), and retry
  if not. Windows can't delete an open file without share-delete, so close
  there is delete-then-unlock with `FILE_SHARE_DELETE`, or unlock-then-delete
  and accept the small window.
- Locks on network file systems (NFS, SMB) range from working to silently
  ignored. The conflict check in `DocumentFile::save` still catches
  overwrites, so the worst case is a refused save, not lost work.
- The per-operation lock `DocumentFile::save` takes on the `.vrdp` itself
  stays: it keeps a save and another program reading the file apart, which
  the sidecar lock doesn't cover.

**Close and quit.** Closing a document or the window with a save in flight
waits for it: close requests are intercepted (`window::close_requests()`,
exit on close off), the status shows "Saving…", and the close finishes on
the response. Pending proposals are waited for first, like Save. Unsaved
changes prompt as usual. The sidecar is deleted and
unlocked only once the close is clean.

**Recent files.** Loaded by the IO lane at startup; the welcome screen shows an
empty list until it arrives. Written with `WriteRecent`, fire and forget,
the newest list replacing a queued one.

**Dialogs.** `rfd::AsyncFileDialog` already runs through the portal
asynchronously; it stays as it is.

**Web.** There's no path-based file system.

- Auto-saves, of new designs and of designs opened from the user's files
  alike: the Origin Private File System, laid out like the native store.
  Its synchronous access handles (`createSyncAccessHandle`: read and write
  at an offset, truncate, flush) exist *only in workers*, which suits the
  IO worker, and map well onto the append-only format.
- Locking across tabs: a sync access handle is exclusive per file, so
  holding one open on the sidecar for the session is the lock, and a second
  tab fails to get it and opens read-only. The Web Locks API
  (`navigator.locks.request(name, { ifAvailable: true })`) is the
  alternative if handles turn out to be awkward to hold; like OS locks,
  both are released when the tab dies.
- So the IO lane's `DocumentFile` sits on a small storage trait
  (`Storage`: len, read at, write at, truncate, sync) instead of
  `std::fs::File`: `std::fs` natively, OPFS handles on the web.
- Open, Save and Save As of the user's files: the File System Access API
  (`showOpenFilePicker`/`showSaveFilePicker`, Chromium only), or a file
  input and a download elsewhere. Pickers need a user gesture, so they
  run on the main thread, from `update` right after the click; the
  `FileSystemFileHandle` they return is posted to the IO worker along with
  the request, which reads it with `getFile()` and replaces it whole with
  `createWritable()`: a file of the user's has no sync access handle to
  append through, so it's written as a `.vrdp` with one record, after the
  same conflict check as natively, made on the bytes read back. Only the
  page can ask for permission to write, so the app asks as the user saves,
  before sending the save. Such a file can't be locked across tabs; the
  conflict check is all there is.
- Built since: Save and Save As keep designs in browser storage, by name
  (`saved/<name>.vrdp` in OPFS, history kept by appending, the sidecar
  `saved/.<name>.vrdp.autosave` held as the lock across tabs, as natively;
  see `agents/web-files.md`). A file from a file input is copied there
  and opened from it. Download is a command of its own, never a save,
  recorded in `downloads.toml` for the welcome screen and the file menu
  to say where a design stands against its downloads.
- Auto-saves of designs opened from the user's files go to an OPFS entry
  like a new design's, along with the file's name, so one left behind is
  recovered by name, as a copy: the handle is gone with the tab. Keeping
  handles in IndexedDB, for recent files and for recovering in place, is
  left for later.

## Options for coordinating with the UI thread

Roughly simplest first. Not exclusive. **Decided:** Web Workers (4) on the
web, and on native a worker thread (2) with a latest-value slot (3), both
behind one `Transport` trait, with tests answering requests themselves (7).

1. **`Task::perform` per request.** Spawn a future that computes and maps to
   a `Message`. Native: runs on iced's thread pool, zero plumbing. Web: runs
   on the main thread, so it blocks the UI; useless there. No latest-wins or
   cancellation without extra state. Good as a first step on native only.

2. **Worker thread + `Subscription::run` stream.** One long-lived compute
   thread per document; `Subscription::run` owns a stream that yields
   `Response`s as `Message`s. Requests go in via a channel sender stored in
   `Doc` (handed to the app once via a first `Message::WorkerReady(Sender)`,
   the usual iced pattern). The UI side only ever `try_send`s, which is safe
   on the web main thread too. Channels: `futures::channel::mpsc` or `flume`
   (both have async receivers for the stream side).

3. **Single-slot mailbox (latest-value).** Instead of a queue, a
   `Mutex<Option<Request>>` + wakeup (or `tokio::sync::watch`, or
   `arc-swap`). The UI overwrites the slot; the worker takes the newest when
   it's free. Gives coalescing for free. The same shape works for results
   (UI takes the newest `Arc<RenderMesh>` on the next frame), but pushing results
   as `Message`s through option 2 keeps iced's redraw logic simple.

4. **Web Worker + `postMessage` (`gloo-worker`, or hand-rolled with
   `web-sys`).** A second entry point built by trunk
   (`<link data-trunk rel="rust" data-type="worker" data-bin="...">`),
   postcard bytes in, transferred buffers out. `onmessage` feeds a
   `futures::channel::mpsc` that the same `Subscription::run` stream reads,
   so the app code is shared with option 2 behind a `Transport` trait. Works
   on stable Rust and any static host. Cost: two wasm instances (memory,
   download is shared/cached), serialization on each request, awkward
   cancellation.

5. **wasm threads + SharedArrayBuffer.** Same code as native (options 2/3,
   rayon). Needs nightly + `build-std`, COOP/COEP headers on every host
   serving the app, and discipline that nothing on the main thread blocks
   (no `lock()`, `recv()`, `join()`, and nothing in a dependency that does,
   e.g. a rayon `join` called from `update`). Best performance; worst
   deployment story; toolchain churn.

6. **Rayon inside operations.** Orthogonal to the above: data-parallel
   work within one job (e.g. per-body tessellation, boolean sub-steps) on
   native. Always called from the compute thread, never from `update`.
   On the web it's sequential unless option 5 is used; code must work
   either way.

7. **Tests answer requests themselves.** A test `Transport` keeps the
   requests, and the test answers each with `handle`, delivered as the lane's
   message would be. Headless tests of the app logic (request → response →
   viewport state) run deterministically without threads or a browser, down
   the same path as real responses. The threaded transport gets its own
   small test that round-trips a request.

## Plan

Steps 1-6 are done, see "Status". They made the mesh a request/response
tagged with generations (the app's `MeshFeed`, "Regenerating…" while it's
stale), put `Request`/`Response` and the pure `handle` in `varde-regen` and the
`Transport` trait in `varde-lane` (tests answer with their own, option 7),
ran the regeneration lane as a thread per document natively and a Web
Worker on the web, and built the IO lane natively and then on the web over
OPFS. Snapshots are a whole `Arc<Document>`, not shared parts. Not done from
them: cancellation of a running job.

7. **Solver** lands with the pure API above and the solver lane, plus
   proposals: commands that can be invalid go through `Propose`, commit on
   `Accepted` with their solutions as one undo entry, show pending state,
   queue in order, and are dropped by undo; the drag session commits
   `SetSketchPoints` on release. Save waits for pending proposals. Decide
   inline vs solver lane for drag steps from measurements.
8. **Later:** per-feature cache in the worker, rayon inside jobs (6),
   progress reporting, cancellation of a running job (`AtomicU64`
   natively, step-and-yield on the web). Revisit wasm threads (5) only if
   the Web Worker path proves too slow and COOP/COEP is acceptable where
   the app is hosted.

## Open questions

- Where the web build will be hosted, which decides whether COOP/COEP (and
  so option 5) is available at all.
- Which boolean kernel runs on the web, given manifold is C++.
- Whether feature edits that break later features are refused like sketch
  edits (consistent, but can keep an edit pending for seconds on a large
  history) or accepted with the later features marked as failed.
