//! Calibration of the kernel's unit table
//! (`varde_decide::records::shadow::UNITS`): times each computation kind
//! the kernel charges, per precision level where it has levels, and
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
//! (`shadow::UNIT_PARTS`): a record charges the sum of its parts,
//! rounded up to whole units.
//!
//! `VARDE_CALIBRATE=quick` runs each kind for a tenth of the time: a
//! smoke test, its numbers noisier.

use std::hint::black_box;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use glam::DVec3;
use varde_decide::bezier::{HomPatch, Poly, Scalar};
use varde_decide::count::boxes::{Corners, FaceUv, Trim, UvSpan};
use varde_decide::count::pieces::{Ribbon, SpanMap};
use varde_decide::count::s02::{Charge, FaceQuery, TrimEdge, count_pieces};
use varde_decide::ids::{CoedgeId, EdgeId, FaceId, VertId};
use varde_decide::num::bigf::{Big, BigF, Width};
use varde_decide::num::bounded::Bounded;
use varde_decide::num::dd::Dd;
use varde_decide::num::decide::Sign;
use varde_decide::num::exact::{self, Num, Pred, det3, point, sub};
use varde_decide::order::{EdgeKey, EntityOrder, VertKey};
use varde_decide::perturb::{NU, R_HAT, UP};
use varde_decide::records::feet::NearFeet;
use varde_decide::records::height::Records;
use varde_decide::records::keys::{Operand, Point};
use varde_decide::records::ray::{ChainCrossing, ChainRec, Crossing, Span};
use varde_decide::records::shadow::{Cost, LEVELS, LevelWork, Shadows, UNIT_PARTS, UNITS};
use varde_decide::records::side::Sides;
use varde_kernel::MAX_WORK;
use varde_kernel::geom::certify::{SeamBound, calibration};
use varde_kernel::geom::curve::{Curve3D, PCurve};
use varde_kernel::geom::form::PlaneKey;
use varde_kernel::geom::lattice::{CELL, Lattice};
use varde_kernel::geom::surface::Surface;

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
        count_piece_rows(budget, &mut rows);
        record_rows(budget, &mut rows);
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

const ROOT_HALF: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// The plane `z = 0` on its one base tile, 4·10⁶ mm across.
fn base_plane() -> Surface {
    let lat = Lattice::new(PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap().into()).unwrap();
    let rect = lat.half(0).unwrap();
    Surface::new(lat, rect).unwrap()
}

/// The p-curve of `c` on the base plane `s`.
fn on_plane(s: &Surface, c: &Curve3D) -> PCurve {
    let lat = s.lattice();
    let (o, [u, v, _]) = (lat.origin(), lat.axes());
    let spans = c
        .spans()
        .iter()
        .map(|sp| {
            sp.map(|h| {
                let d = [h[0] - o[0], h[1] - o[1], h[2] - o[2]];
                let along = |a: [f64; 3]| (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5;
                [along(u), along(v), h[3]]
            })
        })
        .collect();
    PCurve::new(spans, c.knots().to_vec()).unwrap()
}

/// The quarter circle of radius 50, end weights `w` and `1/w`.
fn quarter(w: f64) -> Curve3D {
    let r = 50.0;
    Curve3D::new(
        vec![[
            [r, 0.0, 0.0, w],
            [r, r, 0.0, ROOT_HALF],
            [0.0, r, 0.0, 1.0 / w],
        ]],
        vec![0.0, 1.0],
    )
    .unwrap()
}

/// The arcs with end weights 1, 3 and 64 on the base plane, and the arc
/// with weights 3 moved off it by `2·10⁻⁷` of the tile.
fn arcs(s: &Surface) -> Vec<(Curve3D, PCurve)> {
    let mut out: Vec<_> = [1.0, 3.0, 64.0]
        .map(|w| {
            let c = quarter(w);
            let p = on_plane(s, &c);
            (c, p)
        })
        .into();
    let c = quarter(3.0);
    let p = on_plane(s, &c).perturbed(&[[2e-7, -2e-7]]).unwrap();
    out.push((c, p));
    out
}

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

/// The seconds of a seam pass that runs out at `MAX_WORK` on the current
/// pool, and the units it used: the arcs' bounds towards targets they
/// can't all meet, repeated past the budget.
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

/// A straight span from `a` to `b`.
fn line(a: [f64; 3], b: [f64; 3]) -> Span {
    let m = [0, 1, 2].map(|k| 0.5 * (a[k] + b[k]));
    [
        [a[0], a[1], a[2], 1.0],
        [m[0], m[1], m[2], 1.0],
        [b[0], b[1], b[2], 1.0],
    ]
}

/// A straight p-span.
fn pline(a: [f64; 2], b: [f64; 2]) -> UvSpan {
    [
        [a[0], a[1], 1.0],
        [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 1.0],
        [b[0], b[1], 1.0],
    ]
}

/// The crossings of the ray of a point of `A` at `q` (along `−r̂`) with
/// straight spans: per span the root of `ν·(P − q)`, ahead along the ray,
/// signed `sign(ρ × T)`.
fn chain(spans: &[Span], q: [f64; 3]) -> ChainRec {
    let dot =
        |a: DVec3, p: [f64; 4]| a.x * (p[0] - q[0]) + a.y * (p[1] - q[1]) + a.z * (p[2] - q[2]);
    let mut out = ChainRec::default();
    for (k, sp) in spans.iter().enumerate() {
        let (n0, n1) = (dot(NU, sp[0]), dot(NU, sp[2]));
        if (n0 > 0.0) == (n1 > 0.0) || n0 == n1 {
            continue;
        }
        let t = n0 / (n0 - n1);
        let (r0, r1) = (dot(R_HAT, sp[0]), dot(R_HAT, sp[2]));
        if -(r0 + (r1 - r0) * t) <= 0.0 {
            continue;
        }
        // `ρ × T` with `ρ = −r̂`: the sign of `−ν·T`.
        let sign = if n1 - n0 < 0.0 { Sign::Pos } else { Sign::Neg };
        let clamp = |t: f64| t.clamp(0.0, 1.0);
        out.crossings.push(ChainCrossing {
            span: k as u32,
            crossing: Crossing {
                t: [clamp(t - 1e-12), clamp(t + 1e-12)],
                sign,
                single: true,
            },
        });
    }
    out
}

/// The geometric order of a face's vertices on `z = 0`.
struct Order {
    pos: Vec<[f64; 2]>,
    n: usize,
}

impl EntityOrder for Order {
    type Key = ();

    fn ends(&self, _: Operand, g: EdgeId) -> [VertId; 2] {
        let k = g.0 as usize - 1;
        [VertId(k as u32 + 1), VertId(((k + 1) % self.n) as u32 + 1)]
    }

    fn vert_key(&self, _: Operand, v: VertId) -> VertKey<()> {
        let p = self.pos[v.0 as usize - 1];
        VertKey::new([p[0], p[1], 0.0], []).expect("no NaN")
    }

    fn edge_key(&self, operand: Operand, g: EdgeId) -> EdgeKey<()> {
        let [s, e] = self.ends(operand, g);
        let [a, b] = [s, e].map(|v| self.pos[v.0 as usize - 1]);
        let mid = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 0.0];
        EdgeKey::new([s, e].map(|v| self.vert_key(operand, v)), mid, [(), ()]).expect("no NaN")
    }
}

