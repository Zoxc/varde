//! Reading a whole file: following its chain, stepping over damage by
//! headers, and searching past damage that headers don't lead over, see
//! the module docs of [`vrdp`](super).

use std::collections::{HashMap, HashSet};

use serde::de::IgnoredAny;
use xxhash_rust::xxh3::xxh3_128;

use super::{
    BACK_LEN, BLOCK_OVERHEAD, Block, DamagedRecord, Error, FILE_HEADER_LEN, FileType, FoundRecord,
    MAX_BLOCK, Opened, Outcome, Payload, Report, Result, TAG_LEN_AT, check_file_header,
    decode_record, decompress, header_id, intact, stored_at, stored_before, u64_at,
    unchecked_record,
};
use crate::UnixSeconds;

/// A whole file as reading follows its chain: the records it proves, and
/// how it ended, see the module docs.
pub(super) struct Chain {
    /// The file's `id`; 0 for a held file cut short within its header,
    /// which has no blocks.
    pub(super) id: u128,
    /// The records the chain proves, oldest first: intact, and following on
    /// from the block before or from a damaged block stepped over by its
    /// header.
    pub(super) records: Vec<Block>,
    /// Where the block the newest of `records` follows on from starts,
    /// intact or stepped over by its header: `None` if it follows on from
    /// the file header.
    before_newest: Option<u64>,
    end: End,
    /// Whether damaged blocks were stepped over by their headers.
    bridged: bool,
    /// Where the first damage is, if there is any but a torn tail.
    damage: Option<u64>,
    /// The bytes that can't be read, see [`Report::unreadable`].
    unreadable: u64,
}

/// How a [`Chain`] ended.
enum End {
    /// At the end of the file.
    File,
    /// In a torn tail.
    Torn,
    /// At the newest save, damaged, its header intact.
    NewestDamaged(DamagedRecord),
    /// In damage, past which the rest of the file was searched: the newest
    /// record found, if any.
    Searched(Option<Block>),
}

impl Chain {
    /// Follows the chain of the whole file `bytes` of type `ty`. A held
    /// file shorter than the file header has no records yet, as left by a
    /// crash while the first record was written; a design file so short is
    /// [`Error::NotVarde`].
    ///
    /// Each block must be intact and follow on. An intact block that
    /// doesn't follow on is of the file's past and ends the chain as a torn
    /// tail. A block that isn't intact is stepped over by its header if
    /// that leads to an intact block following on from it: one framed like
    /// a record with no record after it is the newest save, damaged. Other
    /// damaged blocks stepped over count only if a record follows them, or
    /// damage framed like one: with neither, like a damaged preview after
    /// the last record, they're part of a torn tail. Otherwise, what
    /// follows tells how the chain ended, see [`Chain::damaged`].
    pub(super) fn scan(bytes: &[u8], ty: FileType) -> Result<Chain> {
        let mut chain = Chain {
            id: 0,
            records: Vec::new(),
            before_newest: None,
            end: End::File,
            bridged: false,
            damage: None,
            unreadable: 0,
        };
        let Some(header) = bytes.get(..FILE_HEADER_LEN) else {
            check_file_header(bytes, ty)?;
            return if ty == FileType::Held {
                Ok(chain)
            } else {
                Err(Error::NotVarde)
            };
        };
        check_file_header(header, ty)?;
        let header: &[u8; FILE_HEADER_LEN] = header.try_into().unwrap();
        chain.id = header_id(header);
        // The `sum` of every block in the chain, damaged ones stepped over
        // included, after the header's hash: the last one is what the next
        // block must follow on from.
        let mut sums = vec![xxh3_128(header)];
        // A record stepped over by its header, with no record after it: the
        // newest save, damaged.
        let mut newest_damaged = None;
        // Damaged blocks stepped over since the last record, not counted
        // yet, see [`Chain::stepped_over`].
        let mut stepped = None;
        // Where the chain's last block starts, if there is one.
        let mut last = None;
        let mut at = FILE_HEADER_LEN;
        while at < bytes.len() {
            let prev = *sums.last().unwrap();
            let block = match intact_at(bytes, at, chain.id) {
                Some(block) if block.prev == prev => block,
                // Of this file's past: a torn tail.
                Some(_) => {
                    chain.end = End::Torn;
                    break;
                }
                None => {
                    let bridge = header_at(bytes, at, prev).and_then(|damaged| {
                        // Within `bytes`, so exact.
                        let next = intact_at(bytes, damaged.end as usize, chain.id)?;
                        (next.prev == damaged.sum).then_some((damaged, next))
                    });
                    let Some((damaged, next)) = bridge else {
                        chain.damaged(bytes, ty, at, sums, stepped);
                        break;
                    };
                    let (_, unreadable) = stepped.get_or_insert((damaged.start, 0));
                    *unreadable += damaged.end - damaged.start;
                    if framed_record(bytes, &damaged, ty) {
                        newest_damaged = Some(damaged);
                        chain.stepped_over(stepped.take());
                    }
                    sums.push(damaged.sum);
                    last = Some(damaged.start);
                    next
                }
            };
            if block.kind == ty.record() {
                chain.records.push(block);
                chain.before_newest = last;
                newest_damaged = None;
                chain.stepped_over(stepped.take());
            }
            sums.push(block.sum);
            last = Some(block.start);
            // Within `bytes`, so exact.
            at = block.end as usize;
        }
        if stepped.is_some() && matches!(chain.end, End::File) {
            chain.end = End::Torn;
        }
        if let (Some(damaged), End::File | End::Torn) = (newest_damaged, &chain.end) {
            chain.end = End::NewestDamaged(damaged);
        }
        Ok(chain)
    }

