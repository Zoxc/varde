//! The block framing: a block's layout and kinds, checking one intact,
//! hashing and framing one, and finding one in a file, see the module
//! docs of [`vrdp`](super).

use std::io;

use xxhash_rust::xxh3::Xxh3Default;

use super::record::MAX_DECOMPRESSED;
use super::{Error, ReadAt, Result, Tail};

/// What kind of block a block is, its last 16 bytes.
pub(super) type Kind = [u8; 16];
/// A design file's record: a saved [`Document`](varde_document::Document).
pub(super) const RECORD: Kind =
    *b"\xf0\x92\xcd\x00\x73\x28\xa2\xda\x21\x72\x72\xf7\x83\x46\x33\x4d";
/// A design file's preview, after its last record, see [`Preview`](super::Preview).
/// Reading a file's records steps over them as over any kind it doesn't
/// know.
pub(super) const PREVIEW: Kind =
    *b"\xf1\x98\x66\xb7\x25\xe7\xf7\xb0\x9d\x73\xef\x9a\x3a\x14\x50\xb6";
/// A held file's record: what a [`HeldFile`](super::HeldFile) holds.
pub(super) const AUTOSAVE: Kind =
    *b"\x16\x85\x72\x3f\x4d\x25\x9d\x78\x8e\x39\xc4\x5f\x2d\xa8\xf1\x8d";

/// Offset of `prev` in a block.
pub(super) const PREV_AT: usize = 8;
/// Offset of `sum` in a block.
pub(super) const SUM_AT: usize = 24;
/// Offset of `tag_len` in a block, after `len`, `prev` and `sum`.
pub(super) const TAG_LEN_AT: usize = 40;
/// Bytes a block adds around what its `len` covers.
pub(super) const BLOCK_OVERHEAD: u64 = 64;
/// Bytes after what a block's `len` covers: `len` again, and `kind`.
pub(super) const BACK_LEN: usize = 8 + 16;

/// The most a block's `len` may be: what snappy may compress
/// [`MAX_DECOMPRESSED`] bytes to (`snap::raw::max_compress_len`, which
/// isn't `const`, so its formula, checked by a test), `tag_len`, and the
/// longest tag. Checked before anything is allocated for a block, whatever
/// its kind.
pub(super) const MAX_BLOCK: u64 = {
    let decompressed = MAX_DECOMPRESSED as u64;
    32 + decompressed + decompressed / 6 + 2 + u16::MAX as u64
};

/// The longest tag a writer writes. Readers take any a `u16` holds.
pub(super) const MAX_TAG: usize = 1024;

/// An intact block: both its lengths agree, and its `sum` checks.
#[derive(Debug, Clone, Copy)]
pub(super) struct Block {
    /// Its offset in the file.
    pub(super) start: u64,
    /// Its `len`.
    pub(super) len: u64,
    pub(super) tag_len: u16,
    pub(super) prev: u128,
    pub(super) sum: u128,
    pub(super) kind: Kind,
    /// The offset just past it.
    pub(super) end: u64,
}

impl Block {
    /// The tail of the file cut short right after it.
    pub(super) fn tail(&self) -> Tail {
        Tail {
            last: self.start,
            sum: self.sum,
            end: self.end,
        }
    }

    /// Its tag and its payload, in `bytes`, all of the file it's in.
    pub(super) fn parts<'a>(&self, bytes: &'a [u8]) -> (&'a [u8], &'a [u8]) {
        // Within `bytes`, where the block was found intact, so exact.
        let start = self.start as usize + TAG_LEN_AT + 2;
        let tag_end = start + usize::from(self.tag_len);
        let end = self.start as usize + TAG_LEN_AT + self.len as usize;
        (&bytes[start..tag_end], &bytes[tag_end..end])
    }
}

