use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use varde_document::{Command, Editor, Opacity, OriginPlane, Plane};

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
    write_new(&mut file, document).unwrap()
}

/// Saves `document` to the design at `path`, last read or written as
/// `tail`, as the IO lane does, less the lock.
fn save_at(path: &Path, tail: Tail, document: &Document) -> Result<Tail> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    save(&mut file, tail, document)
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
    let (bytes, _) = to_bytes(document).unwrap();
    let (read, _) = from_bytes(&bytes).unwrap();
    assert_eq!(read.body(body).unwrap().opacity, opacity);
    assert_eq!(&read, document);
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

/// Storage whose syncs fail.
struct SyncFails(Memory);

impl ReadAt for SyncFails {
    fn len(&self) -> io::Result<u64> {
        self.0.len()
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        self.0.read_at(buf, at)
    }
}

impl Storage for SyncFails {
    fn write_at(&mut self, buf: &[u8], at: u64) -> io::Result<()> {
        self.0.write_at(buf, at)
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.0.truncate(len)
    }

    fn sync(&mut self) -> io::Result<()> {
        Err(io::Error::other("fsync failed"))
    }
}

/// A save whose sync fails leaves the file at the version before, and the
/// next save goes ahead rather than finding its own record a conflict.
#[test]
fn a_failed_sync_leaves_the_next_save_working() {
    let mut file = SyncFails(Memory::default());
    let tail = write_new(&mut file, &edited(0)).unwrap();
    let failed = save(&mut file, tail, &edited(1));
    assert!(matches!(failed, Err(Error::Io(_))));
    let mut file = file.0;
    assert_eq!(read(&file).unwrap(), (edited(0), tail));
    save(&mut file, tail, &edited(1)).unwrap();
    assert_eq!(read(&file).unwrap().0, edited(1));
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
}

#[test]
fn torn_append_is_ignored_and_truncated() {
    let dir = TempDir::new("torn");
    let mut tail = create_at(&dir.file(), &edited(0));
    let (record, _) = encode(&edited(1)).unwrap();
    let header = &record[..HEADER_LEN as usize];
    let footer_start = record.len() - FOOTER.len();

    // Header only, a zero-filled payload of the right length, a missing
    // footer, a zeroed footer, and a zeroed header too.
    for torn in [
        header.to_vec(),
        [header, &vec![0; footer_start - header.len()]].concat(),
        record[..footer_start].to_vec(),
        [&record[..footer_start], &[0; FOOTER.len()][..]].concat(),
        vec![0; record.len()],
    ] {
        let len = std::fs::metadata(dir.file()).unwrap().len();
        append(&dir.file(), &torn);
        assert_eq!(open_at(&dir.file()).unwrap(), (edited(0), tail));
        tail = save_at(&dir.file(), tail, &edited(0)).unwrap();
        assert_eq!(
            std::fs::metadata(dir.file()).unwrap().len(),
            len + encode(&edited(0)).unwrap().0.len() as u64
        );
    }
}

#[test]
fn corruption_before_the_end_is_an_error() {
    let dir = TempDir::new("corrupt");
    let tail = create_at(&dir.file(), &edited(0));
    save_at(&dir.file(), tail, &edited(1)).unwrap();

    let mut bytes = std::fs::read(dir.file()).unwrap();
    bytes[FILE_HEADER_LEN + HEADER_LEN as usize] ^= 0xff;
    std::fs::write(dir.file(), bytes).unwrap();
    assert!(matches!(
        open_at(&dir.file()),
        Err(Error::Corrupt { offset }) if offset == FILE_HEADER_LEN as u64
    ));
}

