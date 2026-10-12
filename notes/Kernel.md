# Kernel

`varde-kernel-compat` (`crates/kernel-compat`) is the facade between the app
and the Varde kernel (`crates/kernel`, `varde-kernel`), which replaces the old
kernel. The facade keeps the old kernel's public API so the other crates
build unchanged; the old kernel's implementation is gone, and the git tag
`varde-kernel-legacy` holds it whole. `agents/kernel-compat/README.md` says
what the facade keeps and stubs, a file per part.

## The facade

- **Kept**: the value types and their pure helpers (profiles, frames,
  tolerances, boxes, motions and aligns, conics and patches, the mesh data
  model and face names, evidence and failures, render meshes and lines,
  manifold meshes and their checks, the display settings, the topology's
  types), and every error enum callers match on.
- **Stubbed**: every solid is empty. Extrude, revolve, primitives, booleans
  of non-empty solids, assembly and moves of them, and the feature
  operations (split, chamfer, fillet, shell, offset, draft, sweep, loft,
  tapered extrude) fail with `KernelError::NotImplemented` (extrude and
  revolve after checking the profile, so a broken one still says what is
  wrong with it); booleans with an empty operand and queries answer for
  the empty solid. Nothing panics on a caller's data.
- **Moved**: the work `Budget` is `varde-kernel`'s, the quadrature rule the
  profile's area uses is `varde-kernel`'s, the trigonometry
  `varde-decide`'s.

So the app starts and edits documents, but regenerates no geometry: every
feature that makes a solid fails, regen saying it "isn't supported yet".

## The Varde kernel

`varde-kernel` grows behind the facade, with its decision layer in
`varde-decide` and its test support in `varde-testkit` (all three under
the bans in their `clippy.toml`: results the same bits on every platform,
thread count and run). As each of its operations lands (primitives,
extrude, revolve and tessellation first, then booleans), the facade
forwards to it; once every caller uses the Varde kernel's types directly,
the facade goes.
