# How the viewport works

The document screen docks the toolbar above and the side panel left of the
viewport, which fills the rest. The viewport is a `shader` widget whose
primitive calls `varde_render::Renderer`, which opens its own render pass with
a depth buffer and composites onto iced's frame (`LoadOp::Load`). iced then
draws the UI on top: in a sketch the layer of widgets anchored to the
sketch (`anchors.rs`, below), and the camera controls in the viewport's
top-right corner (`controls.rs`: the view cube, and Home under it at its
right, wrapped in a `mouse_area` so clicks on it don't reach the
viewport; the cube's `mouse_area` only says when the cursor enters or
leaves it, `Look::HoverCube`). Neither takes events off its widgets, so
they reach the scene.

The tool rail (`rail.rs`) floats over the viewport's left edge: a card
per tool set of the mode (see "The tool rail" in `agents/sketch.md`), 48
px wide, 6 px in from the viewport's left and top and apart, each a head
with the set's icon (`theme::rail_head`, highlighted on hover and while
its set is open) over a recessed strip of its first tools, as many as
fit (`theme::rail_strip`; behind the strip's rounded top corners the head's
highlight shows while open, `rail_strip_backing`), each with an instant
tooltip at its right (`chrome::side_tip`) of its name and key. The open
set's list (230 px, `theme::rail_list` inside `rail_list_band`, whose
colour, the set's category's, shows as a 3 px band along its top) is
6 px right of the cards, its top level with its card's, moved up as far
as it must to stay clear of the status bar (`STATUS_BAR_ROOM` and 6 px),
and scrolled if it's taller than the room. Both are one layer of the
viewport's stack, above the anchored widgets and the knobs and under the
controls and the operation panel (the tooltips are overlays, over all
of them), a `Rail` widget that places them like `operation_panel::placed`
and takes only what's over them: each card and the list are a
`mouse_area` with the `Idle` interaction, so clicks between and below
cards reach the scene and those on a card's border or the list's
padding don't. A press anywhere but on the cards and the list, the side
panel and toolbar included, sends `RailLook::Close` and goes on to what's
there. The head's and the list's `mouse_area`s say when the cursor comes
over or leaves them, and the tools' when it comes over them
(`RailLook::Hover`); the app (`doc/rail.rs`) opens a head's set as the
cursor comes over it, closes the list at once over a tool on a card, and
once the cursor has left the head or the list it was over, closes it
after `RAIL_CLOSE_DELAY` (250 ms) unless it's over one again by then,
which the frames subscription runs for (`Doc::animating`). Which spot
the cursor is over is kept by the spot, so an enter and the leave
before it can come in either order. The layer is `responsive`: from the
viewport's height, clear of the status bar like the list, `rail::fitting`
works out how many tools each card shows, adding them one at a time to
the set showing the fewest that has more (the first of those) until the
next doesn't fit, so the room is shared evenly; a set showing all its
tools leaves the room to the rest. Heads always show: a rail too tall
even without tools runs past the viewport's bottom. The open list
highlights the row the keys are on (`theme::rail_row`, as on hover, and
on a disabled row too); the cursor coming over a row puts the keys there
(`RailLook::Row`), so one row shows highlighted. Its rows' scrollable is
`RAIL_LIST`, which the app snaps to the row's share of the list's end
(`Rail::take_scroll`) as the keys move or a set opens: at that share the
row always shows, however tall the list shows.

The status bar (`status.rs`) floats over the viewport's bottom right, 12
px in from its right and 10 px up from its bottom (`STATUS_BAR_ROOM` is
what it takes of the height), `opaque` so a drag on it doesn't orbit; on
the welcome screen it floats over the window's. The feature selected in
the Timeline is in a box of its own, with the key clearing it (`Space`);
then a bar with what's going on (picking a plane, the sketch's or the
extrude's status, regenerating, a failed edit, saving: nothing with
nothing selected), the hints, and the button of the view options menu,
which opens above it: Orthographic or Perspective, then Mouse hints,
without which the bar leaves out the hints of the mouse, and Hidden
edges, without which the viewport leaves out the edges the model hides
(`Frame::hidden_edges`). The app keeps those two for every document
(`Varde::options`, a `ViewOptions`, both on to start with, not saved),
and either closes the menu. The hints and the button show whole:
the bar's widget (`Bar`, which draws the boxes and the lines between
their parts itself) lays them out first, then the selection, then the
status, each in what's left, cut short, so the status gives way first.

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
and `README.md` follow it. The wheel zooms towards the cursor, as the
mock does: `Look::Zoom` carries the cursor's offset from the viewport's
middle in fractions of its height, and `Camera::zoom_at` moves the
target towards the point at the target's depth there by the proportion
the distance changes, so that point stays under the cursor in either
projection.