/// The column through `(x, y)` of `z = 0`, `z` above it.
fn column(x: f64, y: f64, z: f64) -> [f64; 3] {
    [x + z / UP.z * UP.x, y + z / UP.z * UP.y, z]
}

/// `f` given the count of the polygon `pts` (a face of `B` on the plane
/// `z = 0` over the tiles `[0, 2)²`) against the point of `A` at `q`
/// with a charge, giving its cost. The face and its corners are built
/// once, before `f` runs.
fn with_count<R>(
    pts: &[[f64; 2]],
    q: [f64; 3],
    f: impl FnOnce(&dyn Fn(&Charge) -> Cost) -> R,
) -> R {
    let band = 1.0 / 4096.0;
    let first = Bounded::exact(0.0);
    let mut patches = Vec::new();
    for i in 0..2 {
        for j in 0..2 {
            let net = (0..9).map(|ab| {
                let (a, b) = (ab / 3, ab % 3);
                [
                    i as f64 + a as f64 / 2.0,
                    j as f64 + b as f64 / 2.0,
                    0.0,
                    1.0,
                ]
            });
            patches.push(HomPatch::from_affine(2, 2, net, &first).unwrap());
        }
    }
    let n = pts.len();
    let p3 = |p: [f64; 2]| [p[0], p[1], 0.0];
    let spans: Vec<[UvSpan; 1]> = (0..n).map(|k| [pline(pts[k], pts[(k + 1) % n])]).collect();
    let curves: Vec<[Span; 1]> = (0..n)
        .map(|k| [line(p3(pts[k]), p3(pts[(k + 1) % n]))])
        .collect();
    let maps = [SpanMap {
        span: 0,
        t: [0.0, 1.0],
    }];
    let order = Order {
        pos: pts.to_vec(),
        n,
    };
    let trims: Vec<Trim<'_>> = (0..n)
        .map(|k| Trim {
            coedge: CoedgeId(k as u32 + 1),
            reversed: false,
            spans: &spans[k],
            ends: order.ends(Operand::B, EdgeId(k as u32 + 1)),
            zero_length: false,
            band,
        })
        .collect();
    let face = FaceUv::new(
        Operand::B,
        FaceId(2),
        [0..2, 0..2],
        [1.0, 1.0],
        &trims,
        false,
    )
    .expect("a valid face");
    let mut verts: Vec<VertId> = (1..=n as u32).map(VertId).collect();
    verts.sort_by_key(|&v| order.vert_key(Operand::B, v));
    let corners = Corners::new(&face, &patches, &verts).expect("corners");
    let recs: Vec<ChainRec> = curves.iter().map(|c| chain(c, q)).collect();
    let edges: Vec<TrimEdge<'_>> = (0..n)
        .map(|k| TrimEdge {
            edge: EdgeId(k as u32 + 1),
            ribbon: Ribbon {
                coedge: CoedgeId(k as u32 + 1),
                curve: &curves[k],
                map: &maps,
                rays: &recs[k],
            },
        })
        .collect();
    let fq = FaceQuery {
        face: &face,
        corners: &corners,
        patches: &patches,
        edges: &edges,
        point: Point::vert(Operand::A, VertId(7)),
        at: q,
        eps_tie: 1e-9,
        nets: None,
    };
    let (shadows, feet) = (Shadows::new(), NearFeet::new());
    let reads = Records {
        shadows: &shadows,
        feet: &feet,
        order: &order,
    };
    let sides = Sides::new();
    f(&|charge| count_pieces(&fq, &sides, &reads, charge).1)
}

