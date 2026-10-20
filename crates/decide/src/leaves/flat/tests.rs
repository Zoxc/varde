use malachite::Rational;
use varde_testkit::sheets::{self, OFFSET, Polygons, Wavy};

use super::*;
use crate::bezier::{Dyadic, HomPatch};
use crate::count::boxes::{BoxKey, Extent};
use crate::ids::FaceId;
use crate::num::exact::rat;
use crate::records::keys::Operand;

/// The gap bound of the snap at the default tolerance, `ε/2 = 5·10⁻⁴`.
const B_K: f64 = 5e-4;

fn q3(p: [f64; 3]) -> Q3 {
    p.map(rat)
}

fn qi(p: [i64; 3]) -> Q3 {
    p.map(Rational::from)
}

/// The parallelogram `p0 + u·(pu − p0) + v·(pv − p0)`, exactly, as a
/// bilinear tile.
fn quad(p0: [f64; 3], pu: [f64; 3], pv: [f64; 3]) -> HomPatch<Rational> {
    let (p0, pu, pv) = (q3(p0), q3(pu), q3(pv));
    let p11: Q3 = [0, 1, 2].map(|k| &pu[k] + &pv[k] - &p0[k]);
    let h = |p: &Q3| [p[0].clone(), p[1].clone(), p[2].clone(), Rational::from(1)];
    HomPatch::new(1, 1, [h(&p0), h(&pv), h(&pu), h(&p11)]).expect("a bilinear net")
}

fn whole(operand: Operand, face: u32) -> BoxKey {
    BoxKey::new(operand, FaceId(face), [0, 0], Extent::WHOLE, Extent::WHOLE)
}

fn piece_of(patch: &HomPatch<Rational>, flipped: bool) -> FlatPiece {
    FlatPiece::of_box(patch, &whole(Operand::A, 0), flipped).expect("a polynomial patch")
}

/// The axis-aligned square `[x0, x0 + s] × [y0, y0 + s]` at height `z`,
/// facing up.
fn square(x0: f64, y0: f64, s: f64, z: f64) -> HomPatch<Rational> {
    quad([x0, y0, z], [x0 + s, y0, z], [x0, y0 + s, z])
}

/// A polygon face's tile: the parallelogram on its first corner and the
/// corners before and after it, facing as the loop does.
fn face_tile(body: &Polygons, f: usize) -> HomPatch<Rational> {
    let l = &body.faces[f];
    let v = |k: u32| body.verts[k as usize];
    quad(v(l[0]), v(l[1]), v(l[l.len() - 1]))
}

fn work() -> LeafWork {
    LeafWork::default()
}

/// Whether `d` separates the hulls of `a` and `b`, exactly.
fn separates(d: &Q3, a: &FlatPiece, b: &FlatPiece) -> bool {
    let (_, hi_a) = extent(a.points(), d);
    let (lo_b, _) = extent(b.points(), d);
    hi_a < lo_b
}

#[test]
fn a_plane_piece_has_its_normal_as_its_one_graph_vector() {
    let up = piece_of(&square(0.0, 0.0, 4.0, 1.0), false);
    assert_eq!(up.graph(), &[qi([0, 0, 1])]);
    assert_eq!(up.points().len(), 4);
    assert_eq!(up.world_box(), [[0.0, 0.0, 1.0], [4.0, 4.0, 1.0]]);
    let down = piece_of(&square(0.0, 0.0, 4.0, 1.0), true);
    assert_eq!(down.graph(), &[qi([0, 0, -1])]);
    // A tilted plane: one vector, scaled to a largest component of 1.
    let tilted = piece_of(&quad([0.0; 3], [2.0, 0.0, 1.0], [0.0, 3.0, 0.0]), false);
    let half = Rational::from_signeds(1, 2);
    assert_eq!(
        tilted.graph(),
        &[[-half, Rational::from(0), Rational::from(1)]]
    );
}

