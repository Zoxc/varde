//! A design file's previews: `PREVIEW` blocks after its last record,
//! written with each save, and read from the end of the file without
//! reading any record's payload, see [`read_preview`].

use std::io;

use xxhash_rust::xxh3::Xxh3Default;

use super::{
    BLOCK_OVERHEAD, Error, FILE_HEADER_LEN, Kind, PREVIEW, RECORD, ReadAt, Result, SUM_AT, Stored,
    TAG_LEN_AT, Tail, design_header, frame, header_id, intact, read_pieces, stored_before, zeroed,
};

/// The longest media type a preview may have, in bytes.
pub const MAX_MEDIA_TYPE: usize = 127;

/// The most bytes a preview's data may have.
pub const MAX_PREVIEW: usize = 4 << 20;

/// The most previews a save writes, and reading walks back over.
pub const MAX_PREVIEWS: usize = 8;

/// The longest a preview block may be, by its bounds: [`MAX_PREVIEW`] bytes
/// of data, a [`MAX_MEDIA_TYPE`] long tag, `tag_len` and the block's
/// overhead.
const MAX_PREVIEW_BLOCK: u64 = MAX_PREVIEW as u64 + MAX_MEDIA_TYPE as u64 + 2 + BLOCK_OVERHEAD;

/// A preview of a saved design, such as a thumbnail: a media type, its
/// block's `tag`, and the data as it is, not compressed again. It's never
/// needed to open or save a file: damaged, missing or stale, it's ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    media_type: String,
    data: Vec<u8>,
}

impl Preview {
    /// A preview of `media_type` holding `data`, unless the media type is
    /// longer than [`MAX_MEDIA_TYPE`] bytes or the data longer than
    /// [`MAX_PREVIEW`].
    pub fn new(media_type: impl Into<String>, data: Vec<u8>) -> Option<Preview> {
        let media_type = media_type.into();
        (media_type.len() <= MAX_MEDIA_TYPE && data.len() <= MAX_PREVIEW)
            .then_some(Preview { media_type, data })
    }

    /// Its media type, as written.
    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    /// Its data, as it is.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Whether its media type is `media_type`, compared without case and
    /// parameters: `image/PNG; x=1` is `image/png`.
    pub fn is(&self, media_type: &str) -> bool {
        essence(&self.media_type).eq_ignore_ascii_case(essence(media_type))
    }
}

/// A media type without its parameters.
fn essence(media_type: &str) -> &str {
    media_type.split(';').next().unwrap_or_default().trim()
}

/// Appends to `bytes`, which end in a record that ends the file at `tail`,
/// the first [`MAX_PREVIEWS`] of `previews` as `PREVIEW` blocks, each
/// following on from the block before, in a file with `id`. Any more are
/// left out, as reading walks back over no more than that.
pub(super) fn append_previews(
    bytes: &mut Vec<u8>,
    id: u128,
    tail: Tail,
    previews: &[Preview],
) -> Result<()> {
    let (mut prev, mut at) = (tail.sum, tail.end);
    for preview in previews.iter().take(MAX_PREVIEWS) {
        let (block, after) = frame(
            id,
            prev,
            at,
            PREVIEW,
            preview.media_type.as_bytes(),
            &preview.data,
        )?;
        bytes
            .try_reserve(block.len())
            .map_err(|_| Error::TooLarge)?;
        bytes.extend_from_slice(&block);
        (prev, at) = (after.sum, after.end);
    }
    Ok(())
}

/// The first preview of the design file `file` that `supported` accepts,
/// see [`previews`]. Any failure, of reading the file included, is no
/// preview.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn read_preview(
    file: &(impl ReadAt + ?Sized),
    supported: impl Fn(&Preview) -> bool,
) -> Option<Preview> {
    previews(file)
        .ok()?
        .into_iter()
        .find(|preview| supported(preview))
}

/// What the end of a design file says, read without any record's payload,
/// see [`end`]: what a listing of designs needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileEnd {
    /// Its header isn't a design file's.
    NotADesign,
    Design {
        /// The `sum` its newest record's header holds, if walking back to
        /// it found it: not checked, so the record may turn out damaged.
        newest: Option<u128>,
        /// Its previews, as [`previews`] reads them.
        previews: Vec<Preview>,
    },
}

/// The previews of the design file `file`'s newest record, see [`end`].
pub(super) fn previews(file: &(impl ReadAt + ?Sized)) -> io::Result<Vec<Preview>> {
    Ok(match end(file)? {
        FileEnd::NotADesign => Vec::new(),
        FileEnd::Design { previews, .. } => previews,
    })
}

