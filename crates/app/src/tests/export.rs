//! Exporting the visible bodies as a 3MF file from the File menu.

use varde_io::three_mf;

use super::*;
use crate::doc::Exporting;

/// What a test's lane stand-in keeps of the requests sent to it.
type Sent<R> = Rc<RefCell<Vec<R>>>;

/// An app showing `/d/part.vrdp` as [`with_open_file`] does, holding the
/// example's plate, its model shown, with a regeneration lane whose
/// requests wait for the test: the app, the IO requests and the
/// regeneration requests, both lists empty.
fn with_plate() -> (Varde, Sent<IoRequest>, Sent<Request>) {
    let (mut varde, io) = with_open_file();
    let regen = Rc::default();
    let doc = varde.screen.doc_mut().unwrap();
    doc.feed.connect(Deferred(Rc::clone(&regen)));
    doc.apply(Command::Replace(Box::new(Document::example())));
    doc.sync();
    answer(doc, &regen);
    sent(&io);
    (varde, io, regen)
}

/// Answers the regeneration requests waiting, through the app's messages
/// as the lane would.
fn regenerate(varde: &mut Varde, regen: &RefCell<Vec<Request>>) {
    let id = document(varde).id;
    for request in regen.take() {
        let _ = varde.update(Message::Doc(id, ForDoc::Computed(handle(request))));
    }
}

/// Picks `chosen` in the Export dialog, after asking for it.
fn export_to(varde: &mut Varde, chosen: Option<Chosen>) {
    let _ = varde.update(Message::Ui(Ui::File(File::Export)));
    assert_eq!(document(varde).export_state(), Some(&Exporting::Picking));
    let id = document(varde).id;
    let _ = varde.update(Message::Doc(id, ForDoc::ExportPicked(chosen)));
}

/// The texts of `doc`'s screen with the file menu open.
fn menu_texts(doc: &mut Doc) -> Vec<String> {
    doc.file_menu = true;
    let texts = screen_texts(doc);
    doc.file_menu = false;
    texts
}

#[test]
fn export_goes_while_a_body_is_shown_and_regenerating_works() {
    let (mut varde, _, regen) = with_plate();
    let doc = varde.screen.doc_mut().unwrap();
    assert!(doc.exportable());
    assert!(menu_texts(doc).iter().any(|text| text == "Export 3MF…"));
    // A sketch alone has nothing to export.
    let (sketch, _) = holding(with_a_line());
    assert!(!sketch.exportable());
    // Nor before the first model is in.
    let (blank, _) = deferred();
    assert!(!blank.exportable());

    // Hidden, the plate isn't exported.
    let body = doc.editor.document().bodies()[0].id;
    doc.update(Edit::ToggleVisible(body));
    assert!(!doc.exportable());
    doc.update(Edit::ToggleVisible(body));
    answer(doc, &regen);
    assert!(doc.exportable());

    // While regenerating fails, it isn't either: the model shown is old.
    doc.update(Edit::SetTolerance(
        varde_document::Tolerance::new(1e-2).unwrap(),
    ));
    let request = regen.take().pop().unwrap();
    doc.computed(Response::Failed {
        generation: request.generation().unwrap(),
        exclude: None,
        draft: None,
        inspect: None,
        error: "the kernel gave up".to_owned(),
    });
    assert!(matches!(
        doc.feed.status(&doc.editor),
        MeshStatus::Failed(_)
    ));
    assert!(!doc.exportable());
    doc.update(Edit::Undo);
    answer(doc, &regen);
    assert!(doc.exportable());
    // Read-only designs export too: nothing of them changes.
    doc.read_only = Some("open elsewhere".to_owned());
    assert!(doc.exportable());
}

