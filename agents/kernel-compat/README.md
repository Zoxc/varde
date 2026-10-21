# Kernel facade

`varde-kernel-compat` (`crates/kernel-compat`) is the **facade** between the
app and the Varde kernel (`crates/kernel`, `varde-kernel`). It keeps the
public API the other crates call, so they build unchanged, but the old
kernel behind it is gone; the git tag `varde-kernel-legacy` holds it whole.

What the facade keeps, as it was:

- the value types with their fields and pure helpers: `Profile`, `Loop`,
  `Segment` (`check` and `area`, the area by `varde-kernel`'s Gauss rule),
  `Frame`, `Tolerance`, `Aabb`, `Motion` (moves, turns, mirrors, scales,
  `then`, `align` with `Datum` and `AlignOptions`), the curves and
  triangles of `patch` (`Conic`, `Patch`, `Bounds`, `PatchError`), the
  halfedge data model of `mesh` with `MeshBuilder` and the face names,
  keys, surfaces and forms, `Evidence` and `Failure`, `RenderMesh`,
  `RenderLines`, `MeshUpload`, `ManifoldMesh` and its checks (the
  collinear-corner test exact by an expansion sum over the `f32`
  positions' products), `Display`, `Topology`'s types and the fixed name
  `mix`, and `section::crossings` and `sample` on chains of conics;
- every error enum callers match on. `KernelError` keeps the old variants;
  the facade itself only gives `TooComplex` and `NotImplemented(what)`, and
  `varde-kernel`'s `KernelError` converts into it.

What it stubs: every `Solid` is empty (`Solid::empty`). Making geometry
(`extrude`, `extrude_tapered`, `revolve`, `Solid::cuboid` and `cylinder`,
`Solid::new` of a mesh with triangles, `boolean` and `touches` of two
non-empty solids, `assemble` and `Solid::transformed` of non-empty ones,
`split` and its tools, `chamfer`, `fillet`, `shell`, `offset_faces`,
`draft_faces`, `sweep::sweep`, `loft::loft`) fails with
`KernelError::NotImplemented` (`extrude`, `extrude_tapered` and `revolve`
first run `Profile::check`, so a broken profile still gives its
`ProfileError`); with an empty operand a boolean's result is
known and given (a union is the other operand, a difference the first, an
intersection empty; the empty solid touches nothing). Queries answer for
the empty solid: no bounds, volume or area, an empty `Topology` (nothing
resolves), an empty `RenderMesh`, `ManifoldError::Empty`, `measure` and
`distance` finding nothing, `face_section` no curves; `Display::flatten`
cuts a curve as an edge would be cut and `sample_patch` draws nothing. Nothing
panics on a caller's data.

So the app runs, but regenerates no geometry: every feature that makes
a solid fails, regen saying that it "isn't supported yet". As the Varde kernel's
operations land the facade forwards to them, and once every caller uses
its types directly the facade goes.

The detail of what is kept is split by part; a section named in quotes
(as "Names") is a heading in one of these files:

- `patch.md`: curves and triangles, exact arcs, splits, the patch limits.
- `profile.md`: profiles, their check and area, frames.
- `mesh.md`: the halfedge data model, the builder, face names and keys,
  surfaces and forms.
- `topology.md`: the topology's types, the fixed name mix, resolving
  references, points and directions.
- `transform.md`: motions and aligning.
- `section.md`: where a plane crosses a chain of conics.
- `render.md`: `RenderMesh`, `MeshUpload`, `RenderLines`, `Display`, and
  `ManifoldMesh` with its checks.

Regen's side of the extrude feature and the extrude UI are in
`agents/features/extrude.md` and `agents/features/extrude-ui.md`.

## Tolerances and trigonometry

`Tolerance` holds the design's **fit** tolerance (how far a fitted curve
or patch may be from the true surfaces), in mm, within `MIN_FIT ..=
MAX_FIT` (`1e-5 ..= 1e-1`), default `1e-3`; `new` refuses anything else.
The **resolution** is a thousandth of it. Neither ever decides that two
things are the same vertex or edge.

`trig` is `varde_decide::num::trig`, re-exported: the angles through the
`libm` crate, so they have the same bits natively and on the web. The
facade's own maths (patches, motions, aligns, measures of angles) uses
only `+ − × ÷ √` and `trig`.

## Limits, errors and failures

| constant | value | why |
|---|---|---|
| `MAX_COORD` | `1e6` | the largest coordinate or size a design may have; sums and products of them stay finite well within `f32` |
| `MAX_PATCHES` | `1 << 22` | patches in a mesh; ids and counts fit a `u32` |
| `MAX_PROFILE_SEGMENTS` | `1 << 16` | segments in a profile |
| `MAX_WORK` | `1 << 22` | work units in one operation (`varde-kernel`'s) |
| `MAX_EVIDENCE` | 4096 patches, 4096 curves, 256 points, 4096 sketch curves, 256 faces | items of each kind one failure's `Evidence` holds; past them the first ones and `truncated` |
| `EVIDENCE_WORK` | `1 << 16` | work units gathering one failure's evidence may take, apart from the operation's budget |

`Budget` (`Budget::new(work)`, at most `MAX_WORK`; `Budget::DEFAULT`) is
`varde-kernel`'s, re-exported. `KernelError` is the facade's own, with the
old variants callers match on: `TooComplex`, `Invalid(CheckError)`,
`Patch(PatchError)`, `Profile(ProfileError)`, `Boolean(BooleanError)` and
`NotImplemented(&'static str)` (naming what isn't built, "… isn't
implemented yet"); `varde_kernel::KernelError` converts into it:
`OutOfBudget` of any cause is `TooComplex`, `Internal(Inconsistent)` is
`Boolean(BooleanError::Inconsistent)`, `NotImplemented` stays, and the
variants no facade call raises yet (`Unrepresentable`, `Invalid`,
`Unsupported`) are `NotImplemented` naming the error not passed on (the
old variants that would fit carry the old kernel's types). The
operations that make or combine solids return `Result<_, Failure>`:
`Failure { error: KernelError, evidence: Box<Evidence> }`, with
`From<KernelError>` (empty evidence) and `Display` the error's.
`Evidence` is the geometry an error is about, by value in world
coordinates: `patches`, `curves` (`Conic<DVec3>`), `points`,
`sketch_curves` (`Segment::curve` ids), `faces` (`(Operand, FaceKey)`)
and `truncated`; the `add_*` methods keep to `MAX_EVIDENCE`, and
`within_caps` checks one received from elsewhere. The facade's stubs give
no evidence; regen builds its own (points and sketch curves) for the
errors it finds itself.
