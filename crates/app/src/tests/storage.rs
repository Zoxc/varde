//! Designs kept in browser storage, as on the web: Save and Save As keep
//! them there by name, through the app's own dialog; Download is a command
//! of its own, recorded; a file from a file input is copied in; the
//! welcome screen lists them. Natively the lane has no browser storage, so
//! the app is tested against its requests and the answers the web's lane
//! gives.

use varde_io::{BrowserDesign, DownloadStatus, LastDownload};
use varde_view::SavePlace;

use super::damaged::welcome_texts;
use super::*;

/// An app as on the web, its new design's store entry made as file 9.
fn on_the_web() -> (Varde, Rc<RefCell<Vec<IoRequest>>>) {
    let (mut varde, requests) = with_new_design();
    varde.files.browser_storage = true;
    (varde, requests)
}

fn file(varde: &mut Varde, message: File) {
    let _ = varde.update(Message::Ui(Ui::File(message)));
}

/// The name the Save As dialog shows, if it shows.
fn naming(varde: &Varde) -> Option<String> {
    document(varde).naming().map(|naming| naming.name.clone())
}

/// A design in browser storage as listed, standing as `download`.
fn listed(name: &str, download: DownloadStatus) -> BrowserDesign {
    BrowserDesign {
        name: name.to_owned(),
        saved: Some(varde_io::UnixSeconds(1)),
        sum: None,
        thumbnail: None,
        download,
        unsaved: false,
        in_use: false,
        damage: None,
    }
}

/// The Save As to browser storage in `sent`, its name and whether it
/// replaces one.
fn stored_save_as(sent: &[IoRequest]) -> (String, bool, Revision) {
    match sent {
        [
            IoRequest::SaveAs {
                file: Some(FileId(9)),
                to: SaveTo::Browser { name, overwrite },
                revision,
                ..
            },
        ] => (name.clone(), *overwrite, *revision),
        sent => panic!("not one Save As to browser storage: {sent:?}"),
    }
}

/// Saves the new design as `bracket.vrdp` in browser storage through the
/// dialog, and answers it: file 9 from then on.
fn saved_as_bracket(varde: &mut Varde, requests: &RefCell<Vec<IoRequest>>) {
    file(varde, File::Save);
    file(varde, File::Name("bracket".to_owned()));
    file(varde, File::ConfirmName);
    let (name, _, revision) = stored_save_as(&sent(requests));
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(9)),
        to: Chosen::Browser(name),
        revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(9),
            access: Access::Edit,
            offered: false,
        }),
    }));
}

/// The first Save of a new design is a Save As, asking for a name in the
/// app's own dialog, empty to start with as the design has no name, its
/// field focused, saying where it will be saved: browser storage has no
/// picker of the system's. Without a name it can't be saved there. The design is kept
/// there by that name from then on, and Save appends to it. The browser
/// is asked to keep its storage for good, once.
#[test]
fn saving_a_new_design_asks_for_a_name_and_keeps_it_in_browser_storage() {
    let (mut varde, requests) = on_the_web();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    sent(&requests);
    // Kept nowhere yet: no name, nor a bar saying where.
    let shown = screen_texts(document(&varde));
    assert!(shown.iter().any(|text| text == "Not saved"), "{shown:?}");
    assert!(
        !shown.iter().any(|text| text == "In browser storage"),
        "{shown:?}"
    );
    file(&mut varde, File::Save);
    assert_eq!(naming(&varde).as_deref(), Some(""));
    assert_eq!(document(&varde).dialog(), Some(Dialog::Naming));
    let shown = screen_texts(document(&varde));
    assert!(
        shown
            .iter()
            .any(|text| text == "Saved in this browser's storage."),
        "{shown:?}"
    );
    for name in ["", "  "] {
        file(&mut varde, File::Name(name.to_owned()));
        file(&mut varde, File::ConfirmName);
        assert_eq!(naming(&varde).as_deref(), Some(name));
    }
    // Keys don't act behind it: the name is typed.
    assert!(document(&varde).keys().is_none());
    // The app had its field take the focus as it showed.
    assert!(!varde.screen.doc_mut().unwrap().take_name_focus());
    assert!(sent(&requests).is_empty());
    // Saving again while it shows does nothing more.
    file(&mut varde, File::Save);
    file(&mut varde, File::Name("bracket/1".to_owned()));
    file(&mut varde, File::ConfirmName);
    assert_eq!(naming(&varde), None);
    let (name, overwrite, revision) = stored_save_as(&sent(&requests));
    assert_eq!((name.as_str(), overwrite), ("bracket_1.vrdp", false));
    // Asked as the app took the step, once.
    assert!(varde.files.storage.asked);
    assert!(!varde.files.take_ask_persist());

    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(9)),
        to: Chosen::Browser(name.clone()),
        revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(9),
            access: Access::Edit,
            offered: false,
        }),
    }));
    let doc = document(&varde);
    assert_eq!(doc.name, "bracket_1");
    assert_eq!(
        *doc.target(),
        Target::Browser {
            file: FileId(9),
            name
        }
    );
    assert!(!doc.edited());
    assert_eq!(doc.title_name(), "bracket_1.vrdp");
    assert_eq!(
        doc.state(false, Mode::Light, Default::default(), Default::default())
            .location,
        Some(varde_view::Location::Browser)
    );
    // The bar under the file cell says so; the name is the design's.
    let shown = screen_texts(doc);
    for text in ["In browser storage", "bracket_1"] {
        assert!(
            shown.iter().any(|shown| shown == text),
            "no {text:?} in {shown:?}"
        );
    }
    assert!(!shown.iter().any(|text| text == "Not saved"), "{shown:?}");
    // Listed again, the name taken now.
    assert!(matches!(sent(&requests)[..], [IoRequest::ListBrowser]));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    file(&mut varde, File::Save);
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::Save {
            file: FileId(9),
            ..
        }]
    ));
}

