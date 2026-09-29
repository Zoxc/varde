use std::sync::Arc;

use varde_document::{Document, MAX_NAME_LEN};

use super::*;
use crate::tests::{TempDir, auto_saved_at, with_bodies};

#[test]
fn sidecar_sits_next_to_the_document() {
    assert_eq!(
        sidecar_path(Path::new("/dir/design.vrdp")).unwrap(),
        Path::new("/dir/.design.vrdp.autosave")
    );
    assert_eq!(sidecar_path(Path::new("/")), None);
}

#[test]
fn sidecar_names_are_known() {
    assert!(is_sidecar_name(Path::new("/dir/.design.vrdp.autosave")));
    assert!(is_sidecar_name(Path::new(".x.AUTOSAVE")));
    assert!(is_sidecar_name(
        &sidecar_path(Path::new("/dir/.hidden")).unwrap()
    ));
    assert!(!is_sidecar_name(Path::new("/dir/design.vrdp")));
    assert!(!is_sidecar_name(Path::new("/dir/design.autosave")));
    assert!(!is_sidecar_name(Path::new("/dir/.autosave")));
    assert!(!is_sidecar_name(Path::new("/")));
}

/// The clean close deletes only the file it locked: one renamed over it
/// since is left, as is the new file of a design saved there.
#[cfg(unix)]
#[test]
fn close_leaves_a_file_put_in_its_place() {
    let dir = TempDir::new("sidecar-replaced-held");
    let sidecar = lock(&dir.0.join("doc.vrdp")).unwrap();
    let other = dir.0.join("other");
    std::fs::write(&other, "someone else's").unwrap();
    std::fs::rename(&other, dir.sidecar()).unwrap();
    sidecar.end(Ending::Close).unwrap();
    assert_eq!(std::fs::read(dir.sidecar()).unwrap(), b"someone else's");
}

#[test]
fn a_second_lock_is_refused_until_the_first_closes() {
    let dir = TempDir::new("sidecar-twice");
    let first = lock(&dir.0.join("doc.vrdp")).unwrap();
    assert!(dir.sidecar().exists());
    assert_eq!(lock(&dir.0.join("doc.vrdp")).unwrap_err(), ReadOnly::InUse);

    first.end(Ending::Close).unwrap();
    assert!(!dir.sidecar().exists());
    lock(&dir.0.join("doc.vrdp"))
        .unwrap()
        .end(Ending::Close)
        .unwrap();
}

#[test]
fn a_crash_leaves_the_sidecar_unlocked() {
    let dir = TempDir::new("sidecar-crash");
    drop(lock(&dir.0.join("doc.vrdp")).unwrap());
    assert!(dir.sidecar().exists());
    lock(&dir.0.join("doc.vrdp"))
        .unwrap()
        .end(Ending::Close)
        .unwrap();
    assert!(!dir.sidecar().exists());
}

/// Letting go of it without the clean close, as the lane does as it ends,
/// keeps what's in it for the next editor to recover.
#[test]
fn release_keeps_a_sidecar_with_something_in_it() {
    let dir = TempDir::new("sidecar-kept");
    let mut sidecar = lock(&dir.0.join("doc.vrdp")).unwrap();
    sidecar
        .append(None, &Arc::new(Document::example()), Origin::Edited)
        .unwrap();
    sidecar.end(Ending::Release).unwrap();
    let mut again = lock(&dir.0.join("doc.vrdp")).unwrap();
    assert_eq!(read(&mut again), Some(Document::example()));
    // Empty, it's deleted.
    again.clear().unwrap();
    again.end(Ending::Release).unwrap();
    assert!(!dir.sidecar().exists());
}

/// The clean close empties and deletes it whatever is in it.
#[test]
fn close_deletes_a_sidecar_with_something_in_it() {
    let dir = TempDir::new("sidecar-closed");
    let mut sidecar = lock(&dir.0.join("doc.vrdp")).unwrap();
    sidecar
        .append(None, &Arc::new(Document::example()), Origin::Edited)
        .unwrap();
    sidecar.end(Ending::Close).unwrap();
    assert!(!dir.sidecar().exists());
}

