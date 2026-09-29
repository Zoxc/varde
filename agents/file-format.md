# File format

A `.vrdp` project file is an append-only list of snappy-compressed document
snapshots, one per save; the last valid one is current. Each record is
checksummed and closed by a fixed random footer. Appends are
crash-safe (a torn final record is ignored and later truncated), access is
guarded by file locks, and a save fails with a conflict if the file changed
since it was opened.

A document (`crates/document/src/lib.rs`) holds its bodies (a name, a
`Shape`, a position, whether it's visible), its features (a name, whether
it's visible, and for now always a sketch: an origin plane, XY, XZ or YZ,
and the `varde_sketch::Sketch`, whose points, lines, circles, arcs
(a fillet an arc and a chamfer a line with a `Corner`: the two lines and
the point they meet at), splines (through fit points or by control
points, open or closed, their points and handle tips by id, by control
points their knots), constraints (a tangent with a spline, and a smooth
join, naming the spline's end they're at) and dimensions carry ids from the
sketch's own `next_id`,
below the three kept for the origin and axes, which aren't stored but
which constraints and dimensions may name, a dimension its expression as
typed and its value), the design's units (a
`varde_expr::LengthUnit`, millimetres by default) and the next id bodies
and features take. A document read from a file is checked
(`Document::check`, which runs `Sketch::check` on each sketch): body and
feature ids increasing and below the next id, names, coordinates, radii
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
