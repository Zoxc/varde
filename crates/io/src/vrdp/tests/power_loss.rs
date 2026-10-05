//! Power loss during a save: whatever subset of a save's writes since the
//! last sync lands, in whatever order, with stale bytes where they didn't,
//! the file opens, at the save acknowledged before or at the one that was
//! lost, never at a block of another file, and can be saved to again
//! unless reading found it damaged.

use super::*;

/// Storage over a file in memory that records what's done to it, for a
/// power loss to replay part of.
#[derive(Debug, Default)]
struct Recorded {
    file: Memory,
    ops: Vec<Op>,
}

/// What [`Recorded`] saw done to its file.
#[derive(Debug, Clone)]
enum Op {
    Truncate(u64),
    Write { at: u64, bytes: Vec<u8> },
    Sync,
}

impl ReadAt for Recorded {
    fn len(&self) -> io::Result<u64> {
        self.file.len()
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        self.file.read_at(buf, at)
    }
}

impl Storage for Recorded {
    fn write_at(&mut self, buf: &[u8], at: u64) -> io::Result<()> {
        self.ops.push(Op::Write {
            at,
            bytes: buf.to_vec(),
        });
        self.file.write_at(buf, at)
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.ops.push(Op::Truncate(len));
        self.file.truncate(len)
    }

    fn sync(&mut self) -> io::Result<()> {
        self.ops.push(Op::Sync);
        self.file.sync()
    }
}

/// A part of a save a power loss may keep or drop on its own: the
/// truncate, or a page of what it wrote.
#[derive(Debug, Clone)]
enum Unit {
    Truncate(u64),
    Page { at: u64, bytes: Vec<u8> },
}

/// The units of `ops`, a save's operations before its sync, writes split
/// into pages of `page` bytes.
fn units(ops: &[Op], page: usize) -> Vec<Unit> {
    let mut units = Vec::new();
    for op in ops {
        match op {
            Op::Truncate(len) => units.push(Unit::Truncate(*len)),
            Op::Write { at, bytes } => {
                for (i, chunk) in bytes.chunks(page).enumerate() {
                    units.push(Unit::Page {
                        at: at + (i * page) as u64,
                        bytes: chunk.to_vec(),
                    });
                }
            }
            Op::Sync => panic!("a sync among the units"),
        }
    }
    units
}

/// The file `durable`, as last synced, after a power loss that kept
/// `kept`, indices into `units`, applied in that order: where the file
/// grows, it holds what `stale` says was at each offset before.
fn lose_power(
    durable: &[u8],
    units: &[Unit],
    kept: &[usize],
    stale: &dyn Fn(u64) -> u8,
) -> Vec<u8> {
    let mut bytes = durable.to_vec();
    let grow = |bytes: &mut Vec<u8>, len: usize| {
        while bytes.len() < len {
            let at = bytes.len() as u64;
            bytes.push(stale(at));
        }
    };
    for &i in kept {
        match &units[i] {
            Unit::Truncate(len) => {
                bytes.truncate(*len as usize);
                grow(&mut bytes, *len as usize);
            }
            Unit::Page { at, bytes: page } => {
                let at = *at as usize;
                grow(&mut bytes, at + page.len());
                bytes[at..at + page.len()].copy_from_slice(page);
            }
        }
    }
    bytes
}

/// A small deterministic generator, for orders and samples.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn shuffle(&mut self, items: &mut [usize]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

/// A design file before the save that power is lost during, as last
/// synced, and what a save is to know of it.
struct Before {
    name: &'static str,
    durable: Vec<u8>,
    /// The tails of its saves, oldest first.
    tails: Vec<Tail>,
    /// The newest save acknowledged: the one that must open, unless the
    /// save after it does.
    acknowledged: (Document, Tail),
}

/// The files power is lost while saving to: a clean one, one with a torn
/// tail the save cuts off, one whose newest save is damaged with an intact
/// header, which the save goes after, and one of only a first save.
fn befores() -> Vec<Before> {
    let (bytes, tails) = saved(3);
    let clean = Before {
        name: "clean",
        durable: bytes.clone(),
        tails: tails.clone(),
        acknowledged: (edited(3), tails[2]),
    };
    let (record, _) = next(&bytes, tails[2], &edited(9));
    let torn = Before {
        name: "torn",
        durable: [&bytes[..], &record[..record.len() - 7]].concat(),
        tails: tails.clone(),
        acknowledged: (edited(3), tails[2]),
    };
    let damaged = Before {
        name: "newest damaged",
        durable: flip(&bytes, tails[2].last as usize + TAG_LEN_AT + 10),
        tails: tails[..2].to_vec(),
        acknowledged: (edited(2), tails[1]),
    };
    let (one, first) = saved(1);
    let first_only = Before {
        name: "first only",
        durable: one,
        tails: first.clone(),
        acknowledged: (edited(1), first[0]),
    };
    vec![clean, torn, damaged, first_only]
}

/// What was at each offset of a file before, where it grows.
type Stale = Box<dyn Fn(u64) -> u8>;

/// Where the file grows, what was there before: zeros, noise, the bytes of
/// another design file holding the same saves at the same offsets (as after
/// a Save As over the same path), another one holding others, and this
/// file's own bytes as last synced.
fn stales(durable: &[u8]) -> Vec<(&'static str, Stale)> {
    let (same, _) = saved(6);
    let mut others = Memory::default();
    let mut tail = write_new(&mut others, &edited(20), &[]).unwrap().tail();
    for n in [30, 5, 41, 2] {
        tail = save_to(&mut others, tail, &edited(n)).unwrap();
    }
    let others = others.bytes;
    let noise = noise(1 << 16, 11);
    let own = durable.to_vec();
    let at = |bytes: Vec<u8>| move |offset: u64| bytes.get(offset as usize).copied().unwrap_or(0);
    vec![
        ("zeros", Box::new(|_| 0)),
        ("noise", Box::new(at(noise))),
        ("another file of the same saves", Box::new(at(same))),
        ("another file", Box::new(at(others))),
        ("its own past", Box::new(at(own))),
    ]
}

