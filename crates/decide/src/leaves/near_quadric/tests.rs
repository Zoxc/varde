use malachite::Rational;
use varde_testkit::sheets::{self, SMOOTH_RADIUS};

use super::*;
use crate::bezier::Dyadic;
use crate::count::boxes::Extent;
use crate::ids::FaceId;
use crate::num::bounded::Bounded;
use crate::records::keys::Operand;

/// The gap bound of the snap at the default tolerance, `ε/2 = 5·10⁻⁴`.
const B_K: f64 = 5e-4;

/// The smooth pair's cylinder: axis `y` through the origin.
fn cylinder() -> Quadric {
    Quadric::cylinder([0.0; 3], [0.0, 1.0, 0.0], SMOOTH_RADIUS).expect("a cylinder")
}

/// The cylinder's lattice tile of angle cell `k` over `y ∈ [−20, 20]`, as
/// the kernel's lattice lays it for this key: `e₁ = +z`, `e₂ = +x`, angle
/// `[k, k + 1]·90°` from `e₁` towards `e₂` along `u`, the axis along `v`;
/// its `x_u × x_v` points out.
fn cylinder_tile<S: Scalar>(k: i32, like: &S) -> HomPatch<S> {
    let r = SMOOTH_RADIUS;
    let dirs = [
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0],
        [-1.0, 0.0, 0.0],
    ];
    let dir = |i: i32| dirs[i.rem_euclid(4) as usize];
    let (from, to) = (dir(k), dir(k + 1));
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let arc = [
        (from.map(|x| r * x), 1.0),
        ([0, 1, 2].map(|i| r * (from[i] + to[i])), half),
        (to.map(|x| r * x), 1.0),
    ];
    let ys = [-20.0, 0.0, 20.0];
    let net = (0..9).map(|n| {
        let ((p, w), y) = (arc[n / 3], ys[n % 3]);
        [p[0], p[1] + y, p[2], w]
    });
    HomPatch::from_affine(2, 2, net, like).expect("a biquadratic net")
}

fn sheet_patch<S: Scalar>(gap: f64, like: &S) -> HomPatch<S> {
    HomPatch::from_affine(2, 2, sheets::smooth(gap).net, like).expect("a biquadratic net")
}

fn ext(lo: u64, hi: u64, exp: u32) -> Extent {
    Extent::new(Dyadic::new(lo, exp).unwrap(), Dyadic::new(hi, exp).unwrap()).unwrap()
}

fn key(operand: Operand, tile: [i64; 2], u: Extent, v: Extent) -> BoxKey {
    BoxKey::new(operand, FaceId(0), tile, u, v)
}

/// The sheet's box `[½, ½ + 1/128]²`: `x, y ∈ [0, 0.15625]`, where it lies
/// within `b_K` of the cylinder for a gap of at most `10⁻⁴`.
fn centre_box() -> BoxKey {
    key(Operand::B, [0, 0], ext(64, 65, 7), ext(64, 65, 7))
}

/// The cylinder's box over the top: angle `[0, 90°/64]` from `+z`, `y ∈
/// [0, 0.625]`.
fn top_box() -> BoxKey {
    key(Operand::A, [0, 0], ext(0, 1, 6), ext(32, 33, 6))
}

struct Pair<S> {
    target: QuadricPiece,
    sheet: SheetPiece<S>,
}

fn pair<S: Scalar>(
    like: &S,
    gap: f64,
    cell: i32,
    a: &BoxKey,
    b: &BoxKey,
    flipped: [bool; 2],
) -> Pair<S> {
    let q = cylinder();
    let tile = cylinder_tile(cell, like);
    let sheet = sheet_patch(gap, like);
    let fa = QuadricFace {
        quadric: &q,
        patch: &tile,
        flipped: flipped[0],
    };
    let fb = SheetFace {
        patch: &sheet,
        flipped: flipped[1],
    };
    Pair {
        target: QuadricPiece::of_box(&fa, a).expect("a piece"),
        sheet: SheetPiece::of_box(&fa, &fb, b).expect("a piece"),
    }
}

fn levels() -> (Bounded<f64>, Rational) {
    (Bounded::exact(0.0), Rational::from(0))
}