A middle click, the cursor let go within `CLICK_SLOP` (3 px) of where
it was pressed, picks the point the camera orbits (until the cursor
leaves that, a middle drag doesn't orbit): `viewport/pivot.rs` casts the
cursor's ray (`Projector::ray`) at the model's mesh, the nearest hit
winning (in an orthographic view anywhere along the ray, which starts
at the target's depth), and off the model onto the grid's plane (XY, or
the sketch's in a sketch) where the grid shows, within
`GRID_FADE_HEIGHTS` view heights of the target seen on it, so a click
near the horizon doesn't fly off; it sends `Look::SetPivot` with the
point, or `None` off them all. The status bar hints it after Zoom, by
the wheel's icon ("Click to set pivot", `viewport::hints`). The
app's `Doc` keeps it (`doc/camera.rs`'s `Pivot`), pans to bring it to
the middle of the view (`Camera::center_on`, across the view only, so
the zoom stays; animated as the view cube's turns are), and orbits about
it
(`Camera::orbit_about`, which turns the target about the pivot with
the view, so the pivot stays where it shows on screen); `None`, and
Home, go back to orbiting the target. Panning, zooming and the view
cube's faces leave it. Its marker (below) shows whole for
`PIVOT_SHOWN` (2 s) once picked and fades over `PIVOT_FADE` (0.4 s),
which takes frames, and shows whole while the cursor is over the cube,
fading from when it leaves.

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
app's `doc/extrude.rs`) or a revolve (`viewport/revolve.rs`, see
`agents/features.md`), the `Program` is given the session
(`viewport::Operating`: `Extruding` or `Revolving`; the region picking
and shading below is theirs alike, `viewport/regions.rs`) in place of a
sketch: the model isn't faded and the grid
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
12 px clear of the status bar (its body scrolls rather than run past
it), `opaque` so clicks and the wheel on it don't reach the scene;
the knobs' layer under it stays, empty, without knobs (and always for
a revolve, which has none), so the panel's widget state survives the
handle coming and going.

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
`Colors::faded_alpha` (depth still written); its feature edges, `EDGE_WIDTH`
logical pixels wide, depth tested and pulled towards the camera, so the
faces they bound don't hide them, at `faded_alpha` too when faded; the grid,
ray traced per pixel on `Frame::grid`, a `GridPlane` (the XY plane, or a
sketch's plane), faded out a few view heights from the target and at
grazing angles, with its two axis lines over it, coloured by the world
axis they lie along; the finished sketches (`Frame::sketches`, `RenderLines`), `LINE_WIDTH`
logical pixels wide, cut at the near plane, depth tested and pulled
towards the camera like the edges, so bodies in front hide them but a face
they lie on doesn't; the edges again where the model hides them, if
`Frame::hidden_edges` and not faded (below); the origin marker; and on top of it all the sketch
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

The pivot's marker (`Frame::pivot`, a `Pivot` with its point and
opacity) is the same quad's second instance: the same ring and dot, but
the ring lies in the screen's plane, so it's always round, its core in
the accent (`Colors::pivot`) and the whole faded by the opacity. Where
it shows within 1.5 px of the origin it isn't drawn: the origin's
marker is there.

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

Lines, the model's feature edges, the finished sketches' and the sketch
being edited's, are one shader (`line_vertex`, `fs_line`): a quad per
segment a pixel wider than the line, cut at the near plane in perspective
and to the viewport and a margin (so pixel coordinates stay exact in
`f32`), divided through so fragments see pixel positions, and the fragment
shader turns the distance from the segment into coverage: anti-aliased at
any zoom and display scale without MSAA, with round ends and joins. A
segment of an edge or of the sketch being edited knows its neighbours, and
where two overlap at a join a pixel is drawn only by the nearer, so
translucent lines don't darken there; the finished sketches' segments come
alone, as they're uploaded, and overlap at joins, which only thickens the
fringe of an opaque line. Dashes run along a polyline by its length (in
sketch units at the target's scale, or logical pixels on the screen),
averaged over what a pixel spans along the line (a box filter, from how
much longer the segment is measured than it shows), so their ends fade
over a pixel on the screen, and dashes shorter than a pixel there, on an
edge seen nearly end on or far behind the target in perspective, blur to
their average rather than beating into dashes of their own.

The edges are uploaded as a stream of points (`EdgePoint`: position, how
far along its polyline in world units, and which polyline), polyline after
polyline, a point again in a row left out, with a point of no edge at each
end of the stream. An edge that closes on itself, round three segments or
more, has its last but one point before it and its second after it, marked
neighbours only (`NEIGHBOUR_ONLY`, the edge's top bit), so its first and
last segments join like any others. That's 20 bytes a point, at most one
and a half points per edge vertex, so `RenderMesh::MAX_EDGE_POINTS` fits a
256 MiB buffer. The same buffer is bound to four vertex buffer slots a
point apart, step mode instance, so instance `i` sees points `i` to `i + 3`
as the point before, the segment's start and end, and the point after
(`EdgeIn`, `edge_segment`): the segment is drawn if its ends are of the
same edge, and joined to the points either side that are of it too,
marked or not.
That's 4 of WebGL2's 8 vertex buffers and 9 of its 16 attributes, and no
storage buffers. Each segment and its neighbours are cut at the near plane
before pulling (`pulled_segment`, which the finished sketches' lines use
too), so they agree on where they meet. Pulling (`pulled`) only changes
the depth: the line shows where it is, not where the pulled point would in
perspective, off its faces' boundary near the eye. Where two edges meet at
a corner their fringes overlap, and so do a polyline's segments where
they're shorter on the screen than the line is wide, since a segment only
knows the two either side of it.

The edges the model hides are the same stream drawn again
(`vs_hidden_edge`, its own entry point, since the GL backend keys
programs by them) with `depth_compare: Greater` against the model's
depth, which nothing drawn after the faces writes: the same quads (both
`EDGE_WIDTH` wide, the hidden pass narrowing only the coverage, and the
position `@invariant`), so the visible pass and this one split the
pixels between them, and a visible stretch and a hidden one of the same
edge meet without a gap or a pixel of both. They're `HIDDEN_EDGE_WIDTH`
(1) logical pixel wide, in the edge colour at `Colors::hidden_edge_alpha`
(0.45 in both themes), dashed (`HIDDEN_DASH`, 4 on and 3 off) along the
edge by its length at the target's scale, as the sketch's dashes are. The
dashes' phase is worked out a segment at a time: where the segment
starts along its edge, in pixels, modulo a dash and a gap, then on by
its own length from its ends, then again modulo the period from where
it's cut to the viewport, so the numbers the fragment shader mixes stay
within a period and what shows of the segment. Zoomed far into a long
edge, where how far along it a pixel is, in pixels, is past what `f32`
holds, only the phase where a segment starts or is cut is off by the
rounding, not the dashes along it. An edge's fringe within a pixel of a
silhouette is depth tested at the edge's depth, so a back edge there
leaks a little of it past the silhouette. Not drawn faded: in a sketch
the model already reads as see-through. Setting up an extrude, they're
drawn.

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
body that has a solid (`bodies`). A request can carry a `Draft`, a
feature (an extrude, a revolve) being set up and not committed (new, or
one being edited), with a
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
It also keeps the joined model mesh and its picking tables by scene (the
shown bodies and their mesh keys in order; the committed model's scene is never evicted, however long a
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

An answer carries the model's **picking tables** (`regen::Picking`, with
the mesh), by the mesh's own ids: the body of each of the mesh's parts
(the shown bodies in order; `Picking::face_body` finds a face's), its
faces (`PickFace`: the face key, its aliases, sorted, and a `Summary` of
its form: a plane's outward unit `n` and `d`, a cylinder's point, axis
and radius, a cone's, sphere's or torus's numbers, a conic cylinder's
direction, a revolved conic's axis, else `Other`: no known form, or
numbers past the bound), and for each edge whether it's a chain closing
on itself and its tangent chain (below). The mesh's faces are the kernel topology's regions and its first
edges in each part its chains, in the topology's order, the creases
inside one face after them (see "Topology and names" and "Tessellation"
in `agents/kernel.md`), so a circle's quarter walls, or flush faces
merged under one name, are one face, a face cut in two by a groove is
two faces of one key, and which face a triangle is on (its `face_ends`
range) and which faces an edge is between (`edge_faces`; a crease has
the same face twice) are the mesh's to say. `Picking::edge_keys` gives
an edge's two keys sorted, as an edge reference stores them (none for a
crease). Each body's tables are made with its mesh (`Solid::topology`,
then `Solid::tessellate_with`) and cached with it, counted in its
bytes; the scene's are theirs joined in the shown bodies' order, as the
mesh is, the tangent chains moved on (checked). A `Picking` always goes
with its mesh (a body per part, a face per face, a flag and a tangent
chain per edge, a closed edge between two faces and on one corner,
summaries finite, within `Picking::MAX_VALUE`, their directions
unit vectors, aliases sorted apart from the key): the fields are private
and `Picking::from_parts` checks parts from elsewhere. The app keeps
them with the mesh (`MeshFeed::parts` gives the parts' bodies).

**Picking the model** (`view/src/pick.rs`, the app's `doc/pick.rs`) is on
the CPU, against the mesh drawn and its tables; no GPU id buffer (WebGL2
readback stalls or is a frame late). The feed keeps the answer's
`Picking` with its mesh and counts the models shown (`MeshFeed::model`,
up whenever the mesh or the tables change; natively an unchanged scene
comes back as the same `Arc`s, from the web worker as copies, which are
compared, and either way it keeps its index). A
`PickIndex` (the mesh, the tables, the model's count, a bounding volume
hierarchy over the triangles and one over the segments of the edges
between two faces, and each tangent chain's edges) is built the first time it's
asked for (`MeshFeed::pick_index`, a `OnceCell`), sequentially (each
node split along the longest side of its items' middles at that side's
middle, in one pass, or at the median where that leaves a quarter or
less on one side, ties by index, so the same mesh gives the same tree),
and dropped with the model. It's built on the UI thread: a plate with
400 holes (217,000 triangles, 33,000 edges) takes about 50 ms optimized
on a loaded machine (twice that with median splits throughout), once
per model, on the first hover or click after it shows; a pick on it
under a millisecond (the ignored test `measure_the_index_of_a_plate_with_400_holes`,
run with `--config 'profile.dev.opt-level=3'`: `varde-view`'s tests don't
build without debug assertions). Not capped. Tables that don't go with
the mesh pick nothing. `PickIndex::pick(camera, size, at)` casts the
cursor's ray (`Projector::ray`; in an orthographic view from far enough
back that the whole mesh is ahead, in perspective from the near plane):
the nearest triangle names the face (the one whose `face_ends` range
holds it). An edge between two faces (not a crease) wins over it if a
segment of it shows within `EDGE_REACH` (6) pixels of the cursor and
isn't hidden: the candidates, found through the edge tree with each
node's box grown by 6 pixels at its deepest, are cut at the near plane,
measured on screen, sorted by distance, depth and index, and the first
of at most 64 whose point showing nearest the cursor (found back in the
world with the perspective divide undone) no triangle hides wins:
hidden means the ray from the eye to it meets a triangle nearer by more
than 0.002 view heights (what the renderer pulls edges by) and the
mesh's `f32` rounding. So an edge either side of a face shows, and one
behind the plate isn't picked. The `Pick` carries the model's count,
the target (`Picked::Face` or `Picked::Edge`, the mesh's ids),
the body (the face's part's, or the edge's first face's) and the point (the
ray's hit, or the edge's point).

Outside sketches and the extrude session, and not over a draft's preview
(`Doc::picks`; they pick what they need themselves) the viewport is given `ModelPicking` (the index,
the target the app holds hovered, and what the cursor picks, `Picks`:
faces and edges, or only one of them, from the selection's mode). It
picks on each cursor move while the camera isn't dragged, and on each
frame drawn (`RedrawRequested`) whose camera, model or cursor position
differs from those it last picked with (`Interaction::hover_seen`), so the
hover follows a zoom, the camera's animation or a new model under a
cursor that stays; only when the target differs from the app's does it
send `Look::Hover(pick)`, and `Hover(None)` once the cursor leaves the
model or the viewport, or the camera is dragged (past a click's slop:
from the first frame drawn while it is; the hover is worked out again
once the drag ends). The cursor is a pointer while
something is hovered. A pick of a model no longer shown (its count
differs) is dropped, and the hover is dropped once the model changes or
the cursor stops picking (`Doc::prune_picks`, after answers, edits and
looks); a hover names nothing else (no document change, no request).

**Selecting** (`view/src/select.rs`, `Selection`, kept in `Doc::pick`).
The left button, which orbits when dragged outside sketches, is a click
while the cursor stays within `CLICK_SLOP` (3) pixels of where it went
down, as the middle button's pivot click is: letting go sends
`Look::ClickModel { pick, add, double }`, the pick worked out where the
button went down, `add` with `Held::TOGGLE` (Shift, or Ctrl/Cmd as in a
sketch) and `double` for a second click within 400 ms and 4 pixels
(the sketch's double-click rule). What a click selects depends on the
`SelectionMode`: `Any` outside the sessions (the face or edge clicked;
a double-click its body), `Faces`, `Edges { tangent }` (with the whole
tangent chain clicked if `tangent`) and `Bodies` (the body of whatever is
clicked), which the face, edge and body sessions will set. A click alone
selects what it's on, or nothing (a click off the model clears); with
`add` it adds that, or takes it out if all of it is selected: a tangent
chain toggles as one, and a double-click with `add` first undoes its
first click's toggle, then toggles the body. Objects' body rows select
too (`Look::ClickBody`, the app filling in `add` from Ctrl/Cmd held,
where the mode takes bodies), and mark the bodies the selection holds
as bodies, so a body double-clicked in the viewport shows selected in
Objects and one clicked there shows all its faces selected in the
viewport. `Esc` (once nothing else is open) and `Space` clear it with
the Timeline's feature; selecting a feature in the Timeline clears it,
and selecting in the model lets go of the feature.

What's selected is kept by name, as a reference would be: a face as its
body, key and the point it was picked at (`Selected::Face`), an edge as
its body, the sorted keys of the faces either side and the point
(`Selected::Edge`), a body by id. Each item also holds its target in the
model it was last found in (`Selection::model`). When another model
shows (an edit, an undo, a tolerance), `Selection::resolve` finds each
again as the kernel resolves references (`PickIndex::find_face`,
`find_edge`): the faces of that body named by the key (key or alias),
or the edges between faces so named either way round; one is taken
wherever the point is, of several the nearest to the point (measured to
the drawn triangles or segments; a later one counts only where it comes
nearer by more than a billionth of the mesh's size, so ties go to the
lowest; the first where the point isn't finite). A face or edge of a
body a join merged into another is looked for in that one (the merges
the model shown found, `MeshFeed::merged_bodies`), so it stays selected
through the join, and the status bar names the body it's in now. What
isn't found (a hidden body's faces, a face an edit removed), and a body
the document no longer holds or a join merged away, stops being
selected but is kept, in its place, and looked for in each later model
until the selection changes (a click, a body's row, `Esc`, `Space`), so
undoing the edit brings it back. A merged body's row in Objects selects
the body holding it, which draws it. Replacing the document whole
(restoring recovered changes, or undoing or redoing that) forgets what's
selected and hovered, and the cursor doesn't pick until a model of the
new document shows: the ids they name may be others' now. While a sketch is
edited or an extrude set up, and while an extrude's preview still shows
after it ends (until the answer without the draft comes,
`MeshFeed::shows_draft`), the cursor doesn't pick and the selection isn't
drawn or resolved, so a draft's preview doesn't drop it; it's found again
once the cursor picks. The highlight is drawn only over the model it was
built for.

Tangent chains come from the kernel: `Topology::tangent_chains` (see
`agents/kernel.md`) gives each chain its tangent chain's lowest chain,
regen carries it for each edge of the mesh as `Picking::tangents` (a
crease its own; checked on the page: the first of a tangent chain is an
edge between two faces of the same part, no later than its members and
its own first), and `PickIndex::tangent_chain` groups them. Hovering in `Edges { tangent: true }` highlights the whole chain,
in `Bodies` the whole body.

The app's highlight (`Doc::highlight`) is `Selection::highlight`: what's
selected in `Emphasis::Selected` (a body as all of its faces), then
what's hovered that isn't selected in `Emphasis::Hovered` (a selected
face hovered keeps its colour, and nothing is drawn twice). It's rebuilt
only when the model, the target hovered or the selection changes, so
moving over one face uploads nothing.

The status bar's box tells of the selection when no feature is
selected: one face as "Face", its surface ("Plane", "Cylinder", "Cone",
"Sphere", "Torus", "Curved") and its body's name; one edge as "Edge" and
the body; one body by name and "Body"; several as "N selected" and how
many faces, edges and bodies. Its hints: "Select" and a double-click
"Body" with nothing selected, then Shift-click "Add or remove" and
"Body", before the camera's.

The **highlight** (`render::Highlight`, `Frame::highlight`, keyed by its
`Arc` and the colours) is the faces' triangles, copied from the mesh
with their normals by `PickIndex::highlight`, and the edges' polylines,
each with an `Emphasis`, hovered or selected. Faces are drawn after the model and
before its edges, lit as the model is but in `Colors::hovered_face` or
`selected_face` (the mock's hues, hsl 110 and 188, at the model's
lightness: light #cde4c8 and #b9e3e9, dark #8ab582 and #72bac5), culled
like the model, their depth pulled towards the camera as the sketch's
depth tested layers are (`overlay_depth`), so the face they lie on
doesn't hide them and what's in front does. Edges are drawn after the
model's edges as the sketch's depth tested lines in the world
(`SketchLayer::world_polyline`), `HIGHLIGHT_WIDTH` (3) pixels wide, in
`hovered_edge` (the mock's highlight line: light #9dd488, dark #76cc60 at
half opacity) or `selected_edge` (the accent).

Natively each open document has a regeneration thread (`regen::lane`),
started by an iced subscription keyed by the document's id. The
subscription's stream first hands the app the lane's sender, then yields
each response as a message tagged with the id, so an answer for a closed
document is never applied to the next one. Requests go into a single slot
that a newer one overwrites, so a burst of edits costs one run after the
current one, and one older than a request already sent is dropped.
Closing the document ends the subscription and with it the thread.

Exports are the one other request (`Request::Export`, tagged by the
app, of the committed document as it was when asked): the lane welds the
visible bodies into `ManifoldMesh`es (`regen::export`, see "Exporting
3MF" in `agents/files.md`) from the same cache and answers with
`Response::Exported`. They aren't latest-wins: each waits in an ordered
queue of its own beside the regeneration slot (`regen::newest::Newest`,
on both lanes), goes ahead of the regeneration waiting, and is never
replaced or dropped by one, so an edit right after Export 3MF… still
gets its export answered. An export doesn't begin a cache request of its
own (that would let go of the meshes the regeneration before used and it
doesn't). Its answer has no generation, so `MeshFeed::apply` ignores it
and the document takes it (`Doc::export_welded`). The feed keeps the
bodies that have a solid in the model shown, for the File menu's Export
3MF… to go only while one is shown and regenerating hasn't failed.

On the web the lane is a Web Worker with the same API. It shares no memory
with the page, so a request is the generation, the postcard-encoded
document (the encoding `.vrdp` records use) and the sketch to leave out, and
the answer is a postcard head (with the failed features, the
bodies each join, cut or intersect touches, the bodies' boxes, the
draft's outcome and the picking tables: the body of each of the mesh's
parts, the faces, and the edges' closed flags and tangent chains) and the mesh's vectors
(positions, normals, indices, face ends, edge vertices, edge ends, edge
faces, corners, edge corners, part ends) and the sketches' line points
and ends as raw bytes. A model whose head would be over its bound (64
MiB), or with more faces or aliases than a reply may carry (2²⁰ and 2²⁰
all faces' together, decoded within those bounds, and the parts and
flags within the mesh's, so a short head can't make the page build
more), is answered as failed. Both
directions transfer their `ArrayBuffer`s instead of copying them. The page
checks what comes back before using it (whole elements, a size bound,
the mesh through `RenderMesh::from_parts`, points within their bound,
line ends splitting the points into polylines of two or more, bodies'
boxes finite and in order, the picking tables checked by
`Picking::from_parts` against the mesh and naming only bodies the head
lists; see `regen::wire`). An export's answer is a head and one part,
the bodies' postcard, copied within 1 GiB and decoded with every
`ManifoldMesh` checked again. The worker keeps
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

Scenarios (`shots_01` .. `shots_20`, each at 1280×800, scale 1, light,
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
saying which body it joins into; the pivot's marker on a corner of the
plate, panned to the middle and orbited about, whole, half faded and
on the origin; the tool rail outside a sketch and in one, with a list
open, a tool's tooltip, and a list scrolled in a short window; a face
and an edge of the plate hovered (`shots_18`, light, dark, scale 2, and
from below; the scenario applies the `Look::Hover` the cursor's move
sends, `Shooter::hover`); the top selected with an edge hovered, then
the edge added, then the body double-clicked and marked in Objects
(`shots_19`, light and dark); the revolve (`shots_20`): `O` with the
lathe's regions, the axis asked for, a full turn about the construction
line, one side of 270° flipped, two sides (also scale 2, dark), then its
Timeline row selected and the rail's Create list. Shots
are for looking (pixels differ by GPU and driver), never compared and
never committed: a fault a shot finds gets an ordinary headless test of
the state or layout behind it. A scenario answers each regeneration it
asks for before its shots, unless the shot is of the wait (`-waiting`):
an unanswered one shows "Regenerating…" with the last answer's preview and
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
