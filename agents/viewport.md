# How the viewport works

The document screen docks the toolbar above and the side panel left of the
viewport, which fills the rest. The viewport is a `shader` widget whose
primitive calls `varde_render::Renderer`, which opens its own render pass with
a depth buffer and composites onto iced's frame (`LoadOp::Load`). iced then
draws the UI on top: in a sketch the layer of widgets anchored to the
sketch (`anchors.rs`, below), and the camera controls in the viewport's
corner, wrapped in a `mouse_area` so clicks on them don't reach the
viewport. Neither takes events off its widgets, so they reach the scene.

In a sketch the viewport's `Program` is given the sketch (`Sketching`, in
`viewport/sketch.rs`): the model is drawn faded, the grid on the sketch's
plane, and the sketch itself by the renderer over everything; the left
button is the sketch's, for selecting, dragging and drawing geometry, and
so is `Esc` while it's held (see `agents/sketch.md`), and with a drawing
tool the modifiers, whose `Shift` turns snapping off. The `Program` sends
the left button and cursor moves to the sketch, unless the camera is being
dragged, and the other buttons and the wheel to the camera. It sets the
cursor: grabbing or moving while the camera is dragged, grabbing while
geometry is, and over the viewport a crosshair with a sketch tool or a
pointer over an item to select. Drags map to camera moves in
`DragKind::for_button`, from the button, the modifiers the widget tracks
from `ModifiersChanged` and whether a sketch is open: middle and `Shift` +
right orbit, right pans, left orbits outside a sketch; `viewport::hints`
and `README.md` follow it.

The anchored layer is an `Anchors` widget: widgets each centred where a
sketch point shows (the `Projector` hit testing uses, sized at layout), and
only those whose point shows inside the viewport are laid out, drawn and
given events. It fills the viewport but takes nothing itself, so the
cursor and clicks off its widgets go through to the scene; a full-size
widget that captured, or set a cursor, would hide the scene's (iced's
stack "levitates" the cursor for the layers below one that sets it).
There are five: the constraints' glyphs, each nudged a little up and
right of its anchor and apart from those before it (`Anchors::nudged`, see
`agents/sketch.md`); dimensions' labels, where they're put; the value
field a dimension's value is typed in, alone in its layer so its focus and
text stay its own as labels come and go; the glyph of where a drawing
tool's click snaps, by the cursor, which the `Program` tells the app as
the cursor moves or `Shift` (which turns snapping off) changes
(`Look::Snap`); and a drawing tool's fields, their top left corner beside
where its click would go (`Anchors::beside`), which the `Program` tells
the app on every move while the tool's shape has fields (`Look::Aim`,
carrying the snap too). A focused field takes the keys
typed in it, so no shortcut fires, and wraps itself in `OnEscape` so the
first `Esc` closes it. A label takes its own press; the `Program` then
follows the cursor while the app says a label is grabbed, and sends the
drag and the release.

Setting up an extrude (`viewport/extrude.rs`, `crate::extrude`, the
app's `doc/extrude.rs`), the `Program` is given the session
(`Extruding`) in place of a sketch: the model isn't faded and the grid
stays on XY, and the renderer's sketch layers carry the extrude. Before
the source sketch is chosen, every candidate's regions (the visible
sketches with any, each on its own plane) are filled in the live layer
in screen space, projected every frame; after, the source's regions are
the base layer on its plane, those picked filled stronger and outlined,
kept until the profiles (by pointer), the picked set or the colours
change. The region hovered is filled over them. Picking casts the
cursor's ray onto each candidate's plane (`Projector::cursor`), asks
`Profiles::region_at` there and takes the nearest hit by depth
(`Projector::depth`); a left press on a region picks it (captured),
anywhere else the left button orbits as outside a sketch, and the camera
keeps the other buttons. The handle is an arrow from the picked regions'
area-weighted centre (holes taking theirs away) along the plane's
normal: its shaft is drawn in screen space in the live layer, and each
knob is a widget in an `Anchors` layer whose placement has the axis as
its x axis, so the knob at `t` mm is the "sketch point" `(t, 0)`. One
side has a knob at its distance (negative when flipped), symmetric at
half of it, two sides one per side. Pressing a knob sends
`GrabHandle`; while the app says one is grabbed the `Program` follows
the cursor (the raw position, over the rest of the window too): the
distance is the point of the axis nearest the cursor's ray
(`Projector::ray`), snapped to the roundest 1, 2 or 5 × 10ⁿ of the
design's units at least 6 pixels long (`snap_step`), and nothing while
looking along the axis; letting go sends `DropHandle`. The floating
panel is the viewport's last layer, at its right under the camera
controls, `opaque` so clicks on it don't reach the scene.