    /// Counts `stepped`, the first and the bytes of damaged blocks stepped
    /// over by their headers, if any: the chain was bridged.
    fn stepped_over(&mut self, stepped: Option<(u64, u64)>) {
        if let Some((start, unreadable)) = stepped {
            self.bridged = true;
            self.damage.get_or_insert(start);
            self.unreadable += unreadable;
        }
    }

    /// Ends the chain at `at` in `bytes`, where the block isn't intact and
    /// its header doesn't lead past it, `sums` being those of the chain's
    /// blocks after the header's hash, and `stepped` the damaged blocks
    /// stepped over since the last record, see [`Chain::stepped_over`]. If
    /// nothing from there on is framed like a record, it's a torn tail,
    /// from those blocks on. If the block there is, with an intact header,
    /// and nothing after it is, it's the newest save, damaged. Otherwise
    /// the rest of the file is searched, see [`search`].
    fn damaged(
        &mut self,
        bytes: &[u8],
        ty: FileType,
        at: usize,
        mut sums: Vec<u128>,
        stepped: Option<(u64, u64)>,
    ) {
        if !framed_from(bytes, at, ty) {
            self.end = End::Torn;
            return;
        }
        self.stepped_over(stepped);
        // `usize` is at most 64 bits wide, so exact.
        self.damage.get_or_insert(at as u64);
        let prev = *sums.last().unwrap();
        if let Some(damaged) = header_at(bytes, at, prev)
            && framed_record(bytes, &damaged, ty)
            // Within `bytes`, so exact.
            && !framed_from(bytes, damaged.end as usize, ty)
        {
            self.unreadable += damaged.end - damaged.start;
            self.end = End::NewestDamaged(damaged);
            return;
        }
        // What follows on from the chain's last block may be its own.
        sums.pop();
        let (newest, kept) = search(bytes, at, self.id, ty, &sums);
        // `usize` is at most 64 bits wide, and the blocks kept are within
        // what was searched, so exact.
        self.unreadable += (bytes.len() - at) as u64 - kept;
        self.end = End::Searched(newest);
    }

    /// The record to open, and what it holds, read from `bytes`, all of the
    /// file, with how reading found the file: the newest record the chain
    /// proves, or the search's newest if it proves none. `None` if there's
    /// none and nothing is damaged; none with damage is
    /// [`Error::Corrupt`].
    pub(super) fn open<P: Payload>(&self, bytes: &[u8]) -> Result<Option<Opened<P>>> {
        let found = match self.end {
            End::Searched(found) => found,
            _ => None,
        };
        let Some(record) = self.records.last().copied().or(found) else {
            return match self.damage {
                Some(offset) => Err(Error::Corrupt { offset }),
                None => Ok(None),
            };
        };
        let (payload, time) = decode_record(bytes, &record)?;
        let outcome = match self.end {
            End::Searched(found) => Outcome::Damaged {
                found: found
                    .filter(|found| found.start != record.start)
                    .map(|found| FoundRecord {
                        tail: found.tail(),
                        time: record_time(bytes, &found),
                    }),
            },
            End::NewestDamaged(damaged) => Outcome::NewestDamaged(damaged),
            _ if self.bridged => Outcome::Bridged,
            End::Torn => Outcome::TornTail,
            End::File => Outcome::Intact,
        };
        Ok(Some(Opened {
            payload,
            id: self.id,
            // `None` with no record in the chain, opening the search's
            // newest, which is never saved to.
            before: self.before_newest,
            tail: record.tail(),
            report: Report {
                outcome,
                time,
                unreadable: self.unreadable,
            },
        }))
    }
}

