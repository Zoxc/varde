use std::path::Path;
use std::sync::Arc;

use varde_lane::thread::testing::{TIMEOUT, next};

use super::*;
use crate::tests::{TempDir, with_sketches};
use crate::{Access, Chosen, Closing, FileId, OpenId, RecentFile, SaveError, SaveTo, Transport};

#[test]
fn answers_in_order_and_closes_files_when_dropped() {
    let dir = TempDir::new("thread");
    let design = dir.design();
    let (mut lane, mut responses) = spawn_at(Stores {
        recent: Some(dir.0.join("recent.toml")),
        designs: None,
    });

    let entries = vec![RecentFile {
        path: design.clone(),
        opened: crate::UnixSeconds(1),
    }];
    lane.send(Request::WriteRecent {
        entries: entries.clone(),
    });
    lane.send(Request::LoadRecent);
    lane.send(Request::Open {
        id: OpenId(1),
        from: Chosen::Path(design.clone()),
    });

    assert!(matches!(
        next(&mut responses),
        Response::RecentWritten { result: Ok(()) }
    ));
    assert!(matches!(
        next(&mut responses),
        Response::RecentLoaded { entries: loaded, .. }
            if loaded.iter().map(|listed| &listed.entry).eq(&entries)
    ));
    let Response::Opened {
        id: OpenId(1),
        result: Ok(opened),
        ..
    } = next(&mut responses)
    else {
        panic!("not opened");
    };
    assert_eq!(opened.access, Access::Edit);
    assert!(dir.sidecar().exists());

    responses.close().join().unwrap();
    assert!(!dir.sidecar().exists());
}

/// Opening the sidecar of a document the lane has open for editing must
/// not wait for the lane's own lock on it.
#[test]
fn opening_a_locked_sidecar_does_not_hang_the_lane() {
    let dir = TempDir::new("thread-sidecar");
    let design = dir.design();
    let (mut lane, mut responses) = spawn_at(Stores::default());
    lane.send(Request::Open {
        id: OpenId(1),
        from: Chosen::Path(design),
    });
    lane.send(Request::Open {
        id: OpenId(2),
        from: Chosen::Path(dir.sidecar()),
    });
    lane.send(Request::LoadRecent);
    assert!(matches!(
        next(&mut responses),
        Response::Opened {
            id: OpenId(1),
            result: Ok(_),
            ..
        }
    ));
    assert!(matches!(
        next(&mut responses),
        Response::Opened {
            id: OpenId(2),
            result: Err(_),
            ..
        }
    ));
    assert!(matches!(
        next(&mut responses),
        Response::RecentLoaded { .. }
    ));
}

/// A request that panics is answered with an error, and the lane goes on.
#[test]
fn a_panic_is_answered_and_the_lane_goes_on() {
    let (mut lane, mut responses) = spawn_on(|_| panic!("on purpose"));
    lane.send(Request::Open {
        id: OpenId(3),
        from: Chosen::Path("/a.vrdp".into()),
    });
    lane.send(Request::Close {
        file: FileId(1),
        closing: Closing::Clean,
    });
    lane.send(Request::LoadRecent);
    lane.send(Request::WriteRecent { entries: vec![] });
    lane.send(Request::Abandon { id: OpenId(3) });
    let failed = |result: Result<(), String>| {
        assert!(result.unwrap_err().contains("on purpose"));
    };
    let Response::Opened {
        id: OpenId(3),
        path,
        result,
    } = next(&mut responses)
    else {
        panic!("not the open");
    };
    assert_eq!(path.as_deref(), Some(Path::new("/a.vrdp")));
    failed(result.map(|_| ()));
    let Response::Closed {
        file: FileId(1),
        result,
    } = next(&mut responses)
    else {
        panic!("not the close");
    };
    failed(result);
    assert!(matches!(
        next(&mut responses),
        Response::RecentLoaded { entries, home: None, .. } if entries.is_empty()
    ));
    let Response::RecentWritten { result } = next(&mut responses) else {
        panic!("not the write");
    };
    failed(result);
    let Response::Abandoned {
        id: OpenId(3),
        result,
    } = next(&mut responses)
    else {
        panic!("not the abandon");
    };
    failed(result);
}

fn save(file: u64, revision: u64) -> Request {
    Request::Save {
        file: FileId(file),
        revision: revision.into(),
        document: Arc::new(varde_document::Document::default()),
    }
}

/// Saves arriving while one runs: the one running finishes, the waiting
/// ones collapse into the newest, and a flush after them answers last.
#[test]
fn saves_waiting_behind_a_running_one_collapse() {
    let dir = TempDir::new("thread-saves");
    let design = dir.design();
    let (gate, wait) = std::sync::mpsc::channel::<()>();
    let (started, running) = std::sync::mpsc::channel::<()>();
    let mut files = Files::new(Stores::default());
    let (mut lane, mut responses) = spawn_on(move |request| {
        if matches!(request, Request::Save { revision, .. } if revision == 1.into()) {
            // Held until the test has queued the rest.
            started.send(()).unwrap();
            wait.recv().unwrap();
        }
        files.handle(request)
    });
    lane.send(Request::Open {
        id: OpenId(1),
        from: Chosen::Path(design.clone()),
    });
    let Response::Opened {
        result: Ok(opened), ..
    } = next(&mut responses)
    else {
        panic!("not opened");
    };
    let snapshot = |sketches: usize| Arc::new(with_sketches(sketches));
    for (revision, sketches) in [(1, 0), (2, 1), (3, 0)] {
        lane.send(Request::Save {
            file: opened.file,
            revision: revision.into(),
            document: snapshot(sketches),
        });
        if revision == 1 {
            running.recv_timeout(TIMEOUT).unwrap();
        }
    }
    lane.send(Request::Flush);
    gate.send(()).unwrap();

    let mut answered = Vec::new();
    loop {
        match next(&mut responses) {
            Response::Saved {
                revision, result, ..
            } => {
                result.unwrap();
                answered.push(u64::from(revision));
            }
            Response::Flushed => break,
            response => panic!("unexpected {response:?}"),
        }
    }
    assert_eq!(answered, [1, 3]);
    let (document, _) = crate::vrdp::from_bytes(&std::fs::read(&design).unwrap()).unwrap();
    assert_eq!(document, *snapshot(0));
}

