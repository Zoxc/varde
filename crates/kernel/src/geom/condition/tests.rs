//! Conditioning against its definitions: standard spans are the given
//! curve under the Möbius map and have the least `κ_w` of any map;
//! profiles given with unequal joint weights tile and share their rows;
//! tiles cache their `κ_w`; an edge's chains keep their knot points and
//! move by one map, exactly.

use malachite::Rational as Q;
use varde_decide::bezier::Dir;
use varde_testkit::rng::Rng;

use super::*;
use crate::geom::conic::Conic;
use crate::geom::curve::Span3;
use crate::geom::form::{ConicCylinderKey, CylinderKey, RevolvedKey, SphereKey};
use crate::geom::forms::cyl::{conic_chain, cylinder};
use crate::geom::forms::revolved::revolved;
use crate::geom::forms::sphere::sphere;
use crate::geom::lattice::{Lattice, Level, Rect};
use crate::geom::round::nearest;

const Z: [f64; 3] = [0.0, 0.0, 1.0];
const XY: [[f64; 3]; 2] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

/// The exact point of the rational quadratic `(x, w)` rows at `s`.
fn at<const D: usize>(span: &[[f64; D]; 3], s: &Q) -> Vec<Q> {
    let one = Q::from(1u32);
    let b = [
        (&one - s) * (&one - s),
        Q::from(2u32) * s * (&one - s),
        s * s,
    ];
    let w: Q = (0..3).map(|i| &b[i] * q(span[i][D - 1])).sum();
    (0..D - 1)
        .map(|k| {
            (0..3)
                .map(|i| &b[i] * q(span[i][D - 1]) * q(span[i][k]))
                .sum::<Q>()
                / &w
        })
        .collect()
}

/// The given span's parameter `s = ρσ/(1 − σ + ρσ)` at the conditioned
/// `σ`, for `ρ` exact.
fn given(rho: &Q, sigma: &Q) -> Q {
    let r = rho * sigma;
    &r / (Q::from(1u32) - sigma + &r)
}

#[test]
fn standard_spans_are_the_given_curve() {
    // Hyperbola arcs over `[s₀, s₁]` with `s₀·s₁` a square: `√(w₀/w₂)`
    // is rational, so the middle weight and the map are exact.
    let h = Conic::<2>::hyperbola([0.5, -1.0], [[1.0, 0.0], [0.0, 1.0]], [1.0, 0.75]).unwrap();
    for (s0, s1, rho) in [(1.0, 4.0, Q::from(2u32)), (0.25, 4.0, Q::from(4u32))] {
        let span = h.arc::<3>(s0, s1).unwrap();
        let std = standard(&span);
        assert_eq!([std[0][2], std[2][2]], [1.0, 1.0]);
        for k in 0..3 {
            assert_eq!(&std[k][..2], &span[k][..2], "control points kept");
        }
        // `ρ = √(w₀/w₂)` and its inverse orientation: the given span is
        // the standard one's map.
        let rho = Q::from(1u32) / rho;
        for n in 0..=8 {
            let sigma = Q::from_signeds(n, 8);
            assert_eq!(at(&std, &sigma), at(&span, &given(&rho, &sigma)));
        }
    }
}

#[test]
fn standard_spans_keep_standard_bits() {
    let w = nearest_sqrt(&Q::from_signeds(1, 2));
    let quarter = [[1.0, 0.0, 1.0], [1.0, 1.0, w], [0.0, 1.0, 1.0]];
    assert_eq!(standard(&quarter), quarter);
    let flat = [[1.0, 2.0, 1.0], [2.0, 2.0, 2.0], [3.0, 2.0, 1.0]];
    assert_eq!(standard(&flat), flat);
}

#[test]
fn standard_spans_have_the_least_ratio() {
    let mut rng = Rng::new(7);
    for _ in 0..200 {
        let w = [0, 1, 2].map(|_| rng.log_range(1.0 / 64.0, 64.0));
        let span = [[0.0, 0.0, w[0]], [1.0, 1.0, w[1]], [2.0, 0.0, w[2]]];
        let best = kappa(standard(&span).map(|p| p[2]));
        for _ in 0..20 {
            let rho = rng.log_range(1e-3, 1e3);
            let other = kappa([w[0], w[1] * rho, w[2] * rho * rho]);
            assert!(best <= other * (1.0 + 1e-14), "{w:?}: {best} > {other}");
        }
    }
}

