//! Termination and charging: a thin rectangle whose two long edges no box
//! separates refines in space, then in precision, and ends keyed out of
//! budget; a small share stops the count at the first level, a share
//! spent above it is precision; a count that settles at the first level
//! charges only there.

use super::*;

/// A polygon of `B` on the plane `z = 0` over the tiles `[0, 2)²` (as
/// [`triangle`]), its vertices `1, 2, …` in loop order, edge `k` from
/// vertex `k` to the next.
fn polygon(band: f64, pts: &[[f64; 2]]) -> FaceData {
    let mut f = triangle(band, TRI);
    let p3 = |p: [f64; 2]| [p[0], p[1], 0.0];
    let n = pts.len();
    f.trims = (0..n)
        .map(|k| {
            let (a, b) = (pts[k], pts[(k + 1) % n]);
            TrimData {
                coedge: CoedgeId(k as u32 + 1),
                edge: EdgeId(k as u32 + 1),
                reversed: false,
                spans: vec![pline(a, b)],
                ends: [VertId(k as u32 + 1), VertId(((k + 1) % n) as u32 + 1)],
                band,
                curve: vec![line(p3(a), p3(b))],
                maps: whole(1),
            }
        })
        .collect();
    f
}

/// The column through `(x, y)` on the plane, `z` above it.
fn column(x: f64, y: f64, z: f64) -> [f64; 3] {
    [x + z / UP.z * UP.x, y + z / UP.z * UP.y, z]
}

/// The counts of `pts`' polygon against the point of `A` at each of
/// `qs`, each with its charge.
fn counts(
    pts: &[[f64; 2]],
    runs: &[([f64; 3], Charge)],
) -> Vec<(Result<FacePieces, CountError>, Cost)> {
    let f = polygon(1.0 / 4096.0, pts);
    let mut order = Order::default();
    for (k, p) in pts.iter().enumerate() {
        order
            .pos
            .insert((Operand::B, VertId(k as u32 + 1)), [p[0], p[1], 0.0]);
    }
    for t in &f.trims {
        order.edges.insert((Operand::B, t.edge), t.ends);
    }
    let (sh, ft, o) = no_reads(&order);
    let reads = Records {
        shadows: &sh,
        feet: &ft,
        order: o,
    };
    let point = Point::vert(Operand::A, VertId(7));
    let sides = Sides::new();
    runs.iter()
        .map(|(q, charge)| {
            let rays = |t: &TrimData| chain(&t.curve, *q, Operand::A);
            f.query(&order, point, *q, &rays, &|fq, _| {
                count_pieces(fq, &sides, &reads, charge)
            })
        })
        .collect()
}

/// A rectangle `2⁻⁵⁰` high and `½` long: its long sides are closer than
/// any box [`MAX_SUBDIV_DEPTH`] splits deep separates.
const H: f64 = 1.0 / (1u64 << 50) as f64;
const THIN: [[f64; 2]; 4] = [
    [1.25, 1.25],
    [1.75, 1.25],
    [1.75, 1.25 + H],
    [1.25, 1.25 + H],
];

fn on_thin() -> [f64; 3] {
    column(1.375, 1.25, -0.01)
}

fn out(cause: Exhausted) -> Result<FacePieces, CountError> {
    Err(CountError::OutOfBudget {
        cause,
        point: Point::vert(Operand::A, VertId(7)),
        face: FaceId(2),
        at: None,
    })
}

/// `r` with the box a share ran out at dropped: a crossing of the share
/// names the box whose test crossed it, an unsettled last level none.
fn keyed(r: &Result<FacePieces, CountError>) -> Result<FacePieces, CountError> {
    match r {
        Err(CountError::OutOfBudget {
            cause,
            point,
            face,
            at,
        }) => {
            let unsettled = *cause == Exhausted::Precision && at.is_none();
            assert!(at.is_some() || unsettled, "{r:?}");
            Err(CountError::OutOfBudget {
                cause: *cause,
                point: *point,
                face: *face,
                at: None,
            })
        }
        r => r.clone(),
    }
}

