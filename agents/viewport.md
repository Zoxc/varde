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

A banner can sit on top of the viewport, between the toolbar's banners
and it, over the viewport's column only: a sketch edit the solver
refused after its sketch was left ("An edit of Sketch 1 wasn't kept",
in the warning colour, with Dismiss; see `agents/sketch.md`). It takes
height from the viewport rather than covering the scene, and sits on the
panel's colour, as the toolbar's banners sit on the window's. A banner's
detail fills what's left of its row and wraps, so its actions stay on a
small window.

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
stays on XY, and the renderer's sketch layers carry the extrude, depth
tested (`SketchScene::depth_tested`), so the model, the preview body
included, hides what's behind it of them. Before
the source sketch is chosen, every candidate's regions (the visible
sketches with any, each on its own plane, `Space::On`) are filled in the
live layer; after, the source's regions are
the base layer on its plane, those picked filled stronger and outlined,
kept until the profiles (by pointer), the picked set or the colours
change. The region hovered is filled over them. Picking casts the
cursor's ray onto each candidate's plane (`Projector::cursor`), asks
`Profiles::region_at` there and takes the nearest hit by depth
(`Projector::depth`); a left press on a region picks it (captured),
anywhere else the left button orbits as outside a sketch, and the camera
keeps the other buttons. The handle is an arrow from the picked regions'
area-weighted centre (holes taking theirs away) along the plane's
normal: its shaft is drawn in the live layer on the plane of that
placement (`Space::On`), and each
knob is a widget in an `Anchors` layer whose placement has the axis as
its x axis, so the knob at `t` mm is the "sketch point" `(t, 0)`. One
side has a knob at its distance (negative when flipped), symmetric at
half of it, two sides one per side. A knob the model's mesh hides isn't
laid out (`viewport/extrude.rs`'s `hidden`: a ray from the knob towards
the eye meets a triangle more than 0.002 view heights in front of it, as
far as the renderer pulls the layers, so a knob on the cap it ends on
shows; a triangle whose plane passes within the mesh's `f32` rounding of
the knob doesn't count either, or far from the origin, seen at a grazing
angle, the cap's rounded corners would hide its own knob; past 2¹⁸
triangles the knobs always show, rather than slow every frame). An extrude its own check refuses (`ExtrudeState::refused`, two
sides over the limit) has no preview, and draws no shaft, which would
be a line on its own; its knobs stay. Pressing a knob sends
`GrabHandle`; while the app says one is grabbed the `Program` follows
the cursor (the raw position, over the rest of the window too): the
distance is the point of the axis nearest the cursor's ray
(`Projector::ray`), snapped to the roundest 1, 2 or 5 × 10ⁿ of the
design's units at least 6 pixels long (`snap_step`), and nothing while
looking along the axis; letting go sends `DropHandle`. The floating
panel is the viewport's last layer, at its right under the camera
controls (over them in a short viewport, `operation_panel::placed`) and
12 px clear of the viewport's bottom (its body scrolls rather than run
past it), `opaque` so clicks and the wheel on it don't reach the scene;
the knobs' layer under it stays, empty, without knobs, so the panel's
widget state survives the handle coming and going.

