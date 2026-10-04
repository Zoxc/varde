use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use varde_document::{Command, Editor, Opacity, OriginPlane, Plane};

use super::check::{Append, Found, block_at};
use super::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("varde-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self) -> PathBuf {
        self.0.join("doc.vrdp")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The example design with `n` sketches, each version told apart.
fn edited(n: usize) -> Document {
    let mut editor = Editor::new(Document::example());
    for i in 0..n {
        editor
            .apply(Command::AddSketch {
                name: format!("Sketch {i}"),
                plane: Plane::Origin(OriginPlane::XY),
            })
            .unwrap();
    }
    editor.document().clone()
}

fn append(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

/// Reads the newest document of the design file `file`, with the file's
/// tail, as [`read_with_found`] does, less the rest.
fn read(file: &impl ReadAt) -> Result<(Document, Tail)> {
    read_with_found(file).map(|read| (read.opened.payload, read.opened.tail))
}

/// Reads the design at `path`, as the IO lane opens one, less the lock.
fn open_at(path: &Path) -> Result<(Document, Tail)> {
    read(&File::open(path)?)
}

/// Creates the design at `path` holding `document`, as the IO lane does,
/// less the lock, and returns its tail.
fn create_at(path: &Path, document: &Document) -> Tail {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    write_new(&mut file, document, &[]).unwrap().tail()
}

/// Saves `document` to the design at `path`, last read or written as
/// `tail`, as the IO lane does, less the lock.
fn save_at(path: &Path, tail: Tail, document: &Document) -> Result<Tail> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    save_to(&mut file, tail, document)
}

/// What a writer with no failed save knows of the design file `file`,
/// read or written as `tail`, its `id` taken from the file as it is now:
/// for tests not about telling another file by its `id`.
fn known_now(file: &(impl ReadAt + ?Sized), tail: Tail) -> Known {
    let mut header = [0; FILE_HEADER_LEN];
    let id = file
        .read_at(&mut header, 0)
        .map_or(0, |()| header_id(&header));
    Known::written(id, tail)
}

/// Saves `document` to the design file `file`, as a writer with no failed
/// save that read or wrote it as `tail`, see [`known_now`].
fn save_to(file: &mut impl Storage, tail: Tail, document: &Document) -> Result<Tail> {
    let mut known = known_now(file, tail);
    save(file, &mut known, document, &[])
}

/// [`check_unchanged`], for a writer with no failed save that read or wrote
/// the design file `file` as `tail`, see [`known_now`].
fn check(file: &[u8], tail: Tail) -> Result<Append> {
    check_unchanged(file, &known_now(file, tail))
}

#[test]
fn save_and_reopen() {
    let dir = TempDir::new("reopen");
    let tail = create_at(&dir.file(), &edited(0));
    let tail = save_at(&dir.file(), tail, &edited(1)).unwrap();
    let saved = save_at(&dir.file(), tail, &edited(2)).unwrap();

    let (doc, tail) = open_at(&dir.file()).unwrap();
    assert_eq!(doc, edited(2));
    assert_eq!(tail, saved);
    save_at(&dir.file(), tail, &edited(3)).unwrap();
    assert_eq!(open_at(&dir.file()).unwrap().0, edited(3));
}

/// A body's opacity is kept through a file.
#[test]
fn opacity_round_trips() {
    let mut editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let opacity = Opacity::new(35).unwrap();
    editor.apply(Command::SetOpacity(body, opacity)).unwrap();
    let document = editor.document();
    let (bytes, _) = to_bytes(document, &[]).unwrap();
    let (read, _) = from_bytes(&bytes).unwrap();
    assert_eq!(read.body(body).unwrap().opacity, opacity);
    assert_eq!(&read, document);
}

/// A move about a model face and a mirror in one are kept through a
/// file.
#[test]
fn a_move_and_a_mirror_round_trip() {
    use glam::DVec3;
    use varde_document::{AxisRef, FaceKey, FaceRef, FeatureKind, Mirror, Move, PartKey, PlaneRef};
    use varde_expr::Value;
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let design = editor.document().design();
    let length = |text: &str| Value::new(text, &Move::offset_ask(&design)).unwrap();
    let top = FaceRef {
        body: plate,
        key: FaceKey {
            feature: editor.document().features()[1].id.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::new(0.0, 15.0, 10.0),
    };
    let moved = Move {
        bodies: vec![plate],
        offset: [length("1 in"), length("-2"), length("0")],
        turn: Some((
            AxisRef::Face(top),
            Value::new("-30", &Move::angle_ask(&design)).unwrap(),
        )),
    };
    let mirror = Mirror {
        bodies: vec![plate],
        plane: PlaneRef::Face(top),
        keep_original: true,
    };
    for kind in [FeatureKind::from(moved), mirror.into()] {
        editor.apply(editor.document().add_feature(kind)).unwrap();
    }
    let document = editor.document();
    let (bytes, _) = to_bytes(document, &[]).unwrap();
    let (read, _) = from_bytes(&bytes).unwrap();
    assert_eq!(&read, document);
}

/// Patterns, linear and circular, are kept through a file, an axis on a
/// copy's face too.
#[test]
fn patterns_round_trip() {
    use glam::DVec3;
    use varde_document::{
        Axis3, AxisRef, FaceKey, FaceRef, FeatureKind, PartKey, Pattern, PatternKind,
    };
    use varde_expr::Value;
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let design = editor.document().design();
    let count = |text: &str| Value::new(text, &Pattern::count_ask(&design)).unwrap();
    let row = Pattern {
        bodies: vec![plate],
        kind: PatternKind::Linear {
            along: AxisRef::Origin(Axis3::Y),
            count: count("3"),
            spacing: Value::new("-2 in", &Pattern::spacing_ask(&design)).unwrap(),
        },
        copies: Default::default(),
    };
    editor
        .apply(editor.document().add_feature(row.into()))
        .unwrap();
    let row = editor.document().features()[2].id;
    let wall = FaceRef {
        body: plate,
        key: FaceKey {
            feature: editor.document().features()[1].id.get(),
            part: PartKey::EndCap,
            instance: 0,
        }
        .copy(row.get(), 2),
        near: DVec3::new(0.0, 15.0, 10.0),
    };
    let ring = Pattern {
        bodies: vec![plate],
        kind: PatternKind::Circular {
            about: AxisRef::Face(wall),
            count: count("2 * 3"),
            angle: Value::new("90", &Pattern::angle_ask(&design)).unwrap(),
        },
        copies: Default::default(),
    };
    editor
        .apply(editor.document().add_feature(FeatureKind::from(ring)))
        .unwrap();
    let document = editor.document();
    let (bytes, _) = to_bytes(document, &[]).unwrap();
    let (read, _) = from_bytes(&bytes).unwrap();
    assert_eq!(&read, document);
}

/// A combine is kept through a file.
#[test]
fn a_combine_round_trips() {
    use varde_document::{BodyId, BodyOp, Combine, Extrude, FeatureKind, Operation};
    let mut editor = Editor::new(Document::example());
    let FeatureKind::Extrude(extrude) = &editor.document().features()[1].kind else {
        panic!("the example's extrude");
    };
    let again = Extrude {
        operation: Operation::NewBody(BodyId::NEW),
        flip: true,
        ..extrude.clone()
    };
    editor
        .apply(editor.document().add_feature(again.into()))
        .unwrap();
    let [first, second] = [0, 1].map(|k| editor.document().bodies()[k].id);
    let combine = Combine {
        target: second,
        tools: vec![first],
        op: BodyOp::Intersect,
        keep_tools: true,
    };
    editor
        .apply(editor.document().add_feature(combine.clone().into()))
        .unwrap();
    let document = editor.document();
    let (bytes, _) = to_bytes(document, &[]).unwrap();
    let (read, _) = from_bytes(&bytes).unwrap();
    assert_eq!(&read, document);
    let last = &read.features().last().unwrap().kind;
    assert_eq!(*last, FeatureKind::Combine(combine));
}

#[test]
fn concurrent_save_conflicts() {
    let dir = TempDir::new("conflict");
    let tail = create_at(&dir.file(), &edited(0));
    save_at(&dir.file(), tail, &edited(1)).unwrap();
    assert!(matches!(
        save_at(&dir.file(), tail, &edited(2)),
        Err(Error::Conflict)
    ));
    assert_eq!(open_at(&dir.file()).unwrap().0, edited(1));
}

/// Storage that fails as told, over a file in memory.
#[derive(Debug, Default)]
struct Failing {
    file: Memory,
    /// Writes fail having written this many bytes, or all of theirs.
    write: Option<usize>,
    /// Syncs fail.
    sync: bool,
    /// Once writing or syncing fails, truncating fails too.
    stuck: bool,
    /// Writing or syncing has failed.
    failed: bool,
}

impl Failing {
    /// Stops failing, keeping the file.
    fn mend(&mut self) {
        let file = std::mem::take(&mut self.file);
        *self = Failing {
            file,
            ..Failing::default()
        };
    }

    fn fail(&mut self, error: &str) -> io::Result<()> {
        self.failed = true;
        Err(io::Error::other(error.to_owned()))
    }
}

impl ReadAt for Failing {
    fn len(&self) -> io::Result<u64> {
        self.file.len()
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        self.file.read_at(buf, at)
    }
}

impl Storage for Failing {
    fn write_at(&mut self, buf: &[u8], at: u64) -> io::Result<()> {
        let Some(written) = self.write else {
            return self.file.write_at(buf, at);
        };
        self.file.write_at(&buf[..written.min(buf.len())], at)?;
        self.fail("write failed")
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        if self.stuck && self.failed {
            return Err(io::Error::other("truncate failed"));
        }
        self.file.truncate(len)
    }

    fn sync(&mut self) -> io::Result<()> {
        if self.sync {
            return self.fail("fsync failed");
        }
        self.file.sync()
    }
}

/// How a save's writes fail: partway, having landed after all, or in the
/// sync after.
fn failures() -> [Failing; 3] {
    [
        Failing {
            write: Some(10),
            ..Failing::default()
        },
        Failing {
            write: Some(usize::MAX),
            ..Failing::default()
        },
        Failing {
            sync: true,
            ..Failing::default()
        },
    ]
}

/// A save whose writes fail leaves the file at the version before, and the
/// next save goes ahead rather than finding its own record a conflict.
#[test]
fn a_failed_save_is_cut_back() {
    for mut file in failures() {
        let mut known = write_new(&mut file.file, &edited(0), &[]).unwrap();
        let before = file.file.bytes.clone();
        let failed = save(&mut file, &mut known, &edited(1), &[]);
        assert!(matches!(failed, Err(Error::Io(_))), "{file:?}");
        assert!(known.attempt.is_some());
        let mut file = file.file;
        assert_eq!(file.bytes, before);
        save(&mut file, &mut known, &edited(1), &[]).unwrap();
        assert_eq!(known.attempt, None);
        assert_eq!(read(&file).unwrap().0, edited(1));
    }
}

/// A failed save whose record can't be cut back either, which may have
/// landed, isn't a conflict for the next save, which saves over it, as
/// long as it remembers the attempt.
#[test]
fn a_failed_save_left_in_the_file_is_not_a_conflict() {
    for mut file in failures() {
        file.stuck = true;
        let whole = file.write != Some(10);
        let mut known = write_new(&mut file.file, &edited(0), &[]).unwrap();
        let tail = known.tail();
        assert!(save(&mut file, &mut known, &edited(1), &[]).is_err());
        // Stuck: a save after it fails before writing, which leaves the
        // record there, and the attempt as it was.
        let remembered = known;
        assert!(save(&mut file, &mut known, &edited(2), &[]).is_err());
        assert_eq!(known, remembered);

        // Without the attempt, a whole record left is someone else's; a
        // torn one isn't anyone's.
        file.mend();
        let mut copy = Memory {
            bytes: file.file.bytes.clone(),
            syncs: 0,
        };
        let forgotten = save_to(&mut copy, tail, &edited(3));
        assert_eq!(matches!(forgotten, Err(Error::Conflict)), whole);

        let saved = save(&mut file, &mut known, &edited(3), &[]).unwrap();
        assert_eq!(known.attempt, None);
        assert_eq!(saved.last, tail.end);
        assert_eq!(read(&file).unwrap(), (edited(3), saved));
    }
}

/// The `id` of the file `bytes`.
fn id_of(bytes: &[u8]) -> u128 {
    header_id(bytes[..FILE_HEADER_LEN].try_into().unwrap())
}

/// The record a save of `document` appends to the design file `bytes`,
/// read or written as `tail`, and the file's tail after it.
fn next(bytes: &[u8], tail: Tail, document: &Document) -> (Vec<u8>, Tail) {
    record_at(id_of(bytes), tail.sum, tail.end, FileType::Design, document).unwrap()
}

/// What `file` holds as last read or written: its `id` and tail.
fn held_at<S: Storage, P: Payload>(file: &HeldFile<S, P>) -> (u128, Tail) {
    match file.held {
        Held::At { id, tail, .. } => (id, tail),
        held => panic!("{held:?}"),
    }
}

/// The record appending `payload` to `file` writes.
fn next_held<S: Storage, P: Payload>(file: &HeldFile<S, P>, payload: &P) -> Vec<u8> {
    let (id, tail) = held_at(file);
    record_at(id, tail.sum, tail.end, FileType::Held, payload)
        .unwrap()
        .0
}

/// A failed save's record after the tail is ours, once: then only a torn
/// tail may follow, and any other record is someone else's.
#[test]
fn a_failed_save_is_ours_only_once() {
    let (bytes, tail) = to_bytes(&edited(1), &[]).unwrap();
    let (record, new) = next(&bytes, tail, &edited(2));
    let (other, _) = next(&bytes, tail, &edited(3));
    let (after, _) = next(&bytes, new, &edited(3));
    let attempt = Some(Attempt::of(&record));
    let check = |rest: &[&[u8]], attempt| {
        let file = [&bytes[..], &rest.concat()].concat();
        let known = Known {
            attempt,
            ..Known::written(id_of(&bytes), tail)
        };
        format!("{:?}", check_unchanged(file.as_slice(), &known).map(|_| ()))
    };
    let ok = "Ok(())";
    let conflict = "Err(Conflict)";
    assert_eq!(check(&[&record], attempt), ok);
    assert_eq!(check(&[&record[..record.len() - 1]], attempt), ok);
    assert_eq!(check(&[&record, &after[..10]], attempt), ok);
    // A copy of it doesn't follow on from it: a torn tail.
    assert_eq!(check(&[&record, &record], attempt), ok);
    assert_eq!(check(&[&record], None), conflict);
    assert_eq!(check(&[&other], attempt), conflict);
    assert_eq!(check(&[&record, &after], attempt), conflict);
    let mut damaged = after.clone();
    *damaged.last_mut().unwrap() ^= 1;
    assert_eq!(check(&[&record, &damaged, &after], attempt), "Err(Damaged)");
}

#[test]
fn replaced_file_conflicts() {
    let dir = TempDir::new("replaced");
    let tail = create_at(&dir.file(), &edited(0));
    std::fs::remove_file(dir.file()).unwrap();
    create_at(&dir.file(), &edited(1));
    assert!(matches!(
        save_at(&dir.file(), tail, &edited(2)),
        Err(Error::Conflict)
    ));
    // Even with the same document: it's another file.
    std::fs::remove_file(dir.file()).unwrap();
    create_at(&dir.file(), &edited(0));
    assert!(matches!(
        save_at(&dir.file(), tail, &edited(2)),
        Err(Error::Conflict)
    ));
}

#[test]
fn torn_append_is_ignored_and_truncated() {
    let dir = TempDir::new("torn");
    let mut tail = create_at(&dir.file(), &edited(0));
    let bytes = std::fs::read(dir.file()).unwrap();
    let (record, _) = next(&bytes, tail, &edited(1));
    let front = &record[..TAG_LEN_AT];
    let back = record.len() - BACK_LEN;

    // The front only, zeros after it to the block's length, a missing
    // back, a zeroed back, and a zeroed front too.
    for torn in [
        front.to_vec(),
        [front, &vec![0; record.len() - front.len()]].concat(),
        record[..back].to_vec(),
        [&record[..back], &[0; BACK_LEN][..]].concat(),
        vec![0; record.len()],
    ] {
        let len = std::fs::metadata(dir.file()).unwrap().len();
        append(&dir.file(), &torn);
        assert_eq!(open_at(&dir.file()).unwrap(), (edited(0), tail));
        tail = save_at(&dir.file(), tail, &edited(0)).unwrap();
        assert_eq!(tail.last, len);
        assert_eq!(std::fs::metadata(dir.file()).unwrap().len(), tail.end);
    }
}

#[test]
fn rejects_other_files() {
    let dir = TempDir::new("other");
    std::fs::write(dir.file(), b"hello").unwrap();
    assert!(matches!(open_at(&dir.file()), Err(Error::NotVarde)));
}

#[test]
fn rejects_other_versions() {
    let dir = TempDir::new("version");
    create_at(&dir.file(), &edited(0));
    let mut bytes = std::fs::read(dir.file()).unwrap();
    bytes[25] = 2;
    std::fs::write(dir.file(), bytes).unwrap();
    assert!(matches!(
        open_at(&dir.file()),
        Err(Error::UnsupportedVersion(2))
    ));
}

/// A held file opened as a design, and a design as a held file, are each
/// refused by their header, saying what they are.
#[test]
fn each_file_type_is_refused_as_the_other() {
    let mut held = HeldFile::new(Memory::default());
    held.append(&edited(1)).unwrap();
    let held = held.into_storage().bytes;
    let (design, _) = to_bytes(&edited(1), &[]).unwrap();

    let as_design = from_bytes(&held).unwrap_err();
    assert!(matches!(as_design, Error::IsAutoSave));
    assert_eq!(as_design.to_string(), "an auto-save, not a design");
    let mut file = HeldFile::<_, Document>::new(Memory {
        bytes: design.clone(),
        syncs: 0,
    });
    let as_held = file.read().unwrap_err();
    assert!(matches!(as_held, Error::IsDesign));
    assert_eq!(as_held.to_string(), "a design, not an auto-save");
    // The header alone says so, of another version too.
    for len in [25, FILE_HEADER_LEN] {
        let mut other = held[..len].to_vec();
        assert!(matches!(
            check_file_header(&other, FileType::Design),
            Err(Error::IsAutoSave)
        ));
        other[..25].copy_from_slice(DESIGN_MAGIC);
        assert!(matches!(
            check_file_header(&other, FileType::Held),
            Err(Error::IsDesign)
        ));
    }
    // Nor does the web's check before a save take one for the other.
    let (_, tail) = to_bytes(&edited(1), &[]).unwrap();
    assert!(matches!(check(held.as_slice(), tail), Err(Error::Conflict)));

    // A design where a held file should be is started over by appending,
    // as anything it can't read.
    let mut file = HeldFile::<_, Document>::new(Memory {
        bytes: design,
        syncs: 0,
    });
    file.append(&edited(2)).unwrap();
    assert_eq!(file.read().unwrap(), Some(edited(2)));
    assert!(matches!(
        from_bytes(&file.into_storage().bytes),
        Err(Error::IsAutoSave)
    ));
}

/// A huge file that isn't a design, like a renamed disk image, is refused
/// from its start, not read into memory first. Sparse, so it takes no space.
#[test]
fn rejects_huge_other_files_from_their_start() {
    let dir = TempDir::new("huge");
    std::fs::write(dir.file(), b"hello").unwrap();
    File::options()
        .write(true)
        .open(dir.file())
        .unwrap()
        .set_len(1 << 40)
        .unwrap();
    assert!(matches!(open_at(&dir.file()), Err(Error::NotVarde)));

    let mut bytes = FileType::Design.header(1).to_vec();
    bytes[25] = 2;
    std::fs::write(dir.file(), bytes).unwrap();
    File::options()
        .write(true)
        .open(dir.file())
        .unwrap()
        .set_len(1 << 40)
        .unwrap();
    assert!(matches!(
        open_at(&dir.file()),
        Err(Error::UnsupportedVersion(2))
    ));
}

/// Storage claiming to be larger than memory can hold, starting with
/// `head` and zeros after, counting the bytes read from it.
struct Huge {
    head: Vec<u8>,
    read: std::cell::Cell<u64>,
}

impl ReadAt for Huge {
    fn len(&self) -> io::Result<u64> {
        Ok(u64::MAX >> 1)
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        for (i, b) in buf.iter_mut().enumerate() {
            let at = usize::try_from(at).unwrap() + i;
            *b = self.head.get(at).copied().unwrap_or(0);
        }
        self.read.set(self.read.get() + buf.len() as u64);
        Ok(())
    }
}

impl Storage for Huge {
    fn write_at(&mut self, _: &[u8], _: u64) -> io::Result<()> {
        unreachable!()
    }

    fn truncate(&mut self, _: u64) -> io::Result<()> {
        unreachable!()
    }

    fn sync(&mut self) -> io::Result<()> {
        unreachable!()
    }
}

/// Only the file header of a huge file is read before it's refused, and
/// one too large for memory is an error, not an abort.
#[test]
fn a_file_too_large_for_memory_is_refused() {
    let mut file = HeldFile::<_, Document>::new(Huge {
        head: b"hello".to_vec(),
        read: Default::default(),
    });
    assert!(matches!(file.read(), Err(Error::NotVarde)));
    assert_eq!(file.storage().read.get(), FILE_HEADER_LEN as u64);

    let mut file = HeldFile::<_, Document>::new(Huge {
        head: FileType::Held.header(1).to_vec(),
        read: Default::default(),
    });
    assert!(matches!(
        file.read(),
        Err(Error::Io(e)) if e.kind() == io::ErrorKind::OutOfMemory
    ));
    assert_eq!(file.storage().read.get(), FILE_HEADER_LEN as u64);
}

/// The buffer the web reads a file of the user's into is only made once
/// its start is checked, and fallibly: a design's head grows to the file's
/// length, anything else, or a length too large, is an error.
#[test]
fn whole_read_checks_first() {
    let head = FileType::Design.header(7).to_vec();
    let read = WholeRead::new(100);
    assert_eq!(read.head_len(), FILE_HEADER_LEN);
    let bytes = read.buffer(head.clone()).unwrap();
    assert_eq!(bytes.len(), 100);
    assert!(bytes.starts_with(&head));
    let short = WholeRead::new(3);
    assert_eq!(short.head_len(), 3);
    assert_eq!(short.buffer(b"var".to_vec()).unwrap(), b"var");

    let huge = WholeRead::new(usize::MAX);
    let mut other = head.clone();
    other[0] ^= 1;
    assert!(matches!(huge.buffer(other), Err(Error::NotVarde)));
    let held = FileType::Held.header(7).to_vec();
    assert!(matches!(huge.buffer(held), Err(Error::IsAutoSave)));
    assert!(matches!(
        huge.buffer(head),
        Err(Error::Io(e)) if e.kind() == io::ErrorKind::OutOfMemory
    ));
}

/// A head shorter than asked for would pass the header check whatever
/// the file holds, so it's a bug, not a design.
#[test]
#[should_panic]
fn whole_read_wants_the_whole_head() {
    let _ = WholeRead::new(100).buffer(Vec::new());
}

/// A save after someone appended a lot reads only the next block to tell
/// a newer version, or other content, from a torn tail, rather than all
/// of it, which could be more than memory holds.
#[test]
fn a_save_reads_only_the_next_block() {
    let dir = TempDir::new("save-huge");
    let tail = create_at(&dir.file(), &edited(0));
    save_at(&dir.file(), tail, &edited(1)).unwrap();
    // Sparse, so it takes no space.
    File::options()
        .write(true)
        .open(dir.file())
        .unwrap()
        .set_len(1 << 40)
        .unwrap();
    assert!(matches!(
        save_at(&dir.file(), tail, &edited(2)),
        Err(Error::Conflict)
    ));

    // A newer record, and one with a damaged kind, in storage too large
    // for memory: only its `len` and then the block are read.
    let (first, tail) = to_bytes(&edited(0), &[]).unwrap();
    let (second, _) = next(&first, tail, &edited(1));
    let mut damaged = second.clone();
    *damaged.last_mut().unwrap() ^= 0xff;
    let id = id_of(&first);
    for (after, corrupt) in [(second, false), (damaged, true)] {
        let huge = Huge {
            head: [&first[..], &after].concat(),
            read: Default::default(),
        };
        let found = block_at(&huge, tail.end, huge.len().unwrap(), id).unwrap();
        assert_eq!(matches!(found, Found::Corrupt), corrupt);
        assert!(matches!(found, Found::Intact(_)) != corrupt);
        assert_eq!(huge.read.get(), 8 + after.len() as u64);
    }
}

/// A block's `len` is bounded by `MAX_BLOCK` before anything is allocated
/// for it, wherever it's read, and writers keep to the same bounds.
#[test]
fn block_lengths_are_bounded() {
    assert_eq!(
        MAX_BLOCK,
        snap::raw::max_compress_len(MAX_DECOMPRESSED) as u64 + 2 + u64::from(u16::MAX)
    );
    // On wasm too, where a `usize` is 32 bits.
    assert!(MAX_BLOCK + BLOCK_OVERHEAD <= u64::from(u32::MAX));

    let (first, tail) = to_bytes(&edited(0), &[]).unwrap();
    let id = id_of(&first);
    for claimed in [MAX_BLOCK + 1, u64::MAX - BLOCK_OVERHEAD, u64::MAX] {
        let huge = Huge {
            head: [&first[..], &claimed.to_le_bytes()].concat(),
            read: Default::default(),
        };
        let found = block_at(&huge, tail.end, huge.len().unwrap(), id).unwrap();
        // Within the file, but too long: damage, not a torn tail, and never
        // allocated for.
        let past_the_end = claimed
            .checked_add(BLOCK_OVERHEAD)
            .is_none_or(|size| size > huge.len().unwrap() - tail.end);
        assert_eq!(matches!(found, Found::Torn), past_the_end, "{claimed}");
        assert_eq!(matches!(found, Found::Corrupt), !past_the_end, "{claimed}");
        assert_eq!(huge.read.get(), 8);
        // In a whole file, cut off.
        let bytes = [&first[..], &claimed.to_le_bytes(), &[1; 100]].concat();
        assert_eq!(from_bytes(&bytes).unwrap(), (edited(0), tail));
    }

    // Writers check theirs: a tag too long for its `u16` is refused.
    let tag = vec![b'x'; usize::from(u16::MAX) + 1];
    assert!(matches!(
        frame(id, tail.sum, tail.end, RECORD, &tag, b""),
        Err(Error::TooLarge)
    ));
    // `len` covers `tag_len`, the tag and the payload; a block is 64 more.
    let (block, framed) = frame(id, tail.sum, tail.end, PREVIEW, b"image/png", b"png").unwrap();
    assert_eq!(block.len(), 64 + 2 + 9 + 3);
    assert_eq!(u64_at(&block, 0), 2 + 9 + 3);
    assert_eq!(framed.end - framed.last, block.len() as u64);
    assert!(writer().len() <= MAX_TAG);
}

/// A record's payload says how long it decompresses to, and the decoder
/// allocates that up front: a claim no payload of its length can back is
/// refused before anything is allocated.
#[test]
fn a_payload_claiming_more_than_it_can_hold_is_refused() {
    // A varint claiming 4 GiB - 1, then a few bytes of nothing much.
    let payload = [0xff, 0xff, 0xff, 0xff, 0x0f, 0, 0, 0];
    let error = decode::<Document>(&payload).unwrap_err();
    assert!(error.to_string().contains("claims"), "{error}");

    // Through a whole file, whose sums are fine.
    let dir = TempDir::new("snappy-claim");
    let header = FileType::Design.header(3);
    let (record, _) = frame(
        3,
        xxh3_128(&header),
        FILE_HEADER_LEN as u64,
        RECORD,
        writer().as_bytes(),
        &payload,
    )
    .unwrap();
    std::fs::write(dir.file(), [&header[..], &record].concat()).unwrap();
    assert!(matches!(
        open_at(&dir.file()),
        Err(Error::Decode { error, .. }) if error.to_string().contains("claims")
    ));

    // Snappy's densest output, of a run of zeros, is within the bound, and
    // real documents decode.
    let zeros = vec![0; 1 << 20];
    let dense = snap::raw::Encoder::new().compress_vec(&zeros).unwrap();
    assert!(zeros.len() <= dense.len() * MAX_EXPANSION);
    let large = edited(2000);
    assert_eq!(
        decode::<Document>(&encode(&large).unwrap()).unwrap().0,
        large
    );
}

/// A record that won't decode says who wrote it, from its tag, which
/// readers take at any length and show the start of.
#[test]
fn a_decode_error_names_the_writer() {
    let mut file = HeldFile::new(Memory::default());
    file.append(&Based {
        base: None,
        document: edited(1),
    })
    .unwrap();
    let mut plain = HeldFile::<_, Document>::new(file.into_storage());
    let error = plain.read().unwrap_err();
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        matches!(&error, Error::Decode { writer, .. } if *writer == format!("{APP_NAME} {version}")),
        "{error:?}"
    );
    assert!(
        error.to_string().starts_with(&format!(
            "couldn't decode document (saved by {APP_NAME} {version}, this is {version}): "
        )),
        "{error}"
    );

    let header = FileType::Design.header(5);
    let garbage = [0x05, 0xc1, 1, 2, 3, 4];
    for (tag, shown) in [
        (b"Varde CAD 9.9.9".to_vec(), "Varde CAD 9.9.9".to_owned()),
        (vec![b'a'; 2000], "a".repeat(128)),
        (b"bad \xff utf-8".to_vec(), "bad \u{fffd} utf-8".to_owned()),
    ] {
        let (record, _) = frame(
            5,
            xxh3_128(&header),
            FILE_HEADER_LEN as u64,
            RECORD,
            &tag,
            &garbage,
        )
        .unwrap();
        let error = from_bytes(&[&header[..], &record].concat()).unwrap_err();
        assert!(
            matches!(&error, Error::Decode { writer, .. } if *writer == shown),
            "{error:?}"
        );
        assert!(error.to_string().contains(&format!("saved by {shown},")));
    }
}

/// A file open for reading and writing, created if needed, as the IO lane
/// holds a sidecar.
fn held(path: &Path) -> HeldFile {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap();
    HeldFile::new(file)
}

#[test]
fn a_held_file_starts_empty_and_keeps_the_newest_document() {
    let dir = TempDir::new("held");
    let mut file = held(&dir.file());
    assert!(file.is_empty().unwrap());
    assert_eq!(file.read().unwrap(), None);
    file.append(&edited(1)).unwrap();
    file.append(&edited(2)).unwrap();
    assert_eq!(file.read().unwrap(), Some(edited(2)));
    // Not a design.
    assert!(matches!(open_at(&dir.file()), Err(Error::IsAutoSave)));
    // Read by another handle, which appends after what's there.
    file = held(&dir.file());
    file.append(&edited(3)).unwrap();
    assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(3)));

    file.clear().unwrap();
    assert!(file.is_empty().unwrap());
    assert_eq!(file.read().unwrap(), None);
    file.append(&edited(4)).unwrap();
    assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(4)));
}

