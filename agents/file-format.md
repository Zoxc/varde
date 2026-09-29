# File format

A `.vrdp` project file is an append-only list of snappy-compressed document
snapshots, one per save; the last valid one is current. Each record is
checksummed and closed by a fixed random footer. Appends are
crash-safe (a torn final record is ignored and later truncated), access is
guarded by file locks, and a save fails with a conflict if the file changed
since it was opened. A document read from a file is checked (body ids unique
and below the next id), and ids running out refuses the edit instead of
overflowing. The length a record claims to decompress to is bounded by 32
times its compressed length (snappy expands at most about 21 times) and
1 GiB, since the decoder allocates it up front. See
`crates/io/src/vrdp.rs`, and for the locking and replacing of files
natively `crates/io/src/native/files/document_file.rs`. The version field stays at 1 while
the app is WIP. Auto-save sidecars and store entries use the same records,
each holding an auto-saved document along with the saved version of the
design it was based on.
