//! The thumbnail a save writes, rendered by the viewport's next frame
//! first, which the save waits for, though never for long.

use varde_io::thumbnail::Image;

use super::export::with_plate;
use super::*;

/// A thumbnail standing in for one rendered, told apart by `shade`.
fn image(shade: u8) -> Image {
    Image::new(2, 1, vec![shade; 8]).unwrap()
}

/// Answers the thumbnail being rendered with `image`, as the viewport's
/// frame and the task waiting for it would.
fn render(varde: &mut Varde, image: Option<Image>) {
    let doc = document(varde);
    let tag = doc.thumbnail_tag().expect("a thumbnail is being rendered");
    let _ = varde.update(Message::Doc(doc.id, ForDoc::Thumbnail(tag, image)));
}

/// Edits the design and saves it.
fn edit_and_save(varde: &mut Varde) {
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
}

/// The thumbnail of the only request in `sent`, a Save.
fn thumbnail(sent: &[IoRequest]) -> Option<Image> {
    match sent {
        [IoRequest::Save { thumbnail, .. }] => thumbnail.clone(),
        sent => panic!("expected a save, not {sent:?}"),
    }
}

#[test]
fn a_save_waits_for_its_thumbnail() {
    let (mut varde, io, _regen) = with_plate();
    edit_and_save(&mut varde);
    // Saving, but nothing sent until the viewport has drawn it.
    assert!(sent(&io).is_empty());
    assert!(varde.title().contains("Saving…"));
    let doc = document(&varde);
    let request = doc.thumbnail_request().unwrap();
    // Of the bodies of the model shown, which the edit hasn't changed
    // yet: the last the committed document regenerated to.
    assert!(Arc::ptr_eq(&request.mesh, doc.feed.mesh()));
    let room = varde_view::THUMBNAIL_ROOM.map(|side| side * varde_view::THUMBNAIL_SCALE);
    assert!(request.shot.size[0] <= room[0] && request.shot.size[1] <= room[1]);
    // The plate is wider than tall from where Home looks: cropped to it.
    assert_eq!(request.shot.size[0], room[0]);
    assert!(request.shot.size[1] < room[1]);
    let state = doc.state(
        false,
        Mode::Light,
        ViewOptions::default(),
        Offers::default(),
    );
    assert!(
        state
            .thumbnail
            .is_some_and(|shown| Arc::ptr_eq(shown, request))
    );

    render(&mut varde, Some(image(1)));
    assert_eq!(thumbnail(&sent(&io)), Some(image(1)));
    assert!(document(&varde).thumbnail_request().is_none());

    // The model unchanged, the next save takes the same one at once.
    edit_and_save(&mut varde);
    assert_eq!(thumbnail(&sent(&io)), Some(image(1)));
}

#[test]
fn the_model_changed_asks_for_another() {
    let (mut varde, io, regen) = with_plate();
    edit_and_save(&mut varde);
    render(&mut varde, Some(image(1)));
    sent(&io);
    // Hidden, the body is gone from the model shown.
    let body = document(&varde).editor.document().bodies()[0].id;
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::ToggleVisible(body))));
    let doc = varde.screen.doc_mut().unwrap();
    answer(doc, &regen);
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    // Nothing to frame: saved without one, at once.
    assert_eq!(thumbnail(&sent(&io)), None);
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::ToggleVisible(body))));
    answer(varde.screen.doc_mut().unwrap(), &regen);
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(sent(&io).is_empty());
    render(&mut varde, Some(image(2)));
    assert_eq!(thumbnail(&sent(&io)), Some(image(2)));
}

#[test]
fn a_thumbnail_that_fails_saves_without_and_is_tried_again() {
    let (mut varde, io, _regen) = with_plate();
    edit_and_save(&mut varde);
    render(&mut varde, None);
    assert_eq!(thumbnail(&sent(&io)), None);
    // Not kept: the next save waits for one again.
    edit_and_save(&mut varde);
    assert!(sent(&io).is_empty());
    render(&mut varde, Some(image(3)));
    assert_eq!(thumbnail(&sent(&io)), Some(image(3)));
}

#[test]
fn a_thumbnail_never_drawn_is_given_up_on() {
    let (mut varde, io, _regen) = with_plate();
    let start = Instant::now();
    edit_and_save(&mut varde);
    let tag = document(&varde).thumbnail_tag().unwrap();
    // The timer ticks for it, frames are drawn for it.
    assert!(document(&varde).rendering_thumbnail());
    tick(&mut varde, start, 1);
    assert!(sent(&io).is_empty());
    // Past two seconds from when it was asked for, a little after `start`.
    tick(&mut varde, start, 3);
    let saves: Vec<_> = (sent(&io).into_iter())
        .filter(|request| matches!(request, IoRequest::Save { .. }))
        .collect();
    assert_eq!(thumbnail(&saves), None);
    assert!(!document(&varde).rendering_thumbnail());
    // Drawn late, it's dropped: the save went.
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(id, ForDoc::Thumbnail(tag, Some(image(4)))));
    assert!(sent(&io).is_empty());
}

#[test]
fn closing_saves_after_the_thumbnail() {
    let (mut varde, io, _regen) = with_plate();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Save))));
    assert!(sent(&io).is_empty());
    assert!(!is_welcome(&varde));
    render(&mut varde, Some(image(5)));
    let sent_now = sent(&io);
    assert_eq!(thumbnail(&sent_now), Some(image(5)));
    let IoRequest::Save { revision, .. } = sent_now[0] else {
        unreachable!();
    };
    let _ = varde.update(saved(revision, Ok(())));
    assert!(is_welcome(&varde));
}

#[test]
fn a_save_as_waits_for_its_thumbnail_too() {
    let (mut varde, io, _regen) = with_plate();
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let id = document(&varde).id;
    let chosen = Chosen::Path("/d/copy.vrdp".into());
    let _ = varde.update(Message::Doc(id, ForDoc::SaveAsPicked(Some(chosen))));
    assert!(sent(&io).is_empty());
    render(&mut varde, Some(image(6)));
    assert!(matches!(
        &sent(&io)[..],
        [IoRequest::SaveAs { thumbnail: Some(sent), .. }] if *sent == image(6)
    ));
}

/// The app showing the welcome screen, its recent files listed.
#[test]
fn the_welcome_screen_shows_the_thumbnails_the_lane_reads() {
    let (mut varde, requests) = with_files();
    let paths: Vec<PathBuf> = ["/d/a.vrdp", "/d/b.vrdp"].map(PathBuf::from).into();
    let entries = (paths.iter())
        .map(|path| varde_io::recent::Listed {
            entry: varde_io::RecentFile {
                path: path.clone(),
                opened: varde_io::UnixSeconds(0),
            },
            available: true,
        })
        .collect();
    let _ = varde.update(Message::Io(IoResponse::RecentLoaded {
        entries,
        home: None,
    }));
    assert!(matches!(
        requests.borrow().last(),
        Some(IoRequest::LoadThumbnails { paths: asked }) if *asked == paths
    ));
    let _ = varde.update(Message::Io(IoResponse::ThumbnailsLoaded {
        thumbnails: vec![(paths[1].clone(), image(7))],
    }));
    assert_eq!(varde.files.thumbnails.len(), 1);
    assert_eq!(varde.files.thumbnails[0].0, paths[1]);
}
