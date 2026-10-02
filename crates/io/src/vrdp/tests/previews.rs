//! Previews: written after a save's record, replaced by the next save,
//! read from the end of the file, and never needed to open or save it.

use std::cell::RefCell;

use super::*;

fn png() -> Preview {
    Preview::new("image/png", b"\x89PNG\r\n\x1a\n not really".to_vec()).unwrap()
}

/// A preview of a type no reader shows.
fn unknown() -> Preview {
    Preview::new("application/x-varde-test", b"what".to_vec()).unwrap()
}

fn jpeg() -> Preview {
    Preview::new("image/jpeg", vec![0xff, 0xd8, 0xff, 0xd9]).unwrap()
}

/// The previews of the design file `file`.
fn previews_of(file: &(impl ReadAt + ?Sized)) -> Vec<Preview> {
    preview::previews(file).unwrap()
}

fn read_png(file: &(impl ReadAt + ?Sized)) -> Option<Preview> {
    read_preview(file, |preview| preview.is("image/png"))
}

/// Saves `document` and `previews` to the design file `file`, as a writer
/// with no failed save that read or wrote it as `tail`.
fn save_with(
    file: &mut impl Storage,
    tail: Tail,
    document: &Document,
    previews: &[Preview],
) -> Result<Tail> {
    let mut known = known_now(file, tail);
    save(file, &mut known, document, previews)
}

/// Where each block of the design file `bytes` from `from` on starts and
/// ends, by their first `len`s.
fn blocks_from(bytes: &[u8], from: u64) -> Vec<std::ops::Range<usize>> {
    let mut blocks = Vec::new();
    let mut at = from as usize;
    while at < bytes.len() {
        let end = at + (u64_at(bytes, at) + BLOCK_OVERHEAD) as usize;
        blocks.push(at..end);
        at = end;
    }
    blocks
}

#[test]
fn media_types_compare_without_case_and_parameters() {
    let preview = Preview::new("Image/PNG; foo=bar", vec![]).unwrap();
    assert!(preview.is("image/png"));
    assert!(preview.is(" image/png ;x=1"));
    assert!(!preview.is("image/pn"));
    assert!(!preview.is("image/png+x"));
    assert_eq!(preview.media_type(), "Image/PNG; foo=bar");
}

#[test]
fn a_preview_is_bounded() {
    let long = "a".repeat(MAX_MEDIA_TYPE);
    assert!(Preview::new(long.clone(), vec![0; MAX_PREVIEW]).is_some());
    assert!(Preview::new(long + "a", vec![]).is_none());
    assert!(Preview::new("image/png", vec![0; MAX_PREVIEW + 1]).is_none());

    // The largest preview there may be is read back.
    let largest = Preview::new("a".repeat(MAX_MEDIA_TYPE), noise(MAX_PREVIEW, 1)).unwrap();
    let (bytes, _) = to_bytes(&edited(1), std::slice::from_ref(&largest)).unwrap();
    assert_eq!(previews_of(bytes.as_slice()), [largest]);
}

/// One preview or several, of known types and not, round-trip through a
/// whole file, after its record: the file opens as it would without them,
/// at the record, and a save from its tail goes ahead.
#[test]
fn previews_round_trip() {
    for previews in [
        vec![png()],
        vec![unknown(), png(), jpeg()],
        vec![png(), png()],
    ] {
        let (bytes, tail) = to_bytes(&edited(1), &previews).unwrap();
        assert_eq!(previews_of(bytes.as_slice()), previews);
        assert_eq!(read_png(bytes.as_slice()), Some(png()));
        assert_eq!(
            read_preview(bytes.as_slice(), |preview| preview.is("image/jpeg")),
            previews
                .iter()
                .find(|preview| preview.is("image/jpeg"))
                .cloned()
        );

        let (document, at, report) = opened(&bytes);
        assert_eq!((document, at), (edited(1), tail));
        assert_eq!((report.outcome, report.unreadable), (Outcome::Intact, 0));
        assert_eq!(blocks_from(&bytes, tail.end).len(), previews.len());
        check(bytes.as_slice(), tail).unwrap();
    }
    // A type the reader doesn't show is skipped.
    let (bytes, _) = to_bytes(&edited(1), &[unknown()]).unwrap();
    assert_eq!(read_png(bytes.as_slice()), None);
    assert_eq!(previews_of(bytes.as_slice()), [unknown()]);
    // None at all.
    let (bytes, tail) = to_bytes(&edited(1), &[]).unwrap();
    assert_eq!(previews_of(bytes.as_slice()), []);
    assert_eq!(tail.end as usize, bytes.len());
}