/// `block`, all of a block by its first `len`, at `start` in a file with
/// `id`, if it's intact.
pub(super) fn intact(block: &[u8], start: u64, id: u128) -> Option<Block> {
    let size = block.len();
    let len = u64_at(block, 0);
    let tag_len = tag_len_of(block);
    if !len_fits(len, tag_len) || !back_is(&block[size - BACK_LEN..], len, &block[size - 16..]) {
        return None;
    }
    let sum = u128_at(block, SUM_AT);
    if block_sum(id, block) != sum {
        return None;
    }
    Some(Block {
        start,
        len,
        tag_len,
        prev: u128_at(block, PREV_AT),
        sum,
        kind: block[size - 16..].try_into().unwrap(),
        // `usize` is at most 64 bits wide.
        end: start.checked_add(size as u64)?,
    })
}

/// The `sum` of `block` in a file with `id`: of `id` and every byte of the
/// block but its `sum`.
pub(super) fn block_sum(id: u128, block: &[u8]) -> u128 {
    let mut hasher = BlockHasher::new(id);
    hasher.update(block);
    hasher.digest()
}

/// The hasher of a block's `sum` in a file with `id`: fed the block in
/// order, in pieces of any size, it skips the `sum`'s own bytes itself.
pub(super) struct BlockHasher {
    pub(super) hasher: Xxh3Default,
    /// How much of the block it's been fed.
    pub(super) at: usize,
}

impl BlockHasher {
    pub(super) fn new(id: u128) -> Self {
        let mut hasher = Xxh3Default::new();
        hasher.update(&id.to_le_bytes());
        Self { hasher, at: 0 }
    }

    /// Feeds it `piece`, the next bytes of the block.
    pub(super) fn update(&mut self, piece: &[u8]) {
        let start = self.at;
        let end = start + piece.len();
        // Before the `sum`, then after it, each in `piece`'s terms.
        let before = start..end.min(SUM_AT).max(start);
        let after = start.max(SUM_AT + 16).min(end)..end;
        for range in [before, after] {
            self.hasher
                .update(&piece[range.start - start..range.end - start]);
        }
        self.at = end;
    }

    pub(super) fn digest(self) -> u128 {
        self.hasher.digest128()
    }
}

/// The `tag_len` of a block whose front, at least `TAG_LEN_AT + 2` bytes
/// of it, is `front`.
pub(super) fn tag_len_of(front: &[u8]) -> u16 {
    u16::from_le_bytes([front[TAG_LEN_AT], front[TAG_LEN_AT + 1]])
}

/// Whether `len`, a block's first `len`, is within bounds for its
/// `tag_len`: long enough for the tag, and at most [`MAX_BLOCK`].
pub(super) fn len_fits(len: u64, tag_len: u16) -> bool {
    (u64::from(tag_len) + 2..=MAX_BLOCK).contains(&len)
}

/// Whether `back`, a block's last [`BACK_LEN`] bytes, says `len` and
/// `kind`.
pub(super) fn back_is(back: &[u8], len: u64, kind: &[u8]) -> bool {
    u64_at(back, 0) == len && back[8..] == *kind
}

/// What a block's header holds, whether or not the block is intact.
pub(super) struct Stored {
    pub(super) prev: u128,
    pub(super) sum: u128,
    /// Where the block ends by its first `len`, if that's within bounds:
    /// at least 2, for `tag_len`, and at most [`MAX_BLOCK`].
    pub(super) end: Option<u64>,
}

/// What the header of the block at `at` in `file`, which is `len` bytes
/// long, holds, if the file holds all of the header.
pub(super) fn stored_at(
    file: &(impl ReadAt + ?Sized),
    at: u64,
    len: u64,
) -> io::Result<Option<Stored>> {
    // `TAG_LEN_AT` is small, so exact.
    if len
        .checked_sub(at)
        .is_none_or(|rest| rest < TAG_LEN_AT as u64)
    {
        return Ok(None);
    }
    let mut front = [0; TAG_LEN_AT];
    file.read_at(&mut front, at)?;
    let block_len = u64_at(&front, 0);
    Ok(Some(Stored {
        prev: u128_at(&front, PREV_AT),
        sum: u128_at(&front, SUM_AT),
        end: (2..=MAX_BLOCK)
            .contains(&block_len)
            .then(|| at.checked_add(BLOCK_OVERHEAD + block_len))
            .flatten(),
    }))
}