#[test]
fn a_box_piece_is_the_exact_sub_patch_and_its_world_box_is_outward() {
    let tile = square(0.0, 0.0, 3.0, 0.0);
    let lo = Extent::new(Dyadic::ZERO, Dyadic::new(1, 1).unwrap()).unwrap();
    let b = BoxKey::new(Operand::B, FaceId(2), [0, 0], lo, Extent::WHOLE);
    let p = FlatPiece::of_box(&tile, &b, false).unwrap();
    let mut xs: Vec<Rational> = p.points().iter().map(|p| p[0].clone()).collect();
    xs.dedup();
    assert_eq!(xs, [Rational::from(0), Rational::from_signeds(3, 2)]);
    assert_eq!(p.world_box(), [[0.0, 0.0, 0.0], [1.5, 3.0, 0.0]]);
    // A coordinate that isn't a float rounds outward.
    let r = |x: i64| Rational::from(x);
    let third = Rational::from_signeds(1, 3);
    let net = [
        [r(0), r(0), r(0), r(1)],
        [r(0), r(1), r(0), r(1)],
        [third.clone(), r(0), r(0), r(1)],
        [third.clone(), r(1), r(0), r(1)],
    ];
    let w = piece_of(&HomPatch::new(1, 1, net).unwrap(), false).world_box();
    assert!(rat(w[1][0]) > third && rat(w[1][0]) < Rational::from_signeds(1, 2));
    assert_eq!(w[0], [0.0; 3]);
}

#[test]
fn rational_weights_and_bad_nets_are_refused() {
    let r = |x: i64| Rational::from(x);
    let net = [
        [r(0), r(0), r(0), r(1)],
        [r(0), r(2), r(0), r(2)],
        [r(1), r(0), r(0), r(1)],
        [r(1), r(1), r(0), r(1)],
    ];
    let p = HomPatch::new(1, 1, net).unwrap();
    assert_eq!(
        FlatPiece::of_box(&p, &whole(Operand::A, 0), false),
        Err(PieceError::Weights)
    );
    assert_eq!(
        FlatPiece::from_net(1, 1, &[[0.0; 3]; 3], false),
        Err(PieceError::Net)
    );
    assert_eq!(
        FlatPiece::from_net(1, 0, &[[0.0; 3]; 2], false),
        Err(PieceError::Net)
    );
    assert_eq!(
        FlatPiece::from_net(1, 1, &[[f64::NAN, 0.0, 0.0]; 4], false),
        Err(PieceError::NotFinite)
    );
}

#[test]
fn parallel_pieces_apart_are_empty_however_close() {
    // Pieces δ apart certify Empty first, whatever b_K: flat pieces
    // closer than b_K but apart stay exact and form no cluster.
    let a = piece_of(&square(0.0, 0.0, 10.0, 0.0), false);
    let b = piece_of(&square(2.0, 3.0, 10.0, OFFSET), false);
    let mut w = work();
    let kind = certify(&a, &b, &rat(B_K), &mut w);
    assert_eq!(kind, Some(Kind::Empty { d: qi([0, 0, 1]) }));
    assert_eq!(w.programs, 0, "a graph vector separates");
    // Below them: the separating direction points from a to b.
    let below = piece_of(&square(2.0, 3.0, 10.0, -OFFSET), true);
    assert_eq!(empty(&a, &below, &mut work()), Some(qi([0, 0, -1])));
}

