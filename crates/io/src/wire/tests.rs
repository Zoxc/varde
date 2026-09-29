use std::path::PathBuf;
use std::sync::Arc;

use varde_document::{Command, Document, Editor, Revision};

use super::*;
use crate::{
    Access, Chosen, Closing, FileId, Offer, OpenId, Opened, Picked, PickedFrom, ReadOnly,
    RecentFile, Recovered, SaveError, SaveTo, SavedAs,
};

fn round_trip<T: Serialize + for<'a> Deserialize<'a>>(message: &T) -> T {
    decode(&encode(message, MAX_MESSAGE_BYTES).unwrap()).unwrap()
}

/// A design with a hidden sketch.
fn hidden() -> Document {
    let mut editor = Editor::new(Document::default());
    let xy = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
    editor.apply(editor.document().add_sketch(xy)).unwrap();
    let sketch = editor.document().features()[0].id;
    editor
        .apply(Command::SetFeatureVisible(sketch, false))
        .unwrap();
    editor.document().clone()
}

/// Every request, one of each kind.
fn requests() -> Vec<Request> {
    let document = Arc::new(Document::example());
    vec![
        Request::Open {
            id: OpenId(1),
            from: Chosen::Path(PathBuf::from("/home/me/design.vrdp")),
        },
        Request::New { id: OpenId(2) },
        Request::Save {
            file: FileId(3),
            revision: 4.into(),
            document: Arc::clone(&document),
        },
        Request::SaveAs {
            file: Some(FileId(5)),
            to: SaveTo::Path {
                path: PathBuf::from("copy.vrdp"),
                overwrite: true,
            },
            revision: u64::MAX.into(),
            document: Arc::clone(&document),
        },
        Request::SaveAs {
            file: None,
            to: SaveTo::Path {
                path: PathBuf::from("ünïcode.vrdp"),
                overwrite: false,
            },
            revision: 0.into(),
            document: Arc::new(Document::default()),
        },
        Request::AutoSave {
            file: FileId(6),
            revision: 7.into(),
            document: Arc::clone(&document),
        },
        Request::KeepDownload {
            file: FileId(6),
            revision: 8.into(),
            document,
        },
        Request::DiscardRecovery { file: FileId(8) },
        Request::Close {
            file: FileId(9),
            closing: Closing::Clean,
        },
        Request::Abandon { id: OpenId(10) },
        Request::LoadRecent,
        Request::ListRecovered,
        Request::OpenRecovered {
            id: OpenId(11),
            path: PathBuf::from("designs/1-2-3.vrdp"),
        },
        Request::DiscardRecovered {
            path: PathBuf::from("designs/1-2-3.vrdp"),
        },
        Request::Open {
            id: OpenId(12),
            from: Chosen::File(Picked {
                id: 13,
                name: "bracket.vrdp".to_owned(),
                from: PickedFrom::Handle,
            }),
        },
        Request::SaveAs {
            file: Some(FileId(14)),
            to: SaveTo::Picked(Picked {
                id: u64::MAX,
                name: "ünïcode.vrdp".to_owned(),
                from: PickedFrom::Input,
            }),
            revision: 15.into(),
            document: Arc::new(Document::example()),
        },
        Request::SaveAs {
            file: None,
            to: SaveTo::Picked(Picked {
                id: 0,
                name: String::new(),
                from: PickedFrom::Handle,
            }),
            revision: 0.into(),
            document: Arc::new(Document::default()),
        },
        Request::WriteRecent {
            entries: vec![RecentFile {
                path: PathBuf::from("/a.vrdp"),
                opened: crate::UnixSeconds(-12),
            }],
        },
        Request::Flush,
    ]
}

