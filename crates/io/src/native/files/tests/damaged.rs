//! Damaged files through the lane: designs, sidecars and store entries.

use std::sync::Arc;

use varde_document::Document;

use super::{
    auto_save, close_keeping, create, edited, entry, listed, offered, open, save, save_as,
    with_store,
};
use crate::native::files::{DocumentFile, Files};
use crate::open::{KEPT, NOT_FOUND};
use crate::tests::{TempDir, auto_saved_at, with_sketch_named};
use crate::vrdp::Tail;
use crate::{
    Damage, DamageKind, FileId, OpenId, Opened, RecoveryError, Request, Response, SaveError,
    StoredDamage, Stores, UnixSeconds,
};

/// Where `prev` is in a block.
const PREV_AT: u64 = 8;
/// Where `sum` is in a block.
const SUM_AT: u64 = 24;
/// Somewhere in a block's tag or payload, past its header.
const BODY_AT: u64 = 60;

/// The `n`th save of a design, told apart by a sketch's name.
fn version(n: usize) -> Document {
    with_sketch_named(&format!("Save {n}"))
}

/// A design `doc.vrdp` in `dir` saved `n` times, `version(1)` to
/// `version(n)`, and the tail after each save.
fn saved(dir: &TempDir, n: usize) -> (std::path::PathBuf, Vec<Tail>) {
    let path = dir.0.join("doc.vrdp");
    let mut file = DocumentFile::create(&path, &version(1), &[]).unwrap();
    let mut tails = vec![file.tail()];
    for i in 2..=n {
        file.save(&version(i), &[]).unwrap();
        tails.push(file.tail());
    }
    (path, tails)
}

/// Flips the byte at `at` in the file at `path`.
fn flip(path: &std::path::Path, at: u64) {
    let mut bytes = std::fs::read(path).unwrap();
    bytes[at as usize] ^= 0xff;
    std::fs::write(path, bytes).unwrap();
}

/// Whether `time` is of a save written just now.
fn recent(time: UnixSeconds) -> bool {
    UnixSeconds::now()
        .checked_since(time)
        .is_some_and(|age| (0..3600).contains(&age))
}

fn open_found(files: &mut Files, file: FileId, found: Tail) -> Result<Opened, String> {
    match files.handle(Request::OpenFound {
        id: OpenId(7),
        file,
        found,
    }) {
        Response::Opened {
            id: OpenId(7),
            result,
            ..
        } => result,
        response => panic!("unexpected {response:?}"),
    }
}

/// How reading found a design's file is passed on with it: nothing for
/// an intact one or a torn tail, which the next save cuts off; earlier
/// saves stepped over; the newest damaged, opening the one before; damage
/// only a search got past, opening the newest the file proves and
/// offering the search's newest. Each with the time of the save opened
/// and the bytes that can't be read.
#[test]
fn how_reading_found_a_design_is_passed_on() {
    let dir = TempDir::new("files-damaged-report");
    let (path, tails) = saved(&dir, 4);
    let intact = std::fs::read(&path).unwrap();
    let size = |i: usize| tails[i].span().end - tails[i].span().start;
    let mut files = Files::new(Stores::default());
    let opened = |files: &mut Files| {
        let opened = open(files, &path).unwrap();
        close_keeping(files, opened.file).unwrap();
        opened
    };

    let found = opened(&mut files);
    assert_eq!((found.document, found.damage), (version(4), None));
    std::fs::write(&path, [&intact[..], &[0; 100]].concat()).unwrap();
    let found = opened(&mut files);
    assert_eq!((found.document, found.damage), (version(4), None));

    let cases = [
        (
            tails[1].span().start + BODY_AT,
            4,
            DamageKind::Bridged,
            size(1),
        ),
        (
            tails[3].span().start + BODY_AT,
            3,
            DamageKind::NewestDamaged,
            size(3),
        ),
    ];
    for (at, opens, kind, unreadable) in cases {
        std::fs::write(&path, &intact).unwrap();
        flip(&path, at);
        let found = opened(&mut files);
        assert_eq!(found.document, version(opens));
        let damage = found.damage.unwrap();
        assert_eq!((damage.kind, damage.unreadable), (kind, unreadable));
        assert!(recent(damage.time), "{damage:?}");
    }

    std::fs::write(&path, &intact).unwrap();
    flip(&path, tails[1].span().start + PREV_AT);
    let found = opened(&mut files);
    assert_eq!(found.document, version(1));
    let damage = found.damage.unwrap();
    let DamageKind::Damaged { found: Some(save) } = damage.kind else {
        panic!("{damage:?}");
    };
    assert_eq!(save.tail, tails[3]);
    assert!(recent(save.time) && recent(damage.time), "{damage:?}");
    assert_eq!(damage.unreadable, size(1));

    // With no save before the damage, the search's newest opens, with
    // nothing else to offer.
    std::fs::write(&path, &intact).unwrap();
    flip(&path, tails[0].span().start + PREV_AT);
    let found = opened(&mut files);
    assert_eq!(found.document, version(4));
    assert!(matches!(
        found.damage,
        Some(Damage {
            kind: DamageKind::Damaged { found: None },
            ..
        })
    ));
}

