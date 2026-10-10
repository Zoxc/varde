# Plane sections (`src/section.rs`)

For sketches' links (`agents/features.md`, Sketch links): where a plane
(a point and a unit normal, `section::Plane`) meets a solid's curves and
faces. `crossings` solves each conic of a chain for where its signed
distance's numerator, `d0 (1-t)² + 2 w dc t(1-t) + d1 t²`, has roots in
`[0, 1]` (a conic whose three control points are within the given
distance of the plane lies in it: `SectionError::InPlane`), dropping a
crossing within that distance of the one before or the first (where one
conic ends and the next starts, or a closed chain comes round). `sample` gives places along a chain, evenly by parameter on
each conic. `face_section` cuts a face (a region's triangles): each
patch's signed distance's numerator is the quadratic form `Σ a_ij u_i
u_j` in its barycentric coordinates (corners on the diagonal, each
edge's weight times its control point's distance off it), whose zero set
is found where it crosses the patch's edges (their roots, the same as
the edge conic's, so two patches agree but for rounding) and traced
between paired crossings (in order round the boundary, the pairing
whose chords' midpoints are nearer the zero set) by Newton's steps along
the domain gradient from the chord, nine places a piece, each evaluated
on the patch; a patch on one side (its control points strictly so) is
skipped, one touching (an odd count) gives nothing, one within the
distance is in the plane. A piece along an edge lying in the plane,
which the patches on both sides trace with the same places, is kept
once (else it would weld to itself, there and back). Pieces are welded
end to end through a grid of their ends, closed where they come back to
the start. A face every
patch it meets lies in the plane is `InPlane`; past
`MAX_SECTION_PLACES` places, `TooComplex`. Exact surfaces give places
on them to rounding: a cylinder cut square is a circle, through its
axis two lines, aslant an ellipse.
