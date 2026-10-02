# How files are opened and saved

All file system access goes through the IO lane (`crates/io`): natively one
thread for the whole app, not iced's thread pool, which also delivers the
regeneration lanes' meshes and mustn't be held up by a slow disk. Like
the regeneration lanes it's started by an iced subscription, whose stream
first hands the app the lane's sender and then yields each response as a
message.
Requests sent before it has started wait in the app. Unlike the regeneration
lanes it's an ordered queue, since writes to one file must land in order;
only a write of the recent files list or the settings still waiting is
replaced by a newer one, a save by a newer save of the same file (see "Saving"), and an
auto-save likewise, never across anything else asked of that file or a
flush. On the web the lane is a Web Worker, see `web-files.md`.

The lane keeps the app's own files where `varde_io::Stores` says: natively
the user's config and data directories. Tests pass temporary directories,
or none, so they never touch the user's.

The lane owns the open `DocumentFile`s, the app refers to them by a
`FileId`. At startup it reads the recent files list, and the welcome screen
shows an empty list until it arrives, and the settings
(`varde_io::settings`, `settings.toml` next to `recent.toml`; on the web at
the root of OPFS). They hold the theme: Auto (iced's system theme, light
when the system doesn't say), Light or Dark, cycled by the theme button and
written as chosen, though not before the stored settings have arrived; a
theme chosen before then wins over them. Each key is read on its own, so
one gone bad gets its default and costs no other. Each open is tagged, and only the
answer to the open the welcome screen is waiting for is shown. When the
user starts a new design or opens another file instead, the app tells the
lane it gave up on the open before sending anything else, so the file is
closed again, releasing its lock, before a later open of the same design
could find it locked. A request that panics is answered with an error and
the lane carries on.

**One editor per document.** Opening `dir/design.vrdp` creates the hidden
sidecar `dir/.design.vrdp.autosave` (on Windows with the hidden attribute;
for a symbolic link, next to its target, so both share one lock) if
needed and takes an exclusive, non-blocking OS lock on it, held by the IO
lane until the document is closed. If another window, process or user
holds that lock, or the sidecar can't be created or locked (say, in a
read-only directory), the document opens read-only: a banner says why, and
editing, undo and redo are off; the camera still works. OS locks go away
with the process, so a crash never leaves a design locked. Closing an
editable document deletes the sidecar, unless it holds changes to recover
(see "Recovery"), then unlocks it. An editor opening the document just then
can end up locking the deleted file, so after locking it checks that the
path still refers to the file it locked, and starts over if not. On Windows the
sidecar is opened without share-delete instead, so it can't be deleted
while open. The sidecar also holds auto-saves, so on Unix it's created
with the design's permission bits (the umask may only take some away).

**Auto-save.** A timer ticks every second while an editable document is
open (natively a small thread, since iced's thread pool executor has no
timer; on the web iced's `time::every`, on the browser's timers). Once the
document has unsaved changes and has gone three seconds without an edit,
or two minutes have passed since the first edit not auto-saved while edits
keep coming, the app sends the lane a snapshot to auto-save. It goes to the sidecar, never the design's own file, so Save
keeps its meaning and a synced file isn't touched on every edit. The
sidecar is a `.vrdp` like the design, appended to through the handle the
lane holds the lock on: `DocumentFile`'s own lock per operation would be
refused by that lock (`flock` locks per open file description,
`LockFileEx` per handle), so the sidecar is written by `HeldFile`, which
takes none. Each auto-save also records which saved version of the design
it was made from: the offset and checksum of the design file's last
record, as the lane last read or wrote it (new designs have none). Save
and Save As empty the sidecar once the design is written, and so does
undoing or redoing back to the state last saved, at the next tick, so a
crash doesn't offer edits already undone (on the web a store entry holding
the design as downloaded goes back to that instead). Read-only
designs are never auto-saved. An auto-save that fails shows in a banner
until dismissed, or until a later auto-save or save succeeds.