The renderer draws, in order: the background; the model's faces, or with
`Frame::faded` its depth and then only its nearest faces blended at
`Colors::faded_alpha` (depth still written); its feature edges; the grid,
ray traced per pixel on `Frame::grid`, a `GridPlane` (the XY plane, or a
sketch's plane), its axis lines coloured by the world axis they lie along;
the finished sketches (`Frame::sketches`, `RenderLines`), `LINE_WIDTH`
logical pixels wide, cut at the near plane, depth tested and pulled
towards the camera like the edges, so bodies in front hide them but a face
they lie on doesn't; the origin marker; and on top of it all the sketch
being edited (`Frame::sketch`, a `SketchScene`), not depth tested, so the
faded model never hides it.

Lines, the finished sketches' and the sketch being edited's, are one
shader (`line_vertex`, `fs_line`): a quad per segment a pixel wider than
the line, cut at the near plane in perspective and to the viewport and a
margin (so pixel coordinates stay exact in `f32`), divided through so
fragments see pixel positions, and the fragment shader turns the distance
from the segment into coverage: anti-aliased at any zoom and display scale
without MSAA, with round ends and joins. A segment of the sketch being
edited knows its neighbours, and where two overlap at a join a pixel is
drawn only by the nearer, so translucent lines don't darken there; the
finished sketches' segments come alone, as they're uploaded, and overlap
at joins, which only thickens the fringe of an opaque line. Dashes run
along a polyline by its length (in sketch units at the target's scale, or
logical pixels on the screen), faded at their ends. The model's feature
edges are still hardware lines: as quads they'd need a buffer of every
edge's ends, past the 256 MiB buffer bound for `RenderMesh::MAX_EDGES`.

The sketch being edited comes as two `SketchLayer`s the view builds, each
with fills (triangles, tessellated even-odd by tess2-rust, holes left out),
lines and points (a disc with a rim, filled with the rim's colour if
fixed), drawn in that order, in sketch coordinates on `SketchScene::plane`
projected on the GPU, or in logical pixels on the screen (`Space`). The
base layer, with the profiles' regions shaded under the rest and
splines' handles (a selected one's also where its bare ends' would be)
and a selected one's control polygon, is
re-uploaded only when its `Arc` changes, which the view makes happen only
when the sketch, the selection, the items' states (fixed, in a conflict,
waiting on the solver), a label being dragged, the profiles the app found,
whether curvature combs show, or the theme do, so the camera only changes
uniforms; the live layer (hover, the region under the cursor included,
the tool's preview, a shape tool's piece to trim, extension or mirror
images, the box, and what keeps its size on screen and so is made where
it projects: dimensions' arrowheads, as triangles in screen space, the
rings marking near misses, whose gap is a few pixels at the target and so
is paired again from the profiles as the zoom changes, and the selected
splines' curvature combs, their teeth worked out with the base layer and
scaled by the pixel's size at the target) is rewritten every frame into
buffers the slot keeps and grows.
The depth range is fitted to the target, the grid around the target seen on
its plane, and the bounds of the mesh and the lines. `Camera::face` turns
the camera to look straight at a plane with its normal and an up
direction, which `look_from(View)` is a case of; the camera has no roll, so
up is only chosen freely for a vertical normal (see `notes/SketchImpl.md`).

The document mesh (the regeneration lane evaluates the feature history
into the bodies' solids, extruding sketch regions, and tessellates the
visible ones with `Solid::tessellate`, see "Bodies from the history" in
`agents/kernel.md`) and the visible sketches'
lines are kept in the app's `MeshFeed` and keyed by `Editor::generation()`, so they're only
rebuilt when the document changes; the viewport widget just draws what it's
handed. The renderer's GPU copies are keyed by their `Arc`s instead, since
iced shares one pipeline between documents whose generations each start at
0. The feed asks the regeneration lane for them with a cheap `Arc` snapshot
of the document and takes the answer when it arrives, dropping answers
older than the model shown. Until then it keeps the last one and the status
bar says "Regenerating…". A request leaves the sketch being edited out of
the lines (`exclude`), which the viewport draws over everything instead. Entering or
leaving a sketch asks again for the same generation with the new
`exclude`; the answer, a model or a failure, says which sketch it left
out, and one of the same generation as what's shown (or failed) is taken
only if it left out the sketch asked for last and the answer applied
didn't. So a failure holds back only the request that failed, not asking
again with another `exclude`. Sketch curves are flattened by
`Sketch::flatten` (lines exact, circles into `CIRCLE_SEGMENTS`, arcs their
share) and placed with the document's `Plane::placement`.

An answer also carries the features that failed and why (`failed`, an
extrude whose region is gone, whose profile the kernel refuses, or whose
join, cut or intersect touches no body or can't be worked out) and the box of each
body that has a solid (`bodies`). A request can carry a `Draft`, an
extrude being set up and not committed (new, or one being edited), with a
revision the app counts up: the lane answers with it applied as its
command would apply it, and says how it went (`Drafted`, with the
revision, its error and the bodies a join, cut or intersect touches,
which the panel lists: `MeshFeed::draft_touched` gives the newest
answer's that ran the touch test, of the current run of drafts, while a
draft is asked for); a draft that fails, or that the document
refuses, is answered with the committed model and the draft's error. The
lane keeps a cache of what it worked out per feature (profiles, solids,
meshes, whether each sketch solves), keyed by a hash of the feature, the
tolerance and its inputs' keys, holding what the last request used, so an
edit or a draft being dragged reruns only what it changes. It also keeps
the joined model mesh of two scenes (the shown bodies' mesh keys in
order; a dragged draft's revisions take turns in one of the two, so the
committed model's stays), so an answer whose shown bodies and tolerance didn't change
carries the same `Arc<RenderMesh>` as before and the renderer, keyed by
that `Arc`, doesn't upload it again (natively; the web wire still sends
it whole). The app sends
the extrude being set up as the draft (see "Setting up an extrude"
above); `MeshFeed` gives each draft differing from the last the next
revision, counted over the document's life, and asks again whenever the
generation, the sketch left out or the draft revision changes (a draft
dropped asks again without one). An answer of the same generation is
taken only if it's for what was asked last, sketch and draft revision
both, so a late answer for an older draft never replaces a newer one;
the draft's error shows in the panel only while it's for the draft asked
for last. While the draft asked for last (or its absence) isn't the one
the model shown has, a preview changed or cancelled, the status bar says
"Regenerating…" too. The feed keeps the answer's `failed` with the model shown, and
the Timeline marks those features (see "The extrude UI" in
`agents/kernel.md`).

Natively each open document has a regeneration thread (`regen::lane`),
started by an iced subscription keyed by the document's id. The
subscription's stream first hands the app the lane's sender, then yields
each response as a message tagged with the id, so an answer for a closed
document is never applied to the next one. Requests go into a single slot
that a newer one overwrites, so a burst of edits costs one run after the
current one, and one older than a request already sent is dropped.
Closing the document ends the subscription and with it the thread.

On the web the lane is a Web Worker with the same API. It shares no memory
with the page, so a request is the generation, the postcard-encoded
document (the encoding `.vrdp` records use) and the sketch to leave out, and
the answer is a small postcard head (with the failed features, the
bodies' boxes and the draft's outcome) and the mesh's positions, normals,
indices and edges and the sketches' line points and ends as raw bytes. Both
directions transfer their `ArrayBuffer`s instead of copying them. The page
checks what comes back before using it (whole elements, a size bound,
indices and edges within the vertex count, positions and points within
their bound, line ends splitting the points into polylines of two or more,
bodies' boxes finite and in order; see `regen::wire`). The worker keeps
its cache between requests, as the thread does. The worker can't see new messages while it works, so
the page keeps latest-wins itself: one request is with the worker at a
time, and newer ones replace each other until it answers. A job that has
started always finishes. If the worker dies (a panic traps it), the
request it was working on is reported as failed; a request waiting
behind it starts a new worker at once, otherwise the next edit does. A worker that dies before it's ready, or doesn't load, isn't
started again for the request waiting on it, which is reported as failed
too, so a worker that always crashes can't restart without end. Closing the
document terminates it.
