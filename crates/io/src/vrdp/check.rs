//! The check before a save: that a design file is still as a writer last
//! read or wrote it, reading only what it must, see [`check_unchanged`].

use std::io;

use xxhash_rust::xxh3::xxh3_128;

use super::chain::framed_from;
use super::{
    After, BACK_LEN, BLOCK_OVERHEAD, Block, Error, FILE_HEADER_LEN, FileType, Known, MAX_BLOCK,
    RECORD, ReadAt, Result, Stored, buffer_len, design_header, header_id, intact, read_pieces,
    stored_at, stored_before, u64_at, zeroed,
};

/// Where a save appends, once [`check_unchanged`] passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Append {
    /// The file's `id`.
    pub(super) id: u128,
    /// Where the record goes: the file is cut to it.
    pub(super) at: u64,
    /// The `sum` of the block before, the record's `prev`.
    pub(super) prev: u128,
    /// Where the block before starts.
    pub(super) before: u64,
}

/// Checks that the design file `file` is still as `known` last read or
/// wrote it, before a save appends to it or replaces it: [`save`] makes
/// this check, and the web build makes it on a whole file in memory.
///
/// A file read as [`Outcome::Damaged`] is refused first, as
/// [`Error::OpenedDamaged`]. Then the file must have the same `id`, and the
/// record at the tail must be there, intact, linked by its `prev` to the
/// block before it (the file header for the first record), found by its
/// trailing `len` or, should that be damaged, where reading or writing
/// found it, as reading steps over it by its header. Another intact
/// block there, or before it, or another file, or something else, is
/// someone else's change, [`Error::Conflict`]; anything else amiss there
/// is [`Error::Damaged`].
///
/// After it, the chain is followed as reading does, and it may hold:
///
/// - blocks of other kinds, like the previews of the save at the tail,
///   damaged ones stepped over by their headers as reading steps over
///   them, then a torn tail or the end;
/// - a block whose header holds the `sum` and `prev` of the last failed
///   save, intact or not, which landed, wholly or partly: replaced;
/// - the newest save, damaged, its header intact, as reading found it:
///   the save goes after it, as the returned [`Append`] says.
///
/// Any other record the chain reaches is someone else's change,
/// [`Error::Conflict`]; anything framed like a record after damage is
/// [`Error::Damaged`], as reading would have found it damaged. Only that
/// record, the `sum` of the block before it and the blocks after it are
/// read: not what comes before, nor the rest of whatever someone appended,
/// except to tell damage from a torn tail.
///
/// [`save`]: super::save
/// [`Outcome::Damaged`]: super::Outcome::Damaged
pub(crate) fn check_unchanged(file: &(impl ReadAt + ?Sized), known: &Known) -> Result<Append> {
    if known.after == After::Damaged {
        return Err(Error::OpenedDamaged);
    }
    let Known {
        id, tail, before, ..
    } = *known;
    let len = file.len()?;
    // Replaced by another file, or by something else.
    let Some(header) = design_header(file, len)?.filter(|header| header_id(header) == id) else {
        return Err(Error::Conflict);
    };

    // The record at `tail.last`: still there, or someone else's, or
    // damaged. Ours is after the file header.
    if !(FILE_HEADER_LEN as u64..len).contains(&tail.last) {
        return Err(Error::Damaged);
    }
    let record = match block_at(file, tail.last, len, id)? {
        Found::Intact(block) if block.sum == tail.sum && block.end == tail.end => block,
        Found::Intact(_) => return Err(Error::Conflict),
        Found::Torn | Found::Corrupt => return Err(Error::Damaged),
    };

    // Linked to the block it was appended to.
    if tail.last == FILE_HEADER_LEN as u64 {
        if xxh3_128(&header) != record.prev {
            return Err(Error::Damaged);
        }
    } else {
        match stored_before(file, tail.last, FILE_HEADER_LEN as u64)? {
            Some((_, stored)) if stored.sum == record.prev => {}
            // Its trailing `len` damaged, it's stepped over by its header,
            // as reading did.
            _ if linked_at(file, before, len, tail.last, record.prev)? => {}
            Some((start, _)) => {
                return Err(match block_at(file, start, len, id)? {
                    Found::Intact(block) if block.end == tail.last => Error::Conflict,
                    _ => Error::Damaged,
                });
            }
            None => return Err(Error::Damaged),
        }
    }

    let mut append = Append {
        id,
        at: tail.end,
        prev: tail.sum,
        before: tail.last,
    };
    let mut attempt = known.attempt;
    let mut prev = tail.sum;
    let mut at = tail.end;
    while at < len {
        let stored = stored_at(file, at, len)?;
        if let Some(stored) = &stored {
            // Ours, after all: once. Stepped over by its header, as it may
            // be damaged; where it was cut short, that's the end.
            if attempt.is_some_and(|a| (a.sum, a.prev) == (stored.sum, stored.prev)) {
                attempt = None;
                let Some(end) = stored.end.filter(|&end| end <= len) else {
                    break;
                };
                prev = stored.sum;
                at = end;
                continue;
            }
            // The damaged newest save reading found: appended after.
            if let After::NewestDamaged(damaged) = known.after
                && at == damaged.start
                && stored.end == Some(damaged.end)
                && damaged.end <= len
                && (stored.prev, stored.sum) == (prev, damaged.sum)
            {
                append = Append {
                    id,
                    at: damaged.end,
                    prev: damaged.sum,
                    before: damaged.start,
                };
                prev = damaged.sum;
                at = damaged.end;
                continue;
            }
        }
        match block_at(file, at, len, id)? {
            // Of this file's past: a torn tail.
            Found::Intact(block) if block.prev != prev => break,
            Found::Intact(block) if block.kind == RECORD => return Err(Error::Conflict),
            Found::Intact(block) => {
                prev = block.sum;
                at = block.end;
            }
            // Damaged: stepped over by its header as reading does, unless
            // it's framed like a record; else a torn tail unless something
            // after is framed like a record, which reading would find
            // damaged.
            Found::Torn | Found::Corrupt => {
                if let Some(stored) = stored
                    && let Some(end) = stepped_over(file, at, len, id, prev, &stored)?
                {
                    prev = stored.sum;
                    at = end;
                    continue;
                }
                if framed_after(file, at, len)? {
                    return Err(Error::Damaged);
                }
                break;
            }
        }
    }
    Ok(append)
}

