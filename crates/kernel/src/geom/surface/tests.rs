//! Surfaces on the lattices of every tiled form, at several levels: a
//! point on a tile line the same bits from either tile, transposed, and
//! as its row's span; nodes their control points; local coordinates
//! exact but on tile −1; positions and indices; rows from either side;
//! the tree's hits against brute force over the tiles' boxes, charged.

use std::convert::Infallible;

use malachite::Rational as Q;
use varde_decide::bezier::HomCurve;
use varde_testkit::rng::Rng;

use super::*;
use crate::geom::form::{CylinderKey, FormKey, PlaneKey, SphereKey};
use crate::geom::lattice::Level;
use crate::infra::budget::{Budget, Pass, Stop, Work};

const PASS: Pass = Pass {
    stage: "surface",
    rung: 0,
};

/// A plane, a cylinder and a sphere at the origin, and random ones up to
/// `1e3` out with radii from `1e-2` to `1e3`.
fn keys(seed: u64) -> Vec<FormKey> {
    let mut rng = Rng::new(seed);
    let mut out = vec![
        PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap().into(),
        CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], 1.0)
            .unwrap()
            .into(),
        SphereKey::new([0.0; 3], 1.0, [0.0, 0.0, 1.0])
            .unwrap()
            .into(),
    ];
    for _ in 0..2 {
        let p = rng.point(1e3).to_array();
        let d = rng.direction().to_array();
        let r = rng.log_range(1e-2, 1e3);
        out.push(PlaneKey::new(d, p).unwrap().into());
        out.push(CylinderKey::new(p, d, r).unwrap().into());
        out.push(SphereKey::new(p, r, d).unwrap().into());
    }
    out
}

/// A surface of `key` at `level` straddling base-cell lines in both
/// directions where the form has them (less than a turn in `u`, the
/// sphere pole to pole).
fn straddling(key: &FormKey, level: Level) -> Surface {
    let (hu, hv) = (1i64 << level.u, 1i64 << level.v);
    let v = match key {
        FormKey::Sphere(_) => -hv..hv,
        _ => -hv - 1..hv + 1,
    };
    let rect = Rect {
        level,
        u: -1..hu + 1,
        v,
    };
    Surface::new(Lattice::new(key.clone()).unwrap(), rect).unwrap()
}

const LEVELS: [Level; 3] = [
    Level { u: 0, v: 0 },
    Level { u: 1, v: 2 },
    Level { u: 3, v: 1 },
];

fn bits(p: &[Bounded<f64>; 3]) -> [[u64; 3]; 3] {
    p.map(|b| [b.v, b.err_trunc, b.err_round].map(f64::to_bits))
}

/// `x` in `(0, 1)`, a dyadic fraction or not.
fn inner(rng: &mut Rng) -> f64 {
    if rng.unit() < 0.3 {
        (1 + rng.next_u64() % 7) as f64 / 8.0
    } else {
        rng.range(1e-9, 1.0 - 1e-9)
    }
}