#[test]
fn a_thin_face_ends_out_of_budget_at_the_last_level_keyed() {
    let [(r, full)] = counts(&THIN, &[(on_thin(), Charge::new(u64::MAX))])
        .try_into()
        .unwrap();
    assert_eq!(keyed(&r), out(Exhausted::Precision));
    assert!(
        full.levels.iter().all(|l| l.evals > 0),
        "every level ran: {full:?}"
    );
    // A share spent at the first level is work; one spent above it,
    // precision. Shares in units: the first level's pieces cost `u[0]`
    // each, the next's `u[1]`.
    let u = UNITS.count_piece;
    let first = u64::from(full.levels[0].evals) * u[0];
    let runs: Vec<_> = [0, 10, first - 1, first + 2 * u[1]]
        .into_iter()
        .map(|s| (on_thin(), Charge::new(s)))
        .collect();
    for ((r, cost), (_, charge)) in counts(&THIN, &runs).into_iter().zip(&runs) {
        assert!(charge.spent(&cost) > charge.share, "{cost:?}");
        if charge.share < first {
            assert_eq!(keyed(&r), out(Exhausted::Work), "share {}", charge.share);
            assert!(cost.levels[1..].iter().all(|l| l.evals == 0), "{cost:?}");
        } else {
            assert_eq!(keyed(&r), out(Exhausted::Precision));
            assert_eq!(cost.levels[0], full.levels[0]);
            assert!(cost.levels[1].evals > 0 && cost.levels[2].evals == 0);
        }
    }
}

#[test]
fn a_settled_count_charges_the_first_level_alone() {
    let pts = [[1.25, 1.25], [1.75, 1.25], [1.75, 1.75], [1.25, 1.75]];
    for (z, want) in [(0.01, 0), (-0.01, 1)] {
        let q = column(1.5, 1.4, z);
        let [(r, cost)] = counts(&pts, &[(q, Charge::new(u64::MAX))])
            .try_into()
            .unwrap();
        let p = r.expect("settled");
        assert_eq!(u64::from(cost.levels[0].evals), p.pieces);
        assert!(cost.levels[1..].iter().all(|l| l.evals == 0));
        let got = p.count(
            &Records {
                shadows: &Shadows::new(),
                feet: &NearFeet::default(),
                order: &Order::default(),
            },
            &Heights::new(),
            &[],
        );
        assert_eq!(got, Ok(want), "z = {z}");
        let units = p.pieces * UNITS.count_piece[0];
        let [(enough, _), (short, _)] = counts(
            &pts,
            &[(q, Charge::new(units)), (q, Charge::new(units - 1))],
        )
        .try_into()
        .unwrap();
        assert_eq!(enough, Ok(p), "the share it spent is enough");
        assert_eq!(keyed(&short), out(Exhausted::Work));
    }
}

/// A level's degrees above, below and in all, or its error.
type Degrees = Result<(i64, i64, i64), CountError>;

/// The pieces' degree of `fq` at every level, the patches lifted as
/// [`count_pieces`] lifts them.
struct EveryLevel<'c, 'a, 'f, R> {
    fq: &'c FaceQuery<'a, 'f, Bounded<f64>>,
    reads: &'c R,
    got: Vec<Degrees>,
}

impl<R: HeightReads> AtLevel for EveryLevel<'_, '_, '_, R> {
    type Out = ();

    fn at<S: RayLevel>(&mut self, like: &S, _: usize, _: &mut LevelWork) {
        let patches: Vec<HomPatch<S>> = self.fq.patches.iter().map(|p| lift(p, like)).collect();
        let f = self.fq;
        let fq = FaceQuery {
            face: f.face,
            corners: f.corners,
            patches: &patches,
            edges: f.edges,
            point: f.point,
            at: f.at,
            eps_tie: f.eps_tie,
            nets: f.nets,
            motion: f.motion,
        };
        let r = face_pieces(&fq, &Sides::new(), self.reads).map(|p| (p.above, p.below, p.degree()));
        self.got.push(r);
    }

    fn escalates(_: &()) -> bool {
        true
    }
}

