use glam::DVec3;
use varde_testkit::ray_oracle::Shift;
use varde_testkit::rng::Rng;

use super::families::{FAMILIES, KERNEL_EXACT};
use super::runs::{
    Counting, Outcome, Placement, Report, check_answers, check_with, counting, near_shift,
    oracle_faces, par, run, run_counted, truth, truths_near,
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
                Outcome::Agreed { .. } | Outcome::TieScale(_) | Outcome::OperandLobe(_) => {
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
/// read from the geometry, now their edge–edge height records.
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
/// takes its curves' near strip leaves, so its cuts lie past them.
/// `VARDE_TESTS=full` only (seconds).
#[test]
fn a_shared_corner_s_counts_settle() {
    let cases: &[(&str, u64)] = varde_testing::pick(&[], &[("shared corner", 1)]);
    for &(family, seed) in cases {
        let o = check_answers(
            &family_named(family)(seed),
            &Config::RUNS,
            &mut Stats::default(),
        )
        .0;
        assert!(
            !matches!(&o, Outcome::Refused(super::world::Cause::OutOfBudget(_), _)),
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
