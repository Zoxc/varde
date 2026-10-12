# Old-kernel test cases as corpus ideas

The old kernel's tests stay at the git tag `varde-kernel-legacy`, under
`crates/kernel-compat/src` (`git show varde-kernel-legacy:crates/kernel-compat/src/<path>`).
They exercised a kernel that is gone, but many of their scenarios are worth
building again against the Varde kernel: shapes with closed-form volumes,
areas and centroids, tangencies and near-ties, flush faces, names that
must survive, refusals that must be the right kind, and determinism. This
note lists them by area: each entry gives the test's name, its file
(relative to `crates/kernel-compat/src` at the tag), the geometry, what it
checks (with the expected values where the test had a closed form) and why
it is worth having. Three of them also run in today's `kernel-compat`
under the same names, on the motions alone or on hand-built meshes:
`quarter_turns_are_exact`, `bits_tell_motions_apart` and
`mutated_meshes_are_refused_unless_still_manifolds`.

Many of them share one oracle, "right or refused": an operation may fail
with a named error, but a result it gives must be a valid solid whose
volumes agree with the closed form, or with the identities
`|A∪B| + |A∩B| = |A| + |B|` and `|A−B| = |A| − |A∩B|`, within a bound
from the fit tolerance and the operands' areas (some curved tests instead
take a bound relative to the operands' volumes where every cut is exact,
or one from the fit and the result's fitted area). "On 1 and 8 threads"
means the test ran the operation on thread pools of both sizes and
compared the results to the bit (by their `Debug` text, which is
bit-exact for floats).


## Boolean, curved

- **`a_cylinder_through_a_box_is_exact`** (`boolean/curved_tests.rs`).
  Geometry: slab 4x4x2 (x,y in [-2,2], z in [0,2]); upright cylinder r=1,
  h=4, axis through the slab centre, base z=-1 (pierces both faces). All
  four of A+B, A&B, A-B, B-A, and A&B vs B&A. Checks: A&B volume = 2 pi (to
  1e-12), the other three by inclusion-exclusion; every patch of every
  result lies on its face's plane or quadric to 1e-12 x the size (no
  fitted/free patches); the drilled slab keeps the cylinder's wall face
  under the cylinder's feature id. Why: the baseline exactness case;
  plane-cylinder cuts must be exact circles, not approximations.

- **`crossing_cylinders_are_traced_within_the_tolerance`**
  (`boolean/curved_tests.rs`). Geometry: upright cylinder r=1, z in [-2,2]
  on the origin, crossed by a cylinder along x with r=0.7, axis at
  (y,z)=(0.1,0.2), x in [-2,2]. All four ops. Checks: volumes against a
  numeric closed form for the intersection of two perpendicular cylinders
  (Simpson integral of the product of chord widths), within fit tolerance x
  total area / 10; every vertex away from the ends (|x|, |z| < 1.99) lies on
  at least one of the cylinders to fit/4; fitted bands exist (free patches >
  0) while the non-cut patches stay exact to 4e-12. Why: curve-curve
  intersection (a non-planar space curve) is the case that cannot be exact;
  tests that the traced curve stays within the stated tolerance and the
  exact parts stay exact.

- **`a_ball_cut_by_a_box_through_its_axis`** (`boolean/curved_tests.rs`).
  Geometry: ball R=1.25 revolved about y (two quarter arcs plus an axis
  line, so the pole and seam lie on the axis), against the box [-0.75,0.75]
  x [0,2] x [-0.25,0.75] that covers the pole and straddles the seam. All
  four ops. Checks: ball&box volume vs a 1500x1500 midpoint integral of
  min(2, sqrt(R^2-x^2-z^2)); all four results (union, intersection, both
  differences) within fit x (areas)/5, and A&B = B&A. Why: sphere pole plus
  seam plus a cut through the axis: a regression where the seam rules
  flipped a decision and the intersection hit the complexity bound.

- **`a_pin_through_a_holes_wall`** (`boolean/curved_tests.rs`). Geometry:
  plate 6x4x1 (x in [-3,3], y in [-2,2], z in [0,1]) with a through hole r=1
  at (0.5,0.2); upright pin r=0.3 at (1.4,0.2), z from -1 to 3, crossing the
  hole's wall (centres 0.9 apart, so the circles overlap in a lens). Checks:
  all four ops; pin&plate = pi r^2 - lens(0.3, 1, 0.9) with the lens by the
  circle-circle area formula, to 1e-10; all patches exact to 1e-12 x the
  size. Why: two parallel-axis cylinder walls cross in two straight lines
  (exact rulings), which the pairing code must treat as exact, not traced.

- **`a_boss_joined_flush`** (`boolean/curved_tests.rs`). Geometry: the plate
  above (with its hole) and a boss r=0.4 at (-1.5,0), z in [1,2], standing
  on the plate's top face so the boss's base is coplanar with the plate's
  top. Checks: union volume = plate + boss to 1e-10 and all patches exact;
  intersection is empty; plate-boss equals the plate's volume. Why: coplanar
  contact where a circle's rim lies in a plane face; the plate's top must be
  cut round the foot without leaving slivers or a double count.

- **`tangent_cylinders_meet_in_no_manifold`** (`boolean/curved_tests.rs`).
  Geometry: at the coarsest fit tolerance, cylinder A r=1, h=2 at the origin
  and cylinder B r=1, h=1 at (2,0), z in [0.5,1.5]: side by side, touching
  along one vertical ruling segment. Checks: union is refused with exactly
  `Boolean(NotManifold)` (the result would touch itself along a line, like
  boxes sharing an edge); intersection is empty. A companion test checks A-B
  and B-A (either operand order) return the first operand unchanged, volume
  within 1e-9. Why: tangency along a line; refusal must be the named kind
  rather than a wrong solid.

- **`walls_tangent_along_a_line_are_decided_within_a_small_budget`**
  (`boolean/curved_tests.rs`). Geometry: at the default tolerance, (a) A r=1
  h=2 at the origin with B r=1, z in [0.5,1.5], centre (2,0), and (b) a
  plate 6x6x1 (x,y in [-3,3]) with a hole r=1 at the origin plus a pin r=0.5
  centred at 0.5*(cos 0.3, sin 0.3) (inside the hole, tangent to its wall at
  one line, off the seams), z in [-0.5,1.5]. Checks: with a budget of only
  200,000 units, all four differences return the first operand unchanged
  (volume within 1e-9), both intersections are empty, and the A-B and
  pin-plate differences are identical at 1 and 8 threads. Why: tangent walls
  have no ends and no certificate except parallel directions; refining to
  flatness once took around 3 million units. A budget and determinism test
  in one.

- **`coaxial_cylinders_stacked_or_overlapping_unite`**
  (`boolean/curved_tests.rs`). Geometry: cylinder A r=1 centred (0.5,0.2), z
  in [0,1]; B the same circle (drawn as A's four quarter arcs, or from a
  start turned by 0.3 rad in 3 arcs) over z-spans [1,2] (stacked), [0.5,2],
  [0,2] and [-0.5,0.5] (overlapping or flush at one end). Checks: all four
  ops, A&B volume = pi x overlap length (0 for the stack), union volume the
  same in both orders to 1e-9, union has at most 64 patches and exactly one
  quadric wall face. Why: coaxial walls of the same circle with a rim lying
  exactly on the other's wall; the union must merge to one face and one seam
  without leaving a zero-width ring from the cap.

- **`a_saddle_pairs_its_cut_the_right_way`** (`boolean/curved_tests.rs`,
  with the geometry in `boolean/pairs/tests.rs`). Geometry: a free-form
  solid whose top is the saddle z = 10 + 0.5(x^2 - y^2) over the triangle
  (-1.5,-1), (1.5,-1), (0,1.5) (as a quadratic patch), planar walls and
  floor z=0; cut by a slab from z=-1 up to c = 10.2 (above the saddle point:
  two hyperbola branches, two lobes) and c = 9.8 (below: one band). Checks:
  all four ops with volume identities to fit x (areas)/10, and the saddle
  minus the slab (the part above the plane) has positive volume; the
  pairs-level test
  (`a_saddle_pairs_its_ends_by_the_side_of_its_saddle_point`) also checks
  that each arc's two ends lie on the same side of x=0 (c = 10.2) or of y=0
  (c = 9.8). Why: ends of cut curves on a saddle patch do not determine
  their pairing; the choice either side of the saddle point decides the
  topology.

- **`cylinders_a_hair_apart_are_right_or_refused`**
  (`boolean/curved_tests.rs`). Geometry: at the coarsest tolerance, cylinder
  r=1, h=2 at the origin and an r=1 cylinder with axis at x = 2 + 1e-9 (a
  gap far below the tolerance), either z in [0,2] or z in [-0.5,2.5].
  Checks: all six op/order combinations; each either errors or has the
  operands as they are (union = va+vb, intersection = 0, differences = the
  minuend) within 1e-6. Why: near-tie of a tangent pair; the seam vertex's
  ray once treated the gap as closed and put one operand inside the other;
  the contract is "right or refused", never silently wrong.

- **`a_countersink_is_exact`** (`boolean/curved_tests/cones.rs`). Geometry:
  plate 6x4x1 (z in [0,1]) against a revolved countersink tool (r=0.5 shank
  from z=-0.5 to 0.5, a 45-degree cone out to r=1.5 at z=1.5, so r=1 at the
  plate top); both upright and on a frame rotated about x by 0.4 and z by
  0.3 and shifted. Checks: tool&plate = pi 0.25 x 0.5 + (pi 0.5 / 3)(0.25 +
  0.5 + 1.0) to 1e-12, all four ops' patches exact to 1e-12 x the size, the
  countersunk plate keeps a cone face on a quadric; the difference is
  identical on 1 and 8 threads. Why: cone against plane (exact circle) and
  cone-to-cylinder junction at an exact parallel, on and off the world axes.

