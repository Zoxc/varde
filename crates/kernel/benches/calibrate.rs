//! Calibration of the kernel's unit table
//! (`varde_decide::num::schedule::UNITS`): times each computation kind
//! the kernel charges, per precision level where it has levels (an exact
//! predicate per predicate kind and stage, on each of the kind's
//! predicates), and
//! compares the time per unit charged with the target of half a
//! microsecond per unit. A kind more than 4× off fails the run, so a
//! change that makes a kind much cheaper or dearer, or a machine much
//! unlike the reference one, shows here. It also runs a seam pass that
//! uses up `MAX_WORK` and fails unless it takes seconds.
//!
//! Run with `cargo bench -p varde-kernel --bench calibrate` (optimised,
//! one thread). It prints, per kind, the time of one computation, the
//! units the table charges for it and the time per unit; `units?` is the
//! value the measurement suggests (the time over half a microsecond,
//! rounded, at least 1). The reference machine is an AMD Ryzen 7 5700X3D.
//! Kinds below a unit are measured in parts of a unit
//! (`schedule::UNIT_PARTS`): a record charges the sum of its parts,
//! rounded up to whole units.
//!
//! `VARDE_CALIBRATE=quick` runs each kind for a tenth of the time: a
//! smoke test, its numbers noisier.

use std::collections::BTreeMap;
use std::hint::black_box;
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use glam::DVec3;
use malachite::Rational;
use varde_decide::bezier::{HomCurve, Poly, Scalar};
use varde_decide::count::boxes::CUSP_PIECES;
use varde_decide::count::s02::{Charge, count_pieces};
use varde_decide::count::testing::{FaceData, Order, chain, motion_of, polygon};
use varde_decide::count::xrec::clusters_at_charged;
use varde_decide::ids::{EdgeId, VertId};
use varde_decide::num::bounded::Bounded;
use varde_decide::num::decide::{Band, Fail, Sign};
use varde_decide::num::exact::{self, ORDERS, Pred};
use varde_decide::num::level::{AtLevel, LevelScalar, at_level};
use varde_decide::num::preds::{
    Dot2d, LineGap, LinePlace, NearGap, Orient2Exact, Orient2d, Orient3Dir, Orient3d, PlaneSide,
    polygon_area_sign,
};
use varde_decide::num::schedule::{LEVELS, Level, PredKind, UNIT_PARTS, UNITS};
use varde_decide::num::trig;
use varde_decide::perturb::{
    Curved, EdgeEdge, HomLineShadow, HomPairShadow, HomPoint, HomPointLineShadow, HomShadow,
    LineShadow, Moving, NU, PlaneHeight, PointHeight, R_HAT, Rate, Seg, ShadowAxis, SharedCrossing,
    UP, VvShadow,
};
use varde_decide::records::cusp::{CuspSpan, Sharp, cusp_rec};
use varde_decide::records::feet::{EdgeCurve, NearFeet, decide_feet};
use varde_decide::records::height::hit::face_hits;
use varde_decide::records::height::{AnchorRow, HeightGeometry, Heights, KeyValue, Records};
use varde_decide::records::keys::{
    Anchor, FootId, HKey, HeightKey, Operand, Point, Query, RayKey, SpanKey, XKey,
};
use varde_decide::records::ray::{ChainRec, CurveGeometry, CurvedRays, Span};
use varde_decide::records::shadow::{
    Cost, Decided, LevelWork, Pass, RayRec, ShadowGeometry, Shadows,
};
use varde_decide::records::side::Sides;
use varde_decide::records::table::Fresh;
use varde_decide::records::xrec::{EdgeRays, XCurve, XPair, XSides, XTerms};
use varde_kernel::MAX_WORK;
use varde_kernel::boolean::classify::calibration as audit;
use varde_kernel::boolean::classify::{AuditInput, AuditRegion, Bands, Cover, Leaf, Ribbon, Tri};
use varde_kernel::bvh_calibration::Tree;
use varde_kernel::geom::certify::SeamBound;
use varde_kernel::geom::certify::calibration::{self, arcs, base_plane, quarter};
use varde_kernel::geom::certify::implicit::calibration as implicit;
use varde_kernel::geom::curve::Curve3D;
use varde_kernel::geom::form::PlaneKey;
use varde_kernel::geom::formkey::calibration as keys;
use varde_kernel::geom::lattice::{Lattice, Level as TileLevel, Rect};
use varde_kernel::geom::pcurve::PCurve;
use varde_kernel::geom::surface::Surface;
use varde_kernel::topo::brep_min::{Brep, calibration as scan};
use varde_testkit::rng::Rng;

/// The time one unit stands for, in nanoseconds.
const UNIT_NS: f64 = 500.0;

/// The most a kind's time per unit may differ from [`UNIT_NS`], either
/// way.
const DRIFT: f64 = 4.0;

/// The longest a pass at `MAX_WORK` may take on one thread, in seconds:
/// `MAX_WORK` units at [`UNIT_NS`] each, times the drift allowed.
fn max_pass_seconds() -> f64 {
    MAX_WORK as f64 * UNIT_NS * 1e-9 * DRIFT
}

#[allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "a benchmark: it sets up its own one-thread pool and reports its size"
)]
fn main() -> ExitCode {
    // `cargo bench` passes `--bench`; nothing else is taken.
    let quick = std::env::var("VARDE_CALIBRATE").is_ok_and(|v| v == "quick");
    let budget = Duration::from_millis(if quick { 30 } else { 300 });
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("a thread pool");
    let rows = pool.install(|| {
        let mut rows = Vec::new();
        seam_cells(budget, &mut rows);
        implicit_cells(budget, &mut rows);
        count_piece_rows(budget, &mut rows);
        setup_rows(budget, &mut rows);
        xrec_rows(budget, &mut rows);
        hit_rows(budget, &mut rows);
        record_rows(budget, &mut rows);
        record_kind_rows(budget, &mut rows);
        audit_rows(budget, &mut rows);
        scan_rows(budget, &mut rows);
        form_key_rows(budget, &mut rows);
        bvh_rows(budget, &mut rows);
        rows
    });
    println!(
        "{:<28} {:>12} {:>10} {:>10} {:>8}",
        "kind", "ns each", "units", "ns/unit", "units?"
    );
    let mut failed = Vec::new();
    for r in &rows {
        let per_unit = r.ns / r.units;
        let ratio = per_unit / UNIT_NS;
        let off = !(1.0 / DRIFT..=DRIFT).contains(&ratio);
        let suggest = (r.ns / UNIT_NS).round();
        let suggest = if suggest > 1.0 { suggest } else { 1.0 };
        println!(
            "{:<28} {:>12.1} {:>10.3} {:>10.1} {:>8}{}",
            r.kind,
            r.ns,
            r.units,
            per_unit,
            suggest,
            if off { "  DRIFT" } else { "" }
        );
        if off {
            failed.push(r.kind.clone());
        }
    }
    // The seam pass at `MAX_WORK`, on one thread and on all.
    let (one, used) = pool.install(seam_pass_at_max_work);
    let (all, used_all) = seam_pass_at_max_work();
    println!(
        "seam pass at MAX_WORK ({MAX_WORK} units): {used} used, {one:.2} s on one thread, \
         {all:.2} s on {} (limit {:.1} s)",
        rayon::current_num_threads(),
        max_pass_seconds()
    );
    if used != MAX_WORK || used_all != MAX_WORK {
        failed.push("seam pass: didn't use MAX_WORK up".into());
    }
    if one > max_pass_seconds() {
        failed.push("seam pass at MAX_WORK".into());
    }
    if failed.is_empty() {
        println!("calibration: every kind within {DRIFT}× of {UNIT_NS} ns per unit");
        ExitCode::SUCCESS
    } else {
        println!("calibration FAILED: {}", failed.join(", "));
        ExitCode::FAILURE
    }
}

/// One measured kind: its name, the time of one computation and the units
/// the table charges for it.
struct Row {
    kind: String,
    ns: f64,
    units: f64,
}

/// The time of one computation of `f`, which does some and gives their
/// number: the least over rounds of repeating it for `budget`.
#[allow(clippy::disallowed_methods, reason = "a benchmark: it reads the clock")]
fn time(budget: Duration, mut f: impl FnMut() -> u64) -> f64 {
    // Warm up.
    black_box(f());
    let mut best = f64::INFINITY;
    for _ in 0..3 {
        let (mut n, start) = (0u64, Instant::now());
        while start.elapsed() < budget / 3 {
            n += f();
        }
        let ns = start.elapsed().as_nanos() as f64 / n as f64;
        if ns < best {
            best = ns;
        }
    }
    best
}

// ---------------------------------------------------------------------
// Seam cells.

/// Seam cells at each level, 8 to a piece (a depth of 3 halvings) on the
/// arcs.
fn seam_cells(budget: Duration, rows: &mut Vec<Row>) {
    let s = base_plane();
    let arcs = arcs(&s);
    for level in 0..LEVELS as u8 {
        let ns = time(budget, || {
            arcs.iter()
                .map(|(c, p)| calibration::cells(&s, c, p, level, 3).unwrap())
                .sum()
        });
        rows.push(Row {
            kind: format!("seam cell L{level}"),
            ns,
            units: UNITS.seam_cell[usize::from(level)] as f64,
        });
    }
}

