use std::sync::Arc;

use varde_document::Document;

use super::*;
use crate::native::sidecar::sidecar_path;
use crate::tests::{TempDir, auto_saved_at, with_sketch_named};
use crate::{Access, Offer, Picked, PickedFrom};

fn open(files: &mut Files, path: &Path) -> Result<Opened, String> {
    match files.handle(Request::Open {
        id: OpenId(7),
        from: Chosen::Path(path.to_owned()),
    }) {
        Response::Opened { id, result, .. } => {
            assert_eq!(id, OpenId(7));
            result
        }
        response => panic!("unexpected {response:?}"),
    }
}

fn close(files: &mut Files, file: FileId) -> Result<(), String> {
    match files.handle(Request::Close {
        file,
        closing: Closing::Clean,
    }) {
        Response::Closed {
            file: closed,
            result,
        } => {
            assert_eq!(closed, file);
            result
        }
        response => panic!("unexpected {response:?}"),
    }
}

#[test]
fn opens_the_document_for_editing() {
    let dir = TempDir::new("files-open");
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    assert_eq!(opened.document, Document::example());
    assert_eq!(opened.access, Access::Edit);
    assert!(sidecar_path(&dir.design()).unwrap().exists());
}

#[test]
fn a_second_open_is_read_only_until_the_first_closes() {
    let dir = TempDir::new("files-twice");
    let mut files = Files::new(Stores::default());
    let first = open(&mut files, &dir.design()).unwrap();
    let second = open(&mut files, &dir.design()).unwrap();
    assert_eq!(second.access, Access::ReadOnly(ReadOnly::InUse));
    assert_eq!(second.document, Document::example());
    assert_ne!(first.file, second.file);

    // Closing the read-only one leaves the lock alone.
    close(&mut files, second.file).unwrap();
    let sidecar = sidecar_path(&dir.design()).unwrap();
    assert!(sidecar.exists());
    let third = open(&mut files, &dir.design()).unwrap();
    assert_eq!(third.access, Access::ReadOnly(ReadOnly::InUse));
    close(&mut files, third.file).unwrap();

    close(&mut files, first.file).unwrap();
    assert!(!sidecar.exists());
    let fourth = open(&mut files, &dir.design()).unwrap();
    assert_eq!(fourth.access, Access::Edit);
}

#[test]
fn a_file_that_does_not_open_gets_no_sidecar() {
    let dir = TempDir::new("files-missing");
    let mut files = Files::new(Stores::default());
    let missing = dir.0.join("missing.vrdp");
    assert!(open(&mut files, &missing).is_err());
    std::fs::write(dir.0.join("text.vrdp"), "not a design").unwrap();
    assert!(open(&mut files, &dir.0.join("text.vrdp")).is_err());
    assert!(!sidecar_path(&missing).unwrap().exists());
    assert!(!sidecar_path(&dir.0.join("text.vrdp")).unwrap().exists());
}

#[test]
fn closing_twice_fails_the_second_time() {
    let dir = TempDir::new("files-close-twice");
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    close(&mut files, opened.file).unwrap();
    assert!(close(&mut files, opened.file).is_err());
}

#[test]
fn close_all_removes_the_sidecars() {
    let dir = TempDir::new("files-close-all");
    let mut files = Files::new(Stores::default());
    open(&mut files, &dir.design()).unwrap();
    files.close_all();
    assert!(!sidecar_path(&dir.design()).unwrap().exists());
}

#[cfg(unix)]
#[test]
fn a_read_only_directory_opens_read_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("files-read-only");
    let design = dir.design();
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root writes anyway, so there's nothing to test then.
    let writable = std::fs::write(dir.0.join("probe"), "").is_ok();
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &design);
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    let opened = opened.unwrap();
    if !writable {
        assert!(
            matches!(&opened.access, Access::ReadOnly(ReadOnly::NoLock(_))),
            "{:?}",
            opened.access
        );
    }
    assert_eq!(opened.document, Document::example());
}

#[test]
fn recent_files_round_trip_through_the_store() {
    let dir = TempDir::new("files-recent");
    let mut files = Files::new(Stores {
        recent: Some(dir.0.join("recent.toml")),
        settings: None,
        designs: None,
        panic: None,
    });
    let entries = vec![RecentFile {
        path: dir.design(),
        opened: crate::UnixSeconds(5),
    }];
    let response = files.handle(Request::WriteRecent {
        entries: entries.clone(),
    });
    assert!(matches!(
        response,
        Response::RecentWritten { result: Ok(()) }
    ));
    let Response::RecentLoaded {
        entries: loaded, ..
    } = files.handle(Request::LoadRecent)
    else {
        panic!("not the recent files");
    };
    let entries: Vec<_> = entries
        .into_iter()
        .map(|entry| crate::recent::Listed {
            entry,
            available: true,
        })
        .collect();
    assert_eq!(loaded, entries);
}

#[test]
fn recent_files_without_a_store_are_empty() {
    let mut files = Files::new(Stores::default());
    assert!(matches!(
        files.handle(Request::WriteRecent { entries: vec![] }),
        Response::RecentWritten { result: Ok(()) }
    ));
    assert!(matches!(
        files.handle(Request::LoadRecent),
        Response::RecentLoaded { entries, .. } if entries.is_empty()
    ));
}

#[test]
fn the_panic_recorded_loads_and_is_discarded() {
    let dir = TempDir::new("files-panic");
    let store = dir.0.join("panic.toml");
    let mut files = Files::new(Stores {
        panic: Some(store.clone()),
        ..Stores::default()
    });
    assert!(matches!(
        files.handle(Request::LoadPanic),
        Response::PanicLoaded { panic: None }
    ));
    let panic = crate::Panic::new(None, "on purpose", None, None);
    std::fs::write(&store, panic.serialize()).unwrap();
    assert!(matches!(
        files.handle(Request::LoadPanic),
        Response::PanicLoaded { panic: Some(loaded) } if loaded == panic
    ));
    assert!(matches!(
        files.handle(Request::DiscardPanic { panic }),
        Response::PanicDiscarded { result: Ok(()) }
    ));
    assert!(!store.exists());
    // Nowhere to keep one.
    let mut files = Files::new(Stores::default());
    assert!(matches!(
        files.handle(Request::LoadPanic),
        Response::PanicLoaded { panic: None }
    ));
}

#[test]
fn settings_round_trip_through_the_store() {
    let dir = TempDir::new("files-settings");
    let mut files = Files::new(Stores {
        settings: Some(dir.0.join("settings.toml")),
        ..Stores::default()
    });
    assert!(matches!(
        files.handle(Request::LoadSettings),
        Response::SettingsLoaded { settings } if settings == Settings::default()
    ));
    let settings = Settings {
        theme: crate::settings::Theme::Light,
    };
    assert!(matches!(
        files.handle(Request::WriteSettings { settings }),
        Response::SettingsWritten { result: Ok(()) }
    ));
    assert!(matches!(
        files.handle(Request::LoadSettings),
        Response::SettingsLoaded { settings: loaded } if loaded == settings
    ));
}