impl Chain {
    /// The newest record the search found past damage, and what it holds,
    /// read from `bytes`, all of the file, if [`Chain::open`] opened
    /// another, the newest the chain proves, and it decodes: the file
    /// opened at it instead, as [`Outcome::Damaged`] with nothing else
    /// found.
    pub(super) fn open_found<P: Payload>(&self, bytes: &[u8]) -> Option<Opened<P>> {
        let End::Searched(Some(found)) = self.end else {
            return None;
        };
        // Otherwise the search's newest is what `open` opened.
        self.records.last()?;
        let (payload, time) = decode_record(bytes, &found).ok()?;
        Some(Opened {
            payload,
            id: self.id,
            // Never saved to, see [`Outcome::Damaged`].
            before: None,
            tail: found.tail(),
            report: Report {
                outcome: Outcome::Damaged { found: None },
                time,
                unreadable: self.unreadable,
            },
        })
    }
}

/// The intact block at `at` in `bytes`, all of a file with `id`, by its
/// first `len`, if there is one.
fn intact_at(bytes: &[u8], at: usize, id: u128) -> Option<Block> {
    let len = bytes
        .get(at..at.checked_add(8)?)
        .map(|len| u64_at(len, 0))?;
    let size = usize::try_from(len.checked_add(BLOCK_OVERHEAD)?).ok()?;
    // `usize` is at most 64 bits wide, so exact.
    intact(bytes.get(at..at.checked_add(size)?)?, at as u64, id)
}

/// The block at `at` in `bytes`, if its header is intact, whether or not
/// the block is: its first `len` within bounds, see [`stored_at`], the
/// block within `bytes`, and its `prev` equal to `prev`, the `sum` of the
/// block before.
fn header_at(bytes: &[u8], at: usize, prev: u128) -> Option<DamagedRecord> {
    // `usize` is at most 64 bits wide, so exact.
    let (start, len) = (at as u64, bytes.len() as u64);
    // A slice fails to read only past its end, which `stored_at` checks.
    let stored = stored_at(bytes, start, len).ok()??;
    let end = stored.end.filter(|&end| end <= len)?;
    (stored.prev == prev).then_some(DamagedRecord {
        start,
        end,
        sum: stored.sum,
    })
}

/// Whether `block`, a block by its header in `bytes`, is framed like a
/// record of a file of type `ty`: its kind that of the records, and its
/// trailing `len` its first.
fn framed_record(bytes: &[u8], block: &DamagedRecord, ty: FileType) -> bool {
    // Within `bytes`, and at least a block's overhead long, so exact.
    let end = block.end as usize;
    let len = block.end - block.start - BLOCK_OVERHEAD;
    u64_at(bytes, end - BACK_LEN) == len && bytes[end - 16..end] == ty.record()
}

/// Whether anything in `bytes` from `from` on is framed like a record of
/// a file of type `ty`: the kind of its records, with a trailing `len`
/// leading back to the start of a block, at or after `from`, whose first
/// `len` is the same.
pub(super) fn framed_from(bytes: &[u8], from: usize, ty: FileType) -> bool {
    let kind = ty.record();
    let Some(rest) = bytes.get(from..) else {
        return false;
    };
    rest.windows(kind.len())
        .enumerate()
        // A kind ending a whole block after `from`.
        .skip(BLOCK_OVERHEAD as usize - kind.len())
        .filter(|(_, window)| *window == kind)
        .any(|(i, _)| {
            // Within `rest`: the kind is at `i`, after a block's overhead.
            let end = i + kind.len();
            let len = u64_at(rest, end - BACK_LEN);
            len.checked_add(BLOCK_OVERHEAD)
                .and_then(|size| usize::try_from(size).ok())
                .and_then(|size| end.checked_sub(size))
                .is_some_and(|start| u64_at(rest, start) == len)
        })
}

/// Searches `bytes`, all of a file of type `ty` with `id`, from `from`
/// on, where its chain ended in damage, for intact blocks. `past` holds the
/// `sum`s of the chain's blocks before its last, and the header's hash:
/// a block found following on from one of those is of the file's past, and
/// is rejected, with everything following on from it, see [`Search`].
/// Returns the newest record kept and the bytes the blocks kept take up.
///
/// Every place is a candidate whose two lengths agree, within bounds;
/// only those are hashed, in order, skipping those overlapping a block
/// found. Hashing stops once it would hash more than 4 times the file's
/// length, so a file crafted with many long candidates can't keep the IO
/// lane busy for long: the rest counts as damage.
fn search(
    bytes: &[u8],
    from: usize,
    id: u128,
    ty: FileType,
    past: &[u128],
) -> (Option<Block>, u64) {
    let mut search = Search {
        bytes,
        // `usize` is at most 64 bits wide, so exact.
        from: from as u64,
        past: past.iter().copied().collect(),
        found: HashMap::new(),
        walked: HashMap::new(),
    };
    // `usize` is at most 64 bits wide, so exact.
    let budget = (bytes.len() as u64).saturating_mul(4);
    let mut hashed = 0u64;
    let mut newest = None;
    let mut kept = 0;
    let mut at = from;
    while at < bytes.len() {
        let Some(block) = candidate(bytes, at) else {
            at += 1;
            continue;
        };
        // `usize` is at most 64 bits wide, so exact.
        hashed = hashed.saturating_add(block.len() as u64);
        if hashed > budget {
            break;
        }
        let Some(found) = intact(block, at as u64, id) else {
            at += 1;
            continue;
        };
        at += block.len();
        let of_past = search.of_past(found.start, found.prev);
        search.found.insert(found.end, found.sum);
        if of_past {
            search.past.insert(found.sum);
            continue;
        }
        kept += block.len() as u64;
        if found.kind == ty.record() {
            newest = Some(found);
        }
    }
    (newest, kept)
}

