# Plane sections (`src/section.rs`)

For sketches' links (`agents/features/sketch-links.md`, Sketch links): where a
plane (a point and a unit normal, `section::Plane`) meets a chain of
conics. `crossings` solves each conic of a chain for where its signed
distance's numerator, `d0 (1-t)² + 2 w dc t(1-t) + d1 t²`, has roots in
`[0, 1]` (a conic whose three control points are within the given
distance of the plane lies in it: `SectionError::InPlane`), dropping a
crossing within that distance of the one before or the first (where one
conic ends and the next starts, or a closed chain comes round). `sample`
gives places along a chain, `per` evenly by parameter on each conic, and
the chain's end unless it is closed. Past `MAX_SECTION_PLACES` places,
`TooComplex`.

`face_section(solid, tris, plane, weld)`, where a plane cuts a face of a
solid, finds no curves: the facade's solids are empty.