/// Whether the header of the block at `before`, if it's known, in `file`,
/// which is `len` bytes long, leads by its first `len` to `end`, where the
/// record whose `prev` is `prev` starts, and holds that `sum`.
fn linked_at(
    file: &(impl ReadAt + ?Sized),
    before: Option<u64>,
    len: u64,
    end: u64,
    prev: u128,
) -> Result<bool> {
    let Some(before) = before else {
        return Ok(false);
    };
    Ok(stored_at(file, before, len)?
        .is_some_and(|stored| stored.end == Some(end) && stored.sum == prev))
}

/// Where the damaged block at `at` in `file`, a file with `id` that's
/// `len` bytes long, ends, if reading steps over it by its header,
/// `stored`, to a block after it that it doesn't take for a record: its
/// `prev` is `prev`, the `sum` of the block before, it isn't framed like a
/// record, and the block its `len` leads to is intact with its stored
/// `sum` as its `prev`. One framed like a record is the newest save,
/// damaged, which only the one reading found is saved after.
fn stepped_over(
    file: &(impl ReadAt + ?Sized),
    at: u64,
    len: u64,
    id: u128,
    prev: u128,
    stored: &Stored,
) -> Result<Option<u64>> {
    let Some(end) = stored.end.filter(|&end| end < len && stored.prev == prev) else {
        return Ok(None);
    };
    let mut back = [0; BACK_LEN];
    // The block is within the file, and longer than this.
    file.read_at(&mut back, end - BACK_LEN as u64)?;
    // Its `len` is within bounds, so exact.
    if u64_at(&back, 0) == end - at - BLOCK_OVERHEAD && back[8..] == RECORD {
        return Ok(None);
    }
    Ok(match block_at(file, end, len, id)? {
        Found::Intact(next) if next.prev == stored.sum => Some(end),
        _ => None,
    })
}

/// Whether anything in `file`, which is `len` bytes long, from `at` on is
/// framed like a design file's record, see [`framed_from`]. Reads the rest
/// of the file, so only for damage, which is rare.
fn framed_after(file: &(impl ReadAt + ?Sized), at: u64, len: u64) -> Result<bool> {
    let mut rest = zeroed(
        buffer_len(len.saturating_sub(at))?,
        "the rest of the file is too large to read",
    )?;
    file.read_at(&mut rest, at)?;
    Ok(framed_from(&rest, 0, FileType::Design))
}

/// What's at a place in a file where a block should be.
#[derive(Debug)]
pub(super) enum Found {
    Intact(Block),
    /// Cut off by the end of the file, as left by an interrupted append,
    /// or zeros to the end, as a crash can leave.
    Torn,
    /// Complete, but not intact.
    Corrupt,
}

/// What's at `at` in `file`, which is `len` bytes long, where a block of
/// the file with `id` should be: an intact block, a torn tail, or damage.
/// Only that block is read, if the file holds all of it, and not the rest
/// of a file someone may have appended any amount to; its `len` is bounded
/// by [`MAX_BLOCK`] before anything is allocated. Only a complete block of
/// zeros, which may be the start of a crash's zeros, has the rest read,
/// piece by piece, to tell whether it's all zeros: a real block never is,
/// as its `len` isn't.
pub(super) fn block_at(
    file: &(impl ReadAt + ?Sized),
    at: u64,
    len: u64,
    id: u128,
) -> Result<Found> {
    let rest = len
        .checked_sub(at)
        .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
    if rest < 8 {
        return Ok(Found::Torn);
    }
    let mut front = [0; 8];
    file.read_at(&mut front, at)?;
    let block_len = u64::from_le_bytes(front);
    let size = match block_len.checked_add(BLOCK_OVERHEAD) {
        Some(size) if size <= rest => size,
        _ => return Ok(Found::Torn),
    };
    if block_len > MAX_BLOCK {
        // Not intact, and its `len` isn't zeros.
        return Ok(if size == rest {
            Found::Torn
        } else {
            Found::Corrupt
        });
    }
    let mut block = zeroed(buffer_len(size)?, "the block is too large to read")?;
    file.read_at(&mut block, at)?;
    if let Some(block) = intact(&block, at, id) {
        return Ok(Found::Intact(block));
    }
    // Cut off by the end of the file.
    if rest == size {
        return Ok(Found::Torn);
    }
    let zeros = |bytes: &[u8]| bytes.iter().all(|&b| b == 0);
    // Within the file.
    if zeros(&block) && read_pieces(file, at + size, len, zeros)? {
        return Ok(Found::Torn);
    }
    Ok(Found::Corrupt)
}
