use std::sync::Arc;

use super::*;
use crate::{Chosen, Closing, OpenId, Picked, PickedFrom, RecentFile, Response, SaveError, SaveTo};

fn write(n: i64) -> Request {
    Request::WriteRecent {
        entries: vec![RecentFile {
            path: "/a.vrdp".into(),
            opened: crate::UnixSeconds(n),
        }],
    }
}

fn opened(request: &Request) -> Option<i64> {
    match request {
        Request::WriteRecent { entries } => Some(entries[0].opened.0),
        _ => None,
    }
}

#[test]
fn queue_keeps_order_and_replaces_a_waiting_write() {
    let mut queue = Queue::default();
    assert!(queue.push(Request::LoadRecent).is_none());
    assert!(queue.push(write(1)).is_none());
    assert!(
        queue
            .push(Request::Close {
                file: FileId(3),
                closing: Closing::Clean,
            })
            .is_none()
    );
    let replaced = queue.push(write(2)).unwrap();
    assert_eq!(opened(&replaced), Some(1));

    assert!(matches!(queue.pop(), Some(Request::LoadRecent)));
    assert!(matches!(
        queue.pop(),
        Some(Request::Close {
            file: FileId(3),
            closing: Closing::Clean,
        })
    ));
    assert_eq!(opened(&queue.pop().unwrap()), Some(2));
    assert!(queue.pop().is_none());

    // One already taken by the lane isn't replaced.
    assert!(queue.push(write(3)).is_none());
}

/// A load of the recent files list must see the write queued before it.
#[test]
fn a_waiting_write_is_not_replaced_across_a_load() {
    let mut queue = Queue::default();
    assert!(queue.push(write(1)).is_none());
    assert!(queue.push(Request::LoadRecent).is_none());
    assert!(queue.push(write(2)).is_none());

    assert_eq!(opened(&queue.pop().unwrap()), Some(1));
    assert!(matches!(queue.pop(), Some(Request::LoadRecent)));
    assert_eq!(opened(&queue.pop().unwrap()), Some(2));
    assert!(queue.pop().is_none());
}

fn save(file: u64, revision: u64) -> Request {
    Request::Save {
        file: FileId(file),
        revision: revision.into(),
        document: Arc::new(varde_document::Document::default()),
    }
}

/// What's queued, as `(kind, file, revision)`.
fn queued(queue: &Queue) -> Vec<(&'static str, Option<u64>, Option<u64>)> {
    queue
        .requests
        .iter()
        .map(|request| {
            let (kind, revision) = match request {
                Request::Save { revision, .. } => ("save", Some(u64::from(*revision))),
                Request::SaveAs { revision, .. } => ("save as", Some(u64::from(*revision))),
                Request::Close { .. } => ("close", None),
                _ => ("other", None),
            };
            let file = match request.target() {
                Some(Target::File(file)) => Some(file.0),
                _ => None,
            };
            (kind, file, revision)
        })
        .collect()
}

#[test]
fn a_waiting_save_is_replaced_by_a_newer_one_of_the_same_file() {
    let mut queue = Queue::default();
    assert!(queue.push(save(1, 1)).is_none());
    assert!(queue.push(save(2, 1)).is_none());
    assert!(queue.push(Request::LoadRecent).is_none());
    let replaced = queue.push(save(1, 2)).unwrap();
    assert!(matches!(replaced, Request::Save { revision, .. } if revision == 1.into()));
    assert_eq!(
        queued(&queue),
        [
            ("save", Some(2), Some(1)),
            ("other", None, None),
            ("save", Some(1), Some(2)),
        ]
    );

    // Not across a Save As or a close of the file, which must see the
    // older save first.
    let mut queue = Queue::default();
    queue.push(save(1, 1));
    queue.push(Request::SaveAs {
        file: Some(FileId(1)),
        to: SaveTo::Path {
            path: "/b.vrdp".into(),
            overwrite: false,
        },
        revision: 2.into(),
        document: Arc::new(varde_document::Document::default()),
    });
    assert!(queue.push(save(1, 3)).is_none());
    queue.push(Request::Close {
        file: FileId(1),
        closing: Closing::Clean,
    });
    assert!(queue.push(save(1, 4)).is_none());
    assert_eq!(queue.requests.len(), 5);
}

