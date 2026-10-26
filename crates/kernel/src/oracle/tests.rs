use glam::DVec3;
use varde_testkit::ray_oracle::Shift;
use varde_testkit::rng::Rng;

use super::families::{FAMILIES, KERNEL_EXACT, kernel_exact_kind};
use super::runs::{
    Checked, Counting, Flip, Limit, Outcome, Placement, Report, cause_name, check_answers,
    check_counted, check_with, counting, near_shift, oracle_faces, par, run, run_counted, truth,
    truths_near,
};
use super::solids::{Make, Shape, Solid, Turn};
use super::turned::TurnedEdges;
use super::world::{Answers, Cause, Config, Refusal, Stats};

type Counted = Result<Answers, Refusal>;

/// The number of placements of a run: `VARDE_ORACLE_RUNS` if set, else
/// `quick` (`full` with `VARDE_TESTS=full`).
fn placements(quick: u64, full: u64) -> u64 {
    match std::env::var("VARDE_ORACLE_RUNS") {
        Ok(n) => n.parse().expect("VARDE_ORACLE_RUNS is a count"),
        Err(_) => varde_testing::pick(quick, full),
    }
}

/// The family named `name`, or else the one family whose name holds it
/// (more than one is an error: "exact ties" is part of two names).
fn family_named(name: &str) -> fn(u64) -> Placement {
    if let Some(f) = FAMILIES.iter().find(|x| x.0 == name) {
        return f.1;
    }
    let found: Vec<_> = FAMILIES.iter().filter(|x| x.0.contains(name)).collect();
    match found[..] {
        [f] => f.1,
        [] => panic!("no family named {name:?}"),
        _ => panic!(
            "{name:?} is part of several families' names: {:?}",
            found.iter().map(|x| x.0).collect::<Vec<_>>()
        ),
    }
}

#[test]
fn families_are_found_by_a_unique_part_of_their_name() {
    let grid = family_named("half grid")(5);
    assert_eq!(family_named("exact ties: axis-aligned")(5), grid);
    assert_eq!(
        family_named(KERNEL_EXACT)(5),
        family_named("kernel-exact")(5)
    );
    assert_ne!(family_named(KERNEL_EXACT)(5), grid);
    let both = std::panic::catch_unwind(|| family_named("exact ties"));
    assert!(both.is_err(), "\"exact ties\" names two families");
}

fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// Whether runs counted only on request do the `±d` check
/// (`VARDE_ORACLE_FLIP` set): it counts each answered placement again.
fn flip_asked() -> bool {
    std::env::var("VARDE_ORACLE_FLIP").is_ok()
}

/// Random rigid placements of every pair of solid kinds
/// ([`Placement::random`]); with `VARDE_ORACLE_FLIP` the `±d` check too.
/// At most [`RANDOM_LIMIT`] of them may refuse.
#[test]
fn random_placements_agree_with_the_oracle() {
    let n = placements(1, 40);
    let r = run(n, threads(), &Config::RUNS, flip_asked(), &|i| {
        Placement::random(i)
    });
    eprintln!("{}{}", r.text(), r.against(RANDOM_LIMIT));
    assert!(r.mismatches.is_empty(), "{}", r.text());
    assert!(r.unresolved.is_empty(), "{}", r.text());
    if let Err(e) = r.within("random placements", RANDOM_LIMIT) {
        panic!("refusals over the limit: {e}");
    }
}

/// The random placements' refusal limit: 0.5%, asserted from 1000
/// placements.
const RANDOM_LIMIT: Limit = Limit {
    per_mille: 5,
    min: 1000,
};

/// The placements a family's refusal limit needs before it is asserted.
const FAMILY_MIN: u64 = 100;

/// Each adversarial family's refusal limit, in per mille of its
/// placements, by its name in [`FAMILIES`]: the measured rate rounded up
/// modestly, 1% (the resolution of a 100-placement sweep) where none
/// refused, none for the kernel-exact ties, whose answers alone check
/// the tie direction. The stacked half cylinders' 90% is a hold until
/// overlapping curved shadows are decided by the sign rule, then 10%.
const FAMILY_LIMITS: [(&str, u64); 14] = [
    ("vertex within the band of an edge", 100),
    ("vertex on a silhouette", 30),
    ("vertex on a curved face's tile line", 10),
    ("vertex at a plane lattice's origin", 10),
    ("folds inside a face (axis across UP)", 10),
    ("acute corner (2^-6 rad knife wedge)", 10),
    ("roof-ridge pyramid, vertex on the ridge", 10),
    ("cusp corners (arc and its tangents)", 10),
    ("shared corner (vertex on a vertex)", 10),
    ("overhanging cylinder half", 10),
    ("exact ties: axis-aligned boxes on a half grid", 10),
    ("trim through a plane lattice's origin", 10),
    ("stacked half cylinders (arcs on one circle)", 900),
    (KERNEL_EXACT, 0),
];

/// The refusal limit of the family `name`.
fn family_limit(name: &str) -> Limit {
    let per_mille = FAMILY_LIMITS
        .iter()
        .find(|l| l.0 == name)
        .unwrap_or_else(|| panic!("no refusal limit for the family {name:?}"))
        .1;
    Limit {
        per_mille,
        min: FAMILY_MIN,
    }
}

#[test]
fn every_family_has_one_refusal_limit() {
    for (name, _) in FAMILIES {
        let n = FAMILY_LIMITS.iter().filter(|l| l.0 == name).count();
        assert_eq!(n, 1, "{name:?} has {n} refusal limits");
    }
    assert_eq!(FAMILY_LIMITS.len(), FAMILIES.len());
}

/// A run of `n` placements, the first `refused` of them refused as
/// `causes` gives, the rest agreed.
fn refusing(n: u64, refused: u64, cause: fn(u64) -> Cause) -> Report {
    let mut r = Report::default();
    for i in 0..n {
        r.add(
            i,
            if i < refused {
                Outcome::Refused(cause(i), format!("at {i}"))
            } else {
                Outcome::Agreed { ties: 0 }
            },
        );
    }
    r
}

/// The limit check passes a family at or below its limit and fails one
/// above it, naming the family, its rate, the limit and its refusals by
/// cause; below the placements it needs it asserts nothing.
#[test]
fn a_family_over_its_refusal_limit_fails() {
    let name = "stacked half cylinders (arcs on one circle)";
    let cause = |i: u64| {
        if i % 3 == 0 {
            Cause::Height
        } else {
            Cause::Clusters(format!(
                "OutOfBudget {{ cause: Precision, at: Some(XBox {{ lo: [{i}, 0] }}) }}"
            ))
        }
    };
    let r = refusing(100, 9, cause);
    assert_eq!(r.within(name, family_limit(name)), Ok(()));
    let tight = Limit {
        per_mille: 90,
        ..family_limit(name)
    };
    assert_eq!(r.within(name, tight), Ok(()), "9% is within 9%");
    let lowered = Limit {
        per_mille: 80,
        ..tight
    };
    let e = r.within(name, lowered).expect_err("9% is over 8%");
    for part in [
        name,
        "refused 9/100 (9.0%)",
        "limit 8%",
        "Clusters(OutOfBudget) 6",
        "Height 3",
    ] {
        assert!(e.contains(part), "{e:?} doesn't say {part:?}");
    }
    let exact = family_limit(KERNEL_EXACT);
    let one = refusing(100, 1, cause);
    let e = one
        .within(KERNEL_EXACT, exact)
        .expect_err("one refusal is over 0");
    assert!(e.contains(KERNEL_EXACT), "{e}");
    let few = refusing(99, 99, cause);
    assert_eq!(few.within(KERNEL_EXACT, exact), Ok(()));
    assert!(few.against(exact).contains("not asserted"));
    let random = refusing(1000, 6, cause);
    assert!(random.within("random placements", RANDOM_LIMIT).is_err());
    assert_eq!(
        refusing(1000, 5, cause).within("random placements", RANDOM_LIMIT),
        Ok(())
    );
}