**New designs** have no directory for a sidecar, so New asks the lane for
an entry in the app's store of new designs, `designs/<id>.vrdp` in the
platform data directory (e.g. `~/.local/share/varde-cad/designs`, readable by
the user only), made and locked right away and auto-saved to the same way.
The first Save As writes the user's file and deletes the entry; from then on
the file's sidecar applies. Making it eagerly, rather than on the first
auto-save, means auto-saves always have a file and coalesce like any
other, and Save As moves the design from its entry like from any file; an
unedited new design costs an empty file, deleted on close.

**Recovery.** A clean close (the design is saved, or the user chose not to
save it) empties and deletes the sidecar or store entry. One found unlocked
with a document in it is from a session that crashed. Opening a design
whose sidecar holds something other than the design offers it in a banner:
Restore applies it as one undoable edit, leaving the design edited, and
Discard empties the sidecar. If the design has changed since the changes
were auto-saved from it (saved by another program or session, rewritten,
or saved by the crashed session itself before it could empty the
sidecar), that is, its last record isn't the one the auto-save records,
the banner warns that restoring may undo newer changes. File times play no
part, so touching the design doesn't warn and a save within the same
clock tick doesn't go unnoticed. Until then auto-saves wait, so as not to
replace it, and closing keeps it to be offered again. Store entries left
behind show up on the welcome screen as recovered designs, which open as
untitled designs backed by their entry, or can be discarded; empty ones are
deleted. A damaged sidecar never keeps a design from opening: it's ignored,
reported, and the next auto-save starts it over; a torn one gives up its
last complete record. If the UI dies without closing a document, the lane
lets go of it as a crash would, keeping what was auto-saved.

**Saving.** Save (`Ctrl S`, or in the file menu) sends the lane a cheap
`Arc` snapshot of the document and the editor revision it's of; encoding,
locking and writing all happen in the lane, and editing, the camera and the
UI carry on meanwhile. A save still waiting behind another save of the same
file is replaced by it. The answer carries the revision, which becomes the
saved one, so edits made while saving keep the document edited. The title
and the status bar say "Saving…" while a save is in flight. A Save or Save
As asked for while sketch edits wait on the solver waits for them too (it
counts as in flight) and is sent once they're answered or undone, so what's
on screen is saved; auto-saves don't wait (see `agents/sketch.md`). Saving
takes the
`.vrdp`'s own lock like opening does, waiting at most two seconds for
another program to let go of it. If the file was changed by someone else
since it was opened or last saved, the save is refused; a banner says so
and offers Save As. Other errors show in the same banner, and the document
stays as it is. Only the newest save's outcome counts: a save that fails
while a newer one is still in flight shows nothing, and the newer one
succeeding leaves no banner behind. Except a Save As failing while only
Saves are in flight: they write the design's own file, not the one the
Save As was to write, so its error shows.

Save As (`Ctrl Shift S`; Save of a design never saved) asks for a path in
the platform's dialog (adding `.vrdp` if it's missing, refusing to replace a
file the dialog didn't ask about) and writes a new file there: next to it
first, then renamed over whatever was there, keeping that file's
permissions. It takes the new file's lock first, so a design another editor
has open is never written over, and then lets go of the old file and its
lock. A read-only design can't be saved (Save is disabled, and `Ctrl S`
does nothing), but can be saved as a copy, which is editable if its lock
could be taken. The new file joins the recent files.

