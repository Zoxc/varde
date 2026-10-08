# File format

A `.vrdp` project file is an append-only chain of checksummed blocks
after a header; each save appends a record block holding a
snappy-compressed MessagePack document snapshot, and the newest record
reached by following the chain is current. Auto-save sidecars and store
entries are a second file type, each record an auto-save. Reading steps
over damage where it can and says how it found the file; saving refuses
to go over someone else's change or over damage it didn't expect.
`crates/io/src/vrdp.rs` and its modules `chain` (reading), `check` (the
check before a save) and `preview` hold the code. How the lane and the
app use it is in `files.md` and `web-files.md`.

## Container

```text
file   = header | block*
header = MAGIC: [u8; 25] | version: u32 LE | id: u128 LE
block  = len: u64 LE | prev: u128 LE | sum: u128 LE
         | tag_len: u16 LE | tag: [u8; tag_len]
         | payload: [u8; len - 2 - tag_len]
         | len: u64 LE | kind: [u8; 16]
```

All integers are little-endian; a block is `64 + len` bytes.

- `MAGIC` is `varde-cad` followed by random bytes, one for each file
  type. `version` stays 1 while the app is WIP.
- `id` is random (`getrandom`), made anew by every write of a whole file
  (a new file, a replace, the web's whole-file write, a held file written
  from its start) and kept by appends.
- `len` is the length of `tag_len | tag | payload`, written before and
  after them.
- `prev` is the `sum` of the block before; the first block's is the
  XXH3-128 of the header.
- `sum` is the XXH3-128 of `id` followed by every byte of the block but
  the `sum` field.
- `tag` says what the payload is, for its kind; `kind` is a random 16
  byte value naming the block's kind.

| File type | Holds |
| --- | --- |
| design file | the header, `RECORD` blocks, then the newest save's `PREVIEW` blocks |
| held file (an auto-save sidecar or a store entry, `HeldFile`) | the header, then `AUTOSAVE` blocks; never previews |

| Kind | `tag` | `payload` |
| --- | --- | --- |
| `RECORD` | the writer, `APP_NAME` and the crate version (at most 1024 bytes written, any read) | a saved `Document`, see "Records" |
| `PREVIEW` | a media type, at most `MAX_MEDIA_TYPE` (127) bytes | the data as it is, at most `MAX_PREVIEW` (4 MiB) |
| `AUTOSAVE` | the writer | an auto-save (`AutoSaved`): the document, the `Tail` of the design's save it was based on, on the web the design's file name, and whether it's the design as downloaded (only older web builds wrote that; still read) |

Every `MAGIC` and kind is a different random value: a file of one type
is refused as the other by its header (`Error::IsAutoSave`,
`Error::IsDesign`), and a block of one type's kind is a kind the other
doesn't know. Blocks of a kind a reader doesn't know are stepped over, so
a new kind, a new random value, can go between the last record and the
previews; like previews it then belongs to one save, as each save cuts
the file back to its last record. Below, a held file's `AUTOSAVE`
blocks are its records.

**Bounds.** Every length read from a file is checked before anything is
allocated, with checked arithmetic (a `usize` is 32 bits on wasm). A
block's `len` is at least `2 + tag_len` and at most `MAX_BLOCK`, whatever
its kind: what snappy may compress `MAX_DECOMPRESSED` (1 GiB, the most a
record may decompress to) to, plus `tag_len` and the longest tag. Writers
check the same bounds (`Error::TooLarge`), so whatever is written can be
read.

**Intact.** A block is intact if its two lengths agree, within bounds,
and its `sum` checks with the file's `id`, whatever its kind; it follows
on if its `prev` is the `sum` of the block before. `sum` covering `id`
means a block of another file, or of an earlier whole-file write at the
same place, never checks in this one, so stale bytes a file system
leaves in space a file grows into can't pass as its own, even holding
the same document. `prev` keeps out intact blocks of this file's own
past, such as one cut off by a save that a later, shorter write didn't
cover. A writer's saved version is a `Tail { last, sum, end }`: the
offset, `sum` and end of the last record it read or wrote, and through
`prev` what that record was appended to. Auto-saves store their design's
as their `base`.