/// Distance-to-implicit cells at each level, 8 to a span (a depth of 3
/// halvings), on the arc near the sphere (a quadric) and the one near
/// the torus (a quartic).
fn implicit_cells(budget: Duration, rows: &mut Vec<Row>) {
    let (sphere, near_sphere, torus, near_torus) = implicit::arcs();
    for level in 0..LEVELS as u8 {
        let l = usize::from(level);
        let ns = time(budget, || {
            implicit::cells(&sphere, &near_sphere, level, 3).unwrap()
        });
        rows.push(Row {
            kind: format!("implicit cell quadric L{level}"),
            ns,
            units: UNITS.implicit_cell.quadric[l] as f64,
        });
        let ns = time(budget, || {
            implicit::cells(&torus, &near_torus, level, 3).unwrap()
        });
        rows.push(Row {
            kind: format!("implicit cell quartic L{level}"),
            ns,
            units: UNITS.implicit_cell.quartic[l] as f64,
        });
    }
}

/// The seconds of a seam pass that runs out at `MAX_WORK` on the current
/// pool, and the units it used: the arcs' bounds towards targets they
/// can't all meet, repeated past the budget.
#[allow(clippy::disallowed_methods, reason = "a benchmark: it reads the clock")]
fn seam_pass_at_max_work() -> (f64, u64) {
    let s = base_plane();
    let one = arcs(&s);
    let (_, per) = calibration::pass(&s, &one, 0.0, MAX_WORK);
    let copies = (MAX_WORK / per.max(1) + 2) as usize;
    let coedges: Vec<(Curve3D, PCurve)> = one
        .iter()
        .cycle()
        .take(copies * one.len())
        .cloned()
        .collect();
    let start = Instant::now();
    let (out, used) = calibration::pass(&s, &coedges, 0.0, MAX_WORK);
    let secs = start.elapsed().as_secs_f64();
    let out: Option<Vec<SeamBound>> = black_box(out);
    assert!(out.is_none(), "the pass runs out");
    (secs, used)
}

// ---------------------------------------------------------------------
// Count pieces.

/// The column through `(x, y)` of `z = 0`, `z` above it.
fn column(x: f64, y: f64, z: f64) -> [f64; 3] {
    [x + z / UP.z * UP.x, y + z / UP.z * UP.y, z]
}

/// The trims' band of the polygons.
const BAND: f64 = 1.0 / 4096.0;

/// `f` given the count of the polygon `pts` (a face of `B` on the plane
/// `z = 0` over the tiles `[0, 2)²`, [`polygon`]) against the point of
/// `A` at `q` with a charge, giving its cost. The face and its corners
/// are built once, before `f` runs. The face is counted without its
/// tiles' nets, as a curved face is: no hit is read.
fn with_count<R>(
    pts: &[[f64; 2]],
    q: [f64; 3],
    f: impl FnOnce(&dyn Fn(&Charge) -> Cost) -> R,
) -> R {
    let mut order = Order::default();
    let face = FaceData {
        nets: None,
        ..polygon(pts, BAND, &mut order)
    };
    let corners = face.corners(&order);
    let (shadows, feet) = (Shadows::new(), NearFeet::new());
    let reads = Records {
        shadows: &shadows,
        feet: &feet,
        order: &order,
    };
    let sides = Sides::new();
    // `query_with` takes a `Fn`: `f` runs once.
    let f = std::cell::Cell::new(Some(f));
    face.query_with(
        Some(&corners),
        &order,
        Point::vert(Operand::A, VertId(7)),
        q,
        &|t| chain(&t.curve, q, Operand::A),
        &|fq, _| {
            let f = f.take().expect("run once");
            f(&|charge| count_pieces(fq, &sides, &reads, charge).1)
        },
    )
}

/// A unit a piece at every level, to count a search's pieces.
fn per_piece(share: u64) -> Charge {
    Charge {
        share,
        units: [1; LEVELS],
    }
}

/// The time of a count with `charge`.
fn count_ns(budget: Duration, count: &dyn Fn(&Charge) -> Cost, charge: &Charge) -> f64 {
    time(budget, || {
        black_box(count(charge));
        1
    })
}

/// Count pieces at each level: a rectangle `2⁻⁵⁰` high and `½` long,
/// against a point below its long side, refines at every level and is
/// unsettled at the last. A count on a share of `n` units, a unit a
/// piece, stops at its `n + 1`-th piece, before testing it. Run on
/// shares that stop it at each level's last piece, the time between two
/// shares is the level's own: its patches lifted to the level and its
/// pieces, the level before's last piece counted as one of its (the
/// first level's lift is timed with the count's setup, on a share of
/// nothing). Then the first level on a square that settles there.
fn count_piece_rows(budget: Duration, rows: &mut Vec<Row>) {
    let h = 1.0 / (1u64 << 50) as f64;
    let thin = [
        [1.25, 1.25],
        [1.75, 1.25],
        [1.75, 1.25 + h],
        [1.25, 1.25 + h],
    ];
    with_count(&thin, column(1.375, 1.25, -0.01), |count| {
        let full = count(&per_piece(u64::MAX));
        let pieces: Vec<u64> = full.levels.iter().map(|l| u64::from(l.evals)).collect();
        assert!(pieces.iter().all(|&n| n > 1), "{pieces:?}");
        let mut before = count_ns(budget, count, &per_piece(0));
        let mut through = 0u64;
        for level in 0..LEVELS {
            through += pieces[level];
            // Up to this level's last piece: the next level not begun.
            let ns = count_ns(budget, count, &per_piece(through - 1));
            // Its last piece is timed with the next level, the level
            // before's with it: the first level has one fewer.
            let n = if level == 0 {
                pieces[0] - 1
            } else {
                pieces[level]
            };
            rows.push(Row {
                kind: format!("count piece L{level}"),
                ns: (ns - before) / n as f64,
                units: UNITS.count_piece[level] as f64,
            });
            before = ns;
        }
    });
    let square = [[1.25, 1.25], [1.75, 1.25], [1.75, 1.75], [1.25, 1.75]];
    with_count(&square, column(1.5, 1.4, -0.01), |count| {
        let n = u64::from(count(&per_piece(u64::MAX)).levels[0].evals);
        rows.push(Row {
            kind: "count piece L0, square".into(),
            ns: count_ns(budget, count, &per_piece(u64::MAX)) / n as f64,
            units: UNITS.count_piece[0] as f64,
        });
    });
}

// ---------------------------------------------------------------------
// Records.

/// The units a record is charged for `cost` alone, in parts of a unit.
fn parts(cost: &Cost) -> f64 {
    cost.parts(&UNITS.record) as f64 / UNIT_PARTS as f64
}

/// A pseudo-random dyadic number in `[0, 1)` with `bits` bits after the
/// point, the `k`-th of a sequence: sums and halvings of a few stay
/// exact.
fn dyadic(k: u64, bits: u32) -> f64 {
    let x = (k.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 11) as f64 / (1u64 << 53) as f64;
    let s = (1u64 << bits) as f64;
    (x * s).floor() / s
}

/// A pseudo-random point of `[0, 200)³` at full precision.
fn general(k: u64) -> DVec3 {
    let f = |x: u64| ((x.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 11) as f64) / (1u64 << 53) as f64;
    DVec3::new(
        f(4 * k) * 200.0,
        f(97 * k + 1) * 200.0,
        f(31 * k + 2) * 200.0,
    )
}

/// A pseudo-random dyadic point of `[0, 200)³`, 10 bits after the point.
fn grid(k: u64) -> DVec3 {
    DVec3::new(
        dyadic(4 * k, 10) * 200.0,
        dyadic(97 * k + 1, 10) * 200.0,
        dyadic(31 * k + 2, 10) * 200.0,
    )
}

/// `2^e`, exactly.
fn pow2(e: i32) -> f64 {
    let mut x = 1.0;
    for _ in 0..e.unsigned_abs() {
        x = if e < 0 { x * 0.5 } else { x * 2.0 };
    }
    x
}

/// One predicate's fixtures for each exact stage it can reach: general
/// positions the filter decides, exact zeros the expansions find, and
/// exact zeros so small the rationals do (`None` where its inputs can't
/// leave the expansions' range: a predicate of degree 1 in stored
/// floats).
struct Fixtures<P> {
    name: &'static str,
    kind: PredKind,
    stages: [Option<Vec<P>>; 3],
}

/// The stages' names, in [`Fixtures::stages`]' order.
const STAGES: [&str; 3] = ["filter", "expansion", "rational"];

/// The rows of one predicate's stages, each priced at its kind's row:
/// the time of one evaluation against the units its runs (the stages
/// before included) are charged.
fn exact_pred_rows<P: Pred>(budget: Duration, f: Fixtures<P>, rows: &mut Vec<Row>) {
    for (stage, preds) in f.stages.iter().enumerate() {
        let Some(preds) = preds else { continue };
        let mut work = exact::Work::default();
        for p in preds {
            assert_eq!(p.kind(), f.kind, "{}", f.name);
            work = work.and(exact::value_sign(p).1);
        }
        let r = work.of(f.kind);
        let ran = [r.filters, r.expansions, r.rationals];
        assert!(
            ran[stage] as usize == preds.len() && ran[stage + 1..].iter().all(|&n| n == 0),
            "{} {}: {work:?}",
            f.name,
            STAGES[stage]
        );
        let ns = time(budget, || {
            for p in preds {
                let _ = black_box(exact::value_sign(p));
            }
            preds.len() as u64
        });
        rows.push(Row {
            kind: format!("exact {:?} {}, {}", f.kind, STAGES[stage], f.name),
            ns,
            units: parts(&Cost::exact(work)) / preds.len() as f64,
        });
    }
}

/// A homogeneous point of a weighted quadratic at `t = ½`: control
/// points `p + e`, `p`, `p − e`, weights 1, `w`, 1, so `X = W·p`
/// exactly, the point `p`.
fn hom_at(p: DVec3, e: DVec3, w: u32) -> HomPoint {
    let q = |n: u32, d: u32| Rational::from(n) / Rational::from(d);
    HomPoint {
        terms: vec![(p + e, q(1, 4)), (p, q(w, 2)), (p - e, q(1, 4))],
        rates: [Rate::ZERO; ORDERS],
    }
}

