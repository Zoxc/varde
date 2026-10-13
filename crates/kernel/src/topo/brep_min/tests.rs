//! The store on a half cylinder (a closed solid: the cylinder's 180°
//! face, the plane through its axis and two half-disc caps), with exact
//! and perturbed p-curves, and on a half disc whose arcs have weights
//! other than 1 at their ends and knots: the ribbon joins hold, the
//! curves end at their vertices exactly at every level, each fault is
//! found with its entity, and every seam bound holds against exact
//! rational evaluation.

use malachite::Rational as Q;
use malachite::base::num::arithmetic::traits::Abs;
use varde_decide::bezier::{HomCurve, Poly, Scalar};
use varde_decide::num::bigf::{Big, BigF, Width};
use varde_decide::num::bounded::Bounded;
use varde_decide::num::dd::Dd;
use varde_testkit::rng::Rng;

use super::*;
use crate::geom::certify::SeamError;
use crate::geom::curve::{CurveError, Span2, Span3};
use crate::geom::form::{CylinderKey, PlaneKey};
use crate::geom::lattice::{CELL, Lattice};

const R: f64 = 50.0;
const H: f64 = 10.0;
const ROOT_HALF: f64 = std::f64::consts::FRAC_1_SQRT_2;

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

/// The greater, by compare and select.
fn most(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|x| x * k)
}

/// The control point `p` with weight `w`, stored affine.
fn pt(p: [f64; 3], w: f64) -> [f64; 4] {
    [p[0], p[1], p[2], w]
}

/// A line span from `a` to `b`.
fn line(a: [f64; 3], b: [f64; 3]) -> Span3 {
    let m = scale(add(a, b), 0.5);
    [pt(a, 1.0), pt(m, 1.0), pt(b, 1.0)]
}

/// The UV line span from `a` to `b`.
fn uv_line(a: [f64; 2], b: [f64; 2]) -> Span2 {
    let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]
}

/// The p-curve on a plane of `c`'s spans: each control point mapped by
/// the plane's frame, `(x − o)·axis / CELL + 1/2`, its weight kept (the
/// same point maps to the same bits on every curve through it).
fn on_plane(lat: &Lattice, c: &Curve3D) -> PCurve {
    let (o, [u, v, _]) = (lat.origin(), lat.axes());
    let spans = c
        .spans()
        .iter()
        .map(|s| {
            s.map(|h| {
                let d = [h[0] - o[0], h[1] - o[1], h[2] - o[2]];
                let along = |a: [f64; 3]| (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5;
                [along(u), along(v), h[3]]
            })
        })
        .collect();
    PCurve::new(spans, c.knots().to_vec()).unwrap()
}

/// The half cylinder and the ids a test needs.
struct Fixture {
    brep: Brep,
    verts: [VertId; 4],
    /// arc at 0, arc at `H`, ruling at 0°, ruling at 180°, diameter at 0,
    /// diameter at `H`.
    edges: [EdgeId; 6],
    cylinder: FaceId,
}

/// The cylinder's p-curves, over the edges' `t`: arc at 0, arc at `H`,
/// ruling at 0° (`u = 0`), ruling at 180° (`u = 2`).
fn cylinder_pcurves() -> [PCurve; 4] {
    let (lo, hi) = (0.5, 0.5 + H / CELL);
    let arc = |v: f64| {
        PCurve::new(
            vec![uv_line([0.0, v], [1.0, v]), uv_line([1.0, v], [2.0, v])],
            vec![0.0, 1.0, 2.0],
        )
        .unwrap()
    };
    let ruling = |u: f64| PCurve::new(vec![uv_line([u, lo], [u, hi])], vec![0.0, 1.0]).unwrap();
    [arc(lo), arc(hi), ruling(0.0), ruling(2.0)]
}

/// The half cylinder of radius `R` and height `H` on the axis `z`, its
/// cylinder face's p-curves `cyl` (as [`cylinder_pcurves`]).
fn half_cylinder(cyl: [PCurve; 4]) -> Fixture {
    let key = CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], R).unwrap();
    let lat = Lattice::new(key.into()).unwrap();
    let [e1, e2, a] = lat.axes();
    let rect = lat.half(0).unwrap();
    let mut b = Brep::new();
    let s_cyl = b.add_surface(Surface::new(lat, rect).unwrap()).unwrap();

    let at = |dir: [f64; 3], z: f64| add(scale(dir, R), scale(a, z));
    let m1 = scale(e1, -1.0);
    let pts = [at(e1, 0.0), at(m1, 0.0), at(e1, H), at(m1, H)];
    let verts = pts.map(|p| b.add_vert(p).unwrap());

    // A half circle at height z: two quarters, 0° to 90° to 180°.
    let arc = |z: f64| {
        let quarter = |from: [f64; 3], to: [f64; 3]| -> Span3 {
            [
                pt(at(from, z), 1.0),
                pt(add(at(add(from, to), 0.0), scale(a, z)), ROOT_HALF),
                pt(at(to, z), 1.0),
            ]
        };
        Curve3D::new(vec![quarter(e1, e2), quarter(e2, m1)], vec![0.0, 1.0, 2.0]).unwrap()
    };
    let curves = [
        arc(0.0),
        arc(H),
        Curve3D::new(vec![line(pts[0], pts[2])], vec![0.0, 1.0]).unwrap(),
        Curve3D::new(vec![line(pts[1], pts[3])], vec![0.0, 1.0]).unwrap(),
        Curve3D::new(vec![line(pts[0], pts[1])], vec![0.0, 1.0]).unwrap(),
        Curve3D::new(vec![line(pts[2], pts[3])], vec![0.0, 1.0]).unwrap(),
    ];
    let ends = [(0, 1), (2, 3), (0, 2), (1, 3), (0, 1), (2, 3)];
    let mut edges = [EdgeId::NONE; 6];
    for (k, c) in curves.iter().enumerate() {
        let id = b.add_curve(c.clone()).unwrap();
        let t1 = c.knots()[c.knots().len() - 1];
        let (s, e) = ends[k];
        edges[k] = b.add_edge(id, [0.0, t1], verts[s], verts[e]).unwrap();
    }
    let [arc0, arc1, rul0, rul1, dia0, dia1] = edges;

    // The cylinder: its normal points out, the loop runs u up at z = 0.
    let cylinder = b.add_face(s_cyl, false).unwrap();
    let [p_arc0, p_arc1, p_rul0, p_rul1] = cyl;
    b.add_loop(
        cylinder,
        true,
        vec![
            (arc0, false, p_arc0),
            (rul1, false, p_rul1),
            (arc1, true, p_arc1),
            (rul0, true, p_rul0),
        ],
    )
    .unwrap();

    // The plane through the axis, and the caps.
    let plane = |n: [f64; 3], p: [f64; 3]| {
        let lat = Lattice::new(PlaneKey::new(n, p).unwrap().into()).unwrap();
        let rect = lat.half(0).unwrap();
        Surface::new(lat, rect).unwrap()
    };
    let faces = [
        (
            plane(e2, [0.0; 3]),
            vec![(rul0, false), (dia1, false), (rul1, true), (dia0, true)],
        ),
        (plane(a, [0.0; 3]), vec![(arc0, true), (dia0, false)]),
        (plane(a, scale(a, H)), vec![(arc1, false), (dia1, true)]),
    ];
    for (s, uses) in faces {
        let lat = s.lattice().clone();
        let id = b.add_surface(s).unwrap();
        let f = b.add_face(id, false).unwrap();
        let uses = uses
            .into_iter()
            .map(|(e, r)| (e, r, on_plane(&lat, b.curve(e))))
            .collect();
        b.add_loop(f, true, uses).unwrap();
    }
    Fixture {
        brep: b,
        verts,
        edges,
        cylinder,
    }
}

