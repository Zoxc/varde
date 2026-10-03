//! Files found damaged as they're opened: the banner, the prompt before a
//! design damaged past the save opened shows, saving such a design, what
//! a damaged auto-save offers, and damaged store entries.

use varde_io::{Damage, DamageKind, FoundSave, ListedDamage, RecoveryError, UnixSeconds};

use super::*;

/// When the save opened was saved: 2 h before the clock reads.
fn two_hours_ago() -> UnixSeconds {
    UnixSeconds(when::now().0 - 2 * 3600)
}

/// `kind` of damage to the save from [`two_hours_ago`], 3000 bytes
/// unreadable.
fn damage(kind: DamageKind) -> Damage {
    Damage {
        kind,
        time: two_hours_ago(),
        unreadable: 3000,
    }
}

/// A save found after the damage, from an hour ago, named by the tail of
/// a design the lane might make.
fn found() -> FoundSave {
    FoundSave {
        tail: varde_io::vrdp::to_bytes(&Document::default(), &[])
            .unwrap()
            .1,
        time: UnixSeconds(when::now().0 - 3600),
    }
}

/// `opened` as the answer to the open tagged `id` of `/d/part.vrdp`.
fn answer_open(varde: &mut Varde, id: OpenId, opened: Opened) {
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: Some("/d/part.vrdp".into()),
        result: Ok(opened),
    }));
}

/// The design with a line, as file 0 opened for editing with `damage`.
fn opened_with(damage: Option<Damage>) -> Opened {
    Opened {
        file: FileId(0),
        document: with_a_line(),
        access: Access::Edit,
        recovered: Ok(None),
        browser: None,
        not_copied: None,
        download: None,
        damage,
    }
}

/// An app that asked to open `/d/part.vrdp`, with the tag of the open
/// and the requests sent so far forgotten.
fn opening() -> (Varde, Rc<RefCell<Vec<IoRequest>>>, OpenId) {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(IoResponse::RecentLoaded {
        entries: vec![],
        home: None,
    }));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/part.vrdp".into(),
    ))));
    let (id, _) = last_open(&requests);
    sent(&requests);
    (varde, requests, id)
}

/// An app showing `/d/part.vrdp` opened as `opened`, the requests sent
/// so far forgotten.
fn shown_as(opened: Opened) -> (Varde, Rc<RefCell<Vec<IoRequest>>>) {
    let (mut varde, requests, id) = opening();
    answer_open(&mut varde, id, opened);
    sent(&requests);
    (varde, requests)
}

/// An app asking about `/d/part.vrdp`, opened as file 0, damaged past the
/// save opened, with the save `found` after the damage if there's one.
fn prompting(found: Option<FoundSave>) -> (Varde, Rc<RefCell<Vec<IoRequest>>>) {
    let (mut varde, requests, id) = opening();
    answer_open(
        &mut varde,
        id,
        opened_with(Some(damage(DamageKind::Damaged { found }))),
    );
    sent(&requests);
    (varde, requests)
}

pub(super) fn welcome(varde: &Varde) -> &Welcome {
    match &varde.screen {
        Screen::Welcome(welcome) => welcome,
        Screen::Document(_) => panic!("not on the welcome screen"),
    }
}

/// The texts the welcome screen shows at 1280 × 800, light.
pub(super) fn welcome_texts(varde: &Varde) -> Vec<String> {
    let mut renderer = varde_view::probe::renderer();
    let size = iced::Size::new(1280.0, 800.0);
    let view = welcome(varde).view(&varde.files, Mode::Light, varde.options.theme);
    let mut ui = shown(view, size, &mut renderer);
    (texts(&mut ui, &renderer).into_iter())
        .map(|text| text.text)
        .collect()
}

/// Whether one of `texts` holds `part`.
pub(super) fn shows(texts: &[String], part: &str) -> bool {
    texts.iter().any(|text| text.contains(part))
}

