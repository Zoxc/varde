# Kernel

`varde-kernel-compat` (`crates/kernel-compat`) is heading for solids that are closed,
oriented 2-manifolds of **rational quadratic triangle patches**, with
union, difference and intersection built the way
[Manifold](https://github.com/elalish/manifold) builds them for flat
triangles: every topological fact derived by counting from primitives that
are each computed once, so the result is always a valid manifold even when
a primitive is numerically off. Planes, circles, arcs and cylinders are
exact in this representation; everything is `f64`, and only drawing
goes to `f32` (`RenderMesh`).

What exists today: the patch math (`patch`), closed meshes of patches with
the check of their invariants, the BVH and the hull tests, red–green
refinement and repair, and box and cylinder meshes (`mesh`), the
tolerances (`Tolerance`), the limits, `Budget` and `KernelError`, and the
parallel map (`par`), below, and `Solid`, a checked mesh, with its
tessellation for drawing (`tessellate`) and its volume and area,
`extrude`, which sweeps a `Profile` into a solid, the swept strips
(`sweep`: exact patches on cones and quadrics of revolution, fitted
bands and caps), `revolve`, which turns a `Profile` about an axis into a
solid (not yet a feature), and `boolean` and
`touches` for solids of flat and curved patches (curved cuts exact where
planes meet planes or quadrics, traced and fitted elsewhere), and a
solid's `Topology` (its faces, edges and corners as users see them, and
resolving the names references keep to them). Documents
store no geometry:
bodies are the outputs of the feature history, which `varde-regen`
evaluates into solids, extrudes making bodies or joining, cutting and
intersecting them, and draws (see "Bodies from the history").

The detail is split by part; a section named in quotes (as "Swept strips")
is a heading in one of these files:

- `patch.md`: patch math: curves, triangles, the normal and fold check, exact cylinder strips, swept, fitted and lathed strips.
- `numerics.md`: tolerances, deterministic parallel maps and trigonometry.
- `mesh.md`: closed meshes of patches: structure, invariants and `check`, forms, orientation, control hulls, BVH, refinement, repair, primitives.
- `solid.md`: solids, tessellation and manifold meshes for export.
- `extrude.md`: profiles and extrude.
- `revolve.md`: revolve.
- `measure.md`: volume, area and measuring.
- `topology.md`: topology and names.
- `section.md`: plane sections.
- `transform.md`: transforms and assembly.
- `boolean.md`: booleans: overview, primitives, exact predicates, counting, curved primitives, pairs of faces, touches.
- `boolean-cut.md`: booleans: cutting curved faces, assembly, triangulating loops, clean-up.
- `boolean-results.md`: booleans: results that aren't manifolds, repair, errors and budget, costs, tests, known gaps.
- `history.md`: bodies from the history: the extrude feature and regeneration.
- `extrude-ui.md`: the extrude UI.
- `limits.md`: limits, budgets and errors.
- `deviations.md`: deviations from the plan.