/// A unit a piece at every level, to count them.
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
/// unsettled at the last. Run on shares that stop it one level later
/// each, the time between two shares is the next level's (with a share
/// of nothing a count stops at its first piece). Then the first level on
/// a square that settles there.
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
        let mut before = count_ns(budget, count, &per_piece(0));
        let mut through = 0u64;
        for level in 0..LEVELS {
            through += pieces[level];
            // Through this level, and a piece of the next.
            let ns = count_ns(budget, count, &per_piece(through));
            rows.push(Row {
                kind: format!("count piece L{level}"),
                ns: (ns - before) / pieces[level] as f64,
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

/// The orientation of four points, `det(b − a, c − a, d − a)`.
struct Orient([DVec3; 4]);

impl Pred for Orient {
    fn eval<N: Num>(&self) -> N {
        let [a, b, c, d] = self.0.map(point::<N>);
        det3(&sub(&b, &a), &sub(&c, &a), &sub(&d, &a))
    }
}

/// The units a record is charged for `cost` alone, in parts of a unit.
fn parts(cost: &Cost) -> f64 {
    cost.parts(&UNITS.record) as f64 / UNIT_PARTS as f64
}

/// Exact predicate stages: the filter deciding, an exact zero the
/// expansions find, and one the rationals do (a coordinate's lowest set
/// bit far below the expansions' range).
fn exact_rows(budget: Duration, rows: &mut Vec<Row>) {
    let pts = |k: u64| {
        let f =
            |x: u64| ((x.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 11) as f64) / (1u64 << 53) as f64;
        let p = |i: u64| {
            DVec3::new(
                f(4 * k + i) * 200.0,
                f(97 * k + i) * 200.0,
                f(31 * k + i) * 200.0,
            )
        };
        [p(0), p(1), p(2), p(3)]
    };
    let general: Vec<Orient> = (0..64).map(|k| Orient(pts(k))).collect();
    // `d = a + ½(b − a) + ¼(c − a)`, exactly: coplanar, so the filter
    // can't decide.
    let planar: Vec<Orient> = (0..64)
        .map(|k| {
            let [a, b, c, _] = pts(k).map(|p| (p * 1024.0).floor());
            Orient([a, b, c, a + (b - a) * 0.5 + (c - a) * 0.25])
        })
        .collect();
    // Points on the plane `z = x + y`, exactly, with coordinates as small
    // as `2⁻⁴⁰⁰`: the orientation's products fall below the expansions'
    // range.
    let tiny: Vec<Orient> = (0..16)
        .map(|k| {
            let t = (k + 1) as f64 * f64::from_bits(((1023 - 400) as u64) << 52);
            let on = |x: f64, y: f64| DVec3::new(x, y, x + y);
            Orient([
                on(t, 0.0),
                on(0.0, t),
                on(2.0 * t, t),
                on(1.0 + k as f64, 1.0),
            ])
        })
        .collect();
    for (kind, preds, stage) in [
        ("exact filter", &general, 0),
        ("exact expansion", &planar, 1),
        ("exact rational", &tiny, 2),
    ] {
        let mut work = exact::Work::default();
        for p in preds.iter() {
            work = work.and(exact::value_sign(p).1);
        }
        let ran = [work.filters, work.expansions, work.rationals];
        assert!(ran[stage] as usize == preds.len(), "{kind}: {work:?}");
        let ns = time(budget, || {
            for p in preds.iter() {
                let _ = black_box(exact::value_sign(p));
            }
            preds.len() as u64
        });
        // The stages before this one ran too: charged with it.
        let cost = Cost::exact(exact::Work {
            filters: work.filters,
            expansions: work.expansions,
            longest: work.longest,
            rationals: work.rationals,
        });
        rows.push(Row {
            kind: kind.into(),
            ns,
            units: parts(&cost) / preds.len() as f64,
        });
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
fn curved_rows<S: Scalar>(budget: Duration, level: usize, like: &S, rows: &mut Vec<Row>) {
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
    curved_rows(budget, 0, &Bounded::exact(0.0), rows);
    curved_rows(budget, 1, &Bounded::exact(Dd::from_f64(0.0)), rows);
    for (k, w) in Width::ALL.into_iter().enumerate() {
        curved_rows(budget, 2 + k, &Bounded::exact(Big::from_f64(0.0, w)), rows);
    }
}
