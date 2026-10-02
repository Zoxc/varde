//! Saving to files reading found damaged, and the check before a save:
//! each row of its table, appending past a damaged newest save, a failed
//! save that partly landed, damage after opening, refusing a file read as
//! damaged, and held files' rules for damage.

use super::*;

/// What's known of the design file `bytes` once read.
fn known_of(bytes: &[u8]) -> Known {
    from_bytes_with_report(bytes).unwrap().known()
}

/// What [`check_unchanged`] says of `file`, as `known` last read or wrote
/// it, as a string: where a save would append, or why it's refused.
fn checked(file: &[u8], known: &Known) -> String {
    format!("{:?}", check_unchanged(file, known))
}

/// Where a save after `tail`, the newest record, goes in the file with
/// `id`, as [`checked`] shows it.
fn after(id: u128, tail: Tail) -> String {
    format!(
        "{:?}",
        Ok::<_, Error>(Append {
            id,
            at: tail.end,
            prev: tail.sum,
            before: tail.last,
        })
    )
}

/// Where the payload of the record ending at `tail` is in its file.
fn payload_of(tail: Tail) -> usize {
    tail.last as usize + TAG_LEN_AT + 10
}

/// `record`, a whole block, with all but its header and its back zeroed:
/// a save that partly landed, framed like a record but not intact.
fn partly_landed(record: &[u8]) -> Vec<u8> {
    let mut record = record.to_vec();
    let back = record.len() - BACK_LEN;
    record[TAG_LEN_AT..back].fill(0);
    record
}

/// `known` with `attempt` remembered, as after a failed save that wrote
/// `record`.
fn with_attempt(known: &Known, record: &[u8]) -> Known {
    Known {
        attempt: Some(Attempt::of(record)),
        ..*known
    }
}

