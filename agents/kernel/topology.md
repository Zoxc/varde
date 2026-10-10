# Topology and names (`src/topology.rs`, `src/topology/`)

`Topology::new(&solid)` (or `Solid::topology()`) is the solid's B-rep as
users see it, derived from the mesh and never stored:

- **Regions** (`Region { key, aliases, tris }`): connected sets of
  triangles of one `FaceKey`, joined across mesh edges, numbered by their
  lowest triangle; `aliases` is the sorted union of their faces'
  aliases. A region is what users see, pick and name as a face: a
  circle's four quarter walls are one, a face cut in two by a groove is
  two of one key. `region_of(tri)` maps triangles to regions.
- **Chains** (`Chain { regions, halfedges, closed }`): maximal paths of
  mesh edges with the same two regions either side, the lower region
  first, its halfedges on that region's triangles end to end in the
  order they run. A chain runs on through a vertex where exactly its two
  edges meet, one running in and one out (regions' boundaries are
  oriented loops, so that is the case wherever only two regions meet,
  once each); it ends anywhere else (a corner, or two regions meeting
  twice at one vertex). A closed one starts at its lowest halfedge.
  Numbered by their lowest halfedge.
- **Corners** (`Corner { vertex, regions }`): vertices where three or
  more regions meet, by vertex.

Everything is one sequential pass in index order: the same mesh gives
the same topology at any thread count; linear in the mesh (a sort for the
corners), like tessellating, and not budgeted: `check` bounds the mesh.

