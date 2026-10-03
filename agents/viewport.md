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
the Timeline, or what's selected in the model (see "Selecting" below), is
in a box of its own, with the key clearing it (`Space`);
then a bar with what's going on (picking a plane, the sketch's or the
extrude's status, regenerating, a failed edit, saving: nothing with
nothing selected), the hints, and the button of the view options menu,
which opens above it: Orthographic or Perspective, then Mouse hints,
without which the bar leaves out the hints of the mouse, Hidden
edges, without which the viewport leaves out the edges the model hides
(`Frame::hidden_edges`), and Wireframe, with which it draws the mesh's
wires too, every patch's edges that aren't feature edges
(`Frame::wireframe`). The app keeps those three for every document
(`Varde::options`, a `ViewOptions`, all but Wireframe on to start with,
not saved), and any of them closes the menu. The hints and the button show whole:
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
projection. A native wheel notch is a line (`ZOOM_PER_LINE`) and touchpads
give pixels; browsers give a notch as pixels too, tens of them, so on the
web a pixel zooms less and one event at most a line.

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
laid out (`viewport/extrude.rs`'s `hidden_by`: a ray from the knob
towards the eye meets a triangle of an opaque part (glass hides nothing,
as the shaft shows through it) more than 0.002 view heights in front of
it, as far as the renderer pulls the layers, so a knob on the cap it ends on
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

The renderer draws, in order: the background; the model's opaque parts'
faces (below for the parts less than opaque), or with `Frame::faded` the
whole model's depth and then only its nearest faces blended at
`Colors::faded_alpha` (depth still written); their feature edges, `EDGE_WIDTH`
logical pixels wide, depth tested and pulled towards the camera, so the
faces they bound don't hide them, at `faded_alpha` too when faded; the grid,
ray traced per pixel on `Frame::grid`, a `GridPlane` (the XY plane, or a
sketch's plane), faded out a few view heights from the target and at
grazing angles, with its two axis lines over it, coloured by the world
axis they lie along; the finished sketches (`Frame::sketches`, `RenderLines`), `LINE_WIDTH`
logical pixels wide, cut at the near plane, depth tested and pulled
towards the camera like the edges, so bodies in front hide them but a face
they lie on doesn't; the edges again where the model hides them, if
`Frame::hidden_edges` and not faded (below); the parts less than opaque
(below); the origin marker; in passes of their own, the geometry of the
failures shown (`Frame::errors`, see "Error geometry" below), faded or
not; and on top of it all the sketch being edited (`Frame::sketch`, a
`SketchScene`), not depth tested, so neither the faded model nor the
errors ever hide what's edited (with errors it's drawn in their last
pass, `Renderer::draw_sketch`). Setting up an extrude, the same layers are
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
finished sketch's line would. With parts less than opaque in the model,
the extrude's layers are drawn before them instead, so the glass in front
of them dims them (and the origin marker goes over them).

Each body is drawn as opaque as its `Body::opacity` (10 to 100 %), or
as its context menu's Opacity slider has it while that's dragged
(`DocumentState::opacity_preview`, `shown_opacity`; the menu is in
`agents/kernel.md`). The view hands the frame an alpha per part of the
mesh (`Frame::opacity`, from `MeshFeed::parts` and the document); a part
of no body in the document (a draft's new body), with no entry, or out
of range or NaN is opaque. It isn't part of the mesh, so changing it
uploads nothing. At upload the renderer keeps each part's index range,
its range of the edge stream (a part's points follow the last part's,
with another part's or none either side, which no segment joins) and its
triangles' bounds. Each prepare splits the parts into runs of opaque
ones, a draw each, and the rest, sorted far to near by their bounds'
centres along the view. Bodies are one colour each, so blending their
layers in another order changes only the shading: no order-independent
transparency is needed, and the sort is only wrong for bodies whose
bounds interleave. The alpha is per draw and multiplies the faces' and
edges' own. WebGL2 has no push constants, so a uniform buffer, written
once, holds a `PartUniforms` for each of `ALPHA_STEPS` (256, the 8 bits
the target shows) steps from 0 to 1 at the device's uniform offset
alignment, bound as group 1 of every pipeline at the step's dynamic
offset. A device whose buffers hold fewer gets fewer steps, at least an
opaque one, and only an alpha of 0 gets the first, so a faint part is
drawn a step up rather than not at all. In a sketch every part is drawn
faded alike at `faded_alpha`, whatever its opacity.

Outside a sketch, the passes round the glass (the parts less than
opaque) go:

1. The opaque parts' faces, then their hovered and selected faces (see
   "The highlight" below), then their edges. Their depth is the only
   depth there is until step 4.
2. The hidden edges (below) of every part against it, so a transparent
   part's edges an opaque one hides are dashed at its alpha too; then
   the transparent parts' visible edges, the extrude's layers, the
   hovered edges' outline, the selected edges and the hovered and
   selected vertices, all under the glass in front of them, which dims
   them.
3. Each transparent part, far to near: its back faces (front culled, lit
   as seen from inside: `fs_mesh` flips the normal of a face that isn't
   front facing), then its front faces, blended at its alpha, depth
   tested, writing no depth.
4. Their front faces' depth only (`glass_depth`), a part at a time far to
   near, each writing a stencil reference of its own (1 to 255,
   repeating) where it's the nearest of the model so far, and, with
   hidden edges on, followed by every part's edges it hides there
   (`hidden_by_glass`: `Greater`, stencil `Equal`), dashed at its alpha
   times theirs (`product_step`). So an edge behind glass shows solid,
   dimmed by it, and dashed at its alpha: a body nearly opaque hides
   edges much as an opaque one does, and two layers of glass give both
   layers' dashes. The depth buffer has a stencil for this:
   `Depth32FloatStencil8` where the device has it, else
   `Depth24PlusStencil8`.
5. Their hovered and selected faces, then their edges again, so the
   edges on the nearest surface show undimmed on the glass they lie on;
   then the outline, the selected edges and the vertices again, undimmed
   where they're in front of the glass.

The grid's axis lines are drawn in the grid's pass (`axis_line`) but not
faded with distance: they run on at full strength to the horizon, and
show when the plane is seen edge on. Each fades out on its own as it
turns to point at the camera (`AXIS_FADE`, by the sine of its angle to
the view direction, `facing`), where its image shrinks to a point; a
sketch's axes, drawn over them as `SketchLayer::axis_polyline`s (the
`FADES` flag), fade with them. Each pixel's coverage comes from its distance to
the line's image on screen, the homogeneous line through the images of a
point of the axis (the one nearest the target, so clip coordinates stay
small) and of its direction, exact at any distance, zoom and display
scale in either projection, so they're anti-aliased without MSAA (which
WebGL2 would make costly). Their depth is that of the axis's point nearest
the pixel's ray, written as the fragment's depth (clamped to the range,
so past the far plane they still show), and where that point is behind
the near plane the pixel isn't drawn, which also drops the part of the
image that's behind the eye. So bodies in front hide them like the grid.