/// A refusal's cause is named by its kinds only, without the keys, boxes
/// or levels of its failure.
#[test]
fn causes_are_named_by_their_kinds() {
    for (c, name) in [
        (
            Cause::Clusters(
                "OutOfBudget { cause: Precision, at: Some(XBox { lo: [1, 2] }) }".into(),
            ),
            "Clusters(OutOfBudget)",
        ),
        (
            Cause::Clusters("Depth(XBox { exp: 52 })".into()),
            "Clusters(Depth)",
        ),
        (Cause::Clusters("Boxes".into()), "Clusters(Boxes)"),
        (
            Cause::Record {
                kind: "x",
                fail: "Escalated { level: 3 }".into(),
            },
            "Record(x: Escalated)",
        ),
        (
            Cause::OutOfBudget("Precision".into()),
            "OutOfBudget(Precision)",
        ),
        (
            Cause::Count("Piece(Ribbon { coedge: CoedgeId(4) })".into()),
            "Count(Piece(Ribbon))",
        ),
        (Cause::Count("Box(Tiles)".into()), "Count(Box(Tiles))"),
        (Cause::Shared("Unsettled".into()), "Shared(Unsettled)"),
        (
            Cause::Other("corners Pinch(3)".into()),
            "Other(corners Pinch)",
        ),
        (Cause::Signal("Outside".into()), "Signal(Outside)"),
        (Cause::Height, "Height"),
        (Cause::ZOrder, "ZOrder"),
    ] {
        assert_eq!(cause_name(&c), name);
    }
}

/// Replays one placement: `VARDE_ORACLE_SEED=<seed>` (and
/// `VARDE_ORACLE_FAMILY=<part of a family's name>` for an adversarial
/// one) prints the placement, the counting's answers, the oracle's
/// verdict and the `±d` check, and the oracle's answers (with its tie
/// shift and at `±2^-28·d`) whether or not the counting answered;
/// without the variable it does nothing.
#[test]
fn replay() {
    let Ok(seed) = std::env::var("VARDE_ORACLE_SEED") else {
        return;
    };
    let seed: u64 = seed.parse().expect("a seed");
    let p = match std::env::var("VARDE_ORACLE_FAMILY") {
        Ok(f) => family_named(&f)(seed),
        Err(_) => Placement::random(seed),
    };
    eprintln!("{p:?}");
    let mut stats = Stats::default();
    let c = check_with(&p, &Config::RUNS, &mut stats, true);
    eprintln!("{:?}\n{:?}\n{:?}", c.answers, c.outcome, c.flip);
    let (a, b) = (p.a.build(&p.make_a), p.b.build(&p.make_b));
    let (fa, fb) = (oracle_faces(&a), oracle_faces(&b));
    eprintln!("oracle: {:?}", truth(&a, &b, p.d, &fa, &fb));
    eprintln!(
        "oracle at ±2^-28·d: {:?}",
        truths_near(&a, &b, p.d, &fa, &fb)
    );
}