#[test]
fn near_and_contact_bound_the_gap_exactly() {
    let a = piece_of(&square(0.0, 0.0, 10.0, 0.0), false);
    let b = piece_of(&square(2.0, 3.0, 10.0, OFFSET), false);
    let opp = piece_of(&square(2.0, 3.0, 10.0, OFFSET), true);
    let at = rat(OFFSET);
    let t = near(&a, &b, &at, &mut work()).expect("the gap is b_K exactly");
    assert_eq!(t.ell, qi([0, 0, 1]));
    assert_eq!(t.cone.gens(), &[qi([0, 0, 1])]);
    assert_eq!(t.cone.b_k(), &at);
    // A bound a hair below the offset fails, exactly.
    let below = &at - Rational::from_signeds(1, 1i64 << 62);
    assert_eq!(near(&a, &b, &below, &mut work()), None);
    // Opposed pieces aren't Near; they are Contact.
    assert_eq!(near(&a, &opp, &at, &mut work()), None);
    assert_eq!(contact(&a, &b, &at, &mut work()), None);
    let c = contact(&a, &opp, &at, &mut work()).expect("opposed within b_K");
    assert_eq!(c.ell, qi([0, 0, 1]));
    assert_eq!(c.cone.gens(), &[qi([0, 0, 1])]);
    assert_eq!(contact(&a, &opp, &below, &mut work()), None);
}

#[test]
fn coplanar_overlapping_pieces_are_near_or_contact() {
    let a = piece_of(&square(0.0, 0.0, 10.0, 5.0), false);
    let b = piece_of(&square(4.0, 4.0, 10.0, 5.0), false);
    let opp = piece_of(&square(4.0, 4.0, 10.0, 5.0), true);
    let zero = Rational::from(0);
    match certify(&a, &b, &zero, &mut work()) {
        Some(Kind::Near(t)) => assert_eq!(t.ell, qi([0, 0, 1])),
        k => panic!("{k:?}"),
    }
    match certify(&a, &opp, &zero, &mut work()) {
        Some(Kind::Contact(t)) => assert_eq!(t.ell, qi([0, 0, 1])),
        k => panic!("{k:?}"),
    }
    // Sharing only an edge: not Empty (the gap is zero exactly), Near.
    let edge = piece_of(&square(10.0, 0.0, 10.0, 5.0), false);
    assert_eq!(empty(&a, &edge, &mut work()), None);
    assert!(matches!(
        certify(&a, &edge, &zero, &mut work()),
        Some(Kind::Near(_))
    ));
}

#[test]
fn coplanar_pieces_apart_take_the_program() {
    let a = piece_of(&square(0.0, 0.0, 1.0, 0.0), false);
    let b = piece_of(&square(1.0, 1.0, 1.0, 0.0), false);
    // Touching at a corner: not Empty.
    let mut w = work();
    assert_eq!(empty(&a, &b, &mut w), None);
    assert_eq!(w.programs, 1);
    // A hair apart along the diagonal: separated, but not by the normal.
    let tiny = f64::EPSILON;
    let b = piece_of(&square(1.0 + tiny, 1.0 + tiny, 1.0, 0.0), false);
    let mut w = work();
    let d = empty(&a, &b, &mut w).expect("disjoint hulls");
    assert_eq!(w.programs, 1);
    assert!(separates(&d, &a, &b));
    assert_eq!(d, qi([1, 1, 0]), "the least max–min direction");
}

#[test]
fn crossing_pieces_are_simple_at_any_angle() {
    let a = piece_of(&square(-5.0, -5.0, 10.0, 0.0), false);
    for slope in [1.0, 1e-3, f64::EPSILON] {
        // The plane `z = slope·(x − 1)`, through the middle of `a`.
        let b = piece_of(
            &quad(
                [-4.0, -4.0, -5.0 * slope],
                [6.0, -4.0, 5.0 * slope],
                [-4.0, 6.0, -5.0 * slope],
            ),
            false,
        );
        match certify(&a, &b, &rat(B_K), &mut work()) {
            Some(Kind::Simple { ell }) => {
                for g in a.graph() {
                    for h in b.graph() {
                        assert!(dot(&ell, &cross(g, h)) > 0u32);
                    }
                }
                assert_eq!(ell[0], 0u32);
                assert_eq!(ell[2], 0u32);
            }
            k => panic!("slope {slope}: {k:?}"),
        }
    }
    // Meeting at one corner only, not parallel: not Empty, Simple.
    let c = piece_of(
        &quad([5.0, 5.0, 0.0], [6.0, 5.0, 1.0], [5.0, 6.0, 0.0]),
        false,
    );
    assert_eq!(empty(&a, &c, &mut work()), None);
    assert!(matches!(
        certify(&a, &c, &rat(B_K), &mut work()),
        Some(Kind::Simple { .. })
    ));
}