/// What's saved to a file after power was lost.
static AFTER: std::sync::LazyLock<Document> = std::sync::LazyLock::new(|| edited(5));

/// Checks the file `bytes`, left by losing power during the save of
/// `document` to `before`, which would have made `lost` its tail.
fn check_after_power_loss(
    bytes: &[u8],
    before: &Before,
    document: &Document,
    lost: Tail,
    case: &str,
) {
    let opened = from_bytes_with_report(bytes).unwrap_or_else(|e| panic!("{case}: {e}"));
    let tail = opened.tail;
    let report = opened.report;
    let known = opened.known();
    // Ours: the save acknowledged, or the one lost.
    if tail == lost {
        assert_eq!(&opened.payload, document, "{case}");
    } else {
        let (acknowledged, at) = &before.acknowledged;
        assert_eq!(
            (&opened.payload, tail),
            (acknowledged, *at),
            "{case}: {report:?}"
        );
    }
    // Nothing of another file found by a search either.
    if let Outcome::Damaged { found: Some(found) } = report.outcome {
        assert!(
            found.tail == lost || before.tails.contains(&found.tail),
            "{case}: found {found:?}"
        );
    }

    // Saved to again, unless read as damaged.
    let mut file = Memory {
        bytes: bytes.to_vec(),
        syncs: 0,
    };
    let mut known = known;
    let after = &*AFTER;
    match save(&mut file, &mut known, after, &[]) {
        Ok(saved) => {
            assert!(!matches!(report.outcome, Outcome::Damaged { .. }), "{case}");
            let reopened = from_bytes_with_report(&file.bytes).unwrap();
            assert_eq!(
                (reopened.payload, reopened.tail),
                (after.clone(), saved),
                "{case}"
            );
            assert!(
                !matches!(reopened.report.outcome, Outcome::Damaged { .. }),
                "{case}: {:?}",
                reopened.report
            );
        }
        Err(Error::OpenedDamaged) => {
            assert!(matches!(report.outcome, Outcome::Damaged { .. }), "{case}");
        }
        Err(e) => panic!("{case}: {e:?} after {report:?}"),
    }
}

/// Runs a save of `document` to `before`, returning its operations before
/// the sync that would acknowledge it, and the tail it makes.
fn record_save(before: &Before, document: &Document) -> (Vec<Op>, Tail) {
    let mut file = Recorded {
        file: Memory {
            bytes: before.durable.clone(),
            syncs: 0,
        },
        ops: Vec::new(),
    };
    let mut known = from_bytes_with_report(&before.durable).unwrap().known();
    let tail = save(&mut file, &mut known, document, &[]).unwrap();
    let mut ops = file.ops;
    assert!(matches!(ops.pop(), Some(Op::Sync)));
    assert!(!ops.iter().any(|op| matches!(op, Op::Sync)));
    (ops, tail)
}

/// Every subset of a save of a few pages, in a random order, over every
/// file before and every kind of stale bytes.
///
/// About 0.6s in a debug build, over the suite's 0.5s aim: it is
/// exhaustive, and fewer subsets would leave cases unchecked.
#[test]
fn every_power_loss_during_a_short_save_opens() {
    let document = edited(4);
    let mut rng = Rng(0x5eed);
    let mut runs = 0;
    for before in befores() {
        let (ops, lost) = record_save(&before, &document);
        let written: usize = ops
            .iter()
            .map(|op| match op {
                Op::Write { bytes, .. } => bytes.len(),
                _ => 0,
            })
            .sum();
        let units = units(&ops, written.div_ceil(5));
        assert!(units.len() <= 7, "{}", units.len());
        for (stale_name, stale) in stales(&before.durable) {
            for subset in 0..1u32 << units.len() {
                let mut kept: Vec<usize> = (0..units.len())
                    .filter(|i| subset & (1 << i) != 0)
                    .collect();
                rng.shuffle(&mut kept);
                let bytes = lose_power(&before.durable, &units, &kept, &*stale);
                let case = format!("{}, {stale_name}, kept {kept:?}", before.name);
                check_after_power_loss(&bytes, &before, &document, lost, &case);
                runs += 1;
            }
        }
    }
    assert!(runs > 1000, "{runs}");
}

/// Samples of the subsets of a save in many small pages.
#[test]
fn sampled_power_losses_during_a_save_in_small_pages_open() {
    let document = edited(4);
    // Quick runs take 30 samples per case, full runs 150. Each case has a
    // generator of its own, so quick's samples are the first of full's.
    let samples = varde_testing::pick(30, 150);
    let mut case_seed = 0xfa11_u64;
    for before in befores() {
        let (ops, lost) = record_save(&before, &document);
        let units = units(&ops, 16);
        assert!(units.len() > 20);
        for (stale_name, stale) in stales(&before.durable) {
            case_seed = case_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut rng = Rng(case_seed | 1);
            for sample in 0..samples {
                // Each unit kept with a chance that varies by sample, so
                // that nearly whole and nearly empty saves come up.
                let chance = 1 + rng.below(9);
                let mut kept: Vec<usize> = (0..units.len())
                    .filter(|_| rng.below(10) < chance)
                    .collect();
                rng.shuffle(&mut kept);
                let bytes = lose_power(&before.durable, &units, &kept, &*stale);
                let case = format!("{}, {stale_name}, sample {sample}", before.name);
                check_after_power_loss(&bytes, &before, &document, lost, &case);
            }
        }
    }
}