/// No more than [`MAX_PREVIEWS`] are written, so reading, walking back over
/// at most that many blocks, always reaches the record. One more block
/// there, of any kind, and it doesn't.
#[test]
fn at_most_max_previews_are_written_and_read() {
    let many: Vec<_> = (0..MAX_PREVIEWS + 2)
        .map(|i| Preview::new(format!("image/x-{i}"), vec![i as u8]).unwrap())
        .collect();
    let (bytes, tail) = to_bytes(&edited(1), &many).unwrap();
    assert_eq!(previews_of(bytes.as_slice()), many[..MAX_PREVIEWS]);
    assert_eq!(opened(&bytes).1, tail);

    // Framed by hand: one block too many.
    let blocks = blocks_from(&bytes, tail.end);
    let last = blocks.last().unwrap();
    let sum = u128_at(&bytes, last.start + SUM_AT);
    let (extra, _) = frame(
        id_of(&bytes),
        sum,
        last.end as u64,
        PREVIEW,
        b"image/png",
        b"x",
    )
    .unwrap();
    let file = [&bytes[..], &extra].concat();
    assert_eq!(previews_of(file.as_slice()), []);
    assert_eq!(opened(&file).1, tail);
}

/// Blocks of a kind the reader doesn't know, between the record and the
/// previews, are stepped over, and so are previews too large to be one.
#[test]
fn other_kinds_before_the_previews_are_stepped_over() {
    let (bytes, tail) = to_bytes(&edited(1), &[]).unwrap();
    let id = id_of(&bytes);
    let (unknown_kind, after) = frame(id, tail.sum, tail.end, [7; 16], b"", b"what").unwrap();
    // `MAX_PREVIEW` bytes, and a longer tag than a preview may have.
    let tag = vec![b'a'; MAX_MEDIA_TYPE + 1];
    let (too_large, after) = frame(
        id,
        after.sum,
        after.end,
        PREVIEW,
        &tag,
        &vec![0; MAX_PREVIEW],
    )
    .unwrap();
    // A media type that isn't UTF-8.
    let (not_utf8, after) = frame(id, after.sum, after.end, PREVIEW, b"\xff", b"x").unwrap();
    let preview = png();
    let (block, _) = frame(
        id,
        after.sum,
        after.end,
        PREVIEW,
        preview.media_type().as_bytes(),
        preview.data(),
    )
    .unwrap();
    let file = [&bytes[..], &unknown_kind, &too_large, &not_utf8, &block].concat();
    assert_eq!(previews_of(file.as_slice()), [png()]);
    let (document, at, report) = opened(&file);
    assert_eq!((document, at), (edited(1), tail));
    assert_eq!(report.outcome, Outcome::Intact);
    check(file.as_slice(), tail).unwrap();

    // Damaged, the block of the kind the reader doesn't know ends them.
    let damaged = flip(&file, tail.end as usize + TAG_LEN_AT + 3);
    assert_eq!(previews_of(damaged.as_slice()), []);
    assert_eq!(opened(&damaged).2.outcome, Outcome::TornTail);
    // So does one whose `sum` checks but whose `tag_len` is longer than
    // the block: not intact, as reading has it.
    let mut damaged = file.clone();
    let start = tail.end as usize;
    let end = start + unknown_kind.len();
    damaged[start + TAG_LEN_AT..start + TAG_LEN_AT + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    let sum = block_sum(id, &damaged[start..end]);
    damaged[start + SUM_AT..start + SUM_AT + 16].copy_from_slice(&sum.to_le_bytes());
    assert_eq!(previews_of(damaged.as_slice()), []);
    assert_eq!(opened(&damaged).2.outcome, Outcome::TornTail);
}

/// Each save replaces the previews: the old ones, after its tail, are cut
/// off, and the new ones follow its record.
#[test]
fn previews_are_replaced_on_save() {
    let mut file = Memory::default();
    let mut known = write_new(&mut file, &edited(1), &[png(), unknown()]).unwrap();
    assert_eq!(previews_of(&file), [png(), unknown()]);

    let saved = save(&mut file, &mut known, &edited(2), &[jpeg()]).unwrap();
    assert_eq!(saved.last, opened(&file.bytes).1.last);
    assert_eq!(previews_of(&file), [jpeg()]);
    let (document, tail, report) = opened(&file.bytes);
    assert_eq!((document, tail), (edited(2), saved));
    assert_eq!(report.outcome, Outcome::Intact);

    let saved = save(&mut file, &mut known, &edited(3), &[]).unwrap();
    assert_eq!(previews_of(&file), []);
    assert_eq!(file.bytes.len() as u64, saved.end);
    let saved = save(&mut file, &mut known, &edited(4), &[png()]).unwrap();
    assert_eq!(previews_of(&file), [png()]);
    assert_eq!(opened(&file.bytes).1, saved);
}

/// A save's record and previews go in one write, and a failed one is cut
/// back whole; one that couldn't be cut back is saved over.
#[test]
fn a_failed_save_with_previews_is_cut_back() {
    for mut file in failures() {
        file.stuck = true;
        let mut known = write_new(&mut file.file, &edited(0), &[png()]).unwrap();
        assert!(save(&mut file, &mut known, &edited(1), &[jpeg(), png()]).is_err());
        file.mend();
        let saved = save(&mut file, &mut known, &edited(2), &[unknown()]).unwrap();
        assert_eq!(read(&file).unwrap(), (edited(2), saved));
        assert_eq!(previews_of(&file), [unknown()]);
    }
}

/// Damage to any part of a preview block keeps the file opening, at its
/// record, silently, and saving; the damaged preview is never read, nor
/// those after it. A preview whose kind is damaged into a record's isn't
/// taken for one.
#[test]
fn damaged_previews_are_ignored() {
    let (bytes, tails) = saved(2);
    let mut file = Memory { bytes, syncs: 0 };
    let previews = [png(), jpeg()];
    let tail = save_with(&mut file, tails[1], &edited(3), &previews).unwrap();
    let bytes = file.bytes;
    let blocks = blocks_from(&bytes, tail.end);
    assert_eq!(blocks.len(), 2);
    for (i, block) in blocks.iter().enumerate() {
        let tag_len = PREVIEW_TAG_LEN[i];
        for (part, at) in [
            ("len", 3),
            ("prev", PREV_AT + 2),
            ("sum", SUM_AT + 5),
            ("tag_len", TAG_LEN_AT),
            ("tag", TAG_LEN_AT + 2 + 1),
            ("data", TAG_LEN_AT + 2 + tag_len + 1),
            ("trailing len", block.len() - BACK_LEN),
            ("kind", block.len() - 3),
        ] {
            let damaged = flip(&bytes, block.start + at);
            let (document, at, report) = opened(&damaged);
            assert_eq!((document, at), (edited(3), tail), "{i} {part}");
            assert_eq!(
                (report.outcome, report.unreadable),
                (Outcome::TornTail, 0),
                "{i} {part}"
            );
            let read = previews_of(damaged.as_slice());
            assert!(previews[..i].starts_with(&read), "{i} {part}: {read:?}");

            let mut file = Memory {
                bytes: damaged,
                syncs: 0,
            };
            let saved = save_with(&mut file, tail, &edited(4), &[unknown()]).unwrap();
            assert_eq!(saved.last, tail.end, "{i} {part}");
            let (document, at, report) = opened(&file.bytes);
            assert_eq!((document, at), (edited(4), saved), "{i} {part}");
            assert_eq!(report.outcome, Outcome::Intact, "{i} {part}");
            assert_eq!(previews_of(&file), [unknown()], "{i} {part}");
        }
    }

    // A kind damaged into a record's: not intact, so not a record, though
    // framed like one, so reading takes it for the newest save, damaged,
    // and opens the record before it, the newest; a save goes after it.
    for (i, block) in blocks.iter().enumerate() {
        let mut damaged = bytes.clone();
        damaged[block.end - 16..block.end].copy_from_slice(&RECORD);
        let read = from_bytes_with_report(&damaged).unwrap();
        assert_eq!((&read.payload, read.tail), (&edited(3), tail), "{i}");
        assert!(
            matches!(read.report.outcome, Outcome::NewestDamaged(d) if d.start == block.start as u64),
            "{i} {:?}",
            read.report.outcome
        );
        assert!(!previews_of(damaged.as_slice()).contains(&previews[i]));
        let mut file = Memory {
            bytes: damaged,
            syncs: 0,
        };
        let saved = save(&mut file, &mut read.known(), &edited(4), &[]).unwrap();
        assert_eq!(read_with_found(&file).unwrap().opened.tail, saved, "{i}");
    }
}

/// The length of the tag of each of `damaged_previews_are_ignored`'s
/// previews.
const PREVIEW_TAG_LEN: [usize; 2] = ["image/png".len(), "image/jpeg".len()];

/// A damaged preview that reading steps over by its header, to the
/// preview after it, is stepped over so by the check before a save too:
/// what follows is told as reading tells it. A record of the file's past
/// after them is a torn tail, saved over; the newest save, damaged, after
/// them is saved after.
#[test]
fn a_save_steps_over_a_damaged_preview_as_reading_does() {
    let (bytes, tail) = to_bytes(&edited(1), &[png(), jpeg()]).unwrap();
    let id = id_of(&bytes);
    let blocks = blocks_from(&bytes, tail.end);
    let damaged = flip(
        &bytes,
        blocks[0].start + TAG_LEN_AT + 2 + PREVIEW_TAG_LEN[0] + 1,
    );
    let last = u128_at(&bytes, blocks[1].start + SUM_AT);
    let end = bytes.len() as u64;

    // A record of the file's past: not following on.
    let (past, _) = record_at(id, tail.sum, end, FileType::Design, &edited(0)).unwrap();
    let file = [&damaged[..], &past].concat();
    let (document, at, report) = opened(&file);
    assert_eq!((document, at), (edited(1), tail));
    assert_eq!(report.outcome, Outcome::TornTail);
    check(file.as_slice(), tail).unwrap();
    let mut file = Memory {
        bytes: file,
        syncs: 0,
    };
    let saved = save_with(&mut file, tail, &edited(2), &[png()]).unwrap();
    assert_eq!(read(&file).unwrap(), (edited(2), saved));

    // The newest save, damaged, following on from the previews.
    let (newest, _) = record_at(id, last, end, FileType::Design, &edited(2)).unwrap();
    let newest = flip(&newest, newest.len() / 2);
    let file = [&damaged[..], &newest].concat();
    let read = from_bytes_with_report(&file).unwrap();
    assert_eq!((&read.payload, read.tail), (&edited(1), tail));
    assert!(
        matches!(read.report.outcome, Outcome::NewestDamaged(d) if d.start == end),
        "{:?}",
        read.report.outcome
    );
    let mut file = Memory {
        bytes: file,
        syncs: 0,
    };
    let saved = save(&mut file, &mut read.known(), &edited(3), &[]).unwrap();
    assert_eq!(saved.last, end + newest.len() as u64);
    assert_eq!(read_with_found(&file).unwrap().opened.tail, saved);
}

/// A preview that doesn't follow on from the newest record is ignored: one
/// of an older record, and one whose record didn't land.
#[test]
fn previews_of_another_record_are_ignored() {
    let (bytes, tail) = to_bytes(&edited(1), &[]).unwrap();
    let id = id_of(&bytes);
    let preview = |prev: u128, at: u64| {
        frame(id, prev, at, PREVIEW, b"image/png", png().data())
            .unwrap()
            .0
    };
    let (record, newest) = next(&bytes, tail, &edited(2));

    // After the newest record, of the one before.
    let file = [&bytes[..], &record, &preview(tail.sum, newest.end)].concat();
    assert_eq!(previews_of(file.as_slice()), []);
    let (document, at, report) = opened(&file);
    assert_eq!((document, at), (edited(2), newest));
    assert_eq!(report.outcome, Outcome::TornTail);
    // Before the newest record, following on from the one before.
    let before = preview(tail.sum, tail.end);
    let (record, newest) = next(
        &[&bytes[..], &before].concat(),
        Tail {
            end: tail.end + before.len() as u64,
            sum: u128_at(&before, SUM_AT),
            ..tail
        },
        &edited(2),
    );
    let file = [&bytes[..], &before, &record].concat();
    assert_eq!(opened(&file).1, newest);
    assert_eq!(previews_of(file.as_slice()), []);

    // Its record lost, as zeros or torn, by a power loss, and with a
    // preview of the record before it still there.
    let (record, newest) = next(&bytes, tail, &edited(2));
    let lost = preview(newest.sum, newest.end);
    let older = preview(tail.sum, tail.end);
    for gap in [vec![0; record.len()], record[..record.len() / 2].to_vec()] {
        let file = [&bytes[..], &gap, &lost].concat();
        assert_eq!(previews_of(file.as_slice()), [], "{}", gap.len());
        assert_eq!(opened(&file).1, tail);
    }
    let file = [&bytes[..], &older, &vec![0; 100][..], &lost].concat();
    assert_eq!(previews_of(file.as_slice()), []);
    assert_eq!(opened(&file).1, tail);
}

/// A file that reads every read it's asked to: where, and how much.
struct Counted<'a> {
    bytes: &'a [u8],
    reads: RefCell<Vec<std::ops::Range<u64>>>,
}

