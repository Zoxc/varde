//! Mass properties of hand-built primitive stores (a box, a box with a
//! cubic void, a cylinder, a sphere) against their closed forms, the
//! enclosures holding the exact values to 1e-12; the sheet, empty-solid
//! and budget outcomes.
//!
//! The stores hold what the measure reads (faces, loops, coedges with
//! their p-curves and seam bounds, edges with their curves): each coedge
//! has its own edge here, as the measure reads no pairing.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use varde_decide::bezier::Dir;
use varde_decide::num::storage::OpStart;

use super::*;
use crate::geom::certify::seam_bound_alone;
use crate::geom::curve::{Curve3D, Span3};
use crate::geom::form::{CylinderKey, PlaneKey, SphereKey};
use crate::geom::forms::{cyl, plane, sphere};
use crate::geom::lattice::CELL;
use crate::geom::pcurve::PCurve;
use crate::infra::budget::MAX_WORK;
use crate::infra::ops::{FeatureId, InstanceId, Status, Tolerance};
use crate::topo::brep::{
    AttrSetId, CoedgeRow, Continuity, EdgeKind, EdgeRow, ExactRows, FaceKey, FacePart, FaceRow,
    LoopId, LoopRow, Origin, ShellId, SurfaceRow, VertKind, VertRow,
};

fn status() -> Status {
    let start = OpStart {
        max_coord: 1.0,
        rho_op: 0.0,
    };
    Status::new(Tolerance::new(1e-3).expect("in range"), &start)
}

/// A trim: the edge's curve and the coedge's p-curve over the same
/// knots, and whether the coedge runs against them.
struct TrimIn {
    curve: Curve3D,
    pcurve: PCurve,
    reversed: bool,
}

fn trim(curve: Curve3D, pcurve: PCurve) -> TrimIn {
    TrimIn {
        curve,
        pcurve,
        reversed: false,
    }
}

/// Adds a face of `surface` with `loops`, each coedge on its own edge,
/// its seam bound certified.
fn push_face(b: &mut Brep, surface: Surface, flipped: bool, loops: Vec<Vec<TrimIn>>) {
    let face = FaceId(b.face_count() as u32);
    let surf = b
        .push_surface(SurfaceRow {
            surface: surface.clone(),
            fit: None,
            exact_rows: ExactRows::default(),
        })
        .unwrap();
    let first_loop = LoopId(b.loop_count() as u32);
    let n_loops = loops.len();
    for (li, lp_trims) in loops.into_iter().enumerate() {
        let lp = LoopId(first_loop.0 + li as u32);
        let first = b.coedge_count() as u32;
        let n = lp_trims.len() as u32;
        for (k, t) in lp_trims.into_iter().enumerate() {
            let seam = seam_bound_alone(&surface, &t.curve, &t.pcurve, 1e-12, MAX_WORK)
                .expect("a seam bound")
                .bound;
            assert!(seam < 1e-9, "a trim off its surface: {seam}");
            let spans = t.curve.spans();
            let a = spans[0][0];
            let z = spans[spans.len() - 1][2];
            let [t0, t1] = [t.curve.knots()[0], *t.curve.knots().last().unwrap()];
            let vert = |b: &mut Brep, p: [f64; 4]| {
                b.push_vert(VertRow {
                    pos: [p[0], p[1], p[2]],
                    tol: 0.0,
                    kind: VertKind::Regular,
                    coedge: CoedgeId::NONE,
                    tag: None,
                    attrs: AttrSetId::NONE,
                })
                .unwrap()
            };
            let v0 = vert(b, a);
            let v1 = if a == z { v0 } else { vert(b, z) };
            let curve = b.push_curve(t.curve).unwrap();
            let edge = b
                .push_edge(EdgeRow {
                    curve,
                    t_range: [t0, t1],
                    v_start: v0,
                    v_end: v1,
                    kind: EdgeKind::Regular,
                    cont: Continuity::default(),
                    origin: Origin::default(),
                    tol: seam,
                    dev: 0.0,
                    embedded_join: false,
                    coedge: CoedgeId(first + k as u32),
                    tag: None,
                    attrs: AttrSetId::NONE,
                })
                .unwrap();
            let pcurve = b.push_pcurve(t.pcurve).unwrap();
            let k = k as u32;
            b.push_coedge(CoedgeRow {
                edge,
                reversed: t.reversed,
                face,
                lp,
                next: CoedgeId(first + (k + 1) % n),
                prev: CoedgeId(first + (k + n - 1) % n),
                pair: CoedgeId::NONE,
                pcurve,
                seam_bound: seam,
            })
            .unwrap();
        }
        b.push_loop(LoopRow {
            face,
            first_coedge: CoedgeId(first),
            next_loop: if li + 1 < n_loops {
                LoopId(lp.0 + 1)
            } else {
                LoopId::NONE
            },
            is_outer: li == 0,
        })
        .unwrap();
    }
    b.push_face(FaceRow {
        surface: surf,
        flipped,
        shells: [ShellId::NONE; 2],
        first_loop,
        key: FaceKey {
            feature: FeatureId(1),
            part: FacePart::Primitive(face.0),
            instance: InstanceId(0),
        },
        attrs: AttrSetId::NONE,
    })
    .unwrap();
}