/// Every response, one of each kind and result.
fn responses() -> Vec<Response> {
    let edited = hidden();
    vec![
        Response::Opened {
            id: OpenId(1),
            path: Some(PathBuf::from("designs/a.vrdp")),
            result: Ok(Opened {
                file: FileId(2),
                document: Document::example(),
                access: Access::Edit,
                recovered: Ok(Some(Offer {
                    document: edited,
                    design_changed: true,
                })),
                downloaded: false,
            }),
        },
        Response::Opened {
            id: OpenId(1),
            path: Some(PathBuf::from("designs/b.vrdp")),
            result: Ok(Opened {
                file: FileId(3),
                document: Document::default(),
                access: Access::ReadOnly(ReadOnly::InUse),
                recovered: Err("damaged".to_owned()),
                downloaded: true,
            }),
        },
        Response::Opened {
            id: OpenId(4),
            path: None,
            result: Err("no".to_owned()),
        },
        Response::Created {
            id: OpenId(5),
            result: Ok(FileId(6)),
        },
        Response::Saved {
            file: FileId(7),
            revision: 8.into(),
            result: Err(SaveError::Conflict),
        },
        Response::SavedAs {
            file: None,
            to: Chosen::Path(PathBuf::from("x.vrdp")),
            revision: 9.into(),
            result: Ok(SavedAs {
                file: FileId(10),
                access: Access::ReadOnly(ReadOnly::NoLock("read-only".to_owned())),
                offered: false,
            }),
        },
        Response::SavedAs {
            file: Some(FileId(16)),
            to: Chosen::File(Picked {
                id: 17,
                name: "copy.vrdp".to_owned(),
                from: PickedFrom::Handle,
            }),
            revision: 18.into(),
            result: Ok(SavedAs {
                file: FileId(16),
                access: Access::Edit,
                offered: false,
            }),
        },
        Response::SavedAs {
            file: None,
            to: Chosen::File(Picked {
                id: 19,
                name: "copy.vrdp".to_owned(),
                from: PickedFrom::Handle,
            }),
            revision: 20.into(),
            result: Err(SaveError::Conflict),
        },
        Response::AutoSaved {
            file: FileId(11),
            revision: 12.into(),
            result: Ok(()),
        },
        Response::RecoveryDiscarded {
            file: FileId(13),
            result: Err(SaveError::Failed("x".to_owned()).to_string()),
        },
        Response::Closed {
            file: FileId(14),
            result: Ok(()),
        },
        Response::Abandoned {
            id: OpenId(15),
            result: Ok(()),
        },
        Response::RecentLoaded {
            entries: vec![crate::recent::Listed {
                entry: RecentFile {
                    path: PathBuf::from("/b.vrdp"),
                    opened: crate::UnixSeconds(i64::MAX),
                },
                available: false,
            }],
            home: Some(PathBuf::from("/home/me")),
        },
        Response::RecentWritten { result: Ok(()) },
        Response::RecoveredListed {
            designs: vec![
                Recovered {
                    path: PathBuf::from("designs/a.vrdp"),
                    modified: Some(crate::UnixSeconds(1_700_000_000)),
                    name: Some("bracket.vrdp".to_owned()),
                    downloaded: true,
                },
                Recovered {
                    path: PathBuf::from("designs/b.vrdp"),
                    modified: None,
                    name: None,
                    downloaded: false,
                },
            ],
        },
        Response::RecoveredDiscarded {
            path: PathBuf::from("designs/a.vrdp"),
            result: Err("in use".to_owned()),
        },
        Response::Flushed,
    ]
}

/// Every message, both ways, encoded, and whether it's to the worker.
fn encoded() -> Vec<(Vec<u8>, bool)> {
    let to_worker = requests()
        .into_iter()
        .enumerate()
        .map(|(seq, request)| ToWorker {
            seq: seq as u64,
            request,
        })
        .map(|message| (encode(&message, MAX_MESSAGE_BYTES).unwrap(), true));
    let replies = responses()
        .into_iter()
        .map(|response| Reply {
            seq: u64::MAX,
            response,
        })
        .map(|message| (encode(&message, MAX_MESSAGE_BYTES).unwrap(), false));
    to_worker.chain(replies).collect()
}

/// Decodes `bytes` as a message to the worker if `to_worker`, otherwise as
/// one to the page, returning whether that worked.
fn decodes(bytes: &[u8], to_worker: bool) -> bool {
    if to_worker {
        decode::<ToWorker>(bytes).is_ok()
    } else {
        decode::<Reply>(bytes).is_ok()
    }
}

#[test]
fn every_request_round_trips() {
    for (seq, request) in requests().into_iter().enumerate() {
        // What the page sends, borrowing the request.
        let borrowed = ToWorker {
            seq: seq as u64,
            request: &request,
        };
        let bytes = encode(&borrowed, MAX_MESSAGE_BYTES).unwrap();
        let message = ToWorker {
            seq: seq as u64,
            request,
        };
        assert_eq!(bytes, encode(&message, MAX_MESSAGE_BYTES).unwrap());
        assert_eq!(
            format!("{:?}", decode::<ToWorker>(&bytes).unwrap()),
            format!("{message:?}")
        );
    }
}