/// Saving as a name a design in browser storage has asks about replacing
/// it first: saving again replaces it. Changing the name asks no more.
#[test]
fn saving_as_a_name_in_use_asks_before_replacing_it() {
    let (mut varde, requests) = on_the_web();
    varde.files.browser = vec![listed("bracket.vrdp", DownloadStatus::Never)];
    sent(&requests);
    file(&mut varde, File::SaveAs);
    file(&mut varde, File::Name("bracket".to_owned()));
    file(&mut varde, File::ConfirmName);
    assert!(sent(&requests).is_empty());
    let replacing = document(&varde).naming().unwrap().replacing.clone();
    assert_eq!(replacing.as_deref(), Some("bracket.vrdp"));
    let shown = screen_texts(document(&varde));
    assert!(shown.iter().any(|text| text == "Replace"), "{shown:?}");
    file(&mut varde, File::ConfirmName);
    let (name, overwrite, _) = stored_save_as(&sent(&requests));
    assert_eq!((name.as_str(), overwrite), ("bracket.vrdp", true));

    // Another name, free, saves at once.
    let (mut varde, requests) = on_the_web();
    varde.files.browser = vec![listed("bracket.vrdp", DownloadStatus::Never)];
    sent(&requests);
    file(&mut varde, File::SaveAs);
    file(&mut varde, File::Name("bracket".to_owned()));
    file(&mut varde, File::ConfirmName);
    file(&mut varde, File::Name("lid".to_owned()));
    file(&mut varde, File::ConfirmName);
    let (name, overwrite, _) = stored_save_as(&sent(&requests));
    assert_eq!((name.as_str(), overwrite), ("lid.vrdp", false));
}

/// Closing with changes, choosing to save, the dialog asks for a name;
/// cancelling it stays, as backing out of the system's dialog does.
#[test]
fn cancelling_the_name_backs_out_of_leaving() {
    let (mut varde, requests) = on_the_web();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    file(&mut varde, File::CloseDocument);
    file(&mut varde, File::Unsaved(Unsaved::Save));
    assert!(naming(&varde).is_some());
    sent(&requests);
    let _ = varde.update(escape(&varde));
    assert_eq!(naming(&varde), None);
    assert!(document(&varde).leaving().is_none());
    assert!(sent(&requests).is_empty());
    // Closing again asks again; saved by name, it closes.
    file(&mut varde, File::CloseDocument);
    file(&mut varde, File::Unsaved(Unsaved::Save));
    file(&mut varde, File::Name("lid".to_owned()));
    file(&mut varde, File::ConfirmName);
    let (name, _, revision) = stored_save_as(&sent(&requests));
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(9)),
        to: Chosen::Browser(name),
        revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(9),
            access: Access::Edit,
            offered: false,
        }),
    }));
    assert!(is_welcome(&varde));
    assert!(matches!(
        sent(&requests)[..],
        [
            IoRequest::ListBrowser,
            IoRequest::Close {
                file: FileId(9),
                closing: Closing::Clean
            },
            IoRequest::ListBrowser
        ]
    ));
}