fn line(a: [f64; 3], b: [f64; 3]) -> Curve3D {
    let m = [0, 1, 2].map(|k| (a[k] + b[k]) * 0.5);
    let pt = |p: [f64; 3]| [p[0], p[1], p[2], 1.0];
    Curve3D::new(vec![[pt(a), pt(m), pt(b)]], vec![0.0, 1.0]).expect("a line")
}

/// A plane's UV of `x`: `(x − o)·axis / CELL + ½` per axis.
fn plane_uv(s: &Surface, x: [f64; 3]) -> [f64; 2] {
    let lat = s.lattice();
    let (o, [u, v, _]) = (lat.origin(), lat.axes());
    let d = [x[0] - o[0], x[1] - o[1], x[2] - o[2]];
    let along = |a: [f64; 3]| (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5;
    [along(u), along(v)]
}

/// A curve's p-curve on a plane: each control point mapped, its weight
/// kept, as Q spans.
fn on_plane(s: &Surface, c: &Curve3D) -> PCurve {
    let spans = c
        .spans()
        .iter()
        .map(|sp| {
            sp.map(|h| {
                let [u, v] = plane_uv(s, [h[0], h[1], h[2]]);
                [u, v, h[3]]
            })
        })
        .collect();
    PCurve::new(spans, c.knots().to_vec()).expect("a p-curve")
}

/// The planar face with outward normal `n` round the polygon `corners`
/// (counter-clockwise about `n`): flipped where the plane's surface
/// faces the other way (a plane's frame doesn't follow the key's sense).
fn polygon(b: &mut Brep, n: [f64; 3], corners: &[[f64; 3]]) {
    let s = plane::surface(PlaneKey::new(n, corners[0]).unwrap()).unwrap();
    let flipped = facing_away(&s, n);
    let k = corners.len();
    let trims = (0..k)
        .map(|i| {
            let (a, c) = (corners[i], corners[(i + 1) % k]);
            let (ua, uc) = (plane_uv(&s, a), plane_uv(&s, c));
            let p = PCurve::from_spans(
                vec![PSpan::Linear([[ua[0], ua[1], 1.0], [uc[0], uc[1], 1.0]])],
                vec![0.0, 1.0],
            )
            .unwrap();
            trim(line(a, c), p)
        })
        .collect();
    push_face(b, s, flipped, vec![trims]);
}

/// Whether a plane's surface normal is against `n`.
fn facing_away(s: &Surface, n: [f64; 3]) -> bool {
    let a = s.lattice().axes()[2];
    a[0] * n[0] + a[1] * n[1] + a[2] * n[2] < 0.0
}

/// The six faces of the box `[lo, hi]`, outward unless `inward` (a void's
/// faces: outward normals into the box, surfaces kept outward, flipped).
fn box_faces(b: &mut Brep, lo: [f64; 3], hi: [f64; 3], inward: bool) {
    let c = |x: usize, y: usize, z: usize| {
        [
            if x == 0 { lo[0] } else { hi[0] },
            if y == 0 { lo[1] } else { hi[1] },
            if z == 0 { lo[2] } else { hi[2] },
        ]
    };
    // Each face counter-clockwise about its outward normal.
    let faces: [([f64; 3], [[f64; 3]; 4]); 6] = [
        (
            [0.0, 0.0, -1.0],
            [c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0)],
        ),
        (
            [0.0, 0.0, 1.0],
            [c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)],
        ),
        (
            [0.0, -1.0, 0.0],
            [c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1)],
        ),
        (
            [0.0, 1.0, 0.0],
            [c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0)],
        ),
        (
            [-1.0, 0.0, 0.0],
            [c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0)],
        ),
        (
            [1.0, 0.0, 0.0],
            [c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1)],
        ),
    ];
    for (n, q) in faces {
        if inward {
            // The void's boundary faces the material outside it.
            let mut q = q;
            q.reverse();
            polygon(b, n.map(|x| -x), &q);
        } else {
            polygon(b, n, &q);
        }
    }
}