#[test]
fn every_response_round_trips() {
    for response in responses() {
        let message = Reply { seq: 3, response };
        assert_eq!(
            format!("{:?}", round_trip(&message)),
            format!("{message:?}")
        );
    }
}

/// A revision crosses as the number it is.
#[test]
fn a_revision_is_its_number() {
    for number in [0, 1, u64::MAX] {
        assert_eq!(
            postcard::to_stdvec(&Revision::from(number)).unwrap(),
            postcard::to_stdvec(&number).unwrap()
        );
    }
}

#[test]
fn documents_arrive_whole() {
    let document = hidden();
    let message = ToWorker {
        seq: 0,
        request: Request::KeepDownload {
            file: FileId(0),
            revision: 1.into(),
            document: Arc::new(document.clone()),
        },
    };
    let ToWorker {
        request: Request::KeepDownload { document: got, .. },
        ..
    } = round_trip(&message)
    else {
        panic!("not a download kept");
    };
    assert_eq!(*got, document);
}

/// `message` with the document `placeholder` in it, which it must hold
/// once, as the bytes of [`Document::to_postcard`], swapped for `document`.
/// Both are shorter than 128 bytes, so their lengths take one byte.
fn swap_document(message: &[u8], placeholder: &Document, document: &[u8]) -> Vec<u8> {
    let old = placeholder.to_postcard();
    let old = [&[u8::try_from(old.len()).unwrap()], &old[..]].concat();
    let new = [&[u8::try_from(document.len()).unwrap()], document].concat();
    let at = message
        .windows(old.len())
        .position(|w| w == old)
        .expect("the placeholder is in the message");
    [&message[..at], &new[..], &message[at + old.len()..]].concat()
}

/// A document is checked as it's decoded, like one read from a file.
#[test]
fn a_document_that_fails_its_checks_is_refused() {
    // Two copies of a sketch, spliced into a design's bytes since a
    // document's fields are private and one can't be decoded unchecked: no
    // bodies, the feature count, the feature, millimetres, the tolerance
    // and the next id.
    let sketched = hidden().to_postcard();
    let [0, 1, rest @ ..] = &sketched[..] else {
        panic!("not the design's bytes: {sketched:?}");
    };
    let (feature, tail) = rest.split_at(rest.len() - 10);
    assert_eq!(tail[0], 0, "millimetres");
    assert_eq!(tail[9], 1, "the next id");
    let twins = [&[0, 2], feature, feature, tail].concat();
    assert!(Document::from_postcard(&twins).is_err());
    let bytes = encode(
        &ToWorker {
            seq: 0,
            request: Request::AutoSave {
                file: FileId(0),
                revision: 1.into(),
                document: Arc::new(hidden()),
            },
        },
        MAX_MESSAGE_BYTES,
    )
    .unwrap();
    assert!(decode::<ToWorker>(&bytes).is_ok());
    let bytes = swap_document(&bytes, &hidden(), &twins);
    assert!(matches!(decode::<ToWorker>(&bytes), Err(Error::Decode(_))));

    let bytes = encode(
        &Reply {
            seq: 0,
            response: Response::Opened {
                id: OpenId(0),
                path: None,
                result: Ok(Opened {
                    file: FileId(0),
                    document: Document::example(),
                    access: Access::Edit,
                    recovered: Ok(Some(Offer {
                        document: hidden(),
                        design_changed: false,
                    })),
                    downloaded: false,
                }),
            },
        },
        MAX_MESSAGE_BYTES,
    )
    .unwrap();
    assert!(decode::<Reply>(&bytes).is_ok());
    let bytes = swap_document(&bytes, &hidden(), &twins);
    assert!(matches!(decode::<Reply>(&bytes), Err(Error::Decode(_))));
}

#[test]
fn a_message_cut_short_is_refused() {
    for (bytes, to_worker) in encoded() {
        for cut in 0..bytes.len() {
            let result = std::panic::catch_unwind(|| decodes(&bytes[..cut], to_worker));
            assert!(!result.expect("decoding panicked"), "cut at {cut}");
        }
    }
}