/// The exact `|S(p(t)) − C(t)|²` of coedge `c` at `t`, a knot range
/// point given as a rational.
fn deviation_sq(b: &Brep, c: CoedgeId, t: &Q) -> Q {
    let e = b.edge(c);
    let (curve, p) = (b.curve(e), b.pcurve(c));
    let surface = b.surface(b.face(c));
    let span_at = |knots: &[f64]| {
        let k = knots[1..knots.len() - 1]
            .iter()
            .take_while(|&&k| q(k) <= *t)
            .count();
        let (a, z) = (q(knots[k]), q(knots[k + 1]));
        (k, (t - &a) / (z - a))
    };
    let (kc, sc) = span_at(curve.knots());
    let (kp, sp) = span_at(p.knots());
    let zero = Q::from(0u32);
    let cs = HomCurve::<Q, 3>::from_affine(
        curve.spans()[kc].iter().map(|h| ([h[0], h[1], h[2]], h[3])),
        &zero,
    )
    .unwrap();
    let ps =
        HomCurve::<Q, 2>::from_affine(p.spans()[kp].iter().map(|h| ([h[0], h[1]], h[2])), &zero)
            .unwrap();
    let x = cs.eval(&sc);
    let [u, v] = ps.eval(&sp);
    // The span's tile, by its middle control point.
    let mid = p.spans()[kp][1];
    let rect = surface.rect();
    let i = (mid[0].floor() as i64).clamp(rect.u.start, rect.u.end - 1);
    let j = (mid[1].floor() as i64).clamp(rect.v.start, rect.v.end - 1);
    let tile = surface.tile(i, j).unwrap();
    let y = tile
        .patch_at(&Q::from(0u32))
        .eval(&(u - Q::from(i)), &(v - Q::from(j)));
    (0..3)
        .map(|k| {
            let d = &x[k] - &y[k];
            &d * &d
        })
        .fold(Q::from(0u32), |s, d| s + d)
}