#[test]
fn settings_without_a_store_are_the_defaults() {
    let mut files = Files::new(Stores::default());
    let settings = Settings {
        theme: crate::settings::Theme::Dark,
    };
    assert!(matches!(
        files.handle(Request::WriteSettings { settings }),
        Response::SettingsWritten { result: Ok(()) }
    ));
    assert!(matches!(
        files.handle(Request::LoadSettings),
        Response::SettingsLoaded { settings } if settings == Settings::default()
    ));
}

/// Two paths to one design, through a symbolic link, share one lock.
#[cfg(unix)]
#[test]
fn a_link_to_an_open_design_is_read_only() {
    let dir = TempDir::new("files-link");
    let design = dir.design();
    let link = dir.0.join("link.vrdp");
    std::os::unix::fs::symlink(&design, &link).unwrap();
    let mut files = Files::new(Stores::default());
    let first = open(&mut files, &design).unwrap();
    assert_eq!(first.access, Access::Edit);
    let second = open(&mut files, &link).unwrap();
    assert_eq!(second.access, Access::ReadOnly(ReadOnly::InUse));
    assert!(!sidecar_path(&link).unwrap().exists());
}

/// The UI gives up on opens it asked for and asks again: the ones it gave
/// up on let go of their locks before the next open is handled.
#[test]
fn an_abandoned_open_lets_go_of_the_lock() {
    let dir = TempDir::new("files-abandon");
    let other = dir.0.join("other.vrdp");
    DocumentFile::create(&other, &Document::example(), &[]).unwrap();
    let mut files = Files::new(Stores::default());
    let requests = [
        Request::Open {
            id: OpenId(0),
            from: Chosen::Path(dir.design()),
        },
        Request::Open {
            id: OpenId(1),
            from: Chosen::Path(other),
        },
        Request::Abandon { id: OpenId(0) },
        Request::Abandon { id: OpenId(1) },
        // Never opened.
        Request::Abandon { id: OpenId(5) },
    ];
    for request in requests {
        let response = files.handle(request);
        if let Response::Abandoned { result, .. } = response {
            result.unwrap();
        }
    }
    let again = open(&mut files, &dir.design()).unwrap();
    assert_eq!(again.access, Access::Edit);
    assert!(!sidecar_path(&dir.0.join("other.vrdp")).unwrap().exists());
}

/// The changes offered as recovered on opening, if any.
fn offered(opened: &Opened) -> Option<&Document> {
    opened
        .recovered
        .as_ref()
        .ok()?
        .as_ref()
        .map(|offer| &offer.document)
}

/// A document other than the example, as an edit would leave it.
fn edited() -> Arc<Document> {
    let edited = Arc::new(crate::tests::with_sketches(1));
    assert_ne!(*edited, Document::example());
    edited
}

fn save(files: &mut Files, file: FileId, document: Arc<Document>) -> Result<(), SaveError> {
    match files.handle(Request::Save {
        file,
        revision: 4.into(),
        document,
        thumbnail: None,
    }) {
        Response::Saved {
            file: saved,
            revision,
            result,
        } if revision == 4.into() => {
            assert_eq!(saved, file);
            result
        }
        response => panic!("unexpected {response:?}"),
    }
}

fn save_as(
    files: &mut Files,
    file: Option<FileId>,
    path: &Path,
    overwrite: bool,
) -> Result<SavedAs, SaveError> {
    match files.handle(Request::SaveAs {
        file,
        to: SaveTo::Path {
            path: path.to_owned(),
            overwrite,
        },
        revision: 9.into(),
        document: edited(),
        thumbnail: None,
    }) {
        Response::SavedAs {
            file: asked,
            to: Chosen::Path(saved),
            revision,
            result,
        } if revision == 9.into() => {
            assert_eq!(asked, file);
            assert_eq!(saved, path);
            result
        }
        response => panic!("unexpected {response:?}"),
    }
}

#[test]
fn a_save_shows_on_reopening() {
    let dir = TempDir::new("files-save");
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    save(&mut files, opened.file, edited()).unwrap();
    close(&mut files, opened.file).unwrap();
    let again = open(&mut files, &dir.design()).unwrap();
    assert_eq!(again.document, *edited());
}

/// The thumbnails `files` finds for `paths`.
fn thumbnails(files: &mut Files, paths: &[PathBuf]) -> Vec<(PathBuf, Image)> {
    match files.handle(Request::LoadThumbnails {
        paths: paths.to_vec(),
    }) {
        Response::ThumbnailsLoaded { thumbnails } => thumbnails,
        response => panic!("unexpected {response:?}"),
    }
}

#[test]
fn saves_write_their_thumbnail_for_the_list_to_read() {
    let dir = TempDir::new("files-thumbnail");
    let mut files = Files::new(Stores::default());
    let design = dir.design();
    let other = dir.0.join("other.vrdp");
    let missing = dir.0.join("missing.vrdp");
    let paths = [design.clone(), other.clone(), missing];
    // Written as the example, with no thumbnail.
    assert_eq!(thumbnails(&mut files, &paths), []);

    let image = |shade| Image::new(2, 1, vec![shade, 2, 3, 255, 4, 5, 6, 128]).unwrap();
    let opened = open(&mut files, &design).unwrap();
    let saved = files.handle(Request::Save {
        file: opened.file,
        revision: 4.into(),
        document: edited(),
        thumbnail: Some(image(1)),
    });
    assert!(
        matches!(saved, Response::Saved { result: Ok(()), .. }),
        "{saved:?}"
    );
    let saved_as = files.handle(Request::SaveAs {
        file: None,
        to: SaveTo::Path {
            path: other.clone(),
            overwrite: false,
        },
        revision: 9.into(),
        document: edited(),
        thumbnail: Some(image(7)),
    });
    assert!(
        matches!(saved_as, Response::SavedAs { result: Ok(_), .. }),
        "{saved_as:?}"
    );
    // Read while they're open, in the order asked.
    let found = thumbnails(&mut files, &paths);
    assert_eq!(found, [(design.clone(), image(1)), (other, image(7))]);

    // A save without one drops the one before.
    save(&mut files, opened.file, edited()).unwrap();
    assert_eq!(thumbnails(&mut files, std::slice::from_ref(&design)), []);
    // And the design opens as saved.
    close(&mut files, opened.file).unwrap();
    assert_eq!(open(&mut files, &design).unwrap().document, *edited());
}