#[test]
fn the_implicit_is_the_cylinders_and_bad_keys_are_refused() {
    let q = cylinder();
    let at = |p: [i64; 3]| q.implicit(&p.map(Rational::from));
    assert_eq!(at([30, 7, 40]), 0u32);
    assert!(at([30, 7, 39]) < 0u32);
    assert!(at([30, -7, 41]) > 0u32);
    // The axis is used as given: twice as long, `|a|²` times the value.
    let q2 = Quadric::cylinder([0.0, 5.0, 0.0], [0.0, 2.0, 0.0], 50.0).unwrap();
    let p = [3, 1, 4].map(Rational::from);
    assert_eq!(q2.implicit(&p), q.implicit(&p) * Rational::from(4));
    assert_eq!(Quadric::cylinder([0.0; 3], [0.0; 3], 1.0), None);
    assert_eq!(Quadric::cylinder([0.0; 3], [1.0, 0.0, 0.0], 0.0), None);
    assert_eq!(
        Quadric::cylinder([f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], 1.0),
        None
    );
}

/// The implicit's form is the implicit at every point, in the foot's
/// frame; a cylinder tile in it is zero exactly.
#[test]
fn the_implicit_form_is_the_implicit() {
    let q = Quadric::cylinder([1.5, -2.0, 0.25], [3.0, -1.0, 2.0], 7.5).unwrap();
    let f = q.implicit_form(&Rational::from(0u32));
    assert_eq!(f.degree(), 2);
    let one = Rational::from(1u32);
    for p in [[0, 0, 0], [3, 1, 4], [-7, 2, 9], [10, -10, 5]] {
        let p = p.map(Rational::from);
        assert_eq!(f.eval_hom(&p, &one), q.implicit(&p));
    }
    let tile = cylinder_tile(1, &Rational::from(0u32));
    let g = tile
        .compose_implicit(&cylinder().implicit_form(&Rational::from(0u32)))
        .expect("degrees (4, 4)");
    assert_eq!(g.degrees(), (4, 4));
    // The tile's middle weight is `√½` rounded: off the cylinder by its
    // rounding, every coefficient tiny against `ρ²|a|²`, not zero.
    let scale = crate::num::exact::rat(SMOOTH_RADIUS * SMOOTH_RADIUS);
    for c in g.coefs() {
        assert!(c.clone() * Rational::from(1u64 << 40) < scale, "{c}");
        assert!(-(c.clone()) * Rational::from(1u64 << 40) < scale, "{c}");
    }
    assert!(g.coefs().iter().any(|c| *c != 0u32));
}

/// Touching the top: Near along about `+z` when the sheet faces out as
/// the cylinder does, Contact when it faces in, at the first level and
/// exactly. (The touch is at the boxes' corner, so neither Empty nor
/// Simple certifies.)
fn near_and_contact_at<S: Scalar>(like: S) {
    let (a, b) = (top_box(), centre_box());
    let mut w = LeafWork::default();
    let p = pair(&like, 0.0, 0, &a, &b, [false, false]);
    assert_eq!(empty(&p.target, &p.sheet), None);
    assert_eq!(simple(&p.sheet), None);
    let t = near(&p.target, &p.sheet, B_K, &mut w).expect("near");
    assert!(t.ell[2] > 0.0 && t.ell[0].abs() < 0.1 * t.ell[2] && t.ell[1].abs() < 0.1 * t.ell[2]);
    assert!(t.cone.contains(&t.ell));
    assert!(t.cone.contains(&[0.0, 0.0, 1.0]));
    assert!(t.cone.contains(&[0.0, 0.0, 3.0]));
    assert!(!t.cone.contains(&[0.0, 0.0, -1.0]));
    assert!(!t.cone.contains(&[1.0, 0.0, 0.0]));
    assert!(!t.cone.contains(&[0.0, 1.0, 0.0]));
    assert_eq!(t.cone.b_k(), B_K);
    assert!(!t.cone.gens().is_empty());
    assert!(t.cone.gens().iter().all(|g| g[2] > 0.0));
    assert_eq!(contact(&p.target, &p.sheet, B_K, &mut w), None);
    assert!(matches!(
        certify(&p.target, &p.sheet, B_K, &mut w),
        Some(Kind::Near(_))
    ));

    let p = pair(&like, 0.0, 0, &a, &b, [false, true]);
    assert_eq!(near(&p.target, &p.sheet, B_K, &mut w), None);
    let t = contact(&p.target, &p.sheet, B_K, &mut w).expect("contact");
    assert!(t.ell[2] > 0.0);
    assert!(t.cone.contains(&[0.0, 0.0, 1.0]));
    assert!(matches!(
        certify(&p.target, &p.sheet, B_K, &mut w),
        Some(Kind::Contact(_))
    ));

    // Both flipped: co-oriented again, along `−z`.
    let p = pair(&like, 0.0, 0, &a, &b, [true, true]);
    let t = near(&p.target, &p.sheet, B_K, &mut w).expect("near");
    assert!(t.ell[2] < 0.0);
    assert!(w.programs > 0);
}