/// On every tile line of the rectangle, `u = k` and `v = l`, inner ones
/// and the far sides included, the surface's point is the same bits as
/// the tile on either side's at its local 0 or 1, as the transposed
/// patch's, and as the row's own span at the same parameter (the local
/// coordinate along the line, certified).
#[test]
fn tile_lines_evaluate_alike_from_either_side() {
    let mut rng = Rng::new(31);
    for key in keys(30) {
        for level in LEVELS {
            let s = straddling(&key, level);
            let r = s.rect().clone();
            let zero = Bounded::exact(0.0);
            let (b0, b1) = (Bounded::exact(0.0), Bounded::exact(1.0));
            for k in r.u.start..=r.u.end {
                for j in r.v.clone() {
                    let uv = [k as f64, j as f64 + inner(&mut rng)];
                    let bt = s.locate(uv).unwrap().local[1];
                    let p = s.eval(uv).unwrap();
                    if let Some(tile) = s.tile(k, j) {
                        assert_eq!(bits(&p), bits(&tile.patch().eval(&b0, &bt)));
                        let tr = tile.patch().transposed();
                        assert_eq!(bits(&p), bits(&tr.eval(&bt, &b0)));
                    }
                    if let Some(tile) = s.tile(k - 1, j) {
                        assert_eq!(bits(&p), bits(&tile.patch().eval(&b1, &bt)));
                    }
                    let row = s.row(Dir::V, k, j).unwrap();
                    let span = HomCurve::<_, 3>::from_affine(
                        row.map(|q| ([q[0], q[1], q[2]], q[3])),
                        &zero,
                    )
                    .unwrap();
                    assert_eq!(bits(&p), bits(&span.eval(&bt)), "u = {k}, v in {j}");
                }
            }
            for l in r.v.start..=r.v.end {
                for i in r.u.clone() {
                    let uv = [i as f64 + inner(&mut rng), l as f64];
                    let bt = s.locate(uv).unwrap().local[0];
                    let p = s.eval(uv).unwrap();
                    if let Some(tile) = s.tile(i, l) {
                        assert_eq!(bits(&p), bits(&tile.patch().eval(&bt, &b0)));
                    }
                    if let Some(tile) = s.tile(i, l - 1) {
                        assert_eq!(bits(&p), bits(&tile.patch().eval(&bt, &b1)));
                    }
                    let row = s.row(Dir::U, l, i).unwrap();
                    let span = HomCurve::<_, 3>::from_affine(
                        row.map(|q| ([q[0], q[1], q[2]], q[3])),
                        &zero,
                    )
                    .unwrap();
                    assert_eq!(bits(&p), bits(&span.eval(&bt)), "v = {l}, u in {i}");
                }
            }
        }
    }
}

/// A lattice node is its control point exactly, from every tile meeting
/// it, and so are its partials' point; off the lines the surface is the
/// located tile's patch.
#[test]
fn nodes_are_control_points_and_inner_points_their_tiles() {
    let mut rng = Rng::new(33);
    for key in keys(32) {
        let s = straddling(&key, Level { u: 2, v: 1 });
        let r = s.rect().clone();
        for k in r.u.start..=r.u.end {
            for l in r.v.start..=r.v.end {
                let p = s.eval([k as f64, l as f64]).unwrap();
                assert!(p.iter().all(|b| b.err() == 0.0), "({k}, {l})");
                let p = p.map(|b| b.v.to_bits());
                for (a, i) in [(0, k), (2, k - 1)] {
                    for (b, j) in [(0, l), (2, l - 1)] {
                        if let Some(t) = s.tile(i, j) {
                            assert_eq!(p, t.point(a, b).map(f64::to_bits), "({k}, {l})");
                        }
                    }
                }
                let d = s.eval_partials([k as f64, l as f64]).unwrap();
                assert_eq!(d.p.map(|b| b.v.to_bits()), p);
            }
        }
        for _ in 0..64 {
            let (i, j) = (
                r.u.start + (rng.next_u64() % r.u.end.abs_diff(r.u.start)) as i64,
                r.v.start + (rng.next_u64() % r.v.end.abs_diff(r.v.start)) as i64,
            );
            let (a, b) = (inner(&mut rng), inner(&mut rng));
            // Away from tile −1's upper half, where the local coordinate
            // isn't exact.
            if (i == -1 && a > 0.5) || (j == -1 && b > 0.5) {
                continue;
            }
            let uv = [i as f64 + a, j as f64 + b];
            let at = s.locate(uv).unwrap();
            assert_eq!((at.i, at.j), (i, j));
            let tile = s.tile(i, j).unwrap();
            let (u, v) = (at.local[0], at.local[1]);
            assert_eq!(bits(&s.eval(uv).unwrap()), bits(&tile.eval(u.v, v.v)));
            let d = s.eval_partials(uv).unwrap();
            let e = tile.eval_partials(u.v, v.v);
            assert_eq!(bits(&d.du), bits(&e.du));
            assert_eq!(bits(&d.dv), bits(&e.dv));
        }
    }
}

/// `x − i`, exactly.
fn exact_local(x: f64, i: i64) -> Q {
    Q::try_from(x).unwrap() - Q::from(i)
}