/// What `Esc` sends with the open document's dialog showing.
fn escape(varde: &Varde) -> Message {
    let event = keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(key::Named::Escape),
        modified_key: keyboard::Key::Named(key::Named::Escape),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    };
    escape_key((document(varde).dialog(), event)).unwrap()
}

/// Where the File System Access API is, the dialog offers a file on the
/// computer, saying the user chooses where next, which goes on to the
/// system's save picker; with no name typed too, the picker suggesting
/// none.
#[test]
fn the_dialog_offers_a_file_on_the_computer() {
    let (mut varde, requests) = on_the_web();
    sent(&requests);
    file(&mut varde, File::SaveAs);
    file(&mut varde, File::Place(SavePlace::Computer));
    let shown = screen_texts(document(&varde));
    let next = "Saved to a file on your computer: you choose where next.";
    assert!(shown.iter().any(|text| text == next), "{shown:?}");
    file(&mut varde, File::ConfirmName);
    assert_eq!(naming(&varde), None);
    assert_eq!(document(&varde).picking(), Some(Picking::SaveAs));
    assert!(sent(&requests).is_empty());
    assert!(!varde.files.take_ask_persist());
}

/// The names of the files a test's downloader was handed.
type Downloaded = Rc<RefCell<Vec<String>>>;

/// A design saved in browser storage, file 9 named `bracket.vrdp`, with a
/// downloader that keeps what it's handed.
fn stored_with_downloads() -> (Varde, Rc<RefCell<Vec<IoRequest>>>, Downloaded) {
    let (mut varde, requests) = on_the_web();
    saved_as_bracket(&mut varde, &requests);
    sent(&requests);
    let downloaded = Rc::<RefCell<Vec<String>>>::default();
    let kept = Rc::clone(&downloaded);
    varde.files.downloader = Some(Box::new(move |name, bytes| {
        assert!(varde_io::vrdp::from_bytes(bytes).is_ok());
        kept.borrow_mut().push(name.to_owned());
        Ok(())
    }));
    (varde, requests, downloaded)
}

/// Download hands the design over as `<name>.vrdp` and records it, which
/// changes nothing in storage nor whether it's saved: the file menu says
/// it's the latest downloaded, till it's edited or saved since.
#[test]
fn download_is_a_command_of_its_own_and_recorded() {
    let (mut varde, requests, downloaded) = stored_with_downloads();
    let status = |varde: &Varde| document(varde).download_status();
    assert_eq!(status(&varde), Some(varde_view::Downloads::Never));
    file(&mut varde, File::Download);
    assert_eq!(*downloaded.borrow(), ["bracket.vrdp"]);
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::RecordDownload {
            file: FileId(9),
            edited: false
        }]
    ));
    assert!(matches!(
        status(&varde),
        Some(varde_view::Downloads::Latest(Some(_)))
    ));
    assert!(!document(&varde).edited());
    let _ = varde.update(Message::Io(IoResponse::DownloadRecorded {
        file: FileId(9),
        result: Ok(Some(LastDownload {
            time: varde_io::UnixSeconds(1),
            latest: true,
        })),
    }));
    assert!(document(&varde).notice.is_none());

    // Edited since: changed since downloaded, and a download of the
    // changes isn't of the design as saved.
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    assert!(matches!(
        status(&varde),
        Some(varde_view::Downloads::Changed(Some(_)))
    ));
    file(&mut varde, File::Download);
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::RecordDownload {
            file: FileId(9),
            edited: true
        }]
    ));
    assert!(matches!(
        status(&varde),
        Some(varde_view::Downloads::Changed(Some(_)))
    ));
    // A failure to record it says so.
    let _ = varde.update(Message::Io(IoResponse::DownloadRecorded {
        file: FileId(9),
        result: Err("full".to_owned()),
    }));
    assert!(document(&varde).notice.as_deref().unwrap().contains("full"));
}

/// A new design, never saved, downloads too, with nothing to record: it's
/// not in browser storage.
#[test]
fn a_new_design_downloads_without_a_record() {
    let (mut varde, requests) = on_the_web();
    let downloaded = Rc::<RefCell<Vec<String>>>::default();
    let kept = Rc::clone(&downloaded);
    varde.files.downloader = Some(Box::new(move |name, _| {
        kept.borrow_mut().push(name.to_owned());
        Ok(())
    }));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    sent(&requests);
    let saved = document(&varde).saved_revision();
    file(&mut varde, File::Download);
    assert_eq!(*downloaded.borrow(), ["Untitled.vrdp"]);
    assert!(sent(&requests).is_empty());
    assert_eq!(document(&varde).download_status(), None);
    // Nor does a download count as saving.
    assert_eq!(document(&varde).saved_revision(), saved);
    assert!(document(&varde).edited());
}

