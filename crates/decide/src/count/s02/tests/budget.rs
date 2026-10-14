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
    })
}

#[test]
fn a_thin_face_ends_out_of_budget_at_the_last_level_keyed() {
    let [(r, full)] = counts(&THIN, &[(on_thin(), Charge::new(u64::MAX))])
        .try_into()
        .unwrap();
    assert_eq!(r, out(Exhausted::Precision));
    assert!(
        full.levels.iter().all(|l| l.evals > 0),
        "every level ran: {full:?}"
    );
    // A share spent at the first level is work; one spent above it,
    // precision.
    let first = u64::from(full.levels[0].evals);
    let runs: Vec<_> = [0, 10, first - 1, first + 8]
        .into_iter()
        .map(|s| (on_thin(), Charge::new(s)))
        .collect();
    for ((r, cost), (_, charge)) in counts(&THIN, &runs).into_iter().zip(&runs) {
        assert!(charge.spent(&cost) > charge.share, "{cost:?}");
        if charge.share < first {
            assert_eq!(r, out(Exhausted::Work), "share {}", charge.share);
            assert!(cost.levels[1..].iter().all(|l| l.evals == 0), "{cost:?}");
        } else {
            assert_eq!(r, out(Exhausted::Precision));
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
            &LayerTable::new(),
            &Heights::new(),
        );
        assert_eq!(got, Ok(want), "z = {z}");
        let [(enough, _), (short, _)] = counts(
            &pts,
            &[(q, Charge::new(p.pieces)), (q, Charge::new(p.pieces - 1))],
        )
        .try_into()
        .unwrap();
        assert_eq!(enough, Ok(p), "the share it spent is enough");
        assert_eq!(short, out(Exhausted::Work));
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
        let n = full.pieces;
        assert_eq!(u64::from(cost.levels[0].evals), n);
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
                assert_eq!(r, out(Exhausted::Work), "share {}", charge.share);
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
