//! The caps' quality: profiles whose plain constrained Delaunay caps hold
//! triangles too thin, or too crowded, for the mesh's rules, which the
//! caps' refinement for quality (circumcentres of triangles with an angle
//! under 5°, chords halved where those encroach, on the first try)
//! mends, and the patch counts of caps refined.
//!
//! The test that still fails is ignored, with what it fails with: a
//! hole of sharply weighted conics, "thin": circumcentres of triangles
//! thinner than a few resolutions, wherever an angle bound leaves some
//! (chords too short to halve).
//!
//! In release builds these take a few minutes all told, most of it the
//! corner cuts and the cut circles at many seeds; debug builds run a
//! smaller set of each.

#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::time::Instant;

use glam::{DVec2, DVec3};

use super::tests::{plate_with_holes, profile, straightened, uneven_circles_from};
use super::*;
use crate::Stripped;
use crate::boolean::{Op, boolean};
use crate::par::assert_deterministic;
use crate::profile::tests::{arc, circle, polygon, rect, reversed};
use crate::profile::{Loop, Segment};
use crate::test_rng::Rng;
use crate::{Budget, Solid};

const TOL: Tolerance = Tolerance::DEFAULT;

/// `release` in a release build, `debug` in a debug one.
fn sized<T>(release: T, debug: T) -> T {
    if cfg!(debug_assertions) {
        debug
    } else {
        release
    }
}

/// `p` extruded on the XY plane from 0 to `h` at `tol`, within the
/// default budget.
fn run(p: &Profile, tol: &Tolerance, h: f64) -> Result<Solid, KernelError> {
    extrude(p, &Frame::XY, 0.0, h, 1, tol, &Budget::DEFAULT).stripped()
}

/// The cases a test saw refused, by name and error.
#[derive(Default)]
struct Refused(Vec<String>);

impl Refused {
    /// The solid of `result`, `p` extruded from 0 to `h` at `tol`, if it
    /// isn't refused (`what` names the case). A solid must be right: it
    /// passes `check_faces`, and its volume is the area of `p` as the
    /// extrude straightens it times `h`, within `1e-12` relative.
    fn solid(
        &mut self,
        p: &Profile,
        tol: &Tolerance,
        h: f64,
        result: Result<Solid, KernelError>,
        what: &str,
    ) -> Option<Solid> {
        let solid = match result {
            Ok(solid) => solid,
            Err(e) => {
                self.0.push(format!("{what}: {e:?}"));
                return None;
            }
        };
        assert_eq!(solid.mesh().check_faces(tol), Ok(()), "{what}");
        let exact = straightened(p, tol).area() * h;
        let volume = solid.volume();
        assert!(
            (volume - exact).abs() <= 1e-12 * exact,
            "{what}: volume {volume}, not {exact}"
        );
        Some(solid)
    }

    /// Fails, listing the cases refused, if there are any.
    fn none(self) {
        assert!(self.0.is_empty(), "{} refused: {:#?}", self.0.len(), self.0);
    }
}

/// The `n` points `at(θ)` for `θ = 2π·i/n`, `i` in `0..n`.
fn around(n: usize, at: impl Fn(f64) -> DVec2) -> Vec<DVec2> {
    (0..n).map(|i| at(i as f64 / n as f64 * TAU)).collect()
}

/// The point at angle `θ` on the circle of radius `r` round the origin.
fn on_circle(r: f64) -> impl Fn(f64) -> DVec2 {
    move |a: f64| DVec2::new(a.cos(), a.sin()) * r
}

/// The regular `n`-gon of radius `r` round the origin, curves `0..n`.
fn ngon(n: usize, r: f64) -> Loop {
    polygon(&around(n, on_circle(r)), 0)
}

/// The quarter disc of radius `r` round the origin whose arc is `n`
/// straight pieces: its plain caps are a fan from the centre, every two
/// of whose triangles' boxes overlap.
fn fan(n: usize, r: f64) -> Profile {
    let mut points = vec![DVec2::ZERO];
    points.extend((0..=n).map(|i| on_circle(r)(i as f64 / n as f64 * FRAC_PI_2)));
    profile(vec![polygon(&points, 0)])
}

// Fine polygons: thousands of straight sides, each vertex within a
// resolution or two of its neighbours' chord. The plain caps are fans and
// ears whose angles at the loop are about the turning angle, and an ear
// beside a wall comes within the resolution of it at their shared vertex
// (`Invalid(VertexNeighbours)` or `Hull`); the second try's moved-in
// points form an inner polygon just as flat. Refined, they pass.