/// Rename asks for the name in the same dialog, refused for one taken,
/// and the design goes by the new name once the lane has renamed it.
#[test]
fn renaming_a_design_in_browser_storage() {
    let (mut varde, requests, _) = stored_with_downloads();
    varde.files.browser = vec![listed("lid.vrdp", DownloadStatus::Never)];
    file(&mut varde, File::Rename);
    assert_eq!(naming(&varde).as_deref(), Some("bracket"));
    file(&mut varde, File::Name("lid".to_owned()));
    file(&mut varde, File::ConfirmName);
    // Taken: said so, never replaced.
    assert!(sent(&requests).is_empty());
    file(&mut varde, File::ConfirmName);
    assert!(sent(&requests).is_empty());
    file(&mut varde, File::Name("washer".to_owned()));
    file(&mut varde, File::ConfirmName);
    assert!(matches!(
        &sent(&requests)[..],
        [IoRequest::Rename { file: FileId(9), name }] if name == "washer.vrdp"
    ));
    let _ = varde.update(Message::Io(IoResponse::Renamed {
        file: FileId(9),
        name: "washer.vrdp".to_owned(),
        result: Ok(()),
    }));
    let doc = document(&varde);
    assert_eq!(doc.name, "washer");
    assert!(matches!(doc.target(), Target::Browser { name, .. } if name == "washer.vrdp"));
    // A failure says why in the status bar.
    let _ = varde.update(Message::Io(IoResponse::Renamed {
        file: FileId(9),
        name: "x.vrdp".to_owned(),
        result: Err("x.vrdp is open elsewhere".to_owned()),
    }));
    assert!(
        document(&varde)
            .notice
            .as_deref()
            .unwrap()
            .contains("open elsewhere")
    );
    assert_eq!(document(&varde).name, "washer");
}

/// A file from a file input is copied into browser storage by the lane,
/// under its name made unique, and opens from there, saved, the status
/// bar saying so.
#[test]
fn a_file_from_a_file_input_opens_copied_into_browser_storage() {
    let (mut varde, requests) = with_files();
    varde.files.browser_storage = true;
    let input = Picked {
        id: 3,
        name: "bracket.vrdp".to_owned(),
        from: PickedFrom::Input,
    };
    let _ = varde.update(Message::Picked(Some(Chosen::File(input))));
    let Some(IoRequest::Open { id, .. }) = sent(&requests).pop() else {
        panic!("not opened");
    };
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: None,
        result: Ok(Opened {
            file: FileId(0),
            document: with_a_line(),
            access: Access::Edit,
            recovered: Ok(None),
            browser: Some("bracket (2).vrdp".to_owned()),
            not_copied: None,
            download: None,
            damage: None,
        }),
    }));
    let doc = document(&varde);
    assert_eq!(doc.name, "bracket (2)");
    assert_eq!(
        *doc.target(),
        Target::Browser {
            file: FileId(0),
            name: "bracket (2).vrdp".to_owned()
        }
    );
    assert!(!doc.edited());
    assert_eq!(
        doc.notice.as_deref(),
        Some("Copied to browser storage as bracket (2).vrdp")
    );
    assert_eq!(doc.download_status(), Some(varde_view::Downloads::Never));
}