#[test]
fn just_above_the_top_is_near_or_contact() {
    let (l0, exact) = levels();
    near_and_contact_at(l0);
    near_and_contact_at(exact);
}

/// The T1 gap is a bound of the gap along `ℓ`: farther than `b_K` above
/// the top the pair is never Near (and is Empty).
#[test]
fn farther_than_b_k_is_not_near_but_empty() {
    let (l0, exact) = levels();
    let mut w = LeafWork::default();
    for gap in [6e-4, 2e-3] {
        let p = pair(&l0, gap, 0, &top_box(), &centre_box(), [false, false]);
        assert_eq!(near(&p.target, &p.sheet, B_K, &mut w), None, "{gap}");
        assert!(empty(&p.target, &p.sheet).is_some());
        let p = pair(&exact, gap, 0, &top_box(), &centre_box(), [false, false]);
        assert_eq!(near(&p.target, &p.sheet, B_K, &mut w), None, "{gap}");
    }
    // A gap bound of zero certifies no box of a sheet that is not on
    // the cylinder.
    let p = pair(&exact, 0.0, 0, &top_box(), &centre_box(), [false, false]);
    assert_eq!(near(&p.target, &p.sheet, 0.0, &mut w), None);
}

/// A sheet crossing the cylinder away from its closest point is Simple:
/// the implicit grows along `u` there.
#[test]
fn a_transversal_crossing_is_simple() {
    let (_, exact) = levels();
    // Below the top by 0.01: the crossing runs through `x ≈ 1.41` on
    // `y = 0`; the box `x ∈ [1.25, 1.5625]`, `y ∈ [0, 0.3125]`.
    let b = key(Operand::B, [0, 0], ext(36, 37, 6), ext(32, 33, 6));
    let a = key(Operand::A, [0, 0], ext(0, 1, 5), ext(16, 17, 5));
    let p = pair(&exact, -0.01, 0, &a, &b, [false, false]);
    let mut w = LeafWork::default();
    assert_eq!(empty(&p.target, &p.sheet), None);
    assert_eq!(simple(&p.sheet), Some(Dir::U));
    assert_eq!(
        certify(&p.target, &p.sheet, B_K, &mut w),
        Some(Kind::Simple { dir: Dir::U })
    );
}

/// The line through the sheet along `ℓ` meets the cylinder again on its
/// far side, 100 mm below: a quadric piece there is never Near to the
/// sheet, though the T1 gap at the top holds.
#[test]
fn the_far_side_of_the_cylinder_is_never_near() {
    let (l0, exact) = levels();
    let whole = key(Operand::A, [1, 0], Extent::WHOLE, Extent::WHOLE);
    let mut w = LeafWork::default();
    // Cell 1 runs from `+x` to `−z`: under the sheet's box.
    let far = pair(&l0, 0.0, 1, &whole, &centre_box(), [false, false]);
    assert_eq!(near(&far.target, &far.sheet, B_K, &mut w), None);
    let top = pair(&l0, 0.0, 0, &whole, &centre_box(), [false, false]);
    assert!(near(&top.target, &top.sheet, B_K, &mut w).is_some());
    let far = pair(&exact, 0.0, 1, &whole, &centre_box(), [false, false]);
    assert_eq!(near(&far.target, &far.sheet, B_K, &mut w), None);
}

