# Sketch links

A sketch's links (`agents/sketch/model.md`, Links) take geometry from outside
it: another sketch's point or curve, or a model edge, face or corner,
projected onto its plane or cut by it. The sketch holds what each link
made; the feature holds where each comes from (`FeatureKind::Sketch::
sources`, a `LinkSource { link, source: OutsideRef }` per link, in the
links' order). `OutsideRef` (`document/src/outside.rs`) is another
sketch's feature and item ids, or an `EdgeRef`, a `FaceRef` or a
`PointRef::Corner`, named as features name them; `OutsideRef::takes`
says which a link's kind can take (Project: edges, faces, corners,
sketch items; Intersect: faces, edges). It lives in the document, not the
sketch, because `varde-sketch` can't name the model (`document` depends
on `sketch`); the sketch holds only what the solver and editing need.

**Checks** (`Document::check_sources`): one source per link, in order,
of a kind it takes, its own parts right (`EdgeRef::check_own` and the
rest), and what it names made before the sketch, or, as a sketch's face,
not there with ids below the next id: a removed source leaves the link,
which then doesn't find it (`Document::removal` doesn't follow links). A
sketch source is a sketch feature before it; whether it holds the item is
regenerating's to find. `CheckError::SketchLink(feature, LinkError)`.

**Commands**: `Command::AddLink { feature, sketch, source }` sets the
sketch (with a new link) and records the link's source;
`Command::SetSketch` keeps the sources of the links the new sketch still
has and drops the rest, so deleting a link (a sketch edit) drops its
source; a sketch with a link the feature has no source for is refused.
**The sketch face** (`document/src/outside.rs`, `sketch_face`): every
sketch on a face has a Project link of that face, the first one whose
source is `OutsideRef::Face` of its plane's face, known by comparing
sources, with no field of its own. `Command::AddSketch` on a face adds
it (empty, out of profiles, so its curves come construction once
relinked) in the same change; `Command::SetSketchPlane` makes it follow
(onto another face the same link takes the new face, or goes if a
Project link of that face is there already, which then is it; onto an
origin plane it's deleted); `Command::SetSketch` and `Command::AddLink`
taking it away are refused, `EditError::SketchFace`.
`Document::set_sketch_whole` gives a sketch drawn afresh one if it lacks
it (as `AddLink`). A document read (`Unchecked::check`, every file,
auto-save and wire decode) gets one where it lacks it
(`Document::with_sketch_faces`, after the check, before any editor sees
it, so it isn't an edit: no undo step, the editor's first revision).
Relinking then fills it on the first answer, an amend; the app keeps a
document as it was opened or saved clean through amends that only fill
empty links (`Doc::keep_clean` in `app/src/doc/relink.rs`: not
unsaved, not auto-saved).

`Editor::amend` applies a command folded into the change before it:
undo takes both back at once, the redo history stays, and the document
gets a new revision and generation.

## Regeneration

`regen/src/history/link.rs`, run in `walk` at each sketch with sources,
before it's listed among the sketches later features use. Each source is
found where the sketch is in the history (`link::find`): another sketch's
item on that sketch as it is, at its placement, projected through the
affine map from its plane into this one's (`Sketch::project_item`); a
model edge, face or corner on its body among those the features before
made, through `Evaluation::holder` and then the splits before (`on_body`,
as `place_on_face` follows a sketch's face), so a later split moving it
onto another body doesn't lose it, and a face or edge isn't looked for in
the final model. A model edge projected is sampled along its conics
(`varde_kernel::section::sample`, 32 places a conic) and fitted
(`LinkShape::fit`, to the resolution, splines within the fit tolerance);
a corner is a point; a face projected is its outline (`link::outline`):
every edge of its region with another region beside it, outer loop and
holes, sampled and projected so, fitted together as one link; an edge
intersected gives the points where it crosses the plane
(`section::crossings`), a face the curves where the plane cuts it
(`section::face_section`), fitted likewise, or its outline where it
lies in the plane. Like every link, a face's follows the model: its
loops changing (moved, resized, a hole added or gone) make the link
stale and relinked (below). What it finds
is cached by the solid's key, the source, the link's kind, the sketch's
placement and the tolerance (a sketch source by that sketch's key and
both placements). Found nothing is a broken link, with why (the words are
`link.rs`'s constants: its sketch isn't there or isn't placed, it isn't
in its sketch any more, the sketch isn't placed, its body is gone, its
edge, face or corner wasn't found, its edge doesn't cross the plane or
lies in it, the plane doesn't cut its face, too complex,
or a fit's refusal); it keeps what it holds.

`link::relink`: a link whose shape found isn't what it holds, to the
resolution, as relinking would give it (`Sketch::link_follows`), is stale; the sketch is proposed with
every stale link relinked (`SketchEdit::Relink`), solved so what's tied to
them follows, or relinked unsolved if it doesn't solve, cached by the
sketch's key and the shapes. That sketch goes into `Evaluation::relinked`;
broken links into `Evaluation::broken`. The regeneration itself goes on
with the sketch as the document holds it. The answer carries both
(`Response::Regenerated::relinked`, none with a draft: only the committed
document's links are followed; `broken`), and so does the wire
(`Head::Regenerated`, the sketches checked by the document's check when
the app folds one in; decoded within bounds, each list within a sketch's
limits and all of them within `wire::MAX_RELINKED_ITEMS`, the broken
links within `wire::MAX_BROKEN`, so a few bytes can't make the page
allocate far more; relinked sketches past the bound aren't sent, nor
broken links past theirs; and the relinked dropped, with the failures'
geometry, when the head is too large).

## Following the model (`app/src/doc/relink.rs`)

The app folds what regenerating found into the document: the answer's
relinked sketches are kept by `MeshFeed` with the generation they're of
and taken once (`MeshFeed::take_relinked`), only if that's the editor's
generation now (an older answer's document is gone; the next answer is
of the new one) and the document can be changed; each is set by
`Editor::amend`, folded into the change it follows from, so moving a
source and the links following are one undo step: undo takes both back
(the sketch then holds what it held, in step with the model it was
made with), redo gives both again, nothing waits on it, and following
never makes a step of its own, so it never fights undo or the redo
history. It's a new revision, so the file has unsaved changes, and a
proposal in flight is proposed again on it. A link just added holds
nothing; the next answer fills it, folded into the change that added it.
A link whose shape changed keeps what ids it can (`agents/sketch/model.md`,
Links); the constraints and dimensions on what it no longer makes go
with it, which the status bar says ("Sketch 2's links changed shape,
removing 1 constraint on what they no longer make").
Broken links are only shown (`MeshFeed::broken`), never written: a link
keeps what it last found, and finds it again once its source is back.
A relinked sketch the document refuses is left as it was, the links it
would have changed (all of them if none) shown broken with why ("What it
found was refused: ..."; `MeshFeed::mark_broken`) until the next answer;
the document unchanged, nothing more is asked, so it doesn't loop.
Only the committed document's links are followed: a draft's answer
relinks nothing. A file whose links are out of step with the model
(saved by an older build, say) is put in step once it's opened, with no
undo step.

The sections themselves are `varde_kernel::section`'s
(`agents/kernel/section.md`, Plane sections), called with the resolution as the
distance within which something lies in the plane and pieces weld.
