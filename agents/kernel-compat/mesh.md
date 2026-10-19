# Meshes, faces and forms (`src/mesh.rs`, `src/mesh/`)

The old kernel's data model of a solid, kept for its names and forms,
which documents and the regeneration lane use. No solid is made of a
`Mesh` any more: `Solid::new` takes only the empty mesh and fails with
`KernelError::NotImplemented` on any other, and every `Solid`'s mesh is
empty.

| file | holds |
|---|---|
| `mesh.rs` | `Mesh`, `Edge`, `Halfedge`, `Tri`, accessors, `LookupMap` |
| `mesh/face.rs` | `Face`, `FaceName`, `FacePart`, `FaceKey`, `PartKey`, `Surface`, `Quadric` |
| `mesh/form.rs` | `Form`: what surface a face was meant to be, distances to it |
| `mesh/build.rs` | `MeshBuilder`: triangles by vertex id, paired up |
| `mesh/check.rs` | `CheckError`, the invariants a mesh could break (`KernelError::Invalid` carries one) |

## Structure

A halfedge mesh in the Manifold style. `verts: Vec<DVec3>` are the patch
corners. `edges: Vec<Edge>` holds, once per undirected edge, the middle
control point and weight (`Edge { ctrl, weight }`), shared by the two
halfedges that run along it: neighbours trace the same curve because they
read the same record. `tris: Vec<Tri>`, each three `Halfedge { start,
pair, edge }` and a `face`. Halfedge `h` is corner `h % 3` of triangle
`h / 3` and runs to the next corner (`Mesh::next`); corners run
counter-clockwise seen from outside. `Mesh::patch(t)` is triangle `t` as a
`Patch`: corners from the starts, edge `i` from halfedge `i`'s record. All
ids are `u32`; `MAX_PATCHES` (`1 << 22`) keeps three per patch well
inside. The accessors that follow indices panic on out-of-range indices,
which only a caller's own `Mesh::from_parts` can make.

`faces: Vec<Face>`: `Face { name: FaceName, surface: Surface, form: Form,
slack: f64 }`. Beside the faces, `aliases: Vec<(u32, FaceKey)>`: the keys
of faces merged into each face, sorted by face then key, without repeats
or a face's own key (`Mesh::with_aliases` puts them so, `Mesh::aliases`
and `Mesh::face_aliases(f)` read them).

`MeshBuilder` takes vertices, faces, aliases, curved edges (`edge(a, b,
ctrl, w)`, either way round; others are straight) and triangles by vertex
id, and pairs halfedge `a → b` with `b → a`: topology, never a comparison
of positions. It numbers edges in the order their first halfedge comes and
refuses more than `MAX_PATCHES` triangles, repeated or unpaired directed
edges, bad triangles and curves no triangle uses (`BuildError`).

## Names

`FaceName { feature: u64, part: FacePart, instance: u64 }` is made only
from what a feature was given (curve ids, references), never from mesh
indices or positions, so it is stable across regenerations. `FacePart` is
`StartCap`, `EndCap`, `Side { curve, segment }`, `Split(n)`, and the parts
the later features make (`Blend`, `Corner`, `Offset`, `BackSide`,
`Swept`, `Lofted`); `instance` is 0 but on copies. `FaceName::new(feature,
part)` is a name that isn't a copy. `FaceKey { feature, part: PartKey,
instance }` (`FaceName::key`) drops what numbers the pieces of one
surface: `segment` (and a sweep's `piece`); a loft's `span` stays. Keys
are what references store (they derive `serde`; their fields and
variants' order are fixed). Derived names use the fixed mix of
`topology.md`: `FaceKey::mixed`, and a copy's instance
(`FaceName::copy`, `FaceKey::copy`), `mix(parent's instance, feature,
index)`, so copies of copies stay unique.

## Surfaces and forms

`Surface` is a construction's claim of the surface a face lies on:
`Plane { n, d }` (`n·x = d`, `n` any length), `Quadric`, or `Free`.
`Quadric { origin, a, b, c }` is `F(x) = y·(a·y) + 2b·y + c` with `y = x −
origin`: measuring from a point near the surface keeps the rounding of `F`
relative to the quadric's size. `Quadric::cylinder`, `sphere`, `cone`
(both nappes, a half-angle by its cosine and sine) and `revolution` (`ρ² =
r0 + r1·h + r2·h²`) build them. Distances are first order, `|F| / |∇F|`,
infinite where the gradient overflows; a plane's `n` and `d` are divided
by `n`'s largest coordinate before measuring.

`Face::form` is what surface the construction meant the face to be, with
its parameters; regen's picking tables and the datums of `topology.md`
read it.

```rust
pub enum Form {
    Unknown,
    Plane { n: DVec3, d: f64 },                     // n unit, out of the solid
    Cylinder { point: DVec3, axis: DVec3, radius: f64 },
    ConicCylinder { conic: Conic3, along: DVec3 },  // over a non-circular conic
    Cone { apex: DVec3, axis: DVec3, cos: f64, sin: f64 },  // one nappe, axis into it
    Sphere { centre: DVec3, radius: f64 },
    Torus { centre: DVec3, axis: DVec3, major: f64, minor: f64 },
    Revolved { origin: DVec3, axis: DVec3, meridian: Conic2 },  // meridian in (ρ, h)
    Quadric(Quadric),   // none of the above: a cone or sphere scaled per axis
}
```

Axes are unit; a half-angle is its cosine and sine (no trig). Only a
plane's form says which way the face faces (`n` out of the solid); a
curved form is the same surface either way. So `Form::flipped` turns a
plane round and leaves the rest. `Form::plane(n, d)` takes any length of
`n` (dividing both by it), `Unknown` for a zero or non-finite one.
`Form::distance` is exact for planes, cylinders, cones (behind the apex,
the distance to it), spheres and tori, and first order for the conic forms
(`|F|/|∇F|` of `λ1² − 4w²·λ0·λ2` on the conic's control triangle, along
the cylinder or in the meridian half-plane) and quadrics, 0 for
`Unknown`. `slack` is how many fit tolerances a fitted face may stray from
its form (1 as built; the facade keeps it as set).