**Names.** `FaceName { feature, part, instance }` is made only from what
a feature was given (curve ids, references), never from mesh indices or
positions, so a regenerated solid with other dimensions, another
tolerance or another triangulation has the same names. `FaceKey {
feature, part: PartKey, instance }` (`FaceName::key`) drops what numbers
the pieces of one surface: `segment` (and a sweep's `piece`); a loft's
`span` stays, since a ruled loft's spans meet at creases. Keys are what
references store (they derive `serde`; their fields and variants' order
are fixed). Names derived from other names are the fixed 64-bit
`topology::mix(parts)`: from the count of parts, each part as `h =
f((h + γ) ^ part)` with splitmix64's finalizer `f` and constant `γ =
0x9e37_79b9_7f4a_7c15`, wrapping; a test pins its values, worked out
apart from the code. `FaceKey::mixed` is the mix of the feature, the
part's place in `PartKey` and its two fields (0 where it has fewer), and
the instance; a copy's instance is `mix(parent's instance, feature,
index)` (`FaceName::copy`), so copies of copies stay unique; a blend's
`edge` is `blend_edge(faces, ordinal)`, the mix of the two keys' mixes
(lower first) and the ordinal among the feature's references with that
pair.

**Aliases.** Where two faces on one surface become one, the merged
face's key (and its aliases) become aliases of the face that took it in
(the lower index, so the first operand's name stays), through
`Soup::absorb`. The clean-up's merge of plane faces joined by mended
seams does this for whole faces (a boss's top flush with the plate's
merges into it, and the boss's top key then names only the plate's top,
through its alias). `unbend` does it too for the triangles it flips onto
the lower of two faces of one plane, where the higher face may keep
other triangles under its own key: then the key names both regions, and
the point picks. Whether `unbend` fires depends on the triangulation, so
such a partial alias can come and go with the tolerance; it only ever
adds a candidate in the same plane. Any later pass that moves triangles
between faces of one surface must call `Soup::absorb` the same way.
After repair, `Mesh::merge_faces` names adjacent faces of one surface
(planes and quadrics) alike, each member of a set taking the others'
keys and aliases as its own aliases ("Structure"): two coaxial walls
stacked, flush plates side by side, collinear lines' walls in an
extrude. So a key absorbed anywhere still resolves, to the region it is
part of now.
Flush caps often never meet as triangles: the perturbation keeps one
whole and drops the other (a plate first, a boss standing in it flush
on top: the boss's top is gone, with no seam to mend). So after a union
or an intersection (`boolean::covered`), an operand's plane face whose
key or aliases no face or alias of the result names any more gives
those names as aliases to the lowest result face of the same plane
(unit normals within `1e-12`, offsets within the short length) that it
meets: the middle of a triangle of one lies within the short length of
the other, either way round, since a large face's middles can all miss
a small face over part of it (looking only at the dropped face's first
triangle lost a plate's top when a boss inside it came first in an
intersection, so the names differed with the order). A point on the
result's boundary facing the same way as the dropped face is a point
where it lay flush. Only names: the geometry is untouched, and a face
that was cut away or lay inside the other operand gets none; a flush
face kept in part keeps its key there and gets no alias for the part
dropped. The middles are looked up through a box hierarchy of each
face's triangles, charged a unit a triangle, and each middle a unit and
one a patch it is measured against. A difference makes none (the
tool's faces become walls of their own). A key can be a face's and
another's alias at once (one feature's two bosses, one flush and merged
into the plate's top, the other standing): it names both, and the
point picks, which is right for a reference to either boss.
Transforms will carry the table with the faces, as booleans do.

**Tangent chains.** `Topology::tangent_chains(solid)` gives each chain
the lowest chain of its tangent chain: chains joined end to end through
vertices where one runs on smoothly into another, their curves' tangents
leaving the vertex (towards the control point, or along the chord where
it's on the end) opposite within 1° (`u·v < 0` and `(u·v)² ≥ cos²1°
|u|²|v|²`, `cos 1°` a literal). Closed chains have no ends and stay their
own. The ends at a vertex are compared pairwise (sorted by vertex and
chain) and joined by a union-find under the lowest index, so it's one
sequential pass, the same at any thread count, and independent of the
tolerance (the curves, not the drawn segments). The edge sessions'
"tangent chain" and the viewport's selection use it through regen's
`Picking::tangents`.

**Resolving.** `Topology::face(solid, key, near)`: the regions named by
`key` (their key, or an alias); `Topology::edge(solid, [a, b], near)`:
the chains between regions named by `a` and `b`, either way round;
`Topology::corner(solid, [a, b, c], near)`: the corners where some
region is named by each, the three keys all different (a key twice
would let any corner of its face answer; a key and its alias may still
name one region, as a reference stored before two faces merged does,
and the point decides). One is taken whatever `near` says (even NaN);
of several the nearest to `near`, a later one counting only where it
comes nearer by more than a billionth of the solid's size (about what
the searches are accurate to), so ties go to the lowest index whatever
the rounding; none is
`NotFound::{Face, Edge, Corner}` ("face not found", ...). The distance
(`topology/distance.rs`) is a best-first search over pieces by
blossoming, the box of a piece's control points (which holds it) the
lower bound, corners, middles and on patches the foot of the
perpendicular by Newton's method (second derivatives by central
differences, Gauss–Newton's step where that isn't a minimum's; plain
Gauss–Newton doesn't converge off a curved surface by its radius) the
upper bounds; it stops when no piece can come within a billionth of the
patch's size of the best, or at 256 pieces a patch or curve. The
candidates' patches are taken nearest box first and only while their
box comes nearer than the best so far, across candidates too (a later
one must come nearer by more than the billionth). One resolve's searches share a fixed
allowance of 2²⁰ evaluations (a Newton foot 80, a patch's piece 25, a
curve's 7); past it, the patches and curves still to look at count by
their corners and ends alone, an upper bound. So resolving costs a
bounded search plus a pass over the candidates' boxes and corners, not
256 pieces a patch: a point at the middle of a sphere cut in pieces is
as near every patch, and at the patch limit that would be minutes. The
allowance only runs out where many patches are about equally near,
where any candidate is as good. A `near` that isn't finite takes the
lowest candidate without measuring. It only chooses among candidates of
one name: nothing is decided by distance.

**Points and directions** (`topology/datums.rs`): what align moves a
body by, resolved by the same keys and points as faces, edges and
corners. Points: `corner_point(solid, [a, b, c], near)` (the corner's
vertex), `middle(solid, [a, b], near)` (a straight chain's ends'
mean), `centre(solid, [a, b], near)` (a circle's or an ellipse's centre
from `measure::edge_shape`, the same as the picking tables' snap points;
it may lie far off for a nearly straight arc, so what uses it bounds
it). Directions: `normal(solid, key, near)` (a flat face's form's unit
normal, out of the solid), `face_axis(solid, key, near)` (a cylinder's,
cone's, torus's or revolved face's form's unit axis as the form has it,
through the point of the axis nearest `near`: `topology::beside`, since a
near-cylinder cone's apex can be millions out), `edge_direction(solid,
[a, b], near)`: a straight chain from end to end, the way it runs with
the first key's face on its left seen from outside
(`topology::runs_with`, the rule a revolve's axis edge and a move's turn
axis already followed; `EdgeRef::runs_with` calls it, and the keys the
other way round reverse it); a round chain's axis is a flat neighbour's
outward normal (the first key's face first), else a round neighbour's
form axis where it runs along the circle's axis within `1e-6` (the sine;
a torus's meridian circle doesn't), else the circle's own axis turning
the way the chain runs by the keys. So a hole's top rim points up out of
the plate, a pin's bottom rim down out of the pin, and putting the two
face to face puts the pin in the hole. Normals and axes come from forms
(intent, exact where the form is: a face square to a world axis gives
that axis to the bit). `Topology::form(solid, region)` is a region's
form (its first triangle's face's, as regen's picking tables take it).
The errors (`Unresolved`) carry the region or chain found for showing:
`NotFound`, `NotFlat` ("isn't flat"), `NotRound` and `NotCircular`
("isn't round"), `NotStraight` ("isn't straight"), `NotAnAxis` ("isn't
straight or round"), `Undirected` (aliases name both faces by both keys,
"its direction can't be told"). Tests (`topology/tests/datums.rs`): an
extruded plate with a hole (corners, middles and normals to the bit, the
rim's centre, axes and signs, every straight edge's direction against its
faces' normals and reversed with its keys, every wrong kind), a revolved
cylinder under a cone (the shoulder rim between two round faces) and a
torus, a boss joined on a plate, drilled and pocketed, everything after
a turn following the motion, a pin aligned by its rim into the plate's
hole and into a tilted plate's (joined, analytic volume to `1e-9`), and
determinism at 1 and 8 threads.