/// The check's table, row by row: after the tail, nothing that matters,
/// a failed save of ours, the damaged newest save reading found, another
/// record, damage reading didn't find; a file read as damaged; and the
/// record at the tail, its link, and the file itself not as expected.
#[test]
fn a_save_checks_the_file_is_unchanged() {
    let (bytes, tails) = saved(3);
    let tail = tails[2];
    let id = id_of(&bytes);
    let known = known_of(&bytes);
    assert_eq!(
        known,
        Known {
            before: Some(tails[1].last),
            ..Known::written(id, tail)
        }
    );
    let ok = after(id, tail);
    let conflict = "Err(Conflict)";
    let damaged = "Err(Damaged)";
    let file = |rest: &[&[u8]]| [&bytes[..], &rest.concat()].concat();

    // Nothing, previews, blocks of other kinds, then a torn tail or the
    // end.
    let (preview, shown) = frame(id, tail.sum, tail.end, PREVIEW, b"image/png", b"png").unwrap();
    let (unknown, _) = frame(id, shown.sum, shown.end, [7; 16], b"", b"what").unwrap();
    let (record, _) = next(&bytes, tail, &edited(4));
    assert_eq!(checked(&bytes, &known), ok);
    assert_eq!(checked(&file(&[&preview, &unknown]), &known), ok);
    for torn in [
        &[0; 100][..],
        &noise(1000, 3),
        &record[..record.len() / 2],
        &partly_landed(&record)[..record.len() - 1],
    ] {
        assert_eq!(checked(&file(&[torn]), &known), ok);
        assert_eq!(checked(&file(&[&preview, &unknown, torn]), &known), ok);
    }

    // A failed save of ours, whole or partly landed, intact or not, is
    // replaced; without the attempt, it's someone else's change, or
    // damage.
    let attempted = with_attempt(&known, &record);
    for (landed, without) in [
        (record.clone(), conflict),
        (partly_landed(&record), damaged),
    ] {
        assert_eq!(checked(&file(&[&landed]), &attempted), ok);
        assert_eq!(checked(&file(&[&landed, &[0; 10]]), &attempted), ok);
        assert_eq!(checked(&file(&[&landed]), &known), without);
    }

    // Another record reached by the chain, after other kinds too.
    assert_eq!(checked(&file(&[&record]), &known), conflict);
    let (later, _) = next(&file(&[&preview]), shown, &edited(4));
    assert_eq!(checked(&file(&[&preview, &later]), &known), conflict);

    // Damage reading didn't find: a newest save damaged, its header intact,
    // and one whose header is damaged too, which reading searches past.
    let newest_damaged = flip(&record, TAG_LEN_AT + 10);
    let header_damaged = flip(&record, PREV_AT);
    assert_eq!(checked(&file(&[&newest_damaged]), &known), damaged);
    assert_eq!(checked(&file(&[&header_damaged]), &known), damaged);
    assert_eq!(
        checked(&file(&[&preview, &header_damaged, &noise(50, 1)]), &known),
        damaged
    );

    // The damaged newest save reading found: appended after, with a torn
    // tail after it, or a failed save of ours, but not another record.
    let damaged_file = flip(&bytes, payload_of(tail));
    let opened = from_bytes_with_report(&damaged_file).unwrap();
    let found = DamagedRecord {
        start: tail.last,
        end: tail.end,
        sum: tail.sum,
    };
    assert_eq!(opened.report.outcome, Outcome::NewestDamaged(found));
    let known_damaged = opened.known();
    let past_damage = after(id, tail);
    let with = |rest: &[&[u8]]| [&damaged_file[..], &rest.concat()].concat();
    assert_eq!(checked(&damaged_file, &known_damaged), past_damage);
    assert_eq!(checked(&with(&[&[0; 100]]), &known_damaged), past_damage);
    let (record_after, _) = next(&damaged_file, tail, &edited(4));
    assert_eq!(checked(&with(&[&record_after]), &known_damaged), conflict);
    let attempted = with_attempt(&known_damaged, &record_after);
    for landed in [record_after.clone(), partly_landed(&record_after)] {
        assert_eq!(checked(&with(&[&landed]), &attempted), past_damage);
    }
    // More damage after it, which reading didn't find.
    let (damaged_after, _) = next(&damaged_file, tail, &edited(5));
    let damaged_after = flip(&damaged_after, TAG_LEN_AT + 10);
    assert_eq!(checked(&with(&[&damaged_after]), &known_damaged), damaged);
    // Not found by a save that didn't read it: damage since.
    assert_eq!(
        checked(&damaged_file, &Known::written(id, tails[1])),
        damaged
    );

    // Read as damaged, by a search: refused, whatever the file holds.
    let searched = flip(&bytes, tails[1].last as usize + PREV_AT);
    let opened = from_bytes_with_report(&searched).unwrap();
    assert!(matches!(opened.report.outcome, Outcome::Damaged { .. }));
    for state in [&searched, &bytes] {
        assert_eq!(checked(state, &opened.known()), "Err(OpenedDamaged)");
    }

    // The record at the tail damaged, or cut short: damage. Another
    // intact record there: someone else's change.
    assert_eq!(checked(&flip(&bytes, payload_of(tail)), &known), damaged);
    assert_eq!(checked(&bytes[..bytes.len() - 1], &known), damaged);
    assert_eq!(checked(&bytes[..tail.last as usize], &known), damaged);
    let (other, _) = next(&bytes, tails[1], &edited(7));
    let replaced = [&bytes[..tails[1].end as usize], &other].concat();
    assert_eq!(checked(&replaced, &known), conflict);

    // Its link: the block before it damaged where it holds its `sum`, or
    // another intact block there.
    assert_eq!(
        checked(&flip(&bytes, tails[1].last as usize + SUM_AT), &known),
        damaged
    );
    let size = (tails[1].end - tails[1].last) as usize;
    let filler = vec![1; size - BLOCK_OVERHEAD as usize - 2];
    let (block, _) = frame(id, tails[0].sum, tails[1].last, [7; 16], b"", &filler).unwrap();
    assert_eq!(block.len(), size);
    let relinked = [&bytes[..tails[0].end as usize], &block, &bytes[span(tail)]].concat();
    assert_eq!(checked(&relinked, &known), conflict);

    // Another file, even holding the same, or something else.
    let mut other_id = bytes.clone();
    other_id[FILE_HEADER_LEN - 1] ^= 1;
    for state in [
        &other_id[..],
        &whole_file(&edited(3), &[]).unwrap().0,
        b"",
        b"other",
    ] {
        assert_eq!(checked(state, &known), conflict);
    }
}

