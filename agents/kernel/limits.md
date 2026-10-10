# Limits, budgets and errors (`src/lib.rs`, `src/budget.rs`, `src/error.rs`, `src/failure.rs`)

| constant | value | why |
|---|---|---|
| `MAX_PATCHES` | `1 << 22` | patches in a mesh; ids and counts fit a `u32` |
| `MAX_REFINE_DEPTH` | 24 | red splits from an input patch: `2^24` times smaller |
| `MAX_TRACE_STEPS` | 4096 | steps tracing one cut of a boolean; past them the cut falls back to a simpler curve |
| `SPLIT_ROUNDS` (boolean) | 6 | rounds of halving curves while cutting faces; what the last keeps must be within the fit tolerance |
| `MAX_NEAR_NODES` (boolean) | 512 | pieces of a patch looked at certifying a crossing only placed, or checking a root of an edge is on it |
| `MAX_TURN_COS` (boolean) | 0.7 | the most a cut's conic turns (about 45°) |
| `MEND_ROUNDS` (boolean) | 4 | rounds of Steiner points in one face's triangulation |
| `SIN_SHAPE` (`mesh/shape.rs`) | sin 5° | the smallest angle under which an extrude cap's triangle is refined (see "Cap quality"), and, in a curved patch's layout, under which a cut face's triangle takes a point at its circumcentre; at most 4 per loop vertex and 16 more, none in a triangle whose circumradius is under `MIN_CURVED_SPLIT` resolutions |
| `MAX_WORK` | `1 << 22` | work units in one operation: about two seconds on one thread at most; the heaviest booleans measured take about half of it |
| `MIN_SPLIT` (repair) | 64 resolutions | the smallest flat piece repair splits, and the smallest profile segment an extrude halves |
| `MIN_CURVED_SPLIT` (repair) | 8 resolutions | the smallest curved piece repair splits; the refiner's floor in repair |
| `FLAT_STOP` (repair) | 1/16 | how flat, in resolutions, failing non-neighbour pieces must both be for repair to stop splitting them |
| `MAX_PROFILE_SEGMENTS` | `1 << 16` | segments in a profile |
| `SIN_MIN` (extrude) | `1e-3` | cusps between segments; the narrowest cap patch corner |
| `MAX_SPLIT_DEPTH` (extrude) | 24 | how often a profile segment may be halved |
| `MAX_ROUNDS`, `MAX_CAP_DEPTH` (caps) | 32, 16 | rounds of mending the caps, and the halvings all told past which the caps halve a segment no more |
| `MAX_MEND_DEPTH`, `MAX_QUALITY_ROUNDS` (caps) | 6, 64 | the halvings all told past which mending on the tries that refine leaves a corner to refinement, and the runs of refinement that ask for anything |
| `CROWDED`, `MIN_CROWDED` (caps) | 32, 65 536 | pairs of the caps' triangles' boxes within the resolution, per triangle and at least, past which the caps are refined for crowding |
| `MAX_EVIDENCE` | 4096 patches, 4096 curves, 256 points, 4096 sketch curves, 256 faces | items of each kind one failure's `Evidence` holds; past them the first ones and `truncated` |
| `EVIDENCE_WORK` | `1 << 16` | work units gathering one failure's evidence may take, apart from the operation's budget (about 30 ms on one thread) |