#[test]
fn a_file_with_earlier_saves_damaged_opens_with_a_banner() {
    let (mut varde, requests) = shown_as(opened_with(Some(damage(DamageKind::Bridged))));
    let doc = document(&varde);
    assert!(!doc.damaged_file());
    let texts = screen_texts(doc);
    assert!(shows(&texts, "Damaged file"), "{texts:?}");
    assert!(shows(&texts, "Some earlier saves in this file are damaged"));
    assert!(!varde.title().contains("damaged"));

    // Saved to as usual.
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    saving(&sent(&requests));

    let _ = varde.update(Message::Ui(Ui::Edit(Edit::DismissDamage)));
    assert!(document(&varde).damage().is_none());
    assert!(!shows(&screen_texts(document(&varde)), "Damaged file"));
}

#[test]
fn a_file_whose_newest_save_is_damaged_says_which_opened() {
    let (mut varde, requests) = shown_as(opened_with(Some(damage(DamageKind::NewestDamaged))));
    let texts = screen_texts(document(&varde));
    assert!(
        shows(
            &texts,
            "The newest save in this file is damaged; opened the one from 2 h ago"
        ),
        "{texts:?}"
    );
    // Saves go after the damaged one, as usual.
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    saving(&sent(&requests));
}

#[test]
fn a_damaged_file_is_asked_about_before_it_shows_and_cancel_keeps_it() {
    let (mut varde, requests) = prompting(None);
    assert!(welcome(&varde).prompting());
    let texts = welcome_texts(&varde);
    assert!(shows(&texts, "This file is damaged."), "{texts:?}");
    assert!(shows(
        &texts,
        "3 KB of part.vrdp can't be read. The newest save that can is from 2 h ago."
    ));
    assert!(shows(&texts, "saving it saves another file"));
    assert!(!shows(&texts, "Open found save"));
    // Nothing is auto-saved, as nothing is shown yet, nor is it a recent
    // file before it's opened.
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 10);
    assert!(sent(&requests).is_empty());
    assert!(varde.files.recent.entries().is_empty());
    // The welcome screen's keys are off behind it, and `Esc` cancels it.
    let n = press(keyboard::Key::Character("n".into()), Default::default());
    assert!(keys::welcome_key((false, n.clone())).is_none());
    assert!(keys::welcome_key((true, n)).is_some());
    let escape = press(keyboard::Key::Named(key::Named::Escape), Default::default());
    assert!(matches!(
        escape_key((Some(Dialog::Damaged), escape)),
        Some(Message::Ui(Ui::Welcome(WelcomeUi::CancelDamaged)))
    ));

    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::CancelDamaged)));
    assert!(is_welcome(&varde));
    assert!(!welcome(&varde).prompting());
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::Close {
            file: FileId(0),
            closing: Closing::Keep
        }]
    ));
}

#[test]
fn a_damaged_file_opened_says_so_and_saves_as_another() {
    let (mut varde, requests) = prompting(None);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenDamaged)));
    let doc = document(&varde);
    assert_eq!(*doc.editor.document(), with_a_line());
    assert!(doc.damaged_file());
    assert_eq!(
        varde.title(),
        format!("part.vrdp (damaged file) — {APP_NAME}")
    );
    // Asked about already: no banner.
    assert!(doc.damage().is_none());
    assert!(!shows(&screen_texts(doc), "Damaged file"));
    // A recent file once opened.
    assert_eq!(
        varde.files.recent.entries()[0].entry.path,
        Path::new("/d/part.vrdp")
    );
    sent(&requests);

    // Save asks where, never writing the damaged file.
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert_eq!(document(&varde).picking(), Some(Picking::SaveAs));
    assert!(sent(&requests).is_empty());
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/copy.vrdp".into()))),
    ));
    let sent_now = sent(&requests);
    let [IoRequest::SaveAs { revision, .. }] = &sent_now[..] else {
        panic!("not a save as: {sent_now:?}");
    };
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(0)),
        to: Chosen::Path("/d/copy.vrdp".into()),
        revision: *revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(0),
            access: Access::Edit,
            offered: false,
        }),
    }));
    // The copy is a file like any.
    let doc = document(&varde);
    assert!(!doc.damaged_file());
    assert!(doc.damage().is_none());
    assert_eq!(varde.title(), format!("copy.vrdp — {APP_NAME}"));
    sent(&requests);
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    saving(&sent(&requests));
}