/// Hyperbola arcs in space whose middle weights fall in different
/// binades, so their joints' weights differ (2 then 1).
fn binade_arcs() -> Vec<Span3> {
    let h = Conic::hyperbola([0.0; 3], XY, [1.0, 0.5]).unwrap();
    [0.6, 0.8, 1.0, 1.2, 1.5]
        .windows(2)
        .map(|s| h.arc::<4>(s[0], s[1]).unwrap())
        .collect()
}

#[test]
fn profiles_with_unequal_joint_weights_tile() {
    let arcs = binade_arcs();
    assert!(
        arcs.windows(2).any(|p| p[0][2][3] != p[1][0][3]),
        "the fixture's joints differ"
    );
    let key = ConicCylinderKey::new(Z, &arcs).unwrap();
    let s = conic_chain(key.clone(), 0..4).unwrap();
    let l = s.lattice();
    let spans = l.profile().unwrap();
    for (k, t) in s.tiles().iter().enumerate() {
        // The column's weights are its standard span's, times 1.
        let w = spans[k].map(|p| p[2]);
        for n in 0..9 {
            assert_eq!(t.net()[n][3], w[n / 3], "tile {k} weight {n}");
        }
        assert_eq!(t.kappa_w(), kappa(w));
        assert!(t.is_regular(), "tile {k}");
    }
    // Neighbours share their row, weights included, at every level.
    for level in [Level::BASE, Level { u: 2, v: 1 }] {
        let rect = Rect {
            level,
            u: 0..(4 << level.u),
            v: 0..(1 << level.v),
        };
        let tiles = l.tiles(&rect).unwrap();
        let nv = (rect.v.end - rect.v.start) as usize;
        for a in 0..tiles.len() / nv - 1 {
            for b in 0..nv {
                let (t, s) = (&tiles[a * nv + b], &tiles[(a + 1) * nv + b]);
                for r in 0..3 {
                    assert_eq!(
                        t.net()[6 + r].map(f64::to_bits),
                        s.net()[r].map(f64::to_bits),
                        "{level:?} column {a}"
                    );
                }
            }
        }
    }
    // Revolved about an axis clear of them, the same rows.
    let meridian: Vec<Span2> = arcs
        .iter()
        .map(|s| s.map(|p| [p[0] + 3.0, p[1], p[3]]))
        .collect();
    let key = RevolvedKey::new([0.0; 3], Z, &meridian).unwrap();
    let s = revolved(key, 0, 0..4).unwrap();
    let nv = 4;
    for a in 0..2 {
        for b in 0..nv - 1 {
            let (t, n) = (&s.tiles()[a * nv + b], &s.tiles()[a * nv + b + 1]);
            for r in 0..3 {
                assert_eq!(
                    t.net()[3 * r + 2].map(f64::to_bits),
                    n.net()[3 * r].map(f64::to_bits),
                    "row {b}"
                );
            }
        }
    }
}

#[test]
fn a_profile_tile_follows_the_given_span() {
    // Arcs whose standard map is exact (`ρ = 2`): the tile at `σ` is the
    // given span at `s(σ)`, to the coordinates' rounding.
    let h = Conic::hyperbola([0.25, 0.5, 0.0], XY, [1.0, 0.75]).unwrap();
    let span = h.arc::<4>(1.0, 4.0).unwrap();
    let key = ConicCylinderKey::new(Z, &[span]).unwrap();
    let s = conic_chain(key, 0..1).unwrap();
    let given_span = s.lattice().key().profile().unwrap().spans()[0];
    // `ρ = √(w₀/w₂)`: the key may have reversed the span.
    let rho = if given_span[0][2] < given_span[2][2] {
        Q::from_signeds(1, 2)
    } else {
        Q::from(2u32)
    };
    let t = &s.tiles()[0];
    let e = t.exact();
    for n in 1..8 {
        let sigma = Q::from_signeds(n, 8);
        let p = e.eval(&sigma, &Q::from_signeds(1, 2));
        let want = at(&given_span, &given(&rho, &sigma));
        // The profile lies in the frame's `e₁`, `e₂` plane through the
        // world origin: `x·e₁ + y·e₂`.
        let [e1, e2, _] = s.lattice().axes();
        for c in 0..2 {
            let w = q(e1[c]) * &want[0] + q(e2[c]) * &want[1];
            let d = nearest(&(&p[c] - &w)).abs();
            assert!(d <= 1e-15, "σ = {n}/8: {d:e}");
        }
    }
}