#[test]
fn a_save_over_changes_made_elsewhere_is_a_conflict() {
    let dir = TempDir::new("files-conflict");
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    let (mut other, _, _) = DocumentFile::open(dir.design()).unwrap();
    other.save(&edited(), &[]).unwrap();
    assert_eq!(
        save(&mut files, opened.file, Arc::new(Document::example())),
        Err(SaveError::Conflict)
    );
    assert_eq!(DocumentFile::open(dir.design()).unwrap().1, *edited());
}

#[test]
fn a_read_only_design_is_not_saved() {
    let dir = TempDir::new("files-save-read-only");
    let mut files = Files::new(Stores::default());
    let _first = open(&mut files, &dir.design()).unwrap();
    let second = open(&mut files, &dir.design()).unwrap();
    assert!(save(&mut files, second.file, edited()).is_err());
    assert!(save(&mut files, FileId(99), edited()).is_err());
    assert_eq!(
        DocumentFile::open(dir.design()).unwrap().1,
        Document::example()
    );
}

#[test]
fn save_as_of_a_new_design_creates_and_locks_the_file() {
    let dir = TempDir::new("files-save-as-new");
    let mut files = Files::new(Stores::default());
    let path = dir.0.join("new.vrdp");
    let saved = save_as(&mut files, None, &path, false).unwrap();
    assert_eq!(saved.access, Access::Edit);
    assert_eq!(DocumentFile::open(&path).unwrap().1, *edited());
    let sidecar = sidecar_path(&path).unwrap();
    assert!(sidecar.exists());
    // Locked: a second open is read-only.
    let other = open(&mut files, &path).unwrap();
    assert_eq!(other.access, Access::ReadOnly(ReadOnly::InUse));
    close(&mut files, other.file).unwrap();

    // And saved to from then on.
    save(&mut files, saved.file, Arc::new(Document::example())).unwrap();
    assert_eq!(DocumentFile::open(&path).unwrap().1, Document::example());
    close(&mut files, saved.file).unwrap();
    assert!(!sidecar.exists());
}

#[test]
fn save_as_moves_the_lock_to_the_new_file() {
    let dir = TempDir::new("files-save-as-moves");
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    let path = dir.0.join("copy.vrdp");
    let saved = save_as(&mut files, Some(opened.file), &path, false).unwrap();
    assert_eq!(saved.file, opened.file);
    assert_eq!(saved.access, Access::Edit);

    // The old design is let go of, its lock file gone and editable again.
    assert!(!dir.sidecar().exists());
    let old = open(&mut files, &dir.design()).unwrap();
    assert_eq!(old.access, Access::Edit);
    assert_eq!(old.document, Document::example());
    // The new one is held.
    let new = open(&mut files, &path).unwrap();
    assert_eq!(new.access, Access::ReadOnly(ReadOnly::InUse));
    assert_eq!(new.document, *edited());

    // Saves go to the new file.
    save(&mut files, opened.file, Arc::new(Document::example())).unwrap();
    assert_eq!(DocumentFile::open(&path).unwrap().1, Document::example());
}

#[test]
fn save_as_over_a_design_open_elsewhere_is_refused() {
    let dir = TempDir::new("files-save-as-locked");
    let mut files = Files::new(Stores::default());
    let other = dir.0.join("other.vrdp");
    DocumentFile::create(&other, &Document::example(), &[]).unwrap();
    let elsewhere = sidecar::lock(&other).unwrap();
    let opened = open(&mut files, &dir.design()).unwrap();

    let error = save_as(&mut files, Some(opened.file), &other, true).unwrap_err();
    assert_eq!(
        error,
        SaveError::Failed("other.vrdp is open elsewhere".to_owned())
    );
    assert_eq!(DocumentFile::open(&other).unwrap().1, Document::example());
    // The old file is kept, lock and all.
    assert_eq!(
        open(&mut files, &dir.design()).unwrap().access,
        Access::ReadOnly(ReadOnly::InUse)
    );
    save(&mut files, opened.file, edited()).unwrap();
    assert_eq!(DocumentFile::open(dir.design()).unwrap().1, *edited());
    drop(elsewhere);
}

#[test]
fn save_as_replaces_a_file_only_if_asked_to() {
    let dir = TempDir::new("files-save-as-replace");
    let mut files = Files::new(Stores::default());
    let other = dir.0.join("other.vrdp");
    std::fs::write(&other, "something else").unwrap();
    let error = save_as(&mut files, None, &other, false).unwrap_err();
    assert_eq!(
        error,
        SaveError::Failed("other.vrdp already exists".to_owned())
    );
    assert_eq!(std::fs::read(&other).unwrap(), b"something else");
    // The lock taken for it is let go of.
    assert!(!sidecar_path(&other).unwrap().exists());

    save_as(&mut files, None, &other, true).unwrap();
    assert_eq!(DocumentFile::open(&other).unwrap().1, *edited());
}

/// Save As onto the design's own file keeps its lock rather than finding
/// it taken by itself.
#[test]
fn save_as_over_itself_keeps_the_lock() {
    let dir = TempDir::new("files-save-as-itself");
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    let saved = save_as(&mut files, Some(opened.file), &dir.design(), true).unwrap();
    assert_eq!(saved.access, Access::Edit);
    assert_eq!(DocumentFile::open(dir.design()).unwrap().1, *edited());
    assert_eq!(
        open(&mut files, &dir.design()).unwrap().access,
        Access::ReadOnly(ReadOnly::InUse)
    );
    save(&mut files, opened.file, Arc::new(Document::example())).unwrap();
}

/// A design open read-only because another editor has it can be saved as
/// a copy, which is editable.
#[test]
fn save_as_of_a_read_only_design_makes_it_editable() {
    let dir = TempDir::new("files-save-as-read-only");
    let mut files = Files::new(Stores::default());
    let _first = open(&mut files, &dir.design()).unwrap();
    let second = open(&mut files, &dir.design()).unwrap();
    let copy = dir.0.join("copy.vrdp");
    let saved = save_as(&mut files, Some(second.file), &copy, false).unwrap();
    assert_eq!(saved.access, Access::Edit);
    save(&mut files, second.file, Arc::new(Document::example())).unwrap();
    assert_eq!(DocumentFile::open(&copy).unwrap().1, Document::example());
    // The first editor's lock is untouched.
    assert!(dir.sidecar().exists());
}