The panel follows the mock's: its text in the text colour
(`theme::operation_panel`, headings and the summary muted); extents and
operations are choices with a 1 px border (`theme::choice`, accent on the
soft accent and semibold while on); Flip and the Bodies rows are
checkboxes (`theme::tick`: a faint box turning accent on hover, accent
filled with a white check while ticked); a distance's label is 62 px with
a 6 px gap, and why its text is refused shows under the field, 68 px in.
Editing an extrude shows each distance with the design's unit after its
bare numbers ("10 mm" for a typed "10", `Value::pin_units`), as a new
one's does, while the value kept is the stored one, so OK with nothing
changed writes nothing. Every scrollable (the panel's body and message,
the side panel's lists, the delete prompt, the welcome page) is made by
`chrome::scrolled`: a 4 px faint scroller on no rail (`theme::scrollbar`).
A disabled filled button fades whole, fill, text and border at 0.45
opacity (`DISABLED_OPACITY`), so a disabled OK reads as one in dark too;
dark danger text is #f07563, the Delete button keeps #e0564b
(`Palette::danger_fill`) under its white text.

The renderer draws, in order: the background; the model's faces, or with
`Frame::faded` its depth and then only its nearest faces blended at
`Colors::faded_alpha` (depth still written); its feature edges; the grid,
ray traced per pixel on `Frame::grid`, a `GridPlane` (the XY plane, or a
sketch's plane), faded out a few view heights from the target and at
grazing angles, with its two axis lines over it, coloured by the world
axis they lie along; the finished sketches (`Frame::sketches`, `RenderLines`), `LINE_WIDTH`
logical pixels wide, cut at the near plane, depth tested and pulled
towards the camera like the edges, so bodies in front hide them but a face
they lie on doesn't; the origin marker; and on top of it all the sketch
being edited (`Frame::sketch`, a `SketchScene`), not depth tested, so the
faded model never hides it. Setting up an extrude, the same layers are
depth tested instead (`SketchScene::depth_tested`, the shader's
`SKETCH_DEPTH` override on a second set of pipelines, made from a shader
module of their own, since wgpu's GL backend caches programs by module
and entry point, not by overrides, see `notes/upstream/wgpu.md`): what isn't in
screen space gets its depth pulled towards the camera like the edges
(`overlay_depth`: 0.002 view heights, at least `EDGE_DEPTH_BIAS`), so a
region on a body's face or cap shows and one behind a face doesn't, and
the depth range is fitted to the layers' bounds too
(`SketchLayer::bounds`), so nothing of them is cut at the far plane.
Screen-space items are on top either way. An outline lying on a side
face (a region in the middle of a two-sided body) shows on it, as a
finished sketch's line would.

The grid's axis lines are drawn in the grid's pass (`axis_line`) but not
faded: they run on at full strength to the horizon, and show when the
plane is seen edge on. Each pixel's coverage comes from its distance to
the line's image on screen, the homogeneous line through the images of a
point of the axis (the one nearest the target, so clip coordinates stay
small) and of its direction, exact at any distance, zoom and display
scale in either projection, so they're anti-aliased without MSAA (which
WebGL2 would make costly). Their depth is that of the axis's point nearest
the pixel's ray, written as the fragment's depth (clamped to the range,
so past the far plane they still show), and where that point is behind
the near plane the pixel isn't drawn, which also drops the part of the
image that's behind the eye. So bodies in front hide them like the grid.

The origin marker is flat: a ring lying in the grid's plane around the
world origin, and a dot at the origin itself, each a white core with a
dark rim (`Colors::origin_outline`) so it reads on both themes'
backgrounds. It's one screen-space quad: the ring is the image of a circle
in the plane, scaled so its widest is `RING_RADIUS` logical pixels at any
zoom, an ellipse as the plane tilts away and a segment when it's seen edge
on (its distance is to a polygon of it, which stays right then), so it
shows the plane the axis lines run in without a third axis sticking out
of it; the dot stays round. It's drawn over the model and the finished
sketches, since the origin often coincides with model corners, and under
the sketch being edited. The world axes' directions and names are on the
view cube instead (below).

The view cube (`view_cube.rs`) is a canvas in the controls, turned with
the camera: its faces lettered and lit as in the mock, clicking one looks
from that side. The X, Y and Z axes are drawn on it, in the scene's axis
colours: each along one of the four cube edges parallel to it and on past
the cube, with an arrowhead and its letter past the tip (kept inside the
widget, 116 px, which is larger than the cube for them). Of the edges on a face turned to the camera, an axis takes one
whose part past the cube nothing of the cube hides, if any, and of those
the lowest and furthest left on screen, so in most views the axes gather
at the cube's bottom left like a triad; a fixed corner can't do that,
since from Home the edges from any one corner along all three positive
directions don't all show. The edge is drawn over the faces; the part
past the cube over them if nothing hides it, else under them, which
hide what's behind. The letters are stroked paths rather than text, which
a canvas draws over all its shapes, so faces can hide them too. An axis
seen end on (head-on views) has no edge in sight and isn't drawn.

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
projected on the GPU, in logical pixels on the screen, or (lines and
fills) in coordinates on a plane of their own, placed in the world as
they're added (`Space::On`, flagged `WORLD` to the shader, x and y with
z apart), so a layer can draw on several planes. The
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
tolerance and its inputs' keys, bounded by bytes (256 MiB natively, 64
MiB on the web, least recently used out, never what the last request
used), so an edit or a draft being dragged reruns only what it changes,
and undo, redo or an option changed and changed back finds what it had.
It also keeps the joined model mesh by scene (the shown bodies' mesh keys
in order; the committed model's scene is never evicted, however long a
draft is dragged), so an answer whose shown bodies and tolerance didn't change
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
bodies each join, cut or intersect touches, the bodies' boxes and the
draft's outcome) and the mesh's positions, normals,
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