/// Auto-saves are written through the lock's own handle while it's held:
/// they neither wait for the lock nor let go of it.
#[test]
fn auto_saves_go_through_the_lock() {
    let dir = TempDir::new("sidecar-append");
    let mut sidecar = lock(&dir.0.join("doc.vrdp")).unwrap();
    let start = std::time::Instant::now();
    for bodies in [1, 0, 1] {
        let document = with_bodies(bodies);
        sidecar
            .append(None, &Arc::new(document.clone()), Origin::Edited)
            .unwrap();
        assert_eq!(read(&mut sidecar), Some(document));
    }
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(lock(&dir.0.join("doc.vrdp")).unwrap_err(), ReadOnly::InUse);
    // Readable by a reader that doesn't lock it.
    let copy = auto_saved_at(&dir.sidecar()).unwrap();
    assert_eq!(*copy.document, Document::example());
}

/// Each auto-save keeps the saved version of the design it was based on,
/// if any.
#[test]
fn auto_saves_keep_the_version_they_were_based_on() {
    let dir = TempDir::new("sidecar-base");
    let design = dir.design();
    let (_, tail) = crate::vrdp::from_bytes(&std::fs::read(&design).unwrap()).unwrap();
    let mut sidecar = lock(&design).unwrap();
    sidecar
        .append(Some(tail), &Arc::new(Document::default()), Origin::Edited)
        .unwrap();
    assert!(sidecar.read().unwrap().unwrap().based_on(tail));

    // A new design's is based on no design's file.
    sidecar
        .append(None, &Arc::new(Document::default()), Origin::Edited)
        .unwrap();
    let saved = sidecar.read().unwrap().unwrap();
    assert_eq!(saved.base, None);
    assert!(!saved.based_on(tail));
}

/// Each record says whether it's the design as downloaded, which only the
/// newest record decides, and which survives being read back from the
/// file, as by the next session.
#[test]
fn auto_saves_keep_whether_they_were_downloaded() {
    let dir = TempDir::new("sidecar-downloaded");
    let mut sidecar = lock(&dir.0.join("doc.vrdp")).unwrap();
    sidecar
        .append(None, &Arc::new(Document::default()), Origin::Edited)
        .unwrap();
    assert!(!sidecar.read().unwrap().unwrap().origin.is_download());
    sidecar
        .append(None, &Arc::new(Document::example()), Origin::Downloaded)
        .unwrap();
    assert!(sidecar.read().unwrap().unwrap().origin.is_download());
    let copy = auto_saved_at(&dir.sidecar()).unwrap();
    assert!(copy.origin.is_download());
    assert_eq!(*copy.document, Document::example());

    sidecar
        .append(None, &Arc::new(Document::default()), Origin::Edited)
        .unwrap();
    assert!(!sidecar.read().unwrap().unwrap().origin.is_download());
    sidecar.end(Ending::Close).unwrap();
}

/// A store entry's design name is bounded like a body name: a crafted
/// entry with a long one, which the welcome screen would lay out, doesn't
/// read.
#[test]
fn long_names_are_refused() {
    let dir = TempDir::new("sidecar-long-name");
    for (len, reads) in [(MAX_NAME_LEN / 2, true), (70_000, false)] {
        let mut sidecar = lock(&dir.0.join("doc.vrdp")).unwrap();
        let name = Some("é".repeat(len));
        sidecar
            .held
            .append(None, name, &Arc::new(Document::example()), Origin::Edited)
            .unwrap();
        let read = sidecar.read();
        assert_eq!(read.is_ok(), reads, "{len}: {read:?}");
        sidecar.end(Ending::Close).unwrap();
    }
}

/// The document `sidecar` holds, if any.
fn read(sidecar: &mut LockFile) -> Option<Document> {
    sidecar
        .read()
        .unwrap()
        .map(|saved| Arc::unwrap_or_clone(saved.document))
}