/// A design in browser storage opens from the welcome screen by its name,
/// with its last download as the lane read it.
#[test]
fn a_design_in_browser_storage_opens_with_its_downloads() {
    let (mut varde, requests) = with_files();
    varde.files.browser_storage = true;
    let _ = varde.update(Message::Io(IoResponse::BrowserListed {
        designs: vec![listed("lid.vrdp", DownloadStatus::Never)],
    }));
    let texts = welcome_texts(&varde);
    assert!(texts.iter().any(|text| text == "lid"), "{texts:?}");
    assert!(
        texts.iter().any(|text| text == "Never downloaded"),
        "{texts:?}"
    );
    sent(&requests);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenFromBrowser(
        "lid.vrdp".to_owned(),
    ))));
    let Some(IoRequest::Open {
        id,
        from: Chosen::Browser(name),
    }) = sent(&requests).pop()
    else {
        panic!("not opened by name");
    };
    assert_eq!(name, "lid.vrdp");
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: None,
        result: Ok(Opened {
            file: FileId(4),
            document: with_a_line(),
            access: Access::ReadOnly(ReadOnly::InUse),
            recovered: Ok(None),
            browser: Some(name),
            not_copied: None,
            download: Some(LastDownload {
                time: varde_io::UnixSeconds(1),
                latest: true,
            }),
            damage: None,
        }),
    }));
    let doc = document(&varde);
    assert_eq!(doc.name, "lid");
    assert!(!doc.editable());
    assert!(matches!(
        doc.download_status(),
        Some(varde_view::Downloads::Latest(Some(_)))
    ));
}

/// Deleting a design whose latest is downloaded goes at once; one never
/// downloaded, or changed since, is asked about first.
#[test]
fn deleting_a_design_not_downloaded_asks_first() {
    let (mut varde, requests) = with_files();
    varde.files.browser_storage = true;
    let _ = varde.update(Message::Io(IoResponse::BrowserListed {
        designs: vec![
            listed("a.vrdp", DownloadStatus::Latest(varde_io::UnixSeconds(1))),
            listed("b.vrdp", DownloadStatus::Never),
        ],
    }));
    sent(&requests);
    let welcome = |varde: &Varde| match &varde.screen {
        Screen::Welcome(welcome) => welcome.deleting().map(str::to_owned),
        Screen::Document(_) => panic!("not on the welcome screen"),
    };
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::DeleteFromBrowser(
        "a.vrdp".to_owned(),
    ))));
    assert!(matches!(
        &sent(&requests)[..],
        [IoRequest::DeleteFromBrowser { name }] if name == "a.vrdp"
    ));
    assert_eq!(varde.files.browser.len(), 1);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::DeleteFromBrowser(
        "b.vrdp".to_owned(),
    ))));
    assert_eq!(welcome(&varde).as_deref(), Some("b.vrdp"));
    assert!(sent(&requests).is_empty());
    let texts = welcome_texts(&varde);
    assert!(
        texts.iter().any(|text| text == "Delete b.vrdp?"),
        "{texts:?}"
    );
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::CancelDelete)));
    assert_eq!(welcome(&varde), None);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::DeleteFromBrowser(
        "b.vrdp".to_owned(),
    ))));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::ConfirmDelete)));
    assert!(matches!(
        &sent(&requests)[..],
        [IoRequest::DeleteFromBrowser { name }] if name == "b.vrdp"
    ));
    assert!(varde.files.browser.is_empty());
    // The lane lists what's left after; a failure says so.
    let _ = varde.update(Message::Io(IoResponse::DeletedFromBrowser {
        name: "b.vrdp".to_owned(),
        result: Err("it's open".to_owned()),
    }));
    assert!(matches!(sent(&requests)[..], [IoRequest::ListBrowser]));
    let Screen::Welcome(welcome) = &varde.screen else {
        panic!("not on the welcome screen");
    };
    assert!(welcome.error().unwrap().contains("it's open"));
}

/// Download from the welcome screen reads the design as saved from the
/// lane, which records it, and hands it over.
#[test]
fn a_design_downloads_from_the_welcome_screen() {
    let (mut varde, requests) = with_files();
    varde.files.browser_storage = true;
    let downloaded = Rc::<RefCell<Vec<(String, usize)>>>::default();
    let kept = Rc::clone(&downloaded);
    varde.files.downloader = Some(Box::new(move |name, bytes| {
        kept.borrow_mut().push((name.to_owned(), bytes.len()));
        Ok(())
    }));
    sent(&requests);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::DownloadFromBrowser(
        "a.vrdp".to_owned(),
    ))));
    assert!(matches!(
        &sent(&requests)[..],
        [IoRequest::DownloadFromBrowser { name }] if name == "a.vrdp"
    ));
    let _ = varde.update(Message::Io(IoResponse::DownloadedFromBrowser {
        name: "a.vrdp".to_owned(),
        result: Ok(vec![7; 12]),
        not_recorded: None,
    }));
    assert_eq!(*downloaded.borrow(), [("a.vrdp".to_owned(), 12)]);
    // Listed again, with the download recorded.
    assert!(matches!(sent(&requests)[..], [IoRequest::ListBrowser]));
}