#[test]
fn an_export_is_welded_then_written_by_the_io_lane() {
    let (mut varde, io, regen) = with_plate();
    export_to(&mut varde, Some(Chosen::Path("/d/plate".into())));
    // One at a time: the menu item is off while it's on its way.
    assert!(document(&varde).exporting());
    assert!(!document(&varde).exportable());
    let requests = regen.borrow().clone();
    let [
        Request::Export {
            document: asked, ..
        },
    ] = &requests[..]
    else {
        panic!("not one export: {requests:?}");
    };
    assert_eq!(**asked, Document::example());
    assert!(sent(&io).is_empty());

    // An edit before the lane gets to it: regenerated, and the export
    // still answered, of the document as it was asked for.
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::SetTolerance(
        varde_document::Tolerance::new(1e-2).unwrap(),
    ))));
    assert_eq!(regen.borrow().len(), 2);
    let generation = document(&varde).editor.generation();
    regenerate(&mut varde, &regen);
    assert_eq!(document(&varde).feed.generation(), Some(generation));

    let sent_now = sent(&io);
    let [
        IoRequest::Export {
            to: SaveTo::Path { path, overwrite },
            title,
            bodies,
        },
    ] = &sent_now[..]
    else {
        panic!("not one export: {sent_now:?}");
    };
    // The dialog only asked about the name typed, without the extension.
    assert_eq!(path, Path::new("/d/plate.3mf"));
    assert!(!overwrite);
    assert_eq!(title, "part");
    let evaluation = varde_regen::evaluate(&Document::example(), &mut Default::default());
    let welded = varde_regen::export(&Document::example(), &evaluation).unwrap();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].name, welded[0].name);
    assert_eq!(bodies[0].mesh, welded[0].mesh);
    assert_eq!(document(&varde).export_state(), Some(&Exporting::Writing));

    let _ = varde.update(Message::Io(IoResponse::Exported {
        to: Chosen::Path(path.clone()),
        result: Ok(()),
    }));
    let doc = document(&varde);
    assert_eq!(doc.export_state(), None);
    assert_eq!(doc.export_error(), None);
    assert!(doc.exportable());
    // Picked with the extension, a file there was asked about.
    export_to(&mut varde, Some(Chosen::Path("/d/plate.3MF".into())));
    regenerate(&mut varde, &regen);
    assert!(matches!(
        &sent(&io)[..],
        [IoRequest::Export { to: SaveTo::Path { path, overwrite: true }, .. }]
            if path == Path::new("/d/plate.3MF")
    ));
}

#[test]
fn a_failed_export_shows_why_until_dismissed() {
    let (mut varde, io, regen) = with_plate();
    export_to(&mut varde, Some(Chosen::Path("/d/plate.3mf".into())));
    regenerate(&mut varde, &regen);
    sent(&io);
    let _ = varde.update(Message::Io(IoResponse::Exported {
        to: Chosen::Path("/d/plate.3mf".into()),
        result: Err("plate.3mf already exists".to_owned()),
    }));
    let doc = document(&varde);
    assert_eq!(doc.export_error(), Some("plate.3mf already exists"));
    assert!(doc.exportable());
    let texts = screen_texts(doc);
    assert!(
        texts.iter().any(|text| text == "Couldn't export"),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text == "— plate.3mf already exists"),
        "{texts:?}"
    );
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::DismissExportError)));
    assert_eq!(document(&varde).export_error(), None);

    // A body that can't be welded fails before the file is written.
    export_to(&mut varde, Some(Chosen::Path("/d/plate.3mf".into())));
    let Some(Request::Export { export, .. }) = regen.take().pop() else {
        panic!("no export asked for");
    };
    let id = document(&varde).id;
    let error = "Body 1 can't be exported: vertex 0 is out of range".to_owned();
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::Computed(Response::Exported {
            export,
            result: Err(error.clone()),
        }),
    ));
    assert!(sent(&io).is_empty());
    assert_eq!(document(&varde).export_error(), Some(&error[..]));
    // Nor is anything written when nothing is left to: hidden meanwhile.
    export_to(&mut varde, Some(Chosen::Path("/d/plate.3mf".into())));
    let Some(Request::Export { export, .. }) = regen.take().pop() else {
        panic!("no export asked for");
    };
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::Computed(Response::Exported {
            export,
            result: Ok(Vec::new()),
        }),
    ));
    assert!(sent(&io).is_empty());
    assert_eq!(
        document(&varde).export_error(),
        Some("there are no visible bodies to export")
    );
    // And a starting export clears the error.
    export_to(&mut varde, Some(Chosen::Path("/d/plate.3mf".into())));
    assert_eq!(document(&varde).export_error(), None);
}