fn solid() -> Brep {
    Brep::new(BodyKind::Solid, status())
}

/// The direction `k` quarter turns from `e1` towards `e2`.
fn dir(e: [[f64; 3]; 3], k: i64) -> [f64; 3] {
    match k.rem_euclid(4) {
        0 => e[0],
        1 => e[1],
        2 => e[0].map(|x| -x),
        _ => e[1].map(|x| -x),
    }
}

/// The circle of radius `r` about `c` in the frame `e` as quarter spans
/// `from..to` (counter-clockwise about `e₃`), knots `from..=to`.
fn arcs(c: [f64; 3], e: [[f64; 3]; 3], r: f64, from: i64, to: i64) -> Curve3D {
    let at = |d: [f64; 3], w: f64| [c[0] + r * d[0], c[1] + r * d[1], c[2] + r * d[2], w];
    let spans: Vec<Span3> = (from..to)
        .map(|k| {
            let (a, b) = (dir(e, k), dir(e, k + 1));
            let m = [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
            [at(a, 1.0), at(m, FRAC_1_SQRT_2), at(b, 1.0)]
        })
        .collect();
    let knots = (from..=to).map(|k| k as f64).collect();
    Curve3D::new(spans, knots).unwrap()
}

/// An iso p-curve: coordinate `fixed` at `at`, the other running through
/// the integers `from` to `to` (either way), one span per tile.
fn iso(fixed: Dir, at: f64, from: i64, to: i64) -> PCurve {
    let step = if to > from { 1 } else { -1 };
    let mut spans = Vec::new();
    let mut k = from;
    while k != to {
        spans.push(PSpan::Iso {
            fixed,
            at,
            m: [[k as f64, 1.0], [(k + step) as f64, 1.0]],
        });
        k += step;
    }
    let knots = (0..=spans.len()).map(|i| i as f64).collect();
    PCurve::from_spans(spans, knots).unwrap()
}

/// `c` with its knots `0..n` and, if `rev`, run the other way.
fn renumbered(c: &Curve3D, rev: bool) -> Curve3D {
    let mut spans = c.spans().to_vec();
    if rev {
        spans.reverse();
        for s in &mut spans {
            s.reverse();
        }
    }
    let knots = (0..=spans.len()).map(|i| i as f64).collect();
    Curve3D::new(spans, knots).unwrap()
}

/// The cylinder of radius `r` about the `z` axis from `z = 0` to `z = h`
/// (`h/CELL` exact): two 180° walls and two caps.
fn cylinder(r: f64, h: f64) -> Brep {
    let mut b = solid();
    let key = CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], r).unwrap();
    let (v0, v1) = (0.5, 0.5 + h / CELL);
    for first in [0, 2] {
        let s = cyl::cylinder(key, first).unwrap();
        let e = s.lattice().axes();
        let o = s.lattice().origin();
        let (u0, u1) = (first, first + 2);
        let ring = |z: f64| arcs([o[0], o[1], o[2] + z], e, r, u0, u1);
        let seam = |u: i64, up: bool| {
            let d = dir(e, u);
            let p = |z: f64| [o[0] + r * d[0], o[1] + r * d[1], o[2] + z];
            if up {
                line(p(0.0), p(h))
            } else {
                line(p(h), p(0.0))
            }
        };
        let trims = vec![
            trim(renumbered(&ring(0.0), false), iso(Dir::V, v0, u0, u1)),
            trim(seam(u1, true), iso_line(Dir::U, u1 as f64, v0, v1)),
            trim(renumbered(&ring(h), true), iso(Dir::V, v1, u1, u0)),
            trim(seam(u0, false), iso_line(Dir::U, u0 as f64, v1, v0)),
        ];
        push_face(&mut b, s, false, vec![trims]);
    }
    for (z, n) in [(h, 1.0), (0.0, -1.0)] {
        let s = plane::surface(PlaneKey::new([0.0, 0.0, n], [0.0, 0.0, z]).unwrap()).unwrap();
        let e = cyl::cylinder(key, 0).unwrap().lattice().axes();
        let c = arcs([0.0, 0.0, z], e, r, 0, 4);
        let p = on_plane(&s, &c);
        // Counter-clockwise about +z, run against on the bottom cap,
        // whose outward normal is −z.
        let flipped = facing_away(&s, [0.0, 0.0, n]);
        let mut t = trim(c, p);
        t.reversed = n < 0.0;
        push_face(&mut b, s, flipped, vec![vec![t]]);
    }
    b
}