/// A save past the newest save, damaged with its header intact, goes after
/// it, linked to the `sum` its header holds, dropping a torn tail after it
/// but nothing else; reading then steps over it by its header, and saves
/// go on as usual.
#[test]
fn a_save_goes_past_a_damaged_newest_save() {
    let (bytes, tails) = saved(3);
    let damaged_file = flip(&bytes, payload_of(tails[2]));
    let mut file = Memory {
        bytes: [&damaged_file[..], &[0; 100]].concat(),
        syncs: 0,
    };
    let mut known = known_of(&file.bytes);
    let saved = save(&mut file, &mut known, &edited(4), &[]).unwrap();
    assert_eq!(
        known,
        Known {
            before: Some(tails[2].last),
            ..Known::written(id_of(&bytes), saved)
        }
    );
    assert_eq!(saved.last, tails[2].end);
    assert!(file.bytes.starts_with(&damaged_file));
    assert_eq!(file.bytes.len() as u64, saved.end);
    let unreadable = tails[2].end - tails[2].last;
    let (document, tail, report) = opened(&file.bytes);
    assert_eq!((document, tail), (edited(4), saved));
    assert_eq!(
        (report.outcome, report.unreadable),
        (Outcome::Bridged, unreadable)
    );

    let newer = save(&mut file, &mut known, &edited(5), &[]).unwrap();
    let (document, tail, report) = opened(&file.bytes);
    assert_eq!((document, tail), (edited(5), newer));
    assert_eq!(report.outcome, Outcome::Bridged);
    // Opened again, the same.
    let mut known = known_of(&file.bytes);
    save(&mut file, &mut known, &edited(6), &[]).unwrap();
    assert_eq!(opened(&file.bytes).0, edited(6));
}

/// The newest save damaged, its header intact, with blocks of other kinds
/// after it, say its previews, that reading steps over it by its header
/// to: it's still the newest save, damaged, and a save goes after it,
/// dropping them, rather than finding damage reading didn't.
#[test]
fn a_damaged_newest_save_with_blocks_after_it_is_saved_past() {
    let (bytes, tails) = saved(3);
    let id = id_of(&bytes);
    let damaged = DamagedRecord {
        start: tails[2].last,
        end: tails[2].end,
        sum: tails[2].sum,
    };
    let (preview, _) = frame(id, damaged.sum, damaged.end, PREVIEW, b"image/png", b"png").unwrap();
    let damaged_file = flip(&bytes, payload_of(tails[2]));
    let mut file = Memory {
        bytes: [&damaged_file[..], &preview].concat(),
        syncs: 0,
    };
    let (document, tail, report) = opened(&file.bytes);
    assert_eq!((document, tail), (edited(2), tails[1]));
    assert_eq!(report.outcome, Outcome::NewestDamaged(damaged));
    assert_eq!(report.unreadable, damaged.end - damaged.start);

    let mut known = known_of(&file.bytes);
    let saved = save(&mut file, &mut known, &edited(4), &[]).unwrap();
    assert_eq!(saved.last, damaged.end);
    assert!(file.bytes.starts_with(&damaged_file));
    let (document, tail, report) = opened(&file.bytes);
    assert_eq!((document, tail), (edited(4), saved));
    assert_eq!(report.outcome, Outcome::Bridged);
}