impl ReadAt for Counted<'_> {
    fn len(&self) -> io::Result<u64> {
        ReadAt::len(self.bytes)
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        self.reads.borrow_mut().push(at..at + buf.len() as u64);
        self.bytes.read_at(buf, at)
    }
}

/// Reading the previews reads no record's payload, only the newest
/// record's header, and the blocks after it.
#[test]
fn reading_previews_reads_no_record_payload() {
    let mut file = Memory::default();
    let mut tail = write_new(&mut file, &edited(30), &[png()]).unwrap().tail();
    let mut records = vec![tail];
    for i in 31..33 {
        tail = save_with(&mut file, tail, &edited(i), &[unknown(), png()]).unwrap();
        records.push(tail);
    }
    let counted = Counted {
        bytes: &file.bytes,
        reads: RefCell::new(Vec::new()),
    };
    assert_eq!(read_png(&counted), Some(png()));
    let reads = counted.reads.into_inner();
    let read: u64 = reads.iter().map(|read| read.end - read.start).sum();
    let after = file.bytes.len() as u64 - tail.end;
    // The file header, the trailing `len`, kind and header of each block
    // walking back to the record, and the blocks after it once each.
    let walked = 3 * (8 + 16 + TAG_LEN_AT as u64);
    assert_eq!(read, FILE_HEADER_LEN as u64 + walked + after, "{reads:?}");
    for record in records {
        let payload = record.last + TAG_LEN_AT as u64..record.end - BACK_LEN as u64;
        for read in &reads {
            assert!(
                read.end <= payload.start || read.start >= payload.end,
                "{read:?} of {payload:?}"
            );
        }
    }
}