/// Checks every coedge's seam bound against exact deviations at `n`
/// points per span and returns the greatest deviation's square per
/// coedge.
fn assert_bounds_hold(b: &Brep, n: u32) -> Vec<Q> {
    (0..b.coedge_count())
        .map(|c| {
            let c = CoedgeId(c as u32);
            let bound = q(b.seam_bound(c));
            let bound_sq = &bound * &bound;
            let knots = b.pcurve(c).knots();
            let mut most = Q::from(0u32);
            for w in knots.windows(2) {
                for k in 0..=n {
                    let s = Q::from(k) / Q::from(n);
                    let t = q(w[0]) + s * (q(w[1]) - q(w[0]));
                    let d = deviation_sq(b, c, &t);
                    assert!(d <= bound_sq, "coedge {c:?}: deviation over its seam bound");
                    if d > most {
                        most = d;
                    }
                }
            }
            most
        })
        .collect()
}

#[test]
fn exact_half_cylinder_has_well_defined_ribbons() {
    let f = half_cylinder(cylinder_pcurves());
    let b = &f.brep;
    assert_eq!(b.check_ribbons(), Ok(()));
    assert_eq!(
        (
            b.vert_count(),
            b.edge_count(),
            b.coedge_count(),
            b.face_count()
        ),
        (4, 6, 12, 4)
    );
    for c in 0..b.coedge_count() {
        let c = CoedgeId(c as u32);
        let p = b.pair(c);
        assert_ne!(p, CoedgeId::NONE);
        assert_eq!((b.pair(p), b.edge(p)), (c, b.edge(c)));
        assert_ne!(b.reversed(p), b.reversed(c));
        assert_eq!(b.prev(b.next(c)), c);
        // Exact pairs: the bound is the rounding of the stored values.
        assert!(b.seam_bound(c) < 1e-8, "{c:?}: {}", b.seam_bound(c));
        assert!(b.seam_bound(c) <= b.edge_tol(b.edge(c)));
    }
    for e in f.edges {
        let [s, t] = b.ends(e);
        let tol = b.edge_tol(e);
        assert!(b.vert_tol(s) >= tol && b.vert_tol(t) >= tol);
        let c = b.edge_coedge(e);
        assert_eq!(tol, most(b.seam_bound(c), b.seam_bound(b.pair(c))));
    }
    let loops: Vec<_> = b.loops(f.cylinder).collect();
    assert_eq!(loops.len(), 1);
    assert!(b.is_outer(loops[0]));
    assert_eq!(b.loop_coedges(loops[0]).count(), 4);
    assert_bounds_hold(b, 8);
}

#[test]
fn perturbed_pcurves_keep_their_joins_and_widen_their_ribbons() {
    let exact = half_cylinder(cylinder_pcurves());
    let [a0, a1, r0, r1] = cylinder_pcurves();
    // A turn of 1e-4 quarter turns, about 8 µm on the arcs; 1e-9 axial
    // cells, about 4 µm on the rulings.
    let pc = [
        a0.perturbed(&[[1e-4, 0.0], [0.0, -1e-9]]).unwrap(),
        a1.perturbed(&[[0.0, 1e-9], [-1e-4, 0.0]]).unwrap(),
        r0.perturbed(&[[1e-4, 0.0]]).unwrap(),
        r1.perturbed(&[[-1e-4, 1e-9]]).unwrap(),
    ];
    let f = half_cylinder(pc);
    let b = &f.brep;
    assert_eq!(b.check_ribbons(), Ok(()));
    let seen_sq = assert_bounds_hold(b, 16);
    for c in b.loop_coedges(b.loops(f.cylinder).next().unwrap()) {
        let bound = b.seam_bound(c);
        assert!(
            bound > 1e-3 && bound > 100.0 * exact.brep.seam_bound(c),
            "{c:?}: {bound}"
        );
        // Not far above the deviation sampled.
        let seen = seen_sq[c.0 as usize].clone();
        assert!(
            q(bound / 4.0) * q(bound / 4.0) <= seen,
            "{c:?}: {bound} loose"
        );
    }
    for e in f.edges {
        let c = b.edge_coedge(e);
        let tol = most(b.seam_bound(c), b.seam_bound(b.pair(c)));
        assert_eq!(b.edge_tol(e), tol);
    }
}

#[test]
fn random_perturbations_stay_within_their_seam_bounds() {
    let mut rng = Rng::new(15);
    for _ in 0..6 {
        let [a0, a1, r0, r1] = cylinder_pcurves();
        let mut off = |k: f64| [rng.range(-k, k) * 1e-3, rng.range(-k, k) * 1e-9];
        let pc = [
            a0.perturbed(&[off(1.0), off(1.0)]).unwrap(),
            a1.perturbed(&[off(1.0), off(1.0)]).unwrap(),
            // The rulings lie on the domain's sides: moved inwards.
            r0.perturbed(&[off(1.0)].map(|[u, v]| [u.abs(), v]))
                .unwrap(),
            r1.perturbed(&[off(1.0)].map(|[u, v]| [-u.abs(), v]))
                .unwrap(),
        ];
        let f = half_cylinder(pc);
        assert_eq!(f.brep.check_ribbons(), Ok(()));
        assert_bounds_hold(&f.brep, 12);
    }
}