/// The local coordinate is `x − i` exactly on every tile but −1, where
/// `x` in `(−½, 0)` is the nearest `f64` with the exact residual as its
/// error; a tile line belongs to the tile above it, the far side to the
/// last tile; `−0` is `+0`; outside the domain, or a NaN, is nowhere.
#[test]
fn local_coordinates_are_exact_but_on_tile_minus_one() {
    let mut rng = Rng::new(35);
    for n in [1i64, 8, 1 << 20, 1 << 52] {
        // Ranges about 0 and far out.
        for range in [-n..n, (n - 3)..(n + 2), (-n - 3)..(-n + 1)] {
            let (lo, hi) = (range.start as f64, range.end as f64);
            for _ in 0..2000 {
                let x = match rng.next_u64() % 4 {
                    0 => rng.range(lo, hi),
                    1 => -rng.log_range(1e-300, 0.5),
                    2 => rng.log_range(1e-300, 1.0),
                    _ => lo + (rng.next_u64() % (range.end.abs_diff(range.start) + 1)) as f64,
                };
                let Some((j, t)) = cell(x, &range) else {
                    assert!(!(lo <= x && x <= hi), "{x} in {range:?}");
                    continue;
                };
                assert!(j as f64 <= x && x <= (j + 1) as f64, "{x} in {j}");
                assert!(x < (j + 1) as f64 || j == range.end - 1, "{x} in {j}");
                let want = exact_local(x, j);
                let got = Q::try_from(t.v).unwrap();
                if j == -1 && x > -0.5 {
                    let r = &want - &got;
                    let r = if r < 0u32 { -r } else { r };
                    assert_eq!(Q::try_from(t.err_round).unwrap(), r, "{x}");
                } else {
                    assert_eq!(got, want, "{x} in {j}");
                    assert!(t.err() == 0.0);
                }
            }
            // The far side is the last tile's, at 1.
            let (j, t) = cell(hi, &range).unwrap();
            assert_eq!((j, t.v, t.err()), (range.end - 1, 1.0, 0.0));
            for x in [f64::NAN, lo.next_down(), hi.next_up(), f64::INFINITY] {
                assert!(cell(x, &range).is_none(), "{x}");
            }
        }
    }
    // Past 2⁵³ an index has no `f64` UV.
    assert!(cell(0.5, &(-(1 << 54)..1)).is_none());
    // A residual too fine for `x + 1`: `x = −2⁻⁶⁰`.
    let tiny = f64::from_bits((1023 - 60) << 52);
    let (j, t) = cell(-tiny, &(-1..1)).unwrap();
    assert_eq!((j, t.v, t.err_round), (-1, 1.0, tiny));
    // −0 is the line 0, at +0 on tile 0.
    let (j, t) = cell(-0.0, &(-1..1)).unwrap();
    assert_eq!((j, t.v.to_bits(), t.err()), (0, 0.0f64.to_bits(), 0.0));
    // The surface locates by the same rule in each direction.
    let key: FormKey = CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], 1.0)
        .unwrap()
        .into();
    let rect = Rect {
        level: Level { u: 1, v: 0 },
        u: 0..3,
        v: -1..1,
    };
    let s = Surface::new(Lattice::new(key).unwrap(), rect).unwrap();
    let at = s.locate([2.25, -0.75]).unwrap();
    assert_eq!((at.i, at.j, at.index), (2, -1, 4));
    assert_eq!(at.local, [Bounded::exact(0.25), Bounded::exact(0.25)]);
    let at = s.locate([3.0, 1.0]).unwrap();
    assert_eq!((at.i, at.j, at.index), (2, 0, 5));
    for uv in [[0.5, f64::NAN], [f64::NAN, 0.0], [-1e-300, 0.0], [0.5, 1.5]] {
        assert!(s.locate(uv).is_none() && s.eval(uv).is_none(), "{uv:?}");
    }
}