/// Only a design file has previews: anything else, cut short, or not a
/// `.vrdp` file, has none.
#[test]
fn only_design_files_have_previews() {
    let (bytes, _) = to_bytes(&edited(1), &[png()]).unwrap();
    assert_eq!(previews_of(&bytes[..FILE_HEADER_LEN + 10]), []);
    assert_eq!(previews_of(&bytes[..FILE_HEADER_LEN - 1]), []);
    assert_eq!(previews_of(&bytes[1..]), []);
    assert_eq!(previews_of(&b""[..]), []);
    let mut held = HeldFile::<_, Document>::new(Memory::default());
    held.append(&edited(1)).unwrap();
    assert_eq!(previews_of(held.storage()), []);
}

/// A held file never gets previews: its blocks are its records. A
/// `PREVIEW` block in one is a kind it doesn't know, stepped over.
#[test]
fn held_files_never_get_previews() {
    let mut file = HeldFile::<_, Document>::new(Memory::default());
    file.append(&edited(1)).unwrap();
    file.append(&edited(2)).unwrap();
    file.clear().unwrap();
    file.append(&edited(3)).unwrap();
    file.append(&edited(4)).unwrap();
    let (id, tail) = held_at(&file);
    let bytes = file.into_storage().bytes;
    let blocks = blocks_from(&bytes, FILE_HEADER_LEN as u64);
    assert_eq!(blocks.len(), 2);
    for block in blocks {
        assert_eq!(bytes[block.end - 16..block.end], AUTOSAVE);
    }

    let (preview, _) = frame(id, tail.sum, tail.end, PREVIEW, b"image/png", b"x").unwrap();
    let mut file = HeldFile::<_, Document>::new(Memory {
        bytes: [&bytes[..], &preview].concat(),
        syncs: 0,
    });
    let read = file.read_with_report().unwrap().unwrap();
    assert_eq!((read.payload, read.tail), (edited(4), tail));
    assert_eq!(read.report.outcome, Outcome::Intact);
    file.append(&edited(5)).unwrap();
    assert_eq!(held_at(&file).1.last, tail.end);
}