The origin marker is flat: a ring lying in the grid's plane around its
origin (the world's, or that of the sketch being edited, where its axis
lines cross), and a dot at the origin itself, each a white core with a
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
`f32`; `shown` works out the cuts from the end nearer the middle of the
view, since mixing from an end far off, like a sketch axis's, lands
pixels off), divided through so fragments see pixel positions, and the fragment
shader turns the distance from the segment into coverage: anti-aliased at
any zoom and display scale without MSAA, with round ends and joins. A
segment of an edge or of the sketch being edited knows its neighbours, and
where two overlap at a join a pixel is drawn only by the nearer, so
translucent lines don't darken there; the finished sketches' segments come
alone and overlap at joins, which only thickens the fringe of an opaque
line. Where two edges meet at a corner their fringes overlap too, as do a
polyline's segments shorter on the screen than the line is wide. Dashes
run along a polyline by its length (in sketch units at the target's
scale, or logical pixels on the screen), box filtered over what a pixel
spans along the line, so their ends fade over a pixel, and dashes shorter
than a pixel (an edge seen nearly end on, or far off in perspective) blur
to their average rather than beating.

The edges are uploaded as a stream of points (`EdgePoint`, 20 bytes:
position, how far along its polyline in world units, and which polyline),
polyline after polyline, a point repeated in a row left out, with a point
of no edge at each end: every part's edges, then every part's wires
(numbered after the edges), so a run of parts' edges, or their wires, is
one draw (`GpuPart::points`, `wires`); the wires are drawn only in a
wireframe, a second draw after the edges'. A closed edge of three
segments or more has its last but one point before it and its second
after it, marked neighbours only (`NEIGHBOUR_ONLY`, the edge's top bit),
so its ends join like any other. That's at most one and a half points
per edge or wire vertex, so `RenderMesh::MAX_EDGE_POINTS`, which bounds
the two together, fits a 256 MiB buffer. Creases (an edge with one face
both sides, see `agents/kernel.md`: patches whose normals part inside a
face, as where a boolean's cut leaves fitted patches on a cylinder) and
wires are marked `CREASE` (the next bit) and drawn `CREASE_WIDTH` (1)
logical pixel wide at `CREASE_ALPHA` (0.35) of the edges' opacity, seen
and hidden, in the same quads, so they read as how a face was cut into
patches rather than where it ends. The buffer is bound
to four vertex buffer slots a point apart, step mode instance, so
instance `i` sees points `i` to `i + 3` as the point before, the
segment's ends and the point after (`EdgeIn`, `edge_segment`): the
segment is drawn if its ends are of one edge, joined to the points
either side that are of it too. That's 4 of WebGL2's 8 vertex buffers,
9 of its 16 attributes and no storage buffers. A segment and its
neighbours are cut at the near plane before pulling (`pulled_segment`,
shared with the finished sketches), so they agree where they meet.
Pulling (`pulled`) only changes the depth: the line shows where it is,
not where the pulled point would in perspective.

The edges the model hides are the same stream drawn again
(`vs_hidden_edge`) with `depth_compare: Greater` against the opaque
parts' depth. The quads are the same (`EDGE_WIDTH` wide, the hidden pass
narrowing only the coverage, the position `@invariant`), so the two
passes split the pixels and a visible stretch meets a hidden one without
a gap or a pixel of both. They're `HIDDEN_EDGE_WIDTH` (1) logical pixel
wide, in the edge colour at `Colors::hidden_edge_alpha` (0.45), dashed
(`HIDDEN_DASH`, 4 on and 3 off) by length at the target's scale. The
dash phase is worked out per segment (where it starts along its edge,
modulo the period, then again from where it's cut to the viewport), so
the numbers the fragment shader mixes stay small; zoomed far into a long
edge only that phase is off by `f32` rounding, not the dashes. An edge's
fringe within a pixel of a silhouette is depth tested at the edge's
depth, so a back edge leaks a little past it there; avoiding that would
mean sampling a copy of the depth buffer. Not drawn faded (in a sketch
the model already reads as see-through), but drawn setting up an
extrude.

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
share) and placed on an origin plane's placement, or a sketch on a face
at the placement regeneration found for it (`Evaluation::placements`;
one not placed isn't drawn: see `agents/features.md`).

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
the mesh), by the mesh's own ids: the body of each part (the shown
bodies in order; `Picking::face_body` finds a face's), each face
(`PickFace`: the face key, its aliases, sorted, and a `Summary` of its
form: a plane's outward unit `n` and `d`, a cylinder's point, axis and
radius, a cone's, sphere's or torus's numbers, a conic cylinder's
direction, a revolved conic's axis, else `Other`: no known form, or
numbers past the bound), for each edge whether it's a chain closing
on itself, its tangent chain (below) and its snap point
(`Picking::snaps`: a straight edge's middle or a round edge's centre
from `measure::edge_shape`, else none, a crease's none), and the
corners (`PickCorner`: three of the faces meeting at a vertex where
three or more meet, the lowest regions ascending, as face ids of one
part, and the vertex's point exactly, in f64; `Picking::corner_keys`
gives their keys sorted, as a corner reference stores them). The snap
points and corners are the measure tool's points (and later align's and
scale's). Everything else is the mesh's
to say: its faces are the kernel topology's regions and the first edges
of each part its chains, the creases after them (see "Tessellation" and
"Topology and names" in `agents/kernel.md`), so a circle's quarter walls,
or flush faces merged under one name, are one face, a face cut in two by
a groove is two faces of one key, a triangle's face is the `face_ends`
range holding it, and an edge's faces are its `edge_faces` (a crease's
the same face twice). `Picking::edge_keys` gives an edge's two keys
sorted, as an edge reference stores them (none for a crease). Each
body's tables are made with its mesh (`Solid::topology`, kept in the
cache for the measure tool too, then `Solid::tessellate_with`) and
cached with it, counted in its bytes; the scene's are theirs joined as
the mesh is, the tangent chains and corners' faces moved on (checked).
The fields are private and `Picking::from_parts` checks parts from
elsewhere against the mesh (a body per part, a face per face, a flag, a
tangent chain and a snap point or none per edge, a closed edge between
two faces and on one corner, a crease without a snap point, each corner
between three faces of one part, ascending, no more corners than the
mesh's vertices, summaries, snap points and corners' points finite and
within `Picking::MAX_VALUE`, their directions unit vectors, aliases
sorted apart from the key). The app
keeps them with the mesh; `MeshFeed::parts` gives the parts' bodies,
none once the document was replaced whole until a model of it shows
(the ids may name other bodies), so the parts are drawn opaque
meanwhile.