/// The owner's lock on the file doesn't get in the way, where a lock of
/// its own would wait for it and fail.
#[test]
fn a_held_file_works_under_its_owners_lock() {
    let dir = TempDir::new("held-locked");
    let file = held(&dir.file());
    file.storage().try_lock().unwrap();
    let mut file = HeldFile::new(file.into_storage());
    file.append(&edited(1)).unwrap();
    file.append(&edited(2)).unwrap();
    assert_eq!(file.read().unwrap(), Some(edited(2)));
    file.clear().unwrap();
}

/// A crash while appending leaves the last complete document readable, and
/// the torn rest is dropped by the next append.
#[test]
fn a_held_file_ignores_a_torn_append() {
    let dir = TempDir::new("held-torn");
    let mut file = held(&dir.file());
    file.append(&edited(1)).unwrap();
    let len = std::fs::metadata(dir.file()).unwrap().len();
    let record = next_held(&file, &edited(2));
    append(&dir.file(), &record[..record.len() - 3]);
    assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(1)));

    let mut again = held(&dir.file());
    again.append(&edited(3)).unwrap();
    let (_, tail) = held_at(&again);
    assert_eq!(tail.last, len);
    assert_eq!(std::fs::metadata(dir.file()).unwrap().len(), tail.end);
    assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(3)));

    // Torn while writing the first document, even within the file header.
    for cut in [5, FILE_HEADER_LEN, FILE_HEADER_LEN + 10] {
        let mut file = held(&dir.file());
        file.clear().unwrap();
        file.append(&edited(1)).unwrap();
        let bytes = std::fs::read(dir.file()).unwrap();
        std::fs::write(dir.file(), &bytes[..cut]).unwrap();
        let mut file = held(&dir.file());
        assert_eq!(file.read().unwrap(), None, "cut at {cut}");
        file.append(&edited(2)).unwrap();
        assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(2)));
    }
}