#[test]
fn damaged_footer_before_the_end_is_an_error() {
    let dir = TempDir::new("footer");
    let tail = create_at(&dir.file(), &edited(0));
    save_at(&dir.file(), tail, &edited(1)).unwrap();

    let mut bytes = std::fs::read(dir.file()).unwrap();
    let footer_end = FILE_HEADER_LEN + encode(&edited(0)).unwrap().0.len();
    bytes[footer_end - 1] ^= 0xff;
    std::fs::write(dir.file(), bytes).unwrap();
    assert!(matches!(
        open_at(&dir.file()),
        Err(Error::Corrupt { offset }) if offset == FILE_HEADER_LEN as u64
    ));
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
    bytes[MAGIC.len()] = 2;
    std::fs::write(dir.file(), bytes).unwrap();
    assert!(matches!(
        open_at(&dir.file()),
        Err(Error::UnsupportedVersion(2))
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

    let mut bytes = FILE_HEADER.to_vec();
    bytes[MAGIC.len()] = 2;
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
        head: FILE_HEADER.to_vec(),
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
    let head = FILE_HEADER.to_vec();
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

/// A save after someone appended a lot reads only the next record to tell
/// a newer version, or other content, from a torn append, rather than all
/// of it, which could be more than memory holds.
#[test]
fn a_save_reads_only_the_next_record() {
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

    // A newer record, and one with a damaged footer, in storage too large
    // for memory: only the header and then the record are read.
    let (first, _) = encode(&edited(0)).unwrap();
    let (second, _) = encode(&edited(1)).unwrap();
    let mut damaged = second.clone();
    *damaged.last_mut().unwrap() ^= 0xff;
    let at = (FILE_HEADER_LEN + first.len()) as u64;
    for (after, corrupt) in [(second, false), (damaged, true)] {
        let huge = Huge {
            head: [&FILE_HEADER[..], &first, &after].concat(),
            read: Default::default(),
        };
        let found = record_at(&huge, at, huge.len().unwrap()).unwrap();
        assert_eq!(matches!(found, Record::Corrupt), corrupt);
        assert!(matches!(found, Record::Valid { .. }) != corrupt);
        assert_eq!(huge.read.get(), HEADER_LEN + after.len() as u64);
    }
}

/// A record's payload says how long it decompresses to, and the decoder
/// allocates that up front: a claim no payload of its length can back is
/// refused before anything is allocated.
#[test]
fn a_payload_claiming_more_than_it_can_hold_is_refused() {
    // A varint claiming 4 GiB - 1, then a few bytes of nothing much.
    let payload = [0xff, 0xff, 0xff, 0xff, 0x0f, 0, 0, 0];
    let Err(Error::Decode(error)) = decode::<Document>(&payload) else {
        panic!("decoded");
    };
    assert!(error.to_string().contains("claims"), "{error}");

    // Through a whole file, whose checksum is fine.
    let dir = TempDir::new("snappy-claim");
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&checksum(&payload).to_le_bytes());
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(FOOTER);
    std::fs::write(dir.file(), bytes).unwrap();
    assert!(matches!(
        open_at(&dir.file()),
        Err(Error::Decode(error)) if error.to_string().contains("claims")
    ));

    // Snappy's densest output, of a run of zeros, is within the bound, and
    // real documents decode.
    let zeros = vec![0; 1 << 20];
    let dense = snap::raw::Encoder::new().compress_vec(&zeros).unwrap();
    assert!(zeros.len() <= dense.len() * MAX_EXPANSION);
    let large = edited(2000);
    let (record, _) = encode(&large).unwrap();
    let payload = &record[HEADER_LEN as usize..record.len() - FOOTER.len()];
    assert_eq!(decode::<Document>(payload).unwrap(), large);
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
    // It's a `.vrdp` like any other.
    assert_eq!(open_at(&dir.file()).unwrap().0, edited(2));
    // Read by another handle, which appends after what's there.
    file = held(&dir.file());
    file.append(&edited(3)).unwrap();
    assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(3)));

    file.clear().unwrap();
    assert!(file.is_empty().unwrap());
    assert_eq!(file.read().unwrap(), None);
    file.append(&edited(4)).unwrap();
    assert_eq!(open_at(&dir.file()).unwrap().0, edited(4));
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
    let (record, _) = encode(&edited(2)).unwrap();
    append(&dir.file(), &record[..record.len() - 3]);
    assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(1)));

    let mut again = held(&dir.file());
    again.append(&edited(3)).unwrap();
    assert_eq!(
        std::fs::metadata(dir.file()).unwrap().len(),
        len + encode(&edited(3)).unwrap().0.len() as u64
    );
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