/// Per query, the degrees at every level.
fn every_level(pts: &[[f64; 2]], qs: &[[f64; 3]]) -> Vec<Vec<Degrees>> {
    let f = polygon(1.0 / 4096.0, pts);
    let mut order = Order::default();
    for (k, p) in pts.iter().enumerate() {
        order
            .pos
            .insert((Operand::B, VertId(k as u32 + 1)), [p[0], p[1], 0.0]);
    }
    for t in &f.trims {
        order.edges.insert((Operand::B, t.edge), t.ends);
    }
    let (sh, ft, o) = no_reads(&order);
    let reads = Records {
        shadows: &sh,
        feet: &ft,
        order: o,
    };
    let point = Point::vert(Operand::A, VertId(7));
    qs.iter()
        .map(|q| {
            let rays = |t: &TrimData| chain(&t.curve, *q, Operand::A);
            f.query(&order, point, *q, &rays, &|fq, _| {
                let mut e = EveryLevel {
                    fq,
                    reads: &reads,
                    got: Vec::new(),
                };
                ladder(&mut e, 0, &mut Cost::default());
                e.got
            })
        })
        .collect()
}

const QUAD: [[f64; 2]; 4] = [[1.25, 1.25], [1.75, 1.25], [1.75, 1.75], [1.25, 1.75]];
const SLIVER: [[f64; 2]; 3] = [[1.25, 1.25], [1.75, 1.25], [1.5, 1.25 + 1.0 / 256.0]];

#[test]
fn a_count_settled_at_a_level_gives_the_same_degree_at_every_higher_one() {
    for (pts, qs) in [
        (
            &QUAD[..],
            vec![
                column(1.5, 1.4, 0.01),
                column(1.5, 1.4, -0.01),
                column(1.3, 1.7, -0.25),
                column(1.0, 1.0, -0.01),
            ],
        ),
        (
            &SLIVER[..],
            vec![column(1.5, 1.2502, -0.01), column(1.5, 1.2502, 0.01)],
        ),
    ] {
        for (q, got) in qs.iter().zip(every_level(pts, &qs)) {
            assert_eq!(got.len(), LEVELS);
            let first = got
                .iter()
                .position(|r| !matches!(r, Err(CountError::Unsettled)))
                .expect("settles at some level");
            for r in &got[first..] {
                assert_eq!(r, &got[first], "{q:?}: {got:?}");
            }
            assert!(got[first].is_ok(), "{q:?}: {got:?}");
        }
    }
}

/// Shares from none to past the full spend of a count that settles: past
/// the share at the first level for each short one, the same pieces from
/// the full one on, both with the same work for the same share, and the
/// pieces those of the uncharged count.
#[test]
fn each_share_of_a_settled_count_ends_the_same_way() {
    for (pts, q) in [
        (&QUAD[..], column(1.5, 1.4, -0.01)),
        (&SLIVER[..], column(1.5, 1.2502, -0.01)),
    ] {
        let [(full, cost)] = counts(pts, &[(q, Charge::new(u64::MAX))])
            .try_into()
            .unwrap();
        let full = full.expect("settled");
        assert_eq!(u64::from(cost.levels[0].evals), full.pieces);
        // The units the full count spends.
        let n = full.pieces * UNITS.count_piece[0];
        let mut shares: Vec<u64> = (0..8)
            .map(|k| n * k / 8)
            .chain([0, 1, 2, n - 2, n - 1, n, n + 1, u64::MAX - 1])
            .collect();
        shares.sort_unstable();
        shares.dedup();
        let runs: Vec<_> = shares.iter().map(|&s| (q, Charge::new(s))).collect();
        let again = counts(pts, &runs);
        let mut last = 0;
        for (((r, c), (_, charge)), (r2, c2)) in
            counts(pts, &runs).into_iter().zip(&runs).zip(again)
        {
            assert_eq!((&r, &c), (&r2, &c2), "deterministic");
            assert!(c.levels[1..].iter().all(|l| l.evals == 0), "{c:?}");
            let spent = charge.spent(&c);
            assert!(spent >= last, "work grows with the share");
            last = spent;
            if charge.share < n {
                assert_eq!(keyed(&r), out(Exhausted::Work), "share {}", charge.share);
                assert!(spent > charge.share, "stops past the share");
            } else {
                assert_eq!(r.as_ref(), Ok(&full), "share {}", charge.share);
                assert_eq!(c, cost);
            }
        }
    }
}

