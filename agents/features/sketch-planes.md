# Sketch planes on faces

`crates/document/src/plane.rs`.

```rust
pub enum Plane { Origin(OriginPlane), Face(FaceRef) }      // Face appended
pub struct FaceRef { pub body: BodyId, pub key: FaceKey, pub near: DVec3 }
```

- **The reference**: the body, the face's key (the kernel's `FaceKey`,
  re-exported with `PartKey`; its `feature` is the number of the feature
  that made the face, `FaceRef::maker`) and the picked point, which
  chooses among several regions with the key, as the kernel's
  `Topology::face` resolves it. The document stores the reference, never
  a placement.
- `Plane::placement() -> Option<Placement>`: an origin plane's; `None`
  for a face, whose placement only regeneration finds. `Plane::name()`
  is "XY" ... or "a face"; `Plane::face()` the reference.
- **The placement rule**, `Placement::on_plane(n, d) -> Option<Placement>`
  for a flat face's form `n·p = d` with `n` out of the solid, one pure
  function regen and the app both call on the same bits:
  - normal `n̂ = n / |n|` (seen from outside; an extrude's positive side
    grows the body), origin `n̂ d / |n|` (the plane's point nearest the
    world origin, so thickening a plate moves the drawing only along the
    normal and a face's size moves nothing);
  - horizontal (`n̂x² + n̂y² ≤ 1e-18`, `HORIZONTAL`, a stated decision):
    `x` is world X less its part along `n̂`, normalized, `y = n̂ × x`;
    otherwise `y` is world Z less its part along `n̂` (its z as
    `n̂x² + n̂y²`, which doesn't cancel), normalized, `x = y × n̂` ("up
    stays up", what the Z-up camera shows facing the plane); the stored
    `normal` is `x × y`;
  - signed zeros made positive (`+ 0.0`), so a face parallel to an origin
    plane and facing its way gets that plane's axes to the bit (whatever
    `|n|`), and the bottom face `x` = X, `y` = −Y; axes don't depend on
    `d` (a moved face keeps them to the bit);
  - only `+ − × ÷ √` and one comparison; `None` for a zero or non-finite
    `n` (one whose length overflows or whose square underflows), a
    non-finite `d` or result.
- **Checks** (`Document::check`, `CheckError::SketchPlane(id,
  PlaneError)`): the point finite and within `MAX_COORD`
  (`FaceRef::check_own`, `PlaneError::Near`); the body, if it's there,
  made by a feature before the sketch (`Body`); the key's feature, if
  it's there, before the sketch (`Maker`; the sketch itself isn't). A
  body or feature that isn't there is allowed (the reference fails to
  resolve, as a region can), but only with an id below `next_id`: ids
  never come back, so it can't later name something after the sketch.
  One at or past `next_id` (only a file can hold one) is refused, as the
  edit that hands that id out would otherwise be, again and again.
- **Commands**: `AddSketch { name, plane }` takes either kind;
  `SetSketchPlane { feature, plane }` puts a sketch on another plane, one
  undo step, keeping its drawing in its own coordinates. Not a sketch,
  not there, or the plane it's on already: no change (no new revision).
- **No removal cascade**: `FeatureKind::uses` doesn't list the face
  body's maker, so removing that feature or the body (or setting its
  extrude to stop making the body) leaves the sketch, naming a body
  that's gone, for regeneration to fail ("its face's body is gone") until
  it's put on another plane. Removing the sketch still takes what was
  made from it.
- `SetUnits` leaves planes alone (no values in them).

**Regeneration** (`regen/src/history.rs`, `place_on_face`): the history
runs in order, so when it reaches a sketch on a face, the face's body is
as the features before the sketch left it. The sketch is placed:

- the body through `Evaluation::holder(face.body)`: a body a join
  consumed is followed to the body holding it, where its faces live on;
  one with no solid fails, "its face's body is gone" (its maker failed,
  was removed, or no longer makes it);
