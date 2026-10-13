//! What the format reads and writes through ([`ReadAt`], [`Storage`]),
//! and reading a whole file into memory.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};

use super::{Error, FILE_HEADER_LEN, FileType, MAGIC_LEN, PREFIX_LEN, Result};

/// What the `.vrdp` code needs to read a file: its length, and reading at
/// an offset. [`Storage`] adds writing; a byte slice, a whole file read
/// into memory, only reads.
pub(crate) trait ReadAt {
    /// The length of the file, in bytes.
    fn len(&self) -> io::Result<u64>;

    /// Whether the file has nothing in it.
    fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Fills `buf` with the bytes starting at `at`, failing with
    /// [`io::ErrorKind::UnexpectedEof`] if the file ends before.
    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()>;
}

/// What the `.vrdp` code needs of a file: reading and writing at an
/// offset, truncating and syncing. [`File`] natively; the web build's IO
/// worker implements it on an Origin Private File System sync access
/// handle, which only has these, and no cursor.
pub(crate) trait Storage: ReadAt {
    /// Writes all of `buf` starting at `at`, growing the file if needed.
    fn write_at(&mut self, buf: &[u8], at: u64) -> io::Result<()>;

    /// Cuts the file to `len` bytes, or grows it with zeros to that.
    fn truncate(&mut self, len: u64) -> io::Result<()>;

    /// Makes what was written durable.
    fn sync(&mut self) -> io::Result<()>;
}

impl ReadAt for [u8] {
    fn len(&self) -> io::Result<u64> {
        // `usize` is at most 64 bits wide, so exact.
        Ok(<[u8]>::len(self) as u64)
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        let bytes = usize::try_from(at)
            .ok()
            .and_then(|at| self.get(at..at.checked_add(buf.len())?))
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        buf.copy_from_slice(bytes);
        Ok(())
    }
}

impl ReadAt for File {
    fn len(&self) -> io::Result<u64> {
        Ok(self.metadata()?.len())
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        let mut file = self;
        file.seek(SeekFrom::Start(at))?;
        file.read_exact(buf)
    }
}

impl Storage for File {
    fn write_at(&mut self, buf: &[u8], at: u64) -> io::Result<()> {
        self.seek(SeekFrom::Start(at))?;
        self.write_all(buf)
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.set_len(len)
    }

    fn sync(&mut self) -> io::Result<()> {
        self.sync_data()
    }
}

/// Writes `bytes` at `at`, dropping whatever follows: a torn tail, or
/// anything else, and syncs them. Should any of that fail, the file is cut
/// back to `at`, as far as it can be. `writing` is called once the bytes
/// may land, before they're written.
pub(super) fn write_or_cut_back(
    file: &mut impl Storage,
    at: u64,
    bytes: &[u8],
    writing: impl FnOnce(),
) -> io::Result<()> {
    let written = file.truncate(at).and_then(|()| {
        writing();
        file.write_at(bytes, at)?;
        file.sync()
    });
    if written.is_err() {
        let _ = file.truncate(at);
    }
    written
}

/// All of `file`, once its start shows it's a `.vrdp` file of type `ty`,
/// or one cut short within the file header. Anything else is refused
/// having read no more than the file header, however large it is, and a
/// file too large to hold in memory is an error rather than an abort.
pub(super) fn read_all(file: &impl ReadAt, ty: FileType) -> Result<Vec<u8>> {
    let read = WholeRead::of_type(buffer_len(file.len()?)?, ty);
    let mut head = vec![0; read.head_len()];
    file.read_at(&mut head, 0)?;
    let mut bytes = read.buffer(head)?;
    // At most `FILE_HEADER_LEN`, so exact.
    let start = read.head_len();
    file.read_at(&mut bytes[start..], start as u64)?;
    Ok(bytes)
}

/// Reading a whole file of `len` bytes in two steps, as [`read_all`] does,
/// for callers that can't go through [`ReadAt`] (the web worker's async
/// `Blob` reads): read [`WholeRead::head_len`] bytes from the start, hand
/// them to [`WholeRead::buffer`], which refuses anything but a design file,
/// then read the rest over the buffer from `head_len` on.
pub(crate) struct WholeRead {
    pub(super) len: usize,
    pub(super) ty: FileType,
}

impl WholeRead {
    /// For a design file of `len` bytes.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn new(len: usize) -> Self {
        Self::of_type(len, FileType::Design)
    }

    pub(super) fn of_type(len: usize, ty: FileType) -> Self {
        WholeRead { len, ty }
    }

    /// How many bytes to read first, from the start: the file header, or
    /// all of a file shorter than it.
    pub(crate) fn head_len(&self) -> usize {
        self.len.min(FILE_HEADER_LEN)
    }

    /// The buffer to read the whole file into, given `head`, its first
    /// [`WholeRead::head_len`] bytes: `head` followed by zeros to read the
    /// rest over, once [`check_file_header`] passes. Reserved fallibly, so
    /// a file too large to hold in memory is an error rather than an
    /// abort.
    ///
    /// Panics if `head` isn't `head_len` bytes: the header check accepts
    /// any prefix of the header, so a shorter one would pass anything.
    pub(crate) fn buffer(&self, head: Vec<u8>) -> Result<Vec<u8>> {
        assert_eq!(head.len(), self.head_len(), "the head of a whole read");
        check_file_header(&head, self.ty)?;
        let mut bytes = zeroed(self.len, "the file is too large to open")?;
        bytes[..head.len()].copy_from_slice(&head);
        Ok(bytes)
    }
}

/// Checks that `head`, the first bytes of a file up to [`FILE_HEADER_LEN`]
/// of them, starts the file header of a file of type `ty`: one of the other
/// type is refused as such, see [`FileType::read_as_other`], one of
/// another version is [`Error::UnsupportedVersion`], anything else
/// [`Error::NotVarde`]. The `id` may be anything.
pub(super) fn check_file_header(head: &[u8], ty: FileType) -> Result<()> {
    let prefix = &head[..head.len().min(PREFIX_LEN)];
    if ty.prefix().starts_with(prefix) {
        return Ok(());
    }
    if head.starts_with(ty.other().magic()) {
        return Err(ty.other().read_as_other());
    }
    match head.get(MAGIC_LEN..PREFIX_LEN) {
        Some(version) if head.starts_with(ty.magic()) => Err(Error::UnsupportedVersion(
            u32::from_le_bytes(version.try_into().unwrap()),
        )),
        _ => Err(Error::NotVarde),
    }
}

/// A file length as a buffer length, which on the web's 32 bit wasm is
/// smaller than a file can be.
pub(super) fn buffer_len(len: u64) -> io::Result<usize> {
    usize::try_from(len).map_err(|_| io::Error::from(io::ErrorKind::FileTooLarge))
}

/// `len` zeros to read into, reserved fallibly, so a file too large to
/// hold in memory is an error, `too_large` and the length, rather than an
/// abort.
pub(super) fn zeroed(len: usize, too_large: &str) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            format!("{too_large}: {len} bytes"),
        )
    })?;
    bytes.resize(len, 0);
    Ok(bytes)
}