/// Exact predicate stages per predicate kind, on the kind's own
/// predicates: the shadow rows of the records and the 2D and 3D
/// orientations of the planar checks and the label audit.
fn exact_rows(budget: Duration, rows: &mut Vec<Row>) {
    let n = 64u64;
    let fixed = Moving::fixed;
    // Side: `a·(w − q)` along `r̂` (`w − q = (a, b, 16a)`), along `ν`
    // (`(514m − 48n, 3m, 3n)`) and up (`(16a, b·0, −a)`).
    let side = |k: u64, scale: f64, zero: bool| {
        let (q, a, b) = (
            grid(k) * scale,
            dyadic(k + 7, 8) * scale,
            dyadic(k + 9, 8) * scale,
        );
        let along = if zero {
            DVec3::new(a, b, 16.0 * a)
        } else {
            general(k + 3) * scale
        };
        VvShadow {
            q: fixed(q),
            w: fixed(q + along),
            axis: ShadowAxis::Along,
        }
    };
    let side_nu = |k: u64, zero: bool| {
        let q = grid(k);
        let (m, n) = (dyadic(k + 7, 6), dyadic(k + 9, 6));
        let d = if zero {
            DVec3::new(514.0 * m - 48.0 * n, 3.0 * m, 3.0 * n)
        } else {
            general(k + 3)
        };
        VvShadow {
            q: fixed(q),
            w: fixed(q + d),
            axis: ShadowAxis::Side,
        }
    };
    let up = |k: u64, zero: bool| {
        let (q, a) = (grid(k), dyadic(k + 7, 8));
        let d = if zero {
            DVec3::new(16.0 * a, 0.0, -a)
        } else {
            general(k + 3)
        };
        PointHeight {
            q: fixed(q),
            w: fixed(q + d),
        }
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "vv shadow along",
            kind: PredKind::Side,
            stages: [
                Some((0..n).map(|k| side(k, 1.0, false)).collect()),
                Some((0..n).map(|k| side(k, 1.0, true)).collect()),
                None,
            ],
        },
        rows,
    );
    exact_pred_rows(
        budget,
        Fixtures {
            name: "vv shadow side",
            kind: PredKind::Side,
            stages: [
                Some((0..n).map(|k| side_nu(k, false)).collect()),
                Some((0..n).map(|k| side_nu(k, true)).collect()),
                None,
            ],
        },
        rows,
    );
    exact_pred_rows(
        budget,
        Fixtures {
            name: "point height",
            kind: PredKind::Side,
            stages: [
                Some((0..n).map(|k| up(k, false)).collect()),
                Some((0..n).map(|k| up(k, true)).collect()),
                None,
            ],
        },
        rows,
    );
    // A point's side of a stored plane, `n·p − c`: `c = n·p` exactly on
    // the plane (10-bit grids); for the rationals all scaled by `2⁻⁶⁰⁰`.
    let plane_side = |k: u64, scale: f64, zero: bool| {
        let (nrm, p) = (grid(k) / 200.0 + DVec3::Z, grid(k + 5) * scale);
        let c = if zero {
            nrm.dot(p)
        } else {
            general(k).x * scale
        };
        PlaneSide {
            normal: nrm * scale,
            offset: c * scale,
            p,
        }
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "plane side",
            kind: PredKind::Side,
            stages: [
                Some((0..n).map(|k| plane_side(k, 1.0, false)).collect()),
                Some((0..n).map(|k| plane_side(k, 1.0, true)).collect()),
                Some((0..n).map(|k| plane_side(k, pow2(-600), true)).collect()),
            ],
        },
        rows,
    );
    // Orient2: `orient(a, b, c)` with `c = a + ½(b − a)` on the line,
    // scaled by `2⁻⁶⁰⁰` for the rationals.
    let orient2 = |k: u64, scale: f64, zero: bool| {
        let p = |i: u64| {
            let g = grid(3 * k + i) * scale;
            [g.x, g.y]
        };
        let (a, b) = (p(0), p(1));
        let c = if zero {
            [a[0] + 0.5 * (b[0] - a[0]), a[1] + 0.5 * (b[1] - a[1])]
        } else {
            let g = general(k) * scale;
            [g.x, g.y]
        };
        Orient2d([a, b, c])
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "orient 2D",
            kind: PredKind::Orient2,
            stages: [
                Some((0..n).map(|k| orient2(k, 1.0, false)).collect()),
                Some((0..n).map(|k| orient2(k, 1.0, true)).collect()),
                Some((0..n).map(|k| orient2(k, pow2(-600), true)).collect()),
            ],
        },
        rows,
    );
    // `(b − a)·(d − c)` of 2D segments, square to each other for the
    // expansions.
    let dot2 = |k: u64, scale: f64, zero: bool| {
        let p = |i: u64| {
            let g = grid(4 * k + i) * scale;
            [g.x, g.y]
        };
        let (a, b, c) = (p(0), p(1), p(2));
        let d = if zero {
            [c[0] - (b[1] - a[1]), c[1] + (b[0] - a[0])]
        } else {
            let g = general(k) * scale;
            [g.x, g.y]
        };
        Dot2d([a, b, c, d])
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "dot 2D",
            kind: PredKind::Orient2,
            stages: [
                Some((0..n).map(|k| dot2(k, 1.0, false)).collect()),
                Some((0..n).map(|k| dot2(k, 1.0, true)).collect()),
                Some((0..n).map(|k| dot2(k, pow2(-600), true)).collect()),
            ],
        },
        rows,
    );
    // A distance against a radius, `r² − ‖w − q‖²`, as the point–vertex
    // records' near test forms it ([`NearGap`]): `r = ‖w − q‖` exactly for
    // the expansions (`w − q = (3, 4, 12)·s`, `r = 13·s`).
    let near = |k: u64, zero: bool| {
        let (q, s) = (grid(k), dyadic(k + 3, 6) + 0.5);
        let (w, r) = if zero {
            (q + DVec3::new(3.0, 4.0, 12.0) * s, 13.0 * s)
        } else {
            (general(k + 1), dyadic(k + 5, 8) * 100.0)
        };
        NearGap {
            q,
            w,
            r: Rational::try_from(r).expect("a finite radius"),
        }
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "near gap",
            kind: PredKind::Orient2,
            stages: [
                Some((0..n).map(|k| near(k, false)).collect()),
                Some((0..n).map(|k| near(k, true)).collect()),
                None,
            ],
        },
        rows,
    );
    // A point against a line's shadow: `q = a + ½(b − a) + c·UP`.
    let line = |k: u64, scale: f64, zero: bool| {
        let (a, b) = (grid(3 * k) * scale, grid(3 * k + 1) * scale);
        let q = if zero {
            a + (b - a) * 0.5 + UP * (dyadic(k, 4) * scale)
        } else {
            general(k) * scale
        };
        LineShadow {
            q: fixed(q),
            line: Seg {
                a,
                b,
                rates: [Rate::ZERO; ORDERS],
            },
        }
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "line shadow",
            kind: PredKind::Orient3,
            stages: [
                Some((0..n).map(|k| line(k, 1.0, false)).collect()),
                Some((0..n).map(|k| line(k, 1.0, true)).collect()),
                Some((0..n).map(|k| line(k, pow2(-600), true)).collect()),
            ],
        },
        rows,
    );
    // Orient3: four points, `d = a + ½(b − a) + ¼(c − a)` on the
    // plane; for the rationals points on `z = x + y` as small as
    // `2⁻⁴⁰⁰`.
    let orient3 = |k: u64, zero: bool| {
        let [a, b, c] = [0, 1, 2].map(|i| grid(4 * k + i));
        let d = if zero {
            a + (b - a) * 0.5 + (c - a) * 0.25
        } else {
            general(k)
        };
        Orient3d([a, b, c, d])
    };
    let tiny = |k: u64| {
        let t = (k + 1) as f64 * pow2(-400);
        let on = |x: f64, y: f64| DVec3::new(x, y, x + y);
        Orient3d([
            on(t, 0.0),
            on(0.0, t),
            on(2.0 * t, t),
            on(1.0 + k as f64, 1.0),
        ])
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "orient 3D",
            kind: PredKind::Orient3,
            stages: [
                Some((0..n).map(|k| orient3(k, false)).collect()),
                Some((0..n).map(|k| orient3(k, true)).collect()),
                Some((0..16).map(tiny).collect()),
            ],
        },
        rows,
    );
    // The side of a line through `p` along `d` a segment passes,
    // `det(a − p, b − p, d)`: `d` in the plane of the three for the
    // expansions, all scaled by `2⁻⁴⁰⁰` for the rationals.
    let across = |k: u64, scale: f64, zero: bool| {
        let [p, a, b] = [0, 1, 2].map(|i| grid(4 * k + i) * scale);
        let d = if zero {
            (a - p) * 0.5 + (b - p) * 0.25
        } else {
            general(k) * scale
        };
        Orient3Dir { p, a, b, d }
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "orient 3D along",
            kind: PredKind::Orient3,
            stages: [
                Some((0..n).map(|k| across(k, 1.0, false)).collect()),
                Some((0..n).map(|k| across(k, 1.0, true)).collect()),
                Some((0..n).map(|k| across(k, pow2(-400), true)).collect()),
            ],
        },
        rows,
    );
    // A plane's height above a point: `c = n·q` exactly on the plane.
    let plane = |k: u64, zero: bool| {
        let (nrm, q) = (grid(k) / 200.0 + DVec3::Z, grid(k + 5));
        let c = if zero { nrm.dot(q) } else { general(k).x };
        PlaneHeight {
            normal: nrm,
            offset: c,
            rates: [Rate::ZERO; ORDERS],
            q: fixed(q),
        }
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "plane height",
            kind: PredKind::Orient3,
            stages: [
                Some((0..n).map(|k| plane(k, false)).collect()),
                Some((0..n).map(|k| plane(k, true)).collect()),
                None,
            ],
        },
        rows,
    );
    // Crossing: `g₀ = e₀ + ½(e₁ − e₀) + ¼·dg` in the plane of `e` and
    // `dg`; for the rationals scaled by `2⁻⁴⁰⁰`.
    let crossing = |k: u64, scale: f64, zero: bool| {
        let (e0, e1, dg) = (
            grid(4 * k) * scale,
            grid(4 * k + 1) * scale,
            grid(4 * k + 2) * scale,
        );
        let g0 = if zero {
            e0 + (e1 - e0) * 0.5 + dg * 0.25
        } else {
            general(k) * scale
        };
        let seg = |a, b| Seg {
            a,
            b,
            rates: [Rate::ZERO; ORDERS],
        };
        EdgeEdge {
            e: seg(e0, e1),
            g: seg(g0, g0 + dg),
        }
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "edge-edge",
            kind: PredKind::Crossing,
            stages: [
                Some((0..n).map(|k| crossing(k, 1.0, false)).collect()),
                Some((0..n).map(|k| crossing(k, 1.0, true)).collect()),
                Some((0..n).map(|k| crossing(k, pow2(-400), true)).collect()),
            ],
        },
        rows,
    );
    // A gap of two lines on one vertical plane ([`LineGap`]): `P` the
    // middle of `e`, `g₀ = P + ½·d + ¼·dg` in the plane of `d` and `dg`;
    // for the rationals scaled by `2⁻⁴⁰⁰`.
    let gap = |k: u64, scale: f64, zero: bool| {
        let [e0, e1, dg, d] = [0, 1, 2, 3].map(|i| grid(5 * k + i) * scale);
        let g0 = if zero {
            e0 + (e1 - e0) * 0.5 + d * 0.5 + dg * 0.25
        } else {
            general(k) * scale
        };
        LineGap {
            e: [e0, e1],
            t: 0.5,
            g: [g0, g0 + dg],
            d,
        }
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "line gap",
            kind: PredKind::Crossing,
            stages: [
                Some((0..n).map(|k| gap(k, 1.0, false)).collect()),
                Some((0..n).map(|k| gap(k, 1.0, true)).collect()),
                Some((0..n).map(|k| gap(k, pow2(-400), true)).collect()),
            ],
        },
        rows,
    );
    // A place on a line against a point ([`LinePlace`], degree 3, an
    // `Orient3`): `p` the middle of `e` moved across `dir` (`⅛·(d_y,
    // −d_x, 0)`, so the terms cancel in the sum only); for the rationals
    // scaled by `2⁻⁶⁰⁰`.
    let place = |k: u64, scale: f64, zero: bool| {
        let [e0, e1, dir] = [0, 1, 2].map(|i| grid(3 * k + i) * scale);
        let p = if zero {
            e0 + (e1 - e0) * 0.5 + DVec3::new(dir.y, -dir.x, 0.0) * 0.125
        } else {
            general(k) * scale
        };
        LinePlace {
            dir,
            e: [e0, e1],
            t: 0.5,
            p,
        }
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "line place",
            kind: PredKind::Orient3,
            stages: [
                Some((0..n).map(|k| place(k, 1.0, false)).collect()),
                Some((0..n).map(|k| place(k, 1.0, true)).collect()),
                Some((0..n).map(|k| place(k, pow2(-600), true)).collect()),
            ],
        },
        rows,
    );
    // A stored segment's side of an exact corner ([`Orient2Exact`]): the
    // middle for the expansions, `1/1031` of the way for the rationals
    // (not dyadic: the coordinates' differences are below 1031 times
    // their grid step).
    let corner = |k: u64, rational: bool, zero: bool| {
        let p = |i: u64| {
            let g = grid(3 * k + i);
            [g.x, g.y]
        };
        let (a, b) = (p(0), p(1));
        let r = |x: f64| Rational::try_from(x).expect("finite");
        let c = if !zero {
            let g = general(k);
            [r(g.x), r(g.y)]
        } else {
            let f = if rational {
                Rational::from(1u32) / Rational::from(1031u32)
            } else {
                Rational::from(1u32) / Rational::from(2u32)
            };
            [0, 1].map(|i| r(a[i]) + &f * (r(b[i]) - r(a[i])))
        };
        Orient2Exact { a, b, c }
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "orient 2D exact",
            kind: PredKind::Orient2,
            stages: [
                Some((0..n).map(|k| corner(k, false, false)).collect()),
                Some((0..n).map(|k| corner(k, false, true)).collect()),
                Some((0..n).map(|k| corner(k, true, true)).collect()),
            ],
        },
        rows,
    );
    // The shared-curve crossing at a stored point ([`SharedCrossing`]):
    // the rate along the chord in the shadow (`m·(b − a) + c·UP`) for
    // the expansions, that over the prime 1000003 (exact, not dyadic: its
    // components are below 1000003 times their grid step) for the
    // rationals.
    let chords: Vec<[DVec3; 2]> = (0..n).map(|k| [grid(2 * k), grid(2 * k + 1)]).collect();
    let shared_rate = |k: u64, rational: bool, zero: bool| {
        let [a, b] = chords[k as usize];
        if !zero {
            return Rate::Float(general(k));
        }
        let d = (b - a) * (dyadic(k + 3, 4) + 1.0) + UP * dyadic(k + 5, 4);
        if rational {
            let r = |x: f64| Rational::try_from(x).expect("finite") / Rational::from(1_000_003u32);
            Rate::exact([r(d.x), r(d.y), r(d.z)])
        } else {
            Rate::Float(d)
        }
    };
    let shared_rates: [Vec<Rate>; 3] = [(false, false), (false, true), (true, true)]
        .map(|(rational, zero)| (0..n).map(|k| shared_rate(k, rational, zero)).collect());
    fn shared<'a>(chords: &[[DVec3; 2]], rates: &'a [Rate]) -> Vec<SharedCrossing<'a>> {
        chords
            .iter()
            .zip(rates)
            .map(|(&chord, rate)| SharedCrossing { chord, rate })
            .collect()
    }
    exact_pred_rows(
        budget,
        Fixtures {
            name: "shared crossing",
            kind: PredKind::Orient3,
            stages: [
                Some(shared(&chords, &shared_rates[0])),
                Some(shared(&chords, &shared_rates[1])),
                Some(shared(&chords, &shared_rates[2])),
            ],
        },
        rows,
    );
    // Hom: the shadow rows against points of weighted quadratics at
    // their middle ([`hom_at`]), each placed on its zero for the
    // expansions.
    let hom = |k: u64, zero: bool| {
        let (p, e) = (grid(k), grid(k + 3) / 8.0);
        let a = dyadic(k + 7, 8);
        let q = if zero {
            p + DVec3::new(a, 0.5, 16.0 * a)
        } else {
            general(k)
        };
        HomShadow {
            q: fixed(q),
            w: hom_at(p, e, 3),
            axis: ShadowAxis::Along,
        }
    };
    let pair = |k: u64, zero: bool| {
        let (p, e) = (grid(k), grid(k + 3) / 8.0);
        let a = dyadic(k + 7, 8);
        let d = if zero {
            DVec3::new(a, 0.5, 16.0 * a)
        } else {
            general(k) - p
        };
        HomPairShadow {
            q: hom_at(p, e, 3),
            w: hom_at(p + d, e, 3),
            axis: ShadowAxis::Along,
        }
    };
    let hline = |k: u64, scale: f64, zero: bool| {
        let (pa, pb) = (grid(3 * k) * scale, grid(3 * k + 1) * scale);
        let e = grid(k + 5) / 8.0 * scale;
        let q = if zero {
            pa + (pb - pa) * 0.5 + UP * (dyadic(k, 4) * scale)
        } else {
            general(k) * scale
        };
        HomLineShadow {
            q: fixed(q),
            a: hom_at(pa, e, 3),
            b: hom_at(pb, e, 5),
        }
    };
    let hpoint_line = |k: u64, zero: bool| {
        let (pa, pb) = (grid(3 * k), grid(3 * k + 1));
        let e = grid(k + 5) / 8.0;
        let pq = if zero {
            pa + (pb - pa) * 0.5 + UP * dyadic(k, 4)
        } else {
            general(k)
        };
        HomPointLineShadow {
            q: hom_at(pq, e, 7),
            a: hom_at(pa, e, 3),
            b: hom_at(pb, e, 5),
        }
    };
    exact_pred_rows(
        budget,
        Fixtures {
            name: "hom shadow",
            kind: PredKind::Hom,
            stages: [
                Some((0..n).map(|k| hom(k, false)).collect()),
                Some((0..n).map(|k| hom(k, true)).collect()),
                None,
            ],
        },
        rows,
    );
    exact_pred_rows(
        budget,
        Fixtures {
            name: "hom pair shadow",
            kind: PredKind::Hom,
            stages: [
                Some((0..n).map(|k| pair(k, false)).collect()),
                Some((0..n).map(|k| pair(k, true)).collect()),
                None,
            ],
        },
        rows,
    );
    exact_pred_rows(
        budget,
        Fixtures {
            name: "hom line shadow",
            kind: PredKind::Hom,
            stages: [
                Some((0..n).map(|k| hline(k, 1.0, false)).collect()),
                Some((0..n).map(|k| hline(k, 1.0, true)).collect()),
                Some((0..n).map(|k| hline(k, pow2(-560), true)).collect()),
            ],
        },
        rows,
    );
    exact_pred_rows(
        budget,
        Fixtures {
            name: "hom point-line shadow",
            kind: PredKind::Hom,
            stages: [
                Some((0..n).map(|k| hpoint_line(k, false)).collect()),
                Some((0..n).map(|k| hpoint_line(k, true)).collect()),
                None,
            ],
        },
        rows,
    );
    area_rows(budget, rows);
}