- the face on that solid's `Topology` by `Topology::face(solid, &key,
  near)` (name or alias, the nearest to `near` among several); none
  fails, "its face wasn't found";
- the region's form (its first triangle's face's, `picking::region_form`,
  the same the picking tables summarize, so the app's pick gives the
  same bits) must be `Form::Plane { n, d }`, else "its face isn't flat",
  which shows the face (see "What has geometry so far" above);
- `Placement::on_plane(n, d)`, refused unless `Placement::valid` (every
  number finite, axes unit and square within `Placement::SLACK` = 1e-9,
  `normal = x × y` within it, origin within `MAX_COORD` on each axis):
  "its face is too far out to sketch on" (a tilted face near the
  coordinate limit can have its origin past it).

A placed one is listed in `Evaluation::placements` (face sketches only,
in the document's order) and its extrudes and revolves run on that
frame; a failed one is drawn nowhere, its profiles are still worked out,
and every extrude or revolve made from it fails with "its sketch isn't
placed". No last plane is kept.

Limits, seen fuzzing whole histories: a tool built on a tilted face is
flush with it only to rounding (its frame is the face form's normal
renormalized, an ulp or so off the frame the face came from), so a
boss joined or a hole cut flush with a tilted face, or on the end of a
boss built on one, now and then fails in the kernel's boolean ("leaves
no clean solid": 12 of 200 random prisms, each with a wall boss or hole,
a second one on its end and three edits); square faces are exact and
don't. An error, never a wrong solid. And the origin
rule (the plane's point nearest the world origin) can put a far tilted
face's points more than `MAX_COORD` from the origin in the sketch's own
coordinates, where no point can be drawn; the face is placed, but only
its nearer part can be drawn on.

Caches: a sketch's profiles are keyed by the sketch alone (not the
plane: they're 2D, so the same drawing on another plane finds them);
the placement by the face's solid's key, the face's key and `near`'s
bits (`Entry::Placement`, the result or its message), so the topology is
worked out again only when that solid changes. That's all it depends
on: a solid's key holds everything the solid was made from, so the same
key is the same solid to the bit, the same topology, and the same region
for the same name and point (an upstream edit that leaves the solid's
key alone can't move the face, and one that moves it changes the key);
the body's id isn't in it, as the placement doesn't depend on which body
holds the solid; an extrude's or
revolve's tool key adds the placement's twelve numbers' bits
(`Keyer::placement`) to the sketch's key. An edit upstream changes the
body's key, which re-places the sketch, which changes the tool keys
after it: the sketch and what it made follow the face. Drafts need
nothing new: a draft runs the history in order as a commit does, so its
preview shows the sketches on its faces following.

The answer carries `placements: Vec<(FeatureId, Placement)>` (as
`Evaluation::placements`, with a draft that goes as the document with it
applied placed them); on the wire (`Head::Regenerated::placements`, the
four vectors as `[[f64; 3]; 4]`) each is checked `Placement::valid` and
each sketch listed once (`wire::Error::Placement`, the generation
answered as failed). `flatten_sketches(document, placements, exclude)`
draws an origin plane's sketch at the plane and a face sketch at its
placement, skipping one that isn't listed. `regen::Request::Regenerate`
boxes its draft (`Option<Box<Draft>>`): a feature kind is large next to
the rest of the request.

**The app** reads every placement through `Doc::placement(feature)`: an
origin plane's, or a face sketch's from the model shown (`MeshFeed`
keeps the answer's `placements`, given out as `MeshFeed::placement`
only while the model shown is of the document as it is, as the failed
features are, and only for a sketch on the plane it had in the document
that model is of: `MeshFeed` keeps the snapshots it asked about until
their answers can no longer come, at most 64, so after an undo or redo
of a change of plane the old face's placement isn't used while the
answer is on its way). Right after a face is picked, before any answer places
the new sketch, the app works the placement out itself: the face is
found in the model shown (`PickIndex::find_face`, a merged body's face
on the body holding it), its picking summary
must be `Summary::Plane { n, d }`, and `Placement::on_plane(n, d)` on
those bits (`PickIndex::face_placement`) is what regen will get, so
nothing jumps when the answer comes; it's kept as `Doc::placed` (the
sketch, the placement, the generation it was added at) until a model at
least that new shows, and refused if not `Placement::valid` ("That face
is too far out to sketch on"). The sketch session (`SketchSession::placement`,
kept in step by `Doc::follow_placement` after every sync and answer, and
kept as it was while `Doc::placement` knows none), so the viewport's
hit testing, snapping, grid and anchors, Home and the camera facing it,
reads it from there; the extrude and revolve sessions' candidates
(`RegionPick::candidates`, given `Doc::placement`, which their regions,
handle and axis are drawn on) leave out a sketch with none. A sketch on a face that
failed to be placed is still listed in the Timeline, red with its
reason, and isn't drawn; entering it asks for another plane first (Change
plane, `agents/sketch/editing.md`), which `SetSketchPlane` puts it on, a face's
placement worked out at the pick as for a new sketch. Before the first
answer, or read-only, it isn't entered: the status bar says "Can't edit
Sketch 2: <reason>" (`Doc::notice`, until the next thing asked).

`PickIndex::find_face` resolves a `FaceRef` as `Topology::face` does:
among the faces named by the key or an alias, the only one, or the
nearest to `near`, a later one only where it's nearer by more than a
billionth of the model's size, the faces numbered alike (the picking
tables list a body's faces in its topology's region order). It measures
to the drawn triangles (`f32`) rather than the exact patches; for a flat
face, the only kind sketched on, those lie in its plane, so the two
differ only by `f32` rounding, which can change the answer only where
the point picked is within that rounding of two faces with the same
key, and the answer's placement replaces the app's anyway. What does
differ for Change plane is the model: the app finds the face in the
final model, regen at the sketch's place in the history. A face a later
feature removed can't be picked, and one moved by a later feature would
be placed at the pick where it ends up and then where it was; faces of
a body a later join merged into another are named by the body they
were made on (`PlanePick::face_ref`: the body the feature naming the
face made, or else the body the cut, intersect or join naming it touched
(a join only its first, which it merges into), among those merged into
the one shown; `MeshFeed::touched_features` lists the holders as they
were then), which regen follows on to the holder wherever the join is,
so they're found before the join too. Where those are more than one body
at the sketch's place (a hole cut through two bodies a join after the
sketch merged; the merges before it are replayed from the joins that
worked with `note_merge`), which one the face is on can't be told:
Change plane refuses it ("Which body that face is on at Sketch 2 can't
be told: pick another"); a new sketch, at the end, takes it on the
holder.
