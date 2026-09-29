use std::fs::{File, OpenOptions};
use std::sync::Arc;

use super::Origin::{Downloaded, Edited};
use super::*;
use crate::tests::{TempDir, with_sketches};

/// A plain file in `dir` holding a design with as many sketches as each of
/// `records` says, auto-saved in turn, marked as downloaded where it says
/// so. The rules of [`Held::end`] are the web's too, which holds an entry
/// through another [`Storage`].
fn held(dir: &TempDir, records: &[(usize, Origin)]) -> Held<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(dir.0.join("entry"))
        .unwrap();
    let mut held = Held::new(file);
    for &(sketches, origin) in records {
        held.append(None, None, &Arc::new(with_sketches(sketches)), origin)
            .unwrap();
    }
    held
}

/// The number of sketches of the design `held` holds, if any.
fn sketches(held: &mut Held<File>) -> Option<usize> {
    held.read()
        .unwrap()
        .map(|saved| saved.document.features().len())
}

/// The clean close empties it to be deleted, whatever is in it.
#[test]
fn ending_with_close_empties_it() {
    let dir = TempDir::new("held-close");
    let mut entry = held(&dir, &[(1, Downloaded), (0, Edited)]);
    assert!(entry.end(Ending::Close).unwrap());
    assert_eq!(sketches(&mut entry), None);
}

/// Released, it's deleted only if there's nothing in it, which stays.
#[test]
fn ending_with_release_keeps_what_is_in_it() {
    let dir = TempDir::new("held-release");
    let mut entry = held(&dir, &[(1, Edited), (0, Downloaded)]);
    assert!(!entry.end(Ending::Release).unwrap());
    assert_eq!(sketches(&mut entry), Some(0));
    entry.clear().unwrap();
    assert!(entry.end(Ending::Release).unwrap());
}

/// The clean close of a store entry goes back to the newest design as
/// downloaded, dropping the changes after it, and keeps it; one it never
/// had is emptied as by [`Ending::Close`].
#[test]
fn ending_with_close_but_downloaded_goes_back_to_the_download() {
    let dir = TempDir::new("held-close-but-downloaded");
    let mut entry = held(&dir, &[(0, Downloaded), (1, Downloaded), (0, Edited)]);
    assert!(!entry.end(Ending::CloseButDownloaded).unwrap());
    let saved = entry.read().unwrap().unwrap();
    assert!(saved.origin.is_download());
    assert_eq!(saved.document.features().len(), 1);

    let mut never = held(&dir, &[(1, Edited)]);
    assert!(never.end(Ending::CloseButDownloaded).unwrap());
    assert_eq!(sketches(&mut never), None);
}

/// Only one that may hold a download reads for it: one auto-saved as
/// downloaded, or left behind and opened again.
#[test]
fn ending_with_close_but_downloaded_reads_only_what_may_hold_one() {
    let dir = TempDir::new("held-downloads");
    let file = held(&dir, &[(1, Downloaded), (0, Edited)])
        .file
        .into_storage();
    assert!(Held::new(file).end(Ending::CloseButDownloaded).unwrap());

    let file = held(&dir, &[(1, Downloaded), (0, Edited)])
        .file
        .into_storage();
    let mut again = Held::new(file).left_behind();
    assert!(!again.end(Ending::CloseButDownloaded).unwrap());
    assert_eq!(sketches(&mut again), Some(1));
}

/// Discarding from the welcome screen takes the design as downloaded when
/// it's what's listed, the newest record, but only the changes on top of
/// one otherwise.
#[test]
fn ending_with_discard_keeps_a_download_under_changes() {
    let dir = TempDir::new("held-discard");
    let mut entry = held(&dir, &[(1, Downloaded), (0, Edited)]).left_behind();
    assert!(!entry.end(Ending::Discard).unwrap());
    assert_eq!(sketches(&mut entry), Some(1));
    assert!(entry.end(Ending::Discard).unwrap());
    assert_eq!(sketches(&mut entry), None);

    let mut never = held(&dir, &[(1, Edited)]);
    assert!(never.end(Ending::Discard).unwrap());
}

/// Checking an auto-save says which part of it is wrong.
#[test]
fn checking_an_auto_save_says_what_is_wrong() {
    let unchecked = |name: Option<String>, document: &[u8]| UncheckedAutoSaved {
        base: None,
        name,
        document: postcard::from_bytes(document).unwrap(),
        origin: Origin::Edited,
    };
    let document = with_sketches(1).to_postcard();
    assert!(AutoSaved::check(unchecked(Some("a".into()), &document)).is_ok());
    assert_eq!(
        AutoSaved::check(unchecked(Some("é".repeat(70_000)), &document)).err(),
        Some(AutoSavedError::Name(140_000))
    );
    // The next id, now 0, which the sketch's id 0 isn't below.
    let mut invalid = document;
    *invalid.last_mut().unwrap() = 0;
    assert!(matches!(
        AutoSaved::check(unchecked(None, &invalid)),
        Err(AutoSavedError::Document(CheckError::FeatureNextId(_)))
    ));
}

/// [`Origin`] is written as the `bool` it replaced, so files written
/// before read the same.
#[test]
fn an_origin_encodes_as_a_bool() {
    let bytes = |origin: Origin| postcard::to_stdvec(&origin).unwrap();
    assert_eq!(bytes(Edited), postcard::to_stdvec(&false).unwrap());
    assert_eq!(bytes(Downloaded), postcard::to_stdvec(&true).unwrap());
    assert_eq!(postcard::from_bytes::<Origin>(&[1]).unwrap(), Downloaded);
}