/// A design found damaged past what opened is never saved to, only saved
/// as another file; after the newest save damaged, saves go after it.
/// Damage since a design was opened refuses saves too.
#[test]
fn saves_to_damaged_designs_are_refused() {
    let dir = TempDir::new("files-damaged-save");
    let (path, tails) = saved(&dir, 4);
    let intact = std::fs::read(&path).unwrap();
    let mut files = Files::new(Stores::default());

    flip(&path, tails[1].span().start + PREV_AT);
    let damaged = std::fs::read(&path).unwrap();
    let opened = open(&mut files, &path).unwrap();
    assert_eq!(
        save(&mut files, opened.file, edited()),
        Err(SaveError::OpenedDamaged)
    );
    assert_eq!(std::fs::read(&path).unwrap(), damaged);
    let other = dir.0.join("other.vrdp");
    save_as(&mut files, Some(opened.file), &other, false).unwrap();
    save(&mut files, opened.file, edited()).unwrap();
    close_keeping(&mut files, opened.file).unwrap();

    std::fs::write(&path, &intact).unwrap();
    flip(&path, tails[3].span().start + BODY_AT);
    let opened = open(&mut files, &path).unwrap();
    save(&mut files, opened.file, edited()).unwrap();
    let reopened = open(&mut files, &path).unwrap();
    assert_eq!(reopened.document, *edited());
    assert_eq!(reopened.damage.unwrap().kind, DamageKind::Bridged);
    close_keeping(&mut files, reopened.file).unwrap();
    close_keeping(&mut files, opened.file).unwrap();

    std::fs::write(&path, &intact).unwrap();
    let opened = open(&mut files, &path).unwrap();
    flip(&path, tails[3].span().start + BODY_AT);
    assert_eq!(
        save(&mut files, opened.file, edited()),
        Err(SaveError::Damaged)
    );
}

/// The save a search found past damage opens instead when chosen, once,
/// in the same file: auto-saves are based on it, saves still refused.
#[test]
fn the_save_found_past_damage_opens_when_chosen() {
    let dir = TempDir::new("files-damaged-found");
    let (path, tails) = saved(&dir, 4);
    flip(&path, tails[1].span().start + PREV_AT);
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &path).unwrap();
    let Some(DamageKind::Damaged {
        found: Some(save_found),
    }) = opened.damage.map(|d| d.kind)
    else {
        panic!("{:?}", opened.damage);
    };

    assert_eq!(
        open_found(&mut files, opened.file, tails[2]).map(|_| ()),
        Err(NOT_FOUND.to_owned())
    );
    assert!(open_found(&mut files, FileId(99), save_found.tail).is_err());
    let found = open_found(&mut files, opened.file, save_found.tail).unwrap();
    assert_eq!(found.file, opened.file);
    assert_eq!(found.document, version(4));
    assert_eq!(found.access, opened.access);
    assert_eq!(found.recovered, Ok(None));
    let damage = found.damage.unwrap();
    assert_eq!(damage.kind, DamageKind::Damaged { found: None });
    assert_eq!(damage.time, save_found.time);
    assert_eq!(
        open_found(&mut files, opened.file, save_found.tail).map(|_| ()),
        Err(NOT_FOUND.to_owned())
    );

    assert_eq!(
        save(&mut files, opened.file, edited()),
        Err(SaveError::OpenedDamaged)
    );
    auto_save(&mut files, opened.file, edited()).unwrap();
    let auto_saved = auto_saved_at(&dir.sidecar()).unwrap();
    assert_eq!(auto_saved.base, Some(tails[3]));

    // Opened by path, it's answered with the path.
    close_keeping(&mut files, opened.file).unwrap();
    let opened = open(&mut files, &path).unwrap();
    let Response::Opened { path: answered, .. } = files.handle(Request::OpenFound {
        id: OpenId(8),
        file: opened.file,
        found: save_found.tail,
    }) else {
        panic!("not opened");
    };
    assert_eq!(answered.as_deref(), Some(&*path));
}