/// Natively nothing is kept in browser storage: Save As shows the
/// system's dialog, the File menu offers no Download, and nothing is
/// listed.
#[test]
fn natively_save_as_shows_the_system_dialog() {
    let (mut varde, requests) = with_new_design();
    sent(&requests);
    file(&mut varde, File::SaveAs);
    assert_eq!(naming(&varde), None);
    assert_eq!(document(&varde).picking(), Some(Picking::SaveAs));
    let state = document(&varde).state(false, Mode::Light, Default::default(), Default::default());
    assert!(!state.downloadable && state.rename.is_none() && state.location.is_none());
    assert!(sent(&requests).is_empty());
}

/// Whatever is in browser storage, the welcome screen says the browser may
/// clear it unless it keeps it for good, and how much is used.
#[test]
fn the_welcome_screen_says_what_the_browser_keeps() {
    let (mut varde, _requests) = with_files();
    varde.files.browser_storage = true;
    let _ = varde.update(Message::StorageState {
        persisted: Some(false),
        space: Some(varde_io::storage::Space {
            used: 1_500_000,
            quota: 2_000_000_000,
        }),
    });
    let _ = varde.update(Message::Io(IoResponse::BrowserListed {
        designs: vec![listed("a.vrdp", DownloadStatus::Never)],
    }));
    let texts = welcome_texts(&varde);
    assert!(
        texts
            .iter()
            .any(|text| text == "This browser may clear what's kept in it")
    );
    assert!(texts.iter().any(|text| text == "1.5 MB of 2.0 GB used"));
    let _ = varde.update(Message::Persisted(Some(true)));
    let texts = welcome_texts(&varde);
    assert!(
        !texts
            .iter()
            .any(|text| text == "This browser may clear what's kept in it")
    );
}

/// Saving as the design's own name in the dialog is a Save: appended to
/// it, its history kept, never written over it whole.
#[test]
fn saving_as_its_own_name_saves() {
    let (mut varde, requests, _) = stored_with_downloads();
    varde.files.browser = vec![listed("bracket.vrdp", DownloadStatus::Never)];
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    sent(&requests);
    file(&mut varde, File::SaveAs);
    assert_eq!(naming(&varde).as_deref(), Some("bracket"));
    file(&mut varde, File::ConfirmName);
    assert_eq!(naming(&varde), None);
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::Save {
            file: FileId(9),
            ..
        }]
    ));
}

/// A design in browser storage opened past damage isn't saved to: Save
/// asks for a name, and its own replaces it whole, only once the user
/// agrees, its history gone.
#[test]
fn saving_a_damaged_design_as_its_own_name_asks_first() {
    let (mut varde, requests, _) = stored_with_downloads();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    file(&mut varde, File::Save);
    let Some(IoRequest::Save { revision, .. }) = sent(&requests).pop() else {
        panic!("not saved");
    };
    let _ = varde.update(Message::Io(IoResponse::Saved {
        file: FileId(9),
        revision,
        result: Err(SaveError::OpenedDamaged),
    }));
    assert_eq!(naming(&varde).as_deref(), Some("bracket"));
    file(&mut varde, File::ConfirmName);
    assert!(sent(&requests).is_empty());
    let replacing = document(&varde).naming().unwrap().replacing.clone();
    assert_eq!(replacing.as_deref(), Some("bracket.vrdp"));
    file(&mut varde, File::ConfirmName);
    let (name, overwrite, _) = stored_save_as(&sent(&requests));
    assert_eq!((name.as_str(), overwrite), ("bracket.vrdp", true));
}

/// Another tab saved a design of the name since the app listed them: the
/// lane says it's taken, and the dialog asks about replacing it, as it
/// would have; nothing is said to have failed.
#[test]
fn a_name_taken_elsewhere_asks_before_replacing_it() {
    let (mut varde, requests) = on_the_web();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    sent(&requests);
    file(&mut varde, File::SaveAs);
    file(&mut varde, File::Name("lid".to_owned()));
    file(&mut varde, File::ConfirmName);
    let (name, overwrite, revision) = stored_save_as(&sent(&requests));
    assert_eq!((name.as_str(), overwrite), ("lid.vrdp", false));
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(9)),
        to: Chosen::Browser(name),
        revision,
        result: Err(SaveError::Taken),
    }));
    assert!(matches!(sent(&requests)[..], [IoRequest::ListBrowser]));
    let doc = document(&varde);
    assert_eq!(doc.save_error(), None);
    assert!(!doc.saving() && doc.edited());
    assert_eq!(naming(&varde).as_deref(), Some("lid"));
    let replacing = document(&varde).naming().unwrap().replacing.clone();
    assert_eq!(replacing.as_deref(), Some("lid.vrdp"));
    file(&mut varde, File::ConfirmName);
    let (name, overwrite, _) = stored_save_as(&sent(&requests));
    assert_eq!((name.as_str(), overwrite), ("lid.vrdp", true));
}