/// What can't be read is an error, and appending starts over.
#[test]
fn a_held_file_starts_over_from_garbage() {
    let dir = TempDir::new("held-garbage");
    std::fs::write(dir.file(), b"not a design at all").unwrap();
    let mut file = held(&dir.file());
    assert!(matches!(file.read(), Err(Error::NotVarde)));
    file.append(&edited(1)).unwrap();
    assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(1)));

    // Damaged before the end.
    let mut file = held(&dir.file());
    file.append(&edited(2)).unwrap();
    let mut bytes = std::fs::read(dir.file()).unwrap();
    bytes[FILE_HEADER_LEN + HEADER_LEN as usize] ^= 0xff;
    std::fs::write(dir.file(), bytes).unwrap();
    let mut file = held(&dir.file());
    assert!(matches!(file.read(), Err(Error::Corrupt { .. })));
    file.append(&edited(3)).unwrap();
    assert_eq!(held(&dir.file()).read().unwrap(), Some(edited(3)));
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
    let memory = file.into_storage();
    std::fs::write(dir.file(), &memory.bytes).unwrap();
    assert_eq!(open_at(&dir.file()).unwrap().0, edited(2));

    // A torn append leaves the last complete document, and the next
    // append drops it.
    let mut torn = memory.bytes.clone();
    let (record, _) = encode(&edited(3)).unwrap();
    torn.extend_from_slice(&record[..record.len() / 2]);
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
    let short = &FILE_HEADER[..FILE_HEADER_LEN - 1];
    let held = |bytes: &[u8]| Records::scan(bytes, true)?.undamaged();
    let design = |bytes: &[u8]| Records::scan(bytes, false)?.undamaged();
    assert!(held(short).unwrap().is_empty());
    assert!(matches!(design(short), Err(Error::NotVarde)));
    assert!(matches!(held(b"var-"), Err(Error::NotVarde)));

    let mut other = FILE_HEADER;
    other[MAGIC.len()] = 2;
    for check in [held, design] {
        assert!(matches!(check(&other), Err(Error::UnsupportedVersion(2))));
        assert!(matches!(check(b"hello"), Err(Error::NotVarde)));
        assert!(check(&FILE_HEADER).unwrap().is_empty());
    }
}

/// The tail tells saved versions apart: each save gives another, another
/// handle sees the same one, and a file rewritten with other content has
/// another too.
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
    type Error = varde_document::CheckError;

    fn check(unchecked: UncheckedBased) -> std::result::Result<Based, varde_document::CheckError> {
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

    let mut torn = file.into_storage().bytes;
    let (record, _) = encode(&based(4)).unwrap();
    torn.extend_from_slice(&record[..record.len() - 1]);
    let mut file = HeldFile::<Memory, Based>::new(Memory {
        bytes: torn,
        syncs: 0,
    });
    assert_eq!(file.read().unwrap(), Some(based(3)));
    // Not a plain document.
    let mut plain = HeldFile::<Memory>::new(file.into_storage());
    assert!(matches!(plain.read(), Err(Error::Decode(_))));
}

/// A record holding more than a document, like an auto-save, isn't taken
/// for the document its first bytes decode as. The codec's tests check the
/// bytes alone.
#[test]
fn a_record_holding_more_than_a_document_is_refused() {
    let based = Based {
        base: None,
        document: edited(1),
    };

    let dir = TempDir::new("based-as-design");
    let mut file = HeldFile::<File, Based>::new(held(&dir.file()).into_storage());
    file.append(&based).unwrap();
    drop(file);
    assert!(matches!(open_at(&dir.file()), Err(Error::Decode(_))));
}