#[test]
fn closing_a_damaged_file_with_changes_saves_as_another() {
    let (mut varde, requests) = prompting(None);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenDamaged)));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    sent(&requests);
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Save))));
    assert_eq!(document(&varde).picking(), Some(Picking::SaveAs));
    assert!(sent(&requests).is_empty());
    // Backing out stays.
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(id, ForDoc::SaveAsPicked(None)));
    assert!(document(&varde).leaving().is_none());
}

#[test]
fn the_save_found_after_the_damage_opens_instead() {
    let found = found();
    let (mut varde, requests) = prompting(Some(found));
    let texts = welcome_texts(&varde);
    assert!(
        shows(
            &texts,
            "A newer save, from 1 h ago, was found after the damage."
        ),
        "{texts:?}"
    );
    assert!(shows(&texts, "Open found save"));

    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenFound)));
    let sent_now = sent(&requests);
    let [
        IoRequest::OpenFound {
            id,
            file: FileId(0),
            found: tail,
        },
    ] = sent_now[..]
    else {
        panic!("not opening the save found: {sent_now:?}");
    };
    assert_eq!(tail, found.tail);
    assert!(shows(&welcome_texts(&varde), "Opening…"));
    // Nothing else meanwhile, but Cancel.
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenDamaged)));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenFound)));
    assert!(welcome(&varde).prompting());
    assert!(sent(&requests).is_empty());

    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: Some("/d/part.vrdp".into()),
        result: Ok(Opened {
            document: Document::default(),
            ..opened_with(Some(damage(DamageKind::Damaged { found: None })))
        }),
    }));
    let doc = document(&varde);
    assert_eq!(*doc.editor.document(), Document::default());
    assert_eq!(doc.name, "part");
    assert!(doc.damaged_file());
}

#[test]
fn a_save_found_that_fails_to_open_leaves_the_one_that_can() {
    let (mut varde, requests) = prompting(Some(found()));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenFound)));
    let [IoRequest::OpenFound { id, .. }] = sent(&requests)[..] else {
        panic!("not opening the save found");
    };
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: None,
        result: Err("no such save was found in the file".to_owned()),
    }));
    assert!(welcome(&varde).prompting());
    let texts = welcome_texts(&varde);
    assert!(
        shows(
            &texts,
            "Couldn't open the save found: no such save was found in the file"
        ),
        "{texts:?}"
    );
    assert!(!shows(&texts, "Open found save"));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenFound)));
    assert!(sent(&requests).is_empty());
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenDamaged)));
    assert_eq!(*document(&varde).editor.document(), with_a_line());
}

#[test]
fn a_read_only_damaged_file_opens_read_only_after_the_prompt() {
    let (mut varde, requests, id) = opening();
    answer_open(
        &mut varde,
        id,
        Opened {
            access: Access::ReadOnly(ReadOnly::InUse),
            ..opened_with(Some(damage(DamageKind::Damaged { found: None })))
        },
    );
    assert!(welcome(&varde).prompting());
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenDamaged)));
    let doc = document(&varde);
    assert!(doc.read_only.is_some());
    assert!(doc.damaged_file());
    sent(&requests);
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 10);
    assert!(sent(&requests).is_empty());
}

#[test]
fn the_prompt_comes_before_the_recovery_offer() {
    let (mut varde, requests, id) = opening();
    answer_open(
        &mut varde,
        id,
        Opened {
            recovered: Ok(Some(Offer {
                document: Document::default(),
                design_changed: true,
                damage: None,
                newer_base: true,
            })),
            ..opened_with(Some(damage(DamageKind::Damaged { found: None })))
        },
    );
    assert!(welcome(&varde).prompting());
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenDamaged)));
    let doc = document(&varde);
    assert!(doc.recovered().is_some());
    let texts = screen_texts(doc);
    assert!(
        shows(&texts, "auto-saved from a newer save than could be read"),
        "{texts:?}"
    );
    assert!(!shows(&texts, "may undo newer changes"));
    // Auto-saves wait for the answer, as for any offer.
    sent(&requests);
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 10);
    assert!(sent(&requests).is_empty());
}