/// A polygon's signed area ([`polygon_area_sign`]), a predicate of
/// variable length charged per term at `Orient2`, at 4 and 32 corners:
/// a convex polygon about a grid point for the filter; corners on one
/// line through it (`a + s·d`, `s` on a grid) for the expansions; those
/// scaled by `2⁻⁶⁰⁰` for the rationals.
fn area_rows(budget: Duration, rows: &mut Vec<Row>) {
    let polygon = |k: u64, corners: u64, scale: f64, zero: bool| -> Vec<[f64; 2]> {
        let (c, d) = (grid(2 * k), grid(2 * k + 1) / 8.0);
        (0..corners)
            .map(|i| {
                let p = if zero {
                    c + d * dyadic(k * corners + i, 6)
                } else {
                    let a = std::f64::consts::TAU * i as f64 / corners as f64;
                    c + DVec3::new(trig::cos(a), trig::sin(a), 0.0) * (10.0 + dyadic(k + i, 4))
                };
                [p.x * scale, p.y * scale]
            })
            .collect()
    };
    for corners in [4, 32] {
        let stages = [(1.0, false), (1.0, true), (pow2(-600), true)]
            .map(|(scale, zero)| (0..64).map(|k| polygon(k, corners, scale, zero)).collect());
        for (stage, polys) in stages.iter().enumerate() {
            let polys: &Vec<Vec<[f64; 2]>> = polys;
            let work = polys
                .iter()
                .fold(exact::Work::default(), |w, p| w.and(polygon_area_sign(p).1));
            let r = work.of(PredKind::Orient2);
            let ran = [r.filters, r.expansions, r.rationals];
            let want = polys.len() as u32 * corners as u32;
            assert!(
                ran[stage] == want && ran[stage + 1..].iter().all(|&n| n == 0),
                "polygon area {corners} {}: {work:?}",
                STAGES[stage]
            );
            let ns = time(budget, || {
                for p in polys {
                    let _ = black_box(polygon_area_sign(p));
                }
                polys.len() as u64
            });
            rows.push(Row {
                kind: format!("exact Orient2 {}, polygon area {corners}", STAGES[stage]),
                ns,
                units: parts(&Cost::exact(work)) / polys.len() as f64,
            });
        }
    }
}