/// An iso p-curve of one span from `a` to `b` in the free coordinate.
fn iso_line(fixed: Dir, at: f64, a: f64, b: f64) -> PCurve {
    PCurve::from_spans(
        vec![PSpan::Iso {
            fixed,
            at,
            m: [[a, 1.0], [b, 1.0]],
        }],
        vec![0.0, 1.0],
    )
    .unwrap()
}

/// The sphere of radius `r` about `c`: two 180° faces pole to pole.
fn sphere_body(c: [f64; 3], r: f64) -> Brep {
    let mut b = solid();
    let key = SphereKey::new(c, r, [0.0, 0.0, 1.0]).unwrap();
    for first in [0, 2] {
        let s = sphere::sphere(key, first).unwrap();
        let (u0, u1) = (first, first + 2);
        let meridian = |u: i64| {
            let spans = vec![s.row(Dir::V, u, -1).unwrap(), s.row(Dir::V, u, 0).unwrap()];
            Curve3D::new(spans, vec![0.0, 1.0, 2.0]).unwrap()
        };
        let pole = |v: i64| {
            let p = s.row(Dir::U, v, u0).unwrap()[0];
            Curve3D::new(vec![[p; 3], [p; 3]], vec![0.0, 1.0, 2.0]).unwrap()
        };
        let trims = vec![
            trim(pole(-1), iso(Dir::V, -1.0, u0, u1)),
            trim(meridian(u1), iso(Dir::U, u1 as f64, -1, 1)),
            trim(pole(1), iso(Dir::V, 1.0, u1, u0)),
            trim(
                renumbered(&meridian(u0), true),
                iso(Dir::U, u0 as f64, 1, -1),
            ),
        ];
        push_face(&mut b, s, false, vec![trims]);
    }
    b
}

/// `x` holds `exact` and is within `rel` of it relative to `scale`.
#[track_caller]
fn assert_holds(x: B, exact: f64, scale: f64, rel: f64) {
    let d = (x.v - exact).abs();
    assert!(d <= x.err() + 1e-15 * scale, "{x:?} misses {exact}");
    assert!(
        x.err() <= rel * scale,
        "{x:?}: error above {rel} of {scale}"
    );
    assert!(d <= rel * scale, "{x:?} off {exact}");
}

#[test]
fn a_box_has_its_volume_area_and_centroid() {
    let (lo, hi) = ([1.0, -2.0, 0.5], [4.0, 3.0, 7.25]);
    let mut b = solid();
    box_faces(&mut b, lo, hi, false);
    let m = mass_props(&b, &Budget::DEFAULT).unwrap();
    let d = [0, 1, 2].map(|k| hi[k] - lo[k]);
    let v = d[0] * d[1] * d[2];
    let a = 2.0 * (d[0] * d[1] + d[1] * d[2] + d[2] * d[0]);
    assert_holds(m.volume, v, v, 1e-12);
    assert_holds(m.area, a, a, 1e-12);
    let c = m.centroid.unwrap();
    for k in 0..3 {
        assert_holds(c[k], (lo[k] + hi[k]) * 0.5, 10.0, 1e-12);
    }
}