/// Downloaded while a save is on its way, the download isn't recorded as
/// the design as saved: the save may yet fail.
#[test]
fn a_download_while_saving_is_of_changes() {
    let (mut varde, requests, downloaded) = stored_with_downloads();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    file(&mut varde, File::Save);
    sent(&requests);
    file(&mut varde, File::Download);
    assert_eq!(*downloaded.borrow(), ["bracket.vrdp"]);
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::RecordDownload {
            file: FileId(9),
            edited: true
        }]
    ));
}

/// Rename… shows for a design in browser storage, offered only while it
/// may be renamed: not as it's saved.
#[test]
fn rename_waits_for_saves() {
    let (mut varde, _requests, _) = stored_with_downloads();
    let rename = |varde: &Varde| {
        let offers = varde.files.offers();
        (document(varde).state(false, Mode::Light, Default::default(), offers)).rename
    };
    assert_eq!(rename(&varde), Some(true));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    file(&mut varde, File::Save);
    assert_eq!(rename(&varde), Some(false));
    let state =
        (document(&varde)).state(false, Mode::Light, Default::default(), varde.files.offers());
    assert!(state.downloadable);
}

/// The answer to opening a file from a file input.
fn opened_input(varde: &mut Varde, requests: &RefCell<Vec<IoRequest>>, opened: Opened) {
    let input = Picked {
        id: 3,
        name: "bracket.vrdp".to_owned(),
        from: PickedFrom::Input,
    };
    let _ = varde.update(Message::Picked(Some(Chosen::File(input))));
    let Some(IoRequest::Open { id, .. }) = sent(requests).pop() else {
        panic!("not opened");
    };
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: None,
        result: Ok(opened),
    }));
}

/// A file from a file input that couldn't be copied into browser storage,
/// say with the site's data blocked, opens as a copy, as a new design, the
/// status bar saying why; copied in, the browser is asked to keep it.
#[test]
fn a_file_not_copied_into_browser_storage_opens_as_a_copy() {
    let (mut varde, requests) = with_files();
    varde.files.browser_storage = true;
    opened_input(
        &mut varde,
        &requests,
        Opened {
            file: FileId(0),
            document: with_a_line(),
            access: Access::Edit,
            recovered: Ok(None),
            browser: None,
            not_copied: Some("couldn't copy it into browser storage: blocked".to_owned()),
            download: None,
            damage: None,
        },
    );
    let doc = document(&varde);
    assert_eq!(doc.name, "bracket");
    assert_eq!(*doc.target(), Target::Entry { file: FileId(0) });
    assert!(
        doc.notice.as_deref().unwrap().contains("blocked"),
        "{:?}",
        doc.notice
    );
    assert!(!varde.files.storage.asked);

    let (mut varde, requests) = with_files();
    varde.files.browser_storage = true;
    opened_input(
        &mut varde,
        &requests,
        Opened {
            file: FileId(0),
            document: with_a_line(),
            access: Access::Edit,
            recovered: Ok(None),
            browser: Some("bracket.vrdp".to_owned()),
            not_copied: None,
            download: None,
            damage: None,
        },
    );
    assert!(varde.files.storage.asked);
}

/// Saving a design opened from browser storage asks the browser to keep
/// its storage, once a session.
#[test]
fn saving_to_browser_storage_asks_to_keep_it() {
    let (mut varde, requests) = with_files();
    varde.files.browser_storage = true;
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenFromBrowser(
        "lid.vrdp".to_owned(),
    ))));
    let Some(IoRequest::Open { id, .. }) = sent(&requests).pop() else {
        panic!("not opened");
    };
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: None,
        result: Ok(Opened {
            file: FileId(4),
            document: with_a_line(),
            access: Access::Edit,
            recovered: Ok(None),
            browser: Some("lid.vrdp".to_owned()),
            not_copied: None,
            download: None,
            damage: None,
        }),
    }));
    assert!(!varde.files.storage.asked);
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    file(&mut varde, File::Save);
    assert!(varde.files.storage.asked);
}