/// The adversarial families ([`super::families`]), each placement with
/// the `±d` check: `VARDE_TESTS=full` or `VARDE_ORACLE_RUNS` only (some
/// families take seconds per placement); `VARDE_ORACLE_FAMILY` picks the
/// families whose name holds it. A family fails on a mismatch, an
/// unresolved oracle, or refusals over its limit ([`FAMILY_LIMITS`],
/// from [`FAMILY_MIN`] placements); the report gives each family's
/// refusal rate against its limit.
#[test]
fn adversarial_families_agree_with_the_oracle() {
    let n = placements(0, 8);
    let only = std::env::var("VARDE_ORACLE_FAMILY").ok();
    let mut bad = Vec::new();
    let mut rates = Vec::new();
    for (name, family) in FAMILIES {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        let mut r = run(n, threads(), &Config::RUNS, true, &|i| family(i));
        if name == KERNEL_EXACT {
            r.kinds = Some(kernel_exact_kind);
        }
        let limit = family_limit(name);
        eprintln!("== {name}\n{}{}", r.text(), r.against(limit));
        rates.push(format!("{name}: {}", r.against(limit)));
        if !r.mismatches.is_empty() || !r.unresolved.is_empty() {
            bad.push(format!("{name}: mismatches or oracle unresolved"));
        }
        if let Err(e) = r.within(name, limit) {
            bad.push(format!("refusals over the limit, {e}"));
        }
    }
    eprintln!("== refusal rates against the limits\n{}", rates.join("\n"));
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// What a variant's answers keep of the base's: per operand the sorted
/// windings, and the sorted sizes of the non-zero crossing counts (an
/// edge stored the other way, from its other end's id, negates its
/// counts).
fn invariants(a: &Answers) -> ([Vec<i64>; 2], Vec<i64>) {
    let w = a.winding.clone().map(|mut v| {
        v.sort_unstable();
        v
    });
    let mut x: Vec<i64> = a
        .x12
        .iter()
        .chain(&a.x21)
        .flatten()
        .map(|x| x.abs())
        .filter(|&x| x != 0)
        .collect();
    x.sort_unstable();
    (w, x)
}

/// The same placements counted again with the faces' lattice levels
/// raised (re-tiling), with core boxes refined further (depth), with the
/// faces' boxes split in other directions (partition), with other
/// p-curve perturbations and vertex ids, and turned by exact quarter
/// turns about each axis: each variant checked against the oracle, and
/// its answers against the base's where both answered. `VARDE_TESTS=full`
/// or `VARDE_ORACLE_RUNS` only.
#[test]
fn counts_are_invariant() {
    let n = placements(0, 20);
    type Variant = (&'static str, fn(&Placement, u64) -> Placement, Config);
    let same = |p: &Placement, _: u64| p.clone();
    let mut deep = Config::RUNS;
    deep.eps_tie = 1e-8;
    let mut skew = Config::RUNS;
    skew.scale = [3.0, 1.0 / 3.0];
    let variants: [Variant; 8] = [
        (
            "re-tiling (curved levels +1, +1)",
            |p, _| {
                let mut p = p.clone();
                p.make_a.levels = [1, 1];
                p.make_b.levels = [1, 1];
                p
            },
            Config::RUNS,
        ),
        ("depth (core boxes to 1e-8 mm)", same, deep),
        ("partition (split lengths ×3, ×1/3)", same, skew),
        (
            "p-curve perturbation (other offsets)",
            |p, seed| {
                let mut p = p.clone();
                let mut rng = Rng::new(seed ^ 0x5eed);
                for m in [&mut p.make_a, &mut p.make_b] {
                    m.pert = rng.range(5e-7, 1e-6);
                    m.pert_seed = rng.next_u64();
                }
                p
            },
            Config::RUNS,
        ),
        (
            "vertex ids (other shuffles)",
            |p, seed| {
                let mut p = p.clone();
                let mut rng = Rng::new(seed ^ 0x1d5);
                p.make_a.ids = Some(rng.next_u64());
                p.make_b.ids = Some(rng.next_u64());
                p
            },
            Config::RUNS,
        ),
        (
            "quarter turn about x",
            |p, _| turn(p, &Turn::QUARTERS[0]),
            Config::RUNS,
        ),
        (
            "quarter turn about y",
            |p, _| turn(p, &Turn::QUARTERS[1]),
            Config::RUNS,
        ),
        (
            "quarter turn about z",
            |p, _| turn(p, &Turn::QUARTERS[2]),
            Config::RUNS,
        ),
    ];
    let mut text = String::new();
    let mut bad = false;
    let bases = par(n, threads(), &|i| {
        check_answers(&Placement::random(i), &Config::RUNS, &mut Stats::default())
    });
    for (name, f, cfg) in &variants {
        let (mut agreed, mut mismatch, mut refused, mut status, mut differ) = (0, 0, 0, 0, 0);
        let outs = par(n, threads(), &|i| {
            check_answers(&f(&Placement::random(i), i), cfg, &mut Stats::default())
        });
        for (i, ((o, ans), (bo, bans))) in outs.iter().zip(&bases).enumerate() {
            match o {
                Outcome::Agreed { .. }
                | Outcome::TieScale(_)
                | Outcome::OperandLobe(_)
                | Outcome::TurnedEdgeFaces(_)
                | Outcome::OracleOnlyFlip => agreed += 1,
                Outcome::Mismatch(m) => {
                    mismatch += 1;
                    text += &format!("{name}: seed {i}: MISMATCH {m}\n");
                }
                Outcome::Refused(..) => refused += 1,
                Outcome::Unresolved(u) => text += &format!("{name}: seed {i}: unresolved {u}\n"),
            }
            if matches!(o, Outcome::Refused(..)) != matches!(bo, Outcome::Refused(..)) {
                status += 1;
                text += &format!("{name}: seed {i}: base {bo:?}, variant {o:?}\n");
            }
            if let (Some(x), Some(y)) = (ans, bans)
                && invariants(x) != invariants(y)
            {
                differ += 1;
                text += &format!("{name}: seed {i}: answers differ from the base's\n");
            }
        }
        bad |= mismatch > 0 || differ > 0;
        text += &format!(
            "{name}: {n} placements, {agreed} agreed, {mismatch} mismatches, {refused} refused, {status} refusals differing from the base's, {differ} answers differing\n"
        );
    }
    eprintln!("{text}");
    assert!(!bad, "{text}");
}

/// `p` turned by `t`, both shapes and `A`'s direction.
fn turn(p: &Placement, t: &Turn) -> Placement {
    Placement {
        a: p.a.turned(t),
        b: p.b.turned(t),
        d: t.apply_v(p.d),
        ..p.clone()
    }
}

/// The oracle against the ideal shapes (exact forms, no rounding): on
/// every solid kind at random placements, the oracle's winding of each
/// vertex of one in the other equals the ideal inside test wherever the
/// vertex is clear of the other's surface by `10⁻⁶`.
#[test]
fn the_oracle_agrees_with_the_ideal_shapes() {
    let n = placements(2, 60);
    let checked: u64 = par(n, threads(), &|i| {
        let p = Placement::random(i);
        let (a, b) = (p.a.build(&p.make_a), p.b.build(&p.make_b));
        let (fa, fb) = (oracle_faces(&a), oracle_faces(&b));
        let t = truth(&a, &b, p.d, &fa, &fb).expect("settled");
        let mut k = 0;
        for (s, o, w) in [(&a, &p.b, &t.winding[0]), (&b, &p.a, &t.winding[1])] {
            for (v, &w) in s.verts().into_iter().zip(w) {
                if let Some(inside) = o.inside(s.brep.pos(v), 1e-6) {
                    assert_eq!(w, i64::from(inside), "seed {i}, {v:?}");
                    k += 1;
                }
            }
        }
        k
    })
    .into_iter()
    .sum();
    assert!(checked > 0);
}

/// Two plane boxes, `A` turned, scaled and moved at random to overlap
/// `B` and moving along a random direction, vertex ids shuffled, every
/// vertex clear of the other box by a gap (`4·10⁻³`): the counting
/// agrees with the oracle and with the ideal boxes, and each operand's
/// seed is cleared. The faces are planes on one huge tile each, whose
/// whole box holds every vertex near it: only its pieces clear them.
#[test]
fn moving_boxes_count_from_cleared_seeds() {
    const GAP: f64 = 4e-3;
    let mut rng = Rng::new(0x3a5);
    let (mut trials, mut skipped, mut inside) = (0, 0, 0);
    while trials < varde_testing::pick(3, 40) {
        let mut at = |e: f64| DVec3::new(rng.range(-e, e), rng.range(-e, e), rng.range(-e, e));
        let (at_b, at_a) = (at(0.5), at(1.5));
        let b = Shape::random(&mut rng, 0, at_b, 2.0);
        let a = Shape::random(&mut rng, 0, at_a, 2.0);
        let mut make = || Make {
            ids: Some(rng.next_u64()),
            ..Make::PLAIN
        };
        let (make_a, make_b) = (make(), make());
        let p = Placement {
            a,
            b,
            make_a,
            make_b,
            d: rng.direction(),
        };
        let (sa, sb) = (p.a.build(&p.make_a), p.b.build(&p.make_b));
        let ideal: Option<Vec<Vec<i64>>> = [(&sa, &p.b), (&sb, &p.a)]
            .into_iter()
            .map(|(s, o)| {
                s.verts()
                    .into_iter()
                    .map(|v| o.inside(s.brep.pos(v), GAP).map(i64::from))
                    .collect()
            })
            .collect();
        let Some(ideal) = ideal else {
            skipped += 1;
            assert!(skipped < 100, "too many placements near a tie");
            continue;
        };
        trials += 1;
        let mut stats = Stats::default();
        let (outcome, answers) = check_answers(&p, &Config::RUNS, &mut stats);
        assert!(
            matches!(outcome, Outcome::Agreed { .. }),
            "{p:?}: {outcome:?}"
        );
        let answers = answers.expect("answered");
        assert_eq!(answers.winding.to_vec(), ideal, "{p:?}");
        assert_eq!(
            (stats.cleared_seeds, stats.uncleared_seeds),
            (2, 0),
            "{p:?}"
        );
        inside += ideal.iter().flatten().filter(|&&w| w == 1).count();
    }
    assert!(inside > 0, "{skipped} skipped, none inside");
}

/// Placements the counting once refused, each since fixed: boxes split
/// past half the subdivision cap in each direction (seeds 3, 7, 8: a
/// point within a few hundredths of a face on a plane's one huge tile;
/// the cap counted the splits of a box, not the halvings in each
/// direction), an edge pair's cell whose inner side at a knot isn't
/// settled at the cell's size (seed 139), and boxes on a plane's huge
/// tile as deep as boxes go while still millimetres long, their splits
/// shifted off the point by fractions of boxes no longer aligned to the
/// dyadic grid (seeds 400, 1403, 1457, 1726), and a box side inside a
/// tile whose shadow no split position certified off the point, which
/// had no side record (seeds 2861, 8919). Seed 8 only by default (the
/// others take seconds each; varde-decide's own tests pin the rules).
#[test]
fn placements_once_refused_agree() {
    let seeds: &[u64] =
        varde_testing::pick(&[8], &[3, 7, 8, 139, 400, 1403, 1457, 1726, 2861, 8919]);
    let outs = par(seeds.len() as u64, threads(), &|i| {
        let seed = seeds[i as usize];
        let o = check_answers(
            &Placement::random(seed),
            &Config::RUNS,
            &mut Stats::default(),
        );
        (seed, o.0)
    });
    for (seed, o) in outs {
        assert!(matches!(o, Outcome::Agreed { .. }), "seed {seed}: {o:?}");
    }
}

/// Adversarial placements the runs once miscounted, each since fixed: a
/// vertex of `A` on an edge of `B` (exactly in the exact ties' seed 31,
/// at a rounded point of the edge in the band family's seed 84), whose
/// height against the edge read a rounded foot point a hair off the
/// exact foot, of the other sign or none; vertices within an edge's band
/// whose near pieces took the edge's height for the faces' (band seeds
/// 11, 66, 93, 97: a face's sheet on the other side of the vertex from
/// the edge; roof-ridge seeds 63 and 88: a face's tile and ribbon on
/// either side of it), now counted by their column hits' own heights;
/// roof-ridge seeds 2 and 44, a vertex `10⁻¹⁷` off the ridge, where the
/// ridge's stored curve (its middle control point a rounded midpoint)
/// bent off the chord the edge–edge records read, so the faces' crossings
/// split the other way (the edges are straight exactly since); band seed
/// 58, a vertex whose count against a cylinder's face read a height no
/// model gave, now its exact column hit's; shared-corner seed 0, edge
/// pairs at the shared corner whose crossing's z-order a stand-in once
/// read from the geometry, now their edge–edge height records;
/// shared-corner seeds 42 and 44, faces at the shared vertex whose tile
/// misses it by a rounding, its hit there counted by its own height
/// beside the ribbons' hits read by the vertex's germ (now all by the
/// germ). And placements once refused for want of precision: kernel-exact
/// seeds 14, 29, 34 and 39 (a corner of `A` exactly on an edge of the
/// cube, the near strip's cut next to it never certified off the ray
/// however small the boxes beside it, all clear of its height: those
/// join the near run once refined as far as they go) and half-grid seeds
/// 1, 4 and 7 (corner pieces next to a face's vertex).
/// Seed 31 only by default (each takes seconds).
#[test]
fn adversarial_placements_once_miscounted_agree() {
    let cases: &[(&str, u64)] = varde_testing::pick(
        &[("half grid", 31)],
        &[
            ("half grid", 31),
            ("band of an edge", 84),
            ("band of an edge", 11),
            ("band of an edge", 66),
            ("band of an edge", 93),
            ("band of an edge", 97),
            ("roof-ridge", 63),
            ("roof-ridge", 88),
            ("roof-ridge", 2),
            ("roof-ridge", 44),
            ("band of an edge", 58),
            ("shared corner", 0),
            ("shared corner", 42),
            ("shared corner", 44),
            (KERNEL_EXACT, 14),
            (KERNEL_EXACT, 29),
            (KERNEL_EXACT, 34),
            (KERNEL_EXACT, 39),
            ("half grid", 1),
            ("half grid", 4),
            ("half grid", 7),
        ],
    );
    let outs = par(cases.len() as u64, threads(), &|i| {
        let (family, seed) = cases[i as usize];
        let f = family_named(family);
        let o = check_answers(&f(seed), &Config::RUNS, &mut Stats::default()).0;
        (family, seed, o)
    });
    for (family, seed, o) in outs {
        assert!(
            matches!(o, Outcome::Agreed { .. }),
            "{family} seed {seed}: {o:?}"
        );
    }
}

/// Vertices on a curved face's tile line, near a silhouette, at a
/// cusp's corner, and under an overhanging or folding cylinder: their
/// counts read the curved faces' exact column hits (once refused for a
/// height no model gave), and each agrees with the oracle. Tile-line
/// seed 0 by default (seconds), more with `VARDE_TESTS=full`.
#[test]
fn curved_faces_count_by_their_column_hits() {
    let cases: &[(&str, u64)] = varde_testing::pick(
        &[("tile line", 0)],
        &[
            ("tile line", 0),
            ("tile line", 1),
            ("tile line", 2),
            ("tile line", 3),
            ("silhouette", 1),
            ("silhouette", 2),
            ("folds inside", 0),
            ("overhanging", 0),
            ("overhanging", 1),
        ],
    );
    let outs = par(cases.len() as u64, threads(), &|i| {
        let (family, seed) = cases[i as usize];
        let o = check_answers(
            &family_named(family)(seed),
            &Config::RUNS,
            &mut Stats::default(),
        )
        .0;
        (family, seed, o)
    });
    for (family, seed, o) in outs {
        assert!(
            matches!(o, Outcome::Agreed { .. }),
            "{family} seed {seed}: {o:?}"
        );
    }
}

/// A line span is straight only where its middle control point is the
/// exact midpoint: a rounded one bends the stored curve (which the
/// counting surface ends on) off the chord (which a straight edge's
/// records read), by up to half an ulp.
#[test]
fn a_rounded_midpoint_is_no_line() {
    use super::solids::{is_line, line, snap};
    // `a + b` exact: straight.
    assert!(is_line(&[line([0.5, 1.0, 2.0], [0.25, 3.0, -1.0])]));
    // `0.1 + 0.7` rounds: the midpoint is a hair off the chord.
    let (a, b) = ([0.1, 0.0, 0.0], [0.7, 0.0, 0.0]);
    assert_ne!(
        num_rational(0.1) + num_rational(0.7),
        num_rational(0.1 + 0.7)
    );
    assert!(!is_line(&[line(a, b)]));
    // Snapped, every pair's line is straight.
    let vs = snap(&[a, b, [-0.3, 1.0 / 3.0, 1.9], [1.7, -0.123, 0.456]]);
    for p in &vs {
        for q in &vs {
            assert!(is_line(&[line(*p, *q)]), "{p:?} {q:?}");
        }
    }
    // Snapping moves a coordinate by at most an ulp of the largest.
    for (p, q) in vs
        .iter()
        .zip([a, b, [-0.3, 1.0 / 3.0, 1.9], [1.7, -0.123, 0.456]])
    {
        for k in 0..3 {
            assert!((p[k] - q[k]).abs() <= 2.0 * f64::EPSILON, "{p:?} {q:?}");
        }
    }
}

/// Every edge of the polyhedra the families build is straight exactly.
#[test]
fn polyhedra_edges_are_straight_exactly() {
    for seed in 0..placements(8, 64) {
        for family in ["roof-ridge", "band of an edge", "shared corner"] {
            let p = family_named(family)(seed);
            for (shape, make) in [(&p.a, &p.make_a), (&p.b, &p.make_b)] {
                if !matches!(shape, Shape::Box { .. } | Shape::Poly { .. }) {
                    continue;
                }
                let s = shape.build(make);
                assert!(
                    s.straight.iter().all(|&x| x),
                    "{family} seed {seed}: {shape:?}"
                );
            }
        }
    }
}

fn num_rational(x: f64) -> malachite::Rational {
    malachite::Rational::try_from(x).expect("finite")
}

/// A shared corner whose count once ran out of precision: `B`'s vertex 3
/// on `A`'s vertex, against `A`'s plane face 4, a strip's cut next to the
/// vertex no level certified off the vertex's ray; the corner's piece now
/// takes its curves' near strip leaves, so its cuts lie past them (seed
/// 1). Shared-corner seeds 12, 46, 68, 71 and 98 and stacked seed 54: a
/// trim crossing a box side within an `f64` rounding of the side's end
/// next to the shared vertex, its place against the end once read from
/// the rounded root enclosure, now exact on the stored values.
/// Shared-corner seeds 12 and 43: boxes at the shared vertex refined to
/// the depth cap, or out of refining rounds, over a tile hit within
/// `ε_tie` of the vertex beside the ribbon's hit at it, now settled there
/// as the near piece reads them (by the vertex's germ). Half-grid seeds
/// 56 and 93, once out of refining rounds, too. `VARDE_TESTS=full` only
/// (minutes: the oracle's, on stacked seed 54).
#[test]
fn a_shared_corner_s_counts_settle() {
    let cases: &[(&str, u64)] = varde_testing::pick(
        &[],
        &[
            ("shared corner", 1),
            ("shared corner", 12),
            ("shared corner", 43),
            ("shared corner", 46),
            ("shared corner", 68),
            ("shared corner", 71),
            ("shared corner", 98),
            ("stacked", 54),
            ("half grid", 56),
            ("half grid", 93),
        ],
    );
    let outs = par(cases.len() as u64, threads(), &|i| {
        let (family, seed) = cases[i as usize];
        let o = check_answers(
            &family_named(family)(seed),
            &Config::RUNS,
            &mut Stats::default(),
        )
        .0;
        (family, seed, o)
    });
    for (family, seed, o) in outs {
        assert!(
            !matches!(&o, Outcome::Refused(..)),
            "{family} seed {seed}: {o:?}"
        );
    }
}

/// Shared corners once miscounted (seeds 31, 50 and 60) or refused for a
/// near tie (seed 1, its "shared" vertices an ulp apart before both
/// solids were snapped to one grid): a face whose tile misses the shared
/// vertex by a rounding reads its hit there by the vertex's germ with the
/// ribbons' hits at it, as the ideal face through the vertex is read. So
/// each answers as the oracle does past the misfit (at `2^-28·d` or
/// `−2^-28·d`): tie-scale where the oracle's tie shift, inside the
/// misfit, reads the rounding, or an operand lobe the rounding makes
/// there (seed 60) without a fallback, the counting's winding staying
/// in `{0, 1}`. `VARDE_TESTS=full` only (seconds each).
#[test]
fn shared_corners_count_as_the_ideal_faces() {
    let seeds: &[u64] = varde_testing::pick(&[], &[1, 31, 50, 60]);
    let outs = par(seeds.len() as u64, threads(), &|i| {
        let seed = seeds[i as usize];
        let p = family_named("shared corner")(seed);
        let c = check_with(&p, &Config::RUNS, &mut Stats::default(), true);
        (seed, c.outcome, c.flip)
    });
    for (seed, o, flip) in outs {
        assert!(
            matches!(
                &o,
                Outcome::Agreed { .. } | Outcome::TieScale(_) | Outcome::OperandLobe(_)
            ),
            "seed {seed}: {o:?}"
        );
        assert!(
            matches!(
                flip,
                Some(super::runs::Flip::Checked {
                    sides: [true, true],
                    ..
                })
            ),
            "seed {seed}: {flip:?}"
        );
    }
}

/// Stacked half cylinders (seed 18) where a straight edge of one meets an
/// arc of the other at a shared vertex: the counting decides the corner's
/// z-order by its orders (once refused), but the oracle's tie shift
/// leaves the coincident arcs degenerate. Settled past the misfit along
/// `d`, the oracle's answers equal the counting's: tie-scale, never an
/// unchecked answer. `VARDE_TESTS=full` only (minutes: the oracle runs
/// to its work cap at the tie shift).
#[test]
fn answers_the_oracle_settles_only_past_the_misfit_are_checked_there() {
    let cases: &[(&str, u64)] = varde_testing::pick(&[], &[("stacked", 18)]);
    for &(family, seed) in cases {
        let o = check_answers(
            &family_named(family)(seed),
            &Config::RUNS,
            &mut Stats::default(),
        )
        .0;
        assert!(
            matches!(&o, Outcome::TieScale(m) if m.contains("oracle unresolved")),
            "{family} seed {seed}: {o:?}"
        );
    }
}

/// Placements whose column passes within rounding of an edge, or
/// through it exactly: their boxes once ran out of precision (roof-ridge
/// seeds 9 and 15, a vertex `3·10⁻¹⁷` off the ridge) or read both faces
/// at a cube edge as holding the moved column (kernel-exact seeds 9, 19
/// and 24, a corner of `A` exactly on the edge); a plane face is now
/// counted by its exact hits where its boxes don't settle or disagree
/// with them, and a box holding a hit at `q`'s height stops at its last
/// split. Each agrees, the exact ones with `±d` too. `VARDE_TESTS=full`
/// only (seconds).
#[test]
fn columns_at_an_edge_count_by_their_hits() {
    let cases: &[(&str, u64)] = varde_testing::pick(
        &[],
        &[
            ("roof-ridge", 9),
            ("roof-ridge", 15),
            (KERNEL_EXACT, 9),
            (KERNEL_EXACT, 19),
            (KERNEL_EXACT, 24),
        ],
    );
    let outs = par(cases.len() as u64, threads(), &|i| {
        let (family, seed) = cases[i as usize];
        let c = check_with(
            &family_named(family)(seed),
            &Config::RUNS,
            &mut Stats::default(),
            family == KERNEL_EXACT,
        );
        (family, seed, c.outcome, c.flip)
    });
    for (family, seed, o, flip) in outs {
        assert!(
            matches!(o, Outcome::Agreed { .. }),
            "{family} seed {seed}: {o:?}"
        );
        if family == KERNEL_EXACT {
            assert!(
                matches!(
                    flip,
                    Some(super::runs::Flip::Checked {
                        sides: [true, true],
                        ..
                    })
                ),
                "{family} seed {seed}: {flip:?}"
            );
        }
    }
}

/// Ties exact in the counting's own records ([`KERNEL_EXACT`]): every
/// placement the counting answers agrees with the oracle (its tie shift
/// along `d`, no tie-scale allowance, no operand lobe), with `−d` it
/// agrees with the oracle at `−2^-28·d`, and the two counts differ
/// exactly where the oracle's answers at `±2^-28·d` do, some placements
/// flipping. Two placements by default (a corner of `A` on a face of
/// `B`, and the other way round; seconds each), 40 with
/// `VARDE_TESTS=full`.
#[test]
fn kernel_exact_ties_follow_the_direction() {
    let r = kernel_exact_run(counting);
    let text = r.text();
    eprintln!("{text}");
    let bad = kernel_exact_violations(&r);
    assert!(bad.is_empty(), "{bad:?}\n{text}");
    assert!(text.contains("answered by kind: "), "{text}");
}

/// The kernel-exact family's silhouette placements (seeds `7 mod 10`): a
/// corner of `A` exactly on an exact half cylinder's face at the tile
/// corner nearest its silhouette, at angle levels 0, 3 and 5. Each
/// agrees with the oracle (no tie-scale allowance), and its counts with
/// `d` and `−d` differ, as the oracle's at `±2^-28·d` do, each equal to
/// the oracle's on its side. Seed 7 by default (seconds), ten with
/// `VARDE_TESTS=full`.
#[test]
fn kernel_exact_silhouettes_flip_with_the_direction() {
    let seeds: Vec<u64> = (0..varde_testing::pick(1, 10))
        .map(|i| 7 + 10 * i)
        .collect();
    let family = family_named(KERNEL_EXACT);
    let outs = par(seeds.len() as u64, threads(), &|i| {
        let seed = seeds[i as usize];
        assert_eq!(kernel_exact_kind(seed), "corner of A at a silhouette of B");
        let c = check_with(&family(seed), &Config::RUNS, &mut Stats::default(), true);
        (seed, c)
    });
    for (seed, c) in outs {
        assert!(
            matches!(c.outcome, Outcome::Agreed { .. }),
            "seed {seed}: {c:?}"
        );
        assert!(
            matches!(
                c.flip,
                Some(Flip::Checked {
                    counted: true,
                    truth: true,
                    sides: [true, true],
                })
            ),
            "seed {seed}: {c:?}"
        );
    }
}

/// The `±2^-28·d` shift is relative to the data: `d` at max-norm 1,
/// `2^-28` of the binade of the largest coordinate.
#[test]
fn the_near_shift_is_relative_to_the_data() {
    let p = family_named(KERNEL_EXACT)(0);
    let (a, b) = (p.a.build(&p.make_a), p.b.build(&p.make_b));
    // The cube `[0, 2]³` holds the largest coordinate, 2.
    let d = DVec3::new(3.0, -4.0, 0.0);
    assert_eq!(near_shift(&a, &b, d), Shift::always([0.75, -1.0, 0.0], -27));
    let big = |s: &Shape| match s {
        Shape::Box { o, axes } => Shape::Box {
            o: o.map(|x| x * 1024.0),
            axes: axes.map(|v| v.map(|x| x * 1024.0)),
        },
        _ => unreachable!(),
    };
    let (a, b) = (big(&p.a).build(&p.make_a), big(&p.b).build(&p.make_b));
    assert_eq!(near_shift(&a, &b, d), Shift::always([0.75, -1.0, 0.0], -17));
}

/// The kernel-exact family's run ([`kernel_exact_ties_follow_the_direction`])
/// counted by `count`.
fn kernel_exact_run(count: Counting) -> Report {
    let n = placements(2, 40);
    let family = family_named(KERNEL_EXACT);
    let mut r = run_counted(n, threads(), &Config::RUNS, true, count, &|i| family(i));
    r.kinds = Some(kernel_exact_kind);
    r
}

/// What the kernel-exact family's check finds wrong with a run.
fn kernel_exact_violations(r: &Report) -> Vec<&'static str> {
    let mut bad = Vec::new();
    for (x, what) in [
        (r.mismatches.is_empty(), "mismatches"),
        (r.unresolved.is_empty(), "oracle unresolved"),
        (r.tie_scale.is_empty(), "tie-scale results"),
        (r.operand_lobe.is_empty(), "operand lobes"),
    ] {
        if !x {
            bad.push(what);
        }
    }
    let Some(f) = &r.flip else {
        bad.push("no ±d check");
        return bad;
    };
    for (x, what) in [
        (
            f.differ.is_empty(),
            "±d: flips where the oracle doesn't, or the other way",
        ),
        (f.unresolved.is_empty(), "±d: oracle unresolved"),
        (
            f.sides == f.both,
            "±d: a side differs from the oracle's on it",
        ),
        (f.counted > 0, "±d: no placement flips"),
    ] {
        if !x {
            bad.push(what);
        }
    }
    bad
}

/// The kernel-exact family's check catches a deliberately broken
/// counting: one moving `A` along `−d` (its answers are the oracle's on
/// the other side: tie-scale elsewhere, refused here), one ignoring `d`
/// (it never flips where the oracle does), and one off by one in a
/// winding (a mismatch).
#[test]
fn the_kernel_exact_check_catches_a_broken_counting() {
    fn backwards(a: &Solid, b: &Solid, d: DVec3, cfg: &Config, st: &mut Stats) -> Counted {
        counting(a, b, -d, cfg, st)
    }
    fn fixed(a: &Solid, b: &Solid, _: DVec3, cfg: &Config, st: &mut Stats) -> Counted {
        counting(a, b, DVec3::new(0.48, -0.6, 0.64), cfg, st)
    }
    fn off_by_one(a: &Solid, b: &Solid, d: DVec3, cfg: &Config, st: &mut Stats) -> Counted {
        let mut c = counting(a, b, d, cfg, st)?;
        c.winding[0][0] += 1;
        Ok(c)
    }
    let cases: [(&str, Counting, &[&str]); 3] = [
        (
            "backwards",
            backwards,
            &[
                "tie-scale results",
                "±d: a side differs from the oracle's on it",
            ],
        ),
        (
            "fixed direction",
            fixed,
            &[
                "±d: flips where the oracle doesn't, or the other way",
                "±d: a side differs from the oracle's on it",
                "±d: no placement flips",
            ],
        ),
        ("off by one", off_by_one, &["mismatches"]),
    ];
    for (name, count, want) in cases {
        let r = kernel_exact_run(count);
        let bad = kernel_exact_violations(&r);
        for w in want {
            assert!(bad.contains(w), "{name}: {bad:?}\n{}", r.text());
        }
    }
}

/// Placements whose oracle answers once ran past the shifted count's
/// box cap: in the exact-ties grid's seed 0 two boxes share a corner,
/// and an edge from it, moved by the tie shift, crosses the other's face
/// within `2^-512` of its end and of the face's corner (about a million
/// boxes in one count). `VARDE_TESTS=full` only (minutes).
#[test]
fn the_oracle_settles_placements_it_once_gave_up_on() {
    let cases: &[(&str, u64)] = varde_testing::pick(&[], &[("half grid", 0)]);
    for &(family, seed) in cases {
        let f = family_named(family);
        let p = f(seed);
        let (a, b) = (p.a.build(&p.make_a), p.b.build(&p.make_b));
        let (fa, fb) = (oracle_faces(&a), oracle_faces(&b));
        let t = truth(&a, &b, p.d, &fa, &fb);
        assert!(t.is_ok(), "{family} seed {seed}: {t:?}");
    }
}

/// Adversarial placements at an operand lobe: the oracle's winding of a
/// vertex lies outside `{0, 1}` (the faces at an edge overshoot each
/// other by their perturbed p-curves' seam, so their counting surfaces
/// aren't embedded at the vertex), the counting takes the lobe fallback
/// there and answers (roof ridge seed 82). `VARDE_TESTS=full` only
/// (seconds each).
#[test]
fn operand_lobes_take_the_fallback() {
    let cases: &[(&str, u64)] = varde_testing::pick(&[], &[("roof-ridge", 82)]);
    let outs = par(cases.len() as u64, threads(), &|i| {
        let (family, seed) = cases[i as usize];
        let f = family_named(family);
        check_answers(&f(seed), &Config::RUNS, &mut Stats::default())
    });
    for ((family, seed), (o, answers)) in cases.iter().zip(outs) {
        assert!(
            matches!(o, Outcome::OperandLobe(_)),
            "{family} seed {seed}: {o:?}"
        );
        let lobes = answers.map_or(0, |a| a.lobes);
        assert!(lobes > 0, "{family} seed {seed}: no lobe fallback");
    }
}

/// A cube edge in a key plane crossing an edge of the turned box inside
/// it (kernel-exact seed 42: the box's edge in the cube face's plane
/// leaves the face across the cube's edge): the crossing lies on the
/// turned faces' misfit-width ribbons, so its per-face counts in `x21`
/// are tie-scale and only their sum over the box edge's two faces is
/// checked. The counting agrees, with `±d` too; a counting moving the
/// crossing to the edge's other face is reported tie-scale per face and
/// still agrees; one changing the sum, or moving a crossing between two
/// faces elsewhere, is a mismatch.
#[test]
fn crossed_turned_edges_are_checked_by_their_sum() {
    const SEED: u64 = 42;
    assert_eq!(kernel_exact_kind(SEED), "edge of A in a key plane");
    let found = crossed_turned_edges(SEED + 1);
    let (_, turned) = found.last().expect("a crossed turned edge");
    let (g, group) = turned
        .x21
        .iter()
        .enumerate()
        .find_map(|(g, gs)| gs.first().map(|x| (g, x.clone())))
        .expect("in x21");
    assert_eq!((g, group.len()), (3, 2), "{turned:?}");
    assert!(turned.x12.iter().all(Vec::is_empty), "{turned:?}");
    assert_eq!(found.last().map(|x| x.0), Some(SEED));

    /// The groups of the placement's crossed turned edges.
    fn groups(a: &Solid, b: &Solid) -> TurnedEdges {
        TurnedEdges::of(a, b, true)
    }
    /// The first crossing count of row `g` and face `f`'s neighbour
    /// `h`, with `f` and `h` in one group (`within`) or not.
    fn pick(a: &Solid, b: &Solid, within: bool) -> (usize, usize, usize) {
        let t = groups(a, b);
        let (g, x) = t
            .x21
            .iter()
            .enumerate()
            .find_map(|(g, gs)| gs.first().map(|x| (g, x.clone())))
            .expect("a group");
        if within {
            (g, x[0], x[1])
        } else {
            let others: Vec<usize> = (0..a.faces().len()).filter(|f| !x.contains(f)).collect();
            (g, others[0], others[1])
        }
    }
    fn moved(a: &Solid, b: &Solid, d: DVec3, cfg: &Config, st: &mut Stats) -> Counted {
        let mut c = counting(a, b, d, cfg, st)?;
        let (g, f, h) = pick(a, b, true);
        c.x21[g][f] -= 1;
        c.x21[g][h] += 1;
        Ok(c)
    }
    fn summed(a: &Solid, b: &Solid, d: DVec3, cfg: &Config, st: &mut Stats) -> Counted {
        let mut c = counting(a, b, d, cfg, st)?;
        let (g, f, _) = pick(a, b, true);
        c.x21[g][f] += 1;
        Ok(c)
    }
    fn elsewhere(a: &Solid, b: &Solid, d: DVec3, cfg: &Config, st: &mut Stats) -> Counted {
        let mut c = counting(a, b, d, cfg, st)?;
        let (g, f, h) = pick(a, b, false);
        c.x21[g][f] -= 1;
        c.x21[g][h] += 1;
        Ok(c)
    }
    let cases: [Counting; 4] = [counting, moved, summed, elsewhere];
    let p = family_named(KERNEL_EXACT)(SEED);
    let outs: Vec<Checked> = par(4, threads(), &|i| {
        check_counted(
            &p,
            &Config::RUNS,
            &mut Stats::default(),
            i < 2,
            cases[i as usize],
        )
    });
    let flip_ok = |c: &Checked| {
        matches!(
            c.flip,
            Some(Flip::Checked {
                counted: true,
                truth: true,
                sides: [true, true],
            })
        )
    };
    let [real, moved, summed, elsewhere] = &outs[..] else {
        unreachable!()
    };
    assert_eq!(real.turned, 1);
    assert!(
        matches!(
            real.outcome,
            Outcome::Agreed { .. } | Outcome::TurnedEdgeFaces(_)
        ),
        "{real:?}"
    );
    assert!(flip_ok(real), "{real:?}");
    let Outcome::TurnedEdgeFaces(m) = &moved.outcome else {
        panic!("{moved:?}")
    };
    assert!(m.contains(&format!("x21 e{g} f")), "{m}");
    assert!(flip_ok(moved), "{moved:?}");
    let Outcome::Mismatch(m) = &summed.outcome else {
        panic!("{summed:?}")
    };
    assert!(
        m.contains(&format!("x21 e{g} faces {group:?} summed")),
        "{m}"
    );
    assert!(
        matches!(elsewhere.outcome, Outcome::Mismatch(_)),
        "{elsewhere:?}"
    );
    // The report counts the per-face tie-scale result apart and the
    // checked sums.
    let mut r = Report::default();
    for (i, c) in outs.iter().take(2).enumerate() {
        r.add_checked(i as u64, c.clone());
    }
    assert_eq!((r.turned_checked, r.turned_edge.len()), (2, 1));
    assert!(r.mismatches.is_empty() && r.tie_scale.is_empty());
}

/// Vertices of `A` on a silhouette of `B`'s half sphere zone (seeds 1,
/// 3, 5, 7; on it, `10⁻⁹` above and below it, `10⁻⁶` above it along
/// `UP`): the upward ray grazes the zone with a gap of about `4·10⁻¹³`,
/// not 0, below the `±2^-28·d` probe (about `10⁻⁹`), so the oracle's
/// answers at `±2^-28·d` differ while its answers at its tie shift don't
/// flip with `d`. The counting's answers with `d` and `−d` both equal
/// those: flip only in the oracle, gap below the probe, neither an
/// agreement nor a `±d` disagreement. Seed 1 by default (seconds), all
/// four with `VARDE_TESTS=full`.
#[test]
fn flips_below_the_probe_are_only_the_oracle_s() {
    let seeds: &[u64] = varde_testing::pick(&[1], &[1, 3, 5, 7]);
    let outs = par(seeds.len() as u64, threads(), &|i| {
        let seed = seeds[i as usize];
        let p = family_named("silhouette")(seed);
        let c = check_with(&p, &Config::RUNS, &mut Stats::default(), true);
        (seed, c)
    });
    let mut r = Report::default();
    for (seed, c) in outs {
        assert!(
            matches!(c.outcome, Outcome::OracleOnlyFlip),
            "seed {seed}: {c:?}"
        );
        assert!(
            matches!(c.flip, Some(Flip::OracleOnly)),
            "seed {seed}: {c:?}"
        );
        r.add_checked(seed, c);
    }
    let f = r.flip.as_ref().expect("the ±d check");
    assert_eq!(r.oracle_only, seeds);
    assert_eq!(f.oracle_only, seeds);
    assert_eq!((r.agreed, f.both), (0, 0));
    assert!(f.differ.is_empty() && r.mismatches.is_empty());
    assert!(r.text().contains("flip only in the oracle"), "{}", r.text());
}

/// The kernel-exact family's kinds follow its placements' geometry, and
/// the report gives the answered rate per kind.
#[test]
fn kernel_exact_kinds_are_reported_with_their_answered_rates() {
    let family = family_named(KERNEL_EXACT);
    let on = |x: f64| x == 0.0 || x == 2.0;
    for seed in 0..placements(20, 200) {
        let p = family(seed);
        let (a, b) = (p.a.build(&p.make_a), p.b.build(&p.make_b));
        let kind = kernel_exact_kind(seed);
        if kind == "corner of A at a silhouette of B" {
            assert!(silhouette_corner(&p, &a, &b), "seed {seed}: {kind}");
            continue;
        }
        let (turned, cube) = if kind == "corner of B on a face of A" {
            (&b, &a)
        } else {
            (&a, &b)
        };
        assert!(
            cube.verts()
                .iter()
                .all(|&v| cube.brep.pos(v).iter().all(|&x| on(x))),
            "seed {seed}: {kind}"
        );
        // How many of the turned box's coordinates lie on the cube's
        // planes, at its corner.
        let corner = turned
            .verts()
            .into_iter()
            .map(|v| turned.brep.pos(v).iter().filter(|&&x| on(x)).count())
            .max()
            .unwrap_or(0);
        // An edge of the turned box in a cube plane: both ends on it.
        let in_plane = turned.edges().into_iter().any(|e| {
            let [s, t] = turned.brep.ends(e).map(|v| turned.brep.pos(v));
            (0..3).any(|k| on(s[k]) && s[k] == t[k])
        });
        let want = match kind {
            "edge of A in a key plane" => corner >= 1 && in_plane,
            "corner of A on a face of B" | "corner of B on a face of A" => corner >= 1 && !in_plane,
            "corner of A on an edge of B" => corner >= 2 && !in_plane,
            "ray through an edge" => {
                // A corner whose upward ray meets an edge of the cube.
                let up = DVec3::new(2.0, 3.0, 32.0);
                !in_plane
                    && turned.verts().into_iter().any(|v| {
                        [1.0 / 32.0, 1.0 / 64.0].iter().any(|&s| {
                            let q = DVec3::from_array(turned.brep.pos(v)) + up * s;
                            let q = q.to_array();
                            q.iter().filter(|&&x| on(x)).count() == 2
                                && q.iter().all(|&x| (0.0..=2.0).contains(&x))
                        })
                    })
            }
            k => panic!("unknown kind {k:?}"),
        };
        assert!(
            want,
            "seed {seed}: {kind}, corner {corner}, in plane {in_plane}"
        );
    }
    let mut r = Report {
        kinds: Some(kernel_exact_kind),
        ..Report::default()
    };
    r.add(0, Outcome::Agreed { ties: 1 });
    r.add(
        5,
        Outcome::Refused(super::world::Cause::Height, String::new()),
    );
    r.add(1, Outcome::Agreed { ties: 1 });
    assert_eq!(
        r.by_kind(kernel_exact_kind),
        "answered by kind: corner of A on a face of B 1/2 (50.0%); corner of B on a face of A 1/1 (100.0%)\n"
    );
    assert!(r.text().contains("answered by kind: "));
}

/// Whether a silhouette placement of the kernel-exact family has `A`'s
/// corner exactly on a corner of `B`'s cylinder tiles at its level,
/// inside the face (between its rulings and caps), its tangent plane
/// there within `6°` of `UP` and within half a tile's angle at levels
/// past 0 (`45°·2^-L`).
fn silhouette_corner(p: &Placement, a: &Solid, b: &Solid) -> bool {
    let Shape::HalfCylinder { axis, z, .. } = &p.b else {
        return false;
    };
    let k = axis
        .iter()
        .position(|&c| c == 1.0)
        .expect("an axis along x or y");
    let Shape::Box { o: q, .. } = p.a else {
        return false;
    };
    if !a.verts().iter().any(|&v| a.brep.pos(v) == q) {
        return false;
    }
    let Some(f) = b.faces().into_iter().find(|&f| {
        matches!(
            b.brep.face_surface(f).lattice().key(),
            crate::geom::form::FormKey::Cylinder(_)
        )
    }) else {
        return false;
    };
    let s = b.brep.face_surface(f);
    let level = s.rect().level.u;
    let up = DVec3::new(2.0, 3.0, 32.0);
    let inner = (s.rect().u.start + 1)..s.rect().u.end;
    let on = inner.into_iter().find_map(|i| {
        let t = s.tile(i, 0)?;
        let c = t.point(0, 0);
        let same = (0..3).all(|j| j == k || c[j] == q[j]);
        let tu = DVec3::from_array(t.point(1, 0)) - DVec3::from_array(c);
        let m = tu.cross(DVec3::from_array(*axis));
        same.then(|| (m.dot(up) / (m.length() * up.length())).abs())
    });
    // The sine of the angle between the tangent plane and `UP`.
    let Some(graze) = on else {
        return false;
    };
    let mut deg = 6.0;
    if level > 0 && 45.0 / f64::from(1u32 << level) < deg {
        deg = 45.0 / f64::from(1u32 << level);
    }
    let limit = varde_decide::num::trig::sin(deg.to_radians());
    q[k] > z[0] && q[k] < z[1] && graze <= limit
}

/// The kernel-exact placements of seeds `0..n` whose cube edge crosses
/// an edge of the turned box exactly inside it ([`TurnedEdges`]).
fn crossed_turned_edges(n: u64) -> Vec<(u64, TurnedEdges)> {
    let family = family_named(KERNEL_EXACT);
    (0..n)
        .filter_map(|seed| {
            let p = family(seed);
            let (a, b) = (p.a.build(&p.make_a), p.b.build(&p.make_b));
            let t = TurnedEdges::of(&a, &b, p.unperturbed());
            (t.count() > 0).then_some((seed, t))
        })
        .collect()
}

/// Bodies the constructions build count from their store as the
/// oracle's solids do: a box and a cylinder from the primitives (linear
/// p-curves on the planes, iso lines on the walls, seams of one form
/// between the walls' halves) against a half cylinder and against each
/// other, at generic offsets, agree with the oracle.
#[test]
fn constructed_primitives_count_from_their_store() {
    use super::solids::is_line;
    use crate::build::primitives::{Primitive, build};
    use crate::infra::budget::{Budget, Work};
    use crate::infra::ops::{FeatureId, OpCtx, Tolerance};

    let built = |p: Primitive| {
        let eps = Tolerance::new(1e-5).expect("a tolerance");
        let brep = build(
            &p,
            OpCtx::new(FeatureId(1)),
            eps,
            &mut Work::new(&Budget::DEFAULT),
        )
        .expect("a primitive");
        let straight = (0..brep.edge_count() as u32)
            .map(|e| is_line(brep.edge_spans(crate::topo::brep::EdgeId(e))))
            .collect();
        // Per face its UV unit lengths: chords of its first tile.
        let scale = (0..brep.face_count() as u32)
            .map(|f| {
                let s = brep.face_surface(crate::topo::brep::FaceId(f));
                let r = s.rect();
                let net = s.tile(r.u.start, r.v.start).expect("a tile").net();
                let chord = |k: usize| {
                    let [a, b] = [net[0], net[k]].map(|p| DVec3::new(p[0], p[1], p[2]));
                    (b - a).length()
                };
                [chord(6), chord(2)]
            })
            .collect();
        Solid {
            brep,
            straight,
            scale,
        }
    };
    let cuboid = built(Primitive::Cuboid {
        min: [0.3125, -0.40625, -0.21875],
        size: [1.25, 0.6875, 0.5625],
    });
    let cylinder = built(Primitive::Cylinder {
        base: [0.53125, 0.15625, -0.65625],
        radius: 0.4375,
        height: 1.1875,
    });
    let half = Shape::HalfCylinder {
        point: [0.0, 0.0, 0.0],
        axis: [0.6, 0.0, 0.8],
        radius: 0.75,
        z: [-1.0, 1.0],
        first: 3,
    }
    .build(&Make::PLAIN);
    let d = DVec3::new(0.3, -0.7, 0.62);
    for (name, a, b) in [
        ("box, half cylinder", &cuboid, &half),
        ("half cylinder, cylinder", &half, &cylinder),
        ("box, cylinder", &cuboid, &cylinder),
    ] {
        let answers = counting(a, b, d, &Config::RUNS, &mut Stats::default())
            .unwrap_or_else(|r| panic!("{name}: refused {r:?}"));
        let t = truth(a, b, d, &oracle_faces(a), &oracle_faces(b)).expect("the oracle settles");
        assert_eq!(
            super::runs::compare(&answers, &t, &TurnedEdges::of(a, b, false)),
            None,
            "{name}"
        );
        assert!(
            answers.winding.iter().flatten().any(|&w| w == 1),
            "{name}: a vertex inside"
        );
    }
}