## Screenshots of the screen

The app's tests can draw the document screen to PNGs, to look at what
headless tests of messages and state can't show: layout, drawing order,
colours, scale. `crates/app/src/doc/extrude/tests/shots.rs` drives a
`Doc` through scripted scenarios with the extrude tests' fixtures (the
plate, the example and a hole, many bodies) and the regeneration answered
in the test (`deferred`/`answer`), builds `doc.view(..)` in a
`UserInterface` on iced's headless **wgpu** renderer (tiny-skia would
leave the viewport blank: only `iced_wgpu` prepares and renders shader
primitives) and takes `Headless::screenshot` at the window's physical
size and scale factor, which reaches the viewport's primitive as on
screen. Hover and tooltips come from a `CursorMoved` and a later
`RedrawRequested` sent through `ui.update`, never from poking state; the
camera is set through `Look` messages (Home, orbit, zoom: Home shows only
about 7.5 mm, so shots zoom out to frame the 60 mm plate). Each shot draws
twice: the first frame, cleared to magenta, lays out the scene's caches
and the anchored knobs and checks that less than 2 % of the window is left
undrawn (the translucent 1 px separators show the clear colour; a viewport
that didn't draw would be most of it); the second is cleared to the
theme's background, as the app clears its window, and written out.

The tests are `#[ignore]`d, and write nothing unless `VARDE_SHOTS` names a
directory; with no adapter they print "no GPU adapter, skipping". Each
makes its own wgpu instance, under a lock, so run them one at a time:

```sh
VARDE_SHOTS=$PWD/target/shots cargo test -p varde-app shots_ -- --ignored --test-threads=1
```

Scenarios (`shots_01` .. `shots_15`, each at 1280×800, scale 1, light,
the busiest also at scale 2 and dark): `E` with every candidate's regions
(and one hovered); a region picked before and after its answer; flip,
symmetric, two sides, a refused distance and a draft the document
refuses; a cut, through all, with its Bodies list, a row hovered, a body
taken out; join and intersect, also from below the plate, and the
"Through all" tip; 20 and 30 bodies, scrolled, also at 1024×600; the
panel in a small window with errors; editing an extrude whose region is
gone; the Timeline with each extent and a failed row's tip; the delete
prompt, long and short; the file menu and its tolerances; odd cameras
(along the handle's axis, perspective with the handle behind the eye,
grazing, the knob off the screen, a 100 m extrude, a knob dragged past
the limit, one `Doc` at three window sizes); a long status beside the
key hints; the banner over the viewport for a refused sketch edit, short
and long in a small window, and the body delete prompt's warning of a
cut left with nothing to work on; two plates a join merged, Objects
showing the merged one "in Body 1" (hovered, dark) and the join's panel
saying which body it joins into. Shots are for
looking (pixels differ by GPU and driver), never compared and never
committed: a fault a shot finds gets an ordinary headless test of the
state or layout behind it. A scenario answers each regeneration it asks
for before its shots, unless the shot is of the wait (`-waiting`): an
unanswered one shows "Regenerating…" with the last answer's preview and
Bodies list, which reads like a fault and isn't one.

Reading shots: a finding names the shot, what's wrong and the code
behind it. First drop what the harness made (an unanswered request, a
scenario that left nothing to see, shading that differs by adapter);
then compare with `notes/ui-mock.html`, the design the screen follows.
What's wrong or misleading, or differs from the mock where the mock is
the design, and is cheap to change, is fixed, in stages of related
changes, each with its failing headless test first; what needs a
product decision or a large change (the renderer, a new feature) goes
to the user with its shot; the rest stays as it is.