#[test]
fn the_cone_admits_by_graph_vectors_and_by_the_vertical_gap() {
    // Parallel unit squares `h = 2⁻¹²` apart, `b_K = 5h/3`: along `ℓ` the
    // gap is `h·|ℓ|/ℓ_z`, within `b_K` while `|ℓ|²/ℓ_z² ≤ 25/9`.
    let h = 1.0 / 4096.0;
    let b_k = rat(h) * Rational::from_signeds(5, 3);
    let a = piece_of(&square(0.0, 0.0, 1.0, 0.0), false);
    let b = piece_of(&square(0.0, 0.0, 1.0, h), false);
    let t = near(&a, &b, &b_k, &mut work()).unwrap();
    assert_eq!(t.ell, qi([0, 0, 1]));
    let admits = |ell: [i64; 3]| t.cone.contains(&qi(ell), &mut work());
    assert!(t.cone.contains(&t.ell, &mut work()));
    // Any positive multiple; not against the normal or across it.
    assert!(admits([0, 0, 7]));
    assert!(!admits([0, 0, -1]));
    assert!(!admits([1, 0, 0]));
    // Tilted: the hull extents along `(1, 0, 1)` reach `(1 + h)/√2`, far
    // above `b_K`, but the gap between the planes is `h·√2`.
    assert!(admits([1, 0, 1]));
    assert!(admits([0, -1, 1]));
    // `(4, 0, 3)` is `b_K` exactly; a hair more tilt is above it.
    assert!(admits([4, 0, 3]));
    assert!(admits([0, 4, -3].map(|x| -x)));
    let more = [
        Rational::from(4) + Rational::from_signeds(1, 1i64 << 40),
        Rational::from(0),
        Rational::from(3),
    ];
    assert!(!t.cone.contains(&more, &mut work()));
    assert!(!admits([2, 0, 1]));
    // Tilted so far the projections along `ℓ` miss each other (`b`'s
    // moves by `h·10⁴ > 1` against `a`'s): no common point, no gap to
    // bound; a program decides it.
    let mut w = work();
    assert!(t.cone.contains(&qi([10_000, 0, 1]), &mut w));
    assert_eq!((w.gaps, w.programs), (1, 1));
    assert!(!admits([100, 0, 1]));
}

#[test]
fn coplanar_pieces_admit_every_direction_they_are_graphs_over() {
    let a = piece_of(&square(0.0, 0.0, 20.0, 3.0), false);
    let b = piece_of(&square(5.0, -5.0, 20.0, 3.0), false);
    let t = near(&a, &b, &Rational::from(0), &mut work()).unwrap();
    for ell in [[0, 0, 1], [1, 1, 1], [-1000, 999, 1], [0, -1, 1]] {
        let mut w = work();
        assert!(t.cone.contains(&qi(ell), &mut w), "{ell:?}");
        assert_eq!(w, LeafWork { gaps: 1, ..work() }, "exact, unsplit");
    }
    assert!(!t.cone.contains(&qi([1, 0, 0]), &mut work()));
    // Opposed: Contact, the same along any direction about the first's
    // normal.
    let opp = piece_of(&square(5.0, -5.0, 20.0, 3.0), true);
    let c = contact(&a, &opp, &Rational::from(0), &mut work()).unwrap();
    assert!(c.cone.contains(&qi([3, -2, 1]), &mut work()));
    assert!(!c.cone.contains(&qi([3, -2, -1]), &mut work()));
}