/// The block ending at `end` in `file`, found through its trailing `len`,
/// if that leads back to a block starting at or after `from`: where it
/// starts, and what its header holds.
pub(super) fn stored_before(
    file: &(impl ReadAt + ?Sized),
    end: u64,
    from: u64,
) -> io::Result<Option<(u64, Stored)>> {
    let Some(len_at) = end.checked_sub(BACK_LEN as u64) else {
        return Ok(None);
    };
    let mut len = [0; 8];
    file.read_at(&mut len, len_at)?;
    let start = u64::from_le_bytes(len)
        .checked_add(BLOCK_OVERHEAD)
        .and_then(|size| end.checked_sub(size))
        .filter(|&start| start >= from);
    let Some(start) = start else {
        return Ok(None);
    };
    // The block ends at `end`, so `end` bounds its header as the file's
    // length would.
    Ok(stored_at(file, start, end)?.map(|stored| (start, stored)))
}

/// Reads `file` from `from` to `to` in pieces, handing each to `each` until
/// it returns false, so that a large block is never held in memory.
/// Whether `each` took every piece.
pub(super) fn read_pieces(
    file: &(impl ReadAt + ?Sized),
    from: u64,
    to: u64,
    mut each: impl FnMut(&[u8]) -> bool,
) -> io::Result<bool> {
    let mut piece = vec![0; 1 << 16];
    let mut at = from;
    while at < to {
        // At most the piece's length, so exact.
        let n = (to - at).min(piece.len() as u64) as usize;
        file.read_at(&mut piece[..n], at)?;
        if !each(&piece[..n]) {
            return Ok(false);
        }
        at += n as u64;
    }
    Ok(true)
}

/// The block of `kind` holding `tag` and `payload` at `at` in a file with
/// `id`, after the block whose `sum` is `prev`, and the tail of the file
/// cut short after it. One too large to read back is [`Error::TooLarge`].
pub(super) fn frame(
    id: u128,
    prev: u128,
    at: u64,
    kind: Kind,
    tag: &[u8],
    payload: &[u8],
) -> Result<(Vec<u8>, Tail)> {
    let tag_len = u16::try_from(tag.len()).map_err(|_| Error::TooLarge)?;
    let len = u64::try_from(payload.len())
        .ok()
        .and_then(|len| len.checked_add(u64::from(tag_len) + 2))
        .filter(|&len| len <= MAX_BLOCK)
        .ok_or(Error::TooLarge)?;
    let size = len
        .checked_add(BLOCK_OVERHEAD)
        .and_then(|size| usize::try_from(size).ok())
        .ok_or(Error::TooLarge)?;
    let mut block = Vec::new();
    block.try_reserve_exact(size).map_err(|_| Error::TooLarge)?;
    block.extend_from_slice(&len.to_le_bytes());
    block.extend_from_slice(&prev.to_le_bytes());
    block.extend_from_slice(&[0; 16]);
    block.extend_from_slice(&tag_len.to_le_bytes());
    block.extend_from_slice(tag);
    block.extend_from_slice(payload);
    block.extend_from_slice(&len.to_le_bytes());
    block.extend_from_slice(&kind);
    let sum = block_sum(id, &block);
    block[SUM_AT..SUM_AT + 16].copy_from_slice(&sum.to_le_bytes());
    let end = u64::try_from(size)
        .ok()
        .and_then(|size| at.checked_add(size))
        .ok_or_else(|| io::Error::from(io::ErrorKind::FileTooLarge))?;
    Ok((block, Tail { last: at, sum, end }))
}

pub(super) fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

pub(super) fn u128_at(bytes: &[u8], at: usize) -> u128 {
    u128::from_le_bytes(bytes[at..at + 16].try_into().unwrap())
}