#[test]
fn fine_polygons_at_coarse_tolerances() {
    let cases = sized(
        vec![
            (1024, 0.1),
            (2048, 0.1),
            (4096, 0.1),
            (4096, 1e-2),
            (8192, 1e-3),
        ],
        vec![(1024, 0.1), (4096, 1e-2)],
    );
    let mut refused = Refused::default();
    for (n, fit) in cases {
        let p = profile(vec![ngon(n, 10.0)]);
        let tol = Tolerance::new(fit).unwrap();
        let result = assert_deterministic(|| run(&p, &tol, 1.0));
        refused.solid(&p, &tol, 1.0, result, &format!("{n}-gon at {fit}"));
    }
    refused.none();
}

/// Fine outlines at fit 0.1 other than regular polygons: a plate with a
/// fine hole, a fine ring, an ellipse, an outline of uneven radius and
/// uneven sides, and fine straight pieces next to an exact quarter arc.
/// With the plain caps the plate and the ring passed, the other three
/// were refused.
fn fine_outlines() -> Vec<(&'static str, Profile)> {
    let n = sized(2048, 1024);
    let square = rect(DVec2::splat(-15.0), DVec2::splat(15.0), 1_000_000);
    let mut rng = Rng::new(14);
    // Angles 0.6 to 1.4 steps apart, radius 10 ± 1 in three waves.
    let mut jittered = Vec::with_capacity(n);
    let mut angle = 0.0;
    while angle < TAU - 0.7 * TAU / n as f64 {
        jittered.push(DVec2::new(angle.cos(), angle.sin()) * (10.0 + (3.0 * angle).sin()));
        angle += rng.range(0.6, 1.4) * TAU / n as f64;
    }
    // A half disc of radius 10: its first quarter `n / 2` straight pieces,
    // its second an exact arc, closed by its diameter.
    let mut half: Vec<DVec2> = (0..n / 2)
        .map(|i| on_circle(10.0)(i as f64 / (n / 2) as f64 * FRAC_PI_2))
        .collect();
    half.extend([DVec2::new(0.0, 10.0), DVec2::new(-10.0, 0.0)]);
    let mut segments = polygon(&half, 0).segments;
    let last = segments.len() - 1;
    segments[last - 1] = arc(
        DVec2::ZERO,
        DVec2::new(0.0, 10.0),
        DVec2::new(-10.0, 0.0),
        1_000,
    );
    vec![
        (
            "a plate with a fine hole",
            profile(vec![square, reversed(&ngon(n, 10.0))]),
        ),
        (
            "a fine ring",
            profile(vec![ngon(n, 10.0), reversed(&ngon(n, 9.0))]),
        ),
        (
            "a fine ellipse",
            profile(vec![polygon(
                &around(n, |a| DVec2::new(10.0 * a.cos(), 5.0 * a.sin())),
                0,
            )]),
        ),
        (
            "an uneven fine outline",
            profile(vec![polygon(&jittered, 0)]),
        ),
        (
            "fine pieces next to an exact arc",
            profile(vec![Loop { segments }]),
        ),
    ]
}

#[test]
fn fine_outlines_with_holes() {
    let tol = Tolerance::new(0.1).unwrap();
    let mut refused = Refused::default();
    for (what, p) in fine_outlines() {
        let result = assert_deterministic(|| run(&p, &tol, 1.0));
        refused.solid(&p, &tol, 1.0, result, what);
    }
    refused.none();
}

#[test]
fn a_fine_polygon_past_the_budget_runs_out_in_time() {
    // 65 536 sides of radius 100, each vertex 5e-3 resolutions off its
    // neighbours' chord at fit 0.1: more patches than the budget pays
    // for, refined or not. With the plain caps it ran out only after 16 s
    // (release, on a machine loaded seven times over), in the second
    // try's repair. Refined, the caps keep a fan from their centre (its
    // chords, under twice `MIN_SPLIT` resolutions, exempt the fan's
    // triangles); counting its box pairs finds it crowded, and refining
    // it for that runs out of budget in about a second, with no work left
    // for a second try.
    let p = profile(vec![ngon(65_536, 100.0)]);
    let start = Instant::now();
    let result = run(&p, &Tolerance::new(0.1).unwrap(), 1.0);
    assert_eq!(result.map(|_| ()), Err(KernelError::TooComplex));
    let bound = sized(30, 120);
    assert!(start.elapsed().as_secs() < bound, "{:?}", start.elapsed());
}

// Crowded caps: fans and strips of long thin triangles whose boxes
// overlap by the thousand. Repair counts every pair of boxes before
// anything else and runs out of budget (`TooComplex`), and a budget run
// out leaves no second try, so only caps refined before the first repair
// pass. Refined on the first try, with the points inserted into the
// triangulation one by one rather than rebuilding it every round, they
// do.