#[test]
fn tiles_cache_their_ratio() {
    let root = nearest_sqrt(&Q::from_signeds(1, 2));
    let c = cylinder(CylinderKey::new([0.0; 3], Z, 2.0).unwrap(), 0).unwrap();
    assert!(c.tiles().iter().all(|t| t.kappa_w() == 1.0 / root));
    let s = sphere(SphereKey::new([1.0, 2.0, 3.0], 2.0, Z).unwrap(), 0).unwrap();
    assert!(s.tiles().iter().all(|t| t.kappa_w() == 2.0));
    // A tile's halves have their own.
    let [lo, _] = s.tiles()[0].split_half(Dir::U).unwrap();
    assert_eq!(lo.kappa_w(), kappa(lo.net().iter().map(|p| p[3])));
    assert!(lo.kappa_w() < 2.0);
}

#[test]
fn ratios_of_weights() {
    assert_eq!(kappa([]), 1.0);
    assert_eq!(kappa([0.5, 2.0, 1.0]), 4.0);
    assert_eq!(kappa([3.0]), 1.0);
    assert_eq!(kappa([1.0, 0.0]), f64::INFINITY);
}

#[test]
fn a_span_beyond_the_weight_range_has_no_tile() {
    // `ω = 60/(1/64) = 3840`: no map brings it within `[1/64, 64]`.
    let span = [
        [1.0, 0.0, 1.0 / 64.0],
        [2.0, 1.0, 60.0],
        [3.0, 0.0, 1.0 / 64.0],
    ];
    let key = RevolvedKey::new([0.0; 3], Z, &[span]).unwrap();
    let l = Lattice::new(key.into()).unwrap();
    assert!(matches!(
        l.base_tile(0, 0),
        Err(crate::geom::lattice::LatticeError::Tile(
            crate::geom::tile::TileError::Weight { .. }
        ))
    ));
}

/// An edge's chains for the edge tests: a space curve of two spans with
/// skewed weights, and two p-curves over other knots, one with a G and
/// an iso span.
fn edge() -> (Curve3D, PCurve, PCurve) {
    let c = Curve3D::new(
        vec![
            [
                [0.0, 0.0, 0.0, 0.25],
                [1.0, 1.0, 0.0, 1.5],
                [2.0, 0.0, 1.0, 8.0],
            ],
            [
                [2.0, 0.0, 1.0, 1.0],
                [3.0, -1.0, 1.0, 0.125],
                [4.0, 0.0, 0.0, 2.0],
            ],
        ],
        vec![0.0, 1.0, 3.0],
    )
    .unwrap();
    let p = PCurve::new(
        vec![
            [[0.0, 0.0, 1.0], [0.5, 0.25, 4.0], [1.0, 0.5, 16.0]],
            [[1.0, 0.5, 2.0], [1.5, 0.75, 1.0], [2.0, 1.0, 0.5]],
        ],
        vec![0.0, 2.0, 3.0],
    )
    .unwrap();
    let r = PCurve::from_spans(
        vec![
            PSpan::G {
                mobius: Dir::U,
                m: [[0.0, 1.0], [0.5, 8.0]],
                q: [[0.0, 0.5], [0.25, 1.0], [0.75, 4.0]],
            },
            PSpan::Iso {
                fixed: Dir::U,
                at: 0.5,
                m: [[0.75, 4.0], [1.0, 1.0 / 32.0]],
            },
        ],
        vec![0.0, 0.5, 3.0],
    )
    .unwrap();
    (c, p, r)
}