/// The record before the newest damaged at its end, as a torn sector
/// leaves it, its trailing `len` among what's lost: reading steps over it
/// by its header, and saves go on, the newest's link checked through the
/// header reading stepped over rather than the trailing `len`.
#[test]
fn a_save_goes_on_past_a_record_damaged_at_its_end() {
    let (bytes, tails) = saved(3);
    let len_at = tails[1].end as usize - BACK_LEN;
    for at in [len_at, len_at + 7] {
        let mut file = Memory {
            bytes: flip(&bytes, at),
            syncs: 0,
        };
        let (document, tail, report) = opened(&file.bytes);
        assert_eq!((document, tail), (edited(3), tails[2]));
        assert_eq!(report.outcome, Outcome::Bridged);

        let mut known = known_of(&file.bytes);
        let saved = save(&mut file, &mut known, &edited(4), &[]).unwrap();
        let newer = save(&mut file, &mut known, &edited(5), &[]).unwrap();
        assert_eq!(saved.end, newer.last);
        let (document, tail, report) = opened(&file.bytes);
        assert_eq!((document, tail), (edited(5), newer));
        assert_eq!(report.outcome, Outcome::Bridged);
    }
}

/// A save past a damaged newest save that fails is cut back to the end of
/// the damaged save, not before it; one left partly written, its header
/// landed but not the rest, is the next save's own to replace.
#[test]
fn a_failed_save_past_a_damaged_newest_save_keeps_it() {
    let (bytes, tails) = saved(3);
    let damaged_file = flip(&bytes, payload_of(tails[2]));
    for mut file in failures() {
        file.file.bytes = [&damaged_file[..], &[0; 50]].concat();
        let mut known = known_of(&file.file.bytes);
        assert!(save(&mut file, &mut known, &edited(4), &[]).is_err());
        assert_eq!(file.file.bytes, damaged_file);
        let mut file = file.file;
        save(&mut file, &mut known, &edited(4), &[]).unwrap();
        assert_eq!(opened(&file.bytes).0, edited(4));
    }

    // Partly landed, then the cutting back failed.
    let mut file = Failing {
        write: Some(usize::MAX),
        stuck: true,
        ..Failing::default()
    };
    file.file.bytes = damaged_file.clone();
    let mut known = known_of(&damaged_file);
    assert!(save(&mut file, &mut known, &edited(4), &[]).is_err());
    let attempt = known.attempt.unwrap();
    let written = file.file.bytes[damaged_file.len()..].to_vec();
    assert_eq!(attempt, Attempt::of(&written));
    assert_eq!(attempt.prev, tails[2].sum);
    let mut file = Memory {
        bytes: [&damaged_file[..], &partly_landed(&written)].concat(),
        syncs: 0,
    };
    let saved = save(&mut file, &mut known, &edited(5), &[]).unwrap();
    assert_eq!(saved.last, tails[2].end);
    assert_eq!(opened(&file.bytes).0, edited(5));
}

/// A save finds damage that wasn't there when the file was read, natively
/// too, and writes nothing.
#[test]
fn damage_since_reading_is_refused() {
    let dir = TempDir::new("damaged-since");
    let first = create_at(&dir.file(), &edited(1));
    let tail = save_at(&dir.file(), first, &edited(2)).unwrap();
    let bytes = std::fs::read(dir.file()).unwrap();
    let known = known_of(&bytes);
    let (record, _) = next(&bytes, tail, &edited(3));
    for state in [
        flip(&bytes, payload_of(tail)),
        [&bytes[..], &flip(&record, TAG_LEN_AT + 10)].concat(),
    ] {
        std::fs::write(dir.file(), &state).unwrap();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.file())
            .unwrap();
        let refused = save(&mut file, &mut known.clone(), &edited(4), &[]);
        assert!(matches!(refused, Err(Error::Damaged)), "{refused:?}");
        assert_eq!(
            refused.unwrap_err().to_string(),
            "file was damaged since it was opened"
        );
        assert_eq!(std::fs::read(dir.file()).unwrap(), state);
    }
}

/// A file read as damaged, with blocks found only by search, is never
/// saved to, so its damaged tail isn't cut off; it can be written whole.
#[test]
fn a_file_read_as_damaged_is_not_saved_to() {
    let (bytes, tails) = saved(3);
    let damaged = flip(&bytes, tails[1].last as usize + PREV_AT);
    let opened = from_bytes_with_report(&damaged).unwrap();
    assert!(matches!(
        opened.report.outcome,
        Outcome::Damaged { found: Some(_) }
    ));
    let mut known = opened.known();
    let mut file = Memory {
        bytes: damaged.clone(),
        syncs: 0,
    };
    let refused = save(&mut file, &mut known, &edited(4), &[]);
    assert!(matches!(refused, Err(Error::OpenedDamaged)));
    assert_eq!(
        refused.unwrap_err().to_string(),
        "file is damaged, so it can only be saved as another file"
    );
    assert_eq!(file.bytes, damaged);
    assert_eq!(known, opened.known());
}