/// What can't be read is an error, and appending starts over from what
/// isn't a held file; records framed but none intact are kept, until the
/// file is emptied.
#[test]
fn a_held_file_starts_over_from_garbage() {
    let dir = TempDir::new("held-garbage");
    std::fs::write(dir.file(), b"not a design at all").unwrap();
    let mut file = held(&dir.file());
    assert!(matches!(file.read(), Err(Error::NotVarde)));
    file.append(&edited(1)).unwrap();
    assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(1)));

    // Its one record damaged: no intact record.
    let mut bytes = std::fs::read(dir.file()).unwrap();
    bytes[FILE_HEADER_LEN + TAG_LEN_AT + 2] ^= 0xff;
    std::fs::write(dir.file(), bytes).unwrap();
    let damaged = std::fs::read(dir.file()).unwrap();
    let mut file = held(&dir.file());
    assert!(matches!(file.read(), Err(Error::Corrupt { .. })));
    let refused = file.append(&edited(3));
    assert!(matches!(refused, Err(Error::Corrupt { offset }) if offset == FILE_HEADER_LEN as u64));
    // Not read first, the same.
    assert!(matches!(
        held(&dir.file()).append(&edited(3)),
        Err(Error::Corrupt { .. })
    ));
    assert_eq!(std::fs::read(dir.file()).unwrap(), damaged);
    file.clear().unwrap();
    file.append(&edited(3)).unwrap();
    assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(3)));
    let opened = held(&dir.file()).read_with_report().unwrap().unwrap();
    assert_eq!(opened.report.outcome, Outcome::Intact);
}

#[test]
fn a_file_reads_and_writes_at_offsets_as_storage() {
    let dir = TempDir::new("storage");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(dir.file())
        .unwrap();
    assert_eq!(ReadAt::len(&file).unwrap(), 0);
    assert!(ReadAt::is_empty(&file).unwrap());

    file.write_at(b"hello", 0).unwrap();
    // Past the end: the gap is zeros.
    file.write_at(b"world", 7).unwrap();
    assert_eq!(ReadAt::len(&file).unwrap(), 12);
    let mut buf = [0; 5];
    file.read_at(&mut buf, 7).unwrap();
    assert_eq!(&buf, b"world");
    let mut all = [1; 12];
    file.read_at(&mut all, 0).unwrap();
    assert_eq!(&all, b"hello\0\0world");

    // Reading past the end fails rather than filling part of the buffer.
    let error = file.read_at(&mut buf, 9).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    assert!(file.read_at(&mut buf, u64::MAX).is_err());

    // Offsets don't depend on where the last read or write left off.
    file.write_at(b"J", 0).unwrap();
    file.read_at(&mut buf, 0).unwrap();
    assert_eq!(&buf, b"Jello");

    Storage::truncate(&mut file, 3).unwrap();
    file.sync().unwrap();
    assert_eq!(std::fs::read(dir.file()).unwrap(), b"Jel");
    Storage::truncate(&mut file, 0).unwrap();
    assert!(ReadAt::is_empty(&file).unwrap());
}

/// A file in memory, standing in for storage other than `std::fs`, like
/// the web's sync access handles.
#[derive(Debug, Default)]
struct Memory {
    bytes: Vec<u8>,
    syncs: u32,
}

impl Memory {
    fn range(&self, at: u64, len: usize) -> io::Result<std::ops::Range<usize>> {
        let start = usize::try_from(at).map_err(|_| io::ErrorKind::UnexpectedEof)?;
        let end = start.checked_add(len).ok_or(io::ErrorKind::UnexpectedEof)?;
        Ok(start..end)
    }
}

impl ReadAt for Memory {
    fn len(&self) -> io::Result<u64> {
        Ok(self.bytes.len() as u64)
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        let range = self.range(at, buf.len())?;
        let bytes = self.bytes.get(range).ok_or(io::ErrorKind::UnexpectedEof)?;
        buf.copy_from_slice(bytes);
        Ok(())
    }
}

impl Storage for Memory {
    fn write_at(&mut self, buf: &[u8], at: u64) -> io::Result<()> {
        let range = self.range(at, buf.len())?;
        if self.bytes.len() < range.end {
            self.bytes.resize(range.end, 0);
        }
        self.bytes[range].copy_from_slice(buf);
        Ok(())
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        let len = usize::try_from(len).map_err(|_| io::ErrorKind::FileTooLarge)?;
        self.bytes.resize(len, 0);
        Ok(())
    }

    fn sync(&mut self) -> io::Result<()> {
        self.syncs += 1;
        Ok(())
    }
}

/// An append whose writes fail is cut back, and one that can't be is read
/// again before the next append, which goes after it.
#[test]
fn a_held_file_append_that_fails_is_cut_back() {
    for stuck in [false, true] {
        for mut failing in failures() {
            let mut file = HeldFile::<_, Document>::new(Memory::default());
            file.append(&edited(1)).unwrap();
            let before = file.into_storage().bytes;
            failing.file.bytes = before.clone();
            failing.stuck = stuck;
            let mut file = HeldFile::new(failing);
            assert!(matches!(file.append(&edited(2)), Err(Error::Io(_))));
            let bytes = &file.file.file.bytes;
            assert!(bytes.starts_with(&before));
            let left = bytes.len() - before.len();
            match (stuck, file.file.write) {
                (false, _) => assert_eq!(left, 0),
                (true, Some(10)) => assert_eq!(left, 10),
                // The whole record, following on.
                (true, _) => {
                    let chain = Chain::scan(bytes, FileType::Held).unwrap();
                    assert_eq!(chain.records.len(), 2);
                    assert_eq!(chain.records[1].end, bytes.len() as u64);
                }
            }

            file.file.mend();
            file.append(&edited(3)).unwrap();
            assert_eq!(file.read().unwrap(), Some(edited(3)));
        }
    }
}

/// The format doesn't depend on `std::fs`: the same bytes in any storage.
#[test]
fn a_held_file_works_on_any_storage() {
    let mut file = HeldFile::new(Memory::default());
    assert!(file.is_empty().unwrap());
    assert_eq!(file.read().unwrap(), None);
    file.append(&edited(1)).unwrap();
    file.append(&edited(2)).unwrap();
    assert_eq!(file.read().unwrap(), Some(edited(2)));
    assert_eq!(file.storage().syncs, 2);

    // The same bytes as a file on disk.
    let dir = TempDir::new("held-memory");
    let torn_record = next_held(&file, &edited(3));
    let memory = file.into_storage();
    std::fs::write(dir.file(), &memory.bytes).unwrap();
    assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(2)));

    // A torn append leaves the last complete document, and the next
    // append drops it.
    let mut torn = memory.bytes.clone();
    torn.extend_from_slice(&torn_record[..torn_record.len() / 2]);
    let mut file = HeldFile::new(Memory {
        bytes: torn,
        syncs: 0,
    });
    assert_eq!(file.read().unwrap(), Some(edited(2)));
    file.append(&edited(4)).unwrap();
    assert_eq!(file.read().unwrap(), Some(edited(4)));
    assert_eq!(
        HeldFile::new(file.into_storage()).read().unwrap(),
        Some(edited(4))
    );

    let mut file = HeldFile::new(Memory::default());
    file.append(&edited(1)).unwrap();
    file.clear().unwrap();
    assert!(file.is_empty().unwrap());
    assert_eq!(file.read().unwrap(), None);
}