#[test]
fn seam_bounds_cut_at_the_knots_of_both_curves() {
    // A ruling of the cylinder: its curve one span over [0, 2], its
    // p-curve two (cut at t = 1); and the other way round.
    let key = CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], R).unwrap();
    let lat = Lattice::new(key.into()).unwrap();
    let [e1, _, a] = lat.axes();
    let s = Surface::new(lat.clone(), lat.half(0).unwrap()).unwrap();
    let (p0, p1) = (scale(e1, R), add(scale(e1, R), scale(a, H)));
    let (lo, hi) = (0.5, 0.5 + H / CELL);
    let mid = 0.5 + 0.5 * H / CELL;
    let one = Curve3D::new(vec![line(p0, p1)], vec![0.0, 2.0]).unwrap();
    let pm = scale(add(p0, p1), 0.5);
    let two = Curve3D::new(vec![line(p0, pm), line(pm, p1)], vec![0.0, 1.0, 2.0]).unwrap();
    let p_one = PCurve::new(vec![uv_line([0.0, lo], [0.0, hi])], vec![0.0, 2.0]).unwrap();
    let p_two = PCurve::new(
        vec![
            uv_line([0.0, lo], [0.0, mid]),
            uv_line([0.0, mid], [0.0, hi]),
        ],
        vec![0.0, 1.0, 2.0],
    )
    .unwrap();
    for (c, p) in [(&one, &p_two), (&two, &p_one), (&two, &p_two)] {
        // The rounding of the pieces' restrictions, on the axial cell's
        // scale (`CELL`, 4·10⁶ mm).
        let bound = seam_bound(&s, c, p).unwrap();
        assert!(bound < 1e-7, "{bound}");
    }
    // Moved by a millimetre along the axis on the second half.
    let shift = 1.0 / CELL;
    let p_off = PCurve::new(
        vec![
            uv_line([0.0, lo], [0.0, mid]),
            [
                [0.0, mid, 1.0],
                [0.0, 0.5 * (mid + hi) + shift, 1.0],
                [0.0, hi, 1.0],
            ],
        ],
        vec![0.0, 1.0, 2.0],
    )
    .unwrap();
    let bound = seam_bound(&s, &one, &p_off).unwrap();
    // The middle control point moved by 1 mm: the curve by ½ mm.
    assert!((0.5..=1.0 + 1e-6).contains(&bound), "{bound}");
    // A p-curve outside the curve's range, or off the domain.
    let long = PCurve::new(vec![uv_line([0.0, lo], [0.0, hi])], vec![0.0, 3.0]).unwrap();
    assert_eq!(seam_bound(&s, &one, &long), Err(SeamError::Range));
    let off = PCurve::new(vec![uv_line([-0.5, lo], [0.0, hi])], vec![0.0, 2.0]).unwrap();
    assert_eq!(seam_bound(&s, &one, &off), Err(SeamError::Tile { span: 0 }));
    let across = PCurve::new(vec![uv_line([0.5, lo], [1.5, hi])], vec![0.0, 2.0]).unwrap();
    assert_eq!(
        seam_bound(&s, &one, &across),
        Err(SeamError::Tile { span: 0 })
    );
}

#[test]
fn check_ribbons_finds_the_first_broken_join() {
    // (a): a vertex at −0 where its edges' curves end at +0.
    let mut f = half_cylinder(cylinder_pcurves());
    let v = f.verts[0];
    let pos = f.brep.verts.pos[v.0 as usize];
    let zero = pos.iter().position(|&x| x == 0.0).unwrap();
    f.brep.verts.pos[v.0 as usize][zero] = -pos[zero];
    let first = (0..f.brep.edge_count())
        .map(|e| EdgeId(e as u32))
        .find(|&e| f.brep.ends(e).contains(&v))
        .unwrap();
    assert_eq!(f.brep.check_ribbons(), Err(RibbonFault::EdgeEnds(first)));

    // (a): an edge whose vertices are not canonical.
    let mut f = half_cylinder(cylinder_pcurves());
    let e = f.edges[2].0 as usize;
    let (s, t) = (f.brep.edges.v_start[e], f.brep.edges.v_end[e]);
    f.brep.edges.v_start[e] = t;
    f.brep.edges.v_end[e] = s;
    assert_eq!(
        f.brep.check_ribbons(),
        Err(RibbonFault::EdgeEnds(f.edges[2]))
    );

    // (b): the arc ends at another vertex than the ruling starts at.
    let mut f = half_cylinder(cylinder_pcurves());
    let c = CoedgeId(0);
    assert_eq!(f.brep.next(c), CoedgeId(1));
    f.brep.coedges.reversed[1] = !f.brep.coedges.reversed[1];
    assert_eq!(f.brep.check_ribbons(), Err(RibbonFault::Corner(c)));

    // (b): the arc's p-curve ends an ulp off where the ruling starts.
    let mut f = half_cylinder(cylinder_pcurves());
    let c = CoedgeId(0);
    let mut spans = f.brep.pcurve(c).spans().to_vec();
    spans[1][2][1] = spans[1][2][1].next_up();
    let knots = f.brep.pcurve(c).knots().to_vec();
    f.brep.coedges.pcurve[0] = PCurve::new(spans, knots).unwrap();
    assert_eq!(f.brep.check_ribbons(), Err(RibbonFault::Join(c)));

    // (e): the ruling's p-curve over another range of t.
    let mut f = half_cylinder(cylinder_pcurves());
    let c = CoedgeId(1);
    let spans = f.brep.pcurve(c).spans().to_vec();
    f.brep.coedges.pcurve[1] = PCurve::new(spans, vec![0.0, 0.5]).unwrap();
    assert_eq!(f.brep.check_ribbons(), Err(RibbonFault::SameT(c)));
}

