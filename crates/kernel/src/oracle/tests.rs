use varde_testkit::rng::Rng;

use super::runs::{Outcome, Placement, check_answers, oracle_faces, par, run, truth};
use super::solids::Turn;
use super::world::{Answers, Config, Stats};

/// The number of placements of a run: `VARDE_ORACLE_RUNS` if set, else
/// `quick` (`full` with `VARDE_TESTS=full`).
fn placements(quick: u64, full: u64) -> u64 {
    match std::env::var("VARDE_ORACLE_RUNS") {
        Ok(n) => n.parse().expect("VARDE_ORACLE_RUNS is a count"),
        Err(_) => varde_testing::pick(quick, full),
    }
}

fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// Random rigid placements of every pair of solid kinds ([`Placement::random`]).
#[test]
fn random_placements_agree_with_the_oracle() {
    let n = placements(1, 40);
    let r = run(n, threads(), &Config::RUNS, &|i| Placement::random(i));
    eprintln!("{}", r.text());
    assert!(r.mismatches.is_empty(), "{}", r.text());
    assert!(r.unresolved.is_empty(), "{}", r.text());
}

/// Replays one placement: `VARDE_ORACLE_SEED=<seed>` (and
/// `VARDE_ORACLE_FAMILY=<part of a family's name>` for an adversarial
/// one) prints the placement, the counting's answers and the oracle's
/// verdict; without the variable it does nothing.
#[test]
fn replay() {
    let Ok(seed) = std::env::var("VARDE_ORACLE_SEED") else {
        return;
    };
    let seed: u64 = seed.parse().expect("a seed");
    let p = match std::env::var("VARDE_ORACLE_FAMILY") {
        Ok(f) => {
            let (_, family) = super::families::FAMILIES
                .iter()
                .find(|x| x.0.contains(f.as_str()))
                .expect("a family");
            family(seed)
        }
        Err(_) => Placement::random(seed),
    };
    eprintln!("{p:?}");
    let mut stats = Stats::default();
    let (o, answers) = check_answers(&p, &Config::RUNS, &mut stats);
    eprintln!("{answers:?}\n{o:?}");
}

/// The adversarial families ([`super::families`]): `VARDE_TESTS=full`
/// or `VARDE_ORACLE_RUNS` only (some families take seconds per
/// placement); `VARDE_ORACLE_FAMILY` picks the families whose name holds
/// it.
#[test]
fn adversarial_families_agree_with_the_oracle() {
    let n = placements(0, 8);
    let only = std::env::var("VARDE_ORACLE_FAMILY").ok();
    let mut bad = Vec::new();
    for (name, family) in super::families::FAMILIES {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        let r = run(n, threads(), &Config::RUNS, &|i| family(i));
        eprintln!("== {name}\n{}", r.text());
        if !r.mismatches.is_empty() || !r.unresolved.is_empty() {
            bad.push(name);
        }
    }
    assert!(bad.is_empty(), "mismatches in {bad:?}");
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
                Outcome::Agreed { .. } | Outcome::TieScale(_) => agreed += 1,
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

/// Placements the counting once refused, each since fixed: boxes split
/// past half the subdivision cap in each direction (seeds 3, 7, 8: a
/// point within a few hundredths of a face on a plane's one huge tile;
/// the cap counted the splits of a box, not the halvings in each
/// direction), and an edge pair's cell whose inner side at a knot isn't
/// settled at the cell's size (seed 139). Seed 8 only by default (the
/// others take seconds each; varde-decide's own tests pin both rules).
#[test]
fn placements_once_refused_agree() {
    let seeds: &[u64] = varde_testing::pick(&[8], &[3, 7, 8, 139]);
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

/// Adversarial placements the runs once miscounted, each since fixed in
/// the runs' height model (a stand-in for the store-backed one): a
/// vertex of `A` on an edge of `B` (exactly in the exact ties' seed 31,
/// at a rounded point of the edge in the band family's seed 84), whose
/// height against the edge read a rounded foot point a hair off the
/// exact foot, of the other sign or none (a tie, decided by `A`'s
/// motion). Seed 31 only by default (each takes seconds).
#[test]
fn adversarial_placements_once_miscounted_agree() {
    let cases: &[(&str, u64)] = varde_testing::pick(
        &[("exact ties", 31)],
        &[("exact ties", 31), ("band of an edge", 84)],
    );
    let outs = par(cases.len() as u64, threads(), &|i| {
        let (family, seed) = cases[i as usize];
        let (_, f) = super::families::FAMILIES
            .iter()
            .find(|x| x.0.contains(family))
            .expect("a family");
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
