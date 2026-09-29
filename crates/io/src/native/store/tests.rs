use std::sync::Arc;

use varde_document::Document;

use super::*;
use crate::autosave::{AutoSaved, Origin};
use crate::tests::TempDir;
use crate::vrdp::HeldFile;

#[test]
fn entries_are_new_locked_and_private() {
    let dir = TempDir::new("store-create");
    let designs = dir.0.join("designs");
    let first = create(&designs).unwrap();
    let second = create(&designs).unwrap();
    assert_eq!(first.as_file().metadata().unwrap().len(), 0);
    let paths: Vec<_> = std::fs::read_dir(&designs)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(paths.len(), 2);
    for path in &paths {
        assert_eq!(path.extension().unwrap(), "vrdp");
        assert!(matches!(open(&designs, path), Err(e) if e.contains("open elsewhere")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "{mode:o}");
        }
    }
    // Held ones aren't listed.
    assert_eq!(list(&designs), []);
    first.end(Ending::Close).unwrap();
    second.end(Ending::Close).unwrap();
    assert_eq!(std::fs::read_dir(&designs).unwrap().count(), 0);
}

/// A file system that can't lock gives its error, after one attempt, and
/// leaves no entry behind.
#[test]
fn a_lock_error_gives_up_and_leaves_nothing() {
    let dir = TempDir::new("store-lock-error");
    let mut attempts = 0;
    let error = create_with(&dir.0, |_| {
        attempts += 1;
        Err(TryLockError::Error(io::Error::other("no locks available")))
    })
    .unwrap_err();
    assert_eq!(attempts, 1);
    assert_eq!(error.to_string(), "no locks available");
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
}

/// An entry someone listing entries holds gets a new name.
#[test]
fn an_entry_held_while_made_gets_a_new_name() {
    let dir = TempDir::new("store-held");
    let mut attempts = 0;
    let entry = create_with(&dir.0, |file| {
        attempts += 1;
        if attempts == 1 {
            Err(TryLockError::WouldBlock)
        } else {
            file.try_lock()
        }
    })
    .unwrap();
    assert_eq!(attempts, 2);
    assert!(matches!(open(&dir.0, entry.path()), Err(e) if e.contains("open elsewhere")));
}

/// Entries left behind by a crash are listed if there's a design in them,
/// and deleted if there's nothing; damaged ones are left alone.
#[test]
fn list_finds_entries_left_behind() {
    let dir = TempDir::new("store-list");
    let mut saved = create(&dir.0).unwrap();
    saved
        .append(None, &Arc::new(Document::example()), Origin::Edited)
        .unwrap();
    let saved_path = saved.path().to_owned();
    drop(saved);
    let empty = create(&dir.0).unwrap();
    let empty_path = empty.path().to_owned();
    drop(empty);
    let damaged = dir.0.join("damaged.vrdp");
    std::fs::write(&damaged, "not a design").unwrap();
    std::fs::write(dir.0.join("other.txt"), "not an entry").unwrap();
    let held = create(&dir.0).unwrap();

    let listed = list(&dir.0);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].path, saved_path);
    assert!(listed[0].modified.is_some());
    assert_eq!(listed[0].name, None);
    assert!(!empty_path.exists());
    assert!(damaged.exists());
    assert!(held.path().exists());

    // Opening one takes it over, so it isn't listed again meanwhile.
    let mut opened = open(&dir.0, &saved_path).unwrap();
    assert_eq!(
        opened.read().unwrap().map(|saved| saved.document),
        Some(Arc::new(Document::example()))
    );
    assert_eq!(list(&dir.0), []);
    drop(opened);
    assert_eq!(list(&dir.0).len(), 1);
    assert_eq!(list(&dir.0.join("missing")), []);
}

#[test]
fn discard_deletes_only_entries_nobody_holds() {
    let dir = TempDir::new("store-discard");
    let designs = dir.0.join("designs");
    let mut left = create(&designs).unwrap();
    left.append(None, &Arc::new(Document::example()), Origin::Edited)
        .unwrap();
    let path = left.path().to_owned();
    // Held.
    assert!(discard(&designs, &path).is_err());
    drop(left);

    // Not in the store.
    let design = dir.design();
    assert!(discard(&designs, &design).is_err());
    assert!(design.exists());
    let text = designs.join("notes.txt");
    std::fs::write(&text, "").unwrap();
    assert!(discard(&designs, &text).is_err());
    assert!(text.exists());

    discard(&designs, &path).unwrap();
    assert!(!path.exists());
    assert!(discard(&designs, &path).is_err());
}

/// An entry holding the auto-saves of a design opened from a file of the
/// user's, as on the web, is listed by the file's name.
#[test]
fn an_entry_is_listed_by_the_name_it_holds() {
    let dir = TempDir::new("store-name");
    let path = dir.0.join("named.vrdp");
    let file = options(true).open(&path).unwrap();
    HeldFile::<_, AutoSaved>::new(file)
        .append(&AutoSaved {
            base: None,
            name: Some("bracket.vrdp".to_owned()),
            document: Arc::new(Document::example()),
            origin: Origin::Edited,
        })
        .unwrap();
    let listed = list(&dir.0);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].path, path);
    assert_eq!(listed[0].name.as_deref(), Some("bracket.vrdp"));
    assert!(!listed[0].downloaded);
}

/// An entry is listed as downloaded only while its newest record is the
/// design as downloaded: a later auto-save makes it changes never saved.
#[test]
fn an_entry_is_listed_as_downloaded_by_its_newest_record() {
    let dir = TempDir::new("store-downloaded");
    let mut entry = create(&dir.0).unwrap();
    entry
        .append(None, &Arc::new(Document::default()), Origin::Edited)
        .unwrap();
    entry
        .append(None, &Arc::new(Document::example()), Origin::Downloaded)
        .unwrap();
    drop(entry);
    let listed = list(&dir.0);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert!(listed[0].downloaded);

    let mut entry = open(&dir.0, &listed[0].path).unwrap();
    entry
        .append(None, &Arc::new(Document::default()), Origin::Edited)
        .unwrap();
    drop(entry);
    let listed = list(&dir.0);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert!(!listed[0].downloaded);
}