## Reading

Reading (`chain::Chain::scan`) reads the whole file and follows the
chain from the first block: each block intact and following on. Records
are collected, other kinds stepped over, and the newest record reached
is opened. An intact block that doesn't follow on is of the file's past
and ends the chain as a torn tail. A block that isn't intact:

1. **Stepped over by its header**, if its first `len` is within bounds,
   its `prev` is the `sum` of the block before, and the block its `len`
   leads to is intact with the damaged block's stored `sum` as its
   `prev`. A record stepped over so with no record after it is the
   newest save, damaged. Other blocks stepped over with no record after
   them, like a damaged preview, are part of a torn tail, unless damage
   framed like a record follows.
2. **Otherwise the chain ends there.** If nothing from there on is framed
   like a record (a record's kind, with a trailing `len` leading back to
   a block's matching first `len`), it's a torn tail: ignored, and cut
   off by the next save. If the block there is framed like a record with
   an intact header and nothing after it is, it's the newest save,
   damaged.
3. **Else the rest of the file is searched** (`chain::search`) for
   intact blocks. Candidates are places whose two lengths agree within
   bounds; only those are hashed, in order, skipping ones that overlap a
   block found, and hashing stops past 4 times the file's length, the
   rest counting as damage, so a crafted file can't keep the lane busy.
   Each block found is linked to whatever ends just before it, through
   its trailing `len`, walking back over damaged blocks whose stored
   `sum` the link holds. It's of the file's past, and rejected with all
   that follows on from it, if it follows on from the header or from a
   chained block other than the last, or from a block rejected so; if it
   doesn't follow on from the intact block found just before it; or if
   the link reaches a block that starts before the chain's end, which
   was overwritten. The newest record kept is the search's newest.

How the file ended is reported (`Report`: the `Outcome`, when the record
opened was written, and how many bytes can't be read, a torn tail not
counted):

| `Outcome` | Meaning |
| --- | --- |
| `Intact` | the chain reaches the end of the file |
| `TornTail` | the chain ends in a torn tail (an interrupted save, zeros, a block of the file's past, a damaged preview) |
| `Bridged` | damaged blocks were stepped over by their headers to a record after them |
| `NewestDamaged` | the newest save is damaged with an intact header (2, or stepped over in 1): the record before it is opened |
| `Damaged { found }` | the rest of the file was searched (3); `found` is the search's newest record if it isn't the one opened, with its time if it decodes as far as that |

The record opened is the newest the chain proves, or the search's newest
when the chain proves none. Damage after an intact record never keeps a
file from opening; no intact record is `Error::Empty`, or
`Error::Corrupt` if there's damage, and a newest record that won't
decode is `Error::Decode`, naming its writer by its tag. A held file
shorter than its header holds nothing yet, as a crash while writing its
first record leaves it. `from_bytes_with_found` also opens the search's
`found` record, if it decodes, for the user to choose instead
(`Request::OpenFound`), as `Damaged` with nothing found. `based_past`
tells whether an auto-save's `base` is a save after the one opened,
which couldn't be read: the block at it has a header, intact or not,
holding its `sum`.

**Previews** are read from the end, reading no record's payload
(`read_previews`): the header must be a design file's; from the end,
blocks are walked back over by their trailing `len` while their two
lengths agree within bounds, at most `MAX_PREVIEWS` (8) of them of any
kind, to a block whose kind is `RECORD`, of which only the header is
read, for its `sum` (so a preview may show for a file whose newest record
turns out damaged); then going forward each block must be intact and
follow on, and the previews among them are taken until one isn't. A
preview whose media type isn't UTF-8 or that's out of bounds is stepped
over. Natively `DocumentFile::read_previews` takes the shared lock
without waiting, no lock being no preview. The app's saves write two,
the design's thumbnail in each theme's colours, `image/png;
theme=light` then `image/png; theme=dark` (`crates/io/src/thumbnail.rs`,
`Preview::is` ignores the parameter, the thumbnail module reads it),
when it was rendered, and the welcome screen reads them back (see
"Thumbnails" in `files.md`); a download on the web writes none.

## Saving

**A new file** (`write_new`) is the header with a new `id`, its record and
its previews, written in one go; natively it's synced with its directory
entry, and **a replace** writes one next to the old file and renames it
over it (`crates/io/src/native/files/document_file.rs`, which also locks
a design's file around each read and save).

**A save** (`save`) appends under the file's exclusive lock, to a file
whose `Known` (its `id`, `Tail`, where the block before the tail's
record starts, what reading found after the tail, and a failed save's
`Attempt`) a `DocumentFile` keeps:

1. `check_unchanged` (below) checks the file is as last read or written.
2. The file is cut to `tail.end`, dropping the previews and a torn tail;
   after the newest save damaged with an intact header, as reading found
   it, to that block's end instead, so nothing is cut off.
3. The record, its `prev` `tail.sum` or the damaged block's stored `sum`,
   and the first `MAX_PREVIEWS` previews go in one write, and are synced.
4. Should cutting, writing or syncing fail, the file is cut back. Once
   writing has started, the record's stored `sum` and `prev` are kept as
   the `Attempt` until a save succeeds, in case cutting back failed too
   and it landed, wholly or partly. A failed sync isn't retried.

The file keeps its `id`; appends never rewrite the header or write before
`tail.end`.

**Unchanged** (`check::check_unchanged`, also run by the web worker on
the file read whole before replacing it) reads only what it must. A file
read as `Damaged` is refused first, `Error::OpenedDamaged`: cutting it
short could lose what can still be got out of it, so it's only saved as
another file. Then:

| Found | Result |
| --- | --- |
| no design header, or another `id` | `Conflict` |
| another intact block at `tail.last` (another `sum` or end) | `Conflict` |
| the record at `tail.last` torn or damaged, or `tail.last` out of the file | `Damaged` |
| the record's link: the block ending before it, intact, with another `sum` | `Conflict` |
| the record's link otherwise wrong (or the header's hash, for the first record) | `Damaged` |

The block before is found by its trailing `len`; should that be damaged,
by its header where reading or writing found it (`Known` keeps where it
starts), as reading steps over a block by its header, so a file read as
`Bridged` past a record damaged at its end is saved to as usual.

Then the chain is followed from `tail.end` as reading does:

| After `tail.end` | Result |
| --- | --- |
| nothing, blocks of other kinds (damaged ones stepped over by their headers), then a torn tail or the end | unchanged |
| a block whose header holds the `Attempt`'s `sum` and `prev`, intact or not, once | ours: stepped over by its header, and replaced |
| the damaged newest save reading found | the save goes after it |
| any other record the chain reaches | `Conflict` |
| damage with something framed like a record from it on, which reading would have found | `Damaged` |

`Error::Damaged` and `Error::OpenedDamaged` are errors of their own, not
conflicts; the app's banner offers Save As for all three.

**The web** (`to_bytes`, `whole_file`) can only replace a file of the
user's whole, so after the check it writes a new file of one record,
with a new `id`: no history, and no failed attempt to allow for, as the
browser replaces the file as a write closes. A file read as
`NewestDamaged` is replaced the same way.

**Held files** (`HeldFile`) are locked by their owner for as long as it
holds them, so they take no lock per operation and check for no one
else's changes. An append goes after the record last read or written,
or after the newest record if it's damaged with an intact header,
cutting off a torn tail and other damage. One whose records are framed
but none intact (`Error::Corrupt`) isn't written to until its owner
empties it, nor is one that couldn't be read (`Error::Io`), until a read
succeeds; anything else that can't be read (not a held file, a newest
record that won't decode) is started over with a new `id`.

**Durability.** Appends never write before `tail.end`, so saved records
are only at risk from the device. A crash leaves a prefix of a save's
writes: the file reads as before the save or after it, with at most a
torn tail. After a power loss, the writes since the last sync may land
in any subset and order, as zeros, or with stale bytes where the file
grew; none of that makes a file unreadable, passes a block of another
file or an earlier whole-file write as this one's, or brings back a
block of the file's past as its newest save: at most the newest,
unsynced save is lost. A torn sector damaging the end of the record
before an append is stepped over by its header.

## Records

Each record's payload, a map `{ time, payload }` of when it was written
(`UnixSeconds`, only for showing) and what it holds, is MessagePack
compressed with raw snappy, with fields and variants by name, so it can be
extended with new features: a new field `#[serde(default)]` reads from
older files as its default and older builds skip it, a new variant can go
anywhere, and names are never renamed or reused. Prefer keeping older
files working that way; while the app is WIP a change that can't be made
so just breaks them. Once the format must be stable, the preferred design
is `version` bumps with migrations, records listing the features they
need so an older build refuses a newer file, and unknown fields kept
through a save; `crates/io/src/vrdp.rs`'s module docs hold the detail.
The length a record claims to decompress to is bounded by 32 times its
compressed length (snappy expands at most about 21 times) and
`MAX_DECOMPRESSED`, since the decoder allocates it up front. Arrays and
maps in the MessagePack may nest 31 deep (a record needs 12), so skipping
an unknown field can't overflow the stack. The workers get documents as
postcard instead (`varde_document::codec`).

## Documents

A document (`crates/document/src/lib.rs`) holds its bodies (a name,
whether it's visible, its opacity, a `u8` percent, its colour if it
has one of its own (`color`, `#[serde(default)]`, so older files read as
none: a `Tint`, a hue in degrees and a saturation percent), and the
extrude,
revolve or pattern that makes it: no geometry, which regenerating the feature
history gives), its features in the Timeline's order, which ids don't
follow (a feature added while rolled back goes in at the marker, before
features with lower ids; older files have them in id order) (a name,
whether it's visible, and a kind,
stored by its variant name in `FeatureKind`: a sketch, an
extrude,
`crates/document/src/extrude.rs`: the sketch feature it uses, its
regions as `varde_sketch::RegionRef`s (curve ids of the outer loop and of
each hole, and a point inside), the extent (one side, symmetric, two
sides, each distance a typed expression and its value, or through all),
flip, the taper (a typed expression and its value in radians, or none,
written as nil; `#[serde(default)]`, so an extrude written before tapers
reads untapered, and an older build skips it, reading a tapered extrude
untapered; one of zero reads as none), and the operation: a
new body, by id, or join, cut or intersect with the bodies taken out of
its targets and `held`, the id of the body the feature made when it
was a new body, held for it so that a new body again gets it back
(`#[serde(default)]`: none, as every record written before it reads;
an older build skips it); or a revolve,
`crates/document/src/revolve.rs`: the sketch, regions, flip and
operation as an extrude's, the axis (a line of the sketch by curve id,
the sketch's x or y axis, or a straight edge of a body,
`crates/document/src/edge.rs`: the body's id, the keys of the faces
either side, sorted, and a picked point) and the turn (full, one side,
symmetric or two sides, each angle a typed expression and its value in
radians); or a combine, `crates/document/src/combine.rs`: the target
body's id, the tool bodies' ids (sorted), the operation (union, subtract
or intersect) and whether the tools are kept; or a move or a mirror,
`crates/document/src/motion.rs`: the bodies' ids (sorted); a move's
three offsets along X, Y and Z (typed expressions and their values in
millimetres) and its turn, if any (an axis: a world axis X, Y or Z, an
edge as a revolve's, or a face as a sketch's, and an angle as a
revolve's); a mirror's plane (an origin plane, or a face as a sketch's)
and whether the original is kept; or a pattern,
`crates/document/src/pattern.rs`: the bodies' ids (sorted) and its
kind, linear (an axis as a move's, the count and the spacing, typed
expressions and their values, the spacing in millimetres) or circular
(an axis, the count and the angle in radians the copies spread over),
and `copies`, "Join to original": `Joined` (the copies in their
bodies) or `Separate` with the ids of the bodies the copies are, copy
by copy then body by body. `copies` is `#[serde(default)]`: a record
written before it (none had it) reads as `Joined`, and an older build
skips it (reading a separate pattern's copy bodies as bodies made by a
feature that doesn't make them, which its check refuses); or an align,
`crates/document/src/align.rs`: the moved body's id, its two sides
(each a point: the origin, a corner by the keys of three faces, sorted,
and a picked point, the middle of an edge or the centre of an edge as a
revolve's; and an optional primary and secondary direction: an origin
axis, a face's normal as a sketch's face, or an axis as a move's), the
flip, and an optional offset (a typed length and its value in
millimetres) and turn (an angle as a revolve's); or a scale,
`crates/document/src/scale.rs`: the bodies' ids (sorted), the point it
scales about (a point as an align's) and its factor: uniform (a typed
number and its value), per axis (three, X, Y and Z) or an edge length
(an edge as a revolve's, a typed length and its value in millimetres,
and whether it scales along the edge's axis only); or a split,
`crates/document/src/split.rs`: the body's id, its tool (an origin
plane or a face's plane as a mirror's; a face as a sketch's; another
body's id; a sketch's id and regions as an extrude's; or a sketch's id
and the ids of its curves, sorted), which piece keeps the body's id
(front or back), which pieces are kept (both, front or back) and the
id of the body the other piece becomes: there when both are kept, and
while one side is kept the id it had (held for it, for when both are
kept again) or none for a split that never kept both. `original`,
`keep` and `new_body` are `#[serde(default)]` (front, both, none);
splits are new, so no older record holds one, but one written before
ids were held, keeping a side with no new body, reads as one that never
kept both; an older build reads a held id as a new body that isn't
there and refuses the file. A held id (a split's or a join's, cut's or
intersect's) must be below the next id, no body's and held by one
feature only, or the record is refused; or a
chamfer, `crates/document/src/chamfer.rs`: its edges (each as a
revolve's axis edge: a body's id, two face keys, sorted, and a picked
point), sorted by body, keys and point, its size (equal: one typed
length and its value in millimetres; two: two lengths, the first along
each edge's first face; or a length and an angle, as a revolve's,
`ChamferSize`), whether tangent chains are taken in and `flip` (the
edges' first faces their second keys', `#[serde(default)]`, false).
Chamfers are new, so no older record holds one; or a shell,
`crates/document/src/shell.rs`: the body's id, the faces it opens (each
as a sketch's face: a body's id, a face key and a picked point), sorted
by body, key and point, its thickness (a typed length and its value in
millimetres) and `outward` (the walls outside the faces,
`#[serde(default)]`, false). Shells are new, so no older record holds
one; or a fillet, `crates/document/src/fillet.rs`: its edges as a
chamfer's, its radius (a typed length and its value in millimetres)
and whether tangent chains are taken in. Fillets are new, so no older
record holds one; or an offset face, `crates/document/src/offset_face.rs`:
the faces it moves (each as a sketch's face), sorted by body, key and
point, its distance (a typed length and its value in millimetres, above
zero), `inward` (moved into the body) and `tangent` (tangent faces taken
in). Offset faces are new, so no older record holds one; or a draft
(`FaceDraft`), `crates/document/src/face_draft.rs`: the faces it turns
(each as a sketch's face), sorted by body, key and point, its neutral
plane (a `PlaneRef`, as a mirror's: an origin plane or a face), its
angle (a typed angle and its value in radians), `flip` (the pull
against the plane's normal) and `tangent` (tangent faces taken in).
Drafts are new, so no older record holds one; or a sweep, `crates/document/src/sweep.rs`: its profile's sketch's
id and regions as an extrude's, its path (`PathRef`: a chain of parts,
each a sketch's id and the ids of its curves, sorted (`CurveChain`), or
the edges of one body, each as a revolve's axis edge, sorted by body,
keys and point, and whether tangent chains are taken in; or a helix,
`Helix`: its axis as a move's, its pitch (a typed length and its value
in millimetres), its turns (a typed number and its value), whether
left-handed and whether flipped), its orientation (`Orientation`:
follow the path or keep it), an optional twist (an angle as a
revolve's) and its operation as an extrude's. Sweeps are new, so no
older record holds one; or a loft, `crates/document/src/loft.rs`: its
sections in order (each a region of a sketch, named as an extrude's
regions are, with an optional start point's sketch id, or a sketch
point: the sketch's feature id and the point's id; `Section`'s variants
by name), its mode (`Smooth` or `Ruled`), whether it's closed, its
rails (each a `CurveChain`, as a sweep path's part: a sketch's feature
id and its curves' ids, sorted) and its operation as an extrude's.
Lofts are new, so no older record holds one. A
sketch is a plane, `crates/document/src/plane.rs` (an origin plane,
XY, XZ or YZ, or a face of a body: the body's id, the face's key, the
kernel's `FaceKey` and `PartKey` with serde, whose fields and order are
then part of the format, and a picked point; never a placement, which
regenerating finds), and the `varde_sketch::Sketch`, whose points, lines, circles, arcs
(a fillet an arc and a chamfer a line with a `Corner`: the two lines and
the point they meet at), splines (through fit points or by control
points, open or closed, their points and handles' tips and ends by id
(a handle saved before ends were points reads with `Id::MISSING` and is
given one on reading, `Sketch::add_handle_ends`), by control points
their knots), constraints (a tangent with a spline, and a smooth
join, naming the spline's end they're at) and dimensions carry ids from the
sketch's own `next_id`,
below the three kept for the origin and axes, which aren't stored but
which constraints and dimensions may name, a dimension its expression as
typed and its value, and its links, `links`, defaulted to none: each a
`Link` with an id from the same counter, its kind, `Project` or
`Intersect`, the ids of the points and curves it made, which are in the
lists with the rest, and whether they count for profiles), with
`sources`, defaulted to none too, beside it in the feature: each link's id
and what it comes from (`OutsideRef`: another sketch's feature id and
item id, or an edge, face or corner of a body named as a revolve's axis
edge, a sketch's face or an align's corner are), the design's units (a
`varde_expr::LengthUnit`, millimetres by default), its fit tolerance (an
`f64` in millimetres, `1e-5 ..= 1e-1`, 1 µm by default), the next id
bodies and features take, and `rollback`, the feature the Timeline is
rolled back to before (defaulted to none; checked to name a feature;
`Command::SetRollback`, kept by a new feature, which goes in before it,
moved to the next
feature kept when its own is removed), and `params`, the design's
parameters (defaulted to none, so older files read as having none; each a
`Param`, its `name` and its expression `text` as typed; what they come to
isn't stored but resolved again from the list as the document is read,
`Document::params_resolved`). A document read from a file is checked
(`Document::check`, which runs `Sketch::check` on each sketch): at most
`varde_expr::MAX_PARAMS` (1000) parameters, each named as
`varde_expr::check_name` takes (a word of ASCII letters, digits and `_`
not starting with a digit, at most 64 bytes, not a unit's name), no name
twice, each text at most `MAX_LEN` (256) bytes (`CheckError::Param`,
`CheckError::Params`; a parameter's expression may be in error, only
values using it may not); body ids increasing, feature ids each used
once (`CheckError::FeatureTwice`), and both below the next id, every body's opacity from
10 to 100, every body's colour's hue below 360 and saturation at most
30 %, every body made by an
extrude, revolve or sweep the document holds whose operation makes it as its
new body, or a split naming it as its new body, and every such body there, or by a pattern listing it as a
copy body (one per copy, none repeated, every body it makes listed, at
most 1024, none for a joined one), an extrude's or revolve's sketch a
sketch feature before it, 1 to 256 regions, each within the coordinate
limit with its id lists sorted, distances their expressions give in the
document's units from 1 µm to the coordinate limit (two sides together
too), through all only for a cut, revolve angles their expressions give
above zero and at most a turn (two sides together too; a revolve's axis
line isn't required to exist, as a region isn't: regeneration reports
it), excluded bodies sorted and made by earlier features, a sketch's
links (`Sketch::check`: in id order, each naming points and curves of its
own, its curves construction unless it counts for profiles, no other
curve made from its points) with one source each, in their order, of a
kind the link takes, naming what's made before the sketch or, as a
sketch's face, what isn't there with an id below the next id, a sketch's face
point finite and within the coordinate limit, its body (if there) made
and its key's feature (if there) placed before the sketch, and either
id, if nothing has it, below the next id; the same of a revolve's axis
edge (both keys' features), its keys sorted and different; a combine's,
move's, mirror's or pattern's bodies there, made by earlier features and
(but a combine's target) sorted without repeats, 1 to 256; a move's
offsets their expressions give within the coordinate limit of zero and
its angle within a turn either way; a pattern's count a whole number
from 2 to 1024, its spacing within the coordinate limit of zero and not
zero, its angle above zero and at most a turn; a move's or pattern's
axis edge or face and a mirror's face as a revolve's edge and a
sketch's face; an align's body there and made by an earlier feature,
its sides paired (a primary on both or neither, a secondary likewise
and only with a primary), its flip, offset and turn only with
primaries, its offset and turn as a move's, its corners' keys sorted
and different, its moved side's references on the moved body and none
the origin's, its target side's on other bodies, each named as a move's
axis; a scale's bodies as a move's, its factors their expressions give
from 0.001 to 1000, its edge length a length as an extrude's distance,
its edge on one of its bodies, its point and edge named as a move's
axis; a split's body made by an earlier feature, its tool body there,
made earlier and not the body, its face tool's body there and made
earlier, its plane face's body and any face's maker as a mirror's
plane's, a sketch tool's sketch a sketch before it, 1 to 256 regions
or 1 to 256 curves sorted without repeats, and its new body there
when it keeps both pieces, a body it makes (a body made by a split
must be its new body, and so the one it keeps both pieces for); every
id held for a body a feature made before (a split's new body while it
keeps a side, a join's, cut's or intersect's `held`) below the next id,
no body's and held once;
a chamfer's 1 to 256 edges on one body there and made by an earlier
feature, sorted without repeats, each edge's keys sorted and different,
its point finite and within the coordinate limit, and its keys'
features (if there) before the chamfer, or with ids below the next id;
its distances lengths as an extrude's, its angle above 0 and under
90°;
a shell's body there and made by an earlier feature, its at most 256
open faces on that body, sorted without repeats, each point finite and
within the coordinate limit, and each key's feature (if there) before
the shell, or with an id below the next id; its thickness a length as
an extrude's;
a fillet's edges as a chamfer's, its radius a length as an
extrude's;
an offset face's 1 to 256 faces all on one body made by an earlier
feature, sorted without repeats, each point finite and within the
coordinate limit, and each key's feature (if there) before the offset,
or with an id below the next id; its distance a length as an
extrude's;
a draft's faces as an offset face's, its neutral face (if a face) on a
body made by an earlier feature, its point finite and within the
coordinate limit and its key's feature (if there) before the draft or
with an id below the next id; its angle above zero and under 90°;
a sweep as an extrude (its profile's sketch, regions, operation and
excluded bodies), its chain path of 1 to 64 parts with at most 1024
curves and edges in all, each part naming at least one, a part's
curves sorted without repeats and its sketch a sketch before the sweep
other than its profile's (its curves aren't required to exist, as a
region isn't), a part's edges on one body there and made by an earlier
feature, sorted without repeats, each as a chamfer's edge; or its
helix's axis as a move's, the axis's body there and made earlier, its
pitch a length as an extrude's, its turns from 0.001 to 1000, and with
a helix the orientation following the path and no twist; a twist
within 8 turns either way;
a loft's 2 to 64 sections each of a sketch feature before it, a region
whose reference passes its check with no holes, a point only first or
last and not every section one, closed only with three or more
sections, no point and no rail, at most 4 rails each of a sketch
feature before it with 1 to 256 curves sorted without repeats and none
twice, and its operation as an extrude's (its points and curves in
their sketches are only checked as it's added or edited, as a split's
line's are);
the tolerance within its range, names, coordinates, radii
and labels within bounds, a sketch's item counts bounded, every reference
naming an item of the right kind, every fillet and chamfer on a corner
of two lines ending at its point, every spline's point count (at most
100), handles and knots as its kind takes them, and every dimension's
expression giving its stored value exactly when evaluated in the
document's units, a value
its measure can be (a length at least a micrometre, an angle under a
turn). Ids
running out refuses the edit instead of overflowing.