- **`nearly_flat_cones_cut_through_their_axis_are_exact`**
  (`boolean/curved_tests/cones.rs`). Geometry: frustum of a cone opening 80,
  88 and 89.5 degrees from its axis (r=0.5 up to 20.5), its middle half (z
  from 0.25 h to 0.75 h) cut by half or a quarter of a box 22x44 / 22x22
  whose side planes pass through the axis along the cone's seams; placed at
  the origin and at (1e3, -700, 300). Checks: share x frustum volume (pi
  dz/3 (a^2 + ab + b^2), a = r0+5, b = r0+15) within 1e-12 x volumes x (1 +
  far); every patch exact. Why: long thin cone patches where Newton from the
  midpoint strayed, giving chords 3e-7 off the cone; also tests
  far-from-origin rounding.

- **`a_cross_hole_through_a_round_boss`** (`boolean/curved_tests.rs`).
  Geometry: boss r=10, h=10 on XY; a small cross hole (circle sketched on
  YZ, extruded 30 along x) with radii 0.30-1.06, centres at (y,z) such as
  (5.96, 3.98), (-7.26, 3.95), (-0.16, 5.78) (seven cases). Checks: boss
  minus hole = pi R^2 h - R^3 x crossing_volume(1, 0, r/R, s/R) (the numeric
  perpendicular cylinders integral, scaled) within fit x areas/5; case 0
  (all in a full run) identical on 1 and 8 threads. Why: a small curve
  inside one strip triangle of a big wall; checks that the placed points for
  the cut faces are bounded and deterministic (repair once ran out of budget
  on thin fans).

- **`tangent_contacts_are_right_or_refused`**
  (`boolean/curved_tests/tangent.rs`). Geometry: a table of minimal tangent
  cases at millimetre scale (k=30), such as a 2x4x1 plate against a cylinder
  r=1 outside it or inside touching both long sides (as tall, taller,
  standing on it or through the top), a plate with a hole r=0.5 and slots
  ending in or crossing the hole's wall, and a cylinder standing on a
  cylinder; each case lists which of the four ops must work. Checks: each op
  either errors or its volume is within 1e-9 of the operands' total (or
  fit/10 x free area if a cut is fitted); the ops marked "must work" do
  work. A companion test runs four cases (the cylinder inside the plate as
  tall, through and through the top, and the slot beside the hole) at the
  finest tolerance and unit size, requires their intersections to work, and
  checks one of them on 1 and 8 threads. Why: a systematic catalogue of
  tangent contact, with an "expected refusal" matrix; the tangency leaves
  crossing pairs roughly 2e-7 apart that clean-up must not mistake for
  geometry.

- **`spheres_touching_faces_at_a_point_are_right_or_refused`**
  (`boolean/curved_tests/tangent.rs`). Geometry: on a frame turned off all
  axes, a 4x4 plate and a sphere r=0.5 resting on its top (plate z in [0,1],
  sphere centre at height 1.5), inside it touching its top from below (plate
  z in [-1,1], centre at 0.5) or between top and bottom (centre 0.5, plate
  [0,1]); and a cylinder r=0.25 at x=0.75 beside a sphere r=0.5 at the
  origin (touching at a point). Checks: exactly the unions of the outside
  contacts (sphere on the top, cylinder beside the sphere) and plate minus
  ball for the inside contacts (a void touching the shell) are refused, each
  as `NotManifold`; the rest work with volumes equal to operands
  (intersection = ball volume if inside, else 0) within 1e-9. Why: point
  contacts of a quadric and a plane (not line contacts), where whether the
  result self-touches determines whether it must refuse.