/// A file header cut short is a held file with no records yet, but not a
/// design; anything else is checked the same way for both.
#[test]
fn a_short_file_header_is_only_empty_when_held() {
    let held = |bytes: &[u8]| Chain::scan(bytes, FileType::Held).map(|chain| chain.records);
    let design = |bytes: &[u8]| Chain::scan(bytes, FileType::Design).map(|chain| chain.records);
    let held_header = FileType::Held.header(9);
    let design_header = FileType::Design.header(9);
    assert!(
        held(&held_header[..FILE_HEADER_LEN - 1])
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        design(&design_header[..FILE_HEADER_LEN - 1]),
        Err(Error::NotVarde)
    ));
    assert!(matches!(held(b"var-"), Err(Error::NotVarde)));

    for (check, header) in [
        (&held as &dyn Fn(&[u8]) -> Result<Vec<Block>>, held_header),
        (&design, design_header),
    ] {
        let mut other = header;
        other[25] = 2;
        assert!(matches!(check(&other), Err(Error::UnsupportedVersion(2))));
        assert!(matches!(check(b"hello"), Err(Error::NotVarde)));
        assert!(check(&header).unwrap().is_empty());
    }
}

/// The tail tells saved versions apart: each save gives another, another
/// handle sees the same one, and a file rewritten, even with the same
/// content, has another too.
#[test]
fn the_tail_identifies_the_saved_version() {
    let dir = TempDir::new("tail");
    let first = create_at(&dir.file(), &edited(0));
    assert_eq!(open_at(&dir.file()).unwrap().1, first);
    let second = save_at(&dir.file(), first, &edited(1)).unwrap();
    assert_ne!(second, first);
    assert_eq!(open_at(&dir.file()).unwrap().1, second);
    let third = save_at(&dir.file(), second, &edited(2)).unwrap();
    assert_eq!(open_at(&dir.file()).unwrap().1, third);
    assert_ne!(third, second);

    std::fs::remove_file(dir.file()).unwrap();
    let rewritten = create_at(&dir.file(), &edited(3));
    assert_ne!(rewritten, third);
    assert_ne!(rewritten, first);
    std::fs::remove_file(dir.file()).unwrap();
    let again = create_at(&dir.file(), &edited(0));
    assert_ne!(again, first);
}

/// Every block's `prev` is the `sum` of the block before it, the first
/// one's the hash of the file header; a design's records are `RECORD`
/// blocks and a held file's `AUTOSAVE` ones.
#[test]
fn blocks_chain() {
    let mut file = Memory::default();
    let mut tail = write_new(&mut file, &edited(0), &[]).unwrap().tail();
    for n in 1..4 {
        tail = save_to(&mut file, tail, &edited(n)).unwrap();
    }
    let mut held = HeldFile::new(Memory::default());
    for n in 0..4 {
        held.append(&edited(n)).unwrap();
    }
    let held = held.into_storage().bytes;
    for (bytes, ty, kind) in [
        (&file.bytes, FileType::Design, RECORD),
        (&held, FileType::Held, AUTOSAVE),
    ] {
        let chain = Chain::scan(bytes, ty).unwrap();
        assert_eq!(chain.records.len(), 4);
        let mut prev = xxh3_128(&bytes[..FILE_HEADER_LEN]);
        let mut at = FILE_HEADER_LEN as u64;
        for record in &chain.records {
            assert_eq!(record.kind, kind);
            assert_eq!((record.start, record.prev), (at, prev));
            // The stored sum is of the id and the rest of the block.
            let block = &bytes[record.start as usize..record.end as usize];
            assert_eq!(u128_at(block, SUM_AT), record.sum);
            assert_eq!(block_sum(chain.id, block), record.sum);
            assert_eq!(&block[block.len() - 16..], &kind);
            prev = record.sum;
            at = record.end;
        }
        assert_eq!(at, bytes.len() as u64);
    }
    assert_eq!(
        Chain::scan(&file.bytes, FileType::Design).unwrap().records[3].tail(),
        tail
    );
}

/// Appends keep a file's `id`; every write of a whole file makes a new one.
#[test]
fn the_id_is_kept_by_appends_and_new_for_whole_files() {
    let mut file = Memory::default();
    let tail = write_new(&mut file, &edited(0), &[]).unwrap().tail();
    let id = id_of(&file.bytes);
    let tail = save_to(&mut file, tail, &edited(1)).unwrap();
    assert_eq!(id_of(&file.bytes), id);
    save_to(&mut file, tail, &edited(2)).unwrap();
    assert_eq!(id_of(&file.bytes), id);
    let mut other = Memory::default();
    write_new(&mut other, &edited(0), &[]).unwrap().tail();
    assert_ne!(id_of(&other.bytes), id);
    let (first, _) = to_bytes(&edited(0), &[]).unwrap();
    let (second, _) = to_bytes(&edited(0), &[]).unwrap();
    assert_ne!(id_of(&first), id_of(&second));

    let mut held = HeldFile::<_, Document>::new(Memory::default());
    held.append(&edited(1)).unwrap();
    let (id, _) = held_at(&held);
    held.append(&edited(2)).unwrap();
    assert_eq!(held_at(&held).0, id);
    // Read again by another handle, which appends with it.
    let mut again = HeldFile::<_, Document>::new(held.into_storage());
    again.append(&edited(3)).unwrap();
    assert_eq!(held_at(&again).0, id);
    assert_eq!(id_of(&again.storage().bytes), id);
    again.clear().unwrap();
    again.append(&edited(4)).unwrap();
    let (cleared, _) = held_at(&again);
    assert_ne!(cleared, id);
    assert_eq!(id_of(&again.storage().bytes), cleared);
    // Started over from what it can't read.
    let mut garbage = HeldFile::<_, Document>::new(Memory {
        bytes: b"not an auto-save".to_vec(),
        syncs: 0,
    });
    garbage.append(&edited(1)).unwrap();
    assert_eq!(id_of(&garbage.storage().bytes), held_at(&garbage).0);
}

/// Blocks of another file never check in this one, even holding the same
/// document at the same place: stale bytes a file system leaves in space a
/// file grows into aren't taken for the file's own.
#[test]
fn a_block_of_another_file_never_checks() {
    let save_two = || {
        let mut file = Memory::default();
        let first = write_new(&mut file, &edited(1), &[]).unwrap().tail();
        let second = save_to(&mut file, first, &edited(2)).unwrap();
        (file.bytes, first, second)
    };
    let (ours, first, _) = save_two();
    let (theirs, their_first, their_second) = save_two();
    let ours = &ours[..first.end as usize];
    let stale = &theirs[their_first.end as usize..their_second.end as usize];

    // At the end: not intact, but framed like a record, so damage. The
    // file opens at our record, and a save that didn't know of it refuses
    // it as damage, as a save that did refuses a file read as damaged.
    let file = [ours, stale].concat();
    let opened = from_bytes_with_report(&file).unwrap();
    let mut known = opened.known();
    assert_eq!((opened.payload, opened.tail), (edited(1), first));
    assert_eq!(opened.report.outcome, Outcome::Damaged { found: None });
    assert!(matches!(
        block_at(&file[..], first.end, file.len() as u64, id_of(ours)),
        Ok(Found::Torn)
    ));
    assert!(matches!(check(&file, first), Err(Error::Damaged)));
    let mut memory = Memory {
        bytes: file.clone(),
        syncs: 0,
    };
    assert!(matches!(
        save(&mut memory, &mut known, &edited(3), &[]),
        Err(Error::OpenedDamaged)
    ));
    assert_eq!(memory.bytes, file);
    // Not taken for a record, with more after it, even searched for.
    let file = [ours, stale, stale].concat();
    let opened = from_bytes_with_report(&file).unwrap();
    assert_eq!((opened.payload, opened.tail), (edited(1), first));
    assert_eq!(opened.report.outcome, Outcome::Damaged { found: None });
    assert_eq!(opened.report.unreadable, 2 * stale.len() as u64);
    // Their whole file under our header: none of it.
    let file = [&ours[..FILE_HEADER_LEN], &theirs[FILE_HEADER_LEN..]].concat();
    assert!(
        matches!(from_bytes(&file), Err(Error::Corrupt { offset }) if offset == FILE_HEADER_LEN as u64)
    );
    // Cut short, it's not framed like a record: a torn tail, which saving
    // drops.
    let mut file = Memory {
        bytes: [ours, &stale[..stale.len() - 1]].concat(),
        syncs: 0,
    };
    let opened = from_bytes_with_report(&file.bytes).unwrap();
    assert_eq!(opened.report.outcome, Outcome::TornTail);
    let saved = save(&mut file, &mut opened.known(), &edited(3), &[]).unwrap();
    assert_eq!(read(&file).unwrap(), (edited(3), saved));
}

/// An intact block of this file's past, truncated away and then left
/// behind a shorter append, doesn't follow on from the newest record: a
/// torn tail, never taken in, by reading, by a save's check or by a held
/// file.
#[test]
fn a_block_of_the_files_past_is_not_taken_in() {
    let mut file = Memory::default();
    let mut tails = vec![write_new(&mut file, &edited(1), &[]).unwrap().tail()];
    for n in 2..4 {
        let tail = *tails.last().unwrap();
        tails.push(save_to(&mut file, tail, &edited(n)).unwrap());
    }
    let past = file.bytes.clone();
    // Back to the first record, cut short, and saved again.
    file.truncate(tails[0].end).unwrap();
    let newest = save_to(&mut file, tails[0], &edited(4)).unwrap();
    for stale in [1, 2] {
        let stale = &past[tails[stale - 1].end as usize..tails[stale].end as usize];
        let mut bytes = file.bytes.clone();
        bytes.extend_from_slice(stale);
        assert_eq!(from_bytes(&bytes).unwrap(), (edited(4), newest));
        check(bytes.as_slice(), newest).unwrap();
        // Saving after it drops it.
        let mut copy = Memory { bytes, syncs: 0 };
        let saved = save_to(&mut copy, newest, &edited(5)).unwrap();
        assert_eq!(read(&copy).unwrap(), (edited(5), saved));
    }

    let based = |n| Based {
        base: None,
        document: edited(n),
    };
    let mut held = HeldFile::new(Memory::default());
    held.append(&based(1)).unwrap();
    let (_, first) = held_at(&held);
    held.append(&based(2)).unwrap();
    held.append(&based(3)).unwrap();
    let (_, third) = held_at(&held);
    let past = held.storage().bytes.clone();
    // Back to the first record, cut short, and appended to again.
    let mut held = HeldFile::<_, Based>::new(Memory {
        bytes: past[..first.end as usize].to_vec(),
        syncs: 0,
    });
    assert_eq!(held.read().unwrap(), Some(based(1)));
    held.append(&based(4)).unwrap();
    let mut bytes = held.into_storage().bytes;
    bytes.extend_from_slice(&past[first.end as usize..third.end as usize]);
    let mut held = HeldFile::<_, Based>::new(Memory { bytes, syncs: 0 });
    assert_eq!(held.read().unwrap(), Some(based(4)));
}

/// Blocks of kinds a reader doesn't know, like previews, are stepped over
/// as the chain goes on, and a save drops those after the last record;
/// another file type's records are such a kind.
#[test]
fn other_kinds_are_stepped_over() {
    let (bytes, tail) = to_bytes(&edited(1), &[]).unwrap();
    let id = id_of(&bytes);
    let (preview, _) = frame(id, tail.sum, tail.end, PREVIEW, b"image/png", b"png").unwrap();
    let file = [&bytes[..], &preview].concat();
    assert_eq!(from_bytes(&file).unwrap(), (edited(1), tail));
    check(file.as_slice(), tail).unwrap();
    let mut memory = Memory {
        bytes: file.clone(),
        syncs: 0,
    };
    let saved = save_to(&mut memory, tail, &edited(2)).unwrap();
    assert_eq!(saved.last, tail.end);
    assert_eq!(memory.bytes.len() as u64, saved.end);

    // Between records, of a kind of the other file type too.
    for kind in [PREVIEW, AUTOSAVE, [7; 16]] {
        let (between, after) = frame(id, tail.sum, tail.end, kind, b"", b"what").unwrap();
        let (record, newest) = next(&[&bytes[..], &between].concat(), after, &edited(2));
        let file = [&bytes[..], &between, &record].concat();
        assert_eq!(from_bytes(&file).unwrap(), (edited(2), newest));
        // A save from before it finds the record after it.
        assert!(matches!(check(file.as_slice(), tail), Err(Error::Conflict)));
        check(file.as_slice(), newest).unwrap();
    }
    // A record after a preview that doesn't follow on from it isn't taken.
    let (record, _) = next(&bytes, tail, &edited(2));
    let file = [&bytes[..], &preview, &record].concat();
    assert_eq!(from_bytes(&file).unwrap(), (edited(1), tail));
}

/// A payload other than a plain document, as the IO lane keeps in an
/// auto-save sidecar.
#[derive(Debug, PartialEq, Serialize)]
struct Based {
    base: Option<Tail>,
    document: Document,
}

/// [`Based`] before its check.
#[derive(Deserialize)]
struct UncheckedBased {
    base: Option<Tail>,
    document: Unchecked,
}

impl Payload for Based {
    type Unchecked = UncheckedBased;
    type Error = CheckError;