/// Other spellings of a path find the same lock: Save As through a link
/// to a design open elsewhere is refused, and Save As onto the design's
/// own file through a link or `..` keeps its lock.
#[cfg(unix)]
#[test]
fn save_as_sees_through_other_spellings_of_a_path() {
    let dir = TempDir::new("files-save-as-spellings");
    let mut files = Files::new(Stores::default());
    let other = dir.0.join("other.vrdp");
    DocumentFile::create(&other, &Document::example(), &[]).unwrap();
    let elsewhere = sidecar::lock(&other).unwrap();
    let link = dir.0.join("link.vrdp");
    std::os::unix::fs::symlink(&other, &link).unwrap();
    let opened = open(&mut files, &dir.design()).unwrap();
    assert!(save_as(&mut files, Some(opened.file), &link, true).is_err());
    assert_eq!(DocumentFile::open(&other).unwrap().1, Document::example());
    drop(elsewhere);

    let own = dir.0.join("own.vrdp");
    std::os::unix::fs::symlink(dir.design(), &own).unwrap();
    std::fs::create_dir(dir.0.join("sub")).unwrap();
    for path in [own, dir.0.join("sub/../doc.vrdp")] {
        let saved = save_as(&mut files, Some(opened.file), &path, true).unwrap();
        assert_eq!(saved.access, Access::Edit);
        assert!(std::fs::symlink_metadata(dir.design()).unwrap().is_file());
    }
    assert_eq!(DocumentFile::open(dir.design()).unwrap().1, *edited());
    save(&mut files, opened.file, Arc::new(Document::example())).unwrap();
}

/// After a conflict, saving keeps failing, and Save As, to a copy or over
/// the file itself, is the way on: saving works again after it.
#[test]
fn save_as_gets_past_a_conflict() {
    let dir = TempDir::new("files-conflict-save-as");
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    let (mut other, _, _) = DocumentFile::open(dir.design()).unwrap();
    other.save(&edited(), &[]).unwrap();
    for _ in 0..2 {
        assert_eq!(
            save(&mut files, opened.file, edited()),
            Err(SaveError::Conflict)
        );
    }
    let copy = dir.0.join("copy.vrdp");
    save_as(&mut files, Some(opened.file), &copy, false).unwrap();
    save(&mut files, opened.file, Arc::new(Document::example())).unwrap();
    assert_eq!(DocumentFile::open(&copy).unwrap().1, Document::example());

    other = DocumentFile::open(&copy).unwrap().0;
    other.save(&edited(), &[]).unwrap();
    assert_eq!(
        save(&mut files, opened.file, edited()),
        Err(SaveError::Conflict)
    );
    save_as(&mut files, Some(opened.file), &copy, true).unwrap();
    save(&mut files, opened.file, Arc::new(Document::example())).unwrap();
    assert_eq!(DocumentFile::open(&copy).unwrap().1, Document::example());
}

/// Files with new designs kept in `dir/designs`.
fn with_store(dir: &TempDir) -> Files {
    Files::new(Stores {
        recent: None,
        settings: None,
        designs: Some(dir.0.join("designs")),
        panic: None,
    })
}

fn auto_save(files: &mut Files, file: FileId, document: Arc<Document>) -> Result<(), String> {
    match files.handle(Request::AutoSave {
        file,
        revision: 6.into(),
        document,
    }) {
        Response::AutoSaved {
            file: saved,
            revision,
            result,
        } if revision == 6.into() => {
            assert_eq!(saved, file);
            result
        }
        response => panic!("unexpected {response:?}"),
    }
}

fn close_keeping(files: &mut Files, file: FileId) -> Result<(), String> {
    match files.handle(Request::Close {
        file,
        closing: Closing::Keep,
    }) {
        Response::Closed { result, .. } => result,
        response => panic!("unexpected {response:?}"),
    }
}

/// What's in the sidecar at `path`, read without locking it.
fn auto_saved(path: &Path) -> Option<Document> {
    auto_saved_at(path).map(|saved| Arc::unwrap_or_clone(saved.document))
}

/// A session that crashed with `document` auto-saved for the design at
/// `path`, based on the design as it is: the sidecar is left behind,
/// unlocked.
fn crashed_with(path: &Path, document: &Document) {
    let (file, _, _) = DocumentFile::open(path).unwrap();
    let mut sidecar = sidecar::lock(path).unwrap();
    sidecar
        .append(Some(file.tail()), &Arc::new(document.clone()))
        .unwrap();
    drop(sidecar);
}

#[test]
fn auto_saves_go_to_the_sidecar_and_a_save_empties_it() {
    let dir = TempDir::new("files-auto-save");
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    assert_eq!(opened.recovered, Ok(None));
    auto_save(&mut files, opened.file, edited()).unwrap();
    assert_eq!(auto_saved(&dir.sidecar()), Some((*edited()).clone()));
    // Never the design itself.
    assert_eq!(
        DocumentFile::open(dir.design()).unwrap().1,
        Document::example()
    );

    save(&mut files, opened.file, edited()).unwrap();
    assert_eq!(std::fs::metadata(dir.sidecar()).unwrap().len(), 0);
    assert_eq!(DocumentFile::open(dir.design()).unwrap().1, *edited());
    close(&mut files, opened.file).unwrap();
    assert!(!dir.sidecar().exists());
}

#[test]
fn a_read_only_design_is_not_auto_saved() {
    let dir = TempDir::new("files-auto-save-read-only");
    let mut files = Files::new(Stores::default());
    let first = open(&mut files, &dir.design()).unwrap();
    let second = open(&mut files, &dir.design()).unwrap();
    assert!(auto_save(&mut files, second.file, edited()).is_err());
    assert!(auto_save(&mut files, FileId(99), edited()).is_err());
    assert_eq!(auto_saved(&dir.sidecar()), None);
    close(&mut files, first.file).unwrap();
}

/// A clean close deletes what was auto-saved, the user having chosen not
/// to save it; closing without discarding keeps it, to be offered again.
#[test]
fn close_discards_auto_saves_only_if_asked_to() {
    let dir = TempDir::new("files-close-auto-saved");
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    auto_save(&mut files, opened.file, edited()).unwrap();
    close_keeping(&mut files, opened.file).unwrap();
    assert_eq!(auto_saved(&dir.sidecar()), Some((*edited()).clone()));

    let opened = open(&mut files, &dir.design()).unwrap();
    assert_eq!(offered(&opened), Some(&*edited()));
    close(&mut files, opened.file).unwrap();
    assert!(!dir.sidecar().exists());
    let opened = open(&mut files, &dir.design()).unwrap();
    assert_eq!(opened.recovered, Ok(None));
}

