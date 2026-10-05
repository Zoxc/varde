# To decide

Design issues noticed in passing, to investigate and decide later. Each
says what happens now, why it's awkward, and the options seen so far.

## Regeneration changing the document apart from an edit

**Now.** A sketch's links are filled from the model by regeneration, off
the UI thread. When an answer gives a link new geometry, the app folds it
into the change it follows from (`Editor::amend`, `crates/app/src/doc/relink.rs`),
so an edit and the relink it causes undo as one step. A document just
opened has no change to fold into, so a relink after opening goes into the
opened state itself:

- If it only fills links that were empty (the sketch face added on
  reading, a link added before a save that didn't wait for the model), the
  document is kept saved (`Doc::keep_clean`): nothing really changed.
- If it moves a link's geometry (the file was saved with a stale link,
  e.g. one whose source changed without a regeneration answering), the
  document shows unsaved but there is nothing to undo. The test is
  `opening_a_file_with_a_stale_link_besides_its_missing_sketch_face_is_edited`
  in `crates/app/src/doc/sketch/tests/faces/sketch_face.rs`.

**Why it's awkward.** The document holds derived data (link geometry) that
regeneration owns, so the saved file and the edit history can disagree
with the model, and a change can appear that no user action made. Making
the relink its own undo step doesn't help: undoing it brings back the
stale geometry, which the next answer relinks again.

**Options.**

1. Leave it: unsaved, nothing to undo (as now).
2. Treat any relink after opening as part of the opened state and keep the
   document saved, accepting that the file on disk is stale until the
   next save.
3. Stop storing link geometry as document state that edits own: keep it
   derived (cached beside the document, refilled by regeneration), so
   relinks never edit the document. Bigger change, touches the `.vrdp`
   format and the solver's view of links.

The same question may come up for other derived data kept in the
document; list such cases here as they're found.