/// Rolling back drops the records after the newest one accepted, and a
/// torn one, leaving it the newest to read and append after; accepting
/// none leaves the file as it is.
#[test]
fn a_held_file_rolls_back_to_the_newest_record_accepted() {
    let based = |base, n| Based {
        base,
        document: edited(n),
    };
    let dir = TempDir::new("held-roll-back");
    let marked = Some(create_at(&dir.file(), &edited(0)));
    let mut file = HeldFile::<Memory, Based>::new(Memory::default());
    assert_eq!(file.roll_back(|_| true).unwrap(), None);
    file.append(&based(marked, 1)).unwrap();
    file.append(&based(None, 2)).unwrap();
    let len = file.storage().bytes.len();
    file.append(&based(marked, 3)).unwrap();
    file.append(&based(None, 4)).unwrap();
    file.append(&based(None, 5)).unwrap();
    let (record, _) = encode(&based(None, 6)).unwrap();
    let mut torn = file.into_storage().bytes;
    torn.extend_from_slice(&record[..record.len() - 2]);
    let mut file = HeldFile::<Memory, Based>::new(Memory {
        bytes: torn,
        syncs: 0,
    });

    let accepted = |saved: &Based| saved.base.is_some();
    assert_eq!(file.roll_back(accepted).unwrap(), Some(based(marked, 3)));
    assert_eq!(
        file.storage().bytes.len(),
        len + encode(&based(marked, 3)).unwrap().0.len()
    );
    assert_eq!(file.read().unwrap(), Some(based(marked, 3)));
    // Already the newest.
    assert_eq!(file.roll_back(accepted).unwrap(), Some(based(marked, 3)));
    file.append(&based(None, 7)).unwrap();
    assert_eq!(file.read().unwrap(), Some(based(None, 7)));

    let before = file.storage().bytes.clone();
    assert_eq!(
        file.roll_back(|saved| saved.document == edited(9)).unwrap(),
        None
    );
    assert_eq!(file.storage().bytes, before);
    let mut file = HeldFile::<Memory, Based>::new(Memory {
        bytes: b"not a design at all".to_vec(),
        syncs: 0,
    });
    assert!(matches!(file.roll_back(accepted), Err(Error::NotVarde)));
}

/// A record damaged after the one to go back to, say by the disk, doesn't
/// keep rolling back from going back to it: the records before it are
/// whole, and cutting the file short drops the damage too. Records that
/// don't decode aren't accepted either. Nothing to go back to before the
/// damage leaves the file as it is.
#[test]
fn a_held_file_rolls_back_past_damage() {
    let based = |base, n| Based {
        base,
        document: edited(n),
    };
    let dir = TempDir::new("held-roll-back-damaged");
    let marked = Some(create_at(&dir.file(), &edited(0)));
    let accepted = |saved: &Based| saved.base.is_some();
    let mut file = HeldFile::<Memory, Based>::new(Memory::default());
    file.append(&based(marked, 1)).unwrap();
    let len = file.storage().bytes.len();
    file.append(&based(None, 2)).unwrap();
    let damaged = file.storage().bytes.len() - FOOTER.len() - 1;
    file.append(&based(marked, 3)).unwrap();
    let mut bytes = file.into_storage().bytes;
    // The second record's payload, followed by the third.
    bytes[damaged] ^= 0xff;
    let mut file = HeldFile::<Memory, Based>::new(Memory {
        bytes: bytes.clone(),
        syncs: 0,
    });
    assert!(matches!(file.read(), Err(Error::Corrupt { .. })));
    assert_eq!(file.roll_back(accepted).unwrap(), Some(based(marked, 1)));
    assert_eq!(file.storage().bytes.len(), len);
    assert_eq!(file.read().unwrap(), Some(based(marked, 1)));

    let mut file = HeldFile::<Memory, Based>::new(Memory { bytes, syncs: 0 });
    let before = file.storage().bytes.clone();
    assert_eq!(
        file.roll_back(|saved| saved.document == edited(3)).unwrap(),
        None
    );
    assert_eq!(file.storage().bytes, before);
}