#[test]
fn add_loop_refuses_bad_coedges_and_adds_nothing() {
    let mut f = half_cylinder(cylinder_pcurves());
    let b = &mut f.brep;
    let before = (b.coedge_count(), b.edge_tol(f.edges[0]));
    let [a0, ..] = cylinder_pcurves();
    let face = f.cylinder;
    // Every edge has its two coedges.
    assert_eq!(
        b.add_loop(face, false, vec![(f.edges[0], true, a0.clone())]),
        Err(StoreError::Coedge {
            index: 0,
            why: CoedgeError::Third
        })
    );
    // A new edge used twice the same way.
    let curve = b.curve(f.edges[0]).clone();
    let id = b.add_curve(curve).unwrap();
    let e = b.add_edge(id, [0.0, 2.0], f.verts[0], f.verts[1]).unwrap();
    assert_eq!(
        b.add_loop(
            face,
            false,
            vec![(e, false, a0.clone()), (e, false, a0.clone())]
        ),
        Err(StoreError::Coedge {
            index: 1,
            why: CoedgeError::SameWay
        })
    );
    let short = PCurve::new(a0.spans().to_vec(), vec![0.0, 1.0, 1.5]).unwrap();
    assert_eq!(
        b.add_loop(face, false, vec![(e, false, short)]),
        Err(StoreError::Coedge {
            index: 0,
            why: CoedgeError::SameT
        })
    );
    let off = a0.perturbed(&[[0.0, 0.0], [0.0, -0.6]]).unwrap();
    assert_eq!(
        b.add_loop(face, false, vec![(e, false, off)]),
        Err(StoreError::Coedge {
            index: 0,
            why: CoedgeError::Seam(SeamError::Tile { span: 1 })
        })
    );
    assert_eq!(b.add_loop(face, false, vec![]), Err(StoreError::EmptyLoop));
    assert_eq!((b.coedge_count(), b.edge_tol(f.edges[0])), before);
    assert_eq!(b.check_ribbons(), Ok(()));
    // Ends and ranges of edges.
    assert_eq!(
        b.add_edge(id, [0.0, 2.0], f.verts[1], f.verts[0]),
        Err(StoreError::NotCanonical)
    );
    assert_eq!(
        b.add_edge(id, [0.0, 1.5], f.verts[0], f.verts[1]),
        Err(StoreError::TRange)
    );
    assert_eq!(b.add_vert([f64::NAN, 0.0, 0.0]), Err(StoreError::NotFinite));
}

#[test]
fn chains_join_bit_for_bit() {
    let a = [[0.0, 0.0, 1.0], [0.5, 0.0, 1.0], [1.0, 0.0, 1.0]];
    // The same point at the knot with another weight, 3.
    let b = [[1.0, 0.0, 3.0], [1.5, 0.5, 2.0], [2.0, 2.0, 1.0]];
    assert!(PCurve::new(vec![a, b], vec![0.0, 1.0, 2.0]).is_ok());
    let mut c = b;
    c[0][1] = -0.0;
    assert_eq!(
        PCurve::new(vec![a, c], vec![0.0, 1.0, 2.0]),
        Err(CurveError::Join { span: 1 })
    );
    assert_eq!(
        PCurve::new(vec![a, b], vec![0.0, 1.0, 1.0]),
        Err(CurveError::Knots { knot: 2 })
    );
    let mut w = a;
    w[1][2] = 65.0;
    assert_eq!(
        PCurve::new(vec![w], vec![0.0, 1.0]),
        Err(CurveError::Weight { span: 0 })
    );
    assert_eq!(PCurve::new(vec![a], vec![0.0]), Err(CurveError::Shape));
    let p = PCurve::new(vec![a, b], vec![0.0, 1.0, 2.0]).unwrap();
    assert_eq!(p.knot_point(1), Some([1.0, 0.0]));
    assert_eq!(p.knot_point(2), Some([2.0, 2.0]));
    assert_eq!(p.knot_point(3), None);
    assert_eq!(p.perturbed(&[[0.0, 0.0]]), Err(CurveError::Shape));
}

