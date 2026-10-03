# Saving on the web

Goal: on the web, Save and Save As keep the design in the browser's own
storage, as named designs that stay there between visits, and a download
is something the user asks for, never what Save does. Where Chromium's
File System Access API is there, a file on the user's computer can still
be opened, and saved back to. Wherever a design is kept, the app says so.

Built (Steps 1–6 below, with the deviations under "As built");
`agents/web-files.md` says how it works now; the code wins over both.

## Why

Today a design on the web lives in one of three places, and only one of
them is the user's choice:

- An entry in the Origin Private File System (OPFS), `designs/<id>.vrdp`,
  holding auto-saves. Every design has one, but it's a recovery copy, not
  a place to keep work: closing cleanly deletes it.
- A file of the user's, through a `FileSystemFileHandle` (Chromium only):
  Save writes it back.
- A download (Firefox, Safari): Save downloads `<name>.vrdp` and calls
  the design saved, since the page can't know whether the user kept the
  file. To make up for that, the entry is kept, marked `downloaded`, and
  the welcome screen lists it under "Downloaded" (`Request::KeepDownload`,
  `agents/web-files.md`).

So outside Chromium, saving means a file in Downloads, a new one each time
(`bracket (3).vrdp`), and the work you can come back to in the browser is
whatever the download marker kept. The welcome screen can't list recent
files (they'd need handles kept in IndexedDB), so a returning user sees
recovered entries and downloads, not their designs.

## The model

A design is kept in one of two places, its **location**:

- **Browser storage**: a named design in OPFS, saved and listed like a
  file in a folder. Every browser with OPFS has it, and it's the default.
- **A file on your computer**: a `FileSystemFileHandle` from a picker,
  Chromium only, as today.

A **download** is a copy handed to the browser. It doesn't change where
the design is kept, or whether it counts as saved.

| Command | Browser storage | A file on your computer |
|---|---|---|
| Save | writes a save into the design's storage file | writes the file back, as today |
| Save As… | asks for a name (and on Chromium, the place); a new storage file | the same dialog; can move the design into browser storage |
| Download | downloads `<name>.vrdp`, records the download | the same |
| Open… | the file picker or file input; see below | |
| Close, unsaved | asks Save / Don't save / Cancel, as natively | as natively |

### Browser storage as a folder

Browser storage gets a layout like a folder on disk, beside the store of
new designs:

    designs/<id>.vrdp           new designs never saved, as today
    saved/<name>.vrdp           designs saved to browser storage
    saved/.<name>.vrdp.autosave their auto-saves and lock, as natively
    downloads.toml              the downloads made, see below
    settings.toml               as today

A design saved to browser storage behaves like a native design saved to a
path (`agents/files.md`): the `.vrdp` keeps one record per save, appended
through a sync access handle, so its history is kept the way native
saves' is. Auto-saves go to the sidecar, whose held sync access handle is
the lock: another tab can't open the design for editing, as natively
another process can't. A tab closed with changes leaves the sidecar
behind, offered back as natively after a crash. This is the native path
code over the `Storage` trait instead of `std::fs`. The native files and
sidecar code (`src/native/files.rs`, `src/native/sidecar.rs`) would move
over `Storage` and a small directory trait, with OPFS (`src/web/worker/opfs.rs`)
as the web's implementation.

The name is the file name, so there's no metadata to keep in step:
renaming is Save As (and deleting the old one), and names follow
`document::name::download_name`'s rules, so a design downloads under the
name it has in storage. Two designs can't share a name: Save As to a name
in use asks before replacing it, as a native save dialog does.

New designs keep `designs/<id>.vrdp` until first saved, as natively. The
first Save is a Save As.

### Downloads

Download (in the File menu, beside Export) downloads `<name>.vrdp` from
the page, as Save does today on Firefox. It changes nothing in storage.
It records the download in `downloads.toml`: the storage file's name,
when, and which saved record it was (the record's id, so a later save is
told apart). The welcome screen and the file menu use that to say where
the design stands against its downloads:

- **Never downloaded**: nothing recorded for it.
- **Changed since downloaded <date>**: saves or unsaved changes since the
  record downloaded.
- **Latest downloaded <date>**: the download is the design as saved, with
  no changes since.

A file on the user's computer needs no record: it's on their computer
already.

### Opening

- **Chromium**: Open… shows the file picker, as today. The design goes on
  from that file (location: your computer), Save writing it back. Opening a
  design from browser storage is the welcome screen's (and Open's, see
  below).