    fn check(unchecked: UncheckedBased) -> std::result::Result<Based, CheckError> {
        Ok(Based {
            base: unchecked.base,
            document: unchecked.document.check()?,
        })
    }
}

/// A held file keeps any payload in the same records, torn appends and all.
#[test]
fn a_held_file_holds_other_payloads() {
    let dir = TempDir::new("held-payload");
    let saved = create_at(&dir.0.join("design.vrdp"), &edited(0));
    let based = |n| Based {
        base: Some(saved),
        document: edited(n),
    };
    let mut file = HeldFile::<Memory, Based>::new(Memory::default());
    file.append(&based(1)).unwrap();
    file.append(&Based {
        base: None,
        document: edited(2),
    })
    .unwrap();
    file.append(&based(3)).unwrap();
    assert_eq!(file.read().unwrap(), Some(based(3)));

    let record = next_held(&file, &based(4));
    let mut torn = file.into_storage().bytes;
    torn.extend_from_slice(&record[..record.len() - 1]);
    let mut file = HeldFile::<Memory, Based>::new(Memory {
        bytes: torn,
        syncs: 0,
    });
    assert_eq!(file.read().unwrap(), Some(based(3)));
    // Not a plain document.
    let mut plain = HeldFile::<Memory>::new(file.into_storage());
    assert!(matches!(plain.read(), Err(Error::Decode { .. })));
}

/// A whole file made in memory, as the web build writes files of the
/// user's, is a `.vrdp` like any other: one record, which opening reads,
/// with the tail opening finds, and saving appends to.
#[test]
fn a_whole_file_in_memory_is_a_file_with_one_record() {
    let dir = TempDir::new("to-bytes");
    let (bytes, tail) = to_bytes(&edited(1), &[]).unwrap();
    std::fs::write(dir.file(), &bytes).unwrap();
    assert_eq!(open_at(&dir.file()).unwrap(), (edited(1), tail));
    let saved = save_at(&dir.file(), tail, &edited(2)).unwrap();

    assert_eq!(from_bytes(&bytes).unwrap(), (edited(1), tail));
    let (document, appended) = from_bytes(&std::fs::read(dir.file()).unwrap()).unwrap();
    assert_eq!(document, edited(2));
    assert_eq!(appended, saved);
}

/// Reading a whole file from bytes checks it like opening one does, and
/// never panics on what it's given.
#[test]
fn reading_bytes_refuses_what_isnt_a_design() {
    let (bytes, _) = to_bytes(&edited(1), &[]).unwrap();
    assert!(matches!(from_bytes(b""), Err(Error::NotVarde)));
    assert!(matches!(from_bytes(b"not a design"), Err(Error::NotVarde)));
    assert!(matches!(
        from_bytes(&bytes[..FILE_HEADER_LEN]),
        Err(Error::Empty)
    ));
    // Cut short, it's torn, which leaves nothing.
    assert!(matches!(
        from_bytes(&bytes[..bytes.len() - 1]),
        Err(Error::Empty)
    ));
    let mut version = bytes.clone();
    version[25] = 2;
    assert!(matches!(
        from_bytes(&version),
        Err(Error::UnsupportedVersion(2))
    ));
    let mut flipped = bytes.clone();
    let payload = FILE_HEADER_LEN + TAG_LEN_AT + 2;
    flipped[payload] ^= 1;
    // No intact record.
    assert!(matches!(
        from_bytes(&flipped),
        Err(Error::Corrupt { offset }) if offset == FILE_HEADER_LEN as u64
    ));
    // Every prefix, and a flipped bit at each place, is an error or a
    // design, never a panic.
    for len in 0..bytes.len() {
        let _ = from_bytes(&bytes[..len]);
    }
    for at in 0..bytes.len() {
        let mut flipped = bytes.clone();
        flipped[at] ^= 0x80;
        let _ = from_bytes(&flipped);
    }
}

/// Before replacing a file, the web build checks it still holds what it
/// last read or wrote: the same file, or one with a torn tail after it,
/// is unchanged; a damaged record is damage; anything else is someone
/// else's change.
#[test]
fn a_file_is_unchanged_only_while_it_ends_with_the_same_record() {
    let (bytes, known) = whole_file(&edited(1), &[]).unwrap();
    let tail = known.tail();
    let check = |bytes: &[u8]| check_unchanged(bytes, &known);
    check(&bytes).unwrap();

    let (record, _) = next(&bytes, tail, &edited(2));
    let mut torn = bytes.clone();
    torn.extend_from_slice(&record[..record.len() - 1]);
    check(&torn).unwrap();
    // A copy of its own record doesn't follow on from it.
    let mut again = bytes.clone();
    again.extend_from_slice(&bytes[FILE_HEADER_LEN..]);
    check(&again).unwrap();

    let conflict = |bytes: &[u8]| matches!(check(bytes), Err(Error::Conflict));
    let damaged = |bytes: &[u8]| matches!(check(bytes), Err(Error::Damaged));
    let mut appended = bytes.clone();
    appended.extend_from_slice(&record);
    assert!(conflict(&appended));
    // Another program saved a document of its own, or the same one again.
    assert!(conflict(&to_bytes(&edited(2), &[]).unwrap().0));
    assert!(conflict(&to_bytes(&edited(1), &[]).unwrap().0));
    // Emptied, or replaced by something else.
    assert!(conflict(b""));
    assert!(conflict(b"not a design"));
    // Its `id` changed: another file, as far as can be told.
    let mut other = bytes.clone();
    other[FILE_HEADER_LEN - 1] ^= 1;
    assert!(conflict(&other));
    // Damaged, or cut short.
    let mut flipped = bytes.clone();
    *flipped.last_mut().unwrap() ^= 1;
    assert!(damaged(&flipped));
    assert!(damaged(&bytes[..bytes.len() - 1]));
    assert!(damaged(&bytes[..FILE_HEADER_LEN]));
}

/// A save natively and the web's check before replacing a file are one
/// check: the same file gets the same answer from both.
#[test]
fn a_save_and_the_web_check_agree() {
    let dir = TempDir::new("one-check");
    let first = create_at(&dir.file(), &edited(0));
    let tail = save_at(&dir.file(), first, &edited(1)).unwrap();
    let saved = std::fs::read(dir.file()).unwrap();
    let known = from_bytes_with_report(&saved).unwrap().known();

    let (record, _) = next(&saved, tail, &edited(2));
    let mut damaged = record.clone();
    *damaged.last_mut().unwrap() ^= 1;
    let mut last_damaged = saved.clone();
    *last_damaged.last_mut().unwrap() ^= 1;
    let mut first_damaged = saved.clone();
    first_damaged[FILE_HEADER_LEN + TAG_LEN_AT + 2] ^= 0xff;
    // The block before the last record no longer the one it was appended
    // to, by its stored sum.
    let mut unlinked = saved.clone();
    unlinked[FILE_HEADER_LEN + SUM_AT] ^= 1;
    let ok = "Ok(())";
    let conflict = "Err(Conflict)";
    let damage = "Err(Damaged)";
    let states = [
        (saved.clone(), ok),
        // A torn tail, or damage before the last record, isn't read.
        ([&saved[..], &record[..record.len() - 1]].concat(), ok),
        (first_damaged, ok),
        (unlinked, damage),
        ([&saved[..], &record].concat(), conflict),
        ([&saved[..], &damaged, &record].concat(), damage),
        (last_damaged, damage),
        (to_bytes(&edited(2), &[]).unwrap().0, conflict),
        (saved[..saved.len() - 1].to_vec(), damage),
        (Vec::new(), conflict),
    ];
    for (state, expected) in states {
        std::fs::write(dir.file(), &state).unwrap();
        let web = format!(
            "{:?}",
            check_unchanged(state.as_slice(), &known).map(|_| ())
        );
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.file())
            .unwrap();
        let native = save(&mut file, &mut known.clone(), &edited(3), &[]).map(|_| ());
        assert_eq!(web, expected);
        assert_eq!(format!("{native:?}"), expected);
    }
}

/// [`Based`] as a later build might extend it, with a defaulted field.
#[derive(Debug, PartialEq, Serialize)]
struct Noted {
    base: Option<Tail>,
    document: Document,
    note: String,
}

/// [`Noted`] before its check.
#[derive(Deserialize)]
struct UncheckedNoted {
    base: Option<Tail>,
    document: Unchecked,
    #[serde(default)]
    note: String,
}

impl Payload for Noted {
    type Unchecked = UncheckedNoted;
    type Error = CheckError;

    fn check(unchecked: UncheckedNoted) -> std::result::Result<Noted, CheckError> {
        Ok(Noted {
            base: unchecked.base,
            document: unchecked.document.check()?,
            note: unchecked.note,
        })
    }
}

/// A field added with a default reads from records written without it as
/// its default, and a build without it skips it.
#[test]
fn a_new_field_reads_from_old_records_and_old_builds_skip_it() {
    let mut file = HeldFile::<Memory, Based>::new(Memory::default());
    file.append(&Based {
        base: None,
        document: edited(1),
    })
    .unwrap();
    let mut newer = HeldFile::<Memory, Noted>::new(file.into_storage());
    let noted = Noted {
        base: None,
        document: edited(1),
        note: String::new(),
    };
    assert_eq!(newer.read().unwrap(), Some(noted));

    let noted = Noted {
        base: None,
        document: edited(2),
        note: "kept by a later build".to_owned(),
    };
    newer.append(&noted).unwrap();
    let mut older = HeldFile::<Memory, Based>::new(newer.into_storage());
    assert_eq!(
        older.read().unwrap(),
        Some(Based {
            base: None,
            document: edited(2),
        })
    );
}

/// A record's MessagePack holding `document`.
fn record_msgpack(document: &Document) -> Vec<u8> {
    rmp_serde::to_vec_named(&Record {
        time: UnixSeconds(1_790_000_000),
        payload: document,
    })
    .unwrap()
}

/// A record followed by more bytes isn't taken for the record alone.
#[test]
fn bytes_after_a_payload_are_refused() {
    let mut raw = record_msgpack(&edited(1));
    assert_eq!(
        from_msgpack::<Document>(&raw).map(|(document, _)| document),
        Ok(edited(1))
    );
    raw.push(0xc0);
    assert!(matches!(
        from_msgpack::<Document>(&raw),
        Err(error) if error.to_string().contains("1 bytes after the end")
    ));
}

/// Values nested past anything a payload holds, in a field skipped as
/// unknown, are refused rather than recursed into until the stack runs
/// out.
#[test]
fn deeply_nested_values_are_refused() {
    let mut raw = record_msgpack(&edited(1));
    // A map of the record's two fields, given a third.
    assert_eq!(raw[0], 0x82);
    raw[0] = 0x83;
    raw.extend_from_slice(&[0xa1, b'x']);
    // Arrays of one element, each holding the next, the last nil.
    raw.extend(std::iter::repeat_n(0x91, 1000));
    raw.push(0xc0);
    let decoded = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || from_msgpack::<Document>(&raw).map_err(|e| e.to_string()))
        .unwrap()
        .join()
        .unwrap();
    assert!(
        matches!(&decoded, Err(error) if error.contains("depth")),
        "{decoded:?}"
    );
}

/// A design file of `n` saves, of `edited(1)` to `edited(n)`, and the tail
/// after each.
fn saved(n: usize) -> (Vec<u8>, Vec<Tail>) {
    let mut file = Memory::default();
    let mut tails = vec![write_new(&mut file, &edited(1), &[]).unwrap().tail()];
    for i in 2..=n {
        let tail = *tails.last().unwrap();
        tails.push(save_to(&mut file, tail, &edited(i)).unwrap());
    }
    (file.bytes, tails)
}

/// Where the record at the end of `tail` is in its file.
fn span(tail: Tail) -> std::ops::Range<usize> {
    tail.last as usize..tail.end as usize
}

/// The design file `bytes` opened: the document, its tail, and the report,
/// whose time must be of a record written just now.
fn opened(bytes: &[u8]) -> (Document, Tail, Report) {
    let opened = from_bytes_with_report(bytes).unwrap();
    let report = opened.report;
    let age = UnixSeconds::now().checked_since(report.time);
    assert!(
        age.is_some_and(|age| (0..3600).contains(&age)),
        "{report:?}"
    );
    (opened.payload, opened.tail, report)
}

