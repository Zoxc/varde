# Profiles and frames (`src/profile.rs`, `src/extrude.rs`)

A `Profile` is loops of `Segment { conic: Conic2, curve: u64 }`: outer
loops counter-clockwise, holes clockwise, so the region is on the left of
every segment; the curve id names the wall. The kernel knows no sketch:
whoever builds a profile (regen) turns lines into `Conic2::line`, arcs
into exact conics of at most 90° and splines into fitted chains, and
merges regions (`varde_sketch::Profiles::merge`, exact, by the pieces'
shared vertices; see `agents/sketch/profiles.md`). A sketch piece's ends
are put at its vertices (`Profiles::vertices`), so segments close to the
bit.

`Profile::check` holds what needs no tolerance: at least one loop, at
most `MAX_PROFILE_SEGMENTS` (`1 << 16`) segments, loops of two or more
segments, each segment within the patch bounds with its ends within
`MAX_COORD` and apart, each segment starting **to the bit** where the one
before ends (closure is never decided by distance), and every loop
enclosing some area. `extrude`, `extrude_tapered` and `revolve` run it
before failing as not implemented, so a broken profile gives its
`ProfileError`. Whether loops touch, cross or nest needs the resolution
and was the old extrude's to find (`Touching`, `Nesting` and the other
variants stay for callers' messages).

`Loop::area` is signed: the polygon of the ends plus each segment's bulge
off its chord, `½∫ (P − p0) × P' dt`, by `varde-kernel`'s 8-point
Gauss–Legendre rule over pieces split off at `½` until their weights are
within `0.9..=1.1` (a weight far from 1 crowds a conic's points, which the
rule can't follow). `Profile::area` sums the loops'.

A `Frame` is where a profile lies in space: its origin and the unit axes
its `x` and `y` run along; `normal()` is `x × y`, the direction an extrude
goes, and `point(p, height)` places a profile point moved along it.
