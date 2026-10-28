//! Per-piece counts sum to the face's on random partitions and records;
//! root degrees on implicit faces.

use malachite::Rational;
use varde_decide::bezier::{Dyadic, HomCurve, Implicit, Limits};
use varde_decide::count::cross::{Coedge, x12};
use varde_decide::ids::EdgeId;
use varde_decide::num::decide::Sign;
use varde_decide::records::keys::XClusterIx;
use varde_decide::records::xrec::{XCluster, XPart, XRec, ZRow};

use super::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn signed(&mut self, k: i64) -> i64 {
        self.below(2 * k as u64 + 1) as i64 - k
    }
}

/// A random record of `e` against one edge: clusters on dyadic eighths of
/// span 0 or 1, some clear above or below, some read by their key.
fn record(rng: &mut Rng) -> XRec {
    let mut clusters = Vec::new();
    for span in 0..2u32 {
        for j in 0..8u32 {
            if rng.below(3) != 0 {
                continue;
            }
            let lo = f64::from(j) / 8.0 + 1.0 / 64.0;
            let clear = match rng.below(3) {
                0 => Some(Sign::Pos),
                1 => Some(Sign::Neg),
                _ => None,
            };
            let mut degree = rng.signed(2);
            if degree == 0 {
                degree = 1;
            }
            clusters.push(XCluster {
                e: XPart {
                    span: 0,
                    t: [0.25, 0.5],
                },
                g: XPart {
                    span,
                    t: [lo, lo + 1.0 / 32.0],
                },
                degree,
                clear,
                row: ZRow::EdgeEdge,
            });
        }
    }
    XRec { clusters, level: 0 }
}

/// Whether a key-read cluster lies above: a fixed function of its place.
fn read_above(g: EdgeId, c: XClusterIx) -> Sign {
    if (g.index() + c.0 as usize) % 2 == 0 {
        Sign::Pos
    } else {
        Sign::Neg
    }
}

#[test]
fn pieces_sum_to_the_face() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..500 {
        let pieces = 1 + rng.below(6) as u32;
        let edges = 1 + rng.below(5) as u32;
        let mut boundary: Vec<Coedge> = (0..edges)
            .map(|g| Coedge {
                edge: EdgeId(g),
                forward: rng.below(2) == 0,
            })
            .collect();
        // A slit: one edge run both ways.
        if rng.below(4) == 0 {
            let g = EdgeId(rng.below(u64::from(edges)) as u32);
            boundary.push(Coedge {
                edge: g,
                forward: true,
            });
            boundary.push(Coedge {
                edge: g,
                forward: false,
            });
        }
        let recs: Vec<XRec> = (0..edges).map(|_| record(&mut rng)).collect();
        // Each coedge's spans cut at eighths (never inside a cluster).
        let mut trims = Vec::new();
        for c in 0..boundary.len() as u32 {
            for span in 0..2u32 {
                let mut a = 0u32;
                while a < 8 {
                    let b = (a + 1 + rng.below(4) as u32).min(8);
                    trims.push(TrimPart {
                        coedge: c,
                        span,
                        s: [f64::from(a) / 8.0, f64::from(b) / 8.0],
                        piece: rng.below(u64::from(pieces)) as u32,
                    });
                    a = b;
                }
            }
        }
        let hits: [Vec<(PieceIx, i64)>; 2] = std::array::from_fn(|_| {
            (0..rng.below(5))
                .map(|_| (rng.below(u64::from(pieces)) as u32, rng.signed(1)))
                .collect()
        });
        let free: Vec<FreeSide> = (0..rng.below(6))
            .map(|_| FreeSide {
                pieces: [
                    rng.below(u64::from(pieces)) as u32,
                    rng.below(u64::from(pieces)) as u32,
                ],
                s11: rng.signed(2),
            })
            .collect();
        let parts = FaceParts {
            pieces,
            boundary: &boundary,
            hits: [&hits[0], &hits[1]],
            trims: &trims,
            free: &free,
        };
        let got = piece_counts(
            &parts,
            |g| &recs[g.index()],
            |g, c, _| Ok::<_, ()>(read_above(g, c)),
        )
        .expect("every cluster in one part");
        assert_eq!(got.len(), pieces as usize);
        // The face's count by its own identity, from the whole records.
        let ends = [0, 1].map(|k| hits[k].iter().map(|h| h.1).sum::<i64>());
        let face = x12(ends, &boundary, |g| {
            recs[g.index()].s11(|c, _| Ok::<_, ()>(read_above(g, c)))
        })
        .unwrap();
        assert_eq!(got.iter().sum::<i64>(), face);
    }
}