#[test]
fn a_cancelled_export_does_nothing() {
    let (mut varde, io, regen) = with_plate();
    export_to(&mut varde, None);
    let doc = document(&varde);
    assert_eq!(doc.export_state(), None);
    assert!(doc.exportable());
    assert!(regen.borrow().is_empty());
    assert!(sent(&io).is_empty());
    // A pick for a document closed since, or not asked for, is dropped.
    let _ = varde.update(Message::Doc(
        DocId::unique(),
        ForDoc::ExportPicked(Some(Chosen::Path("/d/x.3mf".into()))),
    ));
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::ExportPicked(Some(Chosen::Path("/d/x.3mf".into()))),
    ));
    assert!(regen.borrow().is_empty());
    // Nor is an answer to an export not asked for taken.
    let _ = varde.update(Message::Io(IoResponse::Exported {
        to: Chosen::Path("/d/x.3mf".into()),
        result: Err("no".to_owned()),
    }));
    assert_eq!(document(&varde).export_error(), None);
}

#[test]
fn without_a_save_picker_the_export_is_downloaded() {
    let (mut varde, io, regen) = with_plate();
    let downloaded: Sent<(String, Vec<u8>)> = Rc::default();
    let into = Rc::clone(&downloaded);
    varde.files.downloader = Some(Box::new(move |name, bytes| {
        into.borrow_mut().push((name.to_owned(), bytes.to_vec()));
        Ok(())
    }));
    // No dialog: welded at once.
    let _ = varde.update(Message::Ui(Ui::File(File::Export)));
    assert!(matches!(
        document(&varde).export_state(),
        Some(Exporting::Welding { to: None, .. })
    ));
    regenerate(&mut varde, &regen);
    assert!(sent(&io).is_empty());
    let downloaded = downloaded.take();
    let [(name, bytes)] = &downloaded[..] else {
        panic!("not one download");
    };
    assert_eq!(name, "part.3mf");
    let evaluation = varde_regen::evaluate(&Document::example(), &mut Default::default());
    let bodies: Vec<three_mf::Body> = varde_regen::export(&Document::example(), &evaluation)
        .unwrap()
        .into_iter()
        .map(|body| three_mf::Body {
            name: body.name,
            mesh: body.mesh,
        })
        .collect();
    assert_eq!(*bytes, three_mf::package("part", &bodies).unwrap());
    assert_eq!(document(&varde).export_state(), None);

    // The browser refusing it is shown.
    varde.files.downloader = Some(Box::new(|name, _| Err(format!("couldn't download {name}"))));
    let _ = varde.update(Message::Ui(Ui::File(File::Export)));
    regenerate(&mut varde, &regen);
    assert_eq!(
        document(&varde).export_error(),
        Some("couldn't download part.3mf")
    );
}

#[test]
fn bodies_welded_while_quitting_are_not_written() {
    let (mut varde, io, regen) = with_plate();
    export_to(&mut varde, Some(Chosen::Path("/d/plate.3mf".into())));
    let _ = varde.update(Message::CloseRequested(window_id()));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(
        varde_view::Unsaved::Discard,
    ))));
    assert!(varde.quitting.is_some());
    sent(&io);
    regenerate(&mut varde, &regen);
    assert!(sent(&io).is_empty());
}

#[test]
fn an_export_the_replaced_lane_was_welding_fails() {
    let (mut varde, io, regen) = with_plate();
    export_to(&mut varde, Some(Chosen::Path("/d/plate.3mf".into())));
    let Some(Request::Export { export, .. }) = regen.take().pop() else {
        panic!("no export asked for");
    };
    let doc = varde.screen.doc_mut().unwrap();
    doc.regen_replaced();
    assert_eq!(doc.export_state(), None);
    assert!(doc.export_error().is_some_and(|e| e.ends_with("try again")));
    assert!(doc.exportable());
    let id = doc.id;
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::DismissExportError)));
    // The old lane's answer, should it still come, is dropped.
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::Computed(Response::Exported {
            export,
            result: Ok(Vec::new()),
        }),
    ));
    assert_eq!(document(&varde).export_error(), None);
    assert!(sent(&io).is_empty());
}
