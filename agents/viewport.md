# How the viewport works

The document screen docks the toolbar above and the side panel left of the
viewport, which fills the rest. The viewport is a `shader` widget whose
primitive calls `varde_render::Renderer`, which opens its own render pass with
a depth buffer and composites onto iced's frame (`LoadOp::Load`). iced then
draws the UI on top: only the camera controls in the viewport's corner are
stacked over it, wrapped in a `mouse_area` so clicks on them don't reach the
viewport.

The tessellated document mesh is kept in the app's `MeshFeed` and
keyed by `Editor::generation()`, so it's only rebuilt when the document
changes; the viewport widget just draws what it's handed. The
renderer's GPU copy is keyed by the mesh's `Arc` instead, since iced shares
one pipeline between documents whose generations each start at 0.
The feed asks the regeneration lane for it with a cheap `Arc` snapshot of the
document and takes the answer when it arrives, dropping answers older than
the mesh shown. Until then it keeps the last mesh and the status bar says
"Regenerating…".

Natively each open document has a regeneration thread (`regen::lane`),
started by an iced subscription keyed by the document's id. The
subscription's stream first hands the app the lane's sender, then yields
each response as a message tagged with the id, so an answer for a closed
document is never applied to the next one. Requests go into a single slot
that a newer one overwrites, so a burst of edits costs one run after the
current one, and one older than a request already sent is dropped.
Closing the document ends the subscription and with it the thread.

On the web the lane is a Web Worker with the same API. It shares no memory
with the page, so a request is the generation and the postcard-encoded
document (the encoding `.vrdp` records use), and the answer is a small
postcard head and the mesh's positions, normals, indices and edges as raw
bytes. Both
directions transfer their `ArrayBuffer`s instead of copying them. The page
checks what comes back before using it (whole elements, a size bound, indices
and edges within the vertex count; see `regen::wire`). The worker can't see
new messages while it works, so the page keeps latest-wins itself: one
request is with the worker at a time, and newer ones replace each other until
it answers. A job that has started always finishes. If the worker dies (a
panic traps it), the generation it was working on is reported as failed; a
request waiting behind it starts a new worker at once, otherwise the next
edit does. A worker that dies before it's ready, or doesn't load, isn't
started again for the request waiting on it, which is reported as failed
too, so a worker that always crashes can't restart without end. Closing the
document terminates it.