#[test]
fn curved_pieces_bound_the_gap_over_sub_piece_pairs() {
    // The dome `z = 4k·x(1 − x)`, `k = b_K/4`, on the plane along two
    // edges: along `ℓ = (1, 0, 64)` the hulls' extents reach `1/64`, but
    // over sub-piece pairs whose projections meet the bound tightens to
    // within `b_K`.
    let b_k = rat(B_K);
    let plane = piece_of(&square(0.0, 0.0, 1.0, 0.0), false);
    let d = piece_of(&dome(0.0, B_K / 4.0), false);
    let t = near(&plane, &d, &b_k, &mut work()).expect("within b_K along z");
    let ell = qi([1, 0, 64]);
    assert!(t.cone.gens().iter().all(|g| dot(g, &ell) > 0u32));
    let mut w = work();
    assert!(t.cone.contains(&ell, &mut w));
    assert!(w.gaps > 1 && w.gaps <= MAX_GAP_PAIRS, "{w:?}");
    // A deeper dome is more than `b_K` above the plane in the middle,
    // along every direction: no split helps.
    let deep = piece_of(&dome(0.0, 2.0 * B_K), false);
    let mut w = work();
    assert_eq!(near(&plane, &deep, &b_k, &mut w), None);
    assert!(w.gaps > 1);
}

/// `p` turned a quarter about `z`: a signed permutation of the axes.
fn turn(p: [f64; 3]) -> [f64; 3] {
    [-p[1], p[0], p[2]]
}

fn turn_q(p: &Q3) -> Q3 {
    [-&p[1], p[0].clone(), p[2].clone()]
}

#[test]
fn directions_move_with_quarter_turns() {
    // Tilted planes in general position: a crossing pair and a parallel
    // pair `2⁻¹³` apart along `z` (exact offsets).
    let dz = 1.0 / 8192.0;
    let pa = [[0.0, 0.0, 0.0], [4.0, 1.0, 2.0], [-1.0, 3.0, 1.0]];
    let pb = [[0.0, 0.0, 0.5], [3.0, -2.0, 1.0], [1.0, 4.0, -1.0]];
    let pc = pa.map(|p| [p[0], p[1], p[2] + dz]);
    let mk = |p: &[[f64; 3]; 3], f: fn([f64; 3]) -> [f64; 3]| {
        piece_of(&quad(f(p[0]), f(p[1]), f(p[2])), false)
    };
    let id: fn([f64; 3]) -> [f64; 3] = |p| p;
    let (a, b, c) = (mk(&pa, id), mk(&pb, id), mk(&pc, id));
    let (at, bt, ct) = (mk(&pa, turn), mk(&pb, turn), mk(&pc, turn));
    let s = simple(&a, &b, &mut work()).expect("crossing");
    assert_eq!(simple(&at, &bt, &mut work()), Some(turn_q(&s)));
    let k = rat(B_K);
    let n = near(&a, &c, &k, &mut work()).expect("parallel, close");
    let nt = near(&at, &ct, &k, &mut work()).expect("parallel, close");
    assert_eq!(nt.ell, turn_q(&n.ell));
    // The shared normal itself, `(−5, −6, 13)` up to scale.
    assert_eq!(dot(&n.ell, &cross(&n.ell, &qi([-5, -6, 13]))), 0u32);
    assert!(cross(&n.ell, &qi([-5, -6, 13])).iter().all(|x| *x == 0u32));
}