/// `n` bytes of noise, the same for the same `seed`.
fn noise(n: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// The file `bytes` with the byte at `at` flipped.
fn flip(bytes: &[u8], at: usize) -> Vec<u8> {
    let mut bytes = bytes.to_vec();
    bytes[at] ^= 0xff;
    bytes
}

/// Nothing after the chain's end framed like a record is a torn tail:
/// the newest record the chain reaches opens, and nothing counts as
/// unreadable.
#[test]
fn a_torn_tail_opens_the_newest_record() {
    let (bytes, tails) = saved(3);
    let (document, tail, report) = opened(&bytes);
    assert_eq!((document, tail), (edited(3), tails[2]));
    assert_eq!((report.outcome, report.unreadable), (Outcome::Intact, 0));

    let (record, _) = next(&bytes, tails[2], &edited(4));
    let old = &bytes[span(tails[1])];
    // Zeros, noise, an interrupted save, and a crash's mix of them with
    // part of an old record.
    for torn in [
        vec![0; 1000],
        noise(1000, 7),
        record[..record.len() - 3].to_vec(),
        record[..record.len() / 2].to_vec(),
        [&[0; 100][..], &old[10..old.len() - 1], &noise(50, 3)].concat(),
    ] {
        let (document, tail, report) = opened(&[&bytes[..], &torn].concat());
        assert_eq!((document, tail), (edited(3), tails[2]));
        assert_eq!((report.outcome, report.unreadable), (Outcome::TornTail, 0));
    }

    // A leftover block of the file's past, which a record intact but not
    // following on is, right after an intact block.
    let file = [&bytes[..tails[0].end as usize], &bytes[span(tails[2])]].concat();
    let (document, tail, report) = opened(&file);
    assert_eq!((document, tail), (edited(1), tails[0]));
    assert_eq!((report.outcome, report.unreadable), (Outcome::TornTail, 0));
}

/// Damage before the newest record whose header is intact is stepped over
/// by it, to the newest record: an older record's payload, the torn end of
/// the record before an append, the first record, and several of them.
/// Two damaged blocks in a row can't be: the second header leads nowhere
/// intact.
#[test]
fn damage_before_the_newest_is_stepped_over() {
    let (bytes, tails) = saved(4);
    let size = |i: usize| tails[i].end - tails[i].last;
    let payload = |i: usize| tails[i].last as usize + TAG_LEN_AT + 10;
    for (damaged, unreadable) in [
        (flip(&bytes, payload(1)), size(1)),
        (flip(&bytes, tails[1].end as usize - 1), size(1)),
        (flip(&bytes, payload(0)), size(0)),
        (
            flip(&flip(&bytes, payload(0)), payload(2)),
            size(0) + size(2),
        ),
    ] {
        let (document, tail, report) = opened(&damaged);
        assert_eq!((document, tail), (edited(4), tails[3]));
        assert_eq!(
            (report.outcome, report.unreadable),
            (Outcome::Bridged, unreadable)
        );
        // With a torn tail after it, it's still stepped over.
        let torn = [&damaged[..], &[0; 100]].concat();
        assert_eq!(opened(&torn).2.outcome, Outcome::Bridged);
    }
    let damaged = flip(&flip(&bytes, payload(0)), payload(1));
    assert!(matches!(
        opened(&damaged).2.outcome,
        Outcome::Damaged { .. }
    ));

    // Through a block of a kind the reader doesn't know.
    let id = id_of(&bytes);
    let at = tails[1].end;
    let (unknown, after) = frame(id, tails[1].sum, at, [7; 16], b"", b"what").unwrap();
    let (record, newest) =
        record_at(id, after.sum, after.end, FileType::Design, &edited(9)).unwrap();
    let file = [&bytes[..at as usize], &unknown, &record].concat();
    let (document, tail, report) = opened(&flip(&file, payload(1)));
    assert_eq!((document, tail), (edited(9), newest));
    assert_eq!(report.outcome, Outcome::Bridged);
    // A held file the same.
    let mut file = HeldFile::<_, Document>::new(Memory::default());
    file.append(&edited(1)).unwrap();
    let second = held_at(&file).1.end as usize;
    file.append(&edited(2)).unwrap();
    file.append(&edited(3)).unwrap();
    let bytes = file.into_storage().bytes;
    let mut file = HeldFile::<_, Document>::new(Memory {
        bytes: flip(&bytes, second + TAG_LEN_AT + 10),
        syncs: 0,
    });
    let opened = file.read_with_report().unwrap().unwrap();
    assert_eq!(opened.payload, edited(3));
    assert_eq!(opened.report.outcome, Outcome::Bridged);
}

/// The newest record damaged, its header intact, opens the one before,
/// and says where the damaged one is and the `sum` its header holds; a
/// torn tail may follow it. With no record before it, nothing opens.
#[test]
fn a_damaged_newest_save_opens_the_one_before() {
    let (bytes, tails) = saved(3);
    let damaged = DamagedRecord {
        start: tails[2].last,
        end: tails[2].end,
        sum: tails[2].sum,
    };
    let payload = tails[2].last as usize + TAG_LEN_AT + 10;
    for file in [
        flip(&bytes, payload),
        [&flip(&bytes, payload)[..], &noise(100, 5)].concat(),
    ] {
        let (document, tail, report) = opened(&file);
        assert_eq!((document, tail), (edited(2), tails[1]));
        assert_eq!(report.outcome, Outcome::NewestDamaged(damaged));
        assert_eq!(report.unreadable, damaged.end - damaged.start);
    }
    // Its kind damaged, it's not framed like a record: a torn tail.
    let (_, tail, report) = opened(&flip(&bytes, bytes.len() - 1));
    assert_eq!((tail, report.outcome), (tails[1], Outcome::TornTail));

    let (one, _) = saved(1);
    assert!(matches!(
        from_bytes(&flip(&one, FILE_HEADER_LEN + TAG_LEN_AT + 10)),
        Err(Error::Corrupt { offset }) if offset == FILE_HEADER_LEN as u64
    ));
}

/// Damage whose header is damaged too is searched past: the newest record
/// the chain proves opens, and the search's newest is offered, with its
/// time; blocks of kinds the reader doesn't know link the found ones. With
/// no record before the damage, the search's newest opens. Framed damage
/// past which nothing is found is damage too.
#[test]
fn damage_with_its_header_damaged_is_found_by_search() {
    let (bytes, tails) = saved(4);
    let size = |i: usize| tails[i].end - tails[i].last;
    let found = |file: &[u8]| match opened(file).2.outcome {
        Outcome::Damaged { found } => found,
        outcome => panic!("{outcome:?}"),
    };
    // Its `prev`, and its first `len`.
    for at in [PREV_AT, 6] {
        let file = flip(&bytes, tails[1].last as usize + at);
        let (document, tail, report) = opened(&file);
        assert_eq!((document, tail), (edited(1), tails[0]));
        assert_eq!(report.unreadable, size(1));
        let found = found(&file).unwrap();
        assert_eq!(found.tail, tails[3]);
        assert!(found.time.is_some_and(|time| time <= UnixSeconds::now()));
    }

    // Through a block of a kind the reader doesn't know.
    let id = id_of(&bytes);
    let at = tails[1].end;
    let (unknown, after) = frame(id, tails[1].sum, at, [7; 16], b"", b"what").unwrap();
    let (record, newest) =
        record_at(id, after.sum, after.end, FileType::Design, &edited(9)).unwrap();
    let file = [&bytes[..at as usize], &unknown, &record].concat();
    let file = flip(&file, tails[1].last as usize + PREV_AT);
    assert_eq!(found(&file).unwrap().tail, newest);

    // The first record: the search's newest opens.
    let file = flip(&bytes, FILE_HEADER_LEN + PREV_AT);
    let (document, tail, report) = opened(&file);
    assert_eq!((document, tail), (edited(4), tails[3]));
    assert_eq!(report.outcome, Outcome::Damaged { found: None });

    // The newest record: nothing found past it.
    let file = flip(&bytes, tails[3].last as usize + PREV_AT);
    let (document, tail, report) = opened(&file);
    assert_eq!((document, tail), (edited(3), tails[2]));
    assert_eq!(report.outcome, Outcome::Damaged { found: None });
    assert_eq!(report.unreadable, size(3));
}

/// Read with the search's newest, the file gives it too, opened, if it's
/// another record than the one opened and decodes: at the tail the report
/// names, reported as damaged with nothing else found, and refusing saves.
#[test]
fn the_searchs_newest_is_opened_alongside() {
    let (bytes, tails) = saved(4);
    let file = flip(&bytes, tails[1].last as usize + PREV_AT);
    let read = from_bytes_with_found(&file).unwrap();
    assert_eq!(read.opened.payload, edited(1));
    let Outcome::Damaged { found: Some(named) } = read.opened.report.outcome else {
        panic!("{:?}", read.opened.report);
    };
    let found = read.found.unwrap();
    assert_eq!((&found.payload, found.tail), (&edited(4), named.tail));
    assert_eq!(found.report.outcome, Outcome::Damaged { found: None });
    assert_eq!(Some(found.report.time), named.time);
    assert_eq!(found.report.unreadable, read.opened.report.unreadable);
    let mut memory = Memory {
        bytes: file.clone(),
        syncs: 0,
    };
    assert!(matches!(
        save(&mut memory, &mut found.known(), &edited(5), &[]),
        Err(Error::OpenedDamaged)
    ));
    assert_eq!(memory.bytes, file);
    // Read from a file, the same.
    let read = read_with_found(&memory).unwrap();
    assert_eq!(read.found.unwrap().tail, named.tail);

    // Intact, or the search's newest opened itself: nothing alongside.
    assert!(from_bytes_with_found(&bytes).unwrap().found.is_none());
    let file = flip(&bytes, FILE_HEADER_LEN + PREV_AT);
    let read = from_bytes_with_found(&file).unwrap();
    assert_eq!(read.opened.payload, edited(4));
    assert!(read.found.is_none());

    // A found record that won't decode isn't opened, though named.
    let id = id_of(&bytes);
    let at = tails[1].end;
    let (garbage, _) = frame(id, tails[1].sum, at, RECORD, b"", b"not a record").unwrap();
    let file = [&bytes[..at as usize], &garbage].concat();
    let file = flip(&file, tails[1].last as usize + PREV_AT);
    let read = from_bytes_with_found(&file).unwrap();
    assert!(matches!(
        read.opened.report.outcome,
        Outcome::Damaged {
            found: Some(FoundRecord { time: None, .. })
        }
    ));
    assert!(read.found.is_none());
}

/// A block of this file's past that a search finds, following on from a
/// block of the chain before the damage other than its last, is rejected,
/// and so is what follows on from it.
#[test]
fn a_block_of_the_files_past_found_by_search_is_rejected() {
    let (bytes, tails) = saved(3);
    let past = &bytes[tails[0].end as usize..tails[2].end as usize];
    let mut file = Memory {
        bytes: bytes[..tails[0].end as usize].to_vec(),
        syncs: 0,
    };
    let newest = save_to(&mut file, tails[0], &edited(4)).unwrap();
    let (record, _) = next(&file.bytes, newest, &edited(5));
    let mut damaged = record.clone();
    damaged[PREV_AT] ^= 0xff;
    let bytes = [&file.bytes[..], &damaged, past].concat();
    let (document, tail, report) = opened(&bytes);
    assert_eq!((document, tail), (edited(4), newest));
    assert_eq!(report.outcome, Outcome::Damaged { found: None });
    assert_eq!(report.unreadable, (damaged.len() + past.len()) as u64);

    // What follows on from the chain's last block isn't its past.
    let bytes = [&file.bytes[..], &damaged, &record].concat();
    assert!(matches!(
        opened(&bytes).2.outcome,
        Outcome::Damaged { found: Some(found) } if found.tail.last == bytes.len() as u64 - record.len() as u64
    ));
}

/// Blocks of the file's past that follow on from a block a shorter write
/// overwrote, brought back by stale bytes where the file grew, are
/// rejected too, whole or through a damaged block before them: a held
/// file cut back and appended to, and a design file the same. Nothing
/// is offered as found.
#[test]
fn a_block_following_on_from_an_overwritten_one_is_rejected() {
    let mut file = HeldFile::<_, Document>::new(Memory::default());
    let mut second = None;
    for i in 1..=5 {
        file.append(&edited(i)).unwrap();
        if i == 2 {
            second = Some(held_at(&file).1);
        }
    }
    let old = file.storage().bytes.clone();
    // Cut back to the second record, and appended to again.
    let mut file = HeldFile::<_, Document>::new(Memory {
        bytes: old[..second.unwrap().end as usize].to_vec(),
        syncs: 0,
    });
    assert_eq!(file.read().unwrap(), Some(edited(2)));
    file.append(&edited(0)).unwrap();
    let new = file.into_storage().bytes;
    let stale = [&new[..], &old[new.len()..]].concat();
    let mut file = HeldFile::<_, Document>::new(Memory {
        bytes: stale,
        syncs: 0,
    });
    let read = file.read_with_report().unwrap().unwrap();
    assert_eq!(read.payload, edited(0));
    assert_eq!(read.report.outcome, Outcome::Damaged { found: None });

    let (bytes, tails) = saved(5);
    let (shorter, newest) = next(&bytes, tails[1], &edited(0));
    let end = newest.end as usize;
    assert!(end < tails[2].end as usize);
    let file = [&bytes[..tails[1].end as usize], &shorter, &bytes[end..]].concat();
    let (document, tail, report) = opened(&file);
    assert_eq!((document, tail), (edited(0), newest));
    assert_eq!(report.outcome, Outcome::Damaged { found: None });
    assert_eq!(report.unreadable, (file.len() - end) as u64);
    // The fourth record damaged: the fifth follows on from it.
    let file = flip(&file, tails[3].last as usize + TAG_LEN_AT + 10);
    let (_, tail, report) = opened(&file);
    assert_eq!(tail, newest);
    assert_eq!(report.outcome, Outcome::Damaged { found: None });
}

/// Hashing in a search is bounded by the file's length: a file crafted
/// with candidates overlapping at every eighth byte, each half the file
/// long, which would take hashing tens of gigabytes to search through,
/// stops the search early, and the record after them isn't found.
#[test]
fn a_search_stops_at_its_budget() {
    let (bytes, tails) = saved(1);
    let len = 1u64 << 19;
    let crafted: Vec<u8> = std::iter::repeat_n(len.to_le_bytes(), 1 << 17)
        .flatten()
        .collect();
    let (record, _) = next(&bytes, tails[0], &edited(2));
    let file = [&bytes[..], &crafted, &record].concat();
    let started = std::time::Instant::now();
    let (document, tail, report) = opened(&file);
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    assert_eq!((document, tail), (edited(1), tails[0]));
    assert_eq!(report.outcome, Outcome::Damaged { found: None });
    assert_eq!(report.unreadable, (crafted.len() + record.len()) as u64);
}

mod power_loss;
mod previews;
mod saving;

/// A record whose move or mirror was changed on disk to what the
/// document refuses (an offset or angle not what its text gives, past
/// its bounds or not a number, a mirror face's point out of bounds) is
/// refused as it's read, never a panic; as written, it reads.
#[test]
fn a_tampered_move_or_mirror_is_refused() {
    use glam::DVec3;
    use varde_document::{Axis3, AxisRef, FaceKey, FaceRef, Mirror, Move, PartKey, PlaneRef};
    use varde_expr::Value;
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let design = editor.document().design();
    let length = |text: &str| Value::new(text, &Move::offset_ask(&design)).unwrap();
    let moved = Move {
        bodies: vec![plate],
        offset: [length("12.5"), length("0"), length("0")],
        turn: Some((
            AxisRef::Origin(Axis3::Z),
            Value::new("30", &Move::angle_ask(&design)).unwrap(),
        )),
    };
    let mirror = Mirror {
        bodies: vec![plate],
        plane: PlaneRef::Face(FaceRef {
            body: plate,
            key: FaceKey {
                feature: editor.document().features()[1].id.get(),
                part: PartKey::EndCap,
                instance: 0,
            },
            near: DVec3::new(0.0, 17.25, 10.0),
        }),
        keep_original: true,
    };
    editor
        .apply(editor.document().add_feature(moved.into()))
        .unwrap();
    editor
        .apply(editor.document().add_feature(mirror.into()))
        .unwrap();
    let raw = record_msgpack(editor.document());
    assert!(from_msgpack::<Document>(&raw).is_ok());
    // Each number as MessagePack writes a float: 0xcb and its bits.
    let float = |x: f64| {
        let mut bytes = vec![0xcb];
        bytes.extend_from_slice(&x.to_bits().to_be_bytes());
        bytes
    };
    let swap = |was: f64, now: f64| {
        let (was, now) = (float(was), float(now));
        let at = (raw.windows(was.len()))
            .position(|window| window == was)
            .unwrap_or_else(|| panic!("{was:?} isn't in the record"));
        let mut changed = raw.clone();
        changed[at..at + now.len()].copy_from_slice(&now);
        changed
    };
    let thirty = 30.0 * (std::f64::consts::PI / 180.0);
    for (was, now) in [
        (12.5, f64::NAN),
        (12.5, 12.25),
        (12.5, 2e6),
        (12.5, f64::INFINITY),
        (thirty, 7.0),
        (thirty, -f64::INFINITY),
        (17.25, f64::NAN),
        (17.25, 1e300),
    ] {
        let decoded = from_msgpack::<Document>(&swap(was, now));
        assert!(decoded.is_err(), "{was} as {now} was taken");
    }
}

/// A record whose pattern was changed on disk to what the document
/// refuses (a count that isn't whole, in range or what its text gives, a
/// spacing of nothing or past the limit, an angle past a turn or none, a
/// kind it doesn't know) is refused as it's read, never a panic; as
/// written, it reads.
#[test]
fn a_tampered_pattern_is_refused() {
    use varde_document::{Axis3, AxisRef, Pattern, PatternKind};
    use varde_expr::Value;
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let design = editor.document().design();
    let count = |text: &str| Value::new(text, &Pattern::count_ask(&design)).unwrap();
    for kind in [
        PatternKind::Linear {
            along: AxisRef::Origin(Axis3::Y),
            count: count("7"),
            spacing: Value::new("-12.375", &Pattern::spacing_ask(&design)).unwrap(),
        },
        PatternKind::Circular {
            about: AxisRef::Origin(Axis3::Z),
            count: count("9"),
            angle: Value::new("123.5", &Pattern::angle_ask(&design)).unwrap(),
        },
    ] {
        let pattern = Pattern {
            bodies: vec![plate],
            kind,
            copies: Default::default(),
        };
        editor
            .apply(editor.document().add_feature(pattern.into()))
            .unwrap();
    }
    let raw = record_msgpack(editor.document());
    let (read, _) = from_msgpack::<Document>(&raw).unwrap();
    assert_eq!(&read, editor.document());
    let float = |x: f64| {
        let mut bytes = vec![0xcb];
        bytes.extend_from_slice(&x.to_bits().to_be_bytes());
        bytes
    };
    let swap = |was: &[u8], now: &[u8]| {
        let at = (raw.windows(was.len()))
            .position(|window| window == was)
            .unwrap_or_else(|| panic!("{was:?} isn't in the record"));
        let mut changed = raw[..at].to_vec();
        changed.extend_from_slice(now);
        changed.extend_from_slice(&raw[at + was.len()..]);
        changed
    };
    let angle = 123.5 * (std::f64::consts::PI / 180.0);
    for (was, now) in [
        (7.0, 7.5),
        (7.0, 1.0),
        (7.0, 1e300),
        (7.0, f64::NAN),
        (9.0, 2048.0),
        (9.0, -9.0),
        (-12.375, 0.0),
        (-12.375, -0.0),
        (-12.375, 2e6),
        (-12.375, f64::NEG_INFINITY),
        (angle, 7.0),
        (angle, 0.0),
        (angle, -angle),
        (angle, f64::NAN),
    ] {
        let decoded = from_msgpack::<Document>(&swap(&float(was), &float(now)));
        assert!(decoded.is_err(), "{was} as {now} was taken");
    }
    // A kind it doesn't know, named in place of one it does.
    let named = |name: &str| {
        let mut bytes = vec![0xa0 | name.len() as u8];
        bytes.extend_from_slice(name.as_bytes());
        bytes
    };
    for (was, now) in [("Linear", "Spiral"), ("Circular", "Circulaz")] {
        let decoded = from_msgpack::<Document>(&swap(&named(was), &named(now)));
        assert!(decoded.is_err(), "{was} as {now} was taken");
    }
}

/// A pattern written before "Join to original" (no `copies` field) reads
/// as joined, its copies in their bodies; one whose copies are bodies of
/// their own goes through a file, and one whose list of them was changed
/// on disk, or whose kind of copies isn't one it knows, is refused.
#[test]
fn join_to_original_reads_from_older_files_and_is_checked() {
    use varde_document::{Axis3, AxisRef, Copies, FeatureKind, Pattern, PatternKind};
    use varde_expr::Value;
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let design = editor.document().design();
    let row = |copies: Copies| Pattern {
        bodies: vec![plate],
        kind: PatternKind::Linear {
            along: AxisRef::Origin(Axis3::Y),
            count: Value::new("3", &Pattern::count_ask(&design)).unwrap(),
            spacing: Value::new("40", &Pattern::spacing_ask(&design)).unwrap(),
        },
        copies,
    };
    editor
        .apply(editor.document().add_feature(row(Copies::Joined).into()))
        .unwrap();
    let joined = editor.document().clone();
    // The field taken out, as an older build wrote it: the pattern's map
    // of three fields made two.
    let raw = record_msgpack(&joined);
    let mut header = vec![0x83, 0xa6];
    header.extend_from_slice(b"bodies");
    let at = (raw.windows(header.len()))
        .position(|window| window == header)
        .expect("the pattern's map");
    let mut field = vec![0xa6];
    field.extend_from_slice(b"copies");
    field.push(0xa6);
    field.extend_from_slice(b"Joined");
    let end = (raw.windows(field.len()))
        .position(|window| window == field)
        .expect("its copies");
    let mut older = raw[..end].to_vec();
    older.extend_from_slice(&raw[end + field.len()..]);
    older[at] = 0x82;
    let (read, _) = from_msgpack::<Document>(&older).unwrap();
    assert_eq!(read, joined);
    let FeatureKind::Pattern(pattern) = &read.features()[2].kind else {
        panic!("the pattern");
    };
    assert!(pattern.joins());

    // Copies of their own, through a file.
    let id = joined.features()[2].id;
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(row(Copies::Separate(Vec::new())).into()),
        })
        .unwrap();
    let separate = editor.document();
    assert_eq!(separate.bodies().len(), 3);
    let (bytes, _) = to_bytes(separate, &[]).unwrap();
    let (read, _) = from_bytes(&bytes).unwrap();
    assert_eq!(&read, separate);
    let raw = record_msgpack(separate);
    let mut list = vec![0xa8];
    list.extend_from_slice(b"Separate");
    let at = (raw.windows(list.len()))
        .position(|window| window == list)
        .expect("the copies' variant")
        + list.len();
    // Its two bodies, small ids, one each.
    assert_eq!(raw[at], 0x92);
    let mut short = raw[..at].to_vec();
    short.push(0x91);
    short.push(raw[at + 1]);
    short.extend_from_slice(&raw[at + 3..]);
    assert!(
        from_msgpack::<Document>(&short).is_err(),
        "one copy body short"
    );
    let mut repeated = raw.clone();
    repeated[at + 2] = repeated[at + 1];
    assert!(
        from_msgpack::<Document>(&repeated).is_err(),
        "a copy body twice"
    );
    let mut unknown = raw.clone();
    unknown[at - 1] = b'z';
    assert!(from_msgpack::<Document>(&unknown).is_err(), "Separatz");
    assert!(from_msgpack::<Document>(&raw).is_ok());
}