#[test]
fn a_failed_save_is_answered_with_its_revision() {
    let (mut lane, mut responses) = spawn_on(|_| panic!("on purpose"));
    lane.send(save(2, 7));
    lane.send(Request::SaveAs {
        file: None,
        to: SaveTo::Path {
            path: "/b.vrdp".into(),
            overwrite: true,
        },
        revision: 8.into(),
        document: Arc::new(varde_document::Document::default()),
    });
    lane.send(Request::Flush);
    assert!(matches!(
        next(&mut responses),
        Response::Saved { file: FileId(2), revision, result: Err(SaveError::Failed(e)) }
            if revision == 7.into() && e.contains("on purpose")
    ));
    assert!(matches!(
        next(&mut responses),
        Response::SavedAs {
            file: None,
            revision,
            result: Err(SaveError::Failed(_)),
            ..
        } if revision == 8.into()
    ));
    assert!(matches!(next(&mut responses), Response::Flushed));
}

fn auto_save(file: u64, revision: u64) -> Request {
    Request::AutoSave {
        file: FileId(file),
        revision: revision.into(),
        document: Arc::new(varde_document::Document::default()),
    }
}

/// Every new request that fails is answered, tagged as asked.
#[test]
fn a_failed_new_request_is_answered() {
    let (mut lane, mut responses) = spawn_on(|_| panic!("on purpose"));
    lane.send(Request::New { id: OpenId(1) });
    lane.send(auto_save(2, 7));
    lane.send(Request::DiscardRecovery { file: FileId(2) });
    lane.send(Request::ListRecovered);
    lane.send(Request::OpenRecovered {
        id: OpenId(3),
        path: "/r.vrdp".into(),
    });
    lane.send(Request::DiscardRecovered {
        path: "/r.vrdp".into(),
    });
    assert!(matches!(
        next(&mut responses),
        Response::Created {
            id: OpenId(1),
            result: Err(_)
        }
    ));
    assert!(matches!(
        next(&mut responses),
        Response::AutoSaved {
            file: FileId(2),
            revision,
            result: Err(_)
        } if revision == 7.into()
    ));
    assert!(matches!(
        next(&mut responses),
        Response::RecoveryDiscarded {
            file: FileId(2),
            result: Err(_)
        }
    ));
    assert!(matches!(
        next(&mut responses),
        Response::RecoveredListed { designs } if designs.is_empty()
    ));
    assert!(matches!(
        next(&mut responses),
        Response::Opened {
            id: OpenId(3),
            result: Err(_),
            ..
        }
    ));
    assert!(matches!(
        next(&mut responses),
        Response::RecoveredDiscarded { result: Err(_), .. }
    ));
}

/// Auto-saves arriving while one runs collapse into the newest, which is
/// what the sidecar ends up with.
#[test]
fn auto_saves_waiting_behind_a_running_one_collapse() {
    let dir = TempDir::new("thread-auto-saves");
    let design = dir.design();
    let (gate, wait) = std::sync::mpsc::channel::<()>();
    let (started, running) = std::sync::mpsc::channel::<()>();
    let mut files = Files::new(Stores::default());
    let (mut lane, mut responses) = spawn_on(move |request| {
        if matches!(request, Request::AutoSave { revision, .. } if revision == 1.into()) {
            started.send(()).unwrap();
            wait.recv().unwrap();
        }
        files.handle(request)
    });
    lane.send(Request::Open {
        id: OpenId(1),
        from: Chosen::Path(design),
    });
    let Response::Opened {
        result: Ok(opened), ..
    } = next(&mut responses)
    else {
        panic!("not opened");
    };
    let snapshot = |sketches: usize| Arc::new(with_sketches(sketches));
    for (revision, sketches) in [(1, 0), (2, 1), (3, 0)] {
        lane.send(Request::AutoSave {
            file: opened.file,
            revision: revision.into(),
            document: snapshot(sketches),
        });
        if revision == 1 {
            running.recv_timeout(TIMEOUT).unwrap();
        }
    }
    lane.send(Request::Flush);
    gate.send(()).unwrap();
    let mut answered = Vec::new();
    loop {
        match next(&mut responses) {
            Response::AutoSaved {
                revision, result, ..
            } => {
                result.unwrap();
                answered.push(u64::from(revision));
            }
            Response::Flushed => break,
            response => panic!("unexpected {response:?}"),
        }
    }
    assert_eq!(answered, [1, 3]);
    // The lane ending lets go of it as a crash would, keeping the newest.
    responses.close().join().unwrap();
    let saved = crate::tests::auto_saved_at(&dir.sidecar()).unwrap();
    assert_eq!(saved.document, snapshot(0));
}