#[test]
fn fans_of_thin_triangles() {
    let mut refused = Refused::default();
    for n in sized(vec![4096, 16_384], vec![4096]) {
        let p = fan(n, 100.0);
        let what = format!("fan of {n}");
        if let Some(solid) = refused.solid(&p, &TOL, 1.0, run(&p, &TOL, 1.0), &what) {
            let patches = solid.mesh().tris().len();
            assert!(patches < 12 * (n + 2), "{what}: {patches} patches");
        }
    }
    refused.none();
}

#[test]
fn strips_between_fine_rings() {
    let n = sized(8192, 4096);
    let p = profile(vec![ngon(n, 100.0), reversed(&ngon(n, 50.0))]);
    let mut refused = Refused::default();
    let what = format!("rings of {n}");
    if let Some(solid) = refused.solid(&p, &TOL, 1.0, run(&p, &TOL, 1.0), &what) {
        let patches = solid.mesh().tris().len();
        assert!(patches < 12 * 2 * n, "{what}: {patches} patches");
    }
    refused.none();
    if cfg!(debug_assertions) {
        return;
    }
    // Two 16 384-gons fit the budget too, with some 157 000 patches,
    // whose volumes add up to the rings' within `1e-11` relative (the
    // rounding of that many terms).
    let n = 16_384;
    let p = profile(vec![ngon(n, 100.0), reversed(&ngon(n, 50.0))]);
    let solid = run(&p, &TOL, 1.0).unwrap();
    assert_eq!(solid.mesh().check_faces(&TOL), Ok(()));
    let exact = p.area();
    assert!((solid.volume() - exact).abs() <= 1e-11 * exact);
    assert!(solid.mesh().tris().len() < 12 * 2 * n);
}

#[test]
fn crowded_caps_are_deterministic() {
    let p = fan(4096, 100.0);
    let result = assert_deterministic(|| run(&p, &TOL, 1.0));
    let mut refused = Refused::default();
    refused.solid(&p, &TOL, 1.0, result, "fan of 4096");
    refused.none();
}

#[test]
fn crowded_caps_run_out_of_budget_quickly() {
    // Plain, the fan's box pairs ran out of the budget at once; refined,
    // its first triangulation and refinement do, as soon.
    let p = fan(16_384, 100.0);
    let start = Instant::now();
    let result = extrude(&p, &Frame::XY, 0.0, 1.0, 1, &TOL, &Budget::new(1 << 18)).stripped();
    assert_eq!(result.map(|_| ()), Err(KernelError::TooComplex));
    let bound = sized(2, 20);
    assert!(start.elapsed().as_secs() < bound, "{:?}", start.elapsed());
}

// Cap triangles thinner than a few resolutions at single vertices.

#[test]
fn polygons_a_few_resolutions_off_their_chords() {
    // Regular polygons whose every vertex is 1, 2 or 3 resolutions off
    // its neighbours' chord, `r·(1 − cos 2π/n)`.
    let sides = sized(vec![64, 256, 1024], vec![64, 256]);
    let mut refused = Refused::default();
    for n in sides {
        for off in [1.0, 2.0, 3.0] {
            for fit in [1e-3, 0.1] {
                let tol = Tolerance::new(fit).unwrap();
                let r = off * tol.resolution() / (1.0 - (TAU / n as f64).cos());
                let p = profile(vec![ngon(n, r)]);
                let what = format!("{n}-gon {off} resolutions off at {fit}");
                refused.solid(&p, &tol, r, run(&p, &tol, r), &what);
            }
        }
    }
    refused.none();
}