/// A list of copy bodies changed on disk is refused, never taken as room
/// to make: a length of 2³² − 1, ids past any made (the largest there
/// is among them), and a pattern's count raised so its bodies times its
/// copies would overflow were they multiplied unchecked.
#[test]
fn a_tampered_list_of_copy_bodies_is_refused() {
    use varde_document::{Axis3, AxisRef, Copies, Pattern, PatternKind};
    use varde_expr::Value;
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let design = editor.document().design();
    let row = Pattern {
        bodies: vec![plate],
        kind: PatternKind::Linear {
            along: AxisRef::Origin(Axis3::Y),
            count: Value::new("3", &Pattern::count_ask(&design)).unwrap(),
            spacing: Value::new("40", &Pattern::spacing_ask(&design)).unwrap(),
        },
        copies: Copies::Separate(Vec::new()),
    };
    editor
        .apply(editor.document().add_feature(row.into()))
        .unwrap();
    let raw = record_msgpack(editor.document());
    assert!(from_msgpack::<Document>(&raw).is_ok());
    let mut list = vec![0xa8];
    list.extend_from_slice(b"Separate");
    let at = (raw.windows(list.len()))
        .position(|window| window == list)
        .expect("the copies' variant")
        + list.len();
    assert_eq!(raw[at], 0x92);
    let (first, second) = (raw[at + 1], raw[at + 2]);
    assert!(first < 0x80 && second < 0x80, "small ids");
    let with = |head: &[u8], items: &[u8]| {
        let mut bytes = raw[..at].to_vec();
        bytes.extend_from_slice(head);
        bytes.extend_from_slice(items);
        bytes.extend_from_slice(&raw[at + 3..]);
        bytes
    };
    // An array of 2³² − 1, holding the two.
    let long = with(&[0xdd, 0xff, 0xff, 0xff, 0xff], &[first, second]);
    assert!(from_msgpack::<Document>(&long).is_err());
    // As an array 32 of two, it reads.
    let same = with(&[0xdd, 0, 0, 0, 2], &[first, second]);
    assert!(from_msgpack::<Document>(&same).is_ok());
    // The largest id there is in place of the second.
    let mut largest = vec![first, 0xcf];
    largest.extend_from_slice(&u64::MAX.to_be_bytes());
    let past = with(&[0x92], &largest);
    assert!(from_msgpack::<Document>(&past).is_err());
    // The count raised as far as a number goes: refused, not overflowed.
    let mut three = vec![0xa5];
    three.extend_from_slice(b"value");
    three.extend_from_slice(&[0xcb]);
    three.extend_from_slice(&3.0f64.to_be_bytes());
    let count = (raw.windows(three.len()))
        .position(|window| window == three)
        .expect("the count's value")
        + 7;
    for value in [f64::MAX, 1.8e19, 4294967297.0, 1025.0] {
        let mut bytes = raw.clone();
        bytes[count..count + 8].copy_from_slice(&value.to_be_bytes());
        assert!(from_msgpack::<Document>(&bytes).is_err(), "{value}");
    }
}