/// Storage whose reads fail while `fail` is set, as a disk might.
#[derive(Debug, Default)]
struct Unreadable {
    file: Memory,
    fail: bool,
}

impl ReadAt for Unreadable {
    fn len(&self) -> io::Result<u64> {
        self.file.len()
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        if self.fail {
            return Err(io::Error::other("read failed"));
        }
        self.file.read_at(buf, at)
    }
}

impl Storage for Unreadable {
    fn write_at(&mut self, buf: &[u8], at: u64) -> io::Result<()> {
        self.file.write_at(buf, at)
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.file.truncate(len)
    }

    fn sync(&mut self) -> io::Result<()> {
        self.file.sync()
    }
}

/// A held file that can't be read isn't started over: it's left as it is,
/// and appended to once it can be read.
#[test]
fn a_held_file_that_cant_be_read_is_not_started_over() {
    let mut file = HeldFile::<_, Document>::new(Memory::default());
    file.append(&edited(1)).unwrap();
    file.append(&edited(2)).unwrap();
    let (id, _) = held_at(&file);
    let bytes = file.into_storage().bytes;
    let mut file = HeldFile::<_, Document>::new(Unreadable {
        file: Memory {
            bytes: bytes.clone(),
            syncs: 0,
        },
        fail: true,
    });
    assert!(matches!(file.read(), Err(Error::Io(_))));
    assert!(matches!(file.append(&edited(3)), Err(Error::Io(_))));
    assert_eq!(file.storage().file.bytes, bytes);

    file.file.fail = false;
    file.append(&edited(3)).unwrap();
    assert_eq!(held_at(&file).0, id);
    assert_eq!(file.read().unwrap(), Some(edited(3)));
    let chain = Chain::scan(&file.storage().file.bytes, FileType::Held).unwrap();
    assert_eq!(chain.records.len(), 3);
}

/// A held file appends past its newest record damaged with an intact
/// header, as a save does, and cuts off other damage after the record it
/// read.
#[test]
fn a_held_file_appends_past_a_damaged_newest_record() {
    let mut file = HeldFile::<_, Document>::new(Memory::default());
    for n in 1..=3 {
        file.append(&edited(n)).unwrap();
    }
    let (_, newest) = held_at(&file);
    let bytes = file.into_storage().bytes;

    let damaged = flip(&bytes, payload_of(newest));
    let mut file = HeldFile::<_, Document>::new(Memory {
        bytes: damaged.clone(),
        syncs: 0,
    });
    file.append(&edited(4)).unwrap();
    assert!(file.storage().bytes.starts_with(&damaged));
    let opened = file.read_with_report().unwrap().unwrap();
    assert_eq!(opened.payload, edited(4));
    assert_eq!(opened.report.outcome, Outcome::Bridged);

    // Found only by search: the damage goes.
    let chain = Chain::scan(&bytes, FileType::Held).unwrap();
    let second = chain.records[1];
    let searched = flip(&bytes, second.start as usize + PREV_AT);
    let mut file = HeldFile::<_, Document>::new(Memory {
        bytes: searched,
        syncs: 0,
    });
    let opened = file.read_with_report().unwrap().unwrap();
    assert!(matches!(opened.report.outcome, Outcome::Damaged { .. }));
    file.append(&edited(5)).unwrap();
    let (_, appended) = held_at(&file);
    assert_eq!(appended.last, chain.records[0].end);
    assert_eq!(file.storage().bytes.len() as u64, appended.end);
    let opened = file.read_with_report().unwrap().unwrap();
    assert_eq!(
        (opened.payload, opened.report.outcome),
        (edited(5), Outcome::Intact)
    );
}