/// Auto-saves `documents` in turn to the design at `path` in a lane of
/// its own, then lets go of it as a crash would, returning where each
/// record starts in the sidecar.
fn crash_after(
    path: &std::path::Path,
    sidecar: &std::path::Path,
    documents: &[Document],
) -> Vec<u64> {
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, path).unwrap();
    let mut starts = Vec::new();
    for document in documents {
        starts.push(
            std::fs::metadata(sidecar)
                .unwrap()
                .len()
                .max(crate::vrdp::FILE_HEADER_LEN as u64),
        );
        auto_save(&mut files, opened.file, Arc::new(document.clone())).unwrap();
    }
    starts
}

/// The sidecar's newest intact auto-save is offered past damage, saying
/// so.
#[test]
fn damage_in_the_sidecar_is_noted_in_the_offer() {
    let dir = TempDir::new("files-damaged-sidecar");
    let design = dir.design();
    let starts = crash_after(&design, &dir.sidecar(), &[version(7), version(8)]);
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &design).unwrap();
    assert_eq!(offered(&opened), Some(&version(8)));
    let offer = opened.recovered.clone().unwrap().unwrap();
    assert_eq!(offer.damage, None);
    assert!(!offer.newer_base);
    close_keeping(&mut files, opened.file).unwrap();

    flip(&dir.sidecar(), starts[1] + BODY_AT);
    let opened = open(&mut files, &design).unwrap();
    assert_eq!(offered(&opened), Some(&version(7)));
    let offer = opened.recovered.unwrap().unwrap();
    assert_eq!(offer.damage.unwrap().kind, DamageKind::NewestDamaged);
    assert!(!offer.design_changed);
}

/// A sidecar whose auto-saves are framed but none intact is offered as
/// one that can't be read, kept: auto-saves are refused, the same way
/// each time, without touching it, till it's discarded. One that isn't
/// an auto-save at all isn't kept.
#[test]
fn a_sidecar_that_cant_be_read_holds_auto_saves_till_discarded() {
    let dir = TempDir::new("files-damaged-sidecar-kept");
    let design = dir.design();
    let starts = crash_after(&design, &dir.sidecar(), &[version(7)]);
    flip(&dir.sidecar(), starts[0] + BODY_AT);
    let damaged = std::fs::read(dir.sidecar()).unwrap();
    let mut files = Files::new(Stores::default());
    let opened = open(&mut files, &design).unwrap();
    assert!(
        matches!(&opened.recovered, Err(RecoveryError { kept: true, .. })),
        "{:?}",
        opened.recovered
    );
    for _ in 0..2 {
        assert_eq!(
            auto_save(&mut files, opened.file, edited()),
            Err(KEPT.to_owned())
        );
    }
    assert_eq!(std::fs::read(dir.sidecar()).unwrap(), damaged);
    // Kept by a close that keeps what's offered.
    close_keeping(&mut files, opened.file).unwrap();
    assert_eq!(std::fs::read(dir.sidecar()).unwrap(), damaged);

    let opened = open(&mut files, &design).unwrap();
    files.handle(Request::DiscardRecovery { file: opened.file });
    auto_save(&mut files, opened.file, edited()).unwrap();
    close_keeping(&mut files, opened.file).unwrap();

    std::fs::write(dir.sidecar(), "not an auto-save").unwrap();
    let opened = open(&mut files, &design).unwrap();
    assert!(matches!(
        &opened.recovered,
        Err(RecoveryError { kept: false, .. })
    ));
    auto_save(&mut files, opened.file, edited()).unwrap();
}

