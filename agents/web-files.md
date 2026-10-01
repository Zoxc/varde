# Files on the web

There's no path based file system in a browser, so the web's IO lane is a
Web Worker (`io::lane`, `crates/io/src/web.rs`) with the same API as the
native thread, keeping auto-saves in the Origin Private File System
(`crates/io/src/opfs.rs`), laid out like the native store:
`designs/<id>.vrdp`. OPFS files are only read and written synchronously,
at an offset, through sync access handles, which exist only in workers, so
the worker does all of it. The `.vrdp` code works on them through a small
`Storage` trait (length, read at, write at, truncate, sync), which
`std::fs::File` implements natively.

A sync access handle is exclusive, so it's the lock: New makes an entry and
holds its handle for as long as the design is open, and auto-saves are
appended through it. Another tab can't take it
(`NoModificationAllowedError`), so it doesn't list the design as
recovered, and can't open it. The browser lets go of the handle when the
tab closes or reloads. Entries left behind with a design in them show up on
the welcome screen as recovered designs, like after a crash natively, and
open as untitled designs backed by their entry; Discard deletes one, empty
ones are deleted when listed. Closing the design cleanly, saved or not
kept, empties and deletes its entry, except after a download, see below.

**The user's files** (`crates/io/src/pick.rs`) are only the ones the user
picks; the page shows the pickers, from the app's `update` right after the
click or key press, within the browser's transient activation. Where the
File System Access API is there (Chromium, `showOpenFilePicker` in
`window`), Open and Save As show its pickers, which hand over a
`FileSystemFileHandle`, and the design goes on from that file like
natively: Save writes it back, Save As (suggesting the design's name)
moves the design to the new file. The page keeps the handle and the app a
`Picked` for it; requests using one post it to the worker along with the
bytes, handles being structured-cloneable. The worker reads the file whole
(`getFile()`) and replaces it whole (`createWritable()`, which writes a
copy and moves it over the file on close), with the design as the file's
one record: a file of the user's has no sync access handle to append
through. Chromium asks the user before a page first writes to a file it
opened, and only the page can ask, so the app asks as the user saves,
before sending the save. Before writing, the worker checks the file still
holds the record it last read or wrote, and refuses the save as a conflict
otherwise, like natively; another program writing between that check and
the write isn't caught. Nor can a file of the user's be locked: two tabs
can have it open for editing, and only the conflict check keeps one from
saving over the other.

Elsewhere (Firefox, Safari) Open is a hidden file input: the design opens
as a copy, an untitled design with the file's name, since the file can't be
written back. Save and Save As download the design as `<name>.vrdp`
(encoded on the page, one snappy pass over the postcard it makes for
every auto-save anyway), after which it counts as saved: the browser has it.
Whether the user kept the download, rather than cancelling the browser's
save dialog, isn't something a page is told, though. So instead of being
emptied as by a Save, the design's entry gets the design as downloaded,
an auto-save record marked `downloaded` (sent as a request of its own,
`KeepDownload`, behind the auto-saves before it, which it doesn't
replace, and never replaced by a later one while it waits; auto-saves of
later edits aren't marked), and closing the design keeps the entry.
Should the user choose "Don't save" for changes made since, the clean
close goes back to the download rather than deleting the entry: the lane
cuts the entry short right after the newest marked record, dropping the
auto-saves after it (and a torn one), so it's the newest again. Only an
entry never downloaded to is deleted. A download just after New, before
the lane has made the entry, is kept by the app and sent, marked, as soon
as the entry arrives, ahead of any auto-save; closed before that, the app
waits for the entry instead of giving it up, then sends the download and
the clean close. Should the entry fail to be made, the design isn't
auto-saved at all, which the banner says. The welcome screen lists
entries whose newest record is marked apart from designs never saved,
and more quietly: a row under "Downloaded" with the name it was
downloaded as and when, to open or discard. Opened, it goes on as an
untitled copy like a recovered design, but not edited, since it's what
was downloaded (as the entry holds it when opened, not as listed: another
tab may have left changes in it since); closing it again keeps it listed.
Discarding changes never saved listed on top of a download goes back to
the download, as "Don't save" does, rather than deleting it too. They stay
until discarded; expiring old ones is a possible follow-up. A record
damaged after the download doesn't keep a clean close from going back to
it: cutting the entry short drops the damage too.

Auto-saves of every design, opened from a file or not, go to an entry in
OPFS, each record along with the file's name and, for a file written in
place, its last record, as the sidecar does natively. One left behind is
listed on the welcome screen by its name, and opens as a copy by that name:
the handle went with the tab. Should the entry not be made, say with the
site's data blocked, a file of the user's still opens and saves, only
without auto-saves, which the banner says. Recent files would need the
handles kept in IndexedDB, and aren't there yet.

The page and the worker share no memory. Each message is postcard bytes in
a transferred `ArrayBuffer` (with a picked file's handle or `File` cloned
alongside): requests numbered in the order sent, and responses carrying
the number, with documents as the encoding `.vrdp` records use. Both sides
check what they get (a size bound, no bytes left over, documents that
decode and pass their checks; see `io::wire`), and a picked file is read
only up to the same bound. Requests sent before the worker is ready wait
on the page. The worker queues them and replaces waiting saves and
auto-saves like the native lane. If it dies, the requests it owed and every
later one are answered with the error, and no new worker is started, since
it would hand out file ids the app holds for the old one's files; the
browser lets go of its entries, keeping what was auto-saved.

**Exporting 3MF** (see `files.md`) goes the way Save As does. With the
File System Access API, Export 3MF… shows the save picker (a "3MF model"
type, suggesting `name.3mf`), and the bodies, welded in the regeneration
worker, go to the IO worker along with the handle, which replaces the
file whole with the package (`createWritable()`); the picker granted the
write, so nothing is asked. Elsewhere there's nothing to pick: the bodies
are welded at once and the page encodes the package and downloads it as
`<name>.3mf`, as a design's download is encoded on the page. That
download comes when the regeneration worker answers, after the click's
transient activation may have run out; Firefox downloads it all the same.
Nothing is kept in OPFS for an export. The welded bodies cross both
workers as postcard, each mesh checked again as it's decoded: from the
regeneration worker as one part after the reply's head (within 1 GiB),
to the IO worker in the request (within its message bound).

Import is still to come on the web.