/// A level whose pieces cost nothing never runs out: the count is the
/// full one and spends nothing.
#[test]
fn free_pieces_spend_nothing() {
    let q = column(1.5, 1.4, -0.01);
    let free = Charge {
        share: 0,
        units: [0; LEVELS],
    };
    let [(full, _), (r, cost)] = counts(&QUAD, &[(q, Charge::new(u64::MAX)), (q, free)])
        .try_into()
        .unwrap();
    assert_eq!(r, full);
    assert_eq!(free.spent(&cost), 0);
}

/// The thin face's count on shares at the exact end of each level's
/// spend and a unit short of it, at the table's units: a share that
/// covers a level ends in the next one, a unit short ends in that level;
/// past the first level both are precision, and the levels run before
/// the one that ran out did the full count's work.
#[test]
fn shares_at_each_level_s_end_run_out_in_the_next_level() {
    let [(_, full)] = counts(&THIN, &[(on_thin(), Charge::new(u64::MAX))])
        .try_into()
        .unwrap();
    let u = UNITS.count_piece;
    let mut ends = Vec::new();
    let mut through = 0u64;
    for (level, l) in full.levels.iter().enumerate() {
        through += u64::from(l.evals) * u[level];
        ends.push(through);
    }
    let runs: Vec<_> = ends
        .iter()
        .flat_map(|&s| [s - 1, s])
        .map(|s| (on_thin(), Charge::new(s)))
        .collect();
    for (k, ((r, cost), (_, charge))) in counts(&THIN, &runs).into_iter().zip(&runs).enumerate() {
        let (level, short) = (k / 2, k % 2 == 0);
        // The level the count ran out at.
        let at = if short {
            level
        } else {
            (level + 1).min(LEVELS - 1)
        };
        let cause = if at == 0 {
            Exhausted::Work
        } else {
            Exhausted::Precision
        };
        assert_eq!(keyed(&r), out(cause), "share {}", charge.share);
        assert_eq!(
            cost.levels[..at],
            full.levels[..at],
            "share {}",
            charge.share
        );
        assert!(
            cost.levels[at].evals > 0,
            "share {}: {cost:?}",
            charge.share
        );
        assert!(
            cost.levels[at + 1..].iter().all(|l| l.evals == 0),
            "share {}: {cost:?}",
            charge.share
        );
        if short || level + 1 < LEVELS {
            assert!(charge.spent(&cost) > charge.share, "share {}", charge.share);
        } else {
            assert_eq!(cost, full, "the full spend, unsettled at the top");
        }
    }
}

/// A triangle `2⁻⁵⁰` high over a base of `½`: every point of each side
/// is within the band of another, so each corner's run reaches along the
/// whole sliver, thousands of pieces each.
const NEEDLE: [[f64; 2]; 3] = [[1.25, 1.25], [1.75, 1.25], [1.5, 1.25 + H]];

/// The needle's face and its corners built on a share of `share` units
/// (the face's counting setup), with what they spent.
fn needle_corners(share: u64) -> (Result<Corners, BoxError>, Cost) {
    let f = polygon(1.0 / 4096.0, &NEEDLE);
    let mut order = Order::default();
    for (k, p) in NEEDLE.iter().enumerate() {
        order
            .pos
            .insert((Operand::B, VertId(k as u32 + 1)), [p[0], p[1], 0.0]);
    }
    f.corners_on(&order, &Charge::setup(share))
}