/// Every leaf of the smooth pair over both cylinder tiles under the
/// sheet, all certified.
fn smooth_leaves<S: Scalar>(like: &S, gap: f64, flipped: bool) -> Vec<Leaf<S>> {
    let q = cylinder();
    let sheet = sheet_patch(gap, like);
    let fb = SheetFace {
        patch: &sheet,
        flipped,
    };
    let b0 = key(Operand::B, [0, 0], Extent::WHOLE, Extent::WHOLE);
    let mut all = Vec::new();
    for cell in [-1, 0] {
        let tile = cylinder_tile(cell, like);
        let fa = QuadricFace {
            quadric: &q,
            patch: &tile,
            flipped: false,
        };
        let a0 = key(Operand::A, [cell as i64, 0], Extent::WHOLE, Extent::WHOLE);
        let (ls, work) = leaves(fa, a0, fb, b0, B_K, 1 << 20, |_| BoxUse::Test).expect("leaves");
        assert!(work.pairs as usize >= ls.len());
        all.extend(ls);
    }
    all
}

/// The sheet's points over `b` on a grid, in `f64`.
fn samples(gap: f64, b: &BoxKey) -> Vec<[f64; 3]> {
    let net = sheets::smooth(gap).net;
    let bern = |t: f64| [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
    let [u, v] = [Dir::U, Dir::V].map(|d| {
        let e = b.extent(d);
        let (lo, hi) = (e.lo().to_f64().unwrap(), e.hi().to_f64().unwrap());
        (0..=4).map(move |k| lo + (hi - lo) * k as f64 / 4.0)
    });
    let vs: Vec<f64> = v.collect();
    let mut out = Vec::new();
    for s in u {
        for &t in &vs {
            let (bu, bv) = (bern(s), bern(t));
            let mut p = [0.0; 3];
            for (n, c) in net.iter().enumerate() {
                let w = bu[n / 3] * bv[n % 3];
                for k in 0..3 {
                    p[k] += w * c[k];
                }
            }
            out.push(p);
        }
    }
    out
}

fn check_smooth<S: Scalar>(like: &S) {
    for (gap, flipped) in [(0.0, false), (-1e-4, false), (0.0, true)] {
        let ls = smooth_leaves(like, gap, flipped);
        let tangent: Vec<&Leaf<S>> = ls
            .iter()
            .filter(|l| matches!(l.kind, Kind::Near(_) | Kind::Contact(_)))
            .collect();
        assert!(!tangent.is_empty(), "{gap} {flipped}");
        for l in &tangent {
            let t = match (&l.kind, flipped) {
                (Kind::Near(t), false) | (Kind::Contact(t), true) => t,
                _ => panic!("{:?}", l.kind),
            };
            assert!(t.cone.contains(&t.ell));
            // Within `b_K` of the cylinder (vertically, to second order).
            for p in samples(gap, &l.b) {
                let z = libm::sqrt(SMOOTH_RADIUS * SMOOTH_RADIUS - p[0] * p[0]);
                assert!((p[2] - z).abs() <= B_K * 1.001, "{p:?} {:?}", l.b);
            }
        }
    }
    // Apart, however close: the implicit along the sheet is positive and
    // its least value lies on a box corner, so it certifies Empty at once.
    for gap in [1e-6, 1e-4, 2e-3] {
        let ls = smooth_leaves(like, gap, false);
        assert!(ls.iter().all(|l| matches!(l.kind, Kind::Empty(_))), "{gap}");
    }
}

/// The smooth pair touching (or crossing in a small ellipse): Near
/// leaves (Contact for the flipped sheet) at the closest point, each
/// within `b_K` of the cylinder; apart, only Empty leaves.
#[test]
fn the_smooth_pair_is_near_at_its_closest_point() {
    let (l0, _) = levels();
    check_smooth(&l0);
}

#[test]
fn the_smooth_pair_is_near_at_its_closest_point_exactly() {
    let (_, exact) = levels();
    check_smooth(&exact);
}

#[test]
fn rational_sheets_are_refused() {
    let (l0, _) = levels();
    let q = cylinder();
    let tile = cylinder_tile(0, &l0);
    let fa = QuadricFace {
        quadric: &q,
        patch: &tile,
        flipped: false,
    };
    // The cylinder's own tile as a sheet: its weights differ.
    let fb = SheetFace {
        patch: &tile,
        flipped: false,
    };
    assert_eq!(
        SheetPiece::of_box(&fa, &fb, &centre_box()),
        Err(PieceError::Weights)
    );
}

/// The smooth pair's leaves against the exact oracle
/// ([`varde_testkit::rational_oracle::SmoothOracle`]): at every sample of a
/// Near or Contact leaf's sheet box the sheet lies within `b_K` of the
/// cylinder (`|√D − r| ≤ b_K` iff `|D − r²| ≤ b_K·(2r + b_K)`, exactly), at
/// every sample of an Empty-by-implicit box strictly on one side of it,
/// and the oracle's relation is the leaves': clear of the cylinder exactly
/// where every leaf is Empty, else a tangent leaf holds its least point.
#[test]
fn the_smooth_pair_s_leaves_agree_with_the_exact_oracle() {
    use malachite::base::num::arithmetic::traits::PowerOf2;
    use varde_testkit::rational_oracle::{SmoothOracle, SmoothRelation};
    let (l0, _) = levels();
    let rat = |x: f64| Rational::try_from(x).expect("finite");
    let r = rat(SMOOTH_RADIUS);
    let bk = rat(B_K);
    let near_bound = &bk * (Rational::from(2) * &r + &bk);
    for (gap, flipped) in [
        (-1e-4, false),
        (0.0, false),
        (0.0, true),
        (1e-6, false),
        (1e-4, false),
        (2e-3, false),
    ] {
        let oracle = SmoothOracle::new(&sheets::smooth(gap)).expect("the oracle takes the pair");
        let ls = smooth_leaves(&l0, gap, flipped);
        let grid = |b: &BoxKey| {
            let [u, v] = [Dir::U, Dir::V].map(|d| {
                let e = b.extent(d);
                let (lo, hi) = (e.lo().rational(), e.hi().rational());
                (0..=4u32)
                    .map(|k| &lo + (&hi - &lo) * Rational::from(k) / Rational::from(4u32))
                    .collect::<Vec<_>>()
            });
            let mut out = Vec::new();
            for s in &u {
                for t in &v {
                    out.push(oracle.implicit(s, t));
                }
            }
            out
        };
        for l in &ls {
            match &l.kind {
                Kind::Near(_) | Kind::Contact(_) => {
                    for d in grid(&l.b) {
                        assert!(
                            d <= near_bound && -d <= near_bound,
                            "{gap} {flipped}: {:?}",
                            l.b
                        );
                    }
                }
                Kind::Empty(Separation::Implicit) => {
                    let ds = grid(&l.b);
                    let pos = ds.iter().all(|d| *d > 0u32);
                    let neg = ds.iter().all(|d| *d < 0u32);
                    assert!(pos || neg, "{gap} {flipped}: {:?}", l.b);
                }
                Kind::Empty(Separation::Boxes) | Kind::Simple { .. } => {}
            }
        }
        let tangent: Vec<&Leaf<_>> = ls
            .iter()
            .filter(|l| matches!(l.kind, Kind::Near(_) | Kind::Contact(_)))
            .collect();
        match oracle.relation(&Rational::power_of_2(-60i64), 4000) {
            SmoothRelation::Clear(_) => {
                assert!(ls.iter().all(|l| matches!(l.kind, Kind::Empty(_))), "{gap}");
            }
            SmoothRelation::Crossing(least) | SmoothRelation::Undecided(least) => {
                let holds = |l: &&Leaf<_>| {
                    [Dir::U, Dir::V].iter().zip(&least.at).all(|(&d, x)| {
                        let e = l.b.extent(d);
                        e.lo().rational() <= *x && *x <= e.hi().rational()
                    })
                };
                assert!(tangent.iter().any(holds), "{gap} {flipped}");
            }
        }
    }
}