/// The exact point of a p-curve span at local `s`, per coordinate.
fn pspan_at(s: &PSpan, t: &Q) -> Vec<Q> {
    s.coords()
        .iter()
        .map(|c| {
            let v = c.values();
            let one = Q::from(1u32);
            let d = v.len() - 1;
            let b: Vec<Q> = (0..=d)
                .map(|i| {
                    let binom = Q::from([1u32, d as u32, 1][i.min(2)].max(1));
                    let mut x = binom;
                    for _ in 0..i {
                        x *= t;
                    }
                    for _ in i..d {
                        x *= &one - t;
                    }
                    x
                })
                .collect();
            let w: Q = (0..=d).map(|i| &b[i] * q(v[i][1])).sum();
            (0..=d).map(|i| &b[i] * q(v[i][1]) * q(v[i][0])).sum::<Q>() / w
        })
        .collect()
}

#[test]
fn an_edge_moves_by_one_map() {
    let (c, p, r) = edge();
    let e = condition_edge(&c, &[&p, &r]).unwrap();
    let knots = [0.0, 0.5, 1.0, 2.0, 3.0];
    assert_eq!(e.curve.knots(), knots);
    assert!(e.pcurves.iter().all(|x| x.knots() == knots));
    assert_eq!(e.shifts.len(), 4);
    assert!(e.shifts.iter().any(|&k| k != 0), "{:?}", e.shifts);
    // The chains as split, before the map: the same knot points.
    let c0 = c.split(&knots).unwrap();
    let ps0 = [p.split(&knots).unwrap(), r.split(&knots).unwrap()];
    for k in 0..knots.len() {
        assert_eq!(e.curve.knot_point(k), c0.knot_point(k));
        for (a, b) in e.pcurves.iter().zip(&ps0) {
            assert_eq!(a.knot_point(k), b.knot_point(k));
        }
    }
    for (j, &k) in e.shifts.iter().enumerate() {
        // `wᵢ·2^{ik}`: the conditioned span at `σ` is the split one at
        // `s = ρσ/(1 − σ + ρσ)` with `ρ = 2ᵏ`, for every chain alike.
        let rho = q(times_pow2(1.0, k));
        for n in 1..8 {
            let sigma = Q::from_signeds(n, 8);
            let s = given(&rho, &sigma);
            assert_eq!(at(&e.curve.spans()[j], &sigma), at(&c0.spans()[j], &s));
            for (a, b) in e.pcurves.iter().zip(&ps0) {
                assert_eq!(a.spans()[j].kind(), b.spans()[j].kind());
                assert_eq!(pspan_at(&a.spans()[j], &sigma), pspan_at(&b.spans()[j], &s));
            }
        }
        // The map lowers the interval's worst ratio, or keeps it.
        let worst = |c: &Curve3D, ps: &[PCurve]| {
            let mut rows = vec![c.spans()[j].iter().map(|p| p[3]).collect::<Vec<_>>()];
            for x in ps {
                rows.extend(weight_rows(&x.spans()[j]));
            }
            rows.iter()
                .map(|r| kappa(r.iter().copied()))
                .fold(0.0, |m, x| if x > m { x } else { m })
        };
        assert!(worst(&e.curve, &e.pcurves) <= worst(&c0, &ps0));
    }
}

#[test]
fn an_edge_on_common_knots_with_balanced_weights_keeps_its_bits() {
    let c = Curve3D::new(
        vec![[
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0, 0.75],
            [2.0, 0.0, 0.0, 1.0],
        ]],
        vec![0.0, 1.0],
    )
    .unwrap();
    let p = PCurve::new(
        vec![[[0.0, 0.0, 1.0], [0.5, 0.5, 0.75], [1.0, 0.0, 1.0]]],
        vec![0.0, 1.0],
    )
    .unwrap();
    let e = condition_edge(&c, &[&p]).unwrap();
    assert_eq!(e.shifts, [0]);
    assert_eq!(e.curve, c);
    assert_eq!(e.pcurves, [p]);
}

#[test]
fn an_edge_needs_one_range() {
    let (c, p, _) = edge();
    let short = p.slice(0.0, 2.0).unwrap();
    assert_eq!(
        condition_edge(&c, &[&p, &short]).unwrap_err(),
        ConditionError::Range { pcurve: 1 }
    );
}
