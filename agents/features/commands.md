# Commands shared by every kind

`crates/document/src/editor.rs`.

- `Command::AddFeature { name, kind: Box<FeatureKind> }`, made by
  `Document::add_feature(kind)`, which names it one past the highest
  "Extrude N", "Revolve N" ... (`FeatureKind::noun`) among the feature
  and body names both (`Document::next_number`, as "Sketch N" and
  "Body N" are). It hides the
  sketches whose regions the feature takes (`FeatureKind::profile_sketches`:
  `FeatureKind::sketch`'s, or a loft's sections' sketches); one whose
  operation is `NewBody` also adds "Body N" with the next id, replacing
  whatever id the command held (`BodyId::NEW`); a pattern whose copies
  are bodies of their own adds one per copy, laid out as "Pattern"
  says; a split keeping both pieces adds one for its other piece
  (`Split::new_body`, filled in as its `keep` says, see "Split"). One
  undo step. A sketch
  kind is refused (`EditError::SketchKind`): sketches are added empty by
  `AddSketch` and set by `SetSketch`.
- `Command::SetFeature { feature, kind }` replaces a feature's kind,
  keeping its id, name and visibility. A missing feature is a no-op; a
  sketch feature, or a sketch kind, is refused (`EditError::SketchKind`).
  The kind may change (an extrude may become a revolve): what matters is
  the operation. A `NewBody` that stays one keeps its body (whatever id
  the command held; a split's new body likewise, `FeatureKind::new_body`
  / `new_body_mut`); one that stops removes the body and drops it from
  the other features' excluded lists, but **holds its id**
  (`FeatureKind::held_body`: a join's, cut's or intersect's
  `Targets::held`, a split's `new_body` while it keeps a side); one that
  starts gets the body back with the id it held (in its place in id
  order, named "Body N" afresh), else adds one with a new id. So what
  was named on the body (a sketch on its face, a later feature's
  reference) finds it again across edits (New body, Join, New body; a
  split keeping both, one side, both). The commands fill the id in
  whatever they held (`planned_new_body`: the old feature's body or
  held id, carried across kinds that have the slot, else `BodyId::NEW`
  for one to make; `AddFeature` never holds one), so a panel needn't.
  The document's check wants every held id below the next id, no body's
  and held by one feature only (`CheckError::Held`), so features added
  meanwhile never get it. A pattern's copy bodies go the same way, copy
  by copy (see "Pattern"), but aren't held.
  The caller passes regions referenced afresh from the sketch as it is.
  Setting what's already there changes nothing (no new revision).
- Both run the document's whole check on the result, and then
  `check_new` on the added or set feature: what's required of a feature
  when the user makes or edits it, but not of one already in a document
  (a later edit of its sketch may break it, which regeneration reports).
  Today that's a revolve's axis line, a split's line's curves, a
  sweep's path's curves and a loft's start points, point sections and
  rails' curves.
- `FeatureKind` helpers: `noun`, `sketch` (the profile sketch, or a
  split's regions' or line's sketch; a sweep's profile's, not its
  path's, so adding a sweep hides only its profile's sketch),
  `operation` / `new_body` (an extrude's or revolve's `Operation`, or a
  split's new body; a body's maker is checked by `new_body`, or a
  pattern's copy bodies),
  and `uses`, now a **list**
  (sorted, no repeats) of the features this one builds on (a sweep's
  profile's sketch and its path's sketches), which
  `Document::removal` follows. Every extrude-only path that only cared
  about the operation (`drop_excluded`, the app's delete prompt and its
  join merges) goes through `operation()` so revolves get them too.
- **Renaming** (`document/src/rename.rs`, `app/src/doc/rename.rs`):
  `Command::Rename { target: Named, name }` sets a feature's (a
  sketch's too) or a body's name as given, one undo step.
  `Document::rename(target, wanted)` makes it: trimmed, cut to
  `MAX_NAME_LEN`, `None` for an empty name or the one it has. Names
  are kept apart across features and bodies together, but only here: a
  document may still hold repeats (files, `next_number`). A name taken
  gets the first free " (N)" from 1, a " (N)" ending the name asked for
  taken off first, and `Rename::taken` says so; the app then shows a
  toast "The name \"…\" already exists". `F2`
  (`DocumentKeys::rename`: the feature selected in the Timeline, else
  the one sketch selected in Objects, else the one body the model
  selection is of) or Rename in a feature's, sketch's or body's
  context menu opens the rename field in its row (`Look::StartRename`,
  focused with its text selected); `Enter` renames
  (`Edit::CommitRename`), `Esc` closes it, and any other message but
  hovering, scrolling and moving the camera renames first. The name
  isn't new to the file format: `Feature::name` and `Body::name` were
  always stored.
- **Toasts** (`app/src/toast.rs`, `view::toast`): one short message at
  a time at the bottom middle of the window for `TOAST_TIME` (3 s),
  timed by `platform::toast_ticks` while one shows. A document hands
  one over through `Doc::toast` / `Doc::take_toast`, which the app takes
  after every message. Unlike `Doc::notice`, which the status bar
  shows until the next thing asked.
- `SetUnits` pins revolve angles by `Turn::ask` as it pins extrude
  distances by `Extent::ask` (angles' bare numbers are degrees whatever
  the units, so only lengths inside an angle's expression change). Every
  kind's values go through one list, `FeatureKind::values_mut(design)`
  (each value with its ask; `FeatureKind::values` reads them), which
  pinning and the parameter commands share; the kind sections below say
  which values each kind has.
- Kinds are **appended** to `FeatureKind` (`Sketch` 0, `Extrude` 1,
  `Revolve` 2, `Combine` 3, `Move` 4, `Mirror` 5, `Pattern` 6, `Align` 7,
  `Scale` 8, `Split` 9, `Chamfer` 10, `Shell` 11, `Fillet` 12, `OffsetFace` 13,
  `FaceDraft` 14, `Sweep` 15, `Loft` 16): files store a kind by its variant name, and
  the variant index is what the workers' postcard holds.
- **Fresh lengths fit the camera.** A session opening a new feature
  picks its length defaults from the view as it is, and no tool moves the
  camera to show what it makes (the user's rule): `doc::camera::
  fitting_length(camera, units, share)` takes `share` of
  `Camera::view_height` (capped at that share of `MAX_COORD`), rounded
  down to 1, 2 or 5 × 10ⁿ of the design's units, at least 0.001 of one.
  Shares: an extrude's distance 0.25 (`extrude::DEFAULT_SHARE`), a
  linear pattern's spacing 0.25 (its 3 copies span half the view), a
  helix's pitch 0.1 (5 turns rise half of it), a fillet's radius and a
  shell's thickness 0.02 (`motion::BLEND_SHARE`), a chamfer's distance and
  an offset face's half that (a chamfer's second distance twice its
  first). A move's offsets stay 0, angles, counts and factors are fixed,
  and a sketch tool has no default size (only `DEFAULT_SIDES`, a count).
  The camera is the only input: the bodies are mostly picked after the
  session opens, and a size from them could land off screen. Editing a
  feature opens with its stored values, and a value typed stays.
- **The toolbar by selection** (`view::toolbar::selection_bar`):
  outside sketches and sessions, what's selected decides the model
  bar's operations (Measure always last), those fitting it first:
  - Edges alone: Sketch, Revolve (while all straight), Fillet,
    Chamfer, Sweep, Move, and Pattern for one straight edge; one round
    edge (a rim) gives Fillet, Chamfer, Sketch, Sweep, Circular
    pattern, Move.
  - Faces alone: Sketch on face (while all flat), Extrude, Offset,
    Fillet, Chamfer, Shell, Draft, Move, Mirror (while all flat).
  - Edges and faces: Fillet, Chamfer, Sketch, Move.
  - Vertices: Sketch, Align, Scale, Move.
  - Bodies: Sketch, Move, Mirror, Pattern, Combine, Split, Scale,
    Shell.
  - A sketch selected in the Timeline: Extrude, Revolve, Sweep, Loft,
    then the model bar's others.
  - A sketch's curves (`toolbar::sketch_items_bar`): Extrude, Revolve,
    Sweep, Split, Edit sketch; a straight line alone the same less
    Extrude; its points, or curves and points, Edit sketch alone.

  Each takes the selection when started (`Doc::start_motion`,
  `Doc::start_revolve`, `Doc::start_measure`): a blend the edges
  and faces selected (an edge selected around a face selected is left
  out, see "Chamfer"), keeping to the first one's body; a face
  session the faces; a draft with two faces or more the first, if flat,
  as its neutral plane and the rest as its faces; a sweep the edges as
  its path; a mirror the face alone selected as its plane (a face that
  can't be one said why); a move or a pattern the edge alone selected
  as its axis, if it can be (a face selected says only which body, as
  clicking one to pick its body does); a split the face alone selected
  as its tool, its body picked next; an align the vertex alone selected
  as its moved point, a scale as its point; a revolve the straight edge
  alone selected as its axis once its profile is picked
  (`RevolveSession::axis_selected`, tried once); the measure tool one
  or two things selected as A and B. Curves of one sketch selected
  (`Doc::selected_curves`) give an extrude, a revolve or a sweep's
  profile the regions they bound alone (`RegionPick::take_curves`: each
  piece of the outer loop on one of them), a revolve as its axis the
  one straight line among them on none of those loops (or a line
  alone), a sweep with no region bounded the chains through them as
  its path, and a split its line (`Doc::curves_selected`). Move,
  pattern, mirror, align and scale can't name a sketch's line or point
  (`AxisRef`, `PlaneRef`, `PointRef`), so take none.