/// After a crash, the design opens as saved, with the auto-saved state
/// alongside to offer, until it's discarded.
#[test]
fn a_crash_is_recovered_from_on_opening() {
    let dir = TempDir::new("files-recover");
    let design = dir.design();
    crashed_with(&design, &edited());
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &design).unwrap();
    assert_eq!(opened.document, Document::example());
    assert_eq!(offered(&opened), Some(&*edited()));
    // Another editor only reads the design.
    let other = open(&mut files, &design).unwrap();
    assert_eq!(other.recovered, Ok(None));
    close(&mut files, other.file).unwrap();

    // Abandoning the open keeps it too: the user never saw it.
    files.handle(Request::Abandon { id: OpenId(7) });
    let opened = open(&mut files, &design).unwrap();
    assert_eq!(offered(&opened), Some(&*edited()));

    let Response::RecoveryDiscarded { result, .. } =
        files.handle(Request::DiscardRecovery { file: opened.file })
    else {
        panic!("not discarded");
    };
    result.unwrap();
    assert_eq!(auto_saved(&dir.sidecar()), None);
    close_keeping(&mut files, opened.file).unwrap();
    assert!(!dir.sidecar().exists());
    assert_eq!(open(&mut files, &design).unwrap().recovered, Ok(None));
}

/// Auto-saved as it was saved, e.g. a crash between saving and emptying
/// the sidecar: there's nothing to offer.
#[test]
fn nothing_is_offered_when_the_auto_save_is_what_was_saved() {
    let dir = TempDir::new("files-recover-same");
    crashed_with(&dir.design(), &Document::example());
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    assert_eq!(opened.recovered, Ok(None));
}

/// A damaged or torn sidecar never keeps the design from opening; what's
/// wrong is said, and auto-saving starts over.
#[test]
fn a_damaged_sidecar_is_ignored() {
    let dir = TempDir::new("files-recover-damaged");
    std::fs::write(dir.sidecar(), "garbage, not a design").unwrap();
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    assert_eq!(opened.access, Access::Edit);
    assert_eq!(opened.document, Document::example());
    assert!(opened.recovered.is_err());
    auto_save(&mut files, opened.file, edited()).unwrap();
    assert_eq!(auto_saved(&dir.sidecar()), Some((*edited()).clone()));
    close(&mut files, opened.file).unwrap();

    // Torn: what was complete is offered.
    crashed_with(&dir.design(), &edited());
    let mut bytes = std::fs::read(dir.sidecar()).unwrap();
    bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    std::fs::write(dir.sidecar(), bytes).unwrap();
    let opened = open(&mut files, &dir.design()).unwrap();
    assert_eq!(offered(&opened), Some(&*edited()));
}

/// A sidecar whose records are framed but none intact, say damaged by the
/// disk, is kept for whatever can be got out of it: the design opens,
/// saying so, auto-saves fail rather than write over it, and saves leave
/// it be, until the user discards it.
#[test]
fn a_sidecar_of_damaged_records_is_kept_until_discarded() {
    let dir = TempDir::new("files-recover-damaged-records");
    crashed_with(&dir.design(), &edited());
    let mut bytes = std::fs::read(dir.sidecar()).unwrap();
    // Within its one record's payload.
    bytes[crate::vrdp::FILE_HEADER_LEN + 80] ^= 0xff;
    std::fs::write(dir.sidecar(), &bytes).unwrap();
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    assert_eq!(opened.access, Access::Edit);
    assert!(opened.recovered.is_err());
    assert!(auto_save(&mut files, opened.file, edited()).is_err());
    save(&mut files, opened.file, edited()).unwrap();
    assert_eq!(std::fs::read(dir.sidecar()).unwrap(), bytes);

    files.handle(Request::DiscardRecovery { file: opened.file });
    auto_save(&mut files, opened.file, edited()).unwrap();
    assert_eq!(auto_saved(&dir.sidecar()), Some((*edited()).clone()));
}

/// A sidecar of plain documents, as auto-saves were before they kept what
/// they were based on, is no auto-save: it's said so and the design opens,
/// and auto-saving starts over.
#[test]
fn a_sidecar_of_plain_documents_is_ignored() {
    for (n, document) in [
        Document::default(),
        Document::example(),
        (*edited()).clone(),
    ]
    .into_iter()
    .enumerate()
    {
        let dir = TempDir::new(&format!("files-recover-plain-{n}"));
        let mut plain = crate::vrdp::HeldFile::<_, Document>::new(
            std::fs::File::options()
                .read(true)
                .write(true)
                .create_new(true)
                .open(dir.sidecar())
                .unwrap(),
        );
        plain.append(&document).unwrap();
        drop(plain);
        let mut files = Files::new(Stores::default());
        let opened = open(&mut files, &dir.design()).unwrap();
        assert_eq!(opened.access, Access::Edit);
        assert_eq!(opened.document, Document::example());
        assert!(opened.recovered.is_err(), "{document:?}");
        auto_save(&mut files, opened.file, edited()).unwrap();
        assert_eq!(auto_saved(&dir.sidecar()), Some((*edited()).clone()));
    }
}

/// Save As over a design a crashed editor left an auto-save of replaces
/// that design, and with it what was auto-saved; Save As over itself
/// empties its own.
#[test]
fn save_as_empties_the_sidecar_it_ends_up_with() {
    let dir = TempDir::new("files-save-as-sidecar");
    let other = dir.0.join("other.vrdp");
    DocumentFile::create(&other, &Document::example(), &[]).unwrap();
    crashed_with(&other, &Document::default());
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    auto_save(&mut files, opened.file, edited()).unwrap();
    save_as(&mut files, Some(opened.file), &other, true).unwrap();
    assert!(!dir.sidecar().exists());
    let other_sidecar = sidecar_path(&other).unwrap();
    assert_eq!(auto_saved(&other_sidecar), None);

    auto_save(&mut files, opened.file, Arc::new(Document::example())).unwrap();
    save_as(&mut files, Some(opened.file), &other, true).unwrap();
    assert_eq!(auto_saved(&other_sidecar), None);
}

fn create(files: &mut Files) -> Result<FileId, String> {
    match files.handle(Request::New { id: OpenId(3) }) {
        Response::Created {
            id: OpenId(3),
            result,
        } => result,
        response => panic!("unexpected {response:?}"),
    }
}

/// The only entry in the store.
fn entry(dir: &TempDir) -> Option<PathBuf> {
    let mut entries = std::fs::read_dir(dir.0.join("designs")).ok()?;
    let entry = entries.next()?.unwrap().path();
    assert!(entries.next().is_none());
    Some(entry)
}

fn listed(files: &mut Files) -> Vec<crate::Recovered> {
    match files.handle(Request::ListRecovered) {
        Response::RecoveredListed { designs } => designs,
        response => panic!("unexpected {response:?}"),
    }
}

/// How many designs are listed after the answer to `request`, if its
/// answer is followed by the list, see [`Files::answer`].
fn relisted(files: &mut Files, request: Request) -> Option<usize> {
    match &files.answer(request)[..] {
        [_] => None,
        [_, Response::RecoveredListed { designs }] => Some(designs.len()),
        responses => panic!("unexpected {responses:?}"),
    }
}