- **Elsewhere**: Open… is a file input, as today, but the design no longer
  opens as an untitled copy. It's copied into browser storage under the
  file's name (made unique with " (2)" and so on), then opened from there,
  saved. The app says so: "Copied to browser storage".
- **Dropping** a `.vrdp` file on the welcome screen opens it as Open… would
  with what the drop hands over: on Chromium a handle
  (`DataTransferItem.getAsFileSystemHandle`), so the file itself, and
  elsewhere a `File`, so a copy into storage.

Open… might also list browser storage in an app dialog before the
browser's picker, since browser storage has no picker of the system's.
The welcome screen already lists it, so this can wait.

### Saying where it's kept

The user always sees the location, since the two look the same otherwise:

- **A bar under the file cell** in the toolbar (the design's name) says
  "In browser storage" or "On your computer" with its icon, and pointing
  at the cell says more.
- **Save As…** is an app dialog (no system picker for browser storage)
  with the name, and on Chromium a choice of "Browser storage" or "A file
  on your computer…", the second going on to the system's save picker.
- **The welcome screen** lists browser storage as "In browser storage",
  each with its download status, and Download beside Open and Delete. See
  the web layouts in `notes/ui-mock/welcome.html`.
- **The title** (`document.title`) has the design's name, so the browser's
  tab and history show it.
- **Banner**: browser storage can be cleared by the user, or evicted by
  the browser under storage pressure unless the site has persistent
  storage. The app asks for it (`navigator.storage.persist()`) on the
  first save to browser storage, and if it's refused, the welcome screen
  says designs could be cleared by the browser, and to download what
  matters. `navigator.storage.estimate()` gives the space used and left,
  for the welcome screen's foot.

## What goes

- **Save as download**: Save no longer downloads. `Request::KeepDownload`,
  the `downloaded` mark on auto-save records, and keeping entries past a
  clean close go, along with the "Downloaded" list. The mark is a field of
  the auto-save payload, so dropping it changes the `.vrdp` format and
  needs the user's say-so first (`AGENTS.md`). It can stay, unused, until
  then: old entries with it still read.
- **Untitled copies** of opened files (Firefox, Safari): opened files are
  copied into browser storage instead.
- No migration: entries left from before (`designs/<id>.vrdp`, marked or
  not) are offered back as recovered designs, as now, while the app is WIP.

## Steps

1. **Storage over a directory trait.** Move the native file + sidecar
   code (save, auto-save, the lock, recovery) over `Storage` and a
   directory trait (list, open, create, remove, rename), `std::fs`
   natively and OPFS on the web. Native behaviour unchanged, its tests
   passing; the OPFS side tested natively as `src/opfs.rs` is, through the
   trait.
2. **Browser storage in the IO worker.** `saved/` in OPFS: Save and Save As
   to a name (`Request::Save` with a storage name where natively a path),
   open, list, delete, the sidecar lock across tabs. Tests over an
   in-memory directory.
3. **The app's commands.** A location on the open design (storage name,
   handle, or none for a new design). Save to browser storage by default,
   Save As as an app dialog, Download as its own command, the file cell's
   location. Firefox's Open copies into storage.
4. **Downloads.** `downloads.toml`, recorded by Download, read for the
   status. Drop Save's download path (and, with permission, the
   `downloaded` mark).
5. **Welcome screen.** List browser storage with download status, Delete
   and Download beside Open; the drop zone; persistent storage and the
   space used.