/// The shortest chord of `p`'s segments.
fn shortest_chord(p: &Profile) -> f64 {
    p.loops
        .iter()
        .flat_map(|lp| &lp.segments)
        .map(|s| s.conic.p0.distance(s.conic.p1))
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn circles_cut_unevenly_at_the_coarsest_tolerance() {
    // The uneven circles at fit 0.1: those whose shortest chord is 32
    // resolutions or more extrude. The same bits at 1 and 8 threads.
    let cases: Vec<_> = uneven_circles_from(3, [0.1; 3])
        .into_iter()
        .enumerate()
        .filter(|(_, (p, _, tol))| shortest_chord(p) >= 32.0 * tol.resolution())
        .take(sized(120, 30))
        .collect();
    let results = assert_deterministic(|| {
        cases
            .iter()
            .map(|(_, (p, r, tol))| run(p, tol, *r))
            .collect::<Vec<_>>()
    });
    let mut refused = Refused::default();
    for ((case, (p, r, tol)), result) in cases.iter().zip(results) {
        refused.solid(p, tol, *r, result, &format!("case {case}"));
    }
    refused.none();
}

// Circles cut unevenly, at other seeds than `circles_cut_unevenly`'s: arcs
// a few dozen resolutions long meeting their neighbours nearly straight
// at fit 1e-2 (and rarely 1e-3). The plain caps' slivers along the short
// chords fail at the walls, the second try's moved-in points line up
// into slivers of their own. Refined, with triangles whose shortest
// side is a chord too short to halve for long exempt, they pass.

#[test]
fn circles_cut_unevenly_at_many_seeds() {
    // 32 cases of these 1 600 failed with the plain caps, in 18 of the 40
    // seeds.
    let start = Instant::now();
    let mut refused = Refused::default();
    for seed in 1000..sized(1040, 1004) {
        let cases = uneven_circles_from(seed, [Tolerance::MIN_FIT, 1e-3, 1e-2]);
        for (case, (p, r, tol)) in cases.iter().enumerate() {
            if case % 3 != 2 {
                continue;
            }
            let what = format!("seed {seed} case {case}");
            refused.solid(p, tol, *r, run(p, tol, *r), &what);
        }
    }
    // About three minutes in release on a machine loaded seven times
    // over.
    refused.none();
    let bound = sized(240, 240);
    assert!(start.elapsed().as_secs() < bound, "{:?}", start.elapsed());
}

#[test]
fn cut_circles_refused_with_plain_caps() {
    // Seed 1003 cases 2 and 104 and seed 4000 case 23 at fit 1e-2, seed
    // 517 case 70 at 1e-3; the same bits at 1 and 8 threads.
    let mut refused = Refused::default();
    for (seed, case) in [(1003, 2), (1003, 104), (4000, 23), (517, 70)] {
        let (p, r, tol) = &uneven_circles_from(seed, [Tolerance::MIN_FIT, 1e-3, 1e-2])[case];
        let result = assert_deterministic(|| run(p, tol, *r));
        refused.solid(p, tol, *r, result, &format!("seed {seed} case {case}"));
    }
    refused.none();
}

#[test]
#[ignore = "Invalid until thin, or longer"]
fn a_hole_of_sharply_weighted_conics_at_every_tolerance() {
    // A small hole of twelve conics, weights up to 13.7 (from a stress of
    // plates with small holes): its plain caps held a sliver that the
    // mesh's rules refuse at every tolerance, `Invalid` down to 1e-5.
    // Refined (at 5° or 10°) it is still `Invalid` at every fit, repair
    // finding two pieces of one patch within the resolution at a vertex.
    let plate = sharply_weighted_hole();
    let mut refused = Refused::default();
    for fit in [1e-2, 1e-3, 1e-4, 1e-5] {
        let tol = Tolerance::new(fit).unwrap();
        refused.solid(
            &plate,
            &tol,
            1.0,
            run(&plate, &tol, 1.0),
            &format!("at {fit}"),
        );
    }
    refused.none();
}

/// A 10 × 10 plate with a small hole of twelve conics, weights up to
/// 13.7.
fn sharply_weighted_hole() -> Profile {
    let p = |x: f64, y: f64| DVec2::new(x, y);
    let hole = [
        (
            p(-2.940098442981379, 0.46978343062370953),
            p(-2.9592941074190304, 0.4670705018959389),
            0.6942110234194312,
            p(-2.9661569147101647, 0.44683349579275844),
            111,
        ),
        (
            p(-2.9661569147101647, 0.44683349579275844),
            p(-2.9892904281756953, 0.40823956572040254),
            9.309625263822857,
            p(-2.9698000311493837, 0.39568291893834306),
            110,
        ),
        (
            p(-2.9698000311493837, 0.39568291893834306),
            p(-2.9962063687252694, 0.4048029623843545),
            3.1993010817093057,
            p(-3.009375564787181, 0.384791618860897),
            109,
        ),
        (
            p(-3.009375564787181, 0.384791618860897),
            p(-3.0074356984917037, 0.40573149238467593),
            13.685473069138258,
            p(-3.013259284663757, 0.4337841315765918),
            108,
        ),
        (
            p(-3.013259284663757, 0.4337841315765918),
            p(-3.0427016707550405, 0.43304242130372617),
            0.46769333585044814,
            p(-3.0730071502644116, 0.43109565530306276),
            107,
        ),
        (
            p(-3.0730071502644116, 0.43109565530306276),
            p(-3.083305361509317, 0.45019602971022915),
            0.6842713618032584,
            p(-3.0569805976114237, 0.4705565404903875),
            106,
        ),
        (
            p(-3.0569805976114237, 0.4705565404903875),
            p(-3.047945747846238, 0.47894047546935425),
            0.28398432682261116,
            p(-3.043437932495277, 0.49257172018154144),
            105,
        ),
        (
            p(-3.043437932495277, 0.49257172018154144),
            p(-3.0307535824218976, 0.5083881691454106),
            0.06798574543281817,
            p(-3.040046314560975, 0.5294320165616994),
            104,
        ),
        (
            p(-3.040046314560975, 0.5294320165616994),
            p(-3.0097217410004613, 0.5197247509757335),
            0.962232797687248,
            p(-2.998554174264776, 0.49742421917438967),
            103,
        ),
        (
            p(-2.998554174264776, 0.49742421917438967),
            p(-2.9945238413739497, 0.4894023450219336),
            1.5025057676603009,
            p(-2.9763846633450264, 0.4955205366693143),
            102,
        ),
        (
            p(-2.9763846633450264, 0.4955205366693143),
            p(-2.958370297299105, 0.5017321807861478),
            1.0,
            p(-2.9403559312531837, 0.5079438249029813),
            101,
        ),
        (
            p(-2.9403559312531837, 0.5079438249029813),
            p(-2.9402271871172814, 0.4888636277633454),
            1.0,
            p(-2.940098442981379, 0.46978343062370953),
            100,
        ),
    ];
    let hole = Loop {
        segments: hole
            .iter()
            .map(|&(p0, c, w, p1, curve)| Segment {
                conic: Conic2::new(p0, c, w, p1).unwrap(),
                curve,
            })
            .collect(),
    };
    profile(vec![rect(p(-5.0, -5.0), p(5.0, 5.0), 0), hole])
}

// Perforated plates cut afterwards. The plain caps of a plate with rows
// of equal holes have fans tens of millimetres long out of its corners,
// to the holes' lowest and leftmost points, which lie on common tangent
// lines; a later cut near or along their long sides fails (`Invalid`).
// The same plates with outlines split into 1 mm pieces have no such
// fans and none of these fails; nor do the caps refined.

/// The plate `k·10` square, 2 thick, with `k × k` holes 10 apart, the
/// first centred at (5, 5), of radius `r`, or 4 and 4.9 by turns.
fn perforated(k: usize, r: Option<f64>) -> (Solid, f64) {
    let size = 10.0 * k as f64;
    let mut loops = vec![rect(DVec2::ZERO, DVec2::splat(size), 0)];
    for i in 0..k {
        for j in 0..k {
            let at = DVec2::new(5.0 + 10.0 * i as f64, 5.0 + 10.0 * j as f64);
            let r = r.unwrap_or(if (i + j) % 2 == 0 { 4.0 } else { 4.9 });
            loops.push(circle(at, r, 100 + (k * i + j) as u64, true));
        }
    }
    let p = profile(loops);
    let solid = run(&p, &TOL, 2.0).unwrap();
    (solid, p.area() * 2.0)
}

#[test]
fn corner_cuts_on_perforated_plates() {
    // Boxes 0.3 to 2 across cut from each corner, flush with the plate's
    // sides or 0.1 in: none comes near a hole. With the plain caps 15, 12,
    // 20 and 32 of each plate's 32 were refused.
    let plates = sized(
        vec![
            (10, Some(2.0)),
            (10, Some(3.0)),
            (10, Some(4.0)),
            (14, Some(2.0)),
        ],
        vec![(10, Some(2.0))],
    );
    let mut refused = Refused::default();
    for (k, r) in plates {
        let (plate, volume) = perforated(k, r);
        let size = 10.0 * k as f64;
        for (cx, cy) in [(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)] {
            for s in [0.3, 0.5, 1.0, 2.0] {
                for inset in [0.0, 0.1] {
                    let from = |c: f64| {
                        if c == 0.0 {
                            -s / 2.0 + inset
                        } else {
                            c - s / 2.0 - inset
                        }
                    };
                    let min = DVec3::new(from(cx), from(cy), -1.0);
                    let cut = Solid::cuboid(min, DVec3::new(s, s, 4.0), 2, &TOL).unwrap();
                    let what = format!("{k}×{k} r {r:?} corner ({cx}, {cy}) {s} in {inset}");
                    match boolean(&plate, &cut, Op::Difference, &TOL, &Budget::DEFAULT) {
                        Ok(solid) => {
                            let exact = volume - (s / 2.0 + inset).powi(2) * 2.0;
                            assert!(
                                (solid.volume() - exact).abs() < 1e-9 * volume,
                                "{what}: volume {}, not {exact}",
                                solid.volume()
                            );
                        }
                        Err(e) => refused.0.push(format!("{what}: {e:?}")),
                    }
                }
            }
        }
    }
    refused.none();
}

#[test]
fn holes_drilled_in_line_with_extruded_ones() {
    // A plate with one to three holes in a row, extruded, then the next
    // hole of the row drilled. With the plain caps 2 of the 36 were
    // refused, the second hole 5 from the first on the smaller plate.
    let mut refused = Refused::default();
    for size in [20.0, 60.0] {
        for r in [0.5, 1.0] {
            for pitch in [2.4, 3.0, 5.0] {
                for k in 1..4 {
                    let mut loops = vec![rect(DVec2::ZERO, DVec2::splat(size), 0)];
                    for i in 0..k {
                        let at = DVec2::new(2.0 + pitch * i as f64, 2.0);
                        loops.push(circle(at, r, 100 + i as u64, true));
                    }
                    let plate = run(&profile(loops), &TOL, 1.0).unwrap();
                    let at = DVec3::new(2.0 + pitch * k as f64, 2.0, -1.0);
                    let drill = Solid::cylinder(at, r, 3.0, 2, &TOL).unwrap();
                    let what = format!("{size} r {r} pitch {pitch} after {k}");
                    match boolean(&plate, &drill, Op::Difference, &TOL, &Budget::DEFAULT) {
                        Ok(solid) => {
                            let exact = size * size - (k + 1) as f64 * PI * r * r;
                            assert!(
                                (solid.volume() - exact).abs() < 1e-9 * exact,
                                "{what}: volume {}, not {exact}",
                                solid.volume()
                            );
                        }
                        Err(e) => refused.0.push(format!("{what}: {e:?}")),
                    }
                }
            }
        }
    }
    refused.none();
}

// Pins: the patch counts of caps that passed before refinement, now
// refined wherever a triangle had an angle under 5° (they were 4 012, 92,
// 12 and 65 532 with the plain caps).

#[test]
fn refined_caps_patches() {
    let patches = |p: &Profile| run(p, &TOL, 2.0).unwrap().mesh().tris().len();
    // The 80 × 80 plate with 64 holes, and `a_plate_with_holes`' 100 × 60
    // plate with four.
    assert_eq!(patches(&plate_with_holes(8, 8, 80.0, 80.0)), 4292);
    let four = profile(vec![
        rect(DVec2::ZERO, DVec2::new(100.0, 60.0), 0),
        circle(DVec2::new(25.0, 30.0), 5.0, 4, true),
        circle(DVec2::new(75.0, 30.0), 5.0, 5, true),
        circle(DVec2::new(50.0, 10.0), 3.0, 6, true),
        reversed(&rect(DVec2::new(45.0, 40.0), DVec2::new(55.0, 50.0), 7)),
    ]);
    assert_eq!(patches(&four), 116);
    // A rib 100 × 1: its two plain triangles a cap have a 0.6° corner, and
    // no small input angle exempts them, so its long sides are halved.
    let rib = profile(vec![rect(DVec2::ZERO, DVec2::new(100.0, 1.0), 0)]);
    assert_eq!(patches(&rib), 100);
    // 16 384 sides of radius 100.
    if !cfg!(debug_assertions) {
        assert_eq!(patches(&profile(vec![ngon(16_384, 100.0)])), 78_560);
    }
}

#[test]
fn uneven_circles_and_random_plates_patches() {
    // The patches of `circles_cut_unevenly`'s circles and of
    // `random_plates_with_holes`' plates that extrude, all told (18 872
    // and 1 150 with the plain caps).
    let circles: usize = uneven_circles_from(3, [Tolerance::MIN_FIT, 1e-3, 1e-2])
        .iter()
        .map(|(p, r, tol)| run(p, tol, *r).unwrap().mesh().tris().len())
        .sum();
    assert_eq!(circles, 30_092);
    let plates: usize = super::tests::random_plates()
        .iter()
        .filter_map(|(p, tol, h)| run(p, tol, *h).ok())
        .map(|solid| solid.mesh().tris().len())
        .sum();
    assert_eq!(plates, 1636);
}

#[test]
fn plain_caps_are_not_refined_for_crowding() {
    // The fan of 4 096, every two of whose boxes overlap: the plain caps
    // (the last tries') are the fan as triangulated, as before the caps
    // were refined, not refined for crowding as the first try's are.
    let p = fan(4096, 100.0);
    let margin = TOL.resolution();
    let mut work = Work::new(&Budget::DEFAULT);
    let mut chain = Chain::new(&p, margin).unwrap();
    chain.separate(&mut work).unwrap();
    for (mode, refined) in [(Mode::PLAIN, false), (Mode::QUALITY, true)] {
        let mut work = Work::new(&Budget::DEFAULT);
        let start = Rounds::new(chain.clone());
        let (_, cap) =
            cap::triangulate(start, margin, mode, &mut None, &mut false, &mut work).unwrap();
        assert_eq!(cap.steiner.len() > 500, refined, "{mode:?}");
        if !refined {
            assert_eq!(cap.tris.len(), 4096);
        }
    }
}

/// One of `xs`, at random.
fn pick<T: Copy>(rng: &mut Rng, xs: &[T]) -> T {
    xs[(rng.next_u64() % xs.len() as u64) as usize]
}

/// A wavy closed curve round `c`, its radius `r·(1 + amp·sin(k·θ + φ))`
/// with the phase `φ` at random, as `n` conics through its points at even
/// angles, each through the curve's point at its middle angle too, with
/// weights at random (log-uniform) in `weights`: a fitted spline's
/// pieces, sharply weighted.
fn wavy_spline(
    rng: &mut Rng,
    c: DVec2,
    r: f64,
    n: usize,
    [amp, k]: [f64; 2],
    curve: u64,
    weights: (f64, f64),
) -> Loop {
    let phase = rng.range(0.0, TAU);
    let at = |t: f64| c + DVec2::new(t.cos(), t.sin()) * (r * (1.0 + amp * (k * t + phase).sin()));
    let segments = (0..n)
        .map(|i| {
            let [t0, t1] = [i, i + 1].map(|j| j as f64 / n as f64 * TAU);
            let (p0, p1) = (at(t0), if i + 1 == n { at(0.0) } else { at(t1) });
            let w = rng.log_range(weights.0, weights.1);
            // The conic's point at its middle, (p0 + 2w·c + p1) / (2 + 2w),
            // on the curve.
            let mid = at(0.5 * (t0 + t1));
            let control = (mid * (2.0 + 2.0 * w) - p0 - p1) / (2.0 * w);
            Segment {
                conic: Conic2::new(p0, control, w, p1).unwrap(),
                curve: curve + i as u64,
            }
        })
        .collect();
    Loop { segments }
}

#[test]
fn the_plain_caps_second_try_is_made() {
    // A spline of 64 sharply weighted pieces (weights 1/64 to 64) round
    // two holes like it, at fit 1e-5: refined, the caps run out of budget
    // (their mending halving the pieces to `MAX_MEND_DEPTH` at once, and
    // refinement after), and so do the plain caps, with no flat corners
    // on the refined try to fork a second try from; the plain caps'
    // second try, from their first try's fork, passes, as it did before
    // the caps were refined.
    let mut rng = Rng::new(701_441);
    let tol = Tolerance::new(pick(&mut rng, &[1e-5, 1e-4, 1e-3, 1e-2, 1e-1])).unwrap();
    let r = rng.log_range(0.1, 500.0);
    let n = pick(&mut rng, &[64, 128, 256]);
    let wave = [
        pick(&mut rng, &[0.02, 0.2, 0.5]),
        pick(&mut rng, &[3.0, 5.0, 11.0, 40.0]),
    ];
    let weights = pick(&mut rng, &[(1.0, 1.0), (0.5, 2.0), (1.0 / 64.0, 64.0)]);
    assert_eq!(
        (tol.fit(), n, wave, weights),
        (1e-5, 64, [0.5, 3.0], (1.0 / 64.0, 64.0))
    );
    let mut loops = vec![wavy_spline(&mut rng, DVec2::ZERO, r, n, wave, 0, weights)];
    for h in 0..rng.next_u64() % 3 {
        let size = r * rng.log_range(0.01, 0.15);
        let at = DVec2::new(rng.range(-0.3, 0.3), rng.range(-0.3, 0.3)) * r;
        let n = pick(&mut rng, &[8, 64, 500]);
        let hole = wavy_spline(&mut rng, at, size, n, [0.1, 3.0], 1000 * (h + 1), weights);
        loops.push(reversed(&hole));
    }
    let h = rng.log_range(0.01, 100.0);
    let p = profile(loops);

    // The refined caps and the plain caps' first try fail.
    let margin = tol.resolution();
    let mut work = Work::new(&Budget::DEFAULT);
    let mut chain = Chain::new(&p, margin).unwrap();
    chain.separate(&mut work).unwrap();
    for (mode, fork) in [(Mode::QUALITY, false), (Mode::PLAIN, true)] {
        let mut work = Work::new(&Budget::DEFAULT);
        let mut forked = None;
        let caps = cap::triangulate(
            Rounds::new(chain.clone()),
            margin,
            mode,
            &mut forked,
            &mut false,
            &mut work,
        );
        let solid = caps.and_then(|(chain, cap)| {
            let mesh = build(&chain, &cap, &Frame::XY, 0.0, h, 1)?;
            Solid::new_repaired_within(mesh, &tol, &mut work).map_err(|f| f.error)
        });
        assert!(solid.is_err(), "{mode:?}");
        assert_eq!(forked.is_some(), fork, "{mode:?}");
    }
    let mut refused = Refused::default();
    refused.solid(&p, &tol, h, run(&p, &tol, h), "spline round two holes");
    refused.none();
}

/// A circle of radius `r` round `c` cut at random angles (steps from
/// `min` to 1.5 radians, log-uniform) into arcs, curves `curve..`.
fn cut_at_random(rng: &mut Rng, c: DVec2, r: f64, min: f64, curve: u64) -> Loop {
    let mut angles = vec![0.0];
    loop {
        let next = angles.last().unwrap() + rng.log_range(min, 1.5);
        if next >= TAU - min {
            break;
        }
        angles.push(next);
    }
    let points: Vec<DVec2> = angles
        .iter()
        .map(|&a: &f64| c + DVec2::new(a.cos(), a.sin()) * r)
        .collect();
    let n = points.len();
    let segments = (0..n)
        .map(|i| arc(c, points[i], points[(i + 1) % n], curve + i as u64))
        .collect();
    Loop { segments }
}

#[test]
fn a_run_of_fine_pieces_in_a_cut_circle_extrudes() {
    // A circle of radius 4.3 cut at random into arcs, one of which is
    // 1 000 straight pieces of its chord, round two holes cut so, at fit
    // 1e-3: the refined first try fails, and the plain caps' first try
    // passes after repair. Its caps' boxes crowd each other (some 500 000
    // pairs); refined for that, they failed instead.
    let mut rng = Rng::new(300_291);
    let tol = Tolerance::new(pick(&mut rng, &[1e-5, 1e-4, 1e-3, 1e-2, 1e-1])).unwrap();
    let r = rng.log_range(0.1, 1000.0);
    let mut outline = cut_at_random(&mut rng, DVec2::ZERO, r, 2e-3, 0);
    let n = outline.segments.len() as u64;
    let i = (rng.next_u64() % n) as usize;
    let (a, b) = (outline.segments[i].conic.p0, outline.segments[i].conic.p1);
    let at = |j: usize| a.lerp(b, j as f64 / 1000.0);
    let pieces = (0..1000).map(|j| Segment::line(at(j), at(j + 1), 9000 + j as u64).unwrap());
    outline.segments.splice(i..=i, pieces);
    let mut loops = vec![outline];
    for h in 0..rng.next_u64() % 3 {
        let size = r * rng.log_range(0.01, 0.2);
        let at = DVec2::new(rng.range(-0.4, 0.4), rng.range(-0.4, 0.4)) * r;
        loops.push(reversed(&cut_at_random(
            &mut rng,
            at,
            size,
            2e-3,
            1000 * (h + 1),
        )));
    }
    let h = rng.log_range(0.01, 100.0);
    let p = profile(loops);
    assert_eq!(
        (
            tol.fit(),
            p.loops.iter().map(|l| l.segments.len()).collect::<Vec<_>>()
        ),
        (1e-3, vec![1036, 18, 32])
    );
    let margin = tol.resolution();
    let mut work = Work::new(&Budget::DEFAULT);
    let mut chain = Chain::new(&p, margin).unwrap();
    chain.separate(&mut work).unwrap();
    for (mode, passes) in [(Mode::QUALITY, false), (Mode::PLAIN, true)] {
        let mut work = Work::new(&Budget::DEFAULT);
        let start = Rounds::new(chain.clone());
        let caps = cap::triangulate(start, margin, mode, &mut None, &mut false, &mut work);
        let solid = caps.and_then(|(chain, cap)| {
            let mesh = build(&chain, &cap, &Frame::XY, 0.0, h, 1)?;
            Solid::new_repaired_within(mesh, &tol, &mut work).map_err(|f| f.error)
        });
        assert_eq!(solid.is_ok(), passes, "{mode:?}");
    }
    let mut refused = Refused::default();
    refused.solid(&p, &tol, h, run(&p, &tol, h), "a run of fine pieces");
    refused.none();
}