/// The leaves of every face pair of `pair` (whole tiles, tested at once).
fn pair_kinds(pair: &sheets::Pair, b_k: f64) -> Vec<(usize, usize, Kind)> {
    let mut out = Vec::new();
    for fa in 0..pair.a.faces.len() {
        for fb in 0..pair.b.faces.len() {
            let (ta, tb) = (face_tile(&pair.a, fa), face_tile(&pair.b, fb));
            let face = |patch| FlatFace {
                patch,
                flipped: false,
            };
            let (ls, w) = leaves(
                face(&ta),
                whole(Operand::A, fa as u32),
                face(&tb),
                whole(Operand::B, fb as u32),
                &rat(b_k),
                64,
                |_| BoxUse::Test,
            )
            .expect("flat pieces certify at once");
            assert_eq!((ls.len(), w.pairs), (1, 1));
            let [l] = <[Leaf; 1]>::try_from(ls).unwrap();
            out.push((fa, fb, l.kind));
        }
    }
    out
}

#[test]
fn offset_sheet_fixtures_give_no_tangent_leaves() {
    // Flat faces δ apart are Empty or Simple: no cluster forms.
    let small = Wavy {
        cells: 3,
        ..Wavy::DEFAULT
    };
    for pair in [
        sheets::wavy(&small),
        sheets::box_edge_fold(OFFSET),
        sheets::l_corner(OFFSET),
        sheets::l_corner_quad(OFFSET),
    ] {
        for (fa, fb, k) in pair_kinds(&pair, B_K) {
            assert!(
                matches!(k, Kind::Empty { .. } | Kind::Simple { .. }),
                "faces {fa}, {fb}: {k:?}"
            );
        }
    }
}

#[test]
fn flush_sheet_fixtures_give_near_leaves() {
    // At offset zero the sheet lies on the faces, co-oriented: those
    // pairs are Near with a zero gap along the shared normal.
    let pair = sheets::l_corner_quad(0.0);
    let kinds = pair_kinds(&pair, B_K);
    match &kinds[..] {
        [(0, 0, Kind::Near(t))] => assert_eq!(t.ell, qi([0, 0, 1])),
        k => panic!("{k:?}"),
    }
    let pair = sheets::box_edge_fold(0.0);
    let near: Vec<(usize, usize)> = pair_kinds(&pair, B_K)
        .into_iter()
        .filter(|(_, _, k)| matches!(k, Kind::Near(_)))
        .map(|(fa, fb, _)| (fa, fb))
        .collect();
    // The box's top (face 1) under the top quad (0), its front (face 2,
    // `y = 0`) behind the front quad (1).
    assert_eq!(near, [(1, 0), (2, 1)]);
}

/// A dome over the unit square in `x`, `y`: `z = h + 4k·x(1 − x)`, its
/// net quadratic in `u = x`.
fn dome(h: f64, k: f64) -> HomPatch<Rational> {
    let z = [h, h + 2.0 * k, h];
    let net = (0..3).flat_map(|i| {
        (0..2).map(move |j| {
            [
                rat(i as f64 / 2.0),
                Rational::from(j),
                rat(z[i]),
                Rational::from(1),
            ]
        })
    });
    HomPatch::new(2, 1, net).unwrap()
}

#[test]
fn leaves_split_until_every_pair_certifies() {
    // The dome reaches `k = 2b_K`, the plane at `k − b_K/2` crosses it
    // at `x = ¼` and `x = ¾`: over the whole square the gap isn't
    // certified; boxes in the middle are Near, at the crossings Simple,
    // at the edges (below the plane) Empty.
    let plane = square(0.0, 0.0, 1.0, 1.5 * B_K);
    let d = dome(0.0, 2.0 * B_K);
    let face = |patch| FlatFace {
        patch,
        flipped: false,
    };
    let run = || {
        leaves(
            face(&plane),
            whole(Operand::A, 0),
            face(&d),
            whole(Operand::B, 0),
            &rat(B_K),
            1000,
            |_| BoxUse::Test,
        )
        .unwrap()
    };
    let (ls, w) = run();
    assert!(ls.len() > 1);
    assert!(w.pairs as usize > ls.len());
    assert!(ls.iter().all(|l| matches!(
        l.kind,
        Kind::Near(_) | Kind::Simple { .. } | Kind::Empty { .. }
    )));
    assert!(ls.iter().any(|l| matches!(l.kind, Kind::Empty { .. })));
    assert!(ls.iter().any(|l| matches!(l.kind, Kind::Near(_))));
    // The leaves partition the pair: their area products sum to one.
    let area = |b: &BoxKey| {
        let w = |dir| {
            let e = b.extent(dir);
            e.hi().rational() - e.lo().rational()
        };
        w(Dir::U) * w(Dir::V)
    };
    let total: Rational = ls.iter().map(|l| area(&l.a) * area(&l.b)).sum();
    assert_eq!(total, Rational::from(1));
    // Ordered by boxes, the same on every run.
    assert!(ls.windows(2).all(|p| (p[0].a, p[0].b) < (p[1].a, p[1].b)));
    assert_eq!(run().0, ls);
    // Past the cap.
    let r = leaves(
        face(&plane),
        whole(Operand::A, 0),
        face(&d),
        whole(Operand::B, 0),
        &rat(B_K),
        3,
        |_| BoxUse::Test,
    );
    assert!(matches!(r, Err(LeafError::Pairs(w)) if w.pairs == 3));
}