/// A Save As to a file picked on the web is ordered like any Save As: a
/// save of the file isn't moved past it. One that can't be handled is
/// answered with what it was for, and so is an open of a picked file.
#[test]
fn a_save_as_to_a_picked_file_is_ordered_and_answered_like_a_save_as() {
    let picked = Picked {
        id: 1,
        name: "b.vrdp".to_owned(),
        from: PickedFrom::Handle,
    };
    let mut queue = Queue::default();
    queue.push(save(1, 1));
    queue.push(Request::SaveAs {
        file: Some(FileId(1)),
        to: SaveTo::Picked(picked.clone()),
        revision: 2.into(),
        document: Arc::new(varde_document::Document::default()),
    });
    assert!(queue.push(save(1, 3)).is_none());
    assert_eq!(queue.requests.len(), 3);

    let Response::SavedAs {
        file: Some(FileId(1)),
        to: Chosen::File(answered),
        revision,
        result: Err(SaveError::Failed(error)),
    } = queue.requests[1].failure()("gone".to_owned())
    else {
        panic!("not the Save As");
    };
    assert_eq!(revision, 2.into());
    assert_eq!((answered, error.as_str()), (picked.clone(), "gone"));
    let Response::Opened {
        id: OpenId(4),
        path,
        result: Err(error),
    } = (Request::Open {
        id: OpenId(4),
        from: Chosen::File(picked),
    })
    .failed("gone".to_owned())
    else {
        panic!("not the open");
    };
    assert_eq!((path, error.as_str()), (None, "gone"));
}

/// A flush is answered once everything sent before it is done, so nothing
/// sent before it may be replaced by something sent after it.
#[test]
fn nothing_is_replaced_across_a_flush() {
    let mut queue = Queue::default();
    queue.push(save(1, 1));
    queue.push(write(1));
    queue.push(Request::Flush);
    assert!(queue.push(save(1, 2)).is_none());
    assert!(queue.push(write(2)).is_none());
    assert_eq!(
        queued(&queue),
        [
            ("save", Some(1), Some(1)),
            ("other", None, None),
            ("other", None, None),
            ("save", Some(1), Some(2)),
            ("other", None, None),
        ]
    );
    // After the flush they coalesce as usual.
    assert!(queue.push(save(1, 3)).is_some());
    assert!(queue.push(write(3)).is_some());
}

fn auto_save(file: u64, revision: u64) -> Request {
    Request::AutoSave {
        file: FileId(file),
        revision: revision.into(),
        document: Arc::new(varde_document::Document::default()),
    }
}

/// An auto-save still waiting is replaced by a newer one of its file, but
/// never across anything else done to the file, like a save, which empties
/// the sidecar the older one would have written to first, or a flush.
#[test]
fn a_waiting_auto_save_is_replaced_by_a_newer_one() {
    let mut queue = Queue::default();
    queue.push(auto_save(1, 1));
    queue.push(auto_save(2, 1));
    queue.push(Request::LoadRecent);
    let replaced = queue.push(auto_save(1, 2)).unwrap();
    assert!(matches!(replaced, Request::AutoSave { revision, .. } if revision == 1.into()));
    assert_eq!(queue.requests.len(), 3);

    for between in [
        save(1, 3),
        Request::SaveAs {
            file: Some(FileId(1)),
            to: SaveTo::Path {
                path: "/b.vrdp".into(),
                overwrite: false,
            },
            revision: 3.into(),
            document: Arc::new(varde_document::Document::default()),
        },
        Request::DiscardRecovery { file: FileId(1) },
        Request::Close {
            file: FileId(1),
            closing: Closing::Keep,
        },
        Request::Flush,
    ] {
        let mut queue = Queue::default();
        queue.push(auto_save(1, 2));
        queue.push(between);
        assert!(queue.push(auto_save(1, 4)).is_none());
        assert_eq!(queue.requests.len(), 3);
    }

    // Nor does a save replace an auto-save, or the other way round.
    let mut queue = Queue::default();
    queue.push(save(1, 1));
    assert!(queue.push(auto_save(1, 1)).is_none());
    assert!(queue.push(save(1, 2)).is_none());
    assert_eq!(queue.requests.len(), 3);
}

/// An auto-save of the design as downloaded is never replaced: a clean
/// close goes back to it, see `Request::Close`. Nor does it replace an
/// auto-save before it, which it keeps from being replaced in turn.
#[test]
fn a_waiting_auto_save_of_a_download_is_kept() {
    let downloaded = |revision: u64| Request::KeepDownload {
        file: FileId(1),
        revision: revision.into(),
        document: Arc::new(varde_document::Document::default()),
    };
    let mut queue = Queue::default();
    queue.push(auto_save(1, 1));
    assert!(queue.push(downloaded(1)).is_none());
    assert!(queue.push(auto_save(1, 2)).is_none());
    assert!(queue.push(auto_save(1, 3)).is_some());
    assert!(queue.push(downloaded(3)).is_none());
    assert!(queue.push(downloaded(4)).is_none());
    let revisions: Vec<_> = queue
        .requests
        .iter()
        .map(|request| match request {
            Request::AutoSave { revision, .. } => (u64::from(*revision), false),
            Request::KeepDownload { revision, .. } => (u64::from(*revision), true),
            request => panic!("unexpected {request:?}"),
        })
        .collect();
    assert_eq!(
        revisions,
        [(1, false), (1, true), (3, false), (3, true), (4, true)]
    );
}