6. **Docs.** `agents/web-files.md` and `agents/files.md` rewritten for
   it, and the module docs of `src/pick.rs` and `src/opfs.rs`.

## Open questions

- Should Chromium's Save As default to browser storage or to a file? The
  table says storage, so it's the same everywhere; users who live in files
  pick the other.
- Recent files on Chromium (handles kept in IndexedDB, permission asked
  again per session) would list files on the computer beside browser
  storage. Separate from this, but the welcome screen should leave room.
- Should browser storage have folders? Not to start with.
- Quota errors on save: they need their own message ("Browser storage is
  full: download designs and delete them, or free space") rather than the
  generic write failure.

## As built

- **The directory trait** (`crates/io/src/dir.rs`, `Dir`: names, take
  exclusively, read whole or in parts without taking, modified, remove,
  rename, pause) is async, as OPFS is. Browser storage's code over it is `src/browser/folder.rs`,
  tested natively over `std::fs` with OS locks standing in for the sync
  access handles (`dir/fs.rs`, test only); `OpfsDir` is the web's. The
  native lane's files and sidecars were *not* moved over it: their paths
  need symbolic links resolved, permissions kept, Windows' share modes,
  the deleted-while-locking race and rename-over-a-temporary, none of
  which OPFS has, so a trait carrying them would be the native code
  itself. What's shared is the format and lock code over `Storage`:
  `vrdp::save` and its conflict check, `autosave::Held`, and the
  sidecar's lock with its recovery offer, moved out of
  `native/files.rs` into `src/lock.rs`, which both lanes use.
- **Rename** is a command of its own (file menu, the Save As dialog
  asking for the name), moving the file so its history is kept; Save As
  to another name writes a new file of one save, as natively, keeping the
  old design.
- **Every whole file** written to storage, a new design's, one saved
  over another (Save As to a name taken, after asking) or a file copied
  in, goes under `.<name>.<id>.tmp` and is put in place, the design's
  sidecar held throughout; listing puts one a closed tab left in between
  in place, taking the sidecar first so it never touches one being
  written. Save As to the design's own name is a Save, keeping the
  history, except for one opened past damage, replaced after asking. A
  name another tab took since the app listed them comes back as
  `SaveError::Taken`, and the dialog asks about replacing it.
- **Names** are cut to leave room for the temporary file's name
  (`browser::MAX_NAME`), a copy's " (2)" cutting the stem.
- **Listing** reads only the end of each design file (the newest record's
  `sum` and previews), so damage before it shows as the design opens; its
  time is when the browser says the file was written. Others wait a
  moment for the sidecars it holds as it lists (`dir::take_waiting`).
- **A tab closed mid-rename** can leave the old name's sidecar with
  changes and no design: listing moves them to the store of new designs,
  offered as a recovered design under the old name.
- **Downloads** are recorded by the saved record's `sum`, which tells any
  later save apart; one with changes not saved, or still on their way to
  the file, records none. `downloads.toml` is written in place, each
  entry read on its own, so a torn write costs one entry.
- **Opening on Firefox/Safari** with browser storage unavailable (blocked
  site data, a private window, full) opens the file as a copy, as before,
  saying why it wasn't kept.
- **The banner** shows whenever the browser says the site's storage
  isn't persistent and something's kept there, refused or not asked yet.
- **Download on the welcome screen's cards** reads the file as saved,
  history and all, through the lane, which records it; the download
  happens even if recording it fails.
- **The mock** (`notes/ui-mock/welcome.html`) lags the built web cards:
  it has no Download button on them, nor the note that the browser may
  clear its storage, the space used at the foot, or the cards' notes
  ("Changes not saved", "Open in another tab").
- **Left for later**, as the open questions say: recent files on
  Chromium, folders, Open… listing browser storage in an app dialog.
- The `downloaded` mark on auto-save records stays in the format, written
  by nothing, read as before; `Request::KeepDownload` and the rolling back
  to it are gone.