`Budget` is a limit (`Budget::new(work)`, at most `MAX_WORK`;
`Budget::DEFAULT`); an operation counts it down in a `Work` its steps share
(`repair_within` takes one), and running out is `TooComplex`. A unit is
about a patch or a pair of patches tested or split: repair measured about
0.5 µs a unit on one thread and 0.3 µs on seven. `KernelError` is
`TooComplex` (the budget, or a limit such as `MAX_PATCHES`,
`MAX_REFINE_DEPTH` or a cap of rounds: never detail too small for the
tolerance, which is `Invalid` or `ProfileError::TooFine`),
`Invalid(CheckError)` (the input breaks an invariant the operation
can't restore, or the result would, as for a solid too thin for its
resolution), `Patch(PatchError)` (a
parameter, or a split outside the patch bounds),
`Profile(ProfileError)` (a profile that can't be extruded), and
`Boolean(BooleanError)` (see "Booleans").

**Failures and evidence.** `KernelError` stays the small `Copy` enum
every internal step returns and matches on (extrude's and revolve's
retries, `pinched_named`, the boolean's flat retry). The public
operations that make or combine solids, `extrude`, `revolve`,
`boolean`, `touches`, `assemble` and `Solid::transformed`, return
`Result<_, Failure>`: `Failure { error: KernelError, evidence:
Box<Evidence> }` (boxed so the `Result` stays small: five vectors
would make every `Err` past clippy's `result_large_err`), with
`From<KernelError>` (empty evidence) so `?` converts,
and `Display` the error's. Each is a thin wrapper over a private
function returning `KernelError` (`extruded`, `revolved`, and
`boolean_within`, `touches_within`, `transformed_within`, which take the
`Work`); `assemble`
returns its unions' failures less their operand faces (`of_parts` in
`transform.rs`: those name the union's operands, not any of
`assemble`'s, so they're dropped and `truncated` set). Other public functions that
can fail (`Solid::new`, `cuboid`, `cylinder`, `Mesh::repair`,
`Solid::moments`) keep `KernelError`. `Evidence` is the geometry the
error is about, by value and in the operation's world coordinates
(extrude and revolve place the profile by their frame): `patches`
(`Patch`), `curves` (`Conic<DVec3>`), `points`, `sketch_curves` (the
`Segment::curve` ids of profile segments) and `faces` (`(Operand,
FaceKey)`, `Operand::A` or `B` for a boolean's or `touches`' first or
second operand; the boolean's own `Side` converts into it), plus
`truncated`. Only a step holding both the error and its geometry fills
it.

**Profile evidence** (`profile/evidence.rs`). Every `ProfileError` names
what it is about by indices into the profile as given (revolve's
`remap` maps the ends' pieces back; loops alone keep their index, the
ends' loops being the profile's), so `extrude` and `revolve` gather it
in their public wrappers from the error they return, after every
retry, and the profile: the evidence is the returned error's by
construction and can't change the outcome. A `Gather` holds the
profile, its frame (only if `Frame::check` passes: a profile is checked
before its frame, so without one only the sketch curves are given), the
evidence, the operation's resolution and its `EVIDENCE_WORK` allowance
(a unit a segment placed, `NEAREST_WORK` for a nearest pair, a unit per
4 segments scanned for the axis's extent). Segments are placed on the
frame at height 0 (`Frame::conic`), the sketch's own plane, where its
curves are mended (extrude's `from` and `to` don't move them; a
revolve's frame is the sketch's plane), as `Conic<DVec3>` passing
`Conic3::check` with every control point in range, else left out; each gives its `Segment::curve` once (a
`BTreeSet` of those given). By error:

| error | evidence |
|---|---|
| `Empty` | none |
| `TooManySegments`, `Triangulation` | every segment, loop by loop, up to the caps and the allowance |
| `Short(l)`, `Area(l)`, `Nesting(l)` | loop `l` |
| `Segment`, `Degenerate`, `TooFine` | the segment |
| `Open(l, s)` | the gap's two ends (`s`'s end, the next one's start) and both segments |
| `Cusp(l, s)` | the vertex where `s` starts, and the segments before and at it |
| `Touching([a, b])` | both segments, and where they come nearest: one point half way between where they are within the resolution (touching, as far as the operation can tell), else the point on each |
| `CrossesAxis(l, s)` | the segment, and the axis as a line from the least to the greatest `y` of the profile (each segment's at its ends and where its `y` turns, `measure::turns`: control points can reach past the curve) |
| `TouchesAxis(l, s)` | a vertex on the axis (`s`'s start within the resolution of it) as a cusp is given; else the point of `s` nearest the axis, and `s` |
| `NearlyFullTurn` | the whole profile on the frames at both ends of the turn (`x` turned to `from` and to `to`, as `Turn` turns it) |

Where two segments come nearest is `measure::distance`'s search between
two edges (`curves_distance`, the placed curves as two one-curve
elements, to the resolution), from its own `NEAREST_WORK` (4 096 units;
arcs crossing, touching or nested take about a hundred) taken out of
the allowance: past it, no point and `truncated`. The chain's
`separate`, which raises `Touching`, finds only two pieces whose hulls
aren't apart, not a point, and hands up indices alone, so the search is
run again on the whole segments. (Two in a row always meet at their shared end, so
for them that end is the point; `separate` raises `Touching` on such a
pair only when nothing else of theirs fails first, which in practice is
a sharp corner.) The axis's nearest point
(`nearest_axis`) is exact: the segment's ends and where its `x` turns
(`measure::turns`, the roots of the derivative's numerator), the least
`|x|` of those; `axis_rules` decides by signs and finds no place
either.

**Check evidence** (`failure/check.rs`). An `Invalid(CheckError)` of a
mesh an operation built names its triangles: `Failure::of_mesh` gives
them as patches of that mesh, from a fresh `EVIDENCE_WORK` allowance (a
unit a triangle looked at): one for `Fold`, `Face` and `FacesAgainst`;
two for `Hull`, `EdgeNeighbours`, `VertexNeighbours` and `SameCorners`
(one where both name one triangle); for `InsideOut` the shell whose
lowest triangle it names, walked outward from that triangle breadth
first through the halfedges' pairs (not numbered among all shells as
the check does, so the work stays within the allowance), up to
`MAX_EVIDENCE` and then `truncated`; for the structural errors the
triangle named (`Patch`) or the halfedge's (`Index`, `Pair`, `Loop`,
`DirectedEdge`, `SharedEdge`) if its indices are in range; none for
`Fan`, `EdgeUse`, `Alias`, `Counts` and `TooManyPatches`. Patches are
as the mesh holds them; the receiver draws those it can. Where repair
refused the mesh, the evidence is the pieces it names (`Refusal`,
`Failure::of_patches`; see "Repair"): within the input triangles the
error names, and smaller where repair split first, so a failure in a
corner of a long wall strip shows the corner, not the strip; repair's
errors naming no piece give the input's triangles. Where the check
refused the repaired and merged mesh, its triangles. `Unfinished`
holds the mesh that failed (`Solid::finished_or_unfinished`) and makes
the failure (`Unfinished::failure`); `Solid::new_repaired_within`
(extrude's, from repair on) and `Solid::finished` (revolve's) are that
function with and without the check's `CHECK_WORK`, and return the
`Failure`; the boolean takes the `Unfinished` itself, gathers the
evidence in `checked`, keeps the mesh for `pinched_named`, and keeps the
failure with the error through its retries: unfolding again (the first
try's error and evidence stand), the second try without line joins (the
first's again), and `pinched_named`, whose `NotManifold` keeps the
check's evidence. Extrude and revolve keep each try's `Failure` whole,
so the evidence returned is that of the first try, whose error they
return. `TooComplex` past the check, and `assemble`'s and the
transforms' own checks, carry none.

**Boolean evidence.** Besides the check's, a boolean's own errors carry
where they are. The internal steps on those paths fail with a `Failure`
rather than a `KernelError` (`count::count` and its `crossings`,
`pairs::counted`, `refined_with`, `decide` and `flat`,
`assemble::assemble`, `Along::new`, `Cutting::certify` and
`Cutting::round`, `boolean::assembled`, `flat_soup`, `flat_decided`,
`cleaned` and `build`; `touches_within` and `near::touching` too), the
rest as before, converted by `?`; `chain::chains` hands up the index of
the first arc it refused and the curve refused for it (`chain` returns
that curve rather than `None`), which `assemble` makes the failure of.
The flat retry after `Inconsistent` (`tied_or_exact`, which both
`flat_soup` and `touches_within` go through) returns the second try's
failure whole (none if it succeeds; its own if it fails differently),
and the boolean's tries keep theirs as for the check's. The decisions'
and the assembly's sites gather through `evidence::Gather` (a unit an
item, two a pair of patches given whole or not at all, also where the
cap has room for only one; an operand's
triangle given as its patch and the operand's face it lies on, named
once, or the face by name alone). By error:

| error | evidence |
|---|---|
| `NotManifold` named from a near pair (`pinched_named`) | what repair or the check named, and the pair's two vertices as points, one where they are at one place |
| `NotManifold` named from separate shells | what repair or the check named: the two triangles, or repair's pieces of them |
| `NotManifold` from the decisions (`pinched_line`) | each pair refused, in order: its two patches as refined, and the operands' faces they lie on, each once (`pinch_failure`, stopping at the first left out) |
| `Degenerate` from a face's triangulation | the face's kept halfedges as curves, and the operand's face (`face::boundary_failure`, which charges working the boundary out again a unit a vertex or halfedge it holds: without as much left, the face alone) |
| `Degenerate` from the mesh's builder | the triangle it names (its sides and corners) or the halfedge (its curve and ends), by the soup's positions and curve records (`built_evidence`) |
| `Inconsistent`: an edge through a face whose crossings can't be (`Primitives::crossings`: a straight edge through a flat face more than once, or nowhere inside it) | the edge's curve, and the face crossed (its patch and name) |
| `Inconsistent`: an edge whose winding numbers don't add up (`agree`) | the edge's curve, each of its crossings as a point, and the faces they cross, each once (`edge_failure`) |
| `Inconsistent`: a part's second ray disagreeing with what its edges carried, or a winding number out of `0..=1` | the vertex (the second ray's, or the first out of range) as a point, the other operand's triangles its layer counts put above it, and its own triangles round it (scanned for in order, a unit per 64 triangles, until the allowance or a cap runs out), each with its face (`vertex_failure`) |
| `Inconsistent`: a pair of faces whose ends don't join (`pairs::flat`, or `decide`'s first failing pair: ends on one curved surface, two ends of one sign, ends round the pair that don't pair up) | both patches, their faces, and the ends as points (`pair_failure`) |
| `Inconsistent`: an edge's crossings out of order along it (`Along::new`, where a grazing pair went to one place) | the edge's curve, the two crossings (where they are, not where they went), and the faces they cross |
| `Inconsistent`: a crossing only placed that isn't on the other operand (`Cutting::certify`) | the first such in order: its vertex, its edge's curve, and the face it crosses |
| `Inconsistent`: an arc with no chain near enough the true cut (`chain::chains`) | the first such arc's ends as points, the curve refused between them (the conic along their tangents, else the chord: what `chain` refused last), and the pair's patches and faces |
| `Inconsistent` from a face's boundary that doesn't close into loops (`cut_face`) | as for a `Degenerate` face: its kept halfedges as curves and its face, and the vertices with other than one halfedge leaving and one arriving as points (`face::boundary_failure`) |

Each from a fresh `EVIDENCE_WORK` allowance where it can grow (a step
adding many items, such as an edge's crossings, the faces above a
vertex, a pair's ends or a face's boundary, stops at the first left
out, so what it gives is the first in its order); none
changes what the operation does or spends: `pinched` hands back the
pair it stopped at (the same work), the decisions' and the round's
failures are the first in order as before, the face's halfedges are
worked out again only after the round has failed (and from the
allowance), and the counting's
and assembly's sites only look at what they had already (`certify`
finds the crossing's side and edge from its vertex id, `crossing_of`,
rather than keeping a list beside the one it asks about). Old against
new on the release kernel suites, ignored tests too, every operation's
outcome and work were the same.

Its rules: evidence never changes an outcome (no `Ok` becomes an error
or the reverse, and the error is the one returned without it; tests
compare errors with the evidence stripped, through the test-only
`Stripped` trait); it is bounded, the adders (`Evidence::add_patches`,
`add_curves`, `add_points`, `add_sketch_curves`, `add_faces`) keeping the first items in
order up to `MAX_EVIDENCE` and setting `truncated` past it (the fields
are open, so a receiver checks `within_caps`); it is gathered from a
fresh `EVIDENCE_WORK` allowance (`failure::evidence_work`, spent through
`Evidence::afford`, which sets `truncated` once it runs out), never the
operation's budget, so finding it can't make an error `TooComplex`;
it is deterministic; and it goes with the error returned, so where an
operation retries and returns an earlier try's error, that try's
evidence is kept with it (`(KernelError, Evidence)` together, not a
side slot on `Work`). `TooComplex` carries none. Regen makes the
evidence drawable once and keeps that in its cache
(`ErrorGeometry`, with `Display::flatten` and `Display::sample_patch`;
see `agents/features/failures.md`, "Failures and where they are").