/// A file from a file input copied in and found damaged, asked about, and
/// not opened after all: the copy goes again.
#[test]
fn a_damaged_copy_not_opened_goes() {
    let (mut varde, requests) = with_files();
    varde.files.browser_storage = true;
    let damage = varde_io::Damage {
        kind: varde_io::DamageKind::Damaged { found: None },
        time: varde_io::UnixSeconds(1),
        unreadable: 10,
    };
    opened_input(
        &mut varde,
        &requests,
        Opened {
            file: FileId(5),
            document: with_a_line(),
            access: Access::Edit,
            recovered: Ok(None),
            browser: Some("bracket (2).vrdp".to_owned()),
            not_copied: None,
            download: None,
            damage: Some(damage),
        },
    );
    assert!(is_welcome(&varde));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::CancelDamaged)));
    assert!(matches!(
        &sent(&requests)[..],
        [
            IoRequest::Close {
                file: FileId(5),
                closing: Closing::Clean
            },
            IoRequest::DeleteFromBrowser { name }
        ] if name == "bracket (2).vrdp"
    ));
}

/// Downloaded from the welcome screen though the download couldn't be
/// recorded: handed over all the same, the screen saying it wasn't
/// recorded.
#[test]
fn a_download_not_recorded_is_downloaded_still() {
    let (mut varde, requests) = with_files();
    varde.files.browser_storage = true;
    let downloaded = Rc::<RefCell<Vec<String>>>::default();
    let kept = Rc::clone(&downloaded);
    varde.files.downloader = Some(Box::new(move |name, _| {
        kept.borrow_mut().push(name.to_owned());
        Ok(())
    }));
    sent(&requests);
    let _ = varde.update(Message::Io(IoResponse::DownloadedFromBrowser {
        name: "a.vrdp".to_owned(),
        result: Ok(vec![7; 12]),
        not_recorded: Some("busy".to_owned()),
    }));
    assert_eq!(*downloaded.borrow(), ["a.vrdp"]);
    assert!(matches!(sent(&requests)[..], [IoRequest::ListBrowser]));
    let Screen::Welcome(welcome) = &varde.screen else {
        panic!("not on the welcome screen");
    };
    assert!(welcome.error().unwrap().contains("busy"));
}

/// A design another tab has open can't be deleted from the welcome
/// screen: its card offers no Delete, and asking does nothing.
#[test]
fn a_design_open_elsewhere_is_not_deleted() {
    let (mut varde, requests) = with_files();
    varde.files.browser_storage = true;
    let _ = varde.update(Message::Io(IoResponse::BrowserListed {
        designs: vec![BrowserDesign {
            in_use: true,
            ..listed("a.vrdp", DownloadStatus::Latest(varde_io::UnixSeconds(1)))
        }],
    }));
    sent(&requests);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::DeleteFromBrowser(
        "a.vrdp".to_owned(),
    ))));
    assert!(sent(&requests).is_empty());
    assert_eq!(varde.files.browser.len(), 1);
}

/// The thumbnails of designs in browser storage are made into handles
/// once per design and save: listed again unchanged, the same handle.
#[test]
fn thumbnails_are_made_once_per_save() {
    let (mut varde, _requests) = with_files();
    let image = |shade| varde_io::thumbnail::Image::new(2, 1, vec![shade; 8]).unwrap();
    let image = varde_io::thumbnail::Thumbnail {
        light: image(9),
        dark: image(3),
    };
    let design = |sum| BrowserDesign {
        sum: Some(sum),
        thumbnail: Some(image.clone()),
        ..listed("a.vrdp", DownloadStatus::Never)
    };
    let handle = |varde: &Varde, sum| {
        (varde.files.browser_thumbnail(&design(sum), Mode::Light))
            .unwrap()
            .id()
    };
    varde.files.browser_listed(vec![design(1)]);
    let first = handle(&varde, 1);
    varde.files.browser_listed(vec![design(1)]);
    assert_eq!(handle(&varde, 1), first);
    varde.files.browser_listed(vec![design(2)]);
    assert_ne!(handle(&varde, 2), first);
    assert!(
        varde
            .files
            .browser_thumbnail(&design(1), Mode::Light)
            .is_none()
    );
    // The dark theme's is its own.
    let dark = varde
        .files
        .browser_thumbnail(&design(2), Mode::Dark)
        .unwrap();
    assert_ne!(dark.id(), handle(&varde, 2));
}