/// Polynomials of degree 4 on `[0, 1]`, their roots near `¼` and `¾`
/// or none, at `like`'s level.
fn quartics<S: Scalar>(like: &S) -> Vec<Poly<S>> {
    (0..16)
        .map(|k| {
            let s = 1.0 + k as f64 / 16.0;
            // `(t − a)(t − b)(t² + s)` in Bernstein form, from its
            // values' power form.
            let (a, b) = (0.25 + k as f64 / 512.0, 0.75 - k as f64 / 1024.0);
            let power = poly_mul(&poly_mul(&[-a, 1.0], &[-b, 1.0]), &[s, 0.0, 1.0]);
            let bern = to_bernstein(&power);
            Poly::new(bern.iter().map(|&c| S::from_f64(c, like))).unwrap()
        })
        .collect()
}

fn poly_mul(a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] += x * y;
        }
    }
    out
}

/// Power-form coefficients to Bernstein ones of the same degree.
fn to_bernstein(p: &[f64]) -> Vec<f64> {
    let n = p.len() - 1;
    let binom = |n: usize, k: usize| (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64);
    (0..=n)
        .map(|i| (0..=i).map(|j| binom(i, j) / binom(n, j) * p[j]).sum())
        .collect()
}

/// Curved record work at one level: rows evaluated (a quartic at a
/// point, with its sign) and root isolation (its splits, tests and
/// steps).
fn curved_rows<S: LevelScalar>(budget: Duration, like: &S, rows: &mut Vec<Row>) {
    let level = like.level().index();
    let polys = quartics(like);
    let ts: Vec<S> = (0..16)
        .map(|k| S::from_f64(k as f64 / 15.0, like))
        .collect();
    let ns = time(budget, || {
        for p in &polys {
            for t in &ts {
                black_box(p.eval(t).sign());
            }
        }
        (polys.len() * ts.len()) as u64
    });
    let mut evals = Cost::default();
    evals.levels[level] = LevelWork {
        evals: 1,
        ..LevelWork::default()
    };
    rows.push(Row {
        kind: format!("record eval L{level}"),
        ns,
        units: parts(&evals),
    });
    let limits = varde_decide::bezier::roots::Limits {
        max_depth: 40,
        max_splits: 4096,
    };
    let mut roots = Cost::default();
    for p in &polys {
        let r = varde_decide::bezier::roots::isolate(p, limits).expect("isolated");
        roots.levels[level].roots = roots.levels[level].roots.and(r.work);
    }
    let ns = time(budget, || {
        for p in &polys {
            black_box(varde_decide::bezier::roots::isolate(p, limits).ok());
        }
        1
    });
    let w = roots.levels[level].roots;
    let ops = u64::from(w.splits) + u64::from(w.tests) + u64::from(w.steps);
    rows.push(Row {
        kind: format!("record root op L{level}"),
        ns: ns / ops as f64,
        units: parts(&roots) / ops as f64,
    });
}

fn record_rows(budget: Duration, rows: &mut Vec<Row>) {
    exact_rows(budget, rows);
    for level in Level::ALL {
        at_level(&mut CurvedRows { budget, rows }, level, &mut ());
    }
}

/// [`curved_rows`] at one level.
struct CurvedRows<'r> {
    budget: Duration,
    rows: &'r mut Vec<Row>,
}

impl AtLevel<()> for CurvedRows<'_> {
    type Out = ();

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) {
        curved_rows(self.budget, like, self.rows);
    }

    fn escalates(_: &()) -> bool {
        false
    }
}

// ---------------------------------------------------------------------
// A face's counting setup, edge pairs' cluster searches, column hits.

/// A face's counting setup ([`Corners::new`]) per piece: a needle (a
/// triangle `2⁻⁵⁰` high, whose runs reach along it, thousands of pieces)
/// and a square (short runs at right angles). Then a sharp corner's
/// record, charged as [`CUSP_PIECES`] setup pieces: two sides `1/50`
/// rad apart.
fn setup_rows(budget: Duration, rows: &mut Vec<Row>) {
    let h = 1.0 / (1u64 << 50) as f64;
    let needle = vec![[1.25, 1.25], [1.75, 1.25], [1.5, 1.25 + h]];
    let square = vec![[1.25, 1.25], [1.75, 1.25], [1.75, 1.75], [1.25, 1.75]];
    for (kind, pts) in [
        ("face setup, needle", needle),
        ("face setup, square", square),
    ] {
        let mut order = Order::default();
        let face = polygon(&pts, BAND, &mut order);
        let build = || {
            let cost = face.corners_on(&order, &per_piece(u64::MAX)).1;
            u64::from(cost.levels[0].evals)
        };
        let n = build();
        let ns = time(budget, || {
            black_box(build());
            1
        }) / n as f64;
        rows.push(Row {
            kind: kind.into(),
            ns,
            units: UNITS.face_setup as f64,
        });
    }
    // The sharp corner at `(1.25, 1.25)` between a side to `(1.75, 1.26)`
    // (into the corner, run backwards) and one to `(1.75, 1.25)`, on the
    // plane's tiles (unit partial derivatives).
    let a: CuspSpan = [[1.25, 1.25, 1.0], [1.5, 1.255, 1.0], [1.75, 1.26, 1.0]];
    let b: CuspSpan = [[1.25, 1.25, 1.0], [1.5, 1.25, 1.0], [1.75, 1.25, 1.0]];
    let unit = |k: usize| std::array::from_fn(|i| Bounded::exact(if i == k { 1.0 } else { 0.0 }));
    let (du, dv) = (unit(0), unit(1));
    let bands = 2.0 * BAND;
    assert!(
        matches!(cusp_rec(&[a], &[b], &du, &dv, bands), Ok(Sharp::Cusp(_))),
        "a sharp corner with its record"
    );
    let ns = time(budget, || {
        black_box(cusp_rec(&[a], &[b], &du, &dv, bands).ok());
        1
    });
    rows.push(Row {
        kind: "cusp record".into(),
        ns,
        units: (CUSP_PIECES * UNITS.face_setup) as f64,
    });
}

