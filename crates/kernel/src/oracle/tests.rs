use glam::DVec3;
use varde_testkit::ray_oracle::Shift;
use varde_testkit::rng::Rng;

use super::families::{FAMILIES, KERNEL_EXACT};
use super::runs::{
    Counting, Outcome, Placement, Report, Truth, by_edge_height, check_answers, check_with,
    counting, near_shift, oracle_faces, par, run, run_counted, truth, truths_near,
};
use super::solids::{Shape, Solid, Turn};
use super::world::{Answers, Config, Refusal, Stats};

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
#[test]
fn random_placements_agree_with_the_oracle() {
    let n = placements(1, 40);
    let r = run(n, threads(), &Config::RUNS, flip_asked(), &|i| {
        Placement::random(i)
    });
    eprintln!("{}", r.text());
    assert!(r.mismatches.is_empty(), "{}", r.text());
    assert!(r.unresolved.is_empty(), "{}", r.text());
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
/// families whose name holds it.
#[test]
fn adversarial_families_agree_with_the_oracle() {
    let n = placements(0, 8);
    let only = std::env::var("VARDE_ORACLE_FAMILY").ok();
    let mut bad = Vec::new();
    for (name, family) in FAMILIES {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        let r = run(n, threads(), &Config::RUNS, true, &|i| family(i));
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
                Outcome::Agreed { .. } | Outcome::TieScale(_) | Outcome::EdgeHeight(_) => {
                    agreed += 1
                }
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
        &[("half grid", 31)],
        &[("half grid", 31), ("band of an edge", 84)],
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

/// Ties exact in the counting's own records ([`KERNEL_EXACT`]): every
/// placement the counting answers agrees with the oracle (its tie shift
/// along `d`, no tie-scale or edge-height allowance), with `−d` it
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
    run_counted(n, threads(), &Config::RUNS, true, count, &|i| family(i))
}

/// What the kernel-exact family's check finds wrong with a run.
fn kernel_exact_violations(r: &Report) -> Vec<&'static str> {
    let mut bad = Vec::new();
    for (x, what) in [
        (r.mismatches.is_empty(), "mismatches"),
        (r.unresolved.is_empty(), "oracle unresolved"),
        (r.tie_scale.is_empty(), "tie-scale results"),
        (r.edge_height.is_empty(), "edge-height results"),
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

/// The edge-height class explains a difference from the oracle only at
/// a vertex whose layer of a face an edge's height decided: its winding
/// and column, and its edges' crossing counts with that face; anything
/// else stays a mismatch.
#[test]
fn edge_height_explains_only_its_vertex() {
    let p = family_named(KERNEL_EXACT)(0);
    let (a, b) = (p.a.build(&p.make_a), p.b.build(&p.make_b));
    let zeros = |n: usize, m: usize| vec![vec![0; m]; n];
    let (na, nb) = (a.verts().len(), b.verts().len());
    let (ea, eb) = (a.edges(), b.edges());
    let (fa, fb) = (a.faces().len(), b.faces().len());
    let t = Truth {
        winding: [vec![0; na], vec![0; nb]],
        x12: zeros(ea.len(), fb),
        x21: zeros(eb.len(), fa),
        ties: 0,
    };
    let mut base = Answers {
        winding: t.winding.clone(),
        column: t.winding.clone(),
        x12: t.x12.clone(),
        x21: t.x21.clone(),
        edge_height: [vec![vec![false; fb]; na], vec![vec![false; fa]; nb]],
    };
    // Vertex 0 of `A`: its layer of `B`'s face 1 decided by an edge.
    base.edge_height[0][0][1] = true;
    let from = |v: usize| {
        ea.iter()
            .position(|&e| a.brep.ends(e).iter().any(|u| u.index() == v))
    };
    let not_from = ea
        .iter()
        .position(|&e| a.brep.ends(e).iter().all(|u| u.index() != 0));
    let (e0, e1) = (
        from(0).expect("an edge from v0"),
        not_from.expect("an edge"),
    );
    type Mutation = (&'static str, fn(&mut Answers, usize, usize), bool);
    let cases: [Mutation; 7] = [
        ("winding at v0", |c, _, _| c.winding[0][0] = 1, true),
        ("column at v0", |c, _, _| c.column[0][0] = 1, true),
        (
            "x12 of an edge from v0, face 1",
            |c, e0, _| c.x12[e0][1] = 1,
            true,
        ),
        ("winding at v1", |c, _, _| c.winding[0][1] = 1, false),
        (
            "x12 of an edge from v0, face 0",
            |c, e0, _| c.x12[e0][0] = 1,
            false,
        ),
        (
            "x12 of an edge not from v0",
            |c, _, e1| c.x12[e1][1] = 1,
            false,
        ),
        ("winding of B's v0", |c, _, _| c.winding[1][0] = 1, false),
    ];
    assert!(by_edge_height(&base, &t, &a, &b));
    for (name, m, ok) in cases {
        let mut c = base.clone();
        m(&mut c, e0, e1);
        assert_eq!(by_edge_height(&c, &t, &a, &b), ok, "{name}");
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

/// Adversarial placements whose difference from the oracle is known:
/// a vertex within an edge's band whose layer the edge's height decides
/// though the face lies on the other side of it (band of an edge seed
/// 11, roof ridge seed 82, where the oracle's own answer with its tie
/// shift sits in the faces' rounding misfit and the edge-height rule
/// explains the difference from its answer at `2^-28·d`); and a vertex
/// on a ridge whose counting equals the oracle's at both `±2^-28·d`
/// while its answer with the tie shift, in the misfit, differs from
/// both (roof ridge seeds 63 and 88: tie-scale, not a mismatch; the
/// oracle's answer about the vertex changes only within about `2^-32`
/// of it, along `±d` and other directions alike, so the sheets meeting
/// there straddle it only within the misfit).
/// `VARDE_TESTS=full` only (seconds each).
#[test]
fn known_differences_are_classified() {
    type Case = (&'static str, u64, fn(&Outcome) -> bool);
    let cases: &[Case] = varde_testing::pick(
        &[],
        &[
            ("band of an edge", 11, |o| {
                matches!(o, Outcome::EdgeHeight(_))
            }),
            ("roof-ridge", 82, |o| matches!(o, Outcome::EdgeHeight(_))),
            ("roof-ridge", 63, |o| matches!(o, Outcome::TieScale(_))),
            ("roof-ridge", 88, |o| matches!(o, Outcome::TieScale(_))),
        ],
    );
    let outs = par(cases.len() as u64, threads(), &|i| {
        let (family, seed, _) = cases[i as usize];
        let f = family_named(family);
        check_answers(&f(seed), &Config::RUNS, &mut Stats::default()).0
    });
    for ((family, seed, ok), o) in cases.iter().zip(outs) {
        assert!(ok(&o), "{family} seed {seed}: {o:?}");
    }
}
