# File format

A `.vrdp` project file is an append-only list of snappy-compressed document
snapshots, one per save; the last valid one is current. Each record is
checksummed and closed by a fixed random footer. Appends are
crash-safe (a torn final record is ignored and later truncated), access is
guarded by file locks, and a save fails with a conflict if the file changed
since it was opened.

A document (`crates/document/src/lib.rs`) holds its bodies (a name,
whether it's visible, its opacity, a `u8` percent, and the extrude or
revolve that makes it: no geometry, which regenerating the feature
history gives), its features (a name, whether it's visible, and a kind,
stored by its place in `FeatureKind`, new kinds appended: a sketch, an
extrude,
`crates/document/src/extrude.rs`: the sketch feature it uses, its
regions as `varde_sketch::RegionRef`s (curve ids of the outer loop and of
each hole, and a point inside), the extent (one side, symmetric, two
sides, each distance a typed expression and its value, or through all),
flip, and the operation: a new body, by id, or join, cut or intersect
with the bodies taken out of its targets; or a revolve,
`crates/document/src/revolve.rs`: the sketch, regions, flip and
operation as an extrude's, the axis (a line of the sketch by curve id,
or the sketch's x or y axis) and the turn (full, one side, symmetric or
two sides, each angle a typed expression and its value in radians). A
sketch is a plane, `crates/document/src/plane.rs` (an origin plane,
XY, XZ or YZ, or a face of a body: the body's id, the face's key, the
kernel's `FaceKey` and `PartKey` with serde, whose fields and order are
then part of the format, and a picked point; never a placement, which
regenerating finds), and the `varde_sketch::Sketch`, whose points, lines, circles, arcs
(a fillet an arc and a chamfer a line with a `Corner`: the two lines and
the point they meet at), splines (through fit points or by control
points, open or closed, their points and handle tips by id, by control
points their knots), constraints (a tangent with a spline, and a smooth
join, naming the spline's end they're at) and dimensions carry ids from the
sketch's own `next_id`,
below the three kept for the origin and axes, which aren't stored but
which constraints and dimensions may name, a dimension its expression as
typed and its value), the design's units (a
`varde_expr::LengthUnit`, millimetres by default), its fit tolerance (an
`f64` in millimetres, `1e-5 ..= 1e-1`, 1 µm by default) and the next id
bodies and features take. A document read from a file is checked
(`Document::check`, which runs `Sketch::check` on each sketch): body and
feature ids increasing and below the next id, every body's opacity from
10 to 100, every body made by an
extrude or revolve the document holds whose operation makes it as its
new body, and every such body there, an extrude's or revolve's sketch a
sketch feature before it, 1 to 256 regions, each within the coordinate
limit with its id lists sorted, distances their expressions give in the
document's units from 1 µm to the coordinate limit (two sides together
too), through all only for a cut, revolve angles their expressions give
above zero and at most a turn (two sides together too; a revolve's axis
line isn't required to exist, as a region isn't: regeneration reports
it), excluded bodies sorted and made by earlier features, a sketch's face
point finite and within the coordinate limit, its body (if there) made
and its key's feature (if there) placed before the sketch,
the tolerance within its range, names, coordinates, radii
and labels within bounds, a sketch's item counts bounded, every reference
naming an item of the right kind, every fillet and chamfer on a corner
of two lines ending at its point, every spline's point count (at most
100), handles and knots as its kind takes them, and every dimension's
expression giving its stored value exactly when evaluated in the
document's units, a value
its measure can be (a length at least a micrometre, an angle under a
turn). Ids
running out refuses the edit instead of overflowing. The length a record
claims to decompress to is bounded by 32 times its compressed length
(snappy expands at most about 21 times) and 1 GiB, since the decoder
allocates it up front. See `crates/io/src/vrdp.rs`, and for the locking
and replacing of files natively
`crates/io/src/native/files/document_file.rs`. The version field stays at
1 while the app is WIP. Auto-save sidecars and store entries use the same
records, each holding an auto-saved document along with the saved version
of the design it was based on.