/// An auto-save based on a newer save of the design than could be read,
/// its newest save damaged, says so; one based on an older save, or on
/// one whose header is damaged too, doesn't.
#[test]
fn an_auto_save_from_a_newer_save_than_could_be_read_says_so() {
    let dir = TempDir::new("files-damaged-newer-base");
    let (path, tails) = saved(&dir, 3);
    crash_after(&path, &dir.sidecar(), &[version(9)]);
    let intact = std::fs::read(&path).unwrap();
    let sidecar = std::fs::read(dir.sidecar()).unwrap();
    let offer = |at: u64| {
        std::fs::write(&path, &intact).unwrap();
        std::fs::write(dir.sidecar(), &sidecar).unwrap();
        flip(&path, at);
        let mut files = Files::new(Stores::default());
        let opened = open(&mut files, &path).unwrap();
        assert_eq!(opened.document, version(2));
        opened.recovered.unwrap().unwrap()
    };
    for at in [BODY_AT, PREV_AT] {
        let offer = offer(tails[2].span().start + at);
        assert!(offer.newer_base && offer.design_changed, "{offer:?}");
    }
    let offer = offer(tails[2].span().start + SUM_AT);
    assert!(!offer.newer_base && offer.design_changed, "{offer:?}");

    // Based on an older save than the one opened.
    std::fs::write(&path, &intact).unwrap();
    std::fs::write(dir.sidecar(), &sidecar).unwrap();
    let (mut other, _, _) = DocumentFile::open(&path).unwrap();
    other.save(&version(4), &[]).unwrap();
    let mut files = Files::new(Stores::default());
    let offer = open(&mut files, &path).unwrap().recovered.unwrap().unwrap();
    assert!(!offer.newer_base && offer.design_changed, "{offer:?}");
}

/// Store entries with damaged auto-saves are listed, marked so: one whose
/// newest intact auto-save opens, with the damage noted, and one none of
/// whose auto-saves can be read, which only discarding takes away.
#[test]
fn damaged_store_entries_are_listed_and_open() {
    let dir = TempDir::new("files-damaged-entries");
    let mut crashed = with_store(&dir);
    let file = create(&mut crashed).unwrap();
    auto_save(&mut crashed, file, Arc::new(version(1))).unwrap();
    drop(crashed);
    let path = entry(&dir).unwrap();
    let second = std::fs::metadata(&path).unwrap().len();
    let mut crashed = with_store(&dir);
    let Response::Opened {
        result: Ok(opened), ..
    } = crashed.handle(Request::OpenRecovered {
        id: OpenId(1),
        path: path.clone(),
    })
    else {
        panic!("not opened");
    };
    assert_eq!(opened.damage, None);
    auto_save(&mut crashed, opened.file, Arc::new(version(2))).unwrap();
    drop(crashed);
    flip(&path, second + BODY_AT);

    let mut files = with_store(&dir);
    let designs = listed(&mut files);
    assert_eq!(designs.len(), 1);
    assert_eq!(designs[0].damage, Some(StoredDamage::Opens));
    let Response::Opened {
        result: Ok(opened), ..
    } = files.handle(Request::OpenRecovered {
        id: OpenId(2),
        path: path.clone(),
    })
    else {
        panic!("not opened");
    };
    assert_eq!(opened.document, version(1));
    assert_eq!(opened.damage.unwrap().kind, DamageKind::NewestDamaged);
    close_keeping(&mut files, opened.file).unwrap();

    // None intact.
    flip(&path, crate::vrdp::FILE_HEADER_LEN as u64 + BODY_AT);
    let designs = listed(&mut files);
    assert_eq!(designs.len(), 1);
    assert_eq!(designs[0].damage, Some(StoredDamage::Unreadable));
    assert_eq!(designs[0].name, None);
    assert!(matches!(
        files.handle(Request::OpenRecovered {
            id: OpenId(3),
            path: path.clone(),
        }),
        Response::Opened { result: Err(_), .. }
    ));
    let Response::RecoveredDiscarded { result, .. } =
        files.handle(Request::DiscardRecovered { path })
    else {
        panic!("not discarded");
    };
    result.unwrap();
    assert_eq!(entry(&dir), None);
    assert_eq!(listed(&mut files), []);
}