/// What a [`search`] knows of the blocks it found so far, to tell which
/// are of the file's past: what's left must chain among itself, after the
/// chain's last block.
struct Search<'a> {
    /// All of the file.
    bytes: &'a [u8],
    /// Where the search began, the chain's end.
    from: u64,
    /// The `sum`s of blocks of the file's past: the chain's before its last,
    /// the header's hash, and the blocks found that were rejected.
    past: HashSet<u128>,
    /// The `sum` of each block found, by where it ends.
    found: HashMap<u64, u128>,
    /// Whether what ends at a place, followed on from with a `prev`, is of
    /// the file's past, for the damaged blocks walked back over.
    walked: HashMap<(u64, u128), bool>,
}

impl Search<'_> {
    /// Whether a block found at `start` following on from `prev` is of the
    /// file's past. Each block is linked to whatever ends right before it,
    /// through its trailing `len`, back over damaged blocks whose stored
    /// `sum` the link holds:
    ///
    /// - following on from a block of the past is of the past;
    /// - so is an intact block found that doesn't follow on from the
    ///   intact block found right before it, as in the chain;
    /// - and so is one linked to a block starting before the search's
    ///   start: that block overlaps the chain, so it was overwritten, by a
    ///   shorter write after it was cut off, say, and what follows on from
    ///   it is of the past too.
    ///
    /// Reaching the chain's end, or a damaged block the link doesn't lead
    /// to, it can't be told, and the block is kept. Each damaged block is
    /// walked over once, so the search stays linear.
    fn of_past(&mut self, start: u64, prev: u128) -> bool {
        let (mut end, mut prev) = (start, prev);
        let mut walked = Vec::new();
        let past = loop {
            if self.past.contains(&prev) {
                break true;
            }
            if end <= self.from {
                break false;
            }
            if let Some(&sum) = self.found.get(&end) {
                break sum != prev;
            }
            if let Some(&past) = self.walked.get(&(end, prev)) {
                break past;
            }
            // A slice fails to read only past its end, and `end` is within
            // it. `FILE_HEADER_LEN` is small, so exact.
            let before = stored_before(self.bytes, end, FILE_HEADER_LEN as u64)
                .ok()
                .flatten();
            let Some((before, stored)) = before else {
                break false;
            };
            if before < self.from {
                break true;
            }
            if stored.sum != prev {
                break false;
            }
            walked.push((end, prev));
            (end, prev) = (before, stored.prev);
        };
        for link in walked {
            self.walked.insert(link, past);
        }
        past
    }
}

/// The block at `at` in `bytes` by its first `len`, if it passes the
/// checks that come before hashing it: both its lengths agree, within
/// [`MAX_BLOCK`] (the bound for every kind so far), and
/// long enough for its `tag_len`.
fn candidate(bytes: &[u8], at: usize) -> Option<&[u8]> {
    let front = bytes.get(at..at.checked_add(TAG_LEN_AT + 2)?)?;
    let len = u64_at(front, 0);
    let tag_len = u16::from_le_bytes([front[TAG_LEN_AT], front[TAG_LEN_AT + 1]]);
    if !(u64::from(tag_len) + 2..=MAX_BLOCK).contains(&len) {
        return None;
    }
    let size = usize::try_from(len + BLOCK_OVERHEAD).ok()?;
    let block = bytes.get(at..at.checked_add(size)?)?;
    (u64_at(block, size - BACK_LEN) == len).then_some(block)
}

/// When the record `record`, found intact in `bytes`, all of the file,
/// was written, if its payload decodes as far as that.
fn record_time(bytes: &[u8], record: &Block) -> Option<UnixSeconds> {
    let (_, payload) = record.parts(bytes);
    let raw = decompress(payload).ok()?;
    unchecked_record::<IgnoredAny>(&raw)
        .ok()
        .map(|record| record.time)
}
