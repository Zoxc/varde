# Topology and names (`src/topology.rs`, `src/topology/`)

A `Topology` is a solid's B-rep as users see it: **regions** (`Region {
key, aliases, tris }`, a face as users see, pick and name it), **chains**
(`Chain { regions, halfedges, closed }`, an edge between two regions) and
**corners** (`Corner { vertex, regions }`, where three or more regions
meet). Regen, the picking tables, measuring and the MCP queries read it,
and references (`FaceKey`s with a point near them) resolve through it.

The facade's solids are empty, so `Topology::new(&solid)` (and
`Solid::topology()`) is the empty topology: no regions, chains or corners,
`region_of` gives `u32::MAX` for every triangle, `tangent_chains` is
empty, and every reference fails to resolve. The Varde kernel's user
topology takes this over.

**Names.** The face names and keys are in `mesh.md` ("Names"). Names
derived from other names are the fixed 64-bit `topology::mix(parts)`:
from the count of parts, each part as `h = f((h + γ) ^ part)` with
splitmix64's finalizer `f` and constant `γ = 0x9e37_79b9_7f4a_7c15`,
wrapping. Names are stored in files, so it must never change (std's
hasher may); `topology/tests.rs` pins its values, worked out apart from
the code. `FaceKey::mixed` is the mix of the feature, the part's place in
`PartKey` and its two fields (0 where it has fewer), and the instance; a
blend's `edge` is `blend_edge(faces, ordinal)`, the mix of the two keys'
mixes (lower first) and the ordinal among the feature's references with
that pair.

**Resolving.** `Topology::face(solid, key, near)`: the region named by
`key` (its key, or an alias); `Topology::edge(solid, [a, b], near)`: the
chain between regions named by `a` and `b`, either way round;
`Topology::corner(solid, [a, b, c], near)`: the corner where some region is
named by each, the three keys all different. One candidate is taken
whatever `near` says; none is `NotFound::{Face, Edge, Corner}` ("face not
found", ...). Of several, the old kernel took the nearest to `near`; the
facade, which measures no distance, finds none (it never has several).

**Points and directions** (`topology/datums.rs`): what align moves a
body by, resolved by the same keys and points. Points:
`corner_point(solid, [a, b, c], near)` (the corner's vertex),
`middle(solid, [a, b], near)` (a straight chain's ends' mean),
`centre(solid, [a, b], near)` (a circle's or an ellipse's centre, from
`measure::edge_shape`). Directions: `normal(solid, key, near)` (a flat
face's form's unit normal, out of the solid), `face_axis(solid, key,
near)` (a round face's form's unit axis, through the point of the axis
nearest `near`: `topology::beside`), `edge_direction(solid, [a, b],
near)`: a straight chain from end to end, the way it runs with the first
key's face on its left seen from outside (`topology::runs_with`); a round
chain's axis is a flat neighbour's outward normal (the first key's face
first), else a round neighbour's form axis where it runs along the
circle's axis within `1e-6` (the sine), else the circle's own axis turning
the way the chain runs by the keys. `Topology::form(solid, region)` is a
region's form (`Form::Unknown` for one it doesn't have). The errors
(`Unresolved`) carry the region or chain found for showing: `NotFound`,
`NotFlat` ("isn't flat"), `NotRound` and `NotCircular` ("isn't round"),
`NotStraight` ("isn't straight"), `NotAnAxis` ("isn't straight or
round"), `Undirected` (aliases name both faces by both keys, "its
direction can't be told"). `runs_with(faces, left, right)` and `beside`
are pure and also used by regen.