/// The needle's setup on a share runs out at a coedge's run, its work
/// past the share by one piece at most; it is the same on every run and
/// thread (charged units are a function of the input and the share).
/// The full setup (thousands of pieces, seconds in debug builds) only
/// with `VARDE_TESTS=full`.
#[test]
fn a_needle_s_setup_runs_out_at_a_run() {
    let unit = UNITS.face_setup;
    let charge = Charge::setup(1 << 12);
    let one = || needle_corners(charge.share);
    let (r, cost) = one();
    let Err(BoxError::OutOfBudget { coedge }) = r else {
        panic!("{r:?}");
    };
    assert!((1..=3).contains(&coedge.0), "{coedge:?}");
    let spent = charge.spent(&cost);
    assert!(
        spent > charge.share && spent <= charge.share + unit,
        "{spent}"
    );
    // On other threads at once, the same.
    let others: Vec<_> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..4).map(|_| s.spawn(one)).collect();
        hs.into_iter()
            .map(|h| h.join().expect("a thread"))
            .collect()
    });
    for (r2, c2) in others {
        assert_eq!(
            (r2.err(), c2),
            (Some(BoxError::OutOfBudget { coedge }), cost)
        );
    }
    if varde_testing::pick(false, true) {
        let (r, full) = needle_corners(u64::MAX);
        let corners = r.expect("built on the whole budget");
        assert_eq!(u64::from(full.levels[0].evals), corners.pieces());
        assert!(Charge::setup(u64::MAX).spent(&full) > charge.share);
    }
}

/// The needle counted with corners built on a share that suffices, on a
/// share of its own: past it the count stops at the box whose test
/// crossed it, at the first level.
#[test]
fn a_needle_s_count_runs_out_at_a_box() {
    if !varde_testing::pick(false, true) {
        return;
    }
    let f = polygon(1.0 / 4096.0, &NEEDLE);
    let mut order = Order::default();
    for (k, p) in NEEDLE.iter().enumerate() {
        order
            .pos
            .insert((Operand::B, VertId(k as u32 + 1)), [p[0], p[1], 0.0]);
    }
    for t in &f.trims {
        order.edges.insert((Operand::B, t.edge), t.ends);
    }
    let corners = f.corners(&order);
    let (sh, ft, o) = no_reads(&order);
    let reads = Records {
        shadows: &sh,
        feet: &ft,
        order: o,
    };
    let q = column(1.5, 1.25, -0.01);
    let point = Point::vert(Operand::A, VertId(7));
    let rays = |t: &TrimData| chain(&t.curve, q, Operand::A);
    let charge = Charge::new(1 << 10);
    let (r, cost) = f.query_with(Some(&corners), &order, point, q, &rays, &|fq, _| {
        count_pieces(fq, &Sides::new(), &reads, &charge)
    });
    let Err(CountError::OutOfBudget {
        cause: Exhausted::Work,
        at: Some(b),
        ..
    }) = r
    else {
        panic!("{r:?}");
    };
    assert_eq!(b.face(), FaceId(2));
    assert!(charge.spent(&cost) > charge.share);
    assert!(cost.levels[1..].iter().all(|l| l.evals == 0));
}

/// A count's charge is a function of its input and share: run alone or
/// on several threads at once, each share stops at the same box with the
/// same work.
#[test]
fn charges_are_the_same_on_every_thread() {
    let runs: Vec<_> = [0, 500, 5000, u64::MAX]
        .into_iter()
        .map(|s| (on_thin(), Charge::new(s)))
        .collect();
    let alone = counts(&THIN, &runs);
    let threads: Vec<_> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..4).map(|_| s.spawn(|| counts(&THIN, &runs))).collect();
        hs.into_iter()
            .map(|h| h.join().expect("a thread"))
            .collect()
    });
    for t in threads {
        assert_eq!(t, alone);
    }
}