/// An editor closing the document deletes the sidecar between this one
/// opening and locking it: the lock is on a file no longer at the path, so
/// it starts over.
#[cfg(unix)]
#[test]
fn a_sidecar_deleted_before_it_was_locked_is_locked_again() {
    let dir = TempDir::new("sidecar-deleted");
    let mut attempts = Vec::new();
    let sidecar = lock_at(&dir.sidecar(), &options(&dir.design()), |attempt| {
        attempts.push(attempt);
        if attempt == 0 {
            std::fs::remove_file(dir.sidecar()).unwrap();
        }
    })
    .unwrap();
    assert_eq!(attempts, [0, 1]);
    assert!(still_at(sidecar.as_file(), &dir.sidecar()).unwrap());
    // The lock is on the file at the path, so it keeps others out.
    assert_eq!(
        lock_at(&dir.sidecar(), &options(&dir.design()), |_| {}).unwrap_err(),
        ReadOnly::InUse
    );
}

/// As above, with another editor already having created a new one.
#[cfg(unix)]
#[test]
fn a_sidecar_replaced_before_it_was_locked_is_locked_again() {
    let dir = TempDir::new("sidecar-replaced");
    let mut attempts = 0;
    let sidecar = lock_at(&dir.sidecar(), &options(&dir.design()), |attempt| {
        attempts += 1;
        if attempt == 0 {
            std::fs::remove_file(dir.sidecar()).unwrap();
            std::fs::write(dir.sidecar(), "").unwrap();
        }
    })
    .unwrap();
    assert_eq!(attempts, 2);
    assert!(still_at(sidecar.as_file(), &dir.sidecar()).unwrap());
}

#[cfg(unix)]
#[test]
fn a_sidecar_that_keeps_being_replaced_gives_up() {
    let dir = TempDir::new("sidecar-gives-up");
    let error = lock_at(&dir.sidecar(), &options(&dir.design()), |_| {
        std::fs::remove_file(dir.sidecar()).unwrap();
    })
    .unwrap_err();
    assert!(matches!(error, ReadOnly::NoLock(_)), "{error:?}");
}

#[cfg(unix)]
#[test]
fn a_read_only_directory_gives_no_lock() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("sidecar-read-only");
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root writes anyway, so there's nothing to test then.
    let writable = std::fs::write(dir.0.join("probe"), "").is_ok();
    let result = lock(&dir.0.join("doc.vrdp"));
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    if !writable {
        let error = result.unwrap_err();
        assert!(
            matches!(&error, ReadOnly::NoLock(reason) if reason.contains(".doc.vrdp.autosave")),
            "{error:?}"
        );
    }
}

/// The sidecar holds the document's content, so it's no more readable
/// than the document.
#[cfg(unix)]
#[test]
fn the_sidecar_gets_the_permissions_of_the_document() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("sidecar-mode");
    let design = dir.design();
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    for document in [0o600, 0o640] {
        std::fs::set_permissions(&design, std::fs::Permissions::from_mode(document)).unwrap();
        let sidecar = lock(&design).unwrap();
        let mode = mode(&dir.sidecar());
        // The umask may take bits away, never add them; owner access is
        // what any umask in use leaves.
        assert_eq!(mode & !document, 0, "{mode:o}");
        assert_eq!(mode & 0o600, 0o600, "{mode:o}");
        sidecar.end(Ending::Close).unwrap();
    }
}

/// A read-only document still gets a sidecar its editor can write, and
/// that a later one can lock again to recover from a crash.
#[cfg(unix)]
#[test]
fn a_read_only_document_gets_a_writable_sidecar() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("sidecar-read-only-document");
    let design = dir.design();
    std::fs::set_permissions(&design, std::fs::Permissions::from_mode(0o444)).unwrap();
    let mut sidecar = lock(&design).unwrap();
    sidecar
        .append(None, &Arc::new(Document::default()), Origin::Edited)
        .unwrap();
    drop(sidecar);
    let mut sidecar = lock(&design).unwrap();
    assert_eq!(read(&mut sidecar), Some(Document::default()));
    sidecar.end(Ending::Close).unwrap();
}