#[test]
fn quitting_while_asking_about_a_damaged_file_keeps_it() {
    let (mut varde, requests) = prompting(None);
    let window = window_id();
    let _ = varde.update(Message::CloseRequested(window));
    let sent = sent(&requests);
    assert!(
        matches!(
            &sent[..],
            [
                IoRequest::Close {
                    file: FileId(0),
                    closing: Closing::Keep
                },
                IoRequest::Flush
            ]
        ),
        "{sent:?}"
    );
}

#[test]
fn a_save_refused_as_damaged_asks_where_from_then_on() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(saved(revision, Err(SaveError::OpenedDamaged)));
    let doc = document(&varde);
    assert_eq!(doc.picking(), Some(Picking::SaveAs));
    assert!(doc.damaged_file());
    assert!(doc.save_error().is_none());
    assert!(!doc.saves().any());
    assert!(varde.title().contains("(damaged file)"));

    let id = doc.id;
    let _ = varde.update(Message::Doc(id, ForDoc::SaveAsPicked(None)));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert_eq!(document(&varde).picking(), Some(Picking::SaveAs));
    assert!(sent(&requests).is_empty());
}

#[test]
fn a_file_damaged_since_it_was_opened_is_shown_with_save_as() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(saved(revision, Err(SaveError::Damaged)));
    let texts = screen_texts(document(&varde));
    assert!(shows(&texts, "Couldn't save"), "{texts:?}");
    assert!(shows(
        &texts,
        "the file was damaged since it was opened or saved"
    ));
    assert!(shows(&texts, "Save As…"));
    // Not a file opened damaged: Save still saves to it.
    assert!(!document(&varde).damaged_file());
}

#[test]
fn a_damaged_auto_save_offered_says_so() {
    let (varde, _) = shown_as(Opened {
        recovered: Ok(Some(Offer {
            document: Document::default(),
            design_changed: false,
            damage: Some(damage(DamageKind::NewestDamaged)),
            newer_base: false,
        })),
        ..opened_with(None)
    });
    let texts = screen_texts(document(&varde));
    assert!(
        shows(
            &texts,
            "the newest auto-save of them is damaged, so these are from 2 h ago"
        ),
        "{texts:?}"
    );
    assert!(shows(&texts, "Restore"));
}

#[test]
fn an_auto_save_that_cannot_be_read_waits_for_discard() {
    let error = RecoveryError {
        message: "what was auto-saved is damaged and can't be read".to_owned(),
        kept: true,
    };
    let (mut varde, requests) = shown_as(Opened {
        recovered: Err(error),
        ..opened_with(None)
    });
    let doc = document(&varde);
    assert!(doc.recovery_kept());
    let texts = screen_texts(doc);
    assert!(
        shows(&texts, "the auto-save is damaged and can't be read"),
        "{texts:?}"
    );
    assert!(!shows(&texts, "Restore"));
    assert!(shows(&texts, "Discard"));

    // Auto-saves wait for the answer, which the lane would refuse, so no
    // error shows either.
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 10);
    assert!(sent(&requests).is_empty());
    assert!(document(&varde).banner_error().is_none());

    let _ = varde.update(Message::Ui(Ui::File(File::DiscardChanges)));
    assert!(!document(&varde).recovery_kept());
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::DiscardRecovery { file: FileId(0) }]
    ));
    tick(&mut varde, start, 11);
    tick(&mut varde, start, 14);
    assert_eq!(auto_saves(&sent(&requests)), [(0, 1)]);
}

#[test]
fn an_auto_save_that_cannot_be_read_is_kept_on_closing() {
    let error = RecoveryError {
        message: "couldn't read what was auto-saved".to_owned(),
        kept: true,
    };
    let (mut varde, requests) = shown_as(Opened {
        recovered: Err(error),
        ..opened_with(None)
    });
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert!(matches!(
        sent(&requests)[..],
        [
            IoRequest::Close {
                file: FileId(0),
                closing: Closing::Keep
            },
            IoRequest::LoadThumbnails { .. }
        ]
    ));
}

