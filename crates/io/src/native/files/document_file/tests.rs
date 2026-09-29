use super::*;
use crate::tests::{TempDir, with_sketch_named};
use crate::vrdp::to_bytes;

/// The design in `dir`, not created yet.
fn doc_path(dir: &TempDir) -> PathBuf {
    dir.0.join("doc.vrdp")
}

/// A document told apart by `n`.
fn edited(n: usize) -> Document {
    with_sketch_named(&format!("Edit {n}"))
}

#[test]
fn save_and_reopen() {
    let dir = TempDir::new("reopen");
    let mut file = DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
    file.save(&edited(1)).unwrap();
    file.save(&edited(2)).unwrap();

    let (mut file, doc) = DocumentFile::open(doc_path(&dir)).unwrap();
    assert_eq!(doc, edited(2));
    file.save(&edited(3)).unwrap();
    assert_eq!(DocumentFile::open(doc_path(&dir)).unwrap().1, edited(3));
}

#[test]
fn create_does_not_overwrite() {
    let dir = TempDir::new("create");
    DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
    assert!(matches!(
        DocumentFile::create(doc_path(&dir), &edited(1)),
        Err(Error::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists
    ));
}

#[test]
fn concurrent_save_conflicts() {
    let dir = TempDir::new("conflict");
    DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
    let (mut a, _) = DocumentFile::open(doc_path(&dir)).unwrap();
    let (mut b, _) = DocumentFile::open(doc_path(&dir)).unwrap();

    a.save(&edited(1)).unwrap();
    assert!(matches!(b.save(&edited(2)), Err(Error::Conflict)));
    assert_eq!(DocumentFile::open(doc_path(&dir)).unwrap().1, edited(1));
}

#[test]
fn replaced_file_conflicts() {
    let dir = TempDir::new("replaced");
    let mut file = DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
    std::fs::remove_file(doc_path(&dir)).unwrap();
    DocumentFile::create(doc_path(&dir), &edited(1)).unwrap();
    assert!(matches!(file.save(&edited(2)), Err(Error::Conflict)));
}

/// A file another handle keeps locked, e.g. a lock file, is refused after a
/// while rather than waited on forever.
#[test]
fn a_file_locked_for_good_is_refused() {
    let dir = TempDir::new("locked");
    DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
    let holder = File::open(doc_path(&dir)).unwrap();
    holder.lock().unwrap();
    let start = Instant::now();
    assert!(matches!(
        DocumentFile::open(doc_path(&dir)),
        Err(Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock
    ));
    assert!(start.elapsed() >= LOCK_WAIT);

    holder.unlock().unwrap();
    DocumentFile::open(doc_path(&dir)).unwrap();
}

/// A lock let go of while waiting is taken.
#[test]
fn a_file_locked_for_a_moment_opens() {
    let dir = TempDir::new("locked-moment");
    DocumentFile::create(doc_path(&dir), &edited(1)).unwrap();
    let holder = File::open(doc_path(&dir)).unwrap();
    holder.lock().unwrap();
    let unlock = std::thread::spawn(move || {
        std::thread::sleep(LOCK_WAIT / 4);
        holder.unlock().unwrap();
    });
    let (_, document) = DocumentFile::open(doc_path(&dir)).unwrap();
    assert_eq!(document, edited(1));
    unlock.join().unwrap();
}

/// A save waits for another program's lock only for a while, like opening.
#[test]
fn a_save_locked_out_for_good_is_refused() {
    let dir = TempDir::new("save-locked");
    let mut file = DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
    let holder = File::open(doc_path(&dir)).unwrap();
    holder.lock_shared().unwrap();
    let start = Instant::now();
    assert!(matches!(
        file.save(&edited(1)),
        Err(Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock
    ));
    assert!(start.elapsed() >= LOCK_WAIT);

    // Nothing was written, so the next save goes through.
    holder.unlock().unwrap();
    file.save(&edited(2)).unwrap();
    assert_eq!(DocumentFile::open(doc_path(&dir)).unwrap().1, edited(2));
}

#[test]
fn replace_writes_a_new_file_or_over_an_old_one() {
    let dir = TempDir::new("replace");
    let mut file = DocumentFile::replace(doc_path(&dir), &edited(1)).unwrap();
    assert_eq!(file.path(), doc_path(&dir));
    assert_eq!(DocumentFile::open(doc_path(&dir)).unwrap().1, edited(1));
    file.save(&edited(2)).unwrap();

    let (mut old, _) = DocumentFile::open(doc_path(&dir)).unwrap();
    let mut new = DocumentFile::replace(doc_path(&dir), &edited(3)).unwrap();
    assert_eq!(DocumentFile::open(doc_path(&dir)).unwrap().1, edited(3));
    // Only the new record is in it, and the new handle saves on.
    assert_eq!(
        std::fs::read(doc_path(&dir)).unwrap(),
        to_bytes(&edited(3)).unwrap().0
    );
    assert_ne!(new.tail(), old.tail());
    new.save(&edited(4)).unwrap();
    assert!(matches!(old.save(&edited(5)), Err(Error::Conflict)));
    // No temporary file is left behind.
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn replace_keeps_the_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("replace-mode");
    DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
    std::fs::set_permissions(doc_path(&dir), std::fs::Permissions::from_mode(0o640)).unwrap();
    DocumentFile::replace(doc_path(&dir), &edited(1)).unwrap();
    let mode = std::fs::metadata(doc_path(&dir))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o640);
}

/// A temporary name already taken, by another instance saving to the same
/// directory (a shared drive, where process ids repeat) or one that crashed
/// mid-replace: it's left alone, and another name is used.
#[test]
fn replace_skips_a_temporary_name_that_is_taken() {
    let dir = TempDir::new("replace-taken");
    DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
    let taken = dir.0.join(".doc.vrdp.taken.tmp");
    std::fs::write(&taken, "someone else's").unwrap();
    let mut names = [taken.clone(), dir.0.join(".doc.vrdp.free.tmp")].into_iter();
    let mut temps = || Ok(names.next().expect("asked for too many names"));
    let hooks = Hooks {
        temps: Some(&mut temps),
        ..Hooks::default()
    };
    DocumentFile::replace_with(doc_path(&dir), &edited(1), hooks).unwrap();
    assert_eq!(DocumentFile::open(doc_path(&dir)).unwrap().1, edited(1));
    assert_eq!(std::fs::read(&taken).unwrap(), b"someone else's");
}

/// A replace that fails once the temporary file is there removes it and
/// leaves the old file as it was.
#[test]
fn a_failed_replace_leaves_no_temporary_file() {
    let dir = TempDir::new("replace-fails");
    DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
    let hooks = Hooks {
        created: Some(&mut |_| Err(io::Error::other("disk full"))),
        ..Hooks::default()
    };
    let failed = DocumentFile::replace_with(doc_path(&dir), &edited(1), hooks);
    assert!(failed.is_err());
    assert_eq!(DocumentFile::open(doc_path(&dir)).unwrap().1, edited(0));
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
}

/// A replace whose directory sync fails after the rename still hands back
/// the new file, which saves on rather than finding itself a conflict.
#[test]
fn a_failed_directory_sync_after_replace_keeps_the_new_file() {
    let dir = TempDir::new("replace-dir-sync-fails");
    DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
    let hooks = Hooks {
        sync_dir: Some(&mut |_| Err(io::Error::other("fsync failed"))),
        ..Hooks::default()
    };
    let mut file = DocumentFile::replace_with(doc_path(&dir), &edited(1), hooks).unwrap();
    assert_eq!(file.path(), doc_path(&dir));
    assert_eq!(DocumentFile::open(doc_path(&dir)).unwrap().1, edited(1));
    file.save(&edited(2)).unwrap();
    assert_eq!(DocumentFile::open(doc_path(&dir)).unwrap().1, edited(2));
}

/// A create that fails part way, e.g. on a full disk, leaves nothing at
/// the path: no damaged design, and no file making the next try fail as
/// already there.
#[test]
fn a_failed_create_leaves_no_file() {
    let dir = TempDir::new("create-fails");
    let mut hooks = Hooks {
        created: Some(&mut |_| Err(io::Error::other("disk full"))),
        ..Hooks::default()
    };
    let failed = DocumentFile::create_with(doc_path(&dir), &edited(0), None, &mut hooks);
    assert!(failed.is_err());
    assert!(!doc_path(&dir).exists());
    DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
}

/// The replacing file never has looser permissions than the design it
/// replaces, not even while it's being written.
#[cfg(unix)]
#[test]
fn replace_writes_with_the_permissions_from_the_start() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("replace-mode-early");
    DocumentFile::create(doc_path(&dir), &edited(0)).unwrap();
    std::fs::set_permissions(doc_path(&dir), std::fs::Permissions::from_mode(0o600)).unwrap();
    let hooks = Hooks {
        created: Some(&mut |temp| {
            let mode = std::fs::metadata(temp)?.permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
            Ok(())
        }),
        ..Hooks::default()
    };
    DocumentFile::replace_with(doc_path(&dir), &edited(1), hooks).unwrap();
    let mode = std::fs::metadata(doc_path(&dir))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

/// Temporary files are hidden next to the file they replace, named with a
/// fresh name each.
#[test]
fn temporary_paths_are_hidden_next_to_the_file() {
    assert_eq!(
        temp_path(Path::new("dir/doc.vrdp"), "name").unwrap(),
        Path::new("dir/.doc.vrdp.name.tmp")
    );
    assert!(temp_path(Path::new("/"), "name").is_err());
}