#[test]
fn a_void_takes_its_volume_and_adds_its_area() {
    let mut b = solid();
    box_faces(&mut b, [0.0; 3], [10.0; 3], false);
    box_faces(&mut b, [2.0, 3.0, 4.0], [5.0, 6.0, 7.0], true);
    let m = mass_props(&b, &Budget::DEFAULT).unwrap();
    assert_holds(m.volume, 1000.0 - 27.0, 1000.0, 1e-12);
    assert_holds(m.area, 600.0 + 54.0, 654.0, 1e-12);
    // The moments: the cube's minus the void's, about the origin.
    let c = m.centroid.unwrap();
    let mid = [3.5, 4.5, 5.5];
    for k in 0..3 {
        let exact = (1000.0 * 5.0 - 27.0 * mid[k]) / 973.0;
        assert_holds(c[k], exact, 10.0, 1e-12);
    }
}

#[test]
fn a_cylinder_has_its_volume_area_and_centroid() {
    let (r, h) = (3.0, 8.0);
    let b = cylinder(r, h);
    let m = mass_props(&b, &Budget::DEFAULT).unwrap();
    let v = PI * r * r * h;
    let a = 2.0 * PI * r * (r + h);
    assert_holds(m.volume, v, v, 1e-12);
    assert_holds(m.area, a, a, 1e-12);
    let c = m.centroid.unwrap();
    for (k, exact) in [0.0, 0.0, h * 0.5].into_iter().enumerate() {
        assert_holds(c[k], exact, h, 1e-12);
    }
}

#[test]
fn a_sphere_has_its_volume_area_and_centroid() {
    let (c0, r) = ([10.0, -20.0, 5.0], 7.0);
    let b = sphere_body(c0, r);
    let m = mass_props(&b, &Budget::DEFAULT).unwrap();
    let v = 4.0 / 3.0 * PI * r * r * r;
    let a = 4.0 * PI * r * r;
    assert_holds(m.volume, v, v, 1e-12);
    assert_holds(m.area, a, a, 1e-12);
    let c = m.centroid.unwrap();
    for k in 0..3 {
        assert_holds(c[k], c0[k], r, 1e-12);
    }
}

#[test]
fn a_sheet_has_an_area_but_no_volume() {
    let mut b = Brep::new(BodyKind::Sheet, status());
    let q = [
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [2.0, 3.0, 0.0],
        [0.0, 3.0, 0.0],
    ];
    polygon(&mut b, [0.0, 0.0, 1.0], &q);
    assert_eq!(
        mass_props(&b, &Budget::DEFAULT),
        Err(KernelError::Invalid(Invalid::NotClosed))
    );
    assert_holds(area(&b, &Budget::DEFAULT).unwrap(), 6.0, 6.0, 1e-12);
}

#[test]
fn the_empty_solid_has_nothing() {
    let m = mass_props(&solid(), &Budget::DEFAULT).unwrap();
    for x in [m.volume, m.area] {
        assert_eq!(x.v, 0.0);
        assert!(x.err() < 1e-300);
    }
    assert_eq!(m.centroid, None);
}

#[test]
fn a_small_budget_runs_out() {
    let b = cylinder(3.0, 8.0);
    assert!(matches!(
        mass_props(&b, &Budget::new(100)),
        Err(KernelError::OutOfBudget { .. })
    ));
}

#[test]
fn a_wider_ribbon_widens_the_bounds() {
    let mut b = solid();
    box_faces(&mut b, [0.0; 3], [1.0; 3], false);
    let tight = mass_props(&b, &Budget::DEFAULT).unwrap();
    b.coedges.seam_bound[0] = 1e-6;
    let wide = mass_props(&b, &Budget::DEFAULT).unwrap();
    assert_eq!(wide.volume.v, tight.volume.v);
    // A unit edge on both sides of a 1e-6 ribbon, at most √3 from the
    // centre: the flux at most about 1e-6·√3/3.
    assert!(wide.volume.err() > 1e-7 && wide.volume.err() < 1e-6);
    assert!(wide.area.err() >= 1e-6 && wide.area.err() < 2e-6);
}

#[test]
fn a_store_with_a_span_off_its_tiles_is_refused() {
    let mut b = solid();
    box_faces(&mut b, [0.0; 3], [1.0; 3], false);
    // The first face's first p-curve moved past its plane's one tile.
    b.pcurves[0] = PCurve::from_spans(
        vec![PSpan::Linear([[5.0, 5.0, 1.0], [5.5, 5.0, 1.0]])],
        vec![0.0, 1.0],
    )
    .unwrap();
    assert_eq!(
        mass_props(&b, &Budget::DEFAULT),
        Err(KernelError::Invalid(Invalid::Check { inv: 2 }))
    );
}