#[test]
fn a_cut_through_a_cluster_moves() {
    let boundary = [Coedge {
        edge: EdgeId(0),
        forward: true,
    }];
    let rec = XRec {
        clusters: vec![XCluster {
            e: XPart {
                span: 0,
                t: [0.0, 1.0],
            },
            g: XPart {
                span: 0,
                t: [0.25, 0.75],
            },
            degree: 1,
            clear: Some(Sign::Pos),
            row: ZRow::EdgeEdge,
        }],
        level: 0,
    };
    let split = [
        TrimPart {
            coedge: 0,
            span: 0,
            s: [0.0, 0.5],
            piece: 0,
        },
        TrimPart {
            coedge: 0,
            span: 0,
            s: [0.5, 1.0],
            piece: 1,
        },
    ];
    let mut parts = FaceParts {
        pieces: 2,
        boundary: &boundary,
        hits: [&[], &[]],
        trims: &split,
        free: &[],
    };
    let read = |_: EdgeId, _: XClusterIx, _: &XCluster| Ok::<_, ()>(Sign::Pos);
    assert_eq!(
        piece_counts(&parts, |_| &rec, read),
        Err(PieceError::CutInCluster {
            coedge: 0,
            cluster: XClusterIx(0)
        })
    );
    parts.trims = &split[..0];
    assert_eq!(
        piece_counts(&parts, |_| &rec, read),
        Err(PieceError::Uncovered {
            coedge: 0,
            cluster: XClusterIx(0)
        })
    );
    let whole = [TrimPart {
        coedge: 0,
        span: 0,
        s: [0.0, 1.0],
        piece: 1,
    }];
    parts.trims = &whole;
    assert_eq!(piece_counts(&parts, |_| &rec, read), Ok(vec![0, -1]));
}

const LIMITS: Limits = Limits {
    max_depth: 63,
    max_splits: 1 << 16,
};

fn q(x: i64) -> Rational {
    Rational::from(x)
}

/// The unit sphere `x² + y² + z² − 1`, its gradient outward.
fn sphere() -> Implicit<Rational> {
    Implicit::new(
        2,
        [q(0), q(0), q(0)],
        [
            ([2, 0, 0], q(1)),
            ([0, 2, 0], q(1)),
            ([0, 0, 2], q(1)),
            ([0, 0, 0], q(-1)),
        ],
    )
    .unwrap()
}

/// The plane `z = 0`, its gradient up.
fn plane() -> Implicit<Rational> {
    Implicit::new(
        1,
        [q(0), q(0), q(0)],
        [([0, 0, 1], q(1)), ([0, 0, 0], q(0))],
    )
    .unwrap()
}

fn line(a: [f64; 3], b: [f64; 3]) -> HomCurve<Rational, 3> {
    HomCurve::from_affine([(a, 1.0), (b, 1.0)], &q(0)).unwrap()
}

fn no_ends(_: u32, _: bool) -> Option<Sign> {
    panic!("no cluster reaches a span end")
}

#[test]
fn a_chord_enters_then_leaves() {
    let c = [line([-2.0, 0.5, 0.0], [2.0, 0.5, 0.0])];
    let r = implicit_roots(&c, &sphere(), Sign::Pos, LIMITS, no_ends).unwrap();
    let deg: Vec<i8> = r.iter().map(|r| r.degree).collect();
    assert_eq!(deg, [1, -1]);
    assert!(r.iter().all(|r| r.kind == RootKind::Isolated));
    assert!(r[0].t[1] <= r[1].t[0]);
    realise(0, &r).unwrap();
    // Reversed: mirrored order, the degrees swapped.
    let back = [c[0].reversed()];
    let rb = implicit_roots(&back, &sphere(), Sign::Pos, LIMITS, no_ends).unwrap();
    assert_eq!(rb.iter().map(|r| r.degree).collect::<Vec<_>>(), [1, -1]);
    // The solid's complement: the face flipped.
    let rf = implicit_roots(&c, &sphere(), Sign::Neg, LIMITS, no_ends).unwrap();
    assert_eq!(rf.iter().map(|r| r.degree).collect::<Vec<_>>(), [-1, 1]);
}

#[test]
fn a_tangent_edge_has_no_record() {
    let c = [line([-2.0, 1.0, 0.0], [2.0, 1.0, 0.0])];
    let r = implicit_roots(&c, &sphere(), Sign::Pos, LIMITS, no_ends).unwrap();
    assert!(r.is_empty(), "{r:?}");
    realise(0, &r).unwrap();
    assert_eq!(
        realise(1, &r),
        Err(RootError::Unrealised { want: 1, got: 0 })
    );
}

#[test]
fn a_root_at_a_span_end_reads_its_record() {
    // From a point on the sphere outward: the start's record says the
    // edge leaves the solid there.
    let c = [line([1.0, 0.0, 0.0], [2.0, 0.0, 0.0])];
    let r = implicit_roots(&c, &sphere(), Sign::Pos, LIMITS, |s: u32, at_end: bool| {
        assert_eq!((s, at_end), (0, false));
        Some(Sign::Neg)
    })
    .unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].degree, r[0].kind), (-1, RootKind::Cluster));
    assert_eq!(r[0].t[0], Dyadic::ZERO);
    let unsettled = implicit_roots(&c, &sphere(), Sign::Pos, LIMITS, |_: u32, _: bool| None);
    assert_eq!(unsettled, Err(RootError::EndUnsettled { span: 0 }));
}

#[test]
fn spans_in_order_and_an_edge_in_the_face() {
    let c = [
        line([0.0, 0.0, 1.0], [0.0, 0.0, -1.0]),
        line([0.0, 0.0, -1.0], [1.0, 0.0, 1.0]),
    ];
    let r = implicit_roots(&c, &plane(), Sign::Pos, LIMITS, no_ends).unwrap();
    let got: Vec<(u32, i8)> = r.iter().map(|r| (r.span, r.degree)).collect();
    assert_eq!(got, [(0, 1), (1, -1)]);
    let flat = [line([0.0, 0.0, 0.0], [1.0, 1.0, 0.0])];
    let r = implicit_roots(&flat, &plane(), Sign::Pos, LIMITS, |_: u32, _: bool| None);
    assert_eq!(r, Err(RootError::InFace { span: 0 }));
}