/// An edge pair's cluster search per piece at each level: a conic whose
/// shadow touches a line's, where no box becomes a leaf, searched until
/// a share of 4096 pieces runs out (the four terms' records empty: the
/// search's work, not its answer, is timed).
fn xrec_rows(budget: Duration, rows: &mut Vec<Row>) {
    let lat = |x: f64, y: f64, z: f64| (R_HAT * x + NU * y + UP * z).to_array();
    let cp = |p: [f64; 3], w: f64| [p[0], p[1], p[2], w];
    let e: [Span; 1] = [[
        cp(lat(-1.0, 1.0, 0.0), 1.0),
        cp(lat(0.0, -1.0, 0.0), 1.0),
        cp(lat(1.0, 1.0, 0.0), 1.0),
    ]];
    let none = ChainRec::default();
    let off = EdgeRays::Line(RayRec { crossing: None });
    let pair = XPair {
        e: XCurve::Spans(&e),
        g: XCurve::Line(lat(-2.0, 0.0, 0.0), lat(0.0, 0.0, 0.0)),
        terms: XTerms {
            a_g: off,
            b_g: off,
            g0_e: EdgeRays::Chain(&none),
            g1_e: EdgeRays::Chain(&none),
        },
        shared: false,
    };
    let sides = XSides::new();
    let x = XKey {
        e: EdgeId(0),
        g: EdgeId(1),
    };
    for (level, units) in UNITS.xrec_piece.into_iter().enumerate() {
        let run = || clusters_at_charged(x, &pair, &sides, level, &per_piece(4096)).1;
        let n = run();
        let ns = time(budget, || {
            black_box(run());
            1
        });
        rows.push(Row {
            kind: format!("xrec piece L{level}"),
            ns: ns / n as f64,
            units: units as f64,
        });
    }
}

/// A column's hits on a face per tile and ribbon span searched: the
/// square on its four tiles, a column inside it.
fn hit_rows(budget: Duration, rows: &mut Vec<Row>) {
    let square = [[1.25, 1.25], [1.75, 1.25], [1.75, 1.75], [1.25, 1.75]];
    let mut order = Order::default();
    let data = polygon(&square, BAND, &mut order);
    let face = data.hit_face(&order).expect("a plane face");
    let searches = face.nets.len() + face.trims.iter().map(|t| t.pspans.len()).sum::<usize>();
    let motion = motion_of(Operand::A);
    let q = column(1.5, 1.4, -0.01);
    let found = face_hits(&face, q, &motion).expect("hits");
    assert_eq!(found.len(), 1, "one hit");
    let ns = time(budget, || {
        black_box(face_hits(&face, q, &motion).ok());
        1
    });
    rows.push(Row {
        kind: "column hit search".into(),
        ns: ns / searches as f64,
        units: UNITS.record.hit as f64 / UNIT_PARTS as f64,
    });
}

// ---------------------------------------------------------------------
// Record-evaluation kinds, on real record computations.

/// A pass deciding in order on one thread, summing the cost.
#[derive(Default)]
struct Seq {
    decided: u64,
    cost: Cost,
}

impl Pass for Seq {
    type Error = Fail;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, Fail> {
        keys.iter()
            .map(|k| {
                self.decided += 1;
                let (v, c) = decide(k);
                self.cost = self.cost.and(c);
                v
            })
            .collect()
    }
}

/// A row for records of `kind` whose decisions took `ns` each and
/// charged `cost` in all over `records`, and a line suggesting the
/// kind's own price: its count `ops` in the cost, the rest of the cost
/// at the table's prices taken off.
fn record_kind_row(
    rows: &mut Vec<Row>,
    kind: &str,
    ns: f64,
    (cost, records): (&Cost, u64),
    (ops, parts): (u64, u64),
) {
    let all = cost.parts(&UNITS.record) as f64 / UNIT_PARTS as f64 / records as f64;
    let own = (ops * parts) as f64 / UNIT_PARTS as f64 / records as f64;
    let per_op = (ns - (all - own) * UNIT_NS) / (ops as f64 / records as f64);
    println!(
        "  {kind}: {:.1} per record, {:.0} parts each now, about {:.0} parts measured",
        ops as f64 / records as f64,
        parts,
        per_op / UNIT_NS * UNIT_PARTS as f64
    );
    rows.push(Row {
        kind: format!("{kind} records"),
        ns,
        units: all,
    });
}

/// Foot polynomials, curved key values and end tests at the first
/// level, each on records that run them: the feet of points near an
/// arc, the ray records of points beside a circle's joints, and
/// point–curve heights read at a foot's parameter. (Their higher levels
/// keep the table's ratios to the first: no fixture here escalates.)
fn record_kind_rows(budget: Duration, rows: &mut Vec<Row>) {
    feet_rows(budget, rows);
    end_rows(budget, rows);
    key_rows(budget, rows);
}

fn feet_rows(budget: Duration, rows: &mut Vec<Row>) {
    let arc = quarter(1.0);
    let spans: Vec<Span> = arc.spans().to_vec();
    let r = 1e-3;
    let qs: Vec<DVec3> = (0..16)
        .map(|k| {
            let a = (k as f64 + 0.5) / 16.0 * std::f64::consts::FRAC_PI_2;
            DVec3::new(trig::cos(a), trig::sin(a), 0.0) * (50.0 + r * 0.5)
        })
        .collect();
    let key = |k: usize| RayKey {
        q: Point::vert(Operand::A, VertId(k as u32)),
        g: EdgeId(1),
    };
    let curve = EdgeCurve::Spans {
        spans: &spans,
        closed: false,
    };
    let mut cost = Cost::default();
    for (k, &q) in qs.iter().enumerate() {
        let (feet, c) = decide_feet(key(k), q, curve, r);
        assert!(!feet.expect("feet").feet.is_empty());
        cost = cost.and(c);
    }
    let ns = time(budget, || {
        for (k, &q) in qs.iter().enumerate() {
            black_box(decide_feet(key(k), q, curve, r).0.ok());
        }
        qs.len() as u64
    });
    let ops = u64::from(cost.levels[0].feet);
    let parts = UNITS.record.foot_poly[0];
    record_kind_row(
        rows,
        "foot_poly L0",
        ns,
        (&cost, qs.len() as u64),
        (ops, parts),
    );
}

/// Curved edges of `B` and points of `A` moving along `d`: the ray
/// records' geometry.
struct Rays {
    pos: BTreeMap<Point, DVec3>,
    edges: BTreeMap<EdgeId, ([VertId; 2], Vec<Span>)>,
    d: DVec3,
}

impl ShadowGeometry for Rays {
    fn point(&self, p: Point) -> Moving {
        match p.operand {
            Operand::A => Moving::of_a(self.pos[&p], Sign::Pos, self.d),
            Operand::B => Moving::fixed(self.pos[&p]),
        }
    }

    fn near_radius(&self, _: Point) -> f64 {
        BAND
    }

    fn ends(&self, _: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&g].0
    }

    fn line(&self, _: Point, _: EdgeId) -> Seg {
        unreachable!("only curved edges here")
    }
}

impl CurveGeometry for Rays {
    fn spans(&self, _: Operand, g: EdgeId) -> &[Span] {
        &self.edges[&g].1
    }

    fn knot_rates(&self, _: Point, _: EdgeId, _: u32) -> [Rate; ORDERS] {
        [Rate::ZERO; ORDERS]
    }

    fn span_rates<S: Curved>(&self, _: Point, _: EdgeId, _: u32, t: &S) -> [[S; 3]; ORDERS] {
        std::array::from_fn(|_| [0.0; 3].map(|x| S::from_f64(x, t)))
    }
}

/// A circle of radius 2 in the shadow frame, of two edges of two spans,
/// and points of `A` beside its joints' columns and `r̂` lines: their
/// ray records test the crossings beside the joints' ends.
fn end_rows(budget: Duration, rows: &mut Vec<Row>) {
    let lat = |x: f64, y: f64| R_HAT * x + NU * y;
    let cp = |p: DVec3, w: f64| [p.x, p.y, p.z, w];
    // Squashed along `ν`, as an ellipse in the shadow.
    let at = |a: f64, r: f64| lat(r * trig::cos(a), r * trig::sin(a) / 8.0);
    let step = std::f64::consts::FRAC_PI_2;
    let w = trig::cos(step / 2.0);
    let start = 0.3;
    let ends: Vec<[f64; 4]> = (0..=4)
        .map(|k| {
            // Dyadic joints, so the points beside them are exact.
            let p = at(start + step * (k % 4) as f64, 2.0);
            let r = |x: f64| (x * 64.0).round() / 64.0;
            cp(DVec3::new(r(p.x), r(p.y), r(p.z)), 1.0)
        })
        .collect();
    let spans: Vec<Span> = (0..4)
        .map(|k| {
            let mid = at(start + step * (k as f64 + 0.5), 2.0 / w);
            [ends[k], cp(mid, w), ends[k + 1]]
        })
        .collect();
    let joint = |s: &Span| DVec3::new(s[0][0], s[0][1], s[0][2]);
    let mut geom = Rays {
        pos: BTreeMap::new(),
        edges: BTreeMap::new(),
        d: DVec3::new(0.25, -0.5, 0.125),
    };
    geom.pos
        .insert(Point::vert(Operand::B, VertId(1)), joint(&spans[0]));
    geom.pos
        .insert(Point::vert(Operand::B, VertId(2)), joint(&spans[2]));
    geom.edges
        .insert(EdgeId(0), ([VertId(1), VertId(2)], spans[0..2].to_vec()));
    let back: Vec<Span> = spans[2..4]
        .iter()
        .rev()
        .map(|s| [s[2], s[1], s[0]])
        .collect();
    geom.edges.insert(EdgeId(1), ([VertId(1), VertId(2)], back));
    let mut qs = Vec::new();
    for s in &spans {
        for (l, m) in [(0.5, 0.0), (-0.5, 1.0), (0.0, 0.0), (0.0, -2.0), (3.0, 1.0)] {
            let q = Point::vert(Operand::A, VertId(100 + qs.len() as u32));
            geom.pos.insert(q, joint(s) + R_HAT * l + UP * m);
            qs.push(q);
        }
    }
    let keys: Vec<SpanKey> = qs
        .iter()
        .flat_map(|&q| {
            [(EdgeId(0), 2), (EdgeId(1), 2)]
                .into_iter()
                .flat_map(move |(g, n)| (0..n).map(move |span| SpanKey { q, g, span }))
        })
        .collect();
    let band = Band::curved(0.0, 1.0 / 1_073_741_824.0).expect("a band");
    let fill = |seq: &mut Seq| {
        let (mut r, mut sh) = (CurvedRays::new(), Shadows::new());
        r.fill(keys.iter().copied(), &mut sh, &geom, band, seq)
            .expect("decided");
    };
    let mut seq = Seq::default();
    fill(&mut seq);
    let ops = u64::from(seq.cost.levels[0].ends);
    assert!(ops > 0, "end tests ran");
    let records = seq.decided;
    let ns = time(budget, || {
        fill(&mut Seq::default());
        records
    });
    let parts = UNITS.record.at_end[0];
    record_kind_row(rows, "at_end L0", ns, (&seq.cost, records), (ops, parts));
}