/// What may change the recovered designs is answered, then followed by
/// the list as it is now: closing or abandoning a new design's entry,
/// opening or discarding one. Closing a design's sidecar isn't: it's
/// never listed.
#[test]
fn what_may_change_the_recovered_designs_lists_them() {
    let dir = TempDir::new("files-relist");
    let mut files = with_store(&dir);
    let opened = open(&mut files, &dir.design()).unwrap();
    auto_save(&mut files, opened.file, edited()).unwrap();
    let close = |file, closing| Request::Close { file, closing };
    assert_eq!(
        relisted(&mut files, close(opened.file, Closing::Keep)),
        None
    );

    let file = create(&mut files).unwrap();
    auto_save(&mut files, file, edited()).unwrap();
    assert_eq!(relisted(&mut files, close(file, Closing::Keep)), Some(1));
    let path = entry(&dir).unwrap();
    // Held while open, so not listed; listed again once given up on.
    let open_recovered = Request::OpenRecovered {
        id: OpenId(4),
        path: path.clone(),
    };
    assert_eq!(relisted(&mut files, open_recovered), Some(0));
    assert_eq!(
        relisted(&mut files, Request::Abandon { id: OpenId(4) }),
        Some(1)
    );
    assert_eq!(
        relisted(&mut files, Request::DiscardRecovered { path }),
        Some(0)
    );
    // Nothing else is followed by it.
    assert_eq!(relisted(&mut files, Request::Flush), None);
}

/// A new design lives in its store entry, which its auto-saves go to,
/// until the first Save As, which deletes it.
#[test]
fn a_new_design_moves_from_its_entry_to_its_file() {
    let dir = TempDir::new("files-new");
    let mut files = with_store(&dir);
    let file = create(&mut files).unwrap();
    let entry_path = entry(&dir).unwrap();
    assert_eq!(std::fs::metadata(&entry_path).unwrap().len(), 0);
    auto_save(&mut files, file, edited()).unwrap();
    // Read from a copy: the lane holds its lock.
    assert_eq!(auto_saved(&entry_path), Some((*edited()).clone()));
    // Held: not a recovered design.
    assert_eq!(listed(&mut files), []);
    // It has no file of its own to save to.
    assert!(save(&mut files, file, edited()).is_err());

    let path = dir.0.join("new.vrdp");
    let saved = save_as(&mut files, Some(file), &path, false).unwrap();
    assert_eq!(saved.file, file);
    assert_eq!(saved.access, Access::Edit);
    assert_eq!(entry(&dir), None);
    assert_eq!(DocumentFile::open(&path).unwrap().1, *edited());
    // From then on the file's sidecar applies.
    auto_save(&mut files, file, Arc::new(Document::example())).unwrap();
    assert_eq!(
        auto_saved(&sidecar_path(&path).unwrap()),
        Some(Document::example())
    );
    save(&mut files, file, Arc::new(Document::example())).unwrap();
    close(&mut files, file).unwrap();
    assert!(!sidecar_path(&path).unwrap().exists());
}

/// A new design the user chose not to save is deleted with its entry.
#[test]
fn a_discarded_new_design_is_deleted() {
    let dir = TempDir::new("files-new-discard");
    let mut files = with_store(&dir);
    let file = create(&mut files).unwrap();
    auto_save(&mut files, file, edited()).unwrap();
    close(&mut files, file).unwrap();
    assert_eq!(entry(&dir), None);

    // One given up on before it was answered, and never auto-saved.
    create(&mut files).unwrap();
    files.handle(Request::Abandon { id: OpenId(3) });
    assert_eq!(entry(&dir), None);
}

/// A store entry, its own design's or another's, is locked on itself, not
/// by a sidecar: saving over it is refused, not deleted by its close.
#[test]
fn save_as_into_the_store_is_refused() {
    let dir = TempDir::new("files-save-as-store");
    let mut files = with_store(&dir);
    let file = create(&mut files).unwrap();
    auto_save(&mut files, file, Arc::new(Document::example())).unwrap();
    let entry_path = entry(&dir).unwrap();
    assert!(save_as(&mut files, Some(file), &entry_path, true).is_err());
    assert!(save_as(&mut files, None, &entry_path, true).is_err());
    // Nor a new name there, which would look like an entry.
    let beside = dir.0.join("designs").join("new.vrdp");
    assert!(save_as(&mut files, Some(file), &beside, false).is_err());
    assert!(!beside.exists());
    // The design still lives in its entry.
    assert_eq!(auto_saved(&entry_path), Some(Document::example()));
    close_keeping(&mut files, file).unwrap();
    assert_eq!(entry(&dir), Some(entry_path));
}

/// A sidecar isn't locked by a sidecar of its own: saving over one, held
/// here, elsewhere or by nobody, is refused rather than deleted by its
/// design's close or taken for a lock file by its next editor.
#[test]
fn save_as_onto_a_sidecar_is_refused() {
    let dir = TempDir::new("files-save-as-onto-sidecar");
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    let held = dir.sidecar();
    assert!(save_as(&mut files, Some(opened.file), &held, true).is_err());
    assert!(save_as(&mut files, None, &held, true).is_err());
    assert_eq!(std::fs::metadata(&held).unwrap().len(), 0);
    // Nor a sidecar of a design not open, whatever the suffix's case.
    let loose = dir.0.join(".other.vrdp.AutoSave");
    assert!(save_as(&mut files, None, &loose, false).is_err());
    assert!(!loose.exists());
    // Only the name matters.
    let dotted = dir.0.join(".hidden.vrdp");
    save_as(&mut files, None, &dotted, false).unwrap();
    close(&mut files, opened.file).unwrap();
    assert!(!held.exists());
}

/// A store entry left behind by a crash holds auto-saves, not a design:
/// opening it as one is refused rather than showing it empty, and it's
/// still offered back.
#[test]
fn opening_a_store_entry_as_a_design_is_refused() {
    let dir = TempDir::new("files-open-entry");
    let mut crashed = with_store(&dir);
    let file = create(&mut crashed).unwrap();
    auto_save(&mut crashed, file, Arc::new(Document::example())).unwrap();
    drop(crashed);
    let entry_path = entry(&dir).unwrap();

    let mut files = with_store(&dir);
    assert!(open(&mut files, &entry_path).is_err());
    assert_eq!(entry(&dir), Some(entry_path.clone()));
    assert_eq!(auto_saved(&entry_path), Some(Document::example()));
    assert_eq!(listed(&mut files).len(), 1);
}