#[test]
fn an_auto_save_of_no_use_is_ignored_and_auto_saved_over() {
    let error = RecoveryError {
        message: "it's not an auto-save".to_owned(),
        kept: false,
    };
    let (mut varde, requests) = shown_as(Opened {
        recovered: Err(error),
        ..opened_with(None)
    });
    let doc = document(&varde);
    assert!(!doc.recovery_kept());
    assert!(doc.recovered().is_none());
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 3);
    assert_eq!(auto_saves(&sent(&requests)), [(0, 1)]);
}

#[test]
fn damaged_store_entries_are_listed_and_unreadable_ones_only_discarded() {
    let (mut varde, requests) = with_files();
    let designs = vec![
        Recovered {
            path: "/data/designs/a.vrdp".into(),
            modified: None,
            name: None,
            damage: Some(ListedDamage::Opens),
        },
        Recovered {
            path: "/data/designs/b.vrdp".into(),
            modified: None,
            name: None,
            damage: Some(ListedDamage::Unreadable),
        },
    ];
    let _ = varde.update(Message::Io(IoResponse::RecoveredListed { designs }));
    let texts = welcome_texts(&varde);
    assert!(texts.iter().any(|text| text == "Damaged"), "{texts:?}");
    assert!(shows(&texts, "Damaged, can't be read"));
    sent(&requests);

    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenStored(
        "/data/designs/b.vrdp".into(),
    ))));
    assert!(sent(&requests).is_empty());
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::DiscardStored(
        "/data/designs/b.vrdp".into(),
    ))));
    assert!(matches!(
        &sent(&requests)[..],
        [IoRequest::DiscardRecovered { path }] if path == Path::new("/data/designs/b.vrdp")
    ));

    // One that opens is asked about if it's damaged past what opens, and
    // then opened as its entry, whose next auto-save cuts the damage off.
    let path = PathBuf::from("/data/designs/a.vrdp");
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenStored(
        path.clone(),
    ))));
    let [IoRequest::OpenRecovered { id, .. }] = sent(&requests)[..] else {
        panic!("not opened");
    };
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: Some(path),
        result: Ok(Opened {
            file: FileId(4),
            ..opened_with(Some(damage(DamageKind::Damaged { found: None })))
        }),
    }));
    let texts = welcome_texts(&varde);
    assert!(
        shows(&texts, "3 KB of the recovered Untitled can't be read."),
        "{texts:?}"
    );
    assert!(shows(&texts, "cuts off what can't be read"));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenDamaged)));
    let doc = document(&varde);
    assert!(matches!(doc.target(), Target::Entry { .. }));
    assert!(!doc.damaged_file());
    assert!(!varde.title().contains("damaged"));
    assert!(doc.damage().is_none());
}

#[test]
fn a_damaged_store_entry_keeps_its_name_while_asked_about() {
    let (mut varde, requests) = with_files();
    let path = PathBuf::from("/data/designs/a.vrdp");
    let designs = vec![Recovered {
        path: path.clone(),
        modified: None,
        name: Some("bracket.vrdp".to_owned()),
        damage: Some(ListedDamage::Opens),
    }];
    let _ = varde.update(Message::Io(IoResponse::RecoveredListed { designs }));
    sent(&requests);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenStored(
        path.clone(),
    ))));
    let [IoRequest::OpenRecovered { id, .. }] = sent(&requests)[..] else {
        panic!("not opened");
    };
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: Some(path),
        result: Ok(opened_with(Some(damage(DamageKind::Damaged {
            found: None,
        })))),
    }));
    // The lane lists the store again after the open, without the entry it
    // holds now.
    let _ = varde.update(Message::Io(IoResponse::RecoveredListed { designs: vec![] }));
    let texts = welcome_texts(&varde);
    assert!(
        shows(&texts, "3 KB of the recovered bracket can't be read."),
        "{texts:?}"
    );
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenDamaged)));
    assert_eq!(document(&varde).name, "bracket");
}