/// Points of `A` above an arc, each with the height key of its foot on
/// it, the arc's point read at the foot's parameter at the level: a
/// curved key's value, formed from level `from` on (the levels before
/// escalate at once, each call counted per key).
struct Keys {
    arc: Span,
    pts: Vec<(DVec3, f64)>,
    from: usize,
    calls: Mutex<BTreeMap<u32, usize>>,
}

impl HeightGeometry for Keys {
    type Flat = Orient3d;

    fn value(&self, _: HKey) -> KeyValue<Orient3d> {
        KeyValue::Curved
    }

    fn curved_value<S: Curved>(&self, h: HKey, like: &S, _: &mut LevelWork) -> Option<S> {
        let HKey::Curve { q, .. } = h else {
            return None;
        };
        let Query::Vert(v) = q.q else {
            return None;
        };
        let mut calls = self.calls.lock().expect("a lock");
        let n = calls.entry(v.0).or_default();
        *n += 1;
        if *n <= self.from {
            return None;
        }
        drop(calls);
        let (at, t) = *self.pts.get(v.index())?;
        let c = HomCurve::from_affine(self.arc.map(|p| ([p[0], p[1], p[2]], p[3])), like)?;
        let x = c.eval(&S::from_f64(t, like));
        let up = [UP.x, UP.y, UP.z].map(|u| S::from_f64(u, like));
        let len = up
            .iter()
            .fold(S::int(0, like), |s, u| s.add(&u.mul(u)))
            .sqrt();
        let at = [at.x, at.y, at.z].map(|v| S::from_f64(v, like));
        let d = (0..3).fold(S::int(0, like), |s, i| s.add(&up[i].mul(&x[i].sub(&at[i]))));
        Some(d.div(&len))
    }

    fn row(&self, _: HeightKey) -> AnchorRow<Orient3d> {
        AnchorRow::Curved
    }

    fn curved_row<S: Curved>(&self, _: HeightKey, _: &S, _: &mut LevelWork) -> Option<[S; ORDERS]> {
        None
    }

    fn geometric(&self, _: HeightKey) -> bool {
        false
    }
}

fn key_rows(budget: Duration, rows: &mut Vec<Row>) {
    let arc = quarter(1.0).spans()[0];
    let pts: Vec<(DVec3, f64)> = (0..16)
        .map(|k| {
            let t = (k as f64 + 0.5) / 16.0;
            let a = t * std::f64::consts::FRAC_PI_2;
            (DVec3::new(50.0 * trig::cos(a), 50.0 * trig::sin(a), 0.5), t)
        })
        .collect();
    let keys: Vec<HeightKey> = (0..pts.len() as u32)
        .map(|v| HeightKey {
            h: HKey::Curve {
                q: Point::vert(Operand::A, VertId(v)),
                g: EdgeId(1),
                foot: FootId(0),
            },
            anchor: Anchor::Edge(Operand::B, EdgeId(1)),
        })
        .collect();
    let band = Band::curved(0.0, 1.0 / 1_073_741_824.0).expect("a band");
    for level in 0..LEVELS {
        let fill = |seq: &mut Seq| {
            let geom = Keys {
                arc,
                pts: pts.clone(),
                from: level,
                calls: Mutex::new(BTreeMap::new()),
            };
            Heights::new()
                .fill(keys.iter().copied(), &geom, band, seq)
                .expect("decided");
        };
        let mut seq = Seq::default();
        fill(&mut seq);
        let ops = u64::from(seq.cost.levels[level].keys);
        assert!(ops > 0, "values formed at level {level}");
        let records = seq.decided;
        let ns = time(budget, || {
            fill(&mut Seq::default());
            records
        });
        let parts = UNITS.record.key_at[level];
        let kind = format!("key_at L{level}");
        record_kind_row(rows, &kind, ns, (&seq.cost, records), (ops, parts));
    }
}

// ---------------------------------------------------------------------
// The label audit.

/// The cuboid `[lo, hi]` as twelve triangles facing out.
fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Vec<Tri> {
    let c =
        |x: usize, y: usize, z: usize| [[lo[0], hi[0]][x], [lo[1], hi[1]][y], [lo[2], hi[2]][z]];
    let quads = [
        [c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0)],
        [c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1)],
        [c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1)],
        [c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0)],
        [c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0)],
        [c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)],
    ];
    quads
        .iter()
        .flat_map(|[a, b, c, d]| [[*a, *b, *c], [*a, *c, *d]])
        .collect()
}

/// The near/far certificate of an audit square, per piece: 256 squares
/// 1 mm across inside a square region 2048 mm across on the plane `z =
/// 0`, classified alone (the square row, its loop tests and the exact
/// predicates of its inside test included), then against 192 triangles
/// (16 cuboids above and beside the squares: feet inside some, outside
/// others), 256 ribbons, and a region of 1024 sides with 1024 leaves
/// apart from the squares: each piece's time is the difference over its
/// count. Then queries at a square's centre against the cuboids, timed
/// whole against their predicates' units: the 3D determinants' row on
/// the audit's own predicates.
fn audit_rows(budget: Duration, rows: &mut Vec<Row>) {
    let (tile, uv) = audit::plane();
    let region = |pts: &[[f64; 2]]| AuditRegion {
        loops: vec![pts.iter().map(|&p| uv(p)).collect()],
        winding: 0,
    };
    let s = 1024.0;
    let square = region(&[[-s, -s], [s, -s], [s, s], [-s, s]]);
    let gon: Vec<[f64; 2]> = (0..1024)
        .map(|k| {
            let a = k as f64 / 1024.0 * std::f64::consts::TAU;
            [s * trig::cos(a), s * trig::sin(a)]
        })
        .collect();
    let many = region(&gon);
    let solid: Vec<Tri> = (0..16)
        .flat_map(|k| {
            let (x, y) = ((k % 4) as f64 * 8.0 - 12.0, (k / 4) as f64 * 8.0 - 12.0);
            cuboid([x, y, 2.0 + k as f64], [x + 6.0, y + 6.0, 4.0 + k as f64])
        })
        .collect();
    let ribbons: Vec<Ribbon> = (0..256)
        .map(|k| {
            let (x, y) = ((k % 16) as f64 - 8.0, (k / 16) as f64 - 8.0);
            Ribbon {
                ends: [[x, y, 1.0], [x + 0.5, y + 0.25, 1.5]],
                beta: 1e-6,
            }
        })
        .collect();
    let apart: Vec<Leaf> = (0..1024)
        .map(|k| {
            let [p, q] = [
                uv([1100.0 + k as f64, 1100.0]),
                uv([1100.5 + k as f64, 1100.5]),
            ];
            let lo = |k: usize| if q[k] < p[k] { q[k] } else { p[k] };
            let hi = |k: usize| if q[k] > p[k] { q[k] } else { p[k] };
            Leaf {
                uv: [[lo(0), lo(1)], [hi(0), hi(1)]],
                cover: Cover::Near,
                partner: k,
            }
        })
        .collect();
    let cells: Vec<(u32, u64, u64)> = (0..256)
        .map(|k| audit::cell_at(uv([(k % 16) as f64 - 8.0, (k / 16) as f64 - 8.0]), 22))
        .collect();
    let bands = Bands {
        eps_aud: 1.0 / 2048.0,
        rho_tie: 1.0 / 16384.0,
    };
    let input = |regions: &[AuditRegion], leaves: &[Leaf], ribbons: &[Ribbon], solid: &[Tri]| {
        // Each fixture's squares classified: the time per square and
        // the units per square.
        let input = AuditInput {
            tile: &tile,
            regions,
            leaves,
            ribbons,
            solid,
            before: &[],
            bands,
        };
        let units = audit::squares(&input, 0, &cells).expect("classified");
        let ns = time(budget, || {
            black_box(audit::squares(&input, 0, &cells));
            cells.len() as u64
        });
        (ns, units as f64 / cells.len() as f64)
    };
    let alone = std::slice::from_ref(&square);
    let (base, base_units) = input(alone, &[], &[], &[]);
    // The square's own price: its inside test's predicates timed apart
    // and taken off (its loop tests are a few nanoseconds).
    let preds: Vec<Orient2d> = cells
        .iter()
        .flat_map(|&(level, i, j)| {
            let n = (1u64 << level) as f64;
            let c = [(i as f64 + 0.5) / n, (j as f64 + 0.5) / n];
            let l = &square.loops[0];
            (0..l.len())
                .map(move |k| (l[k], l[(k + 1) % l.len()]))
                .filter(move |(a, b)| (a[1] > c[1]) != (b[1] > c[1]))
                .map(move |(a, b)| Orient2d([a, b, c]))
        })
        .collect();
    let per_square = preds.len() as f64 / cells.len() as f64;
    let pred_ns = time(budget, || {
        for p in &preds {
            let _ = black_box(exact::value_sign(p));
        }
        preds.len() as u64
    });
    println!(
        "  audit square: {} parts each now, about {:.0} parts measured \
         ({per_square} inside-test predicates of {pred_ns:.0} ns each taken off)",
        UNITS.audit.square,
        (base - per_square * pred_ns) / UNIT_NS * UNIT_PARTS as f64
    );
    rows.push(Row {
        kind: "audit square".into(),
        ns: base,
        units: base_units,
    });
    let part = |n: usize, p: u64| (n as f64, p as f64 / UNIT_PARTS as f64);
    let u = UNITS.audit;
    let tests = 2 * (gon.len() - 4) + apart.len();
    for (kind, (ns, _), (n, units)) in [
        (
            "audit triangle",
            input(alone, &[], &[], &solid),
            part(solid.len(), u.triangle),
        ),
        (
            "audit ribbon",
            input(alone, &[], &ribbons, &[]),
            part(ribbons.len(), u.ribbon),
        ),
        (
            "audit test",
            input(std::slice::from_ref(&many), &apart, &[], &[]),
            part(tests, u.test),
        ),
    ] {
        rows.push(Row {
            kind: kind.into(),
            ns: (ns - base) / n,
            units,
        });
    }
    // Queries from inside the third cuboid's column, and from between the
    // cuboids, each along every direction.
    for (kind, p) in [
        ("audit query, inside", [-12.0 + 3.0 * 8.0 / 8.0, -10.0, 4.5]),
        ("audit query, outside", [-13.0, -13.0, 0.0]),
    ] {
        let mut units = 0;
        for k in 0..3 {
            let (w, n) = audit::query(&solid, p, k);
            assert!(w.is_some(), "{kind}: settled along direction {k}");
            units += n;
        }
        let ns = time(budget, || {
            for k in 0..3 {
                black_box(audit::query(&solid, p, k));
            }
            3
        });
        // Not a kind of its own: a query costs its predicates, charged at
        // their kinds' rows, so the row checks those prices on the
        // audit's own predicates.
        rows.push(Row {
            kind: kind.into(),
            ns,
            units: units as f64 / 3.0,
        });
    }
}