#[test]
fn evaluation_follows_the_chains_and_the_tiles() {
    let f = half_cylinder(cylinder_pcurves());
    let b = &f.brep;
    for c in 0..b.coedge_count() {
        let c = CoedgeId(c as u32);
        let (curve, p) = (b.curve(b.edge(c)), b.pcurve(c));
        let surface = b.surface(b.face(c));
        let knots = curve.knots();
        // At a knot: the knot's point, to the bit.
        for (k, &t) in knots.iter().enumerate() {
            let x = curve.eval(t).unwrap().map(|x| x.v);
            assert_eq!(
                x.map(f64::to_bits),
                curve.knot_point(k).unwrap().map(f64::to_bits)
            );
        }
        let [t0, t1] = b.t_range(b.edge(c));
        assert_eq!(curve.eval(t1 + 1.0), None);
        assert_eq!(p.eval(t0 - 1.0), None);
        for k in 0..=10 {
            let t = t0 + (t1 - t0) * f64::from(k) / 10.0;
            let uv = p.eval(t).unwrap();
            let y = surface.eval(uv.map(|x| x.v)).unwrap();
            let x = curve.eval(t).unwrap();
            // The UV point's own error moves it at most this far.
            let slack = 1e-8;
            for m in 0..3 {
                let gap = (x[m].v - y[m].v).abs();
                assert!(
                    gap <= b.seam_bound(c) + x[m].err() + y[m].err() + slack,
                    "{c:?} at {t}"
                );
            }
        }
    }
    let s = b.surface(f.cylinder);
    assert_eq!(s.eval([2.5, 0.5]), None);
    assert!(s.eval([2.0, 1.0]).is_some());
}

/// A half disc of radius `R` in the plane `z = 0`, both sides, its arc
/// chains with weights other than 1 at their ends and knots: the first
/// quarter as the two halves of its split at ½ (the split point's weight
/// `½ + √2/4 ≈ 0.854`; the second half scaled by 3, so the knot's
/// weights differ), the second quarter reparametrised to end weights 3
/// and ⅓ (`W(0) ≠ W(1)`); and the diameter as two lines through the
/// centre.
fn weighted_half_disc() -> Brep {
    let c = 0.5 + std::f64::consts::SQRT_2 / 4.0;
    let tan = R * (std::f64::consts::SQRT_2 - 1.0);
    let s = R * ROOT_HALF;
    let (a, d, e, o) = ([R, 0.0, 0.0], [0.0, R, 0.0], [-R, 0.0, 0.0], [0.0; 3]);
    let m = [s, s, 0.0];
    let first = Curve3D::new(
        vec![
            [pt(a, 1.0), pt([R, tan, 0.0], c), pt(m, c)],
            [pt(m, 3.0 * c), pt([tan, R, 0.0], 3.0 * c), pt(d, 3.0)],
        ],
        vec![0.0, 0.5, 1.0],
    )
    .unwrap();
    let second = Curve3D::new(
        vec![[pt(d, 3.0), pt([-R, R, 0.0], ROOT_HALF), pt(e, 1.0 / 3.0)]],
        vec![1.0, 2.0],
    )
    .unwrap();
    let mut b = Brep::new();
    let lat = Lattice::new(PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap().into()).unwrap();
    let surf = b
        .add_surface(Surface::new(lat.clone(), lat.half(0).unwrap()).unwrap())
        .unwrap();
    let v = [a, d, e, o].map(|p| b.add_vert(p).unwrap());
    let curves = [
        (first, [0.0, 1.0], (0, 1)),
        (second, [1.0, 2.0], (1, 2)),
        (
            Curve3D::new(vec![line(e, o)], vec![0.0, 1.0]).unwrap(),
            [0.0, 1.0],
            (2, 3),
        ),
        (
            Curve3D::new(vec![line(a, o)], vec![0.0, 1.0]).unwrap(),
            [0.0, 1.0],
            (0, 3),
        ),
    ];
    let mut edges = Vec::new();
    for (curve, t, (s, e)) in curves {
        let id = b.add_curve(curve).unwrap();
        edges.push(b.add_edge(id, t, v[s], v[e]).unwrap());
    }
    // a → d → e → o → a, on top; the other way below.
    let ways = [false, false, false, true];
    for flipped in [false, true] {
        let f = b.add_face(surf, flipped).unwrap();
        let mut uses: Vec<_> = edges
            .iter()
            .zip(ways)
            .map(|(&e, r)| (e, r != flipped, on_plane(&lat, b.curve(e))))
            .collect();
        if flipped {
            uses.reverse();
        }
        b.add_loop(f, true, uses).unwrap();
    }
    b
}

/// Checks that each knot's point of `c` at `like`'s level is its control
/// point, exactly, with no error.
fn knots_exact<S: Scalar>(c: &Curve3D, like: &S, bits: impl Fn(&S) -> Option<f64>) {
    for (k, &t) in c.knots().iter().enumerate() {
        let x = c.eval_at(t, like).unwrap();
        let p = c.knot_point(k).unwrap();
        for m in 0..3 {
            assert_eq!(
                bits(&x[m]).map(f64::to_bits),
                Some(p[m].to_bits()),
                "knot {k}"
            );
        }
    }
}