/// A whole file made in memory, as the web build writes files of the
/// user's, is a `.vrdp` like any other: one record, which opening reads,
/// with the tail opening finds, and saving appends to.
#[test]
fn a_whole_file_in_memory_is_a_file_with_one_record() {
    let dir = TempDir::new("to-bytes");
    let (bytes, tail) = to_bytes(&edited(1)).unwrap();
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
    let (bytes, _) = to_bytes(&edited(1)).unwrap();
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
    version[MAGIC.len()] = 2;
    assert!(matches!(
        from_bytes(&version),
        Err(Error::UnsupportedVersion(2))
    ));
    let mut flipped = bytes.clone();
    let payload = FILE_HEADER_LEN + HEADER_LEN as usize;
    flipped[payload] ^= 1;
    let mut corrupt = flipped.clone();
    corrupt.extend_from_slice(&bytes[FILE_HEADER_LEN..]);
    assert!(matches!(from_bytes(&corrupt), Err(Error::Corrupt { .. })));
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
/// last read or wrote: the same file, or one with a torn append after it,
/// is unchanged; a damaged record is damage; anything else is someone
/// else's change.
#[test]
fn a_file_is_unchanged_only_while_it_ends_with_the_same_record() {
    let (bytes, tail) = to_bytes(&edited(1)).unwrap();
    check_unchanged(bytes.as_slice(), tail).unwrap();

    let (record, _) = encode(&edited(2)).unwrap();
    let mut torn = bytes.clone();
    torn.extend_from_slice(&record[..record.len() - 1]);
    check_unchanged(torn.as_slice(), tail).unwrap();

    let conflict = |bytes: &[u8]| matches!(check_unchanged(bytes, tail), Err(Error::Conflict));
    let mut appended = bytes.clone();
    appended.extend_from_slice(&record);
    assert!(conflict(&appended));
    // Another program saved a document of its own, or the same one again.
    assert!(conflict(&to_bytes(&edited(2)).unwrap().0));
    let mut again = bytes.clone();
    again.extend_from_slice(&bytes[FILE_HEADER_LEN..]);
    assert!(conflict(&again));
    // Emptied, or replaced by something else.
    assert!(conflict(b""));
    assert!(conflict(b"not a design"));
    assert!(conflict(&bytes[..FILE_HEADER_LEN]));
    // Damaged.
    let mut flipped = bytes.clone();
    *flipped.last_mut().unwrap() ^= 1;
    assert!(matches!(
        check_unchanged(flipped.as_slice(), tail),
        Err(Error::Corrupt { offset }) if offset == FILE_HEADER_LEN as u64
    ));
}

/// A save natively and the web's check before replacing a file are one
/// check: the same file gets the same answer from both.
#[test]
fn a_save_and_the_web_check_agree() {
    let dir = TempDir::new("one-check");
    let tail = create_at(&dir.file(), &edited(0));
    let tail = save_at(&dir.file(), tail, &edited(1)).unwrap();
    let saved = std::fs::read(dir.file()).unwrap();

    let (record, _) = encode(&edited(2)).unwrap();
    let mut damaged = record.clone();
    *damaged.last_mut().unwrap() ^= 1;
    let mut last_damaged = saved.clone();
    *last_damaged.last_mut().unwrap() ^= 1;
    let mut first_damaged = saved.clone();
    first_damaged[FILE_HEADER_LEN + HEADER_LEN as usize] ^= 0xff;
    let conflict = "Err(Conflict)".to_owned();
    let corrupt = |offset: u64| format!("Err(Corrupt {{ offset: {offset} }})");
    let states = [
        (saved.clone(), "Ok(())".to_owned()),
        // A torn append, or damage before the last record, isn't read.
        (
            [&saved[..], &record[..record.len() - 1]].concat(),
            "Ok(())".to_owned(),
        ),
        (first_damaged, "Ok(())".to_owned()),
        ([&saved[..], &record].concat(), conflict.clone()),
        ([&saved[..], &damaged, &record].concat(), corrupt(tail.end)),
        (last_damaged, corrupt(tail.last)),
        (to_bytes(&edited(2)).unwrap().0, conflict.clone()),
        (saved[..saved.len() - 1].to_vec(), conflict.clone()),
        (Vec::new(), conflict),
    ];
    for (state, expected) in states {
        std::fs::write(dir.file(), &state).unwrap();
        let web = format!("{:?}", check_unchanged(state.as_slice(), tail));
        let native = format!("{:?}", save_at(&dir.file(), tail, &edited(3)).map(|_| ()));
        assert_eq!(web, expected);
        assert_eq!(native, expected);
    }
}
