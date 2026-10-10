# Features

The features after sketches and extrudes (revolves, combines, moves, mirrors, patterns, aligns, scales, splits, chamfers, shells, fillets, offset faces,
drafts, sweeps and lofts), and sketches' planes on faces:
their document types, checks and commands, how regeneration evaluates them, and their UI. Extrudes are
described in `agents/kernel/history.md` ("The extrude feature") and
`agents/kernel/extrude-ui.md`; what
they share with the newer kinds is here. The kernel math of each is in
`agents/kernel/`.

The detail is split by part; a section named in quotes is a heading in one of these files:

- `commands.md`: Commands shared by every kind.
- `parameters.md`: Parameters.
- `failures.md`: Failures and where they are.
- `sketch-planes.md`: Sketch planes on faces.
- `sketch-links.md`: Sketch links.
- `revolve.md`: Revolve.
- `combine.md`: Combine.
- `move-mirror.md`: Move and mirror.
- `pattern.md`: Pattern.
- `align.md`: Align.
- `scale.md`: Scale.
- `split.md`: Split.
- `chamfer.md`: Chamfer.
- `shell.md`: Shell.
- `fillet.md`: Fillet.
- `offset-face.md`: Offset face.
- `draft.md`: Draft.
- `sweep.md`: Sweep.
- `loft.md`: Loft.
- `knobs.md`: Operation knobs.