**Measuring** goes through regeneration too (`regen/src/inspect.rs`):
`Request::Regenerate` carries `inspect: Option<Inspect { revision,
first, second }>`, each an `InspectPick { body, entity, near }` whose
`Entity` is `Body`, `Face(key)`, `Edge(keys)`, `EdgePoint(keys)` or
`Corner(keys)`, the keys a reference stores (`Picking::edge_keys`,
`corner_keys`) and `near` the point picked at. Regen resolves each on
its body's `Topology` as references resolve (by key or alias, the
nearest to `near` among several) on the model the answer draws (a draft
applied if it worked), a body a join merged into another on the body
holding it (`Evaluation::holder`: its faces and edges keep their keys
there, and the body whole is the holder; its places are the holder's),
so the same picks sent after an edit measure what they name now, and answers `Response::Regenerated.inspected:
Option<Inspected { revision, first, second, between }>`: per pick
`Err("face not found")` (or edge, corner, "body not found", "the pick
has no point" for a face, edge or corner whose `near` isn't finite: of
several of one name it would take the lowest, maybe not the one
picked), else `Probed { at, measure }`, `at` its entry in the answer's
own mesh and tables (`At::Face` a face id, `Edge` an edge id for an
edge or its point, `Corner` an index into the corners; `None` for a
body or a hidden body's entity; the entry found from the body's part
(its faces and edges, its run of corners) and checked to be the
region's key, the chain's faces, the corner's faces and point, else
`None`), `measure` a `Measure` (`Body` volume,
area, centre, tight box; `Face` area, its form's `Summary`, a cone's
half-angle; `Edge` length, closed, `EdgeForm` line/circle/ellipse;
`Point`) or "too complex to measure"; `between` (both found) the
`Gap` (distance and the two points) or its error, and the angle between
their directions (`measure::angle`). Each measure runs within
`Budget::DEFAULT`, the most a kernel operation may do (`Budget::new`
caps at it): coaxial revolved faces can take ~10M units, which then
come back "too complex to measure". Measures, distances and topologies
are kept in the cache (outside its feature counts), keyed by the bodies'
solid keys, the resolved picks and the fit, so picks asked again, or
after an edit elsewhere, cost nothing. `Response::Failed.inspect` and
`Response::inspect()`/`Request::inspect()` give the revision. The
inspect rides the regeneration slot, latest wins, rather than queueing
as exports do: the newest request always carries the session's current
picks, so a request it replaces had only an answer the panel would drop,
and a measure is always of the model the same answer draws (its `at`
indices name that model's tables). The app must count the revision into
what it compares to tell a new request is needed and an answer is the
one asked for last, as it does a draft's. The cost of that choice: a
measure running (up to the full budget, as coaxial curved faces can
take) holds the slot, so an edit's model waits behind it, and
while a draft is dragged with picks on the drafted body each step
would measure again (the solid is new each step). It doesn't arise: the
measure tool and the operations being set up never run together (one
starting ends the other), so no request carries both a draft and picks.

**The measure tool** (`app/src/doc/measure.rs`, `Doc::measure`, a
`MeasureSession`; `view/src/measure.rs`; `view/src/viewport/measure.rs`)
writes nothing to the document, has no undo and leaves the selection as
it was. `I` (`Shortcut::MEASURE`, outside sketches, where `I` is
Coincident's), the toolbar's Measure button (after a separator, as the
mock has it) and the rail's Inspect set (the model rail's second, `W`;
its list's letter `I`) send `Look::StartMeasure` through
`shortcut::measure_binding`: enabled outside sketches and the operations
being set up, read-only documents included; again, `Esc` or the panel's
Close (`MeasureLook::Close`) leave it, and starting a sketch, an extrude
or a revolve, editing a feature, or replacing the document whole end it
(`Doc::prune_measure`). While it's in use the cursor picks the model as
outside the sessions (`Doc::picks`), faces, edges and vertices whatever
the selection's mode, and with `ModelPicking::snaps` the snap points of
what it's over (`PickIndex::snaps`: a face's corners, the corners naming
it among their three faces; an edge's ends, the corners naming both its
faces, and its own point, `Picking::snaps`; a vertex's corner, the one
of its faces at its point): the nearest showing within
`SNAP_REACH` (10) pixels is taken (`PickIndex::snap`, `Pick::snap` a
`Snapped::Corner` or `EdgePoint`), and while the cursor is within reach
of a dot of what's held hovered it keeps that hovered and takes the dot
though it has left it, so a round edge's centre, off the edge and often
over nothing, can be reached from the edge. `Look::Hover` is sent when
the target or its snap changes. Clicks (`Look::ClickModel`) go to the
session (`MeasureSession::click`): the first picks A, the second B, a
third A again without B; with `Shift` (or `Ctrl`, `Held::TOGGLE`) it
picks B (A while there's none); the second click of a double-click makes
what the first picked its body; a click off the model lets go of both;
a body's row in Objects picks the body (B with `Ctrl`). A pick is an
`InspectPick` by name: a snapped corner by its three keys
(`Picking::corner_keys`) at its exact point, an edge's point by the
edge's keys at the point on the edge picked, a face by its key, an edge
by its sorted keys, each at the point picked, a body by id; a vertex
with no corner taken (past the table's bound) picks nothing. The session
sends A and B with every request (`MeshFeed::request_with`'s `inspect`):
the feed gives picks differing from the last the next revision (counted
over the document's life, as drafts are), asks again when they change,
and counts the revision into `Asked`, so an answer of the same
generation is taken only for the picks asked last and an older one is
dropped; `MeshFeed::inspected` gives the measure of the model shown only
while it's for the picks asked last (after an edit, the old model's
answer for the same picks shows until the new one comes, as the model
does). After an edit regen resolves the same picks in the new model; one
it doesn't find shows its reason under its row ("Face not found") and is
kept, so an undo finds it again.

The highlight while measuring is the session's own (the selection's
isn't drawn and stays as it was), a `ModelHighlight` as the
selection's is (`PickIndex::highlight_with`): what's hovered, drawn as
it is outside the tool, A drawn as selected (the accent: faces tinted,
edges in it) and B in the second colour (`Colors::second`, the
construction colour: `ModelHighlight::second_faces` tinted as selected
faces are, `Highlights::second_edges` drawn as selected edges are, both
over the selection's; the renderer keeps its red, green and blue in the
sketch plane's unused uniform w's, the uniforms being full), built only
from the newest answer's `Probed::at` (a face's, an edge's; a body's
faces from the body id, or its holder's for a body a join merged,
once its measure came back), so it always names entries of the model
shown; a point is drawn as a dot instead. On top of the model (not depth
tested, so a distance through the plate or a point behind it shows) the
viewport (`Measuring`, one of `viewport::Operating`) draws in its live
layer the hovered target's snap dots (the one taken bigger and filled),
A's and B's points as dots in their colours, and the minimum distance
between its two points, dashed, with its ends (none for picks that
touch, at 0); the distance's label is a widget anchored at the
segment's middle, in the knobs' layer. World points are
`SketchLayer::world_point` (a `PointInstance` flagged `WORLD` with its
z, as lines and fills are).

The panel (`measure::panel`, the operation panel with only Close,
`Parts::close`) has a row per pick with its tag ("A" in the accent, "B"
in the second colour, `theme::pick_tag`, its letter white where that
reads at 3:1, else dark: dark on the dark palette's light accent and on
both construction colours) and its name ("Planar face of
Body 1", "Circular edge of Body 1", "Point of Body 1", "Body 1"; the
kind from the measure's form once it's back) or what to click. With one
pick its values follow; with two, what's between them (Distance and its
ΔX, ΔY, ΔZ from A's point to B's, Angle where both have a direction),
then each pick's own values under a header folding them away
(`MeasureLook::Fold`), folded to start with. Values
(`measure::values`, `between_values`): a point's X, Y, Z; an edge's
length and a line's direction, a circle's radius, diameter and centre,
an ellipse's semi-axes and centre; a face's area, a plane's normal, a
cylinder's or sphere's radius and diameter, a torus's radii, a cone's
half-angle; a body's volume, area, centre and box (from, to, size).
Lengths, areas and volumes are in the design's units, squared and
cubed (`varde_expr::format_power`: 3 decimals in mm, 4 in inches, as
lengths show), angles in degrees; "Measuring…" while the answer is on
its way, the kernel's refusal ("Too complex to measure") in red. Each
value has a copy button sending `Message::Copy` with it at full
precision and its unit (`varde_expr::full`, `full_power`: the shortest
decimal that reads back as the same `f64` in that unit, "12.7 mm",
"0.3937007874015748 in", "1 in²"); natively the app hands it to iced's
clipboard (`platform::copy`), on the web straight to the browser's
Clipboard API while the click still counts as the user's (iced's does
nothing there). The status bar hints "Pick A" or "Pick B", `Shift`-click
"Replace B", a double-click "Body" and `Esc` "Done"; the toolbar's tag
says "Measure".

**Picking the model** (`view/src/pick.rs`, the app's `doc/pick.rs`) is on
the CPU, against the mesh drawn and its tables; no GPU id buffer (WebGL2
readback stalls or is a frame late). The feed keeps the answer's
`Picking` with its mesh and counts the models shown (`MeshFeed::model`,
up whenever the mesh or the tables change; natively an unchanged scene
comes back as the same `Arc`s, from the web worker as copies, which are
compared, and either way it keeps its index). A
`PickIndex` (the mesh, the tables, the model's count, a bounding volume
hierarchy over the triangles, one over the segments of the edges
between two faces and one over the vertices, the faces at each corner
an edge between two faces ends at, and each tangent chain's edges) is
built the first time it's
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
the nearest triangle it meets the front of, as the renderer draws them
with their backs culled, names the face (the one whose `face_ends`
range holds it). An edge between two faces (not a crease) wins over it
if a segment of it shows within `EDGE_REACH` (6) pixels of the cursor
and isn't hidden: the candidates, found through the edge tree with each
node's box grown by 6 pixels at its deepest, are cut at the near plane,
measured on screen, sorted by distance (to `SAME_PLACE`, half a pixel),
depth and index, and the first of at most 64 whose point showing
nearest the cursor (found back in the world with the perspective divide
undone) no triangle hides wins: hidden means the ray from the eye to it
meets a triangle nearer by more than 0.002 view heights (what the
renderer pulls edges by) and the mesh's `f32` rounding. So an edge
either side of a face shows, and one behind the plate isn't picked. A
vertex wins over both, found the same way within `VERTEX_REACH` (6): a
corner where three faces or more meet (those of the edges between two
faces ending there), so a box's corners are vertices and a hole's rim
has none. Distances within half a pixel go by depth so that of two
things in one place (a corner seen straight down the edge to the one
behind it) the nearer is tried first: the ray from the one behind runs
along the faces between them, where the hidden test can miss them. With
`Picks::Faces` or `Picks::Edges` only that kind is picked. The `Pick`
carries the model's count, the target (`Picked::Face`, `Picked::Edge`
or `Picked::Vertex`, the mesh's ids, a vertex by its corner), the body
(the face's part's, or the first face's at the edge or the vertex) and
the point (the ray's hit, the edge's point, or the vertex).

Outside sketches and the extrude session, and not over a draft's preview
(`Doc::picks`; they pick what they need themselves) the viewport is given `ModelPicking` (the index,
the target the app holds hovered, and what the cursor picks, `Picks`:
all, or only faces or only edges, from the selection's mode). It
picks on each cursor move while the camera isn't dragged, and on each
frame drawn (`RedrawRequested`) whose camera, model or cursor position
differs from those it last picked with (`Interaction::hover_seen`), so the
hover follows a zoom, the camera's animation or a new model under a
cursor that stays; only when the target differs from the app's does it
send `Look::Hover(pick)`, and `Hover(None)` once the cursor leaves the
model or the viewport, or the camera is dragged (past a click's slop:
from the first frame drawn while it is; the hover is worked out again
once the drag ends). The cursor is a pointer while
something is hovered. While a plane is picked for a sketch
(`ModelPicking::planes`, the `PlanePick`) only faces are picked, a click
on one that takes the sketch sends `Edit::FacePicked` instead of
selecting and a click elsewhere nothing, and only such a face hovered is
highlighted (`Doc::shown_hover`) or gets the pointer
(`ModelPicking::takes`, `PlanePick::takes`: flat, and for a sketch whose
plane is changed, of a body made before it); the status bar says why
another isn't (see `agents/sketch.md`). A pick of a model no longer shown (its count
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
`SelectionMode`: `Any` outside the sessions (the face, edge or vertex
clicked; a double-click its body), `Faces`, `Edges { tangent }` (with the whole
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
(`Selected::Edge`), a vertex as its body, the lowest three keys of the
faces meeting there, sorted (as a corner reference names one), and the
point (`Selected::Vertex`), a body by id. Each item also holds its target in the
model it was last found in (`Selection::model`). When another model
shows (an edit, an undo, a tolerance), `Selection::resolve` finds each
again as the kernel resolves references (`PickIndex::find_face`,
`find_edge`, `find_vertex`): the faces of that body named by the key
(key or alias), the edges between faces so named either way round, or
the vertices where faces so named meet; one is taken
wherever the point is, of several the nearest to the point (measured to
the drawn triangles or segments; a later one counts only where it comes
nearer by more than a billionth of the mesh's size, so ties go to the
lowest; the first where the point isn't finite). A face, edge or vertex of a
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
regen carries it per edge of the mesh as `Picking::tangents` (a crease
its own; checked on the page: the first of a tangent chain is an edge
between two faces of the same part, no later than its members and its
own first), and `PickIndex::tangent_chain` groups them. Hovering in
`Edges { tangent: true }` highlights the whole chain, in `Bodies` the
whole body.

The app's highlight (`Doc::highlight`, a `ModelHighlight`) is
`Selection::highlight`: what's selected (a body as all of its faces)
and what's hovered, in the mode (a tangent chain, a body's faces), by
the mesh's ids (`PickIndex::highlight`, which leaves out ids the mesh
hasn't). That's the faces hovered and selected and a small layer of
edges and vertices (`varde_render::Highlights`): the hovered edges
outlined (not a hovered face's: they're drawn as they are), the
selected edges, and the vertices hovered or selected, flagged which. It's rebuilt only when
the model, the target hovered or the selection changes, so moving over
one face uploads nothing; the renderer rewrites the layer only when it's
another `Arc` or the mesh is (it holds ids, the positions come from the
mesh). With nothing to draw every frame gets one shared empty highlight.

The status bar's box tells of the selection when no feature is
selected: one face as "Face", its surface ("Plane", "Cylinder", "Cone",
"Sphere", "Torus", "Curved") and its body's name; one edge or vertex as
"Edge" or "Vertex" and the body; one body by name and "Body"; several
as "N selected" and how many faces, edges, vertices and bodies. Its hints: "Select" and a double-click
"Body" with nothing selected, then Shift-click "Add or remove" and
"Body", before the camera's.

The **highlight** reaches the frame as the faces hovered
(`Frame::hovered_faces`), the faces selected (`Frame::selected_faces`)
and the layer (`Frame::highlights`). Hover changes no colour but the
hovered faces': the hovered edges keep theirs, the rest of the body its
own. Selection is in the accent (`Colors::selected`). In a sketch
(`Frame::faded`) none of it is drawn; the app doesn't pick there anyway.

- Faces: drawn again over themselves by their index range, a draw
  each, so nothing per vertex, with `vs_mesh` (its position
  `@invariant`), `depth_compare: Equal` and no depth written, so exactly
  their own pixels: the hovered ones lit in `Colors::hover_face` (a
  neutral grey, lighter than the model) at their part's alpha
  (`fs_hover_face`), then each selected one blended
  `Colors::selected_tint` (0.3 light, 0.6 dark) of the way to the accent
  (`fs_selected_face`), so a hovered selected face is tinted and
  brighter. An opaque part's are drawn before anything else writes depth
  where they are, so one behind glass is dimmed by it but still tinted;
  a transparent part's after the glass's depth, so only where it's the
  nearest glass (behind other glass it shows untinted).
- Outline: the outlined edges' polylines in a stream of their own
  (`EdgeStream`, as the mesh's), drawn with the edges' quads, depth
  tested and pulled like them, so only what shows is outlined: the edge
  drawn again `HOVERED_EDGE_WIDTH` (2.5 logical pixels) wide in the
  edges' colour, opaque whatever its part's alpha (`vs_hovered_edge`),
  within a rim `HOVER_RIM` (1.5) wide either side of it in
  `Colors::hover_outline` (`vs_outline`), white, brighter than the lit
  faces. The rim is hollow: its coverage is a line `HOVER_RIM` wider
  than the hovered edge less the edge's own (`style.w` in `fs_line`),
  drawn before the edge. The hover's and the selection's lines and
  vertices (`fs_highlight_line`, `fs_highlight_point`) write their
  depth pulled in further the farther a pixel is from their middle
  (`highlight_slope`: `HIGHLIGHT_SLOPE`, 3, pixels' worth of the world
  per pixel), so a face rising steeply towards the eye from the edge,
  as a wall does from its bottom edge seen from above, doesn't hide
  their outer pixels on its side; their middle is hidden as the edges'
  is. The outlined
  edges meeting at a corner are joined into one polyline
  (`Highlights::build`, either way round, until the loop closes; past
  two at a corner the others start their own), so neither's rim covers
  the other's middle there. A rim knows only its polyline's segments and
  their neighbours, so where it comes within `HOVER_RIM` of another's
  middle elsewhere (a third edge at a corner, the two sides of a face
  seen nearly edge on), it covers it.
- Selected edges: the same stream's other range (a point of no edge
  between them, so an edge both outlined and selected isn't joined to
  itself), `SELECTED_EDGE_WIDTH` (2.5) wide in the accent shaded by
  `Colors::selected_edge_shade` towards black or white (a little darker
  in the light theme, over a light face tint; much lighter in the dark),
  so it shows on a selected face's tint (`vs_selected_edge`,
  `selected_edge`), over the outline, within a hollow rim `SELECTED_RIM`
  (1 logical pixel) wide either side in white at half alpha
  (`vs_selected_outline`), for contrast with what's behind it.
- Edges in the second colour (`Highlights::second_edges`, the measure
  tool's B): a third range after the selected, drawn as those are but
  in `Colors::second` unshaded (the dark theme's shade, far towards
  white, would wash it out) (`vs_second_outline`, `vs_second_edge`); its faces (`Frame::second_faces`) are tinted as
  selected faces are, in it (`fs_second_face`), after them.
- Vertices: only those hovered or selected, an instance each
  (`VertexInstance`: position and flags), drawn as a sketch point is
  (`vs_vertex` into `fs_highlight_point`): a disc of `VERTEX_RADIUS`
  (3.5 logical pixels) within a rim, depth tested at its centre's pulled
  depth. Hovered, the disc is in the edge colour, its rim `HOVER_RIM`
  wide in `hover_outline`; selected, it's filled with the selected
  edges' colour, within the selected edges' faint white rim, or the
  hover's if it's hovered too.

**Error geometry** (`Frame::errors`, a slice of `ErrorParts`: a failure's
patches as a `RenderMesh` of triangles only, its curves as `RenderLines`,
its points, and the `Weak` of what they're parts of, the failure's `Arc`,
since the renderer can't name regen's `ErrorGeometry`; holding the `Weak`
keeps the allocation from being reused, as `Slot::source` does) is drawn
after everything but the sketch being edited, in two passes of its own,
faded or not: solid red
(`Colors::error`) within a halo of the same red at 0.3
(`Colors::error_halo`) reaching `ERROR_HALO` (12 logical pixels) beyond it
on every side; curves `ERROR_EDGE_WIDTH` (3) wide, points discs of radius
`ERROR_POINT_RADIUS` (4), patches filled red and lit as faces are
(`fs_error_face`), their boundary among the curves. Both themes take the
app's red, `#e0564b`, as a sketch's conflict and the danger button do.
The slot keeps the sources and uploads the errors again (all together:
the patches' triangles a corner at a time, unindexed, the curves as an
`EdgePoint` stream, a polyline each, the points as `VertexInstance`s)
only when one differs, as `highlights` is; their box joins the depth
range's bounds. A point not finite or past `RenderLines::MAX_POSITION`
isn't drawn. Past the device's buffer size they're skipped and reported
once (`PrepareError::ErrorsTooLarge`). Errors with nothing to draw (no
triangles, curves or points) count as none: no passes, no target. Parts
`halo_only` get their halo but no core: of each kind those drawn whole
are built first, then the rest, and the core's draws stop at the first
ones' counts (`Cores`), the halo's take all. The view adds, after the
failures shown, the sketch being edited's failing curves (`ErrorParts`
of lines only, `halo_only`, keyed by their `Arc<RenderLines>`), for their
halo under the sketch's red curves at the sketch's own width: a red
core under them would show as a thicker line around a thin one, and
around a selected curve's blue.

**Which failures show** is the app's (`app/src/doc/errors.rs`,
`Doc::shown_errors`, a `varde_view::ShownErrors`): the draft's geometry
while an operation is set up and the newest answer for its draft as it
is fails with some (`MeshFeed::draft_geometry`, as the panel's error
is); each failed feature's whose Timeline row is hovered
(`Look::HoverFeature` on the row's mouse area's enter,
`Look::LeaveFeature(id)` on its exit, which lets go only of that row:
moving up, the row above tells it's entered before the one below that
it's left; outside a sketch, and let go of when the side panel's tab
changes, by a click or the peek key, on entering a sketch, and when the
feature is removed or the document replaced, since the rows go without
an exit) or selected, or whose panel is open, the edited feature's only
while its draft has none, and not at all once a draft of the current
run is shown (`MeshFeed::draft_shown`: the model shown's failure for it
is then a draft's, the one before while a changed draft is on its way,
whose error the panel doesn't show either); nothing otherwise, so a model
with an old failure isn't covered in red. A sketch's own failure is a
failed feature's as any other (its row shows the face it couldn't be put
on, for not being flat). While a sketch is edited, a
failure of a feature using it that names its curves is drawn without
its curves (`ShownError::lines` false), selected or not, as long as it
has points or patches: the sketch marks those curves itself, red within
the same halo (`agents/sketch.md`, "Failing curves"), and the copy in
its plane would draw them twice, and late while a drag moves them; but
its points (where a profile touches itself, an open gap's ends, an axis
line of no length's place) are the
most precise part of it, and the sketch has nothing to mark them with.
So, like the curves marked, every such failure's show while its sketch
is edited, not only one selected. A revolve's axis crossing the
profile, a curve of that failure that isn't the sketch's, goes with
them. All of it is the model shown's (its answer brought it), and each
`Arc` is shown once.
`Doc::refresh_errors` (after `sync`, `look` and an answer) picks them
again and makes a new `ShownErrors` only when they're other `Arc`s, in
order, than it shows: an unchanged failure, the same `Arc` from the
regeneration side's cache, keeps what's drawn. `ShownErrors` holds each
geometry with its `Weak`, downgraded from the live `Arc` once as it's
made (never `Weak::new()`, which would compare equal to any dangling
one), or for one drawn without its curves a `Weak` of a fresh `Arc<()>`
(held, so its allocation isn't reused), so the same geometry drawn whole
then without is uploaded again; a frame borrows them into `ErrorParts` (`ShownErrors::parts`, a
small vector a frame, since the parts borrow), so the renderer uploads
only when they change. The operand faces an `ErrorGeometry` names
(`faces`) are drawn through its mesh and lines: regen copies their
triangles from the model into it, and their outlines into its lines,
when it resolves them (see `agents/features.md`), so the viewport draws
nothing of its own for them.

**Show** frames the camera on a failure's box (`Look::ShowFailure`, a
feature's or with `None` the draft's; `ErrorGeometry::bounds`): the
camera turns as Home does, keeping its direction, its target to the
box's middle and its view `FRAME_MARGIN` (1.5, as a sketch entered is
framed) times the box's diagonal tall, at least `MIN_FRAME_HEIGHT` (1 mm), and the pivot picked is let
go of. A Show button sits at the right of a failed feature's Timeline
row (not in its tooltip: iced's tooltips can't be clicked) and beside
the error in an operation panel's footer (`footer_message`'s `show`,
the states' `show_error`), only where the geometry has a box.

- The halo is coverage, not colour: halos overlap everywhere (a
  polyline's joints, two segments that touch, a point on a curve), and
  blending each over the frame would darken the overlaps unevenly. So
  the scene's pass keeps its depth (stored, where it's discarded without
  errors), and a pass into an `R8Unorm` target of the target's size
  (`ErrorTarget`, made the first time there are errors, again on a
  resize while there are, kept while there are none so a hover showing
  and hiding them makes no texture), cleared to 0, with the scene's depth loaded, draws each kind's
  coverage with `BlendOperation::Max`: the curves through the edges'
  quads and `fs_line`'s coverage (`vs_error_halo_line`, `fs_halo_line`,
  writing the alpha `line_color` gives, which `output` leaves as it is)
  `ERROR_HALO` wider either side, the points' discs as wider
  (`vs_error_halo_point`, `fs_halo_point`), the patches filled
  (`fs_halo_face`).
- A pass over the frame (the scene's depth loaded again) then composites
  it once (`fs_error_halo`, a full-screen triangle reading its own texel
  with `textureLoad`, no sampler) as `error_halo` at the coverage, and
  draws the core over it, a kind at a time: patches, curves
  (`vs_error_line` into `fs_highlight_line`) and points
  (`vs_error_point` into `fs_highlight_point`); then the sketch being
  edited. The composite and the core bind a group 0 of their own
  (`ErrorTarget::group`): the scene's uniforms, the errors' colours
  (`ErrorUniforms`, `ErrorColors` in the shader, written each frame there
  are errors; the scene's uniforms are full) and the halo's texture. Not
  a third group: iced asks the device for `max_bind_groups: 2`. The
  sketch drawn after them binds the scene's group 0 again. The halo's
  own pipelines take the scene's layout, since they draw into that
  texture.
- Each is drawn twice, by the same programs with another depth test, so
  the two split the pixels: `LessEqual`, at full strength, and `Greater`,
  where the model hides it, at 0.4 (`ERROR_HIDDEN`), core and halo
  alike, as hidden edges are dimmed. The strength is the part's alpha
  (the `Alphas` step), which the core's colours and the halo's coverage
  multiply by. In the core the hidden draw goes first, so what shows of a
  kind goes over it; in the halo order doesn't matter. Hidden patches
  that overlap (two failures', or a curved patch's own folds) would blend
  twice and darken, so the hidden patches' draw is stencilled: the
  stencil is cleared to 0 (every pass clears it), the reference, and the
  first drawn at a pixel increments it, so later ones fail there. Curves
  and points over a hidden patch still blend over it, outlining it as
  they do where it shows. Seen patches are opaque and don't write depth,
  so of two overlapping where they show the one drawn last is on top
  rather than the nearer: both are the same red, lit. Curves and points
  are pulled in by their distance from their middle as the highlight's
  are (`highlight_slope`), patches as the edges are (`pulled`), so the
  faces they lie on don't hide them; but a patch's corner before the
  near plane isn't pulled, so its triangle is clipped there as the
  model's are (`pulled` keeps a point at the plane, and the triangle
  unclipped would show what's between the plane and the eye across the
  view). The halo's quads are pulled by the same slope, out to their
  edge (`ERROR_HALO` beyond the core: 40 pixels' worth of the world
  there), so the halo of a curve on a face seen slanted shows evenly both
  sides; the cost is that the halo of a curve hidden just under a face
  (within about that depth) shows at full strength away from it while
  its core is dimmed. Geometry inside glass is hidden by its depth, so
  drawn at 0.4 there.

On WebGL2 all of it holds: `R8` is colour-renderable and blendable in
OpenGL ES 3.0, `MIN`/`MAX` are core blend equations there, `textureLoad`
is `texelFetch`, and every layout is within the two bind groups iced's
device has (the render tests' devices ask for that limit too). The GL tests draw it on wgpu's GL backend
(`errors_and_their_halo_are_drawn_on_gl`), the halo off the middle of
the target, so one read upside down would show.

Pipelines that differ only by what they draw each have their own entry
point, since wgpu's GL backend keys programs by module and entry point. The uniforms stay within 512 bytes (the limits
tests' device's largest buffer): the grid's axis lines' colours are
picked in the shader by which world axis each lies along (the `w` of
`grid_x` and `grid_y`), the hidden edges' alpha is `edge`'s `w`, the
selected edges' shade `selected`'s, the selected faces' tint
`hover_face`'s, and the model's faded alpha `model`'s. The errors' colours have a
uniform of their own (`ErrorUniforms`, above), bound only where they're
drawn.

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
document (`varde_document::codec`) and the sketch to leave out, and
the answer is a postcard head (with the failed features, the
bodies each join, cut or intersect touches, the bodies' boxes, the
draft's outcome, the picking tables and the measure's answer) and the
mesh's vectors and the sketches' line points and ends as raw bytes. A
model whose head would be over its bound (64 MiB), or with more faces,
corners or aliases than a reply may carry (2²⁰, 2²² and 2²⁰ all faces'
together; the tables are decoded within those bounds and the mesh's, so
a short head can't make the page build more), is answered as failed. A
measure's answer is checked against the decoded mesh and tables
(`Inspected::checked`, also run where it's made: numbers finite, sizes
not negative, points within `Picking::MAX_VALUE`, directions unit,
`at` within its table, an edge a chain, and of the measure's kind (a
face's, an edge's or its point's, a corner's point, none for a body),
angles within range, a distance its points'); one that fails becomes
errors ("the measure came back broken") with the model taken as usual. Both
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

## Thumbnails

A save's thumbnail (see "Thumbnails" in `files.md`) is drawn by the
viewport, which has the GPU: `DocumentState::thumbnail` hands the
viewport a `ThumbnailRequest` (`view/src/thumbnail.rs`: the mesh, its
parts' opacities, and the `varde_render::PreviewShot` that
`varde_render::frame` worked out: the home camera made orthographic,
targeting the middle of the mesh's extent across the view, zoomed so it
fills the room less the margin, and the image cropped to it). The first
frame prepared with it takes its callback (once) and calls
`varde_render::render_preview` beside the frame: a slot and a texture of
its own, in the pipeline's format, cleared transparent, the model drawn
by `Renderer::record` without the backdrop (background, grid, finished
sketches, origin and pivot markers), lines at a scale of two, then
copied to a buffer and mapped. Natively it waits for the GPU there, a
small image's worth; on the web the buffer maps on a later submit, which
the frames drawn while a thumbnail waits bring. The pixels are read back
as straight alpha sRGB RGBA (`Layout::straight`: drawn over transparent
black they're premultiplied, so the alpha is divided out in the space the
target blends in, linear for an sRGB format); only 8 bit RGBA and BGRA
targets are read, another fails, and the save goes without.

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

Scenarios (`shots_01` .. `shots_27`, each at 1280×800, scale 1, light,
the busiest also at scale 2 and dark): `X` with every candidate's regions
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
Timeline row selected and the rail's Create list; a ball and a torus
turned about one line (`shots_21`, also scale 2, and close up from
above, dark), for their silhouettes; the measure tool
(`shots_22`): `I`, a corner of the top hovered with its face's or edge's
dots, the corner picked and the hole's rim's centre reached from the
rim, the distance between them with its segment and label (light,
dark), the top and the rim highlighted in the two colours with the
rim's values unfolded, and the body double-clicked in inches; the
combine (`shots_23`): `B` with nothing picked, the plate the target
and a disc a kept tool in their two colours with the other disc
hovered, a union using both discs up (Objects listing them faint), a
subtract (light, dark), its Timeline row, and a disc's extrude edited
into a join, its panel saying why it stays a new body; sketches
on faces (`shots_24`): `S` with the top hovered, a circle in a sketch on the top
with the Sketch tab naming the face, its row menu with Change plane, the
first sketch's plane changed with the later top hovered (refused, red),
and the top's sketch failing once the plate is gone, its tip and the
plane asked for; the banners of a file found damaged and of a damaged
auto-save (`shots_25`); the welcome screen's prompt about a file damaged
past the save opened, with a save found and with it failing to open
(`shots_26`, drawn with `Shooter::take_view`, from a `Varde`); a
save's thumbnail of the example's plate, rendered by the viewport's
frame and written as `27-thumbnail.png`, then the welcome screen showing
it in a recent file's card beside one without, light, dark and at scale
2 (`shots_27`). Shots
are for looking (pixels differ by GPU and driver), never compared and
never committed: a fault a shot finds gets an ordinary headless test of
the state or layout behind it. A scenario answers each regeneration it
asks for before its shots, unless the shot is of the wait (`-waiting`):
an unanswered one shows "Regenerating…" with the last answer's preview and
Bodies list, which reads like a fault and isn't one.

Reading shots: a finding names the shot, what's wrong and the code
behind it. First drop what the harness made (an unanswered request, a
scenario that left nothing to see, shading that differs by adapter);
then compare with `notes/ui-mock/` (its welcome, model and sketch
pages), the design the screen follows.
What's wrong or misleading, or differs from the mock where the mock is
the design, and is cheap to change, is fixed, in stages of related
changes, each with its failing headless test first; what needs a
product decision or a large change (the renderer, a new feature) goes
to the user with its shot; the rest stays as it is.