#[test]
fn the_caller_drops_and_splits_boxes() {
    let a = square(0.0, 0.0, 8.0, 0.0);
    let b = square(0.0, 0.0, 8.0, 0.0);
    let face = |patch| FlatFace {
        patch,
        flipped: false,
    };
    // Split every box to depth 1 in both directions, drop the second
    // operand's boxes with `u ≥ ½`.
    let (ls, _) = leaves(
        face(&a),
        whole(Operand::A, 0),
        face(&b),
        whole(Operand::B, 0),
        &rat(B_K),
        1000,
        |bx| {
            let (u, v) = (bx.extent(Dir::U), bx.extent(Dir::V));
            if bx.operand() == Operand::B && u.lo() >= Dyadic::new(1, 1).unwrap() {
                BoxUse::Drop
            } else if u.depth() < 1 || v.depth() < 1 {
                BoxUse::Split
            } else {
                BoxUse::Test
            }
        },
    )
    .unwrap();
    assert_eq!(ls.len(), 4 * 2);
    assert!(ls.iter().all(|l| l.a.depth() == 1 && l.b.depth() == 1));
    // Quarters of one plane all overlap or touch: Near, gap zero.
    assert!(ls.iter().all(|l| matches!(l.kind, Kind::Near(_))));
}

#[test]
fn a_degenerate_pair_stops_at_the_finest_box() {
    // A piece collapsed to a point, against itself: no kind certifies at
    // any depth.
    let r = |x: i64| Rational::from(x);
    let point = HomPatch::new(1, 1, (0..4).map(|_| [r(1), r(2), r(3), r(1)])).unwrap();
    let face = FlatFace {
        patch: &point,
        flipped: false,
    };
    let res = leaves(
        face,
        whole(Operand::A, 0),
        face,
        whole(Operand::B, 0),
        &rat(B_K),
        100_000,
        |_| BoxUse::Test,
    );
    match res {
        Err(LeafError::Depth(ab)) => {
            let [a, b] = *ab;
            assert_eq!((a.depth(), b.depth()), (MAX_BOX_DEPTH, MAX_BOX_DEPTH));
        }
        r => panic!("{r:?}"),
    }
}

#[test]
fn halving_takes_the_coarser_extent_u_first() {
    let b = whole(Operand::A, 0);
    let [lo, hi] = halve(&b).unwrap();
    let half = Dyadic::new(1, 1).unwrap();
    assert_eq!(lo.extent(Dir::U).hi(), half);
    assert_eq!(hi.extent(Dir::U).lo(), half);
    assert_eq!(lo.extent(Dir::V), Extent::WHOLE);
    let [l2, _] = halve(&lo).unwrap();
    assert_eq!(l2.extent(Dir::U), lo.extent(Dir::U));
    assert_eq!(l2.extent(Dir::V).hi(), half);
}