/// What the end of the design file `file` says: the sum its newest
/// record's header holds and that record's previews, in the order
/// written, reading no record's payload:
///
/// 1. The file header must be a design file's.
/// 2. From the end, blocks are walked back over by their trailing `len`,
///    as long as their two lengths agree, within bounds, and at most
///    [`MAX_PREVIEWS`] of them, of any kind, until a block whose kind is
///    `RECORD`. Anything else ends the walk, with neither sum nor previews.
/// 3. Of that record only the header is read, for the `sum` it holds: the
///    record isn't checked, so a preview may show for a file whose newest
///    record turns out damaged when opened.
/// 4. Going forward from it, each block must be intact and follow on from
///    the `sum` the header of the block before holds; the previews among
///    them are taken until one isn't. A preview whose media type isn't
///    UTF-8, or that's out of bounds, is stepped over.
///
/// A preview block is read whole, at most [`MAX_PREVIEW`] and a little;
/// any other block is hashed piece by piece.
pub(crate) fn end(file: &(impl ReadAt + ?Sized)) -> io::Result<FileEnd> {
    let none = Ok(FileEnd::Design {
        newest: None,
        previews: Vec::new(),
    });
    let len = file.len()?;
    let Some(header) = design_header(file, len)? else {
        return Ok(FileEnd::NotADesign);
    };
    let id = header_id(&header);

    // The blocks after the record, newest first: where each starts and
    // ends, its kind, and what its header holds.
    let mut after: Vec<(u64, u64, Kind, Stored)> = Vec::new();
    let mut end = len;
    let record = loop {
        // `FILE_HEADER_LEN` is small, so exact.
        let Some((start, stored)) = stored_before(file, end, FILE_HEADER_LEN as u64)? else {
            return none;
        };
        // Its first `len` agrees with its trailing one, within bounds.
        if stored.end != Some(end) {
            return none;
        }
        let mut kind = [0; 16];
        // A block ends at `end`, so it's at least 16 bytes in.
        file.read_at(&mut kind, end - 16)?;
        if kind == RECORD {
            break stored;
        }
        if after.len() == MAX_PREVIEWS {
            return none;
        }
        after.push((start, end, kind, stored));
        end = start;
    };

    let mut previews = Vec::new();
    let mut prev = record.sum;
    for (start, end, kind, stored) in after.into_iter().rev() {
        if stored.prev != prev {
            break;
        }
        // Within the file and `MAX_BLOCK` and a block's overhead, so exact.
        let size = end - start;
        if kind == PREVIEW && size <= MAX_PREVIEW_BLOCK {
            let Some(preview) = preview_at(file, start, size, id)? else {
                break;
            };
            previews.extend(preview);
        } else if !streamed_intact(file, start, size, id, stored.sum)? {
            break;
        }
        prev = stored.sum;
    }
    Ok(FileEnd::Design {
        newest: Some(record.sum),
        previews,
    })
}

/// The preview block of `size` bytes at `start` in `file`, a file with
/// `id`, if it's intact: the preview it holds, if its media type is UTF-8
/// and it's within bounds.
fn preview_at(
    file: &(impl ReadAt + ?Sized),
    start: u64,
    size: u64,
    id: u128,
) -> io::Result<Option<Option<Preview>>> {
    // At most `MAX_PREVIEW_BLOCK`, so exact.
    let mut block = zeroed(size as usize, "the preview is too large to read")?;
    file.read_at(&mut block, start)?;
    let Some(intact) = intact(&block, 0, id) else {
        return Ok(None);
    };
    let (tag, data) = intact.parts(&block);
    let preview = std::str::from_utf8(tag)
        .ok()
        .and_then(|media_type| Preview::new(media_type, data.to_vec()));
    Ok(Some(preview))
}

/// Whether the block of `size` bytes at `start` in `file`, a file with
/// `id`, whose two lengths agree within bounds, is intact with `sum`, as
/// [`intact`] has it, hashed piece by piece so a large block isn't held in
/// memory.
fn streamed_intact(
    file: &(impl ReadAt + ?Sized),
    start: u64,
    size: u64,
    id: u128,
    sum: u128,
) -> io::Result<bool> {
    let mut front = [0; TAG_LEN_AT + 2];
    file.read_at(&mut front, start)?;
    let tag_len = u16::from_le_bytes([front[TAG_LEN_AT], front[TAG_LEN_AT + 1]]);
    if u64::from(tag_len) + 2 + BLOCK_OVERHEAD > size {
        return Ok(false);
    }
    let mut hasher = Xxh3Default::new();
    hasher.update(&id.to_le_bytes());
    hasher.update(&front[..SUM_AT]);
    // A block is longer than its header, and within the file.
    read_pieces(file, start + TAG_LEN_AT as u64, start + size, |piece| {
        hasher.update(piece);
        true
    })?;
    Ok(hasher.digest128() == sum)
}