#[test]
fn weighted_ends_are_their_vertices_bit_for_bit() {
    let b = weighted_half_disc();
    assert_eq!(b.check_ribbons(), Ok(()));
    let zero = Q::from(0u32);
    for e in 0..b.edge_count() {
        let e = EdgeId(e as u32);
        let c = b.curve(e);
        let [v0, v1] = b.ends(e);
        let [t0, t1] = b.t_range(e);
        // The stored curve itself ends at its vertices, exactly: in
        // rationals, the span's point at its ends.
        for (t, v) in [(t0, v0), (t1, v1)] {
            let k = c.knots().iter().position(|&k| k == t).unwrap();
            let x = match c.span_at(k, &zero) {
                Some(h) => h.eval(&Q::from(0u32)),
                None => c.span_at(k - 1, &zero).unwrap().eval(&Q::from(1u32)),
            };
            assert_eq!(x, b.pos(v).map(q), "{e:?}");
        }
        // Inner knots too, from both spans.
        for k in 1..c.spans().len() {
            let p = c.knot_point(k).unwrap().map(q);
            assert_eq!(c.span_at(k - 1, &zero).unwrap().eval(&Q::from(1u32)), p);
            assert_eq!(c.span_at(k, &zero).unwrap().eval(&Q::from(0u32)), p);
        }
        // At every level the point at a knot is the control point, with
        // no error.
        let exact = |x: &Bounded<f64>| (x.err() == 0.0).then_some(x.v);
        knots_exact(c, &Bounded::exact(0.0), exact);
        let dd = |x: &Bounded<Dd>| (x.err() == 0.0 && x.v.lo() == 0.0).then_some(x.v.hi());
        knots_exact(c, &Bounded::exact(Dd::from_f64(0.0)), dd);
        let big = Bounded::exact(Big::from_f64(0.0, Width::MIN));
        knots_exact(c, &big, |x: &Bounded<Big>| {
            (x.err() == 0.0).then(|| x.v.to_f64())
        });
    }
    // Weights other than powers of two at the ends: projecting the
    // products `w·x`, as a homogeneous store would, misses the vertex.
    let missed = (0..b.edge_count()).any(|e| {
        let c = b.curve(EdgeId(e as u32));
        c.spans().iter().any(|s| {
            [s[0], s[2]]
                .iter()
                .any(|p| (0..3).any(|m| (p[m] * p[3]) / p[3] != p[m]))
        })
    });
    assert!(missed);
    assert_bounds_hold(&b, 8);
    // Exact pairs: the bounds are the first level's rounding on the
    // plane's one tile, 4 km across, scaled up by the spread of the
    // weights (about 2·10⁻⁴ mm for the quarter with end weights 3 and ⅓,
    // below 10⁻⁸ mm for the rest).
    for c in 0..b.coedge_count() {
        let c = CoedgeId(c as u32);
        let most = if b.edge(c) == EdgeId(1) { 1e-3 } else { 1e-8 };
        assert!(b.seam_bound(c) < most, "{c:?}: {}", b.seam_bound(c));
    }
}

#[test]
fn weight_one_spans_are_exact_at_every_level() {
    let line = Curve3D::new(
        vec![line([0.1, -0.2, 1e3 / 3.0], [7.0, 1.0 / 3.0, -2.5])],
        vec![0.0, 1.0],
    )
    .unwrap();
    let exact =
        |c: &[Poly<Bounded<f64>>]| c.iter().all(|p| p.coefs().iter().all(|x| x.err() == 0.0));
    let h = line.span(0).unwrap();
    assert!(exact(h.coords()) && exact(std::slice::from_ref(h.weights())));
    let h = line.span_at(0, &Bounded::exact(Dd::from_f64(0.0))).unwrap();
    assert!(
        h.coords()
            .iter()
            .chain([h.weights()])
            .all(|p| p.coefs().iter().all(|x| x.err() == 0.0))
    );
    let h = line
        .span_at(0, &Bounded::exact(Big::from_f64(0.0, Width::MIN)))
        .unwrap();
    assert!(
        h.coords()
            .iter()
            .chain([h.weights()])
            .all(|p| p.coefs().iter().all(|x| x.err() == 0.0))
    );
    // A weight that isn't 1 rounds its products at the first level, with
    // the rounding carried; from double-double up they are exact.
    let w = 1.0 / 3.0;
    let c = Curve3D::new(
        vec![[pt([0.1, 0.0, 0.0], w), pt([0.0; 3], 1.0), pt([1.0; 3], 1.0)]],
        vec![0.0, 1.0],
    )
    .unwrap();
    let x = c.span(0).unwrap().coords()[0].coefs()[0];
    assert!(x.err() > 0.0);
    assert!((q(x.v) - q(0.1) * q(w)).abs() <= q(x.err()));
    let x = c
        .span_at(0, &Bounded::exact(Dd::from_f64(0.0)))
        .unwrap()
        .coords()[0]
        .coefs()[0];
    assert_eq!(x.err(), 0.0);
    assert_eq!(q(x.v.hi()) + q(x.v.lo()), q(0.1) * q(w));
}

/// A full circle of radius `r` about the origin in the plane `z = 0`:
/// four quarters from `(r, 0, 0)` round, over `[0, 4]`.
fn circle(r: f64) -> Curve3D {
    let p = |x: f64, y: f64| [x * r, y * r, 0.0];
    let quarters = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)];
    let spans = quarters
        .windows(2)
        .map(|w| {
            let ((ax, ay), (bx, by)) = (w[0], w[1]);
            [
                pt(p(ax, ay), 1.0),
                pt(p(ax + bx, ay + by), ROOT_HALF),
                pt(p(bx, by), 1.0),
            ]
        })
        .collect();
    Curve3D::new(spans, vec![0.0, 1.0, 2.0, 3.0, 4.0]).unwrap()
}