/// Positions and indices invert each other over the rectangle and are
/// `None` outside it; a tile is at its position; a row is the tile's on
/// either side of its line, `None` past both.
#[test]
fn positions_indices_and_rows() {
    for key in keys(36) {
        let s = straddling(&key, Level { u: 1, v: 1 });
        let r = s.rect().clone();
        for (k, t) in s.tiles().iter().enumerate() {
            let (i, j) = s.indices(k).unwrap();
            assert!(r.u.contains(&i) && r.v.contains(&j));
            assert_eq!(s.index(i, j), Some(k));
            assert_eq!(s.tile(i, j), Some(t));
            let bits = |r: Row| r.map(|p| p.map(f64::to_bits));
            assert_eq!(
                bits(s.row(Dir::V, i, j).unwrap()),
                bits(row_of(t, Dir::V, 0))
            );
            assert_eq!(
                bits(s.row(Dir::V, i + 1, j).unwrap()),
                bits(row_of(t, Dir::V, 2))
            );
            assert_eq!(
                bits(s.row(Dir::U, j, i).unwrap()),
                bits(row_of(t, Dir::U, 0))
            );
            assert_eq!(
                bits(s.row(Dir::U, j + 1, i).unwrap()),
                bits(row_of(t, Dir::U, 2))
            );
        }
        assert_eq!(s.indices(s.tiles().len()), None);
        assert_eq!(s.index(r.u.end, r.v.start), None);
        assert_eq!(s.index(r.u.start - 1, r.v.start), None);
        assert_eq!(s.index(r.u.start, r.v.end), None);
        assert!(s.row(Dir::V, r.u.end + 1, r.v.start).is_none());
        assert!(s.row(Dir::V, r.u.start - 1, r.v.start).is_none());
        assert!(s.row(Dir::V, r.u.start, r.v.end).is_none());
        assert!(s.row(Dir::U, r.v.end + 1, r.u.start).is_none());
        assert!(s.rows_shared());
    }
}

/// The tree's hits are the tiles whose boxes meet the query, by brute
/// force, ascending; the surface's box holds every tile's and is the
/// tree's; a query is charged, and an exhausted budget stops it.
#[test]
fn the_tree_finds_the_tiles_a_box_meets() {
    let mut rng = Rng::new(38);
    for key in keys(37) {
        let s = straddling(&key, Level { u: 3, v: 3 });
        let hull = s.hull_box();
        let all: Aabb<3> = hull.into();
        assert_eq!(s.tree().bounds(), Some(all));
        assert_eq!(s.tree().len(), s.tiles().len());
        for t in s.tiles() {
            let b: Aabb<3> = t.hull_box().into();
            assert_eq!(all.union(&b), all);
        }
        let mut queries = vec![all];
        for _ in 0..40 {
            let size = rng.log_range(1e-6, 1.0);
            let c: [f64; 3] = std::array::from_fn(|k| rng.range(hull.lo[k], hull.hi[k]));
            let half: [f64; 3] = std::array::from_fn(|k| (hull.hi[k] - hull.lo[k]) * size * 0.5);
            queries.push(Aabb {
                lo: std::array::from_fn(|k| c[k] - half[k]),
                hi: std::array::from_fn(|k| c[k] + half[k]),
            });
        }
        // A tile's own box, and the box of a node of the lattice.
        queries.push(s.tiles()[s.tiles().len() / 2].hull_box().into());
        let node = s.eval([0.0, 0.0]).unwrap().map(|b| b.v);
        queries.push(Aabb::point(node));
        for q in &queries {
            let want: Vec<u32> = s
                .tiles()
                .iter()
                .enumerate()
                .filter(|(_, t)| Aabb::from(t.hull_box()).meets(q))
                .map(|(k, _)| k as u32)
                .collect();
            let mut work = Work::new(&Budget::DEFAULT);
            let got = work
                .pass(PASS, &[()], |_, ledger| {
                    s.tiles_meeting(q, ledger).map_err(Stop::<Infallible>::from)
                })
                .expect("in budget")
                .swap_remove(0);
            assert_eq!(got, want);
            assert!(work.used() > 0);
        }
        // The node's box meets every tile around it.
        let mut work = Work::new(&Budget::DEFAULT);
        let around = work
            .pass(PASS, &[()], |_, ledger| {
                s.tiles_meeting(&Aabb::point(node), ledger)
                    .map_err(Stop::<Infallible>::from)
            })
            .unwrap()
            .swap_remove(0);
        for (i, j) in [(-1, -1), (-1, 0), (0, -1), (0, 0)] {
            if let Some(k) = s.index(i, j) {
                assert!(around.contains(&(k as u32)), "({i}, {j})");
            }
        }
        let mut work = Work::new(&Budget::new(0));
        assert!(
            work.pass(PASS, &[()], |_, ledger| {
                s.tiles_meeting(&all, ledger)
                    .map_err(Stop::<Infallible>::from)
            })
            .is_err()
        );
    }
}
