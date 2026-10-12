# How sketching works

What the user sees is in `notes/Sketching.md`, the plan in
`notes/SketchImpl.md`. This note says how what's built works: the model,
the solver and its lane, profiles, how it's edited, drawing it and the
input on it in the viewport, dimensions included. Every edit is proposed to the solver
lane and committed once it's accepted, solved.

The detail is split by part; a section named in quotes is a heading in one of these files:

- `model.md`: The model (`varde-sketch`).
- `solver.md`: The solver (`varde-sketch`, `solve/`).
- `profiles.md`: Profiles (`varde-sketch`, `intersect.rs`, `profile.rs`).
- `proposals.md`: Edits and proposals (`varde-sketch`, `edit.rs`, `propose.rs`).
- `solver-lane.md`: The solver lane (`varde-solve`).
- `editing.md`: Editing.
- `viewport.md`: In the viewport (`crates/view/src/viewport/sketch.rs` and its modules).
- `snapping.md`: Snapping (`crates/view/src/snap.rs`).
- `hit-testing.md`: Hit testing (`crates/view/src/hit.rs`).