**Closing and quitting.** Closing the document, or the window, waits for
sketch edits waiting on the solver first, and the changes waiting behind
them (a delete among them that asks is asked, and waited for), then with
unsaved changes asks
whether to save them first, not to, or to stay. With a save in
flight, it waits for the answer ("Saving…"), and stays if a save failed
that no newer one supersedes, unless the changes were to be dropped anyway. Closing the window is
intercepted (iced's exit on close request is off) for this. Quitting then
closes the file and waits until the lane has finished all it was asked to
do, closing files and deleting their lock files and writing the recent
files list, before the window closes and the app exits. Auto-saves never
hold up closing: closing comes after them in the lane's queue and deletes
what they wrote. The recent files list is written to a temporary file of
its own name and renamed over the list, so two instances writing at once
don't mix, and an entry of it that doesn't parse loses only itself. The
settings are written the same way (`io/src/native/config.rs`); on the web
through the file's sync access handle. A theme chosen while quitting is
ignored, as it would be written after the flush.

**Exporting 3MF.** The visible bodies are written as a 3MF package, for
printing, from the File menu's Export 3MF… (under Save As; no key and no
tool on the rail). It goes while the model shown has a body the document
shows and regenerating hasn't failed, one export at a time, read-only
designs too. Like Save As it asks where first: the platform's save dialog
(`io::pick::pick_export`, a "3MF model" filter, suggesting `name.3mf`
from the design's name; `.3mf` is added to a path without it, and a file
there is replaced only if the dialog asked about the name as typed).
Backing out does nothing. Then the app asks the document's regeneration
lane to weld the committed document's visible bodies (`Request::Export`,
which a later regeneration never replaces, see `agents/viewport.md`), and
hands the bodies to the IO lane (`Request::Export`, never replaced
either), which writes the package: natively a new file, or one written
next to the file it replaces and renamed over it, with that file's
permissions, through a symbolic link to its target, as a design's Save
As does. The status bar says "Exporting…" until the IO lane answers.
Errors (a body that doesn't weld, by name; nothing visible left to
write; the file there; the write failing) show in a "Couldn't export"
banner until dismissed or another export starts, the message after the
dash in lower case as the save banner's are. An export closed or quit
before the bodies are welded is dropped: closing doesn't wait for it,
and bodies welded while quitting aren't sent, since they'd come after
the flush the window closes on; one already with the IO lane is written
before the app exits. Should the document's regeneration lane be replaced
while it welds, the export fails with a message to try again rather than
waiting for an answer the old lane won't send. On the web, see `web-files.md`.

`varde_regen::export` welds each visible body's solid,
in the order the history made them, into a `varde_kernel::ManifoldMesh` at
the document's tolerance (see "Solids and tessellation" in
`agents/kernel.md`), failing with the first body that doesn't give one, by
name; `varde_io::three_mf::write` makes the package's bytes from them, the
design's name as its title. The package is an OPC zip:
`[Content_Types].xml` (defaults for `rels` and `model`), `_rels/.rels`
(one relationship to `/3D/3dmodel.model` of the 3MF model type) and the
model, in the 3MF core namespace with `unit="millimeter"` (model units
are millimetres), `Title` and `Application` metadata, one `<object
type="model">` per body named after it, ids from 1, and a build item for
each. A mesh's vertices are about its origin, the middle of the body's
box rounded to whole millimetres, and are `f32` values (readers such as
lib3mf and the slicers keep single precision): the build item's
`transform` is the identity moved by the origin, left out at zero, so a
body far from the origin keeps its detail, and a reader keeping `f32`s or
`f64`s reads exactly the mesh that was checked. Coordinates are written in
Rust's shortest round-trip `f64` form (with an exponent below `1e-4`). A
body whose samples would still round together is refused, not written. Names lose
the characters XML 1.0 doesn't allow. The zip is written by hand
(`miniz_oxide` deflates the parts; no zip64, fixed 1980 timestamps); the
model's size is bounded from the meshes' counts before anything is built,
and past what a zip without zip64 holds the export is refused (no size or
offset may reach `u32::MAX`, which would say a zip64 record follows). No bodies
to write is refused too. Choices made where the 3MF spec leaves it open:
no thumbnail, no colours or materials, no `<components>`, bodies placed
where they are in the design rather than arranged for a build plate.