/// The example plate and a second one with an align of the second's top
/// corner onto the first's, face to face with a secondary pair, an
/// offset of 12.5 and a turn of 30°: the document.
fn aligned_plates() -> Document {
    use glam::DVec3;
    use varde_document::{
        Align, AlignRefs, AxisRef, BodyId, DirRef, EdgeRef, FaceKey, FaceRef, FeatureKind, Move,
        Operation, PartKey, PointRef,
    };
    use varde_expr::Value;
    let mut editor = Editor::new(Document::example());
    let FeatureKind::Extrude(extrude) = editor.document().features()[1].kind.clone() else {
        panic!("the example's extrude");
    };
    let again = varde_document::Extrude {
        operation: Operation::NewBody(BodyId::NEW),
        ..extrude
    };
    editor
        .apply(editor.document().add_feature(again.into()))
        .unwrap();
    let document = editor.document();
    let [a, b] = [0, 1].map(|k| document.bodies()[k].id);
    let [first, second] = [1, 2].map(|k| document.features()[k].id.get());
    let design = document.design();
    let key = |feature, part| FaceKey {
        feature,
        part,
        instance: 0,
    };
    let corner = |body, feature| {
        let mut faces = [
            key(feature, PartKey::EndCap),
            key(feature, PartKey::Side { curve: 0 }),
            key(feature, PartKey::Side { curve: 1 }),
        ];
        faces.sort();
        PointRef::Corner {
            body,
            faces,
            near: DVec3::new(30.0, 17.25, 10.0),
        }
    };
    let top = |body, feature| {
        DirRef::Normal(FaceRef {
            body,
            key: key(feature, PartKey::EndCap),
            near: DVec3::new(0.0, 15.0, 10.0),
        })
    };
    let mut faces = [
        key(first, PartKey::EndCap),
        key(first, PartKey::Side { curve: 0 }),
    ];
    faces.sort();
    let edge = EdgeRef {
        body: a,
        faces,
        near: DVec3::new(0.0, 20.0, 10.0),
    };
    let align = Align {
        body: b,
        from: AlignRefs {
            point: corner(b, second),
            primary: Some(top(b, second)),
            secondary: Some(DirRef::Axis(AxisRef::Face(FaceRef {
                body: b,
                key: key(second, PartKey::Side { curve: 0 }),
                near: DVec3::new(0.0, 20.0, 5.0),
            }))),
        },
        to: AlignRefs {
            point: corner(a, first),
            primary: Some(top(a, first)),
            secondary: Some(DirRef::Axis(AxisRef::Edge(edge))),
        },
        flip: true,
        offset: Some(Value::new("12.5", &Move::offset_ask(&design)).unwrap()),
        turn: Some(Value::new("30", &Move::angle_ask(&design)).unwrap()),
    };
    editor
        .apply(editor.document().add_feature(align.into()))
        .unwrap();
    editor.document().clone()
}

/// An align is kept through a file.
#[test]
fn an_align_round_trips() {
    let document = aligned_plates();
    let (bytes, _) = to_bytes(&document, &[]).unwrap();
    let (read, _) = from_bytes(&bytes).unwrap();
    assert_eq!(read, document);
}

/// A pattern and an align, the two kinds after the mirror, go through
/// one file together: each read back as its own kind, in its place.
#[test]
fn a_pattern_and_an_align_round_trip_together() {
    use varde_document::{Axis3, AxisRef, Copies, FeatureKind, Pattern, PatternKind};
    use varde_expr::Value;
    let mut editor = Editor::new(aligned_plates());
    let plate = editor.document().bodies()[0].id;
    let design = editor.document().design();
    let pattern = Pattern {
        bodies: vec![plate],
        kind: PatternKind::Linear {
            along: AxisRef::Origin(Axis3::X),
            count: Value::new("3", &Pattern::count_ask(&design)).unwrap(),
            spacing: Value::new("80", &Pattern::spacing_ask(&design)).unwrap(),
        },
        copies: Copies::Separate(Vec::new()),
    };
    editor
        .apply(editor.document().add_feature(pattern.into()))
        .unwrap();
    let document = editor.document();
    let kinds: Vec<&str> = (document.features().iter())
        .map(|feature| feature.kind.noun())
        .collect();
    assert_eq!(kinds[kinds.len() - 2..], ["Align", "Pattern"]);
    let (bytes, _) = to_bytes(document, &[]).unwrap();
    let (read, _) = from_bytes(&bytes).unwrap();
    assert_eq!(&read, document);
    let n = read.features().len();
    assert!(matches!(read.features()[n - 2].kind, FeatureKind::Align(_)));
    assert!(matches!(
        read.features()[n - 1].kind,
        FeatureKind::Pattern(_)
    ));
}

/// A record whose align was changed on disk to what the document refuses
/// (its offset or turn not what its text gives or past its bounds, a
/// reference's point out of bounds) is refused as it's read, never a
/// panic; as written, it reads.
#[test]
fn a_tampered_align_is_refused() {
    let raw = record_msgpack(&aligned_plates());
    assert!(from_msgpack::<Document>(&raw).is_ok());
    let float = |x: f64| {
        let mut bytes = vec![0xcb];
        bytes.extend_from_slice(&x.to_bits().to_be_bytes());
        bytes
    };
    let swap = |was: f64, now: f64| {
        let (was, now) = (float(was), float(now));
        let at = (raw.windows(was.len()))
            .position(|window| window == was)
            .unwrap_or_else(|| panic!("{was:?} isn't in the record"));
        let mut changed = raw.clone();
        changed[at..at + now.len()].copy_from_slice(&now);
        changed
    };
    let thirty = 30.0 * (std::f64::consts::PI / 180.0);
    for (was, now) in [
        (12.5, f64::NAN),
        (12.5, 12.25),
        (12.5, 2e6),
        (thirty, 7.0),
        (thirty, f64::INFINITY),
        (17.25, f64::NAN),
        (17.25, -1e300),
    ] {
        let decoded = from_msgpack::<Document>(&swap(was, now));
        assert!(decoded.is_err(), "{was} as {now} was taken");
    }
}

/// An align's part of a record damaged on disk, every float in it set to
/// what's out of bounds or not a number and random bytes in it changed
/// (2 000 ways): refused as it's read or read as a document that passes
/// its check, never a panic.
#[test]
fn a_damaged_align_is_refused_or_checked() {
    let raw = record_msgpack(&aligned_plates());
    let from = (raw.windows(5))
        .position(|window| window == b"Align")
        .expect("the align's variant name");
    let read = |bytes: &[u8]| {
        if let Ok((document, _)) = from_msgpack::<Document>(bytes) {
            document.check().unwrap();
            for feature in document.features() {
                if let varde_document::FeatureKind::Align(align) = &feature.kind {
                    align.check_own(&document.design()).unwrap();
                }
            }
        }
    };
    // Every float after it.
    let floats: Vec<usize> = (from..raw.len().saturating_sub(8))
        .filter(|&at| raw[at] == 0xcb)
        .collect();
    assert!(floats.len() > 10, "{}", floats.len());
    for &at in &floats {
        for x in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1e300,
            -1e300,
            1.000_001e6,
            1e6,
            -0.0,
            f64::MIN_POSITIVE,
        ] {
            let mut changed = raw.clone();
            changed[at + 1..at + 9].copy_from_slice(&x.to_bits().to_be_bytes());
            read(&changed);
        }
    }
    // Random bytes.
    let size = raw.len() - from;
    for seed in 0..2_000u64 {
        let picks = noise(8, seed.wrapping_mul(0x9e37_79b9) + 1);
        let mut changed = raw.clone();
        for pair in picks.chunks(2).take(1 + (seed % 3) as usize) {
            let at = from + (usize::from(pair[0]) * 256 + usize::from(pair[1])) % size;
            changed[at] = picks[(seed % 8) as usize] ^ pair[1];
        }
        read(&changed);
    }
}

/// The example plate scaled about its top corner by 2.5, 1.25 and 1 per
/// axis, then along the axis of its top edge on the side of curve 0 so
/// that edge is 77.75 long: the document.
fn scaled_plate() -> Document {
    use glam::DVec3;
    use varde_document::{EdgeRef, FaceKey, PartKey, PointRef, Scale, ScaleFactor};
    use varde_expr::Value;
    let mut editor = Editor::new(Document::example());
    let document = editor.document();
    let plate = document.bodies()[0].id;
    let maker = document.features()[1].id.get();
    let design = document.design();
    let key = |part| FaceKey {
        feature: maker,
        part,
        instance: 0,
    };
    let mut faces = [
        key(PartKey::EndCap),
        key(PartKey::Side { curve: 0 }),
        key(PartKey::Side { curve: 1 }),
    ];
    faces.sort();
    let factor = |text| Value::new(text, &Scale::factor_ask(&design)).unwrap();
    let per_axis = Scale {
        bodies: vec![plate],
        about: PointRef::Corner {
            body: plate,
            faces,
            near: DVec3::new(30.0, 17.25, 10.0),
        },
        factor: ScaleFactor::PerAxis([factor("2.5"), factor("1.25"), factor("1")]),
    };
    editor
        .apply(editor.document().add_feature(per_axis.into()))
        .unwrap();
    let mut faces = [key(PartKey::EndCap), key(PartKey::Side { curve: 0 })];
    faces.sort();
    let to_edge = Scale {
        bodies: vec![plate],
        about: PointRef::Origin,
        factor: ScaleFactor::EdgeLength {
            edge: EdgeRef {
                body: plate,
                faces,
                near: DVec3::new(0.0, 20.0, 10.0),
            },
            length: Value::new("77.75", &Scale::length_ask(&design)).unwrap(),
            axis_only: true,
        },
    };
    editor
        .apply(editor.document().add_feature(to_edge.into()))
        .unwrap();
    editor.document().clone()
}

/// Scales go through a file after an align, each read back as a scale
/// in its place.
#[test]
fn scales_round_trip_after_an_align() {
    use varde_document::FeatureKind;
    let mut document = scaled_plate();
    let (bytes, _) = to_bytes(&document, &[]).unwrap();
    let (read, _) = from_bytes(&bytes).unwrap();
    assert_eq!(read, document);
    // After an align.
    let aligned = aligned_plates();
    let mut editor = Editor::new(aligned);
    for feature in &document.features()[2..] {
        editor
            .apply(editor.document().add_feature(feature.kind.clone()))
            .unwrap();
    }
    document = editor.document().clone();
    let (bytes, _) = to_bytes(&document, &[]).unwrap();
    let (read, _) = from_bytes(&bytes).unwrap();
    assert_eq!(read, document);
    let n = read.features().len();
    assert!(matches!(read.features()[n - 3].kind, FeatureKind::Align(_)));
    assert!(matches!(read.features()[n - 1].kind, FeatureKind::Scale(_)));
}

/// A record whose scale was changed on disk to what the document refuses
/// (a factor not what its text gives or out of range, an edge length out
/// of bounds, a point out of bounds) is refused as it's read, never a
/// panic; as written, it reads.
#[test]
fn a_tampered_scale_is_refused() {
    let raw = record_msgpack(&scaled_plate());
    assert!(from_msgpack::<Document>(&raw).is_ok());
    let float = |x: f64| {
        let mut bytes = vec![0xcb];
        bytes.extend_from_slice(&x.to_bits().to_be_bytes());
        bytes
    };
    let swap = |was: f64, now: f64| {
        let (was, now) = (float(was), float(now));
        let at = (raw.windows(was.len()))
            .position(|window| window == was)
            .unwrap_or_else(|| panic!("{was:?} isn't in the record"));
        let mut changed = raw.clone();
        changed[at..at + now.len()].copy_from_slice(&now);
        changed
    };
    for (was, now) in [
        (2.5, f64::NAN),
        (2.5, 2.25),
        (1.25, 2e3),
        (77.75, 0.0),
        (77.75, f64::INFINITY),
        (17.25, f64::NAN),
        (17.25, 3e6),
    ] {
        let decoded = from_msgpack::<Document>(&swap(was, now));
        assert!(decoded.is_err(), "{was} as {now} was taken");
    }
}

/// The example plate split by XY keeping both sides, then by its top
/// face's plane keeping the back: the document.
fn split_plate() -> Document {
    use glam::DVec3;
    use varde_document::{
        FaceKey, FaceRef, Keep, OriginPlane, PartKey, PlaneRef, Side, Split, SplitTool,
    };
    let mut editor = Editor::new(Document::example());
    let document = editor.document();
    let plate = document.bodies()[0].id;
    let maker = document.features()[1].id.get();
    let by_xy = Split {
        body: plate,
        tool: SplitTool::Plane(PlaneRef::Origin(OriginPlane::XY)),
        original: Side::Back,
        keep: Keep::Both,
        new_body: None,
    };
    editor
        .apply(editor.document().add_feature(by_xy.into()))
        .unwrap();
    let top = FaceRef {
        body: plate,
        key: FaceKey {
            feature: maker,
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::new(3.0, 7.25, 10.0),
    };
    let trim = Split {
        body: plate,
        tool: SplitTool::Plane(PlaneRef::Face(top)),
        original: Side::Front,
        keep: Keep::Back,
        new_body: None,
    };
    editor
        .apply(editor.document().add_feature(trim.into()))
        .unwrap();
    editor.document().clone()
}

/// Splits go through a file, each read back as a split in its place,
/// the first with the new body it makes.
#[test]
fn splits_round_trip() {
    use varde_document::FeatureKind;
    let document = split_plate();
    let (bytes, _) = to_bytes(&document, &[]).unwrap();
    let (read, _) = from_bytes(&bytes).unwrap();
    assert_eq!(read, document);
    assert_eq!(read.bodies().len(), 2);
    let FeatureKind::Split(split) = &read.features()[2].kind else {
        panic!("a split");
    };
    assert_eq!(split.new_body, Some(read.bodies()[1].id));
}

/// A record whose split's face point was changed on disk to what the
/// document refuses is refused as it's read; as written, it reads.
#[test]
fn a_tampered_split_is_refused() {
    let raw = record_msgpack(&split_plate());
    assert!(from_msgpack::<Document>(&raw).is_ok());
    let was = {
        let mut bytes = vec![0xcb];
        bytes.extend_from_slice(&7.25f64.to_bits().to_be_bytes());
        bytes
    };
    let at = (raw.windows(was.len()))
        .position(|window| window == was)
        .expect("the face's point is in the record");
    for now in [f64::NAN, f64::INFINITY, 3e6] {
        let mut changed = raw.clone();
        changed[at + 1..at + 9].copy_from_slice(&now.to_bits().to_be_bytes());
        assert!(
            from_msgpack::<Document>(&changed).is_err(),
            "{now} was taken"
        );
    }
}