// ---------------------------------------------------------------------
// Operand scan.

/// The operation-start scan of an operand, per position and per net: a
/// body of 4096 vertices on a helix joined by line edges (a vertex, an
/// edge and its curve's three control points a link), scanned whole and
/// timed against its units per position; then a body of 16 planes of 256
/// tiles each, the time of its positions (a form each) at the first
/// row's taken off, timed per net.
fn scan_rows(budget: Duration, rows: &mut Vec<Row>) {
    let u = UNITS.operand_scan;
    let n = 4096;
    let mut chain = Brep::new();
    let verts: Vec<VertId> = (0..n)
        .map(|k| {
            let a = k as f64 / 64.0;
            let p = [100.0 * trig::cos(a), 100.0 * trig::sin(a), k as f64 / 8.0];
            chain.add_vert(p).expect("a vertex")
        })
        .collect();
    for w in verts.windows(2) {
        let (a, z) = (chain.pos(w[0]), chain.pos(w[1]));
        let mid = [0, 1, 2].map(|k| a[k] * 0.5 + z[k] * 0.5);
        let line = vec![[
            [a[0], a[1], a[2], 1.0],
            [mid[0], mid[1], mid[2], 1.0],
            [z[0], z[1], z[2], 1.0],
        ]];
        let c = chain
            .add_curve(Curve3D::new(line, vec![0.0, 1.0]).expect("a line"))
            .expect("a curve");
        chain.add_edge(c, [0.0, 1.0], w[0], w[1]).expect("an edge");
    }
    let mut planes = Brep::new();
    for k in 0..16 {
        let key = PlaneKey::new([0.0, 0.0, 1.0], [0.0, 0.0, k as f64]).expect("a plane");
        let lat = Lattice::new(key.into()).expect("a lattice");
        let rect = Rect {
            level: TileLevel { u: 4, v: 4 },
            u: 0..16,
            v: 0..16,
        };
        let s = Surface::new(lat, rect).expect("a surface");
        planes.add_surface(s).expect("a surface added");
    }
    let timed = |b: &Brep| {
        let (out, units) = scan::op_start_on(&[b], 1e-3, MAX_WORK);
        assert!(out.is_ok(), "the body passes its scan");
        assert_eq!(units, b.scan_size().units());
        time(budget, || {
            let _ = black_box(scan::op_start_on(&[b], 1e-3, MAX_WORK));
            1
        })
    };
    let size = chain.scan_size();
    let per = timed(&chain) / size.positions as f64;
    rows.push(Row {
        kind: "operand scan position".into(),
        ns: per,
        units: u.position as f64 / UNIT_PARTS as f64,
    });
    let size = planes.scan_size();
    let ns = timed(&planes) - per * size.positions as f64;
    rows.push(Row {
        kind: "operand scan net".into(),
        ns: ns / size.nets as f64,
        units: u.net as f64 / UNIT_PARTS as f64,
    });
}

// ---------------------------------------------------------------------
// Form keys.

/// A form key's parameter rounded, by step: the exact value formed, each
/// level of the loop, a round of settling exactly; on the components of
/// general unit directions (none rational).
fn form_key_rows(budget: Duration, rows: &mut Vec<Row>) {
    let u = UNITS.form_key;
    let comps: Vec<keys::Component> = (1..=64u64)
        .flat_map(|k| {
            let d = general(k).to_array();
            (0..3).map(move |c| keys::Component::new(d, c))
        })
        .collect();
    let each = |f: &dyn Fn(&keys::Component) -> bool| {
        time(budget, || {
            for c in &comps {
                black_box(f(black_box(c)));
            }
            comps.len() as u64
        })
    };
    assert!(comps.iter().all(|c| !c.rational()));
    rows.push(Row {
        kind: "form key rational".into(),
        ns: each(&|c| c.rational()),
        units: u.rational as f64 / UNIT_PARTS as f64,
    });
    for (level, parts) in u.level.into_iter().enumerate() {
        rows.push(Row {
            kind: format!("form key level {level}"),
            ns: each(&|c| c.level(level).expect("a level")),
            units: parts as f64 / UNIT_PARTS as f64,
        });
    }
    let exact: Vec<keys::Exact> = comps.iter().map(keys::Exact::of).collect();
    assert!(exact.iter().all(|e| e.settle()));
    let ns = time(budget, || {
        for e in &exact {
            black_box(black_box(e).settle());
        }
        exact.len() as u64
    });
    rows.push(Row {
        kind: "form key settle".into(),
        ns,
        units: u.settle as f64 / UNIT_PARTS as f64,
    });
}

// ---------------------------------------------------------------------
// The broad phase's tree.

/// `n` random boxes in `[0, 100]³`, sides from `lo` to `hi` on a log
/// scale.
fn random_boxes(rng: &mut Rng, n: usize, lo: f64, hi: f64) -> Vec<[[f64; 3]; 2]> {
    (0..n)
        .map(|_| {
            let a: [f64; 3] = std::array::from_fn(|_| rng.range(0.0, 100.0));
            let b = a.map(|x| x + rng.log_range(lo, hi));
            [a, b]
        })
        .collect()
}

/// The tree's kinds: a leaf built, over a thousand boxes and over sixty
/// thousand (one row, its spread shown); a box tested, by queries of
/// sides up to 1 meeting few of sixty thousand boxes of sides up to 1; a
/// hit collected, by queries holding most of four thousand boxes, the
/// time of their count taken off (a collection walks the tree again,
/// charged as tests).
fn bvh_rows(budget: Duration, rows: &mut Vec<Row>) {
    let u = UNITS.bvh;
    let mut rng = Rng::new(17);
    for n in [1024, 65536] {
        let boxes = random_boxes(&mut rng, n, 1e-3, 1.0);
        let ns = time(budget, || {
            black_box(Tree::new(black_box(&boxes)));
            n as u64
        });
        rows.push(Row {
            kind: format!("bvh build ({n} leaves)"),
            ns,
            units: u.build as f64 / UNIT_PARTS as f64,
        });
    }
    let tree = Tree::new(&random_boxes(&mut rng, 65536, 1e-3, 1.0));
    let queries = random_boxes(&mut rng, 4096, 1e-3, 1.0);
    let tests: u64 = queries.iter().map(|&q| tree.count(q).1).sum();
    let ns = time(budget, || {
        for &q in &queries {
            black_box(tree.count(black_box(q)));
        }
        tests
    });
    rows.push(Row {
        kind: "bvh test".into(),
        ns,
        units: u.test as f64 / UNIT_PARTS as f64,
    });
    let tree = Tree::new(&random_boxes(&mut rng, 4096, 1e-3, 1.0));
    let crowded: Vec<[[f64; 3]; 2]> = (0..16)
        .map(|k| {
            let lo = f64::from(k) * 0.5;
            [[lo; 3], [lo + 95.0; 3]]
        })
        .collect();
    let (hits, tests) = crowded.iter().fold((0u64, 0u64), |(h, t), &q| {
        let (c, n) = tree.count(q);
        (h + c as u64, t + n)
    });
    let count = time(budget, || {
        for &q in &crowded {
            black_box(tree.count(black_box(q)));
        }
        tests
    });
    let collect = time(budget, || {
        for &q in &crowded {
            black_box(tree.hits(black_box(q)));
        }
        hits
    });
    let walk = count * tests as f64 / hits as f64;
    rows.push(Row {
        kind: "bvh hit".into(),
        ns: collect - walk,
        units: u.hit as f64 / UNIT_PARTS as f64,
    });
}