#[test]
fn bytes_after_a_message_are_refused() {
    for (mut bytes, to_worker) in encoded() {
        bytes.push(0);
        let error = if to_worker {
            decode::<ToWorker>(&bytes).unwrap_err()
        } else {
            decode::<Reply>(&bytes).unwrap_err()
        };
        assert_eq!(
            error.to_string(),
            "couldn't decode the message: 1 bytes after the end"
        );
        let source = std::error::Error::source(&error).expect("the decode error");
        assert_eq!(source.to_string(), "1 bytes after the end");
    }
}

/// Flipped bytes and noise are refused or decode to something else, but
/// never panic, nor allocate what a length in them claims.
#[test]
fn damaged_messages_never_panic() {
    for (bytes, to_worker) in encoded() {
        for at in 0..bytes.len() {
            for flip in [0x01, 0x80, 0xff] {
                let mut damaged = bytes.clone();
                damaged[at] ^= flip;
                let result = std::panic::catch_unwind(|| decodes(&damaged, to_worker));
                assert!(result.is_ok(), "flipping {flip:#x} at {at} panicked");
            }
        }
    }
    // A cheap generator, the same every run.
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    for _ in 0..20_000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let len = (state % 64) as usize;
        let noise: Vec<u8> = (0..len)
            .map(|i| (state.rotate_left(i as u32 * 8) & 0xff) as u8)
            .collect();
        let result = std::panic::catch_unwind(|| decodes(&noise, true) || decodes(&noise, false));
        assert!(result.is_ok(), "{noise:?} panicked");
    }
}

/// A length claiming more than the message holds is refused, not
/// allocated.
#[test]
fn a_huge_claimed_length_is_refused() {
    // `WriteRecent` with u64::MAX entries: the variant's index, then the
    // count as a varint.
    let index = encode(
        &ToWorker {
            seq: 0,
            request: Request::WriteRecent {
                entries: Vec::new(),
            },
        },
        MAX_MESSAGE_BYTES,
    )
    .unwrap();
    let mut bytes = index[..index.len() - 1].to_vec();
    bytes.extend([0xff; 9]);
    bytes.push(0x01);
    assert!(decode::<ToWorker>(&bytes).is_err());
}

#[test]
fn a_message_too_large_is_refused() {
    let message = Reply {
        seq: 0,
        response: Response::Flushed,
    };
    let len = encode(&message, MAX_MESSAGE_BYTES).unwrap().len();
    assert!(encode(&message, len).is_ok());
    assert_eq!(encode(&message, len - 1), Err(Error::TooLarge(len)));
}

/// A path that isn't Unicode can't cross, which the web's never are: it's
/// an error, not a panic.
#[cfg(unix)]
#[test]
fn a_path_that_is_not_unicode_is_an_error() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let message = ToWorker {
        seq: 0,
        request: Request::Open {
            id: OpenId(0),
            from: Chosen::Path(PathBuf::from(OsStr::from_bytes(b"\xff.vrdp"))),
        },
    };
    assert!(matches!(
        encode(&message, MAX_MESSAGE_BYTES),
        Err(Error::Encode(_))
    ));
}

/// A response too large to post fails its request alone, rather than
/// the worker that couldn't send it.
#[test]
fn a_response_too_large_fails_its_request() {
    let request = Request::Open {
        id: OpenId(1),
        from: Chosen::Path(PathBuf::from("designs/a.vrdp")),
    };
    let response = responses().swap_remove(0);
    let len = encode(
        &Reply {
            seq: 7,
            response: response.clone(),
        },
        MAX_MESSAGE_BYTES,
    )
    .unwrap()
    .len();

    let bytes = encode_response(7, response.clone(), request.failure(), len).unwrap();
    let Reply {
        seq: 7,
        response: sent,
    } = decode(&bytes).unwrap()
    else {
        panic!("not the response");
    };
    assert_eq!(format!("{sent:?}"), format!("{response:?}"));

    let bytes = encode_response(7, response, request.failure(), len - 1).unwrap();
    let Reply {
        seq: 7,
        response: Response::Opened { id, path, result },
    } = decode(&bytes).unwrap()
    else {
        panic!("not a failed open");
    };
    assert_eq!(id, OpenId(1));
    assert_eq!(path, Some(PathBuf::from("designs/a.vrdp")));
    assert_eq!(result.unwrap_err(), Error::TooLarge(len).to_string());
}