#[test]
fn new_designs_need_a_store() {
    let mut files = Files::new(Stores::default());
    assert!(create(&mut files).is_err());
    assert_eq!(listed(&mut files), []);
}

/// A new design left behind by a crash is listed, opened as a new design
/// backed by its entry, and can be discarded.
#[test]
fn new_designs_left_behind_are_recovered() {
    let dir = TempDir::new("files-new-recovered");
    let mut crashed = with_store(&dir);
    let file = create(&mut crashed).unwrap();
    auto_save(&mut crashed, file, edited()).unwrap();
    // Dropping the lane lets go of it as a crash would.
    drop(crashed);
    // And one never auto-saved, which is cleaned up.
    let mut crashed = with_store(&dir);
    create(&mut crashed).unwrap();
    drop(crashed);

    let mut files = with_store(&dir);
    let designs = listed(&mut files);
    assert_eq!(designs.len(), 1);
    let path = designs[0].path.clone();
    assert_eq!(entry(&dir), Some(path.clone()));

    let Response::Opened {
        id: OpenId(4),
        result: Ok(opened),
        ..
    } = files.handle(Request::OpenRecovered {
        id: OpenId(4),
        path: path.clone(),
    })
    else {
        panic!("not opened");
    };
    assert_eq!(opened.document, *edited());
    assert_eq!(opened.access, Access::Edit);
    assert_eq!(listed(&mut files), []);
    auto_save(&mut files, opened.file, Arc::new(Document::example())).unwrap();
    // Not saved as a file of the user's yet: closing it without
    // discarding keeps it.
    close_keeping(&mut files, opened.file).unwrap();
    let saved = auto_saved_at(&path).unwrap();
    assert_eq!(*saved.document, Document::example());
    // Based on no file of the user's.
    assert_eq!(saved.base, None);

    let Response::RecoveredDiscarded { result, .. } =
        files.handle(Request::DiscardRecovered { path: path.clone() })
    else {
        panic!("not discarded");
    };
    result.unwrap();
    assert_eq!(entry(&dir), None);
    assert!(matches!(
        files.handle(Request::OpenRecovered {
            id: OpenId(5),
            path
        }),
        Response::Opened { result: Err(_), .. }
    ));
}

/// Only an entry of the store opens as a recovered design: anything else
/// would be taken for a new design's entry, auto-saved to and deleted on
/// closing. Refused for another file that reads as one, and for any path
/// without a store.
#[test]
fn only_entries_of_the_store_open_as_recovered() {
    let dir = TempDir::new("files-recovered-elsewhere");
    let mut crashed = with_store(&dir);
    let file = create(&mut crashed).unwrap();
    auto_save(&mut crashed, file, edited()).unwrap();
    drop(crashed);
    let path = entry(&dir).unwrap();
    let elsewhere = dir.0.join("elsewhere.vrdp");
    std::fs::copy(&path, &elsewhere).unwrap();
    let bytes = std::fs::read(&elsewhere).unwrap();

    let open_recovered =
        |files: &mut Files, path: &Path| match files.handle(Request::OpenRecovered {
            id: OpenId(4),
            path: path.to_owned(),
        }) {
            Response::Opened { result, .. } => result,
            response => panic!("unexpected {response:?}"),
        };
    let mut files = with_store(&dir);
    assert!(matches!(
        open_recovered(&mut files, &elsewhere),
        Err(e) if e.contains("isn't a recovered design")
    ));
    assert!(open_recovered(&mut Files::new(Stores::default()), &path).is_err());
    assert_eq!(std::fs::read(&elsewhere).unwrap(), bytes);
    assert_eq!(listed(&mut files).len(), 1);
}

/// A Save As that fails leaves the sidecar of the file it would have
/// replaced as it found it: what a crashed editor of that file auto-saved
/// is still to be offered when it's next opened.
#[test]
fn a_failed_save_as_keeps_what_the_target_auto_saved() {
    let dir = TempDir::new("files-save-as-failed");
    let other = dir.0.join("other.vrdp");
    DocumentFile::create(&other, &Document::example(), &[]).unwrap();
    crashed_with(&other, &edited());
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &dir.design()).unwrap();
    // Not asked to replace it, so it's refused.
    assert!(save_as(&mut files, Some(opened.file), &other, false).is_err());
    let other_sidecar = sidecar_path(&other).unwrap();
    assert_eq!(auto_saved(&other_sidecar), Some((*edited()).clone()));
    close(&mut files, opened.file).unwrap();
    assert_eq!(
        offered(&open(&mut files, &other).unwrap()),
        Some(&*edited())
    );

    // A sidecar made for the failed Save As only is deleted again.
    let missing = dir.0.join("missing").join("new.vrdp");
    let opened = open(&mut files, &dir.design()).unwrap();
    assert!(save_as(&mut files, Some(opened.file), &missing, false).is_err());
    let failed = dir.0.join("failed.vrdp");
    std::fs::create_dir(&failed).unwrap();
    assert!(save_as(&mut files, Some(opened.file), &failed, false).is_err());
    assert!(!sidecar_path(&failed).unwrap().exists());
}

/// What a crashed session auto-saved and the user hasn't answered the
/// offer of yet outlives a save: it's their other version of the design.
/// Discarding it, or auto-saving over it once it's restored, answers.
#[test]
fn a_save_keeps_recovered_changes_not_answered_yet() {
    let dir = TempDir::new("files-save-keeps-recovered");
    let design = dir.design();
    // Neither what's saved below nor what `save_as` writes.
    let recovered = with_sketch_named("Recovered");
    crashed_with(&design, &recovered);
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &design).unwrap();
    assert_eq!(offered(&opened), Some(&recovered));

    save(&mut files, opened.file, Arc::new(Document::example())).unwrap();
    assert_eq!(auto_saved(&dir.sidecar()), Some(recovered.clone()));
    // Save As over itself too, still offered.
    assert!(
        save_as(&mut files, Some(opened.file), &design, true)
            .unwrap()
            .offered
    );
    assert_eq!(auto_saved(&dir.sidecar()), Some(recovered.clone()));
    // Closing without an answer keeps it, to be offered again.
    close_keeping(&mut files, opened.file).unwrap();
    let opened = open(&mut files, &design).unwrap();
    assert_eq!(offered(&opened), Some(&recovered));

    // Save As elsewhere leaves it with the design it's of, no longer
    // offered for the new file.
    let other = dir.0.join("other.vrdp");
    assert!(
        !save_as(&mut files, Some(opened.file), &other, false)
            .unwrap()
            .offered
    );
    assert_eq!(auto_saved(&dir.sidecar()), Some(recovered.clone()));
    close_keeping(&mut files, opened.file).unwrap();

    // Restored and auto-saved: saves empty it from then on.
    let opened = open(&mut files, &design).unwrap();
    auto_save(&mut files, opened.file, Arc::new(recovered.clone())).unwrap();
    save(&mut files, opened.file, Arc::new(recovered.clone())).unwrap();
    assert_eq!(auto_saved(&dir.sidecar()), None);
    close(&mut files, opened.file).unwrap();

    // Discarded: likewise.
    crashed_with(&design, &Document::example());
    let opened = open(&mut files, &design).unwrap();
    assert!(matches!(opened.recovered, Ok(Some(_))));
    files.handle(Request::DiscardRecovery { file: opened.file });
    auto_save(&mut files, opened.file, edited()).unwrap();
    save(&mut files, opened.file, edited()).unwrap();
    assert_eq!(auto_saved(&dir.sidecar()), None);
}