#[test]
fn closed_edges_make_one_coedge_loops() {
    // A plate: the plane z = 0 on both sides, an outer circle and a
    // hole, each one closed edge at one vertex.
    let mut b = Brep::new();
    let lat = Lattice::new(PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap().into()).unwrap();
    let s = b
        .add_surface(Surface::new(lat.clone(), lat.half(0).unwrap()).unwrap())
        .unwrap();
    let mut edges = Vec::new();
    for r in [R, R / 4.0] {
        let c = circle(r);
        let v = b.add_vert(c.knot_point(0).unwrap()).unwrap();
        let id = b.add_curve(c).unwrap();
        edges.push(b.add_edge(id, [0.0, 4.0], v, v).unwrap());
    }
    let (top, bottom) = (b.add_face(s, false).unwrap(), b.add_face(s, true).unwrap());
    for (f, outer_reversed) in [(top, false), (bottom, true)] {
        for (k, &e) in edges.iter().enumerate() {
            let p = on_plane(&lat, b.curve(e));
            // The hole runs the other way.
            let reversed = outer_reversed != (k == 1);
            b.add_loop(f, k == 0, vec![(e, reversed, p)]).unwrap();
        }
    }
    assert_eq!(b.check_ribbons(), Ok(()));
    for f in [top, bottom] {
        let loops: Vec<_> = b.loops(f).collect();
        assert_eq!(loops.len(), 2);
        assert!(b.is_outer(loops[0]) && !b.is_outer(loops[1]));
        for l in loops {
            let c: Vec<_> = b.loop_coedges(l).collect();
            assert_eq!(c.len(), 1);
            assert_eq!((b.next(c[0]), b.prev(c[0])), (c[0], c[0]));
            assert_eq!(b.pair(b.pair(c[0])), c[0]);
            assert_ne!(b.face(b.pair(c[0])), f);
        }
    }
    // A third use of a closed edge, and an open edge alone in a loop.
    let p = on_plane(&lat, b.curve(edges[0]));
    assert_eq!(
        b.add_loop(top, false, vec![(edges[0], true, p)]),
        Err(StoreError::Coedge {
            index: 0,
            why: CoedgeError::Third
        })
    );
    let (v0, v1) = (
        b.add_vert([0.0; 3]).unwrap(),
        b.add_vert([1.0, 0.0, 0.0]).unwrap(),
    );
    let c = Curve3D::new(vec![line([0.0; 3], [1.0, 0.0, 0.0])], vec![0.0, 1.0]).unwrap();
    let id = b.add_curve(c).unwrap();
    let e = b.add_edge(id, [0.0, 1.0], v0, v1).unwrap();
    let p = on_plane(&lat, b.curve(e));
    let l = b.add_loop(top, false, vec![(e, false, p)]).unwrap();
    let c = b.loop_coedges(l).next().unwrap();
    assert_eq!(b.check_ribbons(), Err(RibbonFault::Corner(c)));
}

#[test]
fn add_loop_is_atomic_on_every_error() {
    let mut f = half_cylinder(cylinder_pcurves());
    let b = &mut f.brep;
    // A fresh edge on the first arc's curve, unused.
    let curve = b.edges.curve[f.edges[0].0 as usize];
    let e = b
        .add_edge(curve, [0.0, 2.0], f.verts[0], f.verts[1])
        .unwrap();
    let [a0, ..] = cylinder_pcurves();
    let wide = a0.perturbed(&[[1e-3, 0.0], [0.0, 0.0]]).unwrap();
    let off = a0.perturbed(&[[0.0, 0.0], [0.0, -0.6]]).unwrap();
    let face = f.cylinder;
    let before = format!("{b:?}");
    type Uses = Vec<(EdgeId, bool, PCurve)>;
    let cases: Vec<(FaceId, Uses)> = vec![
        // Each fails at its last use, after a use that raises tolerances.
        (
            face,
            vec![(e, false, wide.clone()), (EdgeId::NONE, true, a0.clone())],
        ),
        (
            face,
            vec![(e, false, wide.clone()), (EdgeId(999), true, a0.clone())],
        ),
        (FaceId::NONE, vec![(e, false, wide.clone())]),
        (
            face,
            vec![(e, false, wide.clone()), (f.edges[1], false, a0.clone())],
        ),
        (face, vec![(e, false, wide.clone()), (e, false, a0.clone())]),
        (
            face,
            vec![
                (e, false, wide.clone()),
                (e, true, a0.clone()),
                (e, true, a0.clone()),
            ],
        ),
        (face, vec![(e, false, wide.clone()), (e, true, off)]),
        (face, vec![]),
    ];
    for (k, (face, uses)) in cases.into_iter().enumerate() {
        assert!(b.add_loop(face, false, uses).is_err(), "case {k}");
        assert_eq!(format!("{b:?}"), before, "case {k}");
    }
}