- **`a_chain_of_flush_joins`** (`boolean/curved_tests/flush_seams.rs`).
  Geometry: a 60x40x10 plate with a filled hole (r=3 at (-15,1)) and an open
  hole (r=3 at (20,12)), joined in sequence with four bosses sketched on the
  plate's plane: r=8 through (z 0-20, fills one hole), r=6 flush with both
  plate caps (z 0-10), r=5 standing on it (z 10-20), r=4 through (z 0-20,
  whose top is flush with the last one's); on the world frame and on a frame
  rotated about two axes and off the origin. Checks: each step's volume
  exactly (relative 1e-12, with lens areas for overlapping boss circles),
  under 400 patches, every patch exact, the plate's two flush caps are
  single faces (at z=20 the r=5 and r=4 tops merge into one face, the r=8
  boss's top apart from them is a second), the last step identical on 1 and
  8 threads. Why: the realistic modelling workflow of repeated flush joins,
  which used to split each seam into tens of thousands of patches.

- **`cylinders_side_by_side_touch_along_a_ruling`**
  (`boolean/near/tests.rs`). Geometry: rod A r=1, z in [0,2] at the origin,
  and rod B r=1 at distance 2 + gap x resolution along phase 0 (on the
  seams) or 0.3, either as tall or z in [0.5,1.5]; gaps of 0, 0.5, 0.99, 2.5
  and 3 resolutions; at both the default and coarsest tolerance. Checks: the
  `touches` predicate, both operand orders, within a 200,000-unit budget:
  true for gaps below one resolution, false from 2.5 on. Why: near-miss
  threshold behaviour: gap = resolution boundary between touch and apart,
  the search for surfaces within the resolution. A sibling test checks the
  same for rods of r=1 and 0.7 along an off-axis direction, and a budget
  test checks a too-small budget gives `TooComplex` while the exact need
  succeeds.

- **`a_six_by_six_grid_of_holes_cut_at_once`**
  (`boolean/curved_tests/holes.rs`). Geometry: plate 100x100x10, with a 6x6
  grid of discs r=2 at 100(k+1/2)/6 extruded as one profile from z=-1.1 to
  11.1 and subtracted in one operation (an 8x8 and 10x10 version is kept
  ignored as a known-failing stress case). Checks: volume = 100000 - 36 pi 4
  x 10 within 1e-6 (the 6x6 cut must succeed). Why: scale (36 simultaneous
  cylinder cuts, long thin cap triangles around rims, holes in line with one
  another) as in a through-all cut.

## Boolean, seeded

Common oracle of the seeded suite (`four` in `boolean/seeded_tests.rs`): for
each pair, all of A∪B, A∩B, A−B, B−A are run. Each must be a valid solid or
be refused (never wrong, never a panic). Right means: the result passes the
solid check; volume identities |A∪B|+|A∩B|=|A|+|B|, |A−B|=|A|−|A∩B| hold
within fit_tol*(areaA+areaB)/5+1e-9; closed-form volumes where the test has
them; no volume below zero; random sample points (24 per op, release only,
away from both operands' surfaces by 0.01+0.3% of the box diagonal) are
inside the result exactly when the operation says, by winding numbers of
tessellations (result winding must be 0 or 1, not 2 or -1); and every shell
of the result faces the right way for where it lies (outer shells wind 0
from the others, voids wind 1), judged by an independent floating-point
oracle. Refusals are allowed but tallied, and each test asserts a minimum
success share. Debug builds run far fewer cases (`cases(release, debug)`).

- **`near_tangent_cylinders_are_right_or_refused`**
  (`boolean/seeded_tests.rs`). Generator: not random; a fixed table of 8
  (fit tolerance, gap) pairs: fit = the coarsest fit with gaps 0, +-1e-9,
  +-1e-6, 1e-3, and a mid fit 1e-2 with gaps 1e-9 and 1e-3; times 3 configs
  (z0, h, turn about axis) = (0.5, 1.0, 0), (0, 2.0, 0.3 rad), (0.5, 0.25,
  0). Geometry: upright cylinder A (r 1, height 2) at the origin; cylinder B
  (r 1, height h) with axis at x = 2 + gap, base at z0, turned about its own
  axis; so the walls are tangent, a hair apart, or a hair overlapping.
  Checks: the shared oracle, plus closed-form volumes when gap >= 0
  (intersection volume 0, union = sum, differences = operands); every
  operation must succeed unless it is the union with |gap| below the
  resolution (the only allowed refusal); no random-success floor, an empty
  "lost" list. Why: tangency and near-tangency at several tolerances; gap 0
  and +-1e-9 sit below the resolution, so results must collapse to the exact
  tangent answer or a hair overlap consistently, including turned seams.

- **`parts_built_in_chains_of_twenty_are_right_or_refused`**
  (`boolean/seeded_tests.rs`). Generator: seed 21 (samples seed 22); 3
  chains of 20 steps (debug 1 chain of 3). Each part is random: sketch plane
  XY, XZ or YZ through the origin; centre on a 0.25 grid in [-2,2]; one of
  circle r 0.25-1.5, rectangle 0.5-4, slot (stadium) with half length
  0.25-1.5 and r 0.25-0.75, rounded rectangle (corner r 0.25-0.5), or a 3-6
  x 2-4 plate with two r 0.5 holes; extruded from a grid start in [-2,1.5]
  by 0.25-2.5. Every step runs all four ops of (current, tool), then the
  next current is the union (50%), difference (40%) or intersection (10%)
  result if it worked and is non-empty. Checks: shared oracle at every step;
  at least 70% of operations succeed. Why: the way a user builds a part,
  each result feeding the next, on grid values so many faces are flush or
  coaxial; stresses accumulated tiny faces, determinism of names and growth
  of triangle counts.

- **`turned_and_moved_solids_are_right_or_refused`**
  (`boolean/seeded_tests.rs`). Generator: seed 31 (samples 32); 40 cases
  (debug 3). A is the random part above, rotated by a random axis/angle (0-6
  rad) and shifted within +-2; B is, with equal chance, a random box (sides
  0.5-3, centre within +-1.5) or a cylinder (r log-uniform 0.2-1.5, length
  4) turned about a random axis and moved. Checks: shared oracle for all
  four ops, at least 75% succeed. Why: arbitrary orientation means no
  axis-aligned coincidences are left to luck; curved and flat walls cross at
  general angles with rounding in every coordinate.

- **`flush_unions_either_order`** (`boolean/seeded_tests.rs`). Generator:
  seed 93 (samples 92); 30 cases (debug 2). One sketch plane (XY, a Y-normal
  (ZX) plane at an offset, or a randomly turned frame), at millimetre scale
  (k=10, height 10) or unit scale (k=0.1, height 0.5), values on a grid of
  k/20. First solid: a 6k square plate, or the plate drilled with one
  circular hole, or a random curved outline (circle, rounded square or
  slot); second: a curved outline over the same plane spanning (0..h),
  (0..2h), (-h..h) or (0.5h..h), so its cap is flush with the first's top,
  its bottom is coplanar, or it stands through. Both operand orders, all
  four ops. Checks: shared oracle; unions must succeed at least 95% (a union
  whose triangle count exceeds 10x its operands' counts as failed); all
  operations at least 86%. Why: flush caps meeting along curved rims, a
  union that has to keep one cap whole and cut the other along a curve in
  the same plane; guards against repair splitting seams into thousands of
  flat pieces and tests order independence.

- **`plates_drilled_hole_after_hole`** (`boolean/seeded_tests.rs`).
  Generator: seed 81 (samples 82); 4 plates (debug 1) of x in [4,12], y in
  [3,8] on a 0.25 grid, thickness 1; hole radius one of 0.25, 0.4, 0.5; 10
  holes (debug 2) each a cylinder of height 3 from z=-1, placed either in
  rows (x = 1+1.5*(k%4), y = 1+1.5*(k/4)) or at random grid positions inside
  the plate with 0.75 margin. Each step does all four ops with the drill and
  keeps the difference as the next plate. Checks: shared oracle at every
  step; at least 95% of operations succeed (148 of 160 worked before sliver
  flipping was added). Why: repeated cuts into the long thin cap triangles
  earlier holes left, at glancing angles where rims share a tangent; models
  placing features one at a time; also hits holes that nearly touch.

- **`bosses_sunk_through_drilled_plates`** (`boolean/seeded_tests.rs`).
  Generator: seed 5 (samples seed 6), first 40 cases (debug 2) of a 150-case
  family (the 150-case version is `#[ignore]`). Plate: 6x4x1 slab at
  (-3,-2,0), drilled by two through cylinders (centres in x in [-2.2,-0.5] /
  [0.5,2.2], y +-1.2, r 0.2-0.7, on a 0.05 grid; a pair that nearly touch is
  skipped as refused). Boss: a cylinder at (bx,by) in +-1 x +-0.8, r
  0.2-1.2, in one of three modes: standing on top (z 1..2), sunk (z 0..2,
  flush with the bottom, through the top) or through (z 0..1, flush with
  both caps). Checks: shared oracle, plus an exact expected volume: the
  shared area is the area of the boss disc less two hole discs and clipped
  to y within +-2, computed in closed form (`disc_less_discs`) times the
  overlap height, so all four results are compared to va+vb-both, both,
  va-both, vb-both; any result over 20x the operands' triangles counts as
  failed; at least 92% succeed. Why: a boss wall crossing holes' walls and
  caps, rim tangent to a hole, rim vertex on another rim, all
  cylinder-on-cylinder with flush caps; closed-form areas (lens areas of
  overlapping circles) give independent ground truth without trusting any
  kernel result.

- **`seeded_booleans_are_deterministic`** (`boolean/seeded_tests.rs`).
  Generator: seed 71; 4 cases (release only; debug 0) of `related` pairs on
  snapped (0.25 grid) values: a cylinder and a second solid (cylinder,
  square prism, or a cross bar) on a random sketch plane or random turned
  frame, stacked flush, over the same span, inside its span, crossing, or a
  bar across. Checks: union and difference of each run with the thread-count
  variation helper (`assert_deterministic`) and give identical meshes (and
  identical errors) regardless of worker threads. Why: byte-identical output
  across thread counts on exactly the flush/coaxial cases where tie
  decisions could depend on ordering.

- **`the_closed_form_areas_are_right`** (`boolean/seeded_tests.rs`).
  Geometry: the 2D reference `disc_less_discs(centre, r, half, holes)`, area
  of a disc clipped to |y| <= half less hole discs, integrated exactly
  between arc intersections. Checks: single disc of r 0.7 equals pi*0.49
  within 1e-12; disc of r 1 centred (0,1.5) clipped by y <= 2 equals pi
  minus the circular segment (acos(0.5) - 0.5*sqrt(0.75)) within 1e-12; two
  overlapping holes (r 0.4 and 0.35), one containing the outer disc's centre
  (the test's comment says the other hole's), matches a 2000x2000 grid
  sample within 1e-3. Why: the oracle behind the sunk-boss test is itself
  tested, with the awkward case of overlapping holes.

- **`boss_rim_tangent_to_cap_edges_between_holes`**
  (`boolean/seeded_tests.rs`). Geometry: 12 x 12 x 1 plate (z 0..1) with
  eight r 0.4 holes at (+-1,+-3) and (+-3,+-1) style symmetric positions,
  unioned with a boss of r 3 (z 1..2) centred at the origin, whose rim
  passes tangent to the plate's cap edge line x = 3 between two holes, at
  the boss's own vertex (3, 0). Checks: the union succeeds and its volume
  equals plate + boss within 1e-9. Why: a tangent rim against a cap edge at
  a shared vertex; crossings placed a micrometre apart in the wrong order
  once gave `Inconsistent`, and alternating in/out left zero-width
  triangles.

- **`coaxial_solids_and_pins_in_holes`** (`boolean/seeded_tests.rs`).
  Geometry: on each of five frames (sketch planes and turned ones), a 6x4x1
  plate with an r 1 hole at (0.5, 0.2) against a pin cylinder r 1 in the
  hole over spans (-1..2), (0.5..2), (0.25..0.75), (1..2), (0..1); plus
  cylinder pairs: r 1 over 0..1 against r 1 / r 0.5 over 1..2, 0..2, 0.5..2
  and so on. Checks: shared oracle plus exact volumes (pin and plate share
  0; coaxial overlap pi*r^2*overlap height); at least 97% succeed (195 of
  200; the known refusals are the union of the pin standing on the plate
  (span 1..2), touching it only along the hole's rim, on each of the five
  frames). Why: walls on one cylindrical surface (coincident curved faces),
  a pin in a hole of the same radius, flush caps; the only-along-a-rim
  contact is non-manifold.

- **`flush_pairs_keep_their_names_either_order`**
  (`boolean/seeded_tests.rs`). Geometry: the same flush pairs as the
  flush-union test (seed 93, 20 cases, debug 2); each pair's union and
  intersection computed both A,B and B,A. Checks: the set of face keys (and
  aliases) on the result is identical either way round; at least 30
  comparisons made. Why: references to faces must not depend on operand
  order when flush caps were merged (5 of 35 pairs differed before).

## Boolean, flat

Boxes use `cube(min, size)`; `case` runs union both ways, intersection both
ways, and both differences, checks each volume within 1e-9 (relative) and
that a zero volume is the empty solid, that each plane face's normal agrees
with its triangles' facing, or, where the expectation is `None`, that the
failure is `NotManifold`. Grid boxes sit on a half-unit grid of 8^3 cells
(corner -0.5 + 0.5*i) and are checked against the cell sets (volume and
manifoldness of the cell set decide whether a refusal is allowed).

- **`boxes_flush`** (`boolean/tests.rs`). Geometry: A = [0,2]^3 (volume 8)
  against: itself (same box); a 1x1x1 pocket at (0.5,0.5,1) flush with the
  top (union 8, intersection 1); a 2x2x2 box slid by 1 in x (shares four
  face planes, overlap 4); box (1,1,0)-(3,3,2) (shares two, overlap 2);
  (1,1,1) size (2,2,1) (shares one plane, overlap 1); a slab (-1,0,0.5) size
  (4,2,1) through flush with two sides (overlap 4); a unit cube inside at a
  corner (1,1,1) flush with three planes (overlap 1). Checks: volumes of all
  four results equal va+vb-both, both, va-both, vb-both in both operand
  orders. Why: every coincident-plane configuration from one to six shared
  planes, the core exactness case for a perturbation scheme.

- **`boxes_touching_along_an_edge_or_a_corner`** (`boolean/tests.rs`).
  Geometry: A = [0,2]^3 against a 2-cube sharing only the edge x=y=2 (box at
  (2,2,0)) or only the corner (2,2,2) (box at (2,2,2)). Checks: the union is
  refused as `NotManifold` (both orders); intersection is the empty solid
  (volume 0), the differences give A and B unchanged (volume 8 each). Why:
  non-manifold contact (edge or point touching) must be refused, not
  repaired into a wrong body, while the other three ops stay exact.

- **`empty_operands`** (`boolean/tests.rs`). Geometry: a 2-cube against
  `Solid::empty()`. Checks: A union empty equals A (both orders); A minus
  empty equals A; empty minus A is empty; A intersect empty is empty. Why:
  identities with the empty solid should not touch the pipeline or errors.

- **`grid_boxes_folded_sheets_on_turned_frames`** (`boolean/tests.rs`).
  Geometry: five fixed chains (recorded from a seeded run), each: a starting
  grid box and 2 or 3 union/intersection/difference steps with other grid
  boxes; each chain extruded on its own turned and moved frame (an explicit
  quaternion and a shift up to about +-100), so coordinates carry rounding
  and plane tags stay. Checks: every step's volume matches the cell set
  within 1e-6, results are never refused, and plane faces face out. Why:
  zero-thickness sheets folded on a flush face (previously failed with
  `Fold` or the hull check) on faces flush only to rounding; fixes by moving
  a vertex within its star's planes.

- **`turned_grid_boxes_tied_at_higher_orders`** (`boolean/tests.rs`).
  Geometry: rerun the random pairs from seed 31 (index 0 to 594), selecting
  pairs 117, 130, 239, 452, 509, 561, 573, 594: two grid boxes (random
  corners and sizes), both turned by a random axis/angle (0-6 rad) and
  shifted by up to +-100 as free-surface meshes. Checks: all three ops give
  the cell set's volume within 1e-6 and none fails. Why: collinear edges and
  vertices moving along a face have a zero first-order perturbation, so the
  decision must continue to higher orders instead of resolving by rounding
  noise (previously `Inconsistent`).

- **`near_ties_that_dont_fit_together_are_decided_again_exactly`**
  (`boolean/tests.rs`). Geometry: seed 5; pairs 238 (difference), 492
  (intersection), 646 (union): turned grid boxes, B additionally nudged in a
  random direction by the tie distance times 10^[-1.5,1.5]. Checks: deciding
  with near ties as ties fails as `Inconsistent`; on 646 the failure's
  evidence is two triangles from A and B and at least two points each within
  the tie of one triangle's plane and within the resolution of both; the
  exact retry (tie 0) succeeds; the two-attempt version costs exactly the
  sum of both attempts' work; the whole operation succeeds with volume
  within 1e-5 of the cell set, and the boxes are reported touching. Why:
  documents the two-stage strategy for near ties (tie as tolerance first,
  exact if decisions disagree) and its budget accounting; over 3 000 such
  operations none was wrong.

- **`tetrahedra_sharing_a_face_askew`** (`boolean/tests.rs`). Geometry: 24
  pairs (seed 9) of tetrahedra over one random triangle (corners within +-2
  of the origin, normal length at least 0.5) not aligned with axes: the
  first has its apex above the triangle at height 0.5-2; the second shares
  the same three corner numbers and either has an apex inside the first
  (even pairs) or on the far side at depth 0.5-2 (odd pairs). Checks: all
  operations succeed; union, intersection, A-B, B-A volumes are
  [va,vb,va-vb,0] (inside) or [va+vb,0,va,vb] (far side) within 1e-9. Why:
  faces flush exactly but with oblique normals; the perturbation must move
  every face of a thin corner outward (the sum of normals is not always
  enough), otherwise slivers appear.

- **`faces_keep_their_names`** (`boolean/tests.rs`). Geometry: a 2-cube
  [0,2]^3 (feature 1) and an octahedron of radius 1 centred (2,2,2) at its
  corner (feature 2, each triangle its own plane face). Checks: for union,
  intersection and difference the result succeeds, contains faces of both
  features 1 and 2 (the difference carries the tool's faces on the cut), and
  plane faces face outward. Why: face identity (the naming used for
  references) survives booleans of mixed operands overlapping round a
  corner.

- **`stacked_boxes_are_one_box_of_faces`** (`boolean/tests.rs`). Geometry:
  unit cube (feature 1) and a unit cube on top at z 1..2 (feature 2); union
  both orders. Checks: volume 2; the result has exactly 6 face keys (merged
  side faces), non-cap faces carry the first operand's feature; every key of
  either operand's side faces resolves via topology lookup to a face of the
  result (aliases); no drawn feature edge lies at z = 1. Why: coplanar
  neighbours merge into single faces, with names kept stable as aliases, so
  the result is the same as one box and no seam edge shows.

- **`a_pinch_comes_with_its_two_vertices`** (`boolean/tests.rs`). Geometry:
  2-cube united with a 2-cube sharing only an edge (box at (2,2,0)) or only
  a corner (box at (2,2,2)), either order, at default tolerance. Checks:
  fails `NotManifold`, deterministically at 1 and 8 threads; the failure
  evidence carries the offending triangles within the operands' box and one
  or two points, all within the resolution of the edge x=y=2 (z within
  [0,2]) or the corner (2,2,2); two points are distinct and within the
  clean-up's short length of each other. Why: a failure should tell where it
  is, not just that it is.

- **`turned_grid_boxes_chained_are_never_inconsistent`**
  (`boolean/tests.rs`). Geometry: seed 77; 100 chains (debug 5) of up to 6
  steps: a random grid box turned and shifted by one random rigid motion,
  each step a random op (union, intersection, difference) with another
  random grid box under the same motion, the result fed on (stops when
  empty). Checks: each step's volume equals the cell set's within 1e-6;
  refusals only as `Invalid` or `NotManifold`, never `Inconsistent`; at
  least 349 of about 359 steps right. Why: chained flush faces are near ties
  with zero first order, the situation that produced `Inconsistent` (12 of
  356 steps) before ties were decided at every order.

- **`boxes_flush_with_a_slanted_wall_on_tilted_frames`**
  (`boolean/tests.rs`). Geometry: hexagonal prism (circumradius 3, 4 tall);
  boxes extruded from a frame on one slanted wall: boss flush on the wall,
  pocket cut flush, straddling, at the wall's ends (sharing prism edge
  lines), flush with the prism top, a wall-high slot; the prism upright and
  on 6 random turned and moved frames (2 in quick runs). Checks: every
  operation works and has the analytic volume to 1e-9 (in a past 60-frame
  run, 8 of 2 160 failed as `Invalid(Hull)`, the boss in a corner on turned
  frames). Why: flush to rounding on an oblique plane with shared edge
  lines.

- **`a_void_thinner_than_the_resolution_fails_at_once`**
  (`boolean/tests.rs`). Geometry: cylinder (r 5 or 1, height 10) minus a
  blind cylinder from z=2, height 6, r smaller by half the resolution, so a
  void with a wall of half a resolution. Checks: fails `NotManifold` within
  a small budget of 100 000 (rather than burning the full budget and
  returning `TooComplex`). Why: a result closer to itself than the
  resolution over an area is a refusal that must be fast and named.

- **`tori_keep_the_volume_identities`** (`boolean/tests.rs`). Geometry: flat
  torus (48 x 24 quads, 2 304 patches, major 3, minor 1) against itself
  turned upright (a quarter turn about x and 0.1 about z, shifted (1.5, 0.1,
  0.2)) through its hole and tube, and against a block (0.5,-0.7,-2) size
  (4, 1.3, 4). Checks: intersection volume above 0.1; |A union B| + |A
  intersect B| = |A| + |B| and |A minus B| = |A| - |A intersect B| within
  1e-9*(va+vb); plane faces face out. Why: curved, high-patch-count bodies
  with no flush faces, checked by volume identities alone.

## Extrude

- **`a_cylinder_is_exact`** (`extrude/tests.rs`). Geometry: one circle
  (centre (1,-2), curve 0) extruded on the XY plane from -h to 0, for (r,h)
  in (2,5), (0.01,3), (400,0.5), (0.3,1000). Checks: volume = pi r^2 h and
  area = 2 pi r^2 + 2 pi r h within 1e-12 relative; faces are start cap, end
  cap, then exactly four side faces (curve 0, segments 0..3). Why: exactness
  of a round wall across over four orders of magnitude of radius and extreme
  aspect ratios (flat disc, long rod); the circle splitting into four
  numbered segments is pinned.

- **`a_plate_with_holes`** (`extrude/tests.rs`). Geometry: 100 x 60
  rectangle outline, three circular holes (r=5, 5, 3) and one reversed
  (clockwise) 10 x 10 square hole, extruded 0 to 8. Checks: volume (6000 -
  59 pi - 100) * 8 and area from profile area/perimeter, within 1e-12;
  draws. Why: multiple holes of mixed kind (round and polygonal) in one
  profile, hole orientation convention, closed-form check.

- **`a_slot_and_a_d`** (`extrude/tests.rs`). Geometry: slot (length 20
  between centres, radius 3: two lines and four quarter arcs), a half disc
  (two quarter arcs plus diameter, extruded 1 to 4) and a lens (one quarter
  arc plus its chord). Checks: volume (2rl + pi r^2)*2, pi r^2/2 * 3, and
  area from perimeter, within 1e-12 relative. Why: lines meeting arcs
  tangentially (slot), square (D) and at 45 degrees (lens), arcs sharing a
  curve id across a smooth joint, minimal two-segment loop.

- **`an_ellipse_and_a_parabola`** (`extrude/tests.rs`). Geometry: unit
  circle's four rational quarter arcs scaled by (5,2) giving an exact
  ellipse; a parabolic arch (weight 1 conic from (2,0) via (0,4) to (-2,0),
  closed by its chord), both extruded height 1. Checks: volumes pi*5*2 and
  2/3 of its 4 x 2 box (= 16/3), within 1e-12. Why: non-circular conic
  sections as walls, so curved walls are not assumed to be circles; closed
  forms from rational quadratics.

- **`a_tilted_frame_far_out`** (`extrude/tests.rs`). Geometry: 22 x 17
  rectangle with a r=4 hole, on a frame with a skew unit axis (1,2,3)/|.|
  and origin (3e4,-2e4,1e4), extruded 2 to 7. Checks: volume = area * 5
  within 1e-9 relative; end cap plane normal equals the frame normal (1e-15)
  and passes through origin + 7 n (1e-9). A second run with axes a hair off
  orthogonal and off unit length (5e-10) still passes the face-tag check at
  1e-5. Why: arbitrary orientation and large coordinates (rounding far from
  the origin), near-degenerate frame tolerance, plane equation accuracy.

- **`random_plates_with_holes`** (`extrude/tests.rs`). Geometry: 30 seeded
  random outlines (3-12 corners, scale 100, each side line or random conic)
  with 0-5 random reversed holes, at fit tolerances MIN_FIT, 1e-3, MAX_FIT,
  random heights 0.1-100. Checks: each case is either built, with volume =
  profile area * h to 1e-12 * 1e4 * h, or refused as touching or non-nesting
  loops (nothing else allowed); at least 10 built; the total triangle count
  is pinned (1636). Why: randomised right-or-refused property over curved
  holes at every tolerance, plus a regression pin on mesh size to catch
  quality or refinement drift.

- **`crowded_boxes_run_out_of_budget_not_memory`** (`extrude/tests.rs`).
  Geometry: a 65,535-vertex star polygon of radius 100 (each chord nearly a
  diameter, 0.28 degrees from the previous), valid as a profile; extruded
  with a budget of 2^20. Checks: profile check passes; extrude returns
  `TooComplex`. Why: some two billion overlapping bounding-box pairs must
  hit the budget before being collected, so the failure is a clean refusal
  instead of memory exhaustion. Adversarial scale and budget enforcement.

- **`sides_are_numbered_per_curve_in_profile_order`** (`extrude/tests.rs`).
  Geometry: 10 x 10 square whose segments 1 and 3 are given curve 5, with a
  circular hole (4 arcs) also curve 5, extruded 0 to 1. Checks: face order
  is start cap, end cap, then side(0,0), side(5,0), side(2,0), side(5,1),
  side(5,2..5): the per-curve segment counter keeps counting through the
  second loop. Why: stable face naming, which downstream features rely on; a
  curve spanning non-adjacent segments and several loops.

- **`bad_input_is_refused`** (`extrude/tests.rs`). Geometry: unit square and
  variants. Checks: zero, reversed, NaN and over-large (2e6) extents give a
  patch error; a skewed frame errors; an origin at z=9.9e5 is fine but a 2e4
  extrusion from there gives a coordinate error; empty profile -> Empty;
  overlapping squares, corner-touching squares and two crossing circles ->
  Touching (naming original, not halved, segments); bow-tie fails; wrong-way
  hole, lone hole, outer loop inside another -> Nesting(loop); an island in
  a hole is accepted; a two-arc cusp -> Cusp(0,0); a line whose control
  point lies past its end -> Degenerate; height 1e-7 -> Invalid (scale
  limit, never TooComplex); budget 10 -> TooComplex. Why: one catalogue of
  every refusal kind with the exact error variant and its index, including
  valid near-misses (island, large-but-legal coordinate).

- **`extruding_is_deterministic`** (`extrude/tests.rs`). Geometry: 100 x 60
  plate with a r=5 hole, a r=20 hole and a reversed slot (5 between centres,
  radius 1, shifted to (20,50), curves renumbered), extruded 0 to 3. Checks:
  the whole solid is identical across thread counts
  (`assert_deterministic`), and volume = area * 3 within 1e-12. Why:
  bit-identical output regardless of parallelism, on a profile mixing arcs,
  lines, holes and a large hole near the outline.

- **`a_thin_band`** (`extrude/tests.rs`). Geometry: ring with outer radius
  10 and inner radius 9.9, then 9.999 (widths a hundredth and a
  ten-thousandth of the radius), extruded 0 to 1. Checks: volume pi(10^2 -
  r_in^2) within 1e-13 of the full disc's volume; area within 1e-12 of the
  disc; arcs are halved so the pieces are still named per curve (four walls
  per circle) and triangle count exceeds the plain minimum. Why:
  near-tangent concentric loops forcing arc subdivision so chords of the two
  circles stay apart; extreme tolerance on accumulated rounding.

- **`rounded_and_bitten_outlines`** (`extrude/tests.rs`). Geometry: 10 x 6
  rounded rectangle (r=1.5, lines tangent to convex quarter arcs); 10 x 10
  square with a half disc of radius 3 bitten from its top (concave arcs
  meeting the edge square); an S-shape with a convex arc running smoothly
  into a concave one. Checks: volume and area via perimeter formulas at
  1e-12 (bitten volume (100 - 4.5 pi) * 2 at 1e-11). Why: tangent joins and
  both convexities of arcs on the outer loop.

- **`fine_polygons_at_coarse_tolerances`** (`extrude/quality_tests.rs`).
  Geometry: regular n-gons of radius 10, (n, fit) = (1024, 0.1), (2048,
  0.1), (4096, 0.1), (4096, 1e-2), (8192, 1e-3) (quick set: 1024 at 0.1,
  4096 at 1e-2), extruded height 1. Checks: built identically under
  `assert_deterministic`, face tags pass `check_faces`, volume equals the
  straightened profile area * h within 1e-12; none refused. Why: sides and
  angles far below the resolution make cap triangulation produce slivers and
  near-coincident vertices; this is the stress that cap quality refinement
  exists for.

- **`a_tapered_circle_is_a_cone_frustum`** (`extrude/taper/tests.rs`).
  Geometry: disc of radius 3 extruded 0 to 4 with a 15 degree draft angle
  (companions: 10 degree square boss, negative taper widening, symmetric
  -4..4 taper, tapered slot, pockets cut into a 20 x 20 x 10 plate by
  tapered tools). Checks: volume pi h/3 (r^2 + r r_top + r_top^2) with r_top
  = r - h tan(angle) to 1e-12; all faces claim an exact form (no fitted
  faces); at least one wall is an exact cone. Why: drafted walls with closed
  forms (frustum formulas) and exact cone/plane forms; boolean pocket volume
  = plate - frustum. These tests were ignored in the old kernel as taper was
  not built, so they are scenario ideas, not validated ones. Related
  refusals: a taper that closes a narrow profile (1-wide strip at 10 degrees
  closes about 2.8 up; a circle shrinking to nothing) -> `Closes`; a -89
  degree widening beyond the coordinate range -> `OutOfRange`; zero taper
  equals the plain extrude exactly.

## Revolve

- **`shapes_are_solids_of_their_volume_and_area`** (`revolve/tests.rs`).
  Geometry: 13 profiles in the xz plane about the z axis: disc, washer,
  cylinder, cone, hollow frustum, sphere, hollow ball (shell), torus, a
  lathe part (base, chamfer, neck, quarter round, shoulder, pin, ball end),
  a groove with a concave quarter round, a ring with a round hole, a cavity
  between two axis edges, and a diamond with a lone vertex on the axis (part
  turns only). Each swept Full, 30, 90, 270 degrees and -45 to 60 degrees
  (default runs two sweeps per shape). Checks: mesh `check` and face-tag
  check; volume = theta * (integral of x^2 dy / 2) and area = theta *
  (integral of x ds) (+ 2 * profile area for part turns), by Simpson
  integration (Pappus), within 1e-10 relative (plus half the fit tolerance
  times area for fitted faces); turned inside out the solid is refused; it
  tessellates. Why: the core matrix of shapes x sweeps with independent
  closed forms, covering planes, cones, spheres, tori, poles and axis edges.

- **`exact_faces_are_exact`** (`revolve/tests.rs`). Geometry: shapes that
  need no fitting (disc, washer, cylinder, hollow frustum), full turn and a
  10 to 100 degree part turn. Checks: the usual volume/area checks, and on
  the full turn no face is `Surface::Free`: every face claims an exact
  surface. Why: line profiles must stay analytic, not be approximated;
  guards against silently degrading to fitted patches.

- **`part_turns_start_and_end_where_asked`** (`revolve/tests.rs`). Geometry:
  washer (rectangle x 2..3, z 0..1) turned (0,30), (-45,60), (90,180) and
  (200,470) degrees about z. Checks: the start cap is a plane through the
  axis at angle `from`, normal pointing back, the end cap at `to` with
  normal pointing on (normals within 1e-12, plane offset 0), plus all Pappus
  checks. Why: angle conventions, orientation of the cap planes, angles
  outside [0,360) (from -45, to 470).

- **`spheres_drawn_a_hair_off_the_axis_are_solids`** (`revolve/tests.rs`).
  Geometry: a 170 degree band of a radius-5 circle (rational weight 0.087)
  centred at x = off, closed by a wall at 0.2 of its start x, for offsets 0,
  +-0.02 res, 0.2 res, 0.99 res, -0.5 res; swept Full and 270 degrees.
  Checks: each is a valid solid with Pappus volume/area; the band's face is
  classified as a sphere only for the offsets within a few hundredths of the
  resolution (0, +-0.02), else a torus or lemon fitted form. Why: sphere
  detection tolerance when the arc centre is almost on the axis, with a
  near-degenerate rational weight; solid validity must hold on both sides of
  the classification threshold.

- **`a_spindle_and_a_lemon`** (`revolve/tests.rs`). Geometry: a lemon (two
  arcs of radius 5 with centre at x=-3, across the axis, both ends on the
  axis at z = +-4, closed along the axis); a spindle torus outside (circle
  of radius 3 centred x=2, from -60 to +60 degrees, closed by two lines in
  towards the axis); and a walled spindle (same arcs closed by a straight
  wall). All five sweeps. Checks: Pappus volume and area (fitted slack),
  face tags and inside-out refusal. Why: arc circles that reach across the
  axis while the profile does not (self-intersecting torus), a classic
  fragile case; the wall meets the arcs in creases that only the pencil
  splitting parts (without it, 229,136 patches, past the budget).

- **`vertices_near_the_axis_are_put_on_it`** (`revolve/tests.rs`). Geometry:
  cylinder (radius 3, height 5) whose axis-side edge is drawn at x = +0.5
  res, -0.5 res and +res off the axis, full turn. Checks: volume pi*9*5
  within 1e-9 relative; no face is generated for the axis side (curve 4).
  Why: snapping to the axis within the resolution, either side, including
  the boundary value, so a slightly imprecise sketch still produces a clean
  solid.

- **`revolves_are_the_same_on_any_thread_count`** (`revolve/tests.rs`).
  Geometry: the lathe-part profile revolved Full and the
  ring-with-a-round-hole profile revolved from -45 to 60 degrees, in a
  random frame (random origin up to 1e3, random axis). Checks: the result
  (solid and volume) is identical across thread counts under
  `assert_deterministic`. Why: determinism for fitted tori, spheres, cones
  and axis edges in a tilted frame far from the origin.

- **`profiles_across_the_axis_are_refused`** (`revolve/tests/refusals.rs`).
  Geometry: a rectangle x in [-1,3]; a profile with a conic bulging to x < 0
  from end points on the axis; a rectangle with the axis side drawn at x =
  -2 res. Checks: `CrossesAxis(loop, segment)` with the exact indices, for
  full and part sweeps (the -2 res case for a full turn only), the -2 res
  case refused while the 0.5 to 1 res cases of the vertices test are
  accepted. Why: the boundary of the axis snap tolerance, and a curved
  segment crossing the axis between its end points.

- **`profiles_touching_the_axis_at_a_point_are_refused_in_a_full_turn`**
  (`revolve/tests/refusals.rs`). Geometry: diamond with a lone vertex at the
  origin; a parabola x = (1-2t)^2 touching the axis half way along; a lens
  whose circle (centre 2+gap, radius 2) comes within gap = 0, 1e-12, 0.5
  res, 0.9 res of the axis. Checks: the diamond is `TouchesAxis(0,0)` for a
  full turn but a valid solid for a 2 radian part turn; the parabola and all
  lenses give `TouchesAxis` in full and part sweeps. Why: pinched-to-a-point
  results, tangential axis contact detected within rounding and within
  resolution (a past bug: the repair loop ran out of budget with
  `TooComplex`).

- **`turns_too_close_to_a_full_one_are_refused`**
  (`revolve/tests/refusals.rs`). Geometry: washer (x 1..3, z 0..1) turned 2
  pi - gap for gap = 0, 1e-12, 1e-10, 0.2 res. Checks: gap 0 gives a
  parameter error, the others `NearlyFullTurn`; gap 1e-3 radians is a valid
  solid with Pappus checks (ends 3e-3 apart at the outside). Why: a sliver
  gap between the two end caps; a clear refusal threshold rather than a
  broken solid.

- **`random_profiles_are_right_or_refused`** (`revolve/tests/random.rs`).
  Geometry: 120 seeded random profiles, alternating star-shaped loops off or
  across the axis, and loops fanned from a point on the axis; random frame
  for two thirds, tolerances 1e-2 and 1e-3, Full or random part sweep from
  -pi to pi spanning 0.1 to 6 radians, budget 2^20. Checks: each case is a
  solid whose volume and area match Pappus (within fit slack) and passes
  `check`, or is refused; at least 112 of 120 must succeed. Why: robustness
  regression over random curved profiles, with a high success floor so new
  refusals show up.

- **`rings_at_turns_are_solids`** (`sweep/lathe/tests/turns.rs`). Geometry:
  a torus (major radius 20, minor 2) with its tube cut into four fitted
  quarter bands at outside, top, inside and bottom points, built straight
  from lathe bands at fit 1e-2. Siblings:
  `profiles_with_rings_at_turns_are_solids_*` over 12 profiles with rings at
  turns (pucks, tori, an S, a boss fillet, a lip, ...), and
  `rings_just_past_turns_are_solids` with a tube, a puck and an S split 1e-5
  to 1e-3 radians past a turn. Checks: mesh passes `check` with no repair;
  volume 2 pi^2 R r^2 within half the fit times area, area 4 pi^2 R r within
  the matching bound; turned inside out it is refused. Why: seams where the
  profile arc is tangent to a parallel plane (torus top and bottom) make
  ring neighbours ambiguous; the near-turn variants stress slivers at the
  ring.

- **`crease_revolves_through_booleans`** (`revolve/tests.rs`, shapes in
  `crease_shapes`). Geometry: nine profiles revolved fully about Z whose
  corners are creases where both faces leave the ring into one quadrant of
  the meridian plane: a triangle (2,0) (5,1) (2,2) and the same flipped onto
  an outer wall, two lines leaving a corner up and out, a 10° wedge, lenses
  of 40° and 10°, a D (an arc from the bottom of its circle out and up, a
  wall down), a round just past its turn against a wall, and a dovetail as a
  control. Each is combined with a 0.8 x 0.6 x 0.7 box through the first
  corner's ring, a cylinder of 1.03 times the ring's radius around the axis
  through it, and a drill r=0.3 across the ring along x. Checks: union,
  intersection and difference each pass the mesh and face-tag checks; u + i
  = a + b and d = a − i within fit x (areas)/5 + 1e-9; at least 72 of the 81
  results work in full runs (quick runs take three shapes with the box
  only); by the test's comment the rest are refused as `Invalid` (thin tips,
  the cylinder brushing the rings), a kind the test doesn't check. A
  sibling, `creases_are_parted_by_the_pencil`, caps the patch count of each
  revolve (192 to 5632) at fits 1e-2 to 1e-5. Why: rings where neither the
  plane through the ring nor the cylinder over it parts the patches either
  side; refining there once took 229 232 patches at 1e-3 and ran out of
  budget at finer fits.

## Topology and names

- **`the_mix_is_fixed`** (`topology/tests.rs`). Geometry: none; the 64-bit
  hash that mixes a sequence of u64 (feature, part, curve ...) into a
  face-name key. Checks: pinned literal outputs for `[]` (0), `[0]`,
  `[1,2,3]`, `[3,2,1]`, `[0,0]` and for the key of feature 7, side 3
  (`0x90bb_bddc_bee9_3480`); order and length change the result. Why: names
  are stored in files, so the hash must never change across versions; the
  constants were worked out independently from the definition, a
  golden-value test for persistent naming.

- **`a_box_has_six_faces_twelve_edges_and_eight_corners`**
  (`topology/tests.rs`). Geometry: cuboid 1 x 2 x 3 at the origin. Checks: 6
  regions, 12 chains each open with one half-edge, 8 corners each of 3
  regions; the edge named (front side, top) runs exactly (0,0,3)-(1,0,3);
  the corner named (top, front, left) sits exactly at (0,0,3). Why: baseline
  for the face/edge/corner naming model and for resolving a name to exact
  geometry.

- **`names_are_the_same_whatever_the_dimensions_and_tolerance`**
  (`topology/tests.rs`). Geometry: 10 x 6 x 2 plate with a through hole (r
  1.5) and a pocket cut in from its front edge, built at scale 1, 1.37 and
  0.61, at the default and a 1e-4 tolerance. Checks: identical signature (11
  region keys, chain key pairs with closed flags, corner key sets) in all
  four; only the two hole rims are closed chains; every edge reference
  resolves to a chain with the same key pair on every variant when the pick
  point is scaled with the part. Why: a name must depend on construction
  history only, not on size or tolerance, so saved references survive
  parameter edits.

- **`several_regions_or_chains_of_one_name_go_by_the_nearest`**
  (`topology/tests.rs`). Geometry: two 10 x 4 x 2 boxes of the same feature
  20 apart (as pattern copies) with a 2-wide groove of another feature cut
  across the first top. Checks: the top key names 3 regions, the top/front
  edge key names 3 chains, the top/front/left corner key resolves by the
  pick point; each lookup with a point returns the nearest, verified
  geometrically (the surface reaches the expected point to 1e-9, edge
  distance <= 1e-12, corner vertex exactly (0,0,2) or (20,0,2)); from the
  gap it takes the left box. Why: one name can denote several pieces after
  splits and copies, and the pick point must disambiguate deterministically.

- **`a_key_merged_into_another_face_resolves_through_its_alias`**
  (`topology/tests.rs`). Geometry: 10 x 8 x 2 plate unioned with a cylinder
  boss (r 2, height 3) whose top is flush with the plate's, either operand
  order. Checks: the merged top keeps the first operand's key and lists the
  other's as its only alias; the absorbed key finds the same face from any
  pick point (including the boss's far end), and also resolves as part of an
  edge pair (exact edge keys) and of a corner (exact vertex (0,0,2)). Why:
  coplanar clean-up merges faces, and references to the absorbed key must
  keep working in both operand orders.

- **`aliases_go_with_their_faces_through_booleans`** (`topology/tests.rs`).
  Geometry: two 2-cubes offset by (1,1,1), the first with a manually aliased
  top key, combined by union, difference and intersection with it as first
  operand, and by union and intersection as second; also a lid cut that
  removes the top, and a union with an empty solid. Checks: the alias stays
  attached to the top face in every case, including on copies that claim no
  surface, and still resolves; when the top is cut away the alias is gone
  and lookup fails with "face not found"; union with the empty solid returns
  the other operand unchanged, aliases included. Why: alias bookkeeping must
  be preserved under every operation and not leak onto wrong faces.

- **`a_tie_goes_to_the_lowest`** (`topology/tests.rs`). Geometry: the
  two-box-with-groove solid, picking at x = 15, exactly midway (5 away) from
  the left box's top past the groove and the right box's top. Checks: the
  lookup returns the lowest region index of the tied candidates. Why:
  tie-breaking must be fixed and deterministic, otherwise names flip between
  runs and thread counts (the neighbour test also sends NaN, +-1e15, 1e200
  and -f64::MAX points and requires a stable, panic-free answer).

- **`rims_through_part_arcs_and_slot_ends_are_tangent_chains`**
  (`topology/tests.rs`). Geometry: prism of a profile with a 60-degree arc
  of radius 5 joined tangentially to a line, square corners, and a slot hole
  (two lines, two ends of two quarter arcs each), at tolerances default and
  1e-2. Checks: for both caps, the rim chains of curves {0,1,2} (line, arc,
  line) form one tangent set, the square corners 3 and 4 stay single, and
  the slot's six segments (curves {6,7,8,9}) form one set. A neighbour test
  shows edges turning by 0.99 degrees join and by 1.01 do not. Why: grouping
  smooth rims is needed for selecting a whole rim by one pick (fillet-style
  selections); arcs that are not quarters and arcs split in two must still
  chain.

- **`a_pin_aligned_by_its_rim_drops_into_a_hole`**
  (`topology/tests/datums.rs`). Geometry: cylinder pin r 1.5, length 5,
  rotated 90 degrees about a skew axis and translated; plate 10 x 6 x 2 with
  a hole r 1.5 at (3,3). The pin's foot-rim centre and axis are aligned to
  the hole's top-rim centre and axis (flipped, offset -2). Checks: foot
  centre lands at (3,3,0) within 1e-13 and its direction is -Z within 1e-15;
  union with the plate has volume 120 + pi*1.5^2*3 within 1e-9 relative. A
  twin test does it with a plate tilted 30 degrees about a skew axis (foot
  at the hole's bottom-rim centre within 1e-13). Why: end-to-end use of
  named edge datums (centre and direction from a name) to assemble parts;
  the closed-form volume proves the fit is exact enough that the union
  leaves no sliver.

## Measure and distance

- **`a_half_cylinder_has_its_centroid_at_four_r_over_three_pi`**
  (`measure/tests.rs`). Geometry: r = 3 half-disc (two quarter-arc conics
  plus the diameter) extruded 2. Checks: volume pi r^2 h/2 to 1e-12
  relative; centroid (0, 4r/(3 pi), h/2) to 1e-12; box (-3,0,0)-(3,3,2) to
  1e-15; two circular edges of length pi r to 1e-14. Why: exact curved
  moments from conic arcs, not tessellation.

- **`a_revolved_torus_has_its_area_volume_and_box`** (`measure/tests.rs`).
  Geometry: circle r = 2 centred at 10 from the axis, revolved fully, with
  the axis along z and along a tilted frame with an offset origin. Checks:
  volume 2 pi^2 R r^2, area 4 pi^2 R r, centroid at the frame's origin, all
  within the fit tolerance bounds (half the fit times area, etc.); tight box
  per axis is R*sqrt(1 - y_k^2) + r around the frame's origin within the fit
  and inside the control-hull box; one torus face with major 10, minor 2
  recognised to 1e-15. Why: a fitted (non-exact) surface of revolution
  measured against a closed form in an arbitrary orientation, with error
  bounds stated from the fit tolerance.

- **`conic_lengths_match_a_fine_reference`** (`measure/tests.rs`). Geometry:
  the parabola y = x^2 on [-1, 1] as a rational quadratic; a straight conic
  with an off-centre control point from (0,0,0) to (3,4,0); 300 random 3D
  conics with weights from 1/64 to 64. Checks: parabola length sqrt(5) +
  asinh(2)/2 to 1e-14; straight one exactly 5.0; random ones within 1e-13 of
  an adaptive 8-point Gauss-Legendre reference. Why: arc length of weighted
  conics where there is no simple closed form for every case; a closed-form
  anchor, a degenerate one, and a randomised comparison.

- **`a_sphere_zone_has_its_extremes_inside_its_patches`**
  (`measure/tests.rs`). Geometry: zone of a sphere of radius 5 about a
  tilted axis (0.3, 0.2, 1) normalised, between heights -4.5 and 4, in 8
  pieces per band. Checks: tight box reaches +-5 in x and y (the sphere's
  extremes, found inside the patches) within 1e-12 r; z extremes are the cap
  rims h*axis.z +- sqrt(r^2 - h^2)*sqrt(1 - axis.z^2); the box is strictly
  tighter than the control-point box; volume pi(cube(hi) - cube(lo)) and
  axial centroid match closed forms to 1e-10. Why: bounding boxes need real
  extremes of curved patches, not control hulls, and the extremes can lie in
  a patch interior.

- **`a_flat_face_of_four_square_edges_is_a_rectangle`**
  (`measure/tests.rs`). Geometry: box 2 x 3 x 4; a tilted extrude of a 4 x 3
  rectangle by 5; an L prism; a square plate with a round hole; a
  parallelogram prism; a circle prism. Checks: the box's six faces report
  side pairs [2,3],[2,3],[2,4],[2,4],[3,4],[3,4] with exactly orthogonal
  vectors and area = product; tilted extrude gives [3,4],[3,5],[4,5] twice
  each to 1e-12; L: 8 faces, 6 rectangles; plate with hole: 7 faces, 4
  rectangles; parallelogram: 6 faces, 4; circle: none. Why: rectangle
  detection must be strict on the number of edges and right angles, yet
  orientation independent.

- **`two_boxes_are_exactly_their_gap_apart`** (`measure/distance/tests.rs`).
  Geometry: unit box against another unit box placed face to face (gap 2),
  edge to edge (gap (1,2,0)) and corner to corner (gap (1,2,3)); also
  planes, edges and corners as picks. Checks: distances equal sqrt of the
  gap squared with `assert_eq` (2, sqrt 5, sqrt 14), witness points lie on
  each box and differ by the gap; corner to the far plane is 3 through the
  box. Why: exact polyhedral distance with witness points for all feature
  pairings.

- **`two_skew_cylinders_are_their_axes_less_their_radii_apart`**
  (`measure/distance/tests.rs`). Geometry: cylinder r 1 along z and a rod r
  1.5 along an in-plane direction, axes 4 apart; then a tilted rod off the
  seams. Checks: distance = 4 - 1 - 1.5 to 1e-13 for bodies and for the wall
  faces alone, witness points at the analytic positions to 1e-7. Why:
  closest points between curved surfaces, found analytically, with a closed
  form for the answer.

- **`a_tube_and_a_pin_are_their_radii_apart`**
  (`measure/distance/tests.rs`). Geometry: tube (radii 2 and 1, height 3)
  and a coaxial pin r 0.5, length 5. Checks: outer to inner wall = 1, pin
  wall to bore = 0.5, pin body to tube body = 0.5, and the inner rim to the
  outer wall = 1, each to 1e-14, each costing under 20,000 work units. Why:
  constant-distance (coaxial) surfaces should be recognised at once rather
  than split down, tested with a work bound.

- **`concentric_spheres_are_their_radii_apart`**
  (`measure/distance/tests.rs`). Geometry: two sphere zones about a tilted
  axis, radii 3 (heights -2..2) and 1 (heights -0.9..0.9). Checks: sphere
  face to sphere face = 2 to 1e-14 within 20,000 work units; the bodies are
  1.1 apart (the small one's cap to the big one's cap) to 1e-14. Why: second
  family of surfaces with a constant gap, plus the bodies case where the
  nearest point is a cap.

- **`bodies_far_apart_or_far_out_cost_little`**
  (`measure/distance/tests.rs`). Geometry: 20 x 20 plate with 64 holes (over
  3,000 triangles) against a lid box 0.5 above and against a box at
  (300,-200,50); two rods placed near the origin and at (9000,-7000,5000).
  Checks: distances 0.5 and sqrt(280^2 + 199^2 + 46^2) to 1e-12 relative,
  each under 40,000 work units; the rods' distance far out equals the one at
  the origin within 64 eps * 9000. Why: bounding trees must prune most
  pairs; results should not degrade with position.

- **`a_pin_off_centre_in_a_tube_near_the_coordinate_limit`**
  (`measure/distance/tests.rs`). Geometry: tube (radii 5 and 3, height 15)
  and pin r 2 a quarter off-axis, in a frame tilted and placed near (999000,
  -999000, 999000), at the finest fit tolerance. Checks: wall-to-wall
  distances 0.75 (to the bore) and 2.75 (to the outside) within resolution
  plus 64 eps * 1e6, in either argument order, under 200,000 work units.
  Why: rounding must scale with the pieces' reach from the axis rather than
  with the coordinates; a regression guard for huge positions.

## Transform and assembly

- **`quarter_turns_are_exact`** (`transform/tests.rs`). Geometry: an
  L-shaped extrusion (4 x 1 bar, 1 x 3 upright, round hole r 0.3,
  half-ellipse cap, extruded 2) turned by 90 degrees about x, y, z and a
  scaled -z axis. Checks: after four quarter turns the solid is
  bit-identical to the original (`assert_eq`), volume stays within 1e-12 and
  plane forms face out at each step; a turn about z maps (x,y,z) to (-y,x,z)
  exactly; `sin_cos_degrees` gives exact values for 0, +-90, +-180, +-270,
  450, 720 degrees. Why: rotation by multiples of 90 degrees must be exact,
  not trig round-off.

- **`mirrors_reverse_triangles_and_face_out`** (`transform/tests.rs`).
  Geometry: the same L part mirrored in planes x = 0, z = 2, and a slanted
  plane through (3,-1,0). Checks: volume stays positive and equal (so
  winding was reversed and the faces point out), face checks pass; mirrored
  twice gives the same triangles in the same corner order (the whole solid
  for the x mirror); mirror x then mirror y equals a 180-degree turn about z
  and is not marked as mirroring. Why: handedness bookkeeping is a classic
  source of inside-out solids.

- **`copies_are_named_by_instance`** (`transform/tests.rs`). Geometry: the L
  part translated by 20 in x as copy index 3 of feature 40. Checks: every
  face name equals the original's `copy(40, 3)` and its key differs from the
  original's; copying the copy again gives a different name. Why: pattern
  copies get distinct yet reproducible names, so references into one copy
  are unambiguous. A companion test carries aliases through a mirrored copy
  and checks they resolve.

- **`a_hundred_copies_assemble_in_linear_work`** (`transform/tests.rs`).
  Geometry: 100 cylinders (r 0.4, height 3) in a 10 x 10 grid with pitch 1
  (disjoint), as instances of one feature. Checks: assembly succeeds within
  a budget of 100 * patches * (per-patch transform cost + 32 + 2) + 100
  units (so no pair tests or booleans), volume is 100 times the pin's to
  1e-12, triangle count is exactly 100 times the pin's; 25 and 100 copy
  timings are printed. Why: patterns of disjoint parts must scale linearly,
  enforced by the work budget.

- **`a_part_in_a_void_closed_by_other_parts_is_put_beside_them`**
  (`transform/tests.rs`). Geometry: six plates flush to make a hollow box of
  outer 4 around a void of 2, with a unit cube floating inside the void
  (touching nothing, hull clear of the walls). Checks: volume = 64 - 8 + 1 =
  57; the cube's vertices are appended unchanged; a unit cube half sunk into
  one wall plate, assembled with that plate alone, is unioned with it
  (volume 8 + 0.5). Why: the tricky assembly case where overlap tests by
  hulls pass but the cube is nested in a void; the result must stay a valid
  solid with an inner shell facing out.

- **`side_names_survive_a_turn`** (`transform/tests.rs`). Geometry: the L
  part turned 45 degrees about x. Checks: the key for the elliptic cap's
  side (feature 9, side curve 20) still resolves to a face in the turned
  solid's topology. Why: names are untouched by rigid motions of a
  non-axis-aligned angle.

- **`bits_tell_motions_apart`** (`transform/tests.rs`). Geometry:
  translations, a 90-degree turn, a mirror, composed motions. Checks: equal
  motions have equal bit patterns however built (identity then shift equals
  the shift); a change of 1 eps in a number changes the bits; a turn differs
  from the identity and from a mirror; the mirror flag slot is 1 for a
  mirror, 0 for a turn, 1 for turn then mirror. Why: motions are hashed into
  caches and cache keys, and equality has to be bitwise and mirror-aware.

- **`powers_of_two_are_exact_to_the_bit`** (`transform/tests/scale.rs`).
  Geometry: the L part scaled by (2, 0.5, 4) about the origin, then by the
  inverse. Checks: every vertex equals the vertex times the factors, edge
  control points scale and weights stay, plane normals scale by the inverse
  with unchanged offset, quadric matrices transform exactly; scaling back
  reproduces vertices, edges, triangles and surface descriptions bit for
  bit; a uniform doubling gives cylinder radius exactly 6. Why: scaling by
  powers of two must be lossless, and a round trip an identity.

- **`a_sphere_scaled_per_axis_is_an_exact_ellipsoid`**
  (`transform/tests/scale.rs`). Geometry: ball of radius 2 at (3,-1,2)
  scaled by (2, 3, 0.5) about the origin and by (1.3, 0.7, 2.9) about
  (1,1,1). Checks: every face becomes a quadric; sampled points on the
  strips lie on the ellipsoid with semi-axes 2*factors to 1e-12 (both
  implicit value and distance); the polar caps are within the fit tolerance
  times their slack; more than half of the triangles are exact quadric
  strips. Why: non-uniform scaling must turn spheres into exact ellipsoids
  rather than approximate patches.

- **`a_torus_scaled_up_carries_its_slack`** (`transform/tests/scale.rs`).
  Geometry: torus R = 10, r = 2 scaled uniformly by 25.4 (then by 2 and by
  0.5), and by (1,1,3). Checks: volume within area * fit/2 * 25.4^3 of 2
  pi^2 R r^2 * 25.4^3; major 254 and minor 50.8 to 1e-15; each face's fit
  slack becomes 25.4, multiplies on a second scale up (50.8), stays on a
  scale down; without the slack the consistency check fails; the per-axis
  scale becomes an unknown form with slack 3. Why: error bounds of fitted
  surfaces grow with scale and must be tracked, otherwise the validity check
  fails after a unit change such as inch to mm.

## Manifold export

- **`a_cylinder_shares_its_rims`** (`manifold/tests.rs`). Geometry: cylinder
  r 5, height 2 at (1,2,3), welded at the default display tolerance. Checks:
  origin is the box middle (1,2,4); the rim point (5,0,-1) relative to the
  origin occurs exactly once among the positions, so wall and cap share it;
  volume is a little under the exact one (chords cut inside). A companion
  test, `a_box_welds_to_its_corners`, welds a box to exactly 8 vertices and
  12 triangles with volume 24. Why: watertight output requires vertices
  shared between the wall and cap tessellations.

- **`curved_solids_weld_into_manifolds`** (`manifold/tests.rs`). Geometry:
  round octahedron (also offset by (3e5,-2e5,1e5)), half cylinders of r 4
  and 0.01, a torus with 24 x 12 samples, and a thin two-shell solid with a
  void whose inner shell faces in; each at default and 1e-5 display
  tolerances. Checks: welding gives a valid manifold mesh with the same
  triangle count as the drawn tessellation, each drawn position within f32
  rounding of a welded one, origin the rounded box middle, volume within
  chord * area of the exact one, identical at 1 and 8 threads, and the drawn
  mesh encloses the same volume. Why: broad coverage of curved shapes with
  cross-checks against the drawing path and determinism.

- **`mutated_meshes_are_refused_unless_still_manifolds`**
  (`manifold/tests.rs`). Geometry: welded meshes of a box, a cylinder and a
  torus mutated 1-3 times each by one of 17 random edits (flip or retarget
  triangles, delete or duplicate, add or merge or split vertices, NaN and
  1e300 coordinates, shifted origin, a second shell or a pinched vertex,
  vertex moved onto a triangle's line, truncation, renumbering); 1500 to
  6000 seeded cases. Checks: the constructor's verdict matches an
  independent checker (edge maps, vertex-link cycles, coincidence and
  collinearity in integers, the sign of the volume in f64) in every case
  where that checker can tell; coverage thresholds: over 1/40 of the cases
  accepted, over half refused, under a third undecided. Why: a differential
  test of the manifold gate against an independent oracle, with benign
  mutations (renumbering, extra shell) required to stay accepted.

- **`drawing_picking_and_welding_share_their_triangles`**
  (`manifold/tests.rs`). Geometry: box, cylinder, round octahedron, half
  cylinder, torus, a plate with three holes, the plate joined with a boss,
  and two solids moved to about (4096, -1516, 2130) (an octahedron and a
  revolved torus), at default and 1e-4 tolerances. Checks: the drawn mesh is
  identical with and without picking data; the welded triangle count equals
  the drawn count; walking the welded triangles in the drawn (face) order,
  each corner matches the drawn corner within 2 f32-eps of the size. Why:
  the same tessellation plan must serve drawing, picking and export,
  including the strip diagonals chosen from f64 points far from the origin.

- **`far_solids_weld_about_their_own_middle`** (`manifold/tests.rs`).
  Geometry: a 40 x 20 x 3 plate with a hole r 4 placed at (9e5,-7e5,5e5) and
  at (1e5,0,-1e5). Checks: origin equals the rounded box middle, every
  stored position is within 21 of it, volume within 1% of 2400 - 48 pi. Why:
  f32 positions far from the origin would merge the hole's samples; the
  origin-relative encoding keeps a precise mesh.

- **`samples_that_round_together_are_refused`** (`manifold/tests.rs`).
  Geometry: a unit block joined with a thin pin (r 0.01, length 1) 1e5 away,
  so the middle is 50 km out. Checks: the export fails with a
  coincident-vertex or degenerate-triangle error rather than producing a bad
  mesh. Why: the export must refuse instead of silently emitting a
  non-manifold mesh when single precision cannot tell samples apart.