/// A version of the design neither the example nor [`edited`].
fn saved_elsewhere() -> Document {
    with_sketch_named("Saved elsewhere")
}

/// Opens `design` in a lane of its own and auto-saves `document` to it,
/// then lets go of it as a crash would.
fn crash_after_auto_saving(design: &Path, document: &Document) {
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, design).unwrap();
    auto_save(&mut files, opened.file, Arc::new(document.clone())).unwrap();
}

/// Whether reopening `design` says it changed since what's offered as
/// recovered, which must be `edited`, was auto-saved. Closes it keeping
/// that, to be offered again.
fn reopened_changed(design: &Path) -> bool {
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, design).unwrap();
    assert_eq!(offered(&opened), Some(&*edited()));
    close_keeping(&mut files, opened.file).unwrap();
    opened
        .recovered
        .is_ok_and(|offer| offer.is_some_and(|offer| offer.design_changed))
}

/// Changes auto-saved from the design as it still is are offered as they
/// are, whatever the files' times say.
#[test]
fn recovered_changes_based_on_the_design_as_it_is_are_offered() {
    let dir = TempDir::new("files-recover-same-base");
    let design = dir.design();
    crash_after_auto_saving(&design, &edited());
    assert!(!reopened_changed(&design));

    // The sidecar older than the design: still the same version.
    let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(dir.sidecar())
        .unwrap()
        .set_modified(hour_ago)
        .unwrap();
    assert!(!reopened_changed(&design));

    // Saved by the session first, then auto-saved from what it saved.
    let dir = TempDir::new("files-recover-saved-first");
    let design = dir.design();
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &design).unwrap();
    save(&mut files, opened.file, Arc::new(saved_elsewhere())).unwrap();
    auto_save(&mut files, opened.file, edited()).unwrap();
    drop(files);
    assert!(!reopened_changed(&design));
}

/// Changes auto-saved from an older version of the design say so: saved
/// since through another handle, rewritten, or saved by the session they
/// were offered to, which kept them.
#[test]
fn recovered_changes_from_an_older_design_say_so() {
    let dir = TempDir::new("files-recover-other-handle");
    let design = dir.design();
    crash_after_auto_saving(&design, &edited());
    let (mut other, _, _) = DocumentFile::open(&design).unwrap();
    other.save(&saved_elsewhere(), &[]).unwrap();
    assert!(reopened_changed(&design));

    let dir = TempDir::new("files-recover-rewritten");
    let design = dir.design();
    crash_after_auto_saving(&design, &edited());
    DocumentFile::replace(&design, &saved_elsewhere(), &[]).unwrap();
    assert!(reopened_changed(&design));
    // Rewritten as it was, too: a new file.
    let dir = TempDir::new("files-recover-rewritten-same");
    let design = dir.design();
    crash_after_auto_saving(&design, &edited());
    DocumentFile::replace(&design, &Document::example(), &[]).unwrap();
    assert!(reopened_changed(&design));

    let dir = TempDir::new("files-recover-saved-by-us");
    let design = dir.design();
    crash_after_auto_saving(&design, &edited());
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &design).unwrap();
    assert!(matches!(
        opened.recovered,
        Ok(Some(Offer {
            design_changed: false,
            ..
        }))
    ));
    save(&mut files, opened.file, Arc::new(saved_elsewhere())).unwrap();
    close_keeping(&mut files, opened.file).unwrap();
    assert!(reopened_changed(&design));
}

/// A torn auto-save leaves the one before it, compared by its own base.
#[test]
fn a_torn_auto_save_is_compared_by_the_one_before() {
    let dir = TempDir::new("files-recover-torn-base");
    let design = dir.design();
    crash_after_auto_saving(&design, &edited());
    let mut bytes = std::fs::read(dir.sidecar()).unwrap();
    let half = bytes[bytes.len() / 2..].to_vec();
    bytes.extend_from_slice(&half);
    std::fs::write(dir.sidecar(), &bytes).unwrap();
    assert!(!reopened_changed(&design));

    std::fs::write(dir.sidecar(), &bytes).unwrap();
    let (mut other, _, _) = DocumentFile::open(&design).unwrap();
    other.save(&saved_elsewhere(), &[]).unwrap();
    assert!(reopened_changed(&design));
}

/// After Save As, auto-saves are based on the new file.
#[test]
fn auto_saves_after_save_as_are_based_on_the_new_file() {
    let dir = TempDir::new("files-recover-save-as");
    let mut files = with_store(&dir);
    let file = create(&mut files).unwrap();
    let design = dir.0.join("new.vrdp");
    save_as(&mut files, Some(file), &design, false).unwrap();
    auto_save(&mut files, file, edited()).unwrap();
    let (saved, _, _) = DocumentFile::open(&design).unwrap();
    let auto_saved = auto_saved_at(&sidecar_path(&design).unwrap()).unwrap();
    assert!(auto_saved.based_on(saved.tail()));
}

/// Natively files are picked by path: the web's picked files are refused,
/// answered with what they were for, and nothing is opened.
#[test]
fn picked_files_are_refused_natively() {
    let mut files = Files::new(Stores::default());
    let picked = Picked {
        id: 1,
        name: "a.vrdp".to_owned(),
        from: PickedFrom::Handle,
    };
    let response = files.handle(Request::Open {
        id: OpenId(2),
        from: Chosen::File(picked.clone()),
    });
    assert!(matches!(
        response,
        Response::Opened {
            id: OpenId(2),
            result: Err(_),
            ..
        }
    ));
    let response = files.handle(Request::SaveAs {
        file: None,
        to: SaveTo::Picked(picked),
        revision: 3.into(),
        document: Arc::new(Document::example()),
        thumbnail: None,
    });
    assert!(matches!(
        response,
        Response::SavedAs {
            revision,
            result: Err(SaveError::Failed(_)),
            ..
        } if revision == 3.into()
    ));
    assert!(files.open.is_empty());
}

mod damaged;
