# Files on the web

There's no path based file system in a browser, so the web's IO lane is a
Web Worker (`io::lane`, `crates/io/src/web.rs`) with the same API as the
native thread, keeping designs in the Origin Private File System (OPFS):
**browser storage**. OPFS files are only read and written synchronously,
at an offset, through sync access handles, which exist only in workers, so
the worker does all of it. The `.vrdp` code works on them through a small
`Storage` trait (length, read at, write at, truncate, sync), which
`std::fs::File` implements natively.

## Where a design is kept

A design is kept in one of two places, its location, which the app always
shows: a bar under the toolbar's file cell, as wide as the side panel,
says "In browser storage" or "On your computer" with its icon (a
database's drum, a computer), in the accent or green on the panel's
colour (`toolbar::location_bar`). Pointing at the file cell says more:
how it's kept and what that means (cleared with the site's data; a file
picked whose folder the browser doesn't give). The file menu starts with
where a design in browser storage stands against its downloads (see
"Downloads" below).

- **Browser storage**: a design saved by name, in every browser with OPFS.
  The default.
- **A file on your computer**: a `FileSystemFileHandle` from a picker, on
  Chromium only (the File System Access API, `showOpenFilePicker` in
  `window`).

A new design, never saved, is neither: it lives in the store of new
designs until it's first saved, as natively, and has no bar; with no
name, "Not saved" shows in its name's place (see `files.md`). A **download** is a copy
handed to the browser: it changes neither where the design is kept nor
whether it counts as saved.

OPFS is laid out like a folder on disk (`crates/io/src/browser.rs`):

    designs/<id>.vrdp            new designs never saved (`store.rs`, `opfs.rs`)
    saved/<name>.vrdp            designs saved to browser storage
    saved/.<name>.vrdp.autosave  their auto-saves and lock
    downloads.toml               the downloads made of them (`downloads.rs`)
    settings.toml                the settings

## Browser storage

A design saved to browser storage behaves like a native design saved at a
path (`files.md`), the same code where it can be: the `.vrdp` keeps one
record per save, appended (`vrdp::save`, with its conflict check) through
a sync access handle taken for the save, so its history is kept; its
auto-saves go to its sidecar, whose handle, held for as long as the design
is open, is the lock (`io::lock::Lock`, which the native lane's sidecars
use too, with the recovery offer). Another tab can't take the sidecar
(`NoModificationAllowedError`) and opens the design read-only, "open
elsewhere", as natively another process can't take the OS lock; the
browser lets go of the handle when the tab closes or reloads. A tab
closed with changes leaves the sidecar behind: the welcome screen marks
the design "Changes not saved", and opening it offers them back in the
banner, as natively after a crash (Restore, Discard, the design changed
since, damage). Opening reads the design first (`getFile`, no lock), so a
name that isn't a design never gets a sidecar. Another tab listing the
designs holds each sidecar a moment (to tell whether it's open elsewhere
or holds changes), so opening, Save As, Rename and Delete try to take a
sidecar a few times, a short pause between (`dir::take_waiting`, 300 ms
in all), before taking the design to be open elsewhere.

The code over OPFS is generic over a small directory trait, `Dir`
(`crates/io/src/dir.rs`: names, take a file exclusively, read one whole
or in parts without taking it, when it was written, remove, rename, and
a pause before trying again), so it's tested natively over `std::fs` with
OS locks standing in for the sync access handles (`FsDir`, in the tests,
and a wrapper having another tab act between the steps of one);
`OpfsDir` (`src/web/worker/opfs.rs`) is the web's, the store of new
designs (`designs/`) one too. Reading in parts goes through the file's
`getFile()` snapshot and `FileReaderSync`, which workers have.
`src/browser/folder.rs` is the code over it.

The file name is the design's name, as `document::name::download_name`
makes it, so a design downloads under the name it has in storage, and
there's no metadata to keep in step. A name is cut, at its stem, to
leave room for the longest name made from it, its temporary file's
(`browser::MAX_NAME`, 233 bytes); a name past that isn't a design's.
Names are compared as they're written; two designs can't share one.

- **Save** of a design in browser storage appends to it. Of a new design,
  it's a Save As.
- **Save As…** is an app dialog (browser storage has no picker of the
  system's, `Doc::request_save_as`, `Overlay::NamePrompt`): the name, its
  field focused with the text selected (empty for a design with no name,
  `Doc::suggested_name`: Save is disabled until one is typed), and on
  Chromium a choice of "Browser storage" (the default) or "A file on your
  computer…", which goes on to the system's save picker as before,
  suggesting no name for a design without one. Under them it says where
  the design will be saved: "Saved in this browser's storage." or "Saved
  to a file on your computer: you choose where next.". To a name a design
  listed on the welcome screen has, it asks first ("Replace"); saving
  again replaces it (`SaveTo::Browser { overwrite }`). The lane refuses a
  name taken that it wasn't asked to replace (`SaveError::Taken`), as one
  another tab saved since the app listed them, and the dialog shows again
  asking about replacing it. The list is read again after each Save As and
  rename. To the design's own name, Save As is a Save, appending, its
  history kept; only a design opened past damage, which isn't saved to,
  is replaced whole, after asking. Another name gets a new file of one
  save, its sidecar taken first and held while it's written, refused if
  another tab has the design of that name open. Every whole file is
  written under a temporary name (`.<name>.<id>.tmp`), made durable, then
  put in place, so a design is never half written: listing puts one left
  by a tab closed in between in place, or deletes it beside the design,
  taking the design's sidecar first, so it never touches one a tab is
  writing just then. What a closed tab left in the sidecar of a design
  replaced is older, and emptied. `Esc` or Cancel backs out as of the
  system's dialog, out of leaving too.
- **Rename…** (file menu, designs in browser storage, while it's
  editable and nothing's on its way) asks in the same dialog; the lane
  moves the file (`FileSystemHandle.move()`, or a copy where the browser
  hasn't it, within the message bound, the copy deleted again should the
  original stay), keeping its saves, refused over a name taken, and moves
  the newest auto-save to the new sidecar, still offered if it was. Its
  downloads follow it. A tab closed between moving the file and the
  auto-saves leaves the old name's sidecar with changes in it and no
  design: listing moves its newest auto-save to the store of new designs,
  as a new design known by the old name (`folder::tidy`), offered with the
  designs crashed sessions left; until then, no design is saved or copied
  in under that name.
- **Delete**, from the welcome screen, takes the design and its sidecar,
  refused while it's open in this tab or another: the card of one open
  in another tab offers no Delete. One whose latest isn't downloaded is
  asked about first: the browser may have the only copy.

Browser storage running out of room is said plainly ("browser storage is
full: download designs and delete them, or free space"). Browser storage
can be cleared by the user, or evicted by the browser under storage
pressure unless the site's storage is persistent: the app asks for that
(`navigator.storage.persist()`, page side, `varde_io::storage`) once a
session, as the user first saves there (a Save As within the click), or
a file is copied in as it's opened, and at startup and
on the welcome screen reads whether it is (`persisted()`) and how much is
used (`estimate()`). If it isn't, the welcome screen says the browser may
clear what's kept in it, and to download what matters; its foot says how
much is used.

## Downloads

Download (file menu, web only; and the arrow on a welcome screen card)
hands the design over as `<name>.vrdp`. From the document it's encoded on
the page (`Doc::download`, `varde_io::vrdp::to_bytes`, one encoding,
cheaper than a round trip through the lane): the design as it is, unsaved
changes too. From the welcome screen the lane reads the file as it's
saved, history and all (`Request::DownloadFromBrowser`), and the page
downloads it whether or not the download could be recorded, saying so if
it couldn't. A download of a design in browser storage is recorded in
`downloads.toml` (TOML, at the root): the design's file name, when, and
which save it was by the save's record `sum`, which any later save, or a
whole file written in its place, tells apart; a download with changes not
saved, or on their way to the file, records none. Each entry is read on
its own, from its `[[download]]` line to the next, one gone bad costing
only itself, the newer of two of one design kept, at most 1024 designs',
the oldest dropped. It's changed while held, read and written through its
sync access handle, waiting a moment for another tab recording at once
(`dir::take_waiting`), so neither loses the other's; it's written in
place and cut to length, so a tab closed as it writes it loses at most
the entry it was in. Listing reads it without taking it. A design written
anew under a name, saved as it, copied in as it or deleted, has the
downloads recorded of the name forgotten. The welcome screen and the file menu
say where a design stands against its downloads (a coloured dot):

- amber **Never downloaded**: nothing recorded;
- blue **Changed since downloaded** *when*: saved since, or changes not
  saved (in the sidecar, or in the editor), or downloaded with changes
  not saved;
- green **Latest downloaded** *when*: the download is the design as saved,
  with no changes since.

The file menu goes by the editor: latest while the editor is at the
revision downloaded and that's the one saved. A file on the computer, or
a new design, records nothing. Nothing is downloaded by Save.

Older builds downloaded on Save and kept the design as downloaded in its
store entry, marked `downloaded` in the auto-save record. Nothing writes
that mark now; the format keeps it, so those entries still read, and are
offered back as recovered designs like any other.

## Opening

- **Chromium**: Open… shows the file picker; the design goes on from that
  file (location: your computer), Save writing it back (see "Files of the
  user's" below).
- **Elsewhere** (Firefox, Safari): Open… is a hidden file input. The lane
  checks the file is a design, copies it into browser storage under its
  name, made unique with " (2)" and so on, the stem cut to make room
  (`folder::copy_in`), as it is, with its history, written as Save As
  writes, and opens it from there, saved; the status bar says "Copied to
  browser storage as …". A name with a sidecar but no design isn't taken.
  Should copying it fail (the site's data blocked, a private window,
  storage full), it opens as before there was browser storage: a copy, a
  new design known by the file's name, auto-saved to an entry if one can
  be made, the status bar saying why it wasn't kept (`Opened::not_copied`).
  A copy found damaged and not opened after all, the prompt cancelled, is
  deleted again.
- **Dropping** a `.vrdp` file on the page opens it as Open… would with
  what the drop hands over (`varde_io::pick::dropped`): where the File
  System Access API is, its handle (`DataTransferItem.getAsFileSystemHandle`,
  asked for as it's dropped and waited for after), else the `File`, copied
  in.
- **The welcome screen** lists what's in browser storage (see below);
  clicking a card opens it.

## The welcome screen

On the web it's a page rather than the native column: a header bar with
the logo and New design and Open…, over the cards of what's in browser
storage, newest first, the first of them a drop zone: the designs saved
there, each with its thumbnail, when it was saved (when the browser says
its file was written) and its download status, "Changes not saved" or
"Open in another tab" in place of when if so, Download and Delete at its
foot (Delete faint and refused for one open in another tab); and new
designs never saved that a closed tab left (`designs/`), "Auto-saved",
opening as untitled designs backed by their entry, the trash discarding
one. Listing reads only the end of each design's file (`vrdp::end`: the
header, the newest record's `sum` and its previews, no payload), so a
file that isn't a design is marked so, and damage before the end shows as
it's opened; a sidecar is only checked for being empty. The app keeps each
thumbnail's image handle while the design's newest `sum` stays the same.
Over them, a note when the browser may clear its storage; at its foot,
how much is used. The app asks the lane for the list
(`Request::ListBrowser`) at startup, before the store's, as listing may
move changes into it (the lane then lists the store again), as the
welcome screen shows again (after the close, so the design just closed is
let go of first), and after a Save As to it, a rename, a delete or a
download. A `.vrdp` file dropped anywhere on the page
opens; clicking the drop zone shows the Open picker. The app always
listens for drags carrying files (`platform::drops`) and takes them from
the browser, which would otherwise leave the page for the file dropped.
Whether one is over the page is counted by the elements it entered and
hasn't left, as WebKit doesn't say where a drag went, and a `dragover`
with none counted comes over it again; the app is told only as that
changes, lighting the drop zone. A file is kept in the page's registry as
it's dropped; one the app doesn't open, dropped with a document open or
over a dialog, is let go of (`pick::forget`).

### Samples

Below browser storage the page can list **samples**: the designs of the
workspace's `examples/` (written by `cargo run -p varde-view --example
samples`, with the thumbnail a save would write, rendered offscreen
through `varde_view::thumbnail_shot`, `ThumbnailRequest::COLORS` and
`varde_render::render_preview`, encoded by `varde_io::thumbnail::previews`;
`crates/io/tests/samples.rs` checks each file has one), each a card with
its thumbnail (`thumbnail::of_file`, decoded once into
`Files::sample_thumbnails`),
its name and a line on what it shows. They're built into the app with `include_bytes!` behind
`varde-app`'s `samples` feature (`crates/app/src/samples.rs`), which
`varde-web`'s feature of the name turns on; trunk passes it for a
`data-cargo-features="samples"` on the page's `varde-web` link, which the
GitHub Pages build adds. Without it (`trunk serve`, native builds) there
are none, and the heading isn't shown. Built in rather than fetched as
files: they're a few kilobytes, need no manifest, copy step or fetch
and can't go missing from the site, and open without waiting.

Clicking one opens it as New design does, as a new design known by the
sample's name: never saved, auto-saved to an entry in `designs/`, its
first Save a Save As suggesting the name (`Welcome::open_sample`).
Nothing is put in browser storage until the user saves it, so opening a
sample again and again leaves no copies, and closing one unchanged
leaves nothing.

The page's title (`document.title`) is the design's name, as the native
window's (`platform::show_title`), so the tab and the history show it.

## New designs

New makes an entry in `designs/`, holds its handle for as long as the
design is open, and auto-saves are appended through it. Another tab can't
take it, so it doesn't list the design as recovered, and can't open it.
The first Save is a Save As, which moves the design into browser storage
(or to a file on Chromium) and deletes the entry. Closing it cleanly,
saved or not kept, empties and deletes its entry. Entries left behind with
a design in them are listed on the welcome screen; empty ones are deleted
when listed. Should the entry not be made, say with the site's data
blocked, the design isn't auto-saved, which the banner says.

## Files of the user's

Files of the user's (`crates/io/src/pick.rs`) are only the ones the user
picks; the page shows the pickers, from the app's `update` right after the
click or key press, within the browser's transient activation. Where the
File System Access API is there, Open and Save As's "A file on your
computer…" show its pickers, which hand over a `FileSystemFileHandle`,
and the design goes on from that file like natively: Save writes it back,
Save As (suggesting the design's name) moves the design to the new file.
The page keeps the handle and the app a `Picked` for it; requests using
one post it to the worker along with the bytes, handles being
structured-cloneable. The worker reads the file whole (`getFile()`) and
replaces it whole (`createWritable()`, which writes a copy and moves it
over the file on close), with the design as the file's one record, with a
new file `id`: a file of the user's has no sync access handle to append
through. So a file saved on the web keeps no history, which saves space
too: one saved elsewhere comes back with one record, a damaged newest
save dropped with the rest. Chromium asks the user before a page first
writes to a file it opened, and only the page can ask, so the app asks as
the user saves, before sending the save. Before writing, the worker makes
the native save's check (that the file still holds the record it last
read or wrote, undamaged), refusing the save as a conflict or damage like
natively; another program writing between that check and the write isn't
caught. Nor can a file of the user's be locked: two tabs can have it open
for editing, and only the conflict check keeps one from saving over the
other. Its auto-saves go to an entry in `designs/`, each record along
with the file's name and its last record, as the sidecar does natively;
one left behind is listed by its name, and opens as a copy by that name:
the handle went with the tab. Should the entry not be made, the file
still opens and saves, only without auto-saves, which the banner says.
Saved as a design in browser storage, it lets go of the entry; a design in
browser storage saved as a file gets one. Recent files on Chromium would
need the handles kept in IndexedDB, and aren't there yet.

A picked file opened damaged comes with its `Opened::damage` and the save
a search found, as natively, and one damaged past a save that couldn't be
read is asked about the same way and only saved as another file: Save
acts as Save As. The same goes for a design in browser storage.

## The rest

The settings are `settings.toml` at the root of OPFS, read through
`getFile` (no handle taken, so another tab writing them doesn't block it)
and written through a sync access handle, which every browser with OPFS
has in workers.

The panic recorded (see "Panics" in `files.md`) is the exception: the
panic hook runs on the page, which can't reach OPFS without waiting, so
it's kept in the page's `localStorage` under `varde-panic`, and the page
answers `Request::LoadPanic` and `Request::DiscardPanic` itself
(`io/src/web/page/panicked.rs`) without posting them to the worker,
whatever became of it. A worker can't keep it either (no
`localStorage`, and its instance traps), so a worker's panic is recorded
by the page as the panic message arrives (`varde_lane::page::on_worker_panic`),
with the worker's name as the thread and no location or backtrace (they
are in the console).

The page and the worker share no memory. Each message is postcard bytes in
a transferred `ArrayBuffer` (with a picked file's handle or `File` cloned
alongside): requests numbered in the order sent, and responses carrying
the number, with documents as postcard too (`varde_document::codec`).
Both sides check what they get (a size bound, no bytes left over, documents that
decode and pass their checks; see `io::wire`), and a picked file, or one
in browser storage, is read only up to the same bound. Requests sent
before the worker is ready wait on the page. The worker queues them and
replaces waiting saves and auto-saves like the native lane (a rename or a
download recorded keeps a save after it from replacing one before). If it
dies, the requests it owed and every later one are answered with the
error, and no new worker is started, since it would hand out file ids the
app holds for the old one's files; the browser lets go of its handles,
keeping what was auto-saved.

**Exporting 3MF** (see `files.md`) goes the way a file on the computer
does. With the File System Access API, Export 3MF… shows the save picker
(a "3MF model" type, suggesting `name.3mf`), and the bodies, welded in
the regeneration worker, go to the IO worker along with the handle, which
replaces the file whole with the package (`createWritable()`); the picker
granted the write, so nothing is asked. Elsewhere there's nothing to
pick: the bodies are welded at once and the page encodes the package and
downloads it as `<name>.3mf`, as a design's download is encoded on the
page. That download comes when the regeneration worker answers, after the
click's transient activation may have run out; Firefox downloads it all
the same. Nothing is kept in OPFS for an export. The welded bodies cross
both workers as postcard, each mesh checked again as it's decoded: from
the regeneration worker as one part after the reply's head (within 1 GiB),
to the IO worker in the request (within its message bound).

Import is still to come on the web. Open… listing browser storage in an
app dialog, folders in browser storage, and recent files on Chromium are
left for later.
